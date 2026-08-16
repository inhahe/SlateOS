#!/usr/bin/env python3
"""Fence-aware structural scanner for `known-issues.md`.

`known-issues.md` is ~68k lines and ~1.6k fenced code blocks, and a great many
lines *inside* those fences begin with `#` — shell comments in the bash/osh
comparison blocks (`# bash: …`, `# osh : …`) most of all. A naive `^#` grep
therefore mis-parses comments as headings and tears entries in half. Every
consumer of this file's structure must go through `parse()` here.

Used by `ki_archive.py`; also runnable directly for a report:

    python scripts/ki_split.py            # summarise entries by owner/status
"""

from __future__ import annotations

import io
import re
import sys
from dataclasses import dataclass, field

KI = "known-issues.md"

FENCE = re.compile(r"^\s*(?P<t>`{3,}|~{3,})")
HEADING = re.compile(r"^(?P<hashes>#{1,6})\s+(?P<title>.*)$")

# Entry-id prefixes owned by lane B (POSIX & userland). The `OILS` family is the
# Oils/osh shell-compatibility work; `POSIX` is the libc layer.
LANE_B_ID = re.compile(r"^(?:TD|BUG)-(?:OILS|POSIX)\b", re.IGNORECASE)

# Status markers are matched ONLY against the part of the heading after the
# entry id, never the whole heading. The ids are long hyphen-joined English
# sentences, so matching the whole line goes wrong in both directions:
#
#   …-TAKEN-FOR-ONE-THAT-CLOSED      -> false "resolved" (CLOSED is in the id)
#   …-EXTERNAL-ARGV0-IS-THE-RESOLVED-PATH … — OPEN (host-blocked)
#                                     -> false "resolved", and it is open
#   …-A-PENDING-HERE-DOCUMENT-… — ✅ FIXED
#                                     -> false "hedged" (PENDING is in the id)
#
# `[\s(*—-]` on the left boundary is load-bearing: the marker is very often
# bolded (`✅ **FIXED**`), and a `\b`-only match misses it behind the asterisk.
RESOLVED = re.compile(
    r"(?:^|[\s(*—-])(?:✅\s*)?\**(?:FIXED|RESOLVED|DONE|CLOSED)\b",
    re.IGNORECASE,
)
# An explicit OPEN overrides everything: several entries read
# "… — ✅ RESOLVED upstream — OPEN (host-blocked)" and are still open here.
OPEN_MARK = re.compile(r"(?:^|[\s(*—-])\**OPEN\b", re.IGNORECASE)
# "WON'T FIX" on its own is an accepted divergence that is still current
# behaviour, not a resolution — it stays in `known-issues.md`. ("✅ CLOSED
# WONTFIX" does archive, because CLOSED matches above and nothing here fires.)
WONTFIX_ONLY = re.compile(r"WON'?T\s*FIX|WONTFIX", re.IGNORECASE)
# What the status tail of a heading starts with: an ISO date, or a status word
# possibly behind a `✅`/`⛔`/`⚠️`/`⏳`/`⚖️` glyph and/or bold markers.
_TAIL_START = re.compile(
    r"^(?:\d{4}-\d{2}-\d{2}"
    r"|[^\w\s]*\s*\**(?:FIXED|RESOLVED|DONE|CLOSED|OPEN|WON'?T|WONTFIX|NOT[- ]A[- ]BUG"
    r"|WAIVED|MINOR|ACCEPTED|INTENTIONAL|PARTIALLY|MOSTLY|NOT\b|KNOWN|SUPERSEDED)\b)",
    re.IGNORECASE,
)
HEDGED = re.compile(
    r"\b(?:partly|partially|mostly|not\s+yet|pending|unverified|claimed)\b"
    r"|FIXED\s*\?|\bFIXED\s+IN\s+PART\b",
    re.IGNORECASE,
)


