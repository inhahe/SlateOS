/*
 * ctest-python-repl — CPython's INTERACTIVE path, on a real pty, in ring 3.
 *
 * WHY THIS EXISTS.  `roadmap.md` has said for over a week that the CPython
 * initiative's remaining work is one sentence: "Nobody has ever run it
 * interactively. That is the actual state, and no measurement here replaces
 * doing so."  Everything around that line argues the path *should* work —
 * `isatty` is real, the pty line discipline handles `ISIG`, `sshd` already
 * hosts a login shell on a pty, and `self_test_cpython_on_slateos_libc`
 * proves the interpreter starts and produces byte-exact output.  None of it
 * says the REPL does.
 *
 * This is that measurement, automated so it can be repeated: a pty pair, an
 * interpreter on the slave end, an expression typed into the master, and the
 * answer read back out.
 *
 * WHAT IT COVERS THAT THE EXISTING RUNG DOES NOT.  `self_test_cpython_*` runs
 * `python3 -c` with stdout to a pipe.  That exercises startup, the import of
 * `encodings` out of the 20 MiB zip, and the write path — and nothing about a
 * terminal.  Between them and here lie: `isatty` answering true on the slave,
 * CPython choosing its interactive branch because of it, the line discipline
 * assembling a typed line and delivering it on ENTER, and the prompt/echo
 * traffic coming back the other way.  Those are exactly the parts a `-c` run
 * cannot reach, and exactly the parts a user meets first.
 *
 * WHY `-q -i -u`.
 *   `-q`  no version banner, so the output this scans is the answer rather
 *         than a build string that happens to contain digits.
 *   `-i`  force interactive even if the tty detection is what is broken.  That
 *         is deliberate: if `isatty` is wrong, this fixture should fail on the
 *         REPL rather than silently degrade to batch mode and pass.
 *   `-u`  unbuffered, so an answer cannot sit in a stdio buffer while this
 *         spins to its budget and calls it a timeout.
 *
 * THE EXPRESSION IS `6*7`, NOT `1+1`.  A pty echoes what is typed, so the
 * master sees the expression before it sees the answer.  With `print(1+1)`
 * the answer `2` also appears inside the echoed text, so a scan for it
 * matches the echo and passes without the interpreter having done anything.
 * `6*7` does not contain `42` — the only way `42` appears is if something
 * evaluated it.  That is the whole reason for the odd-looking choice, and it
 * is the same class of error as a test that reads back its own writes.
 *
 * BOUNDS.  Structural, and copied from `ctest-pty` because they are proven
 * there: every read is gated on `poll(POLLIN|POLLHUP, 0)`, and the wait is a
 * counted spin with `sched_yield()` per iteration.  Deliberately no `alarm` —
 * the timers work now, but a fixture's bounds should not depend on a subsystem
 * other than the one under test, or a timer regression surfaces here as a
 * CPython failure and points at the wrong place.
 *
 * The startup budget is separate and much larger than the steady-state one.
 * Starting this interpreter means mapping an 11 MiB binary and reading
 * `encodings` out of a 20 MiB archive off ext4; the gap before the FIRST byte
 * is nothing like the gap between two bytes of one line, and a single budget
 * sized for either is wrong for the other.
 *
 * Exit codes — one per check, because a failure that cannot say which half
 * failed is most of the investigation missing:
 *    42  the interpreter evaluated the expression and returned the answer
 *     1  forkpty() failed
 *     2  writing the expression to the master failed
 *     3  the interpreter produced NO output at all — it never started
 *     4  it produced output, but the answer never appeared
 *     5  waitpid() failed or returned the wrong pid
 *     6  the interpreter exited non-zero after being asked to quit
 *     7  writing the quit command failed
 *     8  /bin/python3 could not be EXEC'd -- missing from the image or not
 *        executable. A fact about the image, not about the pty or the REPL.
 *
 * On 3 and 4 the fixture also prints what it DID read from the master --
 * escaped, the first KiB -- and how the interpreter ended, so the serial log
 * carries the evidence: a traceback, a "Fatal Python error", a prompt with
 * nothing after it. Until 2026-09-26 it printed neither, and exit 4 sat red
 * on every boot with nothing to say which of those it was.
 *
 * 8 exists because 2 was standing for it. The child execs `/bin/python3` and
 * `_exit(127)`s if that fails; the parent then writes to the master WITHOUT
 * having reaped it, the slave is already closed, the write gets EIO, and the
 * fixture answered "writing the expression to the master failed". That sent a
 * reader to the pty layer for what was a missing file.
 *
 * It was caught because two fixtures disagreed: this one reported the master
 * write failing in the same boot that `ctest-pty` reported it working. The
 * difference is that `ctest-pty`'s child execs nothing, so its slave stays
 * open. Two fixtures disagreeing about one primitive is worth more than either
 * result on its own -- neither was wrong, and the disagreement was the finding.
 *
 * `/bin/python3` is NOT in `scripts/rootfs-bin-manifest.txt`; it is staged by
 * its own block in `create-ext4-rootfs.sh`, so its absence is a separate
 * failure from an empty manifest staging and has to be reported separately.
 */

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

