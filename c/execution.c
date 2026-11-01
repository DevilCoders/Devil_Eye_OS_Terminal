#include "de_shell/execution.h"

#include "internal/command_registry.h"
#include "internal/lexer.h"
#include "internal/parser.h"
#include "internal/shell_internal.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define DE_SHELL_EXEC_MAX_TOKENS ((size_t)4096U)
#define DE_SHELL_EXEC_MAX_AST_NODES ((size_t)4096U)
#define DE_SHELL_EXEC_MAX_WORDS ((size_t)4096U)
#define DE_SHELL_EXEC_MAX_REDIRECTIONS ((size_t)1024U)
#define DE_SHELL_EXEC_MAX_ENVIRONMENT_ENTRIES ((size_t)256U)
#define DE_SHELL_EXEC_MAX_ENVIRONMENT_KEY_BYTES ((size_t)256U)
#define DE_SHELL_EXEC_MAX_ENVIRONMENT_VALUE_BYTES ((size_t)4096U)
#define DE_SHELL_EXEC_MAX_PATH_BYTES ((size_t)4096U)
#define DE_SHELL_EXEC_MAX_VARIABLE_NAME_BYTES ((size_t)256U)
#define DE_SHELL_EXEC_MAX_VARIABLES ((size_t)DE_SHELL_INTERNAL_MAX_VARIABLES)

typedef enum de_shell_quote_state {
    DE_SHELL_QUOTE_STATE_NONE = 0,
    DE_SHELL_QUOTE_STATE_SINGLE = 1,
    DE_SHELL_QUOTE_STATE_DOUBLE = 2
} de_shell_quote_state_t;

typedef struct de_shell_execution_frame {
    size_t node_index;
    unsigned int state;
    int left_exit_code;
    int left_exit_requested;
} de_shell_execution_frame_t;

typedef struct de_shell_eval_result {
    int exit_code;
    int exit_requested;
} de_shell_eval_result_t;

static void de_shell_set_diagnostic(
    de_shell_execution_diagnostic_t *diagnostic,
    de_shell_execution_stage_t stage,
    int code,
    size_t source_offset)
{
    diagnostic->stage = stage;
    diagnostic->code = code;
    diagnostic->source_offset = source_offset;
}

static size_t de_shell_bounded_length(const char *value, size_t maximum)
{
    size_t length = 0U;

    while (length <= maximum && value[length] != '\0') {
        length += 1U;
    }

    return length;
}

static int de_shell_execution_context_is_valid(
    const de_shell_execution_context_t *context)
{
    size_t index;
    size_t path_length;

    if (context == NULL
        || (context->environment_count != 0U && context->environment == NULL)
        || context->environment_count > DE_SHELL_EXEC_MAX_ENVIRONMENT_ENTRIES) {
        return 0;
    }

    if (context->working_directory != NULL) {
        path_length = de_shell_bounded_length(
            context->working_directory, DE_SHELL_EXEC_MAX_PATH_BYTES);
        if (path_length == 0U || path_length > DE_SHELL_EXEC_MAX_PATH_BYTES) {
            return 0;
        }
    }

    for (index = 0U; index < context->environment_count; index += 1U) {
        const char *key = context->environment[index].key;
        const char *value = context->environment[index].value;
        size_t key_length;
        size_t value_length;

        if (key == NULL || value == NULL) {
            return 0;
        }

        key_length = de_shell_bounded_length(
            key, DE_SHELL_EXEC_MAX_ENVIRONMENT_KEY_BYTES);
        value_length = de_shell_bounded_length(
            value, DE_SHELL_EXEC_MAX_ENVIRONMENT_VALUE_BYTES);

        if (key_length == 0U
            || key_length > DE_SHELL_EXEC_MAX_ENVIRONMENT_KEY_BYTES
            || value_length > DE_SHELL_EXEC_MAX_ENVIRONMENT_VALUE_BYTES
            || strchr(key, '=') != NULL) {
            return 0;
        }
    }

    return 1;
}


static int de_shell_identifier_start(unsigned char value)
{
    return (value >= (unsigned char)'A' && value <= (unsigned char)'Z')
        || (value >= (unsigned char)'a' && value <= (unsigned char)'z')
        || value == (unsigned char)'_';
}

static int de_shell_identifier_part(unsigned char value)
{
    return de_shell_identifier_start(value)
        || (value >= (unsigned char)'0' && value <= (unsigned char)'9');
}

