/*
 * SlateOS: what this C library's <fcntl.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <fcntl.h>

#ifndef _SLATEOS_FCNTL_H
#define _SLATEOS_FCNTL_H


#ifdef __cplusplus
extern "C" {
#endif

#if defined(_GNU_SOURCE) || defined(_LARGEFILE64_SOURCE)
/* fcntl by the name glibc gives its 64-bit-offset locks; fcntl's are those
 * already. */
int fcntl64(int, int, ...);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_FCNTL_H */
