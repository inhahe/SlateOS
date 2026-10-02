#!/usr/bin/env python3
"""Gate: the shared entry documents stay well-formed, findable and honest about status.

Replaces four gates that existed because the entry documents were single files
(`check-known-issues-index.py`, `check-open-questions.py`,
`check-design-decisions-bands.py`, `ki_dupes.py`). Since the 2026-10-02 cutover
each issue, decision and question is its own file (`docs_layout.py`), so the
failure modes those gates chased -- an entry torn in half by a fence, a question
filed below `# Resolved`, two lanes inserting at one line offset, an archived
entry resurrected by a merge -- cannot happen. What they protected still matters,
and each rule below carries one of those invariants forward or enforces the
done/open split the documents always claimed to have:

  S1  no old-format document holds entries (a stray `known-issues.md` written
      after the cutover, or an unfinished carry-forward)              -- error
  S2  every file in an entry directory starts with its heading         -- error
  S3  no two issue files differ only in case, across both directories  -- error
  S4  no merge-conflict markers in an entry file: `docs-carry-forward.py` leaves
      them in files git does not know are conflicted, so git would let them be
      committed                                                         -- error
  I1  a new issue has a `**Status:**` line under its heading
  I2  a status marker is upper-case (`FIXED`, not `fixed`): a lower-case one was
      counted open by triage greps and worked three weeks after it was done
  I3  an issue lives where its status says: closed ones in
      `known-issues-resolved/`, and nothing there says OPEN
  D1  a decision file's number is its heading's number
  D2  no new duplicate decision number (the recorded ones are grandfathered)
  D3  a new decision's number is in an open band of its own lane, above every
      number already in that band, and the entry says `**Lane:**`
  Q1  every file in `open-questions/` is an OPEN question whose id is its name
  Q2  no open question reuses an id the resolved index already records
  F1  a new deferred question carries a `Trigger:`
  T1  no new done-marked paragraph in `todo.txt` (done items are deleted)
  R1  no fully done `[x]` item stays in `roadmap.md` (it moves to
      `roadmap-done.md`; `--fix-roadmap` does it)
  R2  an `[x]` item containing open sub-items is reported, one warning per
      owning lane; `--list-mixed <lane>` lists them                   -- warning

**Whose problem it is.** Rules I*, D*, Q*, F*, T1 and R1 are errors when the
entry belongs to the lane running the gate (its worktree, via `which-lane.py`)
and warnings otherwise: a build must not refuse over a sentence only another lane
may edit (design-decisions §903). `--lane X` overrides, `--strict` makes every
finding an error (used to verify the cutover itself). "New" means not in
`scripts/docs-baseline.json`, which grandfathers what the cutover inherited and
should only ever shrink.

Exit 0: clean (warnings allowed). 1: violations. 2: no verdict -- the documents
could not be read, or the parse came back implausibly empty, which a gate must
never report as a pass.

    python scripts/check-docs.py                     # check this checkout
    python scripts/check-docs.py --head <rev>        # check a commit (the push hook)
    python scripts/check-docs.py --self-test         # every rule against fixtures
    python scripts/check-docs.py --next-decision B   # your next section number
    python scripts/check-docs.py --next-question B   # your next question id
    python scripts/check-docs.py --fix-roadmap       # move done items to roadmap-done.md
    python scripts/check-docs.py --list-mixed B      # your [x] items that still hold open sub-items
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import subprocess
import sys
import tarfile
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))
import doc_entries as D  # noqa: E402
import docs_layout as L  # noqa: E402

BASELINE = SCRIPTS / "docs-baseline.json"
_CONFLICT = re.compile(r"(?m)^(<{7}|>{7})( |$)")
MIN_ISSUES = 50  # a real tree has thousands; fewer means the parse failed
_MARKER_WORD = re.compile(r"\b(fixed|resolved|closed|done|open)\b", re.IGNORECASE)
_TRIGGER = re.compile(r"(?im)^\s*[-*]?\s*\**Trigger\b")


@dataclass
class Finding:
    rule: str
    path: str
    message: str
    lane: str = ""
    error: bool = True

    def render(self) -> str:
        sev = "ERROR" if self.error else "warning"
        who = f" [lane {self.lane}]" if self.lane else ""
        return f"{sev} {self.rule} {self.path}{who}: {self.message}"


@dataclass
class Result:
    findings: list[Finding] = field(default_factory=list)
    counts: dict = field(default_factory=dict)
    no_verdict: str = ""


def _hash(text: str) -> str:
    return hashlib.sha1(" ".join(text.split()).encode("utf-8")).hexdigest()[:16]


def _entry_key(name: str) -> str:
    """An issue's grandfathering key: its file name without `.md`, lower-cased.

    Not `Path(name).stem` on a key that is already a stem: ids contain dots
    (`B-CP.RS`, `...-history.jsonl-recorded-...`), and a second `.stem` strips
    them as if they were extensions -- 13 grandfathered entries failed that way
    the moment the baseline was written."""
    base = name.rsplit("/", 1)[-1]
    return (base[:-3] if base.endswith(".md") else base).lower()


def load_baseline(path: Path = BASELINE) -> dict:
    """The baseline at `path`, unioned with every `docs-baseline-carried-<lane>.json`
    beside it: what each lane wrote before the cutover and carried across in its
    merge (docs-carry-forward.py), grandfathered exactly as the trunk's was."""
    if not path.is_file():
        return {}
    out = json.loads(path.read_text(encoding="utf-8"))
    for extra in sorted(path.parent.glob("docs-baseline-carried-*.json")):
        for k, v in json.loads(extra.read_text(encoding="utf-8")).items():
            if isinstance(v, list):
                out[k] = sorted(set(out.get(k, [])) | set(v), key=str)
    return out


