//! Finding the lines that moved, and scrolling them into place:
//! `hashmap.c` and `hardscroll.c`.
//!
//! Each line of `curscr` and `newscr` is hashed. A line whose hash occurs
//! once on each screen, at different rows, anchors a move; runs of lines
//! moving together grow from the anchors while moving them is cheaper than
//! rewriting them; runs too short, or moving too far for their size, are
//! dropped. Then the terminal scrolls each run the shortest way it can
//! ([`crate::update::scrolln`]) -- first those moving up, top to bottom,
//! then those moving down, bottom to top -- and `curscr` and the old hashes
//! move with it.
//!
//! The table of hashes is upstream's: `(lines + 1) * 2` slots searched in
//! order and ended by the first whose hash is 0, so a line that happens to
//! hash to 0 shares its slot with the next new hash, as it does there.

use crate::addch::Ctype;
use crate::caps::boolean;
use crate::cell::Cell;
use crate::term::Term;
use crate::update;
use crate::window::{NEWINDEX, NOCHANGE, Window};

/// One slot of the table: `HASHMAP`.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    hashval: u64,
    oldcount: i32,
    newcount: i32,
    oldindex: i32,
    newindex: i32,
}

/// The screen's hashing state: `oldhash`, `newhash`, `hashtab`,
/// `hashtab_len`, `_oldnum_list`.
#[derive(Clone, Debug, Default)]
pub struct HashState {
    /// `oldhash`: each `curscr` line's hash, once made.
    pub oldhash: Option<Vec<u64>>,
    /// `newhash`: each `newscr` line's hash, once made.
    pub newhash: Option<Vec<u64>>,
    hashtab: Vec<Slot>,
    /// `hashtab_len`: the lines the table was made for.
    hashtab_len: i32,
    /// `OLDNUM (n)`: the `curscr` line that moves to `newscr` line `n`, or
    /// [`NEWINDEX`].
    pub oldnum: Vec<i32>,
}

/// `hash (text)`: `result += (result << 5) + ch.chars[0]`, in `unsigned
/// long`.
fn hash(text: &[Cell]) -> u64 {
    text.iter().fold(0u64, |result, ch| {
        let v = i64::from(ch.chars[0]).cast_unsigned();
        result.wrapping_add(result.wrapping_shl(5).wrapping_add(v))
    })
}

