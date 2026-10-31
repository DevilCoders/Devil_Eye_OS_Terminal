# Linux PTY Backend

## Scope

DE-006-01 provides a Linux x86_64 PTY master/slave resource. It allocates and configures descriptors but does not spawn a child process, establish a controlling terminal, manage process groups, or implement bidirectional I/O loops. Those behaviors belong to DE-006-02/03 and the Linux runtime adapter.

The implementation lives in `de_runtime::linux::pty::PtyPair` and is compiled only on Linux. It uses the pinned `libc` bindings for Linux/POSIX interfaces; the rest of `de-runtime` remains platform-neutral.

## Allocation sequence

1. Call `posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC)` to acquire the master.
2. Immediately wrap the returned descriptor in `OwnedFd`.
3. Call `grantpt` and `unlockpt`.
4. Resolve the slave path with the reentrant `ptsname_r`.
5. Open the slave with `O_RDWR | O_NOCTTY | O_CLOEXEC` and immediately wrap it in `OwnedFd`.
6. Configure canonical input, echo, signal processing, standard control characters, and newline handling on the slave.
7. Apply the requested non-zero window dimensions to the master with `TIOCSWINSZ`.
8. Return the pair only after every step succeeds.

Every intermediate descriptor is RAII-owned. Any error after master allocation drops the master; any error after slave allocation drops both descriptors. The API returns the original OS error through `io::Error`.

## Ownership and descriptor transfer

- `PtyPair` owns exactly one master and one slave descriptor.
- `master()` and `slave()` return borrowed descriptors.
- `try_clone_master()` and `try_clone_slave()` return independently owned duplicates created with `F_DUPFD_CLOEXEC`.
- `into_parts()` transfers ownership of both descriptors to the caller.
- All descriptors are close-on-exec. Process spawning must explicitly duplicate/attach the intended descriptors and must not leak the master or unrelated descriptors into child processes.

The slave device path is exposed for diagnostics only; callers should prefer the owned descriptors over reopening the path.

## Dimensions and termios

The PTY is initialized from the shared `TerminalSize` contract, which rejects zero columns or rows. `query_size()` reads the effective kernel window size. `resize()` updates the kernel first and updates the cached size only after success.

The initial slave termios mode is canonical with echo and signal processing enabled. Standard interrupt, quit, erase, kill, EOF, flow-control, suspend, and minimum/timeout control characters are initialized. The child shell may subsequently change termios through the slave descriptor. The PTY layer does not force raw mode after the child starts.

## Error and cleanup guarantees

- Failure to allocate, grant, unlock, resolve, open, configure termios, or set dimensions returns an OS error.
- No partially initialized pair is returned.
- `OwnedFd` closes each acquired descriptor exactly once on every error path and on `PtyPair` drop.
- Resize failure leaves the cached dimensions unchanged.
- Cloned descriptors have independent ownership and remain valid after the original pair is dropped.

## Tests

Linux unit tests verify PTY allocation, `isatty` on both ends, close-on-exec flags, initial dimensions, canonical/echo/signal termios flags, resize propagation, independent descriptor clones, and closure of both owned descriptors on drop.
