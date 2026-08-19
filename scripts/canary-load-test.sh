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
# # The load itself lives in canary-load.py, and why it had to move there
#
# This script used to follow the serial log in bash and start the spinners with
# `python -c 'while True: pass'` at the trigger.  That is what produced P22's
# first run, and the run was void: the benchmarks in the labelled window had a
# median slowdown of x1.00 while the ones *after* it ran x1.44 slower.  The
# load arrived, but late and long.
#
# The reason is arithmetic.  The whole 86-benchmark suite is a ~6 s tail of a
# ~131 s boot.  An interior window is therefore a couple of *seconds*, and the
# old follower spent a `sleep 1` per poll and then six MSYS interpreter spawns
# before a single spinner reached its loop -- most of the window, gone, and the
# load still running when the window had passed.  No amount of care in bash
# fixes that; the costs are structural.
#
# `canary-load.py` pays all of them up front instead.  Its spinners are spawned
# before QEMU is booted and park on a semaphore, so firing the load is a kernel
# wakeup rather than six process creations; it tails the log in-process at
# 0.1 s; and it writes a JSON record of what it actually did, which the grader
# prints as a diagnostic (never as evidence -- the record is the claim under
# suspicion, not the measurement).
#
# # Process hygiene
#
# The spinners are started by `canary-load.py` and are stopped by asking *it*
# to wind down -- a stop-file, not a signal, because an MSYS `kill` of a native
# Windows process is `TerminateProcess`: no handler runs, no cleanup happens,
# and the record never gets written.  If that fails they are killed **by the
# exact PIDs the controller published**, never by process name.  A blanket
# `pkill python` would take down the operator's unrelated Python work,
# including the backends serving this session.  The spinners additionally watch
# their own parent and exit on their own if it dies, so no path through this
# script -- interrupt, timeout, or the harness killing the shell -- can leave
# CPU burners running.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

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

# Temporary files are created only after the arguments are known to be good.
# Creating them first leaked one per rejected invocation, which is trivial in
# itself and exactly the kind of thing that makes a /tmp full of stale logs
# indistinguishable from a run in progress.
LOG="$(mktemp -t canary-load-XXXXXX.log)"
LOAD_DIR="$(mktemp -d -t canary-load-ctl-XXXXXX)"
LOAD_READY="$LOAD_DIR/ready"
LOAD_STOP="$LOAD_DIR/stop"

HISTORY="$PROJECT_ROOT/bench/history.jsonl"
HISTORY_BACKUP="$(mktemp -t bench-history-XXXXXX.jsonl)"
SERIAL_FILE="$PROJECT_ROOT/build/serial-test.txt"
# A stable path rather than the temp dir: this record is the only account of
# what the stimulus did, it is what explains an ungradeable run, and it must
# still be there tomorrow when someone asks why the window did not land.
LOAD_RECORD="$PROJECT_ROOT/build/canary-load-record.json"
# The live benchmark names the last run actually saw.  Rewritten by every run,
# so it maintains itself; consulted below to reject a window bound that could
# never fire.  See canary-load.py's header: the live result line and the SCORE
# line disagree for 27 of the suite's 86 benchmarks, which is how P22's second
# attempt was lost.
LOAD_NAMES="$PROJECT_ROOT/build/canary-load-names.txt"

# Checked here, before the build and the boot, rather than inside the
# controller -- which does not start until QEMU is already up, by which point
# the mistake has cost the very thing this check exists to save.
if [ -n "$LOAD_AT" ] && [ -f "$LOAD_NAMES" ]; then
    if ! python "$SCRIPT_DIR/canary-load.py" \
            --serial "$SERIAL_FILE" --check-names-only \
            --known-names "$LOAD_NAMES" \
            ${LOAD_AT:+--at "$LOAD_AT"} ${LOAD_UNTIL:+--until "$LOAD_UNTIL"}; then
        exit 2
    fi
fi

SPIN_PIDS=()
LOAD_PID=""

