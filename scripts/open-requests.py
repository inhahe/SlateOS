#!/usr/bin/env python3
"""List the `requests/` entries addressed to a lane that are still open.

Why this exists
---------------
The dropbox is `requests/<from>-<to>-<slug>.md`, and "what is still open for
me?" gets asked at the start of every task. Answering it by hand with `ls` and
`grep` went wrong twice in two days, both times in the direction that matters --
reporting *nothing open* when something was:

* ``ls requests/ | grep -E '^[bc]-a-' | head -20`` truncated at twenty lines.
  The `a-b-*` files sort first, so the twenty shown were all outgoing and the
  incoming ones were cut off. **Never truncate a listing you are using to prove
  an absence** -- but better still, do not hand-roll the listing.
* ``grep '^\\*\\*Status:\\*\\*'`` missed four resolved requests whose marker sits
  mid-line (``**Filed:** ... **Status:** ...``) and would equally have missed an
  *open* one whose author wrote the header differently.

So the matching here is deliberately loose about *where* the marker is and
strict about *what counts as done*, and the failure mode is biased the safe way:
anything it cannot classify is reported as open, because a false "you have work"
costs one file read and a false "you are clear" costs a missed request.

Usage
-----
    python scripts/open-requests.py            # your lane, from CLAUDE_CONFIG_DIR
    python scripts/open-requests.py --lane b   # a specific lane
    python scripts/open-requests.py --all      # every lane, grouped
    python scripts/open-requests.py --outgoing # what you filed on others

Exit status is 0 whether or not anything is open -- this is a report, not a
gate. It is 2 only if the lane could not be determined or `requests/` is
missing.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
REQUESTS_DIR = REPO_ROOT / "requests"

# `<from>-<to>-<slug>.md`, both lane letters single characters.
NAME_RE = re.compile(r"^([a-z])-([a-z])-(.+)\.md$")

# How much of a file to read when looking for a status marker. Every marker in
# the dropbox today is in the first few lines (a header) or the last few (a
# reply section); reading both ends beats reading whole 200-line design essays.
HEAD_LINES = 25
TAIL_LINES = 25

# Phrases that mean "this one is finished". Matched case-insensitively anywhere
# in the head or tail, not anchored to a line start, because the marker is
# written inline as often as it is written as its own paragraph.
DONE_PATTERNS = [
    re.compile(p, re.IGNORECASE)
    for p in (
        r"\*\*status:\*\*[^\n]{0,80}?\b(done|landed|fixed|implemented|delivered|"
        r"resolved|answered|folded in|closed|declined|withdrawn|obsolete|"
        r"superseded|wont ?fix|won't ?fix)\b",
        r"^#{1,4}\s+(resolved|answered|done|fixed|landed)\b",
        r"^#{1,4}\s+lane [a-z]'s (answer|reply)\b",
    )
]

# A marker that says the opposite, and outranks the ones above -- a file whose
# header says "PARTIALLY DONE" or "reopened" is still work.
OPEN_PATTERNS = [
    re.compile(p, re.IGNORECASE)
    for p in (
        r"\*\*status:\*\*[^\n]{0,80}?\b(open|reopened|partial|partially|"
        r"in progress|blocked|pending|not started)\b",
    )
]

LANE_BY_CONFIG_DIR = {
    "": "a",
    ".claude": "a",
    ".claude-account-b": "b",
    ".claude-account-c": "c",
}


def detect_lane() -> str | None:
    """Derive the current lane from ``CLAUDE_CONFIG_DIR``, as which-lane.py does."""
    raw = os.environ.get("CLAUDE_CONFIG_DIR", "")
    key = Path(raw).name if raw else ""
    return LANE_BY_CONFIG_DIR.get(key)


def head_and_tail(path: Path) -> tuple[str, str]:
    """Return the first and last chunk of a file as two strings.

    Read as UTF-8 with replacement rather than strictly: a request that someone
    pasted a stray byte into should still be classified, not crash the report.
    """
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError as exc:  # unreadable file -> treat as open, and say why
        return (f"<<unreadable: {exc}>>", "")
    return ("\n".join(lines[:HEAD_LINES]), "\n".join(lines[-TAIL_LINES:]))


def title_of(head: str, path: Path) -> str:
    """The file's H1, which is what tells a request apart from a reply.

    The dropbox carries both -- `a-b-native-rlimit-syscalls-landed.md` is lane A
    telling lane B something shipped, not work for lane B to do -- and the
    filename convention does not distinguish them. Nothing in the text reliably
    does either, so this tool does not guess: it prints the title and lets the
    reader see "…-landed" at a glance instead of opening the file.
    """
    for line in head.splitlines():
        if line.startswith("# "):
            return line[2:].strip()
    return path.stem


def classify(path: Path) -> tuple[bool, str, str]:
    """``(is_open, reason, title)`` for one request file."""
    head, tail = head_and_tail(path)
    title = title_of(head, path)
    if head.startswith("<<unreadable:"):
        return (True, head, title)
    both = head + "\n" + tail
    for pat in OPEN_PATTERNS:
        m = pat.search(both)
        if m:
            return (True, m.group(0).strip(), title)
    for pat in DONE_PATTERNS:
        m = pat.search(both)
        if m:
            return (False, m.group(0).strip(), title)
    return (True, "no status marker", title)


def collect() -> list[tuple[str, str, Path, bool, str, str]]:
    """Every well-named request as ``(from, to, path, is_open, reason, title)``."""
    out: list[tuple[str, str, Path, bool, str, str]] = []
    for path in sorted(REQUESTS_DIR.glob("*.md")):
        m = NAME_RE.match(path.name)
        if not m:
            continue
        frm, to = m.group(1), m.group(2)
        is_open, reason, title = classify(path)
        out.append((frm, to, path, is_open, reason, title))
    return out


def elide(text: str, width: int = 96) -> str:
    """Collapse whitespace, cut to `width`, and force ASCII.

    The ASCII step is not cosmetic: this prints to a Windows console whose code
    page is cp1252, and a single emoji in a request title (there is one) raises
    `UnicodeEncodeError` and kills the whole report mid-listing -- turning a tool
    whose entire job is "do not silently under-report" into one that silently
    under-reports.
    """
    text = " ".join(text.split())
    if len(text) > width:
        text = text[: width - 3] + "..."
    return text.encode("ascii", "replace").decode("ascii")


def report(entries, *, lane: str, outgoing: bool, show_all: bool) -> None:
    if show_all:
        groups = sorted({e[1] for e in entries})
    else:
        groups = [lane]

    for to in groups:
        if outgoing:
            selected = [e for e in entries if e[0] == to]
            title = f"filed BY lane {to.upper()} on other lanes"
        else:
            selected = [e for e in entries if e[1] == to and e[0] != to]
            title = f"addressed TO lane {to.upper()}"
        open_ones = [e for e in selected if e[3]]
        print(f"=== requests {title}: {len(open_ones)} unresolved of {len(selected)} ===")
        for frm, _to, path, _is_open, reason, h1 in open_ones:
            print(f"  [{frm}->{_to}] {path.name}  ({elide(reason, 40)})")
            print(f"           {elide(h1)}")
        if not open_ones and selected:
            print("  (none open)")
        print()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--lane", help="lane letter to report for (default: yours)")
    ap.add_argument("--all", action="store_true", help="report every lane")
    ap.add_argument(
        "--outgoing",
        action="store_true",
        help="report what this lane filed on others, not what was filed on it",
    )
    args = ap.parse_args()

    if not REQUESTS_DIR.is_dir():
        print(f"open-requests: no {REQUESTS_DIR}", file=sys.stderr)
        return 2

    lane = (args.lane or detect_lane() or "").lower()
    if not args.all and lane not in {"a", "b", "c"}:
        print(
            "open-requests: could not determine your lane from CLAUDE_CONFIG_DIR; "
            "pass --lane a|b|c or --all",
            file=sys.stderr,
        )
        return 2

    report(collect(), lane=lane, outgoing=args.outgoing, show_all=args.all)
    return 0


if __name__ == "__main__":
    sys.exit(main())
