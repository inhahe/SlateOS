/*
 * ctest-pi-mutex -- ring-3 test of PTHREAD_PRIO_INHERIT mutexes, over the
 * kernel's priority-inheritance futexes.
 *
 * Until 2026-10-06 pthread_mutex_init refused the protocol with ENOTSUP.
 * It now makes a mutex whose word is a kernel PI futex: a thread that finds
 * it held sleeps in the kernel, which lends the holder the sleeper's
 * priority until the holder lets go.
 *
 * The host tests run the library's half against a stand-in kernel that has
 * no priorities; the lending is what only this can check.  It is the
 * classic inversion: a low-priority thread holds the mutex, medium-priority
 * threads keep every CPU busy, and a high-priority thread asks for the
 * mutex.  Lent the high priority, the holder runs ahead of the medium
 * threads and is done in its own few tens of milliseconds; not lent it, it
 * waits behind them for the scheduler's anti-starvation boost, two seconds
 * at the least (kernel/src/sched/mod.rs, STARVATION_THRESHOLD_TICKS).  So
 * every timing below is a wide margin, not a race.
 *
 * Priorities are the kernel scheduler's, 0 highest to 31, set with
 * SYS_THREAD_SET_PRIORITY: POSIX's are not mapped to them
 * (pthread_setschedparam refuses every real-time priority with EPERM).
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check and step: the tens digit names the check, the units the step.  No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  PTHREAD_PRIO_INHERIT is taken (10: init refused it), and
 *       PTHREAD_PRIO_PROTECT still refused (11)
 *   2x  one thread: an error-checking PI mutex's owner relocking it is
 *       EDEADLK (20), trying it EBUSY (21), unlocking it twice EPERM (22);
 *       a recursive one counts levels (23); a normal one its owner relocks
 *       with a deadline sleeps it out to ETIMEDOUT (24)
 *   3x  two threads: while one holds it, the other's try is EBUSY (30), its
 *       timed lock ETIMEDOUT (31), its unlock EPERM (32); and its lock,
 *       asleep, is handed the mutex when the holder lets go (33)
 *   5x  the holder is lent the waiter's priority: the waiter has the mutex
 *       within a second (51: it waited behind the medium threads -- nothing
 *       was lent), and once the holder has let go it has its own priority
 *       back (52: it ran on ahead of the medium threads); 50: a priority
 *       could not be set or a thread made
 *   6x  a waiter that gives up takes its loan back: its timed lock ends in
 *       ETIMEDOUT (61); while it waited the holder ran (62: it never did --
 *       nothing was lent); after, the medium threads hold the holder back
 *       again (63: it ran on -- the loan outlived the waiter); 60: setup
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>

/* kernel/src/syscall/number.rs: arg0 the thread (0 = the caller), arg1 the
 * priority; the old priority, or a negative error. */
#define SYS_THREAD_SET_PRIORITY 515

#define PRIO_MAIN 1
#define PRIO_HIGH 4
#define PRIO_MEDIUM 10
#define PRIO_LOW 20

#define MAX_MEDIUM 8

static long set_priority(long prio)
{
    long ret;
    __asm__ volatile("syscall"
                     : "=a"(ret)
                     : "a"((long)SYS_THREAD_SET_PRIORITY), "D"(0L), "S"(prio)
                     : "rcx", "r11", "memory");
    return ret;
}

static int64_t now_us(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000 + ts.tv_nsec / 1000;
}

/* `ms` from now on CLOCK_REALTIME, pthread_mutex_timedlock's clock. */
static struct timespec in_ms(long ms)
{
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += ms / 1000;
    ts.tv_nsec += (ms % 1000) * 1000000L;
    if (ts.tv_nsec >= 1000000000L) {
        ts.tv_sec += 1;
        ts.tv_nsec -= 1000000000L;
    }
    return ts;
}

static void sleep_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
}

static volatile uint64_t sink;

/* `n` rounds of CPU. */
static void spin(long n)
{
    uint64_t x = sink;
    for (long i = 0; i < n; i++)
        x = x * 6364136223846793005ULL + 1442695040888963407ULL;
    sink = x;
}

/* Rounds of spin() a millisecond of CPU buys, measured now. */
static long rounds_per_ms(void)
{
    long n = 100000;
    for (;;) {
        int64_t t0 = now_us();
        spin(n);
        int64_t t = now_us() - t0;
        if (t >= 20000 || n >= (1L << 40))
            return t >= 1000 ? (long)(n / (t / 1000)) : n;
        n *= 2;
    }
}

static int make(pthread_mutex_t *m, int type, int protocol)
{
    pthread_mutexattr_t a;
    if (pthread_mutexattr_init(&a) != 0 || pthread_mutexattr_settype(&a, type) != 0
        || pthread_mutexattr_setprotocol(&a, protocol) != 0)
        return -1;
    int r = pthread_mutex_init(m, &a);
    pthread_mutexattr_destroy(&a);
    return r;
}

