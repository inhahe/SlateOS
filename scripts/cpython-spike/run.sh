#!/bin/bash
# Cross-compile upstream CPython and build it far enough to link against
# SlateOS's own libc.a.
#
# This is "try the port before you write a line" (roadmap-detailed.md, "Porting
# vs. Reimplementing"; design-decisions.md §307) applied to the third and by far
# the largest real-world C program we have pointed at our libc -- after GNU bash
# (§305, shipped) and pkgconf 2.3.0 (shipped, zero missing symbols).
#
# WHAT THIS SPIKE IS FOR
#
# Not a working interpreter. The deliverable is a *number*: how many libc
# symbols CPython needs that we do not have, and which ones. roadmap.md's
# "Enough of POSIX libc for: gcc, coreutils, bash, Python (CPython)" has been
# open with no measurement behind it; bash needed three shims and pkgconf needed
# zero, and neither tells us much about CPython, which wants threads, dynamic
# loading, locales and a far wider syscall surface than either.
#
# A missing-symbol list is worth more than a yes/no: it is directly actionable
# work for posix/src, and it is honest in a way "we should port CPython
# eventually" is not.
#
# WHY 3.12
#
# CPython 3.11+ needs a *host* interpreter of the same major.minor to run build
# steps (deepfreeze, sysconfig generation) during a cross build --
# `--with-build-python`. This machine's WSL has 3.12.3, so 3.12 is the version
# that removes an entire class of cross-build failure for free. The spike
# measures libc surface, which does not meaningfully move between patch
# releases, so pinning to the host's minor costs nothing that matters here.
# roadmap.md item 4.4 says "latest"; when this graduates from a spike to a port,
# revisit -- and note that a newer CPython needs a newer build interpreter,
# which is a prerequisite, not a preference.
set -euo pipefail

# Locate the worktree this script lives in -- never a typed-out path. There are
# four checkouts, and a hard-coded one means linking against another lane's
# libc.a, which proves nothing about the tree you are in. See
# scripts/lib/worktree.sh for the incident that motivated this.
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="${CPYTHON_VER:-3.12.3}"
SYSROOT="$SLATE_SYSROOT"
# Scratch keyed by worktree so two lanes building at once cannot write each
# other's objects.
WORK="/tmp/cpython-spike-$SLATE_LANE"

# Everything except the sysroot lives off /mnt/d: the 9p mount is slow, and a
# CPython build is thousands of compiler invocations rather than pkgconf's tens.
mkdir -p "$WORK"
cd "$WORK"

slate_make_zig_wrappers || exit 1

BUILD_PY="${BUILD_PY:-python3.12}"
command -v "$BUILD_PY" >/dev/null 2>&1 || {
    echo "no $BUILD_PY on PATH; CPython $VER cross-build needs a matching host interpreter" >&2
    exit 1
}
echo "BUILD_PYTHON=$("$BUILD_PY" -V 2>&1)"

TARBALL="Python-$VER.tar.xz"
[ -f "$TARBALL" ] || curl -sSLO "https://www.python.org/ftp/python/$VER/$TARBALL"
[ -d "Python-$VER" ] || tar xf "$TARBALL"
cd "Python-$VER"

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

# --disable-shared: SlateOS has no dynamic loader on this path, so the target is
#   a static ET_EXEC, same as bash and pkgconf.
# --without-ensurepip / --disable-test-modules: pip and the test suite are
#   megabytes of Python source that cannot affect which libc symbols the
#   interpreter core references.
# ac_cv_file__dev_pt*: configure cannot stat files on the target, and left to
#   guess it errors out. We do intend to have a pty layer; saying "no" here only
#   keeps configure from probing a filesystem that does not exist yet.
# ac_cv_buggy_getaddrinfo=no: configure detects the well-known broken-getaddrinfo
#   bug by *running* a test program. It cannot run a target binary in a cross
#   build, so it assumes the bug is present and hard-errors ("You must get
#   working getaddrinfo()"). Asserting "not buggy" is the correct cross answer
#   and is what distro cross-recipes do; the alternative configure suggests,
#   --disable-ipv6, would silently compile a *different* interpreter with a
#   smaller socket surface -- which is the opposite of what a spike measuring
#   our libc surface wants. If our getaddrinfo turns out to be genuinely buggy
#   that is a posix/src bug to fix, not a reason to build less of CPython.
if [ ! -f config.status ]; then
    ./configure \
        --host=x86_64-linux-musl \
        --build=x86_64-pc-linux-gnu \
        --with-build-python="$BUILD_PY" \
        --disable-shared \
        --without-ensurepip \
        --disable-test-modules \
        ac_cv_file__dev_ptmx=no \
        ac_cv_file__dev_ptc=no \
        ac_cv_buggy_getaddrinfo=no \
        >conf.log 2>&1 && echo "CONFIGURE_EXIT=0" || {
            echo "CONFIGURE_EXIT=$?"
            echo "--- last 40 lines of conf.log ---"
            tail -40 conf.log
            exit 1
        }
else
    echo "CONFIGURE_EXIT=0 (config.status already present; delete $WORK to redo)"
fi

# Keep going past a failing module: a module that will not build is a data
# point, not a reason to abandon the run. The core interpreter and libpython
# are what the link step needs.
make -j"$(nproc)" -k >make.log 2>&1 && echo "MAKE_EXIT=0" || echo "MAKE_EXIT=$?"

echo "--- build products ---"
ls -l libpython"${VER%.*}".a Programs/python.o 2>/dev/null || echo "  (libpython/python.o absent)"
echo "--- error lines in make.log (first 20) ---"
grep -E "^[^ ]+\.c:[0-9]+:[0-9]+: error|Error [0-9]" make.log | head -20 || echo "  (none)"
echo "--- modules that failed to build ---"
sed -n '/Following modules built successfully/,/^$/p' make.log | head -5 || true
sed -n '/necessary bits to build these .* modules/,/^$/p' make.log | head -10 || true

echo "CPYTHON_SPIKE_BUILD_DONE"
