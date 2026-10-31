# Architecture Documentation

This directory contains the system architecture baseline and architecture decision records.

## System architecture

- [System Architecture](system-architecture.md) — component boundaries, process/session model, ownership, lifecycle, data flows, concurrency, error propagation, Linux-first platform strategy, and security invariants.
- [Terminal Screen State](terminal-screen-state.md) — cursor bounds, inclusive scroll margins, CR/LF, erase ranges, wide-cell invariants, and resize reconciliation.
- [Cross-Component API Contracts](api-contracts.md) — lifecycle state machine, runtime/PTY I/O, terminal and renderer boundaries, FFI ownership/threading, error mapping, versioning, and contract tests.
- [DE-Shell Library Lifecycle](de-shell-lifecycle.md) — opaque C handle ownership, create/destroy semantics, status codes, and lifecycle tests.
- [DE-Shell Input Buffer](de-shell-input-buffer.md) — bounded byte storage, explicit line submission, EOF flushing, capacity behavior, and ownership.
- [DE-Shell Lexer](de-shell-lexer.md) — token types, source spans, quote/escape grouping, supported operators, and deterministic diagnostics.
- [DE-Shell Alpha Parser](de-shell-parser.md) — alpha grammar, AST ownership, operator precedence, source-positioned diagnostics, and bounded nesting.
- [DE-Shell Command Registry](de-shell-command-registry.md) — per-shell command metadata, registration/collision policy, lookup, built-in dispatch, and ownership.
- [DE-Shell Execution Engine](de-shell-execution.md) — bounded AST evaluation, quote removal, explicit external-executor callback, environment/working-directory forwarding, exit propagation, and unsupported alpha nodes.
- [DE-Runtime Process Interface](runtime-interface.md) — process launch configuration, stdio/PTY attachment, signals, exit status, capabilities, and ownership.
- [Linux Process Runtime Adapter](linux-runtime.md) — concrete Linux stdio/PTY process spawning, non-blocking I/O, process groups, signals, EOF semantics, and child reaping.
- [Linux PTY Backend](linux-pty.md) — master/slave allocation, initial termios and dimensions, descriptor ownership, CLOEXEC duplication, resize, and cleanup.
- [Linux PTY Child Process Lifecycle](linux-pty-process.md) — session/controlling-terminal setup, process groups, signal delivery, exit-status caching, kill-on-drop, and reaping.
- [Application Lifecycle](application-lifecycle.md) — implemented service initialization, rollback, reverse-order shutdown, retry semantics, and integration-test coverage.
- [Application Errors and Diagnostics](application-diagnostics.md) — stable error categories/codes, safe summaries, source-chain handling, secret redaction, bounded records, and log-injection protection.

## Required design boundaries

- Rust owns the desktop application, lifecycle, orchestration, GUI, and renderer integration.
- C owns the independent DE-Shell implementation.
- The terminal emulator must host external compatible shells; it is not coupled to DE-Shell.
- PTY/process sessions are isolated per terminal tab.
- OS-specific behavior is hidden behind platform adapters.
- Rust/C interfaces are explicit, versioned, and ownership-safe.

The system architecture document is the design baseline, not evidence that the described subsystems are implemented. Exact types, threading rules, ownership details, and ABI signatures must be frozen in the cross-component API contract (#30) before dependent implementation proceeds.

Architecture changes must record the decision, alternatives, compatibility impact, and test implications. See the [engineering roadmap](../engineering/ROADMAP.md) and [workflow](../engineering/WORKFLOW.md).
