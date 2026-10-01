"""glibc 2.39's C23 and GNU <math.h> functions that are exact -- no rounding
of their own -- as the oracle for posix/src/c23math.rs: `nextup`,
`nextdown`, `llogb` (and for long double the `ilogbl` and `logbl` it stands
on), `canonicalize`, the `fromfp` four, the payload three,
`totalorder` and `totalordermag`, the ten maximum and minimum functions, and
`scalbl`, each for `float`, `double` and `long double`.

    python posix/tools/oracle/c23math_harness.py   # writes posix/src/c23math_oracle.txt

The C program sets errno to 0 and clears every exception flag before each
call and prints one line after it:

    <function> <type> <inputs> = <outputs> <flags> <errno>

`<type>` is `f`, `d` or `l`; values are their bits in hex -- 8 digits for a
float, 16 for a double, `SSSS:MMMMMMMMMMMMMMMM` (sign and exponent, then the
explicit significand) for a long double, the form mathl_oracle.txt uses --
and integers decimal. `<flags>` are the exceptions the call raised, a
letter each from `IZOUX` (invalid, divide-by-zero, overflow, underflow,
inexact), or `-` for none. A `fromfp` line answers for `fromfp` and
`fromfpx` together (`ufromfp` and `ufromfpx` likewise), in all five rounding
directions: `<result>:<flags>:<errno>` five times -- FP_INT_UPWARD,
FP_INT_DOWNWARD, FP_INT_TOWARDZERO, FP_INT_TONEARESTFROMZERO,
FP_INT_TONEAREST -- for the first, then `/`, then five for the `x` one.

None of these functions rounds in the current direction, so every call is
made to nearest. Built -O0 with -frounding-math, -fsignaling-nans and
-fno-builtin, from volatile arrays, so gcc neither folds a call nor quiets a
signaling NaN on its way in.
"""

import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "c23math_oracle.txt"


# -- values, as each type's bits ---------------------------------------------

FORMATS = {
    # (exponent bits, stored significand bits, explicit integer bit)
    "f": (8, 23, False),
    "d": (11, 52, False),
    "l": (15, 64, True),
}


def encode(t, x):
    """The value `x` (a Fraction or int) in type `t`, rounded to nearest-even:
    an int of its bits for f and d, (sign_exp, significand) for l."""
    ebits, mbits, explicit = FORMATS[t]
    bias = (1 << (ebits - 1)) - 1
    prec = mbits if explicit else mbits + 1
    x = Fraction(x)
    sign = 1 if x < 0 else 0
    a = abs(x)
    if a == 0:
        biased, sig = 0, 0
    else:
        e = a.numerator.bit_length() - a.denominator.bit_length()
        while a >= Fraction(2) ** (e + 1):
            e += 1
        while a < Fraction(2) ** e:
            e -= 1
        emin = 1 - bias
        e = max(e, emin)
        m = a / Fraction(2) ** (e - prec + 1)
        q, r = divmod(m.numerator, m.denominator)
        if 2 * r > m.denominator or (2 * r == m.denominator and q & 1):
            q += 1
        if q == 1 << prec:
            q >>= 1
            e += 1
        if q >> (prec - 1):
            biased = e + bias
        else:
            biased = 0
        if biased >= (1 << ebits) - 1:
            raise ValueError(f"{x} overflows {t}")
        sig = q if explicit else q & ((1 << mbits) - 1)
    if t == "l":
        return ((sign << 15) | biased, sig)
    return (sign << (ebits + mbits)) | (biased << mbits) | sig


def representable(t, x):
    try:
        return decode(t, encode(t, x)) == Fraction(x)
    except ValueError:
        return False


def decode(t, bits):
    """The exact value of finite bits, as a Fraction."""
    ebits, mbits, explicit = FORMATS[t]
    bias = (1 << (ebits - 1)) - 1
    if t == "l":
        se, sig = bits
        sign, biased = se >> 15, se & 0x7FFF
        e = max(biased, 1) - bias - 63
        v = Fraction(sig) * Fraction(2) ** e
    else:
        sign = bits >> (ebits + mbits)
        biased = (bits >> mbits) & ((1 << ebits) - 1)
        sig = bits & ((1 << mbits) - 1)
        if biased:
            sig |= 1 << mbits
        v = Fraction(sig) * Fraction(2) ** (max(biased, 1) - bias - mbits)
    return -v if sign else v


