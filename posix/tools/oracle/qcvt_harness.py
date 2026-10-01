"""glibc 2.39's qecvt, qfcvt and qgcvt -- ecvt, fcvt and gcvt for a long
double -- as the oracle for posix/src/stdlib.rs.

    python posix/tools/oracle/qcvt_harness.py   # writes posix/src/qcvt_oracle.txt

One call a line, the value given by its 80 bits (sign and exponent, then the
significand, integer bit included):

    qecvt <sexp>:<significand> <ndigit> = <digits>|<decpt>|<sign> <exact digits>|<exact decpt>
    qfcvt <sexp>:<significand> <ndigit> = <digits>|<decpt>|<sign> <exact digits>|<exact decpt>
    qgcvt <sexp>:<significand> <ndigit> = <text>

The exact answer is the value's own digits, correctly rounded, by glibc's
conventions -- as cvt_harness.py's `<exact>` flag decides for the double
forms, which design-decisions section 1135 says this library gives: glibc's
qecvt scales the value into [1, 10) by repeated multiplication by ten, in
long double arithmetic, and qfcvt with a negative ndigit divides by ten, so
their last digits are sometimes not the value's. It is computed here from
glibc's own `%.*Le` and `%.*Lf`, which are exact: for qecvt the first
min(ndigit, 21) significant digits and one digit more when rounding carried
into a new leading one; for qfcvt with a negative ndigit the value rounded,
ties to even, at 10^k -- k = -ndigit, but at most one less than its integer
digits and 0 below 10, where glibc's loop stops -- then the k zeros. Where
glibc's answer is exact, the two halves of the line agree.
"""

import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "qcvt_oracle.txt"


def ld_of_double(x: float) -> tuple[int, int]:
    """The 80 bits of the long double equal to the double `x`."""
    b = struct.unpack("<Q", struct.pack("<d", x))[0]
    sign, e, frac = b >> 63, (b >> 52) & 0x7FF, b & ((1 << 52) - 1)
    if e == 0x7FF:
        return (sign << 15) | 0x7FFF, (1 << 63) | (frac << 11)
    if e == 0:
        if frac == 0:
            return sign << 15, 0
        # A subnormal double, frac * 2^-1074, is a normal long double:
        # m * 2^(E - 16383 - 63) with m's integer bit set.
        shift = 64 - frac.bit_length()
        return (sign << 15) | (16383 + 63 - 1074 - shift), frac << shift
    return (sign << 15) | (e - 1023 + 16383), (1 << 63) | (frac << 11)


def values(rng):
    vs = [ld_of_double(x) for x in [
        0.0, -0.0, 1.0, -1.0, 0.5, 9.9999, 99.5, 0.95, 0.00123, -0.00123, 12345.0, 5.0, 125.0,
        0.125, 1e22, 1e23, 1e-300, 0.1, 2 / 3, -2 / 3, 123456789012345678.0, 9.5, 0.05, 1e-5,
        3.0, 1e15, 999999.5, float("inf"), float("-inf"), float("nan")]]
    vs += [
        (0x7FFE, 0xFFFFFFFFFFFFFFFF),  # LDBL_MAX
        (0x0001, 0x8000000000000000),  # LDBL_MIN
        (0x0000, 0x0000000000000001),  # the least subnormal
        (0x3FFD, 0xAAAAAAAAAAAAAAAB),  # 1/3
        (0x3FFE, 0xAAAAAAAAAAAAAAAB),  # 2/3
        (0x4033, 0xDE0B6B3A763FFFFF),  # a hair below 10^15... in long double
        (0x3FFF, 0xFFFFFFFFFFFFFFFF),  # the largest below 2
        (0x4002, 0x9FFFFFFFFFFFFFFF),  # just below 10
        (0x7FFF, 0xC000000000000001),  # a quiet NaN with a payload
        (0xFFFF, 0x8000000000000000),  # -inf
    ]
    for _ in range(40):
        vs.append((rng.getrandbits(1) << 15 | rng.randint(16383 - 80, 16383 + 80),
                   (1 << 63) | rng.getrandbits(63)))
    for _ in range(10):
        vs.append((rng.getrandbits(1) << 15 | rng.randint(1, 0x7FFE), (1 << 63) | rng.getrandbits(63)))
    return vs


