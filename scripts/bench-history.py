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
import math
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
#
# A ninth field, `<invalid>`, counts reference measurements whose two arms
# failed to separate. It is its own field rather than a zero in `min`/`max`
# because "the instrument failed" and "the instrument found nothing" are
# different results: every release-profile run between 2026-08-14T15:57 and
# 20:30 reported a serene 0% spread over 0-0 cycles while measuring nothing at
# all. See known-issues.md
# B-BENCH-CANARY-MEASURES-ZERO-IN-RELEASE-AND-BLAMES-THE-HOST.
# Two further fields, `<min_centi> <max_centi>`, carry the extremes in
# hundredths of a cycle. They exist because `min`/`max` are rounded to whole
# cycles while `spread` is computed at full precision, so a record could state
# both "the extremes were 5 and 7" (a 40% spread) and "spread = 47" -- the
# 2026-08-14T22:1x record does exactly that. Their presence is also the only
# signal that a record's `spread` is trustworthy at all: see canary_verdict.
CANARY_RE = re.compile(
    r"^\[bench\]\s+CANARY\s+(\d+)\s+(\d+)\s+(\d+)"
    r"(?:\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)"
    r"(?:\s+(\d+)(?:\s+(\d+)\s+(\d+))?)?)?\s*$"
)

# Percent deviation at which a run is called contaminated. Must match
# `CANARY_TOLERANCE_PCT` in `kernel/src/bench.rs`; the kernel prints its own
# verdict, and this recomputes it so a replayed/old log is judged by the same
# rule as a live one.
CANARY_TOLERANCE_PCT = 25

#: Smallest per-access cost whose *spread* is measurable at all.
#:
#: Derived, not chosen. The per-access figure is an integer quotient, so its
#: resolution is one cycle; at a minimum of `m` cycles, one cycle of
#: quantisation is `100/m` percent. Once that exceeds the tolerance the spread
#: verdict is reporting rounding, not host load -- so the measurement is
#: unusable below `100 / CANARY_TOLERANCE_PCT` cycles.
#:
#: This is what the 15:57 and 16:16 release records look like: min=1, max=2,
#: "spread 100%". They were classified as *contamination* on the strength of a
#: single cycle of rounding, which is the same category error as calling a dead
#: canary clean -- just in the other direction.
#:
#: CORRECTION 2026-08-14: this comment used to end "the only honest measurement
#: of this same quantity on this same host is 266-309 cycles." That was wrong,
#: and wrong in a way this constant exists to guard against -- 266-309 was a
#: *debug*-profile figure quoted as if it settled the release case. The honest
#: release measurement is **~5 cycles**. Every number in this file that is
#: compared against a per-access cost must therefore be read at that scale.
#:
#: KNOWN LIMITATION, proven 2026-08-14: this bound does *not* make the spread
#: verdict safe, and it was in force when the canary raised a 40% false alarm on
#: a quiet host. It bounds a **one**-cycle rounding at the tolerance, but a
#: spread is taken across *two* samples and so can carry two roundings -- twice
#: the bound. At the real 5-cycle cost that is 40% against a 25% tolerance, so no
#: machine could have passed. Raising this constant is *not* the repair: it would
#: reject the hardware's true cost as unmeasurable. The repair is in the kernel,
#: which now computes the spread in hundredths of a cycle (`CENTI` in
#: `kernel/src/bench.rs`) and so never rounds the verdict into existence.
#:
#: What this constant still does, and why it is kept: the kernel now applies the
#: identical bound to `delta` *before* emitting a sample, so no record written by
#: the current kernel can fail this test -- on live data it is a check that
#: cannot fire. It is retained solely to keep judging the **historical** records
#: written before that filter existed. Deleting it as dead code would silently
#: re-admit the 15:57 and 16:16 records as usable.
CANARY_MIN_RESOLVABLE = math.ceil(100 / CANARY_TOLERANCE_PCT)

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
                    (start, end, pct, lo, hi, spread, samples,
                     invalid, lo_centi, hi_centi) = match.groups()
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
                    if invalid is not None:
                        result["invalid"] = int(invalid)
                    if lo_centi is not None and hi_centi is not None:
                        result["min_centi"] = int(lo_centi)
                        result["max_centi"] = int(hi_centi)
    except FileNotFoundError:
        return None
    except OSError as exc:
        print(f"bench-history: cannot read {path}: {exc}", file=sys.stderr)
        return None
    return result


