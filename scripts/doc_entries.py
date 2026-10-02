#!/usr/bin/env python3
"""One reading of the shared documents' entries, for every tool that needs them.

The shared documents (`roadmap.md` rule 3) hold thousands of separately owned
entries: issues, decisions, questions, roadmap items, todo notes, requests. Many
tools need them as *entries* rather than as text -- the search index
(`docsearch.py`), the converter that moved the entry documents to one file per
entry (`docs-migrate.py`), the merge helper that carries a lane's in-flight edits
across that move (`docs-carry-forward.py`), and the gates (`check-docs.py`).
Every one of them reads entries through `read_tree()` here, so they cannot
disagree about where an entry starts or what it is called.

Two layouts are understood, because both exist at once during the cutover:

* **per entry** (the current layout): `known-issues/<id>.md` for open issues and
  `known-issues-resolved/<id>.md` for closed ones, `design-decisions/<NNNN>-<slug>.md`,
  `open-questions/<ID>.md`, `deferred-questions/<id>.md`. One file is one entry.
* **monolithic** (the layout it replaced): `known-issues.md`,
  `known-issues-resolved.md`, `design-decisions.md`, `open-questions.md`,
  `deferred-questions.md`. A lane branch keeps this layout until it merges the
  cutover, and the merge helper reads its old copies.

`todo.txt`, `roadmap.md` (+ `roadmap-done.md`), `requests/*.md` and the spec
files are read in place in both layouts; they were never split.

Monolith boundaries follow `ki_split.py` exactly (it is imported, not copied):
headings inside fenced code blocks are not headings (~1,600 fences in
`known-issues.md` hold `# bash:`-style comment lines); a `##` heading always starts
an issue entry; a `###` heading starts one only when it opens with an entry id,
because prose `###` headings ("The shape of the fix") are subsections. In the
decisions log only *numbered* `##` headings start an entry: six stray prose
`##` headings sit inside entries there and belong to them.
"""

from __future__ import annotations

import hashlib
import os
import re
import subprocess
from collections.abc import Iterable, Iterator
from dataclasses import dataclass, field
from pathlib import Path

import ki_split

# --- what lives where -------------------------------------------------------------

ISSUES_OPEN_DIR = "known-issues"
ISSUES_CLOSED_DIR = "known-issues-resolved"
DECISIONS_DIR = "design-decisions"
QUESTIONS_DIR = "open-questions"
QUESTIONS_RESOLVED_DIR = "open-questions-resolved"
DEFERRED_DIR = "deferred-questions"
ENTRY_DIRS = (ISSUES_OPEN_DIR, ISSUES_CLOSED_DIR, DECISIONS_DIR, QUESTIONS_DIR, DEFERRED_DIR)

ISSUES_MONO = "known-issues.md"
ISSUES_CLOSED_MONO = "known-issues-resolved.md"
DECISIONS_MONO = "design-decisions.md"
QUESTIONS_MONO = "open-questions.md"
DEFERRED_MONO = "deferred-questions.md"
MONOLITHS = (ISSUES_MONO, ISSUES_CLOSED_MONO, DECISIONS_MONO, QUESTIONS_MONO, DEFERRED_MONO)

# Files in an entry directory that are not entries.
NON_ENTRY_FILES = {"README.md"}

# A monolith that has been replaced by a directory is left as a short signpost,
# because ~2,000 files cite it by name. A signpost starts with this marker line.
SIGNPOST_MARK = "<!-- docs-layout: signpost -->"

KINDS = ("issue", "decision", "question", "deferred", "todo", "roadmap", "request", "spec")

# --- small helpers ---------------------------------------------------------------

DATE_RE = re.compile(r"\b(20\d\d-\d\d-\d\d)\b")
LANE_FIELD_RE = re.compile(r"\*\*Lane:\*\*\s*([A-F])\b")
LANE_TAG_RE = re.compile(r"^\s*`?\[([A-F])\]`?")
# The question files put the tag after the id: "D-Q7 — [D] The C library ...".
_LANE_TAG_AFTER_ID = re.compile(r"^\s*[A-Za-z0-9-]+:?\s+(?:—|--|-|–)\s+`?\[([A-F])\]`?")
LANE_WORD_RE = re.compile(r"\blane\s+([A-F])\b", re.IGNORECASE)
STATUS_LINE_RE = re.compile(r"^\s*\*\*Status:?\*\*:?\s*(.*)$")
DECISION_HEADING_RE = re.compile(r"^##\s+(§)?(\d+)\s*[.—–-]\s*(.*)$")
QUESTION_ID_RE = re.compile(r"^\s*(?:`?\[[A-F]\]`?\s*)?((?:[A-F]-)?Q\d+|DQ\d+)\b")
ROADMAP_ITEM_RE = re.compile(r"^(\s*)- \[([ xX\-~])\]\s?(.*)$")
_SLUG_BAD = re.compile(r"[^A-Za-z0-9._-]+")
_WIN_RESERVED = {"con", "prn", "aux", "nul", *(f"com{i}" for i in range(10)), *(f"lpt{i}" for i in range(10))}
MAX_NAME = 140  # characters before ".md"; keeps worktree paths well under MAX_PATH


