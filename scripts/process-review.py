#!/usr/bin/env python3
"""The week's numbers on how SlateOS is being built, and what to change first.

    python scripts/process-review.py                  the numbers, as Markdown
    python scripts/process-review.py --out DIR        written to DIR/process-review-<date>.md
    python scripts/process-review.py --out DIR --propose
                                                      ... then Claude names the three changes
                                                      with the largest measured payoff
    python scripts/process-review.py --self-test

Agents improve what they are pointed at, and nothing points them at the
process itself. This does, once a week, from records the project already
keeps (Task Scheduler runs it: see `--register`). It gathers:

- **Usage**, the binding constraint. Every account reaches its weekly
  limit in about half a week. The numbers come from the operator's
  usage analyzer (`claude-usage-analysis`, its `cua report`). From that
  analyzer's database and its usage samples, this fits how heavily the
  plan's meter weighs cache reads -- the share of the budget that longer
  contexts cost.
- **Boot tests:** how many, how many failed and how many of those were
  flaky, and gate time against boot time (`bench/boot-history.jsonl`).
- **Reviews:** how many ran at each tier, the findings fixed and dismissed,
  and the mutants no test caught (the review records' trailers, on main).
- **Backlog:** open known issues and requests, by lane, and how many have
  waited over a month.

Nothing here is a judgment. `--propose` adds one: a single Claude call
(`claude -p`, Opus at medium effort, a few tens of thousands of tokens a
week) reads the numbers and names the three changes with the largest
measured payoff.

Exit status: 0; 2 a usage error.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import sqlite3
import statistics
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE))
import gitenv  # noqa: E402
import selftestflag  # noqa: E402

#: The operator's usage analyzer: `python -m cua report` there.
CUA_DIR = Path(os.environ.get("CUA_DIR", "D:/visual studio projects/claude-usage-analysis"))
#: Seconds of usage summed per fitted window: long enough that the whole
#: points the meter reports in mean something, short enough to give many.
FIT_WINDOW = 2 * 3600
#: The scheduled task's name (`--register`).
TASK = "SlateOS process review"


def git(*args: str) -> str:
    r = subprocess.run(["git", "-c", "core.quotePath=false", *args], cwd=REPO,
                       env=gitenv.clean_env(), capture_output=True, text=True,
                       encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL)
    return r.stdout


# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------

def least_squares_2(rows: list[tuple[float, float, float]]) -> tuple[float, float] | None:
    """(a, b) minimising sum (y - a*x1 - b*x2)^2 over rows (y, x1, x2)."""
    s11 = sum(x1 * x1 for _, x1, _ in rows)
    s22 = sum(x2 * x2 for _, _, x2 in rows)
    s12 = sum(x1 * x2 for _, x1, x2 in rows)
    sy1 = sum(y * x1 for y, x1, _ in rows)
    sy2 = sum(y * x2 for y, _, x2 in rows)
    det = s11 * s22 - s12 * s12
    if abs(det) < 1e-12:
        return None
    return (sy1 * s22 - sy2 * s12) / det, (sy2 * s11 - sy1 * s12) / det


def meter_windows(samples: list[dict], calls: list[tuple[float, float, float]],
                  window: int) -> list[tuple[float, float, float]]:
    """(points used, cache-read $, other $) per window of at least `window`
    seconds between one account's samples, within one weekly period and below
    its cap. `samples` are the analyzer's records, `calls` (time, cache-read
    $, other $) sorted by time."""
    def resets(s: dict) -> float:
        return dt.datetime.fromisoformat(s["seven_day"]["resets_at"]).timestamp()

    good = sorted((s for s in samples if isinstance(s.get("seven_day"), dict)
                   and s["seven_day"].get("utilization") is not None
                   and s["seven_day"].get("resets_at")), key=lambda s: s["ts"])
    out = []
    if not good:
        return out
    start = good[0]
    for s in good[1:]:
        # The reset time is reported to the microsecond and jitters across
        # the hour: the same week if within an hour.
        if abs(resets(s) - resets(start)) > 3600:
            start = s
            continue
        if s["ts"] - start["ts"] < window:
            continue
        u0, u1 = start["seven_day"]["utilization"], s["seven_day"]["utilization"]
        if 0 <= u0 <= u1 < 100:
            read = sum(r for t, r, _ in calls if start["ts"] < t <= s["ts"])
            other = sum(o for t, _, o in calls if start["ts"] < t <= s["ts"])
            out.append((u1 - u0, read, other))
        start = s
    return out


def meter_fit(cua: Path) -> str:
    """How heavily the weekly meter weighs cache reads, from the analyzer's
    database and usage samples; a paragraph, or why there is none."""
    db_path, samples_path = cua / "state" / "cua.sqlite", cua / "state" / "utilization.jsonl"
    if not db_path.is_file() or not samples_path.is_file():
        return "No usage samples to fit (the analyzer's state is missing)."
    sys.path.insert(0, str(cua))
    try:
        from cua import config  # type: ignore[import-not-found]
    except ImportError:
        return "The usage analyzer could not be imported; no fit."
    samples: dict[str, list[dict]] = defaultdict(list)
    for line in samples_path.read_text(encoding="utf-8").splitlines():
        try:
            s = json.loads(line)
        except ValueError:
            continue
        samples[s.get("account")].append(s)
    since = min((s["ts"] for ss in samples.values() for s in ss), default=0)
    db = sqlite3.connect(f"file:{db_path.as_posix()}?mode=ro", uri=True)
    account_of = dict(db.execute("select path, account from files"))
    best: dict[tuple[str, str], tuple] = {}
    for path, req, ts, model, inp, outp, cw5, cw1h, cr in db.execute(
            "select path, req_id, ts, model, inp, outp, cw5, cw1h, cr from lines "
            "where ts > ? and req_id is not null", (since,)):
        row = (ts, model or "", inp or 0, outp or 0, cw5 or 0, cw1h or 0, cr or 0)
        prev = best.get((path, req))
        if prev is None or sum(row[2:]) > sum(prev[2:]):
            best[(path, req)] = row
    calls: dict[str, list[tuple[float, float, float]]] = defaultdict(list)
    for (path, _), (ts, model, inp, outp, cw5, cw1h, cr) in best.items():
        p_in, p_out, p_w5, p_w1h, p_r = config.price_for(model)
        calls[account_of.get(path)].append(
            (ts, cr * p_r / 1e6, (inp * p_in + outp * p_out + cw5 * p_w5 + cw1h * p_w1h) / 1e6))
    rows = []
    for account, ss in samples.items():
        rows += meter_windows(ss, sorted(calls.get(account, [])), FIT_WINDOW)
    fit = least_squares_2(rows)
    if len(rows) < 8 or fit is None or fit[1] <= 0:
        return f"Too few usage windows below the cap to fit ({len(rows)})."
    a, b = fit
    read = sum(r for _, r, _ in rows)
    other = sum(o for _, _, o in rows)
    share = a * read / (a * read + b * other) if a * read + b * other > 0 else 0
    return (f"From {len(rows)} two-hour windows below the cap: a dollar of cache reads "
            f"(at API price) moves the weekly meter {a / b:.2f} times as much as a dollar "
            f"of anything else, so cache reads are about {100 * share:.0f}% of what fills "
            f"the weekly limit. Cache reads grow with the context each call carries, so a "
            f"shorter context per call (compacting earlier) stretches the budget by about "
            f"that share times the fraction of context saved, less what the extra "
            f"compactions cost.")


def usage_section(cua: Path) -> str:
    if not (cua / "cua").is_dir():
        return "## Usage\n\nThe usage analyzer is not at `CUA_DIR`; no usage numbers.\n"
    r = subprocess.run([sys.executable, "-m", "cua", "report"], cwd=cua, capture_output=True,
                       text=True, encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL)
    m = re.search(r"report: (.+\.md)", r.stdout + r.stderr)
    answers = per_lane = ""
    if m and Path(m.group(1).strip()).is_file():
        text = Path(m.group(1).strip()).read_text(encoding="utf-8")
        answers = section(text, "## Answers")
        per_lane = section(text, "## 5.")
    return ("## Usage: the binding constraint\n\n"
            + (answers.replace("## Answers", "### The usage analyzer's findings", 1)
               or "The analyzer's report could not be read.\n")
            + "\n**How the meter weighs cache reads.** " + meter_fit(cua) + "\n\n"
            + (per_lane.replace("## 5.", "### Per lane:", 1) if per_lane else ""))


def section(text: str, heading: str) -> str:
    """The Markdown section that starts with `heading`, up to the next `## `."""
    at = text.find("\n" + heading)
    if at < 0:
        return ""
    end = text.find("\n## ", at + 1)
    return text[at + 1: end if end > 0 else len(text)].strip() + "\n"


# ---------------------------------------------------------------------------
# Boot tests
# ---------------------------------------------------------------------------

def boot_section(rows: list[dict], since: str) -> str:
    week = [r for r in rows if r.get("ts", "") >= since]
    if not week:
        return "## Boot tests\n\nNone this week.\n"
    by_tree: dict[tuple, list[dict]] = defaultdict(list)
    for r in week:
        by_tree[(r.get("src_digest"), r.get("profile"), r.get("sanitizer"))].append(r)
    fails = [r for r in week if r.get("verdict") != "PASS"]
    flaky = [r for r in fails if any(x.get("verdict") == "PASS" for x in
                                     by_tree[(r.get("src_digest"), r.get("profile"),
                                              r.get("sanitizer"))])]
    # Wall-clock seconds, as the history records them; a row without one
    # (a run whose gates never finished) is left out, not counted as 0.
    gates = [r["gates_seconds"] for r in week if isinstance(r.get("gates_seconds"), (int, float))]
    walls = [r["wall_seconds"] for r in week if isinstance(r.get("wall_seconds"), (int, float))]
    verdicts = Counter(r.get("verdict") for r in week)
    branches = Counter(r.get("branch") or "?" for r in week)

    def summary(xs: list[float]) -> str:
        if not xs:
            return "not recorded"
        return f"{sum(xs) / 3600:.1f} hours (median {statistics.median(xs) / 60:.0f} minutes a boot)"

    return (f"## Boot tests\n\n{len(week)} boots: "
            + ", ".join(f"{n} {v}" for v, n in verdicts.most_common())
            + f". Of the {len(fails)} that failed, {len(flaky)} were flaky (the same tree "
            f"passed on another run); the rest failed on trees that never passed. "
            f"Wall time in the gates: {summary(gates)}; in the boots: {summary(walls)}. "
            f"By branch: " + ", ".join(f"{b} {n}" for b, n in branches.most_common()) + ".\n")


def load_history(path: Path) -> list[dict]:
    """The boot history's records, skipping a line a killed run cut short or a
    merge interleaved (the file is merged with merge=union)."""
    rows = []
    if not path.is_file():
        return rows
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            r = json.loads(line)
        except ValueError:
            continue
        if isinstance(r, dict):
            rows.append(r)
    return rows


# ---------------------------------------------------------------------------
# Reviews
# ---------------------------------------------------------------------------

TRAILER = re.compile(r"^(Reviewed|Review-[A-Za-z-]+):[ \t]*(.*?)\s*$", re.M)


def review_section(log: str) -> str:
    """From `git log --format=%x1e%H%n%B` output of the week's main."""
    tiers: Counter = Counter()
    fixed = dismissed = missed = tested = 0
    unavailable = 0
    for piece in log.split("\x1e")[1:]:
        for para in piece.split("\n\n"):
            f = {m.group(1): m.group(2) for m in TRAILER.finditer(para)}
            if ".." not in f.get("Reviewed", ""):
                continue
            tiers[f.get("Review-tier", "?")] += 1
            m = re.match(r"\s*(\d+)\s+fixed\D+(\d+)\s+dismissed", f.get("Review-findings", ""))
            if m:
                fixed, dismissed = fixed + int(m.group(1)), dismissed + int(m.group(2))
            m = re.match(r"\s*(\d+) missed of (\d+) tested", f.get("Review-mutants", ""))
            if m:
                missed, tested = missed + int(m.group(1)), tested + int(m.group(2))
            unavailable += "unavailable" in f.get("Review-runs", "")
    if not tiers:
        return "## Reviews\n\nNo review records reached main this week.\n"
    return ("## Reviews\n\n" + ", ".join(f"{n} at {t}" for t, n in tiers.most_common())
            + f" reached main; {fixed} findings fixed, {dismissed} dismissed"
            + (f"; Fable could not review {unavailable} of them" if unavailable else "")
            + (f"; mutation testing tested {tested} mutants and {missed} survived"
               if tested else "")
            + ". (`review-batch.py --stats` breaks findings down by lane and writing level.)\n")


