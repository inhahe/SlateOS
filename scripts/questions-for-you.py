#!/usr/bin/env python3
"""Rebuilds `questions-for-you/`: the operator's questions not yet answered.

A folder in the integration tree holding only the questions still waiting on
the operator, rebuilt from every lane's branch and the operator's answers.

    python scripts/questions-for-you.py            rebuild the folder
    python scripts/questions-for-you.py --print    show its index, write nothing
    python scripts/questions-for-you.py --selftest

The operator runs it by hand, with an `update.bat` kept in the folder
(2026-10-10): nothing else runs it.

Why
---
`open-questions/` in the integration tree shows the main line. A question
leaves it only when its own lane records the answer and that lane next
publishes to main -- which takes a boot test, and waits on the lane being
able to run at all. A lane rate-limited for days leaves every question the
operator has answered sitting among the open ones (the operator, 2026-10-10:
`operator-answers/2026-10-10-chat-to-lane-f.txt`).

So this builds `questions-for-you/` beside it: a copy of each question still
waiting on the operator, and an index, `_index.md`, which also lists the
answered questions no lane has recorded yet, so nothing leaves the operator's
sight unremarked. Each lane's questions are its files as its own branch has
them (`lane-x`, the local branch every worktree shares, which holds what the
lane has committed before it pushes): a question a lane has just written
shows at once, and one it has recorded (deleted) is gone.

What counts as answered
-----------------------
`operator-answers/LEDGER.md` is the record: a section per answers file or
chat, a row per answer, and -- its own rule -- whatever still waits on the
operator says so in bold. It exists once per branch, each lane adding to its
own copy, so it is read on every lane's branch and on main:

* A row whose first cell starts with a question's id answers that question,
  unless its state is in bold. Bold that names another question ("waits on
  you as C-Q31") leaves the row's own question answered and that one waiting.
  Bold used for anything else reads as waiting too: the question shows, with
  the row beside it in the index.
* Where branches disagree about a row, the question's own lane has the last
  word -- it is the lane that records the answer or asks again -- then
  `origin/main`, then `main` (the integration tree's checkout, which can lag
  it), then whichever branch says the question still waits.
* Of a question's rows, the newest section's decides: the later date; on the
  same day, the one higher in a ledger that holds both (newest first, by its
  rule), else the later time where both give one. Where that leaves more than
  one, any of them saying the question waits is enough.
* A section about a lane's reply to the operator (`claude-answers.txt`)
  answers nothing, whatever its rows say: lane A's account relayed to lane D
  is not the operator choosing.

An answers file in the integration tree whose content no ledger records yet
-- one the operator has just written, or added to -- answers, at once, each
question its new lines name: by id (`D-Q3:`, in either case) or by title, as
`check-lane-signals.py` reads them. A line is new unless the same line is in
one of the verbatim copies in `operator-answers/` on any branch, which are
what the lanes have processed. Such a line beats the ledger, which by then
can only be answering something older, unless the file was last written
before the day of the ledger's word. A question nothing answers is waiting.

The folder
----------
It writes nothing but that folder, which it tells git to ignore
(`.git/info/exclude`, shared by every worktree). Each copy opens with a line
saying it is one and where answers go, and is read-only, since an edit there
would be lost at the next refresh. Other files in the folder are left alone;
one holding answers is named in the index, because nothing reads it. A file is
rewritten only when its text changes (the index's "Last changed" line aside),
and a refresh that fails says so at the top of the index and in its exit
status, rather than leaving the last good index looking current.
"""

from __future__ import annotations

import datetime as dt
import importlib.util
import os
import re
import stat
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
LANES = "ABCDEF"
VIEW = "questions-for-you"
INDEX = "_index.md"
#: The index's one line that changes with time alone: whether the index
#: needs rewriting is judged without it.
STAMP = "Last changed "
#: How the line a failed refresh puts at the top of the index starts.
FAILED = "**The last refresh failed**"
QUESTION_FILE = re.compile(r"^([A-F])-Q([0-9]+)\.md$")
QUESTION_ID = re.compile(r"\b([A-F]-Q[0-9]+)\b")
#: The ids a ledger row's first cell starts with: `C-Q34`, `D-Q3, D-Q8`,
#: `B-Q23: Claude's recommendation -- ...`. Ids later in the cell are only
#: mentioned.
LEADING_IDS = re.compile(r"^[\s*`]*([A-F]-Q[0-9]+(?:\s*,\s*[A-F]-Q[0-9]+)*)\b")
BOLD = re.compile(r"\*\*(.+?)\*\*")
#: A date, and the time of day if one follows it.
WHEN = re.compile(r"\b(20[0-9]{2})-([0-9]{2})-([0-9]{2})(?:[ T]([0-9]{2}):([0-9]{2}))?\b")


def _load(name: str, file: str):
    """A sibling script, as a module."""
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    if spec is None or spec.loader is None:
        raise SystemExit(f"questions-for-you: scripts/{file} cannot be loaded")
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


