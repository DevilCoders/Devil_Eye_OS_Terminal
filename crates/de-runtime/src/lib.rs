//! Platform-independent process and I/O runtime contracts.
//!
//! This crate owns validated process launch specifications and the behavioral
//! interfaces implemented by platform adapters. It does not contain Linux,
//! Windows, or macOS system calls. The first concrete adapter is introduced
//! by a separate implementation task.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

/// Shared terminal dimensions and validation error used by the terminal core and PTY adapter.
pub use de_terminal_core::{TerminalSize, TerminalSizeError};

/// An owned process launch description, validated before it reaches a runtime backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramSpec {
    program: OsString,
    args: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    working_directory: Option<PathBuf>,
}

/// Errors detected while validating a process specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramSpecError {
    EmptyProgram,
    ProgramContainsNul,
    ArgumentContainsNul,
    EmptyEnvironmentKey,
    EnvironmentKeyContainsNul,
    EnvironmentKeyContainsEquals,
    EnvironmentValueContainsNul,
    WorkingDirectoryContainsNul,
}

impl fmt::Display for ProgramSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyProgram => "program name must not be empty",
            Self::ProgramContainsNul => "program name contains a NUL byte",
            Self::ArgumentContainsNul => "program argument contains a NUL byte",
            Self::EmptyEnvironmentKey => "environment key must not be empty",
            Self::EnvironmentKeyContainsNul => "environment key contains a NUL byte",
            Self::EnvironmentKeyContainsEquals => "environment key contains '='",
            Self::EnvironmentValueContainsNul => "environment value contains a NUL byte",
            Self::WorkingDirectoryContainsNul => "working directory contains a NUL byte",
        };

        formatter.write_str(message)
    }
}

impl Error for ProgramSpecError {}

impl ProgramSpec {
    /// Creates a process specification with a non-empty executable name.
    pub fn new(program: impl Into<OsString>) -> Result<Self, ProgramSpecError> {
        let program = program.into();
        if program.is_empty() {
            return Err(ProgramSpecError::EmptyProgram);
        }
        if contains_nul(program.as_os_str()) {
            return Err(ProgramSpecError::ProgramContainsNul);
        }

        Ok(Self {
            program,
            args: Vec::new(),
            environment: Vec::new(),
            working_directory: None,
        })
    }

    /// Appends one argument without invoking a shell or joining a command line.
    ///
    /// NUL validation is performed by ProgramSpec::validate and by
    /// ProcessSpawnRequest::new before the runtime acquires OS resources.
    pub fn with_arg(mut self, argument: impl Into<OsString>) -> Self {
        self.args.push(argument.into());
        self
    }

    /// Adds or replaces an environment variable in this launch specification.
    ///
    /// Validation of the key/value bytes is deferred until spawn-request
    /// construction so builder calls remain composable.
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let key = key.into();
        let value = value.into();

        if let Some((_, existing_value)) = self
            .environment
            .iter_mut()
            .find(|(existing_key, _)| existing_key == &key)
        {
            *existing_value = value;
        } else {
            self.environment.push((key, value));
        }

        self
    }

    /// Sets the requested working directory.
    pub fn with_working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// Validates all executable, argument, environment, and path values.
    pub fn validate(&self) -> Result<(), ProgramSpecError> {
        if self.program.is_empty() {
            return Err(ProgramSpecError::EmptyProgram);
        }
        if contains_nul(&self.program) {
            return Err(ProgramSpecError::ProgramContainsNul);
        }
        if self.args.iter().any(|argument| contains_nul(argument)) {
            return Err(ProgramSpecError::ArgumentContainsNul);
        }

        for (key, value) in &self.environment {
            if key.is_empty() {
                return Err(ProgramSpecError::EmptyEnvironmentKey);
            }
            if contains_nul(key) {
                return Err(ProgramSpecError::EnvironmentKeyContainsNul);
            }
            if key.as_encoded_bytes().contains(&b'=') {
                return Err(ProgramSpecError::EnvironmentKeyContainsEquals);
            }
            if contains_nul(value) {
                return Err(ProgramSpecError::EnvironmentValueContainsNul);
            }
        }

        if self
            .working_directory
            .as_ref()
            .is_some_and(|path| contains_nul(path.as_os_str()))
        {
            return Err(ProgramSpecError::WorkingDirectoryContainsNul);
        }

        Ok(())
    }

    /// Executable path or name.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Arguments in their original, unjoined form.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Environment overrides in insertion order.
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.environment
    }

    /// Optional working directory.
    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }
}

