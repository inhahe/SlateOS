#!/usr/bin/env python3
"""Refuse a test suite whose result depends on the order the tests run in.

`cargo test` runs a crate's tests in one fixed order -- alphabetical by path --
on every machine, every time. That is a single sample from 20,745! possible
orders, and it is the same sample forever, so a test that only fails when it
runs after some other test passes here indefinitely.

WHAT THAT COST, twice, in code this gate now covers:

    stdio.rs   `StdStreamTestGuard`'s own doc records it: `wchar::tests::
               test_fputwc_ascii` running before `stdio::tests::
               test_fclose_stdout_flushes_only` failed it 100% of the time.
               Under `--shuffle` that surfaced as a 26-in-30 flake; in the
               default order it was invisible.

    sysv_msg   Five `MSG_COPY` tests added on 2026-09-13 allocated from the
               8-slot queue pool without the `with_clean()` helper that resets
               it, leaking a slot each. `msgget` then returned -1 for whichever
               test asked ninth, which alphabetically was nobody and under
               shuffle was `msg_copy_without_nowait_is_einval`. Found by
               running this by hand the same day; nothing standing would have.

SEEDS: ONE PINNED, THE REST RANDOM -- AND THE FIRST DESIGN WAS WRONG.

This gate was first written with three fixed seeds, on the reasoning that a
random order "fails on a different test each time and trains everyone to
re-push rather than look". That argument sounds right and is false, and the
gate was probed before shipping rather than after:

  * Reintroducing the sysv_msg queue-slot leak -- a real, reproducible order
    dependency -- left all three fixed seeds GREEN. Three orders out of
    20,745! is not a sample, and the gate would have reported "OK" while the
    exact bug that motivated it walked through.
  * A shuffle gate has no false positives. Every failure is a genuine order
    dependency, so a red run is never noise and re-pushing is never the right
    response. The flakiness that argues against random gates does not apply
    to one that can only be right.

So: one PINNED seed, which is the order that caught the sysv_msg leak and now
serves as its regression test, plus `RANDOM_ORDERS` fresh orders per run. The
pinned seed gives a known-catching case forever; the random ones widen coverage
every time anyone pushes. Both print their seed on failure, so any red run
reproduces exactly.

The cost is about 40 seconds against a warm build, measured 2026-09-13:
11-17 s per seed for `posix`'s 20,745 tests.

    python scripts/check-test-order-independence.py
    python scripts/check-test-order-independence.py --crate posix
    python scripts/check-test-order-independence.py --selftest
"""

import argparse
import os
import random
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NL = chr(10)

# The order that caught the sysv_msg queue-slot leak on 2026-09-13. Kept as a
# regression case: with those five `clean_pool_guard()` calls removed, this seed
# fails and the three arbitrary constants it replaced do not.
PINNED_SEEDS = (1789357352031162400,)

# Fresh orders per run. See the docstring for why random is right here: a
# failure is always a real order dependency, so the usual objection to
# non-deterministic gates does not apply.
RANDOM_ORDERS = 2

# The crates whose tests share process-global state and so can depend on order.
# `posix` is the whole libc; the userspace crates are single-purpose binaries
# whose tests rarely touch statics, and sweeping all 415 would cost minutes for
# very little. Add a crate here the first time it grows a shared-state test.
CRATES = ("posix",)

HOST_TARGET = "x86_64-pc-windows-gnu"

RESULT_RE = re.compile(r"^test result: (ok|FAILED)\.", re.M)
FAILED_NAME_RE = re.compile(r"^test (\S+) \.\.\. FAILED", re.M)


def seeds_for_run():
    """The pinned regression orders plus `RANDOM_ORDERS` fresh ones.

    Fresh orders are the point: the pinned seed only proves the one bug it was
    derived from stays fixed, while every push that runs this samples orders
    nobody has tried. A failure names its seed, so a random order that finds
    something becomes reproducible immediately -- and worth pinning here.
    """
    return list(PINNED_SEEDS) + [
        random.randrange(1, 2**63) for _ in range(RANDOM_ORDERS)
    ]


