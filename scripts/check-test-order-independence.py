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
# Sweeping all 415 would cost minutes for very little, so this is a list -- but
# it is no longer a REMEMBERED list. `scripts/raced-globals.py --check` now
# compares it against the crates it finds with a global reached by two or more
# tests, and fails if one is missing. That cross-check exists because this
# comment used to end "add a crate here the first time it grows a shared-state
# test", and on 2026-09-14 `userspace/authlib` grew one, was serialised the
# same day, and was not added. The rule survived; nobody executed it.
#
# Three fields, because two of them stopped being the same string once the
# list grew past `posix`:
#
#   pkg    what `cargo test -p` wants. `authlib`, not `userspace/authlib`.
#   dir    where the crate lives, for the self-test's existence check.
#   extra  cargo arguments narrowing the run.
#
# `coreutils` is scoped to `--lib` and that is a measurement, not a shortcut:
# its one qualifying global is `DIAGNOSTIC_LOST` in `src/stdfd.rs`, a lib
# global with two lib tests, and raced-globals finds no shared state in any of
# the 83 binaries. Unscoped it would build and shuffle all of them -- about
# twenty minutes an order, an hour for the three -- to re-shuffle 353 lib
# tests that take seventeen seconds.
#
# `vkloader` joined on 2026-09-24, found by that cross-check the day its
# detector stopped misreading it: `gui/vulkan/src/messenger.rs`'s tests share
# six statics under an RAII spin lock taken as `Order::lock()`, which
# raced-globals did not recognise as a lock until then. Serialised is not
# order-independent -- each test resets under the lock, which is what makes it
# safe to shuffle, and shuffling is what keeps it so. `--lib`: the statics are
# in the library's own test module.
CRATES = (
    ("posix", "posix", ()),
    ("authlib", "userspace/authlib", ()),
    ("coreutils", "userspace/coreutils", ("--lib",)),
    ("vkloader", "gui/vulkan", ("--lib",)),
)

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


def run_seed(crate, seed, target, extra=()):
    """Run one crate's tests in the order `seed` picks.

    Returns `(ok, failing_test_names, raw_output)`.
    """
    cmd = [
        "cargo", "+nightly", "test", "-p", crate,
        "--target", target, *extra, "--",
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
    for pkg, d, _extra in CRATES:
        ck(os.path.isdir(os.path.join(ROOT, d)),
           "crate directory should exist: " + d)
        # And `pkg` must be what `-p` will match, which is NOT always the last
        # component of `dir`. Reading it back from the manifest is the only
        # way this stays true: a wrong name makes cargo answer "did not match
        # any packages", which exits non-zero with no test result -- caught by
        # run_seed's `ran` check as "the suite did not run", ten minutes into
        # a push rather than here.
        manifest = os.path.join(ROOT, d, "Cargo.toml")
        declared = None
        if os.path.isfile(manifest):
            with open(manifest, encoding="utf-8", errors="replace") as fh:
                m = re.search(r'^\s*name\s*=\s*"([^"]+)"', fh.read(), re.M)
            declared = m.group(1) if m else None
        ck(declared == pkg,
           "package name for " + d + " should be " + pkg
           + ", manifest says " + str(declared))

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    ap.add_argument("--crate", action="append", default=None,
                    help="crate to check (repeatable; defaults to "
                         + ", ".join(p for p, _d, _e in CRATES) + ")")
    ap.add_argument("--target", default=HOST_TARGET)
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    if args.crate:
        # An explicit --crate names a package. Keep this list's narrowing
        # arguments if it is one of ours, so `--crate coreutils` by hand is
        # the same run the gate does rather than a silently larger one.
        byname = {pkg: extra for pkg, _d, extra in CRATES}
        crates = tuple((c, byname.get(c, ())) for c in args.crate)
    else:
        crates = tuple((pkg, extra) for pkg, _d, extra in CRATES)
    seeds = seeds_for_run()
    failed = []
    for crate, extra in crates:
        for seed in seeds:
            ok, failures, _out = run_seed(crate, seed, args.target, extra)
            if ok:
                if not args.quiet:
                    print("order-independence: " + crate + " seed " + str(seed) + " OK")
                continue
            failed.append((crate, seed, failures, extra))
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
        crate, seed, _, extra = failed[0]
        print("Reproduce exactly -- the seed is a constant, so this is the same "
              "order on any machine:", file=sys.stderr)
        print("    cargo +nightly test -p " + crate + " --target " + args.target
              + ("".join(" " + e for e in extra))
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