static de_shell_status_t de_shell_append_expansion(
    char *storage,
    size_t storage_capacity,
    size_t output_start,
    size_t *output_length,
    const char *value,
    size_t value_length)
{
    if (value_length > DE_SHELL_EXEC_MAX_WORD_BYTES - *output_length
        || output_start > storage_capacity
        || *output_length > storage_capacity - output_start
        || value_length >= storage_capacity - output_start - *output_length) {
        return DE_SHELL_STATUS_RESOURCE_LIMIT;
    }

    if (value_length != 0U) {
        (void)memcpy(
            &storage[output_start + *output_length],
            value,
            value_length);
    }
    *output_length += value_length;
    return DE_SHELL_STATUS_OK;
}

static const char *de_shell_lookup_variable(
    const de_shell_t *shell,
    const de_shell_execution_context_t *context,
    const char *name,
    size_t name_length)
{
    size_t index;

    for (index = 0U; index < shell->variable_count; index += 1U) {
        if (strlen(shell->variables[index].name) == name_length
            && memcmp(shell->variables[index].name, name, name_length) == 0) {
            return shell->variables[index].value;
        }
    }

    for (index = 0U; index < context->environment_count; index += 1U) {
        if (strlen(context->environment[index].key) == name_length
            && memcmp(context->environment[index].key, name, name_length) == 0) {
            return context->environment[index].value;
        }
    }

    return "";
}

static de_shell_status_t de_shell_set_variable(
    de_shell_t *shell,
    const char *name,
    size_t name_length,
    const char *value)
{
    size_t index;
    size_t value_length;
    char *replacement;

    if (name_length == 0U
        || name_length > DE_SHELL_EXEC_MAX_VARIABLE_NAME_BYTES
        || !de_shell_identifier_start((unsigned char)name[0])) {
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }
    for (index = 1U; index < name_length; index += 1U) {
        if (!de_shell_identifier_part((unsigned char)name[index])) {
            return DE_SHELL_STATUS_INVALID_ARGUMENT;
        }
    }

    value_length = strlen(value);
    if (value_length > DE_SHELL_EXEC_MAX_ENVIRONMENT_VALUE_BYTES) {
        return DE_SHELL_STATUS_RESOURCE_LIMIT;
    }

    replacement = malloc(value_length + 1U);
    if (replacement == NULL) {
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }
    (void)memcpy(replacement, value, value_length + 1U);

    for (index = 0U; index < shell->variable_count; index += 1U) {
        if (strlen(shell->variables[index].name) == name_length
            && memcmp(shell->variables[index].name, name, name_length) == 0) {
            free(shell->variables[index].value);
            shell->variables[index].value = replacement;
            return DE_SHELL_STATUS_OK;
        }
    }

    if (shell->variable_count >= DE_SHELL_EXEC_MAX_VARIABLES) {
        free(replacement);
        return DE_SHELL_STATUS_RESOURCE_LIMIT;
    }

    {
        char *owned_name = malloc(name_length + 1U);
        if (owned_name == NULL) {
            free(replacement);
            return DE_SHELL_STATUS_INTERNAL_ERROR;
        }
        (void)memcpy(owned_name, name, name_length);
        owned_name[name_length] = '\0';
        shell->variables[shell->variable_count].name = owned_name;
        shell->variables[shell->variable_count].value = replacement;
        shell->variable_count += 1U;
    }

    return DE_SHELL_STATUS_OK;
}

static int de_shell_assignment_name_length(
    const unsigned char *source,
    const de_shell_word_ref_t *word,
    size_t *out_name_length)
{
    size_t index = 0U;
    const size_t end = word->source_offset + word->source_length;

    if (word->source_offset >= end
        || !de_shell_identifier_start(source[word->source_offset])) {
        return 0;
    }

    index = word->source_offset + 1U;
    while (index < end && de_shell_identifier_part(source[index])) {
        index += 1U;
    }

    if (index >= end || source[index] != (unsigned char)'=') {
        return 0;
    }

    *out_name_length = index - word->source_offset;
    return *out_name_length <= DE_SHELL_EXEC_MAX_VARIABLE_NAME_BYTES;
}

