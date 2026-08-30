//! PNG decoding (RFC 2083), in full and without trusting the file.
//!
//! Every colour type, every bit depth, palettes, transparency and Adam7
//! interlacing. The parts deliberately not implemented are listed in the crate
//! docs; the short version is that colour management is parsed past rather than
//! half-applied, and an animated PNG decodes to its first frame.
//!
//! # The order things must happen in
//!
//! Layout is not incidental here. A PNG's decompressed size is *exactly*
//! computable from its header, so the sequence is:
//!
//! 1. Read `IHDR` and validate the colour-type/bit-depth pairing.
//! 2. Check the pixel count against the caller's [`Limits`] — **before** any
//!    buffer that count would size exists.
//! 3. Compute the exact decompressed size the header implies.
//! 4. Decompress with that as the hard limit, so a stream that wants more is
//!    stopped at the first byte past it rather than after it has been believed.
//!
//! Doing (2) after (4), or (4) without a limit, is how a decoder turns eight
//! bytes of header into gigabytes of allocation.
//!
//! # Checksums
//!
//! Every chunk carries a CRC-32. **Critical chunks** (`IHDR`, `PLTE`, `IDAT`,
//! `IEND` — the ones whose type begins with a capital letter) must pass: a
//! corrupt `IHDR` or `IDAT` does not produce a slightly-wrong picture, it
//! produces a confidently-wrong one. **Ancillary chunks** with a bad CRC are
//! skipped rather than fatal, which is what libpng does and what keeps a file
//! with one damaged text comment from being unopenable. Rejecting the whole
//! file for a bad `tEXt` would make this decoder stricter than every other
//! decoder the user's files have been through.

use alloc::vec;
use alloc::vec::Vec;

use deflate::zlib_inflate_limited;

use crate::{Image, ImageError, ImageResult, Limits};

/// The eight bytes every PNG begins with (RFC 2083 §3.1).
///
/// Chosen by the format's authors to catch the ways a file gets mangled in
/// transit: a high bit to detect seven-bit channels, `\r\n` and `\n` to detect
/// line-ending translation, and a `^Z` so that `type` on DOS stops there.
pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The seven Adam7 passes, as `(x_start, y_start, x_step, y_step)`.
const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// Does this look like a PNG?
///
/// Signature only — a file that starts with these eight bytes and is rubbish
/// afterwards is a *broken PNG*, which is a more useful thing to tell the user
/// than "unknown format".
#[must_use]
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.get(..8) == Some(&SIGNATURE)
}

/// How the samples in the pixel data are arranged (RFC 2083 §4.1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorType {
    /// One grey sample.
    Gray,
    /// Red, green, blue.
    Rgb,
    /// One index into `PLTE`.
    Palette,
    /// Grey and alpha.
    GrayAlpha,
    /// Red, green, blue, alpha.
    Rgba,
}

impl ColorType {
    const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Gray,
            2 => Self::Rgb,
            3 => Self::Palette,
            4 => Self::GrayAlpha,
            6 => Self::Rgba,
            _ => return None,
        })
    }

    /// Samples per pixel in the *stored* data. A palette entry is one sample
    /// however many channels the colour it names has.
    const fn channels(self) -> u32 {
        match self {
            Self::Gray | Self::Palette => 1,
            Self::GrayAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    /// Which bit depths RFC 2083 Table 11.1 allows with this colour type.
    ///
    /// Not a formality: a 16-bit palette index has no meaning (the palette has
    /// at most 256 entries), and a 1-bit RGB pixel has no room for three
    /// channels. A decoder that accepted either would be reading a layout the
    /// encoder never wrote.
    const fn allows_depth(self, depth: u8) -> bool {
        match self {
            Self::Gray => matches!(depth, 1 | 2 | 4 | 8 | 16),
            Self::Palette => matches!(depth, 1 | 2 | 4 | 8),
            Self::Rgb | Self::GrayAlpha | Self::Rgba => matches!(depth, 8 | 16),
        }
    }
}

/// The contents of `IHDR`.
#[derive(Clone, Copy, Debug)]
struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: ColorType,
    interlaced: bool,
}

impl Header {
    /// Bytes one row of `width` pixels occupies, rounded up to a whole byte.
    ///
    /// `u64` throughout: `width` is a number the file chose, and the product
    /// with four channels of sixteen bits overflows `u32` well before it
    /// reaches the declared maximum.
    fn row_bytes(&self, width: u32) -> u64 {
        let bits = u64::from(width)
            .saturating_mul(u64::from(self.color.channels()))
            .saturating_mul(u64::from(self.depth));
        bits.saturating_add(7) / 8
    }

    /// Distance in bytes between a filtered byte and the corresponding byte of
    /// the pixel to its left — RFC 2083's `bpp`, rounded *up* to one for
    /// sub-byte depths, where "the pixel to the left" is the byte to the left.
    const fn filter_step(&self) -> usize {
        let bits = self.color.channels().saturating_mul(self.depth as u32);
        let bytes = bits / 8;
        if bytes == 0 { 1 } else { bytes as usize }
    }
}

/// Parse just enough to answer "how big is it?".
///
/// # Errors
///
/// [`ImageError::UnknownFormat`] if the signature is absent,
/// [`ImageError::Truncated`] if `IHDR` is not there, and
/// [`ImageError::Malformed`] naming the field if one is out of range.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let h = read_header(bytes)?;
    Ok((h.width, h.height))
}

/// Read and validate `IHDR`, which RFC 2083 requires to be the first chunk.
fn read_header(bytes: &[u8]) -> ImageResult<Header> {
    if !is_png(bytes) {
        return Err(ImageError::UnknownFormat);
    }
    let mut chunks = Chunks::new(bytes);
    let first = chunks.next().ok_or(ImageError::Truncated)??;
    if first.kind != *b"IHDR" {
        return Err(ImageError::Malformed("first chunk is not IHDR"));
    }
    parse_ihdr(first.data)
}

