#ifndef DE_SHELL_INTERNAL_PARSER_H
#define DE_SHELL_INTERNAL_PARSER_H

#include <stddef.h>

#include "internal/lexer.h"

typedef enum de_shell_ast_node_type {
    DE_SHELL_AST_SIMPLE_COMMAND = 0,
    DE_SHELL_AST_PIPELINE = 1,
    DE_SHELL_AST_AND = 2,
    DE_SHELL_AST_OR = 3,
    DE_SHELL_AST_SEQUENCE = 4,
    DE_SHELL_AST_BACKGROUND = 5,
    DE_SHELL_AST_GROUP = 6
} de_shell_ast_node_type_t;

typedef enum de_shell_redirection_type {
    DE_SHELL_REDIRECTION_INPUT = 0,
    DE_SHELL_REDIRECTION_OUTPUT = 1,
    DE_SHELL_REDIRECTION_APPEND = 2
} de_shell_redirection_type_t;

typedef struct de_shell_word_ref {
    size_t source_offset;
    size_t source_length;
    unsigned int flags;
} de_shell_word_ref_t;

typedef struct de_shell_redirection {
    de_shell_redirection_type_t type;
    size_t operator_offset;
    size_t target_offset;
    size_t target_length;
    unsigned int target_flags;
} de_shell_redirection_t;

typedef struct de_shell_ast_node {
    de_shell_ast_node_type_t type;
    size_t left_child;
    size_t right_child;
    size_t first_word;
    size_t word_count;
    size_t first_redirection;
    size_t redirection_count;
    size_t source_offset;
    size_t source_length;
} de_shell_ast_node_t;

typedef struct de_shell_ast {
    de_shell_ast_node_t *nodes;
    size_t node_capacity;
    size_t node_count;
    de_shell_word_ref_t *words;
    size_t word_capacity;
    size_t word_count;
    de_shell_redirection_t *redirections;
    size_t redirection_capacity;
    size_t redirection_count;
    size_t root;
} de_shell_ast_t;

typedef enum de_shell_parse_status {
    DE_SHELL_PARSE_STATUS_OK = 0,
    DE_SHELL_PARSE_STATUS_INVALID_ARGUMENT = 1,
    DE_SHELL_PARSE_STATUS_INVALID_TOKEN_STREAM = 2,
    DE_SHELL_PARSE_STATUS_UNEXPECTED_TOKEN = 3,
    DE_SHELL_PARSE_STATUS_EXPECTED_WORD = 4,
    DE_SHELL_PARSE_STATUS_EXPECTED_RIGHT_PAREN = 5,
    DE_SHELL_PARSE_STATUS_NESTING_LIMIT_EXCEEDED = 6,
    DE_SHELL_PARSE_STATUS_NODE_CAPACITY_EXCEEDED = 7,
    DE_SHELL_PARSE_STATUS_WORD_CAPACITY_EXCEEDED = 8,
    DE_SHELL_PARSE_STATUS_REDIRECTION_CAPACITY_EXCEEDED = 9
} de_shell_parse_status_t;

typedef struct de_shell_parse_diagnostic {
    de_shell_parse_status_t code;
    size_t source_offset;
} de_shell_parse_diagnostic_t;

/*
 * Parses lexer tokens into caller-owned AST storage.
 *
 * Grammar implemented by this alpha parser:
 *   list       := and_or ((';' | '&') and_or)* [';' | '&']
 *   and_or     := pipeline (('&&' | '||') pipeline)*
 *   pipeline   := command ('|' command)*
 *   command    := simple_command | '(' list ')'
 *   simple     := (WORD | redirection)+
 *   redirection:= ('<' | '>' | '>>') WORD
 *
 * Precedence, from highest to lowest: redirections/words, pipeline,
 * left-associative AND/OR, then sequence/background separators.
 *
 * Empty token input is a valid empty program (root == SIZE_MAX). The parser
 * performs no allocations. On failure all AST counts are reset to zero and
 * root is SIZE_MAX; partially written arrays must be ignored.
 */
de_shell_parse_status_t de_shell_parse(
    const de_shell_token_t *tokens,
    size_t token_count,
    size_t source_length,
    de_shell_ast_t *out_ast,
    de_shell_parse_diagnostic_t *out_diagnostic);

#endif
