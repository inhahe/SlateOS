//! FFmpeg's tables for H.264, as they are in its source: `h264data.c`,
//! `mathtables.c`'s scans, `h2645data.c`'s pixel shapes and the defaults
//! `h264_ps.c` keeps for itself.

#![allow(
    clippy::indexing_slicing,
    reason = "only in const initialisers, by loops bounded by the arrays' own lengths; an \
              index out of bounds there fails the build, not a decode"
)]

/// The highest QP at any bit depth: 51, plus 6 for each bit over 8 up to 14
/// (`QP_MAX_NUM`).
pub(crate) const QP_MAX_NUM: usize = 51 + 6 * 6;

/// A 4x4 block's coefficients in zigzag order, as raster positions
/// (`ff_zigzag_scan`; FFmpeg's array has a seventeenth entry, zero, that
/// nothing reads).
pub(crate) const ZIGZAG_SCAN: [u8; 16] = [
    0, 1, 4, 8, //
    5, 2, 3, 6, //
    9, 12, 13, 10, //
    7, 11, 14, 15,
];

/// An 8x8 block's coefficients in zigzag order, as raster positions
/// (`ff_zigzag_direct`).
#[rustfmt::skip]
pub(crate) const ZIGZAG_DIRECT: [u8; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// The default 4x4 scaling lists, intra then inter, in raster order as
/// `h264_ps.c` keeps them (`default_scaling4`).
#[rustfmt::skip]
pub(crate) const DEFAULT_SCALING4: [[u8; 16]; 2] = [
    [ 6, 13, 20, 28, 13, 20, 28, 32,
     20, 28, 32, 37, 28, 32, 37, 42],
    [10, 14, 20, 24, 14, 20, 24, 27,
     20, 24, 27, 30, 24, 27, 30, 34],
];

/// The default 8x8 scaling lists, intra then inter (`default_scaling8`).
#[rustfmt::skip]
pub(crate) const DEFAULT_SCALING8: [[u8; 64]; 2] = [
    [ 6, 10, 13, 16, 18, 23, 25, 27,
     10, 11, 16, 18, 23, 25, 27, 29,
     13, 16, 18, 23, 25, 27, 29, 31,
     16, 18, 23, 25, 27, 29, 31, 33,
     18, 23, 25, 27, 29, 31, 33, 36,
     23, 25, 27, 29, 31, 33, 36, 38,
     25, 27, 29, 31, 33, 36, 38, 40,
     27, 29, 31, 33, 36, 38, 40, 42],
    [ 9, 13, 15, 17, 19, 21, 22, 24,
     13, 13, 17, 19, 21, 22, 24, 25,
     15, 17, 19, 21, 22, 24, 25, 27,
     17, 19, 21, 22, 24, 25, 27, 28,
     19, 21, 22, 24, 25, 27, 28, 30,
     21, 22, 24, 25, 27, 28, 30, 32,
     22, 24, 25, 27, 28, 30, 32, 33,
     24, 25, 27, 28, 30, 32, 33, 35],
];

/// The most macroblocks a level's decoded picture buffer holds, by
/// `level_idc` (`level_max_dpb_mbs`): what the reorder depth is worked out
/// from when a stream does not say it.
pub(crate) const LEVEL_MAX_DPB_MBS: [(i32, i32); 16] = [
    (10, 396),
    (11, 900),
    (12, 2376),
    (13, 2376),
    (20, 2376),
    (21, 4752),
    (22, 8100),
    (30, 8100),
    (31, 18000),
    (32, 20480),
    (40, 32768),
    (41, 32768),
    (42, 34816),
    (50, 110_400),
    (51, 184_320),
    (52, 184_320),
];

/// A 4x4 block's dequantization factors at QP % 6, by the three kinds of
/// position (`ff_h264_dequant4_coeff_init`).
pub(crate) const DEQUANT4_COEFF_INIT: [[u8; 3]; 6] = [
    [10, 13, 16],
    [11, 14, 18],
    [13, 16, 20],
    [14, 18, 23],
    [16, 20, 25],
    [18, 23, 29],
];

/// Which of the six kinds of position each of an 8x8 block's 4x4 phases is
/// (`ff_h264_dequant8_coeff_init_scan`).
pub(crate) const DEQUANT8_COEFF_INIT_SCAN: [u8; 16] =
    [0, 3, 4, 3, 3, 1, 5, 1, 4, 5, 2, 5, 3, 1, 5, 1];

/// An 8x8 block's dequantization factors at QP % 6, by the six kinds of
/// position (`ff_h264_dequant8_coeff_init`).
pub(crate) const DEQUANT8_COEFF_INIT: [[u8; 6]; 6] = [
    [20, 18, 32, 19, 25, 24],
    [22, 19, 35, 21, 28, 26],
    [26, 23, 42, 24, 33, 31],
    [28, 25, 45, 26, 35, 33],
    [32, 28, 51, 30, 40, 38],
    [36, 32, 58, 34, 46, 43],
];

/// QP % 6 (`ff_h264_quant_rem6`).
pub(crate) const QUANT_REM6: [u8; QP_MAX_NUM + 1] = {
    let mut t = [0u8; QP_MAX_NUM + 1];
    let mut q = 0;
    while q <= QP_MAX_NUM {
        #[allow(clippy::cast_possible_truncation, reason = "q % 6 is under 6")]
        let r = (q % 6) as u8;
        t[q] = r;
        q += 1;
    }
    t
};

/// QP / 6 (`ff_h264_quant_div6`).
pub(crate) const QUANT_DIV6: [u8; QP_MAX_NUM + 1] = {
    let mut t = [0u8; QP_MAX_NUM + 1];
    let mut q = 0;
    while q <= QP_MAX_NUM {
        #[allow(clippy::cast_possible_truncation, reason = "q / 6 is under 15")]
        let d = (q / 6) as u8;
        t[q] = d;
        q += 1;
    }
    t
};

/// The tail every row of [`CHROMA_QP`] ends with, before the depth's
/// offset: the chroma QP for each luma QP from 0 to 51
/// (`CHROMA_QP_TABLE_END`).
const CHROMA_QP_END: [u8; 52] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, //
    12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, //
    24, 25, 26, 27, 28, 29, 29, 30, 31, 32, 32, 33, //
    34, 34, 35, 35, 36, 36, 37, 37, 37, 38, 38, 38, //
    39, 39, 39, 39,
];

