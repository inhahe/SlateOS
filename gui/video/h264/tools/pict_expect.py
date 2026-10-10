"""Write what FFmpeg makes of H.264 streams' pictures, for
src/decoder/tests.rs to hold the decoder's handling of pictures to.

    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> [--jobs N]
    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> --add-crc [--crc-wsl DIR]
    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> --add-size

`ffmpeg` (built with libx264) and `ffprobe` must be on PATH. The streams,
each named by its path under <out>:

  h264-conformance/...  every conformance stream FATE has, read from
                        <fate-suite>;
  h264/...              FATE's other H.264 samples: raw ones read from
                        <fate-suite>, those in containers as the packets
                        FFmpeg's demuxer reads, written here end to end as
                        `<name>.data`;
  x264/...              streams made here with libx264, some with their
                        picture timing SEI rewritten, for what no sample
                        has: every pic_struct after every other, soft
                        telecine, open GOPs, intra refresh;
  cut/...               each raw stream above begun somewhere other than its
                        start -- at a P picture, a B picture, a non-IDR I
                        picture, a recovery point -- with the parameter sets
                        before that point put in front; and field streams
                        with the second field of one frame and the first of
                        the next taken out. How a decoder joins a stream
                        late, recovers, and refuses to pair fields of two
                        frames: no stream that starts at an IDR shows any
                        of it.

Generated streams are written under <out> by their names. For each stream:

  .packets    each packet FFmpeg's demuxer (and for a raw stream its
              parser) splits the stream into, one a line: its size and its
              position, which ffprobe gives each picture as the position of
              the packet that began it;
  .extradata  the extradata FFmpeg opens the decoder that shows the stream
              with, if any: a container's, or for a raw stream the
              parameter sets its probe took from the first packet that had
              them;
  .pict       FFmpeg's `-debug pict` line for each slice, without its
              `[h264 @ ...]` prefix: where it starts, its picture's frame
              number and order counts, its reference counts, QP, loop filter
              and weights. One thread, so that the lines come in decoding
              order. FFmpeg first decodes the stream's first pictures with a
              decoder of its own to learn its format; only what is logged
              after it says probing is over is kept here;
  .probe      for a raw stream, what the test needs to probe it as FFmpeg
              does: how many packets probing read, offering each to that
              probe decoder until it has the stream's size and format and
              takes its delay as known; and for each of those packets `B`
              where FFmpeg's parser took it for a B picture, which makes
              libavformat set the probe's delay to 1 if it was 0, or `.`.
              The probe is opened with no extradata (before any packet is
              read) and starts holding back no pictures; the delay it ends
              with is the one the decoder that shows the stream starts from
              (`.delay`). The test probes the same way and must end with the
              same delay;
  .frames     each picture FFmpeg hands out, in order: the position of the
              packet that began it, its picture type, and whether it is a
              key frame, interlaced, top field first, and how many fields
              it repeats -- ffprobe's view of the output;
  .delay      how many pictures FFmpeg's decoder holds back from the start
              (`has_b_frames`): what probing found;
  .mmco       FFmpeg's `-debug mmco` lines without their pointers: the
              short- and long-term reference lists as each slice's lists
              are built and after each picture is marked, and each marking
              operation as it is carried out.
  .crc        written by `--add-crc` over streams already described: each
              picture FFmpeg hands out, summed as its `framecrc` muxer sums
              it (see `framecrc`) -- from FFmpeg 8.1.3's own build in WSL
              when `--crc-wsl` names its directory, and then `<size>, ?`
              for a picture with samples FFmpeg never wrote, whose content
              FFmpeg leaves undefined.
  .size       written by `--add-size`: the width and height probing finds,
              which FFmpeg's command line opens the decoder with.
"""
import base64
import json
import re
import shlex
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

