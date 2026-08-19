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

import contextlib
import importlib.util
import inspect
import io
import math
import os
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "bench-history.py")
HISTORY = os.path.join(REPO_ROOT, "bench", "history.jsonl")

#: `bench/history.jsonl` frozen at cdca0c86d (2026-08-15, 26 records), the
#: commit that characterised `http_build_response_1KiB`'s two modes and added
#: the controls that assert them.
#:
#: Most tests here read the *live* HISTORY on purpose, and should keep doing so:
#: they either check a property that must survive growth ("every committed
#: record still loads", "the detector stays quiet on most real runs") or they
#: SKIP by design once the runs they name age out.  Two did neither.  They
#: assert an exact verdict -- `mode-structured` -- for one named series, and
#: that is a claim about a *particular set of measurements*, so every later
#: benchmark boot appending to the file was evidence they never consented to.
#: 37 rows later the series classifies as `run-noise` and both failed, with no
#: code change: on 2026-08-18 the file at cdca0c86d still passes them and the
#: file at HEAD does not.
#:
#: A positive control has to be reproducible to be a control, so it gets the
#: data it was written against.  This is still real project data, not a
#: synthetic fixture -- it is the very history the false bisection happened on.
FROZEN_HISTORY = os.path.join(
    REPO_ROOT, "scripts", "fixtures", "bench-history-2026-08-15.jsonl")

#: The commit both records of the documented A/A pair were measured at -- two
#: runs of one binary that the harness once reported as regressions against each
#: other.  Named here rather than reached as "the last two records", which is
#: what it was when the control was written and what quietly disabled it.
AA_COMMIT = "602fc62e0"

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


def capture(func, *args, **kwargs):
    """Run `func`, returning `(stdout_text, return_value)`.

    Reporting functions here are asserted on for *what they say*, not only for
    what they return -- several of this file's regression tests exist because a
    diagnostic returned the right verdict and printed the wrong explanation
    beside it.
    """
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        result = func(*args, **kwargs)
    return buf.getvalue(), result


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
          {"page_alloc_free": (14052, 1000, "OVER", None, None, None, 0),
           "firewall_check": (228, 300, "PASS", None, None, None, 1)})

    new = write(tmpdir, "new.txt",
                "[bench] SCORE page_alloc_free 14052 1000 OVER 20110 1000\n"
                "[bench] SCORE firewall_check 228 300 PASS 261 2000\n")
    check("new-format SCORE parses mean and iterations",
          bh.parse_serial(new),
          {"page_alloc_free": (14052, 1000, "OVER", 20110, 1000, None, 0),
           "firewall_check": (228, 300, "PASS", 261, 2000, None, 1)})

    # A single boot can straddle the change only in a replayed/concatenated
    # log, but the parser should not care which line has the extension.
    mixed = write(tmpdir, "mixed.txt",
                  "[bench] SCORE a 10 1 OVER 15 500\n"
                  "[bench] SCORE b 20 30 PASS\n")
    check("a log mixing both formats parses both",
          bh.parse_serial(mixed),
          {"a": (10, 1, "OVER", 15, 500, None, 0),
           "b": (20, 30, "PASS", None, None, None, 1)})


def test_suite_position_matches_the_canary_axis(bh, tmpdir):
    """Each entry carries its 0-based SCORE-line ordinal, counted over lines.

    This is the join key between a benchmark and the canary trace, and it only
    exists while the log is being read in order -- records are written with
    `sort_keys=True`, which alphabetises `entries` and destroys suite order on
    the way to disk.

    The duplicate-name case is the one that matters. `record()` on the kernel
    side pushes a SCORECARD entry *and* ticks `CANARY_SCORED` for every scored
    benchmark, name collisions included, so the kernel's position axis counts
    lines. A parser numbering by `len(entries)` would instead count *distinct
    names* and silently shift every benchmark after a collision one place left
    of where the sample was actually taken -- an off-by-one that reads as a
    perfectly plausible integer and would misattribute a contamination spike to
    its neighbour.
    """
    log = write(tmpdir, "positions.txt",
                "[bench] SCORE first 10 20 PASS 12 500\n"
                "[bench] this is not a score line\n"
                "[bench] SCORE second 10 20 PASS 12 500\n"
                "[bench] SCORE third 10 - TRACK 12 500\n")
    got = bh.parse_serial(log)
    check("positions follow SCORE-line order",
          [got[n][6] for n in ("first", "second", "third")], [0, 1, 2])
    check("non-SCORE lines do not consume a position",
          got["second"][6], 1)
    check("a TRACK line occupies a position like any other",
          got["third"][6], 2)

    # A repeat of `first` must not renumber what follows it.
    dup = write(tmpdir, "positions-dup.txt",
                "[bench] SCORE first 10 20 PASS 12 500\n"
                "[bench] SCORE second 10 20 PASS 12 500\n"
                "[bench] SCORE first 11 20 PASS 12 500\n"
                "[bench] SCORE fourth 10 20 PASS 12 500\n")
    got = bh.parse_serial(dup)
    check("a duplicate name takes the later position, last-wins",
          (got["first"][0], got["first"][6]), (11, 2))
    check("a duplicate does not shift its successors",
          got["fourth"][6], 3)


def test_parse_split_column(bh, tmpdir):
    """The split-sample token parses in all four of its shapes.

    The three non-numeric shapes stay three distinct values rather than
    collapsing to one "no answer", for the same reason the canary keeps
    absent/broken/clean apart: a column that was never emitted, an entry the
    kernel declined to check, and a check that ran but could not resolve the
    work are three different facts, and only the middle one is the kernel's
    choice. Fold them together and "nobody looked" starts reading as "looked
    and found nothing wrong".
    """
    log = write(tmpdir, "split.txt",
                "[bench] SCORE absent 10 20 PASS 12 500\n"
                "[bench] SCORE unchecked 10 20 PASS 12 500 -\n"
                "[bench] SCORE unresolved 10 20 PASS 12 500 ?\n"
                "[bench] SCORE clean 10 20 PASS 12 500 3\n"
                "[bench] SCORE flagged 10 20 PASS 12 500 41!\n"
                "[bench] SCORE tracked 10 - TRACK 12 500 7\n")
    got = bh.parse_serial(log)
    check("split column: absent stays absent",
          got["absent"][5], bh.SPLIT_ABSENT)
    check("split column: unchecked is its own value",
          got["unchecked"][5], bh.SPLIT_UNCHECKED)
    check("split column: unresolved is its own value",
          got["unresolved"][5], bh.SPLIT_UNRESOLVED)
    check("split column: a clean percentage parses",
          got["clean"][5], "3")
    check("split column: a flagged percentage keeps its bang",
          got["flagged"][5], "41!")
    check("split column: a TRACK line carries it too",
          got["tracked"][5], "7")

    # The predicate, not just the token. `is_unstable` must be true for
    # exactly one of these: an absent or unchecked column has found nothing,
    # and must never be reported as a finding in either direction.
    check("only the flagged token is unstable",
          [bh.split_is_unstable(got[n][5])
           for n in ("absent", "unchecked", "unresolved", "clean", "flagged")],
          [False, False, False, False, True])
    check("the percentage is recoverable from either numeric shape",
          [bh.split_pct(got[n][5]) for n in ("clean", "flagged")], [3, 41])
    check("the non-numeric shapes have no percentage",
          [bh.split_pct(got[n][5])
           for n in ("absent", "unchecked", "unresolved")],
          [None, None, None])


def test_malformed_rejected(bh, tmpdir):
    """Corrupt SCORE lines are dropped, never coerced into a record."""
    bad = write(tmpdir, "bad.txt",
                "[bench] SCORE a 10 1 OVER 15\n"        # half the extension
                "[bench] SCORE b 20 30 MAYBE 1 2\n"     # verdict not PASS/OVER
                "[bench] SCORE c 20 30 PASS 1 2 3 4\n"  # trailing junk
                "[bench] SCORE f 20 30 PASS 1 2 !\n"    # bang with no number
                "[bench] SCORE g 20 30 PASS 1 2 5%\n"   # split not a bare int
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

    # Every wire arity the kernel has ever emitted must still parse. The format
    # is append-only precisely so old records stay readable, but "append-only"
    # is a claim about a regex with nested optional groups -- easy to get wrong
    # and impossible to notice, since a mis-parse yields None and None reads as
    # "this run had no canary" rather than as an error. Checked by construction
    # rather than by inspection.
    arities = {
        3: ("[bench] CANARY 271 275 101", "pct", 101),
        7: ("[bench] CANARY 271 275 101 266 309 16 9", "samples", 9),
        8: ("[bench] CANARY 271 275 101 266 309 16 9 0", "invalid", 0),
        10: ("[bench] CANARY 5 5 100 5 7 47 10 0 510 750", "max_centi", 750),
    }
    for arity, (line, key, want) in arities.items():
        got = bh.parse_canary(
            write(tmpdir, f"canary-arity{arity}.txt", line + "\n"))
        check(f"a {arity}-field CANARY record parses",
              (got or {}).get(key), want)
    # The centicycle fields must not leak into shorter records as zeros: absent
    # and zero are different, and only absence marks a spread as untrustworthy.
    check("an 8-field record has no centicycle extremes",
          "min_centi" in (bh.parse_canary(
              write(tmpdir, "canary-nocenti.txt",
                    "[bench] CANARY 271 275 101 266 309 16 9 0\n")) or {}),
          False)

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
                "[bench] CANARY 1 2 3 4 5 6 7 8 9\n")  # trailing junk
    check("malformed canary lines are rejected", bh.parse_canary(bad), None)

    # Last wins, matching parse_serial, so a replayed log reports its final run.
    twice = write(tmpdir, "canary-twice.txt",
                  "[bench] CANARY 200 204 102\n"
                  "[bench] CANARY 300 900 300\n")
    check("the last canary wins",
          bh.parse_canary(twice), {"start": 300, "end": 900, "pct": 300})


def test_canary_trace_parses_and_keeps_its_units(bh, tmpdir):
    """The per-sample trace parses, and both wire precisions land in one unit.

    The trace is the only record of *where* in the suite the reference cost
    moved. `min`/`max`/`spread` can say how far it moved and can never say
    where, yet the two causes of movement have opposite remedies: dear at the
    same positions across runs is the suite's own cache/TLB residue, dear at
    differing positions is host load. So the trace has to survive into the
    record, not just onto the console.

    The units assertion is the one with teeth. The kernel printed tenths
    before it printed hundredths, so both widths sit in the logs on disk, and
    "5.1" and "5.10" are the same measurement. A parser that read the fraction
    as a plain integer would make them 51 and 510 -- a tenfold error, in the
    quantity a positional correction would multiply by.
    """
    log = write(tmpdir, "canary-trace.txt",
                "[bench] SCORE a 10 1 PASS\n"
                "[bench] CANARY 5 5 100 5 5 0 13 0 515 517\n"
                "[bench] CANARY-TRACE 0:5.15 8:5.17 16:5.15 end:5.15\n")
    got = bh.parse_canary(log)
    check("the trace is attached to the canary",
          [s["centi"] for s in (got or {}).get("trace", [])],
          [515, 517, 515, 515])
    check("suite positions are kept as integers",
          [s["pos"] for s in (got or {}).get("trace", [])],
          [0, 8, 16, None])

    # `end:` is not at a suite position -- it brackets the suite. None rather
    # than a sentinel integer so no arithmetic can mistake it for position 0,
    # and so it survives a JSON round-trip as null. It additionally names
    # *which* side it brackets, which the tail correction needs and which the
    # sample's ordinal cannot supply.
    check("an endpoint sample has no position, and says which end it is",
          (got or {})["trace"][-1],
          {"pos": None, "centi": 515, "edge": "end"})
    check("a positioned sample carries no edge key at all",
          "edge" in (got or {})["trace"][0], False)

    # Tenths and hundredths must be the same quantity. Padding right is what
    # makes that true; padding left (or int(frac)) silently divides by ten.
    check("a tenths-era trace reads in the same units as a hundredths-era one",
          [s["centi"] for s in bh.parse_canary_trace(" 0:5.1 8:5.10 16:5")],
          [510, 510, 500])

    # The real line from build/ab-old-2.log, kept verbatim. It is a genuinely
    # contaminated run and the reason a *positional* correction is worth
    # having: a 3.2x spike confined to position 32 and a smaller one at 72,
    # against a flat ~5.1 everywhere else. Whole-suite `spread` reports one
    # number for that and cannot tell it from every benchmark being 3x slower.
    real = ("[bench] CANARY-TRACE 0:5.3 8:5.1 16:5.1 24:5.1 32:16.3 40:5.8 "
            "48:5.0 56:5.1 64:5.9 72:7.1 80:5.1 end:5.1 end:14.8")
    samples = bh.parse_canary_trace(real[len("[bench] CANARY-TRACE"):])
    check("the real 13-sample trace parses whole", len(samples), 13)
    check("the real trace preserves the isolated spike",
          max(s["centi"] for s in samples if s["pos"] == 32), 1630)
    check("and the quiet positions stay quiet",
          sorted({s["centi"] for s in samples if s["pos"] in (8, 16, 24, 80)}),
          [510])


def test_canary_trace_absence_is_not_an_error(bh, tmpdir):
    """No trace must read as "not recorded", never as an empty measurement.

    Every one of the 68 canary records already on disk predates this parser,
    so "absent" is the common case and has to stay distinguishable from "the
    trace was taken and was empty". The kernel suppresses the line entirely
    when `samples == 0`, which is exactly when a reader must not conclude the
    host was quiet.
    """
    plain = write(tmpdir, "canary-no-trace.txt",
                  "[bench] CANARY 283 275 97 271 279 2 9\n")
    check("a canary without a trace has no trace key",
          "trace" in (bh.parse_canary(plain) or {}), False)

    # A trace with no samples must not create the key either -- an empty list
    # would read as a completed measurement that found nothing.
    check("an empty trace line does not create the key",
          "trace" in (bh.parse_canary(write(
              tmpdir, "canary-empty-trace.txt",
              "[bench] CANARY 283 275 97 271 279 2 9\n"
              "[bench] CANARY-TRACE\n")) or {}), False)

    # A trace belongs to the CANARY line it follows. Two suites in one log,
    # the second emitting no trace: the first suite's trace must not be
    # stapled onto the second suite's record, which is what tracking the
    # "last trace seen" independently of the last canary would do.
    twice = write(tmpdir, "canary-trace-twice.txt",
                  "[bench] CANARY 100 100 100 100 100 0 9\n"
                  "[bench] CANARY-TRACE 0:9.99 8:9.99\n"
                  "[bench] CANARY 300 900 300\n")
    got = bh.parse_canary(twice)
    check("the last canary still wins", (got or {}).get("start"), 300)
    check("and it does not inherit the earlier suite's trace",
          "trace" in (got or {}), False)

    # A trace arriving before any canary has nothing to attach to and must be
    # dropped rather than crashing or inventing a record.
    check("a trace with no canary at all yields no canary",
          bh.parse_canary(write(tmpdir, "canary-orphan-trace.txt",
                                "[bench] CANARY-TRACE 0:5.15\n")), None)


def test_canary_arms_attach_to_the_trace_they_belong_to(bh, tmpdir):
    """The two raw arms must reach the record, alongside the value they made.

    The traced figure is `(store - nop) * 100 / n` -- a *difference*, so a move
    in it is ambiguous between the two arms, and the two mean opposite things. A
    dearer store arm is the host getting slower at the thing being measured; a
    cheaper nop arm is the measurement's own baseline shifting, which is an
    instrument artefact and no evidence about the host at all. Every trace
    recorded before this line existed computed both arms and threw them away,
    which is why the ~5.79-centicycle cluster cannot be attributed today.

    So the arms have to survive into the record next to the derived value, and
    -- the part with teeth -- next to the *right* value.
    """
    log = write(tmpdir, "canary-arms.txt",
                "[bench] SCORE a 10 1 PASS\n"
                "[bench] CANARY 5 5 100 5 5 0 13 0 515 517\n"
                "[bench] CANARY-TRACE 0:5.15 8:5.17 end:5.15\n"
                "[bench] CANARY-ARMS 0:1000:1515 8:1000:1517 end:1000:1515\n")
    got = bh.parse_canary(log)
    check("each traced sample gains its two arms",
          [(s["nop"], s["store"]) for s in (got or {}).get("trace", [])],
          [(1000, 1515), (1000, 1517), (1000, 1515)])
    check("and keeps the derived value it already had",
          [s["centi"] for s in (got or {}).get("trace", [])],
          [515, 517, 515])
    check("the endpoint label survives the merge",
          (got or {})["trace"][-1]["edge"], "end")

    # The arms are raw totals, not a rescaled value: `n` is not on the line and
    # reconstructing it from the quotient would round-trip through the very
    # division whose ambiguity the arms exist to resolve.
    check("arms are parsed as raw integers, undivided",
          bh.parse_canary_arms(" 0:123456789:987654321"),
          [{"pos": 0, "nop": 123456789, "store": 987654321}])


def test_canary_arms_never_merge_onto_the_wrong_sample(bh, tmpdir):
    """A misaligned arm is worse than a missing one, so a disagreement drops all.

    A missing arm is a gap: the consumer sees no `nop` key and knows to say
    nothing. A *misplaced* arm is evidence pointing confidently at the wrong
    slot, and nothing downstream can detect it -- it would produce exactly the
    wrong attribution the arms were added to prevent. The trace is the older,
    independently useful record, so it survives intact and the arms are what
    get dropped.
    """
    base = ("[bench] CANARY 5 5 100 5 5 0 13 0 515 517\n"
            "[bench] CANARY-TRACE 0:5.15 8:5.17 end:5.15\n")

    short = bh.parse_canary(write(tmpdir, "canary-arms-short.txt", base +
                                  "[bench] CANARY-ARMS 0:1000:1515\n"))
    check("a truncated arms line merges nothing",
          any("nop" in s for s in (short or {})["trace"]), False)
    check("and leaves the trace entirely intact",
          [s["centi"] for s in (short or {})["trace"]], [515, 517, 515])

    # Same length, mismatched labels: position 24 was never traced. Merging by
    # ordinal alone would file 24's arms under 8.
    skewed = bh.parse_canary(write(
        tmpdir, "canary-arms-skewed.txt", base +
        "[bench] CANARY-ARMS 0:1000:1515 24:1000:1517 end:1000:1515\n"))
    check("equal length is not enough -- the labels must agree too",
          any("nop" in s for s in (skewed or {})["trace"]), False)

    # An endpoint against a positioned sample is the same failure wearing a
    # different label, and is the one an ordinal-only merge is likeliest to hit:
    # a failed calibration records one endpoint instead of two.
    edge_skew = bh.parse_canary(write(
        tmpdir, "canary-arms-edge.txt", base +
        "[bench] CANARY-ARMS 0:1000:1515 end:1000:1517 8:1000:1515\n"))
    check("an endpoint may not merge onto a positioned sample",
          any("nop" in s for s in (edge_skew or {})["trace"]), False)


def test_canary_arms_absence_is_not_an_error(bh, tmpdir):
    """Every canary record on disk predates the arms, so absence is the norm.

    A reader must be able to tell "the arms were not recorded" from "the arms
    were recorded and were equal". The first is 79 existing records; the second
    would be a finding.
    """
    plain = write(tmpdir, "canary-arms-absent.txt",
                  "[bench] CANARY 5 5 100 5 5 0 13 0 515 517\n"
                  "[bench] CANARY-TRACE 0:5.15 end:5.15\n")
    got = bh.parse_canary(plain)
    check("a trace without arms keeps every sample",
          len((got or {})["trace"]), 2)
    check("and no sample claims an arm it does not have",
          any("nop" in s or "store" in s for s in (got or {})["trace"]), False)

    # Arms with no trace to attach to are dropped rather than stood up as a
    # second, parallel sample list -- that would hand every consumer a second
    # shape to handle for a case that does not occur in a whole log.
    orphan = bh.parse_canary(write(
        tmpdir, "canary-arms-orphan.txt",
        "[bench] CANARY 5 5 100 5 5 0 13 0 515 517\n"
        "[bench] CANARY-ARMS 0:1000:1515\n"))
    check("arms without a trace create no trace key",
          "trace" in (orphan or {}), False)
    check("but the canary itself still parses",
          (orphan or {}).get("pct"), 100)

    # And arms before any canary have nothing to attach to at all.
    check("arms with no canary at all yield no canary",
          bh.parse_canary(write(tmpdir, "canary-arms-nocanary.txt",
                                "[bench] CANARY-ARMS 0:1000:1515\n")), None)