def status_marker_tokens(title: str, lines: list[str]) -> list[str]:
    """The first marker word of each status segment (heading tail, Status line, bold
    opening line) -- the token a triage grep keys on, which must be upper-case."""
    out = []
    for seg in re.split(r"\s+—\s+", D.issue_status_text(title, lines)):
        s = re.sub(r"^\W+", "", seg.replace("**Status:**", "").replace("**Status**", "").strip())
        m = _MARKER_WORD.match(s)
        if m:
            out.append(m.group(1))
    return out


def fully_done_blocks(text: str) -> list[tuple[int, str, str]]:
    """(line, lane, first line) of every top-level fully done item in a roadmap text."""
    _keep, done, _n = L.archive_roadmap(text, done_text="")
    out = []
    for ln in done.splitlines():
        m = D.ROADMAP_ITEM_RE.match(ln)
        if m and not m.group(1):
            tag = re.match(r"^`?\[([A-F])\]`?", m.group(3))
            out.append((0, tag.group(1) if tag else "", ln.strip()))
    return out


_LANE_TAG = re.compile(r"^`?\[([A-F])\]`?")


def mixed_blocks(text: str) -> list[tuple[int, str, list[tuple[str, str]]]]:
    """(line, first line, [(lane, open sub-item)]) of every top-level `[x]` item that
    still contains open sub-items.

    The marks contradict each other: the sub-items are done and were never ticked,
    or the `[x]` no longer holds (e.g. a lane filed new work under a parent
    finished long before). Whoever owns the open sub-item is the one who can say
    which, so each is attributed by its own `[X]` tag, falling back to the
    parent's ('' when neither has one). Measured on 2026-10-02: 294 such items, of
    which 4 hold a lane-tagged open sub-item and 290 have no tag on either level."""
    out = []
    lines = text.splitlines()
    for i, ln in enumerate(lines):
        m = D.ROADMAP_ITEM_RE.match(ln)
        if not m or m.group(1) or m.group(2) not in "xX":
            continue
        parent = _LANE_TAG.match(m.group(3))
        open_subs = []
        j = i + 1
        while j < len(lines) and lines[j].strip() and (len(lines[j]) - len(lines[j].lstrip())) > 0:
            sub = D.ROADMAP_ITEM_RE.match(lines[j])
            if sub and sub.group(2) not in "xX":
                tag = _LANE_TAG.match(sub.group(3)) or parent
                open_subs.append((tag.group(1) if tag else "", lines[j].strip()))
            j += 1
        if open_subs:
            out.append((i + 1, ln.strip(), open_subs))
    return out


def _wanted(lane: str, want: str) -> bool:
    return want == "ALL" or lane == want or (want == "-" and not lane)


def list_mixed(root: Path, lane: str) -> str:
    """The R2 items holding open sub-items of `lane` (a letter, `-` for untagged
    ones, or `all`), each with those sub-items, for working through them."""
    want = lane.upper()
    out, n = [], 0
    for line, first, subs in mixed_blocks((root / "roadmap.md").read_text(encoding="utf-8")):
        mine = [(o, s) for o, s in subs if _wanted(o, want)]
        if not mine:
            continue
        n += 1
        out.append(f"roadmap.md:{line}  {first[:150]}")
        out.extend(f"      open [{o or 'untagged'}]: {s[:140]}" for o, s in mine)
    out.append(f"{n} item(s). For each: tick the open sub-items that are done (`--fix-roadmap` then moves a "
               "finished block to roadmap-done.md), or un-tick the parent if it is not finished -- a new task "
               "filed under an old finished parent can instead become its own top-level item.")
    return "\n".join(out)


def todo_done_paragraphs(text: str) -> list[tuple[int, str, str]]:
    """(line, lane, first line) of `todo.txt` paragraphs that open with a done marker."""
    return [(e.line, e.lane, e.title) for e in D.parse_todo(text, "todo.txt") if e.status == "done"]


