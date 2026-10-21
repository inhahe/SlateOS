//! The arithmetic decoder CABAC slices are read with (`cabac.c`,
//! `cabac_functions.h`), as FFmpeg's C build has it: 16 bits fetched at a
//! time (`CABAC_BITS`), the refill that finds its shift through the
//! normalisation table (no fast count of trailing zeros), and reads that do
//! not stop at the end of the slice (`UNCHECKED_BITSTREAM_READER`, which
//! `h264_cabac.c` sets).
//!
//! The decoder keeps its place as positions in the buffer the slice is read
//! from -- FFmpeg's `bytestream_start`, `bytestream` and `bytestream_end`
//! pointers -- and is handed that buffer at each read, so that it can live
//! in the slice context. A read past the buffer's end finds zero; FFmpeg's
//! finds its padding, which is zero for as far as any stream reads.
//!
//! One thing about FFmpeg's start depends on memory addresses:
//! `ff_init_cabac_decoder` reads a third byte only when the first two leave
//! the pointer at an odd address, to keep its later two-byte fetches
//! aligned. The bits decoded are the same either way, but the position is a
//! byte apart, and the checks at the end of a slice compare that position
//! with the end of the data. FFmpeg's buffers start aligned, so the parity
//! is that of the slice's offset in its buffer: [`Cabac::init`] takes the
//! buffer's own offset in its allocation (`Bits::origin`) to find it.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on the decoder's range and low value, which stay under \
              2^27 for any stream; written wrapping where a damaged one could push them further"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int and unsigned conversions, made where it makes them"
)]

mod tables;

use tables::CABAC_TABLES;
pub(crate) use tables::{CONTEXT_INIT_I, CONTEXT_INIT_PB};

use crate::decoder::DecodeError;

/// `CABAC_BITS`: the bits a refill fetches.
const CABAC_BITS: u32 = 16;
/// `CABAC_MASK`.
const CABAC_MASK: i32 = (1 << CABAC_BITS) - 1;

/// Where each of the tables in [`CABAC_TABLES`] starts
/// (`H264_*_OFFSET`).
const NORM_SHIFT: i32 = 0;
const LPS_RANGE: i32 = 512;
const MLPS_STATE: i32 = 1024;
pub(crate) const LAST_COEFF_FLAG_OFFSET_8X8: i32 = 1280;

/// `ff_h264_cabac_tables[i]`. Every state a stream can reach indexes inside
/// the array, though not always inside the table the index names, which is
/// why the four are one array; past the array FFmpeg reads memory C leaves
/// undefined, zero here.
pub(crate) fn table(i: i32) -> i32 {
    usize::try_from(i)
        .ok()
        .and_then(|i| CABAC_TABLES.get(i))
        .map_or(0, |&v| i32::from(v))
}

/// The byte at `at`, zero past the buffer (or before it).
fn byte(buf: &[u8], at: isize) -> i32 {
    usize::try_from(at)
        .ok()
        .and_then(|at| buf.get(at))
        .map_or(0, |&b| i32::from(b))
}

/// The decoder's state (`CABACContext`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Cabac {
    pub(crate) low: i32,
    pub(crate) range: i32,
    /// Where the data starts, where the next fetch reads and where the data
    /// ends, as positions in the buffer (`bytestream_start`, `bytestream`,
    /// `bytestream_end`). The end may lie before the start: FFmpeg takes
    /// the size from the bits a damaged header left, which can be negative.
    pub(crate) start: isize,
    pub(crate) pos: isize,
    pub(crate) end: isize,
}

impl Cabac {
    /// The decoder started on `size` bytes of `buf` from `start`
    /// (`ff_init_cabac_decoder`), `buf` being `origin` bytes into an aligned
    /// allocation. Invalid data when the first bits already lie outside the
    /// range.
    pub(crate) fn init(
        &mut self,
        buf: &[u8],
        origin: usize,
        start: isize,
        size: isize,
    ) -> Result<(), DecodeError> {
        self.start = start;
        self.pos = start;
        self.end = start.wrapping_add(size);
        self.low = byte(buf, self.pos) << 18;
        self.pos += 1;
        self.low += byte(buf, self.pos) << 10;
        self.pos += 1;
        // The two-byte fetches from here on start at an even address.
        if (origin as isize).wrapping_add(self.pos) & 1 == 0 {
            self.low += 1 << 9;
        } else {
            self.low += (byte(buf, self.pos) << 2) + 2;
            self.pos += 1;
        }
        self.range = 0x1FE;
        if (self.range << (CABAC_BITS + 1)) < self.low {
            return Err(DecodeError);
        }
        Ok(())
    }

