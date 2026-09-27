#!/bin/bash
# Cross-compile eSpeak NG and link it against SlateOS's own libc.a.
#
# The "try the port before you write a line" step from roadmap-detailed.md's
# "Porting vs. Reimplementing" policy, applied to the speech-output half of the
# roadmap's `[E] Speech input / speech output` (§5.6). eSpeak NG is the mature
# implementation that policy points at: the formant synthesizer every Linux
# screen reader speaks through (Orca, via speech-dispatcher), ~110 languages,
# ~2 MB of data for English. It is GPL-3.0-or-later, like the GNU bash, make
# and coreutils this tree already ships, and it is built here as its own
# program, so the licence stays with it.
#
# Modelled on scripts/make-spike/run.sh and scripts/cmake-spike/run.sh, down to
# the variable names, so a reader of one can read the others.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# It answers: does eSpeak NG, unmodified, resolve every symbol it needs against
# `toolchain/sysroot/lib/libc.a`? MISSING_COUNT=0 is permission to stage it and
# build a ring-3 rung, not proof that it speaks: that needs the binary run on
# SlateOS, writing a WAV file, which is the rung's job.
#
# It does not touch audio. The build is configured without pcaudiolib, so the
# program can write a WAV file (`-w FILE`) or raw samples to stdout
# (`--stdout`) and cannot open a sound device at all. Playing speech is a
# separate step that needs a userspace audio path on SlateOS; see
# scripts/espeak-spike/README.md.
#
# HOW THE DATA IS BUILT
#
# eSpeak's build compiles its phoneme tables and dictionaries by *running the
# espeak-ng it has just built*. Built with `zig cc --target=x86_64-linux-musl`
# that binary is a static Linux executable, which runs under WSL, so the data
# step works as it would natively. The data is architecture-specific binary
# (little-endian x86-64), which SlateOS is. The program is then relinked
# `-nostdlib` against our libc.a, which is the part that answers the question.
#
# Run it from WSL, with `wsl -d Ubuntu --exec bash scripts/espeak-spike/run.sh`.
# Everything but the sysroot lives under $SLATE_WORK (durable; /tmp is not).
set -uo pipefail
set -x

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="$SLATE_ESPEAK_VERSION"
SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/espeak-spike"
SPIKE_LIBS="$SLATE_TMP/espeak-sysroot"
JOBS="$(nproc 2>/dev/null || echo 4)"

slate_make_zig_wrappers || exit 1
slate_ensure_espeak_src || exit 1

HOST_CMAKE="$(command -v cmake)"
if [ -z "$HOST_CMAKE" ]; then
    echo "NO_HOST_CMAKE — install cmake in WSL; eSpeak NG 1.52 builds with CMake."
    exit 1
fi
"$HOST_CMAKE" --version | head -1

mkdir -p "$WORK" && cd "$WORK" || exit 1
rm -rf "espeak-ng-$VER" bld
tar xzf "$SLATE_ESPEAK_TARBALL" || exit 1

# The options, each for a reason:
#   USE_LIBPCAUDIO=OFF  no sound device: WAV/stdout only (see above).
#   USE_ASYNC=OFF       the asynchronous API is a thread feeding pcaudiolib;
#                       without an audio device there is nothing for it to feed.
#   USE_MBROLA=OFF      MBROLA is a separate, non-free diphone synthesizer.
#   USE_LIBSONIC=OFF    sonic speeds up speech past ~450 wpm; an optional
#                       dependency this tree does not have.
#   USE_SPEECHPLAYER=OFF  speechPlayer is C++ and optional; the formant (Klatt)
#                       synthesizer below it is the one eSpeak speaks with.
#   CMAKE_INSTALL_PREFIX=/usr  compiles in /usr/share/espeak-ng-data as the
#                       data path, which is where the image stages it.
"$HOST_CMAKE" -S "espeak-ng-$VER" -B bld \
    -DCMAKE_C_COMPILER="$SLATE_CC" \
    -DCMAKE_CXX_COMPILER="$SLATE_CXX" \
    -DCMAKE_AR="$SLATE_AR" \
    -DCMAKE_RANLIB="$SLATE_RANLIB" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr \
    -DBUILD_SHARED_LIBS=OFF \
    -DUSE_LIBPCAUDIO=OFF \
    -DUSE_ASYNC=OFF \
    -DUSE_MBROLA=OFF \
    -DUSE_LIBSONIC=OFF \
    -DUSE_SPEECHPLAYER=OFF \
    -DESPEAK_BUILD_MANPAGES=OFF \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -15 conf.log

