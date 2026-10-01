/*
 * SlateOS: <mcheck.h> -- glibc's heap consistency checking and allocation
 * tracing, which musl does not have.
 *
 * Answered as glibc's libc answers them without libc_malloc_debug.so, which
 * a statically linked program -- every program here -- cannot preload:
 * checking refused, tracing not done (posix/src/mcheck.rs).
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _MCHECK_H
#define _MCHECK_H 1

#include <features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* What a check found. */
enum mcheck_status {
	MCHECK_DISABLED = -1, /* checking is not on */
	MCHECK_OK,            /* the block is fine */
	MCHECK_FREE,          /* the block was freed twice */
	MCHECK_HEAD,          /* the memory before it was written over */
	MCHECK_TAIL           /* the memory after it was written over */
};

/* Turn checking on, before the first malloc (0, or -1): the function is
 * called with what a failed check found, or for NULL a message is printed
 * and the program aborts. Here, -1: there is no checking to turn on. */
int mcheck(void (*)(enum mcheck_status));
/* The same, every block checked on every call: -1. */
int mcheck_pedantic(void (*)(enum mcheck_status));
/* Check every block now. */
void mcheck_check_all(void);
/* A block's state: MCHECK_DISABLED. */
enum mcheck_status mprobe(void *);

/* Trace allocations to the file MALLOC_TRACE names, and stop: here, nothing
 * is traced. */
void mtrace(void);
void muntrace(void);

#ifdef __cplusplus
}
#endif

#endif /* _MCHECK_H */
