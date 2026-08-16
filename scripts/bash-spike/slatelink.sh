#!/bin/bash
# The decisive step: relink bash's already-compiled objects against SlateOS's
# OWN libc.a instead of musl's. The musl link proved the source cross-compiles;
# this proves (or disproves) that SlateOS's libc actually satisfies bash.
#
# The objects were compiled against musl headers, which is legitimate here:
# SlateOS's libc.a is built to match the musl ABI (see fastpy
# compiler/toolchain.py, which cross-compiles its C runtime exactly this way).
set -x
# Derive the repo root from this script's own location rather than hard-coding
# it. Until 2026-08-16 both paths below named the `os` integration worktree
# outright, which is wrong in three separate ways once there are four
# checkouts: a lane running this relinked bash against *another* lane's
# libc.a (so the artifact proved nothing about its own), wrote the result into
# *another* lane's build/spike (clobbering it), and left its own worktree with
# no bash-slateos.elf at all — which is why lane B's and lane C's boot tests
# silently no-op `self_test_bash_on_slateos_libc` while only `main` ever
# actually runs bash. See known-issues.md ->
# B-BASH-SLATEOS-ELF-WAS-EXEMPT-FROM-THE-STALENESS-GATE.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)" || exit 1
SPIKE="$ROOT/build/spike"
SYSROOT="$ROOT/toolchain/sysroot/lib"

# Scratch dirs are keyed by worktree name (os, os-lane-a, os-lane-b, …) so two
# lanes relinking at once cannot write each other's objects or sysroot copy.
# The boot lock serialises QEMU, not this.
LANE="$(basename "$ROOT")"
BUILD="/tmp/bash-cross"
SLATE_SYSROOT="/tmp/slate-sysroot-$LANE"

if [ ! -d "$BUILD" ]; then
    echo "ERROR: $BUILD does not exist — bash's objects have not been compiled yet."
    echo "       See scripts/bash-spike/README.md (the configure + make step) first;"
    echo "       this script only relinks objects that step produced."
    exit 1
fi
cd "$BUILD" || exit 1
mkdir -p "$SPIKE" || exit 1

# Copy the sysroot off /mnt/d — the linker reads these archives heavily and
# the 9p mount is slow.
mkdir -p "$SLATE_SYSROOT"
cp "$SYSROOT"/libc.a "$SYSROOT"/libstubs.a "$SYSROOT"/libunwind.a "$SLATE_SYSROOT"/ || exit 1
ls -l "$SLATE_SYSROOT"

OBJS="shell.o eval.o y.tab.o general.o make_cmd.o print_cmd.o dispose_cmd.o \
execute_cmd.o variables.o copy_cmd.o error.o expr.o flags.o jobs.o subst.o \
hashcmd.o hashlib.o mailcheck.o trap.o input.o unwind_prot.o pathexp.o sig.o \
test.o version.o alias.o array.o arrayfunc.o assoc.o braces.o bracecomp.o \
bashhist.o bashline.o list.o stringlib.o locale.o findcmd.o redir.o \
pcomplete.o pcomplib.o syntax.o xmalloc.o signames.o"

# -nostdlib: we want SlateOS's libc, not zig's bundled musl.
#
# libstubs.a is deliberately NOT linked. Both it and libc.a are Rust-built and
# each carries its own panic handler, so together they collide on
# `__rustc::rust_begin_unwind`. That costs us nothing here: the symbol survey
# found exactly one bash symbol served only by libstubs.a (killpg), and libc.a
# turns out to cover even that.
#
# NO SHIM. The first run of this script had to stand in for killpg, eaccess and
# __fpurge — the only three of bash's 2,030 referenced symbols SlateOS's libc
# did not provide. All three are now implemented for real in posix/src
# (signal.rs, file.rs, stdio.rs respectively), so bash links against our libc
# and nothing else. If this fails to find them, re-run
# toolchain/build-sysroot.ps1; scripts/bash-spike/checksyms.sh confirms they
# are present in the archive.
/tmp/zigcc -static -nostdlib -o bash-slateos $OBJS \
    -L./builtins -L./lib/glob -L./lib/tilde -L./lib/sh -L./lib/readline \
    -lbuiltins -lglob -lsh -ltilde -lhistory \
    "$SLATE_SYSROOT"/libc.a "$SLATE_SYSROOT"/libc.a \
    "$SLATE_SYSROOT"/libunwind.a \
    2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

echo "=== distinct undefined symbols the SlateOS link is missing ==="
grep -oP "undefined symbol: \K.*" slate-link.log | sort -u | tee "/tmp/slate_missing-$LANE.txt" | head -60
echo "MISSING_COUNT=$(sort -u "/tmp/slate_missing-$LANE.txt" | wc -l)"
echo "=== other errors ==="
grep -v "undefined symbol\|^>>>" slate-link.log | head -20

if [ -x "$BUILD/bash-slateos" ]; then
  file "$BUILD/bash-slateos"
  ls -l "$BUILD/bash-slateos"
  echo "SLATE_BASH_BUILT"
  cp "$BUILD/bash-slateos" "$SPIKE/bash-slateos.elf"
else
  echo "NO_SLATE_BINARY"
fi