stop_load() {
    # Ask the controller to wind down rather than killing it.
    #
    # It owns the spinners and can stop them without a signal, and it writes
    # its record on the way out.  Killing it would be `TerminateProcess` under
    # MSYS -- no handler, no record, and the spinners left to fall back on
    # their own dead-parent detection, which works but takes a second and
    # tells us nothing about what happened.
    # The braces matter.  `: > "$F" 2>/dev/null` does NOT silence a failing
    # redirect: bash applies redirections left to right, so `>` is attempted
    # and reports to the stderr that is still the terminal before the `2>` is
    # reached.  This function runs twice -- once explicitly at the end of a
    # successful run, once again from the EXIT trap -- and on the second pass
    # the directory is already gone, so the run's last line was a bare
    # "No such file or directory" that read like a failure in an otherwise
    # clean run.
    if [ -n "${LOAD_STOP:-}" ]; then
        { : > "$LOAD_STOP"; } 2>/dev/null || true
    fi
    if [ -n "${LOAD_PID:-}" ]; then
        for _ in $(seq 1 30); do
            kill -0 "$LOAD_PID" 2>/dev/null || break
            sleep 1
        done
        kill "$LOAD_PID" 2>/dev/null
        LOAD_PID=""
    fi
    # Belt and braces, by the exact PIDs the controller published.  Never by
    # name -- see the header.
    for pid in "${SPIN_PIDS[@]:-}"; do
        [ -n "$pid" ] && kill "$pid" 2>/dev/null
    done
    SPIN_PIDS=()
}

cleanup() {
    stop_load
    # Clearing the variables after removing the directory makes a second
    # cleanup (the EXIT trap, after an explicit call) a no-op rather than a
    # sequence of failed operations on paths that no longer exist.
    [ -n "${LOAD_DIR:-}" ] && rm -rf "$LOAD_DIR" 2>/dev/null
    LOAD_DIR=""
    LOAD_STOP=""
    LOAD_READY=""

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
# boot-test.sh removes it too, but not until it reaches QEMU -- and the load
# controller opens this path as soon as it appears.  Opening the stale file
# would match the PREVIOUS run's copy of the trigger benchmark instantly and
# fire the load at position 0, i.e. silently turn an interior-window experiment
# back into the whole-suite one it was written to replace.  Deleting it first
# makes that unrepresentable: the only file that can appear at this path is
# this run's.
#
# The controller reinforces this from the other side by opening the file
# lazily rather than holding a handle open across boot-test.sh's own `rm`,
# which on Windows fails with "Device or resource busy" and takes the whole
# boot with it.
rm -f "$SERIAL_FILE"

# Start the boot in the background so load can be applied mid-run.
python "$SCRIPT_DIR/run-timeout.py" --poll 60 1800 \
    "$SCRIPT_DIR/boot-test.sh" --bench > "$LOG" 2>&1 &
BOOT_PID=$!

# Wait for QEMU to actually start before the controller exists at all.
#
# Two reasons, and the second is easy to miss.  Loading the host during the
# *build* would only slow the build; the measurement window is QEMU's.  And
# with no `--load-at` the controller fires the moment it is ready, so starting
# it earlier would apply the P20 whole-window load across a five-minute cargo
# build and then hold it for the boot -- a different experiment.
#
# boot-test.sh can also sit on the cross-lane boot lock for a long time before
# printing this, which is the other reason the controller is not started on a
# timer.
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

# Start the load controller.  It spawns its spinners now -- roughly two minutes
# before the benchmark suite, which is a ~6 s tail of the boot -- and parks
# them on a semaphore, so applying the load later costs a wakeup rather than
# six interpreter startups inside a two-second window.
#
# `--hold` keeps it tailing after the load is released so it can report the
# window as a fraction of the whole suite.  Without that number an ungradeable
# run cannot be explained: "the load was applied at the right benchmark" and
# "the load covered 3% of the suite's wall time" are both true of P22's first
# run, and only the second one accounts for it.
echo "=== starting load controller ==="
python "$SCRIPT_DIR/canary-load.py" \
    --serial "$SERIAL_FILE" \
    ${LOAD_AT:+--at "$LOAD_AT"} ${LOAD_UNTIL:+--until "$LOAD_UNTIL"} \
    --spinners "$SPINNERS" --timeout 1800 \
    --ready-file "$LOAD_READY" --stop-file "$LOAD_STOP" \
    --record "$LOAD_RECORD" --known-names "$LOAD_NAMES" --hold &
LOAD_PID=$!

# Wait for every spinner interpreter to be up before going any further, so the
# cost of starting them is provably outside the load window rather than
# probably outside it.
for _ in $(seq 1 120); do
    [ -f "$LOAD_READY" ] && break
    kill -0 "$LOAD_PID" 2>/dev/null || break
    sleep 1
done

if [ -f "$LOAD_READY" ]; then
    read -r -a SPIN_PIDS < "$LOAD_READY"
    echo "=== load controller ready: spinner PIDs ${SPIN_PIDS[*]} ==="
else
    # Not fatal: the boot is already running and its canary verdict is still
    # worth having.  But it must be said loudly, because a run with no load is
    # indistinguishable from a clean one in every artefact it leaves behind.
    echo "=== WARNING: load controller never became ready; this run has NO LOAD ==="
fi

# Let the controller do its work; it releases the load itself at --load-until.
wait "$BOOT_PID"
BOOT_RC=$?

# The boot is over, so the controller has nothing left to watch.  Stopping it
# by file rather than by signal is what lets it write its record; see stop_load.
: > "$LOAD_STOP"
wait "$LOAD_PID"
LOAD_RC=$?
LOAD_PID=""

case "$LOAD_RC" in
    0) echo "=== load fired and was released; boot exited $BOOT_RC ===" ;;
    # Exit 1 now covers both missing edges: a trigger that never fired and a
    # release that never fired.  The controller has already said which on
    # stderr, with the closest live names.  Either way the labelled window is
    # not the window that was applied, so the run is void for P22(a) -- and
    # saying so here, rather than leaving it to be inferred from a JSON field,
    # is the whole lesson of the second attempt.
    1) echo "=== WARNING: the load's window is not the one that was asked for."
       echo "=== See the PROBLEM line above. Nothing about the model can be"
       echo "=== concluded from this run's labelled window. ===" ;;
    *) echo "=== WARNING: load controller exited $LOAD_RC ===" ;;
