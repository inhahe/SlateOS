"""glibc 2.39's long double Bessel functions -- j0l, j1l, jnl, y0l, y1l, ynl
-- at their special and extreme arguments, as the oracle for
posix/src/besl.rs's special values, exception flags and errno, in all four
rounding directions. (Their values elsewhere are held to mpmath's, not to
glibc's, which are not correctly rounded: besl_tables.py.)

    python posix/tools/oracle/besl_harness.py   # writes posix/src/besl_glibc.txt

One line a call:

    <mode> <function> <order or -> <x> = <y> <flags> <errno>

<mode> is n, u, d or z (to nearest, upward, downward, toward zero); values
are long doubles as `SSSS:MMMMMMMMMMMMMMMM`, sign-and-exponent then the
explicit significand; <flags> the exceptions the call raised, a letter each
from IZOUX (invalid, divide-by-zero, overflow, underflow, inexact), or -.
Built -O0 with -frounding-math, -fsignaling-nans and -fno-builtin, the
arguments read from volatile arrays, so gcc neither folds a call nor quiets
a signalling NaN on its way in.
"""

import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "besl_glibc.txt"


def ld(x):
    """The long double nearest the Fraction x (ties to even): (se, m)."""
    x = Fraction(x)
    if x == 0:
        return (0, 0)
    sign = 0x8000 if x < 0 else 0
    a = abs(x)
    e = a.numerator.bit_length() - a.denominator.bit_length()
    if Fraction(2) ** e > a:
        e -= 1
    # a in [2^e, 2^(e+1))
    if e < -16382:
        q = a * Fraction(2) ** (16382 + 63)
        m = round(q)
        return (sign | (1 if m >> 63 else 0), m)
    m = round(a * Fraction(2) ** (63 - e))
    if m == 1 << 64:
        m >>= 1
        e += 1
    if e > 16383:
        return (sign | 0x7FFF, 1 << 63)
    return (sign | (e + 16383), m)


def neg(b):
    return (b[0] ^ 0x8000, b[1])


TWO = Fraction(2)
# The special encodings and the ends of the range.
SPECIAL = [
    (0x0000, 0), (0x8000, 0),                                  # +-0
    (0x7FFF, 1 << 63), (0xFFFF, 1 << 63),                      # +-inf
    (0x7FFF, 0xC000000000000000), (0xFFFF, 0xC000000000000000),  # +-qNaN
    (0x7FFF, 0xA000000000000000), (0xFFFF, 0xA000000000000000),  # +-sNaN
    (0x3FFF, 0x4000000000000000),                              # an unnormal
    (0x7FFF, 0), (0x7FFF, 0x4000000000000000),                 # pseudo-inf, pseudo-NaN
    (0x0000, 0x8000000000000000), (0x8000, 0x8000000000000000),  # pseudo-denormals
    (0x0000, 1), (0x8000, 1),                                  # least subnormal
    (0x0000, 0x7FFFFFFFFFFFFFFF),                              # greatest subnormal
    (0x0001, 1 << 63), (0x8001, 1 << 63),                      # least normal
    (0x7FFE, (1 << 64) - 1), (0xFFFE, (1 << 64) - 1),          # +-LDBL_MAX
]
# Arguments at every scale where a function changes behaviour: its pole,
# the underflow of j1 (x/2) and of the y functions' -2/(pi x) overflow, the
# ends of the power series, the switch to the asymptotic forms.
SCALES = [TWO ** e for e in (-16400, -16383, -16382, -16381, -16300, -8200, -8192, -8190,
                             -5000, -200, -100, -70, -66, -65, -64, -40, -33, -32, -20, -1,
                             0, 1, 3, 5, 6, 10, 20, 40, 62, 63, 64, 100, 1000, 5000, 16383)]
SCALES += [Fraction(x) for x in ("1e-4000", "1e-10", "0.5", "2.5", "10", "47.9", "48", "48.1",
                                 "100", "1e10", "1e30", "1e100", "1e1000", "1e4000")]
POINTS = SPECIAL + [ld(x) for x in SCALES] + [neg(ld(x)) for x in SCALES[::3]]

ORDERS = [-2**31, -1000, -3, -2, -1, 0, 1, 2, 3, 1000, 100000, 2**31 - 1]


