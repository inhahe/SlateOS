"""Where this decoder first parts from FFmpeg on one stream, in decode order.

    python gui/video/h264/tools/h264cmp.py <name> [--all N] [--release]

<name> is a stream as the pixel test names it (`h264-conformance/BA_MW_D.264`,
`cut/h264/sei-1.h264~p1.h264`). Both decoders print a line for each
macroblock they reconstruct -- its type, cbp, qp and an Adler-32 of its
samples in each plane, before the loop filter (`MB`) -- for each macroblock
once its edges are filtered (`LF`), for each inter macroblock its references
and vectors (`MV`), and for each picture handed out its POCs and each
field's luma (`OUT`). This runs both on the stream and reports, for each
kind, the first line where they disagree: the picture, the macroblock and
what differs. The first `MB` difference is usually the one to read.

FFmpeg's side is its 8.1.3 build in WSL with those lines added
(`tools/ffdbg/build.sh`, in ~/ffdbg/src). This decoder's side is the pixel
test under `H264_MB`, `H264_LF`, `H264_MV` and `H264_OUT`, with `H264_ONLY`
naming the one stream. H264_FATE and H264_PICT must be set as for the pixel
test.
"""
import os
import subprocess
import sys
from pathlib import Path

CONTAINERS = {".avi", ".flv", ".mkv", ".mov", ".mp4", ".mxf", ".ts"}
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
TAGS = ("MB", "LF", "MV", "OUT")


def wsl_path(p: Path) -> str:
    s = p.resolve().as_posix()
    return f"/mnt/{s[0].lower()}{s[2:]}"


def lines(path: Path, tag: str, by_decoder: bool) -> list[str]:
    """The `tag` lines of a log; FFmpeg's only from its main decoder -- the
    last it made, after avformat_find_stream_info's -- without the field
    numbering the decoder."""
    got = [l.rstrip("\n") for l in path.read_text(encoding="utf-8", errors="replace").splitlines()
           if l.startswith(tag + " ")]
    if not by_decoder:
        return got
    groups: dict[int, list[str]] = {}
    for l in got:
        body, _, h = l.rpartition(" h=")
        groups.setdefault(int(h), []).append(body)
    return groups[max(groups)] if groups else []


def compare(tag: str, ff: list[str], ours: list[str], show: int) -> bool:
    shown = 0
    for i, (a, b) in enumerate(zip(ff, ours)):
        if a == b:
            continue
        print(f"{tag} line {i} (decode order):")
        for k, (x, y) in enumerate(zip(a.split(), b.split())):
            if x != y:
                print(f"  field {k}: FFmpeg {x}, here {y}")
        print("  FFmpeg:", a)
        print("  here:  ", b)
        if i:
            print("  before:", ff[i - 1])
        shown += 1
        if shown >= show:
            return False
    if shown:
        return False
    if len(ff) != len(ours):
        print(f"{tag}: the first {min(len(ff), len(ours))} lines agree; FFmpeg has {len(ff)}, "
              f"here {len(ours)}")
        return False
    print(f"{tag}: all {len(ff)} lines agree")
    return True


def main() -> None:
    args = sys.argv[1:]
    show = 1
    if "--all" in args:
        at = args.index("--all")
        show = int(args[at + 1])
        del args[at:at + 2]
    release = "--release" in args
    if release:
        args.remove("--release")
    name = args[0]
    fate, pict = Path(os.environ["H264_FATE"]), Path(os.environ["H264_PICT"])
    stream = (pict if name.startswith("cut/") else fate) / name
    raw = [] if stream.suffix.lower() in CONTAINERS else ["-f", "h264"]
    tmp = Path(os.environ.get("TEMP", "/tmp")) / "h264cmp"
    tmp.mkdir(exist_ok=True)
    ff_log, our_log = tmp / "ffmpeg.txt", tmp / "here.txt"

    # Run with no shell between: the probes' lines go to stderr, the
    # pictures to /dev/null. FFmpeg logs nothing (quiet): its messages come
    # from other threads too -- the demuxer's parser -- and one whose prefix
    # lands in front of a probe line hides that line.
    with ff_log.open("wb") as err:
        subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "env", "FFDBG_MB=1", "FFDBG_LF=1",
                        "FFDBG_MV=1", "FFDBG_OUT=1", os.environ.get("FFDBG", "/home/inhahe/ffdbg/src")
                        + "/ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "quiet",
                        "-threads", "1", "-flags", "unaligned", *raw, "-i", wsl_path(stream),
                        "-map", "0:v:0", "-fps_mode", "passthrough", "-noautoscale",
                        "-f", "rawvideo", "-y", "/dev/null"],
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=err,
                       check=True)

    env = dict(os.environ, H264_ONLY="=" + name, **{f"H264_{t}": "1" for t in TAGS})
    env.pop("CARGO_TARGET_DIR", None)
    cmd = ["cargo", "test", "--target", "x86_64-pc-windows-gnu", "-p", "h264", "--lib"]
    if release:
        cmd.insert(2, "--release")
    cmd += ["--", "--include-ignored", "--nocapture", "sample_for_sample"]
    with our_log.open("wb") as out:
        subprocess.run(cmd, cwd=ROOT, env=env, stdout=out, stderr=subprocess.STDOUT, check=False)

    ok = True
    for tag in TAGS:
        ok &= compare(tag, lines(ff_log, tag, True), lines(our_log, tag, False), show)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
