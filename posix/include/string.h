/*
 * SlateOS: what this C library's <string.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <string.h>

#ifndef _SLATEOS_STRING_H
#define _SLATEOS_STRING_H


#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* memchr for a byte known to be there: no length. */
void *rawmemchr(const void *, int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_STRING_H */
