"""How much CPU this host will give a process right now -- measured, not assumed.

The harness's tests include physical questions: did two spinners each get
most of a core, was a line read before the stop-file, did a run finish within
its timeout. With six lanes building and booting on one machine, a host that
cannot give a process CPU when it asks is an ordinary state -- plain spinners
got 0.012 of a core on 2026-09-25 -- and a test that reads such a host's
answer as a verdict on the code blames the code for the machine. These are
the instruments a test uses to tell the two apart:

| name | measures |
|---|---|
| `host_headroom` | the occupancy plain spinners get over a short window, *now* |
| `ConcurrentProbe` | spinners run *beside* a case, sampling their own CPU every 10 ms |
| `occupancy_over` | a probe's CPU share over any interval inside its run |
| `stall_over` | the longest a probe spinner went unscheduled in an interval |
| `run_measured` | a subprocess run with a probe beside it; a starved timeout raises `HostStarved` |
| `starved_now` | a point measurement: a reason if the host is starved *now*, else `None` |

All of them are deliberately independent of the code they help judge: an
instrument that shared code with its subject could agree with it when both
are wrong. Taken from `test-canary-load.py`, where they were written for the
live occupancy cases, when `test-boot-test.py` needed the same judgement.

A test that uses them must still say what it declined: an attributed failure
is a *skip*, printed by name with the measurement, never a pass.
"""
from __future__ import annotations

import ast
import contextlib
import os
import shutil
import subprocess
import sys
import tempfile
import time

#: Mean share of a core below which this host counts as starved: half a core,
#: the same floor the load canary asks of its own spinners.
STARVED_BELOW = 0.5


class HostStarved(Exception):
    """A case cannot be answered: the host was measured too busy to run it."""


#: Wall window for `host_headroom`: long enough that the ~15.6 ms scheduler
#: grid is noise, short enough to be worth paying on a failure path.
HEADROOM_WINDOW_S = 0.6


def host_headroom(n, seconds=HEADROOM_WINDOW_S):
    """Occupancy that `n` plain spinners actually obtain on this host, now.

    Deliberately independent of `canary-load.py`: this measures the *machine*,
    in order to decide whether a shortfall in the code under test is
    attributable to the host.  Sharing an instrument with the thing being
    judged would let a broken instrument agree with itself.

    Two choices that matter:

    * **Subprocesses, not `multiprocessing`.**  This suite runs its cases at
      module scope and Windows spawns rather than forks, so a `Process` here
      would re-import this file and run the entire suite again inside every
      worker.
    * **Each child times its own window** and reports its own `cpu / wall`,
      rather than dividing every child by one shared window.  Process startup
      on Windows costs tens of milliseconds and the children do not begin
      together; against a shared window that skew reads as missing CPU, biasing
      the probe toward "busy" and skipping cases on an idle host.  Self-timing
      cancels it.

    Returns mean per-spinner occupancy in 0.0..~1.0, or `None` if the probe
    could not be run -- in which case the caller must not use it to excuse
    anything.
    """
    lines = [
        "import time, sys",
        "c0 = time.process_time(); w0 = time.monotonic()",
        "end = w0 + " + repr(float(seconds)),
        "while time.monotonic() < end: pass",
        "sys.stdout.write(repr((time.process_time() - c0,"
        " time.monotonic() - w0)))",
    ]
    code = chr(10).join(lines)
    try:
        procs = [
            subprocess.Popen([sys.executable, "-c", code],
                             stdout=subprocess.PIPE, text=True)
            for _ in range(n)
        ]
    except OSError:
        return None
    ratios = []
    for proc in procs:
        try:
            out, _ = proc.communicate(timeout=seconds * 20 + 30)
            cpu, wall = ast.literal_eval(out.strip())
        except Exception:  # noqa: BLE001 - a probe that fails excuses nothing
            with contextlib.suppress(Exception):
                proc.kill()
            continue
        if wall > 0:
            ratios.append(cpu / wall)
    if not ratios:
        return None
    return sum(ratios) / len(ratios)


