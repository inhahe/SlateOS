//! The decoder this crate had until 2026-09-26 -- a port of zlib's `puff.c`,
//! which walks the code lengths a bit at a time -- kept, test-only, as the
//! oracle the table-driven decoder in `inflate.rs` is held to.
//!
//! It is the old code verbatim but for one change: [`inflate_partial`] returns
//! the bytes produced before a failure alongside it, which the one-shot
//! decoder discarded and the stream delivered. That is what lets one oracle
//! judge both: the new one-shot decoder must give its bytes or its error, and
//! the new stream must deliver exactly these bytes and then this error, at any
//! read size.
//!
//! Slow on purpose, and simple enough to check by reading: "the same bytes out,
//! the same error at the same byte" is a claim about the decoder being
//! replaced, so the decoder being replaced is the thing to compare with.

use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::{
    CL_ORDER, DIST_BASE, DIST_EXTRA, Error, LENGTH_BASE, LENGTH_EXTRA, MAX_BITS, MAX_CL_CODES,
    MAX_DIST_CODES, MAX_LIT_CODES, Result, fixed_dist_lengths, fixed_lit_lengths,
};

/// Inflate `data` as the walk did, producing at most `limit` bytes: every byte
/// produced, and the error that stopped it, if one did.
pub(crate) fn inflate_partial(data: &[u8], limit: usize) -> (Vec<u8>, Option<Error>) {
    let mut reader = BitReader::new(data);
    let mut output = Vec::new();
    let end = (|| -> Result<()> {
        loop {
            let bfinal = reader.read_bits(1)?;
            let btype = reader.read_bits(2)?;
            match btype {
                0 => inflate_stored(&mut reader, &mut output, limit)?,
                1 => inflate_fixed(&mut reader, &mut output, limit)?,
                2 => inflate_dynamic(&mut reader, &mut output, limit)?,
                _ => return Err(Error::ReservedBlockType),
            }
            if bfinal != 0 {
                return Ok(());
            }
        }
    })();
    (output, end.err())
}

/// Reads bits from a byte buffer, least-significant-bit first.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize, // byte position
    bit: u8,    // bit position within current byte (0-7)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit: 0,
        }
    }

    /// Read `n` bits (1..=25) and return as u32 (LSB first).
    fn read_bits(&mut self, n: u8) -> Result<u32> {
        let mut val = 0u32;
        for i in 0..n {
            let Some(&byte) = self.data.get(self.pos) else {
                return Err(Error::UnexpectedEnd);
            };
            let b = (byte >> self.bit) & 1;
            val |= u32::from(b) << i;
            self.bit = self.bit.wrapping_add(1);
            if self.bit >= 8 {
                self.bit = 0;
                self.pos = self.pos.wrapping_add(1);
            }
        }
        Ok(val)
    }

    /// Align to the next byte boundary (discard remaining bits).
    fn align(&mut self) {
        if self.bit > 0 {
            self.bit = 0;
            self.pos = self.pos.wrapping_add(1);
        }
    }

    /// Read a raw byte at the current byte position (must be aligned).
    fn read_byte(&mut self) -> Result<u8> {
        let Some(&b) = self.data.get(self.pos) else {
            return Err(Error::UnexpectedEnd);
        };
        self.pos = self.pos.wrapping_add(1);
        Ok(b)
    }

    /// Read a 16-bit little-endian value (must be aligned).
    fn read_u16_le(&mut self) -> Result<u16> {
        let lo = self.read_byte()?;
        let hi = self.read_byte()?;
        Ok(u16::from(lo) | (u16::from(hi) << 8))
    }
}

/// A Huffman decode table built from a set of code lengths.
///
/// Uses a two-level lookup: codes up to `MAX_BITS` are stored in a
/// flat table indexed by reversed bit pattern.  For a kernel where
/// memory is limited, we use the "counts + symbols" approach from
/// puff.c which is compact and fast.
struct HuffmanTable {
    /// Number of codes of each length (index = length, 0..=MAX_BITS).
    counts: [u16; MAX_BITS + 1],
    /// Symbols sorted by code, then by symbol value.
    symbols: [u16; MAX_LIT_CODES + MAX_DIST_CODES],
    /// Number of valid symbols.
    num_symbols: usize,
}

