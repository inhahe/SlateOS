//! A seeded pseudo-random generator, for the things a UI shuffles and scatters.
//!
//! Six places had written their own: the screen saver (three times over — once
//! as a method and twice inlined into a loop to dodge a borrow), the wallpaper
//! shuffler, the paint program's spatter brush, the network scanner, the
//! password generator and the card game. They had already drifted: two
//! different xorshift shift-triples, one linear congruential generator, three
//! different answers to "what if the seed is zero", and every one of them
//! reduced into a range with a bare `%`, which is both biased and the thing
//! `clippy::arithmetic_side_effects` keeps pointing at.
//!
//! # This is not a source of secrets
//!
//! `SeededRng` is fast, small and completely predictable: two generators built
//! from the same seed produce the same sequence forever, and a few outputs are
//! enough to recover the state and predict the rest. That is exactly what a
//! screen saver or a shuffled wallpaper list wants, and exactly what a
//! password, a key, a token or a nonce must never come from. For those, take
//! bytes from the kernel CSPRNG — see [`RandomSource`], which both this
//! generator and a real entropy source implement, so code that needs the real
//! thing can say so in its type.

use core::num::NonZeroU64;

/// Somewhere to draw uniformly-distributed bits from.
///
/// Written as a trait so that code which *must* have real entropy can demand a
/// particular implementor, while code that only wants variety can take any —
/// and so that a caller which needs a real source in production can still be
/// tested against a seeded one without either of them knowing about the other.
pub trait RandomSource {
    /// Draw 64 uniformly-distributed bits.
    fn next_u64(&mut self) -> u64;

    /// Draw 32 uniformly-distributed bits.
    fn next_u32(&mut self) -> u32 {
        // The high bits of a xorshift generator are better mixed than the low
        // ones, so take those.
        (self.next_u64() >> 32) as u32
    }

    /// A uniformly-distributed integer in `0..bound`, or `0` if `bound` is `0`.
    ///
    /// Unbiased. The obvious `next_u64() % bound` is not: unless `bound`
    /// divides 2^64 exactly, the low residues occur once more often than the
    /// high ones. Draws landing in the short final block are thrown away
    /// instead, which costs an occasional extra draw and nothing else.
    fn below(&mut self, bound: u64) -> u64 {
        let Some(bound) = NonZeroU64::new(bound) else {
            return 0;
        };
        // The largest multiple of `bound` that fits in a u64. Everything at or
        // above it is part of a partial block and is redrawn.
        let limit = u64::MAX.saturating_sub(u64::MAX % bound);
        loop {
            let value = self.next_u64();
            if value < limit {
                return value % bound;
            }
        }
    }

    /// A uniformly-distributed index into a collection of `len` items, or `0`
    /// if there are none.
    fn below_usize(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).unwrap_or(0)
    }

    /// A uniformly-distributed integer in `low..=high`, or `low` if the range
    /// is empty or reversed.
    fn in_range(&mut self, low: u64, high: u64) -> u64 {
        let Some(span) = high.checked_sub(low).and_then(|s| s.checked_add(1)) else {
            return low;
        };
        low.saturating_add(self.below(span))
    }

    /// A uniformly-distributed `f32` in `0.0..1.0`.
    ///
    /// Built from 24 bits, which is every bit an `f32` mantissa can hold, so
    /// the result is exact and never reaches `1.0` — several of the copies this
    /// replaces divided by `u32::MAX` and so *could* return exactly `1.0`,
    /// which a caller scaling by a length then turned into an index one past
    /// the end.
    fn next_f32(&mut self) -> f32 {
        const MANTISSA_BITS: u32 = 24;
        const SCALE: f32 = 1.0 / (1u32 << MANTISSA_BITS) as f32;
        (self.next_u32() >> (32 - MANTISSA_BITS)) as f32 * SCALE
    }

    /// A uniformly-distributed `f32` in `low..high`, or `low` if the range is
    /// empty, reversed, or involves a NaN.
    ///
    /// The comparison goes through `partial_cmp` rather than `<` so that the
    /// NaN case is answered deliberately instead of falling out of whichever
    /// way the comparison happened to be written: every ordering that is not
    /// strictly "low before high" — equal, reversed, or incomparable — yields
    /// `low`, which is a value inside the requested range whenever there is
    /// one at all.
    fn f32_in_range(&mut self, low: f32, high: f32) -> f32 {
        match low.partial_cmp(&high) {
            Some(core::cmp::Ordering::Less) => low + (high - low) * self.next_f32(),
            _ => low,
        }
    }

    /// One of `items`, or `None` if there are none.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        items.get(self.below_usize(items.len()))
    }

    /// Shuffle `items` into a uniformly-random order.
    ///
    /// Fisher-Yates, walking down from the end: every permutation is equally
    /// likely, which the "swap each element with a random other one" version
    /// two of the replaced copies used is not.
    fn shuffle<T>(&mut self, items: &mut [T]) {
        let mut remaining = items.len();
        while remaining > 1 {
            let last = remaining.saturating_sub(1);
            let chosen = self.below_usize(remaining);
            items.swap(last, chosen);
            remaining = last;
        }
    }
}

