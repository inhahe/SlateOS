#!/usr/bin/env python3
"""One Rust lexer for the checkers that need to ignore comments and strings.

## Why this is a module and not a copy

Twelve scripts under `scripts/` defined their own version of this on
2026-09-10. Only one handled raw strings, so `r"a\\"` -- which ends at that
quote, because a backslash is not an escape inside a raw string -- swallowed
its terminator in three of them and paired every later quote one off.

The function has been wrong at least three separate times across its copies:

  * a char literal holding a quote. `rest.find('"')` appears in thirty-odd
    crates; without the char branch it opened a string that ran to the next
    quote anywhere later in the file.
  * a raw string ending in a backslash, as above.
  * blanking string BODIES rather than traversing them, which took a real
    count of 37 to 0 in lane A's `check-absent-operand-default` while its gate
    stayed green -- the pattern it measures IS a string literal.

Every one of those failed toward SILENCE: the scan still produced a plausible
list, still passed `--check`, and nothing in the output said the input had been
misread. That is the argument for one implementation with fixtures rather than
twelve without.

## The contract

`strip_noise(src)` returns a string of **exactly the same length** as `src`,
with comments, string literals, char literals and raw strings replaced by
spaces and newlines preserved. Length is load-bearing: callers match against
the returned text and then index the ORIGINAL at the same offsets to show real
source, which is how `check-read-defaults` reports the argument a call was
written with rather than the blanked version.

`keep_literals=True` blanks comments only and leaves strings, char literals
and raw strings intact. That is a different job with a real caller:
`scripts/host-errmsg.py` searches for message text INSIDE string literals, so
blanking them would take its count to zero with its gate green -- which is
exactly how lane A's `mask_noncode` lost 37 findings. A masker that cannot be
asked to spare literals invites its callers to write their own.

It DOES understand nested block comments (`/* /* */ */`), which Rust
allows and which nothing in this tree uses. If that changes, this is the place
to fix it once.
"""

from __future__ import annotations

import re


# A complete char literal: `'x'`, `'\n'`, `'\''`, `'\u{1F600}'`. Deliberately
# NOT a lifetime -- `'a` has no closing quote and must pass through untouched.
_CHAR_LITERAL = re.compile(r"'(?:\\u\{[0-9a-fA-F]{1,6}\}|\\.|[^\\'\n])'")

# A raw-string opener: `r"`, `r#"`, `br##"`. Backslash is NOT an escape inside
# one, so `r"a\"` ends at that quote -- treating it as an escape swallows the
# terminator and pairs every later quote one off. `b"` is deliberately absent:
# a byte string is escaped like an ordinary one.
_RAW_OPEN = re.compile(r'b?r(#*)"')


