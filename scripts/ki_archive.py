#!/usr/bin/env python3
"""Move lane B's resolved entries from `known-issues.md` into the archive.

Fulfils `requests/c-b-known-issues-archive.md`. Lane B owns ~666 of the file's
993 entries (the `TD-OILS-*`/`BUG-OILS-*` shell-compatibility family plus
`TD-POSIX-*`/`BUG-POSIX-*`), which is the bulk of a 68k-line document that every
lane pays to read.

Structure comes from `ki_split.parse`, which is fence-aware — see its docstring
for why a `^#` grep cannot be used here.

**The verification is the point of this script, not the move.** Cutting 39k
lines out of a document by line number is trivially capable of tearing an entry
in half or dropping one silently, and neither would be visible by eye. So the
result is checked by multiset: every line of the original must appear exactly
as many times across the two output files, plus the header text this script
knowingly adds. Anything else aborts before a byte is written.

    python scripts/ki_archive.py --dry-run   # report only
    python scripts/ki_archive.py             # write both files
"""

from __future__ import annotations

import argparse
import collections
import io
import re
import sys

sys.path.insert(0, __file__.rsplit("\\", 1)[0].rsplit("/", 1)[0])

from ki_split import KI, parse  # noqa: E402

ARCHIVE = "known-issues-resolved.md"
PLACEHOLDER = "*(none moved yet — see `requests/c-b-known-issues-archive.md`)*\n"

# An entry only moves once its fix "has been on `main` through a full boot
# test" — a fix that has not survived a boot is a claim, not a resolution. This
# script cannot observe boot history, so it approximates with a date cutoff:
# anything resolved within the last few days stays put, carrying its
# `**Status: FIXED**` stamp, and gets archived on a later run.
#
# This is not hypothetical caution. The first run of this script swept up
# `TD-POSIX-TIMES-FLAKE`, fixed hours earlier in the very commit series that
# added the script — exactly the case the rule exists to prevent.
ARCHIVE_CUTOFF = "2026-08-13"
_DATE = re.compile(r"(\d{4}-\d{2}-\d{2})")

SECTION_NOTE = """\
Moved from `known-issues.md` on 2026-08-16, fulfilling
`requests/c-b-known-issues-archive.md`. Every entry below carries an explicit
`FIXED` / `RESOLVED` / `CLOSED` marker in its heading; each is dated 2026-07-19
to 2026-08-10 and has therefore been on `main` through many boot tests.

Entries that are merely *accepted* — `WON'T FIX`, `NOT-A-BUG`, `WAIVED`,
`INTENTIONAL`, `MINOR`, `ACCEPTED DIVERGENCE` — were deliberately **not** moved.
They describe behaviour that is still current, so they belong in the file that
says what is currently true, even though nobody intends to change them.

"""


def load(path: str) -> list[str]:
    with io.open(path, "r", encoding="utf-8", newline="") as f:
        return f.readlines()


def save(path: str, lines: list[str]) -> None:
    with io.open(path, "w", encoding="utf-8", newline="") as f:
        f.writelines(lines)


def fence_count(lines: list[str]) -> int:
    return sum(1 for l in lines if re.match(r"^\s*(?:`{3,}|~{3,})", l))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    ki_lines, entries = parse(KI)
    candidates = [e for e in entries if e.lane_b and e.resolved]
    move, too_fresh = [], []
    for e in candidates:
        dates = _DATE.findall(e.status_text)
        (too_fresh if dates and max(dates) > ARCHIVE_CUTOFF else move).append(e)
    for e in too_fresh:
        print(f"  held back (fixed after {ARCHIVE_CUTOFF}): {e.entry_id}")
    if not move:
        print("nothing to archive")
        return 0

    # --- cut ----------------------------------------------------------------
    # Walk back-to-front so earlier entries' indices stay valid.
    cut: set[int] = set()
    moved_blocks: list[list[str]] = []
    for e in sorted(move, key=lambda e: e.start):
        cut.update(range(e.start, e.end))
        moved_blocks.append(e.lines)
    new_ki = [l for i, l in enumerate(ki_lines) if i not in cut]

    # --- splice into the archive's `# Lane B` section ------------------------
    arc = load(ARCHIVE)
    try:
        at = arc.index(PLACEHOLDER)
    except ValueError:
        print(f"ERROR: `# Lane B` placeholder not found in {ARCHIVE}", file=sys.stderr)
        return 1
    added: list[str] = SECTION_NOTE.splitlines(keepends=True)
    for blk in moved_blocks:
        added.extend(blk)
        if blk and not blk[-1].endswith("\n"):
            added.append("\n")
    new_arc = arc[:at] + added + arc[at + 1 :]

    # --- verify: nothing lost, nothing duplicated ---------------------------
    before = collections.Counter(ki_lines) + collections.Counter(arc)
    after = collections.Counter(new_ki) + collections.Counter(new_arc)
    # The two lines this script knowingly changes: the placeholder is consumed,
    # and SECTION_NOTE is introduced.
    after[PLACEHOLDER] += 1
    for l in SECTION_NOTE.splitlines(keepends=True):
        after[l] -= 1
    diff = {k: v for k, v in (after - before).items() if k.strip()}
    diff.update({k: -v for k, v in (before - after).items() if k.strip()})
    if diff:
        print("ERROR: line multiset changed — refusing to write", file=sys.stderr)
        for k, v in list(diff.items())[:20]:
            print(f"  {v:+d}  {k[:100]!r}", file=sys.stderr)
        return 1

    for name, body in ((KI, new_ki), (ARCHIVE, new_arc)):
        n = fence_count(body)
        if n % 2:
            print(f"ERROR: {name} has an odd fence count ({n})", file=sys.stderr)
            return 1

    print(f"entries moved : {len(move)}")
    print(f"{KI:<24} {len(ki_lines):>6} -> {len(new_ki):>6} lines")
    print(f"{ARCHIVE:<24} {len(arc):>6} -> {len(new_arc):>6} lines")
    print("multiset check: OK   fence parity: OK")
    if args.dry_run:
        print("(dry run — nothing written)")
        return 0

    save(KI, new_ki)
    save(ARCHIVE, new_arc)
    # Re-parse both to prove the results are still structurally sound.
    _, e2 = parse(KI)
    print(f"re-parsed {KI}: {len(e2)} headings, "
          f"{len([x for x in e2 if x.lane_b])} lane B remaining")
    return 0


if __name__ == "__main__":
    sys.exit(main())
