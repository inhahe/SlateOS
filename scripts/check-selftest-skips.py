#!/usr/bin/env python3
"""Guard self-tests against skipping themselves and reporting success anyway.

The rule
--------
**A self-test may skip, but it must skip for a reason it looked up, and it
must say so in the line a reader believes.**

Two failures, and the second is the dangerous one
-------------------------------------------------
1. *The skip does not reach the summary.*  `kernel/src/fs/index.rs` printed
   ``(skipped VFS tests: /tmp not mounted)`` in the middle of a run and
   ``[index] Self-test passed`` at the end.  The last line is the one that
   gets believed, so a run that tested half of what it claimed was
   byte-indistinguishable from one that tested all of it.

2. *The decision to skip is a swallowed error from the code under test.*
   This is worse, because it is self-concealing.  ``if mkdir(d).is_ok() {
   ..test.. } else { print SKIPPED }`` reads as "skip when the filesystem has
   no directories", but it actually means "skip on **any** failure" --
   including a permission gate wrongly denying the `mkdir`, a full disk, or
   the very bug the section exists to catch.  The worse the code under test
   gets, the more tests switch themselves off, and the suite goes green.

   `kernel/src/fs/handle.rs` had six of these, and their setup steps included
   `mkdir`, `symlink`, `set_owner` and `set_permissions` -- every one a VFS
   entry point that `fs::vfs::check_path_access` now gates.  A gate that
   started refusing them would have disabled the six tests that would have
   noticed, and printed ``Self-test PASSED``.

Both are invisible at runtime by construction: the whole point of the defect
is that the log looks like a pass.  Only a source-level invariant catches it.

What is checked
---------------
For every function whose name marks it a self-test (``self_test``,
``self_test_inner``, ``*_self_test``, ``self_test_*``):

1. **A skip must not be decided by a discarded `Result`.**  If a branch whose
   body announces a skip is selected by the *outcome* of a call, that is a
   finding.  Three spellings of the same decision are recognised, because a
   rule enforced on only one of them just moves the defect to the other two:

   ===================================  =====================================
   Spelling                             Example
   ===================================  =====================================
   ``.is_ok()`` / ``.is_err()``         ``if mkdir(d).is_ok() { .. } else { skip }``
   (directly, or via a `let` binding)   ``let w = mkdir(d).is_ok(); if w { .. }``
   ``if let Ok(..)`` / ``if let Err``   ``if let Err(e) = mkdir(d) { skip }``
   ``match`` with a catch-all `Err`     ``match mkdir(d) { Ok(_) => .., Err(_) => skip }``
   ===================================  =====================================

   Ask the environment instead (the mount table, a feature query), or
   classify the error and treat only "unimplemented" as a reason to skip.

   An arm that names *specific* errors -- ``Err(KernelError::NotSupported)``,
   ``Err(NoSuchDevice | ReadOnlyFilesystem)`` -- is exempt, because that is
   the approved form: it is a decision about what the error *means*, not a
   decision to ignore whatever came back.  Only a catch-all binding
   (``Err(_)``, ``Err(e)``, ``Err(..)``) is a finding.  `selftest::classify`
   exists so this judgement is made in one place; a match on its
   ``Setup::Unsupported`` never trips the rule, since the pattern is not
   ``Err(..)``.

2. **A section-level skip must not be followed by an unconditional success
   claim.**  If a skip is announced inside a branch *and that branch does not
   return*, then the function may not also print an unqualified "passed" at
   its top level.  Put the success line behind a branch that accounts for the
   skips, or name them in the message (``passed with 2 section(s) SKIPPED``).

   A skip branch that `return`s is exempt: the success line is unreachable in
   that case, so the log cannot claim both.

Scope and honesty about it
--------------------------
A textual, single-file heuristic in the style of its siblings
`check-vfs-under-lock.py`, `check-recursive-locks.py` and
`check-vfs-permission-gate.py`, sharing their parser.  It keys off function
*names* to find self-tests, so a helper called from one -- but named
something else -- is not examined.  It keys off the word "skip" in a printed
literal to find a skip, so a section that quietly does nothing and prints
nothing at all is invisible to it; that failure has no textual signature and
is why ALLOW below demands a reason rather than just a name.

Exit codes: 0 clean, 1 findings, 2 could not run.
"""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

