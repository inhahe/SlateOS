#!/usr/bin/env python3
"""The per-entry document layout: the conversion into it, and its verification.

`convert()` is a pure function from the five monolithic entry documents (plus
`roadmap.md`) to the files of the per-entry layout. Two tools use it:

* `docs-migrate.py` applies it once to the trunk (the cutover), and
* `docs-carry-forward.py` applies it to a lane branch's *old* copies when that
  lane merges the cutover, so the lane's in-flight edits land in the same files
  the trunk's copies did. That only works because the conversion is
  deterministic: the same entry text always becomes the same file with the same
  bytes. Nothing here may depend on the date, the machine, or dictionary order.

Placement rules (each one a decision recorded in design-decisions/1500-*.md):

* An issue lives in `known-issues/` while open and in `known-issues-resolved/`
  once closed. Status is read by `doc_entries.issue_status` -- conservatively: any
  explicit OPEN or hedge keeps an entry open. An archived entry that says OPEN
  is placed with the open ones and listed in the report.
* File names come from the entry: an issue's id (`TD-B-FOO.md`), or for a prose
  heading `<lane>-<slug>.md`; a decision's `NNNN-<slug>.md`; a question's id. Ids
  shared by several entries (follow-ups) get `-2`, `-3` in file order. Names are
  unique case-insensitively across both issue directories, because Windows
  checkouts are case-insensitive and an issue moves between the two.
* Entry text is carried over byte for byte. Only the separators *between*
  entries (blank lines and the `---` rule) are dropped, and every dropped line is
  accounted for by the line-multiset check in `verify()`.
* `[x]` items leave `roadmap.md` for `roadmap-done.md`, under the same headings.
"""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass, field

import doc_entries as D

# --- the documents the layout replaces them with -----------------------------------

SIGNPOST = """{mark}
# {title} — moved

This file used to hold every entry in one document. Since {date} each entry is
its own file:

{where}

Find an entry by id, by section number or by meaning:

    python scripts/docsearch.py {example}

Citations of the form `{cite}` throughout the tree still identify the entry: the
id (or number) is the file name. The old single file is in git history at the
commit before the cutover (`git log --diff-filter=D -1 -- {name}` finds it).
"""

SIGNPOSTS = {
    D.ISSUES_MONO: dict(
        title="Known issues",
        where="* `known-issues/<ID>.md` -- open issues\n* `known-issues-resolved/<ID>.md` -- closed ones\n\n"
              "How to write one: `known-issues/README.md`.",
        example="TD-B-SOME-ID", cite="known-issues.md TD-B-SOME-ID"),
    D.ISSUES_CLOSED_MONO: dict(
        title="Known issues, resolved archive",
        where="* `known-issues-resolved/<ID>.md` -- closed issues\n* `known-issues/<ID>.md` -- open ones",
        example="TD-B-SOME-ID --status closed", cite="known-issues-resolved.md TD-B-SOME-ID"),
    D.DECISIONS_MONO: dict(
        title="Design decisions",
        where="* `design-decisions/NNNN-<slug>.md` -- one decision per file, by section number\n"
              "* `design-decisions/README.md` -- the format, and the numbering-band table",
        example="538 --kind decision", cite="design-decisions.md §538"),
    D.QUESTIONS_MONO: dict(
        title="Open questions",
        where="* `open-questions/<ID>.md` -- the operator's decision queue, one question per file\n"
              "* `open-questions-resolved/<lane>.md` -- the one-line records of answered questions\n"
              "* `open-questions/README.md` -- how to write a question the operator can decide",
        example="A-Q14", cite="open-questions.md A-Q14"),
    D.DEFERRED_MONO: dict(
        title="Deferred questions",
        where="* `deferred-questions/<ID>.md` -- one deferred question per file\n"
              "* `deferred-questions/README.md` -- the rules",
        example="DQ4", cite="deferred-questions.md DQ4"),
}

ROADMAP_DONE_HEADER = """# Roadmap — completed items

Items move here from `roadmap.md` when they are marked `[x]`, under the same
headings they had there, so `roadmap.md` stays the list of what is left. A
prerequisite that is no longer in `roadmap.md` is done: look for it here, or
`python scripts/docsearch.py "<task>" --kind roadmap --status done`.

"""


def signpost(name: str, date: str) -> str:
    s = SIGNPOSTS[name]
    return SIGNPOST.format(mark=D.SIGNPOST_MARK, date=date, name=name, **s)


