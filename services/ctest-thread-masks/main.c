/*
 * ctest-thread-masks -- ring-3 test of signal masks that are each thread's.
 *
 * Until 2026-10-16 the C library kept one blocked mask for the process,
 * though the kernel's are each thread's since lane A's design-decisions
 * 1542: one thread's sigprocmask was what every other thread read back and
 * the next mask any of them wrote to the kernel, and a thread waiting in
 * sigwait could be handed a signal the kernel had aimed at another thread.
 * The host tests cannot see threads apart (the host's storage is per test
 * thread either way), so only here is it seen.
 *
 * Every wait has a deadline -- a second at most -- and an alarm ends the
 * test after twenty, so it cannot hang.
 *
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  a thread made while its creator blocks SIGUSR1 starts with it
 *       blocked (10); its unblocking it leaves the creator's mask as it was
 *       (11); 19: a thread could not be made
 *   2x  a signal sent to the process while the main thread blocks it and
 *       another thread does not runs that thread's handler (20), and is not
 *       left pending for the main thread (21)
 *   3x  a signal sent to the main thread, which blocks it, stays the main
 *       thread's: another thread that does not block it never runs its
 *       handler for it (30), and the main thread takes it with sigwait (31)
 *   5x  a signal sent to the main thread while another thread waits for it
 *       in sigtimedwait is not that thread's: its wait runs out (50), and
 *       the main thread takes the signal (51)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static pthread_t main_thread;
static atomic_int usr1_tid, usr2_runs;
static atomic_int stop_helper, helper_ready;
static atomic_int born_blocked = -1, unblocked = -1;
static atomic_int helper_waited = -1;

static void pause_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
}

static void on_usr1(int sig)
{
    (void)sig;
    atomic_store(&usr1_tid, (int)gettid());
}

static void on_usr2(int sig)
{
    (void)sig;
    atomic_fetch_add(&usr2_runs, 1);
}

static int blocks(int sig)
{
    sigset_t now;
    pthread_sigmask(SIG_BLOCK, 0, &now);
    return sigismember(&now, sig);
}

/* Starts with the creator's mask (SIGUSR1 blocked), says so, then
 * unblocks SIGUSR1 and SIGUSR2 and stays until told to stop. */
static void *helper(void *arg)
{
    (void)arg;
    atomic_store(&born_blocked, blocks(SIGUSR1));
    sigset_t both;
    sigemptyset(&both);
    sigaddset(&both, SIGUSR1);
    sigaddset(&both, SIGUSR2);
    pthread_sigmask(SIG_UNBLOCK, &both, 0);
    atomic_store(&unblocked, 1);
    atomic_store(&helper_ready, 1);
    for (int i = 0; i < 4000 && !atomic_load(&stop_helper); i++)
        pause_ms(5);
    return 0;
}

/* Waits for SIGUSR2 in sigtimedwait for a second, blocking it. */
static void *waiter(void *arg)
{
    (void)arg;
    sigset_t set;
    sigemptyset(&set);
    sigaddset(&set, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &set, 0);
    atomic_store(&helper_ready, 1);
    struct timespec limit = {1, 0};
    errno = 0;
    int got = sigtimedwait(&set, 0, &limit);
    atomic_store(&helper_waited, got == -1 && errno == EAGAIN ? 0 : got);
    return 0;
}

static int take(int sig)
{
    sigset_t set;
    sigemptyset(&set);
    sigaddset(&set, sig);
    struct timespec limit = {1, 0};
    return sigtimedwait(&set, 0, &limit) == sig;
}

int main(void)
{
    alarm(20);
    main_thread = pthread_self();
    signal(SIGUSR1, on_usr1);
    signal(SIGUSR2, on_usr2);

    sigset_t usr1, usr2;
    sigemptyset(&usr1);
    sigaddset(&usr1, SIGUSR1);
    sigemptyset(&usr2);
    sigaddset(&usr2, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &usr1, 0);

    pthread_t h;
    if (pthread_create(&h, 0, helper, 0) != 0)
        return 19;
    while (!atomic_load(&helper_ready))
        pause_ms(1);
    if (atomic_load(&born_blocked) != 1)
        return 10;
    if (!blocks(SIGUSR1))
        return 11;

    /* 2x: to the process; only the helper lets it through. */
    kill(getpid(), SIGUSR1);
    for (int i = 0; i < 200 && atomic_load(&usr1_tid) == 0; i++)
        pause_ms(5);
    if (atomic_load(&usr1_tid) == 0 || atomic_load(&usr1_tid) == (int)gettid())
        return 20;
    sigset_t pending;
    sigpending(&pending);
    if (sigismember(&pending, SIGUSR1))
        return 21;

    /* 3x: to the main thread, which blocks it. */
    pthread_sigmask(SIG_BLOCK, &usr2, 0);
    pthread_kill(main_thread, SIGUSR2);
    pause_ms(200);
    if (atomic_load(&usr2_runs) != 0)
        return 30;
    if (!take(SIGUSR2))
        return 31;
    atomic_store(&stop_helper, 1);
    pthread_join(h, 0);

    /* 5x: to the main thread while another thread waits for it. */
    atomic_store(&helper_ready, 0);
    pthread_t w;
    if (pthread_create(&w, 0, waiter, 0) != 0)
        return 19;
    while (!atomic_load(&helper_ready))
        pause_ms(1);
    pause_ms(50);
    pthread_kill(main_thread, SIGUSR2);
    pthread_join(w, 0);
    if (atomic_load(&helper_waited) != 0)
        return 50;
    if (!take(SIGUSR2))
        return 51;
    return 42;
}
