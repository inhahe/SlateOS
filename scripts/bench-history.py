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

# `[bench] SCORE <name> <measured_ns> <target_ns> <PASS|OVER> [<mean_ns> <iters>]`
#
# The trailing pair is optional because it was added after the history file
# already had records in it: logs written before the kernel emitted it must
# still parse, or the one longitudinal record we have gets truncated at the
# point of the change. Absent, dispersion is simply unknown for that run.
SCORE_RE = re.compile(
    r"^\[bench\]\s+SCORE\s+(\S+)\s+(\d+)\s+(\d+)\s+(PASS|OVER)"
    r"(?:\s+(\d+)\s+(\d+))?\s*$"
)

# `[bench] CANARY <start> <end> <pct> [<min> <max> <spread> <samples>]`
#
# The reference memory-access cost. `start`/`end`/`pct` are the suite's two
# endpoints, `pct` being `end` as a percentage of `start`.
#
# The trailing four are an append-only extension covering samples taken
# *throughout* the suite. They exist because endpoint-only sampling could not
# fire on the case the canary was built for: its first real run reported the
# endpoints stable to 3% while four benchmarks in that same run sat 40-160%
# above their established values. Endpoints catch a sustained load change; the
# contamination that matters is a transient burst landing on whichever
# benchmark is running at the time.
#
# Optional so the one record written before mid-suite sampling existed still
# parses -- and so a log without any canary at all is *unknown*, not clean.
CANARY_RE = re.compile(
    r"^\[bench\]\s+CANARY\s+(\d+)\s+(\d+)\s+(\d+)"
    r"(?:\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+))?\s*$"
)

# Percent deviation at which a run is called contaminated. Must match
# `CANARY_TOLERANCE_PCT` in `kernel/src/bench.rs`; the kernel prints its own
# verdict, and this recomputes it so a replayed/old log is judged by the same
# rule as a live one.
CANARY_TOLERANCE_PCT = 25

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_SERIAL = os.path.join(REPO_ROOT, "build", "serial-test.txt")
DEFAULT_HISTORY = os.path.join(REPO_ROOT, "bench", "history.jsonl")


