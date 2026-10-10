#!/usr/bin/env python3
"""Rebuilds `questions-for-you/`: the operator's questions not yet answered.

A folder in the integration tree holding only the questions still waiting on
the operator, rebuilt from every lane's branch and the operator's answers.

    python scripts/questions-for-you.py            rebuild the folder
    python scripts/questions-for-you.py --print    show its index, write nothing
    python scripts/questions-for-you.py --selftest

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
* Where branches disagree about a row, the question's own lane has the last
  word -- it is the lane that records the answer or asks again -- then main,
  then whichever branch says the question still waits.
* Of a question's rows, the newest section's decides: by the section's first
  date, then by how high in the ledger it sits (newest first, by its rule).
* A section about a lane's reply to the operator (`claude-answers.txt`)
  answers nothing, whatever its rows say: lane A's account relayed to lane D
  is not the operator choosing.

An answers file in the integration tree that no ledger records yet -- one the
operator has just written, or added to -- answers what it names at once
(keyed by id or by title, as `check-lane-signals.py` reads them), unless the
ledger has a newer word on that question. Its lines that a ledger section
about the same file already has rows for were processed before, and the
ledger decides those. A question nothing answers is waiting.

The folder
----------
It writes nothing but that folder, which it tells git to ignore
(`.git/info/exclude`, shared by every worktree). Each copy opens with a line
saying it is one and where answers go, and is read-only, since an edit there
would be lost at the next refresh; a file in the folder that is not the
script's own is left alone and named in the index, because nothing reads it.
A file is rewritten only when its text changes (the index's "Last changed"
line aside), and a failed refresh says so at the top of the index rather than
leaving the last good one looking current.

What runs it is the operator's choice; until it is made, it runs by hand.
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
DATE = re.compile(r"\b(20[0-9]{2})-([0-9]{2})-([0-9]{2})\b")
#: A lane's reply to the operator, saved beside the answers files: no answers.
LANE_REPLY = re.compile(r"\bclaude[^\s/]*\.txt\b", re.IGNORECASE)


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
#: one: their names, the ledger's hash of each, the answer lines in them --
#: and its git environment, clean of any `GIT_DIR` a hook exports.
SIGNALS = _load("check_lane_signals", "check-lane-signals.py")
LEDGER = SIGNALS.ANSWERS_LEDGER


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
    """Each `rev:path` of `names` that exists, from one `git cat-file --batch`."""
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


def tree_entries(obj: Object) -> list[str]:
    """The names in a tree object: each entry is its mode, a space, its name,
    a NUL, and an object id."""
    names, at = [], 0
    while at < len(obj.data):
        space = obj.data.index(b" ", at)
        nul = obj.data.index(b"\0", space)
        names.append(obj.data[space + 1:nul].decode("utf-8", "replace"))
        at = nul + 1 + obj.id_size
    return names


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
    day: int
    rows: dict[str, Row] = field(default_factory=dict)


def _day(text: str) -> int:
    """The start of the first date in `text`, local time, as a timestamp; 0
    if it has none."""
    m = DATE.search(text)
    if not m:
        return 0
    try:
        return int(dt.datetime(*(int(g) for g in m.groups())).timestamp())
    except (ValueError, OverflowError, OSError):
        return 0


def sections(text: str) -> list[Section]:
    """A ledger's sections, top -- the newest -- to bottom, each with what its
    rows say of each question. A section about a lane's reply keeps none."""
    built: list[tuple[Section, dict[str, Row], dict[str, Row]]] = []
    for line in text.splitlines():
        if line.startswith("## "):
            built.append((Section(key="", heading=line[3:].strip(), day=_day(line)), {}, {}))
            continue
        if not built:
            continue
        sec, owns, named = built[-1]
        if not line.startswith("|"):
            sec.day = sec.day or _day(line)
            m = None if sec.key else SIGNALS._LEDGER_SHA256.search(line)
            if m:
                sec.key = m.group(1)
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4 or LANE_REPLY.search(sec.heading):
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
    #: Each section's place in time: its day, then its height in the ledgers
    #: that hold it -- how many sections sit below it, at most.
    rank: dict[str, tuple[int, int]]
    #: Every answers-file hash any branch's ledger records.
    recorded: set[str]


