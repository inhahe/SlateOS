#!/usr/bin/env python3
"""Read Rust source the way a *gate* has to read it: production code only.

Every check in this directory that asks "does this file really do X?" hits the
same three traps, and every one of them turns a gate into a decoration:

1.  **A comment that mentions X.** Each finding a gate causes to be fixed
    leaves behind a comment explaining the fix -- naming the very construct the
    gate searches for. A check that reads raw text therefore goes blind on
    exactly the files it has already helped, which is the worst possible place
    to go blind.
2.  **A test that exercises X.** Same shape, worse: the regression test written
    to catch a relapse is itself what makes the relapse invisible.
3.  **A brace inside a string or char literal.** Any check that matches braces
    to find the end of an item will swallow the rest of the file the first time
    it meets `'{'` or `"unclosed {"`.

`production_only` closes all three. It is the difference between
`scripts/check-tick-wiring.py` naming `apps/stopwatch` and saying nothing at
all when the match arm was deleted again on the live tree -- measured, not
assumed.

The functions here were written for `check-tick-wiring.py` and extracted when
`check-window-wiring.py` needed the same three defences. Two copies of a
600-line scanner are two copies that drift, and the one that drifts is always
the one nobody is currently editing.

## Cost

The shapes here are the fast ones, and the slow ones are documented at their
sites because they were each measured on this tree and each cost minutes:

* `INDENT` is `[ \\t]*`, never `\\s*` -- see the constant.
* Blanking is batched into a single pass ([`blank_ranges`]), because a Python
  string is immutable and blanking one range at a time is quadratic in the
  number of ranges.
* [`strip_cfg_test`] carries a cursor rather than restarting its search after
  each blank.

Together those took the tick gate from 6m24s to under four seconds.

## What it deliberately does not do

This is not a Rust parser and must not grow into one. It blanks text to spaces
while preserving every newline, so an offset into the result is still the
offset it was in the file and a reported line number is a line the reader can
go and look at. Anything that needs real name resolution wants `syn`, not this.
"""

from __future__ import annotations

import re

import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))
import rustlex  # noqa: E402
import sys

# The indentation before a `fn`, in callers' regexes.
#
# `[ \t]*` and not `\s*`, for two independent reasons, both of which bite hard.
#
# Speed: `\s` matches a newline, so `^\s*` at a blank line runs on through every
# following blank line and every following indentation, then gives the whole run
# back one character at a time, retrying `pub`/`fn` at each step. That is O(w^2)
# in the length of a whitespace run -- and these scans run over text that
# [`strip_comments`] and [`strip_cfg_test`] have blanked *to spaces*, so a file
# whose `#[cfg(test)] mod tests` is a third of its bulk presents one whitespace
# run a quarter of a megabyte long. Measured on gui/compositor/src/lib.rs
# (733 KB): 93s to find 243 `fn`s with `\s*`, 0.06s with `[ \t]*`.
#
# Correctness: with `\s*` a match could *start* on an earlier blank line, and a
# gate that reports `text.count("\n", 0, m.start())` as the line number would
# then point the reader at the blank line rather than at the `fn`.
INDENT = r"^[ \t]*"

CFG_TEST_RE = re.compile(r"#\[cfg\((?P<args>[^\]]*)\)\]")

RAW_STRING_RE = re.compile(r"r(?P<hashes>#*)\"")

NOT_NEWLINE_RE = re.compile(r"[^\n]")


def blank_ranges(text: str, ranges: list[tuple[int, int]]) -> str:
    """`text` with each `[start, end)` in `ranges` replaced by spaces.

    Newlines are kept, so every offset in the result is still the offset it was
    in the file. That is what lets a reported line number be the line the reader
    can go and look at, and it is why brace matching can run over the blanked
    text and still give useful positions.

    `ranges` must be sorted and non-overlapping, which is what [`strip_cfg_test`]
    produces. Taking them all at once rather than one at a time is not a
    micro-optimisation: a Python string is immutable, so blanking one range costs
    a full copy of the file, and doing that once per `#[cfg(...)]` attribute made
    the tick gate quadratic in a tree that has thousands of them.
    """
    if not ranges:
        return text
    out: list[str] = []
    pos = 0
    for start, end in ranges:
        out.append(text[pos:start])
        out.append(NOT_NEWLINE_RE.sub(" ", text[start:end]))
        pos = end
    out.append(text[pos:])
    return "".join(out)


def blank(text: str, start: int, end: int) -> str:
    """`text` with `[start, end)` replaced by spaces, newlines kept."""
    return blank_ranges(text, [(start, end)])