fn contains_nul(value: &OsStr) -> bool {
    value.as_encoded_bytes().contains(&0u8)
}

/// How one standard process stream is attached to the parent application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamAttachment {
    /// Create a pipe owned by the runtime process handle.
    Pipe,
    /// Connect the stream to the platform null device.
    Null,
    /// Inherit the corresponding stream from the parent process.
    Inherit,
}

/// Explicit stdin/stdout/stderr attachment for a non-PTY child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StdioConfig {
    stdin: StreamAttachment,
    stdout: StreamAttachment,
    stderr: StreamAttachment,
}

impl StdioConfig {
    pub const fn new(
        stdin: StreamAttachment,
        stdout: StreamAttachment,
        stderr: StreamAttachment,
    ) -> Self {
        Self {
            stdin,
            stdout,
            stderr,
        }
    }

    pub const fn stdin(self) -> StreamAttachment {
        self.stdin
    }

    pub const fn stdout(self) -> StreamAttachment {
        self.stdout
    }

    pub const fn stderr(self) -> StreamAttachment {
        self.stderr
    }
}

/// PTY configuration. A PTY combines the child's terminal-facing streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyConfig {
    size: TerminalSize,
}

impl PtyConfig {
    pub const fn new(size: TerminalSize) -> Self {
        Self { size }
    }

    pub const fn size(self) -> TerminalSize {
        self.size
    }
}

/// Child I/O attachment mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoAttachment {
    /// Independently configured standard streams.
    Stdio(StdioConfig),
    /// One pseudo-terminal connected to the child's terminal streams.
    Pty(PtyConfig),
}

/// Fully owned and validated request to create a child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpawnRequest {
    program: ProgramSpec,
    io: IoAttachment,
    kill_on_drop: bool,
}

impl ProcessSpawnRequest {
    /// Validates the launch specification before a backend can acquire resources.
    pub fn new(program: ProgramSpec, io: IoAttachment) -> Result<Self, ProgramSpecError> {
        program.validate()?;

        Ok(Self {
            program,
            io,
            kill_on_drop: true,
        })
    }

    /// Overrides whether dropping the process handle must terminate a live child.
    pub fn with_kill_on_drop(mut self, enabled: bool) -> Self {
        self.kill_on_drop = enabled;
        self
    }

    pub fn program(&self) -> &ProgramSpec {
        &self.program
    }

    pub const fn io(&self) -> IoAttachment {
        self.io
    }

    pub const fn kill_on_drop(&self) -> bool {
        self.kill_on_drop
    }
}

/// Platform-neutral capability that a runtime backend may expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuntimeCapability {
    StdioPipes,
    PseudoTerminal,
    TerminalResize,
    Signals,
    ProcessGroups,
    EnvironmentOverrides,
    WorkingDirectory,
    NonBlockingIo,
}

/// Immutable capability snapshot reported by a runtime backend.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformCapabilities {
    supported: Vec<RuntimeCapability>,
}

impl PlatformCapabilities {
    pub fn new(capabilities: impl IntoIterator<Item = RuntimeCapability>) -> Self {
        let mut supported: Vec<_> = capabilities.into_iter().collect();
        supported.sort_unstable();
        supported.dedup();
        Self { supported }
    }

    pub fn supports(&self, capability: RuntimeCapability) -> bool {
        self.supported.binary_search(&capability).is_ok()
    }

    pub fn supported(&self) -> &[RuntimeCapability] {
        &self.supported
    }
}

/// Platform-neutral process signal intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Terminate,
    Hangup,
    Quit,
    Kill,
    User1,
    User2,
}

/// Requested child termination policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationMode {
    /// Ask the child to terminate cooperatively.
    Graceful,
    /// Force termination without waiting for child cooperation.
    Force,
}

/// Final child process outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExitStatus {
    Exited(i32),
    Signaled(Signal),
    Unknown,
}

impl ProcessExitStatus {
    pub const fn success(self) -> bool {
        matches!(self, Self::Exited(0))
    }
}

/// Result of a non-blocking read from a child output stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// Number of bytes copied into the caller's non-empty buffer.
    Data(NonZeroUsize),
    /// No bytes are currently available; this is not EOF.
    WouldBlock,
    /// The stream has reached EOF.
    Eof,
}

/// Result of a possibly partial write to child stdin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Number of bytes accepted from the caller's non-empty input slice.
    Written(NonZeroUsize),
    /// The stream cannot currently accept more bytes.
    WouldBlock,
}

