# Optional GitHub Projects Reference

GitHub Projects V2 is optional for this repository and is not part of the required engineering workflow.

The source of truth for engineering planning and execution is:

- GitHub Issues #1–#86 (with #86 closed as not planned because a Project board is unnecessary).
- Taxonomy labels for issue type, priority, status, and component.
- M0–M10 GitHub Milestones.
- Explicit issue references in each Task's Dependencies section.
- `.github/workflows/engineering-taxonomy.yml` for taxonomy and milestone provisioning.
- `.github/workflows/dependency-status.yml` for dependency-aware readiness labels.

Do not require Project V2 views, custom fields, or board membership before starting or completing engineering work.

If a Project board is ever useful later, it may be added as a read-only convenience over the existing Issues. It must not replace issue metadata, milestones, or dependency tracking.
