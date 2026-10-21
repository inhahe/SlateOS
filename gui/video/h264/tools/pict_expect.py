"""Write what FFmpeg makes of H.264 streams' pictures, for
src/decoder/tests.rs to hold the decoder's handling of pictures to.

    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> [--jobs N] [--ffmpeg DIR]
    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> --add-crc [...]
    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> --add-size [...]
    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> --add-newextra [...]
    python gui/video/h264/tools/pict_expect.py <fate-suite> <out> --add-mmco [...]

Everything written here is FFmpeg 8.1.3's -- the release the decoder is a
translation of -- from its own build in WSL (`tools/ffdbg/build.sh`, in
DIR, by default ~/ffdbg/src there), with ffprobe: what another build makes
of a damaged or cut stream can differ. `ffmpeg` on PATH, built with
libx264, only encodes the x264 streams. The streams, each named by its path
under <out>:

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
  damaged/...           each raw stream above with one picture damaged: a
                        slice dropped, cut short, or with bytes changed
                        (see `damages`). How the decoder finds the damage,
                        stops, and conceals what it lost.
  grain/...             each raw stream above with film grain
                        characteristics put in, before every picture or
                        every fifth (see `grains`): the grain each picture
                        is shown with, and the pictures FFmpeg cannot give
                        any (not 8-bit 4:2:0) shown without.

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
              operation as it is carried out. `--add-mmco` writes them
              afresh over streams already described.
  .crc        written by `--add-crc` over streams already described: each
              picture FFmpeg's decoder hands out, summed as its `framecrc`
              muxer sums a picture (see `framecrcs`), and `<size>, ?` for a
              picture with samples FFmpeg never wrote, whose content FFmpeg
              leaves undefined.
  .size       written by `--add-size`: the width and height probing finds,
              which FFmpeg's command line opens the decoder with.
  .newextra   written by `--add-newextra`, for a container stream that has
              any: each packet that brings new extradata with it (an MP4
              whose sample description changes), as the decoder that shows
              the stream takes it up -- the packet's index, then the bytes
              in hex, a line each (`FFDBG_NEWEXTRA`).
"""
import base64
import json
import random
import re
import shlex
import subprocess
import sys
import zlib
from dataclasses import dataclass, field
from pathlib import Path

SKIP_SUFFIXES = {".md5", ".txt", ".yuv"}
CONTAINERS = {".avi", ".flv", ".mkv", ".mov", ".mp4", ".mxf", ".ts"}
# Packets kept after the point a stream is cut at: enough for a recovery
# point's count to run out and the reorder delay to settle.
CUT_LENGTH = 120
START_CODE = b"\x00\x00\x00\x01"


def run(args: list[str]) -> subprocess.CompletedProcess:
    """A program here (the x264 encoding), its output as text."""
    return subprocess.run(args, capture_output=True, text=True, encoding="utf-8",
                          errors="replace", check=False)


# Where FFmpeg 8.1.3's build is in WSL (`--ffmpeg`).
FFMPEG = ""


def wsl_path(p: Path) -> str:
    s = p.resolve().as_posix()
    return f"/mnt/{s[0].lower()}{s[2:]}"


@dataclass
class Result:
    """What a command run in WSL wrote, as text, and how it exited."""
    out: str
    err: str
    status: int


ASSIGNMENT = re.compile(r"^[A-Z][A-Z0-9_]*=[A-Za-z0-9_]*$")


