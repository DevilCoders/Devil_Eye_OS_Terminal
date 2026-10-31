# Devil_Eye OS Terminal — Security Threat Model

**Document ID:** DE-SEC-TM-001  
**Version:** 1.0  
**Scope baseline:** Linux x86_64, local desktop terminal application  
**Status:** Initial threat model and security requirements

## 1. Security objectives

The terminal is a process-launching application that consumes untrusted byte streams and renders terminal control sequences. Its primary security objectives are:

1. Prevent unintended command execution and argument/environment injection.
2. Prevent memory corruption and undefined behavior across Rust/C boundaries.
3. Prevent a child process or malformed terminal stream from corrupting unrelated sessions or the application.
4. Bound memory, CPU, process, descriptor, and queue consumption.
5. Protect secrets that may appear in environment variables, command arguments, terminal output, clipboard data, and diagnostics.
6. Make process/session ownership and cleanup deterministic.
7. Keep privileged operations and future plugin capabilities explicit and reviewable.

The initial application runs with the invoking user's OS privileges. It is not a sandbox for arbitrary child processes and does not claim to isolate a malicious program from the user's account.

## 2. Assets and security impact

| ID | Asset | Security impact if compromised |
| --- | --- | --- |
| A-01 | Executable/argument launch specification | Arbitrary command execution, privilege misuse, data destruction |
| A-02 | Child process, PTY descriptors, process group | Process escape, orphan process, cross-session I/O |
| A-03 | Terminal output and parser state | Memory corruption, UI spoofing, clipboard or hyperlink abuse |
| A-04 | User input and clipboard/paste data | Command injection, unintended multiline execution, secret disclosure |
| A-05 | Environment variables and working directory | Credential leakage, loader/path manipulation, filesystem access |
| A-06 | Session registry and lifecycle state | Cross-session access, stale handles, orphaned children |
| A-07 | Rust/C ABI handles and buffers | Use-after-free, out-of-bounds access, process crash or code execution |
| A-08 | Configuration and profile data | Unsafe launch settings, resource exhaustion, persistent tampering |
| A-09 | Logs and diagnostics | Credential/token disclosure and sensitive command history leakage |
| A-10 | Build toolchain and dependencies | Supply-chain compromise of distributed binaries |

## 3. Trust boundaries

### TB-01: GUI/input adapter → session input queue

Keyboard, paste, mouse, and focus events are user-controlled. Clipboard content may originate from another process or remote source. The input mapper must encode terminal protocol bytes only after applying the active session's modes and paste policy.

### TB-02: Child process/PTY → terminal parser and renderer

PTY output is untrusted, even when produced by a local process. Escape sequences can request terminal mode changes, alter titles, move the cursor, create hyperlinks, or attempt clipboard operations. Parser input may be fragmented at arbitrary byte boundaries.

### TB-03: Rust ↔ C FFI

Pointers, lengths, enum discriminants, opaque handles, buffer lifetimes, allocator ownership, and threading assumptions cross a language boundary. The boundary must be versioned and must reject invalid values before dereferencing or mutating state.

### TB-04: Configuration source → application state

Configuration is user-editable and may be corrupted, stale, malicious, or produced by a newer application. It must be versioned, strictly validated, and bounded before it can influence process launch or resource allocation.

### TB-05: Application → operating system

Process creation, PTY allocation, signals, filesystem access, environment inheritance, and child reaping are OS operations. Platform-specific code must remain behind runtime adapters and return typed errors.

### TB-06: Future plugin host → core application

Plugins are untrusted extensions by default. A future plugin must not inherit unrestricted process, filesystem, clipboard, network, or session access merely because it is loaded by the application.

## 4. Threat actors and assumptions

- **Malicious child process:** a program intentionally emits hostile terminal sequences, floods output, or attempts to interfere with another session.
- **Malicious local user content:** crafted configuration, paste payload, command arguments, file paths, or environment values.
- **Compromised external program:** a legitimate shell or tool that is compromised and executes attacker-controlled behavior under the user's account.
- **Malformed input source:** truncated UTF-8, malformed escape sequences, invalid FFI values, or unexpected PTY behavior.
- **Supply-chain attacker:** a compromised compiler, build action, dependency, or release artifact.
- **Future malicious plugin:** an extension attempting to exceed its declared capabilities.

Assumption: the OS kernel and the current user's account boundary are trusted. A same-UID child process can generally access files and resources available to that user. The terminal does not claim to contain such a child process without a separate OS sandbox.

## 5. Threat register (STRIDE)

