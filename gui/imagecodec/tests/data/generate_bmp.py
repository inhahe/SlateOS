#!/usr/bin/env python3
"""Regenerate the BMP fixtures next to this script, and their answers.

Every answer is Chrome's decode of its fixture -- not an imitation of it, but
Chrome's own BMP decoder, compiled: image-rs 0.25.10 with the four patches
Chromium carries on it (`third_party/rust/chromium_crates_io/patches/
image-v0_25`), driven through the calls Skia's `rust/bmp/FFI.rs` makes. Chrome
shows a BMP only if both of those calls succeed -- any error, a short file
included, fails the image -- so an answer is either the pixels, or `REFUSED`
and why.

The crate and the patches are downloaded, checked against the SHA-256 sums
below, patched and built into a scratch program under the system's temporary
directory, the first time this runs. It needs `cargo`, `patch` (Git for
Windows has one) and a network connection; Pillow too, for the comparison at
the end, which says which fixtures Pillow decodes differently (it is not the
reference: Chrome is).

Answers are text: width, height, then `AARRGGBB` per pixel -- or `REFUSED`.

What the fixtures are for
-------------------------

Between them, every header (OS/2 1.x's 12 bytes; Windows' 40, 52, 56, 108 and
124; OS/2 2.x's short and long ones), every bit depth and storage (palettes of
1, 2, 4 and 8 bits, short, overlong and overlapping the pixels; 16- and 32-bit
bit fields, narrow, wide, odd, missing and with alpha; 24- and 32-bit colour),
every run-length encoding with every escape (end of line, end of bitmap,
deltas, absolute runs of odd length, runs past the end of a row, deltas past
the end of the picture, no end code at all), both row orders, and the files
Chrome refuses: cut short anywhere, masks that are not masks, compressions it
does not read, a colour profile that is not there.

Usage
-----

    python gui/imagecodec/tests/data/generate_bmp.py
"""

from __future__ import annotations

import hashlib
import io
import math
import pathlib
import random
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

HERE = pathlib.Path(__file__).parent

# ---------------------------------------------------------------------------
# The oracle: Chrome's BMP decoder
# ---------------------------------------------------------------------------

CRATE_URL = "https://crates.io/api/v1/crates/image/0.25.10/download"
CRATE_SHA256 = "85ab80394333c02fe689eaf900ab500fbd0c2213da414687ebf995a65d5a6104"
CHROMIUM = "cc113253170eacc79eb1eccae8ef0016b7749f6f"  # 2026-09-16, the patches' last change
PATCH_URL = (
    f"https://raw.githubusercontent.com/chromium/chromium/{CHROMIUM}/"
    "third_party/rust/chromium_crates_io/patches/image-v0_25/"
)
PATCHES = {
    "0001-bmp-decoder-Add-streaming-resumable-BMP-decoding-sup.patch":
        "579b93d71d830ae8fc60a127f8abc8ec95a022c2e6222491556373206b0fbde2",
    "0002-bmp-decoder-Honour-alpha_mask-in-BITMAPV4HEADER-V5HE.patch":
        "89e96f772395fb161d0fdf623e8a1885fca7bb76797d8d1785ce8227a59f367a",
    "0003-bmp-decoder-Validate-ICC-profile-range-before-readin.patch":
        "67a6e018008836d6634043b88b036d107a575b9f65e2b1420dd4033b3eb95a74",
    "0004-bmp-decoder-Apply-lenient-RLE-overflow-handling.patch":
        "c6f25c91be81c23299cf7be1a1fee0f0d89e884d006f01f1c889ed19e122e221",
}

