#include "internal/parser.h"

#include <stdio.h>
#include <string.h>

#define TOKEN_CAPACITY 64U
#define NODE_CAPACITY 128U
#define WORD_CAPACITY 64U
#define REDIRECTION_CAPACITY 32U

typedef struct parser_fixture {
    de_shell_token_t tokens[TOKEN_CAPACITY];
    de_shell_ast_node_t nodes[NODE_CAPACITY];
    de_shell_word_ref_t words[WORD_CAPACITY];
    de_shell_redirection_t redirections[REDIRECTION_CAPACITY];
    de_shell_ast_t ast;
    de_shell_lex_diagnostic_t lex_diagnostic;
    de_shell_parse_diagnostic_t parse_diagnostic;
    size_t token_count;
} parser_fixture_t;

static de_shell_parse_status_t parse_source(
    parser_fixture_t *fixture,
    const char *source)
{
    const size_t source_length = strlen(source);
    const de_shell_lex_status_t lex_status = de_shell_lex(
        (const unsigned char *)source,
        source_length,
        fixture->tokens,
        TOKEN_CAPACITY,
        &fixture->token_count,
        &fixture->lex_diagnostic);

    if (lex_status != DE_SHELL_LEX_STATUS_OK) {
        (void)fprintf(stderr, "test input failed to lex: %d\n", (int)lex_status);
        return DE_SHELL_PARSE_STATUS_INVALID_TOKEN_STREAM;
    }

    fixture->ast.nodes = fixture->nodes;
    fixture->ast.node_capacity = NODE_CAPACITY;
    fixture->ast.node_count = 0U;
    fixture->ast.words = fixture->words;
    fixture->ast.word_capacity = WORD_CAPACITY;
    fixture->ast.word_count = 0U;
    fixture->ast.redirections = fixture->redirections;
    fixture->ast.redirection_capacity = REDIRECTION_CAPACITY;
    fixture->ast.redirection_count = 0U;
    fixture->ast.root = SIZE_MAX;

    return de_shell_parse(
        fixture->tokens,
        fixture->token_count,
        source_length,
        &fixture->ast,
        &fixture->parse_diagnostic);
}

static int test_simple_command_and_redirection(void)
{
    parser_fixture_t fixture;
    const char *source = "echo 'hello world' >output.txt";
    const de_shell_parse_status_t status = parse_source(&fixture, source);
    const de_shell_ast_node_t *root;

    if (status != DE_SHELL_PARSE_STATUS_OK) {
        (void)fprintf(stderr, "simple command parse failed: %d\n", (int)status);
        return 1;
    }

    root = &fixture.ast.nodes[fixture.ast.root];
    if (root->type != DE_SHELL_AST_SIMPLE_COMMAND
        || root->word_count != 2U
        || root->redirection_count != 1U
        || fixture.ast.word_count != 2U
        || fixture.ast.redirection_count != 1U
        || fixture.words[1].flags != DE_SHELL_TOKEN_FLAG_SINGLE_QUOTED
        || fixture.redirections[0].type != DE_SHELL_REDIRECTION_OUTPUT
        || fixture.redirections[0].target_offset != 19U
        || fixture.redirections[0].target_length != 10U) {
        (void)fprintf(stderr, "simple command AST contract failed\n");
        return 1;
    }

    return 0;
}

static int test_pipeline_precedence_over_and_and_sequence(void)
{
    parser_fixture_t fixture;

    if (parse_source(&fixture, "a | b && c ; d") != DE_SHELL_PARSE_STATUS_OK) {
        (void)fprintf(stderr, "precedence expression parse failed\n");
        return 1;
    }

    {
        const de_shell_ast_node_t *root = &fixture.ast.nodes[fixture.ast.root];
        const de_shell_ast_node_t *left;
        const de_shell_ast_node_t *pipeline;

        if (root->type != DE_SHELL_AST_SEQUENCE) {
            (void)fprintf(stderr, "sequence must be the root precedence level\n");
            return 1;
        }

        left = &fixture.ast.nodes[root->left_child];
        if (left->type != DE_SHELL_AST_AND) {
            (void)fprintf(stderr, "AND expression must be below pipeline precedence\n");
            return 1;
        }

        pipeline = &fixture.ast.nodes[left->left_child];
        if (pipeline->type != DE_SHELL_AST_PIPELINE) {
            (void)fprintf(stderr, "pipeline must bind more tightly than AND\n");
            return 1;
        }
    }

    return 0;
}

