use std::num::NonZeroUsize;

use de_app::service_interfaces::{
    ProcessExit, ProcessRead, SessionId, SessionKind, SessionLifecycle,
    SessionLifecycleState, SessionSpec,
};
use de_runtime::ProgramSpec;
use de_terminal_core::TerminalSize;

#[test]
fn session_ids_reject_zero_and_preserve_non_zero_identity() {
    assert_eq!(SessionId::new(0), None);
    assert_eq!(SessionId::new(42).expect("non-zero id").get(), 42);
}

#[test]
fn session_lifecycle_accepts_the_normal_exit_path() {
    let id = SessionId::new(7).expect("non-zero id");
    let mut lifecycle = SessionLifecycle::new(id);

    assert_eq!(lifecycle.state(), SessionLifecycleState::Created);
    lifecycle
        .transition(SessionLifecycleState::Starting)
        .expect("created to starting");
    lifecycle
        .transition(SessionLifecycleState::Running)
        .expect("starting to running");
    lifecycle
        .transition(SessionLifecycleState::Exited)
        .expect("running to exited");
    lifecycle
        .transition(SessionLifecycleState::Stopping)
        .expect("exited to stopping");
    lifecycle
        .transition(SessionLifecycleState::Closed)
        .expect("stopping to closed");

    assert_eq!(lifecycle.id(), id);
    assert_eq!(lifecycle.state(), SessionLifecycleState::Closed);
}

#[test]
fn session_lifecycle_accepts_startup_failure_cleanup() {
    let id = SessionId::new(8).expect("non-zero id");
    let mut lifecycle = SessionLifecycle::new(id);

    lifecycle
        .transition(SessionLifecycleState::Starting)
        .expect("created to starting");
    lifecycle
        .transition(SessionLifecycleState::Failed)
        .expect("starting to failed");
    lifecycle
        .transition(SessionLifecycleState::Stopping)
        .expect("failed to stopping");
    lifecycle
        .transition(SessionLifecycleState::Closed)
        .expect("stopping to closed");
}

#[test]
fn illegal_session_transition_is_rejected_without_mutating_state() {
    let id = SessionId::new(9).expect("non-zero id");
    let mut lifecycle = SessionLifecycle::new(id);

    let error = lifecycle
        .transition(SessionLifecycleState::Running)
        .expect_err("created cannot jump directly to running");

    assert_eq!(
        error.message(),
        "terminal session lifecycle transition is invalid"
    );
    assert_eq!(lifecycle.state(), SessionLifecycleState::Created);
}

#[test]
fn closed_session_cannot_be_reopened() {
    let id = SessionId::new(10).expect("non-zero id");
    let mut lifecycle = SessionLifecycle::new(id);

    lifecycle
        .transition(SessionLifecycleState::Starting)
        .expect("created to starting");
    lifecycle
        .transition(SessionLifecycleState::Failed)
        .expect("starting to failed");
    lifecycle
        .transition(SessionLifecycleState::Stopping)
        .expect("failed to stopping");
    lifecycle
        .transition(SessionLifecycleState::Closed)
        .expect("stopping to closed");
    assert!(lifecycle
        .transition(SessionLifecycleState::Starting)
        .is_err());
}

#[test]
fn session_spec_preserves_separate_program_arguments_and_terminal_size() {
    let size = TerminalSize::new(120, 40).expect("valid terminal size");
    let program = ProgramSpec::new("/usr/bin/example")
        .expect("valid program")
        .with_arg("argument with spaces")
        .with_arg("--literal");

    let spec = SessionSpec::new(SessionKind::ExternalProgram, program.clone(), size);

    assert_eq!(spec.kind(), SessionKind::ExternalProgram);
    assert_eq!(spec.program(), &program);
    assert_eq!(spec.program().args().len(), 2);
    assert_eq!(spec.terminal_size(), size);
}

#[test]
fn process_read_distinguishes_data_backpressure_and_eof() {
    let read = ProcessRead::Data(NonZeroUsize::new(4).expect("non-zero read length"));
    match read {
        ProcessRead::Data(count) => assert_eq!(count.get(), 4),
        ProcessRead::WouldBlock | ProcessRead::Eof => panic!("expected data"),
    }
    assert_eq!(ProcessRead::WouldBlock, ProcessRead::WouldBlock);
    assert_eq!(ProcessRead::Eof, ProcessRead::Eof);
}

#[test]
fn process_exit_success_is_derived_from_exit_code() {
    assert!(ProcessExit::Exited { code: 0 }.success());
    assert!(!ProcessExit::Exited { code: 1 }.success());
    assert!(!ProcessExit::Signaled { signal: 9 }.success());
}
