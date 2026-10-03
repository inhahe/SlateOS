//! Decompression: libbzip2's `BZ2_decompress` (`decompress.c`), the output
//! stage of `BZ2_bzDecompress` (`unRLE_obuf_to_output_FAST` and the CRC
//! checks, `bzlib.c`), and `bzip2 -d`'s reading of what follows a stream
//! (`uncompressStream`, `bzip2.c`).
//!
//! libbzip2 is a state machine fed a buffer at a time, which returns to ask
//! for more input wherever it runs out; its caller then reports
//! `BZ_UNEXPECTED_EOF` if there is none. With the whole input in hand there
//! is nothing to resume, so every `GET_BITS` here is a call that answers
//! [`Error::UnexpectedEnd`] instead. Every check libbzip2 makes is made here,
//! at the same point, so a stream fails here exactly when it fails there.
//!
//! # 7-Zip's reading
//!
//! 7-Zip has a decoder of its own, and the 7z reader must accept what it
//! accepts. Its rules are kept here as a second [`Reader`] and are 7-Zip
//! 26.00's (`CPP/7zip/Compress/BZip2Decoder.cpp` and `HuffmanDecoder.h`,
//! read for these rules; that code is LGPL and none of it is used). Where it
//! parts from libbzip2:
//!
//! - **A coding table must be a prefix code that fits.** Its lengths are
//!   built into a table only if their Kraft sum (the sum of 2^-length) is at
//!   most 1. libbzip2 builds whatever it is given, so an over-full table it
//!   never uses -- or one whose surplus codes no symbol hits -- decodes
//!   there. A table short of 1 is allowed, as lbzip2 writes one.
//! - **A block may end in four equal bytes with no count after them.** The
//!   four are written out, where libbzip2 calls the block corrupt.
//! - **Only the first stream is read.** What follows it is the caller's
//!   business: the 7z handler calls it data after the end.
//!
//! Two of 7-Zip's checks come earlier than libbzip2's -- a start pointer at
//! or past the block size fails as it is read, and a zero run as soon as it
//! outgrows the block -- and are not mirrored: the same streams fail either
//! way, with nothing of the block written, and 7-Zip reports every failure
//! alike, so only the error named here could differ.

use alloc::vec::Vec;

use crate::huffman::{DecodeTable, MAX_ALPHA_SIZE};
use crate::tables::{CRC_TABLE, R_NUMS};
use crate::{Error, Result};

/// `BZ_N_GROUPS`: the most coding tables a block may have.
const N_GROUPS: usize = 6;
/// `BZ_G_SIZE`: symbols coded with one table before the next selector.
const G_SIZE: u32 = 50;
/// `BZ_MAX_SELECTORS`: enough for a 900 000-byte block. A stream may declare
/// more (up to 32 767); libbzip2 1.0.8 reads the rest and ignores them, since
/// some encoders round the count up.
const MAX_SELECTORS: usize = 2 + 900_000 / 50;
/// The two zero-run symbols.
const RUNA: u16 = 0;
const RUNB: u16 = 1;
/// `N` in the zero-run decoding may not reach this: a run that long could
/// not fit any block, and libbzip2 stops it before its `es` could overflow.
const MAX_RUN_WEIGHT: u32 = 2 * 1024 * 1024;
/// The longest code a table may have, in bits.
const MAX_CODE_BITS: u32 = 20;

/// Whose reading of a stream to follow, where the two part (see the
/// module's "7-Zip's reading").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reader {
    /// libbzip2 1.0.8, as `bzip2 -d` drives it.
    Bzip2,
    /// 7-Zip 26.00's decoder, as its 7z handler drives it.
    SevenZip,
}

/// `BZ_UPDATE_CRC`. The index is the top byte of `crc` XOR a byte, so it is
/// below 256 and the table has 256 entries.
fn crc_update(crc: u32, byte: u8) -> u32 {
    let i = usize::from((crc >> 24) as u8 ^ byte);
    #[allow(clippy::indexing_slicing)]
    let entry = CRC_TABLE[i];
    (crc << 8) ^ entry
}