"$HOST_CMAKE" --build bld -j "$JOBS" >build.log 2>&1
echo "BUILD_EXIT=$?"
grep -iE "error" build.log | head -20

# The data the build produced, by the binary it built. Recorded in the log
# because the tree under $WORK is rebuilt on every run.
DATA="bld/espeak-ng-data"
if [ ! -f "$DATA/phondata" ] || [ ! -f "$DATA/en_dict" ]; then
    echo "NO_DATA — the build did not compile the phoneme data or the English"
    echo "          dictionary; see $WORK/build.log"
    exit 1
fi
echo "DATA_BYTES_ALL=$(du -sb "$DATA" | cut -f1)"

# The decisive step. -nostdlib so we get SlateOS's libc, not zig's bundled
# musl. libc.a twice: it is Rust-built and its intra-archive references are not
# topologically ordered, so a second pass is cheaper than --start-group.
# libstubs.a is deliberately not linked — it and libc.a each carry a panic
# handler and collide on __rustc::rust_begin_unwind.
mkdir -p "$SPIKE_LIBS"
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1

# The program's own objects, and the static libraries the build made for it —
# taken from the build tree rather than guessed, so a library the real link
# uses cannot be missed and one it does not use cannot be swept in.
MAIN_OBJ="$(find bld -path '*espeak-ng-bin.dir*' -name '*.o' | sort | tr '\n' ' ')"
LIBS="$(find bld -name '*.a' | sort | tr '\n' ' ')"
echo "MAIN_OBJ_COUNT=$(echo "$MAIN_OBJ" | wc -w)"
echo "ARCHIVES=$LIBS"
if [ -z "$MAIN_OBJ" ] || [ -z "$LIBS" ]; then
    echo "NO_OBJECTS — the build above failed; see $WORK/build.log"
    exit 1
fi

# Unquoted on purpose: MAIN_OBJ and LIBS are space-separated lists of paths
# under bld/, which CMake names without spaces, and each must reach the linker
# as its own argument.
# shellcheck disable=SC2086
"$SLATE_CC" -static -nostdlib -o espeak-ng-slateos $MAIN_OBJ $LIBS $LIBS \
    "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libc.a" "$SPIKE_LIBS/libunwind.a" \
    2>slate-link.log
echo "SLATE_LINK_EXIT=$?"

MISSING="$SLATE_TMP/espeak_missing.txt"
grep -oP "undefined symbol: \K.*" slate-link.log | sort -u >"$MISSING"
echo "MISSING_COUNT=$(wc -l <"$MISSING")"
head -60 "$MISSING"

DUPES="$SLATE_TMP/espeak_dupes.txt"
grep -oP "duplicate symbol: \K.*" slate-link.log | sort -u >"$DUPES"
echo "DUPLICATE_COUNT=$(wc -l <"$DUPES")"
head -20 "$DUPES"

if [ ! -x espeak-ng-slateos ] || [ "$(wc -l <"$MISSING")" -ne 0 ]; then
    echo "NOT_PUBLISHED — the link did not produce a complete program; nothing"
    echo "                was copied to $SLATE_SPIKE."
    exit 1
fi
file espeak-ng-slateos 2>/dev/null
ls -la espeak-ng-slateos

# Published where scripts/create-ext4-rootfs.sh looks for spike artifacts, the
# way bash, pkgconf and make are: the program as `espeak-ng-slateos.elf`, and
# the data it was built with beside it. The two are one artifact -- the data is
# compiled by *this* build of eSpeak, and a data file from a different version
# is not guaranteed to load -- so they are replaced together or not at all.
rm -rf "$SLATE_SPIKE/espeak-ng-data.new"
cp -r "$DATA" "$SLATE_SPIKE/espeak-ng-data.new" || exit 1
cp espeak-ng-slateos "$SLATE_SPIKE/espeak-ng-slateos.elf.new" || exit 1
rm -rf "$SLATE_SPIKE/espeak-ng-data"
mv "$SLATE_SPIKE/espeak-ng-data.new" "$SLATE_SPIKE/espeak-ng-data" || exit 1
mv "$SLATE_SPIKE/espeak-ng-slateos.elf.new" "$SLATE_SPIKE/espeak-ng-slateos.elf" || exit 1
echo "SLATE_ESPEAK_BUILT=$SLATE_SPIKE/espeak-ng-slateos.elf"
echo "SLATE_ESPEAK_DATA=$SLATE_SPIKE/espeak-ng-data"
