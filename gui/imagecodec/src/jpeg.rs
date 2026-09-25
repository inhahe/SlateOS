//! JPEG (JFIF): baseline sequential, and progressive.
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
//!    chroma back to full resolution, and convert YCbCr to RGB. The
//!    upsampling is libjpeg's triangle filter, rounding and all, so that a
//!    subsampled photograph comes out as every other decoder shows it rather
//!    than with its colour edges stepped; see `jpeg/upsample.rs`.
//!
//! **Progressive** files (`SOF2`) send the whole picture several times, each
//! pass adding frequencies or precision, so their coefficients are gathered
//! across every scan and reconstructed at the end; see [`progressive`]. They
//! share everything after the entropy decoding with baseline -- dequantising,
//! the inverse DCT, upsampling, colour -- so the two decode the same
//! coefficients to the same pixels.
//!
//! # What this does not do, and says so
//!
//! **Arithmetic coding** and **12-bit samples** are named rather than
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

mod progressive;
mod upsample;

use upsample::{Rows, Samples, Shape};

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
    /// The first code of each length, and where that length's values begin.
    first: [u32; 16],
    offset: [usize; 16],
    values: Vec<u8>,
    /// For every [`FAST_BITS`]-bit prefix, the symbol the canonical walk finds
    /// within those bits, as `(length << 8) | value`, or 0 when it needs more
    /// of them. Empty until [`Self::index`] runs.
    fast: Vec<u16>,
}

/// How many bits the fast lookup resolves at once.
///
/// Nine covers the codes a table spends on its common symbols — the JPEG
/// standard's own example tables put every DC code and the most frequent AC
/// codes within it — so nearly every symbol is one table read rather than a
/// walk of up to sixteen bits.
const FAST_BITS: u32 = 9;

impl Huffman {
    /// Read one value, or `None` if no code matches.
    // `code - first` is guarded by the `code >= first` test immediately
    // above it, which is the only subtraction here; everything else is
    // `checked_`.
    ///
    /// One table read for a code of up to [`FAST_BITS`] bits. Anything longer,
    /// or a code that would run past the end of the data, takes the canonical
    /// walk a bit at a time — which is also what the table was built from, so
    /// the two cannot disagree about any table, a malformed one included.
    fn decode(&self, bits: &mut BitReader<'_>) -> Option<u8> {
        let (prefix, real) = bits.peek(FAST_BITS);
        if let Some(&entry) = self.fast.get(prefix as usize) {
            let length = u32::from(entry >> 8);
            if length != 0 && length <= real {
                bits.consume(length);
                return u8::try_from(entry & 0xFF).ok();
            }
        }
        self.decode_slowly(bits)
    }

    /// The canonical walk: one bit at a time until a length's code range
    /// holds the code read so far.
    #[allow(clippy::arithmetic_side_effects, reason = "guarded by the bound above")]
    fn decode_slowly(&self, bits: &mut BitReader<'_>) -> Option<u8> {
        let mut code = 0u32;
        for length in 0..16usize {
            code = code.checked_mul(2)?.checked_add(u32::from(bits.bit()?))?;
            let count = u32::from(*self.counts.get(length)?);
            let first = *self.first.get(length)?;
            if count > 0 && code >= first && code < first.checked_add(count)? {
                let at = self
                    .offset
                    .get(length)?
                    .checked_add(code.checked_sub(first)? as usize)?;
                return self.values.get(at).copied();
            }
        }
        None
    }

    /// Fill the first-code and offset tables in from the counts.
    ///
    /// Both are derivable from `counts`, and the first version derived them
    /// per symbol -- a loop inside the decode loop, so about 128 iterations to
    /// read one symbol where 16 would do. Every block of a picture must be
    /// entropy-decoded whatever size it is reconstructed at, so this is the
    /// cost a scaled decode cannot avoid and the one worth spending care on.
    fn index(&mut self) {
        let mut code = 0u32;
        let mut offset = 0usize;
        for length in 0..16usize {
            if let Some(slot) = self.first.get_mut(length) {
                *slot = code;
            }
            if let Some(slot) = self.offset.get_mut(length) {
                *slot = offset;
            }
            let count = u32::from(self.counts.get(length).copied().unwrap_or(0));
            code = code.saturating_add(count).saturating_mul(2);
            offset = offset.saturating_add(count as usize);
        }
        self.fast = (0..1u32 << FAST_BITS)
            .map(|prefix| self.fast_entry(prefix))
            .collect();
    }

    /// What the canonical walk returns reading the [`FAST_BITS`]-bit `prefix`,
    /// if it returns within those bits: `(length << 8) | value`, else 0.
    ///
    /// Computed by walking, rather than by writing each code's range into the
    /// table, so that a table whose counts oversubscribe the code space — which
    /// a hostile file can send — gets exactly the answer [`Self::decode_slowly`]
    /// would give, instead of whichever code happened to be written last.
    fn fast_entry(&self, prefix: u32) -> u16 {
        let mut code = 0u32;
        for length in 0..FAST_BITS {
            let bit = (prefix >> (FAST_BITS.saturating_sub(1).saturating_sub(length))) & 1;
            code = code.saturating_mul(2).saturating_add(bit);
            let at = length as usize;
            let count = u32::from(self.counts.get(at).copied().unwrap_or(0));
            let first = self.first.get(at).copied().unwrap_or(0);
            if count > 0 && code >= first && code < first.saturating_add(count) {
                let index = self
                    .offset
                    .get(at)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(code.saturating_sub(first) as usize);
                return match self.values.get(index) {
                    Some(&value) => {
                        let length = u16::try_from(length.saturating_add(1)).unwrap_or(0);
                        (length << 8) | u16::from(value)
                    }
                    // The walk would return nothing here; so does the table,
                    // and the slow path says so.
                    None => 0,
                };
            }
        }
        0
    }
}

