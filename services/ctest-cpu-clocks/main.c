/*
 * ctest-cpu-clocks -- ring-3 test of the CPU-time clocks: how much processor
 * time a process, or one of its threads, has used.
 *
 * Until 2026-10-16 the C library answered every CPU-time clock with the time
 * since boot, so a program that compared the processor time it had used with
 * the time that had passed saw the two equal, `clock()` counted the time it
 * slept, and no other process's or thread's clock could be named. The kernel
 * now measures each thread's run time and answers through SYS_CPU_CLOCK
 * (lane A's requests/a-d-cpu-time-clocks-have-a-native-call-now.md); the host
 * tests check the library's half against a stand-in that reads the
 * monotonic clock, so only here is it seen that sleeping does not count.
 *
 * Every timing is a wide margin: the busy threads run for 200 ms of wall
 * time and are asked for 50 ms of processor time, the sleeps last 300 ms and
 * may use up to 100 ms. Every busy loop stops after five seconds of wall time
 * whether or not it was told to, and an alarm ends the whole test after
 * twenty, so it cannot hang.
 *
 * It passes on Linux 6.6 with glibc, which is where the clock ids and the
 * error numbers below come from.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  the process's clock: it can be read (10), its resolution is a
 *       nanosecond (11), it advances while the process computes (12) and
 *       not while it sleeps (13); clock() is it in microseconds (15, 16)
 *   2x  clock_getcpuclockid: the caller's id is pid 0's, (~0 << 3) | 2
 *       (20, 21), and reads (22); a child's can be read while it computes
 *       and advances (23, 25), and once the child is reaped its pid is
 *       ESRCH (26); 27: fork or wait failed
 *   3x  pthread_getcpuclockid: a busy thread's clock advances (30, 31)
 *       while the sleeping main thread's own barely does (32); the main
 *       thread's id is its tid's (33); 39: a thread could not be made
 *   5x  setting: the process's clock is read-only, EINVAL (50); another
 *       process's clock is EPERM (51)
 *   6x  sleeping on a CPU-time clock: the thread's own is EINVAL (60); the
 *       process's, while a thread computes, returns once that much processor
 *       time has been used (61, 62), and so does the TIMER_ABSTIME form (63,
 *       64); 69: a thread could not be made
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* include/linux/posix-timers_types.h */
#define CPUCLOCK_SCHED 2
#define MAKE_PROCESS_CPUCLOCK(pid, clock) ((int)(~(unsigned)(pid) << 3) | (clock))
#define MAKE_THREAD_CPUCLOCK(tid, clock) MAKE_PROCESS_CPUCLOCK((tid), (clock) | 4)

#define MS 1000000LL

static int64_t ns_of(clockid_t clock)
{
    struct timespec ts;
    if (clock_gettime(clock, &ts) != 0)
        return -1;
    return (int64_t)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

/* Compute until `ns` of wall time have passed, or `*stop` is set, or five
 * seconds have: whichever comes first. */
static void busy(int64_t ns, atomic_int *stop)
{
    int64_t start = ns_of(CLOCK_MONOTONIC);
    volatile uint64_t x = 1;
    for (;;) {
        for (int i = 0; i < 10000; i++)
            x = x * 6364136223846793005ULL + 1;
        int64_t now = ns_of(CLOCK_MONOTONIC);
        if (now - start >= ns || now - start >= 5000 * MS)
            return;
        if (stop && atomic_load(stop))
            return;
    }
}

static void pause_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
}

static atomic_int stop_spinner;

static void *spinner(void *arg)
{
    (void)arg;
    busy(5000 * MS, &stop_spinner);
    return 0;
}

static int check_process_clock(void)
{
    struct timespec res;
    int64_t t0 = ns_of(CLOCK_PROCESS_CPUTIME_ID);
    if (t0 < 0)
        return 10;
    if (clock_getres(CLOCK_PROCESS_CPUTIME_ID, &res) != 0 || res.tv_sec != 0 || res.tv_nsec != 1)
        return 11;
    busy(200 * MS, 0);
    int64_t t1 = ns_of(CLOCK_PROCESS_CPUTIME_ID);
    if (t1 - t0 < 50 * MS)
        return 12;
    pause_ms(300);
    int64_t t2 = ns_of(CLOCK_PROCESS_CPUTIME_ID);
    if (t2 - t1 >= 100 * MS)
        return 13;

    clock_t c0 = clock();
    if (c0 == (clock_t)-1)
        return 15;
    busy(200 * MS, 0);
    clock_t c1 = clock();
    pause_ms(300);
    clock_t c2 = clock();
    if (c1 - c0 < 50000 || c2 - c1 >= 100000)
        return 16;
    return 0;
}

