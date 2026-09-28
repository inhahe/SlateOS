"""glibc 2.39's `long double` <math.h> as the oracle for posix/src/mathl.rs.

    python posix/tools/oracle/mathl_harness.py

Generates a C program, builds it with gcc under WSL (which must have glibc
2.39 -- Ubuntu 24.04's), runs it there, and writes posix/src/mathl_oracle.txt,
which the tests in posix/src/mathl.rs replay. The cases are drawn from a
seeded generator, so a rerun on the same glibc writes the same file.

One call a line:

    <name> <in...> = <out...> <errno>

A long double is `SSSS:MMMMMMMMMMMMMMMM` -- its sign-and-exponent word and
its significand, in hex, exactly as the 80-bit format holds them -- so the
Rust side rebuilds the value bit for bit; an int is decimal; a double or
float is its bits in hex. Built with -fno-builtin and from arrays, so gcc can
neither fold a call nor substitute its own answer.
"""

import math
import random
import struct
import subprocess
import sys
import tempfile
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent.parent / "src" / "mathl_oracle.txt"


def enc(x):
    """A value -> (sign_exp, significand) of the 80-bit format, rounded to
    nearest from its exact rational value (floats, ints, Fractions)."""
    if isinstance(x, tuple):
        return x
    if isinstance(x, float):
        if math.isnan(x):
            return (0x7FFF, 0xC000_0000_0000_0000)
        if math.isinf(x):
            return (0xFFFF if x < 0 else 0x7FFF, 1 << 63)
        if x == 0:
            return (0x8000 if math.copysign(1, x) < 0 else 0, 0)
        x = Fraction(x)
    sign = 0x8000 if x < 0 else 0
    a = abs(Fraction(x))
    e = a.numerator.bit_length() - a.denominator.bit_length()
    # normalise 2^63 <= a * 2^(63 - e) < 2^64
    while a * Fraction(2) ** (63 - e) >= 2 ** 64:
        e += 1
    while a * Fraction(2) ** (63 - e) < 2 ** 63:
        e -= 1
    be = e + 16383
    if be <= 0:  # subnormal: scale by 2^(16382 + 63)
        m = a * Fraction(2) ** (16382 + 63)
        q, r = divmod(m.numerator, m.denominator)
        if 2 * r > m.denominator or (2 * r == m.denominator and q & 1):
            q += 1
        return (sign | (1 if q >> 63 else 0), q)
    m = a * Fraction(2) ** (63 - e)
    q, r = divmod(m.numerator, m.denominator)
    if 2 * r > m.denominator or (2 * r == m.denominator and q & 1):
        q += 1
    if q == 2 ** 64:
        q >>= 1
        be += 1
    if be >= 0x7FFF:
        return (sign | 0x7FFF, 1 << 63)
    return (sign | be, q)


def s(v):
    se, m = enc(v)
    return f"0x{se:04x}u, 0x{m:016x}ull"


INF = float("inf")
NAN = float("nan")
MAXL = (0x7FFE, 0xFFFF_FFFF_FFFF_FFFF)
MINNORM = (0x0001, 1 << 63)
MINSUB = (0x0000, 1)
MAXSUB = (0x0000, (1 << 63) - 1)
ONE_UP = (0x3FFF, (1 << 63) | 1)
ONE_DOWN = (0x3FFE, 0xFFFF_FFFF_FFFF_FFFF)

SPECIAL = [0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 3.0, INF, -INF, NAN, MINSUB,
           MAXSUB, MINNORM, MAXL, ONE_UP, ONE_DOWN, math.pi, math.pi / 2, 22.0, 1e10, -1e10,
           1e-10, 0.3, -0.7, 100.5, -100.5, 11356.0, 11357.0, -11355.0, -11400.0, -11450.0,
           16384.0, -16384.0, -16400.0, (0x4CF7, 0x9F3D_2A5B_1234_5678),
           (0x33FF, 0xABCD_EF01_2345_6789)]


