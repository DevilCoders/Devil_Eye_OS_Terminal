//! Typed ownership and lifecycle contracts between application subsystems.
//!
//! These interfaces describe boundaries only. Concrete Linux PTY, terminal
//! parser, renderer, configuration persistence, and DE-Shell adapters are
//! implemented by their dedicated tasks.

use std::num::{NonZeroU64, NonZeroUsize};

use de_runtime::ProgramSpec;
use de_terminal_core::TerminalSize;

use crate::lifecycle::{ApplicationService, ServiceError};

/// Opaque identifier for one application-owned terminal session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(NonZeroU64);

impl SessionId {
    /// Creates a session identifier. Zero is reserved as invalid.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the numeric identifier.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Kind of shell/program hosted by a terminal session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    /// Launch an external shell or program through the platform runtime.
    ExternalProgram,
    /// Host the independent DE-Shell implementation.
    DeShell,
}

/// Immutable inputs required to create a terminal session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    kind: SessionKind,
    program: ProgramSpec,
    terminal_size: TerminalSize,
}

impl SessionSpec {
    /// Creates a session specification from validated subsystem values.
    pub fn new(
        kind: SessionKind,
        program: ProgramSpec,
        terminal_size: TerminalSize,
    ) -> Self {
        Self {
            kind,
            program,
            terminal_size,
        }
    }

    /// Requested session backend.
    pub const fn kind(&self) -> SessionKind {
        self.kind
    }

    /// Program specification; executable and arguments remain separate.
    pub fn program(&self) -> &ProgramSpec {
        &self.program
    }

    /// Initial terminal dimensions.
    pub const fn terminal_size(&self) -> TerminalSize {
        self.terminal_size
    }
}

/// Observable lifecycle state of one terminal session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLifecycleState {
    Created,
    Starting,
    Running,
    Exited,
    Stopping,
    Closed,
    Failed,
}

impl SessionLifecycleState {
    /// Returns whether a transition is permitted by the session state machine.
    pub const fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Created => matches!(next, Self::Starting),
            Self::Starting => matches!(next, Self::Running | Self::Stopping | Self::Failed),
            Self::Running => matches!(next, Self::Exited | Self::Stopping | Self::Failed),
            Self::Exited | Self::Failed => matches!(next, Self::Stopping | Self::Closed),
            Self::Stopping => matches!(next, Self::Closed),
            Self::Closed => false,
        }
    }
}

/// Enforces legal state transitions for a single session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLifecycle {
    id: SessionId,
    state: SessionLifecycleState,
}

impl SessionLifecycle {
    /// Creates a new session lifecycle in the Created state.
    pub const fn new(id: SessionId) -> Self {
        Self {
            id,
            state: SessionLifecycleState::Created,
        }
    }

    /// Session identity.
    pub const fn id(&self) -> SessionId {
        self.id
    }

    /// Current state.
    pub const fn state(&self) -> SessionLifecycleState {
        self.state
    }

    /// Applies a legal transition or returns a safe lifecycle error.
    pub fn transition(
        &mut self,
        next: SessionLifecycleState,
    ) -> Result<(), ServiceError> {
        if !self.state.can_transition_to(next) {
            return Err(ServiceError::new(
                "terminal session lifecycle transition is invalid",
            ));
        }

        self.state = next;
        Ok(())
    }
}

/// Result of a non-blocking process read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRead {
    /// The buffer contains this many newly read bytes. The count is non-zero.
    Data(NonZeroUsize),
    /// No bytes are available yet; the process remains active.
    WouldBlock,
    /// The process output stream has reached EOF.
    Eof,
}

/// Requested child-process termination behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationMode {
    /// Request graceful termination; the caller owns the timeout/escalation policy.
    Graceful,
    /// Force termination when graceful shutdown has timed out or is unavailable.
    Force,
}

/// Final state reported by the platform runtime for a child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExit {
    /// The process exited with an integer exit code.
    Exited { code: i32 },
    /// The process was terminated by a platform signal.
    Signaled { signal: i32 },
}

impl ProcessExit {
    /// Whether the process completed successfully.
    pub const fn success(self) -> bool {
        matches!(self, Self::Exited { code: 0 })
    }
}

/// Unique owned process handle returned by a runtime adapter.
///
/// The runtime implementation owns native process/PTY resources. A session
/// owns the returned handle and must release it through its lifecycle contract.
pub trait RuntimeProcess: Send {
    /// Writes input bytes. A partial count is valid; unwritten bytes remain
    /// the caller's responsibility.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, ServiceError>;