def run_seed(crate, seed, target):
    """Run one crate's tests in the order `seed` picks.

    Returns `(ok, failing_test_names, raw_output)`.
    """
    cmd = [
        "cargo", "+nightly", "test", "-p", crate,
        "--target", target, "--",
        "-Z", "unstable-options", "--shuffle-seed", str(seed),
    ]
    try:
        proc = subprocess.run(
            cmd, cwd=ROOT, capture_output=True, text=True, timeout=1800,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return False, ["<could not run: " + str(exc) + ">"], ""
    out = (proc.stdout or "") + (proc.stderr or "")
    failures = FAILED_NAME_RE.findall(out)
    # `proc.returncode` alone is not enough: a build failure and a test failure
    # both exit non-zero, and only one of them is this gate's business.
    ran = RESULT_RE.search(out) is not None
    if not ran:
        return False, ["<the suite did not run; see output>"], out
    return proc.returncode == 0, failures, out


def selftest():
    bad = 0
    checks = 0

    def ck(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    # The pinned seeds must be distinct integers: two equal ones would claim
    # two orders of coverage while sampling one.
    ck(len(set(PINNED_SEEDS)) == len(PINNED_SEEDS), "pinned seeds must be distinct")
    ck(all(isinstance(s, int) for s in PINNED_SEEDS), "pinned seeds must be integers")
    # The regression seed must still be here. Losing it would leave the gate
    # sampling only fresh orders, and the one order known to catch a real bug
    # in this tree would stop being checked.
    ck(1789357352031162400 in PINNED_SEEDS,
       "the sysv_msg regression order must stay pinned")
    ck(RANDOM_ORDERS >= 1,
       "with no random orders the gate only ever checks what already failed once")

    # seeds_for_run must include every pinned seed and add the random ones.
    a = seeds_for_run()
    b = seeds_for_run()
    ck(all(s in a for s in PINNED_SEEDS), "every pinned seed must be run")
    ck(len(a) == len(PINNED_SEEDS) + RANDOM_ORDERS,
       "the run should be pinned + random, got " + str(len(a)))
    ck(a != b, "two runs should pick different random orders")

    # The parsers must find a failure, and must not invent one.
    sample_fail = (
        "running 3 tests" + NL
        + "test a::b ... ok" + NL
        + "test c::d ... FAILED" + NL
        + "test result: FAILED. 2 passed; 1 failed;" + NL
    )
    ck(FAILED_NAME_RE.findall(sample_fail) == ["c::d"],
       "the failing test's name must be extracted")
    ck(RESULT_RE.search(sample_fail) is not None,
       "a FAILED summary must count as having run")

    sample_ok = (
        "running 2 tests" + NL
        + "test a::b ... ok" + NL
        + "test result: ok. 2 passed; 0 failed;" + NL
    )
    ck(FAILED_NAME_RE.findall(sample_ok) == [],
       "a clean run must yield no failing names")
    ck(RESULT_RE.search(sample_ok) is not None,
       "an ok summary must count as having run")

    # A build error is not a test failure, and must not be reported as one --
    # that is the difference between "your tests depend on order" and "your
    # code does not compile", and they want different fixes.
    sample_build_err = "error[E0425]: cannot find value `x`" + NL
    ck(RESULT_RE.search(sample_build_err) is None,
       "a build failure must not be mistaken for a test run")

    # The crate list must be real, or this gate grades nothing.
    for crate in CRATES:
        ck(os.path.isdir(os.path.join(ROOT, crate)),
           "crate directory should exist: " + crate)

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    ap.add_argument("--crate", action="append", default=None,
                    help="crate to check (repeatable; defaults to " + ", ".join(CRATES) + ")")
    ap.add_argument("--target", default=HOST_TARGET)
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    crates = tuple(args.crate) if args.crate else CRATES
    seeds = seeds_for_run()
    failed = []
    for crate in crates:
        for seed in seeds:
            ok, failures, _out = run_seed(crate, seed, args.target)
            if ok:
                if not args.quiet:
                    print("order-independence: " + crate + " seed " + str(seed) + " OK")
                continue
            failed.append((crate, seed, failures))
            print("", file=sys.stderr)
            print("check-test-order-independence: " + crate
                  + " FAILS in the order seed " + str(seed) + " produces:",
                  file=sys.stderr)
            for name in failures[:10]:
                print("    " + name, file=sys.stderr)
            if len(failures) > 10:
                print("    ... and " + str(len(failures) - 10) + " more",
                      file=sys.stderr)

    if failed:
        print("", file=sys.stderr)
        print("A test that passes in one order and fails in another is sharing "
              "state with another test. The usual causes, in the order they "
              "turn up in this tree:", file=sys.stderr)
        print("  * a process-global written by one test and read by another "
              "-- scripts/raced-globals.py finds these;", file=sys.stderr)
        print("  * a fixed-size pool (message queues, semaphores, FILE slots) "
              "that a test takes from and never returns;", file=sys.stderr)
        print("  * buffered output left on a standard stream, which is what "
              "stdio.rs's StdStreamTestGuard exists to purge.", file=sys.stderr)
        print("", file=sys.stderr)
        crate, seed, _ = failed[0]
        print("Reproduce exactly -- the seed is a constant, so this is the same "
              "order on any machine:", file=sys.stderr)
        print("    cargo +nightly test -p " + crate + " --target " + args.target
              + " -- -Z unstable-options --shuffle-seed " + str(seed),
              file=sys.stderr)
        return 1

    if not args.quiet:
        print("check-test-order-independence: OK -- " + str(len(crates))
              + " crate(s) pass in " + str(len(seeds)) + " orders ("
              + str(len(PINNED_SEEDS)) + " pinned, " + str(RANDOM_ORDERS)
              + " fresh) in addition to cargo's alphabetical one.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
