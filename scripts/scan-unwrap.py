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

2.  **Attributing a site to its enclosing function -- all of them.** Two
    earlier versions of this script got this wrong in opposite directions.
    Tracking brace depth is fragile: raw strings, char literals and macro
    bodies all perturb the count, and one slip silently reattributes hundreds
    of lines -- it mis-filed every site in `oci.rs::self_test` as top-level.
    Falling back to the *nearest preceding* `fn` is no better, because these
    self-tests routinely define helpers inside themselves:

        pub fn self_test() {
            fn case(...) { ... }        // <- nearest preceding fn
            foo().expect("register");   // <- but this is still test code
        }

    Nearest-preceding blames `case`, which is not named `*test*`, so all 25
    sites across five files were reported as production. What matters is not
    the nearest enclosing `fn` but whether *any* enclosing `fn` is a test, so
    this version keeps a **stack** of open functions, using indentation as the
    nesting proxy. That is reliable here because the tree is rustfmt-formatted:
    a function's body is indented strictly further than its `fn` line, and its
    closing `}` sits at exactly that line's indent.

A site is TEST if **any** function on the enclosing stack is named `*test*`
(which covers the `self_test` / `*_self_test` convention this kernel uses for
its boot suites), or if it carries `#[test]` / sits under `#[cfg(test)]`.
Doc-comment examples are skipped -- they are prose, not code.

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

FN_RE = re.compile(
    r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:default\s+|const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*"
    r"fn\s+([A-Za-z0-9_]+)"
)
CFG_TEST_MOD_RE = re.compile(r"#\[cfg\(test\)\]")
SITE_RE = re.compile(r"\.(unwrap|expect)\s*\(")
# `.expect(...)?` or `.unwrap()?` -- returns a Result, so not the panicking one.
FALLIBLE_RE = re.compile(r"\.(?:unwrap|expect)\s*\([^()]*\)\s*\?")
SELF_CALL_RE = re.compile(r"\bself\.(?:unwrap|expect)\s*\(")


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


def _top(stack: list[tuple[int, str, bool]]) -> tuple[str, bool]:
    """The name to report for a site, and whether *any* enclosing fn is a test.

    The name is the innermost function -- that is the useful one to print when
    going to fix a site -- but the test verdict is taken over the whole stack,
    because a helper defined inside `self_test` is test code however it is
    named.
    """
    if not stack:
        return "<top level>", False
    return stack[-1][1], any(is_test for _, _, is_test in stack)


def scan_file(path: Path, show_skipped: bool = False):
    """Return (production_findings, skipped) lists."""
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:  # pragma: no cover
        print(f"warning: cannot read {path}: {exc}", file=sys.stderr)
        return [], []

    lines = text.splitlines()

    # Pass 1: for every line, the stack of `fn`s enclosing it.
    #
    # The stack is keyed by indentation. A `fn` at indent I owns every following
    # line indented further than I, and is closed by the `}` at indent I. So on
    # each line we first pop any function the line has fallen out of, then push
    # if the line opens a new one. The `startswith("}")` distinction matters:
    # a line at indent exactly I closes the function only if it is that brace --
    # a multi-line signature's `) -> Foo {` also sits at indent I and must not.
    #
    # `#[cfg(test)]` on a *module* marks everything after it in that module.
    enclosing_at: list[tuple[str, bool]] = []  # index = lineno - 1
    stack: list[tuple[int, str, bool]] = []  # (indent, name, is_own_name_test)
    saw_cfg_test = False
    cfg_test_mod_line = None
    for i, raw in enumerate(lines, start=1):
        stripped = raw.strip()
        if stripped:
            indent = len(raw) - len(raw.lstrip())
            if stripped.startswith("}"):
                while stack and indent <= stack[-1][0]:
                    stack.pop()
            else:
                while stack and indent < stack[-1][0]:
                    stack.pop()

        if CFG_TEST_MOD_RE.search(raw):
            saw_cfg_test = True
            if re.search(r"\bmod\b", lines[i] if i < len(lines) else ""):
                cfg_test_mod_line = i
            enclosing_at.append(_top(stack))
            continue
        if stripped.startswith("#["):
            # Other attributes do not clear the pending cfg(test).
            enclosing_at.append(_top(stack))
            continue

        m = FN_RE.match(raw)
        if m:
            indent, name = len(m.group(1)), m.group(2)
            while stack and indent <= stack[-1][0]:
                stack.pop()
            stack.append((indent, name, saw_cfg_test or "test" in name.lower()))
        if stripped:
            saw_cfg_test = False
        enclosing_at.append(_top(stack))

    def enclosing(lineno: int) -> tuple[str, bool]:
        return enclosing_at[lineno - 1]

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
                skipped.append((i, f"in test fn `{name}`", raw.strip()))
            continue
        # A cfg(test) module past this point makes everything in it a test.
        if cfg_test_mod_line is not None and i > cfg_test_mod_line:
            if show_skipped:
                skipped.append((i, "in #[cfg(test)] mod", raw.strip()))
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
    return 0


if __name__ == "__main__":
    sys.exit(main())