/// The entropy-coded data, read in bits.
///
/// Two pieces of awkwardness live here. A `0xFF` in the data is written
/// `0xFF 0x00`, so the zero is skipped; and any other `0xFF xx` is a marker,
/// which ends the run of data rather than being read as bits.
///
/// Bytes are loaded up to eight at a time into a 64-bit buffer, so a code or a
/// coefficient of up to sixteen bits is a shift rather than a loop. Loading
/// stops at a marker without consuming it — which is what keeps the read-ahead
/// invisible: the buffer never holds a byte from beyond a marker, so a restart
/// finds the marker exactly where a byte-at-a-time reader would.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bits not yet handed out, the next one in bit 63.
    buffer: u64,
    /// How many of the buffer's top bits are real.
    count: u32,
    /// Loading has reached a marker or the end of the data, and loads nothing
    /// more until a restart marker is consumed.
    stopped: bool,
}

impl<'a> BitReader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buffer: 0,
            count: 0,
            stopped: false,
        }
    }

    /// Load whole bytes until the buffer holds more than 56 bits, a marker is
    /// reached, or the data runs out.
    fn refill(&mut self) {
        while self.count <= 56 && !self.stopped {
            let Some(&byte) = self.data.get(self.pos) else {
                self.stopped = true;
                break;
            };
            if byte == 0xFF {
                match self.data.get(self.pos.saturating_add(1)) {
                    // A stuffed zero: the 0xFF is data.
                    Some(0x00) => self.pos = self.pos.saturating_add(2),
                    // Anything else is a marker and the data ends here; the
                    // marker is left for `restart` to find.
                    _ => {
                        self.stopped = true;
                        break;
                    }
                }
            } else {
                self.pos = self.pos.saturating_add(1);
            }
            self.buffer |= u64::from(byte) << (56u32.saturating_sub(self.count));
            self.count = self.count.saturating_add(8);
        }
    }

    /// How far into the data loading has reached: every byte before this has
    /// been taken into the stream, and the next marker is at or after it.
    ///
    /// Bits loaded and not consumed are a scan's padding, never the start of
    /// the next segment, because loading stops *at* a marker.
    const fn position(&self) -> usize {
        self.pos
    }

    /// One bit, or `None` at the end of the data or at a marker.
    fn bit(&mut self) -> Option<u8> {
        if self.count == 0 {
            self.refill();
            if self.count == 0 {
                return None;
            }
        }
        let bit = u8::try_from(self.buffer >> 63).ok();
        self.consume(1);
        bit
    }

    /// The next `n` bits (1 to 16) without consuming them, as a number, and
    /// how many of them are real: past the end of the data they read as 0.
    fn peek(&mut self, n: u32) -> (u32, u32) {
        if self.count < n {
            self.refill();
        }
        let n = n.clamp(1, 16);
        let value = u32::try_from(self.buffer >> (64u32.saturating_sub(n))).unwrap_or(0);
        (value, self.count.min(n))
    }

    /// Drop `n` bits the caller has read with [`Self::peek`]. Never more than
    /// are real: the caller checked.
    fn consume(&mut self, n: u32) {
        let n = n.min(self.count);
        self.buffer = self.buffer.checked_shl(n).unwrap_or(0);
        self.count = self.count.saturating_sub(n);
    }

    /// `n` bits as an unsigned number.
    ///
    /// Up to sixteen at once, which is every size a legal baseline file uses.
    /// Past the end of the data the bits that exist are consumed and the answer
    /// is `None` — exactly what reading them one at a time did, so a truncated
    /// file comes apart in the same place it always did.
    fn bits(&mut self, n: u32) -> Option<i32> {
        if n == 0 {
            return Some(0);
        }
        if n > 16 {
            // A size no legal table produces; read the long way round, which
            // is also how a value too big for an `i32` is refused.
            return self.bits_slowly(n);
        }
        if self.count < n {
            self.refill();
            if self.count < n {
                self.buffer = 0;
                self.count = 0;
                return None;
            }
        }
        let value = i32::try_from(self.buffer >> (64u32.saturating_sub(n))).ok();
        self.consume(n);
        value
    }

    /// [`Self::bits`], one bit at a time.
    fn bits_slowly(&mut self, n: u32) -> Option<i32> {
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
    ///
    /// Only the rest of the byte being read is dropped. Whole bytes already
    /// loaded mean the data went on without a marker — loading stops *at* a
    /// marker — so there is none to consume, and reading resumes with them,
    /// just as it would have from the data.
    fn restart(&mut self) -> bool {
        let partial = self.count % 8;
        self.consume(partial);
        if self.count > 0 {
            return false;
        }
        self.buffer = 0;
        // A restart marker is `FF D0` through `FF D7`.
        while let Some(byte) = self.data.get(self.pos) {
            if *byte != 0xFF {
                self.stopped = false;
                return false;
            }
            match self.data.get(self.pos.saturating_add(1)) {
                Some(0xD0..=0xD7) => {
                    self.pos = self.pos.saturating_add(2);
                    self.stopped = false;
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
    /// The same numbers by output position: `columns[x][u]` is `table[u][x]`,
    /// so the row pass can walk one output's weights in order.
    columns: [[f32; 8]; 8],
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
        let mut columns = [[0.0f32; 8]; 8];
        for (u, row) in table.iter().enumerate() {
            for (x, &weight) in row.iter().enumerate() {
                if let Some(cell) = columns.get_mut(x).and_then(|c| c.get_mut(u)) {
                    *cell = weight;
                }
            }
        }
        Self { table, columns }
    }

    fn at(&self, u: usize, x: usize) -> f32 {
        self.table
            .get(u)
            .and_then(|row| row.get(x))
            .copied()
            .unwrap_or(0.0)
    }
}

/// The inverse DCT at a reduced size, using only the coefficients that matter.
///
/// A block's top-left `n` x `n` coefficients are its low frequencies, and
/// transforming just those yields an `n` x `n` image of the block directly --
/// a scaled decode that costs less than a full one rather than more. At `n =
/// 1` that is the DC coefficient alone: the block's average, which is exactly
/// what a one-pixel-per-block thumbnail wants.
///
/// This is why a JPEG thumbnail need never hold the full picture. Decoding a
/// 4000x5333 photograph whole to make a 128-pixel preview allocates about 85
/// MB on the way; at `n = 1` it allocates a sixty-fourth of that, and
/// `apps/explorer`'s own comment records paying that cost down for PNG for the
/// same reason.
// The same bounded float arithmetic as the full transform, over `0..n` where
// `n` is `1..=8`; the one index expression is `y * 8 + x` with both under 8,
// and every access through it is a `get`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bounded coefficients and 0..8 indices"
)]
fn idct_scaled(block: &[f32; 64], basis: &Basis, n: usize, out: &mut [f32; 64]) {
    let n = n.clamp(1, 8);
    if n == 8 {
        let mut full = *block;
        idct_8x8(&mut full, basis);
        *out = full;
        return;
    }
    // Rows first, into the top-left n x n of the scratch, then columns. The
    // normalisation is the same 1/4 as the full transform and does *not*
    // depend on how many terms are summed -- it is fixed by the definition.
    //
    // The first version scaled by `n/8` on the reasoning that fewer basis
    // functions carry less amplitude. That is wrong, and wrong in a way no
    // test caught: at `n = 1` it made every block an eighth of its true
    // value, so every sample collapsed toward the 128 that centres the range
    // and the picture came out as flat mid-grey. Comparing the mean colour of
    // a scaled decode against a full one showed it at once -- (124, 127, 131)
    // against (103, 128, 158), every channel pulled to the middle.
    let mut scratch = [0.0f32; 64];
    for y in 0..n {
        for x in 0..n {
            let mut sum = 0.0f32;
            for u in 0..n {
                sum += basis.at(u, scaled_position(x, n))
                    * block.get(y * 8 + u).copied().unwrap_or(0.0);
            }
            if let Some(slot) = scratch.get_mut(y * 8 + x) {
                *slot = sum / 2.0;
            }
        }
    }
    for x in 0..n {
        for y in 0..n {
            let mut sum = 0.0f32;
            for v in 0..n {
                sum += basis.at(v, scaled_position(y, n))
                    * scratch.get(v * 8 + x).copied().unwrap_or(0.0);
            }
            if let Some(slot) = out.get_mut(y * 8 + x) {
                *slot = sum / 2.0;
            }
        }
    }
}

/// Where an output sample of an `n`-point transform sits among the 8.
///
/// The basis table is built for eight positions; an `n`-point transform wants
/// the sample at the centre of the `8/n` it stands for, which keeps the
/// reduced image aligned with the full one instead of shifted a fraction of a
/// block to one side.
const fn scaled_position(index: usize, n: usize) -> usize {
    // `n` is clamped to 1..=8 by the caller, so the divisor is never zero and
    // the step is 1..=8; `index` is below `n`, so the product is under 64 and
    // the result is clamped to a valid position regardless.
    let step = 8usize.saturating_div(if n == 0 { 1 } else { n });
    let centre = index.saturating_mul(step).saturating_add(step / 2);
    if centre > 7 { 7 } else { centre }
}

/// The inverse discrete cosine transform, 8x8, separable.
///
/// Rows then columns, which is 16 eight-point transforms rather than the 4096
/// multiply-accumulates the two-dimensional definition asks for. Written for
/// clarity rather than speed beyond that: the fast integer approximations
/// trade exactness for cycles, and a decoder that is subtly wrong is the thing
/// this whole module is trying not to be.
///
/// **What it does skip is zeros, and only zeros.** Quantisation leaves most of
/// a block's coefficients at zero — typically all but the first row or two —
/// and a term with a zero coefficient adds `±0.0` to a sum that started at
/// `+0.0`, which changes nothing, not even the sign of a zero. So each row
/// stops at its last non-zero coefficient, an all-zero row is not transformed
/// at all, and the column pass leaves out the rows that came out zero. The
/// output is bit for bit what the full arithmetic gives, in the same order;
/// the table `tests::the_fast_idct_is_the_full_idct` holds it to that.
fn idct_8x8(block: &mut [f32; 64], basis: &Basis) {
    let mut scratch = [[0.0f32; 8]; 8];
    let mut live = [false; 8];
    // Rows.
    let (rows, _) = block.as_chunks::<8>();
    for ((row, out), alive) in rows.iter().zip(scratch.iter_mut()).zip(live.iter_mut()) {
        let Some(last) = row.iter().rposition(|&c| c != 0.0) else {
            continue;
        };
        *alive = true;
        let terms = last.saturating_add(1);
        for (slot, weights) in out.iter_mut().zip(&basis.columns) {
            let mut sum = 0.0f32;
            for (b, &c) in weights.iter().zip(row).take(terms) {
                sum += b * c;
            }
            *slot = sum / 2.0;
        }
    }
    // Columns, a row of output at a time: each output sample still adds its
    // terms in increasing `v`, which is the order the full transform used.
    // `basis.columns[y][v]` is `basis(v, y)`.
    let (out_rows, _) = block.as_chunks_mut::<8>();
    for (out_row, weights) in out_rows.iter_mut().zip(&basis.columns) {
        let mut acc = [0.0f32; 8];
        for ((srow, &b), &alive) in scratch.iter().zip(weights).zip(&live) {
            if alive {
                for (a, &sv) in acc.iter_mut().zip(srow) {
                    *a += b * sv;
                }
            }
        }
        for (slot, a) in out_row.iter_mut().zip(acc) {
            *slot = a / 2.0;
        }
    }
}

/// The value every sample of a block with only a DC coefficient comes out as.
///
/// The row pass's one live row is the DC times `basis(0, x)`, halved, which is
/// the same for every `x` because the first basis function is the constant
/// `1/sqrt 2` — `cosine(0)` sums its series to exactly 1. The column pass then
/// does the same to that. These are the very operations [`idct_8x8`] and
/// [`idct_scaled`] perform for such a block, with the zero terms (which change
/// nothing) left out, so the answer is theirs bit for bit, at every scale.
fn flat_block(dc: f32, basis: &Basis) -> f32 {
    let row = (basis.at(0, 0) * dc) / 2.0;
    (basis.at(0, 0) * row) / 2.0
}

/// The transform as it was before zeros were skipped: every term of every sum.
/// Kept as the reference the fast one is tested against.
#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bounded coefficients and 0..8 indices"
)]
fn idct_8x8_reference(block: &mut [f32; 64], basis: &Basis) {
    let mut scratch = [0.0f32; 64];
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
    decode_at(bytes, limits, 8)
}

