/*
 * SlateOS: what this C library's <shadow.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <shadow.h>

#ifndef _SLATEOS_SHADOW_H
#define _SLATEOS_SHADOW_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* The reentrant forms of fgetspent, getspent and sgetspent. */
int fgetspent_r(FILE *, struct spwd *, char *, size_t, struct spwd **);
int getspent_r(struct spwd *, char *, size_t, struct spwd **);
int sgetspent_r(const char *, struct spwd *, char *, size_t, struct spwd **);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SHADOW_H */
