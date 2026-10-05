//! zlib's `inflate`, as libmagic's `uncompresszlib` calls it: once, with all
//! of the input and room for `bytes_max` bytes of output, flushing
//! (`Z_SYNC_FLUSH`).
//!
//! # Why `file` has its own
//!
//! The system's DEFLATE decoder is the `deflate` crate, and everything that
//! only needs the bytes back should use it. `file -z` needs more than the
//! bytes: it prints what zlib *says*. A damaged stream is described as
//! `ERROR:[zlib: invalid distance too far back]`, in zlib's words for the
//! first fault zlib meets, and a truncated one is not an error at all -- zlib
//! hands back every byte the input decodes to and `file` describes those. So
//! this decoder reproduces zlib's observable behaviour rather than DEFLATE's:
//!
//! * zlib's messages, each raised at the point zlib raises it -- the dozen
//!   table faults the `deflate` crate folds into one variant included;
//! * zlib's acceptance rules for Huffman codes (`inftrees.c`): an incomplete
//!   code is allowed only when it is a single one-bit code, and a code with no
//!   symbols at all is allowed and fails on first use;
//! * running out of input is not an error: the output is every symbol the
//!   bits present decode to, and as much of a stored block as is there;
//! * a full output buffer stops only a literal, a match or a stored copy --
//!   block headers, tables, an end-of-block and the zlib trailer after it are
//!   still read, so their faults are still reported, as zlib does.
//!
//! The decoder reads codes a bit at a time, canonically (as Mark Adler's
//! `puff.c` does), which needs exactly the bits a code has -- the same
//! condition zlib's table lookups test -- and so stops at the same symbol.

/// What `inflate` came to: the output, for `Z_OK` and `Z_STREAM_END`; zlib's
/// message (`z.msg`, or `zError` of the code) otherwise.
pub type Inflated = Result<Vec<u8>, &'static str>;

/// The most bits a DEFLATE code has.
const MAXBITS: usize = 15;

/// The order the code-length code lengths come in.
const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Lengths 257..285: base and extra bits.
const LBASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEXT: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

/// Distances 0..29: base and extra bits.
const DBASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];

/// The input, read a bit at a time, least significant first.
struct Bits<'a> {
    data: &'a [u8],
    /// Bits read so far.
    pos: usize,
}

impl Bits<'_> {
    /// Whether `n` more bits are there.
    fn need(&self, n: u32) -> bool {
        self.data.len().saturating_mul(8).saturating_sub(self.pos) >= n as usize
    }

    /// `n` bits (at most 16), which must be there.
    fn take(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            let byte = self.data.get(self.pos / 8).copied().unwrap_or(0);
            let bit = u32::from((byte >> (self.pos % 8)) & 1);
            v |= bit << i;
            self.pos += 1;
        }
        v
    }

    /// `BYTEBITS`: on to the next byte boundary.
    fn align(&mut self) {
        self.pos = self.pos.div_ceil(8) * 8;
    }

    /// Whole bytes left, from a byte boundary.
    fn bytes_left(&self) -> usize {
        self.data.len().saturating_sub(self.pos / 8)
    }

    /// The next `n` whole bytes, which must be there.
    fn take_bytes(&mut self, n: usize) -> &[u8] {
        let at = self.pos / 8;
        self.pos += n * 8;
        self.data.get(at..at + n).unwrap_or_default()
    }
}

/// Which table `inflate_table` is building: the code-length code may not be
/// incomplete at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Codes,
    Lens,
}

/// A canonical Huffman code: how many codes of each length, and the symbols
/// in code order.
struct Huff {
    count: [u16; MAXBITS + 1],
    symbol: Vec<u16>,
    /// The longest length; 0 for a code with no symbols, every bit pattern of
    /// which is invalid.
    max: usize,
}

