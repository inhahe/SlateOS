#!/usr/bin/env python3
r"""Pin bash's `$'...'` (ANSI-C quoting) rules, for TD-SHELLQUOTE-NO-ANSI-C-QUOTING.

The shared scanner in `kernel/src/shellquote.rs` models three contexts --
`Unquoted`, `Single`, `Double` -- and has no notion of `$'...'`, which is a
fourth with an escape alphabet unlike either quoted one.  kshell therefore
reads `$'a\tb'` as the four literal characters `$`, `a`, `\`, `t`... rather
than as `a<TAB>b`.  This file measures what the fourth context has to do
before any of it is written in Rust, on the same principle as the other four
checkers here: the rules are bash's, not ours to invent.  Requires WSL; see
`bashprobe.py`.

The headline results, which are what the Rust has to reproduce:

  * The alphabet is C's, not the shell's: `\n \t \r \a \b \f \v \e \\ \' \"`,
    plus `\nnn` octal, `\xHH` hex, `\cX` control, and `\uHHHH`/`\UHHHHHHHH`
    which emit **UTF-8**, not a code unit.
  * `\'` works *inside* the quotes.  This is the one thing `'...'` cannot do
    at all, and it is the main reason the construct exists.
  * An unrecognised escape keeps **both** bytes (`\z` -> `\z`), so this is not
    "drop the backslash" like the unquoted context.
  * `\0` truncates the word -- a bash string is NUL-terminated.  (Which is
    also why NUL is a safe separator for `bashprobe.words`.)
  * **Nothing expands**: `$`, backticks, `~`, `{a,b}` and blanks are all
    inert, exactly as in `'...'`.  Only the escapes are special.
  * It is a *word* construct, not a line one: `x$'a\tb'y` is one word, and
    `$''` is an empty word rather than no word.
  * The `$` must be bare.  Inside `"..."`, or escaped, or quoted off, the
    construct does not exist and the `$` is literal.

Deliberately absent: the interaction with a `$'...'` spanning a newline,
which is a continuation-prompt question rather than a scanner one.
"""
import sys

import bashprobe


def W(*words):
    return [w.encode() if isinstance(w, str) else w for w in words]


# (input, expected words or None for a bash error, what it pins down)
CASES = [
    # --- The C escape alphabet. ------------------------------------------
    (r"$'a\nb'", W("a\nb"), "newline -- a word may contain one"),
    (r"$'a\tb'", W("a\tb"), "tab"),
    (r"$'a\rb'", W("a\rb"), "carriage return"),
    (r"$'\a\b\f\v'", W("\a\b\f\v"), "bell, backspace, form feed, vertical tab"),
    (r"$'\e'", W("\x1b"), "escape -- a bash extension, not in C"),
    (r"$'a\\b'", W("a\\b"), r"\\ is one backslash"),
    (r"$'a\'b'", W("a'b"), r"\' works INSIDE the quotes -- '...' cannot do this"),
    (r"$'a\"b'", W('a"b'), r"\" is allowed though the quote needs no escaping"),

    # --- Numeric and control escapes. ------------------------------------
    (r"$'\x41'", W("A"), "hex"),
    (r"$'\101'", W("A"), "octal -- no leading 0 needed"),
    (r"$'\cA'", W("\x01"), "control-X"),
    (r"$'\u00e9'", W("\u00e9"), "4-digit unicode, emitted as UTF-8 (2 bytes)"),
    (r"$'\U0001F600'", W("\U0001F600"), "8-digit unicode, emitted as UTF-8 (4 bytes)"),
    (r"$'\0'", W(""), "NUL truncates -- a bash string is NUL-terminated"),
    (r"$'a\0b'", W("a"), "and it truncates mid-word, discarding the rest"),

    # --- The unrecognised escape: BOTH bytes survive. --------------------
    # This is the rule most easily got wrong, because the unquoted context
    # does the opposite (there, `\z` is `z`).
    (r"$'\z'", W("\\z"), r"unknown escape keeps the backslash too"),
    (r"$'\8'", W("\\8"), "8 is not an octal digit, so this is unknown as well"),

    # --- Nothing expands.  Only escapes are special. ---------------------
    (r"$'$HOME'", W("$HOME"), "no parameter expansion"),
    (r"$'`echo hi`'", W("`echo hi`"), "no command substitution"),
    (r"$'a b'", W("a b"), "no word splitting"),
    (r"$'{a,b}'", W("{a,b}"), "no brace expansion"),
    (r"$'~'", W("~"), "no tilde expansion"),

    # --- It is a word construct. -----------------------------------------
    (r"x$'a\tb'y", W("xa\tby"), "concatenates with adjacent bare text"),
    (r"$'a'$'b'", W("ab"), "and with another $'...'"),
    (r"$''", W(""), "the empty one IS a word, like '' is"),
    (r"$'a\nb' c", W("a\nb", "c"), "an embedded newline does not end the word"),

    # --- The `$` has to be bare for any of this to happen. ---------------
    (r'''"$'a\nb'"''', W(r"$'a\nb'"), "inside \"...\" it is not ANSI-C at all"),
    (r"\$'a\nb'", W(r"$a\nb"), r"an escaped $ leaves a plain '...' after it"),
    (r"""'$'"'"'a\nb'""", W(r"$'a\nb"), "a quoted-off $ is literal"),

    # --- Error. ----------------------------------------------------------
    (r"$'unterminated", None, "an unclosed $'... is a syntax error"),
]


