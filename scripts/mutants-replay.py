#!/usr/bin/env python3
"""Replay the mutants `cargo test` could not kill through differential harnesses.

Usage:
    python scripts/mutants-replay.py --tree DIR MUTANTS_DIR HARNESS...
    python scripts/mutants-replay.py --self-test

`review-batch.py` judges a mutant by `cargo test` alone. Most of what lane B
ports is held to its upstream by a WSL differential harness instead
(`scripts/*-diff.sh`), which `cargo test` never runs: `curses-diff.sh`'s 990
cases against Ubuntu's libncursesw, say, where the crate's unit tests are a
few dozen. So most of a port's mutants survive the unit tests, and the
`Review-mutants` count says little until each survivor has met the harness
that tests its code.

For each mutant MUTANTS_DIR (one package's cargo-mutants output, holding
`mutants.out/`) lists as missed, this applies the mutant's diff to the
worktree DIR, runs each HARNESS (a path relative to DIR) from DIR with
`DIFF_FAIL_FAST=1` -- a harness that honours it stops at its first
difference -- and records one line in `MUTANTS_DIR/replay.txt`:

    caught <harness> <mutant>   the harness differed, twice running (or hung)
    flaky <harness> <mutant>    it differed once, then agreed: look by hand
    missed - <mutant>           every harness agreed: a gap in the tests
    unviable - <mutant>         the mutant does not compile for Linux
    stale - <mutant>            the diff no longer applies to DIR's commit

The file is appended to as the run goes, and a mutant already in it is not
run again, so an interrupted run carries on where it stopped. Each harness's
output is kept in `MUTANTS_DIR/replay-logs/`.

DIR must be a worktree of its own, made for this (`git worktree add --detach
DIR <commit>`): its changes are thrown away after every mutant, and the run
refuses to start unless `git status` there is clean. The harness builds DIR's
copy of the code, so DIR's commit is the code the mutants are applied to; a
mutant whose lines have changed since the review is `stale`, and the next
review meets the new lines anyway.

Nothing may be built beside a QEMU guest: the run waits for one to finish
before each mutant. A harness that skips -- no WSL, no reference -- would
read as "every case agrees", so a skip stops the run instead.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gitenv  # noqa: E402
import selftestflag  # noqa: E402

#: The Windows images a QEMU guest runs as.
QEMU_IMAGES = ("qemu-system-x86_64.exe", "qemu-system-x86_64w.exe")

#: Seconds one harness may take on one mutant.
HARNESS_TIMEOUT = 15 * 60


def missed_mutants(outcomes: dict) -> list[tuple[str, str]]:
    """(name, diff path relative to `mutants.out`) of every mutant the
    outcomes say no test caught, in the order cargo-mutants ran them."""
    out = []
    for o in outcomes.get("outcomes", []):
        if o.get("summary") != "MissedMutant":
            continue
        scenario = o.get("scenario")
        mutant = scenario.get("Mutant") if isinstance(scenario, dict) else None
        if not mutant or not o.get("diff_path"):
            continue
        out.append((mutant["name"], o["diff_path"]))
    return out


def done_names(text: str) -> set[str]:
    """The mutants a `replay.txt` already has a verdict for."""
    names = set()
    for line in text.splitlines():
        parts = line.split(" ", 2)
        if len(parts) == 3 and parts[0] in VERDICTS:
            names.add(parts[2])
    return names


#: The first word of each line of `replay.txt`.
VERDICTS = ("caught", "flaky", "missed", "unviable", "stale")

#: What a harness prints when it skips rather than compares.
SKIP = re.compile(r"(?i)\bskipp(?:ing|ed)\b")
#: What rustc prints when the mutant does not compile.
RUSTC_ERROR = re.compile(r"^error(?:\[E\d+\])?: ", re.MULTILINE)


def verdict(rc: int, output: str) -> str:
    """`caught`, `agreed`, `unviable` or `skipped`, for a harness that exited
    `rc` having printed `output`."""
    if rc == 0:
        return "skipped" if SKIP.search(output) else "agreed"
    if RUSTC_ERROR.search(output):
        return "unviable"
    return "caught"


def qemu_running() -> bool:
    if os.name != "nt":
        return subprocess.run(["pgrep", "-f", "qemu-system"],
                              capture_output=True).returncode == 0
    out = subprocess.run(["tasklist", "/NH"], capture_output=True, text=True,
                         encoding="utf-8", errors="replace").stdout.lower()
    return any(name in out for name in QEMU_IMAGES)


def wait_for_no_qemu() -> None:
    if qemu_running():
        print("mutants-replay: a QEMU guest is up; waiting for it", flush=True)
        while qemu_running():
            time.sleep(60)


def git(tree: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", "-C", str(tree), *args], capture_output=True, text=True,
                          encoding="utf-8", errors="replace", env=gitenv.clean_env())


def applicable(diff: str) -> str:
    """A cargo-mutants diff as `git apply -p0` takes it: its `+++` line names
    the mutation rather than the file, so it is given the file's name."""
    lines = diff.split("\n")
    if len(lines) > 1 and lines[0].startswith("--- ") and lines[1].startswith("+++ "):
        lines[1] = "+++ " + lines[0][4:]
    return "\n".join(lines)


