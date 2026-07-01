#!/usr/bin/env bash
# boot-test.sh — Build the kernel, boot it in QEMU, verify BOOT_OK.
#
# Exit codes:
#   0 — success marker detected AND no self-test failures
#   1 — Timeout, PANIC, or a non-fatal self-test failure detected
#
# Usage:
#   ./scripts/boot-test.sh              # full build + test (waits for BOOT_OK)
#   ./scripts/boot-test.sh --no-build   # skip build
#   ./scripts/boot-test.sh --bench      # wait for BENCH_OK and print benchmark
#                                       # numbers (the micro-benchmarks run in a
#                                       # deferred background task AFTER BOOT_OK,
#                                       # so the default fast path never sees
#                                       # them — use this to catch perf regressions)

set -euo pipefail

# Scan the serial log for self-test failures that do NOT halt the boot.
#
# Many fs/subsystem self-tests are NON-FATAL: on failure main.rs logs a
# "WARNING: <X> self-test failed" (or "[WARN] ..."/"[hpet] WARNING:
# Self-test failed") and boots on, so BOOT_OK still prints and a naive
# "grep BOOT_OK" reports PASSED even though a test regressed (this exact
# gap hid a stale procfs readdir-count assertion — see todo.txt).
#
# We match the wrapper marker "self-test failed" (case-insensitive),
# which every main.rs self-test failure path emits.  We deliberately do
# NOT grep raw "FAIL:"/"WARNING:": those have legitimate occurrences in a
# passing log — e.g. "[drm-atomic] check FAIL: CRTC 9999 not found"
# (intentional negative tests) and "[lockdep] WARNING: potential deadlock"
# (a deliberately-triggered detector test) — so they would false-positive.
#
# Returns 0 if clean, 1 if any self-test failure marker is present.
check_selftest_failures() {
    local file="$1"
    [ -f "$file" ] || return 0
    if grep -iq "self-test failed" "$file"; then
        echo "SELF-TEST FAILURE detected in serial log:"
        grep -in "self-test failed" "$file" || true
        return 1
    fi
    return 0
}

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
# Default boot timeout.  The boot path runs the full self-test suite before
# printing BOOT_OK, including the Path-Z ring-3 toolchain tests (each spawns a
# real glibc/tcc/make/dash process under ld.so), which now dominate boot time:
# a clean boot reaches BOOT_OK around ~305s and the suite keeps growing as new
# Path-Z rungs land.  Keep a comfortable margin over the observed boot time so
# the default invocation never spuriously "times out" on a healthy kernel;
# override with --timeout= for slower hosts or the --bench wait marker.
TIMEOUT=480
NO_BUILD=0
BENCH=0
# Which serial marker the wait loop treats as "boot finished".  Default is
# BOOT_OK (the fast path); --bench switches it to BENCH_OK so we wait for the
# deferred micro-benchmark task to finish and can scrape its numbers.
WAIT_MARKER="BOOT_OK"

# Parse args
for arg in "$@"; do
    case "$arg" in
        --no-build) NO_BUILD=1 ;;
        --bench) BENCH=1; WAIT_MARKER="BENCH_OK" ;;
        --timeout=*) TIMEOUT="${arg#*=}" ;;
    esac
done

# Print the micro-benchmark result lines from the serial log.  The kernel emits
# them as "[bench] <name>: <number>" plus PASS / "ABOVE TARGET" verdicts from a
# background task that runs AFTER BOOT_OK.  We surface an "ABOVE TARGET" verdict
# as a soft PERF NOTE rather than a hard failure: under QEMU's TCG interpreter
# the absolute cycle counts are noisy and routinely exceed the bare-metal
# targets, so a slow run here is not by itself a regression signal — it's a
# prompt to compare against the previous run's numbers.
print_bench_results() {
    local file="$1"
    [ -f "$file" ] || return 0
    echo "=== Benchmark results ==="
    grep -E '^\[bench\]' "$file" || echo "(no [bench] lines found)"
    if grep -q "ABOVE TARGET" "$file"; then
        echo "PERF NOTE: one or more benchmarks reported ABOVE TARGET."
        echo "  (QEMU/TCG cycle counts are noisy; compare against prior runs"
        echo "   rather than treating this as a hard regression.)"
    fi
}

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

# Strip debug symbols — the unstripped debug binary can exceed 150 MiB,
# which blows past what Limine can load in 256-512 MiB of RAM.  Stripping
# brings it down to ~30 MiB.  We try llvm-strip (ships with rustup) first,
# falling back to a plain copy if no strip tool is found.
LLVM_STRIP=""
for candidate in \
    "$HOME/.rustup/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/llvm-strip.exe" \
    "$(rustup which llvm-strip 2>/dev/null)" \
    "llvm-strip" \
    "strip"; do
    if [ -n "$candidate" ] && command -v "$candidate" &>/dev/null || [ -f "$candidate" ]; then
        LLVM_STRIP="$candidate"
        break
    fi
done

# Stage the kernel.  A strip failure (e.g. the staged image is locked by a
# stray QEMU still holding the disk open → "Permission denied") MUST NOT be
# ignored: if it is, the boot test silently re-runs the previously-staged
# (stale) kernel and reports misleading results.  So we check the exit code,
# fall back to a plain copy, and abort the whole run if staging can't update
# the image.
STAGED_KERNEL="$ESP_DIR/boot/kernel"
stage_ok=0
if [ -n "$LLVM_STRIP" ]; then
    echo "Stripping kernel binary with $LLVM_STRIP..."
    if "$LLVM_STRIP" "$KERNEL_BIN" -o "$STAGED_KERNEL"; then
        stage_ok=1
    else
        echo "WARNING: strip failed; falling back to an unstripped copy." >&2
    fi
