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

Exit codes: 0 normally.  With --fail-on-regression, 1 if any benchmark is still
being *claimed* as a regression once the gates below have had their say.  A
movement is not a claim merely because it crossed the threshold: it must also
leave the benchmark's own recent range (`per_benchmark_bands`), survive the
split-sample check on its own measurement window, and not have been
contradicted by another run of the same binary (`replication_verdict`).  The
word REGRESSED itself is reserved for movements every recorded run of the
commit agrees on; a commit measured only once reports UNREPLICATED, which is
weaker evidence but still fails the build, because "nobody looked twice" is not
a finding of innocence.
"""

from __future__ import annotations

import argparse
import collections
import datetime
import json
import math
import os
import platform
import re
import statistics
import struct
import subprocess
import sys

# `[bench] SCORE <name> <measured_ns> <target_ns|-> <PASS|OVER|TRACK> [<mean_ns> <iters>]`
#
# The trailing pair is optional because it was added after the history file
# already had records in it: logs written before the kernel emitted it must
# still parse, or the one longitudinal record we have gets truncated at the
# point of the change. Absent, dispersion is simply unknown for that run.
#
# `- TRACK` is a benchmark the kernel records without grading, because it has no
# published hardware target (the `vfs_stat_breakdown_*` phases,
# `ipc_channel_roundtrip_64k`). It must be accepted here for the same reason it
# had to be emitted there: a line this regex rejects is silently dropped, and a
# benchmark that is dropped by the parser is indistinguishable from one the
# kernel never measured. The target column is `-` and not `0` so that "has no
# target" cannot be confused with "has a target of zero and failed it".
# The trailing `<split>` is a third, separately-optional extension, added the
# same way and for the same reason. It carries the kernel's split-sample
# cross-check of that benchmark's own measurement window as one token:
#
#   `-`    no cross-check was performed. NOT "stable" -- see SPLIT_ABSENT.
#   `12`   checked; the two half-window sample sets' minima differ by 12%.
#   `31!`  checked and flagged past the kernel's gate.
#   `?`    checked, but a set's minimum was zero, so there is no ratio.
#
# The `!` is matched as part of the token rather than as its own column so the
# kernel stays the single owner of the threshold. Duplicating the constant here
# would let the two drift, and a gate that two programs disagree about is worse
# than no gate: it produces a verdict whose meaning depends on which half of the
# pipeline you read.
SCORE_RE = re.compile(
    r"^\[bench\]\s+SCORE\s+(\S+)\s+(\d+)\s+(\d+|-)\s+(PASS|OVER|TRACK)"
    r"(?:\s+(\d+)\s+(\d+)(?:\s+(-|\?|\d+!?))?)?\s*$"
)

#: `split` token values with no percentage attached.
SPLIT_ABSENT = None      # the log predates the column entirely
SPLIT_UNCHECKED = "-"    # the kernel ran no cross-check for this entry
SPLIT_UNRESOLVED = "?"   # cross-checked, but the timer could not resolve it

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

#: Functions whose *address* has been shown to change their measured cost by
#: several-fold under QEMU's TCG, with their machine code byte-identical.
#:
#: On 2026-08-18 `crypto_sha256_64B` went 7426 -> 30048 cycles across a commit
#: that edits only `audio_mixer.rs`.  The SHA-256 code did not change (same
#: symbol size, same mangled hash); `crypto::compress` merely moved to
#: `…80afce00`, and moving it anywhere else -- two unrelated addresses were
#: tried -- restored the original number exactly.  `crypto_sha512_64B`, a
#: near-identical routine at a different address, was unaffected.  See
#: A-A-4x-CRYPTO-"REGRESSION" in known-issues.md.
#:
#: Recording the addresses turns a repeat of that into a one-line observation
#: rather than the multi-hour bisect it cost the first time: if a crypto
#: benchmark jumps and `compress` moved in the same run, that is the answer.
#:
#: Matched as substrings so that one pattern covers both of Rust's mangling
#: schemes -- legacy `_ZN6kernel6crypto8compress17h…E` and v0
#: `_RNvNtCs…_6kernel6crypto8compress` share the length-prefixed `6crypto8compress`.
#: The `6crypto` prefix is what keeps `fs::compress` and `mm::compress` out.
HOT_SYMBOLS = {
    "crypto::compress": "6crypto8compress",
    "crypto::sha512_compress": "6crypto15sha512_compress",
    "net::tcp::tcp_checksum_ip": "3tcp15tcp_checksum_ip",
}


def elf_symbol_addresses(path, wanted=HOT_SYMBOLS):
    """Map friendly name -> load address for `wanted`, read from an ELF64 file.

    Parsed by hand rather than shelled out to `nm`/`objdump` on purpose: neither
    is on PATH in this environment by default (llvm-tools had to be installed
    mid-investigation to get one), and a diagnostic that silently records
    nothing on a machine missing an optional tool is worse than no diagnostic,
    because its absence looks like "the addresses did not move".

    Returns `{}` -- never raises -- if the file is missing, is not ELF64, or has
    been stripped.  This is bookkeeping attached to a benchmark record; it must
    never be the reason a completed measurement fails to be written.

    On a file that *did* parse, every key of `wanted` is present, mapped to
    `None` where the pattern matched no symbol.  So `{}` means "this binary told
    us nothing" while a `null` value means "we looked for this one and it is not
    there any more" -- most likely because it moved module and the pattern needs
    updating.  The two must stay distinguishable; see the comment at the return.
    """
    try:
        with open(path, "rb") as fh:
            data = fh.read()
    except OSError:
        return {}
    # 0x7f E L F, class 2 (64-bit), little-endian.
    if len(data) < 64 or data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        return {}

    def u(fmt, off):
        return struct.unpack_from(fmt, data, off)[0]

    try:
        e_shoff = u("<Q", 0x28)
        e_shentsize = u("<H", 0x3A)
        e_shnum = u("<H", 0x3C)
        if not e_shoff or not e_shnum:
            return {}
        # Find .symtab (sh_type 2); its sh_link names the matching string table,
        # which is why the string table is not searched for by name.
        symtab = None
        for i in range(e_shnum):
            sh = e_shoff + i * e_shentsize
            if u("<I", sh + 4) == 2:  # SHT_SYMTAB
                symtab = sh
                break
        if symtab is None:
            return {}
        sym_off, sym_size = u("<Q", symtab + 0x18), u("<Q", symtab + 0x20)
        sym_entsize = u("<Q", symtab + 0x38) or 24
        strtab_idx = u("<I", symtab + 0x28)
        st = e_shoff + strtab_idx * e_shentsize
        str_off, str_size = u("<Q", st + 0x18), u("<Q", st + 0x20)
        strs = data[str_off:str_off + str_size]

        # Keep the shortest match per pattern: a monomorphised wrapper that
        # merely mentions the function has a longer name than the function.
        best = {}
        for off in range(sym_off, sym_off + sym_size, sym_entsize):
            name_off = u("<I", off)
            value = u("<Q", off + 8)
            if not value or name_off >= len(strs):
                continue
            end = strs.find(b"\0", name_off)
            name = strs[name_off:end if end >= 0 else None].decode(
                "utf-8", "replace")
            for friendly, pattern in wanted.items():
                if pattern in name:
                    prev = best.get(friendly)
                    if prev is None or len(name) < prev[0]:
                        best[friendly] = (len(name), value)
        # Every wanted name is present, `null` where the pattern matched
        # nothing. A pattern goes stale when its function moves module -- and
        # the obvious next move, `crypto::compress` into the shared `sha2`
        # crate, would do exactly that, turning `6crypto8compress` into
        # `4sha28compress`. Dropping the key on a miss would make that failure
        # silent, and silent in the worst possible way: the diagnostic would
        # disappear at the moment it is most needed, because the very change
        # that broke the pattern is the one that relocates the function.
        return {
            friendly: (f"{best[friendly][1]:#018x}" if friendly in best
                       else None)
            for friendly in sorted(wanted)
        }
    except (struct.error, IndexError, ValueError):
        return {}


def split_is_unstable(token):
    """True only if `token` is a cross-check the kernel actually flagged.

    Everything else is False, and deliberately so: an absent column, an
    unchecked entry and an unresolved one have each found *nothing*, which is
    not the same as having found stability. Callers that need to distinguish
    "checked and clean" from "never checked" must compare against
    `SPLIT_ABSENT`/`SPLIT_UNCHECKED` themselves rather than read this bool --
    the same rule `SplitCheck::is_unstable` states on the kernel side.
    """
    return isinstance(token, str) and token.endswith("!")


def split_pct(token):
    """The spread percentage in `token`, or None if it carries no number."""
    if not isinstance(token, str):
        return None
    body = token[:-1] if token.endswith("!") else token
    return int(body) if body.isdigit() else None


def parse_serial(path):
    """Extract {name: (measured_ns, target_ns, verdict, mean_ns, iters, split)}.

    `mean_ns` and `iters` are `None` for a log predating their emission, and
    `split` is `SPLIT_ABSENT` for a log predating *its* emission. The tuple is
    extended only at the end so existing positional readers (`value[0]`,
    `value[3]`) keep meaning what they meant.

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
                    name, measured, target, verdict, mean, iters, split = (
                        match.groups()
                    )
                    entries[name] = (
                        int(measured),
                        # None for a tracked benchmark. Callers that grade
                        # against the target must skip these rather than treat
                        # the absence as a disagreement -- see
                        # check_baseline_agreement.
                        None if target == "-" else int(target),
                        verdict,
                        int(mean) if mean is not None else None,
                        int(iters) if iters is not None else None,
                        split,
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
    """Print the current run's canary verdict, and return it.

    Returns the `CANARY_*` verdict so the caller can store it on the history
    record without recomputing it.

    **The return value is why this docstring has a second half.** When this
    function was extracted from `main()`, the `verdict = canary_verdict(canary)`
    binding came with it -- and `main()` still referenced `verdict` 250 lines
    further down, to write `record["canary_verdict"]`. That is a `NameError` on
    the recording path, so from the extraction commit onward *every* `--bench`
    boot crashed the recorder after printing its summary and wrote no history
    record at all. It went unnoticed for four commits because `boot-test.sh`
    ignored this tool's exit status and printed "Boot test PASSED" over the
    traceback.

    Returning the verdict, rather than leaving the caller to call
    `canary_verdict` a second time, is what makes the coupling explicit: there
    is now no way to use this function's output without receiving the value
    `main()` needs.

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
        return verdict

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
        # Two independent limits, and the second one is far worse than the
        # first. The sampling limit used to be stated alone, which implies the
        # canary would catch host load if only it sampled more often. It would
        # not: the quantity it measures does not respond to host load at all.
        print("  That is a *sampled* check, ~1 sample per 8 benchmarks. It "
              "does not mean individual benchmarks ran undisturbed - see the "
              "dispersion line below.")
        print("  It also cannot see host descheduling AT ALL, at any sampling "
              "rate: it counts *guest* cycles, and the guest's counter does "
              "not advance while the host is running something else. On the "
              "one pair of boots where the host demonstrably stole the CPU "
              "(160s vs 365s for the same binary) this line read 0% spread - "
              "its cleanest possible verdict - on the contaminated run. See "
              "known-issues.md B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING, and "
              "read the run-level verdict below rather than this line.")

    return verdict


def display_path(path):
    """`path` relative to the repo when that is possible, else as given.

    Cosmetic, and that is the entire point: `os.path.relpath` raises
    `ValueError` on Windows when the two paths sit on different drives, so the
    bare call this replaces could abort the tool *after* the record had already
    been appended -- a non-zero exit and a traceback on a run that had in fact
    succeeded. Prettifying a path in a success message must not be able to fail
    the run it is reporting on.

    Not hypothetical: it fired the first time anything drove `main()` with a
    `--history` outside the repo tree (the end-to-end test's temp directory,
    which lands on C: while the checkout is on D:). Any operator passing an
    explicit `--history` on another volume would have hit the same thing.
    """
    try:
        return os.path.relpath(path, REPO_ROOT)
    except ValueError:
        # Different drive on Windows; there is no relative form to give.
        return path


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


def dispersion_count(record, ratio=DISPERSION_SUSPECT_RATIO):
    """Stalled-benchmark count for a *stored* record, or None if unknowable.

    Recomputed from the record's own `entries` and `mean_ns` rather than read
    from the `dispersion` key that current records also carry. That is not
    redundancy for its own sake: it is the same choice `cmd_list` makes about
    the canary, for the same reason. A stored count freezes whatever
    `DISPERSION_SUSPECT_RATIO` happened to be that day, and the ratio is
    explicitly a placeholder awaiting release-profile data -- so re-judging the
    history after a retune has to be possible, and a stored scalar cannot be
    re-judged. The stored key exists only so `--list` need not do this work.

    `None` (records written before the mean_ns extension) means *unknown*, and
    callers must not average it in as a zero. Every part of this file keeps
    rediscovering that absent is not clean.
    """
    entries = record.get("entries") or {}
    means = record.get("mean_ns")
    if not means:
        return None
    count = 0
    for name, measured in entries.items():
        mean = means.get(name)
        if mean is None or not measured:
            continue
        if mean / measured >= ratio:
            count += 1
    return count


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

    A *tracked* benchmark (target `-` on the wire, `None` here) is none of the
    three: it declares no target, so there is nothing to cross-check. It is
    counted and named in the summary but never listed as unbaselined, which it
    would otherwise be on every single run.

    Reporting only. Deciding which side is right needs a human or a citation,
    and silently trusting either one is how the two drifted apart to begin
    with.
    """
    if baselines is None:
        print("  Baselines: bench/baselines.toml could not be parsed - "
              "targets are UNVERIFIED (this is not the same as agreeing).")
        return
    disagree, missing, tracked = [], [], []
    for name, vals in sorted(current_entries.items()):
        kernel_target = vals[1]
        if kernel_target is None:
            # A tracked benchmark states no target, so there is nothing here to
            # agree or disagree with. Listing it as "unbaselined" would be a
            # false report -- it is not missing a baseline, it is a benchmark
            # that deliberately has none -- and would grow the missing list on
            # every run until the real unbaselined entries were lost in it.
            tracked.append(name)
        elif name not in baselines:
            missing.append(name)
        elif baselines[name] != kernel_target:
            disagree.append((name, kernel_target, baselines[name]))
    # Tracked names are excluded from `unused` too: a baseline that names one
    # is a real inconsistency (the file grades something the kernel does not),
    # so it must stay reportable, and set-differencing against every parsed
    # name would hide it. Hence the difference is taken against the *graded*
    # names only.
    graded = {n for n, v in current_entries.items() if v[1] is not None}
    unused = sorted(set(baselines) - graded)

    # Stated whenever there are any, including on the all-agree path: the count
    # is how a reader tells "N benchmarks agree" from "N benchmarks were graded
    # and M more were not graded at all", which are different facts about the
    # run and would otherwise both print the same line.
    suffix = f" ({len(tracked)} tracked without a target)" if tracked else ""
    if not (disagree or missing or unused):
        print(f"  Baselines: all {len(graded)} targets agree with "
              f"bench/baselines.toml{suffix}.")
        return
    print(f"  Baselines: {len(disagree)} disagree, {len(missing)} unbaselined, "
          f"{len(unused)} unused{suffix} (bench/baselines.toml vs the kernel's "
          "own SCORE targets):")
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


#: What the operator asserted about the host while a run was measured.
#:
#: Three values, and the default is deliberately the useless-sounding one.
#: `unknown` is what every record written before 2026-08-15 carries, because
#: nothing recorded it -- and "nobody said" must never be silently upgraded to
#: "the host was quiet". That upgrade is the exact error this whole axis exists
#: to stop: see known-issues.md B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING, where a
#: run that took 2.3x as long as its own twin was reported as the cleanest run
#: the instrument could describe.
#:
#: `loaded` is not a warning label, it is the *positive control*: a run
#: deliberately poisoned (scripts/canary-load-test.sh) so that a threshold can
#: one day be fitted against something known-contaminated. Such runs are
#: excluded from every baseline and median window below -- a control that
#: silently becomes a baseline is worse than no control.
HOST_LOAD_UNKNOWN = "unknown"
HOST_LOAD_IDLE = "idle"
HOST_LOAD_LOADED = "loaded"
HOST_LOAD_CHOICES = (HOST_LOAD_UNKNOWN, HOST_LOAD_IDLE, HOST_LOAD_LOADED)


def record_host_load(record):
    """The host-load label on a record, defaulting old records to unknown."""
    load = record.get("host_load", HOST_LOAD_UNKNOWN)
    return load if load in HOST_LOAD_CHOICES else HOST_LOAD_UNKNOWN


def record_experiment(record):
    """Why this run was a deliberate probe, or `""` for an ordinary run.

    A run whose *binary* was built to answer a question -- a QEMU flag under
    test, a compiler feature toggled by hand, a bisect step -- is a real
    measurement of a kernel that no checkout reproduces. It belongs in the
    history (throwing away measurements is how findings get re-discovered the
    expensive way) but it must never become the yardstick a later honest run is
    judged against.

    This is a different assertion from the two labels that already exist, which
    is why it is a third field rather than a reuse of either. `dirty` says the
    source moved a little from `commit`, and is true of most runs during
    development, so excluding on it would empty the history. `host_load:
    loaded` says the *host* was poisoned while the guest was fine. Neither
    covers "the guest itself was not the guest we ship".

    The cost of not having had this: five probe runs of the placement
    investigation (three at ~8085 ns for `crypto_sha256_64B`, two at ~1936 for
    the identical source built with a different symbol-mangling scheme) would
    have entered one 8-run window, widening that benchmark's outlier fence past
    4x and silently blinding the detector for it for the next eight runs.
    """
    why = record.get("experiment", "")
    return why if isinstance(why, str) else ""


def comparable_records(records, host, profile=LEGACY_PROFILE):
    """Records that may legitimately serve as history for a run here.

    Same host, same build profile, **not** a deliberately-loaded control, and
    **not** a deliberate experiment (`record_experiment`).

    Extracted because `previous_for_host` and `report_run_position` had each
    open-coded the host/profile filter, so a third rule (excluding controls)
    would otherwise have had to be added twice and could then be added to only
    one of them. One filter, two callers.
    """
    return [
        record for record in records
        if record.get("host") == host
        and record_profile(record) == profile
        and record_host_load(record) != HOST_LOAD_LOADED
        and not record_experiment(record)
    ]


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

    Deliberately-loaded control runs are skipped too (`comparable_records`):
    they are contaminated on purpose, and diffing the next honest run against
    one would report the *recovery* as a suite-wide improvement.
    """
    window = comparable_records(records, host, profile)
    return window[-1] if window else None


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
    window = comparable_records(records, host, profile)[-SPEED_WINDOW:]
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


# ---------------------------------------------------------------------------
# The run-level verdict: several axes, worst wins, and "clean" must be earned.
#
# Until 2026-08-15 the run-level verdict *was* the canary verdict. That was
# shown to be structurally wrong, not merely imprecise: the canary measures a
# reference access in **guest cycles**, and host descheduling of the QEMU
# process lands *between* guest instructions, where an emulated cycle counter
# cannot see it. On the pair of boots that exposed this (identical binary,
# minutes apart) the slower run took 2.3x as long, doubled two unrelated kernel
# benchmarks and scattered nine regressions -- and the canary reported 0%
# spread, the cleanest reading it is capable of emitting. See known-issues.md
# B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING.
#
# The repair is not a better canary threshold; it is to stop letting one
# instrument certify a run. Three consequences are built in below:
#
#  1. The verdict is the **worst** of several axes, so any one of them can
#     condemn a run and none of them can absolve it alone.
#  2. An axis with no measurement, or no fitted threshold, returns `unknown` --
#     never `clean`. A run is CLEAN only when every axis actively says so.
#  3. The canary keeps its job as a *positive* detector (it fires -> the run is
#     contaminated) and loses its implied role as a negative certificate.
#
# Consequence (2) means every record in the history today grades `unknown`,
# because the wall-clock axis has no stored data to compare against yet. That
# is the correct answer, not a defect: those runs were never shown to be quiet,
# and one of them demonstrably was not.
# ---------------------------------------------------------------------------

RUN_CLEAN = "clean"
RUN_UNKNOWN = "unknown"
RUN_CONTAMINATED = "contaminated"

#: Worst-wins ordering. Higher is worse; the run verdict is the max.
_RUN_SEVERITY = {RUN_CLEAN: 0, RUN_UNKNOWN: 1, RUN_CONTAMINATED: 2}

#: How many multiples of the median absolute deviation past the median counts
#: as an outlier on the count/duration axes.
#:
#: 3 is the conventional robust analogue of "3 sigma" (for a normal
#: distribution 1 MAD ~ 0.674 sigma, so 3 MAD ~ 2 sigma -- deliberately the
#: *loose* reading of the convention). It is not fitted to this host's data,
#: which is the point: every threshold in this file that was fitted to the
#: handful of runs available at the time later had to be un-fitted again.
ROBUST_OUTLIER_K = 3.0

#: Fewest comparable prior runs before a median/MAD band means anything.
#: Below this the "distribution" is two or three numbers and its MAD is an
#: artefact of which of them happened to be measured first.
MIN_WINDOW_FOR_BAND = 6


def robust_band(values, k=ROBUST_OUTLIER_K, mad_floor=0.0):
    """Upper edge of `median + k * MAD` over `values`, or None if too few.

    One-sided on purpose. Both quantities this is applied to -- stalled-
    benchmark counts and wall-clock duration -- are contaminated in one
    direction only: host interference can add stalls and add seconds, and can
    never subtract them. A two-sided band would spend half its power watching
    for an impossibility.

    `mad_floor` guards the degenerate case where the sample's MAD is zero or
    near it, which happens easily on small integer counts: without a floor the
    band collapses onto the median and the check fires on any run one unit
    above typical. A floor can only ever make the check *less* likely to fire.
    """
    if len(values) < MIN_WINDOW_FOR_BAND:
        return None
    median = statistics.median(values)
    mad = statistics.median([abs(v - median) for v in values])
    return median + k * max(mad, mad_floor)


def dispersion_axis(count, history):
    """Grade this run's stalled-benchmark count against its own host's history.

    Returns `(verdict, note)`.

    What this axis can and cannot do, stated plainly because implementing it
    contradicted the prescription in the known-issues entry that asked for it.
    That entry said "a run with a materially elevated stall count is
    CONTAMINATED regardless of the canary", on the strength of a 3 -> 8 move
    across the two boots that exposed the canary bug. Against the *full* 18
    release records on this host the counts run 3,3,4,4,4,4,5,5,5,5,5,7,7,8,
    9,9,13,13,15 -- median 5, MAD 2 -- so 8 sits at roughly the 75th percentile
    and is not distinguishable from this host's ordinary behaviour. The band
    below fires on 13 and 15; it would NOT have fired on the run that motivated
    it.

    That is reported rather than fixed by lowering the band, because lowering
    it until the motivating run fires is precisely the fitting-to-one-
    observation that this file has had to undo three times. The axis that
    *would* have separated those two boots is wall-clock time (160s vs 365s),
    which is why recording it matters more than tightening this.
    """
    if count is None:
        return RUN_UNKNOWN, "dispersion: not measured in this run (no mean_ns)"
    # Counts are integers, so a band narrower than one benchmark is reporting
    # quantisation. Same derivation as CANARY_MIN_RESOLVABLE, one axis over.
    band = robust_band(history, mad_floor=1.0)
    if band is None:
        return (RUN_UNKNOWN,
                f"dispersion: {count} benchmark(s) stalled; too few comparable "
                f"runs ({len(history)} < {MIN_WINDOW_FOR_BAND}) to say whether "
                f"that is unusual here")
    if count > band:
        return (RUN_CONTAMINATED,
                f"dispersion: {count} benchmark(s) stalled, above this host's "
                f"band of {band:.1f} (median {statistics.median(history):g} "
                f"over {len(history)} runs)")
    return (RUN_CLEAN,
            f"dispersion: {count} benchmark(s) stalled, within this host's "
            f"band of {band:.1f} (median {statistics.median(history):g} over "
            f"{len(history)} runs)")


#: Floor on the wall-clock MAD, as a fraction of the median.
#:
#: Chosen, not derived, and it only ever suppresses fires. Wall time is not a
#: pure measure of host interference: the suite has grown from 63 to 70
#: benchmarks and the kernel itself changes between runs, so a few percent of
#: run-to-run movement is guest-side and must not be read as host load. Revisit
#: once enough runs carry `wall_seconds` to measure the real quiet-host spread.
WALL_MAD_FLOOR_FRACTION = 0.05


def wall_axis(wall_seconds, history):
    """Grade this run's wall-clock duration against its own host's history.

    Returns `(verdict, note)`.

    Why this is the most sensitive of the three axes: TCG is pure emulation and
    therefore CPU-bound and single-threaded, so for a fixed amount of guest
    work the wall time is guest-work divided by the share of a core the
    emulator actually got. A run descheduled half the time takes twice as long,
    and unlike the canary there is nowhere for that time to hide -- it is
    measured on the *host's* clock, outside the guest, which is exactly the
    frame the canary cannot reach.

    The recorded benchmark numbers are minima over many iterations and so
    largely survive stalls; the wall clock does not, which is why it moves
    first and by the largest factor. On the pair that exposed the canary bug it
    read 160s against 365s -- unmissable, and unrecorded.
    """
    if wall_seconds is None:
        return RUN_UNKNOWN, "wall time: not recorded for this run"
    if not history:
        return (RUN_UNKNOWN,
                f"wall time: {wall_seconds:g}s; no prior run on this host "
                f"records one, so there is nothing to compare against")
    median = statistics.median(history)
    band = robust_band(history, mad_floor=median * WALL_MAD_FLOOR_FRACTION)
    ratio = wall_seconds / median if median else None
    where = (f"wall time: {wall_seconds:g}s vs a median of {median:g}s over "
             f"{len(history)} run(s)")
    if ratio:
        where += f" (x{ratio:.2f})"
    if band is None:
        return (RUN_UNKNOWN,
                f"{where}; too few comparable runs "
                f"({len(history)} < {MIN_WINDOW_FOR_BAND}) to say whether that "
                f"is unusual here")
    if wall_seconds > band:
        return (RUN_CONTAMINATED,
                f"{where}, above this host's band of {band:.0f}s -- the "
                f"emulator did not get the CPU it usually gets")
    return RUN_CLEAN, f"{where}, within this host's band of {band:.0f}s"


def canary_axis(verdict):
    """Map the canary's four outcomes onto the three run-level ones.

    Note the asymmetry, which is the whole point: CONTAMINATED condemns, and
    CLEAN is allowed to say `clean` for *this axis only*. It cannot absolve the
    run, because the run verdict is the worst of all axes and the canary is
    structurally blind to host descheduling.
    """
    if verdict == CANARY_CONTAMINATED:
        return RUN_CONTAMINATED, "canary: measured host-load contamination"
    if verdict == CANARY_CLEAN:
        return (RUN_CLEAN,
                "canary: steady between benchmarks (cannot see host "
                "descheduling -- guest cycles do not advance while the host "
                "runs something else)")
    if verdict == CANARY_BROKEN:
        return RUN_UNKNOWN, "canary: measurement failed, contamination unknown"
    return RUN_UNKNOWN, "canary: absent from this log, contamination unknown"


def run_verdict(canary_v, dispersion, dispersion_history,
                wall_seconds, wall_history):
    """Combine the axes into one run-level verdict. Returns `(verdict, notes)`.

    `notes` is the per-axis reasoning in a fixed order, so the printed block and
    any later re-judgement of a stored record say the same things in the same
    sequence.
    """
    axes = [
        canary_axis(canary_v),
        dispersion_axis(dispersion, dispersion_history),
        wall_axis(wall_seconds, wall_history),
    ]
    worst = max((v for v, _ in axes), key=lambda v: _RUN_SEVERITY[v])
    return worst, [note for _, note in axes]


def report_run_verdict(verdict, notes, extra_notes=()):
    """Print the combined run-level verdict and the axis that produced it.

    `extra_notes` are printed with the axes but do not vote -- the host-load
    label goes here because it is an assertion by whoever ran the test rather
    than a measurement, and nothing that cannot be checked is allowed to move
    the verdict.
    """
    if verdict == RUN_CONTAMINATED:
        headline = ("RUN CONTAMINATED: at least one instrument measured host "
                    "interference.")
    elif verdict == RUN_CLEAN:
        headline = ("RUN CLEAN: every instrument that could measure did, and "
                    "none of them fired.")
    else:
        headline = ("RUN UNPROVEN: nothing fired, but not every instrument "
                    "could measure -- this is NOT a clean run.")
    print(f"  {headline}")
    for note in list(notes) + list(extra_notes):
        print(f"    - {note}")
    if verdict == RUN_UNKNOWN:
        print("    Treat small effects from this run as ungraded. A large "
              "effect can still")
        print("    survive an unproven run (a min-of-N over a short operation "
              "is robust to")
        print("    stalls), but say so explicitly rather than citing the run "
              "as clean.")


#: Where a run-over-run movement sits against the benchmark's own history.
BAND_OUTSIDE = "outside"
BAND_WITHIN = "within"
BAND_UNJUDGED = "unjudged"


#: Tukey's fence multiplier: a sample is an outlier of its own distribution
#: past `Q1 - k*IQR` / `Q3 + k*IQR`. 1.5 is the textbook boxplot constant and
#: is deliberately not fitted to this project's data.
TUKEY_K = 1.5


def per_benchmark_bands(records, k=TUKEY_K):
    """Robust spread of every benchmark across `records`.

    Returns `{name: (lo, hi, median, n)}`, omitting any benchmark with fewer
    than `MIN_WINDOW_FOR_BAND` samples in the window -- the absence is the
    "cannot judge" answer, and callers must not read it as "fine".

    Why this exists
    ---------------
    `diff()` decides REGRESSED/IMPROVED from the **immediately preceding run**.
    For a tight benchmark that is correct and maximally sensitive; for a
    volatile one it compares two samples of the same noise and reports the
    difference as a change in the code. Measured, not assumed: `ipc_channel`
    was reported at `+31% vs suite` on 542 -> 688 ns while its own release
    history spans 420-1475 ns, with 688 sitting near its median.

    Why Tukey's fence and not the median/MAD band used elsewhere in this file
    ------------------------------------------------------------------------
    Consistency argued for MAD, and the data overruled it. MAD measures the
    width of the distribution's **core**, and these per-benchmark
    distributions are visibly clustered rather than unimodal: `ipc_channel`
    alternates between a ~545 ns and a ~650 ns cluster across builds. Over the
    eight runs preceding the motivating one its median is 648.5 with a MAD of
    7.5, so a 3-MAD band is **626-671 ns** -- narrower than the gap between
    that benchmark's own two clusters, on a benchmark whose observed span is
    420-1475. It flags 688, i.e. it would have reproduced the exact false
    positive this function exists to remove.

    Quartiles do not have that failure: a bimodal core *widens* the box instead
    of shrinking it. The same window gives a fence of **505-746 ns**, which
    declines 688 and still catches the following run's 953 (+40%).

    Neither constant was tuned. Replaying the ten most recent release
    comparisons in `bench/history.jsonl`: 47 movements cross the 25% threshold,
    of which MAD confirms 35 and this rule confirms 29 -- and the six they
    disagree on include the written-up `ipc_channel` non-event.

    Two-sided, unlike `robust_band()`: a benchmark's own value can genuinely
    move either way (a real optimisation is the whole point of the IMPROVED
    list), whereas the quantities `robust_band` grades are contaminated in one
    direction only.

    A degenerate zero IQR -- a benchmark that reads the same value every run --
    collapses the fence onto the quartiles, so *any* movement counts as outside
    it. That is safe here and deliberately unfloored, because this band is only
    ever applied as a **veto** on a movement that already crossed
    `threshold_pct`: it can demote a report, never create one.
    """
    acc = {}
    for record in records:
        for name, value in record.get("entries", {}).items():
            if value and value > 0:
                acc.setdefault(name, []).append(float(value))
    bands = {}
    for name, values in acc.items():
        if len(values) < MIN_WINDOW_FOR_BAND:
            continue
        q1, median, q3 = statistics.quantiles(values, n=4, method="inclusive")
        iqr = q3 - q1
        bands[name] = (q1 - k * iqr, q3 + k * iqr, median, len(values))
    return bands


#: How many of the most recent runs the level-shift reference window skips.
#:
#: This is the whole mechanism, so it is worth stating plainly: the reference
#: must not contain the regression it is meant to detect. Three is chosen
#: because it is one more than the number of runs it took to expose the problem
#: below (a regression that appeared in run N was already invisible in N+1), and
#: because a shift that has persisted for more than three runs on this project
#: is old enough that its own commits have been merged and the bisect range is
#: no longer small. Raising it makes the reference cleaner and the report later;
#: lowering it risks the reference absorbing the very shift being looked for.
LEVEL_SHIFT_SKIP = 3

#: How many *recorded* runs, in addition to the one being judged, must also be
#: off baseline before a shift is called sustained.
#:
#: Must be strictly less than `LEVEL_SHIFT_SKIP`, and is: the reference window
#: is `window[:-LEVEL_SHIFT_SKIP]` while the persistence window is
#: `window[-LEVEL_SHIFT_PERSIST:]`, so 2 < 3 guarantees the two never overlap
#: -- a run can never be evidence for a shift *and* part of the baseline that
#: shift is measured against. (It also leaves `window[-3]` in neither, a
#: one-run buffer against an off-by-one turning the baseline dirty.)
#:
#: Two is the smallest value that does any work at all: with the current run
#: that is three consecutive runs above the fence. Measured on the 26 recorded
#: runs, requiring only the current run fired on 11 (42%), almost all of them
#: known single-run host excursions; requiring three consecutive fires only on
#: the genuinely persistent ones. Raising it further would delay the report by
#: a full run each time for no measured gain -- and every run costs a boot.
LEVEL_SHIFT_PERSIST = 2

#: A benchmark this far off its pre-window baseline, after removing whole-suite
#: drift, is reported as a sustained shift. Same figure as the run-over-run
#: threshold so the two reports mean the same thing by "moved".
LEVEL_SHIFT_PCT = 25.0

#: Tukey's *extreme*-outlier multiplier (the textbook companion to the 1.5 used
#: for `TUKEY_K`), used for the level-shift fence only.
#:
#: A flat 25% threshold is not scale-aware, and for a benchmark whose own fence
#: is already ~20% wide it sits inside the noise. Measured: `ipc_channel_sync`
#: read 646 -> 967 -> 684 -> 578 against a baseline median of 530 and a 1.5-IQR
#: fence of 438-640. Three consecutive runs cleared both 25% and that fence --
#: and then it went back to 578, so it was noise. Its 3-IQR fence is 363-716,
#: which declines the run that fired. Over the same window
#: `http_build_response_1KiB` is outside its own 3-IQR fence of 5494-6649 in
#: all three runs, by a factor of two.
#:
#: CORRECTION (2026-08-15): that second example was described here as "a real
#: 2x regression" and is not one -- it is the code-layout lottery (see
#: `mode_structure()`), so this fence *reporting* it is a false positive, not
#: the true positive it was cited as. The constant is unchanged, because the
#: `ipc_channel_sync` half of the justification stands on its own and 3.0 is
#: Tukey's own extreme-outlier multiplier rather than a figure fitted to this
#: data. What removes the false positive is the mode-structure check, which no
#: fence on a single series could ever do: both modes are real values of the
#: metric, so no threshold placed among them is right.
#:
#: Using the wider fence here rather than everywhere is deliberate: elsewhere
#: the band vetoes a single-run movement that other checks still watch, whereas
#: this report claims a *durable* shift worth bisecting for, is deliberately
#: delayed by `LEVEL_SHIFT_PERSIST` runs before it speaks, and is wired into
#: `--fail-on-regression`. It should be correspondingly harder to trip. Like
#: 1.5, this constant is Tukey's, not one fitted to this project's data.
LEVEL_SHIFT_TUKEY_K = 3.0


def level_shifts(records, host, profile, current, threshold_pct=LEVEL_SHIFT_PCT):
    """Benchmarks sitting far off a baseline that PREDATES the recent runs.

    Returns `[(name, reference_median, value, adjusted_pct, band, n)]`, worst
    first, or `[]` when there is not enough clean history to judge.

    Why this exists (a real miss, not a hypothetical)
    -------------------------------------------------
    Everything else in `diff()` is anchored to the **immediately preceding
    run**, and the per-benchmark band is explicitly only a *veto* -- it "can
    demote a report, never create one". Both properties are right for what they
    do, and together they leave one specific hole:

    A regression that appears and then **persists** is reported exactly once.
    On the very next run, (1) run-over-run sees no movement, so the benchmark is
    never a candidate and the band is never consulted; and (2) the trailing
    window has meanwhile absorbed the elevated sample, so even if it were
    consulted it would answer "within range".

    Measured on this repo, 2026-08-15. `http_build_response_1KiB` sat at
    ~6000 ns for nine runs, then read 8546 -> 12431 -> 12407. The run that
    *confirmed* the regression (12431 -> 12407, agreeing to 0.2%) printed:

        No benchmark moved outside its own recent range

    which is true, and reads as "no regressions", and means "nothing changed
    since the regression". The window poisons itself, fastest for exactly the
    regressions that matter most -- a persistent one appears in every
    subsequent run by definition.

    How the reference is kept clean
    -------------------------------
    The reference window skips the most recent `LEVEL_SHIFT_SKIP` runs and takes
    the `SPEED_WINDOW` runs before those. A shift introduced in the last one to
    three runs therefore cannot have entered its own baseline.

    Whole-suite drift is removed the same way `report_run_position` does it, via
    `speed_factor` against the reference medians. Without that, a run on a
    busier host would light up every benchmark at once; with it, only a
    benchmark that moved *relative to its peers* is reported. That matters more
    here than for run-over-run comparisons, because the reference is deliberately
    older and so more likely to differ in machine conditions.

    The Tukey band from the same reference window is attached and used as a
    veto, exactly as `per_benchmark_bands` is used elsewhere: a benchmark whose
    own pre-window spread already covered the new value is not a finding --
    though at `LEVEL_SHIFT_TUKEY_K`, a wider fence than the rest of the file
    uses, for the reasons recorded on that constant.

    What it actually fires on (replayed causally over all 26 recorded runs,
    each judged against only the runs before it)
    -----------------------------------------------------------------------
    ==============================  ========  ==================================
    version                         fires on  notes
    ==============================  ========  ==================================
    newest run vs clean baseline    11 (42%)  `net_ipv6_parse +110%`,
                                              `page_fault +103%`, ... all known
                                              single-run host excursions
    + persistence                    2 (7.7%) target kept; one survivor
    + symmetric test                 2 (7.7%) no change -- see `_shift_pct`
    + extreme fence                  1 (3.8%) survivor dropped; target kept
    ==============================  ========  ==================================

    The one surviving firing is the true positive: `http_build_response_1KiB`
    at run 25 -- the run whose report had read "No benchmark moved outside its
    own recent range".

    Known limitation, stated rather than hidden: this inherits the flat 25%
    threshold, so the concurrent `vfs_stat_root` shift (~3600 -> ~4450, +23%)
    is below it and is NOT reported here. That is the same blind spot the
    run-over-run path has, not a new one.
    """
    if records is None or host is None:
        return []
    window = comparable_records(records, host, profile)
    # Causal and clean: drop the run being judged and the ones that could
    # already contain the shift, then take the window before them.
    reference = window[:-LEVEL_SHIFT_SKIP][-SPEED_WINDOW:] if len(
        window) > LEVEL_SHIFT_SKIP else []
    if len(reference) < MIN_WINDOW_FOR_BAND:
        return []

    medians = per_benchmark_median(reference)
    bands = per_benchmark_bands(reference, k=LEVEL_SHIFT_TUKEY_K)

    # PERSISTENCE IS THE WHOLE DISCRIMINATOR -- measured, not assumed.
    #
    # A first version of this compared only the newest run against the clean
    # baseline. Replayed over the 26 recorded runs it fired on 11 of them (42%),
    # and the firings included `net_ipv6_parse +110%` and `page_fault +103%` on
    # a run where both returned to baseline immediately afterwards. Those are
    # precisely the single-run excursions the existing machinery already grades.
    #
    # The reason drift correction does not save it: contamination is not a
    # uniform slowdown but a heavy tail. `speed_factor` removes the *median*
    # shift, so a run where a few benchmarks stalled badly and the rest did not
    # comes out with its tail intact and looking like several simultaneous
    # regressions.
    #
    # Persistence separates the two cleanly, because it keys on the one property
    # that actually differs: host disturbance is random per run, while a code
    # regression is in every run after the commit. So a benchmark must sit above
    # the fence in the newest run AND in the ones just before it.
    recent = window[-LEVEL_SHIFT_PERSIST:]
    if len(recent) < LEVEL_SHIFT_PERSIST:
        return []

    def _corrected(entries):
        """Entries divided by that run's own whole-suite factor."""
        factor = speed_factor(entries, medians) or 1.0
        return {n: v / factor for n, v in entries.items() if v and v > 0}

    corrected_recent = [_corrected(r.get("entries", {})) for r in recent]
    corrected_current = _corrected(current)

    def _shift_pct(entries_map, name):
        """How far `name` is above baseline in one run, or None if not shifted.

        Deliberately the *same* test for a past run as for the run being
        judged. An earlier version applied only the band veto to the past runs
        and the full threshold to the current one, on the reasoning that the
        past runs merely had to corroborate.

        Making it symmetric is a correctness fix, not a measured improvement,
        and the distinction is worth keeping straight: replayed over the 26
        recorded runs it changed the firing rate not at all (2/26 before and
        after). The false positive it was aimed at -- `ipc_channel_sync`,
        646 -> 967 -> 684 -> 578 -- survived it, because after drift correction
        its 646 reads +25.1%, over the line by a tenth of a point. What
        actually removed that one was `LEVEL_SHIFT_TUKEY_K`.

        It stays because the weaker version let the report *mean* something it
        did not say: "sustained shift of >25%" could be printed for a series
        that was never 25% off in any run but the last.
        """
        value = entries_map.get(name)
        if value is None:
            return None
        median = medians.get(name)
        if not median or median <= 0:
            return None
        band = bands.get(name)
        # Unjudgeable benchmarks are dropped rather than reported: unlike the
        # run-over-run path, this check is not the only thing watching them, and
        # a noisy unbanded benchmark would fire here every single run.
        if not band or band_position(value, band, True) != BAND_OUTSIDE:
            return None
        pct = (value / median - 1.0) * 100.0
        return pct if pct >= threshold_pct else None

    rows = []
    for name in corrected_current:
        adjusted = _shift_pct(corrected_current, name)
        if adjusted is None:
            continue
        # ...and the identical finding must hold in each of the preceding runs.
        if any(_shift_pct(run, name) is None for run in corrected_recent):
            continue
        rows.append((name, medians[name], current[name], adjusted,
                     bands[name], len(reference)))
    rows.sort(key=lambda r: -r[3])
    return rows


#: Verdicts from `mode_structure()`.
#:
#: `MODE_STRUCTURED` -- the split separates *binaries*: every commit measured
#: more than once sits wholly on one side, and both sides are occupied. The
#: "shift" is then a property of the compiled image (code layout), not of the
#: code's speed, and bisecting for a guilty commit is the wrong tool.
#: `MODE_RUN_NOISE` -- some single commit's own repeats span the split, so the
#: split is inside run-to-run noise and cannot mark a durable change.
#: `MODE_UNDECIDED` -- not enough repeat measurements to say either way.
MODE_STRUCTURED = "mode-structured"
MODE_RUN_NOISE = "run-noise"
MODE_UNDECIDED = "undecided"

#: How many commits must have been measured more than once before the check
#: will return anything but `MODE_UNDECIDED`.
#:
#: Two, because one is not enough to demonstrate *both* sides: a single
#: repeated commit can show that it does not straddle the split, but not that
#: any binary ever lands on the other side, and "all repeats are below the
#: fence" is equally consistent with the fence simply being too high to reach.
MODE_MIN_REPEAT_COMMITS = 2

ModeVerdict = collections.namedtuple(
    "ModeVerdict", "verdict repeats straddling below above"
)


def repeats_by_commit(records, host, profile, name):
    """`{commit: [values]}` for commits measured more than once.

    Ordered by first appearance so the report is stable across runs.
    """
    by_commit = collections.OrderedDict()
    for record in comparable_records(records, host, profile):
        value = record.get("entries", {}).get(name)
        commit = record.get("commit")
        if value is None or not commit:
            continue
        by_commit.setdefault(commit, []).append(value)
    return collections.OrderedDict(
        (commit, values) for commit, values in by_commit.items() if len(values) > 1
    )


def mode_structure(records, host, profile, name, split):
    """Does `split` separate *binaries*, or merely *runs*?

    This is the question a "sustained shift" report cannot answer on its own,
    and getting it wrong costs a bisect. Returns a `ModeVerdict`.

    Why this exists (a real miss, not a hypothetical)
    -------------------------------------------------
    `http_build_response_1KiB` was reported as a sustained 2x regression and
    bisected across three commits. It is not a regression. Over 20 release
    records it is cleanly bimodal -- 11 runs averaging 6055 ns and 9 averaging
    10806 ns, with an *empty* gap between 6396 and 8546 and a ratio of 1.78x,
    which is the TCG page-straddle penalty. The mode is a deterministic
    property of the compiled image: it re-rolls when unrelated code moves a
    function's address, and it had already flipped back and forth five times
    across the recorded history. There was no guilty commit to find.

    What makes that decidable is **repeat measurements of the same commit**.
    Three commits had been measured more than once (seven runs in total) and
    not one of them straddled the gap, while host noise moved values by up to
    1.47x *within* a mode. Same binary, same mode, every time.

    Why not a gap test
    ------------------
    The obvious detector -- "is there a suspiciously large gap in the sorted
    values" -- was implemented and measured first, and it does not
    discriminate. Scoring the largest gap against the median spacing of the
    sorted values gives 12.9x for `http_build_response_1KiB`, but 13.3x for
    `vfs_stat_root`, 30x for `ipc_eventfd` and 113x for `page_alloc_free`.
    Every benchmark looks bimodal by that measure, because 20 samples drawn
    from a tight distribution are densely spaced and *any* outlier dwarfs the
    median spacing. A check that fires on everything is as useless as one that
    fires on nothing; the repeat-commit test is used instead because it was
    measured to separate these same four series correctly.

    Choice of `split`
    -----------------
    This function answers the question *for the split it is given*; choosing a
    good one is `mode_split_search`'s job, and it matters more than it looks.
    Two plausible fixed choices were tried against the real history and both
    give the wrong answer for `http_build_response_1KiB`: the midpoint between
    baseline and current (~9200) and the report's own pre-window Tukey fence
    (9103) both land *inside* the HIGH mode's run-to-run spread, so commit
    `26c1c7330` (8818, 11381, 12934) straddles them and the verdict comes back
    `run-noise` for a series that is genuinely mode-structured. The gap that
    actually separates the modes is at ~7500, between 6396 and 8546, and only a
    search over observed values finds it.
    """
    repeats = repeats_by_commit(records, host, profile, name)
    straddling = collections.OrderedDict(
        (commit, values)
        for commit, values in repeats.items()
        if min(values) < split <= max(values)
    )
    below = [c for c, v in repeats.items() if max(v) < split]
    above = [c for c, v in repeats.items() if min(v) >= split]

    if len(repeats) < MODE_MIN_REPEAT_COMMITS:
        verdict = MODE_UNDECIDED
    elif straddling:
        verdict = MODE_RUN_NOISE
    elif below and above:
        verdict = MODE_STRUCTURED
    else:
        # No repeat straddles the split, but they all sit on one side of it, so
        # nothing has been shown about the other side.
        verdict = MODE_UNDECIDED
    return ModeVerdict(verdict, repeats, straddling, below, above)


def mode_split_search(records, host, profile, name, low, high):
    """Best `(split, ModeVerdict)` separating `low` from `high`, or `None`.

    Searches every split that could separate the baseline from the current
    value and returns one that is `MODE_STRUCTURED`, preferring the widest gap.

    Why a search rather than the report's own fence
    -----------------------------------------------
    Passing the pre-window Tukey fence looks principled and fails on the real
    data, which is why it is not what happens. For `http_build_response_1KiB`
    that fence is 9103 ns, because the baseline window had itself already
    absorbed runs from *both* modes and widened accordingly. 9103 sits inside
    the HIGH mode's own run-to-run spread, so commit `26c1c7330`
    (8818, 11381, 12934) straddles it and the answer comes back `run-noise` --
    true of that fence, but not the fact worth reporting. The series really is
    mode-structured; the fence was simply in the wrong place to see it.

    Splits are taken at the midpoint of each adjacent pair of observed values,
    restricted to `(low, high]` so that only splits which actually separate the
    baseline from the current reading are considered. The widest gap is
    preferred because that is the split least likely to be an artefact of where
    a single sample happened to land.
    """
    values = sorted({
        record["entries"][name]
        for record in comparable_records(records, host, profile)
        if name in record.get("entries", {})
    })
    candidates = []
    for lower, upper in zip(values, values[1:]):
        split = (lower + upper) / 2.0
        if not low < split <= high:
            continue
        candidates.append((upper - lower, split))
    # Widest gap first, so a tie between splits resolves to the most separated.
    for _gap, split in sorted(candidates, reverse=True):
        verdict = mode_structure(records, host, profile, name, split)
        if verdict.verdict == MODE_STRUCTURED:
            return split, verdict
    return None


def describe_mode_verdict(name, verdict):
    """Lines to print under a shift report, or `[]` when there is nothing to say."""
    if verdict.verdict == MODE_STRUCTURED:
        sides = (
            f"{len(verdict.below)} commit(s) always below it, "
            f"{len(verdict.above)} always above"
        )
        return [
            f"    -> {name}: NOT a regression to bisect. Every commit measured "
            f"more than once sits wholly on one side of this fence "
            f"({sides}), so the fence separates BINARIES, not runs.",
            "       This is the code-layout lottery: the mode re-rolls when "
            "unrelated code shifts an address. Bisecting will name an "
            "innocent commit.",
        ]
    if verdict.verdict == MODE_RUN_NOISE:
        commit = next(iter(verdict.straddling))
        values = verdict.straddling[commit]
        return [
            f"    -> {name}: treat with suspicion. Commit {commit[:9]} alone "
            f"produced {sorted(values)} -- one binary spanning this fence, so "
            f"the fence is inside run-to-run noise.",
        ]
    return []


#: Replication verdicts for a movement that crossed the run-over-run threshold
#: *and* left the benchmark's own band -- i.e. everything that would otherwise
#: have been printed as a confirmed `REGRESSED`.
#:
#: `REPLICATED`   -- every recorded measurement of this same commit shows it.
#: `UNREPLICATED` -- this commit has been measured once, so nothing has been
#:                   shown either way.
#: `CONTRADICTED` -- another run of this *same binary* did not show it.
REPLICATED = "replicated"
UNREPLICATED = "unreplicated"
CONTRADICTED = "contradicted"

#: How many measurements of one commit are needed before replication can be
#: judged at all. Two, because one run cannot disagree with itself.
REPLICATION_MIN_RUNS = 2

#: What `git_commit()` returns when it could not read HEAD. It must never be
#: treated as a commit identity: two runs that both failed to read HEAD are not
#: two runs of the same binary, and matching them would manufacture exactly the
#: replication this gate exists to demand.
UNKNOWN_COMMIT = "unknown"

ReplicationVerdict = collections.namedtuple("ReplicationVerdict", "verdict values")


def values_for_commit(records, host, profile, name, commit):
    """Every comparable measurement of `name` already recorded for `commit`."""
    if not commit or commit == UNKNOWN_COMMIT:
        return []
    return [
        value
        for record in comparable_records(records, host, profile)
        if record.get("commit") == commit
        and (value := record.get("entries", {}).get(name)) is not None
    ]


def replication_verdict(records, host, profile, name, commit, observed, band):
    """Did a second run of this *same binary* also produce this movement?

    Why this gate exists, and why nothing cheaper works
    ---------------------------------------------------
    Two `boot-test.sh --bench` runs of commit `602fc62e0`, 2.5 minutes apart
    with nothing rebuilt between them, were compared against each other -- an
    A/A test, where every reported regression is a false positive by
    construction. The harness reported three confirmed ones across the pair
    (`pick_next` +92%, then `page_alloc_free` +85% and `vfs_stat_breakdown_full`
    +36%), and the second of those runs was graded `RUN CLEAN` by every
    contamination instrument while it did so. Drift-corrected, 5 of 83
    benchmarks moved by more than 25% on identical code.

    The band is not the weak link and cannot be made into the fix. It is
    already Tukey's fence over quartiles, it was *right* about these values --
    `page_alloc_free` sits at 293-453ns and genuinely measured 680 -- and a
    sweep over the real history showed more history makes the fence *tighter*
    (window 27 gives 309-427), because tail events this rare never move a
    quartile. No `(window, k)` setting separates these false positives from
    real regressions, because on the evidence available within one run the two
    are not different.

    What separates them is structural rather than statistical: **a code-caused
    regression reproduces on the same binary and an environmental outlier does
    not.** So the question asked here is not "how unlikely is this number?" but
    "did this same binary produce it twice?", which is the only question whose
    answer distinguishes the two causes. The cost is one extra boot with no
    rebuild.

    `records` must *exclude* the current run -- the same contract the band
    computation in `report()` already relies on -- because `observed` is added
    to the sample here. Passing a history that already contains this run would
    count it twice and let a single run replicate itself.

    A row with no band is `UNREPLICATED` regardless: there is no fence for a
    repeat to land inside of, so nothing can be contradicted. Those rows are
    already printed as UNCONFIRMED and are deliberately left alone rather than
    judged against an invented fence.
    """
    if band is None:
        return ReplicationVerdict(UNREPLICATED, [observed])
    values = values_for_commit(records, host, profile, name, commit) + [observed]
    if len(values) < REPLICATION_MIN_RUNS:
        return ReplicationVerdict(UNREPLICATED, values)
    _lo, hi, _median, _n = band
    if min(values) > hi:
        return ReplicationVerdict(REPLICATED, values)
    return ReplicationVerdict(CONTRADICTED, values)


def describe_replication(name, verdict, band):
    """Lines to print beneath a contradicted movement.

    Also states the benchmark's *measured* A/A spread, which these repeats are
    the only source of: "this binary produced 367 and 680 ns" is a noise floor
    for `name` on this host, and a floor of 85% is the fact that decides
    whether any smaller movement in it is judgeable at all.
    """
    if verdict.verdict != CONTRADICTED or not verdict.values:
        return []
    _lo, hi, _median, _n = band
    values = sorted(verdict.values)
    # The smallest sample *is* the contradicting one -- it is precisely what
    # `replication_verdict` compared against `hi` to reach CONTRADICTED -- and
    # it is never this run's own value: a row only reaches the gate after
    # `band_position` found it above `hi`, so the current value cannot be the
    # minimum of a set whose minimum is at or below `hi`. Taking `min(values)`
    # rather than filtering for `<= hi` and taking the minimum of *that* is the
    # same number by construction, and cannot be an empty list.
    lines = [
        f"    -> {name}: another run of this same binary measured "
        f"{values[0]:.0f}ns, inside the {hi:.0f}ns edge this claim depends "
        f"on leaving. Same code, both numbers.",
    ]
    if values[0] > 0:
        spread = (values[-1] / values[0] - 1.0) * 100.0
        lines.append(
            f"       same-commit runs: {[round(v) for v in values]} -- a "
            f"{spread:.0f}% spread with no code change, which is this "
            f"benchmark's measured noise floor on this host."
        )
    return lines


def band_position(value, band, worse):
    """Is `value` outside `band` in the direction that matters?

    `worse` is True for a claimed regression (only the upper edge can confirm
    it) and False for a claimed improvement (only the lower edge can). A
    movement that crossed the run-over-run threshold but landed inside the
    benchmark's own historical spread is `BAND_WITHIN` -- not evidence of
    anything, and specifically not evidence that the code is fine either.

    No band at all returns `BAND_UNJUDGED`, and callers must keep reporting
    those: too little history is a reason to withhold the *word* "regressed",
    never a reason to hide the movement. A new benchmark's first real
    regression would otherwise be silenced by the very fact that it is new.
    """
    if band is None:
        return BAND_UNJUDGED
    lo, hi, _median, _n = band
    if worse:
        return BAND_OUTSIDE if value > hi else BAND_WITHIN
    return BAND_OUTSIDE if value < lo else BAND_WITHIN


def describe_band(band):
    """Human-readable form of one `per_benchmark_bands` entry."""
    lo, hi, median, n = band
    return (f"its own range is {max(lo, 0.0):.0f}-{hi:.0f}ns "
            f"(median {median:.0f}ns over {n} runs)")


def split_by_band(flagged, bands, worse):
    """Partition threshold-crossing movements by `band_position`.

    Returns `(outside, within, unjudged)`, each a list of the input tuples with
    the benchmark's band appended.
    """
    outside, within, unjudged = [], [], []
    buckets = {BAND_OUTSIDE: outside, BAND_WITHIN: within,
               BAND_UNJUDGED: unjudged}
    for entry in flagged:
        name, _before, after = entry[0], entry[1], entry[2]
        band = bands.get(name)
        buckets[band_position(after, band, worse)].append(entry + (band,))
    return outside, within, unjudged


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
           records=None, host=None, profile=LEGACY_PROFILE, commit=None):
    """Print the run-over-run comparison. Returns True if anything regressed.

    `records`/`host`/`profile` are optional only so that callers interested
    purely in the run-over-run diff (the tests, chiefly) need not construct a
    history.  When they are supplied, two things change: the run is placed
    against the recent history for this host (`report_run_position`), and each
    threshold-crossing movement is checked against that benchmark's own recent
    range (`per_benchmark_bands`) before being called a regression.  The
    run-over-run diff alone is what produced two written-up regressions that
    never existed, and `ipc_channel`'s "+31%" on a move well inside its own
    420-1475ns span was a third.

    Without a history every movement comes back UNCONFIRMED rather than
    silently confirmed: a caller that supplies no records has not shown the
    benchmark to be stable, and must not be told that it has.

    `commit` is this run's HEAD.  It is what makes the replication gate
    possible -- without it no movement can be shown to have survived a second
    run of the same binary, so every banded regression degrades to
    UNREPLICATED.  See `replication_verdict`.
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

    # Is the baseline the *same binary* as this run? Then the whole
    # run-over-run comparison is an A/A test, and its result is known before it
    # is computed: nothing in it can have been caused by code. That is not a
    # statistical claim to be weighed against the numbers, it is arithmetic --
    # the two runs share a commit, so the diff between them has no code term.
    #
    # Said here, above every list, because the failure it prevents is one that
    # actually happened: a `pick_next` +92% from exactly this situation was
    # written up as a scheduler regression, corroborated by a second statistic,
    # and believed. The band cannot notice this and neither can the canary;
    # only the commit field can, and it was already in the record.
    same_binary = bool(
        commit
        and commit != UNKNOWN_COMMIT
        and previous.get("commit") == commit
    )
    if same_binary:
        print(
            f"  !! A/A COMPARISON: the baseline run is the SAME commit "
            f"({commit}) as this one, so no\n"
            f"     movement below can have been caused by code -- every "
            f"difference is this host's\n"
            f"     measurement noise, by construction, and none of it is "
            f"counted as a regression.\n"
            f"     What such a pair *does* measure is the per-benchmark noise "
            f"floor: the last one\n"
            f"     moved 5 of 83 benchmarks by more than 25%. See "
            f"known-issues.md\n"
            f"     B-BENCH-CONFIRMED-REGRESSIONS-FIRE-ON-AN-UNCHANGED-BINARY."
        )
    print(
        "  Comparison is run-over-run on this host, which cancels the TCG "
        "emulation constant; a movement is only called a regression if it "
        "also leaves that benchmark's own recent range."
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

    # Each threshold-crossing movement is now checked against the benchmark's
    # *own* recent spread before it is called a regression. The window is the
    # same `comparable_records` one every other historical judgement here uses,
    # and it holds only records that precede this run, so a verdict printed at
    # boot still reads the same a week later.
    if records is not None and host is not None:
        bands = per_benchmark_bands(
            comparable_records(records, host, profile)[-SPEED_WINDOW:]
        )
    else:
        bands = {}

    reg_out, reg_within, reg_unjudged = split_by_band(regressed, bands, True)
    imp_out, imp_within, imp_unjudged = split_by_band(improved, bands, False)

    # A movement whose *own* measurement window was unstable is withdrawn from
    # the claim lists entirely.
    #
    # This is a different disqualification from the band and from the canary,
    # and it catches what neither can. The band asks "is this size of movement
    # normal for this benchmark?" -- it cannot tell a real jump from a
    # measurement taken while the floor was moving, because both look like a
    # large number. The canary asks "was the host busy?" -- but it samples
    # between benchmarks, so a burst that lands squarely inside one benchmark's
    # window and ends before the next sample is invisible to it. The split-
    # sample check is taken *inside* the window being questioned, which is the
    # only place the answer exists.
    #
    # Withdrawn, not demoted: these are not "small movements", they are
    # movements whose measurement is void. Printing them under a heading that
    # implies a finding is how the earlier over-claiming warnings taught
    # readers to stop believing the instrument.
    def _split_token(name):
        vals = current_entries.get(name)
        return vals[5] if vals is not None and len(vals) > 5 else SPLIT_ABSENT

    def _withdraw_unstable(rows):
        keep, void = [], []
        for row in rows:
            (void if split_is_unstable(_split_token(row[0])) else keep).append(row)
        return keep, void

    reg_out, reg_void = _withdraw_unstable(reg_out)
    reg_unjudged, reg_unjudged_void = _withdraw_unstable(reg_unjudged)
    imp_out, imp_void = _withdraw_unstable(imp_out)
    imp_unjudged, imp_unjudged_void = _withdraw_unstable(imp_unjudged)
    void_rows = reg_void + reg_unjudged_void + imp_void + imp_unjudged_void

    # The replication gate, applied last because it is the strongest claim and
    # the one the word "REGRESSED" is now reserved for. A movement that left
    # the band is asked one further question: did this *same binary* produce it
    # more than once? Measured A/A evidence says nothing weaker discriminates
    # (see replication_verdict.__doc__ and known-issues.md
    # B-BENCH-CONFIRMED-REGRESSIONS-FIRE-ON-AN-UNCHANGED-BINARY).
    #
    # Three outcomes, and the asymmetry between the last two is the whole
    # design. CONTRADICTED is withdrawn outright and does not fail the build:
    # a repeat of the same binary landed back inside the range, so the
    # excursion is demonstrably not in the code. UNREPLICATED keeps failing it:
    # nobody has shown anything either way, and letting "measured once" excuse
    # a movement would silence the check in the ordinary case -- one run per
    # commit is the norm -- which is the same failure as a check that cannot
    # fire. This mirrors MODE_UNDECIDED, which likewise only ever excuses a
    # *positively evidenced* verdict.
    reg_repl, reg_unrep, reg_contra = [], [], []
    repl_verdicts = {}
    for row in reg_out:
        name, _before, after, _raw, _adj, band = row
        rv = replication_verdict(records, host, profile, name, commit,
                                 after, band)
        repl_verdicts[name] = rv
        if rv.verdict == REPLICATED:
            reg_repl.append(row)
        elif rv.verdict == CONTRADICTED:
            reg_contra.append(row)
        else:
            reg_unrep.append(row)

    def _print_movements(header, rows, key):
        print(header)
        for name, before, after, raw, adj, band in sorted(rows, key=key):
            detail = f"; {describe_band(band)}" if band else ""
            print(
                f"    {name}: {before}ns -> {after}ns "
                f"({adj:+.0f}% vs suite, {raw:+.0f}% raw){detail}"
            )

    worst_first = lambda r: -r[4]  # noqa: E731 - reads better inline
    best_first = lambda r: r[4]    # noqa: E731

    if reg_repl:
        _print_movements(
            f"  REGRESSED (>{threshold_pct:g}% slower than the suite, outside "
            f"its own recent range, AND replicated -- every recorded run of "
            f"this commit shows it):", reg_repl, worst_first)
    if reg_unrep:
        _print_movements(
            f"  REGRESSED, UNREPLICATED (>{threshold_pct:g}% slower than the "
            f"suite and outside its own recent range, but this commit has "
            f"been measured only once):", reg_unrep, worst_first)
        print(
            "    -> re-run `./scripts/boot-test.sh --bench` WITHOUT "
            "rebuilding to confirm. Two runs of one\n"
            "       unchanged binary have been measured moving 85% apart, so "
            "a single run cannot tell a\n"
            "       code regression from an environmental outlier. Still "
            "counted as a regression until it is\n"
            "       either replicated or contradicted -- unmeasured is not "
            "the same as absolved."
        )
    if reg_contra:
        print(
            "  NOT REPLICATED (crossed the threshold and left its own range, "
            "but another run of this SAME binary did not -- withdrawn, and "
            "NOT counted as a regression):"
        )
        for name, before, after, raw, adj, band in sorted(reg_contra,
                                                          key=worst_first):
            print(
                f"    {name}: {before}ns -> {after}ns "
                f"({adj:+.0f}% vs suite, {raw:+.0f}% raw); "
                f"{describe_band(band)}"
            )
            for line in describe_replication(name, repl_verdicts[name], band):
                print(line)
    if reg_unjudged:
        _print_movements(
            f"  REGRESSED, UNCONFIRMED (>{threshold_pct:g}% slower than the "
            f"suite; too few prior runs to know this benchmark's spread):",
            reg_unjudged, worst_first)
    if imp_out:
        _print_movements(
            f"  IMPROVED (>{threshold_pct:g}% faster than the suite AND "
            f"outside its own recent range):", imp_out, best_first)
    if imp_unjudged:
        _print_movements(
            f"  IMPROVED, UNCONFIRMED (>{threshold_pct:g}% faster than the "
            f"suite; too few prior runs to know this benchmark's spread):",
            imp_unjudged, best_first)
    if reg_within or imp_within:
        _print_movements(
            f"  WITHIN ITS OWN RANGE (crossed {threshold_pct:g}% run-over-run, "
            f"but landed inside this benchmark's own recent spread -- this is "
            f"NOT a finding in either direction):",
            reg_within + imp_within, worst_first)
    if void_rows:
        print(
            "  MEASUREMENT VOID (the first and second halves of this run's "
            "measurement window disagreed, so the run's own noise floor moved "
            "*during* the window -- the number below is not a measurement of "
            "anything and is NOT counted as a regression):"
        )
        for name, before, after, raw, adj, band in sorted(void_rows,
                                                          key=worst_first):
            pct = split_pct(_split_token(name))
            spread = f"; sample sets {pct}% apart" if pct is not None else ""
            print(
                f"    {name}: {before}ns -> {after}ns "
                f"({adj:+.0f}% vs suite, {raw:+.0f}% raw){spread}"
            )
        print(
            "    -> re-run the suite; if the same benchmark voids repeatedly "
            "on a quiet host,\n"
            "       it is not ambient load but the benchmark alternating "
            "between two populations."
        )
    # Independent of everything above: a sustained shift is invisible to the
    # run-over-run comparison by construction (see level_shifts.__doc__), so it
    # is computed from its own pre-window reference and printed unconditionally
    # -- including on runs where the run-over-run lists are empty, which is
    # exactly when it is most needed.
    shifts = level_shifts(records, host, profile, current) if records else []
    # Shifts that survive the mode-structure check -- i.e. the ones actually
    # worth bisecting for. Only these fail the build; see the return below.
    bisectable_shifts = []
    if shifts:
        print(
            f"  SUSTAINED SHIFT (>{LEVEL_SHIFT_PCT:g}% off a baseline from "
            f"before the last {LEVEL_SHIFT_SKIP} runs, and outside that "
            f"baseline's own spread -- these do NOT show up run-over-run once "
            f"they persist):"
        )
        any_structured = False
        for name, median, value, adjusted, band, n in shifts:
            lo, hi, _med, _n = band
            print(
                f"    {name}: was ~{median:.0f}ns -> now {value}ns "
                f"({adjusted:+.0f}% vs suite); pre-window baseline "
                f"{lo:.0f}-{hi:.0f}ns over {n} runs"
            )
            # Before pointing anyone at a bisect, ask whether this fence
            # separates binaries or runs. `http_build_response_1KiB` was
            # bisected across three commits before anyone asked; the answer was
            # "binaries", and there was no guilty commit. See mode_structure().
            found = mode_split_search(records, host, profile, name,
                                      median, value)
            verdict = found[1] if found else mode_structure(
                records, host, profile, name, hi)
            for line in describe_mode_verdict(name, verdict):
                print(line)
            if verdict.verdict == MODE_STRUCTURED:
                any_structured = True
            else:
                bisectable_shifts.append(name)
        # Name the first thing to rule out. Under TCG a loop that lands across
        # a 4 KiB guest page costs ~1.7x per iteration, and any commit that
        # shifts a function's address re-rolls that -- so a sustained shift in
        # a benchmark whose code nobody touched is code layout until shown
        # otherwise. It is also the cheapest hypothesis to test (no boot, two
        # disassemblies), which is the argument for checking it *before*
        # reading diffs rather than after. See known-issues.md
        # B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x.
        print(
            "    -> rule out code layout before reading the diff:\n"
            "       python scripts/straddle-check.py --compare "
            "<old-kernel-elf> <new-kernel-elf>"
        )
        if any_structured:
            print(
                "    -> at least one shift above is mode-structured and is "
                "NOT counted as a regression by --fail-on-regression."
            )

    if added:
        print("  NEW:")
        for name, measured in added:
            print(f"    {name}: {measured}ns")
    if removed:
        print("  GONE (present last run, absent now):")
        for name in removed:
            print(f"    {name}")
    # Every "nothing found" line below is qualified by `not shifts`. An
    # unqualified all-clear printed alongside a SUSTAINED SHIFT block would be
    # the original bug wearing a new hat: the reader takes the summary line as
    # the verdict, and the summary line was only ever about run-over-run.
    if not (regressed or improved or added or removed or shifts):
        if drift:
            print(
                f"  No benchmark moved by more than {threshold_pct:g}% "
                f"relative to the suite."
            )
        else:
            print(f"  No benchmark moved by more than {threshold_pct:g}%.")
    elif not (reg_repl or reg_unrep or reg_unjudged or imp_out or imp_unjudged
              or added or removed or shifts):
        # Everything that crossed the threshold was demoted or withdrawn. Say
        # so, rather than printing only the demoted list and leaving the reader
        # to work out that nothing was found -- the whole point of the band is
        # that this outcome is common and unremarkable.
        #
        # The voided count is named separately and never folded into the
        # demoted one. "Landed inside its own range" is a finding of no change;
        # "its measurement was void" is no finding at all, and a summary line
        # that reported them together would let an unmeasurable suite read as a
        # quiet one -- which is the same failure as a check that cannot fire.
        if void_rows:
            print(
                f"  No benchmark moved outside its own recent range "
                f"({len(reg_within) + len(imp_within)} crossed "
                f"{threshold_pct:g}% run-over-run), but {len(void_rows)} "
                f"crossing movement(s) could not be judged at all because "
                f"their measurement was void -- see above."
            )
        else:
            print(
                f"  No benchmark moved outside its own recent range "
                f"({len(reg_within) + len(imp_within)} crossed "
                f"{threshold_pct:g}% run-over-run and are listed above)."
            )

    # The return value drives --fail-on-regression, so it must count only what
    # is still being *claimed* as a regression: confirmed ones and the ones
    # with too little history to judge. A movement inside the benchmark's own
    # spread failing the build is exactly the false positive this fixes.
    #
    # Sustained shifts count too. They are held to a strictly higher bar than
    # the run-over-run claims (off a clean pre-window baseline AND outside that
    # baseline's Tukey fence), so anything reaching this point is better
    # evidenced than the reports that already fail the build -- and a
    # regression that persists is the one most worth failing on, not least.
    #
    # `bisectable_shifts`, not `shifts`: a mode-structured shift is not a
    # regression at all (see mode_structure()), and failing the build on one
    # would gate merges on a coin flip -- `http_build_response_1KiB` re-rolled
    # between its two modes five times across the recorded history without any
    # commit making it slower. Note that only a *positively evidenced*
    # mode-structured verdict is excused; MODE_UNDECIDED still fails, so the
    # absence of repeat measurements can never silence the report.
    #
    # `reg_repl or reg_unrep`, not `reg_out`: the two differ by exactly
    # `reg_contra`, and a contradicted movement is one where another run of the
    # *same binary* landed back inside the range. That is positive evidence
    # that the excursion is not in the code -- the same standard the
    # mode-structured excuse is held to -- and it is the only category here
    # withdrawn on that basis. UNREPLICATED still fails; see the gate above.
    #
    # `same_binary` empties the run-over-run side entirely: a comparison of a
    # binary against itself cannot evidence a code regression, so failing the
    # build on one gates merges on the host's mood. Sustained shifts are
    # deliberately *not* excused by it -- `level_shifts` measures against a
    # baseline drawn from before the last LEVEL_SHIFT_SKIP runs, which are
    # other commits, so that comparison is not self-referential and keeps its
    # force even when the immediately preceding run happens to be a repeat.
    run_over_run = [] if same_binary else (reg_repl + reg_unrep + reg_unjudged)
    return bool(run_over_run or bisectable_shifts)


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
    for index, record in enumerate(records):
        entries = record.get("entries", {})
        over = record.get("over_target", "?")
        verdict = canary_verdict(record.get("canary"))
        if verdict == CANARY_BROKEN:
            broken += 1
        stalls = dispersion_count(record)
        wall = record.get("wall_seconds")
        # Recomputed from the stored measurements rather than read from the
        # stored `run_verdict`, for the reason in `dispersion_count`: the bands
        # are explicitly provisional, and a stored scalar cannot be re-judged
        # when they move. Records written before the axes existed have no
        # stored verdict at all and would otherwise print "?" forever.
        #
        # The window is *causal* -- only records preceding this one -- for the
        # same reason `report_run_position`'s is: a verdict printed at boot
        # must still read the same a week later, rather than being rewritten by
        # runs that had not happened yet.
        prior = comparable_records(records[:index], record.get("host"),
                                   record_profile(record))
        prior_stalls = [c for c in (dispersion_count(r) for r in prior)
                        if c is not None]
        prior_walls = [w for r in prior
                       if isinstance(w := r.get("wall_seconds"), (int, float))]
        run_v, _ = run_verdict(verdict, stalls, prior_stalls, wall, prior_walls)
        print(
            f"{record.get('timestamp', '?')}  {record.get('host', '?'):<20} "
            f"{record_profile(record):<8} {record.get('commit', '?'):<12} "
            f"{len(entries):>3} benchmarks, {over} over hardware target, "
            f"canary {verdict}, stalls {'?' if stalls is None else stalls}, "
            f"wall {'?' if wall is None else f'{wall:g}s'}, "
            f"load {record_host_load(record)}, run {run_v}"
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
    parser.add_argument("--kernel-elf", default=None,
                        help="kernel ELF to read hot-function addresses from, "
                             "recorded as `hot_symbols` so a placement-caused "
                             "swing can be recognised without a bisect")
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
    parser.add_argument("--wall-seconds", type=float, default=None,
                        help="host wall-clock seconds the guest ran for. The "
                             "single most sensitive contamination signal "
                             "available -- TCG is CPU-bound, so host steal "
                             "shows up here at full size and nowhere else.")
    parser.add_argument("--host-load", default=HOST_LOAD_UNKNOWN,
                        choices=HOST_LOAD_CHOICES,
                        help="what the host was doing during the run "
                             "(default: unknown). 'idle' is an assertion by "
                             "whoever ran it, not a measurement. 'loaded' "
                             "marks a deliberately-poisoned control run, which "
                             "is then excluded from every baseline.")
    parser.add_argument("--commit", default="",
                        help="commit the measured kernel was built from. Pass "
                             "the value read BEFORE the build: this tool runs "
                             "after a run long enough for HEAD to have moved "
                             "(default: ask git now).")
    parser.add_argument("--dirty", action="store_true",
                        help="the tree had uncommitted changes at build time, "
                             "so --commit names an ancestor of what ran.")
    parser.add_argument("--experiment", default="",
                        help="why this run was a deliberate probe (a QEMU flag "
                             "under test, a hand-toggled compiler feature, a "
                             "bisect step). Recorded in full, but never used "
                             "as a baseline for a later run: the binary is one "
                             "no checkout reproduces.")
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
    # Read once and used twice -- passed to the report so the replication gate
    # can find this binary's other runs, and stored in the record below so the
    # *next* run can find this one. Two `git_commit()` calls could disagree if
    # HEAD moved mid-run, and a record filed under a different commit than the
    # one it was judged as would corrupt every later replication verdict.
    #
    # Reading it once is necessary but not sufficient: this runs at the *end* of
    # a boot that took ten to twenty minutes, so a single reading here can still
    # name a commit made while QEMU was running rather than the one that was
    # built. `--commit` is boot-test.sh's answer -- it reads HEAD before the
    # build and hands that value down -- and it wins whenever it is given. The
    # git fallback keeps a standalone invocation working.
    commit = args.commit or git_commit()
    regressed = report(previous, current_entries, args.threshold,
                       records=records, host=host, profile=args.profile,
                       commit=commit)

    # Reported *after* the comparison, so it qualifies the verdict the reader
    # has just seen rather than being buried above it. The verdict is *taken
    # from* the printer rather than recomputed here: the two must agree, and a
    # second `canary_verdict(canary)` call is a place where they could silently
    # stop agreeing.
    verdict = print_canary_summary(canary)

    report_dispersion(current_entries)

    # The combined run-level verdict, last, because it is the one the reader is
    # meant to act on and it depends on all three axes above. Its history comes
    # from the same window every other historical judgement here uses, so a
    # deliberately-loaded control can never become part of the band that
    # decides whether an honest run was quiet.
    window = comparable_records(records, host, args.profile)
    dispersions = [c for c in (dispersion_count(r) for r in window)
                   if c is not None]
    walls = [w for r in window
             if isinstance(w := r.get("wall_seconds"), (int, float))]
    here_dispersion = len(suspect_dispersion(current_entries))
    run_v, run_notes = run_verdict(verdict, here_dispersion, dispersions,
                                   args.wall_seconds, walls)
    extra = []
    if args.host_load != HOST_LOAD_UNKNOWN:
        extra.append(f"host load: recorded as '{args.host_load}' by the "
                     f"caller -- an assertion, not a measurement, so it does "
                     f"not move the verdict either way")
    if args.experiment:
        # Said out loud because the exclusion is otherwise invisible: the run
        # prints a full comparison and is then never referred to again, and a
        # caller who passed the flag by habit would have no way to notice.
        extra.append(f"experiment: {args.experiment} -- recorded, but excluded "
                     f"from every future baseline, because the binary is one "
                     f"no checkout reproduces")
    report_run_verdict(run_v, run_notes, extra)

    if not args.no_record:
        record = {
            "timestamp": datetime.datetime.now(
                datetime.timezone.utc
            ).replace(microsecond=0).isoformat(),
            "host": host,
            # Sibling key, absent on pre-2026-08-14 records, which
            # record_profile() reads as "debug". See LEGACY_PROFILE.
            "profile": args.profile,
            "commit": commit,
            # True when the tree carried uncommitted changes at build time, so
            # `commit` names the nearest ancestor rather than what was measured.
            # Stored so a later comparison can qualify itself instead of
            # treating the hash as if it identified the code.
            "dirty": bool(args.dirty),
            # The target is static and already lives in baselines.toml, so
            # only the measured number goes here.
            "entries": {n: v[0] for n, v in current_entries.items()},
            "over_target": sum(
                1 for v in current_entries.values() if v[2] == "OVER"
            ),
            # The run-level verdict, which is NOT the canary verdict stored
            # below: it is the worst of the canary, dispersion and wall-clock
            # axes, and it is `unknown` unless every axis actively said clean.
            # Both are stored because they answer different questions and
            # because the older records carry only the canary one -- a reader
            # that finds `run_verdict` absent is looking at a record written
            # before any of this existed and must not read its `canary_verdict:
            # clean` as a certificate.
            "run_verdict": run_v,
            # Recorded even when the caller said nothing, so that "nobody
            # stated it" is a fact in the record rather than an absence that a
            # later reader might charitably fill in.
            "host_load": args.host_load,
            "dispersion": here_dispersion,
        }
        # Addresses of the functions known to change cost by several-fold when
        # only their placement changes.
        #
        # Absent means "no ELF was offered"; `{}` means "an ELF was offered and
        # yielded nothing" (stripped, or the functions were inlined away). The
        # two are kept distinct on purpose: collapsing them would let a reader
        # who finds no addresses beside a 4x swing conclude the addresses did
        # not move, when in fact nobody looked -- which is the same mistake,
        # one level up, that this field exists to prevent.
        if args.kernel_elf:
            record["hot_symbols"] = elf_symbol_addresses(args.kernel_elf)
        # Only present on probe runs, so that the overwhelming majority of
        # records -- ordinary ones -- carry no field asserting they are
        # ordinary. An empty string here would be a claim; absence is the
        # default.
        if args.experiment:
            record["experiment"] = args.experiment
        # Absent rather than null when the caller did not measure it: an
        # explicit `wall_seconds: null` invites a reader to treat it as zero,
        # and `dispersion_count`-style "absent means unknown" handling is
        # already the convention every other optional key here follows.
        if args.wall_seconds is not None:
            record["wall_seconds"] = args.wall_seconds
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
        # The split-sample tokens, same sibling-map convention. Stored raw
        # rather than pre-reduced to a boolean for the reason the canary block
        # below gives about storing the measurement and not only the verdict: if
        # the kernel's gate is ever retuned, a stored `31!` can be re-judged
        # against the new threshold and a stored `True` cannot.
        splits = {
            n: v[5]
            for n, v in current_entries.items()
            if len(v) > 5 and v[5] is not SPLIT_ABSENT
        }
        if splits:
            record["split"] = splits
        # Same append-only reasoning: a sibling key, absent on older records.
        # Recorded even when clean, because a stored verdict with no stored
        # measurement could never be re-judged if the tolerance is retuned --
        # and the tolerance is explicitly a placeholder awaiting real data.
        if canary is not None:
            record["canary"] = dict(canary)
            # Both, and they are not redundant: the boolean is False for a
            # *broken* canary, so `canary_verdict` is the one to test when what
            # you mean is "may I trust this run?". Storing only the boolean is
            # how nine release records ended up flagged as contaminated when the
            # truth was that the instrument had died.
            #
            # Named `canary_contaminated`, not `contaminated`, as of 2026-08-16.
            # The old name claimed the whole run and delivered only the canary's
            # opinion of it, and the 02:53 record made that concrete: it holds
            # `contaminated: false` beside `run_verdict: "contaminated"`, because
            # the canary was clean while the dispersion instrument counted 25
            # stalled benchmarks. A reader scanning that record for the honest
            # field finds a flat contradiction and no way to tell which half to
            # believe.
            #
            # Renaming rather than deleting because the value is genuinely worth
            # keeping -- it is the only stored trace of *which* instrument
            # objected -- and renaming rather than keeping because nothing reads
            # the key. It is written here and read nowhere: the analysis path
            # deliberately re-derives the verdict from the stored `canary`
            # measurement instead (see `dispersion_count`), so no consumer
            # breaks. Records written before this date carry the old key; that
            # is ordinary append-only schema drift, already the norm here for
            # `split` and `run_verdict`.
            record["canary_contaminated"] = canary_is_contaminated(canary)
            record["canary_verdict"] = verdict
        if append_record(args.history, record):
            print(f"  Recorded {len(current_entries)} benchmarks to "
                  f"{display_path(args.history)}")

    if regressed and args.fail_on_regression:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
