//! One source of random numbers: a seeded generator for variety, the kernel
//! CSPRNG for secrets, and a single vocabulary shared by both.
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
//! ## Fix 1 — reduce with the high bits, and reject the skewed tail
//!
//! [`RandomSource::below_u64`] multiplies by the bound and keeps the **top**
//! half of the 128-bit product, which reads the high bits and never the low
//! ones (Lemire, *Fast Random Integer Generation in an Interval*, 2019).
//!
//! For comparison, `x % bound` is biased as well as low-bit-bound: with
//! 2<sup>64</sup> outputs folded into `bound` buckets, the first
//! `2^64 mod bound` buckets get one extra value each. Negligible for a bound
//! of four; not negligible for a bound near 2<sup>64</sup>, and free to avoid.
//!
//! The multiply alone leaves a residual bias of the same shape but around
//! 2<sup>-64</sup> of it — invisible for a deck of cards. But this is now also
//! the reduction a *password* draws its characters through, and "invisible" is
//! not a word to leave in a secret's derivation, so Lemire's rejection step is
//! kept: when the low half of the product falls in the short final block the
//! draw is thrown away and retried. That makes the result exactly uniform, and
//! it engages about `bound / 2^64` of the time — never, in practice.
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
//! # Two implementations, one vocabulary
//!
//! [`RandomSource`] is the vocabulary — `below`, `between`, `shuffle`,
//! `choose` and the rest are written once, against a single required method.
//! Two types provide that method:
//!
//! | Type | Bits come from | Use it for |
//! |---|---|---|
//! | [`SeededRng`] | a formula and a seed | boards, decks, screen savers, anything a test must replay |
//! | [`SystemRandom`] | the kernel CSPRNG | passwords, keys, tokens, nonces, salts |
//!
//! Writing them as one trait is what lets a routine that *must* have real
//! entropy say so in its signature (`fn derive(rng: &mut SystemRandom)`) while
//! still being exercised by a test that needs reproducible values — the test
//! drives the same algorithm from a [`SeededRng`], and neither type has to know
//! the other exists.
//!
//! **[`SeededRng`] is not cryptographic.** Its state is 64 bits and recoverable
//! from a couple of outputs; two generators built from the same seed agree
//! forever. That is exactly what a shuffled deck wants and exactly what a
//! password must never come from.
//!
//! ## This crate was itself duplicated
//!
//! Worth recording, because the lesson is the crate's own: for a while there
//! were *two* of these. `randrange` grew from the games (a seeded generator and
//! sound reductions, no entropy source); `guitk::rng` grew separately from the
//! desktop (a seeded generator, a `RandomSource` trait, and [`SystemRandom`]).
//! Both were written to end the hand-rolled copies, and between them they used
//! the same word — `below` — for two different things: an index in one and a
//! `u64` bound in the other.
//!
//! They are merged here, into the crate that was already `no_std` and
//! dependency-free, because that is the only one a *headless* component can
//! reach: the credential service needed the kernel entropy source and could not
//! take a dependency on a GUI toolkit to get it. `guitk::rng` is now a
//! re-export of this crate. The names follow the games, which had twice the
//! call sites; the trait and the entropy source follow the desktop, which had
//! the only implementation of either.
//!
//! **Not stream-compatible with the copies it replaces.** Any test that pinned
//! a specific board layout will produce a different — and, for the first time,
//! actually varied — layout. That is the point of the change, not a
//! regression to be worked around by preserving the old stream.

#![no_std]

use core::num::NonZeroU64;

/// Somewhere to draw uniformly-distributed bits from.
///
/// Implementors supply [`next_u64`](Self::next_u64); everything else is built
/// on it here, once, so that a bounded draw or a shuffle is correct wherever
/// the bits came from. See the module documentation for the two implementors
/// and which of them a given job needs.
pub trait RandomSource {
    /// The next raw 64-bit output. Every bit of it is equally good; see the
    /// module documentation for why that took an extra step.
    fn next_u64(&mut self) -> u64;

    /// Draw 32 uniformly-distributed bits.
    fn next_u32(&mut self) -> u32 {
        // The high half is the better-mixed one for any generator, permuted
        // output or not; taking it costs nothing and is the habit worth
        // keeping.
        (self.next_u64() >> 32) as u32
    }

