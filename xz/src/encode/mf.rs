//! The match finders: liblzma's `lz_encoder.c` and `lz_encoder_mf.c`, over
//! the whole input at once.
//!
//! liblzma slides a window over input that arrives a buffer at a time, but
//! it only runs the match finder on a position while `keep_size_after` bytes
//! -- more than any lookahead it takes -- lie beyond it, until the input
//! ends. So every position is seen with the same lookahead a whole-input
//! finder gives it, and the window's moves are invisible: positions keep the
//! offset (`cyclic_size`) liblzma starts them at, and hash values are its
//! own. What is kept exactly is everything the matches depend on: the hash
//! functions and table sizes, the chain and tree updates, the search depth,
//! and the normalisation of stored positions before they would overflow.
//!
//! The one liberty is memory: the son array (the chains or trees) is cut to
//! the input when the input is shorter than the dictionary -- no position
//! reaches past it -- since liblzma's full-size array would be committed
//! memory here for nothing.

// Positions and lengths are bounded by the input and by `nice_len`, and
// `depth` counts down from a value it is tested against before each step;
// the 32-bit position arithmetic that wraps in liblzma wraps explicitly here.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec;
use alloc::vec::Vec;

/// `MATCH_LEN_MAX`: the longest match LZMA codes.
pub(crate) const MATCH_LEN_MAX: u32 = 273;

/// `HASH_2_SIZE`, `HASH_3_SIZE` and their masks; the 2- and 3-byte hash
/// tables sit before the main one (`FIX_3_HASH_SIZE`, `FIX_4_HASH_SIZE`).
const HASH_2_SIZE: u32 = 1 << 10;
const HASH_3_SIZE: u32 = 1 << 16;
const HASH_2_MASK: u32 = HASH_2_SIZE - 1;
const HASH_3_MASK: u32 = HASH_3_SIZE - 1;
const FIX_3_HASH_SIZE: u32 = HASH_2_SIZE;
const FIX_4_HASH_SIZE: u32 = HASH_2_SIZE + HASH_3_SIZE;

/// `EMPTY_HASH_VALUE`: positions start at `cyclic_size`, so 0 is never a
/// match candidate in range.
const EMPTY: u32 = 0;

/// The table the hashes are built from: CRC-32's (`lzma_crc32_table[0]`).
// `i` runs over the table's 256 entries.
#[allow(clippy::indexing_slicing)]
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xedb8_8320
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// The table's entry for a byte.
fn crc(b: u8) -> u32 {
    // A byte indexes the 256-entry table.
    #[allow(clippy::indexing_slicing)]
    let entry = CRC_TABLE[usize::from(b)];
    entry
}

/// A match finder (`lzma_match_finder`): hash chains or binary trees, keyed
/// by the first two, three or four bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchFinder {
    /// Hash chain over three bytes (preset 0).
    Hc3,
    /// Hash chain over four bytes (presets 1 to 3).
    Hc4,
    /// Binary tree over two bytes.
    Bt2,
    /// Binary tree over three bytes.
    Bt3,
    /// Binary tree over four bytes (presets 4 to 9).
    Bt4,
}

impl MatchFinder {
    /// How many bytes it hashes: the shortest `nice_len` it allows.
    pub(crate) const fn hash_bytes(self) -> u32 {
        match self {
            Self::Bt2 => 2,
            Self::Hc3 | Self::Bt3 => 3,
            Self::Hc4 | Self::Bt4 => 4,
        }
    }

    const fn is_bt(self) -> bool {
        matches!(self, Self::Bt2 | Self::Bt3 | Self::Bt4)
    }
}

/// A match the finder found (`lzma_match`): `dist` is the distance less one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Match {
    pub(crate) len: u32,
    pub(crate) dist: u32,
}

/// `lzma_memcmplen`: how far, from `len` up to `limit`, the bytes at `a` and
/// `b` agree.
pub(crate) fn memcmplen(buf: &[u8], a: usize, b: usize, len: u32, limit: u32) -> u32 {
    let end = limit as usize;
    let (Some(x), Some(y)) = (
        buf.get(a..a.saturating_add(end)),
        buf.get(b..b.saturating_add(end)),
    ) else {
        // Callers keep `a + limit` and `b + limit` within the input.
        return len;
    };
    let mut n = len as usize;
    while let (Some(p), Some(q)) = (x.get(n..n.wrapping_add(8)), y.get(n..n.wrapping_add(8))) {
        let diff = word(p) ^ word(q);
        if diff != 0 {
            let at = n.wrapping_add((diff.trailing_zeros() / 8) as usize);
            return at.min(end) as u32;
        }
        n = n.wrapping_add(8);
    }
    while let (Some(p), Some(q)) = (x.get(n), y.get(n)) {
        if p != q {
            break;
        }
        n = n.wrapping_add(1);
    }
    n as u32
}

