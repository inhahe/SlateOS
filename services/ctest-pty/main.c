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
 *
 * Two independent bounds, because the first version of this file had only one
 * and it did not hold.  It set `O_NONBLOCK` and trusted it, and hung the boot
 * test for two hours at check 14: `O_NONBLOCK` is a flag in libc's descriptor
 * table that each read arm has to *consult*, and the pty-slave arm dispatched
 * the handle-less `SYS_TTY_READ`, which resolves `current_tty()` -- the
 * console -- and so had no descriptor whose flags it could honour.  The flag
 * was set, read by nobody, and dropped (`TD-A-CTEST-PTY-HANGS-BOOT`).  That
 * root cause is fixed (872/873, lane B 2026-09-09).
 *
 * So now: `O_NONBLOCK` is set **and** every read is gated on
 * `poll(POLLIN, 0)`.
 *
 * The exact strength of the poll gate, stated precisely because the last
 * confident sentence in this comment was wrong.  `pty::readable()` answers
 * *are there bytes* -- `pending > 0 || !input.is_empty() || master_gone()` --
 * which is exact for a master and for a raw-mode slave, and an **upper bound**
 * for a canonical slave, where the bytes have yet to go through the line
 * editor (the kernel says so itself on `SYS_PTY_READABLE_BYTES`).  So:
 *
 *   * master, raw slave -- poll alone bounds the read; `O_NONBLOCK` is belt.
 *   * canonical slave with an incomplete line -- poll says readable and a
 *     blocking read would still park.  Only `O_NONBLOCK` bounds that one.
 *
 * Every canonical read here is issued after the newline has been written, so
 * a complete line is already queued and neither bound is load-bearing; the
 * pair is what makes that true by construction rather than by luck.
 *
 * Writes are not gated, by inspection rather than by omission: every write
 * here is 1-3 bytes into a freshly created pty whose ring is empty, so none
 * can fill it.  `O_NONBLOCK` is still set, so a surprise there fails rather
 * than parks.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the legend in the kernel rung that runs this).
 */

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
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

/* `sched_yield()` with no <sched.h> in the sysroot.
 *
 * Every spin below calls this once per iteration, and the reason is exact:
 * `readable()` uses `poll(&pfd, 1, 0)`, and this kernel answers a zero-timeout
 * poll immediately without parking -- correct per POSIX, since poll with
 * timeout 0 must not block, and therefore NOT a yield primitive. A spin built
 * on it holds its quantum until the timer preempts.
 *
 * Before this, exit 44 was avoided by timing rather than by construction: the
 * O_NONBLOCK+poll change made each iteration enter the kernel, which under TCG
 * is slow enough that the parent always won. That argument does not survive a
 * faster poll path, KVM instead of TCG, or a smaller SPIN.
 *
 * `sched_yield()` does yield here, checked in the kernel rather than assumed:
 * posix issues SYS_SLEEP(0), and the handler reads
 *   `if duration_ns == 0 { sched::yield_now(); }`. */
extern int sched_yield(void);

/* Is `fd` readable right now?  Zero timeout, so this never waits. */
static int readable(int fd)
{
    struct pollfd pfd;
    pfd.fd = fd;
    pfd.events = POLLIN;
    pfd.revents = 0;
    if (poll(&pfd, 1, 0) <= 0) {
        return 0;
    }
    /* POLLHUP counts: a read at hangup returns 0 immediately rather than
     * blocking, and treating it as "not ready" would spin to the budget and
     * then report a timeout for what is really an EOF. */
    return (pfd.revents & (POLLIN | POLLHUP)) != 0;
}

/* Read exactly `want` bytes, or give up.  Returns bytes read.
 *
 * A read is issued only once `readable()` says bytes are present, which makes
 * this bounded without relying on `O_NONBLOCK` for a master or a raw-mode
 * slave.  For a canonical slave with an incomplete line the gate is an upper
 * bound and `O_NONBLOCK` is what bounds the read -- see the header.  Both are
 * in place; neither is trusted alone. */