def is_char_literal(text: str, i: int) -> bool:
    """Whether the `'` at `i` opens a char literal rather than a lifetime.

    `'a'` is a literal; `'static` and `&'a mut T` are not. The distinguishing
    shape is a closing quote two or three characters along, or a backslash
    immediately after -- `'\\n'`, `'\\''`.
    """
    if text[i + 1 : i + 2] == "\\":
        return True
    return text[i + 2 : i + 3] == "'"


def strip_comments(text: str, keep_literals: bool = False) -> str:
    """Blank comments, and string/char literals unless `keep_literals`.

    Delegates to `scripts/rustlex.py`. This was a local 98-line copy that did
    not understand RAW STRINGS -- `r"a\\"` ends at that quote, because a
    backslash is not an escape inside one, and treating it as an escape
    swallowed the terminator and paired every later quote one off. Four
    checkers import this function, three of them lane C's gates, so the bug
    was live in all four.

    The signature is unchanged, and `keep_literals` is why `rustlex` grew the
    same parameter rather than this file keeping its own lexer: a masker that
    cannot be asked to spare literals invites each caller to write one.
    """
    return rustlex.strip_noise(text, keep_literals=keep_literals)

def item_end(text: str, start: int) -> int:
    """Index just past the item beginning at `start`.

    An item ends either at a brace-matched block (`mod`, `fn`, `impl`) or at a
    semicolon (`use`, `const`). Parens and brackets are tracked so that the
    `;` in `fn f() -> [u8; 4]` is not mistaken for the end of the item.
    """
    i = start
    n = len(text)
    nest = 0
    while i < n:
        c = text[i]
        if c in "([":
            nest += 1
        elif c in ")]":
            nest -= 1
        elif nest == 0 and c == ";":
            return i + 1
        elif nest == 0 and c == "{":
            depth = 0
            j = i
            while j < n:
                if text[j] == "{":
                    depth += 1
                elif text[j] == "}":
                    depth -= 1
                    if depth == 0:
                        return j + 1
                j += 1
            return n
        i += 1
    return n


def strip_cfg_test(text: str) -> str:
    """Blank every item introduced by `#[cfg(test)]`.

    The regression test a fix leaves behind names the construct the gate looks
    for, so a check that read the whole file would accept the test as evidence
    that production does the thing. It does not -- that is precisely the
    confusion this whole class of bug is made of.

    `#[cfg(not(test))]` is left alone: it marks code that runs everywhere
    *except* under test, which is production code by any reading.

    One pass with a moving cursor, blanking once at the end. The earlier shape
    -- blank, then re-`search` the rewritten text from offset 0 -- was
    quadratic twice over, in the repeated scan and in the full string copy each
    blank costs, and `CFG_TEST_RE` matches *every* `#[cfg(...)]`, not only the
    test ones, so the iteration count is every conditional attribute in the
    file. Over lane C's tree that was 6m24s, against the "about a second" a
    pre-build gate is allowed to cost.

    The cursor is exactly equivalent to restarting, not an approximation:
    blanking only ever replaces characters with spaces, so it can destroy a
    `#[cfg(` but never create one, and every match it does destroy lies inside
    the range just blanked -- that is, behind the cursor. `item_end` likewise
    reads only forward from the attribute, into text no earlier range has
    touched, so computing it against the unblanked original gives the same
    answer.
    """
    ranges: list[tuple[int, int]] = []
    pos = 0
    while (m := CFG_TEST_RE.search(text, pos)) is not None:
        args = m.group("args")
        if not re.search(r"\btest\b", args) or re.search(r"\bnot\s*\(\s*test\b", args):
            # Not a test gate. Blank just the attribute so the scan moves on;
            # the item it decorates stays.
            ranges.append((m.start(), m.end()))
            pos = m.end()
            continue
        end = item_end(text, m.end())
        ranges.append((m.start(), end))
        pos = end
    return blank_ranges(text, ranges)


def production_only(text: str) -> str:
    """`text` reduced to the parts that run outside `cargo test`."""
    return strip_cfg_test(strip_comments(text))


def signature_of(text: str, start: int) -> str:
    """The parameter list of the `fn` beginning at `start`, brackets matched.

    A generic bound or a closure type can hold a nested paren, so this cannot
    stop at the first `)`.
    """
    i = text.find("(", start)
    if i < 0:
        return ""
    depth = 0
    j = i
    n = len(text)
    while j < n:
        if text[j] == "(":
            depth += 1
        elif text[j] == ")":
            depth -= 1
            if depth == 0:
                return text[i : j + 1]
        j += 1
    return text[i:]


