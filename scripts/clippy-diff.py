#!/usr/bin/env python3
"""Compare two clippy logs by diagnostic *kind*, so a refactor can be cleared.

    (cd kernel && cargo clippy --message-format short > /tmp/before.txt 2>&1)
    ...make the change...
    (cd kernel && cargo clippy --message-format short > /tmp/after.txt 2>&1)
    python scripts/clippy-diff.py /tmp/before.txt /tmp/after.txt

Why this exists
---------------
The kernel emits ~18 300 clippy warnings from the defensive lint set that
`CLAUDE.md` mandates (`unwrap_used`, `indexing_slicing`,
`arithmetic_side_effects`, ...).  Against that background two obvious checks
both fail:

* **The total is not a signal.**  18 303 before and 18 303 after can still hide
  a new lint that displaced an old one, and any unrelated commit merged in the
  meantime moves the total anyway.
* **A textual diff is useless.**  Inserting a line into a 100 000-line file
  renumbers every diagnostic below it, so `diff` reports thousands of changes
  for a rewrite that introduced none.

Grouping by `(file, message)` and counting is immune to both.  A rewrite that
adds no lint prints "changed kinds: 0" no matter how much code moved -- which
is how the self-test splitting in
`A-LARGE-KERNEL-STACK-FRAMES-REMAIN-IN-SELF-TESTS-AND-KSHELL` was cleared, and
how the eight `clippy::unnecessary_wraps` it *did* introduce were caught.

Note the redirect: cargo writes diagnostics to **stderr**, and `2>&1` must come
after the `>`, or the log captures nothing and the terminal gets 7 MB.
"""

from __future__ import annotations

import argparse
import collections
import re
import sys

# `--message-format short`: `path\to\file.rs:LINE:COL: warning[lint]: message`.
# The path is matched loosely because it is host-shaped (backslashes on
# Windows) and may be relative or absolute.
PAT = re.compile(
    r"^([^\s:]+[\\/][^\s:]+\.rs):(\d+):(\d+): (warning|error)(\[[^\]]*\])?: (.*)"
)


def load(path: str, width: int) -> collections.Counter:
    counts: collections.Counter = collections.Counter()
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = PAT.match(line)
            if m:
                counts[(m.group(1).replace("\\", "/"), m.group(6)[:width])] += 1
    return counts


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("before")
    ap.add_argument("after")
    ap.add_argument(
        "--filter", help="only report files whose path contains this substring"
    )
    ap.add_argument(
        "--width",
        type=int,
        default=70,
        help="how much of each message to key on (default 70); a lint whose "
        "text embeds a variable name needs more",
    )
    ap.add_argument("--top", type=int, default=60, help="rows to print (default 60)")
    args = ap.parse_args()

    before = load(args.before, args.width)
    after = load(args.after, args.width)
    if not before or not after:
        # Silence here means the pattern matched nothing, which looks exactly
        # like "no diagnostics" -- and would be read as a clean result.
        sys.exit(
            f"error: parsed {sum(before.values())} diagnostics from "
            f"{args.before} and {sum(after.values())} from {args.after}. "
            f"Was the log written with `--message-format short`, and with "
            f"`> log 2>&1` rather than `2>&1 > log`?"
        )

    keys = set(before) | set(after)
    if args.filter:
        keys = {k for k in keys if args.filter in k[0]}
    rows = [(k, before.get(k, 0), after.get(k, 0)) for k in keys]
    rows = [r for r in rows if r[1] != r[2]]
    rows.sort(key=lambda t: -(t[2] - t[1]))

    print(f"{sum(before.values())} diagnostics -> {sum(after.values())}")
    print(f"{'before':>7}  {'after':>7}  file / message")
    for k, x, y in rows[: args.top]:
        print(f"{x:7d}  {y:7d}  {k[0]}  {k[1]}")
    if len(rows) > args.top:
        print(f"   ... {len(rows) - args.top} more")
    print(f"changed kinds: {len(rows)}")
    return 1 if rows else 0


if __name__ == "__main__":
    sys.exit(main())
