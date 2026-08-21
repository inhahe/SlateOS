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
rm -rf "$OUT"

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

# SLATE_RELINK_ONLY=1 reuses an existing build tree and re-runs only the relink
# loop below. That loop is the part that gets iterated — after a libc change, or
# after fixing a defect in the harvest itself — and re-running ./configure to
# get there costs ~25 minutes of gnulib's locale probes to reproduce bytes that
# did not change.
#
# Guarded on make.log existing rather than on the directory existing, so it can
# never quietly "reuse" a tree whose build failed or never ran. If the guard
# fails we fall through to a full rebuild rather than erroring: a stale flag in
# the environment should cost time, not correctness.
if [ "${SLATE_RELINK_ONLY:-0}" = 1 ] && [ -f "coreutils-$VER/make.log" ]; then
    cd "coreutils-$VER" || exit 1
    echo "RELINK_ONLY=1 — reusing the existing build in $PWD"
else
    rm -rf "coreutils-$VER"
    tar xf "$SLATE_COREUTILS_TARBALL" || exit 1
    cd "coreutils-$VER" || exit 1

    # --disable-shared: SlateOS has no dynamic loader on this path, so
    # everything is a static ET_EXEC.
    #
    # --disable-nls: gettext is not a libc facility, and letting configure find
    # the host's libintl would link a host library into a cross binary. This
    # narrows what is tested and the narrowing is deliberate — the question is
    # about our libc, not about message catalogues.
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

    # V=1 defeats automake's silent rules. This is not cosmetic: the whole
    # relink step below works by reading the real link command lines out of this
    # log, and with silent rules they are never printed. A run that forgets V=1
    # finds zero link lines and reports LINK_LINES=0, which is why that is a
    # hard failure rather than an empty loop that exits 0.
    make -j8 V=1 >make.log 2>&1
    echo "MAKE_EXIT=$?"
    grep -iE "^[^ ]*error|warning: implicit" make.log | head -20
fi

# Which gnulib replacement modules configure decided to compile in. This is the
# diagnostic that ties the run to §340: a gnulib .o here whose name matches one
# of the seventeen functions that used to share an archive member is a live test
# of that fix, and its ABSENCE would mean this run did not exercise §340 at all
# and must not be read as confirming it.
#
# automake uses two names for these depending on whether the module needs
# per-target flags — `lib/foo.o` and `lib/libcoreutils_a-foo.o`. Probing only
# the first form finds six of the vendored modules and misses four, which is a
# diagnostic that silently understates exactly the thing it exists to measure.
echo "GNULIB_OBJECTS_BUILT=$(ls lib/*.o 2>/dev/null | wc -l)"
echo "GNULIB_REPLACEMENTS_RELEVANT_TO_S340:"
for f in asprintf vasprintf canonicalize_file_name getline getdelim fseeko \
         ftello strndup strverscmp stpcpy stpncpy mempcpy strchrnul memrchr \
         rawmemchr strcasestr strnlen getopt glob fnmatch error; do
    if [ -f "lib/$f.o" ]; then
        echo "  lib/$f.o"
    elif [ -f "lib/libcoreutils_a-$f.o" ]; then
        echo "  lib/libcoreutils_a-$f.o"
    fi
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
    # Drop the system-library flags configure detected, BEFORE appending ours.
    #
    # This is not tidying — leaving them in silently links a second, complete
    # libc into a -nostdlib link. `zig cc --target=x86_64-linux-musl` resolves
    # `-lpthread` against its own bundled musl, and musl folds pthread into
    # libc.a, so that one flag puts all of musl on the line. Our libc.a is a
    # single archive that already provides pthread/rt/dl/m/crypt, so nothing is
    # lost by removing them.
    #
    # Measured, not assumed: `sort` is the only utility configure gave
    # -lpthread, and it was the only one of the 107 to report musl's stdio
    # colliding with ours — 11 duplicate symbols (fflush, fread, fwrite, feof,
    # ferror, clearerr, fputs, __fpurge, putc_unlocked, __progname,
    # __progname_full) that no other binary saw. Every one was musl's member
    # being extracted for an `X_unlocked` name our libc lacks, dragging its `X`
    # in behind it. Those 11 were an artifact of this script, not a fact about
    # our libc, and reporting them as a libc defect would have been wrong.
    #
    # The `:a … ta` loop re-runs the substitution until it stops matching, so
    # two adjacent flags (`-lpthread -lrt`) cannot hide one another by
    # consuming the shared separating space.
    cmd="$(printf '%s' "$cmd" \
        | sed -E ':a; s/ -l(pthread|rt|dl|m|crypt|c)( |$)/ /; ta')"
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

# Assert the thing the -l stripping above is supposed to guarantee: that no link
# consulted zig's bundled musl. If one did, then every symbol our libc lacks was
# available from a second libc, and MISSING_COUNT below is an undercount of
# unknown size — the run would be measuring "our libc plus musl", which is not a
# question anyone asked.
#
# This is checked rather than assumed because the failure is silent: musl
# quietly satisfying a missing symbol produces no diagnostic at all, and only
# became visible here because a few of its members happened to also collide.
FOREIGN="$(grep -l "cache/zig/o" "$WORK"/link-*.log 2>/dev/null | wc -l)"
echo "LINKS_THAT_PULLED_ZIG_MUSL=$FOREIGN"
if [ "$FOREIGN" -gt 0 ]; then
    echo "WARNING — $FOREIGN link(s) resolved symbols against zig's bundled musl."
    echo "          MISSING_COUNT is therefore a LOWER BOUND, not a measurement."
    echo "          Offending binaries:"
    grep -l "cache/zig/o" "$WORK"/link-*.log | sed 's|.*/link-|            |; s|\.log$||'
fi

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
