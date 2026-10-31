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
| Cursor movement, 80×24 | Two-cell invalidation and cursor overlay |
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