def random_values(rng, n, lo=-30, hi=30):
    out = []
    for _ in range(n):
        m = rng.getrandbits(63) | (1 << 63)
        e = rng.randint(lo, hi)
        se = (0x8000 if rng.random() < 0.5 else 0) | (16383 + e)
        out.append((se, m))
    return out


def small_values(rng, n, lo=-4.0, hi=4.0):
    return [rng.uniform(lo, hi) for _ in range(n)]


def near_integers(rng, n):
    out = []
    for _ in range(n):
        k = rng.randint(-1000, 1000)
        out += [k + 0.5, k - 0.5, Fraction(k) + Fraction(1, 2 ** 40), Fraction(k) - Fraction(1, 2 ** 62)]
    return out


def cases():
    rng = random.Random(1998)
    u = SPECIAL + random_values(rng, 120) + small_values(rng, 80)
    wide = u + random_values(rng, 60, -16000, 16000)
    unit = [rng.uniform(-1, 1) for _ in range(120)] + [Fraction(rng.randint(-2**63, 2**63), 2**63) for _ in range(40)]
    posi = [abs(Fraction(rng.uniform(0, 1))) * 10 ** rng.randint(-8, 8) for _ in range(120)]
    near1 = [Fraction(1) + Fraction(rng.randint(-2**40, 2**40), 2**64) for _ in range(60)]
    rounding = near_integers(rng, 40) + [rng.uniform(-1e6, 1e6) for _ in range(40)]
    expo = [rng.uniform(-11500, 11500) for _ in range(80)] + [rng.uniform(-50, 50) for _ in range(80)] \
        + [rng.uniform(-1, 1) * 10 ** rng.randint(-20, 0) for _ in range(40)]
    trig = [rng.uniform(-10, 10) for _ in range(100)] + [rng.uniform(-1e6, 1e6) for _ in range(40)] \
        + [(0x4000 + rng.randint(0, 60), rng.getrandbits(63) | (1 << 63)) for _ in range(40)]

    tab = []
    for name in ("fabsl", "sqrtl", "rintl", "nearbyintl", "floorl", "ceill", "truncl", "roundl", "roundevenl",
                 "logbl", "cbrtl"):
        tab.append((name, "l_l", wide + rounding))
    for name in ("lrintl", "llrintl", "lroundl", "llroundl", "ilogbl"):
        tab.append((name, "i_l", wide + rounding))
    for name in ("logl", "log2l", "log10l"):
        tab.append((name, "l_l", wide + posi + near1))
    tab.append(("log1pl", "l_l", wide + unit + posi + [Fraction(-1) + Fraction(1, 2 ** k) for k in range(1, 60, 3)]))
    for name in ("expl", "exp2l", "expm1l", "exp10l", "sinhl", "coshl", "tanhl"):
        tab.append((name, "l_l", u + expo))
    for name in ("atanl", "asinhl"):
        tab.append((name, "l_l", wide + unit))
    for name in ("asinl", "acosl", "atanhl"):
        tab.append((name, "l_l", u + unit + near1))
    tab.append(("acoshl", "l_l", wide + near1 + posi))
    for name in ("sinl", "cosl", "tanl"):
        tab.append((name, "l_l", u + trig + unit))
    tab.append(("sincosl", "v_lpp", u + trig))
    for name in ("erfl", "erfcl", "lgammal", "tgammal"):
        tab.append((name, "l_l", u + small_values(rng, 120, -30, 30) + [rng.uniform(-200, 1800) for _ in range(40)]))
    tab.append(("frexpl", "l_lpi", wide))
    tab.append(("modfl", "l_lpl", wide + rounding))
    for name in ("ldexpl", "scalbnl"):
        tab.append((name, "l_li", [(v, n) for v in u[:40] for n in (0, 1, -1, 100, -100, 16383, -16383, 20000, -20000, -16445)]))
    tab.append(("scalblnl", "l_ln", [(v, n) for v in u[:30] for n in (0, 5, -5, 1 << 40, -(1 << 40))]))
    pairs = [(a, b) for a in SPECIAL[:20] for b in SPECIAL[:20]]
    rand_pairs = [(x, y) for x, y in zip(random_values(rng, 150), random_values(rng, 150))]
    small_pairs = [(rng.uniform(-10, 10), rng.uniform(-10, 10)) for _ in range(150)]
    for name in ("fmodl", "remainderl", "atan2l", "hypotl", "fdiml", "fmaxl", "fminl", "copysignl",
                 "nextafterl", "nexttowardl", "powl"):
        tab.append((name, "l_ll", pairs + rand_pairs + small_pairs))
    tab.append(("powl", "l_ll", [(rng.uniform(0, 10), rng.uniform(-50, 50)) for _ in range(200)]
                + [(-2.0, float(k)) for k in range(-5, 6)] + [(rng.uniform(0.9, 1.1), rng.uniform(-1e4, 1e4)) for _ in range(60)]))
    tab.append(("remquol", "l_llpi", pairs + small_pairs))
    tab.append(("fmal", "l_lll", [(a, b, c) for a in SPECIAL[:9] for b in SPECIAL[:9] for c in SPECIAL[:9]]
                + [(x, y, z) for x, y, z in zip(random_values(rng, 200), random_values(rng, 200), random_values(rng, 200))]))
    tab.append(("nexttoward", "d_dl", [(float(a) if isinstance(a, float) else 1.5, b) for a in SPECIAL[:12] for b in SPECIAL[:12]]))
    tab.append(("nexttowardf", "f_fl", [(float(a) if isinstance(a, float) else 1.5, b) for a in SPECIAL[:12] for b in SPECIAL[:12]]))
    return tab


