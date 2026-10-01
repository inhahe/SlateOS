/*
 * SlateOS: what this C library's <regex.h> has that musl's does not
 * define -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <regex.h>

#ifndef _SLATEOS_REGEX_H
#define _SLATEOS_REGEX_H

/* glibc's regexec flag, which BSD's has too: pmatch[0] bounds the string --
 * the search begins at rm_so, the string ends at rm_eo, and a NUL before it
 * is an ordinary byte. Defined whatever the feature macros, as glibc's is. */
#define REG_STARTEND (1 << 2)

/* glibc's name for success, and the error codes it adds to POSIX's:
 * regcomp never returns REG_EEND, and reports an unmatched ')' -- glibc's
 * REG_ERPAREN -- as REG_EPAREN. */
#define REG_NOERROR 0
#define REG_EEND 14
#define REG_ESIZE 15
#define REG_ERPAREN 16

#endif /* _SLATEOS_REGEX_H */
