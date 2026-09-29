/*
 * SlateOS: what this C library's <utmp.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <utmp.h>

#ifndef _SLATEOS_UTMP_H
#define _SLATEOS_UTMP_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* The reentrant forms of getutent, getutid and getutline. */
int getutent_r(struct utmp *, struct utmp **);
int getutid_r(const struct utmp *, struct utmp *, struct utmp **);
int getutline_r(const struct utmp *, struct utmp *, struct utmp **);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_UTMP_H */
