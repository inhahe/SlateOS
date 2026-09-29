/*
 * SlateOS: glibc's feature-test conditions, in musl's terms, for the headers
 * in this directory. Internal to them; a program includes none of it.
 *
 * Each of those headers includes musl's own first, so <features.h> has
 * already settled the macros: musl defines _BSD_SOURCE (and _XOPEN_SOURCE
 * 700) when a program asks for nothing, as glibc turns on __USE_MISC then,
 * and _DEFAULT_SOURCE means _BSD_SOURCE to it.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_FEATURES_H
#define _SLATEOS_FEATURES_H

#include <features.h>

/* glibc's __USE_MISC: the BSD and System V extensions, on by default. */
#if defined(_GNU_SOURCE) || defined(_BSD_SOURCE)
#define _SLATEOS_USE_MISC 1
#endif

/* __GLIBC_USE (ISOC2X): C23. */
#if defined(_GNU_SOURCE) || defined(_ISOC2X_SOURCE) || defined(_ISOC23_SOURCE) \
    || (defined(__STDC_VERSION__) && __STDC_VERSION__ > 201710L)
#define _SLATEOS_USE_C23 1
#endif

/* __GLIBC_USE (IEC_60559_BFP_EXT): ISO/IEC TS 18661-1, binary floating
 * point, as the TS has it. */
#if defined(_GNU_SOURCE) || defined(__STDC_WANT_IEC_60559_BFP_EXT__)
#define _SLATEOS_USE_BFP_EXT 1
#endif

/* __GLIBC_USE (IEC_60559_BFP_EXT_C2X): what C23 took from TS 18661-1,
 * available under either. */
#if defined(_SLATEOS_USE_BFP_EXT) || defined(_SLATEOS_USE_C23)
#define _SLATEOS_USE_BFP_EXT_C23 1
#endif

/* __GLIBC_USE (IEC_60559_EXT): the TS 18661-1 functions C23 kept only in
 * its Annex F, behind __STDC_WANT_IEC_60559_EXT__. */
#if defined(_SLATEOS_USE_BFP_EXT) || defined(__STDC_WANT_IEC_60559_EXT__)
#define _SLATEOS_USE_IEC_60559_EXT 1
#endif

/* glibc's __USE_XOPEN2K8: POSIX.1-2008, which glibc's default includes (as
 * musl's default _BSD_SOURCE stands for here). */
#if defined(_SLATEOS_USE_MISC) || (defined(_POSIX_C_SOURCE) && _POSIX_C_SOURCE + 0 >= 200809L) \
    || (defined(_XOPEN_SOURCE) && _XOPEN_SOURCE + 0 >= 700)
#define _SLATEOS_USE_POSIX2008 1
#endif

/* __USE_XOPEN2K8XSI: its X/Open System Interfaces, which _GNU_SOURCE asks for
 * in glibc. */
#if defined(_GNU_SOURCE) || (defined(_XOPEN_SOURCE) && _XOPEN_SOURCE + 0 >= 700)
#define _SLATEOS_USE_XSI2008 1
#endif

/* The attributes glibc's declarations carry that change what a compiler says
 * about a call: a deprecation, and a printf format to check the arguments
 * against. */
#if defined(__GNUC__) || defined(__clang__)
#define _SLATEOS_DEPRECATED(msg) __attribute__((__deprecated__(msg)))
#define _SLATEOS_PRINTF(fmt, first) __attribute__((__format__(__printf__, fmt, first)))
#else
#define _SLATEOS_DEPRECATED(msg)
#define _SLATEOS_PRINTF(fmt, first)
#endif

#endif /* _SLATEOS_FEATURES_H */
