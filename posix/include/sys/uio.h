/*
 * SlateOS: what this C library's <sys/uio.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/uio.h>

#ifndef _SLATEOS_SYS_UIO_H
#define _SLATEOS_SYS_UIO_H

#if defined(_GNU_SOURCE) && defined(_LARGEFILE64_SOURCE)
/* glibc's large-file names for preadv2 and pwritev2, which musl's header
 * declares without them: macros for the standard names, as musl's are for
 * preadv64 and pwritev64. */
#define preadv64v2 preadv2
#define pwritev64v2 pwritev2
#endif

#endif /* _SLATEOS_SYS_UIO_H */
