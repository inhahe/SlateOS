/*
 * ctest-gdb-runs -- does the ported GDB 18.1 run on SlateOS?
 *
 * WHY THIS EXISTS.  scripts/gdb-spike/ builds GDB 18.1 and gdbserver against
 * this library's libc.a with nothing missing, and the rootfs recipe stages
 * them as /bin/gnu-gdb and /bin/gdbserver. Staged is not run: GDB is the
 * largest C++ program on the image, the first to start libc++'s and GMP's
 * static constructors under our libc, and nothing has executed it here.
 *
 * WHAT IT CANNOT ASK.  Whether GDB can debug a running program. That needs
 * the kernel's ptrace and /proc/<pid>/mem
 * (requests/d-a-a-debugger-needs-ptrace-for-native-programs.md); when they
 * exist, a fifth probe -- run a child under GDB to a breakpoint -- belongs
 * here.
 *
 * WHY THESE FOUR, IN THIS ORDER.  Each adds one thing to the one before, so
 * the boundary between the last pass and the first failure is the finding:
 *
 *   1. gnu-gdb --version   start up and exit: the C++ runtime's static
 *                          constructors, GDB's own initialisation, stdout to
 *                          a pipe. Nothing below is interpretable without it.
 *   2. gnu-gdb -batch -nx -ex "print 6*7"
 *                          the command interpreter and the expression
 *                          evaluator: "$1 = 42", exactly.
 *   3. gnu-gdb -batch -nx -ex "info address main" /mnt/bin/gdbserver
 *                          read another program: open an ELF file, read its
 *                          symbol table through bfd, look a symbol up. The
 *                          line is compared at both ends; the address between
 *                          them is gdbserver's, and moves with every link.
 *   4. gdbserver --version the other program: its own start-up and exit.
 *
 * THE COMPARISONS ARE EXACT, NOT SUBSTRINGS.  A probe that passed on a
 * message containing the expected words would pass on the usage message a
 * broken argv handler prints.
 *
 * BOUNDS.  Structural, as in ctest-coreutils-runs: every read is gated on
 * poll(POLLIN|POLLHUP, 0) and the wait is a counted spin with sched_yield(),
 * whose budget restarts at each byte that arrives. No alarm: a fixture's
 * bounds should not depend on a subsystem other than the one under test. The
 * budget is ten times coreutils-runs': GDB is large, and under emulation its
 * start-up is seconds of silence.
 *
 * Exit codes -- one per check:
 *    42  all four ran and answered correctly
 *     1  pipe() failed (this fixture's plumbing, not a finding about GDB)
 *     2  fork() or waitpid() failed (likewise)
 *     3  gnu-gdb --version's first line is not "GNU gdb (GDB) 18.1"
 *     4  gnu-gdb --version did not exit 0
 *     5  print 6*7 printed something other than "$1 = 42\n"
 *     6  print 6*7 did not exit 0
 *     7  info address main printed no line of the expected shape
 *     8  info address main did not exit 0
 *    10  gdbserver --version's first line is not "GNU gdbserver (GDB) 18.1"
 *    11  gdbserver --version did not exit 0
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

/* Where the staged binaries are AT RUNTIME: the image is mounted at /mnt. */
#define BIN "/mnt/bin/"

#define SPIN 40000000L
#define CAP 8192

/* What run() returns besides an exit status. */
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
    /* POLLHUP counts: a read at hangup returns 0 at once rather than
     * blocking, and that 0 is how the loop below learns of EOF. */
    return (pfd.revents & (POLLIN | POLLHUP)) != 0;
}

/* Run argv[0] with argv, capture its stdout into out (NUL-terminated),
 * reap it, and return its exit status -- or R_PLUMBING, R_SIGNAL (with the
 * signal in *sig) or R_NOEXEC. Its stderr is the fixture's, so GDB's own
 * warnings reach the serial log beside this fixture's lines. */
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
        emit_fd(2, "[gdb] execv(");
        emit_fd(2, argv[0]);
        emit_fd(2, ") failed in the child: errno ");
        emit_num(2, err);
        emit_fd(2, " (");
        emit_fd(2, strerror(err));
        emit_fd(2, ")\n");
        /* 127, the shell's "could not exec": distinct from anything GDB's
         * --version or -batch exits with. */
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

