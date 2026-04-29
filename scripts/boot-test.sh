#!/usr/bin/env bash
# Boot the kernel in QEMU and check for a success marker on serial output.
#
# Usage:
#   ./scripts/boot-test.sh [path-to-kernel-binary]
#
# The script:
#   1. Builds an ISO (if no binary path given, builds first)
#   2. Boots QEMU with serial output piped to stdout
#   3. Waits for "BOOT_OK" on serial output
#   4. Exits 0 on success, 1 on timeout
#
# Timeout: 30 seconds.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TIMEOUT_SECS=30
SUCCESS_MARKER="BOOT_OK"
ISO_PATH="$PROJECT_ROOT/os.iso"

# Build ISO if it doesn't exist or is stale.
if [ ! -f "$ISO_PATH" ]; then
    echo "[boot-test] Building ISO first..."
    "$PROJECT_ROOT/scripts/build-iso.sh"
fi

echo "[boot-test] Booting QEMU (timeout: ${TIMEOUT_SECS}s, looking for: $SUCCESS_MARKER)..."

# Run QEMU with serial to stdio, capture output.
# -display none: no graphical window
# -no-reboot: exit on triple fault instead of rebooting
timeout "$TIMEOUT_SECS" \
    qemu-system-x86_64 \
        -cdrom "$ISO_PATH" \
        -serial stdio \
        -display none \
        -m 256M \
        -no-reboot \
        -no-shutdown \
    2>/dev/null | while IFS= read -r line; do
        echo "[serial] $line"
        if echo "$line" | grep -qF "$SUCCESS_MARKER"; then
            echo "[boot-test] SUCCESS: found $SUCCESS_MARKER"
            # Kill the parent timeout+qemu process group.
            kill 0 2>/dev/null || true
            exit 0
        fi
    done

EXIT_CODE=${PIPESTATUS[0]:-1}

if [ "$EXIT_CODE" -eq 124 ]; then
    echo "[boot-test] TIMEOUT: $SUCCESS_MARKER not seen within ${TIMEOUT_SECS}s"
    exit 1
elif [ "$EXIT_CODE" -ne 0 ]; then
    echo "[boot-test] QEMU exited with code $EXIT_CODE"
    exit 1
fi
