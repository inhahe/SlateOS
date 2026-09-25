/*
 * ctest-coreutils-runs — does ANY of our Rust userland execute on SlateOS?
 *
 * WHY THIS EXISTS.  `requests/b-a-our-own-utilities-are-on-the-image-now-can-a-
 * boot-test-run-one.md` has been open since 2026-09-13, and the roadmap states
 * the gap in four words: **staged is not run**.  71 Rust binaries from
 * `userspace/coreutils` are placed in `/bin` by `create-ext4-rootfs.sh`.  Not
 * one of them has ever been executed under SlateOS by anything.  Compiling,
 * linking, being staged and *running* are four different claims, and only the
 * first three have evidence.
 *
 * Until this passes, "SlateOS has 89 commands" means "89 files are present".
 *
 * WHY THESE FOUR, IN THIS ORDER.  Each one adds exactly one capability to the
 * one before it, so a failure names the layer that broke rather than "the
 * userland does not work":
 *
 *   1. `/bin/true`      — exec, run, exit 0.  No argv use, no output, no libc
 *                         beyond start-up and `exit`.  If this fails, nothing
 *                         below it can be interpreted: the loader, the entry
 *                         stub or the exit path is broken, and the rest of the
 *                         fixture would only repeat that.
 *   2. `/bin/false`     — the same, exiting 1.  Together with (1) this proves
 *                         the exit STATUS is carried rather than merely that a
 *                         process ended: a `wait` that always reported 0 would
 *                         pass (1) alone and is a real failure mode, because
 *                         every shell script on the system reads that value.
 *   3. `/bin/echo`      — argv reaches the program, and its stdout reaches a
 *                         pipe.  The first test of the two paths that every
 *                         other utility needs.
 *   4. `/bin/basename`  — the first one that COMPUTES.  `/usr/lib/x.so` in,
 *                         `x.so` out: argument parsing, string work, and a
 *                         result that is wrong if any of it is wrong.
 *
 * WHY NOT ONE BINARY.  A single probe answers "did something run", which is
 * the question everyone asks first and the least useful one.  Four ordered
 * probes answer "how far does it get", and the boundary between the last pass
 * and the first failure is the finding.
 *
 * THE OUTPUT COMPARISON IS EXACT, NOT A SUBSTRING.  `echo hi` must produce
 * exactly `hi\n`.  A substring test would pass on a program that printed a
 * usage message containing the word, and the usage message is precisely what a
 * broken argv handler prints.
 *
 * BOUNDS.  Structural, as in `ctest-pty` and `ctest-python-repl`: every read is
 * gated on `poll(POLLIN|POLLHUP, 0)` and the wait is a counted spin with
 * `sched_yield()`.  Deliberately no `alarm` — a fixture's bounds should not
 * depend on a subsystem other than the one under test.
 *
 * Exit codes — one per check, because "the userland is broken" is not a
 * finding and "basename computed the wrong answer while echo was fine" is:
 *    42  all four ran and answered correctly
 *     1  pipe() failed (this fixture's own plumbing, not a finding about them)
 *     2  fork() failed
 *     3  /bin/true did not exit 0 — nothing below this is interpretable
 *     4  /bin/false did not exit 1 — the exit status is not being carried
 *     5  /bin/echo produced something other than "hi\n"
 *     6  /bin/echo did not exit 0
 *     7  /bin/basename produced something other than "x.so\n"
 *     8  /bin/basename did not exit 0
 *     9  a wait() failed or returned the wrong pid
 *    10  a read of a child's output failed or never finished
 *    11  a program could not be EXEC'd at all. Plumbing, not a verdict about
 *        the program: the child prints the path and execl's errno to the
 *        console before exiting, so the serial says which of "not on the
 *        image" (ENOENT), "this caller may not open it" (EACCES/EPERM) or
 *        "not a program" (ENOEXEC) it was.
 *
 * 11 exists because it did not, and that cost a boot. `execl` failing takes
 * the child to `_exit(127)`, which arrived here as an ordinary exit status and
 * fell through step 1's `if (rc != 0) return 3;`. Exit 3 reads as "the Rust
 * userland does not run", so a rootfs that had staged NONE of the 72 manifest
 * binaries was reported as a broken loader. A missing `/bin/true` looks
 * exactly like a broken one unless the fixture is made to say which -- the
 * same collapse `ctest-pty` carries a note about, in the same words:
 * ONE CODE PER CHILD STATUS.
 */

#include <errno.h>
#include <poll.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

/* `sched_yield()` with no <sched.h> in the sysroot; see `ctest-pty` for why a
 * spin built only on `poll(..., 0)` can starve the writer it is waiting for. */
