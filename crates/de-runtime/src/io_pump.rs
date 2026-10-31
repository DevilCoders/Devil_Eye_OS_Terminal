//! Bounded, cancellation-safe worker loop for one PTY-backed runtime process.
//!
//! The worker owns the process handle. Input and output channels are bounded;
//! a slow output consumer applies backpressure instead of allowing memory growth.

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{
    self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::Arc;
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use crate::{
    ProcessExitStatus, ReadOutcome, RuntimeError, RuntimeProcess, Signal, TerminalSize,
    TerminationMode, WriteOutcome,
};

/// Maximum bytes accepted in one input submission and read in one output event.
pub const MAX_IO_CHUNK_BYTES: usize = 4096;
const COMMAND_QUEUE_CAPACITY: usize = 16;
const EVENT_QUEUE_CAPACITY: usize = 8;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);

/// Events delivered from the PTY worker to its owning session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoPumpEvent {
    /// Ordered PTY output bytes. Each event contains at most MAX_IO_CHUNK_BYTES.
    Output(Vec<u8>),
    /// PTY output reached EOF. The child exit event may follow later.
    Eof,
    /// Child exit status, emitted after output EOF has been delivered.
    Exited(ProcessExitStatus),
    /// The kernel accepted a PTY resize request.
    Resized(TerminalSize),
    /// A runtime operation failed. Fatal I/O errors are followed by cleanup.
    Error(RuntimeError),
}

/// A command could not be queued without violating bounded backpressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoSubmitError {
    Backpressure,
    Closed,
    InputTooLarge { actual: usize, maximum: usize },
}

impl fmt::Display for IoSubmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backpressure => formatter.write_str("PTY worker command queue is full"),
            Self::Closed => formatter.write_str("PTY worker is closed"),
            Self::InputTooLarge { actual, maximum } => {
                write!(formatter, "input chunk of {actual} bytes exceeds {maximum}")
            }
        }
    }
}

impl Error for IoSubmitError {}

/// Failure while starting or joining the PTY worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoPumpError {
    WorkerSpawnFailed,
    WorkerPanicked,
}

impl fmt::Display for IoPumpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkerSpawnFailed => formatter.write_str("failed to start PTY I/O worker"),
            Self::WorkerPanicked => formatter.write_str("PTY I/O worker panicked"),
        }
    }
}

impl Error for IoPumpError {}

enum WorkerCommand {
    Write(Vec<u8>),
    Resize(TerminalSize),
    Signal(Signal),
    CloseStdin,
    Terminate(TerminationMode),
    Shutdown,
}

/// Owns one non-blocking runtime process and drives its PTY I/O on a worker
/// thread. The owner thread only submits bounded commands and receives events.
pub struct PtyIoPump {
    commands: SyncSender<WorkerCommand>,
    events: Receiver<IoPumpEvent>,
    cancelled: Arc<AtomicBool>,
    worker_thread: Thread,
    worker: Option<JoinHandle<()>>,
}

impl PtyIoPump {
    /// Starts a worker for one runtime-owned PTY process.
    pub fn new(process: Box<dyn RuntimeProcess>) -> Result<Self, IoPumpError> {
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);

        let worker = thread::Builder::new()
            .name("de-pty-io".to_owned())
            .spawn(move || worker_loop(process, command_rx, event_tx, worker_cancelled))
            .map_err(|_| IoPumpError::WorkerSpawnFailed)?;
        let worker_thread = worker.thread().clone();