# --- conversion ----------------------------------------------------------------------------

@dataclass
class Conversion:
    files: dict[str, str] = field(default_factory=dict)  # path -> content (entries, indexes, roadmap)
    removed: list[str] = field(default_factory=list)  # paths replaced by signposts
    placed: dict[str, tuple[str, str]] = field(default_factory=dict)  # path -> (kind, key) of the entry in it
    notes: list[str] = field(default_factory=list)  # things a human should look at
    counts: Counter = field(default_factory=Counter)
    # for verify(): original text per document, and every line set aside on the way
    originals: dict[str, str] = field(default_factory=dict)
    set_aside: dict[str, list[str]] = field(default_factory=dict)


def _entry_content(text: str) -> str:
    return text if text.endswith("\n") else text + "\n"


class _Names:
    """Unique, case-insensitive file names within a namespace."""

    def __init__(self) -> None:
        self.used: set[str] = set()

    def take(self, base: str) -> str:
        name, n = base, 1
        while name.lower() in self.used:
            n += 1
            name = f"{base}-{n}"
        self.used.add(name.lower())
        return name


def convert(texts: dict[str, str], roadmap: str | None = None) -> Conversion:
    """Convert the monoliths in `texts` (name -> text; any subset of D.MONOLITHS)."""
    conv = Conversion()
    # --- issues: open file first, so an id shared across the two files keeps its
    # bare name for the copy in the open file.
    names = _Names()
    for name in (D.ISSUES_MONO, D.ISSUES_CLOSED_MONO):
        if name not in texts:
            continue
        text = texts[name]
        conv.originals[name] = text
        archived = name == D.ISSUES_CLOSED_MONO
        preamble, entries, dropped = D.parse_issues_monolith(text, name, archived=archived)
        conv.set_aside[name] = preamble + dropped
        for e in entries:
            status = D.issue_status(e.title, e.text.splitlines(keepends=True), archived=False)
            if archived and status == "open":
                # An archived entry that says OPEN is placed with the open ones:
                # an explicit OPEN keeps an entry open, by the archive's own rule.
                explicit = D.ki_split.OPEN_MARK.search(D.issue_status_text(e.title, e.text.splitlines(True)))
                if not explicit:
                    status = "closed"  # archived without a marker: the archiving was the statement
                else:
                    conv.notes.append(f"{name}:{e.line} was archived but says OPEN; placed with the open issues: "
                                      f"{e.title[:90]}")
            folder = D.ISSUES_CLOSED_DIR if status == "closed" else D.ISSUES_OPEN_DIR
            stem = names.take(e.key)
            if stem != e.key:
                conv.notes.append(f"{name}:{e.line} shares its id with an earlier entry; filed as {stem}.md")
            path = f"{folder}/{stem}.md"
            conv.files[path] = _entry_content(e.text)
            conv.placed[path] = ("issue", e.key)
            conv.counts[f"issue:{folder}"] += 1
        conv.removed.append(name)
    if D.ISSUES_MONO in texts or D.ISSUES_CLOSED_MONO in texts:
        conv.files[f"{D.ISSUES_OPEN_DIR}/README.md"] = issues_readme("", resolved=False)
        conv.files[f"{D.ISSUES_CLOSED_DIR}/README.md"] = issues_readme("", resolved=True)

    # --- decisions
    if D.DECISIONS_MONO in texts:
        text = texts[D.DECISIONS_MONO]
        conv.originals[D.DECISIONS_MONO] = text
        preamble, entries, dropped = D.parse_decisions_monolith(text, D.DECISIONS_MONO)
        conv.set_aside[D.DECISIONS_MONO] = preamble + dropped
        dnames = _Names()
        for e in entries:
            slug = (e.extra.get("slug") or "decision")[:80].rstrip("-.")
            stem = dnames.take(f"{e.number:04d}-{slug}")
            path = f"{D.DECISIONS_DIR}/{stem}.md"
            conv.files[path] = _entry_content(e.text)
            conv.placed[path] = ("decision", e.key)
            conv.counts["decision"] += 1
        conv.files[f"{D.DECISIONS_DIR}/README.md"] = decisions_readme("".join(preamble))
        conv.removed.append(D.DECISIONS_MONO)

    # --- open and deferred questions
    for name, kind, folder in ((D.QUESTIONS_MONO, "question", D.QUESTIONS_DIR),
                               (D.DEFERRED_MONO, "deferred", D.DEFERRED_DIR)):
        if name not in texts:
            continue
        text = texts[name]
        conv.originals[name] = text
        preamble, entries, dropped, index = D.parse_questions_monolith(text, name, kind)
        conv.set_aside[name] = preamble + dropped
        qnames = _Names()
        for e in entries:
            stem = qnames.take(e.key)
            path = f"{folder}/{stem}.md"
            conv.files[path] = _entry_content(e.text)
            conv.placed[path] = (kind, e.key)
            conv.counts[kind] += 1
        if index:
            for path, content in _split_resolved_index(index).items():
                conv.files[path] = content
            conv.set_aside[name] = conv.set_aside[name] + index  # verified against the index files below
        conv.files[f"{folder}/README.md"] = questions_readme(kind, "".join(preamble))
        conv.removed.append(name)

    if roadmap is not None:
        new_roadmap, done, moved = archive_roadmap(roadmap)
        conv.files["roadmap.md"] = new_roadmap
        conv.files["roadmap-done.md"] = done
        conv.counts["roadmap:moved"] = moved
        conv.originals["roadmap.md"] = roadmap
    return conv


