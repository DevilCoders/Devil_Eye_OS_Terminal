# Renderer Profiling and Optimization

**Task:** DE-020-02 (#72)  
**Target:** Linux x86_64  
**Implementation under study:** deterministic CPU `SoftwareRenderer` in `de-terminal-core`.

## Scope and correctness constraints

The software renderer is the current measurable rendering implementation. This task profiles it without selecting a GUI toolkit, font rasterizer, GPU API, or window-system backend. The renderer contract remains authoritative for immutable frame data, wide-grapheme occupancy, selection, clipping, cursor composition, bounded damage, and resize behavior.

A retained scrollback buffer is not implemented in the current terminal model. The visible-grid scroll operation is measured by DE-020-01, but it is not a substitute for large-scrollback profiling. Do not invent scrollback performance claims; add that workload when the bounded scrollback component exists.

## Workload matrix

The dependency-free `terminal_core` benchmark harness records:

| Workload | What it stresses |
| --- | --- |
| Full frame, 80×24 / 160×48 / 240×80 | Background fill, glyph lookup/raster loop, styles, and pixel writes |
| One dirty row at each size | Incremental redraw and region handling |
| Color/style churn at each size | Indexed and RGB colors; bold, dim, italic, inverse, underline, blink, hidden, and strikethrough |
| Linear selection at each size | Per-cell selection membership and color override |
| Cursor movement, 80×24 | Two-cell invalidation and cursor overlay on an ASCII-only screen, avoiding wide-cell cursor-span ambiguity |
| Resize round trip, 80×24 ↔ 160×48 | Surface allocation, checked dimensions, and background initialization |
| Allocation probes for full and dirty frames | Allocation count and requested bytes during one warmed render call |

The styled workload includes single-width and wide Unicode graphemes. The atlas fallback is deterministic and avoids font-discovery variability.

## Allocation measurement

The benchmark executable installs a counting wrapper around the system allocator. It reports allocation calls and requested bytes during a single render call after setup. The counter is isolated to the benchmark executable and adds no third-party dependency.

These values are allocation traffic for the measured call, not live bytes, peak resident memory, or allocator fragmentation. The benchmark's timed throughput measurements are separate from the allocation probes. Allocator instrumentation is not enabled in the production library.

Expected optimization target: steady-state render calls should not allocate when the frame, renderer, and damage tracker are already constructed. A full-frame or dirty-frame render allocation is treated as a hot-path defect unless a documented correctness requirement makes it unavoidable.

## Profiling procedure

Use the pinned Rust toolchain and identical runner class, build profile, dimensions, glyph atlas, and sample settings for before/after comparisons:

```sh
DE_BENCH_WARMUP_MS=500 DE_BENCH_SAMPLE_MS=200 DE_BENCH_SAMPLES=12 \
  cargo bench -p de-terminal-core --bench terminal_core
```

On a Linux host where hardware performance counters are available, collect sampled CPU evidence separately:

```sh
perf stat -r 3 -e cycles,instructions,branches,branch-misses,cache-misses \
  cargo bench -p de-terminal-core --bench terminal_core
perf record -g -- cargo bench -p de-terminal-core --bench terminal_core
perf report --stdio
```

If the host denies access to performance counters, record that limitation; do not substitute synthetic CPU estimates. Compare only measurements from equivalent hosts. Hosted CI runs may use different physical CPU models even with the same `ubuntu-24.04` runner label.

## Optimization protocol

1. Capture the workload matrix and allocation probes on the unmodified renderer.
2. Use the allocation probes and, where available, hardware-counter samples to identify hot paths.
3. Change one hot path at a time. Preserve the existing public renderer contract.
4. Run the complete software-renderer test suite, including golden RGBA, clipping, selection, wide graphemes, cursor, styles, and resize limits.
5. Capture the same workload matrix after the change. Report median and p95 of sample means, allocation calls/bytes, and host metadata.
6. Reject optimizations that change golden output, break wide-cell redraw semantics, or produce an improvement smaller than observed runner variance.

## Evidence

The DE-020-01 benchmark protocol and initial whole-renderer baselines are in [performance-benchmarks.md](performance-benchmarks.md). This section will be updated with same-runner before/after evidence for DE-020-02; different CPU models must remain separate observations.

## Baseline — 2026-10-31, before first optimization

The first DE-020-02 workload capture completed successfully in [workflow run 41250795328](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/actions/runs/41250795328), source commit `7ed572ea067e74471375766fae70ab381bfa7653`.

**Environment:** GitHub-hosted `ubuntu-24.04`, Linux kernel `7.0.0-1017-azure`, 4 vCPU, AMD EPYC 7763 64-Core Processor, Rust/Cargo 1.99.1, release benchmark profile.

| Workload | Median sample mean | p95 of sample means |
| --- | ---: | ---: |
| Full frame, 80×24 | 2.642 ms | 2.708 ms |
| Dirty row, 80×24 | 109.87 µs | 110.26 µs |
| Color/style churn, 80×24 | 2.591 ms | 2.593 ms |
| Linear selection, 80×24 | 2.603 ms | 2.646 ms |
| Cursor movement, 80×24 | 4.175 µs | 4.247 µs |
| Resize 80×24 ↔ 160×48 | 1.250 ms | 1.283 ms |
| Full frame, 160×48 | 10.556 ms | 10.578 ms |
| Dirty row, 160×48 | 219.59 µs | 243.25 µs |
| Color/style churn, 160×48 | 10.360 ms | 10.459 ms |
| Linear selection, 160×48 | 10.391 ms | 10.450 ms |
| Full frame, 240×80 | 26.418 ms | 26.685 ms |
| Dirty row, 240×80 | 327.96 µs | 328.91 µs |
| Color/style churn, 240×80 | 25.994 ms | 28.014 ms |
| Linear selection, 240×80 | 26.054 ms | 26.460 ms |

### Allocation finding

The warmed render call allocated exactly once for both full-frame and dirty-row rendering at every tested size: **1 allocation / 8 requested bytes per call**. The renderer built an owned temporary `Vec<CellRect>` with either `vec![full_region]` or `damage.rectangles().to_vec()`, even though both paths can iterate one stack value or a borrowed slice. This is a measured hot-path allocation, not an inference from source inspection.

The first optimization removes that temporary vector without changing damage semantics or pixel composition. Expected result: zero allocations in warmed render calls. Throughput must be measured separately; allocation reduction alone is not a claim of frame-time improvement.

The benchmark does not measure retained scrollback because that component does not yet exist. CPU samples are per-sample means; p95 is across the 12 sample means, not individual-operation tail latency.

### After first optimization — 2026-10-31

The first optimization was measured in [workflow run 41251178845](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/actions/runs/41251178845), source commit `93f435421e4e6b397a88c1c2545c30cf77f22566`.

**Environment:** GitHub-hosted `ubuntu-24.04`, Linux kernel `7.0.0-1017-azure`, 4 vCPU, AMD EPYC 9V74 80-Core Processor, Rust/Cargo 1.99.1, release benchmark profile.

The allocation probes reported **0 allocation calls / 0 requested bytes** for warmed full-frame and dirty-row renders at all three dimensions. This verifies the targeted hot-path allocation was eliminated.

| Workload | Median sample mean | p95 of sample means |
| --- | ---: | ---: |
| Full frame, 80×24 | 2.140 ms | 2.159 ms |
| Dirty row, 80×24 | 89.16 µs | 89.35 µs |
| Color/style churn, 80×24 | 2.089 ms | 2.113 ms |
| Linear selection, 80×24 | 2.099 ms | 2.103 ms |
| Cursor movement, 80×24 | 3.688 µs | 3.710 µs |
| Resize 80×24 ↔ 160×48 | 1.067 ms | 1.068 ms |
| Full frame, 160×48 | 8.549 ms | 8.589 ms |
| Dirty row, 160×48 | 178.10 µs | 178.23 µs |
| Color/style churn, 160×48 | 8.342 ms | 8.378 ms |
| Linear selection, 160×48 | 8.390 ms | 8.397 ms |
| Full frame, 240×80 | 21.500 ms | 21.742 ms |
| Dirty row, 240×80 | 267.84 µs | 275.77 µs |
| Color/style churn, 240×80 | 21.004 ms | 21.197 ms |
| Linear selection, 240×80 | 21.076 ms | 21.264 ms |

**Comparison limitation:** the pre-optimization baseline used an AMD EPYC 7763 runner, while this after-run used an AMD EPYC 9V74 runner. Both used the same `ubuntu-24.04` label, 4-vCPU allocation, kernel, and Rust toolchain, but the CPU model differs. Therefore the timing values are recorded as separate environment-specific observations and must not be presented as a verified frame-time speedup. The zero-allocation result is directly comparable because it is a per-call count, not a throughput comparison.

