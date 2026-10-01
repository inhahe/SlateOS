"""glibc 2.39's narrowing functions -- C23's `fadd`, `fsub`, `fmul`, `fdiv`,
`fsqrt`, `ffma` and their `l` and `d...l` forms: one operation on wider
arguments, rounded once into the narrower result -- as the oracle for
posix/src/narrow.rs.

    python posix/tools/oracle/narrow_harness.py   # writes posix/src/narrow_oracle.txt

Each call is made in all four rounding directions, and its line carries the
four answers:

    <function> <inputs> = <n> <d> <u> <z>

each answer `<result>:<flags>:<errno>` for to nearest, downward, upward and
toward zero. Values are bits in hex -- 8 digits for a float result, 16 for a
double, `SSSS:MMMMMMMMMMMMMMMM` for a long double argument -- and `<flags>`
the exceptions raised, a letter each from `IZOUX`, or `-`.

The inputs: each argument type's edge values; the arguments of glibc's own
tests of these functions (math/auto-libm-test-out-narrow-*, from the glibc
tree gen_iconv_8bit.py reads), where the type can hold them; and random
operands placed so the exact result falls on or beside a rounding boundary
of the result type -- midpoints, the overflow threshold, the subnormal
range -- where rounding twice would go wrong and round-to-odd must not.
Built -O0 with -frounding-math and -fno-builtin, from volatile arrays.
"""

import random
import re
import sys
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "narrow_oracle.txt"
GLIBC = Path("D:/refsrc/glibc-2.39")

# (exponent bits, stored significand bits, explicit integer bit)
FORMATS = {"f": (8, 23, False), "d": (11, 52, False), "l": (15, 64, True)}


def encode(t, x):
    """Exact bits of the value x in type t, or None if t cannot hold it."""
    ebits, mbits, explicit = FORMATS[t]
    bias = (1 << (ebits - 1)) - 1
    prec = mbits if explicit else mbits + 1
    x = F(x)
    sign = 1 if x < 0 else 0
    a = abs(x)
    if a == 0:
        biased, sig = 0, 0
    else:
        e = a.numerator.bit_length() - a.denominator.bit_length()
        while a >= F(2) ** (e + 1):
            e += 1
        while a < F(2) ** e:
            e -= 1
        e = max(e, 1 - bias)
        m = a / F(2) ** (e - prec + 1)
        if m.denominator != 1:
            return None
        q = m.numerator
        if q >> prec:
            return None
        biased = e + bias if q >> (prec - 1) else 0
        if biased >= (1 << ebits) - 1:
            return None
        sig = q if explicit else q & ((1 << mbits) - 1)
    if t == "l":
        return ((sign << 15) | biased, sig)
    return (sign << (ebits + mbits)) | (biased << mbits) | sig


def special(t):
    """Encodings no Fraction names: zeros' signs, infinities, NaNs."""
    ebits, mbits, explicit = FORMATS[t]
    if t == "l":
        top = 1 << 63
        return [(0x8000, 0), (0x7FFF, top), (0xFFFF, top), (0x7FFF, 0xC000_0000_0000_0000),
                (0xFFFF, 0xC000_0000_0000_0123), (0x7FFF, top | 1)]
    sign = 1 << (ebits + mbits)
    expmask = ((1 << ebits) - 1) << mbits
    quiet = 1 << (mbits - 1)
    return [sign, expmask, sign | expmask, expmask | quiet, sign | expmask | quiet | 0x123,
            expmask | 1]


def hexv(t, b):
    if t == "l":
        return f"{b[0]:04x}:{b[1]:016x}"
    return f"{b:0{8 if t == 'f' else 16}x}"


def edges(narrow):
    """Values at the edges of the result type `narrow`, and of the argument
    types, both signs."""
    ebits, mbits, _ = FORMATS[narrow]
    bias = (1 << (ebits - 1)) - 1
    top = F(2) ** bias * (2 - F(2) ** -mbits)          # the largest finite
    ulp_top = F(2) ** (bias - mbits)
    minnorm = F(2) ** (1 - bias)
    minsub = F(2) ** (1 - bias - mbits)
    vals = [0, 1, F(1, 2), 2, 3, F(1, 3), 10, top, top + ulp_top / 2, top + ulp_top,
            top - ulp_top / 2, top + ulp_top / 4, minnorm, minnorm - minsub, minsub,
            minsub / 2, minsub / 4, minsub * 3 / 4, minnorm - minsub / 2, F(2) ** 1023,
            F(2) ** -1022, F(2) ** -1074, F(2) ** 16383, F(2) ** -16382, F(2) ** -16445]
    return vals + [-v for v in vals if v]


