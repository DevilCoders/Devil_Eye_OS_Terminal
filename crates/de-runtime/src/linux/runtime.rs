//! Linux implementation of the platform-neutral process runtime contracts.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::num::NonZeroUsize;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use crate::{
    EnvironmentInheritance, IoAttachment, PipelineExecutionResult, PipelineRedirection,
    PipelineStage, PlatformCapabilities, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec,
    ProgramSpecError, ReadOutcome, RuntimeCapability,
    RuntimeError, RuntimeProcess, RuntimeService, Signal, StdioConfig, StreamAttachment,
    TerminalSize, TerminationMode, WriteOutcome,
};

use super::process::{map_exit_status, reap_detached_child, resolve_program, LinuxPtyChild};

/// Linux process runtime supporting PTY sessions and explicit stdio attachments.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxRuntime;

impl RuntimeService for LinuxRuntime {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::new([
            RuntimeCapability::StdioPipes,
            RuntimeCapability::PipelineExecution,
            RuntimeCapability::PseudoTerminal,
            RuntimeCapability::TerminalResize,
            RuntimeCapability::Signals,
            RuntimeCapability::ProcessGroups,
            RuntimeCapability::EnvironmentOverrides,
            RuntimeCapability::WorkingDirectory,
            RuntimeCapability::NonBlockingIo,
        ])
    }

    fn execute_pipeline(
        &self,
        stages: &[PipelineStage],
    ) -> Result<PipelineExecutionResult, RuntimeError> {
        LinuxRuntime::execute_pipeline(self, stages)
    }

    fn spawn(&self, request: ProcessSpawnRequest) -> Result<Box<dyn RuntimeProcess>, RuntimeError> {
        request
            .program()
            .validate()
            .map_err(RuntimeError::InvalidProgram)?;

        match request.io() {
            IoAttachment::Pty(config) => {
                let child =
                    LinuxPtyChild::spawn(request.program(), config.size(), request.kill_on_drop())
                        .map_err(map_spawn_error)?;

                let master = match child.try_clone_master() {
                    Ok(master) => master,
                    Err(error) => {
                        let mut child = child;
                        let _ = child.terminate(TerminationMode::Force);
                        let _ = child.wait();
                        return Err(map_io_error(error));
                    }
                };
                let master = File::from(master);
                if let Err(error) = set_nonblocking(master.as_raw_fd()) {
                    let mut child = child;
                    let _ = child.terminate(TerminationMode::Force);
                    let _ = child.wait();
                    return Err(map_io_error(error));
                }

                Ok(Box::new(LinuxRuntimeProcess::Pty(PtyRuntimeProcess {
                    child,
                    master,
                    stdin_closed: false,
                })))
            }
            IoAttachment::Stdio(config) => {
                let process =
                    StdioRuntimeProcess::spawn(request.program(), config, request.kill_on_drop())?;
                Ok(Box::new(LinuxRuntimeProcess::Stdio(process)))
            }
        }
    }
}