esac

cleanup

echo
echo "=== canary verdict ==="
grep -aE "CANARY |CANARY-TRACE|Canary OK|CONTAMINATED|CANARY BROKEN" \
    "$SERIAL_FILE" || echo "(no canary lines found)"

if [ -n "$LOAD_AT" ]; then
    echo
    echo "=== positional grading (prediction P22) ==="
    # Graded even when the load did not fire, on purpose.  The grader measures
    # the stimulus from the benchmark timings before it says anything about the
    # model, so a run whose load went astray comes back UNGRADED *with the
    # measurement that says so* -- which is the finding.  Suppressing the grade
    # would leave only the controller's own account of itself, and that account
    # is precisely what P22's first run showed cannot be taken at face value.
    #
    # The history has already been restored above, so the per-benchmark
    # baseline this compares against is the last genuinely clean run rather
    # than this deliberately-poisoned one.
    python "$SCRIPT_DIR/grade-positional.py" --serial "$SERIAL_FILE" \
        --load-at "$LOAD_AT" ${LOAD_UNTIL:+--load-until "$LOAD_UNTIL"} \
        --load-record "$LOAD_RECORD"
    GRADE_RC=$?
    # 0 = SUPPORTED, 1 = any other verdict (a result, not an error), 2 = the
    # grader could not run.  Only the last one is worth a complaint.
    if [ "$GRADE_RC" -ge 2 ]; then
        echo "(grading could not run; the serial log is still at $SERIAL_FILE"
        echo " and the load record at $LOAD_RECORD)"
    fi
fi

echo
tail -3 "$LOG"
exit "$BOOT_RC"