/* No <pty.h> in the sysroot header set; the symbol is ours, from
 * `posix/src/pty.rs`. A wrong prototype is a link error here rather than a
 * surprise at runtime, which is part of what this exercises. */
extern int forkpty(int *amaster, char *name, const void *termp, const void *winp);

/* `sched_yield()` with no <sched.h> in the sysroot. `ctest-pty` explains the
 * reasoning at length: `poll(..., 0)` must not block and is therefore not a
 * yield primitive, so a spin built only on it can hold a CPU the writer needs. */
extern int sched_yield(void);

/* Where the staged binaries are AT RUNTIME. The image stages into `/bin` and
 * the kernel mounts it at `/mnt`, so a running process execs `/mnt/bin/...`.
 * See `ctest-coreutils-runs/main.c` for the full account -- this fixture had
 * the same fault and would have reported it as exit 8, "python3 is missing",
 * which would have been just as false and just as checkable. */
#define BIN "/mnt/bin/"

/* Steady state: the gap between two bytes of one line. */
#define SPIN 2000000L
/* Startup: an 11 MiB binary and a 20 MiB zip, before the first byte. */
#define STARTUP_SPIN 40000000L

#define CAP 8192

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

/* Write `v` in decimal. */
static void emit_long(long v)
{
    char d[24];
    size_t i = sizeof d - 1;
    unsigned long u = (v < 0) ? (unsigned long)(-(v + 1)) + 1UL : (unsigned long)v;
    d[i] = '\0';
    do {
        d[--i] = (char)('0' + (int)(u % 10UL));
        u /= 10UL;
    } while (u != 0UL && i > 1);
    if (v < 0) {
        d[--i] = '-';
    }
    emit(d + i);
}

/* Write the first KiB of what the interpreter sent, every byte that is not
 * printable ASCII shown as \xHH, so an echo, a prompt and an error message can
 * be told apart in the serial log. A failure's evidence, not a transcript. */
static void emit_received(const char *buf, long n)
{
    static const char hex[] = "0123456789abcdef";
    char out[4 * 1024 + 1];
    size_t o = 0;
    if (n > 1024) {
        n = 1024;
    }
    for (long i = 0; i < n; i++) {
        unsigned char c = (unsigned char)buf[i];
        if (c >= 0x20 && c < 0x7f && c != '\\') {
            out[o++] = (char)c;
        } else {
            out[o++] = '\\';
            out[o++] = 'x';
            out[o++] = hex[c >> 4];
            out[o++] = hex[c & 0xf];
        }
    }
    out[o] = '\0';
    emit(out);
}

/* How did the interpreter end -- or has it not? Bounded like every other wait
 * here: a counted spin on `WNOHANG` with `sched_yield`. */
static void report_child(pid_t child)
{
    for (long i = 0; i < SPIN; i++) {
        int status = 0;
        pid_t got = waitpid(child, &status, WNOHANG);
        if (got == child) {
            if (WIFEXITED(status)) {
                emit("[py] the interpreter exited with status ");
                emit_long((long)WEXITSTATUS(status));
            } else if (WIFSIGNALED(status)) {
                emit("[py] the interpreter was ended by signal ");
                emit_long((long)WTERMSIG(status));
            } else {
                emit("[py] the interpreter stopped, status word ");
                emit_long((long)status);
            }
            emit("\n");
            return;
        }
        if (got < 0) {
            emit("[py] waitpid could not say how the interpreter ended\n");
            return;
        }
        sched_yield();
    }
    emit("[py] the interpreter is still running\n");
}

