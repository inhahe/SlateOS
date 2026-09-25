//! LZW, as libtiff 4.7.1 decodes it (`tif_lzw.c`).
//!
//! Two decoders, as there: the TIFF 6.0 one -- codes most significant bit
//! first, the code width growing one entry early -- and the "old-style" one
//! some pre-6.0 writers used, least significant bit first and growing on
//! time. A strip beginning `00` then a byte with its low bit set is old
//! style; which decoder a file gets is settled by the first strip decoded,
//! and kept for the rest of the file, exactly as libtiff keeps it.
//!
//! Both fill exactly the bytes asked for. A code whose string runs past the
//! end is cut short without complaint -- libtiff would hand the rest to the
//! next call, and a whole-strip decode never makes one -- and running out of
//! codes, or meeting the end-of-information code, before the output is full
//! is an error.

use alloc::vec;
use alloc::vec::Vec;

use crate::{ImageError, ImageResult};

const BITS_MIN: u32 = 9;
const BITS_MAX: u32 = 12;
const CODE_CLEAR: usize = 256;
const CODE_EOI: usize = 257;
const CODE_FIRST: usize = 258;
/// `CODE_FIRST`, as the signed free-entry index.
const FIRST: isize = 258;
/// Table size with libtiff's backwards-compatibility room (`CSIZE`).
const CSIZE: usize = (1 << BITS_MAX) - 1 + 1024;

/// The largest code `bits` bits hold. `bits` is 9 to 12.
const fn max_code(bits: u32) -> usize {
    (1usize << bits).wrapping_sub(1)
}

/// The entry past which a new-style table widens its codes: one before
/// the width's last code.
const fn early(bits: u32) -> usize {
    max_code(bits).wrapping_sub(1)
}

/// One table entry (`code_t`). `next` is the entry for the string without
/// its last byte; `None` for a single byte.
#[derive(Clone, Copy, Default)]
struct Code {
    next: Option<u16>,
    length: u16,
    first: u8,
    value: u8,
    repeated: bool,
}

/// Which decoder the file uses, settled by its first strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    New,
    Old,
}

/// Decoder state that outlives a strip: the table, and the style.
pub(super) struct Lzw {
    table: Vec<Code>,
    style: Option<Style>,
}

impl Lzw {
    pub(super) fn new() -> Self {
        let mut table = vec![Code::default(); CSIZE];
        for (i, code) in table.iter_mut().take(256).enumerate() {
            let byte = u8::try_from(i).unwrap_or(0);
            *code = Code {
                next: None,
                length: 1,
                first: byte,
                value: byte,
                repeated: true,
            };
        }
        Self { table, style: None }
    }

    /// Decode one strip or tile's `raw` bytes into all of `out`.
    ///
    /// # Errors
    ///
    /// [`ImageError::Corrupt`] for a code not yet in the table, or data that
    /// ends before `out` is full.
    pub(super) fn decode(&mut self, raw: &[u8], out: &mut [u8]) -> ImageResult<()> {
        let old_looking = matches!(raw, [0, second, ..] if second & 1 != 0);
        let style = *self
            .style
            .get_or_insert(if old_looking { Style::Old } else { Style::New });
        match style {
            Style::New => self.decode_new(raw, out),
            Style::Old => self.decode_old(raw, out),
        }
    }

    /// Copy the first `n` bytes of `code`'s string to `out` (the string is
    /// stored last byte first, so walk to the prefix of length `n`).
    fn prefix(&self, code: usize, n: usize, out: &mut [u8]) -> ImageResult<()> {
        let mut at = Some(u16::try_from(code).map_err(|_| corrupt())?);
        // Step back until the entry's string is no longer than `n`.
        loop {
            let entry = self.entry(at)?;
            if usize::from(entry.length) <= n {
                break;
            }
            at = entry.next;
        }
        for slot in out.iter_mut().take(n).rev() {
            let entry = self.entry(at)?;
            *slot = entry.value;
            at = entry.next;
        }
        Ok(())
    }

    fn entry(&self, at: Option<u16>) -> ImageResult<Code> {
        at.and_then(|i| self.table.get(usize::from(i)).copied())
            .ok_or_else(corrupt)
    }

