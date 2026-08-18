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
# # Placing the load somewhere in particular (prediction P22)
#
# P20 only needed the canary to fire at all, so the load ran for the whole QEMU
# window.  That is useless for P22, which asks a *positional* question: does the
# drift model flag the benchmarks that ran during the disturbance, rather than
# merely flagging that a disturbance happened?  A load covering the entire suite
# cannot discriminate -- every model that fires at all scores a perfect hit,
# including a model that flags all 86 benchmarks unconditionally.  To separate
# those, the load has to occupy an *interior* window: some benchmarks before it,
# some during, some after.  `--load-at` / `--load-until` do that.
#
# The trigger is a benchmark *name*, not a wall-clock delay, for two reasons.
# A delay would have to be tuned against a suite whose length moves with the
# host -- and worse, it would leave the experiment's own ground truth (which
# positions were actually loaded) as an estimate.  Estimating the ground truth
# of an experiment about positions defeats the experiment.  A name is exact:
# the kernel prints each benchmark's result as it finishes, so the load starts
# within one benchmark of a known point, and the SCORE lines at the end of the
# run say precisely which position that was.
#
# # Usage
#
#   ./scripts/canary-load-test.sh [n_spinners]
#   ./scripts/canary-load-test.sh [n_spinners] --load-at=NAME [--load-until=NAME]
#
# Default 6 spinners.  With no `--load-at` the load runs for the whole QEMU
# window, exactly as it did for P20.  Runs the ordinary `--bench` boot, so it
# takes as long as that does.
#
#   # load across the middle of the suite only, and grade the model against it
#   ./scripts/canary-load-test.sh 6 --load-at=crypto_x25519 --load-until=isr_latency
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
LOG="$(mktemp -t canary-load-XXXXXX.log)"

SPINNERS=6
LOAD_AT=""
LOAD_UNTIL=""

for arg in "$@"; do
    case "$arg" in
        # An empty value is rejected rather than ignored. `--load-at=` with the
        # name lost to shell quoting would otherwise fall through to the
        # whole-window behaviour, and a whole-window run is indistinguishable
        # from an interior-window one until the grading at the end says it
        # could not discriminate -- 12 minutes later.
        --load-at=)     echo "--load-at needs a benchmark name" >&2; exit 2 ;;
        --load-until=)  echo "--load-until needs a benchmark name" >&2; exit 2 ;;
        --load-at=*)    LOAD_AT="${arg#*=}" ;;
        --load-until=*) LOAD_UNTIL="${arg#*=}" ;;
        --spinners=*)   SPINNERS="${arg#*=}" ;;
        -h|--help)      sed -n '2,/^$/p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        [0-9]*)         SPINNERS="$arg" ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

if [ -z "$LOAD_AT" ] && [ -n "$LOAD_UNTIL" ]; then
    # --load-until alone would mean "load from the start of the suite until
    # NAME", which is a prefix, not an interior window.  A prefix is a weak
    # discriminator for P22 -- a model that simply flags everything before some
    # point passes it -- and the asymmetry is far more likely to be a typo'd
    # --load-at than a deliberate design.  Refuse rather than silently run a
    # 12-minute boot that answers a weaker question than the caller asked.
    echo "--load-until requires --load-at (a prefix window cannot discriminate; see P22)" >&2
    exit 2
fi

HISTORY="$PROJECT_ROOT/bench/history.jsonl"
HISTORY_BACKUP="$(mktemp -t bench-history-XXXXXX.jsonl)"
SERIAL_FILE="$PROJECT_ROOT/build/serial-test.txt"

SPIN_PIDS=()

stop_load() {
    # Only the PIDs this script started.  Never by name -- see above.
    for pid in "${SPIN_PIDS[@]:-}"; do
        [ -n "$pid" ] && kill "$pid" 2>/dev/null
    done
    SPIN_PIDS=()
}