/// Eight bytes as a little-endian word, for [`memcmplen`].
fn word(bytes: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    if let Some(src) = bytes.get(..8) {
        w.copy_from_slice(src);
    }
    u64::from_le_bytes(w)
}

/// `lzma_mf`, over the whole input.
pub(crate) struct Mf<'a> {
    buf: &'a [u8],
    /// The next byte to run through the match finder.
    pub(crate) read_pos: usize,
    /// Bytes run through the finder but not yet encoded.
    pub(crate) read_ahead: u32,
    /// `read_pos + offset`: the current position as the tables store it.
    pos: u32,
    hash: Vec<u32>,
    son: Vec<u32>,
    cyclic_pos: u32,
    cyclic_size: u32,
    hash_mask: u32,
    depth: u32,
    pub(crate) nice_len: u32,
    kind: MatchFinder,
}

impl<'a> Mf<'a> {
    /// `lz_encoder_prepare` and `lz_encoder_init`. The options are valid
    /// (`LzmaOptions::validate`): `dict_size` from 4 KiB to 1.5 GiB, and
    /// `nice_len` from the finder's hash length to [`MATCH_LEN_MAX`].
    pub(crate) fn new(
        buf: &'a [u8],
        dict_size: u32,
        kind: MatchFinder,
        nice_len: u32,
        depth: u32,
    ) -> Self {
        let cyclic_size = dict_size.saturating_add(1);
        let hash_bytes = kind.hash_bytes();
        let hash_mask = if hash_bytes == 2 {
            0xffff
        } else {
            // Round the dictionary size up to 2^n - 1, halved, at least
            // 0xFFFF -- exactly liblzma's steps, which leave out `>> 16`.
            let mut hs = dict_size.wrapping_sub(1);
            hs |= hs >> 1;
            hs |= hs >> 2;
            hs |= hs >> 4;
            hs |= hs >> 8;
            hs >>= 1;
            hs |= 0xffff;
            if hs > 1 << 24 {
                if hash_bytes == 3 {
                    hs = (1 << 24) - 1;
                } else {
                    hs >>= 1;
                }
            }
            hs
        };
        let mut hash_count = hash_mask as usize + 1;
        if hash_bytes > 2 {
            hash_count = hash_count.saturating_add(HASH_2_SIZE as usize);
        }
        if hash_bytes > 3 {
            hash_count = hash_count.saturating_add(HASH_3_SIZE as usize);
        }
        // No position is stored beyond the input's length, so a son array
        // longer than the input would never be read.
        let cells = (cyclic_size as usize).min(buf.len().max(1));
        let sons = if kind.is_bt() {
            cells.saturating_mul(2)
        } else {
            cells
        };
        let depth = if depth != 0 {
            depth
        } else if kind.is_bt() {
            16 + nice_len / 2
        } else {
            4 + nice_len / 4
        };
        Self {
            buf,
            read_pos: 0,
            read_ahead: 0,
            pos: cyclic_size,
            hash: vec![EMPTY; hash_count],
            son: vec![EMPTY; sons],
            cyclic_pos: 0,
            cyclic_size,
            hash_mask,
            depth,
            nice_len,
            kind,
        }
    }

