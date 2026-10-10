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
waiting on the operator, and an index, `_index.md`. Nothing here waits on the
question's own lane:

* each lane's questions are its files as its own branch has them (`lane-x`,
  the local branch every worktree shares, which holds what the lane has
  committed even before it pushes): a question a lane has just written shows
  at once, and one it has recorded (deleted) is gone;
* a question the operator has answered drops out -- answered in an answers
  file in the integration tree, or recorded or relayed in
  `operator-answers/LEDGER.md` on any lane's branch or main -- unless its lane
  has changed the question since, which is how a lane asks again;
* the answered ones no lane has recorded yet are listed in the index by
  lane, so nothing leaves the operator's sight unremarked.

It writes nothing but that folder, which it tells git to ignore
(`.git/info/exclude`, shared by every worktree). A file is rewritten only
when its text changes and the folder is never removed, so a window open on
it stays where it is; a file held open elsewhere is left for the next run.
"""

from __future__ import annotations

import datetime as dt
import importlib.util
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
LANES = "ABCDEF"
VIEW = "questions-for-you"
INDEX = "_index.md"
#: The index's one line that changes with time alone: whether the index
#: needs rewriting is judged without it.
STAMP = "Last changed "
LEDGER = "operator-answers/LEDGER.md"
#: The operator's answers files in the integration tree, as
#: `check-lane-signals.py` finds them. `claude-answers.txt` there is a lane's
#: reply to the operator, not answers, and is left out.
ANSWERS_GLOBS = ("open-questions-answers*.txt", "open-questions/*answers*.txt")
QUESTION_FILE = re.compile(r"^([A-F])-Q([0-9]+)\.md$")
QUESTION_ID = re.compile(r"\b([A-F]-Q[0-9]+)\b")
#: An answer keyed by its question's id at the start of a line: `B-Q23: E`.
ANSWER_LINE = re.compile(r"^[ \t]*([A-Fa-f]-[Qq][0-9]+)[ \t]*:", re.MULTILINE)
DATE = re.compile(r"\b(20[0-9]{2})-([0-9]{2})-([0-9]{2})\b")


def _gitenv():
    """`scripts/gitenv.py`, for an environment that names no repository."""
    spec = importlib.util.spec_from_file_location("gitenv", HERE / "gitenv.py")
    if spec is None or spec.loader is None:
        raise SystemExit("questions-for-you: scripts/gitenv.py cannot be loaded")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _env(extra: dict[str, str] | None = None) -> dict[str, str]:
    env = _gitenv().clean_env(dict(os.environ))
    env.update(extra or {})
    return env


def _git(args: list[str], cwd: Path, data: str | None = None) -> str:
    """`git <args>` in `cwd`'s repository -- never one an inherited `GIT_DIR`
    names -- its output as text; "" when it fails."""
    r = subprocess.run(["git", *args], cwd=str(cwd), env=_env(), input=data,
                       capture_output=True, text=True, encoding="utf-8",
                       errors="replace", check=False)
    return r.stdout if r.returncode == 0 else ""


def integration_tree(start: Path) -> Path | None:
    """The repository's main worktree, found from any of its worktrees."""
    common = _git(["rev-parse", "--path-format=absolute", "--git-common-dir"], start).strip()
    if not common:
        return None
    tree = Path(common).parent
    return tree if (tree / ".git").is_dir() else None


def lane_ref(tree: Path, lane: str) -> str | None:
    """The branch holding lane `lane`'s questions: its own, local first."""
    for ref in (f"refs/heads/lane-{lane.lower()}", f"refs/remotes/origin/lane-{lane.lower()}"):
        if _git(["rev-parse", "--verify", "--quiet", ref], tree).strip():
            return ref
    return None