fn parse_ihdr(data: &[u8]) -> ImageResult<Header> {
    if data.len() != 13 {
        return Err(ImageError::Malformed("IHDR length"));
    }
    let width = be_u32(data, 0);
    let height = be_u32(data, 4);
    let depth = *data.get(8).unwrap_or(&0);
    let color_byte = *data.get(9).unwrap_or(&0);
    let compression = *data.get(10).unwrap_or(&0);
    let filter = *data.get(11).unwrap_or(&0);
    let interlace = *data.get(12).unwrap_or(&0);

    // RFC 2083 §4.1.1: "Zero is an invalid value." A zero-dimension image has
    // no pixels, and every downstream size computation would divide by it.
    if width == 0 || height == 0 {
        return Err(ImageError::Malformed("IHDR zero dimension"));
    }
    let color =
        ColorType::from_byte(color_byte).ok_or(ImageError::Malformed("IHDR colour type"))?;
    if !color.allows_depth(depth) {
        return Err(ImageError::Malformed("IHDR bit depth for this colour type"));
    }
    // Only method 0 (zlib/DEFLATE) and filter method 0 (the five filters
    // below) have ever been defined. A file naming another is either from the
    // future or corrupt, and guessing which is not a decoder's job.
    if compression != 0 {
        return Err(ImageError::Unsupported("PNG compression method"));
    }
    if filter != 0 {
        return Err(ImageError::Unsupported("PNG filter method"));
    }
    let interlaced = match interlace {
        0 => false,
        1 => true,
        _ => return Err(ImageError::Malformed("IHDR interlace method")),
    };

    Ok(Header {
        width,
        height,
        depth,
        color,
        interlaced,
    })
}

/// Decode a PNG into `0xAARRGGBB` pixels.
///
/// # Errors
///
/// [`ImageError`] — a malformed header, a picture over `limits`, a failed
/// critical-chunk checksum, or unreadable compressed data. Never panics, for
/// any input.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let header = read_header(bytes)?;

    // (2) in the module docs: refuse from the header, before anything the
    // header would size is allocated.
    let pixels = u64::from(header.width).saturating_mul(u64::from(header.height));
    if pixels > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        });
    }

    let mut palette: Vec<u32> = Vec::new();
    let mut trns: Option<Vec<u8>> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_iend = false;
    let mut seen_idat = false;

    for chunk in Chunks::new(bytes).skip(1) {
        let chunk = chunk?;
        match &chunk.kind {
            b"IHDR" => return Err(ImageError::Malformed("second IHDR")),
            b"PLTE" => {
                if seen_idat {
                    return Err(ImageError::Malformed("PLTE after IDAT"));
                }
                palette = parse_plte(chunk.data)?;
            }
            b"tRNS" => {
                if seen_idat {
                    return Err(ImageError::Malformed("tRNS after IDAT"));
                }
                trns = Some(chunk.data.to_vec());
            }
            b"IDAT" => {
                seen_idat = true;
                // RFC 2083 §4.1.3: the IDATs are one continuous stream that
                // happens to be chopped up, so they must be joined before the
                // zlib header is looked at — not decompressed one at a time.
                idat.extend_from_slice(chunk.data);
            }
            b"IEND" => {
                seen_iend = true;
                break;
            }
            // Everything else — text, timestamps, colour management, APNG
            // control chunks — is skipped. See the crate docs for why colour
            // management in particular is skipped rather than half-applied.
            _ => {}
        }
    }

    if !seen_iend {
        return Err(ImageError::Truncated);
    }
    if !seen_idat {
        return Err(ImageError::Malformed("no IDAT"));
    }
    if header.color == ColorType::Palette && palette.is_empty() {
        // Not pedantry: without it every pixel would name a colour that does
        // not exist, and the natural fallback (black) is a picture rather than
        // an error.
        return Err(ImageError::Malformed("indexed image with no PLTE"));
    }

    // (3) and (4): the exact size, used as the hard limit.
    let raw_size = raw_size(&header);
    let limit = usize::try_from(raw_size)
        .ok()
        .filter(|&n| n <= limits.max_decompressed_bytes)
        .ok_or(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        })?;
    let raw = zlib_inflate_limited(&idat, limit)?;
    if raw.len() != limit {
        // Short is a truncated file; the decompressor already refuses long.
        return Err(ImageError::Truncated);
    }

    let mut out = vec![0u32; usize::try_from(pixels).unwrap_or(0)];
    if header.interlaced {
        expand_adam7(&header, &raw, &palette, trns.as_deref(), &mut out)?;
    } else {
        expand_pass(
            &header,
            &raw,
            &palette,
            trns.as_deref(),
            &mut out,
            (0, 0, 1, 1),
            header.width,
            header.height,
        )?;
    }

    Ok(Image {
        width: header.width,
        height: header.height,
        pixels: out,
    })
}

/// Decompressed size the header implies: for each pass, one filter byte per row
/// plus the row itself.
fn raw_size(h: &Header) -> u64 {
    if !h.interlaced {
        return u64::from(h.height).saturating_mul(h.row_bytes(h.width).saturating_add(1));
    }
    let mut total = 0u64;
    for &(xs, ys, xstep, ystep) in &ADAM7 {
        let (pw, ph) = pass_size(h.width, h.height, xs, ys, xstep, ystep);
        if pw == 0 || ph == 0 {
            // A pass with no pixels contributes no bytes at all — not even
            // filter bytes. Counting them is the classic Adam7 off-by-N and
            // shows up only on images narrower than eight pixels.
            continue;
        }
        total =
            total.saturating_add(u64::from(ph).saturating_mul(h.row_bytes(pw).saturating_add(1)));
    }
    total
}