def slugify(text: str, keep_case: bool = False) -> str:
    """A filesystem-safe name: ASCII letters, digits, `.`, `_`, `-`."""
    text = text.replace("§", "s").replace("`", "")
    s = _SLUG_BAD.sub("-", text).strip("-.")
    s = re.sub(r"-{2,}", "-", s)
    if not keep_case:
        s = s.lower()
    if not s:
        s = "untitled"
    if len(s) > MAX_NAME:
        digest = hashlib.sha1(text.encode("utf-8")).hexdigest()[:8]
        s = s[: MAX_NAME - 9].rstrip("-.") + "-" + digest
    if s.lower() in _WIN_RESERVED:
        s += "-"
    return s


def first_date(*texts: str) -> str:
    for t in texts:
        m = DATE_RE.search(t or "")
        if m:
            return m.group(1)
    return ""


def lane_of(title: str, body_head: str, key: str = "") -> str:
    """The owning lane, from what the entry itself says, in order: a `[X]` tag, a
    `**Lane:**` field, "lane X" in the heading, a `TD-X-`/`BUG-X-` id prefix (or
    lane B's `TD-OILS`/`TD-POSIX` families), an `A-`/`C-`..`F-` prefix. '' when
    nothing says.

    A bare letter prefix on an *issue* is deliberately not read as a lane: before
    the six-lane split the numbering used `B-` for "bug" and `D-` for debt
    (`B-MOUNT-ACCEPTS-UNREACHABLE-...`, `D-FUTEX-ATOMICS-...`, both archived under
    `# Lane A`, the second dated before lane D existed). Lanes' own entries say
    "(lane B, ...)" or carry a `[B]` tag. Question ids (`A-Q14`) are unambiguous."""
    m = LANE_TAG_RE.match(title) or _LANE_TAG_AFTER_ID.match(title)
    if m:
        return m.group(1)
    m = LANE_FIELD_RE.search(body_head)
    if m:
        return m.group(1)
    m = LANE_WORD_RE.search(title)
    if m:
        return m.group(1).upper()
    k = key or title
    m = re.match(r"^(?:TD|BUG)-([A-F])-", k)
    if m:
        return m.group(1)
    if ki_split.LANE_B_ID.match(k):
        return "B"
    m = re.match(r"^([A-F])-Q\d+$", k)
    if m:
        return m.group(1)
    return ""


def fence_aware_headings(lines: list[str]) -> list[tuple[int, int, str]]:
    """(0-based line index, level, title) for every heading outside a code fence.

    Same fence rules as `ki_split.parse`: an opening fence of backticks or tildes
    ends at the next fence line of the same character and at least the same
    length. An unbalanced fence is an error, never a guess."""
    out: list[tuple[int, int, str]] = []
    fence: str | None = None
    for i, raw in enumerate(lines):
        m = ki_split.FENCE.match(raw)
        if m:
            tok = m.group("t")
            if fence is None:
                fence = tok[0] * 3
                continue
            if tok[0] * 3 == fence and len(tok) >= 3:
                fence = None
            continue
        if fence is not None:
            continue
        h = ki_split.HEADING.match(raw)
        if h:
            out.append((i, len(h.group("hashes")), h.group("title").strip()))
    if fence is not None:
        raise ValueError("unbalanced code fence: refusing to guess entry boundaries")
    return out


_ID_LEAD = re.compile(r"^[\s*`]*(?:\[[A-F]\]\s*)?[\s*`]*")
_ID_TOKEN = re.compile(r"[A-Za-z0-9]+(?:-[A-Za-z0-9.]+)*")


def is_issue_entry_heading(level: int, title: str) -> bool:
    """Whether a heading starts an issue entry (vs a subsection of one).

    `ki_split.Entry.is_entry`'s rule -- every `##`, and a `###` only when it
    opens with an id -- plus one case it misses: a `###` carrying a lane tag,
    `### [E] The terminal's shell ran on pipes -- 2026-09-24`. That is how lanes
    E and F (and much of A) write new entries, as the file header asks ("your
    lane letter in the heading"); 179 such entries read as subsections of
    whatever preceded them under ki_split's rule alone."""
    if level == 2:
        return True
    return level == 3 and (ki_split.opens_with_entry_id(title) or bool(LANE_TAG_RE.match(title)))


