"""Mutation test for imagecodec's AVIF colour as Chrome shows it: which
pictures Chrome converts -- HDR, and SDR whose primaries or curve are not
sRGB's (src/avif/convert.rs) -- and the content light level HDR is tone
mapped by, from the `clli` box to the conversion (src/avif/setup.rs,
src/avif/decode.rs).

Each row puts back one way of not showing an AVIF as Chrome shows it -- the
light not read, a picture Chrome converts left alone or the other way about
-- and names the tests that have to notice: the module's own, and
`tests/avif_hdr.rs` and `tests/avif_sdr.rs`, which hold the decoding to
Chrome 154's own screenshots (`tests/data/generate_avif_hdr.py`,
`generate_avif_sdr.py`).

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The sweep runs the crate's unit tests and those two alone: the
crate's other suites decode libavif's and libjpeg-turbo's whole test sets,
and name nothing here.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "avif"
TARGETS = ("--lib", "--test", "avif_hdr", "--test", "avif_sdr")

WHICH = "what_chrome_converts_is_what_its_avif_decoder_reads"
SDR = "sdr_conversions_are_chrome_s_within_a_level"
PATCHES = "hdr_patches_are_chrome_s_pixel_for_pixel"
MOVES = "the_content_light_level_moves_the_tone_map"

CONVERT = [
    (
        "MaxCLL is not the light",
        "                max_cll: image.light.map_or(0.0, |(max_cll, _)| f32::from(max_cll)),",
        "                max_cll: 0.0,",
        [PATCHES, MOVES],
    ),
    (
        "a picture with an ICC profile is HDR by its code points",
        "    if image.icc {",
        "    if false {",
        [WHICH],
    ),
    (
        "unspecified primaries are BT.2020's",
        "    let primaries = if image.primaries == UNSPECIFIED {\n        1",
        "    let primaries = if image.primaries == UNSPECIFIED {\n        9",
        [WHICH],
    ),
    (
        "primaries Chrome has no name for are HDR",
        "    let primaries_named = matches!(primaries, 1 | 4..=12 | 22);",
        "    let primaries_named = true;",
        [WHICH],
    ),
    (
        "matrices Chrome has no name for are HDR",
        "    let matrix_named = matches!(matrix, 0..=2 | 4..=9 | 11);",
        "    let matrix_named = true;",
        [WHICH],
    ),
    (
        "an unspecified transfer is no curve",
        "    let transfer = if image.transfer == UNSPECIFIED {\n        SRGB",
        "    let transfer = if image.transfer == UNSPECIFIED {\n        UNSPECIFIED",
        [WHICH, SDR],
    ),
    (
        "sRGB's own colour is converted",
        "    (!srgb).then_some((transfer, primaries))",
        "    Some((transfer, primaries))",
        [WHICH],
    ),
    (
        "SDR is not converted",
        "    (!srgb).then_some((transfer, primaries))",
        "    transfer.is_hdr().then_some((transfer, primaries))",
        [WHICH, SDR],
    ),
]

DECODE = [
    (
        "the clli does not reach the picture",
        "        light: picture.clli,",
        "        light: None,",
        [PATCHES, MOVES],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [(SRC / "convert.rs", CONVERT), (SRC / "decode.rs", DECODE)]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "imagecodec", timeout=600, only=mine or None,
                             targets=TARGETS))
    raise SystemExit(max(results))
