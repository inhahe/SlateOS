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
 * What the 150s add: the extended wait ABI (2026-08-16).  Three POSIX `waitid`
 * features used to be unreachable from userspace because the facts they need
 * did not survive the reap — `WNOWAIT`, `si_uid`, and a wait on process group
 * 1.  Lane A closed all three in the kernel
 * (`requests/a-b-wait-syscall-grew-wpgid-wnowait-and-a-waitinfo-struct.md`),
 * adding `WPGID`, `WNOWAIT` and a `WINFO`-gated `WaitInfo` out-parameter to
 * `SYS_PROCESS_WAIT_STATUS`, and libc consumes them as of the same day.
 *
 * The 150s reach that ABI **raw**, with a hand-written `syscall`, and they are
 * the only place in the tree that can:
 *
 *   * libc always passes its own `sizeof(WaitInfo)`, by construction.  The
 *     size field exists so that an *older* libc on a *newer* kernel gets the
 *     prefix it understands and is never written past — the `clone3` /
 *     `sched_setattr` convention.  Exercising that means passing a size libc
 *     itself would never pass, so it cannot be done through libc, and it is
 *     deliberately not exposed as a libc entry point either: a size-taking
 *     export whose only correct caller is a test is a permanent hole through
 *     which every other caller can hand the kernel an arbitrary length.
 *   * The kernel's own self-tests cannot reach it either, for the reason lane
 *     A spells out: a bare kernel task has no user address space, so
 *     `dispatch.rs::test_dispatch_wait_info_layout` can check the pure encoder
 *     but never `copy_to_user`.  Truncation and the zero-filled tail are
 *     properties *of the copy*.
 *
 * So: check 157 is the zero-filled tail (`arg4 = 128`, bytes 72..128 must come
 * back zero rather than as we poisoned them), and check 169 is the truncation
 * (`arg4 = 24`, nothing at or past byte 24 may be touched).  Those two are what
 * lane A asked lane B for by name.  Around them, 151-163 check that a real
 * child's `WaitInfo` is filled and plausible — 155 is `si_uid`, which was the
 * whole of request item 3 — 170-177 check that `WNOWAIT` peeks without reaping
 * and that an identical second call sees the same event again, and 178-187
 * check that a process group is nameable, including group 1, which used to be
 * a flat `ENOSYS` from libc because `waitpid`'s signed selector spends -1 on
 * "any child".
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
#include <stdint.h>
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

/* --------------------------------------------------------------------- *
 * The raw `SYS_PROCESS_WAIT_STATUS` ABI, for the 150s.
 *
 * Everything below deliberately bypasses libc.  See the 150s paragraph in the
 * file header for why the size field cannot be exercised through it.
 * --------------------------------------------------------------------- */

/* SlateOS native syscall number.  Not a Linux number — SlateOS's space is
 * unrelated, and 1063 in Linux's table is nothing at all. */
#define SYS_PROCESS_WAIT_STATUS 1063

/* Option bits.  The low three match POSIX's WNOHANG/WUNTRACED/WCONTINUED by
 * value; the high three are SlateOS's, and are spelled out here rather than
 * taken from a header because no header declares them for C. */
#define K_WNOHANG    0x00000001u
#define K_WUNTRACED  0x00000002u
#define K_WCONTINUED 0x00000008u
#define K_WPGID      0x00010000u
#define K_WINFO      0x00020000u
#define K_WNOWAIT    0x01000000u

/* Native error numbers, which are NOT errno values.
 *
 * A raw native syscall returns `KernelError`'s own numbering (kernel/src/
 * error.rs) in rax.  Turning that into an errno is *libc's* job — see
 * posix/src/errno.rs, where `native::NO_CHILD_PROCESS` (-203) maps to ECHILD
 * (10).  So a test that bypasses libc must compare against the kernel's number;
 * comparing against <errno.h>'s is comparing the answer to a translation that
 * has not happened yet.
 *
 * This cost a boot cycle on 2026-08-16: check 177 read `if (r != -ECHILD)` and
 * the kernel had correctly returned -203.  The failure was indistinguishable
 * from a real WNOWAIT bug, which is the argument for spelling these out with a
 * name rather than writing the constants inline. */
