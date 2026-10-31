# Performance Benchmarks and Initial Budgets

**Task:** DE-020-01 (#71)  
**Target:** Linux x86_64  
**Status:** Benchmark harness and reproducible CI capture are being established. The first successful CI artifact is the measured baseline; do not treat target budgets below as measured results.

## 1. Reproducible execution

The harness uses only the Rust standard library; it adds no runtime or third-party benchmark dependencies.

Run locally with the pinned toolchain:

```sh
cargo bench -p de-terminal-core --bench terminal_core
cargo bench -p de-app --bench session_io
```

Default sampling:

| Parameter | Default |
| --- | ---: |
| Warm-up per benchmark | 500 ms |
| Measurement duration per sample | 200 ms |
| Samples | 12 |
| Reported statistics | Median and p95 nanoseconds/op |
| Throughput | Work units/second |

The harness accepts bounded overrides through `DE_BENCH_WARMUP_MS`, `DE_BENCH_SAMPLE_MS`, and `DE_BENCH_SAMPLES`. The CI workflow fixes these to 500 ms, 200 ms, and 12 samples so runs use the same sampling protocol.

Each CSV contains raw sample rows and an aggregate row. Metadata records the operating system, architecture, CPU model, Rust compiler, and commit SHA. The workflow additionally records the runner image/kernel, CPU inventory, Cargo version, and uploads all results as a 90-day artifact named `performance-baseline-<commit>`.

Compare results only when the runner class, toolchain, build profile, workload, and sampling configuration match. CI-hosted results are useful for regression tracking but are not a substitute for measurements on the eventual release hardware.

## 2. Implemented workload coverage

| Concern | Measurement | Current interpretation |
| --- | --- | --- |
| Startup | Session-manager initialize/shutdown; PTY session spawn/close | Subsystem startup proxies; not full GUI cold-start time |
| Rendering | Software renderer full frames at 80×24, 160×48, 240×80; one dirty row | CPU reference renderer, with full and incremental redraw paths |
| Input latency | Parser processing of a short input-like byte sequence | Parser proxy only; no GUI keyboard/input mapper integration yet |
| PTY input latency | One-byte raw-mode PTY loopback round trip | Local transport latency, not end-user keyboard-to-photon latency |
| PTY throughput | 32 KiB raw-mode PTY loopback | End-to-end local PTY transport throughput |
| Memory | Screen cell storage and renderer RGBA surface sizes | Deterministic owned-buffer estimates; excludes allocator/runtime/GUI overhead |
| Scroll/scrollback | Screen-grid row-scroll operation | Measures current screen scrolling only; a retained scrollback buffer is not implemented yet |

The PTY loopback uses a fixed, non-interactive `/bin/sh -c 'stty raw -echo; exec /bin/cat'` child, a private PTY session, and bounded I/O queues. It verifies returned bytes match the submitted payload. The benchmark does not execute user-provided commands or capture environment secrets.

## 3. Initial engineering budgets

These are provisional targets for the first Linux x86_64 release profile. They become enforceable regression gates only after at least three successful, comparable baseline runs establish normal runner variance.

| Area | Workload | Provisional budget |
| --- | --- | ---: |
| Session manager startup | initialize + shutdown | p95 ≤ 1 ms |
| PTY session startup | spawn + close `/usr/bin/true` | p95 ≤ 100 ms |
| Software rendering | 80×24 full frame | p95 ≤ 16.67 ms |
| Software rendering | 160×48 full frame | p95 ≤ 16.67 ms |
| Software rendering | 240×80 full frame | p95 ≤ 33.33 ms |
| Incremental rendering | one dirty row at 80×24 | p95 ≤ 2 ms |
| Local PTY input latency | one-byte round trip | p95 ≤ 16 ms |
| Local PTY throughput | 32 KiB loopback | ≥ 8 MiB/s |
| Parser throughput | representative mixed VT stream | ≥ 10 MiB/s |
| Screen memory | 80×24 cell backing storage | ≤ 256 KiB |
| Renderer memory | 80×24 RGBA surface at 8×16 pixels/cell | ≤ 1 MiB |
| Renderer memory | 160×48 RGBA surface at 8×16 pixels/cell | ≤ 4 MiB |
| Renderer memory | 240×80 RGBA surface at 8×16 pixels/cell | ≤ 8 MiB |

The budgets are acceptance targets, not claims that the current implementation already meets them. A target miss must be reported with its measured value and environment; do not hide or normalize a regression.

## 4. Explicit measurement boundaries

- The desktop GUI startup path is not implemented, so the harness reports session-manager and PTY-session startup proxies instead of inventing an application cold-start number.
- A GUI input mapper and event loop are not integrated, so the short parser benchmark is not presented as keyboard-to-render latency.
- The current terminal model scrolls the visible screen but does not own a retained scrollback history. The row-scroll measurement is not a scrollback benchmark. DE-020-03 must add a bounded scrollback workload and memory budget when that component exists.
- The software renderer is the deterministic CPU reference backend. GPU, window-system, font discovery, and compositor latency are outside this baseline.
- Memory figures report explicit cell and pixel backing buffers only. They exclude allocator metadata, code/data segments, stacks, process RSS, GUI, fonts, and GPU allocations.

## 5. Regression protocol

1. Run the benchmark workflow on the candidate commit and retain its artifact.
2. Compare against at least three artifacts from the same runner class and pinned toolchain.
3. Use median and p95, not a single best sample. Investigate changes larger than 10% before attributing them to code.
4. Keep workload definitions and sampling settings unchanged when comparing commits. A workload change requires a new baseline series.
5. Do not optimize rendering until the profiling evidence identifies a hot path. DE-020-02 (#72) consumes this harness and adds workload-specific profiling and before/after evidence.