def read_blobs(tree: Path, names: list[str]) -> dict[str, str]:
    """The text of each `rev:path` in `names` that exists, by name, from one
    `git cat-file --batch`."""
    if not names:
        return {}
    r = subprocess.run(["git", "cat-file", "--batch"], cwd=str(tree), env=_env(),
                       input="".join(f"{n}\n" for n in names).encode("utf-8"),
                       capture_output=True, check=False)
    out, at, found = r.stdout, 0, {}
    for name in names:
        end = out.find(b"\n", at)
        if end < 0:
            break
        header = out[at:end].decode("utf-8", "replace").split()
        at = end + 1
        if len(header) < 3 or header[1] == "missing":
            continue
        size = int(header[2])
        found[name] = out[at:at + size].decode("utf-8", "replace")
        at += size + 1
    return found


def changed_times(tree: Path, ref: str, since: int) -> dict[str, int]:
    """When each question file of `ref` changed there last, if it has since
    `since`: commit times, from one `git log` over `open-questions/` that
    goes back no further -- the whole history is many thousands of commits,
    and only a change after an answer matters."""
    times: dict[str, int] = {}
    stamp = 0
    for line in _git(["log", f"--since=@{since}", "--format=@%ct", "--name-only", ref, "--",
                      "open-questions/"], tree).splitlines():
        if line.startswith("@"):
            stamp = int(line[1:] or 0)
        elif line.strip():
            times.setdefault(Path(line.strip()).name, stamp)
    return times


def _day_end(text: str) -> int | None:
    """The end of the first date in `text` -- the next day's start, local
    time -- as a timestamp."""
    m = DATE.search(text)
    if not m:
        return None
    y, mo, d = (int(g) for g in m.groups())
    try:
        return int((dt.datetime(y, mo, d) + dt.timedelta(days=1)).timestamp())
    except (ValueError, OverflowError):
        return None


def ledger_answers(text: str) -> dict[str, int]:
    """The questions a ledger's rows record or relay as answered, each with
    the end of its section's day (the section's first date). A row that says
    the question is not answered, or still waits on the operator, is not an
    answer.

    A ledger dates by the day alone, so an answer counts as given at that
    day's close: a question its lane wrote that morning is answered by it. A
    lane asking again the same day says so in the row as well ("waits on
    you"), as the answers process has it."""
    found: dict[str, int] = {}
    day = None
    for line in text.splitlines():
        if line.startswith("## "):
            day = None
        if day is None and not line.startswith("|"):
            day = _day_end(line)
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4:
            continue
        state = cells[3].replace("*", "").lower()
        if "not answered" in state or "waits on you" in state:
            continue
        if "recorded" not in state and "relayed" not in state:
            continue
        for qid in QUESTION_ID.findall(cells[0]):
            found[qid] = max(found.get(qid, 0), day or 0)
    return found


def file_answers(tree: Path) -> dict[str, int]:
    """The questions the answers files in the integration tree answer, each
    with when its file was last written."""
    found: dict[str, int] = {}
    for pattern in ANSWERS_GLOBS:
        for path in tree.glob(pattern):
            if path.name.lower().startswith("claude"):
                continue
            text = path.read_bytes().decode("utf-8", "replace")
            when = int(path.stat().st_mtime)
            for m in ANSWER_LINE.finditer(text):
                qid = m.group(1).upper()
                found[qid] = max(found.get(qid, 0), when)
    return found


@dataclass
class Question:
    qid: str
    lane: str
    text: str
    changed: int


@dataclass
class View:
    waiting: list[Question]
    answered: dict[str, list[str]]