    /// A value in `0..bound`, or `0` when `bound` is zero.
    ///
    /// Zero is answered rather than refused because the natural caller is
    /// `rng.below(items.len())`, and an empty collection is an ordinary state
    /// for that to be in — while `% 0` divides by zero and takes the process
    /// with it. The returned `0` is then an index the caller's own `get` will
    /// decline, which is where the emptiness gets handled properly.
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.below_u64(bound as u64)).unwrap_or(0)
    }

    /// A `u64` in `0..bound`, or `0` when `bound` is zero. Exactly uniform.
    ///
    /// Lemire's method with its rejection step: the draw is multiplied by the
    /// bound into 128 bits and the top half taken, which uses the high bits
    /// and never the low ones. The low half of the same product says which
    /// slice of the input range the answer came from, and draws landing in the
    /// short final slice — the one that would make some answers commoner than
    /// others — are thrown away and retried. See the module documentation.
    fn below_u64(&mut self, bound: u64) -> u64 {
        let Some(divisor) = NonZeroU64::new(bound) else {
            return 0;
        };
        let wide = u128::from(bound);
        // `wrapping_mul` cannot actually wrap — two 64-bit values multiply
        // into 128 bits exactly — it is there to say so rather than to depend
        // on it.
        let mut product = u128::from(self.next_u64()).wrapping_mul(wide);
        let mut remainder = product as u64;
        if remainder < bound {
            // `2^64 mod bound`, computed as `(-bound) mod bound` so that no
            // 128-bit division is needed. Only draws below this are in the
            // partial slice, and the test is skipped entirely when `remainder`
            // is already at or above `bound` — which is all but `bound / 2^64`
            // of the time.
            let threshold = bound.wrapping_neg() % divisor;
            while remainder < threshold {
                product = u128::from(self.next_u64()).wrapping_mul(wide);
                remainder = product as u64;
            }
        }
        (product >> 64) as u64
    }

    /// A value in `min..=max`, **inclusive at both ends**, or `min` when
    /// `max < min`.
    ///
    /// Inclusive because the callers are all of the form "a row of the board"
    /// or "a card of the deck", where the last one is a legal answer and an
    /// exclusive bound invites the `- 1` that gets forgotten.
    fn between(&mut self, min: i64, max: i64) -> i64 {
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

    /// `true` with the given `probability`, a fraction in `0.0..=1.0`.
    ///
    /// The two certainties are answered without drawing, so they are exactly
    /// certain: a probability of `1.0` is always `true` and `0.0` always
    /// `false`, however coarse the underlying draw is. Written as a bare
    /// `draw() < probability` they would not be — [`unit_f32`](Self::unit_f32)
    /// never reaches `1.0`, so `< 1.0` is merely very likely, and callers that
    /// build a probability by arithmetic can land just outside the range.
    /// Anything outside `0.0..=1.0`, NaN included, is clamped to the nearer
    /// certainty.
    fn chance(&mut self, probability: f32) -> bool {
        if probability >= 1.0 {
            return true;
        }
        // `!(probability > 0.0)` would read better but trips a lint; going
        // through `partial_cmp` makes the NaN answer — `false`, no chance —
        // deliberate rather than a side effect of comparison order.
        match probability.partial_cmp(&0.0) {
            Some(core::cmp::Ordering::Greater) => self.unit_f32() < probability,
            _ => false,
        }
    }

    /// `true` with probability `numerator / denominator`, exactly.
    ///
    /// The integer counterpart of [`chance`](Self::chance), for the "one in
    /// twenty" shape where the fraction is known exactly and rounding it
    /// through an `f32` would be a needless approximation.
    ///
    /// A denominator of zero yields `false` — "none out of nothing" — rather
    /// than dividing by zero. A numerator at or above the denominator yields
    /// `true` always, which is what the fraction says.
    fn chance_in(&mut self, numerator: u64, denominator: u64) -> bool {
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
    fn flip(&mut self) -> bool {
        self.next_u64() >> 63 != 0
    }

    /// A `f32` in `0.0..1.0`.
    ///
    /// Built from the top 24 bits — the width of an `f32`'s significand — so
    /// every value it can return is evenly spaced and exactly representable.
    /// Dividing a full 64-bit draw by `u64::MAX` instead would round, and
    /// rounding up at the top of the range yields exactly `1.0`, which callers
    /// scaling by a length then turn into an out-of-bounds index.
    fn unit_f32(&mut self) -> f32 {
        const SIGNIFICAND_BITS: u32 = 24;
        let bits = (self.next_u64() >> (64 - SIGNIFICAND_BITS)) as f32;
        bits / (1u32 << SIGNIFICAND_BITS) as f32
    }

    /// A `f32` in `min..max`, or `min` when the range is empty or not finite.
    fn between_f32(&mut self, min: f32, max: f32) -> f32 {
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
    fn choose<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
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
    fn shuffle<T>(&mut self, items: &mut [T]) {
        let mut i = items.len();
        while i > 1 {
            i = i.saturating_sub(1);
            let j = self.below(i.saturating_add(1));
            items.swap(i, j);
        }
    }
}

// ---------------------------------------------------------------------------
// The seeded generator
// ---------------------------------------------------------------------------

/// A seedable, non-cryptographic pseudo-random source.
///
/// Deterministic for a given seed, which is what makes a game reproducible in a
/// test and a bug report reproducible at all — and what makes this the wrong
/// place to draw a secret from. See the module documentation.
///
/// Deliberately not `Copy`: a generator that duplicates itself on an
/// accidental move-out is one that can hand the same "random" sequence to two
/// callers who each believe they have their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeededRng {
    state: u64,
}

/// The LCG multiplier, from Knuth via *Numerical Recipes*. Kept from the
/// hand-written copies this crate replaces so the lineage is recognisable.
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// The LCG increment. Must be odd for the generator to reach every state; this
/// one is.
const INCREMENT: u64 = 1_442_695_040_888_963_407;

impl SeededRng {
    /// Create a generator from a seed.
    ///
    /// Every seed is valid, including zero: the increment is what stops the
    /// all-zero state being a fixed point, and it is non-zero. That is not
    /// free of charge in general — a xorshift generator seeded with zero emits
    /// zeros for ever, and three separate hand-rolled copies in this tree were
    /// reachable at exactly that seed before they were absorbed here.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl RandomSource for SeededRng {
    fn next_u64(&mut self) -> u64 {
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
}

// ---------------------------------------------------------------------------
// The system entropy source
// ---------------------------------------------------------------------------

/// Why the system entropy source could not be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EntropyError {
    /// The kernel CSPRNG did not answer, or answered with fewer bytes than
    /// were asked for. There is no second-best here — a short read is a
    /// failure, not a smaller helping of randomness.
    Unavailable,
}

impl core::fmt::Display for EntropyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the system random number generator is unavailable"),
        }
    }
}

