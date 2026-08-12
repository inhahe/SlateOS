#!/bin/bash
# Confirm the three functions the spike originally had to shim are now real
# symbols in SlateOS's libc.a (implemented in posix/src, not stubbed).
cd "/mnt/d/visual studio projects/os/toolchain/sysroot/lib" || exit 1
nm --defined-only libc.a 2>/dev/null | awk '$2 ~ /^[TWDiRB]$/ {print $3}' | sort -u > /tmp/libc_syms.txt
echo "libc.a defines: $(wc -l < /tmp/libc_syms.txt)"
for s in killpg eaccess euidaccess __fpurge; do
    if grep -qx "$s" /tmp/libc_syms.txt; then echo "  PRESENT  $s"; else echo "  MISSING  $s"; fi
done
