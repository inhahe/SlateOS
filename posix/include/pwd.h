/*
 * SlateOS: what this C library's <pwd.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <pwd.h>

#ifndef _SLATEOS_PWD_H
#define _SLATEOS_PWD_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* FILE, which musl's header has only for _GNU_SOURCE. */
#define __NEED_FILE
#include <bits/alltypes.h>

/* The reentrant forms of fgetpwent and getpwent, into the caller's storage. */
int fgetpwent_r(FILE *__restrict, struct passwd *__restrict, char *__restrict, size_t,
                struct passwd **__restrict);
int getpwent_r(struct passwd *__restrict, char *__restrict, size_t, struct passwd **__restrict);

/* glibc declares these by default; musl's header only for _GNU_SOURCE. */
struct passwd *fgetpwent(FILE *);
int putpwent(const struct passwd *, FILE *);
#endif

#ifdef _GNU_SOURCE
/* A user's /etc/passwd line, into a buffer with room for it. */
int getpw(uid_t, char *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_PWD_H */
