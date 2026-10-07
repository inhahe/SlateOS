#!/bin/bash
# Build GCC 16.2 as a cross compiler for SlateOS, with libgcc and libstdc++ built for SlateOS.
#
# WHY. scripts/gcc-spike/run.sh measures whether GCC's own programs link
# against our libc.a: the compiler that would run ON SlateOS. A compiler there
# also needs what it links into every program it makes: libgcc -- crtbegin.o
# and crtend.o, the helpers for what x86-64 has no instruction for, and the
# unwinder C++ exceptions need -- and libstdc++, the C++ library. Those are
# built by a compiler that makes SlateOS code and runs where the build runs:
# this one. They are the same files whichever machine the compiler using them
# runs on.
#
# It is also, by itself, a second compiler for SlateOS beside zig's:
# x86_64-linux-musl-gcc and -g++ in $WORK/prefix/bin make native SlateOS
# programs on Linux.
#
# THE SYSROOT -- what this compiler takes for SlateOS's / -- is $WORK/sysroot:
#   usr/include   musl's headers and Linux's, as zig orders them for
#                 x86_64-linux-musl -- x86_64-linux-musl/, generic-musl/,
#                 x86-linux-any/, any-linux-any/ -- merged here lowest first,
#                 so that where two hold one name, the one zig finds first is
#                 the one kept. They are what every C program here is
#                 compiled against today.
#   usr/lib       our libc.a; empty libm.a, libpthread.a, libdl.a, librt.a,
#                 libutil.a, libcrypt.a, libresolv.a and libxnet.a, as musl
#                 installs them (it is all in libc.a, and a link naming -lm
#                 must still find a library); and empty crt1.o, crti.o and
#                 crtn.o: our libc.a carries _start, which the linker takes
#                 as its entry (GNU ld and lld alike), and runs .init_array
#                 itself, so nothing goes in .init -- but GCC's driver names
#                 the three in every link, so they must exist.
# and our overlay, posix/include, in GCC's tool include directory,
# $WORK/prefix/x86_64-linux-musl/include. For a musl target GCC searches C++'s
# headers, then that directory, then the sysroot's usr/include, and its own
# headers last (gcc/config/linux.h's musl INCLUDE_DEFAULTS), so the overlay
# sits immediately before musl's headers, where its #include_next needs them
# to be.
#
# THE ASSEMBLER AND LINKER: binutils 2.47, the pin scripts/binutils-spike/
# links for SlateOS, built here for Linux with the same target.
#
# THE COMPILER -- what is set, each for a reason:
#   --with-specs=... -static unless told otherwise: there is no dynamic loader
#       for native programs, only libc.a.
#   --disable-fixincludes: fixincludes copies the system headers it thinks
#       need fixing into include-fixed/, which for a musl target is not on
#       the search path at all; the copies would be made and never read.
#   --disable-shared: the target libraries as archives only.
#   --disable-lto, --disable-plugin: shared objects a static toolchain here
#       cannot load (scripts/gcc-spike/run.sh says more).
#   --disable-libsanitizer, -libgomp, -libitm, -libquadmath, -libssp,
#       -libvtv, -libatomic: each a library of its own to port; libgcc and
#       libstdc++ are what C and C++ need.
#   --enable-languages=c,c++, --disable-multilib, --disable-nls, --without-isl:
#       as in run.sh.
#
# Run it from WSL. The work tree is durable ($SLATE_WORK, not /tmp).
set -uo pipefail
set -x

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1
. "$HERE/../lib/worktree.sh" || exit 1

TARGET=x86_64-linux-musl
WORK="$SLATE_WORK/gcc-cross"
SYSROOT="$WORK/sysroot"
PREFIX="$WORK/prefix"
JOBS="${SLATE_SPIKE_JOBS:-8}"
VER="$SLATE_GCC_VERSION"
BVER="$SLATE_BINUTILS_VERSION"

# GMP preprocesses its assembly with m4; scripts/gcc-spike/run.sh says how
# this fetches it without root.
ensure_host_m4() {
    command -v m4 >/dev/null && return 0
    local dir="$SLATE_ZIG_CACHE/host-tools/m4" tmp
    if [ ! -x "$dir/usr/bin/m4" ]; then
        tmp="$(mktemp -d "$SLATE_ZIG_CACHE/m4dl.XXXXXX")" || return 1
        if ! (cd "$tmp" && apt-get download m4) || ! mkdir -p "$dir" \
            || ! dpkg -x "$tmp"/m4_*.deb "$dir"; then
            rm -rf "$tmp"
            echo "gcc-cross: could not fetch Ubuntu's m4 package; GMP needs m4" >&2
            return 1
        fi
        rm -rf "$tmp"
    fi
    export PATH="$dir/usr/bin:$PATH"
}

