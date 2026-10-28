/* SPDX-License-Identifier: MIT */
/*
 * slx -- the interface between the driver layer's two halves
 * (design-decisions 1578 rule 3).
 *
 * The Linux-facing half (C, `linuxdrv/emul/`) implements Linux's functions
 * in terms of these; the system-facing half (Rust, `linuxdrv/kit/`)
 * implements these on SlateOS. Nothing here names a Linux type, so the
 * Rust half never needs a Linux header, and this file includes none.
 */
#ifndef SLX_H
#define SLX_H

/* Write `len` bytes of text to the console, as one line or part of one. */
void slx_log(const char *text, unsigned long len);

/*
 * Memory: `size` bytes, aligned to `size` rounded up to a power of two
 * (kmalloc's own promise), or NULL. The contents are not cleared.
 */
void *slx_alloc(unsigned long size);
/* Give back what slx_alloc gave; NULL is ignored. */
void slx_free(const void *ptr);

/* Fill `len` bytes at `buf` from the system's CSPRNG, or stop the driver. */
void slx_random(void *buf, unsigned long len);

/* Nanoseconds since some fixed moment, never going back. */
unsigned long long slx_now_ns(void);

/* The process's arguments: how many, and the n-th (NULL past the last). */
int slx_argc(void);
const char *slx_argv(int n);

/* End the driver process with `code`. */
void slx_exit(int code) __attribute__((noreturn));

/*
 * End it because Linux code reached `what`, a function the layer does not
 * implement, called from `caller`: said on the console -- the caller's
 * address for the driver's symbol table -- then exit code SLX_EXIT_STOP.
 */
void slx_stop(const char *what, const void *caller) __attribute__((noreturn));

/*
 * The exit codes a driver process ends with; each means one thing, so that
 * the kernel can tell them apart (kernel/src/linuxdrv.rs, lxkit::exit).
 */
#define SLX_EXIT_OK 0
#define SLX_EXIT_BUG 121		/* Linux code called BUG() */
#define SLX_EXIT_PANIC 122		/* the layer's Rust half panicked */
#define SLX_EXIT_NO_RANDOM 123		/* the system gave no random bytes */
#define SLX_EXIT_STOP 125		/* a generated stop was reached */
#define SLX_EXIT_INIT_FAILED 126	/* an initcall failed */
#define SLX_EXIT_NO_MEMORY 127		/* the process's memory could not be set up */

/* Implemented by the C half: run the driver; what slx_exit is given. */
int lx_emul_main(void);

#endif /* SLX_H */
