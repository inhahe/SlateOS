#!/usr/bin/env python3
"""Hold the host CPU busy across a *named window* of a `--bench` run.

Driven by `canary-load-test.sh`; not normally run by hand.

Why this exists as a separate program
-------------------------------------
The first attempt at the positional experiment (prediction P22) applied the
load from shell: a `sleep 1` polling loop watched the serial log for the
trigger benchmark's line, and on seeing it spawned six `python -c "while
True: pass"` processes.  The run graded FAILED, and the graded window showed
a median inflation of **x1.00** while the benchmarks *after* it showed
**x1.44** -- so the load demonstrably arrived somewhere other than where it
was labelled.  The model had found the real disturbance; the label was wrong.

Two costs made that inevitable, and both are structural rather than bad luck:

* **Activation latency.**  The whole benchmark suite is a short tail of the
  boot -- about 6 s of measured in-guest work, in ~350 serial lines out of
  ~27,800.  An interior window is therefore a couple of *seconds* wide.
  Against that, a 1 s poll interval plus the time to fork six interpreters
  from MSYS is not a rounding error; it is most of the window.
* **Measuring with the instrument you are perturbing.**  RESULT P19 says
  plainly that a single `grep` during the QEMU window moves the reference
  cost by tens of percent.  A polling loop that spawns a process per second
  is doing exactly that, for the whole window, on the run whose reference
  cost *is* the measurement.

Both are fixed by paying every cost up front:

* the spinners are **spawned before QEMU starts** and block on a semaphore,
  so switching the load on is a kernel wakeup (microseconds) rather than six
  process creations, and switching it off is `Event.set()` rather than six
  kills;
* the log is followed **inside this process** on a held-open descriptor, so
  the whole window costs one file handle and ten small reads per second and
  spawns nothing at all.

What it records, and why that matters more than what it does
------------------------------------------------------------
This program cannot prove it applied the load where it claims -- believing
that claim is what produced the bogus FAILED.  It therefore writes a JSON
record of what actually happened (when the load went on and off in wall
time, how many benchmark result lines went past while it was on, how that
compares with the suite as a whole).  That record is *diagnostic*: it is what
you read to see that a window was only 0.4 s wide, or that the trigger never
fired.  The **ground truth is established independently**, by
`grade-positional.py`, which refuses to grade the model at all unless the
loaded benchmarks measurably slowed down relative to the untouched ones.  A
stimulus is not a stimulus because the script that applied it says so.

The trigger names live in a different namespace from the scorecard
------------------------------------------------------------------
The second attempt was void too, and for a reason worth stating loudly: the
kernel prints a benchmark's *live* result line under one name and its
end-of-run SCORE line under another.  They agree for 59 of the suite's 86
benchmarks and disagree for the other 27 -- the live line says
`vfs_write_16k`, the scorecard says `vfs_throughput_16k_write`; the live line
says `isr_hard_irq`, the scorecard says `isr_latency`.  There are also 11 live
lines with no scorecard entry at all (97 live lines, 86 scored benchmarks).

`--at` and `--until` match **live** lines, but a human reads the window's
bounds off the **scorecard**.  Naming a scorecard-only benchmark therefore
produced a trigger that could never fire, and the run went to completion
reporting success: `--until vfs_throughput_16k_write` matched nothing, the
load was never released, and it ran to the end of the boot while the record
said `released: false` in a field nobody had been asked to check.

Two guards, because one of them is not enough:

* **Up front.**  `--known-names` points at the list of live names the previous
  run actually saw.  A name absent from it is rejected before a single
  spinner is spawned, with the closest matches printed -- so the cost of the
  mistake is a second, not a 2.5-minute boot and a void experiment.  The file
  is rewritten from every run, so it maintains itself.
* **At the end.**  A `--until` that never matched is now a **failure exit**,
  not a footnote in the JSON.  A window with no right-hand edge is not a
  narrower experiment than the one requested; it is a different one.

What the host-side witness is for
---------------------------------
Both void runs turned on the same question -- *when did the load actually
become effective?* -- and neither could answer it, because nothing recorded
the host's own state over time.  The serial log carries no timestamps, so
"the load was on while benchmarks 60-72 ran" was an inference from two
numbers the controller happened to print.

So the controller now keeps a **host canary**, deliberately the same idea as
the guest's: a fixed, tiny amount of CPU work, re-timed on a fixed interval,
recorded with a timestamp.  When the host is saturated the same work takes
longer, so the samples say *when* the host was contended, measured on the
host, independently of both the guest's canary (the instrument under test)
and this script's own beliefs.  Every benchmark completion is timestamped on
the same clock, so the two can be joined afterwards: for each benchmark, was
the host busy while it ran.  That is the ground truth the first two attempts
were missing, and it is what makes an unexpected result diagnosable rather
than merely disappointing.

Process hygiene
---------------
The spinners are `multiprocessing` children of this process.  They are killed
by object, never by name -- a blanket `pkill python` would take down the
operator's unrelated Python work.  They also defend themselves against being
orphaned: every spin chunk they re-check a hard deadline and whether this
process is still alive, so a `TerminateProcess` from MSYS (which runs no
handlers here) cannot leave six CPU burners running forever.
"""

from __future__ import annotations

import argparse
import difflib
import json
import multiprocessing
import os
import re
import statistics
import sys
import threading
import time

#: A benchmark's live result line, printed by the kernel as it finishes:
#: `[bench] name: min=... cycles (...ns), mean=...`.  Matching on `min=` and
#: not merely on the colon is what separates result lines from the prose the
#: same prefix carries (`[bench] Running self-test...`, `[bench]   canary
#: scale check: OK - ...`), whose "name" would otherwise be a stray word.
BENCH_LINE_RE = re.compile(r"^\[bench\]\s+(\S+):\s+min=")

#: Result lines before this marker belong to the benchmark runner's own
#: self-test, which runs a benchmark called `self_test_nop` that is not part
#: of the suite and holds no suite position.  Triggering on it would fire the
#: load before the suite had started.
SUITE_MARKER = "=== Kernel micro-benchmarks ==="

#: How often the serial log is checked, in seconds.  This is the activation
#: latency: the load starts up to this long after the trigger benchmark
#: finished.  0.1 s against a window of a few seconds is a few percent, and
#: ten small reads per second is far below the noise floor that RESULT P19
#: warns about (which was about *spawning* processes, not reading bytes).
POLL_SECONDS = 0.1

#: Iterations of the empty loop between a spinner's liveness checks.  Large
#: enough that the checks are not themselves the workload, small enough that
#: a stop request is honoured in well under a benchmark's runtime.
SPIN_CHUNK = 200_000

#: How often the host canary re-times its fixed unit of work, in seconds.
#: The benchmarks this experiment cares about occupy a few seconds, so 0.05 s
#: puts tens of samples inside the window -- enough to say when the load
#: became effective to within a benchmark or two, which is the resolution the
#: whole positional question is asked at.
HOST_PROBE_INTERVAL = 0.05

#: Iterations of the host canary's unit of work.  Chosen to take well under a
#: millisecond on an idle host: large enough to be timed reliably against a
#: ~15 ms Windows clock granularity when it inflates, small enough that the
#: probe itself is a negligible fraction of one core.  It measures *CPU
#: availability* rather than wakeup latency deliberately -- that is what
#: QEMU's vCPU thread is actually competing for.
HOST_PROBE_WORK = 20_000


