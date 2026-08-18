#!/usr/bin/env python3
"""Give each case of a long self-test its own stack frame.

    python scripts/split-frames.py --file kernel/src/eventlog.rs --fn self_test
    python scripts/split-frames.py --file ... --fn ... --dry-run
    python scripts/split-frames.py --file ... --fn ... --param u32_ptr:u64
    python scripts/split-frames.py --file kernel/src/kshell.rs --fn cmd_oci \
        --arms --param 'parts=&parts:&[&str]'
    python scripts/split-frames.py --check      # self-test the transformer

Why this exists
---------------
`scripts/stack-frames.py` reports which function claims the most stack.  Every
one of the worst offenders in this kernel has the same shape, and it is a shape
this project keeps producing: a self-test that grew, one independent case at a
time, into a single 3 500-line function whose cases are sibling `{ ... }`
blocks.

That is a stack bug at `opt-level = 0`, which is the profile `boot-test.sh`
builds and therefore the profile any canary halt comes from.  With no
optimisation there is no stack-slot colouring, so **every local of every case is
live for the whole function** even though the cases never overlap in time.
`self_test_prctl_dispatch` claimed **32 160 bytes** this way -- half of a 64 KiB
task stack -- for fixtures of which at most one was ever in use.

The fix is not to shrink the cases.  It is to stop them sharing a frame: wrap
each case body in its own `#[inline(never)]` nested `fn`, so the frames are
disjoint and the peak becomes `outer + max(case)` instead of `sum(cases)`.  On
`self_test_prctl_dispatch` that was 32 160 -> 944 bytes, with the largest case
at 3 120, i.e. a ~4 KiB peak instead of a ~32 KiB one.  It costs nothing at
runtime: the calls are not in any hot path, and `#[inline(never)]` is what stops
a release build from undoing the split.

What it will and will not do
----------------------------
It only rewrites **bare sibling blocks** -- `{ ... }` used as a statement -- in a
function returning `KernelResult<()>`.  Each becomes:

    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            <body>
            Ok(())
        }
        case()?;
    }

unless the body neither returns nor uses `?`, in which case it gets the
infallible shape -- `fn case() { <body> }` and `case();` -- because a
`KernelResult` nothing can fail to produce is a `clippy::unnecessary_wraps`
warning per case.

Nested `fn` items cannot capture, so a case that reads an outer local is a
compile error rather than a silent miscompile -- which is the property that
makes this safe to apply mechanically.  Each `case` is scoped to its own block,
so they need no unique names.  `return Err(..)` inside a case keeps its old
meaning: it aborted the whole self-test before, and `case()?` aborts it now --
but any *other* `return` would not, so a case containing one is refused rather
than rewritten.

A case that reads a fixture the cases *share* -- `self_test_remap_ioprio_futex2`
keeps one futex word for all of them -- is `E0434` unless you name the fixture
with `--param NAME:TYPE`, which threads it through as an argument to the cases
that mention it.  Pass cheap handles only: a fixture passed by value is copied
into the case frame, which is the cost this tool exists to remove.

`--arms` handles the other shape this kernel produces: a kshell dispatcher,
whose cases are the arms of one `match cmd { "inspect" => { ... }, ... }`.  The
arm's own braces are kept -- they carry the `pat =>` and the trailing `,` -- and
only its body moves into the `fn`.  Its safety argument is not the same one:
arms reject bad arguments with a bare `return;`, which means *leave the
command*, and inside a nested `fn` it would mean *leave this case*.  Those
coincide exactly when there is nothing after the match to fall back into, so
`--arms` requires a unit return type and a match in tail position, and asserts
both rather than trusting them.

`--flat [MIN_LINES]` handles the third shape: a **flat** body, a long run of
`let a = ..; if dispatch(..) {..}` with no block structure at all
(`eventlog::self_test`, `self_test_sysv_ipc_mqueue`).  An earlier note put
these out of scope, on the grounds that splitting one means inventing the case
boundaries -- a decision about what the test's units are, which is not a
transformer's to make.  That was right about the principle and wrong about the
facts: the boundaries are already there.  The author of such a body separates
each case with a blank line and heads it with `// Test 7: ...`; the structure
is simply not written in *braces*, which is all the default mode can see.  So
`--flat` cuts at the blank lines and nowhere else, and each paragraph becomes a
case if it is at least MIN_LINES long, reads no local bound before it, and
binds nothing read after it.  The last two are `E0434` and `E0425` -- the
compiler is the actual safety net, and these checks only save it the trouble.

It refuses to write a **one-case** split, because that cannot help: the peak
becomes `outer + case`, which is what it already was.  Two is the minimum at
which `max` beats `sum`.

Then measure with `stack-frames.py --peak`, not the ranking
-------------------------------------------------------------
A split is only a win if `outer + deepest case` falls.  The per-symbol ranking
reports the shrunken outer frame and lists the cases separately, so it flatters
a split that achieved nothing: it called a one-case split of
`linux_fd::self_test` 28 224 -> 18 896, when the real peak went 28 224 -> 28 240.
The usual cause is coverage -- if the cases span 300 lines of a 3 000-line
function, the peak is still set by what the outer frame kept, and the split
should be reverted rather than kept for the look of it.

Safety of the rewrite itself
----------------------------
Braces are counted by a scanner that understands line comments, *nesting* block
comments, char literals, byte/raw strings and escapes, so a `{` in a string
never moves the depth.  Re-indenting can only corrupt a literal that spans a
newline verbatim, so every literal crossing into the rewritten region is checked
to be either single-line or backslash-continued (Rust strips the following
line's indent in that case).  Line-count arithmetic is asserted exactly.

`--check` runs a regression suite of synthetic inputs, one per trap this
transformer has actually fallen into against real kernel source: the wrapped-`if`
brace, the `} else {` close, a `}` inside a comment, braces inside strings and
nested block comments, and a multi-line raw string (which it must refuse).
"""

from __future__ import annotations

import argparse
import io
import re
import sys

RET_TYPE = "crate::error::KernelResult<()>"

RETURN_RE = re.compile(r"\breturn\b")
RETURN_NOT_ERR = re.compile(r"\breturn\b(?!\s+Err\b)")
# `-> !` on a signature, i.e. a diverging function: it never returns, so it is
# unit-like for splitting purposes and cannot contain `return` or `?` at all.
DIVERGING = re.compile(r"->\s*!\s*(?:\{|$)")


# --------------------------------------------------------------------------
# Rust-aware brace scanner
# --------------------------------------------------------------------------
def scan(text: str) -> tuple[list[int], bytearray, list[tuple[int, int, bool]]]:
    """Return per-character brace depth (before the char), a code mask, and
    literal spans.

    Comments and literals are skipped, so braces inside them never move the
    depth -- but a caller looking for a specific brace *character* must also
    know it is real, which is what the mask is for: without it, the `}` in a
    comment such as `sched::{set,copy}_task_name` is mistaken for a block's
    close.  Literal spans come back as `(start, end, is_raw)` so the caller can
    check that re-indenting cannot change any literal's value.
    """
    n = len(text)
    depth = [0] * (n + 1)
    code = bytearray(n + 1)
    lits: list[tuple[int, int, bool]] = []
    d = 0
    i = 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            j = _skip_block_comment(text, i)
        elif c in "rb" and _raw_start(text, i):
            j = _skip_raw(text, i)
            lits.append((i, j, True))
        elif c == '"':
            j = _skip_str(text, i)
            lits.append((i, j, False))
        elif c == "'" and _is_char_lit(text, i):
            j = _skip_char(text, i)
        else:
            depth[i] = d
            code[i] = 1
            if c == "{":
                d += 1
            elif c == "}":
                d -= 1
            i += 1
            continue
        for k in range(i, j):
            depth[k] = d
        i = j
    depth[n] = d
    assert d == 0, f"unbalanced braces: final depth {d}"
    return depth, code, lits


def _skip_block_comment(text: str, i: int) -> int:
    level = 0
    j = i
    n = len(text)
    while j < n:
        if text.startswith("/*", j):
            level += 1
            j += 2
        elif text.startswith("*/", j):
            level -= 1
            j += 2
            if level == 0:
                return j
        else:
            j += 1
    raise AssertionError("unterminated block comment")


def _raw_start(text: str, i: int) -> bool:
    j = i + 1 if text[i] == "b" else i
    if j >= len(text) or text[j] != "r":
        return False
    j += 1
    while j < len(text) and text[j] == "#":
        j += 1
    return j < len(text) and text[j] == '"'


def _skip_raw(text: str, i: int) -> int:
    j = i + 1 if text[i] == "b" else i
    assert text[j] == "r"
    j += 1
    hashes = 0
    while text[j] == "#":
        hashes += 1
        j += 1
    assert text[j] == '"'
    close = '"' + "#" * hashes
    k = text.find(close, j + 1)
    assert k > 0, "unterminated raw string"
    return k + len(close)


def _skip_str(text: str, i: int) -> int:
    j = i + 1
    while j < len(text):
        if text[j] == "\\":
            j += 2
            continue
        if text[j] == '"':
            return j + 1
        j += 1
    raise AssertionError("unterminated string")


def _is_char_lit(text: str, i: int) -> bool:
    # Tell 'a' / '\n' from a lifetime such as 'static.
    j = i + 1
    if j < len(text) and text[j] == "\\":
        k = text.find("'", j)
        return 0 < k < j + 8
    return j + 1 < len(text) and text[j + 1] == "'"


def _skip_char(text: str, i: int) -> int:
    j = i + 1
    if text[j] == "\\":
        j += 1
        while text[j] != "'":
            j += 1
        return j + 1
    return i + 3


# --------------------------------------------------------------------------
# The rewrite
# --------------------------------------------------------------------------
def line_starts(lines: list[str]) -> list[int]:
    starts = [0]
    for ln in lines:
        starts.append(starts[-1] + len(ln) + 1)
    return starts


