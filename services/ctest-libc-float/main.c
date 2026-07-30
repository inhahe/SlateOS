/*
 * ctest-libc-float — ring-3 regression test for the sysroot's FLOAT ABI.
 *
 * Guards BUG-SYSROOT-SOFT-FLOAT-ABI (known-issues.md).
 *
 * The posix crate is compiled for `x86_64-unknown-none`, whose target spec
 * is `-sse,+soft-float`, but the resulting `libc.a` is linked into
 * `x86_64-slateos` programs (`+sse,+sse2`) and into C objects built by
 * `zig cc` (SSE2 is x86-64 baseline).  Under the SysV x86-64 ABI a `double`
 * is returned in %xmm0 and passed in %xmm0-7; under soft-float LLVM uses the
 * general-purpose registers and compiler-rt helpers instead.  So every libc
 * function with a float in its signature had a *silently* mismatched ABI:
 * the callee wrote %rax, the caller read %xmm0, and the caller got garbage.
 *
 * `toolchain/build-sysroot.ps1` now forces `-C target-feature=
 * +sse,+sse2,-soft-float` on the sysroot build.  This fixture is what proves
 * it: it is deliberately plain C, because the whole point is to be a caller
 * compiled by a *different* toolchain than the callee, exercising the ABI
 * boundary rather than a Rust-to-Rust call that LLVM could see through.
 *
 * It covers both directions:
 *   - returns   — strtod, strtof, atof, difftime  (callee writes %xmm0)
 *   - arguments — snprintf("%f", ...)             (varargs; caller writes
 *                 %xmm0-7 and %al, callee spills them to the register save
 *                 area, which a soft-float build never emits)
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below, and the legend in
 * kernel/src/proc/spawn.rs::self_test_clibc_float).
 */

#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

/*
 * Compare with a tolerance rather than `==`.  Every value below is exactly
 * representable in binary floating point, so an exact compare would in fact
 * work — but an ABI mismatch produces garbage that is wrong by many orders
 * of magnitude, never by an ulp, so a tolerant compare loses no power here
 * and cannot be tripped by a legitimate rounding difference.
 */
static int close_enough(double got, double want)
{
    double d = got - want;
    if (d < 0) {
        d = -d;
    }
    return d < 1e-9;
}

int main(void)
{
    char *end;

    /* ---- return direction: strtod ---- */
    end = NULL;
    double d = strtod("3.5", &end);
    if (!close_enough(d, 3.5)) {
        return 10;
    }
    if (end == NULL || *end != '\0') {
        return 11;
    }

    /* A negative value: catches a sign-bit lost in a GPR round trip. */
    end = NULL;
    d = strtod("-2.25", &end);
    if (!close_enough(d, -2.25)) {
        return 12;
    }

    /*
     * A value with no exact 32-bit counterpart and a large exponent — if the
     * result travelled through a soft-float helper or the wrong register it
     * will not survive this.
     */
    end = NULL;
    d = strtod("1.5e10", &end);
    if (!close_enough(d / 1e10, 1.5)) {
        return 13;
    }

    /* strtod must still report the unparsed tail. */
    end = NULL;
    d = strtod("6.5xyz", &end);
    if (!close_enough(d, 6.5)) {
        return 14;
    }
    if (end == NULL || strcmp(end, "xyz") != 0) {
        return 15;
    }

    /* ---- return direction: atof ---- */
    if (!close_enough(atof("0.125"), 0.125)) {
        return 20;
    }
    if (!close_enough(atof("-17"), -17.0)) {
        return 21;
    }

    /* ---- return direction: strtof (float, not double: %xmm0 low half) ---- */
    end = NULL;
    float f = strtof("0.5", &end);
    if (!close_enough((double)f, 0.5)) {
        return 30;
    }
    end = NULL;
    f = strtof("-1.75", &end);
    if (!close_enough((double)f, -1.75)) {
        return 31;
    }

    /* ---- return direction: difftime ---- */
    /* difftime takes two integers and returns a double — the purest test of
     * the return register, with no float input to confuse matters. */
    if (!close_enough(difftime((time_t)100, (time_t)40), 60.0)) {
        return 40;
    }
    if (!close_enough(difftime((time_t)40, (time_t)100), -60.0)) {
        return 41;
    }
    if (!close_enough(difftime((time_t)0, (time_t)0), 0.0)) {
        return 42 + 100; /* never 42: keep the success code unambiguous */
    }

    /* ---- argument direction: varargs doubles through snprintf ---- */
    char buf[64];
    memset(buf, 0, sizeof buf);
    int n = snprintf(buf, sizeof buf, "%.2f", 3.25);
    if (strcmp(buf, "3.25") != 0) {
        return 50;
    }
    if (n != (int)strlen(buf)) {
        return 51;
    }

    /* Several doubles at once: %xmm0..%xmm2 plus %al = 3. */
    memset(buf, 0, sizeof buf);
    n = snprintf(buf, sizeof buf, "%.1f|%.1f|%.1f", 1.5, -2.5, 0.5);
    if (strcmp(buf, "1.5|-2.5|0.5") != 0) {
        return 52;
    }
    if (n != (int)strlen(buf)) {
        return 53;
    }

    /* Mixed integer and float varargs: the two argument classes are counted
     * separately by the ABI, so a soft-float callee mis-indexes both. */
    memset(buf, 0, sizeof buf);
    n = snprintf(buf, sizeof buf, "%d %.3f %s %.1f", 7, 2.5, "mid", 9.5);
    if (strcmp(buf, "7 2.500 mid 9.5") != 0) {
        return 54;
    }
    if (n != (int)strlen(buf)) {
        return 55;
    }

    /* ---- round trip: format a value and parse it back ---- */
    memset(buf, 0, sizeof buf);
    snprintf(buf, sizeof buf, "%.4f", -123.0625);
    end = NULL;
    d = strtod(buf, &end);
    if (!close_enough(d, -123.0625)) {
        return 60;
    }

    return 42;
}