    /// Write `code`'s whole string (length `len`) into `out[..len]`.
    fn string(&self, code: usize, len: usize, out: &mut [u8]) -> ImageResult<()> {
        let mut at = Some(u16::try_from(code).map_err(|_| corrupt())?);
        for slot in out.iter_mut().take(len).rev() {
            let entry = self.entry(at)?;
            *slot = entry.value;
            at = entry.next;
        }
        Ok(())
    }

    /// `LZWDecode`.
    fn decode_new(&mut self, raw: &[u8], out: &mut [u8]) -> ImageResult<()> {
        let mut bits = MsbReader::new(raw);
        let mut nbits = BITS_MIN;
        // Index of the next free entry; -1 before the first clear code, when
        // no entry may be added.
        let mut free: isize = -1;
        let mut max_entry = early(BITS_MIN);
        let mut old: usize = 0;
        let mut op = 0usize;
        let occ = out.len();
        if occ == 0 {
            return Ok(());
        }
        loop {
            let code = bits
                .read(nbits)
                .ok_or(ImageError::Corrupt("LZW data without an end code"))?;
            if code == CODE_CLEAR {
                free = FIRST;
                nbits = BITS_MIN;
                max_entry = early(BITS_MIN);
                let mut next = bits
                    .read(nbits)
                    .ok_or(ImageError::Corrupt("LZW data without an end code"))?;
                while next == CODE_CLEAR {
                    next = bits
                        .read(nbits)
                        .ok_or(ImageError::Corrupt("LZW data without an end code"))?;
                }
                if next == CODE_EOI {
                    return Err(short());
                }
                if next > CODE_EOI {
                    return Err(corrupt());
                }
                *out.get_mut(op).ok_or_else(corrupt)? =
                    u8::try_from(next).map_err(|_| corrupt())?;
                op = op.wrapping_add(1);
                old = next;
                if op == occ {
                    return Ok(());
                }
                continue;
            }
            if code == CODE_EOI {
                return Err(short());
            }
            let old_entry = self.table.get(old).copied().ok_or_else(corrupt)?;
            if code < 256 {
                // A single byte: add old + byte.
                if isize::try_from(code).unwrap_or(isize::MAX) > free {
                    return Err(corrupt());
                }
                let slot = usize::try_from(free).map_err(|_| corrupt())?;
                let byte = u8::try_from(code).map_err(|_| corrupt())?;
                *self.table.get_mut(slot).ok_or_else(corrupt)? = Code {
                    next: u16::try_from(old).ok(),
                    first: old_entry.first,
                    length: old_entry.length.saturating_add(1),
                    value: byte,
                    repeated: old_entry.repeated && old_entry.value == byte,
                };
                grow(&mut free, &mut nbits, &mut max_entry);
                old = code;
                *out.get_mut(op).ok_or_else(corrupt)? = byte;
                op = op.wrapping_add(1);
                if op == occ {
                    return Ok(());
                }
                continue;
            }
            // A code for a string.
            let slot = usize::try_from(free).map_err(|_| corrupt())?;
            let value = if code >= slot {
                if code != slot {
                    return Err(corrupt());
                }
                old_entry.first
            } else {
                self.table.get(code).map(|e| e.first).ok_or_else(corrupt)?
            };
            *self.table.get_mut(slot).ok_or_else(corrupt)? = Code {
                next: u16::try_from(old).ok(),
                first: old_entry.first,
                length: old_entry.length.saturating_add(1),
                value,
                repeated: old_entry.repeated && old_entry.value == value,
            };
            grow(&mut free, &mut nbits, &mut max_entry);
            old = code;
            let entry = self.table.get(code).copied().ok_or_else(corrupt)?;
            let len = usize::from(entry.length);
            let left = occ.saturating_sub(op);
            let dest = out.get_mut(op..).ok_or_else(corrupt)?;
            if len > left {
                // Too long for what is left: the start of it fills the rest.
                self.prefix(code, left, dest)?;
                return Ok(());
            }
            if entry.repeated {
                dest.get_mut(..len).ok_or_else(corrupt)?.fill(entry.value);
            } else {
                self.string(code, len, dest)?;
            }
            op = op.wrapping_add(len);
            if op == occ {
                return Ok(());
            }
        }
    }

