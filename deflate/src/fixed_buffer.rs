//! Inflating a zlib stream into a buffer of known size, and stopping exactly
//! where zlib stops -- or exactly where libdeflate stops.
//!
//! # Why two decoders that "do the same thing"
//!
//! [`zlib_inflate`](crate::zlib_inflate) answers "what does this stream
//! decompress to?". A reader of an image format asks a different question:
//! "this strip decompresses to `n` bytes; fill `n` bytes the way the library
//! the format's reference reader uses would fill them". On a well-formed strip
//! the two questions have the same answer. On a damaged one they do not, and
//! the difference is visible to a user: the picture libtiff shows, or the
//! refusal libtiff gives, depends on precisely where its decompressor stopped
//! reading and what it had checked by then.
//!
//! libtiff 4.7.1 uses two decompressors, and they stop in different places:
//!
//! * **PixarLog** strips go through zlib's streaming `inflate()`
//!   (`PixarLogDecode`: `avail_out` is the strip's size, and `inflate(...,
//!   Z_PARTIAL_FLUSH)` is called until the buffer is full, the stream ends or
//!   an error comes back). zlib writes a match or a stored block *as far as it
//!   fits*; once the buffer is full it keeps going through everything that
//!   needs no room -- the next code, a block header, a table -- and reports an
//!   error found there; it stops quietly if it runs out of input.
//!   [`zlib_inflate_into`] is that.
//! * **Deflate** (Adobe Deflate) strips go through libdeflate
//!   (`libdeflate_zlib_decompress` with no size out-parameter). libdeflate
//!   writes nothing of a match or a stored block that does not fit, and stops
//!   there ("insufficient space", which libtiff accepts); it reads the input
//!   past its end as zero bits, and calls the data bad only when it has used
//!   them or read more than eight bytes of them.
//!   [`zlib_decompress_into`] is that.
//!
//! Each is a port of its library's decoding *decisions* -- the order in which
//! things are checked, what counts as a valid code, when input or room runs
//! out -- and of the one speed trick whose effect is visible, libdeflate's
//! word-at-a-time match copy (see `libdeflate_codes`). They are held to their
//! libraries by `tests/fixed_buffer.rs`, which replays 1,519 streams against
//! the answers zlib 1.3 and libdeflate 1.24 themselves gave for them
//! (`tests/data/fixed_buffer/generate.py` builds both from pinned sources and
//! regenerates the answers). Before that corpus was cut, the two were run
//! against the libraries on 105,000 generated streams -- valid, mutated,
//! truncated, and built with long codewords -- with no disagreement, whole
//! output buffers compared.
//!
//! # Where the two libraries disagree about validity
//!
//! | input | zlib 1.3 | libdeflate 1.24 |
//! |---|---|---|
//! | litlen symbol 286, 287 | error | a length of 258 |
//! | distance symbol 30, 31 | error | base 24577, 13 extra bits |
//! | more than 286 litlen / 30 distance lengths | error | accepted |
//! | an empty code (all lengths 0) | error on use; an empty *precode* decodes every length as 0 | every pattern is symbol 0, one bit |
//! | one codeword of length 1 | litlen/distance: `0` is it, `1` is an error; precode: error | both `0` and `1` are that symbol, all three codes |
//! | litlen code without an end-of-block | error | accepted |
//! | a repeat that overruns the lengths | error at that repeat | error after the last one |
//!
//! Every one of those is a place where a single "correct" decoder would agree
//! with at most one of the two.

use crate::{Error, Result, adler32};

/// Where [`zlib_inflate_into`] stopped, when zlib would not have reported an
/// error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZlibStop {
    /// The buffer is full. The stream may go on; zlib had no complaint about
    /// the part of it that it read after the buffer filled.
    Full,
    /// The stream ended (its Adler-32 checked and good) after this many bytes.
    /// libtiff accepts this only when it equals the buffer's length.
    Ended(usize),
}

/// How [`zlib_decompress_into`] filled the buffer, when libdeflate would not
/// have reported an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filled {
    /// The stream ended exactly at the end of the buffer, and its Adler-32
    /// (read from just after the Deflate data) matched.
    Complete,
    /// libdeflate's "insufficient space": the next thing to write -- a
    /// literal, a match or a stored block -- did not fit, and none of it was
    /// written. This many bytes were; the rest of the buffer is untouched.
    Full(usize),
}

// ---------------------------------------------------------------------------
// Tables shared by both (RFC 1951 3.2.5)
// ---------------------------------------------------------------------------

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The order the code-length code's lengths are stored in.
const PRECODE_ORDER: [u8; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
/// The most symbols any code here has: 288 litlen + 32 distance, decoded into
/// one array, plus libdeflate's worst repeat overrun (138 - 1).
const LENS_CAPACITY: usize = 288 + 32 + 137;

/// Which Huffman code is being built: the rules for accepting an incomplete
/// one differ by code in zlib.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CodeRole {
    Precode,
    Litlen,
    Distance,
}

/// What decoding one symbol with a [`Code`] does.
#[derive(Clone, Copy)]
enum Shape {
    /// A complete canonical code: read bits until a codeword is complete.
    Complete,
    /// libdeflate's accepted incomplete codes: one bit, whatever its value,
    /// decodes to this symbol.
    AnyBit(u16),
    /// zlib's single codeword of length 1: bit `0` is this symbol, bit `1`
    /// is an invalid code. One bit either way.
    ZeroOnly(u16),
    /// zlib's empty code: one bit, and then an invalid code -- except in the
    /// precode, whose table zlib never checks for invalid entries, so every
    /// decode there yields code length 0.
    Empty,
}

/// A canonical Huffman code, as `puff.c` represents one: how many codewords
/// of each length, and the symbols in codeword order.
struct Code {
    counts: [u16; 16],
    symbols: [u16; 320],
    shape: Shape,
    /// The longest codeword, at least 1 -- libdeflate's reduced
    /// `max_codeword_len`, which sets its litlen main-table width.
    max_len: u8,
}

/// Which library's acceptance rules to build a code under.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Oracle {
    Zlib,
    Libdeflate,
}