def test_arm_classification_separates_artefact_from_host(bh, tmpdir):
    """The arms must sort excursions the derived value cannot tell apart.

    Every value below is a real recorded arm pair from bench/history.jsonl on
    2026-08-19, not a constructed one, because the whole claim is about what
    actually occurs on this host. The reference is the modal quiet pair
    nop=14156 store=19444.
    """
    ref = (14156, 19444)
    c = lambda nop, store: bh.classify_arm_sample(
        {"nop": nop, "store": store}, *ref)

    check("the modal pair is quiet", c(14156, 19444), bh.ARM_QUIET)
    check("and so is a pair a few cycles off it", c(14160, 19460), bh.ARM_QUIET)

    # centi=579. store is bit-identical to the mode; nop alone fell 4.6%.
    # No host state slows or speeds the nop loop without touching the store
    # loop interleaved with it, so this is one lucky round in one arm.
    check("a nop-only drop is an instrument artefact",
          c(13506, 19444), bh.ARM_ARTEFACT)
    # centi=477, the mirror case: a lucky store round, which *deflates* the
    # reported figure and so masks contamination rather than inventing it.
    check("a store-only drop is the same artefact, masking instead of alarming",
          c(14158, 19044), bh.ARM_ARTEFACT)

    # centi=504/506. Both arms fall by the same proportion: the measurement
    # scaled uniformly, which moves the difference without anything being wrong.
    check("both arms falling together is a uniform scaling",
          c(13836, 19004), bh.ARM_SCALED)
    check("...and it is still a scaling a few cycles either way",
          c(13830, 19012), bh.ARM_SCALED)

    # centi=641 and 766. Both arms up 34-45%: to lift a *minimum*, essentially
    # every round of both arms must have been slower, which is a real host
    # state and not something one lucky round can fake.
    check("both arms rising together is a real host disturbance",
          c(19466, 26040), bh.ARM_DISTURBED)
    check("...at either observed magnitude", c(20320, 28170), bh.ARM_DISTURBED)

    # A sample with no arms is not classifiable, and must say so rather than
    # being lumped in with "quiet" -- 79 records on disk have no arms, and
    # reading them as quiet would be a fabricated clean bill of health.
    check("a sample with no arms classifies as None",
          bh.classify_arm_sample({"centi": 516}, *ref), None)
    check("...and so does one measured against no reference",
          bh.classify_arm_sample({"nop": 1, "store": 2}, 0, 0), None)


def test_arm_classification_uses_the_run_as_its_own_baseline(bh, tmpdir):
    """The reference is per-run and is the median, not a stored constant.

    The absolute arm totals are a property of this host and this build, so a
    baked-in constant would go wrong the first time either changed. And it must
    be the median rather than the mean: the mean is dragged by the very
    excursions being classified, which is how a single +44% sample can make its
    thirteen quiet neighbours look displaced.
    """
    # A whole run scaled to different absolute totals -- a faster host, or a
    # different build -- must still read as quiet throughout.
    scaled_run = [{"pos": i, "centi": 516, "nop": 7078, "store": 9722}
                  for i in range(13)]
    check("a run at wholly different absolute totals is still quiet",
          set(bh.classify_canary_trace(scaled_run)), {bh.ARM_QUIET})

    # One big excursion among twelve quiet samples. Against the mean the
    # excursion would drag the reference up and mislabel the quiet ones.
    trace = [{"pos": i, "centi": 516, "nop": 14156, "store": 19444}
             for i in range(12)]
    trace.append({"pos": 96, "centi": 766, "nop": 20320, "store": 28170})
    got = bh.classify_canary_trace(trace)
    check("the lone excursion is the only sample labelled disturbed",
          got.count(bh.ARM_DISTURBED), 1)
    check("...and it is the last one", got[-1], bh.ARM_DISTURBED)
    check("...leaving every other sample quiet",
          set(got[:-1]), {bh.ARM_QUIET})

    # An unarmed trace yields a same-length list of None, so callers can zip it
    # against the trace without a length check.
    bare = [{"pos": 0, "centi": 516}, {"pos": 8, "centi": 517}]
    check("an unarmed trace classifies as all-None, same length",
          bh.classify_canary_trace(bare), [None, None])

    # Mixed: a trace where only some samples carry arms still classifies the
    # armed ones and reports None for the rest.
    mixed = [{"pos": 0, "centi": 516, "nop": 14156, "store": 19444},
             {"pos": 8, "centi": 516}]
    check("an unarmed sample among armed ones is None, not quiet",
          bh.classify_canary_trace(mixed), [bh.ARM_QUIET, None])


def _trace(*pairs):
    """Build a trace list from `(pos_or_edge, centi)` pairs."""
    out = []
    for pos, centi in pairs:
        if isinstance(pos, str):
            out.append({"pos": None, "centi": centi, "edge": pos})
        else:
            out.append({"pos": pos, "centi": centi})
    return out


def test_positional_model_separates_local_from_uniform(bh, tmpdir):
    """The positional factor must be blind to a uniform slowdown and only that.

    This is the property that makes the model complementary to `global_drift`
    rather than a second, disagreeing estimate of the same thing. If the host
    was uniformly dearer, `global_drift` is the right correction and this one
    must contribute nothing; if a burst hit one stretch of the suite,
    `global_drift` is the *wrong* correction -- it subtracts a median that the
    burst barely moved -- and this one has to carry it.
    """
    flat = _trace((0, 500), (8, 500), (16, 500), (24, 500))
    doubled = _trace((0, 1000), (8, 1000), (16, 1000), (24, 1000))
    positions = {"a": 0, "b": 8, "c": 16, "d": 24}

    check("a flat trace corrects nothing",
          bh.positional_factors({"trace": flat}, positions),
          {"a": 1.0, "b": 1.0, "c": 1.0, "d": 1.0})
    check("a uniformly dear run also corrects nothing -- that is global_drift's",
          bh.positional_factors({"trace": doubled}, positions),
          {"a": 1.0, "b": 1.0, "c": 1.0, "d": 1.0})

    # One sample 3x dear. The median is unmoved, so the burst shows up whole.
    burst = _trace((0, 500), (8, 500), (16, 1500), (24, 500))
    factors = bh.positional_factors({"trace": burst}, positions)
    check("a burst is attributed to the position it landed on",
          round(factors["c"], 3), 3.0)
    check("and the benchmarks either side of it are untouched",
          [factors["b"], factors["d"]], [1.0, 1.0])


def test_positional_interpolation_and_its_refusals(bh, tmpdir):
    """Between samples the model interpolates; outside them it declines to guess."""
    trace = _trace((0, 400), (10, 400), (20, 800))

    check("a benchmark sitting on a sample gets that sample's own value",
          bh.interpolate_trace(trace, 10), 400.0)
    check("one halfway between two samples gets the midpoint",
          bh.interpolate_trace(trace, 15), 600.0)
    check("and a quarter of the way gets a quarter of the rise",
          bh.interpolate_trace(trace, 12), 480.0)

    # Held flat outside the sampled span, not extrapolated. Extrapolation would
    # invent its largest values exactly where there is least evidence.
    check("before the first sample the model holds flat",
          bh.interpolate_trace(trace, 0), 400.0)
    check("after the last sample it holds flat too, rather than extrapolating",
          bh.interpolate_trace(trace, 60), 800.0)

    # Two samples define a line through both of themselves and can express no
    # excursion at all, so the model refuses to be built on them.
    check("two samples are not enough to build a positional model",
          bh.interpolate_trace(_trace((0, 400), (10, 800)), 5), None)
    check("...and yield no factors rather than a map of ones",
          bh.positional_factors({"trace": _trace((0, 400), (10, 800))},
                                {"a": 0, "b": 10}), {})
    check("no trace at all yields no factors",
          bh.positional_factors({}, {"a": 0}), {})
    check("no position map yields no factors",
          bh.positional_factors({"trace": trace}, {}), {})


def test_positional_tail_uses_the_end_sample_only_when_it_is_identifiable(bh, tmpdir):
    """The tail bracket is the end sample -- unless the labels cannot be trusted.

    Mid-suite sampling stops at the last multiple of `CANARY_SAMPLE_EVERY`, so
    the final few benchmarks of every suite lie past all positioned samples.
    The `end` endpoint is their only right-hand bracket.

    Both endpoints printed `end:` before the kernel gave them separate
    sentinels, and with a failed calibration only one endpoint is recorded --
    and it is the *end*, sitting in the slot a reader would take for the start.
    So neither label nor ordinal identifies an endpoint in such a log, and the
    model must fall back to holding flat rather than anchor the suite's tail to
    a measurement that may predate the suite.
    """
    body = ((0, 500), (8, 500), (16, 500))
    positions = {"a": 0, "b": 8, "c": 16, "d": 20, "e": 24}

    labelled = _trace(*body, ("start", 500), ("end", 900))
    check("the tail is bracketed by the end sample when it is named",
          bh.interpolate_trace(labelled, 20, last_pos=24), 700.0)
    check("and the benchmark at the very end reads the end sample itself",
          bh.interpolate_trace(labelled, 24, last_pos=24), 900.0)
    check("so the tail benchmarks carry a factor above 1",
          [round(bh.positional_factors({"trace": labelled}, positions)[n], 2)
           for n in ("c", "d", "e")], [1.0, 1.4, 1.8])

    # The pre-change shape: two samples both claiming to be the end.
    ambiguous = _trace(*body, ("end", 500), ("end", 900))
    check("two samples claiming 'end' means the labels are untrustworthy",
          bh.trace_edge(ambiguous, "end"), None)
    check("...so the tail holds flat instead of guessing",
          bh.interpolate_trace(ambiguous, 24, last_pos=24), 500.0)

    # A start with no end must not be pressed into service as the tail.
    check("a start sample is never used as the tail bracket",
          bh.interpolate_trace(_trace(*body, ("start", 900)), 24, last_pos=24),
          500.0)
    check("the start sample is readable in its own right",
          bh.trace_edge(_trace(*body, ("start", 900)), "start"), 900)

    # Endpoints must stay out of the baseline: the start measures the host
    # before a single benchmark has run, so it describes a moment the model
    # makes no claim about.
    check("endpoints do not enter the baseline median",
          bh.trace_reference(_trace((0, 500), (8, 500), (16, 500),
                                    ("start", 9000), ("end", 9000))), 500)


def test_positional_attribution_is_silent_rather_than_reassuring(bh, tmpdir):
    """With no model, the report says nothing -- it never prints an empty finding.

    The distinction has teeth because of when this line is read. It sits
    directly under a CONTAMINATED verdict, and a reader who has just been told
    the run is untrustworthy is looking for something to narrow it down. An
    empty "affected benchmarks:" list would answer that question with
    "apparently none", which is the same class of error as the canary that
    certified nine runs clean while measuring nothing.
    """
    positions = {"a": 0, "b": 8, "c": 16, "d": 24}
    quiet = _trace((0, 500), (8, 500), (16, 500), (24, 500))
    burst = _trace((0, 500), (8, 500), (16, 1500), (24, 500))

    out, _ = capture(bh.report_positional_attribution, {"trace": burst},
                     positions)
    check("a burst names the benchmarks in its interval", "c" in out, True)
    check("...with the factor that reached them", "3.00x" in out, True)
    check("...and the resolution caveat, every time",
          "not ones shown to be affected" in out, True)

    out, _ = capture(bh.report_positional_attribution, {"trace": quiet},
                     positions)
    check("a quiet trace says so explicitly", "no stretch of the suite" in out,
          True)

    # No trace, no model, no output. Not "0 benchmarks affected".
    out, ret = capture(bh.report_positional_attribution, {}, positions)
    check("no trace prints nothing at all", out.strip(), "")
    check("...and returns no flagged set", ret, {})
    out, _ = capture(bh.report_positional_attribution,
                     {"trace": _trace((0, 500), (8, 900))}, {"a": 0, "b": 8})
    check("too few samples prints nothing either", out.strip(), "")


def test_canary_broken_is_not_contamination(bh, tmpdir):
    """A failed measurement must not be reported as a busy host.

    This is the regression test for
    B-BENCH-CANARY-MEASURES-ZERO-IN-RELEASE-AND-BLAMES-THE-HOST. Every
    release-profile run between 2026-08-14T15:57 and 20:30 emitted
    `CANARY 0 0 0 0 0 0 10` -- the optimiser had deleted the store being timed,
    so the A/B arms could not separate -- and the tooling announced host-load
    contamination. It sent the reader after load that was never there while the
    real fault, a contamination detector that had stopped detecting, went
    unnamed for nine runs.
    """
    v = bh.canary_verdict

    # The eighth field: the kernel counting its own failed measurements.
    log = write(tmpdir, "canary-invalid.txt",
                "[bench] CANARY 271 275 101 271 279 2 9 3\n")
    check("the invalid count parses",
          bh.parse_canary(log),
          {"start": 271, "end": 275, "pct": 101, "min": 271, "max": 279,
           "spread": 2, "samples": 9, "invalid": 3})
    check("a failed measurement among quiet ones makes the canary broken",
          v(bh.parse_canary(log)), bh.CANARY_BROKEN)
    check("a broken canary is NOT reported as contamination",
          bh.canary_is_contaminated(bh.parse_canary(log)), False)

    # Zero invalid, otherwise identical: the field's presence alone must not
    # condemn a run, or the fix would simply invert the old bug.
    ok = write(tmpdir, "canary-valid.txt",
               "[bench] CANARY 271 275 101 271 279 2 9 0\n")
    check("invalid=0 reads as clean", v(bh.parse_canary(ok)), bh.CANARY_CLEAN)

    # The exact shape the nine bad records carry, which predate the field.
    dead = {"start": 0, "end": 0, "pct": 0, "min": 0, "max": 0,
            "spread": 0, "samples": 10}
    check("the historical dead canary is broken, not clean",
          v(dead), bh.CANARY_BROKEN)
    check("the historical dead canary is not blamed on the host",
          bh.canary_is_contaminated(dead), False)

    # A zero minimum with a plausible start: one sample measured nothing. The
    # spread would compute as 0% -- maximally reassuring, entirely false.
    partial = {"start": 271, "end": 275, "pct": 101, "min": 0, "max": 279,
               "spread": 0, "samples": 9}
    check("one zero sample is enough to break the canary",
          v(partial), bh.CANARY_BROKEN)

    # The collapse caught halfway: the 15:57 and 16:16 release records measured
    # 1-2 cycles per guest store and were called *contaminated* on a "spread" of
    # 100% that is one cycle of integer rounding. (This comment used to add
    # "the honest measurement of the same quantity on the same host is 266-309
    # cycles" -- a *debug*-profile figure. The release figure is ~5 cycles.)
    quantised = {"start": 2, "end": 2, "pct": 100, "min": 1, "max": 2,
                 "spread": 100, "samples": 10}
    check("a 1-2 cycle canary is broken, not contaminated",
          v(quantised), bh.CANARY_BROKEN)
    # The bound is derived from the tolerance, not chosen: below it, one cycle
    # of quantisation outweighs the tolerance the spread is judged against.
    check("the resolution bound follows from the tolerance",
          bh.CANARY_MIN_RESOLVABLE, math.ceil(100 / bh.CANARY_TOLERANCE_PCT))

    # CONTRACT CHANGED 2026-08-14. This block used to assert that a record
    # sitting exactly at the resolution bound was CLEAN, i.e. that one cycle of
    # headroom sufficed. P18 disproved that premise with direct evidence: the
    # same 5-cycle quantity read 40% on one whole-cycle run and 0% on the next,
    # and 47% once measured at 0.01-cycle resolution. One cycle of headroom
    # bounds a *single* rounding, but a spread spans two samples and so can
    # carry two -- which is 50% at the bound, twice the tolerance.
    #
    # The test was not relaxed to make the new code pass; the old assertion
    # encoded a belief that measurement then falsified.
    at_bound = {"start": 100, "end": 100, "pct": 100,
                "min": bh.CANARY_MIN_RESOLVABLE, "max": bh.CANARY_MIN_RESOLVABLE,
                "spread": 0, "samples": 9}
    check("a whole-cycle record at the bound cannot support a spread verdict",
          v(at_bound), bh.CANARY_BROKEN)
    # ...but the identical record *with* centicycle extremes can: its spread was
    # computed at 0.01-cycle resolution, so no amount of rounding produced it.
    at_bound_precise = dict(at_bound, min_centi=400, max_centi=400)
    check("centicycle extremes rescue the same record",
          v(at_bound_precise), bh.CANARY_CLEAN)
    # And a precise record genuinely over tolerance is contamination, not
    # breakage -- the two must stay distinguishable at fine resolution too.
    over_precise = {"start": 100, "end": 100, "pct": 100, "min": 5, "max": 7,
                    "spread": 47, "samples": 10, "min_centi": 510,
                    "max_centi": 750}
    check("a precise record over tolerance is contaminated, not broken",
          v(over_precise), bh.CANARY_CONTAMINATED)

    # --- verdict precedence between "failed" and "found something" ---------
    #
    # CONTRACT CHANGED 2026-08-14, from measurement. This function used to
    # return BROKEN on *any* `invalid > 0`, before it had even looked at the
    # spread. The P20 positive control (scripts/canary-load-test.sh, 6 CPU
    # spinners during the QEMU window) produced two runs whose real wire lines
    # are transcribed below: in both, one of ten measurements failed to separate
    # its arms while the other nine spread 53% and 117%. The old rule answered
    # "contamination is UNKNOWN" about a run that was demonstrably, deliberately
    # contaminated -- the instrument measured it and then declined to say so.
    #
    # Noise large enough to invert a 5-cycle A/B split *is* load, so those
    # failures corroborate the spread rather than impeaching it.
    loaded_1 = bh.parse_canary(write(tmpdir, "canary-loaded-1.txt",
                                     "[bench] CANARY 0 10 0 8 12 53 9 1 826 1269\n"))
    loaded_2 = bh.parse_canary(write(tmpdir, "canary-loaded-2.txt",
                                     "[bench] CANARY 0 6 0 5 12 117 9 1 580 1259\n"))
    check("a failed sample does not veto a measured 53% spread",
          v(loaded_1), bh.CANARY_CONTAMINATED)
    check("nor a measured 117% spread", v(loaded_2), bh.CANARY_CONTAMINATED)
    check("and that verdict is reported as contamination",
          bh.canary_is_contaminated(loaded_1), True)
    # The mirror case, which must stay BROKEN: failures alongside a *quiet*
    # spread. A failed sample is not a quiet one, so the excursion could be
    # hiding in the measurement that did not come back.
    quiet_with_failure = dict(loaded_1, spread=2, min_centi=500, max_centi=510)
    check("failures alongside a quiet spread are still UNKNOWN",
          v(quiet_with_failure), bh.CANARY_BROKEN)
    # And nothing measured at all outranks everything: there is no finding.
    check("zero valid samples is broken whatever the spread says",
          v(dict(loaded_1, samples=0)), bh.CANARY_BROKEN)

    check("a legacy zero-start canary is broken",
          v({"start": 0, "end": 200, "pct": 0}), bh.CANARY_BROKEN)
    check("an absent canary is its own verdict", v(None), bh.CANARY_ABSENT)
    check("a real burst is still contamination",
          v({"start": 283, "end": 275, "pct": 97, "min": 271, "max": 740,
             "spread": 173, "samples": 9}), bh.CANARY_CONTAMINATED)

    # And the four verdicts must be four distinct strings, or callers testing
    # equality would silently collapse two of them.
    check("the four verdicts are distinct",
          len({bh.CANARY_ABSENT, bh.CANARY_BROKEN, bh.CANARY_CONTAMINATED,
               bh.CANARY_CLEAN}), 4)


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
    # Consecutive records come from the same suite, so a benchmark that
    # *vanishes* means the suite changed or the schema broke -- and it is the
    # dangerous direction, because the loss is silent: the benchmark stops being
    # measured, its regression coverage disappears with it, and its accumulated
    # history is orphaned with nothing left to diff against.
    check("no benchmark vanished between consecutive records", removed, [])
    # An addition is the opposite: coverage went up. It is the intended outcome
    # of wiring a print-only measurement onto the scorecard, and there are ~20
    # such measurements still to wire up, so asserting `added == []` would fail
    # once per improvement. An assertion that fires on every legitimate change
    # gets deleted -- and deleting this one would take the `removed` half with
    # it, which is the half that actually protects the data. So report additions
    # and do not fail on them.
    #
    # A *rename* is the case that looks like an addition but is really a loss;
    # it is still caught, because it also empties the old name and therefore
    # shows up in `removed`.
    if added:
        print("      note: %d benchmark(s) newly recorded: %s"
              % (len(added), ", ".join(sorted(n for n, _ in added))))
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


