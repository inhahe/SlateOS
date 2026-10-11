#!/usr/bin/env python3
"""Review a batch of commits at the level its contents call for; gate `main` on it.

It also checks that what is published to `main` was reviewed so. Every batch of commits gets an independent review before it is published
(`claude-global.md`, "Code review"): Opus at `xhigh`, or -- where a miss is
expensive -- Opus at `max` and then Fable at `xhigh`. This script makes that
call from the batch itself, the same way every time, so that nobody has to
watch for it:

    python scripts/review-batch.py <base>..<tip>          review it, print the record
    python scripts/review-batch.py --tier <base>..<tip>   only say which level, and why
    python scripts/review-batch.py --check <sha>...       the pre-push gate for `main`
    python scripts/review-batch.py --stats [--since DATE] real findings per changed line
    python scripts/review-batch.py --self-test

A review runs in a scratch worktree at the tip, so that the lane can go on
working in its own while it runs; the scratch worktree and anything built in it
are deleted afterwards. Each reviewer's report is kept in
`<git-common-dir>/reviews/`. Run it in the background: a `max` review of a
large batch takes an hour.

THE RECORD. A run ends by printing trailers for the follow-up commit -- the one
that fixes what the review found, or an empty one (`git commit --allow-empty`)
when nothing needed fixing -- to go in its message's last paragraph, beside
`Co-Authored-By`, with `Review-findings` filled in after triage:

    Reviewed: <base>..<tip>
    Review-tier: max
    Review-runs: opus-max <report>, fable-xhigh <report>
    Review-lane: F
    Review-written-at: xhigh
    Review-findings: <n> fixed, <m> dismissed

`--check` reads them. A commit published to `main` that changes code must lie in
some record's range, at a tier at least its own, and the reports the record
names must be on this machine; the record's own commit is the follow-up, which
is not reviewed again. `--stats` reads them to say whether the level code is
written at shows in how much its reviews find.

Exit status: 0 done (or, for `--check`, every commit reviewed); 1 a review that
could not run, or a commit `--check` refuses; 2 a usage error.
"""

from __future__ import annotations

import argparse
import datetime as dt
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gitenv  # noqa: E402
import selftestflag  # noqa: E402

#: Where a missed bug is expensive, by the list in `claude-global.md`: kernel
#: code, the C library, the system services and PID 1, the network stack,
#: crypto and password handling, and the parsers of untrusted input --
#: decoders, decompressors, the remote desktop's protocol. Repo-relative
#: directory prefixes; a change to any code under one is reviewed at `max`.
MAX_PATHS = (
    "kernel/",
    "posix/",
    "services/",
    "init/",
    "net/",
    "net80211/",
    "netipc/",
    "netproto/",
    "netring/",
    "aes/",
    "hmac/",
    "sha1/",
    "sha2/",
    "blake2/",
    "sm3/",
    "md5/",
    "rustcrypto/",
    "pwkdf/",
    "readpass/",
    "gui/video/",
    "gui/imagecodec/",
    "gui/font/",
    "gui/remote/",
    "deflate/",
    "bzip2/",
    "xz/",
    "sevenz/",
    "ziparchive/",
)

#: What makes a change high-stakes wherever it is, in an added line of code
#: that is not a comment: unsafe Rust and inline assembly, shared mutable
#: state and locking, and deleting or truncating files.
MAX_CONTENT = re.compile(
    r"\bunsafe\b|\b(?:global_|naked_)?asm!|\bstatic\s+mut\b"
    r"|\bAtomic[A-Z]\w*|\bOrdering::(?:Relaxed|Acquire|Release|AcqRel|SeqCst)\b"
    r"|\b(?:Mutex|RwLock|Condvar|SpinLock)\b|\bspin::"
    r"|\bremove_(?:file|dir|dir_all)\b|\b(?:unlink|rmdir|ftruncate)\s*\("
    r"|\.set_len\s*\(|\bshutil\.rmtree\b|\bos\.(?:remove|unlink|rmdir)\b"
)

#: A line that is only a comment, or a Rust attribute such as
#: `#![forbid(unsafe_code)]`: what it says is not what the code does.
COMMENT = re.compile(r"^\s*(?://|#|/\*|\*|--|;|REM\b|::)")

