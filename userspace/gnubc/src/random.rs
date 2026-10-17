//! glibc's `random()`, for `random()` in bc: bc never seeds it, so its
//! numbers are glibc's from the default seed, 1 -- the additive feedback
//! generator `random_r` runs with a 31-word state (`TYPE_3`, the trinomial
//! `x**31 + x**3 + 1`), seeded by `srandom_r`'s Park-Miller sequence and
//! stirred 310 times before the first number.

#![allow(
    // A fixed 34-word window indexed by constants, and glibc's arithmetic,
    // which the seeding keeps within an `i64` (Schrage's method).
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

/// The generator's state: the 34 words its recurrence looks back over.
#[derive(Clone, Debug)]
pub struct Random {
    /// The last 34 values of `r[i] = r[i-31] + r[i-3]`, oldest first.
    r: [u32; 34],
}

impl Random {
    /// `srandom (1)`, as an unseeded program has it.
    #[must_use]
    pub fn new() -> Self {
        Self::seeded(1)
    }

    /// `srandom (seed)`.
    #[must_use]
    pub fn seeded(seed: u32) -> Self {
        let mut r = [0u32; 34];
        // `srandom_r`: a seed of 0 is taken as 1.
        let mut word = i64::from(if seed == 0 { 1 } else { seed });
        r[0] = u32::try_from(word).unwrap_or(1);
        for slot in r.iter_mut().take(31).skip(1) {
            // 16807 * word % 2147483647, computed without overflow as
            // glibc does (Schrage's method), negatives folded back.
            let hi = word / 127_773;
            let lo = word % 127_773;
            word = 16_807 * lo - 2_836 * hi;
            if word < 0 {
                word += 2_147_483_647;
            }
            *slot = u32::try_from(word).unwrap_or(0);
        }
        for i in 31..34 {
            r[i] = r[i - 31];
        }
        let mut g = Self { r };
        // The 310 values `srandom_r` discards: from i = 34 to i = 343.
        for _ in 34..344 {
            g.step();
        }
        g
    }

    /// The next value of the recurrence, moving the window on by one.
    fn step(&mut self) -> u32 {
        let next = self.r[34 - 31].wrapping_add(self.r[34 - 3]);
        self.r.copy_within(1.., 0);
        self.r[33] = next;
        next
    }

    /// `random ()`: the next number, 0 to 2^31 - 1.
    pub fn next(&mut self) -> i32 {
        // The top 31 bits.
        i32::try_from(self.step() >> 1).unwrap_or(0)
    }
}

impl Default for Random {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_numbers_are_glibcs() {
        let mut g = Random::new();
        let first: Vec<i32> = (0..5).map(|_| g.next()).collect();
        assert_eq!(
            first,
            [
                1_804_289_383,
                846_930_886,
                1_681_692_777,
                1_714_636_915,
                1_957_747_793
            ]
        );
    }
}
