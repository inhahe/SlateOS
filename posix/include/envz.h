/*
 * SlateOS: <envz.h>, which musl does not have -- glibc's envz vectors, argz
 * vectors of name=value entries, like an environment (posix/src/argz.rs).
 */

#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_ENVZ_H
#define _SLATEOS_ENVZ_H

#include <argz.h>

#ifdef __cplusplus
extern "C" {
#endif

char *envz_entry(const char *__restrict, size_t, const char *__restrict);
char *envz_get(const char *__restrict, size_t, const char *__restrict);
error_t envz_add(char **__restrict, size_t *__restrict, const char *__restrict,
                 const char *__restrict);
error_t envz_merge(char **__restrict, size_t *__restrict, const char *__restrict, size_t, int);
void envz_remove(char **__restrict, size_t *__restrict, const char *__restrict);
void envz_strip(char **__restrict, size_t *__restrict);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_ENVZ_H */
