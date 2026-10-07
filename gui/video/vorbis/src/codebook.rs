//! Codebooks (Vorbis I §3): a setup header's books unpacked
//! (`vorbis_staticbook_unpack`), readied for decoding -- the codewords
//! sorted for a table-and-bisection decode with no tree, the value vectors
//! unquantised into fixed point -- and the decoders that read entries and
//! vectors out of a packet.
//!
//! Translated into Rust from Tremor's `codebook.c` and `sharedbook.c`,
//! copyright Xiph.Org, used under its BSD licence
//! (`licenses/tremor-COPYING`).
//!
//! Where a hostile book makes Tremor divide by zero (a lattice book of no
//! dimensions) or read through a null pointer (a book with no values used
//! for vectors), this refuses the book or ends the packet instead; where it
//! shifts a value by 32 or more, which C leaves undefined, this does what
//! x86's shift instructions do and takes the count modulo 32.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes bounded by the header's checks (entries times dim under 2^24); sums of decoded values wrap, as C's do"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "every index is within a table this module sized -- entries, used entries, dim x used entries, the first table's 2^n -- and the vector decoders check their n against the slice first"
)]

use crate::bitpack::BitReader;
use crate::misc::{ilog, vfloat_add, vfloat_multi};

/// A book as the setup header gives it (`static_codebook`).
#[derive(Clone, Debug, Default)]
pub(crate) struct StaticBook {
    pub dim: i64,
    pub entries: i64,
    /// Each entry's codeword length, 1 to 32; 0 for an unused entry.
    pub lengths: Vec<u8>,
    /// 0 none, 1 a lattice of `quantvals` values, 2 listed values.
    pub maptype: i64,
    pub q_min: i64,
    pub q_delta: i64,
    pub q_quant: i64,
    pub q_sequencep: i64,
    /// The quantised values, `q_quant` (at most 16) bits each.
    pub quantlist: Vec<u16>,
}

/// A book readied for decoding (`codebook`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Book {
    pub dim: usize,
    pub entries: usize,
    /// The static book's map type: 0 for a book with no values.
    pub maptype: i64,
    pub used_entries: usize,
    /// The values' binary point: each value is `v * 2^binarypoint`.
    pub binarypoint: i32,
    /// `dim` values a used entry, in sorted-codeword order; empty for a
    /// book with no values.
    pub valuelist: Vec<i32>,
    /// The used entries' codewords, MSb first, sorted.
    codelist: Vec<u32>,
    /// Each sorted entry's original number.
    dec_index: Vec<u32>,
    dec_codelengths: Vec<u8>,
    dec_firsttable: Vec<u32>,
    dec_firsttablen: u32,
    dec_maxlength: u32,
}

/// `vorbis_staticbook_unpack`: a book from the setup header; `None` for
/// one that is not, or that runs past the packet's end.
pub(crate) fn unpack(opb: &mut BitReader<'_>) -> Option<StaticBook> {
    if opb.read(24) != 0x0056_4342 {
        return None;
    }
    let mut s = StaticBook {
        dim: opb.read(16),
        entries: opb.read(24),
        ..StaticBook::default()
    };
    if s.entries == -1 {
        return None;
    }
    // `_ilog` takes an unsigned int: -1 (the end) is 32 bits.
    if ilog(s.dim as u32) + ilog(s.entries as u32) > 24 {
        return None;
    }
    let entries = usize::try_from(s.entries).ok()?;
    match opb.read(1) {
        0 => {
            let unused = opb.read(1);
            let bits_each = if unused != 0 { 1 } else { 5 };
            if (s.entries * bits_each + 7) >> 3 > opb.storage_left() {
                return None;
            }
            s.lengths = vec![0; entries];
            if unused != 0 {
                for len in &mut s.lengths {
                    if opb.read(1) != 0 {
                        let num = opb.read(5);
                        if num == -1 {
                            return None;
                        }
                        *len = (num + 1) as u8;
                    }
                }
            } else {
                for len in &mut s.lengths {
                    let num = opb.read(5);
                    if num == -1 {
                        return None;
                    }
                    *len = (num + 1) as u8;
                }
            }
        }
        1 => {
            let mut length = opb.read(5) + 1;
            if length == 0 {
                return None;
            }
            s.lengths = vec![0; entries];
            let mut i = 0usize;
            while i < entries {
                let left = s.entries - i as i64;
                let num = opb.read(ilog(left as u32) as u32);
                if num == -1 {
                    return None;
                }
                if length > 32
                    || num > left
                    || (num > 0 && ((num - 1) >> (length >> 1) >> ((length + 1) >> 1)) > 0)
                {
                    return None;
                }
                for _ in 0..num {
                    s.lengths[i] = length as u8;
                    i += 1;
                }
                length += 1;
            }
        }
        _ => return None,
    }
    s.maptype = opb.read(4);
    match s.maptype {
        0 => {}
        1 | 2 => {
            s.q_min = opb.read(32);
            s.q_delta = opb.read(32);
            s.q_quant = opb.read(4) + 1;
            s.q_sequencep = opb.read(1);
            if s.q_sequencep == -1 {
                return None;
            }
            let quantvals = if s.maptype == 1 {
                if s.dim == 0 {
                    0
                } else {
                    maptype1_quantvals(&s)
                }
            } else {
                s.entries * s.dim
            };
            if (quantvals * s.q_quant + 7) >> 3 > opb.storage_left() {
                return None;
            }
            let quant = s.q_quant as u32;
            let mut list = Vec::with_capacity(usize::try_from(quantvals).ok()?);
            for _ in 0..quantvals {
                // Once one read fails every later one does: C checks only the
                // last, which is the same test.
                let v = opb.read(quant);
                if v == -1 {
                    return None;
                }
                list.push(v as u16);
            }
            s.quantlist = list;
        }
        _ => return None,
    }
    Some(s)
}

