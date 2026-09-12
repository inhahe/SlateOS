/*
 * ctest-hostname — ring-3 regression test for `sethostname`/`setdomainname`
 * and for the claim that the system has exactly one name.
 *
 * Guards `known-issues.md` →
 * A-SYSFS-KEEPS-A-THIRD-HOSTNAME-THAT-NOTHING-ELSE-READS, and the pair of
 * requests that produced the syscalls it exercises:
 * `requests/b-a-no-native-syscall-reports-the-hostname.md` and
 * `requests/b-a-the-first-grant-of-set-hostname-cannot-come-from-my-lane.md`.
 *
 * ## Why a fixture and not a unit test
 *
 * Because every part of this that can be wrong is invisible from the host.
 *
 * `posix`'s own tests run on the development host, where there is no kernel
 * hostname to set: before 2026-09-09 `sethostname` wrote a process-local
 * buffer and `gethostname` read that same buffer back, and a round-trip test
 * asserted they agreed. It passed for the whole life of the defect, because a
 * round trip through one buffer is evidence about the buffer and reads
 * exactly like evidence about the system. `setdomainname` kept the entire
 * defect a commit longer than `sethostname` did, under a commit message
 * claiming both were fixed.
 *
 * So the property worth testing is not "does the value come back" — that is
 * the property that lied. It is "do all the ways of asking agree", which
 * needs a running kernel with a real procfs, and a caller that actually holds
 * `(Process, SET_HOSTNAME)`.
 *
 * ## The three failure classes this separates
 *
 * Lane A's rung reports these by name; the exit codes below are the
 * authoritative mapping.
 *
 *   - **refused** (`EPERM`) — the `(Process, SET_HOSTNAME)` grant did not
 *     reach this process. Nothing is wired wrong; the fixture simply is not
 *     privileged. Checks 4 and 15.
 *   - **`ENOSYS`** — libc is not wired to `SYS_HOSTNAME_SET` (1072) /
 *     `SYS_DOMAINNAME_SET` (1073). This is the expected answer until lane B
 *     wires `posix`, and it is deliberately distinct from `EPERM`: an
 *     unprivileged caller is permanently unprivileged, an unimplemented call
 *     is not. Checks 3 and 14.
 *   - **read-back-unchanged** — the kernel accepted the name and dropped it.
 *     Checks 6 and 17.
 *   - **read-failed-after-a-successful-set** -- the set worked and the reader
 *     could not run. Checks 22 and 23, split out of 6 and 17 on 2026-09-10
 *     after a refused `open` of `/proc/sys/kernel/hostname` -- the rung held
 *     `SET_HOSTNAME` but not `(File, READ)` -- spent a boot test looking like
 *     the kernel dropping the name. Two faults, one exit code, three layers
 *     between the symptom and the cause.
 *   - **the-baseline-read-failed, by layer** -- codes 24-28, added 2026-09-12
 *     for the same reason one day later. Check 13 asked only whether
 *     `getdomainname` returned zero, so a failure said nothing about WHERE.
 *     Lane A traced the entire kernel-side read path by hand, proved every
 *     layer symmetric with the hostname's, and still could not say whether the
 *     fault was the ring-3 `open()` or libc -- because a fixture that can only
 *     report "the call failed" makes hand-tracing the only way forward.
 *     Now: 25 the buffer, 26 an unexpected errno, 13 a failure that set no
 *     errno at all, and the pair that matters -- **27** the same node cannot be
 *     opened directly from ring 3 either (kernel side, since the *hostname*
 *     node opened from this very process minutes earlier), **28** it opens
 *     fine and libc is not getting the bytes (lane B's, and no kernel change
 *     fixes it). Neither of lane A's ring-0 rungs can tell 27 from 28: both
 *     read through the internal VFS call, so the ring-3 open is the one layer
 *     nothing covers.
 *
 * The round trip is only evidence because its ends differ. See check 6 --
 * the write goes through `SYS_HOSTNAME_SET` and the read through
 * `/proc/sys/kernel/hostname`, and if those ever share an implementation this
 * fixture goes back to testing a buffer against itself.
 *
 * And one class that only a fixture reading *two* sources can see:
 *
 *   - **disagreement** — the set succeeded, `gethostname` reports the new
 *     name, and another view of the same value does not. Checks 7, 8 and 18.
 *     This is the shape that has bitten this project three times (sysfs's
 *     private `HOSTNAME` static, `uname` vs `/proc`, the process-local
 *     buffer), and it is the reason lane A declined to add a hostname
 *     *getter* syscall: one value with two sources can disagree.
 *
 * ## It cannot hang
 *
 * Nothing here waits, sleeps, or reads from anything with a writer. The two
 * `/proc` reads are of generated procfs nodes, which materialise their whole
 * contents at `open` time and return them synchronously — there is no peer to
 * block on, and a `read` that returns 0 or -1 is handled as a failed check
 * rather than retried. `ctest-pty` once cost the kernel lane two hours by
 * blocking a boot test on a read that could not return; that is why this
 * paragraph exists and why there is no loop around any `read` below.
 *
 * ## It restores what it changes
 *
 * The hostname and domain name are system-wide, and the boot test continues
 * after this fixture exits. Every exit path that has already mutated a name
 * goes through `FAIL`, which puts the original back first. A failure still
 * reddens the run — that is the point — but it does not leave the machine
 * called something else for whatever runs next.
 *
 * **This has a real cost, and it is worth knowing before you change it.**
 * Because the name is put back on the *failing* path too, the machine's state
 * after a red run tells you nothing about which check failed -- the evidence
 * is destroyed by the same code that protects the next test. Lane A hit this
 * diagnosing the first execution of this fixture on 2026-09-10: reading the
 * hostname after the rung showed the original, so the only way forward was to
 * instrument the syscall itself. That cost about an hour and found a real
 * kernel-side capability bug (the rung granted `(Process, SET_HOSTNAME)` and
 * no `File` right, so this fixture's `open` of `/proc/sys/kernel/hostname`
 * was refused).
 *
 * The trade is deliberate and stands: a fixture that can leave a shared
 * machine misnamed is worse than one that is hard to autopsy, and the exit
 * codes -- not the surviving state -- are meant to be the diagnostic. That is
 * also why the codes were split afterwards, so that 22 and 6 distinguish
 * "the read path is broken" from "the store is" rather than sharing one
 * number. If you ever find the codes insufficient, add a code; do not remove
 * the restore.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below).
 */