#define K_ENOCHILD   (-203L)
#define K_EINTR      (-8L)

/* The kernel's `WaitInfo`, 72 bytes, little-endian, naturally aligned.  Kept
 * as an explicit layout rather than a shared header because the point of the
 * checks below is to verify the *bytes the kernel writes*, and a struct
 * generated from the same source as the kernel's would agree with it by
 * construction even if both were wrong. */
struct wait_info {
    uint64_t pid;
    uint32_t uid;
    uint32_t pad0;
    int32_t  wstatus;
    uint32_t pad1;
    uint64_t utime_us;
    uint64_t stime_us;
    uint64_t minflt;
    uint64_t majflt;
    uint64_t nvcsw;
    uint64_t nivcsw;
};

_Static_assert(sizeof(struct wait_info) == 72, "WaitInfo is 72 bytes");
_Static_assert(__builtin_offsetof(struct wait_info, uid) == 8, "uid at 8");
_Static_assert(__builtin_offsetof(struct wait_info, wstatus) == 16, "wstatus at 16");
_Static_assert(__builtin_offsetof(struct wait_info, utime_us) == 24, "utime at 24");
_Static_assert(__builtin_offsetof(struct wait_info, nivcsw) == 64, "nivcsw at 64");

/* A buffer deliberately larger than the kernel's struct, so "the tail past 72
 * bytes" is a place that exists.  The union gives it the struct's alignment. */
union wait_info_buf {
    struct wait_info info;
    unsigned char raw[128];
};

/* The poison byte.  0x5A is nobody's pid, uid, exit code or plausible counter
 * value, and 0x5A5A5A5A5A5A5A5A is ~6.5e18 — far outside every bound the
 * checks below assert, so an unwritten field cannot pass as a written one. */
#define POISON 0x5A

static void poison_buf(unsigned char *p, unsigned n)
{
    for (unsigned i = 0; i < n; i++) {
        p[i] = POISON;
    }
}

static int all_poison(const unsigned char *p, unsigned n)
{
    for (unsigned i = 0; i < n; i++) {
        if (p[i] != POISON) {
            return 0;
        }
    }
    return 1;
}

static int all_zero(const unsigned char *p, unsigned n)
{
    for (unsigned i = 0; i < n; i++) {
        if (p[i] != 0) {
            return 0;
        }
    }
    return 1;
}

/* Issue SYS_PROCESS_WAIT_STATUS directly, retrying on interruption.
 *
 * Returns the kernel's raw value: a pid, 0 for a WNOHANG miss, or a negative
 * *KernelError* — see the K_E* constants above. The native ABI does not set
 * `errno` and does not speak errno at all; libc's wrappers do both.
 * Five arguments, so `r10`/`r8` are loaded explicitly; this is musl's
 * register-variable form of the x86-64 syscall convention. */
static long wait_status_raw(uint64_t a0, uint64_t a1, uint64_t a2,
                            uint64_t a3, uint64_t a4)
{
    for (;;) {
        long ret;
        register uint64_t r10 __asm__("r10") = a3;
        register uint64_t r8 __asm__("r8") = a4;
        __asm__ volatile("syscall"
                         : "=a"(ret)
                         : "a"((uint64_t)SYS_PROCESS_WAIT_STATUS),
                           "D"(a0), "S"(a1), "d"(a2), "r"(r10), "r"(r8)
                         : "rcx", "r11", "memory");
        if (ret == K_EINTR) {
            continue;
        }
        return ret;
    }
}

/* Fork a child that immediately exits with `BRIEF_CHILD_OK`, for the 150s to
 * observe.  Returns the pid, or -1. */
