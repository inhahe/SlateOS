//! ThunderScan's 4-bit compression: libtiff 4.7.1's `tif_thunder.c`.
//!
//! Each byte is a two-bit code and six bits of data: a run of the last
//! pixel, three pixels as two-bit deltas from it, two as three-bit deltas, or
//! one raw pixel. Transcribed with libtiff's behaviour kept whole: each row
//! carries on through the data from where the last stopped; a run that
//! starts half-way through a byte finishes it and then repeats it whole; a
//! row the data does not finish, or a run overfills, fails the strip, the
//! rest of the row zeroed first. There is no tile decoder, as libtiff has
//! none.

use crate::{ImageError, ImageResult};

/// `twobitdeltas`, indexed by the two-bit field; 2 skips the pixel.
const TWO_BIT: [i32; 4] = [0, 1, 0, -1];
/// `threebitdeltas`, indexed by the three-bit field; 4 skips the pixel.
const THREE_BIT: [i32; 8] = [0, 1, 2, 3, 0, -3, -2, -1];

/// `ThunderSetupDecode`: only 4-bit samples.
pub(super) fn setup(bits_per_sample: u16) -> bool {
    bits_per_sample == 4
}

/// `ThunderDecodeRow`: `data` into `out`, a row of `width` pixels every
/// `scanline` bytes, each row carrying on through the data from where the
/// last stopped.
pub(super) fn decode(
    data: &[u8],
    out: &mut [u8],
    scanline: usize,
    width: usize,
) -> ImageResult<()> {
    if scanline == 0 || !out.len().is_multiple_of(scanline) {
        return Err(ImageError::Malformed(
            "TIFF ThunderScan strip not whole rows",
        ));
    }
    let mut at = 0usize;
    for row in out.chunks_exact_mut(scanline) {
        row_decode(data, &mut at, row, width)?;
    }
    Ok(())
}

/// The output side of one row: where the next byte goes, and how many
/// pixels are in.
struct Row<'r> {
    out: &'r mut [u8],
    op: usize,
    npixels: usize,
    max: usize,
    last: u32,
}

impl Row<'_> {
    /// `SETPIXEL`: two pixels a byte, the first in the top half.
    fn set(&mut self, v: u32) {
        self.last = v & 0xF;
        if self.npixels < self.max {
            let even = self.npixels & 1 == 0;
            self.npixels = self.npixels.saturating_add(1);
            if let Some(byte) = self.out.get_mut(self.op) {
                if even {
                    *byte = (self.last << 4) as u8;
                } else {
                    *byte |= self.last as u8;
                    self.op = self.op.saturating_add(1);
                }
            }
        }
    }

    /// `lastpixel + delta`, wrapped as the C's unsigned arithmetic wraps it
    /// before the nibble is taken.
    fn delta(&mut self, delta: i32) {
        self.set(self.last.wrapping_add_signed(delta));
    }
}

/// `ThunderDecode`: one row of `width` pixels.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a byte assembled from nibbles"
)]
fn row_decode(data: &[u8], at: &mut usize, out: &mut [u8], width: usize) -> ImageResult<()> {
    let mut row = Row {
        out,
        op: 0,
        npixels: 0,
        max: width,
        last: 0,
    };
    while row.npixels < row.max {
        let Some(&n) = data.get(*at) else {
            break;
        };
        *at = at.saturating_add(1);
        match n & 0xC0 {
            0x00 => run(&mut row, i64::from(n)),
            0x40 => {
                for shift in [4, 2, 0] {
                    let delta = usize::from((n >> shift) & 3);
                    if delta != 2 {
                        row.delta(TWO_BIT.get(delta).copied().unwrap_or(0));
                    }
                }
            }
            0x80 => {
                for shift in [3, 0] {
                    let delta = usize::from((n >> shift) & 7);
                    if delta != 4 {
                        row.delta(THREE_BIT.get(delta).copied().unwrap_or(0));
                    }
                }
            }
            _ => row.set(u32::from(n)),
        }
    }
    if row.npixels != row.max {
        // The rest of the row zeroed, then the strip fails.
        let end = row.max.div_ceil(2).min(row.out.len());
        if let Some(rest) = row.out.get_mut(row.op..end) {
            rest.fill(0);
        }
        return Err(ImageError::Malformed(
            "TIFF ThunderScan row does not fit its data",
        ));
    }
    Ok(())
}

/// `THUNDER_RUN`: the last pixel `n` more times.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "counts of at most 63 pixels, and a byte of two nibbles"
)]
fn run(row: &mut Row<'_>, mut n: i64) {
    if n == 0 {
        return;
    }
    if row.npixels & 1 == 1 {
        // Finish the half-done byte; the run then repeats that whole byte.
        if let Some(byte) = row.out.get_mut(row.op) {
            *byte |= row.last as u8;
            row.last = u32::from(*byte);
        }
        row.op = row.op.saturating_add(1);
        row.npixels = row.npixels.saturating_add(1);
        n = n.wrapping_sub(1);
    } else {
        row.last |= row.last << 4;
    }
    row.npixels = row.npixels.saturating_add(n as usize);
    if row.npixels > row.max {
        return;
    }
    while n > 0 {
        if let Some(byte) = row.out.get_mut(row.op) {
            *byte = row.last as u8;
        }
        row.op = row.op.saturating_add(1);
        // At most 63 down by twos: it ends at 0 or -1.
        n = n.wrapping_sub(2);
    }
    if n == -1 {
        // An odd run: its last byte's second pixel is not the run's.
        row.op = row.op.saturating_sub(1);
        if let Some(byte) = row.out.get_mut(row.op) {
            *byte &= 0xF0;
        }
    }
    row.last &= 0xF;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn raw_pixels_and_deltas() {
        let mut out = [0u8; 2];
        // Raw 5; 2-bit deltas +1, skip, -1 (6, 5); 3-bit deltas +3 (8).
        decode(&[0xC5, 0x40 | 0b01_10_11, 0x80 | 0b011_100], &mut out, 2, 4).unwrap();
        assert_eq!(out, [0x56, 0x58]);
    }

    #[test]
    fn a_run_finishes_a_half_done_byte_then_repeats_it() {
        // One pixel of 7, then a run of three more.
        let mut out = [0u8; 2];
        decode(&[0xC7, 0x03], &mut out, 2, 4).unwrap();
        assert_eq!(out, [0x77, 0x77]);
    }

    #[test]
    fn an_odd_run_ends_half_way_through_its_last_byte() {
        // Pixels 3 and 9, then three more 9s: the last byte's second half
        // is cleared, not left as the run.
        let mut out = [0u8; 3];
        decode(&[0xC3, 0xC9, 0x03], &mut out, 3, 5).unwrap();
        assert_eq!(out, [0x39, 0x99, 0x90]);
    }

    #[test]
    fn a_row_short_of_data_is_zeroed_and_fails() {
        let mut out = [0xAAu8; 3];
        assert!(decode(&[0xC5, 0xC6, 0xC7], &mut out, 3, 6).is_err());
        // Zeroed from the byte being filled: its first pixel goes too.
        assert_eq!(out, [0x56, 0x00, 0x00]);
        // Overfilled by a run.
        let mut out = [0u8; 1];
        assert!(decode(&[0xC5, 0x05], &mut out, 1, 2).is_err());
        assert!(setup(4));
        assert!(!setup(8));
    }
}
