/*
 * SlateOS: what this C library's <stdio.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <stdio.h>

#ifndef _SLATEOS_STDIO_H
#define _SLATEOS_STDIO_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* tmpnam into the caller's L_tmpnam bytes; NULL is refused, not a static
 * buffer. */
char *tmpnam_r(char *);
#endif

#ifdef _GNU_SOURCE
/* Close every stream. */
int fcloseall(void);

/* renameat, with flags: fail if the new name exists, swap the two names, or
 * leave a whiteout at the old one. */
#define RENAME_NOREPLACE (1 << 0)
#define RENAME_EXCHANGE (1 << 1)
#define RENAME_WHITEOUT (1 << 2)
int renameat2(int, const char *, int, const char *, unsigned int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_STDIO_H */
