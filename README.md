# Devil_Eye OS Terminal

Devil_Eye OS Terminal is a native terminal application under active engineering development. Its architecture separates the terminal emulator, process runtime, and independent DE-Shell implementation.

> **Current release status:** pre-release engineering. The initial implementation target is Linux x86_64. This repository does not yet claim a production-ready GUI, Windows/macOS support, persistent settings, or a Production 1.0 release.

## Architecture

- **Rust workspace** — application lifecycle and orchestration, terminal engine, runtime abstractions, safe FFI, and integration boundaries.
- **C / DE-Shell** — independent shell input, lexer, parser, command registry, execution semantics, and session-local variables.
- **Linux runtime** — process execution, PTY ownership, signals, descriptor management, and child cleanup.
- **Terminal engine** — shell-agnostic VT/ANSI parsing, screen grid, Unicode cells, cursor and modes, scrollback, and resize behavior.
- **Platform strategy** — Linux x86_64 is the current implementation baseline. Windows/ConPTY and macOS remain future platform adapters, not supported release targets.

The terminal emulator is not DE-Shell: it must remain capable of hosting compatible external shells and programs through independent PTY sessions.

## Implemented on master

The current master baseline includes:

- Rust/C workspace and independent CMake build for DE-Shell.
- Bounded shell input, quote-aware lexing, parsing, and command dispatch.
- Shell execution semantics, built-in command registry, scalar parameter expansion ($NAME, ${NAME}, and $?), and bounded per-shell variables.
- Terminal screen model, VT parser, cursor/scroll/resize behavior, and bounded scrollback.
- Linux PTY allocation, child lifecycle, bidirectional I/O, runtime interfaces, and process/environment hardening.
- Versioned Rust/C FFI boundary and software renderer contracts.
- Reproducible build, lint, unit/integration, coverage, fuzzing, and performance workflows.

Implementation claims are limited to code and validation present on the checked-out branch. See the [engineering roadmap](docs/engineering/ROADMAP.md) and [architecture documentation](docs/architecture/README.md).

## In progress — not yet part of master

[PR #151](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/pull/151) implements DE-012-03: typed pipeline plans, stdin/stdout/stderr redirections, Linux pipeline execution, and foreground terminal job control. Its latest documented head is dc9b10f1587a4bdaf9cd202a2ecc4342a06a039e; the PR remains Draft pending its review gate. Do not treat these features as merged or available in the master build until the PR is merged.

## Architecture constraints

- Rust owns application lifecycle, orchestration, GUI integration, and runtime coordination.
- C owns the independent DE-Shell implementation; it does not spawn OS processes.
- The terminal engine remains shell-agnostic.
- Each terminal session owns independent PTY/process state and bounded I/O resources.
- Rust/C FFI is explicit, versioned, and validated at the boundary.
- Linux-specific process behavior stays behind runtime/platform adapters.
- No placeholder backend, unsupported-platform claim, or completion claim without tests and reproducible evidence.

## Build and development

- [Build guide](docs/BUILDING.md)
- [Pinned developer toolchain](docs/TOOLCHAIN.md)
- [Contributor guide](CONTRIBUTING.md)
- [Engineering workflow](docs/engineering/WORKFLOW.md)

## Engineering management

- [Engineering roadmap](docs/engineering/ROADMAP.md)
- [Task catalog](docs/engineering/TASK_CATALOG.md)
- [Dependency map](docs/engineering/DEPENDENCY_MAP.md)
- [Milestone exit criteria](docs/engineering/MILESTONES.md)
- [Architecture index](docs/architecture/README.md)
- [Plugin capability and permission model](docs/architecture/plugin-capabilities.md) — normative future security gate; plugin execution is disabled.
- [Security threat model](docs/security/THREAT_MODEL.md)
- [Linux packaging strategy](docs/packaging/linux-packaging.md)

GitHub Issues are the source of truth for task state, dependencies, acceptance evidence, and milestone status. Parent Epics are closed only after their child work and acceptance criteria are verified. GitHub Projects V2 is optional.

## Quality gates

Before proposing a change, run the relevant Rust, C, integration, and documentation checks. The repository CI additionally validates GCC/Clang Debug and Release builds, Rust formatting/tests/Clippy/coverage, C coverage, parser fuzzing, and reproducible Linux performance benchmarks. See [Build and validation](docs/BUILDING.md).

## Release

The project is **not yet Production 1.0**. Release qualification, artifact integrity, platform coverage, GUI decisions, and remaining feature work must be completed and evidenced before a production release is declared.