def run_all(commands: list[list[str]], scratch: Path, jobs: int) -> list[Result]:
    """Each of `commands` run in WSL: one bash script, `jobs` chains of
    them at once, started by a single `wsl.exe` -- with ten `wsl.exe` at
    once, some exit -1 partway through their output. A command may begin
    with variable assignments (`FFDBG_FILL=0`). Each one's output goes to
    files under `scratch`, read back and removed."""
    if not commands:
        return []
    # A build that does not run would give nothing, which some streams also
    # give -- FFmpeg shows none of a stream it never recovers in.
    subprocess.run(["wsl", "-d", "Ubuntu", "--exec", f"{FFMPEG}/ffprobe", "-hide_banner",
                    "-version"], stdin=subprocess.DEVNULL, capture_output=True, check=True)
    scratch.mkdir(parents=True, exist_ok=True)
    chains: list[list[str]] = [[] for _ in range(jobs)]
    for k, argv in enumerate(commands):
        words, assigning = [], True
        for a in argv:
            assigning = assigning and bool(ASSIGNMENT.match(a))
            words.append(a if assigning else shlex.quote(a))
        target = wsl_path(scratch / str(k))
        chains[k % jobs].append(f"{' '.join(words)} > {shlex.quote(target + '.out')} "
                                f"2> {shlex.quote(target + '.err')}; "
                                f"echo $? > {shlex.quote(target + '.rc')}")
    script = ["#!/bin/bash"]
    for chain in chains:
        if chain:
            script.append("(\n" + "\n".join(chain) + "\n) &")
    script.append("wait")
    script_file = scratch / "run.sh"
    script_file.write_bytes(("\n".join(script) + "\n").encode())
    subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", wsl_path(script_file)],
                   stdin=subprocess.DEVNULL, check=True)
    results = []
    for k in range(len(commands)):
        read = lambda ext: (scratch / f"{k}.{ext}").read_bytes().decode("utf-8", "replace")
        results.append(Result(read("out"), read("err"), int(read("rc").strip() or "-1")))
    for f in scratch.iterdir():
        f.unlink()
    scratch.rmdir()
    return results


def is_container(path: Path) -> bool:
    return path.suffix.lower() in CONTAINERS


def raw_format(path: Path) -> list[str]:
    """A raw stream read as H.264 whatever its content looks like: FFmpeg
    takes every sample for H.264 by itself, but not a stream cut so short
    that it holds no IDR and only a few slices."""
    return [] if is_container(path) else ["-f", "h264"]


def ffprobe(path: Path, *args: str) -> list[str]:
    """An ffprobe run on `path` writing JSON."""
    return [f"{FFMPEG}/ffprobe", "-v", "error", *args, "-of", "json", *raw_format(path),
            wsl_path(path)]


def json_of(r: Result) -> dict:
    """ffprobe's JSON; output it cut short fails the stream, not the run."""
    try:
        return json.loads(r.out or "{}")
    except ValueError as e:
        raise SystemExit(f"ffprobe wrote no JSON ({e}), exit {r.status}: {r.err[-300:]}") from e


def packets_command(path: Path, container: bool) -> list[str]:
    entries = "packet=pos,size" + (",data" if container else "")
    args = ["-select_streams", "v:0", "-show_entries", entries]
    if container:
        args += ["-show_data", "-data_dump_format", "base64"]
    return ffprobe(path, *args)


def packets(path: Path, container: bool, r: Result) -> tuple[list[tuple[int, int]], bytes]:
    """Each video packet's size and position; for a container, also the
    packets' bytes end to end. A raw stream's packets must tile the file."""
    rows, data = [], bytearray()
    expect_pos = 0
    for p in json_of(r).get("packets", []):
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


def extradata_command(path: Path) -> list[str]:
    return ffprobe(path, "-select_streams", "v:0", "-show_entries", "stream=extradata",
                   "-show_data", "-data_dump_format", "base64")


def extradata(r: Result) -> bytes:
    streams = json_of(r).get("streams", [])
    text = streams[0].get("extradata", "") if streams else ""
    return base64.b64decode("".join(text.split()))


PROBED = "After avformat_find_stream_info()"


PROBE_READ = re.compile(r"frames:(\d+)")


def decoder_lines_command(path: Path, debug: str) -> list[str]:
    """ffprobe, not ffmpeg, decodes for these lines: ffmpeg's demuxer,
    decoder, filter and muxer are threads that log independently, and a
    message the decoder logs while another thread is part way through a
    line comes out without its prefix -- lost to any reading of the log.
    ffprobe decodes in one thread. Repeated lines are not folded."""
    return [f"{FFMPEG}/ffprobe", "-loglevel", "repeat+debug", "-threads", "1", "-debug", debug,
            *raw_format(path), "-i", wsl_path(path), "-select_streams", "v:0",
            "-show_entries", "frame=pkt_pos", "-of", "csv"]


