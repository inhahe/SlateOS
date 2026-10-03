#!/usr/bin/env python3
"""Writes the sevenz crate's fixtures: archives 7-Zip 26.00 made, and 7-Zip's
own verdict on each -- and on every one-byte corruption of a few small ones.

The oracle is 7-Zip 26.00 for Windows (`C:/Program Files/7-Zip/7z.exe`),
the release the port's LZMA SDK 26.00 sources are from. Run from Windows:

    python sevenz/tests/data/generate.py

What it writes, beside itself:

- `input.txt`: the tree every archive is made of -- text, random bytes,
  machine-code-like bytes, an empty file, an empty folder, a name that is not
  ASCII -- one line an entry, with each file's size and FNV-1a hash. The tree
  is generated, built in `input/` while the archives are made, and removed.
- `made/`: archives of `input/` made by 7-Zip with each method, filter and
  option the reader is to handle, and `made.txt`: how each was made and what
  `7z t` said.
- `mutations.txt`: every byte of a few small archives XORed with 01, 80 and
  FF, and chosen bytes of two LZMA2 archives of several chunks -- every bit
  of every chunk header, and data bytes at each chunk's start, middle and
  end -- with `7z t`'s verdict on each: OK, or the errors it reported.
- `mutations-mmt-off.txt`: the same, tested by 7-Zip with one thread
  (`-mmt=off`). Its LZMA2 decoder works differently with several threads,
  and on some damaged streams the two give back different amounts of data.

7-Zip 26.00 sometimes crashes -- an access violation -- testing a damaged
LZMA2 archive of several dictionary-reset blocks with several threads; on the
worst of the mutants here about one run in twelve dies and the rest agree. A
crash is not a verdict, so a run that crashes is run again, and the crashes
are counted in the summary. (A verdict of `CRASH` would mean every attempt
died.)
"""

from __future__ import annotations

import os
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
    # 64 KiB of model for about 105 KB of input: the model fills and starts
    # again several times, which is where an allocator that is not exactly
    # 7-Zip's would show.
    ("ppmd-small-mem", ["-m0=PPMd:mem=64k:o=32"]),
    ("bcj-lzma2", ["-mf=BCJ", "-m0=LZMA2"]),
    ("bcj2", ["-mf=BCJ2", "-m0=LZMA"]),
    ("delta", ["-mf=Delta:4", "-m0=LZMA2"]),
    ("arm", ["-mf=ARM", "-m0=LZMA2"]),
    ("arm64", ["-mf=ARM64", "-m0=LZMA2"]),
    ("riscv", ["-mf=RISCV", "-m0=LZMA2"]),
    ("headers-plain", ["-m0=LZMA2", "-mhc=off"]),
    ("times", ["-m0=LZMA2", "-mtc=on", "-mta=on"]),
    ("encrypted", ["-m0=LZMA2", "-psecret"]),
    ("encrypted-headers", ["-m0=LZMA2", "-psecret", "-mhe=on"]),
]

# Mutated whole: small archives with something in every part of the format.
MUTATED = [
    "small-lzma2",
    "small-headers-plain",
    "small-copy",
    "small-ppmd",
    "small-bzip2",
    "small-deflate",
    "small-bcj-lzma2",
    "small-bcj2",
    "small-arm64",
]
SMALL = {
    "small-lzma2": ["-m0=LZMA2", "-mx=5"],
    "small-headers-plain": ["-m0=LZMA", "-mhc=off"],
    "small-copy": ["-m0=Copy", "-mhc=off"],
    "small-ppmd": ["-m0=PPMd", "-mhc=off"],
    "small-bzip2": ["-m0=BZip2", "-mhc=off"],
    "small-deflate": ["-m0=Deflate", "-mhc=off"],
    # A filter before a coder: two coders, one bond.
    "small-bcj-lzma2": ["-mf=BCJ", "-m0=LZMA2", "-mhc=off"],
    # Four packed streams, three of them through LZMA.
    "small-bcj2": ["-mf=BCJ2", "-m0=LZMA", "-mhc=off"],
    # A filter 7-Zip added in 23.00, over Copy: every byte it converted is
    # in the archive as it stands.
    "small-arm64": ["-mf=ARM64", "-m0=Copy", "-mhc=off"],
}
# The tree they are made of: text, random bytes, machine-code-like bytes for
# the branch converters, an empty file and an empty folder.
SMALL_TREE = [
    ("a.txt", text(300, 6)),
    ("dir", None),
    ("b.bin", random_bytes(40, 7)),
    ("c.bin", code(240, 8)),
    ("e.txt", b""),
]


