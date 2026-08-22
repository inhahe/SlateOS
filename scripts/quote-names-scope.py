#!/usr/bin/env python3
"""How much of the tree would `diagnostics_quote_names` reject if it were widened?

That test (`userspace/coreutils/tests/diagnostics_quote_names.rs`) reads source
rather than behaviour, and today it reads exactly one directory:
`coreutils/src/bin`.  Every other utility crate in `userspace/` — including all
41 that duplicate a coreutils name — is outside it.

This script re-implements the test's two detectors in Python and reports, per
crate, how many lines each would flag.  It exists to price widening the test
before widening it: a sweep of N call sites is worth knowing about in advance.

Usage:  python scripts/quote-names-scope.py [--list] [root]
"""

from __future__ import annotations

import sys
from pathlib import Path

# Kept byte-for-byte in step with NOT_A_NAME in the Rust test.
NOT_A_NAME = {"msg", "e", "err", "error", "message", "reason"}


def bare_interpolated_name(line: str) -> str | None:
    """Port of `bare_interpolated_name` in diagnostics_quote_names.rs."""
    head, _, after = line.partition('eprintln!("')
    if not _:
        return None
    prog, _, tail = after.partition(": {")
    if not _:
        return None
    if not prog or not all(c.islower() or c.isdigit() or c == "_" for c in prog):
        return None
    ident, _, rest = tail.partition("}")
    if not _:
        return None
    if not ident or not all(c.isalnum() or c == "_" for c in ident):
        return None
    if not rest.startswith(": "):
        return None
    if ident in NOT_A_NAME:
        return None
    return ident


def hand_written_quotes(line: str) -> bool:
    """Port of the `no_diagnostic_hand_writes_quotes_around_a_name` detector."""
    if "eprintln!(" not in line and "println!(" not in line:
        return False
    return "'{" in line and "}'" in line


def is_prose(line: str) -> bool:
    """A comment, not code.

    The Rust test does not need this filter because it reads only
    `coreutils/src/bin`, where no comment happens to contain the pattern.  A
    tree-wide scan does: this very script's counterpart test flags seven of its
    own doc-comment lines.  Counting those as violations would overstate the
    price of widening, which is the one number this script exists to produce.
    """
    return line.lstrip().startswith("//")


def crate_of(path: Path, root: Path) -> str:
    rel = path.relative_to(root).parts
    return rel[0] if rel else "?"


def main() -> int:
    args = [a for a in sys.argv[1:] if a != "--list"]
    show_lines = "--list" in sys.argv[1:]
    root = Path(args[0]) if args else Path(__file__).resolve().parent.parent / "userspace"
    if not root.is_dir():
        print(f"not a directory: {root}", file=sys.stderr)
        return 2

    per_crate: dict[str, list[str]] = {}
    files = 0
    for path in root.rglob("*.rs"):
        if "target" in path.parts:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        files += 1
        crate = crate_of(path, root)
        for i, line in enumerate(text.splitlines(), 1):
            if is_prose(line):
                continue
            ident = bare_interpolated_name(line)
            if ident is not None:
                per_crate.setdefault(crate, []).append(
                    f"  {path.relative_to(root)}:{i}: {{{ident}}} unquoted\n"
                    f"      {line.strip()}"
                )
            elif hand_written_quotes(line):
                per_crate.setdefault(crate, []).append(
                    f"  {path.relative_to(root)}:{i}: hand-written quotes\n"
                    f"      {line.strip()}"
                )

    total = sum(len(v) for v in per_crate.values())
    print(f"scanned {files} .rs files under {root}")
    print(f"{total} would-be violations in {len(per_crate)} crates\n")
    for crate, hits in sorted(per_crate.items(), key=lambda kv: (-len(kv[1]), kv[0])):
        print(f"{len(hits):5}  {crate}")
        if show_lines:
            for h in hits:
                print(h)
    return 0


if __name__ == "__main__":
    sys.exit(main())
