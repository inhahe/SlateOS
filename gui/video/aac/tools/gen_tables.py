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

AACTAB = "libavcodec/aactab.c"

#: Where tools/ffaac/tables.sh writes the tables FFmpeg computes as the
#: decoder opens, from the FFmpeg tree: its default, beside the tree.
GENERATED = "../tables"

#: Each window length's scalefactor band tables, by the sample rates (in kHz)
#: aactab.c names them for.
SWB_OFFSETS = [
    (1024, (96, 64, 48, 32, 24, 16, 8)),
    (960, (96, 64, 48, 24, 16, 8)),
    (512, (48, 32, 24)),
    (480, (48, 32, 24)),
    (128, (96, 48, 24, 16, 8)),
    (120, (96, 48, 24, 16, 8)),
]

#: The spectral codebooks and how many codes each has.
SPECTRAL = [(1, 81), (2, 81), (3, 81), (4, 81), (5, 81), (6, 81), (7, 64), (8, 64),
            (9, 169), (10, 169), (11, 289)]

#: Macros aactab.c names one table by another with.
ALIASES = {"swb_offset_960_32": "swb_offset_960_48"}

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
] + [
    # libavcodec's integer square root (mathtables.c).
    ("libavcodec/mathtables.c", "ff_sqrt_tab", "SQRT_TAB", "u8", [256],
     "the square roots `ff_sqrt` starts from, by a number's top bits, scaled by 16"),
    ("libavcodec/mathtables.c", "ff_inverse", "INVERSE", "u32", [257],
     "2^32 divided by each number, rounded up: what `FASTDIV` multiplies by"),
] + [
    # How many scalefactor bands a window has, and where each starts, by
    # sample rate index (aactab.c). The 120-line short window has the
    # 128-line one's band count (`ff_aac_num_swb_120`, a macro).
    (AACTAB, f"ff_aac_num_swb_{n}", f"NUM_SWB_{n}", "u8", [13],
     f"scalefactor bands in a {n}-line window, by sample rate index")
    for n in (1024, 960, 512, 480, 128)
] + [
    (AACTAB, f"swb_offset_{n}_{rate}", f"SWB_OFFSET_{n}_{rate}", "u16", [None],
     f"where each scalefactor band of a {n}-line window starts at {rate} kHz, then the end")
    for n, rates in SWB_OFFSETS for rate in rates
] + [
    (AACTAB, f"ff_swb_offset_{n}", f"SWB_OFFSET_{n}", "&[u16]", [13],
     f"each sample rate index's {n}-line band offsets (none where it has none)")
    for n, _ in SWB_OFFSETS
] + [
    (AACTAB, "ff_aac_pred_sfb_max", "PRED_SFB_MAX", "u8", [13],
     "the scalefactor bands AAC Main's prediction covers, by sample rate index"),
] + [
    (AACTAB, f"ff_tns_max_bands_{n}", f"TNS_MAX_BANDS_{n}", "u8", [13],
     f"the scalefactor bands TNS may cover in a {n}-line window, by sample rate index")
    for n in (1024, 512, 480, 128)
] + [
    (AACTAB, "ff_ltp_coef", "LTP_COEF_FIXED", "q30", [8],
     "long-term prediction's gains, Q30 as `decode_ltp` takes them"),
] + [
    (AACTAB, f"tns_tmp2_map_{m}", f"TNS_TMP2_MAP_{m}", "q31", [size],
     f"TNS coefficients for coef_compress and coef_res {m.replace('_', ' and ')}, Q31 as "
     "`ff_aac_decode_tns` takes them")
    for m, size in (("0_3", 8), ("0_4", 16), ("1_3", 4), ("1_4", 8))
] + [
    (AACTAB, "ff_tns_tmp2_map", "TNS_TMP2_MAP", "&[i32]", [4],
     "the TNS coefficient tables, by 2 * coef_compress + coef_res"),
    (AACTAB, "ff_aac_scalefactor_code", "SCALEFACTOR_CODE", "u32", [121],
     "the scalefactor codebook's codes, by difference plus 60"),
    (AACTAB, "ff_aac_scalefactor_bits", "SCALEFACTOR_BITS", "u8", [121],
     "the scalefactor codebook's code lengths"),
] + [
    (AACTAB, f"codes{i}", f"CODES{i}", "u16", [size], f"spectral codebook {i}'s codes")
    for i, size in SPECTRAL
] + [
    (AACTAB, f"bits{i}", f"BITS{i}", "u8", [size], f"spectral codebook {i}'s code lengths")
    for i, size in SPECTRAL
] + [
    (AACTAB, "ff_aac_spectral_codes", "SPECTRAL_CODES", "&[u16]", [11],
     "the spectral codebooks' codes"),
    (AACTAB, "ff_aac_spectral_bits", "SPECTRAL_BITS", "&[u8]", [11],
     "the spectral codebooks' code lengths"),
    (AACTAB, "ff_aac_spectral_sizes", "SPECTRAL_SIZES", "u16", [11],
     "how many codes each spectral codebook has"),
] + [
    (AACTAB, f"codebook_vector{v}_idx", f"CODEBOOK_VECTOR{v}_IDX", "u16", [None],
     f"each code's symbol in spectral codebooks {books}: its values, packed with how many "
     "are not zero and where their signs go")
    for v, books in (("02", "1 to 4"), ("4", "5 and 6"), ("6", "7 and 8"), ("8", "9 and 10"),
                     ("10", "11"))
] + [
    (AACTAB, "ff_aac_codebook_vector_idx", "CODEBOOK_VECTOR_IDX", "&[u16]", [11],
     "each spectral codebook's symbols"),
] + [
    # ER AAC ELD's windows, Q30 (aactab.c).
    (AACTAB, f"ff_aac_eld_window_{n}_fixed", f"ELD_WINDOW_{n}", "i32%", [n * 15 // 4],
     f"ER AAC ELD's {n}-line window, Q30")
    for n in (512, 480)
] + [
    # What the decoder computes as it opens, as FFmpeg's build computes it
    # (tools/ffaac/tables.sh).
    (f"{GENERATED}/cbrt_fixed.c", "ff_cbrt_tab_fixed", "CBRT_TAB_FIXED", "u32", [8192],
     "n to the power 4/3 for each n under 8192, Q13: dequantization's table"),
] + [
    (f"{GENERATED}/kbd_fixed.c", f"aac_kbd_{kind}_{n}_fixed", f"KBD_{kind.upper()}_{n}", "i32",
     [n], f"the {n}-line Kaiser-Bessel-derived window's rising half, Q31")
    for kind, n in (("long", 1024), ("short", 128), ("long", 960), ("short", 120))
] + [
    (f"{GENERATED}/sinewin_fixed.c", f"SINETABLE({n})", f"SINE_{n}", "i32%", [n],
     f"the {n}-line sine window's rising half, Q31")
    for n in (1024, 128, 960, 120, 512)
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
    "#[cfg(test)]",
    "mod tests;",
    "",
]


def main() -> int:
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    sys.stdout.buffer.write(ctables.generate(root, TABLES, {}, HEADER, source="FFmpeg",
                                             item="pub(crate) static", aliases=ALIASES))
    return 0


if __name__ == "__main__":
    sys.exit(main())
