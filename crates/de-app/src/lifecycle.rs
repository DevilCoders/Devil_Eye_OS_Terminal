//! Deterministic application service startup and shutdown.
//!
//! Services are initialized in registration order and shut down in reverse
//! order. A service whose initialization fails is also asked to shut down,
//! because it may have acquired resources before returning the error.

use std::error::Error;
use std::fmt;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;

/// Observable state of the top-level application lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationState {
    Created,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

/// A service-level failure with a safe, caller-provided description.
///
/// Implementations must not include secrets or raw environment values in the
/// message. The diagnostics subsystem can wrap this value with richer context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    message: String,
}

impl ServiceError {
    /// Creates a service failure description.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the failure description.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ServiceError {}

/// A service that participates in application startup and shutdown.
///
/// `initialize` may acquire resources. If it returns an error, the
/// application immediately calls `shutdown` on that same service before
/// rolling back previously initialized services. Implementations should make
/// `shutdown` idempotent and release any partially acquired resources.
///
/// Shutdown may block while child processes or native resources are released.
/// Application coordinators must invoke it on a lifecycle worker, never on the
/// GUI/event-loop thread.
pub trait ApplicationService: Send {
    /// Stable diagnostic name for this service.
    fn name(&self) -> &str;

    /// Initializes the service.
    fn initialize(&mut self) -> Result<(), ServiceError>;

    /// Releases resources owned by the service.
    fn shutdown(&mut self) -> Result<(), ServiceError>;
}

/// A failure while releasing one service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCleanupFailure {
    /// Name of the service that could not be fully stopped.
    pub service: String,
    /// Service-provided failure description.
    pub cause: ServiceError,
}

/// Errors returned by application lifecycle operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    /// An operation is invalid in the current lifecycle state.
    InvalidState {
        operation: &'static str,
        state: ApplicationState,
    },
    /// A service failed during startup. All cleanup failures are retained.
    StartupFailed {
        service: String,
        cause: ServiceError,
        cleanup_failures: Vec<ServiceCleanupFailure>,
    },
    /// One or more services failed to stop. All other services were still
    /// given their shutdown opportunity.
    ShutdownFailed {
        failures: Vec<ServiceCleanupFailure>,
    },
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState { operation, state } => {
                write!(
                    formatter,
                    "cannot {operation} application in {state:?} state"
                )
            }
            Self::StartupFailed {
                service,
                cause,
                cleanup_failures,
            } => {
                write!(formatter, "service {service} failed to initialize: {cause}")?;
                if !cleanup_failures.is_empty() {
                    write!(
                        formatter,
                        "; {} service(s) also failed during rollback",
                        cleanup_failures.len()
                    )?;
                }
                Ok(())
            }
            Self::ShutdownFailed { failures } => {
                write!(
                    formatter,
                    "{} service(s) failed to shut down",
                    failures.len()
                )
            }
        }
    }
}

impl Error for LifecycleError {}

/// Owns application services and coordinates their lifecycle.
///
/// Register services before calling `start`. Startup is deterministic:
/// services initialize in registration order. Shutdown and rollback run in
/// reverse order. A failed cleanup is retained for a later `shutdown` retry.
pub struct Application {
    state: ApplicationState,
    services: Vec<Box<dyn ApplicationService>>,
    active_services: Vec<usize>,
}

impl Application {
    /// Creates an application with no registered services.
    pub fn new() -> Self {
        Self {
            state: ApplicationState::Created,
            services: Vec::new(),
            active_services: Vec::new(),
        }
    }

    /// Returns the current lifecycle state.
    pub const fn state(&self) -> ApplicationState {
        self.state
    }

    /// Registers a service before startup.
    pub fn register_service(
        &mut self,
        service: impl ApplicationService + 'static,
    ) -> Result<(), LifecycleError> {
        if self.state != ApplicationState::Created {
            return Err(LifecycleError::InvalidState {
                operation: "register a service with",
                state: self.state,
            });
        }

        self.services.push(Box::new(service));
        Ok(())
    }

