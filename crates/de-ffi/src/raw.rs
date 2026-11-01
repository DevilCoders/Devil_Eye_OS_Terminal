//! Private raw declarations and the panic-contained C callback bridge.
//!
//! All unsafe pointer handling is isolated here. The public crate API exposes
//! only borrowed CStr values and owned Rust result types.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;

use super::{
    CallbackContext, DeStatus, EnvironmentEntry, ExternalCommand, ExternalExecutionError,
    ExternalPipeline, ExternalPipelineStage, ExternalRedirection, ExternalRedirectionKind,
};

pub(super) const MAX_ARGUMENTS: usize = 256;
pub(super) const MAX_ENVIRONMENT_ENTRIES: usize = 256;

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
        if raw_command.pipeline_stage_count != 0 {
            let pipeline = unsafe { parse_external_pipeline(raw_command)? };
            callback_context
                .executor
                .execute_pipeline(pipeline)
                .map_err(|error| match error {
                    ExternalExecutionError::Failed => DeStatus::EXTERNAL_EXECUTION_FAILED,
                })
        } else {
            let command = unsafe { parse_external_command(raw_command)? };
            callback_context
                .executor
                .execute(command)
                .map_err(|error| match error {
                    ExternalExecutionError::Failed => DeStatus::EXTERNAL_EXECUTION_FAILED,
                })
        }
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
        || !command.pipeline_stages.is_null()
        || command.pipeline_stage_count != 0
    {
        return Err(DeStatus::INVALID_ARGUMENT);
    }

    // SAFETY: pointer validity and NUL termination are guaranteed by the C
    // library's synchronous callback contract.
    let executable = unsafe { CStr::from_ptr(command.executable) };
    let argv = unsafe { slice::from_raw_parts(command.argv, command.argc + 1) };
    if argv[0].is_null() || unsafe { CStr::from_ptr(argv[0]) } != executable {
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

    let environment = unsafe { parse_environment(command.environment, command.environment_count)? };
    let working_directory = unsafe { parse_working_directory(command.working_directory) };

    Ok(ExternalCommand {
        executable,
        arguments,
        environment,
        working_directory,
    })
}

unsafe fn parse_environment<'a>(
    pointer: *const RawEnvironmentEntry,
    count: usize,
) -> Result<Vec<EnvironmentEntry<'a>>, DeStatus> {
    if count > MAX_ENVIRONMENT_ENTRIES || (count != 0 && pointer.is_null()) {
        return Err(DeStatus::INVALID_ARGUMENT);
    }
    let mut environment = Vec::with_capacity(count);
    if count != 0 {
        let entries = unsafe { slice::from_raw_parts(pointer, count) };
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
    Ok(environment)
}

unsafe fn parse_working_directory<'a>(pointer: *const c_char) -> Option<&'a CStr> {
    if pointer.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(pointer) })
    }
}

unsafe fn parse_external_pipeline<'a>(
    request: &'a RawExternalCommand,
) -> Result<ExternalPipeline<'a>, DeStatus> {
    if !request.executable.is_null()
        || request.argc != 0
        || !request.argv.is_null()
        || request.pipeline_stages.is_null()
        || request.pipeline_stage_count == 0
        || request.pipeline_stage_count > MAX_PIPELINE_STAGES
    {
        return Err(DeStatus::INVALID_ARGUMENT);
    }

    let environment = unsafe { parse_environment(request.environment, request.environment_count)? };
    let working_directory = unsafe { parse_working_directory(request.working_directory) };
    let raw_stages =
        unsafe { slice::from_raw_parts(request.pipeline_stages, request.pipeline_stage_count) };
    let mut stages = Vec::with_capacity(request.pipeline_stage_count);
    let mut total_redirections = 0usize;

    for raw_stage in raw_stages {
        total_redirections = total_redirections
            .checked_add(raw_stage.redirection_count)
            .ok_or(DeStatus::INVALID_ARGUMENT)?;
        if total_redirections > MAX_REDIRECTIONS_PER_PLAN
            || (raw_stage.redirection_count != 0 && raw_stage.redirections.is_null())
        {
            return Err(DeStatus::INVALID_ARGUMENT);
        }
        if raw_stage.command.is_null()
            && (request.pipeline_stage_count != 1 || raw_stage.redirection_count == 0)
        {
            return Err(DeStatus::INVALID_ARGUMENT);
        }

        let command = if raw_stage.command.is_null() {
            None
        } else {
            Some(unsafe { parse_external_command(&*raw_stage.command)? })
        };

        let mut redirections = Vec::with_capacity(raw_stage.redirection_count);
        if raw_stage.redirection_count != 0 {
            let raw_redirections = unsafe {
                slice::from_raw_parts(raw_stage.redirections, raw_stage.redirection_count)
            };
            for raw_redirection in raw_redirections {
                if raw_redirection.target.is_null() {
                    return Err(DeStatus::INVALID_ARGUMENT);
                }
                let target = unsafe { CStr::from_ptr(raw_redirection.target) };
                if target.to_bytes().is_empty() {
                    return Err(DeStatus::INVALID_ARGUMENT);
                }
                let kind = match raw_redirection.kind {
                    0 => ExternalRedirectionKind::Input,
                    1 => ExternalRedirectionKind::Output,
                    2 => ExternalRedirectionKind::Append,
                    3 => ExternalRedirectionKind::ErrorOutput,
                    4 => ExternalRedirectionKind::ErrorAppend,
                    _ => return Err(DeStatus::INVALID_ARGUMENT),
                };
                redirections.push(ExternalRedirection {
                    kind,
                    target,
                    source_offset: raw_redirection.source_offset,
                });
            }
        }

        stages.push(ExternalPipelineStage {
            command,
            redirections,
        });
    }

    Ok(ExternalPipeline {
        stages,
        environment,
        working_directory,
    })
}

pub(super) fn query_abi_version(out_version: &mut RawAbiVersion) -> c_int {
    // SAFETY: the reference guarantees writable storage with the required layout.
    unsafe { de_shell_get_abi_version(out_version) }
}

pub(super) fn check_abi_version(requested_version: &RawAbiVersion) -> c_int {
    // SAFETY: the reference guarantees readable storage with the required layout.
    unsafe { de_shell_check_abi_version(requested_version) }
}

pub(super) fn create_shell(out_shell: &mut *mut DeShellOpaque) -> c_int {
    // SAFETY: the reference guarantees writable pointer storage.
    unsafe { de_shell_create(out_shell) }
}

pub(super) fn destroy_shell(shell: &mut *mut DeShellOpaque) -> c_int {
    // SAFETY: the pointer is the owned handle variable returned by create_shell.
    unsafe { de_shell_destroy(shell) }
}

pub(super) fn execute_source(
    shell: *mut DeShellOpaque,
    source: &[u8],
    context: &RawExecutionContext,
    out_result: &mut RawExecutionResult,
    out_diagnostic: &mut RawExecutionDiagnostic,
) -> c_int {
    // SAFETY: the safe wrapper keeps all source, context, callback, and output
    // storage alive for this synchronous call and exclusively borrows shell.
    unsafe {
        de_shell_execute_source(
            shell,
            source.as_ptr(),
            source.len(),
            context,
            out_result,
            out_diagnostic,
        )
    }
}

#[cfg(test)]
pub(super) fn create_shell_with_null_output() -> c_int {
    // SAFETY: this intentionally exercises the C API's documented NULL path.
    unsafe { de_shell_create(std::ptr::null_mut()) }
}
