"""glibc 2.39's `long double complex` functions, as the oracle for
posix/src/complexl.rs.

    python posix/tools/oracle/complexl_harness.py   # writes posix/src/complexl_oracle.txt

The `long double` twin of `complex_harness.py`: for every case the C program
sets errno to 0, calls glibc's function, and prints one line:

    <name> <re> <im> [<re2> <im2>] = <out...> <errno>

A value is x87's 80-bit format as it holds it, `SSSS:MMMMMMMMMMMMMMMM` (the
sign-and-exponent word, then the 64-bit significand); a complex result is
two, a real one one. Built with -fno-builtin and from arrays, so gcc can
neither fold a call nor substitute its own answer.

The inputs: every pair of a set of special parts -- zeros, units,
infinities, NaN, the extremes of the 80-bit range, and the thresholds the
implementations branch at, among them the magnitudes past a `double`'s --
random points spread over every magnitude whose square does not overflow,
and points crowding where the functions are hard: the branch points of the
inverse functions, and |z| = 1 for clogl.
"""

import random
import sys
from fractions import Fraction
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402
from mathl_harness import enc  # noqa: E402  (exact rounding into the 80-bit format)

OUT = POSIX_SRC / "complexl_oracle.txt"

UNARY_C = ["csqrtl", "cexpl", "clogl", "csinl", "ccosl", "ctanl", "csinhl", "ccoshl", "ctanhl",
           "casinl", "cacosl", "catanl", "casinhl", "cacoshl", "catanhl", "cprojl", "conjl"]
UNARY_R = ["cabsl", "cargl", "creall", "cimagl"]
INVERSE = {"casinl", "cacosl", "catanl", "casinhl", "cacoshl", "catanhl"}

INF = (0x7FFF, 1 << 63)
NAN = (0x7FFF, 0xC000_0000_0000_0000)


def neg(v):
    return (v[0] ^ 0x8000, v[1])


def F(x):
    """A value as the 80-bit format holds it, from a Fraction or int."""
    return enc(Fraction(x))


PI = Fraction(3141592653589793238462643383279502884, 10 ** 36)
SPECIAL = [
    (0, 0), (0x8000, 0), F(1), neg(F(1)), F(Fraction(1, 2)), neg(F(2)), F(3),
    (0x0000, 1), (0x8000, 1),                      # the least subnormals
    (0x0001, 1 << 63),                             # LDBL_MIN
    (0x7FFE, (1 << 64) - 1), (0xFFFE, (1 << 64) - 1),  # +-LDBL_MAX
    INF, neg(INF), NAN,
    F(PI / 2), F(PI), F(22), neg(F(Fraction(113565, 10))), F(11357), F(22712),
    F(1 + Fraction(1, 2 ** 63)), F(1 - Fraction(1, 2 ** 64)), F(Fraction(1, 10 ** 12)),
    F(10 ** 20), F(Fraction(7, 10)), F(Fraction(10) ** 400), F(Fraction(1, 10 ** 400)),
    F(Fraction(10) ** 2466), F(Fraction(1, 10) ** 2466),
]


def rand_value(rng, lo, hi):
    """A random 80-bit value of magnitude 10^lo to 10^hi, all 64 bits used."""
    mag = Fraction(10) ** rng.randint(lo, hi) * Fraction(rng.getrandbits(64) | (1 << 63), 1 << 63)
    v = F(mag)
    return neg(v) if rng.random() < 0.5 else v


def random_points(rng, n_log, n_small):
    """`n_log` points with parts spread log-uniformly over the magnitudes
    whose squares do not overflow, and `n_small` in [-3, 3]^2."""
    pts = []
    for _ in range(n_log):
        pts.append((rand_value(rng, -2400, 2400), rand_value(rng, -2400, 2400)))
    for _ in range(n_small):
        pts.append((F(Fraction(rng.randint(-3 * 2 ** 40, 3 * 2 ** 40), 2 ** 40)),
                    F(Fraction(rng.randint(-3 * 2 ** 40, 3 * 2 ** 40), 2 ** 40))))
    return pts


def near(rng, x, lo):
    """`x` plus a random offset of magnitude 10^lo to 10^-1."""
    d = Fraction(10) ** rng.randint(lo, -1) * Fraction(rng.randint(1, 10 ** 6), 10 ** 6)
    return x + (d if rng.random() < 0.5 else -d)


def hard_points(rng, name):
    """Where each function is hard."""
    pts = []
    lo = -19
    if name in INVERSE:
        for _ in range(60):
            for (a, b) in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                pts.append((F(near(rng, Fraction(a), lo)), F(near(rng, Fraction(b), lo))))
        for _ in range(20):
            t = 1 + Fraction(10) ** rng.randint(0, 3) * Fraction(rng.randint(1, 1000), 1000)
            e = Fraction(10) ** rng.randint(lo, -1)
            pts += [(F(t), F(e)), (F(t), F(-e)), (F(e), F(t)), (F(-e), F(-t))]
    if name == "clogl":
        # |z| just off 1: (r cos t, r sin t) through rational points on the
        # unit circle, (1 - s^2, 2s) / (1 + s^2), scaled by r.
        for _ in range(120):
            s = Fraction(rng.randint(-10 ** 6, 10 ** 6), 10 ** 6)
            r = near(rng, Fraction(1), lo)
            pts.append((F(r * (1 - s * s) / (1 + s * s)), F(r * 2 * s / (1 + s * s))))
    if name in ("csinl", "ccosl", "ctanl", "csinhl", "ccoshl", "ctanhl", "cexpl"):
        for x in (Fraction(229, 10), Fraction(231, 10), Fraction(11355), Fraction(113565, 10),
                  Fraction(11357), Fraction(22710), Fraction(22712), Fraction(-113565, 10)):
            for y in (Fraction(1, 10), Fraction(1), PI / 2, PI, Fraction(3), Fraction(1, 10 ** 10),
                      Fraction(100)):
                pts += [(F(x), F(y)), (F(y), F(x))]
    return pts


