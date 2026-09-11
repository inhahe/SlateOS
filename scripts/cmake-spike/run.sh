#!/bin/bash
# Cross-compile upstream CMake and link it against SlateOS's own libc.a.
#
# This is the "try the port before you write a line" step from
# roadmap-detailed.md's "Porting vs. Reimplementing" policy, applied to the
# `cmake` quarter of the roadmap's `gcc, cmake, make, pkg-config (via POSIX
# layer)` item — the last quarter of it that nobody has measured. It is modelled
# on scripts/make-spike/run.sh, deliberately and down to the variable names, so
# that a reader who has read one does not have to re-learn the shape.
#
# WHAT IS DIFFERENT FROM THE OTHER FOUR SPIKES, AND IT IS THE WHOLE POINT
#
# bash, pkgconf, make and coreutils are C. CMake is C++, and this is the first
# time a real C++ program has been pointed at our libc. That matters because
# the question changes shape: a C program's undefined symbols are all libc's
# business, while a C++ program's are split between libc and the C++ runtime —
# `operator new`, the exception machinery, the typeinfo tables. Those come from
# zig's libc++/libc++abi/libunwind, which the link brings in beside our libc.a.
#
# So a missing symbol here is only interesting if it is a LIBC symbol. If the
# C++ runtime archives are left out, every one of libc++'s symbols is reported
# missing and the output reads exactly like "our libc is badly incomplete",
# which is why `slate_zig_cxx_runtime` refuses rather than returning an empty
# list when it cannot find them.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# It answers one question: does upstream CMake, unmodified, resolve every symbol
# it needs against `toolchain/sysroot/lib/libc.a` plus the C++ runtime? A link
# is a complete, mechanical enumeration of a program's demands on its libc,
# which is why it is worth doing before writing a reimplementation — and this
# tree already has a `userspace/cmake` reimplementation of roughly 190 lines
# whose relationship to the real thing nobody has measured.
#
# It does NOT answer whether cmake RUNS. CMake leans on the OS at least as hard
# as make does: it forks and waits, walks directory trees, stats and globs,
# opens pipes to compilers, and its own test suite compiles and executes
# programs. A clean link tells you nothing there is *missing*. Treat
# MISSING_COUNT=0 as permission to build a rootfs rung, not as a port.
#
# Run it from WSL.
set -uo pipefail
set -x

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="$SLATE_CMAKE_VERSION"
SYSROOT="$SLATE_SYSROOT"
# Durable, not /tmp — see worktree.sh ($SLATE_WORK).
WORK="$SLATE_WORK/cmake-spike"
SPIKE_LIBS="/tmp/slate-sysroot2-$SLATE_LANE"
JOBS="$(nproc 2>/dev/null || echo 4)"

slate_make_zig_wrappers || exit 1
slate_ensure_cmake_src || exit 1

# The host cmake that drives the cross build. CMake is the one port that needs
# its own kind to build itself: `./bootstrap` exists precisely because that is
# otherwise a chicken-and-egg problem, but bootstrap builds a HOST cmake, which
# is not what is being measured here.
HOST_CMAKE="$(command -v cmake)"
if [ -z "$HOST_CMAKE" ]; then
    echo "NO_HOST_CMAKE — install cmake in WSL; this spike cross-builds cmake with cmake."
    exit 1
fi
"$HOST_CMAKE" --version | head -1

mkdir -p "$WORK" && cd "$WORK" || exit 1
rm -rf "cmake-$VER" bld
tar xf "$SLATE_CMAKE_TARBALL" || exit 1

