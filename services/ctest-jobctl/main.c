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
 * What the 100s, 120s and 140s add: `waitid`.  As of 2026-08-16 our `waitid`
 * is no longer a stub that ignored its `siginfo_t` argument and refused every
 * process-group wait — it fills the structure, and it accepts `P_PGID`.
 * Neither fact is testable on the host, for the same reason the rest of this
 * file is not: the host build compiles `waitpid`'s syscall arm out entirely,
 * so a host `waitid` returns ENOSYS before a status word ever exists to
 * decode.  The `wstatus` → `siginfo_t` mapping — the part a C caller reads
 * back through a pointer it supplied — therefore has exactly one place it can
 * be checked against a status the kernel really encoded, and this is it.
 *
 *   * **100s** re-runs the stop/continue cycle through `waitid` instead of
 *     `waitpid`.  It is a *second* cycle, not a re-read of the first, because
 *     a job-control report is consumed by whichever call observes it: had
 *     `waitid` read the same stop check 53 read, one of the two decoders
 *     would be testing nothing.  So the child raises SIGTSTP twice, checks
 *     52-65 keep the first cycle, and 100-111 own the second.  Check 105/106
 *     is the consumption proof — an immediately following `WNOHANG` wait must
 *     report `si_pid == 0`, not hand back the same stop again.
 *   * **120s** covers the termination codes (`CLD_EXITED`, `CLD_KILLED`),
 *     which need *fresh* children: the long-lived child's exit is already
 *     claimed by the 70s.  Check 124 is the one worth naming — `si_status`
 *     must be the child's exit *code*, so a libc that forwarded the raw
 *     `wstatus` word would show `code << 8` and be caught here instead of
 *     silently mis-reporting every exit status to every caller.
 *   * **140s** checks argument validation, which is stricter for `waitid`
 *     than for `waitpid` precisely because `waitid` *names* its selector
 *     instead of overloading a signed integer: a bad `id` has to be an error,
 *     never a silently *different* wait.
 *
 * Every `waitid` here goes through `waitid_retry`, which poisons the
 * `siginfo_t` before each call — see its comment for why zeroing it instead
 * would hide the exact failure these checks exist to catch.
 *
 * Three POSIX `waitid` features are still unreachable from userspace because
 * the facts they need do not survive the reap (`WNOWAIT`, `si_uid`, a wait on
 * process group 1).  Nothing here tests them; they are written up in
 * known-issues.md → TD-POSIX-WAITID-IS-NARROWER-THAN-THE-KERNEL-COULD-MAKE-IT
 * and requested of lane A in requests/b-a-waitid-needs-an-explicit-idtype-wait.md.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check.  `kernel/src/proc/spawn.rs::self_test_jobctl` prints the code
 * and points back here rather than duplicating a table, so a new check needs
 * no change on the kernel side — but its doc comment does paraphrase this one,
 * and as of 2026-08-16 that paraphrase still describes only the waitpid half.
 * It is lane A's file; `requests/b-a-jobctl-fixture-now-covers-waitid.md` asks
 * for the paragraph.
 */

#include <errno.h>
#include <signal.h>
#include <unistd.h>
#include <sys/types.h>
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

/* Same, for the *second* stop — the one the 100s checks observe with `waitid`.
 * A distinct code so a failure names which of the two raises broke. */
#define CHILD_RAISE2_FAILED 92

/* Exit code of the short-lived children the 120s checks reap.  Deliberately not
 * CHILD_OK: those children are reaped while the long-lived one is still alive,
 * so a `waitid` that reported the wrong child would otherwise be
 * indistinguishable from one that reported the right one. */
#define BRIEF_CHILD_OK 23

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

/* waitid(), retrying on EINTR, with the siginfo_t pre-poisoned.
 *
 * The poison is the point: POSIX has `waitid` *write* the structure, and the
 * failure this guards against is a libc that returns 0 having written nothing.
 * Zeroing instead would make that failure look like a legitimate "no child
 * changed state" report (which is `si_pid == 0`), so the caller could not tell
 * "filled in with zero" from "never touched".  0x5A is nobody's pid, nobody's
 * CLD_* code and nobody's signal number.
 *
 * Returns waitid's own return value; the caller inspects `info`. */
