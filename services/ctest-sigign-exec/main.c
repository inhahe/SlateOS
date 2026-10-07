/*
 * ctest-sigign-exec -- does an ignored signal stay ignored across exec on
 * SlateOS, and does an ignored SIGCHLD keep children from becoming zombies?
 *
 * POSIX keeps a signal set to SIG_IGN ignored across exec -- that is how
 * nohup keeps a command alive when its terminal goes, and how a shell keeps
 * ^C away from a background job -- while a signal with a handler goes back
 * to its default. A native SlateOS process's dispositions were the C
 * library's table alone, which exec replaced: everything came back default,
 * so the command died on hang-up after all; and an ignored SIGCHLD, which
 * should stop children becoming zombies, could not, since only the kernel
 * can (known-issues/TD-D-SIG-IGN-DOES-NOT-SURVIVE-EXEC-OR-SPAWN.md). The
 * library now reports the ignored set to the kernel, which keeps it across
 * exec (SYS_SIGNAL_SET_IGNORED, lane A's), and reads it back at start-up.
 *
 * The checks, in order:
 *
 *    1x  SIGHUP ignored and SIGUSR1 handled, then exec of this program:
 *        in the new image SIGHUP is SIG_IGN and SIGUSR1 SIG_DFL, and a
 *        SIGHUP it sends itself is survived
 *    2x  SIGCHLD set to SIG_IGN: a child that exits is reaped, and waitpid
 *        for it answers ECHILD instead of collecting a zombie
 *
 * Exit codes: 42 is every check passed.
 *    2  fork or waitpid failed     3  the exec'd image failed (its step is
 *    printed)     Otherwise <check><step>.
 *
 * Run with an argument "after-exec" (as the checks run it), it is the new
 * image of check 1. The same source compiles and passes on Linux, which is
 * how its expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* Where the image keeps this program; argv[0] is its bare name. */
#ifndef SELF
#define SELF "/mnt/tests/ctest-sigign-exec.elf"
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
    emit("ctest-sigign-exec: FAIL ");
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
    emit("ctest-sigign-exec: ok ");
    emit(what);
    emit("\n");
}

static void on_usr1(int sig)
{
    (void)sig;
}

/* The disposition of `sig` now. */
static void (*disposition(int sig))(int)
{
    struct sigaction old;
    memset(&old, 0, sizeof old);
    sigaction(sig, NULL, &old);
    return old.sa_handler;
}

/* Check 1's new image: what it came with. 0, or the failing step. */
static int after_exec(void)
{
    if (disposition(SIGHUP) != SIG_IGN) {
        return 11;  /* the ignored SIGHUP did not survive the exec */
    }
    if (disposition(SIGUSR1) != SIG_DFL) {
        return 12;  /* a handler cannot survive an exec: it must be default */
    }
    if (kill(getpid(), SIGHUP) != 0) {
        return 13;
    }
    /* Still here: the SIGHUP was ignored. Give a late delivery its chance. */
    struct timespec t = { 0, 50000000 };
    nanosleep(&t, NULL);
    return 0;
}

static const char *self_path(const char *argv0)
{
    return access(SELF, X_OK) == 0 ? SELF : argv0;
}

int main(int argc, char **argv)
{
    if (argc > 1 && strcmp(argv[1], "after-exec") == 0) {
        return after_exec();
    }

    /* 1. */
    if (signal(SIGHUP, SIG_IGN) == SIG_ERR || signal(SIGUSR1, on_usr1) == SIG_ERR) {
        return fail(14, "signal(SIGHUP, SIG_IGN) and a SIGUSR1 handler");
    }
    pid_t child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        char *args[] = { (char *)"ctest-sigign-exec", (char *)"after-exec", NULL };
        execv(self_path(argc > 0 ? argv[0] : SELF), args);
        _exit(15);  /* the exec itself failed */
    }
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    if (WIFSIGNALED(status)) {
        emit("ctest-sigign-exec: the new image died of signal ");
        emit_num(WTERMSIG(status));
        emit("\n");
        return fail(16, "the exec'd image survives the SIGHUP it was sent");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        emit("ctest-sigign-exec: the new image failed at its step ");
        emit_num(WIFEXITED(status) ? WEXITSTATUS(status) : -1);
        emit("\n");
        return fail(3, "the exec'd image");
    }
    signal(SIGHUP, SIG_DFL);
    signal(SIGUSR1, SIG_DFL);
    ok("an ignored signal stays ignored across exec, a handled one goes default");

    /* 2. */
    if (signal(SIGCHLD, SIG_IGN) == SIG_ERR) {
        return fail(21, "signal(SIGCHLD, SIG_IGN)");
    }
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        _exit(0);
    }
    errno = 0;
    pid_t got = waitpid(child, &status, 0);
    if (got != -1 || errno != ECHILD) {
        return fail(22, "with SIGCHLD ignored the child is reaped: waitpid is ECHILD");
    }
    signal(SIGCHLD, SIG_DFL);
    ok("an ignored SIGCHLD leaves no zombie to wait for");

    emit("ctest-sigign-exec: every check passed\n");
    return 42;
}