    /// Initializes all registered services in registration order.
    ///
    /// On failure, the failing service and all previously initialized
    /// services are shut down in reverse order. Cleanup is best-effort: every
    /// active service receives a shutdown call even if another cleanup fails.
    pub fn start(&mut self) -> Result<(), LifecycleError> {
        if self.state != ApplicationState::Created {
            return Err(LifecycleError::InvalidState {
                operation: "start",
                state: self.state,
            });
        }

        self.state = ApplicationState::Starting;

        for index in 0..self.services.len() {
            match self.services[index].initialize() {
                Ok(()) => self.active_services.push(index),
                Err(cause) => {
                    let service = self.services[index].name().to_owned();
                    // Include the failing service: initialize may have
                    // partially acquired resources before returning Err.
                    self.active_services.push(index);
                    self.state = ApplicationState::Failed;
                    let cleanup_failures = self.rollback_active_services();

                    return Err(LifecycleError::StartupFailed {
                        service,
                        cause,
                        cleanup_failures,
                    });
                }
            }
        }

        self.state = ApplicationState::Running;
        Ok(())
    }

    /// Shuts down all active services in reverse initialization order.
    ///
    /// The operation is idempotent after a successful shutdown. Failed
    /// services remain owned by the application and can be retried by calling
    /// `shutdown` again.
    pub fn shutdown(&mut self) -> Result<(), LifecycleError> {
        match self.state {
            ApplicationState::Stopped => return Ok(()),
            ApplicationState::Created | ApplicationState::Running | ApplicationState::Failed => {}
            state => {
                return Err(LifecycleError::InvalidState {
                    operation: "shut down",
                    state,
                });
            }
        }

        self.state = ApplicationState::Stopping;
        let failures = self.rollback_active_services();

        if failures.is_empty() {
            self.state = ApplicationState::Stopped;
            Ok(())
        } else {
            self.state = ApplicationState::Failed;
            Err(LifecycleError::ShutdownFailed { failures })
        }
    }

    /// Consumes the application and performs service shutdown on a dedicated worker.
    ///
    /// GUI/event-loop callers must use this method rather than calling shutdown
    /// directly because service cleanup may wait for child processes and native
    /// resources. The returned handle can be polled with is_finished; join it
    /// only after completion.
    ///
    /// If the operating system cannot create the worker thread, the original
    /// application is returned with the I/O error so the caller can retry or
    /// arrange cleanup on another lifecycle worker. No service is dropped as
    /// a side effect of a failed worker spawn.
    pub fn shutdown_async(
        self,
    ) -> Result<thread::JoinHandle<Result<(), LifecycleError>>, (Self, io::Error)> {
        let application = Arc::new(Mutex::new(Some(self)));
        let worker_application = Arc::clone(&application);
        let result = thread::Builder::new()
            .name("de-application-shutdown".to_owned())
            .spawn(move || {
                let mut application = worker_application
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                    .expect("shutdown worker owns the application exactly once");
                application.shutdown()
            });

        match result {
            Ok(handle) => Ok(handle),
            Err(error) => {
                let application = match Arc::try_unwrap(application) {
                    Ok(application) => application,
                    Err(_) => unreachable!("failed thread spawn must drop its closure"),
                };
                let application = application
                    .into_inner()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .expect("application remains available when worker spawn fails");
                Err((application, error))
            }
        }
    }

    /// Attempts cleanup for every active service, preserving registration
    /// order among failures so that a later retry still runs in reverse order.
    fn rollback_active_services(&mut self) -> Vec<ServiceCleanupFailure> {
        let active = std::mem::take(&mut self.active_services);
        let mut remaining = Vec::new();
        let mut failures = Vec::new();

        for index in active.into_iter().rev() {
            match self.services[index].shutdown() {
                Ok(()) => {}
                Err(cause) => {
                    failures.push(ServiceCleanupFailure {
                        service: self.services[index].name().to_owned(),
                        cause,
                    });
                    remaining.push(index);
                }
            }
        }

        // `remaining` was collected in reverse registration order. Store it
        // in registration order so the next cleanup attempt reverses it again.
        remaining.reverse();
        self.active_services = remaining;
        failures
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}
