# DE-Shell Language Grammar and Execution Semantics

**Status:** Normative language design for the initial DE-Shell language. This document separates the target semantics from the smaller alpha subset currently implemented by the lexer, parser, and execution engine. It is not a POSIX-shell compatibility claim.

## 1. Language goals and boundaries

DE-Shell is a small command language hosted by the terminal application. Its C core owns tokenization, parsing, shell variables, built-in dispatch, and shell control flow. The Rust runtime owns operating-system process creation, pipes, file descriptors, signals, and child reaping through explicit callbacks/contracts.

The initial language prioritizes deterministic parsing, bounded resource use, predictable quoting, and safe process delegation. It does not execute source through `system()`, `popen()`, `/bin/sh -c`, or another implicit command interpreter.

The grammar is deliberately smaller than POSIX shell. Unsupported syntax must be diagnosed or rejected; it must never be silently reinterpreted as a different command.

## 2. Lexical model

### 2.1 Source representation

- Source is a UTF-8 byte buffer with an explicit length; embedded NUL is invalid.
- The current execution API limits source to 65,536 bytes.
- Diagnostics and AST spans use byte offsets into the original source, not character indices.
- The lexer preserves raw source spans. It does not allocate decoded strings or perform expansion.
- Token and AST storage is bounded and caller-owned at the parser boundary.

### 2.2 Whitespace and word boundaries

Outside quotes, space, tab, LF, CR, vertical tab, and form feed separate words. Quoted or escaped whitespace remains part of a word.

For the target language, an unquoted LF or CRLF between complete list items is also a command-list separator, equivalent to `;`. A backslash followed by LF/CRLF is a line continuation and contributes no word byte. The current alpha lexer treats line breaks as whitespace rather than list separators; this is a documented implementation gap, not a claim that multiline command separation already works.

### 2.3 Operators

The initial operator set is:

| Token | Meaning |
| --- | --- |
| `|` | Pipeline |
| `&&` | Execute right side only after status 0 |
| `||` | Execute right side only after non-zero status |
| `;` | Sequential list separator |
| `&` | Background list separator |
| `(` and `)` | Subshell group delimiters |
| `<` | Redirect standard input |
| `>` | Redirect standard output, truncate/create |
| `>>` | Redirect standard output, append/create |

Operators are recognized only outside quotes and when not escaped. Adjacent operators do not require surrounding whitespace. The longest operator is recognized first (`&&`, `||`, `>>` before their single-character prefixes).

The initial subset does not include descriptor prefixes such as `2>`, descriptor duplication (`2>&1`), here-documents, here-strings, or `<<<`.

### 2.4 Comments and unsupported tokens

The initial language does not define shell comments. `#` is an ordinary word character. It must not be silently treated as a comment marker until comment syntax is added to the lexer and grammar together.

Globbing, tilde expansion, command substitution, arithmetic expansion, brace expansion, aliases, functions, loops, conditionals, and command separators such as `|||` are not part of this language version.

## 3. Quoting and escaping

A word is a concatenation of unquoted, single-quoted, double-quoted, and escaped segments. Adjacent segments form one argument; for example, `pre"mid"'post'` is one word. Empty quoted segments create an empty argument, so `''` and `""` are not discarded.

### 3.1 Unquoted text

- Unquoted ordinary bytes are literal word content.
- Unquoted whitespace ends the word.
- Unquoted operators end the word and become operator tokens.
- Backslash protects the following character from whitespace/operator interpretation and removes the backslash during quote removal.
- A trailing backslash is a lexical error with its source offset.

### 3.2 Single quotes

- A single quote begins/ends a single-quoted segment.
- Every character inside is literal, including whitespace, operators, dollar signs, and backslashes.
- No parameter expansion or backslash processing occurs inside single quotes.
- An unmatched single quote is a lexical error at the opening quote.

### 3.3 Double quotes

- Double quotes preserve whitespace and operator characters as part of the current word.
- Parameter expansion is enabled inside double quotes.
- Backslash is special only before double quote, backslash, dollar sign, and a line break. Before other characters it remains literal.
- An unmatched double quote is a lexical error at the opening quote.

The lexer records raw spans and quote/escape summary flags. DE-012-02 adds scalar parameter expansion to the execution decoder while preserving the original AST structure. Double-quoted backslash handling follows the explicit escape set above; unsupported substitutions remain literal or are rejected when a braced parameter form is malformed.

## 4. Grammar

The grammar below is EBNF over lexer tokens. A WORD token may contain multiple adjacent quoted/unquoted segments.

