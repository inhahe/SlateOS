#!/bin/bash
# Link genuine Oils' built objects against the CURRENT libc.a, and stage it.
#
# scripts/oils-spike/run.sh fetches Oils, configures it and compiles it -- the
# minutes -- and then runs this, the seconds: the link against
# toolchain/sysroot/lib/libc.a, its checks, and the stripped copy in
# build/spike/. Every libc.a rebuild leaves that link behind, and
# scripts/create-ext4-rootfs.sh refuses to stage a binary older than the
# library it links, so the recipe runs this alone when the artifact is behind
# libc.a (`spike_rebuild_if_behind`), as it runs bash's, eSpeak's, CPython's
# and LLVM's relinks. The objects do not depend on the C library -- only on
# its headers, which are musl's plus posix/include, and run.sh is what a
# header change needs.
#
# If run.sh has never run here the objects are missing; this says so and
# fails, and the recipe then reports the artifact absent, which is a NOTE.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/oils-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="0.38.0"
SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/oils-spike"
TREE="$WORK/oils-for-unix-$VER"
# Named for the build wrapper run.sh compiles with (slatecxx), in -opt-sh.
OBJDIR="_build/obj/slatecxx-opt-sh"
# libc.a off the /mnt mount: 9p is slow, and a linker reads an archive many
# times.
SPIKE_LIBS="$WORK/libs"

if [ ! -d "$TREE/$OBJDIR" ]; then
    echo "NO_OBJECTS -- $TREE/$OBJDIR does not exist: Oils has not been built here."
    echo "  Build it first: wsl -d Ubuntu --exec bash scripts/oils-spike/run.sh"
    exit 1
fi
cd "$TREE" || exit 1

OBJS="$(find "$OBJDIR" -name '*.o' | sort | tr '\n' ' ')"
echo "OBJ_COUNT=$(echo "$OBJS" | wc -w)"
if [ -z "${OBJS// /}" ]; then
    echo "NO_OBJECTS -- $TREE/$OBJDIR holds none; rebuild with run.sh"
    exit 1
fi

# Through scripts/lib/worktree.sh's link wrapper -- zig's ld.lld itself, given
# the objects and then our libc.a, with zig's C++ runtime ahead of it and its
# compiler runtime behind, in zig's own order. Not zig's c++ driver with
# -nostdlib, as until 2026-10-05: that puts zig's own musl libc.a behind every
# link, where it supplies whatever ours lacks instead of a missing symbol being
# reported (known-issues-resolved/D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC.md).
# The wrapper passes --eh-frame-hdr, which the shell cannot do without: every
# shell error is a C++ exception, and libunwind finds a static program's unwind
# tables through the PT_GNU_EH_FRAME segment that flag makes -- checked below.
# Its own directory, not run.sh's bin/: that is on PATH during the build, and
# a cc and c++ there would be found by anything that asks for them by name.
mkdir -p "$SPIKE_LIBS" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/link" "$SPIKE_LIBS" || exit 1

# Removed first: ld.lld leaves an existing output alone when a link fails, and
# the checks below would then stage the last run's binary as this one's.
rm -f oils-for-unix-slateos
# shellcheck disable=SC2086  # word splitting is what builds the object list
"$SLATE_LINK_CXX" -Wl,--gc-sections -o oils-for-unix-slateos $OBJS 2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

# Both counts, always, and neither inferred from the other: they measure
# opposite failures (scripts/cmake-spike/run.sh tells the run that taught it).
MISSING="$WORK/missing.txt"
grep -oP "undefined symbol: \K.*" slate-link.log | sort -u >"$MISSING"
echo "MISSING_COUNT=$(wc -l <"$MISSING")"
head -40 "$MISSING"
DUPES="$WORK/dupes.txt"
grep -oP "duplicate symbol: \K.*" slate-link.log | sort -u >"$DUPES"
echo "DUPLICATE_COUNT=$(wc -l <"$DUPES")"
head -40 "$DUPES"
grep -v "undefined symbol\|duplicate symbol\|^>>>" slate-link.log | head -20

if [ ! -x oils-for-unix-slateos ] || [ -s "$MISSING" ] || [ -s "$DUPES" ]; then
    echo "NO_SLATE_BINARY -- see $TREE/slate-link.log"
    exit 1
fi
file oils-for-unix-slateos
readelf -h oils-for-unix-slateos | grep -E "Type|Entry"
# `grep >/dev/null`, not `grep -q`: under pipefail, -q exits at the first
# match, readelf dies of SIGPIPE, and the pipeline reads as a failure -- the
# first run of run.sh reported a missing segment that was there.
if ! readelf -lW oils-for-unix-slateos | grep GNU_EH_FRAME >/dev/null; then
    echo "NO_EH_FRAME_HDR -- every exception would terminate the shell"
    exit 1
fi
# The SlateOS ABI note, which posix/src/crt.rs emits beside _start: the
# kernel runs an ELF without it as a Linux program (kernel/src/proc/elf.rs),
# whatever its link said.
if ! readelf -n oils-for-unix-slateos | grep SlateOS >/dev/null; then
    echo "NO_SLATEOS_NOTE -- the kernel would run this as a Linux program"
    exit 1
fi
echo "UNSTRIPPED_BYTES=$(stat -c %s oils-for-unix-slateos)"

# --strip-debug, not --strip-all, as for CPython and CMake (design-decisions.md
# §344): the symbol table is what turns a fault address on the serial console
# into a function name. Written beside the artifact and moved over it, so the
# recipe never stages half a file.
mkdir -p "$SLATE_SPIKE" || exit 1
STAGED="$SLATE_SPIKE/oils-for-unix-slateos.elf"
strip --strip-debug -o "$STAGED.new" oils-for-unix-slateos 2>/dev/null \
    || "$SLATE_ZIG" objcopy --strip-debug oils-for-unix-slateos "$STAGED.new" \
    || { rm -f "$STAGED.new"; echo "STRIP_FAILED"; exit 1; }
mv "$STAGED.new" "$STAGED" || exit 1
echo "STAGED_BYTES=$(stat -c %s "$STAGED")"
ls -l "$STAGED"
echo "SLATE_OILS_LINKED=$STAGED"
