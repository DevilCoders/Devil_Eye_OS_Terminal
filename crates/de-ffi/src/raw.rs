//! Private raw declarations and the panic-contained C callback bridge.
//!
//! All unsafe pointer handling is isolated here. The public crate API exposes
//! only borrowed CStr values and owned Rust result types.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;

use super::{
    CallbackContext, DeStatus, EnvironmentEntry, ExternalCommand, ExternalExecutionError,
};

pub(super) const MAX_ARGUMENTS: usize = 256;
pub(super) const MAX_ENVIRONMENT_ENTRIES: usize = 256;
pub(super) const MAX_SOURCE_BYTES: usize = 65_536;

#[repr(C)]
pub(super) struct DeShellOpaque {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RawAbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct RawEnvironmentEntry {
    pub key: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
pub(super) struct RawExternalCommand {
    pub executable: *const c_char,
    pub argc: usize,
    pub argv: *const *const c_char,
    pub environment: *const RawEnvironmentEntry,
    pub environment_count: usize,
    pub working_directory: *const c_char,
}

pub(super) type RawExternalExecutor = unsafe extern "C" fn(
    command: *const RawExternalCommand,
    out_exit_code: *mut c_int,
    user_data: *mut c_void,
) -> c_int;

#[repr(C)]
pub(super) struct RawExecutionContext {
    pub environment: *const RawEnvironmentEntry,
    pub environment_count: usize,
    pub working_directory: *const c_char,
    pub external_executor: Option<RawExternalExecutor>,
    pub user_data: *mut c_void,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RawExecutionResult {
    pub exit_code: c_int,
    pub exit_requested: c_int,
    pub commands_executed: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RawExecutionDiagnostic {
    pub stage: c_int,
    pub code: c_int,
    pub source_offset: usize,
}

unsafe extern "C" {
    pub fn de_shell_get_abi_version(out_version: *mut RawAbiVersion) -> c_int;
    pub fn de_shell_check_abi_version(requested_version: *const RawAbiVersion) -> c_int;
    pub fn de_shell_create(out_shell: *mut *mut DeShellOpaque) -> c_int;
    pub fn de_shell_destroy(shell: *mut *mut DeShellOpaque) -> c_int;
    pub fn de_shell_execute_source(
        shell: *mut DeShellOpaque,
        source: *const u8,
        source_length: usize,
        context: *const RawExecutionContext,
        out_result: *mut RawExecutionResult,
        out_diagnostic: *mut RawExecutionDiagnostic,
    ) -> c_int;
}

/// Synchronous callback invoked by the C execution engine. Panics are caught
/// and translated to INTERNAL_ERROR before control returns to C.
pub(super) unsafe extern "C" fn external_executor_callback(
    command: *const RawExternalCommand,
    out_exit_code: *mut c_int,
    user_data: *mut c_void,
) -> c_int {
    if command.is_null() || out_exit_code.is_null() || user_data.is_null() {
        return DeStatus::INVALID_ARGUMENT.as_raw();
    }

    // SAFETY: the C execution contract guarantees that the command, its
    // pointer arrays, and all pointed-to strings remain valid until callback
    // return. user_data points to the stack-owned CallbackContext for this call.
    let result = catch_unwind(AssertUnwindSafe(|| {
        let raw_command = unsafe { &*command };
        let callback_context = unsafe { &mut *user_data.cast::<CallbackContext<'_>>() };
        let command = unsafe { parse_external_command(raw_command)? };
        callback_context
            .executor
            .execute(command)
            .map_err(|error| match error {
                ExternalExecutionError::Failed => DeStatus::EXTERNAL_EXECUTION_FAILED,
            })
    }));

    match result {
        Ok(Ok(exit_code)) => {
            // SAFETY: checked for null above; caller owns writable output storage.
            unsafe {
                *out_exit_code = exit_code;
            }
            DeStatus::OK.as_raw()
        }
        Ok(Err(status)) => status.as_raw(),
        Err(_) => DeStatus::INTERNAL_ERROR.as_raw(),
    }
}

unsafe fn parse_external_command<'a>(
    command: &'a RawExternalCommand,
) -> Result<ExternalCommand<'a>, DeStatus> {
    if command.executable.is_null()
        || command.argv.is_null()
        || command.argc == 0
        || command.argc > MAX_ARGUMENTS
        || command.environment_count > MAX_ENVIRONMENT_ENTRIES
        || (command.environment_count != 0 && command.environment.is_null())
    {
        return Err(DeStatus::INVALID_ARGUMENT);
    }

    // SAFETY: pointer validity and NUL termination are guaranteed by the C
    // library's synchronous callback contract.
    let executable = unsafe { CStr::from_ptr(command.executable) };
    let argv = unsafe { slice::from_raw_parts(command.argv, command.argc + 1) };
    if argv[0].is_null() || !unsafe { CStr::from_ptr(argv[0]) }.eq(executable) {
        return Err(DeStatus::INVALID_ARGUMENT);
    }
    if !argv[command.argc].is_null() {
        return Err(DeStatus::INVALID_ARGUMENT);
    }

    let mut arguments = Vec::with_capacity(command.argc.saturating_sub(1));
    for argument in &argv[1..command.argc] {
        if argument.is_null() {
            return Err(DeStatus::INVALID_ARGUMENT);
        }
        arguments.push(unsafe { CStr::from_ptr(*argument) });
    }

    let mut environment = Vec::with_capacity(command.environment_count);
    if command.environment_count != 0 {
        let entries = unsafe {
            slice::from_raw_parts(command.environment, command.environment_count)
        };
        for entry in entries {
            if entry.key.is_null() || entry.value.is_null() {
                return Err(DeStatus::INVALID_ARGUMENT);
            }
            let key = unsafe { CStr::from_ptr(entry.key) };
            let value = unsafe { CStr::from_ptr(entry.value) };
            if key.to_bytes().is_empty() {
                return Err(DeStatus::INVALID_ARGUMENT);
            }
            environment.push(EnvironmentEntry { key, value });
        }
    }

    let working_directory = if command.working_directory.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(command.working_directory) })
    };

    Ok(ExternalCommand {
        executable,
        arguments,
        environment,
        working_directory,
    })
}