impl LinuxRuntime {
    /// Executes a bounded foreground pipeline with runtime-owned pipes,
    /// redirection files, process-group cancellation, and deterministic reaping.
    ///
    /// Every stage is spawned before any stage is waited on. Explicit
    /// redirections are applied in source order and override the matching
    /// pipeline endpoint. A pipeline containing only one redirection-only
    /// stage performs the requested file operations without changing the
    /// application's standard streams.
    pub fn execute_pipeline(
        &self,
        stages: &[PipelineStage],
    ) -> Result<PipelineExecutionResult, RuntimeError> {
        const MAX_PIPELINE_STAGES: usize = 64usize;

        if stages.is_empty() || stages.len() > MAX_PIPELINE_STAGES {
            return Err(RuntimeError::ResourceLimitExceeded);
        }
        if stages.len() > 1usize && stages.iter().any(|stage| stage.program.is_none()) {
            return Err(RuntimeError::InvalidArgument);
        }
        if stages.len() == 1usize && stages[0].program.is_none() {
            if stages[0].redirections.is_empty() {
                return Err(RuntimeError::InvalidArgument);
            }
            for redirection in &stages[0].redirections {
                let _file = open_pipeline_redirection(redirection)?;
            }
            return Ok(PipelineExecutionResult {
                exit_code: 0,
                stage_statuses: vec![ProcessExitStatus::Exited(0)],
            });
        }

        let mut resolved = Vec::with_capacity(stages.len());
        for stage in stages {
            let program = stage.program.as_ref().ok_or(RuntimeError::InvalidArgument)?;
            program.validate().map_err(RuntimeError::InvalidProgram)?;
            resolved.push(resolve_program(program).map_err(RuntimeError::InvalidProgram)?);
            for redirection in &stage.redirections {
                let path = match redirection {
                    PipelineRedirection::Input(path)
                    | PipelineRedirection::Output(path)
                    | PipelineRedirection::Append(path) => path,
                };
                if path.as_os_str().as_encoded_bytes().contains(&0u8) {
                    return Err(RuntimeError::InvalidArgument);
                }
            }
        }

        let mut prepared_redirections: Vec<Vec<(bool, File)>> =
            Vec::with_capacity(stages.len());
        for stage in stages {
            let mut stage_redirections = Vec::with_capacity(stage.redirections.len());
            for redirection in &stage.redirections {
                let is_input = matches!(redirection, PipelineRedirection::Input(_));
                let file = open_pipeline_redirection(redirection)?;
                stage_redirections.push((is_input, file));
            }
            prepared_redirections.push(stage_redirections);
        }

        let mut pipes: Vec<(Option<OwnedFd>, Option<OwnedFd>)> =
            Vec::with_capacity(stages.len().saturating_sub(1usize));
        for _ in 0..stages.len().saturating_sub(1usize) {
            let (read_end, write_end) = create_cloexec_pipe().map_err(map_io_error)?;
            pipes.push((Some(read_end), Some(write_end)));
        }

        let mut children: Vec<Child> = Vec::with_capacity(stages.len());
        let mut process_group: Option<libc::pid_t> = None;

        for (index, stage) in stages.iter().enumerate() {
            let program = stage.program.as_ref().ok_or(RuntimeError::InvalidArgument)?;
            let mut stdin = if index == 0usize {
                Stdio::inherit()
            } else {
                let read_end = match pipes[index - 1usize].0.take() {
                    Some(descriptor) => descriptor,
                    None => {
                        terminate_pipeline_children(&mut children, process_group);
                        return Err(RuntimeError::IoFailed);
                    }
                };
                Stdio::from(read_end)
            };
            let mut stdout = if index + 1usize == stages.len() {
                Stdio::inherit()
            } else {
                let write_end = match pipes[index].1.take() {
                    Some(descriptor) => descriptor,
                    None => {
                        terminate_pipeline_children(&mut children, process_group);
                        return Err(RuntimeError::IoFailed);
                    }
                };
                Stdio::from(write_end)
            };

            for (is_input, file) in prepared_redirections[index].drain(..) {
                if is_input {
                    stdin = Stdio::from(file);
                } else {
                    stdout = Stdio::from(file);
                }
            }

            let mut command = Command::new(&resolved[index].executable);
            command.arg0(program.program());
            command.args(program.args());
            if program.environment_inheritance() == EnvironmentInheritance::Clear {
                command.env_clear();
            }
            command.envs(
                program
                    .environment()
                    .iter()
                    .map(|(key, value)| (key, value)),
            );
            if let Some(directory) = resolved[index].working_directory.as_deref() {
                command.current_dir(directory);
            }
            command.stdin(stdin).stdout(stdout).stderr(Stdio::inherit());

            let requested_group = process_group;
            // SAFETY: setpgid is async-signal-safe. The first stage creates a
            // private group; subsequent stages join that exact group.
            unsafe {
                command.pre_exec(move || {
                    let group = requested_group.unwrap_or(0);
                    if libc::setpgid(0, group) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }

            match command.spawn() {
                Ok(child) => {
                    if process_group.is_none() {
                        process_group = i32::try_from(child.id()).ok();
                        if process_group.is_none() {
                            children.push(child);
                            terminate_pipeline_children(&mut children, process_group);
                            return Err(RuntimeError::ResourceLimitExceeded);
                        }
                    }
                    children.push(child);
                }
                Err(error) => {
                    terminate_pipeline_children(&mut children, process_group);
                    return Err(map_spawn_error(error));
                }
            }
        }

        // All unused parent-side pipe endpoints are closed before waiting.
        drop(pipes);

        let mut stage_statuses = Vec::with_capacity(children.len());
        let mut exit_code = 0;
        for index in 0..children.len() {
            match children[index].wait() {
                Ok(status) => {
                    exit_code = status.code().unwrap_or_else(|| {
                        use std::os::unix::process::ExitStatusExt;
                        128 + status.signal().unwrap_or(0)
                    });
                    stage_statuses.push(map_exit_status(status));
                }
                Err(_error) => {
                    terminate_pipeline_children(&mut children, process_group);
                    return Err(RuntimeError::WaitFailed);
                }
            }
        }

        Ok(PipelineExecutionResult {
            exit_code,
            stage_statuses,
        })
    }
}

fn open_pipeline_redirection(redirection: &PipelineRedirection) -> Result<File, RuntimeError> {
    let mut options = OpenOptions::new();
    match redirection {
        PipelineRedirection::Input(path) => {
            options.read(true);
            options.open(path).map_err(map_io_error)
        }
        PipelineRedirection::Output(path) => {
            options.write(true).create(true).truncate(true);
            options.open(path).map_err(map_io_error)
        }
        PipelineRedirection::Append(path) => {
            options.write(true).create(true).append(true);
            options.open(path).map_err(map_io_error)
        }
    }
}

fn create_cloexec_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut descriptors = [-1; 2];
    // SAFETY: descriptors points to two writable integers and pipe2 initializes
    // both descriptors atomically with close-on-exec set.
    if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe2 returned two fresh owned descriptors.
    let read_end = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
    // SAFETY: pipe2 returned two fresh owned descriptors.
    let write_end = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
    Ok((read_end, write_end))
}

fn terminate_pipeline_children(children: &mut [Child], process_group: Option<libc::pid_t>) {
    if let Some(group) = process_group {
        // SAFETY: a negative PID addresses only the private process group
        // created for this pipeline.
        let _ = unsafe { libc::kill(-group, libc::SIGKILL) };
    }
    for child in children.iter_mut() {
        let _ = child.kill();
    }
    for child in children.iter_mut() {
        let _ = child.wait();
    }
}

#[derive(Debug)]
enum LinuxRuntimeProcess {
    Pty(PtyRuntimeProcess),
    Stdio(StdioRuntimeProcess),
}

#[derive(Debug)]
struct PtyRuntimeProcess {
    child: LinuxPtyChild,
    master: File,
    stdin_closed: bool,
}

#[derive(Debug)]
struct StdioRuntimeProcess {
    child: Option<Child>,
    process_group: libc::pid_t,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    stdin_supported: bool,
    stdout_supported: bool,
    stderr_supported: bool,
    stdin_closed: bool,
    exit_status: Option<ProcessExitStatus>,
    kill_on_drop: bool,
}

impl StdioRuntimeProcess {
    fn spawn(
        program: &ProgramSpec,
        config: StdioConfig,
        kill_on_drop: bool,
    ) -> Result<Self, RuntimeError> {
        let resolved = resolve_program(program).map_err(RuntimeError::InvalidProgram)?;
        let mut command = Command::new(&resolved.executable);
        command.arg0(program.program());
        command.args(program.args());
        if program.environment_inheritance() == EnvironmentInheritance::Clear {
            command.env_clear();
        }
        command.envs(
            program
                .environment()
                .iter()
                .map(|(key, value)| (key, value)),
        );

        if let Some(directory) = resolved.working_directory.as_deref() {
            command.current_dir(directory);
        }

        command
            .stdin(to_stdio(config.stdin()))
            .stdout(to_stdio(config.stdout()))
            .stderr(to_stdio(config.stderr()));

        // SAFETY: setpgid is async-signal-safe and gives this child a private
        // process group so semantic signals never target the application group.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = command.spawn().map_err(map_spawn_error)?;
        let process_group = match i32::try_from(child.id()) {
            Ok(process_group) => process_group,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RuntimeError::ResourceLimitExceeded);
            }
        };

