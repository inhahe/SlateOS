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

`--check` fails on a file that is not in `scripts/host-errmsg-baseline.txt` --
and on a baseline line that names a file which no longer has the defect. A
ratchet only ratchets if it shrinks when the work is done: 17 of the 24 lines
here had been fixed and left listed, and every one of them was a bin that could
have regressed all the way back with the gate still green. Regenerating is one
command, `--write-baseline`, and it can only ever remove lines, because a file
that still has the defect is still found.

A genuine false positive belongs in `IGNORE` below, which records *why*, rather
than in the baseline, which records only *that*.

Scope is `userspace/coreutils/` -- the shipped, on-`PATH` utilities whose
messages are what scripts and people actually read. See `known-issues.md` ->
`TD-B-COREUTILS-PRINT-THE-HOSTS-ERROR-TEXT`.

WHICH TREE IS JUDGED

    python scripts/host-errmsg.py --check              # judge the working tree
    python scripts/host-errmsg.py --check --head <rev> # judge that revision

Without `--head` this reads the working tree, which is what a run by hand and a
run from the boot test both mean. The push hook passes `--head <sha>` for each
commit being pushed, because the question at that boundary is about the code
being published and not about whatever is on the disk at the time -- see
`known-issues.md` ->
`TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH`, and gate 7,
which had exactly this defect and published two unformatted commits under a
green gate.

**Both** inputs go through the `Tree` seam, not just the sources: the `.rs`
files *and* `scripts/host-errmsg-baseline.txt`. The baseline is a suppression
list, so reading it off the disk while judging a revision would let an
uncommitted baseline edit silence a finding in a commit that does not contain
the silencing line -- a false pass of exactly the shape this conversion exists
to remove, and one that no test of the sources alone can see.
`scripts/test-checkers-honour-head.py` makes each input disagree between commit
and worktree in turn.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "host-errmsg-baseline.txt"

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gittree  # noqa: E402

