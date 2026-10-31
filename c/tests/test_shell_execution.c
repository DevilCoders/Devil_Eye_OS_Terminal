#include "de_shell/execution.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct executor_state {
    size_t calls;
    int return_exit_code;
    de_shell_status_t callback_status;
    int arguments_valid;
    int environment_valid;
    int reenter;
    de_shell_t **shell_handle;
    de_shell_status_t nested_status;
    de_shell_status_t destroy_status;
} executor_state_t;

static de_shell_status_t fake_executor(
    const de_shell_external_command_t *command,
    int *out_exit_code,
    void *user_data)
{
    executor_state_t *state = (executor_state_t *)user_data;

    state->calls += 1U;
    if (command == NULL || out_exit_code == NULL) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    if (command->argc == 4U
        && strcmp(command->executable, "tool") == 0
        && strcmp(command->argv[0], "tool") == 0
        && strcmp(command->argv[1], "two words") == 0
        && strcmp(command->argv[2], "escaped value") == 0
        && strcmp(command->argv[3], "") == 0
        && command->argv[command->argc] == NULL) {
        state->arguments_valid = 1;
    }

    if (command->environment_count == 1U
        && command->environment != NULL
        && strcmp(command->environment[0].key, "MODE") == 0
        && strcmp(command->environment[0].value, "test") == 0
        && command->working_directory != NULL
        && strcmp(command->working_directory, "/tmp/de-shell") == 0) {
        state->environment_valid = 1;
    }

    if (state->reenter != 0 && state->shell_handle != NULL) {
        static const unsigned char nested_source[] = "exit";
        const de_shell_execution_context_t nested_context = {
            NULL,
            0U,
            NULL,
            NULL,
            NULL,
        };
        de_shell_execution_result_t nested_result;
        de_shell_execution_diagnostic_t nested_diagnostic;

        state->nested_status = de_shell_execute_source(
            *state->shell_handle,
            nested_source,
            sizeof(nested_source) - 1U,
            &nested_context,
            &nested_result,
            &nested_diagnostic);
        state->destroy_status = de_shell_destroy(state->shell_handle);
    }

    *out_exit_code = strcmp(command->executable, "fail") == 0
        ? 7
        : state->return_exit_code;
    return state->callback_status;
}

