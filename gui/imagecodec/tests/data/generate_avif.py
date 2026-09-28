#!/usr/bin/env python3
"""Fetch the AVIF fixtures next to this script and record Pillow's answers.

The fixtures are libavif's own test files (libavif v1.3.0, `tests/data/`),
chosen because each exercises a rule of the container that no encoder
reachable from Python would write: items stored in `idat`, a `meta` box of
size 0, layered (progressive) images, grids with and without an alpha grid,
grids whose `dimg` references repeat or are reversed, tone-mapped (HDR gain
map) items of every metadata version libavif does and does not read, custom
and unknown properties, transforms wrongly marked inessential, an alpha item
without `ispe`, and image sequences with an alpha track, an audio track, Exif
and XMP. Every one of them is under libavif's licence (BSD-2-Clause, which
`licenses/libavif-LICENSE.txt` reproduces); the files are pinned by SHA-256.

Answers are Pillow's parse (`avifDecoderParse`, libavif 1.3.0 underneath),
one line per fixture in `avif_container.txt`:

    name ok WIDTHxHEIGHT MODE FRAMES ORIENTATION
    name err RESULT

`ORIENTATION` is the Exif orientation Pillow derives from `irot` and `imir`;
`RESULT` is libavif's own name for the refusal.

    python generate_avif.py [LIBAVIF_TESTS_DATA_DIR]

With no directory, the files are fetched from GitHub at the pinned tag.
"""

import hashlib
import pathlib
import sys
import urllib.request

from PIL import _avif

HERE = pathlib.Path(__file__).resolve().parent
TAG = "v1.3.0"
URL = f"https://raw.githubusercontent.com/AOMediaCodec/libavif/{TAG}/tests/data/{{}}.avif"

FILES = [
    ("white_1x1", "ea4e43d1f07e4c00de16c13afa32376111bb306e51f08212cf4c1b6064df3667"),
    ("extended_pixi", "7de53620b571aa61f54df2fc00cfa32955cd4e474a6a4b723a513b51ef21e946"),
    ("draw_points_idat", "ce2fd627efae49391ea82584e9beae05959b867ba429e688a2b95a015b38d3db"),
    ("draw_points_idat_metasize0", "a5f429bef6d2ef2f6022be4848d7266145bfaa060c0fe684150411fa4bf562a1"),
    ("draw_points_idat_progressive", "077ab2ad1e46dd912a973e4f024cb1eb242a08298be2dbf1a52a058e88c48a4a"),
    ("clop_irot_imor", "28e96ad4c913d75a32d66bce116f2963e29d93c01952698c7b33dd893f8bd541"),
    ("clap_irot_imir_non_essential", "33f869fcf2a879913eb394982b8fc03e9a60c25831aa37622ddefa656fd39fc1"),
    ("circle_custom_properties", "6c57595c1b814392c6a0d0e1f60e34c6f7f09a8ce7e46885d85059b7205e82fb"),
    ("colors-animated-8bpc", "2f8683d21725261f37f86e115f0c212cc52d0fefd3a2ddfcc4fa648c1859906d"),
    ("unsupported_gainmap_minimum_version", "d675f46519029ce3da98fac587cb25fa2eb33c7b77d7ae5c04903b2825367331"),
    ("unsupported_gainmap_version", "f67ef979ee9df50c7893eafea591030b3897dba1285ea8331dc0687e063dda9b"),
    (
        "supported_gainmap_writer_version_with_extra_bytes",
        "189398ea72d391c75c8679942c539ae7d4324152fa2f948d742b73f3bd6cc8f1",
    ),
    (
        "unsupported_gainmap_writer_version_with_extra_bytes",
        "1ede5af67433062cffd620833d2b132e1572348596c5659ad2ed4c4d708edd83",
    ),
    ("alpha_noispe", "8deb96e78c3e5d608a157b2de4c98eb1a30e0c85736b4230758400509c88d47e"),
    ("color_nogrid_alpha_nogrid_gainmap_grid", "d783e0d9ce778f972e88586b6b1b9eb062f54d38f28521721a8b9cbbda3b7fb0"),
    ("colors-animated-12bpc-keyframes-0-2-3", "3bf9f91da471749e7df639ba7945d4d94c1c3e3968c26f3619fbbcfc92790576"),
    ("color_grid_alpha_nogrid", "bae56368b348b1d847e2bfb662522599f0c63dfe62fb68826c9e42a300ff405d"),
    ("color_grid_alpha_grid_tile_shared_in_dimg", "1924ad27fa74aff5278367245d56e14804f6f5a6ac9fbc3da19b39033e167235"),
    ("color_grid_alpha_grid_gainmap_nogrid", "c424c43fe4bab3b8ef37b86c0bab3851b850b94e5d46b9fae979586dae45de0a"),
    ("color_grid_gainmap_different_grid", "73a68c3d6daad7b8298db975a00f02bca46b6c3f292eac09d3c1443d2006fab2"),
    ("colors-animated-8bpc-audio", "624f3bfe78b6bd75e9e12fe9b36c6132e3effaf82aa2a443f1b2a207a7d3561b"),
    ("colors-animated-8bpc-alpha-exif-xmp", "c2e38681057c15009c4b76ea08cea68cdde80806abd41d42a646f697bf5aabb2"),
    ("sofa_grid1x5_420", "c9e04ff9d90d7093454750fa33b7543ee5479e0cfb151e2c3d2ce6a16c1651c1"),
    ("sofa_grid1x5_420_dimg_repeat", "0a2abbe8b388df51e51b47cc4f1a932fed8c64d340490604fe6b510d77025514"),
    ("sofa_grid1x5_420_reversed_dimg_order", "8a77888b3d8b4876636666e4f8ebfaf6248361700b78e34b12dfc458919bd2b7"),
]


def fetch(name: str, source: pathlib.Path | None) -> bytes:
    if source is not None:
        return (source / f"{name}.avif").read_bytes()
    with urllib.request.urlopen(URL.format(name)) as response:
        return response.read()


def fixture_name(name: str) -> str:
    """`avif_` and the libavif name, with dashes as underscores."""
    return "avif_" + name.replace("-", "_")


def answer(data: bytes) -> str:
    try:
        decoder = _avif.AvifDecoder(data, "auto", 1)
        size, frames, mode, _icc, _exif, orientation, _xmp = decoder.get_info()
    except Exception as e:  # noqa: BLE001 - every refusal is an answer
        # "Failed to decode image: BMFF parsing failed" -> "BMFF parsing failed"
        return "err " + str(e).split(": ", 1)[-1]
    return f"ok {size[0]}x{size[1]} {mode} {frames} {orientation}"


def main() -> None:
    source = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else None
    lines = []
    for name, digest in FILES:
        data = fetch(name, source)
        got = hashlib.sha256(data).hexdigest()
        if got != digest:
            sys.exit(f"{name}.avif: sha256 {got}, expected {digest}")
        fixture = fixture_name(name)
        (HERE / f"{fixture}.avif").write_bytes(data)
        lines.append(f"{fixture} {answer(data)}")
    with open(HERE / "avif_container.txt", "w", encoding="ascii", newline="\n") as out:
        out.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
