/*
 * SlateOS: what this C library's <assert.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

/* No guard around what follows, as none is around musl's: a program may
 * include <assert.h> again after defining or undefining NDEBUG, and
 * assert_perror must follow assert each time. */
#include_next <assert.h>

/* GNU's assert_perror(errnum): nothing when errnum is 0 (or NDEBUG is
 * defined), else glibc's "Unexpected error: " message with strerror's text
 * for it, and abort. */
#ifdef _GNU_SOURCE
#undef assert_perror
#ifdef NDEBUG
#define assert_perror(errnum) ((void) 0)
#else
#define assert_perror(errnum) \
    (!(errnum) ? (void) 0 : __assert_perror_fail((errnum), __FILE__, __LINE__, __func__))
#endif
#endif

/* The two entries glibc declares beside musl's __assert_fail. */
#ifndef _SLATEOS_ASSERT_H_DECLS
#define _SLATEOS_ASSERT_H_DECLS
#ifdef __cplusplus
extern "C" {
#endif
_Noreturn void __assert_perror_fail(int, const char *, unsigned int, const char *);
_Noreturn void __assert(const char *, const char *, int);
#ifdef __cplusplus
}
#endif
#endif
