#include "internal/command_registry.h"
#include "internal/shell_internal.h"

#include <stdio.h>
#include <string.h>

typedef struct handler_state {
    size_t calls;
    const char *last_argument;
} handler_state_t;

static de_shell_command_result_t record_handler(
    de_shell_t *shell,
    size_t argc,
    const char *const argv[],
    void *user_data)
{
    handler_state_t *state = (handler_state_t *)user_data;

    (void)shell;
    state->calls += 1U;
    state->last_argument = argc > 1U ? argv[1] : NULL;
    return DE_SHELL_COMMAND_RESULT_SUCCESS;
}

static int test_registration_and_lookup(void)
{
    de_shell_command_registry_t *registry = NULL;
    const de_shell_command_info_t *info = NULL;
    handler_state_t state = {0U, NULL};
    const de_shell_command_definition_t definition = {
        "greet",
        "Greet a named target.",
        1U,
        2U,
        DE_SHELL_COMMAND_FLAG_NONE,
        record_handler,
        &state,
    };

    if (de_shell_command_registry_create(&registry) != DE_SHELL_REGISTRY_STATUS_OK
        || registry == NULL
        || de_shell_command_registry_register(registry, &definition)
            != DE_SHELL_REGISTRY_STATUS_OK
        || de_shell_command_registry_lookup(registry, "greet", &info)
            != DE_SHELL_REGISTRY_STATUS_OK
        || info == NULL
        || strcmp(info->name, "greet") != 0
        || strcmp(info->description, "Greet a named target.") != 0
        || info->min_args != 1U
        || info->max_args != 2U) {
        (void)fprintf(stderr, "registration/lookup test failed\n");
        (void)de_shell_command_registry_destroy(&registry);
        return 1;
    }

    if (de_shell_command_registry_lookup(registry, "unknown", &info)
            != DE_SHELL_REGISTRY_STATUS_NOT_FOUND
        || info != NULL) {
        (void)fprintf(stderr, "unknown lookup test failed\n");
        (void)de_shell_command_registry_destroy(&registry);
        return 1;
    }

    (void)de_shell_command_registry_destroy(&registry);
    return registry == NULL ? 0 : 1;
}

static int test_duplicate_collision_is_atomic(void)
{
    de_shell_command_registry_t *registry = NULL;
    const de_shell_command_info_t *info = NULL;
    handler_state_t state = {0U, NULL};
    const de_shell_command_definition_t first = {
        "status",
        "First definition.",
        0U,
        0U,
        DE_SHELL_COMMAND_FLAG_NONE,
        record_handler,
        &state,
    };
    const de_shell_command_definition_t duplicate = {
        "status",
        "Replacement definition.",
        0U,
        0U,
        DE_SHELL_COMMAND_FLAG_NONE,
        record_handler,
        &state,
    };

    if (de_shell_command_registry_create(&registry) != DE_SHELL_REGISTRY_STATUS_OK
        || de_shell_command_registry_register(registry, &first)
            != DE_SHELL_REGISTRY_STATUS_OK
        || de_shell_command_registry_register(registry, &duplicate)
            != DE_SHELL_REGISTRY_STATUS_DUPLICATE_NAME
        || de_shell_command_registry_lookup(registry, "status", &info)
            != DE_SHELL_REGISTRY_STATUS_OK
        || info == NULL
        || strcmp(info->description, "First definition.") != 0) {
        (void)fprintf(stderr, "duplicate collision test failed\n");
        (void)de_shell_command_registry_destroy(&registry);
        return 1;
    }

    (void)de_shell_command_registry_destroy(&registry);
    return 0;
}

