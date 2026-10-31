# Parser Fuzzing

## Targets

The isolated `fuzz/` package contains two coverage-guided libFuzzer targets:

- `shell_lexer_parser`: sends arbitrary bounded byte strings through the safe Rust DE-Shell wrapper into the C lexer/parser/execution boundary. No external command executor is installed, so fuzz input cannot spawn host processes.
- `vt_parser`: feeds arbitrary bytes into the incremental VT/ANSI parser using input chunk boundaries derived from the input itself, exercising parser state across fragmented reads.

Both targets use libFuzzer and are built with AddressSanitizer. The C shell implementation is compiled with sanitizer instrumentation through `CFLAGS` in the fuzz workflow.

## Prerequisites

- Linux x86_64.
- Rust nightly (the normal application toolchain remains pinned separately).
- A working C compiler and C++ compiler.
- `cargo-fuzz`.

Install the fuzz runner:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
```

## Run locally

From the repository root:

```sh
CFLAGS="-fsanitize=address -fno-omit-frame-pointer" \
  cargo +nightly fuzz run --sanitizer address shell_lexer_parser -- -max_len=65536 -timeout=3

CFLAGS="-fsanitize=address -fno-omit-frame-pointer" \
  cargo +nightly fuzz run --sanitizer address vt_parser -- -max_len=65536 -timeout=3
```

To bound a local run, add `-max_total_time=SECONDS` after the `--` separator. The scheduled CI runs each target for three minutes.

## Corpus management

Seed corpora are versioned under:

- `fuzz/corpus/shell_lexer_parser/`
- `fuzz/corpus/vt_parser/`

When a crash or sanitizer finding is discovered, preserve the exact reproducer under the relevant corpus directory with a descriptive filename. Do not discard a crash input after fixing the bug; it becomes a permanent regression seed.

To reproduce a saved crash or corpus input:

```sh
cargo +nightly fuzz run shell_lexer_parser fuzz/corpus/shell_lexer_parser/<seed>
cargo +nightly fuzz run vt_parser fuzz/corpus/vt_parser/<seed>
```

For a generated artifact, use the exact path uploaded by the failing GitHub Actions run:

```sh
cargo +nightly fuzz run <target> <artifact-path>
```

## CI behavior

`.github/workflows/fuzz.yml` runs:

- On pull requests and pushes that modify fuzz targets, lexer/parser code, FFI, or the VT parser.
- On a weekly schedule (Monday 03:00 UTC).
- On manual workflow dispatch.

Each target runs independently with AddressSanitizer, a 65,536-byte maximum input, a three-second per-input timeout, and a three-minute run budget. Crash artifacts are uploaded for 30 days even when a fuzz job fails.

A green fuzz job means the configured time budget completed without a reported crash; it is not a proof of parser correctness or exhaustive input coverage. New crashes must be triaged, fixed, and retained as regression seeds.