# The two inputs again, relative and `/`-separated, which is the only spelling
# the `Tree` seam accepts. `BASELINE` above survives alongside `BASELINE_REL`
# because `--write-baseline` writes to the disk by definition -- it is a repair
# action, not a verdict -- while everything that *reads* goes through the seam.
BASELINE_REL = "scripts/host-errmsg-baseline.txt"
GATED_REL = "userspace/coreutils"

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
    "userspace/coreutils/tests/diagnostics_quote_names.rs:host-error-text": (
        "The `{e}`s there are inside `r#\"...\"#` fixtures -- literal copies of "
        "pre-sweep diagnostics that the test feeds to its own detector to prove "
        "the detector still fires. They are data, not prints: nothing in this "
        "file writes to stderr at all. Converting them to `{why}` would be "
        "converting the *test input*, which is the one edit that makes that "
        "test pass while detecting nothing."
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


def sites(tree: gittree.Tree, rel: str) -> list[tuple[int, str]]:
    """Every offending literal in one file, as `(line number, the line)`.

    Used by `--list` and by the selftest; `analyse` keeps only the first.

    A file the seam cannot read is no findings rather than an error, which is
    what the previous `except OSError` meant and is kept deliberately: the file
    list comes from the same tree an instant earlier, so the only ways to get
    here are a race with a concurrent edit and a submodule gitlink, and neither
    is a statement about the code being judged.
    """
    raw = tree.read_text(rel)
    if raw is None:
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


def analyse(tree: gittree.Tree, rel: str) -> list[tuple[str, int, str]]:
    """`(rule, line, text)` for the first offending literal in one file.

    One entry per file, because the unit of repair is a whole bin -- see the
    module docstring.
    """
    found = sites(tree, rel)
    return [(RULE, found[0][0], found[0][1])] if found else []


def _relpath(p: Path) -> str:
    try:
        return p.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return p.as_posix()


def rust_files(tree: gittree.Tree, prefix: str) -> list[str]:
    """Every `.rs` file below `prefix`, skipping build output.

    The pruning is the seam's: it skips `target/`, `.git` and the `target-*`
    family *while walking* rather than filtering the results, which is the
    difference between a gate that runs in a second and one that descends tens
    of gigabytes of generated sources inside a push.

    This was a hand-rolled walk carrying its own copy of that rule. The copy is
    deleted rather than kept, because two spellings of one rule is one rule that
    drifts -- and the two had already drifted apart in a way that happened not
    to matter yet: the local copy pruned any *directory* whose name starts with
    `target-`, while the seam judges a prefix naming a file by file rules first,
    so `posix/src/target-arch.rs` -- a tracked source file -- survives in the
    seam and is pinned there by a case. Out of this checker's scope today; one
    `userspace/coreutils/…/target-*.rs` away from mattering.
    """
    return [rel for rel in tree.files_under(prefix)
            if rel.rsplit("/", 1)[-1].endswith(".rs")]


def findings(tree: gittree.Tree, prefix: str) -> dict[str, tuple[int, str]]:
    out: dict[str, tuple[int, str]] = {}
    for rel in rust_files(tree, prefix):
        for rule, line, text in analyse(tree, rel):
            key = f"{rel}:{rule}"
            if key in IGNORE:
                continue
            out[key] = (line, text)
    return out


def load_baseline(tree: gittree.Tree) -> set[str]:
    """The baselined backlog, read from the tree being judged.

    From the tree and not the disk, for the same reason as the sources: this
    file is a *suppression* list, so a baseline edited but not committed would
    otherwise silence a finding in a commit that does not contain the silencing
    line. That is a false pass whose every visible symptom -- a green gate, a
    clean summary -- is identical to being genuinely clean.
    """
    text = tree.read_text(BASELINE_REL)
    if text is None:
        return set()
    out = set()
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def stale_entries(known: set[str], gated: set[str]) -> list[str]:
    """Baseline lines naming a finding that is no longer there.

    A function rather than an expression in `main` only so that `--selftest`
    can reach it. That is not ceremony: this guard fails toward *silence* in
    the same way the detector does -- a version of it that never fires looks
    exactly like a baseline that happens to be exact, which is what let this
    very ratchet accumulate 17 dead lines unnoticed.
    """
    return sorted(known - gated)


def _inputs_missing(tree: gittree.Tree, needs_baseline: bool) -> str | None:
    """Why this tree cannot be judged at all, or `None` if it can.

    Every rule in `--selftest` proves the detector classifies a *given* file
    correctly. None of them would notice `GATED_REL` naming a directory that
    had been renamed away, or `rust_files` pruning too eagerly: the listing
    would come back empty, nothing would be new, and the gate would pass
    forever while looking at nothing. A clean report produced by accident is
    the exact failure this tool exists to prevent, so it is worth asking.

    The baseline is the second input and fails the same way for a different
    reason. Unreadable through the seam it reads as an *empty backlog*, so
    `--check` calls every baselined bin NEW and refuses the push with one
    paragraph each, blaming bins nobody touched -- or, on a clean tree, calls
    every baseline line stale. How loud that is depends on how long the ratchet
    still is (two lines today, and it only ever shrinks), but not whether it
    happens. Neither outcome is silent, and both are a false accusation,
    which `scripts/run-checker.sh` exists to argue is the worst thing a gate
    can do. `needs_baseline` is false for `--write-baseline`, which creates the
    file and so must be allowed to run without it, and for `--list`, which
    reports sites and never consults it.

    Asked of the tree under judgement rather than of the disk, because that is
    where the risk lives: a commit that moves either path disarms the gate *for
    that commit*, and a disk-side question answers for a working tree that
    still has both. Non-emptiness rather than a count -- a threshold would be a
    claim about this repository, and this checker is run against fixtures too.
    """
    if not rust_files(tree, GATED_REL):
        return (f"no .rs files under {GATED_REL} -- the gate has nothing to "
                f"judge, which is not the same as a clean tree. Has the "
                f"directory moved?")
    if needs_baseline and tree.read_text(BASELINE_REL) is None:
        return (f"cannot read {BASELINE_REL} -- without the backlog every "
                f"baselined bin reads as a new finding, so this would refuse "
                f"the push over a file that moved rather than over any code.")
    return None


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
            (Path(d) / "x.rs").write_text(src, encoding="utf-8", newline="")
            # Through a real `WorkTree`, not a bare read, so the selftest
            # exercises the same seam the gate does. A selftest that bypasses
            # the seam cannot see a seam-shaped defect.
            with gittree.WorkTree(d) as tree:
                return len(sites(tree, "x.rs"))

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
        newline="")
        with gittree.WorkTree(d) as tree:
            expect("sites", len(sites(tree, "x.rs")), 2)
            expect("findings", len(analyse(tree, "x.rs")), 1)

    # 7. The staleness guard. It fails toward silence exactly as the detector
    #    does -- a version of it that never fires is indistinguishable from a
    #    baseline that happens to be exact -- so it needs its own cases. This
    #    is the ratchet that carried 17 dead lines, so the guard is not
    #    hypothetical here.
    rule("baseline-staleness")
    expect("stale/exact-baseline-is-clean",
           stale_entries({f"a.rs:{RULE}"}, {f"a.rs:{RULE}"}), [])
    expect("stale/fixed-finding-is-reported",
           stale_entries({f"a.rs:{RULE}", f"b.rs:{RULE}"}, {f"a.rs:{RULE}"}),
           [f"b.rs:{RULE}"])
    # A *new* finding is the other ratchet direction and is not staleness; it
    # must not leak into this list, or the fix advice printed would be wrong.
    expect("stale/new-finding-is-not-stale",
           stale_entries({f"a.rs:{RULE}"}, {f"a.rs:{RULE}", f"c.rs:{RULE}"}),
           [])
    expect("stale/empty-baseline-is-clean",
           stale_entries(set(), {f"a.rs:{RULE}"}), [])
    # Several dead lines come back sorted, because they are printed in order.
    expect("stale/multiple-are-sorted",
           stale_entries({f"z.rs:{RULE}", f"a.rs:{RULE}", f"m.rs:{RULE}"}, set()),
           [f"a.rs:{RULE}", f"m.rs:{RULE}", f"z.rs:{RULE}"])

    # Nothing above touches a tree, deliberately. "The inputs are really there"
    # is the other thing that has to be true before a clean report means
    # anything, and it used to be an eighth rule here, asking the working tree
    # whether `userspace/coreutils` held more than fifty `.rs` files and whether
    # the baseline could be read. That was wrong twice over, in the way gate 4's
    # identical rule was wrong before it (`scripts/argv-utf8.py`, `_no_corpus`):
    #
    #   * It asked the *disk* about a run that may be judging a revision. A
    #     commit that renames the gated directory away disarms the gate for that
    #     commit, while a disk-side self-test standing in a working tree that
    #     still has the directory reports all is well -- which is precisely the
    #     working-tree-versus-push defect the `--head` conversion exists to
    #     remove, hiding inside the thing that certifies the conversion.
    #   * A threshold of fifty is a claim about *this checkout*, so the checker
    #     could not be self-tested anywhere else -- including in the fixtures of
    #     `scripts/test-checkers-honour-head.py`, which is where the hook's own
    #     `--head` wiring is proved. Those cases are what caught this.
    #
    # Both questions now live in `main`, asked of whichever tree is under
    # judgement. See `_inputs_missing`.
    for f in failures:
        print(f"selftest FAIL {f}")
    print(
        f"selftest: {len(rules) - len({f.split(':')[0] for f in failures})}"
        f"/{len(rules)} rules ok"
    )
    return 1 if failures else 0


