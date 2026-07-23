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
    # Hunt catch: the Path-Z checkpoint reported a nonzero corruption count (a
    # stale-pointer/UAF write into a parked slot) — the precise B-KNULLJUMP
    # signal. Check the archived serial log for `corruptions=` > 0.
    if [ -f "$SERIAL" ] && grep -aqE '\[hunt\].*corruptions=[1-9]' "$SERIAL"; then
        echo ""
        echo "=== B-KNULLJUMP CORRUPTION CAUGHT on iter $n ==="
        grep -aE '\[quarantine\].*CORRUPTION|\[hunt\].*corruptions=' "$SERIAL" || true
        echo "  serial: $OUTDIR/soak-$RUNSTAMP-iter$n.serial.txt"
        caught=1
        break
    fi
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
