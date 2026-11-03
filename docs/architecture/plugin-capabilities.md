# Plugin Capability and Permission Model

**Document:** DE-PLUGIN-CAP-001  
**Contract version:** 1.0 design baseline (not an implemented plugin ABI)  
**Capability catalog:** 1  
**Target:** Linux x86_64  
**Status:** Normative security gate; plugin execution remains disabled.

## 1. Purpose and hard gate

This contract specifies trust, manifest validation, capability grants, quotas, API versioning, and revocation for a future plugin host. It does not claim that a loader, broker, sandbox, installer, or runtime exists.

No plugin code may execute in the application process or a child process until package verification, a host-enforced isolation mechanism, the permission broker, quotas, revocation, and adversarial tests are implemented. A separate process alone is not a sandbox. A same-UID process generally retains access to user-readable resources unless OS controls deny that access.

## 2. Security invariants

1. **Deny by default:** absent, unknown, malformed, or ungranted capabilities are denied.
2. **No ambient authority:** no inherited application secrets, unrestricted environment, filesystem, process, network, clipboard, or session access.
3. **Broker every privileged operation:** no raw pointers, native handles, host service objects, or unrestricted session registry access cross the boundary.
4. **Least privilege:** grants bind to plugin ID, immutable package digest, publisher identity, host API major, capability, and normalized resource scope.
5. **Explicit consent:** a manifest request is not consent; unrelated capabilities cannot be bundled under one ambiguous trust toggle.
6. **Revocability:** deny new calls immediately, cancel in-flight work where possible, terminate when required, and invalidate stale grants.
7. **Host-enforced quotas:** plugins cannot self-report authoritative CPU, memory, process, descriptor, I/O, network, queue, or output usage.
8. **Fail closed:** signature, isolation, broker, API negotiation, or quota-accounting uncertainty prevents activation or denies the affected operation.
9. **Auditable without secrets:** record stable identity, capability, scope identifier, outcome, and reason; never log credentials, file/clipboard contents, or secret values.
10. **No silent privilege expansion:** digest, publisher, requested capability, scope, or incompatible API changes require fresh verification and consent.

## 3. Trust and package lifecycle

| State | Meaning | Permitted behavior |
| --- | --- | --- |
| Unknown | Not verified | Inspect bounded metadata only; never execute |
| Rejected | Signature, digest, policy, or metadata invalid | No activation |
| Verified-untrusted | Integrity verified; publisher not trusted | Display identity, digest, requested capabilities; no execution |
| Trusted | Publisher satisfies trust policy | Permission review only; trust grants no capabilities |
| Authorized | Exact capabilities/scopes consented | Activate only after isolation and quota gates pass |
| Revoked | Trust or grants withdrawn | Deny new operations and cancel/terminate |

Identify packages by a cryptographic digest of exact immutable package bytes. Verify integrity and publisher signature before activation; missing/invalid signatures are not silently downgraded. Never execute install hooks or plugin code during inspection or permission prompting. A changed package digest is a new authorization identity; grants do not transfer.

Publisher identity, signature algorithm, key distribution, trust roots, rotation, revocation lists, package format, and offline verification are deferred implementation decisions. They must be resolved before plugin distribution is enabled.

## 4. Manifest and API versioning

The manifest is declarative data; parsing it must never load plugin code.

Required logical fields: `manifest_version`, stable `plugin_id`, SemVer `version`, verified `publisher`, `package_digest`, `host_api` major/minor range, package-relative `entrypoint`, `requested_capabilities` with scopes, `resource_profile`, and bounded display metadata.

Reject unknown major versions/capabilities, duplicate keys or capabilities, invalid identifiers, absolute or traversal entrypoints, NULs, oversized fields, unsupported scopes, and resource requests above host ceilings. The manifest cannot mark itself trusted, grant permissions, raise quotas, disable audit, or select its own sandbox. Parsing must bound package size, nesting, strings, and collection counts; exact numeric limits must be selected and tested before a loader ships.

Host API version is independent of manifest and capability-catalog versions:
- Major versions are incompatible; reject unsupported majors.
- Minor versions are additive; optional methods/capabilities require explicit feature detection.
- Every request carries authenticated plugin-instance identity and authorization generation.
- Calls use typed bounded request/response values and stable errors. No native pointers, borrowed application objects, Rust trait objects, C++ objects, or raw OS handles cross the boundary.
- Negotiate API and authorize capabilities before activation.