/// How many pixels wide and tall one Adam7 pass is.
const fn pass_size(
    width: u32,
    height: u32,
    xs: u32,
    ys: u32,
    xstep: u32,
    ystep: u32,
) -> (u32, u32) {
    // `checked_div` rather than `/`: the seven steps are compile-time constants
    // and none is zero, but a divisor the compiler cannot see is a divisor that
    // could one day be zero, and this function must never be the thing that
    // panics on a picture.
    let w = match width
        .saturating_sub(xs)
        .saturating_add(xstep)
        .saturating_sub(1)
        .checked_div(xstep)
    {
        Some(v) if width > xs => v,
        _ => 0,
    };
    let h = match height
        .saturating_sub(ys)
        .saturating_add(ystep)
        .saturating_sub(1)
        .checked_div(ystep)
    {
        Some(v) if height > ys => v,
        _ => 0,
    };
    (w, h)
}

/// Walk the seven passes, each of which is an independently filtered image of
/// its own size, and scatter their pixels into the full-size output.
fn expand_adam7(
    h: &Header,
    raw: &[u8],
    palette: &[u32],
    trns: Option<&[u8]>,
    out: &mut [u32],
) -> ImageResult<()> {
    let mut offset = 0usize;
    for &(xs, ys, xstep, ystep) in &ADAM7 {
        let (pw, ph) = pass_size(h.width, h.height, xs, ys, xstep, ystep);
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = usize::try_from(h.row_bytes(pw).saturating_add(1))
            .map_err(|_| ImageError::Truncated)?;
        let len = stride
            .checked_mul(ph as usize)
            .ok_or(ImageError::Truncated)?;
        let slice = raw
            .get(offset..offset.saturating_add(len))
            .ok_or(ImageError::Truncated)?;
        expand_pass(h, slice, palette, trns, out, (xs, ys, xstep, ystep), pw, ph)?;
        offset = offset.saturating_add(len);
    }
    Ok(())
}

