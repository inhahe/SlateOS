/*
 * ctest-keylayout — the granted half of SYS_KEYLAYOUT_SET (1074).
 *
 * WHY THIS EXISTS.  Lane A's gated-dispatch probe already covers 1074: it
 * proves the number is registered and that the capability check runs before
 * argument validation.  What it cannot prove is that a caller *holding* the
 * right can actually change the layout, because the probe is only ever
 * refused — and from a refusal, "the gate refuses everyone" and "the gate
 * works" are the same observation.  That is the two-probe rule with one probe
 * available.  This fixture is the other one: it runs as init's child, holding
 * `Rights::SET_KEYLAYOUT`, and both sets a layout and gets refused, in the
 * same process, in a fixed order.
 *
 * WHY IT READS `/proc/keylayout` RATHER THAN CALLING A GETTER.  There is no
 * getter, deliberately: `/proc/keylayout` is the single read path, and lane A
 * declined to add a second for the reason `SYS_HOSTNAME_SET` gives — two
 * sources for one value can disagree.  So the check here is a genuine round
 * trip through an independent publisher rather than asking the setter whether
 * the setter worked.  That distinction is the whole point: `localectl` used to
 * "verify" a keymap by reading back the file it had just written itself, which
 * agreed every time and meant nothing.
 *
 * THE LAYOUT IS CHOSEN FROM WHAT THE KERNEL REPORTS, not hardcoded.  A fixture
 * naming `dvorak` would pass on a kernel that ships it and fail on one that
 * does not, for a reason having nothing to do with the syscall.  It parses the
 * list, picks any registered layout that is NOT currently active, and requires
 * at least two to exist — with its own exit code for "this kernel has fewer
 * than two layouts", so that case is reported as unrunnable rather than as a
 * failure of 1074.
 *
 * IT PUTS THE ORIGINAL LAYOUT BACK.  The boot test runs more rungs after this
 * one, and some of them read the console.  A fixture that leaves the keyboard
 * remapped would break a later test in a way nobody would connect to this
 * file.  The restore is checked too: a restore that silently failed would
 * leave exactly that landmine.
 *
 * Exit codes — every failing check gets its own, because "it failed" that
 * cannot say WHICH half failed is most of the investigation missing:
 *    42  every check passed
 *     1  cannot open /proc/keylayout (the node is absent or unreadable)
 *     2  /proc/keylayout has no `Active:` line — format changed
 *     3  fewer than two registered layouts, so nothing to switch TO
 *     4  set of a valid, registered layout was REFUSED
 *     5  set reported success and /proc/keylayout still shows the old layout
 *     6  set of an unregistered name SUCCEEDED — the gate does not refuse
 *     7  ...it failed, but the active layout changed anyway
 *     8  restoring the original layout was refused
 *     9  restore reported success and the layout did not come back
 *    10  read of /proc/keylayout failed partway (short or erroring read)
 */

#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* SlateOS-native. Declared here rather than included: the sysroot header set
 * has no home for a call no other system has, and a wrong prototype is a link
 * error rather than a surprise at runtime -- which is part of the test. */
extern int setkeylayout(const char *name, size_t len);

#define PROC_KEYLAYOUT "/proc/keylayout"
#define BUF_MAX 4096
#define NAME_MAX_LEN 64

/* Unbuffered, for `ctest-initfini`'s reason: a marker held in a stdio buffer
 * is a marker lost in exactly the run that needed it, because a hang never
 * flushes. Each is one short write so two cannot interleave mid-line. */
static void emit(const char *s)
{
    size_t n = 0;
    while (s[n] != '\0') {
        n++;
    }
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

/* Read the whole node into `buf`. Returns the length, or -1.
 *
 * A generated /proc node can come back in more than one read, and treating a
 * short read as the whole file is how a parser concludes "no Active: line"
 * about a file that has one. */
static int slurp(const char *path, char *buf, int cap)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        return -1;
    }
    int got = 0;
    for (;;) {
        if (got >= cap - 1) {
            break;
        }
        ssize_t n = read(fd, buf + got, (size_t)(cap - 1 - got));
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            (void)close(fd);
            return -1;
        }
        if (n == 0) {
            break;
        }
        got += (int)n;
    }
    (void)close(fd);
    buf[got] = '\0';
    return got;
}

/* Copy the value of the `Active:` line into `out`. Returns its length, or -1
 * when there is no such line. `(none)` comes back as length 0, which is the
 * kernel's way of saying no layout is active and is not an error. */
