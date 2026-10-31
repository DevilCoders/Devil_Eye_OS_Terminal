//! Top-level application configuration.
//!
//! The application crate owns subsystem composition and deterministic service
//! lifecycle, and typed diagnostics. GUI startup, rendering, and terminal-session
//! behavior are implemented by their dedicated tasks.

use de_ffi::{DeAbiVersion, ABI_VERSION};
use de_runtime::ProgramSpec;
use de_terminal_core::TerminalSize;

/// Configuration shared by the application bootstrap and its subsystems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    initial_terminal_size: TerminalSize,
    initial_program: ProgramSpec,
}

impl AppConfig {
    /// Creates application configuration from validated subsystem values.
    pub fn new(initial_terminal_size: TerminalSize, initial_program: ProgramSpec) -> Self {
        Self {
            initial_terminal_size,
            initial_program,
        }
    }

    /// Initial terminal dimensions.
    pub const fn initial_terminal_size(&self) -> TerminalSize {
        self.initial_terminal_size
    }

    /// Initial program launch specification.
    pub fn initial_program(&self) -> &ProgramSpec {
        &self.initial_program
    }

    /// Rust/C ABI version expected by this application build.
    pub const fn abi_version(&self) -> DeAbiVersion {
        ABI_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;
    use de_ffi::ABI_VERSION;
    use de_runtime::ProgramSpec;
    use de_terminal_core::TerminalSize;
    use std::ffi::OsStr;

    #[test]
    fn composes_validated_subsystem_configuration() {
        let size = TerminalSize::new(100, 32).expect("valid terminal size");
        let program = ProgramSpec::new("/bin/sh").expect("valid program");
        let config = AppConfig::new(size, program);

        assert_eq!(config.initial_terminal_size(), size);
        assert_eq!(config.initial_program().program(), OsStr::new("/bin/sh"));
        assert_eq!(config.abi_version(), ABI_VERSION);
    }
}

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
