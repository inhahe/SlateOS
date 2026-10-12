#!/bin/bash
# Link GDB's and gdbserver's built objects against the CURRENT libc.a, and stage them.
#
# scripts/gdb-spike/run.sh fetches GDB, GMP and MPFR, configures and compiles
# them -- the minutes -- and then runs this, the seconds: each program's link
# against toolchain/sysroot/lib/libc.a, its counts and checks, and a copy with
# the debugging information stripped in build/spike/. Every libc.a rebuild
# leaves that link behind, and scripts/create-ext4-rootfs.sh refuses to stage
# a binary older than the library it links, so the recipe runs this alone when
# the artifact is behind libc.a (`spike_rebuild_if_behind`), as it runs
# Oils', CPython's and LLVM's relinks. The objects do not depend on the C
# library -- only on its headers, which are musl's plus posix/include, and
# run.sh is what a header change needs.
#
# If run.sh has never run here the objects are missing; this says so and
# fails, and the recipe then reports the artifact absent, which is a NOTE.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/gdb-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/gdb-spike"
BUILD="$WORK/build"
SPIKE_LIBS="$WORK/libs"

if [ ! -f "$BUILD/gdb/Makefile" ] || [ ! -f "$BUILD/gdbserver/Makefile" ]; then
    echo "NO_OBJECTS -- $BUILD has no configured gdb/ and gdbserver/: GDB has not been built here."
    echo "  Build it first: wsl -d Ubuntu --exec bash scripts/gdb-spike/run.sh"
    exit 1
fi

# The link wrappers configure baked into the Makefiles ($WORK/bin/cc and
# c++) are written again, around this libc.a. They are zig's ld.lld itself
# with our libc.a where zig puts musl's (slate_make_link_wrappers in
# scripts/lib/worktree.sh says why the order matters); the zig driver
# wrappers under /tmp they call are written again too, since WSL empties
# /tmp when it idles.
slate_make_zig_wrappers || exit 1
mkdir -p "$SPIKE_LIBS" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
cd "$BUILD" || exit 1

built=0
# One program's link, through its own Makefile -- it knows the inputs, which
# for gdb are some hundreds of objects and a dozen archives -- with its counts.
link_one() {
    local name="$1" dir="$2" bin="$3" log="$WORK/$1-link.log"
    # Removed first: make relinks only a program that is missing or older
    # than its inputs, and libc.a is not among them; and a link that fails
    # must not leave the last run's binary to be staged as this one's.
    rm -f "$dir/$bin"
    make -C "$dir" "$bin" >"$log" 2>&1
    echo "${name}_LINK_EXIT=$?"
    # A link make never reached -- an object that did not compile -- has no
    # undefined symbols to count, and a count of 0 beside it would read as
    # "nothing is missing". Said instead, with the reason. Reached means the
    # linker ran: it made the program, or wrote why not. (Not "the log names
    # ld.lld" alone: a link that succeeds prints only make's CXXLD line, and
    # the first version of this test called GDB's clean link unreached.)
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
    file "$dir/$bin"
    readelf -h "$dir/$bin" | grep -E "Type|Entry"
    # The link must be against ours, not musl's: our libc.a is Rust, and
    # brings its panic entry point; musl's would bring __syscall_cp. And the
    # program must carry the SlateOS ABI note our crt adds, which is how the
    # kernel knows a native program from a Linux one (kernel/src/proc/elf.rs)
    # -- without it, it would be run under the Linux ABI.
    #
    # Written to files and counted there. Not `nm | grep -q`: under pipefail,
    # grep -q stopping at its first match leaves nm to die of SIGPIPE, and the
    # pipe then reports failure for a match -- which is how this check's first
    # version refused GDB's own correct link. And not `$(nm ...)`: the
    # boot test's check-shell-callables.py requires every command a
    # substitution runs to exist where the gate runs, and nm and readelf are
    # this WSL image's, not the Windows host's.
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
    # --strip-debug, as CPython's and eSpeak's are staged: the symbol table
    # stays, so a backtrace names its functions, and the DWARF -- 97 MB of
    # gdb's 111 -- goes.
    strip --strip-debug -o "$SLATE_SPIKE/$bin-slateos.elf" "$dir/$bin" || {
        echo "NO_SLATE_${name}_BINARY -- strip failed"
        return
    }
    ls -l "$SLATE_SPIKE/$bin-slateos.elf"
    echo "SLATE_${name}_BUILT"
    built=$((built + 1))
}
mkdir -p "$SLATE_SPIKE" || exit 1
link_one GDB gdb gdb
link_one GDBSERVER gdbserver gdbserver
# Success is both programs linked, and nothing less.
[ "$built" -eq 2 ] || exit 1
