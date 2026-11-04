# Cross-Platform CI Strategy

**Status:** Linux x86_64 is the only supported and required native CI target. Windows and macOS remain explicitly not enabled. This document describes the CI contract; it does not claim that future adapters exist.

The machine-readable target inventory is [`platform-ci-matrix.json`](platform-ci-matrix.json). The [Platform CI readiness workflow](../../.github/workflows/platform-ci-status.yml) validates that inventory and publishes a per-run status table. A green result from that reporting workflow validates policy only; it is not a build result for every listed operating system.

## 1. Required Linux x86_64 gate

| Layer | Runner/toolchain | Required evidence | Workflow |
| --- | --- | --- | --- |
| Rust static and correctness checks | `ubuntu-24.04`; pinned Rust `1.99.1`; `x86_64-unknown-linux-gnu` | `cargo fmt --all -- --check`, workspace tests, Clippy with warnings denied, Rust coverage artifact | Rust CI |
| C build and correctness checks | `ubuntu-24.04`; C17 GCC and Clang; Debug and Release | CMake configure/build, CTest, strict warnings-as-errors, C coverage artifact | C CI |
| Parser robustness | `ubuntu-24.04`; pinned nightly toolchain and AddressSanitizer | bounded `shell_lexer_parser` and `vt_parser` fuzz runs; crash artifacts retained | Parser Fuzzing |
| Performance regression evidence | `ubuntu-24.04`; pinned Rust toolchain | reproducible terminal-core, renderer-allocation, and Linux session/PTY measurements with environment metadata | Performance Benchmarks |
| Repository automation | `ubuntu-24.04`; Node.js 22 | dependency/taxonomy and platform-matrix policy tests | Repository automation tests |

The Rust and C workflows are the mandatory native build/test gates for the components they cover. Their path filters intentionally avoid running C builds for Rust-only changes or Rust builds for C-only changes. Changes that cross the FFI boundary must trigger both sets of checks. Fuzzing and performance workflows provide additional evidence; they do not replace correctness tests.

The Linux baseline must remain green before a change is considered mergeable. A platform-readiness report cannot substitute for the Rust CI or C CI result. No claim is made that GitHub branch protection currently enforces these checks; the matrix records the engineering merge policy.

## 2. Future target matrix

| Target | Rust target | Candidate native runner | C toolchain | Current CI state |
| --- | --- | --- | --- | --- |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `windows-2022` (verify image availability at activation) | Visual Studio 2022 MSVC and CMake, C17 | **NOT ENABLED** — no native job is scheduled |
| macOS arm64 | `aarch64-apple-darwin` | Apple Silicon macOS hosted runner; verify current label/image | Apple Clang and CMake, C17 | **NOT ENABLED** — no native job is scheduled |
| macOS x86_64 | `x86_64-apple-darwin` | Intel macOS hosted runner; verify that a supported image remains available | Apple Clang and CMake, C17 | **NOT ENABLED** — no native job is scheduled |

Runner candidates are not guarantees of current GitHub-hosted image availability. Revalidate runner labels, architecture, toolchain versions, and quotas immediately before enabling a target. Do not create a cross-compile-only job and present it as native platform support.

### Known implementation blockers

- `crates/de-ffi/build.rs` currently rejects every target except Linux x86_64 and invokes GCC/Clang-style compiler flags plus an `ar` archiver. Its native build path must become target-aware before Windows or macOS workspace builds can pass.
- `de-runtime` currently exposes a concrete Linux adapter only. Windows process/handle/Job Object/ConPTY semantics and macOS process/PTY/signal semantics are not implemented.
- The Linux CMake workflow uses GCC and Clang on Ubuntu. A Windows job must validate the MSVC generator/toolchain; a macOS job must validate Apple Clang on a native runner.
- There are no native Windows or macOS process/PTY integration suites. Compile-only or cross-compilation results cannot establish PTY, signal, handle, filesystem, or cleanup correctness.

## 3. Test tiers for each enabled platform

Every enabled platform must report these tiers independently; a skipped tier is not a pass:

1. **Static and compile:** pinned compiler/toolchain, formatting, lint, CMake/Cargo build, and target-specific dependency resolution.
2. **Unit:** platform-neutral contract tests plus adapter-specific value, capability, and error tests.
3. **Native integration:** process spawn, invalid executable, environment, working directory, partial I/O, EOF/WouldBlock, resize, termination, exit status, child reaping, and resource cleanup.
4. **PTY/terminal:** native PTY or ConPTY creation, dimensions, terminal modes, foreground process behavior, input/output, cancellation, and teardown.
5. **Fuzz and robustness:** parser fuzzing on the supported host; add platform-specific fuzz/integration coverage where native APIs or adapters introduce new parsing or ownership boundaries.
6. **Performance:** collect measurements on a documented native runner. Compare only equivalent architectures and runner classes; do not compare emulated/cross-compiled results to native baselines.
7. **Packaging/release:** install/uninstall and artifact-integrity checks once the release artifact pipeline is implemented. These are not currently claimed as completed CI tiers.

## 4. Enablement gate

A future platform may move from `not-enabled` to `required` only in the same reviewed change that:

1. Implements the target-specific runtime adapter behind the platform-neutral contracts.
2. Removes or replaces Linux-only FFI build assumptions and validates the C subsystem with the target's native compiler and archiver.
3. Adds native unit and integration tests for process/PTY behavior, environment/path semantics, termination, and cleanup.
4. Adds a workflow on a verified native runner with pinned toolchains, explicit test tiers, actionable logs, and retained failure artifacts.
5. Demonstrates a clean native run for the exact proposed commit and updates the architecture capability matrix plus this manifest.
6. Makes the new native workflow required for changes affecting that adapter before advertising the target as supported.

If a runner is unavailable, keep the target `not-enabled`. If a workflow intentionally does not execute a target, report `NOT ENABLED` with the reason; never label that target skipped-success, passed, or supported.

## 5. Skip and status semantics

- `required` means the target has native CI and its defined checks are expected to run.
- `not-enabled` means no native build/test job is scheduled and no support claim is made.
- A skipped job is not evidence. Before any target is enabled, the readiness workflow prints `NOT ENABLED — no native jobs scheduled` and lists the missing native evidence.
- The status workflow validates the manifest and emits the table on every pull request, on pushes to `master`, and on manual dispatch. Its own green check means only that the policy is internally consistent.
- The policy tests fail if Linux is no longer required, if a future target is promoted without changing the gate, if a runner/toolchain/blocker is omitted, or if a non-enabled target is declared as having required workflows.

## 6. Toolchain, cache, and artifact policy

- Pin the Rust toolchain in `rust-toolchain.toml`; workflow setup must match that pin. Nightly is limited to the fuzz workflow and must not silently replace the stable build toolchain.
- Pin or explicitly record native C compiler, CMake, runner image, architecture, and relevant system tool versions in build evidence.
- Use Cargo's normal incremental behavior first. Introduce cross-platform caches only after cache keys include OS, architecture, Rust toolchain, C compiler, and relevant lock/config inputs; a cache hit must never be treated as validation.
- Preserve test failure logs and fuzz crash artifacts. Performance artifacts must include source commit and runner metadata. Artifact retention must be explicit for release evidence and must not be inferred from a successful upload step.
- Keep Linux x86_64 as the release qualification baseline until a target has completed the full native enablement gate.
