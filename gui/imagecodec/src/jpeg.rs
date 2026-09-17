//! JPEG (JFIF), baseline sequential.
//!
//! The format this crate's own header called "the next thing this crate should
//! grow", and the one a photograph is almost always in. PNG is what a
//! screenshot or a diagram is; JPEG is what a camera writes, which is why the
//! photo manager, the image viewer, the wallpaper picker and the file
//! browser's thumbnails all stop at the same place without it.
//!
//! # What a JPEG is, in the order this reads it
//!
//! 1. **Segments.** A marker (`0xFF` then a kind), a two-byte length, and the
//!    payload. [`decode`] walks them.
//! 2. **Quantisation tables** (`DQT`): the divisors that threw away the detail
//!    the encoder judged invisible. Dequantising is multiplying them back.
//! 3. **Huffman tables** (`DHT`): four of them typically, DC and AC for luma
//!    and chroma.
//! 4. **The frame** (`SOF0`): size, and one entry per component saying how
//!    finely it was sampled. Chroma is usually sampled at half the luma rate
//!    in each direction, which is what "4:2:0" means and why an MCU is
//!    sixteen pixels across rather than eight.
//! 5. **The scan** (`SOS`): the entropy-coded blocks themselves, read as a bit
//!    stream with two pieces of awkwardness -- a `0xFF` byte in the data is
//!    written `0xFF 0x00` so it cannot be mistaken for a marker, and the
//!    stream may be cut at intervals by restart markers that reset the
//!    predictors.
//! 6. **Reconstruction**: dequantise, inverse-DCT each 8x8 block, upsample the
//!    chroma back to full resolution, and convert YCbCr to RGB.
//!
//! # What this does not do, and says so
//!
//! **Progressive JPEG** is refused by name. It is a different scan structure
//! -- the image arrives in successive approximations rather than block by
//! block -- and decoding its first scan would produce a recognisable but
//! wrong picture, which is worse than refusing: a thumbnail that is subtly
//! incorrect is one nobody checks. `ImageError::Unsupported` says which.
//!
//! **Arithmetic coding** and **12-bit samples** are likewise named rather than
//! half-read. Both are rare enough that no file on this machine uses them and
//! common enough in the specification to be worth refusing precisely.
//!
//! # Hostile input
//!
//! Every length in a JPEG is a claim, and the entropy stream is a claim about
//! itself. Nothing here allocates on a header's say-so beyond [`Limits`],
//! every table index is checked against the tables actually defined, the bit
//! reader cannot run past its buffer, and a Huffman code that matches nothing
//! ends the scan rather than looping. The same discipline `png` documents, for
//! the same reason: these bytes came from somewhere else.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Image, ImageError, ImageResult, Limits};

/// Zig-zag order: the sequence a block's 64 coefficients are stored in.
///
/// The encoder writes them from the lowest frequency outwards, so the zeros it
/// created cluster at the end and run-length coding can end a block early.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Whether `bytes` begins with a JPEG signature.
///
/// `FF D8` is the start-of-image marker, and the third byte is the start of
/// the next marker, which is always `FF`. Checking three rather than two
/// avoids claiming every file that happens to open with `FF D8`.
#[must_use]
pub fn is_jpeg(bytes: &[u8]) -> bool {
    matches!(bytes, [0xFF, 0xD8, 0xFF, ..])
}

/// One colour component of a frame.
#[derive(Debug, Clone, Copy)]
struct Component {
    /// The component's own id, which the scan header refers to it by.
    id: u8,
    /// Horizontal and vertical sampling factors, relative to the largest.
    h: usize,
    v: usize,
    /// Which quantisation table dequantises it.
    quant: usize,
    /// Which Huffman tables decode it, chosen per scan.
    dc_table: usize,
    ac_table: usize,
    /// The running DC predictor: a block stores its DC as a difference from
    /// the block before it, which is why a restart marker has to reset this.
    dc_prediction: i32,
}

/// A Huffman table, as a flat list the decoder can walk one bit at a time.
///
/// Stored as the count of codes at each length and the values in order, which
/// is exactly how `DHT` gives them: reconstructing canonical codes from that
/// is a few lines and needs no table of 65536 entries.
#[derive(Debug, Clone, Default)]
struct Huffman {
    /// `counts[n]` is how many codes have length `n + 1`.
    counts: [u8; 16],
    values: Vec<u8>,
}

impl Huffman {
    /// Read one value, or `None` if no code matches.
    // `code - first` is guarded by the `code >= first` test immediately
    // above it, which is the only subtraction here; everything else is
    // `checked_`.
    #[allow(clippy::arithmetic_side_effects, reason = "guarded by the bound above")]
    fn decode(&self, bits: &mut BitReader<'_>) -> Option<u8> {
        let mut code = 0u32;
        let mut index = 0usize;
        for length in 0..16usize {
            code = code.checked_mul(2)?.checked_add(u32::from(bits.bit()?))?;
            let count = usize::from(*self.counts.get(length)?);
            // The canonical code for this length starts where the previous
            // length's codes ended, shifted along.
            let first = self.first_code(length);
            if count > 0 && code >= first && code < first.checked_add(count as u32)? {
                let offset = index.checked_add((code - first) as usize)?;
                return self.values.get(offset).copied();
            }
            index = index.checked_add(count)?;
        }
        None
    }

    /// The numeric value of the first code of length `length + 1`.
    fn first_code(&self, length: usize) -> u32 {
        let mut code = 0u32;
        for n in 0..=length {
            if n > 0 {
                code = code.saturating_mul(2);
            }
            if n < length {
                code = code.saturating_add(u32::from(self.counts.get(n).copied().unwrap_or(0)));
            }
        }
        code
    }
}

/// The entropy-coded data, read one bit at a time.
///
/// Two pieces of awkwardness live here. A `0xFF` in the data is written
/// `0xFF 0x00`, so the zero is skipped; and any other `0xFF xx` is a marker,
/// which ends the run of data rather than being read as bits.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bits not yet handed out, most-significant first.
    buffer: u32,
    count: u32,
}