ORACLE_MAIN = r'''
//! Decode each BMP named on the command line as Chrome does, and write the
//! answer beside it: `REFUSED <why>`, or width, height and `AARRGGBB` pixels.
use std::io::Cursor;

use image::codecs::bmp::BmpDecoder;
use image::ImageDecoder;

fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u32>), String> {
    // As Skia's rust/bmp/FFI.rs: a resumable decoder over the whole file,
    // `read_metadata`, then `read_image_data` into RGB or RGBA rows.
    let mut decoder = BmpDecoder::new_resumable(Cursor::new(bytes.to_vec()));
    decoder.read_metadata().map_err(|e| format!("metadata: {e}"))?;
    let (w, h) = decoder.dimensions();
    // Past the tests' own limits (imagecodec's default, the compositor's
    // largest buffer) nothing is compared: a corrupted header can ask for
    // gigabytes, and failing to allocate them aborts rather than unwinds.
    if u64::from(w) * u64::from(h) > 7680 * 4320 {
        return Err(format!("larger than the tests' limits ({w}x{h})"));
    }
    let channels = match decoder.color_type() {
        image::ColorType::Rgb8 => 3usize,
        image::ColorType::Rgba8 => 4,
        other => return Err(format!("colour type {other:?}")),
    };
    let size = (w as usize) * (h as usize) * channels;
    let mut buf = vec![0u8; size];
    decoder.read_image_data(&mut buf).map_err(|e| format!("pixels: {e}"))?;
    let pixels = buf
        .chunks_exact(channels)
        .map(|p| u32::from_be_bytes([if channels == 4 { p[3] } else { 0xFF }, p[0], p[1], p[2]]))
        .collect();
    Ok((w, h, pixels))
}

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        let text = match std::panic::catch_unwind(|| decode(&bytes)) {
            Ok(Ok((w, h, pixels))) => {
                let mut words = vec![w.to_string(), h.to_string()];
                words.extend(pixels.iter().map(|p| format!("{p:08X}")));
                words.join(" ")
            }
            Ok(Err(why)) => format!("REFUSED {why}"),
            Err(_) => "REFUSED panic".to_string(),
        };
        std::fs::write(std::path::Path::new(&path).with_extension("txt"), text + "\n").unwrap();
    }
}
'''


def fetch(url: str, sha256: str) -> bytes:
    with urllib.request.urlopen(url) as response:
        data = response.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        sys.exit(f"{url}: SHA-256 {got}, expected {sha256}")
    return data


def build_oracle() -> pathlib.Path:
    """Chrome's BMP decoder as a program; built once, then reused."""
    root = pathlib.Path(tempfile.gettempdir()) / f"slateos-bmp-oracle-{CHROMIUM[:10]}-2"
    exe = root / "target" / "release" / ("bmp_oracle.exe" if sys.platform == "win32" else "bmp_oracle")
    if exe.exists():
        return exe
    shutil.rmtree(root, ignore_errors=True)
    (root / "src").mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(fetch(CRATE_URL, CRATE_SHA256))) as crate:
        crate.extractall(root, filter="data")
    image = root / "image-0.25.10"
    for name, sha256 in PATCHES.items():
        patch = root / name
        patch.write_bytes(fetch(PATCH_URL + name, sha256))
        subprocess.run(["patch", "-p6", "-d", str(image), "-i", str(patch)], check=True)
    (root / "Cargo.toml").write_text(
        '[package]\nname = "bmp_oracle"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n\n'
        '[dependencies]\nimage = { path = "image-0.25.10", default-features = false, features = ["bmp"] }\n'
    )
    (root / "src" / "main.rs").write_text(ORACLE_MAIN)
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=root, check=True)
    return exe


# ---------------------------------------------------------------------------
# Writing BMPs
# ---------------------------------------------------------------------------


def dib(size: int, w: int, h: int, bits: int, compression: int = 0, colors_used: int = 0,
        masks: tuple[int, int, int, int] = (0, 0, 0, 0), cs_type: int = 0x73524742,
        profile: tuple[int, int] = (0, 0)) -> bytes:
    """A bitmap header of `size` bytes: Windows' 40, 52, 56, 108 or 124, or an
    OS/2 2.x one of 16 to 64 (a BITMAPINFOHEADER cut short or zero-padded)."""
    head = struct.pack("<IiiHHIIiiII", size, w, h, 1, bits, compression, 0, 2835, 2835, colors_used, 0)
    if size in (52, 56, 108, 124):
        head += struct.pack("<III", *masks[:3])
    if size in (56, 108, 124):
        head += struct.pack("<I", masks[3])
    if size in (108, 124):
        head += struct.pack("<I", cs_type) + bytes(36) + bytes(12)
    if size == 124:
        head += struct.pack("<IIII", 4, profile[0], profile[1], 0)
    return (head + bytes(max(0, size - len(head))))[:size]


