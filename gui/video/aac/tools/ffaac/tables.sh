#!/bin/bash
# The tables FFmpeg 8.1.3's fixed-point AAC decoder computes as it starts,
# from `double` arithmetic and libm -- which another maths library could
# round otherwise -- computed by FFmpeg's own code in the oracle's build
# (tools/ffaac/build.sh) and printed as C for tools/gen_tables.py to read:
#
#     wsl -d Ubuntu --exec bash gui/video/aac/tools/ffaac/tables.sh [DIR] [OUT]
#
# DIR is the FFmpeg tree (default ~/ffaac/src); OUT where the C files go
# (default ~/ffaac/tables). FFmpeg's table generators for its hardcoded-table
# builds print the cube roots, sine windows and parametric stereo's tables;
# kbd_tables.c, beside this, prints the Kaiser-Bessel-derived windows, which
# have no generator.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dir=${1:-$HOME/ffaac/src}
out=${2:-$HOME/ffaac/tables}
mkdir -p "$out"
cd "$dir"
build() {
    # A generator, compiled as FFmpeg's build compiles its host tools: no
    # config but the tree's, maths from libm.
    gcc -O1 -I. -DUSE_FIXED=1 "$1" libavutil/libavutil.a -lm -lpthread -o "$out/$2"
    "$out/$2" > "$out/$2.c"
    rm "$out/$2"
}
build libavcodec/cbrt_fixed_tablegen.c cbrt_fixed
build libavcodec/sinewin_fixed_tablegen.c sinewin_fixed
build libavcodec/aacps_fixed_tablegen.c aacps_fixed
build "$here/kbd_tables.c" kbd_fixed
echo "tables in $out"
