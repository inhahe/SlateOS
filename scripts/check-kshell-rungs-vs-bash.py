#!/usr/bin/env python3
"""Check rung 115's assertions against real bash, exactly as written.

`check-kshell-pipeline-vs-bash.py` pins the *rules*; this pins the *literals
typed into the rung*, which is a different risk: a rule can be right and the
case transcribed into Rust can still be a different string than the one that
was measured, because both Rust and Python re-escape backslashes on the way
in.  Requires WSL; see `bashprobe.py`.

So each entry below carries the Rust source text as well.  If the two ever
disagree about how many backslashes there are, that is visible here rather
than as a mysterious boot-test failure.

Only the cases that are questions about *bash* are listed.  `expand_braces`
is our own stage (it runs before word splitting and preserves text, which
bash has no equivalent of), so its assertions are not checkable here and are
deliberately absent rather than faked.

WHAT THIS FILE READS OUT OF THE RUST, AND WHY IT IS THREE-WAY
-------------------------------------------------------------
For a long time this file read the rung *inputs* out of kshell.rs and nothing
else.  `assert_rust_src_is_verbatim` proved the string we asked bash about was
the string the rung was written on -- a real floor, and it has caught a real
error -- but the rung's *expected value* was never opened.  The consequence was
demonstrated by mutation rather than argued: changing rung 115's

    assert_eq!(split_words("a b  c"), alloc::vec!["a", "b", "c"], ..)

to `alloc::vec!["a", "b", "", "c"]` -- a blank word bash never produces -- left
this file printing `0 rung assertion(s) disagree with the reference tool` and
exiting 0.  It was looking straight at the defect and had no way to see it,
because the only thing it compared against bash was `want`, a transcription
sitting in this file.  A rung grades the implementation; it cannot grade
itself, and closing *that* gap is the entire reason these oracles exist.

So every case is now checked three ways, and all three must agree:

  1. the expectation written in the rung, read out of kshell.rs by
     `scripts/rustrungs.py`;
  2. the `want` transcribed into `CASES` below;
  3. what real bash produces.

(2) is deliberately kept rather than dropped as redundant.  Without it a
silently broken reader -- one that finds nothing, or parses the wrong
argument -- would compare bash against bash and pass; the third witness is the
same argument that put `assert_rust_src_is_verbatim` here in the first place.
Leg (1) needs no WSL, so on a host without bash it still runs and can still
fail: a corrupted rung is caught everywhere, not only where bash exists.
"""
import contextlib
import io
import pathlib
import sys

import bashprobe
import rustrungs

KSHELL = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "kshell.rs"


def assert_rust_src_is_verbatim(src: str | None = None, cases=None):
    """Every `rust_src` must occur in kshell.rs, byte for byte.

    `src` and `cases` are injectable so the self-test can drive this against a
    fixture rather than against the real `kshell.rs`. That matters more than it
    looks: `kshell.rs` is **lane A's file**, so a self-test that read it would
    be a lane-B test that lane A can turn red by editing its own code -- which
    is the one thing lane A's cross-lane rule says a self-test must never be
    able to do (`requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-
    not-cover.md` §4: a self-test reads only fixtures the checker carries in
    its own source).

    Without this the field is decoration.  The docstring above says this file
    exists because "the case transcribed into Rust can still be a different
    string than the one that was measured" -- but nothing compared the two, so
    catching that depended on a human reading the printed line and counting
    backslashes by eye, which is the task the file was written to remove.

    It was already wrong when this check was added: six of the thirteen
    entries carried a spurious backslash before every apostrophe (`\\'`), which
    is Python's escaping leaking into a field whose whole purpose is to be a
    verbatim copy of Rust.  Rust does not escape `'` inside a `"..."` literal,
    so none of the six existed in kshell.rs at all.  The backslash *counts* --
    the thing the file is actually about -- were right in all thirteen, which
    is why it went unnoticed: the error was in the one character the file was
    not looking at.

    This doubles as the discovery floor.  A renamed or truncated kshell.rs
    fails every lookup instead of quietly leaving the transcription unchecked.
    """
    if src is None:
        src = KSHELL.read_text(encoding="utf-8", errors="surrogateescape")
    if cases is None:
        cases = CASES
    missing = [c[1] for c in cases if c[1] not in src]
    if not missing:
        return
    hint = ""
    if all(m.replace("\\'", "'") in src for m in missing):
        hint = (
            "\n  All of them ARE present with `\\'` written as `'`. Rust does not\n"
            "  escape an apostrophe inside a string literal -- drop the backslash."
        )
    raise SystemExit(
        f"{len(missing)} of {len(cases)} rust_src literal(s) do not occur in\n"
        f"  {KSHELL}\n"
        "Either the rung was edited and this file was not, or the "
        "transcription is wrong.\n"
        "Until they agree, this file is not checking what it claims to check."
        + hint
        + "\n\n  "
        + "\n  ".join(missing)
    )


