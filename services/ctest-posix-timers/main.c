/*
 * ctest-posix-timers -- ring-3 test of the POSIX per-process timers,
 * timer_create and its family.
 *
 * Until 2026-10-16 timer_create answered ENOSYS: the kernel had no such
 * timer for native programs. Lane A's kernel has them now (SYS_POSIX_TIMER,
 * 1141, requests/d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md)
 * and the C library passes them through; the host tests check the library's
 * argument checks against a stand-in kernel that has none, so only here is
 * a timer seen to fire, with the record Linux gives it.
 *
 * Every timing is a wide margin: a 50 ms timer is given a second, a 20 ms
 * period is asked for five signals in 300 ms, and a timer that must not fire
 * is given three times its value. Every busy loop stops after five seconds
 * of wall time, and an alarm ends the whole test after twenty, so it cannot
 * hang. It takes about two seconds.
 *
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  a one-shot timer on CLOCK_MONOTONIC: made (10); its signal comes
 *       (11) once (12), with SI_TIMER, the timer's id and the event's value
 *       (13); once it has fired it reads as disarmed (15)
 *   2x  a periodic one: five signals in 300 ms (20); with its signal blocked
 *       for 200 ms, one signal comes when it is let through, its si_overrun
 *       counting the expiries it stood for (21), as timer_getoverrun does
 *       (22)
 *   3x  SIGEV_NONE: armed for ten seconds, timer_gettime says between nine
 *       and ten are left (30); TIMER_ABSTIME on CLOCK_REALTIME fires at the
 *       time given (31)
 *   5x  SIGEV_THREAD_ID: the signal goes to the thread named, which takes it
 *       with sigwaitinfo (50), and not to the main thread (51); 59: a thread
 *       could not be made
 *   6x  a timer on CLOCK_PROCESS_CPUTIME_ID fires once the process has
 *       computed that long (60), and not while it sleeps (61)
 *   7x  refusals: a deleted timer is EINVAL to timer_delete (70) and
 *       timer_gettime (72), a NULL new value to timer_settime EINVAL (71),
 *       CLOCK_MONOTONIC_RAW EOPNOTSUPP (73); a forked child does not have
 *       its parent's timers (75); 79: fork or wait failed
 *   8x  two timers on one signal each deliver (80)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define MS 1000000LL

static int64_t ns_of(clockid_t clock)
{
    struct timespec ts;
    clock_gettime(clock, &ts);
    return (int64_t)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static void busy(int64_t ns)
{
    int64_t start = ns_of(CLOCK_MONOTONIC);
    volatile uint64_t x = 1;
    if (ns > 5000 * MS)
        ns = 5000 * MS;
    while (ns_of(CLOCK_MONOTONIC) - start < ns) {
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

/* Wait up to `ms` for `*count` to reach `want`. */
static int wait_for(atomic_int *count, int want, long ms)
{
    for (long waited = 0; waited < ms; waited += 5) {
        if (atomic_load(count) >= want)
            return 1;
        pause_ms(5);
    }
    return atomic_load(count) >= want;
}

static atomic_int hits;
static atomic_int last_code, last_timerid, last_overrun;
static atomic_long last_value;
static atomic_long main_tid;
static atomic_int main_saw;

static void on_timer(int sig, siginfo_t *si, void *ctx)
{
    (void)sig;
    (void)ctx;
    atomic_store(&last_code, si->si_code);
    atomic_store(&last_timerid, si->si_timerid);
    atomic_store(&last_overrun, si->si_overrun);
    atomic_store(&last_value, (long)si->si_value.sival_int);
    if ((long)gettid() == atomic_load(&main_tid))
        atomic_fetch_add(&main_saw, 1);
    atomic_fetch_add(&hits, 1);
}

static void handle(int sig)
{
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_timer;
    sa.sa_flags = SA_SIGINFO | SA_RESTART;
    sigemptyset(&sa.sa_mask);
    sigaction(sig, &sa, 0);
}

static struct itimerspec spec(int64_t first_ns, int64_t period_ns)
{
    struct itimerspec its;
    its.it_value.tv_sec = first_ns / 1000000000LL;
    its.it_value.tv_nsec = first_ns % 1000000000LL;
    its.it_interval.tv_sec = period_ns / 1000000000LL;
    its.it_interval.tv_nsec = period_ns % 1000000000LL;
    return its;
}

