//! Deflate and Deflate64 as 7-Zip reads them in a 7z folder (methods
//! `040108` and `040109`).
//!
//! The format is RFC 1951's; Deflate64 is its variant with a 64 KiB window,
//! 16 extra bits on length code 285 (a length of 3 to 65 538), and distance
//! codes 30 and 31. The decoder here is written from those, not ported:
//! 7-Zip's Deflate decoder (`CPP/7zip/Compress/DeflateDecoder.cpp`) is LGPL,
//! not public domain like the rest of what this crate ports. But a 7z reader
//! must accept what 7-Zip accepts and fail where it fails, so its rules for
//! a Deflate coder in a 7z folder -- read from its source, and checked
//! against 7-Zip 26.00 by the corpus in `tests/data` -- are kept, as rules:
//!
//! - **Input past the end reads as 1-bits**, bytes of `FF`, and decoding
//!   goes on. 7-Zip's bit reader draws bytes into a 32-bit lookahead, four
//!   ahead of the one being read, and asks whether a bit past the end has
//!   been *used* only at block boundaries, after each part of a block
//!   header, at the end of each megabyte of output and at the end of the
//!   stream. Before each symbol it asks only whether more than four bytes
//!   past the end have been *drawn*. So a stream cut short still puts out
//!   the few symbols those 1-bits decode to, and then fails.
//! - **A code need not be complete.** A table of lengths is refused only if
//!   it is over-full (a Kraft sum above 1); a bit pattern no symbol has
//!   fails when it is met. (zlib refuses an incomplete table outright,
//!   unless it is a lone distance code.)
//! - **Literal/length codes 286 and 287 are matches of length 3**, where
//!   RFC 1951 has no meaning for them.
//! - **The stream ends where the output does, loosely.** Once the coder's
//!   output is full, the next symbol must end its block -- a match running
//!   past the end is cut there -- and the blocks after it, up to the final
//!   one, must be empty.
//! - **The output may come up short**: a final block that ends early ends
//!   the coder without an error of its own; the 7z reader fails the files
//!   left without data.
//!
//! What the coder took of its input is reported, for 7-Zip's check of data
//! after the end: up to the byte its last bit was in.

use alloc::vec::Vec;

/// The bits of the longest code.
const MAX_BITS: u32 = 15;
/// The bits of the longest code-length code.
const MAX_LEVEL_BITS: u32 = 7;
/// Literal/length symbols a table may have: 286 and 287 included.
const LIT_SYMBOLS: usize = 288;
/// Distance symbols a table may have: 30 and 31 included.
const DIST_SYMBOLS: usize = 32;
/// The end-of-block symbol.
const END_OF_BLOCK: u16 = 256;
/// The order the code-length code's lengths are sent in.
const LEVEL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
/// Each length symbol's base length less 3, and its extra bits, from 257:
/// RFC 1951's, then 286 and 287 as 7-Zip reads them.
const LEN_BASE: [u32; 31] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128,
    160, 192, 224, 255, 0, 0,
];
const LEN_EXTRA: [u32; 31] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0, 0, 0,
];
/// Length symbol 285 (index 28) in Deflate64: base 3, 16 extra bits.
const LEN_285_DEFLATE64: (u32, u32) = (0, 16);
/// Each distance symbol's base distance less 1, and its extra bits.
const DIST_BASE: [u32; 32] = [
    0, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536,
    2048, 3072, 4096, 6144, 8192, 12288, 16384, 24576, 32768, 49152,
];
const DIST_EXTRA: [u32; 32] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13, 14, 14,
];
/// The distance codes Deflate allows; Deflate64 allows all 32.
const DIST_SYMBOLS_DEFLATE: usize = 30;
/// The output 7-Zip asks of its decoder at a time: at the end of each, it
/// checks for a bit used past the end.
const CHUNK: usize = 1 << 20;
/// The input 7-Zip lets one such request take before, at the next block
/// boundary, it ends the request early (to report progress) and starts
/// another -- whose megabyte then counts from there.
const INPUT_STEP: u64 = 1 << 21;

/// What a Deflate coder gave.
#[derive(Debug)]
pub(crate) struct Inflated {
    pub(crate) out: Vec<u8>,
    /// The coder ended without an error of its own: 7-Zip's `S_OK`.
    pub(crate) ok: bool,
    /// Bytes of input taken -- up to the one its last bit was in.
    pub(crate) used: usize,
}

/// The coder fails: 7-Zip's `S_FALSE`.
#[derive(Debug)]
struct Fail;

type Step<T> = core::result::Result<T, Fail>;

/// The input as 7-Zip's bit reader sees it: least significant bit first,
/// then endless bytes of `FF`; and how far it has drawn ahead.
struct Bits<'a> {
    data: &'a [u8],
    /// Bits used, from the start.
    taken: u64,
    /// Bytes drawn into the lookahead, the ones past the end included.
    drawn: u64,
}

