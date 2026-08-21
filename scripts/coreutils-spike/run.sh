#!/bin/bash
# Cross-compile upstream GNU coreutils and link every one of its ~100 binaries
# against SlateOS's own libc.a.
#
# This is the "try the port before you write a line" step from
# roadmap-detailed.md's "Porting vs. Reimplementing" policy, applied to the
# `coreutils` third of the roadmap's `Enough of POSIX libc for
# gcc/coreutils/bash/CPython` item. It is modelled on scripts/make-spike/run.sh,
# deliberately, down to the variable names.
#
# WHY COREUTILS SPECIFICALLY, AND WHY NOW
#
# design-decisions.md §340 fixed seventeen archive members of libc.a that each
# carried more than one function, so that a caller pulling in `printf` also got
# `asprintf`, and a caller pulling in `memcpy` also got ten string functions.
# That is fatal to any program that vendors gnulib, because gnulib supplies its
# own replacements for exactly those names: the member is extracted for the
# symbol you wanted and the extra symbols in it then collide with gnulib's.
#
# §340's own text names the programs it expected to hit this — "make missed
# them only because its ./configure did not compile in those particular gnulib
# modules; **coreutils and tar** would have hit them". So coreutils is not an
# arbitrary next port. It is the case the fix was written for, and until this
# script ran, that prediction had never been tested against the thing it
# predicted. A fix validated only against the case that did *not* exercise it is
# a fix on probation.
#
# It is also a far better shaped test than make was, for a reason that has
# nothing to do with coreutils being useful: make is ONE link, so it samples the
# duplicate-symbol question once. coreutils is ~100 separate links over one
# shared gnulib archive, each pulling a different subset of libc. That is ~100
# independent samples of "does extracting the member you need drag in a symbol
# you already have", which is the only way to test a property that is about
# *which* members happen to be extracted.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# It answers exactly one question: does real GNU coreutils, unmodified, resolve
# every symbol it needs against `toolchain/sysroot/lib/libc.a`, without
# colliding with it? A link is a complete, mechanical enumeration of a program's
# demands on its libc.
#
# It does NOT answer whether any of these binaries *run*. Same caveat as pkgconf
# and make, and for coreutils it bites harder than for either: `ls` is a VFS
# exerciser, `dd` is an I/O exerciser, `stat` reads out the exact struct we are
# least sure about. Treat MISSING_COUNT=0 as permission to build a rootfs rung,
# not as a port.
#
# Run it from WSL. Everything except the sysroot lives in /tmp, because the
# linker reads the archives heavily and the 9p mount to /mnt/d is slow.
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="$SLATE_COREUTILS_VERSION"
SYSROOT="$SLATE_SYSROOT"
WORK="/tmp/coreutils-spike-$SLATE_LANE"
SPIKE_LIBS="/tmp/slate-sysroot2-$SLATE_LANE"
OUT="$WORK/slateos-bin"

slate_make_zig_wrappers || exit 1
slate_ensure_coreutils_src || exit 1

echo "COREUTILS_VERSION=$VER"
echo "TARBALL=$SLATE_COREUTILS_TARBALL"

mkdir -p "$WORK" && cd "$WORK" || exit 1
rm -rf "coreutils-$VER" "$OUT"
tar xf "$SLATE_COREUTILS_TARBALL" || exit 1
cd "coreutils-$VER" || exit 1

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

# --disable-shared: SlateOS has no dynamic loader on this path, so everything is
# a static ET_EXEC.
#
# --disable-nls: gettext is not a libc facility, and letting configure find the
# host's libintl would link a host library into a cross binary. This narrows
# what is tested and the narrowing is deliberate — the question is about our
# libc, not about message catalogues.
#
# FORCE_UNSAFE_CONFIGURE is not set and must not be: it is coreutils' guard
# against being configured as root, and we are not root.
#
# Nothing else is overridden. The entire point is to let configure probe our
# libc through zig's musl headers and reach its own conclusions, then see
# whether the link agrees.
./configure --host=x86_64-linux-musl --disable-shared --disable-nls \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 conf.log

# V=1 defeats automake's silent rules. This is not cosmetic: the whole relink
# step below works by reading the real link command lines out of this log, and
# with silent rules they are never printed. A run that forgets V=1 finds zero
# link lines and reports LINK_LINES=0, which is why that is a hard failure
# rather than an empty loop that exits 0.
make -j8 V=1 >make.log 2>&1
echo "MAKE_EXIT=$?"
grep -iE "^[^ ]*error|warning: implicit" make.log | head -20

