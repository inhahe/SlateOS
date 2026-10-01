"""The inputs of the math oracle: for every libm function the libc exports,
the values glibc 2.39 is asked about (math_harness.py runs them).

Each case is (function, signature, inputs), inputs as Python floats or ints.
Signatures, by the C prototype:

    d_d    double f(double)            f_f    float f(float)
    d_dd   double f(double, double)    f_ff   float f(float, float)
    d_ddd  double f(double,double,double)  f_fff  float f(float,float,float)
    i_d    int f(double)               i_f    int f(float)
    l_d    long f(double)              l_f    long f(float)
    d_di   double f(double, int)       f_fi   float f(float, int)
    d_dl   double f(double, long)      f_fl   float f(float, long)
    d_id   double f(int, double)       (jn, yn)
    d_dpi  double f(double, int *)     f_fpi  (frexp, lgamma_r)
    d_dpd  double f(double, double *)  f_fpf  (modf)
    d_ddpi double f(double, double, int *)  f_ffpi  (remquo)
    v_dpdpd void f(double, double *, double *)  v_fpfpf  (sincos)
"""

import math
import random
import struct

RNG = random.Random(20260927)


def f32(x):
    """The float nearest x, as a Python float: IEEE round-to-nearest, so a
    value past FLT_MAX by half an ulp or more is an infinity (struct refuses
    those rather than rounding them)."""
    if math.isfinite(x) and abs(x) >= 3.4028235677973366e38:
        return math.copysign(math.inf, x)
    return struct.unpack("<f", struct.pack("<f", x))[0]


