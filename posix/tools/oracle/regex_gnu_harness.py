"""glibc 2.39's GNU regex interface -- re_set_syntax, re_compile_pattern,
re_compile_fastmap, re_search, re_search_2, re_match, re_match_2,
re_set_registers, and BSD's re_comp and re_exec -- in the C locale, as the
oracle for posix/src/regex.rs; and regex_model.py's answers, the
standard's, where glibc's submatches are not.

    python posix/tools/oracle/regex_gnu_harness.py
        # writes posix/src/regex_gnu_oracle.txt and
        # posix/src/regex_gnu_deviations.txt

**The cases.**

- G, the grammar: every token of GTOKENS and every two of them, in each of
  glibc's named syntaxes (RE_SYNTAX_GREP ... _EMACS); and in each syntax
  POSIX's two are with one bit of the twenty-six turned over, every token
  alone, after each of CONTEXT_BEFORE and before each of CONTEXT_AFTER --
  each compiled with re_compile_pattern and searched for in every string of
  GSTRINGS, its registers allocated by re_search;
- M, the searches: re_search over ranges forwards, backwards and empty, out
  of the string and overflowing; re_match; re_search_2 and re_match_2 at
  every split of a string and several stops, and from starts past the stop;
  registers allocated, grown
  (re_set_registers), fixed (REGS_FIXED) short and long, and reused; the
  pattern buffer's not_bol, not_eol, newline_anchor and no_sub; translate
  tables; and regexec of a pattern re_compile_pattern compiled;
- F, the fastmaps: re_compile_fastmap after re_compile_pattern, in several
  syntaxes and through translate tables, and regcomp's own -- with the
  bit-fields and the syntax regcomp leaves;
- C: a sequence of re_comp and re_exec calls, re_set_syntax between.

Every G, M and F case runs in a child process of its own: glibc crashes on
some patterns, and a crash costs that case's answer and no other's.

**regex_gnu_oracle.txt**, one line a case:

    G <syntax> <pattern> <compiled> <answer per string of GSTRINGS>
    M <syntax> <pattern> <trans> <bits> <call> <s1> <s2> <start> <range> <stop> <regs> <compiled> <answer>
    F <syntax|cflags> <pattern> <trans> <compiled> <fastmap> <fields> <syntax after>
    C <op> <arg> = <answer>

`syntax` is hex; `compiled` is `e<code>` for a refused pattern (the code
whose message re_compile_pattern returned), `n<re_nsub>`, or `crash<sig>`.
A G answer is `!` for no match, `x<r>` for another negative return, the
registers 0..re_nsub as two digits a pair (`__` for -1), or `@<start>` when
no registers came back (RE_NO_SUB). An M answer is, for each call, the
return value and, if registers were passed, `num:starts/ends` and
regs_allocated after; for `x` (regexec), the return and every pmatch entry.
A fastmap is 64 hex digits, four bytes a digit, byte 0 the low bit of the
first; fields are can_be_null, regs_allocated, fastmap_accurate, no_sub,
not_bol, not_eol, newline_anchor. Patterns and strings are escaped as in
regex_oracle.txt: `\\xNN` for a backslash, a space and anything outside
printable ASCII, `\\-` alone for the empty string.

**regex_gnu_deviations.txt**: every G and M answer where glibc's is not the
standard's, as regex_model.py computes it, which is what posix/src/regex.rs
answers there -- design-decisions.md sections 1160 and 1161 say why:

    <oracle line> <subject> <glibc's answer> <the model's>

`subject` is the string's index (G), 0 (M), or `c` for the compile. The
harness writes nothing if the model and glibc disagree anywhere it does not
expect: on whether a pattern compiles, or on whether and where a subject
matches, other than for one of the reasons `explained` lists.

    REGEX_HARNESS_CACHE=<file> python posix/tools/oracle/regex_gnu_harness.py

keeps glibc's output and the model's answers in `<file>` between runs.
"""

import multiprocessing
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402
import regex_model as M  # noqa: E402
from regex_harness import c_bytes, escape  # noqa: E402

OUT = POSIX_SRC / "regex_gnu_oracle.txt"
DEVIATIONS = POSIX_SRC / "regex_gnu_deviations.txt"

# glibc's messages, by code (regerror's, which re_compile_pattern returns).
MESSAGES = [
    "Success", "No match", "Invalid regular expression", "Invalid collation character",
    "Invalid character class name", "Trailing backslash", "Invalid back reference",
    "Unmatched [, [^, [:, [., or [=", "Unmatched ( or \\(", "Unmatched \\{",
    "Invalid content of \\{\\}", "Invalid range end", "Memory exhausted",
    "Invalid preceding regular expression", "Premature end of regular expression",
    "Regular expression too big", "Unmatched ) or \\)",
]

