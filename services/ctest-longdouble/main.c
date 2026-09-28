/*
 * ctest-longdouble — ring-3 regression test for the sysroot's `long double`
 * ABI.
 *
 * Guards BUG-POSIX-LONG-DOUBLE-ABI (known-issues.md).
 *
 * The sysroot's printf, scanf and strtold handle `long double` by computing
 * in `double`.  As a *precision* limitation that is documented and
 * acceptable (TD-POSIX-LONG-DOUBLE-PRECISION).  It was also applied to the
 * *ABI*, where it is not a limitation but silent corruption, in two
 * independent ways:
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
 * Since 2026-09-28 it also calls libm's `long double` functions (codes
 * 60-84), which compute in the full 80-bit format and reach C through one
 * assembly thunk per signature shape (posix/src/ld_abi.rs): every shape is
 * called here, with values a double could not produce.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below, and the legend in
 * kernel/src/proc/spawn.rs::self_test_clongdouble).
 */

#define _GNU_SOURCE /* sincosl, lgammal_r, signgam */

#include <errno.h>
#include <math.h>
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

/*
 * Within `tol` of `want`, for the functions that are accurate to a few units
 * in the last place of the 64-bit significand but not correctly rounded.
 * Every tolerance below is 2^-59 or finer relative to the value, where a
 * double's own rounding is 2^-53: a result computed in double fails.  A NaN
 * fails too (every comparison with it is false).
 */