/// `inflate_table`'s acceptance rules: `None` for an over-subscribed code, or
/// an incomplete one other than a single one-bit code.
fn build(lens: &[u16], kind: Kind) -> Option<Huff> {
    let mut count = [0u16; MAXBITS + 1];
    for &l in lens {
        if let Some(c) = count.get_mut(usize::from(l)) {
            *c += 1;
        }
    }
    let max = (1..=MAXBITS).rev().find(|&l| count[l] != 0).unwrap_or(0);
    if max == 0 {
        // "no symbols to code at all": accepted, and any code is invalid.
        return Some(Huff {
            count,
            symbol: Vec::new(),
            max: 0,
        });
    }
    let mut left: i32 = 1;
    for &c in &count[1..] {
        left <<= 1;
        left -= i32::from(c);
        if left < 0 {
            return None;
        }
    }
    if left > 0 && (kind == Kind::Codes || max != 1) {
        return None;
    }
    let mut offs = [0usize; MAXBITS + 1];
    for len in 1..MAXBITS {
        offs[len + 1] = offs[len] + usize::from(count[len]);
    }
    let total = offs[MAXBITS] + usize::from(count[MAXBITS]);
    let mut symbol = vec![0u16; total];
    for (sym, &l) in lens.iter().enumerate() {
        if l != 0 {
            let l = usize::from(l);
            if let Some(slot) = symbol.get_mut(offs[l]) {
                #[allow(clippy::cast_possible_truncation)]
                {
                    *slot = sym as u16;
                }
            }
            offs[l] += 1;
        }
    }
    Some(Huff { count, symbol, max })
}

/// What decoding one code came to.
enum Dec {
    Sym(u16),
    /// A bit pattern the code does not assign.
    Invalid,
    /// The input ended inside the code.
    Short,
}

fn decode(s: &mut Bits<'_>, h: &Huff) -> Dec {
    if h.max == 0 {
        // zlib's table for no symbols: two invalid one-bit entries.
        if !s.need(1) {
            return Dec::Short;
        }
        s.take(1);
        return Dec::Invalid;
    }
    let mut code: i32 = 0;
    let mut first: i32 = 0;
    let mut index: i32 = 0;
    for len in 1..=h.max {
        if !s.need(1) {
            return Dec::Short;
        }
        code |= s.take(1) as i32;
        let count = i32::from(h.count[len]);
        if code - count < first {
            #[allow(clippy::cast_sign_loss)]
            let at = (index + (code - first)) as usize;
            return h.symbol.get(at).map_or(Dec::Invalid, |&v| Dec::Sym(v));
        }
        index += count;
        first += count;
        first <<= 1;
        code <<= 1;
    }
    // Only an incomplete code gets here: its unassigned pattern.
    Dec::Invalid
}

/// Why a block's codes stopped.
enum Flow {
    /// The end-of-block code: on to the next block.
    EndOfBlock,
    /// Out of input, or out of room: `Z_OK` with what there is.
    Leave,
}

