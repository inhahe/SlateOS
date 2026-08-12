#!/usr/bin/env bash
#
# flake-hunt.sh — run a crate's test suite N times and report only the runs
# that failed, with the names of the tests that failed.
#
# Why this exists
# ---------------
# Non-deterministic test failures are invisible to the normal workflow.  A
# suite that fails one run in six looks green five times out of six, so
# "cargo test passed" is not evidence of anything, and the failure gets
# misfiled as a flaky-CI shrug or a real regression in whatever unrelated
# module happened to lose the race that time.
#
# The `posix` crate had a whole class of these: state that is per-*process* in
# POSIX, stored in a `static mut`, mutated by tests — and libtest runs every
# test on its own thread inside one process, so tests silently shared it.  Six
# different tests across five modules were failing at ~1 run in 6, each looking
# like an unrelated one-off.  Running the suite 40 times in a row is what
# turned that into a list of names, and re-running it after each fix is what
# showed which conversions actually helped.  See known-issues.md
# TD-POSIX-TEST-SHARED-STATICS-REMAINING-TIER and design-decisions.md §110.
#
# Usage
# -----
#   scripts/flake-hunt.sh [runs] [package] [target]
#
#   scripts/flake-hunt.sh                       # 40 runs of posix, host target
#   scripts/flake-hunt.sh 100 posix             # 100 runs
#   scripts/flake-hunt.sh 20 net                # a different crate
#
# Exits non-zero if any run failed, so it can gate a commit.
#
# Note this rebuilds from the working tree on the first run only, so do NOT
# edit the crate while a hunt is in flight — a mid-hunt edit silently changes
# what is being measured partway through.
#
# Prefer running it under scripts/run-timeout.py: a deadlocked test never
# exits on its own, and a hunt of N runs is N chances to hit one.
#
#   python scripts/run-timeout.py --poll 120 2400 \
#       "C:/Program Files/Git/usr/bin/bash.exe" ./scripts/flake-hunt.sh 40

set -u

runs="${1:-40}"
pkg="${2:-posix}"
target="${3:-x86_64-pc-windows-gnu}"

echo "flake-hunt: $runs runs of '$pkg' on '$target'"

failed=0
for ((i = 1; i <= runs; i++)); do
    out=$(cargo test -p "$pkg" --target "$target" 2>&1)
    if echo "$out" | grep -q "test result: FAILED"; then
        failed=$((failed + 1))
        echo "=== RUN $i FAILED ==="
        # The test names live in the `failures:` block that libtest prints
        # after the per-test lines.
        echo "$out" | grep -E "^(test .* FAILED|failures:|    [a-z_]+)" | head -20
    fi
done

echo "TOTAL FAILED RUNS: $failed / $runs"
[ "$failed" -eq 0 ]
