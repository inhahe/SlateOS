/*
 * ctest-pgroup — ring-3 regression test for process groups reached through
 * *our own* libc (`AbiMode::Native`), not through the Linux ABI shim.
 *
 * Why this fixture has to exist.  `AbiMode` is per-process: a program is
 * Native or Linux, never both.  The kernel has had real process groups since
 * 2026-06-20, but for a long time they were reachable *only* from the Linux
 * shim, and a program linked against our libc got a userspace-only
 * `static mut PGID` plus `ENOSYS` for every `kill(pid <= 0)`.  That is
 * invisible from a host `cargo test`, because on the host triple the posix
 * crate deliberately answers from a local test double (`host_pg`) rather than
 * issuing a syscall — the syscall arm is compiled only for
 * `target_os = "none"`.  It is equally invisible from the kernel's own
 * dispatch self-test, which calls the handlers directly and never goes
 * through libc at all.  The gap between "the wrappers are right" and "the
 * wrappers are wired to the kernel" can only be closed by a native-ABI binary
 * in ring 3.  That is this file.  See known-issues.md,
 * TD-POSIX-PROCESS-GROUPS-ARE-FAKE-FOR-NATIVE-ABI-PROGRAMS.
 *
 * The load-bearing assertion is check 55: a *parent* asks the kernel for its
 * *child's* group after moving it.  Two processes agreeing about a third
 * process's group is exactly what a userspace `static mut` can never do, so a
 * pass there proves the state is really shared, not merely self-consistent.
 *
 * Starting conditions.  The kernel spawns us with parent 0, so `pcb::create`
 * makes us our own session and group leader (pgid == sid == pid).  Every
 * expectation below follows from that, which is why they can be exact rather
 * than "some plausible value".
 *
 * Keeping the child alive.  The child holds the read end of a pipe the parent
 * never writes to, so it blocks until the parent *closes* the write end.  A
 * `sleep` would have been a race in both directions — too short and the child
 * is a zombie before the parent can measure it, too long and the kernel's
 * bounded yield budget for the fixture expires — whereas the pipe makes the
 * child's lifetime exactly "as long as the parent still needs it".
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `FAIL`/`return` values below and the legend in
 * kernel/src/proc/spawn.rs::self_test_cpgroup).
 */

#include <errno.h>
#include <signal.h>
#include <unistd.h>
#include <sys/wait.h>

/* A pgid no live process will ever hold, used as an empty destination group. */
#define NO_GROUP 7654321