def main() -> int:
    argv = sys.argv[1:]
    if "--selftest" in argv:
        return selftest()

    ap = argparse.ArgumentParser(add_help=True)
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--list", dest="listing", action="store_true")
    ap.add_argument("--write-baseline", dest="write", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--head", metavar="REV",
                    help="judge this revision instead of the working tree")
    args = ap.parse_args(argv)
    check, write, listing = args.check, args.write, args.listing

    # `--write-baseline` regenerates the file on disk from what is found. Doing
    # that from a revision would write a baseline describing a tree that is not
    # the one being written into -- so the two are refused together rather than
    # silently producing a file that matches nothing.
    if write and args.head:
        print("host-errmsg: --write-baseline writes the working tree, so it "
              "cannot be combined with --head", file=sys.stderr)
        return 2

    try:
        tree = gittree.open_tree(str(ROOT), args.head)
    except gittree.GitTreeError as exc:
        print(f"host-errmsg: cannot read {args.head!r}: {exc}", file=sys.stderr)
        return 2

    with tree:
        # Before anything is reported, ask whether there is anything to report
        # *on*. Exit 2 rather than 1 for run-checker.sh's reason: the gate has
        # lost an input, which is not a finding about anybody's code, and
        # printing gate 6's refusal over it would tell the author a utility of
        # theirs prints Windows' wording when what actually happened is that a
        # path moved.
        why = _inputs_missing(tree, needs_baseline=not (write or listing))
        if why is not None:
            where = f"in {args.head}" if args.head else "in the working tree"
            print(f"host-errmsg: {where}, {why}", file=sys.stderr)
            return 2
        return _run(tree, check, write, listing)


def _run(tree: gittree.Tree, check: bool, write: bool, listing: bool) -> int:
    gated = findings(tree, GATED_REL)

    if listing:
        total = 0
        for rel in rust_files(tree, GATED_REL):
            # Honour IGNORE here too. A listing that counted a file the gate
            # has ruled out would report a total the gate disagrees with, and
            # the listing is what the burn-down is measured against.
            if f"{rel}:{RULE}" in IGNORE:
                continue
            found = sites(tree, rel)
            if not found:
                continue
            total += len(found)
            for line, text in found:
                print(f"{rel}:{line}  {text}")
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

    known = load_baseline(tree)
    new = sorted(k for k in gated if k not in known)
    stale = stale_entries(known, set(gated))

    to_show = new if check else sorted(gated)
    for key in to_show:
        line, text = gated[key]
        path, _rule = key.rsplit(":", 1)
        mark = "NEW " if key in set(new) else "    "
        print(f"{mark}{path}:{line}  {text}")

    for key in stale:
        print(f"FIXED {key}  -- in the baseline but no longer found")

    print(
        f"\n{len(gated)} file(s) affected; {len(new)} not in the baseline; "
        f"{len(stale)} baseline line(s) now stale."
    )
    if check and (new or stale):
        if new:
            print(f"\n  {RULE}: {FIX}")
        if stale:
            print(
                "\n  The baseline lines above name files that are already "
                "fixed. Shrink it:\n"
                f"      python {_relpath(Path(__file__))} --write-baseline\n"
                "  It cannot lose a real finding -- a file that still has the "
                "defect is still found."
            )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