_SIBLING = Path(__file__).resolve().parent / "check-recursive-locks.py"
_spec = importlib.util.spec_from_file_location("check_recursive_locks", _SIBLING)
if _spec is None or _spec.loader is None:  # pragma: no cover - packaging error
    print(f"error: cannot load {_SIBLING}", file=sys.stderr)
    raise SystemExit(2)
_rl = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_rl)

# A function is a self-test if its name says so.  Being name-based is a real
# limitation (see the docstring), but the alternative -- following calls --
# needs a resolver this family of checkers deliberately does not have.
SELFTEST_NAME = re.compile(r"\A(?:self_test(?:_inner)?|self_test_.*|.*_self_test)\Z")
# Any `name(` -- the callee half of a same-file call edge.  Deliberately
# crude: over-approximating the call graph only widens what gets checked.
CALL_IDENT = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(")

IS_RESULT = re.compile(r"\.is_(?:ok|err)\s*\(\s*\)")
IF_LET_RESULT = re.compile(r"\bif\s+let\s+(?:Ok|Err)\s*\(")
MATCH_KW = re.compile(r"\bmatch\b")
ERR_PATTERN = re.compile(r"\bErr\s*\(")
# `Err(_)`, `Err(e)`, `Err(..)` -- accepts anything that came back, so the
# branch is a decision to ignore the error rather than a decision about what
# it means.  `Err(KernelError::NotSupported)` is not this, and is exempt.
CATCHALL_BINDING = re.compile(r"\A\s*(?:_|\.\.|(?:mut\s+)?[a-z_][a-z0-9_]*)\s*\Z")
PRINTLN = re.compile(r"\b(?:serial_println|shell_println|println)\s*!\s*\(")
SKIP_WORD = re.compile(r"skip", re.IGNORECASE)
# "passed", "PASSED", "PASS" -- a claim that the whole test succeeded.  Not
# "OK", which self-tests print per-section and which claims nothing global.
PASS_WORD = re.compile(r"\bpass(?:ed)?\b", re.IGNORECASE)
RETURN_KW = re.compile(r"\breturn\b")
LET_BINDING = re.compile(r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::[^=]*)?=\s*\Z")

# Sites that look like the pattern but are not, each with the reason it is
# exempt.  The reason is the point: this list is where a reader checks whether
# an exemption is still true.
ALLOW: dict[str, str] = {}


def _macro_span(src: str, start: int) -> tuple[int, int] | None:
    """Byte span of a `foo!( ... )` invocation whose `(` is at `start`."""
    depth = 0
    i = start
    while i < len(src):
        if src[i] == "(":
            depth += 1
        elif src[i] == ")":
            depth -= 1
            if depth == 0:
                return (start, i + 1)
        i += 1
    return None


def _block_span(src: str, brace: int) -> int | None:
    """Offset just past the `}` matching the `{` at `brace`."""
    depth = 0
    i = brace
    while i < len(src):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return None


def _prints(src: str, raw: str, lo: int, hi: int) -> list[tuple[int, int, str]]:
    """Every print macro in [lo, hi), as (start, end, raw text)."""
    out: list[tuple[int, int, str]] = []
    for m in PRINTLN.finditer(src, lo, hi):
        span = _macro_span(src, m.end() - 1)
        if span is None:
            continue
        out.append((m.start(), span[1], raw[m.start() : span[1]]))
    return out


def _depth_at(src: str, lo: int, pos: int) -> int:
    """Brace depth of `pos` relative to `lo`."""
    return src.count("{", lo, pos) - src.count("}", lo, pos)


def _selftest_bodies(src: str) -> list[tuple[str, int, int]]:
    """Self-tests in this file, by name *and* by being called from one.

    Name alone was not enough.  `ipc/io_ring.rs` splits its suite across
    `test_fh_read_write`, `test_positioned_io` and friends, each called from
    `self_test`; both of the first two skipped themselves on a failed
    `/tmp` write, and neither was examined because neither is *called*
    `self_test`.  A rule that stops at the entry point misses everything the
    entry point delegates to, which is most of a large suite.

    So: seed with the name match, then take the transitive closure over
    same-file calls.  A `#[cfg(test)]` unit test named `test_foo` is not
    reached, because nothing in a self-test calls it -- which is the
    distinction that matters and the reason this is a call check rather than
    a second name pattern.
    """
    bodies = _rl.find_bodies(src)
    # One scan per body for *every* callee name, rather than one search per
    # (body, candidate) pair.  The pairwise form was quadratic in the number
    # of functions and pushed this gate past three minutes on the kernel --
    # long enough that a boot test looked hung.  Extracting the call names
    # once makes the closure a set lookup.
    calls = {
        name: {m.group(1) for m in CALL_IDENT.finditer(src, lo, hi)}
        for name, (lo, hi) in bodies.items()
    }
    # Callers, keyed by callee: a function is a suite *helper* only if the suite
    # is the only thing in the file that calls it.
    callers: dict[str, set[str]] = {n: set() for n in bodies}
    for name, (lo, hi) in bodies.items():
        for cand in calls[name]:
            if cand not in bodies or cand == name:
                continue
            # A recursive or self-overlapping span is not a call edge.
            clo, chi = bodies[cand]
            if clo >= lo and chi <= hi:
                continue
            callers[cand].add(name)

    selected = {n for n in bodies if SELFTEST_NAME.match(n)}
    # Reachability alone is the wrong closure, and `kshell.rs` is the proof: its
    # `self_test` drives commands through the real `dispatch_with_input`, which
    # is what an integration test *should* do -- and that one edge pulled 1050
    # of the file's 1052 functions into "the suite", after which the gate was
    # really just grepping the whole shell for the word "skip".
    #
    # The distinction that matters is not "can the suite reach it" but "does it
    # exist for the suite".  A helper's only callers are the suite; production
    # code reached through a dispatcher has other callers, and its own error
    # handling is not a test disabling itself.  So a callee joins only when
    # *every* in-file caller is already in.  `io_ring.rs`'s `test_fh_read_write`
    # -- called from `self_test` and nothing else, which is the case this
    # closure was added for -- still joins.
    changed = True
    while changed:
        changed = False
        for name in bodies:
            if name in selected:
                continue
            who = callers[name]
            if who and who <= selected:
                selected.add(name)
                changed = True
    return [(n, bodies[n][0], bodies[n][1]) for n in sorted(selected)]


def _branch_blocks(src: str, cond_end: int, body_end: int) -> list[tuple[int, int]]:
    """The `{..}` blocks of the `if` whose condition ends at `cond_end`.

    Returns the then-block and, when present, the else-block.  An `else if`
    chain contributes its own then-block and continues.
    """
    blocks: list[tuple[int, int]] = []
    i = cond_end
    while i < body_end and src[i] != "{":
        # A `;` before any `{` means this was a statement, not a condition.
        if src[i] == ";":
            return blocks
        i += 1
    while i < body_end and src[i] == "{":
        end = _block_span(src, i)
        if end is None:
            break
        blocks.append((i, end))
        # Look for a following `else`.
        j = end
        while j < body_end and src[j].isspace():
            j += 1
        if src[j : j + 4] != "else":
            break
        j += 4
        while j < body_end and src[j].isspace():
            j += 1
        if src[j : j + 2] == "if":
            # `else if <cond> {` -- walk to that block's `{`.
            j += 2
            while j < body_end and src[j] != "{":
                j += 1
        i = j
    return blocks


def _paren_span(src: str, lp: int) -> int | None:
    """Offset just past the `)` matching the `(` at `lp`."""
    depth = 0
    i = lp
    while i < len(src):
        if src[i] == "(":
            depth += 1
        elif src[i] == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return None


def _scan_to_brace(src: str, i: int, limit: int) -> int | None:
    """First `{` at or after `i`, or None if a `;` comes first."""
    while i < limit:
        if src[i] == ";":
            return None
        if src[i] == "{":
            return i
        i += 1
    return None


def _arm_body(src: str, i: int, limit: int) -> tuple[int, int]:
    """Span of a match arm's body, starting just past its `=>`."""
    while i < limit and src[i].isspace():
        i += 1
    if i < limit and src[i] == "{":
        end = _block_span(src, i)
        if end is not None:
            return (i, end)
    # A braceless arm runs to the `,` that ends it.
    depth = 0
    j = i
    while j < limit:
        c = src[j]
        if c in "({[":
            depth += 1
        elif c in ")}]":
            if depth == 0:
                break
            depth -= 1
        elif c == "," and depth == 0:
            break
        j += 1
    return (i, j)


def _result_branch_sites(
    src: str, blo: int, bhi: int
) -> list[tuple[int, list[tuple[int, int]]]]:
    """Branches selected by a `Result`'s outcome, as (report_pos, blocks).

    Covers the `if let Ok(..)/Err(..)` and catch-all-`Err` `match` spellings.
    The `.is_ok()`/`.is_err()` spelling is handled separately because it also
    has to chase a `let` binding to the `if` that consumes it.
    """
    out: list[tuple[int, list[tuple[int, int]]]] = []

    for m in IF_LET_RESULT.finditer(src, blo, bhi):
        close = _paren_span(src, m.end() - 1)
        if close is None:
            continue
        if not CATCHALL_BINDING.match(src[m.end() : close - 1]):
            continue  # names specific errors: a judgement, not a swallow
        blocks = _branch_blocks(src, close, bhi)
        if blocks:
            out.append((m.start(), blocks))

    for m in MATCH_KW.finditer(src, blo, bhi):
        brace = _scan_to_brace(src, m.end(), bhi)
        if brace is None:
            continue
        end = _block_span(src, brace)
        if end is None:
            continue
        for a in ERR_PATTERN.finditer(src, brace + 1, end - 1):
            # Only arm patterns of *this* match, not of a nested one.
            if _depth_at(src, brace, a.start()) != 1:
                continue
            close = _paren_span(src, a.end() - 1)
            if close is None:
                continue
            if not CATCHALL_BINDING.match(src[a.end() : close - 1]):
                continue
            arrow = src.find("=>", close, end)
            if arrow < 0:
                continue
            out.append((a.start(), [_arm_body(src, arrow + 2, end)]))

    return out


def check_file(path: Path, rel: str) -> list[str]:
    raw = path.read_text(encoding="utf-8", errors="replace")
    if "self_test" not in raw:
        return []
    src = _rl.strip_noise(raw)
    findings: list[str] = []

    for name, blo, bhi in _selftest_bodies(src):
        skip_prints = [
            p for p in _prints(src, raw, blo, bhi) if SKIP_WORD.search(p[2])
        ]
        if not skip_prints:
            continue

        # --- Rule 1: the skip must not be decided by a discarded Result. ---
        candidates: list[tuple[int, list[tuple[int, int]]]] = []
        for m in IS_RESULT.finditer(src, blo, bhi):
            blocks = _branch_blocks(src, m.end(), bhi)
            if not blocks:
                # No block follows, so this is a binding: `let x = a.is_ok()
                # && b.is_ok();`.  Find the name and the `if x` that uses it.
                stmt_end = src.find(";", m.end(), bhi)
                if stmt_end < 0:
                    continue
                line_start = src.rfind(";", blo, m.start()) + 1
                lb = LET_BINDING.search(src[line_start : m.start()] + "")
                # The `let` may be several sub-expressions back; search the
                # whole statement for its opening.
                lb = re.search(
                    r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::[^=]*?)?=",
                    src[line_start : stmt_end],
                )
                if lb is None:
                    continue
                var = lb.group(1)
                use = re.search(
                    r"\bif\s+!?\s*" + re.escape(var) + r"\s*\{", src[stmt_end:bhi]
                )
                if use is None:
                    continue
                blocks = _branch_blocks(src, stmt_end + use.end() - 1, bhi)
            candidates.append((m.start(), blocks))

        candidates.extend(_result_branch_sites(src, blo, bhi))

        for pos, blocks in sorted(candidates):
            for lo, hi in blocks:
                if any(SKIP_WORD.search(p[2]) for p in _prints(src, raw, lo, hi)):
                    key = f"{rel}::{name}"
                    if key in ALLOW:
                        break
                    line = raw.count("\n", 0, pos) + 1
                    findings.append(
                        f"{rel}:{line}: `{name}` decides to skip a section from "
                        f"the outcome of a call into the code under test -- any "
                        f"failure, including the bug the section exists to "
                        f"catch, silently disables it. Ask the environment "
                        f"(mount table, feature query), or match the error and "
                        f"skip only on `NotSupported`."
                    )
                    break

        # --- Rule 2: a non-returning section skip forbids an unconditional
        # top-level success claim. ---
        section_skip = False
        for pstart, pend, _text in skip_prints:
            if _depth_at(src, blo, pstart) < 1:
                continue  # a skip printed at function top level, not a section
            # Find the innermost enclosing block and ask whether it returns.
            open_pos = None
            depth = 0
            i = pstart
            while i > blo:
                i -= 1
                if src[i] == "}":
                    depth += 1
                elif src[i] == "{":
                    if depth == 0:
                        open_pos = i
                        break
                    depth -= 1
            if open_pos is None:
                continue
            close_pos = _block_span(src, open_pos)
            if close_pos is None:
                continue
            if RETURN_KW.search(src[open_pos:close_pos]):
                continue  # early-exit skip: the success line is unreachable
            section_skip = True

        if not section_skip:
            continue
        for pstart, _pend, text in _prints(src, raw, blo, bhi):
            if _depth_at(src, blo, pstart) != 0:
                continue  # conditional: it is not an unqualified claim
            if not PASS_WORD.search(text):
                continue
            if SKIP_WORD.search(text):
                continue  # the message names the skips
            key = f"{rel}::{name}"
            if key in ALLOW:
                continue
            line = raw.count("\n", 0, pstart) + 1
            findings.append(
                f"{rel}:{line}: `{name}` prints an unconditional success after "
                f"a section skipped -- the last line is the one a reader "
                f"believes, so a partial run reads as a full one. Guard it, or "
                f"name the skipped sections in the message."
            )
    return findings