impl HuffmanTable {
    const fn empty() -> Self {
        Self {
            counts: [0; MAX_BITS + 1],
            symbols: [0; MAX_LIT_CODES + MAX_DIST_CODES],
            num_symbols: 0,
        }
    }

    /// Build a Huffman table from an array of code lengths.
    ///
    /// `lengths[i]` is the code length for symbol `i`.  A length of 0
    /// means the symbol is not present in the alphabet.
    fn build(lengths: &[u8]) -> Result<Self> {
        let mut table = Self::empty();
        table.num_symbols = lengths.len();

        // Count the number of codes for each code length. The `get_mut` is
        // the length check as well as the bounds check: `counts` has exactly
        // `MAX_BITS + 1` slots, and DEFLATE cannot represent a code longer
        // than `MAX_BITS`, so a length that misses the array came from a
        // malformed stream rather than an unusual one.
        for &len in lengths {
            let slot = table
                .counts
                .get_mut(len as usize)
                .ok_or(Error::InvalidHuffmanTable)?;
            *slot = slot.wrapping_add(1);
        }

        // `counts[0]` is the number of symbols with no code at all. If that
        // is every symbol the alphabet is empty — degenerate, but legal: a
        // block containing no back-references has an empty distance alphabet.
        let uncoded = table.counts.first().copied().unwrap_or(0);
        if uncoded as usize == lengths.len() {
            return Ok(table);
        }

        // Check that the Huffman tree is complete or under-subscribed.
        // The Kraft inequality: sum of 2^(-len) for each code must be ≤ 1.
        let mut left: i32 = 1;
        for bits in 1..=MAX_BITS {
            let count = table.counts.get(bits).copied().unwrap_or(0);
            left = left.wrapping_mul(2).wrapping_sub(i32::from(count));
            if left < 0 {
                return Err(Error::InvalidHuffmanTable); // over-subscribed
            }
        }

        // Compute offsets: where codes of each length start in the
        // symbols array.
        let mut offsets = [0u16; MAX_BITS + 1];
        for bits in 1..MAX_BITS {
            let start = offsets.get(bits).copied().unwrap_or(0);
            let count = table.counts.get(bits).copied().unwrap_or(0);
            if let Some(next) = offsets.get_mut(bits.wrapping_add(1)) {
                *next = start.wrapping_add(count);
            }
        }

        // Fill in the symbols array, sorted by code length then value.
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let Some(offset) = offsets.get_mut(len as usize) else {
                continue;
            };
            let idx = *offset as usize;
            *offset = offset.wrapping_add(1);
            if let Some(slot) = table.symbols.get_mut(idx) {
                *slot = sym as u16;
            }
        }

        Ok(table)
    }

    /// Decode one symbol from the bit stream.
    ///
    /// Reads bits one at a time, accumulating a code and checking
    /// against each code length.  This is simple (no lookup tables)
    /// and works well for the small alphabets in DEFLATE.
    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16> {
        let mut code: u32 = 0;
        let mut first: u32 = 0; // first code of this length
        let mut index: u32 = 0; // index into symbols for this length

        for len in 1..=MAX_BITS {
            let bit = reader.read_bits(1)?;
            code = code.wrapping_mul(2).wrapping_add(bit);
            let count = u32::from(self.counts.get(len).copied().unwrap_or(0));
            if code.wrapping_sub(first) < count {
                let sym_idx = index.wrapping_add(code.wrapping_sub(first)) as usize;
                return self
                    .symbols
                    .get(sym_idx)
                    .copied()
                    .ok_or(Error::InvalidSymbol);
            }
            first = first.wrapping_add(count).wrapping_mul(2);
            index = index.wrapping_add(count);
        }

        Err(Error::InvalidSymbol)
    }
}

/// Decode a stored (uncompressed) block.
fn inflate_stored(reader: &mut BitReader<'_>, output: &mut Vec<u8>, limit: usize) -> Result<()> {
    reader.align();
    let len = reader.read_u16_le()?;
    let nlen = reader.read_u16_le()?;

    // LEN and NLEN should be one's complements of each other.
    if len != !nlen {
        return Err(Error::StoredLengthMismatch);
    }

    for _ in 0..len {
        if output.len() >= limit {
            return Err(Error::OutputTooLarge);
        }
        let b = reader.read_byte()?;
        output.push(b);
    }

    Ok(())
}

