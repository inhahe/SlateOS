//! The DEFLATE decoder (RFC 1951) behind [`inflate_limited`](crate::inflate_limited)
//! and [`InflateStream`](crate::InflateStream).
//!
//! # Tables, not a walk
//!
//! A symbol is decoded by looking the next bits up in a table, as zlib,
//! zlib-ng and libdeflate all do. Until 2026-09-26 this crate walked the code
//! lengths a bit at a time instead -- the method of zlib's `puff.c`, which
//! zlib ships as a readable reference and not as a decoder -- and a PNG
//! photograph's pixels cost five times zlib-ng's cycles to inflate
//! (`requests/f-ab-inflate-decodes-a-bit-at-a-time-five-times-slower-than-zlib-ng.md`).
//! The walk survives as the tests' oracle (`puff.rs`), which this decoder is
//! held to byte for byte and error for error.
//!
//! * **Tables.** The literal/length code is looked up by its next 11 bits,
//!   the distance code by 8 and the code-length code by 7 (its longest code);
//!   a longer code continues in a second-level table under its first bits.
//!   Each entry says what the symbol means -- a literal, a length and its
//!   extra bits, a distance and its extra bits, the end of the block, or "no
//!   such code" -- and how many bits its code takes.
//! * **A 64-bit bit buffer**, topped up eight bytes at a time while eight
//!   remain, which holds a whole length/distance pair and its extra bits.
//! * **A fast loop and a careful one**, zlib's `inflate_fast`/`inflate`
//!   split. While eight input bytes remain to be loaded and the output has
//!   room, symbols are decoded with no check that the input could fail; a
//!   back-reference that does not fit the room goes to the stream's
//!   resumable copy, as the careful path sends it. Nearer the end of the
//!   input, every check the walk made is made, in its order.
//! * **Copies a run at a time**: a back-reference is copied with slice copies
//!   (by doubling runs when it overlaps itself), a stored block in one piece.
//!
//! # Where it stops is where the walk stopped
//!
//! The walk read one bit, then another, and gave up only when it could not
//! go on; a table sees many bits at once and could tell sooner. Three rules
//! make it tell no sooner than the walk, so that the error, and the bytes
//! delivered before it, are the walk's:
//!
//! * A code the block's lengths leave unassigned is [`Error::InvalidSymbol`]
//!   only if 15 bits remain -- the walk read to the longest length before
//!   concluding there was no such code -- and [`Error::UnexpectedEnd`] if
//!   fewer do.
//! * A code longer than the bits that remain is [`Error::UnexpectedEnd`];
//!   past the end of the input the buffer holds zeros, which never make a
//!   short code look complete.
//! * Checks the walk made byte by byte -- the output limit, the end of a
//!   stored block's input, the caller's buffer -- are made once per run, and
//!   the run stops at the byte where the first of them would have failed.

use alloc::vec;
use alloc::vec::Vec;

use crate::{DIST_BASE, DIST_EXTRA, Error, LENGTH_BASE, LENGTH_EXTRA, MAX_BITS, Result};

// ---------------------------------------------------------------------------
// Bits
// ---------------------------------------------------------------------------

/// A DEFLATE bit stream, read least-significant bit first through a 64-bit
/// buffer.
///
/// Only the low `count` bits of `buf` are accounted for. Above them there may
/// be copies of the input bits that follow -- a word load reads eight bytes
/// and counts at most seven -- and never anything else, so a later load ORs
/// the same values into the same places. Past the end of the input the buffer
/// holds zeros.
pub(crate) struct Bits<'a> {
    data: &'a [u8],
    /// The first input byte not yet loaded into `buf`.
    pos: usize,
    buf: u64,
    count: u32,
}

/// What a refill guarantees while eight input bytes remain: enough for a
/// literal/length code, its extra bits, a distance code and its extra bits
/// (15 + 5 + 15 + 13 = 48), with room to spare.
const REFILL_BITS: u32 = 56;

impl<'a> Bits<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            count: 0,
        }
    }

    /// Top the buffer up to at least [`REFILL_BITS`] bits, or to every bit
    /// the input has left.
    #[inline]
    fn refill(&mut self) {
        if self.count >= REFILL_BITS {
            return;
        }
        let word = self
            .data
            .get(self.pos..self.pos.wrapping_add(8))
            .and_then(|w| <[u8; 8]>::try_from(w).ok());
        if let Some(word) = word {
            // Eight bytes loaded, as many whole ones counted as fit: the count
            // ends between 56 and 63, and the byte that did not fit is left in
            // place above it, the same bits the next load will bring.
            self.buf |= u64::from_le_bytes(word).wrapping_shl(self.count);
            let bytes = 63u32.wrapping_sub(self.count).wrapping_shr(3);
            self.pos = self.pos.wrapping_add(bytes as usize);
            self.count = self.count.wrapping_add(bytes.wrapping_mul(8));
        } else {
            while self.count <= REFILL_BITS {
                let Some(&b) = self.data.get(self.pos) else {
                    break;
                };
                self.buf |= u64::from(b).wrapping_shl(self.count);
                self.pos = self.pos.wrapping_add(1);
                self.count = self.count.wrapping_add(8);
            }
        }
    }

    /// The low `n` bits (0..=16) of the buffer, not consumed.
    #[inline]
    fn peek(&self, n: u32) -> u32 {
        (self.buf & 1u64.wrapping_shl(n).wrapping_sub(1)) as u32
    }

    /// Consume `n` bits, which the caller has checked are there.
    #[inline]
    fn consume(&mut self, n: u32) {
        self.buf = self.buf.wrapping_shr(n);
        self.count = self.count.wrapping_sub(n);
    }

    /// Read `n` bits (0..=16).
    ///
    /// # Errors
    ///
    /// [`Error::UnexpectedEnd`] if the input has fewer.
    #[inline]
    fn take(&mut self, n: u32) -> Result<u32> {
        if self.count < n {
            self.refill();
            if self.count < n {
                return Err(Error::UnexpectedEnd);
            }
        }
        let v = self.peek(n);
        self.consume(n);
        Ok(v)
    }

    /// Skip to the next byte boundary and give back the whole bytes still in
    /// the buffer, so that a stored block is read from where the bit stream
    /// stands: the byte after the one holding the last bit consumed.
    fn align_to_byte(&mut self) -> usize {
        let whole = self.count.wrapping_shr(3) as usize;
        self.pos = self.pos.wrapping_sub(whole);
        self.buf = 0;
        self.count = 0;
        self.pos
    }

    /// Continue the bit stream at byte `pos`, after a stored block's bytes.
    /// The buffer is empty: [`align_to_byte`](Self::align_to_byte) emptied it.
    fn resume_at(&mut self, pos: usize) {
        self.pos = pos;
    }
}

// ---------------------------------------------------------------------------
// Decode tables
// ---------------------------------------------------------------------------

/// A decode-table entry: `bits | kind << 5 | extra << 8 | value << 16`.
///
/// `bits` is how many bits the code takes (at this level, for a second-level
/// entry); `extra` is a length's or distance's extra-bit count, or a
/// second-level table's index width; `value` is the literal, the length or
/// distance base, the code-length symbol, or where the second-level table
/// starts.
type Entry = u32;

const KIND_SHIFT: u32 = 5;
const EXTRA_SHIFT: u32 = 8;
const VALUE_SHIFT: u32 = 16;

