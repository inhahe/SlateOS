/*
 * ctest-jobctl — ring-3 regression test for job-control stop and continue,
 * driven end to end through *our own* libc (`AbiMode::Native`).
 *
 * Why this fixture has to exist.  The kernel has had `TaskState::Suspended`,
 * `JobControlEvent`, `stop_process_for_signal` and `continue_process` for
 * weeks, but until 2026-08-12 nothing in userspace could reach them: our libc
 * answered a `Stop` default action with "we have no kernel suspend mechanism"
 * and our `waitpid` accepted `WUNTRACED`/`WCONTINUED` and then dropped them on
 * the floor.  Both halves were fixed together (`SYS_SIGNAL_STOP_SELF` = 1062
 * and `SYS_PROCESS_WAIT_STATUS` = 1063), and this file is the only test that
 * exercises the two halves *against each other*:
 *
 *   * The **host suite** cannot: `stop_self`'s syscall arm is compiled only
 *     for `target_os = "none"`, so on the host triple every stop reports
 *     `ENOSYS` — correct there, and proof of nothing.
 *   * The **kernel dispatch self-test** cannot either.  It calls the handlers
 *     directly with synthetic `record_jc_stopped`/`record_jc_continued`
 *     records, and it can only ever use `WNOHANG`, because a *real* stop on
 *     the boot thread would park the one task left to resume it.
 *
 * So the kernel test proves "the kernel remembers a stop", the host test
 * proves "the wrapper computes the right arguments", and only a native binary
 * with two real processes proves that a child which stops *itself* is seen as
 * stopped by a parent blocked in `waitpid`, and can be resumed by that parent.
 *
 * The load-bearing checks are 53-54 and 61: check 53/54 is a parent decoding
 * `WIFSTOPPED`/`WSTOPSIG` from a status the *kernel* encoded for a stop the
 * *child* initiated, and check 61 is `WIFCONTINUED` after the parent's
 * `kill(child, SIGCONT)` actually restarted it.  Neither is reachable without
 * both new syscalls being wired correctly at both ends.
 *
 * `WSTOPSIG == SIGTSTP` (not SIGSTOP) is deliberate and is the reason
 * `SYS_SIGNAL_STOP_SELF` takes the signal number rather than being a bare
 * "stop me": a shell reporting a Ctrl-Z job must name the signal that really
 * stopped it.  It is also why the child raises SIGTSTP rather than SIGSTOP —
 * SIGTSTP is a *catchable* stop signal, and catchable stop signals are exactly
 * the case the old kernel send path got wrong (a registered trampoline made
 * the kernel classify them as "deliver to the handler", which re-entered the
 * dispatcher that had just resolved the disposition to SIG_DFL: an infinite
 * delivery loop instead of a stop).
 *
 * Authority.  The parent's `kill(child, SIGCONT)` is a real cross-process
 * send, unlike `ctest-pgroup`'s `sig == 0` probes.  It needs no capability
 * grant from the kernel spawn: the kernel authorises a signal when the caller
 * *is the target's parent*, and our libc's own `CAP_KILL` gate reads the
 * process capability words, which start out as "every capability held".
 *
 * Keeping the child alive after it resumes.  Once the parent sends SIGCONT it
 * must still observe the *continued* event before the child exits, because the
 * kernel reports a reapable exit in preference to a pending job-control
 * record — an exiting child would win the race and check 61 would see an exit
 * instead of a continue.  So the resumed child blocks on the read end of a
 * pipe the parent closes only after check 61 has passed.  A `sleep` would race
 * in both directions; the pipe makes the child's lifetime exactly "until the
 * parent no longer needs it".
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the legend in kernel/src/proc/spawn.rs::self_test_jobctl).
 */

#include <errno.h>
#include <signal.h>
#include <unistd.h>
#include <sys/wait.h>

/* The value the child exits with once the parent releases it.  Distinct from
 * the parent's own 42 so a confused kernel-side read of the wrong process's
 * exit code cannot masquerade as a pass. */
#define CHILD_OK 7

/* The child exits with this if its own `raise(SIGTSTP)` failed rather than
 * stopping it.  The parent's blocking wait then reports an *exit* where a stop
 * was expected, and check 52/53 names it — so a broken SYS_SIGNAL_STOP_SELF
 * surfaces as a decisive failure instead of a hang. */
#define CHILD_RAISE_FAILED 91

/* waitpid(), retrying if a signal interrupts the wait.
 *
 * SIGCHLD arrives at the parent exactly when the events below fire, so an
 * un-retried blocking wait would return EINTR at precisely the moments this
 * fixture is trying to measure.  Bounded in practice by the kernel's yield
 * budget for the fixture, so a genuine livelock is reported as a timeout
 * rather than hanging the boot test. */
static pid_t wait_retry(pid_t pid, int *status, int options)
{
    for (;;) {
        errno = 0;
        const pid_t w = waitpid(pid, status, options);
        if (w < 0 && errno == EINTR) {
            continue;
        }
        return w;
    }
}