#: The canary's four possible outcomes. Four rather than two, because
#: "no canary in the log", "the canary could not measure", "the canary
#: measured contamination" and "the canary measured a quiet host" are four
#: different findings and only the last one licenses trusting the run.
CANARY_ABSENT = "absent"
CANARY_BROKEN = "broken"
CANARY_CONTAMINATED = "contaminated"
CANARY_CLEAN = "clean"


def canary_verdict(canary):
    """Classify the canary into one of the four CANARY_* outcomes.

    Uses the mid-suite spread when the log has it, because that is the only
    figure that can see a transient burst; falls back to the endpoint
    comparison for records written before mid-suite sampling existed.

    `broken` is the one that had to be split out. A reference measurement of
    zero cycles is not a fast memory access, it is a failed measurement -- and
    for nine consecutive release-profile runs it was reported as
    *contamination*, sending the reader after host load that was never there
    while the real fault (the optimiser had deleted the store being timed) went
    unnamed. "The instrument failed" is not "the instrument found a problem".

    But "the instrument failed" does not outrank "the instrument found a
    problem" either, and this function used to say it did: any `invalid > 0`
    returned BROKEN before the spread was even looked at. The controlled load
    test (P20) showed what that costs -- under 6 CPU spinners a run had 1 of 10
    measurements fail to separate its arms while the other 9 spread 53%, and
    both the kernel and this function answered "UNKNOWN". The failures are
    evidence *for* contamination there, not against it: noise big enough to
    invert a 5-cycle A/B split is load. So the precedence now matches the
    kernel's `report_canary`: nothing measured at all is BROKEN; an
    over-tolerance spread is CONTAMINATED even alongside failures; only a
    *within*-tolerance spread with failures present is BROKEN, because a failed
    sample is not a quiet one and could have hidden the excursion.
    """
    if canary is None:
        return CANARY_ABSENT
    # Nothing measured at all: there is no finding to report, over-tolerance or
    # otherwise.
    if canary.get("samples") == 0:
        return CANARY_BROKEN
    # A missing start makes `pct` meaningless -- the kernel writes 0, which
    # reads back as a 100% endpoint change -- so on a record with no
    # independent `spread` field there is nothing left to judge. This is the
    # exact shape of the nine dead release records.
    if canary["start"] <= 0 and "spread" not in canary:
        return CANARY_BROKEN
    # A minimum below one cycle of usable resolution means the arms barely
    # separated: `min == 0` is the fully-eliminated case the pre-`invalid` logs
    # express, and `min` of 1-2 is the same failure caught mid-collapse. Either
    # way the spread computed from it is quantisation noise. See
    # CANARY_MIN_RESOLVABLE for why the bound is derived rather than picked.
    low = canary.get("min")
    if low is not None and low < CANARY_MIN_RESOLVABLE:
        return CANARY_BROKEN
    # A whole-cycle record's `spread` may be two roundings wide.
    #
    # CANARY_MIN_RESOLVABLE bounds *one* cycle of quantisation at the tolerance,
    # but a spread is taken across two samples. So on a record that predates the
    # centicycle extremes, a per-access cost below twice that bound cannot
    # support either verdict: `min=5 max=7` is consistent with a true spread
    # anywhere from 17% to 60%, which straddles the 25% tolerance. Neither
    # "contaminated" nor "clean" is assertable, and "the instrument could not
    # measure" is exactly what CANARY_BROKEN means.
    #
    # This deliberately RECLASSIFIES two historical records -- 21:37 from
    # `contaminated` and 21:56 from `clean`, both to `broken`. That is a
    # correction, not a loss: those runs really were unable to resolve their own
    # quantity, and the later centicycle run showed the true figure (47%) sits
    # between what the two of them claimed. Records carrying `min_centi` are
    # exempt because their spread was computed at 0.01-cycle resolution.
    if "min_centi" not in canary and low is not None:
        if low < 2 * CANARY_MIN_RESOLVABLE and canary.get("samples"):
            return CANARY_BROKEN
    if "spread" in canary:
        over = canary["spread"] > CANARY_TOLERANCE_PCT
    else:
        over = abs(canary["pct"] - 100) > CANARY_TOLERANCE_PCT
    # Arm-separation failures are decisive only when the samples that *did*
    # measure came back quiet. `invalid` is authoritative when present (the
    # kernel counted its own failures); on older logs a zero start is the same
    # thing, one failed endpoint measurement.
    if not over and (canary.get("invalid", 0) > 0 or canary["start"] <= 0):
        return CANARY_BROKEN
    return CANARY_CONTAMINATED if over else CANARY_CLEAN