def read_ledgers(texts: dict[str, str]) -> Ledgers:
    """`texts`, each branch's ledger by the branch's name, read."""
    by_branch = {name: sections(text) for name, text in texts.items()}
    rank: dict[str, tuple[int, int]] = {}
    for secs in by_branch.values():
        for i, sec in enumerate(secs):
            day, height = rank.get(sec.key, (0, -1))
            rank[sec.key] = (max(day, sec.day), max(height, len(secs) - 1 - i))
    return Ledgers(by_branch, rank, SIGNALS.recorded_keys(list(texts.values())))


def ledger_word(ledgers: Ledgers, qid: str, lane: str) -> tuple[tuple[int, int], Row] | None:
    """The ledger's last word on `qid`, a question of lane `lane`, and the
    rank of the section it is in; None if no row names it. The newest
    section's row, as the question's own lane has it, else main, else
    whichever branch says the question still waits."""
    said: dict[str, dict[str, Row]] = {}
    for branch, secs in ledgers.by_branch.items():
        for sec in secs:
            if qid in sec.rows:
                said.setdefault(sec.key, {})[branch] = sec.rows[qid]
    if not said:
        return None
    key = max(said, key=lambda k: ledgers.rank[k])
    rows = said[key]
    for branch in (lane, "main", "origin/main"):
        if branch in rows:
            return ledgers.rank[key], rows[branch]
    waiting = [r for r in rows.values() if not r.answered]
    return ledgers.rank[key], (waiting or list(rows.values()))[0]


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


def new_answers(tree: Path, ledgers: Ledgers, listed: list[Question]) -> dict[str, tuple[int, str]]:
    """The questions an answers file in `tree` answers that no ledger records
    yet, each with when the file was last written and its name -- leaving out
    the lines a ledger section about the same file has rows for, which were
    processed before."""
    found: dict[str, tuple[int, str]] = {}
    for path in sorted({p for pattern in SIGNALS.ANSWERS_GLOBS for p in tree.glob(pattern)}):
        rel = path.relative_to(tree).as_posix()
        if LANE_REPLY.search(rel):
            continue
        try:
            data = path.read_bytes()
            when = int(path.stat().st_mtime)
        except OSError:
            continue  # Gone, or held: the next refresh reads it.
        if SIGNALS.answers_key(data) in ledgers.recorded:
            continue
        before = {q for secs in ledgers.by_branch.values() for sec in secs
                  if rel in sec.heading for q in sec.rows}
        text = data.decode("utf-8", "replace")
        qids = [m.group(1) for m in SIGNALS._ANSWER_ID.finditer(text) if "-" in m.group(1)]
        qids += [q for q in (by_title(m, listed) for m in SIGNALS._ANSWER_TITLED.finditer(text))
                 if q]
        for qid in qids:
            if qid not in before and found.get(qid, (-1, ""))[0] < when:
                found[qid] = (when, rel)
    return found


@dataclass
class View:
    """What the folder is to hold."""

    waiting: list[Question]
    #: Answered, and still among their lane's questions.
    answered: list[Question]
    #: Files in the folder that are not this script's.
    stray: list[str]


