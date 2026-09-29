/*
 * SlateOS: what this C library's <grp.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <grp.h>

#ifndef _SLATEOS_GRP_H
#define _SLATEOS_GRP_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* FILE, which musl's header has only for _GNU_SOURCE. */
#define __NEED_FILE
#include <bits/alltypes.h>

/* fgetgrent into the caller's storage. */
int fgetgrent_r(FILE *__restrict, struct group *__restrict, char *__restrict, size_t,
                struct group **__restrict);
#endif

#ifdef _GNU_SOURCE
/* getgrent into the caller's storage. */
int getgrent_r(struct group *__restrict, char *__restrict, size_t, struct group **__restrict);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_GRP_H */
