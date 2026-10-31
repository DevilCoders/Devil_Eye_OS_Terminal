//! Linux implementation of the platform-neutral process runtime contracts.

use std::fs::File;
use std::io::{self, Read, Write};
use std::num::NonZeroUsize;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use crate::{
    IoAttachment, PlatformCapabilities, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec,
    ReadOutcome, RuntimeCapability, RuntimeError, RuntimeProcess, RuntimeService, Signal,
    StdioConfig, StreamAttachment, TerminalSize, TerminationMode, WriteOutcome,
};

use super::process::{map_exit_status, reap_detached_child, LinuxPtyChild};

/// Linux process runtime supporting PTY sessions and explicit stdio attachments.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxRuntime;

impl RuntimeService for LinuxRuntime {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::new([
            RuntimeCapability::StdioPipes,
            RuntimeCapability::PseudoTerminal,
            RuntimeCapability::TerminalResize,
            RuntimeCapability::Signals,
            RuntimeCapability::ProcessGroups,
            RuntimeCapability::EnvironmentOverrides,
            RuntimeCapability::WorkingDirectory,
            RuntimeCapability::NonBlockingIo,
        ])
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
        let mut command = Command::new(program.program());
        command.args(program.args());
        command.envs(
            program
                .environment()
                .iter()
                .map(|(key, value)| (key, value)),
        );

        if let Some(directory) = program.working_directory() {
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
    use super::LinuxRuntime;
    use crate::{
        IoAttachment, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec, PtyConfig, ReadOutcome,
        RuntimeCapability, RuntimeError, RuntimeProcess, RuntimeService, Signal, StdioConfig,
        StreamAttachment, TerminalSize, TerminationMode, WriteOutcome,
    };
    use std::num::NonZeroUsize;
    use std::thread;
    use std::time::{Duration, Instant};

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
            Err(RuntimeError::SpawnFailed)
        ));
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
