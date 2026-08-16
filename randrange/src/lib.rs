//! A seedable pseudo-random source whose reductions into a range use the bits
//! that are actually random.
//!
//! # Why this crate exists
//!
//! Twenty-seven applications in this tree needed a little randomness — a
//! tetromino, a mine, a card, a maze wall — and each one wrote the same
//! generator by hand. Not a similar one: the *same* one, down to the two magic
//! constants, copy-pasted:
//!
//! ```ignore
//! fn next(&mut self) -> u64 {
//!     self.state = self.state
//!         .wrapping_mul(6_364_136_223_846_793_005)
//!         .wrapping_add(1_442_695_040_888_963_407);
//!     self.state
//! }
//!
//! fn next_bounded(&mut self, bound: usize) -> usize {
//!     (self.next() % bound as u64) as usize
//! }
//! ```
//!
//! Both halves are wrong, and the second one is wrong in a way that a player
//! notices.
//!
//! ## The bug: the low bits of that generator are a counter
//!
//! The first function is a linear congruential generator whose modulus is
//! 2<sup>64</sup> — that is what `wrapping_mul` on a `u64` means. In *any* LCG
//! with a power-of-two modulus, bit *k* of the state has period
//! 2<sup>*k*+1</sup>. Bit 0 alternates. Bits 0–1 cycle through four values.
//! Bits 0–2 through eight. This is not a subtle statistical weakness; the low
//! end of the word is a counter with a very short loop, and it is a
//! well-documented property of the construction rather than a mistake in these
//! constants.
//!
//! `x % bound` keeps exactly the low bits when `bound` is a power of two. So
//! `next_bounded(4)` returns a **four-long cycle, for ever, at every seed**.
//!
//! In `apps/simon` that was the entire game: the sequence a player was asked to
//! memorise was Green, Red, Yellow, Blue, Green, Red, Yellow, Blue, … Its 106
//! tests all passed, because the broken draw uses all four colours *exactly
//! equally* — only the order is degenerate, and nothing was testing the order.
//! That is the trap worth remembering: **a distribution check cannot see this
//! bug.**
//!
//! An odd `bound` escapes, because then the remainder depends on the whole
//! word. That is why the defect hid: `% 7` for tetrominoes looks fine, `% 4`
//! for four colours does not, and the two live three lines apart.
//!
//! ## Fix 1 — reduce with the high bits
//!
//! [`Rng::below`] multiplies by the bound and keeps the **top** half of the
//! 128-bit product, which reads the high bits and never the low ones. It is
//! also very nearly unbiased and needs no rejection loop, so it cannot run
//! long (Lemire, *Fast Random Integer Generation in an Interval*, 2019).
//!
//! For comparison, `x % bound` is biased as well as low-bit-bound: with
//! 2<sup>64</sup> outputs folded into `bound` buckets, the first
//! `2^64 mod bound` buckets get one extra value each. Negligible for a bound
//! of four; not negligible for a bound near 2<sup>64</sup>, and free to avoid.
//!
//! ## Fix 2 — make every bit good, so the next caller cannot be bitten
//!
//! Fixing the reduction is enough for the reductions in this crate, but it
//! leaves a loaded gun: a caller who writes `rng.next_u64() % 2` gets the
//! alternating counter back. So the raw output is permuted before it is
//! returned. The state still advances by the same LCG — the recognisable
//! constants are kept deliberately — and the output is then run through the
//! xor-shift/multiply finaliser used by SplitMix64, which mixes the high bits
//! down into the low ones. After it, every bit of the returned word has the
//! full period of the state.
//!
//! This is the same shape as the PCG family: a cheap LCG for the state, an
//! output permutation to repair the bit quality that the LCG cannot provide.
//!
//! ## What this is not
//!
//! **Not cryptographic.** The state is 64 bits and recoverable from a couple of
//! outputs. It is for shuffling a deck, placing mines and picking a colour.
//! Password generation, key material and nonces must not come from here; those
//! need the system entropy source.
//!
//! **Not stream-compatible with the copies it replaces.** Any test that pinned
//! a specific board layout will produce a different — and, for the first time,
//! actually varied — layout. That is the point of the change, not a
//! regression to be worked around by preserving the old stream.

#![no_std]

/// A seedable, non-cryptographic pseudo-random source.
///
/// Deterministic for a given seed, which is what makes a game reproducible in a
/// test and a bug report reproducible at all.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