/// A literal byte -- or, in the code-length code's table, a code-length
/// symbol (0..=18).
const K_LITERAL: u32 = 0;
/// A match length: `value` its base, `extra` its extra bits.
const K_LENGTH: u32 = 1;
/// The end of the block (symbol 256).
const K_END: u32 = 2;
/// The code continues in the second-level table at `value`, indexed by the
/// next `extra` bits.
const K_SUB: u32 = 3;
/// No code has these bits: the block's lengths left them unassigned.
const K_NO_CODE: u32 = 4;
/// A code the block assigned to a symbol with no meaning: literal/length 286
/// or 287, distance 30 or 31 (the fixed code has them).
const K_BAD_SYMBOL: u32 = 5;
/// A match distance: `value` its base, `extra` its extra bits.
const K_DISTANCE: u32 = 6;

const fn entry(kind: u32, extra: u32, value: u32) -> Entry {
    kind.wrapping_shl(KIND_SHIFT)
        | extra.wrapping_shl(EXTRA_SHIFT)
        | value.wrapping_shl(VALUE_SHIFT)
}

const NO_CODE: Entry = entry(K_NO_CODE, 0, 0);

#[inline]
fn bits_of(e: Entry) -> u32 {
    e & 0x1f
}

#[inline]
fn kind_of(e: Entry) -> u32 {
    e.wrapping_shr(KIND_SHIFT) & 0x7
}

#[inline]
fn extra_of(e: Entry) -> u32 {
    e.wrapping_shr(EXTRA_SHIFT) & 0x1f
}

#[inline]
fn value_of(e: Entry) -> u32 {
    e.wrapping_shr(VALUE_SHIFT)
}

/// Index bits of the first-level tables: the literal/length code's, the
/// distance code's and the code-length code's (its longest code).
const LIT_ROOT: u32 = 11;

/// The three tables, by their first levels' sizes.
type LitTable = Table<{ 1 << LIT_ROOT }>;
type DistTable = Table<256>;
type PreTable = Table<128>;

/// The longest code DEFLATE allows, as a bit count.
const MAX_CODE_BITS: u32 = MAX_BITS as u32;

/// The longest match, and so the most one symbol can write.
const MAX_MATCH: usize = 258;

/// What a literal/length symbol means.
fn lit_meaning(sym: usize) -> Entry {
    match sym {
        0..=255 => entry(K_LITERAL, 0, sym as u32),
        256 => entry(K_END, 0, 0),
        _ => {
            let i = sym.wrapping_sub(257);
            match (LENGTH_BASE.get(i), LENGTH_EXTRA.get(i)) {
                (Some(&base), Some(&extra)) => entry(K_LENGTH, u32::from(extra), u32::from(base)),
                _ => entry(K_BAD_SYMBOL, 0, 0),
            }
        }
    }
}

/// What a distance symbol means.
fn dist_meaning(sym: usize) -> Entry {
    match (DIST_BASE.get(sym), DIST_EXTRA.get(sym)) {
        (Some(&base), Some(&extra)) => entry(K_DISTANCE, u32::from(extra), u32::from(base)),
        _ => entry(K_BAD_SYMBOL, 0, 0),
    }
}

/// What a code-length-code symbol means: itself.
fn pre_meaning(sym: usize) -> Entry {
    entry(K_LITERAL, 0, sym as u32)
}

/// Reverse the low `n` bits of `code` (1..=15): DEFLATE sends a Huffman code
/// most-significant bit first into a stream read least-significant first.
#[inline]
fn reverse(code: u32, n: u32) -> u32 {
    code.reverse_bits().wrapping_shr(32u32.wrapping_sub(n))
}

/// A decode table: a first level of `N` entries, indexed by the next
/// `log2 N` bits, and after it -- in `sub` -- the second-level tables of the
/// codes too long for it.
///
/// The first level is a fixed-size array so that a lookup masked to `N - 1`
/// needs no bounds check; the second level is rare and variable.
pub(crate) struct Table<const N: usize> {
    root: alloc::boxed::Box<[Entry; N]>,
    sub: Vec<Entry>,
}

impl<const N: usize> Table<N> {
    /// Index bits of the first level.
    const BITS: u32 = N.trailing_zeros();

    fn new() -> Self {
        // Built on the heap: a boxed slice of exactly `N` always converts.
        let root = vec![NO_CODE; N]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| Self::fresh_root());
        Self {
            root,
            sub: Vec::new(),
        }
    }

    /// The same first level, built the plain way -- which may put the whole
    /// array on the stack first. Never called (the conversion above cannot
    /// fail), and out of line so that its frame is not `new`'s: this crate
    /// runs on kernel stacks too.
    #[cold]
    #[inline(never)]
    #[allow(
        clippy::unnecessary_box_returns,
        reason = "the box is the point: `new` stores one"
    )]
    fn fresh_root() -> alloc::boxed::Box<[Entry; N]> {
        alloc::boxed::Box::new([NO_CODE; N])
    }

    /// Fill the table with the canonical code `lengths` describes (RFC 1951
    /// §3.2.2), each symbol's entry being `meaning(symbol)` plus its code's
    /// length; bits no code has become [`K_NO_CODE`].
    ///
    /// # Errors
    ///
    /// [`Error::InvalidHuffmanTable`] for an over-subscribed code, or a
    /// length above 15 -- the walk's `build`, exactly. An incomplete code is
    /// legal, and so is an empty one (a block with no back-references needs
    /// no distances).
    fn build(&mut self, lengths: &[u8], meaning: fn(usize) -> Entry) -> Result<()> {
        let root = Self::BITS;
        let root_mask = N.wrapping_sub(1);

        let mut count = [0u32; MAX_BITS + 1];
        for &len in lengths {
            let slot = count
                .get_mut(usize::from(len))
                .ok_or(Error::InvalidHuffmanTable)?;
            *slot = slot.wrapping_add(1);
        }
        // Kraft: an over-subscribed code has more codes of some length than
        // the shorter ones leave room for.
        let mut left: i64 = 1;
        for &c in count.iter().skip(1) {
            left = left.wrapping_mul(2).wrapping_sub(i64::from(c));
            if left < 0 {
                return Err(Error::InvalidHuffmanTable);
            }
        }
        // The first code of each length (RFC 1951 §3.2.2, step 2); lengths of
        // 0 have no codes.
        let mut first = [0u32; MAX_BITS + 1];
        let mut code = 0u32;
        for len in 1..=MAX_BITS {
            let shorter = if len == 1 {
                0
            } else {
                count.get(len.wrapping_sub(1)).copied().unwrap_or(0)
            };
            code = code.wrapping_add(shorter).wrapping_shl(1);
            if let Some(slot) = first.get_mut(len) {
                *slot = code;
            }
        }

        self.root.fill(NO_CODE);
        self.sub.clear();

        // Pass 1: the longest code under each first-level prefix whose codes
        // do not fit the first level.
        let mut deepest = [0u8; 1 << LIT_ROOT];
        let mut next = first;
        for &len in lengths {
            if u32::from(len) <= root {
                continue;
            }
            let Some(slot) = next.get_mut(usize::from(len)) else {
                continue;
            };
            let c = *slot;
            *slot = slot.wrapping_add(1);
            let prefix = (reverse(c, u32::from(len)) as usize) & root_mask;
            if let Some(d) = deepest.get_mut(prefix) {
                *d = (*d).max(len);
            }
        }
        // The second-level tables, one per such prefix, in prefix order.
        let mut end = 0usize;
        for (prefix, &d) in deepest.iter().enumerate().take(N) {
            if d == 0 {
                continue;
            }
            let width = u32::from(d).wrapping_sub(root);
            if let Some(slot) = self.root.get_mut(prefix) {
                *slot = entry(K_SUB, width, end as u32) | root;
            }
            end = end.wrapping_add(1usize.wrapping_shl(width));
        }
        self.sub.resize(end, NO_CODE);

        // Pass 2: every code's entries -- all the slots whose low bits are the
        // code, at the level the code ends in.
        let mut next = first;
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let len = u32::from(len);
            let Some(slot) = next.get_mut(len as usize) else {
                continue;
            };
            let c = *slot;
            *slot = slot.wrapping_add(1);
            let rev = reverse(c, len) as usize;
            let e = meaning(sym);
            if len <= root {
                let step = 1usize.wrapping_shl(len);
                let mut i = rev;
                while i < N {
                    if let Some(t) = self.root.get_mut(i) {
                        *t = e | len;
                    }
                    i = i.wrapping_add(step);
                }
            } else {
                let sub = self.root.get(rev & root_mask).copied().unwrap_or(NO_CODE);
                let start = value_of(sub) as usize;
                let span = 1usize.wrapping_shl(extra_of(sub));
                let rest = len.wrapping_sub(root);
                let step = 1usize.wrapping_shl(rest);
                let mut i = rev.wrapping_shr(root);
                while i < span {
                    if let Some(t) = self.sub.get_mut(start.wrapping_add(i)) {
                        *t = e | rest;
                    }
                    i = i.wrapping_add(step);
                }
            }
        }
        Ok(())
    }

    /// The entry the buffer's next bits select, and how many bits its code
    /// takes in all.
    #[inline]
    fn lookup(&self, buf: u64) -> (Entry, u32) {
        let e = self
            .root
            .get(buf as usize & N.wrapping_sub(1))
            .copied()
            .unwrap_or(NO_CODE);
        if kind_of(e) != K_SUB {
            return (e, bits_of(e));
        }
        let sub_mask = 1usize.wrapping_shl(extra_of(e)).wrapping_sub(1);
        let idx =
            (value_of(e) as usize).wrapping_add(buf.wrapping_shr(Self::BITS) as usize & sub_mask);
        let s = self.sub.get(idx).copied().unwrap_or(NO_CODE);
        (s, Self::BITS.wrapping_add(bits_of(s)))
    }
}

