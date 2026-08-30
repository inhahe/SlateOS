/*
 * ctest-ctty — ring-3 regression test for the **controlling terminal**, the
 * **foreground process group** and the **terminal modes** reached through
 * *our own* libc (`AbiMode::Native`), not through the Linux ABI shim.
 *
 * Why this fixture has to exist.  Until 2026-08-12 the foreground process
 * group was not one value but three: `posix` kept a per-process `FG_PGRP`
 * static, `kernel/src/tty.rs` kept a `FOREGROUND_PGID` atomic (the value
 * `^C`/`^\` actually signalled, written only by the Linux shim's
 * `TIOCSPGRP`), and the native ABI had nothing at all.  A shell and the job
 * it foregrounded therefore held two independent copies of "who owns the
 * terminal" and could never contradict each other out loud.  That is
 * invisible from a host `cargo test`, because the syscall arm of every one
 * of these wrappers is `#[cfg(target_os = "none")]` and on the host triple
 * the posix crate answers from a per-thread test double.  It is equally
 * invisible from the kernel's own self-tests, which call `pcb` directly and
 * never go through libc.  Only a native-ABI binary in ring 3 joins the two.
 * See design-decisions.md §113.
 *
 * The load-bearing assertion is check 55: the parent hands the terminal to
 * its child's group with `tcsetpgrp`, and the *child* — a different process,
 * with its own address space and its own copy of libc's statics — reads the
 * new foreground group back with `tcgetpgrp`.  A userspace static can never
 * satisfy that, so a pass there proves the state is genuinely shared rather
 * than merely self-consistent.  Check 52 is its mirror image and matters
 * just as much: *before* the handoff the child must read the *parent's*
 * group, not its own, which is what makes "the child is in the background"
 * a fact about the session rather than a guess.
 *
 * Starting conditions.  The kernel spawns us with parent 0, so `pcb::create`
 * makes us our own session and group leader (pgid == sid == pid), and no
 * session holds the console.  Every expectation below follows from that,
 * which is why they can be exact rather than "some plausible value".
 *
 * Terminal-access job control.  A background process that touches the
 * terminal is now gated (`SIGTTIN` on a read, `SIGTTOU` on `tcsetattr`/
 * `tcsetpgrp`), so checks 72-84 below run in the one window where this
 * fixture is genuinely in the background — after it has handed the terminal
 * to its child's group.  It cannot be *stopped* there: the kernel spawns us
 * with parent 0, so our process group is orphaned, and POSIX substitutes
 * `EIO` for a stop that nothing could ever undo.  That is the reason 72-73
 * assert `EIO` rather than the stop a real shell's job would take.
 *
 * What is deliberately NOT here.  `Ctrl-Z` — `tty::feed` turns `VSUSP` into
 * `SIGTSTP` exactly as it turns `VINTR` into `SIGINT`, and it reaches the
 * foreground group this fixture establishes.  It is untested here only
 * because the fixture has no way to synthesise a keystroke, not because it
 * is missing.
 * The fixture also never lets a hangup reach itself — `TIOCNOTTY` is issued
 * only while the foreground group is the *reaped child's* (an empty group,
 * so no signal is sent at all), which is both the realistic case (a shell
 * whose job just exited) and the deterministic one.
 *
 * Cleanup is the kernel's, on purpose.  The fixture exits still holding the
 * terminal; `pcb::destroy` releases it when the session empties, and
 * `self_test_cctty` asserts exactly that from the other side.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `FAIL`/`return` values below and the legend in
 * kernel/src/proc/spawn.rs::self_test_cctty).
 */

#include <errno.h>
#include <signal.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>

/* A pgid no live process will ever hold. */
#define NO_GROUP 7654321