def decoder_lines(path: Path, r: Result) -> tuple[list[str], list[str], int]:
    """What FFmpeg's H.264 decoders logged with `-debug`, each line
    without its `[h264 @ ...]` prefix: first the probe's -- the decoder
    FFmpeg decodes the first pictures with to learn the stream's format,
    which logs before FFmpeg says probing is over -- then the decoder's that
    shows the stream; and how many packets probing read, each of which it
    offered the probe (`count` in `avformat_find_stream_info`)."""
    err = r.err.splitlines()
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


def frames_command(path: Path) -> list[str]:
    return ffprobe(path, "-threads", "1", "-select_streams", "v:0", "-show_frames",
                   "-show_entries",
                   "frame=pkt_pos,pict_type,key_frame,interlaced_frame,top_field_first,repeat_pict")


def frames(r: Result) -> list[str]:
    return [" ".join(str(f.get(k, "-")) for k in (
        "pkt_pos", "pict_type", "key_frame", "interlaced_frame", "top_field_first",
        "repeat_pict")) for f in json_of(r).get("frames", [])]


# The lines `-debug mmco` prints about references (h264_refs.c), the rest of
# what that log level brings filtered out -- each without the picture's
# address some end with, which `%p` prints as `0x55d5c3a0b1c0` or `(nil)` in
# glibc and as `000001F2C3A0B1C0` on Windows.
MMCO_LINE = re.compile(
    r"^(short term list:|long term list:|no mmco here|mmco:-?\d+ -?\d+ -?\d+|"
    r"mmco: unref short -?\d+ count \d+|mmco: unref long failure|"
    r"remove short -?\d+ count \d+|\d+ fn:-?\d+ poc:-?\d+|\d+ -?\d+)"
    r"(?: (?:(?:0x)?[0-9a-fA-F]+|\(nil\)))?$")


def mmco_lines(main: list[str]) -> list[str]:
    """The marking lines among a decoder's `-debug mmco` lines."""
    return [m.group(1) for m in map(MMCO_LINE.match, main) if m]


def delay_command(path: Path) -> list[str]:
    return ffprobe(path, "-select_streams", "v:0", "-show_entries", "stream=has_b_frames")


def delay(r: Result) -> int:
    streams = json_of(r).get("streams", [])
    return int(streams[0].get("has_b_frames", 0)) if streams else 0


def write_lines(path: Path, lines: list[str]) -> None:
    path.write_text("".join(f"{l}\n" for l in lines), encoding="utf-8", newline="\n")


def sibling(dest: Path, suffix: str) -> Path:
    return dest.with_name(dest.name + suffix)


FAILED: list[str] = []


def describe_commands(path: Path, container: bool) -> list[list[str]]:
    """What `describe` asks FFmpeg of a stream, in the order it reads it."""
    return [packets_command(path, container), extradata_command(path),
            decoder_lines_command(path, "pict"), frames_command(path), delay_command(path),
            decoder_lines_command(path, "mmco")]


def describe_all(streams: list[tuple[str, Path, bool]], out: Path,
                 jobs: int) -> dict[str, list[str]]:
    """`describe` every stream, FFmpeg run for them all at once; each one's
    slice lines by its name."""
    commands = [describe_commands(path, container) for _, path, container in streams]
    results = run_all([c for cs in commands for c in cs], out / ".describe-scratch", jobs)
    found, at = {}, 0
    for (name, path, container), cs in zip(streams, commands):
        found[name] = describe(name, path, container, out, results[at:at + len(cs)])
        at += len(cs)
    return found


def describe(name: str, path: Path, container: bool, out: Path, r: list[Result]) -> list[str]:
    """Write FFmpeg's view of one stream under <out>/<name> from what it
    made of it (`r`, as `describe_commands` asked); give back its slice
    lines (those of the decoder that shows it). A stream FFmpeg makes
    nothing of is reported and left out, its partial lines removed, rather
    than ending the run."""
    try:
        return describe_one(name, path, container, out, r)
    except SystemExit as e:
        for leftover in (out / name).parent.glob(f"{(out / name).name}.*"):
            leftover.unlink()
        print(f"FAILED {name}: {e}", flush=True)
        FAILED.append(name)
        return []


