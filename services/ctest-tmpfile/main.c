/*
 * ctest-tmpfile -- can a SlateOS program make a file with no name, write it,
 * and then give it one?
 *
 * open(dir, O_TMPFILE | O_RDWR) makes a regular file in `dir` that has no
 * name: no other program can open it, nothing of it is left behind if the
 * program dies, and it goes at its last close -- unless linkat names it
 * first. That is how a program replaces a file safely: write the new one
 * whole, then publish it in one step. Until lane A's SYS_FS_TMPFILE made such
 * files and SYS_FS_LINK_HANDLE named them, the C library answered
 * EOPNOTSUPP for O_TMPFILE and ENOENT for the linkat.
 *
 * The checks, in order:
 *
 *    1x  O_TMPFILE makes a file that takes writes and is in no listing of
 *        the directory
 *    2x  linkat(fd, "", ..., AT_EMPTY_PATH) gives it a name, with what was
 *        written (on Linux before 6.10 that needs CAP_DAC_READ_SEARCH, and
 *        an ordinary user is told ENOENT: then that answer is accepted)
 *    3x  linkat of /proc/self/fd/N with AT_SYMLINK_FOLLOW -- glibc's
 *        documented form -- names another
 *    4x  O_TMPFILE | O_EXCL makes one that can never be named: ENOENT
 *    5x  one closed without a name leaves nothing behind
 *    6x  the refusals: a directory that is a file (ENOTDIR), O_TMPFILE
 *        without writing (EINVAL), a name already taken (EEXIST)
 *
 * Exit codes: 42 is every check passed.
 *    2  a setup call failed     Otherwise <check><step>; no step is 42.
 *
 * The same source compiles and passes on Linux, which is how its
 * expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <unistd.h>

#define DIR_PATH "/tmp/ctest-tmpfile"

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
    emit("ctest-tmpfile: FAIL ");
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
    emit("ctest-tmpfile: ok ");
    emit(what);
    emit("\n");
}

/* How many entries the directory lists, "." and ".." aside; -1 on error. */
static int entries(void)
{
    DIR *d = opendir(DIR_PATH);
    if (d == NULL) {
        return -1;
    }
    int n = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (strcmp(e->d_name, ".") != 0 && strcmp(e->d_name, "..") != 0) {
            n++;
        }
    }
    closedir(d);
    return n;
}

/* Whether the file at `path` holds exactly `want`. */
static int holds(const char *path, const char *want)
{
    char buf[64];
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        return 0;
    }
    ssize_t n = read(fd, buf, sizeof buf);
    close(fd);
    return n == (ssize_t)strlen(want) && memcmp(buf, want, (size_t)n) == 0;
}

/* Linux before 6.10 lets only a process with CAP_DAC_READ_SEARCH link a
 * descriptor with AT_EMPTY_PATH, and tells anyone else ENOENT. */
static int linux_before_6_10(void)
{
    struct utsname u;
    if (uname(&u) != 0 || strcmp(u.sysname, "Linux") != 0) {
        return 0;
    }
    int major = 0, minor = 0;
    if (sscanf(u.release, "%d.%d", &major, &minor) != 2) {
        return 0;
    }
    return major < 6 || (major == 6 && minor < 10);
}

static void clean(void)
{
    unlink(DIR_PATH "/published");
    unlink(DIR_PATH "/second");
    unlink(DIR_PATH "/plain");
    rmdir(DIR_PATH);
}

