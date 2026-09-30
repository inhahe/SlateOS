/*
 * SlateOS: what this C library's <string.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <string.h>

#ifndef _SLATEOS_STRING_H
#define _SLATEOS_STRING_H

#include <bits/slateos-features.h>


#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* memchr for a byte known to be there: no length. */
void *rawmemchr(const void *, int);
#endif

/* C23 made these ISO C (and TR 24731-2 had strdup and strndup before it);
 * musl's header has them only for POSIX. */
#if defined(_SLATEOS_USE_C23) || (defined(__STDC_WANT_LIB_EXT2__) && __STDC_WANT_LIB_EXT2__ + 0 == 1)
char *strdup(const char *);
char *strndup(const char *, size_t);
#endif
#ifdef _SLATEOS_USE_C23
void *memccpy(void *__restrict, const void *__restrict, int, size_t);
#endif

/* glibc declares these by default; musl's header only for _GNU_SOURCE. */
#ifdef _SLATEOS_USE_MISC
char *strchrnul(const char *, int);
char *strcasestr(const char *, const char *);
void *mempcpy(void *, const void *, size_t);
#endif

#ifdef _GNU_SOURCE
/* The name of the E* constant an error number is ("EINVAL"), and strerror's
 * text for it; NULL for a number that is no error's. */
const char *strerrorname_np(int);
const char *strerrordesc_np(int);
/* The name of a signal without its SIG ("INT"), and strsignal's text for
 * it; NULL for 0, a real-time signal and a number that is no signal's. */
const char *sigabbrev_np(int);
const char *sigdescr_np(int);
/* The bytes exclusive-ored with 42, in place; and a string's bytes in a
 * random order. */
void *memfrob(void *, size_t);
char *strfry(char *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_STRING_H */