cleanup() {
    stop_load

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

if [ -n "$LOAD_AT" ]; then
    echo "=== load window: from '$LOAD_AT'${LOAD_UNTIL:+ until '$LOAD_UNTIL'} ==="
    # Predict where the window will land, from the *previous* run's serial log.
    #
    # Purely so a typo'd benchmark name costs a message now instead of a
    # 12-minute boot that quietly never applies any load -- which would look
    # exactly like a clean run and could be mistaken for the model failing to
    # fire.  It is a prediction, not the ground truth: the real positions are
    # read back from this run's own SCORE lines after the boot.
    if [ -f "$SERIAL_FILE" ]; then
        python "$SCRIPT_DIR/grade-positional.py" --serial "$SERIAL_FILE" \
            --load-at "$LOAD_AT" ${LOAD_UNTIL:+--load-until "$LOAD_UNTIL"} \
            --positions-only --label "predicted from the previous run" \
            || echo "    (could not predict from the previous serial log; continuing)"
    fi
fi

# Remove the previous run's serial log before the boot starts.
#
# boot-test.sh removes it too, but not until it reaches QEMU -- and the marker
# follower below opens this path as soon as it sees "Booting QEMU", which is
# printed *before* that removal.  Opening the stale file would match the
# PREVIOUS run's copy of the trigger benchmark instantly and fire the load at
# position 0, i.e. silently turn an interior-window experiment back into the
# whole-suite one it was written to replace.  Deleting it first makes that
# unrepresentable: the only file that can appear at this path is this run's.
rm -f "$SERIAL_FILE"

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

# Follow the growing serial log for the first line naming benchmark $1.
#
# Pure-bash, on one long-lived file descriptor, deliberately.  The obvious
# `grep` in a `sleep 1` loop spawns a process per second for the whole QEMU
# window, and under RESULT P19 host activity during that window moves the
# reference cost by tens of percent -- on the very run whose reference cost is
# the measurement.  A held-open fd plus in-shell pattern matching costs one
# `sleep` per second and nothing else.  (EOF on a regular file is not sticky:
# the offset stays put, so a later read returns whatever was appended since.)
#
# Returns 0 on match, 1 if the boot exited first, 2 if the log never appeared,
# 3 on deadline.
wait_for_bench_marker() {
    local name="$1" deadline=$((SECONDS + ${2:-1800}))
    local fd line leftover=""

    while [ ! -f "$SERIAL_FILE" ]; do
        [ "$SECONDS" -lt "$deadline" ] || return 2
        kill -0 "$BOOT_PID" 2>/dev/null || return 1
        sleep 1
    done
    exec {fd}<"$SERIAL_FILE" || return 2

    while :; do
        while :; do
            if IFS= read -r line <&"$fd"; then
                # A line that was split across two reads: `read` consumed the
                # partial text and reported failure, so it is carried here.
                # Dropping it would silently lose the trigger, and the trigger
                # is exactly the line most likely to be mid-flight -- the log is
                # being appended to by QEMU as we read it.
                line="$leftover$line"
                leftover=""
                case "$line" in
                    "[bench] $name:"*|"[bench]   $name:"*|"[bench] $name "*)
                        exec {fd}<&-
                        return 0
                        ;;
                esac
            else
                leftover="$leftover$line"
                break
            fi
        done
        [ "$SECONDS" -lt "$deadline" ] || { exec {fd}<&-; return 3; }
        kill -0 "$BOOT_PID" 2>/dev/null || { exec {fd}<&-; return 1; }
        sleep 1
    done
}

start_load() {
    for _ in $(seq 1 "$SPINNERS"); do
        # A tight pure-Python loop: no I/O, no sleeping, purely CPU-bound,
        # which is what contends with TCG emulation.
        python -c "
while True:
    pass
" &
        SPIN_PIDS+=($!)
    done
    echo "=== spinner PIDs: ${SPIN_PIDS[*]} ==="
}

LOAD_FIRED=0
if [ -z "$LOAD_AT" ]; then
    echo "=== QEMU up: applying load for the whole window ==="
    start_load
    LOAD_FIRED=1
else
    echo "=== QEMU up: waiting for '$LOAD_AT' before applying load ==="
    wait_for_bench_marker "$LOAD_AT"
    case $? in
        0) echo "=== '$LOAD_AT' reached: applying load ==="
           start_load
           LOAD_FIRED=1 ;;
        1) echo "=== boot exited before '$LOAD_AT' was reached; no load applied ===" ;;
        2) echo "=== serial log never appeared; no load applied ===" ;;
        3) echo "=== timed out waiting for '$LOAD_AT'; no load applied ===" ;;
    esac
fi

if [ "$LOAD_FIRED" = 1 ] && [ -n "$LOAD_UNTIL" ]; then
    echo "=== holding load until '$LOAD_UNTIL' ==="
    wait_for_bench_marker "$LOAD_UNTIL"
    case $? in
        0) echo "=== '$LOAD_UNTIL' reached: removing load ==="  ;;
        *) echo "=== '$LOAD_UNTIL' never reached; load ran to the end of the boot ===" ;;
    esac
    stop_load
fi

# Hold whatever load is left, then stop it as soon as the boot finishes.
wait "$BOOT_PID"
BOOT_RC=$?
cleanup
echo "=== load removed; boot exited $BOOT_RC ==="

echo
echo "=== canary verdict ==="
grep -aE "CANARY |CANARY-TRACE|Canary OK|CONTAMINATED|CANARY BROKEN" \
    "$SERIAL_FILE" || echo "(no canary lines found)"

if [ -n "$LOAD_AT" ] && [ "$LOAD_FIRED" = 1 ]; then
    echo
    echo "=== positional grading (prediction P22) ==="
    python "$SCRIPT_DIR/grade-positional.py" --serial "$SERIAL_FILE" \
        --load-at "$LOAD_AT" ${LOAD_UNTIL:+--load-until "$LOAD_UNTIL"} \
        || echo "(grading failed; the serial log is still at $SERIAL_FILE)"
fi

echo
tail -3 "$LOG"
exit "$BOOT_RC"
