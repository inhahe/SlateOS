/*
 * ctest-zombiewait — the shortest path to a process going zombie with a
 * waiter, for B-FORKEXEC-BOOT-HANG.
 *
 * WHY THIS EXISTS.  The hang is recorded against `forkexec`, and a boot on
 * 2026-09-15 reproduced the documented signature ("Process N has no threads
 * left -- now zombie", then "Task M exiting", then silence: no PF, no PANIC,
 * no FATAL) on `spawn-test-dash-statpath` instead.  statpath is
 * `[ -f /bin/dash ] && echo` — one stat and an exit — so whatever idles is
 * not specific to fork+exec.  But statpath still loads ld.so and dash, so it
 * does not separate the *loader* from the *reap* path either.
 *
 * This fixture removes everything that is not the reap.  It is a static
 * native binary: no execve, no dynamic loader, no shell.  It forks, the child
 * exits, the parent waits.  That is all.
 *
 *   If it hangs, the exec path is exonerated and the search collapses to the
 *   wait/reap wakeup.
 *   If it never hangs, the loader is implicated, which is worth knowing too.
 *
 * Both outcomes are informative, which is what makes it worth building rather
 * than waiting for another occurrence.
 *
 * THE TWO ORDERINGS, AND WHY A PIPE RATHER THAN A SLEEP.  "Zombie with a
 * waiter" is reached two ways, and only one of them can deadlock:
 *
 *   A. The child is ALREADY a zombie when the parent calls `wait`.  The
 *      kernel has a corpse to hand over and never has to block the parent.
 *   B. The parent is ALREADY blocked in `wait` when the child exits.  Now
 *      something has to WAKE the parent.  A lost wakeup here looks exactly
 *      like the recorded signature: the child is reported zombie, its task is
 *      reported exiting, and then nothing, because the only thread left is
 *      blocked on a queue nobody posted to.
 *
 * Case B is the suspect, so it must be reached deliberately rather than by
 * luck.  A `sleep` in the child would be a race in both directions — too
 * short and the child beats the parent to `wait` (which is case A wearing
 * case B's name, and passes while proving nothing), too long and the fixture
 * outlives the kernel's bounded yield budget.  The same objection
 * `ctest-pgroup` records about keeping a child alive.
 *
 * A pipe removes the clock.  The child blocks reading a pipe whose write ends
 * are held by both processes; it cannot exit until every write end is closed.
 * The parent closes its copy and *then* calls `wait`.  So the child is
 * released at a moment the parent chooses, and from there the parent has one
 * syscall to reach `wait` while the child must be rescheduled, return from
 * `read`, and call `_exit`.  That does not make case B certain — nothing
 * short of a kernel hook would — but it makes it the overwhelmingly likely
 * interleaving, without depending on a timer.
 *
 * Case A is fully deterministic, and gets that from the same pipe read from
 * the other end.  The child writes a byte and exits.  The parent reads the
 * byte (so the child has certainly run), then reads AGAIN: the second read
 * returns 0 only when no write end remains open anywhere, and the child's
 * copy is closed by the kernel when the child's files are torn down.  So a
 * second read of 0 IS the proof that the child is gone, with no clock and no
 * guess.  Only then does the parent call `wait`.
 *
 * WHAT A PASS MEANS.  Exit 42: both orderings reached a zombie and both were
 * reaped with the right pid and the right status.  Any other code names the
 * first check that failed, per the list below.  A HANG is the finding — this
 * fixture has no timeout of its own, deliberately, because the kernel's
 * bounded yield budget is what should notice and the fixture inventing its
 * own alarm would hide the thing it was built to show.
 *
 * WHY THE MARKERS, AND WHY BEFORE EACH SEGMENT RATHER THAN BETWEEN THEM.  A
 * hang means the process never exits, so the exit code carries nothing: 42
 * says both orderings passed, and silence says one of them did not, without
 * saying which.  The two halves exercise different kernel paths -- the
 * already-zombie case hits the reap lookup, the already-blocked case hits the
 * wakeup -- so a hang that cannot name its half reports that the bug is real
 * and not WHERE TO LOOK, which is most of the investigation missing.
 *
 * Each marker is emitted BEFORE the work it names, so the LAST line in the
 * serial log is the segment that did not finish.  Marking only BETWEEN the
 * halves would leave a hang before the first marker indistinguishable from a
 * hang in ordering one, silently attributing a failure to start to the first
 * case; `[zw] start` is what separates those.
 *
 * `[zw] B child released` is the one marker a child emits, and it buys a
 * distinction the parent cannot make alone.  If the parent hangs after
 * `[zw] B wait` and that line IS present, the child woke, ran and exited --
 * so a corpse exists and nobody delivered the wakeup.  If it is ABSENT, the
 * child never came back from `read`, and the fault is in the pipe or the
 * scheduler rather than in reaping at all.  Two very different searches, told
 * apart by one line.
 *
 * Unbuffered `write(1, ...)`, as `services/ctest-initfini/main.c` explains at
 * length: a marker held in a stdio buffer is a marker lost in exactly the run
 * that needed it, because a hang never flushes.  Each is a single short write
 * so the parent's and the child's cannot interleave mid-line.
 *
 * Exit codes:
 *    42  every check passed
 *     1  pipe() failed (case A)
 *     2  fork() failed (case A)
 *     3  first read did not return the child's one byte
 *     4  second read did not return EOF, so the child had not gone
 *     5  wait() returned an error or the wrong pid (case A)
 *     6  child did not exit normally, or with the wrong code (case A)
 *     7  pipe() failed (case B)
 *     8  fork() failed (case B)
 *     9  wait() returned an error or the wrong pid (case B)
 *    10  child did not exit normally, or with the wrong code (case B)
 */

