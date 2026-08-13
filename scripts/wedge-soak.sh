#!/usr/bin/env bash
# wedge-soak.sh — armed hang-repro soak for the still-open boot wedge race.
#
# Runs boot-test.sh repeatedly WITH the i6300esb hard-lockup NMI watchdog and
# the diagnostic HMP monitor enabled (--hard-lockup-watchdog), so that when a
# boot wedges the harness captures the frozen guest RIP directly from QEMU and
# resolves it to a kernel symbol. Stops on the FIRST caught wedge (a timeout
# with a non-empty -regs.txt RIP dump) or after MAX_ITERS iterations.
#
# Each iteration's serial log and (if any) register dump are archived to
# build/hang-catches/soak-<runstamp>-iterNN.{serial,regs}.txt so nothing is
# clobbered by the next run.
#
# Kernel is assumed already built and current (soak uses --no-build).
#
# Soaking the compiler-instrumented (KASAN) kernel
# ------------------------------------------------
# The strongest configuration for the B-KNULLJUMP hunt is this soak run against
# a kernel built by scripts/kasan-build.sh: the allocator-boundary checks that
# `mm.corruption_hunt=1` arms only see a stomp when the *checkpoint* runs, and
# only on a parked slab slot, whereas compiler instrumentation checks every load
# and store as it happens and so names the faulting instruction directly.  That
# combination is the whole point of design-decisions.md §107.
#
#   ./scripts/kasan-build.sh
#   MAX_ITERS=30 SOAK_TIMEOUT=5400 STALL_SECS=300 ./scripts/wedge-soak.sh
#
# The two overrides are not optional.  Outlined instrumentation costs a call per
# memory access (§119), so a full self-test boot runs several times longer than
# the ~460-470s the defaults are sized for; leaving TIMEOUT at 720 would cut
# every single boot short and the soak would measure nothing.  STALL_SECS must
# grow for the same reason — a quiet self-test window stretches with everything
# else — but not proportionally, since a true wedge is silent forever.
set -u
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
OUTDIR="$ROOT/build/hang-catches"
mkdir -p "$OUTDIR"
RUNSTAMP="$(date +%Y%m%d-%H%M%S)"
MAX_ITERS="${MAX_ITERS:-10}"
# Per-boot timeout.  Must exceed a healthy boot's wall-clock so a slow-but-fine
# boot is never cut short (which would waste the hunt sample); genuine hangs are
# caught far sooner by the serial-stall detector (STALL_SECS) regardless.  Under
# TCG on a loaded host a full self-test boot has been observed at ~460-470s, so
# default to 720s of headroom.  Override with SOAK_TIMEOUT=.
TIMEOUT="${SOAK_TIMEOUT:-720}"
# Serial-stall wedge threshold passed to boot-test.sh (--stall-secs).  A wedged
# kernel goes silent; a slow boot keeps printing.  Set generously (150s) so a
# legitimately long *quiet* self-test window (e.g. a Path-Z tcc/make run under
# TCG) is never mistaken for a hang — a true silent wedge produces INDEFINITE
# silence, so a larger threshold costs only a slightly later catch.
STALL_SECS="${STALL_SECS:-150}"
SERIAL="$ROOT/build/serial-test.txt"
REGS="$ROOT/build/serial-test-regs.txt"

# B-KNULLJUMP corruption hunt: arm the KASAN shadow + slab free-quarantine
# (kernel `mm.corruption_hunt` flag) by default for the soak, so an
# intermittent stale-pointer/UAF write is caught at the Path-Z checkpoint with a
# precise address/class (in addition to the wedge-RIP capture). Set HUNT=0 to
# soak the plain wedge race instead; SLATE_CMDLINE, if already set, is honored.
HUNT="${HUNT:-1}"
if [ "$HUNT" != "0" ]; then
    export SLATE_CMDLINE="${SLATE_CMDLINE:-mm.corruption_hunt=1}"
