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

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_TIME_H */