# ---------------------------------------------------------------------------
# Backlog
# ---------------------------------------------------------------------------

DATED = re.compile(r"(\d{4}-\d{2}-\d{2})")
#: A known issue's lane: its file name's first letter, or the second
#: field of a tech-debt entry (`TD-B-...`).
ISSUE_LANE = re.compile(r"^(?:TD-)?([A-F])-")
#: Or the lane its heading names, `### [B] ...`, which entries named by
#: subsystem (`TD-OILS-...`) still carry.
HEADING_LANE = re.compile(r"\[([A-F])\]|\(lane ([A-F])\b|\*\*Lane:\*\*\s*([A-F])\b")


def backlog_section(root: Path, today: dt.date) -> str:
    month_ago = (today - dt.timedelta(days=30)).isoformat()
    issues: Counter = Counter()
    old_issues: Counter = Counter()
    for p in sorted((root / "known-issues").glob("*.md")):
        if p.name == "README.md":
            continue
        # The directory is the status (known-issues/README.md): an entry
        # still here is open, whatever word its own stamp uses.
        head = p.read_text(encoding="utf-8", errors="replace")[:1500]
        first = head.splitlines()[0] if head else ""
        m = HEADING_LANE.search(first) or ISSUE_LANE.match(p.name) or HEADING_LANE.search(head)
        lane = next((g for g in m.groups() if g), "other") if m else "other"
        issues[lane] += 1
        first = head.splitlines()[0] if head else ""
        m = DATED.findall(first)
        if m and m[-1] < month_ago:
            old_issues[lane] += 1
    requests: Counter = Counter()
    old_requests: Counter = Counter()
    for p in sorted((root / "requests").glob("*.md")):
        head = p.read_text(encoding="utf-8", errors="replace")[:1200]
        if not re.search(r"\*\*Status:\*\*\s*open", head, re.I):
            continue
        parts = p.name.split("-")
        to_lanes = [c.upper() for c in parts[1]] if len(parts) > 2 else ["?"]
        for to in to_lanes:
            requests[to] += 1
        m = re.search(r"\*\*Filed:\*\*\s*(\d{4}-\d{2}-\d{2})", head)
        if m and m.group(1) < month_ago:
            for to in to_lanes:
                old_requests[to] += 1
    lines = ["## Backlog", "", "| | open known issues | of them over a month old | open "
             "requests to | of them over a month old |", "|---|---:|---:|---:|---:|"]
    for lane in sorted(set(issues) | set(requests)):
        lines.append(f"| {lane} | {issues[lane]} | {old_issues[lane]} | {requests[lane]} | "
                     f"{old_requests[lane]} |")
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# The report, and the proposals
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# Context per lane: what the context trial compares
# ---------------------------------------------------------------------------

