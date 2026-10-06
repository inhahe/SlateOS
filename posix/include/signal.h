/*
 * SlateOS: what this C library's <signal.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <signal.h>

#ifndef _SLATEOS_SIGNAL_H
#define _SLATEOS_SIGNAL_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* signal with System V's semantics: the handler is reset to SIG_DFL when it
 * is called, and a system call it interrupts is not restarted. */
void (*sysv_signal(int, void (*)(int)))(int);

/* Send a signal to thread TID of process TGID (Linux; glibc 2.30). */
int tgkill(pid_t, pid_t, int);

/* Send a signal and a value to a thread of this process (glibc 2.11). The
 * value cannot be carried here: a real send answers ENOSYS. */
int pthread_sigqueue(pthread_t, int, union sigval);
#endif

#ifdef _SLATEOS_USE_MISC
/* 4.2BSD's signal masks: signal n as bit n-1, signals 1 to 32. */
#define sigmask(sig) ((int)(1u << ((sig) - 1)))
_SLATEOS_DEPRECATED("use sigprocmask") int sigblock(int);
_SLATEOS_DEPRECATED("use sigprocmask") int sigsetmask(int);
_SLATEOS_DEPRECATED("use sigprocmask") int siggetmask(void);

/* 4.2BSD's signal stack, refused here (ENOSYS): it has no size. */
struct sigstack {
	void *ss_sp;
	int ss_onstack;
};
_SLATEOS_DEPRECATED("use sigaltstack") int sigstack(struct sigstack *, struct sigstack *);

struct sigcontext;
int sigreturn(struct sigcontext *);

/* System V's raise and signal. */
int gsignal(int);
void (*ssignal(int, void (*)(int)))(int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SIGNAL_H */
