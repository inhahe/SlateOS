/*
 * ctest-ctty — ring-3 regression test for the **controlling terminal** and
 * the **foreground process group** reached through *our own* libc
 * (`AbiMode::Native`), not through the Linux ABI shim.
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
 * What is deliberately NOT here.  Nothing raises `SIGTTIN`/`SIGTTOU` on a
 * background read or write yet, and there is no `Ctrl-Z` from the line
 * discipline: both need a real tty driver to enforce at.  See `todo.txt`.
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

    /* Foregrounding the now-empty group fails, which is what proves the
     * foreground group names live membership and not just a number.  Note
     * that *reading* it still works: a shell whose job just died must be
     * able to see the stale group in order to take the terminal back. */
    errno = 0;
    if (tcsetpgrp(0, child) != -1)      return 72;
    if (errno != EPERM)                 return 73;

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

    return 42;
}
