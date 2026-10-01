"""glibc 2.39's libm as the oracle for posix/src/math.rs.

    python posix/tools/oracle/math_harness.py   # writes posix/src/math_oracle.txt

For every case in math_cases.py the C program sets errno to 0, calls glibc's
function, and prints one line:

    <name> <inputs...> = <outputs...> <errno>

Floating-point values are written as their bits in hex (16 digits for a
double, 8 for a float), so the Rust side compares bit patterns, not decimal
renderings; integers are decimal.  Built with -fno-builtin and from arrays, so
gcc can neither constant-fold a call nor substitute its own (MPFR-exact)
answer for glibc's.
"""

import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import math_cases as mc  # noqa: E402
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "math_oracle.txt"


def h64(x):
    return f"0x{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}ULL"


def h32(x):
    return f"0x{struct.unpack('<I', struct.pack('<f', mc.f32(x)))[0]:08x}U"


def gen_c():
    lines = [
        "#define _GNU_SOURCE",
        "#include <math.h>",
        "#include <errno.h>",
        "#include <stdio.h>",
        "#include <stdint.h>",
        "#include <string.h>",
        "static double D(uint64_t b) { double d; memcpy(&d, &b, 8); return d; }",
        "static float F(uint32_t b) { float f; memcpy(&f, &b, 4); return f; }",
        "static uint64_t BD(double d) { uint64_t b; memcpy(&b, &d, 8); return b; }",
        "static uint32_t BF(float f) { uint32_t b; memcpy(&b, &f, 4); return b; }",
        "double gamma(double); double significand(double); double drem(double, double);",
        "int finite(double); int finitef(float);",
        "int main(void) {",
    ]
    for k, (name, sig, rows) in enumerate(mc.cases()):
        a = f"a{k}"
        if sig in ("d_d", "i_d", "l_d", "d_dpi", "d_dpd", "v_dpdpd"):
            lines.append(f"  static const uint64_t {a}[] = {{{', '.join(h64(r[0]) for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / 8; i++) {{ double x = D({a}[i]); errno = 0;")
            if sig == "d_d":
                lines.append(f'    double r = {name}(x); int e = errno; printf("{name} %016llx = %016llx %d\\n", (unsigned long long){a}[i], (unsigned long long)BD(r), e); }}')
            elif sig == "i_d":
                lines.append(f'    int r = {name}(x); int e = errno; printf("{name} %016llx = %d %d\\n", (unsigned long long){a}[i], r, e); }}')
            elif sig == "l_d":
                lines.append(f'    long r = {name}(x); int e = errno; printf("{name} %016llx = %ld %d\\n", (unsigned long long){a}[i], r, e); }}')
            elif sig == "d_dpi":
                lines.append(f'    int o = 12345; double r = {name}(x, &o); int e = errno; printf("{name} %016llx = %016llx %d %d\\n", (unsigned long long){a}[i], (unsigned long long)BD(r), o, e); }}')
            elif sig == "d_dpd":
                lines.append(f'    double o = 0; double r = {name}(x, &o); int e = errno; printf("{name} %016llx = %016llx %016llx %d\\n", (unsigned long long){a}[i], (unsigned long long)BD(r), (unsigned long long)BD(o), e); }}')
            elif sig == "v_dpdpd":
                lines.append(f'    double s = 0, c = 0; {name}(x, &s, &c); int e = errno; printf("{name} %016llx = %016llx %016llx %d\\n", (unsigned long long){a}[i], (unsigned long long)BD(s), (unsigned long long)BD(c), e); }}')
        elif sig in ("f_f", "i_f", "l_f", "f_fpi", "f_fpf", "v_fpfpf"):
            lines.append(f"  static const uint32_t {a}[] = {{{', '.join(h32(r[0]) for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / 4; i++) {{ float x = F({a}[i]); errno = 0;")
            if sig == "f_f":
                lines.append(f'    float r = {name}(x); int e = errno; printf("{name} %08x = %08x %d\\n", {a}[i], BF(r), e); }}')
            elif sig == "i_f":
                lines.append(f'    int r = {name}(x); int e = errno; printf("{name} %08x = %d %d\\n", {a}[i], r, e); }}')
            elif sig == "l_f":
                lines.append(f'    long r = {name}(x); int e = errno; printf("{name} %08x = %ld %d\\n", {a}[i], r, e); }}')
            elif sig == "f_fpi":
                lines.append(f'    int o = 12345; float r = {name}(x, &o); int e = errno; printf("{name} %08x = %08x %d %d\\n", {a}[i], BF(r), o, e); }}')
            elif sig == "f_fpf":
                lines.append(f'    float o = 0; float r = {name}(x, &o); int e = errno; printf("{name} %08x = %08x %08x %d\\n", {a}[i], BF(r), BF(o), e); }}')
            elif sig == "v_fpfpf":
                lines.append(f'    float s = 0, c = 0; {name}(x, &s, &c); int e = errno; printf("{name} %08x = %08x %08x %d\\n", {a}[i], BF(s), BF(c), e); }}')
        elif sig in ("d_dd", "d_ddpi"):
            lines.append(f"  static const uint64_t {a}[][2] = {{{', '.join('{' + h64(r[0]) + ',' + h64(r[1]) + '}' for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / 16; i++) {{ double x = D({a}[i][0]), y = D({a}[i][1]); errno = 0;")
            if sig == "d_dd":
                lines.append(f'    double r = {name}(x, y); int e = errno; printf("{name} %016llx %016llx = %016llx %d\\n", (unsigned long long){a}[i][0], (unsigned long long){a}[i][1], (unsigned long long)BD(r), e); }}')
            else:
                lines.append(f'    int q = 12345; double r = {name}(x, y, &q); int e = errno; printf("{name} %016llx %016llx = %016llx %d %d\\n", (unsigned long long){a}[i][0], (unsigned long long){a}[i][1], (unsigned long long)BD(r), q, e); }}')
        elif sig in ("f_ff", "f_ffpi"):
            lines.append(f"  static const uint32_t {a}[][2] = {{{', '.join('{' + h32(r[0]) + ',' + h32(r[1]) + '}' for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / 8; i++) {{ float x = F({a}[i][0]), y = F({a}[i][1]); errno = 0;")
            if sig == "f_ff":
                lines.append(f'    float r = {name}(x, y); int e = errno; printf("{name} %08x %08x = %08x %d\\n", {a}[i][0], {a}[i][1], BF(r), e); }}')
            else:
                lines.append(f'    int q = 12345; float r = {name}(x, y, &q); int e = errno; printf("{name} %08x %08x = %08x %d %d\\n", {a}[i][0], {a}[i][1], BF(r), q, e); }}')
        elif sig == "d_ddd":
            lines.append(f"  static const uint64_t {a}[][3] = {{{', '.join('{' + ','.join(h64(v) for v in r) + '}' for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / 24; i++) {{ errno = 0;")
            lines.append(f'    double r = {name}(D({a}[i][0]), D({a}[i][1]), D({a}[i][2])); int e = errno; printf("{name} %016llx %016llx %016llx = %016llx %d\\n", (unsigned long long){a}[i][0], (unsigned long long){a}[i][1], (unsigned long long){a}[i][2], (unsigned long long)BD(r), e); }}')
        elif sig == "f_fff":
            lines.append(f"  static const uint32_t {a}[][3] = {{{', '.join('{' + ','.join(h32(v) for v in r) + '}' for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a} / 12; i++) {{ errno = 0;")
            lines.append(f'    float r = {name}(F({a}[i][0]), F({a}[i][1]), F({a}[i][2])); int e = errno; printf("{name} %08x %08x %08x = %08x %d\\n", {a}[i][0], {a}[i][1], {a}[i][2], BF(r), e); }}')
        elif sig in ("d_di", "d_dl"):
            lines.append(f"  static const uint64_t {a}x[] = {{{', '.join(h64(r[0]) for r in rows)}}};")
            lines.append(f"  static const long {a}n[] = {{{', '.join(str(r[1]) + 'L' for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a}n / sizeof {a}n[0]; i++) {{ errno = 0;")
            cast = "(int)" if sig == "d_di" else ""
            lines.append(f'    double r = {name}(D({a}x[i]), {cast}{a}n[i]); int e = errno; printf("{name} %016llx %ld = %016llx %d\\n", (unsigned long long){a}x[i], {a}n[i], (unsigned long long)BD(r), e); }}')
        elif sig in ("f_fi", "f_fl"):
            lines.append(f"  static const uint32_t {a}x[] = {{{', '.join(h32(r[0]) for r in rows)}}};")
            lines.append(f"  static const long {a}n[] = {{{', '.join(str(r[1]) + 'L' for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a}n / sizeof {a}n[0]; i++) {{ errno = 0;")
            cast = "(int)" if sig == "f_fi" else ""
            lines.append(f'    float r = {name}(F({a}x[i]), {cast}{a}n[i]); int e = errno; printf("{name} %08x %ld = %08x %d\\n", {a}x[i], {a}n[i], BF(r), e); }}')
        elif sig == "d_id":
            lines.append(f"  static const int {a}n[] = {{{', '.join(str(r[0]) for r in rows)}}};")
            lines.append(f"  static const uint64_t {a}x[] = {{{', '.join(h64(r[1]) for r in rows)}}};")
            lines.append(f"  for (unsigned i = 0; i < sizeof {a}n / sizeof {a}n[0]; i++) {{ errno = 0;")
            lines.append(f'    double r = {name}({a}n[i], D({a}x[i])); int e = errno; printf("{name} %d %016llx = %016llx %d\\n", {a}n[i], (unsigned long long){a}x[i], (unsigned long long)BD(r), e); }}')
        else:
            raise SystemExit(f"no generator for {sig}")
    lines += ["  return 0;", "}"]
    return "\n".join(lines) + "\n"


def main():
    with workdir() as tmp:
        (Path(tmp) / "math_oracle.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -w -o math_oracle math_oracle.c -lm "
                "&& ./math_oracle")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    lines = [l for l in r.stdout.splitlines() if l.strip()]
    want = sum(len(rows) for _, _, rows in mc.cases())
    assert len(lines) == want, (len(lines), want)
    header = ("# glibc 2.39 libm under WSL (posix/tools/oracle/math_harness.py), one call a line:\n"
              "# <function> <inputs> = <outputs> <errno>; floats as bit patterns in hex.\n")
    OUT.write_text(header + "\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(lines)} calls -> {OUT}")


if __name__ == "__main__":
    main()
