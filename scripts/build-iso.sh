#!/usr/bin/env bash
# Build the kernel and create a bootable ISO image using Limine.
#
# Prerequisites:
#   - Rust toolchain with x86_64-unknown-none target (run setup-toolchain.sh)
#   - Limine cloned into ./limine/ (v8.x-binary branch)
#   - xorriso installed
#
# Usage:
#   ./scripts/build-iso.sh [release]
#
# Output: os.iso in the project root.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

PROFILE="${1:-debug}"
if [ "$PROFILE" = "release" ]; then
    CARGO_FLAGS="--release"
    TARGET_DIR="target/x86_64-unknown-none/release"
else
    CARGO_FLAGS=""
    TARGET_DIR="target/x86_64-unknown-none/debug"
fi

LIMINE_DIR="$PROJECT_ROOT/limine"
ISO_ROOT="$PROJECT_ROOT/target/iso_root"

# --- Sanity checks ---

if [ ! -d "$LIMINE_DIR" ]; then
    echo "ERROR: Limine not found at $LIMINE_DIR"
    echo "Clone it first:"
    echo "  git clone https://github.com/limine-bootloader/limine.git --branch=v8.x-binary --depth=1 limine"
    exit 1
fi

if ! command -v xorriso &>/dev/null; then
    echo "ERROR: xorriso not found. Install it:"
    echo "  Linux:   sudo apt install xorriso"
    echo "  MSYS2:   pacman -S xorriso"
    exit 1
fi

# --- Build kernel ---

echo "[1/4] Building kernel ($PROFILE)..."
cargo build $CARGO_FLAGS

KERNEL_BIN="$TARGET_DIR/kernel"
if [ ! -f "$KERNEL_BIN" ]; then
    echo "ERROR: Kernel binary not found at $KERNEL_BIN"
    exit 1
fi

# --- Create ISO root ---

echo "[2/4] Preparing ISO root..."
rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT/boot" "$ISO_ROOT/boot/limine" "$ISO_ROOT/EFI/BOOT"

cp "$KERNEL_BIN" "$ISO_ROOT/boot/kernel"
cp "$PROJECT_ROOT/limine.conf" "$ISO_ROOT/boot/limine/limine.conf"

# Copy Limine files.
cp "$LIMINE_DIR/limine-bios.sys" "$ISO_ROOT/boot/limine/" 2>/dev/null || true
cp "$LIMINE_DIR/limine-bios-cd.bin" "$ISO_ROOT/boot/limine/" 2>/dev/null || true
cp "$LIMINE_DIR/limine-uefi-cd.bin" "$ISO_ROOT/boot/limine/" 2>/dev/null || true
cp "$LIMINE_DIR/BOOTX64.EFI" "$ISO_ROOT/EFI/BOOT/" 2>/dev/null || true
cp "$LIMINE_DIR/BOOTIA32.EFI" "$ISO_ROOT/EFI/BOOT/" 2>/dev/null || true

# --- Create ISO ---

echo "[3/4] Creating ISO image..."
xorriso -as mkisofs \
    -b boot/limine/limine-bios-cd.bin \
    -no-emul-boot \
    -boot-load-size 4 \
    -boot-info-table \
    --efi-boot boot/limine/limine-uefi-cd.bin \
    -efi-boot-part \
    --efi-boot-image \
    --protective-msdos-label \
    "$ISO_ROOT" \
    -o "$PROJECT_ROOT/os.iso"

# --- Install Limine for BIOS boot ---

echo "[4/4] Installing Limine BIOS bootloader on ISO..."
"$LIMINE_DIR/limine" bios-install "$PROJECT_ROOT/os.iso" 2>/dev/null || true

echo ""
echo "=== ISO created: $PROJECT_ROOT/os.iso ==="
echo "Boot with: qemu-system-x86_64 -cdrom os.iso -serial stdio -m 256M"