SKIP_SUFFIXES = {".md5", ".txt", ".yuv"}
CONTAINERS = {".avi", ".flv", ".mkv", ".mov", ".mp4", ".mxf", ".ts"}
# Packets kept after the point a stream is cut at: enough for a recovery
# point's count to run out and the reorder delay to settle.
CUT_LENGTH = 120
START_CODE = b"\x00\x00\x00\x01"


def run(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(args, capture_output=True, text=True, encoding="utf-8",
                          errors="replace", check=False)


def is_container(path: Path) -> bool:
    return path.suffix.lower() in CONTAINERS


def raw_format(path: Path) -> list[str]:
    """A raw stream read as H.264 whatever its content looks like: FFmpeg
    takes every sample for H.264 by itself, but not a stream cut so short
    that it holds no IDR and only a few slices."""
    return [] if is_container(path) else ["-f", "h264"]


def ffprobe(path: Path, *args: str) -> dict:
    out = run(["ffprobe", "-v", "error", *args, "-of", "json", *raw_format(path),
               str(path)]).stdout
    return json.loads(out or "{}")


def packets(path: Path, container: bool) -> tuple[list[tuple[int, int]], bytes]:
    """Each video packet's size and position; for a container, also the
    packets' bytes end to end. A raw stream's packets must tile the file."""
    entries = "packet=pos,size" + (",data" if container else "")
    args = ["-select_streams", "v:0", "-show_entries", entries]
    if container:
        args += ["-show_data", "-data_dump_format", "base64"]
    rows, data = [], bytearray()
    expect_pos = 0
    for p in ffprobe(path, *args).get("packets", []):
        size = int(p["size"])
        pos = int(p.get("pos", -1))
        if container:
            chunk = base64.b64decode("".join(p.get("data", "").split()))
            if len(chunk) != size:
                raise SystemExit(f"{path}: packet at {pos} has {len(chunk)} of {size} bytes")
            data += chunk
        elif pos != expect_pos:
            raise SystemExit(f"{path}: packet at {pos}, expected {expect_pos}")
        rows.append((size, pos))
        expect_pos = pos + size
    return rows, bytes(data)


def extradata(path: Path) -> bytes:
    streams = ffprobe(path, "-select_streams", "v:0", "-show_entries", "stream=extradata",
                      "-show_data", "-data_dump_format", "base64").get("streams", [])
    text = streams[0].get("extradata", "") if streams else ""
    return base64.b64decode("".join(text.split()))


PROBED = "After avformat_find_stream_info()"


PROBE_READ = re.compile(r"frames:(\d+)")


def decoder_lines(path: Path, debug: str) -> tuple[list[str], list[str], int]:
    """What FFmpeg's H.264 decoders log with `-debug <debug>`, each line
    without its `[h264 @ ...]` prefix: first the probe's -- the decoder
    FFmpeg decodes the first pictures with to learn the stream's format,
    which logs before FFmpeg says probing is over -- then the decoder's that
    shows the stream; and how many packets probing read, each of which it
    offered the probe (`count` in `avformat_find_stream_info`).

    ffprobe, not ffmpeg, decodes for these lines: ffmpeg's demuxer,
    decoder, filter and muxer are threads that log independently, and a
    message the decoder logs while another thread is part way through a
    line comes out without its prefix -- lost to any reading of the log.
    ffprobe decodes in one thread. Repeated lines are not folded."""
    err = run(["ffprobe", "-loglevel", "repeat+debug", "-threads", "1", "-debug", debug,
               *raw_format(path), "-i", str(path), "-select_streams", "v:0",
               "-show_entries", "frame=pkt_pos", "-of", "csv"]).stderr.splitlines()
    start = next((i for i, l in enumerate(err) if PROBED in l), None)
    if start is None:
        raise SystemExit(f"{path}: FFmpeg did not log the end of probing")
    read = PROBE_READ.search(err[start])
    if read is None:
        raise SystemExit(f"{path}: no packet count where probing ends: {err[start]}")
    # A raw stream's demuxer logs as "h264" too; its lines are told from the
    # decoders' by its address, the one the end of probing is logged with.
    demuxer = err[start][:err[start].find("]") + 1]

    def own(lines: list[str]) -> list[str]:
        return [l[l.find("] ") + 2:] for l in lines
                if l.startswith("[h264 @") and not l.startswith(demuxer)]

    return own(err[:start]), own(err[start + 1:]), int(read.group(1))


def frames(path: Path) -> list[str]:
    out = ffprobe(path, "-threads", "1", "-select_streams", "v:0", "-show_frames",
                  "-show_entries",
                  "frame=pkt_pos,pict_type,key_frame,interlaced_frame,top_field_first,repeat_pict")
    return [" ".join(str(f.get(k, "-")) for k in (
        "pkt_pos", "pict_type", "key_frame", "interlaced_frame", "top_field_first",
        "repeat_pict")) for f in out.get("frames", [])]


# The lines `-debug mmco` prints about references (h264_refs.c), the rest of
# what that log level brings filtered out.
MMCO_LINE = re.compile(
    r"^(short term list:|long term list:|no mmco here|mmco:-?\d+ -?\d+ -?\d+|"
    r"mmco: unref short -?\d+ count \d+|mmco: unref long failure|"
    r"remove short -?\d+ count \d+|\d+ fn:-?\d+ poc:-?\d+|\d+ -?\d+)( [0-9a-fA-F]+)?$")


def delay(path: Path) -> int:
    streams = ffprobe(path, "-select_streams", "v:0", "-show_entries",
                      "stream=has_b_frames").get("streams", [])
    return int(streams[0].get("has_b_frames", 0)) if streams else 0


def write_lines(path: Path, lines: list[str]) -> None:
    path.write_text("".join(f"{l}\n" for l in lines), encoding="utf-8", newline="\n")


def sibling(dest: Path, suffix: str) -> Path:
    return dest.with_name(dest.name + suffix)


FAILED: list[str] = []


def describe(name: str, path: Path, container: bool, out: Path) -> list[str]:
    """Write FFmpeg's view of one stream under <out>/<name>; give back its
    slice lines (those of the decoder that shows it). A stream FFmpeg makes
    nothing of is reported and left out, its partial lines removed, rather
    than ending the run."""
    try:
        return describe_one(name, path, container, out)
    except SystemExit as e:
        for leftover in (out / name).parent.glob(f"{(out / name).name}.*"):
            leftover.unlink()
        print(f"FAILED {name}: {e}", flush=True)
        FAILED.append(name)
        return []


def describe_one(name: str, path: Path, container: bool, out: Path) -> list[str]:
    dest = out / name
    dest.parent.mkdir(parents=True, exist_ok=True)
    rows, data = packets(path, container)
    if not container and sum(s for s, _ in rows) != path.stat().st_size:
        print(f"{name}: packets cover {sum(s for s, _ in rows)} of {path.stat().st_size} bytes",
              flush=True)
    write_lines(sibling(dest, ".packets"), [f"{s} {p}" for s, p in rows])
    if container:
        sibling(dest, ".data").write_bytes(data)
    extra = extradata(path)
    if extra:
        sibling(dest, ".extradata").write_bytes(extra)
    _, main, read = decoder_lines(path, "pict")
    slices = [l for l in main if l.startswith("slice:")]
    write_lines(sibling(dest, ".pict"), slices)
    if not container:
        raw = path.read_bytes()
        seen_b = "".join("B" if parser_saw_b(raw[at:at + size]) else "."
                         for size, at in rows[:read])
        write_lines(sibling(dest, ".probe"), [str(read), seen_b])
    write_lines(sibling(dest, ".frames"), frames(path))
    write_lines(sibling(dest, ".delay"), [str(delay(path))])
    _, main, _ = decoder_lines(path, "mmco")
    write_lines(sibling(dest, ".mmco"),
                [m.group(1) for m in map(MMCO_LINE.match, main) if m])
    print(f"{name}: {len(rows)} packets, {len(slices)} slices", flush=True)
    return slices


# --- NAL units, as far as cutting a stream needs them -----------------------


@dataclass
class Nal:
    type: int
    ref_idc: int
    raw: bytes  # from the header byte, trailing zero bytes trimmed


def nal_units(buf: bytes) -> list[Nal]:
    starts = []
    i = 0
    while (j := buf.find(b"\x00\x00\x01", i)) >= 0:
        starts.append(j + 3)
        i = j + 3
    nals = []
    for k, s in enumerate(starts):
        e = starts[k + 1] - 3 if k + 1 < len(starts) else len(buf)
        raw = buf[s:e].rstrip(b"\x00")
        if raw:
            nals.append(Nal(raw[0] & 31, (raw[0] >> 5) & 3, raw))
    return nals


def unescape(b: bytes) -> bytes:
    out, zeros = bytearray(), 0
    for x in b:
        if zeros >= 2 and x == 3:
            zeros = 0
            continue
        out.append(x)
        zeros = zeros + 1 if x == 0 else 0
    return bytes(out)


class BitReader:
    def __init__(self, data: bytes):
        self.data, self.at = data, 0

    def bit(self) -> int:
        byte = self.data[self.at >> 3] if self.at >> 3 < len(self.data) else 0
        self.at += 1
        return (byte >> (7 - ((self.at - 1) & 7))) & 1

    def ue(self) -> int:
        zeros = 0
        while self.bit() == 0 and zeros < 32:
            zeros += 1
        value = 0
        for _ in range(zeros):
            value = value << 1 | self.bit()
        return (1 << zeros) - 1 + value


def parser_saw_b(packet: bytes) -> bool:
    """Whether FFmpeg's H.264 parser takes the packet for a B picture: the
    type of its first slice (`parse_nal_units` in `h264_parser.c`), which
    libavformat reads while probing as a sign that pictures are held back
    (`compute_pkt_fields`)."""
    for n in nal_units(packet):
        if n.type in (1, 5):
            r = BitReader(unescape(n.raw[1:]))
            r.ue()  # first_mb_in_slice
            return r.ue() % 5 == 1
    return False


def sei_types(nal: Nal) -> list[int]:
    """The payload types of an SEI NAL unit's messages."""
    rbsp = unescape(nal.raw[1:])
    i, types = 0, []
    while i < len(rbsp) and not (rbsp[i] == 0x80 and i == len(rbsp) - 1):
        vals = []
        for _ in range(2):
            v = 0
            while i < len(rbsp) and rbsp[i] == 0xFF:
                v += 255
                i += 1
            if i >= len(rbsp):
                return types
            vals.append(v + rbsp[i])
            i += 1
        types.append(vals[0])
        i += vals[1]
    return types


SLICE_LINE = re.compile(r"^slice:\d+ ([FTB]) mb:\d+ ([IPBip])( fix)?( IDR)? frame:(-?\d+) ")


@dataclass
class Packet:
    pos: int
    size: int
    nals: list[Nal]
    structure: str = ""
    types: set = field(default_factory=set)
    idr: bool = False
    frame_num: int = -1
    ref: int = 0
    recovery: bool = False


def classify(data: bytes, sizes: list[int], slices: list[str]) -> list[Packet] | None:
    """Each packet's picture as FFmpeg's slice lines describe it, or None
    where the lines cannot be laid over the packets (a slice FFmpeg skips
    or refuses prints none)."""
    out, pos, k = [], 0, 0
    for size in sizes:
        p = Packet(pos, size, nal_units(data[pos:pos + size]))
        pos += size
        vcl = [n for n in p.nals if n.type in (1, 5)]
        for n, line in zip(vcl, slices[k:k + len(vcl)]):
            m = SLICE_LINE.match(line)
            if not m:
                return None
            if not p.structure:
                p.structure, p.frame_num = m.group(1), int(m.group(5))
            p.types.add(m.group(2))
            p.idr |= n.type == 5
            p.ref = max(p.ref, n.ref_idc)
        k += len(vcl)
        p.recovery = any(6 in sei_types(n) for n in p.nals if n.type == 6)
        out.append(p)
    return out if k == len(slices) else None


def cut_points(pk: list[Packet]) -> list[tuple[str, int]]:
    """Where to begin the stream other than at its start: the first
    reference P picture, the first B, the first non-IDR I, the first
    recovery point."""
    tests = [
        ("p", lambda p: "P" in p.types and p.ref > 0),
        ("b", lambda p: "B" in p.types),
        ("i", lambda p: p.types == {"I"}),
        ("r", lambda p: p.recovery),
    ]
    points = []
    for kind, test in tests:
        k = next((k for k in range(1, len(pk)) if not pk[k].idr and pk[k].types and test(pk[k])),
                 None)
        if k is not None:
            points.append((kind, k))
    return points


def field_drop(pk: list[Packet]) -> int | None:
    """A reference field pair followed by a pair of another frame, whose
    middle two fields are to go: what is left is a field followed by one of
    the other parity from another frame, which must not be paired with it."""
    for i in range(1, len(pk) - 3):
        a, b, c, d = pk[i:i + 4]
        if (all(p.structure in "TB" and p.types for p in (a, b, c, d))
                and a.structure != b.structure and a.frame_num == b.frame_num
                and c.structure != d.structure and c.frame_num == d.frame_num
                and c.frame_num != a.frame_num and a.structure != d.structure and a.ref > 0):
            return i
    return None


def cuts(name: str, data: bytes, sizes: list[int], slices: list[str],
         out: Path) -> list[tuple[str, Path]]:
    """Write the cut versions of a raw stream; give back their names and
    paths."""
    pk = classify(data, sizes, slices)
    if pk is None:
        print(f"{name}: slices do not map onto packets; not cut", flush=True)
        return []
    made = []
    for kind, k in cut_points(pk):
        params = b"".join(START_CODE + n.raw for p in pk[:k] for n in p.nals if n.type in (7, 8))
        end = pk[min(k + CUT_LENGTH, len(pk)) - 1]
        body = data[pk[k].pos:end.pos + end.size]
        cut = f"cut/{name}~{kind}{k}.h264"
        (out / cut).parent.mkdir(parents=True, exist_ok=True)
        (out / cut).write_bytes(params + body)
        made.append((cut, out / cut))
    if (i := field_drop(pk)) is not None:
        end = pk[min(i + 3 + CUT_LENGTH, len(pk)) - 1]
        body = data[:pk[i + 1].pos] + data[pk[i + 3].pos:end.pos + end.size]
        cut = f"cut/{name}~f{i + 1}.h264"
        (out / cut).parent.mkdir(parents=True, exist_ok=True)
        (out / cut).write_bytes(body)
        made.append((cut, out / cut))
    return made


# --- Streams made with libx264 ----------------------------------------------

# x264's picture timing SEI with no HRD, after its start code: one byte,
# pic_struct in its top four bits, then one clock_timestamp_flag (0) for each
# of pic_struct's clock timestamps, then the payload's alignment bits.
X264_PIC_TIMING = b"\x00\x00\x01\x06\x01\x01\x04\x80"
PIC_STRUCT_BYTE = {0: 0x04, 1: 0x14, 2: 0x24, 3: 0x32, 4: 0x42, 5: 0x51, 6: 0x61, 7: 0x72,
                   8: 0x81}

# Every pic_struct after every other that changes what a frame is flagged
# with: top and bottom first frames (3, 4) after an interlaced frame and
# after a progressive one -- FFmpeg carries interlacing over to them, to
# flag soft telecine -- telecined frames (5, 6), doubled and tripled ones
# (7, 8), lone fields (1, 2).
PATTERN = [3, 3, 4, 0, 4, 1, 4, 2, 3, 5, 3, 6, 4, 7, 3, 8, 4, 3, 0, 5, 4, 6, 3, 4,
           1, 0, 2, 3, 7, 4, 8, 0]
# 3:2 pulldown as x264's --pulldown 32 writes it.
PULLDOWN = [5, 4, 6, 3]

X264 = [
    # name, frames, x264 parameters, pic_struct pattern
    ("pic-struct", 64, "pic-struct=1:bframes=0:keyint=250", PATTERN),
    ("pulldown", 40, "pic-struct=1:bframes=0:keyint=250", PULLDOWN),
    ("pic-struct-bframes", 64, "pic-struct=1:bframes=2:keyint=250", PATTERN),
    ("open-gop", 72, "open-gop=1:keyint=12:min-keyint=12:scenecut=0:bframes=3:b-pyramid=normal",
     None),
    ("intra-refresh", 72, "intra-refresh=1:keyint=10:bframes=0", None),
    ("intra-refresh-bframes", 72, "intra-refresh=1:keyint=16:bframes=2", None),
]


def make_x264(out: Path) -> list[tuple[str, Path]]:
    made = []
    for name, count, params, pattern in X264:
        dest = out / "x264" / f"{name}.h264"
        dest.parent.mkdir(parents=True, exist_ok=True)
        r = run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
                 "testsrc2=size=176x144:rate=25", "-frames:v", str(count), "-pix_fmt",
                 "yuv420p", "-c:v", "libx264", "-threads", "1", "-x264-params", params,
                 "-f", "h264", str(dest)])
        if r.returncode != 0:
            raise SystemExit(f"x264 {name}: {r.stderr}")
        if pattern:
            # Rewritten in decoding order, which is the order of the SEI
            # messages in the file.
            data = dest.read_bytes()
            pieces = data.split(X264_PIC_TIMING)
            if len(pieces) < 2:
                raise SystemExit(f"x264 {name}: no picture timing SEI to rewrite")
            rebuilt = bytearray(pieces[0])
            for k, piece in enumerate(pieces[1:]):
                rebuilt += bytes([0, 0, 1, 6, 1, 1, PIC_STRUCT_BYTE[pattern[k % len(pattern)]],
                                  0x80])
                rebuilt += piece
            dest.write_bytes(bytes(rebuilt))
        made.append((f"x264/{name}.h264", dest))
    return made