/* Is `fd` readable right now? Zero timeout, so this never waits.
 *
 * POLLHUP counts: a read at hangup returns 0 immediately rather than blocking,
 * and treating it as "not ready" would spin to the budget and then report a
 * timeout for what is really an EOF. */
static int readable(int fd)
{
    struct pollfd pfd;
    pfd.fd = fd;
    pfd.events = POLLIN;
    pfd.revents = 0;
    if (poll(&pfd, 1, 0) <= 0) {
        return 0;
    }
    return (pfd.revents & (POLLIN | POLLHUP)) != 0;
}

/* Accumulate from `fd` until `needle` appears in what has been read.
 *
 * Returns 1 on a match, 0 on the budget running out or EOF. `*total` is left
 * holding how many bytes ever arrived, so the caller can tell "nothing ever
 * came back" (the interpreter did not start) from "plenty came back and none
 * of it was the answer" — two very different findings that a single boolean
 * would flatten into one.
 *
 * The budget resets on progress, so it bounds the gap between bytes rather
 * than the whole exchange. A slow-but-advancing interpreter is not a hang. */
static int scan_for(int fd, const char *needle, char *buf, long budget, long *total)
{
    long got = *total;
    size_t nlen = strlen(needle);
    for (long i = 0; i < budget; i++) {
        if (got >= CAP - 1) {
            break; /* buffer full: whatever this is, it is not our one line */
        }
        if (!readable(fd)) {
            sched_yield();
            continue;
        }
        ssize_t n = read(fd, buf + got, (size_t)(CAP - 1 - got));
        if (n > 0) {
            got += (long)n;
            buf[got] = '\0';
            *total = got;
            if (nlen != 0 && strstr(buf, needle) != (char *)0) {
                return 1;
            }
            i = 0; /* progress: give the next byte a full budget */
        } else if (n == 0) {
            break; /* EOF */
        } else if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) {
            break;
        }
    }
    *total = got;
    return 0;
}

static int write_all(int fd, const char *s)
{
    size_t len = strlen(s);
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, s + off, len - off);
        if (n > 0) {
            off += (size_t)n;
        } else if (n < 0 && errno == EINTR) {
            continue;
        } else {
            return -1;
        }
    }
    return 0;
}

/* Did the child die without ever becoming the interpreter?
 *
 * Non-zero only when it is REAPED and its status is the `_exit(127)` that sits
 * below the `execl`. A child that is still running, or that exited for any
 * other reason, answers zero -- so this can only ever turn a pty verdict into
 * an image verdict when the image really is the cause, and never the reverse.
 *
 * Bounded like every other wait here: a counted spin on `WNOHANG` with
 * `sched_yield`, and no `alarm`, so the fixture's bounds do not depend on a
 * subsystem other than the one under test. The budget is small because this is
 * only reached after a write has already failed, by which point the child has
 * either died or is not the reason.
 */
static int exec_failed(pid_t child)
{
    for (long i = 0; i < SPIN; i++) {
        int status = 0;
        pid_t got = waitpid(child, &status, WNOHANG);
        if (got == child) {
            return WIFEXITED(status) && WEXITSTATUS(status) == 127;
        }
        if (got < 0) {
            return 0; /* cannot tell; do not invent a verdict */
        }
        sched_yield();
    }
    return 0;
}