/// [`decode`], with each 8x8 block reconstructed at `block` pixels square.
fn decode_at(bytes: &[u8], limits: Limits, block: usize) -> ImageResult<Image> {
    if !is_jpeg(bytes) {
        return Err(ImageError::UnknownFormat);
    }
    let mut tables = Tables::default();
    let mut frame: Option<(usize, usize, Vec<Component>)> = None;
    // A progressive frame's coefficients, gathered across its scans.
    let mut passes: Option<progressive::Coefficients> = None;
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
            // The data ran out before the end-of-image marker. A progressive
            // picture is reconstructed from the passes that did arrive -- a
            // softer picture, which is what a browser shows too.
            if let Some(passes) = passes.take().filter(progressive::Coefficients::has_scans) {
                return passes.finish();
            }
            return Err(ImageError::Truncated);
        };

        match marker {
            // Standalone markers: no length, no payload.
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // End of image: where a progressive picture is finished. For a
            // baseline one it comes before any scan, which is a file with no
            // picture in it.
            0xD9 => {
                if let Some(passes) = passes.take().filter(progressive::Coefficients::has_scans) {
                    return passes.finish();
                }
                return Err(ImageError::Truncated);
            }
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
                let (width, height, components) = read_frame(payload)?;
                passes = Some(progressive::Coefficients::new(
                    width,
                    height,
                    components.clone(),
                    limits,
                    block,
                )?);
                frame = Some((width, height, components));
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
            0xDA if passes.is_some() => {
                let Some(coefficients) = passes.as_mut() else {
                    continue;
                };
                let mut scan = coefficients.read_scan(payload)?;
                let data = bytes.get(at..).ok_or(ImageError::Truncated)?;
                let used = coefficients.decode_scan(data, &mut scan, &tables);
                at = next_marker(bytes, at.saturating_add(used));
            }
            0xDA => {
                let Some((width, height, components)) = frame else {
                    return Err(ImageError::Malformed(
                        "a scan before the frame it belongs to",
                    ));
                };
                let components = read_scan_header(payload, components)?;
                let data = bytes.get(at..).ok_or(ImageError::Truncated)?;
                return decode_scan(data, width, height, components, &tables, limits, block);
            }
            // APPn, COM and everything else carries no state this needs.
            _ => {}
        }
    }
}

