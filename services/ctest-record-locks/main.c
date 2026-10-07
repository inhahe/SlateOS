/*
 * ctest-record-locks -- do fcntl's record locks exclude another process on
 * SlateOS?
 *
 * Record locks are how SQLite keeps two writers out of one database, how
 * login and who share the utmp file, and how lckpwdf guards /etc/passwd.
 * Until 2026-10-07 the C library granted every lock without asking the
 * kernel, so two processes could both hold one exclusive range
 * (known-issues/D-POSIX-FCNTL-RECORD-LOCKS-NEVER-REACH-THE-KERNEL.md). Now
 * F_GETLK, F_SETLK and F_SETLKW go to the kernel's lock table
 * (SYS_FS_RECORD_LOCK, lane A's), and lockf is built on them as glibc
 * builds it. This checks the library and the kernel together, with a parent
 * and a forked child that opens the file for itself:
 *
 *    1x  the parent write-locks bytes 0-99; the child's write lock on 50-149
 *        is refused (EAGAIN), its F_GETLK names the parent as the holder of
 *        0-99, and a read lock on 200-299, which nobody holds, is granted
 *    2x  the child's F_SETLKW on 50-59 waits until the parent unlocks, a
 *        fifth of a second later
 *    3x  deadlock: each holds one byte and waits for the other's; the
 *        kernel refuses the second waiter with EDEADLK rather than letting
 *        both wait forever
 *    4x  closing a dup of a descriptor releases every lock the process
 *        holds on the file (POSIX), though the dup's handle stays open
 *    5x  a lock ends with its process: the child's, when it exits
 *    6x  lockf: F_TLOCK on a section another process holds is EAGAIN (or
 *        EACCES, which POSIX also allows), and F_TEST reports it held
 *    7x  flock, whose lock is the open file description's (lane A's
 *        SYS_FS_FLOCK_HANDLE): a second open of the file in this process
 *        is another owner (EWOULDBLOCK), a dup and a forked child share
 *        the description's lock, and a child's own blocking flock waits
 *        until the parent unlocks (70 is that unlock failing)
 *
 * Exit codes: 42 is every check passed.
 *    2  open, pipe, fork or waitpid failed     3  a child failed (its step
 *    is printed)     Otherwise <check><step>.
 *
 * The same source compiles and passes on Linux (gcc main.c && ./a.out),
 * which is how its expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/file.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define PATH "/tmp/ctest-record-locks"

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
    emit("ctest-record-locks: FAIL ");
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
    emit("ctest-record-locks: ok ");
    emit(what);
    emit("\n");
}

static long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

static void sleep_ms(long ms)
{
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {
    }
}

/* fcntl `cmd` with a lock of `type` over [start, start + len). */
static int lock(int fd, int cmd, short type, off_t start, off_t len)
{
    struct flock fl;
    memset(&fl, 0, sizeof fl);
    fl.l_type = type;
    fl.l_whence = SEEK_SET;
    fl.l_start = start;
    fl.l_len = len;
    return fcntl(fd, cmd, &fl);
}

/* One byte to say "go" through a pipe, and one to wait for it. */
static int signal_go(int fd)
{
    char c = 'g';
    return write(fd, &c, 1) == 1 ? 0 : -1;
}

static int wait_go(int fd)
{
    char c;
    return read(fd, &c, 1) == 1 ? 0 : -1;
}

static int reap(pid_t child)
{
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        emit("ctest-record-locks: the child failed at its step ");
        emit_num(WIFEXITED(status) ? WEXITSTATUS(status) : -1);
        emit("\n");
        return fail(3, "a child");
    }
    return 0;
}

/* Checks 1 and 2, the child's half. `go` says the parent holds 0-99; the
 * child says on `back` when it is about to wait. */
