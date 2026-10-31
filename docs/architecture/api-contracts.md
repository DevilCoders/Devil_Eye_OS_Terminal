# Cross-Component API Contracts

**Status:** Normative design contract for subsystem implementation. The system-level ownership model is defined in [System Architecture](system-architecture.md). Exact C ABI declarations and Rust bindings will be implemented and versioned in DE-008 (#49–#50).

This document defines behavioral contracts. A component must not silently change these semantics; incompatible changes require a major interface version and an architecture review.

## 1. Contract principles

1. Ownership is singular: every resource has one owner and explicit borrowers.
2. Byte streams are lossless and ordered. Reads and writes may be partial.
3. No blocking process I/O, child wait, or unbounded queue growth on the GUI thread.
4. Errors are values at subsystem boundaries; no panic, exception, or process exit crosses a component boundary.
5. No command is executed by concatenating untrusted input into a shell string. Executable and arguments remain separate values.
6. The terminal engine is shell-agnostic. DE-Shell and external shells use the same terminal-facing semantics.
7. All session-local state and resources are isolated per session.
8. Platform-specific types stay behind `de-runtime` adapters.
9. Rust/C calls use a versioned ABI with explicit buffer and handle ownership.
10. Interface changes require contract tests and documentation updates in the same change.

## 2. Component interface inventory

| Producer | Consumer | Contract | Ownership |
| --- | --- | --- | --- |
| GUI event adapter | `de-app` | Ordered user events and window lifecycle events | App owns application state; GUI owns native event objects |
| `de-app` | Session manager | Create, focus, resize, stop, and close session operations | App/session manager owns session lifecycle |
| Session manager | `de-terminal-core` | Input bytes, output bytes, terminal resize, terminal mode changes | Terminal core owns parser and screen state |
| Session manager | `de-runtime` | Validated `ProgramSpec`, terminal size, process controls | Runtime owns OS handles and child process |
| Runtime adapter | Session manager | Ordered output chunks, write progress, exit and signal events | Runtime owns native resources until close |
| `de-app` | `de-ffi` / DE-Shell | Version negotiation, opaque shell handle, input and output events | C owns C allocations; Rust owns Rust allocations |
| Terminal core | Renderer | Immutable frame/damage view | Terminal core owns cells; renderer borrows frame data |
| Input mapper | Session manager | Encoded terminal input bytes | Session manager owns transport and write queue |
| Configuration service | `de-app` | Validated immutable configuration snapshot | Configuration service owns persisted representation |

## 3. Common identifiers and value semantics

### Session identity

- A `SessionId` is an opaque, application-generated identifier unique for the lifetime of one application process.
- IDs are never reused while an old session can still emit events.
- A session ID is not a PID, file descriptor, pointer, or security credential.
- Every runtime event is tagged with its owning session ID.

### Terminal dimensions

Use the existing `de_terminal_core::TerminalSize` value. Columns and rows must be non-zero. Resizing updates the terminal model and the PTY window size as one ordered session operation; failures are reported and reconciled rather than silently ignored.

### Process launch specification

Use the existing `de_runtime::ProgramSpec` as the source representation:

- Executable is a separate `OsString`.
- Arguments are a vector of separate `OsString` values; never join them into a command line.
- Environment overrides are explicit key/value pairs.
- Working directory is an optional `PathBuf`.
- Empty executable is invalid.
- Environment keys must be non-empty and contain no platform-invalid separator or NUL.
- The runtime must not implicitly invoke `/bin/sh -c` or another command interpreter.

The runtime adapter performs platform-specific validation and returns a typed error before creating OS resources when a request is invalid.

## 4. Session lifecycle contract

### States

```text
Created -> Starting -> Running -> Stopping -> Closed
                 |         |
                 v         v
               Failed    Exited
                           |
                           v
                         Closed
```

- `Created`: configuration is validated; no process or PTY is owned yet.
- `Starting`: resources are being acquired; the session is not yet interactive.
- `Running`: input/output transport is active.
- `Exited`: child process has exited and its exit status is known; output may still be drained.
- `Stopping`: termination and cleanup are in progress.
- `Failed`: startup or runtime failure prevents normal operation; cleanup is still required.
- `Closed`: all owned resources are released; terminal history may remain available as read-only UI state.

### Legal transitions

- `Created -> Starting`
- `Starting -> Running | Failed | Stopping`
- `Running -> Exited | Failed | Stopping`
- `Exited -> Stopping | Closed`
- `Failed -> Stopping | Closed`
- `Stopping -> Closed`

Repeated stop/close requests are idempotent. A closed session cannot restart; create a new session instead. A process exit is not itself an application-fatal error.

### Session manager responsibilities

The session manager is the only authority allowed to transition lifecycle state. It must:

- Validate session configuration before starting.
- Allocate a unique session ID.
- Create the terminal model before starting interactive I/O.
- Route each runtime event only to the matching session.
- Reject input after the session leaves `Running`.
- Drain output after child exit until EOF or a bounded drain deadline.
- Reap the child and close transport handles exactly once.
- Ensure a failure in one session does not mutate another session.

## 5. Runtime and PTY contract

### Spawn

The Rust contract is defined in `de-runtime` by `ProcessSpawnRequest`, `IoAttachment`, `RuntimeService`, and `RuntimeProcess`. A request contains a validated `ProgramSpec`, explicit stdio or PTY attachment, and a kill-on-drop policy. The platform adapter returns either:

- A runtime-owned child-session handle and a successful startup result; or
- A typed error with no leaked file descriptors, child process, or partially registered session.

The child starts with the requested executable, argument vector, environment overrides, and working directory. No implicit shell expansion is performed by the runtime.

### I/O

- Output is an ordered byte stream. A chunk boundary has no semantic meaning; VT sequences and UTF-8 characters may span chunks.
- Input writes may complete partially. The caller retains unwritten bytes and retries according to the runtime's write-progress result.
- EOF, hangup, and child exit are explicit events.
- Temporary non-readiness is distinct from EOF and fatal I/O failure.
- Per-session read/write queues are bounded and preserve ordering.
- The runtime must not silently drop bytes. Any forced truncation during shutdown is reported in session diagnostics.

### Resize

A resize request carries non-zero columns and rows. The runtime applies the platform PTY resize and reports success or a typed error. The terminal model must not permanently diverge from the effective PTY dimensions; the session manager reconciles failed updates.

### Signals and termination

Signal/termination operations target only the child owned by the supplied session handle. The runtime reports unsupported operations explicitly. Graceful termination uses a bounded timeout; escalation policy is owned by the session manager.

### Resource ownership

The runtime owns PTY descriptors, child-process handles, and wait/reap responsibilities. Handles are opaque to callers. Closing a runtime session is idempotent and releases every owned OS resource exactly once.

## 6. Terminal engine contract

### Input

The terminal engine consumes arbitrary byte slices incrementally. It must preserve parser state across calls and handle:

- Escape/control sequences split across calls.
- UTF-8 code points split across calls.
- Invalid or incomplete sequences without out-of-bounds access or unbounded buffering.
- Control characters and terminal mode transitions.
- Large input chunks under configured resource limits.

### State ownership

The terminal engine exclusively owns:

- Screen grid and cell contents.
- Cursor and scroll regions.
- Terminal modes and parser state.
- Scrollback storage.
- Damage/dirty-region state.

Callers provide bytes and resize requests; they do not mutate internal cells directly.

### Output to renderer

The renderer receives an immutable frame snapshot or immutable damage view. It must not retain borrowed pointers/references beyond the documented frame lifetime. Rendering does not mutate terminal state. A redraw request may be coalesced, but terminal state updates may not be dropped.

### Shell independence

The terminal engine does not parse shell grammar, expand variables, dispatch commands, or assume DE-Shell-specific control messages. External shells must work through ordinary terminal byte streams.

## 7. Input mapping contract

Input mapping converts GUI events to terminal bytes based on the active session's terminal modes.

- Key events, text input, paste, mouse, and focus events remain distinguishable until mapping is applied.
- Text input is encoded as UTF-8 unless the active terminal protocol explicitly requires another representation.
- Paste is treated as untrusted input and is subject to the application's paste policy.
- Mouse reporting follows the active terminal mode and negotiated encoding.
- Focus events are emitted only when focus reporting is enabled.
- Unsupported GUI events are ignored or surfaced as unsupported; they must not emit malformed terminal sequences.

## 8. DE-Shell and Rust/C FFI contract

### Boundary model

The C shell core is a separate implementation component. Rust owns application/session lifecycle and OS process services; C owns shell-language state and parsing. The C core requests external process work through a typed runtime contract. It must not call `system()`, `popen()`, or construct a command string for implicit shell interpretation.

### ABI rules

- ABI version is negotiated before creating or using a shell handle.
- Public C handles are opaque pointers; Rust must not inspect their layout.
- Cross-boundary integers use fixed-width types with documented ranges.
- C enums have explicit, append-only numeric values; unknown values are rejected.
- No Rust `String`, `Vec`, trait object, reference, or layout-dependent struct crosses the ABI.
- No C++ exception or Rust panic may cross the boundary.
- All pointers are nullable only when the specific function contract says so; every pointer/length pair is validated.
- Buffer lengths are explicit. Strings are not assumed to be NUL-terminated unless the function explicitly says so.
- No allocator ownership crosses the boundary. Output is copied into caller-provided buffers or transferred through an explicitly documented C-owned handle with a matching C destroy function.
- FFI calls do not invoke user callbacks while internal locks are held.

### Handle and threading rules

- One shell handle represents one shell session.
- A handle is single-thread-affine unless a later ABI version explicitly documents synchronization.
- Concurrent or reentrant calls on one handle are forbidden.
- Separate handles may be used by separate sessions.
- Destroying a handle invalidates it permanently; use-after-destroy is caller error and must be prevented by the safe Rust wrapper.
- The C implementation must not retain borrowed input pointers after the call returns.

### Error mapping

Every ABI function returns a stable status code. Output parameters are initialized to safe defaults before work begins. Error details are retrieved through a bounded diagnostic buffer and never include secrets. Unknown status codes map to a generic FFI error in Rust.

The current `DeStatus` values in `de-ffi` are the initial stable values. New values are appended; existing numeric assignments are never reused or changed. The exact C declarations and safe Rust wrapper will be implemented by DE-008.

## 9. Renderer and GUI contract

- GUI owns native window and event-loop resources.
- Application orchestration owns application state and session registry.
- Renderer owns GPU/window rendering resources but not terminal cells.
- Renderer consumes immutable frame data and returns rendering failures as typed errors.
- Resize, scale-factor, and visibility changes are forwarded as ordered application events.
- Renderer failure is isolated from session process ownership; the application may recreate rendering resources without silently terminating child processes.

The GUI toolkit and graphics backend are intentionally not selected by this contract.

## 10. Configuration and diagnostics contract

Configuration is loaded into a validated immutable snapshot before session creation. Invalid configuration produces field-specific diagnostics and either a safe documented default or a startup error; no silent coercion of security-sensitive fields is permitted.

Diagnostics must include subsystem, operation, stable error category, and safe context. They must not include passwords, access tokens, secret environment values, or full raw command input. User-facing text is separate from internal diagnostic detail.

## 11. Compatibility and versioning

- The Rust crate API follows normal semver rules before 1.0, but public cross-crate contracts still require explicit review.
- The C ABI uses `DeAbiVersion { major, minor, patch }`.
- A major mismatch is incompatible and prevents session creation.
- A minor increase may add optional functions or enum values only when old callers remain safe; callers must handle unknown values.
- A patch increase contains compatible fixes and does not change layout or existing numeric values.
- Struct layout changes, ownership changes, threading changes, or semantic changes that can invalidate callers require a major ABI version.
- Version checks occur before any session handle is created.
- Feature detection is explicit; callers must not infer support from a version string alone.

## 12. Resource and security constraints

- Session I/O queues, parser buffers, scrollback, and diagnostic buffers have configurable hard limits.
- Allocation failure is reported; it must not corrupt existing session state.
- All byte streams are untrusted, including PTY output and paste data.
- Executable and arguments remain separate; shell expansion belongs only to the shell implementation.
- Environment overrides and working directories are validated before spawn.
- FFI validates all pointers, lengths, enum values, handle states, and ABI versions.
- One session cannot access another session's runtime handle or terminal state.
- Secrets are excluded from diagnostics and crash context.

Exact numeric budgets are owned by performance/security tasks and must be set before release qualification; this contract requires limits but does not invent unmeasured values.

## 13. Contract test requirements

Each contract implementation must provide tests for:

| Boundary | Required cases |
| --- | --- |
| Session lifecycle | Every legal transition, illegal transition rejection, repeated stop/close, startup failure cleanup |
| Runtime spawn | Empty executable, invalid cwd, environment validation, argument preservation, spawn failure cleanup |
| PTY I/O | Partial read/write, arbitrary chunk boundaries, EOF, hangup, child exit, signal, resize failure |
| Session isolation | Two sessions with independent IDs, output, resize, exit, and teardown |
| Terminal engine | Fragmented escape/UTF-8 input, malformed sequences, zero-size rejection, buffer/resource limits |
| FFI | ABI mismatch, null/invalid pointers, length overflow, unknown enum/status, stale handle, concurrent-call rejection |
| Renderer | Borrowed frame lifetime, redraw coalescing, renderer failure without session loss |
| Diagnostics | Secret redaction, stable error category, bounded output |

Contract tests must use real implementations at the boundary under test. Mocks may be used only for external OS failures that cannot be made deterministic; they do not replace the implementation-level contract tests.

## 14. Implementation order

1. Freeze this contract and record any required changes as reviewable decisions.
2. Implement C library lifecycle and input-buffer contracts (#31–#32).
3. Implement lexer/parser/command planning (#33–#37).
4. Implement runtime interface and Linux adapter (#35, #48).
5. Implement terminal model and parser (#42–#44).
6. Implement PTY lifecycle and I/O (#45–#47).
7. Implement versioned ABI and safe Rust wrapper (#49–#50).
8. Add cross-component integration tests and expand the continuous QA harness (#38, #65–#66).

A dependent implementation must not invent a conflicting interface locally. Update this contract first, then implement and test the change.
