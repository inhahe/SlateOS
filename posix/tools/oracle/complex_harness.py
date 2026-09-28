"""glibc 2.39's <complex.h> as the oracle for posix/src/complex.rs.

    python posix/tools/oracle/complex_harness.py   # writes posix/src/complex_oracle.txt

For every case the C program sets errno to 0, calls glibc's function, and
prints one line:

    <name> <re> <im> [<re2> <im2>] = <out...> <errno>

Floating-point values are their bits in hex (16 digits for a double, 8 for a
float); a complex result is two of them, a real one one. Built with
-fno-builtin and from arrays, so gcc can neither fold a call nor substitute its
own answer for glibc's.

The inputs: every pair of a set of special parts (zeros of both signs, the
units, infinities, NaN, the smallest and largest numbers, the thresholds the
implementations branch at), random points spread over every magnitude, and
points crowding the places the functions are hard -- the branch points of the
inverse functions and |z| = 1 for clog.
"""

import math
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "complex_oracle.txt"

UNARY_C = ["csqrt", "cexp", "clog", "csin", "ccos", "ctan", "csinh", "ccosh", "ctanh",
           "casin", "cacos", "catan", "casinh", "cacosh", "catanh", "cproj", "conj"]
UNARY_R = ["cabs", "carg", "creal", "cimag"]
INVERSE = {"casin", "cacos", "catan", "casinh", "cacosh", "catanh"}


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def bits64(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def bits32(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


NAN = float("nan")
INF = float("inf")

SPECIAL_D = [0.0, -0.0, 1.0, -1.0, 0.5, -2.0, 3.0, 1e-300, -5e-324, 2.2250738585072014e-308,
             1e300, -1.7976931348623157e308, INF, -INF, NAN, math.pi / 2, math.pi, 22.0,
             -710.5, 1000.0, 1455.5, 1 + 2 ** -52, 1 - 2 ** -53, 1e-9, 1e16, 0.7]

SPECIAL_F = [0.0, -0.0, 1.0, -1.0, 0.5, -2.0, 3.0, 1e-38, -1.401298464324817e-45,
             1.1754943508222875e-38, 1e38, -3.4028234663852886e38, INF, -INF, NAN,
             f32(math.pi / 2), f32(math.pi), 9.0, -88.9, 100.0, 192.0, 1 + 2 ** -23,
             1 - 2 ** -24, 1e-5, 1e8, 0.7]


def random_points(rng, single, n_log, n_small):
    """`n_log` points with parts spread log-uniformly over the magnitudes a
    double (or float) can take without overflowing its squares, and `n_small`
    in [-3, 3]^2, where the functions do most of their work."""
    lo, hi = (-30, 30) if single else (-150, 150)
    pts = []
    for _ in range(n_log):
        re = rng.choice((-1, 1)) * 10 ** rng.uniform(lo, hi)
        im = rng.choice((-1, 1)) * 10 ** rng.uniform(lo, hi)
        pts.append((re, im))
    for _ in range(n_small):
        pts.append((rng.uniform(-3, 3), rng.uniform(-3, 3)))
    return pts


def hard_points(rng, name, single):
    """Where each function is hard."""
    pts = []
    lo = -7 if single else -16
    if name in INVERSE:
        # Near the branch points +-1 and +-i, from every side.
        for _ in range(60):
            d1 = rng.choice((-1, 1)) * 10 ** rng.uniform(lo, -1)
            d2 = rng.choice((-1, 1)) * 10 ** rng.uniform(lo, -1)
            for (a, b) in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                pts.append((a + d1, b + d2))
        # Along the cuts, just off them.
        for _ in range(20):
            t = 10 ** rng.uniform(0, 3)
            e = 10 ** rng.uniform(lo, -1)
            pts += [(1 + t, e), (1 + t, -e), (e, 1 + t), (-e, -1 - t)]
    if name == "clog":
        # |z| just off 1.
        for _ in range(120):
            t = rng.uniform(-math.pi, math.pi)
            r = 1 + rng.choice((-1, 1)) * 10 ** rng.uniform(lo, -1)
            pts.append((r * math.cos(t), r * math.sin(t)))
    if name in ("csin", "ccos", "ctan", "csinh", "ccosh", "ctanh", "cexp"):
        # Near the scaling thresholds and zeros of the circular parts.
        for x in (21.9, 22.1, 709.0, 709.9, 710.1, 1454.0, 1456.0, -709.9, -1454.0) if not single \
                else (8.9, 9.1, 10.9, 11.1, 88.5, 88.9, 192.5, -88.9):
            for y in (0.1, 1.0, math.pi / 2, math.pi, 3.0, 1e-10, 100.0):
                pts += [(x, y), (y, x)]
    return pts


def cases(single):
    rng = random.Random(2039 if single else 2026)
    special = SPECIAL_F if single else SPECIAL_D
    grid = [(a, b) for a in special for b in special]
    table = []
    for name in UNARY_C + UNARY_R:
        pts = list(grid)
        pts += random_points(rng, single, 120, 80)
        pts += hard_points(rng, name, single)
        if single:
            pts = [(f32(a), f32(b)) for a, b in pts]
        table.append((name + ("f" if single else ""), pts))
    # cpow: a smaller grid of bases and exponents, and random pairs.
    small = [0.0, -0.0, 1.0, -1.0, 0.5, 2.0, INF, NAN, 1e-5]
    bases = [(a, b) for a in small for b in small]
    exps = [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (0.5, 0.0), (-1.0, 0.0), (0.0, 1.0),
            (1.5, -0.5), (INF, 0.0), (NAN, 0.0), (3.0, 2.0)]
    pairs = [(b, e) for b in bases for e in exps]
    for _ in range(400):
        b = (rng.uniform(-4, 4), rng.uniform(-4, 4))
        e = (rng.uniform(-4, 4), rng.uniform(-4, 4))
        pairs.append((b, e))
    if single:
        pairs = [((f32(b[0]), f32(b[1])), (f32(e[0]), f32(e[1]))) for b, e in pairs]
    table.append(("cpow" + ("f" if single else ""), pairs))
    return table


def gen_c():
    lines = [
        "#define _GNU_SOURCE",
        "#include <complex.h>",
        "#include <errno.h>",
        "#include <stdio.h>",
        "#include <stdint.h>",
        "#include <string.h>",
        "static double D(uint64_t b) { double d; memcpy(&d, &b, 8); return d; }",
        "static float F(uint32_t b) { float f; memcpy(&f, &b, 4); return f; }",
        "static unsigned long long BD(double d) { uint64_t b; memcpy(&b, &d, 8); return b; }",
        "static unsigned BF(float f) { uint32_t b; memcpy(&b, &f, 4); return b; }",
        "int main(void) {",
    ]
    k = 0
    for single in (False, True):
        for name, pts in cases(single):
            k += 1
            a = f"a{k}"
            base = name[:-1] if single else name
            if single:
                h = lambda x: f"0x{bits32(x):08x}U"  # noqa: E731
                ty, mk, fmt, B, n = "uint32_t", "F", "%08x", "BF", 4
            else:
                h = lambda x: f"0x{bits64(x):016x}ULL"  # noqa: E731
                ty, mk, fmt, B, n = "uint64_t", "D", "%016llx", "BD", 8
            cty = "float" if single else "double"
            if base == "cpow":
                flat = ", ".join(f"{h(b[0])}, {h(b[1])}, {h(e[0])}, {h(e[1])}" for b, e in pts)
                lines.append(f"  static const {ty} {a}[] = {{{flat}}};")
                lines.append(f"  for (unsigned i = 0; i < sizeof {a} / {n}; i += 4) {{")
                lines.append(f"    {cty} complex x = __builtin_complex({mk}({a}[i]), {mk}({a}[i+1]));")
                lines.append(f"    {cty} complex y = __builtin_complex({mk}({a}[i+2]), {mk}({a}[i+3]));")
                lines.append(f"    errno = 0; {cty} complex r = {name}(x, y); int e = errno;")
                lines.append(f'    printf("{name} {fmt} {fmt} {fmt} {fmt} = {fmt} {fmt} %d\\n", '
                             f"{B}({mk}({a}[i])), {B}({mk}({a}[i+1])), {B}({mk}({a}[i+2])), "
                             f"{B}({mk}({a}[i+3])), {B}(__real__ r), {B}(__imag__ r), e); }}")
                continue
            flat = ", ".join(f"{h(re)}, {h(im)}" for re, im in pts)
            lines.append(f"  static const {ty} {a}[] = {{{flat}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / {n}; i += 2) {{")
            lines.append(f"    {cty} complex z = __builtin_complex({mk}({a}[i]), {mk}({a}[i+1]));")
            if base in UNARY_R:
                lines.append(f"    errno = 0; {cty} r = {name}(z); int e = errno;")
                lines.append(f'    printf("{name} {fmt} {fmt} = {fmt} %d\\n", '
                             f"{B}({mk}({a}[i])), {B}({mk}({a}[i+1])), {B}(r), e); }}")
            else:
                lines.append(f"    errno = 0; {cty} complex r = {name}(z); int e = errno;")
                lines.append(f'    printf("{name} {fmt} {fmt} = {fmt} {fmt} %d\\n", '
                             f"{B}({mk}({a}[i])), {B}({mk}({a}[i+1])), "
                             f"{B}(__real__ r), {B}(__imag__ r), e); }}")
    lines += ["  return 0;", "}"]
    return "\n".join(lines) + "\n"


def main():
    with workdir() as tmp:
        (Path(tmp) / "complex_oracle.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -o complex_oracle complex_oracle.c -lm "
                f"&& ./complex_oracle > '{wsl_path(OUT)}'; ldd --version | head -1")
    print(r.stdout[-2000:], r.stderr[-2000:])
    if r.returncode != 0:
        sys.exit("oracle build or run failed")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines")


if __name__ == "__main__":
    main()