/// `_book_maptype1_quantvals`: the lattice's values a dimension -- the
/// greatest `vals` with `vals^dim <= entries`.
pub(crate) fn maptype1_quantvals(b: &StaticBook) -> i64 {
    // Tremor divides by `dim` here: a book of no dimensions has no lattice.
    if b.entries < 1 || b.dim < 1 {
        return 0;
    }
    let bits = i64::from(ilog(b.entries as u32));
    // At least 1: `entries` has `bits` bits and the shift is under `bits`.
    let mut vals = b.entries >> ((bits - 1) * (b.dim - 1) / b.dim);
    loop {
        let mut acc: i64 = 1;
        let mut acc1: i64 = 1;
        let mut i = 0;
        while i < b.dim {
            if b.entries / vals < acc {
                break;
            }
            acc *= vals;
            if i64::MAX / (vals + 1) < acc1 {
                acc1 = i64::MAX;
            } else {
                acc1 *= vals + 1;
            }
            i += 1;
        }
        if i >= b.dim && acc <= b.entries && acc1 > b.entries {
            return vals;
        } else if i < b.dim || acc > b.entries {
            // Never below 1: at 1, `acc` stays 1 and no test fails.
            vals -= 1;
        } else {
            vals += 1;
        }
    }
}

/// `_float32_unpack`: Vorbis's packed float as a mantissa and a power of
/// two.
fn float32_unpack(val: i64) -> (i32, i32) {
    let mut mant = (val & 0x1f_ffff) as i32;
    let sign = val & 0x8000_0000 != 0;
    let mut exp = ((val & 0x7fe0_0000) >> 21) as i32;
    exp -= 20 + 768;
    if mant != 0 {
        while mant & 0x4000_0000 == 0 {
            mant <<= 1;
            exp -= 1;
        }
        if sign {
            mant = -mant;
        }
    } else {
        exp = -9999;
    }
    (mant, exp)
}

/// `_make_words` (sparse): the used entries' canonical codewords, LSb
/// first; `None` for lengths that make an over- or (but for a single
/// entry) an under-populated tree.
fn make_words(lengths: &[u8], sparsecount: usize) -> Option<Vec<u32>> {
    let mut marker = [0u32; 33];
    let mut r: Vec<u32> = Vec::with_capacity(sparsecount);
    for &len in lengths {
        if len == 0 {
            continue;
        }
        let length = usize::from(len);
        let mut entry = marker[length];
        // Claiming a node claims the nodes below it, and blocks those
        // above from being leaves.
        if length < 32 && (entry >> length) != 0 {
            return None;
        }
        r.push(entry);
        for j in (1..=length).rev() {
            if marker[j] & 1 != 0 {
                if j == 1 {
                    marker[1] = marker[1].wrapping_add(1);
                } else {
                    marker[j] = marker[j - 1] << 1;
                }
                break;
            }
            marker[j] = marker[j].wrapping_add(1);
        }
        // The longer markers dangled from the node just taken: dangle them
        // from the new one.
        for j in length + 1..33 {
            if (marker[j] >> 1) == entry {
                entry = marker[j];
                marker[j] = marker[j - 1] << 1;
            } else {
                break;
            }
        }
    }
    // A tree with room left is refused, but for the single-entry one,
    // which has no tree at all.
    if sparsecount != 1 {
        for (i, &m) in marker.iter().enumerate().skip(1) {
            if m & (0xffff_ffff_u32 >> (32 - i)) != 0 {
                return None;
            }
        }
    }
    // Bit-reversed: the packer is LSb first.
    let mut count = 0;
    for &len in lengths {
        if len > 0 {
            let mut temp = 0u32;
            for j in 0..u32::from(len) {
                temp = (temp << 1) | ((r[count] >> j) & 1);
            }
            r[count] = temp;
            count += 1;
        }
    }
    Some(r)
}

