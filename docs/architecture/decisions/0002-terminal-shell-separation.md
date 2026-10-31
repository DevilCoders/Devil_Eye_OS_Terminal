# ADR-0002: Keep the terminal emulator independent from shell implementations

- Status: Accepted
- Date: 2026-10-31
- Decision owners: Architecture maintainers
- Related issues: #42, #44, #45, #47

## Context

A terminal emulator transports and interprets terminal byte streams. A shell interprets commands and manages shell-language state. Conflating them would prevent users from running standard external shells and programs and would make terminal behavior depend on one command language.

## Decision

The terminal engine is shell-agnostic. It consumes arbitrary ordered byte streams from the owning session, parses terminal control sequences, and updates terminal screen state. It does not parse shell grammar, expand shell variables, dispatch commands, or assume DE-Shell-specific messages.

DE-Shell and external programs use the same terminal-facing input/output semantics. External compatible shells such as Bash, Zsh, and Fish are launched as child processes through the runtime/PTY layer. DE-Shell is integrated as a separate backend through the versioned FFI/host-runtime boundary.

## Alternatives considered

1. **Terminal engine embeds DE-Shell:** rejected because it would make external shells second-class and couple VT parsing to a shell grammar.
2. **Terminal engine launches commands:** rejected because process ownership belongs to the runtime/session manager.
3. **Separate terminal implementations per shell:** rejected because terminal protocol behavior should be independent of shell choice.

## Consequences

The terminal engine can be tested with synthetic byte streams and reused by any compatible shell. The application must route each session's bytes to its own terminal state and preserve byte ordering. Shell-specific features remain in the shell backend and must not leak into the terminal parser.

## Affected interfaces and verification

- [Terminal Screen State](../terminal-screen-state.md)
- [VT/ANSI Parser](../terminal-vt-parser.md)
- [Cross-Component API Contracts](../api-contracts.md)
- [System Architecture](../system-architecture.md)

Verify with tests that feed identical VT/ANSI streams independently of shell identity and integration tests that run both an external shell and DE-Shell through the same terminal-facing contract.