static int test_builtin_exit_stops_sequence(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] = "exit; tool";
    const de_shell_execution_context_t context = {NULL, 0U, NULL, NULL, NULL};
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }

    if (de_shell_execute_source(
            shell,
            source,
            sizeof(source) - 1U,
            &context,
            &result,
            &diagnostic) != DE_SHELL_STATUS_OK
        || result.exit_code != 0
        || result.exit_requested != 1
        || result.commands_executed != 1U) {
        (void)fprintf(stderr, "exit built-in did not stop the sequence\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_external_arguments_environment_and_working_directory(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] =
        "tool 'two words' escaped\\ value ''";
    const de_shell_environment_entry_t environment[] = {
        {"MODE", "test"},
    };
    executor_state_t state = {0U, 23, DE_SHELL_STATUS_OK, 0, 0, 0, NULL,
        DE_SHELL_STATUS_OK, DE_SHELL_STATUS_OK};
    const de_shell_execution_context_t context = {
        environment,
        1U,
        "/tmp/de-shell",
        fake_executor,
        &state,
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }

    if (de_shell_execute_source(
            shell,
            source,
            sizeof(source) - 1U,
            &context,
            &result,
            &diagnostic) != DE_SHELL_STATUS_OK
        || state.calls != 1U
        || state.arguments_valid == 0
        || state.environment_valid == 0
        || result.exit_code != 23
        || result.exit_requested != 0
        || result.commands_executed != 1U) {
        (void)fprintf(stderr, "external command request contract failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_and_or_and_sequence_control_flow(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] = "ok && fail || ok; exit; never";
    executor_state_t state = {0U, 0, DE_SHELL_STATUS_OK, 0, 0, 0, NULL,
        DE_SHELL_STATUS_OK, DE_SHELL_STATUS_OK};
    const de_shell_execution_context_t context = {
        NULL, 0U, NULL, fake_executor, &state
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }

    /* The callback returns a non-zero status only for the command named "fail". */
    state.return_exit_code = 0;
    if (de_shell_execute_source(
            shell,
            source,
            sizeof(source) - 1U,
            &context,
            &result,
            &diagnostic) != DE_SHELL_STATUS_OK) {
        (void)fprintf(stderr, "control-flow execution failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    /* A non-zero left branch must select the right side of the OR node. */
    if (state.calls != 3U
        || result.exit_requested != 1
        || result.exit_code != 0
        || result.commands_executed != 4U) {
        (void)fprintf(stderr, "sequence/exit control flow was incorrect\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static de_shell_status_t failing_executor(
    const de_shell_external_command_t *command,
    int *out_exit_code,
    void *user_data)
{
    (void)command;
    (void)out_exit_code;
    (void)user_data;
    return DE_SHELL_STATUS_INTERNAL_ERROR;
}

static int test_external_failures_and_missing_executor(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] = "tool";
    const de_shell_execution_context_t failing_context = {
        NULL, 0U, NULL, failing_executor, NULL
    };
    const de_shell_execution_context_t missing_context = {
        NULL, 0U, NULL, NULL, NULL
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }

    if (de_shell_execute_source(
            shell,
            source,
            sizeof(source) - 1U,
            &failing_context,
            &result,
            &diagnostic) != DE_SHELL_STATUS_EXTERNAL_EXECUTION_FAILED
        || diagnostic.stage != DE_SHELL_EXECUTION_STAGE_EXTERNAL
        || diagnostic.code != (int)DE_SHELL_STATUS_INTERNAL_ERROR
        || result.commands_executed != 0U) {
        (void)fprintf(stderr, "external executor failure was not propagated\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_execute_source(
            shell,
            source,
            sizeof(source) - 1U,
            &missing_context,
            &result,
            &diagnostic) != DE_SHELL_STATUS_EXECUTION_UNAVAILABLE
        || diagnostic.stage != DE_SHELL_EXECUTION_STAGE_EXTERNAL) {
        (void)fprintf(stderr, "missing external executor was not reported\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_parse_errors_and_unsupported_nodes(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char malformed[] = "tool &&";
    static const unsigned char pipeline[] = "tool | tool";
    static const unsigned char redirection[] = "tool > output";
    static const unsigned char background[] = "tool &";
    executor_state_t state = {0U, 0, DE_SHELL_STATUS_OK, 0, 0, 0, NULL,
        DE_SHELL_STATUS_OK, DE_SHELL_STATUS_OK};
    const de_shell_execution_context_t context = {
        NULL, 0U, NULL, fake_executor, &state
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }

    if (de_shell_execute_source(
            shell, malformed, sizeof(malformed) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_PARSE_ERROR
        || diagnostic.stage != DE_SHELL_EXECUTION_STAGE_PARSE
        || state.calls != 0U) {
        (void)fprintf(stderr, "malformed input was not rejected before execution\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_execute_source(
            shell, pipeline, sizeof(pipeline) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_UNSUPPORTED_OPERATION
        || state.calls != 0U) {
        (void)fprintf(stderr, "pipeline was executed before backend support exists\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_execute_source(
            shell, redirection, sizeof(redirection) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_UNSUPPORTED_OPERATION
        || state.calls != 0U) {
        (void)fprintf(stderr, "redirection was executed before backend support exists\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_execute_source(
            shell, background, sizeof(background) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_UNSUPPORTED_OPERATION
        || state.calls != 0U) {
        (void)fprintf(stderr, "background command was executed without a job backend\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_reentrancy_and_destroy_guard(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] = "tool";
    executor_state_t state = {0U, 0, DE_SHELL_STATUS_OK, 0, 0, 1, NULL,
        DE_SHELL_STATUS_OK, DE_SHELL_STATUS_OK};
    de_shell_execution_context_t context = {
        NULL, 0U, NULL, fake_executor, &state
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }
    state.shell_handle = &shell;

    if (de_shell_execute_source(
            shell,
            source,
            sizeof(source) - 1U,
            &context,
            &result,
            &diagnostic) != DE_SHELL_STATUS_OK
        || state.nested_status != DE_SHELL_STATUS_BUSY
        || state.destroy_status != DE_SHELL_STATUS_BUSY
        || shell == NULL
        || result.commands_executed != 1U) {
        (void)fprintf(stderr, "execution reentrancy/destruction guard failed\n");
        if (shell != NULL) {
            (void)de_shell_destroy(&shell);
        }
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_context_validation_and_resource_limit(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] = "tool";
    const de_shell_environment_entry_t invalid_environment[] = {
        {"BAD=KEY", "value"},
    };
    const de_shell_execution_context_t invalid_context = {
        invalid_environment, 1U, NULL, fake_executor, NULL
    };
    const de_shell_execution_context_t empty_context = {
        NULL, 0U, NULL, NULL, NULL
    };
    unsigned char *oversized = NULL;
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;
    de_shell_status_t status;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }

    if (de_shell_execute_source(
            shell, source, sizeof(source) - 1U,
            &invalid_context, &result, &diagnostic)
        != DE_SHELL_STATUS_INVALID_ARGUMENT) {
        (void)fprintf(stderr, "invalid environment key was accepted\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    oversized = malloc(DE_SHELL_EXEC_MAX_SOURCE_BYTES + 1U);
    if (oversized == NULL) {
        (void)de_shell_destroy(&shell);
        return 1;
    }
    (void)memset(oversized, (int)'x', DE_SHELL_EXEC_MAX_SOURCE_BYTES + 1U);

    status = de_shell_execute_source(
        shell,
        oversized,
        DE_SHELL_EXEC_MAX_SOURCE_BYTES + 1U,
        &empty_context,
        &result,
        &diagnostic);
    free(oversized);

    if (status != DE_SHELL_STATUS_RESOURCE_LIMIT
        || diagnostic.stage != DE_SHELL_EXECUTION_STAGE_INPUT) {
        (void)fprintf(stderr, "source resource limit was not enforced\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_execute_source(
            shell, NULL, 0U, &empty_context, &result, &diagnostic)
        != DE_SHELL_STATUS_OK
        || result.exit_code != 0
        || result.commands_executed != 0U) {
        (void)fprintf(stderr, "empty source should be a successful no-op\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

int main(void)
{
    if (test_builtin_exit_stops_sequence() != 0
        || test_external_arguments_environment_and_working_directory() != 0
        || test_and_or_and_sequence_control_flow() != 0
        || test_external_failures_and_missing_executor() != 0
        || test_parse_errors_and_unsupported_nodes() != 0
        || test_reentrancy_and_destroy_guard() != 0
        || test_context_validation_and_resource_limit() != 0) {
        return 1;
    }

    (void)puts("DE-Shell execution engine tests passed");
    return 0;
}
