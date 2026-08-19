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


def summarise_probe(samples, fired_at, released_at):
    """Reduce the host canary to what a reader needs to judge the stimulus.

    The ratio is what matters, not the absolute cost: the unit of work's
    idle duration is a property of this host and this Python build, and
    comparing it against anything but itself would be meaningless.
    """
    if not samples:
        return None
    before = [d for t, d in samples if fired_at is None or t < fired_at]
    during = [d for t, d in samples
              if fired_at is not None and t >= fired_at
              and (released_at is None or t < released_at)]
    idle = statistics.median(before) if before else None
    busy = statistics.median(during) if during else None
    return {
        "samples": len(samples),
        "idle_median_s": round(idle, 6) if idle is not None else None,
        "loaded_median_s": round(busy, 6) if busy is not None else None,
        # None rather than 1.0 when either side is missing: "the load made no
        # difference" and "there was nothing to compare" are different
        # statements, and only one of them is evidence.
        "inflation": round(busy / idle, 3)
                     if (idle and busy) else None,
    }


def _spin(go, stop, deadline):
    """One CPU burner: block until `go`, then loop until `stop`.

    A tight pure-Python loop with no I/O and no sleeping, which is what
    contends with TCG emulation for a core.

    The self-defence checks matter as much as the loop.  MSYS `kill` of a
    native Windows process is `TerminateProcess`: no signal handler runs, no
    `finally` executes, and this process's parent simply vanishes.  Polling
    `parent.is_alive()` and a wall-clock deadline is the only thing standing
    between that and six orphaned processes spinning until the machine is
    rebooted.
    """
    parent = multiprocessing.parent_process()

    def should_quit():
        if stop.is_set() or time.monotonic() > deadline:
            return True
        return parent is not None and not parent.is_alive()

    # Wait with a timeout rather than forever: an orphan that never receives
    # `go` would otherwise sit blocked for the life of the machine.  The
    # timeout costs one wakeup per second and does *not* add latency -- the
    # semaphore wakes immediately when `go` is set.
    while not go.is_set():
        if should_quit():
            return
        go.wait(1.0)

    while True:
        for _ in range(SPIN_CHUNK):
            pass
        if should_quit():
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

    workers = [
        ctx.Process(target=_spin, args=(go, stop, deadline), daemon=True)
        for _ in range(args.spinners)
    ]
    for worker in workers:
        worker.start()

    record = {
        "serial": args.serial,
        "at": args.at,
        "until": args.until,
        "spinners": args.spinners,
        "spinner_pids": [worker.pid for worker in workers],
        "fired": False,
        "released": False,
        "outcome": "pending",
    }

    # The host canary starts before the load and stops after it, so it always
    # has an undisturbed stretch to serve as its own reference.
    probe_samples = []
    probe_stop = threading.Event()
    probe = threading.Thread(
        target=_host_probe, args=(probe_samples, probe_stop, started),
        daemon=True)
    probe.start()

    # Announce readiness only once every interpreter is up, because the
    # caller uses this line to decide it is safe to start QEMU.  Announcing
    # earlier would move the interpreter startup cost back inside the
    # measurement window, which is the whole thing this program exists to
    # avoid.
    print(f"=== load controller ready: {args.spinners} spinner(s), "
          f"pids {' '.join(str(w.pid) for w in workers)} ===", flush=True)
    if args.ready_file:
        with open(args.ready_file, "w", encoding="utf-8") as handle:
            handle.write(" ".join(str(worker.pid) for worker in workers))

    tail = SerialTail(args.serial)
    watcher = Watcher()
    on_at = off_at = None
    first_seen_at = None
    lines_before_on = 0
    during = []

    def fire():
        nonlocal on_at
        go.set()
        on_at = time.monotonic()

    def release():
        nonlocal off_at
        stop.set()
        off_at = time.monotonic()

    if args.at is None:
        # No trigger: the whole-window behaviour the P20 control used.
        fire()
        record["fired"] = True
        print("=== load applied (whole window) ===", flush=True)

    try:
        while True:
            now = time.monotonic()
            if now > started + args.timeout:
                record["outcome"] = "timeout"
                break
            if args.stop_file and os.path.exists(args.stop_file):
                record["outcome"] = "stopped"
                break

            if tail.opened():
                seen = watcher.feed(tail.lines(), now)
                if seen and first_seen_at is None:
                    first_seen_at = watcher.completions[0][1]
                for name in seen:
                    if on_at is None:
                        lines_before_on += 1
                        if name == args.at:
                            fire()
                            record["fired"] = True
                            print(f"=== '{name}' finished: load on ===",
                                  flush=True)
                    else:
                        if off_at is None:
                            during.append(name)
                        if args.until is not None and off_at is None \
                                and name == args.until:
                            release()
                            record["released"] = True
                            print(f"=== '{name}' finished: load off ===",
                                  flush=True)
                if off_at is not None and not args.hold:
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
        })
        # A `--until` that never matched is not a detail.  The window has no
        # right-hand edge, so the run answers a different question from the
        # one asked, and the caller must be able to tell without reading the
        # JSON.  See the module docstring: this is exactly how the second
        # attempt was lost.
        if args.until is not None and not record["released"]:
            record["problem"] = "until-never-matched"
        elif args.at is not None and not record["fired"]:
            record["problem"] = "at-never-matched"
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
        write_known_names(args.known_names,
                          [name for name, _ in record["completions"]],
                          read_measured_as(args.serial),
                          read_scored_names(args.serial))

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
    host = record.get("host_probe")
    if host and host.get("inflation") is not None:
        print(f"  host canary       : x{host['inflation']:.2f} "
              f"({host['idle_median_s'] * 1e3:.2f}ms idle -> "
              f"{host['loaded_median_s'] * 1e3:.2f}ms under load, "
              f"{host['samples']} samples)")
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
    if problem:
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