def _host_probe(samples, stop_probe, origin):
    """Re-time a fixed unit of CPU work, and record when it got slower.

    The host-side twin of the guest's canary, and it exists for the same
    reason: a number that should never change, so that when it does change
    the change is the finding.  Runs in a thread rather than a process
    because it must share this process's clock origin, and because a thread
    that spends almost all its time asleep cannot meaningfully perturb the
    measurement -- unlike spawning something, which RESULT P19 shows plainly
    that it would.

    Appends `(seconds_since_origin, seconds_the_work_took)`.  `samples` is a
    plain list: CPython list append is atomic under the GIL, and the reader
    only looks at it once this thread has been joined.
    """
    while not stop_probe.is_set():
        start = time.monotonic()
        for _ in range(HOST_PROBE_WORK):
            pass
        end = time.monotonic()
        samples.append((round(start - origin, 4), round(end - start, 6)))
        # Sleep the remainder of the interval, so the probe's duty cycle stays
        # constant as the work inflates: a probe that ran back-to-back under
        # load would itself become part of the load it is measuring.
        slack = HOST_PROBE_INTERVAL - (end - start)
        if slack > 0:
            stop_probe.wait(slack)


#: Fraction of a spinner's theoretical CPU time below which the load is not
#: credibly "applied".  A spinner that had a core to itself for the whole
#: window scores 1.0; one that was never scheduled scores 0.0.  The host has
#: more cores than this experiment uses spinners, and QEMU's own threads are
#: the only other serious competitor, so a healthy run sits near 1.0 -- run 3
#: measured well above this bar.  The threshold is set low deliberately: its
#: job is to catch spinners that did not run *at all* (the failure mode that
#: silently voided an earlier standalone probe), not to police scheduling
#: jitter, and a tight bar here would reject good runs on a busy desktop.
OCCUPANCY_FLOOR = 0.5

#: Granularity of the per-process CPU clock on Windows -- the scheduler's
#: 64 Hz tick.  A spinner's `process_time` therefore lands on a ~15.6 ms grid,
#: so a *perfectly* consistent measurement can still read slightly above 1.0.
#: This is a property of the clock, not a fudge factor: it is used to derive
#: the ceiling from the span actually measured (see `occupancy_ceiling`),
#: rather than to justify a hardcoded tolerance.
CPU_CLOCK_GRANULARITY_S = 0.0156


def occupancy_ceiling(span_seconds):
    """The highest `occupancy_measured` a *self-consistent* run can report.

    No process burns more CPU than the interval it was measured over, times
    the cores it had, so the true ceiling is 1.0.  The clock only resolves
    ~15.6 ms, so a short span can round up: over 0.3 s that is 5%, over 3 s
    it is 0.5%.  Deriving the bound from the span is the point -- a fixed
    tolerance is either too tight for a short span or too loose for a long
    one, and the loose end is what let a systematically-biased ratio pass
    unnoticed for as long as the host stayed quiet.

    The bound rests on one invariant the caller must uphold: **the span must
    be the interval the numerator covers, not an interval around it.**  Then
    tick quantisation is the only error left, and one grid step is genuinely
    all of it -- the user and kernel counters share one 64 Hz sampler, so
    their sum advances at most once per tick event, and a span of `S` seconds
    contains at most `ceil(S/g)` such events.  With `n` spinners the worst
    case is `n` steps over a denominator of `n x S`, i.e. the same `g / S`
    regardless of `n`.

    That invariant is easy to break by accident and its breach is invisible
    here, because a numerator that outruns its denominator looks exactly like
    a clock that rounded up.  It *was* broken, for as long as this function
    had existed: the spinners published a bare CPU number and the controller
    divided it by an interval it stamped itself, so the ratio's two halves
    began at different instants -- the numerator wherever a parked spinner
    had last published, the denominator at the controller's stamp.  A 64 Hz
    sampler charges a whole 15.6 ms tick to whoever it catches, so a single
    tick landing in that gap was enough to put a 0.2 s window over the bound.
    It refused a boot test at 1.089 on a loaded host, and a 10-run probe then
    caught it at 1.009 on an *idle* one -- both impossible for a
    single-threaded process, which is what said the fault was in the ratio
    rather than in the load.

    The fix is in the publication, not here.  Widening this bound to cover a
    mismeasured span would have retired the only check that noticed, which is
    the same mistake as the hardcoded 2.0 that let 1.82 cores per spinner
    through.  Two things were measured and neither is the cause, so neither
    was patched around: the controller's snapshot reads take 0-20 us, and the
    stamping order they were suspected of is now merely correct rather than
    load-bearing.
    """
    if not span_seconds or span_seconds <= 0:
        return None
    return 1.0 + CPU_CLOCK_GRANULARITY_S / span_seconds


def summarise_occupancy(before, after, window_seconds, barrier_wait=None):
    """How much CPU the spinners actually burned across the load window.

    This is the canary's real instrument, and it is a direct measurement
    rather than an inference.  `before` and `after` are snapshots of the
    spinners' own `process_time` clocks taken at fire and at release; the
    difference is CPU-seconds genuinely consumed.  Divided by the wall time
    they were supposed to be consuming it for, it yields an occupancy per
    spinner: 1.0 means "had a core throughout", 0.0 means "never ran".

    Unlike the timing probe this needs no baseline, cannot be confounded by
    what else the host happens to be doing, and does not care how many cores
    the machine has.  RESULT P22 is why it exists: the probe's median ratio
    has a run-to-run spread of 1.95x on this host and reported x0.78 -- the
    host apparently *speeding up* under load -- for a run whose load was in
    fact applied correctly.

    ## Two denominators, because there are two questions

    `window_seconds` is the window the caller *asked for*; `span_s` is the
    interval each spinner's own clock says its CPU figure covers.  They are
    deliberately not the same, and the difference is not an error to be tuned
    away:

    - The opening snapshot is the pair a spinner publishes after it observes
      `go` and before it burns a chunk, so no pre-window burn can land in the
      window's opening balance.
    - The closing one is the pair it publishes after observing `stop`, because
      truncating earlier would lose its final chunk.

    So the numerator's interval strictly *contains* `[on_at, off_at]`, and
    `occupancy` is biased upward by however long the two ends take.  On a
    quiet host that is microseconds and invisible.  Under load it is not: the
    spinners keep burning until they next observe `stop`, and every one of
    those CPU-seconds lands in a numerator whose denominator already closed.
    On 2026-08-31 that produced `occupancy 2.036` and failed a boot test
    before it built anything -- the ratio was not wrong about the load, it was
    dividing by the wrong interval.

    `span_s` comes from the spinner and not from the controller, and that is
    the whole reason the ceiling means anything.  A controller-stamped span is
    an interval *around* the numerator, not the numerator's own, and the two
    diverge by however long the spinner had been parked since it last spoke --
    silently, and always in the direction of a higher occupancy.

    Both are therefore reported, and they answer different questions:

    | field | denominator | answers |
    |---|---|---|
    | `occupancy` | the requested window | *was the load applied across the window I asked for?* -- the floor check |
    | `occupancy_measured` | the measured span | *is this measurement self-consistent?* -- the ceiling check |

    Only the second has a physical ceiling (see `occupancy_ceiling`); the
    first legitimately exceeds 1.0 whenever the ends are slow, which is
    exactly when it is least safe to hardcode a tolerance for it.
    """
    if not before or not after or len(before) != len(after):
        return None
    burned = [max(0.0, b[0] - a[0]) for a, b in zip(before, after)]
    # Each spinner's own elapsed time, from the clock reading it published
    # *with* the CPU figure above.  This is the denominator that makes the
    # ceiling a bound: it is the interval the numerator covers, not an
    # interval the controller timed around it.
    elapsed = [max(0.0, b[1] - a[1]) for a, b in zip(before, after)]
    total = sum(burned)
    # Wall time is the denominator per spinner, so the ideal total is
    # spinners x window.  A zero-length window would make the ratio
    # meaningless rather than infinite, so it is reported as unavailable.
    expected = len(burned) * window_seconds if window_seconds else None
    measured = sum(elapsed) or None
    # Reported as the mean so `span_s` stays comparable with `window_s`, which
    # is per-spinner; the ratio above uses the sum, which is the same thing.
    span_seconds = (measured / len(elapsed)) if measured else None
    ceiling = occupancy_ceiling(span_seconds)
    return {
        "spinners": len(burned),
        "window_s": round(window_seconds, 4) if window_seconds else None,
        # Absent, not equal to `window_s`, when the spinners published no
        # clock alongside their CPU: a reader must be able to tell "this run
        # measured the span" from "this run assumed it".
        "span_s": round(span_seconds, 4) if span_seconds else None,
        # Per spinner, so a single straggler is visible rather than averaged
        # away.  A spread here that `window_s` does not show is the signature
        # of one spinner being slow off the mark at an edge.
        "span_each_s": [round(v, 4) for v in elapsed],
        # How long each edge barrier waited, fire then release.  This is the
        # only cost the barriers add, and it is time the window's left edge
        # was delayed by, so it belongs in the record next to the window.
        "barrier_wait_s": ([round(v, 5) if v is not None else None
                            for v in barrier_wait]
                           if barrier_wait else None),
        "cpu_seconds": [round(v, 4) for v in burned],
        "cpu_seconds_total": round(total, 4),
        "expected_cpu_seconds": round(expected, 4) if expected else None,
        "occupancy": round(total / expected, 3) if expected else None,
        "occupancy_measured": round(total / measured, 3) if measured else None,
        "occupancy_ceiling": round(ceiling, 3) if ceiling else None,
        "idle_spinners": sum(1 for v in burned
                             if window_seconds and v < 0.1 * window_seconds),
    }


