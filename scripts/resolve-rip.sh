#!/usr/bin/env bash
# resolve-rip.sh — map a kernel RIP (or any code address) to the containing
# symbol + offset, using the unstripped kernel ELF.
#
# The boot image staged in build/esp/boot/kernel is *stripped* (llvm-strip in
# boot-test.sh), so it carries no symbols. The matching unstripped ELF with the
# full symbol table is target/x86_64-unknown-none/debug/kernel. As long as the
# kernel has NOT been rebuilt since the boot that produced the RIP, the two are
# the same image and addresses line up exactly.
#
# Typical use: after a "[hardlockup] NMI WATCHDOG FIRED cpu=… rip=0x…" line, run
#   scripts/resolve-rip.sh 0xffffffff8121a990
# to see which function the BSP was wedged in.
#
# Usage: scripts/resolve-rip.sh <hex-addr> [<hex-addr> ...]
set -u

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KERNEL="$PROJECT_ROOT/target/x86_64-unknown-none/debug/kernel"
NM="$HOME/.rustup/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/llvm-nm.exe"

if [ ! -f "$KERNEL" ]; then
    echo "error: unstripped kernel not found at $KERNEL (build the kernel first)" >&2
    exit 2
fi
if [ ! -x "$NM" ] && ! command -v "$NM" >/dev/null 2>&1; then
    # Fall back to any llvm-nm / nm on PATH.
    if command -v llvm-nm >/dev/null 2>&1; then NM="llvm-nm";
    elif command -v nm >/dev/null 2>&1; then NM="nm";
    else echo "error: no llvm-nm/nm found" >&2; exit 2; fi
fi

if [ "$#" -lt 1 ]; then
    echo "usage: $0 <hex-addr> [<hex-addr> ...]" >&2
    exit 2
fi

# Build a numerically-sorted (address, symbol) table once. Only code/text and
# other addressable symbols matter; keep them all and let the lookup pick the
# greatest address <= target. Demangle for readable Rust names.
TABLE="$("$NM" --numeric-sort --defined-only --demangle "$KERNEL" 2>/dev/null)"
if [ -z "$TABLE" ]; then
    echo "error: no symbols from $KERNEL (is it stripped?)" >&2
    exit 2
fi

for arg in "$@"; do
    # Normalise to 16-digit lowercase hex (no 0x). Kernel addresses live in the
    # top half (>= 2^63), which overflow both bash signed-64 arithmetic and
    # awk's double-precision strtonum — so we do NOT convert to a number.
    # Instead we compare fixed-width lowercase hex strings lexicographically,
    # which is a valid ordering when every address is exactly 16 hex digits.
    hex="${arg#0x}"; hex="${hex#0X}"
    hex="$(printf '%016s' "$hex" | tr ' ' '0' | tr 'A-F' 'a-f')"
    # llvm-nm --numeric-sort already emits ascending 16-digit addresses; the
    # answer is the last symbol whose address string <= target string.
    echo "$TABLE" | awk -v tgt="$hex" '
        # $1 = 16-digit hex address, $2 = type, $3.. = (demangled) name
        ($1 <= tgt) {
            best_hex = $1
            name = ""
            for (i = 3; i <= NF; i++) name = name (i>3 ? " " : "") $i
            best_name = name
        }
        END {
            if (best_name == "") { printf "0x%s -> (no symbol at or below this address)\n", tgt }
            else { printf "0x%s -> %s  (sym @ 0x%s)\n", tgt, best_name, best_hex }
        }'
done