def check(root: Path, lane: str | None, strict: bool, baseline: dict) -> Result:
    res = Result()

    def add(rule: str, path: str, msg: str, owner: str = "", structural: bool = False) -> None:
        # An unknown running lane (the integration checkout, a scratch worktree,
        # a test repository) is judged strictly: a gate that cannot tell whose
        # push it is must refuse rather than wave every finding through.
        own = strict or structural or lane is None or owner == lane
        res.findings.append(Finding(rule, path, msg, owner, error=own))

    has_dirs = any((root / d).is_dir() for d in D.ENTRY_DIRS)
    has_monos = any((root / m).is_file() for m in D.MONOLITHS)
    if not has_dirs and not has_monos:
        res.no_verdict = (f"no shared entry documents here ({D.DECISIONS_DIR}/README.md and "
                          f"{D.DECISIONS_MONO} do not exist)")
        return res
    layout = D.layout_of(root)
    res.counts["layout"] = layout
    # S1
    for name in D.MONOLITHS:
        p = root / name
        if p.is_file() and not D.is_signpost(p) and layout != "monolithic":
            add("S1", name, "holds entries in the old single-file format; carry them into their own files with "
                "`python scripts/docs-carry-forward.py` (or, after a merge conflict, "
                "`python scripts/docs-carry-forward.py --resolve`)", structural=True)
    if layout == "monolithic":
        res.no_verdict = ("this checkout still has the single-file layout; merge origin/main and run "
                          "`python scripts/docs-carry-forward.py` (the 2026-10-02 cutover)")
        return res

    # Issue grandfathering is keyed by the entry (its file name, lower-cased), not
    # its path: an issue moves between known-issues/ and known-issues-resolved/
    # when its status changes, and must not lose its grandfathering on the way.
    bl_status = {_entry_key(x) for x in baseline.get("issue_without_status", [])}
    bl_lower = {_entry_key(x) for x in baseline.get("lowercase_marker", [])}
    bl_decisions = set(baseline.get("decision_files", []))
    bl_dupes = set(baseline.get("duplicate_numbers", []))
    bl_deferred = set(baseline.get("deferred_without_trigger", []))
    bl_todo = set(baseline.get("todo_done_paragraphs", []))
    bl_roadmap = set(baseline.get("roadmap_done_items", []))

    # --- issues
    names: dict[str, str] = {}
    issues = []
    for d in (D.ISSUES_OPEN_DIR, D.ISSUES_CLOSED_DIR):
        for p in sorted((root / d).glob("*.md")) if (root / d).is_dir() else []:
            if p.name in D.NON_ENTRY_FILES:
                continue
            rel = p.relative_to(root).as_posix()
            if p.name.lower() in names:
                add("S3", rel, f"differs only in case from {names[p.name.lower()]}", structural=True)
            names[p.name.lower()] = rel
            text = p.read_text(encoding="utf-8")
            lines = text.splitlines(keepends=True)
            heads = D.fence_aware_headings(lines) if lines else []
            if not heads or heads[0][0] != 0:
                add("S2", rel, "does not start with its heading", structural=True)
                continue
            title = heads[0][2]
            owner = D.lane_of(title, "".join(lines[1:16]), p.stem) or (
                m.group(1) if (m := re.match(r"^([A-F])-[a-z0-9]", p.stem)) else "")
            issues.append(rel)
            has_status = any(ln.lstrip().startswith("**Status") for ln in lines[1:8])
            if not has_status and _entry_key(p.name) not in bl_status:
                add("I1", rel, "has no `**Status:**` line under its heading (OPEN / FIXED <date> / ...)", owner)
            lower = [t for t in status_marker_tokens(title, lines) if t != t.upper()]
            if lower and _entry_key(p.name) not in bl_lower:
                add("I2", rel, f"status marker {lower[0]!r} must be upper-case ({lower[0].upper()}): a triage "
                    "grep for FIXED|RESOLVED|CLOSED counts it as open", owner)
            status = D.issue_status(title, lines, archived=False)
            if d == D.ISSUES_OPEN_DIR and status == "closed":
                add("I3", rel, "says it is closed; move it with "
                    f"`git mv {rel} {D.ISSUES_CLOSED_DIR}/` once the fix has had a boot test on main", owner)
            if d == D.ISSUES_CLOSED_DIR and D.ki_split.OPEN_MARK.search(D.issue_status_text(title, lines)):
                add("I3", rel, f"says OPEN but is filed as resolved; move it back with "
                    f"`git mv {rel} {D.ISSUES_OPEN_DIR}/`", owner)
    res.counts["issues"] = len(issues)

    # --- S4: conflict markers anywhere in the entry documents
    for d in (*D.ENTRY_DIRS, D.QUESTIONS_RESOLVED_DIR):
        for p in sorted((root / d).glob("*.md")) if (root / d).is_dir() else []:
            if _CONFLICT.search(p.read_text(encoding="utf-8", errors="replace")):
                add("S4", p.relative_to(root).as_posix(), "holds merge-conflict markers; resolve them "
                    "(docs-carry-forward.py lists every file it left that way)", structural=True)
    for name in ("roadmap.md", "roadmap-done.md", "todo.txt"):
        p = root / name
        if p.is_file() and _CONFLICT.search(p.read_text(encoding="utf-8", errors="replace")):
            add("S4", name, "holds merge-conflict markers; resolve them", structural=True)

    # --- decisions
    readme = root / D.DECISIONS_DIR / "README.md"
    if not readme.is_file():
        res.no_verdict = f"{D.DECISIONS_DIR}/README.md does not exist, so the numbering bands cannot be read"
        return res
    bands = D.parse_bands(readme.read_text(encoding="utf-8"))
    if not bands:
        res.no_verdict = f"{D.DECISIONS_DIR}/README.md has no numbering-band table this gate can read"
        return res
    numbers: dict[int, list[str]] = {}
    decisions = []
    for p in sorted((root / D.DECISIONS_DIR).glob("*.md")):
        if p.name in D.NON_ENTRY_FILES:
            continue
        rel = p.relative_to(root).as_posix()
        first = p.read_text(encoding="utf-8").splitlines()[:1]
        m = D.DECISION_HEADING_RE.match(first[0]) if first else None
        if not m:
            add("S2", rel, "does not start with a numbered decision heading (`## 1234. Title` or "
                "`## §1234 — Title`)", structural=True)
            continue
        num = int(m.group(2))
        decisions.append((rel, num))
        numbers.setdefault(num, []).append(rel)
        if not p.name.startswith(f"{num:04d}-"):
            add("D1", rel, f"heading says {num} but the file name says {p.name[:4]}; rename it to "
                f"{num:04d}-<slug>.md", structural=True)
    for num, files in numbers.items():
        if len(files) > 1 and num not in bl_dupes:
            for rel in files:
                if rel not in bl_decisions:
                    add("D2", rel, f"number {num} is already used by {', '.join(f for f in files if f != rel)}; "
                        f"take the next number in your band (`check-docs.py --next-decision <lane>`)",
                        _decision_lane(root, rel, num, bands))
    for rel, num in decisions:
        if rel in bl_decisions:
            continue
        text = (root / rel).read_text(encoding="utf-8")
        field_lane = D.LANE_FIELD_RE.search("\n".join(text.splitlines()[:16]))
        owner = field_lane.group(1) if field_lane else ""
        band = next(((lo, hi, ln, st) for lo, hi, ln, st in bands if lo <= num <= hi), None)
        if not field_lane:
            add("D3", rel, "has no `**Lane:** X` field near its heading", owner or D.band_lane(num, bands))
        if band is None or band[3] != "open":
            add("D3", rel, f"number {num} is not in an open band (see {D.DECISIONS_DIR}/README.md)",
                owner or D.band_lane(num, bands))
        elif band[2] and owner and band[2] != owner:
            add("D3", rel, f"number {num} is in lane {band[2]}'s band, but the entry says lane {owner}", owner)
        elif band is not None:
            higher_old = [n for r, n in decisions if r in bl_decisions and band[0] <= n <= band[1] and n > num]
            if higher_old:
                add("D3", rel, f"number {num} is below {max(higher_old)}, already in its band: a gap below the "
                    "high-water mark may be a withdrawn number, and reissuing it misdirects old citations", owner)
    res.counts["decisions"] = len(decisions)

    # --- open questions
    resolved_ids = set()
    rdir = root / D.QUESTIONS_RESOLVED_DIR
    for p in sorted(rdir.glob("*.md")) if rdir.is_dir() else []:
        resolved_ids.update(re.findall(r"\b((?:[A-F]-)?Q\d+|DQ\d+)\b", p.read_text(encoding="utf-8")))
    nq = 0
    for p in sorted((root / D.QUESTIONS_DIR).glob("*.md")) if (root / D.QUESTIONS_DIR).is_dir() else []:
        if p.name in D.NON_ENTRY_FILES:
            continue
        rel = p.relative_to(root).as_posix()
        nq += 1
        first = (p.read_text(encoding="utf-8").splitlines() or [""])[0]
        m = D.QUESTION_ID_RE.match(first.lstrip("# "))
        qid = m.group(1) if m else ""
        owner = qid.split("-")[0] if re.match(r"^[A-F]-Q", qid) else D.lane_of(first.lstrip("# "), "", p.stem)
        if not first.startswith("## "):
            add("S2", rel, "does not start with a `## <ID> — ...` heading", structural=True)
        elif qid and qid != p.stem:
            add("Q1", rel, f"heading says {qid}; the file must be named {qid}.md", owner)
        elif re.search(r"(?i)\bstatus\b", first) and not re.search(r"\bOPEN\b", first):
            add("Q1", rel, "is not OPEN; an answered question is recorded in design-decisions/ and its file "
                f"removed, with a line in {D.QUESTIONS_RESOLVED_DIR}/lane-<x>.md", owner)
        if qid and qid in resolved_ids and rel not in set(baseline.get("question_reuses_resolved_id", [])):
            add("Q2", rel, f"{qid} is already recorded as answered in {D.QUESTIONS_RESOLVED_DIR}/; an answer "
                "naming it would be ambiguous -- take the next id", owner)
    res.counts["questions"] = nq

    # --- deferred
    for p in sorted((root / D.DEFERRED_DIR).glob("*.md")) if (root / D.DEFERRED_DIR).is_dir() else []:
        if p.name in D.NON_ENTRY_FILES:
            continue
        rel = p.relative_to(root).as_posix()
        text = p.read_text(encoding="utf-8")
        if not _TRIGGER.search(text) and rel not in bl_deferred:
            first = (text.splitlines() or [""])[0]
            add("F1", rel, "carries no `Trigger:` (the condition for promoting it back to open-questions/)",
                D.lane_of(first.lstrip("# "), "\n".join(text.splitlines()[:16]), p.stem))

    # --- todo.txt and the roadmap
    if (root / "todo.txt").is_file():
        for line, owner, title in todo_done_paragraphs((root / "todo.txt").read_text(encoding="utf-8")):
            if _hash(title) not in bl_todo:
                add("T1", f"todo.txt:{line}", f"a done item stays in todo.txt (done items are deleted -- git keeps "
                    f"them): {title[:90]}", owner)
    if (root / "roadmap.md").is_file():
        rm = (root / "roadmap.md").read_text(encoding="utf-8")
        for _l, owner, first in fully_done_blocks(rm):
            if _hash(first) not in bl_roadmap:
                add("R1", "roadmap.md", f"a done item stays in roadmap.md; `python scripts/check-docs.py "
                    f"--fix-roadmap` moves it to roadmap-done.md: {first[:90]}", owner)
        # One warning per owning lane, naming its own count and how to list them: a
        # single total over every lane is a line each lane learns to skip.
        by_lane: dict[str, list[tuple[int, str]]] = {}
        for line, first, subs in mixed_blocks(rm):
            for owner in sorted({o for o, _s in subs}):
                by_lane.setdefault(owner, []).append((line, first))
        for owner, items in sorted(by_lane.items()):
            what = (f"lane {owner}: {len(items)} item(s) marked [x] hold open sub-items of yours" if owner else
                    f"{len(items)} item(s) marked [x] hold open sub-items that no lane tag claims")
            res.findings.append(Finding(
                "R2", f"roadmap.md:{items[0][0]}",
                f"{what} -- they are done, or the [x] no longer holds; `python scripts/check-docs.py --list-mixed "
                f"{owner or '-'}` lists them. First: {items[0][1][:80]}", owner, error=False))
    if res.counts["issues"] < MIN_ISSUES:
        res.no_verdict = f"only {res.counts['issues']} issue files found; a parse that sees this few is broken"
    return res


