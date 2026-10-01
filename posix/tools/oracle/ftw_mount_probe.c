/*
 * ftw_mount_probe.c -- what glibc's nftw does with FTW_MOUNT: which entries
 * of a tree with a filesystem mounted inside it it reports, and how.
 *
 * It mounts a tmpfs, so it runs in a user and mount namespace of its own
 * (no root needed), under WSL (Ubuntu 24.04: glibc 2.39, Linux 6.6):
 *
 *     gcc -O2 -Wall -Wextra -o ftw_mount_probe ftw_mount_probe.c
 *     unshare -rm ./ftw_mount_probe
 *
 * It leaves nothing behind: the namespace's mount goes with it, and the tree
 * in /tmp is removed.
 *
 * Read by posix/src/ftw.rs, whose FTW_MOUNT tests hold its walk to these
 * answers. Each walk's visits are sorted, so that directory order -- which
 * glibc leaves to the filesystem -- does not enter.
 *
 * The tree:   top/a   top/sub/b   top/mnt/ (a tmpfs: c, d/e)
 *             top/link-to-mnt -> mnt   top/link-to-mnt-c -> mnt/c
 *             top/nox/f, top/nox readable but not searchable, so that f
 *             can be listed and not stat'ed: it has no device to compare
 *
 * Its answers under glibc 2.39-0ubuntu8.9 and Linux 6.6.87.2-microsoft-
 * standard-WSL2, 2026-10-01:
 *
 *   top and top/mnt are on different devices
 *   == nftw(top, FTW_PHYS) -> 0
 *   FTW_D   level 0  top
 *   FTW_D   level 1  top/mnt
 *   FTW_D   level 1  top/nox
 *   FTW_D   level 1  top/sub
 *   FTW_D   level 2  top/mnt/d
 *   FTW_F   level 1  top/a
 *   FTW_F   level 2  top/mnt/c
 *   FTW_F   level 2  top/sub/b
 *   FTW_F   level 3  top/mnt/d/e
 *   FTW_NS  level 2  top/nox/f
 *   FTW_SL  level 1  top/link-to-mnt
 *   FTW_SL  level 1  top/link-to-mnt-c
 *   == nftw(top, FTW_PHYS|FTW_MOUNT) -> 0
 *   FTW_D   level 0  top
 *   FTW_D   level 1  top/nox
 *   FTW_D   level 1  top/sub
 *   FTW_F   level 1  top/a
 *   FTW_F   level 2  top/sub/b
 *   FTW_NS  level 2  top/nox/f
 *   FTW_SL  level 1  top/link-to-mnt
 *   FTW_SL  level 1  top/link-to-mnt-c
 *   == nftw(top, FTW_MOUNT) -> 0
 *   FTW_D   level 0  top
 *   FTW_D   level 1  top/nox
 *   FTW_D   level 1  top/sub
 *   FTW_F   level 1  top/a
 *   FTW_F   level 2  top/sub/b
 *   FTW_NS  level 2  top/nox/f
 *   == nftw(top, FTW_PHYS|FTW_MOUNT|FTW_DEPTH) -> 0
 *   FTW_DP  level 0  top
 *   FTW_DP  level 1  top/nox
 *   FTW_DP  level 1  top/sub
 *   FTW_F   level 1  top/a
 *   FTW_F   level 2  top/sub/b
 *   FTW_NS  level 2  top/nox/f
 *   FTW_SL  level 1  top/link-to-mnt
 *   FTW_SL  level 1  top/link-to-mnt-c
 *   == nftw(top/mnt, FTW_PHYS|FTW_MOUNT) -> 0
 *   FTW_D   level 0  top/mnt
 *   FTW_D   level 1  top/mnt/d
 *   FTW_F   level 1  top/mnt/c
 *   FTW_F   level 2  top/mnt/d/e
 */

#define _GNU_SOURCE
#include <fcntl.h>
#include <ftw.h>
#include <linux/capability.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

#define MAX_VISITS 64

static char visits[MAX_VISITS][128];
static int nvisits;

static const char *flag_name(int flag)
{
    switch (flag) {
    case FTW_F: return "FTW_F";
    case FTW_D: return "FTW_D";
    case FTW_DNR: return "FTW_DNR";
    case FTW_NS: return "FTW_NS";
    case FTW_SL: return "FTW_SL";
    case FTW_DP: return "FTW_DP";
    case FTW_SLN: return "FTW_SLN";
    default: return "?";
    }
}

