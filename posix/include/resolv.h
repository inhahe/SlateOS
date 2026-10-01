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

#include <bits/slateos-features.h>
/* FILE, for the message printers below: glibc's header includes it. */
#include <stdio.h>

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

/* libresolv's helpers, each a __ name its public one is #defined to, as
 * glibc's header has them, and deprecated where glibc's are, in glibc's
 * words (putlong's and putshort's swapped as glibc's are). */
#define dn_count_labels __dn_count_labels
#define putlong __putlong
#define putshort __putshort
#define res_close __res_close
#define res_randomid __res_randomid
#define res_isourserver __res_isourserver
#define res_nameinquery __res_nameinquery
#define res_queriesmatch __res_queriesmatch
#define hostalias __hostalias
#define res_hostalias __res_hostalias
int dn_count_labels(const char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
void putlong(uint32_t, unsigned char *) _SLATEOS_DEPRECATED("use NS_PUT16 instead");
void putshort(uint16_t, unsigned char *) _SLATEOS_DEPRECATED("use NS_PUT32 instead");
void res_close(void);
unsigned int res_randomid(void) _SLATEOS_DEPRECATED("use getentropy instead");
int res_isourserver(const struct sockaddr_in *) _SLATEOS_ATTRIBUTE_DEPRECATED;
int res_nameinquery(const char *, int, int, const unsigned char *,
                    const unsigned char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
int res_queriesmatch(const unsigned char *, const unsigned char *,
                     const unsigned char *, const unsigned char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
/* The HOSTALIASES file's alias for a name, which res_search asks for in
 * the name's place when the name has no dot. */
const char *hostalias(const char *) _SLATEOS_DEPRECATED("use getaddrinfo instead");
const char *res_hostalias(const res_state, const char *, char *, size_t)
    _SLATEOS_DEPRECATED("use getaddrinfo instead");

/* The symbol tables and their printers, LOC records' text, and base64:
 * libresolv's res_debug.c and base64.c, renamed and deprecated as glibc's
 * are (base64 not deprecated). */
#define sym_ston __sym_ston
#define sym_ntos __sym_ntos
#define sym_ntop __sym_ntop
#define p_class __p_class
#define p_type __p_type
#define p_rcode __p_rcode
#define p_option __p_option
#define p_time __p_time
#define loc_aton __loc_aton
#define loc_ntoa __loc_ntoa
#define b64_ntop __b64_ntop
#define b64_pton __b64_pton
int sym_ston(const struct res_sym *, const char *, int *) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *sym_ntos(const struct res_sym *, int, int *) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *sym_ntop(const struct res_sym *, int, int *) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *p_class(int) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *p_type(int) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *p_rcode(int) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *p_option(unsigned long) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *p_time(uint32_t) _SLATEOS_ATTRIBUTE_DEPRECATED;
int loc_aton(const char *, unsigned char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
const char *loc_ntoa(const unsigned char *, char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
int b64_ntop(const unsigned char *, size_t, char *, size_t);
int b64_pton(char const *, unsigned char *, size_t);

/* The message and name printers, res_debug.c's rest: a whole message as
 * dig prints one, a name read out of a message, and a state's options --
 * renamed and deprecated as glibc's are. p_query prints to stdout. */
#define fp_nquery __fp_nquery
#define fp_query __fp_query
#define p_query __p_query
#define fp_resstat __fp_resstat
#define p_cdname __p_cdname
#define p_cdnname __p_cdnname
#define p_fqname __p_fqname
#define p_fqnname __p_fqnname
void fp_nquery(const unsigned char *, int, FILE *) _SLATEOS_ATTRIBUTE_DEPRECATED;
void fp_query(const unsigned char *, FILE *) _SLATEOS_ATTRIBUTE_DEPRECATED;
void p_query(const unsigned char *) _SLATEOS_ATTRIBUTE_DEPRECATED;
void fp_resstat(const res_state, FILE *) _SLATEOS_ATTRIBUTE_DEPRECATED;
const unsigned char *p_cdnname(const unsigned char *, const unsigned char *, int,
                               FILE *) _SLATEOS_ATTRIBUTE_DEPRECATED;
const unsigned char *p_cdname(const unsigned char *, const unsigned char *,
                              FILE *) _SLATEOS_ATTRIBUTE_DEPRECATED;
const unsigned char *p_fqnname(const unsigned char *, const unsigned char *, int,
                               char *, int) _SLATEOS_ATTRIBUTE_DEPRECATED;
const unsigned char *p_fqname(const unsigned char *, const unsigned char *,
                              FILE *) _SLATEOS_ATTRIBUTE_DEPRECATED;

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_RESOLV_H */
