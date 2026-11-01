//! Safe Rust wrapper around the versioned DE-Shell C ABI.
//!
//! ABI-safe values are public. Raw pointers, C function declarations, and the
//! panic-contained callback bridge are isolated in the private raw module.

use std::error::Error;
use std::ffi::{c_void, CStr};
use std::fmt;
use std::marker::PhantomData;
use std::ptr::{null, null_mut, NonNull};
use std::rc::Rc;

#[allow(unsafe_code)]
mod raw;

/// Current Rust/C interface version.
pub const ABI_VERSION: DeAbiVersion = DeAbiVersion {
    major: 0,
    minor: 2,
    patch: 0,
};

/// Maximum source bytes accepted by the DE-Shell execution API.
pub const MAX_SOURCE_BYTES: usize = 65_536;

/// Version tuple shared with C through a fixed C-compatible memory layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeAbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl DeAbiVersion {
    /// Returns whether a library version can serve this requested version.
    pub const fn is_compatible_with(self, library: Self) -> bool {
        self.major == library.major && self.minor <= library.minor
    }
}

/// A fixed-width status value returned by the C ABI.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeStatus(i32);

impl DeStatus {
    pub const OK: Self = Self(0);
    pub const INVALID_ARGUMENT: Self = Self(1);
    pub const INTERNAL_ERROR: Self = Self(2);
    pub const BUSY: Self = Self(3);
    pub const PARSE_ERROR: Self = Self(4);
    pub const RESOURCE_LIMIT: Self = Self(5);
    pub const UNSUPPORTED_OPERATION: Self = Self(6);
    pub const EXECUTION_UNAVAILABLE: Self = Self(7);
    pub const EXTERNAL_EXECUTION_FAILED: Self = Self(8);
    pub const ABI_MISMATCH: Self = Self(9);

    /// Wraps a raw status returned by C, including unknown future values.
    pub const fn from_raw(value: i32) -> Self {
        Self(value)
    }

    /// Returns the exact signed 32-bit status value.
    pub const fn as_raw(self) -> i32 {
        self.0
    }

    pub const fn is_ok(self) -> bool {
        self.0 == Self::OK.0
    }
}

/// One explicit environment override passed to a shell execution call.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentEntry<'a> {
    pub key: &'a CStr,
    pub value: &'a CStr,
}

/// Immutable environment and working-directory context for one execution.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionContext<'a> {
    pub environment: &'a [EnvironmentEntry<'a>],
    pub working_directory: Option<&'a CStr>,
}

impl<'a> ExecutionContext<'a> {
    pub const fn new() -> Self {
        Self {
            environment: &[],
            working_directory: None,
        }
    }
}

impl Default for ExecutionContext<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// A borrowed external process request decoded from the C callback.
#[derive(Debug)]
pub struct ExternalCommand<'a> {
    executable: &'a CStr,
    arguments: Vec<&'a CStr>,
    environment: Vec<EnvironmentEntry<'a>>,
    working_directory: Option<&'a CStr>,
}

impl<'a> ExternalCommand<'a> {
    pub const fn executable(&self) -> &'a CStr {
        self.executable
    }

    pub fn arguments(&self) -> &[&'a CStr] {
        &self.arguments
    }

    pub fn environment(&self) -> &[EnvironmentEntry<'a>] {
        &self.environment
    }

    pub const fn working_directory(&self) -> Option<&'a CStr> {
        self.working_directory
    }
}

/// Kind of explicit file redirection in a pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalRedirectionKind {
    Input,
    Output,
    Append,
}

/// Borrowed redirection target and its source location.
#[derive(Debug)]
pub struct ExternalRedirection<'a> {
    kind: ExternalRedirectionKind,
    target: &'a CStr,
    source_offset: usize,
}

impl ExternalRedirection<'_> {
    pub const fn kind(&self) -> ExternalRedirectionKind {
        self.kind
    }

    pub const fn target(&self) -> &CStr {
        self.target
    }

    pub const fn source_offset(&self) -> usize {
        self.source_offset
    }
}

/// One borrowed stage in a pipeline. A missing command denotes a
/// redirection-only operation and is valid only in a one-stage plan.
#[derive(Debug)]
pub struct ExternalPipelineStage<'a> {
    command: Option<ExternalCommand<'a>>,
    redirections: Vec<ExternalRedirection<'a>>,
}

