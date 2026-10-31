#include "internal/command_registry.h"

#include <stdlib.h>
#include <string.h>

typedef struct de_shell_command_entry {
    de_shell_command_info_t info;
    de_shell_builtin_handler_t handler;
    void *user_data;
} de_shell_command_entry_t;

struct de_shell_command_registry {
    de_shell_command_entry_t *entries;
    size_t count;
    size_t capacity;
};

static int de_shell_command_name_is_valid(const char *name)
{
    size_t index;

    if (name == NULL || name[0] == '\0') {
        return 0;
    }

    for (index = 0U; name[index] != '\0'; index += 1U) {
        const unsigned char value = (unsigned char)name[index];

        if (value <= (unsigned char)' '
            || value >= UINT8_C(0x7F)
            || value == (unsigned char)'|'
            || value == (unsigned char)'&'
            || value == (unsigned char)';'
            || value == (unsigned char)'<'
            || value == (unsigned char)'>'
            || value == (unsigned char)'('
            || value == (unsigned char)')'
            || value == (unsigned char)'\\'
            || value == (unsigned char)'\''
            || value == (unsigned char)'"') {
            return 0;
        }
    }

    return 1;
}

static char *de_shell_copy_string(const char *value)
{
    const size_t length = strlen(value);
    char *copy = malloc(length + 1U);

    if (copy != NULL) {
        (void)memcpy(copy, value, length + 1U);
    }

    return copy;
}

static size_t de_shell_find_command(
    const de_shell_command_registry_t *registry,
    const char *name)
{
    size_t index;

    for (index = 0U; index < registry->count; index += 1U) {
        if (strcmp(registry->entries[index].info.name, name) == 0) {
            return index;
        }
    }

    return registry->count;
}

static de_shell_registry_status_t de_shell_command_registry_grow(
    de_shell_command_registry_t *registry)
{
    size_t new_capacity;
    de_shell_command_entry_t *new_entries;

    if (registry->capacity == 0U) {
        new_capacity = 4U;
    } else {
        if (registry->capacity > SIZE_MAX / 2U) {
            return DE_SHELL_REGISTRY_STATUS_OUT_OF_MEMORY;
        }
        new_capacity = registry->capacity * 2U;
    }

    if (new_capacity > SIZE_MAX / sizeof(*registry->entries)) {
        return DE_SHELL_REGISTRY_STATUS_OUT_OF_MEMORY;
    }

    new_entries = realloc(
        registry->entries,
        new_capacity * sizeof(*registry->entries));
    if (new_entries == NULL) {
        return DE_SHELL_REGISTRY_STATUS_OUT_OF_MEMORY;
    }

    registry->entries = new_entries;
    registry->capacity = new_capacity;
    return DE_SHELL_REGISTRY_STATUS_OK;
}

static de_shell_command_result_t de_shell_builtin_exit(
    de_shell_t *shell,
    size_t argc,
    const char *const argv[],
    void *user_data)
{
    (void)shell;
    (void)argc;
    (void)argv;
    (void)user_data;
    return DE_SHELL_COMMAND_RESULT_EXIT_REQUESTED;
}

de_shell_registry_status_t de_shell_command_registry_create(
    de_shell_command_registry_t **out_registry)
{
    de_shell_command_registry_t *registry = NULL;

    if (out_registry == NULL) {
        return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
    }

    *out_registry = NULL;
    registry = calloc(1U, sizeof(*registry));
    if (registry == NULL) {
        return DE_SHELL_REGISTRY_STATUS_OUT_OF_MEMORY;
    }

    *out_registry = registry;
    return DE_SHELL_REGISTRY_STATUS_OK;
}

de_shell_registry_status_t de_shell_command_registry_destroy(
    de_shell_command_registry_t **registry)
{
    size_t index;

    if (registry == NULL) {
        return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
    }

    if (*registry == NULL) {
        return DE_SHELL_REGISTRY_STATUS_OK;
    }

    for (index = 0U; index < (*registry)->count; index += 1U) {
        free((void *)(*registry)->entries[index].info.name);
        free((void *)(*registry)->entries[index].info.description);
    }

    free((*registry)->entries);
    free(*registry);
    *registry = NULL;

    return DE_SHELL_REGISTRY_STATUS_OK;
}

