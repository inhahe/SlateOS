"""glibc 2.39's `strfmon` (`<monetary.h>`), in the C locale, as the oracle
for posix/src/monetary.rs's:

    python posix/tools/oracle/strfmon_harness.py   # writes posix/src/strfmon_oracle.txt

One line a call:

    <format> <maxsize> <kind> <values...> = <rc> <errno> <text>

`<kind>` says how the values are passed: `d` as `double`s, `L` as `long
double`s (each written as the decimal `double` it is converted from, so
the two kinds can be compared). `<text>` is the buffer as the call left it
(the harness zeroes it first), up to its first NUL, and `<format>` is
written the same way: a byte from `!` to `~` as itself but `\\`, any other
as `\\xHH`, the empty text `\\x`. errno is `0` where the call left it so.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "strfmon_oracle.txt"

VALUES = ["1.23", "-1.23", "0", "-0.0", "1234567.891", "0.005", "0.015", "0.125", "2.5", "-2.5",
          "1e20", "123456789012345678", "0.1", "999.995", "1e-10", "-1234.5", "INFINITY", "-INFINITY", "NAN", "-NAN"]

FORMATS = [
    "%n", "%i", "%%", "a%nb", "%n %n", "[%10n]", "[%-10n]", "[%=*10n]", "[%#5n]", "[%#5.0n]",
    "[%=*#5n]", "[%=0#8n]", "[%#3n]", "[%.3n]", "[%.0n]", "[%.10n]", "[%^n]", "[%+n]", "[%(n]",
    "[%!n]", "[%(#5n]", "[%-(12n]", "[%-#5n]", "[%-=*#6.1n]", "[%^#10n]", "[%!#5i]",
    "[%11.2i]", "[%-15i]", "[%(i]", "[%+(n]", "%x", "%", "%5", "[%#n]", "[%.n]",
    "[%100n]", "[%01n]", "[%#0n]", "[%=n]",
]


def token(b: bytes) -> str:
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_str(b: bytes) -> str:
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def c_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def with_l(fmt: str) -> str:
    """`fmt` with `L` before each conversion's letter: a `long double` one."""
    out, i = [], 0
    while i < len(fmt):
        c = fmt[i]
        out.append(c)
        i += 1
        if c != "%":
            continue
        if i < len(fmt) and fmt[i] == "%":
            out.append("%")
            i += 1
            continue
        while i < len(fmt) and fmt[i] in "=^+(!-":
            if fmt[i] == "=" and i + 1 < len(fmt):
                out.append(fmt[i])
                i += 1
            out.append(fmt[i])
            i += 1
        while i < len(fmt) and (fmt[i].isdigit() or fmt[i] in "#."):
            out.append(fmt[i])
            i += 1
        if i < len(fmt) and fmt[i] in "ni":
            out.append("L")
    return "".join(out)


def cases():
    out = []
    for fmt in FORMATS:
        # Formats with two conversions take two values.
        for v in VALUES:
            vals = [v, "2.5"] if "%n %n" in fmt else [v]
            out.append((fmt, 100, "d", vals))
            lfmt = with_l(fmt)
            if lfmt != fmt:
                out.append((lfmt, 100, "L", vals))
    # The fits: each size across the edges, for a few.
    for fmt, v in [("%n", "1.23"), ("[%10n]", "-1.23"), ("%n %n", "1234.5"), ("abc", None),
                   ("[%-8n]", "5"), ("%100n", "1.23"), ("%%%n", "7")]:
        for size in range(0, 16):
            out.append((fmt, size, "d", [v] if v is not None else []))
    return out


def c_program(cs) -> str:
    L = [r'''#define _GNU_SOURCE
#include <errno.h>
#include <math.h>
#include <monetary.h>
#include <stdio.h>
#include <string.h>

static const char *name(int e)
{
    switch (e) {
    case 0: return "0";
    case E2BIG: return "E2BIG";
    case EINVAL: return "EINVAL";
    default: return "other";
    }
}

static void text(const char *s, size_t n)
{
    if (n == 0) {
        fputs("\\x", stdout);
        return;
    }
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char) s[i];
        if (c >= 0x21 && c <= 0x7e && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}

static char buf[512];

static void done(const char *head, ssize_t r)
{
    int e = errno;
    printf("%s = %zd %s ", head, r, name(e));
    text(buf, strnlen(buf, sizeof buf));
    putchar('\n');
}

int main(void)
{
''']
    for fmt, size, kind, vals in cs:
        head = " ".join([token(fmt.encode()), str(size), kind] + vals)
        args = ", ".join((f"(long double) {v}" if kind == "L" else f"(double) {v}") for v in vals)
        call = f"strfmon(buf, {size}, {c_str(fmt.encode())}{', ' + args if args else ''})"
        L.append(f'  memset(buf, 0, sizeof buf); errno = 0; done("{c_escape(head)}", {call});')
    L.append("  return 0;\n}")
    return "\n".join(L) + "\n"


def main() -> int:
    cs = cases()
    with workdir() as d:
        c = Path(d) / "strfmon.c"
        c.write_text(c_program(cs), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "strfmon")
        # -Wno-format: some formats here are wrong on purpose.
        r = run(f"gcc -O0 -Wall -Werror -Wno-format -Wno-format-security -o {exe} {wsl_path(c)} && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout[-4000:] + r.stderr[-4000:])
        return 1
    header = ("# glibc 2.39's strfmon in the C locale (posix/tools/oracle/strfmon_harness.py):\n"
              "# one line a call -- see the harness for the forms.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} calls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
