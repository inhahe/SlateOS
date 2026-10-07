//! Choosing what to code next: liblzma's `lzma_encoder_optimum_fast.c` (the
//! greedy chooser of presets 0 to 3) and `lzma_encoder_optimum_normal.c`
//! (the price-driven parser of presets 4 to 9 and the extreme presets).
//!
//! Both return `(back, len)`: `back` is `u32::MAX` for a literal, below
//! [`REPS`] for a repeated distance, and otherwise a distance plus `REPS`.
//! The normal parser looks up to `OPTS` positions ahead, prices every way to
//! reach each one, and walks the cheapest path back (`backward`); the path's
//! symbols are then handed out one call at a time.
//!
//! The arithmetic is liblzma's `uint32_t` arithmetic: prices are sums that
//! stay far below `RC_INFINITY_PRICE` (2^30), and positions and lengths are
//! bounded by `OPTS` and the input, so it is written with wrapping
//! operations, which it never needs.

// liblzma's `uint32_t` arithmetic: prices are sums of a few hundred table
// entries, each under 128, kept below `RC_INFINITY_PRICE` (2^30) plus one more
// symbol; positions and lengths are bounded by `OPTS` (4096) and 273; nothing
// here comes near 2^32. Where liblzma relies on wrapping, the code says so.
#![allow(clippy::arithmetic_side_effects)]

use super::lzma::{
    DIST_ALIGN, DIST_SLOT, DIST_SLOTS, DIST_SPECIAL, Encoder, IS_MATCH, IS_REP, IS_REP0,
    IS_REP0_LONG, IS_REP1, IS_REP2, OPTS, Optimal, REPS, dist_state, is_literal_state,
    next_after_literal, next_after_long_rep, next_after_match, next_after_short_rep,
};
use super::mf::{MATCH_LEN_MAX, Mf, memcmplen};
use super::price;
use crate::lzma::{
    DIST_MODEL_END, DIST_MODEL_START, DIST_SLOT_BITS, DIST_STATES, FULL_DISTANCES, MATCH_LEN_MIN,
    POS_STATES_MAX,
};

/// `ALIGN_BITS`, `ALIGN_SIZE`, `ALIGN_MASK`
const ALIGN_BITS: u32 = 4;
const ALIGN_SIZE: usize = 16;
const ALIGN_MASK: u32 = 15;

/// `change_pair` in the fast chooser: whether a match at `big_dist` is so
/// much farther than one at `small_dist` that one byte less is worth it.
const fn change_pair(small_dist: u32, big_dist: u32) -> bool {
    (big_dist >> 7) > small_dist
}

/// `not_equal_16`: whether the first two bytes at `a` and `b` differ.
fn not_equal_16(mf: &Mf<'_>, a: usize, b: usize) -> bool {
    mf.byte(a) != mf.byte(b) || mf.byte(a.wrapping_add(1)) != mf.byte(b.wrapping_add(1))
}

impl Encoder {
    fn opt(&self, i: u32) -> Optimal {
        self.opts.get(i as usize).copied().unwrap_or_default()
    }

    fn opt_mut(&mut self, i: u32) -> Option<&mut Optimal> {
        self.opts.get_mut(i as usize)
    }

    fn match_at(&self, i: u32) -> super::mf::Match {
        self.matches.get(i as usize).copied().unwrap_or_default()
    }

    fn is_match_prob(&self, state: u32, pos_state: u32) -> u16 {
        self.prob(IS_MATCH + state as usize * POS_STATES_MAX + pos_state as usize)
    }

    // --- prices ------------------------------------------------------------

    /// `get_literal_price`
    fn literal_price(
        &self,
        pos: u32,
        prev_byte: u8,
        match_mode: bool,
        match_byte: u8,
        symbol: u8,
    ) -> u32 {
        let coder = self.literal_coder(pos, prev_byte);
        let probs = self.probs_from(coder, 0x300);
        if !match_mode {
            return price::bittree(probs, 8, u32::from(symbol));
        }
        let mut price = 0u32;
        let mut offset = 0x100u32;
        let mut symbol = u32::from(symbol) | 0x100;
        let mut match_byte = u32::from(match_byte);
        while symbol < 1 << 16 {
            match_byte <<= 1;
            let match_bit = match_byte & offset;
            let index = offset.wrapping_add(match_bit).wrapping_add(symbol >> 8);
            let bit = (symbol >> 7) & 1;
            let p = probs.get(index as usize).copied().unwrap_or(0);
            price = price.wrapping_add(price::bit(p, bit));
            symbol <<= 1;
            offset &= !(match_byte ^ symbol);
        }
        price
    }

