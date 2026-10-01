"""glibc 2.39's ecvt, fcvt and gcvt, as the oracle for posix/src/stdlib.rs.

    python posix/tools/oracle/cvt_harness.py   # writes posix/src/cvt_oracle.txt

One call a line:

    ecvt <double-bits> <ndigit> = <digits>|<decpt>|<sign> <exact>
    fcvt <double-bits> <ndigit> = <digits>|<decpt>|<sign> <exact>
    gcvt <double-bits> <ndigit> = <text>

`<exact>` is 1 where glibc's digits are the value's own, correctly rounded
-- which the same C program decides from glibc's `%.*e` and `%.*f`, which are
exact -- and 0 where they are not: glibc's `ecvt` scales the value into
[1, 10) by repeated multiplication by ten, in floating point, and so gets the
last digits wrong about one call in ten; `fcvt` does the same for a negative
`ndigit`. The library here computes exact digits (design-decisions §1135),
so the tests hold it to glibc's line where `<exact>` is 1, and where it is 0
to the value's own digits, which they compute independently and write by
glibc's conventions: the count of digits, a carry's extra digit, `decpt`,
the sign. (Not glibc's count or `decpt` there: its inexact scaling can carry
where the value does not.)
"""

import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "cvt_oracle.txt"


def bits(x: float) -> int:
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def values(rng):
    v = [0.0, -0.0, 1.0, -1.0, 0.5, 9.9999, 99.5, 0.95, 0.00123, -0.00123, 12345.0, 5.0, 125.0,
         0.125, 1e22, 1e23, 1e-300, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308,
         0.1, 2 / 3, -2 / 3, 123456789012345678.0, 9.5, 0.05, 1e-5, 3.0, 1e15, 999999.5,
         float("inf"), float("-inf"), float("nan")]
    for _ in range(40):
        v.append((rng.random() * 2 - 1) * 10.0 ** rng.randint(-20, 20))
    for _ in range(10):
        v.append(struct.unpack("<d", struct.pack("<Q", rng.getrandbits(64)))[0])
    return v


