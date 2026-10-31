//! Linux child-process lifecycle attached to an owned PTY.

use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use crate::{
    EnvironmentInheritance, ProcessExitStatus, ProgramSpec, ProgramSpecError, Signal, TerminalSize,
    TerminationMode,
};

use super::pty::{duplicate_cloexec, set_window_size, PtyPair};

/// Canonical executable and working directory resolved before acquiring process resources.
pub(super) struct ResolvedProgram {
    pub(super) executable: PathBuf,
    pub(super) working_directory: Option<PathBuf>,
}

/// Resolves a program without relying on Command's implicit PATH search.
///
/// Empty and relative PATH entries are ignored so a child cannot accidentally
/// execute a binary from the current working directory.
pub(super) fn resolve_program(program: &ProgramSpec) -> Result<ResolvedProgram, ProgramSpecError> {
    program.validate()?;

    let working_directory = resolve_working_directory(program)?;
    let executable = resolve_executable(program, working_directory.as_deref())?;

    Ok(ResolvedProgram {
        executable,
        working_directory,
    })
}

fn resolve_working_directory(program: &ProgramSpec) -> Result<Option<PathBuf>, ProgramSpecError> {
    let Some(directory) = program.working_directory() else {
        return Ok(None);
    };

    let canonical = fs::canonicalize(directory).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => ProgramSpecError::WorkingDirectoryNotFound,
        _ => ProgramSpecError::PathResolutionFailed,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|_| ProgramSpecError::PathResolutionFailed)?;
    if !metadata.is_dir() {
        return Err(ProgramSpecError::WorkingDirectoryNotDirectory);
    }

    Ok(Some(canonical))
}

fn resolve_executable(
    program: &ProgramSpec,
    working_directory: Option<&Path>,
) -> Result<PathBuf, ProgramSpecError> {
    let requested = Path::new(program.program());
    if requested.as_os_str().as_bytes().contains(&b'/') {
        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            let base = match working_directory {
                Some(directory) => directory.to_path_buf(),
                None => {
                    std::env::current_dir().map_err(|_| ProgramSpecError::PathResolutionFailed)?
                }
            };
            base.join(requested)
        };
        return resolve_executable_candidate(&candidate);
    }

    let explicit_path = program
        .environment()
        .iter()
        .find(|(key, _)| key.as_os_str() == std::ffi::OsStr::new("PATH"))
        .map(|(_, value)| value.clone());
    let search_path = explicit_path.or_else(|| {
        if program.environment_inheritance() == EnvironmentInheritance::Inherit {
            std::env::var_os("PATH")
        } else {
            None
        }
    });
    let Some(search_path) = search_path else {
        return Err(ProgramSpecError::ExecutableNotFound);
    };

    let mut saw_non_executable = false;
    let mut saw_non_regular = false;
    for directory in std::env::split_paths(&search_path) {
        if directory.as_os_str().is_empty() || !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(requested);
        match resolve_executable_candidate(&candidate) {
            Ok(path) => return Ok(path),
            Err(ProgramSpecError::ExecutableNotFound) => {}
            Err(ProgramSpecError::ExecutableNotExecutable) => saw_non_executable = true,
            Err(ProgramSpecError::ExecutableNotRegularFile) => saw_non_regular = true,
            Err(error) => return Err(error),
        }
    }

    if saw_non_executable {
        Err(ProgramSpecError::ExecutableNotExecutable)
    } else if saw_non_regular {
        Err(ProgramSpecError::ExecutableNotRegularFile)
    } else {
        Err(ProgramSpecError::ExecutableNotFound)
    }
}

fn resolve_executable_candidate(candidate: &Path) -> Result<PathBuf, ProgramSpecError> {
    let canonical = fs::canonicalize(candidate).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => {
            ProgramSpecError::ExecutableNotFound
        }
        io::ErrorKind::PermissionDenied => ProgramSpecError::ExecutableNotExecutable,
        _ => ProgramSpecError::PathResolutionFailed,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|_| ProgramSpecError::PathResolutionFailed)?;
    if !metadata.is_file() {
        return Err(ProgramSpecError::ExecutableNotRegularFile);
    }

    let c_path = CString::new(canonical.as_os_str().as_bytes())
        .map_err(|_| ProgramSpecError::PathResolutionFailed)?;
    // SAFETY: c_path is NUL-terminated and AT_EACCESS checks the effective
    // credentials that will be used by Command::spawn.
    if unsafe {
        libc::faccessat(
            libc::AT_FDCWD,
            c_path.as_ptr(),
            libc::X_OK,
            libc::AT_EACCESS,
        )
    } == -1
    {
        return Err(ProgramSpecError::ExecutableNotExecutable);
    }

    Ok(canonical)
}

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
        let resolved = resolve_program(program)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

        let pty = PtyPair::open(size)?;
        let stdin_fd = pty.try_clone_slave()?;
        let stdout_fd = pty.try_clone_slave()?;
        let stderr_fd = pty.try_clone_slave()?;
        let control_fd = pty.try_clone_slave()?;
        let control_raw_fd = control_fd.as_raw_fd();
        let (master, parent_slave) = pty.into_parts();
        drop(parent_slave);

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

