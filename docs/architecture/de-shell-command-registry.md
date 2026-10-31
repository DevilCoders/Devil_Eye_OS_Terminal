# DE-Shell Command Registry

## Scope

The command registry stores command metadata, rejects name collisions, resolves exact command names, and dispatches registered built-in handlers. It is owned by one `de_shell_t` instance. This task does not parse shell grammar or execute external processes.

## Ownership

- Every shell instance owns one independent command registry.
- The registry copies command names and descriptions at registration time.
- Handler function pointers and `user_data` are borrowed; their code and data must remain valid until the owning shell is destroyed.
- The registry owns its entry array and copied metadata strings.
- A metadata pointer returned by lookup remains valid until the next successful registration or registry destruction.
- No global command table or mutable process-wide registry is used.

## Registration and collision policy

Command names are non-empty printable ASCII strings without whitespace or shell operator characters. Names are case-sensitive.

Registration rejects:
- NULL registry/definition/name/handler.
- Empty or invalid names.
- Minimum argument count greater than maximum.
- Duplicate names.

A duplicate never replaces or mutates the original command. Registration order is preserved; lookup is an exact linear search, making behavior deterministic and suitable for the small initial built-in set.

## Metadata

Each command definition includes:
- Name and optional description.
- Minimum and maximum argument count (excluding `argv[0]`).
- Flags, including `DE_SHELL_COMMAND_FLAG_TERMINATES_SHELL`.
- A handler function and optional borrowed user data.

`DE_SHELL_COMMAND_MAX_ARGS_UNBOUNDED` represents an unbounded upper argument count.

## Dispatch contract

Dispatch receives:
- The owning `de_shell_t` instance.
- `argc` and `argv`, where `argv[0]` is the exact command name.
- An output command result.

The registry validates all pointers, resolves the exact name, checks the argument count, and invokes exactly one handler. The handler result is separate from registry status:
- `SUCCESS`: command completed successfully.
- `EXIT_REQUESTED`: the owning shell should begin termination.
- `FAILURE`: the handler ran but reports command-level failure.

An unknown command, argument-count mismatch, or invalid invocation is returned as a registry status and does not invoke a handler.

## Core built-in

Each newly created shell registers the `exit` built-in. It accepts no arguments, carries the terminate-shell flag, and returns `EXIT_REQUESTED`. The application/session layer remains responsible for performing the actual session shutdown.

## Tests

CTest verifies registration and metadata lookup, unknown names, duplicate collision atomicity, deterministic handler dispatch, argument-count rejection, the core `exit` result, per-shell registry isolation, invalid names, and idempotent registry cleanup.
