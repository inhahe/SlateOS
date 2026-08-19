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


def run_controller(serial, extra, wait_for=None, delay=0.0, spinners=0):
    """Run the controller as a subprocess; return (record, returncode).

    `spinners` defaults to 0 so the suite does not saturate the machine it is
    running on: every test here is about the controller's bookkeeping, which
    is independent of how much CPU the spinners burn.  The one test that is
    *about* the burning passes a real count.
    """
    with tempfile.TemporaryDirectory() as tmp:
        record_path = os.path.join(tmp, "record.json")
        ready_path = os.path.join(tmp, "ready")
        stop_path = os.path.join(tmp, "stop")
        proc = subprocess.Popen(
            [sys.executable, CANARY_LOAD, "--serial", serial,
             "--spinners", str(spinners), "--timeout", "60",
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
    written, written_aliases, written_scored = cl.read_known_names(names_path)
    # Self-maintaining: the list the next run validates against is exactly the
    # set of names that could have triggered this one.
    check("the written list is the suite it saw", written, set(SUITE))
    check_true("and excludes the pre-marker self-test",
               "self_test_nop" not in written, sorted(written)[:5])
    check("a log with no MEASURED-AS lines records no aliases",
          written_aliases, {})


# --------------------------------------------------------------------------
print()
print("scorecard names")

# The trap that voided the second attempt: the caller passed a name off the
# end-of-run scorecard, the controller can only match live result lines, and
# nothing connected the two. The kernel now states the pairing outright
# (`MEASURED-AS`), because it is the only party that knows it -- see
# `ScoreEntry::seq` in kernel/src/bench.rs for why neither the log's ordering
# nor the source's structure can be made to yield it.
with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")
    with open(serial, "w", encoding="utf-8") as handle:
        handle.write(
            "[bench] vfs_write_16k: min=100 cycles (27ns), mean=1 cycles\n"
            "[bench] SCORE vfs_throughput_16k_write 27 50000 PASS 30 100 0\n"
            "[bench] MEASURED-AS vfs_throughput_16k_write vfs_write_16k\n"
            "[bench] MEASURED-AS lock_uncontended lock_tracked\n"
            "[bench] not a measured-as line at all\n")
    found = cl.read_measured_as(serial)
    check("both pairings are read back", found,
          {"vfs_throughput_16k_write": "vfs_write_16k",
           "lock_uncontended": "lock_tracked"})
check("a serial that does not exist yields no pairings",
      cl.read_measured_as(os.path.join(tempfile.gettempdir(), "no-such")), {})

# The real second-attempt log, from a kernel that predates the line. It must
# read back as "no pairings known", not as an error -- that is exactly the
# state a first run after this change is in.
_real_serial = os.path.join(REPO_ROOT, "build", "p22-run2-serial.txt")
if os.path.exists(_real_serial):
    check("the pre-change run 2 log simply has no pairings",
          cl.read_measured_as(_real_serial), {})

with tempfile.TemporaryDirectory() as tmpdir:
    names_path = os.path.join(tmpdir, "names.txt")
    cl.write_known_names(names_path, ["b_one", "b_two"],
                         {"scored_two": "b_two"})
    names, aliases, _ = cl.read_known_names(names_path)
    check("names survive the round trip", names, {"b_one", "b_two"})
    check("and so do the pairings", aliases, {"scored_two": "b_two"})

    # A file written before pairings existed must still read as a name list.
    with open(names_path, "w", encoding="utf-8") as handle:
        handle.write("b_one\nb_two\n")
    names, aliases, _ = cl.read_known_names(names_path)
    check("an older names file still reads as names", names, {"b_one", "b_two"})
    check("...with no pairings rather than an error", aliases, {})

# End to end: the scorecard name is accepted and the substitution is stated.
# Announcing it is not politeness. A silent substitution is the same class of
# fault as the silent non-match that voided the second attempt -- the window
# ends up somewhere the label does not say, and nothing in the output admits it.
rc, out = run_validation(["--at", "scored_05", "--check-names-only"],
                         SUITE + ["alias scored_05 bench_05"])
check("a scorecard name is translated, not refused", rc, 0)
check_true("and the translation is stated",
           "scored_05" in out and "bench_05" in out, out[-400:])

rc, out = run_validation(["--at", "scored_no_such", "--check-names-only"],
                         SUITE + ["alias scored_05 bench_05"])
check("a name that is neither live nor a known scorecard name is still "
      "refused", rc, 2)

# The mirror-image trap. `bench_05` is a fine live name -- the load would fire
# exactly where asked -- but if it never reaches the scorecard the grader
# cannot place a window with it, and that is only discovered after the boot.
_graded = SUITE + ["alias scored_05 bench_05", "scored scored_05",
                   "scored bench_09"]
rc, out = run_validation(["--at", "bench_05", "--check-names-only",
                          "--require-scored"], _graded)
check("a live-only name is refused when the run will be graded", rc, 2)
check_true("and the caller is told the name it should have passed",
           "scored_05" in out, out[-500:])
rc, out = run_validation(["--at", "bench_05", "--check-names-only"], _graded)
check("...but stands on its own without --require-scored", rc, 0)
rc, out = run_validation(["--at", "scored_05", "--check-names-only",
                          "--require-scored"], _graded)
check("a name good in both namespaces passes the graded check", rc, 0)
rc, out = run_validation(["--at", "bench_09", "--check-names-only",
                          "--require-scored"], _graded)
check("a name that needs no translation passes it too", rc, 0)

with tempfile.TemporaryDirectory() as tmpdir:
    names_path = os.path.join(tmpdir, "names.txt")
    cl.write_known_names(names_path, ["b_one"], {"s_one": "b_one"}, {"s_one"})
    _, _, scored_back = cl.read_known_names(names_path)
    check("scorecard names survive the round trip", scored_back, {"s_one"})

with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")
    with open(serial, "w", encoding="utf-8") as handle:
        handle.write("[bench] SCORE page_alloc_free 601 1000 PASS 1319 500 16\n"
                     "[bench] SCORE page_alloc_zeroed_free 3592 - TRACK 3791 5 0\n"
                     "[bench] b_one: min=1 cycles\n")
    check("both graded and tracked entries count as scored",
          cl.read_scored_names(serial),
          {"page_alloc_free", "page_alloc_zeroed_free"})


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
check("every sample is counted", summary["samples"], 8)

# `inflation` is the *best-case* ratio, not the median one.  RESULT P22: the
# median's run-to-run spread on this host (1.95x over 12 trials) is wider than
# the effect it is meant to detect, and it read x0.78 both for a correctly
# loaded run and for a control with literally zero spinners.  The median is
# still reported, but demoted and labelled.
check("inflation is the best-case ratio", summary["inflation"], 2.0)
check("the median ratio is kept, but separately",
      summary["median_inflation"], 4.0)
check_true("and it is labelled unreliable",
           "unreliable" in summary["median_inflation_note"],
           summary["median_inflation_note"])
check("best-case idle is the mean of the fastest quarter",
      summary["idle_best_s"], 0.001)
check("best-case loaded likewise", summary["loaded_best_s"], 0.002)

# The best-case statistic must be the *stable* one: a single wild outlier in
# the window moves the median but must barely touch it.
outlier = ([(0.1, 0.001)] * 8
           + [(1.0 + 0.1 * i, 0.001) for i in range(7)] + [(1.8, 0.500)])
spiked = cl.summarise_probe(outlier, 1.0, 2.0)
check_true("one outlier does not move the best-case ratio",
           abs(spiked["inflation"] - 1.0) < 0.01, spiked["inflation"])

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


# --------------------------------------------------------------------------
# 7b. Occupancy -- the direct measurement that replaces the inference
# --------------------------------------------------------------------------
# The canary above describes what the host felt; this says what the spinners
# actually did.  It is the figure that answers "was the load applied", and it
# exists because the canary demonstrably cannot: on a 12-core host six
# spinners leave the probe a free core.
print()
print("spinner occupancy")

check("no snapshots means no measurement",
      cl.summarise_occupancy(None, None, 3.0), None)
check("mismatched snapshots are refused",
      cl.summarise_occupancy([0.0], [1.0, 2.0], 3.0), None)
check("a zero-length window yields no ratio",
      cl.summarise_occupancy([0.0], [1.0], 0)["occupancy"], None)

full = cl.summarise_occupancy([0.0, 0.0, 0.0, 0.0],
                              [3.0, 3.0, 3.0, 3.0], 3.0)
check("four spinners with a core each score 1.0", full["occupancy"], 1.0)
check("their burned CPU is the snapshot delta",
      full["cpu_seconds_total"], 12.0)
check("expected CPU is spinners x window", full["expected_cpu_seconds"], 12.0)
check("none of them counts as idle", full["idle_spinners"], 0)

# The failure this exists to catch: processes that were nominally started but
# never actually ran.  A standalone probe once did exactly this and reported a
# confident ratio with zero live spinners.
dead = cl.summarise_occupancy([0.0] * 4, [0.0] * 4, 3.0)
check("spinners that never ran score 0", dead["occupancy"], 0.0)
check("and every one is named as idle", dead["idle_spinners"], 4)
check_true("which is below the floor", dead["occupancy"] < cl.OCCUPANCY_FLOOR)

half = cl.summarise_occupancy([0.0] * 4, [3.0, 3.0, 0.0, 0.0], 3.0)
check("partial scheduling is reported proportionally",
      half["occupancy"], 0.5)
check("and the starved ones are counted", half["idle_spinners"], 2)

# A CPU clock cannot run backwards; if a snapshot pair says it did, the
# reading is garbage and must not become negative "credit" that masks another
# spinner's idleness.
backwards = cl.summarise_occupancy([5.0, 0.0], [0.0, 3.0], 3.0)
check("a backwards clock contributes nothing rather than a negative",
      backwards["cpu_seconds_total"], 3.0)


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

    # This run asked for no spinners, so there is nothing to have occupied a
    # core.  The honest report is "not measured", never a reassuring number.
    check("a zero-spinner run reports no occupancy",
          record["host_occupancy"], None)
    check_true("and is not accused of failing to apply a load it never had",
               record.get("problem") is None, record.get("problem"))


# --------------------------------------------------------------------------
# 7c. Occupancy, end to end, with spinners that really run
# --------------------------------------------------------------------------
# The only test here that burns real CPU, and the only one that can: whether
# a spinner gets scheduled is a fact about the operating system, not about
# this script's bookkeeping, so it cannot be established synthetically.
print()
print("spinner occupancy (live)")

with tempfile.TemporaryDirectory() as tmpdir:
    serial = os.path.join(tmpdir, "serial.txt")

    def start_replay7():
        replay(serial, SUITE, per_line=0.02).join()

    record, rc, out = run_controller(
        serial, ["--at", "bench_10", "--until", "bench_20"],
        wait_for=start_replay7, delay=0.3, spinners=2)

    occ = record["host_occupancy"]
    check_true("a loaded run measures spinner occupancy", occ is not None,
               f"record problem={record.get('problem')!r}")
    if occ is not None:
        check("one CPU figure per spinner",
              len(occ["cpu_seconds"]), record["spinners"])
        check("no spinner was starved", occ["idle_spinners"], 0)
        check_true("occupancy clears the floor",
                   occ["occupancy"] >= cl.OCCUPANCY_FLOOR,
                   f"occupancy {occ['occupancy']}")
        # Upper bound too: a spinner cannot burn more CPU than wall time, so
        # a figure far above 1.0 would mean the window or the clocks are
        # wrong.  The slack absorbs the ~15.6 ms granularity of the Windows
        # CPU clock against a window of well under a second.
        check_true("and does not exceed what wall time allows",
                   occ["occupancy"] <= 2.0, f"occupancy {occ['occupancy']}")
    check_true("a correctly-loaded run is not flagged as unapplied",
               record.get("problem") is None, record.get("problem"))
    check_true("the summary states the occupancy in words",
               "spinner occupancy" in out, out[-400:])


print()
print("wrapper argument parsing")

# The wrapper (`canary-load-test.sh`) is what a person actually types, and until
# 2026-08-19 it accepted only `--load-at=NAME` while the controller it drives
# takes `--at NAME`.  Two pre-registered experiments in known-issues.md quoted
# the controller's spelling as the command to run, so the published command for
# each did not parse -- and nothing in the repository could reveal that without
# booting QEMU for twelve minutes, because the wrapper's first act is to build
# and boot.
#
# `--dry-run` exists to make that reachable, and these tests are the reason it
# has to keep working: they are the only thing standing between a renamed flag
# and a registered experiment that silently cannot be run.  They cost about a
# second because a dry run stops before the build.

# Repo-relative, and passed with `cwd=REPO_ROOT`, because MSYS bash does not
# accept a Windows-style absolute path as its script argument -- `bash
# 'D:\...\canary-load-test.sh'` fails with "No such file or directory", which
# reads like a missing file rather than a path-flavour mismatch.
CANARY_LOAD_TEST = "scripts/canary-load-test.sh"


def find_msys_bash():
    """Git-for-Windows bash, located explicitly rather than taken from PATH.

    **`bash` on the Windows PATH is WSL's**, at `C:\\Windows\\System32\\bash.exe`,
    and `shutil.which("bash")` finds that one. It runs, so a test that used it
    would look like it worked -- while actually exercising a Linux environment
    with a `/mnt/d/...` view of the disk, no MSVC, and none of the toolchain the
    script is written against. Every shell script in this repo is run under
    Git/MSYS bash, so testing them under WSL would be testing something the
    project never executes.

    Candidates are *verified*, not assumed: `uname -o` must say `Msys`. A guess
    that silently fell back to WSL is exactly the failure this function exists
    to prevent, so an unverifiable candidate is skipped rather than used.
    """
    candidates = []
    override = os.environ.get("MSYS_BASH")
    if override:
        candidates.append(override)
    # Git bash usually reaches the Windows PATH via its own `usr\bin` or
    # `mingw64\bin`; both sit beside a `bin\bash.exe` under the install root.
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        low = entry.lower().replace("/", "\\")
        for marker in ("\\git\\usr\\bin", "\\git\\mingw64\\bin", "\\git\\bin"):
            if low.endswith(marker):
                root = entry
                for _ in range(marker.count("\\")):
                    root = os.path.dirname(root)
                candidates.append(os.path.join(root, "bin", "bash.exe"))
                candidates.append(os.path.join(root, "usr", "bin", "bash.exe"))
    candidates += [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ]
    seen = set()
    for candidate in candidates:
        key = os.path.normcase(os.path.abspath(candidate))
        if key in seen or not os.path.exists(candidate):
            continue
        seen.add(key)
        try:
            probe = subprocess.run([candidate, "-c", "uname -o"],
                                   capture_output=True, text=True, timeout=60)
        except OSError:
            continue
        if probe.returncode == 0 and "msys" in probe.stdout.strip().lower():
            return candidate
    return None


BASH = find_msys_bash()

# Announced rather than assumed. If this ever names WSL's bash the whole section
# is measuring the wrong shell, and a silent substitution is precisely the class
# of error these tests exist to catch elsewhere.
if BASH is None:
    check_true("Git/MSYS bash was found", False,
               "no bash.exe verified as MSYS; set MSYS_BASH to the right one. "
               "`bash` on PATH is WSL's and would test a different environment.")
else:
    print(f"  ..   using {BASH}")


def wrapper_env():
    """The environment the wrapper needs, with this interpreter reachable.

    A bash spawned from *this* process does not necessarily find `python` on
    PATH even though the bash that launched this process did -- the MSYS/Windows
    PATH round-trip does not preserve it. The wrapper calls `python` for its
    name validation, so without this the accepting cases fail with
    "python: command not found" and rc=2, which is indistinguishable from the
    wrapper *rejecting* the arguments -- every rejection test would still pass
    while every acceptance test failed for an unrelated reason.

    Pinning the running interpreter also means the wrapper is tested against the
    same Python the tests run under rather than whichever one happens to be
    first on PATH.
    """
    env = os.environ.copy()
    env["PATH"] = os.path.dirname(sys.executable) + os.pathsep + env.get("PATH", "")
    return env


def run_wrapper(args):
    """`canary-load-test.sh <args> --dry-run` -> (returncode, stdout+stderr).

    With no MSYS bash this returns a sentinel rather than raising, so the
    section reports one clear cause followed by ordinary failures instead of a
    traceback that hides which checks would have run.
    """
    if BASH is None:
        return None, "no MSYS bash available"
    proc = subprocess.run(
        [BASH, CANARY_LOAD_TEST, *args, "--dry-run"],
        capture_output=True, text=True, cwd=REPO_ROOT, timeout=120,
        env=wrapper_env(),
    )
    return proc.returncode, proc.stdout + proc.stderr


def read_name_file():
    """`build/canary-load-names.txt` -> `(live, aliases, scored)`, or None.

    Parsed with `canary-load.py:read_known_names`'s exact rules, because the
    point is to predict *its* verdict. A bare line is a live name, `alias
    <scored> <live>` is a pairing, `scored <name>` is a scorecard name.
    """
    path = os.path.join(REPO_ROOT, "build", "canary-load-names.txt")
    if not os.path.exists(path):
        return None
    live, aliases, scored = set(), {}, set()
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
                live.add(parts[0])
    return live, aliases, scored


NAMES = read_name_file()


def usable_bound(name):
    """Would the wrapper's name guard accept `name` as a window bound?

    The guard is three stages and a name must survive all of them: it must be
    a *scored* name (the grader can only place a window with those), it is then
    alias-translated, and the result must be a *live* name (the controller can
    only trigger on those). A name can be perfectly real and fail either --
    `io_ring_nop_submit` is live but never scored, `isr_latency` is scored but
    has no live line at all.

    With no name file the wrapper skips validation entirely, so everything is
    "usable" and the parser tests still mean what they say.
    """
    if NAMES is None:
        return True
    live, aliases, scored = NAMES
    if not live or not scored:
        return True
    return name in scored and aliases.get(name, name) in live


def pick_window():
    """Two window bounds the name guard will accept, in suite order.

    Hardcoding names here was a mistake once already: the fixture paired
    `crypto_x25519` (fine) with `isr_latency` (scored, but with no live result
    line), so five *parser* tests failed inside the *name guard* -- and the
    failure output was a list of nearest-name suggestions, which reads like a
    typo in the script under test rather than in the test's own data.

    The names file is rewritten by every run, so any hardcoded pair is a
    standing bet on the suite's composition that this section has no reason to
    be making. P23's registered pair is preferred when it is still valid --
    it keeps the assertions legible and is the pair the section most wants to
    protect -- and otherwise a valid pair is derived.
    """
    preferred = ("io_ring_nop", "crypto_poly1305_1KiB")
    if all(usable_bound(name) for name in preferred):
        return preferred
    if NAMES is None:
        return preferred
    _, _, scored = NAMES
    usable = sorted(name for name in scored if usable_bound(name))
    if len(usable) >= 2:
        return usable[0], usable[-1]
    return preferred


WINDOW_AT, WINDOW_UNTIL = pick_window()
print(f"  ..   window fixtures: --at {WINDOW_AT} --until {WINDOW_UNTIL}")

# The name guard is a *precondition* of every accepting case below, not a thing
# they test. Asserting it separately means a suite change that invalidates a
# fixture reports itself as exactly that, instead of as five parser failures.
check_true("the window fixtures are names the guard accepts",
           usable_bound(WINDOW_AT) and usable_bound(WINDOW_UNTIL),
           f"--at {WINDOW_AT} usable={usable_bound(WINDOW_AT)}, "
           f"--until {WINDOW_UNTIL} usable={usable_bound(WINDOW_UNTIL)}")

# Accepted spellings. Every one of these must reach the same window, because the
# whole point is that a reader can paste whichever form a document happens to
# use.  `--dry-run` echoes the window it parsed, which is what is asserted on.
WANT_WINDOW = f"from '{WINDOW_AT}' until '{WINDOW_UNTIL}'"
for label, args in [
    ("the wrapper's own documented spelling",
     [f"--load-at={WINDOW_AT}", f"--load-until={WINDOW_UNTIL}"]),
    ("the controller's spelling, as quoted by P22 and P23",
     ["--at", WINDOW_AT, "--until", WINDOW_UNTIL]),
    ("the alias with =",
     [f"--at={WINDOW_AT}", f"--until={WINDOW_UNTIL}"]),
    ("the long flags space-separated",
     ["--load-at", WINDOW_AT, "--load-until", WINDOW_UNTIL]),
    ("mixed spellings in one invocation",
     [f"--at={WINDOW_AT}", "--load-until", WINDOW_UNTIL]),
]:
    rc, out = run_wrapper(args)
    check_true(label, rc == 0 and WANT_WINDOW in out,
               f"rc={rc} out={out[-300:]}")

# The spinner count, in each of its three forms.
for label, args, want in [
    ("a bare number still sets the spinner count", ["4"], "4 spinners"),
    ("--spinners=N", ["--spinners=3"], "3 spinners"),
    ("--spinners N", ["--spinners", "3"], "3 spinners"),
]:
    rc, out = run_wrapper(args)
    check_true(label, rc == 0 and want in out, f"rc={rc} out={out[-300:]}")

# Rejections. Each of these would otherwise produce a *plausible-looking* run
# that answers a different question than the one asked -- which is worse than a
# crash, because it is only detectable 12 minutes later at the grading step, and
# only by someone who remembers what they meant to ask.
#
# Each case asserts the *reason* as well as the exit code. `rc == 2` alone is
# not evidence the parser rejected anything: the wrapper also exits 2 from its
# name guard, from a failed build, and from a `python` it could not find. This
# section spent a debugging cycle on exactly that ambiguity in reverse -- five
# acceptance tests failing at rc=2 for a reason that had nothing to do with what
# they were testing -- and a rejection test that passes for the wrong reason is
# the same defect with the sign flipped, only harder to notice because green.
for label, args, want in [
    ("an empty --load-at is refused", ["--load-at="],
     "--load-at needs a benchmark name"),
    ("an empty --at is refused", ["--at="],
     "--load-at needs a benchmark name"),
    ("an empty --load-until is refused", ["--load-until="],
     "--load-until needs a benchmark name"),
    ("--at followed by another flag is refused", ["--at"],
     "--load-at needs a benchmark name"),
    ("--until followed by another flag is refused",
     ["--at", WINDOW_AT, "--until"], "--load-until needs a benchmark name"),
    ("--spinners followed by another flag is refused", ["--spinners"],
     "--spinners needs a count"),
    ("a non-numeric spinner count is refused", ["--spinners=six"],
     "--spinners needs a non-negative integer"),
    ("--load-until without --load-at is refused", ["--until", WINDOW_UNTIL],
     "--load-until requires --load-at"),
    ("an unknown flag is refused", [f"--load-during={WINDOW_AT}"],
     "unknown argument: --load-during"),
]:
    rc, out = run_wrapper(args)
    check_true(label, rc == 2 and want in out,
               f"rc={rc} wanted {want!r} in output; out={out[-300:]}")

# The genuinely-trailing case, with nothing after the flag at all. These run
# *without* `--dry-run` appended -- which is only safe because a value-less flag
# is rejected in the argument loop, before the build and before any file is
# touched. That is precisely the claim being tested: if one of these ever
# stopped erroring, the wrapper would fall through to a real 12-minute boot, and
# this test would be the thing that noticed.
for label, args, want in [
    ("--at as the final argument is refused", ["--at"],
     "--load-at needs a benchmark name"),
    ("--until as the final argument is refused", ["--at", WINDOW_AT, "--until"],
     "--load-until needs a benchmark name"),
    ("--spinners as the final argument is refused", ["--spinners"],
     "--spinners needs a count"),
]:
    if BASH is None:
        rc, out = None, "no MSYS bash available"
    else:
        proc = subprocess.run([BASH, CANARY_LOAD_TEST, *args],
                              capture_output=True, text=True, cwd=REPO_ROOT,
                              timeout=120, env=wrapper_env())
        rc, out = proc.returncode, proc.stdout + proc.stderr
    check_true(label, rc == 2 and want in out,
               f"rc={rc} wanted {want!r} in output; out={out[-300:]}")

# A dry run must leave the previous experiment's evidence alone. It exits before
# the `rm -f "$SERIAL_FILE"` that a real run needs, and if that ever stops being
# true a dry run would destroy the log of the last completed experiment -- the
# one artefact that cannot be regenerated without another 12-minute boot.
serial_path = os.path.join(REPO_ROOT, "build", "serial-test.txt")
before = os.path.exists(serial_path)
before_size = os.path.getsize(serial_path) if before else None
run_wrapper(["--at", WINDOW_AT, "--until", WINDOW_UNTIL])
check_true("a dry run does not delete the previous run's serial log",
           os.path.exists(serial_path) == before
           and (not before or os.path.getsize(serial_path) == before_size),
           f"existed={before} size={before_size}")

# And the registered P23 command itself, verbatim from known-issues.md. This is
# the specific regression that motivated all of the above: if this line stops
# parsing, a pre-registered experiment has become unrunnable as published.
rc, out = run_wrapper(["--at", "io_ring_nop", "--until", "crypto_poly1305_1KiB"])
check_true("PREDICTION P23's registered command parses",
           rc == 0 and "from 'io_ring_nop' until 'crypto_poly1305_1KiB'" in out,
           f"rc={rc} out={out[-300:]}")

# The two-namespace trap, from both sides, on names taken from the current file
# rather than assumed. Both of these are *real benchmarks* that a reader could
# reasonably pass, and both are unusable as window bounds -- which is only
# tolerable because the guard says so in a second instead of after a boot.
if NAMES is not None:
    live, aliases, scored = NAMES
    # Scored but with no live result line to trigger on. Three of the suite's
    # 86 scorecard names are in this state (`context_switch`, `ipc_channel_sync`,
    # `isr_latency`): they are not aliased to anything, so there is no name a
    # caller could pass that would work. Rejecting is correct; being *told* why
    # is what makes it cheap.
    orphans = sorted(n for n in scored if n not in aliases and n not in live)
    if orphans:
        rc, out = run_wrapper(["--at", orphans[0], "--until", WINDOW_UNTIL])
        check_true("a scored name with no live result line is refused by name",
                   rc == 2 and orphans[0] in out
                   and "not a live benchmark name" in out,
                   f"orphan={orphans[0]} rc={rc} out={out[-300:]}")
    # Live but never scored -- the load would fire exactly where asked and the
    # grader would then refuse to place the window, wasting the whole run. The
    # guard must catch this *before* the boot, and must name the scorecard
    # spelling to pass instead rather than leaving the caller to guess.
    unscored = sorted(n for n in live if n not in scored and n in aliases.values())
    if unscored:
        back = next(s for s, lv in aliases.items() if lv == unscored[0])
        rc, out = run_wrapper(["--at", unscored[0], "--until", WINDOW_UNTIL])
        check_true("a live name that never reaches the scorecard is refused, "
                   "naming the spelling to use instead",
                   rc == 2 and "never reaches the scorecard" in out
                   and back in out,
                   f"live={unscored[0]} want-suggestion={back} "
                   f"rc={rc} out={out[-300:]}")


print()
if FAILURES:
    print(f"{len(FAILURES)} FAILURE(S)")
    for failure in FAILURES:
        print(f"  - {failure}")
    sys.exit(1)
print("all canary-load tests passed")
