# Task Catalog

All task records below are GitHub Issues. Each task links to its parent Epic and contains implementation scope, dependencies, acceptance criteria, and Definition of Done.

| Task ID | Issue | Task | Parent Epic |
|---|---:|---|---|
| DE-001-01 | #25 | Initialize repository structure | #1 |
| DE-001-02 | #26 | Create Rust Cargo workspace | #1 |
| DE-001-03 | #27 | Integrate C build system | #1 |
| DE-001-04 | #28 | Pin toolchain and developer tooling | #1 |
| DE-002-01 | #29 | Write system architecture specification | #2 |
| DE-002-02 | #30 | Define cross-component API contracts | #2 |
| DE-004-01 | #31 | Create DE-Shell C library skeleton | #4 |
| DE-004-02 | #32 | Implement shell input buffer | #4 |
| DE-004-03 | #33 | Implement shell lexer | #4 |
| DE-004-04 | #34 | Implement shell parser | #4 |
| DE-007-01 | #35 | Define DE-Runtime interface | #7 |
| DE-013-01 | #36 | Create command registry | #13 |
| DE-004-05 | #37 | Implement shell execution engine | #4 |
| DE-019-01 | #38 | Establish unit testing foundation | #19 |
| DE-003-01 | #39 | Implement application bootstrap and lifecycle | #3 |
| DE-003-02 | #40 | Implement application error and diagnostics model | #3 |
| DE-003-03 | #41 | Implement service orchestration interfaces | #3 |
| DE-005-01 | #42 | Define terminal cell and screen-grid model | #5 |
| DE-005-02 | #43 | Implement cursor, scrolling, and resize semantics | #5 |
| DE-005-03 | #44 | Implement VT/ANSI parser state machine | #5 |
| DE-006-01 | #45 | Implement Linux PTY allocation and setup | #6 |
| DE-006-02 | #46 | Implement child process spawn and lifecycle | #6 |
| DE-006-03 | #47 | Implement PTY bidirectional I/O and resize propagation | #6 |
| DE-007-02 | #48 | Implement Linux process runtime adapter | #7 |
| DE-008-01 | #49 | Define versioned Rust-C ABI contract | #8 |
| DE-008-02 | #50 | Implement FFI boundary and error translation | #8 |
| DE-009-01 | #51 | Define renderer abstraction and frame contract | #9 |
| DE-009-02 | #52 | Implement terminal cell rendering pipeline | #9 |
| DE-010-01 | #53 | Implement keyboard and text input mapping | #10 |
| DE-010-02 | #54 | Implement paste, mouse, and focus input handling | #10 |
| DE-011-01 | #55 | Implement desktop window and terminal surface | #11 |
| DE-011-02 | #56 | Implement tab and pane interaction model | #11 |
| DE-012-01 | #57 | Specify shell grammar and execution semantics | #12 |
| DE-012-02 | #58 | Implement variables, quoting, and expansion | #12 |
| DE-014-01 | #59 | Implement terminal session lifecycle manager | #14 |
| DE-014-02 | #60 | Implement workspace and session restoration metadata | #14 |
| DE-015-01 | #61 | Define configuration schema and validation | #15 |
| DE-015-02 | #62 | Implement configuration storage and migration | #15 |
| DE-016-01 | #63 | Create terminal threat model and security requirements | #16 |
| DE-016-02 | #64 | Harden process, environment, and filesystem boundaries | #16 |
| DE-019-02 | #65 | Add parser and terminal-engine fuzz targets | #19 |
| DE-019-03 | #66 | Create PTY and end-to-end integration test harness | #19 |
| DE-017-01 | #67 | Define plugin capability and permission model | #17 |
| DE-017-02 | #68 | Prototype isolated plugin execution boundary | #17 |
| DE-018-01 | #69 | Implement terminal search and selection workflow | #18 |
| DE-018-02 | #70 | Implement profiles and command history integration | #18 |
| DE-020-01 | #71 | Establish performance benchmarks and budgets | #20 |
| DE-020-02 | #72 | Profile and optimize terminal rendering hot paths | #20 |
| DE-021-01 | #73 | Define platform abstraction and capability matrix | #21 |
| DE-021-02 | #74 | Build cross-platform CI strategy | #21 |
| DE-022-01 | #75 | Write contributor build and development guide | #22 |
| DE-022-02 | #76 | Maintain architecture decision records | #22 |
| DE-023-01 | #77 | Define reproducible packaging and install strategy | #23 |
| DE-023-02 | #78 | Implement release artifact and integrity pipeline | #23 |
| DE-012-03 | #79 | Implement redirection and pipeline execution | #12 |
| DE-011-03 | #80 | Implement settings and terminal profile UI | #11 |
| DE-024-01 | #81 | Create production release qualification checklist | #24 |
| DE-024-02 | #82 | Run release candidate verification and sign-off | #24 |

## Coverage

- 24 Epics: #1–#24.
- 58 implementation tasks: #25–#82.
- 10 delivery milestones: M0–M10 (11 milestones including M0).
- Initial critical-path tasks are explicitly ordered in the Dependency Map.

## Important

This catalog is an index, not a replacement for the GitHub Issues. Use the Issues as the source of truth for task state, discussion, acceptance evidence, and dependency updates.