/// The code `lengths` describe, or `None` if `oracle` refuses it.
fn build(lengths: &[u8], role: CodeRole, oracle: Oracle) -> Option<Code> {
    let mut counts = [0u16; 16];
    for &len in lengths {
        if let Some(c) = counts.get_mut(usize::from(len)) {
            *c = c.saturating_add(1);
        }
    }
    let max = (1..16usize)
        .rev()
        .find(|&l| counts.get(l).copied().unwrap_or(0) != 0);
    let first_symbol = || {
        lengths
            .iter()
            .position(|&l| l != 0)
            .and_then(|s| u16::try_from(s).ok())
            .unwrap_or(0)
    };
    // Over-subscribed or incomplete? `left` is the codespace remaining at each
    // length, as zlib's inflate_table and libdeflate's build_decode_table both
    // compute it (in different units, to the same verdict).
    let mut left: i32 = 1;
    for len in 1..16usize {
        left = left.saturating_mul(2);
        left = left.saturating_sub(i32::from(counts.get(len).copied().unwrap_or(0)));
        if left < 0 {
            return None; // over-subscribed: both refuse
        }
    }
    let shape = match max {
        None => match oracle {
            // zlib: `max == 0` is accepted for every code, and forces an
            // error when used (or length 0, in the precode).
            Oracle::Zlib => Shape::Empty,
            // libdeflate: symbol 0, codewords `0` and `1`.
            Oracle::Libdeflate => Shape::AnyBit(0),
        },
        Some(_) if left == 0 => Shape::Complete,
        Some(max_len) => {
            // Incomplete, with at least one codeword.
            let single_len1 = max_len == 1 && counts.get(1).copied() == Some(1);
            match oracle {
                Oracle::Zlib => {
                    // inftrees.c: `if (left > 0 && (type == CODES || max != 1))`.
                    if role == CodeRole::Precode || max_len != 1 {
                        return None;
                    }
                    Shape::ZeroOnly(first_symbol())
                }
                Oracle::Libdeflate => {
                    // `codespace_used != 1 << (max - 1) || len_counts[1] != 1`.
                    if !single_len1 {
                        return None;
                    }
                    Shape::AnyBit(first_symbol())
                }
            }
        }
    };
    let mut symbols = [0u16; 320];
    if matches!(shape, Shape::Complete) {
        // Offsets of each length's first symbol in `symbols`.
        let mut offs = [0u16; 16];
        for len in 1..15usize {
            let next = offs
                .get(len)
                .copied()
                .unwrap_or(0)
                .saturating_add(counts.get(len).copied().unwrap_or(0));
            if let Some(o) = offs.get_mut(len.saturating_add(1)) {
                *o = next;
            }
        }
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            if let Some(o) = offs.get_mut(usize::from(len)) {
                if let (Some(slot), Ok(sym16)) =
                    (symbols.get_mut(usize::from(*o)), u16::try_from(sym))
                {
                    *slot = sym16;
                }
                *o = o.saturating_add(1);
            }
        }
    }
    let max_len = max.and_then(|m| u8::try_from(m).ok()).unwrap_or(1);
    Some(Code {
        counts,
        symbols,
        shape,
        max_len,
    })
}

/// One decoded symbol, or what stopped it.
enum Sym {
    Symbol(u16),
    /// zlib only: an invalid code (an unused pattern, or an empty code).
    Invalid,
}

/// A source of bits, least-significant first.
trait Bits {
    /// The next bit, or `None` when the input has run out (zlib) -- the
    /// libdeflate reader never runs out, it reads zeros.
    fn bit(&mut self) -> Option<u32>;

    /// `n` bits (n <= 16), least significant first, or `None` if they are
    /// not all there.
    fn bits(&mut self, n: u8) -> Option<u32> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Some(v)
    }
}

/// Decode one symbol and say how long its codeword was. `None` means the
/// input ran out mid-codeword.
fn decode(code: &Code, role: CodeRole, src: &mut impl Bits) -> Option<(Sym, u8)> {
    match code.shape {
        Shape::AnyBit(sym) => {
            src.bit()?;
            Some((Sym::Symbol(sym), 1))
        }
        Shape::ZeroOnly(sym) => Some((
            if src.bit()? == 0 {
                Sym::Symbol(sym)
            } else {
                Sym::Invalid
            },
            1,
        )),
        Shape::Empty => {
            src.bit()?;
            Some((
                if role == CodeRole::Precode {
                    Sym::Symbol(0)
                } else {
                    Sym::Invalid
                },
                1,
            ))
        }
        Shape::Complete => {
            // puff.c's decode(): canonical codes read MSB-first per codeword.
            let mut code_v: i32 = 0;
            let mut first: i32 = 0;
            let mut index: i32 = 0;
            for len in 1..16usize {
                code_v |= i32::try_from(src.bit()?).unwrap_or(0);
                let count = i32::from(code.counts.get(len).copied().unwrap_or(0));
                if code_v.saturating_sub(count) < first {
                    let at = index.saturating_add(code_v.saturating_sub(first));
                    let sym = usize::try_from(at)
                        .ok()
                        .and_then(|i| code.symbols.get(i))
                        .copied();
                    let cw = u8::try_from(len).unwrap_or(15);
                    return Some((sym.map_or(Sym::Invalid, Sym::Symbol), cw));
                }
                index = index.saturating_add(count);
                first = first.saturating_add(count).saturating_mul(2);
                code_v = code_v.saturating_mul(2);
            }
            // A complete code always resolves within 15 bits.
            Some((Sym::Invalid, 15))
        }
    }
}

fn fixed_lengths() -> [u8; 320] {
    let mut lens = [0u8; 320];
    for (i, l) in lens.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            280..=287 => 8,
            _ => 5, // the 32 distance codes
        };
    }
    lens
}

// ===========================================================================
// zlib 1.3 `inflate()`, as libtiff's PixarLogDecode drives it
// ===========================================================================

/// Bits for the zlib decoder: whole input available, nothing past its end.
struct ZlibBits<'a> {
    data: &'a [u8],
    /// Next bit to read, counted from the start of `data`.
    pos: usize,
}

impl Bits for ZlibBits<'_> {
    fn bit(&mut self) -> Option<u32> {
        let byte = self.data.get(self.pos / 8)?;
        let b = u32::from(byte >> (self.pos % 8)) & 1;
        self.pos = self.pos.saturating_add(1);
        Some(b)
    }
}

impl ZlibBits<'_> {
    /// zlib's BYTEBITS(): discard the rest of the current byte.
    fn byte_align(&mut self) {
        self.pos = self.pos.div_ceil(8).saturating_mul(8);
    }

    /// Whether `n` more bits are available (zlib's NEEDBITS succeeds).
    fn have(&self, n: usize) -> bool {
        self.pos.saturating_add(n) <= self.data.len().saturating_mul(8)
    }
}

/// What stopped the zlib state machine, other than an error.
enum ZlibHalt {
    /// A state needed room (LIT, MATCH, COPY with the buffer full).
    NeedRoom,
    /// A state needed input that is not there.
    NeedInput,
    /// DONE: the Adler-32 checked.
    Done,
}

/// Inflate the zlib stream `data` into `out` as zlib 1.3's `inflate()` does
/// when libtiff hands it all of `data` and `out.len()` bytes of room and
/// calls it until the room is gone, the stream ends or it reports an error
/// (libtiff 4.7.1 `PixarLogDecode`).
///
/// * Matches and stored blocks are written as far as they fit.
/// * Once `out` is full, decoding continues through everything that needs no
///   room -- the next literal/length code, a length's distance (whose
///   "too far back" check is *not* reached: zlib makes it only when copying),
///   an end of block, the next block's header and tables, the Adler-32 --
///   and an error found there is returned.
/// * Running out of input is [`Error::UnexpectedEnd`] unless `out` is full,
///   in which case it is [`ZlibStop::Full`].
///
/// # Errors
///
/// Each of zlib's `Z_DATA_ERROR` messages maps to the nearest [`Error`]:
/// header problems to [`Error::BadWrapperHeader`] (a preset dictionary to
/// [`Error::PresetDictionary`]), block type 3 to
/// [`Error::ReservedBlockType`], stored lengths to
/// [`Error::StoredLengthMismatch`], every table problem to
/// [`Error::InvalidHuffmanTable`], invalid codes to [`Error::InvalidSymbol`],
/// "too far back" to [`Error::DistanceTooFar`], and "incorrect data check"
/// to [`Error::ChecksumMismatch`]. `Z_BUF_ERROR` -- no progress possible -- is
/// [`Error::UnexpectedEnd`].
pub fn zlib_inflate_into(data: &[u8], out: &mut [u8]) -> Result<ZlibStop> {
    let mut z = ZlibBits { data, pos: 0 };
    let mut written = 0usize;
    let halt = zlib_run(&mut z, out, &mut written)?;
    let full = written == out.len();
    match halt {
        ZlibHalt::Done => Ok(ZlibStop::Ended(written)),
        ZlibHalt::NeedRoom => Ok(ZlibStop::Full),
        // libtiff calls inflate again while there is room; with no input left
        // that call makes no progress and returns Z_BUF_ERROR. With no room it
        // stops calling -- unless the one call it made (room 0 from the
        // start) made no progress either, which is Z_BUF_ERROR too.
        ZlibHalt::NeedInput if full && (z.pos > 0 || !out.is_empty()) => Ok(ZlibStop::Full),
        ZlibHalt::NeedInput => Err(Error::UnexpectedEnd),
    }
}

