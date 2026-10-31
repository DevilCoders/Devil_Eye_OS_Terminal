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

DE-005-01 #42 ─> DE-005-02 #43
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
