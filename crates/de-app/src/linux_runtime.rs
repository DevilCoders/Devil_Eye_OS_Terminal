//! Linux adapter connecting the platform runtime to application session contracts.

use crate::lifecycle::{ApplicationService, ServiceError};
use crate::service_interfaces::{
    ProcessExit, ProcessRead, RuntimeProcess, RuntimeService, TerminationMode,
};
use de_runtime::linux::runtime::LinuxRuntime;
use de_runtime::{
    IoAttachment, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec, PtyConfig, ReadOutcome,
    RuntimeError, RuntimeProcess as PlatformRuntimeProcess,
    RuntimeService as PlatformRuntimeService, Signal, TerminationMode as PlatformTerminationMode,
    WriteOutcome,
};
use de_terminal_core::TerminalSize;

/// Application-level lifecycle wrapper around the Linux process runtime.
#[derive(Debug, Default)]
pub struct LinuxRuntimeService {
    backend: LinuxRuntime,
    initialized: bool,
}

impl LinuxRuntimeService {
    /// Creates an uninitialized Linux runtime service.
    pub fn new() -> Self {
        Self::default()
    }

    fn map_error(operation: &str, error: RuntimeError) -> ServiceError {
        ServiceError::new(format!("Linux runtime {operation} failed: {error:?}"))
    }
}

impl ApplicationService for LinuxRuntimeService {
    fn name(&self) -> &str {
        "linux-process-runtime"
    }

    fn initialize(&mut self) -> Result<(), ServiceError> {
        if self.initialized {
            return Err(ServiceError::new(
                "Linux runtime service is already initialized",
            ));
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ServiceError> {
        self.initialized = false;
        Ok(())
    }
}

impl RuntimeService for LinuxRuntimeService {
    type Process = LinuxRuntimeProcess;

    fn spawn(
        &mut self,
        program: &ProgramSpec,
        terminal_size: TerminalSize,
    ) -> Result<Self::Process, ServiceError> {
        if !self.initialized {
            return Err(ServiceError::new(
                "Linux runtime service is not initialized",
            ));
        }

        let request = ProcessSpawnRequest::new(
            program.clone(),
            IoAttachment::Pty(PtyConfig::new(terminal_size)),
        )
        .map_err(|error| ServiceError::new(format!("invalid process spawn request: {error}")))?
        .with_kill_on_drop(true);

        let process = PlatformRuntimeService::spawn(&self.backend, request)
            .map_err(|error| Self::map_error("spawn", error))?;
        Ok(LinuxRuntimeProcess { inner: process })
    }
}

/// Owned adapter handle. Dropping the inner runtime handle enforces kill-on-drop.
pub struct LinuxRuntimeProcess {
    inner: Box<dyn PlatformRuntimeProcess>,
}

impl RuntimeProcess for LinuxRuntimeProcess {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, ServiceError> {
        if bytes.is_empty() {
            return Ok(0);
        }
        match self
            .inner
            .write_stdin(bytes)
            .map_err(|error| LinuxRuntimeService::map_error("write", error))?
        {
            WriteOutcome::Written(count) => Ok(count.get()),
            WriteOutcome::WouldBlock => Ok(0),
        }
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<ProcessRead, ServiceError> {
        if buffer.is_empty() {
            return Err(ServiceError::new(
                "Linux runtime read buffer must not be empty",
            ));
        }
        match self
            .inner
            .read_stdout(buffer)
            .map_err(|error| LinuxRuntimeService::map_error("read", error))?
        {
            ReadOutcome::Data(count) => Ok(ProcessRead::Data(count)),
            ReadOutcome::WouldBlock => Ok(ProcessRead::WouldBlock),
            ReadOutcome::Eof => Ok(ProcessRead::Eof),
        }
    }

    fn close_input(&mut self) -> Result<(), ServiceError> {
        self.inner
            .close_stdin()
            .map_err(|error| LinuxRuntimeService::map_error("close stdin", error))
    }

    fn resize(&mut self, size: TerminalSize) -> Result<(), ServiceError> {
        self.inner
            .resize_pty(size)
            .map_err(|error| LinuxRuntimeService::map_error("resize PTY", error))
    }

    fn terminate(&mut self, mode: TerminationMode) -> Result<(), ServiceError> {
        let mode = match mode {
            TerminationMode::Graceful => PlatformTerminationMode::Graceful,
            TerminationMode::Force => PlatformTerminationMode::Force,
        };
        self.inner
            .terminate(mode)
            .map_err(|error| LinuxRuntimeService::map_error("terminate", error))
    }

    fn try_wait(&mut self) -> Result<Option<ProcessExit>, ServiceError> {
        self.inner
            .try_wait()
            .map(|status| status.map(map_process_exit))
            .map_err(|error| LinuxRuntimeService::map_error("wait", error))
    }
}

fn map_process_exit(status: ProcessExitStatus) -> ProcessExit {
    match status {
        ProcessExitStatus::Exited(code) => ProcessExit::Exited { code },
        ProcessExitStatus::Signaled(signal) => ProcessExit::Signaled {
            signal: match signal {
                Signal::Interrupt => 2,
                Signal::Terminate => 15,
                Signal::Hangup => 1,
                Signal::Quit => 3,
                Signal::Kill => 9,
                Signal::User1 => 10,
                Signal::User2 => 12,
            },
        },
        ProcessExitStatus::Unknown => ProcessExit::Unknown,
    }
}
