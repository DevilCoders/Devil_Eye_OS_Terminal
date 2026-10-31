#include "de_shell/build_info.h"

#ifndef DE_SHELL_VERSION
#define DE_SHELL_VERSION "0.0.0-unknown"
#endif

const char *de_shell_build_version(void)
{
    return DE_SHELL_VERSION;
}
