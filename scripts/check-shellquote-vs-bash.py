#!/usr/bin/env python3
"""Cross-check kernel/src/shellquote.rs's rules against real bash.

Ports the Rust scanner byte-for-byte, then asks bash what it actually does
with the same input.  A disagreement means the Rust is wrong, not bash.

Requires WSL; see `bashprobe.py` for why that keeps it out of the boot test.

**The port is the weak point, and is guarded.**  A hand copy of the scanner
can drift away from the Rust it claims to model, and a drifted copy reports
"0 disagreements" about a scanner that no longer exists -- worse than no
checker, because it looks like evidence.  `assert_port_matches_rust()` reads
`shellquote.rs` and refuses to run if the one table that is easy to get
silently wrong -- the set of characters a backslash may escape inside
double quotes -- differs from the set below.  It cannot prove the whole port
is faithful; it can and does stop the failure mode that actually happens,
which is a rule being changed in the Rust and not here.
"""
import contextlib
import io
import pathlib
import re
import sys

import bashprobe

UNQ, SGL, DBL = "U", "S", "D"
DQ_ESCAPABLE = set(b'"\\$`\n')

RUST = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "shellquote.rs"


def assert_port_matches_rust(src: str | None = None):
    """Refuse to run if the Rust's escape alphabet is not the one ported.

    `src` is injectable so the self-test can drive this against a fixture
    rather than against the real `shellquote.rs`. That is not a convenience:
    `shellquote.rs` is **lane A's file**, so a self-test that read it would be
    a lane-B test that lane A can turn red by editing its own code -- the one
    thing lane A's cross-lane rule says a self-test must never be able to do
    (`requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md`
    §4: a self-test reads only fixtures the checker carries in its own source).

    With `src=None` -- the real run -- it reads the real file, which is where
    the drift it exists to catch actually happens.
    """
    if src is None:
        try:
            src = RUST.read_text(encoding="utf-8")
        except OSError as e:
            raise SystemExit(f"cannot read {RUST}: {e}") from e
    m = re.search(r"const DQ_ESCAPABLE: \[u8; \d+\] = \[([^\]]*)\];", src)
    if not m:
        raise SystemExit(
            "DQ_ESCAPABLE was renamed or reshaped in shellquote.rs.\n"
            "  This checker's port can no longer be shown to match it, so its\n"
            "  verdict would be about a scanner that is not the one shipping."
        )
    # `b'"'`, `b'\\'`, `b'$'`, `b'`'`, `b'\n'` -> the bytes themselves.
    escapes = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", "'": "'"}
    theirs = set()
    for lit in re.findall(r"b'((?:\\.|[^'])+)'", m.group(1)):
        theirs.add(ord(escapes[lit[1]] if lit.startswith("\\") else lit))
    if theirs != DQ_ESCAPABLE:
        raise SystemExit(
            "PORT HAS DRIFTED -- every result below would be about the wrong "
            "scanner.\n"
            f"  shellquote.rs: {sorted(theirs)}\n"
            f"  this file    : {sorted(DQ_ESCAPABLE)}"
        )



def scan(bs: bytes):
    """Yield (off, byte, ctx, escaped, structural) -- the Rust `Tok`."""
    i = 0
    ctx = UNQ
    pending = False
    n = len(bs)
    while i < n:
        b = bs[i]
        off = i
        i += 1
        if pending:
            pending = False
            yield (off, b, ctx, True, False)
            continue
        if ctx == SGL:
            structural = b == ord("'")
            if structural:
                ctx = UNQ
            yield (off, b, SGL, False, structural)
        elif ctx == DBL:
            if b == ord("\\") and i < n and bs[i] in DQ_ESCAPABLE:
                pending = True
                yield (off, b, DBL, False, True)
            else:
                structural = b == ord('"')
                if structural:
                    ctx = UNQ
                yield (off, b, DBL, False, structural)
        else:
            if b == ord("\\") and i < n:
                pending = True
                yield (off, b, UNQ, False, True)
            elif b == ord("'"):
                ctx = SGL
                yield (off, b, SGL, False, True)
            elif b == ord('"'):
                ctx = DBL
                yield (off, b, DBL, False, True)
            else:
                yield (off, b, UNQ, False, False)


def is_bare(t):
    return t[2] == UNQ and not t[3] and not t[4]


def strip_quotes(bs):
    return bytes(t[1] for t in scan(bs) if not t[4])


