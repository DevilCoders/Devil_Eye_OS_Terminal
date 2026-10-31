#ifndef DE_SHELL_INTERNAL_SHELL_INTERNAL_H
#define DE_SHELL_INTERNAL_SHELL_INTERNAL_H

struct de_shell {
    unsigned int lifecycle_state;
};

enum {
    DE_SHELL_INTERNAL_STATE_READY = 1U
};

#endif
