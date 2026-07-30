/*
 * ctest-fortify — ring-3 regression test for the sysroot's `_FORTIFY_SOURCE`
 * printf family (`__printf_chk` and friends).
 *
 * These six entry points are pure assembly trampolines (`va_trampoline!` in
 * `posix/src/fortify_printf.rs`): each spills the argument registers into a
 * System V register save area, builds a `va_list` over it, and calls the
 * matching `__v*_chk`.  Nothing about that is observable from a host unit
 * test — `cargo test` can call the `__v*_chk` Rust functions, but the
 * trampolines only exist on the bare-metal target and only a caller that
 * believes the real C ABI can prove they hand over the right `va_list`.
 *
 * Getting them wrong is quiet.  Each trampoline hard-codes an initial
 * `gp_offset` (8 x the number of *named integer* arguments) and the register
 * the `va_list*` travels in, and the two must agree with the C signature.
 * An off-by-one there does not crash: it reads the varargs one slot early or
 * late, so `__snprintf_chk(buf, n, flag, slen, "%d", 7)` prints `slen`, or a
 * plausible-looking garbage integer, and the program carries on.
 *
 * We cannot get a C compiler to *generate* these calls for us: musl (which
 * zig cc uses for headers) deliberately does not implement _FORTIFY_SOURCE,
 * so `-D_FORTIFY_SOURCE=2` rewrites nothing.  Declaring the glibc prototypes
 * by hand and calling them directly is therefore both the only option and the
 * more precise test — it pins the exact ABI a fortified glibc object file
 * expects, which is what we have to match to link one.
 *
 * `flag` is glibc's fortify level; the sysroot accepts and ignores it.
 * `slen` is `__builtin_object_size` of the destination.  glibc aborts on
 * overflow; we truncate (documented deviation), so the checks below assert
 * truncation, not death.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below, and the legend in
 * kernel/src/proc/spawn.rs::self_test_cfortify).
 */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * The glibc fortified prototypes.  Written out rather than included because
 * musl has no `_FORTIFY_SOURCE` support to include them from.
 */
extern int __printf_chk(int flag, const char *fmt, ...);
extern int __fprintf_chk(FILE *fp, int flag, const char *fmt, ...);
extern int __dprintf_chk(int fd, int flag, const char *fmt, ...);
extern int __asprintf_chk(char **strp, int flag, const char *fmt, ...);
extern int __sprintf_chk(char *s, int flag, size_t slen, const char *fmt, ...);
extern int __snprintf_chk(char *s, size_t maxlen, int flag, size_t slen, const char *fmt, ...);

/* Launder through a volatile global so nothing is constant folded away. */
static volatile int         g_launder_i;
static volatile double      g_launder_d;
static volatile long double g_launder_ld;

static int         opaque_i(int x)          { g_launder_i = x;  return g_launder_i;  }
static double      opaque_d(double x)       { g_launder_d = x;  return g_launder_d;  }
static long double opaque_ld(long double x) { g_launder_ld = x; return g_launder_ld; }

/* `__builtin_object_size` semantics: an unknown destination size is -1. */
#define SLEN_UNKNOWN ((size_t)-1)

extern int __vsnprintf_chk(char *s, size_t maxlen, int flag, size_t slen,
                           const char *fmt, va_list ap);

/*
 * A variadic of our own, so check 50 can hand `__vsnprintf_chk` a genuine
 * compiler-built `va_list` rather than one of the trampolines'.  A pass here
 * alongside a failure above localises the fault to the assembly.
 */
static int via_valist(char *dst, size_t n, const char *fmt, ...)
{
    va_list ap;
    int r;

    va_start(ap, fmt);
    r = __vsnprintf_chk(dst, n, 1, n, fmt, ap);
    va_end(ap);
    return r;
}

