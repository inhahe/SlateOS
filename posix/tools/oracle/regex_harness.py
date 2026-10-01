"""glibc 2.39's regcomp, regexec and regerror, in the C locale, as the oracle
for posix/src/regex.rs -- and regex_model.py's answers, the standard's, where
glibc's are not.

    python posix/tools/oracle/regex_harness.py
        # writes posix/src/regex_oracle.txt and posix/src/regex_deviations.txt

**The cases.** Seven sets, each a pattern and the subjects it is run
against:

- GEN: every one of PIECES, and every two of them, as an ERE and as a BRE,
  against every string of `a` and `b` up to four long -- the shapes of
  submatch the standard's rule and glibc's can differ on;
- CLASSIC: the long-known cases of that rule (Fowler's, Kuklewicz's), and
  repetitions of repetitions, each against its own strings;
- RANDOM: 1,200 patterns drawn from a grammar of alternations,
  concatenations and repetitions (the draw seeded), as an ERE and a BRE,
  against every string of `a`, `b` and `c` to length 3 and some longer;
- ICASE: CASE_PIECES, alone and in pairs, under REG_ICASE, against strings
  of both cases;
- TOKENS: every two of TOKENS, as an ERE and a BRE, against TOKEN_STRINGS:
  what the parser does with each special character wherever it can stand,
  and which error it gives a pattern it refuses;
- FILES: glibc's own tests -- posix/rxspencer/tests, PTESTS, TESTS,
  BOOST.tests and PCRE.tests, read as their runners read them, each subject
  on its own;
- EDGES: REG_NEWLINE, REG_NOTBOL, REG_NOTEOL, REG_STARTEND (with NULs in
  the subject), REG_NOSUB, every nmatch from 0 past re_nsub + 1, intervals
  at RE_DUP_MAX, back-references, bytes past 127, deep nesting, long
  patterns, and regerror at every code and buffer size.

**regex_oracle.txt**, one line a pattern (S) or a subject (I):

    S <set> <cflags> <pattern> <compiled> <answer per string of the set>
    I <cflags> <pattern> <compiled> <string> <eflags> <startend> <nmatch> <answer>
    E <code> <size> <returned> <what errbuf holds>

`compiled` is `e<code>` for a pattern regcomp refuses, else `n<re_nsub>`. An
S answer is `!` for REG_NOMATCH, else pmatch[0..re_nsub] as two digits a
pair, `__` for -1; an I answer is regexec's return and then every one of
its `nmatch` entries of pmatch as `so:eo`, a comma between -- including
those it leaves alone, which the harness filled with -7 (`startend` gives
pmatch[0] for REG_STARTEND, `-` for none). Patterns and strings are escaped:
`\\xNN` for a backslash, a space and anything outside printable ASCII, and
`\\-` alone for the empty string.

**regex_deviations.txt**: every case where glibc's answer is not the
standard's, as regex_model.py computes it, which is what posix/src/regex.rs
answers there -- design-decisions.md says why:

    <oracle line> <subject> <glibc's answer> <the model's>

`subject` is the string's index in its set (S), 0 (I), or `c` for regcomp's
result. The harness stops without writing anything if the model and glibc
disagree anywhere it does not expect them to: on a pattern's compiling,
other than under REG_ICASE, or on whether and where a subject matches at all,
other than for one of the reasons `explained` lists -- each a place glibc
contradicts its own answers elsewhere. Submatches alone may differ anywhere:
that is the rule the model is there for.

    REGEX_HARNESS_CACHE=<file> python posix/tools/oracle/regex_harness.py

keeps glibc's output and the model's answers in `<file>` between runs, so
that the comparison can be worked on without computing them again.
"""

import itertools
import multiprocessing
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402
import regex_model as M  # noqa: E402

OUT = POSIX_SRC / "regex_oracle.txt"
DEVIATIONS = POSIX_SRC / "regex_deviations.txt"
GLIBC_SRC = "$HOME/.cache/slateos-glibc-src/glibc-2.39/posix"

EXT, ICASE, NEWLINE, NOSUB = 1, 2, 4, 8
NOTBOL, NOTEOL, STARTEND = 1, 2, 4

# -- GEN ---------------------------------------------------------------------

PIECES = [
    "a", "b", ".", "a*", "a+", "a?", "b*", ".*",
    "(a)", "(b)", "(a*)", "(a+)", "(a?)", "(.)", "(.*)",
    "(a|b)", "(a|ab)", "(ab|a)", "(a*|b)", "(a|b*)", "(|a)", "(a|)", "()",
    "(a)*", "(a*)*", "(a+)*", "(a|b)*", "(a|ab)*", "(ab|a)*", "(a*)+", "(a|b)+", "(a*|b)*",
    "((a)|b)", "((a)|b)*", "(a|(b))*", "((a*)b)*", "(a(b)?)+",
    "a{2}", "a{1,2}", "(a){2}", "(a){0,2}", "(a*){2}", "(a|b){1,2}", "(a?){2,3}",
    "[ab]", "[^a]", "^", "$", "\\1",
    "\\b", "\\B", "\\<", "\\>", "\\w",
]
AB = [""] + ["".join(t) for n in range(1, 5) for t in itertools.product("ab", repeat=n)]