slate_ensure_zig || exit 1
ensure_host_m4 || exit 1
slate_ensure_gcc_src || exit 1
slate_ensure_gmp_src || exit 1
slate_ensure_mpfr_src || exit 1
slate_ensure_mpc_src || exit 1
slate_ensure_binutils_src || exit 1

rm -rf "$WORK"
mkdir -p "$WORK" "$SYSROOT/usr/include" "$SYSROOT/usr/lib" "$PREFIX/$TARGET/include" \
    && cd "$WORK" || exit 1

# --- the sysroot ------------------------------------------------------------
ZIG_INC="$(dirname "$SLATE_ZIG")/lib/libc/include"
for d in any-linux-any x86-linux-any generic-musl x86_64-linux-musl; do
    if [ ! -d "$ZIG_INC/$d" ]; then
        echo "NO_HEADERS -- zig has no $ZIG_INC/$d"
        exit 1
    fi
    cp -R "$ZIG_INC/$d/." "$SYSROOT/usr/include/" || exit 1
done
cp -R "$SLATE_ROOT/posix/include/." "$PREFIX/$TARGET/include/" || exit 1
cp "$SLATE_SYSROOT/libc.a" "$SYSROOT/usr/lib/libc.a" || exit 1
printf '' >"$WORK/empty.c"
gcc -c -o "$WORK/empty.o" "$WORK/empty.c" || exit 1
for lib in m pthread dl rt util crypt resolv xnet; do
    ar rc "$SYSROOT/usr/lib/lib$lib.a" || exit 1
done
for crt in crt1 crti crtn; do
    cp "$WORK/empty.o" "$SYSROOT/usr/lib/$crt.o" || exit 1
done

# --- binutils, for Linux, making SlateOS objects ----------------------------
tar xf "$SLATE_BINUTILS_TARBALL" || exit 1
mkdir build-binutils && cd build-binutils || exit 1
"../binutils-$BVER/configure" --target="$TARGET" --prefix="$PREFIX" \
    --with-sysroot="$SYSROOT" --disable-nls --disable-werror --disable-plugins \
    --disable-gprofng --without-debuginfod --without-zstd --without-msgpack \
    --enable-deterministic-archives >conf.log 2>&1
echo "BINUTILS_CONFIGURE_EXIT=$?"
make -j"$JOBS" all-binutils all-gas all-ld >make.log 2>&1
echo "BINUTILS_MAKE_EXIT=$?"
make install-binutils install-gas install-ld >install.log 2>&1
echo "BINUTILS_INSTALL_EXIT=$?"
cd "$WORK" || exit 1
if [ ! -x "$PREFIX/bin/$TARGET-as" ] || [ ! -x "$PREFIX/bin/$TARGET-ld" ]; then
    echo "NO_CROSS_BINUTILS -- see $WORK/build-binutils"
    exit 1
fi
export PATH="$PREFIX/bin:$PATH"

# --- GCC, for Linux, making SlateOS programs --------------------------------
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
mkdir build-gcc && cd build-gcc || exit 1
"../gcc-$VER/configure" --target="$TARGET" --prefix="$PREFIX" \
    --with-sysroot="$SYSROOT" \
    '--with-specs=%{!shared:%{!static-pie:%{!static:-static}}}' \
    --disable-fixincludes --disable-shared --disable-multilib --disable-nls \
    --disable-werror --enable-languages=c,c++ --disable-plugin --disable-lto \
    --without-isl --disable-libsanitizer --disable-libgomp --disable-libitm \
    --disable-libquadmath --disable-libssp --disable-libvtv --disable-libatomic \
    >conf.log 2>&1
echo "GCC_CONFIGURE_EXIT=$?"
make -j"$JOBS" all-gcc >make-gcc.log 2>&1
echo "GCC_MAKE_EXIT=$?"
make install-gcc >install-gcc.log 2>&1
echo "GCC_INSTALL_EXIT=$?"
make -j"$JOBS" all-target-libgcc >make-libgcc.log 2>&1
echo "LIBGCC_MAKE_EXIT=$?"
make install-target-libgcc >install-libgcc.log 2>&1
echo "LIBGCC_INSTALL_EXIT=$?"
make -j"$JOBS" all-target-libstdc++-v3 >make-libstdcxx.log 2>&1
echo "LIBSTDCXX_MAKE_EXIT=$?"
make install-target-libstdc++-v3 >install-libstdcxx.log 2>&1
echo "LIBSTDCXX_INSTALL_EXIT=$?"
cd "$WORK" || exit 1

# The decisive step: a C program and a C++ one, compiled and linked by this
# compiler, with their counts and checks (cross-check.sh, run alone after a
# libc.a rebuild, as slatelink.sh is for the other ports).
exec bash "$HERE/cross-check.sh"