#: A floor on how much this file must still be pinning, not a target. Set
#: below the real count (see CASES) by enough that ordinary editing does not
#: trip it, and far enough above zero that a table someone gutted, or a bad
#: merge that took one side of a conflict, does.
MIN_CASES = 20


def _selftest() -> int:
    """Check what this file owns, without asking bash anything.

    The scoring loop belongs to `bashprobe.score_cases` and is tested there
    against a stubbed bash; running it again here would test the same code
    twice and this file's own contribution not at all. What is this file's own
    is the *table*, and the floor that keeps an emptied table from reading as
    a clean run.

    Neither needs WSL, which is the whole reason the split is worth making:
    this gate is wired with `--may-skip` and will decline on a host without
    WSL, so a self-test that needed WSL would be skipped in precisely the runs
    where the gate itself was skipped -- covering nothing, on every machine
    where the coverage was the only thing left.
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

    # The floor, seen to fire. A floor that has never fired is a guess about a
    # number, and this one guards the case where the gate reports success over
    # a table that is no longer there.
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


def _assert_table_is_not_gutted() -> None:
    """Refuse to grade a table too thin to be the one this file was written on.

    An emptied or truncated `CASES` sails through the scoring loop and prints
    `0 disagreements with bash`, which is spelled exactly like a clean run.
    No fixture can catch that, because the fixture is precisely the input that
    went missing -- so the assertion has to be on the *real* run. (Lane A's
    framing, in `requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-
    not-cover.md` §2: a floor on discovery, not a target.)

    Raises rather than returning a verdict, deliberately: a breach means the
    question was not answered, not that the answer was no.
    """
    if len(CASES) < MIN_CASES:
        raise SystemExit(
            f"only {len(CASES)} case(s) in CASES, below the floor of "
            f"{MIN_CASES}. Either this table has been gutted or a merge took "
            f"one side of a conflict; both want a human, and reporting "
            f"'0 disagreements with bash' over a table this thin would be the "
            f"failure this checker exists to prevent.")


def main():
    # Before bash is asked anything, and before the transport check, because
    # neither needs WSL and a gutted table is worth reporting on a host that
    # cannot run the rest of this file at all.
    _assert_table_is_not_gutted()
    bashprobe.assert_transport_is_faithful()
    print("transport verified faithful\n")
    fails = bashprobe.score_cases(CASES)
    print(f"\n{fails} disagreement(s) with bash over {len(CASES)} case(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:
        # The scoring loop is bashprobe's and is tested there, against a
        # stubbed bash; what is this file's own is the table. Checking it
        # needs no WSL, which is the point -- these four gates run on a host
        # that has bash, and their self-tests must run on one that does not.
        sys.exit(_selftest())
    sys.exit(main())