/* ---- 3x ---- */

struct pair {
    pthread_mutex_t m;
    atomic_int stage;
    int result[4];
};

static void *other(void *p)
{
    struct pair *x = p;
    while (atomic_load(&x->stage) == 0)
        sleep_ms(1);
    x->result[0] = pthread_mutex_trylock(&x->m);
    struct timespec soon = in_ms(30);
    x->result[1] = pthread_mutex_timedlock(&x->m, &soon);
    x->result[2] = pthread_mutex_unlock(&x->m);
    atomic_store(&x->stage, 2);
    struct timespec later = in_ms(10000);
    x->result[3] = pthread_mutex_timedlock(&x->m, &later);
    if (x->result[3] == 0)
        pthread_mutex_unlock(&x->m);
    return NULL;
}

/* ---- 5x and 6x ---- */

struct scene {
    pthread_mutex_t m;
    atomic_int holds;     /* the low thread has the mutex */
    atomic_int release;   /* 6x: it may let go */
    atomic_int stop;      /* the medium threads stop */
    atomic_long progress; /* 6x: the low thread's rounds of work */
    atomic_int failed;    /* a priority not set, a lock not taken */
    long work;            /* 5x: rounds the low thread works holding it */
    long after;           /* 5x: rounds it works once it has let go */
    int64_t got_us;       /* 5x: how long the high thread's lock took; -1 */
    int64_t done_at;      /* 5x: when the low thread finished, after */
    int timed;            /* 6x: the high thread's timed lock's answer */
};

static void *medium(void *p)
{
    struct scene *s = p;
    if (set_priority(PRIO_MEDIUM) < 0)
        atomic_store(&s->failed, 1);
    int64_t until = now_us() + 5000000;
    while (!atomic_load(&s->stop) && now_us() < until)
        spin(1000);
    return NULL;
}

/* Take the mutex at the low priority, and say so. */
static int low_takes(struct scene *s)
{
    int ok = set_priority(PRIO_LOW) >= 0 && pthread_mutex_lock(&s->m) == 0;
    if (!ok)
        atomic_store(&s->failed, 1);
    atomic_store(&s->holds, 1);
    return ok;
}

static void *low5(void *p)
{
    struct scene *s = p;
    if (!low_takes(s))
        return NULL;
    spin(s->work);
    pthread_mutex_unlock(&s->m);
    spin(s->after);
    s->done_at = now_us();
    return NULL;
}

static void *high5(void *p)
{
    struct scene *s = p;
    if (set_priority(PRIO_HIGH) < 0)
        atomic_store(&s->failed, 1);
    struct timespec limit = in_ms(10000);
    int64_t t0 = now_us();
    if (pthread_mutex_timedlock(&s->m, &limit) == 0) {
        s->got_us = now_us() - t0;
        pthread_mutex_unlock(&s->m);
    } else {
        s->got_us = -1;
    }
    return NULL;
}

static void *low6(void *p)
{
    struct scene *s = p;
    if (!low_takes(s))
        return NULL;
    int64_t until = now_us() + 8000000;
    while (!atomic_load(&s->release) && now_us() < until) {
        spin(1000);
        atomic_fetch_add(&s->progress, 1);
    }
    pthread_mutex_unlock(&s->m);
    return NULL;
}

static void *high6(void *p)
{
    struct scene *s = p;
    if (set_priority(PRIO_HIGH) < 0)
        atomic_store(&s->failed, 1);
    struct timespec soon = in_ms(100);
    s->timed = pthread_mutex_timedlock(&s->m, &soon);
    if (s->timed == 0)
        pthread_mutex_unlock(&s->m);
    return NULL;
}

/* Start the low thread `low` and, once it holds the mutex, `n` medium ones;
 * then give the medium ones the CPUs.  0, or -1 if a thread was not made. */
static int stage(struct scene *s, void *(*low)(void *), pthread_t *lo, pthread_t *med, long n)
{
    if (pthread_create(lo, NULL, low, s) != 0)
        return -1;
    for (int i = 0; i < 5000 && !atomic_load(&s->holds); i++)
        sleep_ms(1);
    if (!atomic_load(&s->holds))
        return -1;
    for (long i = 0; i < n; i++)
        if (pthread_create(&med[i], NULL, medium, s) != 0)
            return -1;
    sleep_ms(20);
    return 0;
}

static void finish(struct scene *s, pthread_t lo, pthread_t *med, long n)
{
    atomic_store(&s->stop, 1);
    atomic_store(&s->release, 1);
    for (long i = 0; i < n; i++)
        pthread_join(med[i], NULL);
    pthread_join(lo, NULL);
}