    /// `get_short_rep_price`
    fn short_rep_price(&self, state: u32, pos_state: u32) -> u32 {
        price::bit0(self.prob(IS_REP0 + state as usize)).wrapping_add(price::bit0(
            self.prob(IS_REP0_LONG + state as usize * POS_STATES_MAX + pos_state as usize),
        ))
    }

    /// `get_pure_rep_price`
    fn pure_rep_price(&self, rep_index: u32, state: u32, pos_state: u32) -> u32 {
        let st = state as usize;
        if rep_index == 0 {
            price::bit0(self.prob(IS_REP0 + st)).wrapping_add(price::bit1(
                self.prob(IS_REP0_LONG + st * POS_STATES_MAX + pos_state as usize),
            ))
        } else {
            let mut price = price::bit1(self.prob(IS_REP0 + st));
            if rep_index == 1 {
                price = price.wrapping_add(price::bit0(self.prob(IS_REP1 + st)));
            } else {
                price = price.wrapping_add(price::bit1(self.prob(IS_REP1 + st)));
                price = price.wrapping_add(price::bit(self.prob(IS_REP2 + st), rep_index - 2));
            }
            price
        }
    }

    /// `get_rep_price`
    fn rep_price(&self, rep_index: u32, len: u32, state: u32, pos_state: u32) -> u32 {
        self.rep_len
            .price(len, pos_state)
            .wrapping_add(self.pure_rep_price(rep_index, state, pos_state))
    }

    /// `get_dist_len_price`
    fn dist_len_price(&self, dist: u32, len: u32, pos_state: u32) -> u32 {
        let ds = dist_state(len) as usize;
        let price = if (dist as usize) < FULL_DISTANCES {
            self.dist_prices
                .get(ds * FULL_DISTANCES + dist as usize)
                .copied()
                .unwrap_or(0)
        } else {
            let slot = price::dist_slot(dist) as usize;
            self.dist_slot_prices
                .get(ds * DIST_SLOTS + slot)
                .copied()
                .unwrap_or(0)
                .wrapping_add(
                    self.align_prices
                        .get((dist & ALIGN_MASK) as usize)
                        .copied()
                        .unwrap_or(0),
                )
        };
        price.wrapping_add(self.match_len.price(len, pos_state))
    }

    /// `fill_dist_prices`
    fn fill_dist_prices(&mut self) {
        for ds in 0..DIST_STATES {
            let tree = self.probs_from(DIST_SLOT + ds * DIST_SLOTS, DIST_SLOTS);
            let mut row = [0u32; DIST_SLOTS];
            for slot in 0..self.dist_table_size.min(DIST_SLOTS as u32) {
                let mut p = price::bittree(tree, DIST_SLOT_BITS, slot);
                // Distances from 128 on add their direct bits; the align
                // bits are priced apart (`fill_align_prices`).
                if slot >= DIST_MODEL_END {
                    p = p.wrapping_add(price::direct(
                        (slot >> 1).wrapping_sub(1).wrapping_sub(ALIGN_BITS),
                    ));
                }
                if let Some(r) = row.get_mut(slot as usize) {
                    *r = p;
                }
            }
            for slot in 0..self.dist_table_size.min(DIST_SLOTS as u32) {
                let i = ds * DIST_SLOTS + slot as usize;
                if let (Some(dst), Some(&p)) =
                    (self.dist_slot_prices.get_mut(i), row.get(slot as usize))
                {
                    *dst = p;
                }
            }
            // Distances 0 to 3 are their slots.
            for i in 0..DIST_MODEL_START as usize {
                if let (Some(dst), Some(&p)) = (
                    self.dist_prices.get_mut(ds * FULL_DISTANCES + i),
                    row.get(i),
                ) {
                    *dst = p;
                }
            }
        }
        // Distances 4 to 127: the slot and its footer, priced in the special
        // trees.
        for i in DIST_MODEL_START..FULL_DISTANCES as u32 {
            let slot = price::dist_slot(i);
            let footer_bits = (slot >> 1).wrapping_sub(1);
            let base = (2 | (slot & 1)) << footer_bits;
            let tree_start = (DIST_SPECIAL + base as usize).wrapping_sub(slot as usize + 1);
            let p = price::bittree_reverse(
                self.probs_from(tree_start, 1 << footer_bits),
                footer_bits,
                i.wrapping_sub(base),
            );
            for ds in 0..DIST_STATES {
                let slot_price = self
                    .dist_slot_prices
                    .get(ds * DIST_SLOTS + slot as usize)
                    .copied()
                    .unwrap_or(0);
                if let Some(dst) = self.dist_prices.get_mut(ds * FULL_DISTANCES + i as usize) {
                    *dst = p.wrapping_add(slot_price);
                }
            }
        }
        self.match_price_count = 0;
    }

