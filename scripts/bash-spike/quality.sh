#!/bin/bash
# Linking is not working: how many symbols bash actually uses are real
# implementations, and how many are stub-only / ENOSYS?
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
POSIX="$SLATE_ROOT/posix/src"

# Symbols bash needs that are provided ONLY by libstubs.a (not by libc.a).
comm -12 "$SLATE_TMP/bash_needs.txt" "$SLATE_TMP/stub_syms.txt" > "$SLATE_TMP/bash_stubonly_all.txt"
comm -23 "$SLATE_TMP/bash_stubonly_all.txt" "$SLATE_TMP/libc_syms.txt" > "$SLATE_TMP/bash_stubonly.txt"
echo "=== bash symbols served ONLY by libstubs.a: $(wc -l < "$SLATE_TMP/bash_stubonly.txt") ==="
head -40 "$SLATE_TMP/bash_stubonly.txt"

echo
echo "=== ENOSYS sites in the posix crate ==="
grep -rn "ENOSYS" "$POSIX" 2>/dev/null | wc -l
echo "--- which functions report ENOSYS (file:fn context) ---"
grep -rn "ENOSYS" "$POSIX" 2>/dev/null | sed 's|.*/posix/src/||' | head -30