```text
program          ::= empty | list [list_separator]

list              ::= and_or {list_separator and_or}
list_separator    ::= ";" | "&" | line_break

and_or            ::= pipeline {("&&" | "||") pipeline}

pipeline          ::= command {"|" command}

command           ::= simple_command | subshell

subshell          ::= "(" list ")"

simple_command    ::= command_part {command_part}

command_part      ::= word | redirection

redirection       ::= "<" word
                    | ">" word
                    | ">>" word

word              ::= word_segment {word_segment}

word_segment      ::= unquoted_segment
                    | single_quoted_segment
                    | double_quoted_segment
                    | escaped_character
```

A redirection target is exactly one WORD token. Empty programs are valid no-ops. A simple command may contain only redirections; it does not require an executable word.

A trailing `;` or `&` is accepted. A trailing `|`, `&&`, or `||` is a syntax error. Empty groups are invalid. Parentheses must balance and nesting is bounded to 128 levels.

### 4.1 Operator precedence and associativity

From highest to lowest:

1. Word construction, quoting, and redirections within a simple command.
2. Pipeline operator `|`.
3. AND/OR operators `&&` and `||`, equal precedence and left-associative.
4. List separators `;`, `&`, and target-language line breaks.

Thus `a || b && c` parses as `(a || b) && c`. A pipeline is one operand of an AND/OR list. A list separator terminates the preceding AND/OR item.

The alpha parser already implements this precedence for tokenized `;`, `&`, `&&`, `||`, `|`, groups, and redirections. Newline-as-separator remains to be implemented.

## 5. Word expansion

The complete source buffer is parsed before execution begins, but word expansion is deferred until a pipeline operand is selected for execution. A list is evaluated left-to-right; within each `and_or`, the next pipeline operand is expanded only if the preceding status satisfies its `&&` or `||` condition. Expansion happens immediately before that selected operand's command lookup or redirection setup. This makes assignments and `$?` from a completed operand visible to the next selected operand, while skipped operands are neither expanded nor executed. Expansion never changes the AST operator structure: bytes produced by a variable cannot become a new pipe, separator, or redirection operator.

### 5.1 Supported parameter forms

The initial variable feature set is:

- `$NAME` — expand a valid identifier.
- `${NAME}` — braced form, required when adjacent characters would extend the identifier.
- `$?` — status of the last completed foreground command/list item.

Identifiers match `[A-Za-z_][A-Za-z0-9_]*`. An unset variable expands to an empty string. A dollar sign not followed by a supported form is literal. Command and arithmetic substitution are not supported.

### 5.2 Expansion context

- Single-quoted segments suppress parameter expansion.
- Unquoted and double-quoted segments permit parameter expansion.
- Backslash-escaped dollar signs are literal.
- Expansion is scalar: one parsed WORD always produces one argument. Variable values containing spaces do not undergo field splitting, and wildcard characters do not trigger pathname expansion.
- Empty expansion preserves the word as an empty argument. This keeps argument cardinality deterministic and avoids implicit splitting.
- Expansion output is bounded by the per-word decoded-byte limit. Overflow is a typed expansion/resource error; truncation is forbidden.

### 5.3 Expansion order

For each word:

1. Scan the original source span and identify quote/escape segments.
2. Expand supported parameters in unquoted and double-quoted segments only.
3. Remove quote delimiters and active escape backslashes.
4. Produce exactly one UTF-8 argument, preserving empty results and embedded spaces.
5. Validate the decoded byte limit and UTF-8 validity before command dispatch.

No field splitting, pathname expansion, command substitution, arithmetic expansion, or recursive re-expansion of variable values occurs. A value such as `$OTHER` stored inside variable `NAME` remains literal when `$NAME` is expanded; expansion is not recursively evaluated. For example, `x=one; echo "$x"` prints `one`; `x=one && echo "$x"` also prints `one`; and `true && echo "$?"` expands `$?` using the completed status of `true`. The right operand of `false && echo "$?"` is skipped and is not expanded.

### 5.4 Assignment words and variable scope

A valid assignment word has the form `NAME=value`, with the identifier grammar above. Assignment recognition occurs before ordinary command dispatch:

- If a simple command contains only assignment words, assignments update the current DE-Shell session's variable store and return status 0.
- Assignment words before the command name are temporary environment overrides for that one external command. They do not mutate the persistent shell variable store.
- A word containing `=` after the command name is an ordinary argument.
- Assignment right-hand sides use quote removal and supported parameter expansion, but remain a single value; whitespace is not split.
- Shell variables are session-local. The host environment is supplied explicitly by the runtime context and is never read or mutated through the C process environment.
- No `export` builtin is defined in this language version. Command-prefix assignments are the explicit mechanism for per-command environment overrides.

The execution API passes environment entries as explicit key/value pairs. DE-012-02 adds a bounded per-shell variable store and assignment semantics without reading or mutating the host process environment. The store is limited to 128 variables; names are limited to 256 bytes and values to 4,096 bytes. The last exit status is retained by the shell handle across input calls. Command-prefix assignments are temporary overrides for external commands; prefix assignments before built-ins are rejected until the built-in environment contract is defined.

