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
//! # A row at a time
//!
//! The decompressed stream is every row of the picture plus a filter byte each
//! — as large as the picture itself — and it is never held whole. Rows are
//! pulled out of the decompressor as they are reconstructed (`Scanlines`),
//! so a decode holds its output, two rows and the decompressor's 32 KiB
//! window. That is what makes a thumbnail of any picture cost a thumbnail:
//! [`decode_scaled`] accumulates rows straight into a destination-sized box
//! filter, interlaced files included, and never holds the source at its own
//! size in any form.
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

use deflate::{ZlibInflateStream, zlib_inflate_stream};

use crate::orientation::Orientation;
use crate::scale::{BoxFilter, Sink, fit_within};
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
    Ok(orientation(bytes).shown((h.width, h.height)))
}

/// Which way up the picture is shown: its EXIF orientation, from the first
/// `eXIf` chunk before the image data, as Chrome reads it (see
/// [`crate::orientation`]). As stored if there is none, or none that counts.
#[must_use]
pub fn orientation(bytes: &[u8]) -> Orientation {
    exif(bytes)
        .and_then(crate::orientation::from_exif)
        .unwrap_or_default()
}

/// The first `eXIf` chunk's contents before `IDAT`. One whose CRC is wrong is
/// passed over, as every ancillary chunk is.
fn exif(bytes: &[u8]) -> Option<&[u8]> {
    if !is_png(bytes) {
        return None;
    }
    for chunk in Chunks::new(bytes).skip(1) {
        let chunk = chunk.ok()?;
        match &chunk.kind {
            b"IDAT" | b"IEND" => return None,
            b"eXIf" => return Some(chunk.data),
            _ => {}
        }
    }
    None
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

/// Decode a picture no larger than `max_w` x `max_h`, box-filtered on the way
/// out.
///
/// For a thumbnailer: the destination is tiny and the source may be enormous.
/// Rows go from the decompressor straight into the box filter, so the source
/// is never held at its own size — not as pixels, and not as decompressed
/// scanlines. Interlaced files take the same path: a box filter adds each
/// source pixel to its cell whatever order the pixels come in, so Adam7's
/// scattered passes accumulate exactly as a plain file's rows do.
///
/// The aspect ratio is preserved and the result never exceeds either bound.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_scaled(bytes: &[u8], limits: Limits, max_w: u32, max_h: u32) -> ImageResult<Image> {
    // The box is for the picture as shown: turned first for one shown on its
    // side.
    let turn = orientation(bytes);
    let (max_w, max_h) = turn.shown((max_w.max(1), max_h.max(1)));
    Ok(turn.apply(decode_inner(bytes, limits, Some((max_w, max_h)))?))
}

/// Decode a PNG into `0xAARRGGBB` pixels, turned as its EXIF orientation
/// ([`orientation`]) says.
///
/// # Errors
///
/// [`ImageError`] — a malformed header, a picture over `limits`, a failed
/// critical-chunk checksum, or unreadable compressed data. Never panics, for
/// any input.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    Ok(orientation(bytes).apply(decode_inner(bytes, limits, None)?))
}