static de_shell_status_t de_shell_decode_word(
    const unsigned char *source,
    size_t source_length,
    const de_shell_word_ref_t *word,
    de_shell_t *shell,
    const de_shell_execution_context_t *context,
    int last_status,
    char *storage,
    size_t storage_capacity,
    size_t *storage_offset,
    const char **out_word)
{
    size_t source_index;
    size_t source_end;
    size_t output_length = 0U;
    size_t output_start;
    de_shell_quote_state_t quote_state = DE_SHELL_QUOTE_STATE_NONE;

    if (source == NULL || word == NULL || storage == NULL
        || storage_offset == NULL || out_word == NULL
        || word->source_offset > source_length
        || word->source_length > source_length - word->source_offset
        || *storage_offset >= storage_capacity) {
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }

    source_index = word->source_offset;
    source_end = word->source_offset + word->source_length;
    output_start = *storage_offset;
    *out_word = &storage[output_start];

    while (source_index < source_end) {
        const unsigned char value = source[source_index];

        if (quote_state == DE_SHELL_QUOTE_STATE_SINGLE) {
            if (value == (unsigned char)'\'') {
                quote_state = DE_SHELL_QUOTE_STATE_NONE;
                source_index += 1U;
                continue;
            }
        } else if (quote_state == DE_SHELL_QUOTE_STATE_DOUBLE) {
            if (value == (unsigned char)'"') {
                quote_state = DE_SHELL_QUOTE_STATE_NONE;
                source_index += 1U;
                continue;
            }

            if (value == (unsigned char)'\\'
                && source_index + 1U < source_end) {
                const unsigned char next = source[source_index + 1U];

                if (next == (unsigned char)'\n') {
                    source_index += 2U;
                    continue;
                }
                if (next == (unsigned char)'"'
                    || next == (unsigned char)'\\'
                    || next == (unsigned char)'$'
                    || next == (unsigned char)0x60U) {
                    if (output_length >= DE_SHELL_EXEC_MAX_WORD_BYTES) {
                        return DE_SHELL_STATUS_RESOURCE_LIMIT;
                    }
                    storage[output_start + output_length] = (char)next;
                    output_length += 1U;
                    source_index += 2U;
                    continue;
                }
            }
        } else {
            if (value == (unsigned char)'\'') {
                quote_state = DE_SHELL_QUOTE_STATE_SINGLE;
                source_index += 1U;
                continue;
            }
            if (value == (unsigned char)'"') {
                quote_state = DE_SHELL_QUOTE_STATE_DOUBLE;
                source_index += 1U;
                continue;
            }
            if (value == (unsigned char)'\\') {
                if (source_index + 1U >= source_end) {
                    return DE_SHELL_STATUS_PARSE_ERROR;
                }
                if (source[source_index + 1U] == (unsigned char)'\n') {
                    source_index += 2U;
                    continue;
                }
                if (output_length >= DE_SHELL_EXEC_MAX_WORD_BYTES) {
                    return DE_SHELL_STATUS_RESOURCE_LIMIT;
                }
                storage[output_start + output_length] =
                    (char)source[source_index + 1U];
                output_length += 1U;
                source_index += 2U;
                continue;
            }
        }


        if (quote_state != DE_SHELL_QUOTE_STATE_SINGLE
            && value == (unsigned char)'$') {
            size_t name_start = source_index + 1U;
            size_t name_end = name_start;
            const char *replacement = "";
            char status_text[32];
            size_t replacement_length = 0U;
            int supported = 0;

            if (name_start < source_end
                && source[name_start] == (unsigned char)'?') {
                const int written = snprintf(status_text, sizeof(status_text), "%d", last_status);
                if (written < 0 || (size_t)written >= sizeof(status_text)) {
                    return DE_SHELL_STATUS_INTERNAL_ERROR;
                }
                replacement = status_text;
                replacement_length = (size_t)written;
                source_index += 2U;
                supported = 1;
            } else if (name_start < source_end
                && source[name_start] == (unsigned char)'{') {
                name_start += 1U;
                name_end = name_start;
                while (name_end < source_end
                    && source[name_end] != (unsigned char)'}') {
                    name_end += 1U;
                }
                if (name_end >= source_end || name_end == name_start
                    || !de_shell_identifier_start(source[name_start])) {
                    return DE_SHELL_STATUS_PARSE_ERROR;
                }
                {
                    size_t name_index;
                    for (name_index = name_start + 1U; name_index < name_end; name_index += 1U) {
                        if (!de_shell_identifier_part(source[name_index])) {
                            return DE_SHELL_STATUS_PARSE_ERROR;
                        }
                    }
                }
                replacement = de_shell_lookup_variable(
                    shell, context, (const char *)&source[name_start], name_end - name_start);
                replacement_length = strlen(replacement);
                source_index = name_end + 1U;
                supported = 1;
            } else if (name_start < source_end
                && de_shell_identifier_start(source[name_start])) {
                name_end = name_start + 1U;
                while (name_end < source_end && de_shell_identifier_part(source[name_end])) {
                    name_end += 1U;
                }
                replacement = de_shell_lookup_variable(
                    shell, context, (const char *)&source[name_start], name_end - name_start);
                replacement_length = strlen(replacement);
                source_index = name_end;
                supported = 1;
            }

            if (supported != 0) {
                const de_shell_status_t append_status = de_shell_append_expansion(
                    storage,
                    storage_capacity,
                    output_start,
                    &output_length,
                    replacement,
                    replacement_length);
                if (append_status != DE_SHELL_STATUS_OK) {
                    return append_status;
                }
                continue;
            }
        }

        if (output_length >= DE_SHELL_EXEC_MAX_WORD_BYTES) {
            return DE_SHELL_STATUS_RESOURCE_LIMIT;
        }
        storage[output_start + output_length] = (char)value;
        output_length += 1U;
        source_index += 1U;
    }

    if (quote_state != DE_SHELL_QUOTE_STATE_NONE
        || output_start > storage_capacity
        || output_length >= storage_capacity - output_start) {
        return quote_state != DE_SHELL_QUOTE_STATE_NONE
            ? DE_SHELL_STATUS_PARSE_ERROR
            : DE_SHELL_STATUS_RESOURCE_LIMIT;
    }

    storage[output_start + output_length] = '\0';
    *storage_offset = output_start + output_length + 1U;
    return DE_SHELL_STATUS_OK;
}

