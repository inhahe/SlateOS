#!/usr/bin/env bash
# boot-test.sh — Build the kernel, boot it in QEMU, verify BOOT_OK.
#
# Exit codes:
#   0 — BOOT_OK detected
#   1 — Timeout or PANIC detected
#
# Usage:
#   ./scripts/boot-test.sh              # full build + test
#   ./scripts/boot-test.sh --no-build   # skip build

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Convert to Windows paths if running under MSYS/Git Bash (QEMU needs them).
to_win_path() {
    if command -v cygpath &>/dev/null; then
        cygpath -w "$1"
    else
        echo "$1"
    fi
}

KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/debug/kernel"
ESP_DIR="$PROJECT_ROOT/build/esp"
SERIAL_FILE="$PROJECT_ROOT/build/serial-test.txt"
# QEMU args need Windows paths
ESP_DIR_WIN="$(to_win_path "$ESP_DIR")"
SERIAL_FILE_WIN="$(to_win_path "$SERIAL_FILE")"
TIMEOUT=90
NO_BUILD=0

# Parse args
for arg in "$@"; do
    case "$arg" in
        --no-build) NO_BUILD=1 ;;
        --timeout=*) TIMEOUT="${arg#*=}" ;;
    esac
done

# Find QEMU
QEMU=""
for candidate in \
    "qemu-system-x86_64" \
    "/c/Program Files/qemu/qemu-system-x86_64.exe" \
    "C:/Program Files/qemu/qemu-system-x86_64.exe"; do
    if command -v "$candidate" &>/dev/null || [ -f "$candidate" ]; then
        QEMU="$candidate"
        break
    fi
done

if [ -z "$QEMU" ]; then
    echo "ERROR: qemu-system-x86_64 not found" >&2
    exit 1
fi

# Find OVMF firmware
OVMF=""
for candidate in \
    "/c/Program Files/qemu/share/edk2-x86_64-code.fd" \
    "C:/Program Files/qemu/share/edk2-x86_64-code.fd" \
    "/usr/share/OVMF/OVMF_CODE.fd" \
    "/usr/share/edk2/ovmf/OVMF_CODE.fd"; do
    if [ -f "$candidate" ]; then
        OVMF="$candidate"
        break
    fi
done

if [ -z "$OVMF" ]; then
    echo "ERROR: OVMF/EDK2 firmware not found" >&2
    exit 1
fi

# Step 1: Build
if [ "$NO_BUILD" -eq 0 ]; then
    echo "=== Building kernel ==="
    CARGO="${CARGO:-cargo}"
    # Try full path on Windows if cargo not in PATH
    if ! command -v "$CARGO" &>/dev/null; then
        CARGO="/c/Users/${USER:-${USERNAME:-$(whoami)}}/.cargo/bin/cargo.exe"
    fi
    (cd "$PROJECT_ROOT" && "$CARGO" build)
    echo "Build OK."
fi

if [ ! -f "$KERNEL_BIN" ]; then
    echo "ERROR: Kernel binary not found at $KERNEL_BIN" >&2
    exit 1
fi

# Step 2: Stage boot files
echo "=== Staging boot files ==="
mkdir -p "$ESP_DIR/EFI/BOOT" "$ESP_DIR/boot"
cp "$PROJECT_ROOT/limine/BOOTX64.EFI" "$ESP_DIR/EFI/BOOT/BOOTX64.EFI"
cp "$KERNEL_BIN" "$ESP_DIR/boot/kernel"
cp "$PROJECT_ROOT/limine.conf" "$ESP_DIR/limine.conf"

# Step 3: Create a small swap disk image (16 MiB) for disk-backed swap testing.
SWAP_IMG="$PROJECT_ROOT/build/swap.img"
SWAP_IMG_WIN="$(to_win_path "$SWAP_IMG")"
if [ ! -f "$SWAP_IMG" ]; then
    echo "=== Creating 16 MiB swap disk image ==="
    dd if=/dev/zero of="$SWAP_IMG" bs=1M count=16 status=none 2>/dev/null
fi

# Step 4: Boot QEMU
echo "=== Booting QEMU (timeout: ${TIMEOUT}s) ==="
rm -f "$SERIAL_FILE"

OVMF_WIN="$(to_win_path "$OVMF")"
"$QEMU" \
    -drive "if=pflash,format=raw,readonly=on,file=$OVMF_WIN" \
    -drive "format=raw,file=fat:rw:$ESP_DIR_WIN" \
    -device virtio-blk-pci,drive=swap-disk \
    -drive "id=swap-disk,if=none,format=raw,file=$SWAP_IMG_WIN" \
    -serial "file:$SERIAL_FILE_WIN" \
    -display none \
    -no-reboot \
    -m 256M \
    -machine q35 &
QEMU_PID=$!

# Wait for BOOT_OK or timeout
ELAPSED=0
while kill -0 "$QEMU_PID" 2>/dev/null && [ "$ELAPSED" -lt "$TIMEOUT" ]; do
    sleep 1
    ELAPSED=$((ELAPSED + 1))

    if [ -f "$SERIAL_FILE" ] && grep -q "BOOT_OK" "$SERIAL_FILE" 2>/dev/null; then
        echo "BOOT_OK detected after ${ELAPSED}s!"
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
        echo "=== Boot test PASSED ==="
        exit 0
    fi
done

# Clean up
kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

# Check final output
if [ -f "$SERIAL_FILE" ]; then
    if grep -q "BOOT_OK" "$SERIAL_FILE"; then
        echo "BOOT_OK found."
        echo "=== Boot test PASSED ==="
        exit 0
    elif grep -q "PANIC\|FATAL" "$SERIAL_FILE"; then
        echo "KERNEL PANIC detected!"
        grep "PANIC\|FATAL\|EXCEPTION" "$SERIAL_FILE" || true
        echo "=== Boot test FAILED ==="
        exit 1
    fi
fi

echo "BOOT_OK not found within ${TIMEOUT}s."
echo "=== Boot test FAILED ==="
exit 1
