#!/bin/bash
# Relink eSpeak NG's already-built objects against the CURRENT libc.a.
#
# scripts/espeak-spike/run.sh configures and builds eSpeak NG from its pinned
# source, compiles the phoneme data with the program it built, and links
# `-nostdlib` against toolchain/sysroot/lib/libc.a.  Every libc.a rebuild
# leaves that link behind, and scripts/create-ext4-rootfs.sh refuses to stage
# a binary older than the library it links -- so this script redoes only the
# last step, as scripts/bash-spike/slatelink.sh does for bash: the objects
# and the data are untouched (the data does not depend on the C library; it
# was compiled by a zig-musl build of eSpeak that runs under WSL).
#
# create-ext4-rootfs.sh runs it for you when the artifact is behind libc.a
# (`spike_rebuild_if_behind`).  If run.sh has never run in this worktree the
# build tree is missing, and this says so and fails: the artifact then cannot
# exist either, which the rootfs script reports as a NOTE, not an error.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/espeak-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/espeak-spike"
SPIKE_LIBS="$SLATE_TMP/espeak-sysroot"

if [ ! -d "$WORK/bld" ]; then
    echo "ERROR: $WORK/bld does not exist -- eSpeak NG has not been built here."
    echo "       Build it first: wsl -d Ubuntu --exec bash scripts/espeak-spike/run.sh"
    exit 1
fi
cd "$WORK" || exit 1

slate_make_zig_wrappers || exit 1

mkdir -p "$SPIKE_LIBS"
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1

# The same inputs as run.sh's link, taken from the build tree the same way.
MAIN_OBJ="$(find bld -path '*espeak-ng-bin.dir*' -name '*.o' | sort | tr '\n' ' ')"
LIBS="$(find bld -name '*.a' | sort | tr '\n' ' ')"
if [ -z "$MAIN_OBJ" ] || [ -z "$LIBS" ]; then
    echo "ERROR: no objects under $WORK/bld -- rebuild with run.sh"
    exit 1
fi

# Unquoted on purpose, as in run.sh: space-separated lists of paths under
# bld/, which CMake names without spaces.
# shellcheck disable=SC2086
"$SLATE_CC" -static -nostdlib -o espeak-ng-slateos.new $MAIN_OBJ $LIBS $LIBS \
    "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libunwind.a" \
    2>slate-relink.log
rc=$?
MISSING="$(grep -oP 'undefined symbol: \K.*' slate-relink.log | sort -u)"
if [ "$rc" -ne 0 ] || [ -n "$MISSING" ] || [ ! -x espeak-ng-slateos.new ]; then
    echo "ERROR: the relink failed (exit $rc); see $WORK/slate-relink.log"
    [ -n "$MISSING" ] && echo "       undefined: $(echo "$MISSING" | head -10 | tr '\n' ' ')"
    rm -f espeak-ng-slateos.new
    exit 1
fi
mv espeak-ng-slateos.new espeak-ng-slateos || exit 1
mkdir -p "$SLATE_SPIKE"
cp espeak-ng-slateos "$SLATE_SPIKE/espeak-ng-slateos.elf.new" || exit 1
mv "$SLATE_SPIKE/espeak-ng-slateos.elf.new" "$SLATE_SPIKE/espeak-ng-slateos.elf" || exit 1
echo "SLATE_ESPEAK_RELINKED=$SLATE_SPIKE/espeak-ng-slateos.elf"
