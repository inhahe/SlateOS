#!/bin/bash
# Link the Mono runtime's built objects against the CURRENT libc.a, and stage it.
#
# scripts/mono-spike/run.sh fetches Mono, configures and compiles the runtime
# -- the minutes -- and then runs this, the seconds: mono-sgen's link against
# toolchain/sysroot/lib/libc.a, its counts and checks, and a copy with the
# debugging information stripped in build/spike/. Every libc.a rebuild leaves
# that link behind, and scripts/create-ext4-rootfs.sh refuses to stage a binary
# older than the library it links, so the recipe runs this alone when the
# artifact is behind libc.a (`spike_rebuild_if_behind`), as it runs GDB's and
# Oils'. The objects do not depend on the C library -- only on its headers,
# which are musl's plus posix/include, and run.sh is what a header change needs.
#
# If run.sh has never run here the objects are missing; this says so and
# fails, and the recipe then reports the artifact absent, which is a NOTE.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/mono-spike/slatelink.sh
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/mono-spike"
TREE="$WORK/mono-$SLATE_MONO_VERSION"
SPIKE_LIBS="$WORK/libs"

if [ ! -f "$TREE/mono/mini/Makefile" ]; then
    echo "NO_OBJECTS -- $TREE has no configured mono/mini/: the runtime has not been built here."
    echo "  Build it first: wsl -d Ubuntu --exec bash scripts/mono-spike/run.sh"
    exit 1
fi

# The wrappers configure baked into the Makefiles ($WORK/bin/cc and c++) are
# written again around this libc.a, and the zig driver wrappers under /tmp
# they call are written again too, since WSL empties /tmp when it idles.
slate_make_zig_wrappers || exit 1
mkdir -p "$SPIKE_LIBS" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
cd "$TREE" || exit 1

link_log="$WORK/mono-link.log"
# Removed first: make relinks only a program that is missing or older than
# its inputs, and libc.a is not among them; and a link that fails must not
# leave the last run's binary to be staged as this one's.
rm -f mono/mini/mono-sgen
make -C mono/mini mono-sgen >"$link_log" 2>&1
echo "MONO_LINK_EXIT=$?"
# Reached means the linker ran: it made the program, or wrote why not. Not
# "the log names ld.lld" alone -- a link that succeeds prints only make's CCLD
# line, and this test's first version called Mono's clean link unreached.
if [ ! -x mono/mini/mono-sgen ] && ! grep -q "ld\.lld" "$link_log"; then
    # Not reached: an object it needs did not compile. A count of 0 here would
    # read as "nothing is missing", so it is not printed.
    echo "MONO_LINK_NOT_REACHED -- the build of its objects failed:"
    grep -E ' error: |Error [0-9]|No rule to make' "$link_log" | head -10
    echo "NO_SLATE_MONO_BINARY"
    exit 1
fi
grep -oP "undefined symbol: \K.*" "$link_log" | sort -u >"$WORK/mono-missing.txt"
echo "MONO_MISSING_COUNT=$(wc -l <"$WORK/mono-missing.txt")"
head -40 "$WORK/mono-missing.txt"
# Printed even at zero: a symbol defined twice fails a link as surely as one
# defined nowhere (make-spike's note).
grep -oP "duplicate symbol: \K.*" "$link_log" | sort -u >"$WORK/mono-dupes.txt"
echo "MONO_DUPLICATE_COUNT=$(wc -l <"$WORK/mono-dupes.txt")"
head -40 "$WORK/mono-dupes.txt"
if [ ! -x mono/mini/mono-sgen ]; then
    echo "NO_SLATE_MONO_BINARY"
    exit 1
fi

file mono/mini/mono-sgen
readelf -h mono/mini/mono-sgen | grep -E "Type|Entry"
# `file` calls it "static-pie": Mono links with -Wl,--export-dynamic, which
# gives a static executable a PT_DYNAMIC segment. readelf's Type is the fact
# (EXEC), and the kernel takes a program for dynamic only by its PT_INTERP
# (kernel/src/proc/elf.rs, interp_path), which this must have none of.
#
# Each tool's output goes to a file and is counted there: neither inside
# $(...) -- check-shell-callables.py requires a substitution's command to exist
# where the gate runs, and readelf and nm are this WSL image's -- nor as
# `... | grep -q`, which pipefail turns into a failure on a match
# (scripts/gdb-spike/slatelink.sh says how).
readelf -l mono/mini/mono-sgen >"$WORK/mono-segments.txt" 2>/dev/null
readelf -n mono/mini/mono-sgen >"$WORK/mono-notes.txt" 2>/dev/null
nm mono/mini/mono-sgen >"$WORK/mono-symbols.txt" 2>/dev/null
if [ "$(grep -c INTERP "$WORK/mono-segments.txt")" -ne 0 ]; then
    echo "MONO_HAS_AN_INTERPRETER -- linked dynamically; nothing here can load it"
    echo "NO_SLATE_MONO_BINARY"
    exit 1
fi
# The link must be against ours, not musl's: our libc.a is Rust, and brings
# its panic entry point; musl's would bring __syscall_cp.
if [ "$(grep -c 'rust_begin_unwind' "$WORK/mono-symbols.txt")" -eq 0 ] \
    || [ "$(grep -c '__syscall_cp' "$WORK/mono-symbols.txt")" -ne 0 ]; then
    echo "MONO_NOT_OUR_LIBC -- the link did not take this library's libc.a"
    echo "NO_SLATE_MONO_BINARY"
    exit 1
fi
# And it must carry the SlateOS ABI note our crt adds: the kernel tells a
# native program from a Linux one by it (kernel/src/proc/elf.rs).
if [ "$(grep -c 'SlateOS' "$WORK/mono-notes.txt")" -eq 0 ]; then
    echo "MONO_NO_SLATEOS_NOTE -- the kernel would run it as a Linux program"
    echo "NO_SLATE_MONO_BINARY"
    exit 1
fi
# --strip-debug, as GDB's and CPython's are staged: the symbol table stays,
# so a backtrace names its functions, and the DWARF goes (24 MB to 7.6).
mkdir -p "$SLATE_SPIKE" || exit 1
strip --strip-debug -o "$SLATE_SPIKE/mono-sgen-slateos.elf" mono/mini/mono-sgen || exit 1
ls -l "$SLATE_SPIKE/mono-sgen-slateos.elf"
echo "SLATE_MONO_BUILT"
