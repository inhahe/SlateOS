/*
 * SlateOS: what this C library's <sys/timex.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/timex.h>

#ifndef _SLATEOS_SYS_TIMEX_H
#define _SLATEOS_SYS_TIMEX_H


#ifdef __cplusplus
extern "C" {
#endif

/* adjtimex by the NTP interface's name. */
int ntp_adjtime(struct timex *);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_TIMEX_H */
