# Contributing to Devil_Eye OS Terminal

This guide is the entry point for contributors working on the Linux x86_64 implementation. Read it together with the [system architecture](docs/architecture/system-architecture.md), [cross-component API contracts](docs/architecture/api-contracts.md), [engineering workflow](docs/engineering/WORKFLOW.md), and [pinned toolchain](docs/TOOLCHAIN.md).

## 1. Project boundaries

The codebase has explicit ownership boundaries:

- **Rust workspace:** desktop application, lifecycle, orchestration, terminal engine, runtime, renderer integration, and safe FFI wrapper.
- **C subsystem:** independent DE-Shell language implementation.
- **Linux runtime:** process creation, PTY ownership, signals, I/O, and child reaping.
- **Terminal engine:** byte-stream parsing, screen state, Unicode cells, cursor, modes, and scrollback.
- **GUI/renderer:** consumes terminal state; it does not own shell parsing or child-process handles.

The terminal emulator must host external compatible shells and programs. Do not couple VT/ANSI behavior to DE-Shell. The first supported development target is Linux x86_64; Windows/ConPTY and macOS are future adapters, not current support claims.

Before changing an interface, read the architecture and API contract. Update the contract first when behavior, ownership, threading, or error semantics change.

## 2. Prerequisites

Use the exact toolchain recorded in [TOOLCHAIN.md](docs/TOOLCHAIN.md):

- Ubuntu 24.04 or a compatible Linux x86_64 environment.
- Rust 1.99.1 with Cargo, rustfmt, Clippy, and llvm-tools-preview.
- GCC 13 or Clang 18 with C17 support.
- CMake 3.25+ and the system archiver (`ar`).
- Git.
- Optional for C coverage: `gcovr`.

Install the pinned Rust toolchain:

```sh
rustup toolchain install 1.99.1 --profile minimal \
  --component rustfmt --component clippy --component llvm-tools-preview
```

The repository's `rust-toolchain.toml` selects the project toolchain automatically. Do not substitute a moving `stable` channel.

## 3. Clean checkout and build

```sh
git clone https://github.com/DevilCoders/Devil_Eye_OS_Terminal.git
cd Devil_Eye_OS_Terminal
rustc --version
cargo --version
```

Build the Rust workspace from a clean checkout:

```sh
cargo build --workspace --all-targets
```

Build the native C subsystem independently:

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

The `de-ffi` Cargo build script also compiles and links the DE-Shell C sources for Rust callers. Keep the CMake and Cargo build paths synchronized when adding or removing C source files. Cargo does not invoke CMake.

The current repository is an implementation workspace; do not assume a production desktop executable or a release installer exists unless the corresponding application/packaging task has landed.

## 4. Required validation before a pull request

Run the same fast checks as CI:

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Run native C tests as well when C sources, headers, or CMake configuration change:

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

For repository automation changes:

```sh
node --test tests/automation/*.test.cjs
```

For coverage reports, follow the Rust and C instructions in [BUILDING.md](docs/BUILDING.md). Coverage is currently informational; do not invent or claim a percentage gate that is not configured.

A passing check on an earlier commit does not validate a later push. Verify the latest PR head SHA and its corresponding workflow run.

## 5. Workspace map

| Path | Responsibility |
| --- | --- |
| `crates/de-app` | Application composition, lifecycle, orchestration, configuration |
| `crates/de-terminal-core` | Platform-independent terminal state, VT/ANSI parsing, rendering contracts |
| `crates/de-runtime` | Process and platform runtime contracts/adapters |
| `crates/de-ffi` | Rust/C ABI declarations, native build integration, safe wrapper |
| `c/` | DE-Shell C implementation and internal modules |
| `include/de_shell/` | Public C headers; private representation stays under `c/internal/` |
| `docs/architecture/` | Normative architecture, subsystem contracts, implementation records |
| `docs/engineering/` | Roadmap, task catalog, dependency map, workflow and milestones |
| `.github/workflows/` | CI and repository engineering automation |
| `tests/automation/` | Dependency/taxonomy automation tests |