        let mut stdin = child.stdin.take();
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        let nonblocking_result = (|| {
            if let Some(stream) = stdin.as_ref() {
                set_nonblocking(stream.as_raw_fd())?;
            }
            if let Some(stream) = stdout.as_ref() {
                set_nonblocking(stream.as_raw_fd())?;
            }
            if let Some(stream) = stderr.as_ref() {
                set_nonblocking(stream.as_raw_fd())?;
            }
            Ok::<(), io::Error>(())
        })();

        if let Err(error) = nonblocking_result {
            let _ = unsafe { libc::kill(-process_group, libc::SIGKILL) };
            let _ = child.kill();
            let _ = child.wait();
            drop(stdin.take());
            drop(stdout.take());
            drop(stderr.take());
            return Err(map_io_error(error));
        }

        Ok(Self {
            child: Some(child),
            process_group,
            stdin,
            stdout,
            stderr,
            stdin_supported: config.stdin() == StreamAttachment::Pipe,
            stdout_supported: config.stdout() == StreamAttachment::Pipe,
            stderr_supported: config.stderr() == StreamAttachment::Pipe,
            stdin_closed: false,
            exit_status: None,
            kill_on_drop,
        })
    }

    fn process_id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    fn try_wait(&mut self) -> Result<Option<ProcessExitStatus>, RuntimeError> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }

        let child = self.child.as_mut().ok_or(RuntimeError::WaitFailed)?;
        match child.try_wait().map_err(|_| RuntimeError::WaitFailed)? {
            Some(status) => {
                let status = map_exit_status(status);
                self.exit_status = Some(status);
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    fn wait(&mut self) -> Result<ProcessExitStatus, RuntimeError> {
        if let Some(status) = self.exit_status {
            return Ok(status);
        }

        let child = self.child.as_mut().ok_or(RuntimeError::WaitFailed)?;
        let status = map_exit_status(child.wait().map_err(|_| RuntimeError::WaitFailed)?);
        self.exit_status = Some(status);
        Ok(status)
    }

    fn send_signal(&mut self, signal: Signal) -> Result<(), RuntimeError> {
        let result = unsafe { libc::kill(-self.process_group, signal_number(signal)) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Err(RuntimeError::ProcessAlreadyExited);
            }
            return Err(RuntimeError::SignalFailed);
        }
        Ok(())
    }
}

impl Drop for StdioRuntimeProcess {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        if self.kill_on_drop {
            // Kill the group even when its leader has exited; descendants may remain.
            let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
            if self.exit_status.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        } else if self.exit_status.is_none() {
            reap_detached_child(child);
        }
    }
}

impl RuntimeProcess for LinuxRuntimeProcess {
    fn process_id(&self) -> Option<u32> {
        match self {
            Self::Pty(process) => Some(process.child.process_id()),
            Self::Stdio(process) => process.process_id(),
        }
    }

    fn write_stdin(&mut self, input: &[u8]) -> Result<WriteOutcome, RuntimeError> {
        if input.is_empty() {
            return Err(RuntimeError::InvalidArgument);
        }

        match self {
            Self::Pty(process) => {
                if process.stdin_closed {
                    return Err(RuntimeError::InvalidArgument);
                }
                write_nonblocking(&mut process.master, input)
            }
            Self::Stdio(process) => {
                if !process.stdin_supported {
                    return Err(RuntimeError::UnsupportedCapability(
                        RuntimeCapability::StdioPipes,
                    ));
                }
                if process.stdin_closed {
                    return Err(RuntimeError::InvalidArgument);
                }
                let stream = process
                    .stdin
                    .as_mut()
                    .ok_or(RuntimeError::ProcessAlreadyExited)?;
                write_nonblocking(stream, input)
            }
        }
    }

    fn read_stdout(&mut self, output: &mut [u8]) -> Result<ReadOutcome, RuntimeError> {
        if output.is_empty() {
            return Err(RuntimeError::InvalidArgument);
        }

        match self {
            Self::Pty(process) => read_nonblocking(&mut process.master, output, true),
            Self::Stdio(process) => {
                if !process.stdout_supported {
                    return Err(RuntimeError::UnsupportedCapability(
                        RuntimeCapability::StdioPipes,
                    ));
                }
                let stream = process
                    .stdout
                    .as_mut()
                    .ok_or(RuntimeError::ProcessAlreadyExited)?;
                read_nonblocking(stream, output, false)
            }
        }
    }

