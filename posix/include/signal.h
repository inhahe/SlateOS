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


#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* signal with System V's semantics: the handler is reset to SIG_DFL when it
 * is called, and a system call it interrupts is not restarted. */
void (*sysv_signal(int, void (*)(int)))(int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SIGNAL_H */
