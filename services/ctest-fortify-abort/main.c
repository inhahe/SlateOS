/*
 * ctest-fortify-abort -- ring-3 test of `_FORTIFY_SOURCE`'s failure path
 * (posix/src/fortify.rs; design-decisions.md §1105; known-issues.md
 * TD-D-FORTIFY-MEM-AND-STR-CHK-IGNORE-THE-OBJECT-SIZE).
 *
 * A glibc-fortified object calls `__memcpy_chk(dst, src, n, objsize)` where
 * its source said `memcpy`, and expects the copy to be refused -- a message
 * on stderr, then abort() -- when `n` does not fit `objsize`. Until
 * 2026-09-25 this libc ignored `objsize` and copied anyway. The host tests in
 * `posix/` show every copy that FITS goes through; only a process that can
 * die shows the ones that do not, so here each overflowing call runs in a
 * forked child whose stderr is a pipe, and the parent checks how it died.
 *
 * `__read_chk` is the other half of the rule: it clamps rather than aborts,
 * because a short read is already part of read()'s contract.
 *
 * The prototypes are glibc's, written out: musl -- whose headers `zig cc`
 * uses -- has no `_FORTIFY_SOURCE`, so nothing would generate these calls.
 * Declaring them by hand is also what pins the ABI a fortified object uses.
 *
 * Exit codes -- 42 is every check passing; otherwise the first failure:
 *   10+i  in-bounds call i misbehaved: wrong result, or it touched the guard
 *         byte past its object (i: 0 memcpy, 1 memmove, 2 mempcpy, 3 memset,
 *         4 strcpy, 5 stpcpy, 6 strncpy, 7 stpncpy, 8 strcat, 9 strncat)
 *   20+i  the same call with an UNKNOWN object size ((size_t)-1) misbehaved
 *   30+i  overflowing call i: pipe or fork failed
 *   50+i  overflowing call i: the child did not exit with 134, the code
 *         abort() gives -- it returned (the check is missing) or died
 *         otherwise
 *   70+i  overflowing call i: its stderr was not glibc's
 *         "*** buffer overflow detected ***: terminated\n"
 *   90    __read_chk: pipe failed
 *   91    __read_chk did not clamp a read to its object
 *   92    __read_chk touched the guard byte past its object
 *   93    the bytes it did not read were not left in the pipe
 *   94    __fdelt_chk (FD_SET's index) gave the wrong word for fd 1023
 *   95    __fdelt_chk(1024) did not abort its child with 134
 *   96    __fdelt_chk(1024)'s stderr was not glibc's message
 *   97    the __fdelt_chk child's pipe or fork failed
 *   98/99 __explicit_bzero_chk one byte past its object: did not abort with
 *         134 / wrong message
 *   100/101 __poll_chk, two entries in a one-entry object: the same
 *   102/103 __open_2 with O_CREAT (no mode): did not abort with 134 / not
 *         glibc's "invalid open call" message
 *   104   one of the last three children's pipe or fork failed
 */

#include <fcntl.h>
#include <poll.h>
#include <stddef.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

extern void *__memcpy_chk(void *dst, const void *src, size_t n, size_t dstlen);
extern void *__memmove_chk(void *dst, const void *src, size_t n, size_t dstlen);
extern void *__mempcpy_chk(void *dst, const void *src, size_t n, size_t dstlen);
extern void *__memset_chk(void *dst, int c, size_t n, size_t dstlen);
extern char *__strcpy_chk(char *dst, const char *src, size_t dstlen);
extern char *__stpcpy_chk(char *dst, const char *src, size_t dstlen);
extern char *__strncpy_chk(char *dst, const char *src, size_t n, size_t dstlen);
extern char *__stpncpy_chk(char *dst, const char *src, size_t n, size_t dstlen);
extern char *__strcat_chk(char *dst, const char *src, size_t dstlen);
extern char *__strncat_chk(char *dst, const char *src, size_t n, size_t dstlen);
extern ssize_t __read_chk(int fd, void *buf, size_t n, size_t buflen);
extern long __fdelt_chk(long d);
extern void __explicit_bzero_chk(void *dst, size_t len, size_t dstlen);
extern int __poll_chk(struct pollfd *fds, nfds_t nfds, int timeout, size_t fdslen);
extern int __open_2(const char *path, int oflag);

