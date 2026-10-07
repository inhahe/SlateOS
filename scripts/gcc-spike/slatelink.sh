#!/bin/bash
# Relink the GCC native.sh built against the CURRENT libc.a, install it, and make the image's tree.
#
# native.sh configures and compiles GCC 16.2 to run on SlateOS -- the hour --
# and then runs this, the minutes. It is also the step the rootfs recipe runs
# alone when libc.a has moved on since (spike_rebuild_if_behind), as every
# port's slatelink.sh is: GCC's programs are linked against libc.a, while
# libgcc and libstdc++ are archives compiled against its headers, which a
# libc.a rebuild leaves as they were.
#
# THE TREE, $SLATE_SPIKE/gcc, rooted at the image's / (--prefix=/usr):
#   usr/bin/gcc, g++, c++, cpp, gcov ...        the drivers and tools
#   usr/libexec/gcc/x86_64-linux-musl/16.2.0/   cc1, cc1plus, collect2
#   usr/lib/gcc/x86_64-linux-musl/16.2.0/       crtbegin*.o, crtend*.o,
#                                               libgcc.a -- with the unwinder
#                                               in it: a GCC built without
#                                               shared libraries makes no
#                                               libgcc_eh.a, and its driver
#                                               names none -- and GCC's own
#                                               headers
#   usr/include/c++/16.2.0/                     libstdc++'s headers
#   usr/lib or usr/lib64                        libstdc++.a, libsupc++.a
#   usr/include                                 musl's headers and Linux's,
#                                               as cross.sh merged them
#   usr/lib/crt1.o, crti.o, crtn.o, libm.a ...  cross.sh's empty objects and
#                                               archives (its comment says
#                                               why each is empty)
# with the programs' debugging information stripped -- the symbol tables
# stay, as every port here is staged. What changes with the tree rather than
# with GCC, the rootfs recipe adds at image time: libc.a in usr/lib, our
# header overlay in usr/x86_64-linux-musl/include (GCC's tool include
# directory, design-decisions.md 1185), and binutils' as and ld in
# usr/x86_64-linux-musl/bin, where GCC looks for them first.
#
# If native.sh has never run here the objects are missing; this says so and
# fails.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/gcc-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

TARGET=x86_64-linux-musl
VER="$SLATE_GCC_VERSION"
CROSS="$SLATE_WORK/gcc-cross"
WORK="$SLATE_WORK/gcc-native"
BUILD="$WORK/build"
DEST="$WORK/dest"
OUT="$SLATE_SPIKE/gcc"
JOBS="${SLATE_SPIKE_JOBS:-8}"

if [ ! -f "$BUILD/gcc/Makefile" ]; then
    echo "NO_OBJECTS -- $BUILD has no configured gcc/: GCC has not been built here."
    echo "  Build it first: wsl -d Ubuntu --exec bash scripts/gcc-spike/native.sh"
    exit 1
fi
if [ ! -x "$CROSS/prefix/bin/$TARGET-g++" ]; then
    echo "NO_CROSS_COMPILER -- $CROSS/prefix/bin has no $TARGET-g++."
    echo "  Build it first: wsl -d Ubuntu --exec bash scripts/gcc-spike/cross.sh"
    exit 1
fi
export PATH="$CROSS/prefix/bin:$PATH"
# What GCC's programs are linked against: the current libc.a, put where the
# cross compiler links from.
cp "$SLATE_SYSROOT/libc.a" "$CROSS/sysroot/usr/lib/libc.a" || exit 1

cd "$BUILD" || exit 1
# The programs, removed and linked again: make relinks only what is missing
# or older than its inputs, and libc.a is not among them; and a link that
# fails must not leave the last run's program to be installed as this one's.
for p in xgcc xg++ cpp cc1 cc1plus collect2 gcov gcov-dump gcov-tool; do
    rm -f "gcc/$p"
done
slate_resume_if_killed "$WORK/relink.log" make -j"$JOBS" all-gcc
echo "RELINK_EXIT=$?"
grep -E ' error: |Error [0-9]|undefined reference|multiple definition' "$WORK/relink.log" | head -20

rm -rf "${DEST:?}"
slate_resume_if_killed "$WORK/install.log" \
    make DESTDIR="$DEST" install-gcc install-target-libgcc install-target-libstdc++-v3
echo "INSTALL_EXIT=$?"
grep -E ' error: |Error [0-9]' "$WORK/install.log" | head -10

rm -rf "${OUT:?}"
mkdir -p "$OUT" || exit 1
cp -a "$DEST/." "$OUT/" || exit 1
# Manuals and info pages: nothing on the image reads them.
rm -rf "${OUT:?}/usr/share"
# gcc-ar, gcc-nm and gcc-ranlib, under both their names, only run binutils'
# ar, nm and ranlib with the LTO plugin, which a GCC built with --disable-lto
# has not got: each would print "Cannot find plugin" and fail. Binutils' own
# are on the image (scripts/binutils-spike/).
for t in ar nm ranlib; do
    rm -f "$OUT/usr/bin/gcc-$t" "$OUT/usr/bin/$TARGET-gcc-$t"
done
# The system's half of what the compiler reads, from cross.sh's sysroot.
mkdir -p "$OUT/usr/include" "$OUT/usr/lib" || exit 1
cp -R "$CROSS/sysroot/usr/include/." "$OUT/usr/include/" || exit 1
for f in crt1.o crti.o crtn.o libm.a libpthread.a libdl.a librt.a libutil.a \
         libcrypt.a libresolv.a libxnet.a; do
    cp "$CROSS/sysroot/usr/lib/$f" "$OUT/usr/lib/$f" || exit 1
