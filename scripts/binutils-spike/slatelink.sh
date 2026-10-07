#!/bin/bash
# Link binutils' built objects against the CURRENT libc.a, and stage its programs.
#
# scripts/binutils-spike/run.sh fetches binutils, configures and compiles it
# -- the minutes -- and then runs this, the seconds: each program's link
# against toolchain/sysroot/lib/libc.a, its counts and checks, and a copy with
# the debugging information stripped in build/spike/binutils/, under the name
# it is installed by. Every libc.a rebuild leaves that link behind, and
# scripts/create-ext4-rootfs.sh refuses to stage a program older than the
# library it links, so a recipe that stages these runs this alone when they
# are behind libc.a, as it runs GDB's and LLVM's relinks. The objects do not
# depend on the C library -- only on its headers, which are musl's plus
# posix/include, and run.sh is what a header change needs.
#
# If run.sh has never run here the objects are missing; this says so and
# fails.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/binutils-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/binutils-spike"
BUILD="$WORK/build"
SPIKE_LIBS="$WORK/libs"
STAGED="$SLATE_SPIKE/binutils"

for dir in binutils gas ld; do
    if [ ! -f "$BUILD/$dir/Makefile" ]; then
        echo "NO_OBJECTS -- $BUILD has no configured $dir/: binutils has not been built here."
        echo "  Build it first: wsl -d Ubuntu --exec bash scripts/binutils-spike/run.sh"
        exit 1
    fi
done

# The link wrappers configure baked into the Makefiles ($WORK/bin/cc and
# c++) are written again, around this libc.a, with the zig driver wrappers
# under /tmp they call (WSL empties /tmp when it idles).
slate_make_zig_wrappers || exit 1
mkdir -p "$SPIKE_LIBS" "$STAGED" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
cd "$BUILD" || exit 1

built=0
tried=0
# One program's link, through its own Makefile, with its counts: NAME for the
# numbers printed, the build directory, the program's name there, and the
# name it is installed by (nm is built as nm-new, as as-new, c++filt as
# cxxfilt ...).
link_one() {
    local name="$1" dir="$2" bin="$3" installed="$4" log="$WORK/$1-link.log"
    tried=$((tried + 1))
    # Removed first: make relinks only a program that is missing or older
    # than its inputs, and libc.a is not among them; and a link that fails
    # must not leave the last run's program to be staged as this one's.
    rm -f "$dir/$bin" "$STAGED/$installed"
    make -C "$dir" "$bin" >"$log" 2>&1
    echo "${name}_LINK_EXIT=$?"
    # Reached means the linker ran: it made the program, or wrote why not. A
    # link make never reached has no undefined symbols to count, and a 0
    # beside it would read as "nothing is missing" (scripts/gdb-spike/
    # slatelink.sh learned that).
    if [ ! -x "$dir/$bin" ] && ! grep -q "ld\.lld" "$log"; then
        echo "${name}_LINK_NOT_REACHED -- the build of its objects failed:"
        grep -E ' error: |Error [0-9]|No rule to make' "$log" | head -10
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    grep -oP "undefined symbol: \K.*" "$log" | sort -u >"$WORK/$name-missing.txt"
    echo "${name}_MISSING_COUNT=$(wc -l <"$WORK/$name-missing.txt")"
    head -40 "$WORK/$name-missing.txt"
    # Counted apart, and printed even at zero: a link can fail by a symbol
    # defined twice as surely as by one defined nowhere (make-spike's note).
    grep -oP "duplicate symbol: \K.*" "$log" | sort -u >"$WORK/$name-dupes.txt"
    echo "${name}_DUPLICATE_COUNT=$(wc -l <"$WORK/$name-dupes.txt")"
    head -40 "$WORK/$name-dupes.txt"
    if [ ! -x "$dir/$bin" ]; then
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    # Ours, not musl's: our libc.a is Rust and brings its panic entry point,
    # musl's would bring __syscall_cp. And marked as a native program, by
    # the SlateOS ABI note our crt adds -- without it the kernel would run it
    # under the Linux ABI (kernel/src/proc/elf.rs). Each tool's output goes
    # to a file and is counted there: not `nm | grep -q`, which pipefail
    # turns into a failure on a match, and not a command substitution around
    # nm or readelf, whose command check-shell-callables.py looks for on the
    # Windows host (gdb-spike/slatelink.sh says more).
    local rust musl note
    nm "$dir/$bin" >"$WORK/$name-symbols.txt" 2>/dev/null
    readelf -n "$dir/$bin" >"$WORK/$name-notes.txt" 2>/dev/null
    rust="$(grep -c 'rust_begin_unwind' "$WORK/$name-symbols.txt")"
    musl="$(grep -c '__syscall_cp' "$WORK/$name-symbols.txt")"
    note="$(grep -c 'SlateOS' "$WORK/$name-notes.txt")"
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
    # --strip-debug, as GDB's are staged: the symbol table stays, so a
    # backtrace names its functions, and the DWARF goes.
    strip --strip-debug -o "$STAGED/$installed" "$dir/$bin" || {
        echo "NO_SLATE_${name}_BINARY -- strip failed"
        return
    }
    ls -l "$STAGED/$installed"
    echo "SLATE_${name}_BUILT"
    built=$((built + 1))
}
link_one AS gas as-new as
link_one LD ld ld-new ld.bfd
link_one AR binutils ar ar
link_one RANLIB binutils ranlib ranlib
link_one NM binutils nm-new nm
link_one OBJDUMP binutils objdump objdump
link_one OBJCOPY binutils objcopy objcopy
link_one READELF binutils readelf readelf
link_one STRIP binutils strip-new strip
link_one SIZE binutils size size
link_one STRINGS binutils strings strings
link_one ADDR2LINE binutils addr2line addr2line
link_one CXXFILT binutils cxxfilt c++filt
link_one ELFEDIT binutils elfedit elfedit
echo "PROGRAMS_BUILT=$built of $tried"
# Success is every program linked, and nothing less.
[ "$built" -eq "$tried" ] || exit 1