impl<'a> Bits<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            taken: 0,
            drawn: 0,
        }
    }

    /// The input's length in bytes.
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    /// Byte `i` of the input, `FF` past its end.
    fn byte(&self, i: u64) -> u8 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.data.get(i))
            .copied()
            .unwrap_or(0xFF)
    }

    /// Draws bytes until more than 24 bits are in hand, as 7-Zip does before
    /// every read but an aligned byte's: four bytes ahead of the one being
    /// read.
    fn top_up(&mut self) {
        let want = (self.taken / 8).saturating_add(4);
        if self.drawn < want {
            self.drawn = want;
        }
    }

    /// The next `n` bits, `n` at most 24, the first in the lowest; not used.
    fn peek(&mut self, n: u32) -> u32 {
        self.top_up();
        let first = self.taken / 8;
        let window = u32::from_le_bytes([
            self.byte(first),
            self.byte(first.saturating_add(1)),
            self.byte(first.saturating_add(2)),
            self.byte(first.saturating_add(3)),
        ]);
        // At most 7 bits of the first byte are used: 25 or more remain, and
        // `n` is at most 24.
        (window >> (self.taken % 8)) & (1u32 << n).wrapping_sub(1)
    }

    /// Uses `n` bits.
    fn skip(&mut self, n: u32) {
        self.taken = self.taken.saturating_add(u64::from(n));
    }

    /// The next `n` bits, used.
    fn read(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.skip(n);
        v
    }

    /// On to a byte boundary.
    fn align(&mut self) {
        self.taken = self.taken.div_ceil(8).saturating_mul(8);
    }

    /// Whether drawn bytes remain unused: 7-Zip reads a stored block's
    /// first bytes from the lookahead, the rest straight from the input.
    fn buffered(&self) -> bool {
        self.drawn.saturating_mul(8) != self.taken
    }

    /// The next byte, at a byte boundary: from the lookahead if it holds
    /// one, else drawn on its own.
    fn aligned_byte(&mut self) -> u8 {
        if !self.buffered() {
            self.drawn = self.drawn.saturating_add(1);
        }
        let b = self.byte(self.taken / 8);
        self.skip(8);
        b
    }

    /// Up to `n` bytes straight from the input, the lookahead empty: only
    /// real ones -- none once the input is over.
    fn direct(&mut self, n: usize) -> &'a [u8] {
        let data: &'a [u8] = self.data;
        let at = usize::try_from(self.drawn).unwrap_or(usize::MAX);
        let k = n.min(data.len().saturating_sub(at));
        let got = data.get(at..at.saturating_add(k)).unwrap_or_default();
        self.drawn = self.drawn.saturating_add(got.len() as u64);
        self.taken = self.drawn.saturating_mul(8);
        got
    }

    /// Whether a bit past the end has been used (`ExtraBitsWereRead`).
    fn over_read(&self) -> bool {
        self.taken > self.len().saturating_mul(8)
    }

    /// Whether more than four bytes past the end have been drawn: the
    /// cheaper check 7-Zip makes before each symbol.
    fn over_drawn(&self) -> bool {
        self.drawn > self.len().saturating_add(4)
    }

    /// The bytes the used bits reach into.
    fn used(&self) -> u64 {
        self.taken.div_ceil(8)
    }
}

/// A canonical prefix code, decoded a length at a time.
#[derive(Clone, Debug)]
struct Code {
    /// Codes of each length, 0 to 15.
    counts: [u16; 16],
    /// The symbols, by length and then by value.
    symbols: Vec<u16>,
    max_bits: u32,
}

impl Code {
    /// The code `lengths` give, from 0 (no code) to `max_bits`: refused if
    /// over-full. A code with room to spare is kept.
    fn new(lengths: &[u8], max_bits: u32) -> Option<Self> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            let slot = counts.get_mut(usize::from(l))?;
            *slot = slot.saturating_add(1);
        }
        // The Kraft sum in units of 2^-max_bits: at most 320 codes of at
        // most 2^14 units each.
        let mut sum = 0u32;
        for (len, &n) in (0u32..).zip(&counts).skip(1) {
            if len > max_bits {
                if n != 0 {
                    return None;
                }
                continue;
            }
            sum = sum.saturating_add(u32::from(n) << max_bits.saturating_sub(len));
        }
        if sum > 1 << max_bits {
            return None;
        }
        let mut symbols = Vec::with_capacity(lengths.len());
        for len in 1..=max_bits {
            for (symbol, &l) in (0u16..).zip(lengths) {
                if u32::from(l) == len {
                    symbols.push(symbol);
                }
            }
        }
        Some(Self {
            counts,
            symbols,
            max_bits,
        })
    }

    /// The next symbol, its code used; `None` for a bit pattern no symbol
    /// has.
    fn decode(&self, bits: &mut Bits<'_>) -> Option<u16> {
        let v = bits.peek(self.max_bits);
        // `code` is the bits so far, first bit highest; `first` the first
        // code of the current length; `index` where its symbols begin. Every
        // value stays below 2^16, and `code` at or above `first`.
        let (mut code, mut first, mut index) = (0u32, 0u32, 0u32);
        for len in 1..=self.max_bits {
            code |= (v >> len.saturating_sub(1)) & 1;
            let count = u32::from(self.counts.get(len as usize).copied().unwrap_or(0));
            let offset = code.checked_sub(first)?;
            if offset < count {
                bits.skip(len);
                return self
                    .symbols
                    .get(index.checked_add(offset)? as usize)
                    .copied();
            }
            index = index.checked_add(count)?;
            first = first.checked_add(count)?.checked_shl(1)?;
            code = code.checked_shl(1)?;
        }
        None
    }
}