/// Decode a block with fixed Huffman codes.
fn inflate_fixed(reader: &mut BitReader<'_>, output: &mut Vec<u8>, limit: usize) -> Result<()> {
    let (lit_table, dist_table) = fixed_tables()?;
    inflate_codes(reader, &lit_table, &dist_table, output, limit)
}

/// Decode a block with dynamic Huffman codes.
fn inflate_dynamic(reader: &mut BitReader<'_>, output: &mut Vec<u8>, limit: usize) -> Result<()> {
    let (lit_table, dist_table) = read_dynamic_tables(reader)?;
    inflate_codes(reader, &lit_table, &dist_table, output, limit)
}

/// The fixed block's two tables (RFC 1951 §3.2.6).
fn fixed_tables() -> Result<(HuffmanTable, HuffmanTable)> {
    Ok((
        HuffmanTable::build(&fixed_lit_lengths())?,
        HuffmanTable::build(&fixed_dist_lengths())?,
    ))
}

/// Read a dynamic block's header -- the code-length code, then the
/// literal/length and distance code lengths it encodes -- and build the two
/// tables the block's symbols are decoded with.
///
fn read_dynamic_tables(reader: &mut BitReader<'_>) -> Result<(HuffmanTable, HuffmanTable)> {
    // Read the number of literal/length codes, distance codes, and
    // code-length codes.
    let hlit = reader.read_bits(5)?.wrapping_add(257) as usize; // 257..286
    let hdist = reader.read_bits(5)?.wrapping_add(1) as usize; // 1..32
    let hclen = reader.read_bits(4)?.wrapping_add(4) as usize; // 4..19

    if hlit > 286 || hdist > 30 || hclen > 19 {
        return Err(Error::InvalidHuffmanTable);
    }

    // Read code-length code lengths (3 bits each, in permuted order).
    let mut cl_lens = [0u8; MAX_CL_CODES];
    for &order in CL_ORDER.iter().take(hclen) {
        // Read first, place second: the three bits must leave the stream
        // whether or not the permuted slot exists, or every following symbol
        // is decoded from a shifted position. Every `CL_ORDER` entry is < 19
        // so the slot always does exist; `get_mut` states that rather than
        // trusting the table.
        let bits = reader.read_bits(3)? as u8;
        if let Some(slot) = cl_lens.get_mut(order as usize) {
            *slot = bits;
        }
    }

    let cl_table = HuffmanTable::build(&cl_lens)?;

    // Decode literal/length and distance code lengths.
    let total = hlit.wrapping_add(hdist);
    let mut all_lens = [0u8; MAX_LIT_CODES + MAX_DIST_CODES];
    let mut i = 0;

    while i < total {
        let sym = cl_table.decode(reader)?;

        match sym {
            0..=15 => {
                // Literal code length.
                if let Some(slot) = all_lens.get_mut(i) {
                    *slot = sym as u8;
                }
                i = i.wrapping_add(1);
            }
            16 => {
                // Repeat previous length 3..6 times.
                if i == 0 {
                    return Err(Error::InvalidHuffmanTable);
                }
                let repeat = reader.read_bits(2)?.wrapping_add(3) as usize;
                let prev = all_lens.get(i.wrapping_sub(1)).copied().unwrap_or(0);
                for _ in 0..repeat {
                    if i >= total {
                        return Err(Error::InvalidHuffmanTable);
                    }
                    if let Some(slot) = all_lens.get_mut(i) {
                        *slot = prev;
                    }
                    i = i.wrapping_add(1);
                }
            }
            17 => {
                // Repeat zero 3..10 times.
                let repeat = reader.read_bits(3)?.wrapping_add(3) as usize;
                for _ in 0..repeat {
                    if i >= total {
                        return Err(Error::InvalidHuffmanTable);
                    }
                    if let Some(slot) = all_lens.get_mut(i) {
                        *slot = 0;
                    }
                    i = i.wrapping_add(1);
                }
            }
            18 => {
                // Repeat zero 11..138 times.
                let repeat = reader.read_bits(7)?.wrapping_add(11) as usize;
                for _ in 0..repeat {
                    if i >= total {
                        return Err(Error::InvalidHuffmanTable);
                    }
                    if let Some(slot) = all_lens.get_mut(i) {
                        *slot = 0;
                    }
                    i = i.wrapping_add(1);
                }
            }
            _ => return Err(Error::InvalidHuffmanTable),
        }
    }

    let lit_table = HuffmanTable::build(all_lens.get(..hlit).ok_or(Error::InvalidHuffmanTable)?)?;
    let dist_table = HuffmanTable::build(
        all_lens
            .get(hlit..total)
            .ok_or(Error::InvalidHuffmanTable)?,
    )?;
    Ok((lit_table, dist_table))
}