/// The state machine proper. `Err` is `Z_DATA_ERROR` (or `Z_NEED_DICT`).
fn zlib_run(z: &mut ZlibBits<'_>, out: &mut [u8], written: &mut usize) -> Result<ZlibHalt> {
    macro_rules! need {
        ($e:expr) => {
            match $e {
                Some(v) => v,
                None => return Ok(ZlibHalt::NeedInput),
            }
        };
    }

    // HEAD: NEEDBITS(16), then the checks in zlib's order.
    if !z.have(16) {
        // zlib pulls what bytes there are into its bit buffer before leaving,
        // which counts as progress; `pos` stands in for that below.
        z.pos = z.data.len().saturating_mul(8).min(16);
        return Ok(ZlibHalt::NeedInput);
    }
    let cmf = u32::from(z.data.first().copied().unwrap_or(0));
    let flg = u32::from(z.data.get(1).copied().unwrap_or(0));
    if (cmf.saturating_mul(256).saturating_add(flg)) % 31 != 0 {
        return Err(Error::BadWrapperHeader); // "incorrect header check"
    }
    if cmf & 0x0f != 8 {
        return Err(Error::BadWrapperHeader); // "unknown compression method"
    }
    if (cmf >> 4).saturating_add(8) > 15 {
        return Err(Error::BadWrapperHeader); // "invalid window size"
    }
    z.pos = 16;
    if flg & 0x20 != 0 {
        // DICTID: NEEDBITS(32), then DICT returns Z_NEED_DICT.
        if !z.have(32) {
            z.pos = z.data.len().saturating_mul(8);
            return Ok(ZlibHalt::NeedInput);
        }
        return Err(Error::PresetDictionary);
    }

    let fixed = fixed_lengths();
    let mut lens = [0u8; LENS_CAPACITY];
    loop {
        // TYPEDO
        let last = need!(z.bit());
        let btype = need!(z.bits(2));
        match btype {
            0 => {
                // STORED: BYTEBITS; NEEDBITS(32); LEN/NLEN.
                z.byte_align();
                let len = need!(z.bits(16));
                let nlen = need!(z.bits(16));
                if len != (nlen ^ 0xffff) {
                    return Err(Error::StoredLengthMismatch); // "invalid stored block lengths"
                }
                // COPY: as far as input and room allow, a piece at a time.
                let mut remaining = usize::try_from(len).unwrap_or(0);
                while remaining > 0 {
                    let room = out.len().saturating_sub(*written);
                    if room == 0 {
                        return Ok(ZlibHalt::NeedRoom);
                    }
                    let start = z.pos / 8;
                    let avail = z.data.len().saturating_sub(start);
                    if avail == 0 {
                        return Ok(ZlibHalt::NeedInput);
                    }
                    let n = remaining.min(room).min(avail);
                    let src = z.data.get(start..start.saturating_add(n)).unwrap_or(&[]);
                    let dst = out
                        .get_mut(*written..written.saturating_add(n))
                        .unwrap_or(&mut []);
                    dst.copy_from_slice(src);
                    *written = written.saturating_add(n);
                    z.pos = z.pos.saturating_add(n.saturating_mul(8));
                    remaining = remaining.saturating_sub(n);
                }
            }
            1 => {
                let (Some(lit), Some(dist)) = (
                    build(
                        fixed.get(..288).unwrap_or(&[]),
                        CodeRole::Litlen,
                        Oracle::Zlib,
                    ),
                    build(
                        fixed.get(288..).unwrap_or(&[]),
                        CodeRole::Distance,
                        Oracle::Zlib,
                    ),
                ) else {
                    return Err(Error::InvalidHuffmanTable); // unreachable: fixed codes are complete
                };
                if let Some(halt) = zlib_codes(z, out, written, &lit, &dist)? {
                    return Ok(halt);
                }
            }
            2 => {
                // TABLE: NEEDBITS(14).
                let nlen = usize::try_from(need!(z.bits(5)))
                    .unwrap_or(0)
                    .saturating_add(257);
                let ndist = usize::try_from(need!(z.bits(5)))
                    .unwrap_or(0)
                    .saturating_add(1);
                let ncode = usize::try_from(need!(z.bits(4)))
                    .unwrap_or(0)
                    .saturating_add(4);
                if nlen > 286 || ndist > 30 {
                    return Err(Error::InvalidHuffmanTable); // "too many length or distance symbols"
                }
                // LENLENS
                let mut pre = [0u8; 19];
                for &slot in PRECODE_ORDER.iter().take(ncode) {
                    let v = need!(z.bits(3));
                    if let Some(p) = pre.get_mut(usize::from(slot)) {
                        *p = u8::try_from(v).unwrap_or(0);
                    }
                }
                let Some(precode) = build(&pre, CodeRole::Precode, Oracle::Zlib) else {
                    return Err(Error::InvalidHuffmanTable); // "invalid code lengths set"
                };
                // CODELENS
                let total = nlen.saturating_add(ndist);
                let mut have = 0usize;
                while have < total {
                    let sym = match need!(decode(&precode, CodeRole::Precode, z)).0 {
                        Sym::Symbol(s) => s,
                        // An invalid precode entry only exists for an empty
                        // precode, which `decode` answers as length 0.
                        Sym::Invalid => 0,
                    };
                    if sym < 16 {
                        if let Some(l) = lens.get_mut(have) {
                            *l = u8::try_from(sym).unwrap_or(0);
                        }
                        have = have.saturating_add(1);
                        continue;
                    }
                    // NEEDBITS(here.bits + extra) comes before the checks.
                    let (value, copy) = match sym {
                        16 => {
                            let extra = need!(z.bits(2));
                            if have == 0 {
                                return Err(Error::InvalidHuffmanTable); // "invalid bit length repeat"
                            }
                            let prev = lens.get(have.saturating_sub(1)).copied().unwrap_or(0);
                            (
                                prev,
                                3usize.saturating_add(usize::try_from(extra).unwrap_or(0)),
                            )
                        }
                        17 => (
                            0,
                            3usize.saturating_add(usize::try_from(need!(z.bits(3))).unwrap_or(0)),
                        ),
                        _ => (
                            0,
                            11usize.saturating_add(usize::try_from(need!(z.bits(7))).unwrap_or(0)),
                        ),
                    };
                    if have.saturating_add(copy) > total {
                        return Err(Error::InvalidHuffmanTable); // "invalid bit length repeat"
                    }
                    for l in lens.iter_mut().skip(have).take(copy) {
                        *l = value;
                    }
                    have = have.saturating_add(copy);
                }
                if lens.get(256).copied().unwrap_or(0) == 0 {
                    return Err(Error::InvalidHuffmanTable); // "invalid code -- missing end-of-block"
                }
                let Some(lit) = build(
                    lens.get(..nlen).unwrap_or(&[]),
                    CodeRole::Litlen,
                    Oracle::Zlib,
                ) else {
                    return Err(Error::InvalidHuffmanTable); // "invalid literal/lengths set"
                };
                let Some(dist) = build(
                    lens.get(nlen..total).unwrap_or(&[]),
                    CodeRole::Distance,
                    Oracle::Zlib,
                ) else {
                    return Err(Error::InvalidHuffmanTable); // "invalid distances set"
                };
                if let Some(halt) = zlib_codes(z, out, written, &lit, &dist)? {
                    return Ok(halt);
                }
            }
            _ => return Err(Error::ReservedBlockType), // "invalid block type"
        }
        if last == 1 {
            break;
        }
    }
    // TYPEDO with `last` set: BYTEBITS, then CHECK: NEEDBITS(32).
    z.byte_align();
    let start = z.pos / 8;
    let Some(check) = z.data.get(start..start.saturating_add(4)) else {
        z.pos = z.data.len().saturating_mul(8);
        return Ok(ZlibHalt::NeedInput);
    };
    let expected = check.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b));
    let actual = adler32(out.get(..*written).unwrap_or(&[]));
    if expected != actual {
        return Err(Error::ChecksumMismatch { expected, actual }); // "incorrect data check"
    }
    z.pos = z.pos.saturating_add(32);
    Ok(ZlibHalt::Done)
}