/// `_book_unquantize`: the used entries' value vectors at a common binary
/// point, placed by `sortindex`; and that point.
fn unquantize(b: &StaticBook, n: usize, sortindex: &[u32]) -> (Vec<i32>, i32) {
    if b.maptype != 1 && b.maptype != 2 {
        return (Vec::new(), 0);
    }
    let dim = b.dim as usize;
    let (mindel, minpoint) = float32_unpack(b.q_min);
    let (delta, delpoint) = float32_unpack(b.q_delta);
    let mut r = vec![0i32; n * dim];
    let mut rp = vec![0i32; n * dim];
    let mut maxpoint = minpoint;
    let quantvals = if b.maptype == 1 {
        maptype1_quantvals(b)
    } else {
        0
    };
    let mut count = 0usize;
    for (j, &len) in b.lengths.iter().enumerate() {
        if len == 0 {
            continue;
        }
        let mut last = 0i32;
        let mut lastpoint = 0i32;
        let mut indexdiv: i64 = 1;
        for k in 0..dim {
            let q = if b.maptype == 1 {
                // `dim` is at least 1 here, so `quantvals` is too.
                b.quantlist[((j as i64 / indexdiv) % quantvals) as usize]
            } else {
                b.quantlist[j * dim + k]
            };
            // C's `point` starts at 0 for each value, and a zero product
            // leaves it there.
            let mut point = 0;
            let val = vfloat_multi(delta, delpoint, i32::from(q), &mut point);
            let val = vfloat_add(mindel, minpoint, val, point, &mut point);
            let val = vfloat_add(last, lastpoint, val, point, &mut point);
            if b.q_sequencep != 0 {
                last = val;
                lastpoint = point;
            }
            let at = sortindex[count] as usize * dim + k;
            r[at] = val;
            rp[at] = point;
            maxpoint = maxpoint.max(point);
            if b.maptype == 1 {
                indexdiv *= quantvals;
            }
        }
        count += 1;
    }
    for (v, &p) in r.iter_mut().zip(&rp) {
        if p < maxpoint {
            *v = v.wrapping_shr((maxpoint - p) as u32);
        }
    }
    (r, maxpoint)
}

