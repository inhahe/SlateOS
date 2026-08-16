#!/bin/bash
# Cross-compile upstream pkgconf and link it against SlateOS's own libc.a.
#
# This is the "try the port before you write a line" step from
# roadmap-detailed.md's "Porting vs. Reimplementing" policy, applied to
# pkgconf. It is deliberately one script: unlike the bash spike, nothing here
# needed a second attempt, a source patch or a shim, so there is no state to
# resume from.
#
# Run it from WSL. Everything except the sysroot lives in /tmp, because the
# linker reads the archives heavily and the 9p mount to /mnt/d is slow.
set -euo pipefail
set -x

# Locate the worktree this script lives in. It used to name
# `/mnt/d/visual studio projects/os` outright in four places, which with four
# checkouts meant a lane linking against another lane's libc.a — an artifact
# that proves nothing about the tree it was produced in. The bash spike had the
# identical bug with worse consequences; see scripts/lib/worktree.sh.
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER=2.3.0
SYSROOT="$SLATE_SYSROOT"
# Scratch dirs are keyed by worktree so two lanes building at once cannot write
# each other's objects or sysroot copy.
WORK="/tmp/pkgconf-spike-$SLATE_LANE"
SPIKE_LIBS="/tmp/slate-sysroot2-$SLATE_LANE"

# zig cc as the cross compiler, exactly as the bash spike and fastpy's own
# toolchain.py do. SlateOS's libc.a is built to match the musl ABI, so
# compiling against zig's musl headers and then linking against our archive is
# legitimate rather than a fudge. `slate_make_zig_wrappers` also documents and
# handles the $CC-must-not-contain-spaces trap.
slate_make_zig_wrappers || exit 1

mkdir -p "$WORK" && cd "$WORK"
[ -f "pkgconf-$VER.tar.xz" ] || curl -sSLO "https://distfiles.ariadne.space/pkgconf/pkgconf-$VER.tar.xz"
[ -d "pkgconf-$VER" ] || tar xf "pkgconf-$VER.tar.xz"
cd "pkgconf-$VER"

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

# --disable-shared: SlateOS has no dynamic loader on this path, so everything
# is a static ET_EXEC.
./configure --host=x86_64-linux-musl --disable-shared --enable-static >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"

make -j8 >make.log 2>&1
echo "MAKE_EXIT=$?"
grep -iE "error|warning: implicit" make.log | head -20 || true

# The decisive step. -nostdlib so we get SlateOS's libc, not zig's bundled
# musl. libc.a is listed twice because it is Rust-built and its intra-archive
# references are not topologically ordered; a second pass is cheaper than
# --start-group. libstubs.a is deliberately NOT linked: it and libc.a each
# carry a panic handler and collide on __rustc::rust_begin_unwind, and libc.a
# turns out to cover pkgconf entirely on its own.
mkdir -p "$SPIKE_LIBS"
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/"

OBJS="cli/pkgconf-main.o cli/pkgconf-getopt_long.o cli/pkgconf-renderer-msvc.o"
"$SLATE_CC" -static -nostdlib -o pkgconf-slateos $OBJS .libs/libpkgconf.a \
    "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libunwind.a" \
    2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

MISSING="/tmp/pkgconf_missing-$SLATE_LANE.txt"
grep -oP "undefined symbol: \K.*" slate-link.log | sort -u >"$MISSING" || true
echo "MISSING_COUNT=$(wc -l <"$MISSING")"
cat "$MISSING"
grep -v "undefined symbol\|^>>>" slate-link.log | head -20 || true

if [ -x pkgconf-slateos ]; then
  file pkgconf-slateos
  readelf -h pkgconf-slateos | grep -E "Type|Entry"
  # Stage it where the rootfs build looks, exactly as the bash spike does with
  # bash-slateos.elf. Leaving the only copy in /tmp is why this port has been
  # "proven to work" since 2026-08-14 and has never been in an image: a
  # /tmp artifact cannot be staged, cannot be staleness-checked, and does not
  # survive a reboot of the host.
  cp pkgconf-slateos "$SLATE_SPIKE/pkgconf-slateos.elf"
  ls -l "$SLATE_SPIKE/pkgconf-slateos.elf"
  echo "SLATE_PKGCONF_BUILT"
else
  echo "NO_SLATE_BINARY"
fi
