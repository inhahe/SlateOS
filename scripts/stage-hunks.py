#!/usr/bin/env python3
"""Stage only the hunks of one file that mention a given marker string.

Splitting a documentation file across several commits is otherwise only possible
with `git add -p`, which needs a terminal. This does the same job from a script:

    python scripts/stage-hunks.py known-issues.md TD-OILS-WAIT-NO-OPERANDS-FLAKE

Every hunk of `git diff -- <file>` whose body contains any of the markers is
written to a patch and applied to the index with `git apply --cached`. The
working tree is untouched, so the remaining hunks stay available for the next
commit.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    path, markers = argv[1], argv[2:]
    diff = subprocess.run(
        ["git", "diff", "--", path],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    ).stdout
    if not diff:
        print(f"no unstaged changes in {path}", file=sys.stderr)
        return 1

    lines = diff.splitlines(keepends=True)
    starts = [i for i, ln in enumerate(lines) if ln.startswith("@@")]
    header = lines[: starts[0]]
    hunks = [lines[s : (starts[j + 1] if j + 1 < len(starts) else len(lines))] for j, s in enumerate(starts)]

    chosen = [h for h in hunks if any(m in "".join(h) for m in markers)]
    if not chosen:
        print(f"no hunk of {path} mentions any of {markers}", file=sys.stderr)
        return 1
    print(f"staging {len(chosen)} of {len(hunks)} hunk(s) of {path}")

    patch = "".join(header) + "".join("".join(h) for h in chosen)
    with tempfile.TemporaryDirectory() as td:
        pf = Path(td) / "hunks.patch"
        pf.write_text(patch, encoding="utf-8", newline="")
        subprocess.run(["git", "apply", "--cached", "--unidiff-zero", str(pf)], check=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
