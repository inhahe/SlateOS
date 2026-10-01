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

#if defined(_SLATEOS_USE_POSIX2008) || (defined(_XOPEN_SOURCE) && _XOPEN_SOURCE + 0 >= 500) 	|| (defined(_POSIX_C_SOURCE) && _POSIX_C_SOURCE + 0 >= 200112L)
/* Whom a read-write lock prefers (pthread_rwlockattr_setkind_np): readers,
 * the default; writers, which glibc and this library take to mean readers,
 * since a reader locking again while a writer waits would wait for itself;
 * and writers, readers never locking again -- while a writer waits, no new
 * reader joins the readers holding the lock. */
enum {
	PTHREAD_RWLOCK_PREFER_READER_NP,
	PTHREAD_RWLOCK_PREFER_WRITER_NP,
	PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP,
	PTHREAD_RWLOCK_DEFAULT_NP = PTHREAD_RWLOCK_PREFER_READER_NP
};

int pthread_rwlockattr_getkind_np(const pthread_rwlockattr_t *__restrict, int *__restrict);
int pthread_rwlockattr_setkind_np(pthread_rwlockattr_t *, int);
#endif

/* The stack by its top, the address it grows down from, as the obsolete
 * interface took it (pthread_attr_setstack takes its lowest address). */
int pthread_attr_getstackaddr(const pthread_attr_t *__restrict, void **__restrict)
	_SLATEOS_DEPRECATED("pthread_attr_getstackaddr is deprecated: use pthread_attr_getstack");
int pthread_attr_setstackaddr(pthread_attr_t *, void *)
	_SLATEOS_DEPRECATED("pthread_attr_setstackaddr is deprecated: use pthread_attr_setstack");

#ifdef _GNU_SOURCE
#define PTHREAD_MUTEX_FAST_NP PTHREAD_MUTEX_TIMED_NP

/* A writer-preferring read-write lock, statically: the fourth int of the
 * lock is its preference (posix/src/pthread.rs, PthreadRwlockT). */
#define PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP {{{0, 0, 0, 1}}}

/* The CPUs threads created with the attributes may run on: every one, here,
 * and pthread_create refuses a set of fewer. */
int pthread_attr_setaffinity_np(pthread_attr_t *, size_t, const cpu_set_t *);
int pthread_attr_getaffinity_np(const pthread_attr_t *, size_t, cpu_set_t *);

/* pthread_timedjoin_np against CLOCK_REALTIME or CLOCK_MONOTONIC. */
int pthread_clockjoin_np(pthread_t, void **, clockid_t, const struct timespec *);

/* sched_yield, by the name glibc gave it before it deprecated it -- the
 * same function, as glibc's header makes it. */
int pthread_yield(void) __asm__("sched_yield")
	_SLATEOS_DEPRECATED("pthread_yield is deprecated: use sched_yield");

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