fi

echo "=== wedge-soak run $RUNSTAMP: up to $MAX_ITERS armed boots, timeout=${TIMEOUT}s each ==="
[ -n "${SLATE_CMDLINE:-}" ] && echo "=== hunt armed via cmdline: $SLATE_CMDLINE ==="

caught=0
for i in $(seq 1 "$MAX_ITERS"); do
    n="$(printf '%02d' "$i")"
    echo ""
    echo "########## soak iter $n/$MAX_ITERS ($(date +%H:%M:%S)) ##########"
    rm -f "$REGS"
    stdout_log="$OUTDIR/soak-$RUNSTAMP-iter$n.stdout.txt"
    bash scripts/boot-test.sh --hard-lockup-watchdog --no-build \
        --timeout="$TIMEOUT" --stall-secs="$STALL_SECS" \
        > "$stdout_log" 2>&1
    rc=$?
    # Archive this iteration's serial log + any register dump.
    [ -f "$SERIAL" ] && cp -f "$SERIAL" "$OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
    if [ -f "$REGS" ] && [ -s "$REGS" ]; then
        cp -f "$REGS" "$OUTDIR/soak-$RUNSTAMP-iter$n.regs.txt"
    fi
    verdict="$(grep -E 'Boot test (PASSED|FAILED)|BOOT_OK detected|WEDGE: serial|Wedged RIP' "$stdout_log" | tr '\n' ' | ')"
    echo "iter $n: rc=$rc :: $verdict"

    # --- Catch classification -------------------------------------------------
    # Only a GENUINE anomaly stops the campaign.  The overriding principle: a boot
    # that reaches BOOT_OK and PASSES (rc=0) is healthy BY DEFINITION — boot-test
    # exits 0 only on BOOT_OK with no self-test failures.  A healthy boot's serial
    # log routinely contains EXPECTED fault/diagnostic lines that must NOT be
    # mistaken for a crash: ring-3 `#GP`/`#UD` from the SEH / SMAP self-tests, the
    # `instructions would #UD` SMAP-absence note, and even a TRANSIENT `[liveness]
    # SYSTEM HANG` dump when a self-test stalls >15s under host load and then
    # recovers.  So fault/hang/regression signatures are only treated as a catch
    # on a boot that did NOT pass (rc != 0); on rc=0 only the hunt's own
    # corruption checkpoint counts.  (A genuinely fatal fault always prevents
    # BOOT_OK, so it surfaces as rc != 0 — nothing real is lost by this gate.)

    # (0) Compiler-KASAN report: an instrumented build (scripts/kasan-build.sh)
    #     caught a bad access at the instruction that made it.  This is the
    #     highest-value catch available — unlike the checkpoint below it names
    #     the faulting access rather than the damage found later — so it is
    #     tested first.  Checked on EVERY boot regardless of rc: the profile
    #     builds with `-asan-recover=1`, so a reported access is still performed
    #     and the boot can go on to reach BOOT_OK and exit 0 (design-decisions
    #     .md §119).  A plain build never prints this line, so the check is
    #     inert for an ordinary soak.
    if [ -f "$SERIAL" ] && grep -aq '\[kasan\] CRITICAL:' "$SERIAL"; then
        echo ""
        echo "=== KASAN REPORT CAUGHT on iter $n (compiler-instrumented build) ==="
        # The backtrace that follows each report is the payload; keep enough
        # lines to carry the first few frames of the first couple of reports.
        grep -aA 12 '\[kasan\] CRITICAL:' "$SERIAL" | head -40 || true
        echo "  serial: $OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
        caught=1
        break
    fi
    # (1) B-KNULLJUMP corruption checkpoint: the Path-Z hunt reported a nonzero
    #     corruption count (a stale-pointer/UAF write into a parked slot) — the
    #     precise, primary signal.  Checked on EVERY boot (the checkpoint runs
    #     before BOOT_OK, so a corruption can be flagged even on a boot that then
    #     goes on to pass).
    if [ -f "$SERIAL" ] && grep -aqE '\[hunt\].*corruptions=[1-9]' "$SERIAL"; then
        echo ""
        echo "=== B-KNULLJUMP CORRUPTION CAUGHT on iter $n ==="
        grep -aE '\[quarantine\].*CORRUPTION|\[hunt\].*corruptions=' "$SERIAL" || true
        echo "  serial: $OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
        caught=1
        break
    fi
    # (2) Silent wedge: boot-test's serial-stall detector fired (exit 2) — the
    #     kernel went quiet with a RIP captured from the emulator.  This is the
    #     reliable "actually hung" signal (vs. a slow host).
    if [ "$rc" -eq 2 ]; then
        echo ""
        echo "=== WEDGE CAUGHT on iter $n (serial stalled; kernel not progressing) ==="
        grep -iE 'WEDGE: serial|Wedged RIP|nearest symbol|sym @' "$stdout_log" || true
        echo "  serial: $OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
        [ -f "$REGS" ] && echo "  regs:   $OUTDIR/soak-$RUNSTAMP-iter$n.regs.txt"
        caught=1
        break
    fi
    # A boot that PASSED (rc=0) is healthy; expected self-test fault/diagnostic
    # lines are noise.  Nothing further to check — move to the next iter.
    if [ "$rc" -eq 0 ]; then
        continue
    fi

    # --- rc != 0 (and != 2): the boot did NOT complete cleanly. --------------
    # (3) A self-test regression (a test explicitly reported failure).
    if [ -f "$SERIAL" ] && grep -aqiE 'self-test failed' "$SERIAL"; then
        echo ""
        echo "=== SELF-TEST REGRESSION on iter $n ==="
        grep -aiE 'self-test failed' "$SERIAL" | head -12 || true
        echo "  serial: $OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
        caught=1
        break
    fi
    # (4) A FATAL fault/hang that prevented boot completion (incl. B-KNULLJUMP's
    #     null-jump #DF storm).  Restricted to unambiguously-fatal signatures —
    #     a kernel PANIC/FATAL, a double-fault storm, a null-RIP jump, or a
    #     SYSTEM HANG that did NOT recover (the boot never reached BOOT_OK).
    #     Deliberately does NOT grep bare `#GP`/`#UD`/`EXCEPTION`, which also
    #     appear in expected ring-3 self-test output.
    if [ -f "$SERIAL" ] && grep -aqE 'KERNEL PANIC|PANIC:|FATAL|DOUBLE FAULT|SYSTEM HANG|RIP=0x0000000000000000|RIP=0000000000000000' "$SERIAL"; then
        echo ""
        echo "=== FATAL FAULT / HANG CAUGHT on iter $n (boot did not reach BOOT_OK) ==="
        grep -aE 'KERNEL PANIC|PANIC|FATAL|DOUBLE FAULT|SYSTEM HANG|Killing task.*ring 0|RIP=0x0000000000000000' "$SERIAL" | head -12 || true
        echo "  serial: $OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
        [ -f "$REGS" ] && echo "  regs:   $OUTDIR/soak-$RUNSTAMP-iter$n.regs.txt"
        caught=1
        break
    fi
    # Otherwise: a plain slow-boot timeout (rc=1, boot still progressing, no
    # fatal signature) — a wasted sample, not a wedge.  Log and press on.
    echo "iter $n: slow-boot timeout (no fault/stall/corruption signature) — NOT a wedge; continuing."
done

echo ""
if [ "$caught" -eq 1 ]; then
    echo "=== SOAK DONE: wedge caught (see hang-catches soak-$RUNSTAMP-*) ==="
else
    echo "=== SOAK DONE: no wedge caught in $MAX_ITERS iters (race did not fire) ==="
fi
echo "WEDGE_SOAK_DONE rc_caught=$caught"