## 6. Simple-command execution

After parsing and expansion:

1. Resolve assignment-only commands.
2. Resolve a built-in command in the DE-Shell command registry.
3. Otherwise construct an external request with a separate executable and argument vector.
4. Apply command-prefix environment overrides and the current working-directory context.
5. Delegate process creation and waiting to the host runtime callback.

The C core must never concatenate arguments into a command string. The executable and every argument remain separate NUL-terminated values at the current C callback boundary. Borrowed pointers are valid only for the callback duration.

Built-ins execute in the owning shell context when used as standalone commands. A built-in inside a pipeline or background job must execute in that pipeline/job's child context so that it cannot unexpectedly mutate the parent shell state. The existing `exit` builtin requests shell termination and stops the remainder of the current list.

## 7. Redirection semantics

The initial supported redirections are:

| Operator | Descriptor | Open behavior |
| --- | --- | --- |
| `< file` | stdin (0) | Open existing file read-only |
| `> file` | stdout (1) | Create or truncate, write-only |
| `>> file` | stdout (1) | Create or append, write-only |

Redirection targets are expanded using the same single-word rules. An empty target or a target that fails validation is an error before command launch.

Multiple redirections are applied in source order. Each successful redirection replaces the previous binding for that descriptor; therefore the last redirection for a descriptor determines the final binding. Earlier open/create/truncate side effects are not rolled back if a later redirection fails.

Redirection setup is owned by the runtime/platform layer. The C core emits typed redirection requests and does not open files itself. All temporary descriptors must have explicit ownership, close-on-exec behavior where appropriate, and deterministic cleanup on every launch/error path.

A redirection-only command performs its open/close operation in a temporary descriptor scope and does not permanently replace the application's own standard streams. Its status is 0 on success and non-zero on failure.

Descriptor-number prefixes, stderr redirection, descriptor duplication, here-documents, and here-strings are deferred extensions and must not be inferred from ordinary words.

## 8. Pipeline semantics

A pipeline contains two or more commands separated by `|`. For stages `C1 | C2 | ... | Cn`:

- stdout of `Ci` is connected to stdin of `Ci+1`.
- The first stage retains its configured stdin unless explicitly redirected.
- The last stage retains its configured stdout unless explicitly redirected.
- Explicit redirections are applied to the stage in source order; an explicit stdin/stdout redirection overrides the corresponding pipeline endpoint for that stage.
- All stages are launched before the runtime begins waiting for completion, preventing sequential-wait pipe deadlocks.
- Parent copies of pipe descriptors are closed immediately after child setup; each child receives only its intended descriptors.
- A launch failure terminates already-started stages according to the runtime cancellation contract, closes every pipe endpoint, and reaps every started child.
- A broken pipe is reported through the affected process status and diagnostics; it must not leak descriptors or hang the remaining pipeline.
- The pipeline exit status is the exit status of the last stage. A pipefail option is not part of the initial language version.
- Pipeline stages execute in child contexts. Built-in state changes in a stage do not mutate the parent shell session.

