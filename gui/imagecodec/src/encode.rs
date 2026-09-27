//! Writing PNG: the encoder an application saves a picture with.
//!
//! [`encode_png`] takes the pixels [`crate::decode`] gives -- `0xAARRGGBB`,
//! straight alpha, row by row -- so a picture makes a round trip through this
//! crate unchanged, and writes the smallest of the lossless forms PNG offers
//! for them:
//!
//! * **grey**, at the fewest bits that hold every level exactly, when every
//!   pixel is an opaque grey -- a black-and-white QR code is one bit a pixel;
//! * **a palette**, with a `tRNS` chunk for any colour not opaque, when there
//!   are at most 256 colours and that takes fewer bits than grey would;
//! * **grey and alpha**, for greys not all opaque in more colours than a
//!   palette holds;
//! * **RGB**, or **RGBA** where some pixel is not opaque, otherwise.
//!
//! Each row is filtered as libpng filters it: not at all for a palette or a
//! depth under eight bits, and otherwise by whichever of the five filters
//! leaves its bytes smallest in sum, each read as a signed difference. The
//! rows are compressed by this tree's `deflate` at level 6, zlib's default and
//! so libpng's.
//!
//! `testing::png_rgba`, which stores its rows uncompressed, stays the fixture
//! writer; this is the one to save a picture with.

use alloc::vec::Vec;

/// Why a picture could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// A width or height of zero: PNG has no picture of nothing.
    Empty,
    /// `pixels` does not hold `width * height` pixels.
    WrongLength {
        /// `width * height`.
        expected: u64,
        /// What was given.
        got: usize,
    },
    /// A side past PNG's 2^31 - 1, or more bytes than this machine can hold.
    TooLarge,
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("a picture of no pixels"),
            Self::WrongLength { expected, got } => {
                write!(f, "{got} pixels given for a picture of {expected}")
            }
            Self::TooLarge => f.write_str("a picture too large to write as PNG"),
        }
    }
}

/// The DEFLATE level: zlib's default, which libpng uses.
const LEVEL: u8 = 6;
/// The most compressed data one `IDAT` chunk carries. Any length to 2^31 - 1
/// is valid; this keeps a chunk -- and so what a streaming reader holds to
/// check its CRC -- modest.
const IDAT_MAX: usize = 1 << 20;
/// PNG's largest side (RFC 2083 §4.1.1: at most 2^31 - 1).
const MAX_SIDE: u32 = 0x7FFF_FFFF;

/// How the pixels are written: `IHDR`'s colour type and bit depth, and the
/// palette for colour type 3.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Form {
    /// Colour type 0 at 1, 2, 4 or 8 bits.
    Grey(u8),
    /// Colour type 4, eight bits each.
    GreyAlpha,
    /// Colour type 3 at 1, 2, 4 or 8 bits: the colours, `0xAARRGGBB`, those
    /// not opaque first so that `tRNS` can stop after the last of them.
    Palette(u8, Vec<u32>),
    /// Colour type 2, eight bits each.
    Rgb,
    /// Colour type 6, eight bits each.
    Rgba,
}

impl Form {
    const fn colour_type(&self) -> u8 {
        match self {
            Self::Grey(_) => 0,
            Self::Rgb => 2,
            Self::Palette(..) => 3,
            Self::GreyAlpha => 4,
            Self::Rgba => 6,
        }
    }

    const fn depth(&self) -> u8 {
        match self {
            Self::Grey(d) | Self::Palette(d, _) => *d,
            Self::GreyAlpha | Self::Rgb | Self::Rgba => 8,
        }
    }

