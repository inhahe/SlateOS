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
# Keyed by worktree so two lanes building at once cannot write each other's
# objects. Durable ($SLATE_WORK), not /tmp — see worktree.sh.
WORK="$SLATE_WORK/cpython-spike"
slate_adopt_legacy_work "/tmp/cpython-spike-$SLATE_LANE" "$WORK"

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

# CONFIGURED AGAINST SLATEOS'S libc.a, NOT ZIG'S MUSL (2026-10-05)
#
# CC is the link wrapper (scripts/lib/worktree.sh, slate_make_link_wrappers):
# it compiles with zig's cc and links with zig's ld.lld against our libc.a
# alone, so every function configure looks for is answered by the library the
# interpreter will run on. Until 2026-10-05 configure linked its checks
# against zig's musl, which has not got close_range, sem_clockwait, getwd or
# tmpnam_r, so CPython used none of them on SlateOS, where all four are there.
# Without close_range, subprocess's close_fds closed nothing: _posixsubprocess
# fell back to listing /proc/self/fd with getdents64, which a native process
# cannot (known-issues
# D-SPIKES-CPYTHON-WAS-CONFIGURED-FOR-MUSL-NOT-FOR-OUR-LIBC).
#
# The interpreter `make` links is therefore SlateOS's, and does not run under
# WSL. What stdlib.sh and the fastpy check run there is python-control, linked
# at the end of this script from the same objects against musl.
SYSCOPY="/tmp/slate-sysroot-cpython-$SLATE_LANE"
mkdir -p "$SYSCOPY"
cp "$SLATE_SYSROOT/libc.a" "$SLATE_SYSROOT/libunwind.a" "$SYSCOPY/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SYSCOPY" || exit 1

# configure's answers to the questions it would answer by RUNNING a test
# program, which a cross build cannot; left alone, it takes each one's
# cross-compiling default. Each is SlateOS's true answer, measured, and each
# is also what Ubuntu 24.04's python3.12, built on glibc 2.39, our library's
# reference, records (sysconfig):
#
#   ac_cv_working_tzset=yes        our tzset is glibc's (posix/src/tz.rs replays
#       glibc 2.39's answers). glibc itself fails configure's test -- for
#       TZ=UTC+0 it sets tzname[1] to "UTC" where the test wants "" -- and
#       Ubuntu's build has time.tzset all the same. The default, no, left the
#       image's Python without time.tzset.
#   --with-computed-gotos          a compiler feature zig's clang has; the
#       default, no, compiled the slower switch dispatch into the eval loop.
#   ac_cv_aligned_required=no      x86-64 needs no aligned access.
#   ac_cv_broken_sem_getvalue=no   our sem_getvalue reads the count
#       (posix/src/semaphore.rs); the default, yes, made
#       multiprocessing's Semaphore.get_value() raise.
#   ac_cv_pthread_system_supported=yes  pthread_attr_setscope takes
#       PTHREAD_SCOPE_SYSTEM and refuses PROCESS with ENOTSUP, as Linux's.
#
# py_cv_module_xxlimited{,_35}=n/a: two test modules for the limited C API,
# built as shared objects whatever MODULE_BUILDTYPE says. A shared object has
# no place on SlateOS, and the link wrapper refuses one, which made `make`
# exit 2 over the pair; --disable-test-modules does not cover them.
CONFIGURE_ANSWERS=(
    ac_cv_file__dev_ptmx=no
    ac_cv_file__dev_ptc=no
    ac_cv_buggy_getaddrinfo=no
    ac_cv_working_tzset=yes
    ac_cv_aligned_required=no
    ac_cv_broken_sem_getvalue=no
    ac_cv_pthread_system_supported=yes
    py_cv_module_xxlimited=n/a
    py_cv_module_xxlimited_35=n/a
)

# A tree configured another way -- for musl, as before 2026-10-05, with other
# answers, or through another link wrapper -- is rebuilt from the tarball
# rather than reused: config.status would otherwise keep the old answers, and
# the build would carry them. The wrapper is in the stamp by its contents.
STAMP="CC=$(sha256sum < "$SLATE_LINK_CC" | cut -c1-16) ${CONFIGURE_ANSWERS[*]} --with-computed-gotos"
if [ -d "Python-$VER" ] && [ "$(cat "Python-$VER/.slate-configure" 2>/dev/null)" != "$STAMP" ]; then
    echo "CONFIGURATION_CHANGED: rebuilding Python-$VER from the tarball"
    rm -rf "Python-$VER"
fi
[ -d "Python-$VER" ] || tar xf "$TARBALL"
cd "Python-$VER"

export CC="$SLATE_LINK_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