B = M.RE_SYNTAX_POSIX_BASIC
E = M.RE_SYNTAX_POSIX_EXTENDED
NAMED = {
    "emacs": 0,
    "awk": (M.RE_BACKSLASH_ESCAPE_IN_LISTS | M.RE_DOT_NOT_NULL | M.RE_NO_BK_PARENS
            | M.RE_NO_BK_REFS | M.RE_NO_BK_VBAR | M.RE_NO_EMPTY_RANGES | M.RE_DOT_NEWLINE
            | M.RE_CONTEXT_INDEP_ANCHORS | M.RE_CHAR_CLASSES | M.RE_UNMATCHED_RIGHT_PAREN_ORD
            | M.RE_NO_GNU_OPS),
    "gnu_awk": ((E | M.RE_BACKSLASH_ESCAPE_IN_LISTS | M.RE_INVALID_INTERVAL_ORD)
                & ~(M.RE_DOT_NOT_NULL | M.RE_CONTEXT_INDEP_OPS | M.RE_CONTEXT_INVALID_OPS)),
    "posix_awk": (E | M.RE_BACKSLASH_ESCAPE_IN_LISTS | M.RE_INTERVALS | M.RE_NO_GNU_OPS
                  | M.RE_INVALID_INTERVAL_ORD),
    "grep": (B | M.RE_NEWLINE_ALT) & ~(M.RE_CONTEXT_INVALID_DUP | M.RE_DOT_NOT_NULL),
    "egrep": ((E | M.RE_INVALID_INTERVAL_ORD | M.RE_NEWLINE_ALT)
              & ~(M.RE_CONTEXT_INVALID_OPS | M.RE_DOT_NOT_NULL)),
    "posix_basic": B,
    "posix_minimal_basic": (M.RE_CHAR_CLASSES | M.RE_DOT_NEWLINE | M.RE_DOT_NOT_NULL
                            | M.RE_INTERVALS | M.RE_NO_EMPTY_RANGES | M.RE_LIMITED_OPS),
    "posix_extended": E,
    "posix_minimal_extended": (M.RE_CHAR_CLASSES | M.RE_DOT_NEWLINE | M.RE_DOT_NOT_NULL
                               | M.RE_INTERVALS | M.RE_NO_EMPTY_RANGES
                               | M.RE_CONTEXT_INDEP_ANCHORS | M.RE_CONTEXT_INVALID_OPS
                               | M.RE_NO_BK_BRACES | M.RE_NO_BK_PARENS | M.RE_NO_BK_REFS
                               | M.RE_NO_BK_VBAR | M.RE_UNMATCHED_RIGHT_PAREN_ORD),
}

# -- G -----------------------------------------------------------------------

GTOKENS = [
    "a", "b", ".", "*", "+", "?", "\\+", "\\?", "|", "\\|", "(", ")", "\\(", "\\)", "{1}",
    "\\{1\\}", "{1", "\\{1", "{", "}", "^", "$", "[ab]", "[^a]", "[b-a]", "[[:alpha:]]",
    "[\\]a]", "\\1", "\\w", "\\<", "\\`", "\\'", "\n", "\\n", "a*", "\\a",
]
CONTEXT_BEFORE = ["a", "(", "\\(", "|", "\\|", "\n", "^"]
CONTEXT_AFTER = ["a", ")", "\\)", "|", "\\|", "\n", "$"]
GSTRINGS = ["", "a", "b", "A", "ab", "ba", "aa", "\n", "a\nb", "\0", "(", ")", "|", "+", "?",
            "*", "{1}", "1", "\\", "]a"]


def flipped():
    """(name, syntax) for POSIX's two syntaxes with one bit turned over."""
    out = []
    for base_name, base in (("b", B), ("e", E)):
        for k in range(26):
            out.append((f"{base_name}^{k}", base ^ (1 << k)))
    return out


def g_cases():
    """(syntax, pattern bytes)."""
    out = []
    pairs = GTOKENS + [a + b for a in GTOKENS for b in GTOKENS]
    for _name, syn in NAMED.items():
        for p in pairs:
            out.append((syn, p.encode("latin-1")))
    ctx = GTOKENS + [c + t for c in CONTEXT_BEFORE for t in GTOKENS] + \
        [t + c for t in GTOKENS for c in CONTEXT_AFTER]
    for _name, syn in flipped():
        for p in ctx:
            out.append((syn, p.encode("latin-1")))
    return out


# -- M -----------------------------------------------------------------------

TRANS = {"-": None,
         "fold": bytes(c + 32 if 0x41 <= c <= 0x5A else c for c in range(256)),
         "swap": bytes(0x62 if c == 0x61 else 0x61 if c == 0x62 else c for c in range(256)),
         "upper": bytes(c - 32 if 0x61 <= c <= 0x7A else c for c in range(256))}
TRANS_ID = {k: i for i, k in enumerate(TRANS)}


