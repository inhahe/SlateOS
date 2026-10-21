#!/bin/bash
# Bring the staged CMake up to date with the CURRENT libc.a, doing the least that does it.
#
# scripts/cmake-spike/run.sh unpacks CMake, configures it and compiles it --
# from a fresh tree every time, about four minutes -- and then runs this for
# the link against toolchain/sysroot/lib/libc.a, its checks, and the stripped
# copy in build/spike/. scripts/create-ext4-rootfs.sh runs this by itself
# before every image build, and this decides how much is needed:
#
#   * nothing, when what run.sh configured against is unchanged (configured.sh)
#     and the staged binary is newer than libc.a: about a second;
#   * the link alone, when libc.a has moved but its C-visible names and
#     posix/include have not -- most libc changes: seconds;
#   * run.sh, when they have, or when no build here records what it was
#     configured against: CMake's configure probes our libc.a and our headers,
#     so its objects are only as current as that configuration.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/cmake-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1
. "$(dirname "${BASH_SOURCE[0]}")/configured.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/cmake-spike"
# run.sh's: the archives off the /mnt mount and the wrapper bld/CMakeCache.txt
# records as its compiler, both rewritten from the current sysroot below.
SPIKE_LIBS="$WORK/libs"
RECORD="$WORK/configured.txt"
STAGED="$SLATE_SPIKE/cmake-slateos.elf"
RUN_SH="$SLATE_ROOT/scripts/cmake-spike/run.sh"

# The pinned zig, as run.sh's setup settles it: the fingerprint names it, and
# the strip below falls back to it.
slate_ensure_zig || exit 1
CURRENT="$(cmake_configure_fingerprint "$SYSROOT/libc.a" "$SLATE_ROOT/posix/include")" \
    || { echo "NO_FINGERPRINT -- configured.sh could not read libc.a or posix/include"; exit 1; }
if [ "$(cat "$RECORD" 2>/dev/null)" != "$CURRENT" ]; then
    if [ "${SLATE_CMAKE_FROM_RUN:-0}" = "1" ]; then
        # run.sh recorded what it configured against moments ago; a difference
        # now means libc.a or posix/include changed during its build.
        echo "CONFIGURATION_MOVED -- libc.a or posix/include changed while run.sh was"
        echo "  building; run it again: wsl -d Ubuntu --exec bash scripts/cmake-spike/run.sh"
        exit 1
    fi
    echo "RECONFIGURE -- the build here was configured against something else, or"
    echo "  records nothing (run.sh never finished here). Configured: $(cat "$RECORD" 2>/dev/null || echo none)"
    echo "  Now:        $CURRENT"
    echo "  Handing over to run.sh, the full configure and build."
    exec bash "$RUN_SH"
fi
if [ "${SLATE_CMAKE_FROM_RUN:-0}" != "1" ] && [ "$STAGED" -nt "$SYSROOT/libc.a" ]; then
    echo "SLATE_CMAKE_CURRENT=$STAGED"
    exit 0
fi

# Relative paths from here on: the object and archive lists are word-split on
# the link line, which no path under $WORK may break.
cd "$WORK" || exit 1

# Take the object list from cmake's own build rather than globbing: the tree
# ships Tests/ and Utilities/ subtrees whose objects the real link does not use.
# The `cmake` executable's objects are the ones CMakeFiles/cmake.dir holds, plus
# the static libraries the build produced for it.
CMAKE_OBJDIR="$(find bld -type d -name 'cmake.dir' -print -quit 2>/dev/null)"
OBJS=""
if [ -n "$CMAKE_OBJDIR" ]; then
    OBJS="$(find "$CMAKE_OBJDIR" -name '*.o' | sort | tr '\n' ' ')"
fi
echo "OBJ_COUNT=$(echo "$OBJS" | wc -w)"
if [ -z "${OBJS// /}" ]; then
    echo "NO_OBJECTS -- $WORK/bld holds no cmake.dir objects; rebuild with run.sh"
    exit 1
fi

# Every static archive the build produced, which is where CMakeLib and the
# bundled third-party libraries live, in no particular order: ld.lld takes a
# symbol from any archive on the line, wherever the reference is, so the
# order the linker needs is not something this has to know.
LIBS="$(find bld -name '*.a' | sort | tr '\n' ' ')"
echo "ARCHIVE_COUNT=$(echo "$LIBS" | wc -w)"

# The decisive step: a link against SlateOS's libc and nothing else, through
# scripts/lib/worktree.sh's link wrapper -- zig's ld.lld itself, given the
# build's own inputs and then our libc.a, with zig's C++ runtime ahead of it
# and zig's compiler runtime behind it, which is zig's own order (the
# wrapper's comment says why the order matters). Not zig's c++ driver with
# -nostdlib: that puts zig's own musl libc.a behind every link, where it would
# supply whatever ours lacks instead of a missing symbol being reported
# (known-issues D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC). libstubs.a is
# deliberately not linked -- it and libc.a each carry a panic handler and
# collide on __rustc::rust_begin_unwind.
mkdir -p "$SPIKE_LIBS" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
echo "LINK_WRAPPER=$SLATE_LINK_CXX:"
grep '^libs=' "$SLATE_LINK_CXX"

# Removed first: ld.lld leaves an existing output alone when a link fails,
# and the checks below would then stage the last run's binary as this one's.
# --error-limit=0: ld.lld stops at 20 errors by default, which would cap both
# counts below and could leave the missing symbols uncounted behind duplicates.
rm -f cmake-slateos
# shellcheck disable=SC2086  # word splitting is what builds the object list
"$SLATE_LINK_CXX" -Wl,--error-limit=0 -o cmake-slateos $OBJS $LIBS 2>slate-link.log
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
strip --strip-debug -o "$STAGED.new" cmake-slateos 2>/dev/null \
    || "$SLATE_ZIG" objcopy --strip-debug cmake-slateos "$STAGED.new" \
    || { rm -f "$STAGED.new"; echo "STRIP_FAILED"; exit 1; }
mv "$STAGED.new" "$STAGED" || exit 1
echo "STAGED_BYTES=$(stat -c %s "$STAGED")"
ls -l "$STAGED"
echo "SLATE_CMAKE_LINKED=$STAGED"
