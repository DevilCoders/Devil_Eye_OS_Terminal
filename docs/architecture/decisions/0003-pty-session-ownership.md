# ADR-0003: Separate resource ownership for external and DE-Shell sessions

- Status: Accepted
- Date: 2026-10-31
- Decision owners: Architecture maintainers
- Related issues: #45, #46, #47, #49, #50, #59

## Context

Tabs and panes need predictable process ownership, independent resize/input/output, and deterministic cleanup. External programs require PTY/process resources, while DE-Shell is an in-process C language backend connected through a versioned FFI handle. Treating every terminal session as an external process would incorrectly require a PTY child-process handle for DE-Shell.

## Decision

- Each independent **external-program session** owns exactly one PTY-backed runtime child-process handle. The session manager owns lifecycle transitions and routes input, output, resize, exit, and shutdown events using a unique session identifier.
- Each independent **DE-Shell session** owns its own opaque DE-Shell FFI handle and shell-language state. It does not own a PTY-backed external child-process handle merely because it is displayed in a terminal tab.
- If DE-Shell requests an external program, the Rust host runtime creates a separate child-process handle for that request. A PTY is attached only when the host execution contract explicitly requests terminal attachment; ordinary child execution must not be silently converted into a PTY session.
- A view or pane displaying an existing session is a view, not another resource owner. Creating a new independent session allocates the appropriate backend resources for that session kind.
- The runtime owns native PTY descriptors and external child handles and closes/reaps them exactly once. The FFI boundary owns and destroys C shell handles according to its documented ABI contract.

## Alternatives considered

1. **One shared PTY for the whole application:** rejected because sessions would share streams, dimensions, and process lifecycle.
2. **One child per visible pane regardless of session identity:** rejected because views should not implicitly duplicate processes.
3. **Require a PTY process for every session kind:** rejected because DE-Shell is an in-process FFI backend, not an external shell process.
4. **GUI owns raw file descriptors or C handles:** rejected because native resource lifecycle belongs to the runtime/FFI ownership boundary.

## Consequences

External sessions retain strict PTY/process isolation. DE-Shell sessions retain independent language state without being forced through an external-process lifecycle. Host-launched child processes remain separately owned and must be scoped to the requesting shell session. The application must enforce bounded per-session queues and prevent cross-session handle reuse. Graceful shutdown may wait for a bounded grace period and must run off the GUI event loop.

## Affected interfaces and verification

- [Linux PTY Backend](../linux-pty.md)
- [Linux PTY Child Process Lifecycle](../linux-pty-process.md)
- [Linux PTY I/O Contract](../linux-pty-io.md)
- [Terminal Session Manager](../session-manager.md)
- [Cross-Component API Contracts](../api-contracts.md)
- [Rust/C ABI Contract](../ffi-abi.md)
- [Safe Rust DE-Shell FFI Wrapper](../ffi-rust-wrapper.md)

Verify external-session isolation, DE-Shell handle isolation, independent cleanup, child-process requests originating from DE-Shell, partial I/O/backpressure, resize independence, child exit/reap, failed-start cleanup, repeated close, and manager shutdown.
