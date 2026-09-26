"""Mutation test for `pngwrite`, the tree's PNG writer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Every test reads what it wrote back through `imagecodec`, the decoder the
system uses, so a mutant that writes a PNG no decoder reads -- or one that
reads back as a different picture -- is caught by a real reader, not by a
second copy of the writer's own assumptions.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

RGB = "an_opaque_picture_round_trips_as_rgb"
RGBA = "a_transparent_picture_round_trips_as_rgba"
FILTERED = "each_row_is_filtered_by_what_it_resembles"
INVERTS = "every_filter_inverts"
REFUSED = "a_picture_that_cannot_be_a_png_is_refused"
THIN = "thin_pictures_round_trip"

MUTATIONS = [
    (
        "transparency is dropped",
        "    let alpha = pixels.iter().any(|p| p >> 24 != 0xFF);",
        "    let alpha = false;",
        [RGBA],
    ),
    (
        "every picture is written with alpha",
        "    let alpha = pixels.iter().any(|p| p >> 24 != 0xFF);",
        "    let alpha = true;",
        [RGB],
    ),
    (
        "the channels are written blue first",
        "            current.extend_from_slice(&[r, g, b]);",
        "            current.extend_from_slice(&[b, g, r]);",
        [RGB, RGBA],
    ),
    (
        "every row is left unfiltered",
        "        for filter in 0..=4_u8 {",
        "        for filter in 0..=0_u8 {",
        [FILTERED],
    ),
    (
        "the costliest filter is chosen",
        "            if cost < best_cost {",
        "            if cost > best_cost || best_cost == u64::MAX {",
        [FILTERED],
    ),
    (
        "the row above is forgotten",
        "        std::mem::swap(&mut previous, &mut current);",
        "",
        [FILTERED],
    ),
    (
        "Sub subtracts the byte above",
        "            1 => x.wrapping_sub(a),",
        "            1 => x.wrapping_sub(b),",
        # Not RGB: its picture happens never to choose Sub.
        [INVERTS, FILTERED, THIN],
    ),
    (
        "Average rounds up",
        "                let average = u16::midpoint(u16::from(a), u16::from(b));",
        "                let average = u16::midpoint(u16::from(a), u16::from(b)).saturating_add(u16::from((a ^ b) & 1));",
        [INVERTS],
    ),
    # Not "Paeth prefers above to left": a tie between left and above that
    # beats above-left needs the two to be equal, so the swap is an
    # equivalent mutant.
    (
        "Paeth prefers above-left to above on a tie",
        "    } else if pb <= pc {",
        "    } else if pb < pc {",
        [INVERTS],
    ),
    (
        "the left neighbour is one byte back, not a pixel",
        "        let left = i.checked_sub(bpp);",
        "        let left = i.checked_sub(1);",
        [INVERTS, FILTERED, THIN, RGBA],
    ),
    (
        "the chunk CRC leaves out the chunk's kind",
        "    let crc = crc32::crc32_seed(crc32::crc32_raw(!0, kind), data);",
        "    let crc = crc32::crc32(data);",
        [RGB],
    ),
    (
        "an empty picture is written",
        "    if width == 0 || height == 0 {\n        return Err(EncodeError::Empty);\n    }",
        "",
        [REFUSED],
    ),
    (
        "a long pixel list is written",
        "    if pixels.len() != expected {",
        "    if pixels.len() < expected {",
        [REFUSED],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "pngwrite", timeout=300, only=only))
