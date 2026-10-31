#ifndef DE_SHELL_SHELL_H
#define DE_SHELL_SHELL_H

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Opaque handle for one independent DE-Shell instance.
 *
 * The handle is created and destroyed by this library. Callers must not
 * inspect, copy, or free its representation. A handle is single-thread-affine
 * unless a future API version explicitly documents synchronization.
 */
typedef struct de_shell de_shell_t;

/**
 * Stable status values shared with the initial Rust DeStatus definition.
 *
 * Existing numeric assignments must never be changed or reused.
 */
typedef enum de_shell_status {
    DE_SHELL_STATUS_OK = 0,
    DE_SHELL_STATUS_INVALID_ARGUMENT = 1,
    DE_SHELL_STATUS_INTERNAL_ERROR = 2
} de_shell_status_t;

/**
 * Creates an independent DE-Shell instance.
 *
 * On entry, out_shell must point to writable storage. On every failure after
 * validating out_shell, *out_shell is set to NULL. The caller owns a
 * successfully created handle and must release it with de_shell_destroy().
 *
 * @param out_shell Receives the new opaque handle.
 * @return DE_SHELL_STATUS_OK on success, otherwise a stable error status.
 */
de_shell_status_t de_shell_create(de_shell_t **out_shell);

/**
 * Destroys an owned DE-Shell instance and clears the caller's handle.
 *
 * Passing a valid pointer whose value is NULL is permitted and succeeds,
 * making repeated cleanup through the same handle variable idempotent.
 * Passing a NULL pointer-to-handle is invalid. The caller must not use any
 * copied alias after destruction.
 *
 * @param shell Address of the caller-owned handle.
 * @return DE_SHELL_STATUS_OK on success, otherwise a stable error status.
 */
de_shell_status_t de_shell_destroy(de_shell_t **shell);

#ifdef __cplusplus
}
#endif

#endif
