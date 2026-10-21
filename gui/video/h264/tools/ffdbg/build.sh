#!/bin/bash
# FFmpeg 8.1.3's H.264 decoder, built in WSL as the oracle this crate is held
# to: the release it is a translation of, with no assembly at all -- every
# routine the C one -- and with the probes in ffdbg.patch, which print what
# tools/h264cmp.py compares and fill what tools/pict_expect.py --crc-wsl needs
# filled. Nothing a probe does happens unless its variable is set, so the
# build decodes exactly as 8.1.3 does.
#
#     wsl -d Ubuntu --exec bash gui/video/h264/tools/ffdbg/build.sh [DIR]
#
# (by path, not `bash -s < build.sh`: the patch is found beside the script).
# Fetches the n8.1.3 tag into DIR (default ~/ffdbg/src) unless it is there,
# patches it, and builds `ffmpeg` with only what the tests read: the H.264
# decoder and parser, the demuxers FATE's H.264 samples come in, and the
# framecrc and rawvideo muxers.
#
# The probes, each an environment variable:
#
#   FFDBG_MB    after each macroblock is reconstructed, before the loop
#               filter: "MB poc= mb=x,y type= cbp= qp= sum=y,cb,cr h=N" --
#               an Adler-32 of its samples in each plane (ff_h264_hl_decode_mb,
#               which error concealment's macroblocks pass through too).
#   FFDBG_LF    after each macroblock's edges are filtered: "LF ... sum=".
#   FFDBG_MV    for each inter macroblock: its references and vectors, block
#               by block, as motion compensation reads them.
#   FFDBG_REF   at each slice's first macroblock, each entry of its
#               reference lists: POC, fields, and its picture's luma summed.
#   FFDBG_PIC   for the picture being decoded, once a packet's slices are
#               decoded and concealed -- shown or not -- each plane summed.
#   FFDBG_OUT   for each picture handed out: its POCs and each field's luma.
#   FFDBG_FRAMECRC
#               "DECODER d=N" as each decoder opens, and for each frame
#               leaving one (decode.c), cropped exactly, "FRAMECRC d=N size,
#               0x...": summed as the framecrc muxer sums a picture but in
#               the frame's own format -- the command line converts every
#               frame to the first one's format for its encoder. What
#               tools/pict_expect.py --crc-wsl reads.
#   FFDBG_ER    when error concealment conceals anything.
#   FFDBG_NEWEXTRA
#               each packet's new extradata as the decoder takes it up:
#               "NEWEXTRA pos=<packet position> <hex> h=N". What
#               tools/pict_expect.py --add-newextra reads.
#   FFDBG_FILL  every picture started filled with this byte (0-255), and
#               the frame it is shown as with film grain, so samples FFmpeg
#               never writes show as it rather than as whatever memory its
#               buffer pool hands back.
#   FFDBG_ZERO_TABLES
#               a picture's tables (vectors, references, types, quantizers)
#               zeroed every time their pools hand them out, not only when
#               first made: to see what of a damaged stream's output
#               depends on what earlier pictures left in them.
#
# `h=N` and `d=N` number the decoder as it opens: avformat_find_stream_info's
# comes first, and the one that shows the stream is the last.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dir=${1:-$HOME/ffdbg/src}
if [ ! -d "$dir" ]; then
    git clone --depth 1 --branch n8.1.3 https://git.ffmpeg.org/ffmpeg.git "$dir"
fi
cd "$dir"
if ! grep -q FFDBG_MB libavcodec/h264_mb.c; then
    patch -p1 < "$here/ffdbg.patch"
fi
./configure --disable-everything --disable-asm --disable-doc --disable-autodetect \
    --enable-debug=3 --optflags=-O1 \
    --enable-decoder=h264 --enable-parser=h264 \
    --enable-demuxer=h264,mov,matroska,flv,mpegts,avi,mxf \
    --enable-muxer=rawvideo,framecrc,null --enable-encoder=rawvideo \
    --enable-protocol=file,pipe \
    --enable-bsf=extract_extradata,h264_mp4toannexb,null \
    --enable-filter=buffer,buffersink,null,format,scale,copy \
    --disable-ffplay
make -j"$(nproc)" ffmpeg ffprobe
echo "built $dir/ffmpeg and $dir/ffprobe"
