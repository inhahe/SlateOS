/*
 * SlateOS: what this C library's <aio.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <aio.h>

#ifndef _SLATEOS_AIO_H
#define _SLATEOS_AIO_H

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* How glibc should size its thread pool (aio_init). This library performs
 * each request in the calling thread, and has no pool to size: aio_init
 * accepts any tuning and ignores it. */
struct aioinit {
	int aio_threads;   /* the most threads */
	int aio_num;       /* requests expected at once */
	int aio_locks;     /* unused */
	int aio_usedba;    /* unused */
	int aio_debug;     /* unused */
	int aio_numusers;  /* unused */
	int aio_idle_time; /* seconds an idle thread waits for work */
	int aio_reserved;
};

void aio_init(const struct aioinit *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_AIO_H */