#define OBJ 8            /* every destination object is 8 bytes... */
#define GUARD 0x5a       /* ...followed by one guard byte */
#define UNKNOWN ((size_t)-1)
#define NCALLS 10

static const char MESSAGE[] = "*** buffer overflow detected ***: terminated\n";
static const char OPEN_MESSAGE[] =
    "*** invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated\n";

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

/* Call i on `buf` (an OBJ-byte object plus a guard) with `n` bytes' worth of
 * operation and object size `objsize`. "n" means bytes for the memory calls
 * and a string of n-1 characters plus its terminator for the string copies,
 * so n == OBJ exactly fills the object and n == OBJ + 1 overflows it by one. */
static void call(int i, char *buf, size_t n, size_t objsize)
{
    static const char src[] = "ABCDEFGHIJKLMNOP";
    char str[OBJ + 2];
    size_t len = n - 1; /* the string's length without its terminator */
    memcpy(str, src, len);
    str[len] = '\0';

    switch (i) {
    case 0: __memcpy_chk(buf, src, n, objsize); break;
    case 1: __memmove_chk(buf, src, n, objsize); break;
    case 2: __mempcpy_chk(buf, src, n, objsize); break;
    case 3: __memset_chk(buf, 'z', n, objsize); break;
    case 4: __strcpy_chk(buf, str, objsize); break;
    case 5: __stpcpy_chk(buf, str, objsize); break;
    case 6: __strncpy_chk(buf, str, n, objsize); break;
    case 7: __stpncpy_chk(buf, str, n, objsize); break;
    case 8:
        /* "A" already there, then the n-2 characters of str after its first,
         * then the terminator: n bytes in all. */
        buf[0] = 'A';
        buf[1] = '\0';
        __strcat_chk(buf, str + 1, objsize);
        break;
    case 9:
        buf[0] = 'A';
        buf[1] = '\0';
        __strncat_chk(buf, src + 1, n - 2, objsize);
        break;
    }
}

/* An in-bounds call must work and leave the guard alone. */
static int in_bounds(int i, size_t objsize)
{
    char buf[OBJ + 1];
    memset(buf, 0, OBJ);
    buf[OBJ] = (char)GUARD;
    call(i, buf, OBJ, objsize);
    if ((unsigned char)buf[OBJ] != GUARD) {
        return 0;
    }
    /* Every call wrote at least its first byte. */
    return buf[0] != '\0';
}

/* Read everything the child wrote to `fd` into `out` (at most cap-1 bytes),
 * NUL-terminated. The child has exited, so this ends at EOF. */
static size_t drain(int fd, char *out, size_t cap)
{
    size_t got = 0;
    while (got + 1 < cap) {
        ssize_t r = read(fd, out + got, cap - 1 - got);
        if (r <= 0) {
            break;
        }
        got += (size_t)r;
    }
    out[got] = '\0';
    return got;
}

/* What a child that was meant to abort did: 0 if it exited with 134 having
 * written exactly `expect` to stderr, 1 if it ended any other way, 2 if it
 * aborted but said something else, 3 if the pipe or fork failed. Case i >= 0
 * is `call(i, ...)` overflowing by one byte; the negative cases are the calls
 * that are not copies (see the switch). */
static int child_aborts(int i, const char *expect)
{
    int fds[2];
    if (pipe(fds) != 0) {
        return 3;
    }
    pid_t pid = fork();
    if (pid < 0) {
        return 3;
    }
    if (pid == 0) {
        close(fds[0]);
        dup2(fds[1], 2);
        char buf[OBJ + 1];
        memset(buf, 0, sizeof buf);
        struct pollfd pfds[2];
        memset(pfds, 0, sizeof pfds);
        switch (i) {
        case -1:
            (void)__fdelt_chk(1024);
            break;
        case -2:
            /* A wipe one byte longer than its object. */
            __explicit_bzero_chk(buf, OBJ + 1, OBJ);
            break;
        case -3:
            /* Two entries claimed, one entry's worth of object. */
            (void)__poll_chk(pfds, 2, 0, sizeof pfds[0]);
            break;
        case -4:
            /* O_CREAT through the two-argument form: no mode to create with. */
            (void)__open_2("/tmp/fortify-abort-never-created", O_CREAT | O_WRONLY);
            break;
        default:
            call(i, buf, OBJ + 1, OBJ);
            break;
        }
        /* Reached only if the check is missing. */
        _exit(0);
    }
    close(fds[1]);
    int status = 0;
    pid_t got = waitpid(pid, &status, 0);
    char err[256];
    drain(fds[0], err, sizeof err);
    close(fds[0]);
    if (got != pid || !WIFEXITED(status) || WEXITSTATUS(status) != 134) {
        return 1;
    }
    if (strcmp(err, expect) != 0) {
        return 2;
    }
    return 0;
}

