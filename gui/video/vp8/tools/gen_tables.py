"""Write gui/video/vp8/src/tables.rs from libvpx's own source.

The VP8 decoder needs a few thousand numbers that are libvpx's and nobody
else's -- default probabilities, the trees modes are coded with, quantiser
steps, filter kernels. Typing them would be the one part of the port nobody
could check, so this reads them out of a libvpx checkout and writes them as
Rust:

    git clone --depth 1 --branch v1.17.0 https://github.com/webmproject/libvpx
    python tools/gen_tables.py path/to/libvpx > src/tables.rs

How each array is read -- by its C name, filled by C's own initializer rules,
and checked against the shape declared here -- is `gui/video/tools/ctables.py`,
which gui/video/vp9's generator shares.
"""

import os
import sys

# The C-initializer reader both codecs' generators share.
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "tools"))
import ctables  # noqa: E402
from ctables import STRUCT  # noqa: E402

#: C names the initializers use, by value: the macroblock modes and the 4x4
#: block modes, which the mode trees' leaves name.
SYMBOLS = {
    **{name: i for i, name in enumerate([
        "DC_PRED", "V_PRED", "H_PRED", "TM_PRED", "B_PRED",
        "NEARESTMV", "NEARMV", "ZEROMV", "NEWMV", "SPLITMV"])},
    **{name: i for i, name in enumerate([
        "B_DC_PRED", "B_TM_PRED", "B_VE_PRED", "B_HE_PRED", "B_LD_PRED",
        "B_RD_PRED", "B_VR_PRED", "B_VL_PRED", "B_HD_PRED", "B_HU_PRED"])},
}