int main(void)
{
    char buf[128];

    /* ------------------------------------------------------------------
     * 10-16: __snprintf_chk — five named arguments, so the varargs start at
     * gp_offset 40 and the va_list* arrives in %r9.  It has the most named
     * arguments of the six, hence the least register slack: if the
     * trampoline is wrong anywhere, it is wrong here.
     * ------------------------------------------------------------------ */
    memset(buf, 0, sizeof buf);
    if (__snprintf_chk(buf, sizeof buf, 1, sizeof buf, "n=%d", opaque_i(42)) != 4) {
        return 10;
    }
    if (strcmp(buf, "n=42") != 0) {
        return 11;
    }

    /* Several integers in a row: the first five varargs come from the
     * register save area, the sixth onward from the overflow area, so this
     * proves the trampoline's `overflow_arg_area` points at the caller's
     * frame and not at its own scratch space. */
    memset(buf, 0, sizeof buf);
    __snprintf_chk(buf, sizeof buf, 1, sizeof buf, "%d%d%d%d%d%d%d%d",
                   opaque_i(1), opaque_i(2), opaque_i(3), opaque_i(4),
                   opaque_i(5), opaque_i(6), opaque_i(7), opaque_i(8));
    if (strcmp(buf, "12345678") != 0) {
        return 12;
    }

    /* `slen` smaller than `maxlen` must win: the wrapper's whole purpose. */
    memset(buf, 0, sizeof buf);
    if (__snprintf_chk(buf, sizeof buf, 1, 4, "%d", opaque_i(123456)) != 6) {
        return 13; /* return value is the would-be length, not the truncation */
    }
    if (strcmp(buf, "123") != 0) { /* bound 4 => 3 chars + NUL */
        return 14;
    }

    /* And `maxlen` smaller than `slen` must win the other way. */
    memset(buf, 0, sizeof buf);
    __snprintf_chk(buf, 3, 1, sizeof buf, "%d", opaque_i(98765));
    if (strcmp(buf, "98") != 0) {
        return 15;
    }

    /* An unknown object size leaves it effectively unfortified. */
    memset(buf, 0, sizeof buf);
    __snprintf_chk(buf, sizeof buf, 1, SLEN_UNKNOWN, "%s-%d", "ok", opaque_i(9));
    if (strcmp(buf, "ok-9") != 0) {
        return 16;
    }

    /* ------------------------------------------------------------------
     * 20-22: __sprintf_chk — four named arguments (gp_offset 32, %r8).
     * ------------------------------------------------------------------ */
    memset(buf, 0, sizeof buf);
    if (__sprintf_chk(buf, 1, sizeof buf, "%s=%d", "k", opaque_i(7)) != 3) {
        return 20;
    }
    if (strcmp(buf, "k=7") != 0) {
        return 21;
    }

    /* Unlike the plain `sprintf`, this one is bounded by `slen`. */
    memset(buf, 0, sizeof buf);
    __sprintf_chk(buf, 1, 4, "%d", opaque_i(555555));
    if (strcmp(buf, "555") != 0) {
        return 22;
    }

    /* ------------------------------------------------------------------
     * 30-32: __asprintf_chk — three named arguments (gp_offset 24, %rcx),
     * and the only one that allocates.
     * ------------------------------------------------------------------ */
    {
        char *p = NULL;
        int n = __asprintf_chk(&p, 1, "a%db%s", opaque_i(3), "c");
        if (n != 4) {
            return 30;
        }
        if (p == NULL) {
            return 31;
        }
        if (strcmp(p, "a3bc") != 0) {
            free(p);
            return 32;
        }
        free(p);
    }

    /* ------------------------------------------------------------------
     * 33-35: the stdout-writing three.  Their output goes to the serial
     * console, so the assertion is the byte count they report — which is
     * still produced by the full formatting pass over the va_list, and so
     * still fails loudly if the trampoline hands over the wrong one.
     * ------------------------------------------------------------------ */
    if (__printf_chk(1, "[fortify] printf %d %s\n", opaque_i(11), "x") != 22) {
        return 33;
    }
    if (__fprintf_chk(stdout, 1, "[fortify] fprintf %d\n", opaque_i(222)) != 22) {
        return 34;
    }
    if (__dprintf_chk(1, 1, "[fortify] dprintf %d\n", opaque_i(3333)) != 23) {
        return 35;
    }

    /* ------------------------------------------------------------------
     * 40-46: floats and `long double` through the fortified path.
     *
     * A `double` proves the trampoline spilled %xmm0-7 and set fp_offset;
     * a `long double` proves it published a correct `overflow_arg_area`,
     * because X87/X87UP is MEMORY-class and lives *only* there.  Guards
     * BUG-POSIX-LONG-DOUBLE-ABI on the fortified family specifically —
     * before the trampolines built a real va_list, %Lf could not work here
     * at all.
     * ------------------------------------------------------------------ */
    memset(buf, 0, sizeof buf);
    __snprintf_chk(buf, sizeof buf, 1, sizeof buf, "%.2f", opaque_d(1.25));
    if (strcmp(buf, "1.25") != 0) {
        return 40;
    }

    memset(buf, 0, sizeof buf);
    __snprintf_chk(buf, sizeof buf, 1, sizeof buf, "%.1Lf", opaque_ld(2.5L));
    if (strcmp(buf, "2.5") != 0) {
        return 41;
    }

    /* The desync case: a long double with arguments after it. */
    memset(buf, 0, sizeof buf);
    __snprintf_chk(buf, sizeof buf, 1, sizeof buf, "%d %.3Lf %s %d",
                   opaque_i(7), opaque_ld(2.5L), "mid", opaque_i(9));
    if (strcmp(buf, "7 2.500 mid 9") != 0) {
        return 43; /* 42 is the success code; skip it */
    }

    /* Mixed classes in one call: the double from the register save area,
     * the long double from the overflow area. */
    memset(buf, 0, sizeof buf);
    __sprintf_chk(buf, 1, sizeof buf, "%.1f/%.1Lf/%.1f",
                  opaque_d(0.5), opaque_ld(1.5L), opaque_d(2.5));
    if (strcmp(buf, "0.5/1.5/2.5") != 0) {
        return 44;
    }

    /* And on the four-named-argument wrapper too, whose va_list travels in a
     * different register than __snprintf_chk's. */
    memset(buf, 0, sizeof buf);
    __sprintf_chk(buf, 1, sizeof buf, "%.2Lf|%d", opaque_ld(-0.25L), opaque_i(6));
    if (strcmp(buf, "-0.25|6") != 0) {
        return 45;
    }

    {
        char *p = NULL;
        if (__asprintf_chk(&p, 1, "%.1Lf#%d", opaque_ld(4.5L), opaque_i(1)) < 0 || p == NULL) {
            return 46;
        }
        if (strcmp(p, "4.5#1") != 0) {
            free(p);
            return 46;
        }
        free(p);
    }

    /* ------------------------------------------------------------------
     * 50: the `__v*_chk` forms, reached through a `va_list` we build
     * ourselves.  These are the trampolines' delegation targets, so a pass
     * here with a failure above localises the fault to the assembly.
     * ------------------------------------------------------------------ */
    memset(buf, 0, sizeof buf);
    if (via_valist(buf, sizeof buf, "%d/%.1Lf", opaque_i(8), opaque_ld(0.5L)) != 5) {
        return 50;
    }
    if (strcmp(buf, "8/0.5") != 0) {
        return 51;
    }

    return 42;
}
