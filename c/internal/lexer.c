#include "lexer.h"

#include <stddef.h>

typedef enum de_shell_quote_state {
    DE_SHELL_QUOTE_NONE = 0,
    DE_SHELL_QUOTE_SINGLE = 1,
    DE_SHELL_QUOTE_DOUBLE = 2
} de_shell_quote_state_t;

static int de_shell_is_whitespace(unsigned char value)
{
    return value == (unsigned char)' '
        || value == (unsigned char)'\t'
        || value == (unsigned char)'\n'
        || value == (unsigned char)'\r'
        || value == (unsigned char)'\v'
        || value == (unsigned char)'\f';
}

static size_t de_shell_operator_length(
    const unsigned char *source,
    size_t source_length,
    size_t offset,
    de_shell_token_type_t *out_type)
{
    const unsigned char current = source[offset];

    if (offset + 1U < source_length) {
        const unsigned char next = source[offset + 1U];

        if (current == (unsigned char)'&' && next == (unsigned char)'&') {
            *out_type = DE_SHELL_TOKEN_AND_IF;
            return 2U;
        }
        if (current == (unsigned char)'|' && next == (unsigned char)'|') {
            *out_type = DE_SHELL_TOKEN_OR_IF;
            return 2U;
        }
        if (current == (unsigned char)'>' && next == (unsigned char)'>') {
            *out_type = DE_SHELL_TOKEN_REDIRECT_APPEND;
            return 2U;
        }
    }

    switch (current) {
    case (unsigned char)'|':
        *out_type = DE_SHELL_TOKEN_PIPE;
        return 1U;
    case (unsigned char)';':
        *out_type = DE_SHELL_TOKEN_SEMICOLON;
        return 1U;
    case (unsigned char)'<':
        *out_type = DE_SHELL_TOKEN_REDIRECT_INPUT;
        return 1U;
    case (unsigned char)'>':
        *out_type = DE_SHELL_TOKEN_REDIRECT_OUTPUT;
        return 1U;
    case (unsigned char)'(':
        *out_type = DE_SHELL_TOKEN_LEFT_PAREN;
        return 1U;
    case (unsigned char)')':
        *out_type = DE_SHELL_TOKEN_RIGHT_PAREN;
        return 1U;
    case (unsigned char)'&':
        *out_type = DE_SHELL_TOKEN_BACKGROUND;
        return 1U;
    default:
        return 0U;
    }
}

static de_shell_lex_status_t de_shell_emit_token(
    de_shell_token_t *tokens,
    size_t token_capacity,
    size_t *token_count,
    de_shell_token_type_t type,
    size_t source_offset,
    size_t source_length,
    unsigned int flags,
    de_shell_lex_diagnostic_t *diagnostic)
{
    if (*token_count >= token_capacity) {
        diagnostic->code = DE_SHELL_LEX_STATUS_TOKEN_CAPACITY_EXCEEDED;
        diagnostic->source_offset = source_offset;
        return diagnostic->code;
    }

    tokens[*token_count].type = type;
    tokens[*token_count].source_offset = source_offset;
    tokens[*token_count].source_length = source_length;
    tokens[*token_count].flags = flags;
    *token_count += 1U;
    return DE_SHELL_LEX_STATUS_OK;
}

