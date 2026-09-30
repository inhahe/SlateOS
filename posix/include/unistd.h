/*
 * SlateOS: what this C library's <unistd.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <unistd.h>

#ifndef _SLATEOS_UNISTD_H
#define _SLATEOS_UNISTD_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* Close every descriptor from the argument up. */
void closefrom(int);
/* Set the host identifier gethostid returns. */
int sethostid(long);
#endif

#ifdef _GNU_SOURCE
/* close_range's flags: unshare the descriptor table first, or mark the
 * range close-on-exec instead of closing it. (<linux/close_range.h> defines
 * the same.) */
#ifndef CLOSE_RANGE_UNSHARE
#define CLOSE_RANGE_UNSHARE (1U << 1)
#endif
#ifndef CLOSE_RANGE_CLOEXEC
#define CLOSE_RANGE_CLOEXEC (1U << 2)
#endif
/* Close the descriptors from the first argument to the second. */
int close_range(unsigned int, unsigned int, int);
#endif

#ifdef _SLATEOS_USE_MISC
/* The old BSD and System V calls glibc keeps: getwd a getcwd into
 * PATH_MAX bytes; revoke, setlogin refused (ENOSYS); ttyslot 0; profil
 * refused but when stopping. */
_SLATEOS_DEPRECATED("use getcwd") char *getwd(char *);
int revoke(const char *);
int setlogin(const char *);
int ttyslot(void);
int profil(unsigned short *, size_t, size_t, unsigned int);
#endif

#ifdef _GNU_SOURCE
/* Whether a group is the process's own or a supplementary one. */
int group_member(gid_t);
/* execve of a path relative to a directory's descriptor, or of the
 * descriptor itself (AT_EMPTY_PATH). */
int execveat(int, const char *, char *const[], char *const[], int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_UNISTD_H */