# Long-known cases of the rule, over more letters (Fowler's AT&T tests,
# Kuklewicz's), each against its own strings.
CLASSIC = [
    ("(a|ab)(c|bcd)(d*)", ["abcd"]),
    ("(a|ab)(bcd|c)(d*)", ["abcd"]),
    ("(ab|a)(c|bcd)(d*)", ["abcd"]),
    ("(ab|a)(bcd|c)(d*)", ["abcd"]),
    ("(a*)(b|abc)(c*)", ["abc"]),
    ("(a*)(abc|b)(c*)", ["abc"]),
    ("(a|ab)(bc|c)", ["abc"]),
    ("(a|ab|c|bcd)(d*)", ["abcd"]),
    ("(a|ab|c|bcd)*(d*)", ["abcd"]),
    ("((a)|b)+", ["ab", "ba"]),
    ("(a|(a*))+", ["aa"]),
    ("(.?)(b)", ["ab"]),
    ("(a*)(b{0,1})(b{1,})b{3}", ["aaabbbbbbb"]),
    ("(a|b)*c|(a|ab)*c", ["abc", "xc"]),
    ("(.a|.b).*|.*(.a|.b)", ["xa"]),
    ("(a*)*(x)", ["x", "ax", "axa"]),
    ("(a*)+(x)", ["x", "ax", "axa"]),
    ("(a*){2}(x)", ["x", "ax", "axa"]),
    ("((a*)*)*", ["", "a", "b"]),
    ("(a*|b)*", ["", "ab", "ba", "abab"]),
    ("(a+|b)*", ["", "ab"]),
    ("(a+|b){0,}", ["ab"]),
    ("(a+|b)+", ["ab"]),
    ("(a+|b){1,}", ["ab"]),
    ("(a+|b)?", ["ab"]),
    ("(a+|b){0,1}", ["ab"]),
    ("(^)*", ["-"]),
    ("($)|()", ["xxx"]),
    ("(a*)*|b", ["b"]),
    ("(.*)*", ["abc"]),
    ("(.*)(.*)", ["abc"]),
    ("(.*)(\\1)", ["abab", "aa", "aaaa"]),
    ("(a*)(\\1)", ["aaaa"]),
    ("(a*)\\1*", ["aaaa"]),
    ("(a|b)*\\1", ["abb", "aba"]),
    ("((a)|b)*\\2", ["aba", "abb"]),
    ("(([a-c])b*?\\2)*", ["ababbbcbc"]),
    ("((.)(.))*\\3", ["abcc"]),
    ("(y*)(x*)\\1", ["xy"]),
    ("(wee|week)(knights|night)", ["weeknights"]),
    ("(we|wee|week|frob)(knights|night|day)", ["weeknights"]),
    ("a(b|c)*d", ["abcd"]),
    ("a(b|c)+d", ["abcd"]),
    ("(a+)(b*)(c+)", ["aabbcc"]),
    ("(x|xx)(y|xy)*", ["xxy", "xxyy"]),
    ("(..)*(...)*", ["a", "abcd"]),
    ("(ab|a)(bc|c)", ["abc"]),
    ("(a)|(b)", ["b"]),
    ("(a)*(b)*", ["ab", "ba"]),
    ("(\\(|a)+", ["((a"]),
] + [
    # A repetition of a repetition: an iteration of the outer one can take
    # none of the inner one's, and so go through no group at all.
    (p, ["", "a", "aa", "ab", "aab", "b", "aba", "abab", "bab"])
    for p in ["(a)*{2}", "(a)*{2,3}", "(a)+{2}", "(a*)*{2}", "(ab|a)*{2}", "((a)|b)*{2}",
              "(a)?{3}", "(a){0,1}{2}", "(a|b)*{1,2}", "(a)**", "(a)+?", "(a)?*", "(a*){2}{2}",
              "((a)*b)*{2}", "(a)*{0,2}b", "(a(b)*)*{2}", "((a)?(b)?)*{2}"]
]

# -- RANDOM ------------------------------------------------------------------

# Deeper shapes than a pair of pieces can make: patterns drawn from a grammar
# of alternations, concatenations and repetitions, groups two deep and no
# pattern past 40 bytes (the draw seeded, so the list is the same every run),
# against every string of `a`, `b` and `c` to length 3 and some longer ones.
ABC = [""] + ["".join(t) for n in range(1, 4) for t in itertools.product("abc", repeat=n)] + [
    "abab", "abcabc", "aabbcc", "cbacba", "bcbc", "aaaa", "abcab", "cabbac", "abccba", "baaab"]


def random_patterns(count: int = 1200, seed: int = 137) -> list:
    import random

    rng = random.Random(seed)

    def atom(d):
        r = rng.random()
        if d <= 0 or r < 0.5:
            return rng.choice(["a", "b", "c", ".", "[ab]", "[^a]", "[a-c]"])
        if r < 0.9:
            return "(" + alt(d - 1) + ")"
        return rng.choice(["^", "$", "\\b", "\\B"])

    def piece(d):
        a = atom(d)
        if a in ("^", "$", "\\b", "\\B"):
            return a
        return a + rng.choice(["", "", "", "*", "+", "?", "{2}", "{1,2}", "{0,1}", "{2,}", "{0,2}"])

    def cat(d):
        return "".join(piece(d) for _ in range(rng.randint(1, 3)))

    def alt(d):
        return "|".join(cat(d) for _ in range(rng.choice([1, 1, 1, 2, 2, 3])))

    out, seen = [], set()
    while len(out) < count:
        p = alt(2)
        if "(" in p and rng.random() < 0.2:
            # A back-reference to the first group, after everything.
            p += "\\1"
        if len(p) <= 40 and p not in seen:
            seen.add(p)
            out.append(p)
    return out


# -- ICASE -------------------------------------------------------------------