def build(tree: Path) -> View:
    """What the folder is to hold: each lane's questions as its branch has
    them, split into those waiting on the operator and those answered."""
    refs = {lane: lane_ref(tree, lane) for lane in LANES}
    listed: list[tuple[str, str, str]] = []
    for lane, ref in refs.items():
        if ref is None:
            continue
        for name in _git(["ls-tree", "--name-only", ref, "open-questions/"], tree).splitlines():
            base = Path(name).name
            m = QUESTION_FILE.match(base)
            if m and m.group(1) == lane:
                listed.append((base[:-3], lane, f"{ref}:open-questions/{base}"))
    ledger_refs = ["refs/heads/main", "refs/remotes/origin/main",
                   *(r for r in refs.values() if r)]
    blobs = read_blobs(tree, [q[2] for q in listed] + [f"{r}:{LEDGER}" for r in ledger_refs])
    answers = file_answers(tree)
    for r in ledger_refs:
        for qid, day in ledger_answers(blobs.get(f"{r}:{LEDGER}", "")).items():
            answers[qid] = max(answers.get(qid, 0), day)
    # Which answered questions their lanes have changed since: only from the
    # earliest of their answers on, lane by lane.
    changed: dict[str, int] = {}
    for lane, ref in refs.items():
        since = [answers[q] for q, ql, _ in listed if ql == lane and q in answers]
        if ref is not None and since:
            for base, when in changed_times(tree, ref, min(since)).items():
                changed[base[:-3]] = when
    view = View(waiting=[], answered={})
    for qid, lane, name in sorted(listed, key=lambda q: _order(q[0])):
        if name not in blobs:
            continue
        if qid in answers and changed.get(qid, 0) <= answers[qid]:
            view.answered.setdefault(lane, []).append(qid)
        else:
            view.waiting.append(Question(qid, lane, blobs[name], changed.get(qid, 0)))
    return view


def _order(qid: str) -> tuple[str, int]:
    lane, _, n = qid.partition("-Q")
    return lane, int(n)


def title(text: str) -> str:
    """A question's heading, without its markdown."""
    for line in text.splitlines():
        if line.startswith("#"):
            return line.lstrip("#").strip()
    return "(no heading)"


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


def index(view: View, now: dt.datetime) -> str:
    """The folder's `_index.md`."""
    out = [
        "# Questions waiting on you",
        "",
        "Each other file here is one question still waiting on you, as its lane last wrote",
        "it. `scripts/questions-for-you.py` rebuilds the folder from every lane's branch and",
        "your answers files: a question you answer drops out the next time it runs, whether",
        "or not its lane is running, and a new one shows as soon as its lane writes it. The",
        "lanes' own records are in `../open-questions/`.",
        "",
        f"{STAMP}{now:%Y-%m-%d %H:%M}.",
        "",
        f"## Waiting on you ({len(view.waiting)})",
        "",
    ]
    if not view.waiting:
        out.append("Nothing.")
    for q in view.waiting:
        out.append(f"- **{q.qid}** ({q.qid}.md): {title(q.text)}")
        summary = in_short(q.text)
        if summary:
            out.append(f"  {summary}")
        out.append("")
    answered = sum(len(v) for v in view.answered.values())
    out += ["", f"## Answered, waiting for their lanes to record them ({answered})", ""]
    if not answered:
        out.append("None.")
    for lane in LANES:
        if view.answered.get(lane):
            out.append(f"- Lane {lane}: {', '.join(view.answered[lane])}")
    return "\n".join(out).rstrip() + "\n"


def write(tree: Path, view: View, now: dt.datetime) -> list[str]:
    """The folder brought to `view`: each file written whose text differs,
    each question file no longer waiting removed. Returns what changed."""
    folder = tree / VIEW
    folder.mkdir(exist_ok=True)
    wanted = {f"{q.qid}.md": q.text for q in view.waiting}
    changed = []
    for name, text in wanted.items():
        if _put(folder / name, text):
            changed.append(name)
    for old in folder.iterdir():
        if QUESTION_FILE.match(old.name) and old.name not in wanted:
            try:
                old.unlink()
                changed.append(f"-{old.name}")
            except OSError:
                pass  # Held open elsewhere; the next run removes it.
    text = index(view, now)
    if _unstamped(_read(folder / INDEX)) != _unstamped(text) and _put(folder / INDEX, text):
        changed.append(INDEX)
    _ignore(tree)
    return changed