impl core::error::Error for EntropyError {}

/// Bytes from the kernel CSPRNG — the source a secret must come from.
///
/// # Failure is reported, never papered over
///
/// [`RandomSource::next_u64`] has no way to return an error, and this type
/// will not invent one out of a fallback generator: that is precisely the bug
/// this type exists to remove. Instead a failure is made *sticky and visible*.
/// [`open`](Self::open) proves the kernel answers before handing back a
/// generator at all, and if a later refill fails, [`is_healthy`] turns false
/// permanently and every subsequent draw is zero — a value that is obviously
/// not random rather than one that merely looks it.
///
/// **Code generating a secret must check [`is_healthy`] after drawing**, not
/// only before, and discard the result if it is false. A draw that begins on a
/// healthy generator can still cross a failed refill part-way through.
/// [`try_next_u64`](Self::try_next_u64) is there for callers that would rather
/// see the error at the point it happens.
///
/// [`is_healthy`]: Self::is_healthy
#[derive(Debug)]
pub struct SystemRandom {
    /// Bytes drawn from the kernel; those before `used` have been handed out
    /// and are zeroed.
    buffer: [u8; Self::BUFFER_BYTES],
    /// How much of `buffer` has been consumed.
    used: usize,
    /// False from the first failed refill onwards, and never true again.
    healthy: bool,
}

impl SystemRandom {
    /// How many bytes to take from the kernel at a time.
    ///
    /// A multiple of eight, so a `u64` never straddles the end of the buffer,
    /// and large enough that a whole password costs one syscall rather than
    /// one per character.
    const BUFFER_BYTES: usize = 256;

    /// Open the system entropy source, failing if the kernel does not answer.
    ///
    /// The first buffer is drawn here rather than lazily, so that a caller
    /// which cannot proceed without real entropy finds out at the point it
    /// asks for the generator instead of half-way through building a secret.
    ///
    /// # Errors
    ///
    /// [`EntropyError::Unavailable`] if the kernel CSPRNG cannot be read —
    /// which is also the case on a host build, where there is no Slate kernel
    /// to ask.
    pub fn open() -> Result<Self, EntropyError> {
        let mut source = Self {
            buffer: [0; Self::BUFFER_BYTES],
            used: Self::BUFFER_BYTES,
            healthy: true,
        };
        source.refill()?;
        Ok(source)
    }

    /// Whether every byte handed out so far came from the kernel CSPRNG.
    ///
    /// Once false, always false.
    #[must_use]
    pub const fn is_healthy(&self) -> bool {
        self.healthy
    }

