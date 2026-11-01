#include "parser.h"

#include <stdint.h>

#define DE_SHELL_PARSE_MAX_NESTING 128U

typedef struct de_shell_parser {
    const de_shell_token_t *tokens;
    size_t token_count;
    size_t cursor;
    size_t source_length;
    de_shell_ast_t *ast;
    de_shell_parse_diagnostic_t *diagnostic;
    size_t nesting_depth;
} de_shell_parser_t;

static const de_shell_token_t *de_shell_current_token(
    const de_shell_parser_t *parser)
{
    if (parser->cursor >= parser->token_count) {
        return NULL;
    }

    return &parser->tokens[parser->cursor];
}

static de_shell_parse_status_t de_shell_set_error(
    de_shell_parser_t *parser,
    de_shell_parse_status_t status,
    size_t source_offset)
{
    parser->diagnostic->code = status;
    parser->diagnostic->source_offset = source_offset;
    return status;
}

static size_t de_shell_expected_token_length(de_shell_token_type_t type)
{
    switch (type) {
    case DE_SHELL_TOKEN_AND_IF:
    case DE_SHELL_TOKEN_OR_IF:
    case DE_SHELL_TOKEN_REDIRECT_APPEND:
        return 2U;
    case DE_SHELL_TOKEN_REDIRECT_ERROR_OUTPUT:
        return 2U;
    case DE_SHELL_TOKEN_REDIRECT_ERROR_APPEND:
        return 3U;
    case DE_SHELL_TOKEN_WORD:
    case DE_SHELL_TOKEN_PIPE:
    case DE_SHELL_TOKEN_SEMICOLON:
    case DE_SHELL_TOKEN_REDIRECT_INPUT:
    case DE_SHELL_TOKEN_REDIRECT_OUTPUT:
    case DE_SHELL_TOKEN_LEFT_PAREN:
    case DE_SHELL_TOKEN_RIGHT_PAREN:
    case DE_SHELL_TOKEN_BACKGROUND:
        return 1U;
    default:
        return 0U;
    }
}

static de_shell_parse_status_t de_shell_validate_tokens(
    const de_shell_token_t *tokens,
    size_t token_count,
    size_t source_length,
    de_shell_parse_diagnostic_t *diagnostic)
{
    size_t index;
    size_t previous_end = 0U;

    for (index = 0U; index < token_count; index += 1U) {
        const de_shell_token_t *token = &tokens[index];
        const unsigned int allowed_flags = DE_SHELL_TOKEN_FLAG_SINGLE_QUOTED
            | DE_SHELL_TOKEN_FLAG_DOUBLE_QUOTED
            | DE_SHELL_TOKEN_FLAG_ESCAPED;
        const size_t expected_length = de_shell_expected_token_length(token->type);

        if (expected_length == 0U || token->source_length == 0U
            || token->source_offset > source_length
            || token->source_length > source_length - token->source_offset
            || token->source_offset < previous_end
            || (token->type == DE_SHELL_TOKEN_WORD
                && (token->flags & ~allowed_flags) != 0U)
            || (token->type != DE_SHELL_TOKEN_WORD && token->flags != 0U)
            || (token->type != DE_SHELL_TOKEN_WORD
                && token->source_length != expected_length)) {
            diagnostic->code = DE_SHELL_PARSE_STATUS_INVALID_TOKEN_STREAM;
            diagnostic->source_offset = token->source_offset <= source_length
                ? token->source_offset
                : source_length;
            return diagnostic->code;
        }

        previous_end = token->source_offset + token->source_length;
    }

    return DE_SHELL_PARSE_STATUS_OK;
}

static de_shell_parse_status_t de_shell_append_node(
    de_shell_parser_t *parser,
    const de_shell_ast_node_t *node,
    size_t *out_index)
{
    if (parser->ast->node_count >= parser->ast->node_capacity
        || parser->ast->nodes == NULL) {
        return de_shell_set_error(
            parser,
            DE_SHELL_PARSE_STATUS_NODE_CAPACITY_EXCEEDED,
            node->source_offset);
    }

    *out_index = parser->ast->node_count;
    parser->ast->nodes[parser->ast->node_count] = *node;
    parser->ast->node_count += 1U;
    return DE_SHELL_PARSE_STATUS_OK;
}

