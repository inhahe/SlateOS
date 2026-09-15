#!/usr/bin/env python3
"""Lane C's per-line scanner for the write-only-field and uncalled-function gates.

**NOT `rustscan.py`.** That module already existed, has since Sep 3, is
imported by seven scripts, and solves the same problem more thoroughly: it
works on whole-file text with real Rust lexing (raw strings, char literals,
nested `#[cfg(test)]` items) and exposes `production_only`, where this one
works line by line. Its docstring opens by naming the two traps -- "a comment
that mentions X" and "a test that exercises X" -- that this file's author spent
2026-09-14 rediscovering one at a time.

This module exists because it was written before its author noticed that, and
it kept its own name only after it had **overwritten** `rustscan.py` with
`cat >` and had to be untangled. The right end state is for the two gates here
to move onto `rustscan.py` and for this file to go; it is kept for now because
that is a behaviour change to two working gates and deserves its own pass with
its own proof, not a rushed edit on top of a repair.

WHY A SHARED MODULE. `check-tested-but-uncalled.py` and
`check-fields-written-never-read.py` ask different questions -- one about
functions, one about struct fields -- but both must answer the same three
sub-questions first, and all three were got wrong at least once:

  * **Which lines are test code?** `#[cfg(test)] mod tests;` puts the attribute
    in the *parent*, so a file that is wholly test code says so nowhere inside
    itself. Scanning `gui/desktop/src/session/tests.rs` as production counted
    the very callers the gates exist to see past.
  * **Which text is code at all?** Comments name functions, and the comments
    that name a function are overwhelmingly the ones explaining a bug it was
    in. Three sentences about the day `load_pinned` had no caller counted as
    three callers, so the record of the last time a door went missing was
    enough to hide the next.
  * **Which program is this?** A save/load pair is one program keeping one
    thing. Matching on bare names across the tree paired `apps/passwordgen`'s
    `export_history` with `gui/desktop`'s `load_history` -- different programs,
    different data, a shared suffix.

Duplicating that into a second gate would mean a second chance to get each one
wrong, and a fix that lands in one copy.
"""

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]

# Lane C's ten directories, per `scripts/which-lane.py`.
LANE_C_ROOTS = (
    "apps",
    "gui",
    "net",
    "netipc",
    "netproto",
    "netring",
    "net80211",
    "aes",
    "hmac",
    "pkg",
)

# Build output, or not Rust.
NOT_SOURCE = {"target", "build", "scripts", "requests", "toolchain", "limine"}

TEST_ATTR = re.compile(r"^\s*#\[(?:cfg\(test\)|test|tokio::test)\]")
EXT_TEST_MOD = re.compile(r"^\s*mod\s+([a-z_][a-z0-9_]*)\s*;")


def every_root(root=None):
    """Every top-level directory with Rust in it."""
    base = pathlib.Path(root) if root else ROOT
    found = []
    for child in sorted(base.iterdir()):
        if not child.is_dir() or child.name.startswith("."):
            continue
        if child.name in NOT_SOURCE:
            continue
        if next(child.rglob("*.rs"), None) is not None:
            found.append(child.name)
    return tuple(found)


def rust_files(roots=LANE_C_ROOTS, root=None):
    base = pathlib.Path(root) if root else ROOT
    for name in roots:
        top = base / name
        if not top.is_dir():
            continue
        for path in top.rglob("*.rs"):
            if "target" in path.parts or "build" in path.parts:
                continue
            yield path


def crate_of(rel_path):
    """`apps/passwordgen/src/main.rs` -> `apps/passwordgen`."""
    parts = rel_path.split("/src/")
    if len(parts) > 1:
        return parts[0]
    return rel_path.rsplit("/", 1)[0]


def strip_comments(line, in_block):
    """The code on a line, with comments removed. Returns (code, still_in_block).

    String literals are deliberately kept: comments were observed masking a
    real defect three times over, strings never once, and changing two things
    at a time would leave neither proven.
    """
    out = []
    i = 0
    n = len(line)
    in_str = False
    while i < n:
        ch = line[i]
        nxt = line[i + 1] if i + 1 < n else ""
        if in_block:
            if ch == "*" and nxt == "/":
                in_block = False
                i += 2
                continue
            i += 1
        elif in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
            out.append(ch)
            i += 1
        elif ch == '"':
            in_str = True
            out.append(ch)
            i += 1
        elif ch == "/" and nxt == "/":
            break
        elif ch == "/" and nxt == "*":
            in_block = True
            i += 2
        else:
            out.append(ch)
            i += 1
    return "".join(out), in_block


def code_lines(path):
    """A file's lines with comments removed."""
    raw = path.read_text(encoding="utf-8", errors="replace").split("\n")
    out = []
    block = False
    for line in raw:
        code, block = strip_comments(line, block)
        out.append(code)
    return out


def test_spans(lines):
    """Line indices inside a `#[cfg(test)]`/`#[test]` region, by brace depth.

    Depth counting rather than a marker scan, because a test module is where a
    fixture's own helpers live and those are as much "test code" as the
    `#[test]` functions beside them.
    """
    inside = [False] * len(lines)
    i = 0
    while i < len(lines):
        if not TEST_ATTR.match(lines[i]):
            i += 1
            continue
        # Walk to the opening brace of the item the attribute is on.
        j = i
        while j < len(lines) and "{" not in lines[j]:
            j += 1
        if j >= len(lines):
            break
        depth = 0
        for k in range(j, len(lines)):
            depth += lines[k].count("{") - lines[k].count("}")
            inside[k] = True
            if depth <= 0 and k >= j:
                i = k + 1
                break
        else:
            break
    return inside


def external_test_files(roots=LANE_C_ROOTS, root=None):
    """Files that are wholly test code because their *parent* declared them so."""
    found = set()
    for path in rust_files(roots, root):
        lines = code_lines(path)
        for n, line in enumerate(lines):
            if not TEST_ATTR.match(line):
                continue
            nxt = lines[n + 1] if n + 1 < len(lines) else ""
            m = EXT_TEST_MOD.match(nxt)
            if not m:
                continue
            stem = path.parent / m.group(1)
            found.add(stem.with_suffix(".rs"))
            found.add(stem / "mod.rs")
    return found


def scanned(roots=LANE_C_ROOTS, root=None):
    """Yield (path, lines, inside) for every Rust file in `roots`."""
    whole_file_tests = external_test_files(roots, root)
    for path in rust_files(roots, root):
        lines = code_lines(path)
        inside = test_spans(lines)
        if path in whole_file_tests:
            inside = [True] * len(lines)
        yield path, lines, inside
