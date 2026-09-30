/*
 * SlateOS: what this C library's <arpa/nameser.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <arpa/nameser.h>

#ifndef _SLATEOS_ARPA_NAMESER_H
#define _SLATEOS_ARPA_NAMESER_H

#ifdef __cplusplus
extern "C" {
#endif

/* Domain names, between text and the wire, compressed and not: glibc's
 * conversions, which musl has only ns_name_uncompress of. */
int ns_name_ntop(const unsigned char *, char *, size_t);
int ns_name_pton(const char *, unsigned char *, size_t);
int ns_name_unpack(const unsigned char *, const unsigned char *,
                   const unsigned char *, unsigned char *, size_t);
int ns_name_pack(const unsigned char *, unsigned char *, int,
                 const unsigned char **, const unsigned char **);
int ns_name_compress(const char *, unsigned char *, size_t,
                     const unsigned char **, const unsigned char **);
int ns_name_skip(const unsigned char **, const unsigned char *);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_ARPA_NAMESER_H */