def _decision_lane(root: Path, rel: str, num: int, bands) -> str:
    text = (root / rel).read_text(encoding="utf-8")
    m = D.LANE_FIELD_RE.search("\n".join(text.splitlines()[:16]))
    return m.group(1) if m else D.band_lane(num, bands)


# --- helpers for writers -----------------------------------------------------------------

def next_decision(root: Path, lane: str) -> str:
    readme = root / D.DECISIONS_DIR / "README.md"
    bands = D.parse_bands(readme.read_text(encoding="utf-8"))
    mine = [(lo, hi) for lo, hi, ln, st in bands if st == "open" and (ln == lane.upper() or
            (lane.lower() == "operator" and not ln))]
    if not mine:
        return f"lane {lane} has no open band in {D.DECISIONS_DIR}/README.md"
    lo, hi = mine[0]
    used = [int(p.name[:4]) for p in (root / D.DECISIONS_DIR).glob("[0-9][0-9][0-9][0-9]-*.md")]
    in_band = [n for n in used if lo <= n <= hi]
    nxt = max(in_band) + 1 if in_band else lo
    if nxt > hi:
        return f"band §{lo}–§{hi} is full; ask for a new band (edit the table in {D.DECISIONS_DIR}/README.md)"
    return (f"next number for lane {lane.upper()}: {nxt}\n"
            f"create {D.DECISIONS_DIR}/{nxt:04d}-<short-slug>.md starting with `## {nxt}. <Title>`\n"
            f"and put `**Lane:** {lane.upper()}` and `**Decided by:** ...` under it")


