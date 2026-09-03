#!/usr/bin/env python3
"""Ask real bash the questions kshell sites 4/5/6/7 have to answer.

`check-shellquote-vs-bash.py` validates the *scanner*; this validates the
*pipeline* it is wired into -- brace expansion, tilde, variable expansion,
quote removal and word splitting -- because those four kshell sites each
decide "is this byte active?" and each once decided it with a different
broken rule.

Transport and arity are handled by bashprobe (see its docstring for why
`bash -c <argv>` and `printf '%s\\n'` are both unsafe here, and why WSL is
required).  Note that this file must be *run as a file*: passing the same
Python inline through a shell here-doc silently eats backslashes, which turns
every backslash case into a test of a different input than the one written
down.

Every expectation is what I believe bash does.  The point of running it is
to find the ones where that belief is wrong -- a disagreement means my model
is wrong, not bash.
"""
import sys

import bashprobe

HOME = "/root"


def W(*words):
    return [w.encode() for w in words]


# (input, expected words, which kshell site the case pins down)
CASES = [
    # --- site 7: expand_vars_bytes.  The bug and its boundaries. -----------
    ('"it\'s $HOME"', W("it's " + HOME), "7: apostrophe in dquotes must not stop expansion"),
    ("'it\\'s'", None, "7: bash rejects; no legal apostrophe inside '...'"),
    ('"\\$HOME"', W("$HOME"), "7: escaped $ in dquotes -- ctx is Double, escaped stops it"),
    ("'\\$HOME'", W("\\$HOME"), "7: no escapes at all inside '...' -- backslash survives"),
    ("\\$HOME", W("$HOME"), "7: escaped $ unquoted"),
    ("$HOME", W(HOME), "7: plain"),
    ('"$HOME"', W(HOME), "7: dquoted expands"),
    ("'$HOME'", W("$HOME"), "7: squoted does not"),
    ('"a\\b$HOME"', W("a\\b" + HOME), "7: backslash before non-escapable is data in dquotes"),

    # --- site 5: remove_quotes.  What survives quote removal. -------------
    ('"C:\\dir"', W("C:\\dir"), "5: \\d is not escapable in dquotes -- backslash kept"),
    ('"say \\"hi\\""', W('say "hi"'), "5: escaped dquote"),
    ("a\\ b", W("a b"), "5: escaped space is one word, backslash removed"),
    ("a\\\\b", W("a\\b"), "5: unquoted \\\\ -> one backslash"),
    ("'a\\\\b'", W("a\\\\b"), "5: both backslashes survive inside '...'"),
    ('"a\\\\b"', W("a\\b"), "5: \\\\ IS escapable in dquotes -> one backslash"),
    ("a'b'c", W("abc"), "5: quotes vanish mid-word"),
    ('a"b"c', W("abc"), "5: same for dquotes"),
    ("'a'\\''b'", W("a'b"), "5: the '\\'' idiom quote_word emits"),

    # --- site 6: split_words.  Arity, including the empty word. ----------
    ("''", W(""), "6: explicitly quoted empty string IS a word"),
    ('""', W(""), "6: same for dquotes"),
    ("a '' b", W("a", "", "b"), "6: empty word in the middle keeps arity 3"),
    ('a"" b', W("a", "b"), "6: adjacent empty quote does not create a word"),
    ("a b  c", W("a", "b", "c"), "6: runs of blanks collapse"),
    ('"a b" c', W("a b", "c"), "6: quoted blank does not split"),
    ("$EMPTY", W(), "6: unquoted empty expansion yields NO word"),
    ('"$EMPTY"', W(""), "6: quoted empty expansion yields ONE empty word"),

    # --- site 4: expand_braces.  Quoting suppresses it. -------------------
    ("{a,b}", W("a", "b"), "4: plain brace expansion"),
    ('"{a,b}"', W("{a,b}"), "4: dquoted brace does NOT expand"),
    ("'{a,b}'", W("{a,b}"), "4: squoted brace does NOT expand"),
    ("\\{a,b}", W("{a,b}"), "4: escaped brace does NOT expand"),
    ("{a,'b,c'}", W("a", "b,c"), "4: quoted comma inside a brace is not a separator"),
    ('{a,"b c"}', W("a", "b c"), "4: and the quoted blank does not split either"),
    ("x{1,2}y", W("x1y", "x2y"), "4: prefix and suffix are distributed"),
    ("{a}", W("{a}"), "4: no comma -- not a brace expansion"),

    # --- tilde: same active/inactive question, third syntax. -------------
    ("~", W(HOME), "tilde: plain expands"),
    ('"~"', W("~"), "tilde: dquoted does NOT expand"),
    ("'~'", W("~"), "tilde: squoted does NOT expand"),
    ("\\~", W("~"), "tilde: escaped does NOT expand"),
    ("a~", W("a~"), "tilde: only at word start"),

    # --- ordering: brace runs BEFORE parameter expansion. ----------------
    ("{$HOME,x}", W(HOME, "x"), "order: brace first, then the parameter inside it"),
]


#: A floor on how much this file must still be pinning, not a target. See
#: `_assert_table_is_not_gutted`.
MIN_CASES = 20


def _assert_table_is_not_gutted() -> None:
    """Refuse to grade a table too thin to be the one this file was written on.

    An emptied or truncated `CASES` sails through the scoring loop and prints
    `0 disagreements with bash`, which is spelled exactly like a clean run. No
    fixture can catch that, because the fixture *is* the input that went
    missing -- so the assertion has to be on the real run. (Lane A's framing,
    `requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md`
    §2: a floor on discovery.)
    """
    if len(CASES) < MIN_CASES:
        raise SystemExit(
            f"only {len(CASES)} case(s) in CASES, below the floor of "
            f"{MIN_CASES}. Either this table has been gutted or a merge took "
            f"one side of a conflict; both want a human, and reporting "
            f"'0 disagreements with bash' over a table this thin would be the "
            f"failure this checker exists to prevent.")


def _selftest() -> int:
    """Check the table and the floor, without asking bash anything.

    The scoring loop is `bashprobe.score_cases` and is tested there against a
    stubbed bash. What is this file's own is the table. Deliberately WSL-free:
    this gate is wired `--may-skip` and declines on a host without WSL, so a
    self-test that needed WSL would be absent from exactly the runs where it
    was the only coverage left.
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

    problems = bashprobe.table_problems(CASES)
    check(f"the {len(CASES)} real cases are well-formed", not problems)
    for p in problems:
        print(f"       {p}", file=sys.stderr)

    check("the floor is below the real table, or the gate cannot pass",
          MIN_CASES <= len(CASES))
    real = CASES
    try:
        globals()["CASES"] = CASES[:1]
        try:
            _assert_table_is_not_gutted()
        except SystemExit as exc:
            check("a gutted table refuses to return a verdict",
                  "below the floor" in str(exc))
        else:
            check("a gutted table refuses to return a verdict", False)
    finally:
        globals()["CASES"] = real
    check("...and the real table passes the same guard",
          _assert_table_is_not_gutted() is None)

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main():
    # Before the transport check: neither this nor the table needs WSL, and a
    # gutted table is worth reporting on a host that cannot run the rest.
    _assert_table_is_not_gutted()
    bashprobe.assert_transport_is_faithful()
    print("transport verified faithful\n")
    fails = bashprobe.score_cases(CASES, width=22)
    print(f"\n{fails} disagreement(s) with bash over {len(CASES)} case(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:
        sys.exit(_selftest())
    sys.exit(main())