def find_fn(lines: list[str], name: str, at: int = 0) -> int:
    """Return the 0-based index of the `fn <name>(` definition line.

    `at` (a 1-based line number) picks one when the name is not unique, which
    a second pass over an already-split file needs: this tool names every case
    it generates `case`, so `kshell.rs` holds 78 of them and the biggest frame
    left in it is one particular `fn case`.  Requiring the exact line rather
    than an ordinal keeps the reference stable against edits above it failing
    loudly instead of silently selecting a different function.
    """
    pat = re.compile(
        r"^(?:pub\s*(?:\([^)]*\)\s*)?)?(?:const\s+)?(?:unsafe\s+)?"
        r'(?:extern\s+"[^"]*"\s+)?fn\s+' + re.escape(name) + r"\s*[(<]"
    )
    hits = [i for i, ln in enumerate(lines) if pat.match(ln.lstrip())]
    assert hits, f"no `fn {name}` found"
    if at:
        assert at - 1 in hits, (
            f"line {at} is not a `fn {name}` definition; found at "
            f"{[h + 1 for h in hits]}"
        )
        return at - 1
    assert len(hits) == 1, (
        f"`fn {name}` is ambiguous: lines {[h + 1 for h in hits]}; "
        f"pass --at LINE to choose one"
    )
    return hits[0]


def body_open(lines: list[str], fn_idx: int) -> int:
    """Index of the line carrying the fn body's opening brace."""
    for i in range(fn_idx, min(fn_idx + 12, len(lines))):
        if lines[i].rstrip().endswith("{"):
            return i
    raise AssertionError(f"no body brace within 12 lines of {fn_idx + 1}")


def container(
    lines: list[str],
    depth: list[int],
    code: bytearray,
    starts: list[int],
    open_idx: int,
    text: str,
    flat: bool = False,
) -> int:
    """Where the cases actually live.

    Some of these functions (`self_test_prctl_dispatch` among them) wrap their
    entire body in one further `{ ... }`, so the cases are the *grand*children
    of the signature.  Taken literally, the one wrapper would be reported as the
    single case and the rewrite would gain nothing.

    A lone bare block is genuinely ambiguous -- it is either such a wrapper or a
    single real case -- and the tell is not its size but its contents: a wrapper
    holds sibling cases, a case does not.  So descend only when the block holds
    at least two cases of its own.  (Size was the first rule here; it is an
    arbitrary constant that misjudges any function short enough for its one case
    to be most of it.)

    What counts as "cases of its own" depends on which shape is being looked
    for, which is why `flat` is a parameter rather than a detail of the caller.
    `self_test_sysv_ipc_mqueue` wraps 1 874 flat lines in one bare block that
    contains no bare block at all, so the brace test says "a single real case"
    and refuses to descend -- and `--flat` then sees one statement where there
    are 96 paragraphs.  Only one level is descended, in either mode: no function
    here nests two wrappers, and each extra level is another chance to guess.
    """
    body_depth = _depth_inside(lines, depth, code, starts, open_idx)
    close = _matching_close(lines, depth, starts, open_idx, body_depth, text, code)
    bare = [
        i
        for i in range(open_idx + 1, close)
        if lines[i].strip() == "{" and depth[starts[i]] == body_depth
    ]
    if len(bare) == 1:
        inner_close = _matching_close(
            lines, depth, starts, bare[0], body_depth + 1, text, code
        )
        if _is_statement_block(lines, starts, text, code, bare[0], inner_close):
            if flat:
                inner_stmts = statements(
                    lines, depth, code, starts, text, bare[0], inner_close,
                    body_depth + 1,
                )
                nested = paragraphs(lines, inner_stmts)
            else:
                nested, _ = find_cases(
                    lines, depth, code, starts, text, bare[0], inner_close,
                    body_depth + 1,
                )
            if len(nested) >= 2:
                return bare[0]
    return open_idx


def find_cases(
    lines: list[str],
    depth: list[int],
    code: bytearray,
    starts: list[int],
    text: str,
    open_idx: int,
    close_idx: int,
    inner: int,
) -> tuple[list[tuple[int, int]], int]:
    """Bare statement blocks that are direct children of `open_idx`'s block.

    Returns them as `(open, close)` line indices plus a count of the `{` lines
    rejected -- an `if` with a wrapped condition, or a block closed by
    `} else {` -- which the caller reports so a silent skip is never silent.
    """
    cases: list[tuple[int, int]] = []
    skipped = 0
    i = open_idx + 1
    while i < close_idx:
        if lines[i].strip() == "{" and depth[starts[i]] == inner:
            j = _matching_close(lines, depth, starts, i, inner + 1, text, code)
            if _is_statement_block(lines, starts, text, code, i, j):
                cases.append((i, j))
            else:
                skipped += 1
            i = j + 1
            continue
        i += 1
    return cases, skipped


def _last_open_brace(
    lines: list[str], code: bytearray, starts: list[int], idx: int
) -> int:
    """Character offset of the last *real* `{` on line `idx`.

    A `{` the scanner swallowed as part of a literal or comment is not a
    brace, so the mask decides which one counts.
    """
    line, base = lines[idx], starts[idx]
    return base + max(k for k in range(len(line)) if line[k] == "{" and code[base + k])


def _masked(text: str, code: bytearray, lo: int, hi: int) -> str:
    """`text[lo:hi]` with comments and literals blanked to spaces.

    Offsets are preserved, so a match position in the result is a position in
    the original.
    """
    return "".join(text[k] if code[k] else " " for k in range(lo, hi))


def _depth_inside(
    lines: list[str],
    depth: list[int],
    code: bytearray,
    starts: list[int],
    idx: int,
) -> int:
    """Brace depth just inside the block whose `{` closes line `idx`.

    Taken from the scanner rather than by counting `{` textually, so a brace in
    a string on that line cannot throw it off.
    """
    # Depth is recorded *before* each char, so just inside the line's last `{`
    # it is one deeper.
    pos = _last_open_brace(lines, code, starts, idx)
    assert depth[pos + 1] == depth[pos] + 1, (
        f"line {idx + 1}: the last `{{` is inside a literal or comment"
    )
    return depth[pos + 1]


def _matching_close(
    lines: list[str],
    depth: list[int],
    starts: list[int],
    open_idx: int,
    inner: int,
    text: str | None = None,
    code: bytearray | None = None,
) -> int:
    """Line index of the `}` that closes the block opened on `open_idx`.

    When `text` is given this is exact: it walks characters to the brace where
    the depth returns to `inner - 1`.  Matching on `lines[j].strip() == "}"`
    alone is *not* safe -- a block closed by `} else {` would be skipped and a
    later, unrelated `}` returned instead, silently swallowing the else branch.
    """
    if text is not None:
        assert code is not None
        pos = _last_open_brace(lines, code, starts, open_idx)
        return _line_of(starts, _close_off(text, code, depth, pos, inner))
    for j in range(open_idx + 1, len(lines)):
        if lines[j].strip() == "}" and depth[starts[j]] == inner:
            return j
    raise AssertionError(f"no close for the block opened at line {open_idx + 1}")


def _close_off(
    text: str, code: bytearray, depth: list[int], open_off: int, inner: int
) -> int:
    """Character offset of the `}` closing the `{` at `open_off`."""
    for k in range(open_off + 1, len(text)):
        if code[k] and text[k] == "}" and depth[k] == inner:
            return k
    raise AssertionError(f"no close for the `{{` at offset {open_off}")


def _line_of(starts: list[int], off: int) -> int:
    lo, hi = 0, len(starts) - 1
    while lo < hi:
        mid = (lo + hi + 1) // 2
        if starts[mid] <= off:
            lo = mid
        else:
            hi = mid - 1
    return lo


def _is_statement_block(
    lines: list[str],
    starts: list[int],
    text: str,
    code: bytearray,
    open_idx: int,
    close_idx: int,
) -> bool:
    """Is this a bare `{ ... }` statement, rather than part of a larger form?

    Two ways to be fooled, both of which occur in this tree:

    * rustfmt puts the `{` of an `if` with a **wrapped condition** on its own
      line, so the opening line looks identical to a bare block's.  The tell is
      what precedes it: a statement block follows a *completed* statement, so
      the last code character before the `{` is `;`, `{` or `}` -- whereas the
      wrapped `if` leaves the `)` of its condition there.
    * the block may be closed by `} else {`, `};` or `},`, none of which is a
      bare block's close.  Require the `}` to end its line.

    The preceding character is found through the code mask rather than by
    reading back over lines, so a block introduced by a `/* ... */` comment --
    which a "does the previous line start with //" test rejects -- is still
    recognised.
    """
    if lines[close_idx].strip() != "}":
        return False
    pos = _last_open_brace(lines, code, starts, open_idx)
    for k in range(pos - 1, -1, -1):
        if code[k] and not text[k].isspace():
            return text[k] in ";{}"
    return False


def find_match(
    lines: list[str],
    depth: list[int],
    code: bytearray,
    starts: list[int],
    text: str,
    open_idx: int,
    close_idx: int,
    inner: int,
) -> tuple[int, int]:
    """The body's trailing `match`, as `(open line, close line)`.

    "Trailing" is not a stylistic preference, it is the whole safety argument
    for `--arms`.  A dispatcher's arms are full of bare `return;` -- the usual
    way to reject bad arguments -- and moving one into a nested `fn` turns
    "leave the command" into "leave this case".  Those are the same thing
    exactly when the match is the last statement of the function, because then
    there is nothing after the case for control to fall back into.  So the
    match is required to be last, and the check is on characters, not lines: a
    single statement after it would make every `return` in every arm a silent
    behaviour change.
    """
    hits = [
        i
        for i in range(open_idx + 1, close_idx)
        if depth[starts[i]] == inner
        and _masked(text, code, starts[i], starts[i + 1] - 1).strip().startswith("match")
        and lines[i].rstrip().endswith("{")
    ]
    assert hits, (
        "no `match ... {` at the top level of this function body; --arms only "
        "handles a function whose whole body is one dispatching match"
    )
    m_open = hits[-1]
    m_close = _matching_close(lines, depth, starts, m_open, inner + 1, text, code)
    after = _close_off(
        text, code, depth, _last_open_brace(lines, code, starts, m_open), inner + 1
    )
    tail = _masked(text, code, after + 1, starts[close_idx]).strip().strip(";")
    assert not tail, (
        f"line {m_close + 1}: the match is not the last statement of "
        f"`{lines[open_idx].strip()}` -- {tail[:60]!r} follows it, so a "
        f"`return` inside an arm would change meaning; split this by hand"
    )
    return m_open, m_close


