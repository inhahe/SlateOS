/*
 * ctest-pthread — native C ring-3 fixture for pthread_create's attributes and
 * for the futex-based synchronisation objects.
 *
 * Compiled by `zig cc --target=x86_64-linux-musl` and linked against the
 * posix sysroot `libc.a` (see build.py).  The kernel runs it as a ring-3
 * self-test and asserts the exit code (requested of lane A in
 * `requests/d-a-run-the-ctest-pthread-fixture.md`).
 *
 * Why it exists.  Two pieces of posix/src/pthread.rs can only be tested where
 * threads really run:
 *
 *   - pthread_create honours its attribute: the stack size (it used to give
 *     every thread 64 KiB, so a thread that asked for more and used it ran
 *     off the end of its stack), an inaccessible guard below the stack, a
 *     stack the caller supplies, and PTHREAD_CREATE_DETACHED; and its thread
 *     table grows past 64 threads (the 65th used to run untracked).
 *   - mutexes, condition variables, barriers and pthread_once sleep on
 *     futexes (SYS_FUTEX_WAIT/WAKE) instead of polling.  The host tests drive
 *     the same algorithms with std threads, but on the host a futex wait is a
 *     yield: only here does a sleeper depend on being woken.
 *
 * Exit codes: 42 = every check passed.  Anything else names the failing step:
 *
 *   50  pthread_attr_init/setstacksize failed          (big stack)
 *   51  pthread_create with a 1 MiB stack failed
 *   52  pthread_join of it failed
 *   53  pthread_getattr_np in the thread failed
 *   54  the thread's stack is smaller than the 1 MiB it asked for
 *   55  the thread's 768 KiB buffer is not inside its own stack
 *   56  the guard is not one 16 KiB page
 *   57  the buffer did not hold what the thread wrote into it
 *   (a thread that ran off a 64 KiB stack faults: no exit code at all)
 *
 *   60  pthread_create of a PTHREAD_CREATE_DETACHED thread failed
 *   61  pthread_join of the detached thread did not answer EINVAL
 *   62  pthread_detach of it did not answer EINVAL
 *   63  the detached thread never finished
 *
 *   70  pthread_barrier_init failed                    (more than 64 threads)
 *   71  pthread_create failed before 70 threads were running
 *   72  pthread_getattr_np failed for one of them
 *   73  one of them reported the *main* thread's stack -- it is untracked
 *   74  pthread_join of one of them failed or returned the wrong value
 *
 *   80  pthread_create on a stack the caller supplied failed
 *   81  pthread_join of it failed
 *   82  the thread did not run on the supplied stack
 *   83  a second thread on the same stack, after the first was joined, failed
 *
 *   90  pthread_create failed                          (mutex contention)
 *   91  a contended mutex lost an increment
 *
 *   100 pthread_create failed                          (condition variable)
 *   101 the consumer saw values out of order or missed one
 *
 *   110 pthread_create failed                          (pthread_once)
 *   111 the once routine ran other than exactly once
 *
 * Progress lines go to stdout as "[pt] ...".
 */

/* Declared locally rather than via <pthread.h>: the musl headers zig ships
 * describe musl's ABI, while the symbols come from our posix libc.a.  The
 * sizes below are musl's x86_64 ones, which ours match (the ABI gate checks
 * that); pthread_t is our 64-bit kernel task id. */
typedef unsigned long pthread_t;
typedef struct { unsigned long s[7]; } pthread_attr_t;    /* 56 bytes */
typedef struct { unsigned long s[5]; } pthread_mutex_t;   /* 40 bytes */
typedef struct { unsigned long s[6]; } pthread_cond_t;    /* 48 bytes */
typedef struct { unsigned long s[4]; } pthread_barrier_t; /* 32 bytes */
typedef int pthread_once_t;

#define PTHREAD_CREATE_DETACHED 1
#define PTHREAD_ONCE_INIT 0
#define EINVAL 22

extern int pthread_create(pthread_t *thread, const pthread_attr_t *attr,
                          void *(*start)(void *), void *arg);
