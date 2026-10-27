"""FFmpeg 8.1.3's fixed-point AAC decoder's view of every AAC sample: what
the crate's oracle test holds the decoder to.

    python gui/video/aac/tools/pcm_expect.py <fate-suite> <out> [--jobs N] [--ffmpeg DIR]

Everything written here is FFmpeg 8.1.3's -- the release the decoder is a
translation of -- from its build in WSL with the FFDBG_PCM probe
(`tools/ffaac/build.sh`, in DIR, by default ~/ffaac/src there). For each
stream under <fate-suite>/aac -- FATE's AAC samples, the conformance streams
among them; LATM and USAC, which the fixed-point decoder does not take, left
out -- files under <out>, named after it:

  .packets  each packet the demuxer hands the decoder, a line each: its size
            and its position in the file (ADTS frames as FFmpeg's parser
            cuts them).
  .data     the packets' bytes, end to end.
  .open     the decoder that plays the stream, as it opens: its channels,
            the layout's mask, the sample rate and the layout's order, on
            one line; the extradata in hex on the next, if it has any.
  .pcm      each frame that decoder gives, as FFDBG_PCM writes it (see
            build.sh) -- untrimmed: ffmpeg runs with -flags2 +skip_manual,
            so the encoder's priming is the container's to drop, not the
            decoder's.

A stream FFmpeg plays nothing of is reported and left out.
"""

import base64
import json
import struct
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
import wsl  # noqa: E402 -- gui/video/tools, put on the path just above

SUFFIXES = {".mp4", ".m4a", ".3gp", ".aac", ".adts"}
OPEN, FPCM = 0x4E45504F, 0x4D435046
# Bytes a sample of each AVSampleFormat, by its number.
SAMPLE_BYTES = {0: 1, 1: 2, 2: 4, 3: 4, 4: 8, 5: 1, 6: 2, 7: 4, 8: 4, 9: 8, 10: 8, 11: 8}

# Where FFmpeg 8.1.3's build is in WSL (`--ffmpeg`).
FFMPEG = ""


def sources(fate: Path) -> list[tuple[str, Path]]:
    """Each AAC sample, by its name under the FATE suite."""
    found = []
    for p in sorted((fate / "aac").rglob("*")):
        name = p.relative_to(fate).as_posix()
        if p.is_file() and p.suffix.lower() in SUFFIXES and "/usac/" not in f"/{name}":
            found.append((name, p))
    return found


def packets_command(path: Path) -> list[str]:
    return [f"{FFMPEG}/ffprobe", "-v", "error", "-select_streams", "a:0",
            "-show_entries", "packet=pos,size,data", "-show_data", "-data_dump_format",
            "base64", "-of", "json", wsl.wsl_path(path)]


def decode_command(path: Path, probe: Path) -> list[str]:
    return [f"FFDBG_PCM={wsl.wsl_path(probe)}", f"{FFMPEG}/ffmpeg", "-nostdin",
            "-hide_banner", "-loglevel", "error", "-flags2", "+skip_manual",
            "-c:a", "aac_fixed", "-i", wsl.wsl_path(path), "-map", "0:a:0",
            "-c:a", "pcm_s32le", "-f", "s32le", "-y", "/dev/null"]


def packets(name: str, r: wsl.Result) -> tuple[list[tuple[int, int]], bytes]:
    """Each packet's size and position, and the packets' bytes end to end."""
    try:
        parsed = json.loads(r.out or "{}")
    except ValueError as e:
        raise SystemExit(f"{name}: ffprobe wrote no JSON ({e}): {r.err[-300:]}") from e
    rows, data = [], bytearray()
    for p in parsed.get("packets", []):
        size, pos = int(p["size"]), int(p.get("pos", -1))
        chunk = base64.b64decode("".join(p.get("data", "").split()))
        if len(chunk) != size:
            raise SystemExit(f"{name}: packet at {pos} has {len(chunk)} of {size} bytes")
        rows.append((size, pos))
        data += chunk
    return rows, bytes(data)


def records(data: bytes) -> list[tuple[int, int, bytes]]:
    """FFDBG_PCM's records: each its kind, its decoder's number, and the
    record whole, header and all."""
    found, at = [], 0
    while at < len(data):
        magic, decoder, ch, _, _, a, _, b = struct.unpack_from("<8i", data, at)
        if magic == OPEN:
            size = a
        elif magic == FPCM:
            size = ch * a * SAMPLE_BYTES[b]
        else:
            raise SystemExit(f"FFDBG_PCM: no record at byte {at}")
        found.append((magic, decoder, data[at:at + 32 + size]))
        at += 32 + size
    return found


def describe(name: str, out: Path, packet_result: wsl.Result, probe: Path,
             decoded: wsl.Result) -> bool:
    """Write the stream's files under `out`; False, its files removed, for
    a stream FFmpeg plays nothing of."""
    dest = out / name
    rows, data = packets(name, packet_result)
    found = records(probe.read_bytes()) if probe.exists() else []
    probe.unlink(missing_ok=True)
    opened = [d for kind, d, _ in found if kind == OPEN]
    if not rows or not opened:
        print(f"FAILED {name}: {len(rows)} packets, no decoder opened "
              f"(ffmpeg {decoded.status}: {decoded.err[-200:].strip()})", flush=True)
        return False
    main = max(opened)
    frames = [rec for kind, d, rec in found if kind == FPCM and d == main]
    if not frames:
        print(f"FAILED {name}: no frame decoded ({decoded.err[-200:].strip()})", flush=True)
        return False
    _, _, ch, lo, hi, size, rate, order = struct.unpack_from(
        "<8i", next(rec for kind, d, rec in found if kind == OPEN and d == main))
    extradata = next(rec for kind, d, rec in found if kind == OPEN and d == main)[32:]
    mask = (lo & 0xFFFF_FFFF) | ((hi & 0xFFFF_FFFF) << 32)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.with_name(dest.name + ".packets").write_text(
        "".join(f"{s} {p}\n" for s, p in rows), encoding="utf-8", newline="\n")
    dest.with_name(dest.name + ".data").write_bytes(data)
    dest.with_name(dest.name + ".open").write_text(
        f"{ch} {mask} {rate} {order}\n{extradata.hex()}\n", encoding="utf-8", newline="\n")
    dest.with_name(dest.name + ".pcm").write_bytes(b"".join(frames))
    print(f"{name}: {len(rows)} packets, {len(frames)} frames", flush=True)
    return True


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
        FFMPEG = f"{home}/ffaac/src"
    fate, out = Path(args[0]), Path(args[1])
    out.mkdir(parents=True, exist_ok=True)
    streams = sources(fate)
    probes = [out / f".probe-{k}.pcm" for k in range(len(streams))]
    for p in probes:
        p.unlink(missing_ok=True)
    commands = []
    for (_, path), probe in zip(streams, probes):
        commands += [packets_command(path), decode_command(path, probe)]
    results = wsl.run_all(commands, out / ".scratch", jobs,
                          check=[f"{FFMPEG}/ffprobe", "-hide_banner", "-version"])
    failed = [name for k, ((name, _), probe) in enumerate(zip(streams, probes))
              if not describe(name, out, results[2 * k], probe, results[2 * k + 1])]
    print(f"{len(streams)} streams, {len(streams) - len(failed)} described")
    if failed:
        print("FFmpeg played nothing of: " + ", ".join(failed))


if __name__ == "__main__":
    main()
