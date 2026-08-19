#!/usr/bin/env python3
"""Tests for `canary-load.py` -- the P22 stimulus, not the model.

Run: `python scripts/test-canary-load.py`

What this can and cannot establish
----------------------------------
It establishes that the load controller **fires where it is told to** and
**stops when it is told to**, by replaying a synthetic serial log into a file
while the controller follows it, and by checking the record it writes against
the replay's known schedule.  That is exactly the property whose absence
invalidated the first P22 run: the load was applied, the script said so, and
the benchmarks it named were not the ones that slowed down.

It does **not** establish that the load perturbs anything -- that is a
property of the host and QEMU, not of this program, and it is checked
empirically by `grade-positional.py`, which refuses to grade a run whose
loaded benchmarks did not measurably slow down.  A green run here is not
evidence for P22 and must never be reported as such.

The replay is deliberately faster than a real suite (milliseconds per
benchmark rather than tens of milliseconds).  That makes the timing
assertions *harder* to pass, not easier: the controller's 0.1 s poll interval
spans several replayed benchmarks, so the tests that pin down which
benchmarks ran under load are checking the ordering logic under conditions
strictly worse than the real thing.
"""

from __future__ import annotations

import ctypes
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CANARY_LOAD = os.path.join(REPO_ROOT, "scripts", "canary-load.py")

FAILURES = []


def load_module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cl = load_module(CANARY_LOAD, "canary_load")


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


def result_line(name, ns=100):
    """A live line in the kernel's real format."""
    return (f"[bench] {name}: min={ns} cycles ({ns}ns), mean={ns + 5} cycles "
            f"({ns + 5}ns), max={ns * 9} cycles  [200 iters] split "
            f"1st={ns} 2nd={ns} (0%)")


# --------------------------------------------------------------------------
# 1. Line recognition
# --------------------------------------------------------------------------
print("line recognition")

check("result line yields the name",
      cl.BENCH_LINE_RE.match(result_line("crypto_x25519")).group(1),
      "crypto_x25519")
check("prose with a colon is not a result",
      cl.BENCH_LINE_RE.match("[bench] Running self-test..."), None)
# `[bench]   canary scale check: OK - ...` would yield the "name" `canary`
# under a looser pattern, and `canary` is a plausible benchmark name -- so
# this is the case that would misfire silently rather than loudly.
check("multi-word prose with a colon is not a result",
      cl.BENCH_LINE_RE.match("[bench]   canary scale check: OK - 5.0"), None)
check("indented result lines still match",
      cl.BENCH_LINE_RE.match("[bench]   " + result_line("x")[8:]).group(1),
      "x")


# --------------------------------------------------------------------------
# 2. The suite marker gates the trigger
# --------------------------------------------------------------------------
print("suite marker")

watcher = cl.Watcher()
seen = watcher.feed([
    result_line("self_test_nop"),
    "[bench] Self-test PASSED",
], now=1.0)
check("self-test results before the marker are ignored", seen, [])
seen = watcher.feed([
    "[bench] === Kernel micro-benchmarks ===",
    result_line("page_alloc_free"),
], now=2.0)
check("results after the marker are counted", seen, ["page_alloc_free"])


# --------------------------------------------------------------------------
# 3. Replay: does the load land on the named window?
# --------------------------------------------------------------------------
print("replay")

SUITE = [f"bench_{i:02d}" for i in range(40)]


def replay(path, names, per_line=0.03, preamble=True):
    """Append a synthetic suite to `path`, one result line at a time."""
    def write():
        with open(path, "a", encoding="utf-8") as handle:
            if preamble:
                handle.write("[bench] TSC calibrated: ...\n")
                handle.write(result_line("self_test_nop") + "\n")
                handle.write("[bench] === Kernel micro-benchmarks ===\n")
                handle.flush()
            for name in names:
                handle.write(result_line(name) + "\n")
                handle.flush()
                time.sleep(per_line)
    thread = threading.Thread(target=write, daemon=True)
    thread.start()
    return thread