def find_arm_cases(
    lines: list[str],
    depth: list[int],
    code: bytearray,
    starts: list[int],
    text: str,
    m_open: int,
    m_close: int,
    arm_depth: int,
) -> tuple[list[tuple[int, int]], int]:
    """Arms of the match at `m_open` whose body is a brace block.

    Found by walking to each `=>` that sits at the arm level -- nested matches
    put theirs deeper, so the depth test alone separates them -- and taking the
    next code character.  An arm whose body is an expression (`_ => println!(..)`)
    or another match (`"test" => match f() { .. }`) has no block to move and is
    counted as skipped rather than mangled.

    Unlike a bare statement block, an arm's braces are *not* regenerated: the
    `"inspect" =>` that precedes the `{`, and the `,` that may follow the `}`,
    are part of the match and must survive verbatim.  So an arm is only usable
    when its `{` ends its line and its `}` owns one, which is what rustfmt
    produces for any arm big enough to be worth splitting.
    """
    cases: list[tuple[int, int]] = []
    skipped = 0
    k = _last_open_brace(lines, code, starts, m_open) + 1
    hi = starts[m_close]
    while k < hi:
        if not (code[k] and text[k] == "=" and text[k + 1 : k + 2] == ">"):
            k += 1
            continue
        if depth[k] != arm_depth:
            k += 1
            continue
        j = k + 2
        while j < hi and not (code[j] and not text[j].isspace()):
            j += 1
        if j >= hi or text[j] != "{":
            skipped += 1
            k = j
            continue
        c = _close_off(text, code, depth, j, arm_depth + 1)
        o_line, c_line = _line_of(starts, j), _line_of(starts, c)
        if _arm_block_ok(lines, code, starts, text, j, o_line, c_line):
            cases.append((o_line, c_line))
        else:
            skipped += 1
        k = c + 1
    return cases, skipped


def _arm_block_ok(
    lines: list[str],
    code: bytearray,
    starts: list[int],
    text: str,
    open_off: int,
    o_line: int,
    c_line: int,
) -> bool:
    """Can this arm's block be wrapped without touching its own braces?"""
    if c_line <= o_line:
        return False
    if lines[c_line].strip() not in ("}", "},"):
        return False
    return not _masked(text, code, open_off + 1, starts[o_line + 1] - 1).strip()


IDENT = re.compile(r"[A-Za-z_]\w*")
LET = re.compile(r"\blet\b")
NOT_A_BINDING = {"mut", "ref", "box"}


def statements(
    lines: list[str],
    depth: list[int],
    code: bytearray,
    starts: list[int],
    text: str,
    open_idx: int,
    close_idx: int,
    body: int,
) -> list[tuple[int, int]]:
    """Top-level statements of a block, as inclusive line ranges.

    A statement ends at a `;` that leaves the depth at `body`, or at the `}`
    that closes a block form (`if`, `match`, `for`, `unsafe`) used as a
    statement.  Each range starts at the line after the previous statement's
    end, so a statement absorbs the blank lines and the `// Test 4:` comment
    above it -- which is what keeps a split readable.

    Anything after the last statement (typically the trailing `Ok(())`) is not
    returned, and is left where it is.
    """
    ends: list[int] = []
    lo, hi = starts[open_idx + 1], starts[close_idx]
    for p in range(lo, hi):
        if not code[p]:
            continue
        c = text[p]
        if not ((c == ";" and depth[p] == body) or (c == "}" and depth[p] == body + 1)):
            continue
        e = _line_of(starts, p)
        # The statement must end its line; one that shares a line with the next
        # is not something this can cut between.
        if _masked(text, code, p + 1, starts[e + 1] - 1).strip():
            continue
        ends.append(e)
    out = []
    prev = open_idx
    for e in ends:
        out.append((prev + 1, e))
        prev = e
    return out


def _let_patterns(masked: str) -> list[tuple[int, int]]:
    """Offsets of each `let` **pattern** -- what is between `let` and `=`/`:`/`;`.

    Both `_binds` and `_reads` need this same span, and for opposite reasons:
    the names in it are introduced, and are therefore *not* mentions of an
    outer name.  Sharing one scan is what keeps the two answers consistent.
    """
    spans: list[tuple[int, int]] = []
    for m in LET.finditer(masked):
        d, i = 0, m.end()
        while i < len(masked):
            ch = masked[i]
            if ch in "([{":
                d += 1
            elif ch in ")]}":
                d -= 1
            elif d == 0 and ch in "=;:":
                break
            i += 1
        spans.append((m.end(), i))
    return spans


ITEM_DECL = re.compile(
    r"\b(?:const|static)\s+(?:mut\s+)?([A-Za-z_]\w*)"
    r"|\b(?:fn|struct|enum|union|trait|type|mod)\s+([A-Za-z_]\w*)"
)
USE_DECL = re.compile(r"\buse\s+([^;]*);")


def _use_names(spec: str) -> set[str]:
    """The names a `use` brings into scope: the last segment, or the alias."""
    out: set[str] = set()
    if "{" in spec:
        inner = spec[spec.index("{") + 1 : spec.rindex("}")] if "}" in spec else ""
        parts, d, cur = [], 0, []
        for ch in inner:
            if ch in "{(":
                d += 1
            elif ch in "})":
                d -= 1
            if ch == "," and d == 0:
                parts.append("".join(cur))
                cur = []
            else:
                cur.append(ch)
        parts.append("".join(cur))
    else:
        parts = [spec]
    for p in parts:
        p = p.strip()
        if not p or "*" in p:
            continue
        if " as " in p:
            p = p.rsplit(" as ", 1)[1]
        name = p.split("::")[-1].strip()
        if IDENT.fullmatch(name):
            out.add(name)
    return out


def _item_binds(masked: str) -> set[str]:
    """Names an *item* declaration in a body introduces.

    A `const`, `static`, `fn`, `struct` or `use` written inside a function body
    is scoped to the block that holds it, and moving that block under a
    generated `fn case` takes the name out of every other case's scope.  Unlike
    a local this is not a capture problem -- a nested `fn` may freely name an
    item from an enclosing block -- so it never shows up as `E0434`; it shows
    up as `E0425` in some *later* case, which is how `kernel_main` failed:
    `const HELLO_ELF: &[u8] = include_bytes!(..)` sat in one paragraph and was
    written to the filesystem three paragraphs further down.

    These names are deliberately not filtered by capitalisation the way `let`
    patterns are: `SCREAMING_CASE` is the convention for exactly the `const`s
    that matter here.
    """
    out = {m.group(1) or m.group(2) for m in ITEM_DECL.finditer(masked)}
    for m in USE_DECL.finditer(masked):
        out |= _use_names(m.group(1))
    return {n for n in out if n}


def _value_idents(masked: str):
    """Identifier occurrences that could be a *local* being read.

    Three shapes are spelled exactly like a local and can never be one.  Each
    was found the same way -- by a cluster that would not cut where it plainly
    should have -- and each is a question about the characters *around* the
    name, not about the name:

    * `x.name` is a field or a method.  `cmd_oci`'s inspect arm never touches
      the outer `cmd` but prints `image.config.cmd` five times, so a bare
      `\\bcmd\\b` threaded an argument nothing used, one clippy warning per
      arm.  The `.` must be a lone one: `&buf[start..cmd]` is a read.
    * `name::init` is a path qualifier -- a module, a type, an enum.  A local
      can never be followed by `::`, and this is the one that mattered most:
      `kernel_main` opens with `if let Some(ref fb) = boot_info.framebuffer`,
      binding an `fb` that dies two lines later, and then calls `fb::init()`
      and `fb::self_test()` 5 000 lines apart.  Reading those as uses of the
      local made the binding live across 395 of the body's 477 paragraphs and
      fused them into a single 17 KiB case -- a split that saved 5%.
    * `core::name` is the same thing seen from the other side.

    Excluding them is not a heuristic that trades safety for yield, which
    matters because under-approximating reads is the one direction that can
    produce a wrong boundary rather than a missed one: none of the three *can*
    be a local, so nothing real is being dropped.
    """
    for m in IDENT.finditer(masked):
        k = m.start() - 1
        while k >= 0 and masked[k].isspace():
            k -= 1
        if k >= 0 and masked[k] == "." and not (k and masked[k - 1] == "."):
            continue  # `x.name` -- a field or method
        if k >= 1 and masked[k] == ":" and masked[k - 1] == ":":
            continue  # `core::name` -- a path segment
        j = m.end()
        while j < len(masked) and masked[j].isspace():
            j += 1
        if masked[j : j + 2] == "::":
            continue  # `name::init` -- a path qualifier
        yield m


def _binds(masked: str) -> set[str]:
    """Names a statement's `let`s introduce -- over-approximated on purpose.

    Over-approximating *bindings* only ever makes the chunker more cautious:
    a spurious name is one more thing that must not cross a boundary.  Under-
    approximating would let a real one cross, so the two directions are not
    symmetric and this leans the safe way.

    Uppercase-initial identifiers are dropped because `let Some(ev) = ..` and
    `let Foo { a } = ..` would otherwise put `Some` and `Foo` in the set, and
    every later statement mentioning them would become unchunkable.  Rust's
    naming convention makes that reliable, and where it is not, the compiler
    still is.
    """
    out: set[str] = set()
    for lo, hi in _let_patterns(masked):
        for m in _value_idents(masked[lo:hi]):
            name = m.group(0)
            if name not in NOT_A_BINDING and not name[:1].isupper():
                out.add(name)
    return out


