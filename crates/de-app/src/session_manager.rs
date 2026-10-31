//! Concrete session ownership and lifecycle coordination.
//!
//! Each managed session owns exactly one runtime process/PTY handle and one
//! bounded input queue. The manager owns session identities and controls
//! create/close/shutdown transitions. It never shares process handles between
//! sessions.

use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use de_terminal_core::TerminalSize;

use crate::lifecycle::{ApplicationService, ServiceError};
use crate::service_interfaces::{
    ProcessExit, ProcessRead, RuntimeProcess, RuntimeService, SessionId, SessionKind,
    SessionLifecycle, SessionLifecycleState, SessionManagerService, SessionSpec, TerminalSession,
    TerminationMode,
};

const DEFAULT_MAX_SESSIONS: usize = 32;
const DEFAULT_MAX_PENDING_INPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_IO_BYTES_PER_POLL: usize = 64 * 1024;
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Resource and timing limits applied by a session manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionManagerLimits {
    max_sessions: NonZeroUsize,
    max_pending_input_bytes: NonZeroUsize,
    max_io_bytes_per_poll: NonZeroUsize,
    graceful_shutdown_timeout: Duration,
    forced_shutdown_timeout: Duration,
}

impl SessionManagerLimits {
    /// Creates validated session resource limits.
    pub const fn new(
        max_sessions: NonZeroUsize,
        max_pending_input_bytes: NonZeroUsize,
        max_io_bytes_per_poll: NonZeroUsize,
        graceful_shutdown_timeout: Duration,
        forced_shutdown_timeout: Duration,
    ) -> Self {
        Self {
            max_sessions,
            max_pending_input_bytes,
            max_io_bytes_per_poll,
            graceful_shutdown_timeout,
            forced_shutdown_timeout,
        }
    }

    /// Maximum number of simultaneously owned sessions.
    pub const fn max_sessions(self) -> usize {
        self.max_sessions.get()
    }

    /// Maximum queued input bytes per session.
    pub const fn max_pending_input_bytes(self) -> usize {
        self.max_pending_input_bytes.get()
    }

    /// Maximum input bytes flushed by one event-loop poll.
    pub const fn max_io_bytes_per_poll(self) -> usize {
        self.max_io_bytes_per_poll.get()
    }

    /// Grace period before forced child termination.
    pub const fn graceful_shutdown_timeout(self) -> Duration {
        self.graceful_shutdown_timeout
    }

    /// Bounded wait after requesting forced termination.
    pub const fn forced_shutdown_timeout(self) -> Duration {
        self.forced_shutdown_timeout
    }
}

impl Default for SessionManagerLimits {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(DEFAULT_MAX_SESSIONS).expect("non-zero default"),
            NonZeroUsize::new(DEFAULT_MAX_PENDING_INPUT_BYTES).expect("non-zero default"),
            NonZeroUsize::new(DEFAULT_MAX_IO_BYTES_PER_POLL).expect("non-zero default"),
            Duration::from_millis(300),
            Duration::from_millis(500),
        )
    }
}

/// One independently owned terminal session.
pub struct ManagedSession<P: RuntimeProcess> {
    lifecycle: SessionLifecycle,
    terminal_size: TerminalSize,
    process: Option<P>,
    pending_input: VecDeque<u8>,
    max_pending_input_bytes: usize,
    max_io_bytes_per_poll: usize,
    graceful_shutdown_timeout: Duration,
    forced_shutdown_timeout: Duration,
    exit_status: Option<ProcessExit>,
}

impl<P: RuntimeProcess> ManagedSession<P> {
    fn new(
        id: SessionId,
        terminal_size: TerminalSize,
        process: P,
        limits: SessionManagerLimits,
    ) -> Result<Self, ServiceError> {
        let mut lifecycle = SessionLifecycle::new(id);
        lifecycle.transition(SessionLifecycleState::Starting)?;
        lifecycle.transition(SessionLifecycleState::Running)?;

        Ok(Self {
            lifecycle,
            terminal_size,
            process: Some(process),
            pending_input: VecDeque::new(),
            max_pending_input_bytes: limits.max_pending_input_bytes(),
            max_io_bytes_per_poll: limits.max_io_bytes_per_poll(),
            graceful_shutdown_timeout: limits.graceful_shutdown_timeout(),
            forced_shutdown_timeout: limits.forced_shutdown_timeout(),
            exit_status: None,
        })
    }

