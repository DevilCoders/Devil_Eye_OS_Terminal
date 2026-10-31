# Application Lifecycle

This document describes the implemented `de-app` service lifecycle API. It is a bootstrap contract, not a GUI, PTY, or terminal-session implementation.

## Ownership

`Application` owns registered services and is the sole coordinator of their initialization and shutdown. Services are registered before startup. The application does not create operating-system resources itself; concrete services own the resources they initialize.

The current API is synchronous. Startup and shutdown must be called from the application control thread, not from a GUI event callback that must remain responsive. An asynchronous service executor can be introduced by a later task without changing the ownership and ordering invariants.

## Startup

1. Services are registered in dependency order.
2. `Application::start` transitions `Created -> Starting`.
3. Each service initializes in registration order.
4. The application transitions to `Running` only after every service initializes successfully.

If service N fails:
- The failing service is included in cleanup because it may have partially initialized.
- The failing service and all earlier successfully initialized services receive `shutdown` in reverse registration order.
- Cleanup continues even if one service's shutdown fails.
- The application transitions to `Failed` and returns the original startup failure plus every cleanup failure.
- Services whose cleanup failed remain owned by the application and can be retried with `shutdown`.

A service's `shutdown` implementation must be idempotent and safe after partial initialization.

## Shutdown

- `shutdown` is called in reverse initialization order.
- One cleanup failure does not prevent cleanup attempts for remaining services.
- Successful services are removed from the active set.
- Failed services remain active and can be retried by another `shutdown` call.
- The application reaches `Stopped` only when all active services have been released.
- Calling `shutdown` after reaching `Stopped` is idempotent.
- Shutting down an application that has not started transitions it directly to `Stopped`.

The application does not silently discard cleanup errors. It returns all service names and failure descriptions to the caller.

## State machine

```text
Created -> Starting -> Running -> Stopping -> Stopped
                 |         |          |
                 v         v          v
               Failed <----+----------+
                 |
                 +---- shutdown retry ----> Stopped
```

The public `ApplicationState` exposes `Created`, `Starting`, `Running`, `Stopping`, `Stopped`, and `Failed`. Invalid operations return `LifecycleError::InvalidState`.

## API contract

- Register services only in `Created`.
- Start exactly once; a failed start cannot be restarted on the same instance.
- Service names must be stable and safe for diagnostics.
- Service errors must not contain secrets, passwords, tokens, or raw environment values.
- No panic is used for expected service failures.
- `Application` does not implicitly perform fallible cleanup in `Drop`; the owner must call `shutdown` and handle the result.
- The richer application-wide diagnostics and error classification model is owned by DE-003-02 (#40).

## Verification

Integration tests in `crates/de-app/tests/lifecycle.rs` verify:
- Initialization in registration order.
- Reverse-order shutdown.
- Cleanup of a service that fails partway through initialization.
- Continued cleanup after an individual shutdown failure.
- Retry of failed cleanup without repeating successful cleanup.
- Invalid repeated startup and late registration.
- Empty-application startup and shutdown.