#: Whose rules for answers files this shares, so the two never disagree about
#: one: which files they are, the ledger's hash of each, the shape of an
#: answer line -- and its git environment, clean of any `GIT_DIR` a hook
#: exports.
SIGNALS = _load("check_lane_signals", "check-lane-signals.py")
LEDGER = SIGNALS.ANSWERS_LEDGER
COPIES = str(Path(LEDGER).parent)
#: `check-lane-signals.py`'s answer by id, in either case: the operator types
#: `b-q23` as readily as `B-Q23`.
ANSWER_ID = re.compile(SIGNALS._ANSWER_ID.pattern, re.MULTILINE | re.IGNORECASE)


def lane_reply(path: str) -> bool:
    """Whether `path` names a lane's reply to the operator, saved beside the
    answers files (`claude-answers.txt`): no answers of the operator's."""
    return Path(path.strip("`")).name.lower().startswith("claude")


class GitError(RuntimeError):
    """A git command failed: the view is not built from a guess."""


def _git(args: list[str], cwd: Path, data: bytes | None = None) -> bytes:
    """`git <args>` in `cwd`'s repository -- never one an inherited `GIT_DIR`
    names -- its output; GitError when it fails."""
    r = subprocess.run(["git", *args], cwd=str(cwd), env=SIGNALS._clean_git_env(), input=data,
                       capture_output=True, check=False)
    if r.returncode != 0:
        why = r.stderr.decode("utf-8", "replace").strip() or f"exit status {r.returncode}"
        raise GitError(f"git {' '.join(args)}: {why}")
    return r.stdout


def branch_refs(tree: Path) -> dict[str, str]:
    """The branches read, by name: `main` and `origin/main` where they exist,
    and each lane's by its letter -- its local branch, which holds what the
    lane has committed before it pushes, else origin's."""
    have = set(_git(["for-each-ref", "--format=%(refname)", "refs/heads/main",
                     "refs/remotes/origin/main", "refs/heads/lane-*", "refs/remotes/origin/lane-*"],
                    tree).decode("utf-8", "replace").split())
    refs = {name: ref for name, ref in (("main", "refs/heads/main"),
                                        ("origin/main", "refs/remotes/origin/main")) if ref in have}
    for lane in LANES:
        for ref in (f"refs/heads/lane-{lane.lower()}", f"refs/remotes/origin/lane-{lane.lower()}"):
            if ref in have:
                refs[lane] = ref
                break
    return refs


@dataclass
class Object:
    """An object `git cat-file --batch` read."""

    kind: str
    data: bytes
    #: How many bytes an object id takes in the repository: 20 for SHA-1,
    #: 32 for SHA-256 -- what a tree object's entries end with.
    id_size: int