    /// Draw 64 bits, reporting a failed refill instead of hiding it.
    ///
    /// # Errors
    ///
    /// [`EntropyError::Unavailable`] if the kernel CSPRNG cannot be read, or
    /// if it has already failed once for this generator.
    pub fn try_next_u64(&mut self) -> Result<u64, EntropyError> {
        if !self.healthy {
            return Err(EntropyError::Unavailable);
        }
        const WORD: usize = size_of::<u64>();
        if self.used.saturating_add(WORD) > Self::BUFFER_BYTES {
            self.refill()?;
        }
        let start = self.used;
        let end = start.saturating_add(WORD);
        let mut word = [0u8; WORD];
        let Some(bytes) = self.buffer.get_mut(start..end) else {
            // Unreachable while `BUFFER_BYTES` is a multiple of eight, but a
            // silently-wrong secret is not a thing to leave to a comment.
            self.healthy = false;
            return Err(EntropyError::Unavailable);
        };
        word.copy_from_slice(bytes);
        // Don't leave spent entropy lying in the buffer: these bytes are, in
        // this type's intended use, the password itself.
        bytes.fill(0);
        self.used = end;
        Ok(u64::from_le_bytes(word))
    }

    /// Take a fresh buffer from the kernel, poisoning the generator if that
    /// fails.
    fn refill(&mut self) -> Result<(), EntropyError> {
        match fill_from_kernel(&mut self.buffer) {
            Ok(()) => {
                self.used = 0;
                Ok(())
            }
            Err(err) => {
                self.healthy = false;
                self.buffer.fill(0);
                self.used = Self::BUFFER_BYTES;
                Err(err)
            }
        }
    }
}

impl RandomSource for SystemRandom {
    fn next_u64(&mut self) -> u64 {
        // A failure cannot be returned through this signature, so it is
        // recorded instead: `is_healthy` is now false and stays false, and the
        // value handed back is zero — plainly not random, rather than
        // something that passes for it. See the type documentation.
        self.try_next_u64().unwrap_or(0)
    }
}

/// Fill `buffer` from the kernel CSPRNG.
///
/// Goes through the posix `getrandom` symbol, which the libc layer routes to
/// `SYS_GETRANDOM`, because no `std` API exposes the kernel CSPRNG. This is
/// the same route `userspace/ssh-keygen` takes for key material.
#[cfg(unix)]
fn fill_from_kernel(buffer: &mut [u8]) -> Result<(), EntropyError> {
    unsafe extern "C" {
        /// Fill `buf` with `buflen` random bytes; returns bytes written or -1.
        fn getrandom(buf: *mut u8, buflen: usize, flags: u32) -> isize;
    }

    // SAFETY: `buffer` is a uniquely-borrowed slice, and the pointer and
    // length handed over are exactly its own, so `getrandom` writes only
    // within it. It returns the number of bytes written, or -1 on failure.
    let written = unsafe { getrandom(buffer.as_mut_ptr(), buffer.len(), 0) };
    if usize::try_from(written).is_ok_and(|count| count == buffer.len()) {
        Ok(())
    } else {
        Err(EntropyError::Unavailable)
    }
}

/// The host build has no Slate kernel to ask.
///
/// Refusing is deliberate rather than unfortunate: a test that wants
/// reproducible values must name [`SeededRng`], and a test that reaches for
/// the system source on the host must see it decline — which is what makes
/// "fails closed when there is no entropy" a testable property.
#[cfg(not(unix))]
fn fill_from_kernel(_buffer: &mut [u8]) -> Result<(), EntropyError> {
    Err(EntropyError::Unavailable)
}

// ---------------------------------------------------------------------------
// Drawing something that has to be unguessable
// ---------------------------------------------------------------------------

/// A source fit to draw a secret from, and the rule for drawing one.
///
/// # Why this is a trait and not just a method on [`SystemRandom`]
///
/// Three crates independently wrote the same wrapper: `apps/passwordgen`'s
/// `AppRandom::secret`, `apps/credmanager`'s `CredRandom::secret`, and the
/// credential service's password generator. Each was an enum over "the kernel",
/// "a named sequence, tests only" and "the kernel said no", and each carried
/// its own copy of the *reasoning* below. A rule about secrets that is written
/// out once per crate is a rule that will be written out slightly wrong.
///
/// The enum itself has to stay in the crate that owns it, because its seeded
/// variant is `#[cfg(test)]` and `cfg(test)` does not cross a crate boundary —
/// a variant defined here as "tests only" would be reachable from every caller
/// in production, which is the property the enums exist to prevent. So the
/// *variants* stay local and the *rule* moves here, where it is stated once and
/// tested once.
///
/// # The rule
///
/// A secret is only worth drawing if the source is trustworthy **before and
/// after** the draw. [`SystemRandom`] refills from the kernel in the middle of
/// a long draw, and that refill can fail; a password whose second half is
/// zeroes is not a password, and no user can tell it from one by looking.
/// Checking on the way out is what turns a partial failure into a refusal.
///
/// # Implementing
///
/// Supply [`is_trustworthy`](Self::is_trustworthy). Do not override
/// [`secret`](Self::secret) — the whole point is that there is one copy of it.
pub trait SecretSource: RandomSource {
    /// Whether a value drawn from this source may be handed to a user as a
    /// secret.
    ///
    /// Must be *latching* for a fallible source: once false, never true again.
    /// A source that could recover would let a draw straddling the failure pass
    /// both checks.
    fn is_trustworthy(&self) -> bool;