def issue_id(title: str) -> str:
    """The `TD-FOO-BAR` id a heading opens with, past any `[X]` tag, bold or
    backticks ('' for a prose heading). `ki_split.Entry.entry_id` does not skip
    a leading backtick, and 248 headings open with one."""
    if not ki_split.opens_with_entry_id(title):
        return ""
    rest = _ID_LEAD.sub("", title, count=1)
    m = _ID_TOKEN.match(rest)
    return m.group(0).rstrip(".-") if m else ""


def strip_separator(chunk: list[str]) -> tuple[list[str], list[str]]:
    """Split trailing blank lines and one `---` rule (the inter-entry separator)
    off an entry's lines. Returns (entry lines, separator lines)."""
    end = len(chunk)
    while end > 1 and not chunk[end - 1].strip():
        end -= 1
    if end > 1 and chunk[end - 1].strip() in ("---", "***", "___"):
        end -= 1
        while end > 1 and not chunk[end - 1].strip():
            end -= 1
    return chunk[:end], chunk[end:]


# --- the entry model ---------------------------------------------------------------

@dataclass
class DocEntry:
    kind: str  # one of KINDS
    key: str  # stable id within its kind: an issue id, "§538", "A-Q14", a slug, ...
    title: str  # the heading text (without the leading #s)
    text: str  # the entry exactly as written: heading line + body
    path: str  # repo-relative path of the file holding it (forward slashes)
    line: int  # 1-based line of the heading in that file
    end_line: int  # 1-based last line
    lane: str = ""  # A-F, or '' when the entry does not say
    status: str = ""  # normalised per kind; see `status_of_*`
    date: str = ""  # first ISO date in the heading or the first lines
    number: int | None = None  # decisions: the section number
    section: str = ""  # enclosing context: a phase heading, a `# Lane X` group, ...
    extra: dict = field(default_factory=dict)

    @property
    def text_hash(self) -> str:
        return hashlib.sha1(self.text.encode("utf-8")).hexdigest()

    @property
    def location(self) -> str:
        return f"{self.path}:{self.line}"


# --- statuses ------------------------------------------------------------------------

_TAIL_SEP = re.compile(r"\s+(?:—|--|–)\s+")
_BOLD_STATUS_LINE = re.compile(r"^\s*\**(?:✅\s*)?\**(FIXED|RESOLVED|CLOSED|DONE|OPEN)\b", re.IGNORECASE)


def issue_status_text(title: str, lines: list[str]) -> str:
    """The parts of an entry that state its status: the heading's tail, any
    `**Status:**` line, and a bold status line opening the body.

    `ki_split.Entry.status_text`'s reading, widened in two places where it read
    clearly fixed entries as open (measured on 2026-10-02):

    * the heading tail may follow ` -- ` or ` – ` as well as ` — `
      ("`B-tar-READ-EVERY-PATH-AS-UTF-8` (lane B, 2026-08-29) -- **FIXED 2026-08-29**");
    * the body may open with a bold status line instead of a `**Status:**` field
      ("**FIXED 2026-09-14 — all four.**").

    Only the *tail* of the heading is read, for the reason ki_split gives: ids and
    prose are English sentences that contain words like CLOSED and PENDING."""
    t = re.sub(r"^\s*`?\[[A-F]\]`?\s*", "", title)
    ident = issue_id(t)
    body = t[t.find(ident) + len(ident):] if ident else t
    segs = _TAIL_SEP.split(body)
    tail = ""
    for i in range(1, len(segs)):
        if ki_split._TAIL_START.match(segs[i].strip().lstrip("*`").strip()):  # noqa: SLF001 - shared grammar
            tail = " — ".join(segs[i:])
            break
    seen = 0
    for ln in lines[1:10]:
        s = ln.strip()
        if not s:
            continue
        seen += 1
        if s.startswith("**Status"):
            tail += " " + s
        elif seen <= 2 and _BOLD_STATUS_LINE.match(s):
            tail += " " + s.split(".")[0]
    return tail


def issue_status(title: str, lines: list[str], archived: bool) -> str:
    """'closed' or 'open', reading the markers as conservatively as ki_split does:
    any OPEN or hedge ("partly", "pending", "FIXED?") keeps an entry open, because
    filing a live bug among the resolved costs far more than leaving a fixed one
    out. Anything in the resolved archive is closed by placement."""
    if archived:
        return "closed"
    s = issue_status_text(title, lines)
    if ki_split.OPEN_MARK.search(s) or ki_split.HEDGED.search(s):
        return "open"
    if ki_split.WONTFIX_ONLY.search(s) and not re.search(r"\**CLOSED\b", s, re.IGNORECASE):
        return "open"
    return "closed" if ki_split.RESOLVED.search(s) else "open"


