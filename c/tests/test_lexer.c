#include "internal/lexer.h"

#include <stdio.h>
#include <string.h>

#define TOKEN_CAPACITY 16U

static int check_token(
    const de_shell_token_t *token,
    de_shell_token_type_t type,
    size_t offset,
    size_t length,
    unsigned int flags)
{
    return token->type == type
        && token->source_offset == offset
        && token->source_length == length
        && token->flags == flags;
}

static int test_whitespace_and_source_spans(void)
{
    static const unsigned char source[] = "  echo\talpha  beta\n";
    de_shell_token_t tokens[TOKEN_CAPACITY];
    de_shell_lex_diagnostic_t diagnostic;
    size_t count = 0U;

    if (de_shell_lex(
            source, sizeof(source) - 1U, tokens, TOKEN_CAPACITY, &count, &diagnostic)
            != DE_SHELL_LEX_STATUS_OK
        || count != 3U
        || !check_token(&tokens[0], DE_SHELL_TOKEN_WORD, 2U, 4U, 0U)
        || !check_token(&tokens[1], DE_SHELL_TOKEN_WORD, 7U, 5U, 0U)
        || !check_token(&tokens[2], DE_SHELL_TOKEN_WORD, 14U, 4U, 0U)) {
        (void)fprintf(stderr, "whitespace/source span test failed\n");
        return 1;
    }

    return 0;
}

static int test_quotes_preserve_raw_spans_and_flags(void)
{
    static const unsigned char source[] = "say 'two words' \"three words\"";
    de_shell_token_t tokens[TOKEN_CAPACITY];
    de_shell_lex_diagnostic_t diagnostic;
    size_t count = 0U;

    if (de_shell_lex(
            source, sizeof(source) - 1U, tokens, TOKEN_CAPACITY, &count, &diagnostic)
            != DE_SHELL_LEX_STATUS_OK
        || count != 3U
        || !check_token(&tokens[0], DE_SHELL_TOKEN_WORD, 0U, 3U, 0U)
        || !check_token(
            &tokens[1],
            DE_SHELL_TOKEN_WORD,
            4U,
            11U,
            DE_SHELL_TOKEN_FLAG_SINGLE_QUOTED)
        || !check_token(
            &tokens[2],
            DE_SHELL_TOKEN_WORD,
            16U,
            13U,
            DE_SHELL_TOKEN_FLAG_DOUBLE_QUOTED)) {
        (void)fprintf(stderr, "quote/source span test failed\n");
        return 1;
    }

    return 0;
}

static int test_escaped_whitespace_and_quotes(void)
{
    static const unsigned char source[] = "echo a\\ b \"c\\\"d\"";
    de_shell_token_t tokens[TOKEN_CAPACITY];
    de_shell_lex_diagnostic_t diagnostic;
    size_t count = 0U;

    if (de_shell_lex(
            source, sizeof(source) - 1U, tokens, TOKEN_CAPACITY, &count, &diagnostic)
            != DE_SHELL_LEX_STATUS_OK
        || count != 3U
        || !check_token(&tokens[1], DE_SHELL_TOKEN_WORD, 5U, 4U, DE_SHELL_TOKEN_FLAG_ESCAPED)
        || !check_token(
            &tokens[2],
            DE_SHELL_TOKEN_WORD,
            10U,
            6U,
            DE_SHELL_TOKEN_FLAG_DOUBLE_QUOTED | DE_SHELL_TOKEN_FLAG_ESCAPED)) {
        (void)fprintf(stderr, "escape handling test failed\n");
        return 1;
    }

    return 0;
}

static int test_operator_tokens(void)
{
    static const unsigned char source[] = "cmd|next && other || x; in<file out>>log stderr 2>err 2>>errlog name2>out & (group)";
    static const de_shell_token_type_t expected[] = {
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_PIPE,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_AND_IF,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_OR_IF,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_SEMICOLON,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_REDIRECT_INPUT,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_REDIRECT_APPEND,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_REDIRECT_ERROR_OUTPUT,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_REDIRECT_ERROR_APPEND,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_REDIRECT_OUTPUT,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_BACKGROUND,
        DE_SHELL_TOKEN_LEFT_PAREN,
        DE_SHELL_TOKEN_WORD,
        DE_SHELL_TOKEN_RIGHT_PAREN
    };
    de_shell_token_t tokens[TOKEN_CAPACITY + 16U];
    de_shell_lex_diagnostic_t diagnostic;
    size_t count = 0U;
    size_t index;

    if (de_shell_lex(
            source,
            sizeof(source) - 1U,
            tokens,
            TOKEN_CAPACITY + 4U,
            &count,
            &diagnostic) != DE_SHELL_LEX_STATUS_OK
        || count != sizeof(expected) / sizeof(expected[0])) {
        (void)fprintf(stderr, "operator token count test failed: got %zu expected %zu\n", count, sizeof(expected) / sizeof(expected[0]));
        for (index = 0U; index < count; index += 1U) {
            (void)fprintf(stderr, "token[%zu]=%d\n", index, (int)tokens[index].type);
        }
        return 1;
    }

    for (index = 0U; index < count; index += 1U) {
        if (tokens[index].type != expected[index]) {
            (void)fprintf(stderr, "operator token type mismatch at %zu\n", index);
            return 1;
        }
    }

    return 0;
}