static int child_contends(pid_t parent, int go, int back)
{
    int fd = open(PATH, O_RDWR);
    if (fd < 0 || wait_go(go) != 0) {
        return 10;
    }
    errno = 0;
    if (lock(fd, F_SETLK, F_WRLCK, 50, 100) != -1 || (errno != EAGAIN && errno != EACCES)) {
        return 11;
    }
    struct flock probe;
    memset(&probe, 0, sizeof probe);
    probe.l_type = F_WRLCK;
    probe.l_whence = SEEK_SET;
    probe.l_start = 50;
    probe.l_len = 100;
    if (fcntl(fd, F_GETLK, &probe) != 0) {
        return 12;
    }
    if (probe.l_type != F_WRLCK || probe.l_pid != parent || probe.l_start != 0
        || probe.l_len != 100) {
        return 13;
    }
    if (lock(fd, F_SETLK, F_RDLCK, 200, 100) != 0) {
        return 14;
    }
    /* 2: wait for the parent's unlock. */
    if (signal_go(back) != 0) {
        return 21;
    }
    long start = now_ms();
    if (lock(fd, F_SETLKW, F_WRLCK, 50, 10) != 0) {
        return 22;
    }
    if (now_ms() - start < 100) {
        return 23;  /* granted while the parent still held it */
    }
    close(fd);
    return 0;
}

/* Check 3, the child's half: hold byte 1, then wait for byte 0. */
static int child_deadlocks(int go, int back)
{
    int fd = open(PATH, O_RDWR);
    if (fd < 0 || lock(fd, F_SETLK, F_WRLCK, 1, 1) != 0) {
        return 30;
    }
    if (wait_go(go) != 0 || signal_go(back) != 0) {
        return 31;
    }
    /* Waits for the parent's byte 0; the parent's wait for byte 1 is the
     * one the kernel refuses, after which the parent unlocks byte 0. */
    if (lock(fd, F_SETLKW, F_WRLCK, 0, 1) != 0) {
        return 32;
    }
    close(fd);
    return 0;
}

/* Checks 4 and 6, the child's half: lock what the parent says is free. */
static int child_after_close(int go)
{
    int fd = open(PATH, O_RDWR);
    if (fd < 0 || wait_go(go) != 0) {
        return 40;
    }
    if (lock(fd, F_SETLK, F_WRLCK, 500, 1) != 0) {
        return 41;  /* the parent's close of a dup did not release it */
    }
    close(fd);
    return 0;
}