def canary_is_contaminated(canary):
    """True only if the canary *measured* host-load contamination.

    Deliberately narrow, and deliberately False for `broken`: this answers the
    question its name asks. Callers wanting "may I trust this run?" must test
    `canary_verdict(...) != CANARY_CLEAN`, which is a different and stricter
    question.
    """
    return canary_verdict(canary) == CANARY_CONTAMINATED


def print_canary_summary(canary):
    """Print the current run's canary verdict and what it does and does not mean.

    # Why this is a function

    It was 55 lines inline in `main()`, which made it unreachable from the test
    suite: the only way to exercise it was to run the whole tool against a real
    log. So the one paragraph in this repo whose entire job is to stop a reader
    misattributing a benchmark result had no test asserting what it says --
    and it spent this whole thread saying the wrong thing. The BROKEN branch
    named the optimiser as the sole cause of an arm-separation failure until
    the P20 load test produced the identical symptom from host load instead.

    That is the same shape as the bug this file exists to document: a check
    nobody could exercise is indistinguishable from a check that passes.
    Extracting it costs nothing and makes the wording assertable.
    """
    verdict = canary_verdict(canary)
    if verdict == CANARY_ABSENT:
        print("  Contamination canary: absent (log predates it) - unknown, "
              "not clean.")
        return

    if "spread" in canary:
        detail = (f"spread {canary['spread']}% over {canary['samples']} "
                  f"samples ({canary['min']}-{canary['max']} cycles)")
    else:
        detail = (f"endpoints {canary['start']} -> {canary['end']} cycles, "
                  f"{canary['pct']}% (no mid-suite sampling in this log)")

    if verdict == CANARY_BROKEN:
        failed = canary.get("invalid")
        how = (f"{failed} measurement(s) failed"
               if failed else "it measured zero cycles per access")
        print(f"  CANARY BROKEN: {how} - contamination is UNKNOWN for this "
              f"run, not clean ({detail}).")
        print("  A reference access cost of zero is not a fast machine, it is "
              "a failed measurement: the A/B arms did not separate.")
        # Two causes, not one. This used to name only the optimiser, which is
        # what happened the first time and was written from that single
        # instance -- and the P20 load test then produced the identical
        # symptom from the opposite cause, on a binary whose store the
        # scale-invariance check had proven intact in the same run. Sending
        # the reader to disassemble a correct function is how a diagnostic
        # becomes a wild-goose chase. The kernel's report_arm_failure_causes
        # carries the same pair; keep them in step.
        print("  Two causes need opposite responses: (1) the store was "
              "optimised away, so there is no signal - check the 'canary "
              "scale check' line in that run's log; or (2) host load exceeded "
              "the ~5-cycle A/B signal and inverted the arms, as demonstrated "
              "by scripts/canary-load-test.sh.")
        print("  Do not read this as host load *by default*. See "
              "known-issues.md "
              "B-BENCH-CANARY-MEASURES-ZERO-IN-RELEASE-AND-BLAMES-THE-HOST.")
    elif verdict == CANARY_CONTAMINATED:
        print(f"  CONTAMINATED: reference access cost {detail}, tolerance "
              f"{CANARY_TOLERANCE_PCT}%.")
        print("  Host load changed during the run. A single-benchmark outlier "
              "here is unproven - the drift correction removes a uniform "
              "factor, and this is not one.")
        # Failures alongside an over-tolerance spread corroborate it: noise big
        # enough to invert a 5-cycle A/B split is load. Say so, or a reader
        # seeing both facts reconciles them the wrong way round.
        failed = canary.get("invalid") or 0
        if failed:
            print(f"  {failed} measurement(s) also failed to separate their "
                  f"arms outright, which corroborates this verdict rather "
                  f"than weakening it.")
    else:
        # NOT "host load stable" -- that is a claim the canary cannot support
        # and which was measurably false every time it was made. All three runs
        # that carried dispersion data were certified clean here while each
        # contained 5-8 benchmarks with >=5x in-run dispersion. See
        # known-issues.md
        # B-BENCH-CANARY-CERTIFIES-CLEAN-RUNS-THAT-CONTAIN-MULTI-X-STALLS.
        print(f"  Canary OK: reference access cost steady between benchmarks, "
              f"{detail}.")
        print("  That is a *sampled* check, ~1 sample per 8 benchmarks. It "
              "does not mean individual benchmarks ran undisturbed - see the "
              "dispersion line below.")


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


