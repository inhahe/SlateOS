/*
 * ctest-tls-thread — native C ring-3 fixture for *child-thread* ELF TLS.
 *
 * Compiled by `zig cc --target=x86_64-linux-musl` and linked against the
 * posix sysroot `libc.a` (see build.py).  The kernel runs it as a ring-3
 * self-test (`self_test_ctls_thread` in kernel/src/proc/spawn.rs) and
 * asserts the exit code.
 *
 * What it proves, and why it needs to be C rather than Rust: a C compiler
 * lowers `__thread` to a `%fs`-relative load and (with -fstack-protector-*)
 * reads the canary from `%fs:0x28` in *every* function prologue.  Both
 * therefore fault instantly on a thread whose `fs_base` is 0 — which is the
 * state the kernel hands every new task.  Only the main thread used to get
 * a thread pointer; a `pthread_create`d thread got none, so the very first
 * thing a threaded C program did in its child was crash.  This fixture
 * fails loudly (a fault, hence no clean exit) if that regresses.
 *
 * It also checks the *contents* of the per-thread block, not just that it
 * is mapped:
 *   - the child's `.tdata` copy holds the initialiser image (not zeros, and
 *     not the parent's mutated value),
 *   - the child's `.tbss` copy is zero,
 *   - writes in the child do not disturb the parent's copies (separate
 *     blocks, correct variant-II offsets),
 *   - a second thread created after the first was joined gets a fresh
 *     block too (the mapping was really reclaimed and re-established).
 *
 * Exit codes: 42 = success; anything else identifies the failing step (see
 * the returns below), which the kernel self-test prints verbatim.
 */

/* Declared locally rather than via <pthread.h>: the musl headers zig ships
 * describe musl's ABI, while the symbols actually come from our posix
 * `libc.a`.  These three declarations are all we need and they match the
 * posix crate's signatures exactly (pthread_t is a 64-bit kernel task id). */
typedef unsigned long pthread_t;
extern int pthread_create(pthread_t *thread, void *attr,
                          void *(*start)(void *), void *arg);
extern int pthread_join(pthread_t thread, void **retval);

/* .tdata: a non-zero initialiser, so a child that got a zeroed block (or no
 * block at all) is distinguishable from one that got a proper copy. */
__thread int tls_data = 0x1234;
/* .tbss: no initialiser, must read as zero in every thread. */
__thread int tls_bss;

/* Marks which value the *parent* stored, so the child can prove it is not
 * sharing the parent's block. */
#define PARENT_DATA 0xabcd
#define PARENT_BSS 7

static void *worker(void *arg)
{
    /* Reaching here at all means %fs is installed: this function's prologue
     * already read the stack-protector canary from %fs:0x28. */
    if (tls_data != 0x1234) {
        return (void *)1; /* .tdata init image missing or shared with parent */
    }
    if (tls_bss != 0) {
        return (void *)2; /* .tbss not zero-filled (or shared) */
    }

    tls_data = 0x5678;
    tls_bss = 99;
    if (tls_data != 0x5678 || tls_bss != 99) {
        return (void *)3; /* TLS writes don't stick */
    }

    *(int *)arg = 1; /* prove the routine ran to completion */
    return (void *)0;
}

int main(void)
{
    pthread_t t;
    void *rv;
    int ran;

    /* Mutate the *parent's* copies first: if the child saw these values it
     * would mean both threads share one block. */
    tls_data = PARENT_DATA;
    tls_bss = PARENT_BSS;

    /* --- first thread ---------------------------------------------- */
    ran = 0;
    if (pthread_create(&t, 0, worker, &ran) != 0) {
        return 10;
    }
    rv = (void *)-1;
    if (pthread_join(t, &rv) != 0) {
        return 11;
    }
    if (rv != (void *)0) {
        return 20 + (int)(long)rv; /* 21/22/23 — see worker() */
    }
    if (!ran) {
        return 12;
    }
    if (tls_data != PARENT_DATA || tls_bss != PARENT_BSS) {
        return 13; /* the child wrote through to the parent's block */
    }

    /* --- second thread: the first one's mapping was reclaimed, so this
     * also proves the reclaim didn't corrupt anything and that a fresh
     * block is built each time. ------------------------------------- */
    ran = 0;
    if (pthread_create(&t, 0, worker, &ran) != 0) {
        return 14;
    }
    rv = (void *)-1;
    if (pthread_join(t, &rv) != 0) {
        return 15;
    }
    if (rv != (void *)0) {
        return 30 + (int)(long)rv; /* 31/32/33 — see worker() */
    }
    if (!ran) {
        return 16;
    }
    if (tls_data != PARENT_DATA || tls_bss != PARENT_BSS) {
        return 17;
    }

    return 42;
}
