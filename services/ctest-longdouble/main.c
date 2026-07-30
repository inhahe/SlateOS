/*
 * ctest-longdouble — ring-3 regression test for the sysroot's `long double`
 * ABI.
 *
 * Guards BUG-POSIX-LONG-DOUBLE-ABI (known-issues.md).
 *
 * The sysroot implements `long double` by computing in `double`.  As a
 * *precision* limitation that is documented and acceptable
 * (TD-POSIX-LONG-DOUBLE-PRECISION).  It was also applied to the *ABI*, where
 * it is not a limitation but silent corruption, in two independent ways:
 *
 *   1. `printf`/`scanf` never consumed the `L` length modifier, so `L` was
 *      read as the *conversion character*.  It matches no conversion, so the
 *      specifier consumed no argument at all and left the va_list cursor on
 *      the long double's 16 bytes — shifting every later argument by two
 *      slots.  A wrong integer three fields later is a much nastier bug than
 *      a wrong float in field one.
 *
 *   2. `strtold` was a Rust `-> f64`, which the SysV ABI returns in %xmm0.
 *      A `long double` is classified X87/X87UP and returned in **%st(0)**, so
 *      every C caller read whatever the x87 stack happened to hold.
 *
 * Neither failure is observable from the posix crate's own host unit tests:
 * those call Rust functions from Rust, where both sides agree on a wrong
 * convention and cancel out.  Only a caller built by a *different* toolchain,
 * which believes the real C ABI, can see it — hence a plain-C fixture.
 *
 * The relevant ABI rules, for reference:
 *   - `long double` is X87/X87UP -> MEMORY: never in a register.  On the
 *     stack it is 16 bytes wide (10 meaningful, 6 padding), 16-byte aligned.
 *   - It is returned in %st(0), never %xmm0.
 *   - Because it is MEMORY, a varargs `long double` touches neither
 *     `gp_offset` nor `fp_offset` — it comes only from the overflow area.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below, and the legend in
 * kernel/src/proc/spawn.rs::self_test_clongdouble).
 */

#include <stdlib.h>
#include <stdio.h>
#include <string.h>

/*
 * Launder values through a volatile global so the compiler cannot constant
 * fold a call away and turn a check into a tautology.  A volatile *local* is
 * weaker — compilers still track its value across the store/load.
 */
static volatile long double g_launder_ld;
static volatile double      g_launder_d;
static volatile int         g_launder_i;

static long double opaque_ld(long double x) { g_launder_ld = x; return g_launder_ld; }
static double      opaque_d(double x)       { g_launder_d = x;  return g_launder_d;  }
static int         opaque_i(int x)          { g_launder_i = x;  return g_launder_i;  }

/*
 * Every value below is exactly representable in binary floating point and
 * survives a double round trip exactly, so an exact compare is legitimate.
 * That matters: an ABI fault produces garbage wrong by orders of magnitude,
 * and a sloppy tolerance could mask the difference between "read the right
 * 16 bytes" and "read 8 of the right bytes plus 8 stale ones".
 */
static int exact_ld(long double got, long double want)
{
    return got == want;
}

