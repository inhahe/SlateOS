/*
 * ctest-sigev-thread -- ring-3 test of SIGEV_THREAD timers, and of the two
 * signals the C library keeps for itself.
 *
 * Until 2026-10-16 timer_create refused SIGEV_THREAD with ENOSYS. It is the
 * library's now, glibc's way: a helper thread waits for the library's own
 * signal (32), which each such timer's kernel timer sends it, and starts a
 * new detached thread for each expiry, which runs the function. SIGRTMIN is
 * 34 for programs, as glibc's is. The host tests cannot make the helper (the
 * host has no threads of this library's), so only here are the timers seen
 * to run their functions.
 *
 * Every wait has a deadline of a second, and an alarm ends the test after
 * twenty, so it cannot hang.
 *
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  a one-shot SIGEV_THREAD timer is made (10); its function runs (11)
 *       with the event's value (12), on a thread that is not the main one
 *       (13), with every signal blocked, SIGUSR2 too, which the main thread
 *       does not block -- glibc's timer threads run so (15)
 *   2x  a periodic one runs its function five times in 300 ms (20), and not
 *       again once it is deleted (21)
 *   3x  attributes given for its threads -- a stack size -- and destroyed
 *       once timer_create returns, still make threads that run (30)
 *   5x  SIGRTMIN is 34 (50); signal 32 cannot be put in a set (51) or given
 *       an action (52)
 *   6x  two SIGEV_THREAD timers each run their own function (60)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define MS 1000000LL

static atomic_int calls, last_value, last_tid, saw_blocked = -1;
static atomic_int calls_a, calls_b;
static int main_tid;

static void pause_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
}

static int wait_for(atomic_int *count, int want, long ms)
{
    for (long waited = 0; waited < ms; waited += 5) {
        if (atomic_load(count) >= want)
            return 1;
        pause_ms(5);
    }
    return atomic_load(count) >= want;
}

static void notified(union sigval v)
{
    sigset_t now;
    pthread_sigmask(SIG_BLOCK, 0, &now);
    atomic_store(&saw_blocked, sigismember(&now, SIGUSR1) && sigismember(&now, SIGUSR2));
    atomic_store(&last_value, v.sival_int);
    atomic_store(&last_tid, (int)gettid());
    atomic_fetch_add(&calls, 1);
}

static void notified_a(union sigval v)
{
    (void)v;
    atomic_fetch_add(&calls_a, 1);
}

static void notified_b(union sigval v)
{
    (void)v;
    atomic_fetch_add(&calls_b, 1);
}

static int make(void (*fn)(union sigval), int value, pthread_attr_t *attr, timer_t *t)
{
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = fn;
    ev.sigev_value.sival_int = value;
    ev.sigev_notify_attributes = attr;
    return timer_create(CLOCK_MONOTONIC, &ev, t);
}

static struct itimerspec spec(long long first_ns, long long period_ns)
{
    struct itimerspec its;
    its.it_value.tv_sec = first_ns / 1000000000LL;
    its.it_value.tv_nsec = first_ns % 1000000000LL;
    its.it_interval.tv_sec = period_ns / 1000000000LL;
    its.it_interval.tv_nsec = period_ns % 1000000000LL;
    return its;
}

int main(void)
{
    alarm(20);
    main_tid = (int)gettid();
    sigset_t usr1;
    sigemptyset(&usr1);
    sigaddset(&usr1, SIGUSR1);
    pthread_sigmask(SIG_BLOCK, &usr1, 0);

    timer_t t;
    if (make(notified, 7, 0, &t) != 0)
        return 10;
    struct itimerspec once = spec(30 * MS, 0);
    timer_settime(t, 0, &once, 0);
    if (!wait_for(&calls, 1, 1000))
        return 11;
    if (atomic_load(&last_value) != 7)
        return 12;
    if (atomic_load(&last_tid) == main_tid)
        return 13;
    if (atomic_load(&saw_blocked) != 1)
        return 15;
    timer_delete(t);

    atomic_store(&calls, 0);
    if (make(notified, 9, 0, &t) != 0)
        return 20;
    struct itimerspec every = spec(20 * MS, 20 * MS);
    timer_settime(t, 0, &every, 0);
    pause_ms(300);
    if (atomic_load(&calls) < 5)
        return 20;
    timer_delete(t);
    pause_ms(50);
    int after = atomic_load(&calls);
    pause_ms(150);
    if (atomic_load(&calls) != after)
        return 21;

    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_attr_setstacksize(&attr, 256 * 1024);
    atomic_store(&calls, 0);
    int made = make(notified, 11, &attr, &t);
    pthread_attr_destroy(&attr);
    if (made != 0)
        return 30;
    timer_settime(t, 0, &once, 0);
    if (!wait_for(&calls, 1, 1000) || atomic_load(&last_value) != 11)
        return 30;
    timer_delete(t);

    if (SIGRTMIN != 34)
        return 50;
    sigset_t set;
    sigemptyset(&set);
    errno = 0;
    if (sigaddset(&set, 32) != -1 || errno != EINVAL)
        return 51;
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = SIG_IGN;
    errno = 0;
    if (sigaction(32, &sa, 0) != -1 || errno != EINVAL)
        return 52;

    timer_t a, b;
    if (make(notified_a, 1, 0, &a) != 0 || make(notified_b, 2, 0, &b) != 0)
        return 60;
    timer_settime(a, 0, &once, 0);
    timer_settime(b, 0, &once, 0);
    if (!wait_for(&calls_a, 1, 1000) || !wait_for(&calls_b, 1, 1000))
        return 60;
    timer_delete(a);
    timer_delete(b);
    return 42;
}