def build(tree: Path) -> View:
    """Each lane's questions as its branch has them, split into those waiting
    on the operator and those answered."""
    refs = branch_refs(tree)
    lanes = [lane for lane in LANES if lane in refs]
    first = read_objects(tree, [f"{refs[lane]}:open-questions" for lane in lanes]
                         + [f"{ref}:{LEDGER}" for ref in refs.values()])
    wanted = []
    for lane in lanes:
        folder = first.get(f"{refs[lane]}:open-questions")
        for name in tree_entries(folder) if folder and folder.kind == "tree" else []:
            m = QUESTION_FILE.match(name)
            if m and m.group(1) == lane:
                wanted.append((name[:-3], lane, f"{refs[lane]}:open-questions/{name}"))
    blobs = read_objects(tree, [w[2] for w in wanted])
    listed = [Question(qid, lane, blobs[name].data.decode("utf-8", "replace"))
              for qid, lane, name in sorted(wanted, key=lambda w: _order(w[0])) if name in blobs]
    ledgers = read_ledgers({branch: first[f"{ref}:{LEDGER}"].data.decode("utf-8", "replace")
                            for branch, ref in refs.items() if f"{ref}:{LEDGER}" in first})
    fresh = new_answers(tree, ledgers, listed)
    view = View(waiting=[], answered=[], stray=strays(tree))
    for q in listed:
        word = ledger_word(ledgers, q.qid, q.lane)
        answered = word is not None and word[1].answered
        q.why = word[1].state if word else ""
        if q.qid in fresh and (word is None or fresh[q.qid][0] >= word[0][0]):
            answered, q.why = True, f"answered in `{fresh[q.qid][1]}`"
        (view.answered if answered else view.waiting).append(q)
    return view