def read_objects(tree: Path, names: list[str]) -> dict[str, Object]:
    """Each object of `names` -- `rev:path`, or an object id -- that exists,
    from one `git cat-file --batch`."""
    if not names:
        return {}
    out = _git(["cat-file", "--batch"], tree, "".join(f"{n}\n" for n in names).encode("utf-8"))
    found: dict[str, Object] = {}
    at = 0
    for name in names:
        end = out.find(b"\n", at)
        if end < 0:
            raise GitError("git cat-file --batch: its output ended early")
        header = out[at:end].split()
        at = end + 1
        if len(header) != 3 or header[-1] == b"missing":
            continue
        size = int(header[2])
        found[name] = Object(header[1].decode("ascii", "replace"), out[at:at + size],
                             len(header[0]) // 2)
        at += size + 1
    return found


def tree_entries(obj: Object) -> list[tuple[str, str]]:
    """The entries of a tree object, as name and object id: each is its mode,
    a space, its name, a NUL, and the id."""
    entries, at = [], 0
    while at < len(obj.data):
        space = obj.data.index(b" ", at)
        nul = obj.data.index(b"\0", space)
        name = obj.data[space + 1:nul].decode("utf-8", "replace")
        entries.append((name, obj.data[nul + 1:nul + 1 + obj.id_size].hex()))
        at = nul + 1 + obj.id_size
    return entries


@dataclass
class Row:
    """What one ledger row says of one question."""

    answered: bool
    #: The row's State cell, as written.
    state: str


@dataclass
class Section:
    """One section of a ledger -- an answers file or a chat -- and its rows."""

    #: The content hash the section records, else its heading: the same
    #: section on every branch.
    key: str
    heading: str
    #: The start of its first date, local time; 0 if it has none.
    day: int = 0
    #: The time of day given with that date, in minutes; None if none is.
    minute: int | None = None
    rows: dict[str, Row] = field(default_factory=dict)


def _when(text: str) -> tuple[int, int | None] | None:
    """The first date in `text` -- the start of its day, local time, as a
    timestamp -- and the time of day after it in minutes, if there is one."""
    m = WHEN.search(text)
    if not m:
        return None
    y, mo, d = (int(g) for g in m.groups()[:3])
    try:
        day = int(dt.datetime(y, mo, d).timestamp())
    except (ValueError, OverflowError, OSError):
        return None
    minute = int(m.group(4)) * 60 + int(m.group(5)) if m.group(4) else None
    return day, minute


def sections(text: str) -> list[Section]:
    """A ledger's sections, top -- the newest -- to bottom, each with what its
    rows say of each question. A section about a lane's reply keeps none."""
    built: list[tuple[Section, dict[str, Row], dict[str, Row]]] = []
    for line in text.splitlines():
        if line.startswith("## "):
            heading = line[3:].strip()
            built.append((Section(key="", heading=heading), {}, {}))
        if not built:
            continue
        sec, owns, named = built[-1]
        if not line.startswith("|"):
            when = None if sec.day else _when(line)
            if when:
                sec.day, sec.minute = when
            m = None if sec.key else SIGNALS._LEDGER_SHA256.search(line)
            if m:
                sec.key = m.group(1)
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4 or lane_reply(sec.heading.split(maxsplit=1)[0] if sec.heading else ""):
            continue
        state = cells[3]
        bold = " ".join(BOLD.findall(state))
        mentioned = set(QUESTION_ID.findall(bold))
        lead = LEADING_IDS.match(cells[0])
        own = QUESTION_ID.findall(lead.group(1)) if lead else []
        waits = bool(bold) and (not mentioned or bool(mentioned & set(own)))
        for qid in own:
            # Two rows for one question in one section: one saying it still
            # waits is enough.
            if qid not in owns or owns[qid].answered:
                owns[qid] = Row(answered=not waits, state=state)
        for qid in mentioned - set(own):
            named.setdefault(qid, Row(answered=False, state=state))
    for sec, owns, named in built:
        sec.rows = {**named, **owns}
        sec.key = sec.key or sec.heading
    return [b[0] for b in built]


@dataclass
class Ledgers:
    """The ledger as every branch read has it."""

    #: Each branch's sections, by the branch's name in `branch_refs`.
    by_branch: dict[str, list[Section]]
    #: Every answers-file hash any branch's ledger records.
    recorded: set[str]

    def newer(self, a: Section, b: Section) -> bool | None:
        """Whether section `a` is newer than `b`; None when nothing tells."""
        if a.day != b.day:
            return a.day > b.day
        for secs in self.by_branch.values():
            keys = [s.key for s in secs]
            if a.key in keys and b.key in keys:
                return keys.index(a.key) < keys.index(b.key)
        if a.minute is not None and b.minute is not None and a.minute != b.minute:
            return a.minute > b.minute
        return None


def read_ledgers(texts: dict[str, str]) -> Ledgers:
    """`texts`, each branch's ledger by the branch's name, read."""
    return Ledgers({name: sections(text) for name, text in texts.items()},
                   SIGNALS.recorded_keys(list(texts.values())))


def ledger_word(ledgers: Ledgers, qid: str, lane: str) -> tuple[Section, Row] | None:
    """The ledger's last word on `qid`, a question of lane `lane`, and the
    section it is in; None if no row names it."""
    said: dict[str, tuple[Section, dict[str, Row]]] = {}
    for branch, secs in ledgers.by_branch.items():
        for sec in secs:
            if qid in sec.rows:
                said.setdefault(sec.key, (sec, {}))[1][branch] = sec.rows[qid]
    if not said:
        return None
    words = []
    for key, (sec, rows) in said.items():
        if any(ledgers.newer(other, sec) for k, (other, _) in said.items() if k != key):
            continue
        for branch in (lane, "origin/main", "main"):
            if branch in rows:
                words.append((sec, rows[branch]))
                break
        else:
            waiting = [r for r in rows.values() if not r.answered]
            words.append((sec, (waiting or list(rows.values()))[0]))
    waiting_words = [w for w in words if not w[1].answered]
    return (waiting_words or words)[0]


@dataclass
class Question:
    qid: str
    lane: str
    #: The question's file, as its lane's branch has it.
    text: str
    #: The ledger's word on it, or where it was answered: for the index.
    why: str = ""


def heading(text: str) -> str:
    """A question's heading, without its markdown."""
    for line in text.splitlines():
        if line.startswith("#"):
            return line.lstrip("#").strip()
    return "(no heading)"


def _plain(text: str) -> str:
    return " ".join(text.replace("*", "").replace("`", "").lower().split())


def by_title(match: re.Match[str], listed: list[Question]) -> str | None:
    """The question a title-keyed answer line (`check-lane-signals.py`'s
    `_ANSWER_TITLED`) names: the one of its lane whose heading holds the
    title, if exactly one does."""
    title = _plain(match.group(0).rsplit("(lane", 1)[0])
    if len(title) < 12:
        return None
    hits = [q.qid for q in listed if q.lane == match.group(1) and title in _plain(heading(q.text))]
    return hits[0] if len(hits) == 1 else None


def answer_lines(text: str, listed: list[Question]) -> list[tuple[str, str | None]]:
    """Each answer line of an answers file, with the question it answers if
    that is one of `listed`'s or has an id."""
    found = []
    for line in text.splitlines():
        m = ANSWER_ID.match(line)
        if m:
            found.append((line, m.group(1).upper() if "-" in m.group(1) else None))
            continue
        t = SIGNALS._ANSWER_TITLED.match(line)
        if t:
            found.append((line, by_title(t, listed)))
    return found


def processed_lines(copies: list[str]) -> set[str]:
    """The answer lines of every verbatim copy in `operator-answers/`: what
    the lanes have processed, compared as `_plain` text."""
    return {_plain(line) for text in copies for line, _ in answer_lines(text, [])}


def new_answers(tree: Path, ledgers: Ledgers, listed: list[Question],
                processed: set[str]) -> dict[str, tuple[int, str]]:
    """The questions answered by a line no lane has processed, in an answers
    file of `tree` whose content no ledger records: each with when its file
    was last written, and the file's name."""
    found: dict[str, tuple[int, str]] = {}
    for path, _ in SIGNALS.unrecorded_answers(tree, ledgers.recorded):
        rel = path.relative_to(tree).as_posix()
        if lane_reply(rel):
            continue
        try:
            text = path.read_bytes().decode("utf-8", "replace")
            when = int(path.stat().st_mtime)
        except OSError:
            continue  # Gone, or held: the next refresh reads it.
        for line, qid in answer_lines(text, listed):
            if qid and _plain(line) not in processed and found.get(qid, (-1, ""))[0] < when:
                found[qid] = (when, rel)
    return found


@dataclass
class View:
    """What the folder is to hold."""

    waiting: list[Question]
    #: Answered, and still among their lane's questions.
    answered: list[Question]
    #: Files in the folder, not this script's, that hold answers nobody reads.
    stray: list[str]


def build(tree: Path) -> View:
    """Each lane's questions as its branch has them, split into those waiting
    on the operator and those answered."""
    refs = branch_refs(tree)
    lanes = [lane for lane in LANES if lane in refs]
    first = read_objects(tree, [f"{refs[lane]}:open-questions" for lane in lanes]
                         + [f"{ref}:{LEDGER}" for ref in refs.values()]
                         + [f"{ref}:{COPIES}" for ref in refs.values()])
    wanted = []
    for lane in lanes:
        folder = first.get(f"{refs[lane]}:open-questions")
        for name, _ in tree_entries(folder) if folder and folder.kind == "tree" else []:
            m = QUESTION_FILE.match(name)
            if m and m.group(1) == lane:
                wanted.append((name[:-3], lane, f"{refs[lane]}:open-questions/{name}"))
    copies = set()
    for ref in refs.values():
        folder = first.get(f"{ref}:{COPIES}")
        for name, oid in tree_entries(folder) if folder and folder.kind == "tree" else []:
            if name.endswith(".txt"):
                copies.add(oid)
    blobs = read_objects(tree, [w[2] for w in wanted] + sorted(copies))
    listed = [Question(qid, lane, blobs[name].data.decode("utf-8", "replace"))
              for qid, lane, name in sorted(wanted, key=lambda w: _order(w[0])) if name in blobs]
    ledgers = read_ledgers({branch: first[f"{ref}:{LEDGER}"].data.decode("utf-8", "replace")
                            for branch, ref in refs.items() if f"{ref}:{LEDGER}" in first})
    processed = processed_lines([blobs[oid].data.decode("utf-8", "replace")
                                 for oid in sorted(copies) if oid in blobs])
    fresh = new_answers(tree, ledgers, listed, processed)
    view = View(waiting=[], answered=[], stray=strays(tree, listed))
    for q in listed:
        word = ledger_word(ledgers, q.qid, q.lane)
        answered = word is not None and word[1].answered
        q.why = word[1].state if word else ""
        if q.qid in fresh and (word is None or fresh[q.qid][0] >= word[0].day):
            answered, q.why = True, f"answered in `{fresh[q.qid][1]}`"
        (view.answered if answered else view.waiting).append(q)
    return view


def strays(tree: Path, listed: list[Question]) -> list[str]:
    """The files in the folder, not this script's, that hold answer lines:
    an answers file saved there, which nothing reads."""
    found = []
    try:
        entries = sorted((tree / VIEW).iterdir())
    except OSError:
        return []
    for path in entries:
        if path.name == INDEX or QUESTION_FILE.match(path.name) or not path.is_file():
            continue
        try:
            text = path.read_bytes()[:1 << 16].decode("utf-8", "replace")
        except OSError:
            continue
        if answer_lines(text, listed):
            found.append(path.name)
    return found


def _order(qid: str) -> tuple[str, int]:
    lane, _, n = qid.partition("-Q")
    return lane, int(n)


def title(text: str) -> str:
    """A question's heading without its id or its status."""
    h = re.sub(r"^[A-F]-Q[0-9]+\s*[—–:-]+\s*", "", heading(text))
    return re.sub(r"\s+[—–-]+\s+(?:Status:\s*)?OPEN\b.*$", "", h)


def in_short(text: str) -> str:
    """A question's `**In short:**` paragraph, on one line; "" if none."""
    lines, taking = [], False
    for line in text.splitlines():
        if line.lstrip().startswith("**In short:**"):
            taking = True
        if taking:
            if not line.strip():
                break
            lines.append(line.strip())
    return " ".join(lines)


def _clip(text: str, most: int = 200) -> str:
    text = " ".join(text.split())
    return text if len(text) <= most else text[:most - 3].rstrip() + "..."


def index(view: View, now: dt.datetime) -> str:
    """The folder's `_index.md`."""
    out = [
        "# Questions waiting on you",
        "",
        "Each other file here is a copy of one question still waiting on you, as its lane",
        "last wrote it. `scripts/questions-for-you.py` rebuilds the folder from every lane's",
        "branch, the ledger of your answers and your answers files: a question you answer",
        "drops out the next time it runs, even while its lane cannot run, and a new one",
        "shows up. To answer, add a line such as `D-Q3: B` to an answers file in",
        "`open-questions/` (a new one, such as `answers.3.txt`, is fine), or tell a lane.",
        "",
        f"{STAMP}{now:%Y-%m-%d %H:%M}.",
        "",
    ]
    if view.stray:
        out += ["**Nothing reads the answers in these files here:** "
                + ", ".join(f"`{name}`" for name in view.stray)
                + ". Answers go in `open-questions/`.", ""]
    out += [f"## Waiting on you ({len(view.waiting)})", ""]
    if not view.waiting:
        out += ["Nothing.", ""]
    for q in view.waiting:
        out.append(f"- **{q.qid}** ({q.qid}.md): {title(q.text)}")
        if BOLD.search(q.why):
            out.append(f"  *The ledger:* {_clip(q.why)}")
        summary = in_short(q.text)
        if summary:
            out.append(f"  {summary}")
        out.append("")
    out += [f"## Answered, waiting for their lanes to record them ({len(view.answered)})", ""]
    if not view.answered:
        out.append("None.")
    for q in view.answered:
        out.append(f"- {q.qid}: {_clip(q.why) or 'answered'}")
    return "\n".join(out).rstrip() + "\n"


def copy_text(q: Question) -> str:
    """A waiting question's file in the folder."""
    return (f"> A copy of lane {q.lane}'s question, refreshed automatically: an edit here is "
            f"lost. To answer, add a line `{q.qid}: <your answer>` to an answers file in "
            f"`open-questions/`.\n\n{q.text}")


def write(tree: Path, view: View, now: dt.datetime) -> list[str]:
    """The folder brought to `view`: each file written whose text differs,
    each question file no longer waiting removed. Returns what changed;
    OSError, after doing what it can, naming what it could not."""
    folder = tree / VIEW
    folder.mkdir(exist_ok=True)
    changed: list[str] = []
    failed: list[str] = []

    def attempt(done: bool | None, name: str) -> None:
        if done is None:
            failed.append(name)
        elif done:
            changed.append(name)

    wanted = {f"{q.qid}.md": copy_text(q) for q in view.waiting}
    for name, text in wanted.items():
        attempt(_try(_put, folder / name, text), name)
    for old in sorted(folder.iterdir()):
        if QUESTION_FILE.match(old.name) and old.name not in wanted:
            attempt(_try(_remove, old), f"-{old.name}")
    text = index(view, now)
    if _unstamped(_read(folder / INDEX)) != _unstamped(text):
        attempt(_try(_put, folder / INDEX, text), INDEX)
    _ignore(tree)
    if failed:
        raise OSError(f"could not write {', '.join(failed)} in {folder}")
    return changed


def _try(action, *args) -> bool | None:
    """`action(*args)`; None if it raised OSError."""
    try:
        return action(*args)
    except OSError:
        return None


def _failed(tree: Path, error: str, now: dt.datetime) -> None:
    """A failed refresh, said at the top of the index above the last good
    one -- which is otherwise left as it was. OSError if it cannot be."""
    path = tree / VIEW / INDEX
    if not path.parent.is_dir():
        return
    lines = [line for line in _read(path).splitlines() if not line.startswith(FAILED)]
    note = (f"{FAILED} ({now:%Y-%m-%d %H:%M}): {_clip(error, 400)} -- what follows is from "
            "the refresh before it.")
    _put(path, "\n".join([*lines[:1], note, *lines[1:]]) + "\n")


def _unstamped(text: str) -> list[str]:
    return [line for line in text.splitlines() if not line.startswith(STAMP)]


def _read(path: Path) -> str:
    """`path`'s text; "" when it is missing or cannot be read."""
    try:
        return path.read_bytes().decode("utf-8", "replace")
    except OSError:
        return ""


def _put(path: Path, text: str) -> bool:
    """`path` holding `text`, read-only; whether it had to be written."""
    data = text.encode("utf-8")
    if path.exists():
        if path.read_bytes() == data:
            os.chmod(path, stat.S_IREAD)
            return False
        os.chmod(path, stat.S_IREAD | stat.S_IWRITE)
    path.write_bytes(data)
    os.chmod(path, stat.S_IREAD)
    return True


def _remove(path: Path) -> bool:
    """`path` removed, read-only or not."""
    os.chmod(path, stat.S_IREAD | stat.S_IWRITE)
    path.unlink()
    return True


def _ignore(tree: Path) -> None:
    """`/questions-for-you/` in `.git/info/exclude`, once: appended, so the
    lines already there are never rewritten."""
    exclude = tree / ".git" / "info" / "exclude"
    line = f"/{VIEW}/".encode("ascii")
    try:
        old = exclude.read_bytes() if exclude.exists() else b""
        if line in old.splitlines():
            return
        exclude.parent.mkdir(parents=True, exist_ok=True)
        with open(exclude, "ab") as f:
            f.write((b"\n" if old and not old.endswith(b"\n") else b"") + line + b"\n")
    except OSError:
        pass  # Untracked either way; `git status` only shows it.


def _snapshot(tree: Path) -> dict[str, tuple[int, int]]:
    """Every file under `tree` but `.git` and the folder: size and mtime."""
    return {p.relative_to(tree).as_posix(): (p.stat().st_size, p.stat().st_mtime_ns)
            for p in tree.rglob("*")
            if p.is_file() and p.relative_to(tree).parts[0] not in (".git", VIEW)}


def _fixture(tree: Path, check) -> None:
    """The self-test's checks, in a throwaway repository at `tree`."""
    tree.mkdir()

    def git(*args: str, when: str | None = None) -> None:
        env = SIGNALS._clean_git_env()
        if when:
            env.update(GIT_AUTHOR_DATE=when, GIT_COMMITTER_DATE=when)
        r = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c",
                            "commit.gpgsign=false", *args], cwd=str(tree), env=env,
                           capture_output=True, text=True, encoding="utf-8", errors="replace",
                           check=False)
        if r.returncode != 0:
            raise SystemExit(f"self-test: git {args}: {r.stderr}")

    def put(rel: str, text: str) -> Path:
        p = tree / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8", newline="\n")
        return p

    def ask(qid: str, about: str) -> None:
        put(f"open-questions/{qid}.md",
            f"## {qid} — [{qid[0]}] {about} — Status: OPEN\n\n**In short:** {about.lower()}.\n")

    git("init", "-q", "-b", "main")
    put("open-questions/README.md", "rules\n")
    ask("B-Q1", "Answered in a new line, its id in lower case")
    ask("B-Q2", "Still open, though a lane's reply names it")
    ask("B-Q3", "Answered by its title alone")
    ask("B-Q4", "Answered in a file whose name ends in claude")
    ask("B-Q5", "Asked again on origin/main, which main lags")
    git("add", "-A")
    git("commit", "-q", "-m", "b", when="2026-10-01T12:00:00")
    git("branch", "lane-b")
    for n, about in enumerate(["Relayed as answered", "Asked again by its own lane",
                               "Not answered, then answered in a line added to the file",
                               "Answered, then asked again in a newer section",
                               "Questions back, then answered in a newer section",
                               "Answered in a row that mentions another",
                               "Mentioned only, and in files that answer nothing",
                               "Recorded, the rest carried by another", "Carrying the rest",
                               "Asked back in a row, then answered in a line added"], start=1):
        ask(f"C-Q{n}", about)
    old = "C-Q1: A\nC-Q3: I cannot decide\n"
    second = "C-Q4: which one?\nC-Q5: A\n"
    zero = "C-Q7: A\n"
    table = "| Answer | Lane | Recorded in | State |\n|---|---|---|---|\n"
    reply = (f"## open-questions/claude-answers.txt\n\n- **Written:** 2026-10-06, a lane's reply.\n"
             f"- **Content:** sha256 `{'c' * 64}`\n\n{table}"
             "| C-Q7 | C | -- | relayed 2026-10-06 (lane A's account) |\n\n")
    # Newer than `first` the same day -- higher in the ledger -- and, like a
    # section a lane has not published yet, on lane C's branch alone.
    newer = (f"## open-questions/answers.2.txt\n\n- **Written:** 2026-10-05 16:15\n"
             f"- **Copy:** `operator-answers/2026-10-05-open-questions-answers.2.txt`\n"
             f"- **Content:** sha256 `{SIGNALS.answers_key(second.encode())}`\n\n{table}"
             "| C-Q4 | C | -- | relayed 2026-10-05; **asked again: which one?** |\n"
             "| C-Q5 | C | -- | relayed 2026-10-05 (A) |\n"
             "| \"...a question of your own?\" | C | -- | **Answered, with one question back to "
             "you: C-Q10.** |\n\n")
    first = (f"## open-questions/answers.txt\n\n- **Written:** 2026-10-05\n"
             f"- **Copy:** `operator-answers/2026-10-05-open-questions-answers.txt`\n"
             f"- **Content:** sha256 `{SIGNALS.answers_key(old.encode())}`\n\n{table}"
             "| C-Q1 | C | -- | relayed 2026-10-05 (A) |\n"
             "| C-Q2 | C | -- | relayed 2026-10-05 (B) |\n"
             "| C-Q3 | C | -- | **not answered** in this file |\n"
             "| C-Q4 | C | -- | relayed 2026-10-05 (A) |\n"
             "| C-Q5 | C | -- | relayed 2026-10-05; **questions back, no option chosen yet** |\n"
             "| C-Q6: A -- the line read as C-Q7's was C-Q6's | C | -- | relayed 2026-10-05 |\n"
             "| C-Q8 | C | §1 | recorded; **the rest waits on you as C-Q9** |\n"
             "| B-Q5 | B | -- | relayed 2026-10-05 (A) |\n\n")
    # Recorded, with no copy kept: its lines must not count as new.
    unkept = (f"## open-questions/answers.0.txt\n\n- **Written:** 2026-10-04\n"
              f"- **Content:** sha256 `{SIGNALS.answers_key(zero.encode())}`\n")
    put(LEDGER, f"# ledger\n\n{reply}{first}{unkept}")
    put("operator-answers/2026-10-05-open-questions-answers.txt", old)
    git("add", "-A")
    git("commit", "-q", "-m", "c", when="2026-10-02T12:00:00")
    git("branch", "lane-c")
    git("checkout", "-q", "lane-c")
    put("open-questions/C-Q2.md", "## C-Q2 — [C] Asked again by its own lane\n\nAsked again.\n")
    put(LEDGER, f"# ledger\n\n{reply}{newer}" + first.replace(
        "relayed 2026-10-05 (B) |", "relayed 2026-10-05; **asked again after your answer** |")
        + unkept)
    put("operator-answers/2026-10-05-open-questions-answers.2.txt", second)
    git("add", "-A")
    git("commit", "-q", "-m", "c asks again", when="2026-10-07T12:00:00")
    # origin/main a commit ahead of main, as when the integration tree has
    # not been fast-forwarded: there, B-Q5 -- whose lane's branch holds no
    # ledger -- is asked again.
    git("checkout", "-q", "-b", "ahead", "main")
    put(LEDGER, f"# ledger\n\n{reply}" + first.replace(
        "| B-Q5 | B | -- | relayed 2026-10-05 (A) |", "| B-Q5 | B | -- | **asked again** |")
        + unkept)
    git("commit", "-q", "-am", "b-q5 asked again", when="2026-10-08T12:00:00")
    git("update-ref", "refs/remotes/origin/main", "refs/heads/ahead")
    git("checkout", "-q", "main")
    git("branch", "-q", "-D", "ahead")
    put("open-questions/answers.txt", old + "b-q1: A\nC-Q3: B\n")
    put("open-questions/answers.2.txt", second + "C-Q10: no\n")
    put("open-questions/answers.0.txt", zero)
    put("open-questions/answers-for-claude.txt", "B-Q4: A\n")
    put("open-questions/claude-answers.txt", "B-Q2: a lane's remark, not an answer\n")
    late = put("open-questions-answers.3.txt",
               "C-Q4: A\nAnswered by its title alone (lane B, 2026-10-01): yes\n")
    stamp = dt.datetime(2026, 10, 1, 12).timestamp()
    os.utime(late, (stamp, stamp))

    view = build(tree)
    check("waiting", [q.qid for q in view.waiting],
          ["B-Q2", "B-Q5", "C-Q2", "C-Q4", "C-Q7", "C-Q9"])
    check("answered", [q.qid for q in view.answered],
          ["B-Q1", "B-Q3", "B-Q4", "C-Q1", "C-Q3", "C-Q5", "C-Q6", "C-Q8", "C-Q10"])
    why = {q.qid: q.why for q in view.waiting + view.answered}
    check("where each new answer is", (why["B-Q1"], why["C-Q3"], why["C-Q10"]),
          ("answered in `open-questions/answers.txt`", "answered in `open-questions/answers.txt`",
           "answered in `open-questions/answers.2.txt`"))
    check("the ledger's word", (why["C-Q5"], why["C-Q4"]),
          ("relayed 2026-10-05 (A)", "relayed 2026-10-05; **asked again: which one?**"))
    check("in short", in_short(view.waiting[0].text),
          "**In short:** still open, though a lane's reply names it.")
    check("title", title(view.waiting[0].text), "[B] Still open, though a lane's reply names it")

    folder = tree / VIEW
    put(f"{VIEW}/B-Q1.md", "left from an earlier refresh\n")
    put(f"{VIEW}/answers.txt", "C-Q9: A\n")
    put(f"{VIEW}/update.bat", "py questions-for-you.py\npause\n")
    before = _snapshot(tree)
    view = build(tree)
    check("strays", view.stray, ["answers.txt"])
    now = dt.datetime(2026, 10, 10, 12, 0)
    check("written", sorted(write(tree, view, now)),
          sorted(["-B-Q1.md", "B-Q2.md", "B-Q5.md", "C-Q2.md", "C-Q4.md", "C-Q7.md", "C-Q9.md",
                  INDEX]))
    check("the folder", sorted(p.name for p in folder.iterdir()),
          sorted([INDEX, "answers.txt", "update.bat", "B-Q2.md", "B-Q5.md", "C-Q2.md", "C-Q4.md",
                  "C-Q7.md", "C-Q9.md"]))
    copy = _read(folder / "C-Q2.md")
    check("a copy says so, then holds its lane's text",
          (copy.startswith("> A copy of lane C's question"), copy.endswith("Asked again.\n")),
          (True, True))
    check("and is read-only", os.access(folder / "C-Q2.md", os.W_OK), False)
    text = _read(folder / INDEX)
    for what, line in (("lists the waiting", "- **C-Q4** (C-Q4.md): [C] Answered, then asked"),
                       ("with the ledger's word",
                        "  *The ledger:* relayed 2026-10-05; **asked again: which one?**"),
                       ("and the answered", "- C-Q5: relayed 2026-10-05 (A)"),
                       ("names a stray", "**Nothing reads the answers in these files here:** "
                                         "`answers.txt`.")):
        check(f"the index {what}", any(x.startswith(line) for x in text.splitlines()), True)
    check("a second refresh changes nothing", write(tree, build(tree), now), [])
    exclude = tree / ".git" / "info" / "exclude"
    check("ignored once", exclude.read_bytes().splitlines().count(f"/{VIEW}/".encode()), 1)
    write(tree, build(tree), now)
    check("and still once", exclude.read_bytes().splitlines().count(f"/{VIEW}/".encode()), 1)
    status = _git(["status", "--porcelain", "--untracked-files=all"], tree).decode()
    check("git status shows the untracked answers but not the folder",
          ("open-questions/answers.txt" in status, VIEW in status), (True, False))
    check("nothing outside the folder written", _snapshot(tree), before)

    # Lane C records C-Q1: no question file here changes, but the index must.
    git("checkout", "-q", "lane-c")
    git("rm", "-q", "open-questions/C-Q1.md")
    git("commit", "-q", "-m", "c records", when="2026-10-08T12:00:00")
    git("checkout", "-q", "main")
    later = dt.datetime(2026, 10, 10, 13, 0)
    check("a recorded answer rewrites only the index", write(tree, build(tree), later), [INDEX])
    lines = _read(folder / INDEX).splitlines()
    check("which no longer lists it", (any(x.startswith("- C-Q1:") for x in lines),
                                       f"{STAMP}2026-10-10 13:00." in lines), (False, True))

    _failed(tree, "git cat-file --batch: broken", dt.datetime(2026, 10, 10, 14, 0))
    lines = _read(folder / INDEX).splitlines()
    check("a failure is said at the top",
          (lines[0], lines[1].startswith(f"{FAILED} (2026-10-10 14:00): git cat-file")),
          ("# Questions waiting on you", True))
    check("over the last good index", "- **C-Q4** (C-Q4.md): [C] Answered" in "\n".join(lines),
          True)
    check("and goes at the next good refresh", write(tree, build(tree), later), [INDEX])
    check("for good", FAILED in _read(folder / INDEX), False)

    _remove(folder / INDEX)
    (folder / INDEX).mkdir()
    try:
        write(tree, build(tree), later)
        check("an index that cannot be written fails the refresh", False, True)
    except OSError as e:
        check("an index that cannot be written fails the refresh", INDEX in str(e), True)
    (folder / INDEX).rmdir()
    try:
        _git(["rev-parse", "--verify", "--quiet", "refs/heads/no-such-branch"], tree)
        check("a failed git command raises", False, True)
    except GitError:
        pass


