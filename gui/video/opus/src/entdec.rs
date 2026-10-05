//! The range decoder every Opus layer reads its symbols with (RFC 6716 §4.1):
//! symbols coded against cumulative frequencies, read from the front of the
//! frame, and raw bits read from its back.
//!
//! Translated into Rust from libopus 1.5.2's `celt/entdec.c` and
//! `celt/entcode.c` (the decoder's half: `ec_tell_frac` and `ec_ilog`),
//! copyright Xiph.Org and the contributors named in its `COPYING`, used under
//! libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "the range coder's state stays within 32 bits by construction (the range is renormalised above 2^23 and below 2^32; values are reduced modulo 2^31); its additions and shifts are libopus's own, and the bit counts are those of a packet of at most 1275 bytes"
)]

/// Bits a symbol carries in the range coder's base.
const SYM_BITS: u32 = 8;
/// The width of the range's register.
const CODE_BITS: u32 = 32;
const SYM_MAX: u32 = (1 << SYM_BITS) - 1;
const CODE_TOP: u32 = 1 << (CODE_BITS - 1);
const CODE_BOT: u32 = CODE_TOP >> SYM_BITS;
const CODE_EXTRA: u32 = (CODE_BITS - 2) % SYM_BITS + 1;
/// Bits in the window raw bits are read through.
const WINDOW_SIZE: i32 = 32;
/// Above this many bits, `uint` reads the low bits raw.
const UINT_BITS: i32 = 8;
/// Fractional bits `tell_frac` counts in: eighths.
pub(crate) const BITRES: u32 = 3;

/// `EC_ILOG`: the number of bits needed to write `v`; 0 for 0.
pub(crate) const fn ilog(v: u32) -> i32 {
    // At most 32: no overflow.
    (u32::BITS - v.leading_zeros()) as i32
}

/// A frame being decoded: libopus's `ec_dec`.
#[derive(Clone, Debug)]
pub(crate) struct Decoder<'a> {
    buf: &'a [u8],
    /// How many of `buf`'s bytes belong to this decoder.
    pub(crate) storage: u32,
    /// How many bytes the raw bits have taken from the end.
    end_offs: u32,
    /// Raw bits read from the end and not yet handed out.
    end_window: u32,
    /// How many of `end_window`'s bits are valid.
    nend_bits: i32,
    /// Whole bits read so far, not counting the range's partial bits.
    nbits_total: i32,
    /// The next byte the range coder reads.
    offs: u32,
    /// The current range.
    rng: u32,
    /// The top of the range less the input value, less one.
    val: u32,
    /// `decode`'s normalisation, for the `update` after it.
    ext: u32,
    /// The input byte whose low bits are not yet used.
    rem: u32,
    /// Set when a value read was out of its range.
    error: bool,
}

