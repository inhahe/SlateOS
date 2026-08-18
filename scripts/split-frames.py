#!/usr/bin/env python3
"""Give each case of a long self-test its own stack frame.

    python scripts/split-frames.py --file kernel/src/eventlog.rs --fn self_test
    python scripts/split-frames.py --file ... --fn ... --dry-run
    python scripts/split-frames.py --file ... --fn ... --param u32_ptr:u64
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

Functions whose cases are `match` arms (`kshell::cmd_oci`) remain out of scope.
So do functions with **flat** bodies -- `eventlog::self_test` and
`self_test_sysv_ipc_mqueue` are long runs of `let a = ..; if dispatch(..) {..}`
with no block structure -- because splitting those means inventing the case
boundaries, which is a decision about what the test's units are.

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


def find_fn(lines: list[str], name: str) -> int:
    """Return the 0-based index of the `fn <name>(` definition line."""
    hits = [
        i
        for i, ln in enumerate(lines)
        if ln.lstrip().startswith(("fn ", "pub fn ", "pub(crate) fn "))
        and ln.split("fn ", 1)[1].split("(")[0].split("<")[0].strip() == name
    ]
    assert hits, f"no `fn {name}` found"
    assert len(hits) == 1, f"`fn {name}` is ambiguous: lines {[h + 1 for h in hits]}"
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
) -> int:
    """Where the cases actually live.

    Some of these functions (`self_test_prctl_dispatch` among them) wrap their
    entire body in one further `{ ... }`, so the cases are the *grand*children
    of the signature.  Taken literally, the one wrapper would be reported as the
    single case and the rewrite would gain nothing.

    A lone bare block is genuinely ambiguous -- it is either such a wrapper or a
    single real case -- and the tell is not its size but its contents: a wrapper
    holds sibling cases, a case does not.  So descend only when the block holds
    at least two bare statement blocks of its own.  (Size was the first rule
    here; it is an arbitrary constant that misjudges any function short enough
    for its one case to be most of it.)
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
            nested, _ = find_cases(
                lines, depth, code, starts, text, bare[0], inner_close, body_depth + 1
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
    line, base = lines[idx], starts[idx]
    # Depth is recorded *before* each char, so just inside the line's last `{`
    # it is one deeper.  A `{` that the scanner swallowed as part of a literal
    # or comment carries the depth of its surroundings, so taking the last one
    # is only correct if it is real -- assert that it is.
    pos = max(k for k in range(len(line)) if line[k] == "{" and code[base + k])
    nxt = base + pos + 1
    assert depth[nxt] == depth[base + pos] + 1, (
        f"line {idx + 1}: the last `{{` is inside a literal or comment"
    )
    return depth[nxt]


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
        base = starts[open_idx]
        line = lines[open_idx]
        pos = max(k for k in range(len(line)) if line[k] == "{" and code[base + k])
        for k in range(base + pos + 1, len(text)):
            if code[k] and text[k] == "}" and depth[k] == inner:
                return _line_of(starts, k)
        raise AssertionError(f"no close for the block opened at line {open_idx + 1}")
    for j in range(open_idx + 1, len(lines)):
        if lines[j].strip() == "}" and depth[starts[j]] == inner:
            return j
    raise AssertionError(f"no close for the block opened at line {open_idx + 1}")


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
    base = starts[open_idx]
    line = lines[open_idx]
    pos = max(k for k in range(len(line)) if line[k] == "{" and code[base + k])
    for k in range(base + pos - 1, -1, -1):
        if code[k] and not text[k].isspace():
            return text[k] in ";{}"
    return False


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
    masked = "".join(text[k] if code[k] else " " for k in range(lo, hi))
    return "?" in masked or RETURN_RE.search(masked) is not None


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
    masked = "".join(text[k] if code[k] else " " for k in range(lo, hi))
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
    params: list[tuple[str, str]],
) -> list[tuple[str, str]]:
    """Which declared fixtures this case actually mentions.

    Passing every fixture to every case would earn an `unused_variables`
    warning per unused one, so each case takes only what it names.  This is a
    textual use-test, not name resolution -- but it does not need to be sound,
    because the compiler is: a fixture this misses is still `E0434`, and one it
    adds spuriously is still an unused-variable warning.
    """
    masked = "".join(text[k] if code[k] else " " for k in range(lo, hi))
    return [p for p in params if re.search(rf"\b{re.escape(p[0])}\b", masked)]


