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
    """Blank `//`-to-newline and `/* */` comments, and string/char literals.

    A doc comment that *talks about* the construct a gate searches for --
    including the one each finding leaves behind at its fix site -- must not
    count as the file having the construct, or every file a gate causes to be
    fixed becomes permanently invisible to it.

    Literals go too, and not for the same reason: nothing writes `Event::Tick`
    in a string. They go because [`strip_cfg_test`] matches braces, and a
    `'{'` or a `"unclosed {"` in the source would otherwise throw the match
    off and swallow the rest of the file.

    `keep_literals=True` blanks the comments and leaves the literals standing.
    A gate whose *subject* is a literal needs that:
    `check-diskcleanup-test-roots.py` looks for the path `"/"` being handed to
    something that deletes, and the default would blank away the only evidence
    there is. Literals are still fully *parsed* in that mode rather than
    skipped over -- a `"http://x"` read as ordinary text would open a comment,
    and a `'"'` would open a string that ran to the end of the file.

    Callers that match braces must keep the default. A `"{"` left standing is
    exactly the trap the blanking was written to close, and this argument does
    not make it safe -- it makes it the caller's problem.
    """
    out = list(text)
    i = 0
    n = len(text)

    def wipe(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    def wipe_literal(a: int, b: int) -> None:
        if not keep_literals:
            wipe(a, b)

    while i < n:
        two = text[i : i + 2]
        if two == "//":
            j = text.find("\n", i)
            j = n if j < 0 else j
            wipe(i, j)
            i = j
        elif two == "/*":
            # Rust block comments nest, so a naive `find("*/")` stops early on
            # `/* /* */ */` and leaves a stray `*/` behind.
            depth = 0
            j = i
            while j < n:
                if text[j : j + 2] == "/*":
                    depth += 1
                    j += 2
                elif text[j : j + 2] == "*/":
                    depth -= 1
                    j += 2
                    if depth == 0:
                        break
                else:
                    j += 1
            wipe(i, j)
            i = j
        elif text[i] == "r" and (m := RAW_STRING_RE.match(text, i)):
            hashes = m.group("hashes")
            close = text.find('"' + hashes, m.end())
            j = n if close < 0 else close + 1 + len(hashes)
            wipe_literal(i, j)
            i = j
        elif text[i] == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                elif text[j] == '"':
                    j += 1
                    break
                else:
                    j += 1
            wipe_literal(i, j)
            i = j
        elif text[i] == "'" and is_char_literal(text, i):
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                elif text[j] == "'":
                    j += 1
                    break
                else:
                    j += 1
            wipe_literal(i, j)
            i = j
        else:
            i += 1
    return "".join(out)


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