/// Where the next segment starts at or after `from`: the `0xFF` of the first
/// marker that is neither a stuffed zero nor a restart.
///
/// For after a progressive scan. The scan's decoder stops where its data ends,
/// but an encoder may leave bytes between the last block and the next marker,
/// and those can include `0xFF 0x00` pairs and restart markers that belong to
/// the scan. Taking the first `0xFF` as the next segment would read one of
/// them as a marker with a length, and walk off into the entropy data.
fn next_marker(bytes: &[u8], from: usize) -> usize {
    let mut at = from;
    while let Some(&byte) = bytes.get(at) {
        if byte == 0xFF {
            match bytes.get(at.saturating_add(1)) {
                Some(0x00 | 0xD0..=0xD7) => {
                    at = at.saturating_add(2);
                    continue;
                }
                _ => return at,
            }
        }
        at = at.saturating_add(1);
    }
    at
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
        let mut table = Huffman {
            counts,
            first: [0; 16],
            offset: [0; 16],
            values,
            fast: Vec::new(),
        };
        table.index();
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
    block_size: usize,
) -> ImageResult<Image> {
    let pixels_claimed = width.saturating_mul(height) as u64;
    if pixels_claimed > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels: pixels_claimed,
            limit: limits.max_pixels,
        });
    }

    // How many pixels each 8x8 block becomes. Eight is a full decode; less is
    // a scaled one, done by transforming fewer coefficients rather than by
    // decoding everything and throwing most of it away.
    let block_size = block_size.clamp(1, 8);
    let max_h = components.iter().map(|c| c.h).max().unwrap_or(1);
    let max_v = components.iter().map(|c| c.v).max().unwrap_or(1);
    // MCU geometry is in source pixels and does not change with the scale; only
    // what each block *becomes* does.
    let mcus_x = width.div_ceil(max_h.saturating_mul(8));
    let mcus_y = height.div_ceil(max_v.saturating_mul(8));
    let out_width = width.saturating_mul(block_size).div_ceil(8).max(1);
    let out_height = height.saturating_mul(block_size).div_ceil(8).max(1);

    // One plane per component, padded out to whole MCUs so a block never has
    // to be clipped while it is being written.
    let mut planes: Vec<Samples> = Vec::with_capacity(components.len());
    let mut plane_bytes = 0usize;
    for component in &components {
        let shape = Shape::of(
            (width, height),
            (component.h, component.v),
            (max_h, max_v),
            block_size,
        );
        let size = shape.len();
        // Four times the pixel budget: the planes are padded out to whole MCUs
        // and a 4:2:0 file carries a plane per component, so a little slack is
        // ordinary while a lot is a file claiming a size it does not have.
        if size as u64 > limits.max_pixels.saturating_mul(4) {
            return Err(ImageError::TooLarge {
                pixels: size as u64,
                limit: limits.max_pixels,
            });
        }
        // And against the caller's byte budget, which bounds a different thing
        // -- `max_pixels` is about the picture's declared size, this about how
        // much memory reconstructing it takes. JPEG cannot expand without end
        // the way a zlib stream can, since its output size follows from the
        // frame header rather than from a compressor; but a caller that states
        // a budget has stated something, and one sample per byte makes the
        // comparison exact rather than approximate.
        plane_bytes = plane_bytes.saturating_add(size);
        if plane_bytes > limits.max_decompressed_bytes {
            return Err(ImageError::TooLarge {
                pixels: plane_bytes as u64,
                limit: limits.max_decompressed_bytes as u64,
            });
        }
        planes.push(Samples::new(shape));
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
                        let any_ac = decode_block(&mut bits, component, tables, &mut block)?;
                        if !any_ac {
                            // Every output of the transform is the same one
                            // number; see `flat_block`.
                            block = [flat_block(block[0], &basis); 64];
                        } else if block_size == 8 {
                            idct_8x8(&mut block, &basis);
                        } else {
                            let mut scaled = [0.0f32; 64];
                            idct_scaled(&block, &basis, block_size, &mut scaled);
                            block = scaled;
                        }
                        let Some(plane) = planes.get_mut(index) else {
                            continue;
                        };
                        let origin_x = mcu_x
                            .saturating_mul(component.h)
                            .saturating_add(bx)
                            .saturating_mul(block_size);
                        let origin_y = mcu_y
                            .saturating_mul(component.v)
                            .saturating_add(by)
                            .saturating_mul(block_size);
                        store_block(&block, block_size, plane, origin_x, origin_y);
                    }
                }
            }
            since_restart = since_restart.saturating_add(1);
        }
    }

    Ok(Image {
        width: u32::try_from(out_width)
            .map_err(|_| ImageError::Malformed("an impossible width"))?,
        height: u32::try_from(out_height)
            .map_err(|_| ImageError::Malformed("an impossible height"))?,
        pixels: to_pixels(out_width, out_height, &planes, block_size > 1),
    })
}