extern int sched_yield(void);

/* Where the staged binaries actually are AT RUNTIME.
 *
 * `create-ext4-rootfs.sh` stages into `$STAGE/bin`, and its log says "into
 * /bin" -- both true OF THE IMAGE. The kernel mounts that image at `/mnt`
 * (`[vfs] Mounted ext4 filesystem at '/mnt'`), so the path a running process
 * must exec is `/mnt/bin/true`. `load_test_elf` reads these fixtures from
 * `/mnt/tests/<name>.elf` for the same reason.
 *
 * This fixture execed `/bin/true` and got ENOENT. `/bin` MEANS TWO DIFFERENT
 * THINGS depending on which side of the mount you are on, and every layer --
 * the manifest called `rootfs-bin-manifest`, the staging log, this file --
 * was honest about the one it was given.
 *
 * It cost a boot, and exit 11 is what ended it in four minutes: the code says
 * "not on the image, or not executable", which is a CHECKABLE claim.
 * `debugfs -R "stat /bin/true" rootfs.ext4` showed the file present, 0755 and
 * the right size, so the claim was false -- and its being false is what
 * pointed at the path. A wrong-but-checkable diagnosis beat the vague one it
 * replaced.
 */
#define BIN "/mnt/bin/"

#define SPIN 4000000L
#define CAP 512

static void emit_fd(int fd, const char *s)
{
    size_t n = 0;
    while (s[n] != '\0') {
        n++;
    }
    if (n != 0) {
        ssize_t written = write(fd, s, n);
        (void)written;
    }
}

static void emit(const char *s)
{
    emit_fd(1, s);
}

/* Name the reason an exec failed, from inside the child that saw it.
 *
 * WHY THE CHILD SAYS IT.  The parent learns only an exit status, and 127 is
 * one number for every reason `execl` can return. For a week that number was
 * explained by a guess -- "not on the image, or not executable" -- which was
 * false both times it was checked: the file was present at mode 0755, and the
 * real errno was EACCES from a `stat` the caller had no right to make
 * (`requests/a-b-libc-execl-passes-a-null-path-to-execve.md`). The errno had
 * existed in this process the whole time and nothing printed it.
 *
 * fd 2, not fd 1: fd 1 is now the pipe the parent is reading and comparing
 * byte for byte, while fd 2 is still the console the kernel rung captures. */
static void report_exec_failure(const char *path, int err)
{
    char digits[12];
    int n = 0;
    unsigned int v = err < 0 ? 0u : (unsigned int)err;
    do {
        digits[n++] = (char)('0' + (int)(v % 10u));
        v /= 10u;
    } while (v != 0u && n < (int)sizeof digits - 1);
    char num[12];
    int k = 0;
    while (n > 0) {
        num[k++] = digits[--n];
    }
    num[k] = '\0';

    emit_fd(2, "[cu] execl(");
    emit_fd(2, path);
    emit_fd(2, ") failed in the child: errno ");
    emit_fd(2, num);
    emit_fd(2, " (");
    emit_fd(2, strerror(err));
    emit_fd(2, ")\n");
}

static int readable(int fd)
{
    struct pollfd pfd;
    pfd.fd = fd;
    pfd.events = POLLIN;
    pfd.revents = 0;
    if (poll(&pfd, 1, 0) <= 0) {
        return 0;
    }
    /* POLLHUP counts: a read at hangup returns 0 at once rather than blocking,
     * and treating it as "not ready" would spin to the budget and report a
     * timeout for what is really the child having finished. */
    return (pfd.revents & (POLLIN | POLLHUP)) != 0;
}

/* Run `/bin/<name>` with one optional argument, capture stdout, reap it.
 *
 * Returns the child's exit status, or -1 if this fixture's own plumbing failed
 * -- which is reported separately from any verdict about the child, because a
 * broken pipe here says nothing about the program under test. */