static int make(clockid_t clock, int notify, int sig, int value, timer_t *t)
{
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = notify;
    ev.sigev_signo = sig;
    ev.sigev_value.sival_int = value;
    return timer_create(clock, &ev, t);
}

static int check_one_shot(int sig)
{
    timer_t t;
    if (make(CLOCK_MONOTONIC, SIGEV_SIGNAL, sig, 1234, &t) != 0)
        return 10;
    atomic_store(&hits, 0);
    struct itimerspec its = spec(50 * MS, 0);
    if (timer_settime(t, 0, &its, 0) != 0)
        return 10;
    if (!wait_for(&hits, 1, 1000))
        return 11;
    pause_ms(150);
    if (atomic_load(&hits) != 1)
        return 12;
    if (atomic_load(&last_code) != SI_TIMER || atomic_load(&last_value) != 1234)
        return 13;
    struct itimerspec now;
    if (timer_gettime(t, &now) != 0 || now.it_value.tv_sec != 0 || now.it_value.tv_nsec != 0)
        return 15;
    timer_delete(t);
    return 0;
}

static int check_periodic(int sig)
{
    timer_t t;
    if (make(CLOCK_MONOTONIC, SIGEV_SIGNAL, sig, 7, &t) != 0)
        return 20;
    atomic_store(&hits, 0);
    struct itimerspec its = spec(20 * MS, 20 * MS);
    timer_settime(t, 0, &its, 0);
    pause_ms(300);
    if (atomic_load(&hits) < 5)
        return 20;

    /* Block it while it fires every 10 ms: one signal stands for them all.
     * It is taken while still blocked, and timer_getoverrun asked at once:
     * the next signal, or a timer_settime, would reset what it reports. */
    sigset_t set, old;
    sigemptyset(&set);
    sigaddset(&set, sig);
    its = spec(10 * MS, 10 * MS);
    pthread_sigmask(SIG_BLOCK, &set, &old);
    timer_settime(t, 0, &its, 0);
    pause_ms(200);
    siginfo_t si;
    struct timespec limit = {1, 0};
    int rc = 0;
    if (sigtimedwait(&set, &si, &limit) != sig || si.si_code != SI_TIMER || si.si_overrun < 5)
        rc = 21;
    else if (timer_getoverrun(t) != si.si_overrun)
        rc = 22;
    struct itimerspec off = spec(0, 0);
    timer_settime(t, 0, &off, 0);
    struct timespec none = {0, 0};
    while (sigtimedwait(&set, &si, &none) == sig) {
    }
    pthread_sigmask(SIG_SETMASK, &old, 0);
    timer_delete(t);
    return rc;
}

static int check_none_and_absolute(int sig)
{
    timer_t t;
    if (make(CLOCK_MONOTONIC, SIGEV_NONE, 0, 0, &t) != 0)
        return 30;
    struct itimerspec its = spec(10000 * MS, 0), now;
    timer_settime(t, 0, &its, 0);
    pause_ms(10);
    if (timer_gettime(t, &now) != 0 || now.it_value.tv_sec < 9 || now.it_value.tv_sec >= 10)
        return 30;
    timer_delete(t);

    if (make(CLOCK_REALTIME, SIGEV_SIGNAL, sig, 31, &t) != 0)
        return 31;
    atomic_store(&hits, 0);
    int64_t at = ns_of(CLOCK_REALTIME) + 100 * MS;
    its = spec(at, 0);
    timer_settime(t, TIMER_ABSTIME, &its, 0);
    if (!wait_for(&hits, 1, 1000) || ns_of(CLOCK_REALTIME) < at)
        return 31;
    timer_delete(t);
    return 0;
}

static atomic_int waiter_tid, waiter_got;
static int thread_sig;

static void *waiter(void *arg)
{
    (void)arg;
    atomic_store(&waiter_tid, (int)gettid());
    sigset_t set;
    sigemptyset(&set);
    sigaddset(&set, thread_sig);
    siginfo_t si;
    struct timespec limit = {2, 0};
    if (sigtimedwait(&set, &si, &limit) == thread_sig && si.si_code == SI_TIMER)
        atomic_store(&waiter_got, 1);
    return 0;
}