_DECISION_REVERSED = re.compile(r"(?i)\b(SUPERSEDED|REVERSED|WITHDRAWN|REPLACED BY)\b")


def decision_status(title: str, head: str) -> str:
    if _DECISION_REVERSED.search(title) or any(
        _DECISION_REVERSED.search(m.group(1)) for m in map(STATUS_LINE_RE.match, head.splitlines()) if m
    ):
        return "superseded"
    return "decided"


_DEFERRED_STATE = re.compile(r"(?i)\b(PROMOTED|ANSWERED|DROPPED|CLOSED|WITHDRAWN)\b")


def deferred_status(title: str) -> str:
    m = _DEFERRED_STATE.search(title)
    return m.group(1).lower() if m else "deferred"


_REQUEST_DONE = re.compile(r"(?i)\b(LANDED|DONE|RESOLVED|CLOSED|ANSWERED|WITHDRAWN|DECLINED|SUPERSEDED)\b|✅")


def request_status(text: str) -> str:
    for ln in text.splitlines()[:30]:
        m = STATUS_LINE_RE.match(ln)
        if m:
            return "closed" if _REQUEST_DONE.search(m.group(1)) else "open"
    return "unknown"


_TODO_DONE = re.compile(r"(?:^|[\s(*—-])(?:✅\s*)?\**(DONE|FIXED|RESOLVED|COMPLETED?)\b")


def todo_status(first_line: str) -> str:
    return "done" if _TODO_DONE.search(first_line) else "open"


ROADMAP_STATUS = {" ": "open", "x": "done", "X": "done", "-": "in-progress", "~": "blocked"}


# --- monolith parsers -----------------------------------------------------------------

def parse_issues_monolith(text: str, path: str, archived: bool) -> tuple[list[str], list[DocEntry], list[str]]:
    """(preamble lines, entries, separator/section lines dropped between entries)."""
    lines = text.splitlines(keepends=True)
    heads = fence_aware_headings(lines)
    starts: list[tuple[int, str, str]] = []  # (index, title, section)
    section = ""
    boundaries: list[int] = []  # level-1 headings end an entry without starting one
    for i, level, title in heads:
        if level == 1:
            boundaries.append(i)
            m = re.match(r"(?i)lane\s+([A-F])\b", title)
            section = f"Lane {m.group(1).upper()}" if m else title
            continue
        if is_issue_entry_heading(level, title):
            starts.append((i, title, section))
    preamble_end = starts[0][0] if starts else len(lines)
    preamble = lines[:preamble_end]
    entries: list[DocEntry] = []
    dropped: list[str] = []
    stops = sorted([s[0] for s in starts] + boundaries + [len(lines)])
    for i, title, sect in starts:
        end = next(s for s in stops if s > i)
        chunk, sep = strip_separator(lines[i:end])
        dropped.extend(sep)
        # Lines between this entry's end and the next start that are a level-1
        # heading block (e.g. "# Lane B" + blank lines) are section structure.
        nxt = next((s[0] for s in starts if s[0] > i), len(lines))
        if end < nxt:
            dropped.extend(lines[end:nxt])
        key = issue_id(title)
        body_head = "".join(chunk[1:16])
        lane = lane_of(title, body_head, key)
        if not lane and sect.startswith("Lane "):
            lane = sect[-1]
        if not key:
            prose = re.sub(r"^\s*`?\[[A-F]\]`?\s*", "", title)
            key = (f"{lane}-" if lane else "") + slugify(prose)
        entries.append(DocEntry(
            kind="issue", key=key, title=title, text="".join(chunk), path=path,
            line=i + 1, end_line=i + len(chunk), lane=lane,
            status=issue_status(title, chunk, archived), date=first_date(title, body_head), section=sect,
            extra={"has_id": bool(key)},
        ))
    return preamble, entries, dropped


BAND_ROW_RE = re.compile(r"^\|\s*§(\d+)\s*[–—-]\s*§(\d+)\s*\|([^|]*)\|([^|]*)\|")
# §217-§220 sit in lane A's first band but are lane C's, permanently (the band
# table's own note, 2026-08-17); the table cannot express that, so it is here.
BAND_EXCEPTIONS = {217: "C", 218: "C", 219: "C", 220: "C"}


def parse_bands(text: str) -> list[tuple[int, int, str, str]]:
    """(lo, hi, lane or '', 'open'/'closed') per row of the numbering-band table."""
    out = []
    for ln in text.splitlines():
        m = BAND_ROW_RE.match(ln)
        if m:
            owner = LANE_WORD_RE.search(m.group(3))
            status = "open" if re.search(r"\bopen\b", m.group(4), re.IGNORECASE) else "closed"
            out.append((int(m.group(1)), int(m.group(2)), owner.group(1).upper() if owner else "", status))
    return out


