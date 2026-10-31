#![cfg(target_os = "linux")]

use std::thread;
use std::time::{Duration, Instant};

use de_app::lifecycle::ApplicationService;
use de_app::service_interfaces::{
    ProcessRead, SessionKind, SessionManagerService, SessionSpec, TerminalSession,
};
use de_app::LinuxRuntimeService;
use de_app::SessionManager;
use de_runtime::ProgramSpec;
use de_terminal_core::TerminalSize;

#[test]
fn linux_session_manager_owns_a_real_pty_child_until_close() {
    let size = TerminalSize::new(80, 24).expect("valid PTY size");
    let program = ProgramSpec::new("/usr/bin/printf")
        .expect("printf executable")
        .with_arg("session-manager-ok");
    let spec = SessionSpec::new(SessionKind::ExternalProgram, program, size);
    let mut manager = SessionManager::new(LinuxRuntimeService::new());
    manager.initialize().expect("initialize Linux runtime");

    let id = manager.create_session(spec).expect("spawn PTY session");
    let session = manager.session_mut(id).expect("session exists");
    assert_eq!(session.terminal_size(), size);

    let mut output = Vec::new();
    let mut buffer = [0u8; 128];
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_eof = false;

    while Instant::now() < deadline {
        match session
            .read_output(&mut buffer)
            .expect("non-blocking PTY read")
        {
            ProcessRead::Data(count) => {
                output.extend_from_slice(&buffer[..count.get()]);
            }
            ProcessRead::WouldBlock => {
                let _ = session.poll_exit().expect("poll child exit");
                thread::sleep(Duration::from_millis(10));
            }
            ProcessRead::Eof => {
                saw_eof = true;
                break;
            }
        }
    }

    assert!(saw_eof, "PTY output did not reach EOF before the deadline");
    assert!(
        output
            .windows(b"session-manager-ok".len())
            .any(|window| window == b"session-manager-ok"),
        "expected marker in PTY output, got {output:?}"
    );

    manager.close_session(id).expect("close PTY session");
    manager.shutdown().expect("shutdown Linux runtime");
    assert!(manager.session_ids().is_empty());
}
