/*
 * ctest-positional-io -- do pread and pwrite leave a file's position alone
 * on SlateOS, even while another thread moves it?
 *
 * pread and pwrite read and write at an explicit offset without reading or
 * moving the descriptor's position, so threads sharing one descriptor can
 * each work at their own offsets. Until 2026-10-07 the C library could only
 * imitate that: seek, transfer, seek back -- three calls, any of which
 * another thread's seek or read could come between, so a pread could read
 * from the wrong place. Lane A's SYS_FS_PREAD and SYS_FS_PWRITE do it in one
 * call, and the library uses them (posix/src/file.rs, positional_io).
 *
 * The checks, in order:
 *
 *    1x  pwrite at offset 4096 and pread there: the bytes come back, and the
 *        position is where an ordinary write left it before and after
 *    2x  pwritev and preadv of three buffers at offset 8192: the pieces land
 *        in order, and the position is still untouched
 *    3x  one thread seeks the descriptor to 0 and reads a byte, over and
 *        over, while another preads 16 bytes at 4096 ROUNDS times: every
 *        pread returns those 16 bytes
 *
 * Exit codes: 42 is every check passed.
 *    2  open or pthread_create failed     Otherwise <check><step>.
 *
 * The same source compiles and passes on Linux (gcc -pthread main.c &&
 * ./a.out), which is how its expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdatomic.h>
#include <string.h>
#include <sys/uio.h>
#include <unistd.h>

#define PATH "/tmp/ctest-positional-io"
#define ROUNDS 20000

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
    emit("ctest-positional-io: FAIL ");
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
    emit("ctest-positional-io: ok ");
    emit(what);
    emit("\n");
}

/* Sixteen bytes, and the terminator, which is not written. */
static const char AT4096[] = "sixteen at 4096!";
#define AT_LEN 16
static int shared_fd;
static atomic_int stop;
static atomic_long seeks;

/* Check 3's mover: the position, to 0 and on a byte, again and again. */
static void *mover(void *unused)
{
    (void)unused;
    char c;
    while (!atomic_load(&stop)) {
        if (lseek(shared_fd, 0, SEEK_SET) == 0 && read(shared_fd, &c, 1) == 1) {
            atomic_fetch_add(&seeks, 1);
        }
    }
    return NULL;
}

int main(void)
{
    int fd = open(PATH, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) {
        return fail(2, "open " PATH);
    }

    /* 1. */
    if (write(fd, "abcde", 5) != 5 || lseek(fd, 0, SEEK_CUR) != 5) {
        return fail(11, "an ordinary write moves the position to 5");
    }
    if (pwrite(fd, AT4096, AT_LEN, 4096) != AT_LEN) {
        return fail(12, "pwrite at 4096");
    }
    if (lseek(fd, 0, SEEK_CUR) != 5) {
        return fail(13, "pwrite left the position at 5");
    }
    char got[AT_LEN];
    if (pread(fd, got, AT_LEN, 4096) != AT_LEN || memcmp(got, AT4096, AT_LEN) != 0) {
        return fail(14, "pread at 4096 gives the bytes back");
    }
    if (lseek(fd, 0, SEEK_CUR) != 5) {
        return fail(15, "pread left the position at 5");
    }
    ok("pread and pwrite leave the position where it was");

    /* 2. */
    char p1[] = "one-", p2[] = "two-", p3[] = "three";
    struct iovec out[3] = { { p1, 4 }, { p2, 4 }, { p3, 5 } };
    if (pwritev(fd, out, 3, 8192) != 13 || lseek(fd, 0, SEEK_CUR) != 5) {
        return fail(21, "pwritev of three pieces at 8192, the position untouched");
    }
    char q1[5], q2[3], q3[5];
    struct iovec in[3] = { { q1, 5 }, { q2, 3 }, { q3, 5 } };
    if (preadv(fd, in, 3, 8192) != 13 || lseek(fd, 0, SEEK_CUR) != 5) {
        return fail(22, "preadv into three pieces at 8192, the position untouched");
    }
    if (memcmp(q1, "one-t", 5) != 0 || memcmp(q2, "wo-", 3) != 0 || memcmp(q3, "three", 5) != 0) {
        return fail(23, "the pieces land in order");
    }
    ok("pwritev and preadv of three pieces land in order");

    /* 3. */
    shared_fd = fd;
    pthread_t t;
    if (pthread_create(&t, NULL, mover, NULL) != 0) {
        return fail(2, "pthread_create");
    }
    long wrong = 0, rounds = 0;
    for (; rounds < ROUNDS; rounds++) {
        if (pread(fd, got, AT_LEN, 4096) != AT_LEN || memcmp(got, AT4096, AT_LEN) != 0) {
            wrong++;
        }
    }
    atomic_store(&stop, 1);
    pthread_join(t, NULL);
    if (wrong != 0) {
        emit("ctest-positional-io: ");
        emit_num(wrong);
        emit(" of ");
        emit_num(rounds);
        emit(" preads read the wrong bytes, with ");
        emit_num(atomic_load(&seeks));
        emit(" seeks beside them\n");
        return fail(31, "every pread at 4096 reads its 16 bytes while the position moves");
    }
    ok("a pread is not disturbed by another thread's seeks");

    close(fd);
    unlink(PATH);
    emit("ctest-positional-io: every check passed\n");
    return 42;
}
