#ifndef DE_SHELL_INTERNAL_SHELL_INTERNAL_H
#define DE_SHELL_INTERNAL_SHELL_INTERNAL_H

#include <stddef.h>

#define DE_SHELL_INTERNAL_MAX_VARIABLES 128U

typedef struct de_shell_variable {
    char *name;
    char *value;
} de_shell_variable_t;

typedef struct de_shell_command_registry de_shell_command_registry_t;

struct de_shell {
    unsigned int lifecycle_state;
    de_shell_command_registry_t *command_registry;
    unsigned int execution_active;
    size_t variable_count;
    de_shell_variable_t variables[DE_SHELL_INTERNAL_MAX_VARIABLES];
};

enum {
    DE_SHELL_INTERNAL_STATE_READY = 1U
};

#endif
