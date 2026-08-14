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


def test_dispersion(bh, tmpdir):
    """A benchmark's own mean/min catches stalls the canary certifies as clean.

    The canary samples the host *between* benchmarks, ~1 sample per 8, so a
    stall confined to one benchmark falls between samples. Measured over the
    three records carrying mean data: the canary reported all three clean, and
    every one contained 5-8 benchmarks at >=5x mean/min. This check is what
    sees them.
    """
    # parse_serial's value tuple is (measured, target, verdict, mean_ns, iters).
    entries = {
        "quiet":      (100, 10, "OVER", 120, 2000),   # 1.2x - normal
        "borderline": (100, 10, "OVER", 500, 2000),   # exactly 5x - flagged
        "stalled":    (100, 10, "OVER", 2400, 2000),  # 24x
        "worse":      (100, 10, "OVER", 5000, 2000),  # 50x
        "no_mean":    (100, 10, "OVER", None, None),  # legacy log
    }
    suspect = bh.suspect_dispersion(entries)
    names = [n for _, n in suspect]

    check("a quiet benchmark is not flagged", "quiet" in names, False)
    check("exactly at the ratio is flagged", "borderline" in names, True)
    check("a stalled benchmark is flagged", "stalled" in names, True)
    check("worst-first ordering", names[0], "worse")
    check("three of five are flagged", len(suspect), 3)

    # An absent mean is not evidence of a quiet run -- same distinction the
    # canary makes between "absent" and "clean". It must not be flagged (we
    # have no measurement) nor silently counted as fine.
    check("a benchmark with no recorded mean is skipped",
          "no_mean" in names, False)

    # The real regression this guards: the run that the canary called clean
    # with spread 2% did contain page_alloc_free at 24x. Feed that shape in.
    real = {"page_alloc_free": (298, 1000, "PASS", 7280, 500)}
    check("the run the canary called clean is flagged here",
          len(bh.suspect_dispersion(real)), 1)

    # Nothing to report is reported as such, not as silence.
    check("an all-quiet suite yields an empty list",
          bh.suspect_dispersion({"a": (100, 10, "PASS", 110, 500)}), [])


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


def _run_position(bh, records, current, previous, host="h", profile="debug"):
    """Capture what `report_run_position` prints, as a single string."""
    import io
    import contextlib
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report_run_position(records, host, profile, current, previous)
    return buf.getvalue()


def _history(bh, host, profile, *factors):
    """Synthetic same-host history: one record per whole-suite speed factor.

    Every benchmark in a record is scaled by the same factor, which is exactly
    the shape of host-side drift: TCG is CPU-bound, so a loaded machine moves
    the entire suite together.
    """
    base = {f"b{i}": 1000 + 10 * i for i in range(20)}
    return [
        {"host": host, "profile": profile,
         "entries": {name: int(v * f) for name, v in base.items()}}
        for f in factors
    ]


def test_run_position_flags_outlier_run(bh):
    """A run that is uniformly fast must be labelled before it is quoted.

    This is the regression test for the mistake that motivated the function:
    six boots scored x1.040, x1.026, x0.990, *x0.775*, x1.000, x1.000 against
    their own median, and two benchmarks were written up as regressions purely
    for having returned to normal afterwards. The correction was never wrong;
    nothing was *saying* that one of the two runs compared was 23% off.
    """
    records = _history(bh, "h", "debug", 1.0, 1.0, 1.0, 1.0)
    typical = {name: v for name, v in records[-1]["entries"].items()}
    fast = {name: int(v * 0.775) for name, v in typical.items()}

    out = _run_position(bh, records, fast, records[-1])
    check("uniformly fast run is called an outlier", "OUTLIER RUN" in out, True)
    check("outlier run reports the direction", "faster" in out, True)
    check("outlier run is refused as a baseline",
          "do not use this run as a baseline" in out, True)
    check("outlier run reports its factor", "x0.77" in out, True)
    # The one number that matters must survive the terminal. This output is
    # read on a cp1252 Windows console, where a U+00D7 multiplication sign
    # arrives as a replacement character: "?0.041". Assert ASCII rather than
    # normalising it away, or the test passes on output the reader cannot read.
    check("the verdict is ASCII-only", out.isascii(), True)

    out = _run_position(bh, records, typical, records[-1])
    check("a typical run raises no warning", "!!" in out, False)
    check("a typical run still states its position", "x1.000" in out, True)