def _split_resolved_index(lines: list[str]) -> dict[str, str]:
    """`# Resolved` + `## Resolved — lane X` sections -> one file per section."""
    heads = [(i, t) for i, lvl, t in D.fence_aware_headings(lines) if lvl == 2]
    out: dict[str, str] = {}
    intro_end = heads[0][0] if heads else len(lines)
    out[f"{D.QUESTIONS_RESOLVED_DIR}/README.md"] = "".join(lines[:intro_end])
    for n, (i, title) in enumerate(heads):
        end = heads[n + 1][0] if n + 1 < len(heads) else len(lines)
        m = re.search(r"lane\s+([A-F])\b", title, re.IGNORECASE)
        stem = f"lane-{m.group(1).lower()}" if m else D.slugify(re.sub(r"(?i)^resolved\s*[—-]\s*", "", title))[:60]
        path = f"{D.QUESTIONS_RESOLVED_DIR}/{stem}.md"
        out[path] = out.get(path, "") + "".join(lines[i:end])
    return out


# --- roadmap -------------------------------------------------------------------------------------

def archive_roadmap(text: str, done_text: str | None = None) -> tuple[str, str, int]:
    """Move every *fully done* item -- an `[x]` item whose nested items are all `[x]`
    too -- to roadmap-done.md, under the same chain of headings. Returns
    (roadmap, roadmap-done, items moved).

    "Fully" is load-bearing. On 2026-10-02, 2,253 of the 2,269 open `[ ]` items sat
    under parents marked `[x]` -- "Personality binaries batch 311" checked, its
    eight programs not, because the programs were deleted and their lines reset
    while the batch line was not. Moving by the parent's mark would have filed
    nearly all remaining work under "completed". A mixed block stays whole, done
    sub-items included, so a task never loses its context."""
    lines = text.splitlines(keepends=True)
    heads = {i: (lvl, t) for i, lvl, t in D.fence_aware_headings(lines)}
    fenced = _fenced_lines(lines)
    keep: list[str] = []
    stack: list[tuple[int, str]] = []  # (level, heading line) chain above the current line
    sections: dict[tuple[str, ...], list[str]] = {}
    order: list[tuple[str, ...]] = []
    moved = 0
    i = 0
    while i < len(lines):
        if i in heads:
            lvl = heads[i][0]
            stack = [s for s in stack if s[0] < lvl] + [(lvl, lines[i])]
            keep.append(lines[i])
            i += 1
            continue
        m = D.ROADMAP_ITEM_RE.match(lines[i]) if i not in fenced else None
        if not m:
            keep.append(lines[i])
            i += 1
            continue
        indent = len(m.group(1))
        j = i + 1
        while j < len(lines) and j not in heads and lines[j].strip() and \
                (len(lines[j]) - len(lines[j].lstrip())) > indent:
            j += 1
        block = lines[i:j]
        marks = [mm.group(2) for k, ln in enumerate(block)
                 if (i + k) not in fenced and (mm := D.ROADMAP_ITEM_RE.match(ln))]
        if all(mk in "xX" for mk in marks):
            key = tuple(h for _l, h in stack)
            if key not in sections:
                sections[key] = []
                order.append(key)
            sections[key].extend(block)
            moved += 1
        else:
            keep.extend(block)
        i = j
    if done_text is None:
        parts = [ROADMAP_DONE_HEADER.rstrip("\n") + "\n"]
        for key in order:
            parts.append("\n")
            parts.extend(key)
            parts.append("\n")
            parts.extend(sections[key])
        return "".join(keep), "".join(parts), moved
    return "".join(keep), _merge_done(done_text, order, sections), moved


