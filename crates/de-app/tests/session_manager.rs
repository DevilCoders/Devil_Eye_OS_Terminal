use std::collections::VecDeque;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use de_app::lifecycle::{ApplicationService, ServiceError};
use de_app::service_interfaces::{
    ProcessExit, ProcessRead, RuntimeProcess, RuntimeService, SessionKind, SessionLifecycleState,
    SessionManagerService, SessionSpec, TerminalSession, TerminationMode,
};
use de_app::{SessionManager, SessionManagerLimits};
use de_runtime::ProgramSpec;
use de_terminal_core::TerminalSize;

#[derive(Debug, Default)]
struct MockProcessState {
    input: Vec<u8>,
    output: VecDeque<u8>,
    exit: Option<ProcessExit>,
    termination_modes: Vec<TerminationMode>,
    input_closed: bool,
    resized: Option<TerminalSize>,
    dropped: bool,
    write_blocked: bool,
    resize_fails: bool,
    graceful_terminates: bool,
    wait_fails: bool,
    shutdown_thread: Option<thread::ThreadId>,
}

#[derive(Debug, Default)]
struct MockRuntimeState {
    initialized: bool,
    fail_spawn: bool,
    spawn_count: usize,
    shutdown_count: usize,
    processes: Vec<Arc<Mutex<MockProcessState>>>,
}

#[derive(Clone)]
struct MockRuntime {
    state: Arc<Mutex<MockRuntimeState>>,
}

impl MockRuntime {
    fn new(state: Arc<Mutex<MockRuntimeState>>) -> Self {
        Self { state }
    }
}

impl ApplicationService for MockRuntime {
    fn name(&self) -> &str {
        "mock-runtime"
    }

    fn initialize(&mut self) -> Result<(), ServiceError> {
        self.state.lock().unwrap().initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ServiceError> {
        let mut state = self.state.lock().unwrap();
        state.shutdown_count += 1;
        state.initialized = false;
        Ok(())
    }
}

struct MockProcess {
    state: Arc<Mutex<MockProcessState>>,
}

impl RuntimeProcess for MockProcess {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, ServiceError> {
        let mut state = self.state.lock().unwrap();
        if state.write_blocked {
            return Ok(0);
        }
        let count = bytes.len().min(2);
        state.input.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<ProcessRead, ServiceError> {
        if buffer.is_empty() {
            return Err(ServiceError::new("empty mock read buffer"));
        }
        let mut state = self.state.lock().unwrap();
        if state.output.is_empty() {
            return Ok(if state.exit.is_some() {
                ProcessRead::Eof
            } else {
                ProcessRead::WouldBlock
            });
        }
        let count = buffer.len().min(state.output.len());
        for slot in &mut buffer[..count] {
            *slot = state
                .output
                .pop_front()
                .expect("count is bounded by queue length");
        }
        Ok(ProcessRead::Data(
            NonZeroUsize::new(count).expect("non-empty output"),
        ))
    }

    fn close_input(&mut self) -> Result<(), ServiceError> {
        self.state.lock().unwrap().input_closed = true;
        Ok(())
    }

    fn resize(&mut self, size: TerminalSize) -> Result<(), ServiceError> {
        let mut state = self.state.lock().unwrap();
        if state.resize_fails {
            return Err(ServiceError::new("mock resize failure"));
        }
        state.resized = Some(size);
        Ok(())
    }

    fn terminate(&mut self, mode: TerminationMode) -> Result<(), ServiceError> {
        let mut state = self.state.lock().unwrap();
        state.termination_modes.push(mode);
        match mode {
            TerminationMode::Graceful if state.graceful_terminates => {
                state.exit = Some(ProcessExit::Exited { code: 0 });
            }
            TerminationMode::Force => {
                state.exit = Some(ProcessExit::Exited { code: 137 });
            }
            TerminationMode::Graceful => {}
        }
        Ok(())
    }

    fn try_wait(&mut self) -> Result<Option<ProcessExit>, ServiceError> {
        let mut state = self.state.lock().unwrap();
        state.shutdown_thread = Some(thread::current().id());
        if state.wait_fails {
            return Err(ServiceError::new("mock wait failure"));
        }
        Ok(state.exit)
    }
}

impl Drop for MockProcess {
    fn drop(&mut self) {
        self.state.lock().unwrap().dropped = true;
    }
}

impl RuntimeService for MockRuntime {
    type Process = MockProcess;