        Ok(Self {
            commands: command_tx,
            events: event_rx,
            cancelled,
            worker_thread,
            worker: Some(worker),
        })
    }

    /// Queues one input chunk. On Backpressure, no bytes were accepted and the
    /// caller retains the original slice for a later retry.
    pub fn try_write(&self, input: &[u8]) -> Result<(), IoSubmitError> {
        if input.is_empty() {
            return Ok(());
        }
        if input.len() > MAX_IO_CHUNK_BYTES {
            return Err(IoSubmitError::InputTooLarge {
                actual: input.len(),
                maximum: MAX_IO_CHUNK_BYTES,
            });
        }
        self.submit(WorkerCommand::Write(input.to_vec()))
    }

    /// Queues a PTY resize. The result is reported as Resized or Error.
    pub fn resize_pty(&self, size: TerminalSize) -> Result<(), IoSubmitError> {
        self.submit(WorkerCommand::Resize(size))
    }

    /// Queues a semantic signal for this process session only.
    pub fn send_signal(&self, signal: Signal) -> Result<(), IoSubmitError> {
        self.submit(WorkerCommand::Signal(signal))
    }

    /// Queues stdin EOF. For PTYs, the runtime sends the configured VEOF byte.
    pub fn close_stdin(&self) -> Result<(), IoSubmitError> {
        self.submit(WorkerCommand::CloseStdin)
    }

    /// Queues graceful or forced process termination.
    pub fn terminate(&self, mode: TerminationMode) -> Result<(), IoSubmitError> {
        self.submit(WorkerCommand::Terminate(mode))
    }

    /// Receives one event without blocking. A successful receive wakes the
    /// worker so it can resume reading after output backpressure.
    pub fn try_recv_event(&self) -> Result<IoPumpEvent, TryRecvError> {
        let result = self.events.try_recv();
        if result.is_ok() {
            self.worker_thread.unpark();
        }
        result
    }

    /// Receives one event with a caller-selected timeout.
    pub fn recv_event_timeout(
        &self,
        timeout: Duration,
    ) -> Result<IoPumpEvent, RecvTimeoutError> {
        let result = self.events.recv_timeout(timeout);
        if result.is_ok() {
            self.worker_thread.unpark();
        }
        result
    }

    /// Cancels the worker and joins it. Cancellation bypasses the bounded
    /// command queue, so shutdown cannot be blocked by a full input queue.
    pub fn shutdown(&mut self) -> Result<(), IoPumpError> {
        self.cancelled.store(true, Ordering::Release);
        self.worker_thread.unpark();

        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| IoPumpError::WorkerPanicked)?;
        }
        Ok(())
    }

    fn submit(&self, command: WorkerCommand) -> Result<(), IoSubmitError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(IoSubmitError::Closed);
        }

        match self.commands.try_send(command) {
            Ok(()) => {
                self.worker_thread.unpark();
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(IoSubmitError::Backpressure),
            Err(TrySendError::Disconnected(_)) => Err(IoSubmitError::Closed),
        }
    }
}