extern int pthread_join(pthread_t thread, void **retval);
extern int pthread_detach(pthread_t thread);
extern pthread_t pthread_self(void);
extern int pthread_attr_init(pthread_attr_t *attr);
extern int pthread_attr_setstacksize(pthread_attr_t *attr, unsigned long size);
extern int pthread_attr_setdetachstate(pthread_attr_t *attr, int state);
extern int pthread_attr_setstack(pthread_attr_t *attr, void *addr,
                                 unsigned long size);
extern int pthread_attr_getstack(const pthread_attr_t *attr, void **addr,
                                 unsigned long *size);
extern int pthread_attr_getguardsize(const pthread_attr_t *attr,
                                     unsigned long *size);
extern int pthread_getattr_np(pthread_t thread, pthread_attr_t *attr);
extern int pthread_mutex_init(pthread_mutex_t *m, const void *attr);
extern int pthread_mutex_lock(pthread_mutex_t *m);
extern int pthread_mutex_unlock(pthread_mutex_t *m);
extern int pthread_cond_init(pthread_cond_t *c, const void *attr);
extern int pthread_cond_wait(pthread_cond_t *c, pthread_mutex_t *m);
extern int pthread_cond_signal(pthread_cond_t *c);
extern int pthread_barrier_init(pthread_barrier_t *b, const void *attr,
                                unsigned count);
extern int pthread_barrier_wait(pthread_barrier_t *b);
extern int pthread_once(pthread_once_t *once, void (*init)(void));
extern int sched_yield(void);
extern int printf(const char *fmt, ...);

/* ---- 1. a thread gets the stack it asked for ------------------------- */

#define BIG_STACK (1UL << 20)
#define BIG_BUFFER (768UL * 1024)

static int big_stack_worker_result = -1;

static void *big_stack_worker(void *arg)
{
    (void)arg;
    /* 768 KiB of locals: three quarters of the megabyte asked for, and
     * twelve times the 64 KiB every thread used to get.  Touch a byte on
     * every page, lowest address first -- on a 64 KiB stack the first write
     * lands far below the mapping. */
    volatile unsigned char buf[BIG_BUFFER];
    unsigned long i;
    for (i = 0; i < BIG_BUFFER; i += 4096) {
        buf[i] = (unsigned char)(i >> 12);
    }
    for (i = 0; i < BIG_BUFFER; i += 4096) {
        if (buf[i] != (unsigned char)(i >> 12)) {
            big_stack_worker_result = 57;
            return 0;
        }
    }

    pthread_attr_t a;
    void *addr = 0;
    unsigned long size = 0, guard = 0;
    if (pthread_getattr_np(pthread_self(), &a) != 0 ||
        pthread_attr_getstack(&a, &addr, &size) != 0 ||
        pthread_attr_getguardsize(&a, &guard) != 0) {
        big_stack_worker_result = 53;
        return 0;
    }
    if (size < BIG_STACK) {
        big_stack_worker_result = 54;
        return 0;
    }
    unsigned char *lo = (unsigned char *)addr;
    const volatile unsigned char *b = buf;
    if ((const unsigned char *)b < lo ||
        (const unsigned char *)b + BIG_BUFFER > lo + size) {
        big_stack_worker_result = 55;
        return 0;
    }
    if (guard != 16384) {
        big_stack_worker_result = 56;
        return 0;
    }
    big_stack_worker_result = 0;
    return 0;
}

static int check_big_stack(void)
{
    pthread_attr_t attr;
    pthread_t t;
    if (pthread_attr_init(&attr) != 0 ||
        pthread_attr_setstacksize(&attr, BIG_STACK) != 0) {
        return 50;
    }
    if (pthread_create(&t, &attr, big_stack_worker, 0) != 0) {
        return 51;
    }
    if (pthread_join(t, 0) != 0) {
        return 52;
    }
    return big_stack_worker_result;
}

/* ---- 2. PTHREAD_CREATE_DETACHED means detached from the start -------- */

static volatile int detached_go;
static volatile int detached_done;

static void *detached_worker(void *arg)
{
    (void)arg;
    /* Stay alive until main has asked its questions, so the answers do not
     * depend on whether this thread has already gone. */
    while (!detached_go) {
        sched_yield();
    }
    detached_done = 1;
    return 0;
}