    fn spawn(
        &mut self,
        _program: &ProgramSpec,
        _terminal_size: TerminalSize,
    ) -> Result<Self::Process, ServiceError> {
        let mut runtime = self.state.lock().unwrap();
        runtime.spawn_count += 1;
        if !runtime.initialized {
            return Err(ServiceError::new("mock runtime is not initialized"));
        }
        if runtime.fail_spawn {
            return Err(ServiceError::new("mock spawn failure"));
        }
        let process = Arc::new(Mutex::new(MockProcessState {
            graceful_terminates: true,
            ..MockProcessState::default()
        }));
        runtime.processes.push(Arc::clone(&process));
        Ok(MockProcess { state: process })
    }
}

fn size(columns: u16, rows: u16) -> TerminalSize {
    TerminalSize::new(columns, rows).expect("valid terminal size")
}

fn spec(kind: SessionKind) -> SessionSpec {
    SessionSpec::new(
        kind,
        ProgramSpec::new("/bin/sh").expect("valid executable"),
        size(100, 32),
    )
}

fn limits(max_sessions: usize, max_input: usize, io_budget: usize) -> SessionManagerLimits {
    SessionManagerLimits::new(
        NonZeroUsize::new(max_sessions).expect("non-zero session limit"),
        NonZeroUsize::new(max_input).expect("non-zero input limit"),
        NonZeroUsize::new(io_budget).expect("non-zero I/O budget"),
        Duration::ZERO,
        Duration::ZERO,
    )
}

fn initialized_manager(
    state: Arc<Mutex<MockRuntimeState>>,
    limits: SessionManagerLimits,
) -> SessionManager<MockRuntime> {
    let mut manager = SessionManager::with_limits(MockRuntime::new(state), limits);
    manager.initialize().expect("initialize session manager");
    manager
}

#[test]
fn creates_independent_session_queues_input_and_closes_owned_process() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(4, 1024, 1024));
    let id = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("create session");
    let process_state = Arc::clone(&state.lock().unwrap().processes[0]);

    assert_eq!(
        manager.session_state(id).expect("session state"),
        SessionLifecycleState::Running
    );
    let session = manager.session_mut(id).expect("session handle");
    session.send_input(b"hello").expect("queue input");
    assert_eq!(session.pending_input_bytes(), 5);
    assert_eq!(session.flush_input().expect("flush input"), 5);
    assert_eq!(session.pending_input_bytes(), 0);
    assert_eq!(process_state.lock().unwrap().input, b"hello");

    manager.close_session(id).expect("close session");
    let process = process_state.lock().unwrap();
    assert!(process.input_closed);
    assert!(process
        .termination_modes
        .contains(&TerminationMode::Graceful));
    assert!(process.dropped);
    assert!(manager.session_state(id).is_err());
}

#[test]
fn asynchronous_close_runs_blocking_shutdown_on_a_worker_thread() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(2, 1024, 1024));
    let id = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("create session");
    let process_state = Arc::clone(&state.lock().unwrap().processes[0]);
    let caller_thread = thread::current().id();

    let shutdown = SessionManagerService::close_session_async(&mut manager, id)
        .expect("start asynchronous close");

    assert!(manager.session_state(id).is_err());
    shutdown
        .join()
        .expect("shutdown worker must not panic")
        .expect("session shutdown must complete");
    assert_ne!(
        process_state.lock().unwrap().shutdown_thread,
        Some(caller_thread),
        "blocking process cleanup must run away from the caller thread"
    );
    assert!(process_state.lock().unwrap().dropped);
}

#[test]
fn observed_child_exit_transitions_session_and_rejects_new_input() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(4, 1024, 1024));
    let id = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("create session");
    let process_state = Arc::clone(&state.lock().unwrap().processes[0]);
    process_state.lock().unwrap().exit = Some(ProcessExit::Exited { code: 7 });

    let session = manager.session_mut(id).expect("session handle");
    assert_eq!(
        session.poll_exit().expect("poll child"),
        Some(ProcessExit::Exited { code: 7 })
    );
    assert_eq!(session.state(), SessionLifecycleState::Exited);
    assert!(session.send_input(b"not accepted").is_err());

    manager.close_session(id).expect("close exited session");
    assert!(process_state.lock().unwrap().dropped);
}

#[test]
fn output_bytes_are_returned_even_if_child_status_polling_would_fail() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(2, 1024, 1024));
    let id = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("create session");
    let process_state = Arc::clone(&state.lock().unwrap().processes[0]);
    {
        let mut process = process_state.lock().unwrap();
        process.output.extend(b"preserve-output");
        process.wait_fails = true;
    }

    let mut buffer = [0u8; 32];
    let result = manager
        .session_mut(id)
        .expect("session")
        .read_output(&mut buffer)
        .expect("read must not be coupled to wait polling");
    let ProcessRead::Data(count) = result else {
        panic!("expected output bytes, got {result:?}");
    };
    assert_eq!(&buffer[..count.get()], b"preserve-output");
}

