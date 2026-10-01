/*
 * SlateOS: what this C library's <utmpx.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <utmpx.h>

#ifndef _SLATEOS_UTMPX_H
#define _SLATEOS_UTMPX_H

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* Copy an entry between struct utmpx and struct utmp -- in musl's headers
 * one structure (<utmp.h> defines utmp as utmpx), which is why glibc's
 * `struct utmp *` is `struct utmpx *` here. */
void getutmp(const struct utmpx *, struct utmpx *);
void getutmpx(const struct utmpx *, struct utmpx *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_UTMPX_H */