static int check_detached(void)
{
    pthread_attr_t attr;
    pthread_t t;
    if (pthread_attr_init(&attr) != 0 ||
        pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED) != 0) {
        return 60;
    }
    if (pthread_create(&t, &attr, detached_worker, 0) != 0) {
        detached_go = 1;
        return 60;
    }
    int joined = pthread_join(t, 0);
    int detached = pthread_detach(t);
    detached_go = 1;
    if (joined != EINVAL) {
        return 61;
    }
    if (detached != EINVAL) {
        return 62;
    }
    int spins;
    for (spins = 0; spins < 100000 && !detached_done; spins++) {
        sched_yield();
    }
    if (!detached_done) {
        return 63;
    }
    return 0;
}

/* ---- 3. more threads than one chunk of the thread table -------------- */

#define MANY 70

static pthread_barrier_t many_barrier;

static void *many_worker(void *arg)
{
    /* Stay alive until main has looked at every thread. */
    pthread_barrier_wait(&many_barrier);
    return arg;
}

static int check_many(void)
{
    static pthread_t t[MANY];
    pthread_attr_t a;
    void *main_stack = 0;
    unsigned long size = 0;
    int i, err = 0;

    if (pthread_getattr_np(pthread_self(), &a) != 0 ||
        pthread_attr_getstack(&a, &main_stack, &size) != 0) {
        return 72;
    }
    if (pthread_barrier_init(&many_barrier, 0, MANY + 1) != 0) {
        return 70;
    }
    for (i = 0; i < MANY; i++) {
        if (pthread_create(&t[i], 0, many_worker, (void *)(long)(i + 1)) != 0) {
            /* The threads already made are parked on a barrier that can
             * no longer fill: there is no clean way out, so report it. */
            return 71;
        }
    }
    for (i = 0; i < MANY && !err; i++) {
        void *addr = 0;
        if (pthread_getattr_np(t[i], &a) != 0 ||
            pthread_attr_getstack(&a, &addr, &size) != 0) {
            err = 72;
        } else if (addr == main_stack) {
            err = 73;
        }
    }
    pthread_barrier_wait(&many_barrier);
    for (i = 0; i < MANY; i++) {
        void *rv = 0;
        if (pthread_join(t[i], &rv) != 0 || rv != (void *)(long)(i + 1)) {
            if (!err) {
                err = 74;
            }
        }
    }
    return err;
}

/* ---- 4. a stack the caller supplies ---------------------------------- */

#define USER_STACK (128UL * 1024)
static unsigned char user_stack[USER_STACK] __attribute__((aligned(16)));
static unsigned char *volatile user_stack_local;

static void *user_stack_worker(void *arg)
{
    unsigned char here = 0;
    (void)arg;
    user_stack_local = &here;
    return 0;
}

static int run_on_user_stack(void)
{
    pthread_attr_t attr;
    pthread_t t;
    user_stack_local = 0;
    if (pthread_attr_init(&attr) != 0 ||
        pthread_attr_setstack(&attr, user_stack, USER_STACK) != 0 ||
        pthread_create(&t, &attr, user_stack_worker, 0) != 0) {
        return 80;
    }
    if (pthread_join(t, 0) != 0) {
        return 81;
    }
    if (user_stack_local < user_stack ||
        user_stack_local >= user_stack + USER_STACK) {
        return 82;
    }
    /* The stack is still the caller's: joining must not have unmapped it. */
    user_stack[0] = 1;
    user_stack[USER_STACK - 1] = 1;
    return 0;
}

static int check_user_stack(void)
{
    int r = run_on_user_stack();
    if (r != 0) {
        return r;
    }
    /* Again, on the same memory: the first thread's reclaim left it alone. */
    return run_on_user_stack() == 0 ? 0 : 83;
}

/* ---- 5. a contended mutex --------------------------------------------- */

#define CONTENDERS 4
#define ROUNDS 20000

static pthread_mutex_t counter_lock;
static unsigned long counter;

static void *contender(void *arg)
{
    int i;
    (void)arg;
    for (i = 0; i < ROUNDS; i++) {
        pthread_mutex_lock(&counter_lock);
        unsigned long c = counter;
        if ((i & 63) == 0) {
            sched_yield(); /* hold the lock across a switch now and then */
        }
        counter = c + 1;
        pthread_mutex_unlock(&counter_lock);
    }
    return 0;
}

