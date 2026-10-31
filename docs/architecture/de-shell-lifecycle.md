# DE-Shell Library Lifecycle

## Scope

DE-004-01 introduces the independent C library boundary, versioned ABI metadata, opaque handles, and a minimal per-instance lifecycle. It does not implement shell input buffering, lexical analysis, parsing, command dispatch, process execution, or terminal emulation.

## Public API

The public header is `include/de_shell/shell.h`.

- `DeShell` is an opaque, caller-owned handle.
- `de_shell_abi_version()` returns ABI version 0.1.0, matching `de_ffi::ABI_VERSION`.
- `de_shell_create(DeShell **out_shell)` allocates one independent instance and returns it through an output parameter.
- `de_shell_start(DeShell *shell)` transitions Created to Running.
- `de_shell_shutdown(DeShell *shell)` transitions Created or Running to Stopped; repeated shutdown is idempotent.
- `de_shell_get_state()` reads the lifecycle state through a caller-provided output pointer.
- `de_shell_destroy(DeShell *shell)` shuts down and releases the instance.

The C ABI uses fixed-width integer types. `DeAbiVersion` is three `uint16_t` fields and has a six-byte layout. `DeStatus` is `int32_t` and matches the stable Rust values: OK = 0, INVALID_ARGUMENT = 1, INTERNAL_ERROR = 2. Shell state values are `uint32_t` constants: INVALID = 0, CREATED = 1, RUNNING = 2, STOPPED = 3.

## Ownership and threading

A successful create transfers ownership of the handle to the caller. The caller must release it exactly once with `de_shell_destroy`. The handle must not be inspected, freed directly, copied and used after destruction, or called concurrently. One handle represents one shell session.

`de_shell_destroy` accepts a live handle, performs shutdown, invalidates its internal marker, and frees the allocation. Passing NULL is invalid. The pointer becomes unusable after destroy returns.

## State transitions

- Created -> Running
- Created -> Stopped
- Running -> Stopped
- Stopped -> Stopped (idempotent shutdown)

Starting an instance outside Created is rejected. A stopped instance cannot restart. Destroy releases either a Created or Stopped instance and also shuts down a Running instance before freeing it.

## Error and allocation behavior

- Invalid pointers or invalid state operations return `DE_STATUS_INVALID_ARGUMENT`.
- Allocation failure returns `DE_STATUS_INTERNAL_ERROR` and leaves the output handle NULL.
- `de_shell_get_state` initializes its output to `DE_SHELL_STATE_INVALID` before validating the handle.
- No global mutable shell state is used; each handle owns its own lifecycle state.
- The library does not retain caller-owned buffers or pointers.

## Tests

The CTest lifecycle suite covers:
- ABI version values and structure layout.
- Successful create/start/state-query/shutdown/destroy.
- Invalid output pointers and NULL handles.
- Rejected repeated start and restart after shutdown.
- Idempotent shutdown.
- Cleanup of a Created instance.
- Independence of two concurrently allocated shell handles.

Allocation failure is checked in the implementation but is not fault-injected because allocator injection is outside this task.

## Deliberate non-goals

The opaque instance has no parser, input buffer, command registry, process handle, PTY, or execution behavior at this stage. Those concerns belong to later DE-004, DE-007, and DE-006 tasks. External shells remain independent of DE-Shell.