def test_unstable_split_withdraws_a_regression(bh):
    """A movement whose own window was unstable is withdrawn, not reported.

    This is the whole point of the split-sample column: the band asks whether
    a movement is large for this benchmark and the canary asks whether the
    host was busy between benchmarks, but neither can see a noise floor that
    moved *inside* one benchmark's own measurement window. Without this, such
    a run is indistinguishable from a real regression and fails the build.

    Both halves are asserted, because either one alone would be a check that
    cannot fire: that the flagged benchmark is withdrawn, *and* that an
    unflagged one beside it still fails. A filter that swallowed everything
    would pass the first assertion on its own.
    """
    import io
    import contextlib

    previous = {
        "timestamp": "T", "commit": "c", "host": "h", "profile": "debug",
        "entries": {"steady": 100, "shaky": 100, "filler": 100},
    }

    def run(current):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            failed = bh.report(previous, current, 25.0)
        return failed, buf.getvalue()

    # `filler` holds the suite median still, so `global_drift` does not simply
    # rescale the two movements away and leave nothing to test.
    both_moved = {
        "steady": (300, 700, "OK", 320, 500, "2"),
        "shaky": (300, 700, "OK", 320, 500, "44!"),
        "filler": (100, 700, "OK", 105, 500, "1"),
    }
    failed, out = run(both_moved)
    check("an unflagged regression is still claimed", "steady" in out, True)
    check("a flagged one is withdrawn to the void list",
          "MEASUREMENT VOID" in out, True)
    check("...and the void block names it",
          out.split("MEASUREMENT VOID")[1].find("shaky") > -1, True)
    check("...and reports how far the sample sets were apart",
          "44% apart" in out, True)
    check("a run with a real regression beside it still fails", failed, True)

    # Now the *only* threshold-crossing movement is the flagged one. Nothing
    # is being claimed, so nothing may fail the build.
    only_shaky = {
        "steady": (100, 700, "OK", 105, 500, "2"),
        "shaky": (300, 700, "OK", 320, 500, "44!"),
        "filler": (100, 700, "OK", 105, 500, "1"),
    }
    failed, out = run(only_shaky)
    check("a run whose only movement is void does not fail the build",
          failed, False)
    check("...and the summary says so rather than printing an all-clear",
          "could not be judged at all" in out, True)

    # An entry from a log predating the column must behave exactly as before:
    # absent is not a licence to withdraw anything.
    legacy = {
        "steady": (300, 700, "OK", 320, 500),
        "filler": (100, 700, "OK", 105, 500),
    }
    failed, out = run(legacy)
    check("a pre-column log still reports its regression", failed, True)
    check("...and prints no void block", "MEASUREMENT VOID" in out, False)


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


def test_baseline_canary_is_reported(bh):
    """A baseline's own canary verdict must reach the reader.

    The record has always carried it; nothing on the comparison path read it,
    so a baseline measured on a loaded machine looked exactly like one measured
    on an idle machine. This asserts each of the four verdicts produces the
    right line -- including that `clean` produces *no* line, since a warning
    printed on every run is a warning nobody reads.
    """
    import io
    import contextlib

    def header(canary):
        # Stored `entries` map name -> ns (a scalar); only the *current* run's
        # entries are the wider tuple. Getting that backwards is what the first
        # draft of this test did, and `global_drift` caught it immediately.
        previous = {
            "timestamp": "T", "commit": "c", "host": "h", "profile": "debug",
            "entries": {"a": 100},
        }
        if canary is not None:
            previous["canary"] = canary
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            bh.report(previous, {"a": (100, 700, "OK", None, None)}, 25.0)
        return buf.getvalue()

    clean = {"start": 100, "end": 101, "pct": 101, "min": 100, "max": 101,
             "spread": 1, "samples": 5, "invalid": 0}
    dirty = dict(clean)
    dirty["max"], dirty["spread"] = 300, 200
    broke = dict(clean)
    broke["invalid"] = 3

    check("a clean baseline prints no warning",
          "WARNING" in header(clean), False)
    check("a contaminated baseline is called out",
          "contamination" in header(dirty), True)
    check("the contaminated line quotes the measured spread",
          "200%" in header(dirty), True)
    check("a broken baseline is UNKNOWN, not contaminated",
          "UNKNOWN" in header(broke), True)
    check("a broken baseline is not called contaminated",
          "measured host-load" in header(broke), False)
    check("a canary-less baseline says so",
          "predates" in header(None), True)


def test_canary_summary_names_both_causes(bh):
    """The current run's canary paragraph must say the right thing.

    This test could not be written before 2026-08-14: the paragraph was 55
    lines inline in `main()`, reachable only by running the whole tool against
    a real serial log. So the one piece of prose in this repo whose entire job
    is to stop a reader misattributing a benchmark result had nothing asserting
    what it said -- and it said the wrong thing for the length of this thread,
    naming the optimiser as the sole cause of an arm-separation failure. A
    check nobody can exercise is indistinguishable from a check that passes,
    which is the lesson this file keeps relearning; extracting
    `print_canary_summary` is that lesson applied to the diagnostic itself.
    """
    import io
    import contextlib

    def summary(canary):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            bh.print_canary_summary(canary)
        return buf.getvalue()

    broken = {"start": 271, "end": 275, "pct": 101, "min": 271, "max": 279,
              "spread": 2, "samples": 9, "invalid": 3}
    out = summary(broken)
    check("a broken run is UNKNOWN, not clean", "UNKNOWN" in out, True)
    check("the broken paragraph names the optimiser cause",
          "optimised away" in out, True)
    check("the broken paragraph also names the host-load cause",
          "host load exceeded" in out, True)
    check("and points at the check that tells them apart",
          "scale check" in out, True)

    # The real P20 wire record: nine valid samples at 53%, one failed arm.
    loaded = {"start": 0, "end": 10, "pct": 0, "min": 8, "max": 12,
              "spread": 53, "samples": 9, "invalid": 1,
              "min_centi": 826, "max_centi": 1269}
    out = summary(loaded)
    check("a loaded run is reported as contamination",
          "CONTAMINATED" in out, True)
    check("its failed sample is called corroboration, not doubt",
          "corroborates" in out, True)
    check("and the loaded run is not filed as instrument failure",
          "CANARY BROKEN" in out, False)

    # A clean run must not borrow the corroboration line, and must still refuse
    # to claim more than a sampled check can support.
    ok = {"start": 5, "end": 5, "pct": 99, "min": 5, "max": 5, "spread": 0,
          "samples": 10, "invalid": 0, "min_centi": 516, "max_centi": 517}
    out = summary(ok)
    check("a clean run says OK", "Canary OK" in out, True)
    check("a clean run does not claim per-benchmark quiet",
          "sampled" in out, True)
    check("a clean run mentions no corroboration",
          "corroborates" in out, False)
    check("an absent canary is not silently clean",
          "not clean" in summary(None), True)


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


def test_tracked_benchmarks_round_trip(bh, tmpdir):
    """A benchmark with no hardware target must still reach the record.

    This is the bug B-BENCH-BREAKDOWN-PHASES-ARE-NOT-RECORDED was: `score()`
    could only file a benchmark *with* a target, so the five
    `vfs_stat_breakdown_*` phases and `ipc_channel_roundtrip_64k` were printed
    as prose and dropped. Every release record in `bench/history.jsonl` carries
    zero of them.

    The fix adds a second wire form, `<measured> - TRACK <mean> <iters>`, and
    the failure mode it introduces is that the parser silently ignores a line it
    does not match -- which looks exactly like the kernel never measuring it.
    So this asserts the whole path: the line parses, the target reads as None
    rather than as a number, the measurement survives, and the graded forms are
    unaffected.
    """
    log = write(tmpdir, "tracked.txt", "\n".join([
        "[bench] SCORE graded_pass 100 500 PASS 120 2000",
        "[bench] SCORE graded_over 900 500 OVER 950 2000",
        "[bench] SCORE vfs_stat_breakdown_ns 263 - TRACK 310 500",
        "[bench] SCORE ipc_channel_roundtrip_64k 41234 - TRACK 55000 200",
    ]) + "\n")
    entries = bh.parse_serial(log)

    check("a TRACK line is not dropped by the parser",
          "vfs_stat_breakdown_ns" in entries, True)
    check("all four SCORE lines parse", len(entries), 4)
    check("a tracked target reads as None, not 0",
          entries["vfs_stat_breakdown_ns"][1], None)
    check("a tracked measurement survives",
          entries["vfs_stat_breakdown_ns"][0], 263)
    check("a tracked verdict is TRACK",
          entries["vfs_stat_breakdown_ns"][2], "TRACK")
    check("tracked dispersion survives",
          entries["ipc_channel_roundtrip_64k"][3:5], (55000, 200))
    check("a graded target still reads as a number",
          entries["graded_pass"][1], 500)

    # `over_target` is what the history record stores as the run's failure
    # count. A TRACK entry must not land in it -- it has no target to be over.
    over = sum(1 for v in entries.values() if v[2] == "OVER")
    check("tracked entries are not counted as over-target", over, 1)

    # And the cross-check must not report them as unbaselined, which is the
    # shape this fix would take if it were half-applied: the parser accepts the
    # line, then every run reports two permanently-missing baselines.
    import io
    import contextlib
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report_baselines(entries, {"graded_pass": 500, "graded_over": 500})
    out = buf.getvalue()
    check("a tracked benchmark is not reported as unbaselined",
          "no baseline for" in out, False)
    check("tracked benchmarks are counted in the summary",
          "2 tracked without a target" in out, True)


def test_main_records_end_to_end(bh, tmpdir):
    """Drive `main()` all the way to an appended record.

    # Why this test exists

    Every other test in this file calls one function. `main()` is the only code
    path that actually *writes* to `history.jsonl`, and it had no test at all --
    so the one thing this tool exists to do was the one thing nothing checked.

    That gap was not theoretical. Extracting the 55-line canary summary out of
    `main()` and into `print_canary_summary` took the `verdict = ...` binding
    with it, while `main()` went on referencing `verdict` 250 lines further down
    to write `record["canary_verdict"]`. Every `--bench` boot from that commit
    onward raised `NameError` *after* printing a complete, correct-looking
    summary, and wrote no record. It survived four commits, because the
    refactor's own evidence of safety -- an assertion count that went from 106
    to 117 -- could only ever cover the functions that had tests, and `main()`
    was not one of them. `boot-test.sh` then printed "Boot test PASSED" over the
    traceback, so nothing anywhere said the run had produced no data.

    So the assertion that matters most below is the dullest one: that `main()`
    returns at all. A `NameError` is not a wrong answer to be compared against
    an expected value; it is an exception, and the only test that catches it is
    one that runs the function.
    """
    import io
    import json
    import contextlib

    log = write(tmpdir, "serial.txt", "\n".join([
        "[bench] SCORE syscall_dispatch 120 200 PASS 130 1000",
        "[bench] SCORE vfs_stat_breakdown_ns 263 - TRACK 310 500",
        # start end pct min max spread samples invalid min_centi max_centi:
        # a quiet host, resolvable, well inside tolerance -> CLEAN.
        "[bench] CANARY 8 8 100 8 9 12 11 0 800 900",
    ]) + "\n")
    history = os.path.join(tmpdir, "history.jsonl")

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = bh.main(["--serial", log, "--history", history,
                      "--profile", "release"])
    out = buf.getvalue()

    check("main() returns from the recording path", rc, 0)
    check("main() appended a history file", os.path.exists(history), True)

    lines = [l for l in open(history, encoding="utf-8").read().splitlines()
             if l.strip()]
    check("exactly one record was appended", len(lines), 1)
    record = json.loads(lines[0])

    check("both benchmarks were recorded", record["entries"],
          {"syscall_dispatch": 120, "vfs_stat_breakdown_ns": 263})
    check("the untargeted benchmark is not counted as over target",
          record["over_target"], 0)
    check("the record is stamped with its profile", record["profile"],
          "release")
    check("the canary was stored for later re-judging",
          record["canary"]["spread"], 12)
    check("the stored verdict is the printed one",
          record["canary_verdict"], bh.CANARY_CLEAN)
    check("a clean canary is not stored as contamination",
          record["canary_contaminated"], False)
    # The old name is gone, not merely supplemented. It claimed the whole run
    # and answered only for the canary, so a record could carry
    # `contaminated: false` beside `run_verdict: "contaminated"`. Asserting its
    # absence is what stops it being reintroduced alongside the new key, which
    # would restore the contradiction while looking like a compatibility
    # courtesy.
    check("the run-wide name is not written for a canary-only judgement",
          "contaminated" in record, False)
    # The stored verdict and the printed prose come from one call now; assert
    # they agree, because storing a verdict that contradicts the summary above
    # it is worse than storing none.
    check("the summary printed the verdict that was stored",
          "Canary OK" in out, True)

    # The other return path: a log with no canary at all. `main()` must still
    # record, and must not invent a verdict for a run whose load state is
    # simply unknown.
    bare = write(tmpdir, "serial-nocanary.txt",
                 "[bench] SCORE syscall_dispatch 120 200 PASS 130 1000\n")
    history2 = os.path.join(tmpdir, "history2.jsonl")
    with contextlib.redirect_stdout(io.StringIO()):
        rc2 = bh.main(["--serial", bare, "--history", history2,
                       "--profile", "release"])
    check("main() returns when the log carries no canary", rc2, 0)
    record2 = json.loads(open(history2, encoding="utf-8").read().strip())
    check("a canary-less run stores no canary", "canary" in record2, False)
    check("...and invents no verdict for it",
          "canary_verdict" in record2, False)

    # Pin each of the printer's return paths directly, so a future edit that
    # drops one shows up here rather than 250 lines away in `main()`.
    #
    # The verdict is collected under the redirect and asserted OUTSIDE it. The
    # obvious form -- calling `check` inside the `with` block -- silently posts
    # its own PASS/FAIL lines into the discarded buffer, so four assertions run
    # and report nothing. That is this suite's own subject matter reproduced in
    # the suite: a check you cannot see is a check you cannot trust.
    def verdict_of(canary):
        with contextlib.redirect_stdout(io.StringIO()):
            return bh.print_canary_summary(canary)

    check("the absent-canary path returns its verdict",
          verdict_of(None), bh.CANARY_ABSENT)
    check("the clean path returns its verdict",
          verdict_of(bh.parse_canary(log)), bh.CANARY_CLEAN)
    check("the contaminated path returns its verdict",
          verdict_of({"start": 8, "end": 20, "pct": 250, "min": 8, "max": 30,
                      "spread": 275, "samples": 10, "invalid": 0,
                      "min_centi": 800, "max_centi": 3000}),
          bh.CANARY_CONTAMINATED)
    check("the broken path returns its verdict",
          verdict_of({"start": 0, "end": 0, "pct": 0, "min": 0, "max": 0,
                      "spread": 0, "samples": 0, "invalid": 10}),
          bh.CANARY_BROKEN)


def _record(**kw):
    """A minimal stored record, with `entries`/`mean_ns` filled to order.

    `stalls=n` builds a record whose first `n` benchmarks have a mean 10x their
    min (so they trip the dispersion ratio) and whose rest sit at 1.0x.
    """
    stalls = kw.pop("stalls", 0)
    total = kw.pop("total", 10)
    entries = {f"b{i}": 100 for i in range(total)}
    means = {f"b{i}": (1000 if i < stalls else 100) for i in range(total)}
    record = {"host": "H", "profile": "release", "entries": entries,
              "mean_ns": means}
    record.update(kw)
    return record


def test_dispersion_count_recomputes_and_admits_ignorance(bh):
    """`dispersion_count` must recompute, and must say None when it cannot.

    The None case is the one that matters. Records written before the `mean_ns`
    extension carry no dispersion data at all, and the tempting shortcut --
    treating a record with no stall data as a record with no stalls -- would
    quietly pull every one of those runs into the band as a zero and drag it
    down, making later runs look contaminated by comparison. Absent is unknown,
    not clean; this file's oldest lesson, one axis over.
    """
    check("stalls are counted from the stored measurements",
          bh.dispersion_count(_record(stalls=3)), 3)
    check("a quiet record counts zero", bh.dispersion_count(_record()), 0)
    legacy = _record()
    del legacy["mean_ns"]
    check("a pre-mean_ns record is unknown, not zero",
          bh.dispersion_count(legacy), None)
    # Recomputation, not retrieval: a stale stored count must not win.
    lying = _record(stalls=2, dispersion=99)
    check("the stored count does not override the measurements",
          bh.dispersion_count(lying), 2)


def test_loaded_control_runs_are_never_a_baseline(bh):
    """A deliberately-poisoned run must not become the thing others are judged against.

    `--host-load=loaded` exists to produce known-contaminated *controls*, which
    is the only way any of these thresholds will ever be fitted. A control that
    silently becomes the previous-run baseline is worse than having no control:
    the next honest run would then report its own recovery as a suite-wide
    improvement, and the fitting data would be gone into the bargain.
    """
    good_old = _record(commit="old")
    control = _record(commit="poisoned", host_load="loaded")
    good_new = _record(commit="new")
    records = [good_old, control, good_new]

    check("a loaded control is excluded from the comparable window",
          [r["commit"] for r in bh.comparable_records(records, "H", "release")],
          ["old", "new"])
    check("the previous-run baseline skips it",
          bh.previous_for_host(records[:2], "H", "release")["commit"], "old")
    check("an unlabelled record is still comparable",
          bh.record_host_load(good_old), bh.HOST_LOAD_UNKNOWN)
    check("a nonsense label reads as unknown rather than crashing",
          bh.record_host_load({"host_load": "quiet"}), bh.HOST_LOAD_UNKNOWN)


def test_run_verdict_is_the_worst_axis_and_clean_must_be_earned(bh):
    """The run verdict is the worst of the axes, and defaults to unknown.

    This is the repair for B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING. Before it,
    the run-level verdict *was* the canary verdict, so a clean canary certified
    the run -- and the canary is structurally incapable of seeing the host steal
    the CPU, because it counts guest cycles and the guest's counter does not
    advance while the host is elsewhere. The two properties asserted here are
    the two halves of the repair: a clean canary can no longer absolve a run on
    its own, and any single axis can still condemn one.
    """
    quiet = [4, 5, 5, 5, 6, 7, 5, 4]
    walls = [160, 162, 158, 161, 159, 163, 160, 158]

    verdict, _ = bh.run_verdict(bh.CANARY_CLEAN, 5, quiet, 160, walls)
    check("all three axes clean -> the run is clean", verdict, bh.RUN_CLEAN)

    verdict, notes = bh.run_verdict(bh.CANARY_CLEAN, 5, quiet, None, walls)
    check("a clean canary cannot certify a run whose wall time is unrecorded",
          verdict, bh.RUN_UNKNOWN)
    check("...and the reason names the axis that abstained",
          any("wall time: not recorded" in n for n in notes), True)

    # The motivating case: the canary's cleanest possible reading on a run that
    # took 2.3x as long as its twin. The wall axis must overrule it.
    verdict, _ = bh.run_verdict(bh.CANARY_CLEAN, 8, quiet, 365, walls)
    check("wall time overrules a clean canary", verdict, bh.RUN_CONTAMINATED)

    verdict, _ = bh.run_verdict(bh.CANARY_CLEAN, 40, quiet, 160, walls)
    check("dispersion overrules a clean canary", verdict, bh.RUN_CONTAMINATED)

    verdict, _ = bh.run_verdict(bh.CANARY_CONTAMINATED, 5, quiet, 160, walls)
    check("a firing canary still condemns on its own",
          verdict, bh.RUN_CONTAMINATED)

    verdict, _ = bh.run_verdict(bh.CANARY_BROKEN, 5, quiet, 160, walls)
    check("a broken canary leaves the run unproven, not clean",
          verdict, bh.RUN_UNKNOWN)


def test_bands_refuse_to_judge_on_too_little_history(bh):
    """A band computed from three numbers is an artefact; it must abstain.

    And the abstention must be `unknown`, not `clean` -- the whole point of the
    three-valued verdict. A two-valued one would have to guess, and every guess
    this project has made in that position has been "clean".
    """
    check("robust_band abstains below the window floor",
          bh.robust_band([5, 5, 6]), None)
    verdict, note = bh.dispersion_axis(9, [5, 5, 6])
    check("the dispersion axis abstains with it", verdict, bh.RUN_UNKNOWN)
    check("...and says why", "too few comparable runs" in note, True)
    verdict, _ = bh.wall_axis(365, [])
    check("the wall axis abstains with no history at all",
          verdict, bh.RUN_UNKNOWN)

    # A degenerate MAD must not collapse the band onto the median: eight runs
    # that all stalled exactly 5 benchmarks would otherwise make 6 an outlier.
    identical = [5] * 8
    check("a zero MAD does not make the next integer an outlier",
          bh.dispersion_axis(6, identical)[0], bh.RUN_CLEAN)


def test_dispersion_band_fires_on_the_real_history(bh):
    """Positive control: the band must actually fire on this project's own data.

    A threshold that never fires is indistinguishable from no threshold, which
    is the failure this entire module is a response to -- so the band is
    exercised against the release records genuinely in `bench/history.jsonl`
    rather than against numbers invented to suit it.

    Note what this test does NOT claim. On that history the band fires on the
    13-, 13- and 15-stall runs and *not* on the 8-stall run that motivated the
    axis; the entry that prescribed it expected otherwise. The band is left
    where a standard robust-outlier rule puts it rather than being lowered
    until the motivating run fires, because fitting a threshold to a single
    observation is the mistake this file has had to undo three times. The axis
    that separates that pair is wall time, not this one.
    """
    import json
    if not os.path.exists(HISTORY):
        check("history.jsonl exists for the positive control", False, True)
        return
    records = [json.loads(l) for l in
               open(HISTORY, encoding="utf-8").read().splitlines() if l.strip()]
    counts = [c for c in
              (bh.dispersion_count(r) for r in records
               if bh.record_profile(r) == "release")
              if c is not None]
    check("the real history has enough release runs to form a band",
          len(counts) >= bh.MIN_WINDOW_FOR_BAND, True)
    band = bh.robust_band(counts, mad_floor=1.0)
    fired = [c for c in counts if c > band]
    check("the band fires on at least one real run", bool(fired), True)
    check("...and not on the majority of them", len(fired) < len(counts) / 2,
          True)


