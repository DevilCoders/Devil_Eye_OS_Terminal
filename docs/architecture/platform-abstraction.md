# Platform Abstraction and Capability Matrix

**Status:** Accepted Linux-first platform contract. Linux x86_64 is the only implemented target in this repository snapshot. Windows/ConPTY and macOS rows describe intended adapter mappings, not support claims.

## 1. Goals and boundaries

The platform layer translates platform-neutral runtime requests into operating-system operations. Platform-specific handles and APIs must not leak into:

- `de-terminal-core` (terminal byte-stream parsing and screen state);
- DE-Shell C language implementation;
- application/session contracts;
- public cross-component APIs.

The current Rust runtime boundary is `RuntimeService` / `RuntimeProcess`. A backend reports a stable `PlatformCapabilities` snapshot and returns `RuntimeError::UnsupportedCapability` when an operation is unavailable. Callers may use `PlatformCapabilities::require` for early validation, but each adapter must still validate every operation at the point of use.

Linux implementation lives under `de-runtime::linux`. Future adapters belong in target-gated modules such as `de-runtime::windows` and `de-runtime::macos`; they must implement the same semantic contract or report a precise capability gap.

## 2. Capability semantics

The existing `RuntimeCapability` set describes operations that a runtime backend can actually provide:

| Capability | Contract meaning |
| --- | --- |
| `StdioPipes` | Child stdin/stdout/stderr can be independently attached to pipes where requested. |
| `PseudoTerminal` | Backend can allocate and own a PTY-like terminal transport for a child. |
| `TerminalResize` | Active pseudo-terminal dimensions can be changed after spawn. |
| `Signals` | Backend can deliver the documented semantic signal/termination operations to the owned child/session. |
| `ProcessGroups` | Backend can isolate and address a child process group or equivalent containment unit. |
| `EnvironmentOverrides` | Spawn request environment overrides are applied without shell-string construction. |
| `WorkingDirectory` | Spawn request working directory is applied and validated. |
| `NonBlockingIo` | Runtime I/O methods provide non-blocking partial progress and explicit WouldBlock/EOF results. |

A capability is present only when the adapter implements and tests the behavior. An OS having a related system call is not sufficient. Conditional features must be probed and either reported as supported for that backend instance or rejected with a typed error.

`PlatformCapabilities::new` sorts and deduplicates values. `supports` is a query; `require` returns `RuntimeError::UnsupportedCapability` for absent features. The snapshot is descriptive, not a substitute for validating handles, request values, or current process state.

## 3. OS API mapping matrix

| Concern | Linux x86_64 — implemented | Windows — planned | macOS — planned |
| --- | --- | --- | --- |
| Process creation | Rust `std::process::Command`, Unix process setup hooks | `CreateProcessW` with explicit application/command-line quoting and handle inheritance | `posix_spawn` or carefully constrained process creation |
| PTY allocation | `openpty`/PTY master-slave facilities through the Linux adapter | ConPTY via `CreatePseudoConsole` and `HPCON` | `openpty` / platform PTY facilities |
| PTY resize | `ioctl(TIOCSWINSZ)` | `ResizePseudoConsole` | `ioctl(TIOCSWINSZ)` |
| Terminal settings | `termios` | ConPTY/console mode configuration; semantics differ from POSIX termios | `termios` |
| Non-blocking I/O | Non-blocking file descriptors and pollable descriptors | Overlapped I/O / completion mechanisms appropriate to ConPTY pipes | Non-blocking descriptors with `poll`/`kqueue` |
| Signals and termination | POSIX signals and process-group delivery | Console control events, process termination, and Job Objects; not a POSIX signal equivalent | POSIX signals and process groups |
| Process containment | `setpgid`, process-group signal delivery, child wait/reap | Job Objects and process handles | POSIX process groups and child wait/reap |
| Environment | Explicit environment inherited/overridden for child spawn | UTF-16 environment block and Windows environment rules | POSIX environment |
| Working directory | Native path passed to process creation | Windows wide path and drive/current-directory semantics | POSIX path passed to process creation |
| Filesystem | Unix file descriptors, permissions, atomic rename semantics | Windows handles, ACLs, sharing modes, replace/move semantics | POSIX file descriptors, permissions, atomic rename semantics |
| Terminal protocol | PTY byte stream; VT/ANSI parser remains platform-neutral | ConPTY transports virtual-terminal sequences; console-specific behavior must be explicit | PTY byte stream; VT/ANSI parser remains platform-neutral |

