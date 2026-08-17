//! An approximate summary of a set of glyph ids, for skipping work in O(1).
//!
//! Applying an OpenType layout table means running every lookup the run's
//! script selects across every glyph in the run. On the development host's
//! monospace face that is **147 lookups over 768 subtables**, and a line of
//! plain Latin reaches all 147 of them: the script selects them, so each one
//! is walked glyph by glyph and each glyph is looked up in each of the
//! lookup's subtables' coverage tables. Almost every one of those searches
//! answers "no" — the lookups belong to features and scripts the run does not
//! contain — and the cost of asking was measured at ~64 microseconds per
//! character, which is roughly three orders of magnitude off HarfBuzz.
//!
//! A digest is what makes the question cheap enough to ask before doing the
//! search. It is a *conservative* membership summary: `may_have` is allowed to
//! answer "yes" for a glyph that is not in the set, and is never allowed to
//! answer "no" for one that is. That one-sidedness is what makes it safe to
//! use as a skip test — a false "yes" costs a search that was going to happen
//! anyway, and a false "no" cannot occur, so no substitution can be missed.
//!
//! This is HarfBuzz's `hb_set_digest_t`, with the same three-way shape:
//! three 64-bit masks, one indexed by bits 4-9 of the glyph id, one by bits
//! 0-5, one by bits 9-14. Three because one alone is too coarse — glyph ids
//! are dense and clustered, so any single window of six bits saturates on a
//! face with more than a few hundred glyphs — and because a set has to miss
//! *all* three to be excluded, the three windows overlapping different parts
//! of the id is what keeps the filter selective. It is not a Bloom filter:
//! there is no hashing, the bits are read straight out of the id, which is
//! why it costs three shifts rather than a hash.

/// One 64-bit window's bit for `glyph`, where `shift` selects the window.
///
/// `& 63` is the modulo the whole scheme rests on, and is also what makes the
/// shift unconditionally in range.
const fn bit(glyph: u16, shift: u32) -> u64 {
    1u64 << (((glyph as u32) >> shift) & 63)
}

/// The bits one window needs for every glyph from `first` to `last`.
///
/// HarfBuzz's `add_range`. When the range spans the whole window every bit is
/// reachable and the answer is all ones; otherwise the bits from `lo` to `hi`
/// inclusive are `2^(hi+1) - 2^lo`, which is `2*mb - ma` — and the `- 1` is
/// what wraps that identity round the end of the word when `hi`'s bit has
/// come out below `lo`'s.
const fn span(first: u16, last: u16, shift: u32) -> u64 {
    let (lo, hi) = ((first as u32) >> shift, (last as u32) >> shift);
    if hi.wrapping_sub(lo) >= 63 {
        return u64::MAX;
    }
    let (ma, mb) = (bit(first, shift), bit(last, shift));
    mb.wrapping_add(mb.wrapping_sub(ma))
        .wrapping_sub(if mb < ma { 1 } else { 0 })
}

/// A conservative summary of which glyph ids a set may contain.
///
/// [`Digest::default`] is the empty set, which excludes everything. A digest
/// that could not be computed must be [`Digest::full`] instead, which excludes
/// nothing: an unreadable coverage table has to mean "assume it covers
/// everything", or a malformed font would silently lose substitutions rather
/// than merely render oddly.
///
/// Three separate fields rather than an array, because every operation here
/// touches all three and a loop over them would only obscure that — and would
/// index a slice on the hottest path in shaping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Digest {
    /// Indexed by bits 4-9 of the glyph id.
    mid: u64,
    /// Indexed by bits 0-5.
    low: u64,
    /// Indexed by bits 9-14.
    high: u64,
}

impl Digest {
    /// The set that may contain nothing.
    pub(crate) const EMPTY: Self = Self {
        mid: 0,
        low: 0,
        high: 0,
    };

    /// The set that may contain anything — what an unreadable table becomes.
    pub(crate) const fn full() -> Self {
        Self {
            mid: u64::MAX,
            low: u64::MAX,
            high: u64::MAX,
        }
    }

    /// Record that `glyph` is in the set.
    pub(crate) const fn add(&mut self, glyph: u16) {
        self.mid |= bit(glyph, 4);
        self.low |= bit(glyph, 0);
        self.high |= bit(glyph, 9);
    }

    /// Record that every glyph from `first` to `last` inclusive is in the set.
    ///
    /// A range is set in one step rather than by looping over its members,
    /// because a coverage format 2 range may span thousands of ids and the
    /// digest cannot tell them apart anyway.
    pub(crate) const fn add_range(&mut self, first: u16, last: u16) {
        if first > last {
            return;
        }
        self.mid |= span(first, last, 4);
        self.low |= span(first, last, 0);
        self.high |= span(first, last, 9);
    }

