//! Linux child-process lifecycle attached to an owned PTY.

use std::fs::File;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::thread;

use crate::{ProcessExitStatus, ProgramSpec, Signal, TerminalSize, TerminationMode};

use super::pty::{duplicate_cloexec, set_window_size, PtyPair};

/// A child process with its own session, process group, controlling terminal,
/// and PTY master descriptor.
#[derive(Debug)]
pub struct LinuxPtyChild {
    child: Option<Child>,
    master: OwnedFd,
    size: TerminalSize,
    kill_on_drop: bool,
    exit_status: Option<ProcessExitStatus>,
}

impl LinuxPtyChild {
    /// Spawns a program attached to a fresh PTY.
    ///
    /// The executable and arguments are passed separately through
    /// `std::process::Command`; no shell command string is constructed.
    /// The child becomes a session and process-group leader, acquires the PTY
    /// slave as its controlling terminal, and receives the slave on stdin,
    /// stdout, and stderr.
    pub fn spawn(
        program: &ProgramSpec,
        size: TerminalSize,
        kill_on_drop: bool,
    ) -> io::Result<Self> {
        program
            .validate()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

        let pty = PtyPair::open(size)?;
        let stdin_fd = pty.try_clone_slave()?;
        let stdout_fd = pty.try_clone_slave()?;
        let stderr_fd = pty.try_clone_slave()?;
        let control_fd = pty.try_clone_slave()?;
        let control_raw_fd = control_fd.as_raw_fd();
        let (master, parent_slave) = pty.into_parts();
        drop(parent_slave);

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
            .stdin(Stdio::from(File::from(stdin_fd)))
            .stdout(Stdio::from(File::from(stdout_fd)))
            .stderr(Stdio::from(File::from(stderr_fd)));

        // SAFETY: the pre-exec closure uses only async-signal-safe libc calls
        // and a valid CLOEXEC slave descriptor owned until spawn returns.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(control_raw_fd, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::tcsetpgrp(control_raw_fd, libc::getpid()) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = command.spawn()?;
        drop(control_fd);

        Ok(Self {
            child: Some(child),
            master,
            size,
            kill_on_drop,
            exit_status: None,
        })
    }

    /// Returns the child process identifier.
    pub fn process_id(&self) -> u32 {
        self.child
            .as_ref()
            .expect("child handle remains present until drop")
            .id()
    }

    /// Borrows the PTY master descriptor.
    pub fn master(&self) -> BorrowedFd<'_> {
        self.master.as_fd()
    }

    /// Duplicates the PTY master descriptor with close-on-exec enabled.
    pub fn try_clone_master(&self) -> io::Result<OwnedFd> {
        duplicate_cloexec(self.master.as_raw_fd())
    }

    /// Returns the dimensions most recently applied to the PTY.
    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    /// Applies a new PTY window size.
    pub fn resize(&mut self, size: TerminalSize) -> io::Result<()> {
        set_window_size(self.master.as_raw_fd(), size)?;
        self.size = size;
        Ok(())
    }

    /// Sends a semantic signal to this child process group only.
    pub fn send_signal(&mut self, signal: Signal) -> io::Result<()> {
        let process_group = i32::try_from(self.process_id()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "process identifier is out of range",
            )
        })?;
        let result = unsafe { libc::kill(-process_group, signal_number(signal)) };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    /// Checks for child exit without blocking. Exit status is cached and stable.
    pub fn try_wait(&mut self) -> io::Result<Option<ProcessExitStatus>> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }

        let child = self.child.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "child handle is unavailable")
        })?;
        match child.try_wait()? {
            Some(status) => {
                let status = map_exit_status(status);
                self.exit_status = Some(status);
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    /// Waits for child exit and caches the final status.
    pub fn wait(&mut self) -> io::Result<ProcessExitStatus> {
        if let Some(status) = self.exit_status {
            return Ok(status);
        }

        let child = self.child.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "child handle is unavailable")
        })?;
        let status = map_exit_status(child.wait()?);
        self.exit_status = Some(status);
        Ok(status)
    }

    /// Requests graceful or forced termination of the owned process group.
    pub fn terminate(&mut self, mode: TerminationMode) -> io::Result<()> {
        let signal = match mode {
            TerminationMode::Graceful => Signal::Terminate,
            TerminationMode::Force => Signal::Kill,
        };

        match self.send_signal(signal) {
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            result => result,
        }
    }

    /// Whether the process is configured to be terminated when this handle drops.
    pub const fn kill_on_drop(&self) -> bool {
        self.kill_on_drop
    }
}

impl Drop for LinuxPtyChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        if self.kill_on_drop {
            if let Ok(process_group) = i32::try_from(child.id()) {
                // SAFETY: the process group was created by this handle in pre_exec.
                // Kill it even if its leader has exited; descendants may remain.
                let _ = unsafe { libc::kill(-process_group, libc::SIGKILL) };
            } else if self.exit_status.is_none() {
                let _ = child.kill();
            }