static int test_and_or_is_left_associative(void)
{
    parser_fixture_t fixture;

    if (parse_source(&fixture, "a && b || c") != DE_SHELL_PARSE_STATUS_OK) {
        (void)fprintf(stderr, "AND/OR expression parse failed\n");
        return 1;
    }

    {
        const de_shell_ast_node_t *root = &fixture.ast.nodes[fixture.ast.root];
        const de_shell_ast_node_t *left;

        if (root->type != DE_SHELL_AST_OR) {
            (void)fprintf(stderr, "OR must be the final left-associative operator\n");
            return 1;
        }

        left = &fixture.ast.nodes[root->left_child];
        if (left->type != DE_SHELL_AST_AND) {
            (void)fprintf(stderr, "AND/OR operators are not left-associative\n");
            return 1;
        }
    }

    return 0;
}

static int test_background_separator_applies_to_one_item(void)
{
    parser_fixture_t fixture;

    if (parse_source(&fixture, "a; b & c") != DE_SHELL_PARSE_STATUS_OK) {
        (void)fprintf(stderr, "background list parse failed\n");
        return 1;
    }

    {
        const de_shell_ast_node_t *root = &fixture.ast.nodes[fixture.ast.root];
        const de_shell_ast_node_t *left;
        const de_shell_ast_node_t *background;

        if (root->type != DE_SHELL_AST_SEQUENCE) {
            return 1;
        }

        left = &fixture.ast.nodes[root->left_child];
        if (left->type != DE_SHELL_AST_SEQUENCE) {
            (void)fprintf(stderr, "list sequence grouping is incorrect\n");
            return 1;
        }

        background = &fixture.ast.nodes[left->right_child];
        if (background->type != DE_SHELL_AST_BACKGROUND) {
            (void)fprintf(stderr, "background operator must wrap only its item\n");
            return 1;
        }
    }

    return 0;
}

static int test_grouped_expression(void)
{
    parser_fixture_t fixture;

    if (parse_source(&fixture, "(a | b) && c") != DE_SHELL_PARSE_STATUS_OK) {
        (void)fprintf(stderr, "grouped expression parse failed\n");
        return 1;
    }

    {
        const de_shell_ast_node_t *root = &fixture.ast.nodes[fixture.ast.root];
        const de_shell_ast_node_t *group;

        if (root->type != DE_SHELL_AST_AND) {
            return 1;
        }

        group = &fixture.ast.nodes[root->left_child];
        if (group->type != DE_SHELL_AST_GROUP
            || fixture.ast.nodes[group->left_child].type != DE_SHELL_AST_PIPELINE) {
            (void)fprintf(stderr, "group node did not preserve nested pipeline\n");
            return 1;
        }
    }

    return 0;
}

static int expect_parse_error(
    const char *source,
    de_shell_parse_status_t expected_status,
    size_t expected_offset)
{
    parser_fixture_t fixture;
    const de_shell_parse_status_t status = parse_source(&fixture, source);

    if (status != expected_status
        || fixture.parse_diagnostic.source_offset != expected_offset
        || fixture.ast.node_count != 0U
        || fixture.ast.word_count != 0U
        || fixture.ast.redirection_count != 0U
        || fixture.ast.root != SIZE_MAX) {
        (void)fprintf(
            stderr,
            "parse error mismatch for '%s': status=%d offset=%zu\n",
            source,
            (int)status,
            fixture.parse_diagnostic.source_offset);
        return 1;
    }

    return 0;
}