static long read_bounded(int fd, char *buf, long want)
{
    long got = 0;
    for (long i = 0; i < SPIN && got < want; i++) {
        if (!readable(fd)) {
            /* The writer is the other side of this pty and cannot run while we
             * hold the CPU. Yielding is what makes this a wait rather than a
             * race we usually win. */
            sched_yield();
            continue;
        }
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
        if (!readable(fd)) {
            break;
        }
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

/* The child's own verdict, if it has one, else `alive`.
 *
 * WHY THE EARLY CHECKS NEED THIS. Lane A's run died at startup: the child went
 * zombie BEFORE the parent wrote 0x03, so the write failed against a slave
 * whose last reader was gone and the fixture returned 44 -- "the parent could
 * not write to the master". True, and useless. A failed master write with a
 * DEAD child says nothing about the master; a failed master write with a LIVE
 * child is a real pty finding. Those were one number.
 *
 * So before reporting its own failure, the parent asks the child. If the child
 * has exited, the child's status is the answer, under the same legend the
 * handler check uses -- the fault is identical and only the place the parent
 * noticed it differs. If the child is still running, the caller's own code
 * stands, and now it means what it says.
 *
 * This is the split of 47 applied one check earlier, which is what lane A
 * asked for after watching 44 and 47 trade places when the yield changed the
 * timing without changing the cause. Neither number was ever about the pty.
 */
static int child_verdict(pid_t kid, int alive)
{
    int status = 0;
    for (long i = 0; i < SPIN; i++) {
        pid_t w = waitpid(kid, &status, WNOHANG);
        if (w == kid) {
            if (!WIFEXITED(status)) {
                return 46;
            }
            switch (WEXITSTATUS(status)) {
            case 70:
                return 48;
            case 71:
                return 49;
            case 72:
                return 50;
            case 78:
                return 47;
            case 77:
                /* The handler ran, so the ^C WAS delivered -- and the parent
                 * still failed. Nothing about the child explains that, so the
                 * parent's own code stands. */
                return alive;
            default:
                return 51;
            }
        }
        if (w < 0) {
            return alive; /* cannot reap it; there is nothing to add */
        }
        sched_yield();
    }
    return alive; /* still running: the caller's fault is the real one */
}

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
        /* No newline yet, so canonical mode must hold the line back.
         *
         * Asked as "does a read return nothing?" and deliberately NOT as
         * "does poll say not-readable".  Poll would be the wrong instrument:
         * `pty::readable()` reports byte *presence*, and on a canonical slave
         * the two bytes of "hi" are present while the line is not complete --
         * so poll says readable here and is right to.  A poll-based version
         * of this check failed the fixture with a spurious 22 that read as
         * "canonical mode leaked a partial line" when it meant "poll is an
         * upper bound".  The read is what the line editor actually gates.
         *
         * This read cannot park: the fd carries `O_NONBLOCK` and the slave
         * arm dispatches `SYS_PTY_SLAVE_TRY_READ`, so an incomplete line is
         * `EAGAIN` rather than a wait. */
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
            /* The signal is delivered by the kernel on our behalf, but the
             * process that raises it needs the CPU to get there. */
            sched_yield();
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
            return child_verdict(kid, 43);
        }
    }
    if (write(fm, "\003", 1) != 1) {
        /* 44 NOW MEANS WHAT IT SAYS: the master write failed with the
         * child still ALIVE. If the child is already gone, its status is
         * the real verdict and 44 would be the pty reporting on a corpse. */
        return child_verdict(kid, 44);
    }

    {
        int status = 0;
        pid_t w = -1;
        for (long i = 0; i < SPIN && w <= 0; i++) {
            w = waitpid(kid, &status, WNOHANG);
            if (w <= 0) {
                /* WNOHANG means the child must run to exit, and it cannot
                 * while this loop owns the quantum. */
                sched_yield();
            }
        }
        if (w != kid) {
            return 45;
        }
        if (!WIFEXITED(status)) {
            return 46;
        }
        /* ONE CODE PER CHILD STATUS.  This returned 47 for every status
         * that was not 77, which collapsed a fault in the kernel's signal
         * delivery with three faults in this fixture's own setup -- and
         * the exit code is the only thing anybody can read here, since
         * the child's status dies with the child and a ring-3 process's
         * exit code is not logged.  So 47 meant "the ^C did not become a
         * SIGINT" and also "isatty said no" and also "signal() refused"
         * and also "the readiness byte did not go out", and no run could
         * tell which, or even which lane owned it.
         *
         * The fixture's own failures are 48-50 and belong to whoever
         * changes this file.  47 keeps its documented meaning and now has
         * only that meaning.
         *
         *   47  child exited 78: the handler never ran.  The line
         *       discipline did not turn 0x03 into a SIGINT that crossed
         *       into the child.  THE INTERESTING ONE.
         *   48  child exited 70: its fd 0 was not a tty, so login_tty did
         *       not give it a controlling terminal.  Nothing about the
         *       signal path was exercised.
         *   49  child exited 71: signal() refused SIGINT.
         *   50  child exited 72: the child could not write its readiness
         *       byte, so the parent's ^C was never synchronised.
         *   51  any other status: the child died in a way it did not
         *       choose, since every path it takes ends in one of the
         *       above.  That is a crash or a startup failure before main,
         *       not a pty result. */
        switch (WEXITSTATUS(status)) {
        case 77:
            break; /* the handler ran; this is the pass */
        case 78:
            return 47;
        case 70:
            return 48;
        case 71:
            return 49;
        case 72:
            return 50;
        default:
            return 51;
        }
    }

    close(fm);
    return PASS;
}
