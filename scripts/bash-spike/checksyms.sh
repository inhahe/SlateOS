#!/bin/bash
# Confirm the three functions the spike originally had to shim are now real
# symbols in SlateOS's libc.a (implemented in posix/src, not stubbed).
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

cd "$SLATE_SYSROOT" || exit 1
nm --defined-only libc.a 2>/dev/null | awk '$2 ~ /^[TWDiRB]$/ {print $3}' | sort -u > "$SLATE_TMP/libc_syms.txt"
echo "libc.a defines: $(wc -l < "$SLATE_TMP/libc_syms.txt")"
for s in killpg eaccess euidaccess __fpurge; do
    if grep -qx "$s" "$SLATE_TMP/libc_syms.txt"; then echo "  PRESENT  $s"; else echo "  MISSING  $s"; fi
done