#: Keys under which a table in `bench/baselines.toml` may state a nanosecond
#: target. Cycle- and access-denominated targets are deliberately absent: they
#: are not comparable with the kernel's `SCORE` line, which is always in ns.
_BASELINE_NS_KEYS = (("target_ns", 1), ("target_us", 1_000), ("target_ms", 1_000_000))


def load_baselines(path=None):
    """Parse `bench/baselines.toml` into {name: target_ns}, or None.

    None means the file could not be read or parsed *at all*, which callers
    must not confuse with "the file agrees" -- the distinction this whole
    module keeps rediscovering.
    """
    path = path or os.path.join(REPO_ROOT, "bench", "baselines.toml")
    try:
        import tomllib
    except ImportError:  # Python < 3.11: no stdlib TOML.
        return None
    try:
        with open(path, "rb") as handle:
            data = tomllib.load(handle)
    except (OSError, ValueError) as exc:
        print(f"bench-history: cannot parse {path}: {exc}", file=sys.stderr)
        return None
    targets = {}
    for name, table in data.items():
        if not isinstance(table, dict):
            continue
        # `tcg_target_ns` wins when present. The two are different quantities,
        # not competing estimates of one: `target_ns` is the hardware reference
        # (Linux/OpenSSL/Fuchsia), while `tcg_target_ns` is the budget the
        # suite is actually graded against under emulation -- and for some
        # benchmarks bench.rs says so outright ("OpenSSL SHA-256 1KiB: ~1500ns.
        # QEMU target: 50000ns"). It also covers scope differences, where the
        # benchmark measures a fixed multiple of the per-operation target
        # (alloc+free = 2x an alloc; the MIME benchmark does 4 lookups).
        # Conflating them produced spurious "disagreements" of up to 20x.
        if isinstance(table.get("tcg_target_ns"), (int, float)):
            targets[name] = int(table["tcg_target_ns"])
            continue
        for key, scale in _BASELINE_NS_KEYS:
            if key in table and isinstance(table[key], (int, float)):
                targets[name] = int(table[key] * scale)
                break
    return targets