/* An overflowing call must abort the child, having said so on stderr. */
static int overflow_dies(int i)
{
    switch (child_aborts(i, MESSAGE)) {
    case 0:
        return 0;
    case 1:
        return 50 + i;
    case 2:
        return 70 + i;
    default:
        return 30 + i;
    }
}

/* FD_SET's index for a descriptor inside the fd_set, and an abort for one
 * outside it: glibc's FD_SETSIZE, 1024, is the structure's size in bits. */
static int fdelt_checks(void)
{
    if (__fdelt_chk(1023) != 15) {
        return 94;
    }
    switch (child_aborts(-1, MESSAGE)) {
    case 0:
        return 0;
    case 1:
        return 95;
    case 2:
        return 96;
    default:
        return 97;
    }
}

/* The calls that abort without being copies: a wipe, a poll and an open. Each
 * gets two codes, (base) did not abort with 134 and (base + 1) wrong message. */
static int other_aborts(void)
{
    static const struct {
        int which;
        const char *expect;
        int base;
    } cases[] = {
        {-2, MESSAGE, 98},       /* __explicit_bzero_chk */
        {-3, MESSAGE, 100},      /* __poll_chk */
        {-4, OPEN_MESSAGE, 102}, /* __open_2 with O_CREAT */
    };
    for (size_t k = 0; k < sizeof cases / sizeof cases[0]; k++) {
        int rc = child_aborts(cases[k].which, cases[k].expect);
        if (rc == 1) {
            return cases[k].base;
        }
        if (rc == 2) {
            return cases[k].base + 1;
        }
        if (rc != 0) {
            return 104;
        }
    }
    return 0;
}

static int read_clamps(void)
{
    int fds[2];
    if (pipe(fds) != 0) {
        return 90;
    }
    ssize_t w = write(fds[1], "0123456789", 10);
    (void)w;
    close(fds[1]);
    char buf[5];
    buf[4] = (char)GUARD;
    /* Ask for 10 into a 4-byte object: 4 arrive, the guard survives. */
    ssize_t r = __read_chk(fds[0], buf, 10, 4);
    if (r != 4 || memcmp(buf, "0123", 4) != 0) {
        close(fds[0]);
        return 91;
    }
    if ((unsigned char)buf[4] != GUARD) {
        close(fds[0]);
        return 92;
    }
    char rest[16];
    size_t n = drain(fds[0], rest, sizeof rest);
    close(fds[0]);
    if (n != 6 || memcmp(rest, "456789", 6) != 0) {
        return 93;
    }
    return 0;
}

int main(void)
{
    emit("[fz] in-bounds copies, known and unknown object size\n");
    for (int i = 0; i < NCALLS; i++) {
        if (!in_bounds(i, OBJ)) {
            return 10 + i;
        }
        if (!in_bounds(i, UNKNOWN)) {
            return 20 + i;
        }
    }
    emit("[fz] overflowing copies (each child must abort)\n");
    for (int i = 0; i < NCALLS; i++) {
        int rc = overflow_dies(i);
        if (rc != 0) {
            return rc;
        }
    }
    emit("[fz] __read_chk clamps\n");
    int rc = read_clamps();
    if (rc != 0) {
        return rc;
    }
    emit("[fz] __fdelt_chk (FD_SET) indexes, and refuses fd 1024\n");
    rc = fdelt_checks();
    if (rc != 0) {
        return rc;
    }
    emit("[fz] a wipe, a poll and a two-argument O_CREAT open (each child must abort)\n");
    rc = other_aborts();
    if (rc != 0) {
        return rc;
    }
    emit("[fz] ok (10 copies, FD_SET, a wipe, a poll and an open refused; read clamped)\n");
    return 42;
}
