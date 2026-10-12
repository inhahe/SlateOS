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
#define __NEED_off_t
#include <bits/alltypes.h>

#include <bits/slateos-features.h>

/* Under _LARGEFILE64_SOURCE -- which _GNU_SOURCE turns on, as glibc's does
 * (<features.h> here) -- musl's <dirent.h> makes getdents64 a macro for its
 * getdents, `int (int, struct dirent *, size_t)`. glibc's getdents64 is a
 * function of its own, `ssize_t (int, void *, size_t)`, declared under
 * _GNU_SOURCE alone, and it is the one this library defines; the macro also
 * named a function nothing declares when _LARGEFILE64_SOURCE came without
 * _GNU_SOURCE. So the macro goes, and glibc's declaration stands below. */
#undef getdents64

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* scandir of a directory named relative to a descriptor. */
int scandirat(int, const char *__restrict, struct dirent ***__restrict,
              int (*)(const struct dirent *),
              int (*)(const struct dirent **, const struct dirent **));

/* The Linux system call, whose records are struct linux_dirent64. */
ssize_t getdents64(int, void *, size_t);
#endif

#ifdef _SLATEOS_USE_MISC
/* getdents64's records, and the position they were read from. */
ssize_t getdirentries(int, char *__restrict, size_t, off_t *__restrict);
#endif

#if defined(_LARGEFILE64_SOURCE)
/* glibc's large-file names for the two above, as musl's <dirent.h> gives
 * the rest of them: macros for the standard names. */
#define scandirat64 scandirat
#define getdirentries64 getdirentries
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_DIRENT_H */