/// Unfilter one pass and convert its samples to `0xAARRGGBB`.
///
/// `placement` is `(x_start, y_start, x_step, y_step)`; for a non-interlaced
/// image it is `(0, 0, 1, 1)`, which is why there is no second copy of this
/// function for the simple case.
#[allow(clippy::too_many_arguments)]
fn expand_pass(
    h: &Header,
    raw: &[u8],
    palette: &[u32],
    trns: Option<&[u8]>,
    out: &mut [u32],
    placement: (u32, u32, u32, u32),
    pass_w: u32,
    pass_h: u32,
) -> ImageResult<()> {
    let (xs, ys, xstep, ystep) = placement;
    let row_len = usize::try_from(h.row_bytes(pass_w)).map_err(|_| ImageError::Truncated)?;
    let step = h.filter_step();

    // Two rows kept, because Up/Average/Paeth all read the *reconstructed*
    // previous row and nothing further back. Reconstructing into a full-size
    // buffer and indexing backwards would work too and would hold the whole
    // filtered image a second time.
    let mut prev = vec![0u8; row_len];
    let mut cur = vec![0u8; row_len];

    for y in 0..pass_h {
        let at = (y as usize)
            .checked_mul(row_len.saturating_add(1))
            .ok_or(ImageError::Truncated)?;
        let filter = *raw.get(at).ok_or(ImageError::Truncated)?;
        let src = raw
            .get(at.saturating_add(1)..at.saturating_add(1).saturating_add(row_len))
            .ok_or(ImageError::Truncated)?;
        cur.clear();
        cur.extend_from_slice(src);
        unfilter(filter, &mut cur, &prev, step)?;

        let out_y = ys.saturating_add(y.saturating_mul(ystep));
        for x in 0..pass_w {
            let out_x = xs.saturating_add(x.saturating_mul(xstep));
            let idx = (out_y as usize)
                .checked_mul(h.width as usize)
                .and_then(|v| v.checked_add(out_x as usize))
                .ok_or(ImageError::Truncated)?;
            let px = pixel_at(h, &cur, x as usize, palette, trns)?;
            if let Some(slot) = out.get_mut(idx) {
                *slot = px;
            }
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    Ok(())
}

/// Reverse one of the five scanline filters (RFC 2083 §6), in place.
///
/// All arithmetic is modulo 256 by definition of the format — `wrapping_add` is
/// the specification here, not a shortcut around an overflow check.
fn unfilter(filter: u8, cur: &mut [u8], prev: &[u8], step: usize) -> ImageResult<()> {
    match filter {
        0 => {}
        // Sub: each byte is a delta from the byte one pixel to the left.
        1 => {
            for i in step..cur.len() {
                let left = *cur.get(i.saturating_sub(step)).unwrap_or(&0);
                if let Some(slot) = cur.get_mut(i) {
                    *slot = slot.wrapping_add(left);
                }
            }
        }
        // Up: a delta from the byte above.
        2 => {
            for i in 0..cur.len() {
                let up = *prev.get(i).unwrap_or(&0);
                if let Some(slot) = cur.get_mut(i) {
                    *slot = slot.wrapping_add(up);
                }
            }
        }
        // Average: a delta from the mean of left and above, floored. The sum is
        // computed in u16 because the spec says so — a u8 sum would wrap before
        // the halving and give a different, wrong answer.
        3 => {
            for i in 0..cur.len() {
                let left = if i >= step {
                    u16::from(*cur.get(i.saturating_sub(step)).unwrap_or(&0))
                } else {
                    0
                };
                let up = u16::from(*prev.get(i).unwrap_or(&0));
                let avg = (left.saturating_add(up) / 2) as u8;
                if let Some(slot) = cur.get_mut(i) {
                    *slot = slot.wrapping_add(avg);
                }
            }
        }
        // Paeth: a delta from whichever of left/above/above-left is closest to
        // their linear prediction.
        4 => {
            for i in 0..cur.len() {
                let (a, c) = if i >= step {
                    let j = i.saturating_sub(step);
                    (*cur.get(j).unwrap_or(&0), *prev.get(j).unwrap_or(&0))
                } else {
                    (0, 0)
                };
                let b = *prev.get(i).unwrap_or(&0);
                let pred = paeth(a, b, c);
                if let Some(slot) = cur.get_mut(i) {
                    *slot = slot.wrapping_add(pred);
                }
            }
        }
        _ => return Err(ImageError::Malformed("scanline filter type")),
    }
    Ok(())
}

/// RFC 2083 §6.6's predictor. `i32` because `p` can be negative even though
/// every input and the answer are bytes.
const fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ai, bi, ci) = (a as i32, b as i32, c as i32);
    // Saturating throughout. Three bytes cannot overflow an `i32` and never
    // will, but the arithmetic in a decoder of untrusted files should not
    // depend on a reader checking that.
    let p = ai.saturating_add(bi).saturating_sub(ci);
    let pa = p.saturating_sub(ai).saturating_abs();
    let pb = p.saturating_sub(bi).saturating_abs();
    let pc = p.saturating_sub(ci).saturating_abs();
    // Ties go to `a`, then `b`. The order is normative: a decoder that broke
    // ties the other way would produce a picture that is right almost
    // everywhere, which is the hardest kind of wrong to notice.
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Read sample number `index` out of an unfiltered row.
///
/// Returns the sample in its own range: 0..=1 for a one-bit image, 0..=65535
/// for a sixteen-bit one. Scaling to eight bits is the caller's, because the
/// transparency comparison has to happen at the original depth.
fn sample(row: &[u8], depth: u8, index: usize) -> u32 {
    match depth {
        16 => {
            let at = index.saturating_mul(2);
            let hi = u32::from(*row.get(at).unwrap_or(&0));
            let lo = u32::from(*row.get(at.saturating_add(1)).unwrap_or(&0));
            (hi << 8) | lo
        }
        8 => u32::from(*row.get(index).unwrap_or(&0)),
        // Sub-byte samples are packed most-significant first within each byte,
        // which is the opposite of the least-significant-first bit order that
        // DEFLATE uses to deliver these very bytes.
        _ => {
            let per_byte = usize::from(8u8.checked_div(depth).unwrap_or(8)).max(1);
            let byte = *row
                .get(index.checked_div(per_byte).unwrap_or(0))
                .unwrap_or(&0);
            let within = index.checked_rem(per_byte).unwrap_or(0);
            let shift = 8u32.saturating_sub(u32::from(depth)).saturating_sub(
                u32::try_from(within)
                    .unwrap_or(0)
                    .saturating_mul(u32::from(depth)),
            );
            let mask = 1u32
                .checked_shl(u32::from(depth))
                .unwrap_or(0)
                .saturating_sub(1);
            (u32::from(byte) >> shift) & mask
        }
    }
}

/// Widen a sample of `depth` bits to the full 0..=255 range.
///
/// Multiplication rather than a shift, so that the maximum value maps to 255
/// and not to 254: a one-bit white of `1 << 7` would be 128, a grey.
const fn scale_to_8(v: u32, depth: u8) -> u32 {
    match depth {
        16 => v >> 8,
        8 => v & 0xFF,
        4 => (v & 0x0F).saturating_mul(17),
        2 => (v & 0x03).saturating_mul(85),
        _ => (v & 0x01).saturating_mul(255),
    }
}

/// Convert pixel `x` of an unfiltered row into `0xAARRGGBB`.
fn pixel_at(
    h: &Header,
    row: &[u8],
    x: usize,
    palette: &[u32],
    trns: Option<&[u8]>,
) -> ImageResult<u32> {
    let base = x.saturating_mul(h.color.channels() as usize);
    let d = h.depth;

    let px = match h.color {
        ColorType::Gray => {
            let raw = sample(row, d, base);
            let g = scale_to_8(raw, d);
            let a = if trns_gray_matches(trns, raw) { 0 } else { 255 };
            (a << 24) | (g << 16) | (g << 8) | g
        }
        ColorType::GrayAlpha => {
            let g = scale_to_8(sample(row, d, base), d);
            let a = scale_to_8(sample(row, d, base.saturating_add(1)), d);
            (a << 24) | (g << 16) | (g << 8) | g
        }
        ColorType::Rgb => {
            let rr = sample(row, d, base);
            let gg = sample(row, d, base.saturating_add(1));
            let bb = sample(row, d, base.saturating_add(2));
            let a = if trns_rgb_matches(trns, rr, gg, bb) {
                0
            } else {
                255
            };
            (a << 24) | (scale_to_8(rr, d) << 16) | (scale_to_8(gg, d) << 8) | scale_to_8(bb, d)
        }
        ColorType::Rgba => {
            let rr = scale_to_8(sample(row, d, base), d);
            let gg = scale_to_8(sample(row, d, base.saturating_add(1)), d);
            let bb = scale_to_8(sample(row, d, base.saturating_add(2)), d);
            let aa = scale_to_8(sample(row, d, base.saturating_add(3)), d);
            (aa << 24) | (rr << 16) | (gg << 8) | bb
        }
        ColorType::Palette => {
            let idx = sample(row, d, base) as usize;
            // An index past the palette is a malformed file. Returning an error
            // rather than a default colour: a picture that renders in the wrong
            // colours is a bug report about the *encoder* that nobody can act
            // on, where a refusal names the file.
            let rgb = *palette
                .get(idx)
                .ok_or(ImageError::Malformed("palette index past PLTE"))?;
            let a = trns
                .and_then(|t| t.get(idx))
                .map_or(255u32, |&v| u32::from(v));
            (a << 24) | (rgb & 0x00FF_FFFF)
        }
    };
    Ok(px)
}

/// Does this greyscale sample match the `tRNS` colour-key?
///
/// The key is stored at the image's own bit depth in a two-byte big-endian
/// field, so the comparison is against the *raw* sample, before scaling.
fn trns_gray_matches(trns: Option<&[u8]>, raw: u32) -> bool {
    let Some(t) = trns else { return false };
    if t.len() < 2 {
        return false;
    }
    let key = (u32::from(*t.first().unwrap_or(&0)) << 8) | u32::from(*t.get(1).unwrap_or(&0));
    key == raw
}

/// The same for truecolour, where `tRNS` holds three two-byte samples.
fn trns_rgb_matches(trns: Option<&[u8]>, r: u32, g: u32, b: u32) -> bool {
    let Some(t) = trns else { return false };
    if t.len() < 6 {
        return false;
    }
    let at = |i: usize| -> u32 {
        (u32::from(*t.get(i).unwrap_or(&0)) << 8)
            | u32::from(*t.get(i.saturating_add(1)).unwrap_or(&0))
    };
    at(0) == r && at(2) == g && at(4) == b
}

/// `PLTE` is a run of RGB triples, at most 256 of them.
fn parse_plte(data: &[u8]) -> ImageResult<Vec<u32>> {
    if !data.len().is_multiple_of(3) || data.len() > 256 * 3 {
        return Err(ImageError::Malformed("PLTE length"));
    }
    Ok(data
        .chunks_exact(3)
        .map(|c| {
            (u32::from(*c.first().unwrap_or(&0)) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0))
        })
        .collect())
}