def next_question(root: Path, lane: str) -> str:
    lane = lane.upper()
    seen = []
    for d in (D.QUESTIONS_DIR, D.QUESTIONS_RESOLVED_DIR, D.DEFERRED_DIR):
        for p in (root / d).glob("*.md") if (root / d).is_dir() else []:
            seen += [int(n) for n in re.findall(rf"\b{lane}-Q(\d+)\b", p.read_text(encoding="utf-8") + p.stem)]
    return f"next question id for lane {lane}: {lane}-Q{max(seen, default=0) + 1}"


def write_baseline(root: Path) -> dict:
    res = check(root, lane=None, strict=True, baseline={})
    bl: dict = {"issue_without_status": [], "lowercase_marker": [], "deferred_without_trigger": [],
                "question_reuses_resolved_id": [], "todo_done_paragraphs": [], "roadmap_done_items": []}
    for f in res.findings:
        if f.rule == "I1":
            bl["issue_without_status"].append(_entry_key(f.path))
        elif f.rule == "I2":
            bl["lowercase_marker"].append(_entry_key(f.path))
        elif f.rule == "F1":
            bl["deferred_without_trigger"].append(f.path)
        elif f.rule == "Q2":
            bl["question_reuses_resolved_id"].append(f.path)
    bl["decision_files"] = sorted(p.relative_to(root).as_posix() for p in (root / D.DECISIONS_DIR).glob("*.md")
                                  if p.name not in D.NON_ENTRY_FILES)
    nums: dict[int, int] = {}
    for p in (root / D.DECISIONS_DIR).glob("[0-9][0-9][0-9][0-9]-*.md"):
        nums[int(p.name[:4])] = nums.get(int(p.name[:4]), 0) + 1
    bl["duplicate_numbers"] = sorted(n for n, c in nums.items() if c > 1)
    if (root / "todo.txt").is_file():
        bl["todo_done_paragraphs"] = sorted({_hash(t) for _l, _o, t in
                                             todo_done_paragraphs((root / "todo.txt").read_text(encoding="utf-8"))})
    if (root / "roadmap.md").is_file():
        bl["roadmap_done_items"] = sorted({_hash(f) for _l, _o, f in
                                           fully_done_blocks((root / "roadmap.md").read_text(encoding="utf-8"))})
    for k in bl:
        if isinstance(bl[k], list):
            bl[k] = sorted(set(bl[k]))
    return bl


def fix_roadmap(root: Path) -> str:
    rm = root / "roadmap.md"
    done = root / "roadmap-done.md"
    new, merged, moved = L.archive_roadmap(rm.read_text(encoding="utf-8"),
                                           done.read_text(encoding="utf-8") if done.exists() else None)
    if not moved:
        return "nothing to move"
    with open(rm, "w", encoding="utf-8", newline="") as fh:
        fh.write(new)
    with open(done, "w", encoding="utf-8", newline="") as fh:
        fh.write(merged)
    return f"moved {moved} done item(s) from roadmap.md to roadmap-done.md"


# --- reading a commit instead of the working tree ------------------------------------------

BASELINE_REL = "scripts/docs-baseline.json"
DOC_PATHS = [*D.ENTRY_DIRS, D.QUESTIONS_RESOLVED_DIR, *D.MONOLITHS, "roadmap.md", "roadmap-done.md", "todo.txt",
             BASELINE_REL]


class NoVerdict(Exception):
    pass


def is_commit(root: Path, rev: str) -> bool:
    r = subprocess.run(["git", "-C", str(root), "rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}"],
                       capture_output=True, text=True)
    return r.returncode == 0


def materialise(root: Path, rev: str, into: Path) -> None:
    """The documents *and the baseline* as they are at `rev`, written under `into`.

    The baseline comes from the same tree as the documents, never from the disk:
    it is the input that forgives, and an uncommitted `--write-baseline` would
    otherwise waive a finding in the very commit being pushed (the defect the
    numbering-band gate shipped with, test-checkers-honour-head.py gate 13)."""
    ls = subprocess.run(["git", "-C", str(root), "ls-tree", "--name-only", rev, "--",
                         *[p for p in DOC_PATHS if "/" not in p], "scripts"],
                        capture_output=True, text=True)
    names = set(ls.stdout.splitlines())
    present = [p for p in DOC_PATHS if p in names or (p == BASELINE_REL and "scripts" in names)]
    if not any(p for p in present if p != BASELINE_REL):
        raise NoVerdict(f"--head {rev}: the shared documents do not exist at that revision")
    out = subprocess.run(["git", "-C", str(root), "archive", "--format=tar", rev, "--",
                          *[p for p in present if p != BASELINE_REL]], capture_output=True)
    if out.returncode != 0:
        raise NoVerdict(f"--head {rev}: git archive failed: {out.stderr.decode(errors='replace')[:300]}")
    with tarfile.open(fileobj=io.BytesIO(out.stdout)) as tf:
        tf.extractall(into, filter="data")
    names = subprocess.run(["git", "-C", str(root), "ls-tree", "--name-only", rev, "--", "scripts/"],
                           capture_output=True, text=True).stdout.split()
    for rel in [n for n in names if n == BASELINE_REL or
                (n.startswith("scripts/docs-baseline-carried-") and n.endswith(".json"))]:
        blob = subprocess.run(["git", "-C", str(root), "show", f"{rev}:{rel}"], capture_output=True)
        if blob.returncode == 0:
            (into / "scripts").mkdir(parents=True, exist_ok=True)
            (into / rel).write_bytes(blob.stdout)