The current parser builds left-associated PIPELINE nodes, but the current execution engine deliberately returns UNSUPPORTED_OPERATION before launching any stage. Pipeline process/descriptor behavior is implemented by DE-012-03 (#79) together with the PTY/runtime contracts.

## 9. Lists, conditional execution, groups, and background jobs

### Sequential lists

`a ; b` executes a then b regardless of a's exit status. The list status is the status of the last item actually executed. A parse/expansion/launch error stops further execution and reports the failing stage.

### AND/OR lists

- `a && b`: execute b only if a returns status 0.
- `a || b`: execute b only if a returns non-zero status.
- AND and OR have equal precedence and associate left-to-right.
- The list status is the status of the last pipeline evaluated.

### Parenthesized groups

`( list )` is a subshell group. It executes in an isolated child context. Variable assignments, directory changes, and shell exit requests inside the group do not mutate or terminate the parent shell. Group exit status is the status of its list.

The parser currently emits GROUP nodes; the alpha execution engine rejects them until a child-context backend is implemented.

### Background separator

`command &` requests asynchronous execution and immediate return to the interactive shell. The runtime must own the background child and its process lifecycle; the shell maintains a bounded job record and reports completion asynchronously. Background jobs do not hold the foreground terminal input stream unless a later job-control contract explicitly assigns it.

The parser currently emits BACKGROUND nodes, but execution rejects them. Full job control, process groups, foreground handoff, and terminal ownership are not claimed by the current alpha implementation.

## 10. Exit status and error semantics

- Exit status 0 means success; non-zero means command failure.
- A normal non-zero child exit is a result, not an API-level execution failure.
- `&&` and `||` evaluate the status of the preceding pipeline/list operand.
- A sequential list reports the last executed item status.
- A pipeline reports the last stage status; no pipefail behavior is implied.
- An explicit `exit` request stops the remaining list and asks the owning shell session to terminate.
- Syntax/parse errors for the complete source buffer are detected before any command executes.
- Expansion errors are detected immediately before the affected pipeline operand executes, and only after its enclosing `&&`/`||` condition selects it. Earlier operands remain completed and their side effects are not rolled back; later operands/items are skipped after an expansion or runtime failure. Operands skipped by conditional execution are not expanded.

The current API result reports exit code, exit-request flag, and completed command count. These values describe work completed before an execution error.

## 11. Diagnostics

Diagnostics are structured and byte-offset based. The user-facing message layer must not parse human-readable error strings to determine control flow.

Required stages:

| Stage | Examples |
| --- | --- |
| Lex | Invalid NUL, unterminated quote, trailing escape, token capacity |
| Parse | Unexpected operator, missing word, unmatched parenthesis, nesting/capacity limit |
| Expand | Invalid variable syntax, invalid UTF-8, expansion output limit |
| Resolve | Unknown/invalid command or invalid assignment identifier |
| Redirect | Empty target, open failure, permission error, descriptor setup failure |
| Launch | Runtime rejected executable/arguments/environment/cwd |
| Wait | Signal termination, wait failure, child status collection failure |

Every diagnostic contains a stable stage/code and the earliest relevant source byte offset. Use SIZE_MAX when no source location exists. Line/column display is derived from the original UTF-8 source and must not replace the byte offset used by the API.

Diagnostics must not include secret environment values, passwords, tokens, or full expanded argument contents. A failed stage must still release its owned temporary resources. The parser/execution API initializes outputs before work and preserves completed-command counts on later failure.

## 12. Resource limits and safety invariants

Current alpha limits:

| Resource | Limit |
| --- | ---: |
| Source bytes | 65,536 |
| Tokens | 4,096 |
| AST nodes | 4,096 |
| AST words | 4,096 |
| Redirection records | 1,024 |
| Arguments per command | 256 |
| Decoded bytes per word | 4,096 |
| Environment entries | 256 |
| Environment key | 256 bytes |
| Environment value / working directory | 4,096 bytes |
| Shell variables per session | 128 |
| Variable name | 256 bytes |
| Variable value | 4,096 bytes |
| Parenthesis nesting | 128 |

New expansion buffers, variable storage, pipeline stages, and job records must have explicit limits before implementation. All arithmetic used for capacity calculation must be checked; allocation failure returns a resource error rather than truncating or panicking.

Security invariants:

- Never evaluate a variable's contents as new shell syntax.
- Never re-tokenize expanded bytes into operators.
- Never build a command string for an implicit interpreter.
- Never inherit or mutate the host environment implicitly; use explicit key/value entries.
- Validate every path and argument at the runtime boundary.
- Bound source, tokens, AST, decoded words, arguments, environment, pipes, and job records.
- Keep C and Rust ownership of buffers/descriptors explicit.
- No panic, exception, or undefined behavior may cross the Rust/C ABI.

## 13. Conformance matrix and implementation status

| Feature | Lexer/parser | Current executor | Target |
| --- | --- | --- | --- |
| Words and whitespace | Implemented | Implemented | Supported |
| Single/double quotes | Raw spans and flags | Quote removal | Supported; precise double-quote escapes to align |
| Backslash escapes | Implemented | Quote removal | Supported |
| Adjacent quoted segments | One WORD span | Decoded as one argument | Supported |
| Empty quoted argument | Implemented | Implemented/tested | Supported |
| `;` sequence | Implemented | Implemented | Supported |
| `&&` / `||` | Implemented | Implemented | Supported |
| `|` pipeline | AST implemented | Rejected | DE-012-03 |
| `<`, `>`, `>>` | AST implemented | Rejected | DE-012-03 |
| Parenthesized group | AST implemented | Rejected | Deferred child context |
| `&` background | AST implemented | Rejected | Deferred job-control backend |
| Parameter expansion | Not implemented | Not implemented | DE-012-02 |
| Variable assignments | Not implemented | Not implemented | DE-012-02 |
| Newline list separator | Currently whitespace | Not implemented | Lexer/parser follow-up |
| Comments/globbing/substitution | Not implemented | Not implemented | Out of scope |

DE-012-02 (#58) implements variables, assignment, quoting alignment, and expansion. DE-012-03 (#79) implements redirection and pipeline execution with runtime-owned descriptor/process cleanup. Any implementation that discovers a mismatch must update this specification and add conformance tests in the same change; it must not silently invent different semantics.
