#!/usr/bin/env python3
"""Writes the sevenz crate's fixtures: archives 7-Zip 26.00 made, and 7-Zip's
own verdict on each -- and on every one-byte corruption of a few small ones.

The oracle is 7-Zip 26.00 for Windows (`C:/Program Files/7-Zip/7z.exe`),
the release the port's LZMA SDK 26.00 sources are from. Run from Windows:

    python sevenz/tests/data/generate.py

What it writes, beside itself:

- `input/`: the tree every archive is made of -- text, random bytes,
  machine-code-like bytes, an empty file, an empty folder, a name that is not
  ASCII -- generated, so it is the same every time.
- `made/`: archives of `input/` made by 7-Zip with each method, filter and
  option the reader is to handle, and `made.txt`: how each was made and what
  `7z t` said.
- `mutations.txt`: every byte of a few small archives XORed with 01, 80 and
  FF, and `7z t`'s verdict on each: OK, or the errors it reported.
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
SEVENZIP = pathlib.Path("C:/Program Files/7-Zip/7z.exe")
MASK64 = (1 << 64) - 1


class Rng:
    """`xz/tests/data/generate.py`'s generator."""

    def __init__(self, seed: int) -> None:
        self.s = seed & MASK64

    def next(self) -> int:
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & MASK64
        return self.s >> 33


WORDS = [b"alpha", b"beta", b"gamma", b"delta", b"epsilon", b"zeta", b"eta", b"theta"]


def text(n: int, seed: int) -> bytes:
    r = Rng(seed)
    out = bytearray()
    while len(out) < n:
        out += WORDS[r.next() % len(WORDS)]
        out += b"\n" if r.next() % 9 == 0 else b" "
    return bytes(out[:n])


def random_bytes(n: int, seed: int) -> bytes:
    r = Rng(seed)
    return bytes(r.next() & 0xFF for _ in range(n))


def code(n: int, seed: int) -> bytes:
    """Calls (E8) to a few targets and recurring instruction runs: work for
    the x86 converters."""
    r = Rng(seed)
    snippets = [bytes((i * 37 + j * 11) & 0xFF for j in range(6 + i % 5)) for i in range(12)]
    out = bytearray()
    while len(out) < n:
        k = r.next() % 8
        if k == 0:
            out += bytes([0xE8]) + (0x400 * (r.next() % 9)).to_bytes(4, "little")
        elif k == 1:
            out += bytes([0x90]) * (1 + r.next() % 4)
        else:
            out += snippets[r.next() % len(snippets)]
    return bytes(out[:n])


# The tree: (path, bytes), or (path, None) for an empty folder.
TREE = [
    ("docs/readme.txt", text(5_000, 1)),
    ("docs/empty.txt", b""),
    ("docs/sub", None),
    ("data/random.bin", random_bytes(20_000, 2)),
    ("data/code.bin", code(30_000, 3)),
    ("top.txt", text(50_000, 4)),
    ("na\u00efve \u2013 \u00fcn\u00efc\u00f8d\u00e9.txt", text(700, 5)),
]

# (name, 7z arguments): each exercises something.
MADE = [
    ("lzma2-solid", ["-m0=LZMA2", "-mx=5", "-ms=on"]),
    ("lzma2-files", ["-m0=LZMA2", "-mx=5", "-ms=off"]),
    ("lzma", ["-m0=LZMA", "-mx=5"]),
    ("lzma-fast", ["-m0=LZMA", "-mx=1"]),
    ("bzip2", ["-m0=BZip2"]),
    ("deflate", ["-m0=Deflate"]),
    ("copy", ["-m0=Copy"]),
    ("ppmd", ["-m0=PPMd"]),
    ("bcj-lzma2", ["-mf=BCJ", "-m0=LZMA2"]),
    ("bcj2", ["-mf=BCJ2", "-m0=LZMA"]),
    ("delta", ["-mf=Delta:4", "-m0=LZMA2"]),
    ("arm", ["-mf=ARM", "-m0=LZMA2"]),
    ("arm64", ["-mf=ARM64", "-m0=LZMA2"]),
    ("headers-plain", ["-m0=LZMA2", "-mhc=off"]),
    ("times", ["-m0=LZMA2", "-mtc=on", "-mta=on"]),
    ("encrypted", ["-m0=LZMA2", "-psecret"]),
    ("encrypted-headers", ["-m0=LZMA2", "-psecret", "-mhe=on"]),
]