def parse_serial(path):
    """Extract {name: (measured_ns, target_ns, verdict, mean_ns, iters)}.

    `mean_ns` and `iters` are `None` for a log predating their emission.

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
                    name, measured, target, verdict, mean, iters = match.groups()
                    entries[name] = (
                        int(measured), int(target), verdict,
                        int(mean) if mean is not None else None,
                        int(iters) if iters is not None else None,
                    )
    except FileNotFoundError:
        print(f"bench-history: no serial log at {path}", file=sys.stderr)
        return {}
    except OSError as exc:
        print(f"bench-history: cannot read {path}: {exc}", file=sys.stderr)
        return {}
    return entries


def parse_canary(path):
    """Extract the contamination canary as a dict, or None.

    Keys: `start`, `end`, `pct` always; `min`, `max`, `spread`, `samples`
    only when the log carries mid-suite sampling.

    None means the log has no canary at all, in which case contamination is
    *unknown* for that run -- materially different from "known clean", and
    callers must not conflate the two.

    The last CANARY line wins, matching `parse_serial`'s last-wins behaviour
    for SCORE, so a concatenated/replayed log reports its final suite.
    """
    result = None
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                match = CANARY_RE.match(line.strip())
                if match:
                    start, end, pct, lo, hi, spread, samples = match.groups()
                    result = {
                        "start": int(start),
                        "end": int(end),
                        "pct": int(pct),
                    }
                    if lo is not None:
                        result.update({
                            "min": int(lo),
                            "max": int(hi),
                            "spread": int(spread),
                            "samples": int(samples),
                        })
    except FileNotFoundError:
        return None
    except OSError as exc:
        print(f"bench-history: cannot read {path}: {exc}", file=sys.stderr)
        return None
    return result


def canary_is_contaminated(canary):
    """True if the canary shows host load changed during the suite.

    Uses the mid-suite spread when the log has it, because that is the only
    figure that can see a transient burst; falls back to the endpoint
    comparison for records written before mid-suite sampling existed.

    None (no canary in the log) is *not* contaminated -- it is unknown. This
    returns False so that old records keep comparing as they always have;
    the caller distinguishes the two by testing for None itself.
    """
    if canary is None:
        return False
    if canary["start"] <= 0:
        return True
    if "spread" in canary:
        return canary["spread"] > CANARY_TOLERANCE_PCT
    return abs(canary["pct"] - 100) > CANARY_TOLERANCE_PCT


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


#: `mean/min` at or above which a benchmark's own number is called unreliable.
#:
#: This is a *reporting* threshold, not a pass/fail gate, and it is deliberately
#: not fitted. Measured over the three records that carry mean data (63
#: benchmarks each): the median benchmark sits at 1.26-1.59 and the great
#: majority are under 2, while excursions land at 5-25x with nothing much in
#: between. 5 sits in that empty band. Only `ipc_channel_sync` is *persistently*
#: above it (6.0/3.9/4.6 across the three runs), i.e. plausibly intrinsic rather
#: than disturbed; every other high reading was spiky, high in one run and ~1.1
#: in another, which is the signature of a transient stall rather than the
#: benchmark's own behaviour.
#:
#: Retune once release-profile records exist -- optimised benchmarks are shorter
#: and so present a smaller target to a burst, which should move this.
DISPERSION_SUSPECT_RATIO = 5.0


def suspect_dispersion(current_entries, ratio=DISPERSION_SUSPECT_RATIO):
    """Benchmarks whose mean/min reaches `ratio`, worst first.

    Returns `[(ratio, name), ...]`. Entries with no recorded mean (logs from
    before the mean_ns extension) are skipped rather than assumed clean: an
    absent measurement is not evidence of a quiet run, which is the same
    distinction the canary's "absent != clean" handling makes.
    """
    suspect = []
    for name, value in current_entries.items():
        measured, mean_ns = value[0], value[3]
        if mean_ns is None or not measured:
            continue
        observed = mean_ns / measured
        if observed >= ratio:
            suspect.append((observed, name))
    suspect.sort(reverse=True)
    return suspect


def report_dispersion(current_entries):
    """Report benchmarks whose own mean/min says they were stalled mid-run.

    Why this exists alongside the canary: the canary samples the host *between*
    benchmarks, roughly once per 8, so a stall confined to one benchmark falls
    between samples and is certified clean. `mean/min` is computed from the
    benchmark's own iterations, so it sees exactly what the canary cannot.

    Note this is not the stronger claim that a flagged number is wrong. A high
    ratio means the run contained large stalls; because the recorded figure is
    the *minimum* over all iterations, the number can still be sound. What it
    rules out is reading such a benchmark's movement as a clean signal.
    """
    suspect = suspect_dispersion(current_entries)
    if not suspect:
        print("  Dispersion OK: every benchmark's mean is within "
              f"{DISPERSION_SUSPECT_RATIO:g}x of its own minimum.")
        return
    print(f"  Dispersion: {len(suspect)} benchmark(s) stalled during their own "
          f"run (mean/min >= {DISPERSION_SUSPECT_RATIO:g}x) - treat any "
          f"movement in these as unproven:")
    for ratio, name in suspect:
        print(f"    {name}: mean is {ratio:.0f}x its min")


#: Profile assumed for records written before the field existed.
#:
#: Every record up to 2026-08-14 was produced by a boot-test.sh that ran a bare
#: `cargo build`, so they are all debug. Defaulting the absent key this way
#: keeps them comparable with each other instead of stranding them.
LEGACY_PROFILE = "debug"


def record_profile(record):
    """Build profile a record was measured on, defaulting old records."""
    return record.get("profile", LEGACY_PROFILE)


def previous_for_host(records, host, profile=LEGACY_PROFILE):
    """Most recent record from the same host *and build profile*, or None.

    Cross-host comparison is meaningless here -- a different machine or QEMU
    build moves every number at once -- so we would rather report "no baseline"
    than report a diff that is really a hardware difference.

    The same argument applies, harder, across build profiles. `opt-level = 0`
    versus `3` on this code is a multiple rather than a percentage, so diffing
    a release run against a debug one would report every benchmark as a
    spectacular improvement and drown any real signal. It is not even rescued
    by the drift correction: that removes a *uniform* factor, and the
    debug-to-release ratio is anything but uniform across the suite.
    """
    for record in reversed(records):
        if record.get("host") == host and record_profile(record) == profile:
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
    parser.add_argument("--profile", default=LEGACY_PROFILE,
                        help="cargo build profile these numbers were measured "
                             "on (default: debug). Records are only ever "
                             "compared within one profile.")
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
    previous = previous_for_host(records, host, args.profile)

    # If there is no same-profile baseline but there *are* same-host records on
    # another profile, say so explicitly. Otherwise the reader sees the generic
    # "no baseline" line and reasonably concludes the history is empty, when in
    # fact it is full of numbers that were deliberately not used.
    if previous is None:
        other = [r for r in records
                 if r.get("host") == host and record_profile(r) != args.profile]
        if other:
            profiles = sorted({record_profile(r) for r in other})
            print(f"  No baseline on the '{args.profile}' profile yet "
                  f"({len(other)} record(s) exist for this host on "
                  f"{', '.join(profiles)}, deliberately not compared: "
                  f"different optimisation level, different numbers).")

    canary = parse_canary(args.serial)
    regressed = report(previous, current_entries, args.threshold)

    # Reported *after* the comparison, so it qualifies the verdict the reader
    # has just seen rather than being buried above it.
    if canary is None:
        print("  Contamination canary: absent (log predates it) - unknown, "
              "not clean.")
    else:
        if "spread" in canary:
            detail = (f"spread {canary['spread']}% over {canary['samples']} "
                      f"samples ({canary['min']}-{canary['max']} cycles)")
        else:
            detail = (f"endpoints {canary['start']} -> {canary['end']} cycles, "
                      f"{canary['pct']}% (no mid-suite sampling in this log)")
        if canary_is_contaminated(canary):
            print(f"  CONTAMINATED: reference access cost {detail}, tolerance "
                  f"{CANARY_TOLERANCE_PCT}%.")
            print("  Host load changed during the run. A single-benchmark "
                  "outlier here is unproven - the drift correction removes a "
                  "uniform factor, and this is not one.")
        else:
            # NOT "host load stable" -- that is a claim the canary cannot
            # support and which was measurably false every time it was made.
            # All three runs that carried dispersion data were certified clean
            # here while each contained 5-8 benchmarks with >=5x in-run
            # dispersion. See known-issues.md
            # B-BENCH-CANARY-CERTIFIES-CLEAN-RUNS-THAT-CONTAIN-MULTI-X-STALLS.
            print(f"  Canary OK: reference access cost steady between "
                  f"benchmarks, {detail}.")
            print("  That is a *sampled* check, ~1 sample per 8 benchmarks. It "
                  "does not mean individual benchmarks ran undisturbed - see "
                  "the dispersion line below.")

    report_dispersion(current_entries)

    if not args.no_record:
        record = {
            "timestamp": datetime.datetime.now(
                datetime.timezone.utc
            ).replace(microsecond=0).isoformat(),
            "host": host,
            # Sibling key, absent on pre-2026-08-14 records, which
            # record_profile() reads as "debug". See LEGACY_PROFILE.
            "profile": args.profile,
            "commit": git_commit(),
            # The target is static and already lives in baselines.toml, so
            # only the measured number goes here.
            "entries": {n: v[0] for n, v in current_entries.items()},
            "over_target": sum(
                1 for v in current_entries.values() if v[2] == "OVER"
            ),
        }
        # Dispersion goes in *sibling* maps rather than by widening `entries`
        # to a dict-of-dicts. history.jsonl is append-only and already holds
        # records without these fields; changing the shape of `entries` would
        # mean every reader had to handle two shapes forever, for no gain over
        # a key that is simply absent on older records.
        mean_ns = {n: v[3] for n, v in current_entries.items() if v[3] is not None}
        iters = {n: v[4] for n, v in current_entries.items() if v[4] is not None}
        if mean_ns:
            record["mean_ns"] = mean_ns
        if iters:
            record["iterations"] = iters
        # Same append-only reasoning: a sibling key, absent on older records.
        # Recorded even when clean, because a stored verdict with no stored
        # measurement could never be re-judged if the tolerance is retuned --
        # and the tolerance is explicitly a placeholder awaiting real data.
        if canary is not None:
            record["canary"] = dict(canary)
            record["contaminated"] = canary_is_contaminated(canary)
        if append_record(args.history, record):
            print(f"  Recorded {len(current_entries)} benchmarks to "
                  f"{os.path.relpath(args.history, REPO_ROOT)}")

    if regressed and args.fail_on_regression:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