de_shell_registry_status_t de_shell_command_registry_register(
    de_shell_command_registry_t *registry,
    const de_shell_command_definition_t *definition)
{
    char *name_copy = NULL;
    char *description_copy = NULL;
    de_shell_command_entry_t *entry = NULL;
    de_shell_registry_status_t status;

    if (registry == NULL
        || definition == NULL
        || !de_shell_command_name_is_valid(definition->name)
        || definition->handler == NULL
        || definition->min_args > definition->max_args) {
        return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
    }

    if (de_shell_find_command(registry, definition->name) != registry->count) {
        return DE_SHELL_REGISTRY_STATUS_DUPLICATE_NAME;
    }

    name_copy = de_shell_copy_string(definition->name);
    description_copy = de_shell_copy_string(
        definition->description != NULL ? definition->description : "");
    if (name_copy == NULL || description_copy == NULL) {
        free(name_copy);
        free(description_copy);
        return DE_SHELL_REGISTRY_STATUS_OUT_OF_MEMORY;
    }

    if (registry->count == registry->capacity) {
        status = de_shell_command_registry_grow(registry);
        if (status != DE_SHELL_REGISTRY_STATUS_OK) {
            free(name_copy);
            free(description_copy);
            return status;
        }
    }

    entry = &registry->entries[registry->count];
    entry->info.name = name_copy;
    entry->info.description = description_copy;
    entry->info.min_args = definition->min_args;
    entry->info.max_args = definition->max_args;
    entry->info.flags = definition->flags;
    entry->handler = definition->handler;
    entry->user_data = definition->user_data;
    registry->count += 1U;

    return DE_SHELL_REGISTRY_STATUS_OK;
}

de_shell_registry_status_t de_shell_command_registry_lookup(
    const de_shell_command_registry_t *registry,
    const char *name,
    const de_shell_command_info_t **out_info)
{
    size_t index;

    if (out_info == NULL) {
        return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
    }

    *out_info = NULL;
    if (registry == NULL || !de_shell_command_name_is_valid(name)) {
        return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
    }

    index = de_shell_find_command(registry, name);
    if (index == registry->count) {
        return DE_SHELL_REGISTRY_STATUS_NOT_FOUND;
    }

    *out_info = &registry->entries[index].info;
    return DE_SHELL_REGISTRY_STATUS_OK;
}

de_shell_registry_status_t de_shell_command_registry_dispatch(
    const de_shell_command_registry_t *registry,
    de_shell_t *shell,
    size_t argc,
    const char *const argv[],
    de_shell_command_result_t *out_result)
{
    size_t index;
    size_t argument_count;
    size_t argument_index;
    const de_shell_command_entry_t *entry;

    if (out_result == NULL) {
        return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
    }

    *out_result = DE_SHELL_COMMAND_RESULT_FAILURE;
    if (registry == NULL || shell == NULL || argc == 0U || argv == NULL
        || argv[0] == NULL) {
        return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
    }

    for (argument_index = 0U; argument_index < argc; argument_index += 1U) {
        if (argv[argument_index] == NULL) {
            return DE_SHELL_REGISTRY_STATUS_INVALID_ARGUMENT;
        }
    }

    index = de_shell_find_command(registry, argv[0]);
    if (index == registry->count) {
        return DE_SHELL_REGISTRY_STATUS_NOT_FOUND;
    }

    entry = &registry->entries[index];
    argument_count = argc - 1U;
    if (argument_count < entry->info.min_args
        || argument_count > entry->info.max_args) {
        return DE_SHELL_REGISTRY_STATUS_ARGUMENT_COUNT_MISMATCH;
    }

    *out_result = entry->handler(shell, argc, argv, entry->user_data);
    if (*out_result != DE_SHELL_COMMAND_RESULT_SUCCESS
        && *out_result != DE_SHELL_COMMAND_RESULT_EXIT_REQUESTED
        && *out_result != DE_SHELL_COMMAND_RESULT_FAILURE) {
        *out_result = DE_SHELL_COMMAND_RESULT_FAILURE;
        return DE_SHELL_REGISTRY_STATUS_HANDLER_FAILED;
    }

    return DE_SHELL_REGISTRY_STATUS_OK;
}

de_shell_registry_status_t de_shell_command_registry_register_core(
    de_shell_command_registry_t *registry)
{
    static const de_shell_command_definition_t exit_command = {
        "exit",
        "Request termination of the current DE-Shell session.",
        0U,
        0U,
        DE_SHELL_COMMAND_FLAG_TERMINATES_SHELL,
        de_shell_builtin_exit,
        NULL,
    };

    return de_shell_command_registry_register(registry, &exit_command);
}
