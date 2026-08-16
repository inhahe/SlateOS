#!/bin/bash
# Cross-compile bash 5.2 -> x86_64-linux-musl (the ABI SlateOS's libc.a targets).
#
# Fixes over cross.sh: the compiler is reached through a wrapper on a path with
# no spaces (configure word-splits $CC, and the repo lives under "visual studio
# projects"), and the whole build happens on the Linux filesystem — /mnt/d is
# both space-laden and slow.
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

set -x
SPIKE="$SLATE_SPIKE"
# Lane-keyed: cross2.sh writes this tree and slatelink.sh reads it, so two lanes
# sharing one /tmp/bash-cross would relink one lane's objects into the other's
# shipped bash-slateos.elf. Keep this name in step with cross3.sh and
# slatelink.sh, which must agree on it.
BUILD="/tmp/bash-cross-$SLATE_LANE"

# The toolchain comes from slate_make_zig_wrappers, which resolves the pinned,
# hash-verified zig (per-worktree copy, else the shared cache, else download).
#
# This script used to require zig at "$SPIKE/zig/zig" and then hand-roll its own
# /tmp/zigcc wrappers. Both halves were wrong once worktree.sh started pinning
# the toolchain on 2026-08-16, and the fix that day only converted the *path*
# derivation here, not the provisioning -- so this script still demanded a
# hand-placed per-worktree zig and still wrote un-lane-keyed wrappers naming it.
# The stale /tmp/zigcc left over from 2026-08-14 pointed at
# `os/build/spike/zig/zig`, so a lane running this relinked a *shipped* artifact
# (design-decisions.md §305) with another worktree's unrecorded, unverified
# compiler -- the exact defect that day's work was supposed to close.
slate_make_zig_wrappers || exit 1
"$SLATE_ZIG" version || exit 1
"$SLATE_CC" --version || exit 1

# Sanity: can the wrapper build a hello-world at all?
echo 'int main(void){return 0;}' > "$SLATE_TMP/t.c"
"$SLATE_CC" "$SLATE_TMP/t.c" -o "$SLATE_TMP/t.out" && echo "WRAPPER_LINKS_OK" || { echo "WRAPPER_BROKEN"; exit 1; }

rm -rf "$BUILD"
mkdir -p "$BUILD"
tar xzf "$SPIKE/bash-5.2.tar.gz" -C "$BUILD" --strip-components=1 || exit 1
cd "$BUILD" || exit 1

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"
# --disable-readline drops termcap (9 of the 23 unresolved symbols); a spike only
# needs `bash -c` and script execution to prove the port is real.
./configure --host=x86_64-linux-musl --build=x86_64-pc-linux-gnu \
    --without-bash-malloc --disable-nls --disable-readline --without-curses \
    >cross-configure.log 2>&1
echo "CROSS_CONFIGURE_EXIT=$?"
tail -15 cross-configure.log

make -j8 >cross-make.log 2>&1
echo "CROSS_MAKE_EXIT=$?"
tail -25 cross-make.log

if [ -x "$BUILD/bash" ]; then
  file "$BUILD/bash"
  ls -l "$BUILD/bash"
  echo "CROSS_BASH_BUILT"
  cp "$BUILD/bash" "$SPIKE/bash-musl.elf"
else
  echo "NO_CROSS_BINARY"
fi
