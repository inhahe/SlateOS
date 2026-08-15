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

VER=2.3.0
SYSROOT="/mnt/d/visual studio projects/os/toolchain/sysroot/lib"
WORK=/tmp/pkgconf-spike
SPIKE_LIBS=/tmp/slate-sysroot2

# zig cc as the cross compiler, exactly as the bash spike and fastpy's own
# toolchain.py do. SlateOS's libc.a is built to match the musl ABI, so
# compiling against zig's musl headers and then linking against our archive is
# legitimate rather than a fudge.
#
# TRAP: $CC must not contain spaces. autotools word-splits it, and this repo
# lives under "D:\visual studio projects\", so pointing CC straight at a path
# under /mnt/d produces a thoroughly misleading "C compiler cannot create
# executables". The /tmp/zigcc wrapper exists to keep the space out of $CC.
cat >/tmp/zigcc <<'EOF'
#!/bin/sh
exec "/mnt/d/visual studio projects/os/build/spike/zig/zig" cc --target=x86_64-linux-musl "$@"
EOF
cat >/tmp/zigar <<'EOF'
#!/bin/sh
exec "/mnt/d/visual studio projects/os/build/spike/zig/zig" ar "$@"
EOF
cat >/tmp/zigranlib <<'EOF'
#!/bin/sh
exec "/mnt/d/visual studio projects/os/build/spike/zig/zig" ranlib "$@"
EOF
chmod +x /tmp/zigcc /tmp/zigar /tmp/zigranlib

mkdir -p "$WORK" && cd "$WORK"
[ -f "pkgconf-$VER.tar.xz" ] || curl -sSLO "https://distfiles.ariadne.space/pkgconf/pkgconf-$VER.tar.xz"
[ -d "pkgconf-$VER" ] || tar xf "pkgconf-$VER.tar.xz"
cd "pkgconf-$VER"

export CC=/tmp/zigcc AR=/tmp/zigar RANLIB=/tmp/zigranlib

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
/tmp/zigcc -static -nostdlib -o pkgconf-slateos $OBJS .libs/libpkgconf.a \
    "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libunwind.a" \
    2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

grep -oP "undefined symbol: \K.*" slate-link.log | sort -u >/tmp/pkgconf_missing.txt || true
echo "MISSING_COUNT=$(wc -l </tmp/pkgconf_missing.txt)"
cat /tmp/pkgconf_missing.txt
grep -v "undefined symbol\|^>>>" slate-link.log | head -20 || true

if [ -x pkgconf-slateos ]; then
  file pkgconf-slateos
  readelf -h pkgconf-slateos | grep -E "Type|Entry"
  echo "SLATE_PKGCONF_BUILT"
else
  echo "NO_SLATE_BINARY"
fi
