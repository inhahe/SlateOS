/*
 * ctest-rusage -- ring-3 test that getrusage reports the caller's own
 * accounting, through the kernel's SYS_PROCESS_GET_RUSAGE.
 *
 * Until 2026-10-06 the C library read SYS_CPU_TIMES -- the machine's system
 * and interrupt time since boot -- and reported it as the caller's: every
 * process got the same growing number, and RUSAGE_CHILDREN was all zero
 * (requests/b-a-native-getrusage-reports-system-wide-cpu-as-per-process.md).
 * A plausible wrong number is the kind nothing notices, so the decisive
 * check here is the one that failure fails: two processes reading at the
 * same moment must get different answers.
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check and step: the tens digit names the check, the units the step.  No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  getrusage(RUSAGE_SELF) answers (10), and its user time grows while
 *       this process burns CPU (11: it never did, within 10 s)
 *   2x  a child forked now reads its own, far smaller, user time (20: fork
 *       or pipe failed; 21: it read what its parent reads -- the
 *       machine's figure, not its own)
 *   3x  that child burns CPU and exits; reaped, it shows in
 *       RUSAGE_CHILDREN (31: it did not); while it ran, the parent's own
 *       user time, asleep in waitpid, hardly moved (32)
 *   5x  RUSAGE_THREAD answers, and is no more than RUSAGE_SELF (51); the
 *       peak resident set is real for SELF (52) and zero for CHILDREN (53)
 *   6x  a bad who is EINVAL (61), a NULL buffer EFAULT (62)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <stdint.h>
#include <sys/resource.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* A timeval in microseconds. */
static int64_t us(struct timeval tv)
{
    return (int64_t)tv.tv_sec * 1000000 + tv.tv_usec;
}

/* This process's user time, or -1. */
static int64_t self_utime(void)
{
    struct rusage ru;
    if (getrusage(RUSAGE_SELF, &ru) != 0)
        return -1;
    return us(ru.ru_utime);
}

static int64_t now_us(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000 + ts.tv_nsec / 1000;
}

/* Burn CPU until this process's own user time has grown by `want`
 * microseconds, or 10 s have passed. Whether it grew enough. */
static int burn(int64_t want)
{
    int64_t start = self_utime();
    int64_t deadline = now_us() + 10 * 1000000;
    volatile uint64_t x = 1;
    if (start < 0)
        return 0;
    while (now_us() < deadline) {
        for (int i = 0; i < 200000; i++)
            x = x * 6364136223846793005ULL + 1442695040888963407ULL;
        int64_t t = self_utime();
        if (t >= 0 && t - start >= want)
            return 1;
    }
    return 0;
}

int main(void)
{
    struct rusage ru;

    /* 1x */
    if (getrusage(RUSAGE_SELF, &ru) != 0)
        return 10;
    if (!burn(150000))
        return 11;
    int64_t parent = self_utime();

    /* 2x: a fresh child reads its own. */
    int p[2];
    if (pipe(p) != 0)
        return 20;
    pid_t child = fork();
    if (child < 0)
        return 20;
    if (child == 0) {
        close(p[0]);
        int64_t mine = self_utime();
        (void)write(p[1], &mine, sizeof mine);
        close(p[1]);
        /* 3x, the child's half: burn 200 ms of its own, then go. */
        _exit(burn(200000) ? 0 : 1);
    }
    close(p[1]);
    int64_t childs = -1;
    if (read(p[0], &childs, sizeof childs) != (ssize_t)sizeof childs)
        return 20;
    close(p[0]);
    /* The parent has at least 150 ms of its own; a child just forked has
     * almost none. One machine-wide figure would give both the same. */
    if (childs < 0 || childs > parent - 100000)
        return 21;

    /* 3x: reap it; its time is the children's now. */
    int64_t before = self_utime();
    int status = 0;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status) != 0)
        return 31;
    int64_t after = self_utime();
    struct rusage kids;
    if (getrusage(RUSAGE_CHILDREN, &kids) != 0 || us(kids.ru_utime) < 150000)
        return 31;
    if (after - before > 100000)
        return 32;

    /* 5x */
    struct rusage self, thread;
    if (getrusage(RUSAGE_THREAD, &thread) != 0 || getrusage(RUSAGE_SELF, &self) != 0)
        return 51;
    if (us(thread.ru_utime) > us(self.ru_utime))
        return 51;
    if (self.ru_maxrss <= 0)
        return 52;
    if (kids.ru_maxrss != 0)
        return 53;

    /* 6x */
    errno = 0;
    if (getrusage(99, &ru) != -1 || errno != EINVAL)
        return 61;
    errno = 0;
    if (getrusage(RUSAGE_SELF, NULL) != -1 || errno != EFAULT)
        return 62;
    return 42;
}
