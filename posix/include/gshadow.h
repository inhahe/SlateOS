/*
 * SlateOS: <gshadow.h> -- the shadow group database, /etc/gshadow, which
 * musl does not have.
 *
 * struct sgrp is glibc's, field for field; the functions read the file as
 * glibc's nss_files reads it (posix/src/gshadow.rs).
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _GSHADOW_H
#define _GSHADOW_H 1

#include <features.h>

#define __NEED_FILE
#define __NEED_size_t
#include <bits/alltypes.h>

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The file (glibc's GSHADOW, its <paths.h>'s _PATH_GSHADOW). */
#define GSHADOW "/etc/gshadow"

/* A group's shadow entry. */
struct sgrp {
	char *sg_namp;   /* the group's name */
	char *sg_passwd; /* its encrypted password */
	char **sg_adm;   /* its administrators, NULL-terminated */
	char **sg_mem;   /* its members, NULL-terminated */
};

/* Rewind, and close, the database getsgent reads. */
void setsgent(void);
void endsgent(void);
/* The database's next entry; a group's; a line's; a stream's next. */
struct sgrp *getsgent(void);
struct sgrp *getsgnam(const char *);
struct sgrp *sgetsgent(const char *);
struct sgrp *fgetsgent(FILE *);
/* An entry, as a line of the file. */
int putsgent(const struct sgrp *, FILE *);

#ifdef _SLATEOS_USE_MISC
/* The same, into the caller's structure and buffer. */
int getsgent_r(struct sgrp *, char *, size_t, struct sgrp **);
int getsgnam_r(const char *, struct sgrp *, char *, size_t, struct sgrp **);
int sgetsgent_r(const char *, struct sgrp *, char *, size_t, struct sgrp **);
int fgetsgent_r(FILE *, struct sgrp *, char *, size_t, struct sgrp **);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _GSHADOW_H */
