#include "de_shell/abi.h"

de_shell_status_t de_shell_get_abi_version(
    de_shell_abi_version_t *out_version)
{
    if (out_version == 0) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    out_version->major = DE_SHELL_ABI_VERSION_MAJOR;
    out_version->minor = DE_SHELL_ABI_VERSION_MINOR;
    out_version->patch = DE_SHELL_ABI_VERSION_PATCH;
    return DE_SHELL_STATUS_OK;
}

de_shell_status_t de_shell_check_abi_version(
    const de_shell_abi_version_t *requested_version)
{
    if (requested_version == 0) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    if (requested_version->major != DE_SHELL_ABI_VERSION_MAJOR
        || requested_version->minor > DE_SHELL_ABI_VERSION_MINOR) {
        return DE_SHELL_STATUS_ABI_MISMATCH;
    }

    return DE_SHELL_STATUS_OK;
}