# --- self-test ---------------------------------------------------------------------------------

def _fixture(tmp: Path) -> Path:
    root = tmp / "repo"
    (root / D.ISSUES_OPEN_DIR).mkdir(parents=True)
    (root / D.ISSUES_CLOSED_DIR).mkdir()
    (root / D.DECISIONS_DIR).mkdir()
    (root / D.QUESTIONS_DIR).mkdir()
    (root / D.QUESTIONS_RESOLVED_DIR).mkdir()
    (root / D.DEFERRED_DIR).mkdir()
    for i in range(MIN_ISSUES):
        (root / D.ISSUES_CLOSED_DIR / f"TD-A-OLD-{i}.md").write_text(
            f"## TD-A-OLD-{i} (lane A, 2026-08-01) — FIXED 2026-08-02\n**Status:** FIXED 2026-08-02\n\nbody\n",
            encoding="utf-8")
    (root / D.ISSUES_OPEN_DIR / "TD-B-LIVE.md").write_text(
        "## TD-B-LIVE (lane B, 2026-10-01) — OPEN\n**Status:** OPEN\n\nbody\n", encoding="utf-8")
    (root / D.DECISIONS_DIR / "README.md").write_text(
        "| Band | Owner | Status | Region |\n|---|---|---|---|\n| §1–§99 | history | closed | x |\n"
        "| §100–§199 | **lane A** | **open** | x |\n| §200–§299 | **lane B** | **open** | x |\n", encoding="utf-8")
    (root / D.DECISIONS_DIR / "0050-old.md").write_text("## §50 — Old\n", encoding="utf-8")
    (root / D.DECISIONS_DIR / "0120-a-thing.md").write_text("## 120. A thing\n**Lane:** A\n", encoding="utf-8")
    (root / D.QUESTIONS_DIR / "B-Q2.md").write_text("## B-Q2 — [B] Which? — Status: OPEN (raised 2026-10-01)\n",
                                                    encoding="utf-8")
    (root / D.QUESTIONS_RESOLVED_DIR / "lane-b.md").write_text("## Resolved — lane B\n\n- B-Q1 — answered\n",
                                                               encoding="utf-8")
    (root / D.DEFERRED_DIR / "DQ1.md").write_text("## DQ1 — Later?\n\nTrigger: when X exists.\n", encoding="utf-8")
    (root / "todo.txt").write_text("## Lane B — userland\n## * write the thing\n##\n", encoding="utf-8")
    (root / "roadmap.md").write_text("## Phase 1\n- [ ] `[B]` open task\n  - [x] done step\n", encoding="utf-8")
    return root