/// `vorbis_book_init_decode`: `s` readied for decoding; `None` for a book
/// whose lengths are no tree.
pub(crate) fn init_decode(s: &StaticBook) -> Option<Book> {
    let n = s.lengths.iter().filter(|&&l| l > 0).count();
    let mut c = Book {
        dim: usize::try_from(s.dim).ok()?,
        entries: usize::try_from(s.entries).ok()?,
        maptype: s.maptype,
        used_entries: n,
        ..Book::default()
    };
    if n == 0 {
        return Some(c);
    }
    // The used entries only, sorted by codeword MSb first so that a
    // codeword can be found by bisection.
    let mut codes = make_words(&s.lengths, n)?;
    for code in &mut codes {
        *code = code.reverse_bits();
    }
    // The codewords of a tree are distinct, so the order is total and a
    // stable sort agrees with C's qsort.
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by_key(|&i| codes[i as usize]);
    let mut sortindex = vec![0u32; n];
    for (i, &position) in order.iter().enumerate() {
        sortindex[position as usize] = i as u32;
    }
    drop(order);
    c.codelist = vec![0; n];
    for (i, &code) in codes.iter().enumerate() {
        c.codelist[sortindex[i] as usize] = code;
    }
    drop(codes);
    (c.valuelist, c.binarypoint) = unquantize(s, n, &sortindex);
    c.dec_index = vec![0; n];
    c.dec_codelengths = vec![0; n];
    let mut m = 0;
    for (i, &len) in s.lengths.iter().enumerate() {
        if len > 0 {
            let at = sortindex[m] as usize;
            c.dec_index[at] = i as u32;
            c.dec_codelengths[at] = len;
            m += 1;
        }
    }
    let tablen = (ilog(n as u32) - 4).clamp(5, 8) as u32;
    c.dec_firsttablen = tablen;
    let tabn = 1usize << tablen;
    c.dec_firsttable = vec![0; tabn];
    for i in 0..n {
        let len = u32::from(c.dec_codelengths[i]);
        c.dec_maxlength = c.dec_maxlength.max(len);
        if len <= tablen {
            let orig = c.codelist[i].reverse_bits() as usize;
            for j in 0..(1usize << (tablen - len)) {
                c.dec_firsttable[orig | (j << len)] = i as u32 + 1;
            }
        }
    }
    // The table's other slots: where in the sorted list to bisect. Only 15
    // bits each, so each is stored as the distance from its end of the
    // list, clamped -- overflowing only widens the search.
    let mask = (0xffff_fffe_u64 << (31 - tablen)) as u32;
    let (mut lo, mut hi) = (0usize, 0usize);
    for i in 0..tabn {
        let word = (i as u32) << (32 - tablen);
        let slot = word.reverse_bits() as usize;
        if c.dec_firsttable[slot] == 0 {
            while lo + 1 < n && c.codelist[lo + 1] <= word {
                lo += 1;
            }
            while hi < n && word >= (c.codelist[hi] & mask) {
                hi += 1;
            }
            let loval = lo.min(0x7fff) as u32;
            let hival = (n - hi).min(0x7fff) as u32;
            c.dec_firsttable[slot] = 0x8000_0000 | (loval << 15) | hival;
        }
    }
    Some(c)
}

impl Book {
    /// `decode_packed_entry_number`: the next codeword's sorted index; -1 at
    /// the packet's end or for a codeword the book has not.
    #[inline(always)]
    fn decode_packed(&self, b: &mut BitReader<'_>) -> i64 {
        let mut read = self.dec_maxlength;
        let (mut lo, mut hi);
        let lok = b.look(self.dec_firsttablen);
        if lok >= 0 {
            let entry = self.dec_firsttable[lok as usize];
            if entry & 0x8000_0000 != 0 {
                lo = i64::from((entry >> 15) & 0x7fff);
                hi = self.used_entries as i64 - i64::from(entry & 0x7fff);
            } else {
                let e = entry as usize - 1;
                b.adv(u32::from(self.dec_codelengths[e]));
                return e as i64;
            }
        } else {
            lo = 0;
            hi = self.used_entries as i64;
        }
        let mut lok = b.look(read);
        while lok < 0 && read > 1 {
            read -= 1;
            lok = b.look(read);
        }
        if lok < 0 {
            // Forces the end of the packet.
            b.adv(1);
            return -1;
        }
        let testword = (lok as u32).reverse_bits();
        while hi - lo > 1 {
            let p = (hi - lo) >> 1;
            let test = i64::from(self.codelist[(lo + p) as usize] > testword);
            lo += p & (test - 1);
            hi -= p & (-test);
        }
        let len = u32::from(self.dec_codelengths[lo as usize]);
        if len <= read {
            b.adv(len);
            return lo;
        }
        b.adv(read + 1);
        -1
    }

    /// `vorbis_book_decode`: the next entry's original number; -1 at the
    /// end.
    pub(crate) fn decode(&self, b: &mut BitReader<'_>) -> i64 {
        if self.used_entries > 0 {
            let packed = self.decode_packed(b);
            if packed >= 0 {
                return i64::from(self.dec_index[packed as usize]);
            }
        }
        -1
    }

    /// The `dim` values of sorted entry `entry`; none for a book without
    /// values, which Tremor would read through a null pointer.
    #[inline]
    fn values(&self, entry: i64) -> Option<&[i32]> {
        let at = entry as usize * self.dim;
        self.valuelist.get(at..at + self.dim)
    }

