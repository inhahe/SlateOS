#!/usr/bin/env python3
"""Find lock guards held across a call that re-acquires the same lock.

This is the cross-function form of the bug that froze a boot in
`fs::encrypt::encrypt`:

    STATE.lock().files_encrypted = STATE.lock().files_encrypted + 1

That one is visible to a grep, because both acquires are in one statement.  The
form this script exists for is not::

    pub fn set_policy(..) {
        let mut state = STATE.lock();
        state.rules.push(..);
        refresh_cache();          // <-- and refresh_cache() takes STATE.lock()
    }

Both deadlock identically on a non-reentrant spinlock, and the second is
invisible to every text search, because no single line mentions the lock twice.

Scope and honesty about it
--------------------------
This is a *heuristic within one file*.  It resolves calls only to functions
defined in the same file, and locks only via ALL-CAPS static receivers.  That
covers the module-private `static STATE: Mutex<..>` pattern this kernel uses
everywhere, which is where the risk actually lives, and it keeps the analysis
free of any need to resolve imports or trait dispatch.

It therefore has false negatives by construction (a call into another module
that reaches back is not seen).  It aims to have few false positives, and every
report is meant to be read by a human before anything is changed:

  * a guard explicitly `drop()`ed before the call is not reported;
  * a guard whose enclosing block ends before the call is not reported;
  * `try_lock()` is never reported -- it cannot deadlock, it returns None.

Exit codes: 0 clean, 1 findings, 2 could not run.
"""

from __future__ import annotations

import re
import sys
from collections.abc import Iterator
from pathlib import Path

# A lock acquisition on a static receiver: STATE.lock(), TABLE.lock_irqsave().
# try_lock() is deliberately excluded -- it returns None rather than spinning,
# so re-entering it is not a deadlock.
ACQUIRE = re.compile(r"\b([A-Z][A-Z0-9_]{2,})\s*\.\s*(lock|lock_irqsave)\s*\(\s*\)")

# `let g = STATE.lock();` / `let mut g = STATE.lock();` -- a *named* guard, which
# is the only kind that can still be alive when a later call runs.
BINDING = re.compile(
    r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*=\s*([A-Z][A-Z0-9_]{2,})\s*\.\s*"
    r"(?:lock|lock_irqsave)\s*\(\s*\)\s*;"
)