def self_test() -> int:
    failures = 0

    def expect(name: str, root: Path, rule: str | None, lane: str | None = "B", error: bool = True) -> None:
        nonlocal failures
        res = check(root, lane=lane, strict=False, baseline=load_baseline(root / "bl.json"))
        hits = [f for f in res.findings if f.rule == rule] if rule else [f for f in res.findings if f.error]
        ok = (bool(hits) and all(f.error == error for f in hits)) if rule else not hits
        if res.no_verdict:
            ok = False
        print(f"  {'ok  ' if ok else 'FAIL'} {name}" + ("" if ok else f" -> {[f.render() for f in res.findings]}"
                                                         + (f" no verdict: {res.no_verdict}" if res.no_verdict else "")))
        failures += 0 if ok else 1

    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        root = _fixture(tmp)
        (root / "bl.json").write_text(json.dumps({"decision_files": ["design-decisions/0050-old.md"]}), encoding="utf-8")
        expect("a clean tree has no errors", root, None)

        def mutate(name, rule, fn, lane="B", error=True):
            r = _fixture(tmp / name)
            (r / "bl.json").write_text(json.dumps({"decision_files": ["design-decisions/0050-old.md"]}), encoding="utf-8")
            fn(r)
            expect(name, r, rule, lane, error)

        mutate("S1 an old-format document with entries", "S1",
               lambda r: (r / "known-issues.md").write_text("# KI\n\n## TD-B-X (lane B) — OPEN\nbody\n", encoding="utf-8"))
        mutate("S2 an issue file without its heading", "S2",
               lambda r: (r / D.ISSUES_OPEN_DIR / "TD-B-NOHEAD.md").write_text("no heading\n", encoding="utf-8"))
        mutate("S3 two issue files differing only in case", "S3",
               lambda r: (r / D.ISSUES_CLOSED_DIR / "td-b-live.md").write_text(
                   "## td-b-live (lane B) — FIXED 2026-10-02\n**Status:** FIXED 2026-10-02\n", encoding="utf-8")
               if not (r / D.ISSUES_CLOSED_DIR / "TD-B-LIVE.md").exists() else None)
        mutate("S4 conflict markers left in an entry", "S4",
               lambda r: (r / D.ISSUES_OPEN_DIR / "TD-B-LIVE.md").write_text(
                   "## TD-B-LIVE (lane B, 2026-10-01) — OPEN\n**Status:** OPEN\n<<<<<<< ours\na\n=======\nb\n"
                   ">>>>>>> theirs\n", encoding="utf-8"))
        mutate("I1 a new issue without a Status line", "I1",
               lambda r: (r / D.ISSUES_OPEN_DIR / "TD-B-NEW.md").write_text("## TD-B-NEW (lane B) — OPEN\nbody\n",
                                                                            encoding="utf-8"))
        mutate("I1 is a warning for another lane's entry", "I1",
               lambda r: (r / D.ISSUES_OPEN_DIR / "TD-C-NEW.md").write_text("## TD-C-NEW (lane C) — OPEN\nbody\n",
                                                                            encoding="utf-8"), error=False)
        mutate("I2 a lower-case status marker", "I2",
               lambda r: (r / D.ISSUES_CLOSED_DIR / "TD-B-LOW.md").write_text(
                   "## TD-B-LOW (lane B, 2026-10-01) -- **fixed 2026-10-02**\n**Status:** fixed\n", encoding="utf-8"))
        mutate("I3 a closed issue left among the open", "I3",
               lambda r: (r / D.ISSUES_OPEN_DIR / "TD-B-DONE.md").write_text(
                   "## TD-B-DONE (lane B) — FIXED 2026-10-02\n**Status:** FIXED 2026-10-02\n", encoding="utf-8"))
        mutate("I3 an OPEN issue filed as resolved", "I3",
               lambda r: (r / D.ISSUES_CLOSED_DIR / "TD-B-REOPENED.md").write_text(
                   "## TD-B-REOPENED (lane B) — OPEN\n**Status:** OPEN\n", encoding="utf-8"))
        mutate("D1 a decision file named for another number", "D1",
               lambda r: (r / D.DECISIONS_DIR / "0201-wrong.md").write_text("## 202. Wrong\n**Lane:** B\n",
                                                                            encoding="utf-8"))
        mutate("D2 a new duplicate decision number", "D2",
               lambda r: (r / D.DECISIONS_DIR / "0120-another.md").write_text("## 120. Another\n**Lane:** A\n",
                                                                              encoding="utf-8"), lane="A")
        mutate("D3 a new decision outside its lane's band", "D3",
               lambda r: (r / D.DECISIONS_DIR / "0130-misfiled.md").write_text("## 130. Misfiled\n**Lane:** B\n",
                                                                               encoding="utf-8"))
        mutate("D3 a new decision without a Lane field", "D3",
               lambda r: (r / D.DECISIONS_DIR / "0201-nolane.md").write_text("## 201. No lane\n", encoding="utf-8"))
        mutate("Q1 a question file whose name is not its id", "Q1",
               lambda r: (r / D.QUESTIONS_DIR / "B-Q9.md").write_text("## B-Q3 — [B] Which? — Status: OPEN\n",
                                                                      encoding="utf-8"))
        mutate("Q1 an answered question left in the queue", "Q1",
               lambda r: (r / D.QUESTIONS_DIR / "B-Q4.md").write_text("## B-Q4 — [B] Which? — Status: ANSWERED\n",
                                                                      encoding="utf-8"))
        mutate("Q2 an open question reusing an answered id", "Q2",
               lambda r: (r / D.QUESTIONS_DIR / "B-Q1.md").write_text("## B-Q1 — [B] Again? — Status: OPEN\n",
                                                                      encoding="utf-8"))
        mutate("F1 a deferred question without a trigger", "F1",
               lambda r: (r / D.DEFERRED_DIR / "DQ2.md").write_text("## DQ2 — [B] Later?\n\nno trigger here\n",
                                                                     encoding="utf-8"))
        mutate("T1 a done item left in todo.txt", "T1",
               lambda r: (r / "todo.txt").write_text("## Lane B — userland\n## * DONE: the thing\n##\n",
                                                      encoding="utf-8"))
        mutate("R1 a done item left in roadmap.md", "R1",
               lambda r: (r / "roadmap.md").write_text("## Phase 1\n- [x] `[B]` finished task\n", encoding="utf-8"))
        mutate("R2 a checked item with open sub-items is only a warning", "R2",
               lambda r: (r / "roadmap.md").write_text("## Phase 1\n- [x] `[B]` batch\n  - [ ] part\n",
                                                       encoding="utf-8"), error=False)
        # R2 goes to whoever owns the open sub-item -- its own tag, else its parent's
        # -- and --list-mixed lists exactly one lane's. The real shape: an untagged
        # parent finished before the lane split, with a lane's new task filed under it.
        r2 = _fixture(tmp / "r2-lanes")
        (r2 / "roadmap.md").write_text(
            "## Phase 1\n- [x] `[B]` b batch\n  - [ ] b part\n  - [x] b done part\n"
            "- [x] old untagged batch\n  - [x] old step\n  - [ ] `[C]` c's new task\n  - [ ] `[A]` a's new task\n"
            "- [x] untagged batch\n  - [ ] u part\n- [x] `[C]` c finished\n  - [x] all done\n", encoding="utf-8")
        res = check(r2, lane="B", strict=False, baseline={})
        got = sorted((f.lane, f.error) for f in res.findings if f.rule == "R2")
        listing = list_mixed(r2, "c")
        ok = (got == [("", False), ("A", False), ("B", False), ("C", False)]
              and "c's new task" in listing and "a's new task" not in listing and "b part" not in listing
              and "all done" not in listing and listing.splitlines()[-1].startswith("1 item(s).")
              and "u part" in list_mixed(r2, "-") and list_mixed(r2, "all").splitlines()[-1].startswith("3 item(s)."))
        print(f"  {'ok  ' if ok else 'FAIL'} R2 goes to the lane owning each open sub-item; --list-mixed lists one lane's"
              + ("" if ok else f" -> {got} / {listing!r}"))
        failures += 0 if ok else 1
        # no verdict on an implausibly empty tree
        empty = tmp / "empty"
        (empty / D.ISSUES_OPEN_DIR).mkdir(parents=True)
        (empty / D.DECISIONS_DIR).mkdir()
        (empty / D.DECISIONS_DIR / "README.md").write_text("| §1–§9 | **lane A** | **open** | x |\n", encoding="utf-8")
        res = check(empty, lane="A", strict=False, baseline={})
        ok = bool(res.no_verdict)
        print(f"  {'ok  ' if ok else 'FAIL'} an implausibly empty tree gets no verdict, not a pass")
        failures += 0 if ok else 1
    print(f"self-test: {'all passed' if not failures else f'{failures} FAILED'}")
    return 0 if not failures else 1