LANE_TITLE = re.compile(r"^OS ([A-F])$")


def context_rows(db: sqlite3.Connection, since: float, price_for) -> dict[str, list[float]]:
    """Per lane, from the usage analyzer's database since `since` (epoch
    seconds): [API calls, context tokens summed over them, compactions, API
    $]. A session is a lane's by its title, `OS A` to `OS F`."""
    lane_of: dict[str, str] = {}
    for path, title in db.execute("select path, title from titles"):
        m = LANE_TITLE.match(title or "")
        if m:
            lane_of[path] = m.group(1)
    best: dict[tuple[str, str], tuple] = {}
    for path, req, model, inp, outp, cw5, cw1h, cr in db.execute(
            "select path, req_id, model, inp, outp, cw5, cw1h, cr from lines "
            "where ts > ? and req_id is not null", (since,)):
        if path not in lane_of:
            continue
        row = (model or "", inp or 0, outp or 0, cw5 or 0, cw1h or 0, cr or 0)
        if (path, req) not in best or sum(row[1:]) > sum(best[(path, req)][1:]):
            best[(path, req)] = row
    rows: dict[str, list[float]] = {}
    for (path, _), (model, inp, outp, cw5, cw1h, cr) in best.items():
        r = rows.setdefault(lane_of[path], [0, 0, 0, 0.0])
        p_in, p_out, p_w5, p_w1h, p_r = price_for(model)
        r[0] += 1
        r[1] += inp + cw5 + cw1h + cr
        r[3] += (inp * p_in + outp * p_out + cw5 * p_w5 + cw1h * p_w1h + cr * p_r) / 1e6
    for (path,) in db.execute("select path from prompts where kind = 'compact' and ts > ?",
                              (since,)):
        if path in lane_of:
            rows.setdefault(lane_of[path], [0, 0, 0, 0.0])[2] += 1
    return rows