    /// `fill_align_prices`
    fn fill_align_prices(&mut self) {
        for i in 0..ALIGN_SIZE {
            let p = price::bittree_reverse(
                self.probs_from(DIST_ALIGN, ALIGN_SIZE),
                ALIGN_BITS,
                i as u32,
            );
            if let Some(dst) = self.align_prices.get_mut(i) {
                *dst = p;
            }
        }
        self.align_price_count = 0;
    }

    // --- the fast chooser -------------------------------------------------

    /// `lzma_lzma_optimum_fast`
    pub(crate) fn optimum_fast(&mut self, mf: &mut Mf<'_>) -> (u32, u32) {
        let nice_len = mf.nice_len;
        let (mut len_main, mut matches_count) = if mf.read_ahead == 0 {
            mf.find(&mut self.matches)
        } else {
            (self.longest_match_length, self.matches_count)
        };

        let buf = mf.read_pos.wrapping_sub(1);
        let buf_avail = mf.avail().saturating_add(1).min(MATCH_LEN_MAX);
        if buf_avail < 2 {
            return (u32::MAX, 1);
        }

        // The longest repeat of the four recent distances.
        let mut rep_len = 0u32;
        let mut rep_index = 0u32;
        for (i, &rep) in (0u32..).zip(self.reps.iter()) {
            let back = buf.wrapping_sub(rep as usize).wrapping_sub(1);
            if not_equal_16(mf, buf, back) {
                continue;
            }
            let len = memcmplen(mf.data(), buf, back, 2, buf_avail);
            if len >= nice_len {
                mf.skip(len - 1);
                return (i, len);
            }
            if len > rep_len {
                rep_index = i;
                rep_len = len;
            }
        }

        if len_main >= nice_len {
            let back = self
                .match_at(matches_count.wrapping_sub(1))
                .dist
                .wrapping_add(REPS);
            mf.skip(len_main - 1);
            return (back, len_main);
        }

        let mut back_main = 0u32;
        if len_main >= 2 {
            back_main = self.match_at(matches_count.wrapping_sub(1)).dist;
            while matches_count > 1
                && len_main == self.match_at(matches_count - 2).len.wrapping_add(1)
            {
                if !change_pair(self.match_at(matches_count - 2).dist, back_main) {
                    break;
                }
                matches_count -= 1;
                len_main = self.match_at(matches_count - 1).len;
                back_main = self.match_at(matches_count - 1).dist;
            }
            if len_main == 2 && back_main >= 0x80 {
                len_main = 1;
            }
        }

        if rep_len >= 2
            && (rep_len.wrapping_add(1) >= len_main
                || (rep_len.wrapping_add(2) >= len_main && back_main > 1 << 9)
                || (rep_len.wrapping_add(3) >= len_main && back_main > 1 << 15))
        {
            mf.skip(rep_len - 1);
            return (rep_index, rep_len);
        }

        if len_main < 2 || buf_avail <= 2 {
            return (u32::MAX, 1);
        }

        // Look one byte ahead: if a better match starts there, code this
        // byte as a literal.
        let (longest, count) = mf.find(&mut self.matches);
        self.longest_match_length = longest;
        self.matches_count = count;
        if longest >= 2 {
            let new_dist = self.match_at(count.wrapping_sub(1)).dist;
            if (longest >= len_main && new_dist < back_main)
                || (longest == len_main.wrapping_add(1) && !change_pair(back_main, new_dist))
                || longest > len_main.wrapping_add(1)
                || (longest.wrapping_add(1) >= len_main
                    && len_main >= 3
                    && change_pair(new_dist, back_main))
            {
                return (u32::MAX, 1);
            }
        }

        let buf = buf.wrapping_add(1);
        let limit = len_main.wrapping_sub(1).max(2);
        for &rep in &self.reps {
            let back = buf.wrapping_sub(rep as usize).wrapping_sub(1);
            if memcmplen(mf.data(), buf, back, 0, limit) == limit {
                return (u32::MAX, 1);
            }
        }

        mf.skip(len_main - 2);
        (back_main.wrapping_add(REPS), len_main)
    }