static int check_getcpuclockid(void)
{
    clockid_t self;
    if (clock_getcpuclockid(0, &self) != 0)
        return 20;
    if (self != MAKE_PROCESS_CPUCLOCK(0, CPUCLOCK_SCHED))
        return 21;
    if (ns_of(self) < 0)
        return 22;

    pid_t child = fork();
    if (child < 0)
        return 27;
    if (child == 0) {
        busy(600 * MS, 0);
        _exit(0);
    }
    clockid_t theirs;
    int rc = 0;
    if (clock_getcpuclockid(child, &theirs) != 0 || theirs != MAKE_PROCESS_CPUCLOCK(child, CPUCLOCK_SCHED)) {
        rc = 23;
    } else {
        int64_t a = ns_of(theirs);
        pause_ms(300);
        int64_t b = ns_of(theirs);
        if (a < 0 || b - a < 50 * MS)
            rc = 25;
    }
    int status;
    if (waitpid(child, &status, 0) != child)
        return rc ? rc : 27;
    if (rc)
        return rc;
    if (clock_getcpuclockid(child, &theirs) != ESRCH)
        return 26;
    return 0;
}

static int check_thread_clocks(void)
{
    pthread_t t;
    atomic_store(&stop_spinner, 0);
    if (pthread_create(&t, 0, spinner, 0) != 0)
        return 39;
    clockid_t theirs;
    int rc = 0;
    int64_t mine0 = ns_of(CLOCK_THREAD_CPUTIME_ID);
    if (pthread_getcpuclockid(t, &theirs) != 0) {
        rc = 30;
    } else {
        int64_t a = ns_of(theirs);
        pause_ms(300);
        int64_t b = ns_of(theirs);
        int64_t mine1 = ns_of(CLOCK_THREAD_CPUTIME_ID);
        if (a < 0 || b - a < 50 * MS)
            rc = 31;
        else if (mine0 < 0 || mine1 - mine0 >= 100 * MS)
            rc = 32;
    }
    atomic_store(&stop_spinner, 1);
    pthread_join(t, 0);
    if (rc)
        return rc;
    clockid_t mine;
    if (pthread_getcpuclockid(pthread_self(), &mine) != 0 || mine != MAKE_THREAD_CPUCLOCK(gettid(), CPUCLOCK_SCHED))
        return 33;
    return 0;
}

static int check_setting(void)
{
    struct timespec ts = {1, 0};
    errno = 0;
    if (clock_settime(CLOCK_PROCESS_CPUTIME_ID, &ts) != -1 || errno != EINVAL)
        return 50;
    errno = 0;
    if (clock_settime(MAKE_PROCESS_CPUCLOCK(0, CPUCLOCK_SCHED), &ts) != -1 || errno != EPERM)
        return 51;
    return 0;
}

static int check_sleeping(void)
{
    struct timespec req = {0, 50 * MS};
    if (clock_nanosleep(CLOCK_THREAD_CPUTIME_ID, 0, &req, 0) != EINVAL)
        return 60;

    pthread_t t;
    atomic_store(&stop_spinner, 0);
    if (pthread_create(&t, 0, spinner, 0) != 0)
        return 69;
    int rc = 0;
    int64_t before = ns_of(CLOCK_PROCESS_CPUTIME_ID);
    if (clock_nanosleep(CLOCK_PROCESS_CPUTIME_ID, 0, &req, 0) != 0) {
        rc = 61;
    } else if (ns_of(CLOCK_PROCESS_CPUTIME_ID) - before < 50 * MS) {
        rc = 62;
    } else {
        int64_t target = ns_of(CLOCK_PROCESS_CPUTIME_ID) + 50 * MS;
        struct timespec abs = {target / 1000000000LL, target % 1000000000LL};
        if (clock_nanosleep(CLOCK_PROCESS_CPUTIME_ID, TIMER_ABSTIME, &abs, 0) != 0)
            rc = 63;
        else if (ns_of(CLOCK_PROCESS_CPUTIME_ID) < target)
            rc = 64;
    }
    atomic_store(&stop_spinner, 1);
    pthread_join(t, 0);
    return rc;
}

int main(void)
{
    alarm(20);
    int rc;
    if ((rc = check_process_clock()) != 0)
        return rc;
    if ((rc = check_getcpuclockid()) != 0)
        return rc;
    if ((rc = check_thread_clocks()) != 0)
        return rc;
    if ((rc = check_setting()) != 0)
        return rc;
    if ((rc = check_sleeping()) != 0)
        return rc;
    return 42;
}
