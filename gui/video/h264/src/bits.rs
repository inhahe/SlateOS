//! A NAL unit's bits, read as FFmpeg's `GetBitContext` reads them
//! (`libavcodec/get_bits.h`), and the Exp-Golomb codes H.264 writes most of
//! its syntax in (`golomb.h`, `golomb.c`).
//!
//! # Reading past the end
//!
//! A stream's last byte is never quite where its syntax ends -- a damaged
//! slice runs on, a truncated one stops short -- and FFmpeg's answer is
//! part of what it decodes: past the bits it was given, a read goes on into
//! the bytes after them (an RBSP's stop bit and the zeros that follow it),
//! and past those into zeros (the 64 bytes of padding every FFmpeg buffer
//! carries). This reader does the same, with zeros past the end of the
//! slice it holds however far a read goes.
//!
//! What differs between FFmpeg's two readers is only where the position is
//! left. The *checked* reader -- the one parameter sets, slice headers and
//! SEI are read with -- stops counting eight bits past the end; the
//! *unchecked* one -- `h264_cavlc.c`'s and `h264dec.c`'s, set with
//! `UNCHECKED_BITSTREAM_READER` -- counts on, so a slice that overran its
//! data says by how much ([`Bits::left`] goes as negative as the overrun).
//! The same `GetBitContext` is read both ways in FFmpeg, by whichever file
//! the call is compiled in; here [`Bits::set_unchecked`] says which way the
//! reads that follow count.

#![allow(
    clippy::indexing_slicing,
    reason = "the only indexing is into golomb.c's 512-entry tables, by a 9-bit value"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions are under i32::MAX - 512 (`new`) and move by at most 63 bits a read; \
              Golomb arithmetic is on 32-bit values and shifts under 32, written wrapping \
              where FFmpeg's unsigned arithmetic wraps"
)]

/// FFmpeg's `AVERROR_INVALIDDATA` (`FFERRTAG('I', 'N', 'D', 'A')`), which
/// [`Bits::ue`] answers for a code too long for it, as `get_ue_golomb`
/// does -- and which callers then compare, store and add to exactly as
/// FFmpeg's do, so the value itself matters.
pub(crate) const AVERROR_INVALIDDATA: i32 = -0x4144_4E49;

/// The most bits a 32-bit cache holds whole after an unaligned load: what
/// [`Bits::bits`] and [`Bits::show`] may be asked for (`MIN_CACHE_BITS`).
const MIN_CACHE_BITS: u32 = 25;

/// The most bits one buffer may hold: `init_get_bits` refuses a size from
/// `INT_MAX - max(7, AV_INPUT_BUFFER_PADDING_SIZE * 8)` up.
const MAX_BITS: usize = (i32::MAX as usize) - 64 * 8;

/// A reader over a buffer's bits, most significant first.
#[derive(Clone, Debug)]
pub(crate) struct Bits<'a> {
    /// The bytes read: those of the syntax, and any after it (an RBSP's stop
    /// bit and trailing zeros) that a read running past `size_in_bits` sees.
    buf: &'a [u8],
    /// Bits read from the start.
    index: u32,
    /// The bits the syntax occupies.
    size_in_bits: u32,
    /// Whether reads count past `size_in_bits + 8` (FFmpeg's
    /// `UNCHECKED_BITSTREAM_READER`); see the module documentation.
    unchecked: bool,
}