def _binding_takes_effect(masked: str, hi: int) -> int:
    """Offset at which the binding of a `let` pattern ending at `hi` is in scope.

    Which is the end of its initialiser, not the end of its pattern -- the
    distinction Rust makes to give `let a = a + 1;` the *outer* `a` on the
    right.  So: the statement's `;`, or the `{` of an `if let`/`while let`
    body, whichever comes first at depth zero.
    """
    d = 0
    for i in range(hi, len(masked)):
        ch = masked[i]
        if ch in "([":
            d += 1
        elif ch in ")]":
            d -= 1
        elif d <= 0 and ch in ";{":
            return i
    return len(masked)


def _reads(masked: str) -> set[str]:
    """Names a statement mentions that must already be in scope around it.

    The distinction is the whole ballgame for `--flat`, and it is a question
    about *position*, not just about names:

    * `let a = SyscallArgs { .. };` mentions `a`, but declares it.  Counting
      that as a use makes every paragraph after the first look like it reads
      the previous paragraph's `a`, which rejected 8 of the 9 paragraphs of
      `self_test_sysv_ipc_mqueue` -- every one of its cases opens with exactly
      that line.
    * `if let Some(ev) = result.events.first() { ev.namespace_str() }` mentions
      `ev` twice, and the second is the binding the first made.  Judging by
      name alone made `eventlog::self_test` look as though Test 8 read Test 2's
      `ev`, which welded nine paragraphs into two clusters.
    * `let a = a + 1;` mentions `a` twice as well -- and there the second *is*
      an outer read, because a binding is not in scope until its initialiser
      has been evaluated.

    All three fall out of one rule: a mention is an outer read when it stands
    before the point where a `let` in this statement brings that name into
    scope.  The pattern text itself is blanked, so a name appearing only there
    is not a read at all.
    """
    spans = _let_patterns(masked)
    in_scope: dict[str, int] = {}
    blanked = masked
    for lo, hi in spans:
        at = _binding_takes_effect(masked, hi)
        for m in _value_idents(masked[lo:hi]):
            name = m.group(0)
            if name not in NOT_A_BINDING and not name[:1].isupper():
                in_scope[name] = min(in_scope.get(name, at), at)
        blanked = blanked[:lo] + " " * (hi - lo) + blanked[hi:]
    return {
        m.group(0)
        for m in _value_idents(blanked)
        if m.start() < in_scope.get(m.group(0), len(masked) + 1)
    }


def _first_code_line(lines: list[str], span: tuple[int, int]) -> int:
    """The first non-blank line of a statement range.

    A range begins where the previous one ended, so it opens with whatever
    blank lines separated them.  Those stay outside the case; the comment that
    heads a statement does not, because it describes the statement.
    """
    top = span[0]
    while top < span[1] and not lines[top].strip():
        top += 1
    return top


def paragraphs(
    lines: list[str], stmts: list[tuple[int, int]]
) -> list[list[tuple[int, int]]]:
    """Group top-level statements the way the author already grouped them.

    A blank line between two statements starts a new group.  This is the whole
    idea behind `--flat`, and the reason it is not the "inventing case
    boundaries" that the earlier note refused to do: in a body like
    `eventlog::self_test` the cases are already marked out -- a blank line and
    a `// Test 7: ...` header before each -- they simply are not marked out
    with *braces*, which is all `find_cases` can see.  Reading the blank lines
    recovers the author's own structure rather than imposing one.

    A blank line *inside* a statement (in the middle of a long `if` body) is
    not a boundary, because only lines above a statement's first line of code
    are considered.
    """
    groups: list[list[tuple[int, int]]] = []
    for k, s in enumerate(stmts):
        blank_above = any(
            not lines[t].strip() for t in range(s[0], _first_code_line(lines, s))
        )
        if k == 0 or blank_above:
            groups.append([])
        groups[-1].append(s)
    return groups


def find_flat_cases(
    lines: list[str],
    depth: list[int],
    code: bytearray,
    starts: list[int],
    text: str,
    open_idx: int,
    close_idx: int,
    body: int,
    min_lines: int,
    outer: set[str],
    declared: set[str],
    reserve_tail: bool = False,
) -> tuple[list[tuple[int, int]], int]:
    """Turn each blank-line-separated paragraph of a flat body into a case.

    `eventlog::self_test` and its kind have no block structure for
    `find_cases` to exploit -- a few hundred statements in a row -- but they do
    have paragraphs, so `paragraphs` supplies the boundaries and this decides
    which of them are legal and worth taking.  A paragraph is taken when:

    * it is at least `min_lines` long (a two-line case moves nothing);
    * it reads no local bound before it -- that would be `E0434`, since a
      nested `fn` cannot capture.  `--param` exempts a name by threading it
      through as an argument, which is what `declared` holds;
    * nothing it binds is read after it -- that would be `E0425`.

    Both compiler errors are the safety net rather than the mechanism: getting
    a boundary wrong fails the build, it does not miscompile.  The checks here
    exist so the common cases do not have to fail the build first.

    **Shadowing has to be modelled or nothing splits.**  These tests rebind one
    scratch name per case -- `let a = SyscallArgs { .. };` appears 96 times in
    `self_test_sysv_ipc_mqueue` -- so a rule that asks only "is the name
    mentioned later" answers yes every time and skips every paragraph, which is
    what the first version did (9 skipped, 0 taken).  A paragraph's *reads* are
    therefore its identifiers minus the ones its own `let`s introduce before
    that point (see `_reads`), and a binding stops being live at the paragraph
    that rebinds it without reading it first.

    **Items are not locals.**  A `const`, `static`, `fn` or `use` written inside
    the body is scoped to its *block*, wherever in that block it stands, and a
    nested `fn` may freely name one.  So a case that *reads* an item is fine
    however far it is from the declaration, above it or below it; what is not
    fine is casing the paragraph that *declares* it, which would re-scope the
    item into that case body and cost every other case -- and the tail -- the
    ability to name it.  That failure is not the `E0434` that guards locals; it
    surfaces as `E0425` somewhere else entirely, which is how it was found in
    `kernel_main`.  A declaring paragraph is therefore held back from being a
    case and left in the outer body, which is the whole of the item rule.

    Note what is *not* checked: whether the split is worth doing.  A single
    case is always a no-op -- the peak becomes `outer + case`, which is what it
    already was -- so that judgement lives in `apply`, which refuses to write
    one, and ultimately in `stack-frames.py --peak`.
    """
    stmts = statements(lines, depth, code, starts, text, open_idx, close_idx, body)
    if not stmts:
        return [], 0
    groups = paragraphs(lines, stmts)

    masked = {s: _masked(text, code, starts[s[0]], starts[s[1] + 1] - 1) for s in stmts}
    binds = {s: _binds(masked[s]) for s in stmts}
    uses = {s: _reads(masked[s]) for s in stmts}
    tail = _masked(text, code, starts[stmts[-1][1] + 1], starts[close_idx + 1] - 1)
    tail_uses = _reads(tail)

    if reserve_tail and len(groups) > 1:
        # A `-> !` body's last statement stands in tail position and has to
        # diverge there; `{ fn case() { .. } case(); }` evaluates to `()`, which
        # is `error[E0308]: expected !, found ()` -- exactly what `kernel_main`
        # produced at main.rs:5961 on the first attempt.  The paragraph could be
        # made a case by giving the generated `fn` a `-> !` of its own, but that
        # is a second signature shape for one paragraph of one function; folding
        # it into the tail instead costs the last case and nothing else.  Its
        # reads join `tail_uses` so the locals it needs still count as live.
        for s in groups.pop():
            tail_uses |= uses[s]

    # A group's reads are what it mentions before its own `let`s introduce it,
    # walking its statements in order so `foo(x); let x = ..;` still counts as
    # reading the outer `x`.
    g_binds: list[set[str]] = []
    g_reads: list[set[str]] = []
    for g in groups:
        reads, seen = set(), set()
        for s in g:
            reads |= uses[s] - seen
            seen |= binds[s]
        g_binds.append(seen)
        g_reads.append(reads)

    # Which paragraphs *declare* a block-scoped item.
    #
    # An item is not a local and needs no weld: items in a block are scoped to
    # the whole block regardless of where they stand, and a nested `fn` may name
    # one freely.  Both halves are checked, not assumed -- this compiles and
    # prints 123:
    #
    #     fn outer() -> u32 {
    #         fn a() -> u32 { BLOB.len() as u32 }   // *above* the declaration
    #         let t = a();
    #         const BLOB: &[u8] = b"xyz";
    #         fn b() -> u32 { BLOB[0] as u32 }
    #         t + b()
    #     }
    #
    # So the only thing that must not happen is the declaration being *moved
    # into* a case, which would re-scope it to that case's body and leave every
    # other case unable to name it.  Refusing to case the declaring paragraph is
    # therefore the whole constraint: the item stays in the outer body, where it
    # was already visible to everything, and every other paragraph is free.
    #
    # This used to fuse every paragraph from an item's first mention to its last
    # into one super-paragraph, which is sound but far stronger than Rust
    # requires -- and expensive, because the fused run then has to clear the
    # min-length and escape tests as a unit.  One `const` near the top of a body
    # welded 637 lines of `self_test_seccomp_ptrace_clone3`'s futex case into a
    # single case, and a single case is arithmetically a no-op: its peak is
    # `outer + case`, which is what it already was.
    g_items: list[set[str]] = []
    for g in groups:
        it: set[str] = set()
        for s in g:
            it |= _item_binds(masked[s])
        g_items.append(it)

    def last_reader(k: int, name: str) -> int:
        """Index of the last group that can still see group `k`'s `name`.

        `k` itself if the binding dies inside `k`; `len(groups) - 1` if it
        survives to the trailing `Ok(())`, which is a boundary no merge can
        absorb and is therefore left for the escape test to reject.
        """
        out = k
        for j in range(k + 1, len(groups)):
            if name in g_reads[j]:
                out = j
            if name in g_binds[j]:
                return out  # rebound, so group k's binding dies here
        return len(groups) - 1 if name in tail_uses else out

    # Coalesce paragraphs whose locals reach into each other.
    #
    # A crossing local does not mean "unsplittable", it means the blank line
    # was in the wrong place: `self_test_sysv_ipc_mqueue` declares
    # `let sops_ptr = ..` in one paragraph and dereferences it in the next, so
    # the two are one case that happens to be written with a gap in it.  Taking
    # the paragraph as final gave 3 cases of 9; merging along the crossings
    # gives 6, and the ones that merge are exactly the ones that had to.
    #
    # This is the partition-labels walk: extend the cluster to the furthest
    # group any of its bindings reaches, re-extending as new groups join, and
    # cut where nothing outstanding is still live.
    #
    # The one thing the walk must not do is swallow the whole body.  A local
    # the author declared at the top and used at the bottom -- `let saved =
    # total(); .. restore(saved);` -- links the first group to the last, and
    # the merge would then produce a single case containing everything, whose
    # peak is `outer + case` and so is not a saving at all.  When that happens
    # the answer is not to merge but to leave that first paragraph where it is:
    # it holds a genuine body-scope local, which belongs in the outer frame.
    def grow(k: int) -> int:
        end, j = k, k
        while j <= end:
            for name in g_binds[j]:
                end = max(end, last_reader(j, name))
            j += 1
        return end

    clusters: list[list[int]] = []
    leading_skipped = 0
    leading_binds: set[str] = set()
    k = 0
    while k < len(groups):
        end = grow(k)
        if k == 0 and end == len(groups) - 1 and end > 0:
            g = groups[0]
            if g[-1][1] - _first_code_line(lines, g[0]) + 1 >= min_lines:
                leading_skipped += 1
            # Dropping the group from `clusters` is only half of leaving it
            # where it is.  Its `let`s are now *outer* locals -- that is what
            # "left in the outer frame" means -- so they have to join `before`,
            # or every cluster below is judged as though the fixture paragraph
            # had never existed and is free to read it.  Skipping that step is
            # not caught by anything here; it surfaces as a pile of `E0434`s at
            # the end of a six-minute build.  `self_test_remap_ioprio_futex2`
            # opens with eleven such `let`s and produced 36 of them.
            leading_binds |= g_binds[0]
            k = 1
            continue
        clusters.append(list(range(k, end + 1)))
        k = end + 1

    cases: list[tuple[int, int]] = []
    skipped = leading_skipped
    before = (set(outer) | leading_binds) - declared
    for cl in clusters:
        c_binds: set[str] = set()
        c_reads: set[str] = set()
        c_items: set[str] = set()
        for k in cl:
            c_reads |= g_reads[k] - c_binds
            c_binds |= g_binds[k]
            c_items |= g_items[k]
        top = _first_code_line(lines, groups[cl[0]][0])
        end = groups[cl[-1]][-1][1]
        long_enough = end - top + 1 >= min_lines
        # Nothing bound in the cluster can be read after it: the walk above
        # only stops where that is true, except at the tail, which it cannot
        # extend past.
        #
        # `c_items` is the separate, unconditional bar: a cluster that *declares*
        # a block-scoped item cannot become a case at all, because wrapping it
        # would re-scope the item into the case body and every other case --
        # and the tail -- would stop being able to name it.  Left in the outer
        # body it stays visible to all of them.  Unconditional rather than
        # "escapes into the tail", because a case three paragraphs *above* the
        # declaration may name it just as legally as one below.
        escapes = (
            any(n in tail_uses for n in c_binds) and cl[-1] == len(groups) - 1
        ) or bool(c_items)
        if long_enough and not (c_reads & before) and not escapes:
            cases.append((top, end))
        elif long_enough:
            skipped += 1
        before |= c_binds - declared
    return cases, skipped


