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


typedef struct expansion_expectation {
    const char *expected_argv[8];
    size_t expected_argc;
    const char *expected_env_key;
    const char *expected_env_value;
    size_t expected_env_count;
    int return_exit_code;
    size_t calls;
    int matched;
} expansion_expectation_t;

static de_shell_status_t expansion_executor(
    const de_shell_external_command_t *command,
    int *out_exit_code,
    void *user_data)
{
    expansion_expectation_t *expectation = (expansion_expectation_t *)user_data;
    size_t index;

    expectation->calls += 1U;
    expectation->matched = 0;
    if (command == NULL || out_exit_code == NULL
        || command->argc != expectation->expected_argc
        || command->environment_count != expectation->expected_env_count) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    for (index = 0U; index < command->argc; index += 1U) {
        if (strcmp(command->argv[index], expectation->expected_argv[index]) != 0) {
            return DE_SHELL_STATUS_INVALID_ARGUMENT;
        }
    }

    if (expectation->expected_env_count != 0U) {
        int found = 0;
        for (index = 0U; index < command->environment_count; index += 1U) {
            if (strcmp(command->environment[index].key, expectation->expected_env_key) == 0
                && strcmp(command->environment[index].value, expectation->expected_env_value) == 0) {
                found = 1;
            }
        }
        if (found == 0) {
            return DE_SHELL_STATUS_INVALID_ARGUMENT;
        }
    }

    expectation->matched = 1;
    *out_exit_code = expectation->return_exit_code;
    return DE_SHELL_STATUS_OK;
}

typedef struct status_sequence_state {
    size_t calls;
} status_sequence_state_t;

static de_shell_status_t status_sequence_executor(
    const de_shell_external_command_t *command,
    int *out_exit_code,
    void *user_data)
{
    status_sequence_state_t *state = (status_sequence_state_t *)user_data;

    if (command == NULL || out_exit_code == NULL) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }
    if (state->calls == 0U) {
        if (command->argc != 1U || strcmp(command->argv[0], "fail") != 0) {
            return DE_SHELL_STATUS_INVALID_ARGUMENT;
        }
        *out_exit_code = 7;
    } else if (state->calls == 1U) {
        if (command->argc != 2U
            || strcmp(command->argv[0], "tool") != 0
            || strcmp(command->argv[1], "7") != 0) {
            return DE_SHELL_STATUS_INVALID_ARGUMENT;
        }
        *out_exit_code = 0;
    } else {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }
    state->calls += 1U;
    return DE_SHELL_STATUS_OK;
}

