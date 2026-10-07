/*
 * ctest-fallocate -- ring-3 test of posix_fallocate and fallocate through a
 * real descriptor: the file grows with zeros, the modes the library cannot
 * do are refused as Linux refuses them, and the refusals come in Linux's
 * order.
 *
 * Until 2026-10-06 posix_fallocate grew a file with ftruncate -- on this
 * kernel's ext4 that builds the whole file in kernel memory -- and fallocate
 * with FALLOC_FL_KEEP_SIZE answered 0 having reserved nothing.  Now mode 0
 * writes zeros from the end of the file, and every other mode is
 * EOPNOTSUPP.  The host tests drive that logic against a stand-in file;
 * only here do fstat, pread and pwrite answer for real.
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check: the tens digit names the check, the units the step.  No check is
 * numbered 4, so that no failure reads as 42.
 *
 *   1x  growing: the scratch file could not be made (11); posix_fallocate
 *       of 200000 bytes over 1000 of 'x' is not 0 (12); its size is not
 *       200000 (13); its first 1000 bytes are not the 'x's (14) or the rest
 *       not zeros (15); fallocate mode 0 past the end is not 0 or does not
 *       grow it to 204096 (16); a range inside it is not 0 or changes the
 *       size (17)
 *   2x  refused modes: FALLOC_FL_KEEP_SIZE is not -1/EOPNOTSUPP (21), nor
 *       PUNCH_HOLE|KEEP_SIZE (22); the reserved NO_HIDE_STALE is not
 *       EOPNOTSUPP (23); UNSHARE_RANGE|ZERO_RANGE is not EINVAL (24)
 *   3x  refused descriptors: posix_fallocate on a read-only descriptor is
 *       not EBADF (31), on a pipe's write end not ESPIPE (32), on an
 *       eventfd not ENODEV (33); it changed errno (34); fallocate on -1 is
 *       not EBADF (35); a negative offset on a read-only descriptor is not
 *       EINVAL -- the range is judged first (36)
 *
 * Nothing in it waits.  Its scratch file is /tmp/ctest-fallocate.dat,
 * removed when every check passes.  It needs the generic rung's `file`
 * grant.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <string.h>
#include <sys/eventfd.h>
#include <sys/stat.h>
#include <unistd.h>

#ifndef FALLOC_FL_KEEP_SIZE
#define FALLOC_FL_KEEP_SIZE 0x01
#endif
#ifndef FALLOC_FL_PUNCH_HOLE
#define FALLOC_FL_PUNCH_HOLE 0x02
#endif
#ifndef FALLOC_FL_NO_HIDE_STALE
#define FALLOC_FL_NO_HIDE_STALE 0x04
#endif
#ifndef FALLOC_FL_ZERO_RANGE
#define FALLOC_FL_ZERO_RANGE 0x10
#endif
#ifndef FALLOC_FL_UNSHARE_RANGE
#define FALLOC_FL_UNSHARE_RANGE 0x40
#endif

static const char PATH[] = "/tmp/ctest-fallocate.dat";

/* -1 and errno == want? */
static int refused(int r, int want)
{
    return r == -1 && errno == want;
}

static off_t size_of(int fd)
{
    struct stat st;
    return fstat(fd, &st) == 0 ? st.st_size : -1;
}

/* Every byte of [from, to) at `fd` is `want`? */
static int all_are(int fd, off_t from, off_t to, unsigned char want)
{
    unsigned char buf[4096];
    while (from < to) {
        size_t n = (size_t)(to - from) < sizeof buf ? (size_t)(to - from) : sizeof buf;
        ssize_t got = pread(fd, buf, n, from);
        if (got <= 0)
            return 0;
        for (ssize_t i = 0; i < got; i++)
            if (buf[i] != want)
                return 0;
        from += got;
    }
    return 1;
}

int main(void)
{
    /* 1x */
    int fd = open(PATH, O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    if (fd < 0)
        return 11;
    char xs[1000];
    memset(xs, 'x', sizeof xs);
    if (write(fd, xs, sizeof xs) != (ssize_t)sizeof xs)
        return 11;
    if (posix_fallocate(fd, 0, 200000) != 0)
        return 12;
    if (size_of(fd) != 200000)
        return 13;
    if (!all_are(fd, 0, 1000, 'x'))
        return 14;
    if (!all_are(fd, 1000, 200000, 0))
        return 15;
    if (fallocate(fd, 0, 200000, 4096) != 0 || size_of(fd) != 204096)
        return 16;
    if (fallocate(fd, 0, 4096, 8192) != 0 || size_of(fd) != 204096)
        return 17;

    /* 2x */
    errno = 0;
    if (!refused(fallocate(fd, FALLOC_FL_KEEP_SIZE, 0, 4096), EOPNOTSUPP))
        return 21;
    errno = 0;
    if (!refused(fallocate(fd, FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, 0, 4096), EOPNOTSUPP))
        return 22;
    errno = 0;
    if (!refused(fallocate(fd, FALLOC_FL_NO_HIDE_STALE, 0, 4096), EOPNOTSUPP))
        return 23;
    errno = 0;
    if (!refused(fallocate(fd, FALLOC_FL_UNSHARE_RANGE | FALLOC_FL_ZERO_RANGE, 0, 4096), EINVAL))
        return 24;

    /* 3x */
    int ro = open(PATH, O_RDONLY | O_CLOEXEC);
    int p[2];
    int efd = eventfd(0, EFD_CLOEXEC);
    if (ro < 0 || pipe(p) != 0 || efd < 0)
        return 31;
    errno = 12345;
    if (posix_fallocate(ro, 0, 4096) != EBADF)
        return 31;
    if (posix_fallocate(p[1], 0, 4096) != ESPIPE)
        return 32;
    if (posix_fallocate(efd, 0, 4096) != ENODEV)
        return 33;
    if (errno != 12345)
        return 34;
    errno = 0;
    if (!refused(fallocate(-1, 0, 0, 4096), EBADF))
        return 35;
    errno = 0;
    if (!refused(fallocate(ro, 0, -1, 4096), EINVAL))
        return 36;

    close(efd);
    close(p[0]);
    close(p[1]);
    close(ro);
    close(fd);
    unlink(PATH);
    return 42;
}
