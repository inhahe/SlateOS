#!/usr/bin/env python3
"""Regression tests for `scripts/bench-history.py`.

Run: `python scripts/test-bench-history.py` (exit 0 = pass, 1 = fail).
No pytest dependency -- this has to be runnable from a bare checkout, and it
is small enough that a dependency would cost more than it saves.

Why this file exists
--------------------
`bench/history.jsonl` is append-only and is the project's **only** longitudinal
record of kernel performance; it cannot be regenerated, because each record is
a ~9-minute QEMU boot that happened on a particular commit. That makes the
parser and the record schema unusually unforgiving: a change that stops old
records loading, or that silently drops entries from a log, destroys data
rather than just producing a wrong number this run. The format has already had
one append-only extension (`mean_ns`/`iterations`), and will need at least one
more when the per-benchmark variance estimator lands, so the property these
tests pin down is **backward compatibility across format changes**, not the
arithmetic.

The malformed-input case matters for the same reason. A regex that is too
permissive turns a corrupted serial log into plausible-looking records that
are indistinguishable from real ones once written -- the failure mode the
project keeps rediscovering as "a check that cannot fire is indistinguishable
from a check that passes".
"""

from __future__ import annotations

import importlib.util
import os
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "bench-history.py")
HISTORY = os.path.join(REPO_ROOT, "bench", "history.jsonl")

_FAILURES = []


def load_module():
    """Import bench-history.py by path (its name is not a valid identifier)."""
    spec = importlib.util.spec_from_file_location("bench_history", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def write(tmpdir, name, text):
    path = os.path.join(tmpdir, name)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)
    return path


def test_parse_formats(bh, tmpdir):
    """Both SCORE line formats parse, including a log spanning the change."""
    # Pre-dispersion format. Records written before the kernel emitted
    # mean/iters must keep parsing forever -- see module docstring.
    old = write(tmpdir, "old.txt",
                "[bench] SCORE page_alloc_free 14052 1000 OVER\n"
                "[bench] SCORE firewall_check 228 300 PASS\n"
                "[bench] this is not a score line\n"
                "random noise\n")
    check("old-format SCORE parses with dispersion absent",
          bh.parse_serial(old),
          {"page_alloc_free": (14052, 1000, "OVER", None, None),
           "firewall_check": (228, 300, "PASS", None, None)})

    new = write(tmpdir, "new.txt",
                "[bench] SCORE page_alloc_free 14052 1000 OVER 20110 1000\n"
                "[bench] SCORE firewall_check 228 300 PASS 261 2000\n")
    check("new-format SCORE parses mean and iterations",
          bh.parse_serial(new),
          {"page_alloc_free": (14052, 1000, "OVER", 20110, 1000),
           "firewall_check": (228, 300, "PASS", 261, 2000)})

    # A single boot can straddle the change only in a replayed/concatenated
    # log, but the parser should not care which line has the extension.
    mixed = write(tmpdir, "mixed.txt",
                  "[bench] SCORE a 10 1 OVER 15 500\n"
                  "[bench] SCORE b 20 30 PASS\n")
    check("a log mixing both formats parses both",
          bh.parse_serial(mixed),
          {"a": (10, 1, "OVER", 15, 500),
           "b": (20, 30, "PASS", None, None)})


def test_malformed_rejected(bh, tmpdir):
    """Corrupt SCORE lines are dropped, never coerced into a record."""
    bad = write(tmpdir, "bad.txt",
                "[bench] SCORE a 10 1 OVER 15\n"        # half the extension
                "[bench] SCORE b 20 30 MAYBE 1 2\n"     # verdict not PASS/OVER
                "[bench] SCORE c 20 30 PASS 1 2 3\n"    # trailing junk
                "[bench] SCORE d 20 30\n"               # verdict missing
                "[bench] SCORE e x 30 PASS\n")          # non-numeric
    check("malformed SCORE lines are rejected", bh.parse_serial(bad), {})