static int test_malformed_syntax_diagnostics(void)
{
    return expect_parse_error(
            "a |",
            DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
            3U)
        || expect_parse_error(
            "a >",
            DE_SHELL_PARSE_STATUS_EXPECTED_WORD,
            3U)
        || expect_parse_error(
            "(a",
            DE_SHELL_PARSE_STATUS_EXPECTED_RIGHT_PAREN,
            2U)
        || expect_parse_error(
            "a )",
            DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
            2U)
        || expect_parse_error(
            "&& a",
            DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN,
            0U);
}

static int test_empty_program(void)
{
    parser_fixture_t fixture;

    if (parse_source(&fixture, "") != DE_SHELL_PARSE_STATUS_OK
        || fixture.ast.root != SIZE_MAX
        || fixture.ast.node_count != 0U) {
        (void)fprintf(stderr, "empty program should produce an empty AST\n");
        return 1;
    }

    return 0;
}

static int test_ast_capacity_failures(void)
{
    parser_fixture_t fixture;
    const char *source = "command";
    const size_t source_length = strlen(source);

    if (de_shell_lex(
            (const unsigned char *)source,
            source_length,
            fixture.tokens,
            TOKEN_CAPACITY,
            &fixture.token_count,
            &fixture.lex_diagnostic) != DE_SHELL_LEX_STATUS_OK) {
        return 1;
    }

    fixture.ast.nodes = fixture.nodes;
    fixture.ast.node_capacity = 0U;
    fixture.ast.words = fixture.words;
    fixture.ast.word_capacity = WORD_CAPACITY;
    fixture.ast.redirections = fixture.redirections;
    fixture.ast.redirection_capacity = REDIRECTION_CAPACITY;

    if (de_shell_parse(
            fixture.tokens,
            fixture.token_count,
            source_length,
            &fixture.ast,
            &fixture.parse_diagnostic) != DE_SHELL_PARSE_STATUS_NODE_CAPACITY_EXCEEDED) {
        (void)fprintf(stderr, "node capacity error was not reported\n");
        return 1;
    }

    fixture.ast.node_capacity = NODE_CAPACITY;
    fixture.ast.word_capacity = 0U;
    if (de_shell_parse(
            fixture.tokens,
            fixture.token_count,
            source_length,
            &fixture.ast,
            &fixture.parse_diagnostic) != DE_SHELL_PARSE_STATUS_WORD_CAPACITY_EXCEEDED) {
        (void)fprintf(stderr, "word capacity error was not reported\n");
        return 1;
    }

    return 0;
}

static int test_invalid_token_span(void)
{
    parser_fixture_t fixture;
    de_shell_token_t invalid_token = {
        .type = DE_SHELL_TOKEN_WORD,
        .source_offset = 4U,
        .source_length = 1U,
        .flags = 0U
    };

    fixture.ast.nodes = fixture.nodes;
    fixture.ast.node_capacity = NODE_CAPACITY;
    fixture.ast.words = fixture.words;
    fixture.ast.word_capacity = WORD_CAPACITY;
    fixture.ast.redirections = fixture.redirections;
    fixture.ast.redirection_capacity = REDIRECTION_CAPACITY;

    if (de_shell_parse(
            &invalid_token,
            1U,
            4U,
            &fixture.ast,
            &fixture.parse_diagnostic) != DE_SHELL_PARSE_STATUS_INVALID_TOKEN_STREAM) {
        (void)fprintf(stderr, "out-of-range token span was not rejected\n");
        return 1;
    }

    return 0;
}

int main(void)
{
    if (test_simple_command_and_redirection() != 0
        || test_pipeline_precedence_over_and_and_sequence() != 0
        || test_and_or_is_left_associative() != 0
        || test_background_separator_applies_to_one_item() != 0
        || test_grouped_expression() != 0
        || test_malformed_syntax_diagnostics() != 0
        || test_empty_program() != 0
        || test_ast_capacity_failures() != 0
        || test_invalid_token_span() != 0) {
        return 1;
    }

    (void)puts("DE-Shell parser tests passed");
    return 0;
}