# Blind pkg-config. This is not tidiness, it is the difference between a usable
# interpreter and a toy.
#
# CPython 3.12 finds zlib, OpenSSL, libffi, sqlite3, liblzma, bzip2, ncurses,
# readline and libuuid through `pkg-config`, which in a cross build answers for
# the *build* machine. configure therefore marked zlib/binascii/_ctypes as
# buildable, and the compiler then failed on `#include <zlib.h>` because zig's
# musl sysroot has no such header -- the three failures this spike used to
# report as "expected". They were never expected; they were configure believing
# the host's library set was the target's.
#
# Pointing PKG_CONFIG_LIBDIR at a directory that does not exist makes every
# probe answer "no", which is the truth: SlateOS's sysroot carries none of these
# yet. The modules with a bundled fallback (_decimal -> libmpdec,
# pyexpat/_elementtree -> expat) are unaffected; the rest turn off cleanly
# instead of turning on and then failing to compile.
export PKG_CONFIG_LIBDIR=/nonexistent-slateos-cross PKG_CONFIG_PATH=

# --disable-shared: SlateOS has no dynamic loader on this path, so the target is
#   a static ET_EXEC, same as bash and pkgconf.
# --without-ensurepip / --disable-test-modules: pip and the test suite are
#   megabytes of Python source that cannot affect which libc symbols the
#   interpreter core references.
# ac_cv_file__dev_pt*: configure cannot stat files on the target, and left to
#   guess it errors out. "no" is the literal answer: devfs has no /dev/ptmx node
#   -- our libc opens the name itself (posix/src/file.rs, open_pty_device) --
#   and it changes nothing, since CPython reaches a pty through openpty, which
#   it has (HAVE_OPENPTY), and reads HAVE_DEV_PTMX only without it.
# ac_cv_buggy_getaddrinfo=no: configure detects the well-known broken-getaddrinfo
#   bug by *running* a test program. It cannot run a target binary in a cross
#   build, so it assumes the bug is present and hard-errors ("You must get
#   working getaddrinfo()"). Asserting "not buggy" is the correct cross answer
#   and is what distro cross-recipes do; the alternative configure suggests,
#   --disable-ipv6, would silently compile a *different* interpreter with a
#   smaller socket surface -- which is the opposite of what a spike measuring
#   our libc surface wants. If our getaddrinfo turns out to be genuinely buggy
#   that is a posix/src bug to fix, not a reason to build less of CPython.
#
# MODULE_BUILDTYPE=static: build every stdlib C extension *into* libpython
#   rather than as a .so in lib-dynload. configure defaults this to "shared" on
#   every host except wasm, and --disable-shared does not change it -- it only
#   governs libpython itself. Left at the default, the interpreter links and
#   starts, and then `import struct` fails: measured on the musl control build,
#   sys.builtin_module_names held only the 31 bootstrap modules, and math,
#   _struct, _json, _random, select, _datetime, _socket, array, binascii,
#   _heapq, _bisect, _pickle and unicodedata were all absent. base64, json,
#   random, hashlib and datetime are all unusable in that state.
#
#   A shared lib-dynload is not an option to fix that with: the SlateOS target
#   is a static ET_EXEC with no dynamic loader (see --disable-shared above), so
#   an extension that is not inside the binary can never be reached. static is
#   the only build type that produces a working interpreter here.
if [ ! -f config.status ]; then
    MODULE_BUILDTYPE=static \
    ./configure \
        --host=x86_64-linux-musl \
        --build=x86_64-pc-linux-gnu \
        --with-build-python="$BUILD_PY" \
        --disable-shared \
        --without-ensurepip \
        --disable-test-modules \
        --with-computed-gotos \
        "${CONFIGURE_ANSWERS[@]}" \
        >conf.log 2>&1 && echo "CONFIGURE_EXIT=0" || {
            echo "CONFIGURE_EXIT=$?"
            echo "--- last 40 lines of conf.log ---"
            tail -40 conf.log
            exit 1
        }
    printf '%s\n' "$STAMP" > .slate-configure
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

# The control interpreter: the same objects, linked against zig's musl so that
# it runs under WSL, where stdlib.sh and the fastpy check run it. The objects
# call what our libc has, and musl has not got all of it; control-shim.c stands
# in for the difference, in this link only -- its comment says why that is
# safe. The image's interpreter is slatelink.sh's, against our libc.a alone.
EXTRA_A=()
for a in Modules/_decimal/libmpdec/libmpdec.a Modules/expat/libexpat.a Modules/_hacl/libHacl_Hash_*.a; do
    [ -f "$a" ] && EXTRA_A+=("$a")
done
rm -f python-control control-shim.o
CONTROL_RC=0
"$SLATE_CC" -O2 -c -o control-shim.o "$SLATE_ROOT/scripts/cpython-spike/control-shim.c" \
    >control-link.log 2>&1 \
    && "$SLATE_CC" -o python-control Programs/python.o "libpython${VER%.*}.a" "${EXTRA_A[@]}" \
        control-shim.o -lm >>control-link.log 2>&1 \
    || CONTROL_RC=$?
echo "CONTROL_LINK_EXIT=$CONTROL_RC"
# A symbol named here is one our libc gained and musl lacks: control-shim.c's.
grep -o "undefined symbol: .*" control-link.log | sort -u | head -20 || true
if [ ! -x python-control ]; then
    echo "NO_CONTROL_INTERPRETER -- see $PWD/control-link.log"
    exit 1
fi

echo "CPYTHON_SPIKE_BUILD_DONE"