def test_canary(bh, tmpdir):
    """The contamination canary parses, and 'absent' stays distinct from 'clean'.

    That distinction is the whole point of the canary: a run with no canary
    has *unknown* contamination, and silently treating it as clean would
    recreate the failure the canary exists to prevent -- a check that cannot
    fire being indistinguishable from a check that passes.
    """
    clean = write(tmpdir, "canary-clean.txt",
                  "[bench] SCORE a 10 1 PASS\n"
                  "[bench] CANARY 283 275 97 271 279 2 9\n")
    check("canary parses with mid-suite sampling",
          bh.parse_canary(clean),
          {"start": 283, "end": 275, "pct": 97,
           "min": 271, "max": 279, "spread": 2, "samples": 9})
    check("a stable canary is not contaminated",
          bh.canary_is_contaminated(
              {"start": 283, "end": 275, "pct": 97,
               "min": 271, "max": 279, "spread": 2, "samples": 9}), False)

    # THE case this whole mechanism exists for, and the one endpoint-only
    # sampling was blind to: the suite starts and ends quiet, so `pct` is a
    # reassuring 97, but a burst in the middle drove the reference 2.7x. Real
    # run be167dd90 looked exactly like this -- endpoints within 3% while four
    # benchmarks sat 40-160% above their established values.
    burst = {"start": 283, "end": 275, "pct": 97,
             "min": 271, "max": 740, "spread": 173, "samples": 9}
    check("a mid-suite burst is contaminated despite quiet endpoints",
          bh.canary_is_contaminated(burst), True)

    # Legacy 3-field records fall back to the endpoint comparison.
    legacy = write(tmpdir, "canary-legacy.txt", "[bench] CANARY 200 204 102\n")
    check("legacy 3-field canary still parses",
          bh.parse_canary(legacy), {"start": 200, "end": 204, "pct": 102})
    check("legacy canary uses the endpoint comparison",
          bh.canary_is_contaminated({"start": 200, "end": 204, "pct": 102}),
          False)
    # 5.1x is the real observed case: crypto_ed25519_verify, 30.7M -> 158.6M.
    check("a large sustained shift is contaminated (legacy form)",
          bh.canary_is_contaminated({"start": 200, "end": 1020, "pct": 510}),
          True)
    # Faster at the end counts too -- the host got *less* busy mid-run, which
    # contaminates just as thoroughly as getting busier.
    check("a downward shift is also contaminated",
          bh.canary_is_contaminated({"start": 200, "end": 100, "pct": 50}),
          True)
    # Exactly at the tolerance is allowed; one percent past it is not.
    check("spread at the tolerance is allowed",
          bh.canary_is_contaminated(
              {"start": 100, "end": 100, "pct": 100,
               "min": 100, "max": 125, "spread": 25, "samples": 9}), False)
    check("spread past the tolerance is contaminated",
          bh.canary_is_contaminated(
              {"start": 100, "end": 100, "pct": 100,
               "min": 100, "max": 126, "spread": 26, "samples": 9}), True)
    # A zero start means the calibration itself failed.
    check("a zero-start canary is contaminated",
          bh.canary_is_contaminated({"start": 0, "end": 200, "pct": 0}), True)

    # Absent canary: distinct from clean, and must not be reported as dirty.
    absent = write(tmpdir, "canary-absent.txt", "[bench] SCORE a 10 1 PASS\n")
    check("a log with no canary yields None", bh.parse_canary(absent), None)
    check("None is not reported as contaminated",
          bh.canary_is_contaminated(None), False)

    # Malformed canary lines must not be coerced into a record.
    bad = write(tmpdir, "canary-bad.txt",
                "[bench] CANARY 200 204\n"            # missing pct
                "[bench] CANARY x 204 102\n"          # non-numeric
                "[bench] CANARY 200 204 102 7\n"      # half the extension
                "[bench] CANARY 200 204 102 1 2 3\n"  # still half
                "[bench] CANARY 1 2 3 4 5 6 7 8\n")   # trailing junk
    check("malformed canary lines are rejected", bh.parse_canary(bad), None)

    # Last wins, matching parse_serial, so a replayed log reports its final run.
    twice = write(tmpdir, "canary-twice.txt",
                  "[bench] CANARY 200 204 102\n"
                  "[bench] CANARY 300 900 300\n")
    check("the last canary wins",
          bh.parse_canary(twice), {"start": 300, "end": 900, "pct": 300})


def test_profile_isolation(bh, tmpdir):
    """Records are only ever compared within one build profile.

    Until 2026-08-14 the bench suite was measured on an `opt-level = 0` kernel
    and scored against targets drawn from optimised implementations. The fix
    builds `--bench` as `--release`, which makes every stored number
    incomparable with every new one -- not by a percentage but by a multiple.
    So the baseline lookup must be profile-scoped, and the 5 legacy records
    (which carry no `profile` key at all) must keep working rather than being
    stranded or, worse, silently diffed against release numbers.
    """
    debug_old = {"host": "H", "commit": "aaa", "entries": {"x": 100}}
    debug_new = {"host": "H", "commit": "bbb", "profile": "debug",
                 "entries": {"x": 110}}
    release = {"host": "H", "commit": "ccc", "profile": "release",
               "entries": {"x": 12}}
    other_host = {"host": "OTHER", "commit": "ddd", "profile": "release",
                  "entries": {"x": 9}}

    check("a record with no profile key reads as debug",
          bh.record_profile(debug_old), "debug")
    check("an explicit profile key is honoured",
          bh.record_profile(release), "release")

    # The load-bearing case: a release run must NOT pick up a debug baseline,
    # even though it is the most recent record for this host.
    check("a release run finds no baseline among debug-only records",
          bh.previous_for_host([debug_old, debug_new], "H", "release"), None)
    check("a debug run does not pick up a release baseline",
          bh.previous_for_host([debug_old, release], "H", "debug"), debug_old)
    check("a release run finds the release record past a later debug one",
          bh.previous_for_host([release, debug_new], "H", "release"), release)
    check("legacy profile-less records are still matched by a debug run",
          bh.previous_for_host([debug_old], "H", "debug"), debug_old)
    check("the host filter still applies within a profile",
          bh.previous_for_host([other_host], "H", "release"), None)
    check("the newest same-profile record wins",
          bh.previous_for_host([debug_old, debug_new], "H", "debug"), debug_new)

    # The default must stay "debug" so an old caller that passes no --profile
    # keeps comparing against the legacy records rather than silently finding
    # nothing.
    check("the profile argument defaults to debug",
          bh.previous_for_host([debug_old], "H"), debug_old)