impl<'a> BitReader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buffer: 0,
            count: 0,
        }
    }

    /// One bit, or `None` at the end of the data or at a marker.
    fn bit(&mut self) -> Option<u8> {
        if self.count == 0 {
            let byte = *self.data.get(self.pos)?;
            self.pos = self.pos.saturating_add(1);
            if byte == 0xFF {
                match self.data.get(self.pos) {
                    // A stuffed zero: the 0xFF is data.
                    Some(0x00) => self.pos = self.pos.saturating_add(1),
                    // Anything else is a marker and the scan ends here.
                    _ => return None,
                }
            }
            self.buffer = u32::from(byte);
            self.count = 8;
        }
        self.count = self.count.saturating_sub(1);
        let bit = (self.buffer >> self.count) & 1;
        u8::try_from(bit).ok()
    }

    /// `n` bits as an unsigned number.
    fn bits(&mut self, n: u32) -> Option<i32> {
        let mut value = 0i32;
        for _ in 0..n {
            value = value.checked_mul(2)?.checked_add(i32::from(self.bit()?))?;
        }
        Some(value)
    }

    /// A signed coefficient, in JPEG's own representation.
    ///
    /// A value of `n` bits whose top bit is zero is negative, and is biased by
    /// `-(2^n - 1)`. This is the step that is easy to get subtly wrong and
    /// produces an image that is recognisable and wrong.
    fn receive_extend(&mut self, n: u32) -> Option<i32> {
        if n == 0 {
            return Some(0);
        }
        let value = self.bits(n)?;
        let threshold = 1i32.checked_shl(n.saturating_sub(1))?;
        if value < threshold {
            let bias = 1i32.checked_shl(n)?.checked_sub(1)?;
            value.checked_sub(bias)
        } else {
            Some(value)
        }
    }

    /// Step to the next byte boundary and past a restart marker, if one is
    /// there. Returns whether a restart was consumed.
    fn restart(&mut self) -> bool {
        self.count = 0;
        // A restart marker is `FF D0` through `FF D7`.
        while let Some(byte) = self.data.get(self.pos) {
            if *byte != 0xFF {
                return false;
            }
            match self.data.get(self.pos.saturating_add(1)) {
                Some(0xD0..=0xD7) => {
                    self.pos = self.pos.saturating_add(2);
                    return true;
                }
                Some(0xFF) => self.pos = self.pos.saturating_add(1),
                _ => return false,
            }
        }
        false
    }
}

/// The 8x8 cosine basis, computed once.
///
/// `BASIS[u][x]` is `C(u) * cos((2x + 1) * u * pi / 16)`, which is every
/// cosine an 8-point inverse DCT needs. The first version of this called a
/// cosine per coefficient per block: sixty-four series evaluations for every
/// eight pixels of every component, which decoded a 256x192 thumbnail slowly
/// enough to be measured in seconds. The arithmetic below is the same
/// arithmetic; only the cosines moved.
struct Basis {
    table: [[f32; 8]; 8],
}

impl Basis {
    // `x` and `u` are both `0..8`, so every product below is under 120
    // and the angle is a bounded float. This runs once per decode.
    #[allow(clippy::arithmetic_side_effects, reason = "0..8 by construction")]
    fn new() -> Self {
        let mut table = [[0.0f32; 8]; 8];
        for u in 0..8usize {
            let c = if u == 0 {
                core::f32::consts::FRAC_1_SQRT_2
            } else {
                1.0
            };
            for x in 0..8usize {
                #[allow(clippy::cast_precision_loss, reason = "0..8")]
                let angle = ((2 * x + 1) as f32) * (u as f32) * core::f32::consts::PI / 16.0;
                if let Some(row) = table.get_mut(u) {
                    if let Some(cell) = row.get_mut(x) {
                        *cell = c * cosine(angle);
                    }
                }
            }
        }
        Self { table }
    }

    fn at(&self, u: usize, x: usize) -> f32 {
        self.table
            .get(u)
            .and_then(|row| row.get(x))
            .copied()
            .unwrap_or(0.0)
    }
}

/// The inverse discrete cosine transform, 8x8, separable.
///
/// Rows then columns, which is 16 eight-point transforms rather than the 4096
/// multiply-accumulates the two-dimensional definition asks for. Written for
/// clarity rather than speed beyond that: the fast integer approximations
/// trade exactness for cycles, and a decoder that is subtly wrong is the thing
/// this whole module is trying not to be.
// Float arithmetic on values the format bounds: a coefficient is at most a
// quantised 12-bit sample times its divisor, and the sums below are 8 terms of
// those. The one index expression, `y * 8 + x`, is `0..8` in both, and every
// access through it is a `get`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bounded coefficients and 0..8 indices"
)]
fn idct_8x8(block: &mut [f32; 64], basis: &Basis) {
    let mut scratch = [0.0f32; 64];
    // Rows.
    for y in 0..8usize {
        for x in 0..8usize {
            let mut sum = 0.0f32;
            for u in 0..8usize {
                sum += basis.at(u, x) * block.get(y * 8 + u).copied().unwrap_or(0.0);
            }
            if let Some(slot) = scratch.get_mut(y * 8 + x) {
                *slot = sum / 2.0;
            }
        }
    }
    // Columns.
    for x in 0..8usize {
        for y in 0..8usize {
            let mut sum = 0.0f32;
            for v in 0..8usize {
                sum += basis.at(v, y) * scratch.get(v * 8 + x).copied().unwrap_or(0.0);
            }
            if let Some(slot) = block.get_mut(y * 8 + x) {
                *slot = sum / 2.0;
            }
        }
    }
}

/// Cosine, without `std`.
///
/// This crate is `no_std`, where `f32::cos` is not available. The series is
/// evaluated after folding the angle into `[0, pi/2]`, which keeps the term
/// count small and the error far below the one-eighth of a quantisation step
/// that could change a byte.
// The series and the argument folding are float arithmetic on an angle already
// reduced to `[0, pi/2]`, where no term can overflow.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a folded angle cannot overflow"
)]
fn cosine(angle: f32) -> f32 {
    const TWO_PI: f32 = core::f32::consts::PI * 2.0;
    let mut x = angle % TWO_PI;
    if x < 0.0 {
        x += TWO_PI;
    }
    // cos is even and repeats every 2pi; fold into [0, pi] then [0, pi/2].
    let mut sign = 1.0f32;
    if x > core::f32::consts::PI {
        x = TWO_PI - x;
    }
    if x > core::f32::consts::FRAC_PI_2 {
        x = core::f32::consts::PI - x;
        sign = -1.0;
    }
    // Taylor series about zero, to x^12. On [0, pi/2] the next term is under
    // 1e-9, which no 8-bit sample can see.
    let x2 = x * x;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    for n in 1..7u32 {
        #[allow(clippy::cast_precision_loss, reason = "n < 7")]
        let denom = ((2 * n - 1) * (2 * n)) as f32;
        term = -term * x2 / denom;
        sum += term;
    }
    sign * sum
}

/// Everything a scan needs, gathered as the segments go by.
struct Tables {
    quant: [[u16; 64]; 4],
    quant_seen: [bool; 4],
    dc: [Huffman; 4],
    ac: [Huffman; 4],
    restart_interval: usize,
}