    /// `vorbis_book_decodevs_add`: `n / dim` entries, every one read before
    /// any is added, each vector's elements `n / dim` apart in `a`. -1 if
    /// any is missing (and then nothing is added).
    pub(crate) fn decodevs_add(
        &self,
        a: &mut [i32],
        b: &mut BitReader<'_>,
        n: usize,
        point: i32,
    ) -> i64 {
        if self.used_entries == 0 {
            return 0;
        }
        if n > a.len() || self.dim == 0 {
            // Tremor divides by `dim`.
            return -1;
        }
        let step = n / self.dim;
        let mut entries = Vec::with_capacity(step);
        for _ in 0..step {
            let e = self.decode_packed(b);
            if e == -1 || self.values(e).is_none() {
                return -1;
            }
            entries.push(e as usize * self.dim);
        }
        let shift = point - self.binarypoint;
        let mut o = 0;
        for i in 0..self.dim {
            let mut j = 0;
            while o + j < n && j < step {
                a[o + j] = a[o + j].wrapping_add(shifted(self.valuelist[entries[j] + i], shift));
                j += 1;
            }
            o += step;
        }
        0
    }

    /// `vorbis_book_decodev_add`: entries' vectors added along `a`, `n`
    /// values in all; -1 at the end (what was added before it stays).
    pub(crate) fn decodev_add(
        &self,
        a: &mut [i32],
        b: &mut BitReader<'_>,
        n: usize,
        point: i32,
    ) -> i64 {
        if self.used_entries == 0 {
            return 0;
        }
        if n > a.len() {
            return -1;
        }
        let shift = point - self.binarypoint;
        let mut i = 0;
        while i < n {
            let entry = self.decode_packed(b);
            if entry == -1 {
                return -1;
            }
            let Some(t) = self.values(entry) else {
                return -1;
            };
            for &v in t {
                if i >= n {
                    break;
                }
                a[i] = a[i].wrapping_add(shifted(v, shift));
                i += 1;
            }
        }
        0
    }

    /// `vorbis_book_decodev_set`: as [`Self::decodev_add`], setting; a book
    /// with no entries sets zeros.
    pub(crate) fn decodev_set(
        &self,
        a: &mut [i32],
        b: &mut BitReader<'_>,
        n: usize,
        point: i32,
    ) -> i64 {
        if n > a.len() {
            return -1;
        }
        if self.used_entries == 0 {
            a[..n].fill(0);
            return 0;
        }
        let shift = point - self.binarypoint;
        let mut i = 0;
        while i < n {
            let entry = self.decode_packed(b);
            if entry == -1 {
                return -1;
            }
            let Some(t) = self.values(entry) else {
                return -1;
            };
            for &v in t {
                if i >= n {
                    break;
                }
                a[i] = shifted(v, shift);
                i += 1;
            }
        }
        0
    }

    /// `vorbis_book_decodevv_add`: vectors dealt across the first `ch`
    /// channels of `a` in turn, from `offset`, `n` values a channel; -1 at
    /// the end.
    pub(crate) fn decodevv_add<T: AsMut<[i32]>>(
        &self,
        a: &mut [T],
        offset: usize,
        ch: usize,
        b: &mut BitReader<'_>,
        n: usize,
        point: i32,
    ) -> i64 {
        if self.used_entries == 0 {
            return 0;
        }
        let m = offset + n;
        if ch == 0 || ch > a.len() || a[..ch].iter_mut().any(|c| c.as_mut().len() < m) {
            return -1;
        }
        let shift = point - self.binarypoint;
        match a {
            // One channel is a vector added in order; two alternate, an
            // entry's values dealt left, right, left. These are almost every
            // stream's, and each channel's slice is cut to the span first.
            [one] if ch == 1 => self.decodev_add(&mut one.as_mut()[offset..m], b, n, point),
            [left, right, ..] if ch == 2 => {
                let (l, r) = (
                    &mut left.as_mut()[offset..m],
                    &mut right.as_mut()[offset..m],
                );
                let mut right_next = false;
                let mut i = 0;
                while i < n {
                    let entry = self.decode_packed(b);
                    if entry == -1 {
                        return -1;
                    }
                    let Some(t) = self.values(entry) else {
                        return -1;
                    };
                    for &v in t {
                        if i >= n {
                            break;
                        }
                        let v = shifted(v, shift);
                        if right_next {
                            r[i] = r[i].wrapping_add(v);
                            i += 1;
                        } else {
                            l[i] = l[i].wrapping_add(v);
                        }
                        right_next = !right_next;
                    }
                }
                0
            }
            _ => {
                let mut chptr = 0;
                let mut i = offset;
                while i < m {
                    let entry = self.decode_packed(b);
                    if entry == -1 {
                        return -1;
                    }
                    let Some(t) = self.values(entry) else {
                        return -1;
                    };
                    for &v in t {
                        if i >= m {
                            break;
                        }
                        let x = &mut a[chptr].as_mut()[i];
                        *x = x.wrapping_add(shifted(v, shift));
                        chptr += 1;
                        if chptr == ch {
                            chptr = 0;
                            i += 1;
                        }
                    }
                }
                0
            }
        }
    }
}