def report_baselines(current_entries, baselines):
    """Cross-check the kernel's own targets against `bench/baselines.toml`.

    The kernel prints `SCORE <name> <measured> <target> ...`, where the target
    is a **literal in `kernel/src/bench.rs`** with a comment beside it saying
    "from baselines.toml". Nothing ever verified that claim: the file was not
    parsed anywhere in the tree, and had in fact been invalid TOML -- two
    `[compositor_frame_4k]` tables -- for months without anyone noticing. See
    `TD-BASELINES-TOML-IS-INVALID-TOML-AND-NOTHING-READS-IT`.

    This makes the claim checkable. It compares the two numbers and reports
    three distinct failures, which are genuinely different problems:

    * **disagree** -- both sides state a target and the values differ. One of
      them has been edited without the other; the file is lying.
    * **no baseline** -- the kernel grades a benchmark against a target that
      exists nowhere but the Rust literal, so it has no recorded provenance.
    * **unused baseline** -- the file states a target for something the suite
      does not measure, which reads as coverage and is not.

    Reporting only. Deciding which side is right needs a human or a citation,
    and silently trusting either one is how the two drifted apart to begin
    with.
    """
    if baselines is None:
        print("  Baselines: bench/baselines.toml could not be parsed - "
              "targets are UNVERIFIED (this is not the same as agreeing).")
        return
    disagree, missing = [], []
    for name, vals in sorted(current_entries.items()):
        kernel_target = vals[1]
        if name not in baselines:
            missing.append(name)
        elif baselines[name] != kernel_target:
            disagree.append((name, kernel_target, baselines[name]))
    unused = sorted(set(baselines) - set(current_entries))

    if not (disagree or missing or unused):
        print(f"  Baselines: all {len(current_entries)} targets agree with "
              "bench/baselines.toml.")
        return
    print(f"  Baselines: {len(disagree)} disagree, {len(missing)} unbaselined, "
          f"{len(unused)} unused (bench/baselines.toml vs the kernel's own "
          "SCORE targets):")
    for name, kernel_target, file_target in disagree:
        print(f"    {name}: kernel says {kernel_target}ns, file says "
              f"{file_target}ns")
    if missing:
        print(f"    no baseline for: {', '.join(missing)}")
    if unused:
        print(f"    baseline never measured: {', '.join(unused)}")


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


#: How many recent same-host/same-profile records form the median that a single
#: run is judged against.  Enough to outvote one or two odd boots; short enough
#: that a real, permanent speed-up stops being treated as an anomaly after a
#: few runs.
SPEED_WINDOW = 8

#: A run whose whole-suite factor sits this far from the historical median is an
#: outlier: not wrong, but every *absolute* number in it is scaled, so it must
#: not be used as a baseline without saying so.
OUTLIER_PCT = 15.0


def per_benchmark_median(records):
    """Median value of each benchmark across `records`.

    The point of a *per-benchmark* median (rather than one global factor per
    record) is that it survives benchmarks appearing and disappearing across
    the window, which happens whenever the suite grows.
    """
    acc = {}
    for record in records:
        for name, value in record.get("entries", {}).items():
            if value and value > 0:
                acc.setdefault(name, []).append(value)
    return {name: statistics.median(vals) for name, vals in acc.items()}


def speed_factor(entries, medians):
    """This run's whole-suite speed relative to `medians`, or None.

    `1.0` means typical for this host; `0.8` means the whole suite ran 20%
    faster than usual, which is a property of the *machine*, not the code.
    """
    ratios = [
        value / median
        for name, value in entries.items()
        if value and value > 0 and (median := medians.get(name)) and median > 0
    ]
    if len(ratios) < MIN_SAMPLES_FOR_DRIFT:
        return None
    return statistics.median(ratios)


