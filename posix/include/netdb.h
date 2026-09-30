/*
 * SlateOS: what this C library's <netdb.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <netdb.h>

#ifndef _SLATEOS_NETDB_H
#define _SLATEOS_NETDB_H

#include <bits/slateos-features.h>

#ifdef _SLATEOS_USE_MISC
/* The RPC program database, which glibc's <netdb.h> includes for these
 * extensions. */
#include <rpc/netdb.h>
#endif

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* The reentrant forms of the database iterators, and of the lookups musl
 * has without them, into the caller's storage. */
int gethostent_r(struct hostent *__restrict, char *__restrict, size_t, struct hostent **__restrict,
                 int *__restrict);
int getnetent_r(struct netent *__restrict, char *__restrict, size_t, struct netent **__restrict,
                int *__restrict);
int getnetbyaddr_r(uint32_t, int, struct netent *__restrict, char *__restrict, size_t,
                   struct netent **__restrict, int *__restrict);
int getnetbyname_r(const char *__restrict, struct netent *__restrict, char *__restrict, size_t,
                   struct netent **__restrict, int *__restrict);
int getprotoent_r(struct protoent *__restrict, char *__restrict, size_t,
                  struct protoent **__restrict);
int getprotobyname_r(const char *__restrict, struct protoent *__restrict, char *__restrict, size_t,
                     struct protoent **__restrict);
int getprotobynumber_r(int, struct protoent *__restrict, char *__restrict, size_t,
                       struct protoent **__restrict);
int getservent_r(struct servent *__restrict, char *__restrict, size_t, struct servent **__restrict);

/* Netgroups, /etc/netgroup: start enumerating a group's (host,user,domain)
 * triples (1, or 0 for no such group); end it; the next triple, its fields
 * NULL for any value (1, or 0 at the end -- errno ERANGE when the buffer is
 * too small); whether a group has a triple for these (NULL: any). */
int setnetgrent(const char *);
void endnetgrent(void);
int getnetgrent(char **__restrict, char **__restrict, char **__restrict);
int getnetgrent_r(char **__restrict, char **__restrict, char **__restrict, char *__restrict,
                  size_t);
int innetgr(const char *, const char *, const char *, const char *);
#endif

/* The obsolete lookups, which POSIX.1-2008 dropped: glibc declares them
 * whatever the feature macros ask for, musl's header not for POSIX.1-2008
 * alone or strict ISO C. */
struct hostent *gethostbyname(const char *);
struct hostent *gethostbyaddr(const void *, socklen_t, int);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_NETDB_H */
