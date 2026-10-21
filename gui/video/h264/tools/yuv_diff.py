"""Where this decoder's pictures first differ from FFmpeg's.

    python gui/video/h264/tools/yuv_diff.py <stream> <ours.yuv> [--max N]

<ours.yuv> is what the pixel test writes under H264_DUMP for a stream
that differs: every picture shown, planes end to end. FFmpeg's pictures
are decoded the way the test's sums were made (C routines, one thread,
exact cropping). For each picture that differs this prints, per plane,
how many samples differ, the first one -- its position and macroblock --
and both values; then stops after N pictures (default 3).
"""
import json
import subprocess
import sys
from pathlib import Path

CONTAINERS = {".avi", ".flv", ".mkv", ".mov", ".mp4", ".mxf", ".ts"}

# Chroma subsampling (x, y) and bytes a sample, by pixel format.
FORMATS = {
    "yuv420p": (1, 1, 1), "yuvj420p": (1, 1, 1), "yuv422p": (1, 0, 1), "yuvj422p": (1, 0, 1),
    "yuv444p": (0, 0, 1), "yuvj444p": (0, 0, 1), "gbrp": (0, 0, 1),
}
for depth in (9, 10, 12, 14):
    for name, (sx, sy) in (("yuv420p", (1, 1)), ("yuv422p", (1, 0)), ("yuv444p", (0, 0)),
                           ("gbrp", (0, 0))):
        FORMATS[f"{name}{depth}le"] = (sx, sy, 2)


def raw_format(path: Path) -> list[str]:
    return [] if path.suffix.lower() in CONTAINERS else ["-f", "h264"]


def main() -> None:
    args = sys.argv[1:]
    limit = 3
    if "--max" in args:
        at = args.index("--max")
        limit = int(args[at + 1])
        del args[at:at + 2]
    stream, ours = Path(args[0]), Path(args[1])
    info = json.loads(subprocess.run(
        ["ffprobe", "-v", "error", *raw_format(stream), "-select_streams", "v:0",
         "-show_entries", "stream=width,height,pix_fmt", "-of", "json", str(stream)],
        capture_output=True, text=True, encoding="utf-8", check=True).stdout)["streams"][0]
    w, h, fmt = info["width"], info["height"], info["pix_fmt"]
    sx, sy, bps = FORMATS[fmt]
    cw, ch = -(-w // (1 << sx)), -(-h // (1 << sy))
    planes = [(w, h), (cw, ch), (cw, ch)]
    frame_size = sum(pw * ph for pw, ph in planes) * bps
    theirs = subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-cpuflags", "0", "-threads", "1",
         "-flags", "unaligned", *raw_format(stream), "-i", str(stream), "-map", "0:v:0",
         "-fps_mode", "passthrough", "-noautoscale", "-f", "rawvideo", "-"],
        capture_output=True, check=True).stdout
    mine = ours.read_bytes()
    print(f"{w}x{h} {fmt}: FFmpeg {len(theirs) // frame_size} pictures, here "
          f"{len(mine) // frame_size}")
    shown = 0
    for k in range(min(len(theirs), len(mine)) // frame_size):
        a = theirs[k * frame_size:(k + 1) * frame_size]
        b = mine[k * frame_size:(k + 1) * frame_size]
        if a == b:
            continue
        print(f"picture {k}:")
        at = 0
        for p, (pw, ph) in enumerate(planes):
            n = pw * ph * bps
            pa, pb = a[at:at + n], b[at:at + n]
            at += n
            if pa == pb:
                continue
            diffs = [i for i in range(0, n, bps) if pa[i:i + bps] != pb[i:i + bps]]
            first = diffs[0] // bps
            x, y = first % pw, first // pw
            mb = (x >> (4 - (sx if p else 0)), y >> (4 - (sy if p else 0)))
            val = lambda s, i: int.from_bytes(s[i:i + bps], "little")
            print(f"  plane {p}: {len(diffs)} samples differ; first at ({x}, {y}), macroblock "
                  f"{mb}: FFmpeg {val(pa, diffs[0])}, here {val(pb, diffs[0])}")
        shown += 1
        if shown >= limit:
            break


if __name__ == "__main__":
    main()