def _merge_done(done_text: str, order: list[tuple[str, ...]], sections: dict[tuple[str, ...], list[str]]) -> str:
    """File newly done items into an existing roadmap-done.md: under their heading
    chain where it already exists (at the end of that section), else as a new
    section at the end. Used by `check-docs.py --fix-roadmap` after the cutover."""
    lines = done_text.splitlines(keepends=True)
    heads = D.fence_aware_headings(lines)
    # end-of-section index for every heading chain present in the file
    chain: list[tuple[int, str]] = []
    ends: dict[tuple[str, ...], int] = {}
    starts: list[tuple[int, tuple[str, ...]]] = []
    for i, lvl, _t in heads:
        chain = [c for c in chain if c[0] < lvl] + [(lvl, lines[i])]
        starts.append((i, tuple(h for _l, h in chain)))
    for n, (i, key) in enumerate(starts):
        nxt = next((s for s, k in starts[n + 1:] if len(k) <= len(key)), len(lines))
        ends[key] = nxt
    inserts: dict[int, list[str]] = {}
    tail: list[str] = []
    for key in order:
        if key in ends:
            at = ends[key]
            while at > 0 and not lines[at - 1].strip():
                at -= 1  # before the blank lines that end the section
            inserts.setdefault(at, []).extend(sections[key])
        else:
            tail += ["\n", *key, "\n", *sections[key]]
    out: list[str] = []
    for i, ln in enumerate(lines + [""]):
        if i in inserts:
            out.extend(inserts[i])
        if i < len(lines):
            out.append(ln)
    if out and not out[-1].endswith("\n"):
        out[-1] += "\n"
    return "".join(out + tail)


def _fenced_lines(lines: list[str]) -> set[int]:
    out: set[int] = set()
    fence = None
    for i, raw in enumerate(lines):
        m = D.ki_split.FENCE.match(raw)
        if m:
            tok = m.group("t")
            if fence is None:
                fence = tok[0] * 3
            elif tok[0] * 3 == fence and len(tok) >= 3:
                fence = None
            out.add(i)
            continue
        if fence is not None:
            out.add(i)
    return out


# --- verification -------------------------------------------------------------------------------

