#!/usr/bin/env python3
"""Replay the mutants `cargo test` could not kill through differential harnesses.

Usage:
    python scripts/mutants-replay.py --tree DIR [--workspace REL] [--limit SECS]
                                     MUTANTS_DIR HARNESS...
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

    caught <harness> <mutant>   the harness reported a difference, twice running
    flaky <harness> <mutant>    it differed once, then agreed: look by hand
    hung <harness> <mutant>     the harness ran out of time: look by hand
    missed - <mutant>           every harness agreed: a gap in the tests
    unviable - <mutant>         the mutant does not compile for Linux
    stale - <mutant>            the diff no longer applies to DIR's commit

"Caught" needs the harness to SAY it found a difference (a `DIFF` line,
curses-diff's "stopped at the first difference", "N differ" with N above
zero): a harness that failed for any other reason -- a fixture it could not
build, WSL gone, a full disk -- stops the run, rather than passing for a
catch. A skip stops it too, rather than passing for agreement.

The file is appended to as the run goes, and a mutant already in it is not
run again, so an interrupted run carries on where it stopped. Each harness's
output is kept in `MUTANTS_DIR/replay-logs/`. Once a harness has grown to
reach what it missed, `--again replay.txt --record replay-2.txt` replays
only the mutants the first record calls missed, into a second record.

DIR must be a worktree of its own, made for this (`git worktree add --detach
DIR <commit>`): its changes are thrown away after every mutant, and the run
refuses to start unless `git status` there is clean. The harness builds DIR's
copy of the code, so DIR's commit is the code the mutants are applied to; a
mutant whose lines have changed since the review is `stale`, and the next
review meets the new lines anyway. The diffs name files relative to the
Cargo workspace cargo-mutants ran in, which `review-batch.py` passes as
`-d`: for a workspace other than the repository's root, say which with
`--workspace`.

Time: each harness is bounded where it runs. `diff-wsl.sh` re-execs it in WSL
under `timeout(1)` for `DIFF_TIMEOUT` seconds, which is set here to `--limit`
-- nothing on the Windows side can kill what runs in WSL (`run-timeout.py`'s
docstring). A harness that has not come back long after that stops the run,
because its WSL half may still be reading a tree the next mutant would
change.

Nothing may be built beside a QEMU guest: the run waits for one to finish
before each mutant.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gitenv  # noqa: E402
import msysbash  # noqa: E402
import selftestflag  # noqa: E402

#: The Windows images a QEMU guest runs as.
QEMU_IMAGES = ("qemu-system-x86_64.exe", "qemu-system-x86_64w.exe")

#: Seconds a harness may take on one mutant, by default: `DIFF_TIMEOUT`.
LIMIT = 90 * 60

#: How much longer than its limit a harness may stay away before the run
#: gives up on it: `timeout -k 10`, and the time to come back out of WSL.
GRACE = 5 * 60

#: The first word of each line of `replay.txt`.
VERDICTS = ("caught", "flaky", "hung", "missed", "unviable", "stale")

#: What a harness prints when it skips rather than compares.
SKIP = re.compile(r"(?i)\bskipp(?:ing|ed)\b")
#: What rustc prints when the mutant does not compile.
RUSTC_ERROR = re.compile(r"^error(?:\[E\d+\])?: ", re.MULTILINE)
#: What a harness prints when it found a difference.
DIFFERENCE = re.compile(
    r"^DIFF\b|stopped at the first difference|\b[1-9]\d* (?:case\(s\) )?differ\b",
    re.MULTILINE,
)
#: `timeout(1)`'s status when the time ran out (124), or the signal it sent
#: when the `-k` grace ran out too (128 + 9).
TIMED_OUT = (124, 137)


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


def verdict(rc: int, output: str) -> str:
    """What a harness that exited `rc` having printed `output` found:
    `agreed`, `caught`, `hung`, `unviable`, or -- for the run to stop on --
    `skipped` or `broken`."""
    if rc == 0:
        return "skipped" if SKIP.search(output) else "agreed"
    if RUSTC_ERROR.search(output):
        return "unviable"
    if DIFFERENCE.search(output):
        return "caught"
    if rc in TIMED_OUT:
        return "hung"
    return "broken"


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


class Abandoned(Exception):
    """A harness stayed away past its limit and the grace after it."""


def run_harness(bash: str, tree: Path, harness: str, log: Path, limit: int) -> str:
    """Run `harness` from `tree`, stopping at its first difference and bounded
    in WSL by `limit` seconds; keep its output in `log` and say what it found
    (`verdict`)."""
    env = dict(os.environ, DIFF_FAIL_FAST="1", DIFF_TIMEOUT=str(limit))
    try:
        r = subprocess.run([bash, harness], cwd=tree, capture_output=True, text=True,
                           encoding="utf-8", errors="replace", env=env,
                           timeout=limit + GRACE)
    except subprocess.TimeoutExpired as e:
        out = e.stdout if isinstance(e.stdout, str) else ""
        log.write_text(out + "\n(never came back)\n", encoding="utf-8", newline="\n")
        raise Abandoned(harness) from e
    output = r.stdout + r.stderr
    log.write_text(output, encoding="utf-8", newline="\n")
    return verdict(r.returncode, output)


def still_missed(text: str) -> set[str]:
    """The mutants a `replay.txt` says every harness missed."""
    names = set()
    for line in text.splitlines():
        parts = line.split(" ", 2)
        if len(parts) == 3 and parts[0] == "missed":
            names.add(parts[2])
    return names


def replay(tree: Path, mutants_dir: Path, harnesses: list[str], workspace: str,
           limit: int, record_name: str = "replay.txt", again: str = "") -> int:
    mo = mutants_dir / "mutants.out"
    outcomes = json.loads((mo / "outcomes.json").read_text(encoding="utf-8"))
    todo = missed_mutants(outcomes)
    if again:
        # Only those an earlier replay's record says every harness missed:
        # to measure harnesses grown since.
        earlier = still_missed((mutants_dir / again).read_text(encoding="utf-8"))
        todo = [m for m in todo if m[0] in earlier]
    record = mutants_dir / record_name
    done = done_names(record.read_text(encoding="utf-8")) if record.exists() else set()
    logs = mutants_dir / "replay-logs"
    logs.mkdir(exist_ok=True)
    status = git(tree, "status", "--porcelain")
    if status.returncode != 0 or status.stdout.strip():
        print(f"mutants-replay: {tree} is not a clean worktree", file=sys.stderr)
        return 2
    bash = msysbash.find_msys_bash() if os.name == "nt" else "bash"
    if bash is None:
        print("mutants-replay: no Git for Windows bash to run the harnesses with",
              file=sys.stderr)
        return 2
    apply_args = ["apply", "-p0"] + ([f"--directory={workspace}"] if workspace else [])
    left = [m for m in todo if m[0] not in done]
    print(f"mutants-replay: {len(todo)} missed, {len(left)} still to replay", flush=True)
    try:
        if left:
            # Unmutated, every harness must agree: otherwise each mutant would
            # be "caught" by whatever already differs.
            wait_for_no_qemu()
            for h in harnesses:
                log = logs / f"baseline.{Path(h).stem}.log"
                v = run_harness(bash, tree, h, log, limit)
                if v != "agreed":
                    print(f"mutants-replay: {h} does not agree on the unmutated tree ({v}); "
                          f"see {log}", file=sys.stderr)
                    return 2
            print("mutants-replay: the unmutated tree agrees", flush=True)
        for n, (name, diff) in enumerate(left, 1):
            wait_for_no_qemu()
            patch = logs / "current.diff"
            patch.write_text(applicable((mo / diff).read_text(encoding="utf-8")),
                             encoding="utf-8", newline="\n")
            applied = git(tree, *apply_args, str(patch))
            if applied.returncode != 0:
                line = f"stale - {name}"
            else:
                line = f"missed - {name}"
                for h in harnesses:
                    base = logs / f"{safe_name(name)}.{Path(h).stem}"
                    v = run_harness(bash, tree, h, base.with_suffix(".log"), limit)
                    if v == "caught":
                        # A difference the machine's load caused would not
                        # come back: the mutant must be caught twice.
                        second = run_harness(bash, tree, h, base.with_suffix(".again.log"), limit)
                        v = "caught" if second == "caught" else "flaky"
                    if v in ("skipped", "broken"):
                        git(tree, "reset", "--hard", "-q")
                        print(f"mutants-replay: {h} {v} rather than compared on {name}; "
                              f"stopping (see {base}.log)", file=sys.stderr)
                        return 2
                    if v in ("caught", "flaky", "hung"):
                        line = f"{v} {h} {name}"
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
    except Abandoned as e:
        git(tree, "reset", "--hard", "-q")
        print(f"mutants-replay: {e} did not come back in {limit + GRACE} s; stopping -- "
              "its WSL half may still be running, so check before the next run",
              file=sys.stderr)
        return 2
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
        (done_names("caught h.sh a.rs:1:1: replace x\nstale - b c d\nhung h.sh e\nnoise\n"),
         {"a.rs:1:1: replace x", "b c d", "e"}),
        (still_missed("missed - a b\ncaught h.sh c\nmissed - d\n"), {"a b", "d"}),
        (verdict(0, "curses-diff: 990 case(s) agree"), "agreed"),
        (verdict(0, "curses-diff: no WSL on this host; skipping"), "skipped"),
        (verdict(0, "coreutils 9.4 did not build; SKIPPED"), "skipped"),
        (verdict(1, "DIFF -b -n 1 [main]\n"), "caught"),
        (verdict(1, "curses-diff: stopped at the first difference: hello/vt100/C/file"),
         "caught"),
        (verdict(1, "slabtop-diff: 200 case(s) agree, 3 differ, 0 differ on purpose"),
         "caught"),
        # A harness that failed without finding anything is not a catch.
        (verdict(1, "top-diff: mkworld failed\n"), "broken"),
        (verdict(1, "x-diff: 9 case(s) agree, 0 differ\n"), "broken"),
        (verdict(124, "building...\n"), "hung"),
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
    ap.add_argument("--workspace", default="",
                    help="the Cargo workspace the mutants were found in, relative to "
                         "the tree, when it is not the tree's root")
    ap.add_argument("--limit", type=int, default=LIMIT,
                    help="seconds each harness may run on one mutant (DIFF_TIMEOUT)")
    ap.add_argument("--record", default="replay.txt",
                    help="the record's name in MUTANTS_DIR")
    ap.add_argument("--again", default="",
                    help="replay only the mutants this earlier record (in MUTANTS_DIR) "
                         "says every harness missed")
    ap.add_argument("mutants_dir", type=Path, help="one package's cargo-mutants output")
    ap.add_argument("harness", nargs="+", help="harness scripts, relative to --tree")
    a = ap.parse_args(argv[1:])
    if a.again and a.again == a.record:
        ap.error("--again and --record must name different records")
    return replay(a.tree.resolve(), a.mutants_dir.resolve(), a.harness, a.workspace, a.limit,
                  a.record, a.again)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
