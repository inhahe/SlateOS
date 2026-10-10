"""Write what FFmpeg makes of each H.264 conformance stream's pictures, for
src/decoder/tests.rs to hold the decoder's handling of pictures to.

For every stream under a directory of FFmpeg's FATE samples this writes,
under an output directory and the stream's own relative path:

  <stream>.packets  the size of each packet FFmpeg's raw H.264 demuxer and
                    parser split the file into, one a line: the decoder is
                    fed exactly those packets;
  <stream>.pict     FFmpeg's `-debug pict` line for each slice, without its
                    `[h264 @ ...]` prefix: where it starts, its picture's
                    frame number and order counts, its reference counts,
                    QP, loop filter and weights. One thread, so that the
                    lines come in decoding order. FFmpeg first decodes a few
                    pictures with a decoder of its own to learn the stream's
                    format; only what is logged after it says probing is
                    over is kept.
  <stream>.frames   each picture FFmpeg hands out, in order: the position of
                    the packet that began it, its picture type, and whether
                    it is a key frame, interlaced, top field first, and how
                    many fields it repeats -- ffprobe's view of the output.
  <stream>.delay    how many pictures FFmpeg's decoder holds back from the
                    start (`has_b_frames`): what probing found, which the
                    decoder FFmpeg shows the stream with starts from.
  <stream>.mmco     FFmpeg's `-debug mmco` lines without their pointers: the
                    short- and long-term reference lists as each slice's
                    lists are built and after each picture is marked, and
                    each marking operation as it is carried out.

    python gui/video/h264/tools/pict_expect.py <fate-suite>/h264-conformance <out>

`ffmpeg` and `ffprobe` must be on PATH.
"""
import json
import re
import subprocess
import sys
from pathlib import Path

SKIP_SUFFIXES = {".md5", ".txt", ".yuv"}


def packets(path: Path) -> list[int]:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
         "packet=pos,size", "-of", "json", str(path)],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=True,
    ).stdout
    sizes = []
    expect_pos = 0
    for p in json.loads(out).get("packets", []):
        size, pos = int(p["size"]), int(p.get("pos", expect_pos))
        if pos != expect_pos:
            raise SystemExit(f"{path}: packet at {pos}, expected {expect_pos}")
        sizes.append(size)
        expect_pos = pos + size
    return sizes


PROBED = "After avformat_find_stream_info()"


def decoder_lines(path: Path, debug: str) -> list[str]:
    """What FFmpeg's decoders log with `-debug <debug>` once probing is
    over -- the probe decodes the stream's first pictures with a decoder of
    its own, and FFmpeg logs where it stops -- each without its
    `[h264 @ ...]` prefix."""
    err = subprocess.run(
        ["ffmpeg", "-hide_banner", "-threads", "1", "-debug", debug, "-i", str(path),
         "-f", "null", "-"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    ).stderr.splitlines()
    start = next((i for i, l in enumerate(err) if PROBED in l), None)
    if start is None:
        raise SystemExit(f"{path}: FFmpeg did not log the end of probing")
    out = []
    for line in err[start + 1:]:
        if line.startswith("[h264 @"):
            out.append(line[line.find("] ") + 2:])
    return out


def pict(path: Path) -> list[str]:
    return [l for l in decoder_lines(path, "pict") if l.startswith("slice:")]


def frames(path: Path) -> list[str]:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-threads", "1", "-show_frames", "-show_entries",
         "frame=pkt_pos,pict_type,key_frame,interlaced_frame,top_field_first,repeat_pict",
         "-of", "json", str(path)],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    ).stdout
    rows = []
    for f in json.loads(out or "{}").get("frames", []):
        rows.append(" ".join(str(f.get(k, "-")) for k in (
            "pkt_pos", "pict_type", "key_frame", "interlaced_frame", "top_field_first",
            "repeat_pict")))
    return rows


# The lines `-debug mmco` prints about references (h264_refs.c), the rest of
# what that log level brings filtered out.
MMCO_LINE = re.compile(
    r"^(short term list:|long term list:|no mmco here|mmco:-?\d+ -?\d+ -?\d+|"
    r"mmco: unref short -?\d+ count \d+|mmco: unref long failure|"
    r"remove short -?\d+ count \d+|\d+ fn:-?\d+ poc:-?\d+|\d+ -?\d+)( [0-9a-fA-F]+)?$")


def mmco(path: Path) -> list[str]:
    """The `-debug mmco` lines without the pointers they end in."""
    return [m.group(1) for m in map(MMCO_LINE.match, decoder_lines(path, "mmco")) if m]


def delay(path: Path) -> int:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
         "stream=has_b_frames", "-of", "json", str(path)],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=True,
    ).stdout
    streams = json.loads(out).get("streams", [])
    return int(streams[0].get("has_b_frames", 0)) if streams else 0


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    count = 0
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        if path.suffix.lower() in SKIP_SUFFIXES or path.name == "md5sum":
            continue
        rel = path.relative_to(root)
        dest = out / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        sizes = packets(path)
        if sum(sizes) != path.stat().st_size:
            print(f"{rel}: packets cover {sum(sizes)} of {path.stat().st_size} bytes", flush=True)
        dest.with_name(dest.name + ".packets").write_text(
            "".join(f"{s}\n" for s in sizes), encoding="utf-8", newline="\n")
        dest.with_name(dest.name + ".pict").write_text(
            "".join(f"{l}\n" for l in pict(path)), encoding="utf-8", newline="\n")
        dest.with_name(dest.name + ".frames").write_text(
            "".join(f"{l}\n" for l in frames(path)), encoding="utf-8", newline="\n")
        dest.with_name(dest.name + ".delay").write_text(
            f"{delay(path)}\n", encoding="utf-8", newline="\n")
        dest.with_name(dest.name + ".mmco").write_text(
            "".join(f"{l}\n" for l in mmco(path)), encoding="utf-8", newline="\n")
        count += 1
        print(f"{rel}: {len(sizes)} packets", flush=True)
    print(f"{count} streams")


if __name__ == "__main__":
    main()
