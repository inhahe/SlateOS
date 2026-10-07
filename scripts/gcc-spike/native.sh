#!/bin/bash
# Build GCC 16.2 to run ON SlateOS, with libgcc and libstdc++; slatelink.sh then makes the image's tree.
#
# scripts/gcc-spike/run.sh showed that GCC's programs link against our libc.a
# (through zig, with nothing missing). This is the build that goes on the
# image: GCC compiled by the cross GCC scripts/gcc-spike/cross.sh builds -- a
# compiler built for SlateOS by GCC, the arrangement GCC's own documentation
# calls a canadian cross, with build, host and target all named. The cross
# compiler also builds the target libraries here, libgcc and libstdc++, in
# the layout a compiler running on SlateOS looks for them in.
#
# Why GCC builds it, not zig, as the link spike did: GCC's build compiles
# some .c sources with $(CXX) and expects them compiled as C++, which g++
# does and zig's c++ does not (run.sh's README); and its `specs` step runs
# the target compiler, which must be one that runs here. The cross GCC is
# both. It links against the same libc.a, through GNU ld 2.47.
#
# This script configures and compiles -- the hour -- and then runs
# slatelink.sh, the minutes: each program linked again against the current
# libc.a, the install, and the tree for the image at $SLATE_SPIKE/gcc, which
# slatelink.sh's comment describes. The rootfs recipe runs slatelink.sh
# alone when libc.a has moved on, as it does every port's.
#
# The configuration is cross.sh's (its comment says why each choice), with
# --with-build-sysroot naming cross.sh's sysroot: the headers this build
# reads, so that GCC's configure checks of the C library are answered by its
# headers, while the compiler's runtime sysroot stays the system's /.
#
# Run it from WSL, after cross.sh. The work tree is durable ($SLATE_WORK).
set -uo pipefail
set -x

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1
. "$HERE/../lib/worktree.sh" || exit 1

TARGET=x86_64-linux-musl
CROSS="$SLATE_WORK/gcc-cross"
WORK="$SLATE_WORK/gcc-native"
JOBS="${SLATE_SPIKE_JOBS:-8}"
VER="$SLATE_GCC_VERSION"

if [ ! -x "$CROSS/prefix/bin/$TARGET-gcc" ] || [ ! -x "$CROSS/prefix/bin/$TARGET-g++" ]; then
    echo "NO_CROSS_COMPILER -- run scripts/gcc-spike/cross.sh first"
    exit 1
fi
export PATH="$CROSS/prefix/bin:$PATH"
# The cross sysroot's libc.a, brought up to date: what every program this
# build links -- GCC's own, and configure's tests -- is linked against.
cp "$SLATE_SYSROOT/libc.a" "$CROSS/sysroot/usr/lib/libc.a" || exit 1

slate_ensure_gcc_src || exit 1
slate_ensure_gmp_src || exit 1
slate_ensure_mpfr_src || exit 1
slate_ensure_mpc_src || exit 1
# GMP's m4, as cross.sh fetches it.
if ! command -v m4 >/dev/null && [ -x "$SLATE_ZIG_CACHE/host-tools/m4/usr/bin/m4" ]; then
    export PATH="$SLATE_ZIG_CACHE/host-tools/m4/usr/bin:$PATH"
fi

rm -rf "$WORK"
mkdir -p "$WORK" && cd "$WORK" || exit 1
tar xf "$SLATE_GCC_TARBALL" || exit 1
for lib in "gmp:$SLATE_GMP_TARBALL:gmp-$SLATE_GMP_VERSION" \
           "mpfr:$SLATE_MPFR_TARBALL:mpfr-$SLATE_MPFR_VERSION" \
           "mpc:$SLATE_MPC_TARBALL:mpc-$SLATE_MPC_VERSION"; do
    name="${lib%%:*}"
    rest="${lib#*:}"
    tarball="${rest%%:*}"
    dir="${rest#*:}"
    tar xf "$tarball" -C "gcc-$VER" || exit 1
    mv "gcc-$VER/$dir" "gcc-$VER/$name" || exit 1
done

mkdir build && cd build || exit 1
# The compilers that build it: the cross GCC for what runs on SlateOS (the
# host here) and for the target libraries, the build machine's own gcc for
# the generator programs that run during the build.
export CC="$TARGET-gcc" CXX="$TARGET-g++" AR="$TARGET-ar" RANLIB="$TARGET-ranlib"
export CC_FOR_BUILD=gcc CXX_FOR_BUILD=g++
"../gcc-$VER/configure" \
    --build=x86_64-pc-linux-gnu --host="$TARGET" --target="$TARGET" \
    --prefix=/usr --with-build-sysroot="$CROSS/sysroot" \
    '--with-specs=%{!shared:%{!static-pie:%{!static:-static}}}' \
    --disable-fixincludes --disable-shared --disable-bootstrap --disable-multilib \
    --disable-nls --disable-werror --enable-languages=c,c++ --disable-plugin \
    --disable-lto --without-isl --disable-libsanitizer --disable-libgomp \
    --disable-libitm --disable-libquadmath --disable-libssp --disable-libvtv \
    --disable-libatomic \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 conf.log

slate_resume_once_if_killed make-gcc.log make -j"$JOBS" all-gcc
echo "GCC_MAKE_EXIT=$?"
slate_resume_once_if_killed make-libgcc.log make -j"$JOBS" all-target-libgcc
echo "LIBGCC_MAKE_EXIT=$?"
slate_resume_once_if_killed make-libstdcxx.log make -j"$JOBS" all-target-libstdc++-v3
echo "LIBSTDCXX_MAKE_EXIT=$?"
for log in make-gcc make-libgcc make-libstdcxx; do
    grep -E ' error: |Error [0-9]' "$log.log" | head -10
done

# The rest -- installing, assembling the tree for the image, and checking
# it -- is the step the rootfs recipe runs alone when libc.a has moved on
# since, as every port's slatelink.sh is.
exec bash "$HERE/slatelink.sh"