/// Write one reconstructed block into its plane at `(origin_x, origin_y)`,
/// clipped to the plane, level-shifted and clamped to a byte.
///
/// A row at a time: the plane's row is sliced once and the block's samples
/// are zipped along it, where writing a sample at a time recomputed and
/// bounds-checked the index sixty-four times a block.
fn store_block(
    block: &[f32; 64],
    block_size: usize,
    plane: &mut Samples,
    origin_x: usize,
    origin_y: usize,
) {
    let (plane_w, plane_h) = (plane.shape.stride, plane.shape.rows);
    if origin_x >= plane_w {
        return;
    }
    let cols = block_size.min(plane_w.saturating_sub(origin_x));
    for (y, samples) in block.as_chunks::<8>().0.iter().take(block_size).enumerate() {
        let py = origin_y.saturating_add(y);
        if py >= plane_h {
            break;
        }
        let start = py.saturating_mul(plane_w).saturating_add(origin_x);
        let Some(row) = plane.data.get_mut(start..start.saturating_add(cols)) else {
            continue;
        };
        for (slot, &sample) in row.iter_mut().zip(samples) {
            // `+ 0.5` because `as` truncates. Without it every sample is
            // biased half a level low, which is invisible in any one pixel
            // and measurable across an image.
            let value = sample + 128.5;
            #[allow(clippy::cast_possible_truncation, reason = "clamped")]
            #[allow(clippy::cast_sign_loss, reason = "clamped to 0..=255")]
            let byte = value.clamp(0.0, 255.0) as u8;
            *slot = byte;
        }
    }
}

/// One 8x8 block: a DC difference, then run-length coded AC coefficients.
///
/// Returns whether any AC coefficient came out non-zero. A block that is its
/// DC alone — a flat patch, and much of every photograph's chroma — is one
/// value everywhere, which the caller fills rather than transforms.
// The coefficient arithmetic is bounded by the format: a dequantised value is
// a 12-bit coefficient times a 16-bit divisor, and the DC predictor is a
// `saturating_add` chain of those.
#[allow(clippy::arithmetic_side_effects, reason = "bounded by the format")]
fn decode_block(
    bits: &mut BitReader<'_>,
    component: &mut Component,
    tables: &Tables,
    block: &mut [f32; 64],
) -> ImageResult<bool> {
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
        return Ok(false);
    };
    let diff = bits.receive_extend(u32::from(length)).unwrap_or(0);
    component.dc_prediction = component.dc_prediction.saturating_add(diff);
    #[allow(clippy::cast_precision_loss, reason = "coefficients are small")]
    if let Some(slot) = block.get_mut(0) {
        *slot = (component.dc_prediction * i32::from(quant.first().copied().unwrap_or(1))) as f32;
    }

    // The 63 AC coefficients, as (run of zeros, value) pairs.
    let mut any_ac = false;
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
            any_ac |= *cell != 0.0;
        }
        n = n.saturating_add(1);
    }
    Ok(any_ac)
}