def m_cases():
    """(syntax, pattern, trans, bits, call, s1, s2, start, range, stop, regs, n)."""
    out = []

    def add(syn, p, call, s1, s2=b"", start=0, rng=None, stop=None, regs="u", n=0, trans="-",
            bits=0):
        s1 = s1.encode("latin-1") if isinstance(s1, str) else s1
        s2 = s2.encode("latin-1") if isinstance(s2, str) else s2
        if rng is None:
            rng = len(s1) + len(s2)
        if stop is None:
            stop = len(s1) + len(s2)
        out.append((syn, p.encode("latin-1") if isinstance(p, str) else p, trans, bits, call,
                    s1, s2, start, rng, stop, regs, n))

    # Ranges: forwards, backwards, empty, from every place, out of the string.
    subj = "xaxab aab"
    for p, syn in (("a", B), ("a*", B), ("ab", B), ("^a", B), ("a$", E), ("\\(a\\)b", B),
                   ("(a|x)(b)?", E), ("\\<a", B), ("b\\b", B), ("", B)):
        for start in (0, 1, 2, 3, 5, 9, 10, -1):
            for rng in (0, 1, 3, -1, -3, 9, -9, 100, -100, 2**62, -(2**62)):
                add(syn, p, "s", subj, start=start, rng=rng)
    # re_match: the length matched at each place.
    for p, syn in (("a*", B), ("x\\|xa", B), ("(a|ab)(c|bcd)?", E), ("\\(a\\)\\1", B), ("$", B),
                   ("^", B), ("\\`", B), ("\\'", B), ("[^a]*", B)):
        for start in range(0, 7):
            add(syn, p, "m", "aabxab", start=start)
    # The two strings taken as one: every split, and the stops.
    whole = "abcabc"
    for p, syn in (("ca", B), ("bc$", B), ("^ab", B), ("\\(b\\)c\\1", B), ("c\\'", B),
                   ("\\bc", B), ("(abc)+", E), ("x*", B)):
        for k in range(len(whole) + 1):
            for stop in (len(whole), len(whole) - 1, 3, 0, 99):
                add(syn, p, "S", whole[:k], whole[k:], 0, len(whole), stop)
                add(syn, p, "M", whole[:k], whole[k:], 0, None, stop)
    for start, rng in ((2, 3), (5, -5), (6, 0), (7, 1), (0, -1)):
        add(B, "c", "S", "abc", "abc", start, rng, 6)
    add(B, "a", "S", "a", "a", 0, 2, -1)
    # Starts past the stop, where glibc's search goes on to find an empty
    # match, though its header says the search stops there.
    for p in ("x*", "$", "\\'", "b*", "c"):
        for start, rng in ((0, 6), (3, 3), (6, -6), (4, -1), (1, 1)):
            add(B, p, "S", "abc", "abc", start, rng, 2)
        for start in (1, 2, 3, 6):
            add(B, p, "M", "abc", "abc", start, None, 2)
    # Registers: none, allocated, grown, fixed short and long, reused.
    for p, syn in (("\\(a\\)\\(b\\)\\(c\\)?", B), ("(a)|(b)", E), ("a", B), ("()", E),
                   ("((a)(b))", E)):
        for regs, n in (("-", 0), ("u", 0), ("U", 0), ("r", 1), ("r", 2), ("r", 9),
                        ("f", 0), ("f", 1), ("f", 2), ("f", 3), ("f", 4), ("f", 9)):
            for s in ("abc", "ab", "b", "x"):
                add(syn, p, "s", s, regs=regs, n=n)
    # The pattern buffer's bits.
    for p, syn in (("^a", B), ("a$", B), ("^", B), ("$", B), ("\\`a", B), ("b\\'", B),
                   ("^b", E), ("a$", E), ("\\(a\\)", B)):
        for bits in range(16):
            for s in ("a\nb", "b\na", "ab", "\na"):
                add(syn, p, "s", s, bits=bits)
    # Translate tables, in the pattern and the subject.
    for trans in ("fold", "swap", "upper"):
        for p, syn in (("a", B), ("A", B), ("[a-c]", B), ("[^a]", B), ("[[:upper:]]", B),
                       ("\\w", B), ("\\W", B), ("\\a", B), ("\\A", B), ("\\(a\\)\\1", B),
                       ("[[.a.]]", B), ("[[=A=]]", B), ("\\bb", B), (".", B), ("b*", B),
                       ("a|b", E), ("\\*", B), ("[\\a]", NAMED["awk"])):
            for s in ("a", "A", "b", "B", "ab", "ba", "aA", "Ab", "AB"):
                add(syn, p, "s", s, trans=trans)
    # regexec of a pattern re_compile_pattern compiled: newline_anchor set.
    for p, syn in (("^a", B), ("a$", B), ("\\(a\\)\\(b\\)", B), ("(a)|b", E)):
        for s in ("a", "b\na", "a\nb", "ab", "ba"):
            for eflags in (0, 1, 2):
                add(syn, p, "x", s, start=eflags)
    # RE_NO_SUB, RE_ICASE, RE_NO_POSIX_BACKTRACKING.
    for syn in (B | M.RE_NO_SUB, E | M.RE_NO_SUB, B | M.RE_ICASE, E | M.RE_NO_POSIX_BACKTRACKING):
        for p in (["\\(a\\)b", "a\\|ab"] if not syn & M.RE_NO_BK_PARENS else ["(a)b", "a|ab"]):
            for s in ("ab", "Ab", "xab"):
                for regs in ("-", "u"):
                    add(syn, p, "s", s, regs=regs)
    return out


# -- F -----------------------------------------------------------------------

FPATTERNS = [
    "a", "ab", "a*b", "a*", "", ".", "[ab]", "[^a]", "[^ab\n]", "a|b", "(a)|b*", "^a", "a$",
    "\\`a", "\\<a", "\\>a", "\\ba", "\\Ba", "^\\>a", "\\<\\>a", "(a*)\\1b", "()\\1a",
    "(a)\\1", "\\w", "\\W", "\\s", "\\S", "[[:upper:]]", "[^[:lower:]]", "[a-c]", "[^a-c]",
    "(^|a)b", "x?y", "(|a)b", "\\'", "$", "^", "a{0}b", "(a){2}", "[[.a.]]", "[[=a=]]",
    "\n", "a\nb",
]