/// Stable runtime error categories. Backends should not include secrets or raw
/// command input in error messages; platform detail belongs in safe diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidArgument,
    InvalidProgram(ProgramSpecError),
    UnsupportedCapability(RuntimeCapability),
    BackendUnavailable,
    SpawnFailed,
    IoFailed,
    SignalFailed,
    ResizeFailed,
    WaitFailed,
    ProcessAlreadyExited,
    ResourceLimitExceeded,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidArgument => "invalid runtime argument",
            Self::InvalidProgram(error) => return write!(formatter, "invalid program: {error}"),
            Self::UnsupportedCapability(capability) => {
                return write!(formatter, "unsupported runtime capability: {capability:?}")
            }
            Self::BackendUnavailable => "runtime backend unavailable",
            Self::SpawnFailed => "child process creation failed",
            Self::IoFailed => "child process I/O failed",
            Self::SignalFailed => "child process signal operation failed",
            Self::ResizeFailed => "PTY resize failed",
            Self::WaitFailed => "child process wait failed",
            Self::ProcessAlreadyExited => "child process has already exited",
            Self::ResourceLimitExceeded => "runtime resource limit exceeded",
        };

        formatter.write_str(message)
    }
}

impl Error for RuntimeError {}

/// Handle to one runtime-owned child process and its attached I/O resources.
///
/// Implementations must be movable to a worker thread. They must not block the
/// GUI/event-loop thread. try_wait is non-blocking; wait may block and must only
/// be called from a worker. A dropped handle follows the spawn request's
/// kill_on_drop policy and releases all owned OS resources exactly once.
pub trait RuntimeProcess: Send {
    /// Platform process identifier, when the backend exposes one.
    fn process_id(&self) -> Option<u32>;

    /// Writes some or all of a non-empty byte slice to child stdin.
    fn write_stdin(&mut self, input: &[u8]) -> Result<WriteOutcome, RuntimeError>;

    /// Reads available bytes from child stdout without blocking.
    fn read_stdout(&mut self, output: &mut [u8]) -> Result<ReadOutcome, RuntimeError>;

    /// Reads available bytes from child stderr without blocking.
    fn read_stderr(&mut self, output: &mut [u8]) -> Result<ReadOutcome, RuntimeError>;

    /// Closes the child's stdin stream. Repeated close calls are successful.
    fn close_stdin(&mut self) -> Result<(), RuntimeError>;

    /// Changes the effective PTY size. Non-PTY processes return UnsupportedCapability.
    fn resize_pty(&mut self, size: TerminalSize) -> Result<(), RuntimeError>;

    /// Sends a semantic signal to this process only.
    fn send_signal(&mut self, signal: Signal) -> Result<(), RuntimeError>;

    /// Checks whether the child exited without blocking.
    ///
    /// Once an exit status is returned, subsequent calls return the same status.
    fn try_wait(&mut self) -> Result<Option<ProcessExitStatus>, RuntimeError>;

    /// Waits for child exit. Must never be called from the GUI/event-loop thread.
    fn wait(&mut self) -> Result<ProcessExitStatus, RuntimeError>;

    /// Applies the requested graceful or forced termination policy.
    fn terminate(&mut self, mode: TerminationMode) -> Result<(), RuntimeError>;
}

/// Platform process service implemented by a concrete OS adapter.
///
/// Implementations must validate the complete request before creating OS
/// resources. On spawn failure, no child, descriptor, or partially registered
/// runtime handle may be leaked.
pub trait RuntimeService: Send + Sync {
    /// Returns a stable snapshot of the backend's supported capabilities.
    fn capabilities(&self) -> PlatformCapabilities;

    /// Creates a child process and transfers OS-resource ownership to its handle.
    fn spawn(&self, request: ProcessSpawnRequest) -> Result<Box<dyn RuntimeProcess>, RuntimeError>;
}

#[cfg(test)]
mod tests {
    use super::{
        IoAttachment, PlatformCapabilities, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec,
        ProgramSpecError, PtyConfig, ReadOutcome, RuntimeCapability, RuntimeError, RuntimeProcess,
        RuntimeService, Signal, StdioConfig, StreamAttachment, TerminalSize, TerminalSizeError,
        TerminationMode, WriteOutcome,
    };
    use std::ffi::{OsStr, OsString};
    use std::num::NonZeroUsize;