def context_section(cua: Path, since: str) -> str:
    db_path = cua / "state" / "cua.sqlite"
    if not db_path.is_file():
        return "## Context per lane\n\nThe usage analyzer's database is missing.\n"
    sys.path.insert(0, str(cua))
    try:
        from cua import config  # type: ignore[import-not-found]
    except ImportError:
        return "## Context per lane\n\nThe usage analyzer could not be imported.\n"
    db = sqlite3.connect(f"file:{db_path.as_posix()}?mode=ro", uri=True)
    start = dt.datetime.fromisoformat(since).replace(tzinfo=dt.timezone.utc).timestamp()
    rows = context_rows(db, start, config.price_for)
    lines = ["## Context per lane",
             "",
             "What the context trial compares (`os/go.bat`: account c's lanes, E and F, "
             "compact at 300k tokens from 2026-10-12; the others at 950k). A shorter "
             "context should show as a smaller context per call and a lower cost per "
             "commit; forgetting, as more compactions, more cost per commit, and more "
             "review findings and failed boots (above) for the same lanes.",
             "",
             "| lane | API calls | context per call | compactions | API $ | commits | API $ per commit |",
             "|---|---:|---:|---:|---:|---:|---:|"]
    for lane in sorted(rows):
        calls, ctx, compactions, cost = rows[lane]
        commits = len(git("rev-list", "--no-merges", f"--since={since}",
                          f"refs/remotes/origin/lane-{lane.lower()}").split())
        per = f"{cost / commits:.2f}" if commits else "-"
        lines.append(f"| {lane} | {calls} | {ctx / calls / 1000:.0f}k | {compactions} | "
                     f"{cost:.0f} | {commits} | {per} |" if calls else
                     f"| {lane} | 0 | - | {compactions} | 0 | {commits} | - |")
    return "\n".join(lines) + "\n"