## 5. Capability catalog

Capabilities are independent; granting one never implies another. Initial catalog version 1:

| Capability ID | User-visible scope | Default | Restrictions |
| --- | --- | --- | --- |
| `terminal.session.read` | Selected session IDs and data classes | Deny | No cross-session discovery; redact secrets |
| `terminal.session.write` | Selected session IDs and input mode | Deny | Separate from read; visible action; no hidden input |
| `terminal.ui.panel` | Named panel contribution | Deny | Declarative UI only; bounded rendering/events |
| `filesystem.read` | Explicit approved directory roots | Deny | Canonical paths; symlink policy; no implicit home/root |
| `filesystem.write` | Roots and create/modify/delete scope | Deny | Separate from read; validated atomic writes |
| `process.spawn` | Executable allowlist, argv policy, working directory | Deny | No implicit shell; no inherited secrets; bounded children |
| `network.connect` | Host/IP ranges, ports, protocol, direction | Deny | No listener/bind by implication; DNS/redirect policy |
| `clipboard.read` | Read access and data types | Deny | User-mediated; sensitive-content policy |
| `clipboard.write` | Write access and data types | Deny | Visible action; bounded payload; no monitoring |
| `configuration.read` | Named non-secret namespaces | Deny | Never expose credentials/secrets |
| `configuration.write` | Named namespaces and keys | Deny | Schema validated; cannot alter security policy/grants |

Unknown capability IDs are rejected, not ignored. Adding a capability requires threat-model update, user-facing scope, broker enforcement, audit event, revocation behavior, and adversarial tests.

### Consent and grant binding

Show verified identity, publisher, digest, version, exact normalized scope, read/write/execute nature, requested quota and enforced maximum, grant lifetime, denial effect, and revocation path. User must explicitly select capabilities. Until secure persistence is implemented, grants are session-scoped and discarded on application exit.

A grant binds to `plugin_id + package_digest + publisher_identity + host_api_major + capability_id + normalized_scope`. A different digest, publisher, major version, or broader scope invalidates it. Do not store grants in ad hoc files, plugin storage, plaintext logs, or the not-yet-selected configuration codec. Persistent grants depend on storage decision #62 and are not permitted yet.

## 6. Quotas and isolation

The host owns accounting and enforcement.

| Resource | Required policy |
| --- | --- |
| CPU | Per-plugin budget, deadline, throttle/terminate policy |
| Memory | Hard limit enforced by isolation boundary |
| Processes | Child count, executable policy, process-tree cleanup |
| Descriptors/handles | Hard ceiling; CLOEXEC; no ambient inheritance |
| Filesystem I/O | Per-operation size/rate and approved-root checks |
| Network | Destination policy, connections, payload size, rate, timeout |
| Host API | Per-method rate and concurrency limits |
| Queues/events | Bounded queues and explicit backpressure |
| Logs/output | Bounded bytes/rate; redact before retention |
| Lifetime | Startup, idle, execution, and shutdown deadlines |

Numeric defaults are deliberately not invented. Derive them from measured resource budgets and record them before enabling plugins. Missing quotas are a release blocker, never an unlimited default.

Plugin code must run outside the trusted application process. The selected Linux isolation mechanism must enforce filesystem, process, network, memory, CPU, and descriptor policy; a process boundary alone is insufficient. Do not pass the host environment, cwd, credentials, clipboard, session handles, or unrelated descriptors. Use authenticated, typed, bounded host IPC with replay/stale-generation rejection. If the required OS facility is unavailable or cannot be verified, activation fails closed. Sandbox technology and privilege setup remain explicit decisions; do not claim isolation until escape tests and deployment prerequisites are documented.

## 7. Authorization and revocation

Authorize every broker call at point of use:
1. Authenticate plugin instance and package digest.
2. Check active authorization generation.
3. Match exact capability.
4. Normalize resource and compare with granted scope.
5. Enforce per-call and aggregate quotas before side effects.
6. Dispatch through a host-owned adapter and return a bounded typed result.
7. Emit a safe audit event.

User revocation, package/publisher revocation, policy change, quota abuse, protocol violation, and shutdown invalidate the grant and increment its generation. Deny old-generation requests immediately. Cancel cancellable work; apply bounded deadlines to non-cancellable work; terminate the process tree if authority cannot be safely withdrawn. Close broker handles and release quotas exactly once. Repeated revocation is idempotent. Restart cannot restore stale authority; re-grant requires new consent and generation.