/// The decoder's state between 7-Zip's requests for output.
struct Inflater<'a> {
    bits: Bits<'a>,
    out: Vec<u8>,
    deflate64: bool,
    /// The window: 32 KiB, or 64 KiB for Deflate64.
    window: usize,
    final_block: bool,
    /// At a block boundary: the next thing read is a block header.
    need_header: bool,
    stored: bool,
    /// What is left of a stored block.
    stored_left: usize,
    /// What is left of a match the last request's output cut.
    pending: usize,
    /// That match's distance less 1.
    pending_distance: usize,
    /// The final block has ended.
    finished: bool,
    lit: Code,
    dist: Code,
}

/// Decodes the Deflate (or `deflate64`) stream `input` to at most
/// `out_size` bytes, as 7-Zip's coder in a 7z folder does.
pub(crate) fn decode(input: &[u8], out_size: usize, deflate64: bool) -> Inflated {
    let empty = Code {
        counts: [0; 16],
        symbols: Vec::new(),
        max_bits: MAX_BITS,
    };
    let mut z = Inflater {
        bits: Bits::new(input),
        out: Vec::with_capacity(out_size.min(1 << 24)),
        deflate64,
        window: if deflate64 { 1 << 16 } else { 1 << 15 },
        final_block: false,
        need_header: true,
        stored: false,
        stored_left: 0,
        pending: 0,
        pending_distance: 0,
        finished: false,
        lit: empty.clone(),
        dist: empty,
    };
    let ok = z.run(out_size).is_ok();
    let used = usize::try_from(z.bits.used()).unwrap_or(usize::MAX);
    Inflated {
        out: z.out,
        ok,
        used,
    }
}

