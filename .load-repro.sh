#!/bin/bash
# Reproduce lane C's `wait_n_ignores_...` failure by recreating the CONDITION
# they reported it under: many test binaries competing for one machine.
#
# Everything started here is waited for in the same script, so nothing outlives
# it and no process outside it is touched. The loaders are copies of this
# crate's own test binary -- real work of the right shape, rather than a spin
# loop that only burns CPU without the scheduler pressure and allocator traffic
# a test binary produces.
set -u
EXE="target/x86_64-pc-windows-gnu/debug/deps/osh-18718e50121aa213.exe"
T=interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported
LOADERS=${1:-8}
ITERS=${2:-25}

if [ ! -x "$EXE" ]; then
    echo "no test binary at $EXE" >&2
    exit 2
fi

pids=()
for _ in $(seq 1 "$LOADERS"); do
    "$EXE" >/dev/null 2>&1 &
    pids+=($!)
done

fails=0
for _ in $(seq 1 "$ITERS"); do
    "$EXE" --exact "$T" >/dev/null 2>&1 || fails=$((fails + 1))
done
echo "under load: $fails / $ITERS failed"

# Only the PIDs this script started, by PID, one at a time.
for p in "${pids[@]}"; do
    wait "$p" 2>/dev/null
done

# Control: the same loop with no load must be clean, or the run says nothing
# about load being the variable.
fails=0
for _ in $(seq 1 "$ITERS"); do
    "$EXE" --exact "$T" >/dev/null 2>&1 || fails=$((fails + 1))
done
echo "unloaded:   $fails / $ITERS failed"
