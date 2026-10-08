#!/bin/bash
# Cross-compile GCC's compilers (gcc, g++, cc1, cc1plus, ...) and link them against SlateOS's libc.a.
#
# The "try the port before you write a line" step from roadmap-detailed.md's
# "Porting vs. Reimplementing" policy, applied to the last quarter of
# roadmap.md's "gcc, cmake, make, pkg-config via the POSIX layer": make,
# pkgconf and cmake are on the image and run, and gcc was "the one unmeasured
# quarter", waiting only on its mathematical libraries -- GMP and MPFR, which
# scripts/gdb-spike/ builds against our libc, and MPC. Shaped like the other
# spikes, down to the names of the numbers it prints.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# One question: do GCC 16.2's programs -- the drivers gcc and g++, the
# preprocessor, the compilers proper cc1 and cc1plus, and collect2 --
# resolve every symbol they need against `toolchain/sysroot/lib/libc.a`?
#
# Not whether they can build a program on SlateOS. That needs three more
# things, each a step of its own: an assembler and a linker there (GNU
# binutils, or LLVM's -- ld.lld is on the image already); GCC's target
# libraries, libgcc and libstdc++, built by this compiler for SlateOS; and the
# C library's headers and libc.a installed where gcc looks. `make all-gcc`
# builds the compilers and none of the target libraries, which is what
# answers the question here.
#
# Run it from WSL. The work tree is durable ($SLATE_WORK, not /tmp, which WSL
# empties when the distribution idles).
set -uo pipefail
set -x

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1
. "$HERE/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/gcc-spike"
SPIKE_LIBS="$WORK/libs"
JOBS="${SLATE_SPIKE_JOBS:-8}"
VER="$SLATE_GCC_VERSION"

# GMP preprocesses its assembly with m4 on this machine, and the Ubuntu image
# the spikes run in does not ship it; `apt-get download` fetches Ubuntu's own
# package without root, as scripts/gdb-spike/run.sh does (its comment says
# more).
ensure_host_m4() {
    command -v m4 >/dev/null && return 0
    local dir="$SLATE_ZIG_CACHE/host-tools/m4" tmp
    if [ ! -x "$dir/usr/bin/m4" ]; then
        tmp="$(mktemp -d "$SLATE_ZIG_CACHE/m4dl.XXXXXX")" || return 1
        if ! (cd "$tmp" && apt-get download m4) || ! mkdir -p "$dir" \
            || ! dpkg -x "$tmp"/m4_*.deb "$dir"; then
            rm -rf "$tmp"
            echo "gcc-spike: could not fetch Ubuntu's m4 package; GMP needs m4" >&2
            return 1
        fi
        rm -rf "$tmp"
    fi
    export PATH="$dir/usr/bin:$PATH"
}

slate_make_zig_wrappers || exit 1
ensure_host_m4 || exit 1
slate_ensure_gcc_src || exit 1
slate_ensure_gmp_src || exit 1
slate_ensure_mpfr_src || exit 1
slate_ensure_mpc_src || exit 1

mkdir -p "$WORK" "$SPIKE_LIBS" && cd "$WORK" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
# The host programs -- GCC's compilers -- through the wrappers, so every
# configure test and link is answered by our libc.a. The build machine's
# own compilers stay the host's gcc and g++: GCC's build makes generator
# programs (genattrtab, gengtype, ...) that run here, during the build.
export CC="$SLATE_LINK_CC" CXX="$SLATE_LINK_CXX" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"
export CC_FOR_BUILD=gcc CXX_FOR_BUILD=g++

rm -rf "gcc-$VER" build
tar xf "$SLATE_GCC_TARBALL" || exit 1
# GMP, MPFR and MPC inside GCC's tree, under the names its build looks for:
# it builds them for the host, through the same wrappers, as part of
# all-gcc -- which is contrib/download_prerequisites' arrangement, with the
# pinned tarballs in place of its downloads.
for lib in "gmp:$SLATE_GMP_TARBALL:gmp-$SLATE_GMP_VERSION" \
           "mpfr:$SLATE_MPFR_TARBALL:mpfr-$SLATE_MPFR_VERSION" \
           "mpc:$SLATE_MPC_TARBALL:mpc-$SLATE_MPC_VERSION"; do
    name="${lib%%:*}"
    rest="${lib#*:}"
    tarball="${rest%%:*}"
    dir="${rest#*:}"
    tar xf "$tarball" -C "gcc-$VER" || exit 1
    mv "gcc-$VER/$dir" "gcc-$VER/$name" || exit 1
done

