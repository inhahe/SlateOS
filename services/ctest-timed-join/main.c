/*
 * ctest-timed-join -- do pthread_tryjoin_np and the timed joins answer as
 * glibc's, now that they ask the kernel?
 *
 * pthread_tryjoin_np, pthread_timedjoin_np and pthread_clockjoin_np looked
 * at a thread's own word that it had exited, every millisecond, until
 * 2026-10-07: a thread killed before it could say so was "still running"
 * for ever. They ask the kernel now (SYS_THREAD_JOIN_TIMEOUT, lane A's),
 * which knows, and which waits for the time left in one call
 * (posix/src/pthread.rs, kernel_join). This checks the answers every
 * program sees, which are glibc's:
 *
 *    1x  tryjoin of a thread still running: EBUSY, and it is not joined
 *    2x  timedjoin with a deadline 100 ms off, of a thread that runs on:
 *        ETIMEDOUT, after about 100 ms and not long after
 *    3x  timedjoin with a deadline 2 s off, of a thread that exits 100 ms
 *        in: joined, with its value, well before the deadline
 *    4x  tryjoin of a thread that has exited: joined, with its value
 *    5x  clockjoin on CLOCK_MONOTONIC: joined; on CLOCK_PROCESS_CPUTIME_ID:
 *        EINVAL
 *
 * Exit codes: 42 is every check passed.
 *    2  pthread_create failed     Otherwise <check><step>.
 *
 * The same source compiles and passes on Linux (gcc -pthread main.c &&
 * ./a.out), which is how its expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <stdatomic.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

static void emit_num(long n)
{
    char buf[24];
    int i = (int)sizeof buf;
    int neg = n < 0;
    unsigned long v = neg ? 0UL - (unsigned long)n : (unsigned long)n;
    buf[--i] = '\0';
    do {
        buf[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v != 0 && i > 1);
    if (neg) {
        buf[--i] = '-';
    }
    emit(buf + i);
}

static int fail(int code, const char *what, int err)
{
    emit("ctest-timed-join: FAIL ");
    emit_num(code);
    emit(": ");
    emit(what);
    emit(" (answer ");
    emit_num(err);
    emit(")\n");
    return code;
}

static void ok(const char *what)
{
    emit("ctest-timed-join: ok ");
    emit(what);
    emit("\n");
}

static long now_ms(clockid_t clock)
{
    struct timespec t;
    clock_gettime(clock, &t);
    return t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

static void sleep_ms(long ms)
{
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {
    }
}

/* A deadline `ms` from now on `clock`. */
static struct timespec in_ms(clockid_t clock, long ms)
{
    struct timespec t;
    clock_gettime(clock, &t);
    t.tv_sec += ms / 1000;
    t.tv_nsec += (ms % 1000) * 1000000L;
    if (t.tv_nsec >= 1000000000L) {
        t.tv_sec += 1;
        t.tv_nsec -= 1000000000L;
    }
    return t;
}

static atomic_int release_waiter;

/* Runs until told to stop; returns 7. */
static void *waits(void *unused)
{
    (void)unused;
    while (!atomic_load(&release_waiter)) {
        sleep_ms(5);
    }
    return (void *)7;
}

/* Sleeps `arg` milliseconds, then returns `arg`. */
static void *sleeps(void *arg)
{
    sleep_ms((long)arg);
    return arg;
}

int main(void)
{
    pthread_t t;
    void *got = NULL;
    int r;

    /* 1 and 2: a thread that runs until it is told. */
    if (pthread_create(&t, NULL, waits, NULL) != 0) {
        return fail(2, "pthread_create", 0);
    }
    r = pthread_tryjoin_np(t, &got);
    if (r != EBUSY) {
        return fail(11, "tryjoin of a running thread is EBUSY", r);
    }
    ok("tryjoin of a running thread is EBUSY");

    struct timespec soon = in_ms(CLOCK_REALTIME, 100);
    long start = now_ms(CLOCK_MONOTONIC);
    r = pthread_timedjoin_np(t, &got, &soon);
    long took = now_ms(CLOCK_MONOTONIC) - start;
    if (r != ETIMEDOUT) {
        return fail(21, "timedjoin of a thread that runs on times out", r);
    }
    if (took < 90 || took > 2000) {
        emit("ctest-timed-join: it took ");
        emit_num(took);
        emit(" ms\n");
        return fail(22, "the timeout came after about 100 ms", r);
    }
    atomic_store(&release_waiter, 1);
    if ((r = pthread_join(t, &got)) != 0 || got != (void *)7) {
        return fail(23, "the thread joins once released, with 7", r);
    }
    ok("timedjoin times out on time, and the thread is still joinable");

    /* 3: a wait the thread's exit ends. */
    if (pthread_create(&t, NULL, sleeps, (void *)100) != 0) {
        return fail(2, "pthread_create", 0);
    }
    struct timespec later = in_ms(CLOCK_REALTIME, 2000);
    start = now_ms(CLOCK_MONOTONIC);
    r = pthread_timedjoin_np(t, &got, &later);
    took = now_ms(CLOCK_MONOTONIC) - start;
    if (r != 0 || got != (void *)100) {
        return fail(31, "timedjoin joins the thread when it exits, with its value", r);
    }
    if (took > 1500) {
        return fail(32, "and well before the deadline", r);
    }
    ok("timedjoin returns when the thread exits, not at the deadline");

    /* 4: tryjoin once it has exited. */
    if (pthread_create(&t, NULL, sleeps, (void *)1) != 0) {
        return fail(2, "pthread_create", 0);
    }
    sleep_ms(200);
    r = pthread_tryjoin_np(t, &got);
    if (r != 0 || got != (void *)1) {
        return fail(41, "tryjoin of an exited thread joins it, with its value", r);
    }
    ok("tryjoin of an exited thread joins it");

    /* 5: clockjoin. */
    if (pthread_create(&t, NULL, sleeps, (void *)50) != 0) {
        return fail(2, "pthread_create", 0);
    }
    struct timespec bad = in_ms(CLOCK_MONOTONIC, 1000);
    r = pthread_clockjoin_np(t, &got, CLOCK_PROCESS_CPUTIME_ID, &bad);
    if (r != EINVAL) {
        return fail(51, "clockjoin on CLOCK_PROCESS_CPUTIME_ID is EINVAL", r);
    }
    struct timespec mono = in_ms(CLOCK_MONOTONIC, 2000);
    r = pthread_clockjoin_np(t, &got, CLOCK_MONOTONIC, &mono);
    if (r != 0 || got != (void *)50) {
        return fail(52, "clockjoin on CLOCK_MONOTONIC joins, with the value", r);
    }
    ok("clockjoin takes CLOCK_MONOTONIC, and refuses a clock it cannot wait on");

    emit("ctest-timed-join: every check passed\n");
    return 42;
}
