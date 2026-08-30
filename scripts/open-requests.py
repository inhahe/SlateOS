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

Where it looks, and the third failure
-------------------------------------
The two failures above were hand-rolling; the third was this script, and it
failed in the *other* direction -- reporting work that was finished, five
requests' worth. Two independent causes, both about where the classifier looked
rather than what it matched:

* **The reply was outside the window.** The tail was a flat 25 lines, on the
  reasonable-sounding grounds that a reply lives at the end of a file. It does,
  but a reply is as long as it needs to be; 25 files had a resolution heading
  further from the end than that. `reply_start` now takes the heading's own
  position as the boundary, so the window fits the reply.
* **An unreadable status short-circuited the reply.** `**Status:** baselined,
  not fixed` matches neither vocabulary, and `classify` returned that "I cannot
  read this" verdict without going on to look at the `## Lane A's answer --
  RESOLVED` heading below it. A less-informative signal outranked a more-
  informative one; `status_verdict`'s third return value now distinguishes them.

The bias is not symmetric here and should not be made so. Both fixes only ever
widen what is read -- they cannot narrow it -- and the window deliberately still
excludes the essay between header and reply, because in this dropbox that essay
is often prose *about* status markers and would classify a request on a sentence
describing a different one.

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
import textwrap
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
REQUESTS_DIR = REPO_ROOT / "requests"

# `<from>-<to>-<slug>.md`, both lane letters single characters.
NAME_RE = re.compile(r"^([a-z])-([a-z])-(.+)\.md$")

# How much of a file to read when looking for a status marker. A request has two
# status-bearing regions -- a header stamp at the top and a reply section at the
# bottom -- and the essay between them must NOT be read: it is prose *about* the
# work, and in this dropbox it is frequently prose about the status protocol
# itself. Eight files discuss `**Status:**` markers in running text ("replace the
# `**Status:** unknown` block with the real outcome", "would show up as open in
# `open-requests.py`"), and reading the body would classify a finished request on
# the strength of a sentence describing someone else's.
#
# So the window stays, and only its lower edge is computed rather than guessed --
# see `reply_start`. `TAIL_LINES` is now just the floor for that edge, used when
# a file has no reply heading at all.
HEAD_LINES = 25
TAIL_LINES = 25

# A fence's contents are not markdown, and `#` inside one is a shell comment, not
# a heading. The dropbox carries 1691 fenced lines, a great many of them commented
# bash, so `^# resolved the race by ...` in an example block is one word away from
# being read as a `## Resolved` section. No such line exists today; the guard is
# here because the identical mistake has already been made once in this repo, on
# `known-issues.md`, and cost a re-parse of the whole file (see `ki_split.py`).
FENCE_RE = re.compile(r"^\s*(`{3,}|~{3,})")