/// The chroma QP for a luma QP (plus the chroma offset), by bit depth from
/// 8 to 14 (`ff_h264_chroma_qp`): the row for depth `d` counts up through
/// the `6 * (d - 8)` QPs the extra bits add, then follows
/// [`CHROMA_QP_END`] raised by as many; past that, zeros, as C fills a
/// shorter initializer, which no index reaches.
pub(crate) const CHROMA_QP: [[u8; QP_MAX_NUM + 1]; 7] = {
    let mut t = [[0u8; QP_MAX_NUM + 1]; 7];
    let mut d = 0;
    while d < 7 {
        let offset = 6 * d;
        let mut i = 0;
        while i < offset {
            #[allow(clippy::cast_possible_truncation, reason = "under 36")]
            let v = i as u8;
            t[d][i] = v;
            i += 1;
        }
        let mut k = 0;
        while k < 52 {
            #[allow(clippy::cast_possible_truncation, reason = "at most 39 + 36")]
            let v = CHROMA_QP_END[k] + offset as u8;
            t[d][offset + k] = v;
            k += 1;
        }
        d += 1;
    }
    t
};

/// The pixel shapes `aspect_ratio_idc` 0 to 16 name, as numerator and
/// denominator (`ff_h2645_pixel_aspect`).
pub(crate) const PIXEL_ASPECT: [(i32, i32); 17] = [
    (0, 1),
    (1, 1),
    (12, 11),
    (10, 11),
    (16, 11),
    (40, 33),
    (24, 11),
    (20, 11),
    (32, 11),
    (80, 33),
    (18, 11),
    (15, 11),
    (64, 33),
    (160, 99),
    (4, 3),
    (3, 2),
    (2, 1),
];

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    fn is_permutation(t: &[u8]) -> bool {
        let mut seen = vec![false; t.len()];
        for &v in t {
            match seen.get_mut(usize::from(v)) {
                Some(s) if !*s => *s = true,
                _ => return false,
            }
        }
        true
    }

    #[test]
    fn the_scans_visit_every_position_once_along_the_diagonals() {
        assert!(is_permutation(&ZIGZAG_SCAN));
        assert!(is_permutation(&ZIGZAG_DIRECT));
        // A zigzag never moves further than one diagonal at a time.
        for (scan, width) in [(&ZIGZAG_SCAN[..], 4), (&ZIGZAG_DIRECT[..], 8)] {
            for pair in scan.windows(2) {
                let diagonal = |p: u8| p % width + p / width;
                assert!(diagonal(pair[1]).abs_diff(diagonal(pair[0])) <= 1);
            }
        }
    }

    /// The rows are FFmpeg's initializers: 8-bit's begins 0, 1, 2 and
    /// flattens through 29, 29 at 29..31, ends at 39; each deeper row is the
    /// same raised by six a bit, after counting up to six a bit.
    #[test]
    fn the_chroma_qp_rows_are_ffmpegs() {
        assert_eq!(&CHROMA_QP[0][..6], [0, 1, 2, 3, 4, 5]);
        assert_eq!(&CHROMA_QP[0][28..32], [28, 29, 29, 30]);
        assert_eq!(CHROMA_QP[0][51], 39);
        assert_eq!(CHROMA_QP[0][52], 0, "past the row: C's zero fill");
        for d in 1..7 {
            let offset = 6 * d;
            for i in 0..offset {
                assert_eq!(usize::from(CHROMA_QP[d][i]), i);
            }
            for q in 0..52 {
                assert_eq!(
                    usize::from(CHROMA_QP[d][offset + q]),
                    usize::from(CHROMA_QP[0][q]) + offset
                );
            }
        }
        assert_eq!(CHROMA_QP[6][QP_MAX_NUM], 39 + 36);
    }

    #[test]
    fn quant_div_and_rem() {
        for q in 0..=QP_MAX_NUM {
            assert_eq!(
                usize::from(QUANT_DIV6[q]) * 6 + usize::from(QUANT_REM6[q]),
                q
            );
        }
        assert_eq!(QUANT_DIV6[QP_MAX_NUM], 14);
        assert_eq!(QUANT_REM6[QP_MAX_NUM], 3);
    }
}