static pid_t spawn_brief(void)
{
    const pid_t p = fork();
    if (p == 0) {
        _exit(BRIEF_CHILD_OK);
    }
    return p;
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

    /* ---------------------------------------------------------------- *
     * 150s — the extended ABI, reached raw.  See the file header for why
     * these cannot go through libc and cannot be done kernel-side either.
     *
     * Every child here is short-lived and reaped within its own block, so
     * the long-lived child's exit stays reserved for the 70s.
     * ---------------------------------------------------------------- */
    {
        union wait_info_buf wbuf;
        const uint64_t bufaddr = (uint64_t)(uintptr_t)&wbuf;
        const uint32_t myuid = (uint32_t)getuid();

        /* -- 150-163: a full-size request, and the zero-filled tail. -- */
        const pid_t k1 = spawn_brief();
        if (k1 < 0)                                     { rc = 150; goto done; }

        int st1 = 0;
        poison_buf(wbuf.raw, sizeof wbuf.raw);
        long r = wait_status_raw((uint64_t)(int64_t)k1, K_WINFO,
                                 (uint64_t)(uintptr_t)&st1,
                                 bufaddr, sizeof wbuf.raw);
        if (r != (long)k1)                              { rc = 151; goto done; }
        if (wbuf.info.pid != (uint64_t)k1)              { rc = 152; goto done; }
        /* The struct's wstatus is documented as "the same word written to
         * arg2", so compare the two rather than re-deriving the encoding. */
        if (wbuf.info.wstatus != st1)                   { rc = 153; goto done; }
        if (!WIFEXITED(st1) || WEXITSTATUS(st1) != BRIEF_CHILD_OK) {
            rc = 154; goto done;
        }
        /* si_uid, request item 3.  Before this the child's uid was destroyed
         * by the reap and libc had nothing to report but 0. */
        if (wbuf.info.uid != myuid)                     { rc = 155; goto done; }
        /* Both pads are zero, not left as we poisoned them: a kernel that
         * wrote only the named fields would leave 0x5A5A5A5A here, and a C
         * caller comparing whole structs would see spurious inequality. */
        if (wbuf.info.pad0 != 0 || wbuf.info.pad1 != 0) { rc = 156; goto done; }
        /* *** Lane A's ask #2 *** — arg4 = 128 and the kernel knows 72, so
         * bytes 72..128 must be zero-filled.  A newer libc on an older kernel
         * depends on exactly this: a field that kernel cannot fill must read
         * as "none", never as whatever was in the caller's buffer. */
        if (!all_zero(wbuf.raw + 72, sizeof wbuf.raw - 72)) {
            rc = 157; goto done;
        }
        /* The six counters were written.  Bounds rather than exact values,
         * because a brief child may legitimately use zero measurable CPU —
         * but an *unwritten* field is 0x5A5A5A5A5A5A5A5A (~6.5e18) and fails
         * every one of these, so "written" is still decisively proven. */
        if (wbuf.info.utime_us > 60000000ULL)           { rc = 158; goto done; }
        if (wbuf.info.stime_us > 60000000ULL)           { rc = 159; goto done; }
        if (wbuf.info.minflt > 1000000ULL)              { rc = 160; goto done; }
        if (wbuf.info.majflt > 1000000ULL)              { rc = 161; goto done; }
        if (wbuf.info.nvcsw > 1000000ULL)               { rc = 162; goto done; }
        if (wbuf.info.nivcsw > 1000000ULL)              { rc = 163; goto done; }

        /* -- 164-169: truncation to a size the caller declared. -- */
        const pid_t k2 = spawn_brief();
        if (k2 < 0)                                     { rc = 164; goto done; }

        poison_buf(wbuf.raw, sizeof wbuf.raw);
        /* arg2 = 0 skips the wstatus word, which also proves the two
         * out-parameters are independent. */
        r = wait_status_raw((uint64_t)(int64_t)k2, K_WINFO, 0, bufaddr, 24);
        if (r != (long)k2)                              { rc = 165; goto done; }
        if (wbuf.info.pid != (uint64_t)k2)              { rc = 166; goto done; }
        if (wbuf.info.uid != myuid)                     { rc = 167; goto done; }
        /* wstatus lives at 16..20, inside the 24 we asked for. */
        if (!WIFEXITED(wbuf.info.wstatus)
            || WEXITSTATUS(wbuf.info.wstatus) != BRIEF_CHILD_OK) {
            rc = 168; goto done;
        }
        /* *** Lane A's ask #1 *** — nothing at or past byte 24 was touched.
         * This is the case an older libc on a newer kernel is in, and getting
         * it wrong means the kernel writes past the end of a buffer that was
         * correctly sized for the ABI its caller was compiled against. */
        if (!all_poison(wbuf.raw + 24, sizeof wbuf.raw - 24)) {
            rc = 169; goto done;
        }

        /* -- 170-177: WNOWAIT peeks, and peeking twice sees it twice. -- */
        const pid_t k3 = spawn_brief();
        if (k3 < 0)                                     { rc = 170; goto done; }

        poison_buf(wbuf.raw, sizeof wbuf.raw);
        r = wait_status_raw((uint64_t)(int64_t)k3, K_WINFO | K_WNOWAIT, 0,
                            bufaddr, sizeof wbuf.raw);
        if (r != (long)k3)                              { rc = 171; goto done; }
        if (wbuf.info.pid != (uint64_t)k3)              { rc = 172; goto done; }

        /* The decisive check: an identical second call.  Without WNOWAIT the
         * first call would have reaped the zombie and this would be ECHILD,
         * so a kernel that ignored the bit fails here rather than silently
         * destroying the report its caller asked only to look at. */
        poison_buf(wbuf.raw, sizeof wbuf.raw);
        r = wait_status_raw((uint64_t)(int64_t)k3, K_WINFO | K_WNOWAIT, 0,
                            bufaddr, sizeof wbuf.raw);
        if (r != (long)k3)                              { rc = 173; goto done; }
        if (wbuf.info.pid != (uint64_t)k3)              { rc = 174; goto done; }
        if (!WIFEXITED(wbuf.info.wstatus)
            || WEXITSTATUS(wbuf.info.wstatus) != BRIEF_CHILD_OK) {
            rc = 175; goto done;
        }

        /* WNOWAIT is per-call, not sticky: dropping it reaps. */
        r = wait_status_raw((uint64_t)(int64_t)k3, 0, 0, 0, 0);
        if (r != (long)k3)                              { rc = 176; goto done; }
        r = wait_status_raw((uint64_t)(int64_t)k3, K_WNOHANG, 0, 0, 0);
        if (r != K_ENOCHILD)                            { rc = 177; goto done; }

        /* -- 178-187: naming a process group, including group 1. -- */
        const pid_t k4 = spawn_brief();
        if (k4 < 0)                                     { rc = 178; goto done; }

        /* Our children inherit our group, so naming the group finds k4.  The
         * long-lived child is in it too, but it is parked on a pipe read and
         * has no state change to report, so this cannot pick it up. */
        r = wait_status_raw((uint64_t)(uint32_t)getpgrp(), K_WPGID, 0, 0, 0);
        if (r != (long)k4)                              { rc = 179; goto done; }

        /* Group 1 is nameable now.  The answer must be ECHILD — we have no
         * child in group 1 — and specifically *not* ENOSYS, which is what
         * libc returned for years because `waitpid`'s signed selector spends
         * -1 on "any child" and so cannot spell -1 as a group. */
        siginfo_t gi;
        errno = 0;
        if (waitid_retry(P_PGID, 1, &gi, WEXITED | WNOHANG) != -1) {
            rc = 180; goto done;
        }
        if (errno == ENOSYS)                            { rc = 181; goto done; }
        if (errno != ECHILD)                            { rc = 182; goto done; }

        /* And the same path through libc rather than raw, so the wrapper's
         * WPGID translation is covered too. */
        const pid_t k5 = spawn_brief();
        if (k5 < 0)                                     { rc = 183; goto done; }

        if (waitid_retry(P_PGID, (id_t)getpgrp(), &gi, WEXITED) != 0) {
            rc = 184; goto done;
        }
        if (gi.si_pid != k5)                            { rc = 185; goto done; }
        if (gi.si_code != CLD_EXITED)                   { rc = 186; goto done; }
        if (gi.si_status != BRIEF_CHILD_OK)             { rc = 187; goto done; }
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