/* One handshake round: tell the child to sample, and read what it saw. */
static int round_trip(int cmd_w, int res_r, pid_t *out)
{
    char go = 'g';
    if (write(cmd_w, &go, 1) != 1) {
        return -1;
    }
    char buf[sizeof(pid_t)];
    size_t got = 0;
    while (got < sizeof(buf)) {
        ssize_t n = read(res_r, buf + got, sizeof(buf) - got);
        if (n <= 0) {
            return -1;
        }
        got += (size_t)n;
    }
    __builtin_memcpy(out, buf, sizeof(*out));
    return 0;
}

/* The child half: sample tcgetpgrp() once per byte the parent sends, and
 * report each answer verbatim.  It asserts nothing itself — a child's
 * failure could not be distinguished from a scheduling artefact, so all
 * judgement stays in the parent. */
static void child_main(int cmd_r, int res_w)
{
    for (;;) {
        char go;
        ssize_t n = read(cmd_r, &go, 1);
        if (n <= 0) {
            break; /* parent closed the command pipe: we are done */
        }
        pid_t fg = tcgetpgrp(0);
        char buf[sizeof(pid_t)];
        __builtin_memcpy(buf, &fg, sizeof(buf));
        size_t sent = 0;
        while (sent < sizeof(buf)) {
            ssize_t w = write(res_w, buf + sent, sizeof(buf) - sent);
            if (w <= 0) {
                _exit(0);
            }
            sent += (size_t)w;
        }
    }
    _exit(0);
}

