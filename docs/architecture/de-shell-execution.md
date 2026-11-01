# DE-Shell Execution Engine

## Scope

DE-004-05 executes the alpha parser's AST through a controlled dispatch layer. The C library owns parsing, quote removal, parameter expansion, session-local variables, built-in lookup, and shell control-flow decisions. It does not create operating-system processes. External commands are delegated to a host-provided callback so the Rust runtime can own process creation and resource cleanup.

## Public API

The API is declared in `de_shell/execution.h`:

- `de_shell_execute_source` accepts one bounded source buffer, an immutable execution context, a result, and a diagnostic.
- `de_shell_execution_context_t` carries explicit environment entries, an optional working directory, an external executor callback, and borrowed user data.
- `de_shell_external_command_t` carries an executable and separate `argv` entries, environment entries, and working directory.
- `de_shell_execution_result_t` reports the last exit code, an exit request, and the number of successfully dispatched commands.
- `de_shell_execution_diagnostic_t` identifies the failure stage, underlying code, and byte offset.

No host environment or process working directory is read or mutated by the C library. Context values are passed to the executor unchanged. A NULL working directory means the host runtime's documented default.

## External process boundary

The C core does not call `system()`, `popen()`, `fork()`, `exec()`, or an implicit shell interpreter. The host supplies `de_shell_external_executor_t`, which receives:

- `executable`: exactly `argv[0]`.
- `argc` and `argv`: separately decoded arguments; no command-line concatenation. The pointer array contains exactly `argc` arguments followed by a NULL sentinel.
- Explicit environment key/value pairs.
- Optional working directory.

The callback is synchronous. It returns `DE_SHELL_STATUS_OK` after launch/wait handling and writes the process exit code. A non-OK callback status becomes `DE_SHELL_STATUS_EXTERNAL_EXECUTION_FAILED` with the original status in the diagnostic. The callback must not retain any borrowed pointer after it returns. A future Rust/C FFI adapter can bridge this callback to `de-runtime`.

## Alpha execution semantics

Supported AST nodes:

- Simple commands.
- Sequence operator `;`.
- AND operator `&&`, executing the right side only after exit code 0.
- OR operator `||`, executing the right side only after a non-zero exit code.
- The per-shell `exit` built-in, which requests shell termination and short-circuits the remainder of the current list.

Words are decoded from their original source spans. Single and double quote delimiters are removed; backslash escaping is applied; `$NAME`, `${NAME}`, and `$?` expand outside single quotes. Expansion is scalar and non-recursive: values containing spaces remain one argument, wildcard bytes are not expanded, and empty values preserve an empty argument. Empty quoted arguments remain empty strings. UTF-8 bytes are preserved.

Assignment-only commands such as `NAME=value` update the bounded variable store owned by the shell handle. Assignment words before an external command are temporary environment overrides for that invocation and are not persisted. Assignment words after the command name are ordinary arguments. Prefix assignments before built-ins are rejected until a built-in environment contract is defined. The host environment is visible only through the explicit execution context; the C core never calls `getenv()`.

The shell handle retains the last completed command status across `de_shell_execute_source` calls, so `$?` works across interactive input lines. Expansion occurs only when a conditional AST operand is selected; skipped `&&`/`||` operands are neither expanded nor executed.

## Explicitly unsupported execution nodes

The alpha parser recognizes more syntax than this execution task implements. The execution engine returns `DE_SHELL_STATUS_UNSUPPORTED_OPERATION` for:

- Pipelines.
- Background jobs.
- Parenthesized subshell groups.
- Input/output/append redirections.

These nodes are not silently executed as ordinary commands. Pipeline, redirection, and job-control behavior belongs to the later execution/backend tasks. DE-012-02 (#58) implements scalar variable expansion and assignment semantics.

## Bounds and failure behavior

- Source: 65,536 bytes maximum.
- Tokens and AST nodes: 4,096 each.
- AST words: 4,096.
- Redirection records: 1,024.
- Arguments per simple command: 256.
- Decoded bytes per word: 4,096.
- Environment entries: 256.
- Environment key: 256 bytes maximum.
- Environment value and working directory: 4,096 bytes maximum.
- Shell variables per session: 128.
- Variable name: 256 bytes maximum.
- Variable value: 4,096 bytes maximum.

Allocation failure is reported as an internal error. Input/AST/argument bounds return a resource-limit status. Lexing and parsing failures return a parse status with byte offset. External launch/runtime failures preserve the callback status in the diagnostic. Normal non-zero process exit codes are results, not API failures.

The result is initialized before execution. If a later command fails at the API boundary, the result retains the last completed command's exit status and the count of commands successfully dispatched; already-completed external side effects cannot be rolled back.

## Reentrancy and ownership

A shell instance is single-thread-affine. One execution call may be active per shell. Reentrant execution and destruction from an external executor callback return `DE_SHELL_STATUS_BUSY`; the shell handle remains valid. Callers must invoke synchronous execution from a worker thread if the host executor may block.

The C core owns its temporary token, AST, argument, and traversal buffers and frees them on every return path. The caller owns the shell handle and all context data. No context or argument pointer is retained after the call.

## Tests

CTest covers built-in exit and short-circuiting, quote removal and empty arguments, explicit environment/working-directory forwarding, AND/OR/sequence control flow, callback error propagation, missing executor behavior, malformed input, unsupported pipeline/redirection/background nodes, reentrancy/destruction guards, context validation, source bounds, empty input, scalar/braced/status expansion, single-quote and escape suppression, assignment persistence, temporary command-prefix environment overrides, non-recursive values, skipped-operand laziness, and expansion output limits.