def big_order_ok(n, b):
    """Whether to ask glibc's jnl/ynl at order n and x = b: every x for an
    order up to 1000; beyond, only a zero, an infinity or a NaN, and for
    order 100000 also |x| < 256, where jnl underflows and ynl overflows in a
    millisecond. glibc recurs about n times wherever it computes: jnl(2^31 -
    1, 1) takes it 18 seconds (and answers -0 where J is positive), and at an
    unnormal, which its NaN test lets through to the arithmetic, order
    -2^31 does not finish at all."""
    if abs(n) <= 1000:
        return True
    se, m = b
    e = se & 0x7FFF
    if (e == 0 and m == 0) or (e == 0x7FFF and m in (1 << 63, 0xC000000000000000, 0xA000000000000000)):
        return True
    if n != 100000 or m >> 63 == 0:
        return False
    return e - 16383 < 8


PRELUDE = r"""
#define _GNU_SOURCE
#include <errno.h>
#include <fenv.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

typedef union { long double v; struct { unsigned long long m; unsigned short se; } s; } U;
static long double L(unsigned se, unsigned long long m) { U u; memset(&u, 0, sizeof u); u.s.m = m; u.s.se = (unsigned short) se; return u.v; }
static void pl(long double x) { U u; memset(&u, 0, sizeof u); u.v = x; printf("%04x:%016llx", u.s.se, u.s.m); }
static const int MODES[4] = { FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO };
static const char NAMES[4] = { 'n', 'u', 'd', 'z' };
static void end(int k, const char *name, const char *order, volatile long double *x, long double y, int f, int e) {
    char s[6]; int n = 0;
    if (f & FE_INVALID) s[n++] = 'I';
    if (f & FE_DIVBYZERO) s[n++] = 'Z';
    if (f & FE_OVERFLOW) s[n++] = 'O';
    if (f & FE_UNDERFLOW) s[n++] = 'U';
    if (f & FE_INEXACT) s[n++] = 'X';
    if (!n) s[n++] = '-';
    s[n] = 0;
    printf("%c %s %s ", NAMES[k], name, order);
    pl(*x);
    printf(" = ");
    pl(y);
    printf(" %s %d\n", s, e);
}
#define CALL1(fn, xv) do { for (int k = 0; k < 4; k++) { volatile long double x = (xv); \
    feclearexcept(FE_ALL_EXCEPT); errno = 0; fesetround(MODES[k]); \
    long double y = fn(x); int f = fetestexcept(FE_ALL_EXCEPT), e = errno; fesetround(FE_TONEAREST); \
    end(k, #fn, "-", &x, y, f, e); } } while (0)
#define CALLN(fn, nv, xv) do { for (int k = 0; k < 4; k++) { volatile long double x = (xv); volatile int n = (nv); \
    char o[16]; snprintf(o, sizeof o, "%d", (int) n); \
    feclearexcept(FE_ALL_EXCEPT); errno = 0; fesetround(MODES[k]); \
    long double y = fn(n, x); int f = fetestexcept(FE_ALL_EXCEPT), e = errno; fesetround(FE_TONEAREST); \
    end(k, #fn, o, &x, y, f, e); } } while (0)
int main(void) {
"""


def build():
    lines = [PRELUDE]
    for b in POINTS:
        arg = f"L(0x{b[0]:04x}, 0x{b[1]:016x}ULL)"
        for fn in ("j0l", "j1l", "y0l", "y1l"):
            lines.append(f"  CALL1({fn}, {arg});")
        for n in ORDERS:
            if big_order_ok(n, b):
                for fn in ("jnl", "ynl"):
                    lines.append(f"  CALLN({fn}, {n}, {arg});")
    lines.append("  return 0;\n}\n")
    return "\n".join(lines)


def main():
    prog = build()
    with workdir() as tmp:
        (Path(tmp) / "besl.c").write_text(prog, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -frounding-math -fsignaling-nans -fno-builtin -w "
                f"-o besl besl.c -lm && ./besl")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-3000:]}")
    header = ("# glibc 2.39's long double Bessel functions at special and extreme arguments,\n"
              "# under WSL (posix/tools/oracle/besl_harness.py): <mode> <function> <order or ->\n"
              "# <x> = <y> <flags> <errno>; mode n u d z, flags from IZOUX or -.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"{len(r.stdout.splitlines())} lines -> {OUT}")


if __name__ == "__main__":
    main()