CASE_PIECES = [
    "a", "A", "[a]", "[A]", "[a-b]", "[A-B]", "[^a]", "[^A]", "[[:lower:]]", "[[:upper:]]",
    "\\a", "\\A", ".", "a*", "[Z-a]", "[a-Z]", "[_-a]", "[`-{]", "[@-[]", "[[.a.]]",
    "[[=A=]]", "\\w", "(a)\\1", "(A)\\1", "[^[:lower:]]",
]
CASE_STRINGS = ["", "a", "A", "b", "B", "z", "Z", "_", "`", "[", "{", "@", "aa", "aA", "Aa",
                "AA", "ab", "Ab", "aB", "zZ"]

# -- TOKENS ------------------------------------------------------------------

TOKENS = [
    "a", "(", ")", "|", "*", "+", "?", "{", "}", "{1", "{1}", "{1,", "{1,2}", "{,2}",
    "{2,1}", "{,}", "{}", "{x}", "{1\\,2}", "{32767}", "{32768}", "^", "$", "[", "]", "[a",
    "[]", "[]a]", "[^]a]", "[a-]", "[-a]", "[z-a]", "[a-c-e]", "[--/]", "[[:alpha:]]",
    "[[:foo:]]", "[[:alpha:]", "[[.a.]]", "[[.ab.]]", "[[=a=]]", "[[.", "[[:", "[[=", "[[.-.]]",
    "\\", "\\(", "\\)", "\\{", "\\}", "\\|", "\\+", "\\?", "\\1", "\\2", "\\9", "\\a", "\\.",
    "\\*", "\\n", "\\w", "\\W", "\\s", "\\S", "\\b", "\\B", "\\<", "\\>", "\\`", "\\'", "()",
    ".", "a*", "\\{1\\}", "\\{1,2\\}", "\\{,2\\}",
]
TOKEN_STRINGS = [
    "", "a", "aa", "ab", "b", "(", ")", "a(", "(a)", "|", "a|b", "*", "a*", "+", "?", "{", "}",
    "a{1}", "{1}", "^", "$", "[", "]", "\\", "-", "1", ",", "x", "a\\", "^a$", ".", "\n",
    "a\nb", "a b",
]

# -- EDGES -------------------------------------------------------------------


def edges():
    """(cflags, pattern, [(string, eflags, startend, nmatch)]) -- nmatch None
    for re_nsub + 1."""
    out = []

    def add(cflags, pattern, subjects):
        out.append((cflags, pattern.encode("latin-1") if isinstance(pattern, str) else pattern,
                    [(s.encode("latin-1") if isinstance(s, str) else s, e, se, nm)
                     for s, e, se, nm in subjects]))

    nl_strings = ["a\nb", "\na", "b\na\n", "\n", "a\n", "ab\nab", "x\nx"]
    for cf in (NEWLINE, NEWLINE | EXT, 0, EXT):
        for p in ["^a", "a$", "^$", ".", "[^a]", "a.b", "\\W", "\\s", "^", "$", "b$", "^b",
                  "\\`a", "b\\'", "a\\'", "\\ba", "\\<b", "\\>", "x", ".*", "[^x]*", "\\S*",
                  "(^|a)b" if cf & EXT else "\\(^\\|a\\)b"]:
            add(cf, p, [(s, ef, None, None) for s in nl_strings for ef in (0, NOTBOL, NOTEOL)])
    for cf in (0, EXT):
        for p in ["^a", "a$", "^$", "\\`a", "a\\'", "\\`", "\\'", "\\ba", "a\\b", "\\<a", "a\\>",
                  "\\B", "^", "$", "\\Ba", "a\\B"]:
            add(cf, p, [(s, ef, None, None) for s in ["", "a", "ba", "ab", " a", "a "]
                        for ef in (0, NOTBOL, NOTEOL, NOTBOL | NOTEOL)])
    # REG_STARTEND: offsets into a subject with NULs and newlines in it.
    sub = b"xa\x00ab\nab"
    ranges = [(0, len(sub)), (1, 3), (2, 5), (0, 0), (3, 3), (1, 2), (3, 8), (6, 8), (5, 6), (8, 8)]
    for cf in (EXT, EXT | NEWLINE, EXT | NOSUB, 0):
        for p in ["a", "^a", "a$", "\\`a", "b\\'", ".", "a.b", "\\ba", "\\<a", "x*", "", "[^b]",
                  "(a)(b)" if cf & EXT else "\\(a\\)\\(b\\)", "\\bb", "b\\>", "\\B", "$", "^"]:
            add(cf, p, [(sub, ef, r, None) for r in ranges for ef in (STARTEND, STARTEND | NOTBOL,
                                                                     STARTEND | NOTEOL)])
    # nmatch from 0 past re_nsub + 1, with and without REG_NOSUB.
    for cf in (EXT, EXT | NOSUB):
        for p in ["(a)(b)?", "(a)|(b)", "a", "()", "((a)(b))"]:
            add(cf, p, [(s, 0, None, nm) for s in ["ab", "b", "a", "x"] for nm in (0, 1, 2, 3, 5)])
    add(EXT, "a", [("a", 8, None, None), ("a", 16, None, None), ("ba", 7, (0, 2), None),
                   ("a", 9, None, None)])
    # Intervals at RE_DUP_MAX, and the empty repetitions.
    for cf in (EXT, 0):
        br = (lambda s: s) if cf & EXT else (lambda s: s.replace("{", "\\{").replace("}", "\\}")
                                                          .replace("(", "\\(").replace(")", "\\)"))
        for p in ["a{32767}", "a{32768}", "a{1,32767}", "a{1,32768}", "a{0,32767}",
                  "a{99999999999}", "a{0}", "a{0,0}b", "(a){0}", "(a){0}b", "(a){0,0}\\1",
                  "a{0}*", "(a)(b){0}", "a{3}", "a{2,}", "a{,}", "(ab){2}", "(a|b){3,}"]:
            add(cf, br(p), [(s, 0, None, None) for s in ["", "a", "b", "aaa", "aaaa", "ab", "abab",
                                                         "ba", "aaaaaaaaaa"]])
    # Back-references.
    for cf in (EXT, 0, EXT | ICASE, ICASE):
        br = (lambda s: s) if cf & EXT else (lambda s: re.sub(r"([(){}|+?])", r"\\\1", s))
        for p in ["(a*)\\1", "(a|b)*\\1", "((a)|b)*\\2", "(a)|\\1", "(a)\\1|b", "(a)(b)\\2\\1",
                  "(.)\\1", "(.*)\\1", "(a*)*\\1", "(a*)+\\1", "()\\1", "(a)\\2", "(a)(\\1)",
                  "(\\1)", "((a)\\2)", "(a)\\1{2}", "(a)\\1*", "(ab*)\\1", "(.)(.)\\2\\1"]:
            add(cf, br(p), [(s, 0, None, None) for s in ["", "a", "aa", "aA", "Aa", "aaa", "abab",
                                                         "abba", "ab", "ba", "aba", "abb",
                                                         "abcab", "xyyx"]])
    # Bytes past 127, which the C locale gives no class.
    hi = [b"\xe9", b"a\xe9b", b"\x80", b"\xff", b"\xe0\xff"]
    for cf in (EXT, EXT | ICASE):
        for p in [b"\xe9", b"[\xe0-\xff]", b".", b"[[:alpha:]]", b"\\w", b"[^a]", b"\\W",
                  b"[\x80-\xff]+", b"[[:print:]]", b"[[:cntrl:]]", b"[[:punct:]]"]:
            add(cf, p, [(s, 0, None, None) for s in hi])
    # Nesting deep, and patterns long.
    for depth in (50, 200):
        add(EXT, "(" * depth + "a" + ")" * depth, [("a", 0, None, None), ("b", 0, None, None)])
        add(0, "\\(" * depth + "a" + "\\)" * depth, [("a", 0, None, None)])
    add(EXT, "a" * 5000, [("a" * 5000, 0, None, None), ("a" * 4999, 0, None, None)])
    add(EXT, "(a|b)" * 300, [("ab" * 150, 0, None, None)])
    add(EXT, "x*" * 200 + "y", [("x" * 100 + "y", 0, None, None)])
    add(EXT, "[" + "a" * 3000 + "]", [("a", 0, None, None)])
    # The empty pattern, and one of nothing but anchors.
    for cf in (0, EXT):
        for p in ["", "^$", "^*", "$*" if cf else "$$", "()", "(()|a)*" if cf else "\\(\\(\\)\\|a\\)*"]:
            add(cf, p, [(s, 0, None, None) for s in ["", "a", "*", "$", "aa"]])
    return out