# A suite that skips itself, a helper that skips itself, an unconditional
# PASSED after a skip -- and, as the control, a production command reached only
# through a dispatcher, whose own "Warning: skipping <file>" must NOT be read as
# a test disabling itself.  The control is the point: the closure that decides
# what counts as "the suite" is the part of this gate most likely to go wrong,
# and it goes wrong silently in both directions (too wide and it grades the
# whole file, too narrow and it grades nothing).
_SELFTEST_FIXTURE = '''pub fn self_test() -> Result<(), E> {
    helper()?;
    dispatch("archive create out.zip a b");
    if Vfs::write_file("/tmp/p", b"").is_ok() {
        do_the_real_check();
    } else {
        serial_println!("skipping the write section");
    }
    serial_println!("Self-test PASSED");
    Ok(())
}

fn helper() -> Result<(), E> {
    if Vfs::write_file("/tmp/q", b"").is_err() {
        serial_println!("  skipped: no /tmp");
        return Ok(());
    }
    Ok(())
}

fn production_command(a: &str) {
    match Vfs::read_file(a) {
        Ok(d) => use_it(d),
        Err(e) => shell_println!("Warning: skipping {}: {:?}", a, e),
    }
}

fn dispatch(c: &str) { production_command(c); }

fn real_shell_entry(line: &str) { dispatch(line); }
'''


