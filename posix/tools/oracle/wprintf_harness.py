"""glibc 2.39's wide printf, as the oracle for posix/src/printf.rs's wide
family (`swprintf`, `fwprintf`, ...).

    python posix/tools/oracle/wprintf_harness.py   # writes posix/src/wprintf_oracle.txt

What the wide family does differently from the narrow one, each through
`swprintf` in the C.UTF-8 locale: a width and a precision on `%s`, `%ls`,
`%c` and `%lc` count wide characters, not bytes; `%s`'s multibyte argument is
converted as `mbsrtowcs` converts it, and an invalid sequence before the
precision runs out fails the call; `%n` stores the count of wide characters
so far; a buffer too small is -1, not a length. And, so that the wide path is
seen to carry the narrow one's integer conversions, a few of those.

One line a call:

    <format> <size> <args> | <ret> <output> [<what %n stored>]

`<format>` and `<output>` are code points, hex, `.`-joined (`-` for none);
`<size>` is `swprintf`'s `n`; `<output>` is `-e<errno>` when `<ret>` is -1.
`<args>` is as `printf_int_harness.py` writes them: `i` an `int`, `l` a
64-bit integer, `w` a `wint_t`, `p` a pointer (hex, `p:0` is NULL), `s` a
narrow string (hex bytes), `S` a wide string (code points), `n<k>` a pointer
to `%n`'s object of kind `k` (`hh h i l`), `-` for none.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "wprintf_oracle.txt"

INT_MIN = -2147483648
LONG_MIN = -9223372036854775808

# (format, size, [(type, value)])
CASES: list[tuple[str, int, list[tuple[str, object]]]] = []


def case(fmt: str, *args: tuple[str, object], size: int = 512) -> None:
    CASES.append((fmt, size, list(args)))


HELLO = "h\u00e9llo\u20ac"           # h é l l o €: 6 characters, 9 bytes
HELLO_WIDE = "h\u00e9llo\u20ac\U0001F600"

# -- %s: a multibyte argument, its width and precision in characters --
for fmt in ["%s", "%10s", "%-10s|", "%.2s", "%.3s", "%10.2s", "%-10.2s|", "%.0s",
            "%.6s", "%.9s", "%*s", "%.*s"]:
    if "*" in fmt:
        case(f"[{fmt}]", ("i", 8), ("s", HELLO.encode()))
    else:
        case(f"[{fmt}]", ("s", HELLO.encode()))
# ... and one with an invalid sequence, before and after the precision runs out
for fmt in ["%s", "%.2s", "%.3s", "%5s", "%-5s|", "%.1s"]:
    case(f"[{fmt}]", ("s", b"ab\xffcd"))
case("[%s]", ("s", b"ab\xc3"))
case("[%.2s]", ("s", b"ab\xc3"))

# -- NULL --
for fmt in ["%s", "%.5s", "%.6s", "%10s", "%-10.3s|", "%ls", "%.3ls", "%.6ls", "%10ls"]:
    case(f"[{fmt}]", ("p", 0))

# -- %ls and %S: width and precision in characters --
for fmt in ["%ls", "%10ls", "%-10ls|", "%.2ls", "%.3ls", "%10.2ls", "%.0ls", "%S", "%5S",
            "%.7ls"]:
    case(f"[{fmt}]", ("S", HELLO_WIDE))
case("[%ls]", ("S", ""))

# -- %lc, %C and %c --
for fmt in ["%lc", "%5lc", "%-5lc|", "%C"]:
    for w in [0x41, 0xE9, 0x20AC, 0x1F600]:
        case(f"[{fmt}]", ("w", w))
for fmt in ["%c", "%5c", "%-5c|", "%hhc"]:
    case(f"[{fmt}]", ("i", 65))
case("[%c]", ("i", 0xE9))
case("[%lc]", ("w", 0))
case("[%lc]", ("w", 0xD800))
case("[%ls]", ("S", "a\ud800b"))

# -- %n counts wide characters --
case("h\u00e9llo%n|", ("ni", 0))
case("%ls%n|", ("S", "\u00e9\u20ac"), ("ni", 0))
case("%s%hhn|", ("s", "\u00e9\u00e9".encode()), ("nhh", 0))
case("\u00e9%5d%ln\u20ac", ("i", 42), ("nl", 0))

# -- the narrow engine's integers and positions, through the wide one --
case("[%d %hhd %ld %x]", ("i", -7), ("i", 300), ("l", LONG_MIN), ("i", INT_MIN))
case("[%2$ls %1$d]", ("i", 5), ("S", "\u00e9"))
case("[%1$s|%1$.1s]", ("s", "\u00e9a".encode()))
case("\u00e9[%5d]\u20ac", ("i", 42))
case("[%m]")
case("[%p %p]", ("p", 0x1234), ("p", 0))

# -- too small a buffer: -1, not the length --
case("hello", size=5)
case("hello", size=6)
case("h\u00e9llo\u20ac", size=6)
case("h\u00e9llo\u20ac", size=7)
case("[%s]", ("s", HELLO.encode()), size=8)
case("[%s]", ("s", HELLO.encode()), size=9)


def wide_literal(text: str) -> str:
    """A C wide string literal: each character an escape, closed after each,
    so a hex escape never runs into the next character."""
    if not text:
        return 'L""'
    return " ".join(f'L"\\x{ord(ch):x}"' for ch in text)


def c_arg(t: str, v: object, idx: int) -> tuple[str, str]:
    """(declarations, expression) for one argument."""
    if t == "i":
        return "", "(int)(-2147483647 - 1)" if v == INT_MIN else f"(int)({v})"
    if t == "l":
        if v == LONG_MIN:
            return "", "(long)(-9223372036854775807L - 1)"
        return "", f"(long)({v}L)"
    if t == "w":
        return "", f"(wint_t){v}u"
    if t == "p":
        return "", f"(void *){v:#x}UL"
    if t == "s":
        body = "".join(f"\\x{b:02x}" for b in bytes(v))
        return "", f'"{body}"'
    if t == "S":
        return "", wide_literal(str(v))
    if t.startswith("n"):
        ctype = {"hh": "signed char", "h": "short", "i": "int", "l": "long"}[t[1:]]
        return f"{ctype} nv{idx} = ({ctype})0x55;", f"&nv{idx}"
    raise ValueError(t)


def code_points(text: str) -> str:
    return ".".join(f"{ord(ch):x}" for ch in text) or "-"


def arg_token(t: str, v: object) -> str:
    if t in ("i", "l", "w"):
        return f"{t}:{v}"
    if t == "p":
        return f"p:{v:x}"
    if t == "s":
        return "s:" + (bytes(v).hex() or "-")
    if t == "S":
        return "S:" + code_points(str(v))
    return f"{t}:0"


def main() -> None:
    lines = []
    for fmt, size, args in CASES:
        decls, exprs, ns = [], [], []
        for i, (t, v) in enumerate(args):
            d, e = c_arg(t, v, i)
            if d:
                decls.append(d)
                ns.append(f"nv{i}")
            exprs.append(e)
        call = (f"n = swprintf(buf, {size}, {wide_literal(fmt)}"
                f"{''.join(', ' + e for e in exprs)});")
        store = "".join(f' printf(" %ld", (long){nv});' for nv in ns)
        tokens = ",".join(arg_token(t, v) for t, v in args) or "-"
        lines.append(
            f"{{ {' '.join(decls)} errno = 0; {call} "
            f'printf("%s {size} {tokens} | %d ", "{code_points(fmt)}", n); '
            f'wide(buf, n); if (n < 0) printf("-e%d", errno);{store} putchar(\'\\n\'); }}'
        )
    program = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdio.h>
#include <wchar.h>

static void wide(const wchar_t *s, int n) {
    if (n <= 0) { if (n == 0) fputs("-", stdout); return; }
    for (int i = 0; i < n; i++) printf(i ? ".%x" : "%x", (unsigned)s[i]);
}

int main(void) {
    if (!setlocale(LC_ALL, "C.UTF-8")) { fputs("no C.UTF-8\n", stderr); return 1; }
    wchar_t buf[512];
    int n;
''' + "\n    ".join(lines) + '''
    return 0;
}
'''
    with workdir() as d:
        d = Path(d)
        (d / "wp.c").write_text(program, encoding="utf-8", newline="\n")
        built = run(f"cd {wsl_path(d)} && gcc -O0 -Wno-format -Wno-format-security "
                    f"-Wno-format-extra-args -o wp wp.c 2>&1")
        if built.returncode:
            sys.exit(f"wp.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./wp")
        if r.returncode or r.stderr:
            sys.exit(f"wp: {r.stdout}{r.stderr}")
    OUT.write_text(r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(r.stdout.splitlines())} lines ({len(CASES)} cases)")


if __name__ == "__main__":
    main()
