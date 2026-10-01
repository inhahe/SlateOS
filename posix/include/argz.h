/*
 * SlateOS: <argz.h>, which musl does not have -- glibc's argz vectors, a list
 * of strings kept as one block, each followed by its NUL, with the block's
 * length beside it (posix/src/argz.rs).
 */

#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_ARGZ_H
#define _SLATEOS_ARGZ_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifndef __error_t_defined
#define __error_t_defined 1
typedef int error_t;
#endif

error_t argz_create(char *const[], char **__restrict, size_t *__restrict);
error_t argz_create_sep(const char *__restrict, int, char **__restrict, size_t *__restrict);
size_t argz_count(const char *, size_t);
void argz_extract(const char *__restrict, size_t, char **__restrict);
void argz_stringify(char *, size_t, int);
error_t argz_append(char **__restrict, size_t *__restrict, const char *__restrict, size_t);
error_t argz_add(char **__restrict, size_t *__restrict, const char *__restrict);
error_t argz_add_sep(char **__restrict, size_t *__restrict, const char *__restrict, int);
void argz_delete(char **__restrict, size_t *__restrict, char *__restrict);
error_t argz_insert(char **__restrict, size_t *__restrict, char *__restrict, const char *__restrict);
error_t argz_replace(char **__restrict, size_t *__restrict, const char *__restrict,
                     const char *__restrict, unsigned int *__restrict);
char *argz_next(const char *__restrict, size_t, const char *__restrict);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_ARGZ_H */
