/*
 * SlateOS: what this C library's <complex.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <complex.h>

#ifndef _SLATEOS_COMPLEX_H
#define _SLATEOS_COMPLEX_H

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* The base-10 logarithm: log10|z| + i arg(z) / ln 10. */
double _Complex      clog10(double _Complex);
float _Complex       clog10f(float _Complex);
long double _Complex clog10l(long double _Complex);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_COMPLEX_H */