int main(void)
{
    const pid_t me = getpid();

    /* ---------------------------------------------------------------- *
     * 10s — the caller's own view.  We were spawned with parent 0, so we
     * lead our own session and group.  Before the fix these came from a
     * userspace static that happened to agree; now they are the kernel's
     * answer, and they must still agree.
     * ---------------------------------------------------------------- */
    if (me <= 0)                return 10;
    if (getpgid(0) != me)       return 11;
    if (getpgrp() != me)        return 12;
    if (getsid(0) != me)        return 13;
    /* Asking by explicit pid must match asking by 0. */
    if (getpgid(me) != me)      return 14;
    if (getsid(me) != me)       return 15;

    /* ---------------------------------------------------------------- *
     * 20s — errors are the kernel's, not a userspace guess.  A session
     * leader's pgid is fixed (EPERM), and a leader cannot start a new
     * session (EPERM).  The old userspace implementation returned 0 for
     * both, because it had no way to know it was a leader.
     * ---------------------------------------------------------------- */
    errno = 0;
    if (setpgid(0, 0) != -1)        return 20;
    if (errno != EPERM)             return 21;

    errno = 0;
    if (setpgid(0, NO_GROUP) != -1) return 22;
    if (errno != EPERM)             return 23;

    errno = 0;
    if (setsid() != -1)             return 24;
    if (errno != EPERM)             return 25;
    /* A failed setsid() must not have moved us. */
    if (getsid(0) != me)            return 26;

    /* Unknown and negative targets are ESRCH, and must not disturb us. */
    errno = 0;
    if (getpgid(NO_GROUP) != -1)    return 27;
    if (errno != ESRCH)             return 28;
    errno = 0;
    if (getsid(-1) != -1)           return 29;
    if (errno != ESRCH)             return 30;

    /* ---------------------------------------------------------------- *
     * 30s — kill() with a non-positive pid reaches the kernel at all.
     * Every one of these returned ENOSYS before the fix, so this band is
     * the plainest statement of what changed.  Only `sig == 0` existence
     * probes are used, so nothing is actually signalled.
     * ---------------------------------------------------------------- */
    if (kill(0, 0) != 0)            return 31;  /* my own group */
    if (kill(-me, 0) != 0)          return 32;  /* the same group, by id */
    if (killpg(me, 0) != 0)         return 33;  /* and again via killpg */
    if (killpg(0, 0) != 0)          return 34;  /* killpg(0) == own group */

    errno = 0;
    if (killpg(NO_GROUP, 0) != -1)  return 35;
    if (errno != ESRCH)             return 36;

    /* kill(-1) is the broadcast form, deliberately not modelled — it needs a
     * credential model to define "every process you may signal".  Pinned so
     * that implementing it is a conscious change.  See known-issues.md,
     * TD-KILL-MINUS-ONE-BROADCAST-NOT-MODELLED. */
    errno = 0;
    if (kill(-1, 0) != -1)          return 37;
    if (errno != ESRCH)             return 38;

    /* Ordering: the signal number is validated in the libc wrapper, before
     * the target is classified, so a bad signal aimed at a live group is
     * EINVAL rather than being reported against the group. */
    errno = 0;
    if (killpg(me, 9999) != -1)     return 39;
    if (errno != EINVAL)            return 40;

    /* ---------------------------------------------------------------- *
     * 50s — the real thing: a parent moving a child into a new group and
     * then observing it.  The child inherits our pgid/sid across fork, so
     * it is not a session leader and may be moved; `setpgid(child, child)`
     * creates a group the child leads.  Under the old userspace-only
     * implementation the parent's `setpgid` wrote the *parent's* static and
     * the child never learned anything, so `getpgid(child)` could not have
     * reported the move and `killpg(child, 0)` could not have found it.
     * ---------------------------------------------------------------- */
    int fds[2];
    if (pipe(fds) != 0)             return 50;

    const pid_t child = fork();
    if (child < 0) {
        close(fds[0]);
        close(fds[1]);
        return 51;
    }
    if (child == 0) {
        /* Block until the parent is done with us.  We assert nothing here:
         * a child's failure could not be distinguished from a scheduling
         * artefact, so all judgement stays in the parent. */
        close(fds[1]);
        char c;
        while (read(fds[0], &c, 1) > 0) {
            /* The parent never writes; drain anyway rather than assume. */
        }
        close(fds[0]);
        _exit(0);
    }

    close(fds[0]);

    /* From here on every failure must release the child before returning,
     * so the exit code the kernel sees is ours and not a timeout. */
    int rc = 42;

    /* The child inherited our group and session verbatim. */
    if (getpgid(child) != me)               { rc = 52; goto done; }
    if (getsid(child) != me)                { rc = 53; goto done; }

    /* Our group now has two live members, so a probe at it still succeeds. */
    if (killpg(me, 0) != 0)                 { rc = 54; goto done; }

    /* Move the child into a brand-new group that it leads — what a shell
     * does to put a job into its own process group.  THE assertion is the
     * next one: we, the parent, then see the child's new group. */
    if (setpgid(child, child) != 0)         { rc = 56; goto done; }
    if (getpgid(child) != child)            { rc = 55; goto done; }

    /* The move did not change the session, and did not move us. */
    if (getsid(child) != me)                { rc = 57; goto done; }
    if (getpgid(0) != me)                   { rc = 58; goto done; }

    /* The child's new group is reachable *as a group*, by both spellings. */
    if (killpg(child, 0) != 0)              { rc = 59; goto done; }
    if (kill(-child, 0) != 0)               { rc = 60; goto done; }

    /* Joining a group no live process holds is EPERM, from the kernel, and
     * the failed attempt leaves the child where it was. */
    errno = 0;
    if (setpgid(child, NO_GROUP) != -1)     { rc = 61; goto done; }
    if (errno != EPERM)                     { rc = 62; goto done; }
    if (getpgid(child) != child)            { rc = 63; goto done; }

done:
    /* Releasing the write end is what lets the child's read() return 0. */
    close(fds[1]);
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return rc == 42 ? 70 : rc;
    }
    if (rc != 42) {
        return rc;
    }

    /* ---------------------------------------------------------------- *
     * 70s — the child is reaped, so confirm the group really was backed by
     * that one process: now that it is gone the group is empty and probes
     * are ESRCH.  This is what makes check 55 mean something — the group id
     * was not merely accepted, it named a live membership that ended with
     * its member.
     * ---------------------------------------------------------------- */
    errno = 0;
    if (killpg(child, 0) != -1)     return 71;
    if (errno != ESRCH)             return 72;

    /* Our own group outlived it. */
    if (killpg(me, 0) != 0)         return 73;
    if (getpgid(0) != me)           return 74;

    return 42;
}
