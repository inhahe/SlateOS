/*
 * ctest-libm — ring-3 test for the sysroot's math library: the *named*
 * floating-point argument ABI, and the numeric behaviour of the code as
 * actually shipped.
 *
 * Companion to `ctest-libc-float` (BUG-SYSROOT-SOFT-FLOAT-ABI).  That fixture
 * proved the sysroot returns doubles in %xmm0 and accepts them as *varargs*.
 * It could not prove anything about **named** float arguments, because no
 * function it calls takes one: `strtod`/`atof`/`strtof` take pointers,
 * `difftime` takes two `time_t`, and `snprintf`'s doubles all go through the
 * variadic path (caller writes %xmm0-7 + %al, callee spills to the register
 * save area).  Named arguments are a *different* rule of the SysV x86-64 ABI
 * — INTEGER vs SSE class assignment straight into %xmm0-%xmm7, no %al, no
 * save area — and libm is where essentially every named-double call in a real
 * program lives.  So the direction that matters most was the one untested.
 *
 * This fixture closes that gap and, in doing so, also becomes the only check
 * that `posix/src/math.rs` is numerically correct *as built for the sysroot*.
 * Its 261 host-side unit tests run on `x86_64-pc-windows-gnu`; the shipped
 * copy is a different compilation entirely (`x86_64-unknown-none`, different
 * target features, different optimisation decisions), so "the host tests pass"
 * is not evidence about the artifact that ends up in `libc.a`.
 *
 * Why plain C, like the other fixture: the bug class is an *inter-toolchain*
 * disagreement.  A Rust caller from the same workspace agrees with the callee
 * by construction and proves nothing.  Here the caller is clang (via `zig cc`)
 * and the callee is rustc — exactly the boundary that broke.
 *
 * Two things defeat this test if you are not careful, and `build.py` guards
 * both:
 *   1. `-fno-builtin` — otherwise clang recognises `sqrt`/`fabs`/`fmax`/… as
 *      builtins and emits `sqrtsd`/`andpd`/`maxsd` inline.  The sysroot would
 *      never be called at all and the test would pass with libc.a absent.
 *   2. `opaque()` below — every input is laundered through a volatile global
 *      so constant folding cannot evaluate the call at compile time.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the legend in kernel/src/proc/spawn.rs::self_test_clibm).
 * 42 is deliberately never used as a failure code.
 */

#include <math.h>

/*
 * <math.h> defines isnan/isinf/isfinite as type-generic *macros* that expand
 * to compiler builtins, so plain `isnan(x)` would test clang and never reach
 * the sysroot.  The sysroot exports all three as real functions
 * (posix/src/math.rs), so undefine the macros and declare the functions —
 * otherwise checks 73-76 below would silently test nothing.
 */
#undef isnan
#undef isinf
#undef isfinite
extern int isnan(double x);
extern int isinf(double x);
extern int isfinite(double x);

/*
 * Reference constants, spelled out rather than taken from <math.h>'s M_PI &c.
 * musl gates those behind feature-test macros, and hard-coding them also means
 * a wrong header cannot make a wrong implementation look right.
 */
#define PI      3.14159265358979323846
#define E       2.71828182845904523536

/*
 * Input laundering.  A volatile global cannot be constant-folded through: the
 * compiler must emit the store, then the load, then a real call.  (`volatile`
 * on a *local* is weaker — some compilers still track the value.)
 */
static volatile double g_launder_d;
static volatile float  g_launder_f;
static volatile int    g_launder_i;

static double opaque(double x)
{
    g_launder_d = x;
    return g_launder_d;
}

static float opaquef(float x)
{
    g_launder_f = x;
    return g_launder_f;
}

static int opaquei(int x)
{
    g_launder_i = x;
    return g_launder_i;
}

/*
 * Relative comparison, floored at 1.0 so it degrades to an absolute
 * comparison near zero (where a relative test is meaningless).
 *
 * An ABI break produces garbage wrong by orders of magnitude, so any sane
 * tolerance catches it.  The tolerance is chosen tight enough (1e-12, ~4 ulp
 * at this magnitude) that it *also* catches a genuinely broken series
 * expansion or range reduction, which is the second thing this fixture is for.
 */
static int close_rel(double got, double want, double tol)
{
    double d = got - want;
    double m = want;
    if (d < 0) {
        d = -d;
    }
    if (m < 0) {
        m = -m;
    }
    if (m < 1.0) {
        m = 1.0;
    }
    return d <= m * tol;
}