    /// The whole input.
    pub(crate) const fn data(&self) -> &'a [u8] {
        self.buf
    }

    /// The input byte at `i` (0 past the end, which no caller reads).
    pub(crate) fn byte(&self, i: usize) -> u8 {
        self.buf.get(i).copied().unwrap_or(0)
    }

    /// `mf_avail`: bytes not yet run through the finder.
    pub(crate) fn avail(&self) -> u32 {
        u32::try_from(self.buf.len().saturating_sub(self.read_pos)).unwrap_or(u32::MAX)
    }

    /// `mf_position`: the first byte not yet encoded.
    pub(crate) fn position(&self) -> usize {
        self.read_pos.saturating_sub(self.read_ahead as usize)
    }

    /// `mf_unencoded`: bytes not yet encoded.
    pub(crate) fn unencoded(&self) -> usize {
        self.buf
            .len()
            .saturating_sub(self.read_pos)
            .saturating_add(self.read_ahead as usize)
    }

    /// Whether every byte has been run through the finder (`read_pos` at
    /// `read_limit`, which at the end of the input is `write_pos`).
    pub(crate) fn at_end(&self) -> bool {
        self.read_pos >= self.buf.len()
    }

    /// `lzma_mf_find`: the matches at the next position into `matches`,
    /// shortest first; returns the longest match's length -- extended past
    /// `nice_len` when the finder stopped there -- and how many there are.
    pub(crate) fn find(&mut self, matches: &mut [Match]) -> (u32, u32) {
        let count = match self.kind {
            MatchFinder::Hc3 => self.hc3_find(matches),
            MatchFinder::Hc4 => self.hc4_find(matches),
            MatchFinder::Bt2 => self.bt2_find(matches),
            MatchFinder::Bt3 => self.bt3_find(matches),
            MatchFinder::Bt4 => self.bt4_find(matches),
        };
        let mut len_best = 0;
        if let Some(last) = count.checked_sub(1).and_then(|i| matches.get(i)) {
            len_best = last.len;
            if len_best == self.nice_len {
                let limit = self.avail().saturating_add(1).min(MATCH_LEN_MAX);
                // The byte just run through the finder, and the match's start.
                let p1 = self.read_pos.wrapping_sub(1);
                let p2 = p1.wrapping_sub(last.dist as usize).wrapping_sub(1);
                len_best = memcmplen(self.buf, p1, p2, len_best, limit);
            }
        }
        self.read_ahead = self.read_ahead.wrapping_add(1);
        (len_best, count as u32)
    }

    /// `mf_skip`: runs `amount` positions through the finder without
    /// collecting their matches.
    pub(crate) fn skip(&mut self, amount: u32) {
        if amount == 0 {
            return;
        }
        match self.kind {
            MatchFinder::Hc3 => self.hc3_skip(amount),
            MatchFinder::Hc4 => self.hc4_skip(amount),
            MatchFinder::Bt2 | MatchFinder::Bt3 | MatchFinder::Bt4 => self.bt_skip_n(amount),
        }
        self.read_ahead = self.read_ahead.wrapping_add(amount);
    }

    // --- positions --------------------------------------------------------

    /// `move_pos`
    fn move_pos(&mut self) {
        self.cyclic_pos = self.cyclic_pos.wrapping_add(1);
        if self.cyclic_pos == self.cyclic_size {
            self.cyclic_pos = 0;
        }
        self.read_pos = self.read_pos.wrapping_add(1);
        self.pos = self.pos.wrapping_add(1);
        if self.pos == u32::MAX {
            self.normalize();
        }
    }

    /// `move_pending`: a position too near the end of the input to hash.
    fn move_pending(&mut self) {
        self.read_pos = self.read_pos.wrapping_add(1);
        self.pos = self.pos.wrapping_add(1);
    }

    /// `normalize`: before the stored positions would overflow, take the
    /// same amount from every one, emptying those out of the window.
    fn normalize(&mut self) {
        let subvalue = u32::MAX - self.cyclic_size;
        for v in self.hash.iter_mut().chain(self.son.iter_mut()) {
            *v = if *v <= subvalue {
                EMPTY
            } else {
                v.wrapping_sub(subvalue)
            };
        }
        self.pos = self.pos.wrapping_sub(subvalue);
    }

    /// The `header` macro: the length limit at this position, or `None`
    /// (the position passed over) when fewer than `len_min` bytes are left.
    fn header(&mut self, len_min: u32) -> Option<u32> {
        let avail = self.avail();
        if self.nice_len <= avail {
            Some(self.nice_len)
        } else if avail < len_min {
            self.move_pending();
            None
        } else {
            Some(avail)
        }
    }

    // --- tables -----------------------------------------------------------

    fn hash_at(&self, i: u32) -> u32 {
        self.hash.get(i as usize).copied().unwrap_or(EMPTY)
    }

    fn set_hash(&mut self, i: u32, v: u32) {
        if let Some(slot) = self.hash.get_mut(i as usize) {
            *slot = v;
        }
    }

    fn son_at(&self, i: usize) -> u32 {
        self.son.get(i).copied().unwrap_or(EMPTY)
    }

    fn set_son(&mut self, i: usize, v: u32) {
        if let Some(slot) = self.son.get_mut(i) {
            *slot = v;
        }
    }

    /// The son slot `delta` positions back from `cyclic_pos`, around the
    /// cycle.
    fn back(&self, delta: u32) -> usize {
        let cp = self.cyclic_pos;
        (if delta > cp {
            cp.wrapping_add(self.cyclic_size).wrapping_sub(delta)
        } else {
            cp.wrapping_sub(delta)
        }) as usize
    }

    /// The hashes of `hash_3_calc` and `hash_4_calc` at the current byte:
    /// (2-byte, 3-byte, full), the full one under `hash_mask` for `bytes`.
    fn hashes(&self, bytes: u32) -> (u32, u32, u32) {
        let cur = self.read_pos;
        let b0 = self.byte(cur);
        let b1 = u32::from(self.byte(cur.wrapping_add(1)));
        let b2 = u32::from(self.byte(cur.wrapping_add(2)));
        let temp = crc(b0) ^ b1;
        let h2 = temp & HASH_2_MASK;
        let t3 = temp ^ (b2 << 8);
        if bytes == 3 {
            (h2, t3 & HASH_3_MASK, t3 & self.hash_mask)
        } else {
            let b3 = self.byte(cur.wrapping_add(3));
            (h2, t3 & HASH_3_MASK, (t3 ^ (crc(b3) << 5)) & self.hash_mask)
        }
    }

    /// Records a match.
    fn push(matches: &mut [Match], count: &mut usize, len: u32, dist: u32) {
        if let Some(m) = matches.get_mut(*count) {
            *m = Match { len, dist };
            *count = count.wrapping_add(1);
        }
    }

    // --- hash chains ------------------------------------------------------

    /// `hc_find_func`
    fn hc_find_func(
        &mut self,
        len_limit: u32,
        mut cur_match: u32,
        matches: &mut [Match],
        mut count: usize,
        mut len_best: u32,
    ) -> usize {
        let cur = self.read_pos;
        self.set_son(self.cyclic_pos as usize, cur_match);
        let mut depth = self.depth;
        loop {
            let delta = self.pos.wrapping_sub(cur_match);
            if depth == 0 || delta >= self.cyclic_size {
                return count;
            }
            depth -= 1;
            // `delta` is below `cyclic_size` and below the bytes already
            // run through, so `pb` is an earlier byte.
            let pb = cur.wrapping_sub(delta as usize);
            cur_match = self.son_at(self.back(delta));
            let lb = len_best as usize;
            if self.byte(pb.wrapping_add(lb)) == self.byte(cur.wrapping_add(lb))
                && self.byte(pb) == self.byte(cur)
            {
                let len = memcmplen(self.buf, pb, cur, 1, len_limit);
                if len_best < len {
                    len_best = len;
                    Self::push(matches, &mut count, len, delta.wrapping_sub(1));
                    if len == len_limit {
                        return count;
                    }
                }
            }
        }
    }

    /// `hc_skip`
    fn hc_skip_one(&mut self, cur_match: u32) {
        self.set_son(self.cyclic_pos as usize, cur_match);
        self.move_pos();
    }

    /// `lzma_mf_hc3_find`
    fn hc3_find(&mut self, matches: &mut [Match]) -> usize {
        let Some(len_limit) = self.header(3) else {
            return 0;
        };
        let cur = self.read_pos;
        let pos = self.pos;
        let (h2, _, hv) = self.hashes(3);
        let delta2 = pos.wrapping_sub(self.hash_at(h2));
        let cur_match = self.hash_at(FIX_3_HASH_SIZE.wrapping_add(hv));
        self.set_hash(h2, pos);
        self.set_hash(FIX_3_HASH_SIZE.wrapping_add(hv), pos);

        let mut len_best = 2;
        let mut count = 0;
        if delta2 < self.cyclic_size
            && self.byte(cur.wrapping_sub(delta2 as usize)) == self.byte(cur)
        {
            len_best = memcmplen(
                self.buf,
                cur.wrapping_sub(delta2 as usize),
                cur,
                len_best,
                len_limit,
            );
            Self::push(matches, &mut count, len_best, delta2.wrapping_sub(1));
            if len_best == len_limit {
                self.hc_skip_one(cur_match);
                return 1;
            }
        }
        let count = self.hc_find_func(len_limit, cur_match, matches, count, len_best);
        self.move_pos();
        count
    }

    /// `lzma_mf_hc3_skip`
    fn hc3_skip(&mut self, amount: u32) {
        for _ in 0..amount {
            if self.avail() < 3 {
                self.move_pending();
                continue;
            }
            let pos = self.pos;
            let (h2, _, hv) = self.hashes(3);
            let cur_match = self.hash_at(FIX_3_HASH_SIZE.wrapping_add(hv));
            self.set_hash(h2, pos);
            self.set_hash(FIX_3_HASH_SIZE.wrapping_add(hv), pos);
            self.hc_skip_one(cur_match);
        }
    }

    /// The two- and three-byte candidates `hc4_find` and `bt4_find` share:
    /// updates the tables and records the candidates; returns the chain or
    /// tree head, the length found so far and how many matches.
    fn four_byte_head(&mut self, len_limit: u32, matches: &mut [Match]) -> (u32, u32, usize) {
        let cur = self.read_pos;
        let pos = self.pos;
        let (h2, h3, hv) = self.hashes(4);
        let mut delta2 = pos.wrapping_sub(self.hash_at(h2));
        let delta3 = pos.wrapping_sub(self.hash_at(FIX_3_HASH_SIZE.wrapping_add(h3)));
        let cur_match = self.hash_at(FIX_4_HASH_SIZE.wrapping_add(hv));
        self.set_hash(h2, pos);
        self.set_hash(FIX_3_HASH_SIZE.wrapping_add(h3), pos);
        self.set_hash(FIX_4_HASH_SIZE.wrapping_add(hv), pos);

        let mut len_best = 1;
        let mut count = 0usize;
        if delta2 < self.cyclic_size
            && self.byte(cur.wrapping_sub(delta2 as usize)) == self.byte(cur)
        {
            len_best = 2;
            Self::push(matches, &mut count, 2, delta2.wrapping_sub(1));
        }
        if delta2 != delta3
            && delta3 < self.cyclic_size
            && self.byte(cur.wrapping_sub(delta3 as usize)) == self.byte(cur)
        {
            len_best = 3;
            // `matches[matches_count++].dist = delta3 - 1`: the length is
            // filled in below.
            Self::push(matches, &mut count, 0, delta3.wrapping_sub(1));
            delta2 = delta3;
        }
        if let Some(last) = count.checked_sub(1).and_then(|i| matches.get_mut(i)) {
            len_best = memcmplen(
                self.buf,
                cur,
                cur.wrapping_sub(delta2 as usize),
                len_best,
                len_limit,
            );
            last.len = len_best;
        }
        (cur_match, len_best, count)
    }

    /// `lzma_mf_hc4_find`
    fn hc4_find(&mut self, matches: &mut [Match]) -> usize {
        let Some(len_limit) = self.header(4) else {
            return 0;
        };
        let (cur_match, len_best, count) = self.four_byte_head(len_limit, matches);
        if count != 0 && len_best == len_limit {
            self.hc_skip_one(cur_match);
            return count;
        }
        let count = self.hc_find_func(len_limit, cur_match, matches, count, len_best.max(3));
        self.move_pos();
        count
    }

    /// `lzma_mf_hc4_skip`
    fn hc4_skip(&mut self, amount: u32) {
        for _ in 0..amount {
            if self.avail() < 4 {
                self.move_pending();
                continue;
            }
            let pos = self.pos;
            let (h2, h3, hv) = self.hashes(4);
            let cur_match = self.hash_at(FIX_4_HASH_SIZE.wrapping_add(hv));
            self.set_hash(h2, pos);
            self.set_hash(FIX_3_HASH_SIZE.wrapping_add(h3), pos);
            self.set_hash(FIX_4_HASH_SIZE.wrapping_add(hv), pos);
            self.hc_skip_one(cur_match);
        }
    }

    // --- binary trees -----------------------------------------------------

    /// `bt_find_func`, or `bt_skip_func` when `matches` is `None`.
    fn bt_func(
        &mut self,
        len_limit: u32,
        mut cur_match: u32,
        mut matches: Option<&mut [Match]>,
        mut count: usize,
        mut len_best: u32,
    ) -> usize {
        let cur = self.read_pos;
        let cp = self.cyclic_pos as usize;
        let mut ptr0 = cp.wrapping_mul(2).wrapping_add(1);
        let mut ptr1 = cp.wrapping_mul(2);
        let mut len0 = 0u32;
        let mut len1 = 0u32;
        let mut depth = self.depth;
        loop {
            let delta = self.pos.wrapping_sub(cur_match);
            if depth == 0 || delta >= self.cyclic_size {
                self.set_son(ptr0, EMPTY);
                self.set_son(ptr1, EMPTY);
                return count;
            }
            depth -= 1;
            let pair = self.back(delta).wrapping_mul(2);
            let pb = cur.wrapping_sub(delta as usize);
            let mut len = len0.min(len1);
            if self.byte(pb.wrapping_add(len as usize)) == self.byte(cur.wrapping_add(len as usize))
            {
                len = memcmplen(self.buf, pb, cur, len.wrapping_add(1), len_limit);
                // `bt_skip_func` collects nothing, but stops the same way.
                let found = match matches.as_deref_mut() {
                    Some(m) => {
                        if len_best < len {
                            len_best = len;
                            Self::push(m, &mut count, len, delta.wrapping_sub(1));
                            len == len_limit
                        } else {
                            false
                        }
                    }
                    None => len == len_limit,
                };
                if found {
                    self.set_son(ptr1, self.son_at(pair));
                    self.set_son(ptr0, self.son_at(pair.wrapping_add(1)));
                    return count;
                }
            }
            if self.byte(pb.wrapping_add(len as usize)) < self.byte(cur.wrapping_add(len as usize))
            {
                self.set_son(ptr1, cur_match);
                ptr1 = pair.wrapping_add(1);
                cur_match = self.son_at(ptr1);
                len1 = len;
            } else {
                self.set_son(ptr0, cur_match);
                ptr0 = pair;
                cur_match = self.son_at(ptr0);
                len0 = len;
            }
        }
    }

    /// `bt_skip`: the tree updated for the current position, which is then
    /// passed.
    fn bt_skip_one(&mut self, len_limit: u32, cur_match: u32) {
        self.bt_func(len_limit, cur_match, None, 0, 0);
        self.move_pos();
    }

    /// `lzma_mf_bt2_find`
    fn bt2_find(&mut self, matches: &mut [Match]) -> usize {
        let Some(len_limit) = self.header(2) else {
            return 0;
        };
        let pos = self.pos;
        let hv = self.hash2();
        let cur_match = self.hash_at(hv);
        self.set_hash(hv, pos);
        let count = self.bt_func(len_limit, cur_match, Some(matches), 0, 1);
        self.move_pos();
        count
    }

    /// `hash_2_calc`: the first two bytes.
    fn hash2(&self) -> u32 {
        u32::from(self.byte(self.read_pos))
            | (u32::from(self.byte(self.read_pos.wrapping_add(1))) << 8)
    }

    /// `lzma_mf_bt3_find`
    fn bt3_find(&mut self, matches: &mut [Match]) -> usize {
        let Some(len_limit) = self.header(3) else {
            return 0;
        };
        let cur = self.read_pos;
        let pos = self.pos;
        let (h2, _, hv) = self.hashes(3);
        let delta2 = pos.wrapping_sub(self.hash_at(h2));
        let cur_match = self.hash_at(FIX_3_HASH_SIZE.wrapping_add(hv));
        self.set_hash(h2, pos);
        self.set_hash(FIX_3_HASH_SIZE.wrapping_add(hv), pos);

        let mut len_best = 2;
        let mut count = 0;
        if delta2 < self.cyclic_size
            && self.byte(cur.wrapping_sub(delta2 as usize)) == self.byte(cur)
        {
            len_best = memcmplen(
                self.buf,
                cur,
                cur.wrapping_sub(delta2 as usize),
                len_best,
                len_limit,
            );
            Self::push(matches, &mut count, len_best, delta2.wrapping_sub(1));
            if len_best == len_limit {
                self.bt_skip_one(len_limit, cur_match);
                return 1;
            }
        }
        let count = self.bt_func(len_limit, cur_match, Some(matches), count, len_best);
        self.move_pos();
        count
    }

    /// `lzma_mf_bt4_find`
    fn bt4_find(&mut self, matches: &mut [Match]) -> usize {
        let Some(len_limit) = self.header(4) else {
            return 0;
        };
        let (cur_match, len_best, count) = self.four_byte_head(len_limit, matches);
        if count != 0 && len_best == len_limit {
            self.bt_skip_one(len_limit, cur_match);
            return count;
        }
        let count = self.bt_func(len_limit, cur_match, Some(matches), count, len_best.max(3));
        self.move_pos();
        count
    }

    /// `lzma_mf_bt2_skip`, `lzma_mf_bt3_skip` and `lzma_mf_bt4_skip`.
    fn bt_skip_n(&mut self, amount: u32) {
        let len_min = self.kind.hash_bytes();
        for _ in 0..amount {
            let Some(len_limit) = self.header(len_min) else {
                continue;
            };
            let pos = self.pos;
            let cur_match = match len_min {
                2 => {
                    let hv = self.hash2();
                    let m = self.hash_at(hv);
                    self.set_hash(hv, pos);
                    m
                }
                3 => {
                    let (h2, _, hv) = self.hashes(3);
                    let m = self.hash_at(FIX_3_HASH_SIZE.wrapping_add(hv));
                    self.set_hash(h2, pos);
                    self.set_hash(FIX_3_HASH_SIZE.wrapping_add(hv), pos);
                    m
                }
                _ => {
                    let (h2, h3, hv) = self.hashes(4);
                    let m = self.hash_at(FIX_4_HASH_SIZE.wrapping_add(hv));
                    self.set_hash(h2, pos);
                    self.set_hash(FIX_3_HASH_SIZE.wrapping_add(h3), pos);
                    self.set_hash(FIX_4_HASH_SIZE.wrapping_add(hv), pos);
                    m
                }
            };
            self.bt_skip_one(len_limit, cur_match);
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Repetitive bytes with some variety: matches of many lengths and
    /// distances, some past a 4 KiB dictionary.
    fn sample() -> Vec<u8> {
        (0..30_000u32)
            .map(|i| (((i * 7) % 251) ^ (i / 97) ^ ((i / 5003) * 13)) as u8)
            .collect()
    }

    /// Runs `mf` over its input as the encoder does -- finding, then skipping
    /// the rest of each match -- and returns every result.
    fn run(mf: &mut Mf<'_>) -> Vec<(u32, Vec<Match>)> {
        let mut matches = vec![Match::default(); MATCH_LEN_MAX as usize + 1];
        let mut out = Vec::new();
        while !mf.at_end() {
            let (len, count) = mf.find(&mut matches);
            out.push((len, matches[..count as usize].to_vec()));
            if len > 1 {
                mf.skip((len - 1).min(mf.avail()));
            }
        }
        out
    }

    /// Normalising the stored positions before they overflow changes no
    /// match: a finder whose positions start 10 000 short of `u32::MAX`
    /// normalises part way through -- emptying entries older than the
    /// dictionary -- and finds exactly what one starting at the bottom finds.
    #[test]
    fn normalisation_changes_no_match() {
        let data = sample();
        for kind in [
            MatchFinder::Hc3,
            MatchFinder::Hc4,
            MatchFinder::Bt2,
            MatchFinder::Bt3,
            MatchFinder::Bt4,
        ] {
            let mut plain = Mf::new(&data, 4096, kind, 32, 0);
            let mut high = Mf::new(&data, 4096, kind, 32, 0);
            high.pos = u32::MAX - 10_000;
            let want = run(&mut plain);
            let got = run(&mut high);
            assert!(high.pos < 1 << 20, "{kind:?} never normalised");
            assert_eq!(got, want, "{kind:?}");
        }
    }

    /// `lzma_memcmplen`: agreement from `len` to the first difference, or to
    /// `limit`.
    #[test]
    fn memcmplen_stops_at_the_first_difference() {
        let buf = b"abcdefghijklmnopqrstuvwxyz_abcdefghijklmnopqrsXuvwxyz";
        assert_eq!(memcmplen(buf, 0, 27, 0, 26), 19);
        assert_eq!(memcmplen(buf, 0, 27, 5, 26), 19);
        assert_eq!(memcmplen(buf, 0, 27, 0, 10), 10);
        assert_eq!(memcmplen(buf, 0, 27, 19, 19), 19);
        assert_eq!(memcmplen(buf, 1, 27, 0, 5), 0);
    }
}