fn decode_inner(bytes: &[u8], limits: Limits, scale_to: Option<(u32, u32)>) -> ImageResult<Image> {
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
    let mut idat_chunks: Vec<&[u8]> = Vec::new();
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
                idat_chunks.push(chunk.data);
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

    // One chunk is borrowed as it stands; only a stream split across several
    // is copied to join it up.
    let joined: Vec<u8>;
    let idat: &[u8] = if let [only] = idat_chunks.as_slice() {
        only
    } else {
        joined = idat_chunks.concat();
        &joined
    };

    // (3) and (4): the exact size, used as the hard limit.
    let raw_size = raw_size(&header);
    let limit = usize::try_from(raw_size)
        .ok()
        .filter(|&n| n <= limits.max_decompressed_bytes)
        .ok_or(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        })?;
    let mut rows = Scanlines::new(idat, limit)?;

    if let Some((max_w, max_h)) = scale_to {
        let (dw, dh) = fit_within(header.width, header.height, max_w, max_h);
        let mut sink = BoxFilter::new(header.width, header.height, dw, dh)?;
        expand(&header, &mut rows, &palette, trns.as_deref(), &mut sink)?;
        rows.finish()?;
        return Ok(Image {
            width: dw,
            height: dh,
            pixels: sink.finish(),
        });
    }

    let mut sink = FullSize::new(header.width, header.height)?;
    expand(&header, &mut rows, &palette, trns.as_deref(), &mut sink)?;
    rows.finish()?;
    Ok(Image {
        width: header.width,
        height: header.height,
        pixels: sink.pixels,
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

/// Filtered scanlines, pulled out of the decompressor a row at a time.
///
/// The reason a decode costs one picture's worth of memory and not two: the
/// decompressed stream — every row and its filter byte, as large as the
/// picture, 72 MB for a 24-megapixel RGB photograph — never exists all at
/// once. The inflater keeps its 32 KiB window and whatever of the current block
/// has not been read yet; the reconstruction keeps two rows.
struct Scanlines<'a> {
    stream: ZlibInflateStream<'a>,
}

impl<'a> Scanlines<'a> {
    /// A reader over `idat`, which must decompress to exactly `limit` bytes.
    fn new(idat: &'a [u8], limit: usize) -> ImageResult<Self> {
        Ok(Self {
            stream: zlib_inflate_stream(idat, limit)?,
        })
    }

    /// The next row's filter type, with its filtered bytes read into `row`.
    fn next_row(&mut self, row: &mut [u8]) -> ImageResult<u8> {
        let mut filter = [0u8; 1];
        self.fill(&mut filter)?;
        self.fill(row)?;
        Ok(u8::from_le_bytes(filter))
    }

    /// Fill `buf` completely, or say the stream ended first: a short file.
    fn fill(&mut self, buf: &mut [u8]) -> ImageResult<()> {
        let mut filled = 0usize;
        while filled < buf.len() {
            let rest = buf.get_mut(filled..).unwrap_or_default();
            let got = self.stream.read(rest)?;
            if got == 0 {
                return Err(ImageError::Truncated);
            }
            filled = filled.saturating_add(got);
        }
        Ok(())
    }

    /// Read the stream to its end, which is where its checksum is checked.
    ///
    /// Every row the header promised has been read by now, so the stream's
    /// limit is spent: a byte still to come is refused by the decompressor as
    /// one too many, exactly as the whole-buffer inflate refused it. Skipping
    /// this would accept a stream whose checksum is wrong — the one check that
    /// tells a corrupted picture from a picture.
    fn finish(mut self) -> ImageResult<()> {
        let mut past_the_end = [0u8; 1];
        match self.stream.read(&mut past_the_end)? {
            0 => Ok(()),
            // The limit makes this unreachable — the decompressor errors
            // rather than hand out a byte past it — but a rule that holds only
            // because a neighbour enforces it is stated here as well.
            _ => Err(ImageError::Compressed(deflate::Error::OutputTooLarge)),
        }
    }
}

/// The whole picture, at its own size.
struct FullSize {
    width: u32,
    pixels: Vec<u32>,
}

impl FullSize {
    fn new(width: u32, height: u32) -> ImageResult<Self> {
        let len = (width as usize)
            .checked_mul(height as usize)
            .ok_or(ImageError::Truncated)?;
        Ok(Self {
            width,
            pixels: vec![0u32; len],
        })
    }
}

impl Sink for FullSize {
    fn put(&mut self, x: u32, y: u32, argb: u32) {
        let at = (y as usize)
            .checked_mul(self.width as usize)
            .and_then(|row| row.checked_add(x as usize));
        if let Some(slot) = at.and_then(|at| self.pixels.get_mut(at)) {
            *slot = argb;
        }
    }
}

/// Reconstruct the whole picture into `sink`: one pass for a plain file, the
/// seven Adam7 passes in order for an interlaced one. Each pass is an
/// independently filtered image of its own size, whose pixels land at
/// `(x_start + x * x_step, y_start + y * y_step)`.
fn expand(
    h: &Header,
    rows: &mut Scanlines<'_>,
    palette: &[u32],
    trns: Option<&[u8]>,
    sink: &mut impl Sink,
) -> ImageResult<()> {
    if !h.interlaced {
        return expand_pass(
            h,
            rows,
            palette,
            trns,
            sink,
            (0, 0, 1, 1),
            h.width,
            h.height,
        );
    }
    for &(xs, ys, xstep, ystep) in &ADAM7 {
        let (pw, ph) = pass_size(h.width, h.height, xs, ys, xstep, ystep);
        if pw == 0 || ph == 0 {
            // A pass with no pixels has no bytes in the stream at all, not
            // even filter bytes — see `raw_size`.
            continue;
        }
        expand_pass(h, rows, palette, trns, sink, (xs, ys, xstep, ystep), pw, ph)?;
    }
    Ok(())
}

/// Unfilter one pass, row by row as the stream delivers it, and hand each pixel
/// to `sink` at its place in the picture.
///
/// `placement` is `(x_start, y_start, x_step, y_step)`; for a non-interlaced
/// image it is `(0, 0, 1, 1)`, which is why there is no second copy of this
/// function for the simple case.
#[allow(clippy::too_many_arguments)]
fn expand_pass(
    h: &Header,
    rows: &mut Scanlines<'_>,
    palette: &[u32],
    trns: Option<&[u8]>,
    sink: &mut impl Sink,
    placement: (u32, u32, u32, u32),
    pass_w: u32,
    pass_h: u32,
) -> ImageResult<()> {
    let (xs, ys, xstep, ystep) = placement;
    let row_len = usize::try_from(h.row_bytes(pass_w)).map_err(|_| ImageError::Truncated)?;
    let step = h.filter_step();

    // Two rows kept, because Up/Average/Paeth all read the *reconstructed*
    // previous row and nothing further back.
    let mut prev = vec![0u8; row_len];
    let mut cur = vec![0u8; row_len];

    for y in 0..pass_h {
        let filter = rows.next_row(&mut cur)?;
        unfilter(filter, &mut cur, &prev, step)?;

        let out_y = ys.saturating_add(y.saturating_mul(ystep));
        for x in 0..pass_w {
            let out_x = xs.saturating_add(x.saturating_mul(xstep));
            let px = pixel_at(h, &cur, x as usize, palette, trns)?;
            sink.put(out_x, out_y, px);
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
    // The length was checked to be a whole number of triples, so the
    // remainder `as_chunks` hands back is empty.
    let (triples, _) = data.as_chunks::<3>();
    Ok(triples
        .iter()
        .map(|&[r, g, b]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
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

    // ------------------------------------------------------------------
    // Scaled decode
    // ------------------------------------------------------------------

    /// A scaled decode is the same picture, at a smaller size.
    ///
    /// The property that matters: it must agree with decoding and *then*
    /// averaging, or the file manager's previews would differ depending on
    /// which path a picture happened to take. Compared to a box filter written
    /// here rather than trusted, so the test says what the answer should be
    /// instead of asking the code.
    #[test]
    fn a_scaled_decode_averages_the_same_way_a_full_one_would() {
        let png =
            crate::testing::png_rgba(8, 8, |x, y| [(x * 32) as u8, (y * 32) as u8, 0x40, 0xFF]);
        let scaled =
            decode_scaled(&png, Limits::default(), 4, 4).expect("the scaled decode must work");
        assert_eq!((scaled.width, scaled.height), (4, 4));

        let full = decode(&png, Limits::default()).expect("full");
        for dy in 0..4u32 {
            for dx in 0..4u32 {
                // Each destination cell covers exactly 2x2 here.
                let mut sums = [0u32; 4];
                for sy in 0..2u32 {
                    for sx in 0..2u32 {
                        let px = full.pixels[((dy * 2 + sy) * 8 + dx * 2 + sx) as usize];
                        sums[0] += (px >> 24) & 0xFF;
                        sums[1] += (px >> 16) & 0xFF;
                        sums[2] += (px >> 8) & 0xFF;
                        sums[3] += px & 0xFF;
                    }
                }
                let want = ((sums[0] / 4) << 24)
                    | ((sums[1] / 4) << 16)
                    | ((sums[2] / 4) << 8)
                    | (sums[3] / 4);
                assert_eq!(
                    scaled.pixels[(dy * 4 + dx) as usize],
                    want,
                    "cell ({dx}, {dy})"
                );
            }
        }
    }

    /// A picture smaller than the bounds comes back untouched.
    ///
    /// Scaling *up* would invent pixels, which is not what a thumbnailer is
    /// for and would make a 16x16 icon a blurry 128x128 one.
    #[test]
    fn a_small_picture_is_not_enlarged() {
        let png = crate::testing::png_gradient(6, 4);
        let scaled = decode_scaled(&png, Limits::default(), 128, 128).expect("scaled");
        let full = decode(&png, Limits::default()).expect("full");

        assert_eq!((scaled.width, scaled.height), (6, 4));
        assert_eq!(scaled.pixels, full.pixels, "and not resampled either");
    }

    /// The aspect ratio survives, and neither bound is exceeded.
    #[test]
    fn a_scaled_decode_keeps_the_shape_and_stays_inside_both_bounds() {
        let png = crate::testing::png_gradient(40, 10);
        let scaled = decode_scaled(&png, Limits::default(), 8, 8).expect("scaled");

        assert!(scaled.width <= 8 && scaled.height <= 8, "inside the box");
        assert_eq!(scaled.width, 8, "the wide side binds first");
        assert_eq!(scaled.height, 2, "and the other follows the ratio");
        assert_eq!(scaled.pixels.len(), 16);
    }

    /// A destination dimension never rounds to zero.
    ///
    /// A 4000x3 panorama scaled to fit 64x64 has a height of 0.048 rows; a
    /// zero there would produce an empty picture, and a caller asking for a
    /// preview would get one with no pixels rather than a thin one.
    #[test]
    fn an_extreme_ratio_still_produces_at_least_one_row() {
        let png = crate::testing::png_gradient(400, 3);
        let scaled = decode_scaled(&png, Limits::default(), 64, 64).expect("scaled");

        assert!(scaled.height >= 1, "height rounded away to nothing");
        assert_eq!(
            scaled.pixels.len(),
            (scaled.width as usize) * (scaled.height as usize)
        );
    }

    /// Every destination cell is covered: none is left at the default zero.
    ///
    /// The failure this catches is an off-by-one in the index arithmetic
    /// leaving the last row or column untouched, which on a dark picture is
    /// almost invisible by eye.
    #[test]
    fn every_destination_cell_receives_at_least_one_source_pixel() {
        let png = crate::testing::png_rgba(37, 23, |_, _| [0x10, 0x20, 0x30, 0xFF]);
        let scaled = decode_scaled(&png, Limits::default(), 9, 5).expect("scaled");

        for (i, px) in scaled.pixels.iter().enumerate() {
            assert_eq!(
                *px, 0xFF10_2030,
                "cell {i} is not the uniform colour, so it took no source pixels"
            );
        }
    }

    // ------------------------------------------------------------------
    // A row at a time
    // ------------------------------------------------------------------

    /// The filtered scanlines of an 8-bit greyscale picture, in Adam7 pass
    /// order, each row behind a filter byte of 0.
    fn adam7_gray(w: u32, h: u32, value: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut raw = Vec::new();
        for &(xs, ys, xstep, ystep) in &ADAM7 {
            let (pw, ph) = pass_size(w, h, xs, ys, xstep, ystep);
            if pw == 0 || ph == 0 {
                continue;
            }
            for py in 0..ph {
                raw.push(0);
                for px in 0..pw {
                    raw.push(value(xs + px * xstep, ys + py * ystep));
                }
            }
        }
        raw
    }

    /// An interlaced picture scales like any other: its scattered passes
    /// accumulate into the same cells, and the thumbnail is the average of
    /// the full decode. It used to fall back to decoding at full size first.
    #[test]
    fn an_interlaced_picture_scales_to_the_average_of_its_full_decode() {
        let (w, h) = (16u32, 16u32);
        let value = |x: u32, y: u32| ((x * 13 + y * 7) % 256) as u8;
        let file = png(&ihdr(w, h, 8, 0, 1), &[], &adam7_gray(w, h, value));
        let full = decode(&file, Limits::default()).expect("full");
        // The full decode is the picture, pixel for pixel.
        for y in 0..h {
            for x in 0..w {
                let g = u32::from(value(x, y));
                assert_eq!(
                    full.pixels[(y * w + x) as usize],
                    0xFF00_0000 | g << 16 | g << 8 | g
                );
            }
        }
        let scaled = decode_scaled(&file, Limits::default(), 4, 4).expect("scaled");
        assert_eq!((scaled.width, scaled.height), (4, 4));
        for dy in 0..4u32 {
            for dx in 0..4u32 {
                // Each cell covers exactly 4x4 here.
                let mut sum = 0u32;
                for sy in 0..4u32 {
                    for sx in 0..4u32 {
                        sum += u32::from(value(dx * 4 + sx, dy * 4 + sy));
                    }
                }
                let g = sum / 16;
                assert_eq!(
                    scaled.pixels[(dy * 4 + dx) as usize],
                    0xFF00_0000 | g << 16 | g << 8 | g,
                    "cell ({dx}, {dy})"
                );
            }
        }
    }

    /// The checksum is still checked. Reading rows as they arrive means the
    /// stream is never read to its end unless something asks, and the
    /// Adler-32 trailer is verified only there.
    #[test]
    fn a_stream_whose_checksum_is_wrong_is_refused() {
        let raw = vec![0, 0xFF, 0x00, 0x00];
        let mut zlib = zlib_stored(&raw);
        let last = zlib.len() - 1;
        zlib[last] ^= 0x01;
        let mut file = Vec::new();
        file.extend_from_slice(&SIGNATURE);
        file.extend_from_slice(&ihdr(1, 1, 8, 2, 0));
        file.extend_from_slice(&chunk(b"IDAT", &zlib));
        file.extend_from_slice(&chunk(b"IEND", b""));
        assert!(matches!(
            decode(&file, Limits::default()),
            Err(ImageError::Compressed(
                deflate::Error::ChecksumMismatch { .. }
            ))
        ));
        assert!(matches!(
            decode_scaled(&file, Limits::default(), 1, 1),
            Err(ImageError::Compressed(
                deflate::Error::ChecksumMismatch { .. }
            ))
        ));
    }

    /// A stream longer than the header promises is refused, not cut to fit:
    /// the extra bytes mean the file is not what its header says.
    #[test]
    fn a_stream_longer_than_the_header_promises_is_refused() {
        let raw = vec![0, 0xFF, 0x00, 0x00, 0x42];
        let file = png(&ihdr(1, 1, 8, 2, 0), &[], &raw);
        assert!(matches!(
            decode(&file, Limits::default()),
            Err(ImageError::Compressed(deflate::Error::OutputTooLarge))
        ));
    }

    /// A stream split across several IDAT chunks is one stream: the chunks are
    /// joined before it is read, and the picture is the same as from one chunk.
    #[test]
    fn a_stream_split_across_chunks_decodes_as_one() {
        let raw: Vec<u8> = (0..4u8)
            .flat_map(|row| {
                let mut r = vec![0u8];
                r.extend((0..4u8).flat_map(|col| [row * 40, col * 40, 0x80]));
                r
            })
            .collect();
        let whole = png(&ihdr(4, 4, 8, 2, 0), &[], &raw);
        let zlib = zlib_stored(&raw);
        let mut split = Vec::new();
        split.extend_from_slice(&SIGNATURE);
        split.extend_from_slice(&ihdr(4, 4, 8, 2, 0));
        for part in zlib.chunks(7) {
            split.extend_from_slice(&chunk(b"IDAT", part));
        }
        split.extend_from_slice(&chunk(b"IEND", b""));
        assert_eq!(
            decode(&split, Limits::default()),
            decode(&whole, Limits::default())
        );
    }
}