/// The LCG multiplier, from Knuth via *Numerical Recipes*. Kept from the
/// hand-written copies this crate replaces so the lineage is recognisable.
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// The LCG increment. Must be odd for the generator to reach every state; this
/// one is.
const INCREMENT: u64 = 1_442_695_040_888_963_407;

impl Rng {
    /// Create a generator from a seed.
    ///
    /// Every seed is valid, including zero: the increment is what stops the
    /// all-zero state being a fixed point, and it is non-zero.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next raw 64-bit output. Every bit of it is equally good; see the
    /// module documentation for why that took an extra step.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        // SplitMix64's finaliser. Without it the low bits of `state` are a
        // short counter, and a caller doing their own `% 2` would get an
        // alternating "random" bit -- the exact bug this crate exists to
        // remove, reintroduced one line outside it.
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0..bound`, or `0` when `bound` is zero.
    ///
    /// Zero is answered rather than refused because the natural caller is
    /// `rng.below(items.len())`, and an empty collection is an ordinary state
    /// for that to be in — while `% 0` divides by zero and takes the process
    /// with it. The returned `0` is then an index the caller's own `get` will
    /// decline, which is where the emptiness gets handled properly.
    pub fn below(&mut self, bound: usize) -> usize {
        // High bits via a widening multiply. `wrapping_mul` cannot actually
        // wrap -- two 64-bit values multiply into 128 bits exactly -- it is
        // there to say so rather than to depend on it.
        let product = u128::from(self.next_u64()).wrapping_mul(bound as u128);
        (product >> 64) as usize
    }

    /// A `u64` in `0..bound`, or `0` when `bound` is zero.
    pub fn below_u64(&mut self, bound: u64) -> u64 {
        let product = u128::from(self.next_u64()).wrapping_mul(u128::from(bound));
        (product >> 64) as u64
    }

    /// A value in `min..=max`, **inclusive at both ends**, or `min` when
    /// `max < min`.
    ///
    /// Inclusive because the callers are all of the form "a row of the board"
    /// or "a card of the deck", where the last one is a legal answer and an
    /// exclusive bound invites the `- 1` that gets forgotten.
    pub fn between(&mut self, min: i64, max: i64) -> i64 {
        let Some(span) = max.checked_sub(min) else {
            // `max - min` overflowing means the range covers most of `i64`;
            // there is no meaningful clamp, so answer from the whole width.
            return self.next_u64() as i64;
        };
        if span < 0 {
            return min;
        }
        // `span + 1` cannot overflow `u64`: `span` is a non-negative `i64`.
        let count = (span as u64).saturating_add(1);
        min.wrapping_add(self.below_u64(count) as i64)
    }

    /// `true` with probability `numerator / denominator`.
    ///
    /// A denominator of zero yields `false` — "none out of nothing" — rather
    /// than dividing by zero. A numerator at or above the denominator yields
    /// `true` always, which is what the fraction says.
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        if denominator == 0 {
            return false;
        }
        self.below_u64(denominator) < numerator
    }

    /// An even coin flip.
    ///
    /// Uses the *top* bit, which needs no reduction at all. (It would be
    /// correct to use the bottom one now that the output is permuted; the top
    /// one is correct for any generator, which is the habit worth keeping.)
    pub fn flip(&mut self) -> bool {
        self.next_u64() >> 63 != 0
    }

    /// A `f32` in `0.0..1.0`.
    ///
    /// Built from the top 24 bits — the width of an `f32`'s significand — so
    /// every value it can return is evenly spaced and exactly representable.
    /// Dividing a full 64-bit draw by `u64::MAX` instead would round, and
    /// rounding up at the top of the range yields exactly `1.0`, which callers
    /// scaling by a length then turn into an out-of-bounds index.
    pub fn unit_f32(&mut self) -> f32 {
        const SIGNIFICAND_BITS: u32 = 24;
        #[allow(
            clippy::cast_precision_loss,
            reason = "the value is below 2^24, which every f32 represents exactly"
        )]
        let bits = (self.next_u64() >> (64 - SIGNIFICAND_BITS)) as f32;
        bits / (1u32 << SIGNIFICAND_BITS) as f32
    }

    /// A `f32` in `min..max`, or `min` when the range is empty or not finite.
    pub fn between_f32(&mut self, min: f32, max: f32) -> f32 {
        let span = max - min;
        if !span.is_finite() || span <= 0.0 {
            return min;
        }
        // `min + unit * span` rather than `mul_add`: the fused form lives in
        // `std`, and this crate is `no_std` so that the headless components can
        // use it too.
        min + self.unit_f32() * span
    }

    /// Pick an element, or `None` from an empty slice.
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        items.get(self.below(items.len()))
    }

    /// Shuffle in place, uniformly.
    ///
    /// Fisher-Yates walked downwards, swapping element `i` with one drawn from
    /// `0..=i`. The upward-walking variant that draws from the *whole* slice
    /// each time — which is the one people write from memory — is not uniform:
    /// it has n^n equally likely execution paths distributed over n! orderings,
    /// and n^n is not divisible by n! for any n above 2, so some orderings come
    /// up more often than others no matter how good the generator is.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        let mut i = items.len();
        while i > 1 {
            i = i.saturating_sub(1);
            let j = self.below(i.saturating_add(1));
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// The property the whole crate exists for: a power-of-two bound must not
    /// produce a cycle whose length is that bound.
    ///
    /// Stated as "the gap between consecutive draws varies" rather than as a
    /// frequency check, because the bug this replaces produces a *perfectly
    /// uniform* frequency table and a completely fixed order.
    ///
    /// Note what this does and does not pin. There are two independent
    /// defences here — the output permutation in [`Rng::next_u64`] and the
    /// high-bit reduction in [`Rng::below`] — and either one alone is enough
    /// to make this pass. Verified by reverting each: with the permutation
    /// present, restoring `% bound` still passes this test (it fails
    /// `below_stays_inside_its_bound_and_survives_zero`, on the division by
    /// zero, which is `%`'s *other* problem). What pins the premise of the
    /// whole crate is
    /// [`the_original_defect_still_cycles_when_reproduced`], which builds the
    /// old code inside the test and watches it fail the same property.
    #[test]
    fn a_power_of_two_bound_does_not_produce_a_cycle() {
        for bound in [2usize, 4, 8, 16, 32, 64] {
            let mut rng = Rng::new(0xDEAD_BEEF_CAFE);
            let draws: [usize; 64] = core::array::from_fn(|_| rng.below(bound));
            let mut gaps = [false; 64];
            for w in draws.windows(2) {
                gaps[(w[1] + bound - w[0]) % bound] = true;
            }
            let distinct = gaps.iter().filter(|seen| **seen).count();
            assert!(
                distinct > 1,
                "with bound {bound} every draw follows the one before it by the \
                 same step; the sequence is a rotation: {draws:?}"
            );
        }
    }

    /// The premise of this crate, reproduced rather than asserted.
    ///
    /// The module documentation makes a strong claim — that the code twenty-
    /// seven applications shared returns a fixed four-long cycle — and a claim
    /// in a comment is not checkable. So the old code is built here, exactly as
    /// it was written, and put to the same test the real generator passes: it
    /// fails, and the failure is printed, so a reader who doubts the story can
    /// see it. If a future change to `Rng` ever makes this test's expectation
    /// wrong, the module documentation is wrong too and should be corrected
    /// with it.
    #[test]
    fn the_original_defect_still_cycles_when_reproduced() {
        /// The generator as it was copy-pasted into twenty-seven crates: a
        /// power-of-two-modulus LCG, returned raw, reduced with `%`.
        fn old_next_bounded(state: &mut u64, bound: u64) -> u64 {
            *state = state.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
            *state % bound
        }

        let mut state = 0xDEAD_BEEF_CAFE;
        let draws: [u64; 32] = core::array::from_fn(|_| old_next_bounded(&mut state, 4));
        let steps: [u64; 31] = core::array::from_fn(|i| (draws[i + 1] + 4 - draws[i]) % 4);
        assert!(
            steps.iter().all(|s| *s == steps[0]),
            "the historical defect no longer reproduces; the module docs need \
             revisiting. Draws: {draws:?}"
        );

        // And the same bound through the real generator does not.
        let mut rng = Rng::new(0xDEAD_BEEF_CAFE);
        let fixed: [usize; 32] = core::array::from_fn(|_| rng.below(4));
        let fixed_steps: [usize; 31] = core::array::from_fn(|i| (fixed[i + 1] + 4 - fixed[i]) % 4);
        assert!(
            fixed_steps.iter().any(|s| *s != fixed_steps[0]),
            "the replacement cycles too: {fixed:?}"
        );
    }

    /// The raw output must be usable bit by bit, so that a caller who reduces
    /// it themselves is not punished for it.
    ///
    /// This is what the output permutation buys. Without it bit 0 alternates,
    /// bit 1 has period 4, and so on up the word.
    ///
    /// Sixty-four draws rather than the eight this test first used. Eight is
    /// not enough to distinguish a broken bit from a lucky one: a fair bit is
    /// constant across eight draws about one time in 128, and with 64 bits to
    /// check that is a coin-flip's chance of failing on correct code. It did.
    /// At 64 draws the same accident has probability 2^-63.
    #[test]
    fn every_bit_of_the_raw_output_varies() {
        let mut rng = Rng::new(1);
        let draws: [u64; 64] = core::array::from_fn(|_| rng.next_u64());
        for bit in 0..64 {
            let ones = draws.iter().filter(|v| (*v >> bit) & 1 == 1).count();
            assert!(
                ones > 0 && ones < draws.len(),
                "bit {bit} is constant across {} draws",
                draws.len()
            );
            // The specific failure the permutation removes: a low bit that
            // simply alternates.
            let alternating = draws
                .windows(2)
                .all(|w| (w[0] >> bit) & 1 != (w[1] >> bit) & 1);
            assert!(!alternating, "bit {bit} alternates; it is a counter");
        }
    }

    #[test]
    fn below_stays_inside_its_bound_and_survives_zero() {
        let mut rng = Rng::new(7);
        for bound in [1usize, 2, 3, 5, 7, 64, 1000, usize::MAX] {
            for _ in 0..500 {
                assert!(rng.below(bound) < bound);
            }
        }
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.below_u64(0), 0);
    }

    #[test]
    fn between_is_inclusive_at_both_ends_and_reaches_them() {
        let mut rng = Rng::new(99);
        let mut saw_min = false;
        let mut saw_max = false;
        for _ in 0..2000 {
            let v = rng.between(-3, 3);
            assert!((-3..=3).contains(&v), "{v} escaped -3..=3");
            saw_min |= v == -3;
            saw_max |= v == 3;
        }
        assert!(saw_min && saw_max, "an end of the range was never reached");
        // A degenerate range answers with its only value rather than panicking.
        assert_eq!(rng.between(5, 5), 5);
        // A backwards range answers with `min` rather than looping or wrapping.
        assert_eq!(rng.between(5, 1), 5);
        // The full width must not overflow the `max - min` subtraction, and
        // must still be a draw rather than a constant. Every `i64` is inside
        // `i64::MIN..=i64::MAX`, so the only thing worth asserting is that the
        // answers vary.
        let widest: [i64; 8] = core::array::from_fn(|_| rng.between(i64::MIN, i64::MAX));
        assert!(
            widest.windows(2).any(|w| w[0] != w[1]),
            "the widest range returned a constant: {widest:?}"
        );
    }

    #[test]
    fn chance_honours_its_fraction_and_its_edges() {
        let mut rng = Rng::new(4);
        assert!(!rng.chance(0, 10), "zero out of ten must never be true");
        assert!(rng.chance(10, 10), "ten out of ten must always be true");
        assert!(
            rng.chance(99, 10),
            "a numerator past the denominator is certain"
        );
        assert!(
            !rng.chance(1, 0),
            "a zero denominator must not divide by zero"
        );
        let hits = (0..10_000).filter(|_| rng.chance(1, 4)).count();
        assert!(
            (2000..3000).contains(&hits),
            "one in four came up {hits} times in ten thousand"
        );
    }

    #[test]
    fn flip_is_a_fair_coin_and_not_an_alternating_one() {
        let mut rng = Rng::new(11);
        let flips: [bool; 1000] = core::array::from_fn(|_| rng.flip());
        let heads = flips.iter().filter(|f| **f).count();
        assert!((400..600).contains(&heads), "{heads} heads in 1000");
        assert!(
            flips.windows(2).any(|w| w[0] == w[1]),
            "the coin never came up the same way twice running; it is a counter"
        );
    }

    #[test]
    fn unit_f32_never_reaches_one() {
        let mut rng = Rng::new(13);
        let mut max_seen = 0.0f32;
        for _ in 0..20_000 {
            let v = rng.unit_f32();
            assert!((0.0..1.0).contains(&v), "{v} escaped 0.0..1.0");
            max_seen = max_seen.max(v);
        }
        // The reason the range is half-open: `(v * len) as usize` must not
        // reach `len`.
        let scaled = (max_seen * 10.0) as usize;
        assert!(
            scaled < 10,
            "a unit draw scaled to a length of ten reached {scaled}"
        );
    }

    #[test]
    fn between_f32_covers_its_range_and_refuses_a_bad_one() {
        let mut rng = Rng::new(17);
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for _ in 0..5000 {
            let v = rng.between_f32(-2.0, 6.0);
            assert!((-2.0..6.0).contains(&v));
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(
            lo < -1.5 && hi > 5.5,
            "the range was not covered: {lo}..{hi}"
        );
        assert_eq!(rng.between_f32(3.0, 3.0), 3.0, "an empty range yields min");
        assert_eq!(
            rng.between_f32(3.0, 1.0),
            3.0,
            "a backwards range yields min"
        );
        assert_eq!(rng.between_f32(1.0, f32::NAN), 1.0, "a NaN end yields min");
    }

    #[test]
    fn choose_declines_an_empty_slice() {
        let mut rng = Rng::new(19);
        let empty: [u8; 0] = [];
        assert!(rng.choose(&empty).is_none());
        let items = [10, 20, 30];
        for _ in 0..100 {
            assert!(items.contains(rng.choose(&items).unwrap()));
        }
    }

    /// A shuffle must permute, and must permute *uniformly*.
    ///
    /// The uniformity half is the one worth testing: the common wrong
    /// Fisher-Yates draws from the whole slice each step, which still produces
    /// a permutation every time and still looks shuffled, but favours some
    /// orderings over others. Counting how often each of the six orderings of
    /// three elements comes up catches that; checking "it is still a
    /// permutation" does not.
    #[test]
    fn shuffle_is_a_uniform_permutation() {
        let mut rng = Rng::new(23);
        let mut counts = [0usize; 6];
        for _ in 0..60_000 {
            let mut items = [0u8, 1, 2];
            rng.shuffle(&mut items);
            let index = match items {
                [0, 1, 2] => 0,
                [0, 2, 1] => 1,
                [1, 0, 2] => 2,
                [1, 2, 0] => 3,
                [2, 0, 1] => 4,
                [2, 1, 0] => 5,
                _ => panic!("{items:?} is not a permutation of 0,1,2"),
            };
            counts[index] += 1;
        }
        for (ordering, count) in counts.iter().enumerate() {
            assert!(
                (9000..11_000).contains(count),
                "ordering {ordering} came up {count} times in 60000; \
                 an even split is 10000 each. Counts: {counts:?}"
            );
        }
    }

    #[test]
    fn shuffle_handles_the_degenerate_lengths() {
        let mut rng = Rng::new(29);
        let mut empty: [u8; 0] = [];
        rng.shuffle(&mut empty);
        let mut one = [7u8];
        rng.shuffle(&mut one);
        assert_eq!(one, [7]);
    }

    #[test]
    fn the_same_seed_gives_the_same_stream() {
        let mut a = Rng::new(0x5EED);
        let mut b = Rng::new(0x5EED);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    /// Two seeds must give two games, not the same game at a different phase.
    #[test]
    fn different_seeds_give_different_streams() {
        let a: [usize; 32] = {
            let mut r = Rng::new(1);
            core::array::from_fn(|_| r.below(4))
        };
        let b: [usize; 32] = {
            let mut r = Rng::new(2);
            core::array::from_fn(|_| r.below(4))
        };
        assert_ne!(a, b);
        for shift in 1..4 {
            let rotated: [usize; 32] = core::array::from_fn(|i| (a[i] + shift) % 4);
            assert_ne!(rotated, b, "one stream is the other rotated by {shift}");
        }
    }

    /// Seed zero must not be a fixed point.
    ///
    /// It is for a multiply-only LCG, which is why the increment exists; worth
    /// pinning because zero is the seed a caller reaches for when they have
    /// nothing better, and a generator that returns the same value for ever is
    /// the worst possible response to it.
    #[test]
    fn seed_zero_still_generates() {
        let mut rng = Rng::new(0);
        let draws: [u64; 8] = core::array::from_fn(|_| rng.next_u64());
        assert!(
            draws.windows(2).any(|w| w[0] != w[1]),
            "seed zero produced a constant stream"
        );
    }
}