def report_run_position(records, host, profile, current, previous):
    """Say where this run and its baseline sit against the recent history.

    Why this exists on top of `global_drift`
    ----------------------------------------
    `global_drift` compares this run to *the single previous run* and removes
    the uniform factor between them.  That is the right correction and it
    works -- but it is silent about which of the two runs was the odd one, and
    it leaves the reader looking at raw before/after numbers drawn from a
    baseline that may itself have been anomalous.

    This is not hypothetical.  Replaying the committed release history through
    this function gives x1.009, x1.010, x1.001, x0.975, **x0.759**, x1.000,
    x1.000: the 2026-08-14T19:05 boot ran ~24% faster across all 64
    benchmarks, for host-side reasons.  Two benchmarks were duly written up as
    regressions (`isr_latency` x2.34, `pick_next` x1.76) on the *next* run,
    when both had merely returned to normal from that boot, and a genuine 2.3x
    improvement in `syscall_dispatch` was reported in pieces that did not add
    up, because one piece was measured against the fast boot.  The drift
    correction had done its job in every individual comparison; what was
    missing was anybody saying "that baseline was 24% off".  (Both of those
    write-ups now carry a CORRECTION in known-issues.md.)

    So: label the outlier at the moment it is recorded, and label it again the
    next time it is used as a baseline.  On that history the second rule fires
    on exactly the run that produced the bogus write-ups.

    The window is *causal* -- only records preceding the run being judged --
    so a verdict never changes retroactively as later runs arrive, and the
    number printed at boot is the number still printed a week later.
    """
    window = [
        record for record in records
        if record.get("host") == host and record_profile(record) == profile
    ][-SPEED_WINDOW:]
    if len(window) < 2:
        return

    medians = per_benchmark_median(window)
    here = speed_factor(current, medians)
    if here is None:
        return

    print(
        f"  This run vs the median of the last {len(window)} run(s) on this "
        # ASCII 'x', not the multiplication sign: this script's output is read
        # on a cp1252 Windows console, where U+00D7 arrives as a replacement
        # character and turns the one number that matters into "?0.041".
        f"host: x{here:.3f} whole-suite."
    )
    if abs(here - 1.0) * 100.0 >= OUTLIER_PCT:
        faster = "faster" if here < 1.0 else "slower"
        print(
            f"  !! OUTLIER RUN: everything measured {faster} than usual by "
            f"{abs(here - 1.0) * 100.0:.0f}%."
        )
        print(
            "     Treat every absolute number below as scaled by that factor. "
            "Do not quote them"
        )
        print("     as the cost of anything, and do not use this run as a baseline.")

    if previous is not None:
        there = speed_factor(previous.get("entries", {}), medians)
        if there is not None and abs(there - 1.0) * 100.0 >= OUTLIER_PCT:
            print(
                f"  !! The baseline this run is diffed against was itself an "
                f"outlier (x{there:.3f})."
            )
            print(
                "     Drift correction still cancels the uniform part, so the "
                "percentages are"
            )
            print(
                "     usable -- but the raw before/after values are not a fair "
                "picture of either run."
            )


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


def report_baseline_canary(previous):
    """State whether the run being diffed *against* was a trustworthy one.

    Every record carries a `canary` block -- the kernel's own direct
    measurement of whether the host stayed quiet for that run -- and until now
    nothing on the comparison path read it. That is this file's recurring
    defect in its purest form: the datum existed, was correct, and had no
    consumer, so a baseline measured on a loaded machine was indistinguishable
    from one measured on an idle machine.

    It matters because the diff is a *ratio*. A baseline inflated by host load
    makes the current run look uniformly faster, and the drift correction then
    subtracts that whole-suite factor -- which is right for the benchmarks that
    moved uniformly and wrong for any that did not, promoting them to
    "REGRESSED". Two benchmarks were written up that way (`isr_latency` 2.34x,
    `pick_next` 1.76x) against a baseline that is now known to be 24% fast.

    Note the deliberate asymmetry with `report_run_position`: that one *infers*
    an outlier statistically, from the run's position in the recent
    distribution, and needs several records before it can say anything. This
    one reads a measurement the kernel already took, and works on the second
    record. When they agree the verdict is strong; when they disagree that is
    itself worth seeing, so neither is folded into the other.

    Nothing is skipped or auto-corrected here. Silently choosing an older,
    cleaner baseline would make the printed diff answer a question the reader
    did not ask -- so the baseline stays the most recent run, and its quality
    is stated instead.
    """
    verdict = canary_verdict(previous.get("canary"))
    if verdict == CANARY_CLEAN:
        return
    if verdict == CANARY_ABSENT:
        print(
            "  NOTE: the baseline run predates the host-load canary, so "
            "whether that machine was quiet is unknown and unknowable."
        )
    elif verdict == CANARY_BROKEN:
        print(
            "  WARNING: the baseline run's canary could not measure "
            "(instrument failure, not a busy host), so contamination is "
            "UNKNOWN for it. Treat every movement below as unproven."
        )
    else:
        canary = previous.get("canary") or {}
        spread = canary.get("spread")
        detail = f" (reference access cost spread {spread}%)" if spread else ""
        print(
            f"  WARNING: the baseline run's canary measured host-load "
            f"contamination{detail}. It is a ratio's denominator, so the "
            f"percentages below carry its error, and the drift correction "
            f"removes only the part that moved uniformly."
        )


