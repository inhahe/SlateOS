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
 * Finally it checks the *libc's own* per-thread storage, which rides in the
 * same mapping just above the TCB (posix/src/perthread.rs): `errno` must be
 * a distinct lvalue per thread.  That is a separate mechanism from `__thread`
 * — the libc is built by rustc on stable, which has no `#[thread_local]`, so
 * it locates its block by hand from `%fs:0`.  A regression there would make
 * one thread's failed syscall visible as another thread's error, which is
 * exactly the class of bug POSIX's per-thread errno rule exists to prevent.
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

/* The glibc/musl errno ABI our libc implements: a function returning the
 * address of *this thread's* errno.  <errno.h> would give us musl's. */
extern int *__errno_location(void);
#define errno (*__errno_location())

/* .tdata: a non-zero initialiser, so a child that got a zeroed block (or no
 * block at all) is distinguishable from one that got a proper copy. */
__thread int tls_data = 0x1234;
/* .tbss: no initialiser, must read as zero in every thread. */
__thread int tls_bss;

/* Marks which value the *parent* stored, so the child can prove it is not
 * sharing the parent's block. */
#define PARENT_DATA 0xabcd
#define PARENT_BSS 7
#define PARENT_ERRNO 0x3f

/* What the child reports back to the parent.  The errno address is compared,
 * never dereferenced, after the join — by then the child's mapping is gone. */
struct probe {
    int ran;
    int *child_errno;
};

static void *worker(void *arg)
{
    struct probe *p = (struct probe *)arg;

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

    /* The libc's own per-thread block, above the TCB.  The parent set errno
     * to PARENT_ERRNO before creating us, so a shared block would show that
     * value here instead of the fresh mapping's zero. */
    p->child_errno = __errno_location();
    if (errno != 0) {
        return (void *)4; /* errno block shared with the parent, or stale */
    }
    errno = 0x21;
    if (errno != 0x21) {
        return (void *)5; /* errno writes don't stick */
    }

    p->ran = 1; /* prove the routine ran to completion */
    return (void *)0;
}

int main(void)
{
    pthread_t t;
    void *rv;
    struct probe p;
    int *parent_errno = __errno_location();

    /* Mutate the *parent's* copies first: if the child saw these values it
     * would mean both threads share one block. */
    tls_data = PARENT_DATA;
    tls_bss = PARENT_BSS;

    /* --- first thread ---------------------------------------------- */
    p.ran = 0;
    p.child_errno = 0;
    errno = PARENT_ERRNO;
    if (pthread_create(&t, 0, worker, &p) != 0) {
        return 10;
    }
    rv = (void *)-1;
    if (pthread_join(t, &rv) != 0) {
        return 11;
    }
    if (rv != (void *)0) {
        return 20 + (int)(long)rv; /* 21..25 — see worker() */
    }
    if (!p.ran) {
        return 12;
    }
    if (tls_data != PARENT_DATA || tls_bss != PARENT_BSS) {
        return 13; /* the child wrote through to the parent's block */
    }
    /* Address comparison only — the child's mapping is unmapped by now. */
    if (p.child_errno == parent_errno) {
        return 18; /* one errno lvalue shared by both threads */
    }

    /* --- second thread: the first one's mapping was reclaimed, so this
     * also proves the reclaim didn't corrupt anything and that a fresh
     * block is built each time. ------------------------------------- */
    p.ran = 0;
    p.child_errno = 0;
    errno = PARENT_ERRNO;
    if (pthread_create(&t, 0, worker, &p) != 0) {
        return 14;
    }
    rv = (void *)-1;
    if (pthread_join(t, &rv) != 0) {
        return 15;
    }
    if (rv != (void *)0) {
        return 30 + (int)(long)rv; /* 31..35 — see worker() */
    }
    if (!p.ran) {
        return 16;
    }
    if (tls_data != PARENT_DATA || tls_bss != PARENT_BSS) {
        return 17;
    }
    if (p.child_errno == parent_errno) {
        return 19;
    }

    /* The parent's own errno slot must still be intact and writable after
     * two threads have come and gone through the same address range. */
    if (__errno_location() != parent_errno) {
        return 40; /* the parent's block moved */
    }
    errno = 0x7e;
    if (errno != 0x7e) {
        return 41;
    }

    return 42;
}
