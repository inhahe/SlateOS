#!/bin/bash
# Compile and link a C program and a C++ one with cross.sh's GCC, against the CURRENT libc.a.
#
# scripts/gcc-spike/cross.sh builds GCC 16.2 as a cross compiler for SlateOS,
# with libgcc and libstdc++ built for SlateOS, and then runs this: the
# question it exists to answer. Do the compiler, its libraries and our libc.a
# make a program between them -- a C one, and a C++ one that throws and
# catches, writes to std::cout, holds strings in a vector, and joins a
# thread? Each link's missing and duplicated symbols are counted, and the
# program must be ours (our libc's rust_begin_unwind and none of musl's
# __syscall_cp) and marked native (the SlateOS ABI note), as every port's
# link is judged. First, though, the names libc.a shares with GCC's runtime
# are listed, and each must be weak in libc.a (check-libc-shape.py
# --runtime-overlap).
#
# The programs are SlateOS's, so they cannot run here; running them is a
# boot's question.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/gcc-spike/cross-check.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

TARGET=x86_64-linux-musl
WORK="$SLATE_WORK/gcc-cross"
SYSROOT="$WORK/sysroot"
PREFIX="$WORK/prefix"
CC="$PREFIX/bin/$TARGET-gcc"
CXX="$PREFIX/bin/$TARGET-g++"
if [ ! -x "$CC" ] || [ ! -x "$CXX" ]; then
    echo "NO_CROSS_COMPILER -- $PREFIX/bin has no $TARGET-gcc and -g++."
    echo "  Build them first: wsl -d Ubuntu --exec bash scripts/gcc-spike/cross.sh"
    exit 1
fi
# The sysroot's libc.a is a copy, brought up to date first: a check against
# yesterday's library answers for yesterday's.
cp "$SLATE_SYSROOT/libc.a" "$SYSROOT/usr/lib/libc.a" || exit 1

# Every name libc.a shares with this GCC's runtime must be weak there -- a
# link that extracts both definitions fails otherwise -- and listed in
# check-libc-shape.py's RUNTIME_PROVIDED, which holds it to that on every
# sysroot build, where there is no GCC to measure (its CHECK 7 says more).
GCCLIB="$PREFIX/lib/gcc/$TARGET/$SLATE_GCC_VERSION"
TLIB="$PREFIX/$TARGET/lib64"
runtime=("$GCCLIB"/crtbegin*.o "$GCCLIB"/crtend*.o "$GCCLIB/libgcc.a"
         "$TLIB/libsupc++.a" "$TLIB/libstdc++.a")
# Only a GCC built with shared libraries has its unwinder apart from libgcc.a.
if [ -f "$GCCLIB/libgcc_eh.a" ]; then
    runtime+=("$GCCLIB/libgcc_eh.a")
fi
python3 "$SLATE_ROOT/scripts/check-libc-shape.py" "$SYSROOT/usr/lib/libc.a" \
    --runtime-overlap "${runtime[@]}"
overlap=$?
echo "RUNTIME_OVERLAP_EXIT=$overlap"

T="$WORK/check"
rm -rf "$T"
mkdir -p "$T" && cd "$T" || exit 1

cat >hello.c <<'C_SOURCE'
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv)
{
    char *copy = strdup(argc > 0 ? argv[0] : "gcc");
    if (copy == NULL) {
        return 1;
    }
    printf("gcc-hello: C, built by GCC %d.%d, as %s\n", __GNUC__, __GNUC_MINOR__, copy);
    free(copy);
    return 42;
}
C_SOURCE

cat >hello.cpp <<'CXX_SOURCE'
#include <iostream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace {
struct Noisy {
    ~Noisy() { std::cout << "gcc-hello-cxx: a static object's destructor ran" << std::endl; }
};
Noisy at_exit;
}

int main()
{
    int caught = 0;
    try {
        throw std::runtime_error("thrown");
    } catch (const std::exception &e) {
        caught = std::string(e.what()) == "thrown";
    }
    std::vector<std::string> words{"built", "by", "GCC"};
    std::string joined;
    for (const auto &w : words) {
        joined += (joined.empty() ? "" : " ") + w;
    }
    int from_thread = 0;
    std::thread t([&from_thread] { from_thread = 7; });
    t.join();
    std::cout << "gcc-hello-cxx: " << joined << ", caught " << caught << ", thread "
              << from_thread << std::endl;
    return caught == 1 && from_thread == 7 ? 42 : 1;
}
CXX_SOURCE

built=0
tried=0
# One program, compiled and linked in one call as a user would, with its
# counts and checks.
check_one() {
    local name="$1" compiler="$2" src="$3" out="$4" log="$T/$1-build.log"
    tried=$((tried + 1))
    rm -f "$out"
    "$compiler" -O2 -Wall -o "$out" "$src" >"$log" 2>&1
    echo "${name}_BUILD_EXIT=$?"
    grep -oP "undefined reference to \`\K[^']*" "$log" | sort -u >"$T/$name-missing.txt"
    echo "${name}_MISSING_COUNT=$(wc -l <"$T/$name-missing.txt")"
    head -40 "$T/$name-missing.txt"
    grep -oP "multiple definition of \`\K[^']*" "$log" | sort -u >"$T/$name-dupes.txt"
    echo "${name}_DUPLICATE_COUNT=$(wc -l <"$T/$name-dupes.txt")"
    head -40 "$T/$name-dupes.txt"
    if [ ! -x "$out" ]; then
        grep -E 'error|warning' "$log" | head -20
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    ls -l "$out"
    # To files and counted there, as every spike's checks are (gdb-spike/
    # slatelink.sh says why not a pipe into grep -q, nor a substitution
    # around nm or readelf).
    local rust musl note exec_type
    nm "$out" >"$T/$name-symbols.txt" 2>/dev/null
    readelf -n "$out" >"$T/$name-notes.txt" 2>/dev/null
    readelf -h "$out" >"$T/$name-header.txt" 2>/dev/null
    rust="$(grep -c 'rust_begin_unwind' "$T/$name-symbols.txt")"
    musl="$(grep -c '__syscall_cp' "$T/$name-symbols.txt")"
    note="$(grep -c 'SlateOS' "$T/$name-notes.txt")"
    exec_type="$(grep -c 'EXEC (Executable file)' "$T/$name-header.txt")"
    if [ "$rust" -eq 0 ] || [ "$musl" -ne 0 ]; then
        echo "${name}_NOT_OUR_LIBC -- the link did not take this library's libc.a"
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    if [ "$note" -eq 0 ]; then
        echo "${name}_NO_SLATEOS_NOTE -- the kernel would run it as a Linux program"
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    if [ "$exec_type" -eq 0 ]; then
        echo "${name}_NOT_STATIC -- not a static executable; the default -static did not hold"
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    echo "SLATE_${name}_BUILT"
    built=$((built + 1))
}
check_one C "$CC" hello.c gcc-hello
check_one CXX "$CXX" hello.cpp gcc-hello-cxx
echo "PROGRAMS_BUILT=$built of $tried"
if [ "$built" -ne "$tried" ] || [ "$overlap" -ne 0 ]; then
    exit 1
fi
