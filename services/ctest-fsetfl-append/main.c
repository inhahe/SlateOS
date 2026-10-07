/*
 * ctest-fsetfl-append -- is O_APPEND turned on with fcntl(F_SETFL) an
 * atomic append on SlateOS?
 *
 * A file opened with O_APPEND has always had each write land at the end in
 * one step. One that turns it on later, with fcntl(F_SETFL), got an
 * imitation until 2026-10-07: the C library sought to the end and then
 * wrote, two calls another process's append could come between, so two
 * writers lost lines (known-issues/D-POSIX-APPEND-SET-BY-FCNTL-IS-A-SEEK-
 * THEN-A-WRITE.md). F_SETFL now gives the flag to the kernel's open
 * description (SYS_FS_SET_STATUS_FLAGS, lane A's), which appends atomically.
 *
 * The checks, in order:
 *
 *    1x  F_SETFL O_APPEND on a descriptor opened without it: F_GETFL says
 *        so, and a write lands at the end though the position was 0
 *    2x  F_SETFL without it again: a write lands at the position
 *    3x  a dup shares the description: after F_SETFL on one, F_GETFL on
 *        the other says O_APPEND, and a write through it appends
 *    4x  two processes, each opening the file without O_APPEND and turning
 *        it on with F_SETFL, each write LINES lines as fast as they can:
 *        every line of both is in the file, whole, none overwritten
 *
 * Exit codes: 42 is every check passed.
 *    2  open, fork or waitpid failed     3  a child failed (its step is
 *    printed)     Otherwise <check><step>.
 *
 * The same source compiles and passes on Linux (gcc main.c && ./a.out),
 * which is how its expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

#define PATH "/tmp/ctest-fsetfl-append"
#define LINES 2000
#define LINE_LEN 16

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
    emit("ctest-fsetfl-append: FAIL ");
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
    emit("ctest-fsetfl-append: ok ");
    emit(what);
    emit("\n");
}

/* The file's size, or -1. */
static long size_of(int fd)
{
    struct stat st;
    return fstat(fd, &st) == 0 ? (long)st.st_size : -1L;
}

/* Line i of writer w: "W 00042 ........\n", exactly LINE_LEN bytes. */
static void make_line(char *line, char w, int i)
{
    memset(line, '.', LINE_LEN);
    line[0] = w;
    line[1] = ' ';
    for (int d = 6; d >= 2; d--) {
        line[d] = (char)('0' + i % 10);
        i /= 10;
    }
    line[LINE_LEN - 1] = '\n';
}

/* One writer of check 4: its own open, F_SETFL O_APPEND, LINES writes. */
static int writer(char w)
{
    int fd = open(PATH, O_WRONLY);
    if (fd < 0) {
        return 40;
    }
    int fl = fcntl(fd, F_GETFL);
    if (fl < 0 || fcntl(fd, F_SETFL, fl | O_APPEND) != 0) {
        return 41;
    }
    char line[LINE_LEN];
    for (int i = 0; i < LINES; i++) {
        make_line(line, w, i);
        if (write(fd, line, LINE_LEN) != LINE_LEN) {
            return 43;  /* not 42, which is every check passed */
        }
    }
    close(fd);
    return 0;
}