#: (C file, C name, Rust name, element type, shape, what it is).
TABLES = [
    ("vp8/common/entropy.c", "vp8_norm", "NORM", "u8", [256],
     "how far the bool decoder shifts a range to bring its top bit to bit 7"),

    ("vp8/decoder/detokenize.c", "kBands", "BANDS", "u8", [17],
     "the coefficient band of each position in zigzag order, and a sentinel"),
    ("vp8/decoder/detokenize.c", "kZigzag", "ZIGZAG", "u8", [16],
     "the raster position of each coefficient in coding order"),
    ("vp8/decoder/detokenize.c", "kCat3", "CAT3", "u8", [4],
     "the probabilities of category 3's extra bits, ending in 0"),
    ("vp8/decoder/detokenize.c", "kCat4", "CAT4", "u8", [5],
     "the probabilities of category 4's extra bits, ending in 0"),
    ("vp8/decoder/detokenize.c", "kCat5", "CAT5", "u8", [6],
     "the probabilities of category 5's extra bits, ending in 0"),
    ("vp8/decoder/detokenize.c", "kCat6", "CAT6", "u8", [12],
     "the probabilities of category 6's extra bits, ending in 0"),
    ("vp8/common/default_coef_probs.h", "default_coef_probs", "DEFAULT_COEF_PROBS", "u8",
     [4, 8, 3, 11],
     "the coefficient probabilities a key frame starts from, by block type, band, context and"
     " tree node"),
    ("vp8/common/coefupdateprobs.h", "vp8_coef_update_probs", "COEF_UPDATE_PROBS", "u8",
     [4, 8, 3, 11], "the probabilities that a frame header updates each coefficient probability"),

    ("vp8/common/vp8_entropymodedata.h", "vp8_ymode_prob", "YMODE_PROB", "u8", [4],
     "the luma mode probabilities an inter frame starts from"),
    ("vp8/common/vp8_entropymodedata.h", "vp8_kf_ymode_prob", "KF_YMODE_PROB", "u8", [4],
     "key-frame luma mode probabilities"),
    ("vp8/common/vp8_entropymodedata.h", "vp8_uv_mode_prob", "UV_MODE_PROB", "u8", [3],
     "the chroma mode probabilities an inter frame starts from"),
    ("vp8/common/vp8_entropymodedata.h", "vp8_kf_uv_mode_prob", "KF_UV_MODE_PROB", "u8", [3],
     "key-frame chroma mode probabilities"),
    ("vp8/common/vp8_entropymodedata.h", "vp8_bmode_prob", "BMODE_PROB", "u8", [9],
     "inter-frame 4x4 block mode probabilities"),
    ("vp8/common/vp8_entropymodedata.h", "vp8_kf_bmode_prob", "KF_BMODE_PROB", "u8",
     [10, 10, 9], "key-frame 4x4 block mode probabilities, by the above and left blocks' modes"),

    ("vp8/common/entropymode.c", "vp8_bmode_tree", "BMODE_TREE", "i8", [18],
     "the tree 4x4 block modes are coded with"),
    ("vp8/common/entropymode.c", "vp8_ymode_tree", "YMODE_TREE", "i8", [8],
     "the tree inter frames' luma modes are coded with"),
    ("vp8/common/entropymode.c", "vp8_kf_ymode_tree", "KF_YMODE_TREE", "i8", [8],
     "the tree key frames' luma modes are coded with"),
    ("vp8/common/entropymode.c", "vp8_uv_mode_tree", "UV_MODE_TREE", "i8", [6],
     "the tree chroma modes are coded with"),
    ("vp8/common/entropymode.c", "vp8_small_mvtree", "SMALL_MV_TREE", "i8", [14],
     "the tree a motion vector component below 8 is coded with"),

    ("vp8/common/entropymv.c", "vp8_mv_update_probs", "MV_UPDATE_PROBS", "u8", [2, STRUCT, 19],
     "the probabilities that a frame header updates each motion vector probability, rows then"
     " columns"),
    ("vp8/common/entropymv.c", "vp8_default_mv_context", "DEFAULT_MV_CONTEXT", "u8",
     [2, STRUCT, 19], "the motion vector probabilities a key frame starts from, rows then columns"),
    ("vp8/common/modecont.c", "vp8_mode_contexts", "MODE_CONTEXTS", "u8", [6, 4],
     "inter mode probabilities, by how many neighbours voted for each candidate"),
    ("vp8/common/findnearmv.c", "vp8_mbsplit_offset", "MBSPLIT_OFFSET", "u8", [4, 16],
     "the first 4x4 block of each part of each way to split a macroblock"),
    ("vp8/decoder/decodemv.c", "vp8_sub_mv_ref_prob3", "SUB_MV_REF_PROB3", "u8", [8, 3],
     "split motion vector mode probabilities, by whether the left and above vectors are zero"
     " and equal"),
    ("vp8/decoder/decodemv.c", "mbsplit_fill_count", "MBSPLIT_FILL_COUNT", "u8", [4],
     "how many 4x4 blocks each part of each way to split a macroblock covers"),
    ("vp8/decoder/decodemv.c", "mbsplit_fill_offset", "MBSPLIT_FILL_OFFSET", "u8", [4, 16],
     "the 4x4 blocks of each part of each way to split a macroblock, part by part"),

    ("vp8/common/quant_common.c", "dc_qlookup", "DC_QLOOKUP", "u16", [128],
     "the DC quantiser step for each index"),
    ("vp8/common/quant_common.c", "ac_qlookup", "AC_QLOOKUP", "u16", [128],
     "the AC quantiser step for each index"),

    ("vp8/common/filter.c", "vp8_bilinear_filters", "BILINEAR_FILTERS", "i16", [8, 2],
     "the bilinear interpolation kernels, by eighth-pixel position"),
    ("vp8/common/filter.c", "vp8_sub_pel_filters", "SUB_PEL_FILTERS", "i16", [8, 6],
     "the six-tap interpolation kernels, by eighth-pixel position"),
]

HEADER = [
    "//! libvpx's constant tables, written by `tools/gen_tables.py` from libvpx",
    "//! v1.17.0's source (copyright the WebM project authors): do not edit;",
    "//! regenerate. Used under libvpx's BSD licence and patent grant",
    "//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).",
    "//!",
    "//! Each table is read by its C name, filled by C's initializer rules and",
    "//! checked against the shape declared here, so a table that does not fit",
    "//! fails the generator instead of being cut short.",
    "",
]


def main() -> int:
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    sys.stdout.buffer.write(ctables.generate(root, TABLES, SYMBOLS, HEADER))
    return 0


if __name__ == "__main__":
    sys.exit(main())
