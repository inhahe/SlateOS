//! Variable-length codes read through lookup tables, built and read as
//! FFmpeg builds and reads them (`ff_vlc_init_sparse` and `build_table` in
//! `vlc.c`, `get_vlc2` in `get_bits.h`).
//!
//! A code is looked up by its first `bits` bits; a longer one through a
//! second table its first table's entry points to, and so on. How a table
//! is laid out decides what a bit pattern that is no code reads as -- how
//! many bits it takes, and the -1 it answers -- which is why the tables are
//! built FFmpeg's way rather than any way that decodes the valid codes.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "code lengths are at most 32 and tables at most a few thousand entries"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "a code's entries lie in the table built for its prefix: an index under 2^bits \
              from that table's start, which the table was resized to hold"
)]

use crate::bits::Bits;

/// One table entry (`VLCElem`): the symbol and how many bits it takes, or
/// -- with a negative length -- where a subtable starts and how many bits
/// index it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Elem {
    sym: i32,
    len: i32,
}

/// A code being placed (`VLCcode`): its bits left-aligned in 32.
#[derive(Clone, Copy, Debug)]
struct Code {
    bits: u32,
    code: u32,
    symbol: i32,
}

/// A code's tables (`VLC`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vlc {
    table: Vec<Elem>,
    bits: u32,
}

/// Why a code could not be tabled: two codes on one pattern, or a code
/// longer than its value allows (FFmpeg's "incorrect codes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VlcError;

