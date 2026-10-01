/*
 * SlateOS: <fts.h> -- file tree traversal, which musl does not have.
 *
 * FTS and FTSENT have glibc's layouts, field for field, and every constant
 * glibc's value, because this library's fts (posix/src/fts.rs, where the
 * layouts are pinned by offset) is glibc's ABI with BSD's algorithm. Fields
 * the walk here does not use are kept at their places: fts_symfd and
 * fts_rfd are always -1, since the walk never changes directory.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _FTS_H
#define _FTS_H 1

#include <features.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

struct stat;

/* An open traversal. */
typedef struct {
	struct _ftsent *fts_cur;   /* the entry fts_read returned last */
	struct _ftsent *fts_child; /* the children fts_children built */
	struct _ftsent **fts_array; /* scratch, for sorting */
	dev_t fts_dev;             /* the device of the root being walked */
	char *fts_path;            /* the path buffer entries point into */
	int fts_rfd;               /* -1 */
	int fts_pathlen;           /* the path buffer's size */
	int fts_nitems;            /* fts_array's size */
	int (*fts_compar)(const void *, const void *); /* the sort order */
	int fts_options;           /* fts_open's options */
} FTS;

/* fts_open's options. */
#define FTS_COMFOLLOW 0x0001  /* follow a symbolic link named as a root */
#define FTS_LOGICAL 0x0002    /* follow every symbolic link */
#define FTS_NOCHDIR 0x0004    /* do not change directory: always, here */
#define FTS_NOSTAT 0x0008     /* no stat for entries that need none */
#define FTS_PHYSICAL 0x0010   /* follow no symbolic link */
#define FTS_SEEDOT 0x0020     /* return . and .. */
#define FTS_XDEV 0x0040       /* stay on the roots' devices */
#define FTS_WHITEOUT 0x0080   /* return whiteouts */
#define FTS_OPTIONMASK 0x00ff /* the options a caller may pass */
#define FTS_NAMEONLY 0x0100   /* fts_children: names only */

/* A file in the hierarchy. Allocated with its name inline, past the end of
 * the structure: never copy one by value. */
typedef struct _ftsent {
	struct _ftsent *fts_cycle;  /* FTS_DC: the ancestor it repeats */
	struct _ftsent *fts_parent; /* the directory containing it */
	struct _ftsent *fts_link;   /* the next entry in that directory */
	long fts_number;            /* the caller's: 0 at first */
	void *fts_pointer;          /* the caller's: NULL at first */
	char *fts_accpath;          /* a path to reach it by */
	char *fts_path;             /* its path from the root */
	int fts_errno;              /* FTS_DNR, FTS_ERR, FTS_NS: the error */
	int fts_symfd;              /* -1 */
	unsigned short fts_pathlen; /* strlen(fts_path) */
	unsigned short fts_namelen; /* strlen(fts_name) */
	ino_t fts_ino;              /* inode number */
	dev_t fts_dev;              /* device */
	nlink_t fts_nlink;          /* link count */
	short fts_level;            /* depth: FTS_ROOTLEVEL for a root */
	unsigned short fts_info;    /* what it is: FTS_D, FTS_F, ... */
	unsigned short fts_flags;   /* FTS_DONTCHDIR, FTS_SYMFOLLOW */
	unsigned short fts_instr;   /* the fts_set instruction pending */
	struct stat *fts_statp;     /* its stat; NULL under FTS_NOSTAT */
	char fts_name[1];           /* its name, the rest after the struct */
} FTSENT;

/* fts_level. */
#define FTS_ROOTPARENTLEVEL (-1)
#define FTS_ROOTLEVEL 0

/* fts_info. */
#define FTS_D 1        /* a directory, before its contents */
#define FTS_DC 2       /* a directory that is its own ancestor */
#define FTS_DEFAULT 3  /* none of the others */
#define FTS_DNR 4      /* a directory that cannot be read */
#define FTS_DOT 5      /* . or .. */
#define FTS_DP 6       /* a directory, after its contents */
#define FTS_ERR 7      /* an error: fts_errno says which */
#define FTS_F 8        /* a regular file */
#define FTS_INIT 9     /* not yet read */
#define FTS_NS 10      /* stat failed */
#define FTS_NSOK 11    /* not stat'd, as asked */
#define FTS_SL 12      /* a symbolic link */
#define FTS_SLNONE 13  /* a symbolic link to nothing */
#define FTS_W 14       /* a whiteout */

/* fts_flags. */
#define FTS_DONTCHDIR 0x01 /* do not chdir .. to the parent */
#define FTS_SYMFOLLOW 0x02 /* reached by following a symbolic link */

/* fts_set's instructions. */
#define FTS_AGAIN 1    /* return this entry again */
#define FTS_FOLLOW 2   /* follow this symbolic link */
#define FTS_NOINSTR 3  /* none */
#define FTS_SKIP 4     /* do not descend into this directory */

/* Walk the files and directories named by the NULL-terminated list, with
 * fts_open's options; the comparison, if not NULL, orders each directory's
 * entries. */
FTS *fts_open(char *const *, int, int (*)(const FTSENT **, const FTSENT **));
/* The next entry, or NULL at the end (errno 0) or on an error. */
FTSENT *fts_read(FTS *);
/* The entries of the directory fts_read returned last, as a linked list. */
FTSENT *fts_children(FTS *, int);
/* Give an instruction about an entry for the next fts_read. */
int fts_set(FTS *, FTSENT *, int);
/* End the walk and free what it holds. */
int fts_close(FTS *);

#if defined(_LARGEFILE64_SOURCE)
/* glibc's large-file names, as musl's headers give the rest of them: macros
 * for the standard ones, whose inode numbers and struct stat are 64-bit
 * already. */
#define FTS64 FTS
#define FTSENT64 FTSENT
#define fts64_open fts_open
#define fts64_read fts_read
#define fts64_children fts_children
#define fts64_set fts_set
#define fts64_close fts_close
#endif

#ifdef __cplusplus
}
#endif

#endif /* _FTS_H */
