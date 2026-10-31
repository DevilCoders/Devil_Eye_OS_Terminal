# Engineering Milestones

Milestones are delivery gates, not calendar promises. A milestone is complete only when its exit criteria and required evidence are satisfied.

| Milestone | Scope | Exit gate |
|---|---|---|
| M0 Engineering Foundation | Repository, toolchain, architecture, build, CI | Clean checkout builds; contracts and conventions reviewed; CI checks pass |
| M1 Shell Alpha | C shell core, lexer/parser, command registry | Deterministic shell parsing and command dispatch tests pass |
| M2 Runtime Alpha | Runtime interface and Linux adapter | Process launch, environment, cwd, signals, and exit status tested |
| M3 Terminal Engine Alpha | Screen model, VT/ANSI parser, cursor and scroll | Parser/state conformance tests and fuzz baseline pass |
| M4 PTY Alpha | PTY allocation, process lifecycle, I/O | Interactive process, resize, signal, EOF, and cleanup tests pass |
| M5 Communication Alpha | Rust-C ABI/FFI | Versioned ABI, ownership rules, and cross-language tests pass |
| M6 Terminal GUI Alpha | Renderer, input, initial desktop GUI | Usable terminal window with input/output and resize behavior |
| M7 Integrated Beta | Shell language, sessions, config, integration | End-to-end workflows stable; state persistence validated |
| M8 Feature Complete | Productivity, plugins, performance, platform expansion | Feature scope complete; performance and isolation tests pass |
| M9 Release Candidate | Security review, docs, packaging, qualification | No unresolved release-blocking security/quality defects; install artifacts verified |
| M10 Production 1.0 | Production release and maintenance handoff | Release checklist signed off; known issues and rollback documented |

## Continuous gates

- **Security (DE-016):** threat model, secure process boundaries, input validation, and secret-safe diagnostics are reviewed throughout development.
- **Testing (DE-019):** tests are added with each implementation; fuzzing and regression suites expand continuously.
- **Documentation (DE-022):** architecture decisions and public behavior stay synchronized with implementation.
- **Performance (DE-020):** record baselines before optimization and verify regressions after changes.

## Milestone governance

1. Do not close a milestone because its target date arrives.
2. Every required Epic and Task must be complete or explicitly descoped with rationale.
3. Any descoping requires a documented decision and updated acceptance criteria.
4. Release milestones require security, test, packaging, and documentation sign-off.