def sources(fate: Path) -> list[tuple[str, Path, bool]]:
    """(name, path, in a container) of every FATE stream to describe."""
    found = []
    for sub in ("h264-conformance", "h264"):
        root = fate / sub
        for p in sorted(q for q in root.rglob("*") if q.is_file()):
            if p.suffix.lower() in SKIP_SUFFIXES or p.name == "md5sum":
                continue
            found.append((f"{sub}/{p.relative_to(root).as_posix()}", p,
                          p.suffix.lower() in CONTAINERS))
    return found


# Where FFmpeg 8.1.3's own build is, in WSL, when `--crc-wsl <dir>` names
# it: the `.crc` lines are then that build's (`tools/ffdbg/build.sh`).
CRC_WSL: str | None = None


def wsl_path(p: Path) -> str:
    s = p.resolve().as_posix()
    return f"/mnt/{s[0].lower()}{s[2:]}"


# FFmpeg's options for summing a stream's pictures (see `framecrc`), around
# its `-i`. -noautoscale: a picture of a new size is summed as the decoder
# made it, not scaled back to the first picture's size for the encoder.
CRC_HEAD = ["-hide_banner", "-loglevel", "error", "-cpuflags", "0", "-threads", "1",
            "-flags", "unaligned"]
CRC_TAIL = ["-map", "0:v:0", "-fps_mode", "passthrough", "-noautoscale", "-f", "framecrc", "-"]