static de_shell_parse_status_t de_shell_append_word(
    de_shell_parser_t *parser,
    const de_shell_token_t *token)
{
    if (parser->ast->word_count >= parser->ast->word_capacity
        || parser->ast->words == NULL) {
        return de_shell_set_error(
            parser,
            DE_SHELL_PARSE_STATUS_WORD_CAPACITY_EXCEEDED,
            token->source_offset);
    }

    parser->ast->words[parser->ast->word_count].source_offset = token->source_offset;
    parser->ast->words[parser->ast->word_count].source_length = token->source_length;
    parser->ast->words[parser->ast->word_count].flags = token->flags;
    parser->ast->word_count += 1U;
    return DE_SHELL_PARSE_STATUS_OK;
}

static de_shell_parse_status_t de_shell_append_redirection(
    de_shell_parser_t *parser,
    const de_shell_token_t *operator_token,
    const de_shell_token_t *target_token)
{
    de_shell_redirection_t *redirection;

    if (parser->ast->redirection_count >= parser->ast->redirection_capacity
        || parser->ast->redirections == NULL) {
        return de_shell_set_error(
            parser,
            DE_SHELL_PARSE_STATUS_REDIRECTION_CAPACITY_EXCEEDED,
            operator_token->source_offset);
    }

    redirection = &parser->ast->redirections[parser->ast->redirection_count];
    switch (operator_token->type) {
    case DE_SHELL_TOKEN_REDIRECT_INPUT:
        redirection->type = DE_SHELL_REDIRECTION_INPUT;
        break;
    case DE_SHELL_TOKEN_REDIRECT_OUTPUT:
        redirection->type = DE_SHELL_REDIRECTION_OUTPUT;
        break;
    case DE_SHELL_TOKEN_REDIRECT_APPEND:
        redirection->type = DE_SHELL_REDIRECTION_APPEND;
        break;
    case DE_SHELL_TOKEN_REDIRECT_ERROR_OUTPUT:
        redirection->type = DE_SHELL_REDIRECTION_ERROR_OUTPUT;
        break;
    case DE_SHELL_TOKEN_REDIRECT_ERROR_APPEND:
        redirection->type = DE_SHELL_REDIRECTION_ERROR_APPEND;
        break;
    default:
        return de_shell_set_error(
            parser,
            DE_SHELL_PARSE_STATUS_INVALID_TOKEN_STREAM,
            operator_token->source_offset);
    }

    redirection->operator_offset = operator_token->source_offset;
    redirection->target_offset = target_token->source_offset;
    redirection->target_length = target_token->source_length;
    redirection->target_flags = target_token->flags;
    parser->ast->redirection_count += 1U;
    return DE_SHELL_PARSE_STATUS_OK;
}

static de_shell_parse_status_t de_shell_parse_list(
    de_shell_parser_t *parser,
    size_t *out_node);

static de_shell_parse_status_t de_shell_parse_simple_command(
    de_shell_parser_t *parser,
    size_t *out_node)
{
    const size_t first_token_index = parser->cursor;
    const size_t first_word = parser->ast->word_count;
    const size_t first_redirection = parser->ast->redirection_count;
    size_t last_end = 0U;
    int saw_element = 0;
    de_shell_ast_node_t node;

    while (parser->cursor < parser->token_count) {
        const de_shell_token_t *token = de_shell_current_token(parser);

        if (token->type == DE_SHELL_TOKEN_WORD) {
            const de_shell_parse_status_t status = de_shell_append_word(parser, token);
            if (status != DE_SHELL_PARSE_STATUS_OK) {
                return status;
            }
            last_end = token->source_offset + token->source_length;
            parser->cursor += 1U;
            saw_element = 1;
            continue;
        }

        if (token->type == DE_SHELL_TOKEN_REDIRECT_INPUT
            || token->type == DE_SHELL_TOKEN_REDIRECT_OUTPUT
            || token->type == DE_SHELL_TOKEN_REDIRECT_APPEND
            || token->type == DE_SHELL_TOKEN_REDIRECT_ERROR_OUTPUT
            || token->type == DE_SHELL_TOKEN_REDIRECT_ERROR_APPEND) {
            const de_shell_token_t *target;

            parser->cursor += 1U;
            target = de_shell_current_token(parser);
            if (target == NULL || target->type != DE_SHELL_TOKEN_WORD) {
                const size_t error_offset = target == NULL
                    ? token->source_offset + token->source_length
                    : target->source_offset;
                return de_shell_set_error(
                    parser,
                    DE_SHELL_PARSE_STATUS_EXPECTED_WORD,
                    error_offset);
            }

            {
                const de_shell_parse_status_t status =
                    de_shell_append_redirection(parser, token, target);
                if (status != DE_SHELL_PARSE_STATUS_OK) {
                    return status;
                }
            }

            last_end = target->source_offset + target->source_length;
            parser->cursor += 1U;
            saw_element = 1;
            continue;
        }

        break;
    }

    if (saw_element == 0) {
        const de_shell_token_t *token = de_shell_current_token(parser);
        const size_t error_offset = token == NULL
            ? parser->source_length
            : token->source_offset;
        return de_shell_set_error(
            parser,
            DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
            error_offset);
    }

    node.type = DE_SHELL_AST_SIMPLE_COMMAND;
    node.left_child = SIZE_MAX;
    node.right_child = SIZE_MAX;
    node.first_word = first_word;
    node.word_count = parser->ast->word_count - first_word;
    node.first_redirection = first_redirection;
    node.redirection_count = parser->ast->redirection_count - first_redirection;
    node.source_offset = parser->tokens[first_token_index].source_offset;
    node.source_length = last_end - node.source_offset;

    return de_shell_append_node(parser, &node, out_node);
}

