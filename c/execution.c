#include "de_shell/execution.h"

#include "internal/command_registry.h"
#include "internal/lexer.h"
#include "internal/parser.h"
#include "internal/shell_internal.h"

#include <stdint.h>
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

static de_shell_status_t de_shell_decode_word(
    const unsigned char *source,
    size_t source_length,
    const de_shell_word_ref_t *word,
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
            word_storage,
            word_storage_capacity,
            &storage_offset,
            &argv[index]);

        if (decode_status != DE_SHELL_STATUS_OK) {
            de_shell_set_diagnostic(
                diagnostic,
                DE_SHELL_EXECUTION_STAGE_DISPATCH,
                (int)decode_status,
                ast->words[node->first_word + index].source_offset);
            return decode_status;
        }
    }

    out_result->exit_requested = 0;
    if (argv[0][0] == '\0') {
        out_result->exit_code = 127;
        execution_result->exit_code = 127;
        execution_result->exit_requested = 0;
        execution_result->commands_executed += 1U;
        return DE_SHELL_STATUS_OK;
    }

    argv[node->word_count] = NULL;

    registry_status = de_shell_command_registry_lookup(
        shell->command_registry, argv[0], &command_info);

    if (registry_status == DE_SHELL_REGISTRY_STATUS_OK) {
        registry_status = de_shell_command_registry_dispatch(
            shell->command_registry,
            shell,
            node->word_count,
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
            node->word_count,
            argv,
            context->environment,
            context->environment_count,
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

    if (ast.word_count > SIZE_MAX - source_length - 1U) {
        de_shell_set_diagnostic(
            out_diagnostic, DE_SHELL_EXECUTION_STAGE_INPUT, 7, SIZE_MAX);
        status = DE_SHELL_STATUS_RESOURCE_LIMIT;
        goto cleanup;
    }
    word_storage_capacity = source_length + ast.word_count + 1U;

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
