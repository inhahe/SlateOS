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


def main():
    bashprobe.assert_transport_is_faithful()
    print("transport verified faithful\n")
    fails = 0
    for line, want, why in CASES:
        got = bashprobe.words(line)
        if want is None:
            ok = got is None
            shown = "<bash error>" if got is None else repr(got)
        else:
            ok = got == want
            shown = "<bash error>" if got is None else repr(got)
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r:26} -> {shown}")
        if not ok:
            print(f"       expected {want!r}")
            print(f"       ({why})")
    print(f"\n{fails} disagreement(s) with bash")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
