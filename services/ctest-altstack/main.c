/*
 * ctest-altstack — ring-3 regression test for `sigaltstack` and `SA_ONSTACK`.
 *
 * Guards `known-issues.md` →
 * B-NO-ALTERNATE-SIGNAL-STACK-SO-A-STACK-OVERFLOW-HANDLER-CANNOT-RUN, and the
 * claim `design-decisions.md` §1009 makes: that a handler registered with
 * `SA_ONSTACK` really does run on the registered stack.
 *
 * ## Why a fixture and not a unit test
 *
 * The posix crate's own tests run on the *host*, where switching to a stack
 * the test harness did not allocate is not something a test may do — so
 * `run_handler`'s host arm deliberately calls the handler directly and only
 * the bookkeeping either side of it is exercised. The assembly thunk that
 * does the actual work, `__call_on_alt_stack`, is `#[cfg(target_os = "none")]`
 * and has no host path at all.
 *
 * That leaves the central claim proved by construction rather than by
 * running, which is the failure shape this project keeps meeting: a link
 * verifies a symbol surface, not a behaviour. This fixture runs it.
 *
 * ## What it can and cannot see
 *
 * It can see that a handler's frames land inside the registered region, that
 * one *without* `SA_ONSTACK` does not, and that the reported state moves
 * through `SS_DISABLE` → 0 → `SS_ONSTACK` → 0 as POSIX says.
 *
 * It cannot see the case the feature exists for — a handler recovering from a
 * stack overflow — because the kernel builds the signal frame on the
 * interrupted stack before any of this code runs. That is the open half, in
 * `requests/b-a-honour-sa-onstack-when-building-the-signal-frame.md`, and a
 * fixture asserting it would fail today by design rather than by regression.
 *
 * ## It cannot hang
 *
 * Every signal here is raised with `raise()`, which dispatches synchronously
 * in-process. Nothing waits, nothing reads, nothing sleeps. The worst case is
 * a wrong exit code. That is deliberate: a sibling fixture once cost the
 * kernel lane two hours by blocking a boot test on a read that could not
 * return.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below).
 */

#include <errno.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/*
 * 64 KiB, far above MINSIGSTKSZ, and static so its address is known before
 * anything runs. Aligned to 16 because that is what the SysV ABI wants of a
 * stack pointer, and because an unaligned base would let the thunk's
 * `and rsp, -16` mask the pointer *below* ss_sp on a region that started
 * mid-word — which the alignment here makes impossible rather than merely
 * unlikely.
 */
#define ALT_SIZE 65536
static char alt_stack[ALT_SIZE] __attribute__((aligned(16)));

/* Written from a signal handler, read afterwards. */
static volatile unsigned long g_handler_sp;
static volatile int           g_handler_flags;
static volatile int           g_handler_ran;
static volatile int           g_change_ret;
static volatile int           g_change_errno;

/*
 * The address of a local in the handler stands in for the stack pointer. It
 * is below the frame the thunk set up, so "inside the region" is the whole
 * claim being tested; the exact offset is a compiler decision and not ours to
 * assert.
 */
static void on_alt(int sig)
{
    volatile char probe = 0;
    stack_t oss;
    stack_t other;

    (void)sig;
    (void)probe;
    g_handler_sp = (unsigned long)(uintptr_t)&probe;

    memset(&oss, 0, sizeof oss);
    if (sigaltstack(NULL, &oss) == 0) {
        g_handler_flags = oss.ss_flags;
    } else {
        g_handler_flags = -1;
    }

    /*
     * Changing the stack from a handler running on it must be EPERM: the
     * frames of this very function are on that memory.
     */
    other.ss_sp = alt_stack;      /* value irrelevant; the refusal is the point */
    other.ss_flags = 0;
    other.ss_size = ALT_SIZE;
    errno = 0;
    g_change_ret = sigaltstack(&other, NULL);
    g_change_errno = errno;

    g_handler_ran = 1;
}

/* The negative control: no SA_ONSTACK, so it must run where it was called. */
static void on_ordinary(int sig)
{
    volatile char probe = 0;

    (void)sig;
    (void)probe;
    g_handler_sp = (unsigned long)(uintptr_t)&probe;
    g_handler_ran = 1;
}

static int inside_alt(unsigned long sp)
{
    unsigned long base = (unsigned long)(uintptr_t)alt_stack;
    return sp >= base && sp < base + ALT_SIZE;
}

static int install(int sig, void (*fn)(int), unsigned long extra_flags)
{
    struct sigaction sa;

    memset(&sa, 0, sizeof sa);
    sa.sa_handler = fn;
    sa.sa_flags = (int)extra_flags;
    sigemptyset(&sa.sa_mask);
    return sigaction(sig, &sa, NULL);
}