    /// Number of bytes retained in this session's bounded input queue.
    pub fn pending_input_bytes(&self) -> usize {
        self.pending_input.len()
    }

    fn require_running(&self, operation: &'static str) -> Result<(), ServiceError> {
        if self.lifecycle.state() != SessionLifecycleState::Running {
            return Err(ServiceError::new(format!(
                "cannot {operation} terminal session in {:?} state",
                self.lifecycle.state()
            )));
        }
        Ok(())
    }

    fn poll_process_exit(&mut self) -> Result<Option<ProcessExit>, ServiceError> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }
        let Some(process) = self.process.as_mut() else {
            return Ok(None);
        };
        let Some(status) = process.try_wait()? else {
            return Ok(None);
        };

        self.exit_status = Some(status);
        if self.lifecycle.state() == SessionLifecycleState::Running {
            self.lifecycle.transition(SessionLifecycleState::Exited)?;
        }
        Ok(Some(status))
    }

    fn wait_for_exit(
        process: &mut P,
        timeout: Duration,
    ) -> Result<Option<ProcessExit>, ServiceError> {
        let started = Instant::now();
        loop {
            if let Some(status) = process.try_wait()? {
                return Ok(Some(status));
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Ok(None);
            }
            thread::sleep(SHUTDOWN_POLL_INTERVAL.min(timeout.saturating_sub(elapsed)));
        }
    }

    fn push_failure(failures: &mut Vec<String>, operation: &str, error: ServiceError) {
        failures.push(format!("{operation}: {error}"));
    }
}

impl<P: RuntimeProcess> TerminalSession for ManagedSession<P> {
    fn id(&self) -> SessionId {
        self.lifecycle.id()
    }

    fn state(&self) -> SessionLifecycleState {
        self.lifecycle.state()
    }

    fn terminal_size(&self) -> TerminalSize {
        self.terminal_size
    }

    fn send_input(&mut self, input: &[u8]) -> Result<(), ServiceError> {
        self.require_running("queue input for")?;
        let new_length = self
            .pending_input
            .len()
            .checked_add(input.len())
            .ok_or_else(|| ServiceError::new("session input queue size overflow"))?;
        if new_length > self.max_pending_input_bytes {
            return Err(ServiceError::new("session input queue capacity exceeded"));
        }
        self.pending_input
            .try_reserve(input.len())
            .map_err(|_| ServiceError::new("could not allocate session input queue"))?;
        self.pending_input.extend(input.iter().copied());
        Ok(())
    }

    fn flush_input(&mut self) -> Result<usize, ServiceError> {
        self.require_running("flush input for")?;
        let mut total_written = 0usize;
        let Some(process) = self.process.as_mut() else {
            return Err(ServiceError::new("session process handle is unavailable"));
        };

        while total_written < self.max_io_bytes_per_poll && !self.pending_input.is_empty() {
            let (first, second) = self.pending_input.as_slices();
            let available = if first.is_empty() { second } else { first };
            let budget = (self.max_io_bytes_per_poll - total_written).min(available.len());
            let written = process.write(&available[..budget])?;
            if written > budget {
                return Err(ServiceError::new(
                    "runtime reported a write count larger than the supplied buffer",
                ));
            }
            if written == 0 {
                break;
            }
            self.pending_input.drain(..written);
            total_written += written;
        }

        Ok(total_written)
    }

    fn read_output(&mut self, buffer: &mut [u8]) -> Result<ProcessRead, ServiceError> {
        if !matches!(
            self.lifecycle.state(),
            SessionLifecycleState::Running | SessionLifecycleState::Exited
        ) {
            return Err(ServiceError::new(format!(
                "cannot read terminal output in {:?} state",
                self.lifecycle.state()
            )));
        }
        if buffer.is_empty() {
            return Err(ServiceError::new(
                "terminal output buffer must not be empty",
            ));
        }
        let process = self
            .process
            .as_mut()
            .ok_or_else(|| ServiceError::new("session process handle is unavailable"))?;
        // Do not poll child status after a successful read: a wait error must
        // never turn already-read output bytes into a lost error result.
        process.read(buffer)
    }