/// Whether `lengths` make a code this decoder accepts, building it as a
/// literal/length code: what the tests ask of the table builder alone.
#[cfg(test)]
pub(crate) fn check_code(lengths: &[u8]) -> Result<()> {
    LitTable::new().build(lengths, lit_meaning)
}

/// A looked-up code, held to the bits there are: the entry and its length to
/// act on, or the error the walk would have stopped with.
#[inline]
fn checked(e: Entry, n: u32, avail: u32) -> Result<(Entry, u32)> {
    if kind_of(e) == K_NO_CODE {
        // The walk reads to the longest length before it gives up.
        return Err(if avail >= MAX_CODE_BITS {
            Error::InvalidSymbol
        } else {
            Error::UnexpectedEnd
        });
    }
    if n > avail {
        return Err(Error::UnexpectedEnd);
    }
    Ok((e, n))
}

/// The two tables a block's symbols are decoded with, and the code-length
/// code's, kept between blocks so that each is allocated once.
pub(crate) struct Tables {
    lit: LitTable,
    dist: DistTable,
    pre: PreTable,
    /// `lit` and `dist` hold the fixed code, so a run of fixed blocks builds
    /// it once.
    fixed: bool,
}

impl Tables {
    pub(crate) fn new() -> Self {
        Self {
            lit: LitTable::new(),
            dist: DistTable::new(),
            pre: PreTable::new(),
            fixed: false,
        }
    }

    /// The fixed code (RFC 1951 §3.2.6).
    pub(crate) fn load_fixed(&mut self) -> Result<()> {
        if !self.fixed {
            self.lit.build(&crate::fixed_lit_lengths(), lit_meaning)?;
            self.dist
                .build(&crate::fixed_dist_lengths(), dist_meaning)?;
            self.fixed = true;
        }
        Ok(())
    }

