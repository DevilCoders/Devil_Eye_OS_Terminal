# ADR-0003: One PTY/process lifecycle per terminal session

- Status: Accepted
- Date: 2026-10-31
- Decision owners: Architecture maintainers
- Related issues: #45, #46, #47, #59

## Context

Tabs and panes need predictable process ownership, independent resize/input/output, and deterministic cleanup. Sharing one PTY between unrelated tabs would mix input/output streams and make lifecycle and security boundaries ambiguous.

## Decision

Each independent terminal session owns one PTY-backed runtime child-process handle. The session manager owns lifecycle transitions and routes all input, output, resize, exit, and shutdown events using a unique session identifier.

A view or pane displaying an existing session is a view, not another process owner. Creating a new independent session allocates a new PTY and child process. The runtime owns native PTY descriptors and child handles and closes/reaps them exactly once.

DE-Shell sessions use the same terminal-facing semantics but do not masquerade as an external process. The DE-Shell adapter is a distinct backend with a versioned FFI handle and explicit host-runtime requests.

## Alternatives considered

1. **One shared PTY for the whole application:** rejected because sessions would share streams, dimensions, and process lifecycle.
2. **One child per visible pane regardless of session identity:** rejected because views should not implicitly duplicate processes.
3. **GUI owns raw file descriptors:** rejected because native resource lifecycle belongs to the runtime adapter.

## Consequences

Session isolation is straightforward to test and close independently. The application must enforce bounded per-session queues and prevent cross-session handle reuse. Graceful shutdown may wait for a bounded grace period and must run off the GUI event loop.

## Affected interfaces and verification

- [Linux PTY Backend](../linux-pty.md)
- [Linux PTY Child Process Lifecycle](../linux-pty-process.md)
- [Linux PTY I/O Contract](../linux-pty-io.md)
- [Terminal Session Manager](../session-manager.md)
- [Cross-Component API Contracts](../api-contracts.md)

Verify two-session isolation, partial I/O/backpressure, resize independence, child exit/reap, failed-start cleanup, repeated close, and manager shutdown.
