# Initial Dependency Map

This map records the first implementation path. Issue references are canonical; update this file when dependencies change.

```text
DE-001-01 #25
  ├─ DE-001-02 #26 ─┐
  └─ DE-001-03 #27 ─┼─> DE-004-01 #31
                    │       └─> DE-004-02 #32
                    │              └─> DE-004-03 #33
                    │                     └─> DE-004-04 #34
                    │
DE-002-01 #29 ─> DE-002-02 #30 ─┬─> DE-007-01 #35 ─> DE-007-02 #48
                               ├─> DE-013-01 #36
                               ├─> DE-008-01 #49 ─> DE-008-02 #50
                               └─> DE-009-01 #51

DE-004-04 #34 + DE-013-01 #36 + DE-007-01 #35
  └─> DE-004-05 #37

DE-005-01 #42 ─> DE-005-02 #43 ─> DE-005-04 #140 ─> DE-020-02 #72
       └───────> DE-005-03 #44

DE-006-01 #45 ─> DE-006-02 #46 ─> DE-006-03 #47

DE-009-01 #51 ─> DE-009-02 #52
DE-010-01 #53 ─> DE-010-02 #54
DE-011-01 #55 ─> DE-011-02 #56
DE-012-01 #57 ─> DE-012-02 #58
DE-014-01 #59 ─> DE-014-02 #60
DE-015-01 #61 ─> DE-015-02 #62

DE-016-01 #63 ─> DE-016-02 #64
DE-019-01 #38 ─> DE-019-02 #65
DE-019-01 #38 ─> DE-019-03 #66
```

## Dependency hygiene

- The map is a planning aid; the Dependencies section in each issue remains the source of truth.
- Use `Blocked by #N` and `Blocks #N` consistently.
- Revisit downstream readiness when a dependency changes.
- Do not close issues solely to unblock a board.


## Backlog governance automation chain

```text
DE-001-01 #25
  └─> DE-001-05 #83 (taxonomy and milestones)
        └─> DE-001-06 #84 (dependency-aware readiness)
              └─> DE-001-07 #85 (metadata audit)
```

The dependency-status workflow derives blocked/ready labels from the task's Dependencies section and the open/closed state of referenced issues. When a dependency closes, downstream eligible tasks are moved to Todo; when it reopens, they return to Blocked. In-progress, review, and done labels are protected.

## Subtask hierarchy note

The 63 implementation Tasks each contain a Markdown subtask checklist. These checklist items clarify execution order but do not create dependency edges or native GitHub parent-child issue relationships. Only dependencies explicitly listed in each Task's `Dependencies` section are consumed by the dependency-status automation. If a checklist item becomes independently schedulable, promote it to a new Issue and update the catalog and dependency graph.


## Later milestone dependencies

The initial graph above is retained as the foundation path. The following later task edges are also present in the canonical GitHub issue bodies and must be kept in sync:

```text
DE-011-01 #55 ─> DE-011-02 #56
      │                 ▲
      └─> DE-011-03 #80 │
DE-014-01 #59 ──────────┘

DE-015-01 #61 ─> DE-015-02 #62 ─> DE-018-02 #70
DE-014-01 #59 ─────────────────> DE-018-02 #70

DE-016-01 #63 + DE-002-02 #30 ─> DE-017-01 #67 ─> DE-017-02 #68
DE-005-01 #42 + DE-011-01 #55 ─> DE-018-01 #69

DE-021-01 #73 + DE-019-01 #38 ─> DE-021-02 #74
DE-023-01 #77 + DE-019-01 #38 ─> DE-023-02 #78

DE-012-02 #58 + DE-006-03 #47 + DE-004-05 #37 ─> DE-012-03 #79

DE-015-01 #61 + DE-011-01 #55 ─> DE-011-03 #80
DE-016-01 #63 + DE-019-01 #38 + DE-022-01 #75 ─> DE-024-01 #81
DE-023-02 #78 + DE-024-01 #81 ─> DE-024-02 #82
```

### Explicit architecture decision gates

- **#55 (GUI window/surface):** do not begin implementation until the user records the GUI/windowing toolkit decision. This is an additional project-level gate beyond the issue's prerequisite tasks.
- **#62 (configuration persistence):** do not begin implementation until the user records the storage format/codec decision.
- Tasks depending on #55 or #62 remain blocked by those gates even if their issue labels have not yet been reconciled. Do not relabel them Ready or start implementation until the decision is recorded.
- #67 (plugin capability contract) is implemented and merged in PR #153; its issue is marked `status:done`. #68 is a separate runtime/isolation task and remains blocked until the Linux sandbox mechanism, package-signing/trust policy, and measurable quota policy are selected and recorded.
- #74 prerequisites #73 and #38 are closed. #78 prerequisites #77 and #38 are closed, but #82 remains downstream of both #78 and #81.
- #79 implementation is under PR #151; keep the issue open until merge and post-merge verification.

The GitHub Issue `Dependencies` sections remain authoritative. If a graph edge and an issue body disagree, reconcile the issue first and then update this map; do not silently choose one source.
