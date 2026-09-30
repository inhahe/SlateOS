/*
 * SlateOS: what this C library's <sched.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sched.h>

#ifndef _SLATEOS_SCHED_H
#define _SLATEOS_SCHED_H


#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* The CPU and NUMA node the calling thread is running on. */
int getcpu(unsigned int *, unsigned int *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SCHED_H */