/* ---------------------------------------------------------------------------
 * THE STDIO PROBE (2026-09-27) -- CPython's line read, without CPython.
 *
 * On the boot of c721b2a13 the interpreter read `print(6*7)` as five lines of
 * three bytes each -- `\x80\x1b0`, `@\x973`, `P\x9b3`, `\xc0C3`, `\xd0G3` --
 * printed five SyntaxErrors, never printed its `>>> ` prompt, and exited 0.
 * Each "line" is a little-endian pointer into libc malloc's own regions
 * (0x6000301b80, 0x6000339740, ...) cut short by the pointer's zero byte:
 * dlmalloc's free-list link, left in a block that was recycled. So what the
 * tokenizer read was a buffer nothing had written -- which, in
 * `PyOS_StdioReadline`, means `fgets` said "done" without putting a byte in it,
 * or wrote somewhere else.
 *
 * Whether that is this libc's stdio on a pty or something only the
 * interpreter does is the first question, and one boot can answer it: do
 * exactly what CPython 3.12 does -- `-u` makes `config_init_stdio` set all
 * three streams `_IONBF`; `PyOS_StdioReadline` flushes stdout, prints the
 * prompt to stderr with `fprintf`, `realloc`s a 100-byte buffer from NULL and
 * `fgets` into it -- in a forked child on a pty of its own, and report what
 * came back with `write`, not stdio. Then the same without `setvbuf`. The
 * verdict of this fixture is still the interpreter's; the probe only prints.
 *
 * THE ANSWER was neither, and the probe could not have given it: C's `stdin`
 * was a NULL pointer (the library exported the integers 0, 1 and 2 for the
 * three streams), and CPython's tokenizer takes `fp == NULL` for "the input
 * is a string" -- so the interpreter never called `fgets` at all, and parsed
 * its own uninitialised buffer. Every stdio call made with that NULL worked,
 * which is why a probe that makes the calls passes; only a comparison with
 * NULL fails. Fixed 2026-09-27 (known-issues.md,
 * `D-POSIX-STDIN-WAS-A-NULL-POINTER`). The probe stays as a check that a
 * line typed at a pty reaches `fgets`, buffered and not.
 * ------------------------------------------------------------------------- */

static size_t append(char *out, size_t o, size_t cap, const char *s)
{
    while (*s != '\0' && o + 1 < cap) {
        out[o++] = *s++;
    }
    out[o] = '\0';
    return o;
}

static size_t append_long(char *out, size_t o, size_t cap, long v)
{
    char d[24];
    size_t i = sizeof d - 1;
    unsigned long u = (v < 0) ? (unsigned long)(-(v + 1)) + 1UL : (unsigned long)v;
    d[i] = '\0';
    do {
        d[--i] = (char)('0' + (int)(u % 10UL));
        u /= 10UL;
    } while (u != 0UL && i > 1);
    if (v < 0) {
        d[--i] = '-';
    }
    return append(out, o, cap, d + i);
}

static size_t append_hex(char *out, size_t o, size_t cap, const unsigned char *p, size_t n)
{
    static const char hex[] = "0123456789abcdef";
    for (size_t i = 0; i < n && o + 4 < cap; i++) {
        out[o++] = hex[p[i] >> 4];
        out[o++] = hex[p[i] & 0xf];
        out[o++] = ' ';
    }
    out[o] = '\0';
    return o;
}

static void stdio_probe_child(int unbuffered)
{
    if (unbuffered) {
        setvbuf(stdin, (char *)NULL, _IONBF, BUFSIZ);
        setvbuf(stdout, (char *)NULL, _IONBF, BUFSIZ);
        setvbuf(stderr, (char *)NULL, _IONBF, BUFSIZ);
    }
    int fl = fcntl(0, F_GETFL);
    int tty = isatty(fileno(stdin));
    fflush(stdout);
    fprintf(stderr, "%s", ">>> ");
    fflush(stderr);
    char *p = (char *)realloc((void *)0, 100);
    if (p == (char *)0) {
        _exit(3);
    }
    memset(p, 'Z', 100);
    p[99] = '\0';
    errno = 0;
    clearerr(stdin);
    char *r = fgets(p, 100, stdin);
    int err = errno;
    char out[1024];
    size_t o = 0;
    o = append(out, o, sizeof out, "{probe isatty=");
    o = append_long(out, o, sizeof out, (long)tty);
    o = append(out, o, sizeof out, " fl=");
    o = append_long(out, o, sizeof out, (long)fl);
    o = append(out, o, sizeof out, (r == p) ? " fgets=buf" : (r == (char *)0) ? " fgets=NULL" : " fgets=OTHER");
    o = append(out, o, sizeof out, " errno=");
    o = append_long(out, o, sizeof out, (long)err);
    o = append(out, o, sizeof out, " feof=");
    o = append_long(out, o, sizeof out, (long)feof(stdin));
    o = append(out, o, sizeof out, " ferror=");
    o = append_long(out, o, sizeof out, (long)ferror(stdin));
    o = append(out, o, sizeof out, " strlen=");
    o = append_long(out, o, sizeof out, (long)strlen(p));
    o = append(out, o, sizeof out, " bytes=");
    o = append_hex(out, o, sizeof out, (const unsigned char *)p, 16);
    o = append(out, o, sizeof out, "}\n");
    ssize_t w = write(1, out, o);
    (void)w;
    _exit(0);
}