def report(today: dt.date, cua: Path) -> str:
    since = (today - dt.timedelta(days=7)).isoformat()
    history = REPO / "bench" / "boot-history.jsonl"
    rows = load_history(history)
    log = git("log", f"--since={since}", "--grep=^Reviewed: ", "--format=%x1e%H%n%B",
              "refs/remotes/origin/main")
    return "\n".join([
        f"# How SlateOS is being built: the week to {today.isoformat()}", "",
        usage_section(cua), context_section(cua, since), boot_section(rows, since),
        review_section(log), backlog_section(REPO, today)])


PROPOSE = """You are reviewing how an autonomous six-lane Claude development effort \
(SlateOS, a Rust microkernel OS) is being run. Below are this week's measured numbers. \
The binding constraint is the weekly usage limit of three Claude accounts, which every \
account reaches about halfway through each week; CPU time on the operator's one PC is a \
secondary cost. Name the three changes with the largest MEASURED payoff for the \
project's progress per week: for each, the number that shows it, the expected effect \
in the same units, who would make the change (a lane, A to F, or the operator), and \
what to measure next week to confirm it worked. Prefer changes the numbers support \
over general advice; if a number is missing that would decide something important, \
say which. Plain prose, at most 400 words.

"""


def newest_claude() -> str:
    """The Claude Code to run: this session's, or else the newest installed
    build (a scheduled task has no session, and the `claude` on PATH can lag
    the builds the lanes run)."""
    if os.environ.get("CLAUDE_CODE_EXECPATH"):
        return os.environ["CLAUDE_CODE_EXECPATH"]
    builds = sorted(Path.home().glob(".claude-cli/*/claude.exe"),
                    key=lambda p: tuple(int(x) for x in re.findall(r"\d+", p.parent.name)))
    return str(builds[-1]) if builds else "claude"


