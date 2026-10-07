"""Write minimp3.h with this crate's two deliberate differences applied, so
that `tools/reference.c` built against it is exactly what the crate is held
to -- everything else being minimp3's own code, untouched.

    python patch_minimp3.py <minimp3.h> <out.h>

The two differences (the crate's docs give the reasons):

1. The intensity-stereo bit of the mode extension counts only in joint
   stereo, as the standard says and mpg123 and FFmpeg's stereo processing
   have it; minimp3 tests the bit in any mode (`HDR_TEST_I_STEREO`).
2. A Layer III frame's private bits are dropped before the scale factor
   selection information is spread over the granules; minimp3 leaves them
   in the first granule's.
"""
import sys

src = open(sys.argv[1], encoding="utf-8").read()
edits = [
    (
        "#define HDR_TEST_I_STEREO(h)        ((h[3]) & 0x10)",
        "#define HDR_TEST_I_STEREO(h)        (((h[3]) & 0xD0) == 0x50)",
    ),
    (
        "        scfsi = get_bits(bs, 7 + gr_count);",
        "        scfsi = get_bits(bs, 7 + gr_count) & ((1u << (2*gr_count)) - 1);",
    ),
]
for old, new in edits:
    if src.count(old) != 1:
        sys.exit(f"expected exactly one {old!r}")
    src = src.replace(old, new)
with open(sys.argv[2], "w", encoding="utf-8", newline="\n") as f:
    f.write(src)
