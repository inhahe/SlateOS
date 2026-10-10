"""glibc 2.39's printf for every length modifier, as the oracle for
posix/src/printf.rs.

    python posix/tools/oracle/printf_int_harness.py   # writes posix/src/printf_int_oracle.txt

The integer conversions `%d %i %u %o %x %X %b %B` with every length modifier
glibc knows -- `hh h l ll q L j z Z t`, and C23's `wN` and `wfN` -- over values
that the narrower types truncate; `%c` and `%lc`, `%s` and `%ls` (and their
`%C`, `%S` spellings); `%n` at every width; the `'` and `I` flags; flags,
widths and precisions on negative numbers; and positional arguments, `%n$`
and `*m$`. Each through `snprintf`, in the C.UTF-8 locale -- the one whose
`wcrtomb` this library's is.

One line a call:

    <format-hex> <args> | <ret> <output-hex> [<what %n stored>]

`<args>` is the argument list as the C caller passed it, comma-separated
`<type>:<value>`: `i` an `int`, `u` an `unsigned int`, `l` a 64-bit integer
(`long`, `long long`, `intmax_t`, `size_t`, `ptrdiff_t`), `w` a `wint_t`, `p`
a pointer (its address, hex: `p:0` is NULL), `s` a narrow string (hex), `S` a
wide string (its code points, hex, `.`-joined), `n<k>` a pointer to an object
of `%n`'s kind `k` (`hh h i l`), and `-` for none. The tests build each argument the way a C caller leaves it in its
register: an `int`'s upper 32 bits are whatever was there, which the
conversion must not read.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "printf_int_oracle.txt"

INT_MIN = -2147483648
LONG_MIN = -9223372036854775808
ULONG_MAX = 18446744073709551615

# (format, [(type, value)])
CASES: list[tuple[str, list[tuple[str, object]]]] = []


def case(fmt: str, *args: tuple[str, object]) -> None:
    CASES.append((fmt, list(args)))


# -- every conversion with every length, over values each length truncates --
SIGNED_VALUES = {
    "": [("i", -7), ("i", 0), ("i", 300), ("i", INT_MIN), ("i", 2147483647)],
    "hh": [("i", 300), ("i", -129), ("i", 255), ("i", 127), ("i", -7)],
    "h": [("i", 70000), ("i", -40000), ("i", 65535), ("i", -7)],
    "l": [("l", -7), ("l", LONG_MIN), ("l", 9223372036854775807)],
    "ll": [("l", -7), ("l", LONG_MIN)],
    "q": [("l", -7), ("l", LONG_MIN)],
    "L": [("l", -7), ("l", LONG_MIN)],
    "j": [("l", -7), ("l", LONG_MIN)],
    "z": [("l", -7), ("l", 4294967296)],
    "Z": [("l", -7)],
    "t": [("l", -7), ("l", LONG_MIN)],
    "w8": [("i", 300), ("i", -129)],
    "w16": [("i", 70000), ("i", -40000)],
    "w32": [("i", -7), ("i", INT_MIN)],
    "w64": [("l", -7), ("l", LONG_MIN)],
    "wf8": [("i", 300), ("i", -129)],
    "wf16": [("l", -7), ("l", 70000)],
    "wf32": [("l", -7), ("l", 4294967296)],
    "wf64": [("l", -7), ("l", LONG_MIN)],
}
for length, values in SIGNED_VALUES.items():
    for conv in "dixXuobB":
        for v in values:
            case(f"[%{length}{conv}]", v)

# -- flags, widths and precisions on negative and zero numbers --
for fmt in ["%+d", "% d", "%05d", "%-5d|", "%5d", "%.3d", "%8.3d", "%-8.3d|", "%+.0d",
            "%.0d", "%#x", "%#o", "%#X", "%#.0x", "%#.0o", "%#b", "%#B", "%08b",
            "%'d", "%'+10d", "%Id", "%'Id", "%-+'8d|"]:
    for v in [-7, 0, 1234567]:
        case(f"[{fmt}]", ("i", v))
for fmt in ["%+ld", "%020lld", "%-#20lo|", "%#lx", "%'ld"]:
    case(f"[{fmt}]", ("l", LONG_MIN))
    case(f"[{fmt}]", ("l", 1234567890123))
case("[%lu]", ("l", ULONG_MAX))
case("[%llx]", ("l", ULONG_MAX))
case("[%zu]", ("l", ULONG_MAX))
case("[%jo]", ("l", ULONG_MAX))

# -- %c and %lc, %s and %ls --
for fmt in ["%c", "%5c", "%-5c|", "%hhc"]:
    case(f"[{fmt}]", ("i", 65))
    case(f"[{fmt}]", ("i", 256 + 66))
for fmt in ["%lc", "%5lc", "%-5lc|", "%C"]:
    for w in [0x41, 0xE9, 0x20AC, 0x1F600, 0]:
        case(f"[{fmt}]", ("w", w))
case("[%lc]", ("w", 0xD800))
case("[%lc]", ("w", 0x110000))
for fmt in ["%s", "%10s", "%-10s|", "%.2s", "%10.2s"]:
    case(f"[{fmt}]", ("s", "hello"))
for fmt in ["%ls", "%10ls", "%-10ls|", "%.2ls", "%.3ls", "%.4ls", "%10.3ls", "%S"]:
    case(f"[{fmt}]", ("S", "h\u00e9llo\u20ac"))
case("[%ls]", ("S", ""))
case("[%ls]", ("S", "a\ud800b"))

# -- %n at every width --
for length, kind in [("hh", "hh"), ("h", "h"), ("", "i"), ("l", "l"), ("ll", "l"),
                     ("j", "l"), ("z", "l"), ("t", "l"), ("w8", "hh"), ("w16", "h"),
                     ("w32", "i"), ("w64", "l"), ("wf8", "hh"), ("wf16", "l")]:
    case(f"abcde%{length}n|", (f"n{kind}", 0))

# -- positional arguments --
case("%2$d %1$d", ("i", 1), ("i", 2))
case("%1$d %1$d %1$x", ("i", -7))
case("%2$s|%1$5d|%2$.2s", ("i", 42), ("s", "abc"))
case("[%3$*1$.*2$d]", ("i", 8), ("i", 4), ("i", -7))
case("[%1$*2$d]", ("i", -7), ("i", -6))
case("%3$ld %1$hhd %2$hd", ("i", 300), ("i", 70000), ("l", LONG_MIN))
case("%2$lc%1$ls", ("S", "\u00e9t\u00e9"), ("w", 0x20AC))
case("%1$s%2$n|", ("s", "abc"), ("ni", 0))

# -- a format that ends inside a conversion specification --
for fmt in ["abc%", "abc%l", "abc%5", "abc%q", "abc%-", "abc%.", "abc%2$", "abc%hh"]:
    case(fmt)

# -- odd ones --
case("[%w12d]", ("i", 5))
case("[%wd]", ("i", 5))
case("[%y]")
for fmt in ["[%5y]", "[%-#5.3y]", "[%ly]", "[%'+0y]", "[% Iy]", "[%.y]", "[%-0 +#y]"]:
    case(fmt)
case("[%*y]", ("i", 7))
case("[%y|%d]", ("i", 5))
case("[%1$y|%1$d]", ("i", 5))
case("[%5%]")
case("[%%]")

# -- what sends glibc's printf to printf_positional: a position (`%2$`), digits
#    a `$` ends after flags (`%-2$`, which it then reads as a width and an
#    unknown conversion `$`), or `*m$` -- whether or not the conversion takes
#    an argument. Read by position, a format cut short is written back. --
for fmt, args in [
    ("abc%1$5", []),
    ("[%*1$", [("i", 7)]),
    ("[%.*1$", [("i", 3)]),
    ("[%1$-+#5.3", []),
    ("[%1$hh", []),
    ("%1$d%", [("i", 5)]),
    ("%1$d%5", [("i", 5)]),
    ("%d %2$", [("i", 5), ("i", 6)]),
    ("[%02$d]", [("i", 1), ("i", 2)]),
    ("[%-2$d abc%", []),
    ("[%-2$d %d]", [("i", 5)]),
    ("[%-02$d abc%", []),
    ("[% 0$d abc%", []),
    ("[%-2l$d abc%", []),
    ("[%-2.3$d abc%", []),
    ("[%*0$d]", [("i", 5)]),
    ("[%*5d]", [("i", 3)]),
    ("[%1$y]", []),
    ("[%1$y abc%", []),
]:
    case(fmt, *args)

# -- numbers past INT_MAX, and a `*` of INT_MIN --
for fmt, args in [
    ("[%2147483648d]", [("i", 5)]),
    ("[%-99999999999d]", [("i", 5)]),
    ("[%.2147483648d]", [("i", 5)]),
    ("[%.2147483648s]", [("s", "abc")]),
    ("[%99999999999$d]", [("i", 5)]),
    ("[%*99999999999$d]", [("i", 5)]),
    ("[%*99999999999d]", [("i", 5)]),
    ("[%.*99999999999$d]", [("i", 5)]),
    ("[%.*99999999999d]", [("i", 5)]),
    ("[%2147483647.0y]", []),
    ("%1$d %2147483648d", [("i", 5)]),
    ("%1$d %.2147483648d", [("i", 5)]),
    ("%1$d %99999999999$d", [("i", 5)]),
    ("[%y %2147483648d]", [("i", 5)]),
    ("[%*d]", [("i", INT_MIN), ("i", 5)]),
    ("[%-*d]", [("i", INT_MIN), ("i", 5)]),
    ("[%.*d]", [("i", INT_MIN), ("i", 5)]),
]:
    case(fmt, *args)

# -- %p: `(nil)`, or `%#x` keeping the `+` and space flags; and NULL for %s
#    and %ls, `(null)` only when the precision leaves room for all of it --
for fmt in ["%p", "%10p", "%-10p|", "%.8p", "%+p", "% p", "%+ p", "%010p", "%#p",
            "%.2p", "%-+12.6p|", "%+012p", "%hhp", "%lp", "%hp"]:
    case(f"[{fmt}]", ("p", 0x1234))
    case(f"[{fmt}]", ("p", 0))
for fmt in ["%s", "%.3s", "%.5s", "%.6s", "%10s", "%-10.3s|", "%010s", "%ls", "%.3ls",
            "%.6ls", "%10ls"]:
    case(f"[{fmt}]", ("p", 0))

# -- after a lone `h`, glibc's first pass knows only `d i u o x X b B n %`:
#    the rest send it to its positional pass, where they format as without
#    the `h` -- and a format cut short is written back. `%m` of errno 0. --
case("[%hs abc%", ("s", "xy"))
case("[%hc abc%", ("i", 65))
case("[%hp abc%", ("p", 0x1234))
case("[%hC abc%", ("w", 0xE9))
case("[%hS abc%", ("S", "xy"))
case("[%hm abc%")
case("[%m]")
case("[%.3m|%-9m|%9m]")
# ... where these stay in the first pass, and fail:
case("[%hhs abc%", ("s", "xy"))
case("[%ls abc%", ("S", "xy"))
case("[%hn abc%", ("nh", 0))
case("[%hd abc%", ("i", 65))
case("[%hhc abc%", ("i", 65))
case("[%lc abc%", ("w", 0x41))


def c_arg(t: str, v: object, idx: int) -> tuple[str, str]:
    """(declarations, expression) for one argument."""
    if t == "i":
        return "", "(int)(-2147483647 - 1)" if v == INT_MIN else f"(int)({v})"
    if t == "u":
        return "", f"(unsigned)({v}U)"
    if t == "l":
        if v == LONG_MIN:
            return "", "(long)(-9223372036854775807L - 1)"
        if v == ULONG_MAX:
            return "", "(unsigned long)18446744073709551615UL"
        return "", f"(long)({v}L)"
    if t == "w":
        return "", f"(wint_t){v}u"
    if t == "p":
        return "", f"(void *){v:#x}UL"
    if t == "s":
        body = "".join(f"\\x{b:02x}" for b in str(v).encode("utf-8"))
        return "", f'"{body}"'
    if t == "S":
        body = "".join(f"\\x{ord(ch):x}\"L\"" for ch in str(v))
        return "", f'L"{body}"' if v else 'L""'
    if t.startswith("n"):
        ctype = {"hh": "signed char", "h": "short", "i": "int", "l": "long"}[t[1:]]
        return f"{ctype} nv{idx} = ({ctype})0x55;", f"&nv{idx}"
    raise ValueError(t)


def arg_token(t: str, v: object) -> str:
    if t in ("i", "u", "l", "w"):
        return f"{t}:{v}"
    if t == "p":
        return f"p:{v:x}"
    if t == "s":
        return "s:" + (str(v).encode("utf-8").hex() or "-")
    if t == "S":
        return "S:" + (".".join(f"{ord(ch):x}" for ch in str(v)) or "-")
    return f"{t}:0"


def main() -> None:
    lines = []
    for k, (fmt, args) in enumerate(CASES):
        decls, exprs, ns = [], [], []
        for i, (t, v) in enumerate(args):
            d, e = c_arg(t, v, i)
            if d:
                decls.append(d)
                ns.append(f"nv{i}")
            exprs.append(e)
        fmt_c = "".join(f"\\x{b:02x}" for b in fmt.encode("utf-8"))
        call = f'n = snprintf(buf, sizeof buf, "{fmt_c}"{"".join(", " + e for e in exprs)});'
        store = ""
        for nv in ns:
            store += f' printf(" %ld", (long){nv});'
        tokens = ",".join(arg_token(t, v) for t, v in args) or "-"
        lines.append(
            f"{{ {' '.join(decls)} errno = 0; {call} "
            f'printf("%s {tokens} | %d ", "{fmt.encode("utf-8").hex()}", n); '
            f'hex(buf, n); if (n < 0) printf("e%d", errno);{store} putchar(\'\\n\'); }}'
        )
    program = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

static void hex(const char *s, int n) {
    if (n <= 0) { fputs("-", stdout); return; }
    for (int i = 0; i < n; i++) printf("%02x", (unsigned char)s[i]);
}

int main(void) {
    if (!setlocale(LC_ALL, "C.UTF-8")) { fputs("no C.UTF-8\n", stderr); return 1; }
    char buf[512];
    int n;
''' + "\n    ".join(lines) + '''
    return 0;
}
'''
    with workdir() as d:
        d = Path(d)
        (d / "pi.c").write_text(program, encoding="utf-8", newline="\n")
        built = run(f"cd {wsl_path(d)} && gcc -O0 -Wno-format -Wno-format-security "
                    f"-Wno-format-extra-args -o pi pi.c 2>&1")
        if built.returncode:
            sys.exit(f"pi.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./pi")
        if r.returncode or r.stderr:
            sys.exit(f"pi: {r.stdout}{r.stderr}")
    OUT.write_text(r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(r.stdout.splitlines())} lines ({len(CASES)} cases)")


if __name__ == "__main__":
    main()