# -- FILES -------------------------------------------------------------------


def files() -> list:
    """glibc's test files' cases, read as their runners read them."""
    r = run(f"cd {GLIBC_SRC} && for f in rxspencer/tests PTESTS TESTS BOOST.tests PCRE.tests; "
            f"do echo \"@@@ $f\"; cat \"$f\"; done")
    if r.returncode != 0:
        sys.exit(f"cannot read glibc's test files: {r.stderr}")
    parts = {}
    cur = None
    for line in r.stdout.split("\n"):
        if line.startswith("@@@ "):
            cur = line[4:]
            parts[cur] = []
        elif cur is not None:
            parts[cur].append(line)
    out = []
    out += _rxspencer(parts["rxspencer/tests"])
    out += _ptests(parts["PTESTS"])
    out += _tests(parts["TESTS"])
    out += _boost(parts["BOOST.tests"])
    out += _pcre(parts["PCRE.tests"])
    return out


def _nstz(s: str) -> bytes:
    return s.replace("N", "\n").replace("T", "\t").replace("S", " ").replace("Z", "\0") \
        .encode("latin-1")


def _rxspencer(lines):
    out = []
    for line in lines:
        if not line or line.startswith("#"):
            continue
        f = [x for x in line.split("\t") if x != ""]
        if len(f) < 3:
            continue
        pat, flags, string = f[0], f[1], f[2]
        if pat == '""':
            pat = ""
        if string == '""':
            string = ""
        if any(c in flags for c in "mp#"):
            continue
        cf, ef, both, fails = EXT, 0, False, False
        for c in flags:
            if c == "b":
                cf &= ~EXT
            elif c == "&":
                both = True
            elif c == "C":
                fails = True
            elif c == "i":
                cf |= ICASE
            elif c == "s":
                cf |= NOSUB
            elif c == "n":
                cf |= NEWLINE
            elif c == "^":
                ef |= NOTBOL
            elif c == "$":
                ef |= NOTEOL
        p = _nstz(pat)
        p = re.sub(rb"\[\[:([<>]):\]\]", rb"\\\1", p)
        subj = [] if fails else [(_nstz(string), ef, None, None)]
        for c in ([cf, cf & ~EXT] if both else [cf]):
            if b"\0" in p:
                continue
            out.append((c, p, subj))
    return out


def _ptests(lines):
    out = []
    for line in lines:
        if not line or line.startswith("#"):
            continue
        f = line.split("|")
        if len(f) < 5:
            continue
        opts = f[4].strip()
        cf = 0
        for name, v in (("REG_EXTENDED", EXT), ("REG_ICASE", ICASE), ("REG_NEWLINE", NEWLINE),
                        ("REG_NOSUB", NOSUB)):
            if name in opts:
                cf |= v
        out.append((cf, f[2].encode("latin-1"), [(f[3].encode("latin-1"), 0, None, None)]))
    return out


