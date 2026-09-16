/*
 * ctest-python-repl — CPython's INTERACTIVE path, on a real pty, in ring 3.
 *
 * WHY THIS EXISTS.  `roadmap.md` has said for over a week that the CPython
 * initiative's remaining work is one sentence: "Nobody has ever run it
 * interactively. That is the actual state, and no measurement here replaces
 * doing so."  Everything around that line argues the path *should* work —
 * `isatty` is real, the pty line discipline handles `ISIG`, `sshd` already
 * hosts a login shell on a pty, and `self_test_cpython_on_slateos_libc`
 * proves the interpreter starts and produces byte-exact output.  None of it
 * says the REPL does.
 *
 * This is that measurement, automated so it can be repeated: a pty pair, an
 * interpreter on the slave end, an expression typed into the master, and the
 * answer read back out.
 *
 * WHAT IT COVERS THAT THE EXISTING RUNG DOES NOT.  `self_test_cpython_*` runs
 * `python3 -c` with stdout to a pipe.  That exercises startup, the import of
 * `encodings` out of the 20 MiB zip, and the write path — and nothing about a
 * terminal.  Between them and here lie: `isatty` answering true on the slave,
 * CPython choosing its interactive branch because of it, the line discipline
 * assembling a typed line and delivering it on ENTER, and the prompt/echo
 * traffic coming back the other way.  Those are exactly the parts a `-c` run
 * cannot reach, and exactly the parts a user meets first.
 *
 * WHY `-q -i -u`.
 *   `-q`  no version banner, so the output this scans is the answer rather
 *         than a build string that happens to contain digits.
 *   `-i`  force interactive even if the tty detection is what is broken.  That
 *         is deliberate: if `isatty` is wrong, this fixture should fail on the
 *         REPL rather than silently degrade to batch mode and pass.
 *   `-u`  unbuffered, so an answer cannot sit in a stdio buffer while this
 *         spins to its budget and calls it a timeout.
 *
 * THE EXPRESSION IS `6*7`, NOT `1+1`.  A pty echoes what is typed, so the
 * master sees the expression before it sees the answer.  With `print(1+1)`
 * the answer `2` also appears inside the echoed text, so a scan for it
 * matches the echo and passes without the interpreter having done anything.
 * `6*7` does not contain `42` — the only way `42` appears is if something
 * evaluated it.  That is the whole reason for the odd-looking choice, and it
 * is the same class of error as a test that reads back its own writes.
 *
 * BOUNDS.  Structural, and copied from `ctest-pty` because they are proven
 * there: every read is gated on `poll(POLLIN|POLLHUP, 0)`, and the wait is a
 * counted spin with `sched_yield()` per iteration.  Deliberately no `alarm` —
 * the timers work now, but a fixture's bounds should not depend on a subsystem
 * other than the one under test, or a timer regression surfaces here as a
 * CPython failure and points at the wrong place.
 *
 * The startup budget is separate and much larger than the steady-state one.
 * Starting this interpreter means mapping an 11 MiB binary and reading
 * `encodings` out of a 20 MiB archive off ext4; the gap before the FIRST byte
 * is nothing like the gap between two bytes of one line, and a single budget
 * sized for either is wrong for the other.
 *
 * Exit codes — one per check, because a failure that cannot say which half
 * failed is most of the investigation missing:
 *    42  the interpreter evaluated the expression and returned the answer
 *     1  forkpty() failed
 *     2  writing the expression to the master failed
 *     3  the interpreter produced NO output at all — it never started
 *     4  it produced output, but the answer never appeared
 *     5  waitpid() failed or returned the wrong pid
 *     6  the interpreter exited non-zero after being asked to quit
 *     7  writing the quit command failed
 */

#include <errno.h>
#include <poll.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

/* No <pty.h> in the sysroot header set; the symbol is ours, from
 * `posix/src/pty.rs`. A wrong prototype is a link error here rather than a
 * surprise at runtime, which is part of what this exercises. */
extern int forkpty(int *amaster, char *name, const void *termp, const void *winp);

/* `sched_yield()` with no <sched.h> in the sysroot. `ctest-pty` explains the
 * reasoning at length: `poll(..., 0)` must not block and is therefore not a
 * yield primitive, so a spin built only on it can hold a CPU the writer needs. */
extern int sched_yield(void);