#: Sampling period of the concurrent probe's spinners. Each sample is a
#: `(monotonic, process_time)` pair, so the CPU a spinner received over any
#: interval inside its run is an interpolation between two samples.
CONCURRENT_SAMPLE_S = 0.01


class ConcurrentProbe:
    """Plain spinners running *alongside* a live case, for attribution.

    `host_headroom` measures the host a moment after the run it is asked
    about. On a host whose load arrives in bursts -- six lanes building on one
    machine -- that moment can be idle while the run was starved, and the
    probe then reports headroom the run never had: lane F's boot test of
    0659769ab failed "occupancy clears the floor" at 0.113, with the probe
    reading headroom, and "no spinner was starved" with one spinner starved.
    The docstring of `attribute_shortfall` already named the gap ("the probe
    and the run measure at different instants").

    These spinners run over the whole live case and record their own CPU
    against `time.monotonic()` -- one clock across processes -- so their
    occupancy can be computed over exactly the interval the canary measured
    (`fired_monotonic`..`released_monotonic` in its record). They compete with
    the canary's spinners on equal terms, which is what makes the comparison
    fair: a host with room for both lets both clear the floor, and a host
    without it starves both.

    Independent of `canary-load.py` for the reason `host_headroom` is: an
    instrument must not share code with what it is judging.
    """

    def __init__(self, n, max_seconds=120):
        self.dir = tempfile.mkdtemp(prefix="hostload-probe-")
        self.stop_path = os.path.join(self.dir, "stop")
        self.n = n
        code = chr(10).join([
            "import os, sys, time",
            "stop = " + repr(self.stop_path),
            "ready = " + repr(os.path.join(self.dir, "ready-")),
            "end = time.monotonic() + " + repr(float(max_seconds)),
            "samples = []",
            "while True:",
            "    t = time.monotonic()",
            "    samples.append((t, time.process_time()))",
            "    if len(samples) == 1:",
            "        open(ready + str(os.getpid()), 'w').close()",
            "    if t > end or os.path.exists(stop):",
            "        break",
            "    until = t + " + repr(CONCURRENT_SAMPLE_S),
            "    while time.monotonic() < until:",
            "        pass",
            "sys.stdout.write(repr(samples))",
        ])
        try:
            self.procs = [
                subprocess.Popen([sys.executable, "-c", code],
                                 stdout=subprocess.PIPE, text=True)
                for _ in range(n)
            ]
        except OSError:
            self.procs = []

    def wait_started(self, timeout=60):
        """Wait until every spinner has taken its first sample; whether all did.

        Call it before taking the start of an interval to be measured. A
        spinner's samples cover only the time since it started, and process
        start is exactly what a starved host delays: a spinner that begins
        after the interval does would leave its start unbracketed, the
        interval would read as unmeasured, and a starved run would be called
        a hang -- the misattribution the probe exists to prevent. Seen on
        2026-09-25: a spinner took its first sample after the stop-file.
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                started = sum(1 for name in os.listdir(self.dir)
                              if name.startswith("ready-"))
            except OSError:
                started = 0
            if started >= self.n:
                return True
            time.sleep(0.02)
        return False

    def finish(self):
        """Stop the spinners; return each one's `(monotonic, cpu)` samples."""
        with open(self.stop_path, "w", encoding="utf-8", newline=""):
            pass
        series = []
        for proc in self.procs:
            try:
                out, _ = proc.communicate(timeout=60)
                series.append(ast.literal_eval(out.strip()))
            except Exception:  # noqa: BLE001 - a probe that fails excuses nothing
                with contextlib.suppress(Exception):
                    proc.kill()
        shutil.rmtree(self.dir, ignore_errors=True)
        return series