# --- main ------------------------------------------------------------------------------------------

def running_lane(root: Path) -> str | None:
    try:
        import srcload
        wl = srcload.load(SCRIPTS / "which-lane.py", "which_lane")
        letter, _how = wl.lane_of_worktree(root)
        return letter if letter and letter != "!" else None
    except Exception:  # noqa: BLE001 - lane detection is advisory: unknown means "warn, don't fail"
        return None


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--head", default=None, help="check the documents at this revision instead of the working tree")
    ap.add_argument("--lane", default=None, help="the lane whose entries are errors (default: this worktree's)")
    ap.add_argument("--strict", action="store_true", help="every finding is an error")
    # Both spellings: the push hook uses `--selftest` (test-pre-push-gates.py keys
    # on it), the boot test `--self-test`.
    ap.add_argument("--self-test", "--selftest", dest="self_test", action="store_true")
    ap.add_argument("--next-decision", metavar="LANE")
    ap.add_argument("--next-question", metavar="LANE")
    ap.add_argument("--fix-roadmap", action="store_true")
    ap.add_argument("--list-mixed", metavar="LANE",
                    help="list a lane's [x] roadmap items that still hold open sub-items (R2); `-` untagged, `all`")
    ap.add_argument("--write-baseline", action="store_true", help="(cutover only) grandfather today's findings")
    ap.add_argument("--quiet", action="store_true", help="print errors only")
    # The push hook runs the open-questions rules as their own gate (29), as they
    # were before the cutover, so each gate's bypass covers exactly one concern.
    ap.add_argument("--only-rules", default="", help="comma-separated rule prefixes to report, e.g. Q")
    ap.add_argument("--exclude-rules", default="", help="comma-separated rule prefixes not to report")
    args = ap.parse_args(argv)
    if args.self_test:
        return self_test()
    if args.head and (args.write_baseline or args.fix_roadmap):
        print("check-docs: --head and --write-baseline/--fix-roadmap are mutually exclusive: those write the "
              "worktree, which --head is defined not to read", file=sys.stderr)
        return 2
    root = D.repo_root()
    if args.next_decision:
        print(next_decision(root, args.next_decision))
        return 0
    if args.next_question:
        print(next_question(root, args.next_question))
        return 0
    if args.fix_roadmap:
        print(fix_roadmap(root))
        return 0
    if args.list_mixed:
        print(list_mixed(root, args.list_mixed))
        return 0
    if args.write_baseline:
        bl = write_baseline(root)
        # Into the checkout being judged, not next to this script: the two differ
        # when the script is run against another tree (the test suites do).
        target = root / BASELINE_REL
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(bl, indent=1, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
        print(f"wrote {BASELINE_REL}: " + ", ".join(f"{k}={len(v)}" for k, v in bl.items()))
        return 0
    lane = args.lane.upper() if args.lane and args.lane.lower() != "none" else (None if args.lane else running_lane(root))

    def judge(tree: Path, where: str) -> Result:
        """Check `tree` with the baseline from that same tree. A tree that has the
        per-entry documents but no baseline gets no verdict: without it, every
        finding the cutover inherited would read as new."""
        bl_path = tree / BASELINE_REL
        if any((tree / d).is_dir() for d in D.ENTRY_DIRS) and not bl_path.is_file():
            res = Result()
            res.no_verdict = f"{BASELINE_REL} does not exist {where}, so new findings cannot be told from inherited ones"
            return res
        return check(tree, lane, args.strict, load_baseline(bl_path))

    try:
        if args.head:
            if not is_commit(root, args.head):
                raise NoVerdict(f"--head {args.head}: not a commit")
            with tempfile.TemporaryDirectory() as t:
                materialise(root, args.head, Path(t))
                res = judge(Path(t), f"at {args.head}")
        else:
            res = judge(root, "in the worktree")
    except NoVerdict as exc:
        print(f"check-docs: NO VERDICT -- {exc}", file=sys.stderr)
        return 2
    if res.no_verdict:
        print(f"check-docs: NO VERDICT -- {res.no_verdict}", file=sys.stderr)
        return 2
    only = [r.strip() for r in args.only_rules.split(",") if r.strip()]
    exclude = [r.strip() for r in args.exclude_rules.split(",") if r.strip()]
    res.findings = [f for f in res.findings if (not only or f.rule.startswith(tuple(only)))
                    and not (exclude and f.rule.startswith(tuple(exclude)))]
    errors = [f for f in res.findings if f.error]
    warnings = [f for f in res.findings if not f.error]
    for f in errors + ([] if args.quiet else warnings):
        print(f.render())
    who = f"lane {lane}" if lane else "no lane, so every lane's findings are errors (R2 is always a warning)"
    print(f"check-docs: {len(errors)} error(s), {len(warnings)} warning(s); checked as {who}; "
          + ", ".join(f"{k}={v}" for k, v in res.counts.items()))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
