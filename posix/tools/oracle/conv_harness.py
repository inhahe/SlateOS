"""glibc 2.39's floating-point conversions, in every rounding mode, as the
oracle for posix/src/decfloat.rs, printf.rs and stdlib.rs.

    python posix/tools/oracle/conv_harness.py   # writes posix/src/conv_oracle.txt

glibc's `printf` and `strtod` follow the current rounding direction
(`fesetround`), and its `long double` conversions work in all 64 bits. Every
case here is run under each of the four directions, one line each:

    p <mode> d <format-hex> <double-bits> = [<output>]
    p <mode> l <format-hex> <SSSS:MMMMMMMMMMMMMMMM> = [<output>]
    s <mode> <function> <input-hex> = <result> <consumed> <errno>

`<mode>` is 0-3 for FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO; a
double is its bits in hex, a float its bits, a long double `SSSS:MMMM...`
(sign-and-exponent word, significand) as the 80-bit format holds it. Formats
and input strings are hex, so that a space or any other byte can be in one.
Built with -fno-builtin and from arrays, so gcc can neither fold a call nor
substitute its own answer.
"""

import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402
from mathl_harness import enc  # noqa: E402  (exact rounding into the 80-bit format)

OUT = POSIX_SRC / "conv_oracle.txt"
MODES = ["FE_TONEAREST", "FE_UPWARD", "FE_DOWNWARD", "FE_TOWARDZERO"]

# ---------------------------------------------------------------------------
# printf
# ---------------------------------------------------------------------------

D_FORMATS = ["%f", "%.0f", "%.1f", "%.2f", "%.3f", "%.17f", "%.30f", "%#.0f", "%e", "%.0e",
             "%.1e", "%.3e", "%.16e", "%.25e", "%#.0e", "%g", "%.0g", "%.1g", "%.3g", "%.17g",
             "%.30g", "%#g", "%#.3g", "%a", "%.0a", "%.1a", "%.3a", "%.12a", "%.13a", "%.20a",
             "%#.0a", "%A", "%.3A", "%E", "%G", "%F", "%+.3e", "% .2f", "%-12.3f|", "%012.3e"]


def with_l(f: str) -> str:
    """`f` with `L` before its conversion letter."""
    i = len(f) - 1
    while f[i] not in "aAeEfFgG":
        i -= 1
    return f[:i] + "L" + f[i:]


L_FORMATS = [with_l(f) for f in D_FORMATS] + [
    "%.40Lf", "%.60Le", "%.16La", "%.15La", "%.14La", "%.2La", "%.1La"]
# Long expansions, for a sample of the values only (every `step`-th).
D_LONG = {"%.1100f": 7, "%.770e": 7}
L_LONG = {"%.5000Lf": 12, "%.11600Le": 25}
# A long double far from 1 prints thousands of digits in `%Lf`: those go
# through this one fixed-notation format only, and every other conversion.
L_F_EXTREME_OK = {"%.3Lf"}


def extreme(se: int) -> bool:
    """Biased exponent more than 200 from 1's: `%Lf` would print pages."""
    return abs((se & 0x7FFF) - 16383) > 200


def dbits(x: float) -> int:
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def d_values(rng):
    v = [0.0, -0.0, 0.25, -0.25, 0.5, -0.5, 0.125, 0.375, -0.375, 1.5, 2.5, -2.5, 3.5, 25.0,
         -25.0, 1e-5, 1e22, 1e23, 0.1, -0.1, 1 / 3, 2 / 3, 9.5, 99.5, 999.5, 0.95, 0.995, 1e-300,
         5e-324, -5e-324, 2.2250738585072014e-308, 1.7976931348623157e308, -1.7976931348623157e308,
         1 + 2 ** -12, -(1 + 2 ** -12), 1 + 2 ** -52, 1.0625, 0.0625, 123456789.125,
         float("inf"), float("-inf"), float("nan"), 9.999999999999999e22, 1e-7, 5e-7, 4.35]
    # NaNs with the sign set and with payloads: glibc writes the sign ("-nan").
    v += [struct.unpack("<d", struct.pack("<Q", b))[0]
          for b in (0xfff8000000000000, 0x7ff8000000000005, 0xfff0000000000001, 0x7ff0000000000001)]
    for _ in range(40):
        v.append((rng.random() * 2 - 1) * 2.0 ** rng.randint(-60, 60))
    for _ in range(12):
        v.append(struct.unpack("<d", struct.pack("<Q", rng.getrandbits(64)))[0])
    return v


