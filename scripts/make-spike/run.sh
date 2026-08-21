#!/bin/bash
# Cross-compile upstream GNU make and link it against SlateOS's own libc.a.
#
# This is the "try the port before you write a line" step from
# roadmap-detailed.md's "Porting vs. Reimplementing" policy, applied to the
# `make` third of the roadmap's `gcc, cmake, make, pkg-config (via POSIX layer)`
# item. It is modelled on scripts/pkgconf-spike/run.sh, deliberately, down to
# the variable names: the two answer the same question, and a reader who has
# read one should not have to re-learn the shape to read the other.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# It answers exactly one question: does the real GNU make, unmodified, resolve
# every symbol it needs against `toolchain/sysroot/lib/libc.a`? A link is a
# complete, mechanical enumeration of a program's demands on its libc, which is
# why it is worth doing before writing a reimplementation — the pkgconf rewrite
# reached 3,143 lines before anyone asked, and the answer was "yes, on the first
# attempt" (design-decisions.md §307).
#
# It does NOT answer whether make *runs*. `make` leans much harder on the
# operating system than pkgconf does — it forks, execs, waits, opens pipes,
# installs SIGCHLD handlers, and on a `-j` build runs a jobserver over a pipe or
# a POSIX named semaphore. A clean link tells you none of that works; it tells
# you nothing is *missing*. Treat MISSING_COUNT=0 as permission to build a
# rootfs rung, not as a port.
#
# Run it from WSL. Everything except the sysroot lives in /tmp, because the
# linker reads the archives heavily and the 9p mount to /mnt/d is slow.
set -uo pipefail
set -x

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="$SLATE_MAKE_VERSION"
SYSROOT="$SLATE_SYSROOT"
WORK="/tmp/make-spike-$SLATE_LANE"
SPIKE_LIBS="/tmp/slate-sysroot2-$SLATE_LANE"

slate_make_zig_wrappers || exit 1
slate_ensure_make_src || exit 1

mkdir -p "$WORK" && cd "$WORK" || exit 1
rm -rf "make-$VER"
tar xf "$SLATE_MAKE_TARBALL" || exit 1
cd "make-$VER" || exit 1

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

# --disable-shared: SlateOS has no dynamic loader on this path, so everything is
# a static ET_EXEC. `--without-guile` because GNU Guile is an optional embedded
# extension language that is not part of what anyone means by "make", and
# letting configure find a host Guile would link a host library into a cross
# binary.
#
# ac_cv_func_gettimeofday and friends are NOT overridden here: the entire point
# is to let configure probe our libc through zig's musl headers and reach its
# own conclusions, then see whether the link agrees.
./configure --host=x86_64-linux-musl --disable-shared --without-guile \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 conf.log

make -j8 >make.log 2>&1
echo "MAKE_EXIT=$?"
grep -iE "error|warning: implicit" make.log | head -20

# What configure decided about the two facilities most likely to be the reason
# a make binary that links does not run: the jobserver style and how it waits
# for children. Recorded in the build log because config.h lives under /tmp and
# does not survive it being cleared — the same "the artifact outlived the facts
# about it" problem that left pkgconf's binary in /tmp for two days.
echo "MAKE_JOBSERVER_AND_WAIT_DECISIONS_FROM_CONFIG_H:"
grep -E 'HAVE_(NAMED_)?SEMAPHORES|HAVE_MKFIFO|HAVE_WAITPID|HAVE_WAIT3|HAVE_POSIX_SPAWN|HAVE_FORK|HAVE_VFORK|MAKE_JOBSERVER|HAVE_SYS_LOADAVG' \
    config.h

# The decisive step. -nostdlib so we get SlateOS's libc, not zig's bundled musl.
# libc.a twice: it is Rust-built and its intra-archive references are not
# topologically ordered, so a second pass is cheaper than --start-group.
# libstubs.a is deliberately not linked — it and libc.a each carry a panic
# handler and collide on __rustc::rust_begin_unwind.
mkdir -p "$SPIKE_LIBS"
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1

# Take the object list from make's own build rather than globbing *.o: the
# tarball ships a `lib/` gnulib subdirectory and a `tests/` tree, and a glob
# would sweep in objects the real link does not use and miss the ones it does.
OBJS="$(ls src/*.o 2>/dev/null | tr '\n' ' ')"
echo "OBJ_COUNT=$(echo $OBJS | wc -w)"
if [ -z "$OBJS" ]; then
    echo "NO_OBJECTS — the build above failed; see $WORK/make-$VER/make.log"
    exit 1
fi

GLLIB="lib/libgnu.a"
"$SLATE_CC" -static -nostdlib -o make-slateos $OBJS "$GLLIB" \
    "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libunwind.a" \
    2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

MISSING="/tmp/make_missing-$SLATE_LANE.txt"
grep -oP "undefined symbol: \K.*" slate-link.log | sort -u >"$MISSING"
echo "MISSING_COUNT=$(wc -l <"$MISSING")"
cat "$MISSING"

# Count duplicates SEPARATELY, and do not let MISSING_COUNT stand in for "the
# link succeeded". The first run of this spike printed `SLATE_LINK_EXIT=1`
# beside `MISSING_COUNT=0` and was briefly read as a fluke, because the only
# number on screen was the one that said everything was fine: all eleven errors
# were *duplicate* definitions, which the grep above does not match. A spike
# whose headline metric can be zero on a failed link is a spike that reports
# success when the answer is no. The two counts measure opposite failures —
# nothing is absent vs. something is present twice — and a libc can fail either
# way, so both are printed unconditionally, even when they are zero.
DUPES="/tmp/make_dupes-$SLATE_LANE.txt"
grep -oP "duplicate symbol: \K.*" slate-link.log | sort -u >"$DUPES"
echo "DUPLICATE_COUNT=$(wc -l <"$DUPES")"
cat "$DUPES"

grep -v "undefined symbol\|duplicate symbol\|^>>>" slate-link.log | head -20

if [ -x make-slateos ]; then
    file make-slateos
    readelf -h make-slateos | grep -E "Type|Entry"
    # Stage it where the rootfs build looks, exactly as the bash and pkgconf
    # spikes do. Leaving the only copy in /tmp is why pkgconf was "proven to
    # work" for two days without ever being in an image.
    cp make-slateos "$SLATE_SPIKE/make-slateos.elf"
    ls -l "$SLATE_SPIKE/make-slateos.elf"
    echo "SLATE_MAKE_BUILT"
else
    echo "NO_SLATE_BINARY"
fi