/// A seeded xorshift64 generator: same seed, same sequence, every time.
///
/// See the module docs — this is for variety, never for secrets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeededRng {
    state: NonZeroU64,
}

impl SeededRng {
    /// The state a zero seed is replaced by.
    ///
    /// A xorshift generator with all-zero state produces zeros forever, so the
    /// seed cannot be zero. Each of the copies this replaces substituted a
    /// different constant, or — in one case — did not substitute at all.
    const ZERO_SEED_REPLACEMENT: NonZeroU64 = NonZeroU64::new(0x9E37_79B9_7F4A_7C15).unwrap();

    /// A generator that will produce the sequence belonging to `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            state: match NonZeroU64::new(seed) {
                Some(state) => state,
                None => Self::ZERO_SEED_REPLACEMENT,
            },
        }
    }

    /// The generator's current state, which is also the seed that would
    /// reproduce the rest of its sequence.
    #[must_use]
    pub const fn state(&self) -> u64 {
        self.state.get()
    }
}

impl RandomSource for SeededRng {
    fn next_u64(&mut self) -> u64 {
        // Marsaglia's xorshift64 with the (13, 7, 17) triple. The state can
        // never reach zero from a non-zero start — the transformation is a
        // bijection on the non-zero words — so the `NonZeroU64` is preserved
        // and the sequence has full period.
        let mut x = self.state.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = NonZeroU64::new(x).unwrap_or(Self::ZERO_SEED_REPLACEMENT);
        x
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // The two float assertions below check the *degenerate* cases of
    // `f32_in_range`, where the documented answer is the `low` bound handed
    // straight back rather than a computed one. That is an exact equality by
    // construction, so an approximate comparison would weaken the assertion
    // rather than make it robust.
    #![allow(clippy::float_cmp)]

    use super::{RandomSource, SeededRng};

    #[test]
    fn the_same_seed_gives_the_same_sequence() {
        let mut a = SeededRng::new(42);
        let mut b = SeededRng::new(42);
        let mut c = SeededRng::new(43);

        let from_a: Vec<u64> = (0..16).map(|_| a.next_u64()).collect();
        let from_b: Vec<u64> = (0..16).map(|_| b.next_u64()).collect();
        let from_c: Vec<u64> = (0..16).map(|_| c.next_u64()).collect();

        assert_eq!(from_a, from_b);
        assert_ne!(from_a, from_c);
    }

    /// A xorshift generator seeded with zero emits zeros forever, so a zero
    /// seed has to become something else.
    #[test]
    fn a_zero_seed_still_generates() {
        let mut rng = SeededRng::new(0);
        let drawn: Vec<u64> = (0..8).map(|_| rng.next_u64()).collect();
        assert!(drawn.iter().all(|v| *v != 0), "{drawn:?}");
        assert!(drawn.windows(2).any(|pair| pair[0] != pair[1]));
    }

    /// The state must never fall to zero, or the generator dies silently part
    /// way through a run rather than at the start.
    #[test]
    fn the_state_never_reaches_zero() {
        let mut rng = SeededRng::new(1);
        for _ in 0..100_000 {
            rng.next_u64();
            assert_ne!(rng.state(), 0);
        }
    }

    #[test]
    fn an_index_stays_inside_the_collection() {
        let mut rng = SeededRng::new(7);
        for len in 1..12_usize {
            for _ in 0..200 {
                assert!(rng.below_usize(len) < len, "len={len}");
            }
        }
    }

    /// An empty range has no index to give, and must say so rather than
    /// dividing by zero.
    #[test]
    fn an_empty_range_yields_zero() {
        let mut rng = SeededRng::new(7);
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.below_usize(0), 0);
        assert_eq!(rng.pick::<u8>(&[]), None);
        assert_eq!(rng.in_range(5, 4), 5);
        assert_eq!(rng.f32_in_range(1.0, 1.0), 1.0);
    }