def report(previous, current_entries, threshold_pct,
           records=None, host=None, profile=LEGACY_PROFILE):
    """Print the run-over-run comparison. Returns True if anything regressed.

    `records`/`host`/`profile` are optional only so that callers interested
    purely in the run-over-run diff (the tests, chiefly) need not construct a
    history.  When they are supplied, the diff is additionally placed against
    the recent history for this host -- see `report_run_position`, and note
    that the run-over-run diff alone is what produced two written-up
    regressions that never existed.
    """
    current = {name: vals[0] for name, vals in current_entries.items()}

    # Run before the early return: the target cross-check is independent of
    # whether there is a previous record to diff against, and the first record
    # on a host is exactly when a wrong target is most likely to go unnoticed.
    report_baselines(current_entries, load_baselines())

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
    report_baseline_canary(previous)
    print(
        "  Comparison is run-over-run on this host, which cancels the TCG "
        "emulation constant."
    )
    print(
        "  (The 'target' column in the scorecard above is a *mix*: mostly a "
        "hardware reference that cannot be met under TCG, but for some "
        "benchmarks an explicit TCG budget. bench/baselines.toml records "
        "which is which as target_ns vs tcg_target_ns.)"
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

    # After the drift line, because it answers the question the drift line
    # raises ("drifted relative to what?") and before the regressed/improved
    # lists, because it says whether those lists can be trusted at all.
    if records is not None and host is not None:
        report_run_position(records, host, profile, current, previous)

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
    """Print a one-line summary of every stored record.

    The canary column is *recomputed* from each record's stored `canary` dict
    rather than read from its stored `contaminated` boolean. Those two
    disagree for every release record written before 2026-08-14T20:30: they
    hold `contaminated: true` when the truth is that the canary measured
    nothing at all. The records are append-only and are left exactly as
    written; this view just declines to repeat their conclusion.
    """
    records = load_history(history_path)
    if not records:
        print(f"bench-history: no records in {history_path}")
        return 0
    broken = 0
    for record in records:
        entries = record.get("entries", {})
        over = record.get("over_target", "?")
        verdict = canary_verdict(record.get("canary"))
        if verdict == CANARY_BROKEN:
            broken += 1
        print(
            f"{record.get('timestamp', '?')}  {record.get('host', '?'):<20} "
            f"{record_profile(record):<8} {record.get('commit', '?'):<12} "
            f"{len(entries):>3} benchmarks, {over} over hardware target, "
            f"canary {verdict}"
        )
    if broken:
        print(
            f"\n  {broken} of {len(records)} record(s) have a canary that could "
            f"not measure: contamination is UNKNOWN for those runs, and any "
            f"single-benchmark movement in them is unproven."
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
    regressed = report(previous, current_entries, args.threshold,
                       records=records, host=host, profile=args.profile)

    # Reported *after* the comparison, so it qualifies the verdict the reader
    # has just seen rather than being buried above it.
    print_canary_summary(canary)

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
            # Both, and they are not redundant. `contaminated` answers only the
            # question its name asks and is False for a broken canary;
            # `canary_verdict` is the one to test when what you mean is "may I
            # trust this run?". Storing only the boolean is how nine release
            # records ended up flagged as contaminated when the truth was that
            # the instrument had died.
            record["contaminated"] = canary_is_contaminated(canary)
            record["canary_verdict"] = verdict
        if append_record(args.history, record):
            print(f"  Recorded {len(current_entries)} benchmarks to "
                  f"{os.path.relpath(args.history, REPO_ROOT)}")

    if regressed and args.fail_on_regression:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