def summarise_probe(samples, fired_at, released_at):
    """Reduce the host canary to what a reader needs to judge the stimulus.

    The ratio is what matters, not the absolute cost: the unit of work's
    idle duration is a property of this host and this Python build, and
    comparing it against anything but itself would be meaningless.

    Both a median and a best-case ratio are reported, and the *best-case* one
    is the trustworthy figure.  Measured over 12 back-to-back trials on this
    host with six spinners verifiably running throughout:

        statistic     median ratio   range           inverted (<1.0)
        median        x1.106         x0.784..x1.526   3 of 12
        min           x1.004         x0.999..x1.036   0 of 12 materially
        trimmed 25%   x1.005         x0.999..x1.130   0 of 12 materially

    The median's 1.95x spread swallows the effect it is meant to detect, and
    one trial produced x0.784 -- indistinguishable from the x0.782 that run 3
    reported and that cost a day's investigation.  A median in this position
    is not a weak measurement, it is a misleading one, so `inflation` now
    carries the best-case ratio and the median is retained only as
    `median_inflation`, explicitly labelled unreliable.

    None of these ratios should be read as confirming the load: with more
    cores than spinners the probe keeps a free core and *correctly* reports
    almost no effect.  `summarise_occupancy` is the instrument that answers
    "was the load applied"; this one only describes what the host felt.
    """
    if not samples:
        return None
    before = [d for t, d in samples if fired_at is None or t < fired_at]
    during = [d for t, d in samples
              if fired_at is not None and t >= fired_at
              and (released_at is None or t < released_at)]

    def best(v):
        """Mean of the fastest quarter: a best-case that averages over
        several observations instead of staking everything on the single
        luckiest one, which is what makes it stable without making it as
        noise-prone as the median."""
        if not v:
            return None
        return statistics.fmean(sorted(v)[:max(1, len(v) // 4)])

    idle = statistics.median(before) if before else None
    busy = statistics.median(during) if during else None
    idle_best, busy_best = best(before), best(during)
    return {
        "samples": len(samples),
        "idle_median_s": round(idle, 6) if idle is not None else None,
        "loaded_median_s": round(busy, 6) if busy is not None else None,
        "idle_best_s": round(idle_best, 6) if idle_best is not None else None,
        "loaded_best_s": round(busy_best, 6) if busy_best is not None else None,
        # None rather than 1.0 when either side is missing: "the load made no
        # difference" and "there was nothing to compare" are different
        # statements, and only one of them is evidence.
        "inflation": round(busy_best / idle_best, 3)
                     if (idle_best and busy_best) else None,
        "median_inflation": round(busy / idle, 3)
                            if (idle and busy) else None,
        "median_inflation_note": "unreliable: 1.95x run-to-run spread on this"
                                 " host; see RESULT P22",
    }


#: How long the controller waits for every spinner to publish an opening
#: balance before giving up on the barrier and saying so.  Generous, because
#: the cost of waiting is nothing (it happens before the window opens) and the
#: cost of firing early is a measurement that counts interpreter startup as
#: load.
SPINNER_READY_TIMEOUT_S = 30.0

#: How long the controller waits at each edge of the load window for every
#: spinner to publish a fresh (CPU, clock) pair.  Short, because by this point
#: every spinner is a running interpreter blocked on an event, so the wait is a
#: scheduler wakeup and a chunk -- single-digit milliseconds even on a loaded
#: host.  Generous against that, because overshooting costs a few milliseconds
#: at the window's edge while undershooting costs the freshness the whole
#: pairing exists to guarantee.
SPINNER_EDGE_TIMEOUT_S = 2.0


def _spin(go, stop, deadline, cpu_slot=None, ready_count=None,
          fired_count=None, done_count=None):
    """One CPU burner: block until `go`, then loop until `stop`.

    A tight pure-Python loop with no I/O and no sleeping, which is what
    contends with TCG emulation for a core.

    `cpu_slot` is this spinner's cell in a shared array, into which it
    publishes its own accumulated CPU time.  That is the whole point of the
    parameter: whether the load was actually applied is a question about CPU
    *consumed*, and this is the only party that can answer it directly.  The
    controller's timing probe can only guess at it from the outside, and
    RESULT P22 established that on a 12-core host the guess is worthless --
    six spinners leave the probe a free core, so its median moved by less
    than its own run-to-run noise, and inverted outright in 3 trials of 12.

    The cell holds a *pair* -- CPU seconds and the `monotonic` reading taken
    with them, written under the cell's lock -- and that pairing is what makes
    the occupancy ratio meaningful rather than merely plausible.  A lone CPU
    number has to be divided by an interval the *controller* timed, and the
    controller cannot see when this process's clock was actually sampled; the
    two intervals then differ by however long this spinner had been parked
    since its last publication, and every bit of that difference is CPU in
    the numerator that the denominator does not cover.  Publishing the pair
    makes numerator and denominator the same interval by construction, which
    is the only form in which "no process burns more CPU than the time it was
    measured over" is a bound rather than an aspiration.  See
    `occupancy_ceiling`.

    `fired_count` and `done_count` are the barriers that keep those pairs
    *fresh*: the controller may not take its opening snapshot until every
    spinner has published one after seeing `go`, nor its closing one until
    every spinner has published after seeing `stop`.

    The self-defence checks matter as much as the loop.  MSYS `kill` of a
    native Windows process is `TerminateProcess`: no signal handler runs, no
    `finally` executes, and this process's parent simply vanishes.  Polling
    `parent.is_alive()` and a wall-clock deadline is the only thing standing
    between that and six orphaned processes spinning until the machine is
    rebooted.
    """
    parent = multiprocessing.parent_process()

    def publish():
        # Written on every chunk boundary, on both sides of the wait, and once
        # more on the way out.  `process_time` is this process's own CPU clock,
        # so it counts only time the spinner was actually scheduled -- which is
        # exactly the quantity in question -- and it is stored together with
        # the `monotonic` reading taken alongside it, under the cell's lock, so
        # a reader can never pair one publication's CPU with another's clock.
        #
        # The staleness of a publication no longer matters, and that is the
        # point of the pairing.  A reader that samples at an arbitrary moment
        # gets an older pair, not an inconsistent one, so the interval it
        # derives is one the CPU figure genuinely covers.  Before the pairing,
        # staleness was silently charged to the controller's window: the
        # opening snapshot was whatever a *parked* spinner had last published,
        # up to a second earlier, while the span was stamped at the read.  Any
        # CPU the parked spinner was charged in between -- and a 64 Hz sampler
        # charges a whole 15.6 ms tick to whoever it catches, however briefly
        # they ran -- landed in the numerator with no denominator to match.
        # That is how `occupancy_measured` reached 1.089 against a hard ceiling
        # of 1.078 and refused a boot test, and how it read 1.009 on an idle
        # host in a 10-run probe: not a clock that rounds, but a ratio whose
        # two halves were measuring different intervals.
        if cpu_slot is not None:
            with cpu_slot.get_lock():
                cpu_slot[0] = time.process_time()
                cpu_slot[1] = time.monotonic()

    def should_quit():
        if stop.is_set() or time.monotonic() > deadline:
            return True
        return parent is not None and not parent.is_alive()

    # Wait with a timeout rather than forever: an orphan that never receives
    # `go` would otherwise sit blocked for the life of the machine.  The
    # timeout costs one wakeup per second and does *not* add latency -- the
    # semaphore wakes immediately when `go` is set.
    # Publish *before* the window can open, and again on every wakeup.
    #
    # This loop used to publish only on its way out, which left the slot at its
    # initial 0.0 for the entire wait.  The controller snapshots `cpu_at_fire`
    # before setting `go` -- deliberately -- so it read that 0.0, while the
    # closing snapshot read a real `process_time()` that includes this
    # process's whole interpreter startup.  Every spinner's Python startup cost
    # therefore landed in the window's burn, inflating occupancy by a fixed
    # amount that a short window cannot absorb: the live test measured 1.82
    # cores per single-threaded spinner, which is not a tolerance problem but
    # an impossibility.  It went unnoticed because the bound it had to clear
    # was a hardcoded 2.0.
    #
    # Publishing here costs nothing -- the spinner is blocked in `go.wait`, so
    # there is no CPU to account for between the last publish and `go`.
    publish()
    # Only now is this spinner's startup accounted for, so only now may the
    # controller consider it ready.  Signalling before the publish would
    # reintroduce the race the barrier exists to close.
    if ready_count is not None:
        with ready_count.get_lock():
            ready_count.value += 1
    while not go.is_set():
        if should_quit():
            publish()
            return
        go.wait(1.0)
        publish()

    # The opening balance.  Published after `go` is set and before a single
    # chunk is burned, so the pair the controller reads brackets the window
    # from its true left edge rather than from wherever this process last
    # happened to speak.  The barrier below is what lets the controller know
    # it may read: without it the controller would race the publication it
    # depends on, and would lose exactly when the host is loaded enough for a
    # spinner to be slow off the mark -- which is when the measurement matters.
    publish()
    if fired_count is not None:
        with fired_count.get_lock():
            fired_count.value += 1

    while True:
        for _ in range(SPIN_CHUNK):
            pass
        publish()
        if should_quit():
            # Symmetrically, the closing balance is published before this
            # spinner announces it has stopped, so the controller's closing
            # snapshot covers the final chunk instead of truncating it.
            if done_count is not None:
                with done_count.get_lock():
                    done_count.value += 1
            return


class SerialTail:
    """Follow a growing serial log on one held-open descriptor.

    Opened lazily, because the file does not exist until QEMU creates it, and
    opening it early is not merely useless but harmful: an open handle on
    Windows makes `boot-test.sh`'s own `rm -f` of the log fail, which is the
    documented cause of a whole class of "the next boot died instantly" bugs
    (see the pidfile comment in `boot-test.sh`).  The caller deletes the log
    before the boot starts, so the only file that can appear at this path is
    the one this run's QEMU creates.
    """

    def __init__(self, path):
        self.path = path
        self._handle = None
        self._leftover = b""

    def opened(self):
        if self._handle is not None:
            return True
        if not os.path.exists(self.path):
            return False
        try:
            self._handle = open(self.path, "rb")
        except OSError:
            # Racing QEMU's creation of the file; try again next poll.
            return False
        return True

    def lines(self):
        """Every complete line appended since the last call."""
        if self._handle is None:
            return []
        chunk = self._handle.read()
        if not chunk:
            return []
        chunk = self._leftover + chunk
        # A trailing fragment is *kept*, not decoded: the log is being
        # appended to as we read it, so the line most likely to be caught
        # mid-write is the newest one -- which is exactly the trigger.
        parts = chunk.split(b"\n")
        self._leftover = parts.pop()
        return [part.decode("utf-8", errors="replace").rstrip("\r")
                for part in parts]

    def close(self):
        if self._handle is not None:
            self._handle.close()
            self._handle = None


class Watcher:
    """Turns serial-log lines into `(name, when)` benchmark completions."""

    def __init__(self):
        self.in_suite = False
        self.completions = []

    def feed(self, lines, now):
        """Record suite benchmark completions; return the names seen."""
        seen = []
        for line in lines:
            if not self.in_suite:
                if SUITE_MARKER in line:
                    self.in_suite = True
                continue
            match = BENCH_LINE_RE.match(line)
            if match:
                seen.append(match.group(1))
                self.completions.append((match.group(1), now))
        return seen


def run(args):
    """Apply the load and return the record dict. Pure of argument parsing."""
    ctx = multiprocessing.get_context("spawn")
    go = ctx.Event()
    stop = ctx.Event()
    started = time.monotonic()
    deadline = started + args.timeout + args.grace

    # One two-element cell per spinner: (CPU seconds, the `monotonic` reading
    # taken with it).  One cell *per spinner* rather than one shared array,
    # because slicing a ctypes array yields a *copy*, so handing a spinner
    # `arr[i:i+2]` would give it a private list to write into and the
    # controller would read zeros forever -- a silent null result dressed up as
    # a measurement, which is the exact failure this code exists to rule out.
    #
    # Locked, unlike the bare `Value` this replaces.  The lock is what makes
    # the two halves of a publication inseparable, and an unpaired CPU figure
    # is not a cheaper measurement but a different and wrong one: it forces the
    # ratio's denominator to be an interval the controller timed, which is not
    # the interval the numerator covers (see `_spin.publish`).  The cost is a
    # semaphore per chunk against a chunk of several milliseconds -- far below
    # the 15.6 ms tick that is the instrument's resolution, so it cannot
    # perturb the quantity being measured.
    cpu = [ctx.Array("d", 2) for _ in range(args.spinners)]
    # Counts spinners that have reached `_spin` and published an opening
    # balance.  This one *is* locked: it has as many writers as there are
    # spinners, it is incremented once each, and it is read as a barrier rather
    # than as a measurement, so contention on it is irrelevant.
    ready_count = ctx.Value("i", 0)
    # The same, for the two edges of the load window: `fired_count` reaches
    # `spinners` once every spinner has published a pair taken after `go` and
    # before its first chunk; `done_count` once every spinner has published one
    # after `stop`.  Without them the controller would read whatever pair
    # happened to be in the cell, which on a loaded host is a pair from before
    # the window opened.
    fired_count = ctx.Value("i", 0)
    done_count = ctx.Value("i", 0)
    workers = [
        ctx.Process(target=_spin,
                    args=(go, stop, deadline, slot, ready_count,
                          fired_count, done_count),
                    daemon=True)
        for slot in cpu
    ]
    for worker in workers:
        worker.start()

    # Wait for every spinner to publish before anything else happens.
    #
    # `Process.start()` returns when the process has been *spawned*, not when
    # its interpreter has finished booting and reached `_spin`.  Under the
    # spawn start method the child re-imports this module, which is hundreds of
    # milliseconds of CPU -- and until the fix on 2026-08-31 all of it landed
    # inside the measured window, because the controller snapshotted an opening
    # balance of 0.0 from a spinner that had not yet published anything.  Two
    # spinners over a 0.2 s window measured 1.82 cores each, which is not a
    # tolerance problem but an impossibility.
    #
    # The comment below this block already claimed readiness meant "every
    # interpreter is up".  It did not; `start()` cannot promise that.  This
    # barrier is what makes the sentence true.
    barrier_deadline = time.monotonic() + SPINNER_READY_TIMEOUT_S
    while True:
        with ready_count.get_lock():
            up = ready_count.value
        if up >= args.spinners:
            break
        if time.monotonic() > barrier_deadline:
            # Not fatal: a spinner that never starts is exactly what
            # `idle_spinners` and OCCUPANCY_FLOOR exist to report, and failing
            # here would turn a measurable problem into an unmeasurable one.
            # But it must be visible rather than silently absorbed.
            print(f"=== WARNING: only {up} of {args.spinners} spinner(s) "
                  f"published within {SPINNER_READY_TIMEOUT_S}s; the opening "
                  f"balance of the rest will include their startup ===",
                  flush=True)
            break
        if not any(w.is_alive() for w in workers):
            print("=== WARNING: no spinner is alive while waiting for the "
                  "readiness barrier ===", flush=True)
            break
        time.sleep(0.01)

    record = {
        "serial": args.serial,
        "at": args.at,
        "until": args.until,
        "spinners": args.spinners,
        "spinner_pids": [worker.pid for worker in workers],
        "fired": False,
        "released": False,
        "outcome": "pending",
        # How many completions the post-stop drain picked up.  Always present,
        # including on the paths that never drain, so a reader can never
        # mistake "the key is missing because this build predates the drain"
        # for "the drain found nothing".  A large number here means the
        # controller was starved near the end of the run: those completions
        # are all stamped at drain time rather than at their own, so the
        # window's right edge is that much less precise.
        "drained_after_stop": 0,
    }

    # The host canary starts before the load and stops after it, so it always
    # has an undisturbed stretch to serve as its own reference.
    probe_samples = []
    probe_stop = threading.Event()
    probe = threading.Thread(
        target=_host_probe, args=(probe_samples, probe_stop, started),
        daemon=True)
    probe.start()

    # Announce readiness only once every interpreter is up -- which the barrier
    # above now actually establishes, rather than this comment merely asserting
    # it.  The caller uses this line to decide it is safe to start QEMU, and
    # announcing earlier would move the interpreter startup cost back inside the
    # measurement window, which is the whole thing this program exists to
    # avoid.
    print(f"=== load controller ready: {args.spinners} spinner(s), "
          f"pids {' '.join(str(w.pid) for w in workers)} ===", flush=True)
    if args.ready_file:
        # Write-then-rename, because the reader waits on this path *existing*.
        # `open(path, "w")` publishes the name before the content, so a reader
        # polling for the file can open it in the gap and read nothing --
        # which is not a hang or a crash but an empty pid list, i.e. a
        # successful-looking answer that is wrong.  `os.replace` is atomic on
        # both POSIX and Windows, so the path is either absent or complete.
        # Seen as a ~1-in-20 flake in `scripts/test-canary-load.py`
        # ("spinner pids were published: got 0, want 2").
        tmp = f"{args.ready_file}.tmp"
        with open(tmp, "w", encoding="utf-8") as handle:
            handle.write(" ".join(str(worker.pid) for worker in workers))
        os.replace(tmp, args.ready_file)

    tail = SerialTail(args.serial)
    watcher = Watcher()
    on_at = off_at = None
    first_seen_at = None
    lines_before_on = 0
    during = []
    cpu_at_fire = None
    cpu_at_release = None
    # How long each edge barrier waited, in wall seconds.  Recorded rather than
    # assumed negligible, for the same reason `poll_seconds` is: it is the
    # instrument's own resolution.  It is also the one cost the barriers add --
    # the window's left edge is delayed by the slowest spinner's wakeup -- so a
    # reader who wants to know whether the load really began where the record
    # says it did can look instead of guessing.
    barrier_wait = [None, None]

    def snapshot():
        """Read every spinner's (cpu, clock) pair, each under its own lock."""
        pairs = []
        for slot in cpu:
            with slot.get_lock():
                pairs.append((slot[0], slot[1]))
        return pairs

    def await_barrier(counter, index):
        """Block until every spinner has published for this window edge.

        Bounded, and a timeout is *not* an error here: a spinner that died --
        the deadline, a vanished parent, `TerminateProcess` -- will never
        bump its counter, and refusing to close the window over it would turn
        a partial measurement into no measurement at all.  The pairs are
        self-consistent either way, so a late or missing publication costs
        accuracy at the edge and nothing else; `idle_spinners` is what reports
        a spinner that contributed nothing.
        """
        began = time.monotonic()
        limit = began + SPINNER_EDGE_TIMEOUT_S
        while time.monotonic() < limit:
            with counter.get_lock():
                if counter.value >= len(cpu):
                    break
            time.sleep(0.0005)
        barrier_wait[index] = time.monotonic() - began

    def fire():
        nonlocal on_at, cpu_at_fire
        go.set()
        on_at = time.monotonic()
        # Wait for every spinner to publish a pair taken after `go` and before
        # its first chunk, then read those pairs.  There is no separate span
        # stamp any more, and its absence is the fix: the span is now each
        # spinner's own clock delta, carried alongside the CPU figure it
        # belongs to, so the numerator and the denominator are the same
        # interval by construction rather than by two processes' timings
        # happening to agree.  The old arrangement -- controller reads a
        # parked spinner's last publication, controller stamps the span --
        # made the numerator start wherever that spinner last spoke while the
        # denominator started at the stamp, and charged the difference to
        # occupancy.  That is what put `occupancy_measured` at 1.089 against a
        # hard ceiling of 1.078 and refused a boot test, and at 1.009 on an
        # *idle* host in a 10-run probe -- an impossibility either way, since
        # a single-threaded process cannot outrun its own elapsed time.  See
        # known-issues.md, A-CANARY-OCCUPANCY-CEILING-IS-DERIVED-FROM-THE-
        # WRONG-ERROR-MODEL.
        await_barrier(fired_count, 0)
        cpu_at_fire = snapshot()

    def release():
        nonlocal off_at, cpu_at_release
        stop.set()
        off_at = time.monotonic()
        # Symmetric: wait for the closing publication so the final chunk is
        # counted rather than truncated, then read.
        await_barrier(done_count, 1)
        cpu_at_release = snapshot()

    if args.at is None:
        # No trigger: the whole-window behaviour the P20 control used.
        fire()
        record["fired"] = True
        print("=== load applied (whole window) ===", flush=True)

    def consume(now, final=False):
        """Read whatever the tail has and act on it. True => stop looping.

        `final` marks the one call made *after* the stop-file appeared, where
        the producer is known to have finished.  It differs in exactly one
        way: it will not `fire()`.  See `drain_after_stop` below.
        """
        nonlocal lines_before_on, first_seen_at
        if not tail.opened():
            return False
        seen = watcher.feed(tail.lines(), now)
        if seen and first_seen_at is None:
            first_seen_at = watcher.completions[0][1]
        for name in seen:
            if on_at is None:
                lines_before_on += 1
                if name == args.at and not final:
                    fire()
                    record["fired"] = True
                    print(f"=== '{name}' finished: load on ===", flush=True)
            else:
                if off_at is None:
                    during.append(name)
                if args.until is not None and off_at is None \
                        and name == args.until:
                    release()
                    record["released"] = True
                    print(f"=== '{name}' finished: load off ===", flush=True)
        if final:
            record["drained_after_stop"] = len(seen)
        return off_at is not None and not args.hold

    try:
        while True:
            now = time.monotonic()
            if now > started + args.timeout:
                record["outcome"] = "timeout"
                # No final drain here, deliberately -- see `drain_after_stop`.
                # A timeout carries no promise that the producer has finished,
                # so the file may still be growing; stamping a release at the
                # instant the controller gave up would date the window's right
                # edge up to `--timeout` seconds after the benchmark that
                # closed it.  A void run reported as void beats a void run
                # reported with a plausible-looking window.
                break
            if args.stop_file and os.path.exists(args.stop_file):
                record["outcome"] = "stopped"
                # THE FINAL DRAIN (`drain_after_stop`).  Until 2026-09-02 this
                # broke immediately, discarding every line written since the
                # last poll -- lines that were *already on disk*, not lines
                # that had yet to arrive.  That is not a resolution limit, it
                # is throwing away the evidence the run exists to collect.
                #
                # The stop-file is what makes reading again correct rather
                # than racy.  `canary-load-test.sh` writes it only after
                # `wait "$BOOT_PID"` returns, so QEMU has exited and the
                # serial log is complete and closed: this read cannot miss a
                # line, and cannot see a partial one.
                #
                # It is a production bug, not a test artefact.  `--until` on
                # the *last* benchmark of a suite is the case that loses every
                # time the poll happens to land before that line: QEMU writes
                # the result, exits, the wrapper stops the controller, and the
                # window is reported `until-never-matched` -- the whole boot's
                # canary data voided over a line sitting in the file.  It is
                # how the second attempt in the module docstring was lost.
                # It surfaced as a *gate* failure on 2026-09-02, refusing a
                # `--bench` boot after 2517 s because the suite's own
                # `spinner occupancy (live)` case hit the same race under
                # three-lane host load; the same suite passed twice on a quiet
                # host, which is exactly what a discarded-read race looks like.
                #
                # `fire()` is suppressed in this pass while `release()` is not,
                # and the asymmetry is the point.  Releasing here is a true
                # statement: the load was on continuously from `on_at` until
                # now, so every drained benchmark did run under it, and the
                # only error is a right edge late by the length of the stall
                # (`occupancy_measured`, which divides by the span actually
                # measured, stays physically bounded regardless).  Firing here
                # would be a false one: the producer has already stopped, so a
                # load applied now covered nothing at all, and a record saying
                # `fired` with a zero-length window is worse than the honest
                # `at-never-matched`.
                consume(time.monotonic(), final=True)
                break

            if consume(now):
                record["outcome"] = "complete"
                break

            time.sleep(POLL_SECONDS)
    finally:
        # Unconditionally: an exception, a timeout and a clean finish must all
        # leave the host idle.  `release()` is idempotent in effect (setting a
        # set Event is a no-op) but would move the timestamp, so guard it.
        if off_at is None:
            release()
        for worker in workers:
            worker.join(timeout=5)
        for worker in workers:
            if worker.is_alive():
                # Last resort, and still by object rather than by name.
                worker.terminate()
        probe_stop.set()
        probe.join(timeout=5)
        last_seen = watcher.completions[-1][1] if watcher.completions else None
        fired_rel = (on_at - started) if on_at is not None else None
        released_rel = (off_at - started) if off_at is not None else None
        record.update({
            "load_seconds": (off_at - on_at)
                            if (on_at is not None and off_at is not None)
                            else None,
            "suite_seconds_seen": (last_seen - first_seen_at)
                                  if (first_seen_at is not None
                                      and last_seen is not None) else None,
            "completions_seen": len(watcher.completions),
            "completions_before_on": lines_before_on,
            "completions_during": len(during),
            "during_names": during,
            # Everything below is on one clock, seconds since this process
            # started, so a benchmark completion and a host-canary sample can
            # be compared directly.  This is what makes "when did the load
            # actually become effective" a measurement rather than a guess --
            # the question both void runs turned on and neither could answer.
            # A completion is stamped with the time the poll *observed* it, so
            # the benchmark actually finished somewhere in
            # [stamp - poll_seconds, stamp].  Recorded rather than assumed,
            # because it is the instrument's real resolution: lines arriving
            # in one batch share a stamp, and the controller cannot tell which
            # of them preceded the trigger it fired on in that same batch.
            "poll_seconds": POLL_SECONDS,
            "fired_at": round(fired_rel, 4) if fired_rel is not None else None,
            "released_at": (round(released_rel, 4)
                            if released_rel is not None else None),
            "completions": [(name, round(when - started, 4))
                            for name, when in watcher.completions],
            "host_probe": summarise_probe(probe_samples, fired_rel,
                                          released_rel),
            "host_probe_samples": probe_samples,
            # The direct measurement, and the one to believe. See
            # `summarise_occupancy`.
            "host_occupancy": summarise_occupancy(
                cpu_at_fire, cpu_at_release,
                (off_at - on_at) if (on_at is not None
                                     and off_at is not None) else None,
                barrier_wait),
        })
        # A `--until` that never matched is not a detail.  The window has no
        # right-hand edge, so the run answers a different question from the
        # one asked, and the caller must be able to tell without reading the
        # JSON.  See the module docstring: this is exactly how the second
        # attempt was lost.
        #
        # Order is deliberate: a window with no edge is reported ahead of an
        # unapplied load, because a run that measured the wrong interval is
        # not made interpretable by having applied its load correctly, whereas
        # the reverse leaves a well-formed window that simply contained no
        # stimulus.  Both are fatal; the first is the more fundamental defect.
        occupancy = record.get("host_occupancy") or {}
        if args.until is not None and not record["released"]:
            record["problem"] = "until-never-matched"
        elif args.at is not None and not record["fired"]:
            record["problem"] = "at-never-matched"
        elif (occupancy.get("occupancy") is not None
                and occupancy["occupancy"] < OCCUPANCY_FLOOR):
            # The spinners were nominally running but barely got scheduled, so
            # whatever the benchmarks felt, it was not the intended stimulus.
            # Caught here rather than left for a human to notice in the JSON:
            # an earlier standalone probe ran with *zero* live spinners and
            # reported a confident ratio, and nothing in the output said so.
            record["problem"] = "load-not-applied"
        tail.close()

    return record


#: `[bench] MEASURED-AS <scored_name> <live_name>`, emitted by the kernel for
#: each benchmark whose two names differ. See `ScoreEntry::seq` in
#: `kernel/src/bench.rs` for why the pairing has to come from the kernel: it
#: cannot be recovered from the log by aligning the two orders, because they
#: genuinely interleave (`lock_uncontended` is recorded after
#: `lock_tracked_nested` but measured before it), and it cannot be recovered
#: from the source either, because six benchmarks build their `BenchResult` by
#: hand and print a live line that matches neither their variable nor their
#: struct's `name` field.
MEASURED_AS_RE = re.compile(r"^\[bench\] MEASURED-AS (\S+) (\S+)\s*$")


SCORE_NAME_RE = re.compile(r"^\[bench\] SCORE (\S+) ")


def read_measured_as(path):
    """Scrape the kernel's own statement of the scored -> live name pairing."""
    aliases = {}
    try:
        with open(path, encoding="utf-8", errors="replace") as handle:
            for line in handle:
                match = MEASURED_AS_RE.match(line.rstrip("\r\n"))
                if match:
                    aliases[match.group(1)] = match.group(2)
    except OSError:
        return {}
    return aliases


def read_scored_names(path):
    """The names that reached the scorecard, which is what the grader scores."""
    scored = set()
    try:
        with open(path, encoding="utf-8", errors="replace") as handle:
            for line in handle:
                match = SCORE_NAME_RE.match(line)
                if match:
                    scored.add(match.group(1))
    except OSError:
        return set()
    return scored


def read_known_names(path):
    """What a previous run saw: `(live_names, scored_to_live, scored_names)`.

    Absence is not an error: the first run on a fresh checkout has no list
    yet, and refusing to run without one would make the guard impossible to
    bootstrap.  The names come back as None (unknown) rather than an empty set
    (nothing matches), because those must not behave the same way.

    A bare line is a live name and an `alias <scored> <live>` line is a
    pairing, so a file written before aliases existed still reads back as a
    plain live-name list rather than as an empty one.
    """
    if not path or not os.path.exists(path):
        return None, {}, set()
    names, aliases, scored = set(), {}, set()
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            parts = line.split()
            if not parts:
                continue
            if parts[0] == "alias" and len(parts) == 3:
                aliases[parts[1]] = parts[2]
            elif parts[0] == "scored" and len(parts) == 2:
                scored.add(parts[1])
            elif len(parts) == 1:
                names.add(parts[0])
    return (names or None), aliases, scored


def write_known_names(path, names, aliases=None, scored=None):
    """Record this run's live names and name pairings, for the next run."""
    try:
        with open(path, "w", encoding="utf-8") as handle:
            for name in sorted(set(names)):
                handle.write(name + "\n")
            for score_name, live in sorted((aliases or {}).items()):
                handle.write(f"alias {score_name} {live}\n")
            for name in sorted(scored or ()):
                handle.write(f"scored {name}\n")
    except OSError as exc:
        # A guard that cannot be written is worth a warning, never a failed
        # run: the experiment itself succeeded or failed on its own terms.
        print(f"warning: could not write {path}: {exc}", file=sys.stderr)


def report_unreachable_scored(live, aliases, scored):
    """Warn about scored names that name no live result line.

    Such a name cannot be a window bound: `--at`/`--until` trigger on the live
    line, so asking for one of these waits for output that is never printed and
    the run is lost to a stimulus that never fired.  That is how P22's second
    attempt died, and the three names responsible sat undiagnosed for a day
    because nothing said them out loud -- the only thing that noticed was a
    test which happened to probe for orphans and reported the count.

    Printed after every run rather than checked once, on the same reasoning as
    the kernel's `SCORED_WITHOUT_MEASUREMENT`: a gap that is recomputed from
    each run's own output cannot go stale, and a silent miscount becomes a
    printed anomaly.  Silent when there is nothing to say, so the line means
    something when it appears.

    Returns the sorted list of offenders, for a caller that wants to act on it.
    """
    if not live or not scored:
        return []
    orphans = sorted(name for name in scored
                     if aliases.get(name, name) not in live)
    if orphans:
        print(f"warning: {len(orphans)} scored benchmark(s) name no live "
              f"result line, so they cannot be used as a window bound:",
              file=sys.stderr)
        for name in orphans:
            print(f"    {name}", file=sys.stderr)
        print("    The kernel emits '[bench] MEASURED-AS <scored> <live>' for "
              "every benchmark whose\n"
              "    two names differ; a missing pairing means the live line's "
              "name was not the one\n"
              "    passed to note_measurement(). See Measurement::name in "
              "kernel/src/bench.rs.", file=sys.stderr)
    return orphans


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Load the host CPU across a named window of a --bench "
                    "run (the stimulus for prediction P22).")
    parser.add_argument("--serial", required=True,
                        help="serial log the boot will write")
    parser.add_argument("--at", default=None,
                        help="apply the load once this benchmark's result "
                             "line appears (default: immediately)")
    parser.add_argument("--until", default=None,
                        help="remove the load once this benchmark's result "
                             "line appears (default: run to the end)")
    parser.add_argument("--spinners", type=int, default=6,
                        help="number of CPU burners (default 6)")
    parser.add_argument("--timeout", type=float, default=1800,
                        help="give up after this many seconds (default 1800)")
    parser.add_argument("--grace", type=float, default=120,
                        help="extra seconds before the spinners' own hard "
                             "deadline, which is what stops them surviving "
                             "an ungraceful kill of this process")
    parser.add_argument("--stop-file", default=None,
                        help="finish as soon as this path exists; the caller "
                             "creates it when the boot is over")
    parser.add_argument("--ready-file", default=None,
                        help="write the spinner PIDs here once they are up")
    parser.add_argument("--record", default=None,
                        help="write the JSON record of what actually "
                             "happened here")
    parser.add_argument("--hold", action="store_true",
                        help="keep following the log after the load is "
                             "removed, until --stop-file or --timeout")
    parser.add_argument("--known-names", default=None,
                        help="file of live benchmark names seen by a previous "
                             "run; --at/--until are checked against it before "
                             "anything is spawned, and it is rewritten from "
                             "this run's own names")
    parser.add_argument("--require-scored", action="store_true",
                        help="also require --at/--until to reach the "
                             "scorecard; pass this when grade-positional.py "
                             "will grade the run, since it can only place a "
                             "window using scorecard names")
    parser.add_argument("--check-names-only", action="store_true",
                        help="validate --at/--until against --known-names and "
                             "exit; used before the boot starts, so a bad "
                             "name costs a second rather than a whole run")
    args = parser.parse_args(argv)

    if args.until is not None and args.at is None:
        parser.error("--until requires --at (a prefix window cannot "
                     "discriminate; see P22)")

    known, aliases, scored = read_known_names(args.known_names)
    if known and args.require_scored and scored:
        # The other half of the two-namespace trap, and the one the live-name
        # check alone cannot catch. `vfs_write_16k` is a perfectly good live
        # name, so the load would fire exactly where asked -- and then
        # grade-positional.py, which can only place a window using scorecard
        # names, refuses it, and the boot that just ran for two and a half
        # minutes is wasted. Checked against the *original* name, before any
        # alias translation, because the original is the one the grader sees.
        unscoreable = [(flag, name)
                       for flag, name in (("--at", args.at),
                                          ("--until", args.until))
                       if name is not None and name not in scored]
        if unscoreable:
            for flag, name in unscoreable:
                print(f"{flag} '{name}' never reaches the scorecard, so the "
                      f"grader cannot place a window with it.", file=sys.stderr)
                back = [s for s, live in aliases.items() if live == name]
                if back:
                    print(f"    it is measured under that name but scored as "
                          f"'{back[0]}' -- pass that instead.", file=sys.stderr)
                else:
                    near = difflib.get_close_matches(name, sorted(scored), n=5,
                                                     cutoff=0.4)
                    if near:
                        print(f"    closest scored names: {', '.join(near)}",
                              file=sys.stderr)
            return 2
    if known:
        # A scorecard name is translated rather than refused, and the
        # translation is announced. The grader scores against scorecard names
        # and this script triggers on live ones, so for the ~6 benchmarks
        # whose two names differ there is no single name the caller could
        # pass that works for both -- refusing here would leave those
        # benchmarks simply unusable as window bounds. Announcing it matters
        # as much as doing it: a silent substitution is how the second
        # attempt's window ended up somewhere other than its label.
        for flag in ("at", "until"):
            name = getattr(args, flag)
            if name is not None and name not in known and name in aliases:
                print(f"--{flag} '{name}' is a scorecard name; triggering on "
                      f"its live result line '{aliases[name]}' instead "
                      f"(the kernel reports them as the same benchmark).",
                      file=sys.stderr)
                setattr(args, flag, aliases[name])
        # Checked here rather than after the boot because the whole point is
        # to make a mistyped or scorecard-only name cost a second instead of
        # a 2.5-minute boot that produces an unusable run.
        unknown = [(flag, name)
                   for flag, name in (("--at", args.at), ("--until", args.until))
                   if name is not None and name not in known]
        if unknown:
            for flag, name in unknown:
                near = difflib.get_close_matches(name, sorted(known), n=5,
                                                 cutoff=0.4)
                print(f"{flag} '{name}' is not a live benchmark name.",
                      file=sys.stderr)
                if near:
                    print(f"    closest live names: {', '.join(near)}",
                          file=sys.stderr)
            print(f"    ({len(known)} live names known, from "
                  f"{args.known_names})", file=sys.stderr)
            print("    Note the live result line and the end-of-run SCORE "
                  "line do not always agree;\n"
                  "    --at/--until match the live line.", file=sys.stderr)
            return 2

    if args.check_names_only:
        # Silent on success: this runs before every boot, and a line of
        # reassurance per run trains the reader to skip the block that also
        # carries the refusal.
        return 0

    record = run(args)

    if args.known_names and record.get("completions"):
        live_seen = [name for name, _ in record["completions"]]
        measured_as = read_measured_as(args.serial)
        scored_seen = read_scored_names(args.serial)
        write_known_names(args.known_names, live_seen, measured_as,
                          scored_seen)
        report_unreachable_scored(set(live_seen), measured_as, scored_seen)

    if args.record:
        with open(args.record, "w", encoding="utf-8") as handle:
            json.dump(record, handle, indent=2, sort_keys=True)

    print("=== load controller summary ===", flush=True)
    print(f"  outcome           : {record['outcome']}")
    print(f"  load applied      : {record['fired']}"
          + ("" if record["fired"] else f" (never saw '{args.at}')"))
    if record.get("load_seconds") is not None:
        print(f"  load held for     : {record['load_seconds']:.2f}s")
    if record.get("suite_seconds_seen") is not None:
        print(f"  suite spanned     : {record['suite_seconds_seen']:.2f}s "
              f"({record['completions_seen']} result lines)")
    print(f"  ran under load    : {record['completions_during']} result "
          f"lines, after {record['completions_before_on']} clean ones")
    # The fraction is the number to look at when a window turns out to be
    # ungradeable: a stimulus occupying 3% of the suite's wall time cannot
    # perturb a quarter of its benchmarks, however precisely it was triggered.
    if record.get("load_seconds") and record.get("suite_seconds_seen"):
        share = 100 * record["load_seconds"] / record["suite_seconds_seen"]
        print(f"  window share      : {share:.1f}% of the suite's wall time")

    # The host's own account of whether it was actually busy.  Printed next to
    # the controller's intentions on purpose: "I set the flag" and "the machine
    # got slower" are different claims, and only the second one is a stimulus.
    # Occupancy first, and on its own line, because it is the one figure here
    # that actually answers "was the load applied".  The canary below it is
    # descriptive colour; printing them the other way round is how a x0.78
    # came to be read as a result.
    occ = record.get("host_occupancy")
    if occ and occ.get("occupancy") is not None:
        verdict = ("load applied" if occ["occupancy"] >= OCCUPANCY_FLOOR
                   else "LOAD NOT APPLIED")
        print(f"  spinner occupancy : {occ['occupancy'] * 100:.0f}% "
              f"({occ['cpu_seconds_total']:.2f}s CPU burned of "
              f"{occ['expected_cpu_seconds']:.2f}s possible across "
              f"{occ['spinners']} spinner(s)) -- {verdict}")
        if occ["idle_spinners"]:
            print(f"  WARNING           : {occ['idle_spinners']} spinner(s) "
                  f"burned almost no CPU", file=sys.stderr)
    elif record.get("spinners") == 0:
        # A deliberate zero-spinner control, not a failure. Said out loud
        # because "no occupancy line" and "occupancy of nothing" would
        # otherwise look identical in the output.
        print("  spinner occupancy : n/a -- 0 spinners requested (control "
              "run: no load was meant to be applied)")
    else:
        print("  spinner occupancy : NOT MEASURED -- treat any apparent "
              "stimulus below with suspicion")

    host = record.get("host_probe")
    if host and host.get("inflation") is not None:
        # Best-case, not median.  The median's spread on this host is wider
        # than the effect, so it is printed only as a parenthetical and only
        # to keep old logs comparable.
        print(f"  host canary       : x{host['inflation']:.2f} best-case "
              f"({host['idle_best_s'] * 1e3:.2f}ms idle -> "
              f"{host['loaded_best_s'] * 1e3:.2f}ms under load, "
              f"{host['samples']} samples; "
              f"median x{host['median_inflation']:.2f}, unreliable)")
        print("  note              : with more cores than spinners the probe "
              "keeps a free core, so a ratio near x1 here is expected and is "
              "not evidence either way -- see occupancy above.")
    elif host:
        print(f"  host canary       : not comparable "
              f"({host['samples']} samples, no idle/loaded contrast)")

    problem = record.get("problem")
    if problem == "at-never-matched":
        print(f"  PROBLEM           : never saw '{args.at}' -- the load was "
              f"never applied.", file=sys.stderr)
    elif problem == "until-never-matched":
        print(f"  PROBLEM           : never saw '{args.until}' -- the load was "
              f"applied but never released, so it ran to the end of the boot "
              f"and the window has no right-hand edge.", file=sys.stderr)
    elif problem == "load-not-applied":
        print(f"  PROBLEM           : the window was bounded correctly, but "
              f"the spinners burned only "
              f"{occ['cpu_seconds_total']:.2f}s of CPU where "
              f"{occ['expected_cpu_seconds']:.2f}s was available -- the load "
              f"was not actually applied, so the window is empty.",
              file=sys.stderr)
    if problem in ("at-never-matched", "until-never-matched"):
        # Only the name-matching failures have anything to do with names;
        # printing this under an occupancy failure would send the reader off
        # to check a spelling that is not the problem.
        print("    The live result line and the end-of-run SCORE line do not "
              "always agree; --at/--until match the live line. Pass "
              "--known-names to have this checked before the boot.",
              file=sys.stderr)
    # Exit non-zero for *either* missing edge.  Returning 0 for a window that
    # was opened and never closed is what let the second attempt run to
    # completion looking like a success.
    return 1 if problem else 0


if __name__ == "__main__":
    sys.exit(main())