def band_lane(number: int, bands: list[tuple[int, int, str, str]]) -> str:
    if number in BAND_EXCEPTIONS:
        return BAND_EXCEPTIONS[number]
    for lo, hi, lane, _status in bands:
        if lo <= number <= hi:
            return lane
    return ""


def parse_decisions_monolith(text: str, path: str) -> tuple[list[str], list[DocEntry], list[str]]:
    lines = text.splitlines(keepends=True)
    heads = fence_aware_headings(lines)
    starts = [(i, t) for i, lvl, t in heads if lvl == 2 and DECISION_HEADING_RE.match(lines[i])]
    preamble = lines[: starts[0][0]] if starts else lines
    bands = parse_bands("".join(preamble))
    entries: list[DocEntry] = []
    dropped: list[str] = []
    for n, (i, title) in enumerate(starts):
        end = starts[n + 1][0] if n + 1 < len(starts) else len(lines)
        chunk, sep = strip_separator(lines[i:end])
        dropped.extend(sep)
        m = DECISION_HEADING_RE.match(lines[i])
        number = int(m.group(2))
        head = "".join(chunk[1:16])
        entries.append(DocEntry(
            kind="decision", key=f"§{number}", title=title, text="".join(chunk), path=path,
            line=i + 1, end_line=i + len(chunk), lane=lane_of(title, head) or band_lane(number, bands),
            status=decision_status(title, head), date=first_date(head, title), number=number,
            extra={"slug": slugify(m.group(3))},
        ))
    return preamble, entries, dropped


def parse_questions_monolith(text: str, path: str, kind: str) -> tuple[list[str], list[DocEntry], list[str], list[str]]:
    """(preamble, entries, dropped separators, resolved-index lines).

    For `open-questions.md` everything from `# Resolved` on is the index of
    answered questions (one-line records), not entries. `deferred-questions.md`
    has no such section today; one titled `# Closed` would be treated the same."""
    lines = text.splitlines(keepends=True)
    heads = fence_aware_headings(lines)
    # The archive heading is matched exactly, as `check-open-questions.py` does:
    # a wrapped prose line inside C-Q28 begins "# Resolved` index at", and a
    # prefix match took it for the archive and lost the 15 questions after it.
    index_start = next((i for i, lvl, t in heads if lvl == 1 and t.strip() in ("Resolved", "Closed")), len(lines))
    starts = [(i, t) for i, lvl, t in heads if lvl == 2 and i < index_start]
    preamble = lines[: starts[0][0]] if starts else lines[:index_start]
    entries: list[DocEntry] = []
    dropped: list[str] = []
    for n, (i, title) in enumerate(starts):
        end = starts[n + 1][0] if n + 1 < len(starts) else index_start
        chunk, sep = strip_separator(lines[i:end])
        dropped.extend(sep)
        m = QUESTION_ID_RE.match(title)
        qid = m.group(1) if m else ""
        head = "".join(chunk[1:16])
        lane = lane_of(title, head, qid)
        if kind == "question":
            status = "open"
        else:
            status = deferred_status(title)
        key = qid or (f"{lane}-" if lane else "") + slugify(re.sub(r"^\s*`?\[[A-F]\]`?\s*", "", title))[:80]
        entries.append(DocEntry(
            kind=kind, key=key, title=title, text="".join(chunk), path=path, line=i + 1,
            end_line=i + len(chunk), lane=lane, status=status, date=first_date(title, head),
            extra={"has_id": bool(qid)},
        ))
    return preamble, entries, dropped, lines[index_start:]


# --- per-entry layout -------------------------------------------------------------------

