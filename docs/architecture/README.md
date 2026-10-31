# Architecture Documentation

This directory contains the system architecture baseline and architecture decision records.

## System architecture

- [System Architecture](system-architecture.md) — component boundaries, process/session model, ownership, lifecycle, data flows, concurrency, error propagation, Linux-first platform strategy, and security invariants.
- [Cross-Component API Contracts](api-contracts.md) — lifecycle state machine, runtime/PTY I/O, terminal and renderer boundaries, FFI ownership/threading, error mapping, versioning, and contract tests.
- [DE-Shell Library Lifecycle](de-shell-lifecycle.md) — opaque C handle ownership, create/destroy semantics, status codes, and lifecycle tests.
- [DE-Shell Input Buffer](de-shell-input-buffer.md) — bounded byte storage, explicit line submission, EOF flushing, capacity behavior, and ownership.
- [DE-Shell Lexer](de-shell-lexer.md) — token types, source spans, quote/escape grouping, supported operators, and deterministic diagnostics.\n- [DE-Shell Alpha Parser](de-shell-parser.md) — alpha grammar, AST ownership, operator precedence, source-positioned diagnostics, and bounded nesting.
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