    // --- the normal parser ------------------------------------------------

    /// `make_literal`
    fn make_literal(o: &mut Optimal) {
        o.back_prev = u32::MAX;
        o.prev_1_is_literal = false;
    }

    /// `make_short_rep`
    fn make_short_rep(o: &mut Optimal) {
        o.back_prev = 0;
        o.prev_1_is_literal = false;
    }

    /// Lowers the price of reaching `at` to `price` if that is cheaper,
    /// with `set` filling in how.
    fn improve(&mut self, at: u32, price: u32, set: impl FnOnce(&mut Optimal)) -> bool {
        match self.opt_mut(at) {
            Some(o) if price < o.price => {
                o.price = price;
                set(o);
                true
            }
            _ => false,
        }
    }

    /// Marks positions up to `offset` as not yet reached.
    fn extend(&mut self, len_end: &mut u32, offset: u32) {
        while *len_end < offset {
            *len_end = len_end.wrapping_add(1);
            if let Some(o) = self.opt_mut(*len_end) {
                o.price = price::INFINITY;
            }
        }
    }

    /// `backward`: follows the cheapest path back from `cur`, turning it
    /// into forward links; returns its first symbol.
    fn backward(&mut self, cur: u32) -> (u32, u32) {
        let mut cur = cur;
        self.opts_end_index = cur;
        let mut pos_mem = self.opt(cur).pos_prev;
        let mut back_mem = self.opt(cur).back_prev;
        loop {
            let here = self.opt(cur);
            if here.prev_1_is_literal {
                if let Some(o) = self.opt_mut(pos_mem) {
                    Self::make_literal(o);
                    o.pos_prev = pos_mem.wrapping_sub(1);
                }
                if here.prev_2 {
                    if let Some(o) = self.opt_mut(pos_mem.wrapping_sub(1)) {
                        o.prev_1_is_literal = false;
                        o.pos_prev = here.pos_prev_2;
                        o.back_prev = here.back_prev_2;
                    }
                }
            }
            let pos_prev = pos_mem;
            let back_cur = back_mem;
            back_mem = self.opt(pos_prev).back_prev;
            pos_mem = self.opt(pos_prev).pos_prev;
            if let Some(o) = self.opt_mut(pos_prev) {
                o.back_prev = back_cur;
                o.pos_prev = cur;
            }
            cur = pos_prev;
            if cur == 0 {
                break;
            }
        }
        let first = self.opt(0);
        self.opts_current_index = first.pos_prev;
        (first.back_prev, first.pos_prev)
    }

