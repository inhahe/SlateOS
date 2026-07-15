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
set -u
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
OUTDIR="$ROOT/build/hang-catches"
mkdir -p "$OUTDIR"
RUNSTAMP="$(date +%Y%m%d-%H%M%S)"
MAX_ITERS="${MAX_ITERS:-10}"
TIMEOUT="${SOAK_TIMEOUT:-240}"
SERIAL="$ROOT/build/serial-test.txt"
REGS="$ROOT/build/serial-test-regs.txt"

echo "=== wedge-soak run $RUNSTAMP: up to $MAX_ITERS armed boots, timeout=${TIMEOUT}s each ==="

caught=0
for i in $(seq 1 "$MAX_ITERS"); do
    n="$(printf '%02d' "$i")"
    echo ""
    echo "########## soak iter $n/$MAX_ITERS ($(date +%H:%M:%S)) ##########"
    rm -f "$REGS"
    stdout_log="$OUTDIR/soak-$RUNSTAMP-iter$n.stdout.txt"
    bash scripts/boot-test.sh --hard-lockup-watchdog --no-build --timeout="$TIMEOUT" \
        > "$stdout_log" 2>&1
    rc=$?
    # Archive this iteration's serial log + any register dump.
    [ -f "$SERIAL" ] && cp -f "$SERIAL" "$OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
    if [ -f "$REGS" ] && [ -s "$REGS" ]; then
        cp -f "$REGS" "$OUTDIR/soak-$RUNSTAMP-iter$n.regs.txt"
    fi
    verdict="$(grep -E 'Boot test (PASSED|FAILED)|BOOT_OK detected|Wedged RIP' "$stdout_log" | tr '\n' ' | ')"
    echo "iter $n: rc=$rc :: $verdict"
    # A genuine wedge catch = timeout (rc!=0) AND a RIP was captured from HMP.
    if [ "$rc" -ne 0 ] && [ -f "$REGS" ] && [ -s "$REGS" ] && grep -qiE 'RIP=[0-9a-f]+' "$REGS"; then
        echo ""
        echo "=== WEDGE CAUGHT on iter $n ==="
        grep -iE 'Wedged RIP|nearest symbol|in function|resolve' "$stdout_log" || true
        echo "  serial: $OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
        echo "  regs:   $OUTDIR/soak-$RUNSTAMP-iter$n.regs.txt"
        caught=1
        break
    fi
done

echo ""
if [ "$caught" -eq 1 ]; then
    echo "=== SOAK DONE: wedge caught (see hang-catches soak-$RUNSTAMP-*) ==="
else
    echo "=== SOAK DONE: no wedge caught in $MAX_ITERS iters (race did not fire) ==="
fi
echo "WEDGE_SOAK_DONE rc_caught=$caught"
