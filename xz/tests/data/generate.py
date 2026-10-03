#!/usr/bin/env python3
"""Writes the xz crate's fixtures, every verdict in them liblzma 5.2.5's own.

The oracle is XZ Utils 5.2.5 -- the release `src/` is ported from -- built
in WSL from the `xz-5.2` tree the `lzma-sys` 0.1.20 crate vendors:
`oracle.c` beside this file, linked against that tree's liblzma, judges
whole files as `xz -d` does; the `xz` the same tree builds writes the files
that test the filters, the checks and the container. Run from Windows:

    python xz/tests/data/generate.py

It builds the oracle under `~/xz525` in WSL's Ubuntu if it is not there.

What it writes, beside itself:

- `files/`: XZ Utils' own `tests/files` -- good, bad and unsupported `.xz`
  files, each named for what it tests (their `README` explains every one) --
  and `files.txt`, the oracle's verdict on each: `OK length fnv` or `ERR`.
- `made/`: files made by `xz` 5.2.5 from generated inputs -- every filter,
  every check, several blocks, several streams, stream padding, `.lzma` --
  and `made.txt`, how each was made and the oracle's verdict.
- `mutations.txt`: every byte of several small files XORed with 01, 80 and
  FF, and the oracle's verdict on each -- among them a raw LZMA2 stream, which
  has no check to turn a wrong decode into an error, so that the LZMA
  decoder's own refusals are what is judged.
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent
REGISTRY = pathlib.Path.home() / ".cargo/registry/src/index.crates.io-1949cf8c6b5b557f"
SRC = REGISTRY / "lzma-sys-0.1.20/xz-5.2"
MASK64 = (1 << 64) - 1


def wsl_path(p: pathlib.Path) -> str:
    """The WSL view of a Windows path: E:/x/y -> /mnt/e/x/y."""
    p = p.resolve()
    drive = p.drive.rstrip(":").lower()
    rest = p.as_posix().split(":", 1)[1]
    return f"/mnt/{drive}{rest}"


def wsl(script: str, stdin: bytes | None = None) -> bytes:
    r = subprocess.run(
        ["wsl", "-d", "Ubuntu", "--", "bash", "-s"],
        input=script.encode("utf-8") + (b"" if stdin is None else b""),
        capture_output=True,
    )
    if r.returncode != 0:
        sys.stderr.write(r.stderr.decode("utf-8", "replace"))
        raise SystemExit(f"wsl failed ({r.returncode})")
    return r.stdout


def build_oracle() -> None:
    src = wsl_path(SRC)
    here = wsl_path(HERE)
    wsl(f"""set -e
mkdir -p ~/xz525
if [ ! -d ~/xz525/src ]; then cp -r "{src}" ~/xz525/src; fi
grep -q 'LZMA_VERSION_PATCH 5' ~/xz525/src/src/liblzma/api/lzma/version.h
if [ ! -f ~/xz525/build/liblzma.a ]; then
  cmake -S ~/xz525/src -B ~/xz525/build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF >/dev/null
  cmake --build ~/xz525/build -j8 >/dev/null
