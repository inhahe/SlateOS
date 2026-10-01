/*
 * SlateOS: what this C library's <fmtmsg.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <fmtmsg.h>

#ifndef _SLATEOS_FMTMSG_H
#define _SLATEOS_FMTMSG_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* Define, redefine, or with a NULL string remove, a severity level above
 * MM_INFO, as glibc's: the string is kept, not copied. */
int addseverity(int, const char *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_FMTMSG_H */