    /// Fold `other`'s members into this set.
    pub(crate) const fn union(&mut self, other: Self) {
        self.mid |= other.mid;
        self.low |= other.low;
        self.high |= other.high;
    }

    /// Whether `glyph` may be in the set. Never `false` for a member.
    pub(crate) const fn may_have(self, glyph: u16) -> bool {
        self.mid & bit(glyph, 4) != 0
            && self.low & bit(glyph, 0) != 0
            && self.high & bit(glyph, 9) != 0
    }

    /// Whether the two sets may share a member. Never `false` when they do.
    pub(crate) const fn may_intersect(self, other: Self) -> bool {
        self.mid & other.mid != 0 && self.low & other.low != 0 && self.high & other.high != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only property that matters for correctness: a member is never
    /// excluded. Checked exhaustively over the whole glyph-id space for a
    /// handful of sets, because "never" is the claim and a sample would not
    /// establish it.
    #[test]
    fn a_member_is_never_excluded() {
        for step in [1u32, 7, 64, 511, 4096] {
            let mut digest = Digest::EMPTY;
            let members: Vec<u16> = (0..=u32::from(u16::MAX))
                .step_by(step as usize)
                .filter_map(|g| u16::try_from(g).ok())
                .collect();
            for &g in &members {
                digest.add(g);
            }
            for &g in &members {
                assert!(digest.may_have(g), "step {step} lost glyph {g}");
            }
        }
    }

    /// The same claim for ranges, which take the arithmetic path rather than
    /// the bit-at-a-time one — including ranges whose end bit sits below their
    /// start bit, which is where the `- (mb < ma)` correction earns its place.
    #[test]
    fn a_range_never_excludes_its_members() {
        for (first, last) in [
            (0u16, 0u16),
            (0, 1),
            (5, 5),
            (60, 70),
            (1000, 1063),
            (1000, 1064),
            (1000, 5000),
            (40000, 40001),
            (0, u16::MAX),
            (u16::MAX, u16::MAX),
        ] {
            let mut digest = Digest::EMPTY;
            digest.add_range(first, last);
            for g in first..=last {
                assert!(
                    digest.may_have(g),
                    "range {first}..={last} lost glyph {g}"
                );
                if g == u16::MAX {
                    break;
                }
            }
        }
    }

    /// A range must summarise exactly what adding its members one at a time
    /// would, or the two ways a font can spell the same coverage would give
    /// different answers.
    #[test]
    fn a_range_matches_adding_its_members() {
        for (first, last) in [(0u16, 200u16), (500, 517), (63, 129), (9000, 9600)] {
            let mut ranged = Digest::EMPTY;
            ranged.add_range(first, last);
            let mut one_by_one = Digest::EMPTY;
            for g in first..=last {
                one_by_one.add(g);
            }
            assert_eq!(ranged, one_by_one, "range {first}..={last}");
        }
    }

    /// The whole point: a set of Latin glyph ids must exclude a distant one,
    /// or the filter would never skip anything.
    #[test]
    fn a_distant_glyph_is_excluded() {
        let mut latin = Digest::EMPTY;
        latin.add_range(3, 120);
        assert!(!latin.may_have(9000));
        assert!(!latin.may_have(40000));
        assert!(latin.may_have(50));
    }

    /// Intersection is one-sided the same way membership is.
    #[test]
    fn intersection_never_misses_a_shared_member() {
        let mut a = Digest::EMPTY;
        a.add_range(10, 90);
        let mut b = Digest::EMPTY;
        b.add(50);
        assert!(a.may_intersect(b));
        assert!(b.may_intersect(a));

        let mut far = Digest::EMPTY;
        far.add_range(20000, 20100);
        assert!(!a.may_intersect(far));
    }

    /// The empty set excludes everything and the full set excludes nothing —
    /// the two defaults the callers rely on.
    #[test]
    fn the_extremes_behave() {
        let empty = Digest::EMPTY;
        assert!(!empty.may_have(0));
        assert!(!empty.may_intersect(Digest::full()));

        let full = Digest::full();
        for g in [0u16, 1, 255, 4096, u16::MAX] {
            assert!(full.may_have(g));
        }
    }

    /// Union is what builds a lookup's digest out of its subtables'.
    #[test]
    fn union_keeps_both_sides() {
        let mut a = Digest::EMPTY;
        a.add(5);
        let mut b = Digest::EMPTY;
        b.add(9000);
        a.union(b);
        assert!(a.may_have(5));
        assert!(a.may_have(9000));
    }
}