    fn poll_exit(&mut self) -> Result<Option<ProcessExit>, ServiceError> {
        self.poll_process_exit()
    }

    fn exit_status(&self) -> Option<ProcessExit> {
        self.exit_status
    }

    fn resize(&mut self, size: TerminalSize) -> Result<(), ServiceError> {
        self.require_running("resize")?;
        let process = self
            .process
            .as_mut()
            .ok_or_else(|| ServiceError::new("session process handle is unavailable"))?;
        process.resize(size)?;
        // Commit the cached dimensions only after the PTY accepts the resize.
        self.terminal_size = size;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ServiceError> {
        if self.lifecycle.state() == SessionLifecycleState::Closed {
            return Ok(());
        }

        match self.lifecycle.state() {
            SessionLifecycleState::Created => {
                self.lifecycle.transition(SessionLifecycleState::Starting)?;
                self.lifecycle.transition(SessionLifecycleState::Stopping)?;
            }
            SessionLifecycleState::Starting
            | SessionLifecycleState::Running
            | SessionLifecycleState::Exited
            | SessionLifecycleState::Failed => {
                self.lifecycle.transition(SessionLifecycleState::Stopping)?;
            }
            SessionLifecycleState::Stopping => {}
            SessionLifecycleState::Closed => return Ok(()),
        }

        let mut failures = Vec::new();
        if let Some(process) = self.process.as_mut() {
            match process.try_wait() {
                // An already-exited child needs no stdin close or termination
                // request; those operations may legitimately return ECHILD.
                Ok(Some(status)) => self.exit_status = Some(status),
                Ok(None) => {
                    if let Err(error) = process.close_input() {
                        Self::push_failure(&mut failures, "close session input", error);
                    }
                    if let Err(error) = process.terminate(TerminationMode::Graceful) {
                        Self::push_failure(&mut failures, "request graceful termination", error);
                    }
                    match Self::wait_for_exit(process, self.graceful_shutdown_timeout) {
                        Ok(Some(status)) => self.exit_status = Some(status),
                        Ok(None) => {
                            if let Err(error) = process.terminate(TerminationMode::Force) {
                                Self::push_failure(&mut failures, "force termination", error);
                            }
                            match Self::wait_for_exit(process, self.forced_shutdown_timeout) {
                                Ok(Some(status)) => self.exit_status = Some(status),
                                Ok(None) => failures.push(
                                    "child did not report exit before the forced-shutdown deadline; \
                                     dropping the kill-on-drop runtime handle"
                                        .to_owned(),
                                ),
                                Err(error) => Self::push_failure(
                                    &mut failures,
                                    "wait after force termination",
                                    error,
                                ),
                            }
                        }
                        Err(error) => {
                            Self::push_failure(
                                &mut failures,
                                "wait after graceful termination",
                                error,
                            );
                            if let Err(force_error) = process.terminate(TerminationMode::Force) {
                                Self::push_failure(&mut failures, "force termination", force_error);
                            }
                            if let Err(wait_error) =
                                Self::wait_for_exit(process, self.forced_shutdown_timeout)
                            {
                                Self::push_failure(
                                    &mut failures,
                                    "wait after force termination",
                                    wait_error,
                                );
                            }
                        }
                    }
                }
                Err(error) => {
                    Self::push_failure(&mut failures, "poll child before shutdown", error);
                    if let Err(close_error) = process.close_input() {
                        Self::push_failure(&mut failures, "close session input", close_error);
                    }
                    if let Err(force_error) = process.terminate(TerminationMode::Force) {
                        Self::push_failure(&mut failures, "force termination", force_error);
                    }
                    if let Err(wait_error) =
                        Self::wait_for_exit(process, self.forced_shutdown_timeout)
                    {
                        Self::push_failure(
                            &mut failures,
                            "wait after force termination",
                            wait_error,
                        );
                    }
                }
            }
        }

        // Runtime handles are created with kill-on-drop enabled. Dropping the
        // handle is the final orphan-prevention boundary after bounded waits.
        self.process.take();
        self.pending_input.clear();
        self.lifecycle.transition(SessionLifecycleState::Closed)?;

        if failures.is_empty() {
            Ok(())
        } else {
            Err(ServiceError::new(format!(
                "session {} closed with {} cleanup warning(s): {}",
                self.id().get(),
                failures.len(),
                failures.join("; ")
            )))
        }
    }
}

impl<P: RuntimeProcess> Drop for ManagedSession<P> {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            // The runtime handle also enforces kill-on-drop. Explicit force
            // termination covers adapters whose cleanup begins asynchronously.
            let _ = process.terminate(TerminationMode::Force);
            drop(process);
        }
    }
}

