#!/usr/bin/env bash
# Install the Rust toolchain and other dependencies needed to build the kernel.
#
# Run this once on a fresh machine.  Safe to re-run (idempotent).
#
# Prerequisites:
#   - Internet connection
#   - On Windows: run from Git Bash, MSYS2, or WSL
#   - QEMU must be installed separately (https://www.qemu.org/download/)

set -euo pipefail

echo "=== OS Kernel Toolchain Setup ==="

# 1. Install rustup if not present.
if ! command -v rustup &>/dev/null; then
    echo "[1/5] Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
else
    echo "[1/5] rustup already installed."
fi

# 2. Install/update stable toolchain.
echo "[2/5] Installing stable Rust toolchain..."
rustup toolchain install stable
rustup default stable

# 3. Add the bare-metal x86_64 target.
echo "[3/5] Adding x86_64-unknown-none target..."
rustup target add x86_64-unknown-none

# 4. Install rust-src (needed if we ever use build-std).
echo "[4/5] Installing rust-src component..."
rustup component add rust-src

# 5. Install additional tools.
echo "[5/5] Installing clippy and rustfmt..."
rustup component add clippy rustfmt

# Verify.
echo ""
echo "=== Verification ==="
rustc --version
cargo --version
echo "Target x86_64-unknown-none:"
rustup target list --installed | grep x86_64-unknown-none && echo "  OK" || echo "  MISSING"

echo ""
echo "=== Manual Steps ==="
echo "1. Install QEMU: https://www.qemu.org/download/"
echo "   On Windows: download the installer."
echo "   On Linux:   sudo apt install qemu-system-x86"
echo "2. Install xorriso (for ISO creation):"
echo "   On Linux:   sudo apt install xorriso"
echo "   On Windows: available via MSYS2 (pacman -S xorriso)"
echo "3. Clone Limine bootloader:"
echo "   git clone https://github.com/limine-bootloader/limine.git --branch=v8.x-binary --depth=1 limine"
echo ""
echo "Done."
