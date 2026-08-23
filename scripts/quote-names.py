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

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "quote-names-baseline.txt"

# The trees lane B owns. `apps/` and `gui/` are lane C's and are deliberately
# outside: a gate that fails another lane's commit for another lane's code is
# a gate that lane turns off.
ROOTS = ("userspace", "services", "init", "posix")

# Kept byte-for-byte in step with NOT_A_NAME in diagnostics_quote_names.rs.
NOT_A_NAME = {"msg", "e", "err", "error", "message", "reason"}

# Files whose hits are not defects, with the reason. This table records *why*
# a file is exempt; the baseline records only *that* a site exists, which is
# the wrong place for a judgement. Keep it short -- every entry is a hole.
IGNORE = {
    # The bad form appears here as the detector's own fixtures: the test feeds
    # `eprintln!("cp: {path}: {e}")` to `bare_interpolated_name` and asserts it
    # is caught. Baselining them would be worse than a false positive -- they
    # can never be "fixed", so the count could never reach zero, and a ratchet
    # with an unreachable floor stops being read.
    "userspace/coreutils/tests/diagnostics_quote_names.rs": "the detector's own fixtures",
}


def bare_interpolated_name(line: str) -> str | None:
    """Port of `bare_interpolated_name` in diagnostics_quote_names.rs.

    Matches `eprintln!("prog: {ident}: ...` — the shape where `ident` reaches
    the message as a bare name.

    `line` is a *logical* line: `join_wrapped_calls` has already pulled a call
    that rustfmt split back onto one. What arrives here can therefore be
    `eprintln!( "cut: {path}: {e}" );`, with the spaces the join left behind,
    so the macro and its opening quote are matched with whitespace between
    them allowed rather than by a bare `partition`.
    """
    m = re.search(r'eprintln!\(\s*"', line)
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


def hand_written_quotes(line: str) -> bool:
    """Port of the `no_diagnostic_hand_writes_quotes_around_a_name` detector.

    `'{path}'` is worse than `{path}`, not better: it *looks* quoted, so it
    survives review, while a name containing a `'` still breaks out.
    """
    if "eprintln!(" not in line and "println!(" not in line:
        return False
    return "'{" in line and "}'" in line


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
_ARM_ATOM = r"""(?:[^"'{};]|"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')"""
_CALL = re.compile(
    rf'^(?P<lead>\s*(?:{_ARM_ATOM}*=>\s*)?)(?P<mac>e?println!)'
    r'\(\s*"(?P<fmt>(?:[^"\\]|\\.)*)"(?P<args>.*?)\s*\)(?P<end>[;,])$'
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
    m = _CALL.match(line)
    if m is None:
        return None, "not a single-line println!/eprintln! call"
    fmt, rest = m.group("fmt"), m.group("args")
    args = _split_args(rest)
    if args is None:
        return None, "argument list not safely splittable"

    def rebuilt(new_fmt: str, new_args: list[str]) -> str:
        tail = "".join(f", {a}" for a in new_args)
        return (
            f'{m.group("lead")}{m.group("mac")}("{new_fmt}"{tail})'
            f'{m.group("end")}'
        )

    ident = bare_interpolated_name(line)
    if ident is not None:
        # The rewrite turns `{ident}` into `{}`, which consumes the *first*
        # unused argument. That is only the one being added if no earlier
        # placeholder is already positional.
        head = fmt.split("{" + ident + "}", 1)[0]
        if "{}" in head or args:
            return None, "would renumber an existing positional argument"
        return rebuilt(fmt.replace("{" + ident + "}", "{}", 1), [f"quotef_os(&{ident})"]), ""

    inline = _INLINE_QUOTED.findall(fmt)
    positional = len(_POSITIONAL_QUOTED.findall(fmt))

    if inline and not positional:
        if any(p == "{}" for p in _ANY_PLACEHOLDER.findall(fmt)) or args:
            return None, "would renumber an existing positional argument"
        return (
            rebuilt(_INLINE_QUOTED.sub("{}", fmt), [f"quoteaf_os(&{n})" for n in inline]),
            "",
        )

    if positional == 1 and not inline and len(args) == 1:
        # The single `'{}'` must be the only positional placeholder, or the
        # lone argument is not the one it names.
        if sum(1 for p in _ANY_PLACEHOLDER.findall(fmt) if p == "{}") != 1:
            return None, "more than one positional placeholder"
        return rebuilt(_POSITIONAL_QUOTED.sub("{}", fmt, count=1), [f"quoteaf_os(&{args[0]})"]), ""

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


def survey(root: Path = ROOT) -> dict[str, list[tuple[int, str, str]]]:
    """Every flagged line in lane B's tree, keyed by repo-relative path."""
    found: dict[str, list[tuple[int, str, str]]] = {}
    for top in ROOTS:
        base = root / top
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            if "target" in path.parts:
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError):
                continue
            rel = path.relative_to(root).as_posix()
            if rel in IGNORE:
                continue
            hits = violations(text)
            if hits:
                found[rel] = hits
    return found