impl<'a> ExternalPipelineStage<'a> {
    pub const fn command(&self) -> Option<&ExternalCommand<'a>> {
        self.command.as_ref()
    }

    pub fn redirections(&self) -> &[ExternalRedirection<'a>] {
        &self.redirections
    }
}

/// Complete ordered pipeline request. All strings are borrowed only for the
/// duration of the synchronous CommandExecutor callback.
#[derive(Debug)]
pub struct ExternalPipeline<'a> {
    stages: Vec<ExternalPipelineStage<'a>>,
    environment: Vec<EnvironmentEntry<'a>>,
    working_directory: Option<&'a CStr>,
}

impl<'a> ExternalPipeline<'a> {
    pub fn stages(&self) -> &[ExternalPipelineStage<'a>] {
        &self.stages
    }

    pub fn environment(&self) -> &[EnvironmentEntry<'a>] {
        &self.environment
    }

    pub const fn working_directory(&self) -> Option<&'a CStr> {
        self.working_directory
    }
}

/// A host executor handles external commands. The callback is synchronous;
/// every C string in the request is borrowed only for the duration of execute.
pub trait CommandExecutor {
    fn execute(&mut self, command: ExternalCommand<'_>) -> Result<i32, ExternalExecutionError>;

    /// Handles a complete pipeline or redirection-only request. Existing
    /// single-command executors remain source-compatible and fail closed for
    /// plans they do not explicitly implement.
    fn execute_pipeline(
        &mut self,
        _pipeline: ExternalPipeline<'_>,
    ) -> Result<i32, ExternalExecutionError> {
        Err(ExternalExecutionError::Failed)
    }
}

impl<F> CommandExecutor for F
where
    F: for<'a> FnMut(ExternalCommand<'a>) -> Result<i32, ExternalExecutionError>,
{
    fn execute(&mut self, command: ExternalCommand<'_>) -> Result<i32, ExternalExecutionError> {
        self(command)
    }
}

/// Failure reported by a host-provided external command executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalExecutionError {
    Failed,
}

struct CallbackContext<'a> {
    pub executor: &'a mut dyn CommandExecutor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStage {
    None,
    Input,
    Lex,
    Parse,
    Dispatch,
    External,
    Expand,
    Redirect,
    Pipeline,
    Unknown(i32),
}

