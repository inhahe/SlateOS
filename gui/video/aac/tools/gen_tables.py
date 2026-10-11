"""Write gui/video/aac/src/tables.rs from FFmpeg's own source.

The AAC decoder needs thousands of numbers that are FFmpeg's: codebooks,
scalefactor band offsets, window shapes, the soft-float library's
approximation tables. Typing them would be the one part of the port nobody
could check, so this reads the static ones out of an FFmpeg 8.1.3 checkout
-- the one `tools/ffaac/build.sh` makes -- and writes them as Rust:

    python gui/video/aac/tools/gen_tables.py //wsl$/Ubuntu/home/<user>/ffaac/src \
        > gui/video/aac/src/tables.rs

How each array is read -- by its C name, filled by C's own initializer rules,
and checked against the shape declared here -- is `gui/video/tools/ctables.py`,
which the VP8 and VP9 generators share.
"""

import os
import sys

# The C-initializer reader the video codecs' generators share.
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "tools"))
import ctables  # noqa: E402

#: (C file, C name, Rust name, element type, shape, what it is).
TABLES = [
    # libavutil's soft-float (softfloat_tables.h).
    ("libavutil/softfloat_tables.h", "av_sqrttbl_sf", "SQRTTBL_SF", "i32", [513],
     "the square root of x for x from 0.5 to 1, in 512 steps, Q30"),
    ("libavutil/softfloat_tables.h", "av_sqr_exp_multbl_sf", "SQR_EXP_MULTBL_SF", "i32", [2],
     "1 and sqrt(2), Q29: the square root's factor for an even and an odd exponent"),
    ("libavutil/softfloat_tables.h", "av_costbl_1_sf", "COSTBL_1_SF", "i32%", [16],
     "cos(2 pi i / 32), Q30"),
    ("libavutil/softfloat_tables.h", "av_costbl_2_sf", "COSTBL_2_SF", "i32%", [32],
     "the cosines of the second step of a soft-float angle, Q30"),
    ("libavutil/softfloat_tables.h", "av_sintbl_2_sf", "SINTBL_2_SF", "i32%", [32],
     "the sines of the second step of a soft-float angle, Q30"),
    ("libavutil/softfloat_tables.h", "av_costbl_3_sf", "COSTBL_3_SF", "i32%", [32],
     "the cosines of the third step of a soft-float angle, Q30"),
    ("libavutil/softfloat_tables.h", "av_sintbl_3_sf", "SINTBL_3_SF", "i32%", [32],
     "the sines of the third step of a soft-float angle, Q30"),
    ("libavutil/softfloat_tables.h", "av_costbl_4_sf", "COSTBL_4_SF", "i32%", [33],
     "the cosines the last step of a soft-float angle interpolates between, Q30"),
    ("libavutil/softfloat_tables.h", "av_sintbl_4_sf", "SINTBL_4_SF", "i32%", [33],
     "the sines the last step of a soft-float angle interpolates between, Q30"),
]

HEADER = [
    "//! FFmpeg's constant tables, written by `tools/gen_tables.py` from FFmpeg",
    "//! 8.1.3's source: do not edit; regenerate. Used under FFmpeg's LGPL",
    "//! (`licenses/FFmpeg-COPYING.LGPLv2.1`).",
    "//!",
    "//! Each table is read by its C name, filled by C's initializer rules and",
    "//! checked against the shape declared here, so a table that does not fit",
    "//! fails the generator instead of being cut short.",
    "",
]


def main() -> int:
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    sys.stdout.buffer.write(ctables.generate(root, TABLES, {}, HEADER, source="FFmpeg",
                                             item="pub(crate) static"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