    #[test]
    fn rejects_empty_and_nul_program_values() {
        assert_eq!(ProgramSpec::new(""), Err(ProgramSpecError::EmptyProgram));
        assert_eq!(
            ProgramSpec::new(OsString::from("bad\0program")),
            Err(ProgramSpecError::ProgramContainsNul)
        );
    }

    #[test]
    fn validates_arguments_environment_and_working_directory() {
        let invalid_argument = ProgramSpec::new("sh")
            .expect("program")
            .with_arg(OsString::from("bad\0argument"));
        assert_eq!(
            invalid_argument.validate(),
            Err(ProgramSpecError::ArgumentContainsNul)
        );

        let empty_key = ProgramSpec::new("sh")
            .expect("program")
            .with_env("", "value");
        assert_eq!(
            empty_key.validate(),
            Err(ProgramSpecError::EmptyEnvironmentKey)
        );

        let invalid_key = ProgramSpec::new("sh")
            .expect("program")
            .with_env("BAD=KEY", "value");
        assert_eq!(
            invalid_key.validate(),
            Err(ProgramSpecError::EnvironmentKeyContainsEquals)
        );

        let invalid_value = ProgramSpec::new("sh")
            .expect("program")
            .with_env("KEY", OsString::from("bad\0value"));
        assert_eq!(
            invalid_value.validate(),
            Err(ProgramSpecError::EnvironmentValueContainsNul)
        );

        let invalid_directory = ProgramSpec::new("sh")
            .expect("program")
            .with_working_directory(OsString::from("/bad\0directory"));
        assert_eq!(
            invalid_directory.validate(),
            Err(ProgramSpecError::WorkingDirectoryContainsNul)
        );
    }

    #[test]
    fn preserves_arguments_and_replaces_duplicate_environment_keys() {
        let spec = ProgramSpec::new("/bin/echo")
            .expect("program")
            .with_arg("two words")
            .with_arg("--literal")
            .with_env("MODE", "first")
            .with_env("MODE", "second");

        assert_eq!(spec.program(), OsStr::new("/bin/echo"));
        assert_eq!(
            spec.args(),
            &[
                OsStr::new("two words").to_owned(),
                OsStr::new("--literal").to_owned()
            ]
        );
        assert_eq!(spec.environment().len(), 1usize);
        assert_eq!(
            spec.environment()[0],
            (
                OsStr::new("MODE").to_owned(),
                OsStr::new("second").to_owned()
            )
        );
    }

    #[test]
    fn validates_pty_dimensions_and_explicit_stdio_modes() {
        assert_eq!(
            TerminalSize::new(0u16, 24u16),
            Err(TerminalSizeError::ZeroColumns)
        );
        assert_eq!(
            TerminalSize::new(80u16, 0u16),
            Err(TerminalSizeError::ZeroRows)
        );

        let size = TerminalSize::new(120u16, 40u16).expect("valid PTY dimensions");
        let request = ProcessSpawnRequest::new(
            ProgramSpec::new("sh").expect("program"),
            IoAttachment::Pty(PtyConfig::new(size)),
        )
        .expect("valid spawn request");

        assert_eq!(request.io(), IoAttachment::Pty(PtyConfig::new(size)));
        assert!(request.kill_on_drop());
        assert!(!request.clone().with_kill_on_drop(false).kill_on_drop());

        let stdio = StdioConfig::new(
            StreamAttachment::Pipe,
            StreamAttachment::Pipe,
            StreamAttachment::Null,
        );
        assert_eq!(stdio.stdin(), StreamAttachment::Pipe);
        assert_eq!(stdio.stdout(), StreamAttachment::Pipe);
        assert_eq!(stdio.stderr(), StreamAttachment::Null);
    }

    #[test]
    fn spawn_request_rejects_invalid_program_before_backend_call() {
        let invalid = ProgramSpec::new("sh")
            .expect("program")
            .with_env("BAD=KEY", "value");
        let io = IoAttachment::Stdio(StdioConfig::new(
            StreamAttachment::Pipe,
            StreamAttachment::Pipe,
            StreamAttachment::Pipe,
        ));

        assert_eq!(
            ProcessSpawnRequest::new(invalid, io),
            Err(ProgramSpecError::EnvironmentKeyContainsEquals)
        );
    }

