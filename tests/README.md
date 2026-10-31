# Test Organization

Tests are organized by the boundary they validate:

- Rust unit tests live beside the implementation in `crates/<crate>/src` under `#[cfg(test)]`.
- Rust integration tests live in `crates/<crate>/tests/` and exercise the crate's public API.
- C tests live in `c/tests/` and are registered with CTest.
- `fixtures/` stores deterministic input/output fixtures, terminal transcripts, and test data.
- Future `e2e/` tests exercise complete terminal workflows and process/session behavior.
- Future `fuzz/` targets exercise parsers, escape sequences, and input-state machines.

## Conventions

1. Name tests by observable behavior, not implementation details.
2. Keep tests deterministic; do not depend on network access, wall-clock timing, or ambient machine state.
3. Use temporary directories and unique per-test resources for filesystem/process tests.
4. Assert cleanup after both success and failure. Tests that spawn processes must reap them.
5. Exercise invalid input, partial initialization, error propagation, and retry paths where applicable.
6. Prefer real component implementations at the boundary under test. Use mocks only for external failures that cannot be made deterministic.
7. Keep fixtures small, reviewable, and free of credentials, personal data, or machine-specific absolute paths.
8. A passing test must fail when the behavior it protects is deliberately broken; avoid tests that only assert constants or implementation structure without an observable contract.

## Local commands

From the repository root:

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

Coverage reports are generated separately; see [Building](../docs/BUILDING.md).