type CloseWorkerCounter = Arc<(Mutex<usize>, Condvar)>;

struct CloseWorkerGuard(CloseWorkerCounter);

impl Drop for CloseWorkerGuard {
    fn drop(&mut self) {
        let (lock, wake) = &*self.0;
        let mut active = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = active.saturating_sub(1);
        wake.notify_all();
    }
}

/// Owns all terminal sessions and the runtime service used to create them.
pub struct SessionManager<R: RuntimeService> {
    runtime: R,
    limits: SessionManagerLimits,
    accepting_sessions: bool,
    runtime_initialized: bool,
    next_session_id: u64,
    sessions: HashMap<SessionId, ManagedSession<R::Process>>,
    closing_workers: CloseWorkerCounter,
}

impl<R: RuntimeService> SessionManager<R> {
    pub fn new(runtime: R) -> Self {
        Self::with_limits(runtime, SessionManagerLimits::default())
    }

    pub fn with_limits(runtime: R, limits: SessionManagerLimits) -> Self {
        Self {
            runtime,
            limits,
            accepting_sessions: false,
            runtime_initialized: false,
            next_session_id: 1,
            sessions: HashMap::new(),
            closing_workers: Arc::new((Mutex::new(0), Condvar::new())),
        }
    }

    /// Returns the immutable resource limits used by this manager.
    pub const fn limits(&self) -> SessionManagerLimits {
        self.limits
    }

    /// Starts session shutdown on a dedicated worker and returns its join handle.
    ///
    /// This is the non-blocking entry point for GUI/event-loop callers. The
    /// session is removed from the active registry immediately; the returned
    /// handle can be polled with `is_finished` and joined only after completion.
    /// Dropping the handle detaches the worker, but does not cancel cleanup.
    pub fn close_session_async(
        &mut self,
        id: SessionId,
    ) -> Result<thread::JoinHandle<Result<(), ServiceError>>, ServiceError>
    where
        R::Process: 'static,
    {
        let session = self
            .sessions
            .remove(&id)
            .ok_or_else(|| ServiceError::new("terminal session does not exist"))?;
        let counter = Arc::clone(&self.closing_workers);
        {
            let (lock, _) = &*counter;
            let mut active = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *active += 1;
        }

        let worker_counter = Arc::clone(&counter);
        match thread::Builder::new()
            .name(format!("de-session-close-{}", id.get()))
            .spawn(move || {
                let _guard = CloseWorkerGuard(worker_counter);
                let mut session = session;
                session.shutdown()
            })
        {
            Ok(handle) => Ok(handle),
            Err(_) => {
                let (lock, wake) = &*counter;
                let mut active = lock
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *active = active.saturating_sub(1);
                wake.notify_all();
                Err(ServiceError::new("could not start session shutdown worker"))
            }
        }
    }

