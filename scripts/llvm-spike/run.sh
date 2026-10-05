#!/bin/bash
# Cross-compile LLVM 20's `opt`, `llc` and `ld.lld` for SlateOS, linked against
# SlateOS's own libc.a.
#
# WHY. The fastpy compiler on SlateOS (design-decisions §1050, the operator's
# answer to B-Q18) builds LLVM IR and needs LLVM's optimizer and code generator
# to make a program of it. fastpy reaches LLVM through llvmlite, which loads a
# shared library with ctypes -- and SlateOS has no dynamic linker. So fastpy
# writes IR text and runs these three programs instead, exactly as it does
# when it cross-compiles from Windows today
# (requests/b-d-fastpy-on-slateos-needs-llvm-tools.md):
#
#   opt -passes='default<O2>' -inline-threshold=225 ...
#   llc -O2 -filetype=obj -mtriple=x86_64-unknown-linux-musl -mcpu=x86-64 ...
#   ld.lld -static --no-dynamic-linker -e _start -o OUT ... -L/usr/lib/x86_64-slateos -lc
#
# They are also the first step of the Rust toolchain port: rustc is an LLVM
# front end.
#
# WHY LLVM 20. It is the LLVM llvmlite 0.47 -- fastpy's on Windows and WSL --
# carries, so SlateOS gets the optimizer every other platform uses. fastpy
# prints typed-pointer IR (`i8*`), which LLVM 17 and later still read: the
# parser upgrades it to opaque pointers (measured by lane B with LLVM 18).
#
# HOW IT DIFFERS FROM THE CMAKE PORT. scripts/cmake-spike/run.sh builds
# against zig's musl and relinks the result against our libc.a afterwards.
# Here every link the build makes is against our libc.a and nothing else,
# from configure's first check to the tools themselves: cmake's compilers are
# scripts/lib/worktree.sh's link wrappers, which compile with zig's cc and
# c++ and link with zig's ld.lld itself -- our libc.a, with zig's C++ runtime
# ahead of it and zig's compiler runtime behind, in zig's own order, and none
# of the -lm, -ldl, -lrt and -lpthread zig would answer with musl (step 2).
# So whatever a check links answers for THIS libc. What a header decides is
# musl's, since the build compiles against musl's headers as every port does
# (design-decisions §1011):
# `pthread_getname_np` and `mallinfo`, which our libc has and those headers do
# not declare to LLVM's checks, read as absent, and LLVM does without them.
#
# Measured, the first way failed three times over (2026-10-01). The libraries
# were cmake's CMAKE_<LANG>_STANDARD_LIBRARIES, a string cmake puts on the
# link line unquoted, so the worktree's path ("visual studio projects")
# reached the linker as three words and nothing linked. cmake does not pass
# that variable into its checks at all, which ran `zig cc -static -nostdlib
# ... -lm`: musl wherever a check named a library, and no C library at all
# where it named none, so every HAVE_* the cache held was musl's or a false
# "no". And zig's cc driver puts its own musl libc.a behind every link,
# -nostdlib or not, where it would have supplied whatever our libc lacks
# (known-issues D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC).
#
# THE STEPS
#   1. llvm-tblgen and llvm-min-tblgen for the HOST: LLVM generates code with
#      them while it builds, so they must run where the build does.
#   2. The cross build: opt, llc and lld, X86 only, static, release; no zlib,
#      zstd, libxml2 or libedit, no threads (opt, llc and a static link do not
#      need them, and fewer moving parts in the first port).
#   3. slatelink.sh: link what is missing or older than libc.a, strip debug
#      info but keep the symbols (a fault address on the console becomes a
#      function name, as for CPython, design-decisions §344), and stage them
#      into $SLATE_SPIKE for the rootfs recipe. The recipe runs slatelink.sh
#      by itself whenever libc.a moves on, which is a minute, not a build.
#
# Run it from WSL. The work tree is several GB and lives in $SLATE_WORK, not
# in the repository.
set -uo pipefail
set -x

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="$SLATE_LLVM_VERSION"
SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/llvm-spike"
# SLATE_JOBS lowers it, for a build run beside a boot test.
JOBS="${SLATE_JOBS:-$(nproc 2>/dev/null || echo 4)}"

slate_make_zig_wrappers || exit 1
slate_ensure_llvm_src || exit 1

HOST_CMAKE="$(command -v cmake)"
if [ -z "$HOST_CMAKE" ]; then
    echo "NO_HOST_CMAKE -- install cmake in WSL; LLVM builds with cmake."
    exit 1
fi
"$HOST_CMAKE" --version | head -1

mkdir -p "$WORK" && cd "$WORK" || exit 1
SRC="llvm-project-$VER.src"
# Only what opt, llc and lld are built from: the full tree is 2 GB unpacked.
# lld's Mach-O half includes libunwind's compact-unwind header, so that one
# directory comes too.
if [ ! -f "$SRC/.extracted" ]; then
    rm -rf "$SRC"
    tar xf "$SLATE_LLVM_TARBALL" "$SRC/llvm" "$SRC/lld" "$SRC/cmake" \
        "$SRC/third-party" "$SRC/libunwind/include" || exit 1
    touch "$SRC/.extracted"
fi