    fn read_stderr(&mut self, output: &mut [u8]) -> Result<ReadOutcome, RuntimeError> {
        if output.is_empty() {
            return Err(RuntimeError::InvalidArgument);
        }

        match self {
            Self::Pty(_) => Err(RuntimeError::UnsupportedCapability(
                RuntimeCapability::StdioPipes,
            )),
            Self::Stdio(process) => {
                if !process.stderr_supported {
                    return Err(RuntimeError::UnsupportedCapability(
                        RuntimeCapability::StdioPipes,
                    ));
                }
                let stream = process
                    .stderr
                    .as_mut()
                    .ok_or(RuntimeError::ProcessAlreadyExited)?;
                read_nonblocking(stream, output, false)
            }
        }
    }

    fn close_stdin(&mut self) -> Result<(), RuntimeError> {
        match self {
            Self::Pty(process) => {
                if process.stdin_closed {
                    return Ok(());
                }
                match write_nonblocking(&mut process.master, &[4u8])? {
                    WriteOutcome::Written(_) => {
                        process.stdin_closed = true;
                        Ok(())
                    }
                    WriteOutcome::WouldBlock => Err(RuntimeError::IoFailed),
                }
            }
            Self::Stdio(process) => {
                process.stdin.take();
                process.stdin_closed = true;
                Ok(())
            }
        }
    }

    fn resize_pty(&mut self, size: TerminalSize) -> Result<(), RuntimeError> {
        match self {
            Self::Pty(process) => process
                .child
                .resize(size)
                .map_err(|_| RuntimeError::ResizeFailed),
            Self::Stdio(_) => Err(RuntimeError::UnsupportedCapability(
                RuntimeCapability::TerminalResize,
            )),
        }
    }

    fn send_signal(&mut self, signal: Signal) -> Result<(), RuntimeError> {
        match self {
            Self::Pty(process) => process.child.send_signal(signal).map_err(|error| {
                if error.raw_os_error() == Some(libc::ESRCH) {
                    RuntimeError::ProcessAlreadyExited
                } else {
                    RuntimeError::SignalFailed
                }
            }),
            Self::Stdio(process) => process.send_signal(signal),
        }
    }

    fn try_wait(&mut self) -> Result<Option<ProcessExitStatus>, RuntimeError> {
        match self {
            Self::Pty(process) => process
                .child
                .try_wait()
                .map_err(|_| RuntimeError::WaitFailed),
            Self::Stdio(process) => process.try_wait(),
        }
    }

    fn wait(&mut self) -> Result<ProcessExitStatus, RuntimeError> {
        match self {
            Self::Pty(process) => process.child.wait().map_err(|_| RuntimeError::WaitFailed),
            Self::Stdio(process) => process.wait(),
        }
    }

    fn terminate(&mut self, mode: TerminationMode) -> Result<(), RuntimeError> {
        match self {
            Self::Pty(process) => process
                .child
                .terminate(mode)
                .map_err(|_| RuntimeError::SignalFailed),
            Self::Stdio(process) => {
                let signal = match mode {
                    TerminationMode::Graceful => Signal::Terminate,
                    TerminationMode::Force => Signal::Kill,
                };
                match process.send_signal(signal) {
                    Err(RuntimeError::ProcessAlreadyExited) => Ok(()),
                    result => result,
                }
            }
        }
    }
}

fn to_stdio(attachment: StreamAttachment) -> Stdio {
    match attachment {
        StreamAttachment::Pipe => Stdio::piped(),
        StreamAttachment::Null => Stdio::null(),
        StreamAttachment::Inherit => Stdio::inherit(),
    }
}