def occupancy_over(series, t0, t1):
    """Mean occupancy of the probe spinners over `[t0, t1]`, or `None`.

    Each spinner's CPU at `t0` and at `t1` is interpolated between the two
    samples around it. A spinner whose samples do not bracket the interval --
    it started late, or stopped early -- contributes nothing, and if none
    does the answer is `None`: an interval nobody measured excuses nothing.
    """
    if t0 is None or t1 is None or t1 <= t0:
        return None

    def cpu_at(samples, t):
        for (ta, ca), (tb, cb) in zip(samples, samples[1:]):
            if ta <= t <= tb:
                if tb == ta:
                    return ca
                return ca + (cb - ca) * (t - ta) / (tb - ta)
        return None

    ratios = []
    for samples in series:
        c0, c1 = cpu_at(samples, t0), cpu_at(samples, t1)
        if c0 is not None and c1 is not None:
            ratios.append((c1 - c0) / (t1 - t0))
    return sum(ratios) / len(ratios) if ratios else None


def stall_over(series, t0, t1):
    """The longest any probe spinner went unscheduled within `[t0, t1]`, or `None`.

    Occupancy is an average, and a half-second stall inside a case several
    seconds long barely moves it -- but that stall is exactly what breaks an
    assertion with one poll's tolerance. On 2026-09-25 a controller that saw
    its trigger acted on it 0.5 s later, and the case's occupancy still read
    above the floor. A probe spinner samples every `CONCURRENT_SAMPLE_S` while
    it is running, so a longer gap between two consecutive samples is time it
    was not: this returns that time, the sampling period already subtracted.
    """
    if t0 is None or t1 is None or t1 <= t0:
        return None
    worst = None
    for samples in series:
        for (ta, _), (tb, _) in zip(samples, samples[1:]):
            if tb >= t0 and ta <= t1:
                gap = max(0.0, (tb - ta) - CONCURRENT_SAMPLE_S)
                worst = gap if worst is None else max(worst, gap)
    return worst


def starved_now(floor=STARVED_BELOW, n=2):
    """Whether the host is starved *now*: a reason string if so, else `None`.

    A point measurement, and weaker than it looks: this host's load arrives in
    bursts, and on 2026-09-25 a probe taken right after a run had been starved
    for 180 s read a full core. So a `None` here is not proof the host had
    room earlier -- prefer `run_measured`, which measures beside the run. This
    remains for a caller that has nothing better: it can excuse a failure when
    the host is starved now, and must not read `None` as a verdict on the past.
    """
    headroom = host_headroom(n)
    if headroom is None or headroom >= floor:
        return None
    return (f"plain spinners get {headroom:.3f} of a core on this host, below "
            f"the {floor} floor -- too busy to answer")


def run_measured(argv, timeout, what, spinners=1, **kwargs):
    """`subprocess.run(argv, timeout=timeout, **kwargs)`, with the host measured beside it.

    Returns the `CompletedProcess`. On a timeout, a `ConcurrentProbe` that ran
    for the whole call decides whose fault it was: if its spinner could not
    average `STARVED_BELOW` of a core over that same interval, the host could
    not run the command in time and `HostStarved` is raised; otherwise the
    `TimeoutExpired` is re-raised -- a hang, for the caller to report.

    Beside the run, not after it: on 2026-09-25 a probe taken after a 180 s
    timeout caught a burst of headroom and called a starved run a hang. The
    cost is one spinner process per call, which is why callers use this for
    the few runs that can time out rather than for every subprocess.
    """
    # Outlasting the timeout by a margin: a probe that stopped first would
    # leave the interval unbracketed, and an unmeasured interval excuses
    # nothing -- every starved timeout would read as a hang.
    probe = ConcurrentProbe(spinners, max_seconds=timeout + 60)
    probe.wait_started()
    t0 = time.monotonic()
    try:
        return subprocess.run(argv, timeout=timeout, **kwargs)
    except subprocess.TimeoutExpired:
        t1 = time.monotonic()
        series = probe.finish()
        probe = None
        headroom = occupancy_over(series, t0, t1)
        if headroom is not None and headroom < STARVED_BELOW:
            raise HostStarved(
                f"{what} did not finish within {timeout}s, and a plain spinner "
                f"beside it got {headroom:.3f} of a core -- too busy to "
                f"answer") from None
        raise
    finally:
        if probe is not None:
            probe.finish()