def describe_one(name: str, path: Path, container: bool, out: Path,
                 r: list[Result]) -> list[str]:
    dest = out / name
    dest.parent.mkdir(parents=True, exist_ok=True)
    rows, data = packets(path, container, r[0])
    if not container and sum(s for s, _ in rows) != path.stat().st_size:
        print(f"{name}: packets cover {sum(s for s, _ in rows)} of {path.stat().st_size} bytes",
              flush=True)
    write_lines(sibling(dest, ".packets"), [f"{s} {p}" for s, p in rows])
    if container:
        sibling(dest, ".data").write_bytes(data)
    extra = extradata(r[1])
    if extra:
        sibling(dest, ".extradata").write_bytes(extra)
    _, main, read = decoder_lines(path, r[2])
    slices = [l for l in main if l.startswith("slice:")]
    write_lines(sibling(dest, ".pict"), slices)
    if not container:
        raw = path.read_bytes()
        seen_b = "".join("B" if parser_saw_b(raw[at:at + size]) else "."
                         for size, at in rows[:read])
        write_lines(sibling(dest, ".probe"), [str(read), seen_b])
    write_lines(sibling(dest, ".frames"), frames(r[3]))
    write_lines(sibling(dest, ".delay"), [str(delay(r[4]))])
    _, main, _ = decoder_lines(path, r[5])
    write_lines(sibling(dest, ".mmco"), mmco_lines(main))
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


# Packets kept after a damaged one: enough to see what the damage passes on
# through prediction, and where it ends.
DAMAGE_LENGTH = 30


