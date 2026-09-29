/*
 * SlateOS: what this C library's <wchar.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <wchar.h>

#ifndef _SLATEOS_WCHAR_H
#define _SLATEOS_WCHAR_H


#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* wmemcpy, returning the end of what it wrote. */
wchar_t *wmempcpy(wchar_t *__restrict, const wchar_t *__restrict, size_t);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_WCHAR_H */
