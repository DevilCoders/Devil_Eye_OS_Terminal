# Versioned Application Configuration Schema

**Status:** Schema version 1 and semantic validator. This task defines the application configuration contract and a format-neutral value representation. File parsing, atomic persistence, and live reload are separate work tracked by DE-015-02 (#62) and later configuration tasks.

## 1. Schema envelope

Every persisted configuration must have an explicit `schema_version`. The current version is **1**.

The root table accepts exactly three sections:

- `terminal`
- `startup`
- `resources`

Unknown keys are rejected at the root and in every fixed-schema section. Environment variable names are dynamic map keys and are validated using environment-identifier rules instead.

## 2. Version 1 structure

The canonical logical structure is:

```text
schema_version = 1

[terminal]
columns = 80
rows = 24
scrollback_lines = 10000

[startup]
program = "/bin/sh"
args = []
working_directory = null

[startup.environment]
TERM = "xterm-256color"

[resources]
max_sessions = 32
max_pending_input_bytes = 1048576
max_paste_bytes = 1048576
```

This is a schema illustration, not a selected on-disk syntax. The persistence task will choose and implement the concrete file format. The application layer consumes a strict `ConfigValue` tree and does not parse file bytes itself.

### Field reference

| Path | Type | Default | Validation |
| --- | --- | --- | --- |
| `schema_version` | Integer | Required | Must equal 1 |
| `terminal.columns` | Integer | 80 | 1–500 |
| `terminal.rows` | Integer | 24 | 1–300 |
| `terminal.scrollback_lines` | Integer | 10,000 | 0–1,000,000 |
| `startup.program` | String | `/bin/sh` | Non-empty, no NUL, at most 4096 bytes |
| `startup.args` | Array of strings | Empty | At most 256; each at most 4096 bytes; no NUL |
| `startup.working_directory` | String or null | Null | At most 4096 bytes; no NUL |
| `startup.environment` | String map | Empty | At most 256 entries |
| `resources.max_sessions` | Integer | 32 | 1–64 |
| `resources.max_pending_input_bytes` | Integer | 1 MiB | 4 KiB–16 MiB |
| `resources.max_paste_bytes` | Integer | 1 MiB | 1 byte–16 MiB |

The terminal grid is additionally limited to 150,000 cells. The initial dimensions are converted into the existing validated `TerminalSize` type.

### Environment map

Environment keys must match `[A-Za-z_][A-Za-z0-9_]*`, be at most 256 bytes, and contain no NUL or equals sign. Values are strings, at most 4096 bytes, and contain no NUL. Entries are stored in deterministic key order.

## 3. Defaults

Omitted optional sections and fields receive the defaults in the table. A missing `schema_version` is an error, not an implicit version 0.

Defaults are applied only after the root value is confirmed to be a table and the schema version is accepted. Defaults do not excuse invalid explicitly supplied values.

The default executable is `/bin/sh`; arguments remain a separate vector. The application never concatenates the executable and arguments into a shell command string.

## 4. Validation behavior

`AppConfig::from_value` performs strict decoding and semantic validation:

1. Require a root table and explicit schema version.
2. Reject any schema version other than 1.
3. Decode known sections and apply documented defaults.
4. Reject unknown keys in the root and fixed-schema sections.
5. Validate dimensions, grid-cell budget, resource limits, executable, arguments, working directory, and environment.
6. Build a validated `ProgramSpec` with separate executable, arguments, environment overrides, and working directory.

Errors are typed: missing field, unknown key, wrong type, unsupported version, invalid value, non-UTF-8 process value, or invalid process specification. Error messages identify the field but never print secret environment values.

No filesystem existence checks are performed during schema validation. The runtime owns path resolution and executable launch errors. A missing working directory or executable is reported at session creation with a runtime diagnostic, not silently rewritten.

## 5. Compatibility and migration policy

- Version 1 is the first persisted schema. No earlier persisted format exists in the repository, so there is no implicit v0 migration.
- Missing, older, and newer schema versions are rejected with an explicit unsupported/missing-version error.
- The application must not silently downgrade, discard unknown fields, or reinterpret a future version as version 1.
- A future migration must be an explicit version-to-version transformation with tests for preserved values and defaults.
- The persistence layer must preserve the original file (or create a verified backup) before writing a migrated representation.
- Migration failure leaves the original configuration untouched and reports a typed error.
- A schema version increase is required for incompatible field meaning, type, default, or ownership changes. Compatible optional additions still require a schema review because unknown keys are rejected.

## 6. Security and ownership invariants

- Executable and arguments remain separate values; no implicit command interpreter is invoked.
- Environment overrides are explicit key/value pairs; the application does not mutate the host process environment.
- Unknown keys fail closed rather than being silently ignored.
- All strings are length-bounded and NUL-checked before becoming a `ProgramSpec`.
- The application does not log environment values or full command arguments.
- Resource limits are validated before session creation and are applied by the session manager/input mapper.
- Non-UTF-8 OS paths can be used by runtime APIs, but cannot be represented in this UTF-8 schema; `to_value` reports this explicitly instead of lossy conversion.

## 7. Implementation boundary

Implemented in DE-015-01:

- `ConfigValue` format-neutral value tree.
- Version 1 `AppConfig` representation and defaults.
- Strict unknown-key and type validation.
- Semantic validation and bounded resource policy.
- Round-trip conversion between `AppConfig` and `ConfigValue`.
- Unit tests for defaults, unknown keys, schema mismatch, invalid values, argument separation, and round-trip behavior.

Not implemented here:

- TOML/JSON byte parsing or serialization.
- File location discovery, atomic writes, backups, or corruption recovery.
- Live configuration reload or per-session overrides.
- GUI settings panels.

Those belong to the persistence and configuration-service tasks. The parser adapter must map its parsed document into `ConfigValue`; it must not bypass this validator.