/// LEN .. LIT for one Huffman block. `Ok(None)`: end of block reached.
fn zlib_codes(
    z: &mut ZlibBits<'_>,
    out: &mut [u8],
    written: &mut usize,
    lit: &Code,
    dist: &Code,
) -> Result<Option<ZlibHalt>> {
    macro_rules! need {
        ($e:expr) => {
            match $e {
                Some(v) => v,
                None => return Ok(Some(ZlibHalt::NeedInput)),
            }
        };
    }
    loop {
        // LEN: decode (zlib's slow path and inflate_fast agree on every
        // decision; the fast path only runs with 258 bytes of room).
        let sym = match need!(decode(lit, CodeRole::Litlen, z)).0 {
            Sym::Symbol(s) => s,
            Sym::Invalid => return Err(Error::InvalidSymbol), // "invalid literal/length code"
        };
        if sym < 256 {
            // LIT
            let Some(slot) = out.get_mut(*written) else {
                return Ok(Some(ZlibHalt::NeedRoom));
            };
            *slot = u8::try_from(sym).unwrap_or(0);
            *written = written.saturating_add(1);
            continue;
        }
        if sym == 256 {
            return Ok(None);
        }
        let li = usize::from(sym.saturating_sub(257));
        let (Some(&base), Some(&extra)) = (LENGTH_BASE.get(li), LENGTH_EXTRA.get(li)) else {
            return Err(Error::InvalidSymbol); // 286, 287: "invalid literal/length code"
        };
        // LENEXT
        let length =
            usize::from(base).saturating_add(usize::try_from(need!(z.bits(extra))).unwrap_or(0));
        // DIST
        let dsym = match need!(decode(dist, CodeRole::Distance, z)).0 {
            Sym::Symbol(s) => usize::from(s),
            Sym::Invalid => return Err(Error::InvalidSymbol), // "invalid distance code"
        };
        let (Some(&dbase), Some(&dextra)) = (DIST_BASE.get(dsym), DIST_EXTRA.get(dsym)) else {
            return Err(Error::InvalidSymbol); // 30, 31: "invalid distance code"
        };
        // DISTEXT
        let distance =
            usize::from(dbase).saturating_add(usize::try_from(need!(z.bits(dextra))).unwrap_or(0));
        // MATCH: room first, then "too far back", then as much as fits.
        if *written == out.len() {
            return Ok(Some(ZlibHalt::NeedRoom));
        }
        if distance > *written {
            return Err(Error::DistanceTooFar); // "invalid distance too far back"
        }
        let n = length.min(out.len().saturating_sub(*written));
        copy_match(out, *written, distance, n);
        *written = written.saturating_add(n);
        if n < length {
            return Ok(Some(ZlibHalt::NeedRoom));
        }
    }
}

/// Byte-at-a-time back-reference copy: overlapping copies repeat, as DEFLATE
/// requires. The caller has checked `distance <= at` and `at + n <= out.len()`.
fn copy_match(out: &mut [u8], at: usize, distance: usize, n: usize) {
    for i in at..at.saturating_add(n) {
        let b = out.get(i.saturating_sub(distance)).copied().unwrap_or(0);
        if let Some(d) = out.get_mut(i) {
            *d = b;
        }
    }
}

// ===========================================================================
// libdeflate 1.24 `libdeflate_zlib_decompress`, no size out-parameter
// ===========================================================================

/// Bits for the libdeflate decoder: the Deflate data between the zlib header
/// and the last four bytes, with implicit zero bytes after it -- and the
/// bookkeeping that makes libdeflate refuse a stream for using them.
///
/// libdeflate refills a 64-bit bit buffer at fixed points of its decoder, to
/// at least 56 bits. Past the end of its input it loads zero bytes, and a
/// refill that would load a ninth one is an error. Its buffer state after a
/// refill at bit position `p` is a function of `p` alone -- it holds
/// `56 + (-p mod 8)` bits, i.e. the bytes up to `ceil((p + 56) / 8)` -- so the
/// emulation below keeps only `loaded` (bytes loaded, zeros included) and the
/// refill *points*, which are copied from decompress_template.h.
struct LdBits<'a> {
    data: &'a [u8],
    /// Bits consumed.
    pos: usize,
    /// Bytes loaded into libdeflate's bit buffer so far, real and zero.
    loaded: usize,
}

impl Bits for LdBits<'_> {
    fn bit(&mut self) -> Option<u32> {
        let b = self
            .data
            .get(self.pos / 8)
            .map_or(0, |byte| u32::from(byte >> (self.pos % 8)) & 1);
        self.pos = self.pos.saturating_add(1);
        Some(b)
    }
}

impl LdBits<'_> {
    /// REFILL_BITS(): top the buffer up to 56+ bits. A ninth zero byte is
    /// BAD_DATA (`SAFETY_CHECK(overread_count <= sizeof(bitbuf_t))`).
    fn refill(&mut self) -> Result<()> {
        self.refill_at(self.pos);
        if self.loaded > self.data.len().saturating_add(8) {
            return Err(Error::UnexpectedEnd);
        }
        Ok(())
    }

    /// The buffer state after a refill made when `p` bits had been consumed:
    /// loaded up to `ceil((p + 56) / 8)` bytes, whichever refill method
    /// libdeflate used. The fastloop's refills go through this directly --
    /// they cannot overread, by construction of its input bound.
    fn refill_at(&mut self, p: usize) {
        self.loaded = self.loaded.max(p.saturating_add(56).div_ceil(8));
    }

    /// Bits in the buffer that have not been consumed (`(u8)bitsleft`).
    fn bits_left(&self) -> usize {
        self.loaded.saturating_mul(8).saturating_sub(self.pos)
    }

    /// Whether any bit consumed so far came from past the real input
    /// (`overread_count > bitsleft >> 3`).
    fn consumed_past_end(&self) -> bool {
        self.pos > self.data.len().saturating_mul(8)
    }
}

