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
    minor: 1,
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

/// A host executor handles external commands. The callback is synchronous;
/// every C string in the request is borrowed only for the duration of execute.
pub trait CommandExecutor {
    fn execute(
        &mut self,
        command: ExternalCommand<'_>,
    ) -> Result<i32, ExternalExecutionError>;
}

impl<F> CommandExecutor for F
where
    F: for<'a> FnMut(ExternalCommand<'a>) -> Result<i32, ExternalExecutionError>,
{
    fn execute(
        &mut self,
        command: ExternalCommand<'_>,
    ) -> Result<i32, ExternalExecutionError> {
        self(command)
    }
}

/// Failure reported by a host-provided external command executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalExecutionError {
    Failed,
}

pub(super) struct CallbackContext<'a> {
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
                write!(formatter, "DE-Shell ABI version query failed: {}", status.as_raw())
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
            Self::SourceTooLarge => formatter.write_str("DE-Shell source exceeds the 65536-byte limit"),
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
        let query_status =
            DeStatus::from_raw(unsafe { raw::de_shell_get_abi_version(&mut raw_version) });
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
        let check_status = DeStatus::from_raw(unsafe {
            raw::de_shell_check_abi_version(&requested)
        });
        if !check_status.is_ok() {
            return Err(ShellError::AbiMismatch(library_version));
        }

        let mut handle = null_mut();
        // SAFETY: handle is valid writable pointer storage.
        let create_status =
            DeStatus::from_raw(unsafe { raw::de_shell_create(&mut handle) });
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
        let user_data = callback_context
            .as_mut()
            .map_or(null_mut(), |callback| {
                (callback as *mut CallbackContext<'_>).cast::<c_void>()
            });

        let raw_context = raw::RawExecutionContext {
            environment: if raw_environment.is_empty() {
                null()
            } else {
                raw_environment.as_ptr()
            },
            environment_count: raw_environment.len(),
            working_directory: context
                .working_directory
                .map_or(null(), CStr::as_ptr),
            external_executor,
            user_data,
        };

        let raw_shell = self.raw.ok_or(ShellError::Closed)?;
        let mut raw_result = raw::RawExecutionResult::default();
        let mut raw_diagnostic = raw::RawExecutionDiagnostic::default();

        // SAFETY: all pointers in raw_context and source remain valid for this
        // synchronous call; the shell handle is exclusively borrowed.
        let status = DeStatus::from_raw(unsafe {
            raw::de_shell_execute_source(
                raw_shell.as_ptr(),
                source.as_ptr(),
                source.len(),
                &raw_context,
                &mut raw_result,
                &mut raw_diagnostic,
            )
        });

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
        let status =
            DeStatus::from_raw(unsafe { raw::de_shell_destroy(&mut handle) });
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
#[allow(unsafe_code)]
mod tests {
    use super::{
        CommandExecutor, DeAbiVersion, DeStatus, EnvironmentEntry, ExecutionContext,
        ExecutionStage, ExternalCommand, ExternalExecutionError, Shell, ShellError, ABI_VERSION,
        MAX_SOURCE_BYTES,
    };
    use std::ffi::CString;
    use std::mem::{align_of, offset_of, size_of};
    use std::ptr::null_mut;

    #[test]
    fn exposes_expected_abi_version() {
        assert_eq!(
            ABI_VERSION,
            DeAbiVersion {
                major: 0,
                minor: 1,
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
            minor: 2,
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
    fn c_api_rejects_null_create_output() {
        // SAFETY: this intentionally exercises the C API's NULL argument path.
        let status = unsafe { super::raw::de_shell_create(null_mut()) };
        assert_eq!(status, DeStatus::INVALID_ARGUMENT.as_raw());
    }

    #[test]
    fn creates_and_destroys_repeated_shell_handles() {
        for _ in 0..64 {
            let mut shell = Shell::new().expect("create C-owned shell");
            shell.close().expect("destroy C-owned shell");
            shell.close().expect("idempotent close");
            assert!(matches!(shell.execute(b"exit", &ExecutionContext::new(), None), Err(ShellError::Closed)));
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
            .execute(
                b"unknown_tool alpha beta",
                &context,
                Some(&mut executor),
            )
            .expect("external callback succeeds");
        assert!(observed);
        assert_eq!(result.exit_code, 7);
        assert_eq!(result.commands_executed, 1);
    }

    #[test]
    fn translates_external_executor_failure() {
        let mut shell = Shell::new().expect("create shell");
        let mut executor = |_command: ExternalCommand<'_>| {
            Err(ExternalExecutionError::Failed)
        };
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