def core(w: int, h: int, bits: int) -> bytes:
    """OS/2 1.x's BITMAPCOREHEADER."""
    return struct.pack("<IHHHH", 12, w, h, 1, bits)


def bmp(header: bytes, *parts: bytes, pixels: bytes, offset: int | None = None) -> bytes:
    """A file: the file header, `header`, whatever `parts` follow it (masks, a
    palette), then the pixels at `offset` -- by default right after the parts;
    past them, the gap is zeros; before them, the parts are cut there."""
    body = header + b"".join(parts)
    start = 14 + len(body) if offset is None else offset
    head = b"BM" + struct.pack("<IHHI", start + len(pixels), 0, 0, start) + body
    head = head[:start] + bytes(max(0, start - len(head)))
    return head + pixels


def palette(colours: list[tuple[int, int, int]], entry: int = 4) -> bytes:
    return b"".join(bytes([b, g, r]) + bytes(entry - 3) for r, g, b in colours)


def pad(row: bytes) -> bytes:
    return row + bytes(-len(row) % 4)


def rows(pixel_rows: list[bytes], top_down: bool = False) -> bytes:
    """Rows as stored: padded, and bottom row first unless top-down."""
    ordered = pixel_rows if top_down else pixel_rows[::-1]
    return b"".join(pad(r) for r in ordered)


def packed(indices: list[int], bits: int) -> bytes:
    """Palette indices packed `bits` to a byte, most significant first."""
    per = 8 // bits
    out = bytearray()
    for i in range(0, len(indices), per):
        byte = 0
        for j, index in enumerate(indices[i : i + per]):
            byte |= index << (8 - bits * (j + 1))
        out.append(byte)
    return bytes(out)


# ---------------------------------------------------------------------------
# Pictures
# ---------------------------------------------------------------------------


def photo(w: int, h: int) -> list[list[tuple[int, int, int]]]:
    """Gradients, texture and a disc, as the other generators draw."""
    out = []
    for y in range(h):
        row = []
        for x in range(w):
            r = int(128 + 100 * math.sin(x / 7.0) * math.cos(y / 11.0))
            g = (x * 255) // max(1, w - 1)
            b = (y * 255) // max(1, h - 1)
            if (x - w * 0.62) ** 2 + (y - h * 0.4) ** 2 < (min(w, h) * 0.22) ** 2:
                r, g, b = 250, 40, 60
            row.append((max(0, min(255, r)), g, b))
        out.append(row)
    return out


def bgr(row: list[tuple[int, int, int]]) -> bytes:
    return b"".join(bytes([b, g, r]) for r, g, b in row)


