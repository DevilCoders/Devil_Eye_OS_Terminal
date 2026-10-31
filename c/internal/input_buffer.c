#include "input_buffer.h"

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

struct de_shell_input_buffer {
    unsigned char *data;
    size_t length;
    size_t max_length;
    int line_ready;
    int eof;
};

de_shell_input_status_t de_shell_input_buffer_create(
    size_t max_length,
    de_shell_input_buffer_t **out_buffer)
{
    de_shell_input_buffer_t *buffer = NULL;

    if (out_buffer == NULL) {
        return DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT;
    }

    *out_buffer = NULL;

    if (max_length == 0U || max_length == SIZE_MAX) {
        return DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT;
    }

    buffer = calloc(1U, sizeof(*buffer));
    if (buffer == NULL) {
        return DE_SHELL_INPUT_STATUS_OUT_OF_MEMORY;
    }

    buffer->data = malloc(max_length + 1U);
    if (buffer->data == NULL) {
        free(buffer);
        return DE_SHELL_INPUT_STATUS_OUT_OF_MEMORY;
    }

    buffer->max_length = max_length;
    *out_buffer = buffer;
    return DE_SHELL_INPUT_STATUS_OK;
}

de_shell_input_status_t de_shell_input_buffer_destroy(
    de_shell_input_buffer_t **buffer)
{
    if (buffer == NULL) {
        return DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT;
    }

    if (*buffer != NULL) {
        free((*buffer)->data);
        (*buffer)->data = NULL;
        free(*buffer);
        *buffer = NULL;
    }

    return DE_SHELL_INPUT_STATUS_OK;
}

de_shell_input_status_t de_shell_input_buffer_append(
    de_shell_input_buffer_t *buffer,
    const unsigned char *data,
    size_t length)
{
    if (buffer == NULL || (data == NULL && length != 0U)) {
        return DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT;
    }

    if (buffer->eof != 0 || buffer->line_ready != 0) {
        return DE_SHELL_INPUT_STATUS_INVALID_STATE;
    }

    if (length > buffer->max_length - buffer->length) {
        return DE_SHELL_INPUT_STATUS_LIMIT_EXCEEDED;
    }

    if (length != 0U) {
        (void)memcpy(buffer->data + buffer->length, data, length);
        buffer->length += length;
    }

    return DE_SHELL_INPUT_STATUS_OK;
}

de_shell_input_status_t de_shell_input_buffer_submit_line(
    de_shell_input_buffer_t *buffer)
{
    if (buffer == NULL) {
        return DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT;
    }

    if (buffer->eof != 0 || buffer->line_ready != 0) {
        return DE_SHELL_INPUT_STATUS_INVALID_STATE;
    }

    buffer->line_ready = 1;
    return DE_SHELL_INPUT_STATUS_OK;
}

de_shell_input_status_t de_shell_input_buffer_read_line(
    de_shell_input_buffer_t *buffer,
    unsigned char *destination,
    size_t destination_capacity,
    size_t *out_length)
{
    if (buffer == NULL || out_length == NULL) {
        return DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT;
    }

    *out_length = 0U;

    if (buffer->line_ready == 0) {
        return buffer->eof != 0
            ? DE_SHELL_INPUT_STATUS_EOF
            : DE_SHELL_INPUT_STATUS_EMPTY;
    }

    if (buffer->length > destination_capacity
        || (buffer->length != 0U && destination == NULL)) {
        return destination == NULL && buffer->length != 0U
            ? DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT
            : DE_SHELL_INPUT_STATUS_DESTINATION_TOO_SMALL;
    }

    if (buffer->length != 0U) {
        (void)memcpy(destination, buffer->data, buffer->length);
    }

    *out_length = buffer->length;
    buffer->length = 0U;
    buffer->line_ready = 0;
    return DE_SHELL_INPUT_STATUS_OK;
}

de_shell_input_status_t de_shell_input_buffer_mark_eof(
    de_shell_input_buffer_t *buffer)
{
    if (buffer == NULL) {
        return DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT;
    }

    if (buffer->eof != 0) {
        return DE_SHELL_INPUT_STATUS_OK;
    }

    buffer->eof = 1;
    if (buffer->length != 0U) {
        buffer->line_ready = 1;
    }

    return DE_SHELL_INPUT_STATUS_OK;
}
