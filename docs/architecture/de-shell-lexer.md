# DE-Shell Lexer

## Scope and boundary

DE-004-03 introduces a deterministic, allocation-free lexer for the DE-Shell input byte stream. It emits token types and byte-based source spans while retaining original source bytes. It does not expand variables, evaluate commands, parse shell grammar, execute processes, or emulate a terminal.

The lexer is internal to the C library at this stage. Its public Rust/C ABI is not exposed.

## Input and output contract

`de_shell_lex()` receives a pointer/length source pair, caller-owned token storage, token capacity, output token count, and a diagnostic structure.

- Empty source is valid and yields zero tokens.
- All offsets and lengths are byte-based.
- Word tokens retain the original source span, including quote and escape characters.
- Quote/escape flags summarize constructs present in each word.
- Operators are emitted as dedicated tokens.
- No memory allocation occurs.
- The caller must keep the source buffer alive while consuming token spans.
- On failure, output token count remains zero. The token array may contain partial writes and must be ignored.

## Token set

| Token | Source operator |
| --- | --- |
| `WORD` | Any non-whitespace word span not beginning with an unquoted operator |
| `PIPE` | `|` |
| `AND_IF` | `&&` |
| `OR_IF` | `||` |
| `SEMICOLON` | `;` |
| `REDIRECT_INPUT` | `<` |
| `REDIRECT_OUTPUT` | `>` |
| `REDIRECT_APPEND` | `>>` |
| `LEFT_PAREN` / `RIGHT_PAREN` | `(` / `)` |
| `BACKGROUND` | `&` |

The lexer recognizes these operators longest-first. Operators inside single or double quotes, or escaped outside quotes, remain part of a word span. Here-documents, file-descriptor-prefixed redirects, arithmetic syntax, and grammar-specific constructs are not implemented by this task.

## Quoting and escapes

- Single quotes group all bytes until the next single quote; backslash has no special meaning inside single quotes.
- Double quotes group bytes until the next unescaped double quote.
- Backslash outside quotes escapes the following byte.
- Backslash inside double quotes preserves the following byte as part of the same word and marks the word as escaped.
- Quotes and backslashes remain in the raw source span. The lexer does not perform quote removal or expansion.
- A word may contain adjacent unquoted, single-quoted, and double-quoted sections; it remains one WORD token.

This is lexical grouping, not a final POSIX shell semantic specification. The full language grammar and expansion rules are frozen by DE-012-01 (#57).

## Deterministic diagnostics

The lexer reports a status and the source byte offset for:

- Invalid arguments.
- Token output capacity exhaustion.
- Unterminated single quote.
- Unterminated double quote.
- Trailing escape.
- Embedded NUL byte.

For an unterminated quote, the diagnostic points to the opening quote. For a trailing escape, it points to the backslash. For token-capacity exhaustion, it points to the first token that could not be emitted.

## Tests

CTest covers whitespace and source spans, quoted words, escaped whitespace and quotes, operator recognition, malformed quotes, trailing escapes, token-capacity boundaries, empty input, invalid pointers, and embedded NUL bytes.
