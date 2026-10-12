/*
 * ctest-mono-runs -- does the ported Mono runtime run a .NET program on SlateOS?
 *
 * WHY THIS EXISTS.  scripts/mono-spike/ links Mono 6.14.1's runtime against
 * this library's libc.a with nothing missing, and the rootfs recipe stages it
 * as /bin/mono with mscorlib.dll at /lib/mono/4.5. Staged is not run. Mono is
 * a JIT compiler: it writes machine code into memory it then executes, turns
 * a hardware fault into a .NET exception from inside a signal handler, and
 * stops its threads for a garbage collection. None of that has been seen to
 * work here.
 *
 * WHAT IT RUNS.  Two probes, the second the one that matters:
 *
 *   1. mono --version      the runtime starts and exits: its own
 *                          initialisation, nothing JIT-compiled yet.
 *   2. mono checks.exe     services/ctest-mono-runs/checks.cs, compiled by
 *                          scripts/mono-spike/bcl.sh: eight checks, each
 *                          adding one thing to the one before -- a program
 *                          started, arithmetic and strings through the JIT, a
 *                          managed exception caught, a null dereference
 *                          caught as NullReferenceException (SIGSEGV, and a
 *                          signal context the runtime rewrites), a division
 *                          by zero caught (SIGFPE), a garbage collection, a
 *                          second thread, and C called by name through
 *                          DllImport("libc") -- dlopen of libc.so.6 and dlsym
 *                          over the symbols mono exports (design-decisions
 *                          1184). One line each, two for the last; the
 *                          output is compared whole, so where it stops says
 *                          which.
 *
 * Mono finds mscorlib beside itself (/mnt/bin/mono looks in /mnt/lib/mono/
 * 4.5), so no environment is set: the run is the one a user's would be.
 *
 * BOUNDS.  As ctest-gdb-runs': poll-gated reads and a counted spin whose
 * budget restarts at each byte. No alarm.
 *
 * Exit codes:
 *    42  both ran and answered correctly
 *     2  pipe, fork or waitpid failed (this fixture's plumbing)
 *     3  mono --version's first line does not begin "Mono JIT compiler version 6.14.1"
 *     4  mono --version did not exit 0
 *     5  checks.exe printed something other than its ten lines
 *     6  checks.exe did not exit 0
 *    12  a probe died on a signal (its number is printed)
 *    13  a probe could not be executed (the child printed its errno)
 */
#include <errno.h>
#include <poll.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

/* `sched_yield()` with no <sched.h> in the sysroot; see `ctest-pty`. */
extern int sched_yield(void);

/* Where the staged files are AT RUNTIME: the image is mounted at /mnt. */
#define ROOT "/mnt"

#define SPIN 40000000L
#define CAP 8192

#define R_PLUMBING (-1)
#define R_SIGNAL (-2)
#define R_NOEXEC (-3)

static void emit_fd(int fd, const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(fd, s, n);
        (void)written;
    }
}

static void emit(const char *s)
{
    emit_fd(1, s);
}

static void emit_num(int fd, int v)
{
    char digits[12];
    int n = 0;
    unsigned int u = v < 0 ? 0u : (unsigned int)v;
    do {
        digits[n++] = (char)('0' + (int)(u % 10u));
        u /= 10u;
    } while (u != 0u && n < (int)sizeof digits);
    char num[13];
    int k = 0;
    while (n > 0) {
        num[k++] = digits[--n];
    }
    num[k] = '\0';
    emit_fd(fd, num);
}

static int readable(int fd)
{
    struct pollfd pfd;
    pfd.fd = fd;
    pfd.events = POLLIN;
    pfd.revents = 0;
    if (poll(&pfd, 1, 0) <= 0) {
        return 0;
    }
    /* POLLHUP counts: a read at hangup returns 0 at once, which is how the
     * loop below learns of EOF. */
    return (pfd.revents & (POLLIN | POLLHUP)) != 0;
}

/* Run argv[0] with argv, capture its stdout into out (NUL-terminated), reap
 * it, and return its exit status -- or R_PLUMBING, R_SIGNAL (the signal in
 * *sig) or R_NOEXEC. Its stderr is the fixture's, so the runtime's own
 * messages reach the serial log beside this fixture's lines. */