#: Files that are code. Everything else -- documentation, data, test
#: fixtures, generated reports -- is reviewed by nobody, as the rule says.
CODE_SUFFIXES = {
    ".rs", ".py", ".c", ".h", ".cc", ".cpp", ".hpp", ".s", ".asm", ".ld",
    ".sh", ".bash", ".ps1", ".bat", ".cmd", ".toml", ".adb", ".ads", ".gpr",
    ".js", ".ts", ".mjs",
}

TIERS = ("none", "xhigh", "max")

#: The reviews each tier runs, in order: (model, `/code-review` level).
RUNS = {"xhigh": (("opus", "xhigh"),), "max": (("opus", "max"), ("fable", "xhigh"))}

#: Reviews before this day were recorded in prose, which `--check` cannot
#: read; commits authored before it are not checked.
RECORDS_START = "2026-10-11"

#: How long one reviewer may run before it is stopped, in seconds.
REVIEW_TIMEOUT = 4 * 3600

QEMU_IMAGES = ("qemu-system-x86_64.exe", "qemu-system-x86_64w.exe")


# ---------------------------------------------------------------------------
# git
# ---------------------------------------------------------------------------

def git(*args: str, cwd: str | Path | None = None, check: bool = True) -> str:
    """`git args...`'s output, in `cwd`, with no inherited `GIT_DIR`."""
    r = subprocess.run(
        ["git", *args], cwd=cwd, env=gitenv.clean_env(), capture_output=True,
        text=True, encoding="utf-8", errors="replace",
    )
    if check and r.returncode != 0:
        raise SystemExit(f"review-batch: git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def common_dir(cwd: str | Path | None = None) -> Path:
    """The git directory every worktree of this repository shares."""
    return Path(git("rev-parse", "--path-format=absolute", "--git-common-dir", cwd=cwd).strip())


def reports_dir(cwd: str | Path | None = None) -> Path:
    return common_dir(cwd) / "reviews"


def resolve(rev: str, cwd: str | Path | None = None) -> str:
    return git("rev-parse", "--verify", f"{rev}^{{commit}}", cwd=cwd).strip()


# ---------------------------------------------------------------------------
# What a change is
# ---------------------------------------------------------------------------

def is_code(path: str) -> bool:
    """Whether `path` (repo-relative, `/`-separated) is code."""
    p = PurePosixPath(path)
    if p.suffix.lower() in CODE_SUFFIXES:
        return True
    # The hooks under scripts/ are shell scripts with no suffix.
    return p.suffix == "" and path.startswith("scripts/")


def judge(files: dict[str, list[str]]) -> tuple[str, list[str]]:
    """The tier a change calls for, and why.

    `files` maps each path the change touches to the lines it adds there. The
    answer is `none` when no code changes, `max` when code under `MAX_PATHS`
    changes or an added line of code matches `MAX_CONTENT`, else `xhigh`.
    """
    tier, why = "none", []
    for path, added in files.items():
        if not is_code(path):
            continue
        if tier == "none":
            tier = "xhigh"
        if path.startswith(MAX_PATHS):
            tier = "max"
            why.append(f"{path}: under {next(p for p in MAX_PATHS if path.startswith(p))}")
            continue
        for line in added:
            if COMMENT.match(line):
                continue
            m = MAX_CONTENT.search(line)
            if m:
                tier = "max"
                why.append(f"{path}: adds `{m.group(0)}`: {line.strip()[:100]}")
                break
    return tier, why


def higher(a: str, b: str) -> str:
    return a if TIERS.index(a) >= TIERS.index(b) else b


class Commit:
    """One non-merge commit: its sha, author date, message and added lines."""

    def __init__(self, sha: str, date: str, message: str):
        self.sha, self.date, self.message = sha, date, message
        self.files: dict[str, list[str]] = {}

    def tier(self) -> tuple[str, list[str]]:
        return judge(self.files)


SEP = "\x1e"  # a record separator: git prints it before each commit


def commits(*revs: str, cwd: str | Path | None = None) -> list[Commit]:
    """The non-merge commits `revs` names (`git log` arguments), each with the
    lines it adds -- one `git log -p` for all of them."""
    out = git("log", "--no-merges", "-p", "-U0", "--no-color", "--no-ext-diff",
              f"--format={SEP}%H%n%aI%n%B{SEP}", *revs, "--", cwd=cwd)
    # Each commit prints as SEP header SEP diff, so the pieces alternate.
    return _parse(out.split(SEP)[1:])


def _parse(chunks: list[str]) -> list[Commit]:
    """`commits`' output, which alternates header (sha, date, message) and diff."""
    out: list[Commit] = []
    i = 0
    while i < len(chunks):
        head = chunks[i]
        sha, date, message = head.split("\n", 2) if head.count("\n") >= 2 else (head, "", "")
        c = Commit(sha.strip(), date.strip(), message)
        diff = chunks[i + 1] if i + 1 < len(chunks) else ""
        path = None
        for line in diff.splitlines():
            if line.startswith("+++ "):
                path = None if line == "+++ /dev/null" else line[6:] if line.startswith("+++ b/") else line[4:]
                if path is not None:
                    c.files.setdefault(path, [])
            elif line.startswith("--- "):
                old = line[4:]
                if old != "/dev/null":
                    # A deletion still touches the path; its lines are not added.
                    c.files.setdefault(old[2:] if old.startswith("a/") else old, [])
            elif line.startswith("+") and path is not None:
                c.files[path].append(line[1:])
        out.append(c)
        i += 2
    return out


# ---------------------------------------------------------------------------
# Records
# ---------------------------------------------------------------------------

TRAILER = re.compile(r"^(Reviewed|Review-[A-Za-z-]+):\s*(.*?)\s*$", re.M)


class Record:
    """A `Reviewed:` record, read from a commit message's trailers."""

    def __init__(self, sha: str, fields: dict[str, str]):
        self.sha = sha
        base, _, tip = fields.get("Reviewed", "").partition("..")
        self.base, self.tip = base.strip(), tip.strip()
        self.tier = fields.get("Review-tier", "").strip()
        self.runs = [r.strip() for r in fields.get("Review-runs", "").split(",") if r.strip()]
        self.lane = fields.get("Review-lane", "-").strip() or "-"
        self.written_at = fields.get("Review-written-at", "unknown").strip() or "unknown"
        m = re.match(r"\s*(\d+)\s+fixed\D+(\d+)\s+dismissed", fields.get("Review-findings", ""))
        self.fixed, self.dismissed = (int(m.group(1)), int(m.group(2))) if m else (None, None)

    def reports(self) -> list[str]:
        """The report files the record names (a run that could not happen
        names none)."""
        return [r.split(None, 1)[1] for r in self.runs
                if len(r.split(None, 1)) == 2 and r.split(None, 1)[1] != "unavailable"]


def record_of(c: Commit) -> Record | None:
    """The review record `c`'s message carries, if any. Trailers are the
    message's last paragraph, as `git interpret-trailers` reads them."""
    last = c.message.strip().split("\n\n")[-1]
    fields = {m.group(1): m.group(2) for m in TRAILER.finditer(last)}
    return Record(c.sha, fields) if "Reviewed" in fields else None


# ---------------------------------------------------------------------------
# --tier and the review run
# ---------------------------------------------------------------------------

def batch_tier(rng: str, cwd: str | Path | None = None) -> tuple[str, list[str], list[Commit]]:
    cs = commits(rng, cwd=cwd)
    tier, why = "none", []
    for c in cs:
        t, w = c.tier()
        tier = higher(tier, t)
        why += [f"{c.sha[:10]} {x}" for x in w]
    return tier, why, cs


def lane_letter(top: Path) -> str:
    m = re.match(r"os-lane-([a-f])\b", top.name)
    return m.group(1).upper() if m else "-"


def qemu_running() -> bool:
    if os.name != "nt":
        return subprocess.run(["pgrep", "-f", "qemu-system"], capture_output=True).returncode == 0
    out = subprocess.run(["tasklist", "/NH"], capture_output=True, text=True,
                         encoding="utf-8", errors="replace").stdout.lower()
    return any(name in out for name in QEMU_IMAGES)


def wait_for_no_qemu(limit: int) -> None:
    """A reviewer may build to check a finding, and nothing may be built
    beside a QEMU guest (`E:/visual studio projects/CLAUDE.md`)."""
    if not qemu_running():
        return
    print("review-batch: a QEMU guest is up; waiting for it to finish", flush=True)
    start = time.monotonic()
    while qemu_running():
        if time.monotonic() - start > limit:
            raise SystemExit("review-batch: a QEMU guest is still up; run the review later")
        time.sleep(60)
    print("review-batch: no QEMU guest is up; starting", flush=True)


def review_commands(base: str, tip: str, tier: str) -> list[tuple[str, str, list[str]]]:
    """(model, level, argv) for each review `tier` runs."""
    claude = os.environ.get("CLAUDE_CODE_EXECPATH") or shutil.which("claude") or "claude"
    return [(model, level, [claude, "-p", "--model", model,
                            f"/code-review {level} commits {base}..{tip}"])
            for model, level in RUNS[tier]]


def run_review(argv: list[str], cwd: Path, report: Path, header: str) -> tuple[int, str]:
    """Run one reviewer in `cwd` under `run-timeout.py` (which stops its whole
    process tree), keep what it says in `report`, and answer its exit status
    and output."""
    runner = Path(__file__).resolve().parent / "run-timeout.py"
    r = subprocess.run([sys.executable, str(runner), str(REVIEW_TIMEOUT), *argv], cwd=cwd,
                       env=gitenv.clean_env(), capture_output=True, text=True,
                       encoding="utf-8", errors="replace")
    said = "\n".join(line for line in (r.stdout + r.stderr).splitlines()
                     if not line.startswith("[run-timeout]"))
    report.parent.mkdir(parents=True, exist_ok=True)
    with open(report, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + f"exit status: {r.returncode}\n\n" + said + "\n")
    return r.returncode, said


def remove_scratch(top: Path, scratch: Path) -> None:
    """Remove the scratch worktree and whatever was built in it."""
    if (scratch / "target").exists() and os.name == "nt":
        # A build directory is tens of gigabytes; `rd` is minutes faster.
        subprocess.run(["cmd", "/c", "rd", "/s", "/q", str(scratch / "target")],
                       capture_output=True)
    git("worktree", "remove", "--force", str(scratch), cwd=top, check=False)
    if scratch.exists():
        gitenv.remove_tree(scratch)
    git("worktree", "prune", cwd=top, check=False)


def review(rng: str, dry_run: bool, qemu_wait: int) -> int:
    top = Path(git("rev-parse", "--show-toplevel").strip())
    base_rev, _, tip_rev = rng.partition("..")
    if not tip_rev:
        raise SystemExit("review-batch: give the batch as <base>..<tip>")
    base, tip = resolve(base_rev), resolve(tip_rev)
    tier, why, cs = batch_tier(f"{base}..{tip}")
    print(f"review-batch: {len(cs)} commits, {base[:10]}..{tip[:10]}: tier {tier}")
    for w in why[:20]:
        print(f"  {w}")
    if tier == "none":
        print("review-batch: no code changes; the rule reviews nothing here")
        return 0
    cmds = review_commands(base, tip, tier)
    if dry_run:
        for _, _, argv in cmds:
            print("would run: " + " ".join(argv))
        return 0

    wait_for_no_qemu(qemu_wait)
    scratch = top.parent / f"{top.name}-review-{tip[:10]}"
    if scratch.exists():
        remove_scratch(top, scratch)
    git("worktree", "add", "--detach", str(scratch), tip, cwd=top)
    runs, failed = [], False
    stamp = dt.datetime.now().strftime("%Y%m%dT%H%M%S")
    try:
        for model, level, argv in cmds:
            name = f"{stamp}-{tip[:12]}-{model}-{level}.md"
            report = reports_dir(top) / name
            header = (f"review of {base}..{tip}\nreviewer: {model} at {level}\n"
                      f"started: {dt.datetime.now().isoformat(timespec='seconds')}\n")
            print(f"review-batch: {model} at {level} -- started", flush=True)
            code, said = run_review(argv, scratch, report, header)
            limited = model == "fable" and "limit" in said.lower() and code != 0
            if limited:
                runs.append(f"{model}-{level} unavailable")
                print(f"review-batch: {model} could not run (its limit is reached); "
                      "the record says so")
            elif code != 0:
                failed = True
                print(f"review-batch: {model} at {level} FAILED (exit {code}); see {report}")
                break
            else:
                runs.append(f"{model}-{level} {name}")
                print(f"review-batch: {model} at {level} -- done; report {report}", flush=True)
                print(said[-6000:])
    finally:
        remove_scratch(top, scratch)
    if failed:
        return 1
    print("\nTrailers for the follow-up commit (fill in the findings after triage):\n")
    print(f"Reviewed: {base}..{tip}")
    print(f"Review-tier: {tier}")
    print(f"Review-runs: {', '.join(runs)}")
    print(f"Review-lane: {lane_letter(top)}")
    print(f"Review-written-at: {os.environ.get('CLAUDE_EFFORT', 'unknown')}")
    print("Review-findings: <n> fixed, <m> dismissed")
    return 0


# ---------------------------------------------------------------------------
# --check: the pre-push gate
# ---------------------------------------------------------------------------

def check(shas: list[str], remote: str, cwd: str | Path | None = None,
          reports: Path | None = None, start: str = RECORDS_START) -> tuple[int, list[str]]:
    """Whether every commit `shas` would add to `<remote>/main` is reviewed:
    (exit status, what is wrong)."""
    exclude = f"refs/remotes/{remote}/main"
    revs = [*shas, "--not", exclude] if git("rev-parse", "--verify", "--quiet", exclude,
                                              cwd=cwd, check=False).strip() else list(shas)
    cs = commits(*revs, cwd=cwd)
    reports = reports if reports is not None else reports_dir(cwd)
    records = [r for r in (record_of(c) for c in cs) if r]
    covers: list[tuple[Record, set[str]]] = []
    problems: list[str] = []
    for r in records:
        missing = [p for p in r.reports() if not (reports / p).is_file()]
        if not r.base or not r.tip or r.tier not in TIERS[1:]:
            problems.append(f"{r.sha[:10]}: its review record is incomplete")
            continue
        if missing:
            problems.append(f"{r.sha[:10]}: its record names reports this machine "
                            f"does not have: {', '.join(missing)}")
            continue
        inside = git("rev-list", f"{r.base}..{r.tip}", cwd=cwd, check=False).split()
        covers.append((r, set(inside)))
    for c in cs:
        if c.date[:10] < start or record_of(c):
            continue
        need, why = c.tier()
        if need == "none":
            continue
        if not any(c.sha in inside and TIERS.index(r.tier) >= TIERS.index(need)
                   for r, inside in covers):
            reason = f" ({why[0]})" if why else ""
            problems.append(f"{c.sha[:10]} needs a review at {need}{reason}: "
                            f"{c.message.strip().splitlines()[0][:70] if c.message.strip() else ''}")
    return (1 if problems else 0), problems


# ---------------------------------------------------------------------------
# --stats
# ---------------------------------------------------------------------------

def code_lines(base: str, tip: str, cwd: str | Path | None = None) -> int:
    n = 0
    for line in git("diff", "--numstat", base, tip, cwd=cwd, check=False).splitlines():
        parts = line.split("\t")
        if len(parts) == 3 and parts[0].isdigit() and is_code(parts[2]):
            n += int(parts[0]) + int(parts[1])
    return n


def stats(rev: str, since: str, cwd: str | Path | None = None) -> list[tuple]:
    """(lane, written-at, reviews, code lines, fixed, dismissed) for the
    records on `rev` since `since`, grouped by lane and writing level."""
    groups: dict[tuple[str, str], list[int]] = {}
    for c in commits(rev, f"--since={since}", "--grep=^Reviewed: ", cwd=cwd):
        r = record_of(c)
        if not r or r.fixed is None:
            continue
        g = groups.setdefault((r.lane, r.written_at), [0, 0, 0, 0])
        g[0] += 1
        g[1] += code_lines(r.base, r.tip, cwd=cwd)
        g[2] += r.fixed
        g[3] += r.dismissed
    return [(lane, at, *v) for (lane, at), v in sorted(groups.items())]


def print_stats(rows: list[tuple]) -> None:
    print(f"{'lane':4} {'written at':10} {'reviews':>7} {'code lines':>10} "
          f"{'fixed':>6} {'dismissed':>9} {'fixed/1000 lines':>16}")
    for lane, at, n, lines, fixed, dismissed in rows:
        rate = f"{1000 * fixed / lines:.2f}" if lines else "-"
        print(f"{lane:4} {at:10} {n:7} {lines:10} {fixed:6} {dismissed:9} {rate:>16}")


# ---------------------------------------------------------------------------
# --self-test
# ---------------------------------------------------------------------------

def self_test() -> int:
    failures: list[str] = []

    def expect(what: str, got, want) -> None:
        if got != want:
            failures.append(f"{what}: got {got!r}, want {want!r}")

    expect("docs only", judge({"README.md": ["text"]})[0], "none")
    expect("plain code", judge({"apps/x/src/lib.rs": ["fn f() {}"]})[0], "xhigh")
    expect("kernel path", judge({"kernel/src/a.rs": ["fn f() {}"]})[0], "max")
    expect("unsafe added", judge({"apps/x/src/lib.rs": ["    unsafe { f() }"]})[0], "max")
    expect("unsafe in a comment", judge({"apps/x/src/lib.rs": ["// not unsafe"]})[0], "xhigh")
    expect("forbid attribute", judge({"apps/x/src/lib.rs": ["#![forbid(unsafe_code)]"]})[0],
           "xhigh")
    expect("atomics", judge({"gui/x.rs": ["x.load(Ordering::Acquire)"]})[0], "max")
    expect("rmtree in a script", judge({"scripts/t.py": ["shutil.rmtree(p)"]})[0], "max")
    expect("hook with no suffix", judge({"scripts/hooks/pre-push": ["echo"]})[0], "xhigh")
    expect("a word inside another", judge({"apps/x.rs": ["let unsafety = 1;"]})[0], "xhigh")

    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        reports = Path(tmp) / "reports"
        reports.mkdir()

        def run(*args: str, date: str = "2026-10-12T10:00:00") -> str:
            env = gitenv.clean_env()
            env.update(GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date,
                       GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t",
                       GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t")
            r = subprocess.run(["git", *args], cwd=repo, env=env, capture_output=True,
                               text=True, encoding="utf-8")
            if r.returncode:
                raise SystemExit(f"self-test: git {args}: {r.stderr}")
            return r.stdout.strip()

        def commit(path: str, text: str, msg: str, date: str = "2026-10-12T10:00:00") -> str:
            f = repo / path
            f.parent.mkdir(parents=True, exist_ok=True)
            with open(f, "a", encoding="utf-8", newline="\n") as h:
                h.write(text + "\n")
            run("add", path)
            run("commit", "-q", "-m", msg, date=date)
            return run("rev-parse", "HEAD")

        run("init", "-q", "-b", "main")
        root = commit("README.md", "hello", "docs")
        run("update-ref", "refs/remotes/origin/main", root)
        old = commit("apps/a.rs", "fn old() {}", "before records", date="2026-10-01T10:00:00")
        docs = commit("notes.md", "more", "docs only")
        plain = commit("apps/b.rs", "fn b() {}", "plain")
        kern = commit("kernel/k.rs", "fn k() {}", "kernel")
        tip = commit("apps/c.rs", "fn c() { unsafe { x() } }", "unsafe")

        status, problems = check([tip], "origin", cwd=repo, reports=reports)
        expect("unreviewed: refused", status, 1)
        flagged = sorted(p.split()[0] for p in problems)
        expect("unreviewed: which", flagged, sorted(s[:10] for s in (plain, kern, tip)))
        expect("old and docs pass", any(old[:10] in p or docs[:10] in p for p in problems), False)

        (reports / "r1.md").write_text("ok", encoding="utf-8", newline="\n")
        rec_x = commit("apps/b.rs", "// fix", "review fixes\n\nReviewed: "
                       f"{root}..{tip}\nReview-tier: xhigh\nReview-runs: opus-xhigh r1.md\n"
                       "Review-lane: F\nReview-written-at: max\n"
                       "Review-findings: 3 fixed, 1 dismissed")
        status, problems = check([rec_x], "origin", cwd=repo, reports=reports)
        expect("xhigh record: max commits still refused", sorted(p.split()[0] for p in problems),
               sorted(s[:10] for s in (kern, tip)))

        (reports / "r2.md").write_text("ok", encoding="utf-8", newline="\n")
        rec_m = commit("apps/b.rs", "// fix 2", "more fixes\n\nReviewed: "
                       f"{root}..{tip}\nReview-tier: max\nReview-runs: opus-max r2.md, "
                       "fable-xhigh unavailable\nReview-lane: F\nReview-written-at: max\n"
                       "Review-findings: 2 fixed, 0 dismissed\n"
                       "Co-Authored-By: someone <x@y>")
        status, problems = check([rec_m], "origin", cwd=repo, reports=reports)
        expect("max record: all reviewed", (status, problems), (0, []))

        (reports / "r2.md").unlink()
        status, problems = check([rec_m], "origin", cwd=repo, reports=reports)
        expect("a missing report is refused", status, 1)
        expect("a missing report is named", any("r2.md" in p for p in problems), True)
        (reports / "r2.md").write_text("ok", encoding="utf-8", newline="\n")

        later = commit("apps/d.rs", "fn d() {}", "new work")
        status, problems = check([later], "origin", cwd=repo, reports=reports)
        expect("work after the record is refused", [p.split()[0] for p in problems],
               [later[:10]])

        rows = stats("HEAD", "2026-01-01", cwd=repo)
        # Both records cover root..tip, so its lines count once for each.
        expect("stats", rows, [("F", "max", 2, 2 * code_lines(root, tip, cwd=repo), 5, 1)])

        rec = record_of(commits(f"{rec_m}^!", cwd=repo)[0])
        expect("record runs", rec.reports() if rec else None, ["r2.md"])

    cmds = review_commands("a" * 40, "b" * 40, "max")
    expect("max runs", [(m, l) for m, l, _ in cmds], [("opus", "max"), ("fable", "xhigh")])
    expect("max prompt", cmds[0][2][-1], f"/code-review max commits {'a' * 40}..{'b' * 40}")

    for f in failures:
        print(f"FAIL {f}")
    print(f"review-batch self-test: {'FAILED' if failures else 'ok'}")
    return 1 if failures else 0


# ---------------------------------------------------------------------------

def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("range", nargs="*", help="<base>..<tip>, or for --check the shas pushed")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--tier", action="store_true", help="only print the tier and why")
    mode.add_argument("--check", action="store_true", help="the pre-push gate for main")
    mode.add_argument("--stats", action="store_true", help="findings per changed line")
    ap.add_argument("--remote", default="origin", help="the remote whose main --check compares with")
    ap.add_argument("--since", default=RECORDS_START, help="--stats: records from this date")
    ap.add_argument("--dry-run", action="store_true", help="print the reviews; run none")
    ap.add_argument("--qemu-wait", type=int, default=4 * 3600,
                    help="seconds to wait for a QEMU guest to finish before reviewing")
    a = ap.parse_args(argv)
    if a.stats:
        print_stats(stats(f"refs/remotes/{a.remote}/main", a.since))
        return 0
    if a.check:
        if not a.range:
            return 0
        status, problems = check(a.range, a.remote)
        if problems:
            print("pre-push: REFUSING to publish commits no review covers at their tier:",
                  file=sys.stderr)
            for p in problems:
                print(f"  {p}", file=sys.stderr)
            print("Review the batch with `python scripts/review-batch.py <base>..<tip>` "
                  "and put the trailers it prints in the follow-up commit.", file=sys.stderr)
        return status
    if len(a.range) != 1:
        ap.error("give one range, <base>..<tip>")
    if a.tier:
        tier, why, cs = batch_tier(a.range[0])
        print(f"{tier}  ({len(cs)} commits)")
        for w in why:
            print(f"  {w}")
        return 0
    return review(a.range[0], a.dry_run, a.qemu_wait)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
