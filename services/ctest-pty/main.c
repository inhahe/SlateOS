/*
 * ctest-pty — ring-3 regression test for **pseudo-terminals** reached through
 * *our own* libc (`AbiMode::Native`): `posix_openpt`/`openpty`/`forkpty`/
 * `login_tty` in `posix/src/pty.rs` and `posix/src/ioctl.rs`, over the kernel
 * pty family (syscalls 544-556).
 *
 * Why this fixture has to exist, in one sentence: `ctest-ctty` says of `^C`
 * that it "is untested here only because the fixture has no way to synthesise
 * a keystroke, not because it is missing" — and a pty master *is* a keystroke
 * synthesiser.
 *
 * That is the whole point of this file.  A console fixture can establish a
 * foreground process group and can assert who owns the terminal, but it cannot
 * make a key be pressed, so the line discipline's most consequential branch —
 * `VINTR` -> `SIGINT` -> a handler in the foreground group — has never been
 * executed by any test at any level.  Writing one byte, 0x03, to a pty master
 * drives exactly that path, end to end, with no keyboard and no human.
 *
 * What each layer can and cannot prove, which is why this is a ring-3 binary:
 *
 *   * The **host suite** cannot.  Every pty wrapper's syscall arm is
 *     `#[cfg(target_os = "none")]`, so on the host triple `posix/src/pty.rs`
 *     has no kernel to open a pty with.  Its five existing unit tests are all
 *     negative paths — NULL rejection and errno propagation — because the
 *     success path is not reachable there.  They prove argument handling and
 *     nothing about the wiring.
 *   * The **kernel's own self-tests** cannot.  They drive `kernel/src/tty` and
 *     `kernel/src/tty/pty.rs` directly and never pass through libc, so they
 *     prove the wiring and nothing about `openpty`'s composition of
 *     `posix_openpt`/`grantpt`/`unlockpt`/`ptsname_r`.
 *   * Only a native binary joins the two, and only *two processes* can show
 *     that a signal raised by the line discipline crosses a process boundary
 *     rather than being a static this process set for itself.
 *
 * The load-bearing checks are 40-44.  Check 43 is a child, in its own address
 * space with its own copy of libc's statics, running a `SIGINT` handler that
 * fired because its *parent* wrote 0x03 to the master end.  Nothing in
 * userspace can fake that.
 *
 * Deliberately no timeouts.  `alarm`/`setitimer` report success and arm
 * nothing (`known-issues.md` -> `B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING`), so
 * a fixture that trusted them would hang the boot test rather than fail it.
 * Every read here is non-blocking with a bounded spin, and every wait for the
 * child is bounded, so this fixture can fail but cannot hang.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the legend in the kernel rung that runs this).
 */

#include <errno.h>
#include <fcntl.h>
#include <pty.h>
#include <signal.h>
#include <string.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
#include <utmp.h>

#define PASS 42

/* Bounded spin for a non-blocking read.  Large enough that a working path
 * always wins, finite so a broken one fails instead of hanging. */
#define SPIN 2000000L

/* Read exactly `want` bytes, or give up.  Returns bytes read. */
static long read_bounded(int fd, char *buf, long want)
{
    long got = 0;
    for (long i = 0; i < SPIN && got < want; i++) {
        long n = read(fd, buf + got, (size_t)(want - got));
        if (n > 0) {
            got += n;
            i = 0; /* progress: give the next byte a full budget */
        } else if (n == 0) {
            break; /* EOF */
        } else if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) {
            break;
        }
    }
    return got;
}

/* Drain whatever is currently readable, discarding it. */
static void drain(int fd)
{
    char junk[256];
    for (int i = 0; i < 64; i++) {
        long n = read(fd, junk, sizeof junk);
        if (n <= 0) {
            break;
        }
    }
}

static volatile sig_atomic_t got_sigint = 0;

static void on_sigint(int sig)
{
    (void)sig;
    got_sigint = 1;
}

/* ---------------------------------------------------------------------- */

