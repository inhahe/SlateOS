/*
 * SlateOS: what this C library's <uchar.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <uchar.h>

#ifndef _SLATEOS_UCHAR_H
#define _SLATEOS_UCHAR_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* C23's UTF-8 code unit -- a keyword in C++20, whose __cpp_char8_t says so. */
#if defined(_SLATEOS_USE_C23) && !defined(__cpp_char8_t)
typedef unsigned char char8_t;
#endif

/* C23's: a character as UTF-8 code units, one a call, and back. */
#if defined(_SLATEOS_USE_C23) || defined(__cpp_char8_t)
size_t mbrtoc8(char8_t *__restrict, const char *__restrict, size_t, mbstate_t *__restrict);
size_t c8rtomb(char *__restrict, char8_t, mbstate_t *__restrict);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_UCHAR_H */