/// One chunk, borrowed out of the file.
struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
}

/// Walks a PNG's chunks, checking each one's CRC.
///
/// An iterator rather than a `Vec<Chunk>` so that a file with ten thousand
/// `tEXt` chunks costs nothing to skip past, and so that the walk stops at
/// `IEND` without having parsed whatever follows it.
struct Chunks<'a> {
    bytes: &'a [u8],
    pos: usize,
    done: bool,
}

impl<'a> Chunks<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 8,
            done: false,
        }
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = ImageResult<Chunk<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.done || self.pos >= self.bytes.len() {
                return None;
            }
            let header = match self.bytes.get(self.pos..self.pos.saturating_add(8)) {
                Some(h) => h,
                None => {
                    self.done = true;
                    return Some(Err(ImageError::Truncated));
                }
            };
            let len = be_u32(header, 0) as usize;
            let kind = [
                *header.get(4).unwrap_or(&0),
                *header.get(5).unwrap_or(&0),
                *header.get(6).unwrap_or(&0),
                *header.get(7).unwrap_or(&0),
            ];
            let start = self.pos.saturating_add(8);
            let end = match start.checked_add(len) {
                Some(e) => e,
                None => {
                    self.done = true;
                    return Some(Err(ImageError::Truncated));
                }
            };
            let crc_end = end.saturating_add(4);
            let (data, crc_bytes) = match (self.bytes.get(start..end), self.bytes.get(end..crc_end))
            {
                (Some(d), Some(c)) => (d, c),
                _ => {
                    self.done = true;
                    return Some(Err(ImageError::Truncated));
                }
            };
            self.pos = crc_end;

            let want = be_u32(crc_bytes, 0);
            let got = crc32(&kind, data);
            if want != got {
                // Critical chunks are named with a capital first letter; bit 5
                // of the byte is the case bit. See the module docs for why the
                // two are treated differently.
                let critical = kind.first().is_some_and(|b| b & 0x20 == 0);
                if critical {
                    self.done = true;
                    return Some(Err(ImageError::Corrupt("critical chunk CRC")));
                }
                continue;
            }
            if kind == *b"IEND" {
                self.done = true;
            }
            return Some(Ok(Chunk { kind, data }));
        }
    }
}

/// Big-endian `u32` at `offset`, or zero if the slice is short. Every caller
/// has already established the slice is long enough; the default keeps the
/// function total rather than making that proof load-bearing.
fn be_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        *data.get(offset).unwrap_or(&0),
        *data.get(offset.saturating_add(1)).unwrap_or(&0),
        *data.get(offset.saturating_add(2)).unwrap_or(&0),
        *data.get(offset.saturating_add(3)).unwrap_or(&0),
    ])
}

/// The CRC-32 table PNG uses (the reflected IEEE 802.3 polynomial).
///
/// Built at compile time, so there is no lazily-initialised static to be racy
/// and no cost to the first picture decoded.
const CRC_TABLE: [u32; 256] = build_crc_table();

// A `const fn` may not call `get_mut`, and an array of 256 has to be written to
// by index to be built at all. Both counters are bounded by the loop conditions
// immediately above them, and the whole function runs at compile time — an
// out-of-range index here would be a build error, not a runtime one.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

