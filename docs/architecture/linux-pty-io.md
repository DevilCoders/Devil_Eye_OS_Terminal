# Linux PTY I/O Contract

**Scope:** DE-006-03. This document describes the Linux implementation of the platform-neutral `RuntimeProcess` contract. It does not define GUI-thread scheduling or the terminal session manager.

## Descriptor ownership

- `PtyPair` owns the initial master and slave descriptors through `OwnedFd`.
- The child receives independent close-on-exec duplicates of the slave for stdin, stdout, stderr, and controlling-terminal setup.
- The parent runtime owns the PTY master used for both terminal input and output.
- The parent copy of the slave is closed after spawn. The child process owns its inherited standard-stream descriptors.
- All descriptors close through RAII. Failed allocation, spawn, nonblocking setup, or adapter construction must not leak descriptors or leave an unowned child.

The PTY master is a single bidirectional byte stream. PTY sessions do not expose a separate stderr stream; stderr is attached to the same terminal as stdout.

## Nonblocking read contract

`read_stdout` on a PTY master:

- Returns `Data(n)` when 1..=buffer length bytes are read.
- Returns `WouldBlock` when no bytes are currently available. This is not EOF.
- Returns `Eof` when the PTY slave side has closed. Linux PTY master `EIO` after slave closure is normalized to EOF.
- Rejects an empty output buffer.
- Retries interrupted system calls and never waits for future data.

The caller must consume exactly the reported byte count. A read boundary has no terminal-protocol meaning: UTF-8 code points and VT/ANSI control sequences can be split across reads.

## Nonblocking write contract

`write_stdin` on a PTY master:

- Returns `Written(n)` when the kernel accepts 1..=input length bytes.
- Returns `WouldBlock` when the PTY cannot currently accept input.
- Maps a closed slave (`EIO`/broken pipe) to `ProcessAlreadyExited`.
- Rejects an empty input slice.
- Retries interrupted system calls and never waits for buffer capacity.

The caller retains the unwritten suffix after `Written(n)` and retries it later. It must not resend the accepted prefix. On `WouldBlock`, the caller waits for readiness or drains output before retrying. The runtime does not buffer or silently drop caller bytes.

## Backpressure and scheduling

The runtime deliberately exposes progress rather than owning an unbounded queue or hidden I/O loop. A session worker should:

1. Keep one bounded pending-input buffer per session.
2. Advance its input offset only by the returned `Written(n)`.
3. Stop attempting writes after `WouldBlock` until the next writable opportunity.
4. Drain readable output in bounded chunks and forward it in order to the terminal engine.
5. Yield or wait for readiness between empty polls; never spin continuously.
6. Apply an explicit queue limit and surface overload rather than growing memory without bound.
7. Keep all PTY operations off the GUI/event-loop thread.

Cancellation is safe at every poll boundary: accepted write bytes remain accepted, unaccepted bytes remain owned by the caller, and the runtime handle remains the sole owner of its descriptors and child process. Dropping a default `kill_on_drop` handle terminates its private process group and reaps the child.

## Resize propagation

`resize_pty` applies `TIOCSWINSZ` to the PTY master. The child observes the new rows and columns through the kernel's PTY window-size state and receives the platform's window-size-change signal where applicable.

- Zero rows or columns cannot be constructed as a `TerminalSize`.
- The runtime updates its cached dimensions only after the ioctl succeeds.
- A failed ioctl returns `ResizeFailed`; the caller must not assume the requested dimensions took effect.
- The session manager must serialize terminal-model and PTY resize requests and reconcile failures.

## EOF and shutdown

PTY EOF differs from closing a pipe: the master cannot half-close its write direction independently of its read direction. The current `close_stdin` implementation sends the configured terminal VEOF control byte; in canonical mode this signals end-of-input to the foreground reader. In raw mode it is ordinary input data. Applications that require deterministic process termination should use the explicit termination API rather than treating PTY VEOF as a transport half-close.

When the child exits, the runtime caches its exit status. The master may still contain unread output; callers should drain until `Eof` or a bounded shutdown deadline. `WouldBlock` is not a reason to discard the session. Forced shutdown may discard remaining bytes only under an explicit policy and should record that outcome.

## Verification evidence

Linux runtime tests exercise:

- Bidirectional input/output with raw-mode `cat`, including NUL and non-UTF-8 bytes.
- 256 KiB output while the parent drains concurrently, preventing the classic wait-before-read deadlock.
- 1 MiB input with observed `WouldBlock`, reader draining, write resumption, and byte-for-byte integrity.
- PTY resize observed by a child process using `stty size`.
- Child cancellation and reaping when the runtime handle is dropped.
- Existing EOF, resize, process-group signal, and descriptor ownership paths.

These tests validate the runtime adapter's nonblocking primitives and lifecycle. They do not replace the later session-manager readiness loop or end-to-end GUI integration tests.