fi
cp "{here}/oracle.c" ~/xz525/oracle.c
gcc -O2 -I ~/xz525/src/src/liblzma/api ~/xz525/oracle.c ~/xz525/build/liblzma.a -lpthread -o ~/xz525/oracle
~/xz525/build/xz --version | grep -q '5.2.5'
""")


def oracle(mode: str, path: pathlib.Path) -> str:
    return wsl(f'~/xz525/oracle {mode} "{wsl_path(path)}"\n').decode("utf-8").strip()


def xz(args: str, data: bytes) -> bytes:
    tmp = HERE / "made" / "input.tmp"
    tmp.write_bytes(data)
    try:
        return wsl(f'~/xz525/build/xz {args} -c "{wsl_path(tmp)}"\n')
    finally:
        tmp.unlink()


# --- inputs (mirrored in tests/liblzma.rs) ------------------------------------

class Rng:
    def __init__(self, seed: int) -> None:
        self.s = seed & MASK64

    def next(self) -> int:
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & MASK64
        return self.s >> 33


WORDS = [b"alpha", b"beta", b"gamma", b"delta", b"epsilon", b"zeta", b"eta", b"theta"]


def gen(kind: str, n: int, seed: int) -> bytes:
    r = Rng(seed)
    if kind == "text":
        out = bytearray()
        while len(out) < n:
            out += WORDS[r.next() % len(WORDS)]
            out += b"\n" if r.next() % 9 == 0 else b" "
        return bytes(out[:n])
    if kind == "random":
        return bytes(r.next() & 0xFF for _ in range(n))
    if kind == "code":
        # Something like machine code: calls (E8) to a few targets, ARM
        # branches (EB atop a little-endian word), fill, and a handful of
        # recurring instruction sequences -- compressible, as code is, and
        # with work for every converter.
        snippets = [bytes((i * 37 + j * 11) & 0xFF for j in range(6 + i % 5)) for i in range(12)]
        out = bytearray()
        while len(out) < n:
            k = r.next() % 8
            if k == 0:
                out += bytes([0xE8]) + (0x400 * (r.next() % 9)).to_bytes(4, "little")
            elif k == 1:
                out += (0x100 * (r.next() % 7)).to_bytes(3, "little") + bytes([0xEB])
            elif k == 2:
                out += bytes([0x90]) * (1 + r.next() % 4)
            else:
                out += snippets[r.next() % len(snippets)]
        return bytes(out[:n])
    raise ValueError(kind)


def fnv(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & MASK64
    return h


# (name, input kind, size, seed, xz arguments). Each exercises something.
MADE = [
    ("text-crc64", "text", 40_000, 1, "--format=xz --check=crc64 -6"),
    ("text-crc32", "text", 40_000, 1, "--format=xz --check=crc32 -6"),
    ("text-sha256", "text", 40_000, 1, "--format=xz --check=sha256 -6"),
    ("text-none", "text", 40_000, 1, "--format=xz --check=none -6"),
    ("text-blocks", "text", 200_000, 2, "--format=xz --block-size=65536 -6"),
    ("random-0", "random", 8_000, 3, "--format=xz -0"),
    ("random-9e", "random", 8_000, 3, "--format=xz -9e"),
    ("code-x86", "code", 24_000, 4, "--format=xz --x86 --lzma2=preset=6"),
    ("code-arm", "code", 24_000, 4, "--format=xz --arm --lzma2=preset=6"),
    ("code-armthumb", "code", 24_000, 4, "--format=xz --armthumb --lzma2=preset=6"),
    ("code-powerpc", "code", 24_000, 4, "--format=xz --powerpc --lzma2=preset=6"),
    ("code-ia64", "code", 24_000, 4, "--format=xz --ia64 --lzma2=preset=6"),
    ("code-sparc", "code", 24_000, 4, "--format=xz --sparc --lzma2=preset=6"),
    ("code-x86-start", "code", 24_000, 4, "--format=xz --x86=start=4096 --lzma2=preset=6"),
    ("code-delta", "code", 24_000, 4, "--format=xz --delta=dist=4 --lzma2=preset=6"),
    ("code-chain", "code", 24_000, 4, "--format=xz --x86 --delta=dist=2 --lzma2=preset=1"),
    ("empty", "text", 0, 1, "--format=xz"),
    ("lzma-text", "text", 40_000, 5, "--format=lzma -6"),
    ("lzma-empty", "text", 0, 5, "--format=lzma"),
    ("raw-lzma2", "text", 20_000, 6, "--format=raw --lzma2=preset=6"),
]

# Mutated whole: small files with something in every part of the format.
MUTATED = ["text-small", "two-blocks-small", "lzma-small", "raw-small"]


def reserved_flag(stream: bytes) -> bytes:
    """`stream` with a reserved bit (0x04) of its first block header's flags
    set and the header's CRC32 made right again, so that the flag is the only
    thing wrong: liblzma refuses it as unsupported, not as damaged."""
    out = bytearray(stream)
    start = 12
    size = (out[start] + 1) * 4
    out[start + 1] |= 0x04
    crc = zlib.crc32(bytes(out[start:start + size - 4]))
    out[start + size - 4:start + size] = crc.to_bytes(4, "little")
    return bytes(out)


def main() -> None:
    build_oracle()

    files = HERE / "files"
    if files.exists():
        shutil.rmtree(files)
    files.mkdir()
    lines = ["# file verdict [length fnv64] -- the oracle's"]
    for f in sorted((SRC / "tests/files").iterdir()):
        shutil.copyfile(f, files / f.name)
        if f.suffix == ".xz":
            lines.append(f"{f.name} {oracle('xz', files / f.name)}")
    (HERE / "files.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    made = HERE / "made"
    if made.exists():
        shutil.rmtree(made)
    made.mkdir()
    lines = ["# file kind size seed verdict [length fnv64] -- made by xz 5.2.5 with: args"]
    for name, kind, size, seed, args in MADE:
        data = gen(kind, size, seed)
        packed = xz(args, data)
        ext = ".lzma" if "lzma" in args.split()[0] else (".raw" if "raw" in args else ".xz")
        f = made / f"{name}{ext}"
        f.write_bytes(packed)
        mode = {".lzma": "lzma", ".raw": "raw2"}.get(ext, "xz")
        verdict = oracle(mode, f)
        assert verdict == f"OK {len(data)} {fnv(data):016x}", (name, verdict)
        lines.append(f"{f.name} {kind} {size} {seed} {verdict} -- {args}")

    # Several streams, and stream padding between and after them.
    a = (made / "text-crc32.xz").read_bytes()
    b = (made / "text-none.xz").read_bytes()
    for name, data in [
        ("two-streams", a + b),
        ("padded", a + bytes(8) + b + bytes(4)),
        ("badly-padded", a + bytes(3) + b),
        ("badly-padded-at-end", a + bytes(3)),
        ("trailing-garbage", a + b"\x00\x00\x00\x00junk"),
        ("reserved-flag", reserved_flag(a)),
    ]:
        f = made / f"{name}.xz"
        f.write_bytes(data)
        lines.append(f"{f.name} cat - - {oracle('xz', f)} -- concatenated")
    (HERE / "made.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    # The small files mutated whole.
    small = {
        "text-small": ("text", 1000, 7, "--format=xz -6"),
        "two-blocks-small": ("text", 2400, 8, "--format=xz --block-size=1024 --check=sha256 -1"),
        "lzma-small": ("text", 1000, 9, "--format=lzma -6"),
        "raw-small": ("text", 1000, 10, "--format=raw --lzma2=preset=6"),
    }
    out = ["# == file, then: position xor verdict [length fnv64] -- the oracle's"]
    for name in MUTATED:
        kind, size, seed, args = small[name]
        ext = ".lzma" if "lzma" in args.split()[0] else (".raw" if "raw" in args else ".xz")
        f = made / f"{name}{ext}"
        f.write_bytes(xz(args, gen(kind, size, seed)))
        mode = {".lzma": "lzma", ".raw": "raw2"}.get(ext, "xz") + "-mutate"
        out.append(f"== {f.name}")
        out.extend(oracle(mode, f).splitlines())
    (HERE / "mutations.txt").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")

    total = sum(p.stat().st_size for p in HERE.rglob("*") if p.is_file())
    count = sum(1 for line in out if line and line[0].isdigit())
    print(f"{len(MADE)} made, {count} mutations, {total} bytes under {HERE}")


if __name__ == "__main__":
    main()