def special(t):
    """Each type's edge encodings, by name: zeros, the smallest and largest
    subnormal and normal, the infinities, quiet and signaling NaNs of both
    signs with and without a payload -- and for long double the encodings
    the x87 refuses (pseudo-denormal, unnormal, pseudo-infinity, pseudo-NaN)."""
    if t == "l":
        top = 1 << 63
        v = {
            "+0": (0, 0), "-0": (0x8000, 0),
            "minsub": (0, 1), "maxsub": (0, top - 1), "-minsub": (0x8000, 1),
            "minnorm": (1, top), "-minnorm": (0x8001, top),
            "max": (0x7FFE, (1 << 64) - 1), "-max": (0xFFFE, (1 << 64) - 1),
            "inf": (0x7FFF, top), "-inf": (0xFFFF, top),
            "qnan": (0x7FFF, 0xC000_0000_0000_0000), "-qnan": (0xFFFF, 0xC000_0000_0000_0000),
            "qnan123": (0x7FFF, 0xC000_0000_0000_0123), "qnanmax": (0x7FFF, (1 << 64) - 1),
            "snan1": (0x7FFF, top | 1), "-snan1": (0xFFFF, top | 1),
            "snanhi": (0x7FFF, 0xA000_0000_0000_0000),
            "pseudodenorm": (0, top | 1), "-pseudodenorm": (0x8000, top),
            "unnormal": (0x3FFF, 0x4000_0000_0000_0000), "unnormal0": (0x4000, 0),
            "pseudoinf": (0x7FFF, 0), "pseudonan": (0x7FFF, 0x4000_0000_0000_0001),
        }
        return v
    ebits, mbits, _ = FORMATS[t]
    sign = 1 << (ebits + mbits)
    expmask = ((1 << ebits) - 1) << mbits
    quiet = 1 << (mbits - 1)
    return {
        "+0": 0, "-0": sign,
        "minsub": 1, "maxsub": (1 << mbits) - 1, "-minsub": sign | 1,
        "minnorm": 1 << mbits, "-minnorm": sign | (1 << mbits),
        "max": expmask - 1, "-max": sign | (expmask - 1),
        "inf": expmask, "-inf": sign | expmask,
        "qnan": expmask | quiet, "-qnan": sign | expmask | quiet,
        "qnan123": expmask | quiet | 0x123, "qnanmax": expmask | ((1 << mbits) - 1),
        "snan1": expmask | 1, "-snan1": sign | expmask | 1,
        "snanhi": expmask | (quiet >> 1),
    }


def values(t, xs):
    """Bits for each representable value in xs (Fractions, ints, or names of
    special encodings), in order, without repeats."""
    sp = special(t)
    out = []
    for x in xs:
        if isinstance(x, str):
            if x not in sp:
                continue
            b = sp[x]
        else:
            if not representable(t, x):
                continue
            b = encode(t, x)
        if b not in out:
            out.append(b)
    return out


F = Fraction
H = F(1, 2)
BASIC = [0, F(1, 4), H, F(3, 4), 1, F(3, 2), 2, F(5, 2), 3, F(7, 2), 10, F(1, 3), 100]
BASIC = BASIC + [-x for x in BASIC if x]
NAMED = ["+0", "-0", "minsub", "-minsub", "maxsub", "minnorm", "-minnorm", "max", "-max",
         "inf", "-inf", "qnan", "-qnan", "qnan123", "qnanmax", "snan1", "-snan1", "snanhi",
         "pseudodenorm", "-pseudodenorm", "unnormal", "unnormal0", "pseudoinf", "pseudonan"]


def boundary_values():
    """Integers and half-integers either side of the powers of two where a
    `fromfp` width runs out."""
    out = []
    for k in (1, 2, 3, 7, 8, 15, 16, 23, 24, 31, 32, 33, 52, 53, 62, 63, 64, 65):
        p = F(2) ** k
        for d in (-1, -H, 0, H, 1):
            out += [p + d, -(p + d)]
    return out


def hexv(t, b):
    if t == "l":
        return f"{b[0]:04x}:{b[1]:016x}"
    return f"{b:0{8 if t == 'f' else 16}x}"


# -- the C program -------------------------------------------------------------

CT = {"f": "float", "d": "double", "l": "long double"}
SUF = {"f": "f", "d": "", "l": "l"}