    /// `helper1`: the choices at the first position. `Err((back, len))` when
    /// the choice is plain without parsing; otherwise the end of the
    /// positions priced so far.
    fn helper1(&mut self, mf: &mut Mf<'_>, position: u32) -> Result<u32, (u32, u32)> {
        let nice_len = mf.nice_len;
        let (len_main, matches_count) = if mf.read_ahead == 0 {
            mf.find(&mut self.matches)
        } else {
            (self.longest_match_length, self.matches_count)
        };

        let buf_avail = mf.avail().saturating_add(1).min(MATCH_LEN_MAX);
        if buf_avail < 2 {
            return Err((u32::MAX, 1));
        }
        let buf = mf.read_pos.wrapping_sub(1);

        // The repeat lengths, and the first longest of them.
        let mut rep_lens = [0u32; REPS as usize];
        let (mut rep_max_index, mut rep_max_len) = (0u32, 0u32);
        for ((i, slot), &rep) in (0u32..).zip(rep_lens.iter_mut()).zip(self.reps.iter()) {
            let back = buf.wrapping_sub(rep as usize).wrapping_sub(1);
            if not_equal_16(mf, buf, back) {
                continue;
            }
            *slot = memcmplen(mf.data(), buf, back, 2, buf_avail);
            if *slot > rep_max_len {
                rep_max_index = i;
                rep_max_len = *slot;
            }
        }

        if rep_max_len >= nice_len {
            mf.skip(rep_max_len - 1);
            return Err((rep_max_index, rep_max_len));
        }

        if len_main >= nice_len {
            let back = self
                .match_at(matches_count.wrapping_sub(1))
                .dist
                .wrapping_add(REPS);
            mf.skip(len_main - 1);
            return Err((back, len_main));
        }

        let current_byte = mf.byte(buf);
        let match_byte = mf.byte(buf.wrapping_sub(self.reps[0] as usize).wrapping_sub(1));

        if len_main < 2 && current_byte != match_byte && rep_max_len < 2 {
            return Err((u32::MAX, 1));
        }

        let state = self.state;
        if let Some(o) = self.opt_mut(0) {
            o.state = state;
        }
        let pos_state = position & self.pos_mask;

        let literal =
            price::bit0(self.is_match_prob(state, pos_state)).wrapping_add(self.literal_price(
                position,
                mf.byte(buf.wrapping_sub(1)),
                !is_literal_state(state),
                match_byte,
                current_byte,
            ));
        if let Some(o) = self.opt_mut(1) {
            o.price = literal;
            Self::make_literal(o);
        }

        let match_price = price::bit1(self.is_match_prob(state, pos_state));
        let rep_match_price =
            match_price.wrapping_add(price::bit1(self.prob(IS_REP + state as usize)));

        if match_byte == current_byte {
            let short_rep_price =
                rep_match_price.wrapping_add(self.short_rep_price(state, pos_state));
            if short_rep_price < self.opt(1).price {
                if let Some(o) = self.opt_mut(1) {
                    o.price = short_rep_price;
                    Self::make_short_rep(o);
                }
            }
        }

        let len_end = len_main.max(rep_max_len);
        if len_end < 2 {
            return Err((self.opt(1).back_prev, 1));
        }

        if let Some(o) = self.opt_mut(1) {
            o.pos_prev = 0;
        }
        let reps = self.reps;
        if let Some(o) = self.opt_mut(0) {
            o.backs = reps;
        }

        let mut len = len_end;
        while len >= 2 {
            if let Some(o) = self.opt_mut(len) {
                o.price = price::INFINITY;
            }
            len -= 1;
        }

        for (i, &len) in (0u32..).zip(rep_lens.iter()) {
            let mut rep_len = len;
            if rep_len < 2 {
                continue;
            }
            let price = rep_match_price.wrapping_add(self.pure_rep_price(i, state, pos_state));
            while rep_len >= 2 {
                let cur_and_len_price = price.wrapping_add(self.rep_len.price(rep_len, pos_state));
                self.improve(rep_len, cur_and_len_price, |o| {
                    o.pos_prev = 0;
                    o.back_prev = i;
                    o.prev_1_is_literal = false;
                });
                rep_len -= 1;
            }
        }

        let normal_match_price =
            match_price.wrapping_add(price::bit0(self.prob(IS_REP + state as usize)));

        let mut len = if rep_lens[0] >= 2 { rep_lens[0] + 1 } else { 2 };
        if len <= len_main {
            let mut i = 0u32;
            while len > self.match_at(i).len {
                i += 1;
            }
            loop {
                let dist = self.match_at(i).dist;
                let cur_and_len_price =
                    normal_match_price.wrapping_add(self.dist_len_price(dist, len, pos_state));
                self.improve(len, cur_and_len_price, |o| {
                    o.pos_prev = 0;
                    o.back_prev = dist.wrapping_add(REPS);
                    o.prev_1_is_literal = false;
                });
                if len == self.match_at(i).len {
                    i += 1;
                    if i == matches_count {
                        break;
                    }
                }
                len += 1;
            }
        }

        Ok(len_end)
    }