    /// A dynamic block's header (RFC 1951 §3.2.7): the code-length code, the
    /// literal/length and distance code lengths it encodes, and the two
    /// tables built from them -- checked in the walk's order, so the same
    /// header fails with the same error.
    pub(crate) fn read_dynamic(&mut self, bits: &mut Bits<'_>) -> Result<()> {
        self.fixed = false;
        let hlit = bits.take(5)?.wrapping_add(257) as usize; // 257..=288
        let hdist = bits.take(5)?.wrapping_add(1) as usize; // 1..=32
        let hclen = bits.take(4)?.wrapping_add(4) as usize; // 4..=19
        if hlit > 286 || hdist > 30 || hclen > 19 {
            return Err(Error::InvalidHuffmanTable);
        }

        let mut cl_lens = [0u8; crate::MAX_CL_CODES];
        for &order in crate::CL_ORDER.iter().take(hclen) {
            // Read first, place second: the bits leave the stream either way.
            let len = bits.take(3)? as u8;
            if let Some(slot) = cl_lens.get_mut(usize::from(order)) {
                *slot = len;
            }
        }
        self.pre.build(&cl_lens, pre_meaning)?;

        let total = hlit.wrapping_add(hdist);
        let mut lens = [0u8; crate::MAX_LIT_CODES + crate::MAX_DIST_CODES];
        let mut i = 0usize;
        while i < total {
            bits.refill();
            let (e, n) = self.pre.lookup(bits.buf);
            let (e, n) = checked(e, n, bits.count)?;
            bits.consume(n);
            let (fill, repeat) = match value_of(e) {
                sym @ 0..=15 => (sym as u8, 1),
                16 => {
                    // Repeat the previous length -- of which there is none
                    // before the first, and the walk says so before reading
                    // the count.
                    let Some(&prev) = i.checked_sub(1).and_then(|p| lens.get(p)) else {
                        return Err(Error::InvalidHuffmanTable);
                    };
                    (prev, bits.take(2)?.wrapping_add(3) as usize)
                }
                17 => (0, bits.take(3)?.wrapping_add(3) as usize),
                18 => (0, bits.take(7)?.wrapping_add(11) as usize),
                _ => return Err(Error::InvalidHuffmanTable),
            };
            for _ in 0..repeat {
                if i >= total {
                    return Err(Error::InvalidHuffmanTable);
                }
                if let Some(slot) = lens.get_mut(i) {
                    *slot = fill;
                }
                i = i.wrapping_add(1);
            }
        }

        self.lit.build(
            lens.get(..hlit).ok_or(Error::InvalidHuffmanTable)?,
            lit_meaning,
        )?;
        self.dist.build(
            lens.get(hlit..total).ok_or(Error::InvalidHuffmanTable)?,
            dist_meaning,
        )
    }
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

/// One literal/length symbol, with the distance that follows a length.
pub(crate) enum Step {
    Literal(u8),
    Match { length: usize, distance: usize },
    End,
}

/// Decode one symbol with every check the walk made, in its order: the code
/// (an end of input inside it, or no such code), a meaningless symbol, the
/// length's extra bits, the distance's code, a meaningless distance, and its
/// extra bits. What the output can take is the caller's to check.
///
/// # Errors
///
/// [`Error::UnexpectedEnd`] or [`Error::InvalidSymbol`], where the walk
/// returned them.
pub(crate) fn careful_symbol(bits: &mut Bits<'_>, t: &Tables) -> Result<Step> {
    bits.refill();
    let (e, n) = t.lit.lookup(bits.buf);
    let (e, n) = checked(e, n, bits.count)?;
    match kind_of(e) {
        K_LITERAL => {
            bits.consume(n);
            Ok(Step::Literal(value_of(e) as u8))
        }
        K_END => {
            bits.consume(n);
            Ok(Step::End)
        }
        K_LENGTH => {
            bits.consume(n);
            let length = value_of(e).wrapping_add(bits.take(extra_of(e))?) as usize;
            bits.refill();
            let (d, dn) = t.dist.lookup(bits.buf);
            let (d, dn) = checked(d, dn, bits.count)?;
            if kind_of(d) != K_DISTANCE {
                return Err(Error::InvalidSymbol);
            }
            bits.consume(dn);
            let distance = value_of(d).wrapping_add(bits.take(extra_of(d))?) as usize;
            Ok(Step::Match { length, distance })
        }
        // A meaningless symbol, its code read in full.
        _ => Err(Error::InvalidSymbol),
    }
}

// ---------------------------------------------------------------------------
// One shot, into a Vec
// ---------------------------------------------------------------------------

/// Inflate all of `data`, producing at most `limit` bytes.
///
/// # Errors
///
/// As [`inflate_limited`](crate::inflate_limited).
pub(crate) fn inflate_vec(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut bits = Bits::new(data);
    let mut out = Vec::with_capacity(data.len().saturating_mul(2).min(limit));
    let mut tables = Tables::new();
    loop {
        let bfinal = bits.take(1)?;
        match bits.take(2)? {
            0 => stored_vec(&mut bits, &mut out, limit)?,
            1 => {
                tables.load_fixed()?;
                codes_vec(&mut bits, &tables, &mut out, limit)?;
            }
            2 => {
                tables.read_dynamic(&mut bits)?;
                codes_vec(&mut bits, &tables, &mut out, limit)?;
            }
            _ => return Err(Error::ReservedBlockType),
        }
        if bfinal != 0 {
            return Ok(out);
        }
    }
}

/// The failure a run of `len` bytes meets when the output has `room` left and
/// the input `avail`, where the walk -- checking the limit, then reading, a
/// byte at a time -- met it; `None` if the run fits.
fn run_failure(len: usize, room: usize, avail: usize) -> Option<Error> {
    if len <= room && len <= avail {
        None
    } else if room < len && room <= avail {
        Some(Error::OutputTooLarge)
    } else {
        Some(Error::UnexpectedEnd)
    }
}

/// A stored block's LEN and NLEN at `pos`, checked.
fn stored_header(data: &[u8], pos: usize) -> Result<usize> {
    let Some(&[a, b, c, d]) = data.get(pos..pos.wrapping_add(4)) else {
        return Err(Error::UnexpectedEnd);
    };
    let len = u16::from_le_bytes([a, b]);
    if len != !u16::from_le_bytes([c, d]) {
        return Err(Error::StoredLengthMismatch);
    }
    Ok(usize::from(len))
}

fn stored_vec(bits: &mut Bits<'_>, out: &mut Vec<u8>, limit: usize) -> Result<()> {
    let pos = bits.align_to_byte();
    let len = stored_header(bits.data, pos)?;
    let start = pos.wrapping_add(4);
    let avail = bits.data.len().saturating_sub(start);
    if let Some(e) = run_failure(len, limit.saturating_sub(out.len()), avail) {
        return Err(e);
    }
    out.extend_from_slice(
        bits.data
            .get(start..start.wrapping_add(len))
            .ok_or(Error::UnexpectedEnd)?,
    );
    bits.resume_at(start.wrapping_add(len));
    Ok(())
}

fn codes_vec(bits: &mut Bits<'_>, t: &Tables, out: &mut Vec<u8>, limit: usize) -> Result<()> {
    loop {
        if fast_run_vec(bits, t, out, limit)? {
            return Ok(());
        }
        match careful_symbol(bits, t)? {
            Step::Literal(b) => {
                if out.len() >= limit {
                    return Err(Error::OutputTooLarge);
                }
                out.push(b);
            }
            Step::Match { length, distance } => {
                if distance > out.len() {
                    return Err(Error::DistanceTooFar);
                }
                if length > limit.saturating_sub(out.len()) {
                    return Err(Error::OutputTooLarge);
                }
                copy_match_vec(out, distance, length);
            }
            Step::End => return Ok(()),
        }
    }
}

/// Eight input bytes at `pos`, little-endian; the caller has seen they exist.
#[inline]
fn word_at(data: &[u8], pos: usize) -> u64 {
    data.get(pos..pos.wrapping_add(8))
        .and_then(|w| <[u8; 8]>::try_from(w).ok())
        .map_or(0, u64::from_le_bytes)
}

/// The fast loop, into a `Vec`: decode symbols while eight input bytes remain
/// to be loaded and the output has room for the longest match, so that no
/// symbol can run out of input or reach the limit. `Ok(true)` when the block
/// ended, `Ok(false)` when an edge came near and the careful path must go on.
///
/// The bit buffer lives in locals here, and each refill -- to 56 bits or more
/// -- is spent on one length/distance pair (48 bits at most) or on up to three
/// literals (15 bits each at most), libdeflate's arrangement.
///
/// # Errors
///
/// [`Error::InvalidSymbol`] -- 15 bits or more are always there, so an
/// unassigned code is one -- and [`Error::DistanceTooFar`].
fn fast_run_vec(bits: &mut Bits<'_>, t: &Tables, out: &mut Vec<u8>, limit: usize) -> Result<bool> {
    let data = bits.data;
    let (mut pos, mut buf, mut count) = (bits.pos, bits.buf, bits.count);
    let lit = &t.lit;
    let dist = &t.dist;
    let result = loop {
        if data.len().saturating_sub(pos) < 8 || limit.saturating_sub(out.len()) < MAX_MATCH {
            break Ok(false);
        }
        if count < REFILL_BITS {
            buf |= word_at(data, pos).wrapping_shl(count);
            let bytes = 63u32.wrapping_sub(count).wrapping_shr(3);
            pos = pos.wrapping_add(bytes as usize);
            count = count.wrapping_add(bytes.wrapping_mul(8));
        }
        let (e, n) = lit.lookup(buf);
        if kind_of(e) == K_LITERAL {
            // At least 56 bits: this literal, and two more if they follow.
            buf = buf.wrapping_shr(n);
            count = count.wrapping_sub(n);
            out.push(value_of(e) as u8);
            let (e, n) = lit.lookup(buf);
            if kind_of(e) == K_LITERAL {
                buf = buf.wrapping_shr(n);
                count = count.wrapping_sub(n);
                out.push(value_of(e) as u8);
                let (e, n) = lit.lookup(buf);
                if kind_of(e) == K_LITERAL {
                    buf = buf.wrapping_shr(n);
                    count = count.wrapping_sub(n);
                    out.push(value_of(e) as u8);
                }
            }
            continue;
        }
        match kind_of(e) {
            K_LENGTH => {
                buf = buf.wrapping_shr(n);
                let extra = extra_of(e);
                let length = value_of(e).wrapping_add((buf & mask(extra)) as u32) as usize;
                buf = buf.wrapping_shr(extra);
                let (d, dn) = dist.lookup(buf);
                if kind_of(d) != K_DISTANCE {
                    break Err(Error::InvalidSymbol);
                }
                buf = buf.wrapping_shr(dn);
                let dextra = extra_of(d);
                let distance = value_of(d).wrapping_add((buf & mask(dextra)) as u32) as usize;
                buf = buf.wrapping_shr(dextra);
                count =
                    count.wrapping_sub(n.wrapping_add(extra).wrapping_add(dn).wrapping_add(dextra));
                if distance > out.len() {
                    break Err(Error::DistanceTooFar);
                }
                copy_match_vec(out, distance, length);
            }
            K_END => {
                buf = buf.wrapping_shr(n);
                count = count.wrapping_sub(n);
                break Ok(true);
            }
            _ => break Err(Error::InvalidSymbol),
        }
    };
    bits.pos = pos;
    bits.buf = buf;
    bits.count = count;
    result
}

/// The low `n` bits set, for `n` up to 16.
#[inline]
fn mask(n: u32) -> u64 {
    1u64.wrapping_shl(n).wrapping_sub(1)
}

/// Append `length` bytes copied from `distance` back (1..=`out.len()`).
///
/// A copy that overlaps itself repeats the `distance` bytes it starts on; it
/// is made by copying the run so far onto its own end, so each copy doubles
/// what the next can take and every one starts a whole number of periods in.
fn copy_match_vec(out: &mut Vec<u8>, distance: usize, length: usize) {
    let start = out.len().wrapping_sub(distance);
    if distance >= length {
        // `start + length <= out.len()`: the whole source exists.
        out.extend_from_within(start..start.wrapping_add(length));
    } else if distance == 1 {
        let b = out.last().copied().unwrap_or(0);
        out.resize(out.len().wrapping_add(length), b);
    } else {
        let mut left = length;
        while left > 0 {
            let n = left.min(out.len().wrapping_sub(start));
            out.extend_from_within(start..start.wrapping_add(n));
            left = left.wrapping_sub(n);
        }
    }
}

// ---------------------------------------------------------------------------
// The stream's half
// ---------------------------------------------------------------------------

/// The 32 KiB a back-reference can reach, as a ring, and the count of bytes
/// ever written through it.
pub(crate) struct Window {
    ring: Vec<u8>,
    total: usize,
}

/// The DEFLATE window: back-references reach up to 32 KiB behind.
pub(crate) const WINDOW_SIZE: usize = 32768;
const WINDOW_MASK: usize = WINDOW_SIZE - 1;

impl Window {
    pub(crate) fn new() -> Self {
        Self {
            ring: vec![0; WINDOW_SIZE],
            total: 0,
        }
    }