def safe_name(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", name)[:150]


def run_harness(bash: str, tree: Path, harness: str, log: Path) -> str:
    """Run `harness` from `tree`, stopping at its first difference, keep its
    output in `log`, and say what it found (`verdict`, or `hung`)."""
    try:
        r = subprocess.run([bash, harness], cwd=tree, capture_output=True, text=True,
                           encoding="utf-8", errors="replace", timeout=HARNESS_TIMEOUT,
                           env=dict(os.environ, DIFF_FAIL_FAST="1"))
    except subprocess.TimeoutExpired as e:
        out = e.stdout if isinstance(e.stdout, str) else ""
        log.write_text(out + "\n(timed out)\n", encoding="utf-8", newline="\n")
        return "hung"
    output = r.stdout + r.stderr
    log.write_text(output, encoding="utf-8", newline="\n")
    return verdict(r.returncode, output)


def replay(tree: Path, mutants_dir: Path, harnesses: list[str]) -> int:
    mo = mutants_dir / "mutants.out"
    outcomes = json.loads((mo / "outcomes.json").read_text(encoding="utf-8"))
    todo = missed_mutants(outcomes)
    record = mutants_dir / "replay.txt"
    done = done_names(record.read_text(encoding="utf-8")) if record.exists() else set()
    logs = mutants_dir / "replay-logs"
    logs.mkdir(exist_ok=True)
    status = git(tree, "status", "--porcelain")
    if status.returncode != 0 or status.stdout.strip():
        print(f"mutants-replay: {tree} is not a clean worktree", file=sys.stderr)
        return 2
    bash = shutil.which("bash")
    if bash is None:
        print("mutants-replay: no bash on PATH", file=sys.stderr)
        return 2
    left = [m for m in todo if m[0] not in done]
    print(f"mutants-replay: {len(todo)} missed, {len(left)} still to replay", flush=True)
    if left:
        # Unmutated, every harness must agree: otherwise each mutant would be
        # "caught" by whatever already differs.
        wait_for_no_qemu()
        for h in harnesses:
            if run_harness(bash, tree, h, logs / f"baseline.{Path(h).stem}.log") != "agreed":
                print(f"mutants-replay: {h} does not agree on the unmutated tree; "
                      f"see {logs}", file=sys.stderr)
                return 2
        print("mutants-replay: the unmutated tree agrees", flush=True)
    for n, (name, diff) in enumerate(left, 1):
        wait_for_no_qemu()
        patch = logs / "current.diff"
        patch.write_text(applicable((mo / diff).read_text(encoding="utf-8")),
                         encoding="utf-8", newline="\n")
        applied = git(tree, "apply", "-p0", str(patch))
        if applied.returncode != 0:
            line = f"stale - {name}"
        else:
            line = f"missed - {name}"
            for h in harnesses:
                v = run_harness(bash, tree, h, logs / f"{safe_name(name)}.{Path(h).stem}.log")
                if v == "caught":
                    # A difference the machine's load caused would not come
                    # back: the mutant must be caught twice.
                    v = run_harness(bash, tree, h,
                                    logs / f"{safe_name(name)}.{Path(h).stem}.again.log")
                    if v == "agreed":
                        line = f"flaky {h} {name}"
                        break
                if v == "skipped":
                    git(tree, "reset", "--hard", "-q")
                    print(f"mutants-replay: {h} skipped rather than compared; stopping",
                          file=sys.stderr)
                    return 2
                if v in ("caught", "hung"):
                    line = f"caught {h} {name}"
                    break
                if v == "unviable":
                    line = f"unviable - {name}"
                    break
        reset = git(tree, "reset", "--hard", "-q")
        if reset.returncode != 0 or git(tree, "status", "--porcelain").stdout.strip():
            print(f"mutants-replay: could not put {tree} back", file=sys.stderr)
            return 2
        with record.open("a", encoding="utf-8", newline="\n") as f:
            f.write(line + "\n")
        print(f"mutants-replay: [{n}/{len(left)}] {line}", flush=True)
    text = record.read_text(encoding="utf-8") if record.exists() else ""
    counts = {k: sum(1 for x in text.splitlines() if x.startswith(k + " "))
              for k in VERDICTS}
    print("mutants-replay: " + ", ".join(f"{v} {k}" for k, v in counts.items()), flush=True)
    return 0


def self_test() -> int:
    outcomes = {"outcomes": [
        {"scenario": "Baseline", "summary": "Success"},
        {"scenario": {"Mutant": {"name": "a.rs:1:1: replace x"}}, "summary": "MissedMutant",
         "diff_path": "diff/a.diff"},
        {"scenario": {"Mutant": {"name": "a.rs:2:1: replace y"}}, "summary": "CaughtMutant",
         "diff_path": "diff/b.diff"},
        {"scenario": {"Mutant": {"name": "a.rs:3:1: delete !"}}, "summary": "MissedMutant",
         "diff_path": "diff/c.diff"},
    ]}
    checks = [
        (missed_mutants(outcomes),
         [("a.rs:1:1: replace x", "diff/a.diff"), ("a.rs:3:1: delete !", "diff/c.diff")]),
        (done_names("caught h.sh a.rs:1:1: replace x\nstale - b c d\nnoise\n"),
         {"a.rs:1:1: replace x", "b c d"}),
        (verdict(0, "curses-diff: 990 case(s) agree"), "agreed"),
        (verdict(0, "curses-diff: no WSL on this host; skipping"), "skipped"),
        (verdict(0, "coreutils 9.4 did not build; SKIPPED"), "skipped"),
        (verdict(1, "DIFF case 3"), "caught"),
        (verdict(101, "error[E0308]: mismatched types\n"), "unviable"),
        (verdict(1, "warning: unused\nerror: could not compile `curses`\n"), "unviable"),
        (safe_name("a/b.rs:1:2: replace < with <="), "a_b.rs_1_2_replace_with_"),
        (applicable("--- src/a.rs\n+++ replace > with == in f\n@@ -1 +1 @@\n"),
         "--- src/a.rs\n+++ src/a.rs\n@@ -1 +1 @@\n"),
    ]
    bad = 0
    for got, want in checks:
        if got != want:
            print(f"mutants-replay self-test: got {got!r}, want {want!r}", file=sys.stderr)
            bad += 1
    with tempfile.TemporaryDirectory() as d:
        p = Path(d) / "replay.txt"
        p.write_text("missed - one\n", encoding="utf-8", newline="")
        if done_names(p.read_text(encoding="utf-8")) != {"one"}:
            print("mutants-replay self-test: replay.txt not read back", file=sys.stderr)
            bad += 1
    print("mutants-replay self-test: " + ("ok" if not bad else f"{bad} failed"))
    return 1 if bad else 0


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--tree", required=True, type=Path,
                    help="a worktree of its own, reset after every mutant")
    ap.add_argument("mutants_dir", type=Path, help="one package's cargo-mutants output")
    ap.add_argument("harness", nargs="+", help="harness scripts, relative to --tree")
    a = ap.parse_args(argv[1:])
    return replay(a.tree.resolve(), a.mutants_dir.resolve(), a.harness)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
