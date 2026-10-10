"""glibc 2.39's `parse_printf_format`, as the oracle for posix/src/printf.rs's.

    python posix/tools/oracle/printf_parse_harness.py   # writes posix/src/printf_parse_oracle.txt

`parse_printf_format(fmt, n, argtypes)` says how many arguments a format
takes and, in `argtypes[0..n]`, the type of each: `PA_INT`, `PA_CHAR`,
`PA_WCHAR`, `PA_STRING`, `PA_WSTRING`, `PA_POINTER`, `PA_FLOAT`, `PA_DOUBLE`,
with the `PA_FLAG_*` bits. Every conversion under every length modifier, `*`
widths and precisions, positions, a later specification naming an argument
an earlier one did, unknown conversions, a format cut short, a `w` with no
width C23 allows, and an `argtypes` too short for the format.

One line a call:

    <format-hex> <n> | <ret> <argtypes[0]>,...,<argtypes[15]>

`argtypes` is 16 entries, each set to -1 before the call, so an entry the
call did not write shows as -1.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "printf_parse_oracle.txt"

CASES: list[tuple[str, int]] = []


def case(fmt: str, n: int = 16) -> None:
    CASES.append((fmt, n))


LENGTHS = ["", "hh", "h", "l", "ll", "q", "L", "j", "z", "Z", "t",
           "w8", "w16", "w32", "w64", "wf8", "wf16", "wf32", "wf64"]
for conv in "diouxXbBeEfFgGaAcCsSpnm%y":
    for length in LENGTHS:
        case(f"%{length}{conv}")

for fmt in ["%*d", "%.*d", "%*.*d", "%-*.*s", "%1$*2$.*3$d", "%2$*1$d", "%*1$d",
            "%.*1$f %1$d", "%*y", "%.*y", "%*.*%",
            "%2$d %1$s", "%3$ls %1$c", "%1$d %1$d", "%d %1$s", "%1$s %d",
            "%3$d", "%5$p %2$n", "%d %s %f %Lf %p %n %c %lc %ls %C %S",
            "abc", "", "%", "abc%", "%5", "%l", "%1$", "%1$5", "%*", "%.*",
            "%w12d", "%wd", "%w12d %s", "%%d %d", "%m %d", "%5% %d",
            "%hhn %hn %ln %lln %jn %zn %tn", "%hs %hc %hp %hf",
            "%I'd %'I5.3lx", "%y %d", "%-0 +#y %d", "%99999999999$d %d"]:
    case(fmt)
for fmt in ["%d %s %f", "%3$d %1$s", "%*d", "%1$*2$d"]:
    case(fmt, n=1)
case("%d %s", n=0)


def main() -> None:
    lines = []
    for fmt, n in CASES:
        fmt_c = "".join(f"\\x{b:02x}" for b in fmt.encode())
        lines.append(
            f'{{ for (int i = 0; i < 16; i++) t[i] = -1; '
            f'size_t r = parse_printf_format("{fmt_c}", {n}, t); '
            f'printf("{fmt.encode().hex() or "-"} {n} | %zu", r); '
            f'for (int i = 0; i < 16; i++) printf(i ? ",%d" : " %d", t[i]); '
            f"putchar('\\n'); }}"
        )
    program = r'''
#define _GNU_SOURCE
#include <printf.h>
#include <stdio.h>

int main(void) {
    int t[16];
''' + "\n    ".join(lines) + '''
    return 0;
}
'''
    with workdir() as d:
        d = Path(d)
        (d / "pp.c").write_text(program, encoding="utf-8", newline="\n")
        built = run(f"cd {wsl_path(d)} && gcc -O0 -Wno-format -o pp pp.c 2>&1")
        if built.returncode:
            sys.exit(f"pp.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./pp")
        if r.returncode or r.stderr:
            sys.exit(f"pp: {r.stdout}{r.stderr}")
    OUT.write_text(r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(r.stdout.splitlines())} lines ({len(CASES)} cases)")


if __name__ == "__main__":
    main()