def _sig_params(sig: str) -> set[str]:
    """Parameter names of a signature, so a chunk reading one is not attempted."""
    a, b = sig.find("("), sig.rfind(")")
    if a < 0 or b < a:
        return set()
    out: set[str] = set()
    for part in sig[a + 1 : b].split(","):
        name = part.split(":")[0].strip().lstrip("&").strip()
        if IDENT.fullmatch(name) and not name[:1].isupper():
            out.add(name)
    return out


def _needs_result(text: str, code: bytearray, lo: int, hi: int) -> bool:
    """Does this case body actually use `?` or `return`?

    A case that only calls `serial_println!` and asserts inline never produces
    an error, so giving it `-> KernelResult<()>` earns a
    `clippy::unnecessary_wraps` warning for every case -- eight of them in
    `httpd::self_test` alone.  Such a case gets a plain `fn case()` and a
    `case();` call instead.

    Deliberately conservative: comments and literals are masked out (so a `?`
    in a message does not count), but anything that survives that counts.  A
    false positive merely reproduces today's warning; a false negative is a
    compile error the build catches immediately.
    """
    masked = _masked(text, code, lo, hi)
    return "?" in masked or RETURN_RE.search(masked) is not None


def _check_unit_returns(
    text: str, code: bytearray, starts: list[int], lo: int, hi: int
) -> None:
    """Refuse a case of a unit-returning function that contains any `return`.

    The `KernelResult` shape has an escape hatch -- `case()?` propagates a
    `return Err(..)` unchanged -- and a unit function has none: a `return;`
    that used to abandon the whole function would come to abandon only the
    case, and every statement it was skipping would run.  There is no call
    spelling that fixes that, so the paragraph cannot be split.

    `--arms` is the exception that proves it: an arm's `return;` is faithful
    only because `find_match` has already established the match is in tail
    position, where returning from the case and falling out of it are the same
    thing.  A paragraph in the middle of a body has no such guarantee.
    """
    masked = _masked(text, code, lo, hi)
    m = RETURN_RE.search(masked)
    assert m is None, (
        f"line {_line_of(starts, lo + m.start()) + 1}: a case of a "
        f"unit-returning function contains a `return`, which would abandon "
        f"only the case once it becomes a call; split this one by hand"
    )
    assert "?" not in masked, (
        f"a case of a unit-returning function contains `?`, which needs a "
        f"`Try` return type it cannot be given here"
    )


def _check_returns(
    text: str, code: bytearray, starts: list[int], lo: int, hi: int
) -> None:
    """Refuse a case whose `return` would change meaning.

    `case()?` reproduces `return Err(..)` exactly: both abandon the whole
    self-test at that point.  It does *not* reproduce `return Ok(())`, which
    used to end the self-test and would now end only the case, silently letting
    every later case run.  Nothing else about this rewrite can change behaviour,
    so this is the one thing worth refusing over -- and it must be a refusal,
    not a skip, because the alternative is a self-test that quietly stops
    meaning what it says.

    A `return` inside a closure nested in the case is caught too, even though it
    is harmless.  Declining to split is cheap; being subtly wrong is not.
    """
    masked = _masked(text, code, lo, hi)
    m = RETURN_NOT_ERR.search(masked)
    assert m is None, (
        f"line {_line_of(starts, lo + m.start()) + 1}: a case contains a "
        f"`return` that is not `return Err(..)`; `case()?` would change its "
        f"meaning, so this function needs splitting by hand"
    )


def _used_params(
    text: str,
    code: bytearray,
    lo: int,
    hi: int,
    params: list[tuple[str, str, str]],
) -> list[tuple[str, str, str]]:
    """Which declared fixtures this case actually mentions.

    Passing every fixture to every case would earn an `unused_variables`
    warning per unused one, so each case takes only what it names.  This is a
    textual use-test, not name resolution -- but it does not need to be sound,
    because the compiler is: a fixture this misses is still `E0434`, and one it
    adds spuriously is still an unused-variable warning.

    It is worth excluding *field* accesses even so, because they are not rare:
    `--param cmd:&str` matched `image.config.cmd` throughout `cmd_oci`'s
    inspect arm and passed a `cmd` nothing used.  A name after a `.` is a field
    or a method, never the local -- unless the dot is half of a `..` range,
    where `a..cmd` really is a use.
    """
    masked = _masked(text, code, lo, hi)
    return [p for p in params if _mentions(masked, p[0])]


def _mentions(masked: str, name: str) -> bool:
    """Does this case actually *read* the local `name`, so `--param` is used?"""
    return any(m.group(0) == name for m in _value_idents(masked))


