/*
 * SlateOS: what this C library's <pthread.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <pthread.h>

#ifndef _SLATEOS_PTHREAD_H
#define _SLATEOS_PTHREAD_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* glibc's names for the mutex types and robustness, from before POSIX had
 * its own. (PTHREAD_MUTEX_ADAPTIVE_NP, glibc's spinning mutex, is not one
 * this library has.) */
#define PTHREAD_MUTEX_TIMED_NP PTHREAD_MUTEX_NORMAL
#define PTHREAD_MUTEX_RECURSIVE_NP PTHREAD_MUTEX_RECURSIVE
#define PTHREAD_MUTEX_ERRORCHECK_NP PTHREAD_MUTEX_ERRORCHECK
#define PTHREAD_MUTEX_STALLED_NP PTHREAD_MUTEX_STALLED
#define PTHREAD_MUTEX_ROBUST_NP PTHREAD_MUTEX_ROBUST

#ifdef _GNU_SOURCE
#define PTHREAD_MUTEX_FAST_NP PTHREAD_MUTEX_TIMED_NP

/* The timed waits, against the clock named: CLOCK_REALTIME or
 * CLOCK_MONOTONIC. */
int pthread_cond_clockwait(pthread_cond_t *__restrict, pthread_mutex_t *__restrict, clockid_t,
                           const struct timespec *__restrict);
int pthread_mutex_clocklock(pthread_mutex_t *__restrict, clockid_t, const struct timespec *__restrict);
int pthread_rwlock_clockrdlock(pthread_rwlock_t *__restrict, clockid_t,
                               const struct timespec *__restrict);
int pthread_rwlock_clockwrlock(pthread_rwlock_t *__restrict, clockid_t,
                               const struct timespec *__restrict);

/* The robust-mutex calls by their names from before POSIX adopted them. */
int pthread_mutex_consistent_np(pthread_mutex_t *)
	_SLATEOS_DEPRECATED("pthread_mutex_consistent_np is deprecated: use pthread_mutex_consistent");
int pthread_mutexattr_getrobust_np(pthread_mutexattr_t *, int *)
	_SLATEOS_DEPRECATED("pthread_mutexattr_getrobust_np is deprecated: use pthread_mutexattr_getrobust");
int pthread_mutexattr_setrobust_np(pthread_mutexattr_t *, int)
	_SLATEOS_DEPRECATED("pthread_mutexattr_setrobust_np is deprecated: use pthread_mutexattr_setrobust");
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_PTHREAD_H */
