"""Mutation test for `exif`, the shared reader of a photograph's EXIF.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Two mutations are deliberately absent.  Removing the depth guard outright
makes a directory that points at itself recurse until the stack runs out,
which takes the whole test binary with it -- the harness cannot say which test
saw it; raising the limit by one (`MAX_DEPTH`) is the row that stands for it.
And removing a bounds check means indexing past the end, which is a panic the
clippy lints forbid writing in the first place.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

JPEG = "a_jpeg_camera_file_is_read_whole"
PNG = "a_png_exif_chunk_before_the_image_data_is_read"
WEBP = "a_webp_exif_chunk_is_read"
SELF = "a_directory_pointing_at_itself_is_read_once"
BOUNDS = "a_value_out_of_bounds_or_of_the_wrong_type_is_left_out"
UTF8 = "text_that_is_not_utf8_is_shown_by_its_bytes"
LONG = "a_long_exposure_is_written_in_seconds"
NAMED = "the_camera_is_named_once"

MUTATIONS = [
    (
        "a pointer below IFD0 is followed",
        "pub const MAX_DEPTH: u8 = 1;",
        "pub const MAX_DEPTH: u8 = 2;",
        [SELF],
    ),
    (
        "a JPEG's APP1 is not read",
        "        if marker == 0xE1",
        "        if marker == 0xE2",
        [JPEG],
    ),
    (
        "a PNG's eXIf after the image data is read",
        "            b\"IDAT\" | b\"IEND\" => return None,",
        "            b\"IEND\" => return None,",
        [PNG],
    ),
    (
        "a WebP chunk's padding is not skipped",
        "        at = body_at.checked_add(length)?.checked_add(length & 1)?;",
        "        at = body_at.checked_add(length)?;",
        [WEBP],
    ),
    (
        "a WebP EXIF chunk's prefix is kept",
        "            return Some(body.strip_prefix(b\"Exif\\0\\0\").unwrap_or(body));",
        "            return Some(body);",
        [WEBP],
    ),
    (
        "a big-endian file is read as little-endian",
        "        Some(b\"MM\") => false,",
        "        Some(b\"MM\") => true,",
        [JPEG],
    ),
    (
        "a long value is read from the entry itself",
        "        let at = if length <= 4 {",
        "        let at = if length <= 8 {",
        [JPEG],
    ),
    (
        "a number is taken for text",
        "    if value.kind != Kind::Ascii && value.kind != Kind::Undefined {",
        "    if false {",
        [BOUNDS],
    ),
    (
        "an orientation outside 1 to 8 is kept",
        "                    .filter(|n| (1..=8).contains(n));",
        "                    ;",
        [BOUNDS],
    ),
    (
        "text that is not UTF-8 is decoded lossily",
        "        |_| quoting::escape_unprintable(raw),",
        "        |_| String::from_utf8_lossy(raw).into_owned(),",
        [UTF8],
    ),
    (
        "an exposure is not in lowest terms",
        "        let common = gcd(num, den).max(1);",
        "        let common = 1;",
        [JPEG, LONG],
    ),
    (
        "a long exposure is written as a fraction",
        "    if num < den {\n        let common",
        "    if num != 0 {\n        let common",
        [LONG],
    ),
    (
        "west is east",
        "    exif.gps_longitude = lon.map(|d| if lon_ref == Some(b'W') { -d } else { d });",
        "    exif.gps_longitude = lon;",
        [JPEG],
    ),
    (
        "below sea level is above it",
        "    exif.gps_altitude = alt.map(|a| if below { -a } else { a });",
        "    exif.gps_altitude = alt;",
        [JPEG],
    ),
    (
        "the exposure program is read from ExposureMode",
        "            0x8822 => exif.exposure_program = number().and_then(program),",
        "            0xA402 => exif.exposure_program = number().and_then(program),",
        [JPEG],
    ),
    (
        "the maker is named twice",
        "                let maker = make.split_whitespace().next().unwrap_or(make);",
        "                let maker = make;",
        [NAMED],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "exif", timeout=300, only=only))
