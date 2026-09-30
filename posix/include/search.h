/*
 * SlateOS: what this C library's <search.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <search.h>

#ifndef _SLATEOS_SEARCH_H
#define _SLATEOS_SEARCH_H


#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* twalk, passing the action an argument. */
void twalk_r(const void *, void (*)(const void *, VISIT, void *), void *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SEARCH_H */
