# DE-Runtime Process Interface

## Scope

DE-007-01 defines platform-independent Rust contracts for process creation and child-process I/O. It does not implement Linux syscalls, PTY allocation, process spawning, signal delivery, or Windows/macOS adapters. Those are concrete backend tasks.

The crate owns launch value types and trait contracts. A backend implements RuntimeService and returns one RuntimeProcess handle per child.

## Process specification

ProgramSpec owns:

- An executable path/name as OsString.
- An ordered vector of separate OsString arguments.
- Environment overrides as explicit key/value pairs.
- An optional working directory.

Arguments are never joined into a command string and the runtime must not implicitly invoke a shell interpreter. ProgramSpec::validate rejects an empty executable, NUL bytes in executable/arguments/environment/path, empty environment keys, and environment keys containing '='. Duplicate environment keys are replaced by the most recent builder call while preserving the key's original insertion position.

ProcessSpawnRequest validates ProgramSpec before it reaches a backend. It also carries an explicit I/O attachment and kill-on-drop policy. The default is to terminate a still-running child when its owning process handle is dropped; a caller may explicitly disable this behavior when another lifecycle owner guarantees cleanup.

## I/O attachment

A child uses one of two explicit modes:

- StdioConfig independently selects Pipe, Null, or Inherit for stdin, stdout, and stderr.
- PtyConfig attaches the child to one PTY with validated, non-zero columns and rows. PTY-backed stdout/stderr are part of the same terminal stream.

The interface never silently chooses inherited stdio or a PTY. The caller selects the intended mode in the spawn request.

## Runtime capabilities

PlatformCapabilities is an immutable, sorted, deduplicated capability snapshot. It reports support for stdio pipes, PTYs, terminal resize, semantic signals, process groups, environment overrides, working directories, and non-blocking I/O.

Callers query capabilities before requesting optional platform behavior. Unsupported operations return RuntimeError::UnsupportedCapability rather than silently degrading.

## RuntimeService contract

RuntimeService is Send + Sync and provides:

- capabilities(): a stable capability snapshot.
- spawn(request): validates the complete request, creates the child and its configured I/O resources, then transfers ownership to a RuntimeProcess handle.

A failed spawn must not leak a child process, descriptor, PTY, or partially registered handle.

## RuntimeProcess contract

One RuntimeProcess owns exactly one child and its associated OS resources. It is Send so it can be moved to a worker thread; process I/O and wait operations must not block the GUI/event-loop thread.

The interface provides:

- process_id(): optional platform process identifier; it is not a portable session identity.
- write_stdin(): accepts a non-empty byte slice and reports partial progress or WouldBlock.
- read_stdout()/read_stderr(): non-blocking reads returning Data(count), WouldBlock, or EOF.
- close_stdin(): idempotently closes the child input stream.
- resize_pty(): changes PTY dimensions or returns UnsupportedCapability for non-PTY sessions.
- send_signal(): sends a semantic signal to this child only.
- try_wait(): non-blocking exit check. Once an exit status is returned, later calls return the same status.
- wait(): blocking wait, permitted only on a worker thread.
- terminate(): applies graceful or forced termination policy.

Read/write counts are non-zero and bounded by the supplied buffer length. WouldBlock is not EOF. Stream bytes are ordered; callers retain unwritten input and retry it after partial writes.

## Exit and signal model

ProcessExitStatus distinguishes an integer exit code, signal termination, and an unknown platform outcome. Only Exited(0) is success. Signal is semantic rather than a raw OS integer so platform adapters can map supported signals and report capability differences.

TerminationMode separates cooperative termination from forced termination. The session manager owns timeout and escalation policy.

## Errors and diagnostics

RuntimeError exposes stable categories for invalid arguments/specifications, unsupported capabilities, backend unavailability, spawn, I/O, signal, resize, wait, already-exited process, and resource-limit failures.

Error values must not contain passwords, environment contents, raw command input, or other secrets. Platform-specific details belong in safe diagnostic records, not in unstable public error strings.

## Threading and ownership

- RuntimeService is safe to share between worker threads.
- A RuntimeProcess handle has one logical owner and is moved to a worker for I/O/wait operations.
- No blocking child I/O or wait runs on the GUI/event-loop thread.
- The backend owns native handles until process-handle destruction.
- Dropping a handle follows its kill-on-drop policy and releases each native resource exactly once.
- A process exit is a normal lifecycle event, not automatically an application-fatal error.

## Tests and implementation boundary

Unit tests validate ProgramSpec values, environment and NUL rejection, PTY dimensions, explicit stdio selection, spawn request validation, capability reporting, and a fake backend implementing the complete trait surface.

The fake backend is a compile-time and contract test only. It does not substitute for the Linux process/PTY adapter, which must provide integration tests against real child processes in its own task.