/// `v >> shift`, or `v << -shift` for a negative one; a count of 32 or
/// more (undefined in C) is taken modulo 32, as x86 does.
#[inline]
fn shifted(v: i32, shift: i32) -> i32 {
    if shift >= 0 {
        v.wrapping_shr(shift as u32)
    } else {
        v.wrapping_shl(shift.unsigned_abs())
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
    fn lengths_make_the_canonical_codewords() {
        // Lengths 2, 2, 2, 3, 3: 00 01 10 110 111 (MSb first), bit-reversed.
        let words = make_words(&[2, 2, 2, 3, 3], 5).unwrap();
        assert_eq!(words, vec![0b00, 0b10, 0b01, 0b011, 0b111]);
        // Over- and under-populated trees are refused.
        assert!(make_words(&[1, 1, 1], 3).is_none());
        assert!(make_words(&[1, 2], 2).is_none());
        // A single entry is the one-codeword pseudo-tree.
        assert!(make_words(&[1], 1).is_some());
        // Unused entries take no codeword.
        assert_eq!(make_words(&[1, 0, 1], 2).unwrap(), vec![0, 1]);
    }

    #[test]
    fn a_lattice_has_the_largest_side_that_fits() {
        let book = |entries, dim| StaticBook {
            dim,
            entries,
            ..StaticBook::default()
        };
        assert_eq!(maptype1_quantvals(&book(81, 4)), 3);
        assert_eq!(maptype1_quantvals(&book(80, 4)), 2);
        assert_eq!(maptype1_quantvals(&book(625, 4)), 5);
        assert_eq!(maptype1_quantvals(&book(1, 8)), 1);
        assert_eq!(maptype1_quantvals(&book(0, 4)), 0);
        assert_eq!(maptype1_quantvals(&book(5, 0)), 0, "Tremor divides by zero");
    }

    #[test]
    fn packed_floats_unpack() {
        // 1.0: mantissa 1, exponent 788 (the bias plus 20).
        assert_eq!(float32_unpack(788 << 21 | 1), (1 << 30, -30));
        // -0.5.
        assert_eq!(
            float32_unpack(0x8000_0000 | 787 << 21 | 1),
            (-(1 << 30), -31)
        );
        assert_eq!(float32_unpack(0), (0, -9999));
    }

    /// Bits written as a setup header writes them, LSb first.
    struct Writer {
        bytes: Vec<u8>,
        bit: usize,
    }

    impl Writer {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                bit: 0,
            }
        }

        fn put(&mut self, value: u64, bits: u32) {
            for k in 0..bits {
                if self.bit.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if (value >> k) & 1 != 0 {
                    *self.bytes.last_mut().unwrap() |= 1 << (self.bit % 8);
                }
                self.bit += 1;
            }
        }
    }

    /// A two-dimensional lattice book over {-1, 0, 1}: nine entries, the
    /// lengths unordered with none unused.
    fn lattice_book(lengths: &[u8]) -> Vec<u8> {
        let mut w = Writer::new();
        w.put(0x0056_4342, 24);
        w.put(2, 16);
        w.put(lengths.len() as u64, 24);
        w.put(0, 1); // unordered
        w.put(0, 1); // none unused
        for &l in lengths {
            w.put(u64::from(l) - 1, 5);
        }
        w.put(1, 4); // a lattice
        w.put(0x8000_0000 | 788 << 21 | 1, 32); // minimum -1.0
        w.put(788 << 21 | 1, 32); // delta 1.0
        w.put(1, 4); // two bits a value
        w.put(0, 1); // not a sequence
        for q in 0..3 {
            w.put(q, 2);
        }
        w.bytes
    }

    #[test]
    fn a_lattice_book_decodes_its_vectors() {
        // Nine lengths making a full tree: 7 / 8 + 2 / 16 = 1.
        let lengths = [3, 3, 3, 3, 3, 3, 3, 4, 4];
        let bytes = lattice_book(&lengths);
        let s = unpack(&mut BitReader::new(&bytes)).unwrap();
        assert_eq!((s.dim, s.entries, s.maptype), (2, 9, 1));
        assert_eq!(s.quantlist, vec![0, 1, 2]);
        let book = init_decode(&s).unwrap();
        assert_eq!(book.used_entries, 9);
        // The codewords, MSb first: 000, 001 ... 110, 1110, 1111.
        let code = |e: usize| -> (u64, u32) {
            if e < 7 {
                (e as u64, 3)
            } else {
                (0b1110 + (e as u64 - 7), 4)
            }
        };
        let order = [4usize, 0, 8, 7, 2];
        let mut w = Writer::new();
        for &e in &order {
            let (c, len) = code(e);
            // The packer is LSb first; a codeword goes in MSb first.
            let mut rev = 0;
            for k in 0..len {
                rev |= ((c >> (len - 1 - k)) & 1) << k;
            }
            w.put(rev, len);
        }
        // 17 bits, padded to 24 with zeros: entry 0 twice more, by the
        // first table and then by bisection on a shorter look, then the end
        // -- which stays the end.
        let mut r = BitReader::new(&w.bytes);
        for &e in order.iter().chain(&[0, 0]) {
            assert_eq!(book.decode(&mut r), e as i64);
        }
        assert_eq!(book.decode(&mut r), -1);
        assert_eq!(book.decode(&mut r), -1);
        // Each entry e is the vector (e % 3 - 1, e / 3 - 1), here at a
        // binary point of -8.
        let mut r = BitReader::new(&w.bytes);
        let mut a = [0i32; 10];
        assert_eq!(book.decodev_set(&mut a, &mut r, 10, -8), 0);
        let want: Vec<i32> = order
            .iter()
            .flat_map(|&e| [(e as i32 % 3 - 1) << 8, (e as i32 / 3 - 1) << 8])
            .collect();
        assert_eq!(a.to_vec(), want);
        // Added, and interleaved: the same vectors, element by element.
        let mut r = BitReader::new(&w.bytes);
        let mut b = [1i32; 10];
        assert_eq!(book.decodevs_add(&mut b, &mut r, 10, -8), 0);
        let mut want_s = [1i32; 10];
        for (j, &e) in order.iter().enumerate() {
            want_s[j] += (e as i32 % 3 - 1) << 8;
            want_s[5 + j] += (e as i32 / 3 - 1) << 8;
        }
        assert_eq!(b, want_s);
        // Dealt across two channels in turn.
        let mut r = BitReader::new(&w.bytes);
        let mut chans = vec![vec![0i32; 5], vec![0i32; 5]];
        assert_eq!(book.decodevv_add(&mut chans, 0, 2, &mut r, 5, -8), 0);
        assert_eq!(
            chans[0],
            want.iter().step_by(2).copied().collect::<Vec<_>>()
        );
        assert_eq!(
            chans[1],
            want.iter().skip(1).step_by(2).copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn hostile_books_are_refused() {
        // Not a book.
        assert!(unpack(&mut BitReader::new(&[0x42, 0x43, 0x55, 0, 0, 0])).is_none());
        // An over-populated tree: unpacks, then is refused.
        let bytes = lattice_book(&[1, 1, 1, 3, 3, 3, 3, 4, 4]);
        let s = unpack(&mut BitReader::new(&bytes)).unwrap();
        assert!(init_decode(&s).is_none());
        // Cut short anywhere: refused.
        let bytes = lattice_book(&[3, 3, 3, 3, 3, 3, 3, 4, 4]);
        for cut in 0..bytes.len() {
            assert!(
                unpack(&mut BitReader::new(&bytes[..cut])).is_none(),
                "cut at {cut}"
            );
        }
        // A vector asked of a book with no values ends the packet, where
        // Tremor reads through a null pointer.
        let mut s = unpack(&mut BitReader::new(&bytes)).unwrap();
        s.maptype = 0;
        let book = init_decode(&s).unwrap();
        let mut a = [0i32; 4];
        assert_eq!(
            book.decodev_add(&mut a, &mut BitReader::new(&[0xff; 4]), 4, 0),
            -1
        );
    }
}