impl ExecutionStage {
    fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Input,
            2 => Self::Lex,
            3 => Self::Parse,
            4 => Self::Dispatch,
            5 => Self::External,
            6 => Self::Expand,
            7 => Self::Redirect,
            8 => Self::Pipeline,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionDiagnostic {
    pub stage: ExecutionStage,
    pub code: i32,
    /// None means that the C API did not associate the error with source text.
    pub source_offset: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionResult {
    pub exit_code: i32,
    pub exit_requested: bool,
    pub commands_executed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellError {
    AbiQueryFailed(DeStatus),
    AbiMismatch(DeAbiVersion),
    Status(DeStatus),
    Execution {
        status: DeStatus,
        diagnostic: ExecutionDiagnostic,
    },
    Closed,
    SourceTooLarge,
    TooManyEnvironmentEntries,
    EmptyEnvironmentKey,
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbiQueryFailed(status) => {
                write!(
                    formatter,
                    "DE-Shell ABI version query failed: {}",
                    status.as_raw()
                )
            }
            Self::AbiMismatch(version) => write!(
                formatter,
                "DE-Shell ABI is incompatible: {}.{}.{}",
                version.major, version.minor, version.patch
            ),
            Self::Status(status) => {
                write!(formatter, "DE-Shell returned status {}", status.as_raw())
            }
            Self::Execution { status, diagnostic } => write!(
                formatter,
                "DE-Shell execution failed with status {} at {:?} (code {}, offset {:?})",
                status.as_raw(),
                diagnostic.stage,
                diagnostic.code,
                diagnostic.source_offset
            ),
            Self::Closed => formatter.write_str("DE-Shell handle is already closed"),
            Self::SourceTooLarge => {
                formatter.write_str("DE-Shell source exceeds the 65536-byte limit")
            }
            Self::TooManyEnvironmentEntries => {
                formatter.write_str("execution context exceeds the environment-entry limit")
            }
            Self::EmptyEnvironmentKey => formatter.write_str("environment keys must not be empty"),
        }
    }
}

impl Error for ShellError {}

/// Owned, single-thread-affine DE-Shell handle.
///
/// The handle is neither Send nor Sync. Mutable borrowing serializes execution
/// and prevents safe Rust callers from using the same C handle concurrently.
pub struct Shell {
    raw: Option<NonNull<raw::DeShellOpaque>>,
    _thread_affinity: PhantomData<Rc<()>>,
}

impl Shell {
    /// Checks the native ABI version and creates one independent shell handle.
    pub fn new() -> Result<Self, ShellError> {
        let mut raw_version = raw::RawAbiVersion::default();
        // SAFETY: raw_version is valid writable storage with the C ABI layout.
        let query_status = DeStatus::from_raw(raw::query_abi_version(&mut raw_version));
        if !query_status.is_ok() {
            return Err(ShellError::AbiQueryFailed(query_status));
        }

        let library_version = DeAbiVersion {
            major: raw_version.major,
            minor: raw_version.minor,
            patch: raw_version.patch,
        };
        if !ABI_VERSION.is_compatible_with(library_version) {
            return Err(ShellError::AbiMismatch(library_version));
        }

        let requested = raw::RawAbiVersion {
            major: ABI_VERSION.major,
            minor: ABI_VERSION.minor,
            patch: ABI_VERSION.patch,
        };
        // SAFETY: requested points to a valid ABI-compatible version tuple.
        let check_status = DeStatus::from_raw(raw::check_abi_version(&requested));
        if !check_status.is_ok() {
            return Err(ShellError::AbiMismatch(library_version));
        }

        let mut handle = null_mut();
        // SAFETY: handle is valid writable pointer storage.
        let create_status = DeStatus::from_raw(raw::create_shell(&mut handle));
        if !create_status.is_ok() {
            return Err(ShellError::Status(create_status));
        }

        let raw = NonNull::new(handle).ok_or(ShellError::Status(DeStatus::INTERNAL_ERROR))?;
        Ok(Self {
            raw: Some(raw),
            _thread_affinity: PhantomData,
        })
    }

    /// Executes one bounded source buffer. External commands are delegated to
    /// the optional host executor; no command string is passed to an implicit
    /// system shell by this wrapper.
    pub fn execute(
        &mut self,
        source: &[u8],
        context: &ExecutionContext<'_>,
        executor: Option<&mut dyn CommandExecutor>,
    ) -> Result<ExecutionResult, ShellError> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(ShellError::SourceTooLarge);
        }
        if context.environment.len() > raw::MAX_ENVIRONMENT_ENTRIES {
            return Err(ShellError::TooManyEnvironmentEntries);
        }
        if context
            .environment
            .iter()
            .any(|entry| entry.key.to_bytes().is_empty())
        {
            return Err(ShellError::EmptyEnvironmentKey);
        }

        let raw_environment: Vec<raw::RawEnvironmentEntry> = context
            .environment
            .iter()
            .map(|entry| raw::RawEnvironmentEntry {
                key: entry.key.as_ptr(),
                value: entry.value.as_ptr(),
            })
            .collect();

        let mut callback_context = executor.map(|executor| CallbackContext { executor });
        let external_executor = callback_context
            .as_ref()
            .map(|_| raw::external_executor_callback as raw::RawExternalExecutor);
        let user_data = callback_context.as_mut().map_or(null_mut(), |callback| {
            (callback as *mut CallbackContext<'_>).cast::<c_void>()
        });

        let raw_context = raw::RawExecutionContext {
            environment: if raw_environment.is_empty() {
                null()
            } else {
                raw_environment.as_ptr()
            },
            environment_count: raw_environment.len(),
            working_directory: context.working_directory.map_or(null(), CStr::as_ptr),
            external_executor,
            user_data,
        };

        let raw_shell = self.raw.ok_or(ShellError::Closed)?;
        let mut raw_result = raw::RawExecutionResult::default();
        let mut raw_diagnostic = raw::RawExecutionDiagnostic::default();

        // SAFETY: all pointers in raw_context and source remain valid for this
        // synchronous call; the shell handle is exclusively borrowed.
        let status = DeStatus::from_raw(raw::execute_source(
            raw_shell.as_ptr(),
            source,
            &raw_context,
            &mut raw_result,
            &mut raw_diagnostic,
        ));

        let diagnostic = ExecutionDiagnostic {
            stage: ExecutionStage::from_raw(raw_diagnostic.stage),
            code: raw_diagnostic.code,
            source_offset: (raw_diagnostic.source_offset != usize::MAX)
                .then_some(raw_diagnostic.source_offset),
        };
        if !status.is_ok() {
            return Err(ShellError::Execution { status, diagnostic });
        }

        Ok(ExecutionResult {
            exit_code: raw_result.exit_code,
            exit_requested: raw_result.exit_requested != 0,
            commands_executed: raw_result.commands_executed,
        })
    }

