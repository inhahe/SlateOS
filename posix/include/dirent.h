/*
 * SlateOS: what this C library's <dirent.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <dirent.h>

#ifndef _SLATEOS_DIRENT_H
#define _SLATEOS_DIRENT_H

#define __NEED_ssize_t
#include <bits/alltypes.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* scandir of a directory named relative to a descriptor. */
int scandirat(int, const char *__restrict, struct dirent ***__restrict,
              int (*)(const struct dirent *),
              int (*)(const struct dirent **, const struct dirent **));

/* The Linux system call, whose records are struct linux_dirent64. (Under
 * _LARGEFILE64_SOURCE musl's <dirent.h> makes the name getdents'.) */
#ifndef getdents64
ssize_t getdents64(int, void *, size_t);
#endif
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_DIRENT_H */