def _self_test() -> int:
    """The fixture's checks, and a count of the failures."""
    failures: list[str] = []

    def check(what: str, got, want) -> None:
        if got != want:
            failures.append(f"{what}: got {got!r}, want {want!r}")

    gitenv = SIGNALS._gitenv()
    if gitenv is None:
        print("questions-for-you self-test: scripts/gitenv.py cannot be loaded")
        return 1
    tmp = Path(tempfile.mkdtemp(prefix="questions-for-you-"))
    try:
        _fixture(tmp / "os", check)
    finally:
        gitenv.remove_tree(tmp)
    for f in failures:
        print(f"FAIL {f}")
    print(f"questions-for-you self-test: {'FAIL' if failures else 'ok'} "
          f"({len(failures)} failure(s))")
    return 1 if failures else 0


def _report(tree: Path, error: Exception, now: dt.datetime, in_index: bool) -> int:
    """A failed refresh, on stderr and, when `in_index`, at the top of the
    index; the exit status for it."""
    print(f"questions-for-you: {error}", file=sys.stderr)
    if in_index:
        try:
            _failed(tree, str(error), now)
        except OSError:
            pass  # The index itself cannot be written: stderr is all there is.
    return 1


def main(argv: list[str]) -> int:
    sys.path.insert(0, str(HERE))
    import selftestflag  # noqa: E402 -- scripts/ is put on the path just above

    unknown = selftestflag.unknown_options(argv, known=("--print",))
    if unknown:
        print(f"questions-for-you: unrecognised option {unknown[0]!r}", file=sys.stderr)
        return 2
    if selftestflag.wants_selftest(argv):
        return _self_test()
    tree = SIGNALS.integration_tree(HERE)
    if tree is None:
        print("questions-for-you: no integration tree found", file=sys.stderr)
        return 1
    now = dt.datetime.now()
    try:
        view = build(tree)
    except (GitError, OSError) as e:
        return _report(tree, e, now, "--print" not in argv)
    if "--print" in argv:
        sys.stdout.buffer.write(index(view, now).encode("utf-8"))
        return 0
    try:
        changed = write(tree, view, now)
    except OSError as e:
        return _report(tree, e, now, True)
    print(f"questions-for-you: {len(view.waiting)} waiting on the operator, "
          f"{len(view.answered)} answered and not yet recorded"
          + (f"; changed {', '.join(changed)}" if changed else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