    /// Explicitly destroys the native handle. Repeated close calls are safe.
    pub fn close(&mut self) -> Result<(), ShellError> {
        let Some(raw_shell) = self.raw else {
            return Ok(());
        };

        let mut handle = raw_shell.as_ptr();
        // SAFETY: handle is a valid owned C handle passed by address.
        let status = DeStatus::from_raw(raw::destroy_shell(&mut handle));
        if !status.is_ok() {
            return Err(ShellError::Status(status));
        }
        if !handle.is_null() {
            return Err(ShellError::Status(DeStatus::INTERNAL_ERROR));
        }

        self.raw = None;
        Ok(())
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DeAbiVersion, DeStatus, EnvironmentEntry, ExecutionContext, ExecutionStage,
        CommandExecutor, ExternalCommand, ExternalExecutionError, ExternalPipeline,
        ExternalRedirectionKind, Shell, ShellError, ABI_VERSION, MAX_SOURCE_BYTES,
    };
    use std::ffi::CString;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn exposes_expected_abi_version() {
        assert_eq!(
            ABI_VERSION,
            DeAbiVersion {
                major: 0,
                minor: 2,
                patch: 0,
            }
        );
    }

    #[test]
    fn abi_version_has_fixed_c_layout() {
        assert_eq!(size_of::<DeAbiVersion>(), 6);
        assert_eq!(align_of::<DeAbiVersion>(), 2);
        assert_eq!(offset_of!(DeAbiVersion, major), 0);
        assert_eq!(offset_of!(DeAbiVersion, minor), 2);
        assert_eq!(offset_of!(DeAbiVersion, patch), 4);
    }

    #[test]
    fn abi_compatibility_accepts_older_minor_and_patch_versions() {
        let library = ABI_VERSION;
        assert!(DeAbiVersion {
            major: 0,
            minor: 0,
            patch: 99,
        }
        .is_compatible_with(library));
        assert!(DeAbiVersion {
            major: 0,
            minor: 1,
            patch: 99,
        }
        .is_compatible_with(library));
        assert!(DeAbiVersion {
            major: 0,
            minor: 2,
            patch: 99,
        }
        .is_compatible_with(library));
    }

    #[test]
    fn abi_compatibility_rejects_major_or_future_minor_versions() {
        assert!(!DeAbiVersion {
            major: 1,
            minor: 0,
            patch: 0,
        }
        .is_compatible_with(ABI_VERSION));
        assert!(!DeAbiVersion {
            major: 0,
            minor: 3,
            patch: 0,
        }
        .is_compatible_with(ABI_VERSION));
    }

    #[test]
    fn status_has_fixed_i32_layout_and_stable_values() {
        assert_eq!(size_of::<DeStatus>(), 4);
        assert_eq!(align_of::<DeStatus>(), 4);
        assert_eq!(DeStatus::OK.as_raw(), 0);
        assert_eq!(DeStatus::INVALID_ARGUMENT.as_raw(), 1);
        assert_eq!(DeStatus::INTERNAL_ERROR.as_raw(), 2);
        assert_eq!(DeStatus::BUSY.as_raw(), 3);
        assert_eq!(DeStatus::PARSE_ERROR.as_raw(), 4);
        assert_eq!(DeStatus::RESOURCE_LIMIT.as_raw(), 5);
        assert_eq!(DeStatus::UNSUPPORTED_OPERATION.as_raw(), 6);
        assert_eq!(DeStatus::EXECUTION_UNAVAILABLE.as_raw(), 7);
        assert_eq!(DeStatus::EXTERNAL_EXECUTION_FAILED.as_raw(), 8);
        assert_eq!(DeStatus::ABI_MISMATCH.as_raw(), 9);
        assert!(DeStatus::OK.is_ok());
        assert!(!DeStatus::INTERNAL_ERROR.is_ok());
    }

