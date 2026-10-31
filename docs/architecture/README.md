# Architecture Documentation

This directory contains the system architecture baseline, component contracts, and implementation-specific design records.

## System architecture and cross-component contracts

- [System Architecture](system-architecture.md) — component boundaries, process/session model, ownership, lifecycle, data flows, concurrency, error propagation, Linux-first platform strategy, and security invariants.
- [Cross-Component API Contracts](api-contracts.md) — session lifecycle, runtime/PTY I/O, terminal and renderer boundaries, FFI ownership/threading, error mapping, versioning, and contract tests.
- [Renderer Contract](renderer-contract.md) — immutable frames, cell/pixel coordinate spaces, glyph metrics, clipping, bounded damage tracking, and resize semantics.
- [Keyboard Input Mapping](input-mapping.md) — key/text event separation, xterm key sequences, modifiers, IME handling, and configurable shortcuts.
- [Linux PTY I/O Contract](linux-pty-io.md) — descriptor ownership, nonblocking progress, backpressure, resize, EOF, cancellation, and stress-test evidence.

## Terminal model and parser

- [Terminal Screen State](terminal-screen-state.md) — cursor bounds, inclusive scroll margins, CR/LF, erase ranges, wide-cell invariants, and resize reconciliation.
- [VT/ANSI Parser](terminal-vt-parser.md) — incremental ESC/CSI/control-string framing, bounded sequence storage, cancellation, malformed-input recovery, and byte-preserving events.

## Rust/C ABI and DE-Shell

- [Rust/C ABI Contract](ffi-abi.md) — fixed-layout ABI versions/status values, compatibility rules, and ownership boundaries.
- [Safe Rust DE-Shell FFI Wrapper](ffi-rust-wrapper.md) — native C linkage, opaque handle lifecycle, safe execution API, callback panic containment, and cross-language tests.
- [DE-Shell Library Lifecycle](de-shell-lifecycle.md) — opaque C handle ownership, create/destroy semantics, status codes, and lifecycle tests.
- [DE-Shell Input Buffer](de-shell-input-buffer.md) — bounded byte storage, explicit line submission, EOF flushing, capacity behavior, and ownership.
- [DE-Shell Lexer](de-shell-lexer.md) — token types, source spans, quote/escape grouping, supported operators, and deterministic diagnostics.
- [DE-Shell Alpha Parser](de-shell-parser.md) — alpha grammar, AST ownership, operator precedence, source-positioned diagnostics, and bounded nesting.
- [DE-Shell Command Registry](de-shell-command-registry.md) — per-shell command metadata, registration/collision policy, lookup, built-in dispatch, and ownership.
- [DE-Shell Execution Engine](de-shell-execution.md) — bounded AST evaluation, quote removal, explicit external-executor callback, environment/working-directory forwarding, exit propagation, and unsupported alpha nodes.

## Runtime and application lifecycle

- [DE-Runtime Process Interface](runtime-interface.md) — process launch configuration, stdio/PTY attachment, signals, exit status, capabilities, and ownership.
- [Linux Process Runtime Adapter](linux-runtime.md) — concrete Linux stdio/PTY process spawning, nonblocking I/O, process groups, signals, EOF semantics, and child reaping.
- [Linux PTY Backend](linux-pty.md) — master/slave allocation, initial termios and dimensions, descriptor ownership, CLOEXEC duplication, resize, and cleanup.
- [Linux PTY Child Process Lifecycle](linux-pty-process.md) — session/controlling-terminal setup, process groups, signal delivery, exit-status caching, kill-on-drop, and reaping.
- [Application Lifecycle](application-lifecycle.md) — service initialization, rollback, reverse-order shutdown, retry semantics, and integration-test coverage.
- [Application Errors and Diagnostics](application-diagnostics.md) — stable error categories/codes, safe summaries, source-chain handling, secret redaction, bounded records, and log-injection protection.

## Required design boundaries

- Rust owns the desktop application, lifecycle, orchestration, GUI, and renderer integration.
- C owns the independent DE-Shell implementation.
- The terminal emulator must host external compatible shells; it is not coupled to DE-Shell.
- PTY/process sessions are isolated per terminal tab.
- OS-specific behavior is hidden behind platform adapters.
- Rust/C interfaces are explicit, versioned, and ownership-safe.

Architecture documents define contracts and design intent; they are not evidence that a component is implemented. Changes must record the decision, alternatives, compatibility impact, and test implications. See the [engineering roadmap](../engineering/ROADMAP.md) and [workflow](../engineering/WORKFLOW.md).