def rewrite(
    text: str,
    fn_name: str,
    params: list[tuple[str, ...]] | None = None,
    arms: bool = False,
    flat: int = 0,
    at: int = 0,
) -> tuple[str, list[tuple[int, int]]]:
    # A fixture's call expression defaults to its own name; `--param` lets it
    # differ, so a `Vec` local can be handed to a `&[T]` parameter.
    params = [(p[0], p[1], p[2] if len(p) > 2 else p[0]) for p in (params or [])]
    # An internal invariant, not a limitation on what the tool accepts: `main`
    # normalises CRLF on read and restores it on write.  Kept as an assertion
    # because everything below indexes offsets produced by splitting on "\n".
    assert "\r\n" not in text, "rewrite() takes LF text; normalise before calling"
    lines = text.split("\n")
    starts = line_starts(lines)
    depth, code, lits = scan(text)

    fn_idx = find_fn(lines, fn_name, at)
    b_open = body_open(lines, fn_idx)
    sig = " ".join(lines[fn_idx : b_open + 1])
    unit = False

    if arms:
        # Every arm keeps its own `return;`, so the cases must be infallible and
        # the function must return unit -- see `find_match` for why that is the
        # condition rather than a preference.
        assert "->" not in sig, (
            f"`fn {fn_name}` returns a value; --arms only handles a unit "
            f"function, because it relies on `return;` inside an arm meaning "
            f"the same thing after the arm becomes a call"
        )
        inner = _depth_inside(lines, depth, code, starts, b_open)
        close_idx = _matching_close(lines, depth, starts, b_open, inner, text, code)
        m_open, m_close = find_match(
            lines, depth, code, starts, text, b_open, close_idx, inner
        )
        cases, skipped = find_arm_cases(
            lines, depth, code, starts, text, m_open, m_close, inner + 1
        )
        noun = "arm whose body is not a block on its own lines"
    else:
        # A unit-returning body is splittable too, on the stricter terms
        # `_check_unit_returns` states: its cases must contain no `return` and
        # no `?`, because neither has a faithful call spelling.  `cmd_oci`'s
        # `run` arm is the reason -- 1 178 lines and the largest frame left in
        # `kshell.rs`, in a function the earlier assertion turned away purely
        # for its signature.
        #
        # A diverging `-> !` body counts as unit: a case extracted from it is
        # an ordinary function that returns normally, and the two rules
        # `_check_unit_returns` enforces are free there -- a `return` cannot
        # appear in a `-> !` function and neither can `?`.  `kernel_main` is
        # the one that matters, being both the largest frame in the kernel and
        # the one frame that is certainly live under every init and every
        # self-test it calls.
        unit = "->" not in sig or DIVERGING.search(sig) is not None
        assert unit or "KernelResult<()>" in sig, (
            f"`fn {fn_name}` returns neither `()` nor KernelResult<()>; this "
            f"transformer only handles those signatures (see the module "
            f"docstring)"
        )
        open_idx = container(lines, depth, code, starts, b_open, text, bool(flat))
        inner = _depth_inside(lines, depth, code, starts, open_idx)
        close_idx = _matching_close(lines, depth, starts, open_idx, inner, text, code)
        if flat:
            declared = {p[0] for p in params}
            cases, skipped = find_flat_cases(
                lines, depth, code, starts, text, open_idx, close_idx, inner,
                flat, _sig_params(sig), declared,
                reserve_tail=DIVERGING.search(sig) is not None,
            )
            noun = "paragraph that reads an outer local, or binds one read later"
        else:
            cases, skipped = find_cases(
                lines, depth, code, starts, text, open_idx, close_idx, inner
            )
            noun = "`{` that is not a bare statement block"

    if skipped:
        print(f"  note: skipped {skipped} {noun}")
    if not cases:
        return text, []

    # Re-indenting only changes a literal's value if it spans a newline
    # verbatim.  Backslash-continued newlines have their following indent
    # stripped by Rust, so those are safe.
    lo, hi = starts[cases[0][0]], starts[cases[-1][1]]
    for a, b, is_raw in lits:
        if b <= lo or a >= hi:
            continue
        span = text[a:b]
        if "\n" not in span:
            continue
        assert not is_raw, f"multi-line raw string at offset {a} inside a case"
        for k, ch in enumerate(span):
            if ch == "\n":
                assert k > 0 and span[k - 1] == "\\", (
                    f"multi-line string without a backslash continuation at "
                    f"offset {a}: {span[:80]!r}"
                )

    out: list[str] = []
    case_at = {o: c for o, c in cases}
    expect = 0
    i = 0
    while i < len(lines):
        if i in case_at:
            c = case_at[i]
            head = lines[i]
            pad = head[: len(head) - len(head.lstrip())]  # the `{`'s own indent
            inner_pad = pad + "    "
            # A flat chunk has no braces of its own: the case body is the
            # statements themselves, and the wrapping block is new.
            lo_off = starts[i] if flat else starts[i + 1]
            hi_off = starts[c + 1] - 1 if flat else starts[c]
            if arms:
                # `return;` stays a `return;`, and the tail-position check in
                # `find_match` is what makes that faithful.
                fallible = False
            elif unit:
                _check_unit_returns(text, code, starts, lo_off, hi_off)
                fallible = False
            else:
                _check_returns(text, code, starts, lo_off, hi_off)
                fallible = _needs_result(text, code, lo_off, hi_off)
            take = _used_params(text, code, lo_off, hi_off, params)
            decl = ", ".join(f"{n}: {t}" for n, t, _ in take)
            pass_ = ", ".join(e for _, _, e in take)
            ret = f" -> {RET_TYPE}" if fallible else ""
            # An arm's own braces carry the `pat =>` and the trailing `,`, so
            # they are kept verbatim; a bare statement block's are regenerated.
            out.append(head if arms else f"{pad}{{")
            out.append(f"{inner_pad}#[inline(never)]")
            out.append(f"{inner_pad}fn case({decl}){ret} {{")
            # A block's contents were already one level in; a flat paragraph
            # was not, and gains two levels (the generated `{` and the `fn`).
            step = "        " if flat else "    "
            for b in lines[i : c + 1] if flat else lines[i + 1 : c]:
                out.append((step + b) if b.strip() else b)
            if fallible:
                out.append(f"{inner_pad}    Ok(())")
            out.append(f"{inner_pad}}}")
            out.append(f"{inner_pad}case({pass_}){'?' if fallible else ''};")
            out.append(lines[c] if arms else f"{pad}}}")
            # Fallible: 2 fn-header lines + `Ok(())` + the fn close + the call
            # = 5.  Infallible: the same without the `Ok(())` = 4.  A flat
            # chunk consumes no lines of its own, so its `{` and `}` are two
            # more.
            expect += (5 if fallible else 4) + (2 if flat else 0)
            i = c + 1
            continue
        out.append(lines[i])
        i += 1

    added = len(out) - len(lines)
    assert added == expect, (added, expect, len(cases))
    return "\n".join(out), [(o + 1, c + 1) for o, c in cases]


# --------------------------------------------------------------------------
# Regression suite
#
# Every case below is a trap this transformer actually fell into against real
# kernel source, reduced to the smallest input that reproduces it.  They are
# synthetic on purpose: the alternative -- re-deriving a rewrite from
# `git show HEAD:...` and diffing against the working tree -- only holds until
# the rewrite is committed, and stops being a test the moment it is.
# --------------------------------------------------------------------------
CASES: list[tuple[str, str, str, list[tuple[str, ...]], bool, int]] = []


def _case(name: str, src: str, want, params=(), arms: bool = False, flat: int = 0) -> None:
    CASES.append((name, src, want, list(params), arms, flat))


_case(
    "two plain cases",
    """fn self_test() -> KernelResult<()> {
    {
        let a = [0u8; 16];
        check(&a)?;
    }
    {
        let b = [0u8; 16];
        check(&b)?;
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            let a = [0u8; 16];
            check(&a)?;
            Ok(())
        }
        case()?;
    }
    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            let b = [0u8; 16];
            check(&b)?;
            Ok(())
        }
        case()?;
    }
    Ok(())
}
""",
)

_case(
    # `httpd::self_test`: eight cases, none of which can fail.  Giving each a
    # `KernelResult` earned eight `clippy::unnecessary_wraps` warnings.
    "infallible case gets no Result",
    """fn self_test() -> KernelResult<()> {
    {
        let a = [0u8; 16];
        serial_println!("[t] a={}", a[0]);
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    {
        #[inline(never)]
        fn case() {
            let a = [0u8; 16];
            serial_println!("[t] a={}", a[0]);
        }
        case();
    }
    Ok(())
}
""",
)

_case(
    # A `?` or a `return` only counts when it is code.  Masking is what stops
    # a question mark in a message from making a case needlessly fallible.
    "punctuation inside a literal does not make a case fallible",
    '''fn self_test() -> KernelResult<()> {
    {
        serial_println!("[t] did it return? no");  // return? no
    }
    Ok(())
}
''',
    '''fn self_test() -> KernelResult<()> {
    {
        #[inline(never)]
        fn case() {
            serial_println!("[t] did it return? no");  // return? no
        }
        case();
    }
    Ok(())
}
''',
)

_case(
    # `linux_fd.rs:916`.  rustfmt puts the `{` of an `if` with a wrapped
    # condition on its own line, so it looks exactly like a bare block.
    # Wrapping it produced `expected expression, found keyword 'else'`.
    "wrapped-`if` brace is not a case",
    """fn self_test() -> KernelResult<()> {
    if t.is_referenced(0x2222, -1)
        && !t.is_referenced(0x2222, 4)
    {
        serial_println!("[t] ok");
    } else {
        return Err(KernelError::InternalError);
    }
    Ok(())
}
""",
    None,
)

_case(
    # The same block's close is `} else {`, not `}`.  Matching the close by
    # line pattern skipped it and returned a later, unrelated `}` -- which
    # swallowed the else branch into the case body.
    "`} else {` does not end a case",
    """fn self_test() -> KernelResult<()> {
    {
        if x {
            serial_println!("[t] a");
        } else {
            return Err(KernelError::InternalError);
        }
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            if x {
                serial_println!("[t] a");
            } else {
                return Err(KernelError::InternalError);
            }
            Ok(())
        }
        case()?;
    }
    Ok(())
}
""",
)

_case(
    # A `}` in a comment (`sched::{set,copy}_task_name` is the real one) was
    # taken for a block's close, which silently found no cases at all.
    "a brace in a comment is not a brace",
    """fn self_test() -> KernelResult<()> {
    // exercises sched::{set,copy}_task_name
    {
        let a = [0u8; 16];
        check(&a)?;
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    // exercises sched::{set,copy}_task_name
    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            let a = [0u8; 16];
            check(&a)?;
            Ok(())
        }
        case()?;
    }
    Ok(())
}
""",
)

_case(
    "braces in strings, chars and nested block comments are not braces",
    r'''fn self_test() -> KernelResult<()> {
    /* outer /* inner } */ still a comment } */
    {
        let s = "a { b } c";
        let e = "quote \" then }";
        let c = '}';
        check(s, e, c)?;
    }
    Ok(())
}
''',
    r'''fn self_test() -> KernelResult<()> {
    /* outer /* inner } */ still a comment } */
    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            let s = "a { b } c";
            let e = "quote \" then }";
            let c = '}';
            check(s, e, c)?;
            Ok(())
        }
        case()?;
    }
    Ok(())
}
''',
)

_case(
    # Re-indenting adds four spaces to every line.  A raw string carries its
    # newlines verbatim, so that would silently change its value -- refuse.
    "a multi-line raw string in a case is refused",
    '''fn self_test() -> KernelResult<()> {
    {
        let s = r"line one
line two";
        check(s)?;
    }
    Ok(())
}
''',
    AssertionError,
)