# CMAKE_SYSTEM_NAME is what puts cmake into cross mode; without it cmake probes
# the host and produces a host binary that would link against the host's glibc
# and prove nothing. CMAKE_SYSTEM_PROCESSOR is set too because leaving it unset
# in cross mode is how a build silently picks the host's word size.
#
# The bundled third-party libraries (zlib, zstd, expat, …) are built rather than
# taken from the system: a system copy would be the HOST's, and linking a host
# .a into a cross binary is the exact mistake `--without-guile` avoids in the
# make spike.
#
# BUILD_TESTING=OFF because the tests compile and RUN programs, which a cross
# build cannot do; CMAKE_USE_OPENSSL=OFF because we ship no OpenSSL and the
# question is about libc, not about TLS.
# THE FIND_ROOT_PATH BLOCK IS NOT BOILERPLATE, it is the first thing this spike
# found. Without it, cmake's bundled libarchive ran `find_path(ICONV iconv.h)`,
# found the HOST's `/usr/include`, and put `-I/usr/include` on the cross compile
# line. clang then read glibc's `stdlib.h` instead of zig's musl one and died on
# `bits/libc-header-start.h`, a glibc-internal header. The visible failure was
# "iconv is required, but was not found" — which reads like a missing libc
# feature and is in fact the host's headers leaking into a cross build.
#
# `NEVER` for PROGRAM because a host binary is never the right answer here;
# `ONLY` for the rest so nothing outside the root is considered.
#
# The root is a REAL SYSROOT and not an empty directory, which was the second
# thing this spike found. Pointed at nothing, cmake's `find_path(iconv.h)`
# failed, the iconv probe was skipped entirely rather than run and failed, and
# the error was the identical "iconv is required, but was not found" — the same
# sentence for the opposite cause. A cross build needs somewhere legitimate to
# look, not merely somewhere illegitimate to be kept away from.
#
# zig keeps musl's headers split in two: an architecture-specific directory and
# a generic one, searched in that order. The order is reproduced here by COPY
# ORDER plus `cp -n`, so that a header present in both resolves to the
# architecture's — flattening them the other way round would substitute
# `generic-musl`'s definitions of the types whose size depends on the
# architecture, and that is the kind of mistake that compiles.
ZINC="$(dirname "$SLATE_ZIG")/lib/libc/include"
FINDROOT="$WORK/sysroot"
rm -rf "$FINDROOT"
mkdir -p "$FINDROOT/include" "$FINDROOT/lib"
cp -rn "$ZINC/x86_64-linux-musl/." "$FINDROOT/include/" 2>/dev/null
cp -rn "$ZINC/generic-musl/." "$FINDROOT/include/" 2>/dev/null
cp "$SYSROOT/libc.a" "$FINDROOT/lib/" || exit 1
if [ ! -f "$FINDROOT/include/iconv.h" ]; then
    echo "NO_MUSL_HEADERS — $ZINC did not yield iconv.h, so the sysroot below is"
    echo "                  empty and every probe will fail for the wrong reason."
    exit 1
fi
echo "SYSROOT_HEADERS=$(find "$FINDROOT/include" -name '*.h' | wc -l)"

"$HOST_CMAKE" -S "cmake-$VER" -B bld \
    -DCMAKE_SYSTEM_NAME=Linux \
    -DCMAKE_SYSTEM_PROCESSOR=x86_64 \
    -DCMAKE_C_COMPILER="$SLATE_CC" \
    -DCMAKE_CXX_COMPILER="$SLATE_CXX" \
    -DCMAKE_FIND_ROOT_PATH="$FINDROOT" \
    -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER \
    -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY \
    -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY \
    -DCMAKE_FIND_ROOT_PATH_MODE_PACKAGE=ONLY \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_TESTING=OFF \
    -DCMAKE_USE_OPENSSL=OFF \
    -DBUILD_CursesDialog=OFF \
    -DBUILD_QtDialog=OFF \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -25 conf.log

"$HOST_CMAKE" --build bld -j "$JOBS" >build.log 2>&1
echo "BUILD_EXIT=$?"
grep -iE "^.*error" build.log | head -20