/// libbzip2's `bsBuff`/`bsLive`: bits taken most significant first, and a
/// byte loaded only when the bits in hand run short -- so that when a stream
/// ends, `pos` is exactly where whatever follows it begins.
struct BitReader<'a> {
    data: &'a [u8],
    /// Bytes loaded so far.
    pos: usize,
    buff: u32,
    /// Bits of `buff` not yet taken: at most 31.
    live: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buff: 0,
            live: 0,
        }
    }

    /// `GET_BITS`: the next `n` bits, for `n` from 1 to 24.
    fn bits(&mut self, n: u32) -> Result<u32> {
        while self.live < n {
            let &byte = self.data.get(self.pos).ok_or(Error::UnexpectedEnd)?;
            self.buff = (self.buff << 8) | u32::from(byte);
            // `pos` indexes `data`, so adding one cannot overflow; `live` was
            // below `n` (at most 24) and grows by 8.
            self.pos = self.pos.wrapping_add(1);
            self.live = self.live.wrapping_add(8);
        }
        self.live = self.live.wrapping_sub(n);
        let mask = 1u32.wrapping_shl(n).wrapping_sub(1);
        Ok(self.buff.wrapping_shr(self.live) & mask)
    }

    /// `GET_BIT`.
    fn bit(&mut self) -> Result<bool> {
        Ok(self.bits(1)? != 0)
    }

    /// `GET_UCHAR`: eight bits, at whatever bit position the stream is at.
    fn byte(&mut self) -> Result<u8> {
        Ok(self.bits(8)?.to_le_bytes()[0])
    }

    /// Four `GET_UCHAR`s, most significant first: a stored CRC.
    fn u32(&mut self) -> Result<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            v = (v << 8) | u32::from(self.byte()?);
        }
        Ok(v)
    }
}

/// The selector-driven choice of coding table: `GET_MTF_VAL`'s
/// `groupNo`/`groupPos` bookkeeping.
struct Coder<'t> {
    selectors: &'t [u8],
    tables: &'t [DecodeTable],
    /// The next selector to use.
    next_group: usize,
    /// Symbols left in the current group of 50.
    group_pos: u32,
    current: Option<&'t DecodeTable>,
}

impl Coder<'_> {
    /// `GET_MTF_VAL`: the next symbol.
    fn symbol(&mut self, r: &mut BitReader<'_>) -> Result<u16> {
        if self.group_pos == 0 {
            // Coded data running past the last selector.
            let &sel = self
                .selectors
                .get(self.next_group)
                .ok_or(Error::InvalidHuffmanCode)?;
            self.next_group = self.next_group.wrapping_add(1);
            self.group_pos = G_SIZE;
            self.current = self.tables.get(usize::from(sel));
        }
        self.group_pos = self.group_pos.wrapping_sub(1);
        // Every selector was checked against the table count as it was read.
        let table = self.current.ok_or(Error::InvalidTables)?;

        let mut zn = table.min_len;
        let mut zvec = i32::try_from(r.bits(zn)?).unwrap_or(i32::MAX);
        loop {
            // The longest code bzip2 allows is 20 bits.
            if zn > 20 {
                return Err(Error::InvalidHuffmanCode);
            }
            let limit = table.limit.get(usize::try_from(zn).unwrap_or(usize::MAX));
            if limit.is_some_and(|&l| zvec <= l) {
                break;
            }
            zn = zn.wrapping_add(1);
            let zj = i32::from(r.bit()?);
            // `zvec` has at most 21 bits.
            zvec = zvec.wrapping_shl(1) | zj;
        }
        let base = table
            .base
            .get(usize::try_from(zn).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0);
        usize::try_from(zvec.wrapping_sub(base))
            .ok()
            .filter(|&i| i < MAX_ALPHA_SIZE)
            .and_then(|i| table.perm.get(i))
            .copied()
            .ok_or(Error::InvalidHuffmanCode)
    }
}

/// `BZ_RAND_UPD_MASK` and `BZ_RAND_MASK`: the mask a randomised block's bytes
/// were XORed with, one byte at a time.
struct RandMask {
    n_to_go: u16,
    t_pos: usize,
}

impl RandMask {
    const fn new() -> Self {
        Self {
            n_to_go: 0,
            t_pos: 0,
        }
    }