    /// `helper2`: the choices at position `cur` of the lookahead, whose
    /// byte is at `buf`.
    #[allow(clippy::too_many_lines)]
    fn helper2(
        &mut self,
        mf: &Mf<'_>,
        reps: &mut [u32; REPS as usize],
        buf: usize,
        len_end: u32,
        position: u32,
        cur: u32,
        nice_len: u32,
        buf_avail_full: u32,
    ) -> u32 {
        let mut len_end = len_end;
        let mut matches_count = self.matches_count;
        let mut new_len = self.longest_match_length;
        let here = self.opt(cur);
        let mut pos_prev = here.pos_prev;
        let mut state;

        if here.prev_1_is_literal {
            pos_prev = pos_prev.wrapping_sub(1);
            if here.prev_2 {
                state = self.opt(here.pos_prev_2).state;
                state = if here.back_prev_2 < REPS {
                    next_after_long_rep(state)
                } else {
                    next_after_match(state)
                };
            } else {
                state = self.opt(pos_prev).state;
            }
            state = next_after_literal(state);
        } else {
            state = self.opt(pos_prev).state;
        }

        if pos_prev == cur.wrapping_sub(1) {
            state = if here.back_prev == 0 {
                next_after_short_rep(state)
            } else {
                next_after_literal(state)
            };
        } else {
            let pos;
            if here.prev_1_is_literal && here.prev_2 {
                pos_prev = here.pos_prev_2;
                pos = here.back_prev_2;
                state = next_after_long_rep(state);
            } else {
                pos = here.back_prev;
                state = if pos < REPS {
                    next_after_long_rep(state)
                } else {
                    next_after_match(state)
                };
            }
            let backs = self.opt(pos_prev).backs;
            *reps = if pos < REPS {
                // The repeated distance moves to the front, those before it
                // one place back.
                let mut r = backs;
                if let Some(head) = r.get_mut(..=pos as usize) {
                    head.rotate_right(1);
                }
                r
            } else {
                [pos - REPS, backs[0], backs[1], backs[2]]
            };
        }

        if let Some(o) = self.opt_mut(cur) {
            o.state = state;
            o.backs = *reps;
        }

        let cur_price = self.opt(cur).price;
        let current_byte = mf.byte(buf);
        let match_byte = mf.byte(buf.wrapping_sub(reps[0] as usize).wrapping_sub(1));
        let pos_state = position & self.pos_mask;

        let cur_and_1_price = cur_price
            .wrapping_add(price::bit0(self.is_match_prob(state, pos_state)))
            .wrapping_add(self.literal_price(
                position,
                mf.byte(buf.wrapping_sub(1)),
                !is_literal_state(state),
                match_byte,
                current_byte,
            ));

        let mut next_is_literal = false;
        if self.improve(cur + 1, cur_and_1_price, |o| {
            o.pos_prev = cur;
            Self::make_literal(o);
        }) {
            next_is_literal = true;
        }

        let match_price = cur_price.wrapping_add(price::bit1(self.is_match_prob(state, pos_state)));
        let rep_match_price =
            match_price.wrapping_add(price::bit1(self.prob(IS_REP + state as usize)));

        let next = self.opt(cur + 1);
        if match_byte == current_byte && !(next.pos_prev < cur && next.back_prev == 0) {
            let short_rep_price =
                rep_match_price.wrapping_add(self.short_rep_price(state, pos_state));
            // `<=`: a short repeat wins a tie with the literal.
            if short_rep_price <= next.price {
                if let Some(o) = self.opt_mut(cur + 1) {
                    o.price = short_rep_price;
                    o.pos_prev = cur;
                    Self::make_short_rep(o);
                }
                next_is_literal = true;
            }
        }

        if buf_avail_full < 2 {
            return len_end;
        }

        let buf_avail = buf_avail_full.min(nice_len);

        if !next_is_literal && match_byte != current_byte {
            // A literal, then a repeat of the last distance.
            let back = buf.wrapping_sub(reps[0] as usize).wrapping_sub(1);
            let limit = buf_avail_full.min(nice_len.wrapping_add(1));
            let len_test = memcmplen(mf.data(), buf, back, 1, limit).wrapping_sub(1);
            if len_test >= 2 {
                let state_2 = next_after_literal(state);
                let pos_state_next = position.wrapping_add(1) & self.pos_mask;
                let next_rep_match_price = cur_and_1_price
                    .wrapping_add(price::bit1(self.is_match_prob(state_2, pos_state_next)))
                    .wrapping_add(price::bit1(self.prob(IS_REP + state_2 as usize)));
                let offset = cur + 1 + len_test;
                self.extend(&mut len_end, offset);
                let cur_and_len_price = next_rep_match_price.wrapping_add(self.rep_price(
                    0,
                    len_test,
                    state_2,
                    pos_state_next,
                ));
                self.improve(offset, cur_and_len_price, |o| {
                    o.pos_prev = cur + 1;
                    o.back_prev = 0;
                    o.prev_1_is_literal = true;
                    o.prev_2 = false;
                });
            }
        }

        let mut start_len = 2u32;

        for (rep_index, &rep) in (0u32..).zip(reps.iter()) {
            let back = buf.wrapping_sub(rep as usize).wrapping_sub(1);
            if not_equal_16(mf, buf, back) {
                continue;
            }
            let mut len_test = memcmplen(mf.data(), buf, back, 2, buf_avail);
            self.extend(&mut len_end, cur + len_test);
            let len_test_temp = len_test;
            let price =
                rep_match_price.wrapping_add(self.pure_rep_price(rep_index, state, pos_state));
            while len_test >= 2 {
                let cur_and_len_price = price.wrapping_add(self.rep_len.price(len_test, pos_state));
                self.improve(cur + len_test, cur_and_len_price, |o| {
                    o.pos_prev = cur;
                    o.back_prev = rep_index;
                    o.prev_1_is_literal = false;
                });
                len_test -= 1;
            }
            len_test = len_test_temp;

            if rep_index == 0 {
                start_len = len_test + 1;
            }

            // The repeat, a literal, then a repeat of the last distance.
            let mut len_test_2 = len_test + 1;
            let limit = buf_avail_full.min(len_test_2.wrapping_add(nice_len));
            if len_test_2 < limit {
                len_test_2 = memcmplen(mf.data(), buf, back, len_test_2, limit);
            }
            len_test_2 = len_test_2.wrapping_sub(len_test + 1);

            if len_test_2 >= 2 {
                let mut state_2 = next_after_long_rep(state);
                let mut pos_state_next = position.wrapping_add(len_test) & self.pos_mask;
                let at = buf.wrapping_add(len_test as usize);
                let cur_and_len_literal_price = price
                    .wrapping_add(self.rep_len.price(len_test, pos_state))
                    .wrapping_add(price::bit0(self.is_match_prob(state_2, pos_state_next)))
                    .wrapping_add(self.literal_price(
                        position.wrapping_add(len_test),
                        mf.byte(at.wrapping_sub(1)),
                        true,
                        mf.byte(back.wrapping_add(len_test as usize)),
                        mf.byte(at),
                    ));
                state_2 = next_after_literal(state_2);
                pos_state_next = position.wrapping_add(len_test).wrapping_add(1) & self.pos_mask;
                let next_rep_match_price = cur_and_len_literal_price
                    .wrapping_add(price::bit1(self.is_match_prob(state_2, pos_state_next)))
                    .wrapping_add(price::bit1(self.prob(IS_REP + state_2 as usize)));
                let offset = cur + len_test + 1 + len_test_2;
                self.extend(&mut len_end, offset);
                let cur_and_len_price = next_rep_match_price.wrapping_add(self.rep_price(
                    0,
                    len_test_2,
                    state_2,
                    pos_state_next,
                ));
                self.improve(offset, cur_and_len_price, |o| {
                    o.pos_prev = cur + len_test + 1;
                    o.back_prev = 0;
                    o.prev_1_is_literal = true;
                    o.prev_2 = true;
                    o.pos_prev_2 = cur;
                    o.back_prev_2 = rep_index;
                });
            }
        }

        if new_len > buf_avail {
            new_len = buf_avail;
            matches_count = 0;
            while new_len > self.match_at(matches_count).len {
                matches_count += 1;
            }
            if let Some(m) = self.matches.get_mut(matches_count as usize) {
                m.len = new_len;
            }
            matches_count += 1;
        }

        if new_len >= start_len {
            let normal_match_price =
                match_price.wrapping_add(price::bit0(self.prob(IS_REP + state as usize)));
            self.extend(&mut len_end, cur + new_len);

            let mut i = 0u32;
            while start_len > self.match_at(i).len {
                i += 1;
            }

            let mut len_test = start_len;
            loop {
                let cur_back = self.match_at(i).dist;
                let mut cur_and_len_price = normal_match_price
                    .wrapping_add(self.dist_len_price(cur_back, len_test, pos_state));
                self.improve(cur + len_test, cur_and_len_price, |o| {
                    o.pos_prev = cur;
                    o.back_prev = cur_back.wrapping_add(REPS);
                    o.prev_1_is_literal = false;
                });

                if len_test == self.match_at(i).len {
                    // The match, a literal, then a repeat of its distance.
                    let back = buf.wrapping_sub(cur_back as usize).wrapping_sub(1);
                    let mut len_test_2 = len_test + 1;
                    let limit = buf_avail_full.min(len_test_2.wrapping_add(nice_len));
                    if len_test_2 < limit {
                        len_test_2 = memcmplen(mf.data(), buf, back, len_test_2, limit);
                    }
                    len_test_2 = len_test_2.wrapping_sub(len_test + 1);

                    if len_test_2 >= 2 {
                        let mut state_2 = next_after_match(state);
                        let mut pos_state_next = position.wrapping_add(len_test) & self.pos_mask;
                        let at = buf.wrapping_add(len_test as usize);
                        let cur_and_len_literal_price = cur_and_len_price
                            .wrapping_add(price::bit0(self.is_match_prob(state_2, pos_state_next)))
                            .wrapping_add(self.literal_price(
                                position.wrapping_add(len_test),
                                mf.byte(at.wrapping_sub(1)),
                                true,
                                mf.byte(back.wrapping_add(len_test as usize)),
                                mf.byte(at),
                            ));
                        state_2 = next_after_literal(state_2);
                        pos_state_next = pos_state_next.wrapping_add(1) & self.pos_mask;
                        let next_rep_match_price = cur_and_len_literal_price
                            .wrapping_add(price::bit1(self.is_match_prob(state_2, pos_state_next)))
                            .wrapping_add(price::bit1(self.prob(IS_REP + state_2 as usize)));
                        let offset = cur + len_test + 1 + len_test_2;
                        self.extend(&mut len_end, offset);
                        cur_and_len_price = next_rep_match_price.wrapping_add(self.rep_price(
                            0,
                            len_test_2,
                            state_2,
                            pos_state_next,
                        ));
                        self.improve(offset, cur_and_len_price, |o| {
                            o.pos_prev = cur + len_test + 1;
                            o.back_prev = 0;
                            o.prev_1_is_literal = true;
                            o.prev_2 = true;
                            o.pos_prev_2 = cur;
                            o.back_prev_2 = cur_back.wrapping_add(REPS);
                        });
                    }

                    i += 1;
                    if i == matches_count {
                        break;
                    }
                }
                len_test += 1;
            }
        }

        len_end
    }

