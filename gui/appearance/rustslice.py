"""Where a Rust file's production code ends.

Every sweep in this directory needs the same answer -- "is this line shipped
or is it a test?" -- and every one of them got it wrong the same way, so the
answer lives here once.

The wrong version is a latch::

    if line.strip().startswith("#[cfg(test)]"):
        in_test = True     # ...and never False again

`#[cfg(test)]` does not mean "tests start here". It means "the next item is a
test-only item", and that item is very often a single `use`::

    #[cfg(test)]
    use guitk::probe;      <- apps/pdfviewer, line 43 of 5000

A latch tripped there declares the remaining 99% of the file to be tests.
`ink-text.py` did exactly that and reported "0 text sites" for a file with
seven dual-use colours in it -- and reported it as a *pass*, which is the
expensive half. The same slice bug, in the production-literal counter, once
measured `pdfviewer` over 0.9% of itself and found no literals.

The rule that is actually right: the file's tests begin at a `#[cfg(test)]`
whose item -- after any further attributes -- is a `mod`. Everything before
that is shipped.

Returns `len(lines)` when there is no test module, so a caller can always
slice `lines[:production_end(lines)]` without a special case.
"""
import re

ATTR = re.compile(r"^\s*#!?\[")
MOD = re.compile(r"^\s*(pub\s+)?mod\s+\w+")


def production_end(lines):
    """Index of the first line of the file's test module, or `len(lines)`."""
    for i, line in enumerate(lines):
        if line.strip() != "#[cfg(test)]":
            continue
        # Skip the rest of the attribute stack, counting brackets rather than
        # looking for `]` -- `#[allow(clippy::indexing_slicing)]` ends in `)]`
        # and an earlier version of this scan stopped on it mid-attribute.
        j, depth = i + 1, 0
        while j < len(lines):
            if depth == 0 and not ATTR.match(lines[j]):
                break
            depth += lines[j].count("[") - lines[j].count("]")
            j += 1
        if j < len(lines) and MOD.match(lines[j]):
            return i
    return len(lines)


def is_production(lines, i):
    """Is line `i` shipped, rather than part of the test module?"""
    return i < production_end(lines)
