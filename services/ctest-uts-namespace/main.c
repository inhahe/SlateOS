/*
 * ctest-uts-namespace -- ring-3 test of a namespace's descriptor, and of
 * unshare and setns as an unprivileged process meets them.
 *
 * Until 2026-10-17 the C library's unshare and setns answered ENOSYS, and
 * /proc/<pid>/ns/<kind> opened as nothing. Lane A's kernel has UTS
 * namespaces and six native calls for them (1161-1166,
 * requests/a-d-the-namespace-calls-have-native-numbers.md); the library
 * opens a namespace's link into a descriptor of its own kind, answers
 * fstat, readlink and NS_GET_NSTYPE from the kernel, and passes unshare and
 * setns to it. The host has no kernel to ask, so only here is it seen.
 *
 * The rung grants no right over namespaces, so the changes are refused
 * here, by the kernel, in Linux's order -- what a non-root process sees on
 * Linux, where this passes too (Linux 6.6.87, run as a user: as root the
 * refusals would not happen there). The rung's process is root, but on
 * SlateOS an id of 0 gives no right over namespaces.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  /proc/self/ns/uts opens (10); its fstat is a regular file read by
 *       all (11), the same file stat finds through the link (12); its
 *       /proc/self/fd link and the namespace's link read uts:[<inode>]
 *       (13); NS_GET_NSTYPE answers CLONE_NEWUTS (15)
 *   2x  it cannot be read -- EINVAL (20) -- nor written, EBADF as it was
 *       opened read-only (21), nor sought, ESPIPE (22)
 *   3x  unshare(CLONE_NEWUTS) is EPERM without the right (30); setns with
 *       the descriptor and another kind is EINVAL before the right is asked
 *       (31), and with its own kind EPERM (32); setns with a pipe's end is
 *       EINVAL (33)
 *   5x  a dup of it is the same namespace (50), and outlives the close of
 *       the first (51)
 *   6x  openat through /proc/self's descriptor opens the same namespace (60);
 *       open's rules for a link to a regular file: O_DIRECTORY is ENOTDIR
 *       (61), O_NOFOLLOW ELOOP (62), O_CREAT|O_EXCL EEXIST (63)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <unistd.h>

#define NS_GET_NSTYPE 0xb703

static int fails_with(long ret, int err)
{
    return ret == -1 && errno == err;
}

int main(void)
{
    alarm(20);
    int fd = open("/proc/self/ns/uts", O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return 10;

    struct stat a, b;
    if (fstat(fd, &a) != 0 || !S_ISREG(a.st_mode) || (a.st_mode & 0777) != 0444)
        return 11;
    if (stat("/proc/self/ns/uts", &b) != 0 || b.st_ino != a.st_ino || b.st_dev != a.st_dev)
        return 12;

    char link[64], text[64], via_ns[64], want[64];
    snprintf(link, sizeof link, "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, text, sizeof text - 1);
    ssize_t m = readlink("/proc/self/ns/uts", via_ns, sizeof via_ns - 1);
    if (n < 0 || m < 0)
        return 13;
    text[n] = 0;
    via_ns[m] = 0;
    snprintf(want, sizeof want, "uts:[%lu]", (unsigned long)a.st_ino);
    if (strcmp(text, want) != 0 || strcmp(via_ns, want) != 0)
        return 13;
    if (ioctl(fd, NS_GET_NSTYPE) != CLONE_NEWUTS)
        return 15;

    char c;
    errno = 0;
    if (!fails_with(read(fd, &c, 1), EINVAL))
        return 20;
    errno = 0;
    if (!fails_with(write(fd, "x", 1), EBADF))
        return 21;
    errno = 0;
    if (!fails_with(lseek(fd, 0, SEEK_SET), ESPIPE))
        return 22;

    errno = 0;
    if (!fails_with(unshare(CLONE_NEWUTS), EPERM))
        return 30;
    errno = 0;
    if (!fails_with(setns(fd, CLONE_NEWNET), EINVAL))
        return 31;
    errno = 0;
    if (!fails_with(setns(fd, CLONE_NEWUTS), EPERM))
        return 32;
    int p[2];
    if (pipe(p) != 0)
        return 33;
    errno = 0;
    if (!fails_with(setns(p[0], 0), EINVAL))
        return 33;

    int again = dup(fd);
    struct stat d;
    if (again < 0 || fstat(again, &d) != 0 || d.st_ino != a.st_ino)
        return 50;
    close(fd);
    if (fstat(again, &d) != 0 || d.st_ino != a.st_ino || ioctl(again, NS_GET_NSTYPE) != CLONE_NEWUTS)
        return 51;
    close(again);

    int dir = open("/proc/self", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    int via = dir < 0 ? -1 : openat(dir, "ns/uts", O_RDONLY | O_CLOEXEC);
    struct stat v;
    if (via < 0 || fstat(via, &v) != 0 || v.st_ino != a.st_ino || ioctl(via, NS_GET_NSTYPE) != CLONE_NEWUTS)
        return 60;
    close(via);
    close(dir);
    errno = 0;
    if (!fails_with(open("/proc/self/ns/uts", O_RDONLY | O_DIRECTORY), ENOTDIR))
        return 61;
    errno = 0;
    if (!fails_with(open("/proc/self/ns/uts", O_RDONLY | O_NOFOLLOW), ELOOP))
        return 62;
    errno = 0;
    if (!fails_with(open("/proc/self/ns/uts", O_RDONLY | O_CREAT | O_EXCL, 0600), EEXIST))
        return 63;
    return 42;
}
