/*
 * SlateOS: <glob.h> -- in place of musl's, whose glob_t hides the fields
 * glibc's names.
 *
 * glob_t is musl's layout and glibc's, 72 bytes. musl calls the words after
 * the three POSIX fields __dummy1 and __dummy2[5]; glibc, gl_flags and the
 * five GLOB_ALTDIRFUNC functions, which this library's glob
 * (posix/src/glob.rs) calls -- so a program that sets gl_opendir must see
 * them by name, and this header cannot be musl's with more after it: the
 * struct is declared once. The rest is musl's header: its flags and error
 * codes, glibc's further flags beside them, glob_pattern_p and GLOB_ABEND
 * for _GNU_SOURCE as glibc has them, and musl's glob64 names for
 * _LARGEFILE64_SOURCE.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _GLOB_H
#define _GLOB_H

#ifdef __cplusplus
extern "C" {
#endif

#include <features.h>

#define __NEED_size_t

#include <bits/alltypes.h>

#ifdef _GNU_SOURCE
struct dirent;
struct stat;
#endif

typedef struct {
	size_t gl_pathc; /* the names matched */
	char **gl_pathv; /* gl_offs NULLs, the names, and a NULL */
	size_t gl_offs;  /* the NULLs GLOB_DOOFFS reserves */
	int gl_flags;    /* glob's flags, and GLOB_MAGCHAR */
	/* GLOB_ALTDIRFUNC's functions, called for the C library's own. */
	void (*gl_closedir)(void *);
#ifdef _GNU_SOURCE
	struct dirent *(*gl_readdir)(void *);
#else
	void *(*gl_readdir)(void *);
#endif
	void *(*gl_opendir)(const char *);
#ifdef _GNU_SOURCE
	int (*gl_lstat)(const char *__restrict, struct stat *__restrict);
	int (*gl_stat)(const char *__restrict, struct stat *__restrict);
#else
	int (*gl_lstat)(const char *__restrict, void *__restrict);
	int (*gl_stat)(const char *__restrict, void *__restrict);
#endif
} glob_t;

int  glob(const char *__restrict, int, int (*)(const char *, int), glob_t *__restrict);
void globfree(glob_t *);

#define GLOB_ERR      0x01
#define GLOB_MARK     0x02
#define GLOB_NOSORT   0x04
#define GLOB_DOOFFS   0x08
#define GLOB_NOCHECK  0x10
#define GLOB_APPEND   0x20
#define GLOB_NOESCAPE 0x40
#define GLOB_PERIOD   0x80

/* glibc's: set in gl_flags when the pattern's wildcards were matched; the
 * GLOB_ALTDIRFUNC functions; {a,b}; the pattern itself when it has no
 * wildcard; directories alone. */
#define GLOB_MAGCHAR    0x100
#define GLOB_ALTDIRFUNC 0x200
#define GLOB_BRACE      0x400
#define GLOB_NOMAGIC    0x800
#define GLOB_ONLYDIR    0x2000

#define GLOB_TILDE       0x1000
#define GLOB_TILDE_CHECK 0x4000

#define GLOB_NOSPACE 1
#define GLOB_ABORTED 2
#define GLOB_NOMATCH 3
#define GLOB_NOSYS   4

#ifdef _GNU_SOURCE
/* glibc's old name for GLOB_ABORTED. */
#define GLOB_ABEND GLOB_ABORTED
/* Does the pattern hold a wildcard -- unescaped, if `quote`? */
int glob_pattern_p(const char *, int);
#endif

#if defined(_LARGEFILE64_SOURCE)
#define glob64 glob
#define globfree64 globfree
#define glob64_t glob_t
#endif

#ifdef __cplusplus
}
#endif

#endif