impl Vlc {
    /// The tables for `lens[i]`-bit codes `codes[i]` of symbol `i`, or of
    /// `symbols[i]` where given, looked up `bits` bits at a time. A length
    /// of 0 is no code.
    pub fn new(
        bits: u32,
        lens: &[u8],
        codes: &[u32],
        symbols: Option<&[i32]>,
    ) -> Result<Self, VlcError> {
        let mut buf = Vec::with_capacity(lens.len());
        let push = |buf: &mut Vec<Code>, long: bool| -> Result<(), VlcError> {
            for (i, (&len, &code)) in lens.iter().zip(codes).enumerate() {
                let len = u32::from(len);
                let wanted = if long {
                    len > bits
                } else {
                    len != 0 && len <= bits
                };
                if !wanted {
                    continue;
                }
                if len > 3 * bits || len > 32 || u64::from(code) >= 1u64 << len {
                    return Err(VlcError);
                }
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "symbol indices are small"
                )]
                let symbol = symbols.map_or(i as i32, |s| s.get(i).copied().unwrap_or(0));
                buf.push(Code {
                    bits: len,
                    code: code << (32 - len),
                    symbol,
                });
            }
            Ok(())
        };
        // The codes longer than a lookup first, in code order, as their
        // subtables need them grouped by prefix; then the rest.
        push(&mut buf, true)?;
        buf.sort_by_key(|c| c.code >> 1);
        push(&mut buf, false)?;

        let mut vlc = Self {
            table: Vec::new(),
            bits,
        };
        vlc.build(bits, &mut buf)?;
        Ok(vlc)
    }

    /// The tables for codes given by their lengths alone
    /// (`ff_vlc_init_from_lengths`): each the next code after the one
    /// before, so shorter codes come first; symbol `symbols[i]` (or `i`),
    /// plus `offset`, looked up `bits` bits at a time. A negative length is
    /// a code no symbol has, which takes its place in the order; 0 is none.
    ///
    /// # Errors
    ///
    /// A length longer than three lookups or 32 bits, or lengths that do
    /// not make a prefix code -- a code that would start inside a shorter
    /// one's, or more codes than the lengths have room for.
    pub fn from_lengths(
        bits: u32,
        lens: &[i8],
        symbols: Option<&[i32]>,
        offset: i32,
    ) -> Result<Self, VlcError> {
        let len_max = (3 * bits).min(32);
        let mut buf = Vec::with_capacity(lens.len());
        let mut code = 0u64;
        for (i, &len) in lens.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let n = u32::from(len.unsigned_abs());
            if len > 0 {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "symbol indices are small"
                )]
                let symbol = symbols.map_or(i as i32, |s| s.get(i).copied().unwrap_or(0));
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "FFmpeg's uint32_t code: past 2^32 the tree is refused below"
                )]
                buf.push(Code {
                    bits: n,
                    code: code as u32,
                    symbol: symbol.wrapping_add(offset),
                });
            }
            if n > len_max || code & ((1u64 << (32 - n)) - 1) != 0 {
                return Err(VlcError);
            }
            code += 1u64 << (32 - n);
            if code > 1u64 << 32 {
                return Err(VlcError);
            }
        }
        let mut vlc = Self {
            table: Vec::new(),
            bits,
        };
        vlc.build(bits, &mut buf)?;
        Ok(vlc)
    }

    /// A table of `2^table_bits` entries for `codes`, after any already
    /// built; its start (`build_table`).
    fn build(&mut self, table_bits: u32, codes: &mut [Code]) -> Result<usize, VlcError> {
        let table_size = 1usize << table_bits;
        let start = self.table.len();
        self.table.resize(start + table_size, Elem::default());

        let mut i = 0;
        while i < codes.len() {
            let Code {
                bits: n,
                code,
                symbol,
            } = codes[i];
            if n <= table_bits {
                let j = (code >> (32 - table_bits)) as usize;
                let nb = 1usize << (table_bits - n);
                #[allow(clippy::cast_possible_wrap, reason = "at most 32")]
                let n_i = n as i32;
                for k in 0..nb {
                    let e = &mut self.table[start + j + k];
                    if (e.len != 0 || e.sym != 0) && (e.len != n_i || e.sym != symbol) {
                        return Err(VlcError);
                    }
                    *e = Elem {
                        sym: symbol,
                        len: n_i,
                    };
                }
                i += 1;
            } else {
                let n = n - table_bits;
                let prefix = code >> (32 - table_bits);
                let mut subtable_bits = n;
                codes[i].bits = n;
                codes[i].code = code << table_bits;
                let mut k = i + 1;
                while k < codes.len() {
                    let Some(m) = codes[k].bits.checked_sub(table_bits).filter(|&m| m > 0) else {
                        break;
                    };
                    if codes[k].code >> (32 - table_bits) != prefix {
                        break;
                    }
                    codes[k].bits = m;
                    codes[k].code <<= table_bits;
                    subtable_bits = subtable_bits.max(m);
                    k += 1;
                }
                let subtable_bits = subtable_bits.min(table_bits);
                let j = start + prefix as usize;
                #[allow(clippy::cast_possible_wrap, reason = "at most 32")]
                {
                    self.table[j].len = -(subtable_bits as i32);
                }
                let index = self.build(subtable_bits, &mut codes[i..k])?;
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "tables are small"
                )]
                {
                    self.table[j].sym = index as i32;
                }
                i = k;
            }
        }
        for e in &mut self.table[start..start + table_size] {
            if e.len == 0 {
                e.sym = -1;
            }
        }
        Ok(start)
    }

    /// The next code's symbol, or -1 for a pattern that is none
    /// (`get_vlc2`): looked up at most `max_depth` tables deep.
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "FFmpeg's index and length arithmetic"
    )]
    pub fn read(&self, gb: &mut Bits<'_>, max_depth: u32) -> i32 {
        let mut index = gb.show(self.bits) as usize;
        let mut e = self
            .table
            .get(index)
            .copied()
            .unwrap_or(Elem { sym: -1, len: 0 });
        let mut nb_bits = self.bits;
        let mut depth = 1;
        while depth < max_depth && e.len < 0 {
            gb.skip(nb_bits);
            nb_bits = (-e.len) as u32;
            index = gb.show(nb_bits) as usize + e.sym as usize;
            e = self
                .table
                .get(index)
                .copied()
                .unwrap_or(Elem { sym: -1, len: 0 });
            depth += 1;
        }
        gb.skip_long(e.len);
        e.sym
    }
}

#[cfg(test)]
mod tests;