int main(void)
{
    pid_t me = getpid();
    int fd = open(PATH, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) {
        return fail(2, "open " PATH);
    }

    /* 1 and 2. */
    int go[2], back[2];
    if (pipe(go) != 0 || pipe(back) != 0) {
        return fail(2, "pipe");
    }
    if (lock(fd, F_SETLK, F_WRLCK, 0, 100) != 0) {
        return fail(15, "the parent's write lock on 0-99");
    }
    pid_t child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        _exit(child_contends(me, go[0], back[1]));
    }
    if (signal_go(go[1]) != 0 || wait_go(back[0]) != 0) {
        return fail(16, "the child's word");
    }
    sleep_ms(200);
    if (lock(fd, F_SETLK, F_UNLCK, 0, 100) != 0) {
        return fail(24, "the parent's unlock");
    }
    int code = reap(child);
    if (code != 0) {
        return code;
    }
    ok("another process's lock is refused, named by F_GETLK, and waited out by F_SETLKW");

    /* 3. */
    if (lock(fd, F_SETLK, F_WRLCK, 0, 1) != 0) {
        return fail(33, "the parent's lock on byte 0");
    }
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        _exit(child_deadlocks(go[0], back[1]));
    }
    if (signal_go(go[1]) != 0 || wait_go(back[0]) != 0) {
        return fail(34, "the child's word");
    }
    sleep_ms(200);  /* let the child go to sleep waiting for byte 0 */
    errno = 0;
    if (lock(fd, F_SETLKW, F_WRLCK, 1, 1) != -1 || errno != EDEADLK) {
        return fail(35, "waiting for the byte of a process waiting for ours is EDEADLK");
    }
    if (lock(fd, F_SETLK, F_UNLCK, 0, 1) != 0) {
        return fail(36, "the parent's unlock of byte 0");
    }
    code = reap(child);
    if (code != 0) {
        return code;
    }
    ok("a wait that could never end is refused with EDEADLK");

    /* 4 and 5. */
    if (lock(fd, F_SETLK, F_WRLCK, 500, 1) != 0) {
        /* 43, not 42: 42 is every check passed. */
        return fail(43, "the parent's lock on byte 500");
    }
    int dup_fd = dup(fd);
    if (dup_fd < 0 || close(dup_fd) != 0) {
        return fail(44, "dup and close");
    }
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        _exit(child_after_close(go[0]));
    }
    if (signal_go(go[1]) != 0) {
        return fail(45, "the child's word");
    }
    code = reap(child);
    if (code != 0) {
        return code;
    }
    ok("closing a dup released the process's lock; the child's ended with it");
    /* 5: the child took byte 500 and exited; it is free again. */
    if (lock(fd, F_SETLK, F_WRLCK, 500, 1) != 0) {
        return fail(51, "a lock ends with its process");
    }
    ok("a lock ends when its process exits");

    /* 6: lockf, against a lock the child holds. */
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        int cfd = open(PATH, O_RDWR);
        if (cfd < 0 || lseek(cfd, 1000, SEEK_SET) != 1000 || lockf(cfd, F_LOCK, 10) != 0) {
            _exit(60);
        }
        if (signal_go(back[1]) != 0 || wait_go(go[0]) != 0) {
            _exit(61);
        }
        _exit(0);
    }
    if (wait_go(back[0]) != 0 || lseek(fd, 1005, SEEK_SET) != 1005) {
        return fail(62, "the child's word, and a seek");
    }
    errno = 0;
    if (lockf(fd, F_TLOCK, 1) != -1 || (errno != EAGAIN && errno != EACCES)) {
        return fail(63, "lockf F_TLOCK on a section another process holds");
    }
    errno = 0;
    if (lockf(fd, F_TEST, 1) != -1 || errno != EACCES) {
        return fail(64, "lockf F_TEST reports it held, EACCES");
    }
    if (signal_go(go[1]) != 0) {
        return fail(65, "the child's word");
    }
    code = reap(child);
    if (code != 0) {
        return code;
    }
    if (lockf(fd, F_TEST, 1) != 0) {
        return fail(66, "lockf F_TEST finds it free once the child is gone");
    }
    ok("lockf locks a section from the offset, and F_TEST sees another's");

    /* 7: flock -- the open description's lock, not the process's. */
    int d1 = open(PATH, O_RDWR);
    int d2 = open(PATH, O_RDWR);
    if (d1 < 0 || d2 < 0) {
        return fail(2, "open " PATH " twice");
    }
    if (flock(d1, LOCK_EX) != 0) {
        return fail(71, "flock LOCK_EX");
    }
    errno = 0;
    if (flock(d2, LOCK_EX | LOCK_NB) != -1 || errno != EWOULDBLOCK) {
        return fail(72, "a second open of the file is another owner, even in this process");
    }
    int d3 = dup(d1);
    if (d3 < 0 || flock(d3, LOCK_EX | LOCK_NB) != 0) {
        return fail(73, "a dup shares the description, and its lock");
    }
    close(d3);
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        /* d1 is the parent's description, shared by the fork. */
        if (flock(d1, LOCK_EX | LOCK_NB) != 0) {
            _exit(74);
        }
        int own = open(PATH, O_RDWR);
        if (own < 0) {
            _exit(75);
        }
        errno = 0;
        if (flock(own, LOCK_EX | LOCK_NB) != -1 || errno != EWOULDBLOCK) {
            _exit(76);
        }
        if (signal_go(back[1]) != 0) {
            _exit(77);
        }
        long start = now_ms();
        if (flock(own, LOCK_EX) != 0) {
            _exit(78);
        }
        if (now_ms() - start < 100) {
            _exit(79);  /* granted while the parent's description held it */
        }
        _exit(0);
    }
    if (wait_go(back[0]) != 0) {
        return fail(77, "the child's word");
    }
    sleep_ms(200);
    if (flock(d1, LOCK_UN) != 0) {
        return fail(70, "flock LOCK_UN");
    }
    code = reap(child);
    if (code != 0) {
        return code;
    }
    close(d1);
    close(d2);
    ok("flock's lock is the open description's: shared by a dup and a fork, not by a second open");

    close(fd);
    unlink(PATH);
    emit("ctest-record-locks: every check passed\n");
    return 42;
}