impl Default for Tables {
    // Hand-written because `[u16; 64]` is past the length `Default` is derived
    // for, and a quantisation table of zeros would divide the picture away.
    fn default() -> Self {
        Self {
            quant: [[1u16; 64]; 4],
            quant_seen: [false; 4],
            dc: [(); 4].map(|()| Huffman::default()),
            ac: [(); 4].map(|()| Huffman::default()),
            restart_interval: 0,
        }
    }
}

/// Decode a baseline JPEG.
///
/// # Errors
///
/// [`ImageError::Unsupported`] naming the variant for progressive, arithmetic
/// and 12-bit files; [`ImageError::Malformed`] naming the field for a header
/// that cannot be true; [`ImageError::Truncated`] when the file stops inside a
/// structure it announced; [`ImageError::TooLarge`] past `limits`.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    if !is_jpeg(bytes) {
        return Err(ImageError::UnknownFormat);
    }
    let mut tables = Tables::default();
    let mut frame: Option<(usize, usize, Vec<Component>)> = None;
    // Past the SOI.
    let mut at = 2usize;

    loop {
        // Markers may be preceded by any number of fill `0xFF` bytes.
        let mut marker = None;
        while at < bytes.len() {
            let byte = *bytes.get(at).ok_or(ImageError::Truncated)?;
            at = at.saturating_add(1);
            if byte != 0xFF {
                continue;
            }
            while *bytes.get(at).unwrap_or(&0) == 0xFF {
                at = at.saturating_add(1);
            }
            marker = bytes.get(at).copied();
            at = at.saturating_add(1);
            break;
        }
        let Some(marker) = marker else {
            return Err(ImageError::Truncated);
        };

        match marker {
            // Standalone markers: no length, no payload.
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            0xD9 => return Err(ImageError::Truncated), // EOI before any scan
            _ => {}
        }

        let length = read_u16(bytes, at)?;
        let payload_start = at.saturating_add(2);
        let payload_end = at
            .checked_add(usize::from(length))
            .ok_or(ImageError::Truncated)?;
        let payload = bytes
            .get(payload_start..payload_end)
            .ok_or(ImageError::Truncated)?;
        at = payload_end;

        match marker {
            // Baseline and extended sequential, both of which decode the same
            // way at 8 bits.
            0xC0 | 0xC1 => frame = Some(read_frame(payload)?),
            0xC2 => {
                return Err(ImageError::Unsupported(
                    "progressive JPEG: the image arrives in successive approximations, and \
                     decoding only its first scan would give a recognisable but wrong picture",
                ));
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(ImageError::Unsupported(
                    "lossless, hierarchical or arithmetic-coded JPEG",
                ));
            }
            0xC4 => read_huffman_tables(payload, &mut tables)?,
            0xDB => read_quant_tables(payload, &mut tables)?,
            0xDD => {
                tables.restart_interval = usize::from(read_u16(payload, 0)?);
            }
            0xDA => {
                let Some((width, height, components)) = frame else {
                    return Err(ImageError::Malformed(
                        "a scan before the frame it belongs to",
                    ));
                };
                let components = read_scan_header(payload, components)?;
                let data = bytes.get(at..).ok_or(ImageError::Truncated)?;
                return decode_scan(data, width, height, components, &tables, limits);
            }
            // APPn, COM and everything else carries no state this needs.
            _ => {}
        }
    }
}

/// A big-endian `u16` at `at`.
fn read_u16(bytes: &[u8], at: usize) -> ImageResult<u16> {
    let hi = *bytes.get(at).ok_or(ImageError::Truncated)?;
    let lo = *bytes
        .get(at.saturating_add(1))
        .ok_or(ImageError::Truncated)?;
    Ok(u16::from_be_bytes([hi, lo]))
}

/// `SOF0`: the picture's size and how each component was sampled.
fn read_frame(payload: &[u8]) -> ImageResult<(usize, usize, Vec<Component>)> {
    let precision = *payload.first().ok_or(ImageError::Truncated)?;
    if precision != 8 {
        return Err(ImageError::Unsupported(
            "a JPEG with more than 8 bits per sample",
        ));
    }
    let height = usize::from(read_u16(payload, 1)?);
    let width = usize::from(read_u16(payload, 3)?);
    if width == 0 || height == 0 {
        return Err(ImageError::Malformed("a frame with a zero dimension"));
    }
    let count = usize::from(*payload.get(5).ok_or(ImageError::Truncated)?);
    if count == 0 || count > 4 {
        return Err(ImageError::Malformed("a frame with no usable components"));
    }
    let mut components = Vec::with_capacity(count);
    for n in 0..count {
        let base = 6usize
            .checked_add(n.checked_mul(3).ok_or(ImageError::Truncated)?)
            .ok_or(ImageError::Truncated)?;
        let id = *payload.get(base).ok_or(ImageError::Truncated)?;
        let sampling = *payload
            .get(base.saturating_add(1))
            .ok_or(ImageError::Truncated)?;
        let quant = usize::from(
            *payload
                .get(base.saturating_add(2))
                .ok_or(ImageError::Truncated)?,
        );
        let (h, v) = (usize::from(sampling >> 4), usize::from(sampling & 0x0F));
        if h == 0 || v == 0 || h > 4 || v > 4 {
            return Err(ImageError::Malformed("a component sampled zero times"));
        }
        if quant > 3 {
            return Err(ImageError::Malformed(
                "a component naming no quantisation table",
            ));
        }
        components.push(Component {
            id,
            h,
            v,
            quant,
            dc_table: 0,
            ac_table: 0,
            dc_prediction: 0,
        });
    }
    Ok((width, height, components))
}

/// `DQT`: one or more quantisation tables.
fn read_quant_tables(payload: &[u8], tables: &mut Tables) -> ImageResult<()> {
    let mut at = 0usize;
    while at < payload.len() {
        let spec = *payload.get(at).ok_or(ImageError::Truncated)?;
        at = at.saturating_add(1);
        let index = usize::from(spec & 0x0F);
        let wide = (spec >> 4) != 0;
        if index > 3 {
            return Err(ImageError::Malformed(
                "a quantisation table numbered past 3",
            ));
        }
        for n in 0..64usize {
            let value = if wide {
                let v = read_u16(payload, at)?;
                at = at.saturating_add(2);
                v
            } else {
                let v = u16::from(*payload.get(at).ok_or(ImageError::Truncated)?);
                at = at.saturating_add(1);
                v
            };
            // Stored zig-zagged; unpicked here so the rest of the decoder can
            // think in rows and columns.
            let slot = ZIGZAG.get(n).copied().unwrap_or(0);
            if let Some(table) = tables.quant.get_mut(index) {
                if let Some(cell) = table.get_mut(slot) {
                    *cell = value;
                }
            }
        }
        if let Some(seen) = tables.quant_seen.get_mut(index) {
            *seen = true;
        }
    }
    Ok(())
}