def gen_c(rng) -> str:
    vs = values(rng)
    return "\n".join([
        "#define _GNU_SOURCE",
        "#include <stdio.h>", "#include <stdlib.h>", "#include <string.h>", "#include <math.h>",
        "static double D(unsigned long long b) { double d; memcpy(&d, &b, 8); return d; }",
        "static const unsigned long long vs[] = {" + ", ".join(f"0x{bits(x):016x}ull" for x in vs) + "};",
        "/* The decimal exponent of finite, nonzero ax > 0: from its exact expansion,",
        "   which %.800e writes unrounded (a double has at most 767 significant digits). */",
        "static int exp10_of(double ax) {",
        "  static char t[1200]; snprintf(t, sizeof t, \"%.800e\", ax);",
        "  return atoi(strchr(t, 'e') + 1);",
        "}",
        "/* The value's own first n digits and decimal-point position, from %.*e (exact). */",
        "static int exact_e(double x, int n, const char *digits, int decpt) {",
        "  if (!isfinite(x)) return 1;",
        "  int own = x == 0 ? 1 : exp10_of(fabs(x)) + 1;",
        "  /* No digits, and the value's own decpt -- glibc's exponent loop, which",
        "     multiplies by ten in floating point, can be one out: 1e23. */",
        "  if (n <= 0) return digits[0] == 0 && decpt == own;",
        "  if (n > 17) n = 17;",
        "  char t[64]; snprintf(t, sizeof t, \"%.*e\", n - 1, fabs(x));",
        "  char d[64]; int k = 0; char *p = t;",
        "  for (; *p && *p != 'e'; p++) if (*p != '.') d[k++] = *p;",
        "  d[k] = 0;",
        "  int dp = x == 0 ? 1 : atoi(p + 1) + 1;",
        "  /* A rounding that carries into a new leading digit is written, as glibc",
        "     writes it, with one digit more: 9.9999 to one digit is \"10\", not \"1\". */",
        "  if (x != 0 && own < dp) { d[k++] = '0'; d[k] = 0; }",
        "  return strcmp(d, digits) == 0 && dp == decpt;",
        "}",
        "/* fcvt with a negative n: |x| rounded, ties to even, at 10^k -- k = -n, but",
        "   at most one less than the integer digits, and 0 below 10, as glibc's loop",
        "   stops -- from the exact expansion %.1100f writes, the digits then the",
        "   rounded integer's. glibc gets there by multiplying by 0.1, inexactly. */",
        "static int exact_f_neg(double x, int n, const char *digits, int decpt) {",
        "  static char t[2000]; snprintf(t, sizeof t, \"%.1100f\", fabs(x));",
        "  char *dot = strchr(t, '.'); int ilen = (int)(dot - t);",
        "  int k = 0; if (fabs(x) >= 10) { k = -n; if (k > ilen - 1) k = ilen - 1; }",
        "  static char r[2000]; int keep = ilen - k; memcpy(r, t, keep); r[keep] = 0;",
        "  /* the dropped part: t[keep..ilen) then the fraction */",
        "  static char rest[2000]; int m = 0;",
        "  for (int i = keep; i < ilen; i++) rest[m++] = t[i];",
        "  for (char *p = dot + 1; *p; p++) rest[m++] = *p;",
        "  rest[m] = 0;",
        "  int up = 0;",
        "  if (m > 0) {",
        "    int tail = 0; for (int i = 1; i < m; i++) if (rest[i] != '0') { tail = 1; break; }",
        "    if (rest[0] > '5' || (rest[0] == '5' && tail)) up = 1;",
        "    else if (rest[0] == '5' && !tail) up = (r[keep - 1] - '0') % 2 == 1;",
        "  }",
        "  if (up) { int i = keep - 1; while (i >= 0 && r[i] == '9') r[i--] = '0';",
        "    if (i >= 0) r[i]++; else { memmove(r + 1, r, keep + 1); r[0] = '1'; keep++; } }",
        "  for (int i = 0; i < k; i++) r[keep + i] = '0';",
        "  r[keep + k] = 0;",
        "  return strcmp(r, digits) == 0 && (int)strlen(r) == decpt;",
        "}",
        "/* The same for fcvt: %.*f's digits, the point dropped, leading zeros",
        "   stripped as fcvt strips them. */",
        "static int exact_f(double x, int n, const char *digits, int decpt) {",
        "  if (!isfinite(x)) return 1;",
        "  if (n < 0) return exact_f_neg(x, n, digits, decpt);",
        "  if (n > 17) n = 17;",
        "  static char t[400]; snprintf(t, sizeof t, \"%.*f\", n, fabs(x));",
        "  static char d[400]; int k = 0, dp = 0, seen = 0;",
        "  for (char *p = t; *p; p++) { if (*p == '.') { seen = 1; continue; } d[k++] = *p; if (!seen) dp++; }",
        "  d[k] = 0;",
        "  char *q = d;",
        "  /* fcvt strips leading zeros only when there is a fraction to strip into */",
        "  if (n > 0 && x != 0 && dp == 1 && d[0] == '0') { q++; dp--; while (*q == '0') { q++; dp--; } }",
        "  return strcmp(q, digits) == 0 && dp == decpt;",
        "}",
        "int main(void) {",
        "  char g[512];",
        "  for (unsigned i = 0; i < sizeof vs / 8; i++) {",
        "    double x = D(vs[i]); int dp, sg; char *r;",
        "    for (int n = 0; n <= 20; n++) {",
        "      r = ecvt(x, n, &dp, &sg);",
        "      printf(\"ecvt %016llx %d = %s|%d|%d %d\\n\", vs[i], n, r, dp, sg, exact_e(x, n, r, dp));",
        "    }",
        "    for (int n = -4; n <= 20; n++) {",
        "      if (!(fabs(x) < 1e30) && isfinite(x) && n > 3) continue;",
        "      r = fcvt(x, n, &dp, &sg);",
        "      printf(\"fcvt %016llx %d = %s|%d|%d %d\\n\", vs[i], n, r, dp, sg, exact_f(x, n, r, dp));",
        "    }",
        "    for (int n = 0; n <= 20; n++) {",
        "      gcvt(x, n, g);",
        "      printf(\"gcvt %016llx %d = %s\\n\", vs[i], n, g);",
        "    }",
        "  }",
        "  return 0;",
        "}",
    ]) + "\n"


def main():
    rng = random.Random(20260928)
    with workdir() as tmp:
        (Path(tmp) / "cvt_oracle.c").write_text(gen_c(rng), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -w -o cvt_oracle cvt_oracle.c -lm "
                f"&& ./cvt_oracle > '{wsl_path(OUT)}'")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines")


if __name__ == "__main__":
    main()