    /// Samples a pixel has.
    const fn samples(&self) -> usize {
        match self {
            Self::Grey(_) | Self::Palette(..) => 1,
            Self::GreyAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    /// Whether libpng filters this form's rows: not a palette's, nor any
    /// under eight bits.
    const fn filtered(&self) -> bool {
        !matches!(self, Self::Palette(..)) && self.depth() == 8
    }
}

/// Write `width` x `height` `pixels` -- `0xAARRGGBB`, straight alpha, row by
/// row, as [`crate::decode`] gives them -- as a compressed PNG, in the
/// smallest lossless form (see the module docs).
///
/// # Errors
///
/// [`EncodeError::Empty`] for a side of zero, [`EncodeError::WrongLength`]
/// when `pixels` is not `width * height` long, [`EncodeError::TooLarge`] for a
/// side past PNG's limit or a picture this machine cannot hold.
pub fn encode_png(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 {
        return Err(EncodeError::Empty);
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(EncodeError::TooLarge);
    }
    let expected = u64::from(width).saturating_mul(u64::from(height));
    if u64::try_from(pixels.len()).ok() != Some(expected) {
        return Err(EncodeError::WrongLength {
            expected,
            got: pixels.len(),
        });
    }
    let w = usize::try_from(width).map_err(|_| EncodeError::TooLarge)?;
    let form = choose(pixels);
    let compressed = zlib(&rows(&form, w, pixels)?);

    let mut out = Vec::new();
    out.try_reserve(compressed.len().saturating_add(1024))
        .map_err(|_| EncodeError::TooLarge)?;
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // Depth and colour type; compression, filter method and interlace all 0.
    ihdr.extend_from_slice(&[form.depth(), form.colour_type(), 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    if let Form::Palette(_, colours) = &form {
        let plte: Vec<u8> = colours
            .iter()
            .flat_map(|c| {
                let [_, r, g, b] = c.to_be_bytes();
                [r, g, b]
            })
            .collect();
        chunk(&mut out, b"PLTE", &plte);
        // Alpha for the entries up to the last not opaque; the rest are
        // opaque by omission.
        let trns: Vec<u8> = colours
            .iter()
            .map(|c| c.to_be_bytes()[0])
            .take_while(|&a| a != 0xFF)
            .collect();
        if !trns.is_empty() {
            chunk(&mut out, b"tRNS", &trns);
        }
    }
    for piece in compressed.chunks(IDAT_MAX) {
        chunk(&mut out, b"IDAT", piece);
    }
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// The smallest lossless form for `pixels`, by bits a pixel -- grey before a
/// palette of the same depth, which would add a `PLTE` for nothing.
fn choose(pixels: &[u32]) -> Form {
    let mut opaque = true;
    let mut grey = true;
    // Grey depths still possible: 1, 2 and 4 bits hold exactly the levels
    // that are multiples of 255, 85 and 17.
    let (mut grey1, mut grey2, mut grey4) = (true, true, true);
    // The colours, sorted, while there are no more than 256.
    let mut colours: Vec<u32> = Vec::new();
    let mut many = false;
    let mut last = None;
    for &p in pixels {
        if last == Some(p) {
            continue;
        }
        last = Some(p);
        let [a, r, g, b] = p.to_be_bytes();
        opaque &= a == 0xFF;
        grey &= r == g && g == b;
        grey1 &= r % 255 == 0;
        grey2 &= r % 85 == 0;
        grey4 &= r % 17 == 0;
        if !many && let Err(at) = colours.binary_search(&p) {
            if colours.len() == 256 {
                many = true;
                colours = Vec::new();
            } else {
                colours.insert(at, p);
            }
        }
    }
    let palette_depth = (!many).then_some(match colours.len() {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    });
    if grey && opaque {
        let depth = if grey1 {
            1
        } else if grey2 {
            2
        } else if grey4 {
            4
        } else {
            8
        };
        return match palette_depth {
            Some(p) if p < depth => palette(p, colours),
            _ => Form::Grey(depth),
        };
    }
    match (palette_depth, grey, opaque) {
        (Some(p), ..) => palette(p, colours),
        (None, true, _) => Form::GreyAlpha,
        (None, false, true) => Form::Rgb,
        (None, false, false) => Form::Rgba,
    }
}

/// A palette of `colours` at `depth`, those not opaque first.
fn palette(depth: u8, mut colours: Vec<u32>) -> Form {
    colours.sort_by_key(|&c| (c.to_be_bytes()[0] == 0xFF, c));
    Form::Palette(depth, colours)
}

/// The rows as `IDAT` compresses them: each a filter byte, then its pixels
/// in `form`, packed most significant bits first below eight bits.
fn rows(form: &Form, width: usize, pixels: &[u32]) -> Result<Vec<u8>, EncodeError> {
    let bits = width
        .checked_mul(usize::from(form.depth()))
        .and_then(|b| b.checked_mul(form.samples()))
        .ok_or(EncodeError::TooLarge)?;
    let row_bytes = bits.div_ceil(8);
    let height = pixels.len().checked_div(width).unwrap_or(0);
    let total = row_bytes
        .checked_add(1)
        .and_then(|r| r.checked_mul(height))
        .ok_or(EncodeError::TooLarge)?;
    let mut out = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| EncodeError::TooLarge)?;
    // A palette's index for each of its colours, by colour, to look up.
    let lookup = match form {
        Form::Palette(_, colours) => {
            let mut lookup: Vec<(u32, u8)> = colours
                .iter()
                .zip(0u8..=255)
                .map(|(&c, i)| (c, i))
                .collect();
            lookup.sort_unstable();
            lookup
        }
        _ => Vec::new(),
    };
    let mut previous = alloc::vec![0u8; row_bytes];
    let mut current = alloc::vec![0u8; row_bytes];
    let mut trial = alloc::vec![0u8; row_bytes];
    let mut best = alloc::vec![0u8; row_bytes];
    for row in pixels.chunks(width) {
        pack(form, &lookup, row, &mut current);
        if form.filtered() {
            let step = form.samples();
            let mut best_filter = 0u8;
            let mut best_cost = u64::MAX;
            for filter in 0..5u8 {
                apply_filter(filter, step, &current, &previous, &mut trial);
                // Each byte as the signed difference it is: 255 is -1.
                let cost: u64 = trial
                    .iter()
                    .map(|&b| u64::from(b.min(b.wrapping_neg())))
                    .sum();
                if cost < best_cost {
                    best_cost = cost;
                    best_filter = filter;
                    core::mem::swap(&mut best, &mut trial);
                }
            }
            out.push(best_filter);
            out.extend_from_slice(&best);
        } else {
            out.push(0);
            out.extend_from_slice(&current);
        }
        core::mem::swap(&mut previous, &mut current);
    }
    Ok(out)
}

/// One row of pixels in `form`, into `out`; `lookup` a palette's indices.
fn pack(form: &Form, lookup: &[(u32, u8)], row: &[u32], out: &mut [u8]) {
    match form {
        Form::Grey(depth) => pack_values(*depth, row.iter().map(|&p| grey_level(p, *depth)), out),
        Form::Palette(depth, _) => {
            let index = |p: &u32| {
                lookup
                    .binary_search_by_key(p, |&(c, _)| c)
                    .ok()
                    .and_then(|at| lookup.get(at))
                    .map_or(0, |&(_, i)| i)
            };
            pack_values(*depth, row.iter().map(index), out);
        }
        Form::GreyAlpha => {
            for (dst, &p) in out.chunks_exact_mut(2).zip(row) {
                let [a, r, _, _] = p.to_be_bytes();
                dst.copy_from_slice(&[r, a]);
            }
        }
        Form::Rgb => {
            for (dst, &p) in out.chunks_exact_mut(3).zip(row) {
                let [_, r, g, b] = p.to_be_bytes();
                dst.copy_from_slice(&[r, g, b]);
            }
        }
        Form::Rgba => {
            for (dst, &p) in out.chunks_exact_mut(4).zip(row) {
                let [a, r, g, b] = p.to_be_bytes();
                dst.copy_from_slice(&[r, g, b, a]);
            }
        }
    }
}

/// A grey pixel's level at `depth`: its eight-bit level scaled down, which
/// [`choose`] checked is exact.
fn grey_level(p: u32, depth: u8) -> u8 {
    let level = p.to_be_bytes()[1];
    match depth {
        1 => level / 255,
        2 => level / 85,
        4 => level / 17,
        _ => level,
    }
}

/// Values of `depth` bits into bytes, most significant bits first.
fn pack_values(depth: u8, values: impl Iterator<Item = u8>, out: &mut [u8]) {
    if depth == 8 {
        for (dst, v) in out.iter_mut().zip(values) {
            *dst = v;
        }
        return;
    }
    out.fill(0);
    // 8, 4 or 2 values to a byte, for depths of 1, 2 and 4.
    let per_byte = 8u32.checked_div(u32::from(depth)).unwrap_or(1);
    let mut at = 0usize;
    let mut slot = 0u32;
    for v in values {
        let Some(byte) = out.get_mut(at) else {
            return;
        };
        let shift = 8u32.saturating_sub(u32::from(depth).saturating_mul(slot.saturating_add(1)));
        *byte |= v.checked_shl(shift).unwrap_or(0);
        slot = slot.saturating_add(1);
        if slot == per_byte {
            slot = 0;
            at = at.saturating_add(1);
        }
    }
}

/// Filter `current` by `filter` (RFC 2083 §6), given the row above, into
/// `out`.
fn apply_filter(filter: u8, step: usize, current: &[u8], previous: &[u8], out: &mut [u8]) {
    for (i, (dst, (&x, &b))) in out.iter_mut().zip(current.iter().zip(previous)).enumerate() {
        let left = i.checked_sub(step);
        let a = left.and_then(|j| current.get(j)).copied().unwrap_or(0);
        let c = left.and_then(|j| previous.get(j)).copied().unwrap_or(0);
        *dst = match filter {
            0 => x,
            1 => x.wrapping_sub(a),
            2 => x.wrapping_sub(b),
            // The floor of the mean, without widening.
            3 => x.wrapping_sub((a >> 1).wrapping_add(b >> 1).wrapping_add(a & b & 1)),
            _ => x.wrapping_sub(paeth(a, b, c)),
        };
    }
}

/// The Paeth predictor (RFC 2083 §6.6).
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    // Within -255..=510: no `i16` arithmetic here can overflow.
    let p = ia.saturating_add(ib).saturating_sub(ic);
    let (pa, pb, pc) = (
        p.saturating_sub(ia).abs(),
        p.saturating_sub(ib).abs(),
        p.saturating_sub(ic).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// `data` as a zlib stream (RFC 1950): the header, this tree's DEFLATE at
/// [`LEVEL`], and the Adler-32 of `data`.
fn zlib(data: &[u8]) -> Vec<u8> {
    let body = deflate::deflate_level(data, LEVEL);
    let mut out = Vec::with_capacity(body.len().saturating_add(6));
    // CMF 0x78: DEFLATE with a 32 KiB window. FLG 0x9C: the default level,
    // with the check bits that make the pair a multiple of 31.
    out.extend_from_slice(&[0x78, 0x9C]);
    out.extend_from_slice(&body);
    out.extend_from_slice(&deflate::adler32(data).to_be_bytes());
    out
}

/// One chunk: its length, type and data, and the CRC-32 of type and data.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    // A chunk's data is at most `IDAT_MAX` bytes, or a palette's or header's.
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32::crc32_seed(crc32::crc32_raw(!0u32, kind), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "a failed test should fail at the line that did it; the pixel \
              makers narrow small counters on purpose"
)]
mod tests {
    use super::*;
    use crate::{Limits, decode};

    /// `IHDR`'s bit depth and colour type.
    fn form_of(png: &[u8]) -> (u8, u8) {
        (png[24], png[25])
    }

    /// Encode, check the form, decode, and check every pixel came back.
    fn round_trip(width: u32, height: u32, pixels: &[u32], depth: u8, colour_type: u8) -> Vec<u8> {
        let png = encode_png(width, height, pixels).unwrap();
        assert_eq!(form_of(&png), (depth, colour_type), "form");
        let image = decode(&png, Limits::default()).unwrap();
        assert_eq!((image.width, image.height), (width, height));
        assert_eq!(image.pixels, pixels, "pixels");
        png
    }

    fn grey(level: u8) -> u32 {
        u32::from_be_bytes([0xFF, level, level, level])
    }

    #[test]
    fn two_opaque_levels_are_one_bit_grey() {
        let pixels: Vec<u32> = (0..9 * 5)
            .map(|i| grey(if i % 3 == 0 { 0 } else { 255 }))
            .collect();
        round_trip(9, 5, &pixels, 1, 0);
    }

    #[test]
    fn greys_a_depth_holds_exactly_take_that_depth() {
        let two: Vec<u32> = (0..11 * 3).map(|i| grey((i % 4) as u8 * 85)).collect();
        round_trip(11, 3, &two, 2, 0);
        let four: Vec<u32> = (0..7 * 7).map(|i| grey((i % 16) as u8 * 17)).collect();
        round_trip(7, 7, &four, 4, 0);
        let eight: Vec<u32> = (0..16 * 16).map(|i| grey(i as u8)).collect();
        round_trip(16, 16, &eight, 8, 0);
    }

    #[test]
    fn a_few_greys_no_depth_holds_are_a_small_palette() {
        // Three levels that are no multiple of 17: eight-bit grey, or a
        // two-bit palette, which is smaller.
        let pixels: Vec<u32> = (0..13 * 4).map(|i| grey(10 + (i % 3) as u8 * 10)).collect();
        round_trip(13, 4, &pixels, 2, 3);
    }

    #[test]
    fn few_colours_are_a_palette_with_their_transparency() {
        let colours = [
            0xFF12_3456,
            0x8000_FF00,
            0x0000_0000,
            0xFFAB_CDEF,
            0x40FF_0000,
        ];
        let pixels: Vec<u32> = (0..10 * 6).map(|i| colours[i % colours.len()]).collect();
        let png = round_trip(10, 6, &pixels, 4, 3);
        // The three colours not opaque come first, and tRNS stops there.
        let trns = png.windows(4).position(|w| w == b"tRNS").unwrap();
        assert_eq!(&png[trns - 4..trns], &3u32.to_be_bytes());
    }

    #[test]
    fn many_colours_are_rgb_or_rgba() {
        let opaque: Vec<u32> = (0..40 * 30)
            .map(|i: u32| 0xFF00_0000 | (i.wrapping_mul(2_654_435_761) >> 8))
            .collect();
        round_trip(40, 30, &opaque, 8, 2);
        let clear: Vec<u32> = opaque.iter().map(|&p| p & 0x7FFF_FFFF).collect();
        round_trip(40, 30, &clear, 8, 6);
    }

    #[test]
    fn many_greys_with_alpha_are_grey_and_alpha() {
        let pixels: Vec<u32> = (0..32 * 32)
            .map(|i: u32| {
                u32::from_be_bytes([
                    (i / 4) as u8,
                    (i % 256) as u8,
                    (i % 256) as u8,
                    (i % 256) as u8,
                ])
            })
            .collect();
        round_trip(32, 32, &pixels, 8, 4);
    }

    #[test]
    fn a_smooth_picture_round_trips_through_every_filter() {
        // A gradient with noise: the adaptive choice picks different filters
        // on different rows, and each must invert exactly.
        let pixels: Vec<u32> = (0..64u32 * 48)
            .map(|i| {
                let (x, y) = (i % 64, i / 64);
                let n = i.wrapping_mul(2_654_435_761) >> 29;
                u32::from_be_bytes([0xFF, (x * 4) as u8, (y * 5) as u8, ((x + y) * 2 + n) as u8])
            })
            .collect();
        round_trip(64, 48, &pixels, 8, 2);
    }

    #[test]
    fn a_qr_code_is_small() {
        // 520 x 520 of two colours in 8-pixel modules, as lane E's QR code
        // generator renders a version-10 code.
        let pixels: Vec<u32> = (0..520u32 * 520)
            .map(|i| {
                let (mx, my) = (i % 520 / 8, i / 520 / 8);
                grey(if (mx * 7 + my * 13 + mx * my) % 5 < 2 {
                    0
                } else {
                    255
                })
            })
            .collect();
        let png = round_trip(520, 520, &pixels, 1, 0);
        assert!(png.len() < 12 * 1024, "{} bytes", png.len());
    }

    #[test]
    fn a_single_pixel_of_each_kind_round_trips() {
        for p in [
            0xFF00_0000,
            0xFFFF_FFFF,
            0x0000_0000,
            0x80FF_8040,
            0xFF12_3456,
        ] {
            let png = encode_png(1, 1, &[p]).unwrap();
            let image = decode(&png, Limits::default()).unwrap();
            assert_eq!(image.pixels, [p]);
        }
    }

    #[test]
    fn nothing_or_the_wrong_count_is_refused() {
        assert_eq!(encode_png(0, 4, &[]), Err(EncodeError::Empty));
        assert_eq!(encode_png(4, 0, &[]), Err(EncodeError::Empty));
        assert_eq!(
            encode_png(2, 2, &[0; 3]),
            Err(EncodeError::WrongLength {
                expected: 4,
                got: 3
            })
        );
        assert_eq!(
            encode_png(MAX_SIDE + 1, 1, &[0]),
            Err(EncodeError::TooLarge)
        );
    }

    #[test]
    fn the_paeth_predictor_follows_the_rfc() {
        // RFC 2083 §6.6's order of preference: left, then above, then
        // upper-left, on ties.
        assert_eq!(paeth(10, 20, 10), 20);
        assert_eq!(paeth(20, 10, 10), 20);
        assert_eq!(paeth(10, 10, 10), 10);
        assert_eq!(paeth(0, 255, 255), 0);
    }
}