/// `DHT`: one or more Huffman tables.
fn read_huffman_tables(payload: &[u8], tables: &mut Tables) -> ImageResult<()> {
    let mut at = 0usize;
    while at < payload.len() {
        let spec = *payload.get(at).ok_or(ImageError::Truncated)?;
        at = at.saturating_add(1);
        let index = usize::from(spec & 0x0F);
        let is_ac = (spec >> 4) != 0;
        if index > 3 {
            return Err(ImageError::Malformed("a Huffman table numbered past 3"));
        }
        let mut counts = [0u8; 16];
        let mut total = 0usize;
        for n in 0..16usize {
            let count = *payload
                .get(at.saturating_add(n))
                .ok_or(ImageError::Truncated)?;
            if let Some(slot) = counts.get_mut(n) {
                *slot = count;
            }
            total = total.saturating_add(usize::from(count));
        }
        at = at.saturating_add(16);
        // 256 is every byte there is; a table claiming more is not a table.
        if total > 256 {
            return Err(ImageError::Malformed("a Huffman table with too many codes"));
        }
        let values = payload
            .get(at..at.saturating_add(total))
            .ok_or(ImageError::Truncated)?
            .to_vec();
        at = at.saturating_add(total);
        let table = Huffman { counts, values };
        let slot = if is_ac {
            tables.ac.get_mut(index)
        } else {
            tables.dc.get_mut(index)
        };
        if let Some(slot) = slot {
            *slot = table;
        }
    }
    Ok(())
}

/// `SOS`: which components this scan carries and which tables decode them.
fn read_scan_header(payload: &[u8], frame: Vec<Component>) -> ImageResult<Vec<Component>> {
    let count = usize::from(*payload.first().ok_or(ImageError::Truncated)?);
    if count == 0 || count > frame.len() {
        return Err(ImageError::Malformed("a scan naming no components"));
    }
    let mut out = Vec::with_capacity(count);
    for n in 0..count {
        let base = 1usize
            .checked_add(n.checked_mul(2).ok_or(ImageError::Truncated)?)
            .ok_or(ImageError::Truncated)?;
        let id = *payload.get(base).ok_or(ImageError::Truncated)?;
        let spec = *payload
            .get(base.saturating_add(1))
            .ok_or(ImageError::Truncated)?;
        let mut component = *frame
            .iter()
            .find(|c| c.id == id)
            .ok_or(ImageError::Malformed(
                "a scan naming a component the frame lacks",
            ))?;
        component.dc_table = usize::from(spec >> 4).min(3);
        component.ac_table = usize::from(spec & 0x0F).min(3);
        out.push(component);
    }
    Ok(out)
}

/// Decode the entropy-coded scan into pixels.
// Block placement arithmetic, all of it `0..8` within an MCU whose size the
// frame header bounds, and every write through it is a `get_mut`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bounded by the MCU geometry"
)]
fn decode_scan(
    data: &[u8],
    width: usize,
    height: usize,
    mut components: Vec<Component>,
    tables: &Tables,
    limits: Limits,
) -> ImageResult<Image> {
    let pixels_claimed = width.saturating_mul(height) as u64;
    if pixels_claimed > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels: pixels_claimed,
            limit: limits.max_pixels,
        });
    }

    let max_h = components.iter().map(|c| c.h).max().unwrap_or(1);
    let max_v = components.iter().map(|c| c.v).max().unwrap_or(1);
    let mcu_w = max_h.saturating_mul(8);
    let mcu_h = max_v.saturating_mul(8);
    let mcus_x = width.div_ceil(mcu_w);
    let mcus_y = height.div_ceil(mcu_h);

    // One plane per component, padded out to whole MCUs so a block never has
    // to be clipped while it is being written.
    let mut planes: Vec<(usize, usize, Vec<u8>)> = Vec::with_capacity(components.len());
    for component in &components {
        let plane_w = mcus_x.saturating_mul(component.h).saturating_mul(8);
        let plane_h = mcus_y.saturating_mul(component.v).saturating_mul(8);
        let size = plane_w.saturating_mul(plane_h);
        // Four times the pixel budget: the planes are padded out to whole MCUs
        // and a 4:2:0 file carries a plane per component, so a little slack is
        // ordinary while a lot is a file claiming a size it does not have.
        if size as u64 > limits.max_pixels.saturating_mul(4) {
            return Err(ImageError::TooLarge {
                pixels: size as u64,
                limit: limits.max_pixels,
            });
        }
        planes.push((plane_w, plane_h, vec![0u8; size]));
    }

    let basis = Basis::new();
    let mut bits = BitReader::new(data);
    let mut since_restart = 0usize;

    for mcu_y in 0..mcus_y {
        for mcu_x in 0..mcus_x {
            if tables.restart_interval > 0 && since_restart == tables.restart_interval {
                if bits.restart() {
                    for component in &mut components {
                        component.dc_prediction = 0;
                    }
                }
                since_restart = 0;
            }
            for (index, component) in components.iter_mut().enumerate() {
                for by in 0..component.v {
                    for bx in 0..component.h {
                        let mut block = [0.0f32; 64];
                        decode_block(&mut bits, component, tables, &mut block)?;
                        idct_8x8(&mut block, &basis);
                        let Some((plane_w, plane_h, plane)) = planes.get_mut(index) else {
                            continue;
                        };
                        let origin_x = mcu_x
                            .saturating_mul(component.h)
                            .saturating_add(bx)
                            .saturating_mul(8);
                        let origin_y = mcu_y
                            .saturating_mul(component.v)
                            .saturating_add(by)
                            .saturating_mul(8);
                        for y in 0..8usize {
                            for x in 0..8usize {
                                let px = origin_x.saturating_add(x);
                                let py = origin_y.saturating_add(y);
                                if px >= *plane_w || py >= *plane_h {
                                    continue;
                                }
                                // `+ 0.5` because `as` truncates. Without it
                                // every sample is biased half a level low,
                                // which is invisible in any one pixel and
                                // measurable across an image.
                                let value = block.get(y * 8 + x).copied().unwrap_or(0.0) + 128.5;
                                #[allow(clippy::cast_possible_truncation, reason = "clamped")]
                                #[allow(clippy::cast_sign_loss, reason = "clamped to 0..=255")]
                                let byte = value.clamp(0.0, 255.0) as u8;
                                if let Some(slot) = plane.get_mut(py * *plane_w + px) {
                                    *slot = byte;
                                }
                            }
                        }
                    }
                }
            }
            since_restart = since_restart.saturating_add(1);
        }
    }

    Ok(Image {
        width: u32::try_from(width).map_err(|_| ImageError::Malformed("an impossible width"))?,
        height: u32::try_from(height).map_err(|_| ImageError::Malformed("an impossible height"))?,
        pixels: to_pixels(width, height, &components, &planes, max_h, max_v),
    })
}