def _tests(lines):
    out = []
    for line in lines:
        if not line:
            continue
        f = line.split(":")
        if len(f) < 3:
            continue
        out.append((EXT, f[1].encode("latin-1"),
                    [(":".join(f[2:]).encode("latin-1"), 0, None, None)]))
    return out


def _frob(s: str, pattern: bool) -> str:
    out, i = [], 0
    while i < len(s):
        if s[i] == "\\" and i + 1 < len(s):
            c = s[i + 1]
            if c in "tnr":
                out.append({"t": "\t", "n": "\n", "r": "\r"}[c])
                i += 2
                continue
            if c in "\\^{|}" and not pattern:
                out.append(c)
                i += 2
                continue
        out.append(s[i])
        i += 1
    return "".join(out)


def _boost(lines):
    out = []
    cf, ef = EXT, 0
    for line in lines:
        if not line or line.startswith(";"):
            continue
        if line.startswith("-"):
            cf = 0 if "REG_BASIC" in line else EXT
            if "REG_ICASE" in line:
                cf |= ICASE
            if "REG_NEWLINE" in line:
                cf |= NEWLINE
            ef = 0
            if "REG_NOTBOL" in line:
                ef |= NOTBOL
            if "REG_NOTEOL" in line:
                ef |= NOTEOL
            continue
        s = line.lstrip(" \t")
        if not s:
            continue
        m = re.match(r"([^ \t]*)[ \t]+(.*)", s)
        if not m:
            continue
        pat, rest = m.group(1), m.group(2).lstrip(" \t")
        if not rest:
            continue
        if rest.startswith('"'):
            end = rest.find('"', 1)
            if end < 0:
                continue
            string = rest[1:end]
        else:
            tok = re.match(r"[^ \t]*", rest).group(0)
            if tok.startswith("!"):
                string = None
            elif len(tok) == len(rest):
                continue
            else:
                string = tok
        p = _frob(pat, True).encode("latin-1")
        subj = [] if string is None else [(_frob(string, False).encode("latin-1"), ef, None, None)]
        out.append((cf, p, subj))
    return out


def _pcre(lines):
    out = []
    pat, icase = None, False
    for line in lines[1:]:
        if line.startswith("#"):
            continue
        if line == "":
            pat = None
            continue
        if line.startswith("/"):
            end = line.rfind("/")
            if end <= 0:
                pat = None
                continue
            flags = line[end + 1:]
            if flags not in ("", "i"):
                pat = None
                continue
            pat, icase = line[1:end], flags == "i"
            continue
        if line.startswith("    ") and pat is not None:
            out.append((EXT | (ICASE if icase else 0), pat.encode("latin-1"),
                        [(line[4:].encode("latin-1"), 0, None, None)]))
    return out


# -- the C side --------------------------------------------------------------


def to_bre(p: str) -> str:
    """An ERE piece as the same BRE: the operators escaped, the escapes kept."""
    out, i = [], 0
    while i < len(p):
        if p[i] == "\\":
            out.append(p[i:i + 2])
            i += 2
            continue
        out.append("\\" + p[i] if p[i] in "(){}|+?" else p[i])
        i += 1
    return "".join(out)


def c_bytes(b: bytes) -> str:
    return '"' + "".join(f"\\{c:03o}" if c in b'"\\?' or not 0x20 <= c <= 0x7E else chr(c)
                         for c in b) + '"'


def escape(b: bytes) -> str:
    if not b:
        return "\\-"
    return "".join(f"\\x{c:02x}" if c in b"\\ " or not 0x21 <= c <= 0x7E else chr(c) for c in b)


def s_cases():
    """(set name, cflags, pattern bytes)."""
    out = []
    pats = PIECES + [a + b for a in PIECES for b in PIECES]
    for cf in (EXT, 0):
        for p in pats:
            out.append(("ab", cf, (p if cf & EXT else to_bre(p)).encode("latin-1")))
    cps = CASE_PIECES + [a + b for a in CASE_PIECES for b in CASE_PIECES]
    for cf in (EXT | ICASE, ICASE):
        for p in cps:
            out.append(("case", cf, (p if cf & EXT else to_bre(p)).encode("latin-1")))
    toks = TOKENS + [a + b for a in TOKENS for b in TOKENS]
    for cf in (EXT, 0):
        for p in toks:
            out.append(("tok", cf, p.encode("latin-1")))
    for p in random_patterns():
        for cf in (EXT, 0):
            out.append(("abc", cf, (p if cf & EXT else to_bre(p)).encode("latin-1")))
    return out


SETS = {"ab": AB, "case": CASE_STRINGS, "tok": TOKEN_STRINGS, "abc": ABC}


def i_cases():
    """(cflags, pattern, [(string, eflags, startend, nmatch)])."""
    out = []
    for p, strings in CLASSIC:
        for cf, pp in ((EXT, p), (0, to_bre(p))):
            out.append((cf, pp.encode("latin-1"), [(s.encode("latin-1"), 0, None, None)
                                                    for s in strings]))
    return out + edges() + files()


