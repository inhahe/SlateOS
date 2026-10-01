"""glibc 2.39's fnmatch, in the C locale, as the oracle for posix/src/fnmatch.rs.

    python posix/tools/oracle/fnmatch_harness.py   # writes posix/src/fnmatch_oracle.txt

The cases are every pattern of one or two TOKENS, and a list of longer ones
(LONG), against every one of STRINGS, under each of FLAGS. One line a pattern
and flag set:

    <flags> <pattern, escaped> <answers>

`answers` holds one character a string, in STRINGS' order: `0` for a match,
`1` for FNM_NOMATCH, `e` for anything else (glibc's -1, for a pattern it
cannot use). A pattern is escaped with `\\xNN` for a backslash, a space and
anything outside printable ASCII, so that a line splits on spaces.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "fnmatch_oracle.txt"
DEVIATIONS = POSIX_SRC / "fnmatch_deviations.txt"

# Pieces of patterns: literals of every kind the matcher treats alike or
# apart, the three wildcards, backslash escapes, and bracket expressions --
# negated, with ranges, with `]` first, with classes, equivalence classes and
# collating symbols, unterminated.
TOKENS = [
    "a", "b", "A", ".", "/", "-", "]", "[", "!", "^",
    "*", "?", "**",
    "\\", "\\*", "\\?", "\\[", "\\a", "\\/", "\\\\", "\\.",
    "[a]", "[!a]", "[^a]", "[a-c]", "[c-a]", "[]a]", "[!]a]", "[]]", "[!]]", "[a-]", "[-a]",
    "[!-]", "[\\]]", "[\\a]", "[a\\-c]", "[/]", "[.]", "[!.]", "[*]", "[?]", "[[]",
    "[[:alpha:]]", "[[:digit:]]", "[[:upper:]]", "[[:lower:]]", "[[:punct:]]", "[[:space:]]",
    "[![:alpha:]]", "[[:alpha:][:digit:]]", "[[:foo:]]", "[[:alpha:]", "[[:alpha]]",
    "[[=a=]]", "[[=b=]]", "[[.a.]]", "[[.-.]]", "[[.hyphen.]]", "[a-[.c.]]",
    "[a", "[!a", "[]", "[!]",
]

# Longer patterns: the shapes a two-token pattern cannot make.
LONG = [
    "*a*", "a*b", "*.*", "*/*", "a/*", "*/a", "a/b", "?/?", "*/", "/*",
    ".*", "*.", "a.*", "*a", "a*", "**a", "a**", "*?*", "?*?", "***",
    "a?b", "a[.]b", "a[/]b", "*[/]*", "a\\/b", "a*/b", "*a/b*", "a/*/b",
    ".a", "..", "./", "/.", "/.a", "a/.b", "a/.*", "a/?b", "*/.a", "*/.*",
    "[a-c]*", "*[a-c]", "[!a-c]*", "\\**", "*\\*", "a\\*b",
    "[[:alpha:]]*", "*[[:digit:]]", "[[:upper:]][[:lower:]]",
    "a*b*c", "*b*", "a*a*a", "*ab", "ab*",
]

STRINGS = [
    "", "a", "b", "c", "A", "B", ".", "..", "/", "-", "]", "[", "!", "^", "*", "?", "\\",
    "ab", "ba", "aa", "abc", "aab", "abb", "a.b", ".a", "a.", "a/b", "/a", "a/", "a/.b",
    "a/b/c", "a//b", "./a", "a/.", "x/y", "1", "5", " ", "\t", "Ab", "aB", "a-b", "-a",
    "a/ab", "ab/c", "a]", "]a", "c/a", "b/a", "abcabc",
]

FLAGS = [
    ("0", 0),
    ("PATHNAME", 1),
    ("NOESCAPE", 2),
    ("PERIOD", 4),
    ("PATHNAME|PERIOD", 5),
    ("PATHNAME|NOESCAPE", 3),
    ("LEADING_DIR", 8),
    ("PATHNAME|LEADING_DIR", 9),
    ("CASEFOLD", 16),
    ("PATHNAME|PERIOD|CASEFOLD", 21),
]

# FNM_EXTMATCH's patterns (ksh's), each alone and inside what a pattern
# around them can be, under the flags that change them.
EXT = [
    "?(a)", "*(a)", "+(a)", "@(a)", "!(a)",
    "?(a|b)", "*(a|b)", "+(a|b)", "@(a|b)", "!(a|b)",
    "?(ab|a)", "*(ab|a)", "+(ab|a)", "@(ab|a)", "!(ab|a)",
    "!(*)", "*(?)", "+(a*)", "@(a|)", "!()", "@()", "*()", "+(|a)",
    "@(", "*(a", "!(a|b", "?(a))", "@(a)(b)",
    "*(*)", "!(.*)", "@(.*)", "*(.)", "?(.)",
    "*(a/b)", "@(a/*)", "+(*/)", "!(a/b)", "@(/)",
    "*(\\))", "@(\\|)", "@([)])", "@([|])", "!([a])", "+([ab])",
    "?(*)", "+(?)", "!(!(a))", "@(@(a)|b)", "*(+(a)|b)", "!(a)*", "*!(a)",
]
EXT_AROUND = [("", ""), ("a", ""), ("", "b"), ("*", ""), ("", "*"), (".", ""), ("a/", "")]
EXT_FLAGS = [
    ("EXTMATCH", 32),
    ("EXTMATCH|PATHNAME", 33),
    ("EXTMATCH|PERIOD", 36),
    ("EXTMATCH|PATHNAME|PERIOD", 37),
    ("EXTMATCH|NOESCAPE", 34),
    ("EXTMATCH|CASEFOLD", 48),
    ("EXTMATCH|LEADING_DIR", 40),
]


def patterns() -> list[str]:
    seen, out = set(), []
    for p in TOKENS + [a + b for a in TOKENS for b in TOKENS] + LONG:
        if p not in seen:
            seen.add(p)
            out.append(p)
    return out


def ext_patterns() -> list[str]:
    seen, out = set(), []
    for pre, post in EXT_AROUND:
        for e in EXT:
            p = pre + e + post
            if p not in seen:
                seen.add(p)
                out.append(p)
    return out


def c_string(s: str) -> str:
    # Octal, not hex: C's hex escapes take every hex digit that follows, so
    # "\x5ca" is one character, 0x5ca, and not a backslash and an `a`. An
    # octal escape stops at three digits.
    return '"' + "".join(f"\\{ord(c):03o}" if c in '"\\' or not " " <= c <= "~" else c for c in s) + '"'


def escape(s: str) -> str:
    return "".join(f"\\x{ord(c):02x}" if c in "\\ " or not "!" <= c <= "~" else c for c in s)


def cases() -> list[tuple[str, int, str]]:
    """Every (flag set's name, its value, pattern) the oracle holds."""
    out = [(n, v, p) for n, v in FLAGS for p in patterns()]
    out += [(n, v, p) for n, v in EXT_FLAGS for p in ext_patterns()]
    return out


def main() -> None:
    all_cases = cases()
    lines = ["#define _GNU_SOURCE", "#include <fnmatch.h>", "#include <stdio.h>", ""]
    lines.append("static const char *P[] = {" + ", ".join(c_string(p) for _n, _v, p in all_cases) + "};")
    lines.append("static const int F[] = {" + ", ".join(str(v) for _n, v, _p in all_cases) + "};")
    lines.append("static const char *S[] = {" + ", ".join(c_string(s) for s in STRINGS) + "};")
    lines.append(r'''
int main(void)
{
    for (unsigned c = 0; c < sizeof P / sizeof *P; c++) {
        printf("%u ", c);
        for (unsigned s = 0; s < sizeof S / sizeof *S; s++) {
            int r = fnmatch(P[c], S[s], F[c]);
            putchar(r == 0 ? '0' : r == FNM_NOMATCH ? '1' : 'e');
        }
        putchar('\n');
    }
    return 0;
}
''')
    with workdir() as t:
        d = Path(t)
        (d / "fnm.c").write_text("\n".join(lines), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O1 -w -o fnm fnm.c && LC_ALL=C ./fnm")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
    out = [
        "# glibc 2.39's fnmatch in the C locale, for posix/src/fnmatch.rs.",
        "# Generated by posix/tools/oracle/fnmatch_harness.py; do not edit.",
        "# strings: " + " ".join(escape(s) or "\\x" for s in STRINGS),
    ]
    import fnmatch_model  # beside this file; see its docstring

    dev = [
        "# Every case of fnmatch_oracle.txt where posix/src/fnmatch.rs answers otherwise than",
        "# glibc 2.39 -- design-decisions section 1148 says why -- as fnmatch_model.py answers it:",
        "# <flags> <pattern> <string> <glibc's answer> <this library's>. Generated by",
        "# posix/tools/oracle/fnmatch_harness.py; do not edit.",
    ]
    for line in r.stdout.splitlines():
        c, answers = line.split(" ")
        name, value, pat = all_cases[int(c)]
        out.append(f"{name} {escape(pat)} {answers}")
        for s, glibc in zip(STRINGS, answers):
            ours = "0" if fnmatch_model.match(pat, 0, s, 0, len(s), value) else "1"
            if ours != glibc:
                dev.append(f"{name} {escape(pat)} {escape(s) or chr(92) + 'x'} {glibc} {ours}")
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    DEVIATIONS.write_text("\n".join(dev) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(out) - 3} lines ({len(patterns())} patterns x {len(FLAGS)} flag sets, "
          f"{len(ext_patterns())} extended ones x {len(EXT_FLAGS)}), each against {len(STRINGS)} strings")
    print(f"{DEVIATIONS.name}: {len(dev) - 4} cases where this library's answer is not glibc's")


if __name__ == "__main__":
    main()