PROGRAM = r'''
#define _GNU_SOURCE
#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static long double LD(unsigned se, unsigned long long m)
{
    unsigned char b[16] = {0};
    long double x;
    memcpy(b, &m, 8);
    memcpy(b + 8, &se, 2);
    memcpy(&x, b, sizeof x);
    return x;
}

static char big[16384];

/* The value's own decpt: from its exact expansion, which %.11800Le writes
 * unrounded (a long double has at most about 11,500 significant digits). */
static int own_decpt(long double ax)
{
    snprintf(big, sizeof big, "%.11800Le", ax);
    return atoi(strchr(big, 'e') + 1) + 1;
}

static void exact_e(long double x, int n, char *out, int *dp)
{
    long double ax = fabsl(x);
    int own = ax == 0 ? 1 : own_decpt(ax);
    if (n <= 0) { out[0] = 0; *dp = own; return; }
    int m = n > 21 ? 21 : n;
    if (ax == 0) { memset(out, '0', m); out[m] = 0; *dp = 1; return; }
    char t[128];
    snprintf(t, sizeof t, "%.*Le", m - 1, ax);
    char *e = strchr(t, 'e');
    int k = 0;
    for (char *p = t; p < e; p++) if (isdigit((unsigned char)*p)) out[k++] = *p;
    *dp = atoi(e + 1) + 1;
    if (*dp > own) out[k++] = '0';
    out[k] = 0;
}

/* fcvt with a negative ndigit: |x| rounded, ties to even, at 10^k. */
static void exact_f_neg(long double x, int n, char *out, int *dp)
{
    long double ax = fabsl(x);
    /* Exact for |x| >= 1: a long double's last place there is 2^-63 at the
     * least, 63 fraction digits. */
    snprintf(big, sizeof big, "%.80Lf", ax);
    char *pt = strchr(big, '.');
    int il = (int)(pt - big);
    int k = ax >= 10 ? (-n < il - 1 ? -n : il - 1) : 0;
    int keep = il - k;
    char r[8000];
    memcpy(r, big, keep);
    r[keep] = 0;
    /* The rest: the dropped integer digits, then the fraction. */
    char rest[8200];
    memcpy(rest, big + keep, k);
    strcpy(rest + k, pt + 1);
    int up = 0;
    if (rest[0] > '5') up = 1;
    else if (rest[0] == '5') {
        int more = 0;
        for (char *p = rest + 1; *p; p++) if (*p != '0') more = 1;
        up = more || ((r[keep - 1] - '0') & 1);
    }
    if (up) {
        int i = keep;
        for (;;) {
            if (i == 0) { memmove(r + 1, r, strlen(r) + 1); r[0] = '1'; break; }
            i--;
            if (r[i] == '9') r[i] = '0'; else { r[i]++; break; }
        }
    }
    int len = (int)strlen(r);
    for (int j = 0; j < k; j++) r[len + j] = '0';
    r[len + k] = 0;
    strcpy(out, r);
    *dp = (int)strlen(r);
}

static void esc(const char *s)
{
    if (!*s) { fputs("\\x", stdout); return; }
    fputs(s, stdout);
}

static const unsigned VS_SE[] = {@SE@};
static const unsigned long long VS_M[] = {@M@};
static const int EN[] = {-1, 0, 1, 2, 5, 10, 18, 21, 25};
static const int FN[] = {-3, -1, 0, 1, 2, 5, 10, 21, 25};
static const int GN[] = {0, 1, 6, 18, 21, 25};

int main(void)
{
    char out[8200];
    for (size_t v = 0; v < sizeof VS_SE / sizeof VS_SE[0]; v++) {
        long double x = LD(VS_SE[v], VS_M[v]);
        int finite = isfinite(x);
        for (size_t i = 0; i < sizeof EN / sizeof EN[0]; i++) {
            int dp = 0, sg = 0, xdp;
            char *s = qecvt(x, EN[i], &dp, &sg);
            printf("qecvt %04x:%016llx %d = ", VS_SE[v], VS_M[v], EN[i]);
            esc(s);
            printf("|%d|%d ", dp, sg);
            if (finite) { exact_e(x, EN[i], out, &xdp); esc(out); printf("|%d\n", xdp); }
            else { esc(s); printf("|%d\n", dp); }
        }
        for (size_t i = 0; i < sizeof FN / sizeof FN[0]; i++) {
            int dp = 0, sg = 0, xdp;
            char *s = qfcvt(x, FN[i], &dp, &sg);
            printf("qfcvt %04x:%016llx %d = ", VS_SE[v], VS_M[v], FN[i]);
            esc(s);
            printf("|%d|%d ", dp, sg);
            if (finite && FN[i] < 0 && fabsl(x) >= 10) { exact_f_neg(x, FN[i], out, &xdp); esc(out); printf("|%d\n", xdp); }
            else { esc(s); printf("|%d\n", dp); }
        }
        for (size_t i = 0; i < sizeof GN / sizeof GN[0]; i++) {
            char b[256];
            qgcvt(x, GN[i], b);
            printf("qgcvt %04x:%016llx %d = ", VS_SE[v], VS_M[v], GN[i]);
            esc(b);
            printf("\n");
        }
    }
    return 0;
}
'''


def main() -> None:
    vs = values(random.Random(97))
    program = (PROGRAM.replace("@SE@", ", ".join(f"0x{se:04x}" for se, _m in vs))
               .replace("@M@", ", ".join(f"0x{m:016x}ull" for _se, m in vs)))
    with workdir() as t:
        d = Path(t)
        (d / "q.c").write_text(program, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -o q q.c -lm && ./q")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
    head = [
        "# glibc 2.39's qecvt, qfcvt and qgcvt, for posix/src/stdlib.rs; each call's exact answer",
        "# beside it (see the harness's docstring). Generated by posix/tools/oracle/qcvt_harness.py;",
        "# do not edit.",
    ]
    OUT.write_text("\n".join(head) + "\n" + r.stdout, encoding="utf-8", newline="\n")
    lines = r.stdout.splitlines()
    inexact = sum(1 for ln in lines if not ln.startswith("qgcvt")
                  and ln.split(" = ")[1].split(" ")[0].rsplit("|", 1)[0] != ln.split(" = ")[1].split(" ")[1])
    print(f"{OUT.name}: {len(lines)} calls, {inexact} of glibc's not exact")


if __name__ == "__main__":
    main()
