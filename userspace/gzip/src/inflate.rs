//! `inflate.c`: Mark Adler's inflate, decoding deflate's three block kinds
//! into the window, which is written out each time it fills.
//!
//! Upstream links its decoding tables with pointers and frees them as a
//! list; here a block's tables live in one array (`Vec<Huft>`) and link by
//! index. Every lookup a crafted stream could steer is checked, and one
//! outside the tables -- which the algorithm's own invariants already rule
//! out -- is reported as data that violates the format.

use crate::{Gz, Io, Res, WSIZE};

/// The window's mask: it is circular.
const WMASK: u32 = WSIZE as u32 - 1;
/// The order of the bit-length code lengths.
const BORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
/// Copy lengths for literal/length codes 257-285 (and two that are invalid).
const CPLENS: [u16; 31] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258, 0, 0,
];
/// Extra bits for literal/length codes 257-285; 99 is invalid.
const CPLEXT: [u16; 31] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0, 99, 99,
];
/// Copy offsets for distance codes 0-29.
const CPDIST: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// Extra bits for distance codes.
const CPDEXT: [u16; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// `mask_bits[n]`: the low `n` bits.
const MASK_BITS: [u32; 17] = [
    0x0000, 0x0001, 0x0003, 0x0007, 0x000f, 0x001f, 0x003f, 0x007f, 0x00ff, 0x01ff, 0x03ff, 0x07ff,
    0x0fff, 0x1fff, 0x3fff, 0x7fff, 0xffff,
];
/// Bits in the first lookup of the literal/length and the distance tables.
const LBITS: i32 = 9;
const DBITS: i32 = 6;
/// The longest code, and the most codes in a set.
const BMAX: usize = 16;
const N_MAX: usize = 288;

/// Inflate's bit buffer, which upstream keeps between blocks.
#[derive(Debug, Default, Clone, Copy)]
pub struct Inf {
    /// Bits not yet used, low first.
    pub bb: u64,
    /// How many.
    pub bk: u32,
}

/// One table entry: `struct huft`. `e` is the extra bits (0-13), 15 for
/// end of block, 16 for a literal, 16 + n for a link to an n-bit table, 99
/// for a code that is not valid; `b` the bits this entry consumes; `n` the
/// literal or base value, or `t` the linked table's start.
#[derive(Debug, Default, Clone, Copy)]
struct Huft {
    e: u8,
    b: u8,
    n: u16,
    t: usize,
}

/// A table set: entries, and where the first-level table starts.
struct Tables {
    arena: Vec<Huft>,
    start: usize,
}

/// What `huft_build` answers besides its tables: 0 complete, 1
/// incomplete (the tables are usable), 2 invalid.
type Built = (i32, Tables, i32);

/// The result codes inflate answers with: 1 and 2 are corrupt data.
const BAD: i32 = 2;

/// `huft_build (b, n, s, d, e, &t, &m)`: decoding tables for the code
/// lengths `b`, the first `s` values simple and the rest looked up in `d`
/// (base) and `e` (extra bits); `m` the bits wanted in the first lookup.
/// Answers the result code, the tables and the first lookup's bits.
fn huft_build(b: &[u32], s: u32, d: &[u16], e: &[u16], m: i32) -> Built {
    let n = b.len();
    let empty = || Tables {
        arena: Vec::new(),
        start: 0,
    };
    let mut c = [0u32; BMAX + 1];
    for &len in b {
        match c.get_mut(len as usize) {
            Some(slot) => *slot += 1,
            None => return (BAD, empty(), m),
        }
    }
    if c[0] as usize == n {
        // All codes zero-length: a two-entry table of invalid codes.
        let invalid = Huft {
            e: 99,
            b: 1,
            n: 0,
            t: 0,
        };
        return (
            0,
            Tables {
                arena: vec![Huft::default(), invalid, invalid],
                start: 1,
            },
            1,
        );
    }

    let mut l = m;
    let mut j = 1usize;
    while j <= BMAX {
        if c[j] != 0 {
            break;
        }
        j += 1;
    }
    let mut k = j as i32;
    if (l as usize) < j {
        l = j as i32;
    }
    let mut i = BMAX;
    while i != 0 {
        if c[i] != 0 {
            break;
        }
        i -= 1;
    }
    let g = i as i32;
    if l as usize > i {
        l = i as i32;
    }
    let m_out = l;

    let mut y: i64 = 1 << j;
    while j < i {
        y -= i64::from(c[j]);
        if y < 0 {
            return (BAD, empty(), m_out);
        }
        j += 1;
        y <<= 1;
    }
    y -= i64::from(c[i]);
    if y < 0 {
        return (BAD, empty(), m_out);
    }
    c[i] += y as u32;

    let mut x = [0u32; BMAX + 1];
    let mut acc = 0u32;
    for len in 1..i {
        acc += c[len];
        x[len + 1] = acc;
    }

    let mut v = [0u32; N_MAX];
    for (value, &len) in b.iter().enumerate() {
        if len != 0 {
            let slot = &mut x[len as usize];
            match v.get_mut(*slot as usize) {
                Some(cell) => *cell = value as u32,
                None => return (BAD, empty(), m_out),
            }
            *slot += 1;
        }
    }
    let nv = x[g as usize] as usize;

    let mut arena: Vec<Huft> = Vec::new();
    let mut start = 0usize;
    let mut first = true;
    let mut code: u32 = 0;
    let mut p = 0usize;
    let mut h: i32 = -1;
    let mut w: i32 = -l;
    let mut u = [0usize; BMAX];
    let mut q = 0usize;
    let mut z: u32 = 0;
    x[0] = 0;
    let mut r = Huft::default();

    while k <= g {
        let mut a = c[k as usize];
        while a != 0 {
            a -= 1;
            while k > w + l {
                h += 1;
                w += l;
                z = (g - w) as u32;
                if z > l as u32 {
                    z = l as u32;
                }
                let mut jj = (k - w) as u32;
                let mut f: u32 = 1 << jj;
                if f > a + 1 {
                    f -= a + 1;
                    let mut xp = k as usize;
                    if jj < z {
                        loop {
                            jj += 1;
                            if jj >= z {
                                break;
                            }
                            f <<= 1;
                            xp += 1;
                            let cx = c.get(xp).copied().unwrap_or(0);
                            if f <= cx {
                                break;
                            }
                            f -= cx;
                        }
                    }
                }
                z = 1 << jj;

                // A table of `z` entries after a link entry, as upstream
                // allocates it.
                arena.push(Huft::default());
                q = arena.len();
                arena.resize(q + z as usize, Huft::default());
                if first {
                    start = q;
                    first = false;
                }
                let Some(slot) = u.get_mut(h as usize) else {
                    return (BAD, Tables { arena, start }, m_out);
                };
                *slot = q;

                if h != 0 {
                    x[h as usize] = code;
                    let link = Huft {
                        b: l as u8,
                        e: (16 + jj) as u8,
                        n: 0,
                        t: q,
                    };
                    let jl = (code >> (w - l)) as usize;
                    match arena.get_mut(u[h as usize - 1] + jl) {
                        Some(entry) => *entry = link,
                        None => return (BAD, Tables { arena, start }, m_out),
                    }
                }
            }

            r.b = (k - w) as u8;
            if p >= nv {
                r.e = 99;
            } else if v[p] < s {
                r.e = if v[p] < 256 { 16 } else { 15 };
                r.n = v[p] as u16;
                p += 1;
            } else {
                let idx = (v[p] - s) as usize;
                let (Some(&ev), Some(&dv)) = (e.get(idx), d.get(idx)) else {
                    return (BAD, Tables { arena, start }, m_out);
                };
                r.e = ev as u8;
                r.n = dv;
                p += 1;
            }

            let f = 1u32 << (k - w);
            let mut jx = code >> w;
            while jx < z {
                match arena.get_mut(q + jx as usize) {
                    Some(entry) => *entry = r,
                    None => return (BAD, Tables { arena, start }, m_out),
                }
                jx += f;
            }

            let mut bit = 1u32 << (k - 1);
            while code & bit != 0 {
                code ^= bit;
                bit >>= 1;
            }
            code ^= bit;

            while h >= 0 && (code & ((1u32 << w) - 1)) != x[h as usize] {
                h -= 1;
                w -= l;
            }
        }
        k += 1;
    }

    let result = i32::from(y != 0 && g != 1);
    (result, Tables { arena, start }, m_out)
}

impl Tables {
    /// The entry at `at`, or `None` outside the tables.
    fn at(&self, at: usize) -> Option<Huft> {
        self.arena.get(at).copied()
    }
}

impl<I: Io> Gz<I> {
    /// `NEXTBYTE` within inflate: the next input byte, with the window's
    /// fill level `w` published first, as `GETBYTE` does, in case the input
    /// ends and the window is flushed before the run aborts.
    fn inflate_byte(&mut self, w: u32) -> Res<u8> {
        if self.inptr < self.insize {
            let byte = self.inbuf[self.inptr as usize];
            self.inptr += 1;
            Ok(byte)
        } else {
            self.outcnt = w;
            Ok(self.fill_inbuf(false)?.unwrap_or(0))
        }
    }

    /// `NEEDBITS (n)` on the locals `b` and `k`.
    fn needbits(&mut self, b: &mut u64, k: &mut u32, n: u32, w: u32) -> Res<()> {
        while *k < n {
            *b |= u64::from(self.inflate_byte(w)?) << *k;
            *k += 8;
        }
        Ok(())
    }

    /// `flush_output (w)`.
    fn flush_output(&mut self, w: u32) -> Res<()> {
        self.outcnt = w;
        self.flush_window()
    }

    /// Walk from a first-level lookup to the entry for the code at the
    /// front of `b`: `e > 16` links are followed, 99 is an invalid code.
    /// Answers the entry, with the link bits already dumped, or `None` for
    /// an invalid code.
    fn huft_lookup(
        &mut self,
        t: &Tables,
        start: usize,
        mask: u32,
        b: &mut u64,
        k: &mut u32,
        w: u32,
    ) -> Res<Option<Huft>> {
        let Some(mut entry) = t.at(start + (*b as u32 & mask) as usize) else {
            return Ok(None);
        };
        while entry.e > 16 {
            if entry.e == 99 {
                return Ok(None);
            }
            *b >>= entry.b;
            *k = k.wrapping_sub(u32::from(entry.b));
            let e = u32::from(entry.e - 16);
            self.needbits(b, k, e, w)?;
            let bits = MASK_BITS.get(e as usize).copied().unwrap_or(0);
            let Some(next) = t.at(entry.t + (*b as u32 & bits) as usize) else {
                return Ok(None);
            };
            entry = next;
        }
        Ok(Some(entry))
    }

    /// `inflate_codes (tl, td, bl, bd)`: decode a block's literals and
    /// matches until its end. 0, or 1 for an invalid code.
    fn inflate_codes(&mut self, tl: &Tables, td: &Tables, bl: i32, bd: i32) -> Res<i32> {
        let mut b = self.inf.bb;
        let mut k = self.inf.bk;
        let mut w = self.outcnt;
        let ml = MASK_BITS[bl as usize];
        let md = MASK_BITS[bd as usize];

        loop {
            self.needbits(&mut b, &mut k, bl as u32, w)?;
            let Some(t) = self.huft_lookup(tl, tl.start, ml, &mut b, &mut k, w)? else {
                return Ok(1);
            };
            b >>= t.b;
            k = k.wrapping_sub(u32::from(t.b));
            let e = u32::from(t.e);
            if e == 16 {
                self.window[w as usize] = t.n as u8;
                w += 1;
                if w as usize == WSIZE {
                    self.flush_output(w)?;
                    w = 0;
                }
                continue;
            }
            if e == 15 {
                break;
            }

            self.needbits(&mut b, &mut k, e, w)?;
            let mut n =
                u32::from(t.n) + (b as u32 & MASK_BITS.get(e as usize).copied().unwrap_or(0));
            b >>= e;
            k = k.wrapping_sub(e);

            self.needbits(&mut b, &mut k, bd as u32, w)?;
            let Some(t) = self.huft_lookup(td, td.start, md, &mut b, &mut k, w)? else {
                return Ok(1);
            };
            b >>= t.b;
            k = k.wrapping_sub(u32::from(t.b));
            let e = u32::from(t.e);
            self.needbits(&mut b, &mut k, e, w)?;
            let mut d = w
                .wrapping_sub(u32::from(t.n))
                .wrapping_sub(b as u32 & MASK_BITS.get(e as usize).copied().unwrap_or(0));
            b >>= e;
            k = k.wrapping_sub(e);

            loop {
                d &= WMASK;
                let far = if d > w { d } else { w };
                let mut e = WSIZE as u32 - far;
                if e > n {
                    e = n;
                }
                n -= e;
                let gap = w.abs_diff(d);
                if e <= gap {
                    self.window
                        .copy_within(d as usize..(d + e) as usize, w as usize);
                    w += e;
                    d += e;
                } else {
                    // Overlapping: byte by byte, so a short distance repeats.
                    for _ in 0..e {
                        self.window[w as usize] = self.window[d as usize];
                        w += 1;
                        d += 1;
                    }
                }
                if w as usize == WSIZE {
                    self.flush_output(w)?;
                    w = 0;
                }
                if n == 0 {
                    break;
                }
            }
        }

        self.outcnt = w;
        self.inf.bb = b;
        self.inf.bk = k;
        Ok(0)
    }

    /// `inflate_stored`: a stored block. 0, or 1 when its length and the
    /// length's complement disagree.
    fn inflate_stored(&mut self) -> Res<i32> {
        let mut b = self.inf.bb;
        let mut k = self.inf.bk;
        let mut w = self.outcnt;

        let n = k & 7;
        b >>= n;
        k -= n;

        self.needbits(&mut b, &mut k, 16, w)?;
        let mut n = b as u32 & 0xffff;
        b >>= 16;
        k -= 16;
        self.needbits(&mut b, &mut k, 16, w)?;
        if n != (!b as u32) & 0xffff {
            return Ok(1);
        }
        b >>= 16;
        k -= 16;

        while n != 0 {
            n -= 1;
            self.needbits(&mut b, &mut k, 8, w)?;
            self.window[w as usize] = b as u8;
            w += 1;
            if w as usize == WSIZE {
                self.flush_output(w)?;
                w = 0;
            }
            b >>= 8;
            k -= 8;
        }

        self.outcnt = w;
        self.inf.bb = b;
        self.inf.bk = k;
        Ok(0)
    }

    /// `inflate_fixed`: a block with the fixed codes.
    fn inflate_fixed(&mut self) -> Res<i32> {
        let mut l = [0u32; 288];
        for (i, len) in l.iter_mut().enumerate() {
            *len = match i {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        let (r, tl, bl) = huft_build(&l, 257, &CPLENS, &CPLEXT, 7);
        if r != 0 {
            return Ok(r);
        }
        let l = [5u32; 30];
        let (r, td, bd) = huft_build(&l, 0, &CPDIST, &CPDEXT, 5);
        if r > 1 {
            return Ok(r);
        }
        if self.inflate_codes(&tl, &td, bl, bd)? != 0 {
            return Ok(1);
        }
        Ok(0)
    }

    /// `inflate_dynamic`: a block that sends its own codes.
    fn inflate_dynamic(&mut self) -> Res<i32> {
        let mut b = self.inf.bb;
        let mut k = self.inf.bk;
        let w = self.outcnt;
        let mut ll = [0u32; 286 + 30];

        self.needbits(&mut b, &mut k, 5, w)?;
        let nl = 257 + (b as u32 & 0x1f);
        b >>= 5;
        k -= 5;
        self.needbits(&mut b, &mut k, 5, w)?;
        let nd = 1 + (b as u32 & 0x1f);
        b >>= 5;
        k -= 5;
        self.needbits(&mut b, &mut k, 4, w)?;
        let nb = 4 + (b as u32 & 0xf);
        b >>= 4;
        k -= 4;
        if nl > 286 || nd > 30 {
            return Ok(1);
        }

        for &slot in BORDER.iter().take(nb as usize) {
            self.needbits(&mut b, &mut k, 3, w)?;
            ll[slot] = b as u32 & 7;
            b >>= 3;
            k -= 3;
        }
        for &slot in BORDER.iter().skip(nb as usize) {
            ll[slot] = 0;
        }

        let (r, tl, bl) = huft_build(&ll[..19], 19, &[], &[], 7);
        if r != 0 {
            return Ok(r);
        }

        let n = (nl + nd) as usize;
        let m = MASK_BITS[bl as usize];
        let mut i = 0usize;
        let mut l = 0u32;
        while i < n {
            self.needbits(&mut b, &mut k, bl as u32, w)?;
            let Some(td) = tl.at(tl.start + (b as u32 & m) as usize) else {
                return Ok(BAD);
            };
            let j = u32::from(td.b);
            b >>= j;
            k = k.wrapping_sub(j);
            if td.e == 99 {
                return Ok(BAD);
            }
            let j = u32::from(td.n);
            if j < 16 {
                l = j;
                ll[i] = l;
                i += 1;
            } else {
                let (bits, base, value) = match j {
                    16 => (2, 3, l),
                    17 => (3, 3, 0),
                    _ => (7, 11, 0),
                };
                self.needbits(&mut b, &mut k, bits, w)?;
                let count = base + (b as u32 & MASK_BITS[bits as usize]) as usize;
                b >>= bits;
                k -= bits;
                if i + count > n {
                    return Ok(1);
                }
                for slot in &mut ll[i..i + count] {
                    *slot = value;
                }
                i += count;
                if j != 16 {
                    l = 0;
                }
            }
        }

        self.inf.bb = b;
        self.inf.bk = k;

        let (r, tl, bl) = huft_build(&ll[..nl as usize], 257, &CPLENS, &CPLEXT, LBITS);
        if r != 0 {
            return Ok(r);
        }
        let (r, td, bd) = huft_build(&ll[nl as usize..n], 0, &CPDIST, &CPDEXT, DBITS);
        if r != 0 {
            return Ok(r);
        }
        Ok(i32::from(self.inflate_codes(&tl, &td, bl, bd)? != 0))
    }

    /// `inflate_block (&e)`: one block, setting `last` if it is the final
    /// one. 0, or the failure's code.
    fn inflate_block(&mut self, last: &mut bool) -> Res<i32> {
        let mut b = self.inf.bb;
        let mut k = self.inf.bk;
        let w = self.outcnt;

        self.needbits(&mut b, &mut k, 1, w)?;
        *last = b & 1 != 0;
        b >>= 1;
        k -= 1;

        self.needbits(&mut b, &mut k, 2, w)?;
        let t = b as u32 & 3;
        b >>= 2;
        k -= 2;

        self.inf.bb = b;
        self.inf.bk = k;

        match t {
            2 => self.inflate_dynamic(),
            0 => self.inflate_stored(),
            1 => self.inflate_fixed(),
            _ => Ok(BAD),
        }
    }

    /// `inflate`: decode a deflate stream. 0, or the failure's code (1 or 2
    /// for data that violates the format).
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`](crate::Stop::Abort) for a read that failed or input
    /// that ended inside the stream, or a write that failed.
    pub fn inflate(&mut self) -> Res<i32> {
        self.outcnt = 0;
        self.inf = Inf::default();

        let mut last = false;
        while !last {
            let r = self.inflate_block(&mut last)?;
            if r != 0 {
                return Ok(r);
            }
        }

        // Undo too much lookahead: the next read starts on a byte.
        while self.inf.bk >= 8 {
            self.inf.bk -= 8;
            self.inptr = self.inptr.wrapping_sub(1);
        }

        self.flush_output(self.outcnt)?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn the_fixed_codes_build_complete_and_incomplete() {
        let mut l = [8u32; 288];
        for (i, len) in l.iter_mut().enumerate() {
            *len = match i {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        let (r, t, bl) = huft_build(&l, 257, &CPLENS, &CPLEXT, 7);
        assert_eq!((r, bl), (0, 7));
        // Code 0000000 (seven zero bits) is the end of block.
        assert_eq!(t.at(t.start).unwrap().e, 15);
        let (r, _, bd) = huft_build(&[5u32; 30], 0, &CPDIST, &CPDEXT, 5);
        assert_eq!((r, bd), (1, 5), "thirty of thirty-two codes: incomplete");
    }

    #[test]
    fn oversubscribed_and_empty_sets() {
        assert_eq!(huft_build(&[1, 1, 1], 3, &[], &[], 7).0, BAD);
        let (r, t, m) = huft_build(&[0, 0, 0], 3, &[], &[], 7);
        assert_eq!((r, m), (0, 1));
        assert_eq!(t.at(t.start).unwrap().e, 99);
        assert_eq!(t.at(t.start + 1).unwrap().e, 99);
    }
}
