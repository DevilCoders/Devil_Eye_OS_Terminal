#ifndef DE_SHELL_INTERNAL_COMMAND_REGISTRY_H
#define DE_SHELL_INTERNAL_COMMAND_REGISTRY_H

#include <stddef.h>
#include <stdint.h>

#include "de_shell/shell.h"

typedef struct de_shell_command_registry de_shell_command_registry_t;

typedef enum de_shell_registry_status {
    DE_SHELL_REGISTRY_STATUS_OK = 0,
    DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT = 1,
    DE_SHELL_REGISTRY_STATUS_OUT_OF_MEMORY = 2,
    DE_SHELL_REGISTRY_STATUS_DUPLICATE_NAME = 3,
    DE_SHELL_REGISTRY_STATUS_NOT_FOUND = 4,
    DE_SHELL_REGISTRY_STATUS_ARGUMENT_COUNT_MISMATCH = 5,
    DE_SHELL_REGISTRY_STATUS_HANDLER_FAILED = 6
} de_shell_registry_status_t;

typedef enum de_shell_command_result {
    DE_SHELL_COMMAND_RESULT_SUCCESS = 0,
    DE_SHELL_COMMAND_RESULT_EXIT_REQUESTED = 1,
    DE_SHELL_COMMAND_RESULT_FAILURE = 2
} de_shell_command_result_t;

#define DE_SHELL_COMMAND_MAX_ARGS_UNBOUNDED SIZE_MAX
#define DE_SHELL_COMMAND_FLAG_NONE UINT32_C(0)
#define DE_SHELL_COMMAND_FLAG_TERMINATES_SHELL (UINT32_C(1) << 0U)

typedef struct de_shell_command_info {
    const char *name;
    const char *description;
    size_t min_args;
    size_t max_args;
    uint32_t flags;
} de_shell_command_info_t;

typedef de_shell_command_result_t (*de_shell_builtin_handler_t)(
    de_shell_t *shell,
    size_t argc,
    const char *const argv[],
    void *user_data);

typedef struct de_shell_command_definition {
    const char *name;
    const char *description;
    size_t min_args;
    size_t max_args;
    uint32_t flags;
    de_shell_builtin_handler_t handler;
    void *user_data;
} de_shell_command_definition_t;

/* Creates an empty per-shell command registry. */
de_shell_registry_status_t de_shell_command_registry_create(
    de_shell_command_registry_t **out_registry);

/* Destroys a registry and clears the caller's handle; NULL values are allowed. */
de_shell_registry_status_t de_shell_command_registry_destroy(
    de_shell_command_registry_t **registry);

/*
 * Registers a command. Names are case-sensitive and unique. The registry
 * copies name/description; handler and user_data remain caller-owned.
 */
de_shell_registry_status_t de_shell_command_registry_register(
    de_shell_command_registry_t *registry,
    const de_shell_command_definition_t *definition);

/*
 * Looks up a command by exact name. out_info is initialized to NULL.
 * The returned metadata pointer remains valid until the next successful
 * registration or registry destruction.
 */
de_shell_registry_status_t de_shell_command_registry_lookup(
    const de_shell_command_registry_t *registry,
    const char *name,
    const de_shell_command_info_t **out_info);

/*
 * Dispatches argv[0] by exact name. argc includes the command name; min_args
 * and max_args count only arguments after argv[0]. All argv entries must be
 * NUL-terminated strings. The handler result is
 * returned through out_result; a handler-level failure is not a registry
 * lookup/dispatch failure.
 */
de_shell_registry_status_t de_shell_command_registry_dispatch(
    const de_shell_command_registry_t *registry,
    de_shell_t *shell,
    size_t argc,
    const char *const argv[],
    de_shell_command_result_t *out_result);

/* Registers the core "exit" built-in. Called once during shell creation. */
de_shell_registry_status_t de_shell_command_registry_register_core(
    de_shell_command_registry_t *registry);

#endif