# Mutated whole: small archives with something in every part of the format.
MUTATED = ["small-lzma2", "small-headers-plain", "small-copy"]
SMALL = {
    "small-lzma2": ["-m0=LZMA2", "-mx=5"],
    "small-headers-plain": ["-m0=LZMA", "-mhc=off"],
    "small-copy": ["-m0=Copy", "-mhc=off"],
}


def sevenzip(*args: str, cwd: pathlib.Path | None = None) -> subprocess.CompletedProcess:
    return subprocess.run([str(SEVENZIP), *args], capture_output=True, cwd=cwd)


def verdict(archive: pathlib.Path) -> str:
    """`7z t`'s verdict: `OK`, or `ERR` and the errors it reported, sorted,
    with the file names left out."""
    r = sevenzip("t", "-bso1", "-bse1", "-bsp0", "-psecret", str(archive))
    out = r.stdout.decode("utf-8", "replace") + r.stderr.decode("utf-8", "replace")
    if r.returncode == 0:
        return "OK"
    kinds = set()
    for line in out.splitlines():
        line = line.strip()
        for key in ("Headers Error", "Data Error", "CRC Failed", "Unsupported Method",
                    "Unexpected end of archive", "Can not open the file as archive",
                    "Is not archive", "Wrong password", "There are data after the end of archive",
                    "There are some data after the end of the payload data"):
            if key.lower() in line.lower():
                kinds.add(key.replace(" ", "_"))
    return "ERR " + (",".join(sorted(kinds)) or f"exit{r.returncode}")


def write_tree(root: pathlib.Path, tree) -> None:
    if root.exists():
        shutil.rmtree(root)
    for path, data in tree:
        p = root / path
        if data is None:
            p.mkdir(parents=True, exist_ok=True)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)


def make(name: str, args: list[str], src: pathlib.Path, out_dir: pathlib.Path) -> pathlib.Path:
    archive = out_dir / f"{name}.7z"
    if archive.exists():
        archive.unlink()
    # -mtm- etc. would leave the times out; the default records mtime. The
    # file order is 7-Zip's own (it sorts by extension and name).
    r = sevenzip("a", "-t7z", "-bso0", "-bsp0", *args, str(archive), ".", cwd=src)
    if r.returncode != 0:
        sys.exit(f"7z a {name} failed: {r.stderr.decode('utf-8', 'replace')}")
    return archive


def main() -> None:
    if not SEVENZIP.exists():
        sys.exit(f"{SEVENZIP} not found")
    version = sevenzip("i").stdout.decode("utf-8", "replace")
    if "7-Zip 26.00" not in version:
        sys.exit("expected 7-Zip 26.00")

    src = HERE / "input"
    write_tree(src, TREE)

    made = HERE / "made"
    if made.exists():
        shutil.rmtree(made)
    made.mkdir()
    lines = ["# archive verdict -- made by 7-Zip 26.00 with: args"]
    for name, args in MADE:
        archive = make(name, args, src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(args)}")

    small_src = HERE / "small-input"
    write_tree(small_src, [("a.txt", text(300, 6)), ("dir", None), ("b.bin", random_bytes(40, 7)), ("e.txt", b"")])
    for name in MUTATED:
        archive = make(name, SMALL[name], small_src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(SMALL[name])}")
    shutil.rmtree(small_src)
    (HERE / "made.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    out = ["# == archive, then: position xor verdict -- 7z t's"]
    tmp = HERE / "mutant.7z"
    for name in MUTATED:
        data = (made / f"{name}.7z").read_bytes()
        out.append(f"== {name}.7z")
        for pos in range(len(data)):
            for x in (0x01, 0x80, 0xFF):
                bad = bytearray(data)
                bad[pos] ^= x
                tmp.write_bytes(bytes(bad))
                out.append(f"{pos} {x:02x} {verdict(tmp)}")
    tmp.unlink()
    (HERE / "mutations.txt").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(MADE)} made, {len(out)} mutation lines")


if __name__ == "__main__":
    main()
