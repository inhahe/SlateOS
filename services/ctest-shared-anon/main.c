/*
 * ctest-shared-anon -- is shared anonymous memory shared with a child on
 * SlateOS?
 *
 * mmap(NULL, n, PROT_READ|PROT_WRITE, MAP_SHARED|MAP_ANONYMOUS, -1, 0) and
 * then fork() is POSIX's everyday way to share memory with a child. The C
 * library asks the kernel for it with the kernel's own MAP_SHARED
 * (posix/src/mman.rs, native_flags), which lane A's kernel honours: the
 * pages are committed and marked shared, and fork maps the same frames into
 * the child, writable on both sides. Until 2026-10-07 the library never
 * asked, and a child's writes stayed in the child's copy.
 *
 * The checks, in order; each leans on the one before:
 *
 *    1x  a three-page shared anonymous mapping and a private one are made
 *        and written
 *    2x  a child writes to both -- the shared one on its first page and its
 *        last -- and its writes to the shared one reach the parent, while
 *        those to the private one do not
 *    3x  the other way round, while both run: the parent writes after the
 *        fork and the child, polling the shared word, sees it and answers
 *        in the same memory
 *    4x  what Linux refuses: anonymous memory with MAP_SHARED_VALIDATE, and
 *        a shared mapping with MAP_GROWSDOWN, are EINVAL (its second step is
 *        43, since 42 is the pass)
 *
 * Exit codes: 42 is every check passed.
 *    2  fork or waitpid failed     3  a child failed (its step is printed)
 * Otherwise <check><step>, each failure's line printed before it returns.
 *
 * The same source compiles and passes on Linux (gcc main.c && ./a.out),
 * which is how its expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#ifndef MAP_SHARED_VALIDATE
#define MAP_SHARED_VALIDATE 0x03
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
    emit("ctest-shared-anon: FAIL ");
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
    emit("ctest-shared-anon: ok ");
    emit(what);
    emit("\n");
}

/* Milliseconds on the monotonic clock. */
static long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

/* Wait, at most five seconds, for *word to become `want`. 1 if it did. */
static int await_word(volatile long *word, long want)
{
    long deadline = now_ms() + 5000;
    while (*word != want) {
        if (now_ms() > deadline) {
            return 0;
        }
        struct timespec pause = { 0, 1000000 };
        nanosleep(&pause, NULL);
    }
    return 1;
}

/* Wait for `child` and require that it exited 0; its step if not. */
static int reap(pid_t child)
{
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        emit("ctest-shared-anon: the child failed at its step ");
        emit_num(WIFEXITED(status) ? WEXITSTATUS(status) : -1);
        emit("\n");
        return fail(3, "the child");
    }
    return 0;
}

int main(void)
{
    long page = sysconf(_SC_PAGESIZE);
    if (page <= 0) {
        return fail(11, "sysconf(_SC_PAGESIZE)");
    }
    size_t len = 3 * (size_t)page;
    size_t last = len / sizeof(long) - 1;

    /* 1: the two mappings. */
    volatile long *shared = mmap(NULL, len, PROT_READ | PROT_WRITE,
                                 MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (shared == MAP_FAILED) {
        return fail(12, "mmap(MAP_SHARED | MAP_ANONYMOUS)");
    }
    volatile long *private = mmap(NULL, (size_t)page, PROT_READ | PROT_WRITE,
                                  MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (private == MAP_FAILED) {
        return fail(13, "mmap(MAP_PRIVATE | MAP_ANONYMOUS)");
    }
    if (shared[0] != 0 || shared[last] != 0) {
        return fail(14, "a new anonymous mapping reads as zeros");
    }
    shared[0] = 1;
    shared[last] = 1;
    private[0] = 1;
    ok("a shared and a private anonymous mapping, written");

    /* 2: the child writes; the shared writes come back, the private stay. */
    pid_t child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        if (shared[0] != 1 || private[0] != 1) {
            _exit(21);  /* the child starts with the parent's contents */
        }
        shared[0] = 42;
        shared[last] = 43;
        private[0] = 44;
        _exit(0);
    }
    int code = reap(child);
    if (code != 0) {
        return code;
    }
    if (shared[0] != 42) {
        emit("ctest-shared-anon: the shared word reads ");
        emit_num(shared[0]);
        emit("\n");
        return fail(22, "the child's write to the shared mapping reaches the parent");
    }
    if (shared[last] != 43) {
        return fail(23, "...on the mapping's last page too");
    }
    if (private[0] != 1) {
        return fail(24, "the child's write to the private mapping stays in the child");
    }
    ok("a child's writes to shared memory reach the parent, and to private memory do not");

    /* 3: both running; the parent writes, the child sees it and answers. */
    shared[0] = 0;
    shared[1] = 0;
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        if (!await_word(&shared[0], 7)) {
            _exit(31);  /* the parent's write never arrived */
        }
        shared[1] = 8;
        _exit(0);
    }
    shared[0] = 7;
    int answered = await_word(&shared[1], 8);
    code = reap(child);
    if (code != 0) {
        return code;
    }
    if (!answered) {
        return fail(32, "the child's answer arrives while both run");
    }
    ok("a write after the fork reaches the running child, and its answer the parent");

    /* 4: what Linux refuses. Anonymous memory is MAP_SHARED or MAP_PRIVATE:
     * do_mmap's switch for a mapping with no file has no MAP_SHARED_VALIDATE
     * case, and a shared mapping may not grow down -- EINVAL for both,
     * measured on Linux 6.6.87. (A file takes MAP_SHARED_VALIDATE.) */
    errno = 0;
    if (mmap(NULL, (size_t)page, PROT_READ | PROT_WRITE, MAP_SHARED_VALIDATE | MAP_ANONYMOUS,
             -1, 0) != MAP_FAILED || errno != EINVAL) {
        return fail(41, "anonymous MAP_SHARED_VALIDATE is EINVAL");
    }
    errno = 0;
    if (mmap(NULL, (size_t)page, PROT_READ | PROT_WRITE,
             MAP_SHARED | MAP_ANONYMOUS | MAP_GROWSDOWN, -1, 0) != MAP_FAILED
        || errno != EINVAL) {
        /* 43, not 42: 42 is every check passed. */
        return fail(43, "anonymous MAP_SHARED | MAP_GROWSDOWN is EINVAL");
    }
    ok("anonymous MAP_SHARED_VALIDATE, and a shared mapping that grows, are EINVAL");

    munmap((void *)private, (size_t)page);
    munmap((void *)shared, len);
    emit("ctest-shared-anon: every check passed\n");
    return 42;
}
