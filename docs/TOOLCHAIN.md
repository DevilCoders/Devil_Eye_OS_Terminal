# Developer Toolchain

This document records the reproducible Linux x86_64 development baseline. The Rust toolchain is pinned exactly; native C tooling is constrained by language and minimum-version requirements.

## Supported baseline

| Tool | Baseline | Policy |
| --- | --- | --- |
| OS | Ubuntu 24.04, Linux x86_64 | Initial supported development and CI platform |
| Rust | 1.99.1 | Exact version in `rust-toolchain.toml` and Rust CI |
| Rust components | rustfmt, Clippy, llvm-tools-preview | Installed with the pinned toolchain; LLVM tools support source coverage |
| C language | ISO C17 | Required by the CMake project |
| C compiler | GCC 13 or Clang 18 | Both compiler families are exercised in CI |
| CMake | 3.25 or newer | Minimum required by the C build |
| NASM | Not required | Do not install or add assembly sources unless profiling justifies a specific low-level routine |

The C compiler major versions describe the CI baseline. The CMake version is a minimum compatibility requirement, not an exact patch-level pin. Rust is the only compiler currently pinned to an exact release.

## Rust setup

Install rustup using the official installer, then from the repository root run:

```sh
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy --component llvm-tools-preview
rustup override set 1.99.1
rustc --version
cargo --version
```

The checked-in `rust-toolchain.toml` selects the same toolchain automatically when entering the repository. Do not replace it with `stable` in CI or local project configuration without an explicit toolchain update.

## Rust validation

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo install cargo-llvm-cov --version 0.9.1 --locked
cargo llvm-cov --workspace --lcov --output-path target/coverage/rust.lcov
```

## Native C prerequisites and validation

Install CMake 3.25+, either GCC 13 or Clang 18 with C17 support, and `ar` (normally from binutils). The C source is built through two independent entry points: the `de-ffi` Cargo build script compiles and links it for Rust callers, while CMake builds it for native C tests. Both use the system C17 compiler; Cargo selects it through `CC` and the archiver through `AR`.

After installing the native C toolchain:

```sh
cmake --version
cc --version
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

NASM is intentionally absent from the baseline. Assembly is not a default dependency and must be introduced only with a measured performance rationale and a portable fallback where required.

## Toolchain update procedure

1. Propose an exact Rust stable release and verify its official release announcement.
2. Update `rust-toolchain.toml`, the Rust CI action version, and this document in one change.
3. Run formatting, all workspace tests, and Clippy using the new compiler.
4. Record any edition, MSRV, lint, or generated-lockfile impact.
5. Merge only after CI passes on the pinned toolchain.

Do not silently track the moving stable channel: reproducible local builds and CI must use the same exact Rust release.