static de_shell_status_t de_shell_execute_simple_command(
    de_shell_t *shell,
    const unsigned char *source,
    size_t source_length,
    const de_shell_ast_t *ast,
    const de_shell_ast_node_t *node,
    const de_shell_execution_context_t *context,
    const char **argv,
    char *word_storage,
    size_t word_storage_capacity,
    de_shell_execution_result_t *execution_result,
    de_shell_eval_result_t *out_result,
    de_shell_execution_diagnostic_t *diagnostic)
{
    size_t index;
    size_t storage_offset = 0U;
    size_t assignment_count = 0U;
    size_t command_argc;
    size_t command_environment_count = 0U;
    de_shell_environment_entry_t command_environment[DE_SHELL_EXEC_MAX_ENVIRONMENT_ENTRIES];
    const de_shell_command_info_t *command_info = NULL;
    de_shell_registry_status_t registry_status;
    de_shell_command_result_t command_result = DE_SHELL_COMMAND_RESULT_FAILURE;

    if (node->redirection_count != 0U) {
        de_shell_set_diagnostic(
            diagnostic,
            DE_SHELL_EXECUTION_STAGE_DISPATCH,
            (int)node->redirection_count,
            node->source_offset);
        return DE_SHELL_STATUS_UNSUPPORTED_OPERATION;
    }

    if (node->word_count == 0U) {
        de_shell_set_diagnostic(
            diagnostic,
            DE_SHELL_EXECUTION_STAGE_DISPATCH,
            0,
            node->source_offset);
        return DE_SHELL_STATUS_UNSUPPORTED_OPERATION;
    }

    if (node->word_count > DE_SHELL_EXEC_MAX_ARGUMENTS
        || node->first_word > ast->word_count
        || node->word_count > ast->word_count - node->first_word) {
        de_shell_set_diagnostic(
            diagnostic,
            DE_SHELL_EXECUTION_STAGE_DISPATCH,
            1,
            node->source_offset);
        return DE_SHELL_STATUS_RESOURCE_LIMIT;
    }

    for (index = 0U; index < node->word_count; index += 1U) {
        const de_shell_status_t decode_status = de_shell_decode_word(
            source,
            source_length,
            &ast->words[node->first_word + index],
            shell,
            context,
            shell->last_exit_status,
            word_storage,
            word_storage_capacity,
            &storage_offset,
            &argv[index]);

        if (decode_status != DE_SHELL_STATUS_OK) {
            de_shell_set_diagnostic(
                diagnostic,
                DE_SHELL_EXECUTION_STAGE_EXPAND,
                (int)decode_status,
                ast->words[node->first_word + index].source_offset);
            return decode_status;
        }
    }


    command_argc = node->word_count;
    while (assignment_count < node->word_count) {
        size_t name_length = 0U;
        if (!de_shell_assignment_name_length(
                source,
                &ast->words[node->first_word + assignment_count],
                &name_length)) {
            break;
        }
        assignment_count += 1U;
    }

    if (assignment_count == node->word_count) {
        for (index = 0U; index < assignment_count; index += 1U) {
            char *equals = strchr((char *)argv[index], '=');
            de_shell_status_t assignment_status;

            if (equals == NULL) {
                return DE_SHELL_STATUS_INTERNAL_ERROR;
            }
            *equals = '\0';
            assignment_status = de_shell_set_variable(
                shell, argv[index], strlen(argv[index]), equals + 1);
            if (assignment_status != DE_SHELL_STATUS_OK) {
                de_shell_set_diagnostic(
                    diagnostic,
                    DE_SHELL_EXECUTION_STAGE_EXPAND,
                    (int)assignment_status,
                    ast->words[node->first_word + index].source_offset);
                return assignment_status;
            }
        }

        out_result->exit_code = 0;
        out_result->exit_requested = 0;
        execution_result->exit_code = 0;
        execution_result->exit_requested = 0;
        execution_result->commands_executed += 1U;
        shell->last_exit_status = 0;
        return DE_SHELL_STATUS_OK;
    }

    if (assignment_count != 0U) {
        for (index = 0U; index < context->environment_count; index += 1U) {
            command_environment[index] = context->environment[index];
        }
        command_environment_count = context->environment_count;

        for (index = 0U; index < assignment_count; index += 1U) {
            char *equals = strchr((char *)argv[index], '=');
            size_t environment_index;
            int replaced = 0;

            if (equals == NULL) {
                return DE_SHELL_STATUS_INTERNAL_ERROR;
            }
            *equals = '\0';

            for (environment_index = 0U;
                 environment_index < command_environment_count;
                 environment_index += 1U) {
                if (strcmp(command_environment[environment_index].key, argv[index]) == 0) {
                    command_environment[environment_index].value = equals + 1;
                    replaced = 1;
                }
            }

            if (replaced == 0) {
                if (command_environment_count >= DE_SHELL_EXEC_MAX_ENVIRONMENT_ENTRIES) {
                    de_shell_set_diagnostic(
                        diagnostic,
                        DE_SHELL_EXECUTION_STAGE_EXPAND,
                        (int)DE_SHELL_STATUS_RESOURCE_LIMIT,
                        ast->words[node->first_word + index].source_offset);
                    return DE_SHELL_STATUS_RESOURCE_LIMIT;
                }
                command_environment[command_environment_count].key = argv[index];
                command_environment[command_environment_count].value = equals + 1;
                command_environment_count += 1U;
            }
        }

        command_argc -= assignment_count;
        (void)memmove(argv, &argv[assignment_count], command_argc * sizeof(*argv));
        argv[command_argc] = NULL;
    }

    out_result->exit_requested = 0;
    if (argv[0][0] == '\0') {
        out_result->exit_code = 127;
        execution_result->exit_code = 127;
        execution_result->exit_requested = 0;
        execution_result->commands_executed += 1U;
        shell->last_exit_status = 0;
        return DE_SHELL_STATUS_OK;
    }

    argv[command_argc] = NULL;

    registry_status = de_shell_command_registry_lookup(
        shell->command_registry, argv[0], &command_info);

    if (registry_status == DE_SHELL_REGISTRY_STATUS_OK) {
        if (assignment_count != 0U) {
            de_shell_set_diagnostic(
                diagnostic,
                DE_SHELL_EXECUTION_STAGE_DISPATCH,
                (int)DE_SHELL_STATUS_UNSUPPORTED_OPERATION,
                node->source_offset);
            return DE_SHELL_STATUS_UNSUPPORTED_OPERATION;
        }
        registry_status = de_shell_command_registry_dispatch(
            shell->command_registry,
            shell,
            command_argc,
            argv,
            &command_result);

        if (registry_status == DE_SHELL_REGISTRY_STATUS_ARGUMENT_COUNT_MISMATCH) {
            out_result->exit_code = 2;
        } else if (registry_status != DE_SHELL_REGISTRY_STATUS_OK) {
            de_shell_set_diagnostic(
                diagnostic,
                DE_SHELL_EXECUTION_STAGE_DISPATCH,
                (int)registry_status,
                node->source_offset);
            return DE_SHELL_STATUS_INTERNAL_ERROR;
        } else if (command_result == DE_SHELL_COMMAND_RESULT_EXIT_REQUESTED) {
            out_result->exit_code = 0;
            out_result->exit_requested = 1;
        } else if (command_result == DE_SHELL_COMMAND_RESULT_FAILURE) {
            out_result->exit_code = 1;
        } else {
            out_result->exit_code = 0;
        }

        execution_result->exit_code = out_result->exit_code;
        execution_result->exit_requested = out_result->exit_requested;
        execution_result->commands_executed += 1U;
        shell->last_exit_status = out_result->exit_code;
        return DE_SHELL_STATUS_OK;
    }

    if (registry_status != DE_SHELL_REGISTRY_STATUS_NOT_FOUND
        && registry_status != DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT) {
        de_shell_set_diagnostic(
            diagnostic,
            DE_SHELL_EXECUTION_STAGE_DISPATCH,
            (int)registry_status,
            node->source_offset);
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }

    if (context->external_executor == NULL) {
        de_shell_set_diagnostic(
            diagnostic,
            DE_SHELL_EXECUTION_STAGE_EXTERNAL,
            0,
            node->source_offset);
        return DE_SHELL_STATUS_EXECUTION_UNAVAILABLE;
    }

    {
        const de_shell_external_command_t command = {
            argv[0],
            command_argc,
            argv,
            assignment_count == 0U ? context->environment : command_environment,
            assignment_count == 0U ? context->environment_count : command_environment_count,
            context->working_directory,
        };
        int exit_code = 127;
        const de_shell_status_t callback_status = context->external_executor(
            &command, &exit_code, context->user_data);

        if (callback_status != DE_SHELL_STATUS_OK) {
            de_shell_set_diagnostic(
                diagnostic,
                DE_SHELL_EXECUTION_STAGE_EXTERNAL,
                (int)callback_status,
                node->source_offset);
            return DE_SHELL_STATUS_EXTERNAL_EXECUTION_FAILED;
        }

        out_result->exit_code = exit_code;
        out_result->exit_requested = 0;
        execution_result->exit_code = exit_code;
        execution_result->exit_requested = 0;
        execution_result->commands_executed += 1U;
        shell->last_exit_status = exit_code;
    }

    return DE_SHELL_STATUS_OK;
}

