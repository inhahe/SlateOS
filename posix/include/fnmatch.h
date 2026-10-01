/*
 * SlateOS: what this C library's <fnmatch.h> has that musl's does not
 * define -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <fnmatch.h>

#ifndef _SLATEOS_FNMATCH_H
#define _SLATEOS_FNMATCH_H

#include <bits/slateos-features.h>

#ifdef _GNU_SOURCE
/* ksh's extended patterns: ?(a|b) *(a|b) +(a|b) @(a|b) !(a|b). musl's
 * header has glibc's other GNU flags, and not this one. */
#define FNM_EXTMATCH (1 << 5)
#endif

#endif /* _SLATEOS_FNMATCH_H */