    /// Bytes written so far.
    pub(crate) fn total(&self) -> usize {
        self.total
    }

    /// The ring's allocation, which the tests hold to the window's size.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.ring.capacity()
    }

    /// Write one byte to `out[*n]` and the ring.
    #[inline]
    pub(crate) fn put(&mut self, b: u8, out: &mut [u8], n: &mut usize) {
        if let Some(slot) = out.get_mut(*n) {
            *slot = b;
        }
        if let Some(slot) = self.ring.get_mut(self.total & WINDOW_MASK) {
            *slot = b;
        }
        *n = n.wrapping_add(1);
        self.total = self.total.wrapping_add(1);
    }

    /// Write `bytes` to `out[*n..]` and the ring; `out` has room for them.
    pub(crate) fn put_slice(&mut self, bytes: &[u8], out: &mut [u8], n: &mut usize) {
        if let Some(to) = out.get_mut(*n..n.wrapping_add(bytes.len())) {
            to.copy_from_slice(bytes);
        }
        // Only the last 32 KiB can be reached back to.
        let tail = bytes
            .get(bytes.len().saturating_sub(WINDOW_SIZE)..)
            .unwrap_or_default();
        let skipped = bytes.len().wrapping_sub(tail.len());
        let mut at = self.total.wrapping_add(skipped) & WINDOW_MASK;
        let mut rest = tail;
        while !rest.is_empty() {
            let k = rest.len().min(WINDOW_SIZE.wrapping_sub(at));
            let (now, later) = rest.split_at(k);
            if let Some(to) = self.ring.get_mut(at..at.wrapping_add(k)) {
                to.copy_from_slice(now);
            }
            at = 0;
            rest = later;
        }
        *n = n.wrapping_add(bytes.len());
        self.total = self.total.wrapping_add(bytes.len());
    }

    /// Copy into `to` the bytes that start `back` bytes before the end of the
    /// window (1..=`total`, at most 32 768), `to.len()` of them, all already
    /// written.
    fn read_back(&self, back: usize, to: &mut [u8]) {
        let mut at = self.total.wrapping_sub(back) & WINDOW_MASK;
        let mut done = 0usize;
        while done < to.len() {
            let k = to
                .len()
                .wrapping_sub(done)
                .min(WINDOW_SIZE.wrapping_sub(at));
            if let (Some(from), Some(dst)) = (
                self.ring.get(at..at.wrapping_add(k)),
                to.get_mut(done..done.wrapping_add(k)),
            ) {
                dst.copy_from_slice(from);
            }
            done = done.wrapping_add(k);
            at = 0;
        }
    }

    /// Take `bytes` -- already in the caller's buffer -- into the window.
    fn absorb(&mut self, bytes: &[u8]) {
        // Only the last 32 KiB can be reached back to.
        let tail = bytes
            .get(bytes.len().saturating_sub(WINDOW_SIZE)..)
            .unwrap_or_default();
        let mut at = self
            .total
            .wrapping_add(bytes.len().wrapping_sub(tail.len()))
            & WINDOW_MASK;
        let mut rest = tail;
        while !rest.is_empty() {
            let k = rest.len().min(WINDOW_SIZE.wrapping_sub(at));
            let (now, later) = rest.split_at(k);
            if let Some(to) = self.ring.get_mut(at..at.wrapping_add(k)) {
                to.copy_from_slice(now);
            }
            at = 0;
            rest = later;
        }
        self.total = self.total.wrapping_add(bytes.len());
    }

    /// Copy `len` bytes from `distance` back (1..=`total`, at most 32 768) to
    /// `out[*n..]` and the ring; `out` has room for them.
    ///
    /// In runs that stay inside the ring on both sides and are no longer than
    /// the distance, so every byte a run reads was written before it started
    /// -- which is what makes a slice copy the same as the walk's byte loop.
    pub(crate) fn copy(&mut self, distance: usize, len: usize, out: &mut [u8], n: &mut usize) {
        let mut left = len;
        while left > 0 {
            let src = self.total.wrapping_sub(distance) & WINDOW_MASK;
            let dst = self.total & WINDOW_MASK;
            let k = left
                .min(distance)
                .min(WINDOW_SIZE.wrapping_sub(src))
                .min(WINDOW_SIZE.wrapping_sub(dst))
                .min(out.len().saturating_sub(*n));
            if k == 0 {
                // Not reachable: `distance` is at least 1, both offsets are
                // inside the ring, and the caller left room. A guard, not a
                // loop without end.
                return;
            }
            if let (Some(from), Some(to)) = (
                self.ring.get(src..src.wrapping_add(k)),
                out.get_mut(*n..n.wrapping_add(k)),
            ) {
                to.copy_from_slice(from);
            }
            // The ranges meet only when the distance is the whole window, and
            // then each byte is its own source; `copy_within` is a memmove.
            self.ring.copy_within(src..src.wrapping_add(k), dst);
            *n = n.wrapping_add(k);
            self.total = self.total.wrapping_add(k);
            left = left.wrapping_sub(k);
        }
    }
}

