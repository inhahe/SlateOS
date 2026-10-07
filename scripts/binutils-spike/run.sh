#!/bin/bash
# Cross-compile GNU binutils (as, ld, ar, nm, objdump ...) and link them against SlateOS's libc.a.
#
# The "try the port before you write a line" step from roadmap-detailed.md's
# "Porting vs. Reimplementing" policy. A compiler on SlateOS needs an
# assembler there: GCC writes assembly and hands it to `as`, and so does
# clang when told -no-integrated-as. LLVM's linker, ld.lld, is on the image
# already (scripts/llvm-spike/); no assembler is. binutils is the assembler
# GCC was written against, with GNU ld beside it and the tools that read and
# edit object files -- ar, nm, objdump, objcopy, readelf, strip, size,
# strings, addr2line, c++filt, elfedit. Shaped like scripts/gdb-spike/run.sh,
# whose tree shares binutils' libraries (bfd, opcodes, libiberty), down to
# the names of the numbers it prints.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# One question: does binutils 2.47, unmodified, resolve every symbol its
# programs need against `toolchain/sysroot/lib/libc.a`?
#
# Not whether they work on SlateOS: that is a boot's question, once they are
# on the image.
#
# Run it from WSL. The work tree is durable ($SLATE_WORK, not /tmp, which WSL
# empties when the distribution idles), and the libraries are copied off the
# /mnt mount first because 9p is slow and a linker reads an archive many
# times.
set -uo pipefail
set -x

# Absolute, before any cd: slatelink.sh is run from here at the end, after
# the build has moved into the work tree.
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1
. "$HERE/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/binutils-spike"
SPIKE_LIBS="$WORK/libs"
JOBS="${SLATE_SPIKE_JOBS:-8}"
VER="$SLATE_BINUTILS_VERSION"

slate_make_zig_wrappers || exit 1
slate_ensure_binutils_src || exit 1

mkdir -p "$WORK" "$SPIKE_LIBS" && cd "$WORK" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
# Every compile and every link goes through the wrappers, so each configure
# test is answered by our libc.a.
export CC="$SLATE_LINK_CC" CXX="$SLATE_LINK_CXX" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"
# The few programs the build runs on this machine (bfd's and opcodes'
# generators) are made with the host's own compiler.
export CC_FOR_BUILD=gcc CXX_FOR_BUILD=g++

rm -rf "binutils-$VER" build
tar xf "$SLATE_BINUTILS_TARBALL" || exit 1
mkdir build && cd build || exit 1
# --build, --host and --target: the programs run on SlateOS and work on its
# objects, and are built here, so configure runs nothing they are linked into.
# The target is x86_64-linux-musl because SlateOS's programs are ELF x86-64
# objects of that ABI with a note of their own (kernel/src/proc/elf.rs).
#
# What is left out, each for a reason:
#   --disable-plugins: ld, ar and nm load the LTO plugin, a shared object,
#       with dlopen; a static program here has nothing to load.
#   --disable-gprofng: a profiler in the same tarball that needs a collector
#       library preloaded into the program it profiles; not binutils proper.
#   --without-debuginfod, --without-zstd, --without-msgpack: optional
#       libraries, each a port of its own; configure must not find the host's
#       copy and link a Linux library into a SlateOS program. zlib is the
#       tree's own copy, as the default has it.
#   --disable-nls: message catalogues; the C locale is the only one we have.
#   --enable-deterministic-archives: ar writes no timestamps or owners unless
#       asked (`U`), as distributions configure it.
"../binutils-$VER/configure" \
    --build=x86_64-pc-linux-gnu --host=x86_64-linux-musl --target=x86_64-linux-musl \
    --prefix=/usr --disable-shared --enable-static --disable-nls --disable-werror \
    --disable-plugins --disable-gprofng --without-debuginfod --without-zstd \
    --without-msgpack --enable-deterministic-archives \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 conf.log

# -k: one failing object must not hide the rest.
make -k -j"$JOBS" all-binutils all-gas all-ld >make.log 2>&1
echo "MAKE_EXIT=$?"
grep -E ' error: |Error [0-9]' make.log | grep -v 'undefined symbol' | head -30

# The decisive step, each program's link against our libc.a on its own so
# its counts can be read, and the stripped copies: the same step a rootfs
# would run alone when libc.a has moved on since.
exec bash "$HERE/slatelink.sh"
