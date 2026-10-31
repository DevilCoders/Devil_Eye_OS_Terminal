# DE-Shell Input Buffer

## Scope

DE-004-02 adds a bounded, byte-oriented input buffer used by the DE-Shell core. It owns its storage and exposes an internal interface; it does not perform UTF-8 decoding, shell tokenization, parsing, expansion, or command execution.

## Capacity and ownership

- Creation requires a non-zero maximum byte length.
- The implementation allocates the buffer and its storage once; the configured maximum is a hard bound.
- The caller owns the opaque buffer handle and releases it through the destroy function.
- Destroy clears the caller's handle and is idempotent for a valid pointer-to-handle.
- All input and output operations use explicit pointer/length pairs. Input is not assumed to be NUL-terminated.
- A rejected append does not mutate already buffered bytes.

The maximum is expressed in bytes, not Unicode scalar values or grapheme clusters. UTF-8 validation belongs to the lexer/parser boundary.

## Line submission

Appending bytes and submitting a line are separate operations:

1. Append one or more byte chunks.
2. Call `de_shell_input_buffer_submit_line()` when the caller receives the line delimiter or an explicit submit event.
3. Read the submitted line into caller-owned storage.
4. The buffer resets to an empty, reusable state after a successful read.

Empty lines are valid submissions. A second submit before consuming the previous line is an invalid state. A destination that is too small does not consume or alter the pending line.

## EOF semantics

Marking EOF prevents further appends. If bytes are pending, they are exposed as one final unterminated line. After that line is consumed, subsequent reads return EOF. If no bytes are pending, reads return EOF immediately. Marking EOF repeatedly is harmless.

## Status behavior

The internal input-buffer status enum distinguishes:

- Success.
- Invalid arguments.
- Capacity exceeded.
- Allocation failure.
- No submitted line available yet.
- End of input.
- Invalid state transition.
- Destination buffer too small.

These are internal C subsystem statuses. They are not yet part of the public Rust/C ABI and do not change the stable public `de_shell_status_t` values.

## Tests

The CTest suite verifies empty line submission, exact maximum capacity, overflow atomicity, buffer reuse, EOF with and without pending bytes, append-after-EOF rejection, destination-too-small preservation, duplicate submission rejection, and invalid pointers.

Allocation failure is checked in the implementation but is not fault-injected because this task does not introduce an allocator-injection seam.
