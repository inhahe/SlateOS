#!/usr/bin/env python3
"""What each boot-test gate has cost, against what it has caught.

Run: `python scripts/gate-cost-report.py [--since YYYY-MM-DD] [--top N]`
     `python scripts/gate-cost-report.py --self-test`

Why this exists
---------------

The operator asked, answering C-Q11 on 2026-09-27, how much the checks cost
under the load this machine actually runs at, against how much they have saved
the times they caught something that nothing else would have. A gate that
costs minutes every boot and has never refused anything is a candidate for
the gate cache, for a narrower scope, or for retirement. A gate that is cheap
and refuses often is doing its job. Neither can be told from the gate's code.
Both can be told from the runs, and every boot already records them:
`run_checker` appends `<label>\\t<seconds>\\t<exit>` for each gate to
`boot-test-gate-timing.<pid>.tsv` in its worktree's git directory
(`CHECKER_TIMING_LOG`, scripts/run-checker.sh). This report reads every such
file in every worktree and totals them per gate.

What it counts, and what it refuses to count
--------------------------------------------

* **Only the boot test's own gates.** Test suites that run a fixture copy of
  the push hook inherit the boot's `CHECKER_TIMING_LOG`, so the files also hold
  the fixtures' rows (`doc-links-<sha>`, `raced-global-<sha>`, ...), whose
  refusals are the fixtures doing their job. A row counts only when its label
  is one `scripts/boot-test.sh` itself passes to `run_checker`; the others are
  tallied separately so the exclusion is visible, not silent.
* **A refusal is exit 1**, run_checker's "finding". It is a catch, or a false
  alarm; the report cannot tell which, and lists the dates so a person can.
  Exit 2 and 3 are declines (no verdict); anything else is a crash.
* **Cost is wall clock under whatever load the run had.** That is the number
  the operator asked about. It is not the gate's cost on an idle machine, and
  the median is reported beside the total for that reason.

Exit status: 0 always (it is a report), 1 if its self-test fails, 2 if it
found no timing files at all -- which is no answer, not an empty one.
"""

from __future__ import annotations

import argparse
import datetime as dt
import glob
import os
import re
import statistics
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

#: A per-sha label (`doc-links-<40 hex>`): only the push hook loops over shas.
SHA_LABEL = re.compile(r"-[0-9a-f]{40}$")


def boot_labels(boot_test_text: str) -> set[str]:
    """The labels scripts/boot-test.sh passes to run_checker, continuation
    lines joined."""
    text = boot_test_text.replace("\\\n", " ")
    out = set()
    for m in re.finditer(r"run_checker\s+(?:--may-skip\s+)?([A-Za-z0-9_.\-]+)", text):
        out.add(m.group(1))
    return out


def timing_files(common_dir: str) -> list[str]:
    """Every gate-timing file in the repository's git dir and its worktrees'."""
    pats = [os.path.join(common_dir, "boot-test-gate-timing.*.tsv"),
            os.path.join(common_dir, "worktrees", "*", "boot-test-gate-timing.*.tsv")]
    out: list[str] = []
    for p in pats:
        out.extend(glob.glob(p))
    return sorted(out)


class Gate:
    __slots__ = ("label", "secs", "refusals", "declines", "crashes", "lanes")

    def __init__(self, label: str) -> None:
        self.label = label
        self.secs: list[int] = []
        self.refusals: list[str] = []   # "<date> <worktree>" per exit-1 run
        self.declines = 0
        self.crashes = 0
        self.lanes: set[str] = set()


def tally(files: list[str], labels: set[str], since: dt.date | None
          ) -> tuple[dict[str, Gate], int, int, int]:
    """Per-gate totals over `files`. Returns (gates, runs, fixture_rows,
    unparsed_rows)."""
    gates: dict[str, Gate] = {}
    runs = fixture = bad = 0
    for path in files:
        when = dt.date.fromtimestamp(os.path.getmtime(path))
        if since is not None and when < since:
            continue
        wt = os.path.basename(os.path.dirname(path))
        try:
            with open(path, encoding="utf-8", errors="replace") as fh:
                rows = [ln.rstrip("\n").split("\t") for ln in fh if ln.strip()]
        except OSError:
            continue
        counted = False
        for r in rows:
            if len(r) < 3 or not r[1].isdigit() or not r[2].lstrip("-").isdigit():
                bad += 1
                continue
            label, secs, rc = r[0], int(r[1]), int(r[2])
            if label not in labels or SHA_LABEL.search(label):
                fixture += 1
                continue
            counted = True
            g = gates.setdefault(label, Gate(label))
            g.secs.append(secs)
            g.lanes.add(wt)
            if rc == 1:
                g.refusals.append(f"{when.isoformat()} {wt}")
            elif rc in (2, 3):
                g.declines += 1
            elif rc != 0:
                g.crashes += 1
        runs += counted
    return gates, runs, fixture, bad


