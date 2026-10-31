# Engineering Kanban

## Recommended GitHub Project

Create a repository-level GitHub Project named **DEVIL_EYE OS Terminal — Engineering** and add the repository's Issues.

### Views

1. **Board — Delivery:** grouped by Status; columns Planned, Todo, In Progress, Blocked, Review, Done.
2. **Roadmap — Milestones:** grouped by Milestone; sorted by Priority.
3. **Backlog — Priority:** table sorted P0 → P3, then Milestone and Epic.
4. **By Component:** board grouped by Component for Rust, C Shell, Terminal Engine, PTY, Runtime, IPC/FFI, Renderer, GUI, Security, Testing, Docs, Release.
5. **Blocked Work:** filtered to status:blocked; show dependency and parent Epic fields.

### Recommended Project fields

- Status (single select): Planned, Todo, In Progress, Blocked, Review, Done.
- Priority (single select): P0, P1, P2, P3.
- Work Type (single select): Epic, Task, Bug, Spike.
- Component (single select): Build, Architecture, Rust, C Shell, Terminal Engine, PTY, Runtime, IPC/FFI, Renderer, GUI, Session, Config, Security, Plugins, Productivity, Performance, Testing, Platform, Docs, Release.
- Milestone (Milestone field).
- Epic ID (text).
- Task ID (text).
- Platform (single select): Linux x86_64, Windows, macOS.
- Estimate (number; optional).

## Operating rules

- Every active task must be on the Project board.
- Only tasks with clear acceptance criteria and resolved dependencies enter Todo.
- Limit active work in progress; recommended starting WIP limit is 2–3 tasks per engineer.
- Review blocked items at every planning session.
- Move issues to Done only after Definition of Done is satisfied.
- Use milestone views for release planning; do not treat the board as a substitute for dependency tracking.

## Provisioning note

GitHub Issues, Milestones, and Labels can be managed through the repository automation workflow. GitHub Projects board creation and field/view configuration are not exposed by the current connected GitHub actions. Create the Project using GitHub's Projects interface, then add this repository and configure the fields/views above.