The table is an adapter design map. It does not mean the Windows or macOS backend exists. In particular, Windows process signaling and filesystem semantics must be represented explicitly rather than falsely mapped to POSIX behavior.

## 4. Platform-neutral contracts

### Process and PTY

Use `ProgramSpec`, `ProcessSpawnRequest`, `RuntimeService`, and `RuntimeProcess`. Executable, argument vector, environment overrides, and working directory remain separate values. No adapter may invoke an implicit command interpreter by concatenating user input.

A successful spawn transfers ownership of all native child/PTY handles to the returned runtime process handle. On failure, no child, descriptor, or partially registered handle may leak.

### I/O

- Reads and writes may be partial.
- `WouldBlock` is distinct from EOF and fatal I/O errors.
- Output is an ordered byte stream; chunk boundaries have no protocol meaning.
- Queues are bounded at the owning session layer.
- GUI/event-loop threads must not block on process I/O or child waits.

### Resize

Terminal dimensions are non-zero `TerminalSize` values. A backend reports resize failure explicitly; the session manager reconciles its cached size only after success.

### Signals and termination

The public API expresses semantic intent, not raw platform signal numbers. Each adapter maps supported intents to native operations. If an intent cannot be honored with the documented semantics, return a typed unsupported/operation error. Never signal a process not owned by the supplied runtime handle.

### Filesystem and environment

Environment values and working directories are validated before native resource acquisition. OS path encoding remains in the adapter. Filesystem persistence APIs, when introduced, must define atomic replace, permission, symlink, and cleanup semantics per platform; they must not expose raw native handles to the terminal core.

## 5. Adapter module layout

Target-gated implementation should follow this shape:

```text
crates/de-runtime/src/
  lib.rs                 # platform-neutral values and traits
  linux.rs               # Linux module root
  linux/
    process.rs
    pty.rs
    runtime.rs
  windows.rs             # future module root
  windows/
    process.rs
    conpty.rs
    runtime.rs
  macos.rs               # future module root
  macos/
    process.rs
    pty.rs
    runtime.rs
```

Only the adapter module may contain OS-specific FFI/system calls and native resource wrappers. The public trait surface uses Rust standard-library values and project-owned semantic enums.

## 6. Current capability profile

The Linux runtime currently advertises:

- StdioPipes
- PseudoTerminal
- TerminalResize
- Signals
- ProcessGroups
- EnvironmentOverrides
- WorkingDirectory
- NonBlockingIo

This profile is implemented by `LinuxRuntime::capabilities` and must stay synchronized with the actual methods and integration tests.

| Target | Adapter status | Capability reporting |
| --- | --- | --- |
| Linux x86_64 | Implemented | Runtime reports the capabilities above; real PTY/process tests are required. |
| Windows x86_64 | Not implemented | No capabilities may be reported until a ConPTY/process adapter and native tests exist. |
| macOS arm64/x86_64 | Not implemented | No capabilities may be reported until a PTY/process adapter and native tests exist. |

## 7. Verification requirements

The platform-neutral contract is tested in `de-runtime`:

- Capability lists are sorted and deduplicated.
- `supports` returns true only for advertised capabilities.
- `require` returns success for an advertised capability and `UnsupportedCapability` for an absent one.
- Fake runtime tests exercise the trait surface without claiming native platform support.

Each concrete adapter must additionally test:

- Exact capability profile against implemented operations.
- Spawn failure cleanup and no leaked native handles.
- Partial non-blocking reads/writes, WouldBlock, EOF, and cancellation.
- PTY resize and terminal settings.
- Child exit status, signal/termination mapping, and child reaping.
- Environment and working-directory validation.
- Process/session isolation and bounded shutdown.

Linux tests run on Linux CI and must exercise real OS behavior. Windows and macOS capability flags remain absent until native CI runs their integration suites.

## 8. Adding a new platform

1. Implement the platform-neutral traits without changing core crates to use native handles.
2. Add target-specific dependencies under Cargo target sections only.
3. Report only capabilities backed by implemented behavior.
4. Add unit tests for capability reporting and typed unsupported operations.
5. Add native integration tests for spawn, PTY, resize, I/O, signals, exit, and cleanup.
6. Update this matrix and the ADR if the semantic contract changes.
7. Add native CI before marking the target supported.

Do not claim cross-platform support from a successful cross-compile alone.
