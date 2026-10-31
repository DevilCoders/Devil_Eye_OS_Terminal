# Engineering Taxonomy Validation

**Validation date:** 2026-10-31  
**Scope:** GitHub issue labels, milestones, and automated issue classification for DE-001-05.

## Taxonomy catalog

The committed taxonomy workflow defines 37 unique labels:

| Category | Count | Values |
| --- | ---: | --- |
| Type | 4 | epic, task, bug, spike |
| Priority | 4 | P0, P1, P2, P3 |
| Status | 6 | planned, todo, in-progress, blocked, review, done |
| Component | 20 | build, architecture, rust, c-shell, terminal-engine, pty, runtime, ipc-ffi, renderer, gui, security, testing, docs, release, platform, session, config, plugins, productivity, performance |
| Platform | 3 | linux, windows, macos |

The milestone catalog contains all eleven milestones, M0 through M10.

GitHub label names are unique per repository. The revised workflow verifies a label by fetching it after a create conflict, reconciles its color/description to the source catalog, and fails if the label cannot actually be retrieved. This avoids silently treating every HTTP 422 response as proof that a label already exists.

## Existing workflow evidence

The previous committed workflow completed successfully in these runs:

- [Run 41199534236](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/actions/runs/41199534236) — success.
- [Run 41199738624](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/actions/runs/41199738624) — success.
- [Run 41199905811](https://github.com/DevilCoders/Devil_Eye_OS_Terminal/actions/runs/41199905811) — success.

Run 41199905811 reported that it provisioned the taxonomy and eleven milestones and classified 86 open issues. A separate metadata audit of the 86 Epic/Task issues found exactly one label in each of the required type, priority, status, and component categories.

## Classification rules

- Epic and Task type comes from the issue title prefix.
- Priority comes from the explicit Priority field in the issue body.
- Component is derived from the DE-XXX identifier.
- Platform labels are derived from the explicit Platform field when it identifies Linux, Windows, or macOS.
- Closed issues receive `status:done`.
- Open Epics receive `status:planned`.
- Open Tasks with an open dependency receive `status:blocked`.
- Open Tasks without an open dependency receive `status:todo`, unless they are already actively in progress or awaiting review.
- Existing non-taxonomy labels are preserved.
- Stale taxonomy labels are removed before missing desired labels are added.

The revised workflow processes both open and closed issues, not only the open backlog. It also runs when the workflow definition changes on master, so changes to the taxonomy are automatically validated after merge.

## Milestone exceptions

The Testing & QA epic (#19) and its continuous QA tasks (#38, #65, #66) intentionally have no single milestone. Their work spans the full delivery lifecycle rather than belonging to one release milestone. This is an explicit planning exception, not missing classification.

Issue #86 (manual GitHub Projects V2 setup) was closed with the `not_planned` state reason because project-board creation requires owner-side UI actions and is optional. Its issue body retains the M0 target for historical traceability; the revised workflow reconciles the milestone field while preserving the closed/not-planned resolution. It is not a blocker for the engineering workflow.

## Repeatability and failure behavior

The revised workflow:
- Uses one canonical list of unique taxonomy labels.
- Verifies pre-existing labels and corrects their metadata.
- Creates missing M0–M10 milestones without duplicating existing milestone titles.
- Reconciles metadata across all repository Issues, including closed work.
- Preserves active `status:in-progress` and `status:review` states when dependencies are satisfied.
- Recomputes dependency-blocked status for other open Tasks.
- Runs on manual dispatch, issue open/reopen/close events, and changes to the workflow on master.

The updated workflow's first post-merge run is the final verification for this revision; the historical green runs above establish that the provisioning path already executed successfully before the stricter verification was added.