static int read_active(const char *buf, char *out, int cap)
{
    const char *p = strstr(buf, "Active:");
    if (p == NULL) {
        return -1;
    }
    p += 7;
    while (*p == ' ' || *p == '\t') {
        p++;
    }
    int n = 0;
    while (p[n] != '\0' && p[n] != '\n' && n < cap - 1) {
        n++;
    }
    if (n == 6 && strncmp(p, "(none)", 6) == 0) {
        out[0] = '\0';
        return 0;
    }
    memcpy(out, p, (size_t)n);
    out[n] = '\0';
    return n;
}

/* Find a registered layout whose name is not `avoid`.
 *
 * Rows are `*name: description` or ` name: description`; the first byte marks
 * the active one. The stat lines above them (`Layouts:`, `Active:` …) also
 * contain a colon, and start at column zero, which is what separates them. */
static int find_other_layout(const char *buf, const char *avoid, char *out, int cap)
{
    const char *line = buf;
    while (line != NULL && *line != '\0') {
        const char *end = strchr(line, '\n');
        int len = (end == NULL) ? (int)strlen(line) : (int)(end - line);
        if (len > 1 && (line[0] == '*' || line[0] == ' ')) {
            const char *colon = memchr(line + 1, ':', (size_t)(len - 1));
            if (colon != NULL) {
                int nlen = (int)(colon - (line + 1));
                if (nlen > 0 && nlen < cap - 1) {
                    char name[NAME_MAX_LEN + 1];
                    if (nlen <= NAME_MAX_LEN) {
                        memcpy(name, line + 1, (size_t)nlen);
                        name[nlen] = '\0';
                        if (strcmp(name, avoid) != 0) {
                            memcpy(out, name, (size_t)nlen + 1);
                            return nlen;
                        }
                    }
                }
            }
        }
        line = (end == NULL) ? NULL : end + 1;
    }
    return -1;
}

/* Re-read the node and report whether `Active:` now equals `want`. */
static int active_is(const char *want, int *io_error)
{
    char buf[BUF_MAX];
    char active[NAME_MAX_LEN + 1];
    *io_error = 0;
    if (slurp(PROC_KEYLAYOUT, buf, BUF_MAX) < 0) {
        *io_error = 1;
        return 0;
    }
    if (read_active(buf, active, sizeof active) < 0) {
        *io_error = 1;
        return 0;
    }
    return strcmp(active, want) == 0;
}

int main(void)
{
    char buf[BUF_MAX];
    char original[NAME_MAX_LEN + 1];
    char target[NAME_MAX_LEN + 1];
    int io_error = 0;

    emit("[kl] start (reading /proc/keylayout)\n");
    if (slurp(PROC_KEYLAYOUT, buf, BUF_MAX) < 0) {
        return 1;
    }
    if (read_active(buf, original, sizeof original) < 0) {
        return 2;
    }
    if (find_other_layout(buf, original, target, sizeof target) < 0) {
        /* Not a failure of 1074: there is simply nothing to switch to. Its own
         * code so it cannot be read as "the syscall is broken". */
        return 3;
    }

    /* PROBE ONE: it runs. A caller holding the right changes the layout, and
     * an independent publisher agrees. */
    emit("[kl] set (granted: does the layout actually change?)\n");
    if (setkeylayout(target, strlen(target)) != 0) {
        return 4;
    }
    if (!active_is(target, &io_error)) {
        return io_error ? 10 : 5;
    }

    /* PROBE TWO: it refuses. An unregistered name must be rejected, and must
     * leave the active layout alone -- a refusal that still moved something is
     * worse than an acceptance, because nothing downstream expects it. */
    emit("[kl] refuse (an unregistered name must be rejected)\n");
    if (setkeylayout("no-such-layout-xyzzy", 20) == 0) {
        return 6;
    }
    if (!active_is(target, &io_error)) {
        return io_error ? 10 : 7;
    }

    /* Put it back, and check. A fixture that leaves the console remapped
     * breaks a later rung in a way nobody would trace to this file. */
    emit("[kl] restore (later rungs read this console)\n");
    if (setkeylayout(original, strlen(original)) != 0) {
        return 8;
    }
    if (!active_is(original, &io_error)) {
        return io_error ? 10 : 9;
    }

    emit("[kl] ok (set, refused, restored)\n");
    return 42;
}