    #[test]
    fn unknown_status_values_remain_representable() {
        let unknown = DeStatus::from_raw(0x1234);
        assert_eq!(unknown.as_raw(), 0x1234);
        assert!(!unknown.is_ok());
    }

    #[test]
    fn execution_abi_structs_have_expected_linux_x86_64_layout() {
        use super::raw::{
            RawEnvironmentEntry, RawExecutionContext, RawExecutionDiagnostic, RawExecutionResult,
            RawExternalCommand,
        };

        assert_eq!(size_of::<RawEnvironmentEntry>(), 16);
        assert_eq!(align_of::<RawEnvironmentEntry>(), 8);
        assert_eq!(offset_of!(RawEnvironmentEntry, key), 0);
        assert_eq!(offset_of!(RawEnvironmentEntry, value), 8);

        assert_eq!(size_of::<RawExternalCommand>(), 64);
        assert_eq!(align_of::<RawExternalCommand>(), 8);
        assert_eq!(offset_of!(RawExternalCommand, executable), 0);
        assert_eq!(offset_of!(RawExternalCommand, argc), 8);
        assert_eq!(offset_of!(RawExternalCommand, argv), 16);
        assert_eq!(offset_of!(RawExternalCommand, environment), 24);
        assert_eq!(offset_of!(RawExternalCommand, environment_count), 32);
        assert_eq!(offset_of!(RawExternalCommand, working_directory), 40);
        assert_eq!(offset_of!(RawExternalCommand, pipeline_stages), 48);
        assert_eq!(offset_of!(RawExternalCommand, pipeline_stage_count), 56);

        assert_eq!(size_of::<super::raw::RawIoRedirection>(), 24);
        assert_eq!(align_of::<super::raw::RawIoRedirection>(), 8);
        assert_eq!(offset_of!(super::raw::RawIoRedirection, kind), 0);
        assert_eq!(offset_of!(super::raw::RawIoRedirection, target), 8);
        assert_eq!(offset_of!(super::raw::RawIoRedirection, source_offset), 16);

        assert_eq!(size_of::<super::raw::RawPipelineStage>(), 24);
        assert_eq!(align_of::<super::raw::RawPipelineStage>(), 8);
        assert_eq!(offset_of!(super::raw::RawPipelineStage, command), 0);
        assert_eq!(offset_of!(super::raw::RawPipelineStage, redirections), 8);
        assert_eq!(offset_of!(super::raw::RawPipelineStage, redirection_count), 16);

        assert_eq!(size_of::<RawExecutionContext>(), 40);
        assert_eq!(align_of::<RawExecutionContext>(), 8);
        assert_eq!(offset_of!(RawExecutionContext, environment), 0);
        assert_eq!(offset_of!(RawExecutionContext, environment_count), 8);
        assert_eq!(offset_of!(RawExecutionContext, working_directory), 16);
        assert_eq!(offset_of!(RawExecutionContext, external_executor), 24);
        assert_eq!(offset_of!(RawExecutionContext, user_data), 32);

        assert_eq!(size_of::<RawExecutionResult>(), 16);
        assert_eq!(align_of::<RawExecutionResult>(), 8);
        assert_eq!(offset_of!(RawExecutionResult, exit_code), 0);
        assert_eq!(offset_of!(RawExecutionResult, exit_requested), 4);
        assert_eq!(offset_of!(RawExecutionResult, commands_executed), 8);

        assert_eq!(size_of::<RawExecutionDiagnostic>(), 16);
        assert_eq!(align_of::<RawExecutionDiagnostic>(), 8);
        assert_eq!(offset_of!(RawExecutionDiagnostic, stage), 0);
        assert_eq!(offset_of!(RawExecutionDiagnostic, code), 4);
        assert_eq!(offset_of!(RawExecutionDiagnostic, source_offset), 8);
    }