_case(
    # ...but a backslash-continued normal string is fine: Rust strips the
    # following line's leading whitespace, so the extra indent is invisible.
    "a backslash-continued string is allowed",
    '''fn self_test() -> KernelResult<()> {
    {
        let s = "line one \\
                 line two";
        check(s)?;
    }
    Ok(())
}
''',
    '''fn self_test() -> KernelResult<()> {
    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            let s = "line one \\
                     line two";
            check(s)?;
            Ok(())
        }
        case()?;
    }
    Ok(())
}
''',
)

_case(
    # `case()?` reproduces `return Err(..)`, but not `return Ok(())` -- which
    # used to end the whole self-test and would now end only the case.
    "a `return Ok(())` in a case is refused",
    """fn self_test() -> KernelResult<()> {
    {
        let a = [0u8; 16];
        check(&a)?;
    }
    {
        if skip_this_platform() {
            return Ok(());
        }
        check_more()?;
    }
    Ok(())
}
""",
    AssertionError,
)

_case(
    # `self_test_remap_ioprio_futex2` shares one futex word across its cases,
    # so every case reads the outer `u32_ptr` -- `E0434` without `--param`.
    # Only the cases that name it take it, or the rest earn `unused_variables`.
    "a declared fixture is threaded through, but only where it is used",
    """fn self_test() -> KernelResult<()> {
    let u32_ptr = buf.as_ptr() as u64;
    {
        check(u32_ptr)?;
    }
    {
        serial_println!("[t] unrelated");
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    let u32_ptr = buf.as_ptr() as u64;
    {
        #[inline(never)]
        fn case(u32_ptr: u64) -> crate::error::KernelResult<()> {
            check(u32_ptr)?;
            Ok(())
        }
        case(u32_ptr)?;
    }
    {
        #[inline(never)]
        fn case() {
            serial_println!("[t] unrelated");
        }
        case();
    }
    Ok(())
}
""",
    [("u32_ptr", "u64")],
)

_case(
    # `cmd_oci`'s inspect arm never uses the outer `cmd`, but prints
    # `image.config.cmd` five times.  A bare `\bcmd\b` test took the field for
    # the local and passed an argument nothing used -- one warning per arm.
    "a field of the same name is not a use of the fixture",
    """fn self_test() -> KernelResult<()> {
    {
        check(image.config.cmd)?;
    }
    {
        check(&buf[start..cmd])?;
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            check(image.config.cmd)?;
            Ok(())
        }
        case()?;
    }
    {
        #[inline(never)]
        fn case(cmd: &str) -> crate::error::KernelResult<()> {
            check(&buf[start..cmd])?;
            Ok(())
        }
        case(cmd)?;
    }
    Ok(())
}
""",
    [("cmd", "&str")],
)

_case(
    # `eventlog::self_test` and friends: nothing to split, and saying so is
    # the correct answer -- not an exception, and not a no-op rewrite.
    "a function with no sibling blocks is left alone",
    """fn self_test() -> KernelResult<()> {
    let a = [0u8; 16];
    check(&a)?;
    Ok(())
}
""",
    None,
)


_case(
    # `kshell::cmd_oci`: the dispatcher shape.  The arm's own braces carry
    # `"inspect" =>` and a trailing `,`, so they are kept rather than
    # regenerated, and the shared `parts` is threaded in with a call expression
    # that differs from the parameter name (`&parts` for a `Vec`).
    "match arms become cases, braces and commas intact",
    """fn self_test(args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "inspect" => {
            let Some(dir) = parts.get(1) else {
                println!("usage");
                return;
            };
            show(dir);
        }
        "test" => match run() {
            Ok(()) => println!("ok"),
            Err(e) => println!("{:?}", e),
        },
        _ => {
            println!("unknown");
        }
    }
}
""",
    """fn self_test(args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "inspect" => {
            #[inline(never)]
            fn case(parts: &[&str]) {
                let Some(dir) = parts.get(1) else {
                    println!("usage");
                    return;
                };
                show(dir);
            }
            case(&parts);
        }
        "test" => match run() {
            Ok(()) => println!("ok"),
            Err(e) => println!("{:?}", e),
        },
        _ => {
            #[inline(never)]
            fn case() {
                println!("unknown");
            }
            case();
        }
    }
}
""",
    [("parts", "&[&str]", "&parts")],
    arms=True,
)

_case(
    # The safety condition, and the only one that cannot be delegated to the
    # compiler: a statement after the match means an arm's `return;` skips it,
    # while `case(); ` would not.
    "a match that is not the last statement is refused",
    """fn self_test(args: &str) {
    match args {
        "a" => {
            return;
        }
        _ => {
            println!("b");
        }
    }
    println!("done");
}
""",
    AssertionError,
    arms=True,
)

_case(
    # A dispatcher that returns a value cannot use this shape at all: `return x`
    # inside an arm would return from the case, not the command.
    "--arms refuses a function that returns a value",
    """fn self_test(args: &str) -> u32 {
    match args {
        "a" => {
            return 1;
        }
        _ => {
            return 0;
        }
    }
}
""",
    AssertionError,
    arms=True,
)


_case(
    # `eventlog::self_test`'s shape: `// Test N:` groups whose locals die with
    # them.  The boundaries are derived, not invented -- `t` is read only
    # inside the first group, `r` only inside the second.
    "a flat body is cut where no local crosses",
    """fn self_test() -> KernelResult<()> {
    serial_println!("[t] start");

    // Test 1.
    emit_one();
    let t = total();
    if t != 1 {
        return Err(KernelError::InternalError);
    }

    // Test 2.
    emit_two();
    let r = query();
    if r.matched != 2 {
        return Err(KernelError::InternalError);
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    serial_println!("[t] start");

    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            // Test 1.
            emit_one();
            let t = total();
            if t != 1 {
                return Err(KernelError::InternalError);
            }
            Ok(())
        }
        case()?;
    }

    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            // Test 2.
            emit_two();
            let r = query();
            if r.matched != 2 {
                return Err(KernelError::InternalError);
            }
            Ok(())
        }
        case()?;
    }
    Ok(())
}
""",
    flat=5,
)

_case(
    # The property the whole mode rests on: a local read after the cut keeps
    # the cut from happening there.  `saved` is restored at the very end, so
    # no chunk may hold it and no chunk may read it.
    "a local read later is not cut across",
    """fn self_test() -> KernelResult<()> {
    let saved = total();

    emit_one();
    let t = total();
    if t != 1 {
        return Err(KernelError::InternalError);
    }

    restore(saved);
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    let saved = total();

    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            emit_one();
            let t = total();
            if t != 1 {
                return Err(KernelError::InternalError);
            }
            Ok(())
        }
        case()?;
    }

    restore(saved);
    Ok(())
}
""",
    flat=4,
)

_case(
    # Only blank lines *above* a statement's first line of code are boundaries.
    # A blank line inside an `if` body is one the author put there to space out
    # a case's own guts, and cutting there would split a case in half -- and,
    # since the second half would open with `report(); }`, produce a paragraph
    # whose braces do not balance.  This also covers the two kinds of case
    # side by side: the first cannot fail, so it gets a plain `fn case()`.
    "a blank line inside a statement is not a boundary",
    """fn self_test() -> KernelResult<()> {
    serial_println!("[t] start");

    emit_one();
    if check() {

        report();
    }

    emit_two();
    let r = query();
    if r != 2 {
        return Err(KernelError::InternalError);
    }
    Ok(())
}
""",
    """fn self_test() -> KernelResult<()> {
    serial_println!("[t] start");

    {
        #[inline(never)]
        fn case() {
            emit_one();
            if check() {

                report();
            }
        }
        case();
    }

    {
        #[inline(never)]
        fn case() -> crate::error::KernelResult<()> {
            emit_two();
            let r = query();
            if r != 2 {
                return Err(KernelError::InternalError);
            }
            Ok(())
        }
        case()?;
    }
    Ok(())
}
""",
    flat=5,
)


_case(
    # A unit-returning body splits too.  Every case is infallible by
    # construction -- `-> KernelResult<()>` would be a lie and a
    # `clippy::unnecessary_wraps` besides -- so this is the plain `fn case()`
    # shape throughout, and the paragraph too short to be worth a case stays
    # in the outer frame where it was.
    "a unit-returning body splits into plain cases",
    """fn self_test() {
    println("a");

    step_one();
    step_two();
    step_three();

    step_four();
    step_five();
    step_six();
}
""",
    """fn self_test() {
    println("a");

    {
        #[inline(never)]
        fn case() {
            step_one();
            step_two();
            step_three();
        }
        case();
    }

    {
        #[inline(never)]
        fn case() {
            step_four();
            step_five();
            step_six();
        }
        case();
    }
}
""",
    flat=3,
)

_case(
    # The counterpart to "a case's `return Ok(())` is refused".  There the
    # objection is that `case()?` reproduces only `return Err(..)`; here there
    # is no `?` to reach for at all, so a `return;` that abandoned the whole
    # function would come to abandon one case and let the rest run.  Refused,
    # not skipped: the rewrite would compile and be wrong.
    "a `return` in a unit-returning body's case is refused",
    """fn self_test() {
    step_one();
    step_two();
    if broken() {
        return;
    }

    step_four();
    step_five();
    step_six();
}
""",
    AssertionError,
    flat=3,
)

_case(
    # An item is not a local.  It is scoped to its whole block wherever it
    # stands, so the paragraph three below may name it from inside a case --
    # what must not happen is the *declaring* paragraph being wrapped, which
    # would re-scope the item into that one case body.  Only the declaring
    # paragraph is therefore held back; the other three all become cases,
    # including the one that reads `BLOB`.
    "an item's declaring paragraph is held back, its readers are not",
    """fn self_test() {
    const BLOB: &[u8] = b"x";
    step_one();
    step_two();

    step_three();
    step_four();
    step_five();

    write(BLOB);
    step_six();
    step_seven();

    step_eight();
    step_nine();
    step_ten();
}
""",
    """fn self_test() {
    const BLOB: &[u8] = b"x";
    step_one();
    step_two();

    {
        #[inline(never)]
        fn case() {
            step_three();
            step_four();
            step_five();
        }
        case();
    }

    {
        #[inline(never)]
        fn case() {
            write(BLOB);
            step_six();
            step_seven();
        }
        case();
    }

    {
        #[inline(never)]
        fn case() {
            step_eight();
            step_nine();
            step_ten();
        }
        case();
    }
}
""",
    flat=3,
)