int main(void)
{
    /* 1x */
    pthread_mutex_t m;
    if (make(&m, PTHREAD_MUTEX_ERRORCHECK, PTHREAD_PRIO_INHERIT) != 0)
        return 10;
    pthread_mutex_t pp;
    if (make(&pp, PTHREAD_MUTEX_NORMAL, PTHREAD_PRIO_PROTECT) != ENOTSUP)
        return 11;

    /* 2x */
    if (pthread_mutex_lock(&m) != 0 || pthread_mutex_lock(&m) != EDEADLK)
        return 20;
    if (pthread_mutex_trylock(&m) != EBUSY)
        return 21;
    if (pthread_mutex_unlock(&m) != 0 || pthread_mutex_unlock(&m) != EPERM)
        return 22;
    pthread_mutex_t r;
    if (make(&r, PTHREAD_MUTEX_RECURSIVE, PTHREAD_PRIO_INHERIT) != 0
        || pthread_mutex_lock(&r) != 0 || pthread_mutex_lock(&r) != 0
        || pthread_mutex_trylock(&r) != 0 || pthread_mutex_unlock(&r) != 0
        || pthread_mutex_unlock(&r) != 0 || pthread_mutex_unlock(&r) != 0
        || pthread_mutex_unlock(&r) != EPERM)
        return 23;
    pthread_mutex_t n;
    if (make(&n, PTHREAD_MUTEX_NORMAL, PTHREAD_PRIO_INHERIT) != 0 || pthread_mutex_lock(&n) != 0)
        return 24;
    struct timespec soon = in_ms(50);
    int64_t t0 = now_us();
    if (pthread_mutex_timedlock(&n, &soon) != ETIMEDOUT || now_us() - t0 < 40000
        || pthread_mutex_unlock(&n) != 0)
        return 24;

    /* 3x */
    static struct pair x;
    pthread_t o;
    if (make(&x.m, PTHREAD_MUTEX_ERRORCHECK, PTHREAD_PRIO_INHERIT) != 0
        || pthread_mutex_lock(&x.m) != 0 || pthread_create(&o, NULL, other, &x) != 0)
        return 30;
    atomic_store(&x.stage, 1);
    while (atomic_load(&x.stage) != 2)
        sleep_ms(1);
    sleep_ms(30); /* asleep in its lock by now */
    if (pthread_mutex_unlock(&x.m) != 0)
        return 33;
    pthread_join(o, NULL);
    if (x.result[0] != EBUSY)
        return 30;
    if (x.result[1] != ETIMEDOUT)
        return 31;
    if (x.result[2] != EPERM)
        return 32;
    if (x.result[3] != 0)
        return 33;

    /* 5x: main above everyone, so that it keeps time while they run. */
    if (set_priority(PRIO_MAIN) < 0)
        return 50;
    long per_ms = rounds_per_ms();
    long cpus = sysconf(_SC_NPROCESSORS_ONLN);
    if (cpus < 1)
        cpus = 1;
    if (cpus > MAX_MEDIUM)
        cpus = MAX_MEDIUM;
    pthread_t lo, hi, med[MAX_MEDIUM];

    static struct scene s5;
    if (make(&s5.m, PTHREAD_MUTEX_NORMAL, PTHREAD_PRIO_INHERIT) != 0)
        return 50;
    s5.work = 50 * per_ms;
    s5.after = 50 * per_ms;
    if (stage(&s5, low5, &lo, med, cpus) != 0 || pthread_create(&hi, NULL, high5, &s5) != 0)
        return 50;
    pthread_join(hi, NULL);
    sleep_ms(300); /* the holder, its own priority back, waits behind them */
    int64_t stopped = now_us();
    finish(&s5, lo, med, cpus);
    if (atomic_load(&s5.failed))
        return 50;
    if (s5.got_us < 0 || s5.got_us > 1000000)
        return 51;
    if (s5.done_at < stopped)
        return 52;

    /* 6x */
    static struct scene s6;
    if (make(&s6.m, PTHREAD_MUTEX_NORMAL, PTHREAD_PRIO_INHERIT) != 0)
        return 60;
    if (stage(&s6, low6, &lo, med, cpus) != 0)
        return 60;
    long before = atomic_load(&s6.progress);
    if (pthread_create(&hi, NULL, high6, &s6) != 0)
        return 60;
    pthread_join(hi, NULL);
    long lent = atomic_load(&s6.progress) - before;
    long p1 = atomic_load(&s6.progress);
    sleep_ms(300);
    long p2 = atomic_load(&s6.progress);
    finish(&s6, lo, med, cpus);
    if (atomic_load(&s6.failed))
        return 60;
    if (s6.timed != ETIMEDOUT)
        return 61;
    if (lent < 10)
        return 62;
    if (p2 - p1 > lent / 4)
        return 63;
    return 42;
}