static int run(char *const argv[], char *out, int cap, int *sig)
{
    int fds[2];
    if (pipe(fds) != 0) {
        return R_PLUMBING;
    }
    pid_t child = fork();
    if (child < 0) {
        (void)close(fds[0]);
        (void)close(fds[1]);
        return R_PLUMBING;
    }
    if (child == 0) {
        (void)close(fds[0]);
        if (fds[1] != 1) {
            (void)dup2(fds[1], 1);
            (void)close(fds[1]);
        }
        execv(argv[0], argv);
        int err = errno;
        emit_fd(2, "[mono] execv(");
        emit_fd(2, argv[0]);
        emit_fd(2, ") failed in the child: errno ");
        emit_num(2, err);
        emit_fd(2, " (");
        emit_fd(2, strerror(err));
        emit_fd(2, ")\n");
        _exit(127);
    }

    (void)close(fds[1]);
    int got = 0;
    for (long i = 0; i < SPIN; i++) {
        if (got >= cap - 1) {
            break;
        }
        if (!readable(fds[0])) {
            sched_yield();
            continue;
        }
        ssize_t n = read(fds[0], out + got, (size_t)(cap - 1 - got));
        if (n > 0) {
            got += (int)n;
            i = 0; /* progress: give the next byte a full budget */
        } else if (n == 0) {
            break; /* EOF: the child closed its end */
        } else if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) {
            break;
        }
    }
    out[got] = '\0';
    (void)close(fds[0]);

    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return R_PLUMBING;
    }
    if (WIFSIGNALED(status)) {
        *sig = WTERMSIG(status);
        return R_SIGNAL;
    }
    if (!WIFEXITED(status)) {
        return R_PLUMBING;
    }
    if (WEXITSTATUS(status) == 127) {
        return R_NOEXEC;
    }
    return WEXITSTATUS(status);
}

static int plumbing(const char *what, int r, int sig)
{
    if (r == R_PLUMBING) {
        emit("[mono] ");
        emit(what);
        emit(": pipe, fork or waitpid failed -- this fixture's plumbing\n");
        return 2;
    }
    if (r == R_SIGNAL) {
        emit("[mono] ");
        emit(what);
        emit(": died on signal ");
        emit_num(1, sig);
        emit("\n");
        return 12;
    }
    if (r == R_NOEXEC) {
        emit("[mono] ");
        emit(what);
        emit(": could not be executed -- the child's errno is printed above\n");
        return 13;
    }
    return 0;
}

/* checks.cs's output on Linux's Mono 6.14.1 (scripts/mono-spike/bcl.sh runs
 * it there first and prints it): the fixture asks for exactly this. */
static const char EXPECTED[] =
    "mono: started\n"
    "mono: sum 5050 ABCD\n"
    "mono: caught thrown\n"
    "mono: caught a null reference\n"
    "mono: caught a division by zero\n"
    "mono: collected, kept intact\n"
    "mono: thread joined, 100000\n"
    "mono: libc strlen 5\n"
    "mono: libc getpid positive\n"
    "mono: done\n";

static char out[CAP];

int main(void)
{
    int sig = 0;
    int r;
    int code;

    /* 1. The runtime starts and exits. */
    {
        char *argv[] = {ROOT "/bin/mono", "--version", (char *)0};
        r = run(argv, out, CAP, &sig);
        if ((code = plumbing("mono --version", r, sig)) != 0) {
            return code;
        }
        const char *want = "Mono JIT compiler version 6.14.1";
        if (strncmp(out, want, strlen(want)) != 0) {
            emit("[mono] mono --version printed, first:\n");
            emit(out);
            emit("\n");
            return 3;
        }
        if (r != 0) {
            return 4;
        }
        emit("[mono] ok mono --version\n");
    }

    /* 2. A .NET program, JIT-compiled and run. */
    {
        char *argv[] = {ROOT "/bin/mono", ROOT "/lib/mono/checks/checks.exe", (char *)0};
        r = run(argv, out, CAP, &sig);
        if ((code = plumbing("mono checks.exe", r, sig)) != 0) {
            emit("[mono] it had printed:\n");
            emit(out);
            emit("\n");
            return code;
        }
        if (strcmp(out, EXPECTED) != 0) {
            emit("[mono] checks.exe printed:\n");
            emit(out);
            emit("\n");
            return 5;
        }
        if (r != 0) {
            return 6;
        }
        emit("[mono] ok checks.exe\n");
    }

    emit("[mono] PASS: Mono 6.14.1 runs a .NET program\n");
    return 42;
}