    /// The mask for the next byte: 1 at the end of each of `R_NUMS`'s runs,
    /// 0 elsewhere.
    fn next(&mut self) -> u8 {
        if self.n_to_go == 0 {
            // `t_pos` wraps at the table's length, so `get` cannot miss.
            self.n_to_go = R_NUMS.get(self.t_pos).copied().unwrap_or(1);
            self.t_pos = self.t_pos.wrapping_add(1);
            if self.t_pos == R_NUMS.len() {
                self.t_pos = 0;
            }
        }
        // Every entry is at least 50, so the count is positive here.
        self.n_to_go = self.n_to_go.wrapping_sub(1);
        u8::from(self.n_to_go == 1)
    }
}

/// Decodes the bzip2 file `data` -- one stream or several -- as `bzip2 -d`
/// does.
pub(crate) fn decompress(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut tt = Vec::new();
    let mut rest = data;
    let mut first = true;
    loop {
        if !first {
            if rest.is_empty() {
                break;
            }
            // `BZ_DATA_ERROR_MAGIC` on a later stream: `bzip2 -d` reports
            // "trailing garbage after EOF ignored" and succeeds.
            if !could_begin_stream(rest) {
                break;
            }
        }
        let used = decode_stream(rest, &mut out, &mut tt, limit, Reader::Bzip2)?;
        rest = rest.get(used..).unwrap_or_default();
        first = false;
    }
    Ok(out)
}

/// Decodes the stream at the start of `data` onto `out` as 7-Zip's decoder
/// does, keeping what was decoded before an error; returns the bytes of
/// `data` the stream took up.
pub(crate) fn decompress_as_7zip(data: &[u8], out: &mut Vec<u8>, limit: usize) -> Result<usize> {
    let mut tt = Vec::new();
    decode_stream(data, out, &mut tt, limit, Reader::SevenZip)
}

/// Whether code lengths, each from 1 to [`MAX_CODE_BITS`], can be a prefix
/// code: whether their Kraft sum -- the sum of 2^-length -- is at most 1.
/// (A sum below 1 leaves codes no symbol has; one above 1 has symbols
/// sharing codes.)
fn fits_prefix_code(lengths: &[u8]) -> bool {
    // In units of 2^-20: at most 258 lengths of 2^19 each, far from
    // overflowing.
    let sum = lengths.iter().fold(0u32, |sum, &len| {
        let share = MAX_CODE_BITS
            .checked_sub(u32::from(len))
            .and_then(|shift| 1u32.checked_shl(shift))
            .unwrap_or(0);
        sum.saturating_add(share)
    });
    sum <= 1 << MAX_CODE_BITS
}

/// Whether `data` could be the start of a stream: whether libbzip2 would get
/// through as much of the four-byte header as there is without answering
/// `BZ_DATA_ERROR_MAGIC`. A prefix of a header counts -- libbzip2 waits for
/// the rest, and then finds the input over.
fn could_begin_stream(data: &[u8]) -> bool {
    let header: [fn(u8) -> bool; 4] = [
        |c| c == b'B',
        |c| c == b'Z',
        |c| c == b'h',
        |c| (b'1'..=b'9').contains(&c),
    ];
    data.iter().zip(header).all(|(&c, ok)| ok(c))
}

/// Decodes one stream from the start of `data` onto `out`, returning the
/// bytes of `data` it took up (the last one possibly padding).
fn decode_stream(
    data: &[u8],
    out: &mut Vec<u8>,
    tt: &mut Vec<u32>,
    limit: usize,
    reader: Reader,
) -> Result<usize> {
    let mut r = BitReader::new(data);

    for magic in [b'B', b'Z', b'h'] {
        if r.byte()? != magic {
            return Err(Error::BadHeader);
        }
    }
    let digit = r.byte()?;
    if !(b'1'..=b'9').contains(&digit) {
        return Err(Error::BadHeader);
    }
    let block_size_100k = u32::from(digit.wrapping_sub(b'0'));

    let mut combined_crc = 0u32;
    loop {
        match r.byte()? {
            0x17 => {
                for magic in [0x72, 0x45, 0x38, 0x50, 0x90] {
                    if r.byte()? != magic {
                        return Err(Error::BadBlockMagic);
                    }
                }
                let stored = r.u32()?;
                if stored != combined_crc {
                    return Err(Error::StreamCrcMismatch {
                        expected: stored,
                        actual: combined_crc,
                    });
                }
                return Ok(r.pos);
            }
            0x31 => {
                for magic in [0x41, 0x59, 0x26, 0x53, 0x59] {
                    if r.byte()? != magic {
                        return Err(Error::BadBlockMagic);
                    }
                }
                let block_crc = decode_block(&mut r, block_size_100k, tt, out, limit, reader)?;
                combined_crc = combined_crc.rotate_left(1) ^ block_crc;
            }
            _ => return Err(Error::BadBlockMagic),
        }
    }
}