/// Decode literal/length + distance symbols until end-of-block (256).
fn inflate_codes(
    reader: &mut BitReader<'_>,
    lit_table: &HuffmanTable,
    dist_table: &HuffmanTable,
    output: &mut Vec<u8>,
    limit: usize,
) -> Result<()> {
    loop {
        let sym = lit_table.decode(reader)?;

        // 256 is the end-of-block symbol, and it is the *only* thing that ends
        // a block: below it is a literal, above it a length code. Written as a
        // three-way comparison because that is what the alphabet is.
        match sym.cmp(&256) {
            Ordering::Less => {
                // Literal byte.
                if output.len() >= limit {
                    return Err(Error::OutputTooLarge);
                }
                output.push(sym as u8);
            }
            Ordering::Equal => return Ok(()),
            Ordering::Greater => {
                // Length/distance pair — back-reference.
                let len_idx = (sym as usize).wrapping_sub(257);
                let base_len = *LENGTH_BASE.get(len_idx).ok_or(Error::InvalidSymbol)?;
                let extra = *LENGTH_EXTRA.get(len_idx).ok_or(Error::InvalidSymbol)?;
                let length = usize::from(base_len).wrapping_add(reader.read_bits(extra)? as usize);

                let dist_sym = dist_table.decode(reader)? as usize;
                let base_dist = *DIST_BASE.get(dist_sym).ok_or(Error::InvalidSymbol)?;
                let dist_extra = *DIST_EXTRA.get(dist_sym).ok_or(Error::InvalidSymbol)?;
                let distance =
                    usize::from(base_dist).wrapping_add(reader.read_bits(dist_extra)? as usize);

                if distance == 0 || distance > output.len() {
                    return Err(Error::DistanceTooFar);
                }

                // Copy from the sliding window. Source and destination overlap
                // whenever `length > distance` — distance=1, length=100 is a
                // hundred copies of one byte — so the read walks forward through
                // bytes this very loop is appending. That is the definition, not
                // an accident: a cursor `distance` behind the tail reproduces the
                // modulo-cycling formulation exactly, because by the time it
                // reaches a repeated position the byte there has been written.
                // It also removes the `% distance`, and with it any question
                // about a zero divisor.
                let mut src = output.len().wrapping_sub(distance);
                for _ in 0..length {
                    if output.len() >= limit {
                        return Err(Error::OutputTooLarge);
                    }
                    // `src` trails the tail by exactly `distance`, which was
                    // checked against the length above, so it is always in range.
                    let Some(&b) = output.get(src) else {
                        return Err(Error::DistanceTooFar);
                    };
                    output.push(b);
                    src = src.wrapping_add(1);
                }
            }
        }
    }
}

/// Decode one symbol of the code `lengths` from the start of `data`, as the
/// walk did: the symbol and the bits it took, or the walk's error.
pub(crate) fn decode_symbol(lengths: &[u8], data: &[u8]) -> Result<(u16, usize)> {
    let table = HuffmanTable::build(lengths)?;
    let mut reader = BitReader::new(data);
    let sym = table.decode(&mut reader)?;
    Ok((
        sym,
        reader
            .pos
            .wrapping_mul(8)
            .wrapping_add(usize::from(reader.bit)),
    ))
}