int main(void)
{
    char buf[128];
    char *end;

    /* ------------------------------------------------------------------
     * 10-13: the type's shape.  If zig cc and the sysroot disagree about
     * how wide a `long double` is, nothing below means anything.
     * ------------------------------------------------------------------ */
    if (sizeof(long double) != 16) {
        return 10;
    }
    if (_Alignof(long double) != 16) {
        return 11;
    }
    /* Range beyond double's proves this really is the 80-bit type and not a
     * `double` in disguise: 1e400 is finite in x87, infinite in binary64. */
    {
        long double big = opaque_ld(1e300L);
        big = big * big; /* 1e600 — finite only with a 15-bit exponent */
        if (big <= 0.0L) {
            return 12;
        }
        if (big == big / 2.0L) { /* would hold if `big` had saturated to inf */
            return 13;
        }
    }

    /* ------------------------------------------------------------------
     * 20-25: printf's `%L` conversions.  The value itself carries only f64
     * precision by design, so these use values a double represents exactly.
     * ------------------------------------------------------------------ */
    memset(buf, 0, sizeof buf);
    if (snprintf(buf, sizeof buf, "%.2Lf", opaque_ld(3.25L)) < 0) {
        return 20;
    }
    if (strcmp(buf, "3.25") != 0) {
        return 21;
    }

    /* Two long doubles in a row: each occupies 16 bytes, so a cursor that
     * advanced by 8 would read the second from the middle of the first. */
    memset(buf, 0, sizeof buf);
    snprintf(buf, sizeof buf, "%.1Lf|%.1Lf", opaque_ld(1.5L), opaque_ld(-2.5L));
    if (strcmp(buf, "1.5|-2.5") != 0) {
        return 22;
    }

    /*
     * THE REGRESSION.  A `%Lf` followed by more arguments: when the `L` was
     * ignored, everything after it was read 16 bytes early.  The trailing
     * integer and string are the real assertion here — the float is almost
     * incidental.
     */
    memset(buf, 0, sizeof buf);
    snprintf(buf, sizeof buf, "%d %.3Lf %s %d",
             opaque_i(7), opaque_ld(2.5L), "mid", opaque_i(9));
    if (strcmp(buf, "7 2.500 mid 9") != 0) {
        return 23;
    }

    /* %Le and %Lg must consume the modifier too, not just %Lf. */
    memset(buf, 0, sizeof buf);
    snprintf(buf, sizeof buf, "%.3Le %d", opaque_ld(1234.0L), opaque_i(5));
    if (strcmp(buf, "1.234e+03 5") != 0) {
        return 24;
    }

    /* Mixing a `double` and a `long double` in one call: the double goes in
     * %xmm0 (register save area), the long double on the stack.  They must
     * not be pulled from the same place. */
    memset(buf, 0, sizeof buf);
    snprintf(buf, sizeof buf, "%.1f/%.1Lf/%.1f",
             opaque_d(0.5), opaque_ld(1.5L), opaque_d(2.5));
    if (strcmp(buf, "0.5/1.5/2.5") != 0) {
        return 25;
    }

    /* ------------------------------------------------------------------
     * 30-33: strtold's %st(0) return.
     * ------------------------------------------------------------------ */
    end = NULL;
    {
        long double v = strtold("2.5", &end);
        if (!exact_ld(v, 2.5L)) {
            return 30;
        }
        if (end == NULL || *end != '\0') {
            return 31;
        }
    }

    /* Negative, and with a tail to consume: a stale %st(0) would be
     * indifferent to both. */
    end = NULL;
    {
        long double v = strtold("-0.125rest", &end);
        if (!exact_ld(v, -0.125L)) {
            return 32;
        }
        if (end == NULL || strcmp(end, "rest") != 0) {
            return 33;
        }
    }

    /*
     * Call it repeatedly.  The x87 stack is only 8 registers deep; a thunk
     * that pushed without the caller popping would overflow it and start
     * returning NaN (the "indefinite" result) after eight calls.  This is the
     * check that the return convention is *sustainable*, not just correct
     * once.
     */
    {
        long double acc = 0.0L;
        for (int i = 0; i < 32; i++) {
            acc = acc + strtold("1.5", NULL);
        }
        if (!exact_ld(acc, 48.0L)) {
            return 34;
        }
    }

    /* ------------------------------------------------------------------
     * 40-44: scanf's `%L`, which must *store* 16 bytes.
     * ------------------------------------------------------------------ */
    {
        /* Pre-poison so a partial 8-byte store leaves a detectable trace in
         * the sign/exponent half. */
        long double v = -1e30L;
        if (sscanf("6.25", "%Lf", &v) != 1) {
            return 40;
        }
        if (!exact_ld(v, 6.25L)) {
            return 41;
        }
    }

    /* And it must not desynchronise the pointers that follow it. */
    {
        long double v = -1e30L;
        int n = 0;
        if (sscanf("0.75 11", "%Lf %d", &v, &n) != 2) {
            return 42 + 100; /* never 42: keep the success code unambiguous */
        }
        if (!exact_ld(v, 0.75L)) {
            return 43;
        }
        if (n != 11) {
            return 44;
        }
    }

    /* ------------------------------------------------------------------
     * 50: round trip — format a long double and parse it back.
     * ------------------------------------------------------------------ */
    memset(buf, 0, sizeof buf);
    snprintf(buf, sizeof buf, "%.4Lf", opaque_ld(-123.0625L));
    end = NULL;
    if (!exact_ld(strtold(buf, &end), -123.0625L)) {
        return 50;
    }

    return 42;
}
