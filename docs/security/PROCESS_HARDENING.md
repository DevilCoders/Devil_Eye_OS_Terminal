# Linux Process Hardening

**Status:** Linux x86_64 implementation under review in [PR #130](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/pull/130). This document describes the current branch implementation; it is not a claim that the controls are already shipped in master.

## 1. Scope

The Linux runtime boundary validates and resolves a process specification before allocating PTY/process resources. The executable, argument vector, environment overrides, and working directory remain distinct values. The runtime does not concatenate user input into a command string or invoke an implicit `sh -c`.

The implementation is Linux-specific and does not claim equivalent behavior for Windows or macOS.

## 2. Executable resolution

The Linux adapter resolves an executable before spawning:

- A non-empty program specification is validated, including NUL rejection in program, arguments, environment keys/values, and working directory.
- Explicit executable paths are resolved relative to the validated working directory when one is supplied, otherwise relative to the current directory.
- Bare executable names are searched using the explicit `PATH` override, or the parent `PATH` only when environment inheritance is enabled.
- Empty and relative `PATH` entries are ignored; the current working directory is never implicitly searched through `PATH`.
- Candidate paths are canonicalized, required to be regular files, and checked for effective execute permission.
- The resolved executable path is passed to `Command::new`; arguments remain a separate vector.

Resolution errors use stable, non-sensitive error categories. Raw argument and environment values are not included in the public error descriptions.

**Residual limitation:** canonicalization and permission checks occur before `Command::spawn`; they do not provide an fd-based `execveat` guarantee against every path replacement race in a hostile writable directory. Do not treat this implementation as a complete defense against filesystem TOCTOU attacks. A stricter descriptor-based execution policy requires a separate design and platform review.

## 3. Environment and working directory

The process specification explicitly selects environment inheritance:

- `Inherit` preserves the parent environment and applies explicit key/value overrides.
- `Clear` starts from an empty environment and applies only explicit overrides.
- Duplicate override keys are replaced in the specification rather than ambiguously appended.
- Environment keys must be non-empty, contain no NUL, and contain no `=`; values must contain no NUL.
- The runtime does not mutate the parent process environment.
- A requested working directory is canonicalized and must resolve to an existing directory before process resources are created.

The default environment policy remains part of the caller's process specification. Security-sensitive launch sites should choose `Clear` and supply a minimal allowlisted environment rather than inheriting ambient variables.

## 4. PTY and descriptor ownership

For PTY-backed launches:

- The PTY master and slave descriptors have explicit Rust ownership through `OwnedFd`.
- Child standard streams receive independently duplicated slave descriptors.
- Descriptor duplication uses close-on-exec behavior for descriptors not intended to cross an exec boundary.
- The parent drops its slave copy after process creation so EOF/hangup behavior is not accidentally held open by the parent.
- The child creates a new session, acquires the PTY slave as its controlling terminal, and sets its foreground process group in the pre-exec setup.
- The runtime handle owns the master descriptor and child-process handle; callers receive only the runtime abstraction.

The pre-exec closure is restricted to the required low-level session/terminal setup operations. Failure to establish the controlling terminal aborts process creation rather than publishing a partially initialized handle.

## 5. Process groups and cleanup

Each PTY child becomes a session and process-group leader. Signals are sent to the process group owned by that runtime handle, not to an arbitrary PID supplied by the caller.

- Graceful termination sends the configured termination signal to the owned process group.
- Forced termination targets the same group.
- A missing process group during termination is treated as already terminated.
- Exit status is cached after observation.
- With `kill_on_drop=true`, dropping the handle kills the owned process group and waits/reaps the direct child.
- With `kill_on_drop=false`, the direct child is transferred to a background reaper so the application does not leave a zombie.

The process/session manager is responsible for bounded graceful-to-forced shutdown and for keeping blocking waits off the GUI/event-loop thread.

## 6. Diagnostics and secret handling

Stable runtime errors describe categories such as invalid program, executable not found, non-executable path, spawn failure, I/O failure, signal failure, wait failure, and resource-limit violation.

Do not log:
- Full inherited or overridden environment blocks.
- Passwords, tokens, credential-bearing command arguments, or raw paste/input payloads.
- Unredacted process launch specifications.

Low-level operating-system diagnostics may be retained only through a separately reviewed redaction policy. The public error surface must not interpolate secret values.

## 7. Verification requirements

The implementation must retain tests for:

- Empty/NUL executable, argument, environment key/value, and working-directory values.
- Missing executable, non-regular executable, non-executable file, missing cwd, and non-directory cwd.
- Explicit `PATH` search and rejection of empty/relative path entries.
- Clear versus inherited environment behavior.
- Argument preservation without command-string construction.
- PTY descriptor ownership, controlling terminal setup, and resize.
- Process-group signal delivery and kill-on-drop cleanup.
- Child exit caching and detached-child reaping.
- Spawn failure without leaked PTY descriptors or partially registered processes.

## 8. Residual risks and release gates

The following remain separate release/security work:

- Descriptor-based executable resolution or an explicit documented TOCTOU threat acceptance.
- Symlink and writable-directory policy for configuration and executable paths.
- Per-session resource budgets and adversarial multi-session tests.
- End-to-end secret-redaction tests across all diagnostic call sites.
- Cross-platform equivalence for non-Linux adapters.

The process-hardening issue (#64) remains in progress until the implementation is reviewed, all required CI/security tests pass, and residual risks have explicit owners and disposition.
