#!/usr/bin/env python3
"""Tests for `hostload.py`, the harness's measurement of how much CPU a process gets.

The arithmetic is tested on hand-written samples, where the right answer is
known exactly. The live instruments -- spinners measuring the real host -- are
tested only for properties that hold on *any* host, idle or starved: a test
that asserted "this host has headroom" would fail on exactly the machines the
module exists for.
"""
from __future__ import annotations

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hostload  # noqa: E402

FAILURES: list[str] = []


def check(label, got, want):
    if got != want:
        FAILURES.append(f"{label}: got {got!r}, want {want!r}")
        print(f"  FAIL {label}: got {got!r}, want {want!r}")
    else:
        print(f"  ok   {label}")


def check_true(label, cond, detail=""):
    if not cond:
        FAILURES.append(f"{label}: {detail}")
        print(f"  FAIL {label}: {detail}")
    else:
        print(f"  ok   {label}")


print("occupancy_over")
# One spinner that got a whole core for a second and then half of one.
_FULL_THEN_HALF = [[(0.0, 0.0), (1.0, 1.0), (2.0, 1.5)]]
for label, series, t0, t1, want in [
    ("a whole core over the interval reads 1.0", _FULL_THEN_HALF, 0.0, 1.0, 1.0),
    ("half a core reads 0.5", _FULL_THEN_HALF, 1.0, 2.0, 0.5),
    ("an interval between samples is interpolated", _FULL_THEN_HALF,
     0.5, 1.5, 0.75),
    ("an interval the samples do not bracket excuses nothing",
     _FULL_THEN_HALF, -1.0, 1.0, None),
    ("an empty interval is no measurement", _FULL_THEN_HALF, 1.0, 1.0, None),
    ("a missing edge is no measurement", _FULL_THEN_HALF, None, 1.0, None),
    ("two spinners are averaged, a starved one included",
     [[(0.0, 0.0), (1.0, 1.0)], [(0.0, 0.0), (1.0, 0.0)]], 0.0, 1.0, 0.5),
    ("a spinner that stopped early is left out rather than read as idle",
     [[(0.0, 0.0), (1.0, 1.0)], [(0.0, 0.0), (0.5, 0.5)]], 0.0, 1.0, 1.0),
]:
    got = hostload.occupancy_over(series, t0, t1)
    check(label, None if got is None else round(got, 6), want)

print()
print("stall_over")
# Samples 10 ms apart with one 0.5 s hole in them.
_HOLE = [[(0.00, 0.0), (0.01, 0.01), (0.51, 0.02), (0.52, 0.03)]]
for label, series, t0, t1, want in [
    ("a spinner that was never descheduled stalled for nothing",
     [[(0.00, 0.0), (0.01, 0.01), (0.02, 0.02)]], 0.0, 0.02, 0.0),
    ("a 0.5 s hole reads as the time not scheduled", _HOLE, 0.0, 0.52, 0.49),
    ("a hole outside the interval does not count", _HOLE, 0.515, 0.52, 0.0),
    ("an interval nobody sampled is no measurement", _HOLE, 1.0, 2.0, None),
]:
    got = hostload.stall_over(series, t0, t1)
    check(label, None if got is None else round(got, 6), want)

print()
print("ConcurrentProbe (live)")
probe = hostload.ConcurrentProbe(2)
# Waited for, not assumed: a starved host starts a spinner late, and one that
# began after the stop-file took a single sample (2026-09-25, in the push hook).
check("both spinners start", probe.wait_started(), True)
t_start = time.monotonic()
time.sleep(0.3)
t_mid = time.monotonic()
series = probe.finish()
check("one series per spinner", len(series), 2)
for i, samples in enumerate(series):
    # At least one sample is all a started spinner guarantees: a host can
    # leave it unscheduled for the whole 0.3 s after that.
    check_true(f"spinner {i} took samples", len(samples) >= 1, samples[:3])
    check_true(f"spinner {i}'s clock only moves forward",
               all(b[0] >= a[0] for a, b in zip(samples, samples[1:])),
               samples[:5])
    check_true(f"spinner {i}'s CPU only accumulates",
               all(b[1] >= a[1] for a, b in zip(samples, samples[1:])),
               samples[:5])
got = hostload.occupancy_over(series, t_start, t_mid)
# Any value in [0, ~1] is a correct measurement of *some* host; the property
# is that it is a share of one core, not that the host had room.
check_true("its occupancy is a share of one core, or unmeasured",
           got is None or 0.0 <= got <= 1.1, got)

print()
print("host_headroom and starved_now (live)")
head = hostload.host_headroom(2)
check_true("host_headroom is a share of one core, or None",
           head is None or 0.0 <= head <= 1.1, head)
# Floors no host can miss or clear make starved_now deterministic anywhere,
# except that a probe which could not run excuses nothing either way.
check("no host is starved below a floor of zero",
      hostload.starved_now(floor=0.0), None)
reason = hostload.starved_now(floor=2.0)
check_true("every host is starved below a floor of two cores, and says why",
           reason is None or ("plain spinners get" in reason and "2.0" in reason),
           reason)

print()
print("run_measured (live)")
import subprocess  # noqa: E402

done = hostload.run_measured(
    [sys.executable, "-c", "print('ran')"], 120, "a trivial command",
    capture_output=True, text=True)
check("a command that finishes returns its CompletedProcess",
      (done.returncode, done.stdout.strip()), (0, "ran"))
# A sleeper past its timeout. Which exception is right depends on the host --
# a spinner beside it that got a core makes it a hang, one that did not makes
# it the host's -- so the property is that it is one of those two, never
# anything else, and that the host's case says what it measured.
try:
    hostload.run_measured([sys.executable, "-c", "import time; time.sleep(60)"],
                          2, "a sleeper", capture_output=True)
    check_true("a command past its timeout does not return", False,
               "it returned")
except subprocess.TimeoutExpired:
    check_true("a command past its timeout is a hang when the host had room",
               True)
except hostload.HostStarved as exc:
    check_true("...or the host's, saying what it measured",
               "of a core" in str(exc) and "a sleeper" in str(exc), str(exc))

print()
if FAILURES:
    print(f"{len(FAILURES)} FAILURE(S)")
    for failure in FAILURES:
        print(f"  - {failure}")
    sys.exit(1)
print("all hostload tests passed")