# Mutated at chosen bytes: archives too big to mutate whole, whose LZMA2
# chunk structure is the point. One block of two LZMA chunks under one
# dictionary; and nine blocks of one chunk each, each its own dictionary
# (`c=64k`: a 64 KiB block size), which 7-Zip's threads decode apart.
TARGETED = {
    "chunks-lzma2": ["-m0=LZMA2", "-mx=5"],
    "blocks-lzma2": ["-m0=LZMA2:d=64k:c=64k"],
}
CHUNKY_TREE = [
    ("a.txt", text(240_000, 21)),
    ("b.bin", random_bytes(30_000, 22)),
    ("c.txt", text(240_000, 23)),
]
# Every bit of a chunk header, and FF; 01, 80 and FF elsewhere.
HEADER_XORS = (0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0xFF)
XORS = (0x01, 0x80, 0xFF)


def lzma2_chunks(data: bytes, start: int) -> list[tuple[int, int, int]]:
    """The chunks of the LZMA2 stream at `start`, as (offset, header
    length, data length), the end marker last as (offset, 1, 0)."""
    out = []
    i = start
    while True:
        c = data[i]
        if c == 0:
            out.append((i, 1, 0))
            return out
        unpack = (data[i + 1] << 8 | data[i + 2]) + 1
        if c & 0x80:
            pack = (data[i + 3] << 8 | data[i + 4]) + 1
            header = 6 if c >= 0xC0 else 5
            out.append((i, header, pack))
            i += header + pack
        else:
            out.append((i, 3, unpack))
            i += 3 + unpack


