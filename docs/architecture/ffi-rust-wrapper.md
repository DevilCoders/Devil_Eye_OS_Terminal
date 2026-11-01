# Safe Rust DE-Shell FFI Wrapper

## Scope

DE-008-02 links the existing C DE-Shell implementation into the Rust workspace and exposes a safe, single-thread-affine Rust API. The C implementation remains authoritative for shell state, lexing, parsing, command dispatch, and execution. Rust owns the opaque handle wrapper and supplies an optional host process-execution callback.

## Native build integration

The de-ffi crate build script compiles the same C sources used by the CMake target into a static archive and links it into Rust:

- C17 compiler selected by CC (defaults to cc).
- Archiver selected by AR (defaults to ar).
- Strict warnings: Wall, Wextra, Wpedantic, Wconversion, Wsign-conversion, Wshadow, Wstrict-prototypes, and Werror.
- Target currently restricted to Linux x86_64.
- No third-party Cargo build dependency is required.
- CMake remains an independent build/test entry point; Cargo does not invoke CMake.

Both build paths compile the same C source files. If the source list changes, update CMakeLists.txt and crates/de-ffi/build.rs in the same change.

## Public Rust API

- Shell::new() negotiates ABI version 0.2.0 and creates an opaque C-owned shell.
- Shell::execute() executes one bounded source buffer with an explicit environment/working-directory context and optional CommandExecutor.
- Shell::close() destroys the C handle; repeated close calls are idempotent.
- Drop performs best-effort destruction.
- Shell is deliberately neither Send nor Sync. A mutable borrow serializes operations on one handle.

Environment values, working directory, executable, and arguments use borrowed CStr values. They are not assumed to be UTF-8. Argument boundaries are preserved; argv is never joined into a command string.

## External command callback

The C execution engine does not call system(), popen(), fork(), exec(), or an implicit command interpreter. It invokes the Rust-provided CommandExecutor synchronously for simple external commands and invokes CommandExecutor::execute_pipeline for typed pipeline/redirection plans.

ExternalCommand exposes:
- executable as a borrowed CStr;
- arguments excluding argv[0], each as a separate borrowed CStr;
- explicit environment key/value pairs;
- optional working directory.

The C request and all pointed-to strings are valid only until the callback returns. The callback must not retain them. The host runtime is responsible for actual process creation and should not block the GUI thread.

If no executor is supplied, external commands and pipeline/redirection plans return EXECUTION_UNAVAILABLE. Existing closure-based CommandExecutor implementations remain valid for single commands and fail closed for pipeline plans unless they override execute_pipeline.

ExternalPipeline exposes ordered stages, the top-level environment/working directory, and per-stage commands/redirections. Each ExternalPipelineStage contains an optional ExternalCommand (None only for a single redirection-only stage) and redirections in source order. ExternalRedirection exposes Input, Output, Append, ErrorOutput, or ErrorAppend, a borrowed target CStr, and the source offset. All borrowed strings are valid only for the callback duration. A host runtime should convert this typed plan to its native process API; it must never reparse a joined command string.

The C execution engine rejects built-ins inside pipeline/redirection plans until isolated child-context semantics are specified. A host executor failure is translated by the C engine to EXTERNAL_EXECUTION_FAILED; the diagnostic code preserves the callback's original status.

## Panic and error containment

The Rust callback bridge validates nullable pointers, bounded argument/environment counts, argv[0], the argv NULL sentinel, and non-empty environment keys before creating borrowed CStr views.

The callback is wrapped in catch_unwind:
- successful execution writes the process exit code and returns OK;
- an executor error returns EXTERNAL_EXECUTION_FAILED;
- a Rust panic is caught and returned to C as INTERNAL_ERROR.

The C execution engine converts any non-OK callback status to EXTERNAL_EXECUTION_FAILED and stores the original callback status in the diagnostic code. No Rust panic is allowed to unwind through C.

The wrapper preserves unknown status values through DeStatus::from_raw. Execution diagnostics preserve unknown stage values and map SIZE_MAX source offsets to None.

## Resource limits

- Source: 65,536 bytes.
- External argv entries: at most 256, including argv[0].
- Environment overrides: at most 256.
- Pipeline stages: at most 64.
- Redirections across one pipeline plan: at most 1,024.
- Environment keys must not be empty.
- All C-owned handles are destroyed by the C library; Rust never frees them directly.
- Callback pointers and temporary arrays remain alive for the entire synchronous C call only.

## Failure behavior

- ABI query failure returns AbiQueryFailed.
- Incompatible major/future minor ABI returns AbiMismatch.
- C status failures return Status or Execution with stage/code/source offset.
- Invalid environment context is rejected before crossing the ABI.
- Source exceeding the source limit is rejected before crossing the ABI.
- A closed handle returns Closed.
- A callback panic is contained and reported as an external execution failure with INTERNAL_ERROR in the diagnostic code.
- Drop never panics; explicit close is available when the caller needs to observe destruction errors.

## Cross-language verification

Rust tests link and execute the actual C library. They cover ABI layout/version negotiation, NULL argument rejection, repeated create/destroy cycles, closed-handle behavior, source limits, C lexer/parser error translation, missing executor behavior, argv/environment/working-directory forwarding, pipeline-stage and redirection decoding, redirection-only plans, host callback failure, and callback panic containment.

The CMake/CTest suite remains independently runnable and validates the native C subsystem without the Rust wrapper.