def glibc_inputs(op):
    """The argument tuples of glibc's own tests of narrowing `op`."""
    path = GLIBC / "math" / f"auto-libm-test-out-narrow-{op}"
    out, seen = [], set()
    for line in path.read_text(encoding="latin-1").splitlines():
        if not line.startswith("= "):
            continue
        m = re.match(r"= \w+ \w+ \w+:arg_fmt\([^)]*\) (.*?) :", line)
        if not m:
            continue
        args = tuple(m.group(1).split())
        if args in seen:
            continue
        seen.add(args)
        vals = []
        for a in args:
            if a in ("plus_infty", "minus_infty", "qnan_value", "-qnan_value", "snan_value",
                     "plus_zero", "minus_zero"):
                vals.append(a)
            else:
                vals.append(parse_hex(a))
        out.append(tuple(vals))
    return out


def parse_hex(s):
    """A C hex float (0x1.8p+3, -0xf.ffp-2) as an exact Fraction."""
    neg = s.startswith("-")
    s = s.lstrip("-")
    m = re.fullmatch(r"0x([0-9a-f]*)\.?([0-9a-f]*)p([+-]\d+)", s)
    if not m:
        raise ValueError(s)
    whole, frac, exp = m.groups()
    digits = int((whole or "0") + frac, 16)
    v = F(digits, 16 ** len(frac)) * F(2) ** int(exp)
    return -v if neg else v


NAMED = {"plus_infty": "inf", "minus_infty": "-inf", "qnan_value": "qnan",
         "-qnan_value": "-qnan", "snan_value": "snan", "plus_zero": 0, "minus_zero": "-0"}


def to_bits(t, v):
    """Bits of v (a Fraction or a glibc test name) in t, or None."""
    if isinstance(v, str):
        name = NAMED.get(v, v)
        sp = special(t)
        table = {"-0": sp[0], "inf": sp[1], "-inf": sp[2], "qnan": sp[3], "-qnan": sp[4],
                 "snan": sp[5]}
        if name == 0:
            return encode(t, 0)
        return table[name]
    return encode(t, v)


def random_cases(op, wide, narrow, rng, n):
    """Operands whose exact result is near a rounding boundary of `narrow`:
    a random value of the result type, plus or minus a sliver around half
    its ulp, reached by the operation."""
    ebits, mbits, _ = FORMATS[narrow]
    wbits = FORMATS[wide][1] + (0 if FORMATS[wide][2] else 1)
    out = []
    for _ in range(n):
        e = rng.randint(-(1 << (ebits - 1)) - mbits + 4, (1 << (ebits - 1)) - 2)
        base = F(rng.getrandbits(mbits) | (1 << mbits), 1 << mbits) * F(2) ** e
        ulp = F(2) ** (e - mbits)
        tweak = rng.choice([ulp / 2, ulp / 2 + ulp * F(1, 1 << rng.randint(3, wbits - mbits - 1)),
                            ulp / 2 - ulp * F(1, 1 << rng.randint(3, wbits - mbits - 1)),
                            ulp * F(rng.getrandbits(8), 256)])
        target = base + tweak if rng.random() < 0.5 else -(base + tweak)
        if op == "add":
            x = target * F(rng.randint(1, 7), 8)
            args = (x, target - x)
        elif op == "sub":
            y = target * F(rng.randint(1, 7), 8)
            args = (target + y, y)
        elif op == "mul":
            y = F(rng.randint(1, 255) | 1, 1 << rng.randint(0, 7))
            args = (target / y, y)
        elif op == "div":
            y = F(rng.randint(1, 255) | 1, 1 << rng.randint(0, 7))
            args = (target * y, y)
        elif op == "sqrt":
            args = (target * target if target > 0 else -target * target,)
        else:  # fma: x*y + z with a product that cancels most of z
            y = F(rng.randint(1, 255) | 1, 1 << rng.randint(0, 7))
            x = base / y
            args = (x, y, target - x * y)
        out.append(args)
    return out


# The functions: (name, operation, argument type, result type).
FUNCS = []
for op in ("add", "sub", "mul", "div", "sqrt", "fma"):
    FUNCS += [(f"f{op}", op, "d", "f"), (f"f{op}l", op, "l", "f"), (f"d{op}l", op, "l", "d")]

CT = {"f": "float", "d": "double", "l": "long double"}
ARITY = {"add": 2, "sub": 2, "mul": 2, "div": 2, "sqrt": 1, "fma": 3}

PRELUDE = r"""
#define _GNU_SOURCE
#define __STDC_WANT_IEC_60559_BFP_EXT__ 1
#include <errno.h>
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef union { long double v; struct { unsigned long long m; unsigned short se; } s; } U;
static double D(uint64_t b) { double x; memcpy(&x, &b, 8); return x; }
static long double L(unsigned se, unsigned long long m) { U u; memset(&u, 0, sizeof u); u.s.m = m; u.s.se = (unsigned short) se; return u.v; }
static void pf(float x) { uint32_t b; memcpy(&b, &x, 4); printf("%08x", b); }
static void pd(double x) { uint64_t b; memcpy(&b, &x, 8); printf("%016llx", (unsigned long long) b); }
static void pl(long double x) { U u; memset(&u, 0, sizeof u); u.v = x; printf("%04x:%016llx", u.s.se, u.s.m); }
static const int MODES[4] = { FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO };
static void flags(void) {
    int e = errno, f = fetestexcept(FE_ALL_EXCEPT);
    char s[6]; int n = 0;
    if (f & FE_INVALID) s[n++] = 'I';
    if (f & FE_DIVBYZERO) s[n++] = 'Z';
    if (f & FE_OVERFLOW) s[n++] = 'O';
    if (f & FE_UNDERFLOW) s[n++] = 'U';
    if (f & FE_INEXACT) s[n++] = 'X';
    if (!n) s[n++] = '-';
    s[n] = 0;
    printf(":%s:%d", s, e);
}
int main(void) {
"""


