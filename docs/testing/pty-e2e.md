# PTY-to-Terminal End-to-End Harness

## Purpose

The Linux integration test `crates/de-runtime/tests/pty_terminal_e2e.rs` validates the boundary between a real PTY-backed child process, the incremental VT parser, and the terminal screen model.

It is intentionally a headless harness. It does not require a desktop session, GUI toolkit, or GPU and is suitable for CI.

## Scenarios

1. **Output to terminal state:** a child emits SGR color and text through a PTY. The harness drains the PTY to EOF while polling child status, feeds the byte stream to `VtParser` in three-byte fragments, applies the resulting text/control events to `TerminalScreen`, and verifies cell contents and foreground attributes.
2. **Interactive input:** a raw-mode child waits on the PTY. The harness waits for a readiness marker, sends input through the PTY master, reads the echoed bytes, and verifies exact byte ordering.
3. **Resize:** a child queries its terminal dimensions after the parent applies `TIOCSWINSZ`; the observed rows and columns must match the requested dimensions.
4. **Signals and exit:** a semantic SIGTERM targets only the child session's process group and produces the expected signaled exit status.
5. **Drop cleanup:** dropping a live default `kill_on_drop` PTY handle terminates and reaps the child; a subsequent `kill -0` probe must fail.

Every polling loop has a finite deadline. The harness does not wait for a child before draining output where doing so could deadlock on a full PTY buffer.

## Run locally

From the repository root on Linux x86_64:

```sh
cargo test -p de-runtime --test pty_terminal_e2e -- --test-threads=1
```

The test uses real Linux PTYs and standard system utilities (`/bin/sh`, `/bin/cat`, `/bin/sleep`, `/bin/kill`, and `stty`). It is not expected to run on non-Linux platforms.

## Scope boundary

This harness validates the runtime/PTY/parser/screen integration boundary. The small event-to-screen adapter in the test is a test fixture, not the production terminal engine's final VT command consumer. The application session manager and renderer integration remain separate tasks.
