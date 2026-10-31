#include "de_shell/build_info.h"

#include <stdio.h>
#include <string.h>

int main(void)
{
    const char *version = de_shell_build_version();

    if (version == NULL || version[0] == '\0') {
        (void)fprintf(stderr, "de_shell_build_version returned an empty value\n");
        return 1;
    }

    if (strcmp(version, "0.1.0") != 0) {
        (void)fprintf(stderr, "unexpected DE-Shell build version: %s\n", version);
        return 1;
    }

    (void)printf("DE-Shell build metadata: version %s\n", version);
    return 0;
}