static de_shell_status_t de_shell_evaluate_ast(
    de_shell_t *shell,
    const unsigned char *source,
    size_t source_length,
    const de_shell_ast_t *ast,
    const de_shell_execution_context_t *context,
    de_shell_execution_frame_t *frames,
    const char **argv,
    char *word_storage,
    size_t word_storage_capacity,
    de_shell_execution_result_t *execution_result,
    de_shell_execution_diagnostic_t *diagnostic)
{
    size_t depth = 1U;
    de_shell_eval_result_t completed = {0, 0};

    frames[0].node_index = ast->root;
    frames[0].state = 0U;
    frames[0].left_exit_code = 0;
    frames[0].left_exit_requested = 0;

    while (depth != 0U) {
        de_shell_execution_frame_t *frame = &frames[depth - 1U];
        const de_shell_ast_node_t *node;

        if (frame->node_index >= ast->node_count) {
            de_shell_set_diagnostic(
                diagnostic,
                DE_SHELL_EXECUTION_STAGE_DISPATCH,
                2,
                SIZE_MAX);
            return DE_SHELL_STATUS_INTERNAL_ERROR;
        }

        node = &ast->nodes[frame->node_index];

        if (frame->state == 0U) {
            if (node->type == DE_SHELL_AST_SIMPLE_COMMAND) {
                const de_shell_status_t command_status = de_shell_execute_simple_command(
                    shell,
                    source,
                    source_length,
                    ast,
                    node,
                    context,
                    argv,
                    word_storage,
                    word_storage_capacity,
                    execution_result,
                    &completed,
                    diagnostic);

                if (command_status != DE_SHELL_STATUS_OK) {
                    return command_status;
                }

                depth -= 1U;
                continue;
            }

            if (node->type == DE_SHELL_AST_SEQUENCE
                || node->type == DE_SHELL_AST_AND
                || node->type == DE_SHELL_AST_OR) {
                if (node->left_child >= ast->node_count
                    || node->right_child >= ast->node_count) {
                    de_shell_set_diagnostic(
                        diagnostic,
                        DE_SHELL_EXECUTION_STAGE_DISPATCH,
                        3,
                        node->source_offset);
                    return DE_SHELL_STATUS_INTERNAL_ERROR;
                }

                frame->state = 1U;
                if (depth >= ast->node_count) {
                    de_shell_set_diagnostic(
                        diagnostic,
                        DE_SHELL_EXECUTION_STAGE_DISPATCH,
                        4,
                        node->source_offset);
                    return DE_SHELL_STATUS_RESOURCE_LIMIT;
                }
                frames[depth].node_index = node->left_child;
                frames[depth].state = 0U;
                frames[depth].left_exit_code = 0;
                frames[depth].left_exit_requested = 0;
                depth += 1U;
                continue;
            }

            de_shell_set_diagnostic(
                diagnostic,
                DE_SHELL_EXECUTION_STAGE_DISPATCH,
                (int)node->type,
                node->source_offset);
            return DE_SHELL_STATUS_UNSUPPORTED_OPERATION;
        }

        if (frame->state == 1U) {
            int execute_right = 0;

            frame->left_exit_code = completed.exit_code;
            frame->left_exit_requested = completed.exit_requested;

            if (completed.exit_requested == 0) {
                if (node->type == DE_SHELL_AST_SEQUENCE) {
                    execute_right = 1;
                } else if (node->type == DE_SHELL_AST_AND
                    && completed.exit_code == 0) {
                    execute_right = 1;
                } else if (node->type == DE_SHELL_AST_OR
                    && completed.exit_code != 0) {
                    execute_right = 1;
                }
            }

            if (execute_right == 0) {
                completed.exit_code = frame->left_exit_code;
                completed.exit_requested = frame->left_exit_requested;
                execution_result->exit_code = completed.exit_code;
                execution_result->exit_requested = completed.exit_requested;
                depth -= 1U;
                continue;
            }

            frame->state = 2U;
            if (depth >= ast->node_count) {
                de_shell_set_diagnostic(
                    diagnostic,
                    DE_SHELL_EXECUTION_STAGE_DISPATCH,
                    5,
                    node->source_offset);
                return DE_SHELL_STATUS_RESOURCE_LIMIT;
            }
            frames[depth].node_index = node->right_child;
            frames[depth].state = 0U;
            frames[depth].left_exit_code = 0;
            frames[depth].left_exit_requested = 0;
            depth += 1U;
            continue;
        }

        if (frame->state == 2U) {
            depth -= 1U;
            continue;
        }

        de_shell_set_diagnostic(
            diagnostic,
            DE_SHELL_EXECUTION_STAGE_DISPATCH,
            6,
            node->source_offset);
        return DE_SHELL_STATUS_INTERNAL_ERROR;
    }

    return DE_SHELL_STATUS_OK;
}