static de_shell_lex_status_t de_shell_scan_word(
    const unsigned char *source,
    size_t source_length,
    size_t *offset,
    de_shell_token_t *tokens,
    size_t token_capacity,
    size_t *token_count,
    de_shell_lex_diagnostic_t *diagnostic)
{
    const size_t start = *offset;
    size_t quote_start = start;
    unsigned int flags = 0U;
    de_shell_quote_state_t quote = DE_SHELL_QUOTE_NONE;

    while (*offset < source_length) {
        const unsigned char current = source[*offset];

        if (current == 0U) {
            diagnostic->code = DE_SHELL_LEX_STATUS_INVALID_BYTE;
            diagnostic->source_offset = *offset;
            return diagnostic->code;
        }

        if (quote == DE_SHELL_QUOTE_SINGLE) {
            if (current == (unsigned char)'\'') {
                quote = DE_SHELL_QUOTE_NONE;
            }
            *offset += 1U;
            continue;
        }

        if (quote == DE_SHELL_QUOTE_DOUBLE) {
            if (current == (unsigned char)'"') {
                quote = DE_SHELL_QUOTE_NONE;
                *offset += 1U;
                continue;
            }
            if (current == (unsigned char)'\\') {
                if (*offset + 1U >= source_length) {
                    diagnostic->code = DE_SHELL_LEX_STATUS_TRAILING_ESCAPE;
                    diagnostic->source_offset = *offset;
                    return diagnostic->code;
                }
                if (source[*offset + 1U] == 0U) {
                    diagnostic->code = DE_SHELL_LEX_STATUS_INVALID_BYTE;
                    diagnostic->source_offset = *offset + 1U;
                    return diagnostic->code;
                }
                flags |= DE_SHELL_TOKEN_FLAG_ESCAPED;
                *offset += 2U;
                continue;
            }
            *offset += 1U;
            continue;
        }

        if (de_shell_is_whitespace(current)) {
            break;
        }

        {
            de_shell_token_type_t ignored_type = DE_SHELL_TOKEN_WORD;
            if (de_shell_operator_length(source, source_length, *offset, &ignored_type) != 0U) {
                break;
            }
        }

        if (current == (unsigned char)'\'') {
            quote = DE_SHELL_QUOTE_SINGLE;
            quote_start = *offset;
            flags |= DE_SHELL_TOKEN_FLAG_SINGLE_QUOTED;
            *offset += 1U;
        } else if (current == (unsigned char)'"') {
            quote = DE_SHELL_QUOTE_DOUBLE;
            quote_start = *offset;
            flags |= DE_SHELL_TOKEN_FLAG_DOUBLE_QUOTED;
            *offset += 1U;
        } else if (current == (unsigned char)'\\') {
            if (*offset + 1U >= source_length) {
                diagnostic->code = DE_SHELL_LEX_STATUS_TRAILING_ESCAPE;
                diagnostic->source_offset = *offset;
                return diagnostic->code;
            }
            if (source[*offset + 1U] == 0U) {
                diagnostic->code = DE_SHELL_LEX_STATUS_INVALID_BYTE;
                diagnostic->source_offset = *offset + 1U;
                return diagnostic->code;
            }
            flags |= DE_SHELL_TOKEN_FLAG_ESCAPED;
            *offset += 2U;
        } else {
            *offset += 1U;
        }
    }

    if (quote == DE_SHELL_QUOTE_SINGLE) {
        diagnostic->code = DE_SHELL_LEX_STATUS_UNTERMINATED_SINGLE_QUOTE;
        diagnostic->source_offset = quote_start;
        return diagnostic->code;
    }
    if (quote == DE_SHELL_QUOTE_DOUBLE) {
        diagnostic->code = DE_SHELL_LEX_STATUS_UNTERMINATED_DOUBLE_QUOTE;
        diagnostic->source_offset = quote_start;
        return diagnostic->code;
    }

    return de_shell_emit_token(
        tokens,
        token_capacity,
        token_count,
        DE_SHELL_TOKEN_WORD,
        start,
        *offset - start,
        flags,
        diagnostic);
}

de_shell_lex_status_t de_shell_lex(
    const unsigned char *source,
    size_t source_length,
    de_shell_token_t *tokens,
    size_t token_capacity,
    size_t *out_token_count,
    de_shell_lex_diagnostic_t *out_diagnostic)
{
    size_t offset = 0U;
    size_t token_count = 0U;

    if (out_token_count == NULL || out_diagnostic == NULL
        || (source == NULL && source_length != 0U)
        || (tokens == NULL && token_capacity != 0U)) {
        return DE_SHELL_LEX_STATUS_INVALID_ARGUMENT;
    }

    *out_token_count = 0U;
    out_diagnostic->code = DE_SHELL_LEX_STATUS_OK;
    out_diagnostic->source_offset = 0U;

    while (offset < source_length) {
        de_shell_token_type_t operator_type = DE_SHELL_TOKEN_WORD;
        const size_t operator_size;

        if (source[offset] == 0U) {
            out_diagnostic->code = DE_SHELL_LEX_STATUS_INVALID_BYTE;
            out_diagnostic->source_offset = offset;
            return out_diagnostic->code;
        }

        if (de_shell_is_whitespace(source[offset])) {
            offset += 1U;
            continue;
        }

        operator_size = de_shell_operator_length(
            source, source_length, offset, &operator_type);

        if (operator_size != 0U) {
            const de_shell_lex_status_t status = de_shell_emit_token(
                tokens,
                token_capacity,
                &token_count,
                operator_type,
                offset,
                operator_size,
                0U,
                out_diagnostic);

            if (status != DE_SHELL_LEX_STATUS_OK) {
                return status;
            }

            offset += operator_size;
            continue;
        }

        {
            const de_shell_lex_status_t status = de_shell_scan_word(
                source,
                source_length,
                &offset,
                tokens,
                token_capacity,
                &token_count,
                out_diagnostic);

            if (status != DE_SHELL_LEX_STATUS_OK) {
                return status;
            }
        }
    }

    *out_token_count = token_count;
    return DE_SHELL_LEX_STATUS_OK;
}
