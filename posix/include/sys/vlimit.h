/*
 * SlateOS: <sys/vlimit.h> -- 4.2BSD's resource limits, which musl does not
 * have, as glibc declares them. Obsolete: <sys/resource.h>'s setrlimit is
 * the interface; vlimit sets a soft limit through it.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SYS_VLIMIT_H
#define _SYS_VLIMIT_H 1

#include <features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The resources: each one past setrlimit's RLIMIT_ number. */
enum __vlimit_resource
{
	LIM_NORAISE, /* none: raising cannot be forbidden (as in glibc) */
	LIM_CPU,     /* CPU time (seconds) */
	LIM_FSIZE,   /* the largest file (bytes) */
	LIM_DATA,    /* the data segment (bytes) */
	LIM_STACK,   /* the stack (bytes) */
	LIM_CORE,    /* the largest core file (bytes) */
	LIM_MAXRSS   /* the resident set (bytes) */
};

/* No limit. glibc's defines this unconditionally, and so meets <math.h>'s
 * INFINITY in any program that includes both; here the first to be included
 * keeps the name. */
#ifndef INFINITY
#define INFINITY 0x7fffffff
#endif

/* Set RESOURCE's soft limit to VALUE: 0, or -1 and errno. */
int vlimit(enum __vlimit_resource, int);

#ifdef __cplusplus
}
#endif

#endif /* _SYS_VLIMIT_H */