static int test_malformed_quotes_and_escape(void)
{
    static const unsigned char single_quote[] = "echo 'unfinished";
    static const unsigned char double_quote[] = "echo \"unfinished";
    static const unsigned char trailing_escape[] = "echo \\";
    de_shell_token_t tokens[TOKEN_CAPACITY];
    de_shell_lex_diagnostic_t diagnostic;
    size_t count = 0U;

    if (de_shell_lex(
            single_quote,
            sizeof(single_quote) - 1U,
            tokens,
            TOKEN_CAPACITY,
            &count,
            &diagnostic) != DE_SHELL_LEX_STATUS_UNTERMINATED_SINGLE_QUOTE
        || count != 0U
        || diagnostic.source_offset != 5U) {
        (void)fprintf(stderr, "unterminated single quote diagnostic failed\n");
        return 1;
    }

    if (de_shell_lex(
            double_quote,
            sizeof(double_quote) - 1U,
            tokens,
            TOKEN_CAPACITY,
            &count,
            &diagnostic) != DE_SHELL_LEX_STATUS_UNTERMINATED_DOUBLE_QUOTE
        || diagnostic.source_offset != 5U) {
        (void)fprintf(stderr, "unterminated double quote diagnostic failed\n");
        return 1;
    }

    if (de_shell_lex(
            trailing_escape,
            sizeof(trailing_escape) - 1U,
            tokens,
            TOKEN_CAPACITY,
            &count,
            &diagnostic) != DE_SHELL_LEX_STATUS_TRAILING_ESCAPE
        || diagnostic.source_offset != 5U) {
        (void)fprintf(stderr, "trailing escape diagnostic failed\n");
        return 1;
    }

    return 0;
}

static int test_token_capacity_and_empty_source(void)
{
    static const unsigned char source[] = "one two";
    de_shell_token_t token;
    de_shell_lex_diagnostic_t diagnostic;
    size_t count = 99U;

    if (de_shell_lex(
            source, sizeof(source) - 1U, &token, 1U, &count, &diagnostic)
            != DE_SHELL_LEX_STATUS_TOKEN_CAPACITY_EXCEEDED
        || count != 0U
        || diagnostic.source_offset != 4U) {
        (void)fprintf(stderr, "token capacity boundary test failed\n");
        return 1;
    }

    if (de_shell_lex(NULL, 0U, NULL, 0U, &count, &diagnostic)
            != DE_SHELL_LEX_STATUS_OK
        || count != 0U) {
        (void)fprintf(stderr, "empty source test failed\n");
        return 1;
    }

    return 0;
}

static int test_invalid_arguments_and_nul_byte(void)
{
    static const unsigned char invalid_source[] = {'a', 0U, 'b'};
    de_shell_token_t tokens[TOKEN_CAPACITY];
    de_shell_lex_diagnostic_t diagnostic;
    size_t count = 0U;

    if (de_shell_lex(NULL, 1U, tokens, TOKEN_CAPACITY, &count, &diagnostic)
        != DE_SHELL_LEX_STATUS_INVALID_ARGUMENT) {
        (void)fprintf(stderr, "NULL source must be rejected\n");
        return 1;
    }

    if (de_shell_lex(
            invalid_source,
            sizeof(invalid_source),
            tokens,
            TOKEN_CAPACITY,
            &count,
            &diagnostic) != DE_SHELL_LEX_STATUS_INVALID_BYTE
        || diagnostic.source_offset != 1U
        || count != 0U) {
        (void)fprintf(stderr, "embedded NUL diagnostic failed\n");
        return 1;
    }

    if (de_shell_lex(
            (const unsigned char *)"x",
            1U,
            NULL,
            1U,
            &count,
            &diagnostic) != DE_SHELL_LEX_STATUS_INVALID_ARGUMENT) {
        (void)fprintf(stderr, "NULL token storage must be rejected\n");
        return 1;
    }

    return 0;
}

int main(void)
{
    if (test_whitespace_and_source_spans() != 0
        || test_quotes_preserve_raw_spans_and_flags() != 0
        || test_escaped_whitespace_and_quotes() != 0
        || test_operator_tokens() != 0
        || test_malformed_quotes_and_escape() != 0
        || test_token_capacity_and_empty_source() != 0
        || test_invalid_arguments_and_nul_byte() != 0) {
        return 1;
    }

    (void)puts("DE-Shell lexer tests passed");
    return 0;
}
