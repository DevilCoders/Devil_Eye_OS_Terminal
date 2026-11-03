# Rust/C ABI Contract

## Scope

DE-008-01 freezes the binary contract shared by Rust and the C DE-Shell library. It defines fixed-width version/status values, compatibility checks, ownership rules, allocator boundaries, and thread-safety expectations. The safe Rust handle, execution wrapper, callback bridge, and native build integration are implemented by DE-008-02 (#50); see [Safe Rust DE-Shell FFI Wrapper](ffi-rust-wrapper.md).

## Public headers and Rust types

- C: `include/de_shell/abi.h`
- C shell lifecycle: `include/de_shell/shell.h`
- Rust: `de_ffi::DeAbiVersion`, `de_ffi::DeStatus`, and `de_ffi::ABI_VERSION`

### ABI version tuple

`de_shell_abi_version_t` and `DeAbiVersion` have identical layout:

| Field | Type | Offset |
| --- | --- | ---: |
| major | uint16_t / u16 | 0 |
| minor | uint16_t / u16 | 2 |
| patch | uint16_t / u16 | 4 |

Required layout: size 6 bytes, alignment 2 bytes. C static assertions and Rust layout tests enforce field offsets, size, and alignment.

Current version: **0.2.0**. Minor version 0.2 appends the pipeline-stage pointer/count to the external-command request and introduces fixed-layout pipeline-stage and redirection descriptors. Rust 0.2 rejects a 0.1 native library before using the expanded callback layout; 0.1 callers remain compatible with 0.2 for the original single-command path.

### Status values

The C ABI status type is `int32_t`; Rust uses a `#[repr(transparent)]` newtype over `i32`. A Rust enum is deliberately not used because a future/unknown C integer must remain representable without constructing an invalid Rust enum discriminant.

| Value | C constant | Rust constant | Meaning |
| ---: | --- | --- | --- |
| 0 | `DE_SHELL_STATUS_OK` | `DeStatus::OK` | Success |
| 1 | `DE_SHELL_STATUS_INVALID_ARGUMENT` | `DeStatus::INVALID_ARGUMENT` | Invalid input |
| 2 | `DE_SHELL_STATUS_INTERNAL_ERROR` | `DeStatus::INTERNAL_ERROR` | Internal failure |
| 3 | `DE_SHELL_STATUS_BUSY` | `DeStatus::BUSY` | Handle is executing or otherwise busy |
| 4 | `DE_SHELL_STATUS_PARSE_ERROR` | `DeStatus::PARSE_ERROR` | Lexing/parsing failure |
| 5 | `DE_SHELL_STATUS_RESOURCE_LIMIT` | `DeStatus::RESOURCE_LIMIT` | Configured bound exceeded |
| 6 | `DE_SHELL_STATUS_UNSUPPORTED_OPERATION` | `DeStatus::UNSUPPORTED_OPERATION` | Operation not implemented by this backend |
| 7 | `DE_SHELL_STATUS_EXECUTION_UNAVAILABLE` | `DeStatus::EXECUTION_UNAVAILABLE` | No external executor is configured |
| 8 | `DE_SHELL_STATUS_EXTERNAL_EXECUTION_FAILED` | `DeStatus::EXTERNAL_EXECUTION_FAILED` | Host executor failed |
| 9 | `DE_SHELL_STATUS_ABI_MISMATCH` | `DeStatus::ABI_MISMATCH` | Requested ABI is incompatible |

Existing numeric assignments are immutable and never reused. New values are appended. Unknown values are preserved by Rust's `DeStatus::from_raw` and mapped to a generic safe error by higher-level wrappers.

## Version negotiation

Call `de_shell_get_abi_version` before creating a shell handle. It writes the library version to caller-owned storage and performs no allocation.

Call `de_shell_check_abi_version` with the caller's compiled version before using version-dependent functions. Compatibility policy:

- Major versions must match.
- A library minor version greater than or equal to the requested minor version is compatible.
- Patch differences are compatible.
- A requested future minor or different major returns `DE_SHELL_STATUS_ABI_MISMATCH`.
- NULL version pointers return `DE_SHELL_STATUS_INVALID_ARGUMENT`.

The Rust `DeAbiVersion::is_compatible_with` method mirrors the C policy. Feature discovery remains explicit; callers must not infer the availability of optional functions solely from a patch version.

## Ownership and allocator boundaries

- `de_shell_t` is an opaque C-owned handle.
- Create/destroy operations must be performed by the C library; Rust must never free the handle with Rust's allocator or inspect its layout.
- C allocations are released only by matching C destroy functions. Rust allocations are released only by Rust.
- ABI structs contain only fixed-width scalar fields and do not own heap memory.
- No Rust `String`, `Vec`, reference, slice, trait object, or allocator-owned buffer crosses the ABI.
- Pointer/length pairs are validated by the receiving side. Borrowed pointers are valid only for the duration stated by the function contract.
- Output structs are written into caller-provided storage. Version negotiation allocates no memory.
- Callback/user-data pointers are borrowed and must not be retained unless a future API explicitly defines registration and teardown semantics.

## Threading and reentrancy

- ABI version query/check functions are stateless and thread-safe.
- Each shell handle is single-thread-affine.
- Concurrent or reentrant operations on one handle are forbidden unless a future major ABI explicitly adds synchronization.
- Destroying a handle while execution is active returns `BUSY` and leaves the handle valid.
- No Rust panic, C++ exception, or non-local jump may cross the C ABI boundary.
- FFI wrappers must catch Rust panics internally and translate them to stable status values where unwinding is enabled; C code must return statuses rather than terminate the host process.

## Compatibility policy

- ABI layout changes, ownership changes, threading changes, or incompatible semantic changes require a major version increment.
- Additive optional functions and appended status values may use a minor version increment only when old callers remain safe.
- Compatible bug fixes may use a patch increment.
- Existing struct field order, field widths, status numeric values, and allocator ownership never change within a major version.
- Version negotiation must complete before a shell handle is created.

## Verification

C tests use `_Static_assert` to verify status width, ABI version size/alignment/field offsets, and the x86_64 layouts of the extended external-command, pipeline-stage, and redirection descriptors. Runtime C tests verify current version reporting, NULL handling, older minor compatibility, patch compatibility, rejection of future minor/major versions, and C-owned handle creation/destruction with idempotent NULL cleanup. Rust tests verify the same layout, status numeric assignments, unknown-status preservation, and version compatibility semantics.

The C and Rust tests intentionally verify the same constants independently; a mismatch fails CI instead of silently crossing the language boundary.
