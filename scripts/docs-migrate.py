#!/usr/bin/env python3
"""Move the entry documents to one file per entry (the 2026-10-02 cutover).

    python scripts/docs-migrate.py --check     # convert and verify in memory; write nothing
    python scripts/docs-migrate.py             # convert this checkout's documents in place

What it does, in one pass, from the five monolithic entry documents:

* `known-issues.md` + `known-issues-resolved.md` -> `known-issues/<ID>.md` (open) and
  `known-issues-resolved/<ID>.md` (closed), placed by each entry's own status line;
* `design-decisions.md` -> `design-decisions/NNNN-<slug>.md`, its header -> README.md;
* `open-questions.md` -> `open-questions/<ID>.md`, its `# Resolved` index ->
  `open-questions-resolved/lane-<x>.md`;
* `deferred-questions.md` -> `deferred-questions/<ID>.md`;
* each monolith -> a short signpost (about 2,000 files cite them by name);
* fully done `[x]` items of `roadmap.md` -> `roadmap-done.md`, under the same headings.

The conversion is `docs_layout.convert()`, and it is verified per element before a
byte is written (`docs_layout.verify()`): every line of every document accounted
for, every entry byte-identical in its file, every issue placed by its own status,
no two file names differing only in case. Afterwards the new tree is parsed back
and compared entry by entry. Any discrepancy aborts.

This is a one-time operation for the trunk. A lane branch that still has the old
files when it merges the cutover uses `docs-carry-forward.py`, which applies the
same conversion to the lane's own copies.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import doc_entries as D  # noqa: E402
import docs_layout as L  # noqa: E402


def dirty(root: Path, paths: list[str]) -> list[str]:
    r = subprocess.run(["git", "-C", str(root), "status", "--porcelain", "--", *paths], capture_output=True, text=True)
    return [ln for ln in r.stdout.splitlines() if ln.strip()]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="convert and verify only")
    ap.add_argument("--root", default=None)
    ap.add_argument("--date", default=date.today().isoformat(), help="cutover date for the signposts")
    args = ap.parse_args(argv)
    root = D.repo_root(Path(args.root) if args.root else None)

    layout = D.layout_of(root)
    if layout != "monolithic":
        print(f"this checkout's layout is {layout!r}; the migration converts a monolithic one only")
        return 2
    texts = {n: (root / n).read_text(encoding="utf-8") for n in D.MONOLITHS if (root / n).is_file()}
    roadmap = (root / "roadmap.md").read_text(encoding="utf-8") if (root / "roadmap.md").is_file() else None
    if (root / "roadmap-done.md").exists():
        print("roadmap-done.md already exists; refusing to guess how to merge into it")
        return 2
    conv = L.convert(texts, roadmap)
    problems = L.verify(conv)
    print("converted:", ", ".join(f"{k}={v}" for k, v in sorted(conv.counts.items())))
    print(f"files: {len(conv.files)}; documents replaced by signposts: {', '.join(conv.removed)}")
    if conv.notes:
        print(f"\n{len(conv.notes)} notes for a human:")
        for n in conv.notes:
            print("  -", n)
    if problems:
        print(f"\nVERIFICATION FAILED ({len(problems)}):")
        for p in problems:
            print("  -", p)
        return 1
    print("\nverification: every line accounted for, every entry byte-identical and placed by its status")
    if args.check:
        return 0

    touched = [*texts, "roadmap.md"]
    if d := dirty(root, touched):
        print("uncommitted changes to the documents being converted; commit or stash them first:\n  " + "\n  ".join(d))
        return 2
    for path, content in conv.files.items():
        p = root / path
        p.parent.mkdir(parents=True, exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as fh:
            fh.write(content)
    for name in conv.removed:
        with open(root / name, "w", encoding="utf-8", newline="") as fh:
            fh.write(L.signpost(name, args.date))

    # Parse the result back and compare it with what was converted, file by file.
    if D.layout_of(root) != "per-entry":
        print("after writing, the layout reads as", D.layout_of(root), "-- something is wrong")
        return 1
    back = {e.path: e for e in D.read_tree(root, kinds=["issue", "decision", "question", "deferred"])}
    if set(back) != set(conv.placed):
        print(f"parse-back: {len(set(conv.placed) - set(back))} files written but not read back as entries, "
              f"{len(set(back) - set(conv.placed))} read back that were not written")
        return 1
    changed = [p for p, e in back.items() if e.text != conv.files[p]]
    misfiled = [p for p, e in back.items()
                if e.kind == "issue" and (e.status == "open") != p.startswith(D.ISSUES_OPEN_DIR + "/")]
    if changed or misfiled:
        print(f"parse-back: {len(changed)} entries read back differently, {len(misfiled)} filed against their "
              f"status: {(changed + misfiled)[:10]}")
        return 1
    print(f"parse-back: {len(back)} entries read back from their files, unchanged and each in its place")
    print("\nnext: review `git status`, then commit the cutover")
    return 0


if __name__ == "__main__":
    sys.exit(main())