    fn wait_for_closing_workers(&self) {
        let (lock, wake) = &*self.closing_workers;
        let mut active = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *active > 0 {
            active = wake
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Borrows a managed session by identity.
    pub fn session(&self, id: SessionId) -> Result<&ManagedSession<R::Process>, ServiceError> {
        self.sessions
            .get(&id)
            .ok_or_else(|| ServiceError::new("terminal session does not exist"))
    }

    /// Mutably borrows a managed session by identity.
    pub fn session_mut(
        &mut self,
        id: SessionId,
    ) -> Result<&mut ManagedSession<R::Process>, ServiceError> {
        self.sessions
            .get_mut(&id)
            .ok_or_else(|| ServiceError::new("terminal session does not exist"))
    }

    fn allocate_session_id(&mut self) -> Result<SessionId, ServiceError> {
        let value = self.next_session_id;
        let id = SessionId::new(value)
            .ok_or_else(|| ServiceError::new("terminal session identifier space exhausted"))?;
        self.next_session_id = value.checked_add(1).unwrap_or(0);
        Ok(id)
    }

    fn shutdown_sessions(&mut self) -> Vec<String> {
        let ids = self.session_ids();
        let mut failures = Vec::new();

        for id in ids {
            if let Some(session) = self.sessions.get_mut(&id) {
                if let Err(error) = session.shutdown() {
                    failures.push(format!("session {}: {error}", id.get()));
                }
            }
            if self
                .sessions
                .get(&id)
                .is_some_and(|session| session.state() == SessionLifecycleState::Closed)
            {
                self.sessions.remove(&id);
            }
        }

        failures
    }
}

impl<R: RuntimeService> ApplicationService for SessionManager<R> {
    fn name(&self) -> &str {
        "terminal-session-manager"
    }

    fn initialize(&mut self) -> Result<(), ServiceError> {
        if self.runtime_initialized {
            return Err(ServiceError::new("session manager is already initialized"));
        }
        self.runtime.initialize()?;
        self.runtime_initialized = true;
        self.accepting_sessions = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ServiceError> {
        self.accepting_sessions = false;
        let mut failures = self.shutdown_sessions();
        // Async session-close workers own process handles outside the registry.
        // Wait for them before tearing down the runtime service.
        self.wait_for_closing_workers();

        if self.runtime_initialized {
            match self.runtime.shutdown() {
                Ok(()) => self.runtime_initialized = false,
                Err(error) => failures.push(format!("runtime service: {error}")),
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(ServiceError::new(format!(
                "session manager shutdown completed with {} cleanup failure(s): {}",
                failures.len(),
                failures.join("; ")
            )))
        }
    }
}

impl<R: RuntimeService> SessionManagerService for SessionManager<R> {
    type Session = ManagedSession<R::Process>;

    fn create_session(&mut self, spec: SessionSpec) -> Result<SessionId, ServiceError> {
        if !self.accepting_sessions {
            return Err(ServiceError::new(
                "session manager is not accepting sessions",
            ));
        }
        if self.sessions.len() >= self.limits.max_sessions() {
            return Err(ServiceError::new("maximum terminal session count reached"));
        }
        if spec.kind() != SessionKind::ExternalProgram {
            return Err(ServiceError::new(
                "DE-Shell sessions require the dedicated FFI session backend",
            ));
        }
        spec.program()
            .validate()
            .map_err(|error| ServiceError::new(format!("invalid session program: {error}")))?;

        let id = self.allocate_session_id()?;
        let process = self.runtime.spawn(spec.program(), spec.terminal_size())?;
        let session = ManagedSession::new(id, spec.terminal_size(), process, self.limits)?;
        self.sessions.insert(id, session);
        Ok(id)
    }

    fn session_state(&self, id: SessionId) -> Result<SessionLifecycleState, ServiceError> {
        self.sessions
            .get(&id)
            .map(|session| session.state())
            .ok_or_else(|| ServiceError::new("terminal session does not exist"))
    }

    fn session_mut(&mut self, id: SessionId) -> Result<&mut Self::Session, ServiceError> {
        self.sessions
            .get_mut(&id)
            .ok_or_else(|| ServiceError::new("terminal session does not exist"))
    }

    fn session_ids(&self) -> Vec<SessionId> {
        let mut ids: Vec<_> = self.sessions.keys().copied().collect();
        ids.sort_by_key(|id| id.get());
        ids
    }

    fn close_session(&mut self, id: SessionId) -> Result<(), ServiceError> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| ServiceError::new("terminal session does not exist"))?;
        let result = session.shutdown();
        if session.state() == SessionLifecycleState::Closed {
            self.sessions.remove(&id);
        }
        result
    }

    fn close_session_async(
        &mut self,
        id: SessionId,
    ) -> Result<thread::JoinHandle<Result<(), ServiceError>>, ServiceError>
    where
        Self::Session: Send + 'static,
    {
        SessionManager::close_session_async(self, id)
    }
}

impl<R: RuntimeService> Drop for SessionManager<R> {
    fn drop(&mut self) {
        if self.runtime_initialized || !self.sessions.is_empty() {
            let _ = <Self as ApplicationService>::shutdown(self);
        }
    }
}