def strays(tree: Path) -> list[str]:
    """The files in the folder that are not this script's."""
    try:
        return sorted(p.name for p in (tree / VIEW).iterdir()
                      if p.name != INDEX and not QUESTION_FILE.match(p.name))
    except OSError:
        return []


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
        out += ["**Nothing reads these files here:** "
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
    each question file no longer waiting removed. Returns what changed."""
    folder = tree / VIEW
    folder.mkdir(exist_ok=True)
    wanted = {f"{q.qid}.md": copy_text(q) for q in view.waiting}
    changed = [name for name, text in wanted.items() if _put(folder / name, text)]
    for old in sorted(folder.iterdir()):
        if QUESTION_FILE.match(old.name) and old.name not in wanted and _remove(old):
            changed.append(f"-{old.name}")
    text = index(view, now)
    if _unstamped(_read(folder / INDEX)) != _unstamped(text) and _put(folder / INDEX, text):
        changed.append(INDEX)
    _ignore(tree)
    return changed


def _failed(tree: Path, error: str, now: dt.datetime) -> None:
    """A failed refresh, said at the top of the index above the last good
    one -- which is otherwise left as it was."""
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
    """`path` holding `text`, read-only; whether it was written. A file held
    open elsewhere is left for the next refresh."""
    data = text.encode("utf-8")
    try:
        if path.exists():
            if path.read_bytes() == data:
                os.chmod(path, stat.S_IREAD)
                return False
            os.chmod(path, stat.S_IREAD | stat.S_IWRITE)
        path.write_bytes(data)
        os.chmod(path, stat.S_IREAD)
        return True
    except OSError:
        return False


def _remove(path: Path) -> bool:
    """`path` removed, read-only or not; whether it was."""
    try:
        os.chmod(path, stat.S_IREAD | stat.S_IWRITE)
        path.unlink()
        return True
    except OSError:
        return False  # Held open elsewhere; the next refresh removes it.


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


def _fixture(tree: Path, check) -> None:
    """The self-test's checks, in a throwaway repository at `tree`."""
    tree.mkdir()

    def git(*args: str, when: str | None = None) -> None:
        env = SIGNALS._clean_git_env()
        if when:
            env.update(GIT_AUTHOR_DATE=when, GIT_COMMITTER_DATE=when)
        r = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c",
                            "commit.gpgsign=false", *args], cwd=str(tree), env=env,
                           capture_output=True, text=True, check=False)
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
    ask("B-Q1", "Answered in a new version of an answers file")
    ask("B-Q2", "Still open, though a lane's reply names it")
    ask("B-Q3", "Answered by its title alone")
    git("add", "-A")
    git("commit", "-q", "-m", "b", when="2026-10-01T12:00:00")
    git("branch", "lane-b")
    for n, about in enumerate(["Relayed as answered", "Asked again by its own lane",
                               "Not answered, whatever a lane's reply says",
                               "Answered, then asked again in a newer section",
                               "Questions back, then answered in a newer section",
                               "Answered in a row that mentions another",
                               "Mentioned only in another question's row",
                               "Recorded, the rest carried by another", "Carrying the rest"],
                              start=1):
        ask(f"C-Q{n}", about)
    old = b"C-Q1: A\nC-Q3: I cannot decide\n"
    second = "C-Q4: which one?\nC-Q5: A\nC-Q7: A\n"
    table = "| Answer | Lane | Recorded in | State |\n|---|---|---|---|\n"
    reply = (f"## open-questions/claude-answers.txt\n\n- **Written:** 2026-10-06, a lane's reply.\n"
             f"- **Content:** sha256 `{'c' * 64}`\n\n{table}"
             "| C-Q3 | C | -- | relayed 2026-10-06 (lane A's account) |\n\n")
    # Newer than `first` the same day -- higher in the ledger -- and, like a
    # section a lane has not published yet, on lane C's branch alone.
    newer = (f"## open-questions/answers.2.txt\n\n- **Written:** 2026-10-05 16:15\n"
             f"- **Content:** sha256 `{SIGNALS.answers_key(second.encode())}`\n\n{table}"
             "| C-Q4 | C | -- | relayed 2026-10-05; **asked again: which one?** |\n"
             "| C-Q5 | C | -- | relayed 2026-10-05 (A) |\n\n")
    first = (f"## open-questions/answers.txt\n\n- **Written:** 2026-10-05\n"
             f"- **Content:** sha256 `{SIGNALS.answers_key(old)}`\n\n{table}"
             "| C-Q1 | C | -- | relayed 2026-10-05 (A) |\n"
             "| C-Q2 | C | -- | relayed 2026-10-05 (B) |\n"
             "| C-Q3 | C | -- | **not answered** in this file |\n"
             "| C-Q4 | C | -- | relayed 2026-10-05 (A) |\n"
             "| C-Q5 | C | -- | relayed 2026-10-05; **questions back, no option chosen yet** |\n"
             "| C-Q6: A -- the line read as C-Q7's was C-Q6's | C | -- | relayed 2026-10-05 |\n"
             "| C-Q8 | C | §1 | recorded; **the rest waits on you as C-Q9** |\n")
    put(LEDGER, f"# ledger\n\n{reply}{first}")
    git("add", "-A")
    git("commit", "-q", "-m", "c", when="2026-10-02T12:00:00")
    git("branch", "lane-c")
    git("checkout", "-q", "lane-c")
    put("open-questions/C-Q2.md", "## C-Q2 — [C] Asked again by its own lane\n\nAsked again.\n")
    put(LEDGER, f"# ledger\n\n{reply}{newer}" + first.replace(
        "relayed 2026-10-05 (B) |", "relayed 2026-10-05; **asked again after your answer** |"))
    git("add", "-A")
    git("commit", "-q", "-m", "c asks again", when="2026-10-07T12:00:00")
    git("checkout", "-q", "main")
    (tree / "open-questions" / "answers.txt").write_bytes(old + b"B-Q1: A\n")
    put("open-questions/answers.2.txt", second)
    put("open-questions/claude-answers.txt", "B-Q2: a lane's remark, not an answer\n")
    late = put("open-questions-answers.3.txt",
               "C-Q4: A\nAnswered by its title alone (lane B, 2026-10-01): yes\n")
    stamp = dt.datetime(2026, 10, 1, 12).timestamp()
    os.utime(late, (stamp, stamp))

    view = build(tree)
    check("waiting", [q.qid for q in view.waiting], ["B-Q2", "C-Q2", "C-Q3", "C-Q4", "C-Q7", "C-Q9"])
    check("answered", [q.qid for q in view.answered], ["B-Q1", "B-Q3", "C-Q1", "C-Q5", "C-Q6", "C-Q8"])
    why = {q.qid: q.why for q in view.waiting + view.answered}
    check("a new answer says where", why["B-Q1"], "answered in `open-questions/answers.txt`")
    check("the ledger's word", (why["C-Q5"], why["C-Q3"]),
          ("relayed 2026-10-05 (A)", "**not answered** in this file"))
    check("in short", in_short(view.waiting[0].text),
          "**In short:** still open, though a lane's reply names it.")
    check("title", title(view.waiting[0].text), "[B] Still open, though a lane's reply names it")

    folder = tree / VIEW
    put(f"{VIEW}/B-Q1.md", "left from an earlier refresh\n")
    put(f"{VIEW}/answers.txt", "C-Q9: A\n")
    view = build(tree)
    check("strays", view.stray, ["answers.txt"])
    now = dt.datetime(2026, 10, 10, 12, 0)
    check("written", sorted(write(tree, view, now)),
          sorted(["-B-Q1.md", "B-Q2.md", "C-Q2.md", "C-Q3.md", "C-Q4.md", "C-Q7.md", "C-Q9.md",
                  INDEX]))
    check("the folder", sorted(p.name for p in folder.iterdir()),
          sorted([INDEX, "answers.txt", "B-Q2.md", "C-Q2.md", "C-Q3.md", "C-Q4.md", "C-Q7.md",
                  "C-Q9.md"]))
    copy = _read(folder / "C-Q2.md")
    check("a copy says so, then holds its lane's text",
          (copy.startswith("> A copy of lane C's question"), copy.endswith("Asked again.\n")),
          (True, True))
    check("and is read-only", os.access(folder / "C-Q2.md", os.W_OK), False)
    text = _read(folder / INDEX)
    for what, line in (("lists the waiting", "- **C-Q3** (C-Q3.md): [C] Not answered"),
                       ("with the ledger's word", "  *The ledger:* **not answered** in this file"),
                       ("and the answered", "- C-Q5: relayed 2026-10-05 (A)"),
                       ("names a stray", "**Nothing reads these files here:** `answers.txt`")):
        check(f"the index {what}", any(x.startswith(line) for x in text.splitlines()), True)
    check("a second refresh changes nothing", write(tree, build(tree), now), [])
    exclude = tree / ".git" / "info" / "exclude"
    check("ignored once", exclude.read_bytes().splitlines().count(f"/{VIEW}/".encode()), 1)
    write(tree, build(tree), now)
    check("and still once", exclude.read_bytes().splitlines().count(f"/{VIEW}/".encode()), 1)
    status = _git(["status", "--porcelain", "--untracked-files=all"], tree).decode()
    check("git status shows the untracked answers but not the folder",
          ("open-questions/answers.txt" in status, VIEW in status), (True, False))

    # Lane C records C-Q1: no question file here changes, but the index must.
    git("checkout", "-q", "lane-c")
    git("rm", "-q", "open-questions/C-Q1.md")
    git("commit", "-q", "-m", "c records", when="2026-10-08T12:00:00")
    git("checkout", "-q", "main")
    later = dt.datetime(2026, 10, 10, 13, 0)
    check("a recorded answer rewrites only the index", write(tree, build(tree), later), [INDEX])
    text = _read(folder / INDEX)
    check("which no longer lists it", ("C-Q1" in text, f"{STAMP}2026-10-10 13:00" in text),
          (False, True))

    _failed(tree, "git cat-file --batch: broken", dt.datetime(2026, 10, 10, 14, 0))
    lines = _read(folder / INDEX).splitlines()
    check("a failure is said at the top",
          (lines[0], lines[1].startswith(f"{FAILED} (2026-10-10 14:00): git cat-file")),
          ("# Questions waiting on you", True))
    check("over the last good index", "- **C-Q3** (C-Q3.md): [C] Not answered" in "\n".join(lines),
          True)
    check("and goes at the next good refresh", write(tree, build(tree), later), [INDEX])
    check("for good", FAILED in _read(folder / INDEX), False)
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
        print(f"questions-for-you: {e}", file=sys.stderr)
        if "--print" not in argv:
            _failed(tree, str(e), now)
        return 1
    if "--print" in argv:
        sys.stdout.buffer.write(index(view, now).encode("utf-8"))
        return 0
    changed = write(tree, view, now)
    print(f"questions-for-you: {len(view.waiting)} waiting on the operator, "
          f"{len(view.answered)} answered and not yet recorded"
          + (f"; changed {', '.join(changed)}" if changed else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