def grid(w: int, h: int, n: int, seed: int) -> list[list[int]]:
    """Indices below `n`, in blocks and noise."""
    rng = random.Random(seed)
    return [[((x // 3) + (y // 2) * 3 + rng.randrange(2)) % n for x in range(w)] for y in range(h)]


def colours(n: int, seed: int = 7) -> list[tuple[int, int, int]]:
    rng = random.Random(seed)
    return [(rng.randrange(256), rng.randrange(256), rng.randrange(256)) for _ in range(n)]


def words16(w: int, h: int, seed: int) -> list[list[int]]:
    rng = random.Random(seed)
    return [[rng.randrange(0x10000) for _ in range(w)] for _ in range(h)]


def words32(w: int, h: int, seed: int) -> list[list[int]]:
    rng = random.Random(seed)
    return [[rng.randrange(1 << 32) for _ in range(w)] for _ in range(h)]


def le16(values: list[int]) -> bytes:
    return b"".join(struct.pack("<H", v) for v in values)


def le32(values: list[int]) -> bytes:
    return b"".join(struct.pack("<I", v) for v in values)


# ---------------------------------------------------------------------------
# Run-length encoding
# ---------------------------------------------------------------------------

EOL, EOF = b"\0\0", b"\0\1"


def delta(dx: int, dy: int) -> bytes:
    return bytes([0, 2, dx, dy])


def rle8_row(indices: list[int]) -> bytes:
    """A row as RLE8: runs of three or more encoded, the rest absolute (at
    least three at a time, as the format needs), shorter leftovers encoded."""
    out = bytearray()
    i, n = 0, len(indices)
    literal: list[int] = []

    def flush() -> None:
        nonlocal literal
        while literal:
            chunk, literal = literal[:255], literal[255:]
            if len(chunk) >= 3:
                out.extend([0, len(chunk)] + chunk + [0] * (len(chunk) & 1))
            else:
                for index in chunk:
                    out.extend([1, index])

    while i < n:
        run = 1
        while i + run < n and indices[i + run] == indices[i] and run < 255:
            run += 1
        if run >= 3:
            flush()
            out.extend([run, indices[i]])
            i += run
        else:
            literal.append(indices[i])
            i += 1
    flush()
    return bytes(out)


def rle4_row(indices: list[int]) -> bytes:
    """A row as RLE4: pairs that alternate encoded, the rest absolute."""
    out = bytearray()
    i, n = 0, len(indices)
    while i < n:
        a = indices[i]
        b = indices[i + 1] if i + 1 < n else a
        run = 1
        while i + run < n and indices[i + run] == (a if run % 2 == 0 else b) and run < 255:
            run += 1
        if run >= 4 or i + run >= n:
            out.extend([run, (a << 4) | b])
            i += run
        else:
            chunk = indices[i : i + min(255, max(3, n - i))][: max(3, min(8, n - i))]
            if len(chunk) < 3:
                out.extend([len(chunk), (chunk[0] << 4) | (chunk[1] if len(chunk) > 1 else 0)])
            else:
                body = packed(chunk + [0] * (len(chunk) & 1), 4)
                out.extend([0, len(chunk)])
                out.extend(body + bytes(len(body) & 1))
            i += len(chunk)
    return bytes(out)


def rle24_row(row: list[tuple[int, int, int]]) -> bytes:
    """A row as OS/2 RLE24: encoded runs (count, B, G, R), absolute runs of
    three or more triples padded to an even number of bytes."""
    out = bytearray()
    i, n = 0, len(row)
    while i < n:
        run = 1
        while i + run < n and row[i + run] == row[i] and run < 255:
            run += 1
        if run >= 2 or n - i < 3:
            r, g, b = row[i]
            out.extend([run, b, g, r])
            i += run
        else:
            chunk = row[i : i + min(5, n - i)]
            out.extend([0, len(chunk)])
            out.extend(bgr(chunk))
            if len(chunk) % 2:
                out.append(0)
            i += len(chunk)
    return bytes(out)


# ---------------------------------------------------------------------------
# The fixtures
# ---------------------------------------------------------------------------


def fixtures() -> dict[str, bytes]:
    f: dict[str, bytes] = {}
    w, h = 13, 9
    pic = photo(w, h)

    # Plain colour: 24-bit both ways up, and 32-bit under each header.
    f["bmp_24"] = bmp(dib(40, w, h, 24), pixels=rows([bgr(r) for r in pic]))
    f["bmp_24_top_down"] = bmp(dib(40, w, -h, 24), pixels=rows([bgr(r) for r in pic], top_down=True))
    f["bmp_24_1x1"] = bmp(dib(40, 1, 1, 24), pixels=rows([bgr([(9, 200, 77)])]))
    rng = random.Random(3)
    alphas = [[rng.randrange(256) for _ in range(w)] for _ in range(h)]
    bgra = [b"".join(bytes([b, g, r, a]) for (r, g, b), a in zip(row, arow)) for row, arow in zip(pic, alphas)]
    f["bmp_32_v3_fourth_byte"] = bmp(dib(40, w, h, 32), pixels=rows(bgra))
    f["bmp_32_v5_alpha"] = bmp(dib(124, w, h, 32, masks=(0xFF0000, 0xFF00, 0xFF, 0xFF000000)), pixels=rows(bgra))
    f["bmp_32_v4_odd_alpha_mask"] = bmp(dib(108, w, h, 32, masks=(0, 0, 0, 0x000000FF)), pixels=rows(bgra))
    zero = [b"".join(bytes([b, g, r, 0]) for (r, g, b) in row) for row in pic]
    f["bmp_32_v4_zero_alpha"] = bmp(dib(108, w, h, 32, masks=(0xFF0000, 0xFF00, 0xFF, 0xFF000000)), pixels=rows(zero))
    f["bmp_32_v3_56_alpha_mask"] = bmp(dib(56, w, h, 32, masks=(0xFF0000, 0xFF00, 0xFF, 0xFF000000)), pixels=rows(bgra))

    # Bit fields, 32-bit.
    bgra_masks = (0xFF0000, 0xFF00, 0xFF, 0xFF000000)
    f["bmp_32_bitfields_bgra"] = bmp(dib(56, w, h, 32, 3, masks=bgra_masks), pixels=rows(bgra))
    f["bmp_32_bitfields_info"] = bmp(dib(40, w, h, 32, 3), le32(bgra_masks[:3]), pixels=rows(bgra))
    f["bmp_32_alphabitfields"] = bmp(dib(40, w, h, 32, 6), le32(list(bgra_masks)), pixels=rows(bgra))
    wide = words32(w, h, 5)
    f["bmp_32_bitfields_10bit"] = bmp(
        dib(56, w, h, 32, 3, masks=(0x3FF00000, 0x000FFC00, 0x000003FF, 0xC0000000)),
        pixels=rows([le32(r) for r in wide]),
    )
    f["bmp_32_bitfields_scattered"] = bmp(
        dib(108, w, h, 32, 3, masks=(0x0000FF00, 0xFF000000, 0x00FF0000, 0)),
        pixels=rows([le32(r) for r in wide]),
    )
    f["bmp_32_bitfields_narrow"] = bmp(
        dib(124, w, h, 32, 3, masks=(0x7 << 20, 0x3 << 9, 0x3F, 0x1 << 31)),
        pixels=rows([le32(r) for r in wide]),
    )
    f["bmp_32_bitfields_no_green"] = bmp(
        dib(52, w, h, 32, 3, masks=(0xFF0000, 0, 0xFF, 0)), pixels=rows([le32(r) for r in wide])
    )

    # 16-bit.
    ws = words16(w, h, 11)
    f["bmp_16_555"] = bmp(dib(40, w, h, 16), pixels=rows([le16(r) for r in ws]))
    f["bmp_16_565"] = bmp(dib(40, w, h, 16, 3), le32([0xF800, 0x07E0, 0x001F]), pixels=rows([le16(r) for r in ws]))
    f["bmp_16_4444"] = bmp(dib(56, w, h, 16, 3, masks=(0x0F00, 0x00F0, 0x000F, 0xF000)), pixels=rows([le16(r) for r in ws]))
    f["bmp_16_1555_alphabitfields"] = bmp(
        dib(40, w, h, 16, 6), le32([0x7C00, 0x03E0, 0x001F, 0x8000]), pixels=rows([le16(r) for r in ws])
    )
    f["bmp_16_v2"] = bmp(dib(52, w, h, 16, 3, masks=(0xF800, 0x07E0, 0x001F, 0)), pixels=rows([le16(r) for r in ws]))
    f["bmp_16_top_down_332"] = bmp(
        dib(56, w, -h, 16, 3, masks=(0xE0, 0x1C, 0x03, 0x7F00)), pixels=rows([le16(r) for r in ws], top_down=True)
    )

    # Palettes.
    for bits, n in [(1, 2), (2, 4), (4, 16), (8, 256)]:
        idx = grid(17, 7, n, bits)
        f[f"bmp_{bits}"] = bmp(dib(40, 17, 7, bits), palette(colours(n)), pixels=rows([packed(r, bits) for r in idx]))
    idx = grid(33, 5, 2, 9)
    f["bmp_1_odd_width"] = bmp(dib(40, 33, 5, 1), palette(colours(2)), pixels=rows([packed(r, 1) for r in idx]))
    idx = grid(w, h, 40, 4)
    f["bmp_8_short_palette"] = bmp(dib(40, w, h, 8, 0, 10), palette(colours(10)), pixels=rows([bytes(r) for r in idx]))
    f["bmp_8_too_many_colours"] = bmp(dib(40, w, h, 8, 0, 1000), palette(colours(256)), pixels=rows([bytes(r) for r in idx]))
    f["bmp_4_colours_used"] = bmp(
        dib(40, w, h, 4, 0, 5), palette(colours(5)), pixels=rows([packed([i % 16 for i in r], 4) for r in idx])
    )
    f["bmp_8_top_down"] = bmp(dib(40, w, -h, 8), palette(colours(256)), pixels=rows([bytes(r) for r in idx], top_down=True))
    # The palette runs into the pixels: all 256 entries are read from where
    # it starts, whatever the pixel offset says, and the pixels from there.
    big = grid(40, 30, 256, 6)
    f["bmp_8_palette_overlaps_pixels"] = bmp(
        dib(40, 40, 30, 8), palette(colours(256)), pixels=rows([bytes(r) for r in big]), offset=14 + 40 + 64
    )
    f["bmp_8_gap_before_pixels"] = bmp(
        dib(40, w, h, 8, 0, 16), palette(colours(16)), pixels=rows([bytes([i % 16 for i in r]) for r in idx]),
        offset=14 + 40 + 64 + 37,
    )

    # OS/2.
    idx = grid(w, h, 256, 12)
    f["bmp_core_8"] = bmp(core(w, h, 8), palette(colours(256), 3), pixels=rows([bytes(r) for r in idx]))
    f["bmp_core_1"] = bmp(core(w, h, 1), palette(colours(2), 3), pixels=rows([packed([i % 2 for i in r], 1) for r in idx]))
    f["bmp_core_24"] = bmp(core(w, h, 24), pixels=rows([bgr(r) for r in pic]))
    f["bmp_os2_16"] = bmp(dib(16, w, h, 8), palette(colours(256)), pixels=rows([bytes(r) for r in idx]))
    f["bmp_os2_64"] = bmp(dib(64, w, h, 24), pixels=rows([bgr(r) for r in pic]))
    f["bmp_os2_42"] = bmp(dib(42, w, h, 4), palette(colours(16)), pixels=rows([packed([i % 16 for i in r], 4) for r in idx]))
    # OS/2 bit fields are three masks from where the header ends, whatever the
    # compression code says: ALPHABITFIELDS here reads no alpha mask.
    f["bmp_os2_44_alphabitfields"] = bmp(dib(44, w, h, 32, 6), le32(list(bgra_masks[:3])), pixels=rows(bgra))
    flat = [[pic[y][x] if (x + y) % 5 else (0, 0, 0) for x in range(w)] for y in range(h)]
    flat = [[row[x - x % 3] for x in range(w)] for row in flat]
    stream = b"".join(rle24_row(r) + EOL for r in flat[::-1]) + EOF
    f["bmp_os2_rle24"] = bmp(dib(64, w, h, 24, 4), pixels=stream)

    # Run-length encoded.
    idx = [[(x // 4 + y // 3) % 7 if (x * y) % 11 else 9 for x in range(21)] for y in range(8)]
    pal16 = palette(colours(16))
    stream = b"".join(rle8_row(r) + EOL for r in idx[::-1]) + EOF
    f["bmp_rle8"] = bmp(dib(40, 21, 8, 8, 1, 16), pal16, pixels=stream)
    f["bmp_rle8_no_end_code"] = bmp(dib(40, 21, 8, 8, 1, 16), pal16, pixels=b"".join(rle8_row(r) + EOL for r in idx[::-1]))
    f["bmp_rle8_top_down"] = bmp(dib(40, 21, -8, 8, 1, 16), pal16, pixels=b"".join(rle8_row(r) + EOL for r in idx) + EOF)
    f["bmp_rle4"] = bmp(dib(40, 21, 8, 4, 2), pal16, pixels=b"".join(rle4_row(r) + EOL for r in idx[::-1]) + EOF)
    # Escapes by hand, on a 10x6 picture: skipped pixels are black.
    pal = palette(colours(16, 21))
    f["bmp_rle8_delta"] = bmp(dib(40, 10, 6, 8, 1, 16), pal, pixels=bytes(
        [3, 1, 0, 3, 4, 5, 6, 0]  # three 1s, then 4 5 6 (padded)
        + list(delta(2, 1))       # right 2, up a row: x is now 8
        + [2, 7]                  # two 7s at 8, 9
        + list(EOL)
        + list(delta(4, 2))       # from the start of row 2: right 4, up 2 rows
        + [1, 2]
        + list(EOF)))
    f["bmp_rle8_past_the_row"] = bmp(dib(40, 10, 4, 8, 1, 16), pal, pixels=bytes(
        [8, 3, 5, 4]              # thirteen pixels on a ten-pixel row: three dropped
        + [0, 5, 1, 2, 3, 4, 5, 0]  # and five more, dropped
        + list(EOL)
        + [0, 12] + list(range(12)) + [4, 6]  # an absolute run of twelve, cut at ten
        + list(EOL)
        + list(delta(1, 1))       # from x = 16 on row 1 to row 2: the skip carries 16
        + [3, 9] + list(EOL)
        + [10, 2]))              # row 3, and no end code: every row is done
    f["bmp_rle8_delta_past_the_end"] = bmp(dib(40, 10, 4, 8, 1, 16), pal, pixels=bytes(
        [10, 4] + list(EOL) + [5, 6] + list(delta(0, 9)) + [7, 7, 7, 7]))
    f["bmp_rle8_early_end"] = bmp(dib(40, 10, 6, 8, 1, 16), pal, pixels=bytes([10, 3] + list(EOL) + [4, 5] + list(EOF)))
    f["bmp_rle8_index_past_palette"] = bmp(dib(40, 10, 2, 8, 1, 4), palette(colours(4)), pixels=bytes(
        [5, 2, 5, 200] + list(EOL) + [0, 4, 1, 250, 3, 9] + list(EOF)))
    f["bmp_rle4_by_hand"] = bmp(dib(40, 11, 3, 4, 2), pal, pixels=bytes(
        [7, 0x1A]                 # 1 A 1 A 1 A 1
        + [0, 3, 0x23, 0x40, 0, 0]  # absolute 2 3 4, padded to four bytes
        + list(EOL)
        + [0, 5, 0x56, 0x78, 0x90, 0]  # absolute five
        + [9, 0xBC]               # nine on a row with six left: three dropped
        + list(EOL)
        + list(delta(3, 0)) + [2, 0xDE] + list(EOF)))

    # Headers Chrome reads that Pillow does not, or reads differently.
    f["bmp_v5_icc_profile"] = bmp(
        dib(124, 4, 2, 24, cs_type=0x4D424544, profile=(124 + 8 * 2 + 4, 8)),
        pixels=rows([bgr(pic[0][:4]), bgr(pic[1][:4])]) + b"ICCPROFL",
    )

    # Refused: cut short, masks that are not masks, what Chrome does not read.
    whole = f["bmp_24"]
    f["bmp_refused_short_padding"] = whole[:-1]
    f["bmp_refused_short_header"] = whole[:40]
    # An 8-bit palette of 256 entries, as no colour count says otherwise, in a
    # file too short to hold one.
    f["bmp_refused_palette_past_end"] = bmp(
        dib(40, 10, 2, 8, 1), palette(colours(16)), pixels=bytes([10, 1] + list(EOL) + [10, 2] + list(EOF))
    )
    f["bmp_refused_rle8_runs_out"] = f["bmp_rle8"][:-40]
    f["bmp_refused_icc_past_end"] = bmp(
        dib(124, 4, 2, 24, cs_type=0x4D424544, profile=(4000, 64)),
        pixels=rows([bgr(pic[0][:4]), bgr(pic[1][:4])]),
    )
    f["bmp_refused_mask_gap"] = bmp(dib(40, w, h, 16, 3), le32([0xF00F, 0x00F0, 0x0F00]), pixels=rows([le16(r) for r in ws]))
    f["bmp_refused_mask_too_wide"] = bmp(dib(40, w, h, 16, 3), le32([0x1F0000, 0x07E0, 0x001F]), pixels=rows([le16(r) for r in ws]))
    f["bmp_refused_jpeg"] = bmp(dib(40, w, h, 24, 4), pixels=b"\xff\xd8\xff\xd9")
    f["bmp_refused_rle8_as_4_bits"] = bmp(dib(40, 10, 2, 4, 1), pal, pixels=bytes([10, 1] + list(EOL) + [10, 2] + list(EOF)))
    f["bmp_refused_zero_height"] = bmp(dib(40, 4, 0, 24), pixels=b"")
    f["bmp_refused_too_wide"] = bmp(dib(40, 65536, 1, 24), pixels=b"")
    f["bmp_refused_header_100"] = bmp(dib(100, 4, 2, 24), pixels=rows([bgr(pic[0][:4]), bgr(pic[1][:4])]))
    f["bmp_refused_24_bit_core_16"] = bmp(core(4, 2, 16), pixels=bytes(16))
    return f


def pillow_fixtures() -> dict[str, bytes]:
    """What Pillow writes, which is what much of the world does."""
    from PIL import Image

    out = {}
    pic = photo(19, 11)
    rgb = Image.new("RGB", (19, 11))
    rgb.putdata([p for row in pic for p in row])
    rgba = rgb.convert("RGBA")
    rgba.putdata([(r, g, b, (x * 37 + y * 11) % 256) for y, row in enumerate(pic) for x, (r, g, b) in enumerate(row)])
    for name, img in [
        ("bmp_pillow_rgb", rgb),
        ("bmp_pillow_rgba", rgba),
        ("bmp_pillow_p", rgb.quantize(37)),
        ("bmp_pillow_l", rgb.convert("L")),
        ("bmp_pillow_1", rgb.convert("1")),
    ]:
        buf = io.BytesIO()
        img.save(buf, "BMP")
        out[name] = buf.getvalue()
    return out


def compare_with_pillow(names: list[str]) -> None:
    """Say which fixtures Pillow reads differently from Chrome."""
    from PIL import Image

    for name in names:
        answer = (HERE / f"{name}.txt").read_text().split()
        try:
            img = Image.open(HERE / f"{name}.bmp")
            img.load()
            rgba = img.convert("RGBA")
            raw = rgba.tobytes()
            got = [str(rgba.width), str(rgba.height)] + [
                f"{raw[i + 3]:02X}{raw[i]:02X}{raw[i + 1]:02X}{raw[i + 2]:02X}" for i in range(0, len(raw), 4)
            ]
        except Exception as e:
            got = ["REFUSED", type(e).__name__]
        if answer[0] == "REFUSED" and got[0] == "REFUSED":
            verdict = "both refuse"
        elif answer == got:
            verdict = "same"
        elif answer[0] == "REFUSED":
            verdict = "Chrome refuses, Pillow decodes"
        elif got[0] == "REFUSED":
            verdict = f"Pillow refuses ({got[1]})"
        else:
            differ = sum(a != b for a, b in zip(answer[2:], got[2:]))
            verdict = f"{differ} pixels differ"
        print(f"  {name}: {verdict}")


def main() -> None:
    oracle = build_oracle()
    made = fixtures() | pillow_fixtures()
    for name, data in made.items():
        (HERE / f"{name}.bmp").write_bytes(data)
    subprocess.run([str(oracle)] + [str(HERE / f"{name}.bmp") for name in made], check=True)
    print("Pillow against Chrome:")
    compare_with_pillow(sorted(made))


if __name__ == "__main__":
    main()