PRELUDE = r"""
#define _GNU_SOURCE
#define __STDC_WANT_IEC_60559_BFP_EXT__ 1
#define __STDC_WANT_IEC_60559_EXT__ 1
#include <errno.h>
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef union { long double v; struct { unsigned long long m; unsigned short se; } s; } U;
static float F(uint32_t b) { float x; memcpy(&x, &b, 4); return x; }
static double D(uint64_t b) { double x; memcpy(&x, &b, 8); return x; }
static long double L(unsigned se, unsigned long long m) { U u; memset(&u, 0, sizeof u); u.s.m = m; u.s.se = (unsigned short) se; return u.v; }
static void pf(float x) { uint32_t b; memcpy(&b, &x, 4); printf("%08x", b); }
static void pd(double x) { uint64_t b; memcpy(&b, &x, 8); printf("%016llx", (unsigned long long) b); }
static void pl(long double x) { U u; memset(&u, 0, sizeof u); u.v = x; printf("%04x:%016llx", u.s.se, u.s.m); }
static void begin(void) { feclearexcept(FE_ALL_EXCEPT); errno = 0; }
static void end(void) {
    int e = errno, f = fetestexcept(FE_ALL_EXCEPT);
    char s[6]; int n = 0;
    if (f & FE_INVALID) s[n++] = 'I';
    if (f & FE_DIVBYZERO) s[n++] = 'Z';
    if (f & FE_OVERFLOW) s[n++] = 'O';
    if (f & FE_UNDERFLOW) s[n++] = 'U';
    if (f & FE_INEXACT) s[n++] = 'X';
    if (!n) s[n++] = '-';
    s[n] = 0;
    printf(" %s %d\n", s, e);
}
static const int ROUNDS[5] = { FP_INT_UPWARD, FP_INT_DOWNWARD, FP_INT_TOWARDZERO,
                               FP_INT_TONEARESTFROMZERO, FP_INT_TONEAREST };
static void flags_errno(char *s, int *e) {
    int f = fetestexcept(FE_ALL_EXCEPT), n = 0;
    *e = errno;
    if (f & FE_INVALID) s[n++] = 'I';
    if (f & FE_DIVBYZERO) s[n++] = 'Z';
    if (f & FE_OVERFLOW) s[n++] = 'O';
    if (f & FE_UNDERFLOW) s[n++] = 'U';
    if (f & FE_INEXACT) s[n++] = 'X';
    if (!n) s[n++] = '-';
    s[n] = 0;
}
int main(void) {
"""


class Gen:
    def __init__(self):
        self.lines = []
        self.k = 0

    def arr(self, t, bits):
        """A volatile array of `t` values; returns (name, count, element expression)."""
        self.k += 1
        name = f"v{self.k}"
        if t == "l":
            body = ", ".join(f"{{0x{b[0]:04x}u, 0x{b[1]:016x}ull}}" for b in bits)
            self.lines.append(f"  static volatile const struct {{ unsigned se; unsigned long long m; }} {name}[] = {{{body}}};")
            return name, len(bits), lambda i: f"L({name}[{i}].se, {name}[{i}].m)"
        width = 8 if t == "f" else 16
        ctype = "uint32_t" if t == "f" else "uint64_t"
        body = ", ".join(f"0x{b:0{width}x}{'u' if t == 'f' else 'ull'}" for b in bits)
        self.lines.append(f"  static volatile const {ctype} {name}[] = {{{body}}};")
        conv = "F" if t == "f" else "D"
        return name, len(bits), lambda i: f"{conv}({name}[{i}])"

    def emit(self, s):
        self.lines.append(s)


def pr(t):
    return {"f": "pf", "d": "pd", "l": "pl"}[t]


def gen_unary(g, fn, t, xs, ret):
    """`ret fn(T)` for each x: ret is 'T' or 'long'."""
    name, n, el = g.arr(t, xs)
    c = CT[t]
    g.emit(f"  for (int i = 0; i < {n}; i++) {{ {c} x = {el('i')};")
    g.emit(f'    printf("{fn} {t} "); {pr(t)}(x); printf(" = ");')
    if ret == "T":
        g.emit(f"    begin(); {c} r = {fn}(x); {c} rr = r; {pr(t)}(rr); end(); }}")
    elif ret == "int":
        g.emit(f'    begin(); int r = {fn}(x); int rr = r; printf("%d", rr); end(); }}')
    else:
        g.emit(f'    begin(); long r = {fn}(x); long rr = r; printf("%ld", rr); end(); }}')


