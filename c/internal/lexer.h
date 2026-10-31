#ifndef DE_SHELL_INTERNAL_LEXER_H
#define DE_SHELL_INTERNAL_LEXER_H

#include <stddef.h>

typedef enum de_shell_token_type {
    DE_SHELL_TOKEN_WORD = 0,
    DE_SHELL_TOKEN_PIPE = 1,
    DE_SHELL_TOKEN_SEMICOLON = 2,
    DE_SHELL_TOKEN_AND_IF = 3,
    DE_SHELL_TOKEN_OR_IF = 4,
    DE_SHELL_TOKEN_REDIRECT_INPUT = 5,
    DE_SHELL_TOKEN_REDIRECT_OUTPUT = 6,
    DE_SHELL_TOKEN_REDIRECT_APPEND = 7,
    DE_SHELL_TOKEN_LEFT_PAREN = 8,
    DE_SHELL_TOKEN_RIGHT_PAREN = 9,
    DE_SHELL_TOKEN_BACKGROUND = 10
} de_shell_token_type_t;

enum {
    DE_SHELL_TOKEN_FLAG_SINGLE_QUOTED = 1U << 0U,
    DE_SHELL_TOKEN_FLAG_DOUBLE_QUOTED = 1U << 1U,
    DE_SHELL_TOKEN_FLAG_ESCAPED = 1U << 2U
};

typedef struct de_shell_token {
    de_shell_token_type_t type;
    size_t source_offset;
    size_t source_length;
    unsigned int flags;
} de_shell_token_t;

typedef enum de_shell_lex_status {
    DE_SHELL_LEX_STATUS_OK = 0,
    DE_SHELL_LEX_STATUS_INVALID_ARGUMENT = 1,
    DE_SHELL_LEX_STATUS_TOKEN_CAPACITY_EXCEEDED = 2,
    DE_SHELL_LEX_STATUS_UNTERMINATED_SINGLE_QUOTE = 3,
    DE_SHELL_LEX_STATUS_UNTERMINATED_DOUBLE_QUOTE = 4,
    DE_SHELL_LEX_STATUS_TRAILING_ESCAPE = 5,
    DE_SHELL_LEX_STATUS_INVALID_BYTE = 6
} de_shell_lex_status_t;

typedef struct de_shell_lex_diagnostic {
    de_shell_lex_status_t code;
    size_t source_offset;
} de_shell_lex_diagnostic_t;

/*
 * Tokenizes a byte-oriented source buffer without allocating memory.
 *
 * Tokens retain byte-based source spans into the original source. Word tokens
 * preserve their original quote/escape bytes; flags summarize which quoting
 * constructs occurred. The parser/expansion layer interprets the raw spans.
 *
 * On every return, out_token_count is zero unless the complete lex succeeds.
 * The token array may contain unspecified partial writes after an error.
 * Empty source is valid. tokens may be NULL only when token_capacity is zero.
 */
de_shell_lex_status_t de_shell_lex(
    const unsigned char *source,
    size_t source_length,
    de_shell_token_t *tokens,
    size_t token_capacity,
    size_t *out_token_count,
    de_shell_lex_diagnostic_t *out_diagnostic);

#endif