def W(*words):
    return [w.encode() for w in words]


# (rung function, rust source as typed in kshell.rs, the actual bytes,
#  expected words)
#
# The leading function name is what makes leg (1) possible: `rustrungs` needs
# the whole call text to find the rung, and the same input appears under two
# different functions (`"a\\ b"` is a rung of both `remove_quotes` and
# `split_words`, asserting the same answer for different reasons).  Looking it
# up by input alone would read whichever came first in the file.
CASES = [
    # --- remove_quotes: one word in, one word out. ---------------------
    ("remove_quotes", r'''"\"it's fine\""''', '"it\'s fine"', W("it's fine")),
    ("remove_quotes", r'"a\\ b"', "a\\ b", W("a b")),
    ("remove_quotes", r'"\"C:\\dir\""', '"C:\\dir"', W("C:\\dir")),
    ("remove_quotes", r'"\"say \\\"hi\\\"\""', '"say \\"hi\\""', W('say "hi"')),
    ("remove_quotes", r'"\"a\\\\b\""', '"a\\\\b"', W("a\\b")),
    ("remove_quotes", r'''"'a\\\\b'"''', "'a\\\\b'", W("a\\\\b")),
    ("remove_quotes", r'''"'a'\\''b'"''', "'a'\\''b'", W("a'b")),
    ("remove_quotes", r'''"a'b'c"''', "a'b'c", W("abc")),

    # --- remove_quotes, the block at kshell.rs:10888. ------------------
    # These three were graded by nothing until leg (1) went in.  They are not
    # new rungs; they are rungs this file did not know existed, found by
    # reading the Rust instead of a Python transcription of it.
    ("remove_quotes", r'''"'a   b'"''', "'a   b'", W("a   b")),
    ("remove_quotes", r'''"a\"b c\"d"''', 'a"b c"d', W("ab cd")),
    ("remove_quotes", r'''"\"it's\""''', '"it\'s"', W("it's")),

    # --- split_words: arity and content. -------------------------------
    ("split_words", r'"a\\ b"', "a\\ b", W("a b")),
    ("split_words", r'"\"a b\" c"', '"a b" c', W("a b", "c")),
    ("split_words", r'"a b  c"', "a b  c", W("a", "b", "c")),
    ("split_words", r'''"x'y z'w"''', "x'y z'w", W("xy zw")),
    ("split_words", r'''"\"a'b\" c"''', '"a\'b" c', W("a'b", "c")),
]

# DELIBERATELY ABSENT: `remove_quotes("a 'b,c'")`, the rung at kshell.rs:10882.
#
# Its input contains an *unquoted* space, so bash splits it into two words
# (`a`, `b,c`) while `remove_quotes` splits nothing at all and returns the one
# string `a b,c`.  Both are right; they answer different questions, and
# `bashprobe.words()` can only ask bash's.  Listing it with `W("a", "b,c")`
# would make leg (3) green while leg (1) went red, and listing it with
# `W("a b,c")` would make leg (1) green while leg (3) went red -- so the only
# way to "cover" it is to invent a comparison neither tool is answering.  That
# is the false coverage this file was rewritten to remove, so it stays out and
# says why.