_case(
    # A `-> !` body's last paragraph stands in tail position, where the type is
    # `!` and a `{ fn case() {..} case(); }` block is `()`.  It is reserved into
    # the tail rather than made a case; every paragraph above it splits as
    # usual.  This is `kernel_main`, whose final `loop { hlt(); }` produced
    # `error[E0308]: expected !, found ()` on the first attempt.
    "the last paragraph of a diverging body is left in tail position",
    """fn self_test() -> ! {
    step_one();
    step_two();
    step_three();

    step_four();
    step_five();
    step_six();

    loop {
        halt();
    }
}
""",
    """fn self_test() -> ! {
    {
        #[inline(never)]
        fn case() {
            step_one();
            step_two();
            step_three();
        }
        case();
    }

    {
        #[inline(never)]
        fn case() {
            step_four();
            step_five();
            step_six();
        }
        case();
    }

    loop {
        halt();
    }
}
""",
    flat=3,
)

_case(
    # A leading fixture paragraph whose locals reach the bottom of the body is
    # left in the outer frame rather than merged -- merging would produce one
    # case containing everything, whose peak is `outer + case`.  Being left
    # there makes its `let`s *outer* locals, so a paragraph below that reads one
    # must be refused exactly as if it read a parameter.
    #
    # Dropping the paragraph without also recording what it binds is the shape
    # of the bug this guards: `fixture` would look unbound, the last paragraph
    # would be cased, and a nested `fn` cannot capture -- `E0434`, 36 of them in
    # `self_test_remap_ioprio_futex2`, discovered only at the end of a build.
    # Note the middle paragraph, which reads nothing outer, still splits.
    "a skipped leading fixture still counts as an outer local",
    """fn self_test() {
    let fixture = make();
    let spare = make_spare();

    step_one();
    step_two();
    step_three();

    use_it(fixture);
    step_five();
    step_six();
}
""",
    """fn self_test() {
    let fixture = make();
    let spare = make_spare();

    {
        #[inline(never)]
        fn case() {
            step_one();
            step_two();
            step_three();
        }
        case();
    }

    use_it(fixture);
    step_five();
    step_six();
}
""",
    flat=3,
)


def self_check() -> int:
    """Run the regression suite.  Returns 0 iff every case behaves."""
    failed = 0
    for name, src, want, params, arms, flat in CASES:
        try:
            got, cases = rewrite(src, "self_test", params, arms, flat)
        except AssertionError as exc:
            if want is AssertionError:
                print(f"ok   {name} (refused: {exc})")
            else:
                print(f"FAIL {name}: unexpected refusal: {exc}")
                failed += 1
            continue
        if want is AssertionError:
            print(f"FAIL {name}: should have been refused, but rewrote it")
            failed += 1
            continue
        if want is None:
            if cases:
                print(f"FAIL {name}: found {len(cases)} case(s), expected none")
                failed += 1
            else:
                print(f"ok   {name} (no cases, as expected)")
            continue
        if got != want:
            print(f"FAIL {name}:")
            for i, (a, b) in enumerate(
                zip(got.split("\n"), want.split("\n"))
            ):
                if a != b:
                    print(f"  line {i + 1}:\n    got  {a!r}\n    want {b!r}")
                    break
            else:
                print(f"  length differs: {len(got.splitlines())} vs "
                      f"{len(want.splitlines())} lines")
            failed += 1
            continue
        print(f"ok   {name} ({len(cases)} case(s))")
    print(f"\n{len(CASES) - failed}/{len(CASES)} passed")
    return 1 if failed else 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--file", help="source file to rewrite")
    ap.add_argument("--fn", dest="fn_name", help="function whose cases to split")
    ap.add_argument(
        "--param",
        action="append",
        default=[],
        metavar="NAME[=EXPR]:TYPE",
        help="pass an outer local into each case that names it, e.g. "
        "--param u32_ptr:u64 (repeatable). Use this for a fixture the cases "
        "share; a nested `fn` cannot capture, so without it such a case is "
        "E0434. EXPR overrides what the call passes, for when the parameter "
        "type is not the local's -- `--param parts=&parts:&[&str]` hands a "
        "`Vec<&str>` to a slice parameter. Pass cheap handles only: a fixture "
        "passed by value is copied into the case frame, which is the cost "
        "this tool exists to remove.",
    )
    ap.add_argument(
        "--arms",
        action="store_true",
        help="split the arms of the function's trailing `match` instead of "
        "bare statement blocks -- the shape of every kshell dispatcher. Only "
        "for a function returning unit whose match is its last statement; "
        "both are checked, because they are what make an arm's `return;` "
        "still mean `return;` once the arm is a call.",
    )
    ap.add_argument(
        "--flat",
        type=int,
        nargs="?",
        const=8,
        default=0,
        metavar="MIN_LINES",
        help="split a body that has no block structure at all -- a long run "
        "of statements, like eventlog::self_test -- into its blank-line-"
        "separated paragraphs. A paragraph becomes a case when it is at least "
        "MIN_LINES lines (default 8), reads no local bound before it, and "
        "binds nothing read after it.",
    )
    ap.add_argument(
        "--at",
        type=int,
        default=0,
        metavar="LINE",
        help="the line --fn's definition is on, when the name is not unique. "
        "Every case this tool generates is named `case`, so a second pass over "
        "an already-split file needs this to say which one",
    )
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="report what would change without writing",
    )
    ap.add_argument(
        "--force",
        action="store_true",
        help="write even a one-case split, which normally cannot help: the "
        "peak is `outer + case` either way",
    )
    ap.add_argument(
        "--check",
        action="store_true",
        help="run the transformer's regression suite and exit",
    )
    args = ap.parse_args()

    if args.check:
        return self_check()
    if not args.file or not args.fn_name:
        ap.error("--file and --fn are required unless --check is given")

    params = []
    for p in args.param:
        # The name never contains a colon and the type routinely does
        # (`crate::error::E`), so the first colon is the split -- and `=`, if
        # present, precedes it.
        head, sep, ty = p.partition(":")
        name, _, expr = head.partition("=")
        if not sep or not name.strip() or not ty.strip():
            ap.error(f"--param wants NAME[=EXPR]:TYPE, got {p!r}")
        params.append((name.strip(), ty.strip(), (expr or name).strip()))

    with io.open(args.file, encoding="utf-8", newline="") as f:
        text = f.read()
    # `rewrite` is a pure LF function -- it splits on "\n", indexes by the
    # offsets that produces, and emits "\n" at a couple of dozen sites -- so
    # the ending is normalised here, at the one boundary, rather than threaded
    # through the transformer.  27 of the kernel's sources are CRLF in the
    # working tree (they were written by something that opened them in text
    # mode on Windows) while every blob in the repository is LF, because
    # `core.autocrlf=input` normalises on check-in and never converts on
    # checkout.  Git therefore calls both clean and the difference is invisible
    # until a line-based tool trips over it -- which is precisely what an
    # assertion inside `rewrite` did, refusing `fs/handle.rs` and its 12 288-byte
    # self-test for a reason that has nothing to do with the refactor.
    #
    # A *mixed* file is refused rather than unified: unifying it would smuggle a
    # whole-file whitespace change into a commit whose subject is a stack frame.
    crlf = text.count("\r\n")
    if crlf:
        if crlf != text.count("\n"):
            ap.error(
                f"{args.file} mixes CRLF and LF line endings; normalise it "
                "first -- rewriting it here would bury a whole-file whitespace "
                "change inside a refactor commit"
            )
        text = text.replace("\r\n", "\n")
    if args.arms and args.flat:
        ap.error("--arms and --flat are different shapes; pick one")
    out, cases = rewrite(
        text, args.fn_name, params, args.arms, args.flat, args.at
    )
    if not cases:
        what = (
            "match arms with block bodies"
            if args.arms
            else "paragraphs no local crosses"
            if args.flat
            else "bare sibling blocks"
        )
        print(f"{args.fn_name}: no {what} found; nothing to do")
        return 0
    span = sum(c - o + 1 for o, c in cases)
    grew = len(out.split("\n")) - len(text.split("\n"))
    print(f"{args.fn_name}: {len(cases)} case(s), {span} lines, +{grew} lines")
    if len(cases) < 2 and not args.force:
        # Only one case is arithmetically a no-op.  Splitting moves a case's
        # locals out of the outer frame, but what is on the stack while the
        # case runs is `outer + case` -- so with a single case the peak is what
        # it always was, and the per-symbol ranking merely *reports* a smaller
        # outer.  That flattering-but-wrong reading is exactly why the
        # `linux_fd::self_test` split was made and then reverted (28 224 ->
        # 18 896 by the ranking, 28 224 -> 28 240 by `--peak`).  Two cases are
        # the minimum that can help, because only then does `max` beat `sum`.
        print(
            "  refusing to write a one-case split: the peak would stay "
            "`outer + case`, which is what it already is. Lower --flat's "
            "minimum, or pass --force if you have a reason."
        )
        return 1
    if args.dry_run:
        print("  (dry run; nothing written)")
        return 0
    with io.open(args.file, "w", encoding="utf-8", newline="") as f:
        # Put back whatever the file had.  Writing LF into a CRLF file would be
        # a correct-looking edit that shows up as a whole-file diff on any
        # checkout whose git is configured to convert, and there is no reason
        # for a stack-frame refactor to have an opinion about line endings.
        f.write(out.replace("\n", "\r\n") if crlf else out)
    print(f"  wrote {args.file}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