static int checks(void)
{
    /* 1. */
    int before = entries();
    int fd = open(DIR_PATH, O_TMPFILE | O_RDWR, 0600);
    if (fd < 0) {
        return fail(11, "open(dir, O_TMPFILE | O_RDWR)");
    }
    if (write(fd, "hello", 5) != 5) {
        return fail(12, "a write to the nameless file");
    }
    if (entries() != before) {
        return fail(13, "the nameless file is in no listing");
    }
    struct stat st;
    if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode) || st.st_size != 5 || st.st_nlink != 0) {
        return fail(14, "fstat: a regular file of 5 bytes and no links");
    }
    ok("O_TMPFILE makes a file with no name that takes writes");

    /* 2. */
    char proc[64];
    if (linkat(fd, "", AT_FDCWD, DIR_PATH "/published", AT_EMPTY_PATH) != 0) {
        if (errno != ENOENT || !linux_before_6_10()) {
            return fail(21, "linkat(fd, \"\", ..., AT_EMPTY_PATH)");
        }
        emit("ctest-tmpfile: (AT_EMPTY_PATH asks CAP_DAC_READ_SEARCH on this Linux;"
             " naming it through /proc/self/fd instead)\n");
        snprintf(proc, sizeof proc, "/proc/self/fd/%d", fd);
        if (linkat(AT_FDCWD, proc, AT_FDCWD, DIR_PATH "/published", AT_SYMLINK_FOLLOW) != 0) {
            return fail(22, "linkat of /proc/self/fd/N");
        }
    }
    close(fd);
    if (!holds(DIR_PATH "/published", "hello")) {
        return fail(23, "the named file holds what was written");
    }
    if (entries() != before + 1) {
        return fail(24, "the named file is listed");
    }
    ok("linkat names it, with what was written");

    /* 3. */
    fd = open(DIR_PATH, O_TMPFILE | O_WRONLY, 0600);
    if (fd < 0 || write(fd, "second", 6) != 6) {
        return fail(31, "a second nameless file");
    }
    snprintf(proc, sizeof proc, "/proc/self/fd/%d", fd);
    if (linkat(AT_FDCWD, proc, AT_FDCWD, DIR_PATH "/second", AT_SYMLINK_FOLLOW) != 0) {
        return fail(32, "linkat of /proc/self/fd/N with AT_SYMLINK_FOLLOW");
    }
    close(fd);
    if (!holds(DIR_PATH "/second", "second")) {
        return fail(33, "the file named through /proc holds what was written");
    }
    ok("linkat of /proc/self/fd/N names one too");

    /* 4. */
    fd = open(DIR_PATH, O_TMPFILE | O_RDWR | O_EXCL, 0600);
    if (fd < 0) {
        return fail(41, "open(dir, O_TMPFILE | O_RDWR | O_EXCL)");
    }
    errno = 0;
    if (linkat(fd, "", AT_FDCWD, DIR_PATH "/never", AT_EMPTY_PATH) != -1 || errno != ENOENT) {
        return fail(43, "an O_EXCL one cannot be named: ENOENT");
    }
    snprintf(proc, sizeof proc, "/proc/self/fd/%d", fd);
    errno = 0;
    if (linkat(AT_FDCWD, proc, AT_FDCWD, DIR_PATH "/never", AT_SYMLINK_FOLLOW) != -1 || errno != ENOENT) {
        return fail(44, "nor through /proc/self/fd: ENOENT");
    }
    close(fd);
    ok("an O_EXCL one can never be named");

    /* 5. */
    int now = entries();
    fd = open(DIR_PATH, O_TMPFILE | O_RDWR, 0600);
    if (fd < 0 || write(fd, "gone", 4) != 4) {
        return fail(51, "a nameless file to let go");
    }
    close(fd);
    if (entries() != now) {
        return fail(52, "a nameless file closed leaves nothing behind");
    }
    ok("one closed without a name leaves nothing behind");

    /* 6. */
    int plain = open(DIR_PATH "/plain", O_CREAT | O_WRONLY | O_TRUNC, 0600);
    if (plain < 0) {
        return fail(2, "create a plain file");
    }
    close(plain);
    errno = 0;
    if (open(DIR_PATH "/plain", O_TMPFILE | O_RDWR, 0600) != -1 || errno != ENOTDIR) {
        return fail(61, "O_TMPFILE in a file that is no directory: ENOTDIR");
    }
    errno = 0;
    if (open(DIR_PATH, O_TMPFILE | O_RDONLY, 0600) != -1 || errno != EINVAL) {
        return fail(62, "O_TMPFILE without writing: EINVAL");
    }
    fd = open(DIR_PATH, O_TMPFILE | O_RDWR, 0600);
    if (fd < 0) {
        return fail(2, "open(dir, O_TMPFILE | O_RDWR)");
    }
    snprintf(proc, sizeof proc, "/proc/self/fd/%d", fd);
    errno = 0;
    if (linkat(AT_FDCWD, proc, AT_FDCWD, DIR_PATH "/published", AT_SYMLINK_FOLLOW) != -1
        || errno != EEXIST) {
        return fail(63, "a name already taken: EEXIST");
    }
    close(fd);
    ok("a file for a directory, no write access and a taken name are refused");
    return 0;
}

int main(void)
{
    clean();
    if (mkdir(DIR_PATH, 0700) != 0) {
        return fail(2, "mkdir " DIR_PATH);
    }
    int r = checks();
    clean();
    if (r != 0) {
        return r;
    }
    emit("ctest-tmpfile: every check passed\n");
    return 42;
}