def _read_entry_file(root: Path, rel: str, kind: str,
                     bands: list[tuple[int, int, str, str]] | None = None) -> DocEntry | None:
    p = root / rel
    try:
        text = p.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    lines = text.splitlines(keepends=True)
    heads = fence_aware_headings(lines) if lines else []
    if not heads:
        return None
    i, _level, title = heads[0]
    head = "".join(lines[i + 1 : i + 16])
    stem = Path(rel).stem
    if kind == "issue":
        archived = rel.startswith(ISSUES_CLOSED_DIR + "/")
        lane = lane_of(title, head, stem)
        if not lane:
            # A prose-titled entry is filed under "<lane>-<lowercase slug>" by
            # docs-migrate.py (its lane came from a `# Lane X` group in the old
            # archive). Real ids are upper-case, so the lower-case slug tells them apart.
            m = re.match(r"^([A-F])-[a-z0-9]", stem)
            lane = m.group(1) if m else ""
        return DocEntry(kind=kind, key=stem, title=title, text=text.rstrip("\n") + "\n", path=rel, line=i + 1,
                        end_line=len(lines), lane=lane, status=issue_status(title, lines[i:], archived),
                        date=first_date(title, head), extra={"has_id": True})
    if kind == "decision":
        m = DECISION_HEADING_RE.match(lines[i])
        number = int(m.group(2)) if m else None
        lane = lane_of(title, head) or (band_lane(number, bands or []) if number is not None else "")
        return DocEntry(kind=kind, key=f"§{number}" if number is not None else stem, title=title,
                        text=text.rstrip("\n") + "\n", path=rel, line=i + 1, end_line=len(lines),
                        lane=lane, status=decision_status(title, head),
                        date=first_date(head, title), number=number)
    status = "open" if kind == "question" else deferred_status(title)
    return DocEntry(kind=kind, key=stem, title=title, text=text.rstrip("\n") + "\n", path=rel, line=i + 1,
                    end_line=len(lines), lane=lane_of(title, head, stem), status=status,
                    date=first_date(title, head))


def is_signpost(path: Path) -> bool:
    try:
        with open(path, encoding="utf-8") as fh:
            return fh.readline().strip() == SIGNPOST_MARK
    except OSError:
        return False


def layout_of(root: Path) -> str:
    """'per-entry' when the entry directories exist and the monoliths are
    signposts or absent, 'monolithic' when the monoliths hold entries, and
    'mixed' when both hold entries (an unfinished merge or a stray old-format
    file: `check-docs.py` reports it)."""
    dirs = any((root / d).is_dir() for d in ENTRY_DIRS)
    monos = any((root / m).is_file() and not is_signpost(root / m) for m in MONOLITHS)
    if dirs and monos:
        return "mixed"
    return "per-entry" if dirs else "monolithic"


# --- the in-place documents ----------------------------------------------------------

def parse_roadmap(text: str, path: str) -> list[DocEntry]:
    """Checkbox items with their continuation lines, under their nearest heading."""
    lines = text.splitlines(keepends=True)
    heads = {i: t for i, _lvl, t in fence_aware_headings(lines)}
    out: list[DocEntry] = []
    section = ""
    i = 0
    while i < len(lines):
        if i in heads:
            section = heads[i]
            i += 1
            continue
        m = ROADMAP_ITEM_RE.match(lines[i])
        if not m:
            i += 1
            continue
        indent = len(m.group(1))
        j = i + 1
        while j < len(lines) and lines[j].strip() and j not in heads and not ROADMAP_ITEM_RE.match(lines[j]) \
                and (len(lines[j]) - len(lines[j].lstrip())) > indent:
            j += 1
        body = m.group(3)
        tag = re.match(r"^`?\[([A-F])\]`?\s*", body)
        lane = tag.group(1) if tag else ""
        title = re.sub(r"\s+", " ", body).strip()
        out.append(DocEntry(
            kind="roadmap", key=f"{Path(path).stem}:{i + 1}", title=title[:300], text="".join(lines[i:j]),
            path=path, line=i + 1, end_line=j, lane=lane, status=ROADMAP_STATUS.get(m.group(2), "open"),
            date=first_date(body), section=section,
        ))
        i = j
    return out


_TODO_LANE = re.compile(r"^##\s+Lane\s+([A-F])\b")
_TODO_RULE = re.compile(r"^##\s*#{6,}\s*$")


def parse_todo(text: str, path: str) -> list[DocEntry]:
    """Paragraphs of `todo.txt` (whose lines are `##`-prefixed), each tagged with
    the `## Lane X` section it sits in; '' before the first lane section."""
    lines = text.splitlines(keepends=True)
    out: list[DocEntry] = []
    lane = ""
    section = ""
    start: int | None = None

    def flush(end: int) -> None:
        nonlocal start
        if start is None:
            return
        chunk = lines[start:end]
        body = "".join(chunk)
        if body.strip("#\n\r\t "):
            first = chunk[0].lstrip("# ").strip()
            out.append(DocEntry(
                kind="todo", key=f"todo:{start + 1}", title=first[:300], text=body, path=path, line=start + 1,
                end_line=end, lane=lane, status=todo_status(first), date=first_date(first), section=section,
            ))
        start = None

    for i, raw in enumerate(lines):
        s = raw.rstrip("\r\n")
        m = _TODO_LANE.match(s)
        if m:
            flush(i)
            lane, section = m.group(1), s.lstrip("# ").strip()
            continue
        if not s.strip() or s.strip() == "##" or _TODO_RULE.match(s):
            flush(i)
            continue
        if start is None:
            start = i
    flush(len(lines))
    return out


