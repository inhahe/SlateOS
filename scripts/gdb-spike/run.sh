#!/bin/bash
# Cross-compile upstream GDB, with GMP and MPFR, and link it against SlateOS's libc.a.
#
# The "try the port before you write a line" step from roadmap-detailed.md's
# "Porting vs. Reimplementing" policy, applied to the debugger the operator
# asked for (design-decisions.md 1050: "do we have a capable debugger, like
# cdb? We should port one or more of those"; roadmap.md gives the port to lane
# D). Shaped like scripts/make-spike/run.sh, deliberately, down to the names
# of the numbers it prints: the spikes answer the same question.
#
# WHY GDB FIRST, AND NOT LLDB. LLDB is a client of the whole of LLVM and Clang
# (its expression evaluator is a Clang compiler), so porting it means porting
# both first; GDB needs two C libraries, GMP and MPFR, which it builds here.
# The kernel half the two need is the same -- see below.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# It answers one question: does the real GDB 18.1, unmodified, resolve every
# symbol it needs against `toolchain/sysroot/lib/libc.a`? -- and the same for
# gdbserver, the small half that runs beside the program being debugged.
#
# It does NOT answer whether GDB can debug anything. A debugger controls
# another process through ptrace(2) and /proc/<pid>/mem, and SlateOS has
# neither for native programs yet: libc's ptrace answers ENOSYS. That half is
# asked of lane A in requests/d-a-a-debugger-needs-ptrace-for-native-programs.md.
# Until it exists a GDB that links can examine a program without running it
# (symbols, types, disassembly) and nothing more. Treat MISSING_COUNT=0 as
# permission to stage it in a rootfs, not as a debugger.
#
# Run it from WSL. The work tree is durable ($SLATE_WORK, not /tmp, which WSL
# empties when the distribution idles), and the libraries are copied off the
# /mnt mount first because 9p is slow and a linker reads an archive many
# times.
set -uo pipefail
set -x

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/gdb-spike"
SPIKE_LIBS="$WORK/libs"
# GMP and MPFR are installed here, and GDB's configure is pointed at it.
PREFIX="$WORK/prefix"
JOBS="${SLATE_SPIKE_JOBS:-8}"

# GMP preprocesses its assembly sources with m4 while it builds, on this
# machine, and the Ubuntu image the spikes run in does not ship it --
# configure stops with "No usable m4". Installing it needs root, which a spike
# must not need. `apt-get download` does not: it fetches Ubuntu's own
# package, checked against the archive's signed index, and `dpkg -x` unpacks
# it into the cache without installing anything.
ensure_host_m4() {
    command -v m4 >/dev/null && return 0
    local dir="$SLATE_ZIG_CACHE/host-tools/m4" tmp
    if [ ! -x "$dir/usr/bin/m4" ]; then
        tmp="$(mktemp -d "$SLATE_ZIG_CACHE/m4dl.XXXXXX")" || return 1
        if ! (cd "$tmp" && apt-get download m4) || ! mkdir -p "$dir" \
            || ! dpkg -x "$tmp"/m4_*.deb "$dir"; then
            rm -rf "$tmp"
            echo "gdb-spike: could not fetch Ubuntu's m4 package; GMP needs m4" >&2
            return 1
        fi
        rm -rf "$tmp"
    fi
    export PATH="$dir/usr/bin:$PATH"
}

slate_make_zig_wrappers || exit 1
ensure_host_m4 || exit 1
slate_ensure_gdb_src || exit 1
slate_ensure_gmp_src || exit 1
slate_ensure_mpfr_src || exit 1

mkdir -p "$WORK" "$SPIKE_LIBS" && cd "$WORK" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
# Every compile and every link goes through the wrappers, so each configure
# test is answered by our libc.a, and GMP and MPFR are measured too: a symbol
# they need from libc is as much a gap as one GDB needs.
export CC="$SLATE_LINK_CC" CXX="$SLATE_LINK_CXX" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

# --build as well as --host, as in every spike: a program linked against our
# libc.a cannot run on Linux, so configure must not try. GMP builds a few
# generator programs it runs during its own build; those are made with the
# host's compiler (CC_FOR_BUILD), which is correct, since they run here.
rm -rf "$PREFIX" "gmp-$SLATE_GMP_VERSION" "mpfr-$SLATE_MPFR_VERSION"
tar xf "$SLATE_GMP_TARBALL" || exit 1
(
    cd "gmp-$SLATE_GMP_VERSION" || exit 1
    ./configure --build=x86_64-pc-linux-gnu --host=x86_64-linux-musl \
        --prefix="$PREFIX" --disable-shared --enable-static \
        CC_FOR_BUILD=gcc >conf.log 2>&1 || exit 11
    make -j"$JOBS" >make.log 2>&1 || exit 12
    make install >install.log 2>&1 || exit 13
)
echo "GMP_EXIT=$?"

