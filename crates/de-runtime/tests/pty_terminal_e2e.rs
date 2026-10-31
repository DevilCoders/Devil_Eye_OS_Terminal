use de_runtime::{
    IoAttachment, LinuxRuntime, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec, PtyConfig,
    ReadOutcome, RuntimeError, RuntimeProcess, RuntimeService, Signal, TerminalSize, TerminationMode,
    WriteOutcome,
};
use de_terminal_core::parser::{TerminalEvent, VtParser};
use de_terminal_core::screen::{CellAttributes, CellContent, Color, GraphemeWidth, StyleFlags, TerminalScreen};
use std::num::NonZeroUsize;
use std::thread;
use std::time::{Duration, Instant};

fn terminal_size() -> TerminalSize {
    TerminalSize::new(80, 24).expect("valid terminal size")
}

fn spawn_pty(program: ProgramSpec) -> Box<dyn RuntimeProcess> {
    let request = ProcessSpawnRequest::new(
        program,
        IoAttachment::Pty(PtyConfig::new(terminal_size())),
    )
    .expect("valid PTY spawn request");
    LinuxRuntime.spawn(request).expect("spawn PTY process")
}

fn read_until_marker(
    process: &mut dyn RuntimeProcess,
    marker: &[u8],
) -> Result<Vec<u8>, RuntimeError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut collected = Vec::new();
    let mut buffer = [0u8; 1024];

    loop {
        match process.read_stdout(&mut buffer)? {
            ReadOutcome::Data(count) => {
                collected.extend_from_slice(&buffer[..count.get()]);
                if collected.windows(marker.len()).any(|window| window == marker) {
                    return Ok(collected);
                }
            }
            ReadOutcome::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(RuntimeError::WaitFailed);
                }
                thread::sleep(Duration::from_millis(2));
            }
            ReadOutcome::Eof => return Err(RuntimeError::ProcessAlreadyExited),
        }
    }
}

fn drain_until_exit(
    process: &mut dyn RuntimeProcess,
) -> Result<(Vec<u8>, ProcessExitStatus), RuntimeError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    let mut buffer = [0u8; 2048];
    let mut status = None;
    let mut eof = false;

    while status.is_none() || !eof {
        match process.read_stdout(&mut buffer)? {
            ReadOutcome::Data(count) => output.extend_from_slice(&buffer[..count.get()]),
            ReadOutcome::WouldBlock => {}
            ReadOutcome::Eof => eof = true,
        }
        if status.is_none() {
            status = process.try_wait()?;
        }
        if Instant::now() >= deadline {
            return Err(RuntimeError::WaitFailed);
        }
        if status.is_none() || !eof {
            thread::sleep(Duration::from_millis(1));
        }
    }

    Ok((output, status.expect("exit status checked")))
}