done

missing=0
for f in usr/bin/gcc usr/bin/g++ usr/bin/cpp \
         "usr/libexec/gcc/$TARGET/$VER/cc1" "usr/libexec/gcc/$TARGET/$VER/cc1plus" \
         "usr/libexec/gcc/$TARGET/$VER/collect2" \
         "usr/lib/gcc/$TARGET/$VER/libgcc.a" \
         "usr/lib/gcc/$TARGET/$VER/crtbeginT.o" "usr/lib/gcc/$TARGET/$VER/crtend.o" \
         "usr/lib/gcc/$TARGET/$VER/include/stddef.h" "usr/include/c++/$VER/iostream" \
         usr/include/stdio.h usr/lib/crt1.o; do
    if [ ! -e "$OUT/$f" ]; then
        echo "MISSING_FROM_TREE $f"
        missing=$((missing + 1))
    fi
done
# libstdc++.a: in lib or lib64, as GCC's x86-64 configuration chooses.
stdcxx=0
for d in usr/lib usr/lib64; do
    if [ -f "$OUT/$d/libstdc++.a" ]; then
        stdcxx=1
        echo "LIBSTDCXX_IN=$d"
    fi
done
if [ "$stdcxx" -eq 0 ]; then
    echo "MISSING_FROM_TREE libstdc++.a"
    missing=$((missing + 1))
fi
# The unwinder C++ exceptions need: in libgcc.a here, or in libgcc_eh.a for a
# GCC built with shared libraries. Read from a file and counted there, as
# below.
unwinder=0
for a in "$OUT/usr/lib/gcc/$TARGET/$VER/libgcc.a" "$OUT/usr/lib/gcc/$TARGET/$VER/libgcc_eh.a"; do
    if [ -f "$a" ]; then
        nm --defined-only "$a" >"$WORK/unwind.txt" 2>/dev/null
        unwinder=$((unwinder + $(grep -c ' T _Unwind_RaiseException$' "$WORK/unwind.txt")))
    fi
done
if [ "$unwinder" -eq 0 ]; then
    echo "MISSING_FROM_TREE the unwinder (_Unwind_RaiseException, in libgcc.a or libgcc_eh.a)"
    missing=$((missing + 1))
fi
# The C++23 standard library modules' sources, which `import std;` and
# `import std.compat;` compile. GCC installs empty ones when it could not
# compile them while building libstdc++, and an empty module gives a program
# none of the library; native.sh's comment on CXX_FOR_TARGET says what once
# caused that.
for m in std std.compat; do
    src="$OUT/usr/include/c++/$VER/bits/$m.cc"
    if [ ! -f "$src" ] || [ "$(wc -c <"$src")" -lt 1000 ]; then
        echo "EMPTY_IN_TREE the $m module's source (usr/include/c++/$VER/bits/$m.cc)"
        missing=$((missing + 1))
    fi
done

# Every ELF program in the tree: stripped of DWARF, and checked as every
# port's link is -- ours, not musl's (our libc's rust_begin_unwind, none of
# musl's __syscall_cp), and marked native by the SlateOS ABI note. Written
# to files and counted there, as gdb-spike/slatelink.sh explains.
programs=0
bad=0
while IFS= read -r -d '' f; do
    # Not ELF (install-tools' shell scripts): readelf refuses it.
    if ! readelf -h "$f" >"$WORK/hdr.txt" 2>/dev/null; then
        continue
    fi
    if [ "$(grep -c 'EXEC (Executable file)' "$WORK/hdr.txt")" -eq 0 ]; then
        continue
    fi
    programs=$((programs + 1))
    if ! strip --strip-debug "$f"; then
        echo "STRIP_FAILED $f"
        bad=$((bad + 1))
        continue
    fi
    nm "$f" >"$WORK/syms.txt" 2>/dev/null
    readelf -n "$f" >"$WORK/notes.txt" 2>/dev/null
    if [ "$(grep -c 'rust_begin_unwind' "$WORK/syms.txt")" -eq 0 ] \
        || [ "$(grep -c '__syscall_cp' "$WORK/syms.txt")" -ne 0 ]; then
        echo "NOT_OUR_LIBC $f"
        bad=$((bad + 1))
    elif [ "$(grep -c 'SlateOS' "$WORK/notes.txt")" -eq 0 ]; then
        echo "NO_SLATEOS_NOTE $f"
        bad=$((bad + 1))
    fi
done < <(find "$OUT" -type f -perm -u+x -print0)
# The libraries a program here links -- libstdc++, libsupc++, libgcc and the
# rest -- were compiled with -g, and their DWARF is most of their size; a
# program built on SlateOS keeps its own. Symbols stay.
archives=0
while IFS= read -r -d '' a; do
    if strip --strip-debug "$a"; then
        archives=$((archives + 1))
    else
        echo "STRIP_FAILED $a"
        bad=$((bad + 1))
    fi
done < <(find "$OUT" -type f -name '*.a' -print0)
echo "ARCHIVES_STRIPPED=$archives"
echo "PROGRAMS=$programs BAD=$bad MISSING=$missing"
du -sh "$OUT"
if [ "$missing" -ne 0 ] || [ "$bad" -ne 0 ] || [ "$programs" -eq 0 ]; then
    exit 1
fi
echo "SLATE_GCC_TREE_BUILT"
