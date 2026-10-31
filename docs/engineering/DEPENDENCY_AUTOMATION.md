# Dependency Status Automation

The repository recalculates task readiness from explicit issue references in each Task's `### Dependencies` section.

## Inputs and rule

The automation considers an issue a blocker when it is an open repository Issue (pull requests are excluded). Only references inside the Dependencies section count; issue references elsewhere in the description or acceptance criteria are not treated as blockers.

For each open Task:

1. If it already has `status:done`, `status:review`, or `status:in-progress`, preserve that protected state.
2. Otherwise, if any referenced dependency Issue is open, set `status:blocked`.
3. Otherwise, set `status:todo`.

The reconciliation step enforces exactly one `status:*` label: stale or conflicting status labels are removed, and the desired label is added only when missing. Non-status labels are not modified by this workflow.

Closed issues are excluded from the open-task pass. The engineering taxonomy workflow assigns `status:done` to closed issues.

## Trigger behavior

`.github/workflows/dependency-status.yml` runs on:
- Task/Dependency Issue opened, reopened, closed, or edited events.
- Manual `workflow_dispatch` for a full reconciliation.

Each run fetches the current set of open issues and recomputes every open Task, so closing or reopening one dependency updates all affected descendants, not only its direct children.

## Implementation and tests

The rule implementation is pure JavaScript in `.github/scripts/dependency-status.cjs`. It has no GitHub API dependency, which allows deterministic tests using Node's built-in test runner.

Run locally:

```sh
node --test tests/automation/dependency-status.test.cjs
```

The `Repository automation tests` workflow runs these tests on pull requests and master pushes that change the rule module, its tests, or the workflow.

Test cases cover:
- Dependency-section parsing and duplicate reference removal.
- No dependency section.
- Any open blocker -> blocked.
- All dependencies closed -> todo.
- Preservation of in-progress, review, and done states.
- Deterministic normalization when multiple status labels exist.
- Minimal label mutations and invalid target rejection.

## Operational safety

- The workflow does not close issues or modify milestones.
- It does not overwrite protected work states.
- It does not modify type, priority, component, or platform labels.
- Reconciliation is idempotent: once the desired status is present and conflicting status labels are absent, another run makes no API writes.
