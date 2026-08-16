#!/usr/bin/env bash
# Positive control for the benchmark suite's host-load canary.
#
# # Why this exists
#
# The canary is *believed* to detect host contamination on the strength of two
# uncontrolled observations: one `--bench` run measured a 47% spread in the
# reference access cost while unrelated tooling happened to be running, and the
# next run measured 0% on an idle machine.  That is circumstantial.  Nobody set
# the load; it was reconstructed afterwards.
#
# By the maxim this project keeps relearning -- a check that cannot fire is
# indistinguishable from a check that passes -- a detector that has never been
# shown to fire on a *known* stimulus is not yet a detector.  This script
# supplies the stimulus: it boots the suite and deliberately loads the host CPU
# during the QEMU window, so "the canary detects host load" becomes something
# demonstrated rather than assumed.
#
# Expected result (prediction P20 in known-issues.md): CONTAMINATED, with a
# spread above the 25% tolerance.  A `Canary OK` here would falsify the
# attribution and mean the two dirty runs had some other cause.
#
# # Usage
#
#   ./scripts/canary-load-test.sh [n_spinners]
#
# Default 6 spinners.  Runs the ordinary `--bench` boot, so it takes as long as
# that does.
#
# # Process hygiene
#
# The spinners are started by this script and are killed **by the exact PIDs it
# started**, never by process name.  A blanket `pkill python` would take down
# the operator's unrelated Python work, including the backends serving this
# session.  The PIDs are also cleaned up by an EXIT trap, so an interrupt or a
# failure cannot leave CPU burners running.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
SPINNERS="${1:-6}"
LOG="$(mktemp -t canary-load-XXXXXX.log)"

HISTORY="$PROJECT_ROOT/bench/history.jsonl"
HISTORY_BACKUP="$(mktemp -t bench-history-XXXXXX.jsonl)"

SPIN_PIDS=()

cleanup() {
    # Only the PIDs this script started.  Never by name -- see above.
    for pid in "${SPIN_PIDS[@]:-}"; do
        [ -n "$pid" ] && kill "$pid" 2>/dev/null
    done
    SPIN_PIDS=()

    # Restore the benchmark history.
    #
    # This run's 64 numbers are deliberately sabotaged, and `bench-history.py`
    # compares each run against the *most recent* record -- so leaving it in
    # place would silently make a knowingly-contaminated run the baseline for
    # the next real one, manufacturing a suite-wide phantom "improvement".
    # Observed: the first execution of this script appended a record whose
    # reference cost was double the idle figure.
    #
    # Discarding it loses nothing.  history.jsonl exists to track the *kernel's*
    # performance over time, and this run measures the load generator, not the
    # kernel.  The experiment's actual finding is the canary verdict, which is
    # printed below and written up in known-issues.md.
    if [ -s "$HISTORY_BACKUP" ]; then
        cp "$HISTORY_BACKUP" "$HISTORY"
        rm -f "$HISTORY_BACKUP"
    fi
}
trap cleanup EXIT INT TERM

[ -f "$HISTORY" ] && cp "$HISTORY" "$HISTORY_BACKUP"

echo "=== canary positive control: $SPINNERS spinners during the QEMU window ==="
echo "=== boot log: $LOG ==="

# Start the boot in the background so load can be applied mid-run.
python "$SCRIPT_DIR/run-timeout.py" --poll 60 1800 \
    "$SCRIPT_DIR/boot-test.sh" --bench > "$LOG" 2>&1 &
BOOT_PID=$!

# Wait for QEMU to actually start.  Loading the host during the *build* would
# only slow the build; the measurement window is QEMU's.
echo "=== waiting for QEMU to start ==="
for _ in $(seq 1 1800); do
    grep -qa "Booting QEMU" "$LOG" && break
    kill -0 "$BOOT_PID" 2>/dev/null || break
    sleep 1
done

if ! grep -qa "Booting QEMU" "$LOG"; then
    echo "=== QEMU never started; boot log tail: ==="
    tail -20 "$LOG"
    wait "$BOOT_PID"
    exit 125
fi

echo "=== QEMU up: applying load ==="
for _ in $(seq 1 "$SPINNERS"); do
    # A tight pure-Python loop: no I/O, no sleeping, purely CPU-bound, which is
    # what contends with TCG emulation.
    python -c "
while True:
    pass
" &
    SPIN_PIDS+=($!)
done
echo "=== spinner PIDs: ${SPIN_PIDS[*]} ==="

# Hold the load for the whole suite, then stop it as soon as the boot finishes.
wait "$BOOT_PID"
BOOT_RC=$?
cleanup
echo "=== load removed; boot exited $BOOT_RC ==="

echo
echo "=== canary verdict ==="
grep -aE "CANARY |CANARY-TRACE|Canary OK|CONTAMINATED|CANARY BROKEN" \
    "$PROJECT_ROOT/build/serial-test.txt" || echo "(no canary lines found)"
echo
tail -3 "$LOG"
exit "$BOOT_RC"