    /// `lzma_lzma_optimum_normal`
    pub(crate) fn optimum_normal(&mut self, mf: &mut Mf<'_>, position: u32) -> (u32, u32) {
        // A path parsed earlier hands out its next symbol.
        if self.opts_end_index != self.opts_current_index {
            let i = self.opts_current_index;
            let o = self.opt(i);
            self.opts_current_index = o.pos_prev;
            return (o.back_prev, o.pos_prev.wrapping_sub(i));
        }

        // The price tables are refreshed only here, between parses.
        if mf.read_ahead == 0 {
            if self.match_price_count >= 1 << 7 {
                self.fill_dist_prices();
            }
            if self.align_price_count >= ALIGN_SIZE as u32 {
                self.fill_align_prices();
            }
        }

        let mut len_end = match self.helper1(mf, position) {
            Ok(len_end) => len_end,
            Err(choice) => return choice,
        };

        let mut reps = self.reps;
        let mut cur = 1u32;
        while cur < len_end {
            let (longest, count) = mf.find(&mut self.matches);
            self.longest_match_length = longest;
            self.matches_count = count;
            if longest >= mf.nice_len {
                break;
            }
            let buf_avail_full = mf.avail().saturating_add(1).min(OPTS - 1 - cur);
            len_end = self.helper2(
                mf,
                &mut reps,
                mf.read_pos.wrapping_sub(1),
                len_end,
                position.wrapping_add(cur),
                cur,
                mf.nice_len,
                buf_avail_full,
            );
            cur += 1;
        }

        self.backward(cur)
    }
}

// Keep the model constants the parser shares with the coder in step.
const _: () = assert!(DIST_STATES == 4 && DIST_SLOTS == 64 && MATCH_LEN_MIN == 2);
const _: () = assert!(DIST_MODEL_START == 4 && DIST_MODEL_END == 14);
