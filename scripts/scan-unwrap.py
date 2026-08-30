#!/usr/bin/env python3
"""Find `.unwrap()` / `.expect(...)` in *production* kernel code.

CLAUDE.md forbids these outside tests: every one is a potential DoS if an
attacker can shape the input, and the crate lints (`unwrap_used = "warn"`,
`expect_used = "warn"`) are set to say so. Tests are exempt -- panicking on bad
data is the point there -- so the only interesting number is the production one,
and a plain `grep -c` cannot produce it: at the time of writing about 4500 of
the ~4575 sites in `kernel/src` are in test bodies.

Two things make this harder than it sounds, and the first version of this script
got both wrong:

1.  **Not every `.expect(` is a panic.** `kernel/src/json.rs` has a parser whose
    own method is named `expect`: `self.expect(b'"')?` returns a `Result` and is
    exemplary error handling, not a panic. The discriminator used here is that
    `Result::expect` / `Option::expect` return `T`, so a `?` after the call
    means it cannot be either of them. Calls on `self` are likewise assumed to
    be inherent methods.

2.  **Attributing a site to its enclosing function -- all of them.** Three
    versions of this script have now existed, and the first two got this wrong
    in opposite directions. Falling back to the *nearest preceding* `fn` fails
    because these self-tests routinely define helpers inside themselves:

        pub fn self_test() {
            fn case(...) { ... }        // <- nearest preceding fn
            foo().expect("register");   // <- but this is still test code
        }

    Nearest-preceding blames `case`, which is not named `*test*`, so all 25
    sites across five files were reported as production. What matters is not
    the nearest enclosing `fn` but whether *any* enclosing `fn` is a test, so
    the second version kept a **stack** of open functions, using indentation as
    the nesting proxy.

    That worked, but only because the tree happens to be rustfmt-formatted --
    a precondition this script asserted and nothing checked. It is now shared
    with `scripts/rust_scopes.py`, which tracks brace depth through a lexer
    that understands strings, raw strings, char literals and comments, so it
    does not care how the file is laid out. Brace depth was rejected when the
    second version was written, on the grounds that "raw strings, char literals
    and macro bodies all perturb the count" -- true of a naive counter, which is
    why the shared one is not naive, and why it is checked: it closes every
    scope it opens across all 800 files of `kernel/src`, and a single
    desynchronisation anywhere would leave a dangling scope at some file's EOF.

    The two methods were run against each other over all 4407 panicking sites
    in the tree before the switch, and agreed on every one.

A site is TEST if **any** function on the enclosing stack is named `*test*`
(which covers the `self_test` / `*_self_test` convention this kernel uses for
its boot suites), or if it carries `#[test]` / sits under `#[cfg(test)]`.
Doc-comment examples are skipped -- they are prose, not code.

3.  **A name is a proxy for "is this a test", and a few production functions
    trip it.** `kshell::eval_test` is the shell's `test` / `[` builtin: it
    parses a user-supplied expression and is production code by any measure,
    but it ends in `_test` and so was silently exempt. Nothing hides behind it
    today -- it has no sites -- which is precisely why it is worth naming now
    rather than after one appears. `NOT_TESTS` below is the list of such
    functions, and adding to it is a deliberate, reviewable act.

    The name is only a proxy for the real question, which is whether the
    function is reachable from anything but the boot self-test suite. Answering
    *that* needs a call graph, and it is not obviously worth one: the convention
    is followed almost everywhere, and the exceptions fit on one screen. But the
    proxy cannot separate `eval_test` (a shell builtin) from `tx_datapath_test`
    (a driver self-test) on the strength of the name alone, so the list is not
    optional -- without it the gate has a hole shaped like anything an author
    happens to call `*_test`.

Usage:
    python scripts/scan-unwrap.py [path ...]        # default: kernel/src
    python scripts/scan-unwrap.py --summary         # per-file counts only
    python scripts/scan-unwrap.py --show-skipped    # explain what was filtered
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import rust_scopes

SITE_RE = re.compile(r"\.(unwrap|expect)\s*\(")
# `.expect(...)?` or `.unwrap()?` -- returns a Result, so not the panicking one.
FALLIBLE_RE = re.compile(r"\.(?:unwrap|expect)\s*\([^()]*\)\s*\?")
SELF_CALL_RE = re.compile(r"\bself\.(?:unwrap|expect)\s*\(")

# The "is this test code?" policy -- the name rule and the list of production
# functions that collide with it -- lives in `rust_scopes`, shared with
# `clippy-sites.py`. Two copies of it would disagree the first time one was
# edited, and this one gates the build.
is_test_name = rust_scopes.is_test_name


def classify_line(line: str) -> tuple[bool, str]:
    """(is_a_panicking_site, reason_if_not)."""
    stripped = line.strip()
    if stripped.startswith("//!") or stripped.startswith("///"):
        return False, "doc comment"
    if stripped.startswith("//"):
        return False, "comment"
    if not SITE_RE.search(line):
        return False, "no site"
    if SELF_CALL_RE.search(line):
        return False, "inherent method on self"
    if FALLIBLE_RE.search(line):
        return False, "returns Result (followed by `?`)"
    return True, ""


def _top(stack: list[rust_scopes.Scope]) -> tuple[str, bool]:
    """The name to report for a site, and whether *any* enclosing scope is a test.

    The name is the innermost function -- that is the useful one to print when
    going to fix a site -- but the test verdict is taken over the whole stack,
    because a helper defined inside `self_test` is test code however it is
    named.
    """
    if not stack:
        return "<top level>", False
    is_test = any(is_test_name(s.name) or s.cfg_test for s in stack)
    return stack[-1].name, is_test


def scan_file(path: Path, show_skipped: bool = False):
    """Return (production_findings, skipped) lists."""
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:  # pragma: no cover
        print(f"warning: cannot read {path}: {exc}", file=sys.stderr)
        return [], []

    lines = text.splitlines()

    # Pass 1: for every line, the stack of items enclosing it. `rust_scopes`
    # tracks `mod` as well as `fn`, so a `#[cfg(test)] mod tests` is a scope
    # that opens and *closes* rather than a line number past which everything
    # is assumed to be test code -- which is what this script used to do, and
    # which was wrong for any file with code after such a module.
    scopes = rust_scopes.scope_stack_per_line(lines)

    def enclosing(lineno: int) -> tuple[str, bool]:
        idx = lineno - 1
        return _top(scopes[idx] if 0 <= idx < len(scopes) else [])

    findings = []
    skipped = []
    for i, raw in enumerate(lines, start=1):
        if not SITE_RE.search(raw):
            continue
        is_site, reason = classify_line(raw)
        if not is_site:
            if show_skipped:
                skipped.append((i, reason, raw.strip()))
            continue
        name, is_test = enclosing(i)
        if is_test:
            if show_skipped:
                skipped.append((i, f"in test scope `{name}`", raw.strip()))
            continue
        findings.append((i, name, raw.strip()))

    return findings, skipped


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("paths", nargs="*", default=None)
    ap.add_argument("--summary", action="store_true", help="per-file counts only")
    ap.add_argument("--show-skipped", action="store_true", help="explain filtering")
    args = ap.parse_args()

    roots = [Path(p) for p in (args.paths or ["kernel/src"])]
    files: list[Path] = []
    for root in roots:
        if root.is_file():
            files.append(root)
        else:
            files.extend(sorted(root.rglob("*.rs")))

    total = 0
    per_file: list[tuple[int, Path]] = []
    for f in files:
        findings, skipped = scan_file(f, args.show_skipped)
        if args.show_skipped and skipped:
            for lineno, reason, line in skipped:
                print(f"  skip {f}:{lineno}: ({reason}) {line}")
        if not findings:
            continue
        total += len(findings)
        per_file.append((len(findings), f))
        if not args.summary:
            for lineno, where, line in findings:
                print(f"{f}:{lineno}: [fn {where}] {line}")

    print()
    for count, f in sorted(per_file, reverse=True):
        print(f"{count:5d}  {f}")
    print(f"\n{total} production site(s) across {len(per_file)} file(s)")
    # Non-zero on findings, so `boot-test.sh` can gate on it. This was
    # documented from the start and returned 0 unconditionally anyway, which
    # made every caller's `if scan-unwrap.py; then` succeed regardless -- the
    # same shape of defect as the `validate()` in `mm/kvspace.rs` that was
    # documented "call once at boot" and called only from a self-test.
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
