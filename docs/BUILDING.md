# Building Devil_Eye OS Terminal

## Current build boundaries

The repository has two independent build graphs:

- **Rust workspace:** Cargo builds `de-app`, `de-terminal-core`, `de-runtime`, and `de-ffi`.
- **C subsystem:** CMake builds the DE-Shell static library and its CTest suite.

The C library exposes build metadata and the initial opaque DE-Shell lifecycle API. Input buffering, lexer/parser, command dispatch, execution, and Rust/C FFI remain separate tasks.

## Prerequisites

Initial supported development platform: Linux x86_64.

- Rust stable toolchain with Cargo, rustfmt, and Clippy.
- CMake 3.25 or newer.
- A C17 compiler (GCC or Clang).
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

The private representation lives under `c/internal/` and is not part of the public API. The lifecycle tests cover invalid arguments, independent instances, successful cleanup, and idempotent destruction. No parser, input buffer, process execution, or terminal behavior is implied by this lifecycle foundation. See [DE-Shell Library Lifecycle](architecture/de-shell-lifecycle.md).

## Rust/C integration strategy

Cargo and CMake remain separate build entry points for now. Do not add a `build.rs`, invoke CMake from Cargo, or expose C implementation details through Rust until the cross-component contract in DE-002-02 (#30) and the versioned FFI work in DE-008 are reviewed. This keeps native build failures attributable and avoids prematurely coupling the Rust workspace to an unstable C ABI.

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