def f_cases():
    """(syntax, cflags or -1, pattern, trans)."""
    out = []
    for p in FPATTERNS:
        pb = p.encode("latin-1")
        for syn in (E, E | M.RE_ICASE, NAMED["egrep"], NAMED["emacs"] | M.RE_NO_BK_PARENS
                    | M.RE_NO_BK_VBAR):
            out.append((syn, -1, pb, "-"))
        for trans in ("fold", "swap", "upper"):
            out.append((E, -1, pb, trans))
        if b"\n" not in pb:
            for cf in (1, 3, 5, 9, 0):
                out.append((0, cf, pb, "-"))
    return out


# -- C -----------------------------------------------------------------------

# No re_exec before a pattern compiles, nor after one fails to: glibc's
# matches through the NULL buffer then, and crashes (this library's answers
# 0).
C_STEPS = [
    ("c", None), ("s", B), ("c", "a\\(b\\)"), ("e", "xab"), ("e", "xa"),
    ("c", None), ("e", "ab"), ("c", "[ab"), ("c", "\\(a"), ("c", "b*"), ("e", ""), ("e", "c"),
    ("s", E), ("c", "(a|b)+c"), ("e", "abac"), ("e", "abab"), ("s", 0), ("c", "a\\|b"),
    ("e", "b"), ("c", "^b"), ("e", "a\nb"), ("c", None), ("e", "zz"),
]


# -- the C side --------------------------------------------------------------

