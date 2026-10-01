/*
 * SlateOS: <fstab.h> -- the BSD file-system table interface over
 * /etc/fstab, which musl does not have.
 *
 * struct fstab is glibc's, field for field; the functions read the table as
 * glibc's misc/fstab.c does, through getmntent (posix/src/fstab.rs).
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _FSTAB_H
#define _FSTAB_H 1

#include <features.h>

#ifdef __cplusplus
extern "C" {
#endif

#define _PATH_FSTAB "/etc/fstab"
#define FSTAB "/etc/fstab" /* deprecated */

/* fs_type: what the options say of the file system. */
#define FSTAB_RW "rw" /* read and written */
#define FSTAB_RQ "rq" /* read and written, with quotas */
#define FSTAB_RO "ro" /* read only */
#define FSTAB_SW "sw" /* a swap device */
#define FSTAB_XX "xx" /* to be ignored */

/* An entry of the table. */
struct fstab {
	char *fs_spec;       /* the device */
	char *fs_file;       /* where it is mounted */
	char *fs_vfstype;    /* its file system's type */
	char *fs_mntops;     /* its mount options */
	const char *fs_type; /* FSTAB_*, from the options, or "??" */
	int fs_freq;         /* how often it is dumped, in days */
	int fs_passno;       /* its place in fsck's order */
};

/* The next entry; the first for a device, or a mount point, from the
 * start; open or rewind the table (1, or 0); close it. */
struct fstab *getfsent(void);
struct fstab *getfsspec(const char *);
struct fstab *getfsfile(const char *);
int setfsent(void);
void endfsent(void);

#ifdef __cplusplus
}
#endif

#endif /* _FSTAB_H */