static int near_ld(long double got, long double want, long double tol)
{
    long double d = got - want;
    return (d < 0 ? -d : d) <= tol;
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

    /* ------------------------------------------------------------------
     * 60-84: libm's long double functions.  Each C name is an assembly
     * thunk (posix/src/ld_abi.rs) that hands a Rust function pointers to
     * the stack arguments and loads the result into %st(0); the host
     * tests call the Rust functions directly, so the thunks -- one per
     * C signature shape -- are only ever exercised here.  Every value is
     * either exact or held to a few units in the last place of the
     * 64-bit significand, far inside what a double-precision stand-in
     * (53 bits) could reach, so "computed in double" fails too.
     * ------------------------------------------------------------------ */

    /* 60: L f(L), correctly rounded -- sqrt is exact in IEEE, so the
     * 64-bit answer is fixed to the last bit. */
    if (!exact_ld(sqrtl(opaque_ld(2.0L)), 1.41421356237309504880168872420969808L)) {
        return 60;
    }
    if (!near_ld(sinl(opaque_ld(0.5L)), 0.479425538604203000273287935215571388L, 0x1p-62L)) {
        return 61;
    }

    /* 62: L f(L, L), in order: powl(2, 0.5) is sqrt 2, where the arguments
     * swapped would give a quarter; fmodl is exact. */
    if (!near_ld(powl(opaque_ld(2.0L), opaque_ld(0.5L)), 1.41421356237309504880168872420969808L, 0x1p-60L)) {
        return 62;
    }
    if (!exact_ld(fmodl(opaque_ld(10.5L), opaque_ld(3.0L)), 1.5L)) {
        return 63;
    }

    /* 64: L f(L, L, L).  (1 + 2^-32)^2 - 1 is 2^-31 + 2^-64 exactly, which
     * only a fused multiply-add keeps: the product rounded first loses the
     * 2^-64.  The second call has y and z apart, so a thunk that swapped
     * them would answer 1 - 2^-32. */
    {
        long double a = opaque_ld(1.0L + 0x1p-32L);
        if (!exact_ld(fmal(a, a, opaque_ld(-1.0L)), 0x1p-31L + 0x1p-64L)) {
            return 64;
        }
        if (!exact_ld(fmal(a, opaque_ld(2.0L), opaque_ld(-1.0L)), 1.0L + 0x1p-31L)) {
            return 65;
        }
    }

    /* 66: L f(L, int) and L f(L, long): scaling far outside double's
     * range, into long double's subnormals. */
    if (!exact_ld(ldexpl(opaque_ld(1.0L), opaque_i(16000)), 0x1p16000L)) {
        return 66;
    }
    if (!exact_ld(scalbnl(opaque_ld(3.0L), opaque_i(-16400)), 0x3p-16400L)) {
        return 67;
    }
    if (!exact_ld(scalblnl(opaque_ld(1.0L), -16445L), 0x1p-16445L)) {
        return 68;
    }

    /* 69: L f(L, P), L f(L, L, P) and L f(P): each out-parameter written,
     * in full, where the caller asked. */
    {
        int e = 12345;
        long double ip = 1e300L;
        int q = 12345;
        int sg = 12345;
        if (!exact_ld(frexpl(opaque_ld(12.0L), &e), 0.75L) || e != 4) {
            return 69;
        }
        if (!exact_ld(frexpl(opaque_ld(0x1p-16440L), &e), 0.5L) || e != -16439) {
            return 70;
        }
        if (!exact_ld(modfl(opaque_ld(-3.25L), &ip), -0.25L) || !exact_ld(ip, -3.0L)) {
            return 71;
        }
        if (!exact_ld(remquol(opaque_ld(10.0L), opaque_ld(3.0L), &q), 1.0L) || q != 3) {
            return 72;
        }
        /* lgamma(-1/2) = log(2 sqrt(pi)), and gamma(-1/2) is negative. */
        if (!near_ld(lgammal_r(opaque_ld(-0.5L), &sg), 1.26551212348464539648894579713470592L, 0x1p-59L)
            || sg != -1) {
            return 73;
        }
        signgam = 0;
        if (!near_ld(lgammal(opaque_ld(-0.5L)), 1.26551212348464539648894579713470592L, 0x1p-59L)
            || signgam != -1) {
            return 74;
        }
        {
            long double n = nanl("0x5");
            unsigned long long m;
            unsigned short se;
            memcpy(&m, &n, sizeof m);
            memcpy(&se, (const char *)&n + 8, sizeof se);
            if (m != 0xC000000000000005ULL || se != 0x7FFF) {
                return 75;
            }
        }
    }

    /* 76: void f(L, P, P): both results, each in its own slot. */
    {
        long double s = 1e300L, c = 1e300L;
        sincosl(opaque_ld(0.5L), &s, &c);
        if (!near_ld(s, 0.479425538604203000273287935215571388L, 0x1p-62L)
            || !near_ld(c, 0.877582561890372716116281582603829651L, 0x1p-61L)) {
            return 76;
        }
    }

    /* 77: int f(L) and long f(L): ilogbl of a long double subnormal, the
     * rounding functions, and musl's classification macros, which call
     * __fpclassifyl and __signbitl for a long double. */
    if (ilogbl(opaque_ld(0x1p-16400L)) != -16400) {
        return 77;
    }
    if (lrintl(opaque_ld(2.5L)) != 2 || llroundl(opaque_ld(-2.5L)) != -3) {
        return 78;
    }
    {
        long double inf = 1.0L / opaque_ld(0.0L);
        if (fpclassify(opaque_ld(0x1p-16400L)) != FP_SUBNORMAL || !isnan(nanl(""))
            || !isinf(-inf) || !signbit(opaque_ld(-0.0L)) || !isfinite(opaque_ld(1e4000L))) {
            return 79;
        }
    }

    /* 80: D f(D, L) and F f(F, L): the double or float stays in %xmm0 while
     * the long double goes on the stack. */
    if (nexttoward(opaque_d(1.0), opaque_ld(2.0L)) != 1.0 + 0x1p-52) {
        return 80;
    }
    if (nexttowardf((float)opaque_d(1.0), opaque_ld(0.0L)) != 1.0f - 0x1p-24f) {
        return 81;
    }

    /* 82: errno, which the Rust half sets and C reads. */
    errno = 0;
    if (!isnan(logl(opaque_ld(-1.0L))) || errno != EDOM) {
        return 82;
    }
    errno = 0;
    if (!isinf(expl(opaque_ld(20000.0L))) || errno != ERANGE) {
        return 83;
    }

    /* 84: every shape, 32 times over.  A thunk that left anything on the
     * 8-deep x87 register stack would overflow it within eight rounds and
     * turn every later result into the NaN "indefinite". */
    {
        long double acc = 0.0L;
        for (int i = 0; i < 32; i++) {
            int e = 0, q = 0;
            long double ip = 0.0L, s = 0.0L, c = 0.0L;
            acc += fabsl(opaque_ld(-1.0L));                                    /* 1 */
            acc += fmodl(opaque_ld(7.5L), opaque_ld(2.0L));                    /* 1.5 */
            acc += fmal(opaque_ld(2.0L), opaque_ld(3.0L), opaque_ld(-5.0L));   /* 1 */
            acc += ldexpl(opaque_ld(1.0L), opaque_i(1));                        /* 2 */
            acc += scalblnl(opaque_ld(1.0L), 2L);                               /* 4 */
            /* The out-parameters are read in statements of their own: in
             * `f(&e) + e` the read of e is not sequenced after the call. */
            acc += frexpl(opaque_ld(8.0L), &e);                                 /* 0.5 */
            acc += e;                                                           /* 4 */
            acc += remquol(opaque_ld(7.0L), opaque_ld(2.0L), &q);               /* -1 */
            acc += q;                                                           /* 4 */
            (void)nanl("");
            sincosl(opaque_ld(0.0L), &s, &c);
            acc += s + c;                                                       /* 1 */
            acc += modfl(opaque_ld(2.5L), &ip);                                 /* 0.5 */
            acc += ip;                                                          /* 2 */
            acc += ilogbl(opaque_ld(8.0L));                                     /* 3 */
            acc += nexttoward(opaque_d(1.0), opaque_ld(1.0L));                  /* 1 */
            acc += nexttowardf(1.0f, opaque_ld(1.0L));                          /* 1 */
        }
        /* 1 + 1.5 + 1 + 2 + 4 + 4.5 + 3 + 1 + 2.5 + 3 + 1 + 1 = 25.5 */
        if (!exact_ld(acc, 32 * 25.5L)) {
            return 84;
        }
    }

    return 42;
}
