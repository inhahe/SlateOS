#!/bin/bash
# Cross-compile genuine Oils (oils-for-unix) and link it against SlateOS's libc.a.
#
# design-decisions.md §1043 is the operator's: the real Oils -- OSH and YSH --
# built from upstream's C++ becomes the default shell, and our Rust OSH stays as
# a fallback. This is the first step: does upstream's generated C++, unmodified,
# build with our cross toolchain and resolve every symbol against SlateOS's
# libc? It is shaped like scripts/cmake-spike/run.sh, the C++ port it follows,
# down to the variable names, so a reader of one need not re-learn the other.
#
# THREE THINGS THAT ARE NOT BOILERPLATE
#
# 1. Two compiler wrappers, not one. Oils' build compiles every source with
#    -std=c++11, its one .c file (cpp/fanos_shared.c) included, because g++
#    compiles a .c file as C++. zig's driver, like clang, compiles it as C and
#    refuses the flag. `-x c++` restores g++'s reading -- but on compile lines
#    only, since on a link line it would turn the objects into source. The
#    configure wrapper always passes it, because configure's probes are .c
#    files compiled and linked in one step, and upstream runs them under g++:
#    compiled as C, `FNM_EXTMATCH` (which needs _GNU_SOURCE, predefined only for
#    C++) reads as missing and Oils would be built without extended globs,
#    which SlateOS's libc has. The script refuses rather than build that.
#
# 2. The posix header overlay is in front of zig's musl headers, as for every C
#    and C++ fixture here: it is what declares what SlateOS's libc has and
#    musl's does not -- FNM_EXTMATCH among them. So the musl-linked binary
#    `_build/oils.sh` leaves behind does NOT match extended globs (musl's
#    fnmatch ignores the flag); only the SlateOS link does. Do not test one and
#    conclude about the other.
#
# 3. --eh-frame-hdr. The generated C++ raises and catches exceptions for every
#    shell error. libunwind finds a static program's unwind tables through the
#    PT_GNU_EH_FRAME segment, which only that flag makes; without it the link
#    succeeds and every `throw` terminates the shell (services/ctest-cxx-throw
#    is the fixture that proves the chain). The link wrapper passes it, and
#    slatelink.sh checks the segment is there.
#
# THE LINK IS slatelink.sh'S. This builds the objects -- the minutes -- and
# then runs it: the link against SlateOS's libc.a, its checks, and the
# stripped copy in build/spike/. The rootfs recipe runs slatelink.sh alone to
# relink these objects whenever libc.a moves, as it does bash's and CPython's.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# MISSING_COUNT=0 and DUPLICATE_COUNT=0 mean the libc covers every symbol Oils
# names. They do not mean Oils runs: that needs the image, and a rung. Built
# --without-readline: SlateOS has no GNU readline yet, so the shell has no line
# editing or history at its prompt (it reads plain lines); see README.md.
#
# Run it from WSL, with a current sysroot (toolchain/build-sysroot.ps1).
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="0.38.0"
# From https://oils.pub/release/0.38.0/, checked against the download.
SHA256="a33453722819b55ee552bfd7f3c2bab8f1940def55d5c8b46af16ce95bdf8803"
URL="https://oils.pub/download/oils-for-unix-$VER.tar.gz"
SYSROOT="$SLATE_SYSROOT"
# Durable, not /tmp -- see worktree.sh ($SLATE_WORK).
WORK="$SLATE_WORK/oils-spike"
TARBALL="$SLATE_WORK/oils-for-unix-$VER.tar.gz"
OVERLAY="$SLATE_ROOT/posix/include"

slate_make_zig_wrappers || exit 1

mkdir -p "$WORK" || exit 1

if [ ! -f "$TARBALL" ] || ! echo "$SHA256  $TARBALL" | sha256sum -c --quiet 2>/dev/null; then
    curl -sSfL -o "$TARBALL.part" "$URL" || { echo "NO_TARBALL -- could not fetch $URL"; exit 1; }
    mv "$TARBALL.part" "$TARBALL"
fi
if ! echo "$SHA256  $TARBALL" | sha256sum -c --quiet; then
    echo "TARBALL_HASH_MISMATCH -- $TARBALL is not the release that was measured"
    exit 1
fi

# A libc.a older than posix/src measures yesterday's libc, and a symbol added
# today reads as missing. Same instrument as the rootfs recipe's sysroot check.
if [ ! -f "$SYSROOT/libc.a" ]; then
    echo "NO_SYSROOT -- $SYSROOT/libc.a is missing; run toolchain/build-sysroot.ps1"
    exit 1
fi
NEWER="$(find "$SLATE_ROOT/posix/src" "$OVERLAY" -type f -newer "$SYSROOT/libc.a" -print -quit)"
if [ -n "$NEWER" ]; then
    echo "STALE_SYSROOT -- $NEWER is newer than libc.a; run toolchain/build-sysroot.ps1"
    exit 1
fi

# The wrappers. Their names become directory names in Oils' build
# (_build/obj/slatecxx-opt-sh), so they are found on PATH, not by path.
BIN="$WORK/bin"
mkdir -p "$BIN" || exit 1
printf '#!/bin/sh\nexec "%s" c++ --target=x86_64-linux-musl -x c++ -I"%s" "$@"\n' \
    "$SLATE_ZIG" "$OVERLAY" >"$BIN/slateconf"
printf '#!/bin/sh\ncase " $* " in\n  *" -c "*) exec "%s" c++ --target=x86_64-linux-musl -x c++ -I"%s" "$@" ;;\n  *) exec "%s" c++ --target=x86_64-linux-musl -I"%s" "$@" ;;\nesac\n' \
    "$SLATE_ZIG" "$OVERLAY" "$SLATE_ZIG" "$OVERLAY" >"$BIN/slatecxx"
chmod +x "$BIN/slateconf" "$BIN/slatecxx"
export PATH="$BIN:$PATH"

cd "$WORK" || exit 1
rm -rf "oils-for-unix-$VER"
tar xzf "$TARBALL" || exit 1
cd "oils-for-unix-$VER" || exit 1

./configure --cxx-for-configure slateconf --without-readline --prefix /usr >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
grep '^#define HAVE_' _build/detected-cpp-config.h
# SlateOS's libc has all three. A 0 here is a probe that went wrong (point 1
# above), and building on it would ship a shell missing a feature the system
# has.
for feature in HAVE_FNM_EXTMATCH HAVE_GLOB_PERIOD HAVE_PWENT; do
    if ! grep -q "^#define $feature 1" _build/detected-cpp-config.h; then
        echo "PROBE_WRONG -- configure says no $feature, which SlateOS's libc has; see conf.log"
        exit 1
    fi
done

_build/oils.sh --cxx slatecxx --without-readline >build.log 2>&1
echo "BUILD_EXIT=$?"
grep -E "error:" build.log | head -20

if [ -z "$(find _build/obj/slatecxx-opt-sh -name '*.o' -print -quit 2>/dev/null)" ]; then
    echo "NO_OBJECTS -- the build above failed; see $WORK/oils-for-unix-$VER/build.log"
    exit 1
fi

# The link, its checks and the stripped copy in build/spike/: slatelink.sh's,
# which the rootfs recipe also runs alone whenever libc.a moves.
bash "$SLATE_ROOT/scripts/oils-spike/slatelink.sh" || exit 1
echo "SLATE_OILS_BUILT $VER"