def run_controller(serial, extra, wait_for=None, delay=0.0):
    """Run the controller as a subprocess; return (record, returncode)."""
    with tempfile.TemporaryDirectory() as tmp:
        record_path = os.path.join(tmp, "record.json")
        ready_path = os.path.join(tmp, "ready")
        stop_path = os.path.join(tmp, "stop")
        proc = subprocess.Popen(
            [sys.executable, CANARY_LOAD, "--serial", serial,
             "--spinners", "0", "--timeout", "60",
             "--stop-file", stop_path, "--ready-file", ready_path,
             "--record", record_path, "--hold"] + extra,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        # The caller starts the replay only once the controller is ready,
        # mirroring the shell script, which does not boot QEMU until the
        # spinners exist.
        for _ in range(600):
            if os.path.exists(ready_path):
                break
            time.sleep(0.05)
        if wait_for is not None:
            wait_for()
        time.sleep(delay)
        with open(stop_path, "w", encoding="utf-8"):
            pass
        out = proc.communicate(timeout=90)[0]
        with open(record_path, encoding="utf-8") as handle:
            record = json.load(handle)
        return record, proc.returncode, out


with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")
    thread = None

    def start_replay():
        global thread
        thread = replay(serial, SUITE)
        thread.join()

    record, rc, out = run_controller(
        serial, ["--at", "bench_10", "--until", "bench_20"],
        wait_for=start_replay, delay=0.3)

    check("fired", record["fired"], True)
    check("released", record["released"], True)
    # The window is the benchmarks *after* the trigger up to and including
    # the release, which is precisely the convention `grade-positional.py`
    # resolves names to positions under.  If these two ever disagree the
    # experiment's ground truth is off by one benchmark at each end.
    check("window is (at, until]",
          record["during_names"], [f"bench_{i:02d}" for i in range(11, 21)])
    check("clean prefix counted",
          record["completions_before_on"], 11)
    check("whole suite seen", record["completions_seen"], len(SUITE))
    check_true("load held for about the window's duration",
               0.15 < record["load_seconds"] < 0.75,
               f"load_seconds={record['load_seconds']}")
    check_true("window share is a sane fraction",
               5 < 100 * record["load_seconds"]
               / record["suite_seconds_seen"] < 60,
               f"{record['load_seconds']} / {record['suite_seconds_seen']}")
    check("exit code 0 when the load fired", rc, 0)


with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")

    def start_replay2():
        replay(serial, SUITE, per_line=0.01).join()

    record, rc, out = run_controller(
        serial, ["--at", "no_such_benchmark"],
        wait_for=start_replay2, delay=0.3)

    # The failure that matters: a typo'd name must not look like a clean run.
    check("never fired on a name that is not in the suite",
          record["fired"], False)
    check("no window recorded", record["load_seconds"], None)
    check("exit code 1 when the load never fired", rc, 1)


with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")

    def start_replay3():
        replay(serial, SUITE, per_line=0.01).join()

    # No --until: the load runs from the trigger to the end of the boot,
    # which is the P20 behaviour with a delayed start.
    record, rc, out = run_controller(
        serial, ["--at", "bench_05"], wait_for=start_replay3, delay=0.3)
    check("fired without --until", record["fired"], True)
    check("everything after the trigger ran under load",
          record["during_names"], [f"bench_{i:02d}" for i in range(6, 40)])


# --------------------------------------------------------------------------
# 4. Spinners cannot outlive the controller
# --------------------------------------------------------------------------
print("orphan defence")


def pid_alive(pid):
    """True if `pid` is still running. Never signals it.

    `os.kill(pid, 0)` is not usable here: on Windows Python maps *every*
    signal other than the console-control ones onto `TerminateProcess`, so
    the conventional liveness probe would kill the process it is testing.
    """
    if os.name != "nt":
        try:
            os.kill(pid, 0)
            return True
        except OSError:
            return False
    SYNCHRONIZE = 0x00100000
    handle = ctypes.windll.kernel32.OpenProcess(SYNCHRONIZE, False, pid)
    if not handle:
        return False
    try:
        # WAIT_OBJECT_0 (0) means the process object is signalled, i.e. it
        # has exited; WAIT_TIMEOUT (0x102) means it is still running.
        return ctypes.windll.kernel32.WaitForSingleObject(handle, 0) != 0
    finally:
        ctypes.windll.kernel32.CloseHandle(handle)


with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")
    ready = os.path.join(tmpdir, "ready")
    # No --at, so the spinners start burning immediately; a short grace so
    # the hard deadline is reachable inside a test.
    proc = subprocess.Popen(
        [sys.executable, CANARY_LOAD, "--serial", serial, "--spinners", "2",
         "--timeout", "30", "--grace", "0", "--ready-file", ready],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(600):
        if os.path.exists(ready):
            break
        time.sleep(0.05)
    with open(ready, encoding="utf-8") as handle:
        pids = [int(p) for p in handle.read().split()]
    check("spinner pids were published", len(pids), 2)
    check_true("spinners are running", all(pid_alive(p) for p in pids),
               f"pids {pids}")

    # `proc.kill()` is `TerminateProcess` on Windows: no handler runs, no
    # cleanup happens.  This is exactly what an MSYS `kill` of the controller
    # does, and it is the case that could strand CPU burners on the operator's
    # machine indefinitely.
    proc.kill()
    proc.wait(timeout=30)

    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and any(pid_alive(p) for p in pids):
        time.sleep(0.2)
    check_true("spinners noticed the parent died and exited",
               not any(pid_alive(p) for p in pids),
               f"still alive: {[p for p in pids if pid_alive(p)]}")


# --------------------------------------------------------------------------
# 5. A window with no right-hand edge is a failure, not a footnote
# --------------------------------------------------------------------------
# This is the exact shape of P22's *second* void run.  `--until` named a
# benchmark that appears on the end-of-run scorecard but never as a live
# result line, so it could not match; the load was applied and never released,
# ran to the end of the boot, and the controller exited 0.  The run looked
# successful and was unusable.
print("unmatched --until")

with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")

    def start_replay4():
        replay(serial, SUITE, per_line=0.01).join()

    record, rc, out = run_controller(
        serial, ["--at", "bench_05", "--until", "not_a_live_line"],
        wait_for=start_replay4, delay=0.3)

    check("the load still fired", record["fired"], True)
    check("but it was never released", record["released"], False)
    check("and the record names the problem",
          record.get("problem"), "until-never-matched")
    check("exit code 1 when the window never closed", rc, 1)
    check_true("the operator is told which name never matched",
               "not_a_live_line" in out, out[-400:])


# --------------------------------------------------------------------------
# 6. Names are validated before anything is spawned
# --------------------------------------------------------------------------
print("name validation")


def run_validation(extra, known):
    """Run only far enough to accept or reject the names. Returns (rc, out)."""
    with tempfile.TemporaryDirectory() as tmp:
        names_path = os.path.join(tmp, "names.txt")
        if known is not None:
            with open(names_path, "w", encoding="utf-8") as handle:
                handle.write("\n".join(known) + "\n")
        proc = subprocess.run(
            [sys.executable, CANARY_LOAD, "--serial",
             os.path.join(tmp, "nonexistent-serial.txt"),
             "--spinners", "0", "--timeout", "5",
             "--known-names", names_path] + extra,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
            timeout=90)
        return proc.returncode, proc.stdout


rc, out = run_validation(["--at", "bench_05", "--until", "bench_99"], SUITE)
check("a name absent from the known list is rejected", rc, 2)
check_true("the rejection names the offending flag and value",
           "--until 'bench_99'" in out, out[-400:])
check_true("and suggests the closest live names",
           "bench_09" in out or "bench_98" in out or "closest" in out,
           out[-400:])
check_true("nothing was spawned before the rejection",
           "load controller ready" not in out, out[-400:])

# A typo one character off is the case the suggestion exists for.
rc, out = run_validation(["--at", "bench_O5"], SUITE)
check("a typo'd --at is rejected too", rc, 2)
check_true("and bench_05 is offered as the correction",
           "bench_05" in out, out[-400:])

# Absence of the file must not block a first run on a fresh checkout.
rc, out = run_validation(["--at", "bench_05"], None)
check_true("an absent known-names file does not reject anything",
           rc != 2, f"rc={rc}: {out[-300:]}")


with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")
    names_path = os.path.join(tmpdir, "names.txt")

    def start_replay5():
        replay(serial, SUITE, per_line=0.01).join()

    record, rc, out = run_controller(
        serial, ["--at", "bench_05", "--known-names", names_path],
        wait_for=start_replay5, delay=0.3)
    check_true("a run writes the live names it saw", os.path.exists(names_path),
               names_path)
    with open(names_path, encoding="utf-8") as handle:
        written = {line.strip() for line in handle if line.strip()}
    # Self-maintaining: the list the next run validates against is exactly the
    # set of names that could have triggered this one.
    check("the written list is the suite it saw", written, set(SUITE))
    check_true("and excludes the pre-marker self-test",
               "self_test_nop" not in written, sorted(written)[:5])


# --------------------------------------------------------------------------
# 7. The host-side witness
# --------------------------------------------------------------------------
# The probe answers "when was the host actually busy", which is the question
# both void runs turned on and neither could answer.  Its *arithmetic* is
# tested here on synthetic samples, deterministically; that it inflates under
# a real load is a property of the host, checked by the run itself.
print("host canary")

check("no samples means no summary", cl.summarise_probe([], 1.0, 2.0), None)

# t < 1.0 is idle, 1.0 <= t < 2.0 is loaded, t >= 2.0 is after the release.
synthetic = ([(0.1, 0.001), (0.5, 0.001), (0.9, 0.003)]
             + [(1.1, 0.004), (1.5, 0.002), (1.9, 0.004)]
             + [(2.1, 0.001), (2.5, 0.001)])
summary = cl.summarise_probe(synthetic, 1.0, 2.0)
check("idle median comes only from before the load",
      summary["idle_median_s"], 0.001)
check("loaded median comes only from the window",
      summary["loaded_median_s"], 0.004)
check("inflation is the ratio of the two", summary["inflation"], 4.0)
check("every sample is counted", summary["samples"], 8)

# A load that never fired has no loaded side, and must not be reported as
# "made no difference" -- that would be an exoneration nobody measured.
never = cl.summarise_probe([(0.1, 0.001), (0.5, 0.001)], None, None)
check("a load that never fired has no inflation", never["inflation"], None)
check("but its idle median is still reported", never["idle_median_s"], 0.001)
# Same for a window so short that no sample landed inside it: the honest
# answer is "not measured", never 1.0.
empty_window = cl.summarise_probe([(0.1, 0.001), (2.5, 0.001)], 1.0, 2.0)
check("a window no sample fell into yields no inflation",
      empty_window["inflation"], None)


with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")

    def start_replay6():
        replay(serial, SUITE, per_line=0.02).join()

    record, rc, out = run_controller(
        serial, ["--at", "bench_10", "--until", "bench_20"],
        wait_for=start_replay6, delay=0.3)

    check_true("the run records host-canary samples",
               len(record["host_probe_samples"]) > 5,
               f"{len(record['host_probe_samples'])} samples")
    stamps = [t for t, _ in record["host_probe_samples"]]
    check_true("samples are in time order", stamps == sorted(stamps),
               "out of order")
    # The join that makes the ground truth measurable: every benchmark
    # completion carries a timestamp on the same clock as the probe.
    times = [t for _, t in record["completions"]]
    check("every completion is timestamped",
          len(times), record["completions_seen"])
    check_true("completion times are in order", times == sorted(times),
               "out of order")
    check_true("the load's own on/off instants are on that clock too",
               record["fired_at"] is not None
               and record["released_at"] is not None
               and record["fired_at"] < record["released_at"],
               f"{record['fired_at']} -> {record['released_at']}")
    # The window's benchmarks must fall between the two instants, to within
    # the instrument's own resolution.
    #
    # The tolerance is one poll interval and is not slack: a completion is
    # stamped when the poll *saw* it, and the trigger line is seen in the same
    # batch as any line that arrived just after it, so those share a stamp
    # fractionally *below* `fired_at`.  Asserting strict containment would be
    # asserting a precision the controller does not have -- exactly the kind
    # of claim this experiment keeps being burned by.  Propagating the
    # uncertainty explicitly is the honest version.
    poll = record["poll_seconds"]
    during = dict(record["completions"])
    inside = [during[n] for n in record["during_names"] if n in during]
    check_true("the window's completions lie inside the load's interval",
               all(record["fired_at"] - poll <= t
                   <= record["released_at"] + poll for t in inside),
               f"fired {record['fired_at']}, released "
               f"{record['released_at']}, poll {poll}, times {inside}")
    check_true("and none of them precedes the trigger by more than one poll",
               all(t >= record["fired_at"] - poll for t in inside),
               f"fired {record['fired_at']}, times {inside}")


print()
if FAILURES:
    print(f"{len(FAILURES)} FAILURE(S)")
    for failure in FAILURES:
        print(f"  - {failure}")
    sys.exit(1)
print("all canary-load tests passed")