# The headings that open a reply section, which is the region a *reply* occupies:
# from the heading to end of file. This is deliberately wider than
# DONE_HEADING_PATTERNS -- it locates the section, it does not judge it. A reply
# headed `## Update` still has its `**Status:**` read; it just is not counted as
# done for having a heading.
REPLY_SECTION_RE = re.compile(
    r"^#{1,4}\s+(?:lane [a-z]'s\b"
    r"|(?:resolved|answered|answer|done|fixed|landed|reply|response|update|outcome)\b)",
    re.IGNORECASE,
)

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
#
# The colon may sit inside the bold or outside it. `**Status:**` is what 167 of
# the 192 files write and what rule 2's example shows; `**Status**:` is what the
# other four write, and to a reader they are the same line -- Markdown renders
# both as bold "Status" then a colon, so the difference is invisible in the
# artifact the writer is looking at while they write it. To this script they
# were not the same: the marker did not match at all, so those four reported "no
# status marker" -- the phrasing reserved for a file that never said anything --
# while each in fact carried a plain verdict, two of them `FIXED on main as of
# 3ccbfcb99`. Typesetting decided whether the sentence was read, which is the
# same failure this pattern was rewritten to end. It is also the `unrecognised
# status` conflation arriving one step earlier: a reader is told the file was
# never stamped, so they go to stamp it, and find it already was.
#
# The marker is captured, not just skipped, so the reason line can quote the
# file as written. Reporting `**Status:** FIXED ...` for a file that says
# `**Status**: FIXED ...` would send a reader grepping for a string that is not
# there.
#
# Safe in a way that widening DONE_WORDS is not, and the distinction is the
# reason this change is made and that one is refused. This decides only *where*
# the classifier looks; the ranking, the negator guard and STATUS_WINDOW all
# then run over the same text and mean the same things. No word changes sense,
# so no open request can become done by it.
#
# Deliberately no bare `Status:`. The two bold forms are unambiguous markers;
# an unemphasised one is a word that appears in prose ("the status: unclear"),
# and the cost of matching prose is a false clear, which is the direction this
# report must not fail in.
STATUS_BLOCK_RE = re.compile(
    r"(\*\*status(?::\*\*|\*\*[ \t]*:))(.*?)(?:\n[ \t]*\n|\Z)",
    re.IGNORECASE | re.DOTALL,
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
# 120 measured against the dropbox as it stands (192 files): the count of open
# files is flat from 80 to 240 (32, 32, 31, 33, 35) and bottoms at 120. Below
# it, done words that wrap fall outside and their files read as unclassifiable;
# above it, prose about neighbouring requests starts being read as this one's
# status. Both tails fail toward "open", which is why the curve is shallow and
# why picking the minimum is safe rather than clever.
#
# Re-measured 2026-08-29, after the read window changed from a flat 25-line tail
# to the reply section proper (see `reply_start`). The minimum did not move; the
# whole curve dropped by ~6 because resolutions that had been outside the window
# are now inside it.
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

# The glyphs. This dropbox does not only stamp statuses in words -- 149 of the
# status blocks in 192 files open with `✅`, and `roadmap.md` rule 2 *instructs*
# the lanes to write `**Status:** ⏳ ask 1 landed …; ask 2 blocked on lane <x>`.
# The script read none of them, so a status whose meaning was carried by the
# glyph and whose words happened to fall outside the vocabulary was reported
# unrecognised, and an unrecognised status is one the reader learns to skim.
#
# Three files were being reported open on that basis, each stamped with a tick
# and a word the list does not have: `✅ **TAKEN by lane A.**`,
# `✅ **ACKNOWLEDGED … the argument is accepted.**`, `✅ **REBUILT … twice**`.
# All three were checked by hand and all three are genuinely finished.
#
# The glyph is a better signal than those words would be, which is why this is
# the fix and widening DONE_WORDS is not. `taken`, `acknowledged` and `accepted`
# all have a live reading -- "seen, not yet done" -- so putting them in the done
# list would clear requests that are still work. The tick has no such reading:
# nobody stamps `✅` on something they have not finished. Measured over the
# whole dropbox the two agree 97.7% of the time (129 status blocks carry a tick,
# 126 already read done from their words), and every one of the three
# disagreements is a case where the tick is right.
#
# The open glyphs change no verdict today -- all three files that use one also
# say `blocked` in words. They are here because the rule the roadmap gives the
# lanes is "put the glyph in the status", and a lane that follows it without
# also writing `blocked` would otherwise get a false clear on the `landed` in
# the same sentence. Reading the marker the convention asks for costs nothing
# and closes that hole before someone falls in it.
#
# Only U+2705 is listed for done, because only U+2705 is used: a survey of every
# non-ASCII character inside a status window found ✅ 149 times and no other
# tick -- no ✔, no ✔️, so no variation-selector case to get wrong.
DONE_MARK_RE = re.compile("\u2705")  # ✅
OPEN_MARK_RE = re.compile("[\u23f3\u26d4]")  # ⏳ ⛔

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


def status_verdict(text: str) -> tuple[bool, str, bool] | None:
    """``(is_open, matched_text, decisive)`` from the `**Status:**` blocks, or None.

    Every block is considered, not just the first: a file that carries a header
    stamp and an appended reply section has two, and the reader wants the answer
    that accounts for both. Open wins over done for the reason in NEGATOR_RE.

    `decisive` says whether a vocabulary word or glyph was actually matched, as opposed
    to the fallback below where a `**Status:**` block exists but says nothing
    this function recognises. Both cases report open, so the flag makes no
    difference to *this* function's answer -- it exists so `classify()` can tell
    "I read a status and it means open" from "I found a status and could not
    read it", and consult the reply headings before settling for the latter.
    Without the distinction, `b-a-raw-nic-claim-tests-race-....md` -- header
    stamped "baselined, not fixed", then answered in August under
    `## Lane A's answer -- RESOLVED` -- was reported open for six days: the
    unreadable header short-circuited before the heading was ever looked at.
    """
    # `(marker, block)`: the marker as the file typeset it, so the reason line
    # quotes something a reader can actually grep for.
    blocks = [
        (m.group(1), m.group(2)[:STATUS_WINDOW]) for m in STATUS_BLOCK_RE.finditer(text)
    ]
    done_hit: str | None = None
    for marker, block in blocks:
        if OPEN_WORD_RE.search(block) or OPEN_MARK_RE.search(block):
            return (True, f"{marker}{block}".strip()[:80], True)
        # Words and glyphs are one vocabulary, ranked the same way: open beats
        # done, and a done hit is only a hit if no negator precedes it. Merging
        # the positions rather than checking the two regexes in sequence keeps
        # that true of `✅ not fixed yet` as well as of `not fixed yet`.
        starts = sorted(
            [m.start() for m in DONE_WORD_RE.finditer(block)]
            + [m.start() for m in DONE_MARK_RE.finditer(block)]
        )
        for start in starts:
            if not NEGATOR_RE.search(block[:start]):
                done_hit = done_hit or f"{marker}{block}".strip()[:80]
    if done_hit is not None:
        return (False, done_hit, True)
    if blocks:
        # A `**Status:**` line whose wording matches neither vocabulary. It is
        # open, because an unclassifiable status is not evidence of completion,
        # but say so differently from "no status marker at all" -- the two call
        # for different fixes, and telling them apart is what stops a reader
        # concluding the tool is simply noisy.
        return (True, f"unrecognised status: {blocks[0][1].strip()[:60]}", False)
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


def blank_fences(lines: list[str]) -> list[str]:
    """The same lines with every fenced region (and its fences) blanked out.

    Blanked rather than dropped so line numbers still line up with the file, and
    because a blank line terminates a `**Status:**` block, which is the correct
    thing for a fence to do to one anyway.
    """
    out: list[str] = []
    fence: str | None = None
    for line in lines:
        m = FENCE_RE.match(line)
        if m:
            tok = m.group(1)[0] * 3
            # A closing fence must be the same character as the opening one, so
            # a ``` inside a ~~~ block does not end it.
            fence = tok if fence is None else (None if tok == fence else fence)
            out.append("")
            continue
        out.append("" if fence is not None else line)
    return out


def reply_start(lines: list[str]) -> int:
    """Index of the last reply-section heading, or ``len(lines) - TAIL_LINES``.

    The tail used to be a flat "last 25 lines", on the stated grounds that a
    reply lives at the end of the file. It does -- but a reply is as long as it
    needs to be, and 25 lines stopped covering them. In today's dropbox 25 files
    carry a resolution heading that a 25-line tail cannot see;
    `b-a-raw-nic-claim-tests-race-...` answers the request at line 123 of 178 and
    was reported open for six days because of it.

    Taking the heading's own position as the boundary makes the window fit the
    reply instead of hoping a constant does. The floor keeps the old behaviour
    for files that have no reply heading, and `min` means this can only ever
    widen the window, never narrow it below what was read before.
    """
    floor = max(0, len(lines) - TAIL_LINES)
    for i in range(len(lines) - 1, -1, -1):
        if REPLY_SECTION_RE.match(lines[i]):
            return min(i, floor)
    return floor


def head_and_tail(path: Path) -> tuple[str, str]:
    """Return the header region and the reply region of a file as two strings.

    Read as UTF-8 with replacement rather than strictly: a request that someone
    pasted a stray byte into should still be classified, not crash the report.
    """
    try:
        raw = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError as exc:  # unreadable file -> treat as open, and say why
        return (f"<<unreadable: {exc}>>", "")
    lines = blank_fences(raw)
    return ("\n".join(lines[:HEAD_LINES]), "\n".join(lines[reply_start(lines) :]))


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
    # Precedence, strongest first: a recognised status word, then a reply
    # heading, then an unreadable status, then nothing. The middle two used to
    # be the other way round -- any `**Status:**` block at all returned here,
    # even one whose wording meant nothing to us -- and that is a
    # *less*-informative signal outranking a more-informative one. A heading
    # reading `## Lane A's answer -- RESOLVED` is an unambiguous statement that
    # the request was answered; "baselined, not fixed" in a header is a
    # sentence we failed to parse. Preferring the latter is how an answered
    # request stayed on the queue.
    verdict = status_verdict(both)
    if verdict is not None and verdict[2]:
        return (verdict[0], verdict[1], title)
    for pat in DONE_HEADING_PATTERNS:
        if m := pat.search(both):
            return (False, m.group(0).strip(), title)
    if verdict is not None:
        return (verdict[0], verdict[1], title)
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


def ascii_only(text: str) -> str:
    """Force ASCII, replacing anything else with `?`.

    Not cosmetic: this prints to a Windows console whose code page is cp1252,
    and a single emoji (request titles contain them, and so did an early draft
    of `UNRECOGNISED_HINT`) raises `UnicodeEncodeError` and kills the whole
    report mid-listing -- turning a tool whose entire job is "do not silently
    under-report" into one that silently under-reports.

    Separated from `elide` because the constraint is the console's, so it
    applies to *everything* this script prints, including its own hard-coded
    strings. Folded into `elide` it only protected the text that happened to be
    elided, and the first hard-coded line added afterwards crashed on the tick
    mark it contained.
    """
    return text.encode("ascii", "replace").decode("ascii")


def elide(text: str, width: int = 96) -> str:
    """Collapse whitespace, cut to `width`, and force ASCII."""
    text = " ".join(text.split())
    if len(text) > width:
        text = text[: width - 3] + "..."
    return ascii_only(text)


# Printed once, at the end, and only if something was reported as
# `unrecognised status`. The vocabulary is deliberately narrow -- see the
# asymmetry argument on NEGATOR_RE -- which means a writer *will* occasionally
# stamp a request with a perfectly clear English word this list does not hold
# ("ACKNOWLEDGED", "REBUILT" were the two that prompted this). Without the hint,
# fixing that requires reading this file to discover what the accepted words
# are; with it, the report says so where the complaint is.
#
# A hint rather than a wider vocabulary because the two are not equivalent. Some
# of the words that show up here are genuinely ambiguous -- "acknowledged" can
# mean "received, work pending" as easily as "agreed, nothing to do" -- and
# admitting one costs an open request vanishing from the only report that looks
# for it. Rewording one stamp is cheap; a done-word that sometimes means open is
# not.
def vocabulary_prose(alternation: str) -> str:
    """Render a regex alternation as a readable, de-duplicated word list.

    Derived, not restated. This list already exists twice -- as `DONE_WORDS`
    here and as the table in `roadmap.md` rule 2 -- and the two are held equal
    by `test-open-requests.py`. A third copy typed into a hint string would be
    the one nothing checks, so it would be the one that ends up naming a word
    the matcher does not know: a reader is told to write "acknowledged", writes
    it, and the request stays open forever with the hint itself as the cause.
    That is the failure the roadmap-table test was written to prevent, and
    hand-copying the list again would reintroduce it one layer down.
    """
    seen: dict[str, str] = {}
    for word in alternation.split("|"):
        # `wont ?fix` / `won't ?fix` are one word to a reader and to the
        # matcher; `_normalise` in the test suite collapses them the same way.
        pretty = word.replace(" ?", "").replace("?", "").replace("'", "")
        seen.setdefault(pretty.lower(), pretty)
    return ", ".join(seen.values())


UNRECOGNISED_HINT = tuple(
    textwrap.wrap(
        "hint: a `**Status:**` line is classified by its wording, not by the "
        "tick mark. Words read as finished: "
        f"{vocabulary_prose(DONE_WORDS)}. Reword the stamp to use one of them "
        "-- the rest of the line is free text -- rather than widening the "
        "list, which is how a word that sometimes means open gets in.",
        width=78,
    )
)


def report(entries, *, lane: str, outgoing: bool, show_all: bool) -> None:
    if show_all:
        groups = sorted({e[1] for e in entries})
    else:
        groups = [lane]

    saw_unrecognised = False
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
            saw_unrecognised = saw_unrecognised or reason.startswith("unrecognised")
            print(f"  [{frm}->{_to}] {path.name}  ({elide(reason, 40)})")
            print(f"           {elide(h1)}")
        if not open_ones and selected:
            print("  (none open)")
        print()

    if saw_unrecognised:
        for line in UNRECOGNISED_HINT:
            print(ascii_only(line))
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
