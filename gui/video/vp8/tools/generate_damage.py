"""Write tests/data/damage.txt: what libvpx's VP8 decoder does with damaged
copies of the committed test vectors, frame by frame.

`tests/damage.rs` damages the same vectors the same way and must decode them
to the same pictures, errors and corruption marks. The cases are chosen
here -- byte flips in each part of a frame, truncations at each boundary,
frames left out -- from a fixed seed, so regenerating gives the same file:

    python tools/generate_damage.py WSL_DIR > tests/data/damage.txt

where WSL_DIR is the libvpx build directory (in WSL) holding
`damage_reference`, built from `tools/damage_reference.c` as its comment
says. Each case is a line `case VECTOR MUTATION...`, then one line per frame
decoded, as the harness prints them with each MD5 cut to its first 8 hex
digits, then `end`.
"""
import os
import random
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "tests", "data")

#: The vectors damaged, and what each brings: key and inter frames at 176x144
#: (001), the bilinear filter and no loop filter (004), whole-pixel chroma
#: (005), golden and altref frames with sign bias (011), a hidden frame
#: (018), token partitions (1405), a size change at a key frame (1436), and
#: segmentation (03).
VECTORS = [
    "vp80-00-comprehensive-001.ivf",
    "vp80-00-comprehensive-004.ivf",
    "vp80-00-comprehensive-005.ivf",
    "vp80-00-comprehensive-011.ivf",
    "vp80-00-comprehensive-018.ivf",
    "vp80-04-partitions-1405.ivf",
    "vp80-03-segmentation-1436.ivf",
    "vp80-03-segmentation-03.ivf",
]

#: Frames past this many are not decoded: the damage has shown by then.
LIMIT = 12


def frames(path):
    data = open(path, "rb").read()
    pos = max(struct.unpack_from("<H", data, 6)[0], 32)
    out = []
    while pos + 12 <= len(data):
        n = struct.unpack_from("<I", data, pos)[0]
        out.append(data[pos + 12:pos + 12 + n])
        pos += 12 + n
    return out


def regions(frame):
    """(start, end) of the tag, the key frame header, the first partition and
    the rest, as far as the frame has them."""
    if len(frame) < 3:
        return [(0, len(frame))]
    tag = frame[0] | frame[1] << 8 | frame[2] << 16
    key = tag & 1 == 0
    first = tag >> 5
    head = 10 if key else 3
    out = [(0, 3)]
    if key:
        out.append((3, min(10, len(frame))))
    end = min(head + first, len(frame))
    if end > head:
        out.append((head, end))
    if len(frame) > end:
        out.append((end, len(frame)))
    return out


def cases(name, fs, rng):
    """The mutations for one vector."""
    out = []
    count = min(len(fs), LIMIT)
    inter = [i for i in range(1, count) if fs[i] and fs[i][0] & 1]
    targets = [0] + rng.sample(inter, min(2, len(inter)))
    for f in targets:
        for start, end in regions(fs[f]):
            for _ in range(3):
                o = rng.randrange(start, end)
                x = rng.choice([1, 0x80, 0xff, rng.randrange(1, 256)])
                out.append([f"flip:{f}:{o}:{x}"])
        first = fs[f][0] | fs[f][1] << 8 | fs[f][2] << 16
        head = 10 if first & 1 == 0 else 3
        for n in {1, 2, 3, 9, 10, 11, head + (first >> 5) - 1, head + (first >> 5),
                  len(fs[f]) // 2, len(fs[f]) - 1}:
            if 0 < n < len(fs[f]):
                out.append([f"cut:{f}:{n}"])
    out.append(["drop:0"])
    if inter:
        out.append([f"drop:{rng.choice(inter)}"])
    # Two flips at once, in different frames.
    for _ in range(4 if count >= 2 else 0):
        a, b = rng.sample(range(count), 2)
        out.append([f"flip:{a}:{rng.randrange(len(fs[a]))}:{rng.randrange(1, 256)}",
                    f"flip:{b}:{rng.randrange(len(fs[b]))}:{rng.randrange(1, 256)}"])
    return out


def main() -> int:
    wsl_dir = sys.argv[1]
    rng = random.Random(0x5eed_0008)
    out = []
    out.append("# What libvpx v1.17.0's VP8 decoder does with damaged vectors:\n"
               "# written by tools/generate_damage.py; do not edit.\n")
    for name in VECTORS:
        path = os.path.join(DATA, name)
        fs = frames(path)
        wsl_path = "/mnt/" + os.path.abspath(path)[0].lower() + os.path.abspath(path)[2:].replace(chr(92), "/")
        for mutation in cases(name, fs, rng):
            r = subprocess.run(
                ["wsl", "--", f"{wsl_dir}/damage_reference", wsl_path, f"limit:{LIMIT}", *mutation],
                capture_output=True, text=True, check=True)
            lines = [l for l in r.stdout.splitlines() if l.strip()][:LIMIT]
            out.append(f"case {name} {' '.join(mutation)}\n")
            for line in lines:
                parts = line.split()
                if parts[0] == "ok":
                    parts[1] = parts[1][:8]
                out.append(" ".join(parts) + "\n")
            out.append("end\n")
    # Bytes, not text: on Windows a text-mode stdout turns every newline into
    # CRLF.
    sys.stdout.buffer.write("".join(out).encode("utf-8"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
