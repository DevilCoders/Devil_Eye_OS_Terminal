# Application Service Interfaces

This document describes the public subsystem contracts exposed by `de-app::service_interfaces`. It defines ownership and lifecycle boundaries; it does not claim that the Linux runtime, terminal parser, renderer, configuration store, or C shell adapter is already implemented.

## Common service lifecycle

Every long-lived subsystem service extends `ApplicationService`. The application owns each registered service and controls initialization and shutdown order through the application lifecycle coordinator.

Service methods return `ServiceError` for expected operational failures. The application error/diagnostics layer can wrap these errors with stable categories and safe user-facing summaries.

## Session identity and lifecycle

- `SessionId` is an opaque, non-zero application-generated identifier.
- A session manager owns its session objects. Callers retain only the stable ID unless the manager explicitly transfers an owned session handle.
- Every terminal tab is an independent session with its own process/runtime handle and terminal state.
- Session state transitions are validated by `SessionLifecycle`.

Legal transitions:

```text
Created -> Starting -> Running -> Exited -> Stopping -> Closed
                 |         |          |
                 v         v          v
               Failed <----+----------+
                 |          |
                 +----------+----> Stopping -> Closed
```

A closed session cannot be restarted. A new session receives a new ID.

## Runtime and process ownership

`RuntimeService` creates a process from a validated `ProgramSpec` and a non-zero `TerminalSize`. Executable, arguments, environment overrides, and working directory remain separate values; the runtime must not implicitly concatenate them into a shell command.

The returned `RuntimeProcess` handle is uniquely owned by the requesting session. It owns native process/PTY resources and exposes:
- Partial input writes.
- Non-blocking reads with explicit Data / WouldBlock / EOF outcomes.
- PTY resize.
- Graceful or forced termination.
- Non-blocking child exit polling.

The session is responsible for driving the process handle and releasing it through the runtime's shutdown contract. The GUI thread must not block on process I/O or child waits.

## Configuration ownership

`ConfigurationService` loads a validated `AppConfig` snapshot and persists validated snapshots. The service owns file/registry persistence details; application/session code consumes the immutable configuration value and does not mutate the backing store directly.

## Terminal engine and renderer boundary

`TerminalEngineService` owns parser state, screen state, terminal modes, and scrollback. It consumes arbitrary output byte chunks, applies resize operations, and produces an immutable `TerminalFrame`.

`RendererService<F>` borrows an immutable frame. It may not mutate terminal state or retain borrowed frame data beyond the documented frame lifetime. Rendering failure is reported to the application but does not transfer process/session ownership to the renderer.

## Shell integration boundary

`ShellIntegrationService` accepts input bytes and yields typed `ShellEvent` values:
- `Output`: bytes to pass to the terminal engine.
- `ExecutionRequest`: a validated `ProgramSpec` to be delegated to the runtime.
- `ExitRequested`: the owning session should begin shutdown.

The shell core does not own OS process handles and must not call `system()`, `popen()`, or an implicit command interpreter. The terminal engine remains shell-agnostic and must host external shells independently of DE-Shell.

## Session manager boundary

`SessionManagerService` creates, queries, and closes independent sessions. It returns `SessionId` values rather than leaking mutable session references. Closing one session must not mutate or terminate unrelated sessions.

## Contract verification

Integration tests in `crates/de-app/tests/service_interfaces.rs` exercise the real session lifecycle state machine, including:
- Non-zero session ID validation.
- Normal process/session exit transitions.
- Startup failure cleanup transitions.
- Illegal transition rejection without state mutation.
- Closed-session non-restart behavior.
- SessionSpec preservation of program arguments and terminal dimensions.
- Process read outcome and exit success semantics.

The traits are compile-checked by the workspace. Concrete platform, terminal, renderer, configuration, and shell implementations are delivered by their respective tasks.