/* Exact equality, for the results that must be bit-exact. */
static int exact(double got, double want)
{
    return got == want;
}

int main(void)
{
    /* ================= 10s: named double args, algebraic ================= */

    if (!close_rel(sqrt(opaque(2.0)), 1.41421356237309504880, 1e-12)) {
        return 10;
    }
    /* Exponent range: a broken initial guess in a Newton sqrt shows up here
     * long before it shows up near 1.0. */
    if (!close_rel(sqrt(opaque(1e300)), 1e150, 1e-12)) {
        return 11;
    }
    /* Scaled back to ~1 before comparing: close_rel() floors its denominator
     * at 1.0, so comparing a 1e-150 result directly would degrade to an
     * absolute test that even a returned 0.0 would pass. */
    if (!close_rel(sqrt(opaque(1e-300)) * 1e150, 1.0, 1e-12)) {
        return 12;
    }
    /* Two named doubles: %xmm0 and %xmm1. */
    if (!exact(pow(opaque(2.0), opaque(10.0)), 1024.0)) {
        return 13;
    }
    if (!close_rel(pow(opaque(9.0), opaque(0.5)), 3.0, 1e-12)) {
        return 14;
    }
    if (!exact(fmod(opaque(7.5), opaque(2.0)), 1.5)) {
        return 15;
    }
    if (!close_rel(hypot(opaque(3.0), opaque(4.0)), 5.0, 1e-12)) {
        return 16;
    }
    if (!close_rel(atan2(opaque(1.0), opaque(1.0)), PI / 4.0, 1e-12)) {
        return 17;
    }
    /* atan2 must use the *signs of both* arguments — a swapped or dropped
     * second %xmm would land in the wrong quadrant. */
    if (!close_rel(atan2(opaque(1.0), opaque(-1.0)), 3.0 * PI / 4.0, 1e-12)) {
        return 18;
    }
    if (!exact(fmax(opaque(2.5), opaque(-3.5)), 2.5)) {
        return 19;
    }
    if (!exact(fmin(opaque(2.5), opaque(-3.5)), -3.5)) {
        return 20;
    }
    if (!exact(copysign(opaque(3.0), opaque(-1.0)), -3.0)) {
        return 21;
    }
    if (!exact(fdim(opaque(5.0), opaque(2.0)), 3.0) ||
        !exact(fdim(opaque(2.0), opaque(5.0)), 0.0)) {
        return 22;
    }
    /* Three named doubles: %xmm0, %xmm1, %xmm2. */
    if (!exact(fma(opaque(2.0), opaque(3.0), opaque(4.0)), 10.0)) {
        return 23;
    }
    if (!exact(fabs(opaque(-7.25)), 7.25)) {
        return 24;
    }
    if (!exact(floor(opaque(-2.5)), -3.0) || !exact(ceil(opaque(-2.5)), -2.0)) {
        return 25;
    }
    if (!exact(trunc(opaque(-2.7)), -2.0) || !exact(round(opaque(-2.5)), -3.0)) {
        return 26;
    }
    /* rint is ties-to-even, unlike round. */
    if (!exact(rint(opaque(2.5)), 2.0) || !exact(rint(opaque(3.5)), 4.0) ||
        !exact(rint(opaque(-2.5)), -2.0)) {
        return 27;
    }

    /* ============ 30s: float (f32) arguments — low half of %xmm ========== */

    if (!close_rel((double)sqrtf(opaquef(2.0f)), 1.4142135623730951, 1e-6)) {
        return 30;
    }
    if (!exact((double)powf(opaquef(2.0f), opaquef(10.0f)), 1024.0)) {
        return 31;
    }
    if (!exact((double)fabsf(opaquef(-3.5f)), 3.5)) {
        return 32;
    }
    if (!exact((double)fmaxf(opaquef(1.5f), opaquef(-1.5f)), 1.5) ||
        !exact((double)fminf(opaquef(1.5f), opaquef(-1.5f)), -1.5)) {
        return 33;
    }
    if (!close_rel((double)atan2f(opaquef(1.0f), opaquef(1.0f)), PI / 4.0, 1e-6)) {
        return 34;
    }

    /* ======== 40s: mixed classes and out-parameters (43 skips 42) ======== */

    {
        /* double in %xmm0, int* in %rdi — an SSE and an INTEGER argument, each
         * consuming from its own register sequence. */
        int e = -1;
        double m = frexp(opaque(12.0), &e);
        if (!exact(m, 0.75) || e != 4) {
            return 40;
        }
    }
    {
        double ip = -1.0;
        double fr = modf(opaque(3.75), &ip);
        if (!exact(fr, 0.75) || !exact(ip, 3.0)) {
            return 41;
        }
        fr = modf(opaque(-3.75), &ip);
        if (!exact(fr, -0.75) || !exact(ip, -3.0)) {
            return 43;
        }
    }
    /* double in %xmm0, int in %edi. */
    if (!exact(ldexp(opaque(1.5), opaquei(3)), 12.0)) {
        return 44;
    }
    if (!exact(ldexp(opaque(1.5), opaquei(-3)), 0.1875)) {
        return 45;
    }

    /* ==================== 50s: transcendental accuracy =================== */

    if (!close_rel(exp(opaque(1.0)), E, 1e-12)) {
        return 50;
    }
    if (!close_rel(exp(opaque(-1.0)), 1.0 / E, 1e-12)) {
        return 51;
    }
    if (!close_rel(log(opaque(E)), 1.0, 1e-12)) {
        return 52;
    }
    if (!close_rel(log10(opaque(1000.0)), 3.0, 1e-12)) {
        return 53;
    }
    if (!close_rel(log2(opaque(1024.0)), 10.0, 1e-12)) {
        return 54;
    }
    if (!close_rel(exp2(opaque(10.0)), 1024.0, 1e-12)) {
        return 55;
    }
    if (!close_rel(sin(opaque(PI / 6.0)), 0.5, 1e-12)) {
        return 56;
    }
    if (!close_rel(cos(opaque(PI / 3.0)), 0.5, 1e-12)) {
        return 57;
    }
    if (!close_rel(tan(opaque(PI / 4.0)), 1.0, 1e-12)) {
        return 58;
    }
    if (!close_rel(atan(opaque(1.0)), PI / 4.0, 1e-12)) {
        return 59;
    }
    if (!close_rel(asin(opaque(0.5)), PI / 6.0, 1e-12)) {
        return 60;
    }
    if (!close_rel(acos(opaque(0.5)), PI / 3.0, 1e-12)) {
        return 61;
    }
    if (!close_rel(cbrt(opaque(27.0)), 3.0, 1e-12)) {
        return 62;
    }
    /* log1p/expm1 exist precisely to keep relative accuracy where log(1+x)
     * and exp(x)-1 lose it to cancellation.  A version that just forwarded to
     * log/exp would fail these two. */
    if (!close_rel(log1p(opaque(1e-10)) * 1e10, 1.0, 1e-9)) {
        return 63;
    }
    if (!close_rel(expm1(opaque(1e-10)) * 1e10, 1.0, 1e-9)) {
        return 64;
    }
    if (!close_rel(sinh(opaque(1.0)), 1.17520119364380145688, 1e-12)) {
        return 65;
    }
    if (!close_rel(cosh(opaque(1.0)), 1.54308063481524377848, 1e-12)) {
        return 66;
    }
    if (!close_rel(tanh(opaque(1.0)), 0.76159415595576488812, 1e-12)) {
        return 67;
    }
    {
        /* Range reduction: sin²+cos² == 1 well outside [-π, π]. */
        double x = opaque(10.0);
        double s = sin(x);
        double c = cos(x);
        if (!close_rel(s * s + c * c, 1.0, 1e-11)) {
            return 68;
        }
    }

    /* ============ 70s: integer returns and classification =============== */

    if (lround(opaque(2.5)) != 3 || lround(opaque(-2.5)) != -3) {
        return 70;
    }
    if (llround(opaque(2.5)) != 3) {
        return 71;
    }
    /* lrint is ties-to-even where lround is ties-away-from-zero. */
    if (lrint(opaque(2.5)) != 2 || lrint(opaque(3.5)) != 4) {
        return 72;
    }
    if (isnan(opaque((double)NAN)) == 0 || isnan(opaque(1.0)) != 0) {
        return 73;
    }
    if (isinf(opaque((double)INFINITY)) == 0 || isinf(opaque(1.0)) != 0) {
        return 74;
    }
    if (isfinite(opaque(1.0)) == 0 || isfinite(opaque((double)INFINITY)) != 0) {
        return 75;
    }
    /* sqrt of a negative is NaN, not a trap and not a wrong finite number. */
    if (isnan(sqrt(opaque(-1.0))) == 0) {
        return 76;
    }

    return 42;
}
