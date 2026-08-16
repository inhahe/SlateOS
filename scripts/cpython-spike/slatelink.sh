#!/bin/bash
# The decisive step: link CPython's already-compiled objects against SlateOS's
# OWN libc.a instead of zig's bundled musl.
#
# run.sh proved the source cross-compiles. This measures the thing we actually
# want to know: which libc symbols CPython needs that posix/src does not yet
# provide. The output is a list, and that list is directly actionable work.
#
# Compiling against musl headers and linking against our archive is legitimate,
# not a fudge: SlateOS's libc.a is built to the musl ABI. bash and pkgconf were
# both done exactly this way.
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="${CPYTHON_VER:-3.12.3}"
MINOR="${VER%.*}"
WORK="/tmp/cpython-spike-$SLATE_LANE"
BUILD="$WORK/Python-$VER"
SYSCOPY="/tmp/slate-sysroot-cpython-$SLATE_LANE"

[ -d "$BUILD" ] || {
    echo "ERROR: $BUILD does not exist — run scripts/cpython-spike/run.sh first."
    exit 1
}
cd "$BUILD" || exit 1

slate_make_zig_wrappers || exit 1

# Copy the archives off /mnt/d: the linker reads them heavily and 9p is slow.
mkdir -p "$SYSCOPY"
cp "$SLATE_SYSROOT/libc.a" "$SLATE_SYSROOT/libunwind.a" "$SYSCOPY/" || exit 1

LIBPY="libpython$MINOR.a"
[ -f "$LIBPY" ] || { echo "ERROR: $LIBPY not built"; exit 1; }
ls -l "$LIBPY" Programs/python.o

# How much libc surface does CPython actually reach for? Context for the
# missing count: bash referenced 2,030 symbols and was missing 3.
#
# `nm --undefined-only` on an *archive* lists every member's undefined symbols,
# and a static library is overwhelmingly self-referential: obmalloc.o's call to
# PyErr_NoMemory is "undefined" in obmalloc.o and defined in errors.o, two
# members later. So this raw count is "external references made by any member",
# not "symbols the archive cannot satisfy". The first version of this script
# differenced it straight against libc.a and reported 1,875 missing — a number
# that is 99% CPython's own API surface (PyAST_Check, PyArg_ParseTuple, …) and
# says nothing whatever about our libc. Subtracting the archive's own
# definitions below is what makes the figure mean what its name claims.
echo "=== undefined symbols CPython references (raw, incl. intra-archive) ==="
nm --undefined-only Programs/python.o "$LIBPY" 2>/dev/null \
    | awk '$1 == "U" {print $2}' | sort -u > "$SLATE_TMP/cpython_undef_raw.txt"
echo "REFERENCED_RAW=$(wc -l < "$SLATE_TMP/cpython_undef_raw.txt")"

# What CPython resolves within itself.
nm --defined-only Programs/python.o "$LIBPY" 2>/dev/null \
    | awk '$2 ~ /^[TWDiRBGSVC]$/ {print $3}' | sort -u > "$SLATE_TMP/cpython_defines.txt"
echo "CPYTHON_SELF_DEFINES=$(wc -l < "$SLATE_TMP/cpython_defines.txt")"

# The honest figure: external symbols CPython genuinely needs somebody else to
# provide. This is the number comparable to bash's 2,030.
comm -23 "$SLATE_TMP/cpython_undef_raw.txt" "$SLATE_TMP/cpython_defines.txt" \
    > "$SLATE_TMP/cpython_needs.txt"
echo "REFERENCED_EXTERNAL=$(wc -l < "$SLATE_TMP/cpython_needs.txt")"

echo "=== symbols our sysroot defines ==="
nm --defined-only "$SYSCOPY/libc.a" "$SYSCOPY/libunwind.a" 2>/dev/null \
    | awk '$2 ~ /^[TWDiRBGSVC]$/ {print $3}' | sort -u > "$SLATE_TMP/libc_syms_cpython.txt"
echo "SYSROOT_DEFINES=$(wc -l < "$SLATE_TMP/libc_syms_cpython.txt")"

# Worth computing separately from the link: the linker only reports symbols on
# paths it actually pulled in, so its list is a lower bound on what a *fuller*
# CPython would need. The set difference is the upper bound. Both are useful;
# neither alone is the whole truth.
comm -23 "$SLATE_TMP/cpython_needs.txt" "$SLATE_TMP/libc_syms_cpython.txt" \
    > "$SLATE_TMP/cpython_missing_static.txt"
echo "MISSING_BY_SET_DIFFERENCE=$(wc -l < "$SLATE_TMP/cpython_missing_static.txt")"
echo "--- the list ---"
cat "$SLATE_TMP/cpython_missing_static.txt"

# -nostdlib: we want SlateOS's libc, not zig's musl.
# libc.a twice: it is Rust-built and its intra-archive references are not
# topologically ordered, so a second pass is cheaper than --start-group.
# libstubs.a is deliberately NOT linked — it and libc.a each carry a panic
# handler and collide on __rustc::rust_begin_unwind (same as bash/pkgconf).
echo "=== attempting the real link ==="
"$SLATE_CC" -static -nostdlib -o python-slateos Programs/python.o "$LIBPY" \
    "$SYSCOPY/libc.a" "$SYSCOPY/libc.a" "$SYSCOPY/libunwind.a" \
    2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

grep -oP "undefined symbol: \K.*" slate-link.log | sort -u > "$SLATE_TMP/cpython_missing_link.txt"
echo "MISSING_AT_LINK=$(wc -l < "$SLATE_TMP/cpython_missing_link.txt")"
head -80 "$SLATE_TMP/cpython_missing_link.txt"
echo "=== other link errors (non-undefined-symbol) ==="
grep -v "undefined symbol\|^>>>" slate-link.log | head -20

if [ -x python-slateos ]; then
    file python-slateos
    ls -l python-slateos
    cp python-slateos "$SLATE_SPIKE/python-slateos.elf"
    echo "SLATE_CPYTHON_BUILT"
else
    echo "NO_SLATE_BINARY"
fi