/// One 8x8 block: a DC difference, then run-length coded AC coefficients.
// The coefficient arithmetic is bounded by the format: a dequantised value is
// a 12-bit coefficient times a 16-bit divisor, and the DC predictor is a
// `saturating_add` chain of those.
#[allow(clippy::arithmetic_side_effects, reason = "bounded by the format")]
fn decode_block(
    bits: &mut BitReader<'_>,
    component: &mut Component,
    tables: &Tables,
    block: &mut [f32; 64],
) -> ImageResult<()> {
    let quant = tables
        .quant
        .get(component.quant)
        .ok_or(ImageError::Malformed(
            "a block naming no quantisation table",
        ))?;
    let dc_table = tables
        .dc
        .get(component.dc_table)
        .ok_or(ImageError::Malformed("a block naming no DC table"))?;
    let ac_table = tables
        .ac
        .get(component.ac_table)
        .ok_or(ImageError::Malformed("a block naming no AC table"))?;

    // The DC coefficient, as a difference from the previous block's.
    let Some(length) = dc_table.decode(bits) else {
        // Running out of bits mid-image is a truncated file, and the pixels
        // decoded so far are still worth returning -- but a block half-read
        // would be visibly wrong, so this stops at the block boundary.
        return Ok(());
    };
    let diff = bits.receive_extend(u32::from(length)).unwrap_or(0);
    component.dc_prediction = component.dc_prediction.saturating_add(diff);
    #[allow(clippy::cast_precision_loss, reason = "coefficients are small")]
    if let Some(slot) = block.get_mut(0) {
        *slot = (component.dc_prediction * i32::from(quant.first().copied().unwrap_or(1))) as f32;
    }

    // The 63 AC coefficients, as (run of zeros, value) pairs.
    let mut n = 1usize;
    while n < 64 {
        let Some(symbol) = ac_table.decode(bits) else {
            break;
        };
        let run = usize::from(symbol >> 4);
        let size = u32::from(symbol & 0x0F);
        if size == 0 {
            if run == 15 {
                // Sixteen zeros, and the block continues.
                n = n.saturating_add(16);
                continue;
            }
            // End of block: everything remaining is zero.
            break;
        }
        n = n.saturating_add(run);
        if n >= 64 {
            break;
        }
        let value = bits.receive_extend(size).unwrap_or(0);
        let slot = ZIGZAG.get(n).copied().unwrap_or(0);
        #[allow(clippy::cast_precision_loss, reason = "coefficients are small")]
        if let Some(cell) = block.get_mut(slot) {
            *cell = (value * i32::from(quant.get(slot).copied().unwrap_or(1))) as f32;
        }
        n = n.saturating_add(1);
    }
    Ok(())
}

/// Upsample the planes and convert to `0xAARRGGBB`.
// Index arithmetic bounded by the loops, and every access through it is a
// `get`. The multiplications are `saturating_mul` where a hostile size could
// reach them.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bounded by the loops; accesses are checked"
)]
fn to_pixels(
    width: usize,
    height: usize,
    components: &[Component],
    planes: &[(usize, usize, Vec<u8>)],
    max_h: usize,
    max_v: usize,
) -> Vec<u32> {
    let mut out = vec![0u32; width.saturating_mul(height)];
    for y in 0..height {
        for x in 0..width {
            // Nearest-neighbour upsampling. A photograph's chroma is already
            // half-resolution and the eye is poor at it, which is why the
            // format throws it away; interpolating would be a better picture
            // than the file contains.
            let sample = |index: usize| -> u8 {
                let Some(component) = components.get(index) else {
                    return 0;
                };
                let Some((plane_w, plane_h, plane)) = planes.get(index) else {
                    return 0;
                };
                let sx = x.saturating_mul(component.h) / max_h;
                let sy = y.saturating_mul(component.v) / max_v;
                if sx >= *plane_w || sy >= *plane_h {
                    return 0;
                }
                plane.get(sy * *plane_w + sx).copied().unwrap_or(0)
            };
            let pixel = if components.len() >= 3 {
                ycbcr_to_rgb(sample(0), sample(1), sample(2))
            } else {
                let grey = u32::from(sample(0));
                0xFF00_0000 | (grey << 16) | (grey << 8) | grey
            };
            if let Some(slot) = out.get_mut(y.saturating_mul(width).saturating_add(x)) {
                *slot = pixel;
            }
        }
    }
    out
}

/// JFIF's YCbCr to RGB, with the chroma centred on 128.
// Three bytes widened to floats and combined with constants under 2: the
// results are clamped to `0..=255` before they become bytes again.
#[allow(clippy::arithmetic_side_effects, reason = "clamped before use")]
fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> u32 {
    let y = f32::from(y);
    let cb = f32::from(cb) - 128.0;
    let cr = f32::from(cr) - 128.0;
    // `+ 0.5` for the same reason as the sample clamp: `as` truncates.
    let clamp = |v: f32| -> u32 {
        #[allow(clippy::cast_possible_truncation, reason = "clamped")]
        #[allow(clippy::cast_sign_loss, reason = "clamped to 0..=255")]
        let byte = (v + 0.5).clamp(0.0, 255.0) as u32;
        byte
    };
    let r = clamp(y + 1.402 * cr);
    let g = clamp(y - 0.344_136 * cb - 0.714_136 * cr);
    let b = clamp(y + 1.772 * cb);
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

/// Decode, then shrink to fit `max_w` x `max_h`.
///
/// A JPEG *can* be scaled during reconstruction -- taking only the top-left
/// corner of each block's coefficients yields a half, quarter or eighth-size
/// image nearly free -- and that is worth doing one day. This does the plain
/// thing instead: decode, then average. The crate's own contract allows it
/// ("falls back to a full decode for formats that cannot be scaled during
/// reconstruction"), and a thumbnailer that gets the right pixels slowly beats
/// one that refuses the format.
///
/// # Errors
///
/// As [`decode`].
#[allow(
    clippy::arithmetic_side_effects,
    reason = "div_ceil and max keep the factor >= 1"
)]
pub fn decode_scaled(bytes: &[u8], limits: Limits, max_w: u32, max_h: u32) -> ImageResult<Image> {
    let full = decode(bytes, limits)?;
    if max_w == 0 || max_h == 0 || (full.width <= max_w && full.height <= max_h) {
        // Already small enough. Inventing pixels is not what this is for.
        return Ok(full);
    }
    // The larger of the two ratios, so the result fits inside both bounds.
    let factor_w = full.width.div_ceil(max_w).max(1);
    let factor_h = full.height.div_ceil(max_h).max(1);
    let factor = factor_w.max(factor_h) as usize;
    Ok(box_filter(&full, factor))
}

