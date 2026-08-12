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
set -x
SPIKE="/mnt/d/visual studio projects/os/build/spike"
BUILD="/tmp/bash-cross"
cd "$BUILD" || exit 1

sed -i 's|\${LIBOBJDIR}strtoimax\$U\.o||' lib/sh/Makefile
grep -n '^LIBOBJS' lib/sh/Makefile
rm -f lib/sh/strtoimax.o lib/sh/libsh.a

export CC=/tmp/zigcc AR=/tmp/zigar RANLIB=/tmp/zigranlib
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
