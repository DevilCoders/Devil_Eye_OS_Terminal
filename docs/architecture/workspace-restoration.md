# Workspace and Session Restoration Metadata

**Status:** Version 1 metadata model and strict format-neutral codec implemented by DE-014-02. The codec persists workspace presentation state and safe session metadata only. It does not serialize or restore live operating-system process state.

## 1. Scope and ownership

WorkspaceSnapshot is an application-owned value model that can be encoded to and decoded from the existing ConfigValue tree. It is independent of the file-format adapter and does not own a SessionManager, runtime service, process handle, PTY, or GUI widget.

The later configuration persistence task (DE-015-02 / #62) owns concrete file parsing, atomic writes, backups, and corruption recovery. This task defines the versioned workspace payload and its validation contract.

## 2. Version 1 logical structure

The root contains exactly four fields:

- schema_version — required integer, exactly 1.
- active_workspace_id — non-zero persistent workspace identifier.
- workspaces — ordered workspace records.
- sessions — safe saved-session metadata records.

Each workspace contains an ID, a bounded display name, its active tab ID, and ordered tabs. Each tab contains an ID, title, active saved-session ID, and a pane layout tree.

A pane layout is either:

- A session leaf referencing one SavedSessionId.
- A split node with horizontal/vertical orientation, a ratio in permille (100–900), and exactly two child nodes.

Session metadata contains only:

- A persistent metadata ID distinct from the runtime SessionId.
- A bounded user-visible title.
- The logical session kind (external_program or de_shell).
- The last known terminal columns and rows.

## 3. Validation and resource limits

| Constraint | Limit |
| --- | ---: |
| Workspaces per snapshot | 1–32 |
| Tabs per workspace | 1–32 |
| Saved sessions per snapshot | 1–128 |
| Pane tree depth | 8 |
| Pane tree nodes per tab | 255 |
| Workspace/tab/session title | 128 UTF-8 bytes |
| Terminal columns | 1–500 |
| Terminal rows | 1–300 |
| Terminal cells | 150,000 |
| Split ratio | 100–900 permille |

All persistent IDs must be non-zero and unique within their type. The active workspace and active tab must exist. Each tab's active session must be a leaf in its layout. Every saved session must be referenced by exactly one pane; duplicate references and orphan metadata are rejected.

The decoder rejects non-table roots, missing fields, wrong types, unknown keys, unsupported schema versions, invalid identifiers, invalid references, oversized collections, excessive pane depth, invalid split ratios, and invalid terminal dimensions. No unknown field is silently discarded.

## 4. Process-restoration safety boundary

A saved session is a metadata record, not a process checkpoint. The schema intentionally excludes:

- Executable paths and command strings.
- Argument vectors and environment values.
- PIDs, process-group IDs, and exit status.
- PTY file descriptors or native handles.
- Pending input buffers or process memory/state.

WorkspaceSnapshot::from_value only reconstructs validated Rust metadata. It has no runtime dependency and cannot launch a child process. A restored workspace can recreate the tab/pane presentation and show prior session titles, but starting a new process requires a separate explicit user action and a fresh validated SessionSpec.

The decoder never attempts to infer or replay a prior command from a title, path, or saved session kind. It does not silently substitute /bin/sh or any other executable.

## 5. Failure behavior

Encoding validates the complete in-memory snapshot before producing a value tree. Decoding builds a complete candidate and validates all cross-references before returning it. Any invalid or corrupt field rejects the whole snapshot; callers do not receive a partially restored workspace.

Errors are typed as root/type/missing/unknown/version/invalid-value failures. Messages identify the field and reason without including process arguments, environment values, or secret data.

The codec is deterministic because its object representation uses ordered maps. It does not itself write files; the storage adapter is responsible for atomic replacement and preserving the previous valid file when persistence fails.

## 6. Verification

Automated tests cover:

- Round-trip encoding/decoding of multiple sessions and a split layout.
- Explicit schema version and root-field contract.
- Corrupt root, missing field, wrong type, unknown key, and unsupported version.
- Duplicate IDs, dangling references, orphan metadata, and invalid active selections.
- Invalid split ratios, excessive pane depth, title control characters, and terminal bounds.
- Absence of executable, arguments, environment, PID, process-group, PTY, pending-input, and exit-state fields.
- Metadata-only decoding with no process-spawn capability.
