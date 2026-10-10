#!/bin/bash
# Link LLVM's opt, llc and ld.lld for SlateOS from the objects
# scripts/llvm-spike/run.sh compiled, against the CURRENT libc.a, and stage
# them for the rootfs recipe.
#
# WHY APART FROM run.sh. A cross build of LLVM is hours; a relink is a minute.
# Every change to the C library makes the staged tools older than libc.a, and
# the rootfs recipe refuses a stale port (it would ship programs built against
# a libc that is no longer in the tree). So, as for bash, CPython and eSpeak
# NG, the recipe runs this whenever libc.a is newer than the tools
# (spike_rebuild_if_behind), and run.sh ends by running it too: linking and
# staging have one home.
#
# A tool is linked again when it is missing or older than libc.a: its file is
# removed and cmake's own rule for it runs, through the compilers the build
# was configured with -- scripts/lib/worktree.sh's link wrappers, which link
# with ld.lld against our libc and nothing else.
#
# Then each is stripped of debug information but not of symbols (a fault
# address on the console becomes a function name, as for CPython,
# design-decisions §344) and staged as $SLATE_SPIKE/<name>-slateos.elf, named
# after the program as the image installs it: /bin/opt, /bin/llc, /bin/ld.lld.
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

WORK="$SLATE_WORK/llvm-spike"
LIBC="$SLATE_SYSROOT/libc.a"
JOBS="${SLATE_JOBS:-$(nproc 2>/dev/null || echo 4)}"

if [ ! -f "$WORK/bld/CMakeCache.txt" ]; then
    echo "NO_BUILD_DIR -- $WORK/bld does not exist; run scripts/llvm-spike/run.sh first."
    exit 1
fi
cd "$WORK" || exit 1

# The wrappers' paths are the ones the build was configured with; writing them
# again keeps them pointing at this machine's zig and C++ runtime.
slate_make_link_wrappers "$WORK/bin" || exit 1
if ! grep -q "^CMAKE_CXX_COMPILER:.*=$SLATE_LINK_CXX\$" bld/CMakeCache.txt; then
    echo "OLD_BUILD_DIR -- bld was configured with another compiler; run scripts/llvm-spike/run.sh."
    exit 1
fi

RELINK=0
for tool in opt llc lld; do
    if [ ! -e "bld/bin/$tool" ] || [ "$LIBC" -nt "bld/bin/$tool" ]; then
        rm -f "bld/bin/$tool"
        RELINK=$((RELINK + 1))
    fi
done
echo "TOOLS_TO_LINK=$RELINK"

cmake --build bld -j "$JOBS" --target opt llc lld >link.log 2>&1
echo "LINK_EXIT=$?"
grep -iE "error|warning: " link.log | head -20
# Counted apart, as the CMake port's are: something our libc lacks and
# something present twice are opposite failures, and either alone stops a
# link (scripts/cmake-spike/run.sh says how one hid behind the other).
echo "MISSING_COUNT=$(grep -oP "undefined symbol: \K.*" link.log | sort -u | wc -l)"
grep -oP "undefined symbol: \K.*" link.log | sort -u | head -40
echo "DUPLICATE_COUNT=$(grep -oP "duplicate symbol: \K.*" link.log | sort -u | wc -l)"
grep -oP "duplicate symbol: \K.*" link.log | sort -u | head -20

mkdir -p "$SLATE_SPIKE" || exit 1
STAGED=0
for tool in opt llc lld; do
    if [ ! -x "bld/bin/$tool" ]; then
        echo "NO_$tool -- see $WORK/link.log"
        continue
    fi
    name="$tool"
    [ "$tool" = lld ] && name=ld.lld
    out="$SLATE_SPIKE/$name-slateos.elf"
    readelf -h "bld/bin/$tool" | grep -E "Type|Entry"
    strip --strip-debug -o "$out" "bld/bin/$tool" \
        || "$SLATE_ZIG" objcopy --strip-debug "bld/bin/$tool" "$out" \
        || { echo "STRIP_FAILED $tool"; continue; }
    # The kernel runs a binary on its native system calls only if it says it
    # is native: the SlateOS note our libc puts beside _start (posix/src/
    # crt.rs; design-decisions §33). Without it, LLVM's objects can carry
    # OSABI GNU, and the tool would be run on the Linux table and die at its
    # first call.
    if ! readelf -n "$out" 2>/dev/null | grep -q "SlateOS"; then
        echo "NO_SLATEOS_NOTE $tool -- the kernel would not run it as a native program"
        rm -f "$out"
        continue
    fi
    echo "STAGED $out $(stat -c %s "$out") (unstripped $(stat -c %s "bld/bin/$tool"))"
    STAGED=$((STAGED + 1))
done
echo "LLVM_TOOLS_STAGED=$STAGED"
[ "$STAGED" -eq 3 ] || exit 1
echo "SLATE_LLVM_BUILT"
