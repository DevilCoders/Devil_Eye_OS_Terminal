#include "de_shell/shell.h"

#include "internal/shell_internal.h"

#include <stdlib.h>

de_shell_status_t de_shell_create(de_shell_t **out_shell)
{
    de_shell_t *shell = NULL;

    if (out_shell == NULL) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    *out_shell = NULL;

    shell = calloc(1U, sizeof(*shell));
    if (shell == NULL) {
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }

    shell->lifecycle_state = DE_SHELL_INTERNAL_STATE_READY;
    *out_shell = shell;

    return DE_SHELL_STATUS_OK;
}

de_shell_status_t de_shell_destroy(de_shell_t **shell)
{
    de_shell_t *owned_shell = NULL;

    if (shell == NULL) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    owned_shell = *shell;
    *shell = NULL;

    free(owned_shell);
    return DE_SHELL_STATUS_OK;
}
