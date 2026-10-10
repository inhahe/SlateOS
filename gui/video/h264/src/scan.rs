//! The orders a block's coefficients are coded in (`init_scan_tables` in
//! `h264_slice.c`, with its field and CAVLC 8x8 tables).
//!
//! Frame macroblocks code their coefficients in zigzag order, field ones
//! in the field scan, which runs down a column sooner; CAVLC codes an 8x8
//! block as four interleaved 4x4 runs. FFmpeg's transforms take their
//! blocks transposed, so each scan is stored transposed; at QP 0 a stream
//! that bypasses the transform (lossless) reads the scans untransposed.
//!
//! The twelve tables live end to end in one array, in the order of
//! `H264Context`'s fields. That is not cosmetic: a damaged stream can run a
//! 4x4 block's coefficients one place past its scan, and FFmpeg then reads
//! the first entry of the table after it -- which is what is read here.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "offsets of tables within the array that holds them"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "const initialisers bounded by the tables' own lengths, and offsets of tables \
              within the array that holds them"
)]

use crate::tables::{ZIGZAG_DIRECT, ZIGZAG_SCAN};

/// A field's 4x4 scan, raster positions (`field_scan`).
const FIELD_SCAN: [u8; 16] = [
    0, 4, 1, 8, //
    12, 5, 9, 13, //
    2, 6, 10, 14, //
    3, 7, 11, 15,
];

/// A field's 8x8 scan (`field_scan8x8`).
#[rustfmt::skip]
const FIELD_SCAN8X8: [u8; 64] = [
     0,  8, 16,  1,  9, 24, 32, 17,
     2, 25, 40, 48, 56, 33, 10,  3,
    18, 41, 49, 57, 26, 11,  4, 19,
    34, 42, 50, 58, 27, 12,  5, 20,
    35, 43, 51, 59, 28, 13,  6, 21,
    36, 44, 52, 60, 29, 14, 22, 37,
    45, 53, 61, 30,  7, 15, 38, 46,
    54, 62, 23, 31, 39, 47, 55, 63,
];

/// A field's 8x8 scan as CAVLC codes it, four 4x4 runs interleaved
/// (`field_scan8x8_cavlc`).
#[rustfmt::skip]
const FIELD_SCAN8X8_CAVLC: [u8; 64] = [
     0,  9,  2, 56, 18, 26, 34, 27,
    35, 28, 36, 29, 45,  7, 54, 39,
     8, 24, 25, 33, 41, 11, 42, 12,
    43, 13, 44, 14, 53, 15, 62, 47,
    16, 32, 40, 10, 49,  4, 50,  5,
    51,  6, 52, 22, 61, 38, 23, 55,
     1, 17, 48,  3, 57, 19, 58, 20,
    59, 21, 60, 37, 30, 46, 31, 63,
];

/// A frame's 8x8 zigzag as CAVLC codes it (`zigzag_scan8x8_cavlc`).
#[rustfmt::skip]
const ZIGZAG_SCAN8X8_CAVLC: [u8; 64] = [
     0,  9, 17, 18, 12, 40, 27,  7,
    35, 57, 29, 30, 58, 38, 53, 47,
     1,  2, 24, 11, 19, 48, 20, 14,
    42, 50, 22, 37, 59, 31, 60, 55,
     8,  3, 32,  4, 26, 41, 13, 21,
    49, 43, 15, 44, 52, 39, 61, 62,
    16, 10, 25,  5, 33, 34,  6, 28,
    56, 36, 23, 51, 45, 46, 54, 63,
];

/// Where each table starts in [`ScanTables::all`], in `H264Context`'s
/// order.
pub(crate) const ZIGZAG: usize = 0;
pub(crate) const ZIGZAG8X8: usize = ZIGZAG + 16;
pub(crate) const ZIGZAG8X8_CAVLC: usize = ZIGZAG8X8 + 64;
pub(crate) const FIELD: usize = ZIGZAG8X8_CAVLC + 64;
pub(crate) const FIELD8X8: usize = FIELD + 16;
pub(crate) const FIELD8X8_CAVLC: usize = FIELD8X8 + 64;
/// The same six at QP 0.
pub(crate) const Q0: usize = FIELD8X8_CAVLC + 64;
const ALL: usize = 2 * Q0;

/// The scans a stream's slices read (`zigzag_scan` to
/// `field_scan8x8_cavlc_q0`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScanTables {
    pub(crate) all: [u8; ALL],
}

/// A 4x4 position transposed.
const fn transpose4(x: u8) -> u8 {
    (x >> 2) | ((x << 2) & 0xF)
}

/// An 8x8 position transposed.
const fn transpose8(x: u8) -> u8 {
    (x >> 3) | ((x & 7) << 3)
}

impl ScanTables {
    /// The scans for a stream, `transform_bypass` where its sequence
    /// parameter set lets QP 0 skip the transform (`init_scan_tables`).
    pub(crate) fn new(transform_bypass: bool) -> Self {
        let mut all = [0u8; ALL];
        for i in 0..16 {
            all[ZIGZAG + i] = transpose4(ZIGZAG_SCAN[i]);
            all[FIELD + i] = transpose4(FIELD_SCAN[i]);
        }
        for i in 0..64 {
            all[ZIGZAG8X8 + i] = transpose8(ZIGZAG_DIRECT[i]);
            all[ZIGZAG8X8_CAVLC + i] = transpose8(ZIGZAG_SCAN8X8_CAVLC[i]);
            all[FIELD8X8 + i] = transpose8(FIELD_SCAN8X8[i]);
            all[FIELD8X8_CAVLC + i] = transpose8(FIELD_SCAN8X8_CAVLC[i]);
        }
        if transform_bypass {
            all[Q0 + ZIGZAG..Q0 + ZIGZAG + 16].copy_from_slice(&ZIGZAG_SCAN);
            all[Q0 + ZIGZAG8X8..Q0 + ZIGZAG8X8 + 64].copy_from_slice(&ZIGZAG_DIRECT);
            all[Q0 + ZIGZAG8X8_CAVLC..Q0 + ZIGZAG8X8_CAVLC + 64]
                .copy_from_slice(&ZIGZAG_SCAN8X8_CAVLC);
            all[Q0 + FIELD..Q0 + FIELD + 16].copy_from_slice(&FIELD_SCAN);
            all[Q0 + FIELD8X8..Q0 + FIELD8X8 + 64].copy_from_slice(&FIELD_SCAN8X8);
            all[Q0 + FIELD8X8_CAVLC..Q0 + FIELD8X8_CAVLC + 64]
                .copy_from_slice(&FIELD_SCAN8X8_CAVLC);
        } else {
            all.copy_within(..Q0, Q0);
        }
        Self { all }
    }

    /// The scans a macroblock reads: its 4x4 scan and its CAVLC 8x8 one,
    /// by whether it is a field macroblock and whether its QP is 0.
    pub(crate) fn cavlc(field: bool, qp0: bool) -> (usize, usize) {
        let base = if qp0 { Q0 } else { 0 };
        if field {
            (base + FIELD, base + FIELD8X8_CAVLC)
        } else {
            (base + ZIGZAG, base + ZIGZAG8X8_CAVLC)
        }
    }
}

impl Default for ScanTables {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests;
