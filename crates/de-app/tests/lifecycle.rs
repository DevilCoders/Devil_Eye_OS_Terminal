use de_app::{
    Application, ApplicationService, ApplicationState, LifecycleError, ServiceError,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Recorder {
    name: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    fail_initialize: bool,
    shutdown_failures_remaining: usize,
}

impl Recorder {
    fn new(name: &'static str, events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            name,
            events,
            fail_initialize: false,
            shutdown_failures_remaining: 0,
        }
    }

    fn failing_initialize(mut self) -> Self {
        self.fail_initialize = true;
        self
    }

    fn fail_shutdown_times(mut self, count: usize) -> Self {
        self.shutdown_failures_remaining = count;
        self
    }
}

impl ApplicationService for Recorder {
    fn name(&self) -> &str {
        self.name
    }

    fn initialize(&mut self) -> Result<(), ServiceError> {
        self.events
            .lock()
            .expect("event log lock")
            .push(format!("start:{}", self.name));

        if self.fail_initialize {
            Err(ServiceError::new("injected initialization failure"))
        } else {
            Ok(())
        }
    }

    fn shutdown(&mut self) -> Result<(), ServiceError> {
        self.events
            .lock()
            .expect("event log lock")
            .push(format!("stop:{}", self.name));

        if self.shutdown_failures_remaining > 0 {
            self.shutdown_failures_remaining -= 1;
            Err(ServiceError::new("injected shutdown failure"))
        } else {
            Ok(())
        }
    }
}

fn event_log() -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(Vec::new()))
}

fn events(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().expect("event log lock").clone()
}

#[test]
fn initializes_in_registration_order_and_shuts_down_in_reverse_order() {
    let log = event_log();
    let mut app = Application::new();

    app.register_service(Recorder::new("config", Arc::clone(&log)))
        .expect("register config");
    app.register_service(Recorder::new("runtime", Arc::clone(&log)))
        .expect("register runtime");
    app.register_service(Recorder::new("terminal", Arc::clone(&log)))
        .expect("register terminal");

    assert_eq!(app.state(), ApplicationState::Created);
    app.start().expect("startup succeeds");
    assert_eq!(app.state(), ApplicationState::Running);

    app.shutdown().expect("shutdown succeeds");
    assert_eq!(app.state(), ApplicationState::Stopped);
    app.shutdown().expect("shutdown is idempotent");

    assert_eq!(
        events(&log),
        [
            "start:config",
            "start:runtime",
            "start:terminal",
            "stop:terminal",
            "stop:runtime",
            "stop:config",
        ]
    );
}

#[test]
fn failed_initialization_cleans_failing_service_and_prior_services() {
    let log = event_log();
    let mut app = Application::new();

    app.register_service(Recorder::new("config", Arc::clone(&log)))
        .expect("register config");
    app.register_service(Recorder::new("runtime", Arc::clone(&log)))
        .expect("register runtime");
    app.register_service(
        Recorder::new("terminal", Arc::clone(&log)).failing_initialize(),
    )
    .expect("register terminal");

    let error = app.start().expect_err("startup must fail");
    assert!(matches!(
        error,
        LifecycleError::StartupFailed {
            ref service,
            ref cleanup_failures,
            ..
        } if service == "terminal" && cleanup_failures.is_empty()
    ));
    assert_eq!(app.state(), ApplicationState::Failed);

    assert_eq!(
        events(&log),
        [
            "start:config",
            "start:runtime",
            "start:terminal",
            "stop:terminal",
            "stop:runtime",
            "stop:config",
        ]
    );

    app.shutdown()
        .expect("shutdown after successful rollback is safe");
    assert_eq!(app.state(), ApplicationState::Stopped);
}

#[test]
fn cleanup_continues_after_failure_and_failed_service_can_be_retried() {
    let log = event_log();
    let mut app = Application::new();

    app.register_service(Recorder::new("config", Arc::clone(&log)))
        .expect("register config");
    app.register_service(
        Recorder::new("runtime", Arc::clone(&log)).fail_shutdown_times(1),
    )
    .expect("register runtime");
    app.register_service(Recorder::new("terminal", Arc::clone(&log)))
        .expect("register terminal");

    app.start().expect("startup succeeds");

    let error = app.shutdown().expect_err("runtime cleanup fails once");
    assert!(matches!(
        error,
        LifecycleError::ShutdownFailed { ref failures }
            if failures.len() == 1 && failures[0].service == "runtime"
    ));
    assert_eq!(app.state(), ApplicationState::Failed);

    // The failed service remains owned and is the only service retried.
    app.shutdown().expect("second cleanup attempt succeeds");
    assert_eq!(app.state(), ApplicationState::Stopped);

    assert_eq!(
        events(&log),
        [
            "start:config",
            "start:runtime",
            "start:terminal",
            "stop:terminal",
            "stop:runtime",
            "stop:config",
            "stop:runtime",
        ]
    );
}

#[test]
fn rejects_duplicate_start_and_late_service_registration() {
    let log = event_log();
    let mut app = Application::new();
    app.register_service(Recorder::new("config", Arc::clone(&log)))
        .expect("register config");

    app.start().expect("startup succeeds");

    assert!(matches!(
        app.start(),
        Err(LifecycleError::InvalidState {
            operation: "start",
            state: ApplicationState::Running,
        })
    ));
    assert!(matches!(
        app.register_service(Recorder::new("late", Arc::clone(&log))),
        Err(LifecycleError::InvalidState {
            operation: "register a service with",
            state: ApplicationState::Running,
        })
    ));

    app.shutdown().expect("shutdown succeeds");
}

#[test]
fn empty_application_has_a_valid_lifecycle() {
    let mut app = Application::default();
    app.start().expect("empty startup succeeds");
    assert_eq!(app.state(), ApplicationState::Running);
    app.shutdown().expect("empty shutdown succeeds");
    assert_eq!(app.state(), ApplicationState::Stopped);
}
