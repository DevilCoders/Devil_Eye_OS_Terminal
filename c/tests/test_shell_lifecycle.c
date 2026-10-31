#include "de_shell/shell.h"

#include <stdio.h>

static int test_create_returns_owned_handle(void)
{
    de_shell_t *shell = NULL;
    const de_shell_status_t status = de_shell_create(&shell);

    if (status != DE_SHELL_STATUS_OK || shell == NULL) {
        (void)fprintf(stderr, "create failed or returned a NULL handle\n");
        return 1;
    }

    if (de_shell_destroy(&shell) != DE_SHELL_STATUS_OK || shell != NULL) {
        (void)fprintf(stderr, "destroy did not release and clear the handle\n");
        return 1;
    }

    return 0;
}

static int test_create_rejects_null_output(void)
{
    if (de_shell_create(NULL) != DE_SHELL_STATUS_INVALID_ARGUMENT) {
        (void)fprintf(stderr, "create must reject a NULL output pointer\n");
        return 1;
    }

    return 0;
}

static int test_destroy_rejects_null_handle_address(void)
{
    if (de_shell_destroy(NULL) != DE_SHELL_STATUS_INVALID_ARGUMENT) {
        (void)fprintf(stderr, "destroy must reject a NULL handle address\n");
        return 1;
    }

    return 0;
}

static int test_destroy_is_idempotent_for_cleared_handle(void)
{
    de_shell_t *shell = NULL;

    if (de_shell_destroy(&shell) != DE_SHELL_STATUS_OK) {
        (void)fprintf(stderr, "destroy of a cleared handle must succeed\n");
        return 1;
    }

    return 0;
}

static int test_multiple_instances_have_independent_lifetimes(void)
{
    de_shell_t *first = NULL;
    de_shell_t *second = NULL;

    if (de_shell_create(&first) != DE_SHELL_STATUS_OK || first == NULL) {
        (void)fprintf(stderr, "first instance creation failed\n");
        return 1;
    }

    if (de_shell_create(&second) != DE_SHELL_STATUS_OK || second == NULL) {
        (void)fprintf(stderr, "second instance creation failed\n");
        (void)de_shell_destroy(&first);
        return 1;
    }

    if (first == second) {
        (void)fprintf(stderr, "independent instances unexpectedly share an address\n");
        (void)de_shell_destroy(&first);
        (void)de_shell_destroy(&second);
        return 1;
    }

    if (de_shell_destroy(&first) != DE_SHELL_STATUS_OK || first != NULL) {
        (void)fprintf(stderr, "first instance destruction failed\n");
        (void)de_shell_destroy(&second);
        return 1;
    }

    if (second == NULL) {
        (void)fprintf(stderr, "destroying first instance affected second instance\n");
        return 1;
    }

    if (de_shell_destroy(&second) != DE_SHELL_STATUS_OK || second != NULL) {
        (void)fprintf(stderr, "second instance destruction failed\n");
        return 1;
    }

    return 0;
}

int main(void)
{
    if (test_create_returns_owned_handle() != 0
        || test_create_rejects_null_output() != 0
        || test_destroy_rejects_null_handle_address() != 0
        || test_destroy_is_idempotent_for_cleared_handle() != 0
        || test_multiple_instances_have_independent_lifetimes() != 0) {
        return 1;
    }

    (void)puts("DE-Shell lifecycle tests passed");
    return 0;
}