    /// Two more bytes into the low value (`refill`).
    fn refill(&mut self, buf: &[u8]) {
        self.low = self
            .low
            .wrapping_add((byte(buf, self.pos) << 9) + (byte(buf, self.pos + 1) << 1))
            .wrapping_sub(CABAC_MASK);
        self.pos += (CABAC_BITS / 8) as isize;
    }

    /// Two more bytes in, at the bit the low value has run down to
    /// (`refill2`, the table-driven form FFmpeg's C build compiles).
    fn refill2(&mut self, buf: &[u8]) {
        let x = (self.low ^ self.low.wrapping_sub(1)) as u32;
        let i = 7 - table(NORM_SHIFT + (x >> (CABAC_BITS - 1)) as i32);
        let fetched = (-CABAC_MASK as u32)
            .wrapping_add(((byte(buf, self.pos) << 9) + (byte(buf, self.pos + 1) << 1)) as u32);
        self.low = self.low.wrapping_add(fetched.wrapping_shl(i as u32) as i32);
        self.pos += (CABAC_BITS / 8) as isize;
    }

    /// The range doubled once if it has fallen under 256
    /// (`renorm_cabac_decoder_once`).
    fn renorm_once(&mut self, buf: &[u8]) {
        let shift = ((self.range.wrapping_sub(0x100) as u32) >> 31) as i32;
        self.range <<= shift;
        self.low = self.low.wrapping_shl(shift as u32);
        if self.low & CABAC_MASK == 0 {
            self.refill(buf);
        }
    }

    /// One bin of the context whose state is `state` (`get_cabac_inline`).
    pub(crate) fn get(&mut self, buf: &[u8], state: &mut u8) -> i32 {
        let mut s = i32::from(*state);
        let range_lps = table(LPS_RANGE + 2 * (self.range & 0xC0) + s);
        self.range = self.range.wrapping_sub(range_lps);
        let mut lps_mask = (self.range << (CABAC_BITS + 1)).wrapping_sub(self.low) >> 31;
        self.low = self
            .low
            .wrapping_sub((self.range << (CABAC_BITS + 1)) & lps_mask);
        self.range = self
            .range
            .wrapping_add(range_lps.wrapping_sub(self.range) & lps_mask);
        s ^= lps_mask;
        *state = table(MLPS_STATE + 128 + s) as u8;
        let bit = s & 1;
        lps_mask = table(NORM_SHIFT + self.range);
        self.range = self.range.wrapping_shl(lps_mask as u32);
        self.low = self.low.wrapping_shl(lps_mask as u32);
        if self.low & CABAC_MASK == 0 {
            self.refill2(buf);
        }
        bit
    }

    /// One bin read with no context, both values equally likely
    /// (`get_cabac_bypass`).
    pub(crate) fn bypass(&mut self, buf: &[u8]) -> i32 {
        self.low = self.low.wrapping_add(self.low);
        if self.low & CABAC_MASK == 0 {
            self.refill(buf);
        }
        let range = self.range << (CABAC_BITS + 1);
        if self.low < range {
            0
        } else {
            self.low = self.low.wrapping_sub(range);
            1
        }
    }

    /// `val`, negated if a bypass bin says so (`get_cabac_bypass_sign`).
    pub(crate) fn bypass_sign(&mut self, buf: &[u8], val: i32) -> i32 {
        self.low = self.low.wrapping_add(self.low);
        if self.low & CABAC_MASK == 0 {
            self.refill(buf);
        }
        let mut range = self.range << (CABAC_BITS + 1);
        self.low = self.low.wrapping_sub(range);
        let mask = self.low >> 31;
        range &= mask;
        self.low = self.low.wrapping_add(range);
        (val ^ mask).wrapping_sub(mask)
    }

    /// Whether the slice ends here (`get_cabac_terminate`): 0 if not, else
    /// the bytes read since the start.
    pub(crate) fn terminate(&mut self, buf: &[u8]) -> isize {
        self.range = self.range.wrapping_sub(2);
        if self.low < self.range << (CABAC_BITS + 1) {
            self.renorm_once(buf);
            0
        } else {
            self.pos - self.start
        }
    }

    /// Where the bits the decoder has not yet used start, in bytes: its
    /// position, less the bytes still held in the low value (the
    /// arithmetic of `skip_bytes` and of an I_PCM macroblock's start).
    pub(crate) fn unread(&self) -> isize {
        let mut ptr = self.pos;
        if self.low & 0x1 != 0 {
            ptr -= 1;
        }
        if self.low & 0x1FF != 0 {
            ptr -= 1;
        }
        ptr
    }
}

#[cfg(test)]
mod tests;
