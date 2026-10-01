/*
 * SlateOS: what this C library's <fenv.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <fenv.h>

#ifndef _SLATEOS_FENV_H
#define _SLATEOS_FENV_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* The default environment with every exception unmasked, so each traps. */
#define FE_NOMASK_ENV ((const fenv_t *) -2)

/* Unmask (or mask again) the exceptions in the argument, in both units;
 * each returns the ones that were unmasked before, or -1. */
int feenableexcept(int);
int fedisableexcept(int);
int fegetexcept(void);
#endif

#ifdef _SLATEOS_USE_BFP_EXT_C23
/* The control modes -- the rounding direction, the exception masks, the
 * x87 unit's precision -- without the exception flags: the x87 control word
 * and MXCSR, in glibc's x86-64 layout. */
typedef struct {
	unsigned short __control_word;
	unsigned short __reserved;
	unsigned int __mxcsr;
} femode_t;

/* The modes a program starts with. */
#define FE_DFL_MODE ((const femode_t *) -1L)

int fegetmode(femode_t *);
int fesetmode(const femode_t *);
int fesetexcept(int);
int fetestexceptflag(const fexcept_t *, int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_FENV_H */
