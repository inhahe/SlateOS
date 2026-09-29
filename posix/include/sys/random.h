/*
 * SlateOS: what this C library's <sys/random.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/random.h>

#ifndef _SLATEOS_SYS_RANDOM_H
#define _SLATEOS_SYS_RANDOM_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Up to 256 bytes from the kernel's generator: glibc declares it here as in
 * <unistd.h>; musl only there. */
int getentropy(void *, size_t);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_RANDOM_H */