/* out's first line, without its newline, compared with want. */
static int first_line_is(const char *out, const char *want)
{
    size_t n = strlen(want);
    return strncmp(out, want, n) == 0 && (out[n] == '\n' || out[n] == '\0');
}

/* Whether some line of out begins with head and ends with tail. */
static int has_line(const char *out, const char *head, const char *tail)
{
    size_t hn = strlen(head);
    size_t tn = strlen(tail);
    const char *line = out;
    while (*line != '\0') {
        const char *end = strchr(line, '\n');
        size_t len = end != (const char *)0 ? (size_t)(end - line) : strlen(line);
        if (len >= hn + tn && strncmp(line, head, hn) == 0
            && strncmp(line + len - tn, tail, tn) == 0) {
            return 1;
        }
        if (end == (const char *)0) {
            break;
        }
        line = end + 1;
    }
    return 0;
}

/* What a probe's run() result means, when it is not an exit status: the
 * fixture's code for it, or 0 to go on and judge the exit status. */
static int plumbing(const char *what, int r, int sig)
{
    if (r == R_PLUMBING) {
        emit("[gdb] ");
        emit(what);
        emit(": pipe, fork or waitpid failed -- this fixture's plumbing\n");
        return 2;
    }
    if (r == R_SIGNAL) {
        emit("[gdb] ");
        emit(what);
        emit(": died on signal ");
        emit_num(1, sig);
        emit("\n");
        return 12;
    }
    if (r == R_NOEXEC) {
        emit("[gdb] ");
        emit(what);
        emit(": could not be executed -- the child's errno is printed above\n");
        return 13;
    }
    return 0;
}

static char out[CAP];

int main(void)
{
    int sig = 0;
    int r;
    int code;

    /* 1. Start up, print the version, exit. */
    {
        char *argv[] = {BIN "gnu-gdb", "--version", (char *)0};
        r = run(argv, out, CAP, &sig);
        if ((code = plumbing("gnu-gdb --version", r, sig)) != 0) {
            return code;
        }
        if (!first_line_is(out, "GNU gdb (GDB) 18.1")) {
            emit("[gdb] gnu-gdb --version printed, first:\n");
            emit(out);
            emit("\n");
            return 3;
        }
        if (r != 0) {
            return 4;
        }
        emit("[gdb] ok gnu-gdb --version\n");
    }

    /* 2. The command interpreter and the expression evaluator. */
    {
        char *argv[] = {BIN "gnu-gdb", "-batch", "-nx", "-ex", "print 6*7", (char *)0};
        r = run(argv, out, CAP, &sig);
        if ((code = plumbing("print 6*7", r, sig)) != 0) {
            return code;
        }
        if (strcmp(out, "$1 = 42\n") != 0) {
            emit("[gdb] print 6*7 printed:\n");
            emit(out);
            emit("\n");
            return 5;
        }
        if (r != 0) {
            return 6;
        }
        emit("[gdb] ok print 6*7\n");
    }

    /* 3. Read another program's symbol table. */
    {
        char *argv[] = {BIN "gnu-gdb", "-batch", "-nx", "-ex", "info address main",
                        BIN "gdbserver", (char *)0};
        r = run(argv, out, CAP, &sig);
        if ((code = plumbing("info address main", r, sig)) != 0) {
            return code;
        }
        if (!has_line(out, "Symbol \"main\" is at 0x",
                      " in a file compiled without debugging.")) {
            emit("[gdb] info address main printed:\n");
            emit(out);
            emit("\n");
            return 7;
        }
        if (r != 0) {
            return 8;
        }
        emit("[gdb] ok info address main\n");
    }

    /* 4. The other program. */
    {
        char *argv[] = {BIN "gdbserver", "--version", (char *)0};
        r = run(argv, out, CAP, &sig);
        if ((code = plumbing("gdbserver --version", r, sig)) != 0) {
            return code;
        }
        if (!first_line_is(out, "GNU gdbserver (GDB) 18.1")) {
            emit("[gdb] gdbserver --version printed, first:\n");
            emit(out);
            emit("\n");
            return 10;
        }
        if (r != 0) {
            return 11;
        }
        emit("[gdb] ok gdbserver --version\n");
    }

    emit("[gdb] PASS: GDB 18.1 and gdbserver run\n");
    return 42;
}
