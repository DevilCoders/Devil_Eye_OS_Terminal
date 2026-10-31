#ifndef DE_SHELL_INTERNAL_INPUT_BUFFER_H
#define DE_SHELL_INTERNAL_INPUT_BUFFER_H

#include <stddef.h>

typedef struct de_shell_input_buffer de_shell_input_buffer_t;

typedef enum de_shell_input_status {
    DE_SHELL_INPUT_STATUS_OK = 0,
    DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT = 1,
    DE_SHELL_INPUT_STATUS_LIMIT_EXCEEDED = 2,
    DE_SHELL_INPUT_STATUS_OUT_OF_MEMORY = 3,
    DE_SHELL_INPUT_STATUS_EMPTY = 4,
    DE_SHELL_INPUT_STATUS_EOF = 5,
    DE_SHELL_INPUT_STATUS_INVALID_STATE = 6,
    DE_SHELL_INPUT_STATUS_DESTINATION_TOO_SMALL = 7
} de_shell_input_status_t;

/*
 * Creates a bounded byte buffer. max_length must be non-zero.
 * The output is set to NULL before allocation.
 */
de_shell_input_status_t de_shell_input_buffer_create(
    size_t max_length,
    de_shell_input_buffer_t **out_buffer);

/* Releases a buffer and clears the caller's handle; NULL handles are allowed. */
de_shell_input_status_t de_shell_input_buffer_destroy(
    de_shell_input_buffer_t **buffer);

/* Appends bytes atomically; a rejected append leaves existing data unchanged. */
de_shell_input_status_t de_shell_input_buffer_append(
    de_shell_input_buffer_t *buffer,
    const unsigned char *data,
    size_t length);

/* Marks the current line ready for consumption; empty lines are valid. */
de_shell_input_status_t de_shell_input_buffer_submit_line(
    de_shell_input_buffer_t *buffer);

/*
 * Copies one submitted line into caller-owned storage.
 * A too-small destination leaves the line pending.
 * If EOF was marked with unterminated bytes, that final line is returned
 * before EOF is reported on the next call.
 */
de_shell_input_status_t de_shell_input_buffer_read_line(
    de_shell_input_buffer_t *buffer,
    unsigned char *destination,
    size_t destination_capacity,
    size_t *out_length);

/* Marks input closed. Pending bytes are exposed as one final line. */
de_shell_input_status_t de_shell_input_buffer_mark_eof(
    de_shell_input_buffer_t *buffer);

#endif