fi
if [ "$stage_ok" -eq 0 ]; then
    if cp "$KERNEL_BIN" "$STAGED_KERNEL"; then
        stage_ok=1
    fi
fi
if [ "$stage_ok" -eq 0 ]; then
    echo "ERROR: could not stage kernel to $STAGED_KERNEL." >&2
    echo "       The image is likely locked by a stray qemu-system-x86_64" >&2
    echo "       process holding the disk open.  Kill it and re-run." >&2
    exit 1
fi
# Guard against a staged image that predates this build: it must be newer
# than the freshly-built kernel binary we just compiled.
if [ "$STAGED_KERNEL" -ot "$KERNEL_BIN" ]; then
    echo "ERROR: staged kernel is older than the build output — staging did" >&2
    echo "       not take effect (stale image).  Aborting to avoid a" >&2
    echo "       misleading boot test." >&2
    exit 1
fi

cp "$PROJECT_ROOT/limine.conf" "$ESP_DIR/limine.conf"

# Step 3: Create a small swap disk image (16 MiB) for disk-backed swap testing.
SWAP_IMG="$PROJECT_ROOT/build/swap.img"
SWAP_IMG_WIN="$(to_win_path "$SWAP_IMG")"
if [ ! -f "$SWAP_IMG" ]; then
    echo "=== Creating 16 MiB swap disk image ==="
    dd if=/dev/zero of="$SWAP_IMG" bs=1M count=16 status=none 2>/dev/null
fi

# Step 3b: Attach the Path-Z glibc rootfs (rootfs.ext4) as a second virtio-blk
# disk when present.  It is enumerated AFTER swap-disk, so it becomes vdb: the
# kernel's swap loop skips it (ext4 superblock detected) and the /mnt ext4 probe
# mounts it, enabling the real-glibc dynamic-execution self-test.  Built on the
# dev box via `wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`; git-ignored,
# so the boot test simply omits it (and the self-test no-ops) when it is absent.
ROOTFS_IMG="$PROJECT_ROOT/rootfs.ext4"
ROOTFS_ARGS=()
if [ -f "$ROOTFS_IMG" ]; then
    ROOTFS_IMG_WIN="$(to_win_path "$ROOTFS_IMG")"
    ROOTFS_ARGS=(
        -device virtio-blk-pci,drive=rootfs-disk
        -drive "id=rootfs-disk,if=none,format=raw,file=$ROOTFS_IMG_WIN"
    )
    echo "=== Attaching Path-Z glibc rootfs: $ROOTFS_IMG (vdb) ==="
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
    "${ROOTFS_ARGS[@]}" \
    -device virtio-gpu-pci \
    -serial "file:$SERIAL_FILE_WIN" \
    -display none \
    -no-reboot \
    -m 512M \
    -machine q35 &
QEMU_PID=$!

# Wait for BOOT_OK or timeout
ELAPSED=0
while kill -0 "$QEMU_PID" 2>/dev/null && [ "$ELAPSED" -lt "$TIMEOUT" ]; do
    sleep 1
    ELAPSED=$((ELAPSED + 1))

    if [ -f "$SERIAL_FILE" ] && grep -q "$WAIT_MARKER" "$SERIAL_FILE" 2>/dev/null; then
        echo "$WAIT_MARKER detected after ${ELAPSED}s!"
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
        if ! check_selftest_failures "$SERIAL_FILE"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but a self-test failed) ==="
            exit 1
        fi
        [ "$BENCH" -eq 1 ] && print_bench_results "$SERIAL_FILE"
        echo "=== Boot test PASSED ==="
        exit 0
    fi
done

# Clean up
kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

# Check final output
if [ -f "$SERIAL_FILE" ]; then
    if grep -q "$WAIT_MARKER" "$SERIAL_FILE"; then
        echo "$WAIT_MARKER found."
        if ! check_selftest_failures "$SERIAL_FILE"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but a self-test failed) ==="
            exit 1
        fi
        [ "$BENCH" -eq 1 ] && print_bench_results "$SERIAL_FILE"
        echo "=== Boot test PASSED ==="
        exit 0
    elif grep -q "PANIC\|FATAL" "$SERIAL_FILE"; then
        echo "KERNEL PANIC detected!"
        grep "PANIC\|FATAL\|EXCEPTION" "$SERIAL_FILE" || true
        echo "=== Boot test FAILED ==="
        exit 1
    fi
fi

# In --bench mode, BENCH_OK is not currently reachable: the deferred
# benchmark task livelocks in bench_pick_next (see known-issues.md "deferred
# benchmark suite hangs after context_switch").  So even on timeout, surface
# whatever benchmark numbers DID get captured — they are still useful for
# spotting regressions in the early benchmarks — before reporting failure.
if [ "$BENCH" -eq 1 ] && [ -f "$SERIAL_FILE" ] && grep -q "BOOT_OK" "$SERIAL_FILE"; then
    echo "Note: BOOT_OK reached but $WAIT_MARKER did not arrive within ${TIMEOUT}s."
    echo "      (Known issue: the deferred benchmark suite hangs in bench_pick_next."
    echo "       Partial benchmark numbers captured up to the hang are shown below.)"
    print_bench_results "$SERIAL_FILE"
    echo "=== Boot test FAILED ($WAIT_MARKER not reached) ==="
    exit 1
fi

echo "$WAIT_MARKER not found within ${TIMEOUT}s."
echo "=== Boot test FAILED ==="
exit 1