def corrupt(raw: bytes) -> bytes:
    """A NAL unit with a few bytes of its slice data changed, at a third,
    a half and two thirds of its length. A zero byte is left alone, and so
    is a byte after two zeros; no byte is made zero: the unit keeps its
    escaping, and no start code appears in it."""
    out = bytearray(raw)
    for at in sorted({len(raw) // 3, len(raw) // 3 + 7, len(raw) // 2, 2 * len(raw) // 3}):
        if at < 4 or at >= len(out) or out[at] == 0 or (out[at - 1] == 0 and out[at - 2] == 0):
            continue
        out[at] ^= 0x5A if out[at] != 0x5A else 0xA5
    return bytes(out)


def damages(name: str, data: bytes, sizes: list[int], slices: list[str],
            out: Path) -> list[tuple[str, Path]]:
    """Write damaged versions of a raw stream; give back their names and
    paths. Damaged is one P or B picture past the first few -- with pictures
    before it to conceal it from -- its second slice dropped (`d`), its
    first slice cut to half (`t`), or a few bytes of its first slice changed
    (`x`); and one I picture past the first, its first slice cut to half
    (`t`), whose concealment weighs the picture before against the
    neighbours. Each runs from the stream's start to `DAMAGE_LENGTH`
    packets after the damage."""
    pk = classify(data, sizes, slices)
    if pk is None:
        print(f"{name}: slices do not map onto packets; not damaged", flush=True)
        return []
    made = []

    def write(kind: str, k: int, change) -> None:
        vcl = [i for i, n in enumerate(pk[k].nals) if n.type in (1, 5)]
        nals = change(list(pk[k].nals), vcl)
        if nals is None:
            return
        end = pk[min(k + 1 + DAMAGE_LENGTH, len(pk)) - 1]
        body = (data[:pk[k].pos] + b"".join(START_CODE + n.raw for n in nals)
                + data[pk[k].pos + pk[k].size:end.pos + end.size])
        damaged = f"damaged/{name}~{kind}{k}.h264"
        (out / damaged).parent.mkdir(parents=True, exist_ok=True)
        (out / damaged).write_bytes(body)
        made.append((damaged, out / damaged))

    def drop_second(nals: list[Nal], vcl: list[int]) -> list[Nal] | None:
        return None if len(vcl) < 2 else nals[:vcl[1]] + nals[vcl[1] + 1:]

    def halve_first(nals: list[Nal], vcl: list[int]) -> list[Nal] | None:
        if not vcl:
            return None
        n = nals[vcl[0]]
        nals[vcl[0]] = Nal(n.type, n.ref_idc, n.raw[:max(4, len(n.raw) // 2)])
        return nals

    def corrupt_first(nals: list[Nal], vcl: list[int]) -> list[Nal] | None:
        if not vcl:
            return None
        n = nals[vcl[0]]
        nals[vcl[0]] = Nal(n.type, n.ref_idc, corrupt(n.raw))
        return nals

    inter = next((k for k in range(3, len(pk)) if pk[k].types & {"P", "B"}), None)
    if inter is not None:
        write("d", inter, drop_second)
        write("t", inter, halve_first)
        write("x", inter, corrupt_first)
    intra = next((k for k in range(1, len(pk)) if pk[k].types == {"I"}), None)
    if intra is not None:
        write("t", intra, halve_first)
    return made


# --- Streams with film grain --------------------------------------------------

# Packets each grain stream keeps from the stream's start: enough for field
# pairs, B pictures and an IDR or two, each picture's grain seeded afresh.
GRAIN_LENGTH = 40


class BitWriter:
    """Bits written most significant first, as H.264 syntax is."""

    def __init__(self):
        self.bits: list[int] = []

    def u(self, n: int, v: int) -> "BitWriter":
        self.bits += [(v >> (n - 1 - i)) & 1 for i in range(n)]
        return self

    def ue(self, v: int) -> "BitWriter":
        n = (v + 1).bit_length()
        return self.u(n - 1, 0).u(n, v + 1)

    def se(self, v: int) -> "BitWriter":
        return self.ue(2 * v - 1 if v > 0 else -2 * v)

    def bytes(self) -> bytes:
        """The bits so far, ended as an SEI payload is: a 1, then zeros to
        the byte (`payload_bit_equal_to_one`, read by nobody here)."""
        bits = self.bits + [1] + [0] * (-(len(self.bits) + 1) % 8)
        return bytes(int("".join(map(str, bits[i:i + 8])), 2) for i in range(0, len(bits), 8))


def escape(rbsp: bytes) -> bytes:
    """`rbsp` with emulation prevention bytes put in: a 3 after any two
    zeros that a byte of 0 to 3 follows."""
    out, zeros = bytearray(), 0
    for x in rbsp:
        if zeros >= 2 and x <= 3:
            out.append(3)
            zeros = 0
        out.append(x)
        zeros = zeros + 1 if x == 0 else 0
    return bytes(out)


def film_grain_sei(model: dict, repetition_period: int) -> bytes:
    """A NAL unit, header first, of one film grain characteristics message
    (payload type 19): frequency filtering, no colour description, additive
    blending, `model`'s scale and bands for each component it has, and
    `repetition_period` (0: this picture only; 1: until the next IDR or
    message -- though FFmpeg forgets any message as a packet begins a new
    picture)."""
    w = BitWriter()
    w.u(1, 0).u(2, 0).u(1, 0)  # cancel flag, model 0, no colour description
    w.u(2, 0).u(4, model["log2_scale_factor"])  # blending mode, scale
    for c in range(3):
        w.u(1, int(c in model["bands"]))
    for c in range(3):
        bands = model["bands"].get(c)
        if bands is None:
            continue
        w.u(8, len(bands) - 1).u(3, 2)  # intervals, 3 model values
        for lo, hi, strength, cut_h, cut_v in bands:
            w.u(8, lo).u(8, hi).se(strength).se(cut_h).se(cut_v)
    w.ue(repetition_period)
    payload = w.bytes()
    size = bytes([255] * (len(payload) // 255) + [len(payload) % 255])
    return b"\x06" + escape(bytes([19]) + size + payload + b"\x80")


def grain_model(seed: int) -> dict:
    """A model for a stream, varied by `seed`: a scale of 2 to 5, luma
    always and each chroma component two times in three, one to three bands
    of brightness -- some with gaps between them, which leave blocks
    ungrained -- each with a strength of 10 to 120 and cut-offs of 2 to 14,
    all as the standard allows (a negative strength, which FFmpeg takes
    too, is left to the unit tests)."""
    rng = random.Random(seed)
    bands = {}
    for c in range(3):
        if c > 0 and rng.random() < 1 / 3:
            continue
        edges = sorted(rng.sample(range(1, 255), 2 * rng.randint(1, 3) - 1))
        lows = [0] + edges[1::2]
        highs = edges[0::2] + [255]
        # A band's top pulled down by up to 20 leaves a gap above it.
        bands[c] = [(lo, max(lo, hi - rng.randint(0, 20) * (hi < 255)),
                     rng.randint(10, 120),
                     rng.randint(2, 14), rng.randint(2, 14))
                    for lo, hi in zip(lows, highs)]
    return {"log2_scale_factor": rng.randint(2, 5), "bands": bands}


def grains(name: str, data: bytes, sizes: list[int], slices: list[str],
           out: Path) -> list[tuple[str, Path]]:
    """Write a raw stream's first `GRAIN_LENGTH` packets with film grain
    characteristics put in before a packet's first slice; give back their
    names and paths. `a`: the stream's model before every packet, to repeat
    -- which carries it to a second field in a packet of its own, FFmpeg
    forgetting a message only as a packet begins a picture. `e`: a model of
    its own before every fifth packet, for that picture alone. FATE has no
    H.264 stream with film grain, and x264 writes none."""
    pk = classify(data, sizes, slices)
    if pk is None:
        print(f"{name}: slices do not map onto packets; no grain", flush=True)
        return []
    seed = zlib.crc32(name.encode("utf-8"))
    made = []

    def write(kind: str, sei_at) -> None:
        body = bytearray()
        for k, p in enumerate(pk[:GRAIN_LENGTH]):
            sei = sei_at(k)
            for n in p.nals:
                if sei is not None and n.type in (1, 5):
                    body += START_CODE + sei
                    sei = None
                body += START_CODE + n.raw
        grained = f"grain/{name}~{kind}.h264"
        (out / grained).parent.mkdir(parents=True, exist_ok=True)
        (out / grained).write_bytes(bytes(body))
        made.append((grained, out / grained))

    write("a", lambda k: film_grain_sei(grain_model(seed), 1))
    write("e", lambda k: film_grain_sei(grain_model(seed + k), 0) if k % 5 == 0 else None)
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


# FFmpeg's options for summing a stream's pictures (see `framecrcs`), around
# its `-i`; after it, every picture kept (`-fps_mode passthrough`) at the
# size it was decoded (`-noautoscale`), written nowhere. FFmpeg's C routines
# only (`-cpuflags 0`, which this build, without assembly, has anyway), one
# thread, and cropped exactly as the SPS says (`-flags unaligned`: otherwise
# FFmpeg keeps a left crop that would leave its planes unaligned short, and
# the picture wider). It logs nothing (`-loglevel quiet`), so that the
# probe's lines are its whole stderr: its messages come from other threads
# too -- the demuxer's parser -- and one whose prefix lands in front of a
# probe line hides it.
CRC_HEAD = ["-nostdin", "-hide_banner", "-loglevel", "quiet", "-cpuflags", "0", "-threads", "1",
            "-flags", "unaligned"]
CRC_TAIL = ["-map", "0:v:0", "-fps_mode", "passthrough", "-noautoscale", "-f", "rawvideo",
            "-y", "/dev/null"]


def decoder_crc_lines(err: str) -> list[str]:
    """The size and sum of each picture the main decoder handed out, from
    the build's `FFDBG_FRAMECRC` lines: `DECODER d=<n>` as each decoder
    opens, `FRAMECRC d=<n> <size>, 0x<sum>` for each frame. The main decoder
    is the last opened, after avformat_find_stream_info's -- which may show
    pictures the main one never does."""
    main = 0
    lines: dict[int, list[str]] = {}
    for l in err.splitlines():
        if l.startswith("DECODER d="):
            main = max(main, int(l[len("DECODER d="):]))
        elif l.startswith("FRAMECRC d="):
            d, _, rest = l[len("FRAMECRC d="):].partition(" ")
            lines.setdefault(int(d), []).append(rest.strip())
    return lines.get(main, [])


def framecrcs(paths: list[Path], scratch: Path, jobs: int) -> list[list[str]]:
    """Each picture of each of `paths` as FFmpeg hands it out, summed as
    its `framecrc` muxer sums a picture: the size and Adler-32 of its planes
    end to end, cropped, rows unpadded, 16-bit samples little-endian.

    The sums are taken as each picture leaves the decoder (`FFDBG_FRAMECRC`),
    cropped exactly -- not from what the command line passes on: that
    converts every picture to the first one's pixel format for its encoder,
    so a stream that changes depth or chroma format midway would be summed
    converted.

    The build fills every picture it starts with one byte; each stream is
    run with one byte and then with another. A picture whose sum moves with
    the byte holds samples FFmpeg never wrote -- handed out undecoded, or
    part of it reached by no slice -- which in an ordinary build are
    whatever memory its buffer pool hands back, and differ from run to run:
    it is `<size>, ?`."""
    commands = [[f"FFDBG_FILL={fill}", "FFDBG_FRAMECRC=1", f"{FFMPEG}/ffmpeg", *CRC_HEAD,
                 *raw_format(path), "-i", wsl_path(path), *CRC_TAIL]
                for path in paths for fill in (0, 255)]
    results = run_all(commands, scratch, jobs)
    sums = []
    for i, path in enumerate(paths):
        a, b = (decoder_crc_lines(r.err) for r in results[2 * i:2 * i + 2])
        if len(a) != len(b):
            raise SystemExit(f"{path}: {len(a)} pictures with one fill, {len(b)} with the other")
        sums.append([x if x == y else f"{x.split(',')[0]}, ?" for x, y in zip(a, b)])
    return sums


def described(fate: Path, out: Path) -> tuple[list[str], list[Path]]:
    """Every stream already described under `out`, and where it is: the
    stream made here if it is there, else the FATE sample of its name."""
    names = sorted(str(p.relative_to(out).as_posix())[:-len(".packets")]
                   for p in out.rglob("*.packets"))
    return names, [(out / n) if (out / n).is_file() else fate / n for n in names]


def add_crc(fate: Path, out: Path, jobs: int) -> None:
    """`.crc` beside every stream already described under `out`."""
    names, paths = described(fate, out)
    sums = framecrcs(paths, out / ".crc-scratch", jobs)
    # The pictures summed are the pictures ffprobe saw handed out (`.frames`):
    # a count that differs is a run that failed.
    def shown(name: str) -> int:
        return len(sibling(out / name, ".frames").read_text(encoding="utf-8").splitlines())

    wrong = [f"{name}: {len(lines)} pictures summed, {shown(name)} in .frames"
             for name, lines in zip(names, sums) if len(lines) != shown(name)]
    if wrong:
        raise SystemExit("\n".join(wrong))
    for name, lines in zip(names, sums):
        write_lines(sibling(out / name, ".crc"), lines)
    print(f"{len(names)} streams summed")


def add_size(fate: Path, out: Path, jobs: int) -> None:
    """`.size` beside every stream already described under `out`: the
    width and height probing finds, which FFmpeg's command line opens the
    decoder that shows the stream with (`width_from_caller`)."""
    names, paths = described(fate, out)
    commands = [[f"{FFMPEG}/ffprobe", "-v", "error", *raw_format(path), "-select_streams", "v:0",
                 "-show_entries", "stream=width,height", "-of", "csv=p=0", wsl_path(path)]
                for path in paths]
    for name, r in zip(names, run_all(commands, out / ".size-scratch", jobs)):
        w, _, h = r.out.strip().partition(",")
        if w.isdigit() and h.isdigit() and int(w) and int(h):
            write_lines(sibling(out / name, ".size"), [f"{w} {h}"])
    print(f"{len(names)} streams sized")


def add_mmco(fate: Path, out: Path, jobs: int) -> None:
    """`.mmco` written afresh beside every stream already described under
    `out`: the marking lines of the decoder that shows it."""
    names, paths = described(fate, out)
    results = run_all([decoder_lines_command(p, "mmco") for p in paths], out / ".mmco-scratch",
                      jobs)
    for name, path, r in zip(names, paths, results):
        _, main, _ = decoder_lines(path, r)
        write_lines(sibling(out / name, ".mmco"), mmco_lines(main))
    print(f"{len(names)} streams' marking lines written")


NEWEXTRA_LINE = re.compile(r"^NEWEXTRA pos=(-?\d+) ([0-9a-f]*) h=(\d+)$")


def add_newextra(fate: Path, out: Path, jobs: int) -> None:
    """`.newextra` beside every container stream already described under
    `out` that brings new extradata with a packet: the lines its decoder
    that shows the stream -- the last FFmpeg opened, after probing's --
    printed for it, each packet named by its index in `.packets`."""
    names, paths = described(fate, out)
    picked = [(n, p) for n, p in zip(names, paths) if is_container(p)]
    commands = [["FFDBG_NEWEXTRA=1", f"{FFMPEG}/ffprobe", "-v", "quiet", "-threads", "1",
                 "-select_streams", "v:0", "-show_entries", "frame=pkt_pos", "-of", "csv",
                 wsl_path(p)] for _, p in picked]
    written = 0
    for (name, _), r in zip(picked, run_all(commands, out / ".newextra-scratch", jobs)):
        found = [m for m in map(NEWEXTRA_LINE.match, r.err.splitlines()) if m]
        if not found:
            continue
        main = max(int(m.group(3)) for m in found)
        at = {int(l.split()[1]): k for k, l in enumerate(
            sibling(out / name, ".packets").read_text(encoding="utf-8").splitlines())}
        lines = []
        for m in found:
            if int(m.group(3)) != main:
                continue
            pos = int(m.group(1))
            if pos not in at:
                raise SystemExit(f"{name}: new extradata at {pos}, which is no packet's position")
            lines.append(f"{at[pos]} {m.group(2)}")
        write_lines(sibling(out / name, ".newextra"), lines)
        written += 1
    print(f"{len(picked)} container streams looked at, {written} with new extradata")


def main() -> None:
    args = sys.argv[1:]
    jobs = 4
    if "--jobs" in args:
        at = args.index("--jobs")
        jobs = int(args[at + 1])
        del args[at:at + 2]
    global FFMPEG
    if "--ffmpeg" in args:
        at = args.index("--ffmpeg")
        FFMPEG = args[at + 1]
        del args[at:at + 2]
    else:
        home = subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "printenv", "HOME"],
                              stdin=subprocess.DEVNULL, capture_output=True, text=True,
                              check=True).stdout.strip()
        FFMPEG = f"{home}/ffdbg/src"
    crc_only = "--add-crc" in args
    if crc_only:
        args.remove("--add-crc")
    size_only = "--add-size" in args
    if size_only:
        args.remove("--add-size")
    newextra_only = "--add-newextra" in args
    if newextra_only:
        args.remove("--add-newextra")
    mmco_only = "--add-mmco" in args
    if mmco_only:
        args.remove("--add-mmco")
    fate, out = Path(args[0]), Path(args[1])
    if mmco_only:
        add_mmco(fate, out, jobs)
        return
    if crc_only:
        add_crc(fate, out, jobs)
        return
    if size_only:
        add_size(fate, out, jobs)
        return
    if newextra_only:
        add_newextra(fate, out, jobs)
        return
    streams = sources(fate) + [(n, p, False) for n, p in make_x264(out)]
    slices = describe_all(streams, out, jobs)
    cut_streams, damaged_streams, grain_streams = [], [], []
    for name, path, container in streams:
        if container or name in FAILED:
            continue
        sizes = [int(l.split()[0]) for l in
                 sibling(out / name, ".packets").read_text(encoding="utf-8").splitlines()]
        data = path.read_bytes()
        cut_streams += cuts(name, data, sizes, slices[name], out)
        damaged_streams += damages(name, data, sizes, slices[name], out)
        grain_streams += grains(name, data, sizes, slices[name], out)
    made = cut_streams + damaged_streams + grain_streams
    describe_all([(c, p, False) for c, p in made], out, jobs)
    # A damaged stream FFmpeg makes nothing of is one the damage went too
    # far in: it goes, stream and all. Any other is an error.
    for name in [n for n in FAILED if n.startswith("damaged/")]:
        (out / name).unlink()
        FAILED.remove(name)
        print(f"{name}: dropped, FFmpeg makes nothing of it", flush=True)
    print(f"{len(streams)} streams, {len(cut_streams)} cut from them, "
          f"{len(damaged_streams)} damaged, {len(grain_streams)} with film grain")
    if FAILED:
        raise SystemExit(f"{len(FAILED)} streams FFmpeg made nothing of: {', '.join(FAILED)}")
    add_crc(fate, out, jobs)
    add_size(fate, out, jobs)
    add_newextra(fate, out, jobs)


if __name__ == "__main__":
    main()