def l_values(rng):
    v = [0.0, -0.0, 0.25, -0.25, 0.5, 0.125, 1.5, 2.5, -2.5, 25.0, 0.1, Fraction(1, 10),
         Fraction(1, 3), Fraction(-2, 3),
         (0x7FFE, 0xFFFF_FFFF_FFFF_FFFF), (0xFFFE, 0xFFFF_FFFF_FFFF_FFFF),  # +-LDBL_MAX
         (0x0001, 1 << 63), (0x0000, 1), (0x8000, 1),  # LDBL_MIN, least subnormals
         (0x0000, 0x7FFF_FFFF_FFFF_FFFF),  # greatest subnormal
         Fraction(10) ** 4000, Fraction(1, 10 ** 4000), 1 + Fraction(1, 2 ** 12),
         1 + Fraction(1, 2 ** 63), 1 - Fraction(1, 2 ** 64), (0x3FFF, 0xFFFF_FFFF_FFFF_FFFF),
         Fraction(19, 2), Fraction(199, 2), Fraction(1999, 2),
         float("inf"), float("-inf"), float("nan"),
         # encodings the unit rejects: an unnormal, a pseudo-infinity, a pseudo-NaN
         (0x3FFF, 0x4000_0000_0000_0000), (0x7FFF, 0), (0x7FFF, 0x4000_0000_0000_0001),
         (0x0000, 1 << 63),  # a pseudo-denormal, which the unit reads as its value
         (0x7FFF, 0xC000_0000_0000_0005), (0xFFFF, 0x8000_0000_0000_0001),
         (0xFFFF, 0xC000_0000_0000_0000)]  # and a negative quiet NaN: "-nan"
    for _ in range(40):
        se = (0x8000 if rng.random() < 0.5 else 0) | (16383 + rng.randint(-70, 70))
        v.append((se, rng.getrandbits(63) | (1 << 63)))
    for _ in range(12):
        se = rng.getrandbits(16)
        if se & 0x7FFF in (0, 0x7FFF):
            se ^= 1
        v.append((se, rng.getrandbits(63) | (1 << 63)))
    return [x if isinstance(x, tuple) else enc(x) for x in v]


def cstr(s: str) -> str:
    """A C string literal holding exactly `s`."""
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


# ---------------------------------------------------------------------------
# strtod / strtof / strtold
# ---------------------------------------------------------------------------

def s_inputs(rng):
    base = [
        "0", "-0", "0.3", "-0.3", "0.1", "1e400", "-1e400", "1e-400", "-1e-400", "1e5000",
        "-1e5000", "1e-5000", "1e-4950", "1e-4940", "1e4932", "1.18973149535723176502e4932",
        "1.189731495357231765e4932", "3.6451995318824746025e-4951", "1.8225997659412373012e-4951",
        "1.82259976594123730126e-4951", "5e-324", "2.4703282292062327e-324",
        "2.4703282292062328e-324", "1.7976931348623157e308", "1.7976931348623158e308",
        "1.7976931348623159e308", "3.4028235e38", "3.4028236e38", "1.4e-45", "7e-46", "0x1p1024",
        "0x1.fffffffffffff8p1023", "0x1.fffffffffffff7ffp1023", "0x1p-1075", "0x1.8p-1075",
        "0x1p16384", "0x1.fffffffffffffffep16383", "0x1p-16446", "0x1.8p-16446", "0xcp-3",
        "0x0.000000000000001p-16385", "0x1.0000000000000001p0", "0x1.00000000000000008p0",
        "0x1.00000000000000018p0", "1.00000000000000000005421010862427522170037264",
        "1.000000000000000000054210108624275221700372640043497085571289062500001",
        "1.0000000000000000000542101086242752217003726400434970855712890625",
        "9007199254740993", "9007199254740993.0000000001", "18446744073709551617",
        "18446744073709551616.5", "36893488147419103233", "1.000000059604644775390625",
        "1.000000059604644830901776231257827021181583404541015625", "inf", "-infinity", "nan",
        "nan(0x5)", "  +1.5e+3x", ".5", "5.", "1e", "1e+", "0x", "0x.p1", "-.e3", "1e-2147483649",
        "1e2147483648", "0.000000000000000000000000000000000000001e40",
        "123456789012345678901234567890e-30",
        # Subnormals that are exact (no ERANGE in glibc) and ones that are not,
        # and values just under the least normal number, which x86's rule --
        # tininess after rounding -- may round up into the normal range.
        "0x1p-1074", "0x1.8p-1073", "0x1p-149", "0x1.8p-148", "0x1p-16445", "0x1.8p-16444",
        "0x1.fffffffffffff8p-1023", "0x1.fffffffffffff7p-1023", "0x1.fffffffffffffp-1023",
        "2.2250738585072011e-308", "2.2250738585072014e-308", "0x1.fffffep-127", "0x1.ffffffp-127",
        "0x1.ffffffffffffffffp-16383", "0x1.fffffffffffffffep-16383",
        "4.9406564584124654e-324", "1e-310", "-1e-310",
    ]
    for _ in range(40):
        digits = "".join(rng.choice("0123456789") for _ in range(rng.randint(1, 40)))
        base.append(f"{digits}e{rng.randint(-400, 400)}")
    for _ in range(20):
        digits = "".join(rng.choice("0123456789") for _ in range(rng.randint(15, 30)))
        base.append(f"0.{digits}e{rng.randint(-4960, 4940)}")
    return base