def parse_request(text: str, path: str) -> DocEntry:
    lines = text.splitlines(keepends=True)
    heads = fence_aware_headings(lines) if lines else []
    title = heads[0][2] if heads else Path(path).stem
    stem = Path(path).stem
    m = re.match(r"^([a-f])-([a-f]+)-", stem)
    return DocEntry(kind="request", key=stem, title=title, text=text, path=path, line=1, end_line=len(lines),
                    lane=m.group(1).upper() if m else "", status=request_status(text),
                    date=first_date(title, "".join(lines[:20])), extra={"to": m.group(2).upper() if m else ""})


def parse_spec(text: str, path: str, chunk_chars: int = 2400) -> list[DocEntry]:
    """Reference documents without entry structure, cut at blank lines into
    passages of about `chunk_chars`, each remembering its line range."""
    lines = text.splitlines(keepends=True)
    out: list[DocEntry] = []
    start, size = 0, 0
    for i, ln in enumerate(lines + ["\n"]):
        size += len(ln)
        if (not ln.strip() and size >= chunk_chars) or i == len(lines):
            chunk = lines[start:i]
            if "".join(chunk).strip():
                first = next((c.strip() for c in chunk if c.strip()), "")
                out.append(DocEntry(kind="spec", key=f"{Path(path).name}:{start + 1}", title=first[:200],
                                    text="".join(chunk), path=path, line=start + 1, end_line=i, status="spec"))
            start, size = i + 1, 0
    return out


SPEC_FILES = ("design.txt", "scheduler.txt", "ipc.txt", "memory management.txt", "roadmap-detailed.md",
              "performance-targets.md", "subsystem-map.md")


# --- reading a whole tree ------------------------------------------------------------------

def _rel(p: Path, root: Path) -> str:
    return p.relative_to(root).as_posix()


def read_tree(root: Path, kinds: Iterable[str] = KINDS) -> list[DocEntry]:
    """Every entry in a checkout, whichever layout it is in."""
    kinds = set(kinds)
    out: list[DocEntry] = []
    root = Path(root)
    readme = root / DECISIONS_DIR / "README.md"
    bands = parse_bands(readme.read_text(encoding="utf-8")) if readme.is_file() else []
    for d, kind in ((ISSUES_OPEN_DIR, "issue"), (ISSUES_CLOSED_DIR, "issue"), (DECISIONS_DIR, "decision"),
                    (QUESTIONS_DIR, "question"), (DEFERRED_DIR, "deferred")):
        if kind in kinds and (root / d).is_dir():
            for p in sorted((root / d).glob("*.md")):
                if p.name not in NON_ENTRY_FILES:
                    e = _read_entry_file(root, _rel(p, root), kind, bands)
                    if e:
                        out.append(e)
    monos = ((ISSUES_MONO, "issue"), (ISSUES_CLOSED_MONO, "issue"), (DECISIONS_MONO, "decision"),
             (QUESTIONS_MONO, "question"), (DEFERRED_MONO, "deferred"))
    for name, kind in monos:
        p = root / name
        if kind not in kinds or not p.is_file() or is_signpost(p):
            continue
        text = p.read_text(encoding="utf-8")
        if kind == "issue":
            out.extend(parse_issues_monolith(text, name, archived=(name == ISSUES_CLOSED_MONO))[1])
        elif kind == "decision":
            out.extend(parse_decisions_monolith(text, name)[1])
        else:
            out.extend(parse_questions_monolith(text, name, kind)[1])
    if "roadmap" in kinds:
        for name in ("roadmap.md", "roadmap-done.md"):
            if (root / name).is_file():
                out.extend(parse_roadmap((root / name).read_text(encoding="utf-8"), name))
    if "todo" in kinds and (root / "todo.txt").is_file():
        out.extend(parse_todo((root / "todo.txt").read_text(encoding="utf-8"), "todo.txt"))
    if "request" in kinds and (root / "requests").is_dir():
        for p in sorted((root / "requests").glob("*.md")):
            if p.name not in NON_ENTRY_FILES:
                out.append(parse_request(p.read_text(encoding="utf-8"), _rel(p, root)))
    if "spec" in kinds:
        for name in SPEC_FILES:
            if (root / name).is_file():
                out.extend(parse_spec((root / name).read_text(encoding="utf-8", errors="replace"), name))
    return out