/// The codes of a fixed or dynamic block (`LEN` through `MATCH`).
fn codes(s: &mut Bits<'_>, out: &mut Vec<u8>, room: usize, lencode: &Huff, distcode: &Huff) -> Result<Flow, &'static str> {
    loop {
        let sym = match decode(s, lencode) {
            Dec::Short => return Ok(Flow::Leave),
            Dec::Invalid => return Err("invalid literal/length code"),
            Dec::Sym(v) => usize::from(v),
        };
        if sym < 256 {
            if out.len() >= room {
                return Ok(Flow::Leave);
            }
            #[allow(clippy::cast_possible_truncation)]
            out.push(sym as u8);
            continue;
        }
        if sym == 256 {
            return Ok(Flow::EndOfBlock);
        }
        let (Some(&lbase), Some(&lext)) = (LBASE.get(sym - 257), LEXT.get(sym - 257)) else {
            // 286 and 287: in the fixed code, and invalid.
            return Err("invalid literal/length code");
        };
        if !s.need(lext) {
            return Ok(Flow::Leave);
        }
        let mut length = (lbase + s.take(lext)) as usize;
        let d = match decode(s, distcode) {
            Dec::Short => return Ok(Flow::Leave),
            Dec::Invalid => return Err("invalid distance code"),
            Dec::Sym(v) => usize::from(v),
        };
        let (Some(&dbase), Some(&dext)) = (DBASE.get(d), DEXT.get(d)) else {
            // 30 and 31, in the fixed code.
            return Err("invalid distance code");
        };
        if !s.need(dext) {
            return Ok(Flow::Leave);
        }
        let dist = (dbase + s.take(dext)) as usize;
        // MATCH: no room stops it before the distance is checked.
        if out.len() >= room {
            return Ok(Flow::Leave);
        }
        if dist > out.len() {
            return Err("invalid distance too far back");
        }
        while length > 0 {
            if out.len() >= room {
                return Ok(Flow::Leave);
            }
            let at = out.len() - dist;
            let c = out.get(at).copied().unwrap_or(0);
            out.push(c);
            length -= 1;
        }
    }
}

/// The fixed code's literal/length and distance code lengths.
fn fixed() -> (Huff, Huff) {
    let mut lens = [0u16; 288];
    for (i, l) in lens.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let dists = [5u16; 32];
    // Both are complete codes: `build` cannot refuse them.
    let empty = || Huff {
        count: [0; MAXBITS + 1],
        symbol: Vec::new(),
        max: 0,
    };
    (
        build(&lens, Kind::Lens).unwrap_or_else(empty),
        build(&dists, Kind::Lens).unwrap_or_else(empty),
    )
}

/// A dynamic block's header: its two codes, or `None` when the input ends
/// inside it.
fn dynamic(s: &mut Bits<'_>) -> Result<Option<(Huff, Huff)>, &'static str> {
    if !s.need(14) {
        return Ok(None);
    }
    let nlen = s.take(5) as usize + 257;
    let ndist = s.take(5) as usize + 1;
    let ncode = s.take(4) as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err("too many length or distance symbols");
    }
    let mut lens = [0u16; 320];
    for &o in ORDER.iter().take(ncode) {
        if !s.need(3) {
            return Ok(None);
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            lens[o] = s.take(3) as u16;
        }
    }
    let Some(lencode) = build(&lens[..19], Kind::Codes) else {
        return Err("invalid code lengths set");
    };
    let mut have = 0usize;
    while have < nlen + ndist {
        // zlib does not look at the entry's validity here: an invalid one
        // (a code-length code with no symbols) reads as length 0.
        let sym = match decode(s, &lencode) {
            Dec::Short => return Ok(None),
            Dec::Invalid => 0,
            Dec::Sym(v) => v,
        };
        if sym < 16 {
            lens[have] = sym;
            have += 1;
            continue;
        }
        let (len, copy) = match sym {
            16 => {
                if !s.need(2) {
                    return Ok(None);
                }
                if have == 0 {
                    return Err("invalid bit length repeat");
                }
                (lens[have - 1], 3 + s.take(2) as usize)
            }
            17 => {
                if !s.need(3) {
                    return Ok(None);
                }
                (0, 3 + s.take(3) as usize)
            }
            _ => {
                if !s.need(7) {
                    return Ok(None);
                }
                (0, 11 + s.take(7) as usize)
            }
        };
        if have + copy > nlen + ndist {
            return Err("invalid bit length repeat");
        }
        for slot in &mut lens[have..have + copy] {
            *slot = len;
        }
        have += copy;
    }
    if lens[256] == 0 {
        return Err("invalid code -- missing end-of-block");
    }
    let Some(lit) = build(&lens[..nlen], Kind::Lens) else {
        return Err("invalid literal/lengths set");
    };
    let Some(dist) = build(&lens[nlen..nlen + ndist], Kind::Lens) else {
        return Err("invalid distances set");
    };
    Ok(Some((lit, dist)))
}

/// Adler-32 of `data`.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &c in chunk {
            a += u32::from(c);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// `inflateInit` (`zlib`, a zlib-wrapped stream) or `inflateInit2(-15)` (raw
/// DEFLATE), then one `inflate(Z_SYNC_FLUSH)` into `room` bytes.
pub fn inflate(input: &[u8], room: usize, zlib: bool) -> Inflated {
    let mut s = Bits { data: input, pos: 0 };
    let mut out = Vec::new();
    if zlib {
        // HEAD
        if !s.need(16) {
            return Ok(out);
        }
        let b0 = s.take(8);
        let b1 = s.take(8);
        if !((b0 << 8) + b1).is_multiple_of(31) {
            return Err("incorrect header check");
        }
        if b0 & 0xf != 8 {
            return Err("unknown compression method");
        }
        if (b0 >> 4) + 8 > 15 {
            return Err("invalid window size");
        }
        if b1 & 0x20 != 0 {
            // DICTID, then DICT: `Z_NEED_DICT`, which sets no message.
            if !s.need(32) {
                return Ok(out);
            }
            return Err("need dictionary");
        }
    }
    let mut last = false;
    loop {
        // TYPE
        if last {
            s.align();
            if zlib {
                // CHECK
                if !s.need(32) {
                    return Ok(out);
                }
                let mut check = 0u32;
                for _ in 0..4 {
                    check = (check << 8) | s.take(8);
                }
                if check != adler32(&out) {
                    return Err("incorrect data check");
                }
            }
            return Ok(out);
        }
        if !s.need(3) {
            return Ok(out);
        }
        last = s.take(1) == 1;
        let flow = match s.take(2) {
            0 => {
                // STORED, then COPY: as much as there is, and room for.
                s.align();
                if !s.need(32) {
                    return Ok(out);
                }
                let len = s.take(16);
                let nlen = s.take(16);
                if len != (nlen ^ 0xffff) {
                    return Err("invalid stored block lengths");
                }
                let mut length = len as usize;
                while length > 0 {
                    let copy = length.min(s.bytes_left()).min(room.saturating_sub(out.len()));
                    if copy == 0 {
                        return Ok(out);
                    }
                    out.extend_from_slice(s.take_bytes(copy));
                    length -= copy;
                }
                Flow::EndOfBlock
            }
            1 => {
                let (lit, dist) = fixed();
                codes(&mut s, &mut out, room, &lit, &dist)?
            }
            2 => match dynamic(&mut s)? {
                Some((lit, dist)) => codes(&mut s, &mut out, room, &lit, &dist)?,
                None => Flow::Leave,
            },
            _ => return Err("invalid block type"),
        };
        if let Flow::Leave = flow {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `printf 'hello\n' | gzip -n | tail -c +11` without its trailer: a
    /// fixed-code block.
    const HELLO_RAW: [u8; 8] = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0xe7, 0x02, 0x00];

    #[test]
    fn a_raw_stream_decodes() {
        assert_eq!(inflate(&HELLO_RAW, 100, false).as_deref(), Ok(&b"hello\n"[..]));
    }

    #[test]
    fn a_truncated_stream_gives_what_it_has() {
        // Four bytes hold the block header and the first three literals.
        assert_eq!(inflate(&HELLO_RAW[..4], 100, false).as_deref(), Ok(&b"hel"[..]));
        assert_eq!(inflate(&[], 100, false).as_deref(), Ok(&b""[..]));
    }

    #[test]
    fn the_room_stops_literals_but_not_the_end() {
        assert_eq!(inflate(&HELLO_RAW, 2, false).as_deref(), Ok(&b"he"[..]));
    }

    #[test]
    fn a_zlib_stream_is_checked() {
        // zlib.compress(b"hello\n")
        let z = [0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0xe7, 0x02, 0x00, 0x08, 0x4b, 0x02, 0x1f];
        assert_eq!(inflate(&z, 100, true).as_deref(), Ok(&b"hello\n"[..]));
        let mut bad = z;
        bad[13] ^= 1;
        assert_eq!(inflate(&bad, 100, true), Err("incorrect data check"));
        assert_eq!(inflate(&z[..12], 100, true).as_deref(), Ok(&b"hello\n"[..]));
        assert_eq!(inflate(&[0x78, 0x9d], 100, true), Err("incorrect header check"));
        assert_eq!(inflate(&[0x78, 0xbb, 0, 0, 0, 0], 100, true), Err("need dictionary"));
    }

    #[test]
    fn zlibs_messages_for_faults() {
        // BFINAL 1, BTYPE 3.
        assert_eq!(inflate(&[0x07], 100, false), Err("invalid block type"));
        // A stored block whose lengths disagree.
        assert_eq!(inflate(&[0x01, 0x05, 0x00, 0x00, 0x00], 100, false), Err("invalid stored block lengths"));
        // A fixed block whose first code is a match of distance 1.
        assert_eq!(inflate(&[0x03, 0x02], 100, false), Err("invalid distance too far back"));
    }

    #[test]
    fn a_stored_block_copies_what_is_there() {
        let s = [0x01, 0x05, 0x00, 0xfa, 0xff, b'a', b'b', b'c'];
        assert_eq!(inflate(&s, 100, false).as_deref(), Ok(&b"abc"[..]));
    }

    #[test]
    fn incomplete_codes_follow_inftrees() {
        // One one-bit code: allowed.
        assert!(build(&[1, 0, 0], Kind::Lens).is_some());
        // ... but not for the code-length code.
        assert!(build(&[1, 0, 0], Kind::Codes).is_none());
        // Two codes of lengths 1 and 2: incomplete, refused.
        assert!(build(&[1, 2], Kind::Lens).is_none());
        // Over-subscribed.
        assert!(build(&[1, 1, 1], Kind::Lens).is_none());
        // Nothing at all: accepted.
        assert!(build(&[0, 0], Kind::Lens).is_some_and(|h| h.max == 0));
    }

    #[test]
    fn adler32_matches_the_reference_value() {
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }
}
