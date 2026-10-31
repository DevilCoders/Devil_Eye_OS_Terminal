# Rust Workspace Area

This directory is reserved for the Rust workspace members. The workspace manifest and crate boundaries are introduced by Task DE-001-02 (#26).

Planned ownership:
- Desktop application and lifecycle.
- Terminal state and rendering integration.
- Runtime and platform adapters.
- Rust/C FFI boundary.

Rules:
- Keep crate dependency direction explicit and acyclic.
- Do not place DE-Shell implementation here; it remains an independent C subsystem.
- Do not add placeholder crates. Each crate must have a defined responsibility and tests.