| ID | Threat | STRIDE | Severity | Required controls | Current status |
| --- | --- | --- | --- | --- | --- |
| T-01 | Concatenated command string causes shell injection | Spoofing / Tampering / Elevation | Critical | Keep executable and argv separate; never call an implicit `sh -c`; validate at runtime boundary | ProgramSpec and runtime launch contracts implemented |
| T-02 | Malicious environment or working-directory values alter child behavior | Tampering / Elevation | High | Explicit key/value overrides; reject NUL and invalid keys; do not mutate host environment; avoid logging values | ProgramSpec validation and strict versioned application configuration schema are implemented in master |
| T-03 | Hostile OSC/CSI output triggers clipboard, title, hyperlink, or mode abuse | Spoofing / Tampering | High | Explicit allowlist for side-effecting sequences; clipboard read/write disabled by default; sanitize titles/URLs; bounded parser state | Parser exists; policy enforcement and adversarial tests remain required |
| T-04 | Parser receives fragmented, malformed, or oversized sequences | Denial of Service / Tampering | High | Incremental state machine; checked arithmetic; bounded sequence/string buffers; safe recovery from malformed input | Parser tests exist; fuzz coverage and resource budgets must be maintained |
| T-05 | FFI invalid pointer/length or stale handle causes memory unsafety | Tampering / Elevation / DoS | Critical | ABI version check; opaque handles; pointer/length validation; explicit ownership; no panic across ABI; no concurrent/reentrant handle use unless synchronized | ABI and wrapper contracts exist; adversarial boundary tests remain mandatory |
| T-06 | PTY output/input queues grow without bound | Denial of Service | High | Per-session bounded queues; backpressure; bounded scrollback; bounded parser and diagnostic buffers | Runtime and screen limits exist; session-manager limits under review |
| T-07 | Child process survives tab/application close | Elevation / DoS | High | Explicit process-group ownership; graceful then forced termination; kill-on-drop; wait/reap; cleanup on partial startup | Linux runtime kill-on-drop exists; session-manager integration under review |
| T-08 | One session receives another session's bytes or controls | Information Disclosure / Tampering | High | Unique non-reused SessionId; one PTY/process handle per session; route every event by owner ID | Contract defined; concrete manager under review |
| T-09 | Paste payload executes unintended control sequences or multiple commands | Tampering / Elevation | High | Bounded paste; control-character policy; explicit confirmation for multiline paste; no silent truncation | Paste/mouse/focus mapper under review |
| T-10 | Secrets leak through diagnostics or crash context | Information Disclosure | High | Redact environment values, tokens, passwords, and raw sensitive input; bounded structured diagnostics | Typed diagnostics model implemented; redaction policy must be enforced at every call site |
| T-11 | Configuration tampering changes launch or resource policy | Tampering / Elevation / DoS | High | Explicit schema version; unknown-key rejection; strict validation; safe defaults; atomic persistence and backup | Strict versioned schema validation is implemented in master; atomic persistence, backup, recovery, and migration remain unimplemented (#62) |
| T-12 | Path or symlink race redirects file/process operations | Tampering / Elevation | High | Validate path inputs; avoid privileged writes; use OS-safe open/exec APIs; define symlink policy for persistence | Runtime validation exists; persistence policy remains to be implemented |
| T-13 | Future plugin obtains ambient core privileges | Elevation / Information Disclosure | Critical | Capability-based permissions; deny by default; separate process boundary where practical; resource quotas; revocation | Plugin model not implemented; must be designed before plugin execution |
| T-14 | Dependency or build action compromise contaminates release | Tampering / Elevation | High | Pinned toolchain; lockfile review; minimal dependencies; pinned action revisions; artifact integrity checks and provenance | Rust toolchain pinned; dependency/release controls remain in release tasks |
| T-15 | Renderer consumes malformed dimensions or glyph data | Tampering / DoS | High | Validate dimensions and bitmap lengths; checked pixel arithmetic; bounded surfaces; safe fallback glyph | Headless CPU software renderer is implemented in master; glyph validation, checked surface arithmetic, pixel limits, clipping, and redraw regression tests are present. GPU/window integration and font discovery remain separate work. |

Severity is based on plausible impact to the user's account, terminal process execution, application integrity, and availability. It is not a claim that every listed threat is currently exploitable.

## 6. Mandatory security requirements

### SEC-001 — Safe process launch

- Executable and each argument remain separate values from configuration through OS spawn.
- Never interpolate user input into a command string for an implicit shell.
- Reject empty executable names, NUL bytes, invalid environment keys, and invalid working-directory values.
- Do not implicitly inherit or mutate the application process environment beyond the explicitly documented runtime policy.
- Do not log full arguments or environment values.

### SEC-002 — Terminal control-sequence policy

- Treat every PTY byte as untrusted.
- Parser state must survive arbitrary chunk boundaries and reject invalid lengths without panic or unbounded allocation.
- Side-effecting OSC features (clipboard, hyperlinks, title changes, notifications, file transfer) require explicit policy checks.
- Clipboard write/read must be disabled by default unless the user or a narrowly scoped policy enables it.
- Titles and URLs must be length-bounded and sanitized before passing to the GUI or opening external handlers.

### SEC-003 — Memory and resource bounds

- Bound terminal dimensions, screen cells, scrollback, parser buffers, escape-sequence payloads, per-session queues, paste payloads, arguments, environment entries, diagnostics, and session count.
- Use checked arithmetic for size calculations.
- Allocation failure returns a typed error; truncation is never silent.
- A slow or malicious session must not starve unrelated sessions or the GUI event loop.

### SEC-004 — FFI memory safety

- Negotiate ABI version before handle creation.
- Use opaque handles and fixed-width scalar types.
- Validate every pointer/length pair, enum discriminant, handle state, and capacity before access.
- Define which side allocates and frees every buffer.
- No Rust panic, C++ exception, or undefined behavior crosses the ABI.
- Define per-handle threading/reentrancy rules and test violations.

### SEC-005 — Process and session lifecycle

- Each session owns one process/PTY handle; handles are never shared.
- Child process groups are terminated on close according to bounded graceful/force policy.
- Every child is reaped; runtime-handle drop is a final kill-on-drop boundary.
- Partial startup failure cleans every acquired descriptor and child.
- Session IDs are never reused while stale events could still exist.

### SEC-006 — Configuration integrity

- Require explicit schema version.
- Reject unknown keys and unsupported versions.
- Validate every launch and resource field before use.
- Persistence must use restrictive file permissions, atomic replacement, and verified backup/restore behavior.
- Failed parse, validation, or migration must not overwrite the last known-good configuration.

### SEC-007 — Diagnostics and privacy

- Diagnostics identify subsystem, operation, stable error category, and safe context.
- Never log passwords, tokens, secret environment values, or raw clipboard payloads.
- Redaction must happen before a value enters a persistent log or crash report.
- Diagnostic buffers and retained history are bounded.

### SEC-008 — Plugin isolation (future gate)

- No plugin execution before a reviewed permission/capability model exists.
- Default capability set is empty.
- Filesystem, process launch, network, clipboard, and session access are separate permissions.
- Plugin resource use is bounded and revocable.
- A plugin must not receive raw internal pointers or unrestricted access to the session registry.

## 7. Security test strategy

| Area | Required adversarial tests |
| --- | --- |
| Process launch | Arguments containing spaces, quotes, shell metacharacters, empty strings, NUL rejection, invalid cwd, hostile environment keys |
| Lexer/parser | Unterminated quotes, trailing escape, nested groups, malformed operators, huge token streams, arbitrary chunk boundaries |
| Terminal parser | Split CSI/OSC sequences, invalid UTF-8, oversized OSC payloads, unknown private modes, reset/recovery behavior |
| Clipboard/paste | Multiline confirmation, ESC/NUL/DEL/control bytes, oversized payload, bracketed-paste mode transitions |
| FFI | Null pointer, invalid length, integer overflow, stale handle, ABI mismatch, unknown enum, concurrent/reentrant call |
| PTY/session | Spawn failure cleanup, partial writes, backpressure, EOF, child exit, signal escalation, close twice, two-session isolation |
| Configuration | Unknown keys, unsupported versions, negative/oversized values, invalid environment, corrupt input, failed migration |
| Renderer | Huge dimensions, malformed glyph bitmap, clipping overflow, stale frame view, allocation failure |
| Diagnostics | Secret redaction, oversized error, invalid UTF-8, repeated errors, cleanup after diagnostic failure |
| Supply chain | Pinned toolchain, lockfile diff review, action revision review, reproducible build and artifact hash verification |

Fuzz targets should be deterministic, bounded, and runnable in CI. A fuzz failure must become a minimized regression test.

## 8. Current implementation versus required controls

The following distinction prevents design intent from being mistaken for shipped protection:

- **Implemented in current master:** validated `ProgramSpec` values; separate executable/argument/environment fields; Rust/C ABI types and versioning foundation; terminal screen dimension limits; structured application diagnostics; Linux PTY/process runtime with kill-on-drop behavior; headless CPU software renderer with bounded surface/glyph validation and clipped redraw; paste/mouse/focus input mapping; strict versioned configuration schema; Linux platform capability reporting; pinned Rust toolchain and CI.
- **Under review in open PRs:** DE-Shell grammar/semantics specification (#126); session lifecycle manager and per-session I/O limits (#127); Linux process/environment hardening (#130, Draft); architecture decision records (#132).
- **Not yet implemented:** complete side-effecting OSC policy, full parser fuzz coverage, persistent configuration atomic-write/backup flow, plugin capability isolation, complete cross-session adversarial integration suite, release artifact provenance/signing.

An open PR is not a shipped mitigation. Release qualification must re-evaluate this table against the merged `master` branch.

## 9. Explicit non-goals and residual risk

- The terminal does not sandbox arbitrary external shells/programs. They execute with the user's OS privileges.
- The terminal does not protect against a compromised kernel, malicious OS account administrator, or malware already running as the same user.
- The initial target is Linux x86_64; Windows/macOS security properties require separate platform-specific review.
- A child process may read or modify any user-accessible file unless an OS sandbox is added.
- Clipboard integration, plugins, remote sessions, and network features are not authorized by this threat model; each requires a separate design review before implementation.

## 10. Change control

Any new process-launch path, FFI entry point, terminal side effect, plugin capability, persistent secret, or OS privilege must update this threat model and add tests in the same change. Security requirements may be tightened without a schema/API version increase; weakening or removing a control requires an explicit architecture/security decision and review.

Threat-model review is a release gate. A feature marked “under review” or “not yet implemented” must not be described as a shipped security guarantee.