def test_run_position_flags_outlier_baseline(bh):
    """An anomalous *baseline* must be called out too.

    The reader is shown `before -> after` values in raw nanoseconds. If the
    `before` came from a boot that ran 23% fast, those nanoseconds describe no
    machine that exists, even though the percentages beside them are correct.
    Labelling only the current run would catch this one boot and miss it on
    every subsequent run that diffs against it.
    """
    records = _history(bh, "h", "debug", 1.0, 1.0, 1.0, 0.775)
    # records[-1] *is* the fast boot, and is what the next run diffs against.
    typical = {name: int(v / 0.775) for name, v in records[-1]["entries"].items()}
    out = _run_position(bh, records, typical, records[-1])
    check("an outlier baseline is called out",
          "baseline this run is diffed against was itself an outlier" in out,
          True)
    check("outlier baseline keeps the percentages usable",
          "percentages" in out, True)
    check("the outlier-baseline verdict is ASCII-only", out.isascii(), True)


def test_run_position_needs_history(bh):
    """Below the evidence floor, say nothing rather than something shaky."""
    single = _history(bh, "h", "debug", 1.0)
    check("one record is not a median",
          _run_position(bh, single, single[0]["entries"], None), "")

    # Enough records, but too few benchmarks in common to average over: the
    # same floor `global_drift` uses, for the same reason.
    thin = [{"host": "h", "profile": "debug", "entries": {"a": 100, "b": 200}}
            for _ in range(4)]
    check("too few comparable benchmarks yields no verdict",
          _run_position(bh, thin, {"a": 100, "b": 200}, thin[-1]), "")

    # Other hosts and other profiles are not evidence about this one.
    foreign = (_history(bh, "other", "debug", 1.0, 1.0, 1.0)
               + _history(bh, "h", "release", 1.0, 1.0, 1.0))
    check("another host's or profile's runs are not a baseline",
          _run_position(bh, foreign, foreign[0]["entries"], None), "")


def test_run_position_wired_into_report(bh):
    """The check must actually run in the real path, not merely exist.

    A diagnostic that is defined and never called is indistinguishable from
    one that always passes -- which is how the bug it detects survived in the
    first place.
    """
    import io
    import contextlib
    records = _history(bh, "h", "debug", 1.0, 1.0, 1.0, 1.0)
    previous = records[-1]
    fast = {name: (int(v * 0.775), 700, "OK", None, None)
            for name, v in previous["entries"].items()}
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(previous, fast, 25.0, records=records, host="h",
                  profile="debug")
    check("report() surfaces the outlier verdict",
          "OUTLIER RUN" in buf.getvalue(), True)

    # And without a history it must stay silent rather than fail: the
    # parameters are optional.
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(previous, fast, 25.0)
    check("report() without a history omits the verdict",
          "OUTLIER RUN" in buf.getvalue(), False)


def test_baselines_crosscheck(bh, tmpdir):
    """The target cross-check must distinguish its three failure modes.

    They are different problems and collapsing them would hide the worst one.
    A *disagreement* means one of the two files was edited without the other,
    so a benchmark is being graded against a number its own documentation
    contradicts. *Unbaselined* means the kernel's literal is the only record of
    the target. *Unused* means the file claims coverage that does not exist.

    Also pins the None case: an unparseable file must not read as agreement.
    """
    entries = {
        "agrees": (100, 500, "PASS", None, None),
        "disagrees": (100, 500, "PASS", None, None),
        "unbaselined": (100, 700, "OVER", None, None),
    }
    baselines = {"agrees": 500, "disagrees": 900, "unused": 42}

    import io
    import contextlib
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report_baselines(entries, baselines)
    out = buf.getvalue()
    check("disagreement is reported with both numbers",
          "disagrees: kernel says 500ns, file says 900ns" in out, True)
    check("unbaselined benchmark is named", "unbaselined" in out, True)
    check("unused baseline is named", "unused" in out, True)
    # Match on a whole line, not a substring: "disagrees:" contains "agrees:",
    # so a substring test passes for the wrong reason -- it was doing so until
    # this assertion was tightened.
    check("an agreeing benchmark is not reported",
          any(line.strip().startswith("agrees:") for line in out.splitlines()),
          False)

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report_baselines(entries, None)
    check("unparseable baselines read as UNVERIFIED, not as agreement",
          "UNVERIFIED" in buf.getvalue(), True)

    # And the real file must load, with the units it actually uses.
    real = bh.load_baselines()
    if real is None:
        print("SKIP  real baselines load (needs Python 3.11+)")
        return
    check("real baselines load to a non-empty target map", bool(real), True)
    check("ms targets are scaled to ns",
          real.get("compositor_frame_4k"), 2_000_000)


