/*
 * SlateOS: what this C library's <sys/epoll.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/epoll.h>

#ifndef _SLATEOS_SYS_EPOLL_H
#define _SLATEOS_SYS_EPOLL_H


#ifdef __cplusplus
extern "C" {
#endif

struct timespec;

/* epoll_pwait with a timeout to the nanosecond. */
int epoll_pwait2(int, struct epoll_event *, int, const struct timespec *, const sigset_t *);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_EPOLL_H */
