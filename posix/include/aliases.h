/*
 * SlateOS: <aliases.h> -- the mail aliases database, /etc/aliases, which
 * musl does not have.
 *
 * struct aliasent is glibc's, field for field; the functions read the file
 * as glibc's nss_files reads it (posix/src/aliases.rs).
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _ALIASES_H
#define _ALIASES_H 1

#include <features.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/* An alias: its name and the addresses it stands for. */
struct aliasent {
	char *alias_name;         /* the alias */
	size_t alias_members_len; /* how many members it has */
	char **alias_members;     /* its members, alias_members_len of them */
	int alias_local;          /* 1: this host's (every /etc/aliases entry) */
};

/* Open or rewind the database; close it. */
void setaliasent(void);
void endaliasent(void);

/* The next alias -- into the caller's storage: 0, ENOENT at the end, ERANGE
 * when the buffer is too small (the same alias comes next time). */
struct aliasent *getaliasent(void);
int getaliasent_r(struct aliasent *__restrict, char *__restrict, size_t,
                  struct aliasent **__restrict);

/* The alias of that name, ignoring case -- into the caller's storage: 0 with
 * *result the alias or NULL, ERANGE, or the error reading the file. */
struct aliasent *getaliasbyname(const char *);
int getaliasbyname_r(const char *__restrict, struct aliasent *__restrict, char *__restrict,
                     size_t, struct aliasent **__restrict);

#ifdef __cplusplus
}
#endif

#endif /* _ALIASES_H */