C_MAIN = r'''
static void pairs(const regmatch_t *m, size_t n)
{
    for (size_t k = 0; k < n; k++) {
        if (m[k].rm_so < 0 && m[k].rm_eo < 0)
            fputs("__", stdout);
        else if (m[k].rm_so > 9 || m[k].rm_eo > 9)
            printf("?");
        else
            printf("%d%d", (int) m[k].rm_so, (int) m[k].rm_eo);
    }
}

static void s_case(unsigned i)
{
    static regmatch_t m[256];
    regex_t re;
    int rc = regcomp(&re, SP[i], SF[i]);
    if (rc) {
        printf("S%u e%d\n", i, rc);
        return;
    }
    printf("S%u n%zu", i, re.re_nsub);
    const char *const *set = SETS[SS[i]];
    for (unsigned j = 0; set[j]; j++) {
        size_t nm = re.re_nsub + 1;
        int r = regexec(&re, set[j], nm, m, 0);
        if (r == REG_NOMATCH)
            fputs(" !", stdout);
        else if (r)
            printf(" x%d", r);
        else {
            putchar(' ');
            pairs(m, nm);
        }
    }
    putchar('\n');
    regfree(&re);
}

static void i_case(unsigned i)
{
    static regmatch_t m[256];
    regex_t re;
    int rc = regcomp(&re, IP[i], IF[i]);
    if (rc) {
        printf("I%u e%d\n", i, rc);
        return;
    }
    printf("I%u n%zu", i, re.re_nsub);
    for (unsigned j = IFIRST[i]; j < IFIRST[i + 1]; j++) {
        size_t nm = JN[j] < 0 ? re.re_nsub + 1 : (size_t) JN[j];
        if (nm > 256)
            nm = 256;
        for (size_t k = 0; k < 256; k++)
            m[k].rm_so = m[k].rm_eo = -7;
        if (JSO[j] >= 0) {
            m[0].rm_so = JSO[j];
            m[0].rm_eo = JEO[j];
        }
        int r = regexec(&re, JS[j], nm, m, JE[j]);
        printf(" %d", r);
        for (size_t k = 0; k < nm; k++)
            printf("%c%ld:%ld", k ? ',' : '=', (long) m[k].rm_so, (long) m[k].rm_eo);
    }
    putchar('\n');
    regfree(&re);
}

/* Each pattern in a child of its own: glibc crashes on some, and a crash
 * must cost that pattern's answers and no others'. */
static void isolated(void (*f)(unsigned), unsigned i, char kind)
{
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        alarm(60);
        f(i);
        fflush(stdout);
        _exit(0);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    if (WIFSIGNALED(st))
        printf("%c%u crash%d\n", kind, i, WTERMSIG(st));
    else if (!WIFEXITED(st) || WEXITSTATUS(st) != 0)
        printf("%c%u crash-exit%d\n", kind, i, WEXITSTATUS(st));
}

int main(void)
{
    for (unsigned i = 0; i < sizeof SP / sizeof *SP; i++)
        isolated(s_case, i, 'S');
    for (unsigned i = 0; i < sizeof IP / sizeof *IP; i++)
        isolated(i_case, i, 'I');
    fflush(stdout);
    static char buf[256];
    for (int code = 0; code <= 16; code++) {
        static const size_t sizes[] = {0, 1, 2, 5, 16, 100};
        for (unsigned z = 0; z < sizeof sizes / sizeof *sizes; z++) {
            memset(buf, '#', sizeof buf);
            buf[sizeof buf - 1] = 0;
            size_t got = regerror(code, NULL, buf, sizes[z]);
            printf("E %d %zu %zu ", code, sizes[z], got);
            for (size_t k = 0; k < sizes[z] + 2 && k < sizeof buf; k++)
                printf("%02x", (unsigned char) buf[k]);
            putchar('\n');
        }
    }
    return 0;
}
'''


def build_c(scases, icases) -> str:
    lines = ["#define _GNU_SOURCE", "#include <regex.h>", "#include <stdio.h>",
             "#include <string.h>", "#include <sys/wait.h>", "#include <unistd.h>", ""]
    names = list(SETS)
    for n in names:
        lines.append(f"static const char *const SET_{n}[] = {{"
                     + ", ".join(c_bytes(s.encode("latin-1")) for s in SETS[n]) + ", 0};")
    lines.append("static const char *const *const SETS[] = {"
                 + ", ".join(f"SET_{n}" for n in names) + "};")
    lines.append("static const char *const SP[] = {" + ", ".join(c_bytes(p) for _s, _c, p in scases)
                 + "};")
    lines.append("static const int SF[] = {" + ", ".join(str(c) for _s, c, _p in scases) + "};")
    lines.append("static const unsigned char SS[] = {"
                 + ", ".join(str(names.index(s)) for s, _c, _p in scases) + "};")
    firsts, js, je, jso, jeo, jn = [], [], [], [], [], []
    for cf, p, subj in icases:
        firsts.append(len(js))
        for s, e, se, nm in subj:
            js.append(s)
            je.append(e)
            jso.append(-1 if se is None else se[0])
            jeo.append(-1 if se is None else se[1])
            jn.append(-1 if nm is None else nm)
    firsts.append(len(js))
    lines.append("static const char *const IP[] = {" + ", ".join(c_bytes(p) for _c, p, _s in icases)
                 + "};")
    lines.append("static const int IF[] = {" + ", ".join(str(c) for c, _p, _s in icases) + "};")
    lines.append("static const unsigned IFIRST[] = {" + ", ".join(map(str, firsts)) + "};")
    lines.append("static const char *const JS[] = {" + ", ".join(c_bytes(s) for s in js) + ", 0};")
    lines.append("static const int JE[] = {" + ", ".join(map(str, je)) + ", 0};")
    lines.append("static const long JSO[] = {" + ", ".join(map(str, jso)) + ", 0};")
    lines.append("static const long JEO[] = {" + ", ".join(map(str, jeo)) + ", 0};")
    lines.append("static const int JN[] = {" + ", ".join(map(str, jn)) + ", 0};")
    lines.append(C_MAIN)
    return "\n".join(lines)


