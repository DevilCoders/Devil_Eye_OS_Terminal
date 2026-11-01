//! Linux adapter connecting the platform runtime to application session contracts.

use crate::lifecycle::{ApplicationService, ServiceError};
use de_ffi::{
    CommandExecutor, ExternalCommand, ExternalExecutionError, ExternalPipeline,
    ExternalRedirectionKind,
};
use crate::service_interfaces::{
    ProcessExit, ProcessRead, RuntimeProcess, RuntimeService, TerminationMode,
};
use de_runtime::linux::runtime::LinuxRuntime;
use de_runtime::{
    IoAttachment, PipelineRedirection, PipelineStage, ProcessExitStatus, ProcessSpawnRequest,
    ProgramSpec, PtyConfig, ReadOutcome,
    RuntimeError, RuntimeProcess as PlatformRuntimeProcess,
    RuntimeService as PlatformRuntimeService, Signal, TerminationMode as PlatformTerminationMode,
    WriteOutcome,
};
use de_terminal_core::TerminalSize;
use std::ffi::{CStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

/// Concrete DE-Shell host executor backed by the Linux process runtime.
///
/// External commands and complete pipeline plans are converted from the
/// borrowed C ABI representation into validated native process specifications.
/// The runtime inherits the shell host's standard streams unless a stage
/// redirection overrides one of them.
#[derive(Debug, Default)]
pub struct LinuxShellCommandExecutor {
    backend: LinuxRuntime,
}

impl LinuxShellCommandExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    fn program_spec(
        command: &ExternalCommand<'_>,
        fallback_working_directory: Option<&CStr>,
    ) -> Result<ProgramSpec, ExternalExecutionError> {
        let mut program = ProgramSpec::new(os_string(command.executable()))
            .map_err(|_| ExternalExecutionError::Failed)?;
        for argument in command.arguments() {
            program = program.with_arg(os_string(argument));
        }
        for entry in command.environment() {
            program = program.with_env(os_string(entry.key), os_string(entry.value));
        }
        if let Some(directory) = command.working_directory().or(fallback_working_directory) {
            program = program.with_working_directory(PathBuf::from(os_string(directory)));
        }
        Ok(program)
    }

    fn redirection_path(target: &CStr, working_directory: Option<&CStr>) -> PathBuf {
        let path = PathBuf::from(os_string(target));
        if path.is_absolute() {
            path
        } else if let Some(directory) = working_directory {
            PathBuf::from(os_string(directory)).join(path)
        } else {
            path
        }
    }
}

fn os_string(value: &CStr) -> OsString {
    OsString::from_vec(value.to_bytes().to_vec())
}

impl CommandExecutor for LinuxShellCommandExecutor {
    fn execute(&mut self, command: ExternalCommand<'_>) -> Result<i32, ExternalExecutionError> {
        let stage = PipelineStage::command(Self::program_spec(&command, None)?);
        PlatformRuntimeService::execute_pipeline(&self.backend, &[stage])
            .map(|result| result.exit_code)
            .map_err(|_| ExternalExecutionError::Failed)
    }

    fn execute_pipeline(
        &mut self,
        pipeline: ExternalPipeline<'_>,
    ) -> Result<i32, ExternalExecutionError> {
        let mut stages = Vec::with_capacity(pipeline.stages().len());
        for external_stage in pipeline.stages() {
            let working_directory = external_stage
                .command()
                .and_then(ExternalCommand::working_directory)
                .or(pipeline.working_directory());
            let mut stage = match external_stage.command() {
                Some(command) => PipelineStage::command(Self::program_spec(
                    command,
                    pipeline.working_directory(),
                )?),
                None => PipelineStage::redirection_only(),
            };

            for redirection in external_stage.redirections() {
                let path = Self::redirection_path(redirection.target(), working_directory);
                let redirection = match redirection.kind() {
                    ExternalRedirectionKind::Input => PipelineRedirection::Input(path),
                    ExternalRedirectionKind::Output => PipelineRedirection::Output(path),
                    ExternalRedirectionKind::Append => PipelineRedirection::Append(path),
                    ExternalRedirectionKind::ErrorOutput => {
                        PipelineRedirection::ErrorOutput(path)
                    }
                    ExternalRedirectionKind::ErrorAppend => {
                        PipelineRedirection::ErrorAppend(path)
                    }
                };
                stage = stage.with_redirection(redirection);
            }
            stages.push(stage);
        }

        PlatformRuntimeService::execute_pipeline(&self.backend, &stages)
            .map(|result| result.exit_code)
            .map_err(|_| ExternalExecutionError::Failed)
    }
}

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

#[cfg(test)]
mod pipeline_tests {
    use super::LinuxShellCommandExecutor;
    use de_ffi::{ExecutionContext, Shell};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "devil-eye-app-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn ffi_shell_pipeline_executes_through_linux_runtime() {
        let output = temporary_path("pipeline");
        let source = format!(
            "/usr/bin/printf 'hello\\n' | /usr/bin/tr a-z A-Z > {}",
            output.display()
        );
        let mut shell = Shell::new().expect("create DE-Shell");
        let context = ExecutionContext::new();
        let mut executor = LinuxShellCommandExecutor::new();

        let result = shell
            .execute(source.as_bytes(), &context, Some(&mut executor))
            .expect("execute shell pipeline through Linux runtime");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.commands_executed, 2usize);
        assert_eq!(fs::read_to_string(&output).expect("read pipeline output"), "HELLO\\n");
        fs::remove_file(output).expect("remove pipeline output");
    }

    #[test]
    fn ffi_shell_stderr_and_redirection_only_plans_reach_linux_runtime() {
        let stderr_path = temporary_path("stderr");
        let redirection_only_path = temporary_path("redirection-only");
        let missing = "/definitely/not/a/devil-eye-app-file";
        let source = format!("/usr/bin/ls {missing} 2> {}", stderr_path.display());
        let mut shell = Shell::new().expect("create DE-Shell");
        let context = ExecutionContext::new();
        let mut executor = LinuxShellCommandExecutor::new();

        let result = shell
            .execute(source.as_bytes(), &context, Some(&mut executor))
            .expect("execute stderr redirection through Linux runtime");
        assert_eq!(result.exit_code, 2);
        assert!(
            fs::read_to_string(&stderr_path)
                .expect("read stderr file")
                .contains(missing)
        );

        let source = format!("> {}", redirection_only_path.display());
        let result = shell
            .execute(source.as_bytes(), &context, Some(&mut executor))
            .expect("execute redirection-only plan through Linux runtime");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            fs::read(&redirection_only_path).expect("read created file"),
            b""
        );

        fs::remove_file(stderr_path).expect("remove stderr file");
        fs::remove_file(redirection_only_path).expect("remove redirection-only file");
    }
}