def verify(conv: Conversion) -> list[str]:
    """Per-element checks; returns problems (empty when the conversion is faithful).

    The checks are per element on purpose (roadmap.md rule 3, the 2026-08-16
    archive cut): a conservation total can hold while every entry is misplaced."""
    problems: list[str] = []

    def lines(text: str) -> Counter:
        return Counter(text.splitlines(keepends=True))

    # 1. Every line of every original document is accounted for exactly once:
    #    in an entry file, or set aside as structure (preamble, separators, the
    #    resolved index). Documents whose entries share directories are pooled.
    groups = {
        "issues": ((D.ISSUES_MONO, D.ISSUES_CLOSED_MONO), ("issue",)),
        "decisions": ((D.DECISIONS_MONO,), ("decision",)),
        "questions": ((D.QUESTIONS_MONO,), ("question",)),
        "deferred": ((D.DEFERRED_MONO,), ("deferred",)),
    }
    for label, (names, kinds) in groups.items():
        present = [n for n in names if n in conv.originals]
        if not present:
            continue
        orig, got = Counter(), Counter()
        for n in present:
            orig += lines(conv.originals[n])
            got += Counter(conv.set_aside.get(n, []))
        for path, (kind, _key) in conv.placed.items():
            if kind in kinds:
                got += lines(conv.files[path])
        if got != orig:
            missing, extra = orig - got, got - orig
            sample = next(iter(missing or extra), "")
            problems.append(f"{label}: line multiset differs (missing {sum(missing.values())}, extra "
                            f"{sum(extra.values())}; e.g. {sample[:80]!r})")
    # The resolved-question index is set aside above; its files must hold exactly it.
    if D.QUESTIONS_MONO in conv.originals:
        _pre, _ents, _drop, index = D.parse_questions_monolith(conv.originals[D.QUESTIONS_MONO],
                                                               D.QUESTIONS_MONO, "question")
        got = Counter()
        for path, content in conv.files.items():
            if path.startswith(D.QUESTIONS_RESOLVED_DIR + "/"):
                got += lines(content)
        if got != Counter(index):
            problems.append("open-questions-resolved/: does not hold exactly the old `# Resolved` index")
    # 2. each entry file parses back as exactly one entry of its kind, with its key
    lower: Counter = Counter(p.lower() for p in conv.files)
    for p, n in lower.items():
        if n > 1:
            problems.append(f"two files differ only in case: {p}")
    for path, (kind, key) in conv.placed.items():
        content = conv.files[path]
        heads = D.fence_aware_headings(content.splitlines(keepends=True))
        if not heads or heads[0][0] != 0:
            problems.append(f"{path}: does not start with its heading")
            continue
        if kind == "issue":
            starts = [h for h in heads if D.is_issue_entry_heading(h[1], h[2])]
            if len(starts) != 1:
                problems.append(f"{path}: holds {len(starts)} issue headings, not 1")
            status = D.issue_status(heads[0][2], content.splitlines(True), archived=False)
            in_closed = path.startswith(D.ISSUES_CLOSED_DIR + "/")
            if in_closed and status == "open" and D.ki_split.OPEN_MARK.search(
                    D.issue_status_text(heads[0][2], content.splitlines(True))):
                problems.append(f"{path}: says OPEN but is filed as resolved")
            if not in_closed and status == "closed":
                problems.append(f"{path}: reads as closed but is filed as open")
        elif kind == "decision":
            m = D.DECISION_HEADING_RE.match(content.splitlines()[0])
            if not m or f"{int(m.group(2)):04d}" != path.split("/")[1][:4]:
                problems.append(f"{path}: heading number does not match the file name")
    return problems


# --- READMEs: the rules each directory carries, rewritten for one file per entry -------------

def decisions_readme(old_header: str) -> str:
    """The decision log's header, with its band table carried over verbatim
    (it is machine-read) and the ordering rules replaced by the file rules."""
    bands = [ln for ln in old_header.splitlines() if D.BAND_ROW_RE.match(ln) or ln.startswith("| Band")
             or ln.startswith("|---")]
    fmt_end = old_header.find("## Numbering and file order")
    fmt = old_header[:fmt_end].rstrip() if fmt_end > 0 else old_header.rstrip()
    fmt = fmt.replace("This file records", "This directory records").replace("this file is a\nrunning", "it is a\nrunning")
    return f"""{fmt}

## One file per decision

Each decision is its own file, `design-decisions/NNNN-<slug>.md`, named by its
section number (zero-padded to four digits) and a slug of its title. The first
line is the decision's heading, exactly as it was: `## 538. Lanes publish to ...`
or `## §482 — A convention ...`. Citations elsewhere in the tree (`design-decisions.md
§538`) keep working: the number is the start of the file name.

**Taking a number.** Take the next unused number in your lane's *open* band (the
table below) -- one above the highest number already in that band, never a gap
below it, because a gap may be a number that was spent and withdrawn, and
reissuing it makes an old citation resolve to the wrong entry:

    python scripts/check-docs.py --next-decision <your lane letter>

prints the number and the file name to create. Write a `**Lane:** X` field near
the heading and a `**Decided by:**` field (see the format above).

**What changed, and why** (2026-10-02, section 1500): with one file per decision
two lanes can no longer write the same lines, so the old rule that each band must
ascend in *file order* -- the rule that kept merges conflict-free in the single
file -- is gone. The bands remain because two lanes must still never take the same
number. `scripts/check-docs.py` enforces the numbering; it replaced
`check-design-decisions-bands.py`.

## Numbering bands

**This table is machine-read** by `scripts/check-docs.py`, which parses each
`§lo–§hi` row for its owner and for the word `open` or `closed`. Keep the shape.

{chr(10).join(bands)}
| §1500–§1599 | operator sessions (not a lane) | **open** | sessions the operator starts outside the six lanes |

Bands below the open ones are closed but **not free**: every number in them is
spent, and spent numbers are never reissued. §217–§220 are lane C's although they
sit in lane A's first band (lane C's choice, 2026-08-17: eight things cite them).
§268–§276 and §626 each name two different decisions; the duplicates are recorded,
never renumbered, and no new duplicate is allowed.
"""


