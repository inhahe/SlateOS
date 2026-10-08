/*
 * ctest-seals -- do file seals hold on SlateOS?
 *
 * A seal forbids something to a file for the rest of its life, and to
 * everyone: F_SEAL_WRITE that it be written, F_SEAL_SHRINK that it be made
 * smaller, F_SEAL_GROW larger, F_SEAL_SEAL that any seal be added. A program
 * that hands another process a memfd (an in-memory file) seals it first, so
 * that what the receiver checked is what it reads -- Wayland clients and
 * compositors do exactly this with keymaps and shared buffers. Until lane
 * A's SYS_FS_ADD_SEALS and SYS_FS_GET_SEALS, the C library answered EINVAL
 * to both fcntl commands, as Linux does for a file that cannot be sealed.
 *
 * The checks, in order:
 *
 *    1x  a memfd made with MFD_ALLOW_SEALING starts unsealed; sealed against
 *        writing, a write is refused (EPERM) and the seals read back
 *    2x  F_SEAL_SHRINK refuses a smaller size and allows a larger;
 *        F_SEAL_GROW refuses a larger size and a write past the end
 *    3x  a memfd made without MFD_ALLOW_SEALING starts sealed with
 *        F_SEAL_SEAL, so nothing more may be added (EPERM); one made with
 *        MFD_NOEXEC_SEAL starts with F_SEAL_EXEC
 *    4x  the refusals: F_SEAL_SEAL forbids further seals (EPERM), an unknown
 *        bit is EINVAL, and a pipe has no seals (EINVAL)
 *
 * Exit codes: 42 is every check passed.
 *    2  a setup call failed     Otherwise <check><step>; no step is 42.
 *
 * The same source compiles and passes on Linux, which is how its
 * expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#ifndef MFD_NOEXEC_SEAL
#define MFD_NOEXEC_SEAL 0x0008U
#endif
#ifndef F_SEAL_EXEC
#define F_SEAL_EXEC 0x0020
#endif

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

static void emit_num(long n)
{
    char buf[24];
    int i = (int)sizeof buf;
    int neg = n < 0;
    unsigned long v = neg ? 0UL - (unsigned long)n : (unsigned long)n;
    buf[--i] = '\0';
    do {
        buf[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v != 0 && i > 1);
    if (neg) {
        buf[--i] = '-';
    }
    emit(buf + i);
}

static int fail(int code, const char *what)
{
    int e = errno;
    emit("ctest-seals: FAIL ");
    emit_num(code);
    emit(": ");
    emit(what);
    emit(" (errno ");
    emit_num(e);
    emit(")\n");
    return code;
}

static void ok(const char *what)
{
    emit("ctest-seals: ok ");
    emit(what);
    emit("\n");
}

static int refused(int ret, int want)
{
    return ret == -1 && errno == want;
}

static off_t size_of(int fd)
{
    struct stat st;
    return fstat(fd, &st) == 0 ? st.st_size : -1;
}

int main(void)
{
    /* 1. */
    int fd = memfd_create("ctest-seals", MFD_ALLOW_SEALING);
    if (fd < 0) {
        return fail(2, "memfd_create(MFD_ALLOW_SEALING)");
    }
    if (fcntl(fd, F_GET_SEALS) != 0) {
        return fail(11, "a sealable memfd starts with no seals");
    }
    if (write(fd, "hello", 5) != 5) {
        return fail(12, "a write before the seal");
    }
    if (fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE) != 0) {
        return fail(13, "fcntl(F_ADD_SEALS, F_SEAL_WRITE)");
    }
    errno = 0;
    if (!refused((int)write(fd, "more", 4), EPERM)) {
        return fail(14, "a write after F_SEAL_WRITE is EPERM");
    }
    /* A truncate is not asked of F_SEAL_WRITE here: Linux leaves a sealed
     * memfd's size to F_SEAL_SHRINK and F_SEAL_GROW, and SlateOS's kernel,
     * whose seals apply to any file, refuses it too ("the contents are
     * fixed", lane A's design-decisions 1526). Check 2 asks the size seals. */
    if (fcntl(fd, F_GET_SEALS) != F_SEAL_WRITE) {
        return fail(16, "F_GET_SEALS reads back F_SEAL_WRITE");
    }
    close(fd);
    ok("sealed against writing, a write is refused");

    /* 2. */
    fd = memfd_create("ctest-seals-size", MFD_ALLOW_SEALING);
    if (fd < 0 || ftruncate(fd, 100) != 0) {
        return fail(2, "a 100-byte sealable memfd");
    }
    if (fcntl(fd, F_ADD_SEALS, F_SEAL_SHRINK) != 0) {
        return fail(21, "fcntl(F_ADD_SEALS, F_SEAL_SHRINK)");
    }
    errno = 0;
    if (!refused(ftruncate(fd, 50), EPERM)) {
        return fail(22, "a smaller size after F_SEAL_SHRINK is EPERM");
    }
    if (ftruncate(fd, 200) != 0 || size_of(fd) != 200) {
        return fail(23, "a larger size is allowed after F_SEAL_SHRINK");
    }
    if (fcntl(fd, F_ADD_SEALS, F_SEAL_GROW) != 0) {
        return fail(24, "fcntl(F_ADD_SEALS, F_SEAL_GROW)");
    }
    errno = 0;
    if (!refused(ftruncate(fd, 300), EPERM)) {
        return fail(25, "a larger size after F_SEAL_GROW is EPERM");
    }
    errno = 0;
    if (!refused((int)pwrite(fd, "x", 1, 200), EPERM)) {
        return fail(26, "a write past the end after F_SEAL_GROW is EPERM");
    }
    if (pwrite(fd, "y", 1, 10) != 1) {
        return fail(27, "a write inside the file is still allowed");
    }
    if (fcntl(fd, F_GET_SEALS) != (F_SEAL_SHRINK | F_SEAL_GROW)) {
        return fail(28, "F_GET_SEALS reads back both");
    }
    close(fd);
    ok("F_SEAL_SHRINK and F_SEAL_GROW hold the size where it is");

    /* 3. */
    fd = memfd_create("ctest-seals-closed", 0);
    if (fd < 0) {
        return fail(2, "memfd_create(0)");
    }
    if (fcntl(fd, F_GET_SEALS) != F_SEAL_SEAL) {
        return fail(31, "a memfd made without MFD_ALLOW_SEALING starts with F_SEAL_SEAL");
    }
    errno = 0;
    if (!refused(fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE), EPERM)) {
        return fail(32, "so no seal may be added: EPERM");
    }
    close(fd);
    fd = memfd_create("ctest-seals-noexec", MFD_NOEXEC_SEAL);
    if (fd < 0) {
        return fail(2, "memfd_create(MFD_NOEXEC_SEAL)");
    }
    int seals = fcntl(fd, F_GET_SEALS);
    if (seals < 0 || (seals & F_SEAL_EXEC) == 0 || (seals & F_SEAL_SEAL) != 0) {
        return fail(33, "MFD_NOEXEC_SEAL starts with F_SEAL_EXEC, and may be sealed further");
    }
    close(fd);
    ok("a memfd starts sealed as Linux makes it");

    /* 4. */
    fd = memfd_create("ctest-seals-final", MFD_ALLOW_SEALING);
    if (fd < 0) {
        return fail(2, "memfd_create(MFD_ALLOW_SEALING)");
    }
    if (fcntl(fd, F_ADD_SEALS, F_SEAL_SEAL) != 0) {
        return fail(41, "fcntl(F_ADD_SEALS, F_SEAL_SEAL)");
    }
    errno = 0;
    if (!refused(fcntl(fd, F_ADD_SEALS, F_SEAL_GROW), EPERM)) {
        return fail(43, "after F_SEAL_SEAL no seal may be added: EPERM");
    }
    close(fd);
    fd = memfd_create("ctest-seals-bits", MFD_ALLOW_SEALING);
    if (fd < 0) {
        return fail(2, "memfd_create(MFD_ALLOW_SEALING)");
    }
    errno = 0;
    if (!refused(fcntl(fd, F_ADD_SEALS, 0x4000), EINVAL)) {
        return fail(44, "an unknown seal bit is EINVAL");
    }
    close(fd);
    int fds[2];
    if (pipe(fds) != 0) {
        return fail(2, "pipe");
    }
    errno = 0;
    if (!refused(fcntl(fds[0], F_GET_SEALS), EINVAL)) {
        return fail(45, "a pipe has no seals: EINVAL");
    }
    close(fds[0]);
    close(fds[1]);
    ok("F_SEAL_SEAL, an unknown bit and a pipe are refused");

    emit("ctest-seals: every check passed\n");
    return 42;
}
