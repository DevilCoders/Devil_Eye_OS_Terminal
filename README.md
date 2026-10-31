# Devil_Eye OS Terminal

A native, scalable terminal application built around a strict separation between the terminal emulator, process runtime, and the independent DE-Shell implementation.

## Architecture at a glance

- **Rust** — desktop application, lifecycle, orchestration, GUI, renderer integration, and configuration.
- **C** — DE-Shell core: input processing, lexer, parser, command dispatch, and execution semantics.
- **PTY/runtime** — process transport and per-tab session lifecycle.
- **Terminal engine** — VT/ANSI parsing, screen grid, Unicode cells, cursor/modes, scrollback, and resize.
- **Platform strategy** — Linux x86_64 first; Windows/ConPTY and macOS behind platform adapters.

The terminal emulator is independent of DE-Shell and must be able to host compatible external shells such as Bash, Zsh, and Fish.

## Repository documentation

- [Engineering roadmap](docs/engineering/ROADMAP.md)
- [Architecture documentation](docs/architecture/README.md)
- [Build guide](docs/BUILDING.md)
- [Pinned developer toolchain](docs/TOOLCHAIN.md)
- [Engineering workflow](docs/engineering/WORKFLOW.md)
- [Task catalog](docs/engineering/TASK_CATALOG.md)
- [Milestones](docs/engineering/MILESTONES.md)
- [Dependency map](docs/engineering/DEPENDENCY_MAP.md)

## Current implementation status

The repository foundation and engineering backlog are being established first. The Rust workspace, C shell implementation, terminal engine, PTY backend, and GUI are separate implementation tasks; no functionality is claimed until code and tests exist.

## Quality requirements

No placeholder implementations, fake capability claims, unverified completion, or silent test skips. Every implementation task must include tests, documentation updates, and reproducible evidence.
