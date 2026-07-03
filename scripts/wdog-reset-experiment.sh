#!/usr/bin/env bash
# wdog-reset-experiment.sh — DIAGNOSTIC (throwaway) loop to discriminate, for the
# intermittent BSP-dead demand-paging wedge, whether the i6300esb counter fires
# at all during the wedge.
#
# Method: boot repeatedly with WATCHDOG_ACTION=reset. Combined with the harness's
# always-present -no-reboot, a stage-2 counter expiry makes QEMU EXIT. So on a
# wedge iteration:
#   * QEMU self-exits early (iteration wall-time << per-boot timeout) => the
#     counter FIRED (reset delivered) => the inject-nmi silence is an NMI
#     delivery/handling problem, not a dead counter.
#   * QEMU stays alive until boot-test's own timeout kills it (iteration
#     wall-time ~= per-boot timeout) => the counter did NOT fire during the
#     wedge => a counter/virtual-clock issue.
#
# A clean boot (BOOT_OK present) is a miss; retry. Break on first wedge.
#
# Usage: scripts/wdog-reset-experiment.sh [MAX_ITERS] [PER_BOOT_TIMEOUT]
set -u
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT" || exit 2
SERIAL_FILE="$PROJECT_ROOT/build/serial-test.txt"
CATCH_DIR="$PROJECT_ROOT/build/hang-catches"
mkdir -p "$CATCH_DIR"

MAX_ITERS="${1:-20}"
PER_BOOT_TIMEOUT="${2:-400}"

for i in $(seq 1 "$MAX_ITERS"); do
    echo "=== reset-experiment iter $i/$MAX_ITERS ($(date +%H:%M:%S)) ==="
    t0=$(date +%s)
    WATCHDOG_ACTION=reset bash scripts/boot-test.sh --no-build --hard-lockup-watchdog \
        --timeout="$PER_BOOT_TIMEOUT" >"$CATCH_DIR/reset-iter-$i.stdout" 2>&1
    rc=$?
    t1=$(date +%s)
    dur=$((t1 - t0))

    if [ -f "$SERIAL_FILE" ] && grep -q "BOOT_OK" "$SERIAL_FILE"; then
        echo "iter $i: clean boot (BOOT_OK) in ${dur}s — miss, retrying"
        continue
    fi

    # Non-BOOT_OK: a wedge (or other boot failure). Preserve and classify.
    cp "$SERIAL_FILE" "$CATCH_DIR/RESET-CAUGHT-iter-$i.txt" 2>/dev/null
    echo "!!! iter $i: NO BOOT_OK — wedge/boot-failure caught (rc=$rc, dur=${dur}s) !!!"
    # Heuristic: if QEMU self-exited well before the per-boot timeout, the reset
    # action fired (counter alive). If it ran ~the full timeout, boot-test killed
    # a still-alive QEMU (counter did NOT fire).
    margin=$((PER_BOOT_TIMEOUT - 20))
    if [ "$dur" -lt "$margin" ]; then
        echo ">>> VERDICT: QEMU SELF-EXITED at ${dur}s (< ${margin}s) => i6300esb COUNTER FIRED (reset delivered)."
        echo ">>> Implication: under inject-nmi the counter also fires; the silent wedge is an NMI DELIVERY/HANDLING problem."
    else
        echo ">>> VERDICT: QEMU ran the full ~${dur}s timeout => i6300esb COUNTER DID NOT FIRE during the wedge."
        echo ">>> Implication: the counter/virtual-clock is not advancing (or the counter got disabled) in this wedge state."
    fi
    exit 1
done

echo "=== $MAX_ITERS clean boots, no wedge reproduced this batch ==="
exit 0