/// `update_cost (from, to)`: the cells that differ.
fn update_cost(from: &[Cell], to: &[Cell]) -> i32 {
    let n = from.iter().zip(to).filter(|(a, b)| a != b).count();
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// `update_cost_from_blank (to)`: the cells that are not blank -- a blank
/// in `stdscr`'s background pair on a terminal that erases in colour.
fn update_cost_from_blank(t: &Term, to: &[Cell], stdscr_bkgd: &Cell) -> i32 {
    let mut blank = Cell::blank();
    if t.flag(boolean::BACK_COLOR_ERASE) {
        blank.set_pair(stdscr_bkgd.pair());
    }
    let n = to.iter().filter(|c| **c != blank).count();
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A screen's line `n`'s text, `TEXTWIDTH` cells of it.
fn text(win: &Window, n: i32, width: usize) -> Vec<Cell> {
    win.line(n)
        .map(|l| l.text.iter().take(width).copied().collect())
        .unwrap_or_default()
}

impl HashState {
    fn oldnum(&self, n: i32) -> i32 {
        usize::try_from(n)
            .ok()
            .and_then(|n| self.oldnum.get(n))
            .copied()
            .unwrap_or(NEWINDEX)
    }

    fn set_oldnum(&mut self, n: i32, v: i32) {
        if let Some(slot) = usize::try_from(n).ok().and_then(|n| self.oldnum.get_mut(n)) {
            *slot = v;
        }
    }

    fn newhash_at(&self, n: i32) -> u64 {
        self.newhash
            .as_ref()
            .and_then(|h| usize::try_from(n).ok().and_then(|n| h.get(n)))
            .copied()
            .unwrap_or(0)
    }

    fn oldhash_at(&self, n: i32) -> u64 {
        self.oldhash
            .as_ref()
            .and_then(|h| usize::try_from(n).ok().and_then(|n| h.get(n)))
            .copied()
            .unwrap_or(0)
    }

    /// `oldhash[row] = newhash[row]` for one row, when both exist
    /// (`TransformLine`).
    pub fn copy_one(&mut self, row: i32) {
        let v = self.newhash_at(row);
        if self.newhash.is_some()
            && let Some(old) = self.oldhash.as_mut()
            && let Some(slot) = usize::try_from(row).ok().and_then(|r| old.get_mut(r))
        {
            *slot = v;
        }
    }

    /// `oldhash[row] = newhash[row]` from `top` to the last line
    /// (`ClrBottom`).
    pub fn copy_new_to_old(&mut self, top: i32, lines: i32) {
        for row in top.max(0)..lines {
            self.copy_one(row);
        }
    }

    /// `_nc_make_oldhash (i)`: line `i` of `curscr` hashed again.
    pub fn make_oldhash(&mut self, curscr: &Window, i: i32) {
        let width = usize::try_from(i32::from(curscr.maxx).wrapping_add(1)).unwrap_or(0);
        let h = hash(&text(curscr, i, width));
        if let Some(slot) = self
            .oldhash
            .as_mut()
            .and_then(|o| usize::try_from(i).ok().and_then(|i| o.get_mut(i)))
        {
            *slot = h;
        }
    }

    /// `_nc_scroll_oldhash (n, top, bot)`: the old hashes moved as
    /// `curscr` was, the rows uncovered hashed afresh.
    pub fn scroll_oldhash(&mut self, curscr: &Window, n: i32, top: i32, bot: i32) {
        let Some(old) = self.oldhash.as_mut() else {
            return;
        };
        let size = usize::try_from(
            bot.wrapping_sub(top)
                .wrapping_add(1)
                .wrapping_sub(n.wrapping_abs()),
        )
        .unwrap_or(0);
        let width = usize::try_from(i32::from(curscr.maxx).wrapping_add(1)).unwrap_or(0);
        let top_u = usize::try_from(top).unwrap_or(0);
        let shift = usize::try_from(n.wrapping_abs()).unwrap_or(0);
        if n > 0 {
            for k in 0..size {
                let from = top_u.saturating_add(shift).saturating_add(k);
                if let Some(v) = old.get(from).copied()
                    && let Some(slot) = old.get_mut(top_u.saturating_add(k))
                {
                    *slot = v;
                }
            }
            let mut i = bot;
            while i > bot.wrapping_sub(n) {
                let h = hash(&text(curscr, i, width));
                if let Some(slot) = usize::try_from(i).ok().and_then(|i| old.get_mut(i)) {
                    *slot = h;
                }
                i = i.wrapping_sub(1);
            }
        } else {
            for k in (0..size).rev() {
                let from = top_u.saturating_add(k);
                if let Some(v) = old.get(from).copied()
                    && let Some(slot) = old.get_mut(top_u.saturating_add(shift).saturating_add(k))
                {
                    *slot = v;
                }
            }
            let mut i = top;
            while i < top.wrapping_sub(n) {
                let h = hash(&text(curscr, i, width));
                if let Some(slot) = usize::try_from(i).ok().and_then(|i| old.get_mut(i)) {
                    *slot = h;
                }
                i = i.wrapping_add(1);
            }
        }
    }

    /// `cost_effective (from, to, blank)`: moving line `from` to `to` (which
    /// is blank, or not) costs no more than rewriting.
    #[allow(clippy::too_many_arguments)]
    fn cost_effective(
        &self,
        t: &Term,
        newscr: &Window,
        curscr: &Window,
        from: i32,
        to: i32,
        blank: bool,
        stdscr_bkgd: &Cell,
    ) -> bool {
        if from == to {
            return false;
        }
        let width = usize::try_from(i32::from(curscr.maxx).wrapping_add(1)).unwrap_or(0);
        let mut new_from = self.oldnum(from);
        if new_from == NEWINDEX {
            new_from = from;
        }
        let new_t = |n| text(newscr, n, width);
        let old_t = |n| text(curscr, n, width);
        // "On the left side of >= is the cost before moving; on the right
        // side -- cost after moving."
        let before = (if blank {
            update_cost_from_blank(t, &new_t(to), stdscr_bkgd)
        } else {
            update_cost(&old_t(to), &new_t(to))
        })
        .wrapping_add(update_cost(&old_t(new_from), &new_t(from)));
        let after = (if new_from == from {
            update_cost_from_blank(t, &new_t(from), stdscr_bkgd)
        } else {
            update_cost(&old_t(new_from), &new_t(from))
        })
        .wrapping_add(update_cost(&old_t(from), &new_t(to)));
        before >= after
    }

    /// `grow_hunks ()`: the anchored runs grown forwards and back while
    /// moving the next line with them pays.
    fn grow_hunks(
        &mut self,
        t: &Term,
        newscr: &Window,
        curscr: &Window,
        lines: i32,
        stdscr_bkgd: &Cell,
    ) {
        let mut back_limit: i32 = 0;
        let mut back_ref_limit: i32 = 0;
        let mut i: i32 = 0;
        while i < lines && self.oldnum(i) == NEWINDEX {
            i = i.wrapping_add(1);
        }
        while i < lines {
            let start = i;
            let shift = self.oldnum(i).wrapping_sub(i);
            // "get forward limit"
            i = start.wrapping_add(1);
            while i < lines && self.oldnum(i) != NEWINDEX && self.oldnum(i).wrapping_sub(i) == shift
            {
                i = i.wrapping_add(1);
            }
            let end = i;
            while i < lines && self.oldnum(i) == NEWINDEX {
                i = i.wrapping_add(1);
            }
            let next_hunk = i;
            let mut forward_limit = i;
            let forward_ref_limit = if i >= lines || self.oldnum(i) >= i {
                i
            } else {
                self.oldnum(i)
            };

            i = start.wrapping_sub(1);
            // "grow back"
            if shift < 0 {
                back_limit = back_ref_limit.wrapping_add(shift.wrapping_neg());
            }
            while i >= back_limit {
                if self.newhash_at(i) == self.oldhash_at(i.wrapping_add(shift))
                    || self.cost_effective(
                        t,
                        newscr,
                        curscr,
                        i.wrapping_add(shift),
                        i,
                        shift < 0,
                        stdscr_bkgd,
                    )
                {
                    self.set_oldnum(i, i.wrapping_add(shift));
                } else {
                    break;
                }
                i = i.wrapping_sub(1);
            }

            i = end;
            // "grow forward"
            if shift > 0 {
                forward_limit = forward_ref_limit.wrapping_sub(shift);
            }
            while i < forward_limit {
                if self.newhash_at(i) == self.oldhash_at(i.wrapping_add(shift))
                    || self.cost_effective(
                        t,
                        newscr,
                        curscr,
                        i.wrapping_add(shift),
                        i,
                        shift > 0,
                        stdscr_bkgd,
                    )
                {
                    self.set_oldnum(i, i.wrapping_add(shift));
                } else {
                    break;
                }
                i = i.wrapping_add(1);
            }

            back_limit = i;
            back_ref_limit = i;
            if shift > 0 {
                back_ref_limit = back_ref_limit.wrapping_add(shift);
            }
            i = next_hunk;
        }
    }

    /// The slot of `hashval` in the table: the first that has it, or the
    /// first empty one -- `for (hsp = hashtab; hsp->hashval; hsp++)`.
    fn slot_of(&self, hashval: u64) -> usize {
        let mut k = 0usize;
        while let Some(s) = self.hashtab.get(k) {
            if s.hashval == 0 || s.hashval == hashval {
                return k;
            }
            k = k.saturating_add(1);
        }
        k.saturating_sub(1)
    }

    /// `_nc_hash_map ()`: `OLDNUM` worked out for every line.
    pub fn hash_map(
        &mut self,
        t: &Term,
        newscr: &Window,
        curscr: &Window,
        lines: i32,
        stdscr_bkgd: &Cell,
    ) {
        let n_lines = usize::try_from(lines).unwrap_or(0);
        if lines > self.hashtab_len {
            self.hashtab = vec![Slot::default(); n_lines.saturating_add(1).saturating_mul(2)];
            self.hashtab_len = lines;
        }
        let width = usize::try_from(i32::from(curscr.maxx).wrapping_add(1)).unwrap_or(0);
        if self.oldhash.is_some() && self.newhash.is_some() {
            // "re-hash only changed lines"
            for i in 0..lines {
                if newscr.line(i).is_some_and(|l| l.firstchar != NOCHANGE) {
                    let h = hash(&text(newscr, i, width));
                    if let Some(slot) = self
                        .newhash
                        .as_mut()
                        .and_then(|v| usize::try_from(i).ok().and_then(|i| v.get_mut(i)))
                    {
                        *slot = h;
                    }
                }
            }
        } else {
            // "re-hash all"
            let newh: Vec<u64> = (0..lines).map(|i| hash(&text(newscr, i, width))).collect();
            let oldh: Vec<u64> = (0..lines).map(|i| hash(&text(curscr, i, width))).collect();
            self.newhash = Some(newh);
            self.oldhash = Some(oldh);
        }

        // "Set up and count line-hash values."
        for s in &mut self.hashtab {
            *s = Slot::default();
        }
        for i in 0..lines {
            let hashval = self.oldhash_at(i);
            let k = self.slot_of(hashval);
            if let Some(s) = self.hashtab.get_mut(k) {
                s.hashval = hashval;
                s.oldcount = s.oldcount.wrapping_add(1);
                s.oldindex = i;
            }
        }
        if self.oldnum.len() < n_lines {
            self.oldnum.resize(n_lines, NEWINDEX);
        }
        for i in 0..lines {
            let hashval = self.newhash_at(i);
            let k = self.slot_of(hashval);
            if let Some(s) = self.hashtab.get_mut(k) {
                s.hashval = hashval;
                s.newcount = s.newcount.wrapping_add(1);
                s.newindex = i;
            }
            // "initialize old indices array"
            self.set_oldnum(i, NEWINDEX);
        }

        // "Mark line pairs corresponding to unique hash pairs."
        let mut k = 0usize;
        while let Some(s) = self.hashtab.get(k).copied() {
            if s.hashval == 0 {
                break;
            }
            if s.oldcount == 1 && s.newcount == 1 && s.oldindex != s.newindex {
                self.set_oldnum(s.newindex, s.oldindex);
            }
            k = k.saturating_add(1);
        }

        self.grow_hunks(t, newscr, curscr, lines, stdscr_bkgd);

        // "Eliminate bad or impossible shifts -- this includes removing those
        // hunks which could not grow because of conflicts, as well those which
        // are to be moved too far, they are likely to destroy more than carry."
        let mut i = 0;
        while i < lines {
            while i < lines && self.oldnum(i) == NEWINDEX {
                i = i.wrapping_add(1);
            }
            if i >= lines {
                break;
            }
            let mut start = i;
            let shift = self.oldnum(i).wrapping_sub(i);
            i = i.wrapping_add(1);
            while i < lines && self.oldnum(i) != NEWINDEX && self.oldnum(i).wrapping_sub(i) == shift
            {
                i = i.wrapping_add(1);
            }
            let size = i.wrapping_sub(start);
            if size < 3 || size.wrapping_add((size / 8).min(2)) < shift.wrapping_abs() {
                while start < i {
                    self.set_oldnum(start, NEWINDEX);
                    start = start.wrapping_add(1);
                }
            }
        }

        // "After clearing invalid hunks, try grow the rest."
        self.grow_hunks(t, newscr, curscr, lines, stdscr_bkgd);
    }
}

/// `_nc_scroll_optimize ()`: the lines that moved scrolled into place --
/// those moving up, from the top, then those moving down, from the bottom.
pub fn scroll_optimize(
    t: &mut Term,
    newscr: &Window,
    curscr: &mut Window,
    hash: &mut HashState,
    ctype: &dyn Ctype,
    stdscr_bkgd: &Cell,
) {
    let lines = t.lines;
    hash.hash_map(t, newscr, curscr, lines, stdscr_bkgd);
    if hash.hashtab_len < lines {
        return;
    }
    // "pass 1 - from top to bottom scrolling up"
    let mut i = 0;
    while i < lines {
        while i < lines && (hash.oldnum(i) == NEWINDEX || hash.oldnum(i) <= i) {
            i = i.wrapping_add(1);
        }
        if i >= lines {
            break;
        }
        let shift = hash.oldnum(i).wrapping_sub(i);
        let start = i;
        i = i.wrapping_add(1);
        while i < lines && hash.oldnum(i) != NEWINDEX && hash.oldnum(i).wrapping_sub(i) == shift {
            i = i.wrapping_add(1);
        }
        let end = i.wrapping_sub(1).wrapping_add(shift);
        update::scrolln(
            t,
            newscr,
            curscr,
            hash,
            ctype,
            stdscr_bkgd,
            shift,
            start,
            end,
            lines.wrapping_sub(1),
        );
    }
    // "pass 2 - from bottom to top scrolling down"
    let mut i = lines.wrapping_sub(1);
    while i >= 0 {
        while i >= 0 && (hash.oldnum(i) == NEWINDEX || hash.oldnum(i) >= i) {
            i = i.wrapping_sub(1);
        }
        if i < 0 {
            break;
        }
        let shift = hash.oldnum(i).wrapping_sub(i);
        let end = i;
        i = i.wrapping_sub(1);
        while i >= 0 && hash.oldnum(i) != NEWINDEX && hash.oldnum(i).wrapping_sub(i) == shift {
            i = i.wrapping_sub(1);
        }
        let start = i.wrapping_add(1).wrapping_sub(shift.wrapping_neg());
        update::scrolln(
            t,
            newscr,
            curscr,
            hash,
            ctype,
            stdscr_bkgd,
            shift,
            start,
            end,
            lines.wrapping_sub(1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_upstreams_arithmetic() {
        let line = [Cell::new2(0x61, 0), Cell::new2(0x62, 0)];
        // ((0 * 33) + 97) * 33 + 98.
        assert_eq!(hash(&line), 97 * 33 + 98);
        assert_eq!(hash(&[]), 0);
        // A negative character sign-extends, as `(unsigned long)` of an int.
        let odd = [Cell::new2(-1, 0)];
        assert_eq!(hash(&odd), u64::MAX);
    }

    #[test]
    fn the_cost_of_a_line_is_its_differing_cells() {
        let a = [Cell::new2(0x61, 0), Cell::blank(), Cell::new2(0x63, 0)];
        let b = [Cell::new2(0x61, 0), Cell::new2(0x62, 0), Cell::blank()];
        assert_eq!(update_cost(&a, &b), 2);
        assert_eq!(update_cost(&a, &a), 0);
    }
}