fn set_nonblocking(fd: libc::c_int) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }

    if flags & libc::O_NONBLOCK == 0
        && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
    {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

fn read_nonblocking<R: Read>(
    stream: &mut R,
    output: &mut [u8],
    pty_stream: bool,
) -> Result<ReadOutcome, RuntimeError> {
    loop {
        match stream.read(output) {
            Ok(0) => return Ok(ReadOutcome::Eof),
            Ok(count) => {
                let count = NonZeroUsize::new(count).ok_or(RuntimeError::IoFailed)?;
                return Ok(ReadOutcome::Data(count));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(ReadOutcome::WouldBlock);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if pty_stream && error.raw_os_error() == Some(libc::EIO) => {
                return Ok(ReadOutcome::Eof);
            }
            Err(_) => return Err(RuntimeError::IoFailed),
        }
    }
}

fn write_nonblocking<W: Write>(stream: &mut W, input: &[u8]) -> Result<WriteOutcome, RuntimeError> {
    loop {
        match stream.write(input) {
            Ok(0) => return Err(RuntimeError::IoFailed),
            Ok(count) => {
                let count = NonZeroUsize::new(count).ok_or(RuntimeError::IoFailed)?;
                return Ok(WriteOutcome::Written(count));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(WriteOutcome::WouldBlock);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if matches!(error.raw_os_error(), Some(libc::EPIPE) | Some(libc::EIO)) => {
                return Err(RuntimeError::ProcessAlreadyExited);
            }
            Err(_) => return Err(RuntimeError::IoFailed),
        }
    }
}

fn signal_number(signal: Signal) -> libc::c_int {
    match signal {
        Signal::Interrupt => libc::SIGINT,
        Signal::Terminate => libc::SIGTERM,
        Signal::Hangup => libc::SIGHUP,
        Signal::Quit => libc::SIGQUIT,
        Signal::Kill => libc::SIGKILL,
        Signal::User1 => libc::SIGUSR1,
        Signal::User2 => libc::SIGUSR2,
    }
}

fn map_spawn_error(error: io::Error) -> RuntimeError {
    if let Some(program_error) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<ProgramSpecError>())
    {
        return RuntimeError::InvalidProgram(*program_error);
    }
    if error.kind() == io::ErrorKind::InvalidInput {
        RuntimeError::InvalidArgument
    } else {
        RuntimeError::SpawnFailed
    }
}

fn map_io_error(error: io::Error) -> RuntimeError {
    if error.raw_os_error() == Some(libc::ESRCH) {
        RuntimeError::ProcessAlreadyExited
    } else {
        RuntimeError::IoFailed
    }
}

#[cfg(test)]
mod tests {
    use super::{LinuxRuntime, PipelineRedirection, PipelineStage};
    use crate::{
        IoAttachment, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec, ProgramSpecError,
        PtyConfig, ReadOutcome, RuntimeCapability, RuntimeError, RuntimeProcess, RuntimeService,
        Signal, StdioConfig, StreamAttachment, TerminalSize, TerminationMode, WriteOutcome,
    };
    use std::fs;
    use std::num::NonZeroUsize;
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::thread;
    use std::time::{Duration, Instant};

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "devil-eye-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn size() -> TerminalSize {
        TerminalSize::new(80u16, 24u16).expect("valid terminal size")
    }

    fn runtime_test_lock() -> std::sync::MutexGuard<'static, ()> {
        super::super::test_resource_lock()
    }

    fn pipe_config() -> StdioConfig {
        StdioConfig::new(
            StreamAttachment::Pipe,
            StreamAttachment::Pipe,
            StreamAttachment::Pipe,
        )
    }

    fn read_until_marker(
        process: &mut dyn RuntimeProcess,
        marker: &[u8],
    ) -> Result<Vec<u8>, RuntimeError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut collected = Vec::new();
        let mut buffer = [0u8; 1024usize];

        loop {
            match process.read_stdout(&mut buffer)? {
                ReadOutcome::Data(count) => {
                    collected.extend_from_slice(&buffer[..count.get()]);
                    if collected
                        .windows(marker.len())
                        .any(|window| window == marker)
                    {
                        return Ok(collected);
                    }
                }
                ReadOutcome::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(RuntimeError::IoFailed);
                    }
                    thread::sleep(Duration::from_millis(2));
                }
                ReadOutcome::Eof => return Err(RuntimeError::ProcessAlreadyExited),
            }
        }
    }

    fn drain_until_exit(
        process: &mut dyn RuntimeProcess,
        timeout: Duration,
    ) -> Result<(Vec<u8>, ProcessExitStatus), RuntimeError> {
        let deadline = Instant::now() + timeout;
        let mut collected = Vec::new();
        let mut buffer = [0u8; 4096usize];
        let mut exit_status = None;
        let mut eof = false;

        while !(exit_status.is_some() && eof) {
            match process.read_stdout(&mut buffer)? {
                ReadOutcome::Data(count) => {
                    collected.extend_from_slice(&buffer[..count.get()]);
                }
                ReadOutcome::WouldBlock => {}
                ReadOutcome::Eof => eof = true,
            }

            if exit_status.is_none() {
                exit_status = process.try_wait()?;
            }

            if Instant::now() >= deadline {
                return Err(RuntimeError::WaitFailed);
            }
            if !eof || exit_status.is_none() {
                thread::sleep(Duration::from_millis(1));
            }
        }

        Ok((collected, exit_status.expect("checked above")))
    }

    fn read_to_eof(
        process: &mut dyn RuntimeProcess,
        stderr: bool,
    ) -> Result<Vec<u8>, RuntimeError> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut collected = Vec::new();
        let mut buffer = [0u8; 256usize];

        loop {
            let outcome = if stderr {
                process.read_stderr(&mut buffer)?
            } else {
                process.read_stdout(&mut buffer)?
            };
            match outcome {
                ReadOutcome::Data(count) => {
                    collected.extend_from_slice(&buffer[..count.get()]);
                }
                ReadOutcome::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(RuntimeError::IoFailed);
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                ReadOutcome::Eof => return Ok(collected),
            }
        }
    }

    #[test]
    fn preserves_requested_argv0_for_stdio_spawn() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("sh")
            .expect("program")
            .clear_environment()
            .with_env("PATH", "/bin:/usr/bin")
            .with_arg("-c")
            .with_arg("printf '%s' \"$0\"");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Stdio(pipe_config()))
            .expect("spawn request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn PATH alias");

        let (output, status) =
            drain_until_exit(&mut *process, Duration::from_secs(3)).expect("drain child output");
        assert_eq!(status, ProcessExitStatus::Exited(0));
        assert_eq!(output, b"sh");
    }

    #[test]
    fn reports_linux_runtime_capabilities() {
        let _guard = runtime_test_lock();
        let capabilities = LinuxRuntime.capabilities();
        for capability in [
            RuntimeCapability::StdioPipes,
            RuntimeCapability::PseudoTerminal,
            RuntimeCapability::TerminalResize,
            RuntimeCapability::Signals,
            RuntimeCapability::ProcessGroups,
            RuntimeCapability::EnvironmentOverrides,
            RuntimeCapability::WorkingDirectory,
            RuntimeCapability::NonBlockingIo,
        ] {
            assert!(capabilities.supports(capability));
        }
    }

    #[test]
    fn rejects_unresolved_executable_and_invalid_working_directory_before_spawn() {
        let runtime = LinuxRuntime;
        let missing = ProgramSpec::new("de-runtime-no-such-executable")
            .expect("program")
            .clear_environment();
        let request = ProcessSpawnRequest::new(missing, IoAttachment::Pty(PtyConfig::new(size())))
            .expect("valid request structure");
        match runtime.spawn(request) {
            Err(error) => assert_eq!(
                error,
                RuntimeError::InvalidProgram(ProgramSpecError::ExecutableNotFound)
            ),
            Ok(_) => panic!("missing executable unexpectedly spawned"),
        }

        let invalid_directory = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_working_directory("/etc/passwd");
        let request =
            ProcessSpawnRequest::new(invalid_directory, IoAttachment::Pty(PtyConfig::new(size())))
                .expect("valid request structure");
        match runtime.spawn(request) {
            Err(error) => assert_eq!(
                error,
                RuntimeError::InvalidProgram(ProgramSpecError::WorkingDirectoryNotDirectory)
            ),
            Ok(_) => panic!("non-directory working directory unexpectedly spawned"),
        }
    }

    #[test]
    fn stdio_spawn_can_clear_environment_and_apply_only_explicit_overrides() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("env")
            .expect("program")
            .clear_environment()
            .with_env("PATH", "/usr/bin")
            .with_env("ONLY_EXPLICIT", "visible");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Stdio(pipe_config()))
            .expect("spawn request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn env");

        let output = read_to_eof(process.as_mut(), false).expect("read environment");
        let output = String::from_utf8(output).expect("UTF-8 environment output");
        assert!(output.contains("PATH=/usr/bin"));
        assert!(output.contains("ONLY_EXPLICIT=visible"));
        assert_eq!(output.lines().count(), 2);
    }

    #[test]
    fn stdio_spawn_preserves_arguments_environment_cwd_and_separate_streams() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("printf '%s:%s' \"$PWD\" \"$DE_RUNTIME_TEST\"; printf 'stderr-data' >&2")
            .with_env("DE_RUNTIME_TEST", "runtime-ok")
            .with_working_directory("/tmp");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Stdio(pipe_config()))
            .expect("spawn request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn shell");

        assert!(process.process_id().is_some());
        assert_eq!(
            process.wait().expect("wait shell"),
            ProcessExitStatus::Exited(0)
        );
        let stdout = read_to_eof(process.as_mut(), false).expect("read stdout");
        let stderr = read_to_eof(process.as_mut(), true).expect("read stderr");

        assert_eq!(
            String::from_utf8(stdout).expect("UTF-8 stdout"),
            "/tmp:runtime-ok"
        );
        assert_eq!(
            String::from_utf8(stderr).expect("UTF-8 stderr"),
            "stderr-data"
        );
    }

    #[test]
    fn pty_spawn_merges_output_and_supports_resize() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("printf 'pty-output'");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Pty(PtyConfig::new(size())))
            .expect("PTY request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn PTY child");
        let resized = TerminalSize::new(120u16, 40u16).expect("resized size");

        process.resize_pty(resized).expect("resize PTY");
        assert_eq!(
            process.wait().expect("wait PTY child"),
            ProcessExitStatus::Exited(0)
        );
        let output = read_to_eof(process.as_mut(), false).expect("read PTY output");
        assert!(String::from_utf8(output)
            .expect("UTF-8 PTY output")
            .contains("pty-output"));
        assert_eq!(
            process.read_stderr(&mut [0u8; 16usize]),
            Err(RuntimeError::UnsupportedCapability(
                RuntimeCapability::StdioPipes
            ))
        );
    }

    #[test]
    fn stdio_input_pipe_closes_and_delivers_eof() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/cat").expect("program");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Stdio(pipe_config()))
            .expect("spawn request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn cat");

        assert_eq!(
            process.write_stdin(b"input-data"),
            Ok(WriteOutcome::Written(
                NonZeroUsize::new(10usize).expect("nonzero")
            ))
        );
        process.close_stdin().expect("close stdin");
        assert_eq!(process.close_stdin(), Ok(()));
        assert_eq!(
            process.wait().expect("wait cat"),
            ProcessExitStatus::Exited(0)
        );
        assert_eq!(
            read_to_eof(process.as_mut(), false).expect("read echoed input"),
            b"input-data"
        );
    }

    #[test]
    fn pty_input_is_delivered_to_child_and_output_returns_to_master() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("stty raw -echo; printf READY; exec /bin/cat");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Pty(PtyConfig::new(size())))
            .expect("PTY request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn raw cat");

        let startup = read_until_marker(process.as_mut(), b"READY").expect("wait for raw mode");
        assert!(startup.windows(5).any(|window| window == b"READY"));

        let payload = b"bidirectional-pty-input\0\xff\n";
        let mut offset = 0usize;
        let deadline = Instant::now() + Duration::from_secs(5);
        while offset < payload.len() {
            match process.write_stdin(&payload[offset..]).expect("PTY write") {
                WriteOutcome::Written(count) => offset += count.get(),
                WriteOutcome::WouldBlock => {
                    let mut buffer = [0u8; 128usize];
                    match process.read_stdout(&mut buffer).expect("drain PTY output") {
                        ReadOutcome::Data(_) | ReadOutcome::WouldBlock => {}
                        ReadOutcome::Eof => panic!("PTY reached EOF before input was echoed"),
                    }
                    if Instant::now() >= deadline {
                        panic!("PTY input did not make progress before deadline");
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }

        let mut echoed = Vec::new();
        let mut buffer = [0u8; 128usize];
        while echoed.len() < payload.len() {
            match process.read_stdout(&mut buffer).expect("read PTY echo") {
                ReadOutcome::Data(count) => {
                    echoed.extend_from_slice(&buffer[..count.get()]);
                }
                ReadOutcome::WouldBlock => {
                    if Instant::now() >= deadline {
                        panic!("PTY output did not arrive before deadline");
                    }
                    thread::sleep(Duration::from_millis(2));
                }
                ReadOutcome::Eof => panic!("PTY reached EOF before returning all input"),
            }
        }

        assert_eq!(echoed, payload);
        process.terminate(TerminationMode::Force).expect("stop cat");
        let _ = process.wait();
    }

    #[test]
    fn pty_high_volume_output_is_drained_without_deadlock() {
        let _guard = runtime_test_lock();
        const OUTPUT_BYTES: usize = 256 * 1024;
        let program = ProgramSpec::new("/usr/bin/head")
            .expect("program")
            .with_arg("-c")
            .with_arg(OUTPUT_BYTES.to_string())
            .with_arg("/dev/zero");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Pty(PtyConfig::new(size())))
            .expect("PTY request");
        let mut process = LinuxRuntime
            .spawn(request)
            .expect("spawn high-volume writer");

        let (output, status) = drain_until_exit(process.as_mut(), Duration::from_secs(10))
            .expect("drain output while child is running");
        assert_eq!(status, ProcessExitStatus::Exited(0));
        assert_eq!(output.len(), OUTPUT_BYTES);
        assert!(output.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn pty_backpressure_reports_would_block_and_resumes_after_drain() {
        let _guard = runtime_test_lock();
        const PAYLOAD_BYTES: usize = 1024 * 1024;
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("stty raw -echo; printf READY; sleep 0.3; exec /bin/cat");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Pty(PtyConfig::new(size())))
            .expect("PTY request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn raw cat");
        let _ = read_until_marker(process.as_mut(), b"READY").expect("wait for raw mode");

        let payload: Vec<u8> = (0..PAYLOAD_BYTES)
            .map(|index| (index % 251) as u8)
            .collect();
        let mut offset = 0usize;
        let mut echoed = Vec::with_capacity(PAYLOAD_BYTES);
        let mut buffer = [0u8; 8192usize];
        let mut observed_backpressure = false;
        let deadline = Instant::now() + Duration::from_secs(20);

        while offset < payload.len() || echoed.len() < payload.len() {
            if offset < payload.len() {
                match process
                    .write_stdin(&payload[offset..])
                    .expect("write PTY payload")
                {
                    WriteOutcome::Written(count) => offset += count.get(),
                    WriteOutcome::WouldBlock => observed_backpressure = true,
                }
            }

            loop {
                match process.read_stdout(&mut buffer).expect("drain PTY output") {
                    ReadOutcome::Data(count) => {
                        echoed.extend_from_slice(&buffer[..count.get()]);
                        if echoed.len() >= payload.len() {
                            break;
                        }
                    }
                    ReadOutcome::WouldBlock => break,
                    ReadOutcome::Eof => panic!("PTY closed before all payload bytes were echoed"),
                }
            }

            if Instant::now() >= deadline {
                panic!(
                    "PTY I/O stalled: wrote {offset}/{} bytes and received {}/{}",
                    payload.len(),
                    echoed.len(),
                    payload.len()
                );
            }
            if offset < payload.len() || echoed.len() < payload.len() {
                thread::sleep(Duration::from_millis(1));
            }
        }

        assert!(
            observed_backpressure,
            "test did not exercise PTY backpressure"
        );
        assert_eq!(echoed, payload);
        process.terminate(TerminationMode::Force).expect("stop cat");
        let _ = process.wait();
    }

    #[test]
    fn pty_resize_is_observed_by_the_child_terminal() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("sleep 0.1; stty size");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Pty(PtyConfig::new(size())))
            .expect("PTY request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn size reporter");
        let resized = TerminalSize::new(132u16, 43u16).expect("resized size");

        process.resize_pty(resized).expect("apply PTY resize");
        assert_eq!(
            process.wait().expect("wait size reporter"),
            ProcessExitStatus::Exited(0)
        );
        let output = read_to_eof(process.as_mut(), false).expect("read reported size");
        let output = String::from_utf8(output).expect("UTF-8 size output");
        assert!(
            output.contains("43 132"),
            "child observed unexpected terminal size: {output:?}"
        );
    }

    #[test]
    fn dropping_pty_runtime_handle_cancels_and_reaps_child() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/sleep")
            .expect("program")
            .with_arg("30");
        let request = ProcessSpawnRequest::new(program, IoAttachment::Pty(PtyConfig::new(size())))
            .expect("PTY request");
        let process = LinuxRuntime.spawn(request).expect("spawn sleep");
        let pid = process.process_id().expect("child PID");
        drop(process);

        let pid = i32::try_from(pid).expect("PID fits pid_t");
        // Drop must kill the private process group and wait for its child.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn semantic_signal_targets_the_child_process_group() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/sleep")
            .expect("program")
            .with_arg("30");
        let config = StdioConfig::new(
            StreamAttachment::Null,
            StreamAttachment::Null,
            StreamAttachment::Null,
        );
        let request =
            ProcessSpawnRequest::new(program, IoAttachment::Stdio(config)).expect("spawn request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn sleep");

        process
            .send_signal(Signal::Terminate)
            .expect("send SIGTERM");
        assert_eq!(
            process.wait().expect("wait signaled process"),
            ProcessExitStatus::Signaled(Signal::Terminate)
        );
    }

    #[test]
    fn invalid_executable_returns_spawn_failure_without_partial_handle() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/definitely/not/a/real/executable").expect("program");
        let request = ProcessSpawnRequest::new(
            program,
            IoAttachment::Stdio(StdioConfig::new(
                StreamAttachment::Null,
                StreamAttachment::Null,
                StreamAttachment::Null,
            )),
        )
        .expect("valid spawn request");

        assert!(matches!(
            LinuxRuntime.spawn(request),
            Err(RuntimeError::InvalidProgram(
                ProgramSpecError::ExecutableNotFound
            ))
        ));
    }


    #[test]
    fn pipeline_connects_stages_and_applies_output_redirection() {
        let _guard = runtime_test_lock();
        let output = temporary_path("pipeline-output");
        let stages = [
            PipelineStage::command(
                ProgramSpec::new("/usr/bin/printf")
                    .expect("printf")
                    .with_arg("hello\\n"),
            ),
            PipelineStage::command(
                ProgramSpec::new("/usr/bin/tr")
                    .expect("tr")
                    .with_arg("a-z")
                    .with_arg("A-Z"),
            )
            .with_redirection(PipelineRedirection::Output(output.clone())),
        ];

        let result = LinuxRuntime
            .execute_pipeline(&stages)
            .expect("execute two-stage pipeline");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stage_statuses,
            vec![ProcessExitStatus::Exited(0), ProcessExitStatus::Exited(0)]
        );
        assert_eq!(fs::read_to_string(&output).expect("read pipeline output"), "HELLO\\n");
        fs::remove_file(output).expect("remove pipeline output");
    }

    #[test]
    fn input_append_and_redirection_only_operations_are_owned_by_runtime() {
        let _guard = runtime_test_lock();
        let input = temporary_path("pipeline-input");
        let output = temporary_path("pipeline-append");
        fs::write(&input, "from-file").expect("write input fixture");

        let cat = PipelineStage::command(
            ProgramSpec::new("/usr/bin/cat").expect("cat"),
        )
        .with_redirection(PipelineRedirection::Input(input.clone()))
        .with_redirection(PipelineRedirection::Output(output.clone()));
        let result = LinuxRuntime
            .execute_pipeline(&[cat])
            .expect("execute redirected cat");
        assert_eq!(result.exit_code, 0);
        assert_eq!(fs::read_to_string(&output).expect("read redirected output"), "from-file");

        let append = PipelineStage::command(
            ProgramSpec::new("/usr/bin/printf")
                .expect("printf")
                .with_arg("-tail"),
        )
        .with_redirection(PipelineRedirection::Append(output.clone()));
        let result = LinuxRuntime
            .execute_pipeline(&[append])
            .expect("execute append redirection");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            fs::read_to_string(&output).expect("read appended output"),
            "from-file-tail"
        );

        let empty = temporary_path("redirection-only");
        let redirection_only = PipelineStage::redirection_only()
            .with_redirection(PipelineRedirection::Output(empty.clone()));
        let result = LinuxRuntime
            .execute_pipeline(&[redirection_only])
            .expect("execute redirection-only command");
        assert_eq!(result.exit_code, 0);
        assert_eq!(fs::read(&empty).expect("read created file"), b"");

        fs::remove_file(input).expect("remove input fixture");
        fs::remove_file(output).expect("remove output fixture");
        fs::remove_file(empty).expect("remove redirection-only file");
    }

    #[test]
    fn broken_pipe_pipeline_reaps_all_stages() {
        let _guard = runtime_test_lock();
        let output = temporary_path("broken-pipe");
        let stages = [
            PipelineStage::command(ProgramSpec::new("/usr/bin/yes").expect("yes")),
            PipelineStage::command(
                ProgramSpec::new("/usr/bin/head")
                    .expect("head")
                    .with_arg("-n")
                    .with_arg("1"),
            )
            .with_redirection(PipelineRedirection::Output(output.clone())),
        ];

        let result = LinuxRuntime
            .execute_pipeline(&stages)
            .expect("execute broken-pipe pipeline");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stage_statuses.len(), 2usize);
        assert_eq!(fs::read_to_string(&output).expect("read head output"), "y\\n");
        fs::remove_file(output).expect("remove broken-pipe output");
    }

    #[test]
    fn non_pipe_streams_reject_pipe_io_and_non_pty_resize() {
        let _guard = runtime_test_lock();
        let program = ProgramSpec::new("/bin/sleep")
            .expect("program")
            .with_arg("1");
        let config = StdioConfig::new(
            StreamAttachment::Null,
            StreamAttachment::Inherit,
            StreamAttachment::Null,
        );
        let request =
            ProcessSpawnRequest::new(program, IoAttachment::Stdio(config)).expect("spawn request");
        let mut process = LinuxRuntime.spawn(request).expect("spawn sleep");

        assert_eq!(
            process.write_stdin(b"x"),
            Err(RuntimeError::UnsupportedCapability(
                RuntimeCapability::StdioPipes
            ))
        );
        assert_eq!(
            process.read_stdout(&mut [0u8; 1usize]),
            Err(RuntimeError::UnsupportedCapability(
                RuntimeCapability::StdioPipes
            ))
        );
        assert_eq!(
            process.resize_pty(size()),
            Err(RuntimeError::UnsupportedCapability(
                RuntimeCapability::TerminalResize
            ))
        );
        process
            .terminate(TerminationMode::Force)
            .expect("terminate");
        let _ = process.wait();
    }
}