def self_test() -> int:
    """Check the gate against a fixture with known answers.

    Run with `--self-test`.  A gate whose scope quietly collapses reports zero
    findings, which reads exactly like a clean tree -- so the scope itself needs
    a test, not just the rules built on top of it.
    """
    import tempfile

    failures = 0
    src = _rl.strip_noise(_SELFTEST_FIXTURE)
    suite = sorted(n for n, _, _ in _selftest_bodies(src))
    if suite != ["helper", "self_test"]:
        failures += 1
        print(f"FAIL closure: expected ['helper', 'self_test'], got {suite}")

    tmp = Path(tempfile.gettempdir()) / "_selftest_skips_fixture.rs"
    tmp.write_text(_SELFTEST_FIXTURE, encoding="utf-8")
    try:
        found = check_file(tmp, "fixture.rs")
    finally:
        tmp.unlink(missing_ok=True)

    def one(substr: str, why: str) -> None:
        nonlocal failures
        if not any(substr in f for f in found):
            failures += 1
            print(f"FAIL: no finding for {why}")

    one("`self_test` decides to skip", "a suite skipping on a failed call")
    one("`helper` decides to skip", "a suite helper skipping on a failed call")
    one("unconditional success", "PASSED printed after a skip")
    if any("production_command" in f for f in found):
        failures += 1
        print("FAIL: graded `production_command`, which is reached only via a dispatcher")

    if failures:
        print(f"\n[selftest-skips self-test] {failures} failure(s)", file=sys.stderr)
        return 1
    print("[selftest-skips self-test] OK", file=sys.stderr)
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
        findings.extend(check_file(path, path.relative_to(root).as_posix()))
    for f in findings:
        print(f)
    print(
        f"\n[selftest-skips] {files} file(s): {len(findings)} finding(s)",
        file=sys.stderr,
    )
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
