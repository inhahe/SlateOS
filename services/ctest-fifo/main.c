/*
 * ctest-fifo -- ring-3 test of named pipes (FIFOs).
 *
 * Until 2026-10-16 mkfifo answered ENOSYS here, and an open of a FIFO's node
 * would have answered ENXIO. Lane A's kernel has FIFOs (SYS_FS_MKFIFO and
 * SYS_FIFO_OPEN, requests/a-d-named-pipes-have-native-calls.md), and the C
 * library makes them through mknod and opens them through open: a pipe
 * descriptor that answers fstat and /proc/self/fd for its node. The host
 * tests have no kernel to make one, so only here are they seen.
 *
 * Every wait is bounded -- a blocking open waits for a child that opens the
 * other side at once -- and an alarm ends the test after twenty seconds, so
 * it cannot hang.
 *
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  mkfifo makes a FIFO (10) with the bits asked less the umask (11);
 *       a name that exists is EEXIST (12); mknod(S_IFIFO) makes one too
 *       (13); readdir lists it as DT_FIFO (15)
 *   2x  a nonblocking reader opens with no writer (20) and reads end of
 *       file (21); a nonblocking writer with no reader is ENXIO (22), and
 *       opens once there is one (23)
 *   3x  what one end writes the other reads (30); fstat of the descriptor
 *       is the node's (31); /proc/self/fd names the node (32); F_GETFL is
 *       the access mode and O_NONBLOCK (33); O_CLOEXEC sets FD_CLOEXEC (35)
 *   5x  a blocking reader waits for a writer in another process and reads
 *       what it writes (50), then end of file once it has gone (51)
 *   6x  O_RDWR opens without a partner and reads back what it writes (60)
 *   7x  mkfifoat makes one relative to a directory's descriptor (70)
 *   8x  a regular file is no FIFO: opening it is a file as before (80)
 *   9x  every exit of an open leaves no descriptor behind: the failed
 *       ENXIO open did not use one (90)
 */

#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static char dir[64];

static void path_of(char *out, const char *name)
{
    snprintf(out, PATH_MAX, "%s/%s", dir, name);
}

static int listed_as_fifo(const char *name)
{
    DIR *d = opendir(dir);
    if (!d)
        return 0;
    int found = 0;
    struct dirent *e;
    while ((e = readdir(d)) != 0)
        if (strcmp(e->d_name, name) == 0)
            found = e->d_type == DT_FIFO;
    closedir(d);
    return found;
}

static int lowest_free_fd(void)
{
    int fd = dup(0);
    if (fd >= 0)
        close(fd);
    return fd;
}

static int check(void)
{
    char f[PATH_MAX], g[PATH_MAX], file[PATH_MAX];
    path_of(f, "f");
    path_of(g, "g");
    path_of(file, "plain");

    umask(027);
    if (mkfifo(f, 0666) != 0)
        return 10;
    struct stat st;
    if (stat(f, &st) != 0 || !S_ISFIFO(st.st_mode))
        return 10;
    if ((st.st_mode & 07777) != 0640)
        return 11;
    errno = 0;
    if (mkfifo(f, 0600) != -1 || errno != EEXIST)
        return 12;
    if (mknod(g, S_IFIFO | 0600, 0) != 0 || stat(g, &st) != 0 || !S_ISFIFO(st.st_mode))
        return 13;
    if (!listed_as_fifo("f"))
        return 15;

    /* 2x */
    int free_before = lowest_free_fd();
    int r = open(f, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (r < 0)
        return 20;
    char buf[16];
    if (read(r, buf, sizeof buf) != 0)
        return 21;
    close(r);
    errno = 0;
    if (open(f, O_WRONLY | O_NONBLOCK) != -1 || errno != ENXIO)
        return 22;
    if (lowest_free_fd() != free_before)
        return 90;
    r = open(f, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    int w = open(f, O_WRONLY | O_NONBLOCK);
    if (r < 0 || w < 0)
        return 23;

    /* 3x */
    if (write(w, "hello", 5) != 5 || read(r, buf, sizeof buf) != 5 || memcmp(buf, "hello", 5) != 0)
        return 30;
    struct stat node, fds;
    if (stat(f, &node) != 0 || fstat(r, &fds) != 0 || !S_ISFIFO(fds.st_mode) || fds.st_ino != node.st_ino
        || fds.st_mode != node.st_mode)
        return 31;
    char link[64], target[PATH_MAX];
    snprintf(link, sizeof link, "/proc/self/fd/%d", r);
    ssize_t n = readlink(link, target, sizeof target - 1);
    if (n < 0)
        return 32;
    target[n] = 0;
    if (strcmp(target, f) != 0)
        return 32;
    /* Masked: Linux's F_GETFL adds the O_LARGEFILE every open on x86_64
     * gets, which <fcntl.h> names 0 there. */
    if ((fcntl(r, F_GETFL) & (O_ACCMODE | O_NONBLOCK | O_APPEND)) != (O_RDONLY | O_NONBLOCK))
        return 33;
    if (fcntl(r, F_GETFD) != FD_CLOEXEC)
        return 35;
    close(w);
    close(r);

    /* 5x */
    pid_t child = fork();
    if (child == 0) {
        int cw = open(f, O_WRONLY);
        if (cw < 0)
            _exit(1);
        _exit(write(cw, "child", 5) == 5 ? 0 : 2);
    }
    r = open(f, O_RDONLY);
    if (r < 0)
        return 50;
    size_t got = 0;
    while (got < 5) {
        ssize_t k = read(r, buf + got, 5 - got);
        if (k <= 0)
            break;
        got += (size_t)k;
    }
    if (got != 5 || memcmp(buf, "child", 5) != 0)
        return 50;
    int status = 0;
    waitpid(child, &status, 0);
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0 || read(r, buf, sizeof buf) != 0)
        return 51;
    close(r);

    /* 6x */
    int rw = open(g, O_RDWR);
    if (rw < 0 || write(rw, "both", 4) != 4 || read(rw, buf, sizeof buf) != 4 || memcmp(buf, "both", 4) != 0)
        return 60;
    close(rw);

    /* 7x */
    int d = open(dir, O_RDONLY | O_DIRECTORY);
    if (d < 0 || mkfifoat(d, "h", 0600) != 0)
        return 70;
    char h[PATH_MAX];
    path_of(h, "h");
    if (stat(h, &st) != 0 || !S_ISFIFO(st.st_mode))
        return 70;
    close(d);

    /* 8x */
    int pf = open(file, O_RDWR | O_CREAT | O_EXCL, 0600);
    if (pf < 0 || fstat(pf, &st) != 0 || !S_ISREG(st.st_mode))
        return 80;
    close(pf);

    unlink(f);
    unlink(g);
    unlink(h);
    unlink(file);
    return 42;
}

int main(void)
{
    alarm(20);
    strcpy(dir, "/tmp/ctest-fifo-XXXXXX");
    if (!mkdtemp(dir))
        return 9;
    int rc = check();
    rmdir(dir);
    return rc;
}