static void stdio_probe(int unbuffered)
{
    int pm = -1;
    char pbuf[CAP];
    long ptotal = 0;
    pbuf[0] = '\0';
    pid_t pc = forkpty(&pm, (char *)0, (const void *)0, (const void *)0);
    if (pc < 0) {
        emit("[py] probe: forkpty failed\n");
        return;
    }
    if (pc == 0) {
        stdio_probe_child(unbuffered);
    }
    if (write_all(pm, "print(6*7)\n") != 0) {
        emit("[py] probe: writing the line failed\n");
    }
    int found = scan_for(pm, "}", pbuf, SPIN * 4L, &ptotal);
    emit(unbuffered ? "[py] probe, streams _IONBF (as -u)" : "[py] probe, streams as they start");
    emit(found ? ": " : " (no report): ");
    emit_received(pbuf, ptotal);
    emit("\n");
    for (long i = 0; i < SPIN; i++) {
        int status = 0;
        pid_t got = waitpid(pc, &status, WNOHANG);
        if (got == pc || got < 0) {
            break;
        }
        sched_yield();
    }
    close(pm);
}

int main(void)
{
    int master = -1;
    char buf[CAP];
    long total = 0;

    buf[0] = '\0';

    stdio_probe(1);
    stdio_probe(0);

    emit("[py] fork (interpreter on the slave end of a pty)\n");
    pid_t child = forkpty(&master, (char *)0, (const void *)0, (const void *)0);
    if (child < 0) {
        return 1;
    }
    if (child == 0) {
        /* forkpty ran login_tty, so the slave is our controlling terminal and
         * is already fds 0/1/2. Nothing to wire up. */
        execl(BIN "python3", "python3", "-q", "-i", "-u", (char *)0);
        /* Only reached if exec failed. 127 is the shell's convention for it,
         * and is distinct from every code this fixture returns, so it cannot
         * be misread as one of our checks failing. */
        _exit(127);
    }

    /* Typed before waiting for a prompt, deliberately. The line discipline
     * buffers it whether or not the interpreter has finished starting, so this
     * removes a round trip and, with it, a reason to guess at how long a
     * prompt takes to appear. */
    emit("[py] type (6*7, whose answer is not in the echo)\n");
    /* DO NOT "simplify" this to print(1+1) or print(2+2).
     *
     * A pty echoes what is typed, so the master sees the expression before
     * it sees any answer.  With print(1+1) the answer 2 is already present
     * in the echoed text, so the scan below matches the ECHO and this
     * fixture passes without the interpreter having evaluated anything --
     * it would then pass equally against an interpreter that never started.
     *
     * 6*7 is chosen because "42" does not occur in "print(6*7)".  Any
     * replacement must keep that property: the answer must not be a
     * substring of the expression that produces it.
     *
     * The header says this too.  It is repeated here because this line is
     * where someone tidying the file will be looking, and the header is
     * not.
     */
    if (write_all(master, "print(6*7)\n") != 0) {
        /* Ask the child what happened before blaming the pty. A write to a
         * master whose slave has been closed gets EIO, and the commonest way
         * for the slave to be closed this early is that the child never
         * became an interpreter at all -- `execl` returned and it took the
         * `_exit(127)` above. Reaping first is what separates "the pty cannot
         * carry a write" from "there was nobody on the other end". */
        return exec_failed(child) ? 8 : 2;
    }

    emit("[py] read (the answer must come from an evaluation)\n");
    if (!scan_for(master, "42", buf, STARTUP_SPIN, &total)) {
        /* Nothing at all means the interpreter never started -- a loader or
         * staging fault. Output without the answer means it started and the
         * REPL did not evaluate, which is this fixture's subject. Either way,
         * say what came back and how the interpreter ended: that is the
         * finding, and the exit code alone is not. */
        emit("[py] received ");
        emit_long(total);
        emit(" byte(s): ");
        emit_received(buf, total);
        emit("\n");
        report_child(child);
        return (total == 0) ? 3 : 4;
    }

    emit("[py] quit\n");
    if (write_all(master, "exit()\n") != 0) {
        return 7;
    }

    int status = 0;
    pid_t reaped = waitpid(child, &status, 0);
    if (reaped != child) {
        return 5;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        return 6;
    }

    emit("[py] ok (the REPL evaluated and answered)\n");
    return 42;
}
