# Building Devil_Eye OS Terminal

## Current build boundaries

The repository has two coordinated build paths:

- **Rust workspace:** Cargo builds `de-app`, `de-terminal-core`, `de-runtime`, and `de-ffi`. The `de-ffi` build script compiles and statically links the DE-Shell C sources for Rust callers.
- **C subsystem:** CMake independently builds the same DE-Shell sources and its CTest suite.

The C library exposes build metadata, the opaque DE-Shell lifecycle API, bounded input buffering, a quote-aware lexer, an alpha parser, a per-shell built-in command registry, and a controlled alpha execution engine. Rust now links this C library through the safe de-ffi wrapper; external process creation remains delegated to the host runtime callback.

## Prerequisites

Initial supported development platform: Linux x86_64.

- Rust stable toolchain with Cargo, rustfmt, and Clippy.
- CMake 3.25 or newer.
- A C17 compiler (GCC or Clang) and an archiver (`ar`, normally provided by binutils).
- Git.

Exact compiler/toolchain pinning and installation instructions are tracked by DE-001-04 (#28).

## Rust workspace

From the repository root:

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

## C subsystem

Configure, build, and run the C tests:

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

For an optimized build, use `-DCMAKE_BUILD_TYPE=Release`. The build requires C17 and enables strict warnings; GCC/Clang builds treat warnings as errors. Generated files stay under `build/` and must not be committed.

CI validates the CMake project with GCC and Clang in both Debug and Release configurations. CTest covers build metadata and DE-Shell instance lifecycle.

## DE-Shell lifecycle API

The public C header `include/de_shell/shell.h` exposes an opaque `de_shell_t` handle, stable status values, and create/destroy entry points. A caller owns a successfully created handle and releases it through `de_shell_destroy()`; the destroy function clears the caller's handle and supports repeated cleanup through that same handle variable.

The private representation lives under `c/internal/` and is not part of the public API. The lifecycle tests cover invalid arguments, independent instances, successful cleanup, and idempotent destruction. The internal bounded input-buffer suite covers line submission, maximum capacity, overflow atomicity, EOF, and invalid states. The lexer suite covers source spans, quoting, escapes, operators, malformed input, and capacity diagnostics. The parser suite covers AST structure, precedence, groups, redirections, malformed syntax, and storage limits. The command registry suite covers registration, lookup, duplicate-name collisions, argument validation, deterministic dispatch, registry growth, and the per-shell `exit` built-in. Expansion and external command execution remain separate tasks. See [DE-Shell Library Lifecycle](architecture/de-shell-lifecycle.md).

## Rust/C integration strategy

The `de-ffi` crate build script compiles the DE-Shell C sources into a static archive and links it into Rust. It uses the system C17 compiler selected by `CC` (default `cc`) and archiver selected by `AR` (default `ar`), with the same strict warning policy as CMake. The build script currently supports Linux x86_64 only and has no third-party Cargo build dependency.

CMake remains a separate build entry point for native C validation and CTest. Cargo does not invoke CMake. When adding or removing C sources, update both `CMakeLists.txt` and `crates/de-ffi/build.rs` in the same change. The safe wrapper and ownership/error contract are documented in [Safe Rust DE-Shell FFI Wrapper](architecture/ffi-rust-wrapper.md).

## Build system ownership

- Rust owns the desktop application and orchestration.
- C owns the independent DE-Shell implementation.
- NASM is reserved for measured, justified low-level routines; it is not a prerequisite.
- The terminal emulator remains independent from DE-Shell and must host compatible external shells.
- Linux x86_64 is the first target; OS-specific behavior stays behind platform adapters.

## Test coverage reports

Coverage is measured separately from the fast correctness/lint checks. Reports are informational while the implementation baseline is small; no percentage gate is imposed until the project has representative runtime, parser, PTY, and integration coverage.

### Rust coverage

Install the pinned tool and generate an LCOV report:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.9.1 --locked
mkdir -p target/coverage
cargo llvm-cov --workspace --lcov --output-path target/coverage/rust.lcov
```

### C coverage

Use GCC or Clang with the opt-in CMake instrumentation:

```sh
cmake -S . -B build-coverage -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON -DDE_ENABLE_COVERAGE=ON
cmake --build build-coverage --parallel
ctest --test-dir build-coverage --output-on-failure
gcovr --root . --object-directory build-coverage --xml-pretty --output build-coverage/coverage.xml --print-summary
```

CI uploads the Rust LCOV and C Cobertura XML reports as separate workflow artifacts. C coverage requires `gcovr`; on Ubuntu 24.04 install it with `sudo apt-get install gcovr`. Generated coverage data stays under ignored build/target directories and must not be committed.

## Repository automation tests

The dependency-status rules are pure JavaScript and use Node's built-in test runner. No npm dependencies are required:

```sh
node --test tests/automation/*.test.cjs
```