/// Average each `factor` x `factor` square down to one pixel.
///
/// Averaging rather than picking one pixel per square: dropping pixels turns a
/// fine texture into moire, which in a thumbnail grid looks like a picture of
/// something else. The cost is one pass over the image.
// The sums are of four bytes at a time into a `u32` and the divisor is at least
// one, so neither can overflow; the index arithmetic is `saturating_` and every
// access through it is a `get`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "byte sums into u32, divisor >= 1"
)]
fn box_filter(image: &Image, factor: usize) -> Image {
    let factor = factor.max(1);
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let out_w = src_w.div_ceil(factor).max(1);
    let out_h = src_h.div_ceil(factor).max(1);
    let mut pixels = vec![0u32; out_w.saturating_mul(out_h)];

    for oy in 0..out_h {
        for ox in 0..out_w {
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for dy in 0..factor {
                for dx in 0..factor {
                    let sx = ox.saturating_mul(factor).saturating_add(dx);
                    let sy = oy.saturating_mul(factor).saturating_add(dy);
                    if sx >= src_w || sy >= src_h {
                        continue;
                    }
                    let Some(pixel) = image
                        .pixels
                        .get(sy.saturating_mul(src_w).saturating_add(sx))
                    else {
                        continue;
                    };
                    r = r.saturating_add((pixel >> 16) & 0xFF);
                    g = g.saturating_add((pixel >> 8) & 0xFF);
                    b = b.saturating_add(pixel & 0xFF);
                    n = n.saturating_add(1);
                }
            }
            let n = n.max(1);
            let pixel = 0xFF00_0000 | ((r / n) << 16) | ((g / n) << 8) | (b / n);
            if let Some(slot) = pixels.get_mut(oy.saturating_mul(out_w).saturating_add(ox)) {
                *slot = pixel;
            }
        }
    }
    Image {
        width: u32::try_from(out_w).unwrap_or(1),
        height: u32::try_from(out_h).unwrap_or(1),
        pixels,
    }
}

#[cfg(test)]
mod tests {
    // The same reasoning the crate's other test modules give: a test that
    // indexes out of range should fail loudly at the line that did it. The
    // defensive lints keep panics out of code that runs on a user's file.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    extern crate std;
    use super::*;

    /// A 24x16 baseline JPEG: two gradients crossed with a hard checker edge.
    ///
    /// Written by a reference encoder, not by this crate. A fixture this crate
    /// produced would only prove it agrees with itself -- the same reason
    /// `testing` exists for PNG, pointed the other way.
    ///
    /// Gradients and a hard edge because a flat field is the one image every
    /// decoder gets right: the DCT has nothing to do with it.
    const FIXTURE: &[u8] = &[
        255, 216, 255, 224, 0, 16, 74, 70, 73, 70, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 255, 219, 0, 67,
        0, 3, 2, 2, 3, 2, 2, 3, 3, 3, 3, 4, 3, 3, 4, 5, 8, 5, 5, 4, 4, 5, 10, 7, 7, 6, 8, 12, 10,
        12, 12, 11, 10, 11, 11, 13, 14, 18, 16, 13, 14, 17, 14, 11, 11, 16, 22, 16, 17, 19, 20, 21,
        21, 21, 12, 15, 23, 24, 22, 20, 24, 18, 20, 21, 20, 255, 219, 0, 67, 1, 3, 4, 4, 5, 4, 5,
        9, 5, 5, 9, 20, 13, 11, 13, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20,
        20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20,
        20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 255, 192, 0, 17, 8, 0, 16, 0, 24, 3, 1, 17, 0,
        2, 17, 1, 3, 17, 1, 255, 196, 0, 31, 0, 0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 255, 196, 0, 181, 16, 0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4,
        4, 0, 0, 1, 125, 1, 2, 3, 0, 4, 17, 5, 18, 33, 49, 65, 6, 19, 81, 97, 7, 34, 113, 20, 50,
        129, 145, 161, 8, 35, 66, 177, 193, 21, 82, 209, 240, 36, 51, 98, 114, 130, 9, 10, 22, 23,
        24, 25, 26, 37, 38, 39, 40, 41, 42, 52, 53, 54, 55, 56, 57, 58, 67, 68, 69, 70, 71, 72, 73,
        74, 83, 84, 85, 86, 87, 88, 89, 90, 99, 100, 101, 102, 103, 104, 105, 106, 115, 116, 117,
        118, 119, 120, 121, 122, 131, 132, 133, 134, 135, 136, 137, 138, 146, 147, 148, 149, 150,
        151, 152, 153, 154, 162, 163, 164, 165, 166, 167, 168, 169, 170, 178, 179, 180, 181, 182,
        183, 184, 185, 186, 194, 195, 196, 197, 198, 199, 200, 201, 202, 210, 211, 212, 213, 214,
        215, 216, 217, 218, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 241, 242, 243, 244,
        245, 246, 247, 248, 249, 250, 255, 196, 0, 31, 1, 0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0,
        0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 255, 196, 0, 181, 17, 0, 2, 1, 2, 4, 4, 3, 4,
        7, 5, 4, 4, 0, 1, 2, 119, 0, 1, 2, 3, 17, 4, 5, 33, 49, 6, 18, 65, 81, 7, 97, 113, 19, 34,
        50, 129, 8, 20, 66, 145, 161, 177, 193, 9, 35, 51, 82, 240, 21, 98, 114, 209, 10, 22, 36,
        52, 225, 37, 241, 23, 24, 25, 26, 38, 39, 40, 41, 42, 53, 54, 55, 56, 57, 58, 67, 68, 69,
        70, 71, 72, 73, 74, 83, 84, 85, 86, 87, 88, 89, 90, 99, 100, 101, 102, 103, 104, 105, 106,
        115, 116, 117, 118, 119, 120, 121, 122, 130, 131, 132, 133, 134, 135, 136, 137, 138, 146,
        147, 148, 149, 150, 151, 152, 153, 154, 162, 163, 164, 165, 166, 167, 168, 169, 170, 178,
        179, 180, 181, 182, 183, 184, 185, 186, 194, 195, 196, 197, 198, 199, 200, 201, 202, 210,
        211, 212, 213, 214, 215, 216, 217, 218, 226, 227, 228, 229, 230, 231, 232, 233, 234, 242,
        243, 244, 245, 246, 247, 248, 249, 250, 255, 218, 0, 12, 3, 1, 0, 2, 17, 3, 17, 0, 63, 0,
        249, 51, 65, 248, 73, 255, 0, 9, 134, 223, 220, 253, 147, 236, 222, 219, 247, 110, 252,
        177, 247, 127, 90, 253, 147, 17, 157, 127, 196, 105, 235, 245, 79, 170, 127, 220, 94, 127,
        107, 255, 0, 130, 249, 121, 125, 159, 157, 239, 210, 218, 252, 166, 77, 196, 31, 217, 214,
        215, 155, 155, 229, 183, 223, 220, 244, 125, 3, 225, 39, 252, 38, 27, 127, 115, 246, 79,
        179, 251, 111, 221, 187, 242, 199, 221, 253, 107, 229, 241, 57, 215, 252, 70, 158, 191, 84,
        250, 167, 253, 197, 231, 246, 191, 248, 47, 151, 151, 217, 249, 222, 253, 45, 175, 238,
        217, 47, 16, 127, 103, 91, 94, 110, 111, 150, 223, 127, 115, 209, 180, 31, 132, 159, 240,
        152, 109, 253, 207, 217, 62, 205, 237, 191, 118, 239, 203, 31, 119, 245, 175, 151, 196,
        231, 95, 241, 26, 122, 253, 83, 234, 159, 247, 23, 159, 218, 255, 0, 224, 190, 94, 95, 103,
        231, 123, 244, 182, 191, 187, 100, 220, 65, 253, 157, 109, 121, 185, 190, 91, 125, 253,
        207, 72, 208, 126, 18, 127, 194, 97, 183, 247, 63, 100, 251, 63, 182, 253, 219, 191, 44,
        125, 223, 214, 190, 95, 17, 157, 127, 196, 105, 235, 245, 79, 170, 127, 220, 94, 127, 107,
        255, 0, 130, 249, 121, 125, 159, 157, 239, 210, 218, 255, 0, 143, 121, 55, 16, 127, 103,
        91, 222, 230, 230, 249, 109, 247, 247, 61, 31, 65, 248, 73, 255, 0, 9, 128, 95, 220, 253,
        147, 236, 254, 219, 247, 110, 252, 177, 247, 127, 90, 249, 124, 78, 117, 255, 0, 17, 167,
        175, 213, 62, 169, 255, 0, 113, 121, 253, 175, 254, 11, 229, 229, 246, 126, 119, 191, 75,
        107, 251, 182, 75, 196, 31, 217, 214, 247, 185, 185, 190, 91, 125, 253, 207, 70, 208, 126,
        18, 127, 194, 97, 183, 247, 63, 100, 251, 63, 182, 253, 219, 191, 44, 125, 223, 214, 190,
        95, 19, 157, 127, 196, 105, 235, 245, 79, 170, 127, 220, 94, 127, 107, 255, 0, 130, 249,
        121, 125, 159, 157, 239, 210, 218, 254, 237, 147, 113, 7, 246, 117, 189, 238, 110, 111,
        150, 223, 127, 115, 255, 217,
    ];