# -- the model's answers -----------------------------------------------------


def s_pairs(pm) -> str:
    return "".join("__" if so < 0 else f"{so}{eo}" for so, eo in pm)


def model_s(args):
    """The model's (compiled, [answer per string]) for an S case."""
    set_name, cf, p = args
    try:
        rx = M.compile(p, cf)
    except M.Error as e:
        return f"e{e.code}", []
    answers = []
    for s in SETS[set_name]:
        pm = rx.exec(s.encode("latin-1"))
        answers.append("!" if pm is None else s_pairs(pm))
    return f"n{rx.re_nsub}", answers


def model_i(args):
    """The model's (compiled, [answer per subject]) for an I case."""
    cf, p, subj = args
    try:
        rx = M.compile(p, cf)
    except M.Error as e:
        return f"e{e.code}", []
    answers = []
    for s, ef, se, nm in subj:
        n = rx.re_nsub + 1 if nm is None else min(nm, 256)
        m = [(-7, -7)] * n
        if se is not None and n:
            m[0] = se
        if ef & ~(NOTBOL | NOTEOL | STARTEND):
            # regexec's REG_BADPAT for eflags it does not know.
            answers.append(i_answer(M.REG_BADPAT, m))
            continue
        if ef & STARTEND:
            so, eo = se
            pm = rx.exec(s, ef, so, eo) if 0 <= so <= eo <= len(s) else None
        else:
            z = s.find(b"\0")
            pm = rx.exec(s[:z] if z >= 0 else s, ef)
        if pm is None:
            answers.append(i_answer(1, m))
            continue
        if not cf & NOSUB:
            for k in range(n):
                m[k] = pm[k] if k < len(pm) else (-1, -1)
        answers.append(i_answer(0, m))
    return f"n{rx.re_nsub}", answers


def explained(cf: int, p: bytes, s: bytes):
    """Why glibc and the model may disagree on whether, or where, `p` matches
    `s` at all -- or None, if they should not.

    - a back-reference: glibc misses matches, and reports half-set pairs;
    - REG_ICASE: glibc upper-cases the pattern, so `\\\\a` loses its case and
      `[`-{]` holds no letter it will see;
    - a newline without REG_NEWLINE: glibc's automaton takes one for a line
      boundary between two parts of a pattern (`$.` matches "\\\\n") though
      not at its end (`a$` does not match "a\\\\nb");
    - `\\\\B` after a star: glibc's `a*\\\\B` matches "ba" at 2, where its own
      `a*\\\\B$` finds `\\\\B` does not hold;
    - `\\\\B` with a repetition: glibc's `\\\\Ba` does not match "a", nor does
      its `(\\\\Ba)?`, but `(\\\\Ba){0,2}` does;
    - `^` inside a repeated group: glibc's `(^[a-c]{0,2}){0,2}.{2,}` matches
      nothing in "aaa", and with `|[^a]{0,1}` after it -- which matches the
      empty string anywhere -- still nothing."""
    if re.search(rb"\\[1-9]", p):
        return "back-reference"
    if cf & ICASE:
        return "REG_ICASE"
    if not cf & NEWLINE and b"\n" in s and re.search(rb"[$^]", p):
        return "a newline taken for a line boundary"
    if b"*\\B" in p:
        return "a star before \\B"
    if b"\\B" in p and re.search(rb"[*+?}]", p):
        return "\\B with a repetition"
    if (b"(^" in p or b"\\(^" in p) and re.search(rb"[*+?}]", p):
        return "^ inside a repeated group"
    return None


def i_answer(r: int, m) -> str:
    return str(r) + "".join(("," if k else "=") + f"{so}:{eo}" for k, (so, eo) in enumerate(m))


