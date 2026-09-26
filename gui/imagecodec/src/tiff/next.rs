//! NeXT's 2-bit greyscale compression: libtiff 4.7.1's `tif_next.c`.
//!
//! Each row is a literal row, a literal span dropped into a row of white, or
//! runs of one grey level, a byte each: the level in the top two bits, the
//! count in the other six. Transcribed with libtiff's edges kept: the buffer
//! starts white, a row the data does not reach stays white and still
//! succeeds, a run stops at the image's width, and for a tile the rows are
//! still measured by the image's scanline -- libtiff's `tif_scanlinesize`
//! -- though the runs stop at the tile's width.

use crate::{ImageError, ImageResult};

/// `LITERALROW`: the whole row follows as it is.
const LITERAL_ROW: u8 = 0x00;
/// `LITERALSPAN`: an offset, a length, and that many bytes of the row.
const LITERAL_SPAN: u8 = 0x40;

/// `NeXTPreDecode`: only 2-bit samples.
pub(super) fn pre_decode(bits_per_sample: u16) -> ImageResult<()> {
    if bits_per_sample == 2 {
        Ok(())
    } else {
        Err(ImageError::Unsupported(
            "TIFF NeXT compression of other than 2-bit samples",
        ))
    }
}

/// `NeXTDecode`: `data` into `out`, rows `scanline` bytes long and runs
/// `width` pixels at most.
pub(super) fn decode(data: &[u8], out: &mut [u8], scanline: usize, width: u64) -> ImageResult<()> {
    let bad = ImageError::Malformed("TIFF NeXT data runs out");
    // Every row starts white: min-is-black, all ones.
    out.fill(0xFF);
    if scanline == 0 || !out.len().is_multiple_of(scanline) {
        return Err(ImageError::Malformed("TIFF NeXT strip not whole rows"));
    }
    let mut input = data.iter().copied();
    for row in out.chunks_exact_mut(scanline) {
        let Some(n) = input.next() else {
            // Out of data at a row's start: the rest stays white, and the
            // strip reads.
            break;
        };
        match n {
            LITERAL_ROW => {
                for slot in row.iter_mut() {
                    *slot = input.next().ok_or(bad.clone())?;
                }
            }
            LITERAL_SPAN => {
                let mut word = || -> ImageResult<usize> {
                    let hi = input.next().ok_or(bad.clone())?;
                    let lo = input.next().ok_or(bad.clone())?;
                    Ok(usize::from(u16::from_be_bytes([hi, lo])))
                };
                let off = word()?;
                let len = word()?;
                let span = row
                    .get_mut(off..off.saturating_add(len))
                    .ok_or(bad.clone())?;
                // libtiff checks the whole span is there before copying any.
                let bytes: alloc::vec::Vec<u8> = input.by_ref().take(len).collect();
                if bytes.len() < len {
                    return Err(bad);
                }
                span.copy_from_slice(&bytes);
            }
            first => runs(first, &mut input, row, width, &bad)?,
        }
    }
    Ok(())
}

/// Run mode: bytes of `<grey><count>` until the row's pixels are done.
fn runs(
    first: u8,
    input: &mut impl Iterator<Item = u8>,
    row: &mut [u8],
    width: u64,
    bad: &ImageError,
) -> ImageResult<()> {
    let scanline = row.len();
    let (mut npixels, mut offset) = (0u64, 0usize);
    let mut n = first;
    loop {
        let grey = (n >> 6) & 3;
        let count = n & 0x3F;
        for _ in 0..count {
            if npixels >= width || offset >= scanline {
                break;
            }
            // `SETPIXEL`: four pixels a byte, the first in the top bits, the
            // byte started afresh (not over the white) by its first pixel.
            if let Some(byte) = row.get_mut(offset) {
                match npixels & 3 {
                    0 => *byte = grey << 6,
                    1 => *byte |= grey << 4,
                    2 => *byte |= grey << 2,
                    _ => {
                        *byte |= grey;
                        offset = offset.saturating_add(1);
                    }
                }
            }
            npixels = npixels.saturating_add(1);
        }
        if npixels >= width {
            return Ok(());
        }
        if offset >= scanline {
            return Err(ImageError::Malformed("TIFF NeXT run past its row"));
        }
        n = input.next().ok_or(bad.clone())?;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_literal_row_is_copied() {
        let mut out = [0u8; 4];
        decode(&[0x00, 1, 2], &mut out, 2, 8).unwrap();
        // The second row has no data: white.
        assert_eq!(out, [1, 2, 0xFF, 0xFF]);
    }

    #[test]
    fn a_span_lands_in_a_white_row() {
        let mut out = [0u8; 4];
        decode(&[0x40, 0, 2, 0, 1, 0x5A], &mut out, 4, 16).unwrap();
        assert_eq!(out, [0xFF, 0xFF, 0x5A, 0xFF]);
    }

    #[test]
    fn runs_pack_four_pixels_a_byte_and_stop_at_the_width() {
        let mut out = [0u8; 2];
        // Three black (level 0), then level 2 for more than is left of the
        // six-pixel row: the run stops at the width.
        decode(&[0x03, 0x80 | 10], &mut out, 2, 6).unwrap();
        // 00 00 00 10 | 10 10 (a byte started by its first pixel: zeros after).
        assert_eq!(out, [0b0000_0010, 0b1010_0000]);
    }

    #[test]
    fn damage_is_refused_where_libtiff_refuses_it() {
        let mut out = [0u8; 4];
        // A literal row cut short.
        assert!(decode(&[0x00, 1], &mut out, 4, 16).is_err());
        // A span past the row.
        assert!(decode(&[0x40, 0, 3, 0, 2, 1, 2], &mut out, 4, 16).is_err());
        // Runs that end before the row does.
        assert!(decode(&[0x02], &mut out, 4, 16).is_err());
        // A strip that is not whole rows.
        assert!(decode(&[0x00], &mut out, 3, 12).is_err());
        assert!(pre_decode(4).is_err());
        assert!(pre_decode(2).is_ok());
    }
}