static int run_one(const char *path, const char *arg, char *out, int cap, int *plumbing)
{
    int fds[2];
    *plumbing = 0;
    if (pipe(fds) != 0) {
        *plumbing = 1;
        return -1;
    }

    pid_t child = fork();
    if (child < 0) {
        (void)close(fds[0]);
        (void)close(fds[1]);
        *plumbing = 2;
        return -1;
    }
    if (child == 0) {
        (void)close(fds[0]);
        if (fds[1] != 1) {
            (void)dup2(fds[1], 1);
            (void)close(fds[1]);
        }
        if (arg == (const char *)0) {
            execl(path, path, (char *)0);
        } else {
            execl(path, path, arg, (char *)0);
        }
        /* Read errno before anything else can touch it. */
        report_exec_failure(path, errno);
        /* 127 is the shell's convention for "could not exec", and is distinct
         * from every code this fixture returns, so it cannot be mistaken for
         * one of our checks failing. */
        _exit(127);
    }

    (void)close(fds[1]);
    int got = 0;
    for (long i = 0; i < SPIN; i++) {
        if (got >= cap - 1) {
            break;
        }
        if (!readable(fds[0])) {
            sched_yield();
            continue;
        }
        ssize_t n = read(fds[0], out + got, (size_t)(cap - 1 - got));
        if (n > 0) {
            got += (int)n;
            i = 0; /* progress: give the next byte a full budget */
        } else if (n == 0) {
            break; /* EOF: the child closed its end */
        } else if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) {
            break;
        }
    }
    out[got] = '\0';
    (void)close(fds[0]);

    int status = 0;
    pid_t reaped = waitpid(child, &status, 0);
    if (reaped != child) {
        *plumbing = 9;
        return -1;
    }
    if (!WIFEXITED(status)) {
        /* A child that died on a signal is a finding about the child, not
         * plumbing -- reported as an impossible exit code so the caller's
         * status comparison fails rather than accidentally matching. */
        return 128;
    }
    int code = WEXITSTATUS(status);
    if (code == 127) {
        /* The child never ran: `execl` returned and it took the `_exit(127)`
         * below the exec. Reported as PLUMBING, not as a verdict about the
         * program, because it is not one -- it says the file is missing or is
         * not executable, which is a fact about the IMAGE.
         *
         * This is the collapse `ctest-pty` already carries a note about --
         * ONE CODE PER CHILD STATUS. Without it, 127 fell through to the
         * caller's `if (rc != 0) return 3;` and *could not exec* became
         * indistinguishable from *ran and exited non-zero*. It cost a whole
         * boot: `create-ext4-rootfs.sh` had staged none of the 72 manifest
         * binaries, and the fixture reported 3, which reads as "the Rust
         * userland is broken" and sent the reader to the ELF loader. A
         * missing `/bin/true` looks exactly like a broken one unless the
         * fixture says which.
         *
         * The `_exit(127)` comment below is true of the CHILD's code and was
         * never true of what this function returns; that is the whole of the
         * mistake. */
        /* The child has already printed WHY, with its errno, on the line
         * above this one. The cases it distinguishes, so a reader does not
         * have to guess the way this message used to:
         *   ENOENT         not on the image at this path
         *   EACCES, EPERM  present, but this caller could not open it --
         *                  look at what its spawner granted, not the image
         *   ENOEXEC        present and readable, and not a program */
        emit("[cu] COULD NOT EXEC ");
        emit(path);
        emit(" -- the child's errno, printed just above, says why.\n");
        emit("[cu] This is NOT a finding about the Rust userland.\n");
        *plumbing = 11;
        return -1;
    }
    return code;
}

int main(void)
{
    char out[CAP];
    int plumbing = 0;
    int rc;

    /* 1. Does anything run at all? */
    emit("[cu] true (exec, run, exit 0 -- nothing below this is readable if it fails)\n");
    rc = run_one(BIN "true", (const char *)0, out, CAP, &plumbing);
    if (plumbing != 0) {
        return plumbing;
    }
    if (rc != 0) {
        return 3;
    }

    /* 2. Is the exit status carried, or is every child reported as 0? */
    emit("[cu] false (a wait that always says 0 would have passed step 1)\n");
    rc = run_one(BIN "false", (const char *)0, out, CAP, &plumbing);
    if (plumbing != 0) {
        return plumbing;
    }
    if (rc != 1) {
        return 4;
    }

    /* 3. argv in, stdout out. */
    emit("[cu] echo (argv reaches it, and its output reaches a pipe)\n");
    rc = run_one(BIN "echo", "hi", out, CAP, &plumbing);
    if (plumbing != 0) {
        return plumbing;
    }
    if (strcmp(out, "hi\n") != 0) {
        return 5;
    }
    if (rc != 0) {
        return 6;
    }

    /* 4. The first one that computes an answer. */
    emit("[cu] basename (the first that computes rather than echoes)\n");
    rc = run_one(BIN "basename", "/usr/lib/x.so", out, CAP, &plumbing);
    if (plumbing != 0) {
        return plumbing;
    }
    if (strcmp(out, "x.so\n") != 0) {
        return 7;
    }
    if (rc != 0) {
        return 8;
    }

    emit("[cu] ok (the Rust userland runs)\n");
    return 42;
}