impl<'a> Bits<'a> {
    /// The first `bit_size` bits of `buf` (`init_get_bits`); `None` for a
    /// size past what FFmpeg's `int` positions can count, which
    /// `init_get_bits` refuses as invalid data.
    pub(crate) fn new(buf: &'a [u8], bit_size: usize) -> Option<Self> {
        if bit_size >= MAX_BITS {
            return None;
        }
        Some(Self {
            buf,
            index: 0,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "under i32::MAX, checked above"
            )]
            size_in_bits: bit_size as u32,
            unchecked: false,
        })
    }

    /// Every bit of `buf` (`init_get_bits8`).
    pub(crate) fn whole(buf: &'a [u8]) -> Option<Self> {
        Self::new(buf, buf.len().saturating_mul(8))
    }

    /// Count reads from here on as `h264_cavlc.c` does (`true`) or as the
    /// parameter-set and header code does (`false`, the default); see the
    /// module documentation.
    pub(crate) fn set_unchecked(&mut self, unchecked: bool) {
        self.unchecked = unchecked;
    }

    /// The bytes being read.
    pub(crate) fn buffer(&self) -> &'a [u8] {
        self.buf
    }

    /// Bits read so far (`get_bits_count`).
    pub(crate) fn count(&self) -> u32 {
        self.index
    }

    /// The syntax's length in bits (`size_in_bits`).
    pub(crate) fn size(&self) -> u32 {
        self.size_in_bits
    }

    /// Bits left to read; negative once a read has run past the end
    /// (`get_bits_left`).
    #[allow(
        clippy::cast_possible_wrap,
        reason = "FFmpeg's (int)(size - index): a position past the end reads negative"
    )]
    pub(crate) fn left(&self) -> i32 {
        self.size_in_bits.wrapping_sub(self.index) as i32
    }

    /// The buffer's size in bytes, its last byte counted whole when
    /// `round_up` (`get_bits_bytesize`).
    pub(crate) fn byte_size(&self, round_up: bool) -> u32 {
        (self.size_in_bits + if round_up { 7 } else { 0 }) >> 3
    }

    /// The byte at `at`, or zero past the end, as FFmpeg's padding reads.
    fn byte(&self, at: usize) -> u8 {
        self.buf.get(at).copied().unwrap_or(0)
    }

    /// `AV_RB32` at the current byte, shifted to the current bit
    /// (`UPDATE_CACHE`): the top `32 - (index & 7)` bits are the stream's,
    /// the rest zero.
    fn cache(&self) -> u32 {
        let at = (self.index >> 3) as usize;
        let word = u32::from_be_bytes([
            self.byte(at),
            self.byte(at + 1),
            self.byte(at + 2),
            self.byte(at + 3),
        ]);
        word << (self.index & 7)
    }

    /// The next 32 bits whole, from a 64-bit load (`UPDATE_CACHE_32`).
    fn cache32(&self) -> u32 {
        let at = (self.index >> 3) as usize;
        let mut bytes = [0u8; 8];
        for (k, b) in bytes.iter_mut().enumerate() {
            *b = self.byte(at + k);
        }
        let word = u64::from_be_bytes(bytes) << (self.index & 7);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the top 32 bits, as FFmpeg keeps them"
        )]
        let top = (word >> 32) as u32;
        top
    }

    /// Move past `n` bits (`SKIP_COUNTER`): no further than eight bits past
    /// the end for the checked reader.
    fn advance(&mut self, n: u32) {
        let to = self.index.wrapping_add(n);
        self.index = if self.unchecked {
            to
        } else {
            to.min(self.size_in_bits + 8)
        };
    }

    /// Read `n` bits, 1 to 25 (`get_bits`).
    pub(crate) fn bits(&mut self, n: u32) -> u32 {
        debug_assert!((1..=MIN_CACHE_BITS).contains(&n));
        let v = self.cache() >> (32 - n);
        self.advance(n);
        v
    }

    /// Read `n` bits, 0 to 25 (`get_bitsz`).
    pub(crate) fn bits_z(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.bits(n) }
    }

    /// Read one bit (`get_bits1`).
    pub(crate) fn bit(&mut self) -> u32 {
        let b = self.byte((self.index >> 3) as usize);
        let v = u32::from(b.wrapping_shl(self.index & 7) >> 7);
        if self.unchecked || self.index < self.size_in_bits + 8 {
            self.index = self.index.wrapping_add(1);
        }
        v
    }

    /// Read one bit as a flag.
    pub(crate) fn flag(&mut self) -> bool {
        self.bit() != 0
    }

    /// Read `n` bits, 0 to 32 (`get_bits_long`).
    pub(crate) fn bits_long(&mut self, n: u32) -> u32 {
        debug_assert!(n <= 32);
        if n == 0 {
            return 0;
        }
        let v = self.cache32() >> (32 - n);
        self.advance(n);
        v
    }

    /// Read `n` bits, 1 to 25, as a two's-complement number (`get_sbits`).
    #[allow(
        clippy::cast_possible_wrap,
        reason = "the bits taken as signed, as NEG_SSR32 does"
    )]
    pub(crate) fn sbits(&mut self, n: u32) -> i32 {
        debug_assert!((1..=MIN_CACHE_BITS).contains(&n));
        let v = (self.cache() as i32) >> (32 - n);
        self.advance(n);
        v
    }

    /// Read `n` bits, 0 to 32, as a two's-complement number
    /// (`get_sbits_long`).
    pub(crate) fn sbits_long(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        sign_extend(self.bits_long(n), n)
    }

    /// The next `n` bits, 1 to 25, without moving (`show_bits`).
    pub(crate) fn show(&self, n: u32) -> u32 {
        debug_assert!((1..=MIN_CACHE_BITS).contains(&n));
        self.cache() >> (32 - n)
    }

    /// The next `n` bits, 0 to 32, without moving (`show_bits_long`).
    pub(crate) fn show_long(&self, n: u32) -> u32 {
        if n <= MIN_CACHE_BITS {
            if n == 0 {
                return 0;
            }
            self.show(n)
        } else {
            self.clone().bits_long(n)
        }
    }

    /// Skip `n` bits (`skip_bits`).
    pub(crate) fn skip(&mut self, n: u32) {
        self.advance(n);
    }

    /// The zeros before the next 1 as the 32-bit cache shows them, read
    /// along with the 1 (`get_level_prefix` in `h264_cavlc.c`). The cache
    /// is the stream's for only its top `32 - (index & 7)` bits, the rest
    /// zero; a cache of zeros reads as 31, having moved 32 bits.
    #[allow(
        clippy::cast_sign_loss,
        reason = "av_log2 of a u32 is 0 to 31, so the count is 1 to 32"
    )]
    pub(crate) fn level_prefix(&mut self) -> i32 {
        let log = 32 - av_log2(self.cache());
        self.advance(log as u32);
        log - 1
    }

    /// Skip `n` bits, back for a negative `n`, no further than the buffer's
    /// start or (checked) eight bits past its end (`skip_bits_long`).
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "positions are under i32::MAX (`new`), as FFmpeg's ints are"
    )]
    pub(crate) fn skip_long(&mut self, n: i32) {
        if self.unchecked {
            self.index = self.index.wrapping_add(n as u32);
        } else {
            let index = self.index as i32;
            let room = (self.size_in_bits + 8) as i32 - index;
            self.index = (index + n.clamp(-index, room)) as u32;
        }
    }

    /// Skip to the next whole byte (`align_get_bits`), answering where that
    /// is, in bytes.
    pub(crate) fn align(&mut self) -> usize {
        let n = self.index.wrapping_neg() & 7;
        if n != 0 {
            self.skip(n);
        }
        (self.index >> 3) as usize
    }

    /// An unsigned Exp-Golomb code of up to 9 bits through `golomb.c`'s
    /// table, and a longer one by counting its leading zeros in the 32-bit
    /// cache: the value, 0 to 8190 (twelve leading zeros), or
    /// [`AVERROR_INVALIDDATA`] for a longer code -- having moved past it as
    /// `get_ue_golomb` does (`32 - log` bits, which for 32 zeros is 63).
    #[allow(
        clippy::cast_possible_wrap,
        reason = "a value under 2^16, as FFmpeg's int"
    )]
    pub(crate) fn ue(&mut self) -> i32 {
        let buf = self.cache();
        if buf >= 1 << 27 {
            let at = (buf >> (32 - 9)) as usize;
            self.advance(u32::from(GOLOMB_VLC_LEN[at]));
            return i32::from(UE_GOLOMB_VLC_CODE[at]);
        }
        let log = 2 * av_log2(buf) - 31;
        #[allow(clippy::cast_sign_loss, reason = "32 - log is 1 to 63")]
        self.advance((32 - log) as u32);
        if log < 7 {
            return AVERROR_INVALIDDATA;
        }
        #[allow(clippy::cast_sign_loss, reason = "log is 7 to 31 here")]
        let v = (buf >> log as u32).wrapping_sub(1);
        v as i32
    }

    /// An unsigned Exp-Golomb code of up to 32 bits of value
    /// (`get_ue_golomb_long`).
    pub(crate) fn ue_long(&mut self) -> u32 {
        let buf = self.show_long(32);
        #[allow(clippy::cast_sign_loss, reason = "31 - av_log2 is 0 to 31")]
        let log = (31 - av_log2(buf)) as u32;
        #[allow(clippy::cast_possible_wrap, reason = "0 to 31")]
        self.skip_long(log as i32);
        self.bits_long(log + 1).wrapping_sub(1)
    }

    /// An unsigned Exp-Golomb code expected to be under 31: through the
    /// 9-bit table alone, whatever follows, so a longer code answers 31 or
    /// more and moves on by the table's length (`get_ue_golomb_31`).
    pub(crate) fn ue_31(&mut self) -> u32 {
        let at = (self.cache() >> (32 - 9)) as usize;
        self.advance(u32::from(GOLOMB_VLC_LEN[at]));
        u32::from(UE_GOLOMB_VLC_CODE[at])
    }

    /// A signed Exp-Golomb code (`get_se_golomb`).
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "FFmpeg's unsigned arithmetic, taken as int at the end"
    )]
    pub(crate) fn se(&mut self) -> i32 {
        let buf = self.cache();
        if buf >= 1 << 27 {
            let at = (buf >> (32 - 9)) as usize;
            self.advance(u32::from(GOLOMB_VLC_LEN[at]));
            return i32::from(SE_GOLOMB_VLC_CODE[at]);
        }
        let log = av_log2(buf) as u32;
        self.advance(31 - log);
        let buf = self.cache() >> log;
        self.advance(32 - log);
        let sign = (buf & 1).wrapping_neg();
        ((buf >> 1) ^ sign).wrapping_sub(sign) as i32
    }

    /// A signed Exp-Golomb code of up to 32 bits (`get_se_golomb_long`).
    #[allow(clippy::cast_possible_wrap, reason = "FFmpeg's int arithmetic")]
    pub(crate) fn se_long(&mut self) -> i32 {
        let buf = self.ue_long();
        let sign = ((buf & 1) as i32).wrapping_sub(1);
        (((buf >> 1) as i32) ^ sign).wrapping_add(1)
    }

    /// A truncated Exp-Golomb code whose range is `range`: one inverted bit
    /// for a range of 2, else [`ue`](Self::ue) (`get_te_golomb`).
    pub(crate) fn te(&mut self, range: u32) -> i32 {
        debug_assert!(range >= 1);
        if range == 2 {
            #[allow(clippy::cast_possible_wrap, reason = "0 or 1")]
            let v = (self.bit() ^ 1) as i32;
            v
        } else {
            self.ue()
        }
    }

    /// [`te`](Self::te), with nothing read for a range of 1
    /// (`get_te0_golomb`).
    pub(crate) fn te0(&mut self, range: u32) -> i32 {
        if range == 1 { 0 } else { self.te(range) }
    }
}

