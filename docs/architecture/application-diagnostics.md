# Application Errors and Diagnostics

This document defines the implemented error and diagnostics boundary in `de-app`. It complements the cross-component contract and does not introduce a third-party logging dependency.

## Error propagation

`ApplicationError` contains:
- A stable `ErrorCategory` for routing and handling.
- A stable `ApplicationErrorCode` for machine-readable support references.
- A safe, user-facing summary.
- An optional source error retained for programmatic inspection.

The source chain is not included in `Display` or automatically serialized into diagnostic records. This prevents lower-level OS or library errors from accidentally leaking paths, arguments, environment values, or credentials into user-facing output.

`LifecycleError` converts into `ApplicationError` with stable lifecycle codes and safe summaries. The original lifecycle error remains available as the source.

## Diagnostic record

A `Diagnostic` records severity, category, code, subsystem, operation, safe message, and optional context fields. `as_record` produces one escaped line suitable for a logger; `write_to` writes that record to a standard writer.

Subsystem and operation fields must be stable identifiers such as `runtime`, `configuration`, `startup`, or `shutdown`. They must never contain raw user input.

## Secret handling

Context values are replaced with `[REDACTED]` when their key contains a sensitive marker, including:
- password / passwd
- token / secret
- credential / authorization
- cookie
- private_key
- api_key / access_key

The key itself remains visible so the diagnostic retains useful context. Context values under non-sensitive keys are sanitized and length-bounded.

Messages supplied to `ApplicationError::new` and `Diagnostic::new` must be safe summaries written by the caller. Never pass raw command input, full environment blocks, passwords, tokens, or untrusted OS error strings as a user-facing message. Attach detailed causes through `with_source`; source text is not serialized by the diagnostics API.

## Bounds and log safety

- Message text is capped at 1,024 characters.
- Context keys are capped at 128 characters.
- Context values are capped at 256 characters.
- Control characters are removed or normalized to spaces to prevent multiline log injection.
- Record values are quoted and escaped.
- `DiagnosticBuffer` is a bounded FIFO buffer. A full buffer evicts the oldest record; it never grows without limit.

The caller owns the buffer capacity and the output writer. The diagnostics module does not create files, open network connections, or persist logs implicitly.

## Verification

Integration tests in `crates/de-app/tests/diagnostics.rs` verify:
- Sensitive context redaction.
- Source error exclusion from user-facing and serialized records.
- Stable conversion from lifecycle errors.
- Single-line sanitization.
- Bounded diagnostic-buffer eviction.
- Safe writing to a standard writer.
