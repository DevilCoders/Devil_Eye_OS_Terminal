#include "de_shell/shell.h"

#include "internal/command_registry.h"
#include "internal/shell_internal.h"

#include <stdlib.h>

de_shell_status_t de_shell_create(de_shell_t **out_shell)
{
    de_shell_t *shell = NULL;
    de_shell_registry_status_t registry_status;

    if (out_shell == NULL) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    *out_shell = NULL;

    shell = calloc(1U, sizeof(*shell));
    if (shell == NULL) {
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }

    shell->lifecycle_state = DE_SHELL_INTERNAL_STATE_READY;
    registry_status = de_shell_command_registry_create(&shell->command_registry);
    if (registry_status != DE_SHELL_REGISTRY_STATUS_OK) {
        free(shell);
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }

    registry_status = de_shell_command_registry_register_core(shell->command_registry);
    if (registry_status != DE_SHELL_REGISTRY_STATUS_OK) {
        (void)de_shell_command_registry_destroy(&shell->command_registry);
        free(shell);
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }

    *out_shell = shell;
    return DE_SHELL_STATUS_OK;
}

de_shell_status_t de_shell_destroy(de_shell_t **shell)
{
    de_shell_t *owned_shell = NULL;
    de_shell_registry_status_t registry_status;

    if (shell == NULL) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    owned_shell = *shell;
    if (owned_shell != NULL && owned_shell->execution_active != 0U) {
        return DE_SHELL_STATUS_BUSY;
    }

    *shell = NULL;

    if (owned_shell != NULL) {
        size_t index;

        for (index = 0U; index < owned_shell->variable_count; index += 1U) {
            free(owned_shell->variables[index].name);
            free(owned_shell->variables[index].value);
        }

        registry_status =
            de_shell_command_registry_destroy(&owned_shell->command_registry);
        free(owned_shell);
        if (registry_status != DE_SHELL_REGISTRY_STATUS_OK) {
            return DE_SHELL_STATUS_INTERNAL_ERROR;
        }
    }

    return DE_SHELL_STATUS_OK;
}
