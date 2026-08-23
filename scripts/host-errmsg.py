#!/usr/bin/env python3
"""Find utilities that print the *host's* error text instead of POSIX's.

`std::io::Error`'s `Display` is whatever the platform said. On Linux and on
SlateOS that is `strerror(3)` -- "No such file or directory". On the Windows
development host the very same error prints

    The system cannot find the file specified. (os error 2)

so a utility written as

    eprintln!("cp: cannot stat {}: {e}", quoteaf_os(src));

produces one message in production and a different one in every test that runs
on the machine we develop on. `userspace/coreutils/src/errmsg.rs` exists for
exactly this and says why: the message is an interface. A shell script doing
`2>&1 | grep 'No such file'`, a test asserting a diagnostic, and a person
reading a log all read it, and none of them are reading the host's wording.

The fix is one line per site:

    let why = strerror(&e);
    let _ = writeln!(err, "cp: cannot stat {}: {why}", quoteaf_os(src));

`strerror` is written as a `let` binding rather than inlined into the format
string because most of these calls are multi-line `writeln!`s whose arguments
would otherwise have to be reordered -- a bigger diff for no gain.

# What is reported, and what is deliberately not

The rule is textual: a string literal that interpolates a binding named `e`,
`err`, `e2` or `error` by `Display`. That is the shape all 139 measured sites
are written in, and it is checkable without type inference, which a script that
does not compile Rust cannot have.

One shape is exempt, and it is the reason the gate can demand *zero* rather
than merely "no more than before": a literal that is exactly

    "<name>: {e}"

-- program name, colon, space, the error, end of string. That is the top-level
"the error already is the whole message" print in `main`, and in every bin here
it carries a `getopt::Error` or a `String`, never an `io::Error`. An `io::Error`
printed with *no* context at all would slip through; that is a separate and
more obvious diagnostic bug (`rm: No such file or directory` names no file), and
it is caught by the quote-names test rather than by this one.

A positional `{}` fed an error is also not matched. It cannot be, without
resolving which argument goes where; the convention in this tree is the named
form, and `--selftest` pins that the named form is what gets caught.

# The ratchet

A finding is keyed on the *file*, not the site, and only the first hit per file
is reported. The unit of work is a bin: converting `tar.rs` means converting all
19 of its sites, because a half-converted bin prints two different wordings for
the same failure, which is worse than one wrong one. So a file is in the
baseline or it is not, and a 52-line baseline is a baseline someone will read
where a 139-line one is not.

`--check` fails only on a file that is not in `scripts/host-errmsg-baseline.txt`.
A genuine false positive belongs in `IGNORE` below, which records *why*, rather
than in the baseline, which records only *that*.

Scope is `userspace/coreutils/` -- the shipped, on-`PATH` utilities whose
messages are what scripts and people actually read. See `known-issues.md` ->
`TD-B-COREUTILS-PRINT-THE-HOSTS-ERROR-TEXT`.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "host-errmsg-baseline.txt"

GATED = ROOT / "userspace" / "coreutils"

RULE = "host-error-text"
FIX = (
    "prints the host's error text, not POSIX's. Bind `let why = strerror(&e);` "
    "(coreutils::errmsg) and interpolate `{why}`."
)

# Bindings that hold an error. Not every `{e}` in the tree is one -- but every
# one of the 139 measured sites is, and a name this short is not used for
# anything else in these files.
ERR_NAMES = ("e", "err", "e2", "error")

# `{e}`, `{e:?}`, `{err:#}` -- the Display/Debug interpolation of an error
# binding. `{{e}}` is an escaped brace and is not an interpolation, which is why
# the opening brace is required not to be preceded by another one.
_INTERP = re.compile(
    r"(?<!\{)\{(" + "|".join(ERR_NAMES) + r")(?:[:][^{}]*)?\}"
)

# The exempt shape: the whole literal is `<name>: {e}`. `<name>` is the program
# writing the message, so it may hold the characters a bin name may hold.
_WHOLE_MESSAGE = re.compile(
    r"^[A-Za-z0-9_.\[\]-]+: \{(?:" + "|".join(ERR_NAMES) + r")\}$"
)

# Macros whose message is read by a developer looking at a failing test, not by
# a script or a user. `assert!(ok, "{err}")` is not a diagnostic and has no
# POSIX wording to get wrong. Excluding them is not a nicety: measured, seven
# bins' *only* hit was a test assertion, and three of those -- rm, mv, cp -- had
# just been converted. A gate that reports the files it has already fixed is a
# gate nobody believes.
TEST_MACROS = frozenset(
    {"assert", "assert_eq", "assert_ne", "debug_assert", "panic", "unreachable",
     "todo", "unimplemented", "expect"}
)

# `impl Display for E { fn fmt(&self, f: &mut Formatter) { write!(f, "{e}") } }`
# is a *source* error being forwarded, not an `io::Error` being printed: there
# is nothing there for `strerror` to translate, and the wrapper's own `Display`
# is what the eventual print will read. Recognised by the sink being named `f`,
# which is the formatter's conventional name and is not used for a diagnostic
# stream anywhere in this tree -- those are `err` and `out`.
FORMATTER_SINK = "f"

# Genuine false positives, keyed `<relative path>:<rule>` and valued with the
# reason -- the difference between this and the baseline, which records only
# that a finding exists.
IGNORE: dict[str, str] = {
    "userspace/coreutils/src/bin/awk/fmt.rs:host-error-text": (
        "`e` there is awk's exponent character in `%e` output -- "
        "`format!(\"{s}{e}{sign}{:02}\", ..)` -- not an error binding. A name "
        "collision, and the only one measured in 85 bins."
    ),
}

assert ":" not in RULE, "the rule name must not contain ':' -- it is the key separator"


def strip_comments(src: str) -> str:
    """Blank out Rust comments, keeping string literals and every byte offset.

    `raced-globals.py`'s `strip_comments_and_strings` is *not* reused here, and
    the reason is not laziness: it blanks string literals too, and the literal
    is the only place this tool's signal lives. Reusing it would make every file
    look clean, which is this gate's one unacceptable failure mode.

    Comments still have to go. A comment is the single most likely place for
    `{e}` to appear without being a print -- including in this file's own
    prose, and in the module docs of every bin that has just been converted and
    explains what it replaced.
    """
    out = list(src)
    i, n = 0, len(src)

    def blank(start: int, stop: int) -> None:
        for k in range(start, min(stop, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        # A raw string is skipped over rather than blanked: `r"..."` can hold a
        # `//` that is text, not a comment.
        m = re.compile(r"(?:b|c)?r(#*)\"").match(src, i)
        if m and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            close = '"' + m.group(1)
            end = src.find(close, m.end())
            i = n if end < 0 else end + len(close)
            continue
        if c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            i = j
            continue
        if c == "'" and i + 2 < n and (src[i + 1] != "\\" and src[i + 2] == "'"):
            # A char literal. `'a'` cannot open a string, but `'\''` and the
            # lifetime `'a` both start the same way, so only the plain form is
            # skipped -- the others hold nothing this cares about anyway.
            i += 3
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            end = src.find("\n", i)
            end = n if end < 0 else end
            blank(i, end)
            i = end
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        i += 1
    return "".join(out)


# A non-raw string literal, contents captured. Escapes are consumed in pairs so
# a `\"` does not end it.
_LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"', re.DOTALL)


def blank_literal_bodies(src: str) -> str:
    """Space out the *contents* of every string literal, keeping the quotes.

    The quotes stay so offsets and literal boundaries are unchanged; only the
    text between them goes, which is what `enclosing_call`'s bracket counting
    needs and what `sites` must still be able to read from the original.
    """
    out = list(src)
    for m in _LITERAL.finditer(src):
        for k in range(m.start(1), m.end(1)):
            if out[k] != "\n":
                out[k] = " "
    return "".join(out)


# The `ident` of `ident!(` or `ident![`, read backwards from the delimiter.
_MACRO_NAME = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\s*!\s*$")


def enclosing_call(src: str, pos: int) -> tuple[str, str]:
    """The macro invocation a byte offset sits inside, as `(name, first arg)`.

    Scans left from `pos` counting brackets until an unmatched opener, then
    reads the identifier before it. Returns `("", "")` if that identifier is not
    followed by `!` -- i.e. the offset is inside a plain function call, not a
    macro.

    Backwards rather than forwards because the alternative is matching a
    macro invocation with a regex, and these span lines and nest: `eprintln!`
    calls hold `quotef_os(..)`, `writeln!` calls hold `format!(..)`. Bracket
    counting is exact where a regex would guess.

    `src` must have had comments blanked *and* string-literal bodies blanked,
    or a bracket that is text derails the count. That is not hypothetical: the
    first draft missed `assert!(err.contains("a["), "{err}")` in `grep.rs`,
    because the `[` inside `"a["` read as an unmatched opener and the scan
    stopped one bracket short of the `assert!`.
    """
    depth = 0
    i = pos - 1
    while i >= 0:
        c = src[i]
        if c in ")]}":
            depth += 1
        elif c in "([{":
            if depth == 0:
                break
            depth -= 1
        i -= 1
    if i < 0:
        return ("", "")
    m = _MACRO_NAME.search(src, 0, i)
    if not m or m.end() != i:
        return ("", "")
    # The first argument, for telling a `Display` impl's formatter from a
    # diagnostic sink. Only the leading identifier is needed.
    rest = src[i + 1 :].lstrip()
    arg = re.match(r"[A-Za-z_][A-Za-z0-9_]*", rest)
    return (m.group(1), arg.group(0) if arg else "")


def sites(path: Path) -> list[tuple[int, str]]:
    """Every offending literal in one file, as `(line number, the line)`.

    Used by `--list` and by the selftest; `analyse` keeps only the first.
    """
    try:
        raw = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    src = strip_comments(raw)
    scan = blank_literal_bodies(src)
    lines = raw.splitlines()

    out: list[tuple[int, str]] = []
    for m in _LITERAL.finditer(src):
        body = m.group(1)
        if not _INTERP.search(body):
            continue
        if _WHOLE_MESSAGE.match(body):
            continue
        macro, first_arg = enclosing_call(scan, m.start())
        if macro in TEST_MACROS:
            continue
        if macro in {"write", "writeln"} and first_arg == FORMATTER_SINK:
            continue
        line = src.count("\n", 0, m.start()) + 1
        out.append((line, lines[line - 1].strip() if line <= len(lines) else ""))
    return out


def analyse(path: Path) -> list[tuple[str, int, str]]:
    """`(rule, line, text)` for the first offending literal in one file.

    One entry per file, because the unit of repair is a whole bin -- see the
    module docstring.
    """
    found = sites(path)
    return [(RULE, found[0][0], found[0][1])] if found else []


def _relpath(p: Path) -> str:
    try:
        return p.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return p.as_posix()


def rust_files(under: Path) -> list[Path]:
    """Every `.rs` file below `under`, pruning build output rather than
    filtering it, so a multi-gigabyte `target/` is never walked."""
    out: list[Path] = []
    stack = [under]
    while stack:
        d = stack.pop()
        try:
            entries = list(d.iterdir())
        except OSError:
            continue
        for e in entries:
            if e.is_dir():
                if e.name in {"target", ".git"} or e.name.startswith("target-"):
                    continue
                stack.append(e)
            elif e.suffix == ".rs":
                out.append(e)
    return out


def findings(under: Path) -> dict[str, tuple[int, str]]:
    out: dict[str, tuple[int, str]] = {}
    for path in sorted(rust_files(under)):
        rel = _relpath(path)
        for rule, line, text in analyse(path):
            key = f"{rel}:{rule}"
            if key in IGNORE:
                continue
            out[key] = (line, text)
    return out


def load_baseline() -> set[str]:
    if not BASELINE.is_file():
        return set()
    out = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def selftest() -> int:
    """Check the rule that decides what this tool reports.

    A detector that fails toward silence looks exactly like a clean tree. This
    one is more exposed to that than most, because its signal lives *inside* a
    string literal and the obvious lexer to reach for blanks literals out.
    """
    import tempfile

    failures: list[str] = []
    rules: list[str] = []
    current = ""

    def classify(src: str) -> int:
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "x.rs"
            p.write_text(src, encoding="utf-8")
            return len(sites(p))

    def rule(name: str) -> None:
        nonlocal current
        current = name
        rules.append(name)

    def expect(label: str, got: object, want: object) -> None:
        if got != want:
            failures.append(f"{current}: {label}: want {want!r}, got {got!r}")

    # 1. The base case, in the exact shape the 139 sites are written in.
    rule("base")
    expect(
        "bare",
        classify(r'fn f() { eprintln!("du: {}: {e}", quotef_os(path)); }'),
        1,
    )
    expect("no-subject", classify(r'fn f() { eprintln!("tar: write error: {e}"); }'), 1)
    expect(
        "map_err",
        classify(r'fn f() { fs::read(p).map_err(|e| format!("{e}"))?; }'),
        1,
    )
    expect("debug", classify(r'fn f() { eprintln!("x: {}: {e:?}", p); }'), 1)
    expect("named-err", classify(r'fn f() { eprintln!("x: {}: {err}", p); }'), 1)

    # 2. The fix must not match, or the tool flags its own remedy and stops
    #    being believed.
    rule("the-fix-is-clean")
    expect(
        "why",
        classify(
            'fn f() { let why = strerror(&e); eprintln!("cp: {}: {why}", quoteaf_os(s)); }'
        ),
        0,
    )

    # 3. The exempt shape. Every converted bin keeps exactly one of these -- the
    #    `getopt::Error` print in `main` -- so if it matched, the baseline could
    #    never reach zero and the gate would measure nothing.
    rule("whole-message-exempt")
    expect("exempt", classify(r'fn main() { eprintln!("rm: {e}"); }'), 0)
    expect("exempt/dashed", classify(r'fn main() { eprintln!("md5sum: {e}"); }'), 0)
    # ...but only when the error really is the whole message. One extra word and
    # it is a context-carrying diagnostic again.
    expect("not-exempt/prefix", classify(r'fn f() { eprintln!("rm: cannot: {e}"); }'), 1)
    expect("not-exempt/suffix", classify(r'fn f() { eprintln!("rm: {e}!"); }'), 1)
    expect("not-exempt/alone", classify(r'fn f() { format!("{e}"); }'), 1)

    # 4. Prose about the defect is not the defect -- and a bin that has just
    #    been converted is the likeliest place on earth for `{e}` to appear in a
    #    comment explaining what it replaced.
    rule("comments")
    expect(
        "line-comment",
        classify('fn f() { /* was eprintln!("x: {e}") */ g(); }\n// or "y: {e}"\n'),
        0,
    )
    expect(
        "doc-comment",
        classify('/// Prints `"x: {}: {e}"`.\nfn f() { g(); }\n'),
        0,
    )

    # 5. Escaped braces are literal text, not an interpolation. `{{e}}` prints
    #    `{e}`, which is what a usage message showing a format string does.
    rule("escaped-braces")
    expect("escaped", classify(r'fn f() { println!("use {{e}} for the error"); }'), 0)

    # 6. A test assertion is not a diagnostic. This is the case that matters
    #    most after `the-fix-is-clean`: seven bins' only hit was an assertion,
    #    three of them bins that had just been converted.
    rule("assertions-are-not-diagnostics")
    expect("assert", classify(r'fn t() { assert!(ok, "{err}"); }'), 0)
    expect(
        "assert-with-context",
        classify(r'fn t() { assert!(e.contains("x"), "{src}: {e}"); }'),
        0,
    )
    expect(
        "assert_eq",
        classify(r'fn t() { assert_eq!(e.referral, None, "{option}: {e}"); }'),
        0,
    )
    expect(
        "panic",
        classify(r'fn t() { parse(s).unwrap_or_else(|e| panic!("at {s:?}: {e}")); }'),
        0,
    )
    # A bracket that is *text* must not derail the backwards scan. This is a
    # real miss, not a hypothetical: `grep.rs` was reported for exactly this
    # line because the `[` inside `"a["` read as an unmatched opener.
    expect(
        "bracket-in-a-literal",
        classify(r'fn t() { assert!(err.contains("a["), "{err}"); }'),
        0,
    )
    # The exclusion is by macro, so a diagnostic nested *inside* an assertion's
    # arguments is still a diagnostic. This is what bracket counting buys over
    # "does the line start with assert".
    expect(
        "nested-print-still-counts",
        classify(r'fn t() { assert!(run(&mut |x| eprintln!("a: {}: {e}", x))); }'),
        1,
    )

    # 7. A `Display` impl forwarding its source error has nothing for
    #    `strerror` to translate.
    rule("display-forwarding")
    expect(
        "formatter",
        classify(r'fn fmt(&self, f: &mut Formatter) { write!(f, "{e}") }'),
        0,
    )
    # ...and the same macro against a diagnostic sink is the defect, which is
    # the whole reason this is keyed on the argument and not on the macro.
    expect(
        "diagnostic-sink",
        classify(r'fn g(err: &mut W) { writeln!(err, "realpath: {p}: {e}"); }'),
        1,
    )

    # 8. Several sites in one file are one finding, because the unit of repair
    #    is the whole bin.
    rule("one-finding-per-file")
    with tempfile.TemporaryDirectory() as d:
        p = Path(d) / "x.rs"
        p.write_text(
            'fn f() { eprintln!("a: {}: {e}", x); eprintln!("b: {}: {e}", y); }',
            encoding="utf-8",
        )
        expect("sites", len(sites(p)), 2)
        expect("findings", len(analyse(p)), 1)

    # 7. The gated tree must really be there. Every case above proves the rule
    #    classifies a *given* file; none would notice `GATED` pointing at a
    #    directory that had been renamed away, which would make `--check` pass
    #    forever while looking at nothing.
    rule("gated-tree-is-not-empty")
    expect("dir-exists", GATED.is_dir(), True)
    expect("has-files", len(rust_files(GATED)) > 50, True)

    for f in failures:
        print(f"selftest FAIL {f}")
    print(
        f"selftest: {len(rules) - len({f.split(':')[0] for f in failures})}"
        f"/{len(rules)} rules ok"
    )
    return 1 if failures else 0


def main() -> int:
    args = sys.argv[1:]
    if "--selftest" in args:
        return selftest()
    check = "--check" in args
    write = "--write-baseline" in args
    listing = "--list" in args

    gated = findings(GATED)

    if listing:
        total = 0
        for path in sorted(rust_files(GATED)):
            # Honour IGNORE here too. A listing that counted a file the gate
            # has ruled out would report a total the gate disagrees with, and
            # the listing is what the burn-down is measured against.
            if f"{_relpath(path)}:{RULE}" in IGNORE:
                continue
            found = sites(path)
            if not found:
                continue
            total += len(found)
            for line, text in found:
                print(f"{_relpath(path)}:{line}  {text}")
        print(f"\n{total} site(s) in {len(gated)} file(s).")
        return 0

    if write:
        body = [
            "# Utilities that print the host's error text instead of POSIX's --",
            "# `The system cannot find the file specified. (os error 2)` where",
            "# every reader expects `No such file or directory`.",
            "# Generated by scripts/host-errmsg.py --write-baseline.",
            "#",
            "# One line per bin, because the unit of repair is a whole bin: a",
            "# half-converted one prints two wordings for the same failure.",
            "#",
            "# This file is a ratchet and only ever shrinks. Do NOT add a line to",
            "# turn a red --check green. Fix it: `let why = strerror(&e);` from",
            "# coreutils::errmsg, then interpolate `{why}`.",
            "#",
            "# A genuine false positive belongs in the IGNORE table in the script,",
            "# which records *why*, not here, which records only *that*.",
            "",
        ]
        body += sorted(gated)
        BASELINE.write_text("\n".join(body) + "\n", encoding="utf-8", newline="")
        print(f"wrote {_relpath(BASELINE)} with {len(gated)} entries")
        return 0

    known = load_baseline()
    new = sorted(k for k in gated if k not in known)

    to_show = new if check else sorted(gated)
    for key in to_show:
        line, text = gated[key]
        path, _rule = key.rsplit(":", 1)
        mark = "NEW " if key in set(new) else "    "
        print(f"{mark}{path}:{line}  {text}")

    print(f"\n{len(gated)} file(s) affected; {len(new)} not in the baseline.")
    if new and check:
        print(f"\n  {RULE}: {FIX}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