#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/utsname.h>
#include <unistd.h>

/*
 * `__NEW_UTS_LEN` is 64 on Linux and the kernel bounds both calls at 0..=64.
 * The buffers are generous so that a kernel returning more than it should is
 * a failed comparison rather than a smashed stack.
 */
#define NAME_MAX_LEN 64
#define BUF 512

static char orig_host[BUF];
static char orig_domain[BUF];
static int host_dirty;
static int domain_dirty;

/* Put back whatever we changed, best effort — we are already failing. */
static void restore(void)
{
    if (host_dirty)
        (void)sethostname(orig_host, strlen(orig_host));
    if (domain_dirty)
        (void)setdomainname(orig_domain, strlen(orig_domain));
}

#define FAIL(n)                                                                \
    do {                                                                       \
        restore();                                                             \
        return (n);                                                            \
    } while (0)

/*
 * Read a whole generated procfs file into `out`, NUL-terminated, with any
 * single trailing newline removed. Returns 0 on success, -1 otherwise.
 *
 * One `read`, no loop: these nodes are generated in full at `open` and a short
 * read from one would be a kernel bug, not a condition to retry around. A loop
 * here is how a fixture stops terminating.
 */
static int read_proc_line(const char *path, char *out, size_t cap)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0)
        return -1;

    ssize_t n = read(fd, out, cap - 1);
    (void)close(fd);
    if (n < 0)
        return -1;

    out[n] = '\0';
    size_t len = (size_t)n;
    if (len > 0 && out[len - 1] == '\n')
        out[len - 1] = '\0';
    return 0;
}