#include <errno.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

/* Distinct from each other and from every exit code above, so a status that
 * arrives from the wrong child is visible rather than plausible. */
#define CHILD_A_CODE 21
#define CHILD_B_CODE 23

/* Unbuffered, and a failed diagnostic must never change the verdict: this
 * fixture's answer is its exit code, so a marker that could not be written is
 * a lost line rather than a different result. */
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

/* read() and write() are declared warn_unused_result under -Werror, and every
 * call here is checked; these wrappers keep the checks readable. */
static int read_exactly(int fd, char *buf, int want)
{
    int got = 0;
    while (got < want) {
        ssize_t n = read(fd, buf + got, (size_t)(want - got));
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            return -1;
        }
        if (n == 0) {
            return got; /* EOF before `want` bytes */
        }
        got += (int)n;
    }
    return got;
}

/* Case A: the child is already a zombie when the parent waits. */
static int already_zombie(void)
{
    int fds[2];
    emit("[zw] A fork (already-zombie: the reap lookup)\n");
    if (pipe(fds) != 0) {
        return 1;
    }

    pid_t child = fork();
    if (child < 0) {
        return 2;
    }
    if (child == 0) {
        /* The child needs only the write end. Closing the read end matters:
         * a read end left open in the parent is fine, but one left open here
         * would keep this process able to read its own pipe, which is not
         * what the parent's EOF is being asked about. */
        (void)close(fds[0]);
        char byte = 'x';
        ssize_t n = write(fds[1], &byte, 1);
        if (n != 1) {
            _exit(99);
        }
        _exit(CHILD_A_CODE);
    }

    /* The parent must drop its own write end or the EOF below can never
     * arrive -- it would be waiting on itself. */
    (void)close(fds[1]);

    char got = 0;
    if (read_exactly(fds[0], &got, 1) != 1 || got != 'x') {
        return 3;
    }
    /* The proof, not a guess: zero here means no write end is open anywhere,
     * and the child's copy closes when the child's files are torn down. */
    char scratch = 0;
    if (read_exactly(fds[0], &scratch, 1) != 0) {
        return 4;
    }
    (void)close(fds[0]);

    int status = 0;
    emit("[zw] A wait (a corpse already exists)\n");
    pid_t reaped = wait(&status);
    if (reaped != child) {
        return 5;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != CHILD_A_CODE) {
        return 6;
    }
    return 0;
}

/* Case B: the parent is already blocked in wait when the child exits. This is
 * the one that can lose a wakeup. */
static int waiter_first(void)
{
    int fds[2];
    emit("[zw] B fork (waiter-first: the wakeup)\n");
    if (pipe(fds) != 0) {
        return 7;
    }

    pid_t child = fork();
    if (child < 0) {
        return 8;
    }
    if (child == 0) {
        /* Drop the write end so the parent's close is the LAST one. Holding it
         * here would mean this read never ends and the fixture deadlocks for a
         * reason that has nothing to do with the kernel under test. */
        (void)close(fds[1]);
        char scratch = 0;
        (void)read_exactly(fds[0], &scratch, 1); /* returns 0 at EOF */
        emit("[zw] B child released\n");
        _exit(CHILD_B_CODE);
    }

    (void)close(fds[0]);
    /* Releasing the child and blocking are adjacent on purpose: from here the
     * parent needs one syscall to be inside `wait`, while the child must be
     * rescheduled, return from `read` and call `_exit`. */
    (void)close(fds[1]);

    int status = 0;
    emit("[zw] B wait (nothing to reap yet; a wakeup must arrive)\n");
    pid_t reaped = wait(&status);
    if (reaped != child) {
        return 9;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != CHILD_B_CODE) {
        return 10;
    }
    return 0;
}

int main(void)
{
    /* Before anything else: a hang with no marker at all is a process that
     * never reached main, which is a loader or spawn fault and not this
     * fixture's subject. */
    emit("[zw] start\n");
    int rc = already_zombie();
    if (rc != 0) {
        return rc;
    }
    rc = waiter_first();
    if (rc != 0) {
        return rc;
    }
    emit("[zw] ok (both orderings reaped)\n");
    return 42;
}