static de_shell_parse_status_t de_shell_parse_primary(
    de_shell_parser_t *parser,
    size_t *out_node)
{
    const de_shell_token_t *token = de_shell_current_token(parser);

    if (token == NULL) {
        return de_shell_set_error(
            parser,
            DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
            parser->source_length);
    }

    if (token->type == DE_SHELL_TOKEN_LEFT_PAREN) {
        const size_t opening_offset = token->source_offset;
        size_t child = SIZE_MAX;
        de_shell_ast_node_t node;
        const de_shell_token_t *closing;

        if (parser->nesting_depth >= DE_SHELL_PARSE_MAX_NESTING) {
            return de_shell_set_error(
                parser,
                DE_SHELL_PARSE_STATUS_NESTING_LIMIT_EXCEEDED,
                opening_offset);
        }

        parser->cursor += 1U;
        parser->nesting_depth += 1U;

        if (de_shell_current_token(parser) == NULL
            || de_shell_current_token(parser)->type == DE_SHELL_TOKEN_RIGHT_PAREN) {
            parser->nesting_depth -= 1U;
            return de_shell_set_error(
                parser,
                DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
                de_shell_current_token(parser) == NULL
                    ? parser->source_length
                    : de_shell_current_token(parser)->source_offset);
        }

        {
            const de_shell_parse_status_t status =
                de_shell_parse_list(parser, &child);
            if (status != DE_SHELL_PARSE_STATUS_OK) {
                parser->nesting_depth -= 1U;
                return status;
            }
        }

        closing = de_shell_current_token(parser);
        if (closing == NULL || closing->type != DE_SHELL_TOKEN_RIGHT_PAREN) {
            parser->nesting_depth -= 1U;
            return de_shell_set_error(
                parser,
                DE_SHELL_PARSE_STATUS_EXPECTED_RIGHT_PAREN,
                parser->source_length);
        }

        parser->cursor += 1U;
        parser->nesting_depth -= 1U;

        node.type = DE_SHELL_AST_GROUP;
        node.left_child = child;
        node.right_child = SIZE_MAX;
        node.first_word = 0U;
        node.word_count = 0U;
        node.first_redirection = 0U;
        node.redirection_count = 0U;
        node.source_offset = opening_offset;
        node.source_length =
            closing->source_offset + closing->source_length - opening_offset;
        return de_shell_append_node(parser, &node, out_node);
    }

    if (token->type == DE_SHELL_TOKEN_WORD
        || token->type == DE_SHELL_TOKEN_REDIRECT_INPUT
        || token->type == DE_SHELL_TOKEN_REDIRECT_OUTPUT
        || token->type == DE_SHELL_TOKEN_REDIRECT_APPEND
        || token->type == DE_SHELL_TOKEN_REDIRECT_ERROR_OUTPUT
        || token->type == DE_SHELL_TOKEN_REDIRECT_ERROR_APPEND) {
        return de_shell_parse_simple_command(parser, out_node);
    }

    return de_shell_set_error(
        parser,
        DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
        token->source_offset);
}