int main(void)
{
    char buf[BUF];

    /* ---------------------------------------------------------------- *
     * 1-2. Baseline: the system can say what it is called.
     * ---------------------------------------------------------------- */
    memset(orig_host, 0, sizeof orig_host);
    if (gethostname(orig_host, sizeof orig_host - 1) != 0)
        return 1; /* the read path is broken before anything was set */
    if (orig_host[0] == '\0')
        return 2;

    /* ---------------------------------------------------------------- *
     * 3-5. Set a name. The three ways this can be refused are three
     * different diagnoses, so they get three different codes.
     * ---------------------------------------------------------------- */
    static const char probe_host[] = "ctest-hostname";

    errno = 0;
    if (sethostname(probe_host, strlen(probe_host)) != 0) {
        if (errno == ENOSYS)
            return 3; /* libc not wired to SYS_HOSTNAME_SET (1072) */
        if (errno == EPERM)
            return 4; /* (Process, SET_HOSTNAME) did not reach this process */
        return 5;     /* something else entirely; see the rung's serial log */
    }
    host_dirty = 1;

    /* ---------------------------------------------------------------- *
     * 6. It took. This is the "accepted and dropped it" check, and it is
     *    the one no host test could ever have made.
     *
     *    READ THIS BEFORE CHANGING THE NEXT FOUR LINES. This check is a
     *    round trip, and a round trip is exactly what the defect this
     *    fixture guards was made of: `sethostname` wrote a process-local
     *    buffer, `gethostname` read that same buffer, they agreed, and the
     *    agreement WAS the evidence it worked. The test passed for the whole
     *    life of the bug.
     *
     *    What makes this round trip valid is that the two ends are now
     *    genuinely different mechanisms. The write goes through
     *    `SYS_HOSTNAME_SET` into the kernel's `fs::nameservice`; the read
     *    goes through `gethostname`, which libc serves by reading
     *    `/proc/sys/kernel/hostname` -- a file the kernel generates from
     *    that same store, through code that shares nothing with the setter.
     *
     *    So the invariant is: **the write path and the read path must not
     *    share an implementation.** If `gethostname` is ever "optimised" to
     *    answer from a cached buffer that `sethostname` fills, this check
     *    silently becomes self-referential again, still passes, and stops
     *    testing anything. Check 7 exists partly as insurance against that,
     *    since it reads the file directly -- but 6 is the one that would
     *    rot quietly, so the warning belongs here.
     *
     *    (Lane A raised this, correctly, having taken the point from my own
     *    report of the original four tests that asserted the bug.)
     * ---------------------------------------------------------------- */
    memset(buf, 0, sizeof buf);
    /*
     * 22 vs 6: the READ FAILING and the read returning the WRONG NAME are
     * different faults and used to share this exit code. Lane A spent a boot
     * test separating them on 2026-09-10: their rung granted
     * (Process, SET_HOSTNAME) but not (File, READ), so this program's open of
     * /proc/sys/kernel/hostname was refused, libc fell back to a per-process
     * buffer holding "localhost", and check 6 reported "the name did not
     * change" -- true, and three layers away from the cause.
     *
     * libc no longer falls back, so a refused open now surfaces here as
     * gethostname returning -1. That deserves its own code: 22 means the read
     * path is broken, 6 means the store is.
     */
    if (gethostname(buf, sizeof buf - 1) != 0)
        FAIL(22);
    if (strcmp(buf, probe_host) != 0)
        FAIL(6);

    /* ---------------------------------------------------------------- *
     * 7. The second source agrees. `/proc/sys/kernel/hostname` is what
     *    `osh` fills `$HOSTNAME` from and what lane A kept as the only
     *    read path rather than adding a getter syscall — so if it and
     *    `gethostname` disagree, the reason that decision was made has
     *    stopped holding.
     * ---------------------------------------------------------------- */
    memset(buf, 0, sizeof buf);
    if (read_proc_line("/proc/sys/kernel/hostname", buf, sizeof buf) != 0)
        FAIL(7);
    if (strcmp(buf, probe_host) != 0)
        FAIL(7);

    /* ---------------------------------------------------------------- *
     * 8. The third. `uname` publishes the same value through a struct
     *    whose layout is musl's here and was the kernel's in `sigaction`
     *    until 2026-09-09 — so this compares the value *and* incidentally
     *    that `struct utsname` agrees across the ABI boundary. A failure
     *    here with 6 and 7 passing means the publishers have diverged,
     *    not that the hostname is wrong.
     * ---------------------------------------------------------------- */
    {
        struct utsname uts;
        memset(&uts, 0, sizeof uts);
        if (uname(&uts) != 0)
            FAIL(8);
        if (strcmp(uts.nodename, probe_host) != 0)
            FAIL(8);
    }

    /* ---------------------------------------------------------------- *
     * 9-11. The bound is enforced, and enforced *before* the store.
     *
     * 65 bytes is one past `__NEW_UTS_LEN`. A kernel that validated after
     * writing would refuse this call and still have changed the name,
     * which is why 11 re-reads rather than trusting 9 and 10.
     * ---------------------------------------------------------------- */
    {
        char toolong[NAME_MAX_LEN + 2];
        memset(toolong, 'x', sizeof toolong - 1);
        toolong[sizeof toolong - 1] = '\0';

        errno = 0;
        if (sethostname(toolong, strlen(toolong)) == 0)
            FAIL(9);
        if (errno != EINVAL)
            FAIL(10);

        memset(buf, 0, sizeof buf);
        if (gethostname(buf, sizeof buf - 1) != 0)
            FAIL(11);
        if (strcmp(buf, probe_host) != 0)
            FAIL(11);
    }

    /* ---------------------------------------------------------------- *
     * 12. Put the machine's own name back before touching the domain, so
     *     a later failure cannot leave both wrong.
     * ---------------------------------------------------------------- */
    if (sethostname(orig_host, strlen(orig_host)) != 0)
        FAIL(12);
    host_dirty = 0;

    /* ---------------------------------------------------------------- *
     * 13. Baseline for the domain. Unlike the hostname an empty domain is
     *     normal, so this checks only that the call works and asserts
     *     nothing about the value.
     *
     *     This comment used to add "Linux reports the literal (none)".
     *     True of Linux and false of us: our kernel's init_defaults sets
     *     the domain to "localdomain" (nameservice.rs), so an unset domain
     *     reads back as that. The sentence was a claim about ANOTHER LANE'S
     *     defaults sitting in a fixture comment, which is a place nobody
     *     checks it against the source — lane A found it by reading my
     *     libc, not by running anything. Lane A intends to change the
     *     kernel to (none) for the same reason we deleted "localhost":
     *     a plausible value makes unset indistinguishable from configured.
     *
     *     This check is deliberately value-blind, so it is correct either
     *     way — and so is every other one: 17 and 18 compare against the
     *     probe this fixture SET, and 21 restores whatever 13 read. No
     *     check anywhere asserts the default. Checked before writing it
     *     down, because the first version of this sentence hedged that
     *     "17-21 would have to move" and that was a guess.
     * ---------------------------------------------------------------- */
    memset(orig_domain, 0, sizeof orig_domain);
    errno = 0;
    if (getdomainname(orig_domain, sizeof orig_domain - 1) != 0) {
        /*
         * 24-28. WHICH WAY IT FAILED, because "13" on its own cost two lanes
         * an evening.
         *
         * libc returns EIO only when the one source could not be READ -- the
         * open or the read itself -- and EINVAL only when the caller's buffer
         * is too small. Those land in different subsystems and 13 named
         * neither. Same repair as 22 and 23, in the place that rebuild did
         * not reach.
         *
         * THE ERRNO IS SAVED BEFORE THE PROBE BELOW RUNS. `read_proc_line`
         * calls open() and read(), both of which set errno, so testing errno
         * afterwards would report the probe's outcome while claiming to
         * report getdomainname's. The first draft of this block did exactly
         * that -- a diagnostic that lies is worse than 13, which at least
         * only failed to say anything.
         *
         * THE PROBE IS THE DECISIVE PART. It opens the same node directly,
         * the way check 18 does and the way check 6 already opened the
         * HOSTNAME node successfully earlier in this same process:
         *
         *   27 - the direct open/read ALSO fails. The hostname node opened
         *        from this very process and this one does not, so it is the
         *        ring-3 open of this specific node. Kernel side.
         *   28 - the direct open/read SUCCEEDS while getdomainname fails.
         *        The bytes are reachable from ring 3 and libc is not getting
         *        them. Lane B's, and no kernel change will fix it.
         *   25 - the buffer was too small. Neither lane; this fixture.
         *   26 - some other errno, printed so it is not swallowed.
         *   13 - the call failed and set no errno at all, which is its own
         *        bug and is now distinguishable from all of the above.
         *
         * Neither of lane A's ring-0 rungs can tell 27 from 28: both read
         * through the internal VFS call, so the ring-3 open is the one layer
         * nothing covers. Rather than ask for a third rung, the fixture that
         * is already standing in ring 3 answers it.
         */
        int saved = errno;
        char probe[BUF];
        int direct;

        memset(probe, 0, sizeof probe);
        direct = read_proc_line("/proc/sys/kernel/domainname", probe, sizeof probe);

        if (saved == EINVAL)
            return 25;
        if (saved == EIO)
            return direct != 0 ? 27 : 28;
        if (saved != 0)
            return 26;
        return direct != 0 ? 27 : 13;
    }

    /* ---------------------------------------------------------------- *
     * 14-16. The same three diagnoses for the domain. `SYS_DOMAINNAME_SET`
     *        (1073) is documented as taking the same capability and the
     *        same errors as 1072, so a split result here — say 14 with the
     *        hostname half passing — means the two handlers have diverged.
     * ---------------------------------------------------------------- */
    static const char probe_domain[] = "ctest.invalid";

    errno = 0;
    if (setdomainname(probe_domain, strlen(probe_domain)) != 0) {
        if (errno == ENOSYS)
            FAIL(14);
        if (errno == EPERM)
            FAIL(15);
        FAIL(16);
    }
    domain_dirty = 1;

    /* 17. Accepted and kept; 23 if the read itself failed. Same split as
     * 6 and 22, for the same reason. */
    memset(buf, 0, sizeof buf);
    if (getdomainname(buf, sizeof buf - 1) != 0)
        FAIL(23);
    if (strcmp(buf, probe_domain) != 0)
        FAIL(17);

    /* 18. And the second source agrees, as for the hostname. */
    memset(buf, 0, sizeof buf);
    if (read_proc_line("/proc/sys/kernel/domainname", buf, sizeof buf) != 0)
        FAIL(18);
    if (strcmp(buf, probe_domain) != 0)
        FAIL(18);

    /* 19-20. The same bound, on the same evidence. */
    {
        char toolong[NAME_MAX_LEN + 2];
        memset(toolong, 'y', sizeof toolong - 1);
        toolong[sizeof toolong - 1] = '\0';

        errno = 0;
        if (setdomainname(toolong, strlen(toolong)) == 0)
            FAIL(19);
        if (errno != EINVAL)
            FAIL(20);
    }

    /* 21. Put the domain back. */
    if (setdomainname(orig_domain, strlen(orig_domain)) != 0)
        FAIL(21);
    domain_dirty = 0;

    return 42;
}