/// Inflate the zlib stream `input` into `out` as libdeflate 1.24's
/// `libdeflate_zlib_decompress` does with no size out-parameter -- the call
/// libtiff 4.7.1 makes for a Deflate strip.
///
/// * The Deflate data is taken to end four bytes before the input does; past
///   that it reads as zero bits.
/// * A literal, match or stored block that does not fit stops decoding with
///   none of it written: [`Filled::Full`], which libtiff accepts.
/// * A stream that ends with room left over is [`Error::ShortOutput`], and
///   its checksum is not looked at.
/// * A stream that fills the buffer exactly is [`Filled::Complete`] if its
///   Adler-32 -- read from just after the Deflate data -- matches.
///
/// # Errors
///
/// libdeflate has one "bad data" result; here it maps to the nearest
/// [`Error`] as [`zlib_inflate_into`] documents. Using the implicit zeros
/// past the input -- or loading more than eight bytes of them -- is
/// [`Error::UnexpectedEnd`].
pub fn zlib_decompress_into(input: &[u8], out: &mut [u8]) -> Result<Filled> {
    // ZLIB_MIN_OVERHEAD: header (2) + footer (4).
    if input.len() < 6 {
        return Err(Error::UnexpectedEnd);
    }
    let cmf = u32::from(input.first().copied().unwrap_or(0));
    let flg = u32::from(input.get(1).copied().unwrap_or(0));
    let hdr = (cmf << 8) | flg;
    if hdr % 31 != 0 || cmf & 0x0f != 8 || (hdr >> 12) > 7 {
        return Err(Error::BadWrapperHeader); // FCHECK, CM, CINFO -- in that order
    }
    if (hdr >> 5) & 1 != 0 {
        return Err(Error::PresetDictionary); // FDICT
    }
    let deflate = input.get(2..input.len().saturating_sub(4)).unwrap_or(&[]);
    let mut d = LdBits {
        data: deflate,
        pos: 0,
        loaded: 0,
    };
    let mut written = 0usize;
    match libdeflate_run(&mut d, out, &mut written)? {
        Some(n) => Ok(Filled::Full(n)),
        None => {
            // SUCCESS from the Deflate decoder: the buffer is exactly full.
            // The Adler-32 follows the consumed Deflate bytes.
            let at = 2usize.saturating_add(d.pos.div_ceil(8));
            let check = input.get(at..at.saturating_add(4)).unwrap_or(&[]);
            let expected = check.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b));
            let actual = adler32(out);
            if expected == actual {
                Ok(Filled::Complete)
            } else {
                Err(Error::ChecksumMismatch { expected, actual })
            }
        }
    }
}

/// The Deflate decoder. `Ok(Some(n))`: insufficient space after `n` bytes.
/// `Ok(None)`: the stream ended with the buffer exactly full.
fn libdeflate_run(
    d: &mut LdBits<'_>,
    out: &mut [u8],
    written: &mut usize,
) -> Result<Option<usize>> {
    let fixed = fixed_lengths();
    let mut lens = [0u8; LENS_CAPACITY];
    loop {
        // next_block: REFILL_BITS(), then BFINAL and BTYPE.
        d.refill()?;
        let block_start = d.pos;
        let last = d.bits(1).unwrap_or(0);
        let btype = d.bits(2).unwrap_or(0);
        match btype {
            2 => {
                let nlit = usize::try_from(d.bits(5).unwrap_or(0))
                    .unwrap_or(0)
                    .saturating_add(257);
                let ndist = usize::try_from(d.bits(5).unwrap_or(0))
                    .unwrap_or(0)
                    .saturating_add(1);
                let npre = usize::try_from(d.bits(4).unwrap_or(0))
                    .unwrap_or(0)
                    .saturating_add(4);
                // The first precode length rides in the header's 20 bits;
                // then one refill; then the rest without one.
                let mut pre = [0u8; 19];
                for (i, &slot) in PRECODE_ORDER.iter().take(npre).enumerate() {
                    if i == 1 {
                        debug_assert_eq!(d.pos, block_start.saturating_add(20));
                        d.refill()?;
                    }
                    let v = d.bits(3).unwrap_or(0);
                    if let Some(p) = pre.get_mut(usize::from(slot)) {
                        *p = u8::try_from(v).unwrap_or(0);
                    }
                }
                let Some(precode) = build(&pre, CodeRole::Precode, Oracle::Libdeflate) else {
                    return Err(Error::InvalidHuffmanTable);
                };
                let total = nlit.saturating_add(ndist);
                let mut i = 0usize;
                while i < total {
                    if d.bits_left() < 7 + 7 {
                        d.refill()?;
                    }
                    let presym = match decode(&precode, CodeRole::Precode, d) {
                        Some((Sym::Symbol(s), _)) => s,
                        _ => 0, // libdeflate's codes have no invalid entries
                    };
                    if presym < 16 {
                        if let Some(l) = lens.get_mut(i) {
                            *l = u8::try_from(presym).unwrap_or(0);
                        }
                        i = i.saturating_add(1);
                        continue;
                    }
                    let (value, count) = match presym {
                        16 => {
                            if i == 0 {
                                return Err(Error::InvalidHuffmanTable); // SAFETY_CHECK(i != 0)
                            }
                            let prev = lens.get(i.saturating_sub(1)).copied().unwrap_or(0);
                            (
                                prev,
                                3usize.saturating_add(
                                    usize::try_from(d.bits(2).unwrap_or(0)).unwrap_or(0),
                                ),
                            )
                        }
                        17 => (
                            0,
                            3usize.saturating_add(
                                usize::try_from(d.bits(3).unwrap_or(0)).unwrap_or(0),
                            ),
                        ),
                        _ => (
                            0,
                            11usize.saturating_add(
                                usize::try_from(d.bits(7).unwrap_or(0)).unwrap_or(0),
                            ),
                        ),
                    };
                    // Overrun is allowed here and refused after the loop.
                    for l in lens.iter_mut().skip(i).take(count) {
                        *l = value;
                    }
                    i = i.saturating_add(count);
                }
                if i != total {
                    return Err(Error::InvalidHuffmanTable); // SAFETY_CHECK(i == total)
                }
                // Offset table first, then litlen -- as libdeflate builds them.
                let Some(dist) = build(
                    lens.get(nlit..total).unwrap_or(&[]),
                    CodeRole::Distance,
                    Oracle::Libdeflate,
                ) else {
                    return Err(Error::InvalidHuffmanTable);
                };
                let Some(lit) = build(
                    lens.get(..nlit).unwrap_or(&[]),
                    CodeRole::Litlen,
                    Oracle::Libdeflate,
                ) else {
                    return Err(Error::InvalidHuffmanTable);
                };
                let tablebits = lit.max_len.min(11);
                if let Some(n) = libdeflate_codes(d, out, written, &lit, &dist, tablebits)? {
                    return Ok(Some(n));
                }
            }
            0 => {
                // Align to the byte after the 3 header bits -- unless those
                // bits were themselves past the end.
                if d.consumed_past_end() {
                    return Err(Error::UnexpectedEnd);
                }
                let mut at = d.pos.div_ceil(8);
                let header = d
                    .data
                    .get(at..at.saturating_add(4))
                    .ok_or(Error::UnexpectedEnd)?;
                let len = u16::from_le_bytes([
                    header.first().copied().unwrap_or(0),
                    header.get(1).copied().unwrap_or(0),
                ]);
                let nlen = u16::from_le_bytes([
                    header.get(2).copied().unwrap_or(0),
                    header.get(3).copied().unwrap_or(0),
                ]);
                at = at.saturating_add(4);
                if len != !nlen {
                    return Err(Error::StoredLengthMismatch);
                }
                let len = usize::from(len);
                if len > out.len().saturating_sub(*written) {
                    return Ok(Some(*written)); // INSUFFICIENT_SPACE, nothing copied
                }
                let src = d
                    .data
                    .get(at..at.saturating_add(len))
                    .ok_or(Error::UnexpectedEnd)?;
                let dst = out
                    .get_mut(*written..written.saturating_add(len))
                    .ok_or(Error::UnexpectedEnd)?;
                dst.copy_from_slice(src);
                *written = written.saturating_add(len);
                at = at.saturating_add(len);
                // The bit buffer is empty: what is loaded is what is used.
                d.pos = at.saturating_mul(8);
                d.loaded = at;
            }
            1 => {
                let (Some(lit), Some(dist)) = (
                    build(
                        fixed.get(..288).unwrap_or(&[]),
                        CodeRole::Litlen,
                        Oracle::Libdeflate,
                    ),
                    build(
                        fixed.get(288..).unwrap_or(&[]),
                        CodeRole::Distance,
                        Oracle::Libdeflate,
                    ),
                ) else {
                    return Err(Error::InvalidHuffmanTable);
                };
                // The static litlen table's main table is 9 bits wide.
                if let Some(n) = libdeflate_codes(d, out, written, &lit, &dist, 9)? {
                    return Ok(Some(n));
                }
            }
            _ => return Err(Error::ReservedBlockType),
        }
        if last == 1 {
            break;
        }
    }
    // The last block: no consumed zero bits, then exactly full.
    if d.consumed_past_end() {
        return Err(Error::UnexpectedEnd);
    }
    if *written != out.len() {
        return Err(Error::ShortOutput { produced: *written });
    }
    Ok(None)
}