            if self.exit_status.is_none() {
                let _ = child.wait();
            }
        } else if self.exit_status.is_none() {
            reap_detached_child(child);
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

fn map_exit_status(status: ExitStatus) -> ProcessExitStatus {
    if let Some(code) = status.code() {
        return ProcessExitStatus::Exited(code);
    }

    match status.signal() {
        Some(libc::SIGINT) => ProcessExitStatus::Signaled(Signal::Interrupt),
        Some(libc::SIGTERM) => ProcessExitStatus::Signaled(Signal::Terminate),
        Some(libc::SIGHUP) => ProcessExitStatus::Signaled(Signal::Hangup),
        Some(libc::SIGQUIT) => ProcessExitStatus::Signaled(Signal::Quit),
        Some(libc::SIGKILL) => ProcessExitStatus::Signaled(Signal::Kill),
        Some(libc::SIGUSR1) => ProcessExitStatus::Signaled(Signal::User1),
        Some(libc::SIGUSR2) => ProcessExitStatus::Signaled(Signal::User2),
        Some(_) | None => ProcessExitStatus::Unknown,
    }
}

fn reap_detached_child(child: Child) {
    static REAPER: OnceLock<Option<Sender<Child>>> = OnceLock::new();

    let sender = REAPER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Child>();
        match thread::Builder::new()
            .name("de-pty-child-reaper".to_owned())
            .spawn(move || {
                while let Ok(mut child) = receiver.recv() {
                    let _ = child.wait();
                }
            }) {
            Ok(_worker) => Some(sender),
            Err(_error) => None,
        }
    });

    if let Some(sender) = sender {
        if let Err(error) = sender.send(child) {
            let mut child = error.0;
            let _ = child.wait();
        }
    } else {
        let mut child = child;
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::LinuxPtyChild;
    use crate::{ProcessExitStatus, ProgramSpec, Signal, TerminalSize, TerminationMode};
    use std::io;
    use std::os::fd::AsRawFd;
    use std::sync::Mutex;
    use std::thread;
    use std::time::{Duration, Instant};

    static PROCESS_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn test_size() -> TerminalSize {
        TerminalSize::new(80, 24).expect("valid PTY size")
    }

    fn sleep_program() -> ProgramSpec {
        ProgramSpec::new("/bin/sleep")
            .expect("program")
            .with_arg("30")
    }

    #[test]
    fn spawns_child_with_controlling_terminal_and_private_process_group() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("test -t 0 && test -t 1 && test -t 2");
        let mut child =
            LinuxPtyChild::spawn(&program, test_size(), true).expect("spawn interactive child");
        let pid = i32::try_from(child.process_id()).expect("pid fits pid_t");

        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        assert_eq!(unsafe { libc::tcgetsid(child.master().as_raw_fd()) }, pid);
        assert_eq!(
            child.wait().expect("wait child"),
            ProcessExitStatus::Exited(0)
        );
    }

    #[test]
    fn delivers_semantic_signal_to_child_process_group() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let mut child =
            LinuxPtyChild::spawn(&sleep_program(), test_size(), true).expect("spawn sleep child");

        child.send_signal(Signal::Terminate).expect("send SIGTERM");
        assert_eq!(
            child.wait().expect("wait signaled child"),
            ProcessExitStatus::Signaled(Signal::Terminate)
        );
    }

    #[test]
    fn force_termination_reports_kill_signal() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let mut child =
            LinuxPtyChild::spawn(&sleep_program(), test_size(), true).expect("spawn sleep child");

        child
            .terminate(TerminationMode::Force)
            .expect("force termination");
        assert_eq!(
            child.wait().expect("wait killed child"),
            ProcessExitStatus::Signaled(Signal::Kill)
        );
    }

    #[test]
    fn wait_and_try_wait_cache_status_and_reap_child() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("exit 7");
        let mut child =
            LinuxPtyChild::spawn(&program, test_size(), true).expect("spawn exit child");
        let pid = i32::try_from(child.process_id()).expect("pid fits pid_t");

        let first = child.wait().expect("wait child");
        assert_eq!(first, ProcessExitStatus::Exited(7));
        assert_eq!(child.try_wait().expect("cached try_wait"), Some(first));
        assert_eq!(child.wait().expect("cached wait"), first);

        let mut raw_status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid, &mut raw_status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn drop_kills_and_reaps_live_child_when_kill_on_drop_is_enabled() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let child =
            LinuxPtyChild::spawn(&sleep_program(), test_size(), true).expect("spawn sleep child");
        let pid = i32::try_from(child.process_id()).expect("pid fits pid_t");

        drop(child);

        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn detached_child_is_reaped_by_background_reaper() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("exit 0");
        let child =
            LinuxPtyChild::spawn(&program, test_size(), false).expect("spawn detached child");
        let pid = i32::try_from(child.process_id()).expect("pid fits pid_t");

        drop(child);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut raw_status = 0;
            let result = unsafe { libc::waitpid(pid, &mut raw_status, libc::WNOHANG) };
            if result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                break;
            }
            if result == pid {
                break;
            }
            assert!(Instant::now() < deadline, "detached child was not reaped");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn can_signal_session_descendants_after_leader_exits() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("sleep 30 & exit 0");
        let mut child = LinuxPtyChild::spawn(&program, test_size(), true)
            .expect("spawn shell with background child");
        let process_group = i32::try_from(child.process_id()).expect("pid fits pid_t");

        assert_eq!(
            child.wait().expect("wait shell leader"),
            ProcessExitStatus::Exited(0)
        );
        assert_eq!(unsafe { libc::kill(-process_group, 0) }, 0);
        child
            .send_signal(Signal::Terminate)
            .expect("signal remaining session process group");
    }

    #[test]
    fn invalid_executable_fails_without_returning_a_partial_child() {
        let _guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let program = ProgramSpec::new("/definitely/not/a/real/executable")
            .expect("valid non-empty program spec");

        let error = LinuxPtyChild::spawn(&program, test_size(), true)
            .expect_err("invalid executable must fail");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
