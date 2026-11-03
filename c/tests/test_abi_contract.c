#include "de_shell/abi.h"
#include "de_shell/execution.h"
#include "de_shell/shell.h"

#include <stddef.h>
#include <stdio.h>

_Static_assert(sizeof(de_shell_external_command_t) == 64U, "external command ABI size mismatch");
_Static_assert(_Alignof(de_shell_external_command_t) == 8U, "external command ABI alignment mismatch");
_Static_assert(offsetof(de_shell_external_command_t, pipeline_stages) == 48U, "pipeline pointer offset mismatch");
_Static_assert(offsetof(de_shell_external_command_t, pipeline_stage_count) == 56U, "pipeline count offset mismatch");
_Static_assert(sizeof(de_shell_pipeline_stage_t) == 24U, "pipeline stage ABI size mismatch");
_Static_assert(_Alignof(de_shell_pipeline_stage_t) == 8U, "pipeline stage ABI alignment mismatch");
_Static_assert(offsetof(de_shell_pipeline_stage_t, command) == 0U, "pipeline command offset mismatch");
_Static_assert(offsetof(de_shell_pipeline_stage_t, redirections) == 8U, "pipeline redirection offset mismatch");
_Static_assert(offsetof(de_shell_pipeline_stage_t, redirection_count) == 16U, "pipeline redirection count offset mismatch");
_Static_assert(sizeof(de_shell_io_redirection_t) == 24U, "redirection ABI size mismatch");
_Static_assert(_Alignof(de_shell_io_redirection_t) == 8U, "redirection ABI alignment mismatch");
_Static_assert(offsetof(de_shell_io_redirection_t, type) == 0U, "redirection kind offset mismatch");
_Static_assert(offsetof(de_shell_io_redirection_t, target) == 8U, "redirection target offset mismatch");
_Static_assert(offsetof(de_shell_io_redirection_t, source_offset) == 16U, "redirection source offset mismatch");

_Static_assert(sizeof(de_shell_status_t) == 4U, "ABI status must be 32-bit");
_Static_assert(_Alignof(de_shell_status_t) == 4U, "ABI status alignment must be 4");
_Static_assert(sizeof(de_shell_abi_version_t) == 6U, "ABI version must be 6 bytes");
_Static_assert(_Alignof(de_shell_abi_version_t) == 2U, "ABI version alignment must be 2");
_Static_assert(offsetof(de_shell_abi_version_t, major) == 0U, "major offset mismatch");
_Static_assert(offsetof(de_shell_abi_version_t, minor) == 2U, "minor offset mismatch");
_Static_assert(offsetof(de_shell_abi_version_t, patch) == 4U, "patch offset mismatch");

static int test_version_query(void)
{
    de_shell_abi_version_t version;

    if (de_shell_get_abi_version(NULL) != DE_SHELL_STATUS_INVALID_ARGUMENT) {
        (void)fprintf(stderr, "NULL version output was accepted\n");
        return 1;
    }

    if (de_shell_get_abi_version(&version) != DE_SHELL_STATUS_OK) {
        (void)fprintf(stderr, "ABI version query failed\n");
        return 1;
    }

    if (version.major != DE_SHELL_ABI_VERSION_MAJOR
        || version.minor != DE_SHELL_ABI_VERSION_MINOR
        || version.patch != DE_SHELL_ABI_VERSION_PATCH) {
        (void)fprintf(stderr, "ABI version tuple does not match the public constants\n");
        return 1;
    }

    return 0;
}

static int test_version_compatibility(void)
{
    const de_shell_abi_version_t oldest_minor = {0U, 0U, 99U};
    const de_shell_abi_version_t older_minor = {0U, 1U, 99U};
    const de_shell_abi_version_t same_minor_new_patch = {0U, 2U, 99U};
    const de_shell_abi_version_t future_minor = {0U, 3U, 0U};
    const de_shell_abi_version_t future_major = {1U, 0U, 0U};

    if (de_shell_check_abi_version(NULL) != DE_SHELL_STATUS_INVALID_ARGUMENT
        || de_shell_check_abi_version(&oldest_minor) != DE_SHELL_STATUS_OK
        || de_shell_check_abi_version(&older_minor) != DE_SHELL_STATUS_OK
        || de_shell_check_abi_version(&same_minor_new_patch) != DE_SHELL_STATUS_OK
        || de_shell_check_abi_version(&future_minor) != DE_SHELL_STATUS_ABI_MISMATCH
        || de_shell_check_abi_version(&future_major) != DE_SHELL_STATUS_ABI_MISMATCH) {
        (void)fprintf(stderr, "ABI compatibility policy was not enforced\n");
        return 1;
    }

    return 0;
}


static int test_c_owned_handle_lifecycle(void)
{
    de_shell_t *shell = NULL;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK || shell == NULL) {
        (void)fprintf(stderr, "C library did not create an owned shell handle\\n");
        return 1;
    }

    if (de_shell_destroy(&shell) != DE_SHELL_STATUS_OK || shell != NULL) {
        (void)fprintf(stderr, "C library did not clear the destroyed shell handle\\n");
        return 1;
    }

    if (de_shell_destroy(&shell) != DE_SHELL_STATUS_OK) {
        (void)fprintf(stderr, "NULL shell cleanup was not idempotent\\n");
        return 1;
    }

    return 0;
}

int main(void)
{
    if (test_version_query() != 0
        || test_version_compatibility() != 0
        || test_c_owned_handle_lifecycle() != 0) {
        return 1;
    }

    (void)puts("DE-Shell ABI contract tests passed");
    return 0;
}