#[test]
fn application_shutdown_closes_every_session_before_runtime_shutdown() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(4, 1024, 1024));
    let first = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("first");
    let second = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("second");
    assert_eq!(manager.session_ids(), vec![first, second]);

    manager.shutdown().expect("shutdown all sessions");

    let runtime = state.lock().unwrap();
    assert_eq!(runtime.shutdown_count, 1);
    assert!(!runtime.initialized);
    assert!(runtime
        .processes
        .iter()
        .all(|process| process.lock().unwrap().dropped));
    assert!(manager.session_ids().is_empty());
}

#[test]
fn failed_spawn_does_not_publish_a_partial_session() {
    let state = Arc::new(Mutex::new(MockRuntimeState {
        fail_spawn: true,
        ..MockRuntimeState::default()
    }));
    let mut manager = initialized_manager(Arc::clone(&state), limits(4, 1024, 1024));

    assert!(manager
        .create_session(spec(SessionKind::ExternalProgram))
        .is_err());
    assert!(manager.session_ids().is_empty());
    assert_eq!(state.lock().unwrap().spawn_count, 1);
}

#[test]
fn manager_drop_forces_and_drops_any_remaining_child_handle() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(4, 1024, 1024));
    let id = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("create");
    let process_state = Arc::clone(&state.lock().unwrap().processes[0]);
    process_state.lock().unwrap().graceful_terminates = false;

    drop(manager);

    let process = process_state.lock().unwrap();
    assert!(process.termination_modes.contains(&TerminationMode::Force));
    assert!(process.dropped);
    assert_eq!(state.lock().unwrap().shutdown_count, 1);
    let _ = id;
}

#[test]
fn session_input_queue_is_bounded_and_preserves_bytes_under_backpressure() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(1, 3, 2));
    let id = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("create");
    let process_state = Arc::clone(&state.lock().unwrap().processes[0]);

    let session = manager.session_mut(id).expect("session handle");
    assert!(session.send_input(b"four").is_err());
    session.send_input(b"abc").expect("queue within limit");
    process_state.lock().unwrap().write_blocked = true;
    assert_eq!(session.flush_input().expect("would block"), 0);
    assert_eq!(session.pending_input_bytes(), 3);

    process_state.lock().unwrap().write_blocked = false;
    assert_eq!(session.flush_input().expect("first bounded flush"), 2);
    assert_eq!(session.pending_input_bytes(), 1);
    assert_eq!(session.flush_input().expect("second flush"), 1);
    assert_eq!(session.pending_input_bytes(), 0);
    assert_eq!(process_state.lock().unwrap().input, b"abc");
}

#[test]
fn resize_commits_cached_size_only_after_runtime_success() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(2, 1024, 1024));
    let id = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("create");
    let process_state = Arc::clone(&state.lock().unwrap().processes[0]);
    let original = size(100, 32);
    let resized = size(120, 40);

    process_state.lock().unwrap().resize_fails = true;
    assert!(manager.session_mut(id).unwrap().resize(resized).is_err());
    assert_eq!(manager.session(id).unwrap().terminal_size(), original);

    process_state.lock().unwrap().resize_fails = false;
    manager
        .session_mut(id)
        .unwrap()
        .resize(resized)
        .expect("resize");
    assert_eq!(manager.session(id).unwrap().terminal_size(), resized);
}

#[test]
fn rejects_unwired_de_shell_backend_without_spawning_a_process() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(2, 1024, 1024));

    assert!(manager.create_session(spec(SessionKind::DeShell)).is_err());
    assert_eq!(state.lock().unwrap().spawn_count, 0);
    assert!(manager.session_ids().is_empty());
}

#[test]
fn session_identifiers_are_never_reused_after_close() {
    let state = Arc::new(Mutex::new(MockRuntimeState::default()));
    let mut manager = initialized_manager(Arc::clone(&state), limits(2, 1024, 1024));
    let first = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("first");
    manager.close_session(first).expect("close first");
    let second = manager
        .create_session(spec(SessionKind::ExternalProgram))
        .expect("second");

    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 2);
    assert_ne!(first, second);
    assert_eq!(NonZeroU64::new(second.get()).unwrap().get(), 2);
}