int main(void)
{
    const pid_t me = getpid();

    /* ---------------------------------------------------------------- *
     * 10s — starting conditions, and the fact that a session with no
     * terminal says so.  `ENOTTY` here is not a stub's excuse: it is the
     * kernel reporting that our session holds no console.  Before this
     * work `tcgetpgrp` could not fail at all, because it read a static
     * that was always populated.
     * ---------------------------------------------------------------- */
    if (me <= 0)                        return 10;
    if (getsid(0) != me)                return 11;   /* session leader */
    if (getpgrp() != me)                return 12;   /* and group leader */

    errno = 0;
    if (tcgetpgrp(0) != -1)             return 13;
    if (errno != ENOTTY)                return 14;

    errno = 0;
    if (tcsetpgrp(0, me) != -1)         return 15;
    if (errno != ENOTTY)                return 16;

    /* A bad fd is diagnosed before the missing terminal — the fd is the
     * argument, the terminal is the state, and Linux's prologue checks the
     * argument first. */
    errno = 0;
    if (tcgetpgrp(-1) != -1)            return 17;
    if (errno != EBADF)                 return 18;

    /* Likewise a bad pgrp: EINVAL beats ENOTTY, so a caller that passes 0
     * learns its argument is wrong rather than being told it has no
     * terminal (which would be true but useless). */
    errno = 0;
    if (tcsetpgrp(0, 0) != -1)          return 19;
    if (errno != EINVAL)                return 20;

    /* ---------------------------------------------------------------- *
     * 20s — acquiring the terminal.  This used to be a silent no-op in
     * our libc, which was harmless only while `tcgetpgrp` could never
     * fail; the moment ENOTTY became reachable, "accepted and ignored"
     * became a lie that would strand every job-control shell.
     * ---------------------------------------------------------------- */
    if (ioctl(0, TIOCSCTTY, 0) != 0)    return 21;
    /* Acquiring seeds the foreground group with the acquirer's own group,
     * which is what makes the shell foreground immediately after setsid()
     * + TIOCSCTTY without an explicit tcsetpgrp(). */
    if (tcgetpgrp(0) != me)             return 22;

    /* Naming our own group again is a no-op that must still succeed. */
    if (tcsetpgrp(0, me) != 0)          return 23;
    if (tcgetpgrp(0) != me)             return 24;

    /* A group no live process holds cannot be foregrounded: the kernel
     * checks membership, so this is EPERM and not a silently accepted id
     * that would send `^C` to nobody. */
    errno = 0;
    if (tcsetpgrp(0, NO_GROUP) != -1)   return 25;
    if (errno != EPERM)                 return 26;
    if (tcgetpgrp(0) != me)             return 27;   /* and it did not move */

    /* A redundant TIOCSCTTY by the session that already owns the terminal
     * succeeds and changes nothing.  Programs issue it defensively at
     * startup; if it reset the foreground group, a shell would yank the
     * terminal back from a job it had just foregrounded. */
    if (ioctl(0, TIOCSCTTY, 0) != 0)    return 28;
    if (tcgetpgrp(0) != me)             return 29;

    /* ---------------------------------------------------------------- *
     * 50s — the real thing: the parent hands the terminal to its child's
     * group and the *child* observes it.  Two processes agreeing about
     * which group is in the foreground is precisely what three unrelated
     * copies of the value could never do.
     * ---------------------------------------------------------------- */
    int cmd[2];     /* parent -> child: "sample now" */
    int res[2];     /* child -> parent: what tcgetpgrp() said */
    if (pipe(cmd) != 0)                 return 50;
    if (pipe(res) != 0) {
        close(cmd[0]);
        close(cmd[1]);
        return 51;
    }

    const pid_t child = fork();
    if (child < 0) {
        close(cmd[0]);
        close(cmd[1]);
        close(res[0]);
        close(res[1]);
        return 52;
    }
    if (child == 0) {
        close(cmd[1]);
        close(res[0]);
        child_main(cmd[0], res[1]);
        /* unreachable */
    }

    close(cmd[0]);
    close(res[1]);

    /* From here on every failure must release the child before returning,
     * so the exit code the kernel sees is ours and not a timeout. */
    int rc = 42;
    pid_t seen = 0;

    /* Put the child in its own group, exactly as a shell does when it
     * starts a job.  The child is not a session leader, so it may move. */
    if (setpgid(child, child) != 0)             { rc = 53; goto done; }
    if (getpgid(child) != child)                { rc = 54; goto done; }

    /* Round 1 — before the handoff.  The child must read the *parent's*
     * group, not its own: the foreground group belongs to the session, and
     * the child is in the background of it.  A per-process static would
     * have reported the child's own group here (it inherits libc's copy
     * across fork), so this check fails loudly under the old design. */
    if (round_trip(cmd[1], res[0], &seen) != 0)  { rc = 55; goto done; }
    if (seen != me)                              { rc = 56; goto done; }

    /* Hand the terminal to the job. */
    if (tcsetpgrp(0, child) != 0)                { rc = 57; goto done; }
    if (tcgetpgrp(0) != child)                   { rc = 58; goto done; }

    /* Round 2 — THE assertion.  The child sees the handoff its parent
     * performed, in a different address space, through a different copy of
     * libc. */
    if (round_trip(cmd[1], res[0], &seen) != 0)  { rc = 59; goto done; }
    if (seen != child)                           { rc = 60; goto done; }

    /* The handoff moved only the terminal: our own group and the session
     * are untouched, and so is the child's session. */
    if (getpgrp() != me)                         { rc = 61; goto done; }
    if (getsid(child) != me)                     { rc = 62; goto done; }

    /* A group in *another* session could never be named, but we cannot
     * build one here (we lead this session and cannot leave it), so the
     * closest reachable statement is that a dead group cannot be either —
     * checked at 25-26 above and again after the reap below. */

done:
    /* Releasing the command pipe is what lets the child's read() return 0. */
    close(cmd[1]);
    close(res[0]);
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return rc == 42 ? 70 : rc;
    }
    if (rc != 42) {
        return rc;
    }

    /* ---------------------------------------------------------------- *
     * 70s — the terminal outlives the job, and giving it up is real.
     * The foreground group is still the child's, but the child is reaped,
     * so that group is now empty: `TIOCNOTTY`'s hangup reaches nobody and
     * cannot disturb this process.  That is deliberate — see the header.
     * ---------------------------------------------------------------- */
    if (tcgetpgrp(0) != child)          return 71;   /* still points at it */

    /* Terminal-access job control fires *before* any of that.  Our group is
     * not the foreground group (it is still the reaped child's), so this is
     * a background `tcsetpgrp` and POSIX says the caller is sent `SIGTTOU`.
     * We cannot be stopped, though: the kernel spawned us with parent 0, so
     * no process outside our group but inside our session survives to
     * `SIGCONT` us — the group is orphaned, and an orphan gets `EIO` instead
     * of a permanent stop.  Reading the foreground group is never gated, as
     * check 71 just showed: a shell whose job died must be able to see the
     * stale group in order to take the terminal back. */
    errno = 0;
    if (tcsetpgrp(0, child) != -1)      return 72;
    if (errno != EIO)                   return 73;

    /* Block `SIGTTOU` and try again.  This is not a test contrivance: it is
     * what a job-control shell must do around every `tcsetpgrp` that takes
     * the terminal back, because at that moment the shell is by definition
     * in the background.  A Linux-ABI shell would use `SIG_IGN` (bash does);
     * on our native ABI the kernel cannot see a userspace disposition, only
     * the blocked mask it owns itself, so blocking is the form that works
     * for both — see `todo.txt`.  A blocked `SIGTTOU` is undeliverable, so
     * the gate lets the write through rather than raising a signal that
     * would never arrive; only a *read* (`SIGTTIN`) is refused in that case,
     * because letting it through would hand a background job the keystrokes
     * meant for the foreground one. */
    sigset_t ttou, saved;
    if (sigemptyset(&ttou) != 0)        return 81;
    if (sigaddset(&ttou, SIGTTOU) != 0) return 82;
    if (sigprocmask(SIG_BLOCK, &ttou, &saved) != 0) return 83;
    errno = 0;
    const int retook = tcsetpgrp(0, child);
    const int retook_errno = errno;
    if (sigprocmask(SIG_SETMASK, &saved, 0) != 0)   return 84;

    /* And now the check this pair was always about, finally reachable: the
     * foreground group names live membership, not just a number, so the
     * reaped child's empty group cannot be foregrounded. */
    if (retook != -1)                   return 85;
    if (retook_errno != EPERM)          return 86;

    /* Give the terminal up.  The foreground group is still the reaped
     * child's — an empty group — so the SIGHUP/SIGCONT hangup reaches
     * nobody and this process is in no danger from its own release.  That
     * is the realistic shape too: a session gives up its terminal after
     * its jobs are gone. */
    if (ioctl(0, TIOCNOTTY, 0) != 0)    return 74;

    /* Released means released: the session has no terminal again, and says
     * so with the same ENOTTY it gave before it ever had one. */
    errno = 0;
    if (tcgetpgrp(0) != -1)             return 75;
    if (errno != ENOTTY)                return 76;
    errno = 0;
    if (tcsetpgrp(0, me) != -1)         return 77;
    if (errno != ENOTTY)                return 78;

    /* And the console really was freed, not merely forgotten by us: a
     * fresh acquire succeeds.  We leave it held on purpose — the kernel
     * releases it when this session empties, and self_test_cctty asserts
     * that from the other side. */
    if (ioctl(0, TIOCSCTTY, 0) != 0)    return 79;
    if (tcgetpgrp(0) != me)             return 80;

    /* ---------------------------------------------------------------- *
     * 90s — the terminal *modes* are the kernel's too.
     *
     * Same disease as the foreground group, found the same day: until
     * 2026-08-12 `tcgetattr` on the native ABI returned a compiled-in
     * constant and `tcsetattr` wrote a userspace static the kernel never
     * read, so asking for raw mode changed nothing and a native program
     * and a Linux-ABI program on the same console reported different
     * terminals.  Syscalls 541/542 make both a view of `tty::TERMIOS`.
     * See design-decisions.md §114.
     *
     * These checks cannot be done anywhere else.  `cargo test` runs the
     * posix crate on the host triple, where the syscall arm is compiled
     * out and a per-thread double answers — it proves the marshalling and
     * nothing about the kernel.  `dispatch::test_dispatch_termios_syscalls`
     * proves the kernel side and never goes through libc.  Only here do
     * the two layouts have to be the same layout.
     * ---------------------------------------------------------------- */
    struct termios orig;
    if (tcgetattr(0, &orig) != 0)       return 90;

    /* Values the *kernel* wrote (Linux's INIT_C_CC, in `tty.rs`), read at
     * indices *musl* computed.  This is the layout proof, and a mere
     * round trip could not substitute for it: the kernel stores and
     * returns whatever it is handed, so a set/get pair would agree even
     * if both directions were consistently wrong.  Here libc marshals its
     * 60-byte user struct (NCCS 32, explicit speeds) into the kernel's
     * 36-byte wire format (NCCS 19, speed folded into c_cflag's CBAUD
     * bits) by hand; if it were off by a field or a byte, these would
     * come back as neighbouring control characters rather than the exact
     * ones below. */
    if (orig.c_cc[VINTR]  != 003)       return 91;   /* ^C  */
    if (orig.c_cc[VQUIT]  != 034)       return 92;   /* ^\  */
    if (orig.c_cc[VERASE] != 0177)      return 93;   /* DEL */
    if (orig.c_cc[VKILL]  != 025)       return 94;   /* ^U  */
    if (orig.c_cc[VEOF]   != 004)       return 95;   /* ^D  */
    if (orig.c_cc[VSUSP]  != 032)       return 96;   /* ^Z  */
    if (orig.c_cc[VMIN]   != 1)         return 97;
    if ((orig.c_lflag & (ICANON | ECHO | ISIG)) != (ICANON | ECHO | ISIG))
                                        return 98;
    /* The speed survives the CBAUD folding in both directions. */
    if (cfgetospeed(&orig) != B38400)   return 99;

    /* Raw mode is real.  A `getch()`-style program clears these three and
     * expects the *next* read to return one byte without waiting for a
     * newline and without echoing it; before 541/542 that request was
     * dropped on the floor. */
    struct termios raw = orig;
    raw.c_lflag &= (tcflag_t)~(ICANON | ECHO | ISIG);
    raw.c_cc[VMIN]  = 1;
    raw.c_cc[VTIME] = 0;
    raw.c_cc[VINTR] = 021;              /* ^Q — a slot no default holds */
    if (tcsetattr(0, TCSANOW, &raw) != 0) return 100;

    struct termios got;
    if (tcgetattr(0, &got) != 0)          return 101;
    if (got.c_lflag & ICANON)             return 102;
    if (got.c_lflag & ECHO)               return 103;
    if (got.c_lflag & ISIG)               return 104;
    /* An individual c_cc slot is addressed correctly through the wire, and
     * the untouched slots came back untouched — a whole-array smear would
     * pass the flag checks above but fail here. */
    if (got.c_cc[VINTR] != 021)           return 105;
    if (got.c_cc[VSUSP] != 032)           return 106;
    if (got.c_cc[VERASE] != 0177)         return 107;
    /* c_iflag/c_oflag are three and two words away from c_lflag; a field
     * shift that survived everything above would show up as a change to
     * one of them. */
    if (got.c_iflag != orig.c_iflag)      return 108;
    if (got.c_oflag != orig.c_oflag)      return 109;

    /* Only the console has modes.  A pipe must say ENOTTY, not answer with
     * the console's settings — the fd-kind gate is what keeps `isatty()`
     * and every "am I interactive?" test honest. */
    int nt[2];
    if (pipe(nt) != 0)                    return 110;
    errno = 0;
    int notty = tcgetattr(nt[0], &got);
    int notty_errno = errno;
    close(nt[0]);
    close(nt[1]);
    if (notty != -1)                      return 111;
    if (notty_errno != ENOTTY)            return 112;

    /* Put the console back the way we found it: the kernel's console is
     * shared with every later boot self-test and with the shell, and a
     * fixture that left it in raw mode would silently break them. */
    if (tcsetattr(0, TCSANOW, &orig) != 0) return 113;
    if (tcgetattr(0, &got) != 0)           return 114;
    if (got.c_lflag != orig.c_lflag)       return 115;
    if (got.c_cc[VINTR] != 003)            return 116;

    return 42;
}
