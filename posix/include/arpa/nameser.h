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

#include <bits/slateos-features.h>

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

/* Labels lowered, and the compression table cut back to a point. */
int ns_name_ntol(const unsigned char *, unsigned char *, size_t);
void ns_name_rollback(const unsigned char *, const unsigned char **,
                      const unsigned char **);

/* A parsed message's header flag, as the function glibc declares -- for a
 * program that takes its address; musl's macro of the name serves a call.
 * The name is in parentheses so that the macro does not expand it. */
int (ns_msg_getflag)(ns_msg, int);

/* BIND's TTLs, dates, and names compared: deprecated by glibc, as here. */
int ns_format_ttl(unsigned long, char *, size_t) _SLATEOS_ATTRIBUTE_DEPRECATED;
int ns_parse_ttl(const char *, unsigned long *) _SLATEOS_ATTRIBUTE_DEPRECATED;
uint32_t ns_datetosecs(const char *, int *) _SLATEOS_ATTRIBUTE_DEPRECATED;
int ns_samedomain(const char *, const char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
int ns_subdomain(const char *, const char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
int ns_makecanon(const char *, char *, size_t) _SLATEOS_ATTRIBUTE_DEPRECATED;
int ns_samename(const char *, const char *) _SLATEOS_ATTRIBUTE_DEPRECATED;

/* A record as zone-file text, from a parsed message or from its fields:
 * deprecated by glibc, as here. */
int ns_sprintrr(const ns_msg *, const ns_rr *, const char *, const char *,
                char *, size_t) _SLATEOS_ATTRIBUTE_DEPRECATED;
int ns_sprintrrf(const unsigned char *, size_t, const char *, ns_class,
                 ns_type, unsigned long, const unsigned char *, size_t,
                 const char *, const char *, char *, size_t)
    _SLATEOS_ATTRIBUTE_DEPRECATED;

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_ARPA_NAMESER_H */
