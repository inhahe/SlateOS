/*
 * SlateOS: <gnu/libc-version.h> -- the version glibc's interfaces are
 * provided at, which musl does not have.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _GNU_LIBC_VERSION_H
#define _GNU_LIBC_VERSION_H 1

#ifdef __cplusplus
extern "C" {
#endif

/* The version of glibc whose interface this library provides, such as
 * "2.38". */
const char *gnu_get_libc_version(void);

/* "stable" or "release". */
const char *gnu_get_libc_release(void);

#ifdef __cplusplus
}
#endif

#endif /* _GNU_LIBC_VERSION_H */