# --- rung 117: awk, whose oracle is awk and not bash. ----------------------
#
# `awk_split_print_args` is internal, so what is checkable from outside is
# what real awk *prints*.  Both cases are about the same disagreement between
# the two languages: awk honours `\"` inside a string and treats `'` as an
# ordinary character, where the shell does neither.  The second case is the
# control -- it is what would break if the shared shell scanner were
# substituted here, which is why the rung refuses that substitution.
#
# (program body, expected stdout, what it pins)
AWK_CASES = [
    (
        r'{ print "a\"b", "c" }',
        'a"b c\n',
        "an escaped quote does not close the string, so the comma separates",
    ),
    (
        "{ print \"it's\", \"x\" }",
        "it's x\n",
        "an apostrophe is data, so the comma still separates",
    ),
    (
        r'{ print "a,b" }',
        "a,b\n",
        "a comma inside a string is not a separator",
    ),
]


def check_rungs_against_transcription(src, cases, quiet=False):
    """Leg (1): what the rung asserts vs. what `CASES` says it asserts.

    `src` and `cases` are injectable for the same reason they are on
    `assert_rust_src_is_verbatim` -- so the self-test drives this against a
    fixture it carries itself and never against lane A's `kshell.rs`.

    Needs no bash.  That is the point of running it first: a rung corrupted in
    the tree is a finding on every host, including the ones where legs (2) and
    (3) will shortly be skipped for want of WSL.
    """
    fails = 0
    for func, rust_src, _actual, want in cases:
        call = f"{func}({rust_src})"
        sites = rustrungs.expectations(src, call)
        if not sites:
            fails += 1
            print(f"FAIL no rung `{call}` -- this file grades a call the tree "
                  f"does not make")
            continue
        # Every occurrence, not the first: a rung can be duplicated, and two
        # copies that disagree is exactly the state worth reporting.
        for line, expr, rung in sites:
            where = f"{call} at kshell.rs:{line}"
            if not rung:
                fails += 1
                print(f"FAIL {where} -- expected side has no string literal: "
                      f"{expr}")
            elif rung != want:
                fails += 1
                print(f"FAIL {where}")
                print(f"       rung asserts  : {rung}")
                print(f"       this file says: {want}")
            elif not quiet:
                print(f"ok   {where} -> {rung}")
    return fails


def check_awk():
    """Ask real awk what rung 117's cases print."""
    fails = 0
    print("\n--- rung 117, against real awk ---")
    for body, want, why in AWK_CASES:
        # The program reaches awk through a quoted here-doc written to a file
        # and then read with `-f`, so neither bash nor the argv transport can
        # reinterpret a backslash on the way in -- the same hazard this whole
        # file exists to rule out, arriving through a different door.
        script = (
            b"tmp=$(mktemp)\ncat > \"$tmp\" <<'AWK_EOF'\n"
            + body.encode()
            + b"\nAWK_EOF\necho x | awk -f \"$tmp\"\nrm -f \"$tmp\"\n"
        )
        r = bashprobe.run(script)
        got = r.stdout.decode("utf-8", "replace")
        ok = r.returncode == 0 and got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} awk {body}")
        print(f"       awk ={got!r}")
        if not ok:
            print(f"       rung={want!r}   <-- the rung is wrong ({why})")
    return fails


#: Floors on discovery. Not targets -- set where a gutted table or a merge
#: that took one side of a conflict trips them and ordinary editing does not.
MIN_CASES = 14
MIN_AWK_CASES = 2


