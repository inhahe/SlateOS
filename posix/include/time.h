/*
 * SlateOS: what this C library's <time.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <time.h>

#ifndef _SLATEOS_TIME_H
#define _SLATEOS_TIME_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* mktime by BSD's other name. */
time_t timelocal(struct tm *);
#endif

#ifdef _GNU_SOURCE
/* getdate into the caller's struct tm, returning what getdate_err would
 * hold. */
int getdate_r(const char *__restrict, struct tm *__restrict);
#endif

/* C23 made these ISO C; musl's header has them only for POSIX, and timegm
 * for BSD. */
#ifdef _SLATEOS_USE_C23
struct tm *gmtime_r(const time_t *__restrict, struct tm *__restrict);
struct tm *localtime_r(const time_t *__restrict, struct tm *__restrict);
time_t timegm(struct tm *);

/* C23's: the resolution of a time base -- TIME_UTC, the only one. */
int timespec_getres(struct timespec *, int);
#endif

#ifdef _GNU_SOURCE
struct timex;
/* adjtimex on a clock: glibc declares it here, musl in <sys/timex.h>. */
int clock_adjtime(clockid_t, struct timex *);
#endif

#ifdef _GNU_SOURCE
/* strptime in a locale, which is always C's here. */
char *strptime_l(const char *__restrict, const char *__restrict, struct tm *, locale_t);
#endif

#ifdef _SLATEOS_USE_MISC
/* The days in a year. */
int dysize(int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_TIME_H */
