#ifndef DE_SHELL_BUILD_INFO_H
#define DE_SHELL_BUILD_INFO_H

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Returns the version embedded by the CMake build.
 *
 * The returned string has static storage duration and must not be modified
 * or freed by the caller.
 */
const char *de_shell_build_version(void);

#ifdef __cplusplus
}
#endif

#endif