def gen(cases):
    lines = []
    for k, (name, op, wide, narrow, rows) in enumerate(cases):
        n = ARITY[op]
        c = CT[wide]
        if wide == "l":
            flat = ", ".join(f"{{0x{b[0]:04x}u, 0x{b[1]:016x}ull}}" for row in rows for b in row)
            lines.append(f"  static volatile const struct {{ unsigned se; unsigned long long m; }} a{k}[] = {{{flat}}};")
            load = [f"L(a{k}[i * {n} + {j}].se, a{k}[i * {n} + {j}].m)" for j in range(n)]
        else:
            flat = ", ".join(f"0x{b:016x}ull" for row in rows for b in row)
            lines.append(f"  static volatile const uint64_t a{k}[] = {{{flat}}};")
            load = [f"D(a{k}[i * {n} + {j}])" for j in range(n)]
        pr = {"f": "pf", "d": "pd", "l": "pl"}
        lines.append(f"  for (int i = 0; i < {len(rows)}; i++) {{")
        lines.append(f"    {c} " + ", ".join(f"x{j} = {load[j]}" for j in range(n)) + ";")
        lines.append(f'    printf("{name}");')
        for j in range(n):
            lines.append(f'    printf(" "); {pr[wide]}(x{j});')
        lines.append('    printf(" =");')
        call = f"{name}(" + ", ".join(f"x{j}" for j in range(n)) + ")"
        rt = CT[narrow]
        lines.append("    for (int m = 0; m < 4; m++) {")
        lines.append("      fesetround(MODES[m]); feclearexcept(FE_ALL_EXCEPT); errno = 0;")
        lines.append(f"      {rt} r = {call}; volatile {rt} rr = r;")
        lines.append("      fesetround(FE_TONEAREST);")
        lines.append(f'      printf(" "); {pr[narrow]}(rr); flags(); }}')
        lines.append('    printf("\\n"); }')
    return PRELUDE + "\n".join(lines) + "\n  return 0;\n}\n"


def build():
    rng = random.Random(20260928)
    cases = []
    for name, op, wide, narrow in FUNCS:
        n = ARITY[op]
        rows = []
        pool = [e for e in edges(narrow)] + [e for e in edges(wide)]
        sp = special(wide)
        vals = [encode(wide, v) for v in pool] + sp
        vals = [v for v in vals if v is not None]
        # every edge value in each position, the others 1 (or, for fma, 1 and 0)
        for v in vals:
            for j in range(n):
                row = [encode(wide, 1)] * n
                if op == "fma" and n == 3:
                    row[2] = encode(wide, 0)
                row[j] = v
                rows.append(tuple(row))
        # pairs of edges for the binary operations
        if n == 2:
            small = [encode(wide, v) for v in edges(narrow)[:12]] + sp[:4]
            small = [v for v in small if v is not None]
            rows += [(a, b) for a in small for b in small]
        for args in glibc_inputs(op):
            bits = [to_bits(wide, a) for a in args]
            if len(bits) == n and all(b is not None for b in bits):
                rows.append(tuple(bits))
        for args in random_cases(op, wide, narrow, rng, 300):
            bits = [encode(wide, a) for a in args]
            if all(b is not None for b in bits):
                rows.append(tuple(bits))
        seen, uniq = set(), []
        for r in rows:
            if r not in seen:
                seen.add(r)
                uniq.append(r)
        cases.append((name, op, wide, narrow, uniq))
    return cases


def main():
    cases = build()
    prog = gen(cases)
    with workdir() as tmp:
        (Path(tmp) / "narrow.c").write_text(prog, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -frounding-math -fno-builtin -w "
                f"-o narrow narrow.c -lm && ./narrow")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-3000:]}")
    header = ("# glibc 2.39's narrowing functions under WSL (posix/tools/oracle/narrow_harness.py):\n"
              "# <function> <inputs> = <result>:<flags>:<errno> to nearest, downward, upward,\n"
              "# toward zero; values as bits in hex, flags from IZOUX or -.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    counts = {name: len(rows) for name, _, _, _, rows in cases}
    print(f"{len(r.stdout.splitlines())} lines -> {OUT}; {counts}")


if __name__ == "__main__":
    main()
