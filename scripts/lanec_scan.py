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

# Lane C's ten directories, per `scripts/which-lane.py` under three lanes.
# (Since the six-lane split of 2026-09-22 these directories are no longer one
# lane's: gui/ is lanes C and F, apps/ lane E, the net crates, aes and hmac
# lane A.  The population this scans is unchanged; only the label is history.)
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


# The first line of an item proper -- as opposed to a field, a variant, a match
# arm or a statement, which a `#[cfg(test)]` can also sit on. An optional
# visibility, then any qualifiers, then the keyword.
ITEM_START = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:(?:unsafe|async|const|default|extern(?:\s+\"[^\"]*\")?)\s+)*"
    r"(?:fn|mod|impl|struct|enum|trait|union|const|static|type|use|extern|macro_rules!)\b"
)
# A field declaration or a struct-literal field: `name: ...`, not a path `a::b`.
FIELD = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?[A-Za-z_][A-Za-z0-9_]*\s*:(?!:)")
ATTRIBUTE = re.compile(r"^\s*#!?\[")
OPENERS = "([{"
CLOSERS = ")]}"


def _chars(lines, first, col):
    """Every code character from `lines[first][col]` on, as (line, column, char),
    with string and character literals skipped whole -- a brace in `"{"` or
    `'{'` is not a brace."""
    k = first
    while k < len(lines):
        line = lines[k]
        i = col if k == first else 0
        while i < len(line):
            ch = line[i]
            if ch == '"':
                i += 1
                while i < len(line) and line[i] != '"':
                    i += 2 if line[i] == "\\" else 1
                i += 1
                continue
            if ch == "'" and i + 2 < len(line):
                # A char literal: 'x', '\n', '\u{..}'. A lifetime ('a) has no
                # closing quote within a few characters and is left alone.
                close = line.find("'", i + 1, i + 12)
                if close > i + 1 and (close == i + 2 or line[i + 1] == "\\"):
                    i = close + 1
                    continue
            yield k, i, ch
            i += 1
        k += 1


def _item_end(lines, first, col):
    """The last line of an item (`fn`, `impl`, `mod`, `use` ...) starting at
    `lines[first][col]`: its `;` at depth 0, or the brace that closes its
    block."""
    depth = 0
    opened = False
    for k, _, ch in _chars(lines, first, col):
        if ch in OPENERS:
            depth += 1
            opened = opened or ch == "{"
        elif ch in CLOSERS:
            depth -= 1
            if depth <= 0 and ch == "}" and opened:
                return k
        elif ch == ";" and depth == 0:
            return k
    return len(lines) - 1


def _member_end(lines, first, col, field):
    """The last line of what a `#[cfg(test)]` on something that is not an
    item covers: a field, a struct-literal field, an enum variant, a match arm
    or a statement.

    It ends at the first `,` or `;` at depth 0; after a braced block that
    brings the depth back to 0 unless `else` follows (an arm's block body, an
    `if` statement); or just before a closer that would leave the enclosing
    scope -- the last field of a struct with no trailing comma. A field
    declaration also counts `<>`, so `m: HashMap<K, V>,` is not cut at its
    inner comma; `->` is an arrow, not a closer.
    """
    depth = 0
    angle = 0
    last = first
    block_closed_at = None
    for k, at, ch in _chars(lines, first, col):
        if block_closed_at is not None and not ch.isspace():
            # What follows a block at depth 0 decides whether it ended the
            # thing: `,`/`;` belong to it, `else` continues it, anything else
            # is the next thing.
            if ch in ",;":
                return k
            if not lines[k][at:].startswith("else"):
                return block_closed_at
            block_closed_at = None
        if ch in OPENERS:
            depth += 1
        elif ch in CLOSERS:
            if depth == 0:
                # The enclosing scope closes: the thing ended before this.
                return k if lines[k][:at].strip() else max(last, first)
            depth -= 1
            if depth == 0 and angle == 0 and ch == "}":
                block_closed_at = k
        elif field and ch == "<":
            angle += 1
        elif field and ch == ">" and angle > 0 and not lines[k][at - 1 : at] == "-":
            angle -= 1
        elif ch in ",;" and depth == 0 and angle == 0:
            return k
        if not ch.isspace():
            last = k
    return block_closed_at if block_closed_at is not None else len(lines) - 1


def test_spans(lines):
    """Line indices a `#[cfg(test)]`/`#[test]` attribute covers.

    An attribute covers the one thing it is on, and what that is decides where
    it ends. On an item -- `mod tests { .. }`, a `fn`, an `impl`, a `use` -- it
    ends at the item's `;` or the brace that closes its block, so a test module
    and the fixture helpers inside it are test code by depth. On anything else
    -- a field, a struct-literal field, a variant, a match arm, a statement --
    it ends at that thing's own `,` or `;`.

    Until 2026-09-27 every attribute was taken to cover "the next item with a
    body": on a field, that was the *next* `{` in the file, so a production
    `impl` after a struct with one test-only field vanished from both gates
    that read this (`requests/e-ac-a-cfg-test-field-hides-the-next-item-from-every-gate.md`).
    `#[cfg(test)] mod tests;` did the same to whatever braced item followed it.
    """
    inside = [False] * len(lines)
    i = 0
    while i < len(lines):
        m = TEST_ATTR.match(lines[i])
        if not m:
            i += 1
            continue
        # The thing it is on: the rest of this line if the attribute is not
        # alone on it, else the next line that is neither blank nor another
        # attribute.
        first, col = i, m.end()
        if not lines[i][col:].strip():
            first, col = i + 1, 0
            while first < len(lines) and (
                not lines[first].strip() or ATTRIBUTE.match(lines[first])
            ):
                first += 1
        if first >= len(lines):
            break
        head = lines[first][col:]
        if ITEM_START.match(head):
            end = _item_end(lines, first, col)
        else:
            end = _member_end(lines, first, col, bool(FIELD.match(head)))
        for k in range(i, end + 1):
            inside[k] = True
        i = end + 1
    return inside


def _self_test():
    """The shapes a `#[cfg(test)]` can sit on, and the lines each covers.

    Each case is (source, the 1-based lines that are test code). Run from
    `check-fields-written-never-read.py --self-test`, the gate that reads this.
    """
    cases = [
        # Lane E's reproduction: a test-only field, then a production impl.
        ("pub struct Window {\n    pub title: String,\n    #[cfg(test)]\n"
         "    scratch: Option<u32>,\n}\n\nimpl Window {\n"
         "    pub fn title(&self) -> &str {\n        &self.title\n    }\n}\n",
         {3, 4}),
        # The last field, with no trailing comma.
        ("struct A {\n    x: u32,\n    #[cfg(test)]\n    y: HashMap<u32, u32>\n}\n"
         "impl A { fn f(&self) {} }\n", {3, 4}),
        # A struct-literal field whose value has a block and a comma inside.
        ("fn new() -> A {\n    A {\n        x: 1,\n        #[cfg(test)]\n"
         "        y: B { a: 1, b: 2 },\n        z: 3,\n    }\n}\n", {4, 5}),
        # An enum variant, then an impl.
        ("enum E {\n    A,\n    #[cfg(test)]\n    Seeded(u64),\n}\nimpl E {\n"
         "    fn f(&self) {}\n}\n", {3, 4}),
        # Match arms, with and without a block body.
        ("fn f(e: E) {\n    match e {\n        #[cfg(test)]\n"
         "        E::Seeded(n) => {\n            g(n);\n        }\n"
         "        E::A => h(),\n        #[cfg(test)]\n        E::B => i(),\n"
         "        E::C => j(),\n    }\n}\n", {3, 4, 5, 6, 8, 9}),
        # A statement, then production code.
        ("fn f() {\n    #[cfg(test)]\n    let t = Instant::now();\n    g();\n}\n",
         {2, 3}),
        # An `if` statement with an `else`.
        ("fn f() {\n    #[cfg(test)]\n    if a {\n        b();\n    } else {\n"
         "        c();\n    }\n    d();\n}\n", {2, 3, 4, 5, 6, 7}),
        # A braced string does not open a block.
        ("fn f() {\n    #[cfg(test)]\n    let s = \"{\";\n    g();\n}\n", {2, 3}),
        # The item cases are unchanged: a test module by depth ...
        ("fn prod() {}\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n"
         "    #[test]\n    fn t() {}\n}\nfn after() {}\n", {2, 3, 4, 5, 6, 7}),
        # ... a `use` ends at its `;` -- it used to swallow the next item ...
        ("#[cfg(test)]\nuse std::fmt;\nimpl A {\n    fn f() {}\n}\n", {1, 2}),
        # ... and so does an external test module.
        ("#[cfg(test)]\nmod tests;\nimpl A {\n    fn f() {}\n}\n", {1, 2}),
        # An attribute and its item on one line.
        ("#[test] fn t() { a(); }\nfn prod() {}\n", {1}),
        # A function whose signature has a `where` clause and arrows.
        ("#[cfg(test)]\nfn f<T>() -> Vec<T>\nwhere\n    T: Clone,\n{\n    vec![]\n}\n"
         "fn prod() {}\n", {1, 2, 3, 4, 5, 6, 7}),
    ]
    failures = []
    for n, (source, want) in enumerate(cases, 1):
        got = {k + 1 for k, v in enumerate(test_spans(source.split("\n"))) if v}
        if got != want:
            failures.append("case %d: covered %s, wanted %s" % (n, sorted(got), sorted(want)))
    return failures


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