# The decisive step. -nostdlib so we get SlateOS's libc, not zig's bundled musl.
# libc.a twice: it is Rust-built and its intra-archive references are not
# topologically ordered, so a second pass is cheaper than --start-group.
# libstubs.a is deliberately not linked — it and libc.a each carry a panic
# handler and collide on __rustc::rust_begin_unwind.
mkdir -p "$SPIKE_LIBS"
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1

CXXRT="$(slate_zig_cxx_runtime)" || exit 1
echo "CXX_RUNTIME_ARCHIVES:"
printf '%s\n' "$CXXRT"

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
if [ -z "$OBJS" ]; then
    echo "NO_OBJECTS — the build above failed; see $WORK/build.log"
    exit 1
fi

# Every static archive the build produced, which is where CMakeLib and the
# bundled third-party libraries live. Ordered by the linker's needs is not
# something we can know here, so the whole set is passed twice for the same
# reason libc.a is.
LIBS="$(find bld -name '*.a' | sort | tr '\n' ' ')"
echo "ARCHIVE_COUNT=$(echo "$LIBS" | wc -w)"

# shellcheck disable=SC2086  # word splitting is what builds the object list
"$SLATE_CXX" -static -nostdlib -o cmake-slateos $OBJS $LIBS $LIBS $CXXRT \
    "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libunwind.a" \
    2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

MISSING="/tmp/cmake_missing-$SLATE_LANE.txt"
grep -oP "undefined symbol: \K.*" slate-link.log | sort -u >"$MISSING"
echo "MISSING_COUNT=$(wc -l <"$MISSING")"
head -40 "$MISSING"

# Counted SEPARATELY, and MISSING_COUNT is never allowed to stand in for "the
# link succeeded". The make spike's first run printed SLATE_LINK_EXIT=1 beside
# MISSING_COUNT=0 and was briefly read as a fluke, because the only number on
# screen was the one saying everything was fine: all eleven errors were
# *duplicate* definitions, which the grep above does not match. The two counts
# measure opposite failures — nothing is absent vs. something is present twice
# — and a libc can fail either way, so both print unconditionally.
DUPES="/tmp/cmake_dupes-$SLATE_LANE.txt"
grep -oP "duplicate symbol: \K.*" slate-link.log | sort -u >"$DUPES"
echo "DUPLICATE_COUNT=$(wc -l <"$DUPES")"
head -40 "$DUPES"

grep -v "undefined symbol\|duplicate symbol\|^>>>" slate-link.log | head -20

if [ -x cmake-slateos ]; then
    file cmake-slateos
    readelf -h cmake-slateos | grep -E "Type|Entry"
    echo "UNSTRIPPED_BYTES=$(stat -c %s cmake-slateos)"

    # Stage the STRIPPED binary. Unstripped this is 274 MB of mostly
    # `debug_info`, against a 384 MB image — it would not fit beside the rest
    # of the rootfs, and an artifact that cannot be staged is not a port.
    # `--strip-debug` and not `--strip-all`, matching how CPython is staged
    # (design-decisions.md §344): the symbol table costs 7 MB and is what makes
    # a fault address on the serial console into a function name.
    cp cmake-slateos "$SLATE_SPIKE/cmake-slateos.elf"
    # binutils `strip` first, because it edits in place. `zig objcopy` needs an
    # explicit output path and fails with "expected output parameter" if given
    # one argument — which it did, harmlessly, while the fallback did the work
    # and left a confusing error above a correct result.
    strip --strip-debug "$SLATE_SPIKE/cmake-slateos.elf" 2>/dev/null \
        || "$SLATE_ZIG" objcopy --strip-debug cmake-slateos \
            "$SLATE_SPIKE/cmake-slateos.elf" \
        || echo "STRIP_FAILED — the staged binary is the unstripped one"
    echo "STAGED_BYTES=$(stat -c %s "$SLATE_SPIKE/cmake-slateos.elf")"
    ls -l "$SLATE_SPIKE/cmake-slateos.elf"
    echo "SLATE_CMAKE_BUILT"
else
    echo "NO_SLATE_BINARY"
fi
