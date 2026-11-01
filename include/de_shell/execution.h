#ifndef DE_SHELL_EXECUTION_H
#define DE_SHELL_EXECUTION_H

#include <stddef.h>

#include "de_shell/shell.h"

#ifdef __cplusplus
extern "C" {
#endif

#define DE_SHELL_EXEC_MAX_SOURCE_BYTES ((size_t)65536U)
#define DE_SHELL_EXEC_MAX_ARGUMENTS ((size_t)256U)
#define DE_SHELL_EXEC_MAX_WORD_BYTES ((size_t)4096U)

/*
 * Environment is passed as explicit key/value pairs. The execution engine
 * never reads or mutates the host process environment.
 */
typedef struct de_shell_environment_entry {
    const char *key;
    const char *value;
} de_shell_environment_entry_t;

typedef enum de_shell_io_redirection_type {
    DE_SHELL_IO_REDIRECTION_INPUT = 0,
    DE_SHELL_IO_REDIRECTION_OUTPUT = 1,
    DE_SHELL_IO_REDIRECTION_APPEND = 2
} de_shell_io_redirection_type_t;

typedef struct de_shell_io_redirection {
    de_shell_io_redirection_type_t type;
    const char *target;
    size_t source_offset;
} de_shell_io_redirection_t;

typedef struct de_shell_pipeline_stage de_shell_pipeline_stage_t;

/*
 * One external command request. argv[0] is the executable name/path and
 * every argument remains a separate NUL-terminated string. argv[argc] is a
 * NULL sentinel. All pointers are borrowed and valid only until the executor
 * callback returns.
 */
typedef struct de_shell_external_command {
    const char *executable;
    size_t argc;
    const char *const *argv;
    const de_shell_environment_entry_t *environment;
    size_t environment_count;
    const char *working_directory;
    /*
     * When pipeline_stage_count is non-zero, this is a complete pipeline or
     * redirection-only execution plan. In that form executable/argc/argv are
     * NULL/zero and pipeline_stages contains the ordered stages. A stage with
     * command == NULL performs redirections only.
     */
    const de_shell_pipeline_stage_t *pipeline_stages;
    size_t pipeline_stage_count;
} de_shell_external_command_t;

struct de_shell_pipeline_stage {
    const de_shell_external_command_t *command;
    const de_shell_io_redirection_t *redirections;
    size_t redirection_count;
};

/*
 * Host-provided process executor. For a normal simple command the request
 * contains one command and pipeline_stage_count == 0. For pipelines or any
 * redirection it contains a complete immutable plan in pipeline_stages; the
 * runtime must prepare every stage before waiting, own every pipe/file
 * descriptor, and reap/cancel all started children on every failure path.
 *
 * The DE-Shell C core does not call system(),
 * popen(), fork(), exec(), or an implicit command interpreter. A Rust/runtime
 * adapter supplies this callback and owns actual process creation.
 *
 * Return DE_SHELL_STATUS_OK when the command was launched and waited for;
 * write its exit code to out_exit_code. Return another status when launch or
 * runtime handling failed. The callback must not retain borrowed pointers.
 */
typedef de_shell_status_t (*de_shell_external_executor_t)(
    const de_shell_external_command_t *command,
    int *out_exit_code,
    void *user_data);

/*
 * Immutable per-invocation execution context. A NULL working_directory means
 * the host runtime's current/default directory. Environment entries are
 * explicit overrides and are passed unchanged to every external command.
 */
typedef struct de_shell_execution_context {
    const de_shell_environment_entry_t *environment;
    size_t environment_count;
    const char *working_directory;
    de_shell_external_executor_t external_executor;
    void *user_data;
} de_shell_execution_context_t;

/*
 * Result of the commands that completed before execution returned. A normal
 * command exit (including a non-zero exit code) is not an API error.
 */
typedef struct de_shell_execution_result {
    int exit_code;
    int exit_requested;
    size_t commands_executed;
} de_shell_execution_result_t;

typedef enum de_shell_execution_stage {
    DE_SHELL_EXECUTION_STAGE_NONE = 0,
    DE_SHELL_EXECUTION_STAGE_INPUT = 1,
    DE_SHELL_EXECUTION_STAGE_LEX = 2,
    DE_SHELL_EXECUTION_STAGE_PARSE = 3,
    DE_SHELL_EXECUTION_STAGE_DISPATCH = 4,
    DE_SHELL_EXECUTION_STAGE_EXTERNAL = 5,
    DE_SHELL_EXECUTION_STAGE_EXPAND = 6
} de_shell_execution_stage_t;

/*
 * code contains the underlying lexer/parser/registry/callback status when
 * applicable. source_offset is byte-based; SIZE_MAX means no source location.
 */
typedef struct de_shell_execution_diagnostic {
    de_shell_execution_stage_t stage;
    int code;
    size_t source_offset;
} de_shell_execution_diagnostic_t;

/*
 * Lexes and parses one source buffer, then executes the supported alpha AST.
 *
 * Supported execution nodes:
 * - simple commands (built-ins or host-provided external commands);
 * - external-command pipelines and <, >, >> redirections through the host
 *   executor's typed execution-plan request;
 * - ';', '&&', and '||' control flow with left-to-right shell semantics.
 *
 * Background execution and parenthesized subshell groups remain unsupported.
 * Pipeline stages containing built-ins are rejected until isolated child
 * contexts are defined. External pipeline stages and redirections are passed
 * as a single typed plan to the host executor; the C core never opens files,
 * creates pipes, or launches processes.
 *
 * Quotes and backslash escapes are removed from word source spans. The
 * execution layer supports scalar $NAME, ${NAME}, and $? expansion outside
 * single quotes. Assignment-only commands persist session-local variables;
 * session variables shadow same-name entries in the explicit host environment.
 * Assignment prefixes are temporary environment overrides for external
 * commands. Values are never field-split, glob-expanded, or recursively
 * expanded. Command and arithmetic substitution are not supported.
 * Source is bounded to DE_SHELL_EXEC_MAX_SOURCE_BYTES; individual decoded
 * words and argument count are bounded by the constants above.
 *
 * The call is synchronous and single-thread-affine to the owning shell. It
 * must run on a worker thread when the external executor can block. Reentrant
 * execution or destruction of the same shell from its callback returns BUSY.
 * Environment, working-directory, callback, and user_data pointers are
 * borrowed for the duration of this call.
 *
 * On a non-OK return, the diagnostic identifies the failing stage and source
 * offset. commands_executed and exit_code describe commands completed before
 * the failure; external side effects cannot be rolled back.
 */
de_shell_status_t de_shell_execute_source(
    de_shell_t *shell,
    const unsigned char *source,
    size_t source_length,
    const de_shell_execution_context_t *context,
    de_shell_execution_result_t *out_result,
    de_shell_execution_diagnostic_t *out_diagnostic);

#ifdef __cplusplus
}
#endif

#endif
