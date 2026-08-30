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

# A status marker is a *paragraph*, not a line: everything from `**Status:**` to
# the next blank line.
#
# It used to be a line -- `\*\*status:\*\*[^\n]{0,80}?` -- and that made the
# verdict depend on typesetting rather than on what the sentence said. The same
# status, wrapped two ways:
#
#     **Status:** landed for ask 1, but ask 2 is blocked on lane B   -> open
#     **Status:** landed for ask 1, but ask 2 is
#     blocked on lane B                                              -> DONE
#
# The second is a false clear -- a request with live work reported finished --
# and it is produced by a line break. The two vocabularies below can only rank
# against each other when both are searched over the same text, so the block is
# extracted once and both look at it.
STATUS_BLOCK_RE = re.compile(
    r"\*\*status:\*\*(.*?)(?:\n[ \t]*\n|\Z)", re.IGNORECASE | re.DOTALL
)

# How far into a status block the deciding word may sit, in CHARACTERS.
#
# The distinction between characters and lines is the whole fix. The old
# pattern bounded the search with `[^\n]{0,80}`, which is a *line*, so a status
# that wrapped hid its own second clause. Counting characters instead spans the
# wrap, and the same sentence classifies the same way however it is typeset.
#
# A bound is still needed, because a status block is a paragraph and a paragraph
# talks about its neighbours. `b-a-cap-enumerating-query-syscall.md` is stamped
# LANDED and then adds "(§312 step 3 is a separate, still-open request …)" 330
# characters in -- searching the whole block reported that finished request as
# open, on the strength of a word about a different one.
#
# 120 measured against the dropbox as it stands (191 files): the count of open
# files is flat from 80 to 240 (38, 37, 36, 38, 40) and bottoms at 120. Below
# it, done words that wrap fall outside and their files read as unclassifiable;
# above it, prose about neighbouring requests starts being read as this one's
# status. Both tails fail toward "open", which is why the curve is shallow and
# why picking the minimum is safe rather than clever.
STATUS_WINDOW = 120

# Words that mean "finished", and words that mean "still work".
DONE_WORDS = (
    r"done|landed|fixed|implemented|delivered|resolved|answered|folded in|"
    r"closed|declined|withdrawn|obsolete|superseded|fulfilled|consumed|"
    r"wont ?fix|won't ?fix"
)
OPEN_WORDS = (
    r"open|reopened|partial|partially|in progress|blocked|pending|not started"
)

# `\b` is the wrong boundary on the right-hand side, because this project writes
# `open-questions.md` constantly and `\bopen\b` matches inside it. That is not a
# hypothetical: `b-a-q47-floor-fired-for-real-and-here-is-the-refill-rate.md` is
# stamped `✅ FOLDED IN` and says where the value went -- "the refill rate is in
# `open-questions.md`" -- and a `\b` boundary read the filename as this request's
# own status and reported a finished request as open.
#
# So: reject a hyphen on the right (`open-questions`, `closed-loop`), allow one
# on the left (`still-open` really does mean open, and it is the tail of the
# compound that carries the meaning). `(?<!\w)` still refuses `reopen`, which
# has its own entry in the list anyway.
DONE_WORD_RE = re.compile(rf"(?<!\w)({DONE_WORDS})(?![\w-])", re.IGNORECASE)
OPEN_WORD_RE = re.compile(rf"(?<!\w)({OPEN_WORDS})(?![\w-])", re.IGNORECASE)

# A done word with a negator just before it is not a completion. Before this
# guard, `**Status:** not yet resolved` and `**Status:** never landed` were both
# reported *done* -- a status line saying in plain English that the work is
# unfinished, read as finished.
#
# Applied to the done vocabulary ONLY, and the asymmetry is the point. The two
# failure directions do not cost the same: a finished request misread as open
# costs one glance at a file, while an open one misread as finished disappears
# from the only report that looks for it. So the rule is to be eager to find
# "open" and reluctant to find "done", and a guard that makes DONE harder to
# match runs with that grain. The same guard on OPEN would run against it --
# it would turn "no longer blocked" into a clear, which is the trade this
# report should never make cheaply.
NEGATOR_RE = re.compile(
    r"\b(not|never|no|isn't|aren't|hasn't|haven't|cannot|can't|un)\b"
    r"[^.;:\n]{0,24}$",
    re.IGNORECASE,
)

# Headings that stand in for a status line, e.g. a `## Resolved` reply section.
# No negation guard: these match only a word immediately after the hashes, so
# there is no room for a negator to sit in front of it.
DONE_HEADING_PATTERNS = [
    re.compile(p, re.IGNORECASE | re.MULTILINE)
    for p in (
        r"^#{1,4}\s+(resolved|answered|done|fixed|landed)\b",
        r"^#{1,4}\s+lane [a-z]'s (answer|reply)\b",
    )
]


def status_verdict(text: str) -> tuple[bool, str] | None:
    """``(is_open, matched_text)`` from the `**Status:**` blocks, or None.

    Every block is considered, not just the first: a file that carries a header
    stamp and an appended reply section has two, and the reader wants the answer
    that accounts for both. Open wins over done for the reason in NEGATOR_RE.
    """
    blocks = [m.group(1)[:STATUS_WINDOW] for m in STATUS_BLOCK_RE.finditer(text)]
    done_hit: str | None = None
    for block in blocks:
        if OPEN_WORD_RE.search(block):
            return (True, f"**Status:**{block}".strip()[:80])
        for m in DONE_WORD_RE.finditer(block):
            if not NEGATOR_RE.search(block[: m.start()]):
                done_hit = done_hit or f"**Status:**{block}".strip()[:80]
    if done_hit is not None:
        return (False, done_hit)
    if blocks:
        # A `**Status:**` line whose wording matches neither vocabulary. It is
        # open, because an unclassifiable status is not evidence of completion,
        # but say so differently from "no status marker at all" -- the two call
        # for different fixes, and telling them apart is what stops a reader
        # concluding the tool is simply noisy.
        return (True, f"unrecognised status: {blocks[0].strip()[:60]}")
    return None

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
    # A blank line between the two halves, so a `**Status:**` at the very end of
    # the head cannot run into the first line of the tail and read as one block.
    both = head + "\n\n" + tail
    if (verdict := status_verdict(both)) is not None:
        return (verdict[0], verdict[1], title)
    for pat in DONE_HEADING_PATTERNS:
        if m := pat.search(both):
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
