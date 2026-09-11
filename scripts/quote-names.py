#!/usr/bin/env python3
"""Diagnostics that name a file must route the name through `quote`.

`userspace/coreutils/tests/diagnostics_quote_names.rs` already says this, and
enforces it — over exactly one directory, `coreutils/src/bin`. Every other
utility crate in the tree is outside it, including all 41 that duplicate a
coreutils name. So `userspace/coreutils/src/bin/du.rs` is checked and
`userspace/du/src/main.rs` is not, and the two are the same program.

The defect being guarded is that a diagnostic written the natural way,

    eprintln!("cut: {path}: {e}");

compiles, reads correctly, and hands the *name* control of the error stream:
a file called `x\\ncut: /etc/shadow: Permission denied` makes `cut` appear to
have written a second line it never wrote. `quotef_os`/`quoteaf_os` render the
name unambiguously; nothing else does.

This script is that test's two detectors, run over the whole of lane B's tree
rather than one directory, as a **ratchet**: every site that exists today is
recorded in `quote-names-baseline.txt` -- which is also the live count, so no
number is repeated here to go stale -- and `--check` fails only on a *new*
one. A backlog that cannot grow is a different thing from a backlog.

    python scripts/quote-names.py                  # per-crate report
    python scripts/quote-names.py --list           # ... with every line
    python scripts/quote-names.py --check          # ratchet: new violations only
    python scripts/quote-names.py --selftest       # check the detectors
    python scripts/quote-names.py --write-baseline # re-record after a burn-down
    python scripts/quote-names.py --fix PATH...    # rewrite the mechanical sites

## Why `--fix` lives in the checker rather than in a script beside it

The backlog is 1700-odd sites across 775 files, and the overwhelming majority
of them are one of two shapes that convert without a judgement. A separate
fixer would have to re-derive "what is a site", and the moment its idea of
that drifts from the checker's, the two disagree in the direction that is
hardest to notice: the fixer skips a line the checker still counts, and the
burn-down silently stalls one site short. Sharing `violations()` makes that
impossible by construction.

`--fix` is deliberately timid. It transforms a line only when the result is
forced -- no existing positional placeholder to renumber, no ambiguity about
which argument a `{}` belongs to -- and prints every line it declined, so the
remainder is a worklist rather than a silent omission. It does not touch
`Cargo.toml` or add the `use`, because whether a crate should depend on
`quoting` at all is the one decision here that is not mechanical.

## Why the baseline counts violations per file rather than naming them

Three keys were possible, and the choice is a real tradeoff:

* `path:line` — exact, and useless: every edit above a site renumbers it, so
  the baseline would go stale on commits that touch nothing relevant.
* `path:<source text>` — exact and stable under line movement, but *not*
  under `rustfmt`, which rewraps argument lists routinely. A gate that fires
  on reformatting is a gate that gets bypassed, and a bypassed gate protects
  nothing.
* `path:<count>` — what is used here. Immune to both, and precise enough:
  adding a site to an already-listed file raises its count and fails the
  check, which is the case that actually happens.

The residual gap is a 1-for-1 swap inside one file — remove one violation and
add another in the same commit, and the count is unchanged. That shape is rare
enough to be worth the two failure modes it avoids, and the *Rust* test still
covers `coreutils/src/bin` exactly, where most of the traffic is.

Per-file (not per-crate) because the unit of repair is a call site and the
unit of review is a file; a crate-level count would hide a new violation in
`btrfs`'s 46 behind any one of them being fixed.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys

import selftestflag
from pathlib import Path
from typing import NamedTuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# The seam that makes `--head` a one-line choice rather than a second
# implementation of this checker. It also carries the `gitenv` handling that
# keeps a fixture's reads inside the fixture; both matter here, and the second
# one matters because this script's own self-test builds one.
import gitenv  # noqa: E402
import gittree  # noqa: E402
from rustlex import live_code  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "quote-names-baseline.txt"
BASELINE_REL = BASELINE.relative_to(ROOT).as_posix()

# The trees lane B owns. `apps/` and `gui/` are lane C's and are deliberately
# outside: a gate that fails another lane's commit for another lane's code is
# a gate that lane turns off.
ROOTS = ("userspace", "services", "init", "posix")

# Kept byte-for-byte in step with NOT_A_NAME in diagnostics_quote_names.rs.
NOT_A_NAME = {"msg", "e", "err", "error", "message", "reason"}

# Cargo's directories for code that is ENTIRELY test code.
#
# `live_code` blanks `#[cfg(test)]` items, which is every test inside a `src/`
# file and none of these: an integration test needs no attribute, because the
# whole file is only ever built by `cargo test`. The gate read them as
# production and `userspace/oils` -- a SHELL, whose integration tests build
# shell source like `format!("( exec >'{p}'; echo X )")` -- was the proof.
#
# This subsumes the entry that used to sit in IGNORE for
# `coreutils/tests/diagnostics_quote_names.rs`, which holds the detector's own
# fixtures and was exempted one file at a time. One file at a time is the
# enumeration-that-misses-the-next-instance shape, and the next instance was
# already in the tree.
TEST_DIRS = ("tests", "benches")

# Files whose hits are not defects, with the reason. This table records *why*
# a file is exempt; the baseline records only *that* a site exists, which is
# the wrong place for a judgement. Keep it short -- every entry is a hole.
IGNORE = {
    # All seven sites are `format!("'{ch}'")` building the NAME of a terminal
    # symbol -- `Symbol::Terminal(..)`, `terminals.insert(..)`, `prec_tag` --
    # where `'a'` is yacc's own spelling for a character-literal token. The
    # quotes are grammar syntax, not decoration on a message, and rewriting
    # them changes which symbols a grammar matches.
    #
    # This is the false positive the `format!` widening was always going to
    # have, and it is here rather than in the baseline because a baseline
    # entry says only that a site exists: these can never reach zero, and a
    # ratchet with an unreachable floor stops being read.
    "userspace/yacc/src/main.rs": "'{ch}' is a yacc terminal name, not a diagnostic",
    # `osh` is a bash-compatible shell and these are bash's own message texts,
    # character for character. Quoting the name would close the hole and would
    # be a visible divergence in the one program whose purpose is not to
    # diverge -- which is an operator decision here, not mine: §78
    # (`OSH_BASH_COMPAT`) and §79 (`OSH_UID`) are both the operator choosing
    # how this shell should differ from bash.
    #
    # Raised as B-Q12 with a recommendation. Exempted rather than baselined
    # because a baseline entry would read as "a defect we have not got to yet",
    # and what these actually are is a question nobody has answered.
    #
    # Two things that are NOT the reason, because both look like it: the values
    # are already text, not raw bytes (`format!` needs `Display`, which byte
    # strings do not implement), so no byte fidelity is at stake; and one of the
    # sixteen interpolates a fixed `"-d"`/`"-t"` and is not a name at all.
    "userspace/oils/src/interp.rs": "bash's own message text -- B-Q12",
    "userspace/oils/src/arith.rs": "bash's own message text -- B-Q12",
}

# The macros that build a message somebody will read.
#
# There were THREE spellings of this set, one inside each predicate below, and
# they had already drifted: `hand_written_quotes` matched `println!` and the
# other two did not. None of the three matched `format!`.
#
# That last gap is the one that bit. `userspace/efibootmgr` was given three
# diagnostics of the same shape in one commit on 2026-09-11; this gate refused
# the push over the `eprintln!` and passed the two that `format!` built a call
# earlier, in the same file, in the same commit. **A message does not stop
# being a message because it is assembled before it is printed.** The value
# reaches a terminal either way, and a newline in it forges a line either way.
#
# `format!` earns its place on measurement rather than on the principle: of the
# 507 `format!` sites this widening finds, 398 are inside `Err(..)`,
# `map_err(..)` or `ok_or(..)`, which is a message by construction, and the
# sampled remainder is mostly `SomeError::Variant(format!(..))` -- the same
# thing with a type around it. `userspace/rsync` alone holds 32 of the shape
# `format!("cannot open '{}': {e}", path.display())`, which is this gate's
# canonical defect written in the one macro it could not see.
#
# Spelled once now, so the next predicate cannot disagree with the other three.
_MESSAGE_OPEN = re.compile(r'(?P<mac>e?println!|format!)\s*\(\s*"')


def bare_interpolated_name(line: str) -> str | None:
    """Port of `bare_interpolated_name` in diagnostics_quote_names.rs.

    Matches `eprintln!("prog: {ident}: ...` — the shape where `ident` reaches
    the message as a bare name — in any of the macros `_MESSAGE_OPEN` lists,
    not only in `eprintln!`. See that constant for why `format!` is one.

    `line` is a *logical* line: `join_wrapped_calls` has already pulled a call
    that rustfmt split back onto one. What arrives here can therefore be
    `eprintln!( "cut: {path}: {e}" );`, with the spaces the join left behind,
    so the macro and its opening quote are matched with whitespace between
    them allowed rather than by a bare `partition`.
    """
    m = _MESSAGE_OPEN.search(line)
    if m is None:
        return None
    after = line[m.end() :]
    prog, sep, tail = after.partition(": {")
    if not sep:
        return None
    if not prog or not all(c.islower() or c.isdigit() or c == "_" for c in prog):
        return None
    ident, sep, rest = tail.partition("}")
    if not sep:
        return None
    if not ident or not all(c.isalnum() or c == "_" for c in ident):
        return None
    if not rest.startswith(": "):
        return None
    if ident in NOT_A_NAME:
        return None
    return ident


def _format_string_end(after_open_quote: str) -> int:
    """Index just past the closing quote of a Rust string literal.

    Written out rather than `find('"')` because a format string may contain an
    escaped quote, and stopping at it puts the argument scan inside the
    literal. `strip_noise` in `check-read-defaults.py` had the mirror-image bug
    twice this week; it is not a hypothetical class.
    """
    i = 0
    n = len(after_open_quote)
    while i < n:
        c = after_open_quote[i]
        if c == chr(92):          # backslash: skip whatever it escapes
            i += 2
            continue
        if c == '"':
            return i + 1
        i += 1
    return -1


def positional_name_arg(line: str) -> str | None:
    """The positional twin of [`bare_interpolated_name`].

    Matches `eprintln!("prog: {}: ...", <path>.display())` -- and the same
    shape in the other macros `_MESSAGE_OPEN` lists -- the same diagnostic,
    the same defect, the older spelling. Returns the argument text
    so the report can show which name reaches the message.

    Deliberately requires `.display()`. A bare identifier in a `{}` could be an
    integer, an enum or a count, and flagging those would make this gate cry
    wolf; `.display()` exists on `Path` and `PathBuf` and nothing else here, so
    it identifies a file name rather than guessing at one.
    """
    m = _MESSAGE_OPEN.search(line)
    if m is None:
        return None
    after = line[m.end():]
    prog, sep, tail = after.partition(": {}")
    if not sep:
        return None
    # Same program-prefix rule as the inline shape: lowercase, digits and
    # underscore. It is what keeps this off prose and off `{}` in the middle of
    # a sentence.
    if not prog or not all(c.islower() or c.isdigit() or c == "_" for c in prog):
        return None
    if not tail.startswith(": "):
        return None
    end = _format_string_end(tail)
    if end < 0:
        return None
    args = tail[end:]
    am = re.match(r"\s*,\s*(?P<arg>[A-Za-z_][\w.]*(?:\([^()]*\))?\.display\(\))", args)
    if am is None:
        return None
    arg = am.group("arg")
    # Already routed through the quoting helpers.
    if "quote" in arg:
        return None
    if arg.split(".")[0] in NOT_A_NAME:
        return None
    return arg


def quotes_around_placeholder(fmt: str) -> bool:
    """Do hand-written single quotes wrap an actual format *placeholder*?

    Scanned the way Rust scans a format string, because the naive test --
    "contains `'{` and contains `}'`" -- cannot tell a placeholder from a
    doubled brace. `{{` and `}}` are Rust's escapes for a literal `{` and `}`,
    so a usage line reading

        aws events put-rule --event-pattern '{{"source":["aws.s3"]}}'

    prints a JSON example with no interpolation whatsoever, yet contains both
    substrings. Reporting it is not merely noise: there is no edit that
    resolves it, so it sits in the backlog forever as a site that must be
    "declined" by hand on every pass. eventbridge-cli's help text has two.

    The scan skips a doubled brace and, at a real placeholder, asks only
    whether the characters immediately around it are quotes.
    """
    i, n = 0, len(fmt)
    while i < n:
        c = fmt[i]
        if c == "{":
            if i + 1 < n and fmt[i + 1] == "{":
                i += 2
                continue
            j = fmt.find("}", i)
            if j == -1:
                return False
            if i and fmt[i - 1] == "'" and j + 1 < n and fmt[j + 1] == "'":
                return True
            i = j + 1
            continue
        if c == "}" and i + 1 < n and fmt[i + 1] == "}":
            i += 2
            continue
        i += 1
    return False


def hand_written_quotes(line: str) -> bool:
    """Port of the `no_diagnostic_hand_writes_quotes_around_a_name` detector.

    `'{path}'` is worse than `{path}`, not better: it *looks* quoted, so it
    survives review, while a name containing a `'` still breaks out.

    The scan starts at the macro's opening quote, not at the start of the
    line, so a brace belonging to enclosing *code* (`if x { println!(...) }`
    joined onto one logical line) cannot be mistaken for a placeholder and
    swallow the real one that follows it.
    """
    m = _MESSAGE_OPEN.search(line)
    if m is None:
        return False
    return quotes_around_placeholder(line[m.end() :])


def is_prose(line: str) -> bool:
    """A comment, not code.

    The Rust test needs no such filter because it reads only
    `coreutils/src/bin`, where no comment happens to contain the pattern. A
    tree-wide scan does — this script's own docstring would otherwise flag
    itself, and so do several doc-comments that quote the bad form in order to
    warn about it.
    """
    return line.lstrip().startswith("//")


# How many physical lines a single wrapped macro call may span before the
# joiner gives up. A bound is needed because the scanner can be defeated by
# Rust syntax it does not model (a raw string, a lifetime that looks like an
# unterminated char literal), and an unbounded join would then swallow the
# rest of the file. 40 is far above the longest real call in this tree.
MAX_JOIN_LINES = 40


def _delta(src: str) -> int | None:
    """Net bracket depth of `src`, ignoring brackets inside literals.

    `None` if a string literal is left open at the end, which means the text
    is not something this scanner understands and the caller must not join.
    A format string is full of `{`, `}` and often `(`, so skipping literals is
    not an optimisation — counting them would make every call look unbalanced.
    """
    depth = 0
    k, n = 0, len(src)
    while k < n:
        c = src[k]
        if c == '"':
            k += 1
            while k < n and src[k] != '"':
                k += 2 if src[k] == "\\" else 1
            if k >= n:
                return None
        elif c == "'":
            # `'x'` and `'\n'` are char literals; `'a` is a lifetime. Only the
            # first two can hide a bracket, and only they are skipped.
            if k + 1 < n and src[k + 1] == "\\":
                end = src.find("'", k + 2)
                if end != -1:
                    k = end
            elif k + 2 < n and src[k + 2] == "'":
                k += 2
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        k += 1
    return depth


def join_wrapped_calls(text: str) -> list[tuple[int, int, str]]:
    """Group physical lines into logical ones: `(first, last, source)`.

    A `println!`/`eprintln!` whose arguments rustfmt split across lines is
    rejoined into one entry. Without this the detectors are blind to exactly
    the sites formatting touched -- and *measurably* so: running `cargo fmt`
    over five untouched crates in this tree made three real violations
    disappear from the count, because rustfmt had moved the macro name onto a
    line of its own. A checker a formatter can silence is not a checker.

    Lines that are not a wrapped call are returned unchanged, one per entry,
    so callers can treat the result as "the lines, but correct".

    A physical line ending in a backslash is Rust's *line continuation* inside
    a string literal: the backslash, the newline and the next line's leading
    whitespace all vanish from the string's value. That boundary is therefore
    joined with nothing rather than with a space, and the backslash dropped --
    otherwise the reassembled literal is not the one the compiler sees. It
    matters twice over: a spurious space lands in the middle of the message the
    detector reads, and `--fix` writes the joined text *back to disk*, where
    `\\ ` is not a valid escape and the file stops compiling. That is not
    hypothetical; it is what this function did to `userspace/diskutil` before
    the continuation branch below existed. Outside a literal a trailing
    backslash is a syntax error in Rust, so there is nothing else it can be.
    """
    lines = text.split("\n")
    out: list[tuple[int, int, str]] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        start = -1
        for mac in ("eprintln!", "println!"):
            at = line.find(mac)
            if at != -1 and (start == -1 or at < start):
                start = at
        if start == -1 or is_prose(line):
            out.append((i + 1, i + 1, line))
            i += 1
            continue
        joined = line
        j = i
        while True:
            d = _delta(joined[start:])
            if d is not None and d <= 0:
                break
            j += 1
            if j >= len(lines) or j - i >= MAX_JOIN_LINES:
                j = i
                joined = line
                break
            if _is_continuation(joined):
                joined = joined[:-1] + lines[j].lstrip()
            else:
                joined += " " + lines[j].strip()
        out.append((i + 1, j + 1, joined))
        i = j + 1
    return out


def _is_continuation(src: str) -> bool:
    """Does `src` end in a string-literal line continuation?

    An *odd* number of trailing backslashes: `"a\\` continues, `"a\\\\` is an
    escaped backslash and ends the line for real.
    """
    n = len(src) - len(src.rstrip("\\"))
    return n % 2 == 1


def violations(text: str) -> list[tuple[int, str, str]]:
    """`(line number, what, source)` for every flagged call in `text`.

    The line number is the *first* physical line of the call, which is where a
    reader looks and where `--fix` rewrites.
    """
    out: list[tuple[int, str, str]] = []
    for first, _last, line in join_wrapped_calls(text):
        if is_prose(line):
            continue
        ident = bare_interpolated_name(line)
        if ident is not None:
            out.append((first, f"{{{ident}}} unquoted", line.strip()))
        elif (arg := positional_name_arg(line)) is not None:
            # The same defect, written `{}` with the name in the argument list
            # instead of captured inline. Invisible to this gate until
            # 2026-09-10, which is why its baseline read zero while 224 of
            # these were in the tree.
            out.append((first, f"{arg} unquoted (positional)", line.strip()))
        elif hand_written_quotes(line):
            out.append((first, "hand-written quotes", line.strip()))
    return out


# A whole `println!`/`eprintln!` call: indentation, the macro and its format
# string, then the optional argument list, then `);`. Whitespace is allowed
# after the `(` and before the `)` because the input may be a call
# `join_wrapped_calls` reassembled from several physical lines; the rewrite
# emits one line and leaves rustfmt to re-wrap it, which is the only way to
# reformat a wrapped call without reimplementing rustfmt's decisions.
# The whole call on one (logical) line. `lead` may carry a match-arm pattern:
# `_ => println!("influx: '{}' completed", sub),` is a real and common shape in
# the CLI wrappers, and without this it was reported as "not a single-line
# call" -- five of `influx-cli`'s nine sites, all of them ordinary. `end` is
# `;` or the `,` that terminates a match arm.
#
# The pattern in that arm is very often a *literal*: `"-a" => println!(...)` is
# how the option-dispatch wrappers are written, and abduco-cli was four sites
# out of five this shape. So `lead` has to be able to contain a quote -- which
# is exactly what makes it dangerous, because a string holding the text
# `println!(` must not be mistaken for the call itself.
#
# `_ARM_ATOM` is what buys the safety back. Outside a literal the lead may not
# contain a quote, a brace or a semicolon, so it still cannot cross a block
# boundary; and a literal may only be consumed *whole*. There is no way for
# the match to end the lead in the middle of a string, because the only
# alternative that can pass an opening quote is the one that also consumes the
# closing one. A pattern of `"a println!(b"` is therefore harmless.
#
# `lead` may also end in `{`, and `tail` may follow the terminator, because an
# arm whose body is a *block* is just as common as one whose body is a bare
# expression:
#
#     other => { eprintln!("npm: unknown command '{}'", other); 1 }
#
# Nine sites in this tree are that exact line with a different program name.
# Neither piece is parsed -- both are reproduced byte for byte around the
# rewritten call -- so allowing them cannot change what the surrounding code
# does. `tail` therefore does not have to *match* the brace in `lead`: an
# unpaired one is text this rewrite copies through untouched either way.
_ARM_ATOM = r"""(?:[^"'{};]|"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')"""
_CALL = re.compile(
    rf'^(?P<lead>\s*(?:{_ARM_ATOM}*=>\s*)?(?:\{{\s*)?)(?P<mac>e?println!)'
    r'\(\s*"(?P<fmt>(?:[^"\\]|\\.)*)"(?P<args>.*?)\s*\)(?P<end>[;,])'
    r'(?P<tail>[^"\'{}]*\})?$'
)

# `'{ident}'` -- the hand-written-quote shape, with a plain identifier inside.
_INLINE_QUOTED = re.compile(r"'\{([A-Za-z_][A-Za-z0-9_]*)\}'")
# `'{}'` -- the same defect, but the value comes from the argument list.
_POSITIONAL_QUOTED = re.compile(r"'\{\}'")
# Any placeholder at all, used only to prove a rewrite cannot renumber one.
_ANY_PLACEHOLDER = re.compile(r"\{[^{}]*\}")


def _split_args(args: str) -> list[str] | None:
    """The top-level comma-separated arguments of a macro call, or `None`.

    Returns `None` rather than guessing when the text contains a string or
    char literal, because a comma inside one is not a separator and getting
    that wrong would silently mangle code.
    """
    args = args.strip()
    if not args:
        return []
    if not args.startswith(","):
        return None
    args = args[1:]
    if '"' in args or "'" in args:
        return None
    out: list[str] = []
    depth = 0
    cur = ""
    for ch in args:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth < 0:
                return None
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    # A trailing comma before `)` is legal Rust and is what rustfmt writes on
    # every call it wraps, so it is the *common* case here, not an oddity: the
    # split leaves a final empty element that must be dropped rather than
    # treated as a malformed argument list.
    while out and not out[-1]:
        out.pop()
    return out if all(out) else None


def _scan_to_close(line: str, i: int) -> int:
    """Index of the paren closing a macro call whose `(` is already consumed.

    Refuses on any quote. A comma inside a string literal is not a separator
    and a `)` inside one is not a terminator, and telling them apart needs a
    lexer rather than a counter -- `_split_args` declines the same input for
    the same reason, so nothing reaches the rewrite on a guess.
    """
    depth = 1
    while i < len(line):
        ch = line[i]
        if ch in "\"'":
            return -1
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def _outside_a_literal(line: str, pos: int) -> bool:
    """Whether `pos` sits outside every string literal on `line`.

    A match-arm pattern is very often a literal -- `"-a" => println!(..)` is
    how the option-dispatch wrappers are written -- so a literal containing
    the text `println!(` is a shape that exists in this tree and must not be
    mistaken for the call itself. The old regex bought this with `_ARM_ATOM`,
    which could consume a literal only whole; a scanner has to count instead.
    """
    inside = False
    i = 0
    while i < pos:
        ch = line[i]
        if inside and ch == "\\":
            i += 2
            continue
        if ch == '"':
            inside = not inside
        i += 1
    return not inside


def _macro_call(line: str) -> tuple[int, str, str, str, int] | None:
    """Locate the one message-building macro call on `line`.

    Returns `(start, mac, fmt, args_text, close)`: where the macro name
    begins, which macro it is, the format string's contents, everything
    between that string and the call's closing paren, and where that paren is.

    A regex did this until 2026-09-11, anchored to the start and the end of
    the line. That works while every site is a whole statement --
    `eprintln!("cut: {path}: {e}");` -- and cannot reach the shape `format!`
    is nearly always written in, because the call is an expression with code
    on both sides of it:

        _ => Err(format!("unknown family '{s}'")),
        .map_err(|e| format!("read '{}': {e}", path.display()))?;

    There is nothing to anchor to there. Counting parens from the macro's own
    `(` finds the end wherever it happens to be, and the text on either side
    is carried through untouched.

    Two calls on one line returns `None`: which one is "the" call is then
    ambiguous, and a rewrite would repair one and leave the other while
    reporting the line fixed.
    """
    cands = [m for m in _MESSAGE_OPEN.finditer(line) if _outside_a_literal(line, m.start())]
    if len(cands) != 1:
        return None
    m = cands[0]
    fmt_start = m.end()
    rel = _format_string_end(line[fmt_start:])
    if rel < 0:
        return None
    fmt_end = fmt_start + rel
    close = _scan_to_close(line, fmt_end)
    if close < 0:
        return None
    return (
        m.start(),
        m.group("mac"),
        line[fmt_start : fmt_end - 1],
        line[fmt_end:close],
        close,
    )


def _positional_slots(fmt: str) -> list[bool] | None:
    """One entry per argument `fmt` consumes, `True` where the placeholder is
    wrapped in hand-written single quotes.

    This is what lets a quoted placeholder share an argument list with a bare
    one. The single commonest remaining shape in the tree is

        format!("Cannot read '{}': {}", profile_path.display(), e)

    -- the name quoted, the error not -- and counting quoted placeholders
    against all placeholders cannot tell which argument is which, so it used
    to decline. Position can: the first slot is the name, the second is the
    error, and only the first gets wrapped.

    `None` when the string contains brace syntax this cannot account for.
    Miscounting a slot does not produce a wrong message, it shifts EVERY
    argument after it, so the uncertain cases are refused rather than guessed:

    * a lone `}`, which is not valid format syntax and means the scan has lost
      its place;
    * an unterminated `{`.

    `{{` and `}}` are brace escapes and consume nothing. `{name}` and
    `{name:spec}` capture from the enclosing scope and consume nothing either;
    `{}` and `{:spec}` consume an argument. That distinction is the whole
    reason this cannot be done by counting `{}` occurrences.
    """
    slots: list[bool] = []
    i, n = 0, len(fmt)
    while i < n:
        ch = fmt[i]
        if ch == "{":
            if fmt.startswith("{{", i):
                i += 2
                continue
            j = fmt.find("}", i)
            if j < 0:
                return None
            body = fmt[i + 1 : j]
            if body == "" or body.startswith(":"):
                slots.append(
                    i > 0 and fmt[i - 1] == "'" and j + 1 < n and fmt[j + 1] == "'"
                )
            i = j + 1
            continue
        if ch == "}":
            if fmt.startswith("}}", i):
                i += 2
                continue
            return None
        i += 1
    return slots


# An argument that is a *value* -- an identifier, a field, an index -- rather
# than the result of a call.
#
# Only a value may be wrapped. A call may already be a rendering: `cal`'s
# `shown(arg)` is `escape_unprintable(..)`, which has octal-escaped the bytes
# already, and `quoteaf_os` around it would escape the backslashes a second
# time and print `\\012` where the name held a newline. Declining every call
# costs a handful of sites that a person can look at, and the alternative is a
# fixer that silently double-escapes.
_PLAIN_VALUE = re.compile(
    r"^&?[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*|\[[^\[\]]*\])*$"
)

# Calls that RENDER a name rather than being one. `quotef_os` takes the value
# itself, so these come off rather than being wrapped.
_RENDERERS = (".display()", ".to_string_lossy()")


def _name_expr(arg: str) -> str:
    """The name a rendering call was applied to.

    `path.display()` is not a name, it is a *rendering* of one, and a lossy
    rendering: `Display for Path` and `to_string_lossy` both put U+FFFD where
    the bytes are not UTF-8, so the name in the message is not the name on
    disk and cannot be matched against it. On a filesystem whose rule is "any
    byte except `/` and NUL" that is not a corner case.

    Stripping the call is therefore not a convenience for the rewrite: it is
    half the repair. `quoteaf_os(&path.display())` would not even compile --
    `std::path::Display` is not `AsRef<OsStr>` -- so without this the fixer
    would emit code that fails to build, which is the one output a fixer must
    never produce.
    """
    for suffix in _RENDERERS:
        if arg.endswith(suffix):
            return arg[: -len(suffix)]
    return arg


def fix_line(line: str) -> tuple[str | None, str]:
    """Rewrite one flagged line, or explain why it was left alone.

    Returns `(new_line_or_None, reason)`. The two shapes handled:

    * `'{name}'` -> `{}` plus `quoteaf_os(&name)`. `quoteaf_os` *always*
      quotes, so an ordinary name renders `'abc'` -- byte for byte what the
      hand-written quotes printed. Choosing `quotef_os` here would change the
      output of every existing message, which is a different change.
    * `"prog: {name}: ..."` -> `{}` plus `quotef_os(&name)`. Nothing was
      quoted before, so the quote-only-when-needed form keeps the common case
      identical and only differs on names that were already ambiguous.

    Anything else -- a call spanning lines, a format string that already has a
    positional placeholder the rewrite would renumber, an argument list this
    cannot parse -- is returned unchanged with a reason, never guessed at.
    """
    call = _macro_call(line)
    if call is None:
        return None, "not a single-line message-building macro call"
    start, mac, fmt, args_text, close = call
    args = _split_args(args_text)
    if args is None:
        return None, "argument list not safely splittable"

    def rebuilt(new_fmt: str, new_args: list[str]) -> str:
        tail = "".join(f", {a}" for a in new_args)
        # The call itself is re-emitted in canonical form, which is how a
        # wrapped call that `join_wrapped_calls` reassembled loses the spaces
        # the join left behind. Everything OUTSIDE it -- `line[:start]` and
        # `line[close + 1:]`, the expression the call is nested in -- is
        # reproduced byte for byte and never parsed.
        return f'{line[:start]}{mac}("{new_fmt}"{tail}){line[close + 1 :]}'

    ident = bare_interpolated_name(line)
    if ident is not None:
        # The rewrite turns `{ident}` into `{}`, which consumes the *first*
        # unused argument. That is only the one being added if no earlier
        # placeholder is already positional.
        head = fmt.split("{" + ident + "}", 1)[0]
        if "{}" in head or args:
            return None, "would renumber an existing positional argument"
        return rebuilt(fmt.replace("{" + ident + "}", "{}", 1), [f"quotef_os(&{ident})"]), ""

    # `prog: {}: reason` with the name in the argument list. Nothing is quoted
    # here and the placeholder count does not change -- only the argument is
    # replaced -- so this is the one shape with no renumbering hazard at all.
    if positional_name_arg(line) is not None:
        if len(args) != 1:
            return None, "the colon form with more than one argument"
        name = _name_expr(args[0])
        if not _PLAIN_VALUE.match(name):
            return None, "the name is the result of a call, which may already render it"
        return rebuilt(fmt, [f"quotef_os(&{name})"]), ""

    inline = _INLINE_QUOTED.findall(fmt)
    positional = len(_POSITIONAL_QUOTED.findall(fmt))

    if inline and not positional:
        if any(p == "{}" for p in _ANY_PLACEHOLDER.findall(fmt)) or args:
            return None, "would renumber an existing positional argument"
        return (
            rebuilt(_INLINE_QUOTED.sub("{}", fmt), [f"quoteaf_os(&{n})" for n in inline]),
            "",
        )

    if positional and not inline:
        slots = _positional_slots(fmt)
        if slots is None:
            return None, "brace syntax this cannot account for"
        if len(args) != len(slots):
            return None, "positional placeholders and arguments do not line up"
        # Every quoted slot must be the plain `'{}'` form. `'{:?}'` is not the
        # same defect -- `Debug` escapes the value already -- and stripping the
        # quotes off one would leave a placeholder this did not repair.
        if sum(slots) != positional:
            return None, "a quoted placeholder carries a format spec"
        wrapped = [_name_expr(a) for a, q in zip(args, slots) if q]
        if not all(_PLAIN_VALUE.match(w) for w in wrapped):
            return None, "the name is the result of a call, which may already render it"
        return (
            rebuilt(
                _POSITIONAL_QUOTED.sub("{}", fmt),
                [
                    f"quoteaf_os(&{_name_expr(a)})" if q else a
                    for a, q in zip(args, slots)
                ],
            ),
            "",
        )

    return None, "mixed or multi-argument quoting -- fix by hand"


def fix_file(path: Path) -> tuple[int, list[str]]:
    """Rewrite what can be rewritten in `path`. Returns `(fixed, skipped)`."""
    text = path.read_text(encoding="utf-8")
    lines = text.split("\n")
    fixed = 0
    skipped: list[str] = []
    flagged = {first for first, _, _ in violations(text)}
    # Latest-first, so that replacing a wrapped call with one line does not
    # shift the line numbers of the sites still to be visited.
    for first, last, joined in reversed(join_wrapped_calls(text)):
        if first not in flagged:
            continue
        new, reason = fix_line(joined)
        if new is None:
            skipped.append(f"{path.as_posix()}:{first}: {reason}\n      {joined.strip()}")
        else:
            # A rejoined call is emitted as one line and left to rustfmt to
            # re-wrap: reproducing where rustfmt would have broken it means
            # reproducing rustfmt, and getting that subtly wrong would put a
            # formatting diff inside every one of these commits.
            lines[first - 1 : last] = [new]
            fixed += 1
    if fixed:
        # newline="" for the same reason write_baseline uses it: Python would
        # otherwise turn every "\n" into "\r\n" and rewrite the whole file.
        path.write_text("\n".join(lines), encoding="utf-8", newline="")
    return fixed, skipped


def _ambient_head() -> str:
    """`HEAD` of the repository this *script* lives in, or `""` if unreadable.

    Read with the environment deliberately left alone -- the one place in this
    file that wants the ambient repository rather than the one it was handed,
    because the question is precisely "did something write here?".

    Not raised on failure: this is a witness for an assertion, and a witness
    that cannot answer must not turn into a finding of its own. `""` compares
    equal to `""`, so an unreadable repository makes the check silently
    inconclusive rather than falsely red -- and the case it guards is still
    asserting everything else it always did.
    """
    try:
        return subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
            check=True, capture_output=True,
        ).stdout.decode("utf-8", "replace").strip()
    except (subprocess.CalledProcessError, OSError):
        return ""


def _decode(raw: bytes | None) -> str | None:
    """Blob bytes as UTF-8 text, or `None` if they are not UTF-8 at all.

    Strict, and not `Tree.read_text` -- which replaces bad bytes -- because a
    non-UTF-8 `.rs` file is not something this lexer can speak about. Lexing
    the replacement characters would invent findings at positions that do not
    exist in the file; skipping says nothing, which is the honest answer.
    """
    if raw is None:
        return None
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        return None


class Survey(NamedTuple):
    """What one pass over a tree saw.

    `found` alone cannot answer the question `main` has to ask before believing
    a clean result, because an empty `found` has two causes that look identical
    from the outside: a tree with no defects, and a tree with no *sources*. So
    the count of files actually read comes back with it, from the same walk
    that produced the findings -- deriving it from a second walk would be a
    second answer to the same question, which is the drift `survey_tree`'s
    docstring below is about.
    """

    found: dict[str, list[tuple[int, str, str]]]
    scanned: int


def survey_tree(tree: gittree.Tree) -> Survey:
    """Every flagged line in lane B's zone of `tree`, keyed by relative path.

    One function for both the disk and a revision, because the seam it reads
    through cannot tell a checker which it is holding. That is the point: the
    previous shape had a `survey()` walking `rglob` and a `survey_at()` driving
    `cat-file`, alike only by the care of whoever edited them last. Two
    implementations of one question drift, and the drift is invisible -- each
    half keeps returning a well-formed answer to a slightly different question.

    `files_under` already prunes build directories by path component, so the
    `"target" not in parts` test both halves used to carry separately is now
    the seam's business and is asserted by `test-gittree.py` on both sides.

    A file that is not UTF-8 counts as scanned even though it is not lexed. The
    count exists to answer "did this tree have a subject", and a `.rs` file this
    lexer declines to speak about is still a subject -- counting it as absent
    would let a corpus of undecodable sources read as no corpus at all.
    """
    found: dict[str, list[tuple[int, str, str]]] = {}
    scanned = 0
    for top in ROOTS:
        for rel in tree.files_under(top):
            if not rel.endswith(".rs") or rel in IGNORE:
                continue
            if any(part in TEST_DIRS for part in rel.split("/")):
                continue
            scanned += 1
            text = _decode(tree.read_bytes(rel))
            if text is None:
                continue
            # Test code is not a diagnostic. This gate was the only one of the
            # eleven rustlex exists for that read `#[cfg(test)]` as production
            # -- harmless while it saw only `eprintln!`, and not harmless the
            # moment it saw `format!`: `userspace/oils` builds shell source in
            # its fixtures (`format!("eval '{src}'")`), which is a shell test
            # doing its job and would have entered the ledger as a defect
            # nobody could ever fix.
            #
            # `live_code` BLANKS the test items to spaces rather than cutting
            # at the first one, so the line numbers this reports still point at
            # the line a reader will open.
            hits = violations(live_code(text)[0])
            if hits:
                found[rel] = hits
    return Survey(found, scanned)


def _no_corpus(seen: Survey, head: str | None) -> bool:
    """Whether the tree under judgement had no subject at all -- and say so.

    A gate that finds nothing reports the same thing whether the tree is clean
    or the tree is *empty*, and the second is not a verdict about anybody's
    code. It was measured, not imagined: a commit renaming `userspace/` away
    was checked against a baseline that still listed a file in it, and the gate
    printed

        fixed: userspace/coreutils/src/bin/cut.rs 1 -> 0
        ok -- 0 known sites in 0 files (1 improved)

    and exited 0. The disappearance of its own subject read as *progress*, and
    the wording invited someone to run `--write-baseline` and delete the whole
    ratchet on the strength of it. That is the worst available failure for a
    ratchet: it does not merely pass a bad push, it offers to forget every site
    it was ever guarding.

    Exit 2 rather than 1, for `run-checker.sh`'s reason: 1 means "this checker
    found something in your code", and printing gate 8's refusal here would
    tell an author their diagnostics leak file names when nothing of the sort
    was observed. Nothing was observed at all.

    Not a heuristic threshold. Gate 4's equivalent originally asserted
    `> 50` files and could therefore only ever be run in *this* checkout; this
    asks for one, which is a claim about the gate having a subject rather than
    about the size of this repository, and holds in a three-file fixture.
    """
    if seen.scanned:
        return False
    where = f"the tree at {head}" if head else "the working tree"
    print(
        f"quote-names: no Rust source under {'/, '.join(ROOTS)}/ in {where}.\n"
        "This gate has lost its subject, not found it clean -- refusing to\n"
        "report a pass (or to rewrite the baseline) from a tree it cannot see.",
        file=sys.stderr,
    )
    return True


def survey(root: Path = ROOT) -> Survey:
    """Every flagged line in lane B's working tree."""
    with gittree.WorkTree(str(root)) as tree:
        return survey_tree(tree)


def survey_at(sha: str, root: Path = ROOT) -> Survey:
    """`survey()`, but reading the tree at `sha` instead of the working tree.

    This is what makes the pre-push gate judge *what is being published*. The
    worktree survey answers a different question, and wrong in the dangerous
    direction: a commit that adds an unquoted name passes if the worktree has
    since fixed it, and the commit is published anyway. It also has the
    mirror-image false positive, where an unrelated uncommitted edit blocks a
    push of clean commits.
    """
    with gittree.RevTree(sha, str(root)) as tree:
        return survey_tree(tree)


def read_baseline_from(tree: gittree.Tree) -> dict[str, int] | None:
    """The baseline as it stands in `tree`, or `None` if that tree has none.

    It has to move with the tree. The baseline is the ratchet, so judging a
    commit's files against a *different* commit's baseline reports the
    difference between the two revisions rather than anything about the commit
    -- which is loudest exactly when it is least useful: a push whose first
    commit fixes sites and whose second records them in the baseline would have
    the first commit judged against the not-yet-updated numbers.

    `None` rather than `{}` for an absent file, and the distinction is the
    whole of `_no_baseline` below. This used to return `{}` with a comment
    calling it "the safe direction -- it can only over-report", which is the
    argument `run-checker.sh` exists to reject: over-reporting is not the safe
    direction, it is a false accusation, and it is what gets a gate bypassed.
    An empty allowance and a missing one are also not distinguishable after the
    fact -- the live baseline is currently empty of entries, so `{}` is a real
    and correct value that the caller must be able to tell apart from a file
    that was never read.
    """
    text = _decode(tree.read_bytes(BASELINE_REL))
    if text is None:
        return None
    return _parse_baseline(text)


def _no_baseline(baseline: dict[str, int] | None, head: str | None) -> bool:
    """Whether the ratchet's own record is missing from the tree -- and say so.

    The mirror of `_no_corpus`, failing the other way round. A missing corpus
    goes silent; a missing baseline goes *loud and wrong*: every site the real
    file forgives reads as brand new, and the author is handed gate 8's whole
    refusal over 1798 diagnostics across 777 files they did not touch. On a
    clean tree the same read calls every entry stale instead, which prints as
    "the backlog is fixed" over a commit that fixed nothing.

    Exit 2 for `_no_corpus`'s reason: 1 is "the checker found something in your
    code", and nothing here was found in anybody's code.

    Gates 4 and 6 both carry this guard. Gate 8 shipped its `--head`
    conversion on 2026-09-02 with the corpus half and not this half, and it
    stayed that way until a behavioural case was finally written for the gate
    on 2026-09-04 and came back red. That is the argument for the case, not
    just for the guard: the defect had been asserted-around for two days by a
    test that checked the *wiring* and never the verdict.
    """
    if baseline is not None:
        return False
    where = f"the tree at {head}" if head else "the working tree"
    print(
        f"quote-names: {BASELINE_REL} does not exist in {where}.\n"
        "That file is the ratchet itself, and reading it as an empty allowance\n"
        "would accuse every already-known site of being new. Refusing to\n"
        "report a verdict rather than report that one.\n"
        "(To create it from scratch: --write-baseline, without --head.)",
        file=sys.stderr,
    )
    return True


def read_baseline_at(sha: str, root: Path = ROOT) -> dict[str, int] | None:
    """The baseline recorded at `sha`."""
    with gittree.RevTree(sha, str(root)) as tree:
        return read_baseline_from(tree)


def read_baseline() -> dict[str, int] | None:
    """`path -> count` from the baseline file, `#` comments stripped."""
    if not BASELINE.is_file():
        return None
    return _parse_baseline(BASELINE.read_text(encoding="utf-8"))


def _parse_baseline(text: str) -> dict[str, int]:
    out: dict[str, int] = {}
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        path, _, count = line.rpartition(":")
        if not path or not count.isdigit():
            print(f"malformed baseline line: {line!r}", file=sys.stderr)
            continue
        out[path] = int(count)
    return out


def write_baseline(found: dict[str, list[tuple[int, str, str]]]) -> None:
    body = [
        "# Diagnostics that put a file name into the message without routing it",
        "# through `quote` -- one line per source file, with the number of sites",
        "# in it. Generated by `scripts/quote-names.py --write-baseline`; see that",
        "# script's docstring for why the key is a count and not a line number.",
        "#",
        "# THIS FILE IS A RATCHET AND ONLY EVER SHRINKS. Each site is a place",
        "# where a hostile file name can forge a line of this program's stderr.",
        "# The fix is one call: `quotef_os(path)` (or `quoteaf_os` where the name",
        "# is always quoted) from the crate's `quoting` module.",
        "#",
        "# Do NOT raise a number, and do NOT add a file, to turn a red `--check`",
        "# green: that is the defect being recorded, not an exception to it.",
        "#",
        "# There is exactly one legitimate reason a number here may go UP: the",
        "# detector was corrected and now sees sites it used to miss. That is a",
        "# commit which changes `quote-names.py` and no `.rs` file under the",
        "# scanned roots. If a number rises in a commit that also edits code,",
        "# the code is what raised it.",
        "#",
        "# It has happened three times.",
        "#",
        "#   2026-08-23  Calls rustfmt had wrapped onto two lines were invisible.",
        "#               71 sites.",
        "#   2026-09-10  `{}` with the name in the argument list counted as well",
        "#               as `{name}` captured inline. 224 sites.",
        "#   2026-09-11  The gate matched `eprintln!` and, in one predicate of",
        "#               three, `println!`. It never matched `format!`. A message",
        "#               does not stop being a message because it is assembled",
        "#               before it is printed: 398 of the sites this found are",
        "#               inside `Err(..)`, `map_err(..)` or `ok_or(..)`. Found",
        "#               because efibootmgr was given three diagnostics of one",
        "#               shape in one commit and this gate refused one of them.",
        "#               507 sites, and the ledger had been at ZERO.",
        "#",
        "# That last line is the one to read twice. This file said the tree was",
        "# clean the day before, and the tree was not clean; it was unexamined in",
        "# a macro nobody had thought to look in. An empty ratchet is a claim",
        "# about the detector as much as about the code.",
        "#",
        f"# {sum(len(v) for v in found.values())} sites across {len(found)} files.",
        "",
    ]
    body += [f"{path}:{len(hits)}" for path, hits in sorted(found.items())]
    # newline="" stops Python translating "\n" to "\r\n" on Windows. Git
    # normalises it on commit either way, so without this the file on disk
    # differs from the file in the index and every checkout shows it dirty.
    BASELINE.write_text("\n".join(body) + "\n", encoding="utf-8", newline="")
    total = sum(len(v) for v in found.values())
    print(f"wrote {BASELINE.name} with {len(found)} files, {total} sites")


def selftest() -> int:
    """Check the rule that decides what this tool reports.

    A detector that fails toward silence looks exactly like a clean tree, and
    this one is unusually exposed to that: its signal lives *inside* a string
    literal, so the obvious lexer to reach for blanks out precisely the text
    being searched. The cases below are the ones that a rewrite would break
    first -- and each of them is a shape that exists in the tree today.
    """
    failures: list[str] = []
    checked = 0

    def expect(label: str, src: str, want: int) -> None:
        nonlocal checked
        checked += 1
        got = len(violations(src))
        if got != want:
            failures.append(f"{label}: want {want}, got {got}\n    {src}")

    def expect_join(label: str, src: str, want_in: str) -> None:
        """Assert the *reassembled* text, not just the count.

        A count says the site was seen; it says nothing about whether the text
        that was seen is the text the compiler sees. `--fix` writes this string
        back to disk, so a join that is off by one space is a source edit that
        is off by one space.
        """
        nonlocal checked
        checked += 1
        joined = " || ".join(s for _f, _l, s in join_wrapped_calls(src))
        if want_in not in joined:
            failures.append(f"{label}: {want_in!r} not in {joined!r}")

    # 1. The base case, in the exact shape the recorded sites are written in.
    # -- the positional spelling of the same defect ------------------------
    #
    # Invisible to this gate until 2026-09-10. It was found because a one-line
    # fix replacing `{}` + argument with an inline `{file}` capture was REFUSED
    # by the gate, while the line it replaced had always passed. Seven sites
    # were in the tree; the burn-down that reported "0 remain" had been
    # counting one of the two ways to write it.
    expect("positional", 'eprintln!("cut: {}: {e}", path.display());', 1)
    expect("positional-verb-tail",
           'eprintln!("scp: {}: cannot read symlink: {e}", p.display());', 1)
    # Already routed through the helpers: the whole point, not a violation.
    expect("positional-quoted", 'eprintln!("cut: {}: {e}", quotef_os(path));', 0)
    expect("positional-quoted-a", 'eprintln!("cut: {}: {e}", quoteaf_os(path));', 0)
    # `.display()` is what identifies a file name. Without that qualifier this
    # would flag every integer and enum in a `{}`, and a gate that cries wolf
    # gets switched off.
    expect("positional-not-a-path", 'eprintln!("cut: {}: {e}", n);', 0)
    expect("positional-count", 'println!("copied {} files", count);', 0)
    # The program-prefix rule keeps it off prose, exactly as for the inline
    # shape.
    expect("positional-prose",
           'eprintln!("Some prose: {}: here", path.display());', 0)
    # An escaped quote inside the format string must not end the scan early --
    # if it does, the argument list is read from inside the literal. The
    # mirror-image bug hit `strip_noise` in check-read-defaults.py twice this
    # week, so it is pinned rather than trusted.
    expect("positional-escaped-quote",
           r'eprintln!("cut: {}: said \"no\"", path.display());', 1)

    expect("bare", 'eprintln!("cut: {path}: {e}");', 1)
    expect("bare-nested-prog", 'eprintln!("tar_x: {name}: {e}");', 1)
    expect("digit-in-prog", 'eprintln!("b2sum: {f}: {e}");', 1)

    # 2. The fix must not match, or the tool flags its own remedy.
    expect("quoted", 'eprintln!("cut: {}: {e}", quotef_os(path));', 0)
    expect("quoted-always", 'eprintln!("cut: {}: {e}", quoteaf_os(path));', 0)

    # 3. A message is not a name: re-quoting rendered text would be wrong, and
    #    flagging it would bury the real hits in noise.
    for ident in sorted(NOT_A_NAME):
        expect(f"not-a-name-{ident}", f'eprintln!("cut: {{{ident}}}: rest");', 0)

    # 4. Shapes that look like the pattern but are not it. Each of these
    #    over-matching would put a false positive into a baseline of hundreds
    #    of lines, where nobody would ever find it again.
    expect("no-trailing-colon", 'eprintln!("cut: {path} is a directory");', 0)
    expect("uppercase-prog", 'eprintln!("Cut: {path}: {e}");', 0)
    expect("empty-prog", 'eprintln!(": {path}: {e}");', 0)
    expect("not-an-ident", 'eprintln!("cut: {path.display()}: {e}");', 0)
    # This case used to want 0, on the reading that stdout is not a diagnostic
    # channel. Two things overturned it. The weaker: section 5 below already
    # wanted 1 for `println!("cut: '{path}'")`, so the file contradicted itself
    # and the contradiction WAS the drift -- one predicate had been widened to
    # `println!` and the other two had not. The stronger: what this predicate
    # matches is not a channel, it is a shape. `<prog>: {name}: <reason>` is
    # the diagnostic form; a program printing one to stdout is a second bug,
    # not an exemption from this one, and the newline in `name` forges a line
    # of output either way.
    expect("bare name on stdout is still a diagnostic", 'println!("cut: {path}: {e}");', 1)

    # 5. Hand-written quotes: the shape that looks fixed and is not.
    expect("hand-quotes", "eprintln!(\"cut: '{path}': {e}\");", 1)
    expect("hand-quotes-stdout", "println!(\"cut: '{path}'\");", 1)
    # This case used to want 0, under the label `hand-quotes-no-macro`: the
    # reading was that `format!` does not print, so it is not a diagnostic.
    # That is true of the macro and false of the tree. Measured on 2026-09-11:
    # 507 sites, 398 of them inside `Err(..)`, `map_err(..)` or `ok_or(..)`,
    # and most of the rest `SomeError::Variant(format!(..))` -- a message with
    # a type around it, printed by whatever `main` catches it. `userspace/rsync`
    # alone holds 32 of `format!("cannot open '{}': {e}", path.display())`.
    #
    # The cost is admitted rather than argued away: nothing in the syntax
    # separates a `format!` that builds a message from one that builds data,
    # so this predicate cannot be precise. The two mitigations are the
    # test-code exclusion (case 13) and the IGNORE table, which records a
    # reason where a baseline entry would record only a number.
    expect("hand-quotes-in-format", "let s = format!(\"'{path}'\");", 1)

    # 5a. A *doubled* brace is Rust's escape for a literal one, so quotes around
    #     it wrap printed text, not a name. Help text full of JSON examples is
    #     the shape this arises in, and it is unfixable by construction: there
    #     is no name to quote, so a report here is a permanent decline.
    expect(
        "escaped-braces-in-help-text",
        'println!("    aws events put-rule --event-pattern \'{{\\"source\\":[\\"aws.s3\\"]}}\'");',
        0,
    )
    expect(
        "escaped-braces-then-a-real-name",
        "eprintln!(\"cut: {{literal}} '{path}': {e}\");",
        1,
    )
    # A brace belonging to enclosing code must not consume the real placeholder
    # that follows it -- the scan starts at the macro's quote for this reason.
    expect("code-brace-before-the-call", "if x { println!(\"a '{p}'\"); }", 1)

    # 6. Prose is not code. Seven doc-comments in this tree quote the bad form
    #    in order to warn about it; counting them would overstate the backlog
    #    and, worse, make the backlog unfixable -- you cannot repair a comment.
    expect("line-comment", '// eprintln!("cut: {path}: {e}");', 0)
    expect("doc-comment", '/// eprintln!("cut: {path}: {e}");', 0)
    expect("module-doc", '//! eprintln!("cut: {path}: {e}");', 0)

    # 7. Multi-line input, since that is what a file is.
    expect(
        "two-in-one-file",
        'eprintln!("cut: {path}: {e}");\nlet x = 1;\neprintln!("cut: {name}: {e}");',
        2,
    )

    # 8. Calls rustfmt has wrapped. This is not a hypothetical shape: running
    #    `cargo fmt` over five untouched crates in this tree moved three real
    #    violations onto two lines each and they vanished from the count. A
    #    checker that a formatter can silence reports a clean tree for a dirty
    #    one, which is the single worst thing this tool can do.
    expect(
        "wrapped-fmt-on-own-line",
        'eprintln!(\n    "cut: {path}: {e}"\n);',
        1,
    )
    expect(
        "wrapped-args-on-own-line",
        'eprintln!(\n    "lp: printer \'{p}\' not found: {e}",\n    x,\n);',
        1,
    )
    expect(
        "wrapped-counts-once-not-per-line",
        'eprintln!(\n    "cut: {path}: {e}"\n);\nlet y = 2;',
        1,
    )
    #    ... and the join must not swallow the lines after a call it cannot
    #    parse, or one unrecognised line would hide every violation below it.
    expect(
        "unterminated-does-not-swallow",
        'let s = "oops;\neprintln!("cut: {path}: {e}");',
        1,
    )
    #    Brackets inside the format string are text, not structure. `job(s)`
    #    appears verbatim in `cancel`'s messages, and counting its parens
    #    would leave the call permanently unbalanced.
    expect(
        "parens-inside-format-string",
        'println!("cancel: purged {n} job(s) on \'{p}\'");',
        1,
    )
    #    A literal broken with a trailing `\` is Rust's line continuation: the
    #    backslash, the newline and the next line's indent all vanish from the
    #    string. Joining that boundary with a space instead put `\ ` -- not a
    #    valid escape -- in the middle of `diskutil`'s message, and because
    #    `--fix` writes the joined text back, it stopped the crate compiling.
    cont = (
        'eprintln!(\n'
        '    "diskutil: cannot format \'{other}\' yet -- only the FAT \\\n'
        '     family has a backend"\n'
        ');'
    )
    expect("continuation-is-still-detected", cont, 1)
    expect_join(
        "continuation-joins-without-a-space",
        cont,
        '"diskutil: cannot format \'{other}\' yet -- only the FAT family has a '
        'backend"',
    )
    #    ... but an *escaped* backslash at end of line is a real backslash and
    #    ends the line for real, so that boundary keeps its separator.
    expect_join(
        "escaped-backslash-is-not-a-continuation",
        'eprintln!(\n    "a\\\\",\n    x\n);',
        '"a\\\\", x',
    )

    # 9. The rewriter. A fixer that is wrong is worse than no fixer: it edits
    #    775 files unattended, and a bad transform lands as a compile error at
    #    best and a mangled message at worst. Each case below asserts the exact
    #    output, and the `None` cases assert that it *declined* -- silence
    #    where a rewrite should have happened is the failure that hides.
    def expect_fix(label: str, src: str, want: str | None) -> None:
        nonlocal checked
        checked += 1
        got, reason = fix_line(src)
        if got != want:
            failures.append(f"fix-{label}: want {want!r}, got {got!r} ({reason})\n    {src}")

    expect_fix(
        "inline-one",
        "        eprintln!(\"lp: printer '{pname}' not found\");",
        '        eprintln!("lp: printer {} not found", quoteaf_os(&pname));',
    )
    expect_fix(
        "inline-two",
        "    println!(\"Playing '{a}' on '{b}'...\");",
        '    println!("Playing {} on {}...", quoteaf_os(&a), quoteaf_os(&b));',
    )
    expect_fix(
        "inline-alongside-unquoted-capture",
        "eprintln!(\"{cmd}: printer '{name}' not found\");",
        'eprintln!("{cmd}: printer {} not found", quoteaf_os(&name));',
    )
    expect_fix(
        "positional-one",
        "eprintln!(\"lp: invalid copies value '{}'\", args[i]);",
        'eprintln!("lp: invalid copies value {}", quoteaf_os(&args[i]));',
    )
    expect_fix(
        "bare-name",
        '    eprintln!("cut: {path}: {e}");',
        '    eprintln!("cut: {}: {e}", quotef_os(&path));',
    )
    # A match arm is a call too, and it ends in `,` rather than `;`. Five of
    # `influx-cli`'s nine sites are this shape and were reported as "not a
    # single-line call" -- a decline that looks like a hard case and is not.
    expect_fix(
        "match-arm-keeps-its-comma",
        "        _ => println!(\"influx: '{}' completed\", sub),",
        '        _ => println!("influx: {} completed", quoteaf_os(&sub)),',
    )
    expect_fix(
        "match-arm-with-a-pattern",
        "    Cmd::Get(k) => eprintln!(\"db: key '{k}' missing\"),",
        '    Cmd::Get(k) => eprintln!("db: key {} missing", quoteaf_os(&k)),',
    )
    # The arm pattern is itself a string literal. This is how every option
    # dispatcher in the CLI wrappers is written -- four of abduco-cli's five
    # sites -- and it is the case that forced `lead` to be able to hold a
    # quote at all.
    expect_fix(
        "match-arm-whose-pattern-is-a-literal",
        "        \"-a\" => println!(\"abduco: session '{}'\", name),",
        '        "-a" => println!("abduco: session {}", quoteaf_os(&name)),',
    )
    expect_fix(
        "match-arm-with-an-or-pattern",
        "        \"-c\" | \"-n\" => println!(\"abduco: new '{}'\", name),",
        '        "-c" | "-n" => println!("abduco: new {}", quoteaf_os(&name)),',
    )
    expect_fix(
        "match-arm-whose-pattern-is-a-char",
        "        'q' => println!(\"pager: quit at '{}'\", pos),",
        "        'q' => println!(\"pager: quit at {}\", quoteaf_os(&pos)),",
    )
    # The reason a quote in `lead` is dangerous, pinned down: a pattern that
    # *contains the text of the call* must not let the match start inside it.
    # A literal is consumed whole or not at all, so the real call still wins.
    expect_fix(
        "arm-pattern-containing-the-macro-text",
        "        \"say println!(\" => println!(\"tool: got '{}'\", w),",
        '        "say println!(" => println!("tool: got {}", quoteaf_os(&w)),',
    )
    # An arm whose body is a *block*, which is how a wrapper returns an exit
    # code from the same arm that prints the complaint. Nine sites in this tree
    # are this line with a different program name in it.
    expect_fix(
        "match-arm-with-a-block-body",
        "        other => { eprintln!(\"npm: unknown command '{}'\", other); 1 }",
        '        other => { eprintln!("npm: unknown command {}", quoteaf_os(&other)); 1 }',
    )
    # The brace and the text after the terminator are copied through, never
    # parsed, so the rewrite is exactly as safe when they do not pair up.
    expect_fix(
        "trailing-brace-without-a-leading-one",
        "            eprintln!(\"npm: bad '{}'\", other); }",
        '            eprintln!("npm: bad {}", quoteaf_os(&other)); }',
    )
    # Declines. Each is a real shape in the tree, and each would be corrupted
    # by a rewrite that went ahead anyway.
    expect_fix("declines-multiline", "eprintln!(\"lp: printer '{p}' not\"", None)
    expect_fix(
        "declines-existing-positional",
        "eprintln!(\"lp: {} wants '{p}'\", n);",
        None,
    )
    # This one used to be a decline, under the blanket comment above that
    # every case in this group "would be corrupted by a rewrite that went
    # ahead anyway". That is true of the other three and was never true of
    # this one: two quoted placeholders and two arguments map in order before
    # the rewrite and in the same order after it, so nothing moves. It was
    # declined by a `positional == 1` guard and the comment was written over
    # the whole group. The shape is real -- `rsync` has
    # `format!("symlink '{}' -> '{}': {e}", dst.display(), target.display())`
    # -- and refusing it sent a fixable line to be done by hand.
    #
    # What still declines is `"a '{}' b {}"`, where a quoted placeholder and a
    # bare one share the argument list: the count no longer identifies which
    # argument is the name, and guessing would swap two values in a message
    # while every test went on passing. That is `fix-declines-mixed-positional`
    # below.
    expect_fix(
        "fixes-two-positional-quotes-because-they-map-in-order",
        "eprintln!(\"lp: '{}' and '{}'\", a, b);",
        'eprintln!("lp: {} and {}", quoteaf_os(&a), quoteaf_os(&b));',
    )
    # A quoted slot sharing an argument list with a bare one. This was a
    # decline for one commit, on the reading that the count no longer says
    # which argument is the name. POSITION says it: slot 1 is quoted so
    # argument 1 is the name, slot 2 is not so argument 2 is left alone. It is
    # the single commonest shape left in the tree --
    # `format!("Cannot read '{}': {}", path.display(), e)`, 34 sites -- and
    # counting rather than positioning is what made it look unfixable.
    expect_fix(
        "a-quoted-slot-beside-a-bare-one-maps-by-position",
        "eprintln!(\"lp: '{}' wants {}\", a, n);",
        'eprintln!("lp: {} wants {}", quoteaf_os(&a), n);',
    )
    # `{name}` captures from scope and consumes NO argument, so it must not be
    # counted as a slot. Counting it would shift every argument after it.
    expect_fix(
        "a-named-placeholder-is-not-a-slot",
        "eprintln!(\"lp: {prog} '{}' at {n}\", a);",
        'eprintln!("lp: {prog} {} at {n}", quoteaf_os(&a));',
    )
    # An argument that is a CALL may already be a rendering. `cal`'s
    # `shown(arg)` is `escape_unprintable(..)`, so wrapping it would escape the
    # backslashes a second time and print \\012 where the name held a newline.
    expect_fix(
        "declines-a-name-that-is-a-call-result",
        "eprintln!(\"cal: '{}'\", shown(arg));",
        None,
    )
    # `'{:?}'` is not this defect: `Debug` escapes the value already. Stripping
    # the quotes would leave a placeholder the rewrite had not repaired.
    expect_fix(
        "declines-a-quoted-slot-with-a-format-spec",
        "eprintln!(\"seq: near '{:?}'\", tok);",
        None,
    )
    # A brace escape consumes nothing and must not be read as a slot.
    expect_fix(
        "brace-escapes-are-not-slots",
        "eprintln!(\"aws: {{json}} '{}'\", a);",
        'eprintln!("aws: {{json}} {}", quoteaf_os(&a));',
    )
    expect_fix(
        "declines-string-literal-arg",
        "eprintln!(\"lp: '{}'\", x.unwrap_or(\", \"));",
        None,
    )
    # A wrapped call is rewritten as one line, from its own indentation.
    expect_fix(
        "wrapped-is-rejoined",
        '    eprintln!( "lp: printer \'{p}\' not found", );',
        '    eprintln!("lp: printer {} not found", quoteaf_os(&p));',
    )
    expect_fix(
        "wrapped-positional-is-rejoined",
        'eprintln!( "lp: bad value \'{}\'", args[i], );',
        'eprintln!("lp: bad value {}", quoteaf_os(&args[i]));',
    )
    # The fixed form must not be a violation any more, or --fix would loop.
    expect("fix-is-clean-inline", 'eprintln!("lp: printer {} not found", quoteaf_os(&p));', 0)
    expect("fix-is-clean-bare", 'eprintln!("cut: {}: {e}", quotef_os(&path));', 0)

    # 10. End to end: a wrapped violation must survive the round trip through
    #     `fix_file`'s span replacement and come back clean, since that is the
    #     path every one of the ~1700 sites will actually take.
    import contextlib
    import io
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        probe = Path(td) / "probe.rs"
        probe.write_text(
            'fn f() {\n    eprintln!(\n        "lp: printer \'{p}\' not found"\n    );\n}\n',
            encoding="utf-8",
            newline="",
        )
        n, left = fix_file(probe)
        after = probe.read_text(encoding="utf-8")
        checked += 1
        if n != 1 or left or violations(after):
            failures.append(
                f"round-trip: fixed={n} left={left} still={violations(after)}\n    {after!r}"
            )

    # 11. `--head` must read the COMMIT, not the worktree.
    #
    #     This is the one case that cannot be written as a string-in/count-out
    #     assertion, and it is also the only one that measures the reason
    #     `--head` exists. The shape is the staged-restore: a commit introduces
    #     a violation, the worktree then repairs it without committing, and the
    #     commit is pushed anyway. A worktree survey calls that clean. If this
    #     case ever passes with `survey_at` delegating to `survey`, the gate has
    #     silently gone back to answering the wrong question.
    bad = 'fn f() {\n    eprintln!("cut: \'{p}\': no such file");\n}\n'
    good = 'fn f() {\n    eprintln!("cut: {}: no such file", quotef_os(&p));\n}\n'

    with tempfile.TemporaryDirectory() as td:
        repo = Path(td)
        src = repo / "userspace" / "probe" / "src"
        src.mkdir(parents=True)
        rs = src / "main.rs"

        def git(*a: str) -> bytes:
            # Identity and signing are forced off for this throwaway repo only:
            # the host's real config may have `commit.gpgsign=true`, and a
            # signing prompt inside a pre-push hook is an unkillable hang.
            #
            # `env=gitenv.clean_env()` is the load-bearing argument, not the
            # `-C`. Git exports `GIT_DIR` into every hook, and this self-test is
            # run from `pre-push`; an inherited one outranks `-C` outright, so
            # without the scrub every command below operates on the repository
            # being pushed. On 2026-09-04 they did: `git init` re-initialised
            # lane B, `git add -A` replaced its index with this one file, and
            # two commits landed on the branch whose tree was then a single
            # `userspace/probe/src/main.rs`. The assertions still passed --
            # `survey_at(sha, repo)` was reading that same real repository, so
            # the fixture agreed with itself about the wrong tree. Recovered
            # with a mixed reset to `f129bd5e0`; nothing reached the remote.
            # `scripts/gitenv.py` records the identical 2026-08-29 incident,
            # which is the one this should have been written from.
            return subprocess.run(
                ["git", "-C", str(repo), "-c", "user.email=selftest@invalid",
                 "-c", "user.name=selftest", "-c", "commit.gpgsign=false", *a],
                check=True, capture_output=True, env=gitenv.clean_env(),
            ).stdout

        def guard(seen: Survey, at: str | None) -> tuple[bool, str]:
            """`_no_corpus`, with its diagnostic captured rather than printed.

            Returned rather than swallowed: the refusal's *wording* is half of
            what this guard does. A gate that stops the push but says only
            "0 known sites" has moved the failure from the ratchet to whoever
            has to work out why the push died.
            """
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                return _no_corpus(seen, at), err.getvalue()

        # What the ambient repository looked like before the fixture ran. Any
        # difference afterwards means these commands went somewhere they were
        # not pointed -- which is the failure that has now happened twice, in
        # two different scripts, and which no assertion about the fixture's
        # *contents* can detect, because a redirected fixture is perfectly
        # self-consistent.
        before = _ambient_head()

        try:
            git("init", "-q")
            rs.write_text(bad, encoding="utf-8", newline="")
            git("add", "-A")
            git("commit", "-qm", "introduce the violation")
            sha = git("rev-parse", "HEAD").decode().strip()

            # The repair that never gets committed.
            rs.write_text(good, encoding="utf-8", newline="")

            checked += 1
            worktree_hits = sum(len(v) for v in survey(repo).found.values())
            commit_hits = sum(len(v) for v in survey_at(sha, repo).found.values())
            if worktree_hits != 0 or commit_hits != 1:
                failures.append(
                    "head-reads-the-commit: worktree should see 0 and the commit 1, "
                    f"got worktree={worktree_hits} commit={commit_hits}"
                )

            # The mirror image: an uncommitted violation must be invisible to
            # `--head`, so unrelated dirty work cannot block a clean push.
            # (`good` is already on disk from the case above, so this commit
            # is the repair; committing `bad` again would be a no-op and git
            # would exit 1 on the empty commit.)
            git("add", "-A")
            git("commit", "-qm", "the repair, committed this time")
            clean_sha = git("rev-parse", "HEAD").decode().strip()
            rs.write_text(bad, encoding="utf-8", newline="")  # dirty, uncommitted

            checked += 1
            dirty_hits = sum(len(v) for v in survey(repo).found.values())
            clean_hits = sum(len(v) for v in survey_at(clean_sha, repo).found.values())
            if dirty_hits != 1 or clean_hits != 0:
                failures.append(
                    "head-ignores-the-worktree: the commit should see 0 and the "
                    f"dirty worktree 1, got worktree={dirty_hits} commit={clean_hits}"
                )

            # 12. A tree with no sources is not a clean tree.
            #
            #     `clean_sha` is the control, and it carries as much weight as
            #     the experiment below: its tree *has* a source and no
            #     violations, which is exactly the state a healthy repository
            #     is in. A guard that fired here would be the failure the
            #     other one is not -- a gate that refuses on every host, which
            #     gets switched off within a day and then protects nothing.
            checked += 1
            clean_seen = survey_at(clean_sha, repo)
            if clean_seen.found or clean_seen.scanned != 1:
                failures.append(
                    "corpus-control: a clean one-file tree should scan 1 file and "
                    f"flag none, got scanned={clean_seen.scanned} "
                    f"found={sorted(clean_seen.found)}"
                )

            checked += 1
            fired, _ = guard(clean_seen, clean_sha)
            if fired:
                failures.append(
                    "corpus-control: the guard fired on a tree that has a subject "
                    "and is merely clean"
                )

            # The experiment: the same repository with its corpus removed. This
            # is the shape of the real event -- a commit that renames or moves
            # lane B's zone -- reduced to the one property that matters, which
            # is that nothing is left for this gate to read.
            git("rm", "-r", "-q", "-f", "userspace")
            git("commit", "-qm", "the corpus goes away")
            gone_sha = git("rev-parse", "HEAD").decode().strip()

            checked += 1
            gone = survey_at(gone_sha, repo)
            if gone.scanned or gone.found:
                failures.append(
                    "corpus-gone: the fixture did not actually remove the corpus, "
                    f"scanned={gone.scanned} found={sorted(gone.found)}"
                )

            checked += 1
            fired, said = guard(gone, gone_sha)
            if not fired or "lost its subject" not in said or gone_sha not in said:
                failures.append(
                    "corpus-guard: an empty corpus must be refused, and the "
                    "refusal must name the tree it read and say the subject is "
                    f"missing rather than clean. fired={fired} said={said!r}"
                )

            # Why the guard has to run *before* `check` rather than inside it:
            # on its own, `check` reads the loss of its own subject as a
            # burn-down and exits 0, inviting a `--write-baseline` that would
            # discard the entire ratchet. That is asserted here, deliberately,
            # as the current behaviour of `check` -- not as behaviour anyone
            # should preserve. If a later change teaches `check` this rule
            # itself, this case fails: delete it *and* `_no_corpus` together,
            # rather than keeping two answers to one question.
            checked += 1
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                rc = check(gone.found, {"userspace/probe/src/main.rs": 1})
            said = out.getvalue()
            if rc != 0 or "1 improved" not in said or "fixed:" not in said:
                failures.append(
                    "corpus-guard-is-load-bearing: `check` alone no longer reads "
                    f"an emptied corpus as progress (rc={rc} said={said!r}). If "
                    "that is now handled inside `check`, remove `_no_corpus` and "
                    "this case together."
                )
        except (subprocess.CalledProcessError, OSError, gittree.GitTreeError) as e:
            # `GitTreeError` belongs here for a reason worth stating: it is what
            # a *half*-repaired version of this bug raises. If the fixture's
            # commits go to the ambient repository while `gittree` correctly
            # reads the temp directory, the sha exists in neither place either
            # side is looking, and `ls-tree` exits 128. Uncaught, that kills the
            # whole self-test with a traceback -- taking the other 58 cases with
            # it and, worse, skipping the ambient-HEAD check below, which is the
            # one line that would have named the actual cause. Caught, the case
            # fails and the diagnosis still prints.
            checked += 1
            failures.append(f"head-selftest could not drive git: {e}")

        checked += 1
        after = _ambient_head()
        if before != after:
            failures.append(
                "the fixture wrote to the repository this script lives in: its "
                f"HEAD was {before!r} before the case and is {after!r} after. "
                "The git commands above are not reaching the temp directory."
            )

    # 12. `format!` builds messages too, and this gate could not see one.
    #
    #     Every case above is an `eprintln!` -- which is how the gate came to
    #     match only that macro, and how it stayed that way. On 2026-09-11
    #     `userspace/efibootmgr` was given three diagnostics of one shape in
    #     one commit; the push was refused over the `eprintln!` and the two
    #     that `format!` built a call earlier went through, in the same file,
    #     under the same review. The first four cases here fail against the
    #     detector as it stood that morning.
    expect(
        "format! with hand-written quotes",
        """fn f() -> String { format!("unrecognized argument '{arg}'") }""",
        1,
    )
    expect(
        "format! into Err -- 398 of the 507 sites are written this way",
        """fn f() { return Err(format!("cannot open '{}': {e}", path.display())); }""",
        1,
    )
    expect(
        "format! carrying a bare name",
        """fn f() -> String { format!("rsync: {path}: {e}") }""",
        1,
    )
    expect(
        "println! carrying a bare name -- two of the three predicates missed it",
        """fn f() { println!("ls: {path}: {e}"); }""",
        1,
    )
    #     The remedy must still not read as the defect, in the new macro as in
    #     the old one. A gate that flags its own fix is a gate that gets turned
    #     off.
    expect(
        "format! already routed through the quoting helpers",
        """fn f() -> String { format!("cut: {}: {e}", quotef_os(path)) }""",
        0,
    )

    # 13. Test code is not a diagnostic.
    #
    #     This gate was the only one of the eleven `rustlex` exists for that
    #     read `#[cfg(test)]` as production code. That was harmless while it
    #     saw only `eprintln!` and stopped being harmless the moment it saw
    #     `format!`: `userspace/oils` is a shell, and its fixtures build shell
    #     source with `format!("eval '{src}'")`. Eleven of its sixteen sites
    #     were exactly that, and they would have entered the ledger as defects
    #     nobody could ever fix -- a ratchet whose floor is unreachable stops
    #     being read, which is the reason the IGNORE table exists at all.
    #
    #     The blanking belongs to `survey_tree` rather than to `violations`,
    #     so what is asserted here is the composition the survey performs.
    fixture = (
        """fn f() -> String { format!("rsync: cannot open '{p}'") }\n"""
        "#[cfg(test)]\n"
        "mod tests {\n"
        """    fn t() { let _ = format!("eval '{src}'"); }\n"""
        "}\n"
    )
    checked += 1
    live = violations(live_code(fixture)[0])
    if [(n, w) for n, w, _s in live] != [(1, "hand-written quotes")]:
        failures.append(
            f"test-code exclusion: want one site on line 1, got {live}. "
            "Either the fixture entered the ledger, or `live_code` cut the "
            "file instead of blanking it and the line numbers no longer point "
            "at the line a reader opens."
        )

    # 14. The rewriter reaches `format!`, which means reaching a call that is
    #     an EXPRESSION rather than a statement. Every case in section 9 is
    #     anchored at both ends of its line; none of these is, and all of them
    #     are shapes I fixed by hand in `rsync` and `nftables` before making
    #     the tool do it -- twice being the point at which an ad-hoc transform
    #     should stop being ad-hoc.
    expect_fix(
        "format-inside-a-match-arm",
        "        _ => Err(format!(\"unknown family '{s}'\")),",
        '        _ => Err(format!("unknown family {}", quoteaf_os(&s))),',
    )
    expect_fix(
        "format-inside-a-closure-with-a-tail",
        "    .map_err(|e| format!(\"read '{}': {e}\", path.display()))?;",
        '    .map_err(|e| format!("read {}: {e}", quoteaf_os(&path)))?;',
    )
    #     `.display()` comes OFF rather than being wrapped. `quoteaf_os(&x.display())`
    #     does not compile -- `std::path::Display` is not `AsRef<OsStr>` -- so a
    #     fixer that kept it would emit code that fails to build, and the lossy
    #     rendering is half of what is being repaired anyway.
    expect_fix(
        "renderer-comes-off-rather-than-being-wrapped",
        'return Err(format!("no input from \'{}\'", list.to_string_lossy()));',
        'return Err(format!("no input from {}", quoteaf_os(&list)));',
    )
    #     The colon form, which nothing rewrote before: the placeholder count
    #     does not change, only the argument, so it is the one shape with no
    #     renumbering hazard at all.
    expect_fix(
        "colon-form-swaps-the-argument-only",
        'let m = format!("scp: {}: {e}", src.display());',
        'let m = format!("scp: {}: {e}", quotef_os(&src));',
    )
    #     An inline capture next to a quoted one takes no positional slot, so
    #     appending is still safe -- this is the check that let 59 nftables
    #     lines be rewritten rather than refused.
    expect_fix(
        "an-inline-capture-alongside-does-not-renumber",
        "    .ok_or_else(|| format!(\"table '{name}' missing in family {family}\"))",
        '    .ok_or_else(|| format!("table {} missing in family {family}", quoteaf_os(&name)))',
    )
    #     And the literal hazard, which the paren scanner reintroduced and the
    #     old regex had bought with `_ARM_ATOM`: a match arm whose PATTERN is a
    #     string containing the macro's own text.
    expect_fix(
        "format-after-an-arm-pattern-holding-the-macro-text",
        "        \"a format!(\" => Err(format!(\"tool: got '{w}'\")),",
        '        "a format!(" => Err(format!("tool: got {}", quoteaf_os(&w))),',
    )

    # 15. An INTEGRATION test is test code with no attribute on it.
    #
    #     Case 13 covers `#[cfg(test)]`, which is every test inside a `src/`
    #     file and none of these: a file under `tests/` is only ever built by
    #     `cargo test`, so it carries no marker for `live_code` to find. The
    #     gate read them as production, and `userspace/oils` -- a shell, whose
    #     integration tests build shell source -- was where it showed.
    #
    #     This replaced a one-file IGNORE entry for the detector's own
    #     fixtures. Exempting test files one at a time is the shape that misses
    #     the next instance by construction, and the next instance was already
    #     in the tree when the entry was written.
    def skipped(rel: str) -> bool:
        return any(part in TEST_DIRS for part in rel.split("/"))

    for rel, want in (
        ("userspace/oils/tests/redirect_dup.rs", True),
        ("userspace/coreutils/tests/diagnostics_quote_names.rs", True),
        ("userspace/foo/benches/throughput.rs", True),
        ("userspace/foo/src/main.rs", False),
        # Not a test directory: the word has to be a whole path component, or
        # a crate called `attestation` would exempt itself.
        ("userspace/attestation/src/main.rs", False),
        ("userspace/foo/src/testsuite.rs", False),
    ):
        checked += 1
        if skipped(rel) != want:
            failures.append(f"test-dir rule: {rel} skipped={not want}, wanted {want}")

    for f in failures:
        print(f"selftest FAIL {f}")
    print(f"selftest: {checked - len(failures)}/{checked} cases pass")
    return 1 if failures else 0


def report(
    found: dict[str, list[tuple[int, str, str]]],
    show_lines: bool,
    scanned: int | None = None,
) -> int:
    """Print the findings, and -- separately -- how much was looked at.

    `scanned` is not decoration. Without it every number on the summary line
    counts something that is wrong, so a clean tree and a scan that lost its
    subject are spelled identically, and the reassuring reading is the one a
    reader reaches for. `_no_corpus` makes the same argument above and already
    holds the number; it was simply never shown on the path a human reads.
    """
    per_crate: dict[str, int] = {}
    for path, hits in found.items():
        parts = path.split("/")
        crate = "/".join(parts[:2]) if len(parts) > 1 else path
        per_crate[crate] = per_crate.get(crate, 0) + len(hits)
    total = sum(len(v) for v in found.values())
    if scanned is not None:
        print(f"inspected {scanned} .rs file(s) under {'/, '.join(ROOTS)}/")
    print(f"{total} violations in {len(found)} files, {len(per_crate)} crates\n")
    for crate, n in sorted(per_crate.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"{n:5}  {crate}")
    if show_lines:
        print()
        for path, hits in sorted(found.items()):
            for line_no, what, src in hits:
                print(f"  {path}:{line_no}: {what}\n      {src}")
    return 0


def check(
    found: dict[str, list[tuple[int, str, str]]],
    baseline: dict[str, int],
) -> int:
    """The ratchet. `baseline` is required, and that is deliberate.

    It used to default to `None` meaning "go and read it yourself", which was
    fine while a missing file read back as `{}`. Now that absence is `None`, the
    same sentinel would mean two opposite things at one call site -- "I did not
    look" and "I looked and there is nothing there". Making the caller pass it
    is what stops that ambiguity existing to be resolved wrongly later; the
    guard belongs in `main`, once, beside `_no_corpus`.
    """
    now = {path: len(hits) for path, hits in found.items()}

    grew = sorted(p for p, n in now.items() if n > baseline.get(p, 0))
    shrank = sorted(p for p, n in baseline.items() if now.get(p, 0) < n)

    for path in shrank:
        was, is_now = baseline[path], now.get(path, 0)
        print(f"fixed: {path} {was} -> {is_now} -- run --write-baseline to record it")

    if not grew:
        total = sum(now.values())
        print(f"ok -- {total} known sites in {len(now)} files ({len(shrank)} improved)")
        return 0

    new_sites = sum(now[p] - baseline.get(p, 0) for p in grew)
    print(
        f"\n{new_sites} NEW diagnostic(s) put a file name into a message without\n"
        "routing it through `quote`. A name containing a newline can then forge a\n"
        "line of this program's stderr:\n\n"
        "    $ touch $'x\\ncut: /etc/shadow: Permission denied'\n\n"
        "and the second line is indistinguishable from one `cut` really wrote.\n",
        file=sys.stderr,
    )
    for path in grew:
        was, is_now = baseline.get(path, 0), now[path]
        where = f"{path}  ({was} known -> {is_now} now)" if was else f"{path}  (new file)"
        print(f"\n  {where}", file=sys.stderr)
        for line_no, what, src in found[path]:
            print(f"    :{line_no}: {what}", file=sys.stderr)
            print(f"        {src}", file=sys.stderr)
    print(
        "\nThe fix is one call, not a baseline entry:\n"
        '    eprintln!("cut: {}: {e}", quotef_os(path));\n'
        "`quotef_os` quotes only when the name needs it; `quoteaf_os` always does.\n"
        "Raising a number in scripts/quote-names-baseline.txt records the defect\n"
        "instead of fixing it, which is the one thing that file must never hold.",
        file=sys.stderr,
    )
    return 1


def fix(targets: list[str]) -> int:
    """`--fix`: rewrite the mechanical sites under each of `targets`."""
    if not targets:
        print("--fix needs at least one path", file=sys.stderr)
        return 2
    files: list[Path] = []
    for t in targets:
        p = (ROOT / t) if not Path(t).is_absolute() else Path(t)
        if p.is_dir():
            files += [f for f in sorted(p.rglob("*.rs")) if "target" not in f.parts]
        elif p.is_file():
            files.append(p)
        else:
            print(f"no such path: {t}", file=sys.stderr)
            return 2
    total_fixed = 0
    all_skipped: list[str] = []
    for f in files:
        # A path outside the repo is legitimate -- it is how this rewriter is
        # checked, by pointing it at a `git show` of a file already converted
        # by hand -- so `relative_to` must not be allowed to raise on one.
        try:
            rel = f.resolve().relative_to(ROOT).as_posix()
        except ValueError:
            rel = f.as_posix()
        if rel in IGNORE:
            continue
        n, skipped = fix_file(f)
        total_fixed += n
        all_skipped += skipped
        if n:
            print(f"{rel}: {n} site(s) rewritten")
    for s in all_skipped:
        print(f"  LEFT {s}")
    print(f"\n{total_fixed} rewritten, {len(all_skipped)} left for a human")
    if total_fixed:
        print(
            "Wire the crate up with `scripts/quote-names-wire.py <crate> --why ...`,\n"
            "then `cargo clippy --fix` to drop the borrows that turn out unnecessary.\n"
            "\n"
            "Then BUILD it. This tool cannot see types, so it will happily wrap a\n"
            "value that is not a name -- a `char` holding an unknown option letter\n"
            "is the shape that occurs, and `quoteaf_os` does not take one. That is a\n"
            "compile error rather than a wrong message, which is the right way round,\n"
            "but it is yours to resolve: `quoteaf(&[byte])` for a single byte, or\n"
            "revert that one site if the value really is not a name."
        )
    return 0


def main() -> int:
    # The thing being reported is a *diagnostic*, and diagnostics in this tree
    # are full of the characters a Windows console's cp1252 cannot encode --
    # an em dash, an arrow, a non-ASCII file name. Printing one raised
    # UnicodeEncodeError from inside `--fix`'s report loop, *after* the files
    # had been written: the edits landed and the list of what was skipped did
    # not. A tool that reports less than it did is much worse than a tool with
    # a mangled character in its output.
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    args = sys.argv[1:]
    if selftestflag.wants_selftest(args):
        return selftest()
    if "--fix" in args:
        return fix([a for a in args if not a.startswith("--")])

    head: str | None = None
    if "--head" in args:
        i = args.index("--head")
        if i + 1 >= len(args) or args[i + 1].startswith("--"):
            print("--head needs a commit-ish argument", file=sys.stderr)
            return 2
        head = args[i + 1]

    if head is not None and ("--write-baseline" in args or "--update-baseline" in args):
        # Refused rather than ignored. Recording a past commit's counts as the
        # current allowance would silently un-fix everything repaired since.
        print("--head cannot be combined with --write-baseline", file=sys.stderr)
        return 2

    if head is not None:
        try:
            # One `RevTree`, not two: each builds its own `ls-tree` index of
            # the whole tree (~4 s) and its own `cat-file` process, and the
            # baseline lookup is a single blob read that has no business
            # paying for a second index.
            with gittree.RevTree(head, str(ROOT)) as tree:
                seen = survey_tree(tree)
                baseline = read_baseline_from(tree)
        except (gittree.GitTreeError, OSError) as e:
            # Loud, and non-zero. A checker that cannot read the tree it was
            # asked about must not report the clean answer -- "no violations
            # found" is byte-identical to a healthy repository, so a silent
            # degradation here would look exactly like success.
            print(f"quote-names: cannot read the tree at {head}: {e}", file=sys.stderr)
            return 2
        if _no_corpus(seen, head):
            return 2
        if "--check" not in args:
            # `report` does not consult the ratchet, so a tree without one is
            # still perfectly reportable. Guarding it here anyway would make
            # the plain listing -- the one mode whose job is to survey a tree
            # nobody has ratcheted yet -- refuse the trees it exists for.
            return report(seen.found, "--list" in args, seen.scanned)
        if _no_baseline(baseline, head):
            return 2
        return check(seen.found, baseline)

    seen = survey()
    if _no_corpus(seen, None):
        return 2
    if "--write-baseline" in args or "--update-baseline" in args:
        # Before `_no_baseline`, and it has to be: this is the mode that
        # *creates* the file. A bootstrap that refused to bootstrap would be
        # found only by whoever next moved the baseline, who is precisely the
        # person the guard is protecting.
        write_baseline(seen.found)
        return 0
    if "--check" in args:
        disk_baseline = read_baseline()
        if _no_baseline(disk_baseline, None):
            return 2
        return check(seen.found, disk_baseline)
    return report(seen.found, "--list" in args, seen.scanned)


if __name__ == "__main__":
    sys.exit(main())
