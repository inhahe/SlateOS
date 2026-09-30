/*
 * SlateOS: what this C library's <resolv.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <resolv.h>

#ifndef _SLATEOS_RESOLV_H
#define _SLATEOS_RESOLV_H

#ifdef __cplusplus
extern "C" {
#endif

/* Whether a name is a host name; may own a host's records (a wildcard's `*`
 * first); is a mailbox as DNS writes one; is a domain name at all. 1 or 0,
 * as glibc answers. */
int res_hnok(const char *);
int res_ownok(const char *);
int res_mailok(const char *);
int res_dnok(const char *);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_RESOLV_H */