static int test_dispatch_is_deterministic(void)
{
    de_shell_t *shell = NULL;
    handler_state_t state = {0U, NULL};
    const de_shell_command_definition_t definition = {
        "greet",
        "Greet a named target.",
        1U,
        2U,
        DE_SHELL_COMMAND_FLAG_NONE,
        record_handler,
        &state,
    };
    const char *const argv[] = {"greet", "Ada"};
    de_shell_command_result_t result = DE_SHELL_COMMAND_RESULT_FAILURE;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK
        || shell == NULL
        || de_shell_command_registry_register(shell->command_registry, &definition)
            != DE_SHELL_REGISTRY_STATUS_OK) {
        (void)fprintf(stderr, "dispatch setup failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_command_registry_dispatch(
            shell->command_registry, shell, 2U, argv, &result)
            != DE_SHELL_REGISTRY_STATUS_OK
        || result != DE_SHELL_COMMAND_RESULT_SUCCESS
        || state.calls != 1U
        || state.last_argument == NULL
        || strcmp(state.last_argument, "Ada") != 0) {
        (void)fprintf(stderr, "first dispatch failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_command_registry_dispatch(
            shell->command_registry, shell, 2U, argv, &result)
            != DE_SHELL_REGISTRY_STATUS_OK
        || state.calls != 2U) {
        (void)fprintf(stderr, "repeated dispatch was not deterministic\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    (void)de_shell_destroy(&shell);
    return shell == NULL ? 0 : 1;
}

static int test_argument_count_and_unknown_dispatch(void)
{
    de_shell_t *shell = NULL;
    handler_state_t state = {0U, NULL};
    const de_shell_command_definition_t definition = {
        "needs-argument",
        NULL,
        1U,
        1U,
        DE_SHELL_COMMAND_FLAG_NONE,
        record_handler,
        &state,
    };
    const char *const no_arguments[] = {"needs-argument"};
    const char *const unknown[] = {"not-registered"};
    de_shell_command_result_t result = DE_SHELL_COMMAND_RESULT_SUCCESS;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK
        || de_shell_command_registry_register(shell->command_registry, &definition)
            != DE_SHELL_REGISTRY_STATUS_OK) {
        (void)fprintf(stderr, "argument validation setup failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_command_registry_dispatch(
            shell->command_registry, shell, 1U, no_arguments, &result)
            != DE_SHELL_REGISTRY_STATUS_ARGUMENT_COUNT_MISMATCH
        || state.calls != 0U
        || result != DE_SHELL_COMMAND_RESULT_FAILURE) {
        (void)fprintf(stderr, "argument count validation failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_command_registry_dispatch(
            shell->command_registry, shell, 1U, unknown, &result)
            != DE_SHELL_REGISTRY_STATUS_NOT_FOUND
        || state.calls != 0U) {
        (void)fprintf(stderr, "unknown dispatch test failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    (void)de_shell_destroy(&shell);
    return 0;
}

static int test_core_exit_builtin_and_instance_isolation(void)
{
    de_shell_t *first = NULL;
    de_shell_t *second = NULL;
    const de_shell_command_info_t *info = NULL;
    const char *const exit_argv[] = {"exit"};
    de_shell_command_result_t result = DE_SHELL_COMMAND_RESULT_FAILURE;

    if (de_shell_create(&first) != DE_SHELL_STATUS_OK
        || de_shell_create(&second) != DE_SHELL_STATUS_OK) {
        (void)fprintf(stderr, "shell creation failed\n");
        (void)de_shell_destroy(&first);
        (void)de_shell_destroy(&second);
        return 1;
    }

    if (de_shell_command_registry_lookup(
            first->command_registry, "exit", &info)
            != DE_SHELL_REGISTRY_STATUS_OK
        || info == NULL
        || (info->flags & DE_SHELL_COMMAND_FLAG_TERMINATES_SHELL) == 0U
        || de_shell_command_registry_dispatch(
            first->command_registry, first, 1U, exit_argv, &result)
            != DE_SHELL_REGISTRY_STATUS_OK
        || result != DE_SHELL_COMMAND_RESULT_EXIT_REQUESTED) {
        (void)fprintf(stderr, "core exit dispatch failed\n");
        (void)de_shell_destroy(&first);
        (void)de_shell_destroy(&second);
        return 1;
    }

    if (de_shell_command_registry_lookup(
            second->command_registry, "exit", &info)
            != DE_SHELL_REGISTRY_STATUS_OK) {
        (void)fprintf(stderr, "core commands must be registered per shell\n");
        (void)de_shell_destroy(&first);
        (void)de_shell_destroy(&second);
        return 1;
    }

    (void)de_shell_destroy(&first);
    (void)de_shell_destroy(&second);
    return 0;
}

static int test_invalid_arguments_and_cleanup(void)
{
    de_shell_command_registry_t *registry = NULL;
    const de_shell_command_info_t *info = (const de_shell_command_info_t *)1;
    handler_state_t state = {0U, NULL};
    const de_shell_command_definition_t invalid = {
        "bad name",
        "Invalid name.",
        0U,
        0U,
        DE_SHELL_COMMAND_FLAG_NONE,
        record_handler,
        &state,
    };

    if (de_shell_command_registry_create(NULL)
        != DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT
        || de_shell_command_registry_create(&registry)
            != DE_SHELL_REGISTRY_STATUS_OK
        || de_shell_command_registry_register(registry, &invalid)
            != DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT
        || de_shell_command_registry_lookup(registry, "bad name", &info)
            != DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT
        || info != NULL
        || de_shell_command_registry_destroy(&registry)
            != DE_SHELL_REGISTRY_STATUS_OK
        || registry != NULL
        || de_shell_command_registry_destroy(&registry)
            != DE_SHELL_REGISTRY_STATUS_OK
        || de_shell_command_registry_destroy(NULL)
            != DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT) {
        (void)fprintf(stderr, "invalid argument/cleanup test failed\n");
        (void)de_shell_command_registry_destroy(&registry);
        return 1;
    }

    return 0;
}

int main(void)
{
    if (test_registration_and_lookup() != 0
        || test_duplicate_collision_is_atomic() != 0
        || test_dispatch_is_deterministic() != 0
        || test_argument_count_and_unknown_dispatch() != 0
        || test_core_exit_builtin_and_instance_isolation() != 0
        || test_invalid_arguments_and_cleanup() != 0) {
        return 1;
    }

    (void)puts("DE-Shell command registry tests passed");
    return 0;
}