/// Decode symbols into `out[*n..]` until the block ends (`Ok(true)`) or
/// `out` is full or a back-reference must wait for room (`Ok(false)`, with
/// the pending copy in `pending`).
///
/// # Errors
///
/// Every error a block's symbols can produce, after the bytes before it.
pub(crate) fn codes_stream(
    bits: &mut Bits<'_>,
    t: &Tables,
    w: &mut Window,
    limit: usize,
    out: &mut [u8],
    n: &mut usize,
    pending: &mut Option<(usize, usize)>,
) -> Result<bool> {
    while *n < out.len() {
        if fast_run_stream(bits, t, w, limit, out, n, pending)? {
            return Ok(true);
        }
        if pending.is_some() || *n >= out.len() {
            break;
        }
        match careful_symbol(bits, t)? {
            Step::Literal(b) => {
                if w.total() >= limit {
                    return Err(Error::OutputTooLarge);
                }
                w.put(b, out, n);
            }
            Step::Match { length, distance } => {
                if distance > w.total() {
                    return Err(Error::DistanceTooFar);
                }
                *pending = Some((length, distance));
                return Ok(false);
            }
            Step::End => return Ok(true),
        }
    }
    Ok(false)
}

/// The fast loop of [`codes_stream`], as [`fast_run_vec`] is of the one-shot
/// decoder: symbols decoded while eight input bytes remain to be loaded and
/// both the caller's buffer and the output limit have room for three
/// literals. A back-reference that does not fit either is left in `pending`
/// for [`copy_stream`], which is where the careful path leaves one too.
///
/// zlib's arrangement: bytes are decoded straight into `out`, a
/// back-reference copies from `out` itself when its source is in this run and
/// from the window only for the part that is older, and the run joins the
/// window once, when it ends -- rather than every byte being written twice.
///
/// # Errors
///
/// As [`fast_run_vec`]; the bytes before the error are in `out` and the
/// window.
fn fast_run_stream(
    bits: &mut Bits<'_>,
    t: &Tables,
    w: &mut Window,
    limit: usize,
    out: &mut [u8],
    n: &mut usize,
    pending: &mut Option<(usize, usize)>,
) -> Result<bool> {
    let data = bits.data;
    let (mut pos, mut buf, mut count) = (bits.pos, bits.buf, bits.count);
    let lit = &t.lit;
    let dist = &t.dist;
    let start = *n;
    let mut at = *n;
    let before = w.total();
    let result = loop {
        // Everything made so far, this run's bytes included.
        let total = before.wrapping_add(at.wrapping_sub(start));
        let room = out
            .len()
            .saturating_sub(at)
            .min(limit.saturating_sub(total));
        if data.len().saturating_sub(pos) < 8 || room < 3 {
            break Ok(false);
        }
        if count < REFILL_BITS {
            buf |= word_at(data, pos).wrapping_shl(count);
            let bytes = 63u32.wrapping_sub(count).wrapping_shr(3);
            pos = pos.wrapping_add(bytes as usize);
            count = count.wrapping_add(bytes.wrapping_mul(8));
        }
        let (e, len) = lit.lookup(buf);
        if kind_of(e) == K_LITERAL {
            buf = buf.wrapping_shr(len);
            count = count.wrapping_sub(len);
            put_at(out, &mut at, value_of(e) as u8);
            let (e, len) = lit.lookup(buf);
            if kind_of(e) == K_LITERAL {
                buf = buf.wrapping_shr(len);
                count = count.wrapping_sub(len);
                put_at(out, &mut at, value_of(e) as u8);
                let (e, len) = lit.lookup(buf);
                if kind_of(e) == K_LITERAL {
                    buf = buf.wrapping_shr(len);
                    count = count.wrapping_sub(len);
                    put_at(out, &mut at, value_of(e) as u8);
                }
            }
            continue;
        }
        match kind_of(e) {
            K_LENGTH => {
                buf = buf.wrapping_shr(len);
                let extra = extra_of(e);
                let length = value_of(e).wrapping_add((buf & mask(extra)) as u32) as usize;
                buf = buf.wrapping_shr(extra);
                let (d, dn) = dist.lookup(buf);
                if kind_of(d) != K_DISTANCE {
                    break Err(Error::InvalidSymbol);
                }
                buf = buf.wrapping_shr(dn);
                let dextra = extra_of(d);
                let distance = value_of(d).wrapping_add((buf & mask(dextra)) as u32) as usize;
                buf = buf.wrapping_shr(dextra);
                count = count.wrapping_sub(
                    len.wrapping_add(extra)
                        .wrapping_add(dn)
                        .wrapping_add(dextra),
                );
                if distance > total {
                    break Err(Error::DistanceTooFar);
                }
                if length > room {
                    // The caller's buffer or the limit comes first: copied as
                    // far as it goes, and failing at the limit's byte, by
                    // `copy_stream`.
                    *pending = Some((length, distance));
                    break Ok(false);
                }
                // The part of the source older than this run is in the
                // window; the rest is in `out`, the same distance back.
                let made = at.wrapping_sub(start);
                let mut left = length;
                if distance > made {
                    let old = distance.wrapping_sub(made).min(length);
                    if let Some(to) = out.get_mut(at..at.wrapping_add(old)) {
                        w.read_back(distance.wrapping_sub(made), to);
                    }
                    at = at.wrapping_add(old);
                    left = left.wrapping_sub(old);
                }
                copy_back_in(out, at, distance, left);
                at = at.wrapping_add(left);
            }
            K_END => {
                buf = buf.wrapping_shr(len);
                count = count.wrapping_sub(len);
                break Ok(true);
            }
            _ => break Err(Error::InvalidSymbol),
        }
    };
    w.absorb(out.get(start..at).unwrap_or_default());
    *n = at;
    bits.pos = pos;
    bits.buf = buf;
    bits.count = count;
    result
}

/// Write one byte at `out[*at]` and step past it; the caller left room.
#[inline]
fn put_at(out: &mut [u8], at: &mut usize, b: u8) {
    if let Some(slot) = out.get_mut(*at) {
        *slot = b;
    }
    *at = at.wrapping_add(1);
}

/// Copy `len` bytes to `out[at..]` from `distance` back in `out` itself,
/// where the source starts: a copy that overlaps itself doubles its run each
/// time, as [`copy_match_vec`] does.
fn copy_back_in(out: &mut [u8], at: usize, distance: usize, len: usize) {
    let src = at.wrapping_sub(distance);
    if distance >= len {
        if src.wrapping_add(len) <= out.len() && at.wrapping_add(len) <= out.len() {
            out.copy_within(src..src.wrapping_add(len), at);
        }
        return;
    }
    let mut done = 0usize;
    while done < len {
        let k = len
            .wrapping_sub(done)
            .min(at.wrapping_add(done).wrapping_sub(src));
        let to = at.wrapping_add(done);
        if src.wrapping_add(k) <= out.len() && to.wrapping_add(k) <= out.len() {
            out.copy_within(src..src.wrapping_add(k), to);
        }
        done = done.wrapping_add(k);
    }
}

/// Copy as much of a back-reference as `out` and the limit take; the bytes
/// left, or [`Error::OutputTooLarge`] once the limit is reached with bytes
/// left to copy and room to put them -- the byte at which the walk failed.
///
/// # Errors
///
/// [`Error::OutputTooLarge`].
pub(crate) fn copy_stream(
    w: &mut Window,
    limit: usize,
    remaining: usize,
    distance: usize,
    out: &mut [u8],
    n: &mut usize,
) -> Result<usize> {
    let room = out.len().saturating_sub(*n);
    let lroom = limit.saturating_sub(w.total());
    let k = remaining.min(room).min(lroom);
    w.copy(distance, k, out, n);
    if lroom < remaining.min(room) {
        return Err(Error::OutputTooLarge);
    }
    Ok(remaining.wrapping_sub(k))
}