impl Drop for PtyIoPump {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn worker_loop(
    mut process: Box<dyn RuntimeProcess>,
    commands: Receiver<WorkerCommand>,
    events: SyncSender<IoPumpEvent>,
    cancelled: Arc<AtomicBool>,
) {
    let mut pending_input: Option<(Vec<u8>, usize)> = None;
    let mut pending_event: Option<IoPumpEvent> = None;
    let mut stdout_eof = false;
    let mut exit_status = None;
    let mut exit_event_queued = false;
    let mut stop_after_event = false;

    loop {
        if cancelled.load(Ordering::Acquire) {
            terminate_and_reap(&mut process);
            return;
        }

        let mut made_progress = false;

        if let Some(event) = pending_event.take() {
            let is_exit = matches!(&event, IoPumpEvent::Exited(_));
            match events.try_send(event) {
                Ok(()) => {
                    made_progress = true;
                    if is_exit {
                        return;
                    }
                }
                Err(TrySendError::Full(event)) => {
                    pending_event = Some(event);
                }
                Err(TrySendError::Disconnected(_)) => {
                    terminate_and_reap(&mut process);
                    return;
                }
            }
        }

        if stop_after_event && pending_event.is_none() {
            terminate_and_reap(&mut process);
            return;
        }

        if pending_event.is_none() && stdout_eof && exit_status.is_some() && !exit_event_queued {
            pending_event = exit_status.map(IoPumpEvent::Exited);
            exit_event_queued = true;
            made_progress = true;
        }

        // A full output queue pauses further I/O and command processing. The
        // owner wakes this worker after receiving an event. Shutdown bypasses
        // this pause through the atomic cancellation flag above.
        if pending_event.is_none() {
            if let Some((bytes, offset)) = pending_input.as_mut() {
                let remaining = &bytes[*offset..];
                match process.write_stdin(remaining) {
                    Ok(WriteOutcome::Written(count)) if count.get() <= remaining.len() => {
                        *offset += count.get();
                        if *offset == bytes.len() {
                            pending_input = None;
                        }
                        made_progress = true;
                    }
                    Ok(WriteOutcome::Written(_)) => {
                        pending_input = None;
                        pending_event = Some(IoPumpEvent::Error(RuntimeError::InvalidArgument));
                        stop_after_event = true;
                        made_progress = true;
                    }
                    Ok(WriteOutcome::WouldBlock) => {}
                    Err(error) => {
                        pending_input = None;
                        pending_event = Some(IoPumpEvent::Error(error));
                        stop_after_event = true;
                        made_progress = true;
                    }
                }
            } else {
                match commands.try_recv() {
                    Ok(WorkerCommand::Write(bytes)) => {
                        pending_input = Some((bytes, 0));
                        made_progress = true;
                    }
                    Ok(WorkerCommand::Resize(size)) => {
                        pending_event = Some(match process.resize_pty(size) {
                            Ok(()) => IoPumpEvent::Resized(size),
                            Err(error) => IoPumpEvent::Error(error),
                        });
                        made_progress = true;
                    }
                    Ok(WorkerCommand::Signal(signal)) => {
                        if let Err(error) = process.send_signal(signal) {
                            pending_event = Some(IoPumpEvent::Error(error));
                        }
                        made_progress = true;
                    }
                    Ok(WorkerCommand::CloseStdin) => {
                        if let Err(error) = process.close_stdin() {
                            pending_event = Some(IoPumpEvent::Error(error));
                        }
                        made_progress = true;
                    }
                    Ok(WorkerCommand::Terminate(mode)) => {
                        if let Err(error) = process.terminate(mode) {
                            pending_event = Some(IoPumpEvent::Error(error));
                        }
                        made_progress = true;
                    }
                    Ok(WorkerCommand::Shutdown) => {
                        terminate_and_reap(&mut process);
                        return;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        terminate_and_reap(&mut process);
                        return;
                    }
                }
            }
        }

        if pending_event.is_none() && exit_status.is_none() {
            match process.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    made_progress = true;
                }
                Ok(None) => {}
                Err(error) => {
                    pending_event = Some(IoPumpEvent::Error(error));
                    stop_after_event = true;
                    made_progress = true;
                }
            }
        }

        if pending_event.is_none() && !stdout_eof {
            let mut buffer = [0u8; MAX_IO_CHUNK_BYTES];
            match process.read_stdout(&mut buffer) {
                Ok(ReadOutcome::Data(count)) if count.get() <= buffer.len() => {
                    pending_event = Some(IoPumpEvent::Output(buffer[..count.get()].to_vec()));
                    made_progress = true;
                }
                Ok(ReadOutcome::Data(_)) => {
                    pending_event = Some(IoPumpEvent::Error(RuntimeError::InvalidArgument));
                    stop_after_event = true;
                    made_progress = true;
                }
                Ok(ReadOutcome::WouldBlock) => {}
                Ok(ReadOutcome::Eof) => {
                    stdout_eof = true;
                    pending_event = Some(IoPumpEvent::Eof);
                    made_progress = true;
                }
                Err(error) => {
                    pending_event = Some(IoPumpEvent::Error(error));
                    stop_after_event = true;
                    made_progress = true;
                }
            }
        }

        if !made_progress {
            thread::park_timeout(POLL_INTERVAL);
        }
    }
}

