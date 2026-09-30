/*
 * SlateOS: what this C library's <sys/socket.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/socket.h>

#ifndef _SLATEOS_SYS_SOCKET_H
#define _SLATEOS_SYS_SOCKET_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* Whether a descriptor's file is of a type (S_IFSOCK, S_IFREG ...). */
int isfdtype(int, int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_SOCKET_H */
