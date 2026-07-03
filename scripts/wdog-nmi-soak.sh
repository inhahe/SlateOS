#!/usr/bin/env bash
# wdog-nmi-soak.sh — DIAGNOSTIC (throwaway): boot repeatedly under the
# inject-nmi hard-lockup watchdog to capture a backtrace of the silent IF=0
# wedge. Stops on the first iteration that prints "NMI WATCHDOG FIRED".
# Usage: scripts/wdog-nmi-soak.sh [MAX_ITERS] [PER_BOOT_TIMEOUT]
set -u
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT" || exit 2
SERIAL_FILE="$PROJECT_ROOT/build/serial-test.txt"
CATCH_DIR="$PROJECT_ROOT/build/hang-catches"
mkdir -p "$CATCH_DIR"
MAX_ITERS="${1:-15}"
PER_BOOT_TIMEOUT="${2:-220}"
for i in $(seq 1 "$MAX_ITERS"); do
    echo "=== nmi-soak iter $i/$MAX_ITERS ($(date +%H:%M:%S)) ==="
    t0=$(date +%s)
    bash scripts/boot-test.sh --no-build --hard-lockup-watchdog \
        --timeout="$PER_BOOT_TIMEOUT" >"$CATCH_DIR/nmi-iter-$i.stdout" 2>&1
    rc=$?
    dur=$(( $(date +%s) - t0 ))
    if grep -q "NMI WATCHDOG FIRED" "$SERIAL_FILE" 2>/dev/null; then
        cp "$SERIAL_FILE" "$CATCH_DIR/NMI-FIRED-iter-$i.txt"
        echo "!!! iter $i: NMI WATCHDOG FIRED (rc=$rc dur=${dur}s) — backtrace captured !!!"
        grep -nE "NMI WATCHDOG FIRED|rip=|kick_stale|backtrace|#[0-9]+ 0x" "$CATCH_DIR/NMI-FIRED-iter-$i.txt" | head -40
        exit 1
    fi
    if grep -q "BOOT_OK" "$SERIAL_FILE" 2>/dev/null; then
        echo "iter $i: clean BOOT_OK in ${dur}s — miss, retrying"
    else
        cp "$SERIAL_FILE" "$CATCH_DIR/NMI-NOBOOTOK-iter-$i.txt"
        echo "??? iter $i: no BOOT_OK and no NMI dump (rc=$rc dur=${dur}s) — saved for inspection"
    fi
done
echo "=== $MAX_ITERS iters, no NMI fire captured this batch ==="
exit 0