def targeted(data: bytes) -> list[tuple[int, int]]:
    """The (position, xor) pairs a TARGETED archive is mutated at: its one
    folder's LZMA2 stream starts right after the start header."""
    out = []
    for at, header, size in lzma2_chunks(data, 32):
        for pos in range(at, at + header):
            out.extend((pos, x) for x in HEADER_XORS)
        if size:
            first = at + header
            for pos in sorted({first, first + 1, first + size // 2, first + size - 2, first + size - 1}):
                out.extend((pos, x) for x in XORS)
    return out


def sevenzip(*args: str, cwd: pathlib.Path | None = None) -> subprocess.CompletedProcess:
    return subprocess.run([str(SEVENZIP), *args], capture_output=True, cwd=cwd)


# 7-Zip's own exit codes: OK, warning, fatal error, command line error, out
# of memory, stopped by the user. Anything else is a crash.
SEVENZIP_EXIT_CODES = (0, 1, 2, 7, 8, 255)
CRASH_RETRIES = 40
# Every crash seen, for the summary.
CRASHES: list[str] = []


def kind(text: str) -> str:
    """A 7-Zip message as one token: `Data Error` -> `Data_Error`."""
    return text.strip().replace(" ", "_")


def verdict(archive: pathlib.Path, one_thread: bool = False) -> str:
    """`7z t`'s verdict -- with `-mmt=off` if `one_thread` -- as one line:

    - `OK`: opened, every item tested good, nothing to say;
    - `OPENFAIL kinds`: not opened, with the reasons 7-Zip gave (its `ERRORS:`
      list after `Open ERROR`, e.g. `Is_not_archive`, `Headers_Error`,
      `Unexpected_end_of_archive`);
    - `OPEN errors=kinds warnings=kinds items=path:kind|...`: opened, with
      what it reported about the archive and, item by item, what failed --
      `#N` for a failure belonging to no file (a folder that failed after
      its last file).
    """
    threads = ["-mmt=off"] if one_thread else []
    # `-t7z`: 7-Zip's 7z handler and no other. Without it, an archive whose
    # 7z signature is damaged is tried as every other format 7-Zip knows --
    # and a BZip2 folder inside one opens as a .bz2 at offset 32.
    args = ["t", "-t7z", *threads, "-bso1", "-bse1", "-bsp0", "-sccUTF-8", "-psecret", str(archive)]
    # 7-Zip 26.00 itself sometimes dies -- an access violation, exit
    # 0xC0000005 -- testing a damaged LZMA2 archive of several blocks with
    # several threads: about one run in twelve on the worst mutants here,
    # the others agreeing. A crash is no verdict; run it again.
    for _ in range(CRASH_RETRIES):
        r = sevenzip(*args)
        if r.returncode in SEVENZIP_EXIT_CODES:
            break
        CRASHES.append(f"{archive.name}: exit {r.returncode:#x}")
    else:
        return "CRASH"
    lines = (r.stdout.decode("utf-8", "replace") + r.stderr.decode("utf-8", "replace")).splitlines()
    open_failed = any(line.startswith("Open ERROR") for line in lines)
    errors, warnings, items = [], [], []
    section = None
    for line in lines:
        s = line.strip()
        if s in ("ERRORS:", "WARNINGS:"):
            section = s
            continue
        if not s:
            section = None
            continue
        if section == "ERRORS:":
            errors.append(kind(s))
        elif section == "WARNINGS:":
            warnings.append(kind(s))
        elif s.startswith("ERROR: ") and " : " in s:
            what, item = s[len("ERROR: "):].rsplit(" : ", 1)
            items.append(f"{item.replace(chr(92), '/')}:{kind(what)}")
    if open_failed:
        return "OPENFAIL " + (",".join(sorted(set(errors))) or "-")
    if r.returncode == 0 and not errors and not warnings and not items:
        return "OK"
    return (
        f"OPEN errors={','.join(sorted(set(errors))) or '-'}"
        f" warnings={','.join(sorted(set(warnings))) or '-'}"
        f" items={'|'.join(items) or '-'}"
    )


def fnv(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & MASK64
    return h


# Every input's time, so that 7-Zip, which records them, writes the same
# archive every run: 2026-01-01 00:00:00 UTC.
FIXED_TIME = 1767225600


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
    # Folders last, as writing into one changes its time.
    for p in sorted(root.rglob("*"), key=lambda q: len(q.parts), reverse=True):
        os.utime(p, (FIXED_TIME, FIXED_TIME))


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
    listing = ["# D path | F size fnv64 path -- the tree every archive in made/ holds"]
    for path, data in sorted(TREE):
        listing.append(f"D {path}" if data is None else f"F {len(data)} {fnv(data):016x} {path}")
    (HERE / "input.txt").write_text("\n".join(listing) + "\n", encoding="utf-8", newline="\n")

    made = HERE / "made"
    if made.exists():
        shutil.rmtree(made)
    made.mkdir()
    lines = ["# archive verdict -- made by 7-Zip 26.00 with: args"]
    for name, args in MADE:
        archive = make(name, args, src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(args)}")

    small_src = HERE / "small-input"
    write_tree(small_src, SMALL_TREE)
    for name in MUTATED:
        archive = make(name, SMALL[name], small_src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(SMALL[name])}")
    shutil.rmtree(small_src)
    chunky_src = HERE / "chunky-input"
    write_tree(chunky_src, CHUNKY_TREE)
    for name, args in TARGETED.items():
        archive = make(name, args, chunky_src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(args)}")
    shutil.rmtree(chunky_src)
    shutil.rmtree(src)
    (HERE / "made.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    header = "# == archive, then: position xor verdict -- 7z t's"
    out = {False: [header], True: [header + " -mmt=off"]}
    tmp = HERE / "mutant.7z"
    plan = []
    for name in MUTATED:
        data = (made / f"{name}.7z").read_bytes()
        plan.append((name, data, [(pos, x) for pos in range(len(data)) for x in XORS]))
    for name in TARGETED:
        data = (made / f"{name}.7z").read_bytes()
        plan.append((name, data, targeted(data)))
    for name, data, cases in plan:
        for one_thread in (False, True):
            out[one_thread].append(f"== {name}.7z")
        for pos, x in cases:
            bad = bytearray(data)
            bad[pos] ^= x
            tmp.write_bytes(bytes(bad))
            for one_thread in (False, True):
                out[one_thread].append(f"{pos} {x:02x} {verdict(tmp, one_thread)}")
    tmp.unlink()
    (HERE / "mutations.txt").write_text("\n".join(out[False]) + "\n", encoding="utf-8", newline="\n")
    (HERE / "mutations-mmt-off.txt").write_text(
        "\n".join(out[True]) + "\n", encoding="utf-8", newline="\n"
    )
    differ = sum(a != b for a, b in zip(out[False][1:], out[True][1:]))
    print(f"{len(MADE)} made, {len(out[False])} mutation lines, {differ} differ with one thread")
    if CRASHES:
        print(f"7-Zip crashed {len(CRASHES)} time(s), each run again:")
        for c in sorted(set(CRASHES)):
            print(f"  {c} x{CRASHES.count(c)}")


if __name__ == "__main__":
    main()
