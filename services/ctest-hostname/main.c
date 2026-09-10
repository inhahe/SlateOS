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
        return 1;
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
     * ---------------------------------------------------------------- */
    memset(buf, 0, sizeof buf);
    if (gethostname(buf, sizeof buf - 1) != 0)
        FAIL(6);
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
     *     normal — Linux reports the literal "(none)" — so this checks
     *     only that the call works.
     * ---------------------------------------------------------------- */
    memset(orig_domain, 0, sizeof orig_domain);
    if (getdomainname(orig_domain, sizeof orig_domain - 1) != 0)
        return 13;

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

    /* 17. Accepted and kept. */
    memset(buf, 0, sizeof buf);
    if (getdomainname(buf, sizeof buf - 1) != 0)
        FAIL(17);
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
