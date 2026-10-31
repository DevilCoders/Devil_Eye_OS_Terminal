# Architecture Documentation

This directory contains the system architecture baseline, component contracts, and implementation-specific design records.

## System architecture and cross-component contracts

- [System Architecture](system-architecture.md) — component boundaries, process/session model, ownership, lifecycle, data flows, concurrency, error propagation, Linux-first platform strategy, and security invariants.
- [Architecture Decision Records](decisions/README.md) — accepted ownership, shell/terminal separation, session lifecycle, renderer-frame, and Linux-first platform decisions.
- [Cross-Component API Contracts](api-contracts.md)
- [Terminal Session Manager](session-manager.md) — session identity, PTY/process ownership, bounded I/O, exit polling, resize, worker-thread close, shutdown, and orphan prevention.
- [Workspace and Session Restoration Metadata](workspace-restoration.md) — versioned workspace/tab/pane layout, safe session metadata, strict validation, and the no-implicit-process-restoration boundary.
- [Renderer Contract](renderer-contract.md) — immutable frames, cell/pixel coordinate spaces, glyph metrics, clipping, bounded damage tracking, and resize semantics.
- [Performance Benchmarks and Initial Budgets](performance-benchmarks.md) — reproducible Linux x86_64 benchmarks, sampling protocol, resource budgets, and measurement boundaries.
- [Renderer Profiling and Optimization](renderer-profiling.md) — workload matrix, allocation probes, profiling procedure, and optimization evidence.
- [Keyboard Input Mapping](input-mapping.md) — key/text event separation, xterm key sequences, modifiers, IME handling, and configurable shortcuts.
- [Linux PTY I/O Contract](linux-pty-io.md) — descriptor ownership, nonblocking progress, backpressure, resize, EOF, cancellation, and stress-test evidence.

## Terminal model and parser

- [Terminal Screen State](terminal-screen-state.md) — cursor bounds, inclusive scroll margins, CR/LF, erase ranges, wide-cell invariants, and resize reconciliation.
- [Bounded Terminal Scrollback](terminal-scrollback.md) — retained full-screen scroll history, cell-budget limits, oldest-row eviction, resize/no-reflow behavior, and failure semantics.
- [VT/ANSI Parser](terminal-vt-parser.md) — incremental ESC/CSI/control-string framing, bounded sequence storage, cancellation, malformed-input recovery, and byte-preserving events.

## Rust/C ABI and DE-Shell

- [Rust/C ABI Contract](ffi-abi.md) — fixed-layout ABI versions/status values, compatibility rules, and ownership boundaries.
- [Safe Rust DE-Shell FFI Wrapper](ffi-rust-wrapper.md) — native C linkage, opaque handle lifecycle, safe execution API, callback panic containment, and cross-language tests.
- [DE-Shell Library Lifecycle](de-shell-lifecycle.md) — opaque C handle ownership, create/destroy semantics, status codes, and lifecycle tests.
- [DE-Shell Input Buffer](de-shell-input-buffer.md) — bounded byte storage, explicit line submission, EOF flushing, capacity behavior, and ownership.
- [DE-Shell Lexer](de-shell-lexer.md) — token types, source spans, quote/escape grouping, supported operators, and deterministic diagnostics.
- [DE-Shell Alpha Parser](de-shell-parser.md) — alpha grammar, AST ownership, operator precedence, source-positioned diagnostics, and bounded nesting.
- [DE-Shell Language Grammar and Semantics](de-shell-language.md) — normative grammar, quoting, expansion order, redirection, pipeline/list execution, diagnostics, and current implementation gaps.
- [DE-Shell Command Registry](de-shell-command-registry.md) — per-shell command metadata, registration/collision policy, lookup, built-in dispatch, and ownership.
- [DE-Shell Execution Engine](de-shell-execution.md) — bounded AST evaluation, quote removal, explicit external-executor callback, environment/working-directory forwarding, exit propagation, and unsupported alpha nodes.

## Runtime and application lifecycle

- [DE-Runtime Process Interface](runtime-interface.md) — process launch configuration, stdio/PTY attachment, signals, exit status, capabilities, and ownership.
- [Linux Process Runtime Adapter](linux-runtime.md) — concrete Linux stdio/PTY process spawning, nonblocking I/O, process groups, signals, EOF semantics, and child reaping.
- [Linux PTY Backend](linux-pty.md) — master/slave allocation, initial termios and dimensions, descriptor ownership, CLOEXEC duplication, resize, and cleanup.
- [Linux PTY Child Process Lifecycle](linux-pty-process.md) — session/controlling-terminal setup, process groups, signal delivery, exit-status caching, kill-on-drop, and reaping.
- [Application Lifecycle](application-lifecycle.md) — service initialization, rollback, reverse-order shutdown, retry semantics, and integration-test coverage.
- [Application Errors and Diagnostics](application-diagnostics.md) — stable error categories/codes, safe summaries, source-chain handling, secret redaction, bounded records, and log-injection protection.

## Platform abstraction

- [Platform Abstraction and Capability Matrix](platform-abstraction.md) — Linux x86_64 implementation profile, future Windows/ConPTY and macOS mappings, capability semantics, adapter layout, and native verification requirements.

## Required design boundaries

- Rust owns the desktop application, lifecycle, orchestration, GUI, and renderer integration.
- C owns the independent DE-Shell implementation.
- The terminal emulator must host external compatible shells; it is not coupled to DE-Shell.
- PTY/process sessions are isolated per terminal tab.
- OS-specific behavior is hidden behind platform adapters.
- Rust/C interfaces are explicit, versioned, and ownership-safe.

Architecture documents define contracts and design intent; they are not evidence that a component is implemented. Changes must record the decision, alternatives, compatibility impact, and test implications. See the [engineering roadmap](../engineering/ROADMAP.md) and [workflow](../engineering/WORKFLOW.md).

## Security

- [Security Threat Model](../security/THREAT_MODEL.md) — assets, trust boundaries, threat register, security requirements, and adversarial test strategy.
