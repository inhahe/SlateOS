/*
 * ctest-itimers -- ring-3 test of the interval timers that count processor
 * time, ITIMER_VIRTUAL and ITIMER_PROF, and of RLIMIT_CPU.
 *
 * Until 2026-10-16 setitimer and getitimer refused both CPU-time timers with
 * ENOSYS, the kernel keeping no processor time to count them on. Lane A's
 * kernel counts each thread's run time now, checks the two timers and the
 * CPU limit at its 10 ms tick, and takes `which` 1 and 2 in SYS_ITIMER_SET
 * and SYS_ITIMER_GET (requests/a-d-cpu-time-clocks-have-a-native-call-now.md).
 * The host tests check that the library passes `which` through; only here is
 * it seen that the signals come while the process computes and not while it
 * sleeps, and that the CPU limit is enforced.
 *
 * Every timing is a wide margin: timers of 50 ms are asked for at least three
 * signals in 400 ms of computing, and at most one in 300 ms of sleep. Every
 * busy loop stops after five seconds of wall time whether or not it was told
 * to, and an alarm ends the whole test after thirty, so it cannot hang. It
 * takes about five seconds, three of them a child's processor time.
 *
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  ITIMER_VIRTUAL: it can be armed (10); SIGVTALRM comes while the
 *       process computes (11) and not while it sleeps (12); disarmed, it
 *       reports no time left and keeps its period (13)
 *   2x  ITIMER_PROF: it can be armed (20); SIGPROF comes while the process
 *       computes (21); what getitimer says is left falls while it computes
 *       (22) and not while it sleeps (23)
 *   3x  RLIMIT_CPU: a child past its soft limit gets SIGXCPU (31), and one
 *       that ignores it is killed at the hard limit (32); 30: fork, wait or
 *       setrlimit failed
 */

#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define MS 1000000LL

static int64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

/* Compute for `ns` of wall time, five seconds at the most. */
static void busy(int64_t ns)
{
    int64_t start = now_ns();
    volatile uint64_t x = 1;
    if (ns > 5000 * MS)
        ns = 5000 * MS;
    while (now_ns() - start < ns) {
        for (int i = 0; i < 10000; i++)
            x = x * 6364136223846793005ULL + 1;
    }
}

static void pause_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
}

static atomic_int vtalrms, profs;

static void on_vtalrm(int sig)
{
    (void)sig;
    atomic_fetch_add(&vtalrms, 1);
}

static void on_prof(int sig)
{
    (void)sig;
    atomic_fetch_add(&profs, 1);
}

static void on_xcpu(int sig)
{
    (void)sig;
    _exit(7);
}

static struct itimerval every(long usec, long first_usec)
{
    struct itimerval v;
    v.it_interval.tv_sec = usec / 1000000;
    v.it_interval.tv_usec = usec % 1000000;
    v.it_value.tv_sec = first_usec / 1000000;
    v.it_value.tv_usec = first_usec % 1000000;
    return v;
}

static int64_t left_ns(int which)
{
    struct itimerval v;
    if (getitimer(which, &v) != 0)
        return -1;
    return (int64_t)v.it_value.tv_sec * 1000000000LL + (int64_t)v.it_value.tv_usec * 1000;
}

static int check_virtual(void)
{
    signal(SIGVTALRM, on_vtalrm);
    struct itimerval v = every(50000, 50000);
    if (setitimer(ITIMER_VIRTUAL, &v, 0) != 0)
        return 10;
    busy(400 * MS);
    int computed = atomic_load(&vtalrms);
    if (computed < 3)
        return 11;
    pause_ms(300);
    if (atomic_load(&vtalrms) - computed > 1)
        return 12;
    struct itimerval off = every(50000, 0), got;
    if (setitimer(ITIMER_VIRTUAL, &off, 0) != 0 || getitimer(ITIMER_VIRTUAL, &got) != 0)
        return 13;
    if (got.it_value.tv_sec != 0 || got.it_value.tv_usec != 0 || got.it_interval.tv_sec != 0 || got.it_interval.tv_usec != 50000)
        return 13;
    signal(SIGVTALRM, SIG_DFL);
    return 0;
}

static int check_prof(void)
{
    signal(SIGPROF, on_prof);
    struct itimerval v = every(50000, 50000);
    if (setitimer(ITIMER_PROF, &v, 0) != 0)
        return 20;
    busy(400 * MS);
    if (atomic_load(&profs) < 3)
        return 21;
    struct itimerval off = every(0, 0);
    setitimer(ITIMER_PROF, &off, 0);
    signal(SIGPROF, SIG_DFL);

    /* Ten seconds of processor time: it never fires here. */
    struct itimerval far = every(0, 10000000);
    if (setitimer(ITIMER_PROF, &far, 0) != 0)
        return 20;
    int64_t a = left_ns(ITIMER_PROF);
    busy(200 * MS);
    int64_t b = left_ns(ITIMER_PROF);
    pause_ms(300);
    int64_t c = left_ns(ITIMER_PROF);
    setitimer(ITIMER_PROF, &off, 0);
    if (a < 0 || b < 0 || a - b < 50 * MS)
        return 22;
    if (c < 0 || b - c >= 100 * MS)
        return 23;
    return 0;
}

/* A child with RLIMIT_CPU soft and hard seconds that computes until it is
 * stopped or four seconds of wall time have gone: its wait status. */
static int child_with_cpu_limit(rlim_t soft, rlim_t hard, int ignore_xcpu, int *status)
{
    pid_t child = fork();
    if (child < 0)
        return -1;
    if (child == 0) {
        struct rlimit lim = {soft, hard};
        if (setrlimit(RLIMIT_CPU, &lim) != 0)
            _exit(9);
        signal(SIGXCPU, ignore_xcpu ? SIG_IGN : on_xcpu);
        busy(4000 * MS);
        _exit(0);
    }
    if (waitpid(child, status, 0) != child)
        return -1;
    return 0;
}

static int check_cpu_limit(void)
{
    int status;
    if (child_with_cpu_limit(1, 3, 0, &status) != 0)
        return 30;
    if (WIFEXITED(status) && WEXITSTATUS(status) == 9)
        return 30;
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 7)
        return 31;
    if (child_with_cpu_limit(1, 2, 1, &status) != 0)
        return 30;
    if (WIFEXITED(status) && WEXITSTATUS(status) == 9)
        return 30;
    if (!WIFSIGNALED(status) || WTERMSIG(status) != SIGKILL)
        return 32;
    return 0;
}

int main(void)
{
    alarm(30);
    int rc;
    if ((rc = check_virtual()) != 0)
        return rc;
    if ((rc = check_prof()) != 0)
        return rc;
    if ((rc = check_cpu_limit()) != 0)
        return rc;
    return 42;
}