/// Decodes one block, after its six magic bytes, onto `out`; returns its CRC.
// One function in libbzip2 (`BZ2_decompress`'s block half and the output
// stage), kept as one so the two read side by side.
#[allow(clippy::too_many_lines)]
fn decode_block(
    r: &mut BitReader<'_>,
    block_size_100k: u32,
    tt: &mut Vec<u32>,
    out: &mut Vec<u8>,
    limit: usize,
    reader: Reader,
) -> Result<u32> {
    let stored_crc = r.u32()?;
    let randomised = r.bit()?;
    let orig_ptr = r.bits(24)?;
    // The first check libbzip2 makes, before it knows the block's length.
    let block_max = 100_000u32.wrapping_mul(block_size_100k);
    if orig_ptr > 10u32.wrapping_add(block_max) {
        return Err(Error::BadOrigPtr);
    }

    // The mapping table: which byte values occur, sixteen at a time.
    let mut in_use16 = [false; 16];
    for slot in &mut in_use16 {
        *slot = r.bit()?;
    }
    let mut seq_to_unseq = [0u8; 256];
    let mut n_in_use = 0usize;
    for (hi, &used) in (0u8..).zip(&in_use16) {
        if used {
            for lo in 0..16u8 {
                if r.bit()? {
                    if let Some(slot) = seq_to_unseq.get_mut(n_in_use) {
                        *slot = (hi << 4) | lo;
                    }
                    n_in_use = n_in_use.wrapping_add(1);
                }
            }
        }
    }
    if n_in_use == 0 {
        return Err(Error::InvalidTables);
    }
    // At most 256 values in use, so at most 258 symbols.
    let alpha_size = n_in_use.wrapping_add(2);

    // The selectors, move-to-front coded in unary.
    let n_groups = usize::try_from(r.bits(3)?).unwrap_or(0);
    if !(2..=N_GROUPS).contains(&n_groups) {
        return Err(Error::InvalidTables);
    }
    let n_selectors = usize::try_from(r.bits(15)?).unwrap_or(0);
    if n_selectors < 1 {
        return Err(Error::InvalidTables);
    }
    let mut pos: [u8; N_GROUPS] = [0, 1, 2, 3, 4, 5];
    let mut selectors = Vec::with_capacity(n_selectors.min(MAX_SELECTORS));
    for i in 0..n_selectors {
        let mut j = 0usize;
        while r.bit()? {
            j = j.wrapping_add(1);
            if j >= n_groups {
                return Err(Error::InvalidTables);
            }
        }
        if i < MAX_SELECTORS {
            // Undo the move-to-front: the table is at `pos[j]`, and moves to
            // the front. `j` is below `n_groups`, at most 6.
            if let Some(front) = pos.get_mut(..=j) {
                front.rotate_right(1);
            }
            selectors.push(pos[0]);
        }
    }

    // The coding tables: a starting length, then a delta per symbol.
    let mut tables = Vec::with_capacity(n_groups);
    let mut lengths = [0u8; MAX_ALPHA_SIZE];
    for _ in 0..n_groups {
        let mut curr = r.bits(5)?;
        for slot in lengths.iter_mut().take(alpha_size) {
            loop {
                if !(1..=20).contains(&curr) {
                    return Err(Error::InvalidTables);
                }
                if !r.bit()? {
                    break;
                }
                // `curr` is from 1 to 20 here, so neither step can wrap.
                curr = if r.bit()? {
                    curr.wrapping_sub(1)
                } else {
                    curr.wrapping_add(1)
                };
            }
            *slot = u8::try_from(curr).unwrap_or(0);
        }
        let table = lengths.get(..alpha_size).unwrap_or_default();
        // 7-Zip builds each table as its lengths are read, and refuses one
        // that is not a prefix code.
        if reader == Reader::SevenZip && !fits_prefix_code(table) {
            return Err(Error::InvalidTables);
        }
        tables.push(DecodeTable::new(table));
    }

    // The symbols: move-to-front positions, with runs of position 0 coded in
    // bijective base 2 by RUNA and RUNB.
    let eob = u16::try_from(n_in_use.wrapping_add(1)).unwrap_or(u16::MAX);
    let nblock_max = usize::try_from(block_max).unwrap_or(0);
    let mut coder = Coder {
        selectors: &selectors,
        tables: &tables,
        next_group: 0,
        group_pos: 0,
        current: None,
    };
    // The move-to-front list of sequence numbers (indices into
    // `seq_to_unseq`). libbzip2 keeps it as sixteen lists of sixteen to make
    // a deep move cheap; a single list moved with `rotate_right` gives the
    // same answers.
    let mut mtf: [u8; 256] = [0; 256];
    for (slot, v) in mtf.iter_mut().zip(0..=255u8) {
        *slot = v;
    }
    let mut unzftab = [0u32; 256];
    tt.clear();

    let mut next_sym = coder.symbol(r)?;
    loop {
        if next_sym == eob {
            break;
        }
        if next_sym == RUNA || next_sym == RUNB {
            // `es` counts the zeros: RUNA adds N, RUNB 2N, N doubling.
            let mut es = 0u32;
            let mut n = 1u32;
            loop {
                if n >= MAX_RUN_WEIGHT {
                    return Err(Error::BlockTooLarge);
                }
                // `n` is below 2^21 and `es` below 2^22: no wrapping.
                es = es.wrapping_add(if next_sym == RUNA {
                    n
                } else {
                    n.wrapping_mul(2)
                });
                n = n.wrapping_mul(2);
                next_sym = coder.symbol(r)?;
                if next_sym != RUNA && next_sym != RUNB {
                    break;
                }
            }
            let byte = seq_to_unseq.get(usize::from(mtf[0])).copied().unwrap_or(0);
            let run = usize::try_from(es).unwrap_or(usize::MAX);
            if run > nblock_max.saturating_sub(tt.len()) {
                return Err(Error::BlockTooLarge);
            }
            tt.resize(tt.len().wrapping_add(run), u32::from(byte));
            if let Some(count) = unzftab.get_mut(usize::from(byte)) {
                *count = count.wrapping_add(es);
            }
        } else {
            if tt.len() >= nblock_max {
                return Err(Error::BlockTooLarge);
            }
            // `next_sym` is from 2 to `n_in_use` here, so the move-to-front
            // position `nn` is from 1 to `n_in_use - 1`, below 256.
            let nn = usize::from(next_sym.wrapping_sub(1));
            if let Some(front) = mtf.get_mut(..=nn) {
                front.rotate_right(1);
            }
            let byte = seq_to_unseq.get(usize::from(mtf[0])).copied().unwrap_or(0);
            if let Some(count) = unzftab.get_mut(usize::from(byte)) {
                *count = count.wrapping_add(1);
            }
            tt.push(u32::from(byte));
            next_sym = coder.symbol(r)?;
        }
    }

    // Now the block's length is known, the start pointer can be checked
    // properly. An empty block fails here, as it does in libbzip2.
    let nblock = tt.len();
    let orig_ptr = usize::try_from(orig_ptr).unwrap_or(usize::MAX);
    if orig_ptr >= nblock {
        return Err(Error::BadOrigPtr);
    }

    // The inverse transform: `cftab[c]` is where the rotations starting with
    // byte `c` begin in sorted order, and `tt[i] >> 8` becomes the position
    // of the rotation after the `i`th. The counts sum to `nblock`, so every
    // running `cftab` value is a position in `tt`; and `nblock` is below
    // 2^24, so a position shifted left 8 fits.
    let mut cftab = [0u32; 256];
    let mut sum = 0u32;
    for (slot, &count) in cftab.iter_mut().zip(&unzftab) {
        *slot = sum;
        sum = sum.wrapping_add(count);
    }
    for i in 0..nblock {
        let byte = tt.get(i).copied().unwrap_or(0) & 0xff;
        let Some(next) = cftab.get_mut(usize::try_from(byte).unwrap_or(usize::MAX)) else {
            continue;
        };
        let to = usize::try_from(*next).unwrap_or(usize::MAX);
        *next = next.wrapping_add(1);
        if let Some(slot) = tt.get_mut(to) {
            *slot |= u32::try_from(i).unwrap_or(0) << 8;
        }
    }

    // Walk the cycle from the start pointer, un-randomising if need be, and
    // undo the initial run-length coding: four equal bytes are followed by a
    // count of further copies.
    let tt = &*tt;
    let mut t_pos = usize::try_from(tt.get(orig_ptr).copied().unwrap_or(0) >> 8).unwrap_or(0);
    let mut mask = randomised.then(RandMask::new);
    let mut fetch = || -> Result<u8> {
        // `BZ_GET_FAST`'s range check. `t_pos` comes from the transform just
        // built, which only holds positions below `nblock`.
        let v = tt.get(t_pos).copied().ok_or(Error::InvalidRun)?;
        t_pos = usize::try_from(v >> 8).unwrap_or(usize::MAX);
        let mut byte = v.to_le_bytes()[0];
        if let Some(mask) = mask.as_mut() {
            byte ^= mask.next();
        }
        Ok(byte)
    };

    let mut crc = 0xffff_ffffu32;
    let mut emit = |byte: u8, count: usize| -> Result<()> {
        let room = limit.saturating_sub(out.len());
        if count > room {
            // As far as the limit, for a caller that keeps what came out.
            out.resize(out.len().wrapping_add(room), byte);
            return Err(Error::OutputTooLarge);
        }
        for _ in 0..count {
            crc = crc_update(crc, byte);
        }
        out.resize(out.len().wrapping_add(count), byte);
        Ok(())
    };

    let mut k0 = fetch()?;
    let mut used = 1usize;
    loop {
        // A run of `k0` begins: up to three more equal bytes may follow.
        let mut run = 1usize;
        let mut next = None;
        while run < 4 && used < nblock {
            let k1 = fetch()?;
            used = used.wrapping_add(1);
            if k1 != k0 {
                next = Some(k1);
                break;
            }
            run = run.wrapping_add(1);
        }
        if run == 4 {
            // Four equal bytes and the block over: the count byte is missing.
            // libbzip2 calls that corrupt; 7-Zip writes the four and is done.
            if used >= nblock {
                if reader == Reader::SevenZip {
                    emit(k0, 4)?;
                    break;
                }
                return Err(Error::InvalidRun);
            }
            let extra = fetch()?;
            used = used.wrapping_add(1);
            emit(k0, usize::from(extra).wrapping_add(4))?;
            if used >= nblock {
                break;
            }
            k0 = fetch()?;
            used = used.wrapping_add(1);
        } else {
            emit(k0, run)?;
            match next {
                Some(k1) => k0 = k1,
                None => break,
            }
        }
    }

    let computed = !crc;
    if computed != stored_crc {
        return Err(Error::BlockCrcMismatch {
            expected: stored_crc,
            actual: computed,
        });
    }
    Ok(computed)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use alloc::vec;

    /// The catalogue check value of CRC-32/BZIP2.
    #[test]
    fn the_crc_is_crc32_bzip2() {
        let crc = !b"123456789".iter().fold(!0u32, |c, &b| crc_update(c, b));
        assert_eq!(crc, 0xfc89_1918);
    }

    #[test]
    fn bits_are_read_most_significant_first() {
        let mut r = BitReader::new(&[0b1010_0000, 0xff]);
        assert!(r.bit().unwrap());
        assert!(!r.bit().unwrap());
        assert_eq!(r.bits(6).unwrap(), 0b10_0000);
        assert_eq!(r.pos, 1);
        assert_eq!(r.bits(8).unwrap(), 0xff);
        assert_eq!(r.bits(1), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn a_header_prefix_could_begin_a_stream() {
        assert!(could_begin_stream(b"B"));
        assert!(could_begin_stream(b"BZh"));
        assert!(could_begin_stream(b"BZh9rest"));
        assert!(!could_begin_stream(b"BZh0"));
        assert!(!could_begin_stream(b"\0\0\0\0"));
        assert!(!could_begin_stream(b"BZx"));
    }

    /// The mask flips the low bit of the byte that ends each run of
    /// `R_NUMS`, the first run being 619 bytes long.
    #[test]
    fn the_rand_mask_marks_the_end_of_each_run() {
        let mut m = RandMask::new();
        let masks: Vec<u8> = (0..2000).map(|_| m.next()).collect();
        let ones: Vec<usize> = (0..2000).filter(|&i| masks[i] == 1).collect();
        // n_to_go is loaded with 619 and counts down; the mask is 1 when it
        // reaches 1, at the 618th byte (index 617).
        assert_eq!(ones[0], 617);
        assert_eq!(ones[1], 619 + 720 - 2);
        assert_eq!(ones[2], 619 + 720 + 127 - 2);
    }

    #[test]
    fn an_empty_input_is_cut_short() {
        assert_eq!(decompress(&[], 100), Err(Error::UnexpectedEnd));
        assert_eq!(decompress(b"BZ", 100), Err(Error::UnexpectedEnd));
        assert_eq!(decompress(b"BZh0", 100), Err(Error::BadHeader));
        assert_eq!(decompress(b"PK\x03\x04", 100), Err(Error::BadHeader));
    }

    #[test]
    fn what_follows_a_stream_is_read_as_bzip2_does() {
        let a = crate::compress(b"first ", crate::Level::FASTEST);
        let b = crate::compress(b"second", crate::Level::BEST);

        let mut two = a.clone();
        two.extend_from_slice(&b);
        assert_eq!(decompress(&two, 100).unwrap(), b"first second");

        // Bytes that cannot begin a stream are ignored...
        let mut padded = a.clone();
        padded.extend_from_slice(&[0; 7]);
        assert_eq!(decompress(&padded, 100).unwrap(), b"first ");

        // ...but the start of one must be all of one.
        let mut cut = a.clone();
        cut.extend_from_slice(b"BZ");
        assert_eq!(decompress(&cut, 100), Err(Error::UnexpectedEnd));
        let mut bad = a.clone();
        bad.extend_from_slice(&b[..b.len() - 1]);
        assert_eq!(decompress(&bad, 100), Err(Error::UnexpectedEnd));
        let mut wrong = a;
        let mut damaged = b;
        let last = damaged.len() - 3;
        damaged[last] ^= 0x40;
        wrong.extend_from_slice(&damaged);
        assert!(matches!(
            decompress(&wrong, 100),
            Err(Error::StreamCrcMismatch { .. } | Error::BlockCrcMismatch { .. })
        ));
    }

    /// Exactly 1 fits, and under it; the least amount over does not.
    #[test]
    fn a_prefix_code_fits_when_its_kraft_sum_is_at_most_one() {
        assert!(fits_prefix_code(&[1, 2, 3, 3]));
        assert!(fits_prefix_code(&[2, 2, 2, 3]));
        assert!(!fits_prefix_code(&[1, 2, 2, 3]));
        assert!(!fits_prefix_code(&[1, 1, 1]));
        // 1/2 + 1/4 + ... + 2^-19 + 2 x 2^-20 is exactly 1; with one more
        // code of 20 bits it is 1 + 2^-20.
        let mut exact: Vec<u8> = (1..=20).collect();
        exact.push(20);
        assert!(fits_prefix_code(&exact));
        exact.push(20);
        assert!(!fits_prefix_code(&exact));
        assert!(!fits_prefix_code(&[1, 1, 20]));
    }

    /// 7-Zip reads the first stream only, and says how far it went; what
    /// follows is left alone, a stream or not.
    #[test]
    fn seven_zip_reads_one_stream() {
        let a = crate::compress(b"first ", crate::Level::FASTEST);
        let b = crate::compress(b"second", crate::Level::BEST);
        for tail in [&b[..], b"BZ", b"\0\0\0", b""] {
            let mut data = a.clone();
            data.extend_from_slice(tail);
            let mut out = b"kept ".to_vec();
            assert_eq!(decompress_as_7zip(&data, &mut out, 100), Ok(a.len()));
            assert_eq!(out, b"kept first ");
        }
        // The limit counts what `out` held already.
        let mut out = b"kept ".to_vec();
        assert_eq!(decompress_as_7zip(&a, &mut out, 8), Err(Error::OutputTooLarge));
        assert_eq!(out, b"kept fir");
        // A stream cut short is cut short.
        let mut out = Vec::new();
        assert_eq!(
            decompress_as_7zip(&a[..a.len() - 1], &mut out, 100),
            Err(Error::UnexpectedEnd)
        );
    }

    #[test]
    fn the_limit_spans_streams() {
        let a = crate::compress(&[1; 600], crate::Level::FASTEST);
        let mut two = a.clone();
        two.extend_from_slice(&a);
        assert_eq!(decompress(&two, 1200).unwrap(), vec![1; 1200]);
        assert_eq!(decompress(&two, 1199), Err(Error::OutputTooLarge));
    }
}
