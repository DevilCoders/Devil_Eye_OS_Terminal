# DE-Shell Alpha Parser

## Scope

DE-004-04 parses the lexer token stream into a caller-owned abstract syntax tree (AST). It defines structural grouping and operator precedence for the initial shell subset. It does not perform variable expansion, command lookup, process creation, execution, or full POSIX shell semantics.

## Alpha grammar

```text
list        := and_or ((';' | '&') and_or)* [';' | '&']
and_or      := pipeline (('&&' | '||') pipeline)*
pipeline    := command ('|' command)*
command     := simple_command | '(' list ')'
simple      := (WORD | redirection)+
redirection := ('<' | '>' | '>>') WORD
```

Precedence, from highest to lowest:

1. Words and redirections within a simple command.
2. Pipeline operator `|`.
3. AND/OR operators `&&` and `||` (same precedence, left-associative).
4. Sequence `;` and background `&` separators.

The background operator wraps only the immediately preceding list item. Parenthesized expressions form an explicit GROUP node. A trailing `;` or `&` is accepted; a trailing `|`, `&&`, or `||` is rejected.

Redirection-only simple commands are accepted by the AST layer. A redirection must always have a WORD target. The parser supports input, output, and append redirection tokens emitted by the lexer.

## AST representation and ownership

The parser performs no allocations. The caller supplies bounded arrays for:

- AST nodes.
- Word source references.
- Redirection records.

The returned AST references the original source by byte offset and length; it does not copy or dequote word contents. The caller must keep the source buffer alive while using the AST.

Simple-command nodes reference contiguous word and redirection ranges. Redirection records contain their own target source span. Composite nodes reference child node indices; GROUP and BACKGROUND are unary, while PIPELINE, AND, OR, and SEQUENCE are binary.

An empty token stream is a valid empty program with root `SIZE_MAX`. On any parse error, all AST counts are reset to zero and root is `SIZE_MAX`; partially written backing arrays must be ignored.

## Diagnostics and limits

Diagnostics contain a stable parser status and byte offset. The parser rejects malformed token streams, unexpected tokens, missing redirection targets, missing closing parentheses, and exhausted node/word/redirection capacities.

Parenthesis nesting is bounded to 128 levels to prevent unbounded recursive descent. Token spans are validated for range, ordering, and allowed flags before parsing begins.

## Deliberate non-goals

- No variable, command, arithmetic, or parameter expansion.
- No command execution or process spawning.
- No here-documents or file-descriptor-prefixed redirections.
- No shell-specific builtin dispatch.
- No full POSIX grammar or shell compatibility claim.

The complete language grammar and expansion semantics are finalized separately by DE-012-01 (#57). The alpha parser provides a small, explicit AST contract for the shell-core implementation tasks.