static de_shell_parse_status_t de_shell_make_unary_node(
    de_shell_parser_t *parser,
    de_shell_ast_node_type_t type,
    size_t child,
    size_t *out_node)
{
    const de_shell_ast_node_t *child_node = &parser->ast->nodes[child];
    de_shell_ast_node_t node;

    node.type = type;
    node.left_child = child;
    node.right_child = SIZE_MAX;
    node.first_word = 0U;
    node.word_count = 0U;
    node.first_redirection = 0U;
    node.redirection_count = 0U;
    node.source_offset = child_node->source_offset;
    node.source_length = child_node->source_length;
    return de_shell_append_node(parser, &node, out_node);
}

static de_shell_parse_status_t de_shell_make_binary_node(
    de_shell_parser_t *parser,
    de_shell_ast_node_type_t type,
    size_t left,
    size_t right,
    size_t *out_node)
{
    const de_shell_ast_node_t *left_node = &parser->ast->nodes[left];
    const de_shell_ast_node_t *right_node = &parser->ast->nodes[right];
    de_shell_ast_node_t node;
    const size_t end = right_node->source_offset + right_node->source_length;

    node.type = type;
    node.left_child = left;
    node.right_child = right;
    node.first_word = 0U;
    node.word_count = 0U;
    node.first_redirection = 0U;
    node.redirection_count = 0U;
    node.source_offset = left_node->source_offset;
    node.source_length = end - node.source_offset;
    return de_shell_append_node(parser, &node, out_node);
}

static de_shell_parse_status_t de_shell_parse_pipeline(
    de_shell_parser_t *parser,
    size_t *out_node)
{
    size_t left = SIZE_MAX;
    de_shell_parse_status_t status = de_shell_parse_primary(parser, &left);

    if (status != DE_SHELL_PARSE_STATUS_OK) {
        return status;
    }

    while (de_shell_current_token(parser) != NULL
        && de_shell_current_token(parser)->type == DE_SHELL_TOKEN_PIPE) {
        size_t right = SIZE_MAX;
        size_t combined = SIZE_MAX;

        parser->cursor += 1U;
        status = de_shell_parse_primary(parser, &right);
        if (status != DE_SHELL_PARSE_STATUS_OK) {
            return status;
        }

        status = de_shell_make_binary_node(
            parser, DE_SHELL_AST_PIPELINE, left, right, &combined);
        if (status != DE_SHELL_PARSE_STATUS_OK) {
            return status;
        }
        left = combined;
    }

    *out_node = left;
    return DE_SHELL_PARSE_STATUS_OK;
}

static de_shell_parse_status_t de_shell_parse_and_or(
    de_shell_parser_t *parser,
    size_t *out_node)
{
    size_t left = SIZE_MAX;
    de_shell_parse_status_t status = de_shell_parse_pipeline(parser, &left);

    if (status != DE_SHELL_PARSE_STATUS_OK) {
        return status;
    }

    while (de_shell_current_token(parser) != NULL
        && (de_shell_current_token(parser)->type == DE_SHELL_TOKEN_AND_IF
            || de_shell_current_token(parser)->type == DE_SHELL_TOKEN_OR_IF)) {
        const de_shell_token_type_t operator_type =
            de_shell_current_token(parser)->type;
        size_t right = SIZE_MAX;
        size_t combined = SIZE_MAX;

        parser->cursor += 1U;
        status = de_shell_parse_pipeline(parser, &right);
        if (status != DE_SHELL_PARSE_STATUS_OK) {
            return status;
        }

        status = de_shell_make_binary_node(
            parser,
            operator_type == DE_SHELL_TOKEN_AND_IF
                ? DE_SHELL_AST_AND
                : DE_SHELL_AST_OR,
            left,
            right,
            &combined);
        if (status != DE_SHELL_PARSE_STATUS_OK) {
            return status;
        }
        left = combined;
    }

    *out_node = left;
    return DE_SHELL_PARSE_STATUS_OK;
}

