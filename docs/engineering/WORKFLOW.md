# Engineering Workflow

## Work item hierarchy

- **Epic:** major engineering outcome; one of DE-001 through DE-024.
- **Task:** bounded implementation unit with a single owner and verifiable acceptance criteria.
- **Subtask:** smaller checklist item inside a Task when it does not need independent scheduling or ownership. Promote it to a separate Issue when it has independent dependencies, review, or delivery value.
- **Bug:** defect with reproduction steps, impact, and regression test.
- **Spike:** time-boxed research task with a written decision or prototype result.

## Status model

Use one status label per issue:

1. `status:planned` — scoped but not ready; dependencies or design are unresolved.
2. `status:todo` — ready to start; acceptance criteria and dependencies are clear.
3. `status:in-progress` — active implementation.
4. `status:blocked` — waiting on an upstream issue or decision.
5. `status:review` — implementation complete and awaiting review/validation.
6. `status:done` — acceptance criteria verified and evidence attached.

Do not infer completion from code being pushed. Completion requires review, tests, docs, and evidence.

## Priority model

- **P0:** critical path, security, data integrity, or release blocker. Resolve before dependent work.
- **P1:** required for the planned alpha/beta or production baseline.
- **P2:** important feature or quality improvement after core stability.
- **P3:** optional enhancement or low-impact backlog item.

Priority is not a substitute for sequencing. Dependencies always govern readiness.

## Required task fields

Every Task must have a stable ID, parent Epic, priority, status, component, platform, description, explicit dependencies, acceptance criteria, relevant test cases, documentation impact, and Definition of Done.

## Dependency protocol

- Use explicit GitHub issue references in the Dependencies section: `Blocked by #N` and `Blocks #N`.
- A blocked task must carry `status:blocked`; do not start it without documenting an approved exception.
- When an upstream task closes, reassess directly dependent tasks.
- Avoid circular dependencies. The task owner must resolve cycles before work starts.

## Definition of Done

- Implementation meets all acceptance criteria.
- Unit, integration, regression, and security tests relevant to the change pass.
- Formatting, lint, and build checks pass.
- API/architecture/user documentation is updated.
- No placeholder code, silently skipped tests, or unsupported capability claims.
- Review feedback is resolved.
- Evidence (test output, benchmark, logs, screenshots) is linked in the Issue or PR.

## Pull request policy

- PR title references the Task ID.
- PR links the relevant Issue using `Closes #N` only when it truly completes that issue.
- Keep changes scoped; split unrelated work.
- Record interface changes, migration needs, security impact, and performance impact.
- Do not close an Epic until every required child Task is complete.

## Engineering constraints

- Rust owns desktop application lifecycle and orchestration.
- C owns the independent DE-Shell implementation.
- The terminal emulator remains independent from the shell and must host compatible external shells.
- NASM is reserved for measured, justified low-level routines.
- Linux x86_64 is the first implementation target; platform-specific behavior stays behind adapters.
- Each terminal tab owns a distinct PTY/process session.

## Subtask execution protocol

Every Task Issue must contain a `Subtasks (implementation checklist)` section. Keep checklist items independently verifiable and scoped to the parent Task. Check an item only when its evidence is attached. If a checklist item needs a separate owner, dependency chain, review, or delivery schedule, create a dedicated Task Issue and link it to the parent Epic; do not represent an independent workstream as a checkbox. The current backlog's 62 task issues have been decomposed into checklists; native GitHub child-issue relationships are not configured.