def crc_lines(out: str) -> list[str]:
    """The size and sum of each picture in a `framecrc` muxer's output."""
    return [", ".join(f.strip() for f in l.split(",")[4:6])
            for l in out.splitlines() if l.strip() and not l.startswith("#")]


def framecrc(path: Path) -> list[str]:
    """Each picture FFmpeg decodes, as its `framecrc` muxer sums it: the
    size and Adler-32 of its planes end to end, cropped, rows unpadded,
    16-bit samples little-endian. FFmpeg's C routines only (`-cpuflags 0`),
    one thread, every picture kept (`-fps_mode passthrough`) at the size it
    was decoded (`-noautoscale`), and cropped exactly as the SPS says
    (`-flags unaligned`: otherwise FFmpeg keeps a left crop that would
    leave its planes unaligned short, and the picture wider). This is the
    FFmpeg on PATH; `wsl_framecrc` is the one `.crc` comes from."""
    return crc_lines(run(["ffmpeg", *CRC_HEAD, *raw_format(path), "-i", str(path),
                          *CRC_TAIL]).stdout)


def wsl_framecrc(paths: list[Path], scratch: Path, jobs: int) -> list[list[str]]:
    """`framecrc` for each of `paths` from FFmpeg 8.1.3's own build in WSL,
    in `CRC_WSL` (`tools/ffdbg/build.sh`): the release this decoder is a
    translation of, built with no assembly at all. Another build parts from
    it where FFmpeg has changed since -- the March 2026 snapshot `.crc` was
    first made with shows four cut streams' pictures differently.

    That build fills every picture it starts with one byte; each stream is
    run with one byte and then with another. A picture whose sum moves with
    the byte holds samples FFmpeg never wrote -- handed out undecoded, or
    part of it reached by no slice -- which in an ordinary build are
    whatever memory its buffer pool hands back, and differ from run to run:
    it is `<size>, ?`.

    The runs are one bash script, `jobs` chains of them at once, started by
    a single `wsl.exe`: with ten `wsl.exe` at once, some exit -1 partway
    through their output."""
    # A build that does not run would sum nothing, which some streams also
    # sum -- FFmpeg shows none of a stream it never recovers in.
    subprocess.run(["wsl", "-d", "Ubuntu", "--exec", f"{CRC_WSL}/ffmpeg", "-hide_banner",
                    "-version"], stdin=subprocess.DEVNULL, capture_output=True, check=True)
    scratch.mkdir(parents=True, exist_ok=True)
    chains: list[list[str]] = [[] for _ in range(jobs)]
    for i, path in enumerate(paths):
        for fill in (0, 255):
            args = [f"FFDBG_FILL={fill}", f"{CRC_WSL}/ffmpeg", "-nostdin", *CRC_HEAD,
                    *raw_format(path), "-i", wsl_path(path), *CRC_TAIL]
            target = wsl_path(scratch / f"{i}.{fill}")
            chains[i % jobs].append(" ".join(shlex.quote(a) if not a.startswith("FFDBG_") else a
                                             for a in args) + f" > {shlex.quote(target)} 2>/dev/null")
    script = ["#!/bin/bash"]
    for chain in chains:
        if chain:
            script.append("(\n" + "\n".join(chain) + "\n) &")
    script.append("wait")
    script_file = scratch / "run.sh"
    script_file.write_bytes(("\n".join(script) + "\n").encode())
    subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", wsl_path(script_file)],
                   stdin=subprocess.DEVNULL, check=True)
    result = []
    for i, path in enumerate(paths):
        a, b = (crc_lines((scratch / f"{i}.{fill}").read_text(encoding="utf-8", errors="replace"))
                for fill in (0, 255))
        if len(a) != len(b):
            raise SystemExit(f"{path}: {len(a)} pictures with one fill, {len(b)} with the other")
        result.append([x if x == y else f"{x.split(',')[0]}, ?" for x, y in zip(a, b)])
    for f in scratch.iterdir():
        f.unlink()
    scratch.rmdir()
    return result


