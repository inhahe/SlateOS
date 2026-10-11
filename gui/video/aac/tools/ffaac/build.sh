#!/bin/bash
# FFmpeg 8.1.3's fixed-point AAC decoder (`aac_fixed`), built in WSL as the
# oracle `gui/video/aac` is held to: the release it is a translation of,
# with no assembly at all -- every routine the C one -- so the samples it
# gives are the ones the C code computes; with the probe in ffaac.patch.
#
#     wsl -d Ubuntu --exec bash gui/video/aac/tools/ffaac/build.sh [DIR]
#
# (by path, not `bash -s < build.sh`: the patch is found beside the script).
# Fetches the n8.1.3 tag into DIR (default ~/ffaac/src) unless it is there,
# patches it, and builds `ffmpeg` and `ffprobe` with only what the tests
# read: the AAC decoders and parsers, the demuxers FATE's AAC samples come in
# (MP4, ADTS, LATM in LOAS and MPEG-TS), and the muxers that write and sum
# PCM.
#
# The probe, an environment variable:
#
#   FFDBG_PCM   a file records are appended to (decode.c), each a header
#               of eight little-endian 32-bit words, then data:
#                 - as each audio decoder opens: 0x4e45504f ("OPEN"), the
#                   decoder's number (counting each opened, from 1:
#                   probing's first, the one that plays the stream last),
#                   channels, the layout's mask (low word, high word),
#                   extradata size, sample rate, layout order; then the
#                   extradata -- what the decoder is opened with;
#                 - as each audio frame leaves a decoder: 0x4d435046
#                   ("FPCM"), the decoder's number, channels, the layout's
#                   mask (low, high), samples per channel, sample rate,
#                   sample format; then each channel's samples in turn, in
#                   the frame's own format.
#               What tools/pcm_expect.py reads: the samples as the decoder
#               gives them, before ffmpeg's command line converts a stream
#               whose layout changes to the first one's.
#
# Nothing the probe does happens unless its variable is set, so the build
# decodes exactly as 8.1.3 does.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dir=${1:-$HOME/ffaac/src}
if [ ! -d "$dir" ]; then
    git clone --depth 1 --branch n8.1.3 https://git.ffmpeg.org/ffmpeg.git "$dir"
fi
cd "$dir"
if ! grep -q FFDBG_PCM libavcodec/decode.c; then
    patch -p1 < "$here/ffaac.patch"
fi
./configure --disable-everything --disable-asm --disable-doc --disable-autodetect \
    --enable-debug=3 --optflags=-O1 \
    --enable-decoder=aac_fixed,aac,aac_latm \
    --enable-parser=aac,aac_latm \
    --enable-demuxer=aac,mov,loas,mpegts,matroska,wav \
    --enable-muxer=framecrc,framemd5,pcm_s32le,null \
    --enable-encoder=pcm_s32le \
    --enable-protocol=file,pipe \
    --enable-filter=abuffer,abuffersink,anull,aformat,aresample \
    --disable-ffplay
make -j"$(nproc)" ffmpeg ffprobe
echo "built $dir/ffmpeg and $dir/ffprobe"
