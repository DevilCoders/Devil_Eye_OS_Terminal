# ADR-0001: Rust application and C DE-Shell ownership

- Status: Accepted
- Date: 2026-10-31
- Decision owners: Architecture maintainers
- Related issues: #25, #26, #30, #49, #50

## Context

The application requires native desktop orchestration and a separate shell-language implementation. Combining shell parsing, terminal emulation, and application lifecycle in one subsystem would create unclear ownership, platform coupling, and a difficult-to-audit FFI boundary.

## Decision

- Rust owns the desktop application, lifecycle, service orchestration, terminal engine, process runtime interfaces/adapters, renderer integration, configuration, and safe FFI wrapper.
- C owns the independent DE-Shell language core, including shell input processing, lexing, parsing, command dispatch, and shell-language execution semantics.
- Rust/C communication uses a versioned C ABI with opaque handles, fixed-width values, explicit pointer/length validation, documented ownership, and stable status codes.
- Rust owns OS process creation and other host services. The C shell core requests external execution through a typed callback/host interface; it does not use `system()` or construct an implicit command string.
- NASM is optional and may be introduced only for measured, justified low-level routines with a documented portability strategy.

## Alternatives considered

1. **Rust-only shell implementation:** rejected because the project explicitly retains an independent C DE-Shell core.
2. **C owns the entire application:** rejected because GUI lifecycle, orchestration, safe concurrency, and platform adaptation belong in the Rust application layer.
3. **Unversioned direct FFI structs:** rejected because layout, allocator, threading, and error semantics would be ambiguous.
4. **C launches host commands directly:** rejected because it bypasses the runtime ownership boundary and complicates process isolation and testing.

## Consequences

The language boundary is explicit and testable, but requires ABI version negotiation, safe wrappers, and cross-language tests. C allocations remain C-owned; Rust allocations remain Rust-owned unless a specific ABI function documents a matching destroy operation. No panic or exception may cross the ABI.

## Affected interfaces and verification

- [System Architecture](../system-architecture.md)
- [Cross-Component API Contracts](../api-contracts.md)
- [Rust/C ABI Contract](../ffi-abi.md)
- [Safe Rust DE-Shell FFI Wrapper](../ffi-rust-wrapper.md)
- [DE-Shell Execution Engine](../de-shell-execution.md)

Verify with ABI mismatch, invalid pointer/length, stale handle, callback error, callback panic containment, and independent shell-instance tests.
