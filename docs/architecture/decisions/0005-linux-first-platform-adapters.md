# ADR-0005: Linux x86_64 first with platform-neutral adapters

- Status: Accepted
- Date: 2026-10-31
- Decision owners: Architecture maintainers
- Related issues: #29, #35, #45, #48, #73

## Context

The initial implementation needs a concrete target so process, PTY, signal, and filesystem behavior can be implemented and tested without pretending that all operating systems expose identical capabilities.

## Decision

Linux x86_64 is the initial supported development and implementation target. Platform-specific system calls and handles remain behind platform-neutral runtime contracts and adapter modules. The core terminal engine and shell-language code must not depend on Linux-only types.

Windows/ConPTY and macOS are future targets. Their capability gaps must be explicit and discoverable; unsupported operations return typed capability/operation errors rather than silently degrading or using placeholder implementations.

The project does not claim Windows or macOS support until a concrete adapter and its integration tests exist.

## Alternatives considered

1. **Implement all operating systems simultaneously:** rejected because it multiplies platform-specific complexity before the Linux behavior is stable.
2. **Expose Linux file descriptors in core APIs:** rejected because it would prevent clean future adapters.
3. **Claim broad portability based only on Rust compilation:** rejected because PTY, process, signal, and GUI semantics require real platform integration tests.

## Consequences

Linux behavior can be implemented and validated first while preserving a migration path. Cross-platform interfaces must model capability differences rather than flattening them into misleading common denominators. Future targets require their own CI runners and integration tests.

## Affected interfaces and verification

- [System Architecture](../system-architecture.md)
- [Cross-Component API Contracts](../api-contracts.md)
- [DE-Runtime Process Interface](../runtime-interface.md)
- [Linux Process Runtime Adapter](../linux-runtime.md)
- [Linux PTY Backend](../linux-pty.md)

Verify capability reporting and unsupported-operation errors on each adapter. Linux integration tests must exercise real PTY/process behavior; future adapters require native platform CI before their capability is marked supported.