int main(void)
{
    /* 1. */
    int fd = open(PATH, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) {
        return fail(2, "open " PATH);
    }
    if (write(fd, "0123456789", 10) != 10 || lseek(fd, 0, SEEK_SET) != 0) {
        return fail(11, "write ten bytes and seek back to 0");
    }
    int fl = fcntl(fd, F_GETFL);
    if (fl < 0 || (fl & O_APPEND) != 0) {
        return fail(12, "F_GETFL before: no O_APPEND");
    }
    if (fcntl(fd, F_SETFL, fl | O_APPEND) != 0 || (fcntl(fd, F_GETFL) & O_APPEND) == 0) {
        return fail(13, "F_SETFL O_APPEND, and F_GETFL says so");
    }
    if (write(fd, "AB", 2) != 2 || size_of(fd) != 12) {
        return fail(14, "a write lands at the end, not at the position (0)");
    }
    char buf[16];
    memset(buf, 0, sizeof buf);
    if (pread(fd, buf, 12, 0) != 12 || memcmp(buf, "0123456789AB", 12) != 0) {
        return fail(15, "what is in the file");
    }
    ok("F_SETFL O_APPEND makes a write land at the end");

    /* 2. */
    if (fcntl(fd, F_SETFL, fl & ~O_APPEND) != 0 || lseek(fd, 2, SEEK_SET) != 2) {
        return fail(21, "F_SETFL without O_APPEND, and a seek to 2");
    }
    if (write(fd, "x", 1) != 1 || size_of(fd) != 12 || pread(fd, buf, 12, 0) != 12
        || memcmp(buf, "01x3456789AB", 12) != 0) {
        return fail(22, "a write lands at the position again");
    }
    ok("clearing it, a write lands at the position again");

    /* 3. */
    int d = dup(fd);
    if (d < 0 || fcntl(fd, F_SETFL, fl | O_APPEND) != 0 || lseek(d, 0, SEEK_SET) != 0) {
        return fail(31, "dup, F_SETFL on the original, seek the dup to 0");
    }
    if ((fcntl(d, F_GETFL) & O_APPEND) == 0) {
        return fail(32, "the dup shares the description: F_GETFL on it says O_APPEND");
    }
    if (write(d, "CD", 2) != 2 || size_of(fd) != 14) {
        return fail(33, "the dup shares the description: its write appends too");
    }
    close(d);
    ok("a dup shares the description's append flag");

    /* 4. */
    if (ftruncate(fd, 0) != 0) {
        return fail(44, "ftruncate");
    }
    pid_t a = fork();
    if (a < 0) {
        return fail(2, "fork");
    }
    if (a == 0) {
        _exit(writer('A'));
    }
    pid_t b = fork();
    if (b < 0) {
        return fail(2, "fork");
    }
    if (b == 0) {
        _exit(writer('B'));
    }
    for (int k = 0; k < 2; k++) {
        int status = 0;
        pid_t who = waitpid(k == 0 ? a : b, &status, 0);
        if (who < 0) {
            return fail(2, "waitpid");
        }
        if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
            emit("ctest-fsetfl-append: a writer failed at its step ");
            emit_num(WIFEXITED(status) ? WEXITSTATUS(status) : -1);
            emit("\n");
            return fail(3, "a writer");
        }
    }
    long want = 2L * LINES * LINE_LEN;
    long have = size_of(fd);
    if (have != want) {
        emit("ctest-fsetfl-append: the file is ");
        emit_num(have);
        emit(" bytes, not ");
        emit_num(want);
        emit("\n");
        return fail(45, "every line of both writers is in the file");
    }
    /* Each line whole, and each writer's lines all there, in its order. */
    int next[2] = { 0, 0 };
    char line[LINE_LEN], expect[LINE_LEN];
    for (long off = 0; off < want; off += LINE_LEN) {
        if (pread(fd, line, LINE_LEN, off) != LINE_LEN) {
            return fail(46, "reading the file back");
        }
        int w = line[0] == 'A' ? 0 : line[0] == 'B' ? 1 : -1;
        if (w < 0 || next[w] >= LINES) {
            return fail(47, "a line no writer wrote");
        }
        make_line(expect, line[0], next[w]);
        if (memcmp(line, expect, LINE_LEN) != 0) {
            return fail(48, "a line overwritten or out of its writer's order");
        }
        next[w]++;
    }
    if (next[0] != LINES || next[1] != LINES) {
        return fail(49, "each writer's lines are all there");
    }
    ok("two processes appending through F_SETFL lose no line");

    close(fd);
    unlink(PATH);
    emit("ctest-fsetfl-append: every check passed\n");
    return 42;
}
