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

/* The resolver on a state of the caller's own (glibc's "things involving a
 * resolver context"). res_ninit and res_nclose are called __res_ninit and
 * __res_nclose in the library, as glibc's header renames them. A state
 * res_ninit has not filled is used as it is: it has no nameserver. */
#define res_ninit __res_ninit
#define res_nclose __res_nclose
int res_ninit(res_state);
int res_nquery(res_state, const char *, int, int, unsigned char *, int);
int res_nsearch(res_state, const char *, int, int, unsigned char *, int);
int res_nquerydomain(res_state, const char *, const char *, int, int,
                     unsigned char *, int);
int res_nmkquery(res_state, int, const char *, int, int,
                 const unsigned char *, int, const unsigned char *,
                 unsigned char *, int);
int res_nsend(res_state, const unsigned char *, int, unsigned char *, int);
void res_nclose(res_state);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_RESOLV_H */
