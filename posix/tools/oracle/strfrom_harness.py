"""glibc 2.39's strfromd, strfromf and strfroml (C23 7.24.1.3) and
timespec_getres (C23 7.29.2.7), as the oracle for posix/src/printf.rs and
posix/src/time.rs.

    python posix/tools/oracle/strfrom_harness.py   # writes posix/src/strfrom_oracle.txt

Lines, tab-separated:

    <function> <format> <value> <size> <result> <text>

where the value is its bits -- `d:` and 16 hex digits for a double, `f:` and 8
for a float, `l:` and 4 + 16 (sign and exponent, then significand) for a long
double -- the size is the buffer's (0: a NULL buffer), the result is the
function's, and the text is what the buffer holds after it, C-escaped
(nothing for a NULL buffer). And one line for timespec_getres:

    timespec_getres <result for bases 0 to 5> / <result with a NULL ts, base 1>

A format C23 does not allow is not replayed: glibc's abort()s on one.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "strfrom_oracle.txt"

FORMATS = ["%a", "%A", "%e", "%E", "%f", "%F", "%g", "%G",
           "%.a", "%.e", "%.f", "%.g",
           "%.0a", "%.1a", "%.3a", "%.13a", "%.20a",
           "%.0e", "%.3e", "%.17e", "%.30e",
           "%.0f", "%.3f", "%.20f",
           "%.0g", "%.1g", "%.6g", "%.17g", "%.25g", "%.40g"]
TRUNC_FORMATS = ["%e", "%.10f", "%a", "%g"]
TRUNC_SIZES = [0, 1, 2, 5, 8]

PROGRAM = r'''
#define _GNU_SOURCE
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static const char *formats[] = {FORMATS};
static const char *trunc_formats[] = {TRUNC_FORMATS};
static const size_t trunc_sizes[] = {TRUNC_SIZES};
#define N(a) (sizeof(a) / sizeof(a)[0])

static void esc(const char *s)
{
    for (; *s; s++) {
        unsigned char c = (unsigned char)*s;
        if (c == '\\') printf("\\\\");
        else if (c < 32 || c > 126) printf("\\x%02x", c);
        else putchar(c);
    }
}

static void show(const char *fn, const char *fmt, const char *val, size_t size, int r,
                 const char *buf)
{
    printf("%s\t%s\t%s\t%zu\t%d\t", fn, fmt, val, size, r);
    if (buf) esc(buf);
    printf("\n");
}

static double dvals[32];
static float fvals[32];
static long double lvals[32];
static size_t nd, nf, nl;

static void d_(double v) { dvals[nd++] = v; }
static void f_(float v) { fvals[nf++] = v; }
static void l_(long double v) { lvals[nl++] = v; }

static void dname(double v, char *out)
{
    uint64_t b; memcpy(&b, &v, 8);
    sprintf(out, "d:%016llx", (unsigned long long)b);
}

static void fname(float v, char *out)
{
    uint32_t b; memcpy(&b, &v, 4);
    sprintf(out, "f:%08x", b);
}

static void lname(long double v, char *out)
{
    unsigned char b[16] = {0}; memcpy(b, &v, 10);
    uint64_t sig; uint16_t se;
    memcpy(&sig, b, 8); memcpy(&se, b + 8, 2);
    sprintf(out, "l:%04x%016llx", se, (unsigned long long)sig);
}

static void each_d(const char *fmt, size_t size, int show_all)
{
    char buf[1024], name[40];
    for (size_t i = 0; i < nd; i++) {
        if (!show_all && i % 7 != 3) continue;
        dname(dvals[i], name);
        memset(buf, 0x55, sizeof buf); buf[sizeof buf - 1] = 0;
        int r = strfromd(size ? buf : NULL, size, fmt, dvals[i]);
        show("strfromd", fmt, name, size, r, size ? buf : NULL);
    }
}

static void each_f(const char *fmt, size_t size, int show_all)
{
    char buf[1024], name[40];
    for (size_t i = 0; i < nf; i++) {
        if (!show_all && i % 7 != 3) continue;
        fname(fvals[i], name);
        memset(buf, 0x55, sizeof buf); buf[sizeof buf - 1] = 0;
        int r = strfromf(size ? buf : NULL, size, fmt, fvals[i]);
        show("strfromf", fmt, name, size, r, size ? buf : NULL);
    }
}

static void each_l(const char *fmt, size_t size, int show_all)
{
    char buf[1024], name[40];
    for (size_t i = 0; i < nl; i++) {
        if (!show_all && i % 5 != 2) continue;
        lname(lvals[i], name);
        memset(buf, 0x55, sizeof buf); buf[sizeof buf - 1] = 0;
        int r = strfroml(size ? buf : NULL, size, fmt, lvals[i]);
        show("strfroml", fmt, name, size, r, size ? buf : NULL);
    }
}

int main(void)
{
    d_(0.0); d_(-0.0); d_(1.0); d_(-1.0); d_(0.5); d_(0.1); d_(1.0 / 3); d_(2.0 / 3);
    d_(123456.789); d_(1e21); d_(1e22); d_(1e300); d_(1e-300); d_(DBL_TRUE_MIN);
    d_(DBL_MIN); d_(DBL_MAX); d_(INFINITY); d_(-INFINITY); d_(NAN); d_(-NAN);
    d_(M_PI); d_(M_E); d_(9.5); d_(0.125); d_(1e15 + 0.3); d_(-2.5e-5);
    f_(0.0f); f_(-0.0f); f_(1.0f); f_(-1.0f); f_(0.1f); f_(1.0f / 3); f_(123456.789f);
    f_(1e30f); f_(1e-30f); f_(FLT_TRUE_MIN); f_(FLT_MIN); f_(FLT_MAX); f_(INFINITY);
    f_(-INFINITY); f_(NAN); f_(-NAN); f_((float)M_PI); f_(9.5f); f_(16777217.0f);
    l_(0.0L); l_(-0.0L); l_(1.0L); l_(-1.0L); l_(0.1L); l_(1.0L / 3); l_(123456.789L);
    l_(1e4000L); l_(1e-4000L); l_(LDBL_TRUE_MIN); l_(LDBL_MIN); l_(LDBL_MAX);
    l_(INFINITY); l_(-INFINITY); l_(NAN); l_(-NAN); l_(3.14159265358979323846264338L);
    l_(9.5L); l_(0x1.fffffffffffffffep+63L);

    for (size_t k = 0; k < N(formats); k++) {
        each_d(formats[k], 1024, 1);
        each_f(formats[k], 1024, 1);
        each_l(formats[k], 1024, 1);
    }
    for (size_t k = 0; k < N(trunc_formats); k++)
        for (size_t z = 0; z < N(trunc_sizes); z++) {
            each_d(trunc_formats[k], trunc_sizes[z], 0);
            each_f(trunc_formats[k], trunc_sizes[z], 0);
            each_l(trunc_formats[k], trunc_sizes[z], 0);
        }

    struct timespec ts;
    printf("timespec_getres");
    for (int base = 0; base <= 5; base++) printf(" %d", timespec_getres(&ts, base));
    printf(" / %d\n", timespec_getres(NULL, TIME_UTC));
    return 0;
}
'''


def c_list(items):
    return "{" + ", ".join(f'"{s}"' if isinstance(s, str) else str(s) for s in items) + "}"


def main() -> None:
    src = (PROGRAM.replace("{FORMATS}", c_list(FORMATS))
           .replace("{TRUNC_FORMATS}", c_list(TRUNC_FORMATS))
           .replace("{TRUNC_SIZES}", c_list(TRUNC_SIZES)))
    with workdir() as t:
        d = Path(t)
        (d / "sf.c").write_text(src, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -std=gnu2x -o sf sf.c -lm && ./sf")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}")
        body = r.stdout
    head = ("# glibc 2.39's strfromd, strfromf, strfroml and timespec_getres, for\n"
            "# posix/src/printf.rs and posix/src/time.rs. Generated by\n"
            "# posix/tools/oracle/strfrom_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