    /// What a reference decoder makes of [`FIXTURE`], `RRGGBB` per pixel.
    const EXPECTED: &str = concat!(
        "0000FE0700FF1704FE1B06FF2F00003200004304004803005800FF5F00FF6E04FE7306FF",
        "8800008E00009D0400A10200B100FFB700FFC704FECB07FFDF0000E40000F30500FA0300",
        "0511FF0B11FB150EFA2013FF2E0C023A1208440F074A10005C12FF6112FB6B0DF97713FF",
        "880E039313089E0F07A40F00B410FFBC12FBC60EFAD014FFE00C01EA1207F50E06FC1000",
        "0020F90D26FD191EFF2624FF281F00352500401E024F23085621F86525FF701DFF7E24FF",
        "8120009026009A1E02A92306AF20F8BF25FFC91EFFD623FFD82000E52600F11E01FF2406",
        "0034FA0838FE1732FF242CFB293A04353300452F005331034F34FB6037FF6D32FE7B2CFA",
        "813A049035009F2F01AE3103A934FCB737FEC732FED52CFBD93A03E73300F52F00FF3102",
        "0A42010C40001645011E4B062E3DFA3843FD4248FE4445FF604202633F016D4603774C07",
        "883DFA9243FE9A49FE9E46FDBA4203BD3F02C64503D04B06E03DFAE943FDF249FEF546FD",
        "0458030752001359001D53002E57FF3852FF4659FD4853FA5957045D52026B5900765300",
        "8758FF9151FFA059FDA253FAB45805B75202C45800CD5300DF57FFEA52FFF859FCFA53F9",
        "006700086605176905236400286AFC3565F74268F74E68FF546600606604706A087C6400",
        "806AFB8C65F49C68F8A868FFAE6600BA6604C96807D46400D86BFCE566F5F368F7FF68FF",
        "0177000C7900157301287000287BFF3B79FE4472FF4F74FF5876006478006D7404817100",
        "807AFE9378FD9E73FFAA75FFB27800BD7900C67202D97100D87BFFEC78FDF473FFFF74FF",
        "0388FF0886FF178AFD1B8DFF318400358701448B004A89005A88FF6086FF708BFE748EFF",
        "8B84008F87009D8B00A48900B488FFB985FFC98AFDCD8DFFE28400E78700F68C00FA8900",
        "039AFF0999FA1495FA1D9BFF2F9503399B084497074A97005B99FF629AFB6C96FA769CFF",
        "889603939B089D9607A39600B599FFBB99FAC496F9CF9BFFE09603EB9B08F39706FA9700",
        "00AAF90BAEFD15A7FF22ADFF26AA0033AF003EA7024CAD0854A9F963AEFF6EA8FF7BADFF",
        "7FA9008DB00098A702A5AC06AEA9F9BBAEFEC7A7FFD4ACFFD7A900E5AF00EFA802FEAD06",
        "00BDFA06C0FD15BAFE21B4FA28C30535BD0144B70052BA034DBDFC5EC0FF6DBAFE7AB5FB",
        "80C3048EBD009DB700ABB902A8BDFCB7BFFEC5BAFED3B4FAD7C304E5BC00F4B600FFBA03",
        "0BCA020DC70116CD0120D3062FC5FA39CBFE42D0FE45CDFF62CA0365C7026FCD0378D407",
        "89C5FB92CAFD9CD0FF9ECCFDBCCA03BDC802C8CC03D1D306E1C4FAE9CBFDF4D0FEF5CEFD",
        "04DE0308D90116E0001EDA0030DEFF3AD8FF49E0FD4BDAFA5CDE0460D9026FE10079DA00",
        "8ADEFF94D8FFA2DFFCA5DAFAB6DE04BAD902C6E100D0DA00E2DEFFEAD8FFF8DFFCFCDBFA",
        "00EE000AEE0518F20623EC0028F2FC34ECF643F0F74EF0FF55EE0060EE0471F3097CEC01",
        "82F2FD8EECF49DF0F8A8F0FFB0EE00BAEE05CAF208D5EC00DAF2FCE4EDF4F3F0F7FFF0FF",
        "00FF000BFF0013FC0226FB0126FFFE38FFFD41FCFF4DFEFF55FF0061FF006CFD0481FB02",
        "7EFFFE92FFFD9AFBFFA6FDFFAFFF00BBFF00C5FC03D7FA00D6FFFEE8FFFDF3FCFFFFFEFF"
    );