def test_baselines_is_valid_toml():
    """`bench/baselines.toml` must actually be TOML, with no duplicate tables.

    This test exists because the file was **not** valid TOML for months and
    nobody noticed: it carried two `[compositor_frame_4k]` tables that
    disagreed even on the unit (`target_ms = 2.0` vs `target_ns = 2000000`).
    It went undetected because every reference to the file in the tree is a
    *comment* -- `kernel/src/bench.rs` hard-codes each target as a literal and
    says "from baselines.toml" beside it. So the file looked like the
    authority while the actual authority was ~60 scattered literals, and a
    parser was never pointed at it.

    That is the project's recurring failure mode once more: a check that
    cannot fire is indistinguishable from a check that passes. Parsing the
    file here is the smallest change that makes it able to fire at all. It
    does not fix the duplication of targets between this file and `bench.rs`
    -- that is tracked separately -- but it does guarantee the file is
    machine-readable, which is the precondition for ever closing that gap.
    """
    try:
        import tomllib
    except ImportError:  # Python < 3.11
        print("SKIP  baselines.toml parse (tomllib needs Python 3.11+)")
        return
    path = os.path.join(REPO_ROOT, "bench", "baselines.toml")
    try:
        with open(path, "rb") as handle:
            data = tomllib.load(handle)
    except Exception as exc:  # noqa: BLE001 - report any parse failure
        check(f"baselines.toml parses as TOML ({exc})", False, True)
        return
    check("baselines.toml parses as TOML", True, True)
    check("baselines.toml is non-empty", bool(data), True)
    # Every table must carry a target in *some* unit, or declare that it is
    # not a target at all. A table with neither is a baseline that can never
    # be compared against -- dead weight that reads as coverage, which is the
    # same defect one level down.
    #
    # The unit is matched by prefix rather than by an enumerated list, because
    # the units here are genuinely open-ended and deliberately so: alongside
    # `target_ns`/`target_cycles` there are delta-based ones
    # (`target_accesses_over_nop`, `target_accesses_delta`) that exist because
    # TCG's harness overhead swamps the absolute number. An enumerated list
    # would have to be extended in lockstep with the file and would silently
    # under-report the day it wasn't -- the failure this test is here to stop.
    #
    # `not_a_target = true` is the declarative opt-out for calibration
    # constants and host metadata. It lives in the data rather than in a name
    # list here so that adding such a table does not require editing this test.
    #
    # An *environment qualifier* may precede the unit: `tcg_target_ns` is the
    # budget the suite is graded against under emulation, as distinct from the
    # `target_ns` hardware reference. It is the same unit with a scope prefix,
    # so the qualifier is stripped before the prefix test rather than being
    # enumerated as a separate unit. Note this must not be softened into a
    # substring match for "target": that would also match the `not_a_target`
    # opt-out key itself, so every opted-out table would count as carrying a
    # target and the test could never fail -- a check that cannot fire is
    # indistinguishable from a check that passes.
    _QUALIFIERS = ("tcg_",)

    def names_a_target(key):
        for qualifier in _QUALIFIERS:
            if key.startswith(qualifier):
                key = key[len(qualifier):]
                break
        return key.startswith("target")

    def is_namespace(table):
        # `[qemu.foo]` creates an implicit `qemu` parent whose every value is
        # a sub-table. Such a parent is a namespace, not a baseline, and has
        # no target of its own; the leaves under it are recorded *measurements*
        # (`min_ns`/`min_cycles` on this host), which is a third kind of table
        # again. Recursing into them would be wrong, not merely noisy.
        return bool(table) and all(isinstance(v, dict) for v in table.values())

    missing = sorted(
        name for name, table in data.items()
        if isinstance(table, dict)
        and not is_namespace(table)
        and not table.get("not_a_target", False)
        and not any(names_a_target(k) for k in table)
    )
    check("every baseline names a target unit or opts out", missing, [])
    # The qualifier-stripping above is only correct if it still rejects the
    # opt-out key; assert that directly rather than trusting the reading.
    check("not_a_target does not read as a target unit",
          names_a_target("not_a_target"), False)
    check("tcg_target_ns reads as a target unit",
          names_a_target("tcg_target_ns"), True)


def main():
    bh = load_module()
    with tempfile.TemporaryDirectory() as tmpdir:
        test_parse_formats(bh, tmpdir)
        test_malformed_rejected(bh, tmpdir)
        test_canary(bh, tmpdir)
        test_dispersion(bh, tmpdir)
        test_profile_isolation(bh, tmpdir)
        test_missing_log(bh, tmpdir)
    test_history_still_loads(bh)
    test_drift_is_subtracted(bh)
    test_drift_needs_samples(bh)
    test_run_position_flags_outlier_run(bh)
    test_run_position_flags_outlier_baseline(bh)
    test_run_position_needs_history(bh)
    test_run_position_wired_into_report(bh)
    test_baselines_is_valid_toml()
    with tempfile.TemporaryDirectory() as tmpdir:
        test_baselines_crosscheck(bh, tmpdir)

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print("all bench-history tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