def bits64(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def from64(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def bits32(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def from32(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def next_up(x):
    return math.nextafter(x, math.inf)


def next_down(x):
    return math.nextafter(x, -math.inf)


INF = math.inf
NAN = math.nan
DMIN = 2.2250738585072014e-308       # DBL_MIN
DSUB = 5e-324                        # the least subnormal
DMAX = 1.7976931348623157e308
FMIN = f32(1.1754943508222875e-38)
FSUB = from32(1)
FMAX = from32(0x7F7FFFFF)

SPECIAL = [0.0, -0.0, DSUB, -DSUB, DMIN, -DMIN, next_down(DMIN), 1.0, -1.0, 0.5, -0.5,
           next_down(0.5), -next_down(0.5), next_up(1.0), next_down(1.0), 2.0, -2.0,
           2.5, -2.5, 3.5, -0.3, 0.3, 1e-10, -1e-10, 1e10, -1e10, 2.0**52, 2.0**52 + 1,
           -(2.0**52 + 1), 2.0**53, 2.0**53 - 1, 1e22, -1e22, 1e300, DMAX, -DMAX, INF,
           -INF, NAN, math.pi, -math.pi, math.pi / 2, math.pi / 4, math.e]

SPECIAL_F = sorted({f32(v) for v in SPECIAL if not math.isnan(v)} | {FSUB, -FSUB, FMIN,
                   -FMIN, FMAX, -FMAX, f32(next_down(0.5)), from32(0x3EFFFFFF),
                   from32(0xBEFFFFFF), 2.0**23 + 1, -(2.0**23 + 1), 2.0**24, 16777215.0,
                   f32(1e20)}, key=lambda v: (math.copysign(1, v), abs(v))) + [NAN]


def rand_log(n, lo_exp, hi_exp, signed=True, single=False):
    """n values with log-uniform magnitude 2^lo_exp .. 2^hi_exp."""
    out = []
    for _ in range(n):
        m = RNG.uniform(1.0, 2.0)
        e = RNG.randint(lo_exp, hi_exp)
        v = math.ldexp(m, e)
        if signed and RNG.random() < 0.5:
            v = -v
        out.append(f32(v) if single else v)
    return out


def rand_uniform(n, lo, hi, single=False):
    return [f32(RNG.uniform(lo, hi)) if single else RNG.uniform(lo, hi) for _ in range(n)]


def near(xs, single=False):
    """Each value and its neighbours."""
    out = []
    for x in xs:
        if single:
            b = bits32(f32(x))
            out += [from32(b - 1) if b & 0x7FFFFFFF else f32(x), f32(x), from32(b + 1)]
        else:
            out += [next_down(x), x, next_up(x)]
    return out


# Boundaries worth their own rows, by function (double; the float lists are
# made by rounding these and adding float-specific ones below).
EXTRA = {
    "exp": [709.782712893384, 709.7827128933841, 709.79, -708.3964185322641, -708.4,
            -744.4400719213812, -745.1332191019411, -745.1332191019412, -746.0, 1e-20],
    "exp2": [1023.0, 1023.9999999999999, 1024.0, -1022.0, -1074.0, -1075.0, -1076.0],
    "exp10": [308.25471555991675, 308.2547155599168, -307.6526555685888, -323.3, -324.0],
    "expm1": [709.782712893384, 709.79, -40.0, -1e-300, 1e-300, 1e-5],
    "log": [1.0000000001, 0.9999999999, 1e-300, DSUB],
    "log2": [1.0000000001, 1024.0, 3.0],
    "log10": [1000.0, 1e22, 1e-310],
    "log1p": [-1.0, next_up(-1.0), -0.9999999, 1e-18, -1e-18, -2.0],
    "sin": [1e22, 2.0**1023, 3.141592653589793, 1e-300, 6381956970095103.0 * 2.0**797],
    "cos": [1e22, 2.0**1023, 1.5707963267948966, 5.0e-324],
    "tan": [1e22, 1.5707963267948966, 2.0**1023, 1e-300],
    "asin": [next_up(1.0), next_down(1.0), 1.0000001, -1.0000001],
    "acos": [next_up(1.0), next_down(-1.0), 1.0000001],
    "atan": [1e300, -1e300, 1e-300],
    "sinh": [710.4758600739439, 710.476, -710.476, 1e-300],
    "cosh": [710.4758600739439, 710.476, -710.476],
    "tanh": [20.0, 22.0, -22.0, 1e-300],
    "asinh": [1e300, -1e300, 1e-300],
    "acosh": [next_down(1.0), 1e300, 1.0000000001],
    "atanh": [next_down(1.0), -next_down(1.0), 1.0000001, 1e-300],
    "cbrt": [27.0, -27.0, 1e-310, 8e300],
    "erf": [6.0, -6.0, 1e-300, 0.84375],
    "erfc": [27.0, 27.3, 28.0, -6.0, 1e-300],
    "lgamma": [-1.0, -2.0, -2.5, 1.0, 2.0, 1e-300, 2.5e305, 2.6e305, -1e-300],
    "tgamma": [-1.0, -2.0, -0.5, 171.6, 171.7, 1e-300, -170.5, -171.5, -190.5],
    "sqrt": [2.0, 3.0, 1e-310, -1e-310],
    "rint": [0.5, 1.5, 2.5, -0.5, -1.5, 4503599627370495.5],
    "nearbyint": [0.5, 1.5, 2.5, -0.5, -1.5],
    "logb": [DSUB, 1e-310],
    "significand": [DSUB, 1e-310, 3.0],
    "j0": [1e30, 2.404825557695773, 1e-300],
    "j1": [1e30, 3.8317059702075125, 1e-300],
    "y0": [1e30, 0.8935769662791675, 1e-300, -1.0],
    "y1": [1e30, 2.197141326031017, 1e-300, -1.0],
}

UNARY = ["fabs", "floor", "ceil", "round", "trunc", "sqrt", "exp", "exp2", "exp10",
         "expm1", "log", "log2", "log10", "log1p", "sin", "cos", "tan", "asin", "acos",
         "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "cbrt", "erf", "erfc",
         "lgamma", "tgamma", "rint", "nearbyint", "logb", "significand", "gamma"]
BESSEL = ["j0", "j1", "y0", "y1"]
BINARY = ["fmod", "pow", "atan2", "copysign", "fmin", "fmax", "hypot", "fdim",
          "nextafter", "remainder", "drem"]


def unary_inputs(name, single):
    base = SPECIAL_F if single else SPECIAL
    extra = EXTRA.get("lgamma" if name == "gamma" else name, [])
    xs = list(base) + [f32(v) if single else v for v in extra]
    xs += near([v for v in extra if math.isfinite(v)][:6], single)
    if single:
        xs += rand_log(40, -30, 30, single=True) + rand_uniform(20, -10, 10, single=True)
    else:
        xs += rand_log(40, -60, 60) + rand_uniform(20, -10, 10) + rand_log(10, -1000, 1000)
    return xs


def binary_inputs(name, single):
    vals = [0.0, -0.0, 1.0, -1.0, 0.5, 2.0, -2.0, 3.0, -3.0, 0.3, 1e300, 1e-300, DSUB,
            INF, -INF, NAN, 2.0**53, 1e10, -1e10, 7.0]
    if single:
        vals = [f32(v) for v in vals] + [FSUB, FMAX]
    pairs = [(a, b) for a in vals for b in vals]
    if name == "pow":
        pairs += [(-2.0, 0.5), (-8.0, 1 / 3), (-2.0, 3.0), (-2.0, 1e300), (2.0, 1024.0),
                  (2.0, -1075.0), (0.0, -1.0), (-0.0, -3.0), (-0.0, -2.0), (10.0, 308.25),
                  (1.0000001, 1e10), (0.9999999, -1e10), (-1.0, INF), (-1.0, 1e300)]
    if name in ("fmod", "remainder", "drem"):
        pairs += [(1e300, 3.0), (5.5, 2.0), (-5.5, 2.0), (2.5, 1.0), (3.5, 1.0),
                  (1e22, math.pi), (DSUB * 3, DSUB * 2)]
    if name == "hypot":
        pairs += [(DMAX, DMAX), (3e300, 4e300), (DSUB, DSUB), (1e308, 1e308)]
    if name == "atan2":
        pairs += [(1e-300, 1e300), (1e300, 1e-300), (-0.0, -1.0), (0.0, -1.0)]
    if single:
        pairs = [(f32(a), f32(b)) for a, b in pairs]
    lo, hi = (-30, 30) if single else (-100, 100)
    ra = rand_log(40, lo, hi, single=single)
    rb = rand_log(40, lo, hi, single=single)
    pairs += list(zip(ra, rb))
    return pairs


def fma_inputs(single):
    """The cases a two-rounding x*y+z gets wrong, and the edges."""
    if single:
        e = 2.0 ** -23
        rows = [(1 + e, 1 - e, -1.0), (1 + e, 1 + e, -(1 + 2 * e)), (FMAX, 2.0, -FMAX),
                (FSUB, 0.5, 0.0), (FSUB, 0.5, -0.0), (0.1, 10.0, -1.0),
                (3.0, 1 / 3, -1.0), (FMIN, FMIN, 0.0), (FMIN, 0.5, FSUB)]
        rows = [tuple(f32(v) for v in r) for r in rows]
        rows += [tuple(rand_log(1, -40, 40, single=True)[0] for _ in range(3)) for _ in range(40)]
    else:
        e = 2.0 ** -52
        rows = [(1 + e, 1 - e, -1.0), (1 + e, 1 + e, -(1 + 2 * e)), (DMAX, 2.0, -DMAX),
                (DSUB, 0.5, 0.0), (DSUB, 0.5, -0.0), (0.1, 10.0, -1.0),
                (3.0, 1 / 3, -1.0), (DMIN, DMIN, 0.0), (DMIN, 0.5, DSUB),
                (1e308, 10.0, -1e308), (INF, 0.0, 1.0), (0.0, INF, NAN), (INF, 1.0, -INF),
                (2.0**-537, 2.0**-537, 0.0), (-(2.0**-537), 2.0**-537, DSUB),
                (1.0, 1.0, -1.0), (-1.0, 1.0, 1.0), (1e-160, 1e-160, -1e-320)]
        rows += [tuple(rand_log(1, -300, 300)[0] for _ in range(3)) for _ in range(40)]
        # Products that cancel against z to the last bit.
        for _ in range(40):
            a = rand_log(1, -20, 20)[0]
            b = rand_log(1, -20, 20)[0]
            rows.append((a, b, -(a * b)))
    return rows


def int_rows():
    """ldexp/scalbn/scalbln, and jn/yn."""
    xs = [1.0, -1.0, 0.0, -0.0, DSUB, DMIN, 1.5, DMAX, INF, NAN, 3.0 * DSUB, 0.75]
    ns = [0, 1, -1, 1023, 1024, -1022, -1074, -1075, -1076, 2000, -2000, 2**31 - 1, -(2**31)]
    return [(x, n) for x in xs for n in ns]


def int_rows_f():
    xs = [1.0, -1.0, 0.0, -0.0, FSUB, FMIN, 1.5, FMAX, INF, NAN, 3.0 * FSUB, 0.75]
    ns = [0, 1, -1, 127, 128, -126, -149, -150, -151, 300, -300, 2**31 - 1, -(2**31)]
    return [(f32(x), n) for x in xs for n in ns]


def cases():
    out = []
    for name in UNARY:
        out.append((name, "d_d", [(x,) for x in unary_inputs(name, False)]))
        if name not in ("exp10", "significand", "gamma"):
            out.append((name + "f", "f_f", [(x,) for x in unary_inputs(name, True)]))
    out.append(("exp10f", "f_f", [(x,) for x in unary_inputs("exp10", True)]))
    for name in BESSEL:
        out.append((name, "d_d", [(x,) for x in unary_inputs(name, False)]))
    for name in BINARY:
        out.append((name, "d_dd", binary_inputs(name, False)))
        if name != "drem":
            out.append((name + "f", "f_ff", binary_inputs(name, True)))
    out.append(("fma", "d_ddd", fma_inputs(False)))
    out.append(("fmaf", "f_fff", fma_inputs(True)))
    ints = SPECIAL + rand_log(20, -60, 64) + [2.0**63, -(2.0**63), 2.0**63 - 1024,
                                              -(2.0**63) - 2048, 9.2e18, -9.3e18]
    ints_f = SPECIAL_F + rand_log(20, -30, 64, single=True) + [f32(2.0**63), f32(-(2.0**63))]
    for name, sig in (("ilogb", "i_d"), ("lround", "l_d"), ("llround", "l_d"),
                      ("lrint", "l_d"), ("finite", "i_d"), ("isnan", "i_d"), ("isinf", "i_d")):
        out.append((name, sig, [(x,) for x in ints]))
    for name, sig in (("ilogbf", "i_f"), ("lroundf", "l_f"), ("llroundf", "l_f"),
                      ("lrintf", "l_f"), ("finitef", "i_f")):
        out.append((name, sig, [(x,) for x in ints_f]))
    out.append(("ldexp", "d_di", int_rows()))
    out.append(("scalbn", "d_di", int_rows()))
    out.append(("scalbln", "d_dl", int_rows()))
    out.append(("ldexpf", "f_fi", int_rows_f()))
    out.append(("scalbnf", "f_fi", int_rows_f()))
    out.append(("scalblnf", "f_fl", int_rows_f()))
    nx = [(n, x) for n in (0, 1, 2, 5, -1, -2, 20, 100) for x in (0.0, 1.0, 2.5, -2.5, 10.0,
                                                               1e-300, 50.0, 1e10, INF, NAN)]
    out.append(("jn", "d_id", nx))
    out.append(("yn", "d_id", nx))
    frex = SPECIAL + rand_log(20, -1070, 1020)
    out.append(("frexp", "d_dpi", [(x,) for x in frex]))
    out.append(("lgamma_r", "d_dpi", [(x,) for x in unary_inputs("lgamma", False)]))
    out.append(("modf", "d_dpd", [(x,) for x in SPECIAL + rand_log(20, -60, 60)]))
    out.append(("frexpf", "f_fpi", [(x,) for x in SPECIAL_F + rand_log(20, -148, 126, single=True)]))
    out.append(("lgammaf_r", "f_fpi", [(x,) for x in unary_inputs("lgamma", True)]))
    out.append(("modff", "f_fpf", [(x,) for x in SPECIAL_F + rand_log(20, -30, 30, single=True)]))
    out.append(("remquo", "d_ddpi", binary_inputs("remainder", False)))
    out.append(("remquof", "f_ffpi", binary_inputs("remainder", True)))
    out.append(("sincos", "v_dpdpd", [(x,) for x in unary_inputs("sin", False)]))
    out.append(("sincosf", "v_fpfpf", [(x,) for x in unary_inputs("sin", True)]))
    # scalb and scalbf come last and draw nothing from the generator, so every
    # line above them is unchanged by their arrival (2026-09-28).
    xs = [0.0, -0.0, 1.0, -1.0, 0.3, 1e300, 1e-300, DSUB, INF, -INF, NAN, 7.0]
    fns = [0.0, -0.0, 1.0, -1.0, 2.5, 10.0, -10.0, 1024.0, -1075.0, 1e5, -1e5, 65000.0,
           65001.0, -65001.0, 2.0**31, INF, -INF, NAN]
    out.append(("scalb", "d_dd", [(x, n) for x in xs for n in fns]))
    out.append(("scalbf", "f_ff", [(f32(x), f32(n)) for x in xs for n in fns]))
    return out


if __name__ == "__main__":
    total = 0
    for name, sig, rows in cases():
        total += len(rows)
        print(f"{name:12} {sig:8} {len(rows)}")
    print("total", total)