/// One Huffman block: libdeflate's fastloop while it would run, then its
/// generic loop. `Ok(Some(n))`: insufficient space after `n` bytes.
/// `Ok(None)`: end of block.
///
/// The fastloop matters for exactly one observable thing: it copies a match a
/// machine word at a time and runs up to 39 bytes past the match's end,
/// relying on later writes to overwrite the excess. When decoding later stops
/// for lack of room, what the excess left behind is still in the buffer --
/// and libtiff shows that buffer. So which matches the fastloop copied, and
/// how, is reproduced here; everything else about it (its three-literal
/// unrolling, its preloads) changes no result and is modelled only where it
/// moves a refill.
fn libdeflate_codes(
    d: &mut LdBits<'_>,
    out: &mut [u8],
    written: &mut usize,
    lit: &Code,
    dist: &Code,
    tablebits: u8,
) -> Result<Option<usize>> {
    // in_fastloop_end / out_fastloop_end: FASTLOOP_MAX_BYTES_READ is 25 and
    // FASTLOOP_MAX_BYTES_WRITTEN is 299 on a 64-bit build.
    let in_end = d.data.len();
    let in_fastloop_end = in_end.saturating_sub(in_end.min(25));
    let out_fastloop_end = out.len().saturating_sub(out.len().min(299));
    let in_next = |d: &LdBits<'_>| d.loaded.min(d.data.len());
    if in_next(d) < in_fastloop_end && *written < out_fastloop_end {
        d.refill_at(d.pos);
        loop {
            if fastloop_iteration(d, out, written, lit, dist, tablebits)? {
                return Ok(None); // end of block
            }
            if !(in_next(d) < in_fastloop_end && *written < out_fastloop_end) {
                break;
            }
        }
    }
    // The generic loop.
    loop {
        d.refill()?;
        let sym = match decode(lit, CodeRole::Litlen, d) {
            Some((Sym::Symbol(s), _)) => s,
            _ => 0,
        };
        if sym < 256 {
            let Some(slot) = out.get_mut(*written) else {
                return Ok(Some(*written)); // a literal with the buffer full
            };
            *slot = u8::try_from(sym).unwrap_or(0);
            *written = written.saturating_add(1);
            continue;
        }
        if sym == 256 {
            return Ok(None);
        }
        let length = libdeflate_length(d, sym);
        // Room is checked before the offset is even decoded.
        if length > out.len().saturating_sub(*written) {
            return Ok(Some(*written));
        }
        let distance = libdeflate_distance(d, dist).0;
        if distance > *written {
            return Err(Error::DistanceTooFar); // SAFETY_CHECK(offset <= out_next - out)
        }
        copy_match(out, *written, distance, length);
        *written = written.saturating_add(length);
    }
}

/// The length a litlen symbol above 256 stands for, reading its extra bits.
/// 285, 286 and 287 all mean 258 with none.
fn libdeflate_length(d: &mut LdBits<'_>, sym: u16) -> usize {
    let li = usize::from(sym.saturating_sub(257)).min(28);
    let base = usize::from(LENGTH_BASE.get(li).copied().unwrap_or(258));
    let extra = LENGTH_EXTRA.get(li).copied().unwrap_or(0);
    base.saturating_add(usize::try_from(d.bits(extra).unwrap_or(0)).unwrap_or(0))
}

/// Decode an offset: the distance, and the offset codeword's length.
/// Symbols 30 and 31 decode as 29 does.
fn libdeflate_distance(d: &mut LdBits<'_>, dist: &Code) -> (usize, u8) {
    let (dsym, cw) = match decode(dist, CodeRole::Distance, d) {
        Some((Sym::Symbol(s), cw)) => (usize::from(s), cw),
        _ => (0, 1),
    };
    let di = dsym.min(29);
    let dbase = usize::from(DIST_BASE.get(di).copied().unwrap_or(24577));
    let dextra = DIST_EXTRA.get(di).copied().unwrap_or(13);
    let distance = dbase.saturating_add(usize::try_from(d.bits(dextra).unwrap_or(0)).unwrap_or(0));
    (distance, cw)
}