Use the crate that owns the behavior. Avoid introducing cross-layer shortcuts or cyclic crate dependencies.

## 6. Debugging and failure diagnosis

### Rust

- Reproduce the failing test by package and test name, for example: `cargo test -p de-terminal-core <test_name> -- --nocapture`.
- Use `RUST_BACKTRACE=1` when a test or process panics.
- Use `cargo test --workspace --all-targets` after a focused fix to catch cross-crate regressions.
- Treat Clippy warnings as errors; fix the underlying design or document a narrowly justified exception.

### C

- Configure a Debug build and retain symbols: `cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug`.
- Run one CTest case with `ctest --test-dir build -R <test_name> --output-on-failure`.
- Use `gdb --args build/<test-executable>` for native debugging when needed.
- Keep sanitizers and compiler diagnostics enabled in local investigations; never suppress a warning merely to make CI green.

### GitHub Actions

- Open the workflow run associated with the current PR head SHA.
- Inspect the failed job and exact step; do not treat cancelled or stale runs as validation.
- If a failure is environment-specific, reproduce it on the documented toolchain before changing CI.
- Record the failing command and the successful rerun in the PR.

### Runtime and PTY

- Prefer deterministic tests with a short-lived child process and explicit timeouts.
- Verify descriptor and child ownership on every error path.
- Do not debug by spawning uncontrolled background processes or leaving PTY children unreaped.

## 7. Implementation and testing expectations

Every implementation PR must:

- Address one Task Issue or a tightly related, explicitly scoped set of tasks.
- Preserve ownership and dependency direction from the architecture.
- Validate normal, boundary, invalid-input, failure, and cleanup behavior relevant to the change.
- Bound memory, queues, parser state, and process waits.
- Preserve byte ordering and report partial I/O rather than dropping data.
- Keep executable and arguments separate; never construct an implicit shell command from untrusted input.
- Keep Rust/C pointers, lengths, allocator ownership, ABI version, and threading rules explicit.
- Update public contracts and subsystem documentation with behavior changes.
- Include reproducible test evidence and known limitations.

Do not add fake implementations, placeholder capability flags, silently skipped tests, or claims of platform support without an actual adapter and tests.

## 8. Pull request workflow

1. Select a ready Task from the [roadmap](docs/engineering/ROADMAP.md) and confirm its dependencies are complete.
2. Create a focused branch named with the Task ID, for example `feat/DE-005-03-vt-parser`.
3. Implement the acceptance criteria and update relevant tests/docs.
4. Run the required local checks and inspect the final diff.
5. Open a PR with the Task ID in its title and link the issue using `Closes #N` only when the PR truly completes it.
6. Include scope boundaries, validation commands/results, security implications, and known limitations.
7. Resolve review findings; verify CI against the latest head SHA.
8. Update the Issue status only when the implementation is genuinely ready for review or done.

Do not merge, close, or retarget unrelated PRs as part of a contribution. Do not mark a task done solely because code has been pushed.

## 9. Commit and repository hygiene

- Keep commits focused and descriptive, with the Task ID where practical.
- Do not commit `target/`, `build/`, coverage output, local IDE state, generated binaries, secrets, or machine-specific configuration.
- Do not reformat unrelated files.
- Avoid adding dependencies without a clear need, license review, maintenance assessment, and security rationale.
- Keep generated lockfiles in sync when dependencies change.
- Never place credentials, tokens, private keys, or sensitive environment values in source, tests, logs, or issue comments.

## 10. Definition of Done

A task is complete only when its acceptance criteria are implemented, relevant tests pass on the latest code, formatting/lint/build checks pass, documentation is current, review findings are resolved, and evidence is attached to the Issue or PR. See the [engineering workflow](docs/engineering/WORKFLOW.md) for the normative Definition of Done.