tar xf "$SLATE_MPFR_TARBALL" || exit 1
(
    cd "mpfr-$SLATE_MPFR_VERSION" || exit 1
    ./configure --build=x86_64-pc-linux-gnu --host=x86_64-linux-musl \
        --prefix="$PREFIX" --disable-shared --enable-static \
        --with-gmp="$PREFIX" >conf.log 2>&1 || exit 21
    make -j"$JOBS" >make.log 2>&1 || exit 22
    make install >install.log 2>&1 || exit 23
)
echo "MPFR_EXIT=$?"
if [ ! -f "$PREFIX/lib/libgmp.a" ] || [ ! -f "$PREFIX/lib/libmpfr.a" ]; then
    echo "NO_GMP_OR_MPFR -- see $WORK/gmp-$SLATE_GMP_VERSION and mpfr-$SLATE_MPFR_VERSION"
    exit 1
fi

# GDB, built outside its source tree as its README asks.
#
# What is left out, and why each is not part of the question:
#   --disable-sim, --disable-gprofng: simulators for other CPUs and a profiler
#       that ship in the same tarball and are not GDB.
#   --disable-inprocess-agent: a shared library loaded INTO the debugged
#       program for fast tracepoints; there is no dynamic loader on this path.
#   --without-python, --without-guile: GDB's extension languages. Python is a
#       port of its own (scripts/cpython-spike); linking GDB against it is a
#       second step once both link.
#   --without-expat, --without-lzma, --without-zstd, --without-debuginfod,
#   --without-babeltrace, --without-intel-pt, --without-xxhash,
#   --disable-source-highlight: optional libraries GDB uses when they are
#       found. Each would be a port of its own, and configure must not find
#       the host's copy and link a Linux library into a SlateOS binary.
#   --disable-nls: message catalogues; the C locale is the only one we have.
# No ncurses: GDB's configure falls back to its own stub-termcap.o when no
# termcap library is found, and turns the TUI off with a warning.
rm -rf "gdb-$SLATE_GDB_VERSION" build
tar xf "$SLATE_GDB_TARBALL" || exit 1
mkdir build && cd build || exit 1
"../gdb-$SLATE_GDB_VERSION/configure" \
    --build=x86_64-pc-linux-gnu --host=x86_64-linux-musl \
    --disable-shared --enable-static --disable-nls --disable-werror \
    --disable-sim --disable-gprofng --disable-inprocess-agent \
    --without-python --without-guile --without-expat --without-lzma \
    --without-zstd --without-debuginfod --without-babeltrace \
    --without-intel-pt --without-xxhash --disable-source-highlight \
    --with-gmp="$PREFIX" --with-mpfr="$PREFIX" \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 conf.log

# -k: one failing object must not hide the rest. The question is everything
# GDB needs, and a build that stops at the first error answers only "the
# first thing".
make -k -j"$JOBS" all-gdb all-gdbserver >make.log 2>&1
echo "MAKE_EXIT=$?"
grep -E ' error: |Error [0-9]' make.log | grep -v 'undefined symbol' | head -30

# The decisive step: each program's link on its own, so its counts can be
# read. The build above linked through the same wrapper; the objects exist
# now, so make does only the link again (or nothing, if it succeeded).
built=0
measure() {
    local name="$1" dir="$2" bin="$3"
    rm -f "$dir/$bin"
    make -C "$dir" "$bin" >"$name-link.log" 2>&1
    echo "${name}_LINK_EXIT=$?"
    # A link make never reached -- an object that did not compile -- has no
    # undefined symbols to count, and a count of 0 beside it would read as
    # "nothing is missing". Said instead, with the reason. Reached means the
    # linker ran: it made the program, or wrote why not. (Not "the log names
    # ld.lld" alone: a link that succeeds prints only make's CXXLD line, and
    # the first version of this test called GDB's clean link unreached.)
    if [ ! -x "$dir/$bin" ] && ! grep -q "ld\.lld" "$name-link.log"; then
        echo "${name}_LINK_NOT_REACHED -- the build of its objects failed:"
        grep -E ' error: |Error [0-9]|No rule to make' "$name-link.log" | head -10
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    grep -oP "undefined symbol: \K.*" "$name-link.log" | sort -u >"$name-missing.txt"
    echo "${name}_MISSING_COUNT=$(wc -l <"$name-missing.txt")"
    cat "$name-missing.txt"
    # Counted apart, and printed even at zero: a link can fail by a symbol
    # defined twice as surely as by one defined nowhere (make-spike's note).
    grep -oP "duplicate symbol: \K.*" "$name-link.log" | sort -u >"$name-dupes.txt"
    echo "${name}_DUPLICATE_COUNT=$(wc -l <"$name-dupes.txt")"
    cat "$name-dupes.txt"
    if [ -x "$dir/$bin" ]; then
        file "$dir/$bin"
        readelf -h "$dir/$bin" | grep -E "Type|Entry"
        cp "$dir/$bin" "$SLATE_SPIKE/$bin-slateos.elf"
        ls -l "$SLATE_SPIKE/$bin-slateos.elf"
        echo "SLATE_${name}_BUILT"
        built=$((built + 1))
    else
        echo "NO_SLATE_${name}_BINARY"
    fi
}
measure GDB gdb gdb
measure GDBSERVER gdbserver gdbserver
# Success is both programs linked, and nothing less.
[ "$built" -eq 2 ] || exit 1