/// One iteration of decompress_template.h's fastloop (64-bit build).
/// `Ok(true)`: end of block.
fn fastloop_iteration(
    d: &mut LdBits<'_>,
    out: &mut [u8],
    written: &mut usize,
    lit: &Code,
    dist: &Code,
    tablebits: u8,
) -> Result<bool> {
    let mut next = || match decode(lit, CodeRole::Litlen, d) {
        Some((Sym::Symbol(s), cw)) => (s, cw),
        _ => (0, 1),
    };
    // A "fast literal" is a literal found in the main table, i.e. whose
    // codeword is no longer than the table is wide.
    let fast_literal = |(s, cw): (u16, u8)| s < 256 && cw <= tablebits;
    let mut put = |out: &mut [u8], s: u16| {
        if let Some(slot) = out.get_mut(*written) {
            *slot = u8::try_from(s).unwrap_or(0);
        }
        *written = written.saturating_add(1);
    };

    // The entry preloaded by the previous iteration, consumed now.
    let e1 = next();
    let entry = if fast_literal(e1) {
        // Up to two more fast literals, each consumed before the previous
        // one is written; a refill only after the third.
        let e2 = next();
        put(out, e1.0);
        if fast_literal(e2) {
            let e3 = next();
            put(out, e2.0);
            if fast_literal(e3) {
                d.refill_at(d.pos);
                put(out, e3.0);
                return Ok(false);
            }
            e3
        } else {
            e2
        }
    } else {
        e1
    };
    let (sym, cw) = entry;
    if sym == 256 {
        return Ok(true); // end of block, main table or subtable
    }
    if cw > tablebits && sym < 256 {
        // A literal that needed a subtable: preload, refill, write.
        d.refill_at(d.pos);
        put(out, sym);
        return Ok(false);
    }
    // A length (main table or subtable), then its offset. The refill before
    // the offset depends on whether the offset codeword needs a subtable
    // (more than 8 bits), which libdeflate knows from a peek; here it is
    // decided after decoding, and applied at the position the peek was made.
    let length = libdeflate_length(d, sym);
    let at = d.pos;
    let left = d.bits_left();
    let (distance, dcw) = libdeflate_distance(d, dist);
    let threshold = if dcw > 8 {
        28 + 11 - 1
    } else {
        13 + 8 + 11 - 1
    };
    if left < threshold {
        d.refill_at(at);
    }
    if distance > *written {
        return Err(Error::DistanceTooFar);
    }
    let start = *written;
    *written = written.saturating_add(length);
    // Preload the next entry and refill, then copy the match.
    d.refill_at(d.pos);
    fastloop_copy(out, start, distance, length);
    Ok(false)
}