def split_bare_words(bs):
    out, start, quoted = [], None, False
    for off, b, ctx, esc, st in scan(bs):
        if b in (32, 9) and is_bare((off, b, ctx, esc, st)):
            if start is not None:
                out.append((start, off, quoted))
                start = None
            quoted = False
        else:
            if start is None:
                start = off
            if st:
                quoted = True
    if start is not None:
        out.append((start, len(bs), quoted))
    return out


def find_bare(bs, needle):
    for t in scan(bs):
        if t[1] == needle and is_bare(t):
            return t[0]
    return None


def bash_words(line: bytes):
    """The exact word list bash produces for `line`.

    Delegated to bashprobe.  The original version of this function passed the
    script as an argv element to `bash -c` and read `printf '%s\\n'` output,
    and BOTH halves of that were wrong in ways that quietly weakened this
    file's verdict:

      * the argv round trip through Windows/wsl.exe ate backslashes, so every
        backslash case below was compared against a *different input* than
        the one written down -- in a file whose whole subject is backslashes;
      * `printf` reruns its format at least once, so zero words and one empty
        word both printed a single blank line.

    Both are why this file once reported 0 failures while the transport was
    silently mangling its own test data.  bashprobe delivers the bytes on
    stdin and counts words with `set --`/`$#`, and proves the transport is
    faithful before any case runs.
    """
    return bashprobe.words(line.decode("latin-1"), setup="")


def ours_words(line: bytes):
    return [strip_quotes(line[s:e]) for s, e, _q in split_bare_words(line)]


CASES = [
    # the two known-issues bugs
    b'"it\'s fine"',
    b'"don\'t.txt"',
    # backslash per context
    b"a\\ b",
    b"a\\'b",
    b"a\\>b",
    b'"C:\\dir"',
    b'"say \\"hi\\""',
    b"'it\\'",
    b"a\\\\",
    # quoting basics
    b"'a > b'",
    b"''",
    b"'' x",
    b'"" x',
    b"a'b'c",
    b'a"b"c',
    b"'a'\\''b'",
    # NOTE: no `$`-bearing cases here.  bash expands before quote removal and
    # this harness only does quote removal, so any `$` case compares apples to
    # oranges.  The `$` question -- "which context is it in?" -- is asked
    # separately below, which is the only part kshell's expander needs.
    b"a b  c",
    b"  lead",
    b'"a b" c',
    b"x'y z'w",
    b'"a\\tb"',
    b'"a\\\\b"',
    b"\\\\",
    # NOTE: `a\` -- a *trailing* backslash -- is deliberately not here. It is
    # not a scanner question at all, and kshell answers it differently from
    # bash on purpose. See DIVERGENCES below, which tests it properly.
]


# Cases where kshell is intentionally NOT bash, with both answers pinned.
#
# These must not sit in CASES, because there they are failures, and a failure
# that is expected trains the reader to ignore the count. They must not be
# deleted either: an intended divergence is exactly the thing that needs a
# test, since nothing else will notice when it stops being intended.
#
# (line, bash's words, kshell's words, why they differ)
DIVERGENCES = [
    (
        b"a\\",
        [b"a"],
        [b"a\\"],
        "A trailing backslash is a backslash-NEWLINE to bash, so bash splices\n"
        "     the next input line and the backslash disappears. kshell has no\n"
        "     continuation prompt, so the line really does end there and the\n"
        "     backslash is data. shellquote.rs:250 says exactly this.\n"
        "     WHEN THE CONTINUATION PROMPT LANDS (shellquote.rs:147 plans one)\n"
        "     this entry is the thing that should start failing -- at which\n"
        "     point the question moves to the line editor and `a\\` stops being\n"
        "     answerable by a scanner at all.",
    ),
]

# Delimiter visibility: the redirect bug, asked directly.
DELIM = [
    (b'echo "it\'s fine" > out', ord(">"), 17),
    (b"cat < \"don't.txt\"", ord("<"), 4),
    (b"echo 'a > b'", ord(">"), None),
    (b'echo "a" > b', ord(">"), 9),
    (b"echo a\\>b", ord(">"), None),
]


