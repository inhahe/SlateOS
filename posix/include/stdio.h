/*
 * SlateOS: what this C library's <stdio.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <stdio.h>

#ifndef _SLATEOS_STDIO_H
#define _SLATEOS_STDIO_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* tmpnam into the caller's L_tmpnam bytes; NULL is refused, not a static
 * buffer. */
char *tmpnam_r(char *);
#endif

#ifdef _GNU_SOURCE
/* Close every stream. */
int fcloseall(void);

/* renameat, with flags: fail if the new name exists, swap the two names, or
 * leave a whiteout at the old one. */
#define RENAME_NOREPLACE (1 << 0)
#define RENAME_EXCHANGE (1 << 1)
#define RENAME_WHITEOUT (1 << 2)
int renameat2(int, const char *, int, const char *, unsigned int);

/* Formatted output onto the end of the object growing in an obstack
 * (<obstack.h>), with no NUL after it: the bytes added, or -1. */
struct obstack;
int obstack_printf(struct obstack *__restrict, const char *__restrict, ...)
    _SLATEOS_PRINTF(2, 3);
int obstack_vprintf(struct obstack *__restrict, const char *__restrict, va_list)
    _SLATEOS_PRINTF(2, 0);
#endif

/* fopencookie: a stream over the caller's four functions. glibc declares it
 * by default; musl's header, and its types, only for _GNU_SOURCE -- so the
 * types are musl's own, here, where its header has not given them. */
#if defined(_SLATEOS_USE_MISC) && !defined(_GNU_SOURCE)
typedef ssize_t (cookie_read_function_t)(void *, char *, size_t);
typedef ssize_t (cookie_write_function_t)(void *, const char *, size_t);
typedef int (cookie_seek_function_t)(void *, off_t *, int);
typedef int (cookie_close_function_t)(void *);

typedef struct _IO_cookie_io_functions_t {
	cookie_read_function_t *read;
	cookie_write_function_t *write;
	cookie_seek_function_t *seek;
	cookie_close_function_t *close;
} cookie_io_functions_t;

FILE *fopencookie(void *, const char *, cookie_io_functions_t);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_STDIO_H */