    /// `LZWDecodeCompat`: old-style codes, least significant bit first.
    fn decode_old(&mut self, raw: &[u8], out: &mut [u8]) -> ImageResult<()> {
        let mut bits = LsbReader::new(raw);
        let mut nbits = BITS_MIN;
        let mut free: isize = -1;
        // Nothing is added before the first clear code, which resets this.
        let mut max_entry = early(BITS_MIN);
        let mut old: usize = 0;
        let mut op = 0usize;
        let occ = out.len();
        while op < occ {
            // Running out of bits reads as the end code.
            let Some(mut code) = bits.read(nbits) else {
                break;
            };
            if code == CODE_EOI {
                break;
            }
            if code == CODE_CLEAR {
                loop {
                    free = FIRST;
                    for entry in self.table.iter_mut().skip(CODE_FIRST) {
                        *entry = Code::default();
                    }
                    nbits = BITS_MIN;
                    max_entry = max_code(BITS_MIN);
                    match bits.read(nbits) {
                        Some(c) => code = c,
                        None => {
                            code = CODE_EOI;
                            break;
                        }
                    }
                    if code != CODE_CLEAR {
                        break;
                    }
                }
                if code == CODE_EOI {
                    break;
                }
                if code > CODE_CLEAR {
                    return Err(corrupt());
                }
                *out.get_mut(op).ok_or_else(corrupt)? =
                    u8::try_from(code).map_err(|_| corrupt())?;
                op = op.wrapping_add(1);
                old = code;
                continue;
            }
            let slot = usize::try_from(free).map_err(|_| corrupt())?;
            if slot >= CSIZE {
                return Err(corrupt());
            }
            let old_entry = self.table.get(old).copied().ok_or_else(corrupt)?;
            let value = if code < slot {
                self.table.get(code).map(|e| e.first).ok_or_else(corrupt)?
            } else {
                old_entry.first
            };
            *self.table.get_mut(slot).ok_or_else(corrupt)? = Code {
                next: u16::try_from(old).ok(),
                first: old_entry.first,
                length: old_entry.length.saturating_add(1),
                value,
                repeated: false,
            };
            free = free.wrapping_add(1);
            if usize::try_from(free).unwrap_or(0) > max_entry {
                nbits = nbits.saturating_add(1).min(BITS_MAX);
                max_entry = max_code(nbits);
            }
            old = code;
            if code >= 256 {
                let entry = self.table.get(code).copied().ok_or_else(corrupt)?;
                let len = usize::from(entry.length);
                if len == 0 {
                    return Err(corrupt());
                }
                let left = occ.saturating_sub(op);
                let dest = out.get_mut(op..).ok_or_else(corrupt)?;
                if len > left {
                    self.prefix(code, left, dest)?;
                    op = occ;
                    break;
                }
                self.string_until_end(code, len, dest)?;
                op = op.wrapping_add(len);
            } else {
                *out.get_mut(op).ok_or_else(corrupt)? =
                    u8::try_from(code).map_err(|_| corrupt())?;
                op = op.wrapping_add(1);
            }
        }
        if op < occ {
            return Err(short());
        }
        Ok(())
    }

    /// As [`Self::string`], but stopping early at the end of the chain, as
    /// the old-style decoder's loop does (`while (codep && tp > op)`).
    fn string_until_end(&self, code: usize, len: usize, out: &mut [u8]) -> ImageResult<()> {
        let mut at = u16::try_from(code).ok();
        for slot in out.iter_mut().take(len).rev() {
            let Some(i) = at else { break };
            let entry = self
                .table
                .get(usize::from(i))
                .copied()
                .ok_or_else(corrupt)?;
            *slot = entry.value;
            at = entry.next;
        }
        Ok(())
    }
}

/// Step the free pointer past a new entry, widening codes as the new-style
/// decoder does: one entry before the width's last code.
fn grow(free: &mut isize, nbits: &mut u32, max_entry: &mut usize) {
    // At most CSIZE: no overflow.
    *free = free.wrapping_add(1);
    if usize::try_from(*free).unwrap_or(0) > *max_entry {
        *nbits = nbits.saturating_add(1).min(BITS_MAX);
        *max_entry = early(*nbits);
        if usize::try_from(*free).unwrap_or(0) >= CSIZE {
            // Full: only a clear or end code may follow.
            *free = -1;
        }
    }
}

