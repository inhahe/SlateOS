/*
 * SlateOS: what this C library's <arpa/inet.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <arpa/inet.h>

#ifndef _SLATEOS_ARPA_INET_H
#define _SLATEOS_ARPA_INET_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* Network numbers (10, 192.168.1/24, 0x0a0b) and NSAP addresses, BIND's:
 * glibc's libresolv has them, and libc.a here, which -lresolv names too. */
char *inet_neta(in_addr_t, char *, size_t) _SLATEOS_DEPRECATED("Use inet_ntop instead");
char *inet_net_ntop(int, const void *, int, char *, size_t);
int inet_net_pton(int, const char *, void *, size_t);
unsigned int inet_nsap_addr(const char *, unsigned char *, int);
char *inet_nsap_ntoa(int, const unsigned char *, char *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_ARPA_INET_H */