static de_shell_parse_status_t de_shell_parse_list(
    de_shell_parser_t *parser,
    size_t *out_node)
{
    size_t sequence_root = SIZE_MAX;
    size_t current_item = SIZE_MAX;
    de_shell_parse_status_t status =
        de_shell_parse_and_or(parser, &current_item);

    if (status != DE_SHELL_PARSE_STATUS_OK) {
        return status;
    }

    while (de_shell_current_token(parser) != NULL
        && (de_shell_current_token(parser)->type == DE_SHELL_TOKEN_SEMICOLON
            || de_shell_current_token(parser)->type == DE_SHELL_TOKEN_BACKGROUND)) {
        const de_shell_token_type_t separator =
            de_shell_current_token(parser)->type;

        parser->cursor += 1U;

        if (separator == DE_SHELL_TOKEN_BACKGROUND) {
            size_t background_node = SIZE_MAX;
            status = de_shell_make_unary_node(
                parser, DE_SHELL_AST_BACKGROUND, current_item, &background_node);
            if (status != DE_SHELL_PARSE_STATUS_OK) {
                return status;
            }
            current_item = background_node;
        }

        if (sequence_root == SIZE_MAX) {
            sequence_root = current_item;
        } else {
            size_t combined = SIZE_MAX;
            status = de_shell_make_binary_node(
                parser,
                DE_SHELL_AST_SEQUENCE,
                sequence_root,
                current_item,
                &combined);
            if (status != DE_SHELL_PARSE_STATUS_OK) {
                return status;
            }
            sequence_root = combined;
        }

        if (de_shell_current_token(parser) == NULL
            || de_shell_current_token(parser)->type == DE_SHELL_TOKEN_RIGHT_PAREN) {
            *out_node = sequence_root;
            return DE_SHELL_PARSE_STATUS_OK;
        }

        current_item = SIZE_MAX;
        status = de_shell_parse_and_or(parser, &current_item);
        if (status != DE_SHELL_PARSE_STATUS_OK) {
            return status;
        }
    }

    if (sequence_root == SIZE_MAX) {
        *out_node = current_item;
        return DE_SHELL_PARSE_STATUS_OK;
    }

    return de_shell_make_binary_node(
        parser,
        DE_SHELL_AST_SEQUENCE,
        sequence_root,
        current_item,
        out_node);
}

de_shell_parse_status_t de_shell_parse(
    const de_shell_token_t *tokens,
    size_t token_count,
    size_t source_length,
    de_shell_ast_t *out_ast,
    de_shell_parse_diagnostic_t *out_diagnostic)
{
    de_shell_parser_t parser;
    de_shell_parse_status_t status;

    if (out_ast != NULL) {
        out_ast->node_count = 0U;
        out_ast->word_count = 0U;
        out_ast->redirection_count = 0U;
        out_ast->root = SIZE_MAX;
    }
    if (out_diagnostic != NULL) {
        out_diagnostic->code = DE_SHELL_PARSE_STATUS_OK;
        out_diagnostic->source_offset = 0U;
    }

    if (out_ast == NULL || out_diagnostic == NULL
        || (tokens == NULL && token_count != 0U)
        || (out_ast->nodes == NULL && out_ast->node_capacity != 0U)
        || (out_ast->words == NULL && out_ast->word_capacity != 0U)
        || (out_ast->redirections == NULL && out_ast->redirection_capacity != 0U)) {
        if (out_diagnostic != NULL) {
            out_diagnostic->code = DE_SHELL_PARSE_STATUS_INVALID_ARGUMENT;
        }
        return DE_SHELL_PARSE_STATUS_INVALID_ARGUMENT;
    }

    status = de_shell_validate_tokens(
        tokens, token_count, source_length, out_diagnostic);
    if (status != DE_SHELL_PARSE_STATUS_OK) {
        return status;
    }

    if (token_count == 0U) {
        return DE_SHELL_PARSE_STATUS_OK;
    }

    parser.tokens = tokens;
    parser.token_count = token_count;
    parser.cursor = 0U;
    parser.source_length = source_length;
    parser.ast = out_ast;
    parser.diagnostic = out_diagnostic;
    parser.nesting_depth = 0U;

    status = de_shell_parse_list(&parser, &out_ast->root);
    if (status != DE_SHELL_PARSE_STATUS_OK) {
        out_ast->node_count = 0U;
        out_ast->word_count = 0U;
        out_ast->redirection_count = 0U;
        out_ast->root = SIZE_MAX;
        return status;
    }

    if (parser.cursor != token_count) {
        const de_shell_token_t *unexpected = de_shell_current_token(&parser);
        out_ast->node_count = 0U;
        out_ast->word_count = 0U;
        out_ast->redirection_count = 0U;
        out_ast->root = SIZE_MAX;
        return de_shell_set_error(
            &parser,
            DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
            unexpected == NULL ? source_length : unexpected->source_offset);
    }

    return DE_SHELL_PARSE_STATUS_OK;
}
