#!/usr/bin/env python3
"""Record and diff the kernel micro-benchmark scorecard across boots.

Why this exists
---------------
`bench/baselines.toml` holds absolute nanosecond targets taken from Linux
publications and from `design.txt`.  Under QEMU's TCG interpreter those targets
are unreachable by construction: every guest memory access carries a softmmu
lookup costing a few hundred host cycles where real hardware would take an L1
hit at 1-4 cycles, so the suite routinely reports 10-400x "ABOVE TARGET" on
code that is perfectly correct.  A whole investigation was burned on exactly
that confusion (known-issues.md,
TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT): five boots of
"ownership tagging costs 8500 cycles" turned out to be the emulator, not the
code.

`boot-test.sh` already tells the reader to "compare against prior runs rather
than treating this as a hard regression" -- but nothing stored the prior runs,
so that advice was unfollowable.  This script stores them.

The useful signal under emulation is not "measured vs. hardware target", it is
**"measured vs. the same benchmark on the same host last time"**.  That
comparison cancels the emulation constant, which is the one thing an absolute
target cannot do.  A change that doubles a benchmark shows up as +100% here
regardless of how slow the emulator is in absolute terms.

Usage
-----
    python scripts/bench-history.py --serial build/serial-test.txt
    python scripts/bench-history.py --serial build/serial-test.txt --no-record
    python scripts/bench-history.py --list

Input format
------------
Parses the machine-readable line that `kernel/src/bench.rs::print_scorecard`
emits for *every* scorecard entry (not just the failures):

    [bench] SCORE <name> <measured_ns> <target_ns> <PASS|OVER>

History is appended to `bench/history.jsonl` as JSON-lines -- one JSON object
per boot -- per the project's "no binary logs" rule.  Records carry the host
name and the git commit, and diffs are only ever taken against the most recent
previous record **from the same host**, because numbers from two different
machines (or two different QEMU builds) are not comparable at all.

Exit codes: 0 normally.  With --fail-on-regression, 1 if any benchmark
regressed by more than the threshold.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import platform
import re
import statistics
import subprocess
import sys

# `[bench] SCORE <name> <measured_ns> <target_ns> <PASS|OVER>`
SCORE_RE = re.compile(
    r"^\[bench\]\s+SCORE\s+(\S+)\s+(\d+)\s+(\d+)\s+(PASS|OVER)\s*$"
)

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_SERIAL = os.path.join(REPO_ROOT, "build", "serial-test.txt")
DEFAULT_HISTORY = os.path.join(REPO_ROOT, "bench", "history.jsonl")


def parse_serial(path):
    """Extract {name: (measured_ns, target_ns, verdict)} from a serial log.

    Returns an empty dict if the log has no scorecard, which is the normal
    case for a boot run without `--bench`.
    """
    entries = {}
    try:
        # The serial log is written by QEMU and can contain stray bytes if a
        # boot is killed mid-write, so decode leniently rather than failing.
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                match = SCORE_RE.match(line.strip())
                if match:
                    name, measured, target, verdict = match.groups()
                    entries[name] = (int(measured), int(target), verdict)
    except FileNotFoundError:
        print(f"bench-history: no serial log at {path}", file=sys.stderr)
        return {}
    except OSError as exc:
        print(f"bench-history: cannot read {path}: {exc}", file=sys.stderr)
        return {}
    return entries


def git_commit():
    """Short HEAD hash, or 'unknown' outside a working repo."""
    try:
        out = subprocess.run(
            ["git", "-C", REPO_ROOT, "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True, timeout=15, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return "unknown"
    if out.returncode != 0:
        return "unknown"
    return out.stdout.strip() or "unknown"


def load_history(path):
    """Read history.jsonl, skipping any record that fails to parse.

    A corrupt line must not destroy the rest of the history: this file is
    appended to by every benchmark boot and is the only longitudinal record we
    have, so partial recovery beats an exception.
    """
    records = []
    try:
        with open(path, "r", encoding="utf-8") as handle:
            for lineno, line in enumerate(handle, 1):
                line = line.strip()
                if not line:
                    continue
                try:
                    records.append(json.loads(line))
                except json.JSONDecodeError:
                    print(
                        f"bench-history: skipping malformed record at "
                        f"{path}:{lineno}",
                        file=sys.stderr,
                    )
    except FileNotFoundError:
        return []
    except OSError as exc:
        print(f"bench-history: cannot read {path}: {exc}", file=sys.stderr)
        return []
    return records


def append_record(path, record):
    """Append one JSON-lines record, creating bench/ if needed.

    `newline="\\n"` is not incidental: Python's text mode would translate to
    CRLF on Windows, and this file is appended to by every benchmark boot and
    committed to git. Mixed line endings in an append-only log are exactly the
    kind of thing that produces phantom whole-file diffs later.
    """
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "a", encoding="utf-8", newline="\n") as handle:
            handle.write(json.dumps(record, sort_keys=True) + "\n")
    except OSError as exc:
        print(f"bench-history: cannot write {path}: {exc}", file=sys.stderr)
        return False
    return True


def previous_for_host(records, host):
    """Most recent record from the same host, or None.

    Cross-host comparison is meaningless here -- a different machine or QEMU
    build moves every number at once -- so we would rather report "no baseline"
    than report a diff that is really a hardware difference.
    """
    for record in reversed(records):
        if record.get("host") == host:
            return record
    return None


#: Below this many comparable benchmarks the median is not a trustworthy
#: estimate of the run's global speed factor, so we skip normalisation and
#: compare raw.  A handful of benchmarks can genuinely all move together.
MIN_SAMPLES_FOR_DRIFT = 8


def global_drift(previous_entries, current):
    """Estimate this run's whole-suite speed factor vs. the previous run.

    Returns the **median** of every benchmark's ratio, or `None` when there are
    too few comparable benchmarks for it to mean anything.

    Why this is needed on top of run-over-run comparison
    ---------------------------------------------------
    The module docstring says run-over-run "cancels the emulation constant".
    That is true across *hosts* but not across *runs on the same host*: TCG is
    pure emulation and therefore CPU-bound, so whatever else the machine was
    doing during a run scales the entire suite by a common factor.  A real
    measurement of that: the 2026-08-14 run recorded a +6.1% median with 48 of
    63 benchmarks slower, against a diff that touched only `sys_thread_join`'s
    ABI -- code that not one of the flagged benchmarks executes.

    A fixed absolute threshold cannot survive that.  Shift a distribution whose
    own per-benchmark wobble reaches ~20% by a further 6% and its tail crosses
    25%, so the comparator names six "REGRESSED" benchmarks that did not
    change.  The tell is that the sorted tail was a smooth continuum --
    24.4, 24.5, 24.6, 24.9, 26.3, 27.2, 27.6 -- with no gap anywhere near the
    threshold: a real regression is a few outliers standing clear of a ~0%
    median, not a slice taken out of the middle of a distribution.

    The median (not the mean) is the estimator because it is unaffected by a
    genuine regression in a minority of benchmarks -- which is precisely the
    signal we must not subtract away.  Dividing each ratio by it leaves the
    residual: how a benchmark moved *relative to its peers on the same run*.
    """
    ratios = [
        measured / before
        for name, measured in current.items()
        if (before := previous_entries.get(name)) and before > 0
    ]
    if len(ratios) < MIN_SAMPLES_FOR_DRIFT:
        return None
    return statistics.median(ratios)


def diff(previous, current, threshold_pct):
    """Split benchmarks into regressed / improved / added / removed.

    `threshold_pct` is deliberately coarse.  Even run-over-run on one host the
    in-kernel harness is noisy: it runs as a deferred low-priority task on a
    live system, so a 10-20% wobble carries no information.

    The threshold is applied to the **drift-corrected** change (see
    `global_drift`), so a run where the whole machine was busy does not report
    its tail as a regression.  Each entry carries both numbers: the raw change
    (what the reader would otherwise compute by hand) and the corrected one
    that the decision was actually made on.

    Returns `(regressed, improved, added, removed, drift)` where each
    regressed/improved entry is `(name, before, after, raw_change, adj_change)`.
    """
    regressed, improved, added = [], [], []
    prev_entries = previous.get("entries", {})
    drift = global_drift(prev_entries, current)

    for name, measured in sorted(current.items()):
        before = prev_entries.get(name)
        if before is None:
            added.append((name, measured))
            continue
        if before <= 0:
            continue
        raw_change = (measured - before) * 100.0 / before
        if drift:
            adj_change = ((measured / before) / drift - 1.0) * 100.0
        else:
            adj_change = raw_change
        if adj_change >= threshold_pct:
            regressed.append((name, before, measured, raw_change, adj_change))
        elif adj_change <= -threshold_pct:
            improved.append((name, before, measured, raw_change, adj_change))

    removed = sorted(set(prev_entries) - set(current))
    return regressed, improved, added, removed, drift


def report(previous, current_entries, threshold_pct):
    """Print the run-over-run comparison. Returns True if anything regressed."""
    current = {name: vals[0] for name, vals in current_entries.items()}

    if previous is None:
        print(
            f"=== Benchmark history: first record for this host "
            f"({len(current)} benchmarks); no baseline to compare against ==="
        )
        return False

    regressed, improved, added, removed, drift = diff(
        previous, current, threshold_pct
    )

    print(
        f"=== Benchmark history: {len(current)} benchmarks vs "
        f"{previous.get('timestamp', '?')} (commit {previous.get('commit', '?')}) ==="
    )
    print(
        "  Comparison is run-over-run on this host, which cancels the TCG "
        "emulation constant."
    )
    print(
        "  (The 'target' column in the scorecard above is a *hardware* "
        "reference and cannot be met under TCG -- see bench/baselines.toml.)"
    )

    if drift:
        drift_pct = (drift - 1.0) * 100.0
        print(
            f"  Whole-suite drift this run: {drift_pct:+.1f}% (median of all "
            f"{len(current)} benchmarks)."
        )
        if abs(drift_pct) >= 15.0:
            print(
                "  !! That is large. TCG is CPU-bound, so a busy machine scales "
                "every benchmark"
            )
            print(
                "     at once -- check nothing else was building/booting, and "
                "prefer re-running"
            )
            print("     before acting on anything below.")
        print(
            "  Percentages below are drift-corrected (raw change in "
            "parentheses); only a"
        )
        print(
            "  benchmark that moved relative to its peers is reported. See "
            "global_drift()."
        )

    if regressed:
        print(f"  REGRESSED (>{threshold_pct:g}% slower than the suite):")
        for name, before, after, raw, adj in sorted(
            regressed, key=lambda r: -r[4]
        ):
            print(
                f"    {name}: {before}ns -> {after}ns "
                f"({adj:+.0f}% vs suite, {raw:+.0f}% raw)"
            )
    if improved:
        print(f"  IMPROVED (>{threshold_pct:g}% faster than the suite):")
        for name, before, after, raw, adj in sorted(improved, key=lambda r: r[4]):
            print(
                f"    {name}: {before}ns -> {after}ns "
                f"({adj:+.0f}% vs suite, {raw:+.0f}% raw)"
            )
    if added:
        print("  NEW:")
        for name, measured in added:
            print(f"    {name}: {measured}ns")
    if removed:
        print("  GONE (present last run, absent now):")
        for name in removed:
            print(f"    {name}")
    if not (regressed or improved or added or removed):
        if drift:
            print(
                f"  No benchmark moved by more than {threshold_pct:g}% "
                f"relative to the suite."
            )
        else:
            print(f"  No benchmark moved by more than {threshold_pct:g}%.")

    return bool(regressed)


def cmd_list(history_path):
    """Print a one-line summary of every stored record."""
    records = load_history(history_path)
    if not records:
        print(f"bench-history: no records in {history_path}")
        return 0
    for record in records:
        entries = record.get("entries", {})
        over = record.get("over_target", "?")
        print(
            f"{record.get('timestamp', '?')}  {record.get('host', '?'):<20} "
            f"{record.get('commit', '?'):<12} {len(entries):>3} benchmarks, "
            f"{over} over hardware target"
        )
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Record and diff kernel benchmark scorecards across boots."
    )
    parser.add_argument("--serial", default=DEFAULT_SERIAL,
                        help="serial log to parse (default: build/serial-test.txt)")
    parser.add_argument("--history", default=DEFAULT_HISTORY,
                        help="JSON-lines history file (default: bench/history.jsonl)")
    parser.add_argument("--threshold", type=float, default=25.0,
                        help="percent change worth reporting (default: 25)")
    parser.add_argument("--no-record", action="store_true",
                        help="compare only; do not append a new record")
    parser.add_argument("--fail-on-regression", action="store_true",
                        help="exit 1 if any benchmark regressed past the threshold")
    parser.add_argument("--list", action="store_true",
                        help="list stored records and exit")
    args = parser.parse_args(argv)

    if args.list:
        return cmd_list(args.history)

    current_entries = parse_serial(args.serial)
    if not current_entries:
        # Not an error: most boots run without --bench and emit no scorecard.
        print("bench-history: no scorecard in serial log (boot without --bench?)")
        return 0

    host = platform.node() or "unknown"
    records = load_history(args.history)
    previous = previous_for_host(records, host)

    regressed = report(previous, current_entries, args.threshold)

    if not args.no_record:
        record = {
            "timestamp": datetime.datetime.now(
                datetime.timezone.utc
            ).replace(microsecond=0).isoformat(),
            "host": host,
            "commit": git_commit(),
            # Only the measured number is worth storing per benchmark; the
            # target is static and already lives in baselines.toml.
            "entries": {n: v[0] for n, v in current_entries.items()},
            "over_target": sum(
                1 for v in current_entries.values() if v[2] == "OVER"
            ),
        }
        if append_record(args.history, record):
            print(f"  Recorded {len(current_entries)} benchmarks to "
                  f"{os.path.relpath(args.history, REPO_ROOT)}")

    if regressed and args.fail_on_regression:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