/// CRC-32 over a chunk's type and data, which is what the trailing four bytes
/// cover — the length field is deliberately *not* included (RFC 2083 §3.2).
fn crc32(kind: &[u8; 4], data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &byte in kind.iter().chain(data.iter()) {
        let idx = ((c ^ u32::from(byte)) & 0xFF) as usize;
        c = CRC_TABLE.get(idx).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use alloc::vec;

    /// Wrap raw bytes in a chunk with a correct CRC.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(kind, data).to_be_bytes());
        out
    }

    fn ihdr(w: u32, h: u32, depth: u8, color: u8, interlace: u8) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&w.to_be_bytes());
        d.extend_from_slice(&h.to_be_bytes());
        d.push(depth);
        d.push(color);
        d.push(0); // compression
        d.push(0); // filter
        d.push(interlace);
        chunk(b"IHDR", &d)
    }

    /// A zlib stream of `raw`, using stored DEFLATE blocks so the tests need no
    /// compressor. Chunked at 60000 so a large image still produces legal
    /// blocks.
    fn zlib_stored(raw: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78u8, 0x01];
        let chunks: Vec<&[u8]> = if raw.is_empty() {
            vec![&[]]
        } else {
            raw.chunks(60000).collect()
        };
        let last = chunks.len() - 1;
        for (i, part) in chunks.iter().enumerate() {
            out.push(u8::from(i == last));
            let len = u16::try_from(part.len()).unwrap();
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(part);
        }
        out.extend_from_slice(&deflate::adler32(raw).to_be_bytes());
        out
    }

    /// Assemble a whole PNG from an IHDR, optional extra chunks, and raw
    /// (filtered) scanlines.
    fn png(ihdr_chunk: &[u8], extra: &[Vec<u8>], raw: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE);
        out.extend_from_slice(ihdr_chunk);
        for e in extra {
            out.extend_from_slice(e);
        }
        out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(raw)));
        out.extend_from_slice(&chunk(b"IEND", b""));
        out
    }

    #[test]
    fn a_two_by_two_truecolour_image_decodes_to_the_colours_it_stores() {
        // Filter 0 (None) on both rows: the simplest complete PNG there is.
        let raw = vec![
            0, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, // red, green
            0, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, // blue, white
        ];
        let file = png(&ihdr(2, 2, 8, 2, 0), &[], &raw);
        let img = decode(&file, Limits::default()).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(
            img.pixels,
            vec![0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF, 0xFFFF_FFFF]
        );
    }

    #[test]
    fn rgba_keeps_its_alpha_straight_rather_than_premultiplying_it() {
        // The compositor's `blend_pixel` multiplies by alpha itself; handing it
        // premultiplied pixels would multiply twice and darken every edge.
        let raw = vec![0, 0x80, 0x40, 0x20, 0x80];
        let file = png(&ihdr(1, 1, 8, 6, 0), &[], &raw);
        let img = decode(&file, Limits::default()).unwrap();
        assert_eq!(img.pixels, vec![0x8080_4020]);
    }

    #[test]
    fn a_greyscale_pixel_fills_all_three_colour_channels() {
        let raw = vec![0, 0x40];
        let file = png(&ihdr(1, 1, 8, 0, 0), &[], &raw);
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0xFF40_4040]
        );
    }

    #[test]
    fn one_bit_white_is_255_and_not_128() {
        // Scaling by multiplication rather than shifting. A shift would make
        // the white of a monochrome icon a mid-grey.
        let raw = vec![0, 0b1000_0000];
        let file = png(&ihdr(1, 1, 1, 0, 0), &[], &raw);
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0xFFFF_FFFF]
        );
    }

    #[test]
    fn sub_byte_samples_are_packed_most_significant_first() {
        // Four two-bit greys in one byte: 0, 1, 2, 3 -> 0, 85, 170, 255.
        let raw = vec![0, 0b00_01_10_11];
        let file = png(&ihdr(4, 1, 2, 0, 0), &[], &raw);
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0xFF00_0000, 0xFF55_5555, 0xFFAA_AAAA, 0xFFFF_FFFF]
        );
    }

    #[test]
    fn sixteen_bit_samples_are_taken_from_the_high_byte() {
        let raw = vec![0, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let file = png(&ihdr(1, 1, 16, 2, 0), &[], &raw);
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0xFF12_569A]
        );
    }

    #[test]
    fn a_palette_image_looks_its_colours_up() {
        let plte = chunk(b"PLTE", &[0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00]);
        let raw = vec![0, 0b0100_0000]; // indices 1, 0 at two bits each
        let file = png(&ihdr(2, 1, 2, 3, 0), &[plte], &raw);
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0xFF00_FF00, 0xFFFF_0000]
        );
    }

    #[test]
    fn a_palette_image_with_no_palette_is_an_error_and_not_a_black_picture() {
        let raw = vec![0, 0x00];
        let file = png(&ihdr(1, 1, 8, 3, 0), &[], &raw);
        assert_eq!(
            decode(&file, Limits::default()),
            Err(ImageError::Malformed("indexed image with no PLTE"))
        );
    }

    #[test]
    fn an_index_past_the_palette_is_named_rather_than_rendered() {
        let plte = chunk(b"PLTE", &[0xFF, 0x00, 0x00]);
        let raw = vec![0, 0x05];
        let file = png(&ihdr(1, 1, 8, 3, 0), &[plte], &raw);
        assert_eq!(
            decode(&file, Limits::default()),
            Err(ImageError::Malformed("palette index past PLTE"))
        );
    }

    #[test]
    fn trns_makes_one_palette_entry_transparent() {
        let plte = chunk(b"PLTE", &[0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00]);
        let trns = chunk(b"tRNS", &[0x00]); // entry 0 fully transparent
        let raw = vec![0, 0x00, 0x01];
        let file = png(&ihdr(2, 1, 8, 3, 0), &[plte, trns], &raw);
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0x00FF_0000, 0xFF00_FF00]
        );
    }

    #[test]
    fn trns_colour_keying_matches_at_the_original_depth() {
        // A truecolour tRNS names one exact colour as transparent. The
        // comparison must be against the raw samples: at 16 bits, two colours
        // that scale to the same byte are different colours.
        let trns = chunk(b"tRNS", &[0, 0xFF, 0, 0x00, 0, 0x00]);
        let raw = vec![0, 0xFF, 0x00, 0x00, 0xFE, 0x00, 0x00];
        let file = png(&ihdr(2, 1, 8, 2, 0), &[trns], &raw);
        let px = decode(&file, Limits::default()).unwrap().pixels;
        assert_eq!(px[0] >> 24, 0, "the keyed colour is transparent");
        assert_eq!(px[1] >> 24, 255, "one off the key is not");
    }

    #[test]
    fn every_filter_reconstructs_the_same_flat_image() {
        // A flat mid-grey encodes to the same picture under all five filters,
        // which makes their reconstructions directly comparable — and a filter
        // implemented wrongly stands out as a gradient.
        //
        // The filtered bytes are produced here by the *forward* formulas of
        // RFC 2083 §6, written out longhand. Deriving them by inverting the
        // decoder's own arithmetic would give a test that agrees with the code
        // by construction — including wherever both are wrong.
        const V: u8 = 0x77;
        for filter in 0..=4u8 {
            let mut raw = Vec::new();
            for y in 0..4usize {
                raw.push(filter);
                for x in 0..4usize {
                    // One byte per pixel, so "the byte to the left" is the
                    // pixel to the left, and neighbours off the image are zero
                    // — which is why row 0 and column 0 carry the real value
                    // under the filters that predict from them.
                    let left = if x > 0 { V } else { 0 };
                    let above = if y > 0 { V } else { 0 };
                    let upper_left = if x > 0 && y > 0 { V } else { 0 };
                    let predicted = match filter {
                        1 => left,
                        2 => above,
                        3 => {
                            u8::try_from(u16::midpoint(u16::from(left), u16::from(above))).unwrap()
                        }
                        4 => paeth(left, above, upper_left),
                        _ => 0,
                    };
                    raw.push(V.wrapping_sub(predicted));
                }
            }
            let file = png(&ihdr(4, 4, 8, 0, 0), &[], &raw);
            let img = decode(&file, Limits::default()).unwrap();
            assert!(
                img.pixels.iter().all(|&p| p == 0xFF77_7777),
                "filter {filter} did not reconstruct a flat field: {:08X?}",
                &img.pixels[..4]
            );
        }
    }

    #[test]
    fn paeth_breaks_ties_towards_the_left_neighbour() {
        // Normative (RFC 2083 §6.6). A decoder that broke ties the other way
        // would be right almost everywhere, which is the hardest wrong to see.
        assert_eq!(paeth(10, 10, 10), 10);
        assert_eq!(paeth(1, 2, 3), 1, "p = 0, all distances equal-ish");
        assert_eq!(paeth(0, 255, 0), 255);
    }

    #[test]
    fn an_interlaced_image_lands_every_pass_in_the_right_place() {
        // 8x8 greyscale, where each pixel's value is its own index. Adam7
        // scatters those across seven passes; getting a single step wrong
        // transposes part of the picture and nothing else.
        let w = 8u32;
        let h = 8u32;
        let value = |x: u32, y: u32| -> u8 { u8::try_from(y * w + x).unwrap() };

        let mut raw = Vec::new();
        for &(xs, ys, xstep, ystep) in &ADAM7 {
            let (pw, ph) = pass_size(w, h, xs, ys, xstep, ystep);
            if pw == 0 || ph == 0 {
                continue;
            }
            for py in 0..ph {
                raw.push(0); // filter None
                for px in 0..pw {
                    raw.push(value(xs + px * xstep, ys + py * ystep));
                }
            }
        }

        let file = png(&ihdr(w, h, 8, 0, 1), &[], &raw);
        let img = decode(&file, Limits::default()).unwrap();
        for y in 0..h {
            for x in 0..w {
                let g = u32::from(value(x, y));
                assert_eq!(
                    img.pixels[(y * w + x) as usize],
                    0xFF00_0000 | (g << 16) | (g << 8) | g,
                    "pixel ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn a_narrow_interlaced_image_skips_the_passes_it_has_no_pixels_for() {
        // 1x1 interlaced: only pass 1 has anything in it. A decoder that
        // counted a filter byte for the six empty passes would demand six bytes
        // that are not there -- the classic Adam7 off-by-N, invisible on any
        // image eight pixels or wider.
        let raw = vec![0, 0x42];
        let file = png(&ihdr(1, 1, 8, 0, 1), &[], &raw);
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0xFF42_4242]
        );
    }

    #[test]
    fn idat_split_across_chunks_is_one_stream_and_not_several() {
        // RFC 2083 lets an encoder chop IDAT anywhere, including mid-zlib-
        // header. Decompressing chunk by chunk fails on the very first one.
        let raw = vec![0, 0xFF, 0x00, 0x00];
        let stream = zlib_stored(&raw);
        let (a, b) = stream.split_at(1);
        let mut file = Vec::new();
        file.extend_from_slice(&SIGNATURE);
        file.extend_from_slice(&ihdr(1, 1, 8, 2, 0));
        file.extend_from_slice(&chunk(b"IDAT", a));
        file.extend_from_slice(&chunk(b"IDAT", b));
        file.extend_from_slice(&chunk(b"IEND", b""));
        assert_eq!(
            decode(&file, Limits::default()).unwrap().pixels,
            vec![0xFFFF_0000]
        );
    }

    #[test]
    fn a_corrupt_critical_chunk_is_fatal_and_a_corrupt_comment_is_not() {
        let raw = vec![0, 0xFF, 0x00, 0x00];

        // A tEXt whose CRC is wrong: skipped, picture still decodes.
        let mut bad_text = chunk(b"tEXt", b"Comment\0hello");
        let last = bad_text.len() - 1;
        bad_text[last] ^= 0xFF;
        let mut file = Vec::new();
        file.extend_from_slice(&SIGNATURE);
        file.extend_from_slice(&ihdr(1, 1, 8, 2, 0));
        file.extend_from_slice(&bad_text);
        file.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&raw)));
        file.extend_from_slice(&chunk(b"IEND", b""));
        assert!(decode(&file, Limits::default()).is_ok());

        // The same damage to IDAT is not survivable: the pixels would be
        // confidently wrong rather than obviously wrong.
        let mut idat = chunk(b"IDAT", &zlib_stored(&raw));
        let last = idat.len() - 1;
        idat[last] ^= 0xFF;
        let mut file = Vec::new();
        file.extend_from_slice(&SIGNATURE);
        file.extend_from_slice(&ihdr(1, 1, 8, 2, 0));
        file.extend_from_slice(&idat);
        file.extend_from_slice(&chunk(b"IEND", b""));
        assert_eq!(
            decode(&file, Limits::default()),
            Err(ImageError::Corrupt("critical chunk CRC"))
        );
    }

    #[test]
    fn a_header_that_declares_more_pixels_than_allowed_is_refused_before_allocating() {
        // Eight bytes of header would otherwise ask for seventeen gigabytes.
        let file = png(&ihdr(65535, 65535, 8, 2, 0), &[], &[]);
        assert_eq!(
            decode(&file, Limits::default()),
            Err(ImageError::TooLarge {
                pixels: 65535 * 65535,
                limit: Limits::DEFAULT_MAX_PIXELS,
            })
        );
    }

    #[test]
    fn a_callers_smaller_limit_is_the_one_that_applies() {
        // An icon cache that only ever wants 64x64 should get its refusal from
        // the header rather than from a hundred-megabyte buffer.
        let raw = vec![0, 0xFF, 0x00, 0x00];
        let file = png(&ihdr(1, 1, 8, 2, 0), &[], &raw);
        let tiny = Limits {
            max_pixels: 0,
            ..Limits::default()
        };
        assert!(matches!(
            decode(&file, tiny),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_stream_that_expands_past_the_headers_own_size_is_refused() {
        // The zip-bomb case, in its PNG-specific form: the header says one
        // pixel, the stream produces a megabyte.
        let raw = vec![0u8; 4096];
        let file = png(&ihdr(1, 1, 8, 2, 0), &[], &raw);
        assert!(matches!(
            decode(&file, Limits::default()),
            Err(ImageError::Compressed(_))
        ));
    }

    #[test]
    fn a_stream_shorter_than_the_header_promises_is_truncated_not_padded() {
        // Padding would produce a picture with a band of black at the bottom
        // and no indication that anything was wrong.
        let raw = vec![0, 0xFF, 0x00];
        let file = png(&ihdr(1, 1, 8, 2, 0), &[], &raw);
        assert_eq!(decode(&file, Limits::default()), Err(ImageError::Truncated));
    }

    #[test]
    fn a_file_with_no_iend_is_truncated_even_if_every_pixel_arrived() {
        let raw = vec![0, 0xFF, 0x00, 0x00];
        let mut file = Vec::new();
        file.extend_from_slice(&SIGNATURE);
        file.extend_from_slice(&ihdr(1, 1, 8, 2, 0));
        file.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&raw)));
        assert_eq!(decode(&file, Limits::default()), Err(ImageError::Truncated));
    }

    #[test]
    fn every_impossible_header_field_is_named() {
        let raw = vec![0, 0x00];
        for (ihdr_chunk, expected) in [
            (ihdr(0, 1, 8, 0, 0), "IHDR zero dimension"),
            (ihdr(1, 0, 8, 0, 0), "IHDR zero dimension"),
            (ihdr(1, 1, 8, 7, 0), "IHDR colour type"),
            (ihdr(1, 1, 3, 0, 0), "IHDR bit depth for this colour type"),
            (ihdr(1, 1, 16, 3, 0), "IHDR bit depth for this colour type"),
            (ihdr(1, 1, 1, 2, 0), "IHDR bit depth for this colour type"),
            (ihdr(1, 1, 8, 0, 2), "IHDR interlace method"),
        ] {
            let file = png(&ihdr_chunk, &[], &raw);
            assert_eq!(
                decode(&file, Limits::default()),
                Err(ImageError::Malformed(expected)),
                "expected {expected}"
            );
        }
    }

    #[test]
    fn dimensions_reads_the_header_and_nothing_else() {
        // Deliberately a file with no IDAT at all: the point is that the file
        // manager's detail column costs a header parse, not a decode.
        let mut file = Vec::new();
        file.extend_from_slice(&SIGNATURE);
        file.extend_from_slice(&ihdr(640, 480, 8, 6, 0));
        assert_eq!(dimensions(&file).unwrap(), (640, 480));
    }

    #[test]
    fn a_file_that_is_not_a_png_is_refused_by_its_signature() {
        assert!(!is_png(b"\x89PNG"));
        assert!(!is_png(b"\x89PNG\r\n\x1a\x0b"));
        assert!(is_png(&SIGNATURE));
        assert_eq!(
            dimensions(b"not a png at all"),
            Err(ImageError::UnknownFormat)
        );
    }

    #[test]
    fn every_byte_of_a_valid_png_can_be_corrupted_without_a_panic() {
        // The property the whole crate exists to keep. A wallpaper is a file
        // the user was handed; nothing here may panic, whatever the bytes are.
        let raw = vec![
            0, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, //
            2, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        let good = png(&ihdr(2, 2, 8, 2, 0), &[], &raw);
        for i in 0..good.len() {
            for bit in 0..8u32 {
                let mut bad = good.clone();
                bad[i] ^= 1u8 << bit;
                let _ = decode(&bad, Limits::default());
                let _ = dimensions(&bad);
            }
        }
    }

    #[test]
    fn a_truncation_at_every_length_is_an_error_and_never_a_panic() {
        let raw = vec![0, 0xFF, 0x00, 0x00];
        let good = png(&ihdr(1, 1, 8, 2, 0), &[], &raw);
        for n in 0..good.len() {
            let _ = decode(&good[..n], Limits::default());
        }
    }

    #[test]
    fn a_chunk_length_that_overflows_the_file_is_truncated_not_indexed() {
        // A four-byte length field is the cheapest lie in the format.
        let mut file = Vec::new();
        file.extend_from_slice(&SIGNATURE);
        file.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        file.extend_from_slice(b"IDAT");
        file.extend_from_slice(&[0, 0, 0, 0]);
        assert!(decode(&file, Limits::default()).is_err());
    }

    #[test]
    fn the_crc_matches_the_reference_value_for_a_known_chunk() {
        // An empty IEND's CRC is a constant every PNG in the world contains,
        // which makes it the one value that can be checked against the world
        // rather than against this implementation.
        assert_eq!(crc32(b"IEND", b""), 0xAE42_6082);
    }
}