static int waitid_retry(idtype_t idtype, id_t id, siginfo_t *info, int options)
{
    for (;;) {
        unsigned char *raw = (unsigned char *)info;
        for (unsigned i = 0; i < sizeof(*info); i++) {
            raw[i] = 0x5A;
        }
        errno = 0;
        const int r = waitid(idtype, id, info, options);
        if (r < 0 && errno == EINTR) {
            continue;
        }
        return r;
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

        /* Stop a second time.  The parent observes this cycle through
         * `waitid` rather than `waitpid` (checks 100-111).  Two cycles rather
         * than one because a job-control report is *consumed* by the first
         * observer: whichever call reads the stop, the other cannot also see
         * it.  Re-stopping is the only way to exercise both decoders against
         * a real kernel-encoded event without either weakening the other. */
        if (raise(SIGTSTP) != 0) {
            close(fds[0]);
            _exit(CHILD_RAISE2_FAILED);
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

    /* ---------------------------------------------------------------- *
     * 100s — the same stop/continue cycle, observed through `waitid`.
     *
     * The numbering skips 80–99 deliberately: the child's own failure codes
     * (91, 92) live in that range, and a parent code that collided with one
     * would make a failing exit status ambiguous to read.
     *
     * `waitid` reports a state change as a `siginfo_t` rather than as a
     * bit-packed status word, so nothing above proves any of it: the status
     * word is the kernel's encoding, while `si_code`/`si_status` are libc's
     * *decoding* of that word into POSIX's terms, and a decoder that got
     * CLD_STOPPED and CLD_CONTINUED backwards would pass every check so far.
     *
     * This is also the only place the decoding is testable at all — the host
     * test suite compiles `waitpid`'s syscall arm out entirely, so on the
     * host every `waitid` returns ENOSYS before reaching a status word.
     * ---------------------------------------------------------------- */
    {
        siginfo_t info;

        /* Blocking, like check 52: the child may not have reached its second
         * `raise` yet.  WEXITED is set alongside WSTOPPED so that a child
         * which died instead of stopping is *reported* (as CLD_EXITED, failing
         * 102 with a name) rather than leaving us parked forever. */
        if (waitid_retry(P_PID, (id_t)child, &info, WSTOPPED | WEXITED) != 0) {
            rc = 100; goto done;
        }
        if (info.si_pid != child)              { rc = 101; goto done; }
        if (info.si_code != CLD_STOPPED)       { rc = 102; goto done; }
        if (info.si_status != SIGTSTP)         { rc = 103; goto done; }
        if (info.si_signo != SIGCHLD)          { rc = 104; goto done; }

        /* Consumed, exactly as the waitpid-observed stop was.  WNOHANG makes
         * this decisive: a correct kernel has nothing to report, and a
         * `waitid` that returned 0 without writing would leave si_pid at the
         * poison value rather than at the 0 POSIX specifies for a miss. */
        if (waitid_retry(P_PID, (id_t)child, &info, WNOHANG | WSTOPPED) != 0) {
            rc = 105; goto done;
        }
        if (info.si_pid != 0)                  { rc = 106; goto done; }

        if (kill(child, SIGCONT) != 0)         { rc = 107; goto done; }

        if (waitid_retry(P_PID, (id_t)child, &info, WCONTINUED | WEXITED) != 0) {
            rc = 108; goto done;
        }
        if (info.si_code != CLD_CONTINUED)     { rc = 109; goto done; }
        if (info.si_pid != child)              { rc = 110; goto done; }
        if (info.si_status != SIGCONT)         { rc = 111; goto done; }
    }

    /* ---------------------------------------------------------------- *
     * 120s — `waitid` on children that *terminate*, which the long-lived child
     * cannot supply: its own exit is claimed by the 70s checks below, and a
     * termination can only be observed once.
     *
     * Both children are reaped while the long-lived child is still alive and
     * still has an outstanding relationship with us, so `si_pid` here is also
     * a check that `waitid` reported the child it was asked about.
     * ---------------------------------------------------------------- */
    {
        siginfo_t info;

        const pid_t brief = fork();
        if (brief < 0)                          { rc = 120; goto done; }
        if (brief == 0) {
            _exit(BRIEF_CHILD_OK);
        }
        if (waitid_retry(P_PID, (id_t)brief, &info, WEXITED) != 0) {
            rc = 121; goto done;
        }
        if (info.si_pid != brief)               { rc = 122; goto done; }
        if (info.si_code != CLD_EXITED)         { rc = 123; goto done; }
        /* For CLD_EXITED, si_status is the exit *code* — not the packed
         * status word.  Reporting `BRIEF_CHILD_OK << 8` here would mean libc
         * forwarded the raw word, which is the likeliest way to get this
         * wrong. */
        if (info.si_status != BRIEF_CHILD_OK)   { rc = 124; goto done; }

        /* Reaped: `waitid` consumed it, so there is no such child left. */
        errno = 0;
        if (waitid_retry(P_PID, (id_t)brief, &info, WNOHANG | WEXITED) != -1) {
            rc = 125; goto done;
        }
        if (errno != ECHILD)                    { rc = 126; goto done; }

        /* A signal death reports CLD_KILLED with the *signal* in si_status.
         * SIGKILL because it cannot be caught, blocked or handled, so the
         * child's own code cannot change the outcome. */
        const pid_t doomed = fork();
        if (doomed < 0)                         { rc = 127; goto done; }
        if (doomed == 0) {
            /* Park until killed.  `pause` returns only via a handler, and we
             * install none, so the only way out is the signal itself. */
            for (;;) {
                pause();
            }
        }
        if (kill(doomed, SIGKILL) != 0)         { rc = 128; goto done; }
        if (waitid_retry(P_PID, (id_t)doomed, &info, WEXITED) != 0) {
            rc = 129; goto done;
        }
        if (info.si_pid != doomed)              { rc = 130; goto done; }
        if (info.si_code != CLD_KILLED)         { rc = 131; goto done; }
        if (info.si_status != SIGKILL)          { rc = 132; goto done; }
    }

    /* ---------------------------------------------------------------- *
     * 140s — `waitid`'s argument validation, which is stricter than
     * `waitpid`'s because `waitpid`'s single selector overloads 0 and
     * negatives as *group* selectors.  Each of these, unvalidated, would
     * become a different wait rather than an error — and since these all run
     * with a live child, "a different wait" means one that could block or
     * reap.  That they return promptly is itself part of the check.
     * ---------------------------------------------------------------- */
    {
        siginfo_t info;

        errno = 0;
        if (waitid_retry(P_PID, 0, &info, WEXITED) != -1)      { rc = 140; goto done; }
        if (errno != EINVAL)                                   { rc = 141; goto done; }

        errno = 0;
        if (waitid_retry(P_PGID, (id_t)-1, &info, WEXITED) != -1) { rc = 142; goto done; }
        if (errno != EINVAL)                                   { rc = 143; goto done; }

        /* At least one of WEXITED/WSTOPPED/WCONTINUED is required. */
        errno = 0;
        if (waitid_retry(P_ALL, 0, &info, WNOHANG) != -1)      { rc = 144; goto done; }
        if (errno != EINVAL)                                   { rc = 145; goto done; }

        /* An unknown option bit is rejected outright, not ignored. */
        errno = 0;
        if (waitid_retry(P_ALL, 0, &info, WEXITED | (1 << 4)) != -1) { rc = 146; goto done; }
        if (errno != EINVAL)                                   { rc = 147; goto done; }
    }

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
    /* There is deliberately no companion check for CHILD_RAISE2_FAILED here.
     * A failed *second* raise makes the child exit rather than stop, and the
     * blocking `waitid` at check 100 reports that exit as CLD_EXITED — so it
     * fails 102 and reaps the child, and this wait never sees it.  Naming it
     * again here would be an unreachable branch pretending to be a check. */

    /* Reaped exactly once: there is no child left to wait for. */
    errno = 0;
    if (wait_retry(child, &st, WNOHANG) != -1) return 76;
    if (errno != ECHILD)                       return 77;

    return 42;
}