def _band_history(bh, name, values, host="H", profile="release"):
    """One record per value, so `per_benchmark_bands` sees `values` for `name`."""
    return [{"host": host, "profile": profile, "entries": {name: v}}
            for v in values]


def test_per_benchmark_band_demotes_a_move_inside_its_own_spread(bh):
    """A movement that stays inside the benchmark's own range is not a finding.

    This is B-BENCH-COMPARES-TO-ONE-PRIOR-RUN-NOT-THE-DISTRIBUTION. The verdict
    used to come from `runs[-1]` alone, so for a volatile benchmark it reported
    the difference between two samples of the same noise as a change in the
    code.

    Note the asymmetry that makes this safe: the band is only ever consulted
    *after* the run-over-run threshold has already been crossed, so it can
    demote a report and can never invent one. A bug here loses sensitivity; it
    cannot manufacture a false regression.
    """
    volatile = _band_history(bh, "b", [420, 1475, 549, 654, 657, 653, 644, 542])
    bands = bh.per_benchmark_bands(volatile)
    check("a benchmark with enough history gets a band", "b" in bands, True)
    lo, hi, _median, n = bands["b"]
    check("the band is built from every sample in the window", n, 8)

    check("a value inside the range is not confirmed",
          bh.band_position(688, bands["b"], True), bh.BAND_WITHIN)
    check("a value well above it is",
          bh.band_position(2500, bands["b"], True), bh.BAND_OUTSIDE)
    check("a value well below it confirms an improvement",
          bh.band_position(50, bands["b"], False), bh.BAND_OUTSIDE)
    # Direction matters: a low value must not confirm a *regression* claim.
    check("the upper edge cannot confirm a downward move",
          bh.band_position(50, bands["b"], True), bh.BAND_WITHIN)
    check("the band brackets the observed samples", lo < 500 and hi > 700, True)

    # Too little history is `unjudged`, not `within`: a new benchmark's first
    # real regression must not be silenced by the fact that it is new.
    thin = _band_history(bh, "b", [500, 510, 505])
    check("three samples yield no band",
          bh.per_benchmark_bands(thin).get("b"), None)
    check("no band means unjudged, which still gets reported",
          bh.band_position(9999, None, True), bh.BAND_UNJUDGED)


def test_band_is_wired_into_report_and_into_the_exit_status(bh):
    """The demotion must happen on the real path and must reach `--fail-on-regression`.

    A demoted movement that still fails the build has not been demoted; the
    return value is the only thing `main()` acts on, so it is asserted here
    rather than inferred from the printed text.
    """
    import io
    import contextlib

    # Twenty benchmarks so `global_drift` has its samples; one of them, `b0`,
    # is volatile and moves 30% while staying inside its own range.
    stable = {f"b{i}": 1000 for i in range(1, 20)}
    history = []
    for value in (420, 1475, 549, 654, 657, 653, 644, 542):
        entries = dict(stable)
        entries["b0"] = value
        history.append({"host": "H", "profile": "release", "entries": entries})
    previous = history[-1]                      # b0 == 542
    current = {name: (v, 10000, "OK", None, None)
               for name, v in previous["entries"].items()}
    current["b0"] = (688, 10000, "OK", None, None)   # +27%, inside 505-746

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        regressed = bh.report(previous, current, 25.0, records=history,
                              host="H", profile="release")
    out = buf.getvalue()
    check("an in-range movement does not fail the build", regressed, False)
    check("...and is still shown, under its own heading",
          "WITHIN ITS OWN RANGE" in out, True)
    check("...with the range spelled out", "median" in out, True)
    check("...and is not called a regression", "  REGRESSED" in out, False)

    # The same machinery must still pass a real outlier through.
    current["b0"] = (3000, 10000, "OK", None, None)
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        regressed = bh.report(previous, current, 25.0, records=history,
                              host="H", profile="release")
    check("a movement outside the range still fails the build", regressed, True)
    # These records carry no `commit`, so the replication gate cannot find a
    # second run of this binary and the claim comes back UNREPLICATED rather
    # than confirmed. Asserted as the *exact* heading: "REGRESSED" alone is a
    # substring of "REGRESSED, UNREPLICATED", so the loose check this replaces
    # would have passed whichever verdict the gate produced -- a test that
    # cannot fail, in a file about checks that cannot fire.
    out = buf.getvalue()
    check("...and is called a regression", "  REGRESSED" in out, True)
    check("...but an unreplicated one, there being no commit to replicate on",
          "REGRESSED, UNREPLICATED" in out, True)
    check("...so the confirmed heading is withheld", "  REGRESSED (" in out,
          False)

    # With no history at all the movement is UNCONFIRMED rather than silently
    # confirmed -- and still fails the build, because withholding the word is
    # not the same as clearing the change.
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        regressed = bh.report(previous, current, 25.0)
    check("without a history the claim is unconfirmed, not dropped",
          regressed, True)
    check("...and says so", "UNCONFIRMED" in buf.getvalue(), True)


def test_band_declines_the_written_up_false_positive_on_the_real_history(bh):
    """Positive control: the documented false positive, replayed from real data.

    `ipc_channel` 542 -> 688 ns was written up at "+31% vs suite" while sitting
    near the middle of its own 420-1475 ns release history. The fix is only
    worth anything if it declines *that* movement using the records genuinely
    in `bench/history.jsonl` -- and only trustworthy if it still catches the
    next run's 953 ns from the same window, which it does.

    This also pins the choice of Tukey's fence over the median/MAD band used
    elsewhere in this file: on this window MAD gives 626-671 ns and would
    reproduce the false positive exactly. The band is quartile-based because
    the data demanded it, and this test is what would notice a silent revert.
    """
    import json
    if not os.path.exists(HISTORY):
        check("history.jsonl exists for the positive control", False, True)
        return
    records = [json.loads(line) for line in
               open(HISTORY, encoding="utf-8").read().splitlines()
               if line.strip()]
    release = [r for r in records if bh.record_profile(r) == "release"]
    values = [r["entries"].get("ipc_channel") for r in release]
    if 688 not in values or 953 not in values:
        print("SKIP  ipc_channel control (history no longer holds those runs)")
        return

    at = values.index(688)
    window = release[max(0, at - bh.SPEED_WINDOW):at]
    band = bh.per_benchmark_bands(window).get("ipc_channel")
    check("the real window supports a band", band is not None, True)
    check("688ns is inside ipc_channel's own range",
          bh.band_position(688, band, True), bh.BAND_WITHIN)

    at953 = values.index(953)
    window = release[max(0, at953 - bh.SPEED_WINDOW):at953]
    band = bh.per_benchmark_bands(window).get("ipc_channel")
    check("953ns from the same benchmark is outside it",
          bh.band_position(953, band, True), bh.BAND_OUTSIDE)


def _shift_history(bh, values, host="H", profile="release"):
    """One record per value for benchmark `v`, alongside stable companions.

    The companions are not padding. `level_shifts` divides every run by that
    run's own `speed_factor`, which is the *median* ratio across benchmarks --
    so in a history containing only the benchmark under test, the factor is
    that benchmark's own ratio and the correction cancels the shift exactly,
    and nothing can ever fire. Any synthetic history for this function needs a
    stable majority for the drift correction to be measured against.
    """
    records = []
    for value in values:
        entries = {f"stable{i}": 1000 for i in range(6)}
        entries["v"] = value
        records.append({"host": host, "profile": profile, "entries": entries})
    return records


def test_level_shift_catches_a_regression_the_other_checks_cannot_see(bh):
    """B-BENCH-A-PERSISTENT-REGRESSION-IS-REPORTED-ONCE-THEN-ABSORBED-INTO-ITS-OWN-RANGE.

    The hole, restated: every other check in `diff()` is anchored to the
    immediately preceding run, and the per-benchmark band is only ever a veto.
    So a regression that appears and *stays* is reported exactly once -- on the
    next run there is no run-over-run movement to report, and the trailing
    window has meanwhile swallowed the elevated samples.

    The two halves of this test are the point. A step that persists must be
    found; the same magnitude of step appearing for one run only must not be,
    because that is the single-run host excursion the existing machinery
    already grades. Persistence is the only thing separating them, so a version
    of `level_shifts` that dropped the persistence requirement would pass the
    first half and fail the second.
    """
    stable = [1000] * 6
    # Eleven prior runs: eight flat (the reference), then three elevated.
    persisted = _shift_history(bh, stable + [1000, 1000] + [3000, 3000])
    rows = bh.level_shifts(persisted, "H", "release",
                           dict(persisted[-1]["entries"], v=3000))
    names = [r[0] for r in rows]
    check("a step that persisted is reported", names, ["v"])

    # Same current value, but the run before it was back at baseline.
    blipped = _shift_history(bh, stable + [1000, 1000] + [1000, 1000])
    rows = bh.level_shifts(blipped, "H", "release",
                           dict(blipped[-1]["entries"], v=3000))
    check("a one-run excursion of the same size is not",
          [r[0] for r in rows], [])

    # And a flat history reports nothing at all.
    flat = _shift_history(bh, stable + [1000] * 4)
    check("a flat history is silent",
          bh.level_shifts(flat, "H", "release",
                          dict(flat[-1]["entries"], v=1000)), [])

    # Too little history is silence, not a finding: same rule as the bands.
    thin = _shift_history(bh, [1000, 1000, 1000])
    check("too little history cannot manufacture a shift",
          bh.level_shifts(thin, "H", "release",
                          dict(thin[-1]["entries"], v=9999)), [])

    # The percent threshold, pinned deliberately, because on the recorded
    # history it is nearly redundant -- the fence and the persistence rule do
    # almost all the work, and lowering the threshold from 25% to 1% moves the
    # real-data firing rate only from 1/26 to 3/26. So the replay control below
    # cannot notice if this knob stops being applied, and without this case
    # nothing would. A flat reference gives a zero-IQR fence, so +10% is
    # outside the fence yet under the threshold: exactly one condition
    # separates it from the firing case above.
    small = _shift_history(bh, stable + [1000, 1000] + [1100, 1100])
    current = dict(small[-1]["entries"], v=1100)
    check("a persistent move below the threshold is not reported",
          bh.level_shifts(small, "H", "release", current), [])
    check("...and the same move is reported once the threshold allows it",
          [r[0] for r in bh.level_shifts(small, "H", "release", current,
                                         threshold_pct=5.0)], ["v"])


def test_level_shift_reference_window_cannot_contain_the_shift(bh):
    """The invariant the whole mechanism rests on, asserted rather than assumed.

    The reference is `window[:-LEVEL_SHIFT_SKIP]`; the persistence evidence is
    `window[-LEVEL_SHIFT_PERSIST:]`. If those ever overlap, a run could be
    simultaneously evidence *for* a shift and part of the baseline the shift is
    measured against -- which is the self-poisoning bug this function exists to
    fix, reintroduced inside the fix. It is one comparison, and it is the kind
    of constant someone tunes later without re-deriving the consequence.
    """
    check("the persistence window is strictly inside the skipped runs",
          bh.LEVEL_SHIFT_PERSIST < bh.LEVEL_SHIFT_SKIP, True)
    check("the level-shift fence is wider than the general-purpose one",
          bh.LEVEL_SHIFT_TUKEY_K > bh.TUKEY_K, True)