# --- 1. the host table generators ---------------------------------------------
HOSTBIN="$WORK/host/bin"
if [ ! -x "$HOSTBIN/llvm-tblgen" ] || [ ! -x "$HOSTBIN/llvm-min-tblgen" ]; then
    "$HOST_CMAKE" -S "$SRC/llvm" -B host \
        -DCMAKE_BUILD_TYPE=Release \
        -DLLVM_TARGETS_TO_BUILD=X86 \
        -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF \
        -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_DOCS=OFF \
        -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_LIBXML2=OFF \
        >host-conf.log 2>&1
    echo "HOST_CONFIGURE_EXIT=$?"
    "$HOST_CMAKE" --build host -j "$JOBS" --target llvm-tblgen llvm-min-tblgen \
        >host-build.log 2>&1
    echo "HOST_BUILD_EXIT=$?"
fi
if [ ! -x "$HOSTBIN/llvm-tblgen" ] || [ ! -x "$HOSTBIN/llvm-min-tblgen" ]; then
    echo "NO_HOST_TBLGEN -- see $WORK/host-build.log"
    exit 1
fi

# --- 2. the cross build -------------------------------------------------------
# The find root: musl's headers as zig orders them, and our libc.a -- the same
# real sysroot scripts/cmake-spike/run.sh explains the need for.
ZINC="$(dirname "$SLATE_ZIG")/lib/libc/include"
FINDROOT="$WORK/sysroot"
rm -rf "$FINDROOT"
mkdir -p "$FINDROOT/include" "$FINDROOT/lib"
cp -rn "$ZINC/x86_64-linux-musl/." "$FINDROOT/include/" 2>/dev/null
cp -rn "$ZINC/generic-musl/." "$FINDROOT/include/" 2>/dev/null
cp "$SYSROOT/libc.a" "$FINDROOT/lib/" || exit 1

# The compilers cmake is given (see the header): zig's to compile, and zig's
# ld.lld to link -- against our libc, with zig's C++ runtime ahead of it and
# zig's compiler runtime behind it. scripts/lib/worktree.sh's
# slate_make_link_wrappers writes them, and says why a link cannot go through
# zig's cc driver and why the order of those three matters.
WRAP="$WORK/bin"
slate_make_link_wrappers "$WRAP" || exit 1
cat "$SLATE_LINK_CXX"

# A build configured the first way holds musl's answers in its cache and the
# other compiler in its rules: it cannot be reused.
if [ -f bld/CMakeCache.txt ] && ! grep -q "^CMAKE_CXX_COMPILER:.*=$SLATE_LINK_CXX\$" bld/CMakeCache.txt; then
    echo "OLD_BUILD_DIR -- configured with another compiler; starting it again"
    rm -rf bld
fi

"$HOST_CMAKE" -S "$SRC/llvm" -B bld \
    -DCMAKE_SYSTEM_NAME=Linux \
    -DCMAKE_SYSTEM_PROCESSOR=x86_64 \
    -DCMAKE_C_COMPILER="$SLATE_LINK_CC" \
    -DCMAKE_CXX_COMPILER="$SLATE_LINK_CXX" \
    -DCMAKE_AR="$SLATE_AR" \
    -DCMAKE_RANLIB="$SLATE_RANLIB" \
    -DCMAKE_FIND_ROOT_PATH="$FINDROOT" \
    -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER \
    -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY \
    -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY \
    -DCMAKE_FIND_ROOT_PATH_MODE_PACKAGE=ONLY \
    -DCMAKE_BUILD_TYPE=Release \
    -DLLVM_ENABLE_PROJECTS=lld \
    -DLLVM_TARGETS_TO_BUILD=X86 \
    -DLLVM_HOST_TRIPLE=x86_64-unknown-linux-musl \
    -DLLVM_DEFAULT_TARGET_TRIPLE=x86_64-unknown-linux-musl \
    -DLLVM_NATIVE_TOOL_DIR="$HOSTBIN" \
    -DLLVM_TABLEGEN="$HOSTBIN/llvm-tblgen" \
    -DLLVM_ENABLE_THREADS=OFF \
    -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_LIBXML2=OFF \
    -DLLVM_ENABLE_LIBEDIT=OFF -DLLVM_ENABLE_LIBPFM=OFF \
    -DLLVM_ENABLE_CURL=OFF -DLLVM_ENABLE_HTTPLIB=OFF \
    -DLLVM_ENABLE_BACKTRACES=OFF -DLLVM_ENABLE_CRASH_OVERRIDES=OFF \
    -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF \
    -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_DOCS=OFF \
    -DLLVM_BUILD_LLVM_DYLIB=OFF -DLLVM_LINK_LLVM_DYLIB=OFF \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -25 conf.log

"$HOST_CMAKE" --build bld -j "$JOBS" --target opt llc lld >build.log 2>&1
echo "BUILD_EXIT=$?"
grep -iE "error|undefined symbol|duplicate symbol" build.log | head -30

# --- 3. link and stage --------------------------------------------------------
# slatelink.sh's: it is also what the rootfs recipe runs to relink the tools
# against a newer libc.a, so the two cannot drift apart. Tools the build above
# just linked are newer than libc.a and are staged as they are; a tool whose
# link failed is missing, and slatelink.sh reports why.
exec bash "$(dirname "${BASH_SOURCE[0]}")/slatelink.sh"