def gen_binary(g, fn, t, xs, ys):
    """`T fn(T, T)` for each pair."""
    nx, cx, ex = g.arr(t, xs)
    ny, cy, ey = g.arr(t, ys)
    c = CT[t]
    g.emit(f"  for (int i = 0; i < {cx}; i++) for (int j = 0; j < {cy}; j++) {{ {c} x = {ex('i')}, y = {ey('j')};")
    g.emit(f'    printf("{fn} {t} "); {pr(t)}(x); printf(" "); {pr(t)}(y); printf(" = ");')
    g.emit(f"    begin(); {c} r = {fn}(x, y); {c} rr = r; {pr(t)}(rr); end(); }}")


def gen_ptr_pair(g, fn, t, xs, ys):
    """`int fn(const T *, const T *)` for each pair."""
    nx, cx, ex = g.arr(t, xs)
    ny, cy, ey = g.arr(t, ys)
    c = CT[t]
    g.emit(f"  for (int i = 0; i < {cx}; i++) for (int j = 0; j < {cy}; j++) {{ {c} x = {ex('i')}, y = {ey('j')};")
    g.emit(f'    printf("{fn} {t} "); {pr(t)}(x); printf(" "); {pr(t)}(y); printf(" = ");')
    g.emit(f'    begin(); int r = {fn}(&x, &y); printf("%d", r); end(); }}')


def gen_canonicalize(g, t, xs):
    fn = "canonicalize" + SUF[t]
    name, n, el = g.arr(t, xs)
    c = CT[t]
    sentinel = {"f": "F(0x12345678u)", "d": "D(0x123456789abcdef0ull)", "l": "L(0x1234u, 0x123456789abcdef0ull)"}[t]
    g.emit(f"  for (int i = 0; i < {n}; i++) {{ {c} x = {el('i')}; {c} cx = {sentinel};")
    g.emit(f'    printf("{fn} {t} "); {pr(t)}(x); printf(" = ");')
    g.emit(f'    begin(); int r = {fn}(&cx, &x); printf("%d ", r); {pr(t)}(cx); end(); }}')


def gen_getpayload(g, t, xs):
    fn = "getpayload" + SUF[t]
    name, n, el = g.arr(t, xs)
    c = CT[t]
    g.emit(f"  for (int i = 0; i < {n}; i++) {{ {c} x = {el('i')};")
    g.emit(f'    printf("{fn} {t} "); {pr(t)}(x); printf(" = ");')
    g.emit(f"    begin(); {c} r = {fn}(&x); {c} rr = r; {pr(t)}(rr); end(); }}")


def gen_setpayload(g, fn, t, xs):
    name, n, el = g.arr(t, xs)
    c = CT[t]
    g.emit(f"  for (int i = 0; i < {n}; i++) {{ {c} p = {el('i')}; {c} x;")
    g.emit(f'    printf("{fn} {t} "); {pr(t)}(p); printf(" = ");')
    g.emit(f'    begin(); int r = {fn}(&x, p); printf("%d ", r); {pr(t)}(x); end(); }}')


def gen_fromfp(g, fn, t, cases):
    """`fn` and `fn`x, for cases (bits, [widths])."""
    c = CT[t]
    signed = not fn.startswith("u")
    fmt = "%jd" if signed else "%ju"
    rtype = "intmax_t" if signed else "uintmax_t"
    fnx = fn[:-1] + "x" + fn[-1] if fn.endswith(("f", "l")) and fn not in ("fromfp", "ufromfp") else fn + "x"
    for bits, widths in cases:
        name, n, el = g.arr(t, [bits])
        ws = ", ".join(f"{w}u" for w in widths)
        g.k += 1
        wn = f"w{g.k}"
        g.emit(f"  {{ static volatile const unsigned {wn}[] = {{{ws}}};")
        g.emit(f"    for (int j = 0; j < {len(widths)}; j++) {{ {c} x = {el('0')};")
        g.emit(f'      printf("{fn} {t} "); {pr(t)}(x); printf(" %u =", {wn}[j]);')
        for k, f in enumerate((fn, fnx)):
            if k:
                g.emit('      printf(" /");')
            g.emit("      for (int k = 0; k < 5; k++) { char s[6]; int e;")
            g.emit(f"        begin(); {rtype} r = {f}(x, ROUNDS[k], {wn}[j]); {rtype} rr = r; flags_errno(s, &e);")
            g.emit(f'        printf(" {fmt}:%s:%d", rr, s, e); }}')
        g.emit('      printf("\\n"); } }')