def _assert_tables_are_not_gutted() -> None:
    """Refuse to grade tables too thin to be the ones this file was written on.

    `assert_rust_src_is_verbatim` is already a discovery floor on *kshell.rs*
    -- a renamed or truncated file fails every lookup. This is the floor on the
    other input, the tables themselves, which that check cannot see: an empty
    `CASES` has nothing to look up, so it passes, and then the scoring loop
    prints `0 rung assertion(s) disagree` over nothing at all.
    """
    if len(CASES) < MIN_CASES:
        raise SystemExit(
            f"only {len(CASES)} case(s) in CASES, below the floor of "
            f"{MIN_CASES}. Reporting '0 disagreements' over a table this thin "
            f"would be the failure this checker exists to prevent -- and note "
            f"that assert_rust_src_is_verbatim cannot catch it, because an "
            f"empty table has nothing to fail to find.")
    if len(AWK_CASES) < MIN_AWK_CASES:
        raise SystemExit(
            f"only {len(AWK_CASES)} case(s) in AWK_CASES, below the floor of "
            f"{MIN_AWK_CASES}. Rung 117 would be reported as agreeing with awk "
            f"without awk having been asked anything.")


def _fixture(src, cases):
    """Run leg (1) against a fixture tree and capture what it printed.

    The capture is not tidiness. `check_rungs_against_transcription` reports by
    printing `FAIL ...`, so a self-test that let those through would print the
    word FAIL during a *passing* run -- the same read-it-wrong ambiguity this
    file exists to remove, reintroduced in the place that proves it was removed.
    Captured, the diagnostics can be asserted on and then shown under a label
    that says they are the expected result.
    """
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        fails = check_rungs_against_transcription(src, cases, quiet=True)
    return fails, buf.getvalue()