static int test_parameter_expansion_and_persistent_variables(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char assignment[] = "A='two words'; B=$A";
    static const unsigned char expansion[] =
        "tool \"$A\" '$A' \\$A ${A} $MISSING \"$HOST\"";
    const de_shell_environment_entry_t environment[] = {
        {"HOST", "host-value"},
    };
    expansion_expectation_t expectation = {
        {"tool", "two words", "two words", "$A", "$A", "two words", "", "host-value"},
        8U, "HOST", "host-value", 1U, 0, 0U, 0
    };
    const de_shell_execution_context_t context = {
        environment, 1U, NULL, expansion_executor, &expectation
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }
    if (de_shell_execute_source(
            shell, assignment, sizeof(assignment) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || result.commands_executed != 1U
        || result.exit_code != 0) {
        (void)de_shell_destroy(&shell);
        return 1;
    }
    if (de_shell_execute_source(
            shell, expansion, sizeof(expansion) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || expectation.calls != 1U
        || expectation.matched == 0) {
        (void)fprintf(stderr, "parameter expansion/quote behavior failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_command_prefix_environment_is_temporary(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char prefix[] = "MODE=temporary tool";
    static const unsigned char later[] = "tool \"$MODE\"";
    const de_shell_environment_entry_t environment[] = {
        {"MODE", "base"},
    };
    expansion_expectation_t expectation = {
        {"tool", NULL, NULL, NULL, NULL, NULL, NULL, NULL},
        1U, "MODE", "temporary", 1U, 0, 0U, 0
    };
    const de_shell_execution_context_t context = {
        environment, 1U, NULL, expansion_executor, &expectation
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }
    if (de_shell_execute_source(
            shell, prefix, sizeof(prefix) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || expectation.calls != 1U
        || expectation.matched == 0) {
        (void)de_shell_destroy(&shell);
        return 1;
    }

    expectation.expected_argv[0] = "tool";
    expectation.expected_argv[1] = "base";
    expectation.expected_argc = 2U;
    expectation.calls = 0U;
    expectation.matched = 0;
    expectation.expected_env_value = "base";

    if (de_shell_execute_source(
            shell, later, sizeof(later) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || expectation.calls != 1U
        || expectation.matched == 0) {
        (void)fprintf(stderr, "command-prefix environment leaked into the shell\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_non_recursive_expansion(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char assignments[] = "OTHER=real; NAME='$OTHER'";
    static const unsigned char expansion[] = "tool \"$NAME\"";
    expansion_expectation_t expectation = {
        {"tool", "$OTHER"}, 2U, NULL, NULL, 0U, 0, 0U, 0
    };
    const de_shell_execution_context_t context = {
        NULL, 0U, NULL, expansion_executor, &expectation
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }
    if (de_shell_execute_source(
            shell, assignments, sizeof(assignments) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || de_shell_execute_source(
            shell, expansion, sizeof(expansion) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || expectation.calls != 1U
        || expectation.matched == 0) {
        (void)fprintf(stderr, "variable values were recursively expanded\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_last_status_and_skipped_operand_expansion(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char failure_source[] = "fail";
    static const unsigned char status_source[] = "tool \"$?\"";
    static const unsigned char skipped_source[] = "fail && tool ${}";
    status_sequence_state_t state = {0U};
    const de_shell_execution_context_t status_context = {
        NULL, 0U, NULL, status_sequence_executor, &state
    };
    expansion_expectation_t skipped = {
        {"fail"}, 1U, NULL, NULL, 0U, 7, 0U, 0
    };
    const de_shell_execution_context_t skipped_context = {
        NULL, 0U, NULL, expansion_executor, &skipped
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }
    if (de_shell_execute_source(
            shell, failure_source, sizeof(failure_source) - 1U,
            &status_context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || result.exit_code != 7
        || de_shell_execute_source(
            shell, status_source, sizeof(status_source) - 1U,
            &status_context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || state.calls != 2U
        || result.exit_code != 0) {
        (void)fprintf(stderr, "last command status expansion failed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    if (de_shell_execute_source(
            shell, skipped_source, sizeof(skipped_source) - 1U,
            &skipped_context, &result, &diagnostic) != DE_SHELL_STATUS_OK
        || skipped.calls != 1U
        || skipped.matched == 0
        || result.exit_code != 7) {
        (void)fprintf(stderr, "skipped conditional operand was expanded or executed\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}

static int test_expansion_output_limit(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] = "tool \"$LONG\"x";
    static char long_value[DE_SHELL_EXEC_MAX_WORD_BYTES + 1U];
    const de_shell_environment_entry_t environment[] = {
        {"LONG", long_value},
    };
    expansion_expectation_t expectation = {
        {"tool", NULL, NULL, NULL, NULL, NULL, NULL, NULL},
        1U, "LONG", long_value, 1U, 0, 0U, 0
    };
    const de_shell_execution_context_t context = {
        environment, 1U, NULL, expansion_executor, &expectation
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    (void)memset(long_value, 'x', sizeof(long_value) - 1U);
    long_value[sizeof(long_value) - 1U] = '\0';

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }
    if (de_shell_execute_source(
            shell, source, sizeof(source) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_RESOURCE_LIMIT
        || diagnostic.stage != DE_SHELL_EXECUTION_STAGE_EXPAND
        || expectation.calls != 0U) {
        (void)fprintf(stderr, "expansion byte limit was not enforced\n");
        (void)de_shell_destroy(&shell);
        return 1;
    }

    return de_shell_destroy(&shell) == DE_SHELL_STATUS_OK ? 0 : 1;
}


static int test_invalid_braced_expansion_is_rejected(void)
{
    de_shell_t *shell = NULL;
    static const unsigned char source[] = "tool ${}";
    expansion_expectation_t expectation = {
        {"tool", NULL, NULL, NULL, NULL, NULL, NULL, NULL},
        1U, NULL, NULL, 0U, 0, 0U, 0
    };
    const de_shell_execution_context_t context = {
        NULL, 0U, NULL, expansion_executor, &expectation
    };
    de_shell_execution_result_t result;
    de_shell_execution_diagnostic_t diagnostic;

    if (de_shell_create(&shell) != DE_SHELL_STATUS_OK) {
        return 1;
    }
    if (de_shell_execute_source(
            shell, source, sizeof(source) - 1U,
            &context, &result, &diagnostic) != DE_SHELL_STATUS_PARSE_ERROR
        || diagnostic.stage != DE_SHELL_EXECUTION_STAGE_EXPAND
        || expectation.calls != 0U) {
        (void)fprintf(stderr, "malformed braced expansion was not rejected\\n");
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
        || test_context_validation_and_resource_limit() != 0
        || test_parameter_expansion_and_persistent_variables() != 0
        || test_command_prefix_environment_is_temporary() != 0
        || test_non_recursive_expansion() != 0
        || test_last_status_and_skipped_operand_expansion() != 0
        || test_expansion_output_limit() != 0
        || test_invalid_braced_expansion_is_rejected() != 0) {
        return 1;
    }

    (void)puts("DE-Shell execution engine tests passed");
    return 0;
}
