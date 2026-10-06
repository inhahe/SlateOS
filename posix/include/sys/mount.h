/*
 * SlateOS: <sys/mount.h> -- musl's, and beside it the new mount API glibc
 * 2.36 declares: fsopen, fsconfig, fsmount, move_mount, fspick, open_tree
 * and mount_setattr, with their flags and struct mount_attr. SlateOS's
 * kernel has no such calls, so each answers ENOSYS, as glibc's wrapper
 * does on a Linux without them, and a program falls back to mount(2)
 * (posix/src/sys_mount.rs).
 */

#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/mount.h>

#ifndef _SLATEOS_SYS_MOUNT_H
#define _SLATEOS_SYS_MOUNT_H

#include <fcntl.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* fsmount flags.  */
#define FSMOUNT_CLOEXEC         0x00000001

/* mount attributes used on fsmount.  */
#define MOUNT_ATTR_RDONLY       0x00000001 /* Mount read-only.  */
#define MOUNT_ATTR_NOSUID       0x00000002 /* Ignore suid and sgid bits.  */
#define MOUNT_ATTR_NODEV        0x00000004 /* Disallow access to device special files.  */
#define MOUNT_ATTR_NOEXEC       0x00000008 /* Disallow program execution.  */
#define MOUNT_ATTR__ATIME       0x00000070 /* Setting on how atime should be updated.  */
#define MOUNT_ATTR_RELATIME     0x00000000 /* - Update atime relative to mtime/ctime.  */
#define MOUNT_ATTR_NOATIME      0x00000010 /* - Do not update access times.  */
#define MOUNT_ATTR_STRICTATIME  0x00000020 /* - Always perform atime updates  */
#define MOUNT_ATTR_NODIRATIME   0x00000080 /* Do not update directory access times.  */
#define MOUNT_ATTR_IDMAP        0x00100000 /* Idmap mount to @userns_fd in struct mount_attr.  */
#define MOUNT_ATTR_NOSYMFOLLOW  0x00200000 /* Do not follow symlinks.  */

#ifndef MOUNT_ATTR_SIZE_VER0
/* For mount_setattr.  */
struct mount_attr {
    uint64_t attr_set;
    uint64_t attr_clr;
    uint64_t propagation;
    uint64_t userns_fd;
};
#endif

#define MOUNT_ATTR_SIZE_VER0    32 /* sizeof first published struct */

/* move_mount flags.  */
#define MOVE_MOUNT_F_SYMLINKS   0x00000001 /* Follow symlinks on from path */
#define MOVE_MOUNT_F_AUTOMOUNTS 0x00000002 /* Follow automounts on from path */
#define MOVE_MOUNT_F_EMPTY_PATH 0x00000004 /* Empty from path permitted */
#define MOVE_MOUNT_T_SYMLINKS   0x00000010 /* Follow symlinks on to path */
#define MOVE_MOUNT_T_AUTOMOUNTS 0x00000020 /* Follow automounts on to path */
#define MOVE_MOUNT_T_EMPTY_PATH 0x00000040 /* Empty to path permitted */
#define MOVE_MOUNT_SET_GROUP    0x00000100 /* Set sharing group instead */
#define MOVE_MOUNT_BENEATH      0x00000200 /* Mount beneath top mount */

/* fspick flags.  */
#define FSPICK_CLOEXEC          0x00000001
#define FSPICK_SYMLINK_NOFOLLOW 0x00000002
#define FSPICK_NO_AUTOMOUNT     0x00000004
#define FSPICK_EMPTY_PATH       0x00000008

#ifndef FSOPEN_CLOEXEC
/* The type of fsconfig call made.   */
enum fsconfig_command {
    FSCONFIG_SET_FLAG = 0,         /* Set parameter, supplying no value */
#define FSCONFIG_SET_FLAG FSCONFIG_SET_FLAG
    FSCONFIG_SET_STRING = 1,       /* Set parameter, supplying a string value */
#define FSCONFIG_SET_STRING FSCONFIG_SET_STRING
    FSCONFIG_SET_BINARY = 2,       /* Set parameter, supplying a binary blob value */
#define FSCONFIG_SET_BINARY FSCONFIG_SET_BINARY
    FSCONFIG_SET_PATH = 3,         /* Set parameter, supplying an object by path */
#define FSCONFIG_SET_PATH FSCONFIG_SET_PATH
    FSCONFIG_SET_PATH_EMPTY = 4,   /* Set parameter, supplying an object by (empty) path */
#define FSCONFIG_SET_PATH_EMPTY FSCONFIG_SET_PATH_EMPTY
    FSCONFIG_SET_FD = 5,           /* Set parameter, supplying an object by fd */
#define FSCONFIG_SET_FD FSCONFIG_SET_FD
    FSCONFIG_CMD_CREATE = 6,       /* Invoke superblock creation */
#define FSCONFIG_CMD_CREATE FSCONFIG_CMD_CREATE
    FSCONFIG_CMD_RECONFIGURE = 7,  /* Invoke superblock reconfiguration */
#define FSCONFIG_CMD_RECONFIGURE FSCONFIG_CMD_RECONFIGURE
    FSCONFIG_CMD_CREATE_EXCL = 8   /* Create new superblock, fail if reusing existing superblock */
#define FSCONFIG_CMD_CREATE_EXCL FSCONFIG_CMD_CREATE_EXCL
};
#endif

/* fsopen flags.  */
#define FSOPEN_CLOEXEC          0x00000001

/* open_tree flags.  */
#define OPEN_TREE_CLONE    1         /* Clone the target tree and attach the clone */
#define OPEN_TREE_CLOEXEC  O_CLOEXEC /* Close the file on execve() */

int fsopen(const char *__fs_name, unsigned int __flags);
int fsmount(int __fd, unsigned int __flags, unsigned int __ms_flags);
int move_mount(int __from_dfd, const char *__from_pathname, int __to_dfd,
               const char *__to_pathname, unsigned int __flags);
int fsconfig(int __fd, unsigned int __cmd, const char *__key, const void *__value,
             int __aux);
int fspick(int __dfd, const char *__path, unsigned int __flags);
int open_tree(int __dfd, const char *__filename, unsigned int __flags);
int mount_setattr(int __dfd, const char *__path, unsigned int __flags,
                  struct mount_attr *__uattr, size_t __usize);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_MOUNT_H */
