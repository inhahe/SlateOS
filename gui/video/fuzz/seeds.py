"""Seed corpora for lane F's decoder targets, from the crates' own test
fixtures: `scripts/fuzz.py` calls `seed(src, corpus)`. See README.md."""
import os
import shutil
import struct
import subprocess

MAX_SEED = 256 * 1024


def ivf_packets(path, limit=8):
    """The first `limit` packets of an IVF file, each prefixed by its length:
    what the VP8 and VP9 targets read."""
    data = open(path, "rb").read()
    if data[:4] != b"DKIF":
        return None
    header = struct.unpack_from("<H", data, 6)[0]
    at, out = header, []
    while at + 12 <= len(data) and len(out) < limit:
        size = struct.unpack_from("<I", data, at)[0]
        out.append(data[at + 12: at + 12 + size])
        at += 12 + size
    return b"".join(struct.pack("<I", len(p)) + p for p in out)


def seed(src, corpus):
    def add_files(target, sub, exts=None):
        base = os.path.join(src, sub)
        if not os.path.isdir(base):
            return
        out = corpus(target)
        for dirpath, _, names in os.walk(base):
            for name in names:
                if exts and not name.lower().endswith(exts):
                    continue
                if name.endswith((".md5", ".txt", ".json", ".py", ".rs", ".md", ".c", ".h")):
                    continue
                p = os.path.join(dirpath, name)
                if os.path.getsize(p) > MAX_SEED:
                    continue
                rel = os.path.relpath(p, base).replace(os.sep, "_")
                shutil.copy2(p, os.path.join(out, f"{sub.replace('/', '_')}__{rel}"))

    add_files("image", "gui/imagecodec/tests/data")
    for s in ("gui/video/codec/tests/data", "gui/video/matroska/tests/data",
              "gui/video/mp4/tests/data"):
        add_files("video", s)
    add_files("video", "gui/video/vp9/tests/data", (".webm",))
    for s in ("gui/video/ogg/tests/data", "gui/video/opus/tests/data",
              "gui/video/vorbis/tests/data", "gui/video/flac/tests/data",
              "gui/video/mp3/tests/data", "gui/video/codec/tests/data"):
        add_files("sound", s)
    add_files("matroska", "gui/video/matroska/tests/data")
    add_files("matroska", "gui/video/vp9/tests/data", (".webm",))
    # MP4 and QuickTime: the demuxer's own fixtures -- ffmpeg's files and the
    # hand-written ones that exercise each table -- and videocodec's.
    add_files("mp4", "gui/video/mp4/tests/data", (".mp4", ".mov"))
    add_files("mp4", "gui/video/codec/tests/data", (".mp4", ".mov", ".m4a"))

    # The subtitle fixtures: SubRip, ASS, SSA and WebVTT tracks, ffmpeg's and
    # mkvmerge's; MP4's timed text, WebVTT and TTML and CEA-608 captions;
    # Blu-ray's PGS, DVD's VobSub and DVB's pictures.
    codec_data = os.path.join(src, "gui/video/codec/tests/data")
    for name in sorted(os.listdir(codec_data)):
        if name.startswith(("subrip", "ass", "ssa", "webvtt", "overlap", "movtext", "pgs",
                            "vobsub", "dvb", "ttml", "cea608")) and name.endswith(
                (".mkv", ".webm", ".mp4")):
            shutil.copy2(os.path.join(codec_data, name), os.path.join(corpus("subtitles"), name))

    for dirpath, _, names in os.walk(os.path.join(src, "gui/video/vp8/tests/data")):
        for name in names:
            if name.endswith(".ivf"):
                packets = ivf_packets(os.path.join(dirpath, name))
                if packets:
                    open(os.path.join(corpus("vp8"), name + ".pkt"), "wb").write(packets)

    # VP9's WebM vectors are remuxed to IVF with ffmpeg first.
    tmp = os.path.join(os.path.expanduser("~"), "fuzz", "ivf-tmp")
    os.makedirs(tmp, exist_ok=True)
    for dirpath, _, names in os.walk(os.path.join(src, "gui/video/vp9/tests/data")):
        for name in names:
            path = os.path.join(dirpath, name)
            if name.endswith(".ivf"):
                ivf = path
            elif name.endswith(".webm"):
                ivf = os.path.join(tmp, name + ".ivf")
                r = subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", path,
                                    "-map", "0:v:0", "-c:v", "copy", "-frames:v", "8", "-f",
                                    "ivf", ivf], stdin=subprocess.DEVNULL)
                if r.returncode != 0:
                    continue
            else:
                continue
            packets = ivf_packets(ivf)
            if packets and len(packets) <= MAX_SEED:
                open(os.path.join(corpus("vp9"), name + ".pkt"), "wb").write(packets)
    shutil.rmtree(tmp)