def cases():
    rng = random.Random(2027)
    grid = [(a, b) for a in SPECIAL for b in SPECIAL]
    table = []
    for name in UNARY_C + UNARY_R:
        pts = list(grid)
        pts += random_points(rng, 120, 80)
        pts += hard_points(rng, name)
        table.append((name, pts))
    small = [(0, 0), (0x8000, 0), F(1), neg(F(1)), F(Fraction(1, 2)), F(2), INF, NAN,
             F(Fraction(1, 10 ** 5))]
    bases = [(a, b) for a in small for b in small]
    exps = [((0, 0), (0, 0)), (F(1), (0, 0)), (F(2), (0, 0)), (F(Fraction(1, 2)), (0, 0)),
            (neg(F(1)), (0, 0)), ((0, 0), F(1)), (F(Fraction(3, 2)), neg(F(Fraction(1, 2)))),
            (INF, (0, 0)), (NAN, (0, 0)), (F(3), F(2))]
    pairs = [(b, e) for b in bases for e in exps]
    for _ in range(400):
        b = tuple(F(Fraction(rng.randint(-4 * 2 ** 40, 4 * 2 ** 40), 2 ** 40)) for _ in range(2))
        e = tuple(F(Fraction(rng.randint(-4 * 2 ** 40, 4 * 2 ** 40), 2 ** 40)) for _ in range(2))
        pairs.append((b, e))
    table.append(("cpowl", pairs))
    return table


def gen_c():
    lines = [
        "#define _GNU_SOURCE",
        "#include <complex.h>",
        "#include <errno.h>",
        "#include <stdio.h>",
        "#include <string.h>",
        "typedef union { long double v; struct { unsigned long long m; unsigned short se; } s; } U;",
        "static long double L(unsigned se, unsigned long long m) "
        "{ U u; memset(&u, 0, sizeof u); u.s.m = m; u.s.se = se; return u.v; }",
        "static void P(long double v) { U u; memset(&u, 0, sizeof u); u.v = v; "
        "printf(\"%04x:%016llx\", u.s.se, u.s.m); }",
        "int main(void) {",
    ]
    k = 0
    for name, pts in cases():
        k += 1
        a = f"a{k}"
        if name == "cpowl":
            flat = ", ".join(f"{{0x{v[0]:04x}, 0x{v[1]:016x}ULL}}"
                             for b, e in pts for v in (b[0], b[1], e[0], e[1]))
            lines.append(f"  static const struct {{ unsigned se; unsigned long long m; }} {a}[] = {{{flat}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / sizeof {a}[0]; i += 4) {{")
            lines.append(f"    long double complex x = __builtin_complex(L({a}[i].se, {a}[i].m), L({a}[i+1].se, {a}[i+1].m));")
            lines.append(f"    long double complex y = __builtin_complex(L({a}[i+2].se, {a}[i+2].m), L({a}[i+3].se, {a}[i+3].m));")
            lines.append(f"    errno = 0; long double complex r = {name}(x, y); int e = errno;")
            lines.append(f'    printf("{name} "); for (int j = 0; j < 4; j++) {{ P(L({a}[i+j].se, {a}[i+j].m)); printf(" "); }}')
            lines.append('    printf("= "); P(__real__ r); printf(" "); P(__imag__ r); printf(" %d\\n", e); }')
            continue
        flat = ", ".join(f"{{0x{v[0]:04x}, 0x{v[1]:016x}ULL}}" for re, im in pts for v in (re, im))
        lines.append(f"  static const struct {{ unsigned se; unsigned long long m; }} {a}[] = {{{flat}}};")
        lines.append(f"  for (unsigned i = 0; i < sizeof {a} / sizeof {a}[0]; i += 2) {{")
        lines.append(f"    long double complex z = __builtin_complex(L({a}[i].se, {a}[i].m), L({a}[i+1].se, {a}[i+1].m));")
        lines.append(f'    printf("{name} "); P(__real__ z); printf(" "); P(__imag__ z); printf(" = ");')
        if name in UNARY_R:
            lines.append(f"    errno = 0; long double r = {name}(z); int e = errno;")
            lines.append('    P(r); printf(" %d\\n", e); }')
        else:
            lines.append(f"    errno = 0; long double complex r = {name}(z); int e = errno;")
            lines.append('    P(__real__ r); printf(" "); P(__imag__ r); printf(" %d\\n", e); }')
    lines += ["  return 0;", "}"]
    return "\n".join(lines) + "\n"


def main():
    with workdir() as tmp:
        (Path(tmp) / "complexl_oracle.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -w -o complexl_oracle complexl_oracle.c -lm "
                f"&& ./complexl_oracle > '{wsl_path(OUT)}'")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines, {OUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