def propose(text: str) -> str:
    """Claude's three changes, as a section -- or, if the call fails or runs
    over half an hour, the reason why, so the week's numbers stand anyway.
    The prompt goes on stdin: the report can outgrow a command line."""
    import proctree  # the timeout that kills the whole tree before draining
    head = "## Three changes to make first (Claude, from the numbers above)\n\n"
    try:
        r = proctree.run_captured([newest_claude(), "-p", "--model", "opus", "--effort",
                                   "medium"], input=(PROPOSE + text).encode("utf-8"),
                                  timeout=1800, env=gitenv.clean_env())
    except OSError as e:
        return head + f"The proposal run could not start: {e}\n"
    out = r.stdout.decode("utf-8", errors="replace").strip()
    if r.timed_out:
        return head + "The proposal run took over half an hour and was stopped.\n"
    if r.returncode != 0 or not out:
        err = r.stderr.decode("utf-8", errors="replace").strip()[:500]
        return head + f"The proposal run failed (exit {r.returncode}): {err}\n"
    return head + out + "\n"


def register(out: str) -> int:
    """Register the weekly Task Scheduler entry: Mondays 06:30, after the
    curator, from the integration tree, which is kept at main. The task
    starts in System32, so `out` is made absolute here."""
    script = Path("E:/visual studio projects/os/scripts/process-review.py")
    pyw = Path(sys.executable).with_name("pythonw.exe")
    cmd = f'"{pyw}" "{script}" --out "{Path(out).resolve()}" --propose'
    r = subprocess.run(["schtasks", "/create", "/f", "/tn", TASK, "/sc", "weekly", "/d", "MON",
                        "/st", "06:30", "/tr", cmd], capture_output=True, text=True,
                       encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL)
    print(r.stdout.strip() or r.stderr.strip())
    return r.returncode


def write_report(out: Path, today: dt.date, text: str) -> Path:
    out.mkdir(parents=True, exist_ok=True)
    dated = out / f"process-review-{today.isoformat()}.md"
    for path in (dated, out / "latest.md"):
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
    return dated


# ---------------------------------------------------------------------------

