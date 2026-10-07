#!/usr/bin/env bash
# Builds tools/reference.c against Tremor and libogg, into $1 (default
# ~/vorbisref/build/reference). Run in WSL (or any Linux with gcc):
#
#   bash tools/build_reference.sh [out]
#
# Expects Tremor at ~/vorbisref/tremor (Xiph's tremor repository) and
# libogg at ~/vorbisref/ogg, with ogg/include/ogg/config_types.h present.
# Tremor is built as its own build does: -O2, not _LOW_ACCURACY_, the C
# (not ARM assembly) arithmetic.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
ref=${VORBISREF:-$HOME/vorbisref}
out=${1:-$ref/build/reference}
obj=$(mktemp -d)
trap 'rm -rf "$obj"' EXIT
cflags="-O2 -g -I$ref/ogg/include -I$ref/tremor"
for f in bitwise framing; do
    gcc $cflags -c "$ref/ogg/src/$f.c" -o "$obj/$f.o"
done
for f in block codebook floor0 floor1 info mapping0 mdct registry res012 sharedbook synthesis window; do
    gcc $cflags -c "$ref/tremor/$f.c" -o "$obj/t_$f.o"
done
gcc $cflags -c "$here/reference.c" -o "$obj/reference.o"
gcc -o "$out" "$obj"/*.o
echo "built $out"