def test_missing_log(bh, tmpdir):
    """A boot without --bench emits no scorecard; that is not an error."""
    check("absent serial log yields no entries",
          bh.parse_serial(os.path.join(tmpdir, "does-not-exist.txt")), {})
    empty = write(tmpdir, "empty.txt", "")
    check("serial log with no scorecard yields no entries",
          bh.parse_serial(empty), {})


def test_history_still_loads(bh):
    """Every record already committed must still load and diff.

    This is the test that actually protects the data: it runs against the real
    `bench/history.jsonl`, not a fixture, so a schema change that orphans the
    existing records fails here rather than at the next benchmark boot.
    """
    records = bh.load_history(HISTORY)
    if not check("committed history loads (non-empty)", bool(records), True):
        return
    ok = all(isinstance(r.get("entries"), dict) and r.get("host")
             for r in records)
    check("every committed record has host + entries", ok, True)

    if len(records) < 2:
        print("SKIP  diff over committed records (need >= 2 records)")
        return
    previous, latest = records[-2], records[-1]
    current = dict(latest["entries"])
    regressed, improved, added, removed, drift = bh.diff(
        previous, current, 25.0
    )
    # Consecutive records come from the same suite, so nothing should appear
    # or vanish; if it does, either the suite changed or the schema broke.
    check("consecutive records share a benchmark set",
          (added, removed), ([], []))
    check("drift is estimated over the committed records", drift is not None,
          True)
    # Guard the invariant, not the values: entries land in exactly one bucket.
    names = [e[0] for e in regressed] + [e[0] for e in improved]
    check("no benchmark is both regressed and improved",
          len(names), len(set(names)))


def test_drift_is_subtracted(bh):
    """A uniform whole-suite slowdown must report nothing.

    This is the property the drift correction exists for: under TCG a busy
    machine scales every benchmark at once, and that is not a regression.
    """
    prev = {"entries": {f"b{i}": 1000 + i for i in range(20)}}
    # Every benchmark 40% slower -- a pure emulation/contention shift.
    current = {name: int(v * 1.4) for name, v in prev["entries"].items()}
    regressed, improved, _, _, drift = bh.diff(prev, current, 25.0)
    check("uniform 40% slowdown reports no regression", regressed, [])
    check("uniform 40% slowdown reports no improvement", improved, [])
    check("drift captures the uniform factor", round(drift, 2), 1.4)

    # One benchmark doubles on top of that shift: that *is* a regression, and
    # the correction must not swallow it.
    current["b7"] = int(prev["entries"]["b7"] * 2.8)
    regressed, _, _, _, _ = bh.diff(prev, current, 25.0)
    check("a real outlier survives drift correction",
          [e[0] for e in regressed], ["b7"])


def test_drift_needs_samples(bh):
    """With too few comparable benchmarks, fall back to raw comparison."""
    prev = {"entries": {f"b{i}": 1000 for i in range(3)}}
    current = {name: 1400 for name in prev["entries"]}
    regressed, _, _, _, drift = bh.diff(prev, current, 25.0)
    check("drift is not estimated below the sample floor", drift, None)
    check("raw change is used when drift is unavailable",
          sorted(e[0] for e in regressed), ["b0", "b1", "b2"])


def main():
    bh = load_module()
    with tempfile.TemporaryDirectory() as tmpdir:
        test_parse_formats(bh, tmpdir)
        test_malformed_rejected(bh, tmpdir)
        test_canary(bh, tmpdir)
        test_profile_isolation(bh, tmpdir)
        test_missing_log(bh, tmpdir)
    test_history_still_loads(bh)
    test_drift_is_subtracted(bh)
    test_drift_needs_samples(bh)

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print("all bench-history tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