FN_DEF = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)\s*[(<]")
CALL = re.compile(r"(?<![\w:.])([a-z_][a-z0-9_]*)\s*\(")
DROP = re.compile(r"\bdrop\s*\(\s*([a-z_][a-z0-9_]*)\s*\)")


def _raw_string_start(src: str, i: int) -> tuple[int, int] | None:
    """If a raw string literal starts at `i`, return `(hash_count, quote_index)`.

    Recognises `r"`, `r#"`, `r##"` and the byte forms `br"`, `br#"`. A raw
    string has no escapes, so `r"C:\\"` ends at its own quote -- scanning it
    with the ordinary `\\`-aware string rule would swallow the closing quote and
    run on into the rest of the file.
    """
    n = len(src)
    j = i
    if src[j] == "b":
        j += 1
    if j >= n or src[j] != "r":
        return None
    j += 1
    hashes = 0
    while j < n and src[j] == "#":
        hashes += 1
        j += 1
    return (hashes, j) if j < n and src[j] == '"' else None


def _char_literal_end(src: str, i: int) -> int | None:
    """If a char literal starts at the quote `i`, return the offset just past it.

    Rust spends `'` on three different things: a literal (`'x'`), a lifetime
    (`&'a T`) and a loop label (`'outer: loop`). Only the first has a closing
    quote, so a scanner that assumes one desynchronises on every lifetime it
    meets. They are told apart the way the grammar does it -- a literal is
    `'\\<escape>'` or `'<single char>'`, and any other `'` opens nothing.

    Returns `None` for a lifetime or label, whose quote must be stepped over
    rather than treated as an opening delimiter.
    """
    n = len(src)
    if i + 1 >= n:
        return None
    if src[i + 1] == "\\":
        # Skip the backslash *and* the character it escapes, so that the
        # closing quote of `'\''` is not mistaken for the escaped one.
        j = i + 3
        while j < n and src[j] not in ("'", "\n"):
            j += 1
        return j + 1 if j < n and src[j] == "'" else None
    # `src` is a `str`, so a multi-byte character such as `'e'` is one element
    # here and this stays a single-character test.
    if i + 2 < n and src[i + 2] == "'":
        return i + 3
    return None


def strip_noise(src: str, keep_literals: bool = False) -> str:
    """Blank out comments and string/char literals, preserving byte offsets.

    Offsets must be preserved because every later step reports and slices by
    index; replacing rather than deleting keeps line numbers exact.

    Char literals matter as much as strings here, and for a reason that is easy
    to miss: a `'"'` in the source opens a string as far as a quote-only scanner
    is concerned, and that phantom string then runs to the next `"` in the file,
    blanking every brace in between. `find_bodies` loses its nesting and returns
    a fraction of the file -- silently, because a gate that finds nothing looks
    exactly like a gate that found nothing wrong. That is not hypothetical: it
    hid a real lock-order inversion in `kshell.rs`, where this function saw 43
    of the file's 984 function bodies.

    With `keep_literals=True`, literals are still *scanned* -- which is the part
    that matters, since a `//` inside `"https://x"` opens no comment and a `"`
    inside a comment opens no string -- but their characters are passed through
    instead of blanked. Comments go either way.

    That mode exists because the sibling gates that match *on* literal text
    (`check-option-refusal.py`, `check-usage-status.py`, `check-query-status.py`
    look for `"Usage: ..."`, `"-x"`, `shell_println!`) each grew their own
    line-local comment stripper that understood only `//` to end of line. There
    were five hand-rolled Rust lexers in `scripts/` and only this one handled
    nested `/* */`, raw strings and char literals or had a self-test. One
    parameter here is cheaper than five parsers, and far cheaper than five
    parsers that disagree: the two `check-{usage,query}-status.py` mirrors had
    already drifted apart on exactly this point without either being able to
    report it.
    """
    out = list(src)
    i, n = 0, len(src)

    def blank(k: int) -> None:
        """Blank one literal character unless the caller asked to keep it."""
        if not keep_literals and src[k] != "\n":
            out[k] = " "

    while i < n:
        c = src[i]
        if c == "'":
            end = _char_literal_end(src, i)
            if end is None:
                i += 1  # a lifetime or a loop label, not a literal
                continue
            while i < min(end, n):
                blank(i)
                i += 1
            continue
        if (
            c in ("r", "b")
            and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_"))
            and (raw := _raw_string_start(src, i)) is not None
        ):
            hashes, quote = raw
            for k in range(i, quote + 1):
                blank(k)
            i = quote + 1
            close = '"' + "#" * hashes
            end = src.find(close, i)
            stop = n if end == -1 else min(end + len(close), n)
            while i < stop:
                blank(i)
                i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                out[i] = " "
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth = 1
            out[i] = out[i + 1] = " "
            i += 2
            while i < n and depth:
                if src[i] == "/" and i + 1 < n and src[i + 1] == "*":
                    depth += 1
                    out[i] = out[i + 1] = " "
                    i += 2
                    continue
                if src[i] == "*" and i + 1 < n and src[i + 1] == "/":
                    depth -= 1
                    out[i] = out[i + 1] = " "
                    i += 2
                    continue
                if src[i] != "\n":
                    out[i] = " "
                i += 1
        elif c == '"':
            blank(i)
            i += 1
            while i < n:
                if src[i] == "\\":
                    blank(i)
                    if i + 1 < n:
                        blank(i + 1)
                    i += 2
                    continue
                if src[i] == '"':
                    blank(i)
                    i += 1
                    break
                blank(i)
                i += 1
        else:
            i += 1
    return "".join(out)


def find_all_bodies(src: str) -> dict[str, list[tuple[int, int]]]:
    """Map each function name to the [start, end) offsets of *every* definition.

    A name is not unique in a Rust file: `fn report` may be a method on four
    different enums, and a nested `fn piped` may be redeclared in a dozen
    sibling blocks. A caller that wants to know "what can a call to this name
    reach" must consider all of them, because it cannot resolve the receiver's
    type -- so it unions their bodies and over-approximates. `find_bodies`
    exists for callers that only need one span and are hand-checked.
    """
    bodies: dict[str, list[tuple[int, int]]] = {}
    for m in FN_DEF.finditer(src):
        name = m.group(1)
        # Walk forward to the body's opening brace, skipping the parameter list,
        # return type and any where-clause. A `{` at paren/bracket depth 0 is it.
        i = m.end() - 1
        depth_paren = 0
        depth_angle = 0
        start = None
        while i < len(src):
            ch = src[i]
            if ch in "([":
                depth_paren += 1
            elif ch in ")]":
                depth_paren -= 1
            elif ch == "<":
                depth_angle += 1
            elif ch == ">":
                depth_angle = max(0, depth_angle - 1)
            elif ch == ";" and depth_paren <= 0:
                break  # a trait method declaration with no body
            elif ch == "{" and depth_paren <= 0:
                start = i
                break
            i += 1
        if start is None:
            continue
        depth = 0
        j = start
        while j < len(src):
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    bodies.setdefault(name, []).append((start + 1, j))
                    break
            j += 1
    return bodies


def find_bodies(src: str) -> dict[str, tuple[int, int]]:
    """Map each function name in the file to its body's [start, end) offsets.

    Nested functions and same-named methods in different impl blocks both occur
    here; a later definition simply wins, which is acceptable for a heuristic
    whose output is hand-checked. Use `find_all_bodies` when every definition
    matters.
    """
    return {name: spans[-1] for name, spans in find_all_bodies(src).items()}


def direct_locks(body: str) -> set[str]:
    return {m.group(1) for m in ACQUIRE.finditer(body)}


def called_names(body: str, known: set[str]) -> set[str]:
    return {m.group(1) for m in CALL.finditer(body) if m.group(1) in known}


def transitive_locks(
    fn: str, bodies: dict[str, tuple[int, int]], src: str, memo: dict[str, set[str]],
    stack: tuple[str, ...] = (),
) -> set[str]:
    """Locks `fn` may acquire, directly or through same-file callees."""
    if fn in memo:
        return memo[fn]
    if fn in stack:
        return set()  # recursion in the call graph; the fixpoint handles it
    span = bodies.get(fn)
    if span is None:
        return set()
    body = src[span[0] : span[1]]
    acc = direct_locks(body)
    for callee in called_names(body, set(bodies)):
        if callee == fn:
            continue
        acc |= transitive_locks(callee, bodies, src, memo, stack + (fn,))
    memo[fn] = acc
    return acc


def block_end(src: str, pos: int, limit: int) -> int:
    """Offset at which the block containing `pos` closes (or `limit`)."""
    depth = 0
    i = pos
    while i < limit:
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            if depth == 0:
                return i
            depth -= 1
        i += 1
    return limit


def walk_block(
    struct: list[str], start: int, start_col: int = 0, limit: int = 300
) -> Iterator[tuple[int, int, int]]:
    """Walk forward from a point until control leaves the block containing it.

    Yields `(index, from_col, to_col)` for each line, where the slice
    `line[from_col:to_col]` is the part of that line that belongs to the block
    the walk started in.  A caller must match only that slice: text outside it
    belongs to a *sibling* block, and attributing it to this one is the entire
    bug this helper exists to prevent.

    `struct` must have comments *and* literals blanked -- `strip_noise(text)`,
    split on newlines.  A brace inside a comment or a string is not structure,
    and counting one shifts every boundary after it.

    Braces are counted a character at a time, never a line at a time.  The line
    `} else {` closes one block and opens another, so a line-granular sum nets
    to zero: the walk never sees depth go negative, sails straight past the
    brace that ends the block, and carries on into the `else` branch.  Anything
    it finds there gets credited to the `if`.  That is not hypothetical --
    `check-usage-status.py` counted by the line and so reported *zero* findings
    while hiding 195 real ones across 50 shell commands, for months, with no
    symptom other than looking clean.

    The distinction worth keeping in mind, because three other counters in this
    directory are line-granular and *correct*: a walk over a **balanced** block
    (start on its `{`, stop when depth returns to 0) is safe at line
    granularity, because `} else {` nets to zero truthfully -- the block really
    does end at depth 0.  Only a walk like this one, which must notice depth
    going **negative** -- leaving a block it never entered -- is broken by it,
    because that dip happens *within* the line and a line-sum cannot see it.

    `start_col` matters for the same reason.  A caller that found its match
    mid-line must pass the match's column, or a one-liner such as
    `if n < 2 { a(); } else { b(); }` walks from column zero, counts the `{` it
    was already inside, and reads the `else` branch as its own.
    """
    depth = 0
    for k in range(start, min(start + limit, len(struct))):
        line = struct[k]
        col = start_col if k == start else 0
        for j in range(col, len(line)):
            ch = line[j]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth < 0:
                    yield k, col, j
                    return
        yield k, col, len(line)


def statements(code: list[str], struct: list[str]) -> Iterator[tuple[int, str]]:
    """Yield `(start_index, text)` for each statement-sized span of a file.

    `text` is the span's source with its newlines collapsed to single spaces,
    so a regex that spans method calls can be matched against it.  Match
    against *this*, never against a line.

    Why: **the author does not decide where the newlines go -- `cargo fmt`
    does.**  These two are the same code, and a line-granular regex sees only
    the first::

        let n = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(20);

        let n = parts
            .get(2)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(20);

    Which form a given site takes is decided by how long the surrounding names
    happen to be.  `check-option-refusal.py` matched by the line and so
    published 240 findings while hiding 466 more of the identical shape -- and,
    like the `} else {` bug that `walk_block` exists for, a gate that is wrong
    in the *clean* direction produces no symptom at all.

    Two views are required, and they must be the two that `strip_noise`
    produces from one text, because it blanks in place and so leaves every line
    at its original length and number:

    * `struct` -- comments *and* literals blanked.  Terminators and nesting are
      counted over this only.  A `(` inside a string literal is not a bracket;
      counting one makes the depth drift upward and never come back, after
      which no terminator fires again and the rest of the file glues into a
      single span.  (Measured: doing it over the literals-kept view collapsed
      this file's 100698 spans into 1068.)
    * `code` -- comments blanked, literals kept.  The text is taken from here,
      because the detectors look for literal option spellings.

    A span ends at `;`, `{`, `}` or `,` appearing at bracket depth zero.  The
    comma is not decoration: at depth zero it separates match arms, and without
    it a `.parse()` in one arm and an `.unwrap_or()` in the next would glue
    into a single span and match a chain that does not exist.  `{` and `}` end
    a span only outside brackets, so a closure body -- `unwrap_or_else(|| {
    ... })` -- stays with the chain that owns it.

    A statement's start index is the line its *first* text sits on, which is
    the line a reader would call it.  Two consequences worth knowing: a site
    can be reported against an earlier line than the one it is written on (a
    wrapped chain is credited to its `let`), and the enclosing function is
    looked up at that line -- always the same function, since a statement
    cannot straddle two.
    """
    depth = 0
    buf: list[str] = []
    start: int | None = None
    for i, sline in enumerate(struct):
        cline = code[i]
        seg = 0
        for j, ch in enumerate(sline):
            if ch in "([":
                depth += 1
            elif ch in ")]":
                # Clamped at zero rather than allowed to go negative: a file
                # this walk cannot balance (a macro with unmatched brackets,
                # say) would otherwise drift permanently and silence every
                # terminator after it.  Clamping localises the damage.
                depth = max(depth - 1, 0)
            elif depth == 0 and ch in ";{},":
                if start is None:
                    start = i
                buf.append(cline[seg : j + 1])
                text = " ".join(part.strip() for part in buf).strip()
                if text:
                    yield start, text
                buf = []
                start = None
                seg = j + 1
        rest = cline[seg:]
        if rest.strip():
            if start is None:
                start = i
            buf.append(rest)
    if buf and start is not None:
        text = " ".join(part.strip() for part in buf).strip()
        if text:
            yield start, text


def analyse(path: Path) -> list[str]:
    raw = path.read_text(encoding="utf-8", errors="replace")
    src = strip_noise(raw)
    bodies = find_bodies(src)
    if not bodies:
        return []
    memo: dict[str, set[str]] = {}
    known = set(bodies)
    findings: list[str] = []

    for fn, (bstart, bend) in sorted(bodies.items()):
        body = src[bstart:bend]
        for bind in BINDING.finditer(body):
            guard, lock = bind.group(1), bind.group(2)
            # The guard is live from just after the binding until either an
            # explicit drop(guard) or the end of its enclosing block.
            live_from = bstart + bind.end()
            live_to = block_end(src, live_from, bend)
            region = src[live_from:live_to]
            d = DROP.search(region)
            if d and d.group(1) == guard:
                region = region[: d.start()]
            for callee in sorted(called_names(region, known)):
                if callee == fn:
                    continue
                if lock in transitive_locks(callee, bodies, src, memo):
                    line = raw.count("\n", 0, bstart + bind.start()) + 1
                    findings.append(
                        f"{path}:{line}: `{fn}` holds `{lock}` in `{guard}` and then "
                        f"calls `{callee}`, which acquires `{lock}` again"
                    )
    return findings


# Each case is (name, source, expected function names). Every one of them is a
# form that made the pre-2026-08-24 `strip_noise` return *nothing* for the whole
# file, or would have. They are kept as a runnable test rather than a comment
# because the failure they guard against is silent: a parser that loses its
# nesting reports zero findings, which is indistinguishable from a clean tree.
# `fn b` is the control in every case -- it is what a desynchronised scan drops.
_PARSER_CASES: tuple[tuple[str, str, list[str]], ...] = (
    # A quote *inside a char literal* opened a phantom string that ran to the
    # next `"` in the file, blanking every brace in between. This is the one
    # that hid a real lock-order inversion in kshell.rs.
    ("quote char literal", """fn a() { if c == '"' { g(); } } fn b() { h(); }""", ["a", "b"]),
    (
        "byte quote literal",
        """fn a() { match c { b'"' => g(), _ => (), } } fn b() { h(); }""",
        ["a", "b"],
    ),
    ("escaped quote", r"""fn a() { p('\''); } fn b() { h(); }""", ["a", "b"]),
    ("escaped backslash", r"""fn a() { p('\\'); } fn b() { h(); }""", ["a", "b"]),
    ("unicode escape", r"""fn a() { p('\u{1F600}'); } fn b() { h(); }""", ["a", "b"]),
    ("multi-byte char", """fn a() { p('\u00e9'); } fn b() { h(); }""", ["a", "b"]),
    # Lifetimes and labels are `'` with no closing quote; treating them as
    # literals desynchronises just as badly in the other direction.
    ("lifetime", """fn a<'x>(v: &'x u8) { g(); } fn b() { h(); }""", ["a", "b"]),
    ("loop label", """fn a() { 'outer: loop { break 'outer; } } fn b() { h(); }""", ["a", "b"]),
    # A raw string has no escapes, so `r"C:\"` ends at its own quote.
    ("raw string ending in backslash", r'''fn a() { p(r"C:\"); } fn b() { h(); }''', ["a", "b"]),
    ("hashed raw string", '''fn a() { p(r#"a"b{"#); } fn b() { h(); }''', ["a", "b"]),
    ("byte raw string", '''fn a() { p(br#"x"y"#); } fn b() { h(); }''', ["a", "b"]),
    ("brace inside string", '''fn a() { p("}{"); } fn b() { h(); }''', ["a", "b"]),
    ("brace inside char literal", """fn a() { p('}'); } fn b() { h(); }""", ["a", "b"]),
    ("apostrophe in block comment", '''fn a() { /* don't */ g(); } fn b() { h(); }''', ["a", "b"]),
    ("apostrophe in line comment", '''fn a() { // don't\n g(); } fn b() { h(); }''', ["a", "b"]),
)


# `keep_literals=True` cases, as (name, source, expected output). These assert
# the *exact* result rather than a body list, because the whole point of the
# mode is which characters survive -- a body list would not notice a literal
# being blanked. Each is a shape that defeated one of the line-local strippers
# this mode replaces.
_KEEP_LITERAL_CASES: tuple[tuple[str, str, str], ...] = (
    # The case that started it: a comment saying a thing is absent satisfied a
    # grep for that thing.
    (
        "line comment goes, string stays",
        'p("Usage: x"); // no set_exit(1) here\n',
        'p("Usage: x");                       \n',
    ),
    # `//` inside a string opens no comment. A stripper that cuts at the first
    # `//` would eat the rest of the line, including a real trailing `set_exit`.
    (
        "slashes inside a string are not a comment",
        'p("https://x"); set_exit(1);\n',
        'p("https://x"); set_exit(1);\n',
    ),
    # A quote inside a comment opens no string -- the failure that makes a
    # naive stripper swallow to the next quote anywhere in the file.
    (
        "quote inside a comment",
        'let a = 1; // it\'s "quoted\np("real");\n',
        'let a = 1;                \np("real");\n',
    ),
    # Neither of the two ad-hoc copies understood block comments at all.
    (
        "block comment goes, spanning lines",
        'a(); /* set_exit(1)\n more */ b("keep");\n',
        'a();               \n         b("keep");\n',
    ),
    # Nested block comments are legal in Rust and end at the *matching* `*/`.
    (
        "nested block comment",
        'a(); /* x /* y */ z */ b();\n',
        'a();                   b();\n',
    ),
    # A raw string keeps its backslashes and may contain `//` and `/*`.
    (
        "raw string is not a comment",
        'p(r"C:\\dir // *"); c();\n',
        'p(r"C:\\dir // *"); c();\n',
    ),
    # A `'"'` must not open a string, or every brace to the next quote is lost.
    (
        "char literal holding a quote",
        'if c == \'"\' { /* gone */ }\n',
        'if c == \'"\' {            }\n',
    ),
)


# `walk_block` cases, as (name, source, start line, start column, expected).
# The expected value is the text the walk says belongs to the starting block,
# lines joined by newline -- written out in full so a boundary that moves by one
# character is visible in the failure message rather than inferred from a count.
#
# Every case below is answered *wrongly* by a line-granular count, which is what
# all of this checker's callers used before the walk was shared. They are
# regression tests for a bug that hid 195 findings while the gate printed a
# clean line.
_BLOCK_WALK_CASES: tuple[tuple[str, str, int, int, str], ...] = (
    # The shipped bug, at its smallest: `} else {` nets to zero by the line, so
    # a line-granular walk reads `b()` -- which is the else branch -- as part of
    # the `if`.
    (
        "} else { ends the block",
        "if n < 2 {\n    a();\n} else {\n    b();\n}\n",
        1,
        0,
        "    a();\n",
    ),
    # The same shape spelled as match arms: a sibling arm must not be read as a
    # continuation of this one. Starts mid-line, as a real caller does.
    (
        "a sibling match arm is not entered",
        "match x {\n    A => { a(); }\n    B => { b(); }\n}\n",
        1,
        11,
        "a(); ",
    ),
    # The converse, and the reason this cannot simply stop at the first `}`:
    # a block *opened after* the start closes back to depth 0 and the walk must
    # continue through it.
    (
        "a balanced nested block does not end the walk",
        "a();\nif q { b(); }\nc();\n}\n",
        0,
        0,
        "a();\nif q { b(); }\nc();\n",
    ),
    # `start_col` earning its place: from column zero this walk counts the `{`
    # it is already inside and hands back the else branch as well.
    (
        "start_col keeps a one-liner honest",
        "if n < 2 { a(); } else { b(); }\n",
        0,
        11,
        "a(); ",
    ),
    # Braces in comments and literals are not structure. Passing raw lines here
    # instead of `strip_noise` output would end the walk at the `}` in the
    # comment, one line early.
    (
        "a brace in a comment is not structure",
        "a();\nb(); // closes with }\nc();\n}\n",
        0,
        0,
        "a();\nb();                 \nc();\n",
    ),
)

# Cases for `statements`. Each is `(name, src, expected)` where `expected` is
# the list of `(start_index, text)` pairs the span walk must produce. Every one
# of them is answered wrongly by matching a line: the first is the bug itself,
# and the rest are the ways a naive splitter gets the *opposite* answer -- too
# few spans, or too many -- once real Rust is fed to it.
_STATEMENT_CASES: tuple[tuple[str, str, list[tuple[int, str]]], ...] = (
    # The shipped bug. `cargo fmt` chose these newlines, and a per-line regex
    # for `.parse()…​.unwrap_or(` sees four fragments and matches none of them.
    (
        "a chain rustfmt wrapped is one span",
        "let n = parts\n    .get(2)\n"
        "    .and_then(|s| s.parse::<u32>().ok())\n    .unwrap_or(20);\n",
        [(0, "let n = parts .get(2) .and_then(|s| s.parse::<u32>().ok()) .unwrap_or(20);")],
    ),
    # The unwrapped spelling of the identical code must give the identical
    # span, or the walk has merely moved the blind spot rather than closed it.
    (
        "the same chain on one line is the same span",
        "let n = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(20);\n",
        [(0, "let n = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(20);")],
    ),
    # Why the depth count must run over the literals-blanked view. With
    # literals kept, this `(` is never closed, the depth drifts up by one, and
    # no terminator fires for the rest of the file -- which is how the first
    # draft of this walk turned 100698 spans into 1068 and found 2 sites.
    (
        "a bracket inside a string literal is not a bracket",
        'shell_println!("(");\nlet n = x.parse::<u32>().unwrap_or(0);\n',
        [(0, 'shell_println!("(");'), (1, "let n = x.parse::<u32>().unwrap_or(0);")],
    ),
    # A `;` inside a literal does not end a span either, for the same reason.
    (
        "a semicolon inside a string literal does not end a span",
        'let s = "a;b".len();\n',
        [(0, 'let s = "a;b".len();')],
    ),
    # The comma is a terminator at depth zero because match arms are separated
    # by one. Without it these two arms glue, and a `.parse()` in the first
    # would pair with an `.unwrap_or()` in the second to match a chain that is
    # not in the source -- a false finding, which costs a gate its credibility
    # just as surely as a missed one.
    (
        "sibling match arms do not glue",
        "match x {\n    A => a.parse::<u32>(),\n    B => b.unwrap_or(0),\n}\n",
        [
            (0, "match x {"),
            (1, "A => a.parse::<u32>(),"),
            (2, "B => b.unwrap_or(0),"),
            (3, "}"),
        ],
    ),
    # ...but a comma *inside* brackets is an argument separator and must not
    # split, or every multi-argument call becomes several spans.
    (
        "a comma inside brackets is not a terminator",
        "let n = cmp::min(a, b).parse::<u32>().unwrap_or(0);\n",
        [(0, "let n = cmp::min(a, b).parse::<u32>().unwrap_or(0);")],
    ),
    # A closure body's braces sit inside the call's parens, so they do not end
    # the span: the fallback stays attached to the chain it belongs to.
    (
        "a closure body stays with its chain",
        "let n = s.parse::<u32>().unwrap_or_else(|_| { 0 });\n",
        [(0, "let n = s.parse::<u32>().unwrap_or_else(|_| { 0 });")],
    ),
    # A span is credited to the line its first text is on, not the line the
    # terminator is on. This is what lets the caller look up the enclosing
    # function and the production/test classification at a sane line.
    (
        "a span is credited to its first line",
        "let n =\n    read();\n",
        [(0, "let n = read();")],
    ),
)


def self_test() -> int:
    """Check the source scanner against the literal forms that have broken it.

    Run with `--self-test`; the boot test runs it before the gate itself, so a
    parser regression is reported as a parser regression rather than as a
    suspiciously clean tree.
    """
    failures = 0
    for name, src, want in _PARSER_CASES:
        stripped = strip_noise(src)
        got = sorted(find_bodies(stripped))
        if got != want:
            failures += 1
            print(f"FAIL {name}: expected {want}, got {got}")
        # Every later step reports and slices by index, so blanking must never
        # move a byte or a line.
        if len(stripped) != len(src):
            failures += 1
            print(f"FAIL {name}: offsets not preserved ({len(stripped)} vs {len(src)})")
        if stripped.count("\n") != src.count("\n"):
            failures += 1
            print(f"FAIL {name}: line count not preserved")
        # The scanning is shared between the two modes, so every case above is
        # also an offset test for `keep_literals=True`. Its body list is not
        # checked: braces inside literals survive in that mode by design, so
        # `find_bodies` is not meaningful on its output.
        kept = strip_noise(src, keep_literals=True)
        if len(kept) != len(src) or kept.count("\n") != src.count("\n"):
            failures += 1
            print(f"FAIL {name}: offsets not preserved with keep_literals")

    for name, src, want in _KEEP_LITERAL_CASES:
        got = strip_noise(src, keep_literals=True)
        if got != want:
            failures += 1
            print(f"FAIL keep_literals/{name}:\n  expected {want!r}\n  got      {got!r}")
        if len(got) != len(src):
            failures += 1
            print(f"FAIL keep_literals/{name}: offsets not preserved")

    for name, src, start, col, want in _BLOCK_WALK_CASES:
        struct = strip_noise(src).split("\n")
        got = "\n".join(struct[k][a:b] for k, a, b in walk_block(struct, start, col))
        if got != want:
            failures += 1
            print(f"FAIL walk_block/{name}:\n  expected {want!r}\n  got      {got!r}")

    for name, src, want_spans in _STATEMENT_CASES:
        code = strip_noise(src, keep_literals=True).split("\n")
        struct = strip_noise(src).split("\n")
        got_spans = list(statements(code, struct))
        if got_spans != want_spans:
            failures += 1
            print(f"FAIL statements/{name}:\n  expected {want_spans!r}\n  got      {got_spans!r}")

    total = (
        len(_PARSER_CASES)
        + len(_KEEP_LITERAL_CASES)
        + len(_BLOCK_WALK_CASES)
        + len(_STATEMENT_CASES)
    )
    if failures:
        print(f"\n[parser self-test] {failures} failure(s) across {total} case(s)", file=sys.stderr)
        return 1
    print(f"[parser self-test] {total} case(s) OK", file=sys.stderr)
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    root = Path(__file__).resolve().parent.parent / "kernel" / "src"
    if not root.is_dir():
        print(f"error: no such directory: {root}", file=sys.stderr)
        return 2
    findings: list[str] = []
    files = 0
    for path in sorted(root.rglob("*.rs")):
        files += 1
        try:
            findings.extend(analyse(path))
        except (OSError, ValueError) as exc:  # keep going; report at the end
            print(f"warning: {path}: {exc}", file=sys.stderr)
    for f in findings:
        print(f)
    print(
        f"\nscanned {files} file(s); "
        f"{len(findings)} guard(s) held across a re-acquiring call",
        file=sys.stderr,
    )
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