def main() -> None:
    import os
    import pickle

    scases, icases = s_cases(), i_cases()
    # REGEX_HARNESS_CACHE names a file to keep glibc's output and the model's
    # answers in between runs, while the comparison below is worked on.
    cache = os.environ.get("REGEX_HARNESS_CACHE")
    cached = None
    if cache and Path(cache).exists():
        cached = pickle.loads(Path(cache).read_bytes())
    if cached is None:
        src = build_c(scases, icases)
        with workdir() as t:
            d = Path(t)
            (d / "rx.c").write_text(src, encoding="utf-8", newline="\n")
            r = run(f"cd {wsl_path(d)} && gcc -O1 -w -o rx rx.c && LC_ALL=C ./rx")
            if r.returncode != 0:
                sys.exit(f"the harness failed:\n{r.stderr[-3000:]}\n{r.stdout[-2000:]}")
        stdout = r.stdout
    else:
        stdout = cached["stdout"]
    glibc_s, glibc_i, errs = {}, {}, []
    for line in stdout.split("\n"):
        if not line:
            continue
        if line.startswith("E "):
            errs.append(line)
            continue
        head, *rest = line.split(" ")
        idx = int(head[1:])
        (glibc_s if head[0] == "S" else glibc_i)[idx] = (rest[0], rest[1:])

    print(f"glibc: {len(scases)} patterns against string sets, {len(icases)} with their own "
          f"subjects", flush=True)
    if cached is not None and "model_s" in cached and "--remodel" not in sys.argv:
        model_s_all, model_i_all = cached["model_s"], cached["model_i"]
    else:
        with multiprocessing.Pool(4) as pool:
            model_s_all = pool.map(model_s, scases, chunksize=64)
            print("model: string sets done", flush=True)
            model_i_all = pool.map(model_i, icases, chunksize=16)
            print("model: the rest done", flush=True)
    if cache:
        Path(cache).write_bytes(pickle.dumps({"stdout": stdout, "model_s": model_s_all,
                                              "model_i": model_i_all}))

    out = [
        "# glibc 2.39's regcomp, regexec and regerror in the C locale, for posix/src/regex.rs.",
        "# Generated by posix/tools/oracle/regex_harness.py; do not edit.",
    ]
    for n, strings in SETS.items():
        out.append(f"# strings {n}: " + " ".join(escape(s.encode("latin-1")) for s in strings))
    dev = [
        "# Every case of regex_oracle.txt where glibc 2.39's answer is not the standard's, as",
        "# posix/tools/oracle/regex_model.py computes it and posix/src/regex.rs answers:",
        "# <oracle line> <subject> <glibc's answer> <the model's>. Generated by",
        "# posix/tools/oracle/regex_harness.py; do not edit.",
    ]
    surprises = []
    kinds = {}

    def note(kind, detail):
        kinds.setdefault(kind, []).append(detail)

    for i, (set_name, cf, p) in enumerate(scases):
        comp, answers = glibc_s[i]
        mcomp, manswers = model_s_all[i]
        line_no = len(out) + 1
        out.append(f"S {set_name} {cf} {escape(p)} {comp}" + "".join(" " + a for a in answers))
        strings = SETS[set_name]
        if comp.startswith("crash"):
            # glibc died on it: what it would have answered is the standard's.
            note("glibc crashed", f"{cf} {p!r}")
            dev.append(f"{line_no} c {comp} {mcomp}")
            for j, ma in enumerate(manswers):
                dev.append(f"{line_no} {j} - {ma}")
            continue
        if comp != mcomp:
            dev.append(f"{line_no} c {comp} {mcomp}")
            if cf & ICASE:
                note("compile/icase", f"{cf} {p!r}: glibc {comp}, model {mcomp}")
            else:
                surprises.append(f"compile {cf} {p!r}: glibc {comp}, model {mcomp}")
        g = answers if answers else ["-"] * len(strings)
        mm = manswers if manswers else ["-"] * len(strings)
        if comp.startswith("e") and mcomp.startswith("e"):
            continue
        for j, (ga, ma) in enumerate(zip(g, mm)):
            if ga != ma:
                dev.append(f"{line_no} {j} {ga} {ma}")
                whole = (ga == "!") != (ma == "!") or ga[:2] != ma[:2]
                s = strings[j].encode("latin-1")
                why = explained(cf, p, s)
                if whole and why is None:
                    surprises.append(f"match {cf} {p!r} {strings[j]!r}: glibc {ga}, model {ma}")
                note(f"whole: {why}" if whole else "submatch",
                     f"{cf} {p!r} {strings[j]!r}: glibc {ga}, model {ma}")
    for i, (cf, p, subj) in enumerate(icases):
        comp, answers = glibc_i[i]
        mcomp, manswers = model_i_all[i]
        g = answers if comp.startswith("n") else ["-"] * len(subj)
        mm = manswers if mcomp.startswith("n") else ["-"] * len(subj)
        for j, (s, ef, se, nm) in enumerate(subj):
            line_no = len(out) + 1
            out.append(f"I {cf} {escape(p)} {comp} {escape(s)} {ef} "
                       f"{'-' if se is None else f'{se[0]},{se[1]}'} {'-' if nm is None else nm}"
                       + (f" {g[j]}" if comp.startswith("n") else ""))
            if comp.startswith("crash"):
                note("glibc crashed", f"{cf} {p[:60]!r} {s[:30]!r}")
                dev.append(f"{line_no} c {comp} {mcomp}")
                dev.append(f"{line_no} 0 - {mm[j]}")
                continue
            if comp != mcomp:
                dev.append(f"{line_no} c {comp} {mcomp}")
                if not cf & ICASE:
                    surprises.append(f"compile {cf} {p[:80]!r}: glibc {comp}, model {mcomp}")
                else:
                    note("compile/icase", f"{cf} {p!r}")
            if g[j] != mm[j] and not (comp.startswith("e") and mcomp.startswith("e")):
                dev.append(f"{line_no} 0 {g[j]} {mm[j]}")
                gw, mw = g[j].split(",")[0], mm[j].split(",")[0]
                whole = gw != mw
                why = explained(cf, p, s)
                if whole and why is None:
                    surprises.append(f"match {cf} {p[:80]!r} {s[:40]!r} {ef} {se}: glibc {g[j]}, model {mm[j]}")
                note(f"whole: {why}" if whole else "submatch",
                     f"{cf} {p[:60]!r} {s[:30]!r} {ef}: glibc {g[j][:60]}, model {mm[j][:60]}")
    for e in errs:
        _e, code, size, got, hexbuf = e.split(" ")
        out.append(f"E {code} {size} {got} {hexbuf}")

    for k, v in sorted(kinds.items()):
        print(f"{k}: {len(v)}")
        for x in v[:12]:
            print("   ", x)
    if surprises:
        print(f"{len(surprises)} disagreement(s) the model does not expect -- nothing written:")
        for x in surprises[:60]:
            print("   ", x)
        if "--force" not in sys.argv:
            sys.exit(1)
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    DEVIATIONS.write_text("\n".join(dev) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(out)} lines; {DEVIATIONS.name}: {len(dev) - 4} deviations")


if __name__ == "__main__":
    main()