    #[test]
    fn c_library_rejects_incompatible_abi_version() {
        let incompatible = super::raw::RawAbiVersion {
            major: 99,
            minor: 0,
            patch: 0,
        };
        assert_eq!(
            super::raw::check_abi_version(&incompatible),
            DeStatus::ABI_MISMATCH.as_raw()
        );
    }

    #[test]
    fn executes_builtin_exit_and_reports_exit_request() {
        let mut shell = Shell::new().expect("create shell");
        let result = shell
            .execute(b"exit", &ExecutionContext::new(), None)
            .expect("execute built-in");
        assert!(result.exit_requested);
        assert_eq!(result.commands_executed, 1);
    }

    #[test]
    fn rejects_empty_environment_keys_before_crossing_abi() {
        let mut shell = Shell::new().expect("create shell");
        let key = CString::new("").expect("empty key");
        let value = CString::new("value").expect("value");
        let environment = [EnvironmentEntry {
            key: key.as_c_str(),
            value: value.as_c_str(),
        }];
        let context = ExecutionContext {
            environment: &environment,
            working_directory: None,
        };
        assert_eq!(
            shell.execute(b"exit", &context, None),
            Err(ShellError::EmptyEnvironmentKey)
        );
    }

    #[test]
    fn c_api_rejects_null_create_output() {
        let status = super::raw::create_shell_with_null_output();
        assert_eq!(status, DeStatus::INVALID_ARGUMENT.as_raw());
    }

    #[test]
    fn creates_and_destroys_repeated_shell_handles() {
        for _ in 0..64 {
            let mut shell = Shell::new().expect("create C-owned shell");
            shell.close().expect("destroy C-owned shell");
            shell.close().expect("idempotent close");
            assert!(matches!(
                shell.execute(b"exit", &ExecutionContext::new(), None),
                Err(ShellError::Closed)
            ));
        }
    }

    #[test]
    fn translates_parser_errors_and_source_offsets() {
        let mut shell = Shell::new().expect("create shell");
        let error = shell
            .execute(b"echo 'unterminated", &ExecutionContext::new(), None)
            .expect_err("malformed source must fail");
        let ShellError::Execution { status, diagnostic } = error else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(status, DeStatus::PARSE_ERROR);
        assert_eq!(diagnostic.stage, ExecutionStage::Lex);
        assert!(diagnostic.source_offset.is_some());
    }

    #[test]
    fn rejects_oversized_source_before_crossing_abi() {
        let mut shell = Shell::new().expect("create shell");
        let source = vec![b'x'; MAX_SOURCE_BYTES + 1];
        assert_eq!(
            shell.execute(&source, &ExecutionContext::new(), None),
            Err(ShellError::SourceTooLarge)
        );
    }

    #[test]
    fn reports_missing_external_executor() {
        let mut shell = Shell::new().expect("create shell");
        let error = shell
            .execute(b"unknown_external_command", &ExecutionContext::new(), None)
            .expect_err("no host executor is installed");
        let ShellError::Execution { status, .. } = error else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(status, DeStatus::EXECUTION_UNAVAILABLE);
    }