def gen_c():
    L = [
        "#define _GNU_SOURCE",
        "#include <math.h>",
        "#include <errno.h>",
        "#include <stdio.h>",
        "#include <stdint.h>",
        "#include <string.h>",
        "typedef union { long double v; struct { unsigned long long m; unsigned short se; } s; } U;",
        "static long double L(unsigned se, unsigned long long m) { U u; memset(&u, 0, sizeof u); u.s.m = m; u.s.se = se; return u.v; }",
        "static void P(long double v) { U u; memset(&u, 0, sizeof u); u.v = v; printf(\" %04x:%016llx\", u.s.se, u.s.m); }",
        "static void PD(double d) { unsigned long long b; memcpy(&b, &d, 8); printf(\" %016llx\", b); }",
        "static void PF(float f) { unsigned b; memcpy(&b, &f, 4); printf(\" %08x\", b); }",
        "int main(void) {",
    ]
    k = 0
    for name, sig, vals in cases():
        k += 1
        a = f"a{k}"
        if sig in ("l_l", "i_l", "v_lpp", "l_lpi", "l_lpl"):
            flat = ", ".join(s(v) for v in vals)
            L.append(f"  static const unsigned long long {a}[] = {{{flat}}};")
            L.append(f"  for (unsigned i = 0; i < sizeof {a} / 8; i += 2) {{ long double x = L({a}[i], {a}[i+1]); errno = 0;")
            L.append(f'    printf("{name}"); P(x); printf(" =");')
            if sig == "l_l":
                L.append(f"    long double r = {name}(x); int e = errno; P(r); printf(\" %d\\n\", e); }}")
            elif sig == "i_l":
                L.append(f"    long long r = {name}(x); int e = errno; printf(\" %lld %d\\n\", r, e); }}")
            elif sig == "v_lpp":
                L.append(f"    long double sn = 0, cs = 0; {name}(x, &sn, &cs); int e = errno; P(sn); P(cs); printf(\" %d\\n\", e); }}")
            elif sig == "l_lpi":
                L.append(f"    int o = 12345; long double r = {name}(x, &o); int e = errno; P(r); printf(\" %d %d\\n\", o, e); }}")
            elif sig == "l_lpl":
                L.append(f"    long double o = 0; long double r = {name}(x, &o); int e = errno; P(r); P(o); printf(\" %d\\n\", e); }}")
        elif sig in ("l_ll", "l_llpi"):
            flat = ", ".join(f"{s(x)}, {s(y)}" for x, y in vals)
            L.append(f"  static const unsigned long long {a}[] = {{{flat}}};")
            L.append(f"  for (unsigned i = 0; i < sizeof {a} / 8; i += 4) {{ long double x = L({a}[i], {a}[i+1]), y = L({a}[i+2], {a}[i+3]); errno = 0;")
            L.append(f'    printf("{name}"); P(x); P(y); printf(" =");')
            if sig == "l_ll":
                L.append(f"    long double r = {name}(x, y); int e = errno; P(r); printf(\" %d\\n\", e); }}")
            else:
                L.append(f"    int q = 12345; long double r = {name}(x, y, &q); int e = errno; P(r); printf(\" %d %d\\n\", q, e); }}")
        elif sig == "l_lll":
            flat = ", ".join(f"{s(x)}, {s(y)}, {s(z)}" for x, y, z in vals)
            L.append(f"  static const unsigned long long {a}[] = {{{flat}}};")
            L.append(f"  for (unsigned i = 0; i < sizeof {a} / 8; i += 6) {{ long double x = L({a}[i], {a}[i+1]), y = L({a}[i+2], {a}[i+3]), z = L({a}[i+4], {a}[i+5]); errno = 0;")
            L.append(f'    printf("{name}"); P(x); P(y); P(z); printf(" =");')
            L.append(f"    long double r = {name}(x, y, z); int e = errno; P(r); printf(\" %d\\n\", e); }}")
        elif sig in ("l_li", "l_ln"):
            flat = ", ".join(f"{s(v)}, {n & 0xFFFF_FFFF_FFFF_FFFF}ull" for v, n in vals)
            ty = "int" if sig == "l_li" else "long"
            L.append(f"  static const unsigned long long {a}[] = {{{flat}}};")
            L.append(f"  for (unsigned i = 0; i < sizeof {a} / 8; i += 3) {{ long double x = L({a}[i], {a}[i+1]); {ty} n = ({ty})(long long){a}[i+2]; errno = 0;")
            L.append(f'    printf("{name}"); P(x); printf(" %lld =", (long long)n);')
            L.append(f"    long double r = {name}(x, n); int e = errno; P(r); printf(\" %d\\n\", e); }}")
        elif sig in ("d_dl", "f_fl"):
            flat = ", ".join(f"{s(y)}" for _, y in vals)
            xs = ", ".join(f"0x{struct.unpack('<Q', struct.pack('<d', float(x)))[0]:016x}ull" for x, _ in vals)
            cty = "double" if sig == "d_dl" else "float"
            pr = "PD" if sig == "d_dl" else "PF"
            L.append(f"  static const unsigned long long {a}[] = {{{flat}}};")
            L.append(f"  static const unsigned long long {a}x[] = {{{xs}}};")
            L.append(f"  for (unsigned i = 0; i < sizeof {a}x / 8; i++) {{ double xd; memcpy(&xd, &{a}x[i], 8); {cty} x = ({cty})xd; long double y = L({a}[2*i], {a}[2*i+1]); errno = 0;")
            L.append(f'    printf("{name}"); {pr}(x); P(y); printf(" =");')
            L.append(f"    {cty} r = {name}(x, y); int e = errno; {pr}(r); printf(\" %d\\n\", e); }}")
    L += ["  return 0;", "}"]
    return "\n".join(L) + "\n"


def to_wsl(p):
    t = str(p).replace("\\", "/")
    return "/mnt/" + t[0].lower() + t[2:]


def main():
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "mathl_oracle.c"
        src.write_text(gen_c(), encoding="utf-8", newline="\n")
        exe = Path(tmp) / "mathl_oracle"
        cmd = (f"gcc -O0 -fno-builtin -o '{to_wsl(exe)}' '{to_wsl(src)}' -lm && "
               f"'{to_wsl(exe)}' > '{to_wsl(OUT)}'")
        r = subprocess.run(["wsl", "bash", "-lc", cmd], capture_output=True, text=True)
    print(r.stdout[-2000:], r.stderr[-3000:])
    if r.returncode != 0:
        sys.exit("oracle build or run failed")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines")


if __name__ == "__main__":
    main()