fn corrupt() -> ImageError {
    ImageError::Corrupt("LZW code not yet in the table")
}

fn short() -> ImageError {
    ImageError::Corrupt("LZW data ends before the strip does")
}

/// Codes most significant bit first, in whole bytes: a code the remaining
/// bytes cannot complete is not read.
struct MsbReader<'a> {
    data: &'a [u8],
    at: usize,
    buffer: u64,
    count: u32,
}

impl<'a> MsbReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            buffer: 0,
            count: 0,
        }
    }

    fn read(&mut self, bits: u32) -> Option<usize> {
        // At most 12 bits are wanted, so the buffer holds under 20.
        while self.count < bits {
            let byte = *self.data.get(self.at)?;
            self.at = self.at.wrapping_add(1);
            self.buffer = (self.buffer << 8) | u64::from(byte);
            self.count = self.count.wrapping_add(8);
        }
        self.count = self.count.wrapping_sub(bits);
        let code = (self.buffer >> self.count) & (1u64 << bits).wrapping_sub(1);
        usize::try_from(code).ok()
    }
}

/// Codes least significant bit first; `None` when fewer bits remain than a
/// code needs.
struct LsbReader<'a> {
    data: &'a [u8],
    at: usize,
    buffer: u64,
    count: u32,
    left: u64,
}

impl<'a> LsbReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            buffer: 0,
            count: 0,
            left: (data.len() as u64).saturating_mul(8),
        }
    }

    fn read(&mut self, bits: u32) -> Option<usize> {
        self.left = self.left.checked_sub(u64::from(bits))?;
        // At most 12 bits are wanted, so the buffer holds under 20.
        while self.count < bits {
            let byte = self.data.get(self.at).copied().unwrap_or(0);
            self.at = self.at.wrapping_add(1);
            self.buffer |= u64::from(byte) << self.count;
            self.count = self.count.wrapping_add(8);
        }
        let code = self.buffer & (1u64 << bits).wrapping_sub(1);
        self.buffer >>= bits;
        self.count = self.count.wrapping_sub(bits);
        usize::try_from(code).ok()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// Pack codes most significant bit first at the widths the new-style
    /// decoder reads them.
    fn pack_msb(codes: &[(usize, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut acc, mut n) = (0u64, 0u32);
        for &(code, bits) in codes {
            acc = (acc << bits) | code as u64;
            n += bits;
            while n >= 8 {
                n -= 8;
                out.push((acc >> n) as u8);
            }
        }
        if n > 0 {
            out.push((acc << (8 - n)) as u8);
        }
        out
    }

    #[test]
    fn a_strip_decodes_its_strings() {
        // CLEAR, a, b, 258 = "ab", 260 = "aba" (the code being defined), EOI.
        let data = pack_msb(&[(256, 9), (97, 9), (98, 9), (258, 9), (260, 9), (257, 9)]);
        let mut out = [0u8; 7];
        Lzw::new().decode(&data, &mut out).unwrap();
        assert_eq!(&out, b"abababa");
    }

    #[test]
    fn a_strip_that_does_not_start_with_clear_is_refused() {
        let data = pack_msb(&[(97, 9), (257, 9)]);
        let mut out = [0u8; 1];
        assert!(Lzw::new().decode(&data, &mut out).is_err());
    }

    #[test]
    fn the_end_code_before_the_strip_is_full_is_an_error() {
        let data = pack_msb(&[(256, 9), (97, 9), (257, 9)]);
        let mut out = [0u8; 2];
        assert!(Lzw::new().decode(&data, &mut out).is_err());
    }

    #[test]
    fn a_full_strip_needs_no_end_code_and_a_long_string_is_cut() {
        let data = pack_msb(&[(256, 9), (97, 9), (98, 9), (258, 9)]);
        let mut out = [0u8; 3];
        Lzw::new().decode(&data, &mut out).unwrap();
        assert_eq!(&out, b"aba");
    }
}