C_MAIN = r'''
static unsigned char TABLES[4][256];

static void init_tables(void)
{
    for (int i = 0; i < 256; i++) {
        TABLES[0][i] = (unsigned char) i;
        TABLES[1][i] = (unsigned char) (i >= 'A' && i <= 'Z' ? i + 32 : i);
        TABLES[2][i] = (unsigned char) (i == 'a' ? 'b' : i == 'b' ? 'a' : i);
        TABLES[3][i] = (unsigned char) (i >= 'a' && i <= 'z' ? i - 32 : i);
    }
}

/* A malloc'ed copy of table k: regfree frees a pattern buffer's. */
static unsigned char *table(int k)
{
    if (!k)
        return NULL;
    unsigned char *t = malloc(256);
    memcpy(t, TABLES[k], 256);
    return t;
}

static void text(const char *s)
{
    for (; *s; s++) {
        unsigned char c = (unsigned char) *s;
        if (c > 0x20 && c < 0x7f && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}

static const char *compile(regex_t *re, unsigned long syntax, const char *p, size_t len, int t)
{
    memset(re, 0, sizeof *re);
    re->translate = table(t);
    re_set_syntax(syntax);
    return re_compile_pattern(p, len, re);
}

static void pair(long so, long eo)
{
    if (so < 0 && eo < 0)
        fputs("__", stdout);
    else if (so > 9 || eo > 9 || so < 0 || eo < 0)
        printf("?");
    else
        printf("%ld%ld", so, eo);
}

static void g_case(unsigned i)
{
    regex_t re;
    const char *err = compile(&re, GSYN[i], GP[i], GPL[i], 0);
    if (err) {
        printf("G%u e", i);
        text(err);
        putchar('\n');
        return;
    }
    printf("G%u n%zu", i, re.re_nsub);
    for (unsigned j = 0; j < sizeof GS / sizeof *GS; j++) {
        struct re_registers regs = {0};
        re.regs_allocated = REGS_UNALLOCATED;
        regoff_t r = re_search(&re, GS[j], GSL[j], 0, GSL[j], &regs);
        if (r == -1)
            fputs(" !", stdout);
        else if (r < 0)
            printf(" x%d", (int) r);
        else if (regs.num_regs == 0)
            printf(" @%d", (int) r);
        else {
            putchar(' ');
            for (size_t k = 0; k <= re.re_nsub; k++)
                pair(regs.start[k], regs.end[k]);
        }
        free(regs.start);
        free(regs.end);
    }
    putchar('\n');
    regfree(&re);
}

static void m_case(unsigned i)
{
    regex_t re;
    const char *err = compile(&re, MSYN[i], MP[i], MPL[i], MTRANS[i]);
    if (err) {
        printf("M%u e", i);
        text(err);
        putchar('\n');
        return;
    }
    if (MBITS[i] & 1)
        re.not_bol = 1;
    if (MBITS[i] & 2)
        re.not_eol = 1;
    if (MBITS[i] & 4)
        re.newline_anchor = 0;
    if (MBITS[i] & 8)
        re.no_sub = 1;
    struct re_registers regs = {0};
    regoff_t fixed_s[64], fixed_e[64];
    struct re_registers *rp = NULL;
    char mode = MREGS[i];
    if (mode == 'u' || mode == 'U')
        rp = &regs;
    else if (mode == 'r') {
        regoff_t *s = malloc(MN[i] * sizeof *s), *e = malloc(MN[i] * sizeof *e);
        for (int k = 0; k < MN[i]; k++)
            s[k] = e[k] = -9;
        re_set_registers(&re, &regs, MN[i], s, e);
        rp = &regs;
    } else if (mode == 'f') {
        for (int k = 0; k < 64; k++)
            fixed_s[k] = fixed_e[k] = -9;
        regs.num_regs = MN[i];
        regs.start = fixed_s;
        regs.end = fixed_e;
        re.regs_allocated = REGS_FIXED;
        rp = &regs;
    }
    printf("M%u n%zu", i, re.re_nsub);
    int times = mode == 'U' ? 2 : 1;
    for (int t = 0; t < times; t++) {
        regoff_t r = 0;
        switch (MCALL[i]) {
        case 's':
            r = re_search(&re, MS1[i], MS1L[i], MSTART[i], MRANGE[i], rp);
            break;
        case 'm':
            r = re_match(&re, MS1[i], MS1L[i], MSTART[i], rp);
            break;
        case 'S':
            r = re_search_2(&re, MS1[i], MS1L[i], MS2[i], MS2L[i], MSTART[i], MRANGE[i], rp,
                            MSTOP[i]);
            break;
        case 'M':
            r = re_match_2(&re, MS1[i], MS1L[i], MS2[i], MS2L[i], MSTART[i], rp, MSTOP[i]);
            break;
        case 'x': {
            regmatch_t pm[16];
            char *s = strndup(MS1[i], MS1L[i]);
            size_t nm = re.re_nsub + 1;
            if (nm > 16)
                nm = 16;
            for (int k = 0; k < 16; k++)
                pm[k].rm_so = pm[k].rm_eo = -9;
            int rc = regexec(&re, s, nm, pm, (int) MSTART[i]);
            printf(" %d", rc);
            for (size_t k = 0; k < nm; k++)
                printf("%c%ld:%ld", k ? ',' : '=', (long) pm[k].rm_so, (long) pm[k].rm_eo);
            free(s);
            continue;
        }
        }
        printf(" %ld", (long) r);
        if (rp) {
            printf(" %u:", (unsigned) regs.num_regs);
            for (unsigned k = 0; k < regs.num_regs && k < 64; k++)
                printf("%s%ld", k ? "," : "", (long) regs.start[k]);
            putchar('/');
            for (unsigned k = 0; k < regs.num_regs && k < 64; k++)
                printf("%s%ld", k ? "," : "", (long) regs.end[k]);
            printf(" %u", re.regs_allocated);
        }
    }
    putchar('\n');
    if (mode != 'f') {
        free(regs.start);
        free(regs.end);
    }
    regfree(&re);
}

static void f_case(unsigned i)
{
    regex_t re;
    if (FCF[i] >= 0) {
        int rc = regcomp(&re, FP[i], FCF[i]);
        if (rc) {
            printf("F%u e%d\n", i, rc);
            return;
        }
    } else {
        const char *err = compile(&re, FSYN[i], FP[i], FPL[i], FTRANS[i]);
        if (err) {
            printf("F%u e", i);
            text(err);
            putchar('\n');
            return;
        }
        re.fastmap = malloc(256);
        int r = re_compile_fastmap(&re);
        if (r) {
            printf("F%u x%d\n", i, r);
            regfree(&re);
            return;
        }
    }
    printf("F%u n%zu ", i, re.re_nsub);
    for (int b = 0; b < 256; b += 4)
        printf("%x", (re.fastmap[b] ? 1 : 0) | (re.fastmap[b + 1] ? 2 : 0)
                         | (re.fastmap[b + 2] ? 4 : 0) | (re.fastmap[b + 3] ? 8 : 0));
    printf(" %u%u%u%u%u%u%u %lx\n", re.can_be_null, re.regs_allocated, re.fastmap_accurate,
           re.no_sub, re.not_bol, re.not_eol, re.newline_anchor, (unsigned long) re.syntax);
    regfree(&re);
}

static void c_steps(void)
{
    for (unsigned i = 0; i < sizeof COP / sizeof *COP; i++) {
        if (COP[i] == 's') {
            re_set_syntax(CSYN[i]);
            printf("C s %lx = ok\n", CSYN[i]);
        } else if (COP[i] == 'c') {
            char *r = re_comp(CARG[i]);
            printf("C c ");
            if (!CARG[i])
                fputs("-", stdout);
            else if (*CARG[i])
                text(CARG[i]);
            else
                fputs("\\-", stdout);
            fputs(" = ", stdout);
            if (r)
                text(r);
            else
                fputs("-", stdout);
            putchar('\n');
        } else {
            int r = re_exec(CARG[i]);
            printf("C e ");
            if (*CARG[i])
                text(CARG[i]);
            else
                fputs("\\-", stdout);
            printf(" = %d\n", r);
        }
        fflush(stdout);
    }
}

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
    init_tables();
    for (unsigned i = 0; i < sizeof GSYN / sizeof *GSYN; i++)
        isolated(g_case, i, 'G');
    for (unsigned i = 0; i < sizeof MSYN / sizeof *MSYN; i++)
        isolated(m_case, i, 'M');
    for (unsigned i = 0; i < sizeof FSYN / sizeof *FSYN; i++)
        isolated(f_case, i, 'F');
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        c_steps();
        _exit(0);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    if (!WIFEXITED(st))
        printf("C crash\n");
    return 0;
}
'''


