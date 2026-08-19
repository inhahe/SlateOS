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
import json
import multiprocessing
import os
import re
import sys
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
        last_seen = watcher.completions[-1][1] if watcher.completions else None
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
        })
        tail.close()

    return record


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
    args = parser.parse_args(argv)

    if args.until is not None and args.at is None:
        parser.error("--until requires --at (a prefix window cannot "
                     "discriminate; see P22)")

    record = run(args)

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
    return 0 if record["fired"] else 1


if __name__ == "__main__":
    sys.exit(main())