def self_test() -> int:
    failures = []

    def expect(what, got, want):
        if got != want:
            failures.append(f"{what}: got {got!r}, want {want!r}")

    a, b = least_squares_2([(2 * x + 3 * y, x, y) for x, y in ((1, 0), (0, 1), (1, 1), (2, 5))])
    expect("fit", (round(a, 6), round(b, 6)), (2.0, 3.0))
    week = "2026-10-20T00:00:00+00:00"
    samples = [{"ts": t, "seven_day": {"utilization": u, "resets_at": r}} for t, u, r in (
        (0, 10.0, week), (3600, 11.0, week), (7200, 14.0, "2026-10-20T00:59:59.9+00:00"),
        (10800, 15.0, week), (14400, 100.0, week), (18000, 100.0, week),
        (21600, 2.0, "2026-10-27T00:00:00+00:00"), (28800, 5.0, "2026-10-27T00:00:00+00:00"))]
    calls = [(1800, 1.0, 2.0), (5400, 3.0, 4.0), (9000, 5.0, 6.0), (25000, 7.0, 8.0)]
    expect("windows", meter_windows(samples, calls, 7200),
           [(4.0, 4.0, 6.0), (3.0, 7.0, 8.0)])
    rows = [{"ts": "2026-10-10", "verdict": "PASS", "src_digest": "x", "gates_seconds": 600,
             "wall_seconds": 300, "branch": "lane-a"},
            {"ts": "2026-10-10", "verdict": "PANIC", "src_digest": "x", "gates_seconds": 600,
             "wall_seconds": 300, "branch": "lane-a"},
            {"ts": "2026-10-11", "verdict": "PANIC", "src_digest": "y", "gates_seconds": 1200,
             "wall_seconds": 60, "branch": "HEAD"},
            {"ts": "2026-09-01", "verdict": "PANIC", "src_digest": "z"}]
    text = boot_section(rows, "2026-10-05")
    expect("boots counted", "3 boots: 2 PANIC, 1 PASS" in text, True)
    expect("flaky told apart", "Of the 2 that failed, 1 were flaky" in text, True)
    log = ("\x1eaaa\nfixes\n\nReviewed: a..b\nReview-tier: max\nReview-runs: opus-max r, "
           "fable-xhigh unavailable\nReview-mutants: 2 missed of 20 tested (25 found) d\n"
           "Review-findings: 5 fixed, 1 dismissed\n"
           "\x1ebbb\nprose\n\nReviewed: 69a27c0ab by Opus\n"
           "\x1eccc\nmore\n\nReviewed: c..d\nReview-tier: xhigh\nReview-findings: 1 fixed, 0 dismissed\n")
    text = review_section(log)
    expect("reviews", ("1 at max, 1 at xhigh" in text, "6 findings fixed, 1 dismissed" in text,
                       "20 mutants and 2 survived" in text, "Fable could not review 1" in text),
           (True, True, True, True))
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "known-issues").mkdir()
        (root / "requests").mkdir()
        (root / "known-issues" / "A-old.md").write_text(
            "### [A] old -- 2026-08-01\n\n**Status:** OPEN\n", encoding="utf-8", newline="\n")
        (root / "known-issues" / "A-new.md").write_text(
            "### [A] new -- 2026-10-10\n\n**Status:** OPEN\n", encoding="utf-8", newline="\n")
        # Stamped otherwise, but still in known-issues/: open all the same.
        (root / "known-issues" / "TD-B-half.md").write_text(
            "### [B] half done -- 2026-08-01\n\n**Status:** HALF\n", encoding="utf-8", newline="\n")
        (root / "requests" / "f-ad-x.md").write_text(
            "# x\n**Filed:** 2026-08-01. **Status:** open.\n", encoding="utf-8", newline="\n")
        text = backlog_section(root, dt.date(2026, 10, 12))
        expect("backlog A", "| A | 2 | 1 | 1 | 1 |" in text, True)
        expect("backlog D", "| D | 0 | 0 | 1 | 1 |" in text, True)
        expect("an entry stamped otherwise is open", "| B | 1 | 1 | 0 | 0 |" in text, True)
        history = root / "h.jsonl"
        history.write_text('{"ts": "a"}\n{"ts": "b", cut sh\n[1, 2]\n{"ts": "c"}\n',
                           encoding="utf-8", newline="\n")
        expect("malformed history lines skipped", [r["ts"] for r in load_history(history)],
               ["a", "c"])
        # Context per lane: requests deduplicated, sessions by their title.
        db = sqlite3.connect(":memory:")
        db.executescript(
            "create table titles(path text, ts real, kind text, title text);"
            "create table lines(path text, req_id text, ts real, model text, inp int, outp int,"
            " cw5 int, cw1h int, cr int);"
            "create table prompts(path text, uuid text, ts real, kind text, native int);")
        db.executemany("insert into titles values (?, 0, 'custom-title', ?)",
                       [("e.jsonl", "OS E"), ("x.jsonl", "something else")])
        db.executemany("insert into lines values (?, ?, ?, 'm', ?, 0, 0, 0, ?)", [
            ("e.jsonl", "r1", 100, 1000, 299000),   # one request, logged twice
            ("e.jsonl", "r1", 100, 1000, 299000),
            ("e.jsonl", "r2", 100, 0, 100000),
            ("e.jsonl", "r0", 1, 0, 9),             # before the window
            ("x.jsonl", "r9", 100, 0, 1)])          # not a lane
        db.executemany("insert into prompts values (?, 'u', ?, ?, 0)",
                       [("e.jsonl", 100, "compact"), ("e.jsonl", 100, "human")])
        got = context_rows(db, 50, lambda m: (0, 0, 0, 0, 1.0))
        expect("context rows", {k: v[:3] for k, v in got.items()}, {"E": [2, 400000, 1]})
        expect("context cost", round(got["E"][3], 6), 0.399)
        # A proposal that cannot run still leaves the numbers written.
        global newest_claude
        saved = newest_claude
        newest_claude = lambda: str(root / "no-such-claude.exe")  # noqa: E731
        try:
            section_text = propose("numbers")
        finally:
            newest_claude = saved
        expect("a failed proposal says why", "could not start" in section_text, True)
    for f in failures:
        print(f"FAIL {f}")
    print(f"process-review self-test: {'FAILED' if failures else 'ok'}")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    # Under pythonw.exe, as the scheduled task runs it, there are no streams.
    for stream in (sys.stdout, sys.stderr):
        if stream is not None:
            stream.reconfigure(encoding="utf-8", errors="replace")
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", help="write the report into this directory")
    ap.add_argument("--propose", action="store_true",
                    help="add Claude's three changes with the largest measured payoff")
    ap.add_argument("--register", metavar="DIR",
                    help="register the weekly scheduled task, writing reports into DIR")
    a = ap.parse_args(argv)
    if a.register:
        return register(a.register)
    today = dt.date.today()
    if not a.out:
        text = report(today, CUA_DIR)
        print(text + ("\n" + propose(text) if a.propose else ""))
        return 0
    out = Path(a.out)
    try:
        text = report(today, CUA_DIR)
        # The numbers are written before the proposal is asked for, so a
        # failed or slow proposal cannot lose them.
        dated = write_report(out, today, text)
        if a.propose:
            dated = write_report(out, today, text + "\n" + propose(text))
    except Exception:  # noqa: BLE001 -- a scheduled run has nobody to tell but this file
        import traceback
        out.mkdir(parents=True, exist_ok=True)
        with open(out / "last-error.txt", "w", encoding="utf-8", newline="\n") as f:
            f.write(f"{dt.datetime.now().isoformat(timespec='seconds')}\n"
                    f"{traceback.format_exc()}")
        return 1
    if sys.stdout is not None:
        print(dated)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
