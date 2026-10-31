# Linux PTY Child Process Lifecycle

## Scope

DE-006-02 spawns one executable attached to a PTY and owns its process/session lifecycle. It does not implement the runtime's non-blocking byte pumps, stdout/stderr read abstraction, or capability service; those belong to DE-006-03 and DE-007-02.

Implementation: `de_runtime::linux::process::LinuxPtyChild`.

## Spawn contract

Before acquiring PTY resources, the backend validates the complete `ProgramSpec`. It then:

1. Allocates a configured `PtyPair`.
2. Duplicates the slave descriptor for stdin, stdout, stderr, and pre-exec controlling-terminal setup. All duplicates are CLOEXEC.
3. Attaches the three stdio descriptors to `std::process::Command`.
4. Passes the executable and arguments as separate `OsStr` values; environment overrides and working directory are applied explicitly.
5. In the child pre-exec boundary, calls `setsid()`, assigns the slave as controlling terminal with `TIOCSCTTY`, and sets the child process group as the terminal foreground process group.
6. Returns the child handle and the sole parent-owned PTY master descriptor only after successful spawn.

The parent closes every slave descriptor after spawn. The returned handle owns the child and PTY master. If any allocation, descriptor duplication, pre-exec, or spawn step fails, RAII closes all acquired descriptors and the standard process API reaps any failed child.

No implicit shell is invoked. A caller that intentionally wants a shell must select that executable and pass its arguments explicitly.

## Session and process-group isolation

Each spawned child becomes a new session leader and process-group leader. Its process-group ID equals its PID. Signals are sent to the negative process-group ID, so shell descendants in that PTY session receive the same session-scoped signal. No operation targets another session.

The PTY slave is the child's stdin, stdout, and stderr and is its controlling terminal. The master remains owned by the parent handle. The child may change terminal modes after startup.

## Exit status and reaping

- `try_wait()` is non-blocking. When the child exits, the standard library reaps it and the semantic exit status is cached.
- `wait()` blocks and caches the same status.
- Repeated `try_wait()` and `wait()` calls return the cached status.
- Normal exit codes are preserved. Known termination signals map to the platform-neutral `Signal` values; an unrecognized signal maps to `Unknown`.
- Sending a signal after the child has exited returns a not-found error. `terminate()` is idempotent for an already-exited child.

## Drop policy

- With `kill_on_drop = true` (the default request policy), dropping a live handle sends SIGKILL to the owned process group and waits for the direct child, preventing an orphaned live session and zombie.
- With `kill_on_drop = false`, the child is detached from the handle but transferred to a dedicated background reaper thread. The reaper waits for and collects its exit status. If the reaper cannot be created or has disconnected, cleanup falls back to synchronous wait rather than silently dropping an unreaped child.
- The PTY master closes when the handle drops. The child process policy is independent from the descriptor ownership policy.

## Signals

| Semantic signal | Linux signal |
| --- | --- |
| Interrupt | SIGINT |
| Terminate | SIGTERM |
| Hangup | SIGHUP |
| Quit | SIGQUIT |
| Kill | SIGKILL |
| User1 | SIGUSR1 |
| User2 | SIGUSR2 |

Graceful termination maps to SIGTERM; forced termination maps to SIGKILL. Signals target the session process group.

## Tests

Linux tests cover a child with all three stdio descriptors attached to a TTY, controlling-session and process-group identity, normal exit code, semantic SIGTERM, forced SIGKILL, stable cached wait status, ECHILD after reaping, kill-on-drop cleanup, detached-child reaping, and invalid-executable failure.