    fn expected_at(index: usize) -> Option<(i32, i32, i32)> {
        let at = index.checked_mul(6)?;
        let text = EXPECTED.get(at..at.checked_add(6)?)?;
        let value = u32::from_str_radix(text, 16).ok()?;
        Ok::<(i32, i32, i32), ()>((
            ((value >> 16) & 0xFF) as i32,
            ((value >> 8) & 0xFF) as i32,
            (value & 0xFF) as i32,
        ))
        .ok()
    }

    /// The decoder agrees with a reference decoder, pixel for pixel.
    ///
    /// Not byte-identical, and it should not be: two decoders differ in how
    /// they round the inverse DCT, and the specification allows it. What is
    /// asserted is that no channel is off by more than 2 and the mean error is
    /// under a tenth of a level -- which is the difference between "rounds
    /// differently" and "decodes differently".
    ///
    /// This caught a real defect. Casting a sample with `as u8` truncates, so
    /// every channel came out half a level low: invisible in any one pixel,
    /// and a mean error of 1.15 across the image. Rounding took the mean to
    /// 0.03 and the worst case from 4 to 2.
    #[test]
    fn it_agrees_with_a_reference_decoder() {
        let image = decode(FIXTURE, Limits::default()).expect("the fixture decodes");
        assert_eq!((image.width, image.height), (24, 16));
        assert_eq!(image.pixels.len(), 24 * 16);

        let mut worst = 0i32;
        let mut total = 0i64;
        for (index, got) in image.pixels.iter().enumerate() {
            let (want_r, want_g, want_b) = expected_at(index).expect("a reference pixel");
            assert_eq!(got >> 24, 0xFF, "every pixel is opaque");
            for (shift, want) in [(16u32, want_r), (8, want_g), (0, want_b)] {
                let mine = ((got >> shift) & 0xFF) as i32;
                let difference = (mine - want).abs();
                assert!(
                    difference <= 2,
                    "pixel {index} ({got:08X}) differs by {difference}, which is a decode and not a rounding"
                );
                worst = worst.max(difference);
                total += i64::from(difference);
            }
        }
        let mean = total as f64 / (image.pixels.len() * 3) as f64;
        assert!(
            mean < 0.10,
            "mean channel error {mean:.3} is a systematic bias, not rounding"
        );
        let _ = worst;
    }

    /// A thumbnail request gets a smaller picture, not a refusal.
    ///
    /// `decode_scaled` is what a thumbnailer calls -- `apps/explorer` reaches
    /// the crate through it -- so a format wired into `decode` alone is a
    /// format the file browser still cannot show.
    #[test]
    fn a_scaled_decode_shrinks_rather_than_refusing() {
        let small = decode_scaled(FIXTURE, Limits::default(), 8, 8).expect("decodes");
        assert!(
            small.width <= 8 && small.height <= 8,
            "got {}x{}",
            small.width,
            small.height
        );
        assert_eq!(small.pixels.len(), (small.width * small.height) as usize);
        assert!(
            small.pixels.iter().all(|p| p >> 24 == 0xFF),
            "every pixel opaque"
        );
    }

    /// A picture already smaller than the bounds comes back at its own size.
    #[test]
    fn a_small_picture_is_not_enlarged() {
        let same = decode_scaled(FIXTURE, Limits::default(), 512, 512).expect("decodes");
        assert_eq!((same.width, same.height), (24, 16), "no pixels invented");
    }

    /// Shrinking averages rather than dropping pixels.
    ///
    /// The factor has to straddle the fixture's 4-pixel checker squares for
    /// this to mean anything: at a factor of exactly 4 each box lands inside
    /// one square and averaging gives the same answer as sampling, which is
    /// how the first version of this test managed to fail against correct
    /// code. A factor of 5 crosses the boundaries, so a true average produces
    /// mid-tones that no dropped-pixel scaler can.
    #[test]
    fn shrinking_averages_rather_than_sampling() {
        let small = decode_scaled(FIXTURE, Limits::default(), 5, 4).expect("decodes");
        let mid_tones = small
            .pixels
            .iter()
            .filter(|p| {
                let blue = *p & 0xFF;
                (40..=215).contains(&blue)
            })
            .count();
        assert!(
            mid_tones > 0,
            "every pixel is at one extreme, which is what sampling gives: {:?}",
            small
                .pixels
                .iter()
                .map(|p| p & 0xFF)
                .collect::<alloc::vec::Vec<_>>()
        );
    }

    /// A progressive JPEG is refused by name, not half-decoded.
    #[test]
    fn a_progressive_jpeg_is_refused() {
        // The fixture with its SOF0 marker changed to SOF2, which is the one
        // byte that makes it progressive.
        let mut progressive = FIXTURE.to_vec();
        let at = progressive
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("the fixture is baseline");
        if let Some(slot) = progressive.get_mut(at + 1) {
            *slot = 0xC2;
        }
        match decode(&progressive, Limits::default()) {
            Err(ImageError::Unsupported(why)) => {
                assert!(why.contains("progressive"), "it should say which: {why}");
            }
            other => panic!("a progressive JPEG should be refused, got {other:?}"),
        }
    }

    /// The signature check does not claim every file starting `FF D8`.
    #[test]
    fn the_signature_wants_three_bytes() {
        assert!(is_jpeg(FIXTURE));
        assert!(!is_jpeg(&[0xFF, 0xD8]));
        assert!(!is_jpeg(&[0xFF, 0xD8, 0x00]));
        assert!(!is_jpeg(&[0x89, b'P', b'N', b'G']));
    }

    /// A truncated file is refused rather than decoded to rubbish.
    #[test]
    fn a_truncated_jpeg_is_refused() {
        let half = FIXTURE.get(..FIXTURE.len() / 3).expect("a prefix");
        assert!(decode(half, Limits::default()).is_err());
    }

    /// A picture past the caller's limit is refused before it is allocated.
    #[test]
    fn a_picture_past_the_limit_is_refused() {
        let limits = Limits {
            max_pixels: 16,
            ..Limits::default()
        };
        match decode(FIXTURE, limits) {
            Err(ImageError::TooLarge { pixels, limit }) => {
                assert_eq!(limit, 16);
                assert_eq!(pixels, 24 * 16);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}