def build_c(gc, mc, fc) -> str:
    lines = ["#define _GNU_SOURCE", "#define _REGEX_RE_COMP", "#include <regex.h>",
             "#include <stdio.h>", "#include <stdlib.h>", "#include <string.h>",
             "#include <sys/wait.h>", "#include <unistd.h>", ""]

    def arr(name, ctype, vals):
        lines.append(f"static const {ctype} {name}[] = {{" + ", ".join(vals) + "};")

    arr("GS", "char *const", [c_bytes(s.encode("latin-1")) for s in GSTRINGS])
    arr("GSL", "regoff_t", [str(len(s)) for s in GSTRINGS])
    arr("GSYN", "unsigned long", [f"0x{s:x}UL" for s, _p in gc])
    arr("GP", "char *const", [c_bytes(p) for _s, p in gc])
    arr("GPL", "size_t", [str(len(p)) for _s, p in gc])
    arr("MSYN", "unsigned long", [f"0x{c[0]:x}UL" for c in mc])
    arr("MP", "char *const", [c_bytes(c[1]) for c in mc])
    arr("MPL", "size_t", [str(len(c[1])) for c in mc])
    arr("MTRANS", "int", [str(TRANS_ID[c[2]]) for c in mc])
    arr("MBITS", "int", [str(c[3]) for c in mc])
    arr("MCALL", "char", [f"'{c[4]}'" for c in mc])
    arr("MS1", "char *const", [c_bytes(c[5]) for c in mc])
    arr("MS1L", "regoff_t", [str(len(c[5])) for c in mc])
    arr("MS2", "char *const", [c_bytes(c[6]) for c in mc])
    arr("MS2L", "regoff_t", [str(len(c[6])) for c in mc])
    # regoff_t is an int in glibc: the large starts and ranges are cut to
    # it, as a C caller's would be.
    arr("MSTART", "long", [str(c[7]) for c in mc])
    arr("MRANGE", "long", [str(c[8]) for c in mc])
    arr("MSTOP", "regoff_t", [str(c[9]) for c in mc])
    arr("MREGS", "char", [f"'{c[10]}'" for c in mc])
    arr("MN", "int", [str(c[11]) for c in mc])
    arr("FSYN", "unsigned long", [f"0x{c[0]:x}UL" for c in fc])
    arr("FCF", "int", [str(c[1]) for c in fc])
    arr("FP", "char *const", [c_bytes(c[2]) for c in fc])
    arr("FPL", "size_t", [str(len(c[2])) for c in fc])
    arr("FTRANS", "int", [str(TRANS_ID[c[3]]) for c in fc])
    arr("COP", "char", [f"'{op}'" for op, _a in C_STEPS])
    arr("CSYN", "unsigned long", [f"0x{a:x}UL" if op == "s" else "0" for op, a in C_STEPS])
    arr("CARG", "char *const", [c_bytes(a.encode("latin-1")) if isinstance(a, str) else "0"
                                for op, a in C_STEPS])
    lines.append(C_MAIN)
    return "\n".join(lines)


# -- the model's answers -----------------------------------------------------


def pairs_of(pm) -> str:
    return "".join("__" if so < 0 and eo < 0 else (f"{so}{eo}" if 0 <= so <= 9 and 0 <= eo <= 9
                                                    else "?") for so, eo in pm)


def compiled(syn: int, p: bytes, trans=None):
    """The model's (compiled answer, Regex or None)."""
    try:
        rx = M.compile_syntax(p, syn, trans)
    except M.Error as e:
        return f"e{e.code}", None
    return f"n{rx.re_nsub}", rx


def model_g(args):
    syn, p = args
    comp, rx = compiled(syn, p)
    if rx is None:
        return comp, []
    answers = []
    for s in GSTRINGS:
        pm = rx.exec(s.encode("latin-1"), newline=True)
        if pm is None:
            answers.append("!")
        elif syn & M.RE_NO_SUB:
            answers.append(f"@{pm[0][0]}")
        else:
            answers.append(pairs_of(pm))
    return comp, answers


def i32(v: int) -> int:
    """`v` as glibc's regoff_t, an int, holds it."""
    v &= 0xFFFFFFFF
    return v - (1 << 32) if v >= 1 << 31 else v


def stub_range(length: int, start: int, rng: int):
    """glibc's re_search_stub's checks: (None, start) for -1 at once, else
    (last_start, start)."""
    start, rng = i32(start), i32(rng)
    last = i32(start + rng)
    if start < 0 or start > length:
        return None, start
    if length < last or (rng >= 0 and last < start):
        last = length
    elif last < 0 or (rng < 0 and start <= last):
        last = 0
    return last, start


def copy_regs(state, pm, nregs):
    """glibc's re_copy_regs over `state` (num_regs, starts, ends, how): the
    new state."""
    num, starts, ends, how = state
    need = nregs + 1
    if how == 0:
        num, starts, ends = need, [0] * need, [0] * need
        rval = 1
    elif how == 1:
        if need > num:
            starts = list(starts) + [0] * (need - num)
            ends = list(ends) + [0] * (need - num)
            num = need
        rval = 1
    else:
        rval = 2
    starts, ends = list(starts), list(ends)
    for i in range(num):
        so, eo = pm[i] if i < nregs else (-1, -1)
        starts[i], ends[i] = so, eo
    return (num, starts, ends, rval)


def initial_regs(regs: str, n: int):
    if regs == "r":
        return (n, [-9] * n, [-9] * n, 1)
    if regs == "f":
        return (n, [-9] * 64, [-9] * 64, 2)
    return (0, [], [], 0)