def strip_noise(src: str, keep_literals: bool = False) -> str:
    """Blank comments and string literals, preserving length and newlines.

    Without this the scan matches its own documentation: the four fixes above
    each carry a doc comment quoting the line they replaced, and an earlier
    version of this query counted all of them as live code.

    # The char-literal case, which the first version got wrong

    `rest.find('"')` is a char literal holding a double quote, and it appears
    in more than thirty crates here. Without the `'` branch below, that `"`
    opened a string that ran to the next `"` anywhere later in the file --
    after which every quote was paired one off, so real code was blanked as
    string and string contents were left as code.

    It failed silently and in the direction that looks fine: the scan still
    produced a plausible list. It was caught because a survey of `dbus` matched
    the word "simulated" INSIDE a println! whose text should have been blanked,
    and the blanked line showed the string contents surviving while the
    delimiters had gone.

    Lifetimes are not char literals -- `'a` and `'static` must pass through --
    so the branch matches only a complete `'x'`, `'\\n'` or `'\\u{1F600}'`.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                out[i] = " "
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            # Rust block comments NEST, so a depth counter rather than a scan for
            # the first `*/`. Without this, `/* outer /* inner */ still out */`
            # ended at the inner close and the tail was read as code.
            depth = 0
            while i < n:
                if src.startswith("/*", i):
                    depth += 1
                    for k in range(i, min(i + 2, n)):
                        out[k] = " "
                    i += 2
                    continue
                if src.startswith("*/", i):
                    depth -= 1
                    for k in range(i, min(i + 2, n)):
                        out[k] = " "
                    i += 2
                    if depth == 0:
                        break
                    continue
                if src[i] != "\n":
                    out[i] = " "
                i += 1
        elif (c in "rb") and (_m := _RAW_OPEN.match(src, i)):
            close = '"' + "#" * len(_m.group(1))
            end = src.find(close, _m.end())
            end = n if end < 0 else end + len(close)
            if not keep_literals:
                for k in range(i, end):
                    if src[k] != "\n":
                        out[k] = " "
            i = end
        elif c == "'":
            # A CHAR LITERAL, not a lifetime. `'a` / `'static` fall through to
            # the catch-all below and are left alone; only a complete literal
            # is blanked, so `find('\"')` cannot open a string.
            m = _CHAR_LITERAL.match(src, i)
            if m:
                if not keep_literals:
                    for k in range(i, m.end()):
                        out[k] = " "
                i = m.end()
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            if not keep_literals:
                for k in range(i, min(j, n)):
                    if src[k] != "\n":
                        out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


# ---------------------------------------------------------------------------
# Moved here from `check-read-defaults.py` on 2026-09-11, because a SECOND
# checker needed it and had written the naive version instead.
#
# `multicall-aliases.py` cut its view of a file at the first `#[cfg(test)]`
# with `text[: m.start()]`. On `userspace/last` that is a `#[cfg(test)] use`
# for the fixture builder's offsets, at line 39 of 2,025 -- so the gate saw 38
# lines, missed the dispatch at 1,233, and reported `last:lastb` and
# `last:lastlog` as NO LONGER PRESENT. `--update-baseline` would have recorded
# that as two aliases fixed. Nothing was fixed; the scanner had gone blind, and
# the ledger cannot tell those apart.
#
# That is the same defect check-read-defaults had and documents below, found
# again in another file eight hours later, and triggered by an edit of mine
# that added an ordinary `#[cfg(test)] use`. One implementation now.
# ---------------------------------------------------------------------------


def live_code(src: str) -> tuple[str, str]:
    """`src` with test code removed and nothing else.

    # What this replaced

    `src.split("#[cfg(test)]")[0]` -- "everything before the tests", which is
    only true when the FIRST such attribute is the test module. A
    `#[cfg(test)]` on a single helper is ordinary, and everything after it was
    discarded along with the tests.

    Measured before the fix: **35,706 lines across `userspace/`, 7% of the
    lane, invisible to this checker.** `fdisk/src/main.rs` was read as 21 lines
    of 3,818; `coreutils/src/bin/tar.rs` as 592 of 5,635.

    The floors did not catch it because they are aggregate. Losing 7% of the
    corpus leaves 398 live `read_to_string` calls against a floor of 120, and
    every file was still opened so the file count never moved. **A floor on the
    total cannot see a hole in the distribution** -- which is worth remembering
    before trusting one anywhere else.

    # How it works

    Blank each `#[cfg(test)]` item by matching its braces, then cut at the test
    module. Blanking preserves length, so match offsets still index the
    original, which is what lets `survey` show real argument text.

    Brace matching runs over the `strip_noise` output: a brace inside a string
    or a comment must not close an item early, and this file has been wrong
    about string boundaries twice already.
    """
    masked = strip_noise(src)
    # Both views are blanked at the same offsets and returned together, so
    # `survey` does not re-run `strip_noise` on the result. Running it twice
    # per file doubled the honour-head suite's wall clock past ten minutes,
    # which is a timeout rather than a slowdown.
    out = list(src)
    mout = list(masked)
    i = 0
    while True:
        i = masked.find("#[cfg(test)]", i)
        if i < 0:
            break
        # The test module ends the live region; everything after it goes.
        rest = masked[i + len("#[cfg(test)]"):]
        head = rest.lstrip()
        # Skip any further attributes (`#[allow(...)]` is usual on test mods).
        while head.startswith("#["):
            close = head.find("]")
            if close < 0:
                break
            head = head[close + 1:].lstrip()
        if head.startswith("mod "):
            return "".join(out[:i]), "".join(mout[:i])
        # A single item: blank it from the attribute to its closing brace.
        brace = masked.find("{", i)
        if brace < 0:
            return "".join(out[:i]), "".join(mout[:i])
        depth = 0
        j = brace
        while j < len(masked):
            if masked[j] == "{":
                depth += 1
            elif masked[j] == "}":
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            j += 1
        for k in range(i, min(j, len(out))):
            if out[k] != "\n":
                out[k] = " "
                mout[k] = " "
        i = j
    return "".join(out), "".join(mout)


def _self_test() -> int:
    """Fixtures for every way this has been wrong.

    Each pins a case that shipped. They are here rather than in a caller
    because the function is here: a caller's suite proves the caller reads the
    lexer correctly, not that the lexer is correct.
    """
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got  {got!r}")
            print(f"          want {want!r}")

    # THE SHIPPED BUG. `find('"')` is a char literal holding a double quote and
    # appears in more than thirty crates. Without the char branch it opened a
    # string that ran to the next quote anywhere later in the file, after which
    # code was blanked as string and string contents stood as code.
    src = 'let end = rest.find(\'"\')?;\nlet x = "hidden";\n'
    out = strip_noise(src)
    expect("a char literal holding a quote does not open a string",
           "rest.find(" in out, True)
    expect("...and the string after it is still blanked",
           "hidden" in out, False)

    # Lifetimes are not char literals and must survive: they have no closing
    # quote, so blanking on sight would eat the rest of the line.
    expect("a lifetime is left alone",
           "'a" in strip_noise("fn f<'a>(s: &'a str) {}"), True)

    # A raw string does not process escapes, so `r"a\"` ends at that quote.
    # Treating the backslash as an escape swallows the terminator.
    out = strip_noise('let re = r"a\\";\nlet y = "seen";\n')
    expect("a raw string ending in a backslash terminates there",
           "seen" in out, False)
    expect("...and the code after it survives",
           "let y =" in out, True)

    # Hashed raw strings close only on the matching hash count.
    out = strip_noise('let a = r#"x"y"#;\nlet b = "gone";\n')
    expect("a hashed raw string spans an inner quote",
           "let b =" in out, True)
    expect("...and the following string is blanked",
           "gone" in out, False)

    # An ordinary string DOES process escapes.
    out = strip_noise('let a = "x\\"y";\nlet b = 1;\n')
    expect("an escaped quote does not end an ordinary string",
           "let b = 1" in out, True)

    expect("a line comment goes",
           "note" in strip_noise("let a = 1; // note\n"), False)
    expect("a block comment goes",
           "note" in strip_noise("let a = /* note */ 1;"), False)

    # Length is preserved so match offsets index the original -- the property
    # `survey` relies on to show real argument text in the baseline.
    for probe in ["let a = \"x\";", "// c\n", "r#\"q\"#", "'\\n'"]:
        expect(f"length preserved: {probe!r}",
               len(strip_noise(probe)), len(probe))

    print(f"rustlex: self-test {'FAILED' if failures else 'passed'} "
          f"({failures} failure(s))")
    return 1 if failures else 0


if __name__ == "__main__":
    import sys
    sys.exit(_self_test())