static int visit(const char *path, const struct stat *sb, int flag, struct FTW *f)
{
    (void)sb;
    if (nvisits < MAX_VISITS) {
        snprintf(visits[nvisits++], sizeof visits[0], "%-7s level %d  %s", flag_name(flag), f->level,
                 path);
    }
    return 0;
}

static int by_text(const void *a, const void *b)
{
    return strcmp((const char *)a, (const char *)b);
}

static void walk(const char *root, const char *flags_name, int flags)
{
    nvisits = 0;
    int r = nftw(root, visit, 8, flags);
    qsort(visits, (size_t)nvisits, sizeof visits[0], by_text);
    printf("== nftw(%s, %s) -> %d\n", root, flags_name, r);
    for (int i = 0; i < nvisits; i++) {
        printf("%s\n", visits[i]);
    }
}

static int touch(const char *path)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    return fd < 0 ? -1 : close(fd);
}

int main(void)
{
    char dir[] = "/tmp/ftw_mount_probeXXXXXX";
    if (mkdtemp(dir) == NULL || chdir(dir) != 0) {
        perror("mkdtemp");
        return 1;
    }
    if (mkdir("top", 0755) != 0 || mkdir("top/sub", 0755) != 0 || mkdir("top/mnt", 0755) != 0
        || touch("top/a") != 0 || touch("top/sub/b") != 0) {
        perror("the tree");
        return 1;
    }
    if (mount("none", "top/mnt", "tmpfs", 0, NULL) != 0) {
        perror("mount (run it under unshare -rm)");
        return 1;
    }
    if (touch("top/mnt/c") != 0 || mkdir("top/mnt/d", 0755) != 0 || touch("top/mnt/d/e") != 0
        || symlink("mnt", "top/link-to-mnt") != 0 || symlink("mnt/c", "top/link-to-mnt-c") != 0) {
        perror("the mounted part");
        return 1;
    }
    /* A directory that can be read but not searched: its entry can be
     * listed and not stat'ed, so it has no device to compare. */
    if (mkdir("top/nox", 0755) != 0 || touch("top/nox/f") != 0 || chmod("top/nox", 0600) != 0) {
        perror("top/nox");
        return 1;
    }
    struct stat top, mnt;
    if (stat("top", &top) != 0 || stat("top/mnt", &mnt) != 0) {
        return 1;
    }
    printf("top and top/mnt are on %s devices\n", top.st_dev == mnt.st_dev ? "the same" : "different");
    fflush(stdout);

    /* The namespace's root may search anything, so the walks are a child's
     * that gives up its capabilities; this process keeps them, to unmount
     * and clean up. */
    pid_t pid = fork();
    if (pid == 0) {
        struct __user_cap_header_struct cap_header = { _LINUX_CAPABILITY_VERSION_3, 0 };
        struct __user_cap_data_struct no_caps[2];
        memset(no_caps, 0, sizeof no_caps);
        if (syscall(SYS_capset, &cap_header, no_caps) != 0) {
            perror("capset");
            _exit(1);
        }
        walk("top", "FTW_PHYS", FTW_PHYS);
        walk("top", "FTW_PHYS|FTW_MOUNT", FTW_PHYS | FTW_MOUNT);
        walk("top", "FTW_MOUNT", FTW_MOUNT);
        walk("top", "FTW_PHYS|FTW_MOUNT|FTW_DEPTH", FTW_PHYS | FTW_MOUNT | FTW_DEPTH);
        walk("top/mnt", "FTW_PHYS|FTW_MOUNT", FTW_PHYS | FTW_MOUNT);
        fflush(stdout);
        _exit(0);
    }
    int status = 1;
    if (pid < 0 || waitpid(pid, &status, 0) != pid || status != 0) {
        printf("the walks failed\n");
    }

    if (umount("top/mnt") != 0) {
        perror("umount");
    }
    if (chmod("top/nox", 0755) != 0 || unlink("top/nox/f") != 0 || rmdir("top/nox") != 0) {
        perror("top/nox");
    }
    unlink("top/link-to-mnt");
    unlink("top/link-to-mnt-c");
    unlink("top/sub/b");
    unlink("top/a");
    rmdir("top/sub");
    rmdir("top/mnt");
    rmdir("top");
    if (chdir("/") != 0 || rmdir(dir) != 0) {
        perror("cleaning up");
    }
    return 0;
}