@dataclass
class Entry:
    """One `##`/`###` block: its heading line plus everything up to the next."""

    level: int
    title: str
    start: int  # 0-based index of the heading line
    end: int = 0  # exclusive
    lines: list[str] = field(default_factory=list)

    @property
    def entry_id(self) -> str:
        """The `TD-FOO-BAR` style id at the head of the title, or ''.

        Headings are not consistent about how the id ends: most use `. ` or
        ` — `, but some run straight into prose with a comma. So the id is
        taken as the leading run of hyphen-joined alphanumerics and nothing is
        assumed about the terminator. A bracketed lane tag (`[A] `) is stripped
        first, but a *bare* leading letter must not be — that would eat the `B`
        of `BUG-OILS-…`.
        """
        title = re.sub(r"^\[[A-C]\]\s*", "", self.title)
        m = re.match(r"([A-Za-z0-9]+(?:-[A-Za-z0-9]+)*)", title)
        return m.group(1) if m else ""

    @property
    def lane_b(self) -> bool:
        # Only `###` entries are archivable; `##` are section headers that
        # organise them (e.g. the `TD-OILS-*` scope gate) and must stay put.
        return self.level == 3 and bool(LANE_B_ID.match(self.entry_id))

    @property
    def status_text(self) -> str:
        """The heading's trailing status segment(s), plus any `**Status:**` line.

        Headings are `ID. prose — DATE — STATUS`, with any of the three tail
        parts optional. Only the tail may be searched for markers — the id is
        excluded for the reasons on `RESOLVED`, and the *prose* has to be
        excluded too, because it describes the bug in ordinary English:

            "… cannot copy a descriptor that is open …  — ✅ **RESOLVED**"
            "… stops holding the enclosing capture open … — RESOLVED"

        Both are resolved, and both look open if you match the whole line.

        The tail starts at the first em-dash segment that opens with a date or
        a status word. A heading with no em dash at all yields "" — i.e. reads
        as open — which is the safe direction.
        """
        title = re.sub(r"^\[[A-C]\]\s*", "", self.title)
        body = title[len(self.entry_id) :]
        segs = re.split(r"\s+—\s+", body)
        tail = ""
        for i in range(1, len(segs)):
            if _TAIL_START.match(segs[i].strip()):
                tail = " — ".join(segs[i:])
                break
        for ln in self.lines[1:8]:
            if ln.lstrip().startswith("**Status"):
                tail += " " + ln
        return tail

    @property
    def resolved(self) -> bool:
        """Fixed per the heading *or* a `**Status:**` line, without hedging.

        Precedence is deliberately conservative: any explicit `OPEN`, any
        hedge, or a bare `WON'T FIX` keeps the entry in `known-issues.md`. The
        cost of leaving a resolved entry in place is a few lines of noise; the
        cost of archiving an open one is a live bug filed away in the document
        nobody re-reads.
        """
        s = self.status_text
        if OPEN_MARK.search(s) or HEDGED.search(s):
            return False
        if WONTFIX_ONLY.search(s) and not re.search(r"\**CLOSED\b", s, re.IGNORECASE):
            return False
        return bool(RESOLVED.search(s))


def parse(path: str = KI) -> tuple[list[str], list[Entry]]:
    """Return (all lines, entries) with fenced regions excluded from headings."""
    with io.open(path, "r", encoding="utf-8", newline="") as f:
        lines = f.readlines()

    entries: list[Entry] = []
    fence: str | None = None
    for i, raw in enumerate(lines):
        m = FENCE.match(raw)
        if m:
            tok = m.group("t")
            if fence is None:
                fence = tok[0] * 3
                continue
            # A closing fence must be at least as long and of the same char.
            if tok[0] * 3 == fence and len(tok) >= 3:
                fence = None
            continue
        if fence is not None:
            continue
        h = HEADING.match(raw)
        if h and len(h.group("hashes")) in (2, 3):
            if entries:
                entries[-1].end = i
            entries.append(
                Entry(level=len(h.group("hashes")), title=h.group("title").strip(), start=i)
            )
    if entries:
        entries[-1].end = len(lines)
    for e in entries:
        e.lines = lines[e.start : e.end]
    if fence is not None:
        raise SystemExit("unbalanced code fence — refusing to report structure")
    return lines, entries


def main() -> int:
    lines, entries = parse()
    b = [e for e in entries if e.lane_b]
    br = [e for e in b if e.resolved]
    print(f"{len(lines)} lines, {len(entries)} headings")
    print(f"lane B entries: {len(b)}  ({sum(len(e.lines) for e in b)} lines)")
    print(f"  resolved:     {len(br)}  ({sum(len(e.lines) for e in br)} lines)")
    print(f"  still open:   {len(b) - len(br)}")
    fams: dict[str, list[int]] = {}
    for e in b:
        k = "-".join(e.entry_id.split("-")[:2]).upper()
        fams.setdefault(k, [0, 0])
        fams[k][0] += 1
        fams[k][1] += 1 if e.resolved else 0
    for k, (n, r) in sorted(fams.items()):
        print(f"    {k:<12} {n:>4} entries, {r:>4} resolved")
    return 0


if __name__ == "__main__":
    sys.exit(main())