/* Steady state: the gap between two bytes of one line. */
#define SPIN 2000000L
/* Startup: an 11 MiB binary and a 20 MiB zip, before the first byte. */
#define STARTUP_SPIN 40000000L

#define CAP 8192

static void emit(const char *s)
{
    size_t n = 0;
    while (s[n] != '\0') {
        n++;
    }
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

/* Is `fd` readable right now? Zero timeout, so this never waits.
 *
 * POLLHUP counts: a read at hangup returns 0 immediately rather than blocking,
 * and treating it as "not ready" would spin to the budget and then report a
 * timeout for what is really an EOF. */
static int readable(int fd)
{
    struct pollfd pfd;
    pfd.fd = fd;
    pfd.events = POLLIN;
    pfd.revents = 0;
    if (poll(&pfd, 1, 0) <= 0) {
        return 0;
    }
    return (pfd.revents & (POLLIN | POLLHUP)) != 0;
}

/* Accumulate from `fd` until `needle` appears in what has been read.
 *
 * Returns 1 on a match, 0 on the budget running out or EOF. `*total` is left
 * holding how many bytes ever arrived, so the caller can tell "nothing ever
 * came back" (the interpreter did not start) from "plenty came back and none
 * of it was the answer" — two very different findings that a single boolean
 * would flatten into one.
 *
 * The budget resets on progress, so it bounds the gap between bytes rather
 * than the whole exchange. A slow-but-advancing interpreter is not a hang. */
static int scan_for(int fd, const char *needle, char *buf, long budget, long *total)
{
    long got = *total;
    size_t nlen = strlen(needle);
    for (long i = 0; i < budget; i++) {
        if (got >= CAP - 1) {
            break; /* buffer full: whatever this is, it is not our one line */
        }
        if (!readable(fd)) {
            sched_yield();
            continue;
        }
        ssize_t n = read(fd, buf + got, (size_t)(CAP - 1 - got));
        if (n > 0) {
            got += (long)n;
            buf[got] = '\0';
            *total = got;
            if (nlen != 0 && strstr(buf, needle) != (char *)0) {
                return 1;
            }
            i = 0; /* progress: give the next byte a full budget */
        } else if (n == 0) {
            break; /* EOF */
        } else if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) {
            break;
        }
    }
    *total = got;
    return 0;
}

static int write_all(int fd, const char *s)
{
    size_t len = strlen(s);
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, s + off, len - off);
        if (n > 0) {
            off += (size_t)n;
        } else if (n < 0 && errno == EINTR) {
            continue;
        } else {
            return -1;
        }
    }
    return 0;
}

int main(void)
{
    int master = -1;
    char buf[CAP];
    long total = 0;

    buf[0] = '\0';

    emit("[py] fork (interpreter on the slave end of a pty)\n");
    pid_t child = forkpty(&master, (char *)0, (const void *)0, (const void *)0);
    if (child < 0) {
        return 1;
    }
    if (child == 0) {
        /* forkpty ran login_tty, so the slave is our controlling terminal and
         * is already fds 0/1/2. Nothing to wire up. */
        execl("/bin/python3", "python3", "-q", "-i", "-u", (char *)0);
        /* Only reached if exec failed. 127 is the shell's convention for it,
         * and is distinct from every code this fixture returns, so it cannot
         * be misread as one of our checks failing. */
        _exit(127);
    }

    /* Typed before waiting for a prompt, deliberately. The line discipline
     * buffers it whether or not the interpreter has finished starting, so this
     * removes a round trip and, with it, a reason to guess at how long a
     * prompt takes to appear. */
    emit("[py] type (6*7, whose answer is not in the echo)\n");
    if (write_all(master, "print(6*7)\n") != 0) {
        return 2;
    }

    emit("[py] read (the answer must come from an evaluation)\n");
    if (!scan_for(master, "42", buf, STARTUP_SPIN, &total)) {
        /* Nothing at all means the interpreter never started -- a loader or
         * staging fault. Output without the answer means it started and the
         * REPL did not evaluate, which is this fixture's subject. */
        return (total == 0) ? 3 : 4;
    }

    emit("[py] quit\n");
    if (write_all(master, "exit()\n") != 0) {
        return 7;
    }

    int status = 0;
    pid_t reaped = waitpid(child, &status, 0);
    if (reaped != child) {
        return 5;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        return 6;
    }

    emit("[py] ok (the REPL evaluated and answered)\n");
    return 42;
}