static int check_thread_id(int sig)
{
    thread_sig = sig;
    sigset_t set, old;
    sigemptyset(&set);
    sigaddset(&set, sig);
    /* Blocked in both threads: the waiter takes it with sigtimedwait. */
    pthread_sigmask(SIG_BLOCK, &set, &old);
    pthread_t th;
    atomic_store(&waiter_tid, 0);
    atomic_store(&waiter_got, 0);
    if (pthread_create(&th, 0, waiter, 0) != 0)
        return 59;
    while (atomic_load(&waiter_tid) == 0)
        pause_ms(1);
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD_ID;
    ev.sigev_signo = sig;
#ifdef sigev_notify_thread_id
    ev.sigev_notify_thread_id = atomic_load(&waiter_tid);
#else
    ev._sigev_un._tid = atomic_load(&waiter_tid);
#endif
    timer_t t;
    int rc = 0;
    if (timer_create(CLOCK_MONOTONIC, &ev, &t) != 0) {
        rc = 50;
    } else {
        atomic_store(&main_saw, 0);
        struct itimerspec its = spec(30 * MS, 0);
        timer_settime(t, 0, &its, 0);
    }
    pthread_join(th, 0);
    if (rc == 0) {
        timer_delete(t);
        if (!atomic_load(&waiter_got))
            rc = 50;
    }
    sigset_t pending;
    sigpending(&pending);
    pthread_sigmask(SIG_SETMASK, &old, 0);
    if (rc == 0 && (atomic_load(&main_saw) != 0 || sigismember(&pending, sig)))
        rc = 51;
    return rc;
}

static int check_cpu_timer(int sig)
{
    timer_t t;
    if (make(CLOCK_PROCESS_CPUTIME_ID, SIGEV_SIGNAL, sig, 60, &t) != 0)
        return 60;
    atomic_store(&hits, 0);
    struct itimerspec its = spec(100 * MS, 0);
    timer_settime(t, 0, &its, 0);
    pause_ms(300);
    if (atomic_load(&hits) != 0)
        return 61;
    busy(600 * MS);
    if (atomic_load(&hits) != 1)
        return 60;
    timer_delete(t);
    return 0;
}

static int check_refusals(void)
{
    timer_t t;
    if (make(CLOCK_MONOTONIC, SIGEV_NONE, 0, 0, &t) != 0)
        return 70;
    pid_t child = fork();
    if (child < 0)
        return 79;
    if (child == 0) {
        struct itimerspec now;
        errno = 0;
        _exit(timer_gettime(t, &now) == -1 && errno == EINVAL ? 0 : 1);
    }
    int status;
    if (waitpid(child, &status, 0) != child)
        return 79;
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
        return 75;
    errno = 0;
    if (timer_settime(t, 0, 0, 0) != -1 || errno != EINVAL)
        return 71;
    if (timer_delete(t) != 0)
        return 70;
    errno = 0;
    if (timer_delete(t) != -1 || errno != EINVAL)
        return 70;
    struct itimerspec now;
    errno = 0;
    if (timer_gettime(t, &now) != -1 || errno != EINVAL)
        return 72;
    errno = 0;
    if (timer_create(CLOCK_MONOTONIC_RAW, 0, &t) != -1 || errno != EOPNOTSUPP)
        return 73;
    return 0;
}

static int check_two_on_one_signal(int sig)
{
    timer_t a, b;
    if (make(CLOCK_MONOTONIC, SIGEV_SIGNAL, sig, 1, &a) != 0 || make(CLOCK_MONOTONIC, SIGEV_SIGNAL, sig, 2, &b) != 0)
        return 80;
    atomic_store(&hits, 0);
    struct itimerspec its = spec(30 * MS, 0);
    timer_settime(a, 0, &its, 0);
    timer_settime(b, 0, &its, 0);
    if (!wait_for(&hits, 2, 1000))
        return 80;
    timer_delete(a);
    timer_delete(b);
    return 0;
}

int main(void)
{
    alarm(20);
    atomic_store(&main_tid, (long)gettid());
    int sig = SIGRTMIN + 2;
    handle(sig);
    int rc;
    if ((rc = check_one_shot(sig)) != 0)
        return rc;
    if ((rc = check_periodic(sig)) != 0)
        return rc;
    if ((rc = check_none_and_absolute(sig)) != 0)
        return rc;
    if ((rc = check_thread_id(SIGRTMIN + 3)) != 0)
        return rc;
    if ((rc = check_cpu_timer(sig)) != 0)
        return rc;
    if ((rc = check_refusals()) != 0)
        return rc;
    if ((rc = check_two_on_one_signal(sig)) != 0)
        return rc;
    return 42;
}
