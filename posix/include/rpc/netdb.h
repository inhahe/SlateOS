/*
 * SlateOS: <rpc/netdb.h> -- the ONC RPC program database, /etc/rpc, which
 * musl does not have.
 *
 * struct rpcent is glibc's, field for field; the functions read the file as
 * glibc's nss_files reads it (posix/src/netdb.rs), and answer from a
 * built-in copy when there is none. glibc's <netdb.h> includes this header
 * for its BSD and System V extensions, and so does this library's.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _RPC_NETDB_H
#define _RPC_NETDB_H 1

#include <features.h>

#define __NEED_size_t
#include <bits/alltypes.h>

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* An RPC program's entry. */
struct rpcent {
	char *r_name;     /* the program's official name */
	char **r_aliases; /* its other names, NULL-terminated */
	int r_number;     /* its program number */
};

/* Open or rewind the database (the argument changes nothing); close it;
 * look a program up by name or alias, or by number; the next entry. */
void setrpcent(int);
void endrpcent(void);
struct rpcent *getrpcbyname(const char *);
struct rpcent *getrpcbynumber(int);
struct rpcent *getrpcent(void);

#ifdef _SLATEOS_USE_MISC
/* The same into the caller's storage: 0, with *result the entry or NULL;
 * ERANGE when the buffer is too small for it. */
int getrpcbyname_r(const char *, struct rpcent *, char *, size_t, struct rpcent **);
int getrpcbynumber_r(int, struct rpcent *, char *, size_t, struct rpcent **);
int getrpcent_r(struct rpcent *, char *, size_t, struct rpcent **);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _RPC_NETDB_H */