/// Copy as much of a stored block as `out`, the limit and the input take;
/// the bytes left, or the error the walk met first.
///
/// # Errors
///
/// [`Error::OutputTooLarge`] or [`Error::UnexpectedEnd`].
pub(crate) fn stored_stream(
    bits: &mut Bits<'_>,
    w: &mut Window,
    limit: usize,
    remaining: usize,
    out: &mut [u8],
    n: &mut usize,
) -> Result<usize> {
    let want = remaining.min(out.len().saturating_sub(*n));
    let room = limit.saturating_sub(w.total());
    let avail = bits.data.len().saturating_sub(bits.pos);
    let k = want.min(room).min(avail);
    let from = bits
        .data
        .get(bits.pos..bits.pos.wrapping_add(k))
        .unwrap_or_default();
    w.put_slice(from, out, n);
    bits.pos = bits.pos.wrapping_add(k);
    if let Some(e) = run_failure(want, room, avail) {
        return Err(e);
    }
    Ok(remaining.wrapping_sub(k))
}

/// Read a block header: its BFINAL bit and, for a stored block, its length;
/// for a coded one, its tables. `Ok((bfinal, Some(len)))` for a stored block.
///
/// # Errors
///
/// Any error a block header can produce.
pub(crate) fn block_header(bits: &mut Bits<'_>, t: &mut Tables) -> Result<(bool, Option<usize>)> {
    let bfinal = bits.take(1)? != 0;
    match bits.take(2)? {
        0 => {
            let pos = bits.align_to_byte();
            let len = stored_header(bits.data, pos)?;
            bits.resume_at(pos.wrapping_add(4));
            Ok((bfinal, Some(len)))
        }
        1 => {
            t.load_fixed()?;
            Ok((bfinal, None))
        }
        2 => {
            t.read_dynamic(bits)?;
            Ok((bfinal, None))
        }
        _ => Err(Error::ReservedBlockType),
    }
}