/// libdeflate's fastloop match copy, overrun included: whole words, five at
/// a time for an offset of at least a word, four at a time for an offset of
/// one, and two at a time, advancing by the offset, for the rest. The
/// caller's fastloop bounds leave room for the overrun; a word that would not
/// fit is skipped rather than risk a panic.
fn fastloop_copy(out: &mut [u8], start: usize, offset: usize, length: usize) {
    let end = start.saturating_add(length);
    let mut src = start.saturating_sub(offset);
    let mut dst = start;
    let len = out.len();
    let word = |out: &mut [u8], s: usize, t: usize| {
        if s.saturating_add(8) <= len && t.saturating_add(8) <= len {
            out.copy_within(s..s.saturating_add(8), t);
        }
    };
    if offset >= 8 {
        loop {
            for _ in 0..5 {
                word(out, src, dst);
                src = src.saturating_add(8);
                dst = dst.saturating_add(8);
            }
            if dst >= end {
                break;
            }
        }
    } else if offset == 1 {
        let v = out.get(src).copied().unwrap_or(0);
        loop {
            for _ in 0..4 {
                if let Some(w) = out.get_mut(dst..dst.saturating_add(8)) {
                    w.fill(v);
                }
                dst = dst.saturating_add(8);
            }
            if dst >= end {
                break;
            }
        }
    } else {
        for _ in 0..2 {
            word(out, src, dst);
            src = src.saturating_add(offset);
            dst = dst.saturating_add(offset);
        }
        loop {
            for _ in 0..2 {
                word(out, src, dst);
                src = src.saturating_add(offset);
                dst = dst.saturating_add(offset);
            }
            if dst >= end {
                break;
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// A DEFLATE bit writer: fields least-significant bit first, Huffman
    /// codes most-significant first.
    #[derive(Default)]
    struct Bw {
        bits: Vec<u8>,
    }

    impl Bw {
        fn put(&mut self, v: u32, n: u32) {
            for i in 0..n {
                self.bits.push(((v >> i) & 1) as u8);
            }
        }
        fn code(&mut self, c: u32, n: u32) {
            for i in (0..n).rev() {
                self.bits.push(((c >> i) & 1) as u8);
            }
        }
        /// A fixed-Huffman litlen symbol.
        fn lit(&mut self, sym: u32) {
            match sym {
                0..=143 => self.code(0x30 + sym, 8),
                144..=255 => self.code(0x190 + sym - 144, 9),
                256..=279 => self.code(sym - 256, 7),
                _ => self.code(0xC0 + sym - 280, 8),
            }
        }
        fn align(&mut self) {
            while !self.bits.len().is_multiple_of(8) {
                self.bits.push(0);
            }
        }
        fn bytes(&self) -> Vec<u8> {
            self.bits
                .chunks(8)
                .map(|c| c.iter().enumerate().fold(0u8, |a, (i, &b)| a | (b << i)))
                .collect()
        }
    }

    /// `deflate` wrapped as a zlib stream whose trailer is the Adler-32 of
    /// `content`.
    fn zlib(deflate: &[u8], content: &[u8]) -> Vec<u8> {
        let mut s = vec![0x78, 0x9c];
        s.extend_from_slice(deflate);
        s.extend_from_slice(&adler32(content).to_be_bytes());
        s
    }

    #[test]
    fn a_whole_stream_fills_its_buffer_either_way() {
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        let stream = crate::zlib_deflate(&data);
        let mut a = vec![0u8; data.len()];
        assert_eq!(
            zlib_inflate_into(&stream, &mut a),
            Ok(ZlibStop::Ended(data.len()))
        );
        assert_eq!(a, data);
        let mut b = vec![0u8; data.len()];
        assert_eq!(zlib_decompress_into(&stream, &mut b), Ok(Filled::Complete));
        assert_eq!(b, data);
    }

    #[test]
    fn a_stream_that_ends_early_is_ended_to_zlib_and_short_to_libdeflate() {
        let data = b"a short stream";
        let stream = crate::zlib_deflate(data);
        let mut out = [0u8; 20];
        assert_eq!(
            zlib_inflate_into(&stream, &mut out),
            Ok(ZlibStop::Ended(data.len()))
        );
        assert_eq!(
            zlib_decompress_into(&stream, &mut out),
            Err(Error::ShortOutput {
                produced: data.len()
            })
        );
    }

    #[test]
    fn a_stored_block_that_does_not_fit_is_cut_by_zlib_and_skipped_by_libdeflate() {
        let payload: Vec<u8> = (0..100u8).collect();
        let mut d = vec![0x01];
        d.extend_from_slice(&100u16.to_le_bytes());
        d.extend_from_slice(&(!100u16).to_le_bytes());
        d.extend_from_slice(&payload);
        let stream = zlib(&d, &payload);

        let mut out = [0xEEu8; 50];
        assert_eq!(zlib_inflate_into(&stream, &mut out), Ok(ZlibStop::Full));
        assert_eq!(&out[..], &payload[..50], "zlib copies what fits");

        let mut out = [0xEEu8; 50];
        assert_eq!(zlib_decompress_into(&stream, &mut out), Ok(Filled::Full(0)));
        assert!(
            out.iter().all(|&b| b == 0xEE),
            "libdeflate copies none of it"
        );
    }

    #[test]
    fn litlen_286_is_an_error_to_zlib_and_a_length_of_258_to_libdeflate() {
        let mut bw = Bw::default();
        bw.put(1, 1);
        bw.put(1, 2);
        bw.lit(u32::from(b'a'));
        bw.lit(u32::from(b'b'));
        bw.lit(286);
        bw.code(0, 5); // distance 1
        bw.lit(256);
        let mut content = b"ab".to_vec();
        content.extend_from_slice(&[b'b'; 258]);
        let stream = zlib(&bw.bytes(), &content);

        let mut out = vec![0u8; content.len()];
        assert_eq!(
            zlib_inflate_into(&stream, &mut out),
            Err(Error::InvalidSymbol)
        );
        let mut out = vec![0u8; content.len()];
        assert_eq!(
            zlib_decompress_into(&stream, &mut out),
            Ok(Filled::Complete)
        );
        assert_eq!(out, content);
    }

    #[test]
    fn zlib_does_not_check_a_distance_it_has_no_room_to_copy() {
        // 'a', then a match of 3 at distance 5: further back than the output.
        let mut bw = Bw::default();
        bw.put(1, 1);
        bw.put(1, 2);
        bw.lit(u32::from(b'a'));
        bw.lit(257); // length 3
        bw.code(4, 5); // distance code 4: base 5, 1 extra bit
        bw.put(0, 1);
        bw.lit(256);
        let stream = zlib(&bw.bytes(), b"a");

        // Room for 'a' only: zlib decodes the match, then stops for room
        // before its "too far back" check is ever reached.
        let mut out = [0u8; 1];
        assert_eq!(zlib_inflate_into(&stream, &mut out), Ok(ZlibStop::Full));
        // With a byte to spare it reaches the check.
        let mut out = [0u8; 2];
        assert_eq!(
            zlib_inflate_into(&stream, &mut out),
            Err(Error::DistanceTooFar)
        );
        // libdeflate checks room before it decodes the distance at all.
        let mut out = [0u8; 2];
        assert_eq!(zlib_decompress_into(&stream, &mut out), Ok(Filled::Full(1)));
    }

    #[test]
    fn libdeflate_refuses_a_stream_that_used_the_zero_bits_past_its_end() {
        // Fixed block: 'A' then end-of-block -- 18 bits, three bytes.
        let mut bw = Bw::default();
        bw.put(1, 1);
        bw.put(1, 2);
        bw.lit(u32::from(b'A'));
        bw.lit(256);
        let deflate = bw.bytes();
        assert_eq!(deflate.len(), 3);
        // Without its third byte, the end-of-block code's last two bits are
        // read from past the end (as zeros, which is what they were).
        let stream = zlib(&deflate[..2], b"A");
        let mut out = [0u8; 1];
        assert_eq!(
            zlib_decompress_into(&stream, &mut out),
            Err(Error::UnexpectedEnd)
        );
        // Whole, it is fine.
        let stream = zlib(&deflate, b"A");
        assert_eq!(
            zlib_decompress_into(&stream, &mut out),
            Ok(Filled::Complete)
        );
    }

    #[test]
    fn libdeflate_leaves_its_match_copy_overrun_in_the_buffer() {
        // "0123456789", a match of 10 at distance 10, then a stored block too
        // big to fit. libdeflate's fastloop copies the match five words at a
        // time -- 40 bytes, 30 past the match -- and the stored block that
        // stops it overwrites none of them.
        let mut bw = Bw::default();
        bw.put(0, 1);
        bw.put(1, 2);
        for c in b"0123456789" {
            bw.lit(u32::from(*c));
        }
        bw.lit(264); // length 10
        bw.code(6, 5); // distance code 6: base 9, 2 extra bits
        bw.put(1, 2); // distance 10
        bw.lit(256);
        bw.put(1, 1);
        bw.put(0, 2);
        bw.align();
        let mut d = bw.bytes();
        d.extend_from_slice(&1000u16.to_le_bytes());
        d.extend_from_slice(&(!1000u16).to_le_bytes());
        d.extend_from_slice(&[0x55; 1000]);
        let stream = zlib(&d, b"");

        let mut out = [0xEEu8; 400];
        assert_eq!(
            zlib_decompress_into(&stream, &mut out),
            Ok(Filled::Full(20))
        );
        let pattern: Vec<u8> = b"0123456789".iter().copied().cycle().take(50).collect();
        assert_eq!(
            &out[..50],
            &pattern[..],
            "20 bytes decoded, 30 more overrun"
        );
        assert!(out[50..].iter().all(|&b| b == 0xEE), "and nothing beyond");

        // zlib copies the stored block as far as it fits.
        let mut out = [0xEEu8; 400];
        assert_eq!(zlib_inflate_into(&stream, &mut out), Ok(ZlibStop::Full));
        assert_eq!(&out[..20], &pattern[..20]);
        assert!(out[20..].iter().all(|&b| b == 0x55));
    }

    #[test]
    fn a_full_buffer_does_not_stop_zlib_finding_an_error_after_it() {
        // Block one fills the buffer; block two has type 3.
        let mut bw = Bw::default();
        bw.put(0, 1);
        bw.put(1, 2);
        bw.lit(u32::from(b'x'));
        bw.lit(256);
        bw.put(1, 1);
        bw.put(3, 2);
        let stream = zlib(&bw.bytes(), b"x");
        let mut out = [0u8; 1];
        assert_eq!(
            zlib_inflate_into(&stream, &mut out),
            Err(Error::ReservedBlockType)
        );
        // ...but running out of input after the buffer is full is fine: two
        // bytes hold the 'x' and part of the end-of-block code after it.
        let stream = zlib(&bw.bytes()[..2], b"x");
        assert_eq!(
            zlib_inflate_into(&stream[..4], &mut out),
            Ok(ZlibStop::Full)
        );
    }

    #[test]
    fn the_headers_are_checked_in_each_librarys_order() {
        let mut out = [0u8; 4];
        for bad in [&[0x78u8, 0x9d][..], &[0x79, 0x9c], &[0x88, 0x98]] {
            let mut s = bad.to_vec();
            s.extend_from_slice(&[3, 0, 0, 0, 0, 1]);
            assert_eq!(
                zlib_inflate_into(&s, &mut out),
                Err(Error::BadWrapperHeader)
            );
            assert_eq!(
                zlib_decompress_into(&s, &mut out),
                Err(Error::BadWrapperHeader)
            );
        }
        // FDICT: a dictionary id, then zlib asks for the dictionary.
        let s = [0x78u8, 0xbb, 0, 0, 0, 1, 3, 0, 0, 0, 0, 1];
        assert_eq!(
            zlib_inflate_into(&s, &mut out),
            Err(Error::PresetDictionary)
        );
        assert_eq!(
            zlib_decompress_into(&s, &mut out),
            Err(Error::PresetDictionary)
        );
        // Fewer than six bytes is libdeflate's bad data; to zlib, no input.
        assert_eq!(
            zlib_decompress_into(&[0x78, 0x9c, 3, 0], &mut out),
            Err(Error::UnexpectedEnd)
        );
        assert_eq!(zlib_inflate_into(&[], &mut out), Err(Error::UnexpectedEnd));
    }
}