def _selftest() -> int:
    """Drive the two guards against fixtures, never against the real tree.

    `kshell.rs` is lane A's file. A self-test that read it would be a lane-B
    test that lane A can turn red by editing its own code, which is precisely
    what lane A's rule says a self-test must never be able to do. So the
    verbatim check is driven through its injected `src`, and the floors through
    a temporarily shortened table.
    """
    checks = bad = 0

    def check(label, ok):
        nonlocal checks, bad
        checks += 1
        if ok:
            print(f"ok   {label}")
        else:
            print(f"selftest FAIL: {label}", file=sys.stderr)
            bad += 1

    fixture = [("remove_quotes", "\"a'b\"", "a'b", [b"a'b"])]

    # True negative: the literal is there, so nothing is reported.
    try:
        assert_rust_src_is_verbatim("let x = \"a'b\";\n", fixture)
    except SystemExit as exc:
        check(f"a literal that IS present passes ({exc})", False)
    else:
        check("a literal that is present passes", True)

    # True positive: it is not there, and the failure says which.
    try:
        assert_rust_src_is_verbatim("let x = \"nothing like it\";\n", fixture)
    except SystemExit as exc:
        check("a missing literal is reported", "do not occur in" in str(exc))
        check("...and the message names the literal, not just a count",
              "a'b" in str(exc))
    else:
        check("a missing literal is reported", False)

    # The specific historical fault this guard was added for: Python's `\'`
    # escaping leaking into a field that must be a verbatim copy of Rust, which
    # does not escape an apostrophe inside a string literal. The hint is the
    # whole value of the check -- without it the failure is thirteen lines of
    # backslashes and no diagnosis.
    try:
        assert_rust_src_is_verbatim(
            "let x = \"a'b\";\n",
            [("remove_quotes", "\"a\\'b\"", "a'b", [b"a'b"])])
    except SystemExit as exc:
        check("a spurious backslash before an apostrophe is diagnosed, "
              "not merely reported", "drop the backslash" in str(exc))
    else:
        check("a spurious backslash before an apostrophe is diagnosed", False)

    check(f"the {len(CASES)} real literals are all present in kshell.rs",
          assert_rust_src_is_verbatim() is None)

    # The floors, seen to fire in both directions.
    real_cases, real_awk = CASES, AWK_CASES
    for name, short in (("CASES", "CASES"), ("AWK_CASES", "AWK_CASES")):
        try:
            globals()[short] = globals()[short][:1]
            try:
                _assert_tables_are_not_gutted()
            except SystemExit as exc:
                check(f"a gutted {name} refuses to return a verdict",
                      "below the floor" in str(exc))
            else:
                check(f"a gutted {name} refuses to return a verdict", False)
        finally:
            globals()["CASES"], globals()["AWK_CASES"] = real_cases, real_awk
    check("...and the real tables pass the same guard",
          _assert_tables_are_not_gutted() is None)

    # --- leg (1), driven against a fixture tree -----------------------------
    #
    # This is the part that must be seen to REFUSE, not merely to pass. The
    # defect it exists for -- a rung asserting something bash never produces --
    # was invisible here for as long as this file existed, and "0 failures" is
    # what that invisibility looked like from outside. So each fixture below
    # plants the defect and the check must both fail AND say something a reader
    # can act on; a bare non-zero count would be the same silence with a
    # different number.
    clean_src = (
        'assert_eq!(remove_quotes("a b  c"), "a b  c");\n'
        'assert_eq!(split_words("a b  c"), alloc::vec!["a", "b", "c"], "runs");\n'
    )
    broken_src = clean_src.replace('"a", "b", "c"', '"a", "b", "", "c"')
    threeway_cases = [
        ("remove_quotes", '"a b  c"', "x", [b"a b  c"]),
        ("split_words", '"a b  c"', "x", [b"a", b"b", b"c"]),
    ]

    clean_fails, clean_out = _fixture(clean_src, threeway_cases)
    check("a tree whose rungs match the transcription passes", clean_fails == 0)
    check("...and says nothing while doing so", clean_out == "")

    broken_fails, broken_out = _fixture(broken_src, threeway_cases)
    check("a corrupted rung expectation is caught", broken_fails == 1)
    check("...and the failure names the rung and its line",
          "split_words" in broken_out and "kshell.rs:2" in broken_out)
    check("...and shows the blank word bash never produces",
          "b''" in broken_out)

    absent_fails, absent_out = _fixture(
        clean_src, [("split_words", '"not in the tree"', "x", [b"nope"])])
    check("a rung this file grades but the tree does not contain is caught",
          absent_fails == 1)
    check("...and says so in those terms",
          "the tree does not make" in absent_out)

    # The reader leg (1) rests on. Its own fixtures, so this stays cross-lane
    # safe; a silently broken reader would make every check above pass by
    # finding nothing, which is the failure mode this whole file is about.
    check("the rung reader agrees with its own fixtures",
          rustrungs.self_test() == 0)

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main():
    # Before bash is asked anything: the cases must be the ones the rung
    # actually contains, or every answer below is about a different string.
    _assert_tables_are_not_gutted()
    assert_rust_src_is_verbatim()
    src = KSHELL.read_text(encoding="utf-8", errors="surrogateescape")
    rung_fails = check_rungs_against_transcription(src, CASES, quiet=True)
    # All three checks above run before the transport check and none needs WSL:
    # a gutted table, a mistranscribed literal or a rung that no longer asserts
    # what this file says it asserts is worth reporting on a host that cannot
    # run the rest of this file, and each exits 1, which is a finding. Their
    # *success* lines are printed after it, because `run_checker --may-skip`
    # takes the checker's FIRST line of output as the reason it skipped -- and
    # "all 16 rust_src literals found verbatim" as the reason a gate did not
    # run reads like a pass, in the one place whose job is to say that nothing
    # was checked.
    if rung_fails:
        print(f"\n{rung_fails} rung(s) do not assert what this file says they "
              f"assert.\nBash was not asked anything: until the two agree, "
              f"asking it would be measuring\na case that is not in the tree.")
        return 1
    bashprobe.assert_transport_is_faithful()
    print(f"all {len(CASES)} rust_src literals found verbatim in kshell.rs")
    print(f"all {len(CASES)} rung expectations agree with the transcription "
          f"below")
    print("transport verified faithful\n")
    fails = 0
    for func, rust_src, line, want in CASES:
        got = bashprobe.words(line)
        ok = got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} rust {func}({rust_src}) = {line!r}")
        print(f"       bash={got!r}")
        if not ok:
            print(f"       rung={want!r}   <-- the rung is wrong")
    fails += check_awk()
    print(f"\n{fails} rung assertion(s) disagree with the reference tool")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:
        sys.exit(_selftest())
    sys.exit(main())
