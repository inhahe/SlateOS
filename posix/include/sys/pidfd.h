/*
 * SlateOS: <sys/pidfd.h> -- process descriptors, which musl does not have.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _PIDFD_H
#define _PIDFD_H 1

#include <fcntl.h>
#include <signal.h>

#ifdef __cplusplus
extern "C" {
#endif

/* pidfd_open's flag: a descriptor on which waiting does not block. */
#define PIDFD_NONBLOCK O_NONBLOCK

/* A descriptor for the process, close-on-exec. The flags are
 * PIDFD_NONBLOCK or 0. */
int pidfd_open(pid_t, unsigned int);

/* A duplicate, in the caller, of the second argument's descriptor in the
 * process. The flags must be 0. */
int pidfd_getfd(int, int, unsigned int);

/* The pid of the process the descriptor refers to (glibc 2.39). */
pid_t pidfd_getpid(int);

/* Send the process the signal, with the siginfo_t if it is not NULL. The
 * flags must be 0. (musl's <signal.h> has siginfo_t only where it has the
 * rest of POSIX: in a strict ISO C compilation there is none to take.) */
#if defined(_POSIX_SOURCE) || defined(_POSIX_C_SOURCE) || defined(_XOPEN_SOURCE) \
    || defined(_GNU_SOURCE) || defined(_BSD_SOURCE)
int pidfd_send_signal(int, int, siginfo_t *, unsigned int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _PIDFD_H */