Stable error categories: `unknown_plugin`, `unverified_package`, `signature_invalid`, `api_incompatible`, `unknown_capability`, `capability_not_granted`, `scope_denied`, `quota_exceeded`, `authorization_revoked`, `stale_generation`, `isolation_unavailable`, `protocol_violation`, `host_unavailable`. Errors must not disclose secret paths, credentials, environment values, or clipboard contents.

## 8. Threat/mitigation matrix

| Threat | Required mitigation | Fail-closed response |
| --- | --- | --- |
| Hidden/unknown capability | Strict manifest/catalog validation | Reject activation |
| User approves overbroad scope | Human-readable normalized scope and least privilege | No grant until exact consent |
| Package changes after approval | Digest-bound grant and signature verification | Treat as new package |
| Plugin steals host authority | Enforced sandbox and broker-only API | Do not execute |
| Forged instance identity | Authenticated per-instance IPC and generation | Deny; terminate on violation |
| Resource flooding | Host quotas and bounded queues | Throttle, deny, terminate repeated abuse |
| Retained authority after revoke | Point-of-use checks, generation invalidation | Deny immediately |
| Filesystem escape | OS boundary plus canonical path checks | Deny and audit |
| Network exfiltration | Explicit destination/egress grant | Deny without grant |
| Session/input abuse | Separate read/write and session-scoped grants | Deny cross-session/ungranted access |
| Malformed broker payload | Typed bounded IPC and fuzzing | Reject frame; terminate instance |

## 9. Required future conformance tests

These are acceptance requirements, **not tests claimed as already run**:

- Manifest: unknown/missing schema, duplicate keys/capabilities, invalid ID, traversal, NUL, oversized fields, unknown capability.
- Trust: valid/invalid/missing signature, digest mismatch, revoked publisher, key rotation, package changed after consent.
- Consent: deny, selective grants, exact scope display, digest/API binding, no persistent grant before persistence.
- Broker: unknown/forged identity, stale generation, ungranted capability, broader path/host/session scope.
- Quotas: exact boundary, one over, concurrency, accounting overflow, exhausted memory/FD/process/network limits.
- Revocation: active/idle, race with request, cancellation, restart after revoke, repeated revoke.
- Isolation: no secret env/FD inheritance, filesystem escape, network denial, process-tree cleanup, sandbox unavailable.
- Failure: broker/plugin crash or hang, malformed IPC, host shutdown, partial startup, descriptor/process leak.
- Audit/privacy: stable allow/deny event, event flood, no secret/path/content leakage, log write failure.

Tests must exercise real OS isolation and broker boundaries; mocks do not prove enforcement. Fuzz manifest and IPC parsers with bounded deterministic corpora and turn every defect into a regression test.

## 10. Enablement gates and deferred decisions

Before plugin execution:
1. Select and threat-model Linux isolation, kernel prerequisites, and failure behavior.
2. Define package format, signature, publisher identity, trust-root distribution, rotation, and revocation.
3. Implement authenticated IPC, broker, capability enforcement, quota accounting, revocation, and process-tree cleanup.
4. Derive numeric quotas from measured budgets.
5. Decide persistent consent storage only after #62 selects the configuration codec; until then grants are session-only.
6. Add end-to-end adversarial tests and release qualification evidence.

No third-party plugin framework/runtime is selected by this design. Any dependency requires justification against isolation, supply-chain, portability, and maintenance requirements.

## 11. Compatibility and change control

This is host API contract 1.0 and capability catalog 1. Adding optional methods requires feature detection and an updated compatibility matrix. Weakening checks, widening defaults, changing scope semantics, or changing revocation guarantees is a security-breaking change requiring a new major contract version and security review. Capability IDs are stable and never silently repurposed.

Every implementation PR updates this contract and the threat model, includes adversarial tests, and identifies the exact enforced OS boundary. A design document, passing unit test, or process boundary alone never authorizes plugin execution.

## References

- [System Architecture](system-architecture.md)
- [Cross-Component API Contracts](api-contracts.md)
- [Security Threat Model](../security/THREAT_MODEL.md)
- [Engineering Dependency Map](../engineering/DEPENDENCY_MAP.md)
