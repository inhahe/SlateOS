/*
 * The functions SlateOS's libc has and zig's musl lacks, for the CONTROL
 * interpreter only: python-control, the same objects as the image's
 * /bin/python3, linked against musl so that it runs under WSL (stdlib.sh and
 * the fastpy check run it there).
 *
 * CPython is configured against our libc.a (run.sh), so its objects call
 * whatever our libc has. Where musl has not got one of those, the control's
 * link needs it from somewhere; this is that somewhere, and nothing else.
 * Each stand-in is Linux's own call, or the nearest musl has, so the control
 * behaves as the image's interpreter does where it matters to what it is
 * used for -- running the standard library -- and is never linked into
 * anything that ships: slatelink.sh links the image's interpreter against
 * our libc.a alone, which defines all of these itself.
 *
 * A control link that still reports an undefined symbol names a function
 * our libc gained and musl lacks; it belongs here, beside these.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <semaphore.h>
#include <stdio.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

#ifndef SYS_close_range
#define SYS_close_range 436
#endif

/* Linux's own call: musl 1.2.5 has no wrapper for it. */
int close_range(unsigned int first, unsigned int last, int flags)
{
    return (int)syscall(SYS_close_range, first, last, flags);
}

/* sem_timedwait counts against CLOCK_REALTIME; a CLOCK_MONOTONIC deadline is
 * moved onto that clock first. Close enough for a host-run check, which is
 * all this is for; the image's interpreter gets our libc's own. */
int sem_clockwait(sem_t *sem, clockid_t clock, const struct timespec *abstime)
{
    struct timespec now_on, now_rt, at;
    if (clock == CLOCK_REALTIME)
        return sem_timedwait(sem, abstime);
    if (clock != CLOCK_MONOTONIC) {
        errno = EINVAL;
        return -1;
    }
    clock_gettime(CLOCK_MONOTONIC, &now_on);
    clock_gettime(CLOCK_REALTIME, &now_rt);
    at.tv_sec = now_rt.tv_sec + (abstime->tv_sec - now_on.tv_sec);
    at.tv_nsec = now_rt.tv_nsec + (abstime->tv_nsec - now_on.tv_nsec);
    while (at.tv_nsec < 0) {
        at.tv_nsec += 1000000000L;
        at.tv_sec -= 1;
    }
    while (at.tv_nsec >= 1000000000L) {
        at.tv_nsec -= 1000000000L;
        at.tv_sec += 1;
    }
    return sem_timedwait(sem, &at);
}

/* glibc's getwd: getcwd into a PATH_MAX buffer. */
char *getwd(char *buf)
{
    return getcwd(buf, PATH_MAX);
}

/* glibc's tmpnam_r: tmpnam, refusing a NULL buffer. */
char *tmpnam_r(char *s)
{
    return s ? tmpnam(s) : NULL;
}
