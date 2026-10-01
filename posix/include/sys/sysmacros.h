/*
 * SlateOS: what this C library's <sys/sysmacros.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/sysmacros.h>

#ifndef _SLATEOS_SYS_SYSMACROS_H
#define _SLATEOS_SYS_SYSMACROS_H

#define __NEED_dev_t
#include <bits/alltypes.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The functions behind major, minor and makedev. */
unsigned int gnu_dev_major(dev_t);
unsigned int gnu_dev_minor(dev_t);
dev_t        gnu_dev_makedev(unsigned int, unsigned int);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_SYSMACROS_H */
