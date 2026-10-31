# DEVIL_EYE OS Terminal — Engineering Roadmap

## Product and architecture baseline

- **Rust:** desktop application, lifecycle, orchestration, GUI, renderer integration, configuration.
- **C:** independent DE-Shell core: input, lexer, parser, command dispatch, execution semantics.
- **NASM:** only justified low-level routines; not used merely to increase assembly usage.
- **PTY:** process/session transport boundary; each terminal tab owns an isolated PTY session.
- **Terminal engine:** VT/ANSI parsing, screen grid, Unicode cells, cursor, modes, scrollback, resize.
- **Platform baseline:** Linux x86_64 first; preserve adapters for Windows/ConPTY and macOS.
- **Shell independence:** emulator must host compatible external shells (Bash, Zsh, Fish); DE-Shell is not the terminal emulator.
- **Quality gate:** no placeholder implementations or support claims without tests and reproducible evidence.

## Epics

| Epic | Issue | Priority | Milestone |
|---|---:|---:|---|
| DE-001 Project Infrastructure | #1 | P0 | M0 |
| DE-002 System Architecture | #2 | P0 | M0 |
| DE-003 Rust Application Core | #3 | P0 | M0 |
| DE-004 C Shell Core | #4 | P0 | M1 |
| DE-005 Terminal Screen Engine | #5 | P0 | M3 |
| DE-006 PTY Backend | #6 | P0 | M4 |
| DE-007 DE-Runtime | #7 | P0 | M2 |
| DE-008 IPC & FFI | #8 | P0 | M5 |
| DE-009 Terminal Renderer | #9 | P0 | M6 |
| DE-010 Terminal Input Engine | #10 | P0 | M6 |
| DE-011 Terminal GUI | #11 | P1 | M6 |
| DE-012 Shell Language | #12 | P1 | M7 |
| DE-013 Command System | #13 | P1 | M1 |
| DE-014 Session & Workspace Manager | #14 | P1 | M7 |
| DE-015 Configuration & Persistence | #15 | P1 | M7 |
| DE-016 Security Engineering | #16 | P0 | Continuous |
| DE-017 Plugin Architecture | #17 | P2 | M8 |
| DE-018 Productivity Features | #18 | P2 | M8 |
| DE-019 Testing & QA | #19 | P0 | Continuous |
| DE-020 Performance Engineering | #20 | P1 | M8 |
| DE-021 Cross-Platform Support | #21 | P2 | M8 |
| DE-022 Documentation | #22 | P1 | Continuous |
| DE-023 Packaging & Distribution | #23 | P1 | M9 |
| DE-024 Production Release | #24 | P0 | M10 |

## Initial execution tasks

| Task | Issue | Epic | Depends on |
|---|---:|---|---|
| DE-001-01 Initialize repository structure | #25 | #1 | — |
| DE-001-02 Create Rust Cargo workspace | #26 | #1 | #25 |
| DE-001-03 Integrate C build system | #27 | #1 | #25 |
| DE-001-04 Pin toolchain and developer tooling | #28 | #1 | #25 |
| DE-002-01 Write system architecture specification | #29 | #2 | #25 |
| DE-002-02 Define cross-component API contracts | #30 | #2 | #29 |
| DE-004-01 Create DE-Shell C library skeleton | #31 | #4 | #27, #30 |
| DE-004-02 Implement shell input buffer | #32 | #4 | #31 |
| DE-004-03 Implement shell lexer | #33 | #4 | #32 |
| DE-004-04 Implement shell parser | #34 | #4 | #33, #30 |
| DE-007-01 Define DE-Runtime interface | #35 | #7 | #30 |
| DE-013-01 Create command registry | #36 | #13 | #30, #31 |
| DE-004-05 Implement shell execution engine | #37 | #4 | #34, #36, #35 |
| DE-019-01 Establish unit testing foundation | #38 | #19 | #26, #27 |

## Milestones

- **M0 — Engineering Foundation:** repository, toolchain, architecture, build and CI foundations.
- **M1 — Shell Alpha:** initial C shell core and command registry.
- **M2 — Runtime Alpha:** runtime contract and first platform implementation.
- **M3 — Terminal Engine Alpha:** screen model and terminal state engine.
- **M4 — PTY Alpha:** interactive process I/O and lifecycle.
- **M5 — Communication Alpha:** Rust↔C IPC/FFI integration.
- **M6 — Terminal GUI Alpha:** renderer, input, and initial GUI.
- **M7 — Integrated Beta:** shell language, sessions, configuration, and end-to-end integration.
- **M8 — Feature Complete:** productivity, plugin architecture, performance, and platform expansion.
- **M9 — Release Candidate:** security, docs, packaging, and release qualification.
- **M10 — Production 1.0:** production release and maintenance handoff.

## Task issue standard

Every task issue should include a stable ID, parent Epic, priority, status, component, platform, description, dependencies, subtasks, observable acceptance criteria, test cases, security/performance considerations, documentation requirements, and Definition of Done.

## Governance

1. Enforce dependency order; blocked tasks cannot be marked ready without an explicit decision.
2. Define interfaces before parallel implementation.
3. Add tests with implementation.
4. Keep platform-specific code behind adapters.
5. Treat security as continuous P0 work.
6. Track technical debt explicitly; do not silently weaken acceptance criteria.
7. Close parent Epics only after child tasks are complete and evidence is linked.

## Current setup note

The initial 24 Epic issues and 14 first-wave task issues are created in GitHub. This roadmap is the human-readable index. Native GitHub Milestones, Labels, and Projects/Kanban still need provisioning; they are not represented as created by this file.


## Expanded engineering management system

- [Task Catalog](TASK_CATALOG.md) — complete index of 61 task Issues (#25–#85).
- [Workflow](WORKFLOW.md) — hierarchy, statuses, priorities, dependencies, Definition of Done, and PR rules.
- [Milestones](MILESTONES.md) — M0–M10 exit criteria.
- [Kanban operating model](KANBAN.md) — recommended Project views and fields.
- [Dependency Map](DEPENDENCY_MAP.md) — initial critical-path graph.

## Current backlog inventory

- 24 Epic Issues: #1–#24.
- 61 Task Issues: #25–#85.
- 85 total open engineering Issues at the time this index was updated.
- Issue templates: Epic, Task, Bug.
- Pull request template with validation and evidence checklist.
- GitHub Actions automation: `.github/workflows/engineering-taxonomy.yml` provisions taxonomy labels and M0–M10 milestones, then classifies open issues. It runs on new/reopened issues and can be run manually for a full backfill. `.github/workflows/dependency-status.yml` reconciles blocked/ready states when issues open, reopen, or close.

## Native Project board

The current GitHub integration does not expose a create/configure GitHub Projects action. Create a repository-level Project named **DEVIL_EYE OS Terminal — Engineering**, add this repository's Issues, and configure the views/fields in [KANBAN.md](KANBAN.md). The labels and milestones can be provisioned by the included workflow after it is run.