/// Bring every plane up to the picture's resolution and convert to
/// `0xAARRGGBB`.
///
/// The upsampling is libjpeg's, filter and rounding alike (see [`upsample`]):
/// a photograph's colour is usually stored at half resolution, and restoring
/// it with the filter every other decoder uses is what makes a subsampled
/// picture come out as it does everywhere else. `fancy` is false only for an
/// eighth-scale decode, which libjpeg does not filter either.
///
/// A row at a time: every plane hands over its row already at the picture's
/// width, so converting it is a walk along three slices.
fn to_pixels(width: usize, height: usize, planes: &[Samples], fancy: bool) -> Vec<u32> {
    let mut out = vec![0u32; width.saturating_mul(height)];
    let mut rows: Vec<Rows<'_>> = planes
        .iter()
        .map(|plane| Rows::new(plane, width, fancy))
        .collect();
    let chroma = Chroma::new();
    for (y, out_row) in out.chunks_exact_mut(width.max(1)).enumerate() {
        match rows.as_mut_slice() {
            [luma, blue, red, ..] => {
                let (luma, blue, red) = (luma.row(y), blue.row(y), red.row(y));
                for (((slot, &cy), &cb), &cr) in out_row.iter_mut().zip(luma).zip(blue).zip(red) {
                    *slot = chroma.rgb(cy, cb, cr);
                }
            }
            // One component, or two, of which only the first is a picture.
            [grey, ..] => {
                for (slot, &sample) in out_row.iter_mut().zip(grey.row(y)) {
                    let grey = u32::from(sample);
                    *slot = 0xFF00_0000 | (grey << 16) | (grey << 8) | grey;
                }
            }
            [] => {}
        }
    }
    out
}

/// [`ycbcr_to_rgb`]'s four chroma products, for every byte a chroma sample can
/// be.
///
/// Each entry is the very expression `ycbcr_to_rgb` evaluates — the same
/// multiplication of the same two floats — so a lookup gives the same bits the
/// arithmetic would, and the sums built from them are added in the same order.
/// What goes is four multiplications per pixel, which over a 21-megapixel
/// photograph is eighty million of them.
struct Chroma {
    red_cr: [f32; 256],
    green_cb: [f32; 256],
    green_cr: [f32; 256],
    blue_cb: [f32; 256],
}

impl Chroma {
    fn new() -> Self {
        let mut tables = Self {
            red_cr: [0.0; 256],
            green_cb: [0.0; 256],
            green_cr: [0.0; 256],
            blue_cb: [0.0; 256],
        };
        for (byte, (((r, gb), gr), b)) in (0u8..=255).zip(
            tables
                .red_cr
                .iter_mut()
                .zip(tables.green_cb.iter_mut())
                .zip(tables.green_cr.iter_mut())
                .zip(tables.blue_cb.iter_mut()),
        ) {
            let centred = f32::from(byte) - 128.0;
            *r = 1.402 * centred;
            *gb = 0.344_136 * centred;
            *gr = 0.714_136 * centred;
            *b = 1.772 * centred;
        }
        tables
    }

    /// [`ycbcr_to_rgb`], by table.
    fn rgb(&self, y: u8, cb: u8, cr: u8) -> u32 {
        let y = f32::from(y);
        let (cb, cr) = (usize::from(cb), usize::from(cr));
        let at = |table: &[f32; 256], i: usize| table.get(i).copied().unwrap_or(0.0);
        let r = clamp_byte(y + at(&self.red_cr, cr));
        let g = clamp_byte(y - at(&self.green_cb, cb) - at(&self.green_cr, cr));
        let b = clamp_byte(y + at(&self.blue_cb, cb));
        0xFF00_0000 | (r << 16) | (g << 8) | b
    }
}

/// A channel value rounded to the nearest byte and clamped: `+ 0.5` because
/// `as` truncates.
fn clamp_byte(v: f32) -> u32 {
    #[allow(clippy::cast_possible_truncation, reason = "clamped")]
    #[allow(clippy::cast_sign_loss, reason = "clamped to 0..=255")]
    let byte = (v + 0.5).clamp(0.0, 255.0) as u32;
    byte
}

/// JFIF's YCbCr to RGB, with the chroma centred on 128.
///
/// The definition [`Chroma::rgb`] is a faster way of computing, and the test
/// that holds the two to the same answer for every input.
// Three bytes widened to floats and combined with constants under 2: the
// results are clamped to `0..=255` before they become bytes again.
#[cfg(test)]
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

/// The picture's size, without decoding it.
///
/// Walks to the frame header and stops. A thumbnailer needs this to choose how
/// much of the picture to reconstruct, and reading it should not cost what
/// reading the picture costs.
///
/// # Errors
///
/// As [`decode`], for the header it does read.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    if !is_jpeg(bytes) {
        return Err(ImageError::UnknownFormat);
    }
    let mut at = 2usize;
    loop {
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
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // Every frame marker carries the size in the same place, including
            // the ones this cannot decode: a caller may want to know how big a
            // progressive file is in order to say so.
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                let length = read_u16(bytes, at)?;
                let payload = bytes
                    .get(at.saturating_add(2)..at.saturating_add(usize::from(length)))
                    .ok_or(ImageError::Truncated)?;
                let height = u32::from(read_u16(payload, 1)?);
                let width = u32::from(read_u16(payload, 3)?);
                if width == 0 || height == 0 {
                    return Err(ImageError::Malformed("a frame with a zero dimension"));
                }
                return Ok((width, height));
            }
            0xD9 => return Err(ImageError::Truncated),
            _ => {}
        }
        let length = read_u16(bytes, at)?;
        at = at
            .checked_add(usize::from(length))
            .ok_or(ImageError::Truncated)?;
    }
}

