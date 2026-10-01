/*
 * SlateOS: what this C library's <semaphore.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <semaphore.h>

#ifndef _SLATEOS_SEMAPHORE_H
#define _SLATEOS_SEMAPHORE_H

#define __NEED_clockid_t
#include <bits/alltypes.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* sem_timedwait against the clock named: CLOCK_REALTIME or CLOCK_MONOTONIC. */
int sem_clockwait(sem_t *__restrict, clockid_t, const struct timespec *__restrict);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SEMAPHORE_H */