int main(void)
{
    stack_t ss;
    stack_t oss;

    /* 1. Nothing registered yet: SS_DISABLE, and the other two fields zero. */
    memset(&oss, 0xAB, sizeof oss);
    if (sigaltstack(NULL, &oss) != 0)
        return 1;
    if (oss.ss_flags != SS_DISABLE)
        return 2;
    if (oss.ss_sp != NULL || oss.ss_size != 0)
        return 3;

    /* 2. Register it. */
    ss.ss_sp = alt_stack;
    ss.ss_flags = 0;
    ss.ss_size = ALT_SIZE;
    if (sigaltstack(&ss, NULL) != 0)
        return 4;

    /*
     * 3. Registered but not in use reports 0 — *not* SS_DISABLE. This is the
     * row the old implementation got wrong invisibly: it answered SS_DISABLE
     * unconditionally, so a caller that registered a stack and then asked was
     * told it had none.
     */
    memset(&oss, 0, sizeof oss);
    if (sigaltstack(NULL, &oss) != 0)
        return 5;
    if (oss.ss_flags != 0)
        return 6;
    if (oss.ss_sp != alt_stack || oss.ss_size != ALT_SIZE)
        return 7;

    /* 4. A stack below MINSIGSTKSZ is refused, and the refusal changes nothing. */
    {
        stack_t tiny;
        tiny.ss_sp = alt_stack;
        tiny.ss_flags = 0;
        tiny.ss_size = 8;
        errno = 0;
        if (sigaltstack(&tiny, NULL) != -1)
            return 8;
        if (errno != ENOMEM)
            return 9;
        memset(&oss, 0, sizeof oss);
        if (sigaltstack(NULL, &oss) != 0 || oss.ss_size != ALT_SIZE)
            return 10;
    }

    /* 5. The handler runs on it. */
    if (install(SIGUSR1, on_alt, SA_ONSTACK) != 0)
        return 11;
    g_handler_ran = 0;
    g_handler_sp = 0;
    if (raise(SIGUSR1) != 0)
        return 12;
    if (!g_handler_ran)
        return 13;
    if (!inside_alt(g_handler_sp))
        return 14;

    /* 6. …and knew it, reporting SS_ONSTACK from inside itself. */
    if (g_handler_flags != SS_ONSTACK)
        return 15;

    /* 7. …and could not move the ground out from under itself. */
    if (g_change_ret != -1)
        return 16;
    if (g_change_errno != EPERM)
        return 17;

    /* 8. Once it returns, the stack is no longer in use. */
    memset(&oss, 0, sizeof oss);
    if (sigaltstack(NULL, &oss) != 0)
        return 18;
    if (oss.ss_flags != 0)
        return 19;
    if (oss.ss_sp != alt_stack || oss.ss_size != ALT_SIZE)
        return 20;

    /*
     * 9. The negative control, and the reason this suite is worth running: a
     * handler *without* SA_ONSTACK must run where it was called from. Without
     * this check every assertion above still passes against an implementation
     * that switches unconditionally, which would be a much worse bug than the
     * one being fixed — it would relocate every handler in the process onto
     * one 64 KiB buffer.
     */
    if (install(SIGUSR2, on_ordinary, 0) != 0)
        return 21;
    g_handler_ran = 0;
    g_handler_sp = 0;
    if (raise(SIGUSR2) != 0)
        return 22;
    if (!g_handler_ran)
        return 23;
    if (inside_alt(g_handler_sp))
        return 24;

    /*
     * 10. Disabling reads neither ss_sp nor ss_size, so the idiomatic
     * `stack_t{.ss_flags = SS_DISABLE}` with everything else zero works.
     */
    ss.ss_sp = NULL;
    ss.ss_flags = SS_DISABLE;
    ss.ss_size = 0;
    if (sigaltstack(&ss, NULL) != 0)
        return 25;
    memset(&oss, 0xAB, sizeof oss);
    if (sigaltstack(NULL, &oss) != 0)
        return 26;
    if (oss.ss_flags != SS_DISABLE || oss.ss_sp != NULL || oss.ss_size != 0)
        return 27;

    /* 11. With none registered, SA_ONSTACK has nothing to switch to. */
    if (install(SIGUSR1, on_ordinary, SA_ONSTACK) != 0)
        return 28;
    g_handler_ran = 0;
    g_handler_sp = 0;
    if (raise(SIGUSR1) != 0)
        return 29;
    if (!g_handler_ran)
        return 30;
    if (inside_alt(g_handler_sp))
        return 31;

    /*
     * 12. The struct ABI itself, which is what checks 1-11 quietly depend on.
     *
     * `struct sigaction` has two different layouts on x86_64 at the same size:
     * the kernel's (handler, flags, restorer, mask) and the C library's
     * (handler, mask, flags, restorer). Our libc used the kernel's under a
     * comment saying it was glibc's until 2026-09-09. `sa_handler` is at
     * offset 0 in both, which is why handlers worked and nothing noticed --
     * and why no Rust test could see it, since Rust builds the struct by field
     * name and agrees with itself whichever order it picks.
     *
     * A flag that does not survive a round trip is that bug and nothing else.
     */
    {
        struct sigaction set;
        struct sigaction got;

        memset(&set, 0, sizeof set);
        memset(&got, 0, sizeof got);
        set.sa_handler = on_ordinary;
        set.sa_flags = SA_NODEFER;
        sigemptyset(&set.sa_mask);
        sigaddset(&set.sa_mask, SIGUSR2);
        if (sigaction(SIGUSR1, &set, NULL) != 0)
            return 32;
        if (sigaction(SIGUSR1, NULL, &got) != 0)
            return 33;
        if (got.sa_handler != on_ordinary)
            return 34;
        if ((got.sa_flags & SA_NODEFER) == 0)
            return 35;
        if (!sigismember(&got.sa_mask, SIGUSR2))
            return 36;
        /* …and nothing it was not given. */
        if (sigismember(&got.sa_mask, SIGALRM))
            return 37;
    }

    return 42;
}