def _unstamped(text: str) -> list[str]:
    return [line for line in text.splitlines() if not line.startswith(STAMP)]


def _read(path: Path) -> str:
    """`path`'s text; "" when it is missing or cannot be read."""
    try:
        return path.read_bytes().decode("utf-8", "replace")
    except OSError:
        return ""


def _put(path: Path, text: str) -> bool:
    data = text.encode("utf-8")
    try:
        if path.exists() and path.read_bytes() == data:
            return False
        path.write_bytes(data)
        return True
    except OSError:
        return False  # Held open elsewhere; the next run writes it.


def _ignore(tree: Path) -> None:
    """`/questions-for-you/` in `.git/info/exclude`, once."""
    exclude = tree / ".git" / "info" / "exclude"
    line = f"/{VIEW}/"
    try:
        text = exclude.read_text(encoding="utf-8") if exclude.exists() else ""
        if line not in text.splitlines():
            exclude.parent.mkdir(parents=True, exist_ok=True)
            sep = "" if not text or text.endswith("\n") else "\n"
            exclude.write_text(f"{text}{sep}{line}\n", encoding="utf-8", newline="\n")
    except OSError:
        pass  # Untracked either way; git status only shows it.


def _self_test() -> int:
    """A throwaway repository: lanes B and C with questions, an answers file,
    a ledger, and a question changed after its answer."""
    failures = []

    def check(what: str, got, want) -> None:
        if got != want:
            failures.append(f"{what}: got {got!r}, want {want!r}")

    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp) / "os"
        tree.mkdir()

        def git(*args: str, when: str | None = None) -> None:
            extra = {"GIT_AUTHOR_DATE": when, "GIT_COMMITTER_DATE": when} if when else {}
            r = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t",
                                "-c", "commit.gpgsign=false", *args], cwd=str(tree),
                               env=_env(extra), capture_output=True, text=True, check=False)
            if r.returncode != 0:
                raise SystemExit(f"self-test git {args}: {r.stderr}")

        def put(rel: str, text: str) -> None:
            p = tree / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8", newline="\n")

        git("init", "-q", "-b", "main")
        put("open-questions/README.md", "rules\n")
        put("open-questions/B-Q1.md", "## B-Q1 -- [B] Answered in the file\n\n**In short:** one.\n")
        put("open-questions/B-Q2.md", "## B-Q2 -- [B] Still open\n\n**In short:** two,\nwrapped.\n\nmore\n")
        git("add", "-A")
        git("commit", "-q", "-m", "b", when="2026-10-01T12:00:00")
        git("branch", "lane-b")
        put("open-questions/C-Q1.md", "## C-Q1 -- [C] Relayed in the ledger\n")
        put("open-questions/C-Q2.md", "## C-Q2 -- [C] Asked again after its answer\n")
        put("open-questions/C-Q3.md", "## C-Q3 -- [C] The ledger says it waits\n")
        put(LEDGER, "# ledger\n\n## open-questions/answers.txt\n\n- **Written:** 2026-10-05\n\n"
            "| Answer | Lane | Recorded in | State |\n|---|---|---|---|\n"
            "| C-Q1 | C | -- | relayed 2026-10-05 |\n| C-Q2 | C | -- | relayed 2026-10-05 |\n"
            "| C-Q3 | C | -- | **not answered** in this file |\n"
            "| C-Q4 | C | -- | relayed 2026-10-05 |\n")
        git("add", "-A")
        git("commit", "-q", "-m", "c", when="2026-10-02T12:00:00")
        git("branch", "lane-c")
        git("checkout", "-q", "lane-c")
        # Written the afternoon of the day the ledger dates its answer.
        put("open-questions/C-Q4.md", "## C-Q4 -- [C] Asked and answered the same day\n")
        git("add", "-A")
        git("commit", "-q", "-m", "c same day", when="2026-10-05T15:00:00")
        put("open-questions/C-Q2.md", "## C-Q2 -- [C] Asked again after its answer\n\nYour answer raised this.\n")
        git("add", "-A")
        git("commit", "-q", "-m", "c again", when="2026-10-07T12:00:00")
        git("checkout", "-q", "main")
        put("open-questions/answers.txt", "B-Q1: A\n")
        put("open-questions/claude-answers.txt", "B-Q2: a lane's remark, not an answer\n")
        view = build(tree)
        check("waiting", [q.qid for q in view.waiting], ["B-Q2", "C-Q2", "C-Q3"])
        check("answered", view.answered, {"B": ["B-Q1"], "C": ["C-Q1", "C-Q4"]})
        check("in short", in_short(view.waiting[0].text), "**In short:** two, wrapped.")
        now = dt.datetime(2026, 10, 10, 12, 0)
        put(f"{VIEW}/B-Q1.md", "stale\n")
        changed = write(tree, view, now)
        check("written", sorted(changed), sorted(["-B-Q1.md", "B-Q2.md", "C-Q2.md", "C-Q3.md", INDEX]))
        check("the folder", sorted(p.name for p in (tree / VIEW).iterdir()),
              sorted([INDEX, "B-Q2.md", "C-Q2.md", "C-Q3.md"]))
        check("C-Q2 as its lane has it", (tree / VIEW / "C-Q2.md").read_text(encoding="utf-8"),
              "## C-Q2 -- [C] Asked again after its answer\n\nYour answer raised this.\n")
        text = (tree / VIEW / INDEX).read_text(encoding="utf-8")
        check("the index lists the waiting", "**B-Q2**" in text and "**C-Q3**" in text, True)
        check("and the answered", "- Lane B: B-Q1" in text and "- Lane C: C-Q1" in text, True)
        check("a second run changes nothing", write(tree, build(tree), now), [])
        exclude = (tree / ".git" / "info" / "exclude").read_text(encoding="utf-8")
        check("ignored once", exclude.splitlines().count(f"/{VIEW}/"), 1)
        write(tree, build(tree), now)
        exclude = (tree / ".git" / "info" / "exclude").read_text(encoding="utf-8")
        check("and still once", exclude.splitlines().count(f"/{VIEW}/"), 1)
        # Lane C records C-Q1: no question file here changes, but the index must.
        git("checkout", "-q", "lane-c")
        git("rm", "-q", "open-questions/C-Q1.md")
        git("commit", "-q", "-m", "c records", when="2026-10-08T12:00:00")
        git("checkout", "-q", "main")
        later = dt.datetime(2026, 10, 10, 13, 0)
        check("a recorded answer rewrites only the index", write(tree, build(tree), later), [INDEX])
        text = (tree / VIEW / INDEX).read_text(encoding="utf-8")
        check("which no longer lists it", ("C-Q1" in text, "- Lane C: C-Q4" in text), (False, True))
        check("and is stamped with the change", f"{STAMP}2026-10-10 13:00" in text, True)
        check("git status leaves it out",
              _git(["status", "--porcelain", "--untracked-files=all"], tree).count(VIEW), 0)
        check("nothing else in the tree touched",
              sorted(p.name for p in tree.iterdir() if p.name != ".git"),
              ["open-questions", "operator-answers", VIEW])

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
    tree = integration_tree(HERE)
    if tree is None:
        print("questions-for-you: no integration tree found", file=sys.stderr)
        return 1
    view = build(tree)
    now = dt.datetime.now()
    if "--print" in argv:
        sys.stdout.buffer.write(index(view, now).encode("utf-8"))
        return 0
    changed = write(tree, view, now)
    print(f"questions-for-you: {len(view.waiting)} waiting on the operator, "
          f"{sum(len(v) for v in view.answered.values())} answered and not yet recorded"
          + (f"; changed {', '.join(changed)}" if changed else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