def report(gates: dict[str, Gate], runs: int, fixture: int, bad: int,
           top: int) -> str:
    lines = []
    total = sum(sum(g.secs) for g in gates.values())
    lines.append(f"{runs} boot run(s), {len(gates)} gate label(s), "
                 f"{total / 3600:.1f} gate-hours in total; {fixture} fixture "
                 f"row(s) excluded, {bad} unparsed.")
    lines.append("")
    lines.append(f"{'gate':44s} {'runs':>5s} {'median':>7s} {'total':>8s} "
                 f"{'refused':>8s} {'declined':>8s} {'crashed':>8s}  last refusal")
    ordered = sorted(gates.values(), key=lambda g: -sum(g.secs))
    for g in ordered[:top] if top else ordered:
        last = g.refusals[-1] if g.refusals else "-"
        lines.append(
            f"{g.label[:44]:44s} {len(g.secs):5d} "
            f"{statistics.median(g.secs):6.0f}s {sum(g.secs) / 3600:7.2f}h "
            f"{len(g.refusals):8d} {g.declines:8d} {g.crashes:8d}  {last}")
    never = [g for g in ordered if not g.refusals]
    lines.append("")
    lines.append(f"{len(never)} of {len(gates)} gate(s) never refused in these runs; "
                 f"together they cost {sum(sum(g.secs) for g in never) / 3600:.2f} "
                 f"of the {total / 3600:.2f} gate-hours.")
    return "\n".join(lines)


def _self_test() -> int:
    fails: list[str] = []

    def check(label: str, got: object, want: object) -> None:
        if got == want:
            print(f"  ok    {label}")
        else:
            print(f"  FAIL  {label}: got {got!r}, want {want!r}")
            fails.append(label)

    boot = ('run_checker alpha "$py" x.py\n'
            'run_checker --may-skip beta "$py" \\\n  y.py\n'
            'run_checker gamma-selftest "$py" z.py --self-test\n')
    check("boot labels, continuation joined, --may-skip skipped",
          sorted(boot_labels(boot)), ["alpha", "beta", "gamma-selftest"])

    with tempfile.TemporaryDirectory() as tmp:
        wt = os.path.join(tmp, "worktrees", "os-lane-x")
        os.makedirs(wt)
        sha = "a" * 40
        with open(os.path.join(wt, "boot-test-gate-timing.1.tsv"), "w",
                  encoding="utf-8", newline="\n") as fh:
            fh.write("alpha\t10\t0\nbeta\t5\t1\n"
                     f"doc-links-{sha}\t30\t1\nunknown\t7\t0\nbroken line\n"
                     "gamma-selftest\t2\t3\n")
        with open(os.path.join(tmp, "boot-test-gate-timing.2.tsv"), "w",
                  encoding="utf-8", newline="\n") as fh:
            fh.write("alpha\t20\t0\nbeta\t5\t0\nalpha\t1\t139\n")
        files = timing_files(tmp)
        check("finds files in the git dir and in worktrees", len(files), 2)
        gates, runs, fixture, bad = tally(files, boot_labels(boot), None)
        check("two runs counted", runs, 2)
        check("per-sha and unknown labels excluded as fixture rows", fixture, 2)
        check("a malformed row is counted as unparsed, not dropped", bad, 1)
        check("alpha's three runs", sorted(gates["alpha"].secs), [1, 10, 20])
        check("a refusal is exit 1", len(gates["beta"].refusals), 1)
        check("exit 3 is a decline", gates["gamma-selftest"].declines, 1)
        check("any other exit is a crash", gates["alpha"].crashes, 1)
        out = report(gates, runs, fixture, bad, 0)
        check("the report names the fixture exclusion", "2 fixture row(s) excluded" in out, True)
        check("never-refused gates are summed", "2 of 3 gate(s) never refused" in out, True)
    if fails:
        print(f"gate-cost-report: {len(fails)} FAILURE(S)")
        return 1
    print("gate-cost-report: self-test passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--since", metavar="YYYY-MM-DD",
                    help="only runs whose timing file is this recent")
    ap.add_argument("--top", type=int, default=40,
                    help="rows to print, by total cost (0 = all; default 40)")
    ap.add_argument("--self-test", "--selftest", "--self_test", dest="selftest",
                    action="store_true", help="run the fixtures and exit")
    args = ap.parse_args(argv)
    if args.selftest:
        return _self_test()
    since = dt.date.fromisoformat(args.since) if args.since else None
    common = subprocess.run(["git", "-C", ROOT, "rev-parse", "--git-common-dir"],
                            capture_output=True, text=True).stdout.strip()
    if not common:
        print("gate-cost-report: not in a git repository -- no answer.", file=sys.stderr)
        return 2
    common = os.path.abspath(os.path.join(ROOT, common))
    files = timing_files(common)
    if not files:
        print(f"gate-cost-report: no timing files under {common} -- no answer.",
              file=sys.stderr)
        return 2
    with open(os.path.join(HERE, "boot-test.sh"), encoding="utf-8") as fh:
        labels = boot_labels(fh.read())
    gates, runs, fixture, bad = tally(files, labels, since)
    print(report(gates, runs, fixture, bad, args.top))
    return 0


if __name__ == "__main__":
    sys.exit(main())