pub(super) fn map_exit_status(status: ExitStatus) -> ProcessExitStatus {
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

pub(super) fn reap_detached_child(child: Child) {
    static REAPER: OnceLock<Option<Sender<Child>>> = OnceLock::new();
    const POLL_INTERVAL: Duration = Duration::from_millis(20);

    let sender = REAPER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Child>();
        match thread::Builder::new()
            .name("de-pty-child-reaper".to_owned())
            .spawn(move || {
                let mut children: Vec<Child> = Vec::new();
                let mut receiver_disconnected = false;

                while !receiver_disconnected || !children.is_empty() {
                    if !receiver_disconnected {
                        match receiver.recv_timeout(POLL_INTERVAL) {
                            Ok(child) => children.push(child),
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                receiver_disconnected = true;
                            }
                        }
                    } else {
                        thread::sleep(POLL_INTERVAL);
                    }

                    let mut index = 0usize;
                    while index < children.len() {
                        match children[index].try_wait() {
                            Ok(Some(_)) => {
                                let mut child = children.swap_remove(index);
                                let _ = child.wait();
                            }
                            Ok(None) => {
                                index += 1usize;
                            }
                            Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
                                let mut child = children.swap_remove(index);
                                let _ = child.wait();
                            }
                            Err(_error) => {
                                index += 1usize;
                            }
                        }
                    }
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
    use super::{resolve_program, LinuxPtyChild};
    use crate::{
        ProcessExitStatus, ProgramSpec, ProgramSpecError, Signal, TerminalSize, TerminationMode,
    };
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::MutexGuard;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn process_test_lock() -> MutexGuard<'static, ()> {
        super::super::test_resource_lock()
    }

    fn test_size() -> TerminalSize {
        TerminalSize::new(80, 24).expect("valid PTY size")
    }

    fn sleep_program() -> ProgramSpec {
        ProgramSpec::new("/bin/sleep")
            .expect("program")
            .with_arg("30")
    }

    #[test]
    fn resolves_absolute_executable_and_explicit_path_search() {
        let absolute = ProgramSpec::new("/bin/sh").expect("absolute executable");
        let resolved = resolve_program(&absolute).expect("resolve absolute executable");
        assert!(resolved.executable.is_absolute());
        assert!(resolved.executable.is_file());

        let searched = ProgramSpec::new("sh")
            .expect("bare executable")
            .clear_environment()
            .with_env("PATH", "/bin:/usr/bin");
        let resolved = resolve_program(&searched).expect("resolve from explicit PATH");
        assert!(resolved.executable.is_absolute());
        assert!(resolved.executable.is_file());
    }

    #[test]
    fn path_search_continues_after_non_directory_entry() {
        let root = std::env::temp_dir().join(format!(
            "de-runtime-path-not-dir-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("create temporary directory");
        let not_directory = root.join("not-a-directory");
        fs::write(&not_directory, b"file, not directory").expect("create PATH file component");
        let path = std::env::join_paths([not_directory.as_os_str(), std::ffi::OsStr::new("/bin")])
            .expect("join PATH components");
        let program = ProgramSpec::new("sh")
            .expect("bare executable")
            .clear_environment()
            .with_env("PATH", path);

        let resolved = resolve_program(&program).expect("continue to valid PATH directory");
        assert!(resolved.executable.is_file());
        assert!(resolved.executable.is_absolute());

        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn ignores_relative_path_entries_and_requires_path_when_environment_is_cleared() {
        let relative_path = ProgramSpec::new("sh")
            .expect("bare executable")
            .clear_environment()
            .with_env("PATH", ".");
        assert!(matches!(
            resolve_program(&relative_path),
            Err(ProgramSpecError::ExecutableNotFound)
        ));

        let no_path = ProgramSpec::new("sh")
            .expect("bare executable")
            .clear_environment();
        assert!(matches!(
            resolve_program(&no_path),
            Err(ProgramSpecError::ExecutableNotFound)
        ));
    }

    #[test]
    fn rejects_missing_non_directory_and_non_executable_paths() {
        let missing_cwd = std::env::temp_dir().join(format!(
            "de-runtime-missing-cwd-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        let missing_directory = ProgramSpec::new("/bin/sh")
            .expect("executable")
            .with_working_directory(missing_cwd);
        assert!(matches!(
            resolve_program(&missing_directory),
            Err(ProgramSpecError::WorkingDirectoryNotFound)
        ));

        let file_as_directory = ProgramSpec::new("/bin/sh")
            .expect("executable")
            .with_working_directory("/etc/passwd");
        assert!(matches!(
            resolve_program(&file_as_directory),
            Err(ProgramSpecError::WorkingDirectoryNotDirectory)
        ));

        let directory_as_executable =
            ProgramSpec::new("/bin").expect("directory path is structurally valid");
        assert!(matches!(
            resolve_program(&directory_as_executable),
            Err(ProgramSpecError::ExecutableNotRegularFile)
        ));

        let non_executable = ProgramSpec::new("/etc/passwd").expect("file path");
        assert!(matches!(
            resolve_program(&non_executable),
            Err(ProgramSpecError::ExecutableNotExecutable)
        ));
    }

    #[test]
    fn resolves_relative_executable_against_configured_working_directory() {
        let root = std::env::temp_dir().join(format!(
            "de-runtime-relative-exec-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("create temporary directory");
        let executable = root.join("test-child");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("write executable");
        let mut permissions = fs::metadata(&executable)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("set executable permissions");

        let program = ProgramSpec::new("./test-child")
            .expect("relative executable")
            .with_working_directory(root.clone());
        let resolved = resolve_program(&program).expect("resolve relative executable");
        assert_eq!(
            resolved.executable,
            fs::canonicalize(&executable).expect("canonical executable")
        );
        assert_eq!(
            resolved.working_directory,
            Some(fs::canonicalize(&root).expect("canonical working directory"))
        );

        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn preserves_requested_argv0_for_pty_spawn() {
        let _guard = process_test_lock();
        let output_path = std::env::temp_dir().join(format!(
            "de-runtime-argv0-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        let script = format!("printf '%s' \"$0\" > {}", output_path.display());
        let program = ProgramSpec::new("sh")
            .expect("program")
            .clear_environment()
            .with_env("PATH", "/bin:/usr/bin")
            .with_arg("-c")
            .with_arg(script);
        let mut child =
            LinuxPtyChild::spawn(&program, test_size(), true).expect("spawn alias through PATH");

        assert_eq!(
            child.wait().expect("wait alias child"),
            ProcessExitStatus::Exited(0)
        );
        assert_eq!(
            fs::read_to_string(&output_path).expect("read argv0 output"),
            "sh"
        );
        fs::remove_file(output_path).expect("remove argv0 output");
    }

    #[test]
    fn spawns_child_with_controlling_terminal_and_private_process_group() {
        let _guard = process_test_lock();
        let program = ProgramSpec::new("/bin/sh")
            .expect("program")
            .with_arg("-c")
            .with_arg("test -t 0 && test -t 1 && test -t 2 && : </dev/tty");
        let mut child =
            LinuxPtyChild::spawn(&program, test_size(), true).expect("spawn interactive child");
        let pid = i32::try_from(child.process_id()).expect("pid fits pid_t");

        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        assert_eq!(unsafe { libc::getsid(pid) }, pid);
        assert_eq!(
            child.wait().expect("wait child"),
            ProcessExitStatus::Exited(0)
        );
    }

    #[test]
    fn delivers_semantic_signal_to_child_process_group() {
        let _guard = process_test_lock();
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
        let _guard = process_test_lock();
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
        let _guard = process_test_lock();
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
        let _guard = process_test_lock();
        let child =
            LinuxPtyChild::spawn(&sleep_program(), test_size(), true).expect("spawn sleep child");
        let pid = i32::try_from(child.process_id()).expect("pid fits pid_t");

        drop(child);

        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn detached_child_is_reaped_by_background_reaper() {
        let _guard = process_test_lock();
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
        let _guard = process_test_lock();
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
        let _guard = process_test_lock();
        let program = ProgramSpec::new("/definitely/not/a/real/executable")
            .expect("valid non-empty program spec");

        let error = LinuxPtyChild::spawn(&program, test_size(), true)
            .expect_err("invalid executable must fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error
                .get_ref()
                .and_then(|source| source.downcast_ref::<ProgramSpecError>()),
            Some(&ProgramSpecError::ExecutableNotFound)
        );
    }
}
