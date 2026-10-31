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
| Reported statistics | Median and p95 of per-sample mean nanoseconds/op |
| Throughput | Work units/second |

The harness accepts bounded overrides through `DE_BENCH_WARMUP_MS`, `DE_BENCH_SAMPLE_MS`, and `DE_BENCH_SAMPLES`. The CI workflow fixes these to 500 ms, 200 ms, and 12 samples so runs use the same sampling protocol.

Each CSV contains raw sample rows and an aggregate row. The p95 is calculated across the 12 sample means; it is not a per-operation latency percentile. For latency-sensitive follow-up work, use a histogram-capable harness when individual-operation tail latency is required. Metadata records the operating system, architecture, CPU model, Rust compiler, and commit SHA. The workflow additionally records the runner image/kernel, CPU inventory, Cargo version, and uploads all results as a 90-day artifact named `performance-baseline-<commit>`.

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

## 3. First measured baseline — 2026-10-31

The first complete benchmark run was captured on the pull-request merge checkout. The measured source branch head was `3558b6b4858ace5d9a09153f0f475f6a47ea4605`; GitHub's PR merge checkout was `5e67ee11ed82e790c05456f8fea4c6ac04ce1fe4`. The workflow run and its raw CSV/environment artifact are available at [run 41242497492](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/actions/runs/41242497492).

**Environment:** GitHub-hosted `ubuntu-24.04` (Ubuntu 24.04.5), Linux kernel `7.0.0-1017-azure`, 4 vCPU, AMD EPYC 9V74 80-Core Processor, Rust/Cargo 1.99.1 (release benchmark profile).

The p95 values below are the 95th percentile of 12 per-sample mean timings, not per-operation tail-latency percentiles.

| Workload | Median sample mean | p95 of sample means | Throughput |
| --- | ---: | ---: | ---: |
| VT parser mixed stream | 356.95 ns/op | 361.88 ns/op | 137.27 MB/s |
| Input-parser proxy | 92.55 ns/op | 93.38 ns/op | 237.70 MB/s |
| Screen update, 80×24 cells | 31.72 µs/op | 33.18 µs/op | 60.53 M cells/s |
| Screen row scroll, 80×24 | 1.392 µs/op | 1.394 µs/op | 1.379 B cells/s |
| Full render, 80×24 | 2.136 ms | 2.164 ms | — |
| Dirty-row render, 80×24 | 89.05 µs | 89.27 µs | — |
| Full render, 160×48 | 8.562 ms | 8.619 ms | — |
| Dirty-row render, 160×48 | 178.36 µs | 178.57 µs | — |
| Full render, 240×80 | 21.545 ms | 21.752 ms | — |
| Dirty-row render, 240×80 | 266.47 µs | 267.47 µs | — |
| Session-manager init + shutdown | 76.99 ns | 77.28 ns | — |
| PTY session spawn + close | 10.539 ms | 10.579 ms | — |
| One-byte PTY round trip | 29.84 µs | 30.58 µs | — |
| 32 KiB PTY round trip | 215.73 µs | 219.43 µs | 152.00 MB/s |

**Explicit backing-buffer memory:**

| Buffer | Measured bytes |
| --- | ---: |
| 80×24 screen cells (76 bytes/cell) | 145,920 |
| 80×24 RGBA renderer surface | 983,040 |
| 160×48 RGBA renderer surface | 3,932,160 |
| 240×80 RGBA renderer surface | 9,830,400 |

All measured workloads met the provisional budgets below in this run. This is one baseline on a shared hosted runner, not enough evidence to turn the budgets into hard regression gates. Collect at least three comparable runs before enforcing them. The raw artifact includes every sample, the aggregate rows, and runner/toolchain metadata.

## 4. Initial engineering budgets

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
| Renderer memory | 240×80 RGBA surface at 8×16 pixels/cell | ≤ 10 MiB |

The budgets are acceptance targets, not claims that the current implementation already meets them. A target miss must be reported with its measured value and environment; do not hide or normalize a regression.

## 5. Explicit measurement boundaries

- The desktop GUI startup path is not implemented, so the harness reports session-manager and PTY-session startup proxies instead of inventing an application cold-start number.
- A GUI input mapper and event loop are not integrated, so the short parser benchmark is not presented as keyboard-to-render latency.
- The current terminal model scrolls the visible screen but does not own a retained scrollback history. The row-scroll measurement is not a scrollback benchmark. A dedicated follow-up task must add a bounded scrollback workload and memory budget when that component exists.
- The software renderer is the deterministic CPU reference backend. GPU, window-system, font discovery, and compositor latency are outside this baseline.
- Memory figures report explicit cell and pixel backing buffers only. They exclude allocator metadata, code/data segments, stacks, process RSS, GUI, fonts, and GPU allocations.

## 6. Regression protocol

1. Run the benchmark workflow on the candidate commit and retain its artifact.
2. Compare against at least three artifacts from the same runner class and pinned toolchain.
3. Use the median and p95 of sample means, not a single best sample. These summary percentiles describe run-to-run sample variation, not the distribution of individual operations. Investigate changes larger than 10% before attributing them to code.
4. Keep workload definitions and sampling settings unchanged when comparing commits. A workload change requires a new baseline series.
5. Do not optimize rendering until the profiling evidence identifies a hot path. DE-020-02 (#72) consumes this harness and adds workload-specific profiling and before/after evidence.
