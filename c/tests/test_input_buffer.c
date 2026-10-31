#include "internal/input_buffer.h"

#include <stdio.h>
#include <string.h>

#define TEST_CAPACITY 8U

static int expect_status(
    const char *name,
    de_shell_input_status_t actual,
    de_shell_input_status_t expected)
{
    if (actual != expected) {
        (void)fprintf(
            stderr,
            "%s: expected status %d, got %d\n",
            name,
            (int)expected,
            (int)actual);
        return 1;
    }

    return 0;
}

static int test_empty_line_submission(void)
{
    de_shell_input_buffer_t *buffer = NULL;
    unsigned char output[TEST_CAPACITY];
    size_t output_length = 99U;

    if (expect_status(
            "create",
            de_shell_input_buffer_create(TEST_CAPACITY, &buffer),
            DE_SHELL_INPUT_STATUS_OK) != 0) {
        return 1;
    }

    if (expect_status(
            "submit empty line",
            de_shell_input_buffer_submit_line(buffer),
            DE_SHELL_INPUT_STATUS_OK) != 0
        || expect_status(
            "read empty line",
            de_shell_input_buffer_read_line(
                buffer, output, sizeof(output), &output_length),
            DE_SHELL_INPUT_STATUS_OK) != 0
        || output_length != 0U) {
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    return expect_status(
        "destroy",
        de_shell_input_buffer_destroy(&buffer),
        DE_SHELL_INPUT_STATUS_OK);
}

static int test_maximum_length_and_reuse(void)
{
    de_shell_input_buffer_t *buffer = NULL;
    const unsigned char input[TEST_CAPACITY] = {'1', '2', '3', '4', '5', '6', '7', '8'};
    unsigned char output[TEST_CAPACITY];
    size_t output_length = 0U;

    if (de_shell_input_buffer_create(TEST_CAPACITY, &buffer)
        != DE_SHELL_INPUT_STATUS_OK) {
        return 1;
    }

    if (de_shell_input_buffer_append(buffer, input, sizeof(input))
            != DE_SHELL_INPUT_STATUS_OK
        || de_shell_input_buffer_submit_line(buffer) != DE_SHELL_INPUT_STATUS_OK
        || de_shell_input_buffer_read_line(
               buffer, output, sizeof(output), &output_length)
            != DE_SHELL_INPUT_STATUS_OK
        || output_length != sizeof(input)
        || memcmp(input, output, sizeof(input)) != 0) {
        (void)fprintf(stderr, "maximum-length line did not round-trip\n");
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    if (de_shell_input_buffer_append(buffer, input, 1U) != DE_SHELL_INPUT_STATUS_OK) {
        (void)fprintf(stderr, "buffer was not reusable after line consumption\n");
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    return expect_status(
        "destroy",
        de_shell_input_buffer_destroy(&buffer),
        DE_SHELL_INPUT_STATUS_OK);
}

static int test_overflow_is_atomic(void)
{
    de_shell_input_buffer_t *buffer = NULL;
    const unsigned char prefix[] = {'a', 'b', 'c'};
    const unsigned char overflow[] = {'d', 'e', 'f', 'g', 'h', 'i'};
    unsigned char output[TEST_CAPACITY];
    size_t output_length = 0U;

    if (de_shell_input_buffer_create(TEST_CAPACITY, &buffer)
        != DE_SHELL_INPUT_STATUS_OK) {
        return 1;
    }

    if (de_shell_input_buffer_append(buffer, prefix, sizeof(prefix))
            != DE_SHELL_INPUT_STATUS_OK
        || expect_status(
               "overflow",
               de_shell_input_buffer_append(buffer, overflow, sizeof(overflow)),
               DE_SHELL_INPUT_STATUS_LIMIT_EXCEEDED) != 0
        || de_shell_input_buffer_submit_line(buffer) != DE_SHELL_INPUT_STATUS_OK
        || de_shell_input_buffer_read_line(
               buffer, output, sizeof(output), &output_length)
            != DE_SHELL_INPUT_STATUS_OK
        || output_length != sizeof(prefix)
        || memcmp(output, prefix, sizeof(prefix)) != 0) {
        (void)fprintf(stderr, "overflow changed or corrupted buffered input\n");
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    return expect_status(
        "destroy",
        de_shell_input_buffer_destroy(&buffer),
        DE_SHELL_INPUT_STATUS_OK);
}

static int test_eof_flushes_final_line_then_reports_eof(void)
{
    de_shell_input_buffer_t *buffer = NULL;
    const unsigned char input[] = {'e', 'o', 'f'};
    unsigned char output[TEST_CAPACITY];
    size_t output_length = 0U;

    if (de_shell_input_buffer_create(TEST_CAPACITY, &buffer)
        != DE_SHELL_INPUT_STATUS_OK) {
        return 1;
    }

    if (de_shell_input_buffer_append(buffer, input, sizeof(input))
            != DE_SHELL_INPUT_STATUS_OK
        || de_shell_input_buffer_mark_eof(buffer) != DE_SHELL_INPUT_STATUS_OK
        || de_shell_input_buffer_read_line(
               buffer, output, sizeof(output), &output_length)
            != DE_SHELL_INPUT_STATUS_OK
        || output_length != sizeof(input)
        || memcmp(input, output, sizeof(input)) != 0
        || expect_status(
               "eof after final line",
               de_shell_input_buffer_read_line(
                   buffer, output, sizeof(output), &output_length),
               DE_SHELL_INPUT_STATUS_EOF) != 0
        || expect_status(
               "append after eof",
               de_shell_input_buffer_append(buffer, input, sizeof(input)),
               DE_SHELL_INPUT_STATUS_INVALID_STATE) != 0) {
        (void)fprintf(stderr, "EOF handling failed\n");
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    return expect_status(
        "destroy",
        de_shell_input_buffer_destroy(&buffer),
        DE_SHELL_INPUT_STATUS_OK);
}

static int test_eof_without_pending_input(void)
{
    de_shell_input_buffer_t *buffer = NULL;
    unsigned char output[TEST_CAPACITY];
    size_t output_length = 0U;

    if (de_shell_input_buffer_create(TEST_CAPACITY, &buffer)
        != DE_SHELL_INPUT_STATUS_OK) {
        return 1;
    }

    if (de_shell_input_buffer_mark_eof(buffer) != DE_SHELL_INPUT_STATUS_OK
        || expect_status(
               "empty eof",
               de_shell_input_buffer_read_line(
                   buffer, output, sizeof(output), &output_length),
               DE_SHELL_INPUT_STATUS_EOF) != 0) {
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    return expect_status(
        "destroy",
        de_shell_input_buffer_destroy(&buffer),
        DE_SHELL_INPUT_STATUS_OK);
}

static int test_destination_too_small_preserves_line(void)
{
    de_shell_input_buffer_t *buffer = NULL;
    const unsigned char input[] = {'f', 'o', 'u', 'r'};
    unsigned char output[2];
    unsigned char complete_output[TEST_CAPACITY];
    size_t output_length = 0U;

    if (de_shell_input_buffer_create(TEST_CAPACITY, &buffer)
        != DE_SHELL_INPUT_STATUS_OK) {
        return 1;
    }

    if (de_shell_input_buffer_append(buffer, input, sizeof(input))
            != DE_SHELL_INPUT_STATUS_OK
        || de_shell_input_buffer_submit_line(buffer) != DE_SHELL_INPUT_STATUS_OK
        || expect_status(
               "small destination",
               de_shell_input_buffer_read_line(
                   buffer, output, sizeof(output), &output_length),
               DE_SHELL_INPUT_STATUS_DESTINATION_TOO_SMALL) != 0
        || de_shell_input_buffer_read_line(
               buffer, complete_output, sizeof(complete_output), &output_length)
            != DE_SHELL_INPUT_STATUS_OK
        || output_length != sizeof(input)
        || memcmp(input, complete_output, sizeof(input)) != 0) {
        (void)fprintf(stderr, "small destination lost the pending line\n");
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    return expect_status(
        "destroy",
        de_shell_input_buffer_destroy(&buffer),
        DE_SHELL_INPUT_STATUS_OK);
}

static int test_invalid_arguments_and_states(void)
{
    de_shell_input_buffer_t *buffer = NULL;
    size_t output_length = 0U;

    if (expect_status(
            "zero capacity",
            de_shell_input_buffer_create(0U, &buffer),
            DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT) != 0
        || expect_status(
            "null create output",
            de_shell_input_buffer_create(TEST_CAPACITY, NULL),
            DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT) != 0
        || expect_status(
            "null append buffer",
            de_shell_input_buffer_append(NULL, NULL, 0U),
            DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT) != 0
        || expect_status(
            "null append data",
            de_shell_input_buffer_append(buffer, NULL, 1U),
            DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT) != 0) {
        return 1;
    }

    if (de_shell_input_buffer_create(TEST_CAPACITY, &buffer)
        != DE_SHELL_INPUT_STATUS_OK) {
        return 1;
    }

    if (expect_status(
            "read without submitted line",
            de_shell_input_buffer_read_line(
                buffer, NULL, 0U, &output_length),
            DE_SHELL_INPUT_STATUS_EMPTY) != 0
        || expect_status(
            "submit line",
            de_shell_input_buffer_submit_line(buffer),
            DE_SHELL_INPUT_STATUS_OK) != 0
        || expect_status(
            "duplicate submit",
            de_shell_input_buffer_submit_line(buffer),
            DE_SHELL_INPUT_STATUS_INVALID_STATE) != 0
        || expect_status(
            "null output length",
            de_shell_input_buffer_read_line(buffer, NULL, 0U, NULL),
            DE_SHELL_INPUT_STATUS_INVALID_ARGUMENT) != 0) {
        (void)de_shell_input_buffer_destroy(&buffer);
        return 1;
    }

    return expect_status(
        "destroy",
        de_shell_input_buffer_destroy(&buffer),
        DE_SHELL_INPUT_STATUS_OK);
}

int main(void)
{
    if (test_empty_line_submission() != 0
        || test_maximum_length_and_reuse() != 0
        || test_overflow_is_atomic() != 0
        || test_eof_flushes_final_line_then_reports_eof() != 0
        || test_eof_without_pending_input() != 0
        || test_destination_too_small_preserves_line() != 0
        || test_invalid_arguments_and_states() != 0) {
        return 1;
    }

    (void)puts("DE-Shell input buffer tests passed");
    return 0;
}
