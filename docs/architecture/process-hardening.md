# Linux Process, Environment, and Descriptor Hardening

**Status:** DE-016-02 implementation proposal under review. This document describes the controls added by the corresponding process-hardening change.

## 1. Executable resolution

The Linux runtime no longer delegates bare-name resolution to `std::process::Command`:

1. Validate the `ProgramSpec` structure before acquiring PTY or child-process resources.
2. Canonicalize and validate an explicit path, or resolve a bare executable against the effective PATH.
3. Require the resolved target to be a regular executable file.
4. Pass the canonical absolute executable path to `Command::new`.

For a bare executable name, the effective PATH is selected in this order:

- An explicit `PATH` override in `ProgramSpec`.
- The parent PATH only when environment inheritance is set to `Inherit`.
- No implicit PATH when environment inheritance is `Clear`.

Empty and relative PATH entries are ignored. This prevents accidental execution from the current working directory. A cleared environment with a bare executable therefore requires an explicit absolute PATH override. Absolute executable paths do not require PATH.

Symlinks are canonicalized before launch. The OS still performs the final executable permission check at spawn time; canonicalization and metadata validation cannot eliminate filesystem TOCTOU races. The application does not claim to pin an executable inode against concurrent same-user filesystem changes.

## 2. Environment inheritance

`ProgramSpec` carries an explicit `EnvironmentInheritance` policy:

- `Inherit` (default): preserve the current process environment, then apply explicit overrides. This preserves normal interactive shell compatibility.
- `Clear`: start with an empty environment, then apply only explicit overrides.

The policy is part of the process specification and is applied to both PTY and stdio child processes. The runtime does not mutate the parent application's environment.

Environment inheritance is not a sandbox. Inherit mode intentionally passes the user's environment—including any credentials or loader-related variables already present in it—to the child. Use Clear when a process must receive a minimal environment, and explicitly supply required variables such as PATH, HOME, TERM, or locale settings.

## 3. Working-directory validation

A configured working directory is canonicalized before process resources are acquired. The runtime rejects:

- A path that does not exist.
- A path that resolves to a non-directory.
- NUL-containing values during structural validation.

The canonical directory is passed to the child process. This prevents ambiguous relative-path interpretation between validation and spawn. It does not sandbox the child or prevent it from accessing other files available to the same OS user.

## 4. File-descriptor ownership

- PTY master and slave descriptors are opened with `O_CLOEXEC`.
- Duplicated PTY descriptors use `F_DUPFD_CLOEXEC`.
- Only the intended slave descriptors are attached to child stdin/stdout/stderr.
- The controlling-terminal descriptor is used in `pre_exec` and remains close-on-exec.
- The parent owns the PTY master; the child owns its standard-stream descriptors after exec.
- Stdio pipes are created and transferred through `Command`'s explicit `Stdio` configuration.
- Child process groups are private; signals target only the owned group.

The descriptor-isolation test checks that the child does not inherit internal PTY/control descriptors beyond standard streams.

## 5. Sensitive-data exposure

`ProgramSpec` debug output is intentionally redacted. It reports only argument and environment entry counts, whether a working directory is set, and the environment inheritance mode. It does not print executable paths, argument values, environment keys/values, or working-directory paths.

Program-spec validation errors are stable categories and do not embed supplied values. Runtime spawn errors are mapped to typed categories rather than exposing raw command/environment content.

Call sites must still avoid logging raw `OsString` values or environment maps directly.

## 6. Verification

The change adds tests for executable resolution, explicit PATH policy, rejection of relative PATH entries, clear environment behavior, working-directory validation, PTY descriptor isolation, and ProgramSpec debug redaction.

## 7. Security limitations

- Environment inheritance defaults to Inherit for compatibility; this is not a security boundary.
- Relative PATH entries are intentionally ignored, which is stricter than some shell environments.
- Filesystem canonicalization cannot eliminate TOCTOU races for files mutable by the same user.
- Child processes run with the user's OS privileges; a separate OS sandbox is required for containment.
- This document is not evidence of release availability until the implementation PR is merged and CI passes.
