# Terminal Session Manager

**Status:** Linux external-program session manager implementation for DE-014-01. The manager owns session lifecycle and PTY process handles; terminal parsing/rendering remains a separate subsystem.

## 1. Ownership model

- One `SessionManager` owns the session registry and the runtime service.
- Each `ManagedSession` owns exactly one runtime process handle, which in the Linux backend owns one PTY master and one child process/session.
- A session owns its lifecycle state, terminal dimensions, exit status, and bounded pending-input queue.
- Session IDs are monotonically allocated, non-zero, and never reused during one manager lifetime.
- A process handle is never shared between sessions. Closing one session cannot close another session's PTY or child process.

The manager uses the existing `SessionId`, `SessionSpec`, and `SessionLifecycle` contracts. The concrete `LinuxRuntimeService` adapts `de-runtime::LinuxRuntime` to the application runtime interface.

## 2. Lifecycle

```text
Created -> Starting -> Running -> Exited -> Stopping -> Closed
                 |         |                  ^
                 v         v                  |
               Failed ----+------------------+
```

The manager publishes a session only after runtime spawn succeeds and the lifecycle reaches Running. A failed spawn does not insert a partially initialized session. The allocated ID is consumed and is not reused.

A child exit is observed through non-blocking `try_wait` polling and transitions Running to Exited. Output may still be drained after exit. New input and resize requests are rejected once the session is no longer Running.

## 3. PTY and process binding

The Linux adapter creates a `ProcessSpawnRequest` with `IoAttachment::Pty` and `kill_on_drop=true`. The underlying Linux runtime owns:

- PTY master descriptor and its non-blocking I/O state.
- Child process and process group.
- Terminal window-size ioctl.
- Signal delivery and child wait/reap.
- Descriptor cleanup and kill-on-drop fallback.

Executable and arguments remain separate `ProgramSpec` values. No command string is constructed and no implicit shell is invoked by the manager.

The current concrete manager supports `SessionKind::ExternalProgram`. `SessionKind::DeShell` is explicitly rejected until the dedicated FFI-backed shell session adapter is connected; it is not silently launched as an external process.

## 4. Input and output

### Input

`send_input` appends bytes to a per-session bounded queue. It never drops bytes due to a partial PTY write. `flush_input` attempts non-blocking writes up to the configured per-poll budget:

- A positive count removes only the accepted prefix.
- A zero count means the runtime is applying backpressure; the remaining bytes stay queued.
- A runtime error leaves unwritten bytes in the queue.
- Queue overflow is returned as an error before any new bytes are appended.

The event loop should call `flush_input` again when the PTY becomes writable. The default queue limit is 1 MiB per session and the default flush budget is 64 KiB per call.

### Output

`read_output` performs one non-blocking read into a caller-owned buffer and returns Data, WouldBlock, or Eof. The session manager does not parse terminal escape sequences. The caller forwards ordered bytes to that session's terminal-engine instance. Each session's PTY stream remains isolated.

### Resize

A resize is sent to the owned PTY first. The cached session dimensions are updated only after the runtime confirms the ioctl succeeded. A failed resize leaves the cached dimensions unchanged.

## 5. Close and orphan-process prevention

Closing a session is idempotent once Closed:

1. Transition the session to Stopping.
2. Close the child input stream.
3. Check whether the child has already exited.
4. If it is still running, request graceful termination.
5. Poll for exit until the configured grace deadline.
6. If necessary, request forced termination and poll for the forced-shutdown deadline.
7. Drop the runtime handle. The Linux spawn request has kill-on-drop enabled; the runtime handle's Drop path kills the owned process group and reaps the child.
8. Clear pending input and transition to Closed.

The same cleanup path is used by application shutdown. The manager attempts to close every session even if another session reports a cleanup error, then shuts down the runtime service. A runtime-shutdown failure remains retryable by the application lifecycle coordinator.

Dropping a live `ManagedSession` also requests forced termination before dropping its kill-on-drop process handle. Dropping the manager invokes shutdown for all remaining sessions. This is a final safety net, not a replacement for explicit orderly shutdown.

Shutdown uses bounded waits and can block for the configured grace/force intervals. The synchronous `close_session` and application-service `shutdown` paths must run on a lifecycle worker, never on the GUI/event-loop thread. GUI callers should use `SessionManager::close_session_async`: it removes the session from the active registry immediately and performs cleanup on a dedicated worker. Callers may poll the returned `JoinHandle::is_finished`; application shutdown waits for all close workers to finish before shutting down the runtime service. Dropping a close handle detaches cleanup rather than cancelling it.

## 6. Limits and errors

Default limits:

| Resource | Default |
| --- | ---: |
| Concurrent sessions | 32 |
| Pending input per session | 1 MiB |
| Input flush budget per call | 64 KiB |
| Graceful termination wait | 300 ms |
| Forced termination wait | 500 ms |

All limits are configurable through `SessionManagerLimits`. Input queue growth uses fallible reservation. Session creation is rejected when the manager is not initialized, the session limit is reached, the executable specification is invalid, or the requested backend is not wired.

Errors use safe service-level descriptions. They do not include command arguments, environment values, or secret process data.

## 7. Current scope and integration boundary

This task implements session/process ownership, bounded input transport, raw output delivery, resize propagation, exit polling, close semantics, and Linux PTY lifecycle binding.

It does not yet:
- Apply VT parser events to a per-session screen model.
- Connect GUI tabs/panes to the session registry.
- Implement DE-Shell's FFI session backend.
- Add background job control or multi-process pipelines.

The terminal engine remains responsible for consuming output bytes and creating immutable render frames. The GUI/session layer is responsible for polling each session and routing bytes/events to the matching terminal model.

## 8. Verification

Tests cover:
- Session creation and ID non-reuse.
- Independent process handles and per-session input queues.
- Partial writes and backpressure without byte loss.
- Input queue capacity enforcement.
- Child exit detection and rejection of post-exit input.
- Resize failure atomicity.
- Failed spawn without publishing a partial session.
- Per-session close and whole-application shutdown.
- Forced termination and process-handle drop on manager destruction.
- Rejection of the unwired DE-Shell backend.
- A Linux integration test that spawns a real PTY child, reads its output, observes EOF, and closes the session.