# Which context is a `$` in?  This is the whole of what kshell's expander needs
# from the scanner, and getting it wrong is bug #2 in known-issues.md:
# `echo "it's $HOME"` prints $HOME literally because the expander tracks only
# `'` and treats the apostrophe inside the double quotes as opening a region.
CTX = [
    (b'echo "it\'s $HOME"', DBL),   # expands  -- the bug
    (b"echo 'it \"is\" $HOME'", SGL),   # does not expand
    (b"echo $HOME", UNQ),           # expands
    (b'echo "\\$HOME"', DBL),       # inside "..."; `escaped` is what stops it
    (b"echo \\' $HOME", UNQ),       # `\'` must not flip quoting for the line
]


#: Floors on how much this file must still be pinning -- not targets. Set
#: below the real counts by enough that ordinary editing does not trip them,
#: and far enough above zero that a gutted table, or a merge that took one
#: side of a conflict, does. `DIVERGENCES` has exactly one entry and its floor
#: is therefore its size: a single intended divergence cannot be thinned, only
#: deleted, and deleting it is precisely the event worth refusing to grade.
MIN_CASES = 15
MIN_DIVERGENCES = 1
MIN_DELIM = 4
MIN_CTX = 4


def _assert_tables_are_not_gutted() -> None:
    """Refuse to grade tables too thin to be the ones this file was written on.

    An emptied or truncated table sails through the loops below and prints
    `0 failure(s)`, which is spelled exactly like a clean run. No fixture can
    catch that, because the fixture *is* the input that went missing -- so the
    assertion has to be on the real run. (Lane A's framing, in `requests/
    a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md` §2: a
    floor on discovery, not a target.)

    Raises rather than returning a verdict, deliberately: a breach means the
    question was not answered, not that the answer was no.
    """
    for name, table, floor in (
        ("CASES", CASES, MIN_CASES),
        ("DIVERGENCES", DIVERGENCES, MIN_DIVERGENCES),
        ("DELIM", DELIM, MIN_DELIM),
        ("CTX", CTX, MIN_CTX),
    ):
        if len(table) < floor:
            raise SystemExit(
                f"only {len(table)} case(s) in {name}, below the floor of "
                f"{floor}. Either this table has been gutted or a merge took "
                f"one side of a conflict; both want a human, and reporting "
                f"'0 failure(s)' over a table this thin would be the failure "
                f"this checker exists to prevent.")


def score_cases(cases) -> int:
    """Compare our port's word split to bash's, for each case. Needs bash."""
    fails = 0
    for line in cases:
        theirs = bash_words(line)
        if theirs is None:
            # A skip is not a failure, so this branch is how a case disappears
            # from the verdict without disturbing the "0 failures" line. Every
            # case in CASES is one bash accepts -- that is a property of the
            # list, checked here rather than assumed -- so reaching this is a
            # defect in the list or in the probe, and either way the count
            # below would be a lie about how much was tested. Counting it is
            # the whole fix: `a\` sat in this branch printing SKIP for as long
            # as the file existed.
            print(f"FAIL (bash rejected a case that should parse): {line!r}")
            fails += 1
            continue
        mine = ours_words(line)
        ok = mine == theirs
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r}\n      bash={theirs!r}\n      ours={mine!r}")
    return fails


def score_divergences(divergences) -> int:
    """Check both pinned sides of each intended divergence. Needs bash."""
    fails = 0
    print("\n--- intended divergences from bash (BOTH sides pinned) ---")
    for line, want_bash, want_ours, why in divergences:
        got_bash = bash_words(line)
        got_ours = ours_words(line)
        ok_bash = got_bash == want_bash
        ok_ours = got_ours == want_ours
        ok = ok_bash and ok_ours
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r}")
        print(f"      bash={got_bash!r} (want {want_bash!r}){'' if ok_bash else '  <-- CHANGED'}")
        print(f"      ours={got_ours!r} (want {want_ours!r}){'' if ok_ours else '  <-- CHANGED'}")
        if not ok:
            print(f"     {why}")
    return fails


def score_ours_half(divergences) -> int:
    """Check only the *our-side* half of each divergence. Needs no bash.

    Split out from `score_divergences` so the self-test can assert the half
    that is ours -- what the ported scanner does with `a\\` -- on a host that
    is not being asked to run bash at all. The bash half stays where it is:
    it is a claim about bash, and only bash can answer it.
    """
    fails = 0
    for line, _want_bash, want_ours, why in divergences:
        got = ours_words(line)
        ok = got == want_ours
        if not ok:
            fails += 1
            print(f"FAIL {line!r} ours={got!r} (want {want_ours!r})\n     {why}")
    return fails