    #[test]
    fn bridges_external_command_arguments_environment_and_working_directory() {
        let mut shell = Shell::new().expect("create shell");
        let key = CString::new("DE_FFI_TEST").expect("key");
        let value = CString::new("bridge-ok").expect("value");
        let cwd = CString::new("/tmp").expect("cwd");
        let environment = [EnvironmentEntry {
            key: key.as_c_str(),
            value: value.as_c_str(),
        }];
        let context = ExecutionContext {
            environment: &environment,
            working_directory: Some(cwd.as_c_str()),
        };
        let mut observed = false;
        let mut executor = |command: ExternalCommand<'_>| {
            assert_eq!(command.executable().to_bytes(), b"unknown_tool");
            assert_eq!(
                command
                    .arguments()
                    .iter()
                    .map(|argument| argument.to_bytes())
                    .collect::<Vec<_>>(),
                vec![b"alpha".as_slice(), b"beta".as_slice()]
            );
            assert_eq!(command.environment().len(), 1);
            assert_eq!(command.environment()[0].key.to_bytes(), b"DE_FFI_TEST");
            assert_eq!(command.environment()[0].value.to_bytes(), b"bridge-ok");
            assert_eq!(
                command.working_directory().expect("cwd").to_bytes(),
                b"/tmp"
            );
            observed = true;
            Ok(7)
        };
        let result = shell
            .execute(b"unknown_tool alpha beta", &context, Some(&mut executor))
            .expect("external callback succeeds");
        assert!(observed);
        assert_eq!(result.exit_code, 7);
        assert_eq!(result.commands_executed, 1);
    }


    struct PipelinePlanExecutor {
        calls: usize,
        redirection_only: bool,
    }

    impl CommandExecutor for PipelinePlanExecutor {
        fn execute(
            &mut self,
            _command: ExternalCommand<'_>,
        ) -> Result<i32, ExternalExecutionError> {
            Err(ExternalExecutionError::Failed)
        }

        fn execute_pipeline(
            &mut self,
            pipeline: ExternalPipeline<'_>,
        ) -> Result<i32, ExternalExecutionError> {
            self.calls += 1;
            if self.redirection_only {
                assert_eq!(pipeline.stages().len(), 1usize);
                let stage = &pipeline.stages()[0];
                assert!(stage.command().is_none());
                assert_eq!(stage.redirections().len(), 1usize);
                assert_eq!(
                    stage.redirections()[0].kind(),
                    ExternalRedirectionKind::Output
                );
                assert_eq!(stage.redirections()[0].target().to_bytes(), b"created.txt");
                return Ok(0);
            }

            assert_eq!(pipeline.stages().len(), 2usize);
            assert_eq!(
                pipeline.stages()[0]
                    .command()
                    .expect("first pipeline command")
                    .executable()
                    .to_bytes(),
                b"first"
            );
            let second = pipeline.stages()[1]
                .command()
                .expect("second pipeline command");
            assert_eq!(second.executable().to_bytes(), b"second");
            assert_eq!(pipeline.stages()[1].redirections().len(), 1usize);
            assert_eq!(
                pipeline.stages()[1].redirections()[0].kind(),
                ExternalRedirectionKind::Append
            );
            assert_eq!(
                pipeline.stages()[1].redirections()[0].target().to_bytes(),
                b"result.txt"
            );
            Ok(23)
        }
    }

    #[test]
    fn bridges_typed_pipeline_stages_and_redirections_across_the_c_abi() {
        let mut shell = Shell::new().expect("create shell");
        let context = ExecutionContext::new();
        let mut executor = PipelinePlanExecutor {
            calls: 0usize,
            redirection_only: false,
        };
        let result = shell
            .execute(
                b"first | second >> result.txt",
                &context,
                Some(&mut executor),
            )
            .expect("dispatch typed pipeline plan");

        assert_eq!(executor.calls, 1usize);
        assert_eq!(result.exit_code, 23);
        assert_eq!(result.commands_executed, 2usize);

        executor.redirection_only = true;
        let result = shell
            .execute(b"> created.txt", &context, Some(&mut executor))
            .expect("dispatch redirection-only plan");
        assert_eq!(executor.calls, 2usize);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.commands_executed, 1usize);
    }

    #[test]
    fn translates_external_executor_failure() {
        let mut shell = Shell::new().expect("create shell");
        let mut executor = |_command: ExternalCommand<'_>| Err(ExternalExecutionError::Failed);
        let error = shell
            .execute(
                b"unknown_tool",
                &ExecutionContext::new(),
                Some(&mut executor),
            )
            .expect_err("executor failure must propagate");
        let ShellError::Execution { status, .. } = error else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(status, DeStatus::EXTERNAL_EXECUTION_FAILED);
    }

    #[test]
    fn catches_executor_panics_before_returning_to_c() {
        let mut shell = Shell::new().expect("create shell");
        let mut executor = |_command: ExternalCommand<'_>| -> Result<i32, ExternalExecutionError> {
            panic!("simulated host callback panic");
        };
        let error = shell
            .execute(
                b"unknown_tool",
                &ExecutionContext::new(),
                Some(&mut executor),
            )
            .expect_err("callback panic must be contained");
        let ShellError::Execution { status, diagnostic } = error else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(status, DeStatus::EXTERNAL_EXECUTION_FAILED);
        assert_eq!(diagnostic.code, DeStatus::INTERNAL_ERROR.as_raw());
    }
}
