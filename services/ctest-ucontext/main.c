/*
 * ctest-ucontext — ring-3 test of <ucontext.h>: getcontext, setcontext,
 * makecontext and swapcontext (posix/src/ucontext.rs).
 *
 * ## Why a fixture and not only the unit tests
 *
 * The posix crate's host tests switch contexts only with swapcontext, which
 * to its caller is an ordinary call that returns once: Rust cannot declare a
 * function that returns twice, as getcontext does when a setcontext resumes
 * it, so a Rust test that relied on that would be unsound. C compilers know
 * getcontext by name and treat it as they treat setjmp. This fixture is the
 * returning-twice half, and the only caller built by another toolchain --
 * the one that checks the assembly against the real C calling convention on
 * the real target.
 *
 * Every expectation below was checked against glibc 2.39 by compiling this
 * file with gcc there: it exits 42 on glibc as it must here.
 *
 * ## It cannot hang
 *
 * Every switch is to a context this program made, on stacks it owns; no
 * check waits on anything. The worst case is a wrong exit code.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below).
 */

#define _GNU_SOURCE

#include <fenv.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ucontext.h>
#include <unistd.h>

#define STACK_SIZE 65536

static char stack_a[STACK_SIZE] __attribute__((aligned(16)));
static char stack_b[STACK_SIZE] __attribute__((aligned(16)));

static ucontext_t main_ctx, ctx_a, ctx_b;
static volatile long seen[8];
static volatile int yields;
static volatile int resumed_ok;
static volatile int aligned_ok;
static volatile int all_passed;

/* 20-23: eight arguments, six in registers and two on the stack. */
static void eight(int a, int b, int c, int d, int e, int f, int g, int h)
{
    long v[8] = {a, b, c, d, e, f, g, h};
    for (int i = 0; i < 8; i++) {
        seen[i] = v[i];
    }
    /* A double through snprintf: SSE code that faults on a misaligned stack,
     * so the context's stack must be aligned as a call's is. */
    char buf[32];
    snprintf(buf, sizeof buf, "%.3f", 2.5 * (double)a);
    aligned_ok = strcmp(buf, "27.500") == 0;
}

/* 30-33: a generator, yielding 1..5 through swapcontext. */
static volatile int value;
static void generator(void)
{
    for (int i = 1; i <= 5; i++) {
        value = i;
        yields++;
        if (swapcontext(&ctx_b, &main_ctx) == 0) {
            resumed_ok++;
        }
    }
    value = -1;
}

/* 60: a context with no uc_link ends the process, with exit(0) as glibc's
 * does -- which the atexit handler turns into 42 when everything before it
 * passed. */
static void last(void)
{
}

static void on_exit_handler(void)
{
    if (all_passed) {
        _exit(42);
    }
}

int main(void)
{
    /* ------------------------------------------------------------------
     * 10-12: getcontext returns twice -- once now, once for each setcontext.
     * ------------------------------------------------------------------ */
    {
        volatile int count = 0;
        ucontext_t uc;
        if (getcontext(&uc) != 0) {
            return 10;
        }
        count++;
        if (count < 3) {
            setcontext(&uc);
            return 11; /* setcontext returned: it failed */
        }
        if (count != 3) {
            return 12;
        }
    }

    /* ------------------------------------------------------------------
     * 20-23: makecontext with eight arguments, resuming uc_link on return.
     * ------------------------------------------------------------------ */
    if (getcontext(&ctx_a) != 0) {
        return 20;
    }
    ctx_a.uc_stack.ss_sp = stack_a;
    ctx_a.uc_stack.ss_size = sizeof stack_a;
    ctx_a.uc_link = &main_ctx;
    makecontext(&ctx_a, (void (*)(void))eight, 8, 11, 22, 33, 44, 55, 66, 77, -88);
    if (swapcontext(&main_ctx, &ctx_a) != 0) {
        return 21;
    }
    {
        static const long want[8] = {11, 22, 33, 44, 55, 66, 77, -88};
        for (int i = 0; i < 8; i++) {
            if (seen[i] != want[i]) {
                return 22;
            }
        }
    }
    if (!aligned_ok) {
        return 23;
    }

    /* ------------------------------------------------------------------
     * 30-33: two contexts handing control back and forth.
     * ------------------------------------------------------------------ */
    if (getcontext(&ctx_b) != 0) {
        return 30;
    }
    ctx_b.uc_stack.ss_sp = stack_b;
    ctx_b.uc_stack.ss_size = sizeof stack_b;
    ctx_b.uc_link = &main_ctx;
    makecontext(&ctx_b, generator, 0);
    {
        int sum = 0;
        for (;;) {
            if (swapcontext(&main_ctx, &ctx_b) != 0) {
                return 31;
            }
            if (value < 0) {
                break;
            }
            sum += value;
        }
        if (sum != 15 || yields != 5) {
            return 32;
        }
        if (resumed_ok != 5) {
            return 33;
        }
    }

    /* ------------------------------------------------------------------
     * 40-42: the signal mask travels with the context.
     * ------------------------------------------------------------------ */
    {
        sigset_t usr1, now;
        sigemptyset(&usr1);
        sigaddset(&usr1, SIGUSR1);
        if (sigprocmask(SIG_BLOCK, &usr1, NULL) != 0) {
            return 40;
        }
        volatile int resumed = 0;
        ucontext_t uc;
        getcontext(&uc);
        if (!resumed) {
            resumed = 1;
            sigprocmask(SIG_UNBLOCK, &usr1, NULL);
            setcontext(&uc);
            return 41;
        }
        sigprocmask(SIG_BLOCK, NULL, &now);
        if (!sigismember(&now, SIGUSR1)) {
            return 42 + 100; /* never 42: keep the success code unambiguous */
        }
        sigprocmask(SIG_UNBLOCK, &usr1, NULL);
    }

    /* ------------------------------------------------------------------
     * 50-51: so does the rounding direction.
     * ------------------------------------------------------------------ */
    {
        if (fesetround(FE_UPWARD) != 0) {
            return 50;
        }
        volatile int resumed = 0;
        ucontext_t uc;
        getcontext(&uc);
        if (!resumed) {
            resumed = 1;
            fesetround(FE_TONEAREST);
            setcontext(&uc);
        }
        if (fegetround() != FE_UPWARD) {
            return 51;
        }
        fesetround(FE_TONEAREST);
    }

    /* ------------------------------------------------------------------
     * 60: a context with no uc_link: returning from it exits the process
     * with 0, which on_exit_handler turns into 42.
     * ------------------------------------------------------------------ */
    if (atexit(on_exit_handler) != 0) {
        return 60;
    }
    all_passed = 1;
    if (getcontext(&ctx_a) != 0) {
        return 61;
    }
    ctx_a.uc_stack.ss_sp = stack_a;
    ctx_a.uc_stack.ss_size = sizeof stack_a;
    ctx_a.uc_link = NULL;
    makecontext(&ctx_a, last, 0);
    setcontext(&ctx_a);
    return 62;
}
