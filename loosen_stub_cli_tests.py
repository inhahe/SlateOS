"""Loosen the over-strict stub-CLI tests created by commit 0816f75b.

The original sweep hard-asserted that `--version` and the default (empty
args) invocation exit with code 0 across all 2293 userspace CLI stubs.
That assumption is wrong for ~236 stubs:

- Many tools use a `version` subcommand (e.g., `argocd version`) and do
  not recognise `--version` as a top-level flag, so the dispatch falls
  through to the unknown-command branch and returns 1.
- Many tools legitimately exit non-zero when called with no arguments
  (e.g., `admesh` needs an input STL file).

Both behaviours are correct CLI design, not bugs. The fix is to weaken
the over-strict asserts to non-asserting calls, preserving the panic-
safety check (the original intent of "real tests, not assert!(true)")
without imposing arbitrary exit-code contracts.

`--help` and `-h` remain asserted to exit 0 — that one is a universal
CLI contract and every stub already honours it.

After this script runs, the test functions are also renamed to reflect
what they actually check:
    help_and_version_exit_zero  -> help_exits_zero
    default_invocation_exits_zero -> default_invocation_does_not_panic
"""

from __future__ import annotations

import re
from pathlib import Path

# Match `<ws>assert_eq!(<run_X(... "--version" ...)>, 0);` on a single line.
# Non-greedy `.*?` keeps the run_X(...) call balanced because run_X always
# closes before `, 0);`.
VERSION_PAT = re.compile(
    r'^(\s*)assert_eq!\((run_\w+\(.*?"--version"\.to_string\(\).*?\))\s*,\s*0\);\s*$',
    re.MULTILINE,
)

# Empty-args calls in all the shapes the original script emitted.
EMPTY_PATTERNS = [
    re.compile(r'^(\s*)assert_eq!\((run_\w+\(&\[\],\s*[^)]*\))\s*,\s*0\);\s*$', re.MULTILINE),
    re.compile(r'^(\s*)assert_eq!\((run_\w+\(&\[\]\))\s*,\s*0\);\s*$', re.MULTILINE),
    re.compile(r'^(\s*)assert_eq!\((run_\w+\(vec!\[\],\s*[^)]*\))\s*,\s*0\);\s*$', re.MULTILINE),
    re.compile(r'^(\s*)assert_eq!\((run_\w+\(vec!\[\]\))\s*,\s*0\);\s*$', re.MULTILINE),
    re.compile(r'^(\s*)assert_eq!\((run_\w+\("[^"]+",\s*&\[\]\))\s*,\s*0\);\s*$', re.MULTILINE),
]


def loosen(text: str) -> tuple[str, int]:
    changes = 0

    def sub_version(m: re.Match[str]) -> str:
        nonlocal changes
        changes += 1
        return f"{m.group(1)}let _ = {m.group(2)};"

    new = VERSION_PAT.sub(sub_version, text)

    def sub_empty(m: re.Match[str]) -> str:
        nonlocal changes
        changes += 1
        return f"{m.group(1)}let _ = {m.group(2)};"

    for pat in EMPTY_PATTERNS:
        new = pat.sub(sub_empty, new)

    # Rename the test functions to honestly describe what they check now.
    if "fn help_and_version_exit_zero" in new:
        new = new.replace("fn help_and_version_exit_zero", "fn help_exits_zero")
        changes += 1
    if "fn default_invocation_exits_zero" in new:
        new = new.replace("fn default_invocation_exits_zero", "fn default_invocation_does_not_panic")
        changes += 1

    return new, changes


def main() -> int:
    root = Path("userspace")
    candidates = list(root.glob("*/src/main.rs"))
    touched = 0
    for p in candidates:
        try:
            text = p.read_text(encoding="utf-8")
        except OSError:
            continue
        new, n = loosen(text)
        if n > 0 and new != text:
            p.write_text(new, encoding="utf-8")
            touched += 1
    print(f"Modified files: {touched}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
