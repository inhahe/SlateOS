/*
 * ctest-cloexec-exec -- does a native exec close the close-on-exec
 * descriptors it leaves behind, at the exec?
 *
 * A native process's descriptor table is the C library's. Before an exec the
 * library hands the kernel the descriptors to keep (SYS_PROCESS_SET_EXEC_FDS)
 * and, since 2026-10-07, the handles of the close-on-exec ones left behind
 * (SYS_PROCESS_SET_EXEC_CLOSE, lane A's), which the kernel closes once the
 * new image is in. Before that they stayed open until the new program
 * exited: a close-on-exec pipe's reader saw end-of-file only then, which is
 * what std::process::Command's fork path waits on to learn the exec
 * happened -- so it, an sshd session's shell and the GUI terminal's first
 * spawn hung until the child exited
 * (requests/b-ad-close-on-exec-does-not-close-on-a-native-exec.md).
 *
 * The checks, in order:
 *
 *    1x  a child execs this program again, told to sleep two seconds,
 *        holding a close-on-exec pipe's write end: the parent's read of the
 *        other end ends -- end of file -- well before the sleeper does
 *    2x  a child whose exec FAILS keeps its close-on-exec descriptors, as
 *        POSIX requires: it writes the exec's errno into the same kind of
 *        pipe, and the parent reads it -- exactly std's protocol
 *    3x  a close-on-exec descriptor dup'd onto one that is not (dup2 onto a
 *        kept descriptor) is not closed by the exec: the sleeper inherits
 *        the pipe's write end as its standard output, so the parent sees no
 *        end of file until the sleeper exits
 *
 * Exit codes: 42 is every check passed.
 *    2  pipe, fork or waitpid failed     3  a child ended other than 0
 * Otherwise <check><step>.
 *
 * Run with an argument "sleep" (as the checks run it), it sleeps two seconds
 * and exits 0. The same source compiles and passes on Linux, which is how its
 * expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* Where the image keeps this program; argv[0] is its bare name. */
#ifndef SELF
#define SELF "/mnt/tests/ctest-cloexec-exec.elf"
#endif
#define SLEEP_MS 2000L
/* End of file within this, while the sleeper sleeps for SLEEP_MS. */
#define PROMPT_MS 1000L

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
    emit("ctest-cloexec-exec: FAIL ");
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
    emit("ctest-cloexec-exec: ok ");
    emit(what);
    emit("\n");
}

static long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

/* The path to exec: this program, where the image keeps it -- or, run by
 * hand from elsewhere, where argv[0] says. */
static const char *self_path(const char *argv0)
{
    if (access(SELF, X_OK) == 0) {
        return SELF;
    }
    return argv0;
}

static int reap(pid_t child)
{
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        emit("ctest-cloexec-exec: a child ended with ");
        emit_num(WIFEXITED(status) ? WEXITSTATUS(status) : -1);
        emit("\n");
        return fail(3, "a child");
    }
    return 0;
}

/* Read p until end of file; the milliseconds it took, or -1 on error. */
static long read_to_eof(int fd)
{
    long start = now_ms();
    char buf[16];
    for (;;) {
        ssize_t n = read(fd, buf, sizeof buf);
        if (n == 0) {
            return now_ms() - start;
        }
        if (n < 0 && errno != EINTR) {
            return -1;
        }
    }
}

int main(int argc, char **argv)
{
    if (argc > 1 && strcmp(argv[1], "sleep") == 0) {
        struct timespec t = { SLEEP_MS / 1000, (SLEEP_MS % 1000) * 1000000L };
        while (nanosleep(&t, &t) != 0 && errno == EINTR) {
        }
        return 0;
    }
    const char *self = self_path(argc > 0 ? argv[0] : SELF);

    /* 1: the exec closes the write end, so the read ends at once. */
    int p[2];
    if (pipe2(p, O_CLOEXEC) != 0) {
        return fail(2, "pipe2(O_CLOEXEC)");
    }
    pid_t child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        close(p[0]);
        char *args[] = { (char *)"ctest-cloexec-exec", (char *)"sleep", NULL };
        execv(self, args);
        _exit(11);  /* the exec itself failed */
    }
    close(p[1]);
    long ms = read_to_eof(p[0]);
    close(p[0]);
    int code = reap(child);
    if (code != 0) {
        return code;
    }
    if (ms < 0) {
        return fail(12, "reading the pipe");
    }
    if (ms > PROMPT_MS) {
        emit("ctest-cloexec-exec: end of file after ");
        emit_num(ms);
        emit(" ms\n");
        return fail(13, "the exec closed the close-on-exec write end, not the sleeper's exit");
    }
    ok("a close-on-exec pipe end is closed at the exec, not when the new program exits");

    /* 2: a failed exec leaves it open -- std's protocol. */
    if (pipe2(p, O_CLOEXEC) != 0) {
        return fail(2, "pipe2(O_CLOEXEC)");
    }
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        close(p[0]);
        char *args[] = { (char *)"nothing", NULL };
        execv("/nonexistent/ctest-cloexec-exec-nothing", args);
        int e = errno;
        if (write(p[1], &e, sizeof e) != (ssize_t)sizeof e) {
            _exit(21);
        }
        _exit(0);
    }
    close(p[1]);
    int got = 0;
    ssize_t n = read(p[0], &got, sizeof got);
    close(p[0]);
    code = reap(child);
    if (code != 0) {
        return code;
    }
    if (n != (ssize_t)sizeof got || got != ENOENT) {
        return fail(22, "a failed exec keeps its close-on-exec descriptors: its errno arrives");
    }
    ok("a failed exec keeps its close-on-exec descriptors, as POSIX requires");

    /* 3: a dup onto a kept descriptor keeps the pipe open across the exec. */
    if (pipe2(p, O_CLOEXEC) != 0) {
        return fail(2, "pipe2(O_CLOEXEC)");
    }
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        close(p[0]);
        /* dup2 clears FD_CLOEXEC on the new descriptor: standard output
         * now holds the write end, and the exec must not close it. */
        if (dup2(p[1], 1) != 1) {
            _exit(31);
        }
        char *args[] = { (char *)"ctest-cloexec-exec", (char *)"sleep", NULL };
        execv(self, args);
        _exit(32);
    }
    close(p[1]);
    ms = read_to_eof(p[0]);
    close(p[0]);
    code = reap(child);
    if (code != 0) {
        return code;
    }
    if (ms < 0) {
        return fail(33, "reading the pipe");
    }
    if (ms < SLEEP_MS / 2) {
        emit("ctest-cloexec-exec: end of file after ");
        emit_num(ms);
        emit(" ms\n");
        return fail(34, "the exec closed a handle its standard output still held");
    }
    ok("a handle a kept descriptor shares stays open across the exec");

    emit("ctest-cloexec-exec: every check passed\n");
    return 42;
}