def rewrite(
    text: str, fn_name: str, params: list[tuple[str, str]] | None = None
) -> tuple[str, list[tuple[int, int]]]:
    params = params or []
    assert "\r\n" not in text, "file has CRLF; this transformer assumes LF"
    lines = text.split("\n")
    starts = line_starts(lines)
    depth, code, lits = scan(text)

    fn_idx = find_fn(lines, fn_name)
    sig = " ".join(lines[fn_idx : fn_idx + 3])
    assert "KernelResult<()>" in sig, (
        f"`fn {fn_name}` does not return KernelResult<()>; this transformer only "
        f"handles that signature (see the module docstring)"
    )

    open_idx = container(lines, depth, code, starts, body_open(lines, fn_idx), text)
    inner = _depth_inside(lines, depth, code, starts, open_idx)
    close_idx = _matching_close(lines, depth, starts, open_idx, inner, text, code)

    cases, skipped = find_cases(
        lines, depth, code, starts, text, open_idx, close_idx, inner
    )
    if skipped:
        print(f"  note: skipped {skipped} `{{` that is not a bare statement block")
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

    first = lines[cases[0][0]]
    pad = first[: len(first) - len(first.lstrip())]  # the `{`'s own indent
    inner_pad = pad + "    "

    out: list[str] = []
    case_at = {o: c for o, c in cases}
    expect = 0
    i = 0
    while i < len(lines):
        if i in case_at:
            c = case_at[i]
            _check_returns(text, code, starts, starts[i + 1], starts[c])
            fallible = _needs_result(text, code, starts[i + 1], starts[c])
            take = _used_params(text, code, starts[i + 1], starts[c], params)
            decl = ", ".join(f"{n}: {t}" for n, t in take)
            pass_ = ", ".join(n for n, _ in take)
            ret = f" -> {RET_TYPE}" if fallible else ""
            out.append(f"{pad}{{")
            out.append(f"{inner_pad}#[inline(never)]")
            out.append(f"{inner_pad}fn case({decl}){ret} {{")
            for b in lines[i + 1 : c]:
                out.append(("    " + b) if b.strip() else b)
            if fallible:
                out.append(f"{inner_pad}    Ok(())")
            out.append(f"{inner_pad}}}")
            out.append(f"{inner_pad}case({pass_}){'?' if fallible else ''};")
            out.append(f"{pad}}}")
            # Fallible: 2 fn-header lines + `Ok(())` + the fn close + the call
            # = 5.  Infallible: the same without the `Ok(())` = 4.
            expect += 5 if fallible else 4
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
CASES: list[tuple[str, str, str, list[tuple[str, str]]]] = []


def _case(name: str, src: str, want, params=()) -> None:
    CASES.append((name, src, want, list(params)))


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


def self_check() -> int:
    """Run the regression suite.  Returns 0 iff every case behaves."""
    failed = 0
    for name, src, want, params in CASES:
        try:
            got, cases = rewrite(src, "self_test", params)
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
        metavar="NAME:TYPE",
        help="pass an outer local into each case that names it, e.g. "
        "--param u32_ptr:u64 (repeatable). Use this for a fixture the cases "
        "share; a nested `fn` cannot capture, so without it such a case is "
        "E0434. Pass cheap handles only -- a fixture passed by value is "
        "copied into the case frame, which is the cost this tool exists to "
        "remove.",
    )
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="report what would change without writing",
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
        name, sep, ty = p.partition(":")
        if not sep or not name.strip() or not ty.strip():
            ap.error(f"--param wants NAME:TYPE, got {p!r}")
        params.append((name.strip(), ty.strip()))

    with io.open(args.file, encoding="utf-8", newline="") as f:
        text = f.read()
    out, cases = rewrite(text, args.fn_name, params)
    if not cases:
        print(f"{args.fn_name}: no bare sibling case blocks found; nothing to do")
        return 0
    span = sum(c - o + 1 for o, c in cases)
    grew = len(out.split("\n")) - len(text.split("\n"))
    print(f"{args.fn_name}: {len(cases)} case(s), {span} lines, +{grew} lines")
    if args.dry_run:
        print("  (dry run; nothing written)")
        return 0
    with io.open(args.file, "w", encoding="utf-8", newline="") as f:
        f.write(out)
    print(f"  wrote {args.file}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