int main(void)
{
    int fds[2];
    if (pipe(fds) != 0) {
        return 10;
    }

    const pid_t child = fork();
    if (child < 0) {
        close(fds[0]);
        close(fds[1]);
        return 11;
    }

    if (child == 0) {
        close(fds[1]);

        /* Stop ourselves.  This does not return until someone sends SIGCONT,
         * so everything after it is evidence that the resume worked. */
        if (raise(SIGTSTP) != 0) {
            close(fds[0]);
            _exit(CHILD_RAISE_FAILED);
        }

        /* Resumed.  Hold still until the parent has finished observing the
         * continue, then exit with a code it can decode. */
        for (;;) {
            char c;
            const ssize_t n = read(fds[0], &c, 1);
            if (n == 0) {
                break; /* parent closed the write end */
            }
            if (n < 0 && errno != EINTR) {
                break; /* nothing to judge here; all assertions are the parent's */
            }
        }
        close(fds[0]);
        _exit(CHILD_OK);
    }

    close(fds[0]);

    /* Every failure from here must release the child before returning, so the
     * exit code the kernel sees is a verdict and not a timeout. */
    int rc = 42;
    int st;

    /* ---------------------------------------------------------------- *
     * 50s — a blocking wait sees the child stop.
     *
     * No WNOHANG: the child may not have reached `raise` yet, and the point
     * of the exercise is that a *parked* parent is woken by the stop.  Before
     * SYS_PROCESS_WAIT_STATUS this call could only ever return on the child's
     * death, so it would have hung here forever.
     * ---------------------------------------------------------------- */
    st = 0;
    if (wait_retry(child, &st, WUNTRACED) != child)  { rc = 52; goto done; }
    if (!WIFSTOPPED(st))                             { rc = 53; goto done; }
    if (WSTOPSIG(st) != SIGTSTP)                     { rc = 54; goto done; }
    /* The three status predicates are mutually exclusive; a status that
     * claimed both would mean the encoder, not the event, is wrong. */
    if (WIFEXITED(st))                               { rc = 55; goto done; }
    if (WIFSIGNALED(st))                             { rc = 56; goto done; }
    if (WIFCONTINUED(st))                            { rc = 57; goto done; }

    /* The report was consumed: asking again finds nothing new.  (WNOHANG
     * here precisely because a *correct* kernel has nothing to report and
     * a blocking call would park us until the child exits.) */
    st = 0;
    if (wait_retry(child, &st, WNOHANG | WUNTRACED) != 0) { rc = 58; goto done; }

    /* A stop is not a death: the child still exists and is not reaped.  If
     * the kernel had turned the stop into an exit, this would be ESRCH. */
    if (kill(child, 0) != 0)                         { rc = 59; goto done; }

    /* ---------------------------------------------------------------- *
     * 60s — the parent resumes the child and sees it happen.
     * ---------------------------------------------------------------- */
    if (kill(child, SIGCONT) != 0)                   { rc = 60; goto done; }

    st = 0;
    if (wait_retry(child, &st, WCONTINUED) != child) { rc = 61; goto done; }
    if (!WIFCONTINUED(st))                           { rc = 62; goto done; }
    if (WIFSTOPPED(st))                              { rc = 63; goto done; }
    if (WIFEXITED(st))                               { rc = 64; goto done; }

    /* Consumed, like the stop was. */
    st = 0;
    if (wait_retry(child, &st, WNOHANG | WCONTINUED) != 0) { rc = 65; goto done; }

done:
    /* Releasing the write end is what lets the resumed child's read() see EOF.
     * If the child is still *stopped* (an early failure above), it would never
     * reach the read at all — so continue it first, unconditionally.  A
     * SIGCONT to an already-running child is a no-op, which is exactly what
     * POSIX says it should be. */
    kill(child, SIGCONT);
    close(fds[1]);

    st = 0;
    if (wait_retry(child, &st, 0) != child) {
        return rc == 42 ? 70 : rc;
    }
    if (rc != 42) {
        return rc;
    }

    /* ---------------------------------------------------------------- *
     * 70s — the child really did resume and run to completion.  A stop that
     * the kernel never lifted would show up here as a timeout instead, and a
     * stop that was silently converted into a signal death would fail 72.
     * ---------------------------------------------------------------- */
    if (!WIFEXITED(st))                     return 71;
    if (WIFSIGNALED(st))                    return 72;
    if (WIFSTOPPED(st))                     return 73;
    if (WEXITSTATUS(st) == CHILD_RAISE_FAILED) return 74;
    if (WEXITSTATUS(st) != CHILD_OK)        return 75;

    /* Reaped exactly once: there is no child left to wait for. */
    errno = 0;
    if (wait_retry(child, &st, WNOHANG) != -1) return 76;
    if (errno != ECHILD)                       return 77;

    return 42;
}