    /// Run `make` only if the source is trustworthy, and keep its result only
    /// if the source is *still* trustworthy afterwards.
    ///
    /// Returns `None` if either check fails — which callers must render as a
    /// refusal, never as a reason to fall back to a [`SeededRng`]. A fallback
    /// is how the original defect survives the fix: the user is handed a
    /// password that looks exactly as good as a real one.
    fn secret<T>(&mut self, make: impl FnOnce(&mut Self) -> T) -> Option<T>
    where
        Self: Sized,
    {
        if !self.is_trustworthy() {
            return None;
        }
        let value = make(self);
        self.is_trustworthy().then_some(value)
    }
}

impl SecretSource for SystemRandom {
    fn is_trustworthy(&self) -> bool {
        self.is_healthy()
    }
}

// ---------------------------------------------------------------------------
// Drawing something that only has to be *different*
// ---------------------------------------------------------------------------

/// A fast generator seeded from the kernel, falling back to `fallback` when the
/// kernel cannot be reached.
///
/// # Choosing between this and [`SecretSource::secret`]
///
/// These are the two halves of one rule, and the choice between them is the
/// whole design. Ask what an adversary gains by predicting the value:
///
/// | If predicting it costs the user… | Use | On no entropy |
/// |---|---|---|
/// | their secrets — a password, a key, a salt, a nonce | [`SystemRandom`] + [`SecretSource::secret`] | **refuse** |
/// | only novelty — a maze layout, a shuffle, a screensaver | this function | fall back |
///
/// A game that refused to start because the entropy pool was empty would be a
/// worse failure than a game whose first board repeats. A password generator
/// that quietly produced a guessable password would be a worse failure than one
/// that refuses. The asymmetry is not about how much randomness each needs —
/// both want the same quality — it is about which failure the user can survive.
///
/// # Why the fallback seed is a parameter and not a constant
///
/// So that the call site has to name it, and in naming it, decide. A crate that
/// writes `seeded_from_system(SOME_SEED)` has stated on the record that it can
/// live with a repeat; a crate that cannot must not be able to reach that
/// behaviour by accident. Distinct constants per crate also keep two programs
/// that both lose entropy on the same boot from producing correlated streams,
/// which a shared default would guarantee.
///
/// # Why the fallback is not the clock
///
/// It could be, and for these callers it would even be a small improvement. It
/// is not, because a clock-seeded generator *looks* unpredictable — the values
/// differ every run — while carrying perhaps twenty bits of real entropy. That
/// appearance is exactly what let the original defect survive review in three
/// password generators. A visibly fixed seed cannot be mistaken for entropy by
/// the next reader, and this function's whole contract is that the caller
/// already decided predictability is acceptable.
///
/// # Examples
///
/// ```
/// use randrange::{seeded_from_system, RandomSource};
///
/// // "MAZE" in ASCII: any constant will do, but a memorable one documents
/// // itself in a stack trace.
/// let mut rng = seeded_from_system(0x4D41_5A45);
/// let wall = rng.below(4);
/// assert!(wall < 4);
/// ```
#[must_use]
pub fn seeded_from_system(fallback: u64) -> SeededRng {
    match SystemRandom::open() {
        Ok(mut kernel) => SeededRng::new(kernel.next_u64()),
        // The kernel is out of reach: a fixed board beats no board at all.
        Err(_) => SeededRng::new(fallback),
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
    /// defences here — the output permutation in [`SeededRng::next_u64`] and
    /// the high-bit reduction in [`RandomSource::below`] — and either one
    /// alone is enough to make this pass. Verified by reverting each: with the
    /// permutation present, restoring `% bound` still passes this test (it
    /// fails `below_stays_inside_its_bound_and_survives_zero`, on the division
    /// by zero, which is `%`'s *other* problem). What pins the premise of the
    /// whole crate is
    /// [`the_original_defect_still_cycles_when_reproduced`], which builds the
    /// old code inside the test and watches it fail the same property.
    #[test]
    fn a_power_of_two_bound_does_not_produce_a_cycle() {
        for bound in [2usize, 4, 8, 16, 32, 64] {
            let mut rng = SeededRng::new(0xDEAD_BEEF_CAFE);
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
    /// see it. If a future change to `SeededRng` ever makes this test's
    /// expectation wrong, the module documentation is wrong too and should be
    /// corrected with it.
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
        let mut rng = SeededRng::new(0xDEAD_BEEF_CAFE);
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
        let mut rng = SeededRng::new(1);
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
        let mut rng = SeededRng::new(7);
        for bound in [1usize, 2, 3, 5, 7, 64, 1000, usize::MAX] {
            for _ in 0..500 {
                assert!(rng.below(bound) < bound);
            }
        }
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.below_u64(0), 0);
    }

    /// The rejection step must keep the answer in range *and* terminate, on
    /// the bounds where it actually engages.
    ///
    /// A bound near 2^63 is where "throw the short final slice away" stops
    /// being theoretical: with `bound = 2^63 + 1` roughly half of all draws
    /// land in the partial slice and are redrawn. A wrongly-computed threshold
    /// loops for ever there, so a test that returns at all is most of the
    /// assertion.
    #[test]
    fn the_rejection_step_terminates_on_the_bounds_that_trigger_it() {
        let mut rng = SeededRng::new(0xFEED_FACE);
        for bound in [u64::MAX, (1u64 << 63) + 1, (1u64 << 63) - 1, 3] {
            for _ in 0..1000 {
                assert!(rng.below_u64(bound) < bound, "bound {bound}");
            }
        }
    }

    /// Uniform means uniform on the unfriendly bounds too.
    ///
    /// Three is the smallest bound that does not divide 2^64, so it is where a
    /// `%` reduction's bias would show first if the sample were large enough.
    /// The rejection step removes it exactly; this checks the distribution is
    /// at least not visibly lopsided.
    #[test]
    fn a_bound_that_does_not_divide_the_word_is_still_even() {
        let mut rng = SeededRng::new(2024);
        let draws = 90_000_u32;
        let mut counts = [0_u32; 3];
        for _ in 0..draws {
            counts[rng.below(3)] += 1;
        }
        let expected = draws / 3;
        for (residue, count) in counts.iter().enumerate() {
            assert!(
                count.abs_diff(expected) * 20 < expected,
                "residue {residue} came up {count} times, expected about {expected}"
            );
        }
    }

    #[test]
    fn between_is_inclusive_at_both_ends_and_reaches_them() {
        let mut rng = SeededRng::new(99);
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
    fn chance_in_honours_its_fraction_and_its_edges() {
        let mut rng = SeededRng::new(4);
        assert!(!rng.chance_in(0, 10), "zero out of ten must never be true");
        assert!(rng.chance_in(10, 10), "ten out of ten must always be true");
        assert!(
            rng.chance_in(99, 10),
            "a numerator past the denominator is certain"
        );
        assert!(
            !rng.chance_in(1, 0),
            "a zero denominator must not divide by zero"
        );
        let hits = (0..10_000).filter(|_| rng.chance_in(1, 4)).count();
        assert!(
            (2000..3000).contains(&hits),
            "one in four came up {hits} times in ten thousand"
        );
    }

    #[test]
    fn the_two_certainties_are_certain() {
        // A bare `draw() < probability` gets neither end right: `unit_f32`
        // never reaches 1.0, so `< 1.0` is merely near-certain, and a
        // probability that arithmetic pushed slightly outside the range would
        // be answered by the draw rather than by the bound.
        let mut rng = SeededRng::new(1);
        for _ in 0..1000 {
            assert!(rng.chance(1.0), "a certainty must not depend on the draw");
            assert!(!rng.chance(0.0), "an impossibility must never happen");
            assert!(rng.chance(1.5), "above certain is still certain");
            assert!(!rng.chance(-0.5), "below impossible is still impossible");
            assert!(!rng.chance(f32::NAN), "no probability is no chance");
        }
    }

    #[test]
    fn a_chance_comes_up_about_as_often_as_it_says() {
        // Wide bounds: this is checking that the probability is used at all
        // and the right way round, not measuring the generator's quality.
        const DRAWS: u32 = 20_000;
        let mut rng = SeededRng::new(0xC0FF_EE00_1234_5678);
        for (probability, low, high) in [
            (0.1_f32, 0.07_f64, 0.13_f64),
            (0.25, 0.21, 0.29),
            (0.5, 0.46, 0.54),
            (0.9, 0.87, 0.93),
        ] {
            let hits = (0..DRAWS).filter(|_| rng.chance(probability)).count();
            let rate = hits as f64 / f64::from(DRAWS);
            assert!(
                (low..high).contains(&rate),
                "a {probability} chance came up {rate} of the time"
            );
        }
    }

    #[test]
    fn flip_is_a_fair_coin_and_not_an_alternating_one() {
        let mut rng = SeededRng::new(11);
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
        let mut rng = SeededRng::new(13);
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
        let mut rng = SeededRng::new(17);
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

    /// Scaling a unit draw by a length must not produce that length, because
    /// that is how the value gets used: as an index.
    #[test]
    fn a_scaled_unit_draw_never_reaches_the_length_it_scales() {
        let mut rng = SeededRng::new(11);
        let len = 8.0_f32;
        for _ in 0..10_000 {
            assert!(rng.unit_f32() * len < len);
        }
    }

    #[test]
    fn choose_declines_an_empty_slice() {
        let mut rng = SeededRng::new(19);
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
        let mut rng = SeededRng::new(23);
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
        let mut rng = SeededRng::new(29);
        let mut empty: [u8; 0] = [];
        rng.shuffle(&mut empty);
        let mut one = [7u8];
        rng.shuffle(&mut one);
        assert_eq!(one, [7]);
    }

    /// Whatever the length, a shuffle must still hold each item exactly once.
    #[test]
    fn shuffling_keeps_every_item_exactly_once() {
        let mut rng = SeededRng::new(17);
        for len in 0..12_usize {
            let mut items: [usize; 12] = core::array::from_fn(|index| index);
            let list = &mut items[..len];
            rng.shuffle(list);
            list.sort_unstable();
            for (index, value) in list.iter().enumerate() {
                assert_eq!(*value, index, "len={len}");
            }
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_stream() {
        let mut a = SeededRng::new(0x5EED);
        let mut b = SeededRng::new(0x5EED);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    /// Two seeds must give two games, not the same game at a different phase.
    #[test]
    fn different_seeds_give_different_streams() {
        let a: [usize; 32] = {
            let mut r = SeededRng::new(1);
            core::array::from_fn(|_| r.below(4))
        };
        let b: [usize; 32] = {
            let mut r = SeededRng::new(2);
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
        let mut rng = SeededRng::new(0);
        let draws: [u64; 8] = core::array::from_fn(|_| rng.next_u64());
        assert!(
            draws.windows(2).any(|w| w[0] != w[1]),
            "seed zero produced a constant stream"
        );
    }

    #[test]
    fn the_upper_half_of_a_draw_is_not_a_constant() {
        let mut rng = SeededRng::new(0x1234_5678);
        let draws: [u32; 32] = core::array::from_fn(|_| rng.next_u32());
        assert!(draws.windows(2).any(|w| w[0] != w[1]));
    }

    /// The whole point of [`SystemRandom`]: no entropy means no generator,
    /// rather than a generator quietly backed by something weaker.
    ///
    /// On the host there is no Slate kernel, so this is the failing path; on
    /// the target it is the succeeding one. Both are asserted, because the
    /// property under test is that the two answers are the *only* two — a
    /// source that is handed back at all is one that answered.
    #[test]
    fn the_system_source_either_answers_or_refuses_to_exist() {
        match SystemRandom::open() {
            Ok(mut source) => {
                assert!(source.is_healthy());
                for _ in 0..1000 {
                    let _ = source.next_u64();
                }
                assert!(
                    source.is_healthy(),
                    "a source that opened must stay healthy across a refill"
                );
            }
            Err(err) => {
                assert_eq!(err, EntropyError::Unavailable);
                #[cfg(unix)]
                panic!("the kernel CSPRNG must answer on the target");
            }
        }
    }

    /// A caller that ignores the error and draws anyway must get something it
    /// cannot mistake for randomness.
    #[test]
    fn a_source_that_never_opened_hands_out_nothing_that_looks_random() {
        // Reachable only where opening fails; on the target the branch above
        // covers the other half.
        if SystemRandom::open().is_ok() {
            return;
        }
        assert!(matches!(
            fill_from_kernel(&mut [0u8; 8]),
            Err(EntropyError::Unavailable)
        ));
    }

    #[test]
    fn the_entropy_error_says_what_went_wrong() {
        // `to_string` lives in `alloc`, which this crate deliberately does not
        // depend on, so the message is rendered into a fixed buffer instead.
        struct Sink {
            bytes: [u8; 128],
            len: usize,
        }
        impl core::fmt::Write for Sink {
            fn write_str(&mut self, text: &str) -> core::fmt::Result {
                for byte in text.as_bytes() {
                    let Some(slot) = self.bytes.get_mut(self.len) else {
                        return Err(core::fmt::Error);
                    };
                    *slot = *byte;
                    self.len += 1;
                }
                Ok(())
            }
        }

        let mut sink = Sink {
            bytes: [0; 128],
            len: 0,
        };
        core::fmt::Write::write_fmt(&mut sink, format_args!("{}", EntropyError::Unavailable))
            .unwrap();
        let message = core::str::from_utf8(&sink.bytes[..sink.len]).unwrap();
        assert!(
            message.contains("system random number generator"),
            "{message}"
        );
    }

    // -- SecretSource ------------------------------------------------------

    /// A source that goes bad after `good_draws`, the way a [`SystemRandom`]
    /// does when the kernel declines a refill part-way through a long draw.
    struct FailsAfter {
        inner: SeededRng,
        good_draws: usize,
        healthy: bool,
    }

    impl RandomSource for FailsAfter {
        fn next_u64(&mut self) -> u64 {
            if self.good_draws == 0 {
                self.healthy = false;
                return 0;
            }
            self.good_draws -= 1;
            self.inner.next_u64()
        }
    }

    impl SecretSource for FailsAfter {
        fn is_trustworthy(&self) -> bool {
            self.healthy
        }
    }

    /// The check on the way *out* is the one that is easy to leave out, and the
    /// one that matters: a source healthy at the start and broken by the end
    /// produced a password whose tail was zeroes, indistinguishable to the user
    /// from a real one.
    #[test]
    fn a_source_that_fails_part_way_through_a_draw_yields_no_secret() {
        let mut source = FailsAfter {
            inner: SeededRng::new(7),
            good_draws: 4,
            healthy: true,
        };
        let drawn = source.secret(|rng| {
            let mut out = [0u64; 8];
            for slot in &mut out {
                *slot = rng.next_u64();
            }
            out
        });
        assert!(drawn.is_none(), "a half-random secret must be refused");
    }

    #[test]
    fn a_healthy_source_yields_its_secret() {
        let mut source = FailsAfter {
            inner: SeededRng::new(7),
            good_draws: 64,
            healthy: true,
        };
        let drawn = source.secret(|rng| rng.next_u64());
        assert!(drawn.is_some());
    }

    /// Refusing before the draw, not merely after it, is what stops `make`
    /// running at all on a dead source -- so a caller cannot observe a
    /// zero-filled buffer even by capturing it out of the closure.
    #[test]
    fn a_dead_source_never_runs_the_draw() {
        let mut source = FailsAfter {
            inner: SeededRng::new(7),
            good_draws: 0,
            healthy: false,
        };
        let mut ran = false;
        let drawn = source.secret(|_| {
            ran = true;
        });
        assert!(drawn.is_none());
        assert!(!ran, "the draw must not run on a source known to be bad");
    }

    /// The host build has no kernel to ask, so this is the shape every caller
    /// on the host sees: opening fails, and nothing is drawn.
    #[test]
    fn the_system_source_on_a_host_build_refuses_rather_than_inventing() {
        #[cfg(not(unix))]
        {
            assert!(SystemRandom::open().is_err());
        }
    }

    // ── seeded_from_system ─────────────────────────────────────────────────

    /// The point of the function: it always yields a working generator, on any
    /// host, whether or not a kernel answered. This is the property a game
    /// needs and a password generator must not have.
    #[test]
    fn a_session_generator_always_exists_and_generates() {
        let mut rng = seeded_from_system(0x1234_5678_9ABC_DEF0);
        let mut seen = [false; 8];
        for _ in 0..400 {
            seen[rng.below(8)] = true;
        }
        assert!(
            seen.iter().all(|&hit| hit),
            "every value in the bound should appear over 400 draws"
        );
    }

    /// On a host with no kernel the fallback is taken verbatim, so the caller
    /// gets exactly the stream its constant names — which is what makes the
    /// degraded mode reproducible instead of merely broken.
    #[test]
    #[cfg(not(unix))]
    fn without_a_kernel_the_fallback_seed_is_used_exactly() {
        const SEED: u64 = 0x0BAD_C0DE_0BAD_C0DE;
        let mut fallen_back = seeded_from_system(SEED);
        let mut named = SeededRng::new(SEED);
        for _ in 0..8 {
            assert_eq!(fallen_back.next_u64(), named.next_u64());
        }
    }

    /// Two crates that pick different constants do not produce the same stream
    /// when they both lose entropy on the same boot. This is the reason the
    /// seed is a parameter rather than a shared default.
    #[test]
    #[cfg(not(unix))]
    fn two_fallback_constants_do_not_collide() {
        let mut a = seeded_from_system(0x4D41_5A45);
        let mut b = seeded_from_system(0x5449_4C45);
        assert_ne!(a.next_u64(), b.next_u64());
    }
}
