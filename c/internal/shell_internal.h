#ifndef DE_SHELL_INTERNAL_SHELL_INTERNAL_H
#define DE_SHELL_INTERNAL_SHELL_INTERNAL_H

typedef struct de_shell_command_registry de_shell_command_registry_t;

struct de_shell {
    unsigned int lifecycle_state;
    de_shell_command_registry_t *command_registry;
    unsigned int execution_active;
};

enum {
    DE_SHELL_INTERNAL_STATE_READY = 1U
};

#endif