def score_delim(delim) -> int:
    """Where is the first *bare* delimiter? Needs no bash."""
    fails = 0
    print("\n--- bare-delimiter offsets ---")
    for line, needle, want in delim:
        got = find_bare(line, needle)
        ok = got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r} {chr(needle)} -> {got} (want {want})")
    return fails


def score_ctx(ctx) -> int:
    """Which quoting context is the `$` in? Needs no bash."""
    fails = 0
    print("\n--- context of the `$` ---")
    for line, want in ctx:
        toks = [t for t in scan(line) if t[1] == ord("$")]
        got = toks[0][2] if toks else None
        esc = toks[0][3] if toks else None
        # For the escaped case the `$` is data, which the expander sees as
        # "escaped" rather than "quoted"; the context is unchanged either way.
        ok = got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r} -> ctx={got} escaped={esc} (want {want})")
    return fails


def _selftest() -> int:
    """Exercise everything in this file that does not need bash.

    That is most of it, and this is the one of the four bash oracles where
    that is true: `scan` and its four consumers are a *port*, not a probe, so
    `DELIM`, `CTX` and the our-side half of `DIVERGENCES` are answerable here
    with no WSL and no subprocess. The gate is wired `--may-skip` and declines
    on a host without WSL, so a self-test that needed WSL would be absent from
    exactly the runs where it was the only coverage left -- and here it would
    be absent while being *able* to answer three of the four tables.

    `assert_port_matches_rust` is driven through its injected `src` and never
    against the real `shellquote.rs`, which is lane A's file: see that
    function's docstring.
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

    def quietly(fn, *a):
        """Run a scorer with its output captured, and hand back both."""
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = fn(*a)
        return rc, buf.getvalue()

    # --- the port-drift guard, in both directions. ------------------------
    same = "const DQ_ESCAPABLE: [u8; 5] = [b'\"', b'\\\\', b'$', b'`', b'\\n'];"
    try:
        assert_port_matches_rust(same)
    except SystemExit as exc:
        check(f"a matching Rust table passes ({exc})", False)
    else:
        check("a matching Rust table passes", True)

    # The drift this exists for: a rule changed in the Rust and not here.
    # `\n` dropped from the alphabet is the realistic shape of it, because it
    # is the member a reader is least likely to remember is there.
    drifted = "const DQ_ESCAPABLE: [u8; 4] = [b'\"', b'\\\\', b'$', b'`'];"
    try:
        assert_port_matches_rust(drifted)
    except SystemExit as exc:
        check("a drifted Rust table is refused", "PORT HAS DRIFTED" in str(exc))
        check("...and the refusal prints both sides, not just a verdict",
              "shellquote.rs:" in str(exc) and "this file" in str(exc))
    else:
        check("a drifted Rust table is refused", False)

    # The other way the guard can go blind: the constant is renamed or
    # reshaped, the regex matches nothing, and a silent `theirs == set()`
    # would compare empty against empty if the code were written carelessly.
    try:
        assert_port_matches_rust("const DQ_ESC: [u8; 0] = [];")
    except SystemExit as exc:
        check("a renamed constant is refused rather than read as empty",
              "renamed or reshaped" in str(exc))
    else:
        check("a renamed constant is refused rather than read as empty", False)

    # --- the bash-free tables, on the real data. --------------------------
    rc, _ = quietly(score_delim, DELIM)
    check(f"the {len(DELIM)} real DELIM cases agree with the port", rc == 0)
    rc, _ = quietly(score_ctx, CTX)
    check(f"the {len(CTX)} real CTX cases agree with the port", rc == 0)
    rc, _ = quietly(score_ours_half, DIVERGENCES)
    check(f"our side of the {len(DIVERGENCES)} divergence(s) is as pinned",
          rc == 0)

    # ...and that those three would actually notice. A scorer that returns 0
    # unconditionally passes all three checks above, which is why each needs a
    # planted wrong answer to prove the comparison is real.
    rc, text = quietly(score_delim, [(b"echo a > b", ord(">"), 999)])
    check("a wrong DELIM offset is counted", rc == 1)
    check("...and named in the output", "FAIL" in text and "999" in text)
    rc, text = quietly(score_ctx, [(b"echo '$HOME'", DBL)])
    check("a wrong CTX answer is counted", rc == 1)
    check("...and shows what the port actually said",
          f"ctx={SGL}" in text)
    rc, _ = quietly(score_ours_half, [(b"a\\", [b"a"], [b"WRONG"], "fixture")])
    check("a wrong our-side divergence is counted", rc == 1)

    # --- the scanner itself, on the cases whose answer needs no bash. -----
    # `CASES` is graded against bash and so cannot be checked here, but three
    # of its entries have answers that are not in dispute, and they are the
    # three the ported scanner is most likely to get wrong.
    check("quote removal drops the quotes and keeps the apostrophe",
          ours_words(b'"it\'s fine"') == [b"it's fine"])
    check("a backslash-escaped blank does not split the word",
          ours_words(b"a\\ b") == [b"a b"])
    check("the '\\'' idiom round-trips to one word with an apostrophe",
          ours_words(b"'a'\\''b'") == [b"a'b"])
    check("an empty quoted string is still a word",
          ours_words(b"''") == [b""])

    # --- the escape alphabet is consulted, not merely declared. -----------
    # `assert_port_matches_rust` proves DQ_ESCAPABLE equals the Rust's table.
    # Nothing above proves `scan` ever *reads* it: a scanner that treated every
    # byte after a backslash as escaped inside "..." passes that guard with the
    # table perfectly intact, and passed every other case here too. That the
    # one table this file goes to the trouble of pinning against the Rust was
    # also the one whose *use* went unchecked is the whole reason to state it
    # separately -- a guarded constant that nothing consults is decoration.
    def dq_backslash_escapes(nxt: int) -> bool:
        """Is the `\\` at offset 2 of `"a\\<nxt>b"` an escape, or data?"""
        return next(t for t in scan(b'"a\\' + bytes([nxt]) + b'b"')
                    if t[0] == 2)[4]

    check(f"inside \"...\" a backslash escapes each of the {len(DQ_ESCAPABLE)} "
          "alphabet members",
          all(dq_backslash_escapes(b) for b in sorted(DQ_ESCAPABLE)))
    outside = [b for b in b"tnzd0 e" if b not in DQ_ESCAPABLE]
    check("...and escapes nothing outside it -- the backslash stays data",
          not any(dq_backslash_escapes(b) for b in outside))
    # The same rule seen through the consumer, on two cases bash has graded:
    # `\t` is not escapable so both bytes survive, `\\` is so one is removed.
    check(r'"a\tb" keeps the backslash (\t is not in the alphabet)',
          ours_words(b'"a\\tb"') == [b"a\\tb"])
    check(r'"a\\b" collapses to one (\\ is in the alphabet)',
          ours_words(b'"a\\\\b"') == [b"a\\b"])

    # --- the floors, seen to fire on each table in turn. ------------------
    real = {n: globals()[n] for n in ("CASES", "DIVERGENCES", "DELIM", "CTX")}
    for name in real:
        try:
            globals()[name] = []
            try:
                _assert_tables_are_not_gutted()
            except SystemExit as exc:
                check(f"a gutted {name} refuses to return a verdict",
                      "below the floor" in str(exc) and name in str(exc))
            else:
                check(f"a gutted {name} refuses to return a verdict", False)
        finally:
            globals().update(real)
    check("...and the real tables pass the same guard",
          _assert_tables_are_not_gutted() is None)

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main():
    # Before bash is asked anything, and before the port check, because
    # neither needs WSL and a gutted table is worth reporting on a host that
    # cannot run the rest of this file at all.
    _assert_tables_are_not_gutted()
    assert_port_matches_rust()
    # Both checks above run before the transport check and neither needs WSL:
    # a gutted table or a drifted port is worth reporting on a host that cannot
    # run the rest of this file at all, and both exit 1, which is a finding.
    #
    # Their *success* lines, though, are printed after it. `run_checker
    # --may-skip` takes the checker's FIRST line of output as the reason it
    # skipped, so anything printed before the decline becomes the explanation
    # in the transcript -- and "port verified against shellquote.rs" as the
    # reason a gate did not run reads like a pass, in the one place whose job
    # is to say that nothing was checked. Today the ordering survives by
    # accident (stdout block-buffers into the log and stderr does not), which
    # is exactly the kind of accident that ends when someone adds `-u`.
    bashprobe.assert_transport_is_faithful()
    print("port verified against shellquote.rs")
    print("transport verified faithful\n")

    fails = score_cases(CASES)
    fails += score_divergences(DIVERGENCES)
    fails += score_delim(DELIM)
    fails += score_ctx(CTX)
    print(f"\n{fails} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:
        sys.exit(_selftest())
    sys.exit(main())