    #[test]
    fn capabilities_are_deduplicated_and_queryable() {
        let capabilities = PlatformCapabilities::new([
            RuntimeCapability::PseudoTerminal,
            RuntimeCapability::Signals,
            RuntimeCapability::PseudoTerminal,
        ]);

        assert!(capabilities.supports(RuntimeCapability::PseudoTerminal));
        assert!(capabilities.supports(RuntimeCapability::Signals));
        assert!(!capabilities.supports(RuntimeCapability::ProcessGroups));
        assert_eq!(capabilities.supported().len(), 2usize);
    }

    struct FakeRuntime;

    struct FakeProcess {
        waited: bool,
    }

    impl RuntimeService for FakeRuntime {
        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities::new([
                RuntimeCapability::StdioPipes,
                RuntimeCapability::PseudoTerminal,
                RuntimeCapability::TerminalResize,
                RuntimeCapability::Signals,
                RuntimeCapability::NonBlockingIo,
            ])
        }

        fn spawn(
            &self,
            request: ProcessSpawnRequest,
        ) -> Result<Box<dyn RuntimeProcess>, RuntimeError> {
            request
                .program()
                .validate()
                .map_err(RuntimeError::InvalidProgram)?;
            Ok(Box::new(FakeProcess { waited: false }))
        }
    }

    impl RuntimeProcess for FakeProcess {
        fn process_id(&self) -> Option<u32> {
            Some(42u32)
        }

        fn write_stdin(&mut self, input: &[u8]) -> Result<WriteOutcome, RuntimeError> {
            NonZeroUsize::new(input.len())
                .map(WriteOutcome::Written)
                .ok_or(RuntimeError::InvalidArgument)
        }

        fn read_stdout(&mut self, output: &mut [u8]) -> Result<ReadOutcome, RuntimeError> {
            if output.is_empty() {
                return Err(RuntimeError::InvalidArgument);
            }
            output[0] = b'o';
            Ok(ReadOutcome::Data(
                NonZeroUsize::new(1usize).expect("non-zero"),
            ))
        }

        fn read_stderr(&mut self, _output: &mut [u8]) -> Result<ReadOutcome, RuntimeError> {
            Ok(ReadOutcome::WouldBlock)
        }

        fn close_stdin(&mut self) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn resize_pty(&mut self, _size: TerminalSize) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn send_signal(&mut self, _signal: Signal) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<ProcessExitStatus>, RuntimeError> {
            if self.waited {
                Ok(Some(ProcessExitStatus::Exited(0)))
            } else {
                self.waited = true;
                Ok(None)
            }
        }

        fn wait(&mut self) -> Result<ProcessExitStatus, RuntimeError> {
            self.waited = true;
            Ok(ProcessExitStatus::Exited(0))
        }

        fn terminate(&mut self, _mode: TerminationMode) -> Result<(), RuntimeError> {
            self.waited = true;
            Ok(())
        }
    }

    #[test]
    fn runtime_traits_define_spawn_io_resize_signal_and_exit_contracts() {
        let runtime = FakeRuntime;
        let capabilities = runtime.capabilities();
        assert!(capabilities.supports(RuntimeCapability::PseudoTerminal));

        let size = TerminalSize::new(80u16, 24u16).expect("valid PTY dimensions");
        let request = ProcessSpawnRequest::new(
            ProgramSpec::new("sh").expect("program"),
            IoAttachment::Pty(PtyConfig::new(size)),
        )
        .expect("valid spawn request");

        let mut process = runtime.spawn(request).expect("fake spawn");
        assert_eq!(process.process_id(), Some(42u32));

        let written = process.write_stdin(b"hello").expect("stdin write");
        assert_eq!(
            written,
            WriteOutcome::Written(NonZeroUsize::new(5usize).expect("non-zero"))
        );

        let mut output = [0u8; 4usize];
        assert_eq!(
            process.read_stdout(&mut output).expect("stdout read"),
            ReadOutcome::Data(NonZeroUsize::new(1usize).expect("non-zero"))
        );
        assert_eq!(output[0], b'o');
        assert_eq!(
            process.read_stderr(&mut output),
            Ok(ReadOutcome::WouldBlock)
        );
        process.resize_pty(size).expect("PTY resize");
        process.send_signal(Signal::Interrupt).expect("signal");
        assert_eq!(process.try_wait(), Ok(None));
        assert_eq!(process.wait(), Ok(ProcessExitStatus::Exited(0)));
        assert!(ProcessExitStatus::Exited(0).success());
        assert!(!ProcessExitStatus::Signaled(Signal::Kill).success());
        process
            .terminate(TerminationMode::Force)
            .expect("terminate");
        process.close_stdin().expect("close stdin");
    }
}
