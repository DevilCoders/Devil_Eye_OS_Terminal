#ifndef DE_SHELL_ABI_H
#define DE_SHELL_ABI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * All public status values use an explicitly sized signed 32-bit integer.
 * Do not replace this typedef with a C enum: enum representation is
 * implementation-defined and must not define the cross-language ABI.
 */
typedef int32_t de_shell_status_t;

#define DE_SHELL_STATUS_OK ((de_shell_status_t)0)
#define DE_SHELL_STATUS_INVALID_ARGUMENT ((de_shell_status_t)1)
#define DE_SHELL_STATUS_INTERNAL_ERROR ((de_shell_status_t)2)
#define DE_SHELL_STATUS_BUSY ((de_shell_status_t)3)
#define DE_SHELL_STATUS_PARSE_ERROR ((de_shell_status_t)4)
#define DE_SHELL_STATUS_RESOURCE_LIMIT ((de_shell_status_t)5)
#define DE_SHELL_STATUS_UNSUPPORTED_OPERATION ((de_shell_status_t)6)
#define DE_SHELL_STATUS_EXECUTION_UNAVAILABLE ((de_shell_status_t)7)
#define DE_SHELL_STATUS_EXTERNAL_EXECUTION_FAILED ((de_shell_status_t)8)
#define DE_SHELL_STATUS_ABI_MISMATCH ((de_shell_status_t)9)

#define DE_SHELL_ABI_VERSION_MAJOR UINT16_C(0)
#define DE_SHELL_ABI_VERSION_MINOR UINT16_C(2)
#define DE_SHELL_ABI_VERSION_PATCH UINT16_C(0)

/*
 * Fixed-layout ABI version tuple. Layout is three consecutive uint16_t
 * fields: size 6 bytes, alignment 2 bytes, offsets 0/2/4.
 */
typedef struct de_shell_abi_version {
    uint16_t major;
    uint16_t minor;
    uint16_t patch;
} de_shell_abi_version_t;

/*
 * Writes the library's current ABI version into caller-owned storage.
 * This function allocates no memory and is safe to call concurrently.
 */
de_shell_status_t de_shell_get_abi_version(
    de_shell_abi_version_t *out_version);

/*
 * Checks whether a caller's requested ABI version can be served.
 *
 * Major versions must match. Older or equal minor versions are compatible;
 * a requested minor version newer than the library is rejected. Patch
 * differences are compatible. This function allocates no memory and is
 * safe to call concurrently.
 */
de_shell_status_t de_shell_check_abi_version(
    const de_shell_abi_version_t *requested_version);

#ifdef __cplusplus
}
#endif

#endif