fn apply_vt_events(
    events: &[TerminalEvent],
    screen: &mut TerminalScreen,
    row: &mut u16,
    column: &mut u16,
    attributes: &mut CellAttributes,
) {
    for event in events {
        match event {
            TerminalEvent::PrintByte(byte) if (0x20..=0x7e).contains(byte) => {
                let value = char::from(*byte).to_string();
                screen
                    .set_grapheme(
                        *row,
                        *column,
                        &value,
                        GraphemeWidth::Single,
                        *attributes,
                    )
                    .expect("write parsed printable byte");
                *column = column.saturating_add(1).min(screen.size().columns() - 1);
                screen.set_cursor(*row, *column);
            }
            TerminalEvent::Execute(b'\r') => {
                *column = 0;
                screen.set_cursor(*row, *column);
            }
            TerminalEvent::Execute(b'\n') => {
                *row = row.saturating_add(1).min(screen.size().rows() - 1);
                screen.set_cursor(*row, *column);
            }
            TerminalEvent::Csi(sequence) if sequence.final_byte() == b'm' => {
                match sequence.parameter_bytes() {
                    b"31" => {
                        *attributes = CellAttributes::new(
                            Color::Indexed(1),
                            Color::Default,
                            StyleFlags::empty(),
                        );
                    }
                    b"" | b"0" => *attributes = CellAttributes::default(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

#[test]
fn pty_output_parses_into_terminal_screen_with_sgr_attributes() {
    let program = ProgramSpec::new("/bin/sh")
        .expect("program")
        .with_arg("-c")
        .with_arg("printf '\\033[31mHello\\033[0m\\r\\n'");
    let mut process = spawn_pty(program);
    let (output, status) = drain_until_exit(process.as_mut()).expect("drain terminal output");

    assert_eq!(status, ProcessExitStatus::Exited(0));

    let mut parser = VtParser::new();
    let mut events = Vec::new();
    for chunk in output.chunks(3) {
        events.extend(parser.process(chunk).expect("bounded VT input"));
    }
    assert!(parser.is_ground());

    let mut screen = TerminalScreen::new(terminal_size(), 80 * 24).expect("screen");
    let mut row = 0u16;
    let mut column = 0u16;
    let mut attributes = CellAttributes::default();
    apply_vt_events(
        &events,
        &mut screen,
        &mut row,
        &mut column,
        &mut attributes,
    );

    for (column, expected) in b"Hello".iter().enumerate() {
        let cell = screen
            .get_cell(0, u16::try_from(column).expect("column fits"))
            .expect("screen cell");
        let CellContent::Grapheme { value, .. } = cell.content() else {
            panic!("expected grapheme cell");
        };
        assert_eq!(value.as_str().as_bytes(), &[*expected]);
        assert_eq!(cell.attributes().foreground(), Color::Indexed(1));
    }
}

#[test]
fn pty_interactive_input_resize_and_exit_are_observable() {
    let program = ProgramSpec::new("/bin/sh")
        .expect("program")
        .with_arg("-c")
        .with_arg("stty raw -echo; printf READY; exec /bin/cat");
    let mut process = spawn_pty(program);
    let _ = read_until_marker(process.as_mut(), b"READY").expect("wait for raw terminal");

    let payload = b"terminal-input-through-pty";
    let mut offset = 0usize;
    let mut received = Vec::new();
    let mut buffer = [0u8; 128];
    let deadline = Instant::now() + Duration::from_secs(5);

    while received.len() < payload.len() {
        if offset < payload.len() {
            match process.write_stdin(&payload[offset..]).expect("write PTY input") {
                WriteOutcome::Written(count) => offset += count.get(),
                WriteOutcome::WouldBlock => {}
            }
        }
        match process.read_stdout(&mut buffer).expect("read PTY output") {
            ReadOutcome::Data(count) => received.extend_from_slice(&buffer[..count.get()]),
            ReadOutcome::WouldBlock => thread::sleep(Duration::from_millis(2)),
            ReadOutcome::Eof => panic!("PTY closed before input echo completed"),
        }
        assert!(Instant::now() < deadline, "interactive PTY I/O timed out");
    }

    assert_eq!(received, payload);
    process.terminate(TerminationMode::Force).expect("stop cat");
    let _ = process.wait();

    let size_reporter = ProgramSpec::new("/bin/sh")
        .expect("program")
        .with_arg("-c")
        .with_arg("sleep 0.1; stty size");
    let mut process = spawn_pty(size_reporter);
    let resized = TerminalSize::new(132, 43).expect("resized terminal");
    process.resize_pty(resized).expect("resize PTY");
    let (output, status) = drain_until_exit(process.as_mut()).expect("read child size");
    assert_eq!(status, ProcessExitStatus::Exited(0));
    assert!(String::from_utf8(output).unwrap().contains("43 132"));
}

#[test]
fn pty_signal_exit_and_drop_cleanup_do_not_leak_children() {
    let program = ProgramSpec::new("/bin/sleep")
        .expect("program")
        .with_arg("30");
    let mut process = spawn_pty(program);
    process
        .send_signal(Signal::Terminate)
        .expect("send SIGTERM to session group");
    assert_eq!(
        process.wait().expect("wait signaled child"),
        ProcessExitStatus::Signaled(Signal::Terminate)
    );

    let program = ProgramSpec::new("/bin/sleep")
        .expect("program")
        .with_arg("30");
    let process = spawn_pty(program);
    let pid = process.process_id().expect("child PID");
    drop(process);

    let status = std::process::Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .expect("execute kill -0 probe");
    assert!(!status.success(), "dropped PTY child PID is still alive");
}
