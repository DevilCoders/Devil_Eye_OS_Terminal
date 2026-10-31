//! Top-level application configuration.
//!
//! The application crate owns subsystem composition and deterministic service
//! lifecycle, and typed diagnostics. GUI startup, rendering, and terminal-session
//! behavior are implemented by their dedicated tasks.

pub mod config;

pub use config::{AppConfig, ConfigError, ConfigValue, CURRENT_CONFIG_SCHEMA_VERSION};

pub mod service_interfaces;

pub use service_interfaces::{
    ConfigurationService, ProcessExit, ProcessRead, RendererService, RuntimeProcess,
    RuntimeService, SessionId, SessionKind, SessionLifecycle, SessionLifecycleState,
    SessionManagerService, SessionSpec, ShellEvent, ShellIntegrationService, TerminalEngineService,
    TerminalFrame, TerminalSession, TerminationMode,
};

pub mod lifecycle;

pub use lifecycle::{
    Application, ApplicationService, ApplicationState, LifecycleError, ServiceCleanupFailure,
    ServiceError,
};

pub mod diagnostics;

pub use diagnostics::{
    ApplicationError, ApplicationErrorCode, Diagnostic, DiagnosticBuffer, DiagnosticField,
    DiagnosticSeverity, ErrorCategory,
};

pub mod session_manager;

pub use session_manager::{ManagedSession, SessionManager, SessionManagerLimits};

#[cfg(target_os = "linux")]
pub mod linux_runtime;

#[cfg(target_os = "linux")]
pub use linux_runtime::{LinuxRuntimeProcess, LinuxRuntimeService};
