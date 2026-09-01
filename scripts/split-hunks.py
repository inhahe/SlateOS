#!/usr/bin/env python3
"""Stage a subset of one file's hunks, chosen by their position in the old file.

`git add -p` cannot be driven from here (it wants a terminal), and a file that
carries two unrelated changes still has to reach history as two commits. This
filters `git diff -U3 -- <path>` down to the hunks whose old-file start line
falls in [--from, --to) and feeds the result to `git apply --cached`.

Usage:
    python scripts/split-hunks.py <path> --from 0 --to 4445
"""

import argparse
import re
import subprocess
import sys

HUNK = re.compile(rb"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--from", dest="lo", type=int, default=0)
    ap.add_argument("--to", dest="hi", type=int, default=1 << 30)
    args = ap.parse_args()

    diff = subprocess.run(
        ["git", "diff", "-U3", "--", args.path],
        check=True,
        stdout=subprocess.PIPE,
    ).stdout

    lines = diff.split(b"\n")
    header: list[bytes] = []
    hunks: list[tuple[int, list[bytes]]] = []
    cur: list[bytes] | None = None
    start = 0

    for line in lines:
        m = HUNK.match(line)
        if m:
            if cur is not None:
                hunks.append((start, cur))
            start = int(m.group(1))
            cur = [line]
        elif cur is not None:
            cur.append(line)
        else:
            header.append(line)
    if cur is not None:
        hunks.append((start, cur))

    kept = [h for s, h in hunks if args.lo <= s < args.hi]
    if not kept:
        print("no hunks in range", file=sys.stderr)
        return 1

    out = b"\n".join(header + [l for h in kept for l in h])
    if not out.endswith(b"\n"):
        out += b"\n"

    subprocess.run(
        ["git", "apply", "--cached", "-"],
        input=out,
        check=True,
    )
    print(f"staged {len(kept)} of {len(hunks)} hunks from {args.path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
