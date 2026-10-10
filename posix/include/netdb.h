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

/* The BSD remote-execution calls. rcmd runs a command through a host's
 * rshd from a reserved port, rexec through its rexecd with a name and
 * password (from ~/.netrc when NULL): each the connection, the stderr
 * channel's in *fd2p when it is not NULL, and *ahost the host's canonical
 * name. ruserok says whether a remote user may log in as a local one, by
 * /etc/hosts.equiv and ~/.rhosts (0 yes, -1 no); iruserok for an address.
 * rresvport binds a socket to a reserved port, from *alport down. The _af
 * forms take an address family. */
int rcmd(char **__restrict, unsigned short int, const char *__restrict, const char *__restrict,
         const char *__restrict, int *__restrict);
int rcmd_af(char **__restrict, unsigned short int, const char *__restrict, const char *__restrict,
            const char *__restrict, int *__restrict, sa_family_t);
int rexec(char **__restrict, int, const char *__restrict, const char *__restrict,
          const char *__restrict, int *__restrict);
int rexec_af(char **__restrict, int, const char *__restrict, const char *__restrict,
             const char *__restrict, int *__restrict, sa_family_t);
int ruserok(const char *, int, const char *, const char *);
int ruserok_af(const char *, int, const char *, const char *, sa_family_t);
int iruserok(uint32_t, int, const char *, const char *);
int iruserok_af(const void *, int, const char *, const char *, sa_family_t);
int rresvport(int *);
int rresvport_af(int *, sa_family_t);
#endif

#ifdef _GNU_SOURCE
/* An asynchronous lookup's control block: the request, and its answer.
 * glibc's <netdb.h> makes struct timespec whole, as musl's types header does
 * here; struct sigevent is <signal.h>'s, whose SIGEV_ constants a caller
 * needs anyway. */
#define __NEED_time_t
#define __NEED_struct_timespec
#include <bits/alltypes.h>
struct sigevent;

struct gaicb {
	const char *ar_name;               /* the name to look up */
	const char *ar_service;            /* the service */
	const struct addrinfo *ar_request; /* the hints, or NULL */
	struct addrinfo *ar_result;        /* the answer, once there is one */
	int __return;                      /* what gai_error answers */
	int __glibc_reserved[5];
};

/* getaddrinfo_a's modes: return once every request has its answer, or at
 * once. */
#define GAI_WAIT 0
#define GAI_NOWAIT 1

/* Look the requests up on the library's threads (0, or an EAI_ code); wait
 * until one of them has its answer (0, EAI_AGAIN at the timeout, EAI_ALLDONE
 * when none is still being looked up); a request's answer, or
 * EAI_INPROGRESS; cancel one (EAI_CANCELED, EAI_NOTCANCELED, EAI_ALLDONE). */
int getaddrinfo_a(int, struct gaicb *[], int, struct sigevent *);
int gai_suspend(const struct gaicb *const[], int, const struct timespec *);
int gai_error(struct gaicb *);
int gai_cancel(struct gaicb *);
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