def regs_text(state) -> str:
    num, starts, ends, how = state
    k = min(num, 64)
    return (f" {num}:" + ",".join(str(starts[i]) for i in range(k)) + "/"
            + ",".join(str(ends[i]) for i in range(k)) + f" {how}")


def model_m(case):
    syn, p, trans, bits, call, s1, s2, start, rng, stop, regs, n = case
    table = TRANS[trans]
    comp, rx = compiled(syn, p, table)
    if rx is None:
        return comp, ""
    nsub = rx.re_nsub
    no_sub = bool(syn & M.RE_NO_SUB) or bool(bits & 8)
    newline = not bits & 4
    eflags = (M.REG_NOTBOL if bits & 1 else 0) | (M.REG_NOTEOL if bits & 2 else 0)
    if call == "x":
        s = s1.split(b"\0")[0]
        if table is not None:
            s = bytes(table[c] for c in s)
        nm = min(nsub + 1, 16)
        pm = rx.exec(s, start, newline=newline)
        if pm is None or no_sub:
            entries = [(-9, -9)] * nm
        else:
            entries = [pm[k] for k in range(nm)]
        return comp, f" {1 if pm is None else 0}" + "".join(
            ("," if k else "=") + f"{so}:{eo}" for k, (so, eo) in enumerate(entries))
    state = initial_regs(regs, n)
    if call in "SM" and stop < 0:
        return comp, " -2" + (regs_text(state) if regs != "-" else "")
    s = s1 + s2 if call in "SM" else s1
    length = len(s)
    if table is not None:
        s = bytes(table[c] for c in s)
    last, st = stub_range(length, start, 0 if call in "mM" else rng)
    out = ""
    for _t in range(2 if regs == "U" else 1):
        if last is None:
            pm = None
        else:
            pm = rx.exec(s, eflags, st, newline=newline, last=last,
                         stop=length if call in "sm" else stop)
        if pm is None:
            ret = -1
        else:
            ret = pm[0][1] - st if call in "mM" else pm[0][0]
        if pm is not None and regs != "-" and not no_sub:
            num, _s, _e, how = state
            nregs = nsub + 1
            if how == 2 and num <= nsub:
                nregs = num
            if nregs >= 1:
                state = copy_regs(state, pm, nregs)
        out += f" {ret}"
        if regs != "-":
            out += regs_text(state)
    return comp, out


def explained(syn: int, p: bytes, s: bytes, trans="-", past_stop=False):
    """Why glibc and the model may disagree on whether, or where, `p` matches
    `s` at all -- or None, if they should not: regex_harness.py's reasons,
    in the GNU syntaxes; the translate table's; and re_search_2's stop,
    past which glibc's search finds an empty match, where its header says the
    search stops there."""
    if past_stop:
        return "an empty match past stop"
    if re.search(rb"\\[1-9]", p) and not syn & M.RE_NO_BK_REFS:
        return "back-reference"
    if syn & M.RE_ICASE:
        return "RE_ICASE"
    if trans != "-" and re.search(rb"\\[^1-9]", p):
        return "an escaped byte not translated"
    if b"*\\B" in p:
        return "a star before \\B"
    if b"\\B" in p and re.search(rb"[*+?}]", p):
        return "\\B with a repetition"
    if (b"(^" in p or b"\\(^" in p) and re.search(rb"[*+?}]", p):
        return "^ inside a repeated group"
    return None


def code_of(text: str) -> str:
    """`e<code>` for one of glibc's messages, escaped as the C side writes it."""
    raw = re.sub(r"\\x([0-9a-f]{2})", lambda m: chr(int(m.group(1), 16)), text)
    if raw in MESSAGES:
        return f"e{MESSAGES.index(raw)}"
    return "e?" + text


