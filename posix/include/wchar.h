/*
 * SlateOS: what this C library's <wchar.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <wchar.h>

#ifndef _SLATEOS_WCHAR_H
#define _SLATEOS_WCHAR_H

#include <bits/slateos-features.h>


#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* wmemcpy, returning the end of what it wrote. */
wchar_t *wmempcpy(wchar_t *__restrict, const wchar_t *__restrict, size_t);
#endif

#ifdef _GNU_SOURCE
/* wcschr, but the terminating NUL rather than NULL where it finds nothing. */
wchar_t *wcschrnul(const wchar_t *, wchar_t);

/* The conversions in a locale, which is always C's here. */
long               wcstol_l(const wchar_t *__restrict, wchar_t **__restrict, int, locale_t);
unsigned long      wcstoul_l(const wchar_t *__restrict, wchar_t **__restrict, int, locale_t);
long long          wcstoll_l(const wchar_t *__restrict, wchar_t **__restrict, int, locale_t);
unsigned long long wcstoull_l(const wchar_t *__restrict, wchar_t **__restrict, int, locale_t);
double             wcstod_l(const wchar_t *__restrict, wchar_t **__restrict, locale_t);
float              wcstof_l(const wchar_t *__restrict, wchar_t **__restrict, locale_t);
long double        wcstold_l(const wchar_t *__restrict, wchar_t **__restrict, locale_t);

/* 4.4BSD's names for wcstoll and wcstoull. */
long long          wcstoq(const wchar_t *__restrict, wchar_t **__restrict, int);
unsigned long long wcstouq(const wchar_t *__restrict, wchar_t **__restrict, int);
#endif

#ifdef _SLATEOS_USE_MISC
/* strlcpy and strlcat for wide strings: what fits, and a NUL; the length
 * the whole would have had. */
size_t wcslcpy(wchar_t *__restrict, const wchar_t *__restrict, size_t);
size_t wcslcat(wchar_t *__restrict, const wchar_t *__restrict, size_t);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_WCHAR_H */