def fromfp_widths(t, bits):
    """The widths worth asking a value about: none, one bit, the width its
    integer part needs and one either side, and past intmax_t."""
    ws = {0, 1, 2, 63, 64, 65, 4294967295}
    try:
        v = decode(t, bits)
    except Exception:
        return sorted(ws)
    if abs(v) >= F(2) ** 66:
        return [0, 1, 64, 4294967295]
    n = abs(int(v)).bit_length() + 1
    for w in (n - 1, n, n + 1, n + 2):
        if w > 0:
            ws.add(w)
    return sorted(ws)


def build():
    g = Gen()
    for t in "fdl":
        s = SUF[t]
        edge = values(t, NAMED + BASIC + [F(2) ** 23, F(2) ** 52, F(2) ** 63, F(2) ** 64 - 1,
                                          F(2) ** -126, F(2) ** -1022, F(2) ** -16382])
        for fn in ("nextup", "nextdown"):
            gen_unary(g, fn + s, t, edge, "T")
        gen_unary(g, "llogb" + s, t, edge, "long")
        if t == "l":
            # ilogbl and logbl, which llogbl stands on, over the same edges --
            # glibc's are the x87's fxtract, which refuses what the unit
            # refuses.
            gen_unary(g, "ilogbl", t, edge, "int")
            gen_unary(g, "logbl", t, edge, "T")
        gen_canonicalize(g, t, edge)
        gen_getpayload(g, t, edge)
        payloads = values(t, [0, 1, 2, F(3, 2), 0x123, F(2) ** 21, F(2) ** 22 - 1, F(2) ** 22,
                              F(2) ** 50, F(2) ** 51 - 1, F(2) ** 51, F(2) ** 61, F(2) ** 62 - 1,
                              F(2) ** 62, -1, "-0", "inf", "qnan", "snan1", F(1, 2), "minsub"])
        for fn in ("setpayload", "setpayloadsig"):
            gen_setpayload(g, fn + s, t, payloads)
        order = values(t, ["-qnan", "-snan1", "-inf", "-max", -1, "-minnorm", "-minsub", "-0",
                           "+0", "minsub", "minnorm", 1, "max", "inf", "snan1", "snanhi", "qnan",
                           "qnan123", "qnanmax", "pseudodenorm", "unnormal", "pseudonan"])
        for fn in ("totalorder", "totalordermag"):
            gen_ptr_pair(g, fn + s, t, order, order)
        pairs = values(t, ["+0", "-0", 1, -1, 2, -2, F(1, 2), "-minsub", "max", "inf", "-inf",
                           "qnan", "-qnan", "snan1", "-snan1"])
        for fn in ("fmaxmag", "fminmag", "fmaximum", "fminimum", "fmaximum_num", "fminimum_num",
                   "fmaximum_mag", "fminimum_mag", "fmaximum_mag_num", "fminimum_mag_num"):
            gen_binary(g, fn + s, t, pairs, pairs)
        conv = values(t, NAMED + BASIC + boundary_values() + [F(2) ** 70, -F(2) ** 70])
        for fn in ("fromfp", "ufromfp"):
            gen_fromfp(g, fn + s, t, [(b, fromfp_widths(t, b)) for b in conv])
    scal = values("l", NAMED + [0, 1, -1, 2, F(1, 2), F(3, 2), -F(3, 2), 10, -10, 16384, -16384,
                                17000, -17000, 100000, -100000, F(2) ** 63])
    gen_binary(g, "scalbl", "l", scal, scal)
    return PRELUDE + "\n".join(g.lines) + "\n  return 0;\n}\n"


def main():
    prog = build()
    with workdir() as tmp:
        (Path(tmp) / "c23.c").write_text(prog, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -frounding-math -fsignaling-nans -fno-builtin -w "
                f"-o c23 c23.c -lm && ./c23")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-3000:]}")
    header = ("# glibc 2.39's exact C23 and GNU <math.h> functions under WSL\n"
              "# (posix/tools/oracle/c23math_harness.py): <function> <type> <inputs> =\n"
              "# <outputs> <flags> <errno>; values as bits in hex, flags from IZOUX or -.\n"
              "# fromfp lines: <result>:<flags>:<errno> for each of the five directions.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"{len(r.stdout.splitlines())} lines -> {OUT}")


if __name__ == "__main__":
    main()