def questions_readme(kind: str, old_header: str) -> str:
    """The queue's header, kept almost verbatim: its rules for writing a question the
    operator can decide do not depend on the layout. Only the file mechanics change."""
    if kind == "question":
        mech = """## One file per question

Each open question is its own file, `open-questions/<ID>.md` (`A-Q14.md`), whose
first line is its heading: `## A-Q14 — [A] <the question> — Status: OPEN (raised
<date>)`. **This directory holds open questions only**: `ls open-questions/` is the
queue. Take your lane's next number (`python scripts/check-docs.py --next-question
<lane>`).

**When the operator answers one**, record the decision in `design-decisions/` as a
`Decided by: Operator` entry, `git rm` the question's file, and add a one-line
record to `open-questions-resolved/lane-<x>.md` (your own lane's file, so lanes
never edit the same lines). `scripts/check-docs.py` refuses a file here whose
heading is not an OPEN question with the id its name claims.
"""
    else:
        mech = """## One file per deferred question

Each deferred question is its own file, `deferred-questions/<ID>.md` (`DQ4.md`),
whose first line is its heading. Every entry carries a `Trigger:` -- the condition
for promoting it back to `open-questions/`. When it is promoted, answered or
dropped, `git rm` its file here (promotion: create it in `open-questions/`), and say
so in the commit message; the history keeps the text.
"""
    header = old_header.replace("This file is distinct from", "This directory is distinct from")
    return header.rstrip() + "\n\n" + mech


def issues_readme(old_header: str, resolved: bool) -> str:
    """The rules for writing an issue, carried over from the old header and rewritten
    for files; the archive's README points back to them."""
    if resolved:
        return """# Known issues — resolved

Closed issues, one file per issue, named by the issue's id. An issue moves here
from `known-issues/` (`git mv known-issues/<ID>.md known-issues-resolved/`) once
its `**Status:**` line says FIXED, RESOLVED or CLOSED and the fix has survived a
full boot test on `main`. Nothing is ever deleted: each file keeps the entry's
full text, follow-ups and commit hashes. A reopened issue moves back.

How to write an issue, and the status rules: `known-issues/README.md`.
`scripts/check-docs.py` refuses a file here that says OPEN.
"""
    return """# Known issues

Bugs and technical debt that are **still wrong**, one file per issue:
`known-issues/<ID>.md`. Closed ones live in `known-issues-resolved/`. Until
2026-10-02 both were single files (`known-issues.md`, `known-issues-resolved.md`);
citations of the form `known-issues.md TD-B-FOO` still name the entry, because the
id is the file name.

## Writing an issue

* **Name the file by the entry's id**, and start it with the heading: `## TD-B-FOO-BAR
  (lane B, 2026-10-02) — OPEN` or `### [E] A prose title -- 2026-10-02`. Put your lane
  letter in the heading. An id is upper-case and hyphen-joined (`TD-<lane>-...`);
  a prose title is fine too, filed as `<lane>-<slug>.md`.
* **Put a `**Status:**` line immediately under the heading**: `OPEN`, `FIXED <date>`,
  `RESOLVED <date>`, `CLOSED <date>`, and keep it current. **Write the stamp in
  capitals**: on 2026-09-13 five entries carried a lowercase `fixed`, a triage grep
  for `FIXED|RESOLVED|CLOSED` counted them open, and one was picked up as the next
  task three weeks after it was finished. `scripts/check-docs.py` requires the line
  in every new entry.
* **Any lane may update any entry's status line** without a request: an issue you
  fixed but cannot mark stays open forever in the one place whose job is knowing what
  is open. Everything else about another lane's entry still needs a request
  (`roadmap.md` rule 3).
* **When it is fixed** and the fix has survived a boot test on `main`, move the file
  to `known-issues-resolved/` (`git mv`). Nothing is ever deleted.

## Finding one

    python scripts/docsearch.py "the thing you are looking for" --kind issue --status open
    ls known-issues/TD-B-*            # by id prefix
    grep -rl "some phrase" known-issues known-issues-resolved

The old fence-aware tooling (`ki_split.py`, `ki_archive.py`, `ki_dupes.py`) existed
because one 180,000-line file could not be split, archived or de-duplicated safely by
line; a file per issue needs none of it, and a move is a `git mv`.
"""
