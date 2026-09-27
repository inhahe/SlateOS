//! Writing a picture as a PNG.
//!
//! # Why
//!
//! Nothing in the tree could write one. `imagecodec` decodes and does not
//! write files; Paint saved only BMP, 32 bits a pixel with no compression at
//! all; the screenshot tool wrote BMP too, eight megabytes for one full-HD
//! screen. PNG is the format every other system opens, keeps every pixel
//! exactly, and carries transparency.
//!
//! # What it writes
//!
//! Eight-bit truecolour, not interlaced: RGB when every pixel is opaque (a
//! quarter less to compress), RGB with alpha when any is not. Each row is
//! filtered with whichever of PNG's five filters leaves the smallest sum of
//! absolute differences -- libpng's own default heuristic, which is what
//! makes a photograph or a screenshot compress well -- and the rows are
//! compressed by the tree's `deflate` at level 6, the level zlib and libpng
//! default to.
//!
//! Nothing else: no gamma, no colour profile, no text chunks. The picture is
//! its pixels, straight-alpha `0xAARRGGBB` as `imagecodec` decodes them, so a
//! picture written here and read back is the same picture, pixel for pixel.

use std::fmt;

/// Why a picture could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// A width or a height of zero: PNG has no empty picture.
    Empty,
    /// A side past PNG's limit of 2^31 - 1 pixels, or a picture too large to
    /// hold in memory to filter.
    TooLarge,
    /// The pixels are not `width * height` of them.
    WrongLength { expected: usize, got: usize },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a picture with no pixels cannot be a PNG"),
            Self::TooLarge => write!(f, "the picture is too large for a PNG"),
            Self::WrongLength { expected, got } => {
                write!(f, "{got} pixels given for a picture of {expected}")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// The PNG signature every file begins with.
const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// PNG's largest side.
const MAX_SIDE: u32 = 0x7FFF_FFFF;

/// The compression effort: what zlib and libpng use unless asked otherwise.
const LEVEL: u8 = 6;

/// The picture `pixels` -- `width` by `height`, row by row, straight-alpha
/// `0xAARRGGBB` -- as the bytes of a PNG file.
///
/// # Errors
///
/// When the picture has no pixels, is larger than PNG allows or than can be
/// held to filter, or `pixels` is not `width * height` long.
pub fn encode(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 {
        return Err(EncodeError::Empty);
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(EncodeError::TooLarge);
    }
    let (w, h) = (
        usize::try_from(width).map_err(|_| EncodeError::TooLarge)?,
        usize::try_from(height).map_err(|_| EncodeError::TooLarge)?,
    );
    let expected = w.checked_mul(h).ok_or(EncodeError::TooLarge)?;
    if pixels.len() != expected {
        return Err(EncodeError::WrongLength {
            expected,
            got: pixels.len(),
        });
    }
    let alpha = pixels.iter().any(|p| p >> 24 != 0xFF);
    let bpp: usize = if alpha { 4 } else { 3 };
    let row_len = w.checked_mul(bpp).ok_or(EncodeError::TooLarge)?;
    let filtered_len = row_len
        .checked_add(1)
        .and_then(|r| r.checked_mul(h))
        .ok_or(EncodeError::TooLarge)?;

    let mut filtered = Vec::with_capacity(filtered_len);
    let mut previous = vec![0_u8; row_len];
    let mut current = vec![0_u8; row_len];
    let mut candidate = vec![0_u8; row_len];
    let mut best = vec![0_u8; row_len];
    for row in pixels.chunks_exact(w) {
        current.clear();
        for &p in row {
            let [a, r, g, b] = p.to_be_bytes();
            current.extend_from_slice(&[r, g, b]);
            if alpha {
                current.push(a);
            }
        }
        let mut best_filter = 0_u8;
        let mut best_cost = u64::MAX;
        for filter in 0..=4_u8 {
            apply_filter(filter, &current, &previous, bpp, &mut candidate);
            let cost = candidate
                .iter()
                .map(|&b| u64::from(b.cast_signed().unsigned_abs()))
                .sum::<u64>();
            if cost < best_cost {
                best_cost = cost;
                best_filter = filter;
                best.copy_from_slice(&candidate);
            }
        }
        filtered.push(best_filter);
        filtered.extend_from_slice(&best);
        std::mem::swap(&mut previous, &mut current);
    }

    let mut out = Vec::new();
    out.extend_from_slice(SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8; colour type 6 (RGBA) or 2 (RGB); deflate; adaptive
    // filtering; no interlace.
    ihdr.extend_from_slice(&[8, if alpha { 6 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&filtered));
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Filter `row` with PNG filter `filter` (0 None, 1 Sub, 2 Up, 3 Average,
/// 4 Paeth), given the row above, into `out`.
fn apply_filter(filter: u8, row: &[u8], above: &[u8], bpp: usize, out: &mut [u8]) {
    for (i, ((slot, &x), &b)) in out.iter_mut().zip(row).zip(above).enumerate() {
        // The byte to the left and the one above-left, zero off the edge.
        let left = i.checked_sub(bpp);
        let a = left.and_then(|j| row.get(j)).copied().unwrap_or(0);
        let c = left.and_then(|j| above.get(j)).copied().unwrap_or(0);
        *slot = match filter {
            0 => x,
            1 => x.wrapping_sub(a),
            2 => x.wrapping_sub(b),
            3 => {
                // Rounded down, as PNG's Average filter is.
                let average = u16::midpoint(u16::from(a), u16::from(b));
                x.wrapping_sub(u8::try_from(average).unwrap_or(u8::MAX))
            }
            _ => x.wrapping_sub(paeth(a, b, c)),
        };
    }
}

/// The Paeth predictor: whichever of left, above and above-left is nearest
/// `left + above - above_left`, in that order of preference.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sums and differences of three bytes held in an i16 cannot overflow"
)]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// `data` as a zlib stream: the header, deflate at [`LEVEL`], the Adler-32.
fn zlib(data: &[u8]) -> Vec<u8> {
    // CMF 0x78: deflate with a 32 KiB window. FLG 0x9C: the default level,
    // no dictionary, and the check bits that make 0x789C a multiple of 31.
    let mut out = vec![0x78, 0x9C];
    out.extend_from_slice(&deflate::deflate_level(data, LEVEL));
    out.extend_from_slice(&deflate::adler32(data).to_be_bytes());
    out
}

/// Append the chunk `kind` holding `data`, with its length and CRC.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    // A chunk's length is four bytes; the only chunk here that could pass
    // it is IDAT, of a picture far past what `encode` accepts.
    let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32::crc32_seed(crc32::crc32_raw(!0, kind), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

#[cfg(test)]
mod tests;