# Which gnulib replacement modules configure decided to compile in. This is the
# diagnostic that ties the run to §340: a gnulib .o here whose name matches one
# of the seventeen functions that used to share an archive member is a live test
# of that fix, and its ABSENCE would mean this run did not exercise §340 at all
# and must not be read as confirming it.
echo "GNULIB_OBJECTS_BUILT=$(ls lib/*.o 2>/dev/null | wc -l)"
echo "GNULIB_REPLACEMENTS_RELEVANT_TO_S340:"
for f in asprintf vasprintf canonicalize_file_name getline getdelim fseeko \
         ftello strndup strverscmp stpcpy stpncpy mempcpy strchrnul memrchr \
         rawmemchr strcasestr strnlen getopt glob fnmatch error; do
    [ -f "lib/$f.o" ] && echo "  lib/$f.o"
done

mkdir -p "$SPIKE_LIBS"
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1

# Take the link lines from coreutils' own build rather than reconstructing them.
# coreutils links each utility from a different set of objects plus a shared
# lib/libcoreutils.a, and several utilities (ls/dir/vdir, and the many that are
# built from one source with different -D) do not follow from their names. A
# glob of src/*.o would link every utility's main() into every binary.
#
# A link line is one that names an output under src/ and is not a compile: -c
# never appears on a link, and `-o src/foo.o` is a compile even without it.
LINKS="$WORK/link-lines.txt"
grep -E '^[^ ]*zigcc[^ ]*' make.log \
    | grep -E ' -o src/[A-Za-z0-9_-]+( |$)' \
    | grep -v ' -c ' >"$LINKS"
echo "LINK_LINES=$(wc -l <"$LINKS")"
if [ ! -s "$LINKS" ]; then
    echo "NO_LINK_LINES — nothing to relink. Either the build above failed (see"
    echo "                $WORK/coreutils-$VER/make.log) or V=1 did not take and"
    echo "                the commands were never printed. Do not read this as"
    echo "                'coreutils needs nothing from our libc'."
    exit 1
fi

mkdir -p "$OUT"
ALL_MISSING="$WORK/missing.txt"
ALL_DUPES="$WORK/dupes.txt"
FAILED="$WORK/failed-binaries.txt"
: >"$ALL_MISSING"
: >"$ALL_DUPES"
: >"$FAILED"

OK=0
BAD=0
while IFS= read -r line; do
    name="$(echo "$line" | grep -oE ' -o src/[A-Za-z0-9_-]+' | head -1 | sed 's|.* -o src/||')"
    [ -n "$name" ] || continue
    # -nostdlib so we get SlateOS's libc, not zig's bundled musl. libc.a twice:
    # it is Rust-built and its intra-archive references are not topologically
    # ordered, so a second pass is cheaper than --start-group. libstubs.a is
    # deliberately not linked — it and libc.a each carry a panic handler and
    # collide on __rustc::rust_begin_unwind.
    cmd="${line/ -o src\/$name/ -o $OUT/$name}"
    cmd="$cmd -nostdlib -static $SPIKE_LIBS/libc.a $SPIKE_LIBS/libc.a $SPIKE_LIBS/libunwind.a"
    eval "$cmd" >"$WORK/link-$name.log" 2>&1
    if [ $? -eq 0 ] && [ -x "$OUT/$name" ]; then
        OK=$((OK + 1))
    else
        BAD=$((BAD + 1))
        echo "$name" >>"$FAILED"
    fi
    grep -oP "undefined symbol: \K.*" "$WORK/link-$name.log" >>"$ALL_MISSING"
    grep -oP "duplicate symbol: \K.*" "$WORK/link-$name.log" >>"$ALL_DUPES"
done <"$LINKS"

echo "BINARIES_LINKED_OK=$OK"
echo "BINARIES_FAILED=$BAD"

# Both counts are printed unconditionally, even when zero, and neither is
# allowed to stand in for "the link succeeded". The make spike's first run
# printed SLATE_LINK_EXIT=1 beside MISSING_COUNT=0 and was briefly read as a
# fluke, because all eleven errors were *duplicate* definitions and the grep
# that produces MISSING_COUNT matches only `undefined symbol:`. The two counts
# measure opposite failures — nothing is absent vs. something is present twice
# — and a libc can fail either way.
sort -u "$ALL_MISSING" -o "$ALL_MISSING"
sort -u "$ALL_DUPES" -o "$ALL_DUPES"
echo "MISSING_COUNT=$(wc -l <"$ALL_MISSING")"
cat "$ALL_MISSING"
echo "DUPLICATE_COUNT=$(wc -l <"$ALL_DUPES")"
cat "$ALL_DUPES"

if [ "$BAD" -gt 0 ]; then
    echo "FAILED_BINARIES:"
    cat "$FAILED"
    echo "FIRST_FAILURE_LOG:"
    head -25 "$WORK/link-$(head -1 "$FAILED").log"
fi

if [ "$OK" -gt 0 ]; then
    sample="$(ls "$OUT" | head -1)"
    file "$OUT/$sample"
    readelf -h "$OUT/$sample" | grep -E "Type|Entry"
    echo "TOTAL_BYTES=$(du -sb "$OUT" | cut -f1)"
    echo "SLATE_COREUTILS_LINKED"
else
    echo "NO_SLATE_BINARIES"
fi