def add_crc(fate: Path, out: Path, jobs: int) -> None:
    """`.crc` beside every stream already described under `out`: the
    stream made here if it is there, else the FATE sample of its name."""
    names = sorted(str(p.relative_to(out).as_posix())[:-len(".packets")]
                   for p in out.rglob("*.packets"))
    paths = [(out / n) if (out / n).is_file() else fate / n for n in names]
    if CRC_WSL:
        sums = wsl_framecrc(paths, out / ".crc-scratch", jobs)
    else:
        with ThreadPoolExecutor(jobs) as pool:
            sums = list(pool.map(framecrc, paths))
    # The pictures summed are the pictures ffprobe saw handed out (`.frames`):
    # a count that differs is a run that failed, or a build that decodes
    # differently from the one `.frames` came from.
    wrong = []
    for name, lines in zip(names, sums):
        shown = sibling(out / name, ".frames").read_text(encoding="utf-8").splitlines()
        if len(lines) != len(shown):
            wrong.append(f"{name}: {len(lines)} pictures summed, {len(shown)} in .frames")
    if wrong:
        raise SystemExit("\n".join(wrong))
    for name, lines in zip(names, sums):
        write_lines(sibling(out / name, ".crc"), lines)
    print(f"{len(names)} streams summed")


def add_size(fate: Path, out: Path, jobs: int) -> None:
    """`.size` beside every stream already described under `out`: the
    width and height probing finds, which FFmpeg's command line opens the
    decoder that shows the stream with (`width_from_caller`)."""
    names = sorted(str(p.relative_to(out).as_posix())[:-len(".packets")]
                   for p in out.rglob("*.packets"))

    def one(name: str) -> None:
        made = out / name
        path = made if made.is_file() else fate / name
        got = run(["ffprobe", "-v", "error", *raw_format(path), "-select_streams", "v:0",
                   "-show_entries", "stream=width,height", "-of", "csv=p=0", str(path)]).stdout
        w, _, h = got.strip().partition(",")
        if w.isdigit() and h.isdigit() and int(w) and int(h):
            write_lines(sibling(out / name, ".size"), [f"{w} {h}"])

    with ThreadPoolExecutor(jobs) as pool:
        list(pool.map(one, names))
    print(f"{len(names)} streams sized")


