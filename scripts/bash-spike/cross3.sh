#!/bin/bash
# Continue the cross build after the strtoimax collision.
#
# bash 5.2's configure has an inverted test: when the system HAS a usable
# strtoimax it *adds* lib/sh/strtoimax.c to LIBOBJS (configure:20446). Against
# a dynamic glibc that is harmless — the archive definition simply wins over
# the shared one. Against a static musl it is fatal: musl puts strtoimax in the
# same object as strtol, that object is pulled in for strtol, and lld sees two
# definitions.
#
# Fix: drop strtoimax from libsh.a. For a fresh configure the equivalent is to
# pass the cache var `bash_cv_func_strtoimax=no`, which skips the LIBOBJS
# branch entirely.
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

set -x
SPIKE="$SLATE_SPIKE"
# Must match cross2.sh, which created this tree, and slatelink.sh, which reads it.
BUILD="/tmp/bash-cross-$SLATE_LANE"
cd "$BUILD" || exit 1

# Same pinned toolchain cross2.sh configured with. Resolving it again (rather
# than assuming cross2.sh's /tmp wrappers still exist) means this script can be
# re-run days later without silently picking up a stale wrapper naming another
# worktree's zig -- which is what the un-lane-keyed /tmp/zigcc did until
# 2026-08-16.
slate_make_zig_wrappers || exit 1

sed -i 's|\${LIBOBJDIR}strtoimax\$U\.o||' lib/sh/Makefile
grep -n '^LIBOBJS' lib/sh/Makefile
rm -f lib/sh/strtoimax.o lib/sh/libsh.a

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"
make -j8 >>cross-make.log 2>&1
echo "CROSS_MAKE_EXIT=$?"
grep -E 'ld\.lld: error|Error [0-9]' cross-make.log | tail -20

if [ -x "$BUILD/bash" ]; then
  file "$BUILD/bash"
  ls -l "$BUILD/bash"
  echo "CROSS_BASH_BUILT"
  cp "$BUILD/bash" "$SPIKE/bash-musl.elf"
else
  echo "NO_CROSS_BINARY"
fi
