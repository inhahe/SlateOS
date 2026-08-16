#!/bin/bash
# Spike helper: what does SlateOS's libc.a define, and what does bash need?
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
BASHDIR="$SLATE_SPIKE/bash-5.2"

cd "$SYSROOT" || exit 1
# Defined symbols (text/data/weak/indirect) provided by the SlateOS libc + stubs.
nm --defined-only libc.a 2>/dev/null | awk '$2 ~ /^[TWDiRB]$/ {print $3}' | sort -u > "$SLATE_TMP/libc_syms.txt"
nm --defined-only libstubs.a 2>/dev/null | awk '$2 ~ /^[TWDiRB]$/ {print $3}' | sort -u > "$SLATE_TMP/stub_syms.txt"
cat "$SLATE_TMP/libc_syms.txt" "$SLATE_TMP/stub_syms.txt" | sort -u > "$SLATE_TMP/provided.txt"
echo "libc.a defines:    $(wc -l < "$SLATE_TMP/libc_syms.txt")"
echo "libstubs.a defines:$(wc -l < "$SLATE_TMP/stub_syms.txt")"
echo "provided total:    $(wc -l < "$SLATE_TMP/provided.txt")"

# Undefined symbols referenced by every bash object file.
#
# If this directory is absent, that is the correct answer and not a regression:
# the script now reads *this* worktree, and bash's source tree is unpacked
# per-worktree. Before 2026-08-16 it silently read `os`'s copy no matter where
# it ran, which is worse than failing — it reported another tree's symbols as
# though they were this one's.
if [ ! -d "$BASHDIR" ]; then
    echo "ERROR: $BASHDIR does not exist."
    echo "       bash's source tree is not unpacked in this worktree ($SLATE_LANE)."
    echo "       See scripts/bash-spike/README.md for the fetch + configure + make step."
    exit 1
fi
cd "$BASHDIR" || exit 1
find . -name '*.o' -print0 | xargs -0 nm --undefined-only 2>/dev/null \
  | awk '$1 == "U" {print $2}' | sed 's/@.*//' | sort -u > "$SLATE_TMP/bash_needs.txt"
echo "bash references:   $(wc -l < "$SLATE_TMP/bash_needs.txt")"

# What bash needs that SlateOS does not provide (ignore bash-internal symbols,
# which resolve among bash's own objects).
comm -23 "$SLATE_TMP/bash_needs.txt" "$SLATE_TMP/provided.txt" > "$SLATE_TMP/missing_all.txt"
echo "unresolved (incl. bash-internal): $(wc -l < "$SLATE_TMP/missing_all.txt")"

# Drop symbols bash itself defines somewhere in its own objects.
find . -name '*.o' -print0 | xargs -0 nm --defined-only 2>/dev/null \
  | awk '$2 ~ /^[TWDiRBCS]$/ {print $3}' | sort -u > "$SLATE_TMP/bash_defines.txt"
comm -23 "$SLATE_TMP/missing_all.txt" "$SLATE_TMP/bash_defines.txt" > "$SLATE_TMP/missing_libc.txt"
echo "=== MISSING FROM SLATEOS LIBC: $(wc -l < "$SLATE_TMP/missing_libc.txt") ==="
cat "$SLATE_TMP/missing_libc.txt"
