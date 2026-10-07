/*
 * ctest-affinity -- does pinning a thread to CPUs hold on SlateOS, and does
 * a thread learn the CPU it is on?
 *
 * "Affinity" is the set of CPUs a thread may run on. sched_setaffinity is
 * how a program pins itself or a thread -- a latency-sensitive loop to one
 * core, a benchmark to a known one -- and sched_getcpu is how an allocator
 * picks the per-CPU arena of the CPU it is on. On SlateOS a narrower mask
 * was refused (ENOSYS) and sched_getcpu answered CPU 0 everywhere, because
 * the native ABI had no calls for either. Lane A's SYS_SCHED_SET_AFFINITY,
 * SYS_SCHED_GET_AFFINITY and SYS_CPU_CURRENT are those calls, and the
 * library asks them now. Ids are Linux's: a process's id is its first
 * thread's, and 0 is the calling thread.
 *
 * The checks, in order:
 *
 *    1x  the calling thread's mask: it holds at least one CPU, and pinned to
 *        one CPU it reads back as that CPU alone, and runs there
 *    2x  each online CPU in turn: pinned there, sched_getcpu and getcpu say
 *        so; then the whole mask again
 *    3x  a thread started while pinned inherits the mask, and a thread pinned
 *        by pthread_setaffinity_np runs on the CPU it was given
 *    4x  the refusals: a mask with no CPU the machine has (EINVAL), no such
 *        process (ESRCH), a process that is not ours (EPERM -- pid 1, asked
 *        to keep the mask it has)
 *
 * Exit codes: 42 is every check passed.
 *    2  pthread_create or pthread_join failed     Otherwise <check><step>;
 *    no step is 42.
 *
 * The same source compiles and passes on Linux, as an ordinary user, which
 * is how its expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <string.h>
#include <unistd.h>

/* A pid nothing has: above Linux's largest pid_max, 4194304. */
#define NO_SUCH_PID 999999999

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

static int fail(int code, const char *what)
{
    int e = errno;
    emit("ctest-affinity: FAIL ");
    emit_num(code);
    emit(": ");
    emit(what);
    emit(" (errno ");
    emit_num(e);
    emit(")\n");
    return code;
}

static void ok(const char *what)
{
    emit("ctest-affinity: ok ");
    emit(what);
    emit("\n");
}

/* The mask of the calling thread when the checks began: every CPU it may
 * use, which they pin within and put back at the end. */
static cpu_set_t everywhere;

/* The lowest CPU in `set` at or after `from`, or -1. */
static int next_cpu(const cpu_set_t *set, int from)
{
    for (int cpu = from; cpu < CPU_SETSIZE; cpu++) {
        if (CPU_ISSET(cpu, set)) {
            return cpu;
        }
    }
    return -1;
}

/* Let the scheduler act on a new mask: a pinned thread is moved at once, but
 * a few yields cost nothing and cover a kernel that moves it at its next
 * scheduling point. */
static void settle(void)
{
    for (int i = 0; i < 4; i++) {
        sched_yield();
    }
}

static int pin_to(int cpu)
{
    cpu_set_t one;
    CPU_ZERO(&one);
    CPU_SET(cpu, &one);
    return sched_setaffinity(0, sizeof one, &one);
}

struct report {
    cpu_set_t mask;
    int cpu;
    int got_mask;
};

/* A thread's report: its own mask, and the CPU it is on. */
static void *report_self(void *arg)
{
    struct report *r = arg;
    settle();
    r->got_mask = sched_getaffinity(0, sizeof r->mask, &r->mask) == 0;
    r->cpu = sched_getcpu();
    return NULL;
}

struct gated {
    int fds[2];
    struct report r;
};

/* report_self, once a byte arrives on the pipe: what it reports is then
 * what the thread was given while it waited. */
static void *gated_report(void *arg)
{
    struct gated *g = arg;
    char c;
    if (read(g->fds[0], &c, 1) != 1) {
        return NULL;
    }
    return report_self(&g->r);
}