mkdir build && cd build || exit 1
# --build, --host and --target: the compilers run on SlateOS and make code for
# it, and are built here, so configure runs nothing they are linked into.
#
# What is left out, each for a reason:
#   --disable-bootstrap: a cross build cannot run the compiler it builds to
#       build itself again.
#   --enable-languages=c,c++: the two compilers the tree's ports need; Fortran,
#       Ada, D, Go and the rest are each a step of their own.
#   --disable-multilib: one ABI, x86-64; there is no 32-bit SlateOS.
#   --disable-plugin: GCC plugins are shared objects, and nothing here loads
#       one.
#   --disable-lto: link-time optimisation needs liblto_plugin, a shared
#       object the linker loads, which a static-only link cannot make (the
#       first run stopped there, in all-lto-plugin, which all-gcc builds
#       whenever LTO is on); lto1 and lto-wrapper go with it.
#   --without-isl: the loop optimiser's library, optional, and a port of its
#       own.
#   --disable-nls: message catalogues; the C locale is the only one we have.
"../gcc-$VER/configure" \
    --build=x86_64-pc-linux-gnu --host=x86_64-linux-musl --target=x86_64-linux-musl \
    --prefix=/usr --disable-bootstrap --disable-multilib --disable-nls \
    --disable-werror --enable-languages=c,c++ --disable-plugin --disable-lto \
    --without-isl \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 conf.log

# -k: one failing object must not hide the rest.
make -k -j"$JOBS" all-gcc >make.log 2>&1
echo "MAKE_EXIT=$?"
grep -E ' error: |Error [0-9]' make.log | grep -v 'undefined symbol' | head -30

# The decisive step: each program's link on its own, so its counts can be
# read. The build above linked through the same wrapper; the objects exist
# now, so make does only the link again.
built=0
tried=0
link_one() {
    local name="$1" bin="$2" log="$WORK/$1-link.log"
    tried=$((tried + 1))
    # Removed first: make relinks only what is missing or older than its
    # inputs, and libc.a is not among them.
    rm -f "gcc/$bin"
    make -C gcc "$bin" >"$log" 2>&1
    echo "${name}_LINK_EXIT=$?"
    # Reached means the linker ran: it made the program, or wrote why not. A
    # link make never reached has no undefined symbols to count, and a 0
    # beside it would read as "nothing is missing" (scripts/gdb-spike/
    # slatelink.sh learned that).
    if [ ! -x "gcc/$bin" ] && ! grep -q "ld\.lld" "$log"; then
        echo "${name}_LINK_NOT_REACHED -- the build of its objects failed:"
        grep -E ' error: |Error [0-9]|No rule to make' "$log" | head -10
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    grep -oP "undefined symbol: \K.*" "$log" | sort -u >"$WORK/$name-missing.txt"
    echo "${name}_MISSING_COUNT=$(wc -l <"$WORK/$name-missing.txt")"
    head -40 "$WORK/$name-missing.txt"
    # Printed even at zero: a symbol defined twice fails a link as surely as
    # one defined nowhere (make-spike's note).
    grep -oP "duplicate symbol: \K.*" "$log" | sort -u >"$WORK/$name-dupes.txt"
    echo "${name}_DUPLICATE_COUNT=$(wc -l <"$WORK/$name-dupes.txt")"
    head -40 "$WORK/$name-dupes.txt"
    if [ ! -x "gcc/$bin" ]; then
        echo "NO_SLATE_${name}_BINARY"
        return
    fi
    ls -l "gcc/$bin"
    # Ours, not musl's, and marked as a native program: each tool's output
    # goes to a file and is counted there -- not inside $(...), whose command
    # check-shell-callables.py looks for on the Windows host, and not as
    # `... | grep -q`, which pipefail turns into a failure on a match.
    nm "gcc/$bin" >"$WORK/$name-symbols.txt" 2>/dev/null
    readelf -n "gcc/$bin" >"$WORK/$name-notes.txt" 2>/dev/null
    if [ "$(grep -c 'rust_begin_unwind' "$WORK/$name-symbols.txt")" -eq 0 ] \
        || [ "$(grep -c '__syscall_cp' "$WORK/$name-symbols.txt")" -ne 0 ]; then
        echo "${name}_NOT_OUR_LIBC -- the link did not take this library's libc.a"
        return
    fi
    if [ "$(grep -c 'SlateOS' "$WORK/$name-notes.txt")" -eq 0 ]; then
        echo "${name}_NO_SLATEOS_NOTE -- the kernel would run it as a Linux program"
        return
    fi
    echo "SLATE_${name}_BUILT"
    built=$((built + 1))
}
link_one GCC xgcc
link_one GXX xg++
link_one CPP cpp
link_one CC1 cc1
link_one CC1PLUS cc1plus
link_one COLLECT2 collect2
echo "PROGRAMS_BUILT=$built of $tried"
[ "$built" -eq "$tried" ] || exit 1
