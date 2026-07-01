#!/usr/bin/env bash
# hang-repro-loop.sh — Repeatedly boot the (already-built) kernel to try to
# reproduce the intermittent total-hang (known-issues.md B-PTHREAD-YIELDBUDGET)
# now that the boot-window liveness watchdog is in place.
#
# On each iteration it runs the boot test WITHOUT rebuilding, then inspects the
# serial log for one of two failure signatures:
#   1. "[liveness] SYSTEM HANG"  — the watchdog fired: the task-table dump that
#      follows names the lost thread. This is the jackpot; we stop and preserve
#      the log.
#   2. missing "BOOT_OK"         — the boot did not complete (a hang so total
#      that even the liveness watchdog's own timer path stalled, OR a different
#      failure). We also stop and preserve the log.
# A clean boot (BOOT_OK present, no liveness line) is a miss; we go again.
#
# Usage: scripts/hang-repro-loop.sh [MAX_ITERS]   (default 15)
set -u

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT" || exit 2
SERIAL_FILE="$PROJECT_ROOT/build/serial-test.txt"
CATCH_DIR="$PROJECT_ROOT/build/hang-catches"
mkdir -p "$CATCH_DIR"

MAX_ITERS="${1:-15}"
caught=""

for i in $(seq 1 "$MAX_ITERS"); do
    echo "=== hang-repro iteration $i/$MAX_ITERS ($(date +%H:%M:%S)) ==="
    bash scripts/boot-test.sh --no-build >"$CATCH_DIR/iter-$i.stdout" 2>&1
    rc=$?

    if [ ! -f "$SERIAL_FILE" ]; then
        echo "iter $i: no serial file produced (rc=$rc) — treating as failure"
        cp "$CATCH_DIR/iter-$i.stdout" "$CATCH_DIR/CAUGHT-iter-$i-noserial.txt" 2>/dev/null
        caught="iter-$i-noserial"
        break
    fi

    if grep -q "\[liveness\] SYSTEM HANG" "$SERIAL_FILE"; then
        echo "!!! iter $i: LIVENESS WATCHDOG FIRED — hang reproduced and dumped !!!"
        cp "$SERIAL_FILE" "$CATCH_DIR/CAUGHT-iter-$i-liveness.txt"
        caught="iter-$i-liveness"
        break
    fi

    if ! grep -q "BOOT_OK" "$SERIAL_FILE"; then
        echo "!!! iter $i: BOOT_OK missing — boot hung (watchdog did not dump) !!!"
        cp "$SERIAL_FILE" "$CATCH_DIR/CAUGHT-iter-$i-nobootok.txt"
        caught="iter-$i-nobootok"
        break
    fi

    echo "iter $i: clean boot (BOOT_OK, no hang) — miss, retrying"
done

if [ -n "$caught" ]; then
    echo "=== HANG REPRODUCED on $caught — log preserved in $CATCH_DIR ==="
    exit 1
fi
echo "=== $MAX_ITERS clean boots, no reproduction this batch ==="
exit 0