/// Decode one symbol of the code `lengths` from the start of `data` through a
/// table with a first level of `root` bits: the symbol and the bits it took,
/// or the error. The tests hold this to the walk's `decode_symbol`.
#[cfg(test)]
pub(crate) fn decode_symbol(lengths: &[u8], root: u32, data: &[u8]) -> Result<(u16, usize)> {
    fn with<const N: usize>(lengths: &[u8], data: &[u8]) -> Result<(u16, usize)> {
        let mut table = Table::<N>::new();
        table.build(lengths, pre_meaning)?;
        let mut bits = Bits::new(data);
        bits.refill();
        let (e, n) = table.lookup(bits.buf);
        let (e, n) = checked(e, n, bits.count)?;
        Ok((value_of(e) as u16, n as usize))
    }
    match root {
        7 => with::<128>(lengths, data),
        8 => with::<256>(lengths, data),
        _ => with::<2048>(lengths, data),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests index and count what they built"
)]
mod tests {
    use super::decode_symbol;
    use crate::puff;
    use crate::{Error, MAX_OUTPUT, deflate_level, inflate_limited, inflate_stream};
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    /// xorshift64*: deterministic, so a failure names a seed that reproduces it.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n).map(|_| self.next() as u8).collect()
        }
    }

    /// A random prefix code over `symbols` symbols: a complete one grown by
    /// splitting leaves (no deeper than 15), then -- sometimes -- thinned
    /// into an incomplete one, or given one code too many.
    fn random_lengths(rng: &mut Rng, symbols: usize) -> Vec<u8> {
        let mut lengths = vec![0u8; symbols];
        let target = 1 + rng.below(symbols);
        let mut leaves: Vec<u8> = vec![0];
        while leaves.len() < target {
            if leaves.iter().all(|&d| d >= 15) {
                break;
            }
            let i = rng.below(leaves.len());
            if leaves[i] >= 15 {
                continue;
            }
            let d = leaves.swap_remove(i) + 1;
            leaves.push(d);
            leaves.push(d);
        }
        if leaves == [0] {
            leaves = vec![1];
        }
        let mut order: Vec<usize> = (0..symbols).collect();
        for k in (1..order.len()).rev() {
            order.swap(k, rng.below(k + 1));
        }
        for (&sym, &d) in order.iter().zip(&leaves) {
            lengths[sym] = d;
        }
        match rng.below(4) {
            // Incomplete: drop some codes.
            0 => {
                for l in &mut lengths {
                    if *l > 0 && rng.below(3) == 0 {
                        *l = 0;
                    }
                }
            }
            // Over-subscribed: one code too many, where there is a symbol for it.
            1 => {
                if let Some(free) = lengths.iter().position(|&l| l == 0) {
                    lengths[free] = 1 + rng.below(15) as u8;
                }
            }
            _ => {}
        }
        lengths
    }

    #[test]
    fn a_table_decodes_a_symbol_where_the_walk_does_and_fails_where_it_fails() {
        let mut rng = Rng(0x5eed_0001);
        let mut compared = 0usize;
        for case in 0..3000 {
            let symbols = [2, 3, 19, 30, 32, 286, 288][case % 7];
            let lengths = random_lengths(&mut rng, symbols);
            for root in [7u32, 8, 11] {
                for len in [0usize, 1, 2, 3, 8, 8] {
                    let data = rng.bytes(len);
                    let want = puff::decode_symbol(&lengths, &data);
                    let got = decode_symbol(&lengths, root, &data);
                    assert_eq!(
                        got, want,
                        "case {case}, root {root}, lengths {lengths:?}, data {data:02x?}"
                    );
                    compared += 1;
                }
            }
        }
        assert!(compared > 50_000);
    }

    /// Drain `data` through a stream in reads of `chunk` bytes: what it
    /// delivered and how it ended.
    fn drain(data: &[u8], limit: usize, chunk: usize) -> (Vec<u8>, Option<Error>) {
        let mut stream = inflate_stream(data, limit);
        let mut got = Vec::new();
        let mut buf = vec![0u8; chunk];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return (got, None),
                Ok(n) => {
                    assert!(n <= chunk, "a read returned more than asked");
                    got.extend_from_slice(&buf[..n]);
                }
                Err(e) => {
                    assert_eq!(stream.read(&mut buf), Err(e), "an error is sticky");
                    return (got, Some(e));
                }
            }
        }
    }

    /// Hold both decoders to the walk on `data` at `limit`: the one-shot
    /// decoder's bytes or error, and the stream's bytes then error, at every
    /// read size in `chunks`.
    fn agrees(data: &[u8], limit: usize, chunks: &[usize], what: &str) -> Option<Error> {
        let (want, end) = puff::inflate_partial(data, limit);
        match inflate_limited(data, limit) {
            Ok(v) => {
                assert_eq!(
                    end, None,
                    "{what} (limit {limit}): the walk failed, the table did not"
                );
                assert!(v == want, "{what} (limit {limit}): different bytes");
            }
            Err(e) => assert_eq!(Some(e), end, "{what} (limit {limit}): a different error"),
        }
        for &chunk in chunks {
            let (got, got_end) = drain(data, limit, chunk);
            assert_eq!(
                got_end, end,
                "{what} (limit {limit}, reads of {chunk}): a different end"
            );
            assert!(
                got == want,
                "{what} (limit {limit}, reads of {chunk}): {} bytes delivered, the walk made {}",
                got.len(),
                want.len()
            );
        }
        end
    }

    /// How often each ending was seen: a comparison that never reached an
    /// error kind has not tested it.
    #[derive(Default)]
    struct Tally(Vec<(Option<Error>, usize)>);
    impl Tally {
        fn add(&mut self, end: Option<Error>) {
            match self.0.iter_mut().find(|(e, _)| *e == end) {
                Some((_, n)) => *n += 1,
                None => self.0.push((end, 1)),
            }
        }
        fn at_least(&self, end: Option<Error>, n: usize) {
            let seen = self
                .0
                .iter()
                .find(|(e, _)| *e == end)
                .map_or(0, |(_, n)| *n);
            assert!(
                seen >= n,
                "{end:?} seen {seen} times, wanted {n}: {:?}",
                self.0
            );
        }
    }

    /// Inputs that exercise every kind of block and copy.
    fn corpus(rng: &mut Rng) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        out.push((String::from("empty"), Vec::new()));
        out.push((String::from("one byte"), vec![7]));
        out.push((String::from("random 20000"), rng.bytes(20_000)));
        let words = [
            "the ", "quick ", "brown ", "fox ", "jumps ", "over ", "lazy ", "dogs\n",
        ];
        let mut text = Vec::new();
        while text.len() < 40_000 {
            text.extend_from_slice(words[rng.below(words.len())].as_bytes());
        }
        out.push((String::from("text 40000"), text));
        out.push((String::from("zeros 40000"), vec![0u8; 40_000]));
        for period in [2usize, 3, 7, 64, 255, 1000, 20_000] {
            let unit = rng.bytes(period);
            let data: Vec<u8> = unit.iter().copied().cycle().take(45_000).collect();
            out.push((format!("period {period}"), data));
        }
        // Symbols with Fibonacci frequencies, shuffled: the deepest codes a
        // block can have, 15 bits.
        let (mut a, mut b) = (1usize, 1usize);
        let mut deep = Vec::new();
        for sym in 0u8..21 {
            deep.extend(core::iter::repeat_n(sym, a));
            (a, b) = (b, a + b);
        }
        for k in (1..deep.len()).rev() {
            deep.swap(k, rng.below(k + 1));
        }
        out.push((String::from("fibonacci"), deep));
        // Back-references far back: a random block, noise, then the block
        // again, 30000 bytes later.
        let block = rng.bytes(3000);
        let mut far = block.clone();
        far.extend(rng.bytes(27_000));
        far.extend_from_slice(&block);
        out.push((String::from("far matches"), far));
        out
    }

    #[test]
    fn every_valid_stream_decodes_as_the_walk_decodes_it() {
        let mut rng = Rng(0x5eed_0002);
        let mut tally = Tally::default();
        for (name, data) in corpus(&mut rng) {
            for level in [0u8, 1, 6, 9] {
                let c = deflate_level(&data, level);
                let what = format!("{name} at level {level}");
                tally.add(agrees(&c, MAX_OUTPUT, &[1, 7, 258, 4096, 1 << 20], &what));
                // The output limit, at and around the output's own size.
                let len = data.len();
                for limit in [len, len.saturating_sub(1), len / 2, 258, 1, 0] {
                    tally.add(agrees(&c, limit, &[300], &what));
                }
            }
        }
        tally.at_least(None, 100);
        tally.at_least(Some(Error::OutputTooLarge), 200);
    }

    #[test]
    fn a_damaged_stream_fails_where_the_walk_fails_after_the_same_bytes() {
        let mut rng = Rng(0x5eed_0003);
        let mut streams = Vec::new();
        for (name, data) in corpus(&mut rng) {
            let data = &data[..data.len().min(6000)];
            streams.push((format!("{name}/6"), deflate_level(data, 6)));
            streams.push((format!("{name}/0"), deflate_level(data, 0)));
        }
        let mut tally = Tally::default();
        for (name, c) in &streams {
            // Every byte flipped three ways -- at most 400 positions a stream,
            // spread over it.
            let step = (c.len() / 150).max(1);
            for at in (0..c.len()).step_by(step) {
                for x in [0x01u8, 0x80, 0xFF] {
                    let mut bad = c.clone();
                    bad[at] ^= x;
                    let what = format!("{name} ^{x:02x}@{at}");
                    tally.add(agrees(&bad, MAX_OUTPUT, &[1, 97, 4096], &what));
                }
            }
            // Every truncation.
            let step = (c.len() / 120).max(1);
            for cut in (0..c.len()).step_by(step) {
                let what = format!("{name} cut at {cut}");
                tally.add(agrees(&c[..cut], MAX_OUTPUT, &[1, 4096], &what));
            }
        }
        for end in [
            Error::UnexpectedEnd,
            Error::InvalidSymbol,
            Error::DistanceTooFar,
            Error::InvalidHuffmanTable,
            Error::StoredLengthMismatch,
        ] {
            tally.at_least(Some(end), 20);
        }
        tally.at_least(Some(Error::ReservedBlockType), 3);
    }

    #[test]
    fn noise_fails_where_the_walk_fails() {
        let mut rng = Rng(0x5eed_0004);
        let mut tally = Tally::default();
        for case in 0..20_000 {
            let len = 1 + rng.below(64);
            let data = rng.bytes(len);
            tally.add(agrees(
                &data,
                MAX_OUTPUT,
                &[1, 64],
                &format!("noise case {case}"),
            ));
        }
        for end in [
            Error::UnexpectedEnd,
            Error::InvalidSymbol,
            Error::DistanceTooFar,
            Error::InvalidHuffmanTable,
            Error::ReservedBlockType,
        ] {
            tally.at_least(Some(end), 20);
        }
    }

    /// A fixed-code block (BFINAL 1, BTYPE 01) holding `codes`, each written
    /// most-significant bit first as RFC 1951 packs a Huffman code.
    fn fixed_block(codes: &[(u32, u32)]) -> Vec<u8> {
        let mut bits: Vec<u8> = vec![1, 1, 0];
        for &(code, len) in codes {
            for i in (0..len).rev() {
                bits.push(((code >> i) & 1) as u8);
            }
        }
        let mut out = vec![0u8; bits.len().div_ceil(8)];
        for (i, &b) in bits.iter().enumerate() {
            out[i / 8] |= b << (i % 8);
        }
        out
    }

    /// The symbols a fixed code has and DEFLATE gives no meaning:
    /// literal/length 286 and 287, distances 30 and 31.
    #[test]
    fn a_fixed_code_symbol_without_a_meaning_fails_as_the_walk_fails() {
        // A literal (8-bit code 0x30 + 65), then 286 or 287 (0xC6, 0xC7).
        for last in [0xC6u32, 0xC7] {
            let s = fixed_block(&[(0x30 + 65, 8), (last, 8)]);
            agrees(&s, MAX_OUTPUT, &[1, 16], "fixed 286/287");
            assert_eq!(inflate_limited(&s, MAX_OUTPUT), Err(Error::InvalidSymbol));
        }
        // A literal, a length of 3 (symbol 257, the 7-bit code 1), then
        // distance 30 or 31 (5-bit codes).
        for dist in [30u32, 31] {
            let s = fixed_block(&[(0x30 + 65, 8), (1, 7), (dist, 5)]);
            agrees(&s, MAX_OUTPUT, &[1, 16], "fixed distance 30/31");
            assert_eq!(inflate_limited(&s, MAX_OUTPUT), Err(Error::InvalidSymbol));
        }
    }
}
