#!/bin/bash
# Link genuine CMake's built objects against the CURRENT libc.a, and stage it.
#
# scripts/cmake-spike/run.sh unpacks CMake, configures it and compiles it --
# every time from a fresh tree, the four minutes -- and then runs this, the
# link against toolchain/sysroot/lib/libc.a, its checks, and the stripped copy
# in build/spike/. Every libc.a rebuild leaves that link behind, and
# scripts/create-ext4-rootfs.sh refuses to stage a binary older than the
# library it links, so the recipe runs this alone when the artifact is behind
# libc.a (`spike_rebuild_if_behind`), as it runs the other ports' relinks. The
# objects do not depend on the C library -- only on its headers, which are
# musl's plus posix/include, and run.sh is what a header change needs. The
# module tree run.sh stages beside the binary (build/spike/cmake-data) does not
# depend on it at all.
#
# If run.sh has never run here the objects are missing; this says so and
# fails, and the recipe then reports the artifact stale.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/cmake-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/cmake-spike"
BLD="$WORK/bld"
# libc.a off the /mnt mount: 9p is slow, and a linker reads an archive many
# times.
SPIKE_LIBS="$WORK/libs"

# Take the object list from cmake's own build rather than globbing: the tree
# ships Tests/ and Utilities/ subtrees whose objects the real link does not use.
# The `cmake` executable's objects are the ones CMakeFiles/cmake.dir holds, plus
# the static libraries the build produced for it.
CMAKE_OBJDIR="$(find "$BLD" -type d -name 'cmake.dir' -print -quit 2>/dev/null)"
if [ -z "$CMAKE_OBJDIR" ]; then
    echo "NO_OBJECTS -- $BLD has no cmake.dir: CMake has not been built here."
    echo "  Build it first: wsl -d Ubuntu --exec bash scripts/cmake-spike/run.sh"
    exit 1
fi
cd "$WORK" || exit 1

OBJS="$(find "$CMAKE_OBJDIR" -name '*.o' | sort | tr '\n' ' ')"
echo "OBJ_COUNT=$(echo "$OBJS" | wc -w)"
if [ -z "${OBJS// /}" ]; then
    echo "NO_OBJECTS -- $CMAKE_OBJDIR holds none; rebuild with run.sh"
    exit 1
fi

# Every static archive the build produced, which is where CMakeLib and the
# bundled third-party libraries live, in no particular order: ld.lld takes a
# symbol from any archive on the line, wherever the reference is, so the
# order the linker needs is not something this has to know. (They were
# passed twice until 2026-10-01, for GNU ld's sake; the binary is
# byte-identical either way.)
LIBS="$(find "$BLD" -name '*.a' | sort | tr '\n' ' ')"
echo "ARCHIVE_COUNT=$(echo "$LIBS" | wc -w)"

# The decisive step: a link against SlateOS's libc and nothing else, through
# scripts/lib/worktree.sh's link wrapper -- zig's ld.lld itself, given the
# build's own inputs and then our libc.a, with zig's C++ runtime ahead of it
# and zig's compiler runtime behind it, which is zig's own order (the
# wrapper's comment says why the order matters). Not zig's c++ driver with
# -nostdlib, as until 2026-10-01: that puts zig's own musl libc.a behind
# every link, where it would supply whatever ours lacks instead of a missing
# symbol being reported (known-issues D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC).
# libstubs.a is deliberately not linked -- it and libc.a each carry a panic
# handler and collide on __rustc::rust_begin_unwind. Its own directory, not
# run.sh's bin/: that is on PATH during the build, and a cc and c++ there
# would be found by anything that asks for them by name.
mkdir -p "$SPIKE_LIBS" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/link" "$SPIKE_LIBS" || exit 1
echo "LINK_WRAPPER=$SLATE_LINK_CXX:"
grep '^libs=' "$SLATE_LINK_CXX"

# Removed first: ld.lld leaves an existing output alone when a link fails,
# and the checks below would then stage the last run's binary as this one's.
rm -f cmake-slateos
# shellcheck disable=SC2086  # word splitting is what builds the object list
"$SLATE_LINK_CXX" -o cmake-slateos $OBJS $LIBS 2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

# Counted SEPARATELY, and MISSING_COUNT is never allowed to stand in for "the
# link succeeded". The make spike's first run printed SLATE_LINK_EXIT=1 beside
# MISSING_COUNT=0 and was briefly read as a fluke, because the only number on
# screen was the one saying everything was fine: all eleven errors were
# *duplicate* definitions, which the first grep does not match. The two counts
# measure opposite failures -- nothing is absent vs. something is present twice
# -- and a libc can fail either way, so both print unconditionally.
MISSING="$WORK/missing.txt"
grep -oP "undefined symbol: \K.*" slate-link.log | sort -u >"$MISSING"
echo "MISSING_COUNT=$(wc -l <"$MISSING")"
head -40 "$MISSING"
DUPES="$WORK/dupes.txt"
grep -oP "duplicate symbol: \K.*" slate-link.log | sort -u >"$DUPES"
echo "DUPLICATE_COUNT=$(wc -l <"$DUPES")"
head -40 "$DUPES"
grep -v "undefined symbol\|duplicate symbol\|^>>>" slate-link.log | head -20

if [ ! -x cmake-slateos ] || [ -s "$MISSING" ] || [ -s "$DUPES" ]; then
    echo "NO_SLATE_BINARY -- see $WORK/slate-link.log"
    exit 1
fi
file cmake-slateos
readelf -h cmake-slateos | grep -E "Type|Entry"
# `grep >/dev/null`, not `grep -q`: under pipefail, -q exits at the first
# match, readelf dies of SIGPIPE, and the pipeline reads as a failure.
# CMake is C++, and libunwind finds a static program's unwind tables through
# the PT_GNU_EH_FRAME segment: without it, any exception terminates the program.
if ! readelf -lW cmake-slateos | grep GNU_EH_FRAME >/dev/null; then
    echo "NO_EH_FRAME_HDR -- any exception would terminate cmake"
    exit 1
fi
# The SlateOS ABI note, which posix/src/crt.rs emits beside _start: the
# kernel runs an ELF without it as a Linux program (kernel/src/proc/elf.rs),
# whatever its link said.
if ! readelf -n cmake-slateos | grep SlateOS >/dev/null; then
    echo "NO_SLATEOS_NOTE -- the kernel would run this as a Linux program"
    exit 1
fi
echo "UNSTRIPPED_BYTES=$(stat -c %s cmake-slateos)"

# Stage the STRIPPED binary. Unstripped this is 274 MB of mostly `debug_info`,
# which would not fit beside the rest of the rootfs, and an artifact that
# cannot be staged is not a port -- so a failed strip fails the link rather
# than staging the unstripped one. `--strip-debug` and not `--strip-all`, as
# for CPython (design-decisions.md §344): the symbol table costs 7 MB and is
# what makes a fault address on the serial console into a function name.
# Written beside the artifact and moved over it, so the recipe never stages
# half a file.
mkdir -p "$SLATE_SPIKE" || exit 1
STAGED="$SLATE_SPIKE/cmake-slateos.elf"
strip --strip-debug -o "$STAGED.new" cmake-slateos 2>/dev/null \
    || "$SLATE_ZIG" objcopy --strip-debug cmake-slateos "$STAGED.new" \
    || { rm -f "$STAGED.new"; echo "STRIP_FAILED"; exit 1; }
mv "$STAGED.new" "$STAGED" || exit 1
echo "STAGED_BYTES=$(stat -c %s "$STAGED")"
ls -l "$STAGED"
echo "SLATE_CMAKE_LINKED=$STAGED"