    /// Reads into a caller-owned buffer without blocking the application thread.
    fn read(&mut self, buffer: &mut [u8]) -> Result<ProcessRead, ServiceError>;

    /// Applies a non-zero terminal size to the process's PTY.
    fn resize(&mut self, size: TerminalSize) -> Result<(), ServiceError>;

    /// Requests graceful or forced termination.
    fn terminate(&mut self, mode: TerminationMode) -> Result<(), ServiceError>;

    /// Returns the exit result when available; `None` means still running.
    fn try_wait(&mut self) -> Result<Option<ProcessExit>, ServiceError>;
}

/// Application-owned process creation service.
pub trait RuntimeService: ApplicationService {
    /// Unique handle type owned by the requesting terminal session.
    type Process: RuntimeProcess;

    /// Spawns an executable using separate program/argument/environment values.
    fn spawn(
        &mut self,
        program: &ProgramSpec,
        terminal_size: TerminalSize,
    ) -> Result<Self::Process, ServiceError>;
}

/// Configuration load/save boundary.
pub trait ConfigurationService: ApplicationService {
    /// Loads and validates the application configuration snapshot.
    fn load(&mut self) -> Result<crate::AppConfig, ServiceError>;

    /// Persists a validated configuration snapshot.
    fn save(&mut self, config: &crate::AppConfig) -> Result<(), ServiceError>;
}

/// Immutable terminal frame shared from the terminal engine to the renderer.
pub trait TerminalFrame: Send + Sync {
    /// Dimensions represented by this frame.
    fn terminal_size(&self) -> TerminalSize;
}

/// Platform-independent terminal state and byte-stream processing service.
pub trait TerminalEngineService: ApplicationService {
    /// Concrete immutable frame type produced by this terminal engine.
    type Frame: TerminalFrame;

    /// Consumes arbitrary output bytes; parser state must survive chunk boundaries.
    fn consume_output(&mut self, bytes: &[u8]) -> Result<(), ServiceError>;

    /// Updates the terminal screen dimensions.
    fn resize(&mut self, size: TerminalSize) -> Result<(), ServiceError>;

    /// Produces an immutable frame for the renderer.
    fn snapshot(&self) -> Result<Self::Frame, ServiceError>;
}

/// Renderer contract for a particular terminal frame type.
pub trait RendererService<F>: ApplicationService
where
    F: TerminalFrame,
{
    /// Renders an immutable frame without mutating terminal-engine state.
    fn render(&mut self, frame: &F) -> Result<(), ServiceError>;
}

/// Events emitted by the DE-Shell integration boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    /// Bytes to be forwarded to the terminal engine.
    Output(Vec<u8>),
    /// A typed process request; the shell does not spawn processes directly.
    ExecutionRequest(ProgramSpec),
    /// The shell requests that its owning session begin shutdown.
    ExitRequested,
}

/// Rust/C DE-Shell integration contract.
pub trait ShellIntegrationService: ApplicationService {
    /// Delivers user input to the shell core.
    fn submit_input(&mut self, input: &[u8]) -> Result<(), ServiceError>;

    /// Polls one shell event without blocking the application thread.
    fn poll_event(&mut self) -> Result<Option<ShellEvent>, ServiceError>;
}

/// Public lifecycle and I/O contract for one terminal session.
pub trait TerminalSession: Send {
    /// Stable application-generated identity.
    fn id(&self) -> SessionId;

    /// Current lifecycle state.
    fn state(&self) -> SessionLifecycleState;

    /// Delivers terminal input bytes to the session's active backend.
    fn send_input(&mut self, input: &[u8]) -> Result<(), ServiceError>;

    /// Resizes the terminal model and its associated runtime transport.
    fn resize(&mut self, size: TerminalSize) -> Result<(), ServiceError>;

    /// Stops the session and releases its owned runtime/shell resources.
    fn shutdown(&mut self) -> Result<(), ServiceError>;
}

/// Application-owned registry for independent terminal sessions.
pub trait SessionManagerService: ApplicationService {
    /// Concrete session handle owned by this manager.
    type Session: TerminalSession;

    /// Creates a new independent session and returns its stable identity.
    fn create_session(&mut self, spec: SessionSpec) -> Result<SessionId, ServiceError>;

    /// Queries the current state without transferring session ownership.
    fn session_state(&self, id: SessionId) -> Result<SessionLifecycleState, ServiceError>;

    /// Closes one session. Other sessions remain unaffected.
    fn close_session(&mut self, id: SessionId) -> Result<(), ServiceError>;
}