def test_level_shift_replays_the_real_history_without_crying_wolf(bh):
    """Positive control *and* false-positive control, on the recorded runs.

    A detector wired into `--fail-on-regression` is only as good as its firing
    rate on real data, so this replays all recorded runs causally -- each judged
    against only the runs before it -- and pins both ends:

    * it must find `http_build_response_1KiB`, which stepped ~6000 -> 8546 ->
      12431 -> 12407 and whose confirming run printed "No benchmark moved
      outside its own recent range";
    * it must stay quiet on the great majority of runs. Measured when written:
      1 firing in 26. Earlier versions scored 11 in 26 -- a rate at which the
      report is noise and gets ignored, which is worse than not having it.

    The bound is deliberately loose (a quarter of runs) rather than pinned at
    exactly 1: this replays a *growing* real history, so a future contaminated
    run may legitimately add a firing, and a test that fails whenever a new
    benchmark run is recorded would simply be deleted. It fails on the
    regime change -- a detector that has started firing constantly.
    """
    import json
    if not os.path.exists(HISTORY):
        check("history.jsonl exists for the level-shift control", False, True)
        return
    records = [json.loads(line) for line in
               open(HISTORY, encoding="utf-8").read().splitlines()
               if line.strip()]

    fired = []
    for i, record in enumerate(records):
        rows = bh.level_shifts(records[:i], record.get("host"),
                               bh.record_profile(record),
                               record.get("entries", {}))
        if rows:
            fired.append((i, [r[0] for r in rows]))

    check("the detector stays quiet on most real runs",
          len(fired) <= max(1, len(records) // 4), True)

    names = {name for _i, found in fired for name in found}
    if any(r["entries"].get("http_build_response_1KiB") == 12407
           for r in records if "entries" in r):
        check("...and still catches the written-up 2x regression",
              "http_build_response_1KiB" in names, True)
    else:
        print("SKIP  http_build_response_1KiB control (run no longer in history)")


def test_main_records_wall_time_and_host_load(bh, tmpdir):
    """The new fields must survive the full `main()` path onto disk.

    Recording them is the entire point: the wall-clock figure was being
    computed by `boot-test.sh` for its progress message and discarded, so the
    most sensitive contamination signal in the harness left no trace, and a run
    whose cause could not be established retroactively is exactly what
    prompted this. A field that is printed but not stored cannot settle
    anything a week later.
    """
    import io
    import json
    import contextlib

    log = write(tmpdir, "serial.txt", "\n".join([
        "[bench] SCORE syscall_dispatch 120 200 PASS 130 1000",
        "[bench] CANARY 8 8 100 8 9 12 11 0 800 900",
    ]) + "\n")
    history = os.path.join(tmpdir, "history.jsonl")
    with contextlib.redirect_stdout(io.StringIO()):
        rc = bh.main(["--serial", log, "--history", history,
                      "--profile", "release", "--wall-seconds", "365",
                      "--host-load", "loaded"])
    check("main() returns with the new options", rc, 0)
    record = json.loads(open(history, encoding="utf-8").read().strip())
    check("wall time was stored", record["wall_seconds"], 365.0)
    check("the host-load label was stored", record["host_load"], "loaded")
    check("the stalled-benchmark count was stored", record["dispersion"], 0)
    check("the run verdict was stored alongside the canary's",
          record["run_verdict"], bh.RUN_UNKNOWN)
    check("...and is not the same field as the canary verdict",
          record["canary_verdict"], bh.CANARY_CLEAN)

    # Omitted, not null, when nobody timed the window -- so a reader cannot
    # mistake "not measured" for "zero seconds".
    history2 = os.path.join(tmpdir, "history2.jsonl")
    with contextlib.redirect_stdout(io.StringIO()):
        bh.main(["--serial", log, "--history", history2, "--profile", "release"])
    record2 = json.loads(open(history2, encoding="utf-8").read().strip())
    check("an untimed run stores no wall_seconds key",
          "wall_seconds" in record2, False)
    check("...and defaults its host load to unknown, not idle",
          record2["host_load"], "unknown")


def test_canary_summary_states_its_structural_blindness(bh):
    """The clean-canary prose must say the canary cannot see host descheduling.

    It already warned that the check is *sampled*, which is true and is the
    lesser limit -- and stating it alone implies the canary would catch host
    load if only it sampled more often. It would not: the quantity it measures
    does not respond to host descheduling at any sampling rate. A reader who
    acts on the weaker warning draws exactly the wrong conclusion from a clean
    line, which is what happened.
    """
    import io
    import contextlib

    clean = {"start": 8, "end": 8, "pct": 100, "min": 8, "max": 9,
             "spread": 12, "samples": 11, "invalid": 0,
             "min_centi": 800, "max_centi": 900}
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.print_canary_summary(clean)
    out = buf.getvalue()
    check("the clean line still warns that it is sampled",
          "sampled" in out, True)
    check("...and that it is blind to host descheduling entirely",
          "descheduling" in out, True)
    check("...and names the issue that proved it",
          "B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING" in out, True)


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


def _mode_records(pairs, name="b"):
    """Records carrying one benchmark, from `[(commit, value), ...]`."""
    return [
        {"host": "H", "profile": "release", "commit": commit,
         "entries": {name: value}}
        for commit, value in pairs
    ]


def test_mode_structure_separates_binaries_from_runs(bh):
    """The check must fire on a mode-structured series -- and only on one.

    Both directions are asserted here because this check replaced one that
    could not tell the two cases apart, and a detector that answers
    "mode-structured" for everything would silence `--fail-on-regression`
    entirely -- a strictly worse failure than the false bisect it fixes.
    """
    # Every repeated commit sits wholly on one side of the split, and both
    # sides are occupied: the split separates binaries.
    structured = _mode_records([
        ("aaa", 6000), ("aaa", 6200),      # always below
        ("bbb", 11000), ("bbb", 12500),    # always above
    ])
    verdict = bh.mode_structure(structured, "H", "release", "b", 7500)
    check("a split no repeat crosses is mode-structured",
          verdict.verdict, bh.MODE_STRUCTURED)
    check("...and it names both sides",
          (len(verdict.below), len(verdict.above)), (1, 1))

    # One binary spanning the split means the split is inside run noise.
    noisy = _mode_records([
        ("aaa", 6000), ("aaa", 6200),
        ("bbb", 6500), ("bbb", 12500),     # same commit, both sides
    ])
    verdict = bh.mode_structure(noisy, "H", "release", "b", 7500)
    check("a single commit spanning the split is run noise",
          verdict.verdict, bh.MODE_RUN_NOISE)

    # Guard: the "run noise" record set must really contain a straddling
    # commit, or the assertion above would pass for the wrong reason.
    check("...and the straddling commit is identified",
          list(verdict.straddling), ["bbb"])


def test_mode_structure_abstains_without_evidence(bh):
    """No repeats, or repeats on only one side, must both be UNDECIDED.

    The one-sided case is the subtle one. "Every repeated commit is below the
    fence" is exactly what a series that simply never got slower looks like,
    so reading it as evidence of two modes would excuse a genuine regression
    the first time it appeared. UNDECIDED still fails the build.
    """
    single = _mode_records([("aaa", 6000), ("bbb", 12000)])
    check("one measurement per commit decides nothing",
          bh.mode_structure(single, "H", "release", "b", 7500).verdict,
          bh.MODE_UNDECIDED)

    one_sided = _mode_records([
        ("aaa", 6000), ("aaa", 6200),
        ("bbb", 6100), ("bbb", 6300),
    ])
    verdict = bh.mode_structure(one_sided, "H", "release", "b", 7500)
    check("repeats all on one side decide nothing", verdict.verdict,
          bh.MODE_UNDECIDED)
    # Guard: this really is the one-sided shape, not an empty-repeats accident.
    check("...though the repeats were found", len(verdict.repeats), 2)

    # A record from another profile must not supply the missing evidence.
    mixed = _mode_records([("aaa", 6000), ("aaa", 6200)])
    mixed += [{"host": "H", "profile": "debug", "commit": "bbb",
               "entries": {"b": 12000}},
              {"host": "H", "profile": "debug", "commit": "bbb",
               "entries": {"b": 12400}}]
    check("another profile's repeats are not evidence here",
          bh.mode_structure(mixed, "H", "release", "b", 7500).verdict,
          bh.MODE_UNDECIDED)


def test_mode_structure_on_the_real_history(bh):
    """Positive control on this project's own data, both ways.

    `http_build_response_1KiB` is the series that was bisected across three
    commits for a regression that did not exist; `vfs_stat_root` is the series
    from the same runs that is *not* mode-structured. The check has to
    separate them, on real data, or it has not earned the right to suppress a
    build failure.

    Reads FROZEN_HISTORY, not the live file: this asserts a verdict about a
    specific set of measurements, and the live file grows on every benchmark
    boot. See FROZEN_HISTORY for what that cost.
    """
    records = bh.load_history(FROZEN_HISTORY)
    if not records:
        check("the frozen history fixture is present", False, True)
        return

    http = bh.mode_structure(
        records, "Logoplex3", "release", "http_build_response_1KiB", 7500)
    check("the real bimodal series is mode-structured",
          http.verdict, bh.MODE_STRUCTURED)
    # Guard: the verdict must rest on real repeat evidence, not on an empty set.
    check("...on at least the documented three repeated commits",
          len(http.repeats) >= 3, True)

    vfs = bh.mode_structure(
        records, "Logoplex3", "release", "vfs_stat_root", 4200)
    check("the continuously-spread series is NOT mode-structured",
          vfs.verdict == bh.MODE_STRUCTURED, False)


def test_mode_split_search_finds_what_a_fixed_fence_misses(bh):
    """The search must find the separating gap the report's own fence misses.

    This is a regression test for a real, measured failure of the first
    implementation, not a hypothetical. Using the pre-window Tukey fence as the
    split returned `run-noise` for `http_build_response_1KiB` -- because that
    fence (9103 ns) had been widened by the baseline window already containing
    both modes, landing it inside the HIGH mode's spread where commit
    `26c1c7330` straddles it. The build kept failing on a layout re-roll.

    Reads FROZEN_HISTORY for the same reason as the control above: the split it
    looks for is the gap between two modes in one particular set of runs.
    """
    records = bh.load_history(FROZEN_HISTORY)
    if not records:
        check("the frozen history fixture is present", False, True)
        return
    args = (records, "Logoplex3", "release", "http_build_response_1KiB")

    # Guard: the fence really must give the wrong answer, or this test is
    # asserting nothing and would keep passing if the search were deleted.
    fence = bh.mode_structure(*args, 9103)
    check("the old fixed fence really does miss it",
          fence.verdict == bh.MODE_STRUCTURED, False)

    found = bh.mode_split_search(*args, 6004, 12191)
    check("the search finds a separating split", found is not None, True)
    if found:
        split, verdict = found
        check("...and it is mode-structured there",
              verdict.verdict, bh.MODE_STRUCTURED)
        check("...at the gap between the two modes",
              6396 < split <= 8546, True)

    # A series that is not mode-structured must yield no split at all.
    none_found = bh.mode_split_search(
        records, "Logoplex3", "release", "vfs_stat_root", 3600, 4488)
    check("no split is invented for a non-bimodal series",
          none_found, None)


def test_mode_structured_shift_does_not_fail_the_build(bh):
    """A mode-structured shift must be reported but must not fail the build.

    This is the behavioural half of the fix and the part most likely to be
    broken by a later refactor: `report()`'s return value drives
    `--fail-on-regression`, and it previously counted every sustained shift.
    """
    lines = []
    verdict = bh.ModeVerdict(bh.MODE_STRUCTURED, {"aaa": [1, 2]}, {},
                             ["aaa"], ["bbb"])
    lines = bh.describe_mode_verdict("b", verdict)
    check("a mode-structured verdict says not to bisect",
          any("NOT a regression to bisect" in line for line in lines), True)

    noise = bh.ModeVerdict(bh.MODE_RUN_NOISE, {"aaa": [1, 9]},
                           {"aaa": [1, 9]}, [], [])
    lines = bh.describe_mode_verdict("b", noise)
    check("a run-noise verdict names the offending commit",
          any("aaa" in line for line in lines), True)

    check("an undecided verdict says nothing",
          bh.describe_mode_verdict(
              "b", bh.ModeVerdict(bh.MODE_UNDECIDED, {}, {}, [], [])), [])


# --------------------------------------------------------------------------
# Replication gate. See known-issues.md
# B-BENCH-CONFIRMED-REGRESSIONS-FIRE-ON-AN-UNCHANGED-BINARY-EVEN-ON-A-CLEAN-RUN.
# --------------------------------------------------------------------------

#: `b0` over eight quiet runs. Deliberately tight, so the band is narrow and
#: any movement the tests introduce is unambiguously outside it -- these tests
#: are about the replication gate and must not also depend on where a quartile
#: lands.
_QUIET = (500, 505, 510, 495, 502, 508, 498, 503)


def _repl_history(bh, earlier, earlier_commit, earlier_profile="release",
                  earlier_host="H"):
    """History whose *first* record is a repeat, followed by `_QUIET`.

    The repeat is placed before the trailing `SPEED_WINDOW` deliberately: the
    band must be computed from the eight quiet runs alone, while the
    replication evidence is still found. That separation is the point --
    replication is drawn from the whole comparable history, not just the band's
    window, and a test where the repeat sits inside the window could not tell
    the two apart because the repeat would move the fence as well.
    """
    stable = {f"b{i}": 1000 for i in range(1, 20)}
    first = {"host": earlier_host, "profile": earlier_profile,
             "commit": earlier_commit, "entries": dict(stable, b0=earlier)}
    rest = [{"host": "H", "profile": "release", "commit": f"h{i}",
             "entries": dict(stable, b0=value)}
            for i, value in enumerate(_QUIET)]
    return [first] + rest


def _repl_report(bh, history, current_b0, commit,
                 changed_files=None, bench_subsystems=None):
    """Run `report()` over `history` with `b0` at `current_b0`. -> (out, failed)."""
    import io
    import contextlib

    previous = history[-1]
    current = {name: (value, 10000, "OK", None, None)
               for name, value in previous["entries"].items()}
    current["b0"] = (current_b0, 10000, "OK", None, None)
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        failed = bh.report(previous, current, 25.0, records=history, host="H",
                           profile="release", commit=commit,
                           changed_files=changed_files,
                           bench_subsystems=bench_subsystems)
    return buf.getvalue(), failed


def test_replication_withdraws_what_the_same_binary_contradicts(bh):
    """A second run of the same commit that lands in-range withdraws the claim.

    This is the measured failure: two `--bench` runs of commit `602fc62e0`
    minutes apart, nothing rebuilt, produced three *confirmed* regressions
    between them. The band was not wrong about the numbers -- `page_alloc_free`
    really does sit at 293-453 ns and really did measure 680 -- so no band
    setting could have declined them. What declines them is that the same
    binary also produced 363 ns.

    The scenario here is the one where this fires without the whole comparison
    being an A/A pair: commit `xxx` was measured, then `h7`, then `xxx` was
    measured again -- an ordinary bisect.
    """
    history = _repl_history(bh, 500, "xxx")
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("a contradicted movement does not fail the build", failed, False)
    check("...and is shown, under a heading that says why",
          "NOT REPLICATED" in out, True)
    check("...and is not called a regression", "  REGRESSED" in out, False)
    check("...and the contradicting measurement is quoted",
          "500ns, inside" in out, True)
    # The A/A noise floor, which only repeat measurements can supply, and which
    # is what decides whether any *smaller* movement in b0 is judgeable at all.
    check("...and the same-commit spread is stated as a noise floor",
          "same-commit runs: [500, 3000]" in out, True)
    check("...as a percentage", "500% spread with no code change" in out, True)


def test_replication_promotes_what_every_run_of_the_commit_shows(bh):
    """The gate must still let a real regression through, or it is a mute check.

    A gate that only ever withdraws is indistinguishable from deleting the
    check. This is the other half: same construction, but the earlier run of
    the same commit is *also* slow, so the movement is a property of the binary
    and the word REGRESSED is earned.
    """
    history = _repl_history(bh, 2900, "xxx")
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("a replicated movement fails the build", failed, True)
    check("...and gets the confirmed heading", "  REGRESSED (" in out, True)
    # "image", not "commit". The gate is `same_image`, and its own docstring
    # says keying it on the commit made it blind to the very re-run it asks the
    # reader to perform -- so a heading saying "commit" describes a check this
    # harness deliberately does not implement.
    check("...which says what was replicated, in the unit it was replicated in",
          "every recorded run of this same kernel image shows it" in out, True)
    check("...and does not claim the commit was that unit",
          "every recorded run of this commit" in out, False)
    # The heading is the strongest label the harness emits, and it must not read
    # as "confirmed as a code effect". Replication is a within-image test while
    # the comparison it decorates is between two *different* images, so a
    # relinking artifact -- being deterministic -- replicates exactly as
    # perfectly as a real regression and arrives wearing this same heading.
    check("...and says what replication does NOT rule out",
          "'replicated' rules out noise, not layout" in out, True)
    check("...and names the check that settles it",
          "straddle-check.py --compare" in out, True)
    check("...and is not withdrawn", "NOT REPLICATED" in out, False)


def test_one_run_of_a_commit_is_unreplicated_and_still_fails(bh):
    """Measured once is not absolved -- it is unmeasured, and must still fail.

    This is the asymmetry the gate turns on, and the temptation to get it wrong
    is real: excusing every single-run movement would make the check silent in
    the ordinary case, since one run per commit is the norm. Then
    `--fail-on-regression` could never fire, which is the same failure as a
    check that cannot fire at all. Only a *positively evidenced* contradiction
    withdraws a claim -- exactly the standard `MODE_UNDECIDED` is held to.
    """
    history = _repl_history(bh, 500, "some-other-commit")
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("an unreplicated movement still fails the build", failed, True)
    check("...and says it has been measured only once",
          "measured only once" in out, True)
    check("...and is not given the confirmed heading",
          "  REGRESSED (" in out, False)
    check("...and tells the reader how to settle it",
          "WITHOUT" in out and "rebuilding to confirm" in out, True)


def test_replication_evidence_must_be_the_same_binary_on_the_same_terms(bh):
    """Three ways a repeat can look like evidence without being any.

    Each of these would silently *manufacture* replication -- the failure
    direction that matters, because it ends with a real regression waved
    through as "contradicted".
    """
    # 1. `unknown` is what git_commit() returns when it could not read HEAD.
    #    Two runs that both failed to read HEAD are not two runs of one binary.
    history = _repl_history(bh, 500, "unknown")
    out, failed = _repl_report(bh, history, 3000, "unknown")
    check("an unknown commit is not an identity", failed, True)
    check("...so the movement is unreplicated, not contradicted",
          "NOT REPLICATED" in out, False)

    # 2. A different build profile. Its numbers are not comparable at all --
    #    the same reason `comparable_records` exists.
    history = _repl_history(bh, 500, "xxx", earlier_profile="debug")
    _out, failed = _repl_report(bh, history, 3000, "xxx")
    check("another profile's run is not evidence here", failed, True)

    # 3. A different host.
    history = _repl_history(bh, 500, "xxx", earlier_host="OTHER")
    _out, failed = _repl_report(bh, history, 3000, "xxx")
    check("another host's run is not evidence here", failed, True)

    # And the unit-level statement of the same rule, so a refactor that moves
    # the filtering out of `values_for_binary` is caught here too.
    history = _repl_history(bh, 500, "xxx")
    check("values_for_binary finds the repeat",
          bh.values_for_binary(history, "H", "release", "b0",
                               {"commit": "xxx"}), [500])
    check("...and refuses to match the unknown sentinel",
          bh.values_for_binary(history, "H", "release", "b0",
                               {"commit": bh.UNKNOWN_COMMIT}), [])
    check("...and refuses to match on no identity at all",
          bh.values_for_binary(history, "H", "release", "b0", None), [])


def test_binary_identity_is_the_image_not_the_commit(bh, tmpdir):
    """The gate must key on the kernel image, because the commit both over-
    and under-identifies it.

    This is a measured failure, not a hypothetical. On 2026-08-19 a run flagged
    `page_alloc_zeroed_free` +83% as REGRESSED, UNREPLICATED and printed its
    standard advice: re-run WITHOUT rebuilding to confirm. That was done --
    `--no-build --no-stage`, a byte-identical image -- but a commit had been
    made in between, so the re-run was filed under a new commit. The gate,
    keyed on commit, could not see the confirmation; the A/A banner, keyed on
    commit, stayed silent; and the re-run was instead judged as a fresh
    commit's first measurement, manufacturing four new "REGRESSED,
    UNREPLICATED" claims (`sched_pick_next_d1` +62%, `vfs_stat_breakdown_ns`
    +45%, both gzip benchmarks ~+30%) out of an image that had not changed by
    one byte.

    The converse holds too and is in the same record: the flagged run carried
    `dirty: true`, so its commit label named a tree that was never built. Two
    such runs share a label while measuring different code.
    """
    sha_a, sha_b = "aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb"

    check("the hash wins when present",
          bh.binary_identity({"kernel_sha": sha_a, "commit": "c1"}),
          f"sha:{sha_a}")
    check("...even on a dirty tree, which is the case it exists for",
          bh.binary_identity({"kernel_sha": sha_a, "commit": "c1",
                              "dirty": True}), f"sha:{sha_a}")
    check("a clean commit still identifies a legacy record",
          bh.binary_identity({"commit": "c1"}), "commit:c1")
    check("a dirty commit identifies nothing -- its tree was never built",
          bh.binary_identity({"commit": "c1", "dirty": True}), None)
    check("an unreadable HEAD identifies nothing",
          bh.binary_identity({"commit": bh.UNKNOWN_COMMIT}), None)
    check("and neither does a record with no fields at all",
          bh.binary_identity({}), None)

    # The two directions the commit gets wrong, stated through the relation the
    # rest of the gate is built on. Note these are *not* expressible as an
    # equality between the identity strings above in every case -- which is why
    # `same_image` exists and is what the gate calls.
    check("one image under two commits is one binary",
          bh.same_image({"kernel_sha": sha_a, "commit": "c1"},
                        {"kernel_sha": sha_a, "commit": "c2"}), True)
    check("two images under one commit are not",
          bh.same_image({"kernel_sha": sha_a, "commit": "c1"},
                        {"kernel_sha": sha_b, "commit": "c1"}), False)

    # The transition case, and the reason identity cannot be a string: a hashed
    # run against a legacy record of the same clean commit. Neither `sha:` nor
    # `commit:` label matches the other, yet they are the same image and the
    # history is full of the unhashed side.
    check("a hashed run matches an older unhashed run of the same clean commit",
          bh.same_image({"commit": "c1"},
                        {"kernel_sha": sha_a, "commit": "c1"}), True)
    check("...in either order -- the relation is symmetric",
          bh.same_image({"kernel_sha": sha_a, "commit": "c1"},
                        {"commit": "c1"}), True)
    check("...but a dirty unhashed run matches nothing, hashed or not",
          bh.same_image({"commit": "c1", "dirty": True},
                        {"kernel_sha": sha_a, "commit": "c1"}), False)
    # Asymmetric in strength, not in argument order: two hashes settle the
    # question between themselves and a commit label cannot overrule them. This
    # project has measured function *placement* moving a benchmark several-fold
    # with identical source, so "same commit" is not a licence to ignore a hash.
    check("two differing hashes are not rescued by a matching clean commit",
          bh.same_image({"kernel_sha": sha_a, "commit": "c1"},
                        {"kernel_sha": sha_b, "commit": "c1"}), False)
    check("two unidentifiable records are never the same image",
          bh.same_image({"commit": "c1", "dirty": True},
                        {"commit": "c1", "dirty": True}), False)
    check("and neither are two unreadable HEADs",
          bh.same_image({"commit": bh.UNKNOWN_COMMIT},
                        {"commit": bh.UNKNOWN_COMMIT}), False)

    # Unidentifiable must never match unidentifiable. `values_for_binary` is
    # where that would leak, because None is a perfectly good dict value.
    unknowns = [{"host": "H", "profile": "release", "commit": "c1",
                 "dirty": True, "entries": {"b0": 500}}]
    check("two unidentifiable runs are not evidence about each other",
          bh.values_for_binary(unknowns, "H", "release", "b0",
                               {"commit": "c1", "dirty": True}), [])
    check("...and no `this_run` at all finds nothing rather than everything",
          bh.values_for_binary(unknowns, "H", "release", "b0", None), [])

    # And the hash itself: same bytes, same value; different bytes, different.
    one = os.path.join(tmpdir, "k1")
    two = os.path.join(tmpdir, "k2")
    three = os.path.join(tmpdir, "k3")
    with open(one, "wb") as handle:
        handle.write(b"kernel-image-bytes")
    with open(two, "wb") as handle:
        handle.write(b"kernel-image-bytes")
    with open(three, "wb") as handle:
        handle.write(b"kernel-image-byteS")
    check("identical images hash alike", bh.kernel_sha(one), bh.kernel_sha(two))
    check("a one-bit difference does not",
          bh.kernel_sha(one) == bh.kernel_sha(three), False)
    check("an unreadable image is 'unknown', not a shared sentinel",
          bh.kernel_sha(os.path.join(tmpdir, "no-such-kernel")), None)


def test_a_no_rebuild_rerun_confirms_across_a_commit(bh):
    """End to end: the re-run the gate asks for is counted even if HEAD moved.

    The regression this locks down is the whole point of `binary_identity`. Two
    records of one image, different commits: the second must be recognised as
    an A/A comparison and must not fail the build. Before the fix this printed
    a regression.
    """
    import io
    import contextlib

    sha = "0123456789abcdef"
    stable = {f"b{i}": 1000 for i in range(1, 20)}
    history = [{"host": "H", "profile": "release", "commit": f"h{i}",
                "kernel_sha": f"old{i:012d}", "entries": dict(stable, b0=value)}
               for i, value in enumerate(_QUIET)]
    # The flagged run: this image's first appearance, filed under a dirty tree.
    history.append({"host": "H", "profile": "release", "commit": "c1",
                    "dirty": True, "kernel_sha": sha,
                    "entries": dict(stable, b0=500)})

    previous = history[-1]
    current = {name: (value, 10000, "OK", None, None)
               for name, value in previous["entries"].items()}
    current["b0"] = (3000, 10000, "OK", None, None)

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        failed = bh.report(previous, current, 25.0, records=history, host="H",
                           profile="release", commit="c2",
                           this_run={"kernel_sha": sha, "commit": "c2"})
    out = buf.getvalue()
    check("a no-rebuild re-run across a commit is seen as A/A",
          "A/A COMPARISON" in out, True)
    check("...and names the image rather than the commit",
          f"sha:{sha}" in out, True)
    check("...and does not fail the build", failed, False)

    # The same pair without hashes -- a legacy record -- must not claim A/A,
    # but must not let the reader infer it from the header's matching commit
    # either.
    legacy_prev = {"host": "H", "profile": "release", "commit": "c1",
                   "dirty": True, "entries": dict(stable, b0=500)}
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(legacy_prev, current, 25.0, records=history + [legacy_prev],
                  host="H", profile="release", commit="c1",
                  this_run={"commit": "c1", "dirty": True})
    out = buf.getvalue()
    check("an unhashed dirty pair is not called A/A",
          "A/A COMPARISON" in out, False)
    check("...but the reader is told the image is unknown",
          "SAME COMMIT" in out and "UNKNOWN IMAGE" in out, True)

    # The third state, which is neither A/A nor unknown: both sides hashed,
    # hashes differ, commit label the same. That is *known different* code, and
    # calling it "unknown" would understate what the record says.
    hashed_prev = {"host": "H", "profile": "release", "commit": "c1",
                   "kernel_sha": "ffffffffffffffff",
                   "entries": dict(stable, b0=500)}
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(hashed_prev, current, 25.0, records=history + [hashed_prev],
                  host="H", profile="release", commit="c1",
                  this_run={"kernel_sha": sha, "commit": "c1"})
    out = buf.getvalue()
    check("two hashed runs of one commit that differ are not called A/A",
          "A/A COMPARISON" in out, False)
    check("...nor merely unknown -- the record says they differ",
          "DIFFERENT IMAGE" in out and "UNKNOWN IMAGE" not in out, True)
    check("...and both hashes are named so the reader can check",
          "ffffffffffffffff" in out and sha in out, True)

    # A legacy pair with no hashes and a *clean* commit is still A/A: that is
    # the entire pre-hash history, and refusing to judge it would retire the
    # check on every record written before today.
    clean_prev = {"host": "H", "profile": "release", "commit": "c1",
                  "entries": dict(stable, b0=500)}
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(clean_prev, current, 25.0, records=history + [clean_prev],
                  host="H", profile="release", commit="c1",
                  this_run={"commit": "c1"})
    out = buf.getvalue()
    check("two clean unhashed runs of one commit are still A/A",
          "A/A COMPARISON" in out, True)


def test_an_aa_comparison_is_named_and_cannot_fail_the_build(bh):
    """Baseline and current sharing a commit makes the whole diff an A/A test.

    Not a statistical claim but an arithmetic one: the two runs share a commit,
    so the difference between them has no code term in it. Worth stating
    separately from the per-benchmark gate because it also covers the movements
    the per-benchmark gate declines to judge -- the ones with too little
    history for a band, which would otherwise still print as `REGRESSED,
    UNCONFIRMED` and still fail the build on a binary compared to itself.
    """
    history = _repl_history(bh, 500, "xxx")
    history[-1]["commit"] = "xxx"          # baseline IS the current commit
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("an A/A comparison cannot fail the build", failed, False)
    check("...and says so before any list", "A/A COMPARISON" in out, True)
    # Named as an *image*, and the identity printed is the one the gate keyed
    # on -- here derived from the commit, because these legacy-shaped records
    # carry no hash. The wording matters: "same commit" was the claim that was
    # wrong in both directions (see `binary_identity`), so the banner must not
    # go on making it.
    check("...naming the shared image", "SAME kernel image (commit:xxx)" in out,
          True)
    check("...and pointing at the measurement",
          "5 of 83 benchmarks" in out, True)

    # A different baseline commit must NOT trip it, or the banner would excuse
    # every run and the check would be gone.
    history = _repl_history(bh, 2900, "xxx")
    out, failed = _repl_report(bh, history, 3000, "xxx")
    check("an ordinary comparison is not called A/A",
          "A/A COMPARISON" in out, False)
    check("...and still fails the build", failed, True)


def test_an_aa_comparison_prints_no_verdict_heading_at_all(bh):
    """The A/A banner and a `REGRESSED` list may not be emitted together.

    Both were, and the heading won. On 2026-08-19 `lock_uncontended` was written
    up as `REGRESSED ... replicated` out of a run whose own banner, four lines
    above, said no movement in it could have been caused by code -- and whose
    `--fail-on-regression` had already declined to count it. Its two samples
    were 280 and 486 ns.

    A report that contradicts itself is decided by whichever half the reader
    happens to weigh, which is not a property a measuring instrument may have.
    Only the *claim* is dropped: the movements are still listed, because the
    spread of an A/A pair is the most useful thing it produces.
    """
    history = _repl_history(bh, 500, "xxx")
    history[-1]["commit"] = "xxx"          # baseline IS the current commit
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("an A/A comparison still cannot fail the build", failed, False)
    check("...and prints no confirmed-regression heading",
          "  REGRESSED (" in out, False)
    # Asserted heading by heading rather than on the bare word, because
    # "REGRESSED" is a substring of three different headings and a loose check
    # would pass while two of them still printed.
    for heading in ("  REGRESSED, UNREPLICATED", "  REGRESSED, UNCONFIRMED",
                    "  IMPROVED (", "  IMPROVED, UNCONFIRMED",
                    "  NOT REPLICATED"):
        check(f"...nor {heading.strip()}", heading in out, False)
    # The movement itself is not hidden -- that would throw away the one number
    # an A/A pair exists to produce.
    check("...but the movement is still shown", "b0" in out, True)
    check("...under a heading that says what it is",
          "A/A MOVEMENT" in out, True)
    check("...and says it is neither a regression nor an improvement",
          "NOT a regression and NOT an improvement" in out, True)
    check("...and tells the reader what the number is for",
          "per-benchmark noise floor" in out, True)


def test_an_aa_listing_keeps_the_per_benchmark_repeat_samples(bh):
    """The A/A heading must not swallow the per-benchmark detail.

    "This binary produced 500 and 3000 ns" is a measured noise floor for *this*
    benchmark on this host, and it is the number that decides whether any
    smaller movement in it is judgeable at all. A collective heading cannot say
    that per benchmark, so the row detail is kept under the new heading rather
    than dropped with the claim.
    """
    history = _repl_history(bh, 500, "xxx")
    history[-1]["commit"] = "xxx"
    out, _failed = _repl_report(bh, history, 3000, "xxx")

    check("the contradicting sample is still quoted",
          "-> b0: another run of this same binary" in out, True)
    check("...and the spread is still stated as a noise floor",
          "500% spread with no code change" in out, True)


def test_direction_histogram_fires_only_on_a_mixed_comparison(bh):
    """A mixed direction histogram is evidence about the set, not about a row.

    A code change moves what it touched, in the direction it pushed. Relinking
    moves whatever lands badly, in whichever direction each one lands -- so
    movement out of band in *both* directions at once is the signature of
    placement rather than code, and it is a fact no per-benchmark statistic can
    supply. On 2026-08-19 a scheduler-only commit produced thirteen perfectly
    replicating movers of which ten were *faster*; read one at a time, there was
    no way to see it.

    It must stay silent on a one-directional comparison, or it would be advice
    printed on every report and read on none.
    """
    import io
    import contextlib

    # Twenty stable benchmarks so the whole-suite drift median does not move,
    # plus b0 slow and b1 fast against a long quiet history of both.
    stable = {f"b{i}": 1000 for i in range(2, 22)}
    history = [{"host": "H", "profile": "release", "commit": f"h{i}",
                "kernel_sha": f"old{i:012d}",
                "entries": dict(stable, b0=v, b1=v)}
               for i, v in enumerate(_QUIET)]
    previous = history[-1]

    def run(b0, b1):
        current = {name: (value, 10 ** 9, "OK", None, None)
                   for name, value in previous["entries"].items()}
        current["b0"] = (b0, 10 ** 9, "OK", None, None)
        current["b1"] = (b1, 10 ** 9, "OK", None, None)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            bh.report(previous, current, 25.0, records=history, host="H",
                      profile="release", commit="new",
                      this_run={"kernel_sha": "newimage", "commit": "new"})
        return buf.getvalue()

    mixed = run(3000, 100)
    check("a mixed comparison prints the histogram",
          "DIRECTION HISTOGRAM" in mixed, True)
    check("...counting both directions",
          "1 benchmark(s) left their own range slower and 1 left it faster"
          in mixed, True)
    check("...and names the check that settles it",
          "straddle-check.py --compare" in mixed, True)
    # Not a verdict: an optimisation commit legitimately produces a mixed
    # histogram, so this is an observation with its own falsifier attached.
    check("...without withdrawing the regression",
          "  REGRESSED" in mixed, True)

    slower_only = run(3000, 1000)
    check("a one-directional comparison prints no histogram",
          "DIRECTION HISTOGRAM" in slower_only, False)


def test_replication_declines_the_measured_false_positives(bh):
    """Positive control: the documented A/A pair, replayed from the real file.

    `bench/history.jsonl`'s last two records share commit `602fc62e0` and are
    the pair written up in known-issues.md. Replaying run B against run A, the
    harness reported `page_alloc_free` +85% and `vfs_stat_breakdown_full` +36%
    as *confirmed* regressions on code that had not changed, and graded the run
    itself `RUN CLEAN` while doing it.

    Synthetic tests can only show the gate does what it was written to do. This
    one shows it does it to the actual numbers that fooled a reader.
    """
    import json
    if not os.path.exists(HISTORY):
        check("history.jsonl exists for the A/A control", False, True)
        return
    records = [json.loads(line) for line in
               open(HISTORY, encoding="utf-8").read().splitlines()
               if line.strip()]

    # The pair is found by the commit it is documented under, not by taking the
    # last two records.  It *was* the tail when this was written, and reading it
    # that way meant every later benchmark boot pushed the control out of reach:
    # by 2026-08-18 it had been silently SKIPping for 29 appended rows, which is
    # a positive control that has stopped controlling while still printing a
    # line.  The pair itself is still right there at indices 33/34.
    at = [i for i, r in enumerate(records) if r.get("commit") == AA_COMMIT]
    if len(at) < 2:
        check(f"the documented A/A pair ({AA_COMMIT}) is still in history",
              False, True)
        return

    run_b = records[at[1]]
    host, profile = run_b["host"], bh.record_profile(run_b)
    prior = records[:at[1]]
    previous = bh.previous_for_host(prior, host, profile)
    current = {name: (value, 10 ** 9, "OK", None, None)
               for name, value in run_b["entries"].items()}

    import io
    import contextlib
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        failed = bh.report(previous, current, 25.0, records=prior, host=host,
                           profile=profile, commit=run_b["commit"])
    out = buf.getvalue()

    check("the measured A/A pair no longer fails the build", failed, False)
    check("...and no benchmark is called a confirmed regression",
          "  REGRESSED (" in out, False)
    for name in ("page_alloc_free", "vfs_stat_breakdown_full"):
        if name not in run_b["entries"]:
            continue
        check(f"...{name} is withdrawn by name",
              f"-> {name}: another run of this same binary" in out, True)


# --- hot-symbol addresses -----------------------------------------------------
#
# `elf_symbol_addresses` exists because a 4x swing in the SHA-256 benchmarks was
# caused by `crypto::compress` changing address with its machine code
# byte-identical (known-issues.md, A-A-4x-CRYPTO-"REGRESSION").  Recording the
# address next to the timing is what makes a repeat recognisable without a
# bisect, so the parser has to work on a *real* kernel ELF -- which is built by
# either of Rust's two mangling schemes depending on the toolchain channel.  The
# investigation itself accidentally produced one build of each, and the parser
# initially appeared to fail on one of them (it was a path problem, not a
# parsing one), which is precisely why both schemes are pinned here.
#
# The ELFs are synthesised rather than taken from `target/`, so the tests run in
# a bare checkout where nothing has been built.

def _synth_elf(path, symbols, *, ident_class=2, ident_data=1, magic=b"\x7fELF"):
    """Write a minimal ELF64 with a .symtab/.strtab holding `symbols`.

    `symbols` is a list of `(name, value)`.  Only the fields
    `elf_symbol_addresses` actually reads are populated; everything else is
    zero, which is the point -- the parser must not depend on more than it
    needs.
    """
    import struct

    strtab = bytearray(b"\x00")
    offsets = []
    for name, _ in symbols:
        offsets.append(len(strtab))
        strtab += name.encode() + b"\x00"

    symtab = bytearray()
    for (_, value), noff in zip(symbols, offsets):
        ent = bytearray(24)
        struct.pack_into("<I", ent, 0, noff)
        struct.pack_into("<Q", ent, 8, value)
        symtab += ent

    ehdr_len = 64
    sym_off = ehdr_len
    str_off = sym_off + len(symtab)
    sh_off = str_off + len(strtab)

    ehdr = bytearray(ehdr_len)
    ehdr[0:4] = magic
    ehdr[4] = ident_class
    ehdr[5] = ident_data
    struct.pack_into("<Q", ehdr, 0x28, sh_off)
    struct.pack_into("<H", ehdr, 0x3A, 64)
    struct.pack_into("<H", ehdr, 0x3C, 3)

    def shdr(sh_type, off, size, link=0, entsize=0):
        s = bytearray(64)
        struct.pack_into("<I", s, 0x04, sh_type)
        struct.pack_into("<Q", s, 0x18, off)
        struct.pack_into("<Q", s, 0x20, size)
        struct.pack_into("<I", s, 0x28, link)
        struct.pack_into("<Q", s, 0x38, entsize)
        return s

    blob = bytes(ehdr) + bytes(symtab) + bytes(strtab)
    blob += bytes(shdr(0, 0, 0))
    blob += bytes(shdr(2, sym_off, len(symtab), link=2, entsize=24))
    blob += bytes(shdr(3, str_off, len(strtab)))
    with open(path, "wb") as fh:
        fh.write(blob)
    return path


def test_hot_symbols_read_legacy_mangling(bh, tmpdir):
    elf = _synth_elf(os.path.join(tmpdir, "legacy.elf"), [
        ("_ZN4sha28compress17h8234a763022d2833E", 0xffffffff80afce00),
        ("_ZN6kernel6crypto15sha512_compress17hff0d9f03de4c6bb2E",
         0xffffffff80af5580),
    ])
    got = bh.elf_symbol_addresses(elf)
    check("legacy mangling: compress address is read",
          got.get("crypto::compress"), "0xffffffff80afce00")
    check("legacy mangling: sha512_compress is read",
          got.get("crypto::sha512_compress"), "0xffffffff80af5580")


def test_hot_symbols_read_v0_mangling(bh, tmpdir):
    elf = _synth_elf(os.path.join(tmpdir, "v0.elf"), [
        ("_RNvCsjQArNW8oxTF_4sha28compress", 0xffffffff80364980),
    ])
    got = bh.elf_symbol_addresses(elf)
    check("v0 mangling: the same pattern still matches",
          got.get("crypto::compress"), "0xffffffff80364980")


def test_hot_symbols_ignore_unrelated_compress_functions(bh, tmpdir):
    """`fs::compress` and `mm::compress` must not be mistaken for the crypto one.

    The kernel has more than fifty symbols containing the word "compress"
    (deflate, lz4, zstd, bzip2, the swap compressor).  The `6crypto` length
    prefix is the whole of what keeps them out, so it is asserted rather than
    assumed.
    """
    elf = _synth_elf(os.path.join(tmpdir, "noise.elf"), [
        ("_RNvNtNtCsjQArNW8oxTF_6kernel2fs8compress7deflate", 0x1111),
        ("_RNvNtNtCsjQArNW8oxTF_6kernel2mm8compress8compress", 0x2222),
        ("_ZN6kernel2fs9fcompress13compress_data17habcdE", 0x3333),
    ])
    got = bh.elf_symbol_addresses(elf)
    check("unrelated compress symbols are not reported",
          {k: v for k, v in got.items() if v is not None}, {})


def test_hot_symbols_prefer_the_function_over_a_wrapper(bh, tmpdir):
    """A monomorphised wrapper mentions the function and is not the function.

    Real output contains entries like
    `drop_in_place<Vec<kernel::fs::fcompress::CompressionRule>>` that embed
    another symbol's name.  Shortest-match is how the function itself is
    picked; if that rule broke, the recorded address would silently become some
    unrelated thunk's, which is worse than recording nothing.
    """
    elf = _synth_elf(os.path.join(tmpdir, "wrap.elf"), [
        ("_RINvNtCsg3_4core3ptr13drop_in_placeNtCsjQ_4sha28compressEB1j_",
         0xdead0000),
        ("_RNvCsjQArNW8oxTF_4sha28compress", 0xffffffff80364980),
    ])
    got = bh.elf_symbol_addresses(elf)
    check("the shorter (real) symbol wins over the wrapper",
          got.get("crypto::compress"), "0xffffffff80364980")


def test_hot_symbols_skip_undefined_symbols(bh, tmpdir):
    """A st_value of 0 is an undefined symbol, not an address of zero."""
    elf = _synth_elf(os.path.join(tmpdir, "undef.elf"), [
        ("_RNvCsjQArNW8oxTF_4sha28compress", 0),
    ])
    got = bh.elf_symbol_addresses(elf)
    check("an undefined symbol contributes no address",
          {k: v for k, v in got.items() if v is not None}, {})


def test_hot_symbols_degrade_quietly_on_bad_input(bh, tmpdir):
    """Never raise: this is bookkeeping bolted to a 9-minute measurement.

    A completed benchmark run must be written even if the ELF is absent,
    truncated, 32-bit, big-endian or not an ELF at all.  Losing the record to
    an exception in the diagnostic would cost far more than the diagnostic is
    worth.
    """
    missing = os.path.join(tmpdir, "nope.elf")
    check("absent file yields {}", bh.elf_symbol_addresses(missing), {})

    notelf = os.path.join(tmpdir, "plain.txt")
    with open(notelf, "w", encoding="utf-8") as fh:
        fh.write("this is not an ELF file, it is a note about one\n")
    check("non-ELF yields {}", bh.elf_symbol_addresses(notelf), {})

    trunc = os.path.join(tmpdir, "trunc.elf")
    with open(trunc, "wb") as fh:
        fh.write(b"\x7fELF\x02\x01" + b"\x00" * 20)
    check("truncated ELF yields {}", bh.elf_symbol_addresses(trunc), {})

    elf32 = _synth_elf(os.path.join(tmpdir, "elf32.elf"),
                       [("_ZN4sha28compress17hE", 0x1000)],
                       ident_class=1)
    check("32-bit ELF yields {}", bh.elf_symbol_addresses(elf32), {})


def test_experiment_runs_are_recorded_but_never_a_baseline(bh, tmpdir):
    """A probe run must be kept and must never become the yardstick.

    The five runs of the placement investigation are the motivating case: three
    read ~8085 ns for `crypto_sha256_64B` and two read ~1936 for *the same
    source*, built with a different symbol-mangling scheme. Landing all five in
    one 8-run window would push that benchmark's outlier fence past 4x and
    silently blind the detector for it. Deleting them was the wrong answer --
    they are the evidence for the finding -- so they are labelled instead.
    """
    import io
    import json
    import contextlib

    log = write(tmpdir, "serial.txt", "\n".join([
        "[bench] SCORE crypto_sha256_64B 8085 - TRACK 2000 1000",
        "[bench] CANARY 8 8 100 8 9 12 11 0 800 900",
    ]) + "\n")
    history = os.path.join(tmpdir, "history.jsonl")
    with contextlib.redirect_stdout(io.StringIO()) as buf:
        bh.main(["--serial", log, "--history", history, "--profile", "release",
                 "--experiment", "QEMU tb-size probe, test arm"])
    record = json.loads(open(history, encoding="utf-8").read().strip())
    check("the reason is stored verbatim",
          record["experiment"], "QEMU tb-size probe, test arm")
    check("...and the run says so, since the exclusion is otherwise invisible",
          "excluded from every future baseline" in buf.getvalue(), True)

    check("an experiment record is not comparable history",
          bh.comparable_records([record], record["host"], "release"), [])
    check("...and so cannot be the previous run",
          bh.previous_for_host([record], record["host"], "release"), None)

    # An ordinary run alongside it is still found, so the filter excludes the
    # probe rather than merely emptying the window.
    ordinary = dict(record)
    ordinary.pop("experiment")
    ordinary["timestamp"] = "2026-08-18T16:00:00+00:00"
    window = bh.comparable_records([record, ordinary], record["host"], "release")
    check("the ordinary run beside it is still eligible", len(window), 1)
    check("...and it is the one without the label",
          window[0]["timestamp"], "2026-08-18T16:00:00+00:00")

    check("an ordinary run carries no experiment key at all",
          "experiment" in ordinary, False)
    check("...which reads as the empty reason", bh.record_experiment(ordinary), "")
    check("a non-string label is not trusted to exclude",
          bh.record_experiment({"experiment": 1}), "")


def test_committed_history_keeps_the_probe_runs_labelled(bh):
    """The real history must carry the labels, not just the code that reads them.

    A guard, not a unit test: the five probe records were labelled by hand
    after the fact, and nothing else would notice if a future merge or rewrite
    dropped the field and quietly re-admitted them to the baseline.
    """
    import json

    path = os.path.join(os.path.dirname(os.path.dirname(
        os.path.abspath(__file__))), "bench", "history.jsonl")
    if not os.path.exists(path):
        check("history file exists to be checked", True, True)
        return
    records = [json.loads(l) for l in
               open(path, encoding="utf-8").read().splitlines() if l.strip()]
    labelled = {r["timestamp"] for r in records if bh.record_experiment(r)}
    expected = {
        "2026-08-18T12:58:40+00:00",
        "2026-08-18T13:02:07+00:00",
        "2026-08-18T15:06:22+00:00",
        "2026-08-18T15:33:15+00:00",
        "2026-08-18T15:46:49+00:00",
    }
    check("every placement-probe run is still labelled an experiment",
          expected - labelled, set())


def test_hot_symbols_absent_and_empty_mean_different_things(bh, tmpdir):
    """`hot_symbols` absent = nobody looked; `{}` = looked, found nothing.

    Collapsing the two would let a reader who finds no addresses next to a 4x
    swing conclude the addresses did not move, when in fact no ELF was passed.
    That is the same mistake this field exists to prevent, one level up, so the
    distinction is asserted rather than left to the comment that states it.
    """
    import io
    import json
    import contextlib

    serial = "\n".join([
        "[bench] SCORE crypto_sha256_64B 1935 - TRACK 2000 1000",
        "[bench] CANARY 8 8 100 8 9 12 11 0 800 900",
    ]) + "\n"

    def run(extra, history_name):
        log = write(tmpdir, "serial-" + history_name, serial)
        history = os.path.join(tmpdir, history_name)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            bh.main(["--serial", log, "--history", history,
                     "--profile", "release"] + extra)
        line = [l for l in open(history, encoding="utf-8").read().splitlines()
                if l.strip()][-1]
        return json.loads(line)

    no_elf = run([], "none.jsonl")
    check("no --kernel-elf leaves the field out entirely",
          "hot_symbols" in no_elf, False)

    blank = _synth_elf(os.path.join(tmpdir, "blank.elf"),
                       [("_ZN6kernel2mm5alloc17habcE", 0x2000)])
    looked = run(["--kernel-elf", blank], "blank.jsonl")
    check("an ELF with none of the hot functions names them all as null",
          looked.get("hot_symbols"),
          {k: None for k in bh.HOT_SYMBOLS})

    real = _synth_elf(os.path.join(tmpdir, "real.elf"),
                      [("_RNvCsjQArNW8oxTF_4sha28compress",
                        0xffffffff80afce00)])
    found = run(["--kernel-elf", real], "real.jsonl")
    expected = {k: None for k in bh.HOT_SYMBOLS}
    expected["crypto::compress"] = "0xffffffff80afce00"
    check("the address reaches the history record",
          found.get("hot_symbols"), expected)


def test_hot_symbols_report_a_stale_pattern_as_null_not_absence(bh, tmpdir):
    """A pattern that stops matching must be visible, not silently dropped.

    `HOT_SYMBOLS` keys on the length-prefixed module path, so a function that
    changes module stops matching. This is not hypothetical: `compress` was
    `kernel::crypto`'s until `kernel/src/crypto.rs` was moved onto the shared
    `sha2` crate, at which point `6crypto8compress` matched nothing and the
    pattern had to become `4sha28compress`.

    The symbol used here is therefore the *pre-migration* one, which is exactly
    what a stale pattern looks like from the current pattern's point of view.
    Dropping the key on a miss would hide that -- and hide it at the worst
    moment, since the change that breaks the pattern is the one that relocates
    the function and so is the most likely to swing the benchmark.
    """
    moved = _synth_elf(os.path.join(tmpdir, "moved.elf"), [
        # Where `compress` lived before the sha2 adoption. The current pattern
        # must NOT match it -- if this ever starts matching, the pattern has
        # been widened enough to catch unrelated modules too.
        ("_RNvNtCsjQArNW8oxTF_6kernel6crypto8compress", 0xffffffff80364980),
    ])
    got = bh.elf_symbol_addresses(moved)
    check("a symbol from the function's old module no longer matches",
          got.get("crypto::compress"), None)
    check("...but the key is still there, so the miss is legible",
          "crypto::compress" in got, True)

    # An unparseable binary says nothing at all, and must not be confused with
    # a binary that said "this symbol is gone".
    check("a binary that told us nothing is empty, not all-null",
          bh.elf_symbol_addresses(os.path.join(tmpdir, "absent.elf")), {})


# ---------------------------------------------------------------------------
# Layout padding (`SLATEOS_TEXT_PAD` -> `textpad=` in the boot banner)
# ---------------------------------------------------------------------------


def test_the_pad_is_read_from_the_banner(bh, tmpdir):
    log = write(tmpdir, "serial.txt", "\n".join([
        "=== Kernel booting ===",
        "[boot] build profile: sanitizer=none textpad=3072",
        "[bench] SCORE syscall_dispatch 120 200 PASS 130 1000",
    ]) + "\n")
    check("the pad is an int, taken from the kernel's own report",
          bh.parse_text_pad(log), 3072)


def test_no_banner_is_none_not_zero(bh, tmpdir):
    """The distinction the whole field exists to preserve.

    Every record on disk before this landed was written by a kernel with no
    `textpad=` in its banner. If absence read as 0, all of them would join the
    unpadded arm of a layout comparison, and the resulting "band" would be the
    spread between builds that may or may not have differed in placement -- a
    number with no defined meaning, used to *dismiss* regressions.
    """
    log = write(tmpdir, "serial.txt", "\n".join([
        "=== Kernel booting ===",
        "[bench] SCORE syscall_dispatch 120 200 PASS 130 1000",
    ]) + "\n")
    got = bh.parse_text_pad(log)
    check("a pre-banner log says None", got, None)
    check("...and None does not compare equal to 0", got == 0, False)


def test_an_explicit_zero_is_zero(bh, tmpdir):
    log = write(tmpdir, "serial.txt",
                "[boot] build profile: sanitizer=none textpad=0\n")
    got = bh.parse_text_pad(log)
    check("an unpadded build reports 0", got, 0)
    check("...and 0 is not None", got is None, False)


def test_the_pad_survives_another_key_being_appended(bh, tmpdir):
    """The banner is matched on its key, not on the whole line.

    `sanitizer=` was added first and `textpad=` second; whatever is added third
    must not silently stop either being read.
    """
    log = write(tmpdir, "serial.txt",
                "[boot] build profile: sanitizer=none textpad=1024 lto=thin\n")
    check("a third key does not hide the second", bh.parse_text_pad(log), 1024)


def test_a_missing_log_says_nothing_rather_than_zero(bh, tmpdir):
    check("an unreadable log cannot report a pad",
          bh.parse_text_pad(os.path.join(tmpdir, "absent.txt")), None)


def test_the_record_carries_the_pad_the_kernel_reported(bh, tmpdir):
    import io
    import json
    import contextlib

    log = write(tmpdir, "serial.txt", "\n".join([
        "[boot] build profile: sanitizer=none textpad=2048",
        "[bench] SCORE syscall_dispatch 120 200 PASS 130 1000",
        "[bench] CANARY 8 8 100 8 9 12 11 0 800 900",
    ]) + "\n")
    history = os.path.join(tmpdir, "history.jsonl")
    with contextlib.redirect_stdout(io.StringIO()):
        bh.main(["--serial", log, "--history", history, "--profile", "release"])
    record = json.loads(open(history, encoding="utf-8").read().strip())
    check("the record stores the pad the kernel reported",
          record.get("text_pad"), 2048)


def test_a_pre_banner_record_carries_no_pad_key_at_all(bh, tmpdir):
    """Absent, not null.

    Every other optional key in bench-history follows the same convention, for
    the same reason: an explicit `text_pad: null` is a claim that somebody
    looked, which invites a later reader to charitably treat it as a measured 0.
    """
    import io
    import json
    import contextlib

    log = write(tmpdir, "serial.txt", "\n".join([
        "[bench] SCORE syscall_dispatch 120 200 PASS 130 1000",
        "[bench] CANARY 8 8 100 8 9 12 11 0 800 900",
    ]) + "\n")
    history = os.path.join(tmpdir, "history.jsonl")
    with contextlib.redirect_stdout(io.StringIO()):
        bh.main(["--serial", log, "--history", history, "--profile", "release"])
    record = json.loads(open(history, encoding="utf-8").read().strip())
    check("no key, rather than a null one", "text_pad" in record, False)


# ---------------------------------------------------------------------------
# Layout-sensitivity calibration (scripts/layout-sweep.py -> layout_bands)
#
# The claim under test is narrow and worth stating plainly, because every test
# below is an attempt to break it in one specific direction:
#
#   A movement may be dismissed as "code placement" ONLY when this exact
#   benchmark has been directly measured, on this host and profile, across at
#   least three deliberate .text offsets of one clean commit, and moved at
#   least that far with no source change at all.
#
# Every clause in that sentence is load-bearing, and every one of them fails in
# the same direction when it is dropped: a band that is too easily granted, or
# too wide, dismisses a real regression. So the tests are deliberately
# lopsided -- most of them assert that some *plausible-looking* evidence is
# refused, and only two assert that a band is granted at all.
# ---------------------------------------------------------------------------

#: A benchmark suite that is identical in every sweep arm, so the only thing
#: that can differ between arms is `b0`. Nineteen entries because
#: `speed_factor` needs `MIN_SAMPLES_FOR_DRIFT` (8) ratios before it will
#: report one at all -- with fewer, every arm would be uncorrectable and every
#: group voided, and the band tests would pass for the wrong reason.
_SWEEP_STABLE = {f"b{i}": 1000 for i in range(1, 20)}


def _sweep_arms(pads, commit="xxx", host="H", profile="release", **overrides):
    """One layout-sweep record per pad. `pads` maps pad bytes -> this arm's b0.

    The arms are `experiment`-tagged exactly as `layout-sweep.py` tags them,
    which is the whole point: it is what makes them invisible to
    `comparable_records`, and therefore what a naive implementation of
    `layout_arms` would have silently skipped.
    """
    return [
        {
            "host": host,
            "profile": profile,
            "commit": commit,
            "text_pad": pad,
            "experiment": f"layout sweep: textpad={pad} (identical source)",
            "timestamp": f"2026-08-19T0{index}:00:00",
            "entries": dict(_SWEEP_STABLE, b0=value),
            **overrides,
        }
        for index, (pad, value) in enumerate(sorted(pads.items()))
    ]


def test_a_layout_band_is_the_peak_to_peak_of_the_arms(bh):
    """Three layouts of one commit, spanning 500-600ns, is a 20% band.

    Peak-to-peak over the *smallest*, not a deviation from the median, because
    of what the band is compared against: a run-over-run percentage change
    between two arbitrary layouts. The largest change placement alone can
    produce between two of them is exactly (max - min) / min, so any narrower
    summary would under-state the very quantity being used to excuse movements.
    """
    bands = bh.layout_bands(_sweep_arms({0: 500, 1024: 600, 2048: 550}),
                            "H", "release")

    check("the swept benchmark gets a band", "b0" in bands, True)
    check("...whose width is peak-to-peak over the smallest",
          round(bands["b0"][0], 6), 20.0)
    check("...and which records how many layouts it rests on",
          bands["b0"][1], 3)
    check("...and which commit was swept", bands["b0"][2], "xxx")
    # A benchmark that placement did not move must come back as a *measured*
    # zero, not as absent: absent means "unswept", which is a different verdict
    # with a different consequence downstream.
    check("a benchmark placement did not move is a measured zero, not absent",
          round(bands["b1"][0], 6), 0.0)


def test_sweep_arms_are_read_even_though_they_are_experiments(bh):
    """The guard against a calibration that cannot fire.

    Every arm of a layout sweep is `experiment`-tagged, and `comparable_records`
    -- the filter every other historical judgement in this file uses -- drops
    experiments on purpose, because a padded kernel is not a kernel any checkout
    reproduces and must never become the baseline an honest run is judged
    against. Reusing that filter here would have been the obvious thing to do
    and would have made `layout_arms` return nothing, forever, silently: a
    calibration that cannot fire, presenting as a calibration that found no
    sensitivity. This test asserts both halves, so that a later tidy-up which
    "unifies the filtering" fails here instead of going unnoticed.
    """
    arms = _sweep_arms({0: 500, 1024: 600, 2048: 550})

    check("the shared filter does refuse them (that part is correct)",
          bh.comparable_records(arms, "H", "release"), [])
    check("...but the layout calibration reads them anyway",
          list(bh.layout_arms(arms, "H", "release")), ["xxx"])


def test_two_layouts_are_not_a_band(bh):
    """Two points define an interval containing both, by construction.

    A two-point spread has no residual and no way to be wrong, so it cannot be
    shown to be unrepresentative of the layouts nobody sampled -- and this
    number's job is to *dismiss* movements, the direction where an
    over-confident estimate hides real regressions. Same reasoning, and the same
    floor of three, as `MIN_SAMPLES_FOR_POSITIONAL_MODEL`.
    """
    check("two arms yield no band",
          bh.layout_bands(_sweep_arms({0: 500, 4096: 5000}), "H", "release"),
          {})
    check("...and three of the same spread do",
          "b0" in bh.layout_bands(
              _sweep_arms({0: 500, 4096: 5000, 1024: 900}), "H", "release"),
          True)


def test_a_record_with_no_pad_is_not_counted_as_an_unpadded_arm(bh):
    """Absent `text_pad` means "placement unknown", never "placement zero".

    ~70 historical records predate the `textpad=` banner. Folding absent into 0
    would enrol every one of them into the unpadded arm of a comparison they
    were never part of -- their spread is ordinary run-to-run noise plus months
    of unrelated code change, and it would be reported as the amount placement
    alone can move a benchmark. That is a wide band manufactured out of nothing,
    in the direction that dismisses regressions.
    """
    arms = _sweep_arms({1024: 600, 2048: 550})
    legacy = dict(_sweep_arms({0: 500})[0])
    del legacy["text_pad"]

    check("a pre-banner record does not make up the third arm",
          bh.layout_bands(arms + [legacy], "H", "release"), {})


def test_a_dirty_arm_is_not_a_layout_sample(bh):
    """The construction rests on the arms sharing identical source.

    `commit` is only a statement about the source when the tree is clean. A
    dirty arm may differ from its siblings in *code*, and its contribution to
    the spread would then be attributed to placement -- which is precisely the
    confound the sweep exists to remove, reintroduced inside the instrument
    meant to measure it.
    """
    arms = (_sweep_arms({0: 500, 1024: 600})
            + _sweep_arms({2048: 5000}, dirty=True))

    check("a dirty arm does not count toward the three",
          bh.layout_bands(arms, "H", "release"), {})
    check("...and it is the dirt, not the pad, that disqualified it",
          "b0" in bh.layout_bands(
              _sweep_arms({0: 500, 1024: 600, 2048: 5000}), "H", "release"),
          True)


def test_an_uncorrectable_arm_voids_the_whole_group(bh):
    """No drift correction is worse than no band, so the group is dropped.

    The arms are separate boots minutes to hours apart, and TCG is CPU-bound, so
    whatever else the host was doing scales a whole arm at once. If one arm's
    `speed_factor` cannot be computed, the tempting fallback is an uncorrected
    1.0 -- but uncorrected host drift *inflates* the spread, and a wider band
    dismisses more movements. The convenient fallback fails in the one direction
    that hides real regressions, so the group is voided instead, which yields
    "unmeasured", under which a movement stays a regression.
    """
    arms = _sweep_arms({0: 500, 1024: 600, 2048: 550})
    # Too few benchmarks for `speed_factor` (MIN_SAMPLES_FOR_DRIFT = 8), so
    # this arm's host speed is unknowable rather than merely unusual.
    arms[2]["entries"] = {"b0": 550, "b1": 1000}

    check("one uncorrectable arm voids every band in the group",
          bh.layout_bands(arms, "H", "release"), {})


def test_a_band_is_measured_on_the_host_and_profile_that_asked(bh):
    """A sweep on another machine says nothing about this one.

    The page-straddle penalty is a property of the emulator and the build, so a
    debug-profile sweep, or one from a different host, is not evidence about a
    release run here -- and admitting it would be admitting a band nobody
    measured for the thing being judged.
    """
    arms = (_sweep_arms({0: 500, 1024: 600})
            + _sweep_arms({2048: 5000}, host="OTHER"))
    check("an arm from another host does not complete the sweep",
          bh.layout_bands(arms, "H", "release"), {})

    arms = (_sweep_arms({0: 500, 1024: 600})
            + _sweep_arms({2048: 5000}, profile="debug"))
    check("nor does one from another profile",
          bh.layout_bands(arms, "H", "release"), {})


def test_split_by_layout_excuses_only_on_positive_evidence(bh):
    """Three-way, not two-way: explained / unexplained / *unmeasured*.

    The two-way version is the bug. With no band for a benchmark there is no
    evidence either way, and the only safe reading is that nobody looked. An
    excuse granted by absence would silence the check in the ordinary case --
    the case where nothing has been swept at all -- which is the same failure as
    a check that cannot fire. Same asymmetry as `replication_verdict` and
    `MODE_UNDECIDED`: only a positively evidenced verdict may excuse a finding.
    """
    rows = [
        ("inside", 100, 110, 10.0, 10.0, None),
        ("outside", 100, 200, 100.0, 100.0, None),
        ("unswept", 100, 200, 100.0, 100.0, None),
        # Equal to the band, not merely under it: the band is a measurement of
        # what placement *did* do, so a movement of exactly that size is one
        # placement has been shown to produce.
        ("exactly", 100, 120, 20.0, 20.0, None),
        # Improvements are excused on the same evidence as regressions. A band
        # is a spread, not a direction -- the same relink that can slow a
        # benchmark by 20% can speed it up by 20%, and excusing only the
        # regressions would quietly turn layout noise into a stream of
        # "improvements" nobody made.
        ("faster", 100, 80, -20.0, -20.0, None),
    ]
    bands = {"inside": (20.0, 3, "xxx"), "outside": (20.0, 3, "xxx"),
             "exactly": (20.0, 3, "xxx"), "faster": (20.0, 3, "xxx")}
    unexplained, explained, unmeasured = bh.split_by_layout(rows, bands)

    check("a movement inside a measured band is explained",
          [r[0] for r in explained], ["inside", "exactly", "faster"])
    check("a movement past its band is not explained",
          [r[0] for r in unexplained], ["outside"])
    check("a benchmark nobody swept is unmeasured, not explained",
          [r[0] for r in unmeasured], ["unswept"])


def test_a_movement_inside_a_measured_band_stops_being_a_regression(bh):
    """End to end: the sweep withdraws a movement it can account for.

    This is the measured failure the whole mechanism exists for. Under TCG a
    loop whose backward branch crosses a guest page costs ~1.7x per iteration,
    and whether it crosses is a property of the loop's *address*; relinking --
    which every commit does -- re-rolls that for every function after the edited
    file. The resulting movement replicates perfectly on every re-run, so it
    arrived wearing the harness's strongest label. Here the same source, built
    at three offsets, is shown to move `b0` further than the commit did.
    """
    history = (_sweep_arms({0: 500, 1024: 5000, 2048: 1000})
               + _repl_history(bh, 2900, "xxx"))
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("a movement placement can account for does not fail the build",
          failed, False)
    check("...and is shown, under a heading that says what accounts for it",
          "EXPLAINED BY CODE PLACEMENT" in out, True)
    check("...and is not called a regression", "  REGRESSED (" in out, False)
    check("...and the band that excused it is quoted, with its size",
          "placement alone moves it 900% (3 layouts of xxx)" in out, True)
    # The band is three samples out of every possible offset, so it cannot
    # contain the worst pair among all of them. A reader who takes it as
    # exhaustive would clear a near-miss that nothing has cleared.
    check("...and the band is stated to be a lower bound",
          "the band is a LOWER bound" in out, True)
    check("...and the reader is told how to widen it",
          "scripts/layout-sweep.py" in out, True)

    # Same withdrawal, same band, but the diff edits code `b0` provably runs.
    # The verdict must not move -- placement still fully accounts for the
    # movement -- but the silence must, because a second explanation is now
    # live and this measurement cannot separate the two.
    loud, loud_failed = _repl_report(
        bh, history, 3000, "xxx",
        bench_subsystems={"b0": {"kernel/src/sched/"}},
        changed_files=["kernel/src/sched/rr.rs"])
    check("...and when the diff edits the benchmark's own subsystem, it says so",
          "provably runs" in loud, True)
    check("...without turning into a build failure", loud_failed, False)
    check("...and still under the same heading",
          "EXPLAINED BY CODE PLACEMENT" in loud, True)


def test_a_movement_past_its_band_is_still_a_regression(bh):
    """The other half, or the gate is indistinguishable from deleting the check.

    A dismissal that only ever dismisses is not a check. Same construction as
    above with the arms tightened to a 20% spread, so placement demonstrably
    cannot account for a ~500% movement, and the finding must survive.
    """
    history = (_sweep_arms({0: 500, 1024: 600, 2048: 550})
               + _repl_history(bh, 2900, "xxx"))
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("a movement past the measured band still fails the build",
          failed, True)
    check("...and keeps the confirmed heading", "  REGRESSED (" in out, True)
    check("...and is not filed under code placement",
          "EXPLAINED BY CODE PLACEMENT" in out, False)


def test_an_unswept_benchmark_keeps_its_regression_and_says_so(bh):
    """Unmeasured is not innocent -- but the reader must be told which it is.

    The arms here carry `b1`-`b19` and no `b0`, so a band exists for the suite
    but not for the benchmark that moved. The movement must survive, and the
    report must say the verdict rests on nobody having looked rather than on
    anybody having shown it -- otherwise "REGRESSED" reads as though placement
    had been ruled out, which is exactly the over-claim that taught readers to
    stop believing this instrument.
    """
    arms = _sweep_arms({0: 0, 1024: 0, 2048: 0})
    for arm in arms:
        arm["entries"] = dict(_SWEEP_STABLE)
    history = arms + _repl_history(bh, 2900, "xxx")
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("an unswept movement still fails the build", failed, True)
    check("...and is still called a regression", "  REGRESSED (" in out, True)
    check("...and the report says placement was not ruled out for it",
          "have no measured layout band" in out, True)
    check("...and says the verdict rests on nobody having looked",
          "not because anybody has shown it" in out, True)
    check("...and gives the command that would settle it",
          "layout-sweep.py --pads" in out, True)


def test_the_most_recent_sweep_wins_over_the_best_sampled_one(bh):
    """Recency beats arm count, and the reason is categorical, not statistical.

    More sampled layouts really is a better lower bound of the true placement
    sensitivity, so "most arms wins" is the tempting rule. It is still wrong: a
    band is evidence about the hot loops that *exist*, and a sweep of a commit
    whose code has since been rewritten is evidence about code that no longer
    runs. Preferring it lets a wide, well-sampled, obsolete band dismiss a real
    regression in today's code.

    The converse error -- a barely-above-floor recent sweep giving a band too
    narrow to excuse a genuine artifact -- fails the safe way: the movement
    stays a regression and a human looks at it.
    """
    old = _sweep_arms({0: 500, 1024: 5000, 2048: 1000, 512: 700},
                      commit="ancient")
    for arm in old:
        arm["timestamp"] = "2026-01-01T00:00:00"
    new = _sweep_arms({0: 500, 1024: 600, 2048: 550}, commit="today")
    for arm in new:
        arm["timestamp"] = "2026-08-19T12:00:00"

    # Deliberately fed oldest-last, so a rule that merely takes the final group
    # would also pass; only the ordering is under test here.
    bands = bh.layout_bands(new + old, "H", "release")

    check("the recent 3-arm sweep beats the ancient 4-arm one",
          bands["b0"][2], "today")
    check("...so the band is the narrow recent one, not the wide stale one",
          round(bands["b0"][0], 6), 20.0)
    check("...and the reader is told which commit it came from",
          "3 layouts of today" in bh.describe_layout_band(bands["b0"]), True)

    # Arm count still decides between sweeps of equal recency -- there the
    # categorical objection does not apply and more samples is simply better.
    for arm in old:
        arm["timestamp"] = "2026-08-19T12:00:00"
    tied = bh.layout_bands(new + old, "H", "release")
    check("at equal recency the better-sampled sweep wins",
          tied["b0"][2], "ancient")


def test_an_aa_pair_is_never_explained_by_placement(bh):
    """The one comparison where placement provably cannot be the cause.

    An A/A pair is the same kernel image twice, so the code sits at *identical*
    addresses in both runs and placement is the single thing that cannot differ
    between them. Filing an A/A movement under "explained by code placement" is
    a false statement about the only case that is certain.

    It also did a worse, quieter harm, and that is what this test really pins.
    Rows the layout split claims are removed from the regressed/improved lists,
    so they never reached the A/A MOVEMENT section either -- and that section
    exists to print exactly this: the spread of an A/A pair, which is this
    host's measurement noise, measured, and the single most useful number such
    a run produces. The band was silently deleting it.

    The general shape is worth recognising elsewhere: a filter that is correct
    for the ordinary case, applied to the one case whose premise it violates.
    """
    import io
    import contextlib

    history = (_sweep_arms({0: 500, 1024: 5000, 2048: 1000})
               + _repl_history(bh, 2900, "xxx"))
    previous = dict(history[-1], kernel_sha="deadbeef", commit="xxx")
    history[-1] = previous
    current = {name: (value, 10000, "OK", None, None)
               for name, value in previous["entries"].items()}
    current["b0"] = (3000, 10000, "OK", None, None)

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        failed = bh.report(
            previous, current, 25.0, records=history, host="H",
            profile="release", commit="xxx",
            this_run={"kernel_sha": "deadbeef", "commit": "xxx"})
    out = buf.getvalue()

    check("an A/A movement is not attributed to placement",
          "EXPLAINED BY CODE PLACEMENT" in out, False)
    check("...and still reaches the A/A section it belongs in",
          "A/A MOVEMENT" in out, True)
    check("...where the noise-floor spread is actually printed",
          "b0: 503ns -> 3000ns" in out, True)
    check("...and an A/A pair still cannot fail the build", failed, False)


def _bands_view(bh, tmpdir, records, profile="release"):
    """Run `--layout-bands` over a written history. -> stdout."""
    import io
    import json
    import contextlib
    import platform

    path = os.path.join(tmpdir, "history.jsonl")
    host = platform.node() or "unknown"
    with open(path, "w", encoding="utf-8") as handle:
        for record in records:
            handle.write(json.dumps(dict(record, host=host)) + "\n")
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.cmd_layout_bands(path, profile)
    return buf.getvalue()


def test_the_bands_view_distinguishes_unswept_from_insensitive(bh, tmpdir):
    """Silence means two opposite things, and the reader must be told which.

    A band is otherwise only visible as a side effect of a comparison, and only
    for benchmarks that moved far enough to be listed. So "swept, and this
    benchmark barely cares about placement" and "never swept, nothing is known"
    both present as nothing at all -- while calling for opposite responses.
    """
    empty = _bands_view(bh, tmpdir, _repl_history(bh, 500, "xxx"))
    check("an unswept history says so in as many words",
          "no sweep has ever been run" in empty, True)
    check("...and names the command that would fix it",
          "layout-sweep.py --pads" in empty, True)
    check("...and does not let the reader read silence as zero sensitivity",
          "judged as code because nobody has measured otherwise" in empty, True)

    swept = _bands_view(bh, tmpdir, _sweep_arms({0: 500, 1024: 600, 2048: 550}))
    check("a swept history reports the band as a number",
          "20.0%" in swept, True)
    check("...and a benchmark placement barely moves as a measured near-zero",
          "0.0%" in swept, True)
    check("...and says over how many layouts, of which commit",
          "3 layouts of xxx" in swept, True)
    check("...and repeats that it is a lower bound",
          "lower bounds" in swept, True)


def test_the_bands_view_says_which_kind_of_nothing_it_found(bh, tmpdir):
    """Three ways to have no band, three different actions.

    Reporting only the conclusion is how a sweep that ran but silently failed to
    qualify -- wrong profile, dirty tree, a voided arm -- would look exactly
    like a sweep nobody ever ran, and the reader would re-run the sweep they
    already have instead of fixing whatever disqualified it.
    """
    # Swept, but on the other profile.
    wrong = _bands_view(bh, tmpdir,
                        _sweep_arms({0: 500, 1024: 600, 2048: 550},
                                    profile="debug"))
    check("a sweep on another profile is not reported as 'never swept'",
          "no sweep has ever been run" in wrong, False)
    check("...it is reported as not qualifying, with the pads it does have",
          "none qualify here" in wrong and "[0, 1024, 2048]" in wrong, True)

    # Swept, qualifying, but one arm cannot be drift-corrected.
    arms = _sweep_arms({0: 500, 1024: 600, 2048: 550})
    arms[2]["entries"] = {"b0": 550, "b1": 1000}
    voided = _bands_view(bh, tmpdir, arms)
    check("a voided group is not reported as 'never swept'",
          "no sweep has ever been run" in voided, False)
    check("...it says the arms exist and the band was voided",
          "every band was voided" in voided, True)


def test_no_sweep_at_all_changes_nothing(bh):
    """The ordinary case: no arms, no bands, and every finding stands.

    Almost every history in this repo has never been swept. If the absence of a
    sweep altered any verdict, the mechanism would have silently rewritten the
    judgement of every run that predates it.
    """
    history = _repl_history(bh, 2900, "xxx")
    out, failed = _repl_report(bh, history, 3000, "xxx")

    check("no sweep leaves the regression intact", failed, True)
    check("...and prints no placement heading",
          "EXPLAINED BY CODE PLACEMENT" in out, False)
    check("...and no band is computed from an unswept history",
          bh.layout_bands(history, "H", "release"), {})


def _shift_with_sweep(bh, sweep, shift_to=3000, kernel_sha=None):
    """A history holding BOTH a persisted level shift in `v` and a layout sweep.

    The two halves must coexist in one list because that is how they arrive in
    reality -- `report` is handed a single history and has to draw the shift
    from the honest runs and the band from the sweep arms. The sweep arms carry
    `experiment`, so `comparable_records` keeps them out of the shift's window
    while `layout_arms` deliberately reads them; a helper that built the two
    halves separately would never exercise that separation.

    Nine stable companions per record, not the six `_shift_history` uses, for
    the reason that helper gives *plus* one it does not need: with only the
    benchmark under test present, `speed_factor` *is* that benchmark's own
    ratio and every correction cancels the very thing being measured -- and
    below `MIN_SAMPLES_FOR_DRIFT` benchmarks it declines to answer at all,
    which `level_shifts` absorbs as a 1.0 fallback but `layout_bands`
    deliberately treats as "no band". Seven benchmarks therefore produce a
    silently unswept history, and every layout assertion here would pass by
    measuring nothing.
    """
    stable = {f"stable{i}": 1000 for i in range(9)}
    history = []
    for value in [1000] * 8 + [shift_to, shift_to]:
        record = {"host": "H", "profile": "release",
                  "entries": dict(stable, v=value)}
        if kernel_sha:
            record["kernel_sha"] = kernel_sha
            record["commit"] = "aaa"
        history.append(record)
    history += [
        {"host": "H", "profile": "release", "commit": "sweptcommit",
         "text_pad": pad, "experiment": f"layout sweep: textpad={pad}",
         "timestamp": f"2026-08-19T0{index}:00:00",
         "entries": dict(stable, v=value)}
        for index, (pad, value) in enumerate(sorted(sweep.items()))
    ]
    return history


def _shift_report(bh, history, shift_to=3000, this_run=None,
                  changed_files=None, bench_subsystems=None):
    """Run `report` over a `_shift_with_sweep` history. -> (stdout, failed)."""
    import io, contextlib
    previous = history[9]
    current = {name: (value, 10000, "OK", None, None)
               for name, value in previous["entries"].items()}
    current["v"] = (shift_to, 10000, "OK", None, None)
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        failed = bh.report(previous, current, 25.0, records=history,
                           host="H", profile="release",
                           this_run=this_run or {"kernel_sha": "now"},
                           changed_files=changed_files,
                           bench_subsystems=bench_subsystems)
    return buf.getvalue(), failed


def test_a_sustained_shift_inside_a_measured_layout_band_is_not_bisectable(bh):
    """The second path to a build failure, closed the same way as the first.

    `level_shifts` is wired into `--fail-on-regression` independently of the
    run-over-run lists, and the layout band was not consulted there at all. The
    gap is visible in that function's own reasoning: "host disturbance is
    random per run, while a code regression is in every run after the commit."
    True, and incomplete -- a code *placement* artifact is also in every run
    after the commit, because the addresses are a property of the image and
    every re-run of that image reproduces them exactly. So persistence
    separates {code OR layout} from host noise and cannot say which it holds.

    `mode_structure` does not close this. It needs the benchmark to have been
    *seen* at both modes across binaries in the recorded history, so the first
    commit whose relink lands a hot loop across a page boundary is
    MODE_UNDECIDED -- and MODE_UNDECIDED fails the build, correctly, on the
    evidence it has. A sweep knows the same thing before the artifact has ever
    repeated, which is the whole reason to run one.
    """
    wide = _shift_with_sweep(bh, {0: 1000, 1024: 4000, 2048: 2000})
    out, failed = _shift_report(bh, wide)
    check("the sustained shift is still reported", "SUSTAINED SHIFT" in out,
          True)
    check("...and named as placement, not as a code finding",
          "NOT a code finding: code placement alone covers this shift" in out,
          True)
    check("...and the band's provenance is printed with it",
          "3 layouts of sweptcommit" in out, True)
    check("...so it does not fail the build", failed, False)
    check("...and the reader is told the band is a lower bound",
          "LOWER" in out and "not 'proven harmless'" in out, True)
    # The advice reads "rule out code layout before reading the diff". Under a
    # shift the sweep has *already* attributed to layout it would send the
    # reader to establish what the line above it just established.
    check("...and is not told to go and rule out what was just ruled out",
          "straddle-check.py --compare" in out, False)

    # Same shift, same everything, a band too narrow to cover it.
    narrow = _shift_with_sweep(bh, {0: 1000, 1024: 1100, 2048: 1050})
    out, failed = _shift_report(bh, narrow)
    check("a shift larger than its measured band is not explained",
          "NOT a code finding" in out, False)
    check("...and still fails the build", failed, True)
    check("...and the straddle-check advice is printed for it",
          "straddle-check.py --compare" in out, True)

    # And the ordinary case: nothing swept, nothing excused.
    unswept = _shift_with_sweep(bh, {})
    out, failed = _shift_report(bh, unswept)
    check("an unswept benchmark keeps its sustained shift",
          "NOT a code finding" in out, False)
    check("...and it still fails the build", failed, True)


def test_a_sustained_shift_across_one_fixed_image_is_never_placement(bh):
    """The A/A mistake in its other costume, refused before it can be made.

    Placement can only explain a shift if placement could have *changed* across
    the runs the shift is drawn from. When every one of them is provably the
    same kernel image, the addresses are identical throughout and layout is the
    one hypothesis ruled out by arithmetic rather than weighed against the
    numbers -- exactly as in an A/A pair. A band wide enough to cover the shift
    must not excuse it here, however well measured it is.

    What such a series actually is, for the record: a host-level change
    (thermal, background load) that persisted. Reporting it is right; calling
    it code placement would be a false statement.
    """
    fixed = _shift_with_sweep(bh, {0: 1000, 1024: 4000, 2048: 2000},
                              kernel_sha="frozen")
    out, failed = _shift_report(bh, fixed, this_run={"kernel_sha": "frozen",
                                                     "commit": "aaa"})
    check("a band that would otherwise cover it does not excuse it",
          "NOT a code finding" in out, False)
    check("...and the shift still fails the build", failed, True)
    check("...and the run-over-run half still sees the A/A pair",
          "A/A COMPARISON" in out, True)

    # The predicate itself, at its two ends.
    check("all-one-image is proof placement could not differ",
          bh.placement_is_constant(fixed, "H", "release",
                                   {"kernel_sha": "frozen"}), True)
    unknown = _shift_with_sweep(bh, {0: 1000, 1024: 4000, 2048: 2000})
    check("...and unidentifiable runs are NOT proof of that",
          bh.placement_is_constant(unknown, "H", "release",
                                   {"kernel_sha": "now"}), False)
    check("...nor is a run that cannot name its own image",
          bh.placement_is_constant(fixed, "H", "release", None), False)


def test_the_layout_veto_reads_the_same_window_the_shift_was_drawn_from(bh):
    """`level_shift_window` has two callers and they must not drift apart.

    A veto computed over a different window than the finding it vetoes is a
    check that appears to fire on the evidence and does not -- this file's
    recurring failure in yet another form. The windows are asserted
    behaviourally rather than by re-deriving the slice arithmetic, which would
    only restate the implementation.
    """
    history = _shift_with_sweep(bh, {})
    reference, recent = bh.level_shift_window(history, "H", "release")
    honest = [r for r in history if "text_pad" not in r]

    check("the corroborating runs are the newest honest ones",
          [id(r) for r in recent],
          [id(r) for r in honest[-bh.LEVEL_SHIFT_PERSIST:]])
    check("the baseline and the corroboration share no run",
          set(map(id, reference)) & set(map(id, recent)), set())
    check("no sweep arm is ever part of either window",
          any("text_pad" in r for r in reference + recent), False)

    current = dict(honest[-1]["entries"])
    check("the shift is found to begin with",
          [r[0] for r in bh.level_shifts(history, "H", "release", current)],
          ["v"])

    # Put one corroborating run back at baseline: the finding must vanish.
    recent[0]["entries"] = dict(recent[0]["entries"], v=1000)
    check("a run inside the corroboration window decides the verdict",
          bh.level_shifts(history, "H", "release", current), [])
    recent[0]["entries"] = dict(recent[0]["entries"], v=3000)

    # The one run in neither window must not: that buffer is why
    # LEVEL_SHIFT_PERSIST < LEVEL_SHIFT_SKIP.
    buffer_run = honest[-bh.LEVEL_SHIFT_SKIP]
    check("the buffer run really is in neither window",
          id(buffer_run) in set(map(id, reference + recent)), False)
    buffer_run["entries"] = dict(buffer_run["entries"], v=99999)
    check("...and changing it changes nothing",
          [r[0] for r in bh.level_shifts(history, "H", "release", current)],
          ["v"])


def test_the_benchmark_subsystem_map_is_read_out_of_bench_rs(bh, tmpdir):
    """Derived from the source, over-attributing, and never a denylist.

    Three properties, each of which is the safe direction of a choice that
    could have gone the other way:

    * The module list comes from `os.listdir` of `kernel/src`, not a table. A
      table is a second place the truth lives, and when a module is renamed it
      does not fail -- it silently stops recognising the module, which reads
      exactly like a benchmark that touches nothing.
    * `core::`, `u64::` and every other non-module `x::` fall out because they
      are not in that listing. No denylist to keep current.
    * A body that merely *mentions* a module counts. That over-attributes, and
      over-attribution costs a human one look; the opposite costs a missed
      escalation nobody ever hears about.
    """
    import os as _os
    src = _os.path.join(tmpdir, "kernel", "src")
    _os.makedirs(_os.path.join(src, "sched"))
    _os.makedirs(_os.path.join(src, "mm"))
    for leaf in ("lib.rs", "tlb.rs"):
        open(_os.path.join(src, leaf), "w").close()
    bench_rs = _os.path.join(tmpdir, "bench.rs")
    with open(bench_rs, "w", encoding="utf-8") as handle:
        handle.write(
            "fn bench_pick_next() {\n"
            "    let t = sched::spawn();\n"
            "    let n = core::hint::black_box(t);\n"
            "    score(\"pick_next\", n);\n"
            "}\n"
            "pub unsafe fn bench_fault() {\n"
            "    mm::map(); tlb::flush();\n"
            "    score(\"page_fault\", 1);\n"
            "    score(\"page_fault_cow\", 2);\n"
            "}\n"
            "fn helper_with_no_score() {\n"
            "    sched::yield_now();\n"
            "}\n")

    got = bh.benchmark_subsystems(bench_rs, src)
    check("a benchmark is attributed to the module its body calls",
          got.get("pick_next"), {"kernel/src/sched/", "kernel/src/bench.rs"})
    check("...including every module it mentions, not just the first",
          got.get("page_fault"),
          {"kernel/src/mm/", "kernel/src/tlb.rs", "kernel/src/bench.rs"})
    check("two scores in one body share that body's attribution",
          got.get("page_fault_cow"), got.get("page_fault"))
    check("`core::` is not a kernel module and does not become one",
          any("core" in path for paths in got.values() for path in paths),
          False)
    check("a function that scores nothing is not a benchmark",
          "helper_with_no_score" in got, False)
    check("the harness itself reaches every benchmark by definition",
          all("kernel/src/bench.rs" in paths for paths in got.values()), True)

    # And against the real tree, because a map that is correct on a fixture and
    # empty on `kernel/src/bench.rs` is a check that cannot fire.
    real = bh.benchmark_subsystems()
    check("the real bench.rs yields a non-trivial map", len(real) > 30, True)
    check("...and pick_next still lands in the scheduler",
          "kernel/src/sched/" in real.get("pick_next", set()), True)


def test_a_layout_excused_movement_says_so_when_the_diff_edits_its_subsystem(bh):
    """The hole in the layout excuse: a wide band can cover a real regression.

    A measured band says "two builds of *identical source* moved this
    benchmark at least this far". That is a true statement about placement and
    it never becomes false. What it is not is a statement about the diff -- so
    when the diff also edits code the benchmark provably runs, placement and
    the diff are both live explanations and the measurement cannot separate
    them. Withdrawing the movement is still right; withdrawing it in silence
    is not, because the reader is never told the second explanation exists.

    The escalation is asserted to be exactly that -- prose. `failed` must not
    change, or this would have become a way for a *diff* to cause a build
    failure that the evidence does not support.
    """
    wide = _shift_with_sweep(bh, {0: 1000, 1024: 4000, 2048: 2000})
    subsystems = {"v": {"kernel/src/sched/", "kernel/src/bench.rs"}}

    quiet, quiet_failed = _shift_report(bh, wide, bench_subsystems=subsystems,
                                        changed_files=["kernel/src/mm/vma.rs"])
    check("a diff elsewhere leaves the excuse unannotated",
          "provably runs" in quiet, False)
    check("...and does not fail the build", quiet_failed, False)

    loud, loud_failed = _shift_report(
        bh, wide, bench_subsystems=subsystems,
        changed_files=["kernel/src/sched/rr.rs", "docs/notes.md"])
    check("a diff inside the benchmark's own subsystem is called out",
          "provably runs" in loud, True)
    check("...naming the file, so the reader knows where to look",
          "kernel/src/sched/rr.rs" in loud, True)
    check("...and saying the two explanations cannot be told apart",
          "BOTH live explanations" in loud and "them apart" in loud, True)
    check("...while the unrelated file stays out of it",
          "docs/notes.md" in loud, False)
    check("...and it is still not counted as a regression",
          loud_failed, False)
    check("...and the movement is still withdrawn as placement",
          "NOT a code finding" in loud, True)


def test_the_diff_escalation_can_only_ever_escalate(bh):
    """Every way of not knowing must produce silence, never an excuse.

    This is the direction-of-failure argument that made the whole feature
    tractable. `known-issues.md`'s standing fix is phrased "label it MOVED
    *unless* the changed files plausibly reach the benchmark", which needs
    proof of NON-reachability -- and nothing here can supply that, because the
    map cannot see through helpers, trait objects, inlining or LTO. Phrased as
    an escalation instead, every unknown resolves to "said nothing", and the
    movement keeps whatever verdict the measurement gave it.
    """
    check("no map at all escalates nothing",
          bh.diff_touches("v", None, ["kernel/src/sched/rr.rs"]), [])
    check("no diff at all escalates nothing",
          bh.diff_touches("v", {"v": {"kernel/src/sched/"}}, None), [])
    check("an unmapped benchmark escalates nothing",
          bh.diff_touches("other", {"v": {"kernel/src/sched/"}},
                          ["kernel/src/sched/rr.rs"]), [])
    check("a directory does not match a sibling that shares its prefix",
          bh.diff_touches("v", {"v": {"kernel/src/sched/"}},
                          ["kernel/src/scheduler_notes.md"]), [])
    check("a file entry matches only itself",
          bh.diff_touches("v", {"v": {"kernel/src/tlb.rs"}},
                          ["kernel/src/tlb.rs", "kernel/src/tlbx.rs"]),
          ["kernel/src/tlb.rs"])
    check("windows separators in git output do not defeat the match",
          bh.diff_touches("v", {"v": {"kernel/src/sched/"}},
                          ["kernel\\src\\sched\\rr.rs"]),
          ["kernel/src/sched/rr.rs"])
    check("describe_diff_touch is silent when there is nothing to say",
          bh.describe_diff_touch("v", {"v": {"kernel/src/sched/"}}, []), None)

    # `changed_paths` must distinguish "nothing changed" from "never asked".
    check("no base commit is not-established, not an empty diff",
          bh.changed_paths(None), None)
    check("an unknown commit is not-established either",
          bh.changed_paths("0" * 40), None)
    check("...and so is the placeholder the recorder writes",
          bh.changed_paths(bh.UNKNOWN_COMMIT), None)


def main():
    """Run every `test_*` in this file, in definition order.

    # Why discovery rather than a hand-written call list

    There used to be one: eighteen explicit calls, one per test. A test added
    without a matching line ran zero times and reported nothing -- silently, and
    indistinguishably from a test that passed. That is the exact failure this
    whole suite exists to catch one level down (a canary that measured zero and
    certified nine runs clean), reproduced in the harness meant to catch it.
    Discovery removes the second place a test has to be listed, so forgetting is
    no longer possible.

    Signatures are dispatched by parameter name: `bh` gets the loaded module,
    `tmpdir` a fresh temporary directory. Each test gets its *own* tmpdir rather
    than a shared one, so a stray filename collision between two tests cannot
    make one depend on another's leftovers. Module dicts preserve insertion
    order, so the run order still matches the file.
    """
    bh = load_module()
    tests = [
        (name, fn) for name, fn in list(globals().items())
        if name.startswith("test_") and callable(fn)
    ]
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes, which is the bug this docstring is about. Assert a floor.
    if len(tests) < 102:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 102. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        with tempfile.TemporaryDirectory() as tmpdir:
            args = {"bh": bh, "tmpdir": tmpdir}
            fn(**{p: args[p] for p in params if p in args})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print("all bench-history tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