def read_baseline() -> dict[str, int]:
    """`path -> count` from the baseline file, `#` comments stripped."""
    if not BASELINE.is_file():
        return {}
    out: dict[str, int] = {}
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
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
        "# scanned roots, and it has happened once -- 2026-08-23, when calls that",
        "# rustfmt had wrapped onto two lines turned out to be invisible, hiding",
        "# 71 real sites. If a number rises in a commit that also edits code, the",
        "# code is what raised it.",
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
    expect("stdout-not-stderr", 'println!("cut: {path}: {e}");', 0)

    # 5. Hand-written quotes: the shape that looks fixed and is not.
    expect("hand-quotes", "eprintln!(\"cut: '{path}': {e}\");", 1)
    expect("hand-quotes-stdout", "println!(\"cut: '{path}'\");", 1)
    expect("hand-quotes-no-macro", "let s = format!(\"'{path}'\");", 0)

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
    # Declines. Each is a real shape in the tree, and each would be corrupted
    # by a rewrite that went ahead anyway.
    expect_fix("declines-multiline", "eprintln!(\"lp: printer '{p}' not\"", None)
    expect_fix(
        "declines-existing-positional",
        "eprintln!(\"lp: {} wants '{p}'\", n);",
        None,
    )
    expect_fix(
        "declines-two-positional-quotes",
        "eprintln!(\"lp: '{}' and '{}'\", a, b);",
        None,
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

    for f in failures:
        print(f"selftest FAIL {f}")
    print(f"selftest: {checked - len(failures)}/{checked} cases pass")
    return 1 if failures else 0


def report(found: dict[str, list[tuple[int, str, str]]], show_lines: bool) -> int:
    per_crate: dict[str, int] = {}
    for path, hits in found.items():
        parts = path.split("/")
        crate = "/".join(parts[:2]) if len(parts) > 1 else path
        per_crate[crate] = per_crate.get(crate, 0) + len(hits)
    total = sum(len(v) for v in found.values())
    print(f"{total} violations in {len(found)} files, {len(per_crate)} crates\n")
    for crate, n in sorted(per_crate.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"{n:5}  {crate}")
    if show_lines:
        print()
        for path, hits in sorted(found.items()):
            for line_no, what, src in hits:
                print(f"  {path}:{line_no}: {what}\n      {src}")
    return 0


def check(found: dict[str, list[tuple[int, str, str]]]) -> int:
    baseline = read_baseline()
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
    if "--selftest" in args:
        return selftest()
    if "--fix" in args:
        return fix([a for a in args if not a.startswith("--")])
    found = survey()
    if "--write-baseline" in args or "--update-baseline" in args:
        write_baseline(found)
        return 0
    if "--check" in args:
        return check(found)
    return report(found, "--list" in args)


if __name__ == "__main__":
    sys.exit(main())