def main() -> None:
    import os
    import pickle

    gc, mc, fc = g_cases(), m_cases(), f_cases()
    cache = os.environ.get("REGEX_HARNESS_CACHE")
    cached = pickle.loads(Path(cache).read_bytes()) if cache and Path(cache).exists() else None
    if cached is None:
        with workdir() as t:
            d = Path(t)
            (d / "gnu.c").write_text(build_c(gc, mc, fc), encoding="utf-8", newline="\n")
            r = run(f"cd {wsl_path(d)} && gcc -O1 -w -o gnu gnu.c && LC_ALL=C ./gnu")
            if r.returncode != 0:
                sys.exit(f"the harness failed:\n{r.stderr[-3000:]}\n{r.stdout[-2000:]}")
        stdout = r.stdout
    else:
        stdout = cached["stdout"]
    glibc = {"G": {}, "M": {}, "F": {}}
    csteps = []
    for line in stdout.split("\n"):
        if not line:
            continue
        if line.startswith("C "):
            csteps.append(line)
            continue
        head, _, rest = line.partition(" ")
        glibc[head[0]][int(head[1:])] = rest
    print(f"glibc: {len(gc)} grammar cases, {len(mc)} searches, {len(fc)} fastmaps", flush=True)
    if cached is not None and "model_g" in cached and "--remodel" not in sys.argv:
        mg, mm = cached["model_g"], cached["model_m"]
    else:
        with multiprocessing.Pool(4) as pool:
            mg = pool.map(model_g, gc, chunksize=64)
            print("model: grammar done", flush=True)
            mm = pool.map(model_m, mc, chunksize=16)
            print("model: searches done", flush=True)
    if cache:
        Path(cache).write_bytes(pickle.dumps({"stdout": stdout, "model_g": mg, "model_m": mm}))

    out = [
        "# glibc 2.39's GNU regex interface in the C locale, for posix/src/regex.rs.",
        "# Generated by posix/tools/oracle/regex_gnu_harness.py; do not edit.",
        "# strings: " + " ".join(escape(s.encode("latin-1")) for s in GSTRINGS),
    ]
    dev = [
        "# Every case of regex_gnu_oracle.txt where glibc 2.39's answer is not the standard's,",
        "# as posix/tools/oracle/regex_model.py computes it and posix/src/regex.rs answers:",
        "# <oracle line> <subject> <glibc's answer> <the model's>. Generated by",
        "# posix/tools/oracle/regex_gnu_harness.py; do not edit.",
    ]
    surprises, kinds = [], {}

    def note(kind, detail):
        kinds.setdefault(kind, []).append(detail)

    def glibc_comp(rest):
        word, _, tail = rest.partition(" ")
        if word.startswith("e"):
            return code_of(word[1:]), ""
        return word, tail

    for i, (syn, p) in enumerate(gc):
        comp, tail = glibc_comp(glibc["G"][i])
        answers = tail.split(" ") if tail else []
        mcomp, manswers = mg[i]
        line_no = len(out) + 1
        out.append(f"G {syn:x} {escape(p)} {comp}" + "".join(" " + a for a in answers))
        if comp.startswith("crash"):
            note("glibc crashed", f"{syn:x} {p!r}")
            dev.append(f"{line_no} c {comp} {mcomp}")
            for j, a in enumerate(manswers):
                dev.append(f"{line_no} {j} - {a}")
            continue
        if comp != mcomp:
            dev.append(f"{line_no} c {comp} {mcomp}")
            if syn & M.RE_ICASE:
                note("compile/icase", f"{syn:x} {p!r}: glibc {comp}, model {mcomp}")
            else:
                surprises.append(f"compile {syn:x} {p!r}: glibc {comp}, model {mcomp}")
        if comp.startswith("e") and mcomp.startswith("e"):
            continue
        g = answers if answers else ["-"] * len(GSTRINGS)
        m = manswers if manswers else ["-"] * len(GSTRINGS)
        for j, (ga, ma) in enumerate(zip(g, m)):
            if ga != ma:
                dev.append(f"{line_no} {j} {ga} {ma}")
                whole = (ga == "!") != (ma == "!") or ga[:2] != ma[:2]
                why = explained(syn, p, GSTRINGS[j].encode("latin-1"))
                if whole and why is None:
                    surprises.append(f"match {syn:x} {p!r} {GSTRINGS[j]!r}: glibc {ga}, model {ma}")
                note(f"whole: {why}" if whole else "submatch",
                     f"{syn:x} {p!r} {GSTRINGS[j]!r}: glibc {ga}, model {ma}")
    for i, case in enumerate(mc):
        syn, p, trans, bits, call, s1, s2, start, rng, stop, regs, n = case
        comp, tail = glibc_comp(glibc["M"][i])
        mcomp, manswer = mm[i]
        line_no = len(out) + 1
        out.append(f"M {syn:x} {escape(p)} {trans} {bits} {call} {escape(s1)} {escape(s2)} "
                   f"{start} {rng} {stop} {regs}{n} {comp}" + (f" {tail}" if tail else ""))
        if comp != mcomp:
            dev.append(f"{line_no} c {comp} {mcomp}")
            surprises.append(f"compile M{i} {syn:x} {p!r}: glibc {comp}, model {mcomp}")
            continue
        if comp.startswith("e") or comp.startswith("crash"):
            continue
        ga, ma = " " + tail, manswer
        if ga != ma:
            dev.append(f"{line_no} 0 {ga.strip()} {ma.strip()}")
            gw, mw = ga.split()[0], ma.split()[0]
            past = ((call == "S" and gw.lstrip("-").isdigit() and int(gw) > stop)
                    or (call == "M" and start > stop))
            why = explained(syn, p, s1 + s2, trans, past)
            if gw != mw and why is None:
                surprises.append(f"M{i} {syn:x} {p!r} {call} {s1!r} {s2!r} {start} {rng} {stop} "
                                 f"{regs}{n} bits {bits} {trans}: glibc{ga}, model{ma}")
            note(f"whole: {why}" if gw != mw else "submatch",
                 f"M{i} {p!r} {call} {s1!r}: glibc{ga[:70]}, model{ma[:70]}")
    for i, (syn, cf, p, trans) in enumerate(fc):
        rest = glibc["F"][i]
        word, _, tail = rest.partition(" ")
        comp = code_of(word[1:]) if word.startswith("e") and cf < 0 else word
        out.append(f"F {syn:x} {cf} {escape(p)} {trans} {comp}" + (f" {tail}" if tail else ""))
    out.extend(csteps)

    for k, v in sorted(kinds.items()):
        print(f"{k}: {len(v)}")
        for x in v[:10]:
            print("   ", x)
    if os.environ.get("REGEX_SURPRISES"):
        # Every one of them, for working through, in the file it names.
        Path(os.environ["REGEX_SURPRISES"]).write_text("\n".join(surprises) + "\n",
                                                       encoding="utf-8", newline="\n")
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
