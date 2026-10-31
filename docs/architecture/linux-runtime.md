# Linux Process Runtime Adapter

**Status:** Initial Linux x86_64 implementation of the platform-neutral contracts in [DE-Runtime Process Interface](runtime-interface.md).

## Supported capabilities

The Linux backend reports support for:

- Explicit stdio pipes, null streams, and inherited streams.
- PTY-backed sessions with resize.
- Non-blocking pipe and PTY I/O.
- Environment overrides and working-directory selection.
- Semantic signals delivered to a process group owned by the runtime handle.
- Cached process exit status and kill-on-drop behavior.

The adapter does not claim Windows, macOS, sandboxing, elevated execution, or plugin isolation support.

## Stdio sessions

Each requested stream is configured independently as Pipe, Null, or Inherit. Pipe descriptors are switched to non-blocking mode before the process handle is returned. Reads and writes return partial progress or WouldBlock; callers retain unwritten bytes and retry. EOF is reported only when the stream actually closes.

The runtime creates a dedicated process group for each stdio child. Signal delivery targets that group, never the application process group. Dropping a handle follows the request's kill-on-drop policy. When kill-on-drop is disabled, a background reaper polls detached children without blocking on one long-running process.

## PTY sessions

A PTY child is created through LinuxPtyChild, which allocates a PTY, creates a new session and process group, assigns the slave as the controlling terminal, and attaches it to stdin/stdout/stderr.

The master descriptor is duplicated and switched to non-blocking mode. PTY output is a single combined terminal stream: read_stdout reads that stream, while read_stderr returns UnsupportedCapability(StdioPipes) because the PTY does not preserve separate stdout/stderr channels.

resize_pty applies the terminal dimensions to the PTY. close_stdin sends the terminal EOF character (VEOF, byte 0x04) to the PTY input stream; it does not close the bidirectional master descriptor. This is terminal line-discipline behavior, not a half-close of the PTY.

On Linux, a PTY master may report EIO after the slave side closes; the adapter maps that condition to stream EOF.

## Failure and ownership rules

- The complete ProcessSpawnRequest is validated before OS resources are acquired.
- If spawn or post-spawn descriptor setup fails, the child is terminated and waited before returning an error.
- PTY, pipe, and child handles are owned by one RuntimeProcess instance.
- Process IDs are exposed for diagnostics only; they are not session identifiers.
- Exit status is cached and stable across repeated try_wait and wait calls.
- ESRCH while signaling maps to ProcessAlreadyExited; other signal/I/O failures map to stable runtime error categories.
- Runtime I/O is non-blocking. Blocking wait is permitted only on a worker thread.

## Integration tests

The Linux adapter tests exercise capability reporting, explicit stdio selection, environment and working-directory overrides, separate stdout/stderr pipes, PTY stream merging and resize, stdin EOF, semantic signal delivery, invalid executable handling, and unsupported operations for non-pipe/non-PTY streams.
