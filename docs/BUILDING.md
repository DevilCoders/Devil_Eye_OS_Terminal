# Building Devil_Eye OS Terminal

## Current repository state

The repository currently contains the engineering-management foundation and source-layout contract. The Rust workspace and buildable application have not yet been introduced. Do not treat this documentation scaffold as evidence that the application builds.

## Build system ownership

- Task DE-001-02 (#26) introduces the Rust Cargo workspace and crate boundaries.
- Task DE-001-03 (#27) integrates the C build system.
- Task DE-001-04 (#28) pins the toolchain and developer tooling.

The exact build, test, lint, and formatting commands will be recorded here when those tasks establish the real toolchain. Until then, no placeholder build commands are provided.

## Development baseline

- Initial platform: Linux x86_64.
- Rust owns the desktop application and orchestration.
- C owns the independent DE-Shell core.
- NASM is reserved for measured, justified low-level routines.
- No external pretrained or hosted runtime components are part of the terminal application architecture.
