"""Make the stream `tests/bench.rs` decodes: 217 frames of 1080p film in VP8.

libvpx's VP8 test vectors are all small (the largest 1432x888, two frames),
so the benchmark encodes one of its VP9 vectors -- the 1080p film
`vp90-2-02-size-lf-1920x1080.webm`, which gui/video/vp9's
`tools/fetch_vectors.py` fetches -- as VP8, with libvpx's encoder through
ffmpeg, at 5 Mbit/s with alternate reference frames, as a web video would
be:

    python gui/video/vp9/tools/fetch_vectors.py
    python gui/video/vp8/tools/make_bench_stream.py

It runs ffmpeg in WSL (`wsl -- ffmpeg`), or `ffmpeg` on the path when there
is no WSL, and writes `target/vp8vectors/bench-film-1080p.ivf`. The stream
is a measurement's input, not a reference: another ffmpeg or libvpx makes
another stream, and libvpx's speed must be measured on the same one
(`tests/bench.rs` says how).
"""
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
SOURCE = os.path.join(ROOT, "target", "vp9vectors", "vp90-2-02-size-lf-1920x1080.webm")
DEST = os.path.join(ROOT, "target", "vp8vectors", "bench-film-1080p.ivf")
ARGS = ["-c:v", "libvpx", "-b:v", "5M", "-g", "9999", "-auto-alt-ref", "1",
        "-lag-in-frames", "16", "-deadline", "good", "-cpu-used", "1", "-threads", "1",
        "-f", "ivf"]


def wsl_path(p: str) -> str:
    p = os.path.abspath(p).replace(chr(92), "/")
    return "/mnt/" + p[0].lower() + p[2:]


def main() -> int:
    if not os.path.exists(SOURCE):
        print(f"{SOURCE} is missing: python gui/video/vp9/tools/fetch_vectors.py")
        return 1
    os.makedirs(os.path.dirname(DEST), exist_ok=True)
    if shutil.which("wsl"):
        cmd = ["wsl", "--", "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
               "-i", wsl_path(SOURCE), *ARGS, wsl_path(DEST)]
    else:
        cmd = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", SOURCE, *ARGS, DEST]
    subprocess.run(cmd, check=True)
    print(f"{DEST}: {os.path.getsize(DEST)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
