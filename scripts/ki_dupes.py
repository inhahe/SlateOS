#!/usr/bin/env python3
"""Detect entries that exist in BOTH `known-issues.md` and its resolved archive.

Why this exists
---------------
`scripts/ki_archive.py` performs a *move*: it deletes an entry from
`known-issues.md` in the same commit that appends it to
`known-issues-resolved.md`, and verifies the result by line multiset across
both files. That check is sound — but it is a check on **one commit**, and the
failure this script guards against happens **later, in a merge**.

Observed 2026-08-16: commit `6e76ce5df` archived 111 lane A entries (removing
them from `known-issues.md`); merge commit `72cc0f7a7` ("Merge
'origin/main' into lane-c") brought three of them straight back. Git resolves a
deleted-on-one-side / touched-on-the-other hunk by keeping the touched text, so
a lane that had edited an entry — even trivially, even in a neighbouring hunk
of the same region — resurrects it, and nothing complains: both files parse,
both are valid Markdown, the multiset check ran three commits ago and passed.

The result is two copies of one entry in two files, which is strictly worse
than either having it once or not at all: they drift silently, and the next
reader has no way to know which copy is current. In the case found, the live
copy was already stale — the archive's had a 217-line follow-up the live one
lacked.

So the invariant this enforces is not about any single edit; it is a standing
property of the pair of files:

    no entry title appears in both known-issues.md and known-issues-resolved.md

Run it after any merge that touched either file, and before archiving.

    python scripts/ki_dupes.py           # exit 0 clean, 1 if duplicates found

Entry boundaries come from `ki_split.parse`, which is fence-aware — a naive
`^#` scan tears entries in half on the ~1,600 code fences whose contents begin
with `#`. See that module's docstring.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import ki_split  # noqa: E402

LIVE = "known-issues.md"
ARCHIVE = "known-issues-resolved.md"


def find_duplicates(
    live_path: str = LIVE, archive_path: str = ARCHIVE
) -> list[tuple[ki_split.Entry, ki_split.Entry]]:
    """Return (live_entry, archive_entry) for every title present in both files.

    Matching is on the heading title, not the body: a resurrected entry is by
    definition one whose *body* may already have diverged, so requiring equal
    bodies would hide exactly the case that matters most.
    """
    _, live_entries = ki_split.parse(live_path)
    _, archive_entries = ki_split.parse(archive_path)
    archived = {e.title: e for e in archive_entries}
    return [(e, archived[e.title]) for e in live_entries if e.title in archived]


def _relation(live: ki_split.Entry, archived: ki_split.Entry) -> str:
    """How the two copies differ, for a report the reader can act on."""
    if live.lines == archived.lines:
        return "identical"
    if archived.lines[: len(live.lines)] == live.lines:
        return f"archive is a superset (+{len(archived.lines) - len(live.lines)} lines)"
    if live.lines[: len(archived.lines)] == archived.lines:
        return f"live is a superset (+{len(live.lines) - len(archived.lines)} lines)"
    return "DIVERGED — neither is a prefix of the other; merge by hand"


def main() -> int:
    dupes = find_duplicates()
    if not dupes:
        print(f"ok: no entry appears in both {LIVE} and {ARCHIVE}")
        return 0

    print(f"{len(dupes)} entr{'y' if len(dupes) == 1 else 'ies'} exist in BOTH files:\n")
    for live, archived in dupes:
        print(f"  {live.title}")
        print(f"    {LIVE}:{live.start + 1}-{live.end}  ({len(live.lines)} lines)")
        print(f"    {ARCHIVE}:{archived.start + 1}-{archived.end}  ({len(archived.lines)} lines)")
        print(f"    -> {_relation(live, archived)}")
    print(
        "\nThe archive is the copy of record for a resolved entry. Delete the\n"
        f"{LIVE} copy — after folding in anything it has that the archive\n"
        "lacks. Only the owning lane may do this to its own entries; for another\n"
        "lane's, file a request."
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