impl Inflater<'_> {
    /// 7-Zip's `CodeReal`: output asked for a megabyte at a time, the last
    /// request -- the one that reaches `out_size` -- in finish mode.
    fn run(&mut self, out_size: usize) -> Step<()> {
        loop {
            let rem = out_size.saturating_sub(self.out.len());
            let (want, finish) = if rem <= CHUNK {
                (rem, true)
            } else {
                (CHUNK, false)
            };
            self.request(want, finish)?;
            if self.finished {
                break;
            }
        }
        // 7-Zip asks once more here whether a bit past the end was used
        // (`InputEofError`). The final block is found finished only at the
        // top of a request's loop, just after that loop asked the same with
        // no bit used since, so the answer cannot have changed.
        Ok(())
    }

    /// 7-Zip's `CodeSpec`: `want` more bytes of output; in `finish` mode,
    /// on to the end of the stream after them.
    fn request(&mut self, mut want: usize, finish: bool) -> Step<()> {
        if self.finished {
            return Ok(());
        }
        while self.pending > 0 && want > 0 {
            self.copy(self.pending_distance, 1)?;
            self.pending = self.pending.saturating_sub(1);
            want = want.saturating_sub(1);
        }
        let start = self.bits.used();
        while want > 0 || finish {
            if self.bits.over_read() {
                return Err(Fail);
            }
            if self.need_header {
                if self.final_block {
                    self.finished = true;
                    break;
                }
                if self.bits.used().saturating_sub(start) >= INPUT_STEP {
                    return Ok(());
                }
                self.header()?;
                if self.bits.over_read() {
                    return Err(Fail);
                }
                self.need_header = false;
            }
            if self.stored {
                if finish && want == 0 && self.stored_left != 0 {
                    return Err(Fail);
                }
                let mut num = self.stored_left.min(want);
                self.stored_left = self.stored_left.saturating_sub(num);
                want = want.saturating_sub(num);
                while num > 0 && self.bits.buffered() {
                    let b = self.bits.aligned_byte();
                    self.out.push(b);
                    num = num.saturating_sub(1);
                }
                while num > 0 {
                    let got = self.bits.direct(num);
                    if got.is_empty() {
                        return Err(Fail);
                    }
                    self.out.extend_from_slice(got);
                    num = num.saturating_sub(got.len());
                }
                self.need_header = self.stored_left == 0;
                continue;
            }
            while want > 0 {
                if self.bits.over_drawn() {
                    return Err(Fail);
                }
                let sym = self.lit.decode(&mut self.bits).ok_or(Fail)?;
                if sym < END_OF_BLOCK {
                    // Below 256.
                    self.out.push(sym.to_le_bytes()[0]);
                    want = want.saturating_sub(1);
                    continue;
                }
                if sym == END_OF_BLOCK {
                    self.need_header = true;
                    break;
                }
                let (len, distance) = self.match_after(sym)?;
                let now = len.min(want);
                self.copy(distance, now)?;
                want = want.saturating_sub(now);
                if len > now {
                    self.pending = len.saturating_sub(now);
                    self.pending_distance = distance;
                    break;
                }
            }
            if finish && want == 0 {
                // The output is full: the block must end here.
                if self.lit.decode(&mut self.bits) != Some(END_OF_BLOCK) {
                    return Err(Fail);
                }
                self.need_header = true;
            }
        }
        if self.bits.over_read() {
            return Err(Fail);
        }
        Ok(())
    }

    /// The length and distance (less 1) of the match length symbol `sym`
    /// begins.
    fn match_after(&mut self, sym: u16) -> Step<(usize, usize)> {
        let i = usize::from(sym.saturating_sub(END_OF_BLOCK + 1));
        let (base, extra) = if self.deflate64 && i == 28 {
            LEN_285_DEFLATE64
        } else {
            (
                LEN_BASE.get(i).copied().ok_or(Fail)?,
                LEN_EXTRA.get(i).copied().ok_or(Fail)?,
            )
        };
        let len = base.saturating_add(3).saturating_add(self.bits.read(extra));
        let d = usize::from(self.dist.decode(&mut self.bits).ok_or(Fail)?);
        let base = DIST_BASE.get(d).copied().ok_or(Fail)?;
        let extra = DIST_EXTRA.get(d).copied().ok_or(Fail)?;
        let distance = base.saturating_add(self.bits.read(extra));
        Ok((len as usize, distance as usize))
    }

    /// Copies `len` bytes from `distance + 1` back: no further back than
    /// the output so far, or than the window.
    fn copy(&mut self, distance: usize, len: usize) -> Step<()> {
        let reach = self.out.len().min(self.window);
        if distance >= reach {
            return Err(Fail);
        }
        let from = self.out.len().saturating_sub(distance.saturating_add(1));
        for k in 0..len {
            let b = self.out.get(from.saturating_add(k)).copied().ok_or(Fail)?;
            self.out.push(b);
        }
        Ok(())
    }

    /// A block header (`ReadTables`), with 7-Zip's checks for a bit used
    /// past the end between its parts.
    fn header(&mut self) -> Step<()> {
        self.final_block = self.bits.read(1) == 1;
        if self.bits.over_read() {
            return Err(Fail);
        }
        let kind = self.bits.read(2);
        if kind > 2 || self.bits.over_read() {
            return Err(Fail);
        }
        if kind == 0 {
            self.stored = true;
            self.bits.align();
            let len = u16::from_le_bytes([self.bits.aligned_byte(), self.bits.aligned_byte()]);
            let nlen = u16::from_le_bytes([self.bits.aligned_byte(), self.bits.aligned_byte()]);
            if len != !nlen {
                return Err(Fail);
            }
            self.stored_left = usize::from(len);
            return Ok(());
        }
        self.stored = false;
        let mut lit = [0u8; LIT_SYMBOLS];
        let mut dist = [0u8; DIST_SYMBOLS];
        if kind == 1 {
            for (i, l) in lit.iter_mut().enumerate() {
                *l = match i {
                    0..144 => 8,
                    144..256 => 9,
                    256..280 => 7,
                    _ => 8,
                };
            }
            dist.fill(5);
        } else {
            self.dynamic_lengths(&mut lit, &mut dist)?;
        }
        self.lit = Code::new(&lit, MAX_BITS).ok_or(Fail)?;
        self.dist = Code::new(&dist, MAX_BITS).ok_or(Fail)?;
        Ok(())
    }

    /// `base` plus the next `bits` bits: a count.
    fn count(&mut self, bits: u32, base: usize) -> usize {
        (self.bits.read(bits) as usize).saturating_add(base)
    }

    /// A dynamic block's code lengths, through its code-length code.
    fn dynamic_lengths(
        &mut self,
        lit: &mut [u8; LIT_SYMBOLS],
        dist: &mut [u8; DIST_SYMBOLS],
    ) -> Step<()> {
        // 257 to 288, 1 to 32 and 4 to 19.
        let n_lit = self.count(5, 257);
        let n_dist = self.count(5, 1);
        let n_level = self.count(4, 4);
        if !self.deflate64 && n_dist > DIST_SYMBOLS_DEFLATE {
            return Err(Fail);
        }
        let mut level_lengths = [0u8; 19];
        for &k in LEVEL_ORDER.iter().take(n_level) {
            let l = self.bits.read(3).to_le_bytes()[0];
            if let Some(slot) = level_lengths.get_mut(k) {
                *slot = l;
            }
        }
        if self.bits.over_read() {
            return Err(Fail);
        }
        let levels = Code::new(&level_lengths, MAX_LEVEL_BITS).ok_or(Fail)?;
        let mut lengths = [0u8; LIT_SYMBOLS + DIST_SYMBOLS];
        let total = n_lit.saturating_add(n_dist);
        let mut i = 0usize;
        while i < total {
            let sym = levels.decode(&mut self.bits).ok_or(Fail)?;
            let (fill, base, extra) = match sym {
                0..=15 => {
                    if let Some(slot) = lengths.get_mut(i) {
                        *slot = sym.to_le_bytes()[0];
                    }
                    i = i.saturating_add(1);
                    continue;
                }
                16 => {
                    // Repeat the last length -- of which there must be one.
                    let last = i.checked_sub(1).and_then(|j| lengths.get(j)).ok_or(Fail)?;
                    (*last, 3, 2)
                }
                17 => (0, 3, 3),
                _ => (0, 11, 7),
            };
            let end = i.saturating_add(self.count(extra, base));
            if end > total {
                return Err(Fail);
            }
            if let Some(run) = lengths.get_mut(i..end) {
                run.fill(fill);
            }
            i = end;
        }
        if self.bits.over_read() {
            return Err(Fail);
        }
        let (l, d) = lengths.split_at(n_lit);
        if let Some(slots) = lit.get_mut(..n_lit) {
            slots.copy_from_slice(l);
        }
        if let (Some(slots), Some(src)) = (dist.get_mut(..n_dist), d.get(..n_dist)) {
            slots.copy_from_slice(src);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation
    )]

    use alloc::vec;

    use super::*;

    /// Bits written least significant first, as Deflate packs them.
    #[derive(Default)]
    struct Writer {
        out: Vec<u8>,
        bits: u32,
    }

    impl Writer {
        fn put(&mut self, v: u32, n: u32) {
            for k in 0..n {
                if self.bits.is_multiple_of(8) {
                    self.out.push(0);
                }
                let last = self.out.last_mut().unwrap();
                *last |= (((v >> k) & 1) as u8) << (self.bits % 8);
                self.bits += 1;
            }
        }

        /// A Huffman code: its bits go most significant first.
        fn code(&mut self, code: u32, len: u32) {
            for k in (0..len).rev() {
                self.put((code >> k) & 1, 1);
            }
        }

        /// A literal or end of block in the fixed code.
        fn fixed(&mut self, sym: u32) {
            match sym {
                0..144 => self.code(0x30 + sym, 8),
                144..256 => self.code(0x190 + sym - 144, 9),
                256..280 => self.code(sym - 256, 7),
                _ => self.code(0xC0 + sym - 280, 8),
            }
        }

        fn finish(self) -> Vec<u8> {
            self.out
        }
    }

    /// A final fixed-code block of `literals`, then whatever `then` adds.
    fn fixed_block(literals: &[u8], then: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut w = Writer::default();
        w.put(1, 1);
        w.put(1, 2);
        for &b in literals {
            w.fixed(u32::from(b));
        }
        then(&mut w);
        w.finish()
    }

    fn eob(w: &mut Writer) {
        w.fixed(256);
    }

    #[test]
    fn a_stream_zlib_wrote_is_read() {
        // Text with repeats, by the workspace's deflate at its best level:
        // dynamic blocks and matches.
        let mut data = Vec::new();
        for i in 0..3000u32 {
            data.extend_from_slice(
                alloc::format!("line {} of the test {}\n", i % 97, i % 13).as_bytes(),
            );
        }
        let packed = deflate::deflate_level(&data, 9);
        let r = decode(&packed, data.len(), false);
        assert!(r.ok);
        assert_eq!(r.out, data);
        assert_eq!(r.used, packed.len());
    }

    #[test]
    fn a_stored_block_is_read() {
        let mut w = Writer::default();
        w.put(1, 1);
        w.put(0, 2);
        let mut s = w.finish();
        s.extend_from_slice(&[3, 0, !3, !0, b'a', b'b', b'c']);
        let r = decode(&s, 3, false);
        assert!(r.ok);
        assert_eq!(r.out, b"abc");
        // LEN must be NLEN's complement.
        s[3] ^= 1;
        assert!(!decode(&s, 3, false).ok);
    }

    #[test]
    fn the_output_may_come_up_short() {
        let s = fixed_block(b"ab", eob);
        let r = decode(&s, 5, false);
        assert!(r.ok);
        assert_eq!(r.out, b"ab");
    }

    #[test]
    fn a_full_output_must_end_its_block() {
        // The end of block straight after: fine.
        let s = fixed_block(b"abc", eob);
        assert!(decode(&s, 3, false).ok);
        // A literal more: refused, the output kept.
        let s = fixed_block(b"abcd", eob);
        let r = decode(&s, 3, false);
        assert!(!r.ok);
        assert_eq!(r.out, b"abc");
    }

    #[test]
    fn a_match_running_past_the_output_is_cut() {
        // "ab", then a match of length 5 at distance 2 (symbol 259, then
        // distance symbol 1), into an output of 4.
        let s = fixed_block(b"ab", |w| {
            w.fixed(259);
            w.code(1, 5);
            w.fixed(256);
        });
        let r = decode(&s, 4, false);
        assert!(r.ok);
        assert_eq!(r.out, b"abab");
        // Into an output of 7, the whole of it.
        let r = decode(&s, 7, false);
        assert!(r.ok);
        assert_eq!(r.out, b"abababa");
    }

    #[test]
    fn empty_blocks_may_follow_a_full_output_up_to_the_final_one() {
        let mut w = Writer::default();
        // Not final: "ab".
        w.put(0, 1);
        w.put(1, 2);
        w.fixed(u32::from(b'a'));
        w.fixed(u32::from(b'b'));
        w.fixed(256);
        // Not final, empty.
        w.put(0, 1);
        w.put(1, 2);
        w.fixed(256);
        // Final, empty.
        w.put(1, 1);
        w.put(1, 2);
        w.fixed(256);
        let s = w.finish();
        let r = decode(&s, 2, false);
        assert!(r.ok);
        assert_eq!(r.out, b"ab");
    }

    #[test]
    fn a_stored_block_after_a_full_output_must_be_empty() {
        let mut w = Writer::default();
        w.put(0, 1);
        w.put(1, 2);
        w.fixed(u32::from(b'a'));
        w.fixed(256);
        w.put(1, 1);
        w.put(0, 2);
        let mut s = w.finish();
        s.extend_from_slice(&[1, 0, !1, !0, b'b']);
        assert!(!decode(&s, 1, false).ok);
        assert!(decode(&s, 2, false).ok);
        let n = s.len();
        s[n - 5..].copy_from_slice(&[0, 0, 0xFF, 0xFF, 0]);
        let r = decode(&s[..n - 1], 1, false);
        assert!(r.ok);
        assert_eq!(r.out, b"a");
    }

    /// A final fixed block of five 9-bit literals and no end: 3 + 45 bits,
    /// six whole bytes, so everything after them is past the end.
    fn five_nine_bit_literals() -> Vec<u8> {
        let s = fixed_block(&[200; 5], |_| {});
        assert_eq!(s.len(), 6);
        s
    }

    /// The input over, 1-bits are read -- nine of them, in the fixed code,
    /// are literal 255 -- and put out until a check sees them. Within a
    /// block, that is the check before each symbol for more than four bytes
    /// drawn past the end, the lookahead being drawn to four bytes past the
    /// byte being read: the symbols past the end start at bits 48 and 57,
    /// whose reads draw to bytes 10 and 11, so the one at 66 is not read.
    #[test]
    fn input_past_the_end_reads_as_ones_until_a_check() {
        let s = five_nine_bit_literals();
        let r = decode(&s, 100, false);
        assert!(!r.ok);
        assert_eq!(r.out, [200, 200, 200, 200, 200, 255, 255]);
        // With the output full on the first 255, the next symbol -- another
        // 255, no end of block -- fails it.
        let r = decode(&s, 6, false);
        assert!(!r.ok);
        assert_eq!(r.out, [200, 200, 200, 200, 200, 255]);
        let r = decode(&s, 5, false);
        assert!(!r.ok);
        assert_eq!(r.out, [200; 5]);
        // One byte more of input -- an 8-bit literal, 97 -- and the checks
        // move with it: two 255s after it.
        let mut longer = fixed_block(&[200; 5], |w| {
            for _ in 0..8 {
                w.fixed(97);
            }
        });
        assert_eq!(longer.len(), 14);
        longer.truncate(7);
        let r = decode(&longer, 100, false);
        assert!(!r.ok);
        assert_eq!(r.out, [200, 200, 200, 200, 200, 97, 255, 255]);
    }

    /// A final dynamic block whose code has 'A' as "0" and the end of block
    /// as "1" -- and one distance code, of one bit: a code with room to
    /// spare -- declaring `hdist` distance lengths; then 'A's until a byte
    /// boundary, and the end of block. Returns the stream, its length in
    /// bits and the number of 'A's.
    fn dynamic_block(hdist: u32) -> (Vec<u8>, u32, usize) {
        let mut w = Writer::default();
        w.put(1, 1);
        w.put(2, 2);
        w.put(0, 5);
        w.put(hdist - 1, 5);
        w.put(15, 4);
        // The code-length code: 18 in one bit ("0"), 0 and 1 in two ("10"
        // and "11").
        let mut lens = [0u32; 19];
        lens[18] = 1;
        lens[0] = 2;
        lens[1] = 2;
        for &k in &LEVEL_ORDER {
            w.put(lens[k], 3);
        }
        let mut todo: Vec<u8> = vec![0; 257 + hdist as usize];
        todo[65] = 1;
        todo[256] = 1;
        todo[257] = 1;
        let mut i = 0;
        while i < todo.len() {
            if todo[i] == 1 {
                w.code(3, 2);
                i += 1;
                continue;
            }
            let mut run = 0;
            while i + run < todo.len() && todo[i + run] == 0 && run < 138 {
                run += 1;
            }
            if run >= 11 {
                w.code(0, 1);
                w.put(run as u32 - 11, 7);
                i += run;
            } else {
                w.code(2, 2);
                i += 1;
            }
        }
        let mut n = 0;
        while n == 0 || !w.bits.is_multiple_of(8) {
            w.code(0, 1);
            n += 1;
        }
        w.code(1, 1);
        let bits = w.bits;
        (w.finish(), bits, n)
    }

    /// A block whose end comes from a 1-bit past the end of the input: its
    /// output is whole, and the check at the next block boundary fails it.
    #[test]
    fn a_bit_used_past_the_end_fails_at_the_next_boundary() {
        let (whole, bits, n) = dynamic_block(30);
        assert_eq!(bits % 8, 1, "the end of block opens a byte");
        let want = vec![b'A'; n];
        let r = decode(&whole, n, false);
        assert!(r.ok);
        assert_eq!(r.out, want);
        // Without that byte, the end of block is read from a 1-bit.
        let cut = &whole[..whole.len() - 1];
        let r = decode(cut, n, false);
        assert!(!r.ok);
        assert_eq!(r.out, want);
        // With an output to fill still, likewise.
        let r = decode(cut, n + 2, false);
        assert!(!r.ok);
        assert_eq!(r.out, want);
    }

    #[test]
    fn an_incomplete_code_is_kept_and_an_over_full_one_refused() {
        assert!(Code::new(&[1, 2, 3, 3], 15).is_some());
        assert!(Code::new(&[1, 2, 3], 15).is_some());
        assert!(Code::new(&[1, 1, 1], 15).is_none());
        assert!(Code::new(&[], 15).is_some());
        assert!(Code::new(&[8], 7).is_none());
        // A code with room to spare -- 0 is "0", 1 is "10", "11" is no
        // symbol's: the pattern no symbol has fails. Bits are read from
        // the lowest: "0", "10", "0".
        let c = Code::new(&[1, 2], 15).unwrap();
        let data = [0b0000_0010];
        let mut b = Bits::new(&data);
        assert_eq!(c.decode(&mut b), Some(0));
        assert_eq!(c.decode(&mut b), Some(1));
        assert_eq!(b.taken, 3);
        assert_eq!(c.decode(&mut b), Some(0));
        let data = [0b0000_0011];
        let mut b = Bits::new(&data);
        assert_eq!(c.decode(&mut b), None);
    }

    #[test]
    fn codes_286_and_287_are_matches_of_three() {
        for sym in [286, 287] {
            let s = fixed_block(b"xy", |w| {
                w.fixed(sym);
                w.code(1, 5);
                w.fixed(256);
            });
            let r = decode(&s, 5, false);
            assert!(r.ok, "{sym}");
            assert_eq!(r.out, b"xyxyx");
        }
    }

    #[test]
    fn distances_reach_back_no_further_than_the_output_or_the_window() {
        // Distance 3 with 2 bytes out: refused.
        let s = fixed_block(b"ab", |w| {
            w.fixed(257);
            w.code(2, 5);
            w.fixed(256);
        });
        assert!(!decode(&s, 5, false).ok);
        // Distance codes 30 and 31 (32769 and up) in the fixed code: past
        // Deflate's 32 KiB window, so refused even after 40 000 bytes.
        let data: Vec<u8> = (0..40_000u32).map(|i| (i % 251) as u8).collect();
        for code in [30, 31] {
            // A stored block, not the final one, of the 40 000 bytes.
            let mut w = Writer::default();
            w.put(0, 1);
            w.put(0, 2);
            let mut s = w.finish();
            let n = data.len() as u16;
            s.extend_from_slice(&n.to_le_bytes());
            s.extend_from_slice(&(!n).to_le_bytes());
            s.extend_from_slice(&data);
            let mut w = Writer::default();
            w.put(1, 1);
            w.put(1, 2);
            w.fixed(257);
            w.code(code, 5);
            w.put(0, 14);
            w.fixed(256);
            s.extend_from_slice(&w.finish());
            let r = decode(&s, 40_003, false);
            assert!(!r.ok, "code {code}");
            assert_eq!(r.out.len(), 40_000);
            // Deflate64's window reaches: 32769 back from 40 000.
            let r = decode(&s, 40_003, true);
            if code == 30 {
                assert!(r.ok);
                assert_eq!(&r.out[40_000..], &data[40_000 - 32_769..][..3]);
            } else {
                // 49153 is past the 40 000 bytes out.
                assert!(!r.ok);
            }
        }
    }

    #[test]
    fn deflate64_reads_sixteen_bits_after_length_285() {
        // Fixed code 285 with 16 extra bits: length 3 + 1000.
        let s = fixed_block(b"q", |w| {
            w.fixed(285);
            w.put(1000, 16);
            w.code(0, 5);
            w.fixed(256);
        });
        let r = decode(&s, 1004, true);
        assert!(r.ok);
        assert_eq!(r.out, vec![b'q'; 1004]);
        // In Deflate, 285 is 258 and takes no extra bits: the 16 bits are
        // read as codes instead, and the stream fails.
        assert!(!decode(&s, 1004, false).ok);
    }

    #[test]
    fn a_dynamic_block_may_declare_32_distance_codes_only_in_deflate64() {
        for (hdist, deflate_ok) in [(30u32, true), (31, false), (32, false)] {
            let (s, _, n) = dynamic_block(hdist);
            assert_eq!(decode(&s, n, false).ok, deflate_ok, "{hdist}");
            let r = decode(&s, n, true);
            assert!(r.ok, "{hdist}");
            assert_eq!(r.out, vec![b'A'; n]);
        }
    }

    #[test]
    fn the_input_used_ends_with_the_final_block() {
        let mut s = fixed_block(b"abc", eob);
        let r = decode(&s, 3, false);
        assert_eq!(r.used, s.len());
        s.extend_from_slice(b"more");
        let r = decode(&s, 3, false);
        assert!(r.ok);
        assert_eq!(r.used, s.len() - 4);
    }

    #[test]
    fn nothing_but_an_empty_final_block_is_an_empty_output() {
        let s = fixed_block(b"", eob);
        let r = decode(&s, 0, false);
        assert!(r.ok);
        assert!(r.out.is_empty());
        // And an output expected of it is short, not an error of its own.
        let r = decode(&s, 1, false);
        assert!(r.ok);
        assert!(r.out.is_empty());
    }

    /// Block type 3 is refused as it is read -- here on the body of a sound
    /// dynamic block, which would decode were it taken for type 2.
    #[test]
    fn a_block_type_of_3_is_refused() {
        let (mut s, _, n) = dynamic_block(30);
        assert!(decode(&s, n, false).ok);
        // BFINAL is bit 0, BTYPE bits 1 and 2: 2 becomes 3.
        s[0] |= 0b010;
        assert!(!decode(&s, n, false).ok);
    }

    /// A code-length repeat (16) with no length before it to repeat is
    /// refused -- not read as repeating a zero. The same lengths with the
    /// first three spelled out decode, so it is the repeat alone that fails.
    #[test]
    fn a_repeat_with_nothing_before_it_is_refused() {
        // `dynamic_block`'s block -- 'A' as "0", the end of block as "1",
        // one distance code of one bit -- with a code-length code of four
        // symbols in two bits each: 0 "00", 1 "01", 16 "10", 18 "11".
        let stream = |first_three: &dyn Fn(&mut Writer)| {
            let mut w = Writer::default();
            w.put(1, 1);
            w.put(2, 2);
            w.put(0, 5);
            w.put(0, 5);
            w.put(15, 4);
            let mut lens = [0u32; 19];
            for k in [0, 1, 16, 18] {
                lens[k] = 2;
            }
            for &k in &LEVEL_ORDER {
                w.put(lens[k], 3);
            }
            first_three(&mut w);
            let zeros = |w: &mut Writer, n: u32| {
                w.code(3, 2);
                w.put(n - 11, 7);
            };
            zeros(&mut w, 62); // 3 to 64
            w.code(1, 2); // 'A', 65
            zeros(&mut w, 138); // 66 to 203
            zeros(&mut w, 52); // 204 to 255
            w.code(1, 2); // the end of block, 256
            w.code(1, 2); // the one distance code
            w.code(0, 1); // 'A'
            w.code(1, 1); // the end of block
            w.finish()
        };
        let spelled = decode(
            &stream(&|w| {
                for _ in 0..3 {
                    w.code(0, 2);
                }
            }),
            1,
            false,
        );
        assert!(spelled.ok, "the lengths spelled out do not decode");
        assert_eq!(spelled.out, b"A");
        let repeated = stream(&|w| {
            w.code(2, 2);
            w.put(0, 2);
        });
        assert!(!decode(&repeated, 1, false).ok);
    }

    /// The end of each megabyte of output is a check for a bit used past the
    /// end. A match the input's last bits begin -- its distance's extra bit
    /// past the end -- that crosses the first megabyte fails there, with the
    /// rest of the match not written; without that check, the next request
    /// would write it before its own check failed.
    #[test]
    fn a_megabyte_boundary_checks_for_a_bit_used_past_the_end() {
        // Stored blocks, not final, of 16 x 65 535 bytes: 2^20 - 16.
        let mut s = Vec::new();
        let chunk: Vec<u8> = (0..65_535u32).map(|i| (i % 251) as u8).collect();
        for _ in 0..16 {
            s.push(0);
            s.extend_from_slice(&65_535u16.to_le_bytes());
            s.extend_from_slice(&0u16.to_le_bytes());
            s.extend_from_slice(&chunk);
        }
        // A final fixed block: 14 literals of 8 bits and one of 9 -- 2^20 - 1
        // bytes in all -- then a match of 3 at distance code 4, whose extra
        // bit is the first past the end: 3 + 112 + 9 + 7 + 5 = 136 bits.
        let mut w = Writer::default();
        w.put(1, 1);
        w.put(1, 2);
        for _ in 0..14 {
            w.fixed(u32::from(b'q'));
        }
        w.fixed(200);
        w.fixed(257);
        w.code(4, 5);
        assert_eq!(w.bits, 136);
        s.extend_from_slice(&w.finish());
        let r = decode(&s, (1 << 20) + 10, false);
        assert!(!r.ok);
        assert_eq!(r.out.len(), 1 << 20);
    }

    #[test]
    fn a_long_stream_crosses_its_megabyte_checks() {
        let data: Vec<u8> = (0..(3 << 20) + 5).map(|i: u32| (i * 7 / 3) as u8).collect();
        let packed = deflate::deflate_level(&data, 1);
        let r = decode(&packed, data.len(), false);
        assert!(r.ok);
        assert_eq!(r.out, data);
    }
}
