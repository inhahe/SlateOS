#!/bin/bash
# Build Mono's class libraries on the host, and stage what SlateOS's mono needs.
#
# The class libraries -- mscorlib.dll, System.dll and the rest -- are .NET
# assemblies: bytecode, the same files on every operating system. They are
# compiled by Mono's own C# compiler, which runs on a Mono runtime, so they are
# built here, on Linux, by a native build of the same Mono that run.sh
# cross-compiles for SlateOS. The bootstrap needs no network: the tarball
# carries monolite-linux, a minimal mscorlib and mcs.exe. The runtime built
# here is Linux's, and is staged nowhere; only what it compiles is SlateOS's.
#
# Nothing here links our libc.a, so nothing here goes stale when libc.a moves:
# the rootfs recipe never reruns it. Run it when the Mono version changes.
#
# What it stages, in build/spike/mono/, for scripts/create-ext4-rootfs.sh:
#   lib/mono/4.5/mscorlib.dll   the one assembly every program needs. mono
#       finds it beside itself: /bin/mono looks in ../lib/mono/4.5 (Mono's
#       set_dirs), which holds whether the image is mounted at / or at /mnt.
#   etc/mono/config             Mono's library map, from the same install:
#       it maps DllImport("libc") to libc.so.6, which dlopen answers with
#       the program itself (design-decisions 1184). mono reads it from
#       ../etc/mono beside itself, as it finds ../lib.
#   lib/mono/checks/checks.exe  services/ctest-mono-runs/checks.cs, compiled:
#       what that fixture runs.
#
# Run it from WSL: wsl -d Ubuntu --exec bash scripts/mono-spike/bcl.sh
set -uo pipefail
set -x

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1
. "$HERE/../lib/worktree.sh" || exit 1

VER="$SLATE_MONO_VERSION"
WORK="$SLATE_WORK/mono-bcl"
PREFIX="$WORK/prefix"
JOBS="${SLATE_SPIKE_JOBS:-8}"
STAGE="$SLATE_SPIKE/mono"
CHECKS_CS="$SLATE_ROOT/services/ctest-mono-runs/checks.cs"

slate_ensure_mono_src || exit 1
[ -f "$CHECKS_CS" ] || { echo "NO_CHECKS_SOURCE -- $CHECKS_CS is missing"; exit 1; }
mkdir -p "$WORK" && cd "$WORK" || exit 1

# The native build is half an hour, and what it makes depends on the Mono
# version and nothing else here, so it is made once per version: a stamp in
# the prefix records which. SLATE_MONO_BCL_REBUILD=1 makes it again anyway.
# checks.exe below is compiled on every run, as checks.cs may have changed.
STAMP="$PREFIX/.slate-mono-version"
if [ "${SLATE_MONO_BCL_REBUILD:-0}" != 1 ] && [ -f "$STAMP" ] \
    && [ "$(cat "$STAMP")" = "$VER" ] && [ -f "$PREFIX/lib/mono/4.5/mscorlib.dll" ]; then
    echo "MONO_LIBRARIES_ALREADY_BUILT ($VER, $PREFIX)"
else
    rm -rf "mono-$VER" "$PREFIX"
    tar xf "$SLATE_MONO_TARBALL" || exit 1
    cd "mono-$VER" || exit 1

    # The host's own compilers: this runtime runs on Linux and nowhere
    # else, and configure must not find the spikes' wrappers in the
    # environment.
    unset CC CXX AR RANLIB
    ./configure --prefix="$PREFIX" --disable-nls --disable-boehm --disable-btls \
        --with-mcs-docs=no --with-libgdiplus=no --with-ikvm-native=no \
        >conf.log 2>&1
    echo "CONFIGURE_EXIT=$?"
    tail -20 conf.log

    make -j"$JOBS" >make.log 2>&1
    echo "MAKE_EXIT=$?"
    grep -E 'error CS|Error [0-9]' make.log | head -30

    make install >install.log 2>&1
    echo "INSTALL_EXIT=$?"
    if [ -f "$PREFIX/lib/mono/4.5/mscorlib.dll" ]; then
        printf '%s\n' "$VER" >"$STAMP"
    fi
fi
echo "ASSEMBLIES_4_5=$(find "$PREFIX/lib/mono/4.5" -maxdepth 1 -name '*.dll' | wc -l)"
if [ ! -f "$PREFIX/lib/mono/4.5/mscorlib.dll" ]; then
    echo "NO_MSCORLIB -- the build installed no mscorlib.dll"
    exit 1
fi

# The fixture's program, compiled by this build's own C# compiler, and run
# here first: what it prints on Linux is what the fixture expects on SlateOS.
cd "$WORK" || exit 1
rm -rf checks && mkdir checks || exit 1
"$PREFIX/bin/mcs" -out:checks/checks.exe -optimize+ "$CHECKS_CS" >checks/mcs.log 2>&1
echo "MCS_EXIT=$?"
"$PREFIX/bin/mono" checks/checks.exe >checks/linux.txt 2>&1
echo "CHECKS_ON_LINUX_EXIT=$?"
cat checks/linux.txt

if [ ! -f checks/checks.exe ]; then
    echo "NO_CHECKS_EXE -- mcs did not compile $CHECKS_CS:"
    cat checks/mcs.log
    exit 1
fi
rm -rf "$STAGE" && mkdir -p "$STAGE/lib/mono/4.5" "$STAGE/lib/mono/checks" "$STAGE/etc/mono" || exit 1
cp "$PREFIX/lib/mono/4.5/mscorlib.dll" "$STAGE/lib/mono/4.5/" || exit 1
cp "$PREFIX/etc/mono/config" "$STAGE/etc/mono/" || exit 1
cp checks/checks.exe "$STAGE/lib/mono/checks/" || exit 1
ls -lR "$STAGE"
echo "SLATE_MONO_LIBRARIES_STAGED"