fn terminate_and_reap(process: &mut Box<dyn RuntimeProcess>) {
    let _ = process.terminate(TerminationMode::Graceful);
    let deadline = Instant::now() + GRACEFUL_SHUTDOWN_TIMEOUT;

    loop {
        if matches!(process.try_wait(), Ok(Some(_))) {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(POLL_INTERVAL);
    }

    let _ = process.terminate(TerminationMode::Force);
    let _ = process.wait();
}

#[cfg(test)]
mod tests {
    use super::{IoPumpEvent, IoSubmitError, PtyIoPump, MAX_IO_CHUNK_BYTES};
    use crate::{
        IoAttachment, ProcessExitStatus, ProcessSpawnRequest, ProgramSpec, PtyConfig, ReadOutcome,
        RuntimeError, RuntimeProcess, RuntimeService, Signal, TerminalSize, TerminationMode,
        WriteOutcome,
    };
    use std::collections::VecDeque;
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct FakeState {
        output: VecDeque<u8>,
        output_eof: bool,
        exit_status: Option<ProcessExitStatus>,
        input_blocked: bool,
        input_write_limit: usize,
        written: Vec<u8>,
        resized: Option<TerminalSize>,
        signals: Vec<Signal>,
        stdin_closed: bool,
        terminations: Vec<TerminationMode>,
    }

    struct FakeProcess {
        state: Arc<Mutex<FakeState>>,
    }

    impl FakeProcess {
        fn lock(&self) -> MutexGuard<'_, FakeState> {
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    impl RuntimeProcess for FakeProcess {
        fn process_id(&self) -> Option<u32> {
            Some(42)
        }

        fn write_stdin(&mut self, input: &[u8]) -> Result<WriteOutcome, RuntimeError> {
            if input.is_empty() {
                return Err(RuntimeError::InvalidArgument);
            }
            let mut state = self.lock();
            if state.input_blocked {
                return Ok(WriteOutcome::WouldBlock);
            }
            let count = input.len().min(state.input_write_limit.max(1));
            state.written.extend_from_slice(&input[..count]);
            Ok(WriteOutcome::Written(
                NonZeroUsize::new(count).expect("non-zero write"),
            ))
        }

        fn read_stdout(&mut self, output: &mut [u8]) -> Result<ReadOutcome, RuntimeError> {
            if output.is_empty() {
                return Err(RuntimeError::InvalidArgument);
            }
            let mut state = self.lock();
            if state.output.is_empty() {
                return if state.output_eof {
                    Ok(ReadOutcome::Eof)
                } else {
                    Ok(ReadOutcome::WouldBlock)
                };
            }
            let count = output.len().min(state.output.len());
            for slot in output.iter_mut().take(count) {
                *slot = state.output.pop_front().expect("count bounded by queue");
            }
            Ok(ReadOutcome::Data(
                NonZeroUsize::new(count).expect("non-zero read"),
            ))
        }

        fn read_stderr(&mut self, _output: &mut [u8]) -> Result<ReadOutcome, RuntimeError> {
            Err(RuntimeError::UnsupportedCapability(
                crate::RuntimeCapability::PseudoTerminal,
            ))
        }

        fn close_stdin(&mut self) -> Result<(), RuntimeError> {
            self.lock().stdin_closed = true;
            Ok(())
        }

        fn resize_pty(&mut self, size: TerminalSize) -> Result<(), RuntimeError> {
            self.lock().resized = Some(size);
            Ok(())
        }

        fn send_signal(&mut self, signal: Signal) -> Result<(), RuntimeError> {
            self.lock().signals.push(signal);
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<ProcessExitStatus>, RuntimeError> {
            Ok(self.lock().exit_status)
        }

        fn wait(&mut self) -> Result<ProcessExitStatus, RuntimeError> {
            Ok(self.lock().exit_status.unwrap_or(ProcessExitStatus::Unknown))
        }

        fn terminate(&mut self, mode: TerminationMode) -> Result<(), RuntimeError> {
            let mut state = self.lock();
            state.terminations.push(mode);
            state.exit_status = Some(ProcessExitStatus::Exited(0));
            state.output_eof = true;
            Ok(())
        }
    }

    fn fake_process(
        output: Vec<u8>,
        output_eof: bool,
        exit_status: Option<ProcessExitStatus>,
    ) -> (Box<dyn RuntimeProcess>, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState {
            output: output.into(),
            output_eof,
            exit_status,
            input_write_limit: 7,
            ..FakeState::default()
        }));
        (
            Box::new(FakeProcess {
                state: Arc::clone(&state),
            }),
            state,
        )
    }

    #[test]
    fn slow_reader_applies_bounded_output_backpressure_and_resumes() {
        let payload: Vec<u8> = (0..128_000).map(|value| (value % 251) as u8).collect();
        let expected = payload.clone();
        let (process, state) = fake_process(payload, true, Some(ProcessExitStatus::Exited(0)));
        let pump = PtyIoPump::new(process).expect("start PTY worker");

        thread::sleep(Duration::from_millis(50));
        let remaining = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .output
            .len();
        assert!(remaining > 0, "worker must stop reading when event queue fills");

        let mut received = Vec::new();
        let mut saw_eof = false;
        let mut exit_status = None;
        let deadline = Instant::now() + Duration::from_secs(3);
        while exit_status.is_none() && Instant::now() < deadline {
            match pump.recv_event_timeout(Duration::from_millis(50)) {
                Ok(IoPumpEvent::Output(bytes)) => received.extend(bytes),
                Ok(IoPumpEvent::Eof) => saw_eof = true,
                Ok(IoPumpEvent::Exited(status)) => exit_status = Some(status),
                Ok(IoPumpEvent::Resized(_)) => {}
                Ok(IoPumpEvent::Error(error)) => panic!("unexpected runtime error: {error}"),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("event channel disconnected: {error}"),
            }
        }

        assert_eq!(received, expected);
        assert!(saw_eof);
        assert_eq!(exit_status, Some(ProcessExitStatus::Exited(0)));
    }

    #[test]
    fn partial_writes_and_full_command_queue_preserve_input_order() {
        let (process, state) = fake_process(Vec::new(), false, None);
        state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .input_blocked = true;
        let pump = PtyIoPump::new(process).expect("start PTY worker");

        let chunks: Vec<Vec<u8>> = (0..32u8)
            .map(|value| vec![value; MAX_IO_CHUNK_BYTES])
            .collect();
        let mut accepted = 0usize;
        let mut saw_backpressure = false;
        while accepted < chunks.len() {
            match pump.try_write(&chunks[accepted]) {
                Ok(()) => accepted += 1,
                Err(IoSubmitError::Backpressure) => {
                    saw_backpressure = true;
                    break;
                }
                Err(error) => panic!("unexpected submit error: {error}"),
            }
        }
        assert!(saw_backpressure);
        assert!(accepted > 0);

        state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .input_blocked = false;

        let deadline = Instant::now() + Duration::from_secs(3);
        while accepted < chunks.len() && Instant::now() < deadline {
            match pump.try_write(&chunks[accepted]) {
                Ok(()) => accepted += 1,
                Err(IoSubmitError::Backpressure) => thread::sleep(Duration::from_millis(2)),
                Err(error) => panic!("unexpected submit error: {error}"),
            }
        }
        assert_eq!(accepted, chunks.len());

        let expected: Vec<u8> = chunks.into_iter().flatten().collect();
        while Instant::now() < deadline {
            let written_len = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .written
                .len();
            if written_len == expected.len() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .written,
            expected
        );
    }

    #[test]
    fn resize_eof_and_exit_events_are_ordered() {
        let size = TerminalSize::new(132, 43).expect("valid dimensions");
        let (process, state) =
            fake_process(Vec::new(), true, Some(ProcessExitStatus::Exited(0)));
        let pump = PtyIoPump::new(process).expect("start PTY worker");
        pump.resize_pty(size).expect("queue resize");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut events = Vec::new();
        while !events.iter().any(|event| matches!(event, IoPumpEvent::Exited(_)))
            && Instant::now() < deadline
        {
            if let Ok(event) = pump.recv_event_timeout(Duration::from_millis(50)) {
                events.push(event);
            }
        }

        assert!(events.contains(&IoPumpEvent::Resized(size)));
        assert!(events.contains(&IoPumpEvent::Eof));
        assert!(events.contains(&IoPumpEvent::Exited(ProcessExitStatus::Exited(0))));
        assert_eq!(
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .resized,
            Some(size)
        );
    }

    #[test]
    fn shutdown_bypasses_full_queues_and_terminates_the_child() {
        let (process, state) = fake_process(Vec::new(), false, None);
        let mut pump = PtyIoPump::new(process).expect("start PTY worker");
        let start = Instant::now();
        pump.shutdown().expect("worker shutdown");
        assert!(start.elapsed() < Duration::from_secs(1));

        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            state.terminations.first(),
            Some(&TerminationMode::Graceful)
        );
        assert_eq!(state.exit_status, Some(ProcessExitStatus::Exited(0)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_pty_pump_propagates_resize_and_drains_eof() {
        use crate::linux::runtime::LinuxRuntime;

        let _guard = crate::linux::test_resource_lock();
        let initial = TerminalSize::new(80, 24).expect("initial size");
        let resized = TerminalSize::new(132, 43).expect("resized size");
        let program = ProgramSpec::new("/bin/sh")
            .expect("shell")
            .with_arg("-c")
            .with_arg("stty size; sleep 0.25; stty size");
        let request = ProcessSpawnRequest::new(
            program,
            IoAttachment::Pty(PtyConfig::new(initial)),
        )
        .expect("spawn request");
        let process = LinuxRuntime.spawn(request).expect("spawn PTY child");
        let pump = PtyIoPump::new(process).expect("start PTY worker");

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        let mut resize_event = false;
        let mut eof = false;
        let mut exited = false;

        while !output.windows(5).any(|window| window == b"24 80")
            && Instant::now() < deadline
        {
            match pump.recv_event_timeout(Duration::from_millis(50)) {
                Ok(IoPumpEvent::Output(bytes)) => output.extend(bytes),
                Ok(IoPumpEvent::Error(error)) => panic!("PTY runtime error: {error}"),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("PTY event channel closed: {error}"),
            }
        }
        assert!(output.windows(5).any(|window| window == b"24 80"));

        pump.resize_pty(resized).expect("queue kernel resize");
        while !output.windows(5).any(|window| window == b"43 132")
            && Instant::now() < deadline
        {
            match pump.recv_event_timeout(Duration::from_millis(50)) {
                Ok(IoPumpEvent::Output(bytes)) => output.extend(bytes),
                Ok(IoPumpEvent::Resized(size)) => resize_event = size == resized,
                Ok(IoPumpEvent::Error(error)) => panic!("PTY runtime error: {error}"),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("PTY event channel closed: {error}"),
            }
        }
        assert!(output.windows(5).any(|window| window == b"43 132"));
        assert!(resize_event);

        while !exited && Instant::now() < deadline {
            match pump.recv_event_timeout(Duration::from_millis(50)) {
                Ok(IoPumpEvent::Eof) => eof = true,
                Ok(IoPumpEvent::Exited(status)) => {
                    assert_eq!(status, ProcessExitStatus::Exited(0));
                    exited = true;
                }
                Ok(IoPumpEvent::Error(error)) => panic!("PTY runtime error: {error}"),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("PTY event channel closed: {error}"),
            }
        }
        assert!(eof);
        assert!(exited);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_pty_shutdown_does_not_deadlock_with_slow_output_reader() {
        use crate::linux::runtime::LinuxRuntime;

        let _guard = crate::linux::test_resource_lock();
        let size = TerminalSize::new(80, 24).expect("valid dimensions");
        let request = ProcessSpawnRequest::new(
            ProgramSpec::new("/usr/bin/yes").expect("yes executable"),
            IoAttachment::Pty(PtyConfig::new(size)),
        )
        .expect("spawn request");
        let process = LinuxRuntime.spawn(request).expect("spawn high-volume PTY child");
        let mut pump = PtyIoPump::new(process).expect("start PTY worker");

        // Deliberately do not drain the bounded event channel. The worker must
        // stop reading instead of buffering unbounded output.
        thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        pump.shutdown().expect("cancel and reap child");
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
