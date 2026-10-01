/*
 * SlateOS: what this C library's <strings.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <strings.h>

#ifndef _SLATEOS_STRINGS_H
#define _SLATEOS_STRINGS_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The BSD functions POSIX.1-2008 dropped: glibc declares them by default and
 * wherever POSIX.1-2008 is not asked for, strict ISO C included; musl's
 * header not for strict ISO C. */
#if defined(_SLATEOS_USE_MISC) || !defined(_SLATEOS_USE_POSIX2008)
int   bcmp(const void *, const void *, size_t);
void  bcopy(const void *, void *, size_t);
void  bzero(void *, size_t);
char *index(const char *, int);
char *rindex(const char *, int);
#endif

/* ffs, which the XSI kept. */
#if defined(_SLATEOS_USE_MISC) || !defined(_SLATEOS_USE_POSIX2008) || defined(_SLATEOS_USE_XSI2008)
int ffs(int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_STRINGS_H */