static int check_mutex(void)
{
    pthread_t t[CONTENDERS];
    int i;
    pthread_mutex_init(&counter_lock, 0);
    counter = 0;
    for (i = 0; i < CONTENDERS; i++) {
        if (pthread_create(&t[i], 0, contender, 0) != 0) {
            return 90;
        }
    }
    for (i = 0; i < CONTENDERS; i++) {
        pthread_join(t[i], 0);
    }
    return counter == (unsigned long)CONTENDERS * ROUNDS ? 0 : 91;
}

/* ---- 6. a condition variable hand-off --------------------------------- */

#define ITEMS 1000

static pthread_mutex_t box_lock;
static pthread_cond_t box_filled, box_emptied;
static int box_full;
static int box_value;

static void *producer(void *arg)
{
    int v;
    (void)arg;
    for (v = 1; v <= ITEMS; v++) {
        pthread_mutex_lock(&box_lock);
        while (box_full) {
            pthread_cond_wait(&box_emptied, &box_lock);
        }
        box_value = v;
        box_full = 1;
        pthread_cond_signal(&box_filled);
        pthread_mutex_unlock(&box_lock);
    }
    return 0;
}

static int check_cond(void)
{
    pthread_t t;
    int expect, err = 0;
    pthread_mutex_init(&box_lock, 0);
    pthread_cond_init(&box_filled, 0);
    pthread_cond_init(&box_emptied, 0);
    box_full = 0;
    if (pthread_create(&t, 0, producer, 0) != 0) {
        return 100;
    }
    for (expect = 1; expect <= ITEMS; expect++) {
        pthread_mutex_lock(&box_lock);
        while (!box_full) {
            pthread_cond_wait(&box_filled, &box_lock);
        }
        if (box_value != expect) {
            err = 101;
        }
        box_full = 0;
        pthread_cond_signal(&box_emptied);
        pthread_mutex_unlock(&box_lock);
    }
    pthread_join(t, 0);
    return err;
}

/* ---- 7. pthread_once from several threads at once --------------------- */

static pthread_once_t once = PTHREAD_ONCE_INIT;
static volatile int once_runs;
static pthread_barrier_t once_barrier;

static void once_routine(void)
{
    int i;
    once_runs++;
    /* Take long enough that the other callers arrive while this runs and
     * have to wait for it. */
    for (i = 0; i < 50; i++) {
        sched_yield();
    }
}

static void *once_caller(void *arg)
{
    (void)arg;
    pthread_barrier_wait(&once_barrier);
    pthread_once(&once, once_routine);
    /* Every caller returns only after the routine has finished. */
    return once_runs == 1 ? (void *)0 : (void *)1;
}

static int check_once(void)
{
    pthread_t t[4];
    int i, err = 0;
    if (pthread_barrier_init(&once_barrier, 0, 4) != 0) {
        return 110;
    }
    for (i = 0; i < 4; i++) {
        if (pthread_create(&t[i], 0, once_caller, 0) != 0) {
            return 110;
        }
    }
    for (i = 0; i < 4; i++) {
        void *rv = (void *)1;
        pthread_join(t[i], &rv);
        if (rv != (void *)0) {
            err = 111;
        }
    }
    return once_runs == 1 ? err : 111;
}

int main(void)
{
    static const struct {
        const char *name;
        int (*run)(void);
    } checks[] = {
        {"a thread gets the stack it asked for", check_big_stack},
        {"PTHREAD_CREATE_DETACHED", check_detached},
        {"more than 64 threads", check_many},
        {"a stack the caller supplies", check_user_stack},
        {"a contended mutex", check_mutex},
        {"a condition variable hand-off", check_cond},
        {"pthread_once from four threads", check_once},
    };
    unsigned i;
    for (i = 0; i < sizeof checks / sizeof checks[0]; i++) {
        int r = checks[i].run();
        if (r != 0) {
            printf("[pt] FAIL %s: %d\n", checks[i].name, r);
            return r;
        }
        printf("[pt] ok %s\n", checks[i].name);
    }
    return 42;
}
