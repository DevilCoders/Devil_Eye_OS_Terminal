# DE-Shell Library Lifecycle

## Scope

DE-004-01 introduces the independent C library boundary, an opaque per-instance handle, and its initial create/destroy lifecycle. The per-instance command registry and core `exit` built-in are added by DE-013-01. Input buffering, lexical analysis, parsing, external process execution, and terminal emulation remain separate components.

## Public API

The public header is `include/de_shell/shell.h`.

- `de_shell_t` is an opaque, caller-owned handle.
- `de_shell_create(de_shell_t **out_shell)` allocates one independent instance and returns it through an output parameter.
- `de_shell_destroy(de_shell_t **shell)` releases the instance and clears the caller's handle.
- `de_shell_status_t` uses the stable initial status values also defined by Rust's `DeStatus`: OK = 0, INVALID_ARGUMENT = 1, INTERNAL_ERROR = 2.

A create call validates its output pointer and initializes the output to NULL before allocation. A successful create transfers ownership of the new handle to the caller. Destroy accepts the address of that handle, clears it before releasing the allocation, and treats an already-cleared handle as a successful no-op. A NULL pointer-to-handle is invalid.

The caller must not inspect the opaque representation, free it directly, use a copied alias after destruction, or call the handle concurrently. These constraints are consistent with the ownership and single-thread-affinity baseline in the cross-component API contract.

## Private representation

The implementation-only structure is declared in `c/internal/shell_internal.h`. Each shell instance owns its own command registry; the registry's private API and metadata are not installed as public headers and must not leak into Rust or other consumers.

## Error and allocation behavior

- Invalid pointer arguments return `DE_SHELL_STATUS_INVALID_ARGUMENT`.
- Allocation failure returns `DE_SHELL_STATUS_INTERNAL_ERROR` with the output handle left NULL.
- Cleanup through a valid pointer-to-handle is idempotent, including when the stored handle is already NULL.
- No error path writes to caller memory unless the output pointer was validated.

## Tests

The CTest lifecycle suite covers successful creation/destruction, invalid arguments, idempotent cleanup, and independent instances. Allocation failure is not fault-injected because no allocator injection mechanism is part of this task; it remains a direct, checked failure path.

## Deliberate non-goals

The opaque instance has no parser, input buffer, process handle, PTY, or external execution behavior at this stage. Those concerns belong to DE-004, DE-007, and DE-006 tasks. The built-in registry only resolves in-process handlers; it does not launch programs. External shells remain independent of DE-Shell.