de_shell_status_t de_shell_execute_source(
    de_shell_t *shell,
    const unsigned char *source,
    size_t source_length,
    const de_shell_execution_context_t *context,
    de_shell_execution_result_t *out_result,
    de_shell_execution_diagnostic_t *out_diagnostic)
{
    static const unsigned char empty_source[] = "";
    de_shell_token_t *tokens = NULL;
    de_shell_ast_node_t *nodes = NULL;
    de_shell_word_ref_t *words = NULL;
    de_shell_redirection_t *redirections = NULL;
    de_shell_execution_frame_t *frames = NULL;
    const char **argv = NULL;
    char *word_storage = NULL;
    size_t word_storage_capacity = 0U;
    size_t token_count = 0U;
    de_shell_ast_t ast;
    de_shell_lex_diagnostic_t lex_diagnostic;
    de_shell_parse_diagnostic_t parse_diagnostic;
    de_shell_lex_status_t lex_status;
    de_shell_parse_status_t parse_status;
    de_shell_status_t status = DE_SHELL_STATUS_INTERNAL_ERROR;

    if (out_result != NULL) {
        out_result->exit_code = 0;
        out_result->exit_requested = 0;
        out_result->commands_executed = 0U;
    }
    if (out_diagnostic != NULL) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_NONE, 0, SIZE_MAX);
    }

    if (shell == NULL || context == NULL || out_result == NULL
        || out_diagnostic == NULL || (source == NULL && source_length != 0U)) {
        if (out_diagnostic != NULL) {
            de_shell_set_diagnostic(
                out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 1, SIZE_MAX);
        }
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    if (shell->lifecycle_state != DE_SHELL_INTERNAL_STATE_READY) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 2, SIZE_MAX);
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    if (shell->execution_active != 0U) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 3, SIZE_MAX);
        return DE_SHELL_STATUS_BUSY;
    }

    if (!de_shell_execution_context_is_valid(context)) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 4, SIZE_MAX);
        return DE_SHELL_STATUS_INVALID_ARGUMENT;
    }

    if (source_length > DE_SHELL_EXEC_MAX_SOURCE_BYTES) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 5, SIZE_MAX);
        return DE_SHELL_STATUS_RESOURCE_LIMIT;
    }

    if (source_length != 0U
        && memchr(source, '\0', source_length) != NULL) {
        de_shell_set_diagnostic(
            out_diagnostic,
            DE_SHELL_EXECUTION_STAGE_INPUT,
            (int)DE_SHELL_LEX_STATUS_INVALID_BYTE,
            SIZE_MAX);
        return DE_SHELL_STATUS_PARSE_ERROR;
    }

    if (source == NULL) {
        source = empty_source;
    }

    shell->execution_active = 1U;

    tokens = calloc(DE_SHELL_EXEC_MAX_TOKENS, sizeof(*tokens));
    nodes = calloc(DE_SHELL_EXEC_MAX_AST_NODES, sizeof(*nodes));
    words = calloc(DE_SHELL_EXEC_MAX_WORDS, sizeof(*words));
    redirections = calloc(DE_SHELL_EXEC_MAX_REDIRECTIONS, sizeof(*redirections));

    if (tokens == NULL || nodes == NULL || words == NULL || redirections == NULL) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 6, SIZE_MAX);
        status = DE_SHELL_STATUS_INTERNAL_ERROR;
        goto cleanup;
    }

    lex_diagnostic.code = DE_SHELL_LEX_STATUS_OK;
    lex_diagnostic.source_offset = 0U;
    lex_status = de_shell_lex(
        source,
        source_length,
        tokens,
        DE_SHELL_EXEC_MAX_TOKENS,
        &token_count,
        &lex_diagnostic);
    if (lex_status != DE_SHELL_LEX_STATUS_OK) {
        de_shell_set_diagnostic(
            out_diagnostic,
            DE_SHELL_EXECUTION_STAGE_LEX,
            (int)lex_status,
            lex_diagnostic.source_offset);
        status = lex_status == DE_SHELL_LEX_STATUS_TOKEN_CAPACITY_EXCEEDED
            ? DE_SHELL_STATUS_RESOURCE_LIMIT
            : DE_SHELL_STATUS_PARSE_ERROR;
        goto cleanup;
    }

    ast.nodes = nodes;
    ast.node_capacity = DE_SHELL_EXEC_MAX_AST_NODES;
    ast.node_count = 0U;
    ast.words = words;
    ast.word_capacity = DE_SHELL_EXEC_MAX_WORDS;
    ast.word_count = 0U;
    ast.redirections = redirections;
    ast.redirection_capacity = DE_SHELL_EXEC_MAX_REDIRECTIONS;
    ast.redirection_count = 0U;
    ast.root = SIZE_MAX;

    parse_diagnostic.code = DE_SHELL_PARSE_STATUS_OK;
    parse_diagnostic.source_offset = 0U;
    parse_status = de_shell_parse(
        tokens,
        token_count,
        source_length,
        &ast,
        &parse_diagnostic);
    if (parse_status != DE_SHELL_PARSE_STATUS_OK) {
        de_shell_set_diagnostic(
            out_diagnostic,
            DE_SHELL_EXECUTION_STAGE_PARSE,
            (int)parse_status,
            parse_diagnostic.source_offset);
        status = parse_status == DE_SHELL_PARSE_STATUS_NODE_CAPACITY_EXCEEDED
            || parse_status == DE_SHELL_PARSE_STATUS_WORD_CAPACITY_EXCEEDED
            || parse_status == DE_SHELL_PARSE_STATUS_REDIRECTION_CAPACITY_EXCEEDED
            ? DE_SHELL_STATUS_RESOURCE_LIMIT
            : DE_SHELL_STATUS_PARSE_ERROR;
        goto cleanup;
    }

    if (ast.root == SIZE_MAX) {
        status = DE_SHELL_STATUS_OK;
        goto cleanup;
    }

    word_storage_capacity = DE_SHELL_EXEC_MAX_ARGUMENTS
        * (DE_SHELL_EXEC_MAX_WORD_BYTES + 1U);

    frames = calloc(ast.node_count, sizeof(*frames));
    argv = calloc(DE_SHELL_EXEC_MAX_ARGUMENTS + 1U, sizeof(*argv));
    word_storage = calloc(word_storage_capacity, sizeof(*word_storage));
    if (frames == NULL || argv == NULL || word_storage == NULL) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 8, SIZE_MAX);
        status = DE_SHELL_STATUS_INTERNAL_ERROR;
        goto cleanup;
    }

    status = de_shell_evaluate_ast(
        shell,
        source,
        source_length,
        &ast,
        context,
        frames,
        argv,
        word_storage,
        word_storage_capacity,
        out_result,
        out_diagnostic);

cleanup:
    free(word_storage);
    free(argv);
    free(frames);
    free(redirections);
    free(words);
    free(nodes);
    free(tokens);
    shell->execution_active = 0U;
    return status;
}