int main(void)
{
    int m = -1, s = -1;

    /* ---- openpty: the basics -------------------------------------- */

    if (openpty(&m, &s, (char *)0, (const struct termios *)0,
                (const struct winsize *)0) != 0) {
        return 1;
    }
    if (m < 0) {
        return 2;
    }
    if (s < 0) {
        return 3;
    }
    if (m == s) {
        return 4;
    }
    if (isatty(m) != 1) {
        return 5;
    }
    if (isatty(s) != 1) {
        return 6;
    }

    /* Both ends must name themselves.  The slave is the interesting one:
     * it is asked of the kernel rather than derived from the handle, so a
     * wrong answer here is a wiring fault and not a formatting one. */
    {
        char *sn = ttyname(s);
        if (sn == (char *)0) {
            return 7;
        }
        if (strncmp(sn, "/dev/pts/", 9) != 0) {
            return 8;
        }
    }

    /* Non-blocking from here on: see the header note on timeouts. */
    if (fcntl(m, F_SETFL, O_NONBLOCK) != 0) {
        return 9;
    }
    if (fcntl(s, F_SETFL, O_NONBLOCK) != 0) {
        return 10;
    }

    /* ---- raw mode: bytes cross intact, both directions ------------- */

    struct termios raw;
    if (tcgetattr(s, &raw) != 0) {
        return 11;
    }
    struct termios saved = raw;
    raw.c_lflag &= (tcflag_t)~(ICANON | ECHO | ISIG);
    raw.c_iflag &= (tcflag_t)~(ICRNL | INLCR);
    raw.c_oflag &= (tcflag_t)~OPOST;
    if (tcsetattr(s, TCSANOW, &raw) != 0) {
        return 12;
    }

    /* master -> slave (what a keystroke does) */
    {
        char buf[4] = {0};
        if (write(m, "abc", 3) != 3) {
            return 13;
        }
        if (read_bounded(s, buf, 3) != 3) {
            return 14;
        }
        if (memcmp(buf, "abc", 3) != 0) {
            return 15;
        }
    }

    /* slave -> master (what the program prints) */
    {
        char buf[4] = {0};
        if (write(s, "xyz", 3) != 3) {
            return 16;
        }
        if (read_bounded(m, buf, 3) != 3) {
            return 17;
        }
        if (memcmp(buf, "xyz", 3) != 0) {
            return 18;
        }
    }

    /* ---- canonical mode: a line arrives only on the newline -------- */

    drain(m);
    drain(s);
    saved.c_lflag |= (ICANON | ECHO);
    if (tcsetattr(s, TCSANOW, &saved) != 0) {
        return 20;
    }

    if (write(m, "hi", 2) != 2) {
        return 21;
    }
    {
        /* No newline yet, so canonical mode must hold the line back.  A
         * short bounded read that finds nothing is the assertion. */
        char buf[8];
        long n = 0;
        for (int i = 0; i < 2000; i++) {
            long r = read(s, buf, sizeof buf);
            if (r > 0) {
                n = r;
                break;
            }
        }
        if (n != 0) {
            return 22;
        }
    }
    if (write(m, "\n", 1) != 1) {
        return 23;
    }
    {
        char buf[8] = {0};
        if (read_bounded(s, buf, 3) != 3) {
            return 24;
        }
        if (memcmp(buf, "hi\n", 3) != 0) {
            return 25;
        }
    }

    /* ECHO is on, so the typed line came back to the master. */
    {
        char buf[8] = {0};
        if (read_bounded(m, buf, 2) != 2) {
            return 26;
        }
        if (memcmp(buf, "hi", 2) != 0) {
            return 27;
        }
    }

    drain(m);
    close(s);
    close(m);

    /* ---- the point of the fixture: a synthesised ^C ---------------- */

    int fm = -1;
    pid_t kid = forkpty(&fm, (char *)0, (const struct termios *)0,
                        (const struct winsize *)0);
    if (kid < 0) {
        return 40;
    }

    if (kid == 0) {
        /* Child: forkpty ran login_tty, so the slave is our controlling
         * terminal, we are its foreground group, and fd 0/1/2 are it. */
        if (isatty(0) != 1) {
            _exit(70);
        }
        if (signal(SIGINT, on_sigint) == SIG_ERR) {
            _exit(71);
        }
        /* Announce readiness *after* the handler is installed, so the
         * parent's 0x03 can never arrive before there is something to
         * catch it.  This is the whole synchronisation of the test. */
        if (write(1, "R", 1) != 1) {
            _exit(72);
        }
        for (long i = 0; i < SPIN; i++) {
            if (got_sigint) {
                _exit(77);
            }
        }
        _exit(78); /* handler never ran */
    }

    /* Parent drives the master end. */
    if (fcntl(fm, F_SETFL, O_NONBLOCK) != 0) {
        return 41;
    }
    {
        char r = 0;
        if (read_bounded(fm, &r, 1) != 1 || r != 'R') {
            /* 43, deliberately not 42.  This check is the first one whose
             * natural number collided with PASS, and a failure path that
             * returns the success code is the one bug a fixture must not
             * have: it would report "all checks passed" for a pty that
             * never delivered the child's readiness byte.  The rule the
             * numbering has to satisfy is only that no check is numbered
             * 42; 43-47 above it are fine. */
            return 43;
        }
    }
    if (write(fm, "\003", 1) != 1) {
        return 44;
    }

    {
        int status = 0;
        pid_t w = -1;
        for (long i = 0; i < SPIN && w <= 0; i++) {
            w = waitpid(kid, &status, WNOHANG);
        }
        if (w != kid) {
            return 45;
        }
        if (!WIFEXITED(status)) {
            return 46;
        }
        if (WEXITSTATUS(status) != 77) {
            /* 78 == the handler never ran, which is the interesting
             * failure: the line discipline did not turn 0x03 into a
             * SIGINT that crossed into the child. */
            return 47;
        }
    }

    close(fm);
    return PASS;
}