def main() -> None:
    args = sys.argv[1:]
    jobs = 4
    if "--jobs" in args:
        at = args.index("--jobs")
        jobs = int(args[at + 1])
        del args[at:at + 2]
    crc_only = "--add-crc" in args
    if crc_only:
        args.remove("--add-crc")
    if "--crc-wsl" in args:
        global CRC_WSL
        at = args.index("--crc-wsl")
        CRC_WSL = args[at + 1]
        del args[at:at + 2]
    size_only = "--add-size" in args
    if size_only:
        args.remove("--add-size")
    fate, out = Path(args[0]), Path(args[1])
    if crc_only:
        add_crc(fate, out, jobs)
        return
    if size_only:
        add_size(fate, out, jobs)
        return
    streams = sources(fate) + [(n, p, False) for n, p in make_x264(out)]

    def base(item: tuple[str, Path, bool]) -> list[tuple[str, Path]]:
        name, path, container = item
        slices = describe(name, path, container, out)
        if container or name in FAILED:
            return []
        data = path.read_bytes()
        sizes = [int(l.split()[0]) for l in
                 sibling(out / name, ".packets").read_text(encoding="utf-8").splitlines()]
        return cuts(name, data, sizes, slices, out)

    with ThreadPoolExecutor(jobs) as pool:
        cut_streams = [c for made in pool.map(base, streams) for c in made]
        list(pool.map(lambda c: describe(c[0], c[1], False, out), cut_streams))
    print(f"{len(streams)} streams, {len(cut_streams)} cut from them")
    if FAILED:
        raise SystemExit(f"{len(FAILED)} streams FFmpeg made nothing of: {', '.join(FAILED)}")


if __name__ == "__main__":
    main()