/// Decode at the smallest size that still covers `max_w` x `max_h`.
///
/// **Scaled during reconstruction, not decoded whole and shrunk.** Each 8x8
/// block is transformed from only its top-left coefficients, so asking for a
/// preview of a 4000x5333 photograph reconstructs it at 500x667 and never
/// allocates the 85 MB the full picture would need. `apps/explorer`'s
/// thumbnailer has a comment recording that it paid exactly this cost down for
/// PNG -- "the larger half of this function's peak, 96 MB for a 24-megapixel
/// photograph, to produce 64 KB of preview" -- and a JPEG path that decoded
/// whole would have handed it straight back.
///
/// The block size is a power of two because that is what transforming a
/// prefix of the coefficients gives; the caller's exact box is fitted by
/// averaging what remains, which is at most a 2x reduction.
///
/// **Measured**, release build, a 4000x5333 photograph (7.5 MB, quality 90,
/// 4:2:0): a 128-pixel thumbnail in about 0.34 s and the whole picture in about
/// 1.1 s, holding 9408 pixels rather than 21332000. Those were 1.25 s and
/// 3.65 s before the bit reader loaded 64 bits at a time, Huffman codes were
/// read from a table, the transform skipped zero coefficients and colour
/// conversion stopped dividing per pixel — every one of which leaves the
/// output bit for bit as it was (`tests::the_fast_*`, and a 28-output
/// comparison against the previous decoder over 4:2:0, 4:2:2, 4:4:4, greyscale,
/// restart-marker and quality-100 files). A debug build is roughly ten times
/// slower, which is worth knowing before anyone optimises against one.
/// (An earlier figure here, 91 seconds for the whole picture, was not
/// reproducible on this machine; 3.65 s was the pre-optimisation baseline.)
///
/// # Errors
///
/// As [`decode`].
pub fn decode_scaled(bytes: &[u8], limits: Limits, max_w: u32, max_h: u32) -> ImageResult<Image> {
    let (width, height) = dimensions(bytes)?;
    let mut block = 8usize;
    if max_w > 0 && max_h > 0 {
        // The smallest power of two whose reconstruction still covers the
        // request in both directions.
        for candidate in [1usize, 2, 4] {
            let at_w = width.saturating_mul(candidate as u32).div_ceil(8);
            let at_h = height.saturating_mul(candidate as u32).div_ceil(8);
            if at_w >= max_w && at_h >= max_h {
                block = candidate;
                break;
            }
        }
    }
    let image = decode_at(bytes, limits, block)?;
    if max_w == 0 || max_h == 0 || (image.width <= max_w && image.height <= max_h) {
        return Ok(image);
    }
    let factor_w = image.width.div_ceil(max_w).max(1);
    let factor_h = image.height.div_ceil(max_h).max(1);
    Ok(box_filter(&image, factor_w.max(factor_h) as usize))
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
    const FIXTURE: &[u8] = crate::testing::SMALL_JPEG;

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

    /// Every entry point on the crate dispatches to JPEG, not just `decode`.
    ///
    /// Three of them exist -- `decode`, `decode_scaled` and `dimensions` --
    /// and a format wired into one is a format the callers of the other two
    /// still cannot use. `apps/explorer` reaches this crate through
    /// `decode_scaled` and `apps/imageviewer` through `dimensions`, so each
    /// omission is a whole application left where it started. Both were
    /// omissions here, found by following the callers rather than by reading
    /// this file.
    #[test]
    fn every_entry_point_knows_about_jpeg() {
        let limits = Limits::default();
        assert_eq!(
            crate::dimensions(FIXTURE).expect("dimensions dispatches"),
            (24, 16)
        );
        let whole = crate::decode(FIXTURE, limits).expect("decode dispatches");
        assert_eq!((whole.width, whole.height), (24, 16));
        let small = crate::decode_scaled(FIXTURE, limits, 8, 8).expect("decode_scaled dispatches");
        assert!(small.width <= 8 && small.height <= 8);
    }

    /// The caller's byte budget is honoured, not only its pixel budget.
    ///
    /// `Limits` has two fields because they bound different things, and a
    /// decoder that reads one and ignores the other supports half a contract
    /// while appearing to support all of it.
    #[test]
    fn a_tight_byte_budget_is_refused() {
        let limits = Limits {
            max_decompressed_bytes: 8,
            ..Limits::default()
        };
        match decode(FIXTURE, limits) {
            Err(ImageError::TooLarge { limit, .. }) => assert_eq!(limit, 8),
            other => panic!("the byte budget was ignored: {other:?}"),
        }
    }

    /// A progressive JPEG is refused by name, not half-decoded.
    #[test]
    fn a_baseline_scan_in_a_progressive_frame_is_refused_as_malformed() {
        // The fixture with its SOF0 marker changed to SOF2: a progressive frame
        // whose one scan sends DC and all 63 AC coefficients together, which
        // no progressive scan may. Read as a pass it would decode a picture
        // subtly wrong, so it is refused, and says why.
        let mut progressive = FIXTURE.to_vec();
        let at = progressive
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("the fixture is baseline");
        if let Some(slot) = progressive.get_mut(at + 1) {
            *slot = 0xC2;
        }
        match decode(&progressive, Limits::default()) {
            Err(ImageError::Malformed(why)) => {
                assert!(why.contains("DC and AC"), "it should say which: {why}");
            }
            other => panic!("a baseline scan cannot be a progressive pass, got {other:?}"),
        }
    }

    #[test]
    fn the_next_marker_skips_stuffed_bytes_and_restarts() {
        let bytes = [
            0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD3, 0x56, 0xFF, 0xFF, 0xC4, 0x00,
        ];
        assert_eq!(
            next_marker(&bytes, 0),
            7,
            "a fill byte before the marker is the marker"
        );
        assert_eq!(next_marker(&bytes, 8), 8);
        assert_eq!(next_marker(&[0x01, 0x02], 0), 2, "none: the end");
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

    // ------------------------------------------------------------------
    // The fast paths are the slow arithmetic, not an approximation of it
    // ------------------------------------------------------------------

    /// A deterministic stream of numbers, so a failure names its case.
    struct Dice(u64);

    impl Dice {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    /// The zero-skipping transform gives the full transform's bits, for
    /// blocks shaped like real ones (a few low frequencies), dense ones, rows
    /// with zeros inside them, and the all-zero block.
    #[test]
    fn the_fast_idct_is_the_full_idct() {
        let basis = Basis::new();
        let mut dice = Dice(0x51A7_E05D);
        for case in 0..4000 {
            let mut block = [0.0f32; 64];
            // How far into the block coefficients may appear: 0 keeps only
            // the DC, 64 fills it.
            let reach = match case % 4 {
                0 => 1 + dice.below(4) as usize,
                1 => 1 + dice.below(16) as usize,
                2 => 64,
                _ => 0,
            };
            for &slot in ZIGZAG.iter().take(reach.max(1)) {
                if dice.below(3) != 0 {
                    block[slot] = (dice.below(2048) as i32 - 1024) as f32;
                }
            }
            let mut fast = block;
            let mut full = block;
            idct_8x8(&mut fast, &basis);
            idct_8x8_reference(&mut full, &basis);
            for i in 0..64 {
                assert_eq!(
                    fast[i].to_bits(),
                    full[i].to_bits(),
                    "case {case}, sample {i}: {} against {}",
                    fast[i],
                    full[i]
                );
            }
        }
    }

    /// A block that is its DC alone fills to the one value every scale of the
    /// transform gives it.
    #[test]
    fn a_flat_block_is_what_the_transform_makes_of_its_dc() {
        let basis = Basis::new();
        for dc in (-2048..=2048).step_by(7).map(|v| v as f32) {
            let mut block = [0.0f32; 64];
            block[0] = dc;
            let mut full = block;
            idct_8x8_reference(&mut full, &basis);
            let flat = flat_block(dc, &basis);
            assert!(
                full.iter().all(|v| v.to_bits() == flat.to_bits()),
                "DC {dc}: {flat} against {:?}",
                &full[..4]
            );
            for n in 1..8 {
                let mut scaled = [0.0f32; 64];
                idct_scaled(&block, &basis, n, &mut scaled);
                for y in 0..n {
                    for x in 0..n {
                        assert_eq!(
                            scaled[y * 8 + x].to_bits(),
                            flat.to_bits(),
                            "DC {dc} at scale {n}, ({x}, {y})"
                        );
                    }
                }
            }
        }
    }

    /// The chroma tables give what the formula gives, for every one of the
    /// sixteen million inputs there are.
    #[test]
    fn the_chroma_tables_are_the_colour_formula_for_every_input() {
        let chroma = Chroma::new();
        for y in 0..=255u8 {
            for cb in 0..=255u8 {
                for cr in 0..=255u8 {
                    assert_eq!(
                        chroma.rgb(y, cb, cr),
                        ycbcr_to_rgb(y, cb, cr),
                        "Y {y} Cb {cb} Cr {cr}: by table, then by formula"
                    );
                }
            }
        }
    }

    /// A Huffman table from counts, as `DHT` would give it.
    fn table(counts: [u8; 16], values: &[u8]) -> Huffman {
        let mut t = Huffman {
            counts,
            values: values.to_vec(),
            ..Huffman::default()
        };
        t.index();
        t
    }

    /// Decode every symbol in `data` with `decode` (the table lookup) and with
    /// `decode_slowly` (the walk), side by side, and require the same symbols.
    ///
    /// The whole sequence, to the end of the data: a lookup that consumed one
    /// bit too many or too few would put every later symbol out of step, so the
    /// sequence is the check on the bit position too. (The readers' buffers are
    /// not compared: when a refill happens depends on how many bits were
    /// asked for, and that is allowed to differ.)
    fn same_symbols(t: &Huffman, data: &[u8]) {
        let mut fast = BitReader::new(data);
        let mut slow = BitReader::new(data);
        for step in 0..10_000 {
            let a = t.decode(&mut fast);
            let b = t.decode_slowly(&mut slow);
            assert_eq!(a, b, "symbol {step}");
            if a.is_none() {
                return;
            }
        }
    }

    /// The fast lookup reads the same symbols from the same bits as the
    /// canonical walk: for the JPEG standard's own luminance AC table, for
    /// tables with codes longer than the lookup covers, and for tables whose
    /// counts claim more codes than there is room for, which a hostile file
    /// can send.
    #[test]
    fn the_fast_huffman_lookup_is_the_canonical_walk() {
        // ITU-T T.81 Table K.5, luminance AC.
        let k5 = table(
            [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D],
            &(0..=255u8).cycle().take(162).collect::<Vec<_>>(),
        );
        // Every length used once, out to sixteen bits.
        let long = table([1; 16], &(0..16u8).collect::<Vec<_>>());
        // Oversubscribed: more codes of length 2 than two bits can hold.
        let crowded = table(
            [1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            &[1, 2, 3, 4, 5, 6, 7],
        );
        // Values listed short of what the counts promise.
        let short = table([0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &[9]);
        let mut dice = Dice(0xC0DE_D00D);
        for t in [&k5, &long, &crowded, &short] {
            for _ in 0..50 {
                let len = dice.below(300) as usize;
                let mut data: Vec<u8> = (0..len).map(|_| dice.below(256) as u8).collect();
                // Sometimes a marker in the middle, where the data must stop.
                if len > 10 && dice.below(2) == 0 {
                    let at = dice.below(len as u64 - 2) as usize;
                    data[at] = 0xFF;
                    data[at + 1] = 0xD9;
                }
                same_symbols(t, &data);
            }
        }
    }

    /// Reading sixteen bits at a time gives what reading them one at a time
    /// gives, including across stuffed bytes and up to a marker.
    #[test]
    fn many_bits_at_once_are_the_same_bits_one_at_a_time() {
        let data = [
            0x12, 0xFF, 0x00, 0xAB, 0xCD, 0xFF, 0x00, 0x7F, 0xFF, 0xD0, 0x55,
        ];
        let mut dice = Dice(7);
        for _ in 0..500 {
            let mut fast = BitReader::new(&data);
            let mut slow = BitReader::new(&data);
            loop {
                let n = 1 + dice.below(16) as u32;
                let a = fast.bits(n);
                let b = slow.bits_slowly(n);
                assert_eq!(a, b, "{n} bits");
                if a.is_none() {
                    // Both consumed what there was, so both are now dry.
                    assert_eq!(fast.bit(), None);
                    assert_eq!(slow.bit(), None);
                    break;
                }
            }
        }
    }
}
