# Architecture Decision Records

This directory records decisions that constrain architecture and public subsystem contracts.

## Status vocabulary

- **Proposed:** under discussion; not a binding implementation requirement.
- **Accepted:** approved for the current architecture baseline.
- **Superseded:** replaced by a later ADR; retained for historical context.
- **Deprecated:** no longer recommended, but not yet fully removed.

## Accepted decisions

| ADR | Decision | Status |
| --- | --- | --- |
| [ADR-0001](0001-rust-c-ownership.md) | Rust application orchestration and independent C DE-Shell ownership | Accepted |
| [ADR-0002](0002-terminal-shell-separation.md) | Terminal emulator is shell-agnostic | Accepted |
| [ADR-0003](0003-pty-session-ownership.md) | Backend-specific ownership: one PTY/process handle per external session; one FFI handle per DE-Shell session | Accepted |
| [ADR-0004](0004-renderer-frame-contract.md) | Immutable terminal frames and bounded damage contract | Accepted |
| [ADR-0005](0005-linux-first-platform-adapters.md) | Linux x86_64 first, future OS adapters behind neutral contracts | Accepted |

## How to add or change a decision

1. Copy the template below into a new monotonically numbered file.
2. Record the problem, constraints, considered alternatives, decision, consequences, and affected interfaces.
3. Link relevant Issues, PRs, architecture documents, and tests.
4. Use Proposed until reviewed. Do not mark a decision Accepted merely because an implementation exists.
5. When changing an accepted decision, add a new ADR that supersedes the old one; do not silently rewrite history.

## ADR template

```markdown
# ADR-NNNN: Short decision title

- Status: Proposed
- Date: YYYY-MM-DD
- Decision owners: Architecture maintainers
- Related issues: #N

## Context
What problem and constraints require a decision?

## Decision
State the chosen approach precisely.

## Alternatives considered
List credible alternatives and why they were not selected.

## Consequences
Record positive outcomes, trade-offs, migration impact, and residual risks.

## Affected interfaces and verification
Link contracts, implementation modules, and tests that must enforce the decision.
```

ADRs are design history, not proof that every described component is already implemented. Implementation status remains tracked in the engineering task catalog and the relevant PRs.
