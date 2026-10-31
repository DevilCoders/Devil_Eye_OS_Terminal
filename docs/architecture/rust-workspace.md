# Rust Workspace

The workspace is deliberately split by responsibility. The dependency graph is
acyclic and points inward toward platform-independent contracts.

## Crates

| Crate | Type | Responsibility | Explicit non-goals at this stage |
| --- | --- | --- | --- |
| de-app | library | Top-level application configuration, service registration, deterministic startup, rollback, and shutdown coordination | No GUI, event loop, renderer, or terminal session implementation yet |
| de-terminal-core | library | Platform-independent terminal model primitives and invariants | No PTY, process spawning, VT parser, or renderer |
| de-runtime | library | Owned process launch specifications and runtime-facing contracts | No process execution, PTY allocation, or signal management yet |
| de-ffi | library | Versioned C-compatible ABI data types | No C implementation or exported unsafe functions yet |

## Dependency direction

~~~text
de-app
  ├── de-terminal-core
  ├── de-runtime
  └── de-ffi

de-terminal-core   (no internal dependencies)
de-runtime         (no internal dependencies)
de-ffi             (no internal dependencies)
~~~

Subsystem crates must not depend on de-app. This prevents cycles and keeps
the terminal model, runtime contracts, and ABI definitions independently
testable.

## Dependency policy

- The initial workspace uses only the Rust standard library.
- New third-party dependencies require a concrete need, security review, and
  an explicit decision recorded in the relevant change.
- Keep Rust/C ABI declarations in de-ffi; do not leak C implementation
  details into the terminal model.
- Keep the terminal emulator independent from DE-Shell and able to host
  compatible external shells.
- Add implementation only when it provides tested behavior. A crate boundary
  is not evidence that a later subsystem is already implemented.

## Validation

Run from the repository root:

~~~sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
~~~