def gen_c(rng) -> str:
    dv = d_values(rng)
    lv = l_values(rng)
    ins = s_inputs(rng)
    c = [
        "#define _GNU_SOURCE",
        "#include <errno.h>", "#include <fenv.h>", "#include <stdio.h>", "#include <stdlib.h>",
        "#include <string.h>",
        "typedef union { long double v; struct { unsigned long long m; unsigned short se; } s; } U;",
        "static long double L(unsigned se, unsigned long long m) "
        "{ U u; memset(&u, 0, sizeof u); u.s.m = m; u.s.se = se; return u.v; }",
        "static double D(unsigned long long b) { double d; memcpy(&d, &b, 8); return d; }",
        "static char buf[20000];",
        "static void hexs(const char *s) { for (; *s; s++) printf(\"%02x\", (unsigned char)*s); }",
        "static const int modes[] = {" + ", ".join(MODES) + "};",
        "static const unsigned long long dv[] = {" + ", ".join(f"0x{dbits(x):016x}ull" for x in dv) + "};",
        "static const unsigned short lse[] = {" + ", ".join(f"0x{se:04x}" for se, _ in lv) + "};",
        "static const unsigned long long lm[] = {" + ", ".join(f"0x{m:016x}ull" for _, m in lv) + "};",
        "static const unsigned char lext[] = {" + ", ".join(str(int(extreme(se))) for se, _ in lv) + "};",
        "static const char *const sin_[] = {" + ", ".join(cstr(s) for s in ins) + "};",
        "int main(void) {",
        "  for (int mode = 0; mode < 4; mode++) {",
        "    fesetround(modes[mode]);",
    ]
    for f, step in [(f, 1) for f in D_FORMATS] + list(D_LONG.items()):
        c += [f"    for (unsigned i = 0; i < sizeof dv / 8; i += {step}) {{",
              f"      snprintf(buf, sizeof buf, {cstr(f)}, D(dv[i]));",
              f'      printf("p %d d ", mode); hexs({cstr(f)}); printf(" %016llx = [%s]\\n", dv[i], buf);',
              "    }"]
    for f, step in [(f, 1) for f in L_FORMATS] + list(L_LONG.items()):
        fixed = f.rstrip("|")[-1] in "fF"
        skip = "      if (lext[i]) continue;" if fixed and f not in L_F_EXTREME_OK else ""
        c += [f"    for (unsigned i = 0; i < sizeof lm / 8; i += {step}) {{"] + ([skip] if skip else []) + [
              f"      snprintf(buf, sizeof buf, {cstr(f)}, L(lse[i], lm[i]));",
              f'      printf("p %d l ", mode); hexs({cstr(f)}); '
              f'printf(" %04x:%016llx = [%s]\\n", lse[i], lm[i], buf);',
              "    }"]
    c += [
        "    for (unsigned i = 0; i < sizeof sin_ / sizeof sin_[0]; i++) {",
        "      char *end; U u; unsigned long long b; unsigned fb; int e;",
        "      errno = 0; double d = strtod(sin_[i], &end); e = errno; memcpy(&b, &d, 8);",
        '      printf("s %d strtod ", mode); hexs(sin_[i]); printf(" = %016llx %d %d\\n", b, (int)(end - sin_[i]), e);',
        "      errno = 0; float f = strtof(sin_[i], &end); e = errno; memcpy(&fb, &f, 4);",
        '      printf("s %d strtof ", mode); hexs(sin_[i]); printf(" = %08x %d %d\\n", fb, (int)(end - sin_[i]), e);',
        "      errno = 0; memset(&u, 0, sizeof u); u.v = strtold(sin_[i], &end); e = errno;",
        '      printf("s %d strtold ", mode); hexs(sin_[i]); '
        'printf(" = %04x:%016llx %d %d\\n", u.s.se, u.s.m, (int)(end - sin_[i]), e);',
        "    }",
        "  }",
        "  return 0;",
        "}",
    ]
    return "\n".join(c) + "\n"


def main():
    rng = random.Random(20260928)
    with workdir() as tmp:
        (Path(tmp) / "conv_oracle.c").write_text(gen_c(rng), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -w -o conv_oracle conv_oracle.c -lm "
                f"&& ./conv_oracle > '{wsl_path(OUT)}'")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines, {OUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