/// The position of `v`'s highest set bit, 0 for 0 (`av_log2`).
#[allow(clippy::cast_possible_wrap, reason = "0 to 31")]
pub(crate) fn av_log2(v: u32) -> i32 {
    (v | 1).ilog2() as i32
}

/// `v`'s low `n` bits, 1 to 32, as a two's-complement number
/// (`sign_extend`).
#[allow(clippy::cast_possible_wrap, reason = "the bits taken as signed")]
pub(crate) fn sign_extend(v: u32, n: u32) -> i32 {
    let shift = 32 - n;
    ((v << shift) as i32) >> shift
}

#[cfg(test)]
mod tests;

// `golomb.c`'s tables, indexed by the next nine bits.

/// How long the code the next nine bits start is (`ff_golomb_vlc_len`); for
/// one longer than nine bits, the length its leading zeros say.
#[rustfmt::skip]
const GOLOMB_VLC_LEN: [u8; 512] = [
    19,17,15,15,13,13,13,13,11,11,11,11,11,11,11,11,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,
    7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,
    5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,
    5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,
    3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,
    3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,
    3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,
    3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
];

/// The unsigned value of the code the next nine bits start
/// (`ff_ue_golomb_vlc_code`); 31 or 32 for one longer than nine bits.
#[rustfmt::skip]
const UE_GOLOMB_VLC_CODE: [u8; 512] = [
    32,32,32,32,32,32,32,32,31,32,32,32,32,32,32,32,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,
     7, 7, 7, 7, 8, 8, 8, 8, 9, 9, 9, 9,10,10,10,10,11,11,11,11,12,12,12,12,13,13,13,13,14,14,14,14,
     3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
     5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
     1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
     1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
     2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
     2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// The signed value of the code the next nine bits start
/// (`ff_se_golomb_vlc_code`); 16 or 17 for one longer than nine bits.
#[rustfmt::skip]
const SE_GOLOMB_VLC_CODE: [i8; 512] = [
     17, 17, 17, 17, 17, 17, 17, 17, 16, 17, 17, 17, 17, 17, 17, 17,  8, -8,  9, -9, 10,-10, 11,-11, 12,-12, 13,-13, 14,-14, 15,-15,
      4,  4,  4,  4, -4, -4, -4, -4,  5,  5,  5,  5, -5, -5, -5, -5,  6,  6,  6,  6, -6, -6, -6, -6,  7,  7,  7,  7, -7, -7, -7, -7,
      2,  2,  2,  2,  2,  2,  2,  2,  2,  2,  2,  2,  2,  2,  2,  2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2,
      3,  3,  3,  3,  3,  3,  3,  3,  3,  3,  3,  3,  3,  3,  3,  3, -3, -3, -3, -3, -3, -3, -3, -3, -3, -3, -3, -3, -3, -3, -3, -3,
      1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,
      1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,
     -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
     -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
];