    #[test]
    fn every_index_below_the_bound_comes_up() {
        let mut rng = SeededRng::new(99);
        let bound = 6_usize;
        let mut seen = [false; 6];
        for _ in 0..2000 {
            seen[rng.below_usize(bound)] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    /// The bound is not a divisor of 2^64, so a plain `%` would favour the low
    /// residues. Rejection sampling must not: over enough draws every residue
    /// should come up about equally often.
    #[test]
    fn the_reduction_into_a_range_is_not_visibly_biased() {
        let mut rng = SeededRng::new(2024);
        let bound = 7_usize;
        let draws = 70_000;
        let mut counts = [0_u32; 7];
        for _ in 0..draws {
            counts[rng.below_usize(bound)] += 1;
        }
        let expected = draws / bound as u32;
        for (residue, count) in counts.iter().enumerate() {
            let drift = count.abs_diff(expected);
            assert!(
                drift * 20 < expected,
                "residue {residue} came up {count} times, expected about {expected}"
            );
        }
    }

    #[test]
    fn a_float_stays_in_the_unit_interval() {
        let mut rng = SeededRng::new(5);
        for _ in 0..10_000 {
            let value = rng.next_f32();
            assert!((0.0..1.0).contains(&value), "{value}");
        }
    }

    /// Scaling a float that could reach 1.0 by a length gives an index one past
    /// the end, which is how this bit gets used.
    #[test]
    fn a_scaled_float_never_reaches_the_length_it_scales() {
        let mut rng = SeededRng::new(11);
        let len = 8.0_f32;
        for _ in 0..10_000 {
            assert!(rng.next_f32() * len < len);
        }
    }

    #[test]
    fn a_float_range_is_respected() {
        let mut rng = SeededRng::new(13);
        for _ in 0..5000 {
            let value = rng.f32_in_range(-2.5, 4.0);
            assert!((-2.5..4.0).contains(&value), "{value}");
        }
        assert_eq!(rng.f32_in_range(3.0, 1.0), 3.0, "a reversed range is empty");
    }

    #[test]
    fn shuffling_keeps_every_item_exactly_once() {
        let mut rng = SeededRng::new(17);
        for len in 0..12_usize {
            let mut items: Vec<usize> = (0..len).collect();
            rng.shuffle(&mut items);
            let mut sorted = items.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..len).collect::<Vec<_>>(), "len={len}");
        }
    }

    /// Fisher-Yates gives every permutation equal probability; "swap each
    /// element with a random other one" does not. Three items have six
    /// orderings, and all six must show up at roughly the same rate.
    #[test]
    fn every_permutation_of_a_short_list_comes_up_about_equally_often() {
        let mut rng = SeededRng::new(23);
        let runs = 60_000;
        let mut counts = std::collections::BTreeMap::new();
        for _ in 0..runs {
            let mut items = [0_u8, 1, 2];
            rng.shuffle(&mut items);
            *counts.entry(items).or_insert(0_u32) += 1;
        }
        assert_eq!(counts.len(), 6, "every ordering must occur");
        let expected = runs / 6;
        for (order, count) in &counts {
            assert!(
                count.abs_diff(expected) * 10 < expected,
                "{order:?} came up {count} times, expected about {expected}"
            );
        }
    }

    #[test]
    fn picking_returns_an_item_that_is_in_the_list() {
        let mut rng = SeededRng::new(31);
        let items = ['a', 'b', 'c', 'd'];
        for _ in 0..500 {
            let picked = *rng.pick(&items).unwrap();
            assert!(items.contains(&picked));
        }
    }
}