int main(void)
{
    /* 1. */
    if (sched_getaffinity(0, sizeof everywhere, &everywhere) != 0) {
        return fail(11, "sched_getaffinity(0)");
    }
    int count = CPU_COUNT(&everywhere);
    int first = next_cpu(&everywhere, 0);
    if (count < 1 || first < 0) {
        return fail(12, "the calling thread may run on at least one CPU");
    }
    if (pin_to(first) != 0) {
        return fail(13, "sched_setaffinity(0) to one CPU");
    }
    cpu_set_t now;
    CPU_ZERO(&now);
    if (sched_getaffinity(0, sizeof now, &now) != 0 || CPU_COUNT(&now) != 1
        || !CPU_ISSET(first, &now)) {
        return fail(14, "pinned to one CPU, the mask reads back as that CPU alone");
    }
    settle();
    if (sched_getcpu() != first) {
        emit("ctest-affinity: pinned to CPU ");
        emit_num(first);
        emit(", sched_getcpu says ");
        emit_num(sched_getcpu());
        emit("\n");
        return fail(15, "a thread pinned to a CPU runs there");
    }
    ok("a thread pinned to one CPU reads back that CPU, and runs there");

    /* 2. */
    for (int cpu = first; cpu >= 0; cpu = next_cpu(&everywhere, cpu + 1)) {
        if (pin_to(cpu) != 0) {
            return fail(21, "sched_setaffinity(0) to each CPU in turn");
        }
        settle();
        unsigned got_cpu = 9999, got_node = 9999;
        if (sched_getcpu() != cpu || getcpu(&got_cpu, &got_node) != 0
            || got_cpu != (unsigned)cpu) {
            emit("ctest-affinity: pinned to CPU ");
            emit_num(cpu);
            emit(", sched_getcpu says ");
            emit_num(sched_getcpu());
            emit("\n");
            return fail(22, "sched_getcpu and getcpu say the CPU it was pinned to");
        }
    }
    if (sched_setaffinity(0, sizeof everywhere, &everywhere) != 0) {
        return fail(23, "the whole mask again");
    }
    ok("pinned to each CPU in turn, sched_getcpu and getcpu say so");

    /* 3. */
    int last = first;
    for (int cpu = first; cpu >= 0; cpu = next_cpu(&everywhere, cpu + 1)) {
        last = cpu;
    }
    if (pin_to(last) != 0) {
        return fail(31, "sched_setaffinity(0) to the last CPU");
    }
    struct report inherited;
    memset(&inherited, 0, sizeof inherited);
    pthread_t t;
    if (pthread_create(&t, NULL, report_self, &inherited) != 0 || pthread_join(t, NULL) != 0) {
        return fail(2, "pthread_create or pthread_join");
    }
    if (!inherited.got_mask || CPU_COUNT(&inherited.mask) != 1 || !CPU_ISSET(last, &inherited.mask)
        || inherited.cpu != last) {
        return fail(32, "a thread started while pinned inherits the mask, and runs there");
    }
    if (sched_setaffinity(0, sizeof everywhere, &everywhere) != 0) {
        return fail(33, "the whole mask again");
    }
    /* A thread pinned by another: started with the whole mask, it waits on
     * a pipe until pthread_setaffinity_np has pinned it, then looks. */
    struct gated gate;
    memset(&gate, 0, sizeof gate);
    if (pipe(gate.fds) != 0) {
        return fail(2, "pipe");
    }
    if (pthread_create(&t, NULL, gated_report, &gate) != 0) {
        return fail(2, "pthread_create");
    }
    cpu_set_t one;
    CPU_ZERO(&one);
    CPU_SET(first, &one);
    int pinned = pthread_setaffinity_np(t, sizeof one, &one);
    cpu_set_t seen;
    CPU_ZERO(&seen);
    int read_back = pthread_getaffinity_np(t, sizeof seen, &seen);
    char go = 'g';
    ssize_t sent = write(gate.fds[1], &go, 1);
    if (pthread_join(t, NULL) != 0 || sent != 1) {
        return fail(2, "pthread_join");
    }
    close(gate.fds[0]);
    close(gate.fds[1]);
    if (pinned != 0) {
        errno = pinned;
        return fail(34, "pthread_setaffinity_np to one CPU");
    }
    if (read_back != 0 || CPU_COUNT(&seen) != 1 || !CPU_ISSET(first, &seen)) {
        errno = read_back;
        return fail(35, "pthread_getaffinity_np reads the CPU it was pinned to");
    }
    if (!gate.r.got_mask || CPU_COUNT(&gate.r.mask) != 1 || !CPU_ISSET(first, &gate.r.mask)
        || gate.r.cpu != first) {
        return fail(36, "a thread pinned by pthread_setaffinity_np runs on its CPU");
    }
    ok("a thread started while pinned inherits the mask, and one pinned by another runs there");

    /* 4. */
    cpu_set_t none_here;
    CPU_ZERO(&none_here);
    CPU_SET(CPU_SETSIZE - 1, &none_here);
    errno = 0;
    if (sched_setaffinity(0, sizeof none_here, &none_here) != -1 || errno != EINVAL) {
        return fail(41, "a mask with no CPU the machine has is EINVAL");
    }
    errno = 0;
    if (sched_setaffinity(NO_SUCH_PID, sizeof everywhere, &everywhere) != -1 || errno != ESRCH) {
        return fail(43, "sched_setaffinity of no such process is ESRCH");
    }
    errno = 0;
    if (sched_getaffinity(NO_SUCH_PID, sizeof now, &now) != -1 || errno != ESRCH) {
        return fail(44, "sched_getaffinity of no such process is ESRCH");
    }
    cpu_set_t init_mask;
    if (sched_getaffinity(1, sizeof init_mask, &init_mask) != 0) {
        return fail(45, "reading pid 1's mask needs no right");
    }
    errno = 0;
    if (sched_setaffinity(1, sizeof init_mask, &init_mask) != -1 || errno != EPERM) {
        return fail(46, "pid 1 is not ours to pin: EPERM");
    }
    ok("no CPU, no such process and a stranger are refused");

    emit("ctest-affinity: every check passed\n");
    return 42;
}
