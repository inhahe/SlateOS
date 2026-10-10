"""Make the streams `tests/bench.rs` decodes: 217 frames of 1080p film in VP8.

libvpx's VP8 test vectors are all small (the largest 1432x888, two frames),
so the benchmark encodes one of its VP9 vectors -- the 1080p film
`vp90-2-02-size-lf-1920x1080.webm`, which gui/video/vp9's
`tools/fetch_vectors.py` fetches -- as VP8, at 5 Mbit/s with alternate
reference frames, as a web video would be:

    python gui/video/vp9/tools/fetch_vectors.py
    python gui/video/vp8/tools/make_bench_stream.py [--vpxenc PATH]

`bench-film-1080p.ivf` is coded by libvpx's encoder through ffmpeg, in one
token partition, as encoders code VP8 unless asked otherwise.
`bench-film-1080p-parts8.ivf`, the same film in eight partitions, which
decodes on up to eight threads, needs libvpx's own `vpxenc` (ffmpeg cannot
ask for partitions), built with VP8's encoder: give its path with
`--vpxenc`. Without it, only the first stream is made.

It runs ffmpeg (and vpxenc) in WSL (`wsl -- ...`; `--vpxenc` is then a
path in WSL), or on the path when there is no WSL, and writes into
`target/vp8vectors/`. The streams are a measurement's input, not a
reference: another ffmpeg or libvpx makes other streams, and libvpx's speed
must be measured on the same ones (`tests/bench.rs` says how).
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
SOURCE = os.path.join(ROOT, "target", "vp9vectors", "vp90-2-02-size-lf-1920x1080.webm")
DEST = os.path.join(ROOT, "target", "vp8vectors", "bench-film-1080p.ivf")
DEST_PARTS = os.path.join(ROOT, "target", "vp8vectors", "bench-film-1080p-parts8.ivf")
ARGS = ["-c:v", "libvpx", "-b:v", "5M", "-g", "9999", "-auto-alt-ref", "1",
        "-lag-in-frames", "16", "-deadline", "good", "-cpu-used", "1", "-threads", "1",
        "-f", "ivf"]
# The same settings as ARGS, in vpxenc's words, with eight token partitions.
VPXENC_ARGS = ["--codec=vp8", "--good", "--cpu-used=1", "--end-usage=vbr",
               "--target-bitrate=5000", "--auto-alt-ref=1", "--lag-in-frames=16",
               "--kf-max-dist=9999", "--threads=1", "--token-parts=3", "--ivf"]


def wsl_path(p: str) -> str:
    p = os.path.abspath(p).replace(chr(92), "/")
    return "/mnt/" + p[0].lower() + p[2:]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--vpxenc", help="libvpx's vpxenc, built with VP8's encoder "
                        "(a path in WSL when WSL is used): also make the eight-partition stream")
    args = parser.parse_args()
    if not os.path.exists(SOURCE):
        print(f"{SOURCE} is missing: python gui/video/vp9/tools/fetch_vectors.py")
        return 1
    os.makedirs(os.path.dirname(DEST), exist_ok=True)
    wsl = shutil.which("wsl") is not None
    run = (lambda cmd: ["wsl", "--", *cmd]) if wsl else (lambda cmd: cmd)
    path = wsl_path if wsl else (lambda p: p)
    ffmpeg = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y"]
    subprocess.run(run([*ffmpeg, "-i", path(SOURCE), *ARGS, path(DEST)]), check=True)
    print(f"{DEST}: {os.path.getsize(DEST)} bytes")
    if args.vpxenc:
        # vpxenc reads raw pictures: the film decoded to Y4M first (about
        # 680 MB), in WSL's /tmp or this machine's temporary directory.
        raw = "/tmp/vp8-bench-film.y4m" if wsl else os.path.join(
            tempfile.gettempdir(), "vp8-bench-film.y4m")
        try:
            subprocess.run(run([*ffmpeg, "-i", path(SOURCE), "-pix_fmt", "yuv420p",
                                "-f", "yuv4mpegpipe", raw]), check=True)
            subprocess.run(run([args.vpxenc, *VPXENC_ARGS, "--quiet", "-o", path(DEST_PARTS),
                                raw]), check=True)
        finally:
            # The pictures are scratch: a failure to remove them leaves a
            # temporary file, not a wrong stream.
            if wsl:
                subprocess.run(["wsl", "--", "rm", "-f", raw], check=False)
            elif os.path.exists(raw):
                os.remove(raw)
        print(f"{DEST_PARTS}: {os.path.getsize(DEST_PARTS)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