def fn_body(text: str, start: int) -> str | None:
    """The braced body of the `fn` beginning at `start`, or None if it has none.

    None is the trait-method-declaration case (`fn f(&self);`) -- a signature
    with no code, which is a different thing from a body that happens to be
    empty. Callers that ask "does this function do anything?" need to tell
    those apart.

    The scan steps over the return type, so a `{` in `-> Foo<{N}>` or a where
    clause does not open the body early: it only accepts a `{` seen at bracket
    depth zero *after* the parameter list has closed.

    `->` is skipped as a unit rather than counted. Its `>` would otherwise
    close a generic that was never opened, leaving the depth at -1 for the rest
    of the scan so that no `{` is ever seen at zero -- which reads every
    function with a return type as having no body at all.
    """
    i = text.find("(", start)
    if i < 0:
        return None
    depth = 0
    n = len(text)
    j = i
    while j < n:
        if text[j] == "(":
            depth += 1
        elif text[j] == ")":
            depth -= 1
            if depth == 0:
                j += 1
                break
        j += 1
    # Between the `)` and the body lie the return type and any where clause.
    # A `;` here means there is no body at all.
    depth = 0
    while j < n:
        if text[j : j + 2] == "->":
            j += 2
            continue
        c = text[j]
        if c == ";" and depth == 0:
            return None
        if c in "(<[":
            depth += 1
        elif c in ")>]":
            depth -= 1
        elif c == "{" and depth == 0:
            end = item_end(text, j)
            return text[j + 1 : end - 1]
        j += 1
    return None


def _self_test() -> int:
    """Pin the lexer's behaviour on the shapes that are easy to get wrong.

    This module is a library, not a gate, and it had no test of any kind until
    2026-09-03 -- while four checkers
    (`check-window-wiring`, `check-key-release-wiring`, `check-frame-needles`
    via its own copy, `check-diskcleanup-test-roots`) decide what to report
    based on what it returns. A wrong answer here does not raise; it silently
    changes what those gates can see, in the direction of seeing less.

    The cases below are the ones where a plausible reimplementation diverges:
    nesting, raw strings whose body contains the delimiter, the lifetime/char
    ambiguity, and escapes. Each is a real Rust construct that appears in this
    tree.

        python scripts/rustscan.py --self-test
    """
    failures = 0

    def check(what: str, got: object, want: object) -> None:
        nonlocal failures
        if got != want:
            failures += 1
            print(f"FAIL {what}\n  got  {got!r}\n  want {want!r}")

    # Comments are blanked, and the offsets of everything else are unchanged --
    # which is what lets a gate report a line number the reader can go to.
    check(
        "line comment",
        strip_comments("let a = 1; // note\nlet b = 2;"),
        "let a = 1;        \nlet b = 2;",
    )
    check(
        "newlines survive a block comment",
        strip_comments("a\n/* one\ntwo */\nb"),
        "a\n      \n      \nb",
    )
    # Rust block comments nest; `find(\"*/\")` stops at the inner one and leaves
    # the tail of the file unblanked.
    check(
        "nested block comment",
        strip_comments("a /* x /* y */ z */ b"),
        "a                   b",
    )
    # A raw string's body may contain the plain delimiter, so the close must be
    # matched on the hash count it opened with.
    check(
        "raw string containing a quote",
        strip_comments('r#"raw " str"# end'),
        "               end",
    )
    check(
        "escaped quote does not close the string",
        strip_comments('"esc \\" still" out'),
        "               out",
    )
    # The one genuine ambiguity in Rust's grammar here: `'a'` is a literal and
    # `&'a T` is a lifetime, and blanking the latter would eat the type.
    check(
        "char literal goes, lifetime stays",
        strip_comments("let c='a'; let l:&'a T;"),
        "let c=   ; let l:&'a T;",
    )
    check(
        "keep_literals leaves the string standing",
        strip_comments('let s = "keep"; // go', keep_literals=True),
        'let s = "keep";      ',
    )
    # `keep_literals` still has to *parse* literals, or a `//` inside one opens
    # a comment that swallows the rest of the line.
    check(
        "a slash inside a kept literal does not open a comment",
        strip_comments('let u = "http://x"; let v = 1;', keep_literals=True),
        'let u = "http://x"; let v = 1;',
    )
    # `#[cfg(test)]` items go, and the production item after them stays.
    check(
        "cfg(test) module is blanked, the item after it is not",
        "fn keep()" in strip_cfg_test("#[cfg(test)]\nmod t { fn gone() {} }\nfn keep() {}"),
        True,
    )
    check(
        "the cfg(test) body really is gone",
        "gone" in strip_cfg_test("#[cfg(test)]\nmod t { fn gone() {} }\nfn keep() {}"),
        False,
    )

    if failures:
        print(f"\n{failures} self-test failure(s) in rustscan.py")
        return 1
    print("ok: rustscan self-test passed")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(_self_test())
    print(__doc__)
    print("This is a library. Run it with --self-test to check it.")
    sys.exit(0)