def source_files(root: Path) -> list[str]:
    """Every file `read_tree` reads, repo-relative, for change detection."""
    root = Path(root)
    out: list[str] = []
    for d in ENTRY_DIRS:
        if (root / d).is_dir():
            out.extend(_rel(p, root) for p in sorted((root / d).glob("*.md")) if p.name not in NON_ENTRY_FILES)
    if (root / DECISIONS_DIR / "README.md").is_file():
        out.append(f"{DECISIONS_DIR}/README.md")  # its band table feeds lane attribution
    for name in (*MONOLITHS, "roadmap.md", "roadmap-done.md", "todo.txt", *SPEC_FILES):
        if (root / name).is_file():
            out.append(name)
    if (root / "requests").is_dir():
        out.extend(_rel(p, root) for p in sorted((root / "requests").glob("*.md")) if p.name not in NON_ENTRY_FILES)
    return out


def read_file(root: Path, rel: str) -> list[DocEntry]:
    """The entries of one source file (see `source_files`)."""
    root = Path(root)
    p = root / rel
    if not p.is_file():
        return []
    top = rel.split("/", 1)[0]
    kind_by_dir = {ISSUES_OPEN_DIR: "issue", ISSUES_CLOSED_DIR: "issue", DECISIONS_DIR: "decision",
                   QUESTIONS_DIR: "question", DEFERRED_DIR: "deferred"}
    if "/" in rel and top in kind_by_dir:
        if Path(rel).name in NON_ENTRY_FILES:
            return []
        bands: list = []
        if kind_by_dir[top] == "decision":
            readme = root / DECISIONS_DIR / "README.md"
            bands = parse_bands(readme.read_text(encoding="utf-8")) if readme.is_file() else []
        e = _read_entry_file(root, rel, kind_by_dir[top], bands)
        return [e] if e else []
    if top == "requests" and "/" in rel:
        return [parse_request(p.read_text(encoding="utf-8"), rel)]
    if rel in MONOLITHS:
        return [] if is_signpost(p) else list(iter_monolith_entries(p.read_text(encoding="utf-8"), rel))
    if rel in ("roadmap.md", "roadmap-done.md"):
        return parse_roadmap(p.read_text(encoding="utf-8"), rel)
    if rel == "todo.txt":
        return parse_todo(p.read_text(encoding="utf-8"), rel)
    if rel in SPEC_FILES:
        return parse_spec(p.read_text(encoding="utf-8", errors="replace"), rel)
    return []


def read_blob(rev: str, path: str, repo: Path) -> str | None:
    """A file's text at `rev` (None if it does not exist there)."""
    r = subprocess.run(["git", "-C", str(repo), "show", f"{rev}:{path}"], capture_output=True)
    if r.returncode != 0:
        return None
    return r.stdout.decode("utf-8")


def iter_monolith_entries(text: str, name: str) -> Iterator[DocEntry]:
    """Entries of one monolith given its text and its file name."""
    if name in (ISSUES_MONO, ISSUES_CLOSED_MONO):
        yield from parse_issues_monolith(text, name, archived=(name == ISSUES_CLOSED_MONO))[1]
    elif name == DECISIONS_MONO:
        yield from parse_decisions_monolith(text, name)[1]
    elif name == QUESTIONS_MONO:
        yield from parse_questions_monolith(text, name, "question")[1]
    elif name == DEFERRED_MONO:
        yield from parse_questions_monolith(text, name, "deferred")[1]


def repo_root(start: Path | None = None) -> Path:
    r = subprocess.run(["git", "-C", str(start or Path.cwd()), "rev-parse", "--show-toplevel"],
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit("not inside a git checkout")
    return Path(r.stdout.strip())


def git_common_dir(root: Path) -> Path:
    r = subprocess.run(["git", "-C", str(root), "rev-parse", "--git-common-dir"], capture_output=True, text=True)
    p = Path(r.stdout.strip())
    return p if p.is_absolute() else (root / p).resolve()


def git_dir(root: Path) -> Path:
    r = subprocess.run(["git", "-C", str(root), "rev-parse", "--git-dir"], capture_output=True, text=True)
    p = Path(r.stdout.strip())
    return p if p.is_absolute() else (root / p).resolve()


if __name__ == "__main__":  # a quick census of a checkout, for humans
    import sys
    from collections import Counter

    root = repo_root(Path(sys.argv[1]) if len(sys.argv) > 1 else None)
    ents = read_tree(root)
    print(f"{root} — layout {layout_of(root)}")
    by = Counter((e.kind, e.status) for e in ents)
    for (k, s), n in sorted(by.items()):
        print(f"  {k:9s} {s:12s} {n:6d}")
    lanes = Counter((e.kind, e.lane or "-") for e in ents if e.kind in ("issue", "decision", "question", "deferred"))
    print("  by lane:", ", ".join(f"{k}/{ln}={n}" for (k, ln), n in sorted(lanes.items())))
    sys.exit(0)