impl<'a> Decoder<'a> {
    /// `ec_dec_init`: a decoder over all of `buf`.
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        let storage = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let mut d = Self {
            buf,
            storage,
            end_offs: 0,
            end_window: 0,
            nend_bits: 0,
            nbits_total: (CODE_BITS + 1 - ((CODE_BITS - CODE_EXTRA) / SYM_BITS) * SYM_BITS) as i32,
            offs: 0,
            rng: 1 << CODE_EXTRA,
            val: 0,
            ext: 0,
            rem: 0,
            error: false,
        };
        d.rem = d.read_byte();
        d.val = d.rng - 1 - (d.rem >> (SYM_BITS - CODE_EXTRA));
        d.normalize();
        d
    }

    /// The next byte from the front, or 0 past the end.
    fn read_byte(&mut self) -> u32 {
        if self.offs < self.storage {
            let b = self.byte(self.offs);
            self.offs += 1;
            b
        } else {
            0
        }
    }

    /// The next byte from the back, or 0 past the front.
    fn read_byte_from_end(&mut self) -> u32 {
        if self.end_offs < self.storage {
            self.end_offs += 1;
            self.byte(self.storage - self.end_offs)
        } else {
            0
        }
    }

    fn byte(&self, at: u32) -> u32 {
        usize::try_from(at)
            .ok()
            .and_then(|i| self.buf.get(i))
            .map_or(0, |&b| u32::from(b))
    }

    /// `ec_dec_normalize`: bring the range back above 2^23, reading bytes.
    fn normalize(&mut self) {
        while self.rng <= CODE_BOT {
            self.nbits_total += SYM_BITS as i32;
            self.rng <<= SYM_BITS;
            let mut sym = self.rem;
            self.rem = self.read_byte();
            sym = ((sym << SYM_BITS) | self.rem) >> (SYM_BITS - CODE_EXTRA);
            self.val = ((self.val << SYM_BITS).wrapping_add(SYM_MAX & !sym)) & (CODE_TOP - 1);
        }
    }

    /// `ec_decode`: the cumulative frequency the next symbol of a total of
    /// `ft` falls at; `update` must follow.
    pub(crate) fn decode(&mut self, ft: u32) -> u32 {
        // Every caller's total is at least 2 and at most 2^16, and the range
        // at least 2^23, so neither division is by 0; the fallbacks only keep
        // a broken caller from panicking.
        self.ext = self.rng.checked_div(ft).unwrap_or(1);
        let s = self.val.checked_div(self.ext).unwrap_or(u32::MAX);
        ft - s.saturating_add(1).min(ft)
    }

    /// `ec_decode_bin`: `decode` with a total of `2^bits`.
    pub(crate) fn decode_bin(&mut self, bits: u32) -> u32 {
        self.ext = self.rng.checked_shr(bits).unwrap_or(0).max(1);
        let s = self.val / self.ext;
        let total = 1u32.checked_shl(bits).unwrap_or(u32::MAX);
        total - s.saturating_add(1).min(total)
    }

    /// `ec_dec_update`: take the symbol spanning `[fl, fh)` of `ft`.
    pub(crate) fn update(&mut self, fl: u32, fh: u32, ft: u32) {
        let s = self.ext.wrapping_mul(ft - fh);
        self.val = self.val.wrapping_sub(s);
        self.rng = if fl > 0 {
            self.ext.wrapping_mul(fh - fl)
        } else {
            self.rng.wrapping_sub(s)
        };
        self.normalize();
    }

    /// `ec_dec_bit_logp`: a bit whose probability of being 1 is `1/2^logp`.
    pub(crate) fn bit_logp(&mut self, logp: u32) -> bool {
        let r = self.rng;
        let d = self.val;
        let s = r >> logp;
        let ret = d < s;
        if !ret {
            self.val = d - s;
        }
        self.rng = if ret { s } else { r - s };
        self.normalize();
        ret
    }

    /// `ec_dec_icdf`: a symbol by its inverse cumulative table, in `2^ftb`.
    pub(crate) fn icdf(&mut self, icdf: &[u8], ftb: u32) -> usize {
        self.icdf_with(ftb, |i| icdf.get(i).map_or(0, |&v| u32::from(v)))
    }

    fn icdf_with(&mut self, ftb: u32, at: impl Fn(usize) -> u32) -> usize {
        let mut s = self.rng;
        let d = self.val;
        let r = s >> ftb;
        let mut ret = 0usize;
        let mut t;
        loop {
            t = s;
            s = r.wrapping_mul(at(ret));
            if d >= s {
                break;
            }
            ret += 1;
        }
        self.val = d - s;
        self.rng = t - s;
        self.normalize();
        ret
    }

    /// `ec_dec_uint`: a value uniformly in `0..ft`; `ft` must be above 1.
    pub(crate) fn uint(&mut self, ft: u32) -> u32 {
        let ft1 = ft.wrapping_sub(1);
        let mut ftb = ilog(ft1);
        if ftb > UINT_BITS {
            ftb -= UINT_BITS;
            let top = (ft1 >> ftb) + 1;
            let s = self.decode(top);
            self.update(s, s + 1, top);
            let t = (s << ftb) | self.bits(ftb as u32);
            if t <= ft1 {
                return t;
            }
            self.error = true;
            ft1
        } else {
            let s = self.decode(ft);
            self.update(s, s + 1, ft);
            s
        }
    }

    /// `ec_dec_bits`: `bits` raw bits, from the end of the frame.
    pub(crate) fn bits(&mut self, bits: u32) -> u32 {
        let mut window = self.end_window;
        let mut available = self.nend_bits;
        if (available as u32) < bits {
            loop {
                window |= self.read_byte_from_end() << available;
                available += SYM_BITS as i32;
                if available > WINDOW_SIZE - SYM_BITS as i32 {
                    break;
                }
            }
        }
        let ret = window & 1u32.checked_shl(bits).unwrap_or(0).wrapping_sub(1);
        window = window.checked_shr(bits).unwrap_or(0);
        available -= bits as i32;
        self.end_window = window;
        self.nend_bits = available;
        self.nbits_total += bits as i32;
        ret
    }

    /// `ec_tell`: whole bits read so far, rounded up.
    pub(crate) fn tell(&self) -> i32 {
        self.nbits_total - ilog(self.rng)
    }

    /// `ec_tell_frac`: bits read so far, in eighths, rounded up.
    pub(crate) fn tell_frac(&self) -> u32 {
        const CORRECTION: [u32; 8] = [35733, 38967, 42495, 46340, 50535, 55109, 60097, 65535];
        let nbits = (self.nbits_total as u32) << BITRES;
        let mut l = ilog(self.rng);
        // The range is at least 2^23 once normalised: `l` is 24 or more.
        let r = self.rng >> (l - 16).max(0);
        let mut b = (r >> 12).wrapping_sub(8);
        b += u32::from(r > CORRECTION.get(b as usize).copied().unwrap_or(u32::MAX));
        l = (l << 3) + b as i32;
        nbits.wrapping_sub(l as u32)
    }

    /// Count bits as read until [`Self::tell`] reports `bits` (a silent CELT
    /// frame's `dec->nbits_total += tell - ec_tell(dec)`: the frame's
    /// remaining bits treated as spent).
    pub(crate) fn pretend_read(&mut self, bits: i32) {
        self.nbits_total = self
            .nbits_total
            .wrapping_add(bits.wrapping_sub(self.tell()));
    }

    /// Whether a value read was out of its range.
    pub(crate) fn error(&self) -> bool {
        self.error
    }

    /// The final range: the check every Opus decoder's output is held to
    /// (`OPUS_GET_FINAL_RANGE`).
    pub(crate) fn range(&self) -> u32 {
        self.rng
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn ilog_counts_bits() {
        assert_eq!(ilog(0), 0);
        assert_eq!(ilog(1), 1);
        assert_eq!(ilog(255), 8);
        assert_eq!(ilog(256), 9);
        assert_eq!(ilog(u32::MAX), 32);
    }

    #[test]
    fn a_fresh_decoder_has_read_one_bit() {
        // As libopus documents it: a newly initialised decoder claims one bit.
        let d = Decoder::new(&[0x55; 8]);
        assert_eq!(d.tell(), 1);
        assert_eq!(d.tell_frac(), 8);
        let empty = Decoder::new(&[]);
        assert_eq!(empty.tell(), 1);
    }

    #[test]
    fn raw_bits_come_from_the_end_least_significant_first() {
        let mut d = Decoder::new(&[0, 0, 0, 0b1010_0101]);
        assert_eq!(d.bits(1), 1);
        assert_eq!(d.bits(2), 0b10);
        assert_eq!(d.bits(5), 0b10100);
        assert_eq!(d.tell(), 9);
    }
}
