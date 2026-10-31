# VT/ANSI Parser Contract

## Scope and ownership

The terminal parser is a platform-independent, incremental byte-stream state machine. It recognizes the 7-bit ESC, CSI, and control-string framing used by VT/ANSI terminals and emits owned events for the terminal state layer.

The parser does not own the screen grid, cursor, renderer, PTY, or shell. It does not perform grapheme segmentation or assign Unicode display widths. Printable bytes, including UTF-8 sequences, are forwarded unchanged so the text-layout layer can decode and assemble grapheme clusters without losing byte boundaries.

## Public API

- Module: `de_terminal_core::parser`
- State machine: `VtParser`
- Entry point: `VtParser::process(&[u8]) -> Result<Vec<TerminalEvent>, ParserError>`
- Reset: `VtParser::reset()`
- State inspection: `VtParser::is_ground()`

Parser state persists across calls. A partial ESC, CSI, or control string at the end of one chunk is continued by the next call. Events own their data; no event borrows the input buffer.

## Bounded-resource rules

| Resource | Limit |
| --- | ---: |
| Input per process call | 4096 bytes |
| Raw CSI parameter bytes | 64 bytes |
| ESC/CSI intermediate bytes | 2 bytes |
| Control-string payload | 4096 bytes |
| Events per call | At most input bytes |

A chunk larger than the input limit is rejected before parser state changes. CSI and ESC sequences exceeding their fixed storage are reported as malformed and ignored through their final byte. Control-string payload is discarded as it arrives; it is never retained in memory. An over-limit control string is reported when its terminator arrives.

The parser allocates one event vector per accepted chunk, with capacity no greater than the chunk length. It does not allocate per sequence or retain input payloads.

## Recognized framing

### Ground state

- Printable ASCII and bytes >= 0x80 are emitted as `PrintByte`.
- C0 controls and DEL are emitted as `Execute`.
- ESC (0x1b) begins an escape sequence.

High bytes are forwarded verbatim. 8-bit C1 controls are intentionally not decoded because their byte values overlap UTF-8 continuation bytes. The initial parser supports 7-bit ESC-prefixed control forms.

### ESC sequences

- CSI introducer: ESC `[`
- OSC: ESC `]`
- DCS: ESC `P`
- SOS: ESC `X`
- PM: ESC `^`
- APC: ESC `_`
- Other valid ESC sequences are emitted as `Escape`, preserving intermediate and final bytes.

### CSI sequences

CSI parameter bytes (0x30..=0x3f), intermediate bytes (0x20..=0x2f), and final bytes (0x40..=0x7e) are preserved. The parser does not impose command-specific parameter defaults or execute terminal semantics; those belong to the CSI command handler. This keeps grammar recognition separate from screen mutation.

The parser permits at most 64 raw parameter bytes and two intermediate bytes. Parameter bytes after an intermediate are malformed. Over-limit or malformed sequences are discarded until a final byte, cancellation, or a new ESC introducer restores synchronization.

### Control strings

OSC terminates with BEL or ST (ESC backslash). DCS, SOS, PM, and APC terminate with ST. Their payloads are intentionally discarded and surfaced as `UnsupportedControlString(kind)`; payloads are not interpreted as commands or rendered as text.

CAN (0x18) and SUB (0x1a) cancel an incomplete escape, CSI, or control string. ESC inside a control string is treated as a possible ST prefix; if not followed by backslash, those bytes remain part of the discarded payload.

## Error and recovery behavior

- Call-level input overflow returns `ParserError::InputChunkTooLarge` and leaves parser state unchanged.
- Invalid sequence bytes produce `TerminalEvent::Malformed(InvalidByte)`.
- Overlong ESC, CSI, and control strings produce their corresponding `SequenceError`.
- CAN/SUB produce `TerminalEvent::Cancelled`.
- Malformed and over-limit sequences are discarded without panic, unbounded allocation, or out-of-bounds access.
- A final byte, cancellation, or new ESC where defined returns the parser to a synchronized state.

Unsupported control strings are explicitly reported rather than silently interpreted. Unknown but syntactically valid ESC and CSI sequences are emitted as raw structured events so the consumer can implement, ignore, or report them according to terminal capability policy.

## Threading

A parser instance is mutable state and must be owned by one session/parser execution context at a time. Independent terminal sessions use independent parser instances. Calls on the same parser must be serialized to preserve byte order.

## Verification

Unit tests cover split chunks, CSI parameters and intermediates, ESC sequences, OSC BEL termination, ST split across chunks, cancellation, malformed/overlong recovery, input chunk rejection without state mutation, and deterministic pseudo-random fuzz input. The fuzz-style test feeds thousands of arbitrary bytes in bounded chunks and asserts event bounds and recovery without panics.

The parser only recognizes sequence framing in this task. CSI command execution, VT mode handling, Unicode grapheme assembly, and renderer integration remain separate responsibilities.
