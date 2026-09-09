//! Non-uniform distributions over a uniform source.
//!
//! [`randrange`] answers "a number, uniformly, in this range". This crate
//! answers "a number, shaped like *this*" — normal, exponential, Poisson —
//! by transforming that uniform stream. It holds no entropy of its own and
//! opens no device; every sample here is a function of bits somebody else
//! drew.
//!
//! # Why this is a separate crate, and not an option on the syscall
//!
//! The operator's decision of 2026-09-07 (alongside `design-decisions.md`
//! §1007): **the entropy syscall stays uniform-bytes-only.** That is the
//! security boundary. A `normal`/`poisson` option on the syscall itself would
//! sit next to the one call a caller reaches for when it needs a key, and
//! would invite someone to draw a secret from a non-uniform source — a key
//! sampled from a bell curve has most of its probability mass in a narrow
//! band, which is precisely the property a key must not have.
//!
//! Keeping the shapes one layer up makes the mistake harder to make and
//! trivial to see in review: anything drawing from this crate is *simulating*,
//! not securing. It mirrors the split Rust's ecosystem settled on between
//! `rand` and `rand_distr`, for the same reason.
//!
//! # Determinism
//!
//! Nothing here holds state, so reproducibility is entirely the source's.
//! [`randrange::SeededRng`] with a fixed seed gives the same sequence of
//! samples on every run, which is what a simulation's regression test needs;
//! [`randrange::SystemRandom`] gives one that cannot be replayed. The
//! distribution does not know or care which it was handed.
//!
//! ```
//! use randdist::{Distribution, Exponential};
//! use randrange::SeededRng;
//!
//! let arrivals = Exponential::new(2.0).expect("a positive rate");
//! let mut rng = SeededRng::new(12_345);
//! let first: f64 = arrivals.sample(&mut rng);
//!
//! let mut again = SeededRng::new(12_345);
//! assert_eq!(first, arrivals.sample(&mut again));
//! ```
//!
//! # What is not here
//!
//! No `Bernoulli`: [`randrange::RandomSource::chance`] already answers "true
//! with probability p", and a second spelling of it would be a second thing to
//! keep correct.

// Float arithmetic, densely. `clippy::arithmetic_side_effects` exists to catch
// integer overflow and division by zero, neither of which an `f64` can do — it
// saturates to an infinity or produces a NaN, and every function below either
// cannot reach one (the parameter validation in each constructor rules out the
// inputs that could) or is documented where it can. The integer arithmetic in
// this crate is confined to Poisson's counter, which uses `saturating_add`.
#![allow(clippy::arithmetic_side_effects)]

use randrange::RandomSource;

/// A `f64` in `0.0..1.0`, uniformly, using all 53 bits an `f64` can hold.
///
/// [`randrange`] offers [`unit_f32`](randrange::RandomSource::unit_f32) but no
/// `f64` equivalent, because nothing needed one until distributions did. The
/// difference matters here rather than in a game: an `f32` unit draw has 2^24
/// distinct values, so an exponential built on it can only ever produce 2^24
/// distinct waiting times and its tail is a staircase. 53 bits puts the
/// quantisation below anything a simulation will notice.
///
/// The top 53 bits are taken, never the low ones, for the reason
/// [`randrange`] documents at length: on a generator whose low bits are weaker
/// than its high ones, a reduction that reads the low end inherits the
/// weakness. `2^-53` scaling then lands the result in `[0, 1)` exactly — the
/// largest representable output is `1.0 - 2^-53`, which is strictly below one,
/// so a caller taking `ln(1.0 - u)` can never be handed `ln(0)`.
#[must_use]
pub fn unit_f64<R: RandomSource + ?Sized>(src: &mut R) -> f64 {
    // 53 = f64::MANTISSA_DIGITS. Shifting right by 11 keeps the top 53.
    #[allow(clippy::cast_precision_loss)]
    let bits = (src.next_u64() >> 11) as f64;
    bits * (1.0 / 9_007_199_254_740_992.0) // 2^-53
}

/// Why a distribution could not be constructed.
///
/// Constructors validate and return this rather than panicking, and rather
/// than silently clamping: a caller that asks for a normal with a negative
/// standard deviation has a bug in the line above, and clamping to zero would
/// hand it a constant that looks like data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistError {
    /// A parameter was NaN or infinite.
    NotFinite,
    /// A parameter that must be strictly positive was zero or negative.
    NotPositive,
    /// A parameter that must not be negative was negative.
    Negative,
}

impl core::fmt::Display for DistError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Self::NotFinite => "parameter is not finite",
            Self::NotPositive => "parameter must be greater than zero",
            Self::Negative => "parameter must not be negative",
        };
        f.write_str(text)
    }
}

impl std::error::Error for DistError {}

/// Something that can be sampled from a uniform source.
///
/// `&self` rather than `&mut self`: a distribution is its parameters and
/// nothing else, so two threads may share one, and the only mutable thing in a
/// draw is the source. That is also why [`Normal`] discards a value it could
/// have cached — see its documentation.
pub trait Distribution<T> {
    /// Draw one sample, consuming as many words from `src` as it needs.
    fn sample<R: RandomSource + ?Sized>(&self, src: &mut R) -> T;
}

// ---------------------------------------------------------------------------
// Normal
// ---------------------------------------------------------------------------

/// The normal (Gaussian) distribution, parameterised by mean and standard
/// deviation.
///
/// # Method
///
/// Marsaglia's polar form of Box–Muller. A point is drawn uniformly from the
/// square `(-1, 1)^2` and rejected unless it falls inside the unit circle;
/// the accepted point is then scaled by `sqrt(-2 ln s / s)`.
///
/// The polar form is chosen over the trigonometric one because it needs no
/// `sin`/`cos` — it gets the same rotation out of the rejected-until-inside
/// step, which is both faster and free of the accuracy loss those functions
/// have near their argument extremes. It costs an average of `4/π ≈ 1.27`
/// pairs of uniforms per accepted pair, which is the price of not calling a
/// transcendental twice.
///
/// **It generates two independent samples and returns one.** The other could
/// be cached, and `rand_distr` does cache it — but caching needs `&mut self`,
/// and a distribution that must be borrowed mutably to be read cannot be
/// shared. The trade is one extra pair of `u64` draws per sample against a
/// type that is `Sync` and can live in a `static`; for a source that produces
/// a `u64` in a few nanoseconds, sharing is worth more.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normal {
    mean: f64,
    std_dev: f64,
}

impl Normal {
    /// A normal distribution with the given mean and standard deviation.
    ///
    /// # Errors
    ///
    /// [`DistError::NotFinite`] if either parameter is NaN or infinite;
    /// [`DistError::Negative`] if `std_dev` is negative. A `std_dev` of
    /// exactly zero is **allowed** and yields the constant `mean` — that is
    /// the honest limit of the distribution, and simulations legitimately dial
    /// noise down to nothing to isolate a variable.
    pub fn new(mean: f64, std_dev: f64) -> Result<Self, DistError> {
        if !mean.is_finite() || !std_dev.is_finite() {
            return Err(DistError::NotFinite);
        }
        if std_dev < 0.0 {
            return Err(DistError::Negative);
        }
        Ok(Self { mean, std_dev })
    }

    /// The mean this was built with.
    #[must_use]
    pub const fn mean(&self) -> f64 {
        self.mean
    }

    /// The standard deviation this was built with.
    #[must_use]
    pub const fn std_dev(&self) -> f64 {
        self.std_dev
    }
}

impl Distribution<f64> for Normal {
    fn sample<R: RandomSource + ?Sized>(&self, src: &mut R) -> f64 {
        self.mean + self.std_dev * standard_normal(src)
    }
}

/// One draw from the standard normal (mean 0, standard deviation 1).
///
/// Split out because [`Normal`] is a scale-and-shift of it and because it is
/// the part worth testing on its own.
fn standard_normal<R: RandomSource + ?Sized>(src: &mut R) -> f64 {
    loop {
        // Map [0,1) onto (-1,1). The endpoints matter: `s == 0.0` would divide
        // by zero below, and it is reachable only when both coordinates are
        // exactly zero, which the `s > 0.0` test rejects along with everything
        // outside the circle.
        let u = 2.0 * unit_f64(src) - 1.0;
        let v = 2.0 * unit_f64(src) - 1.0;
        let s = u * u + v * v;
        if s > 0.0 && s < 1.0 {
            // `ln(s)` is finite and negative for `0 < s < 1`, so the argument
            // to `sqrt` is positive and the result is finite.
            return u * (-2.0 * s.ln() / s).sqrt();
        }
    }
}

// ---------------------------------------------------------------------------
// Exponential
// ---------------------------------------------------------------------------

/// The exponential distribution: the waiting time until the next event when
/// events arrive independently at a constant average `rate`.
///
/// # Method
///
/// Inverse transform: `-ln(1 - u) / rate` for `u` uniform in `[0, 1)`.
///
/// `1 - u` rather than `u` is the whole of the numerical care here. `u` can be
/// exactly `0.0`, and `ln(0)` is negative infinity — one sample in 2^53 would
/// be an infinite waiting time, which is the kind of defect that survives
/// every test and then stops a simulation months later. `1 - u` lies in
/// `(0, 1]`, so the logarithm is always finite, and the `u == 0` case yields
/// `ln(1) == 0` — a zero-length wait, which is the correct answer for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exponential {
    rate: f64,
}

impl Exponential {
    /// An exponential distribution with the given `rate` (often written λ),
    /// whose mean is `1 / rate`.
    ///
    /// # Errors
    ///
    /// [`DistError::NotFinite`] if `rate` is NaN or infinite;
    /// [`DistError::NotPositive`] if it is zero or negative. Zero is refused
    /// rather than treated as "never arrives": the mean would be infinite, and
    /// a caller that meant "no events" is better served by not sampling.
    pub fn new(rate: f64) -> Result<Self, DistError> {
        if !rate.is_finite() {
            return Err(DistError::NotFinite);
        }
        if rate <= 0.0 {
            return Err(DistError::NotPositive);
        }
        Ok(Self { rate })
    }

    /// The rate this was built with.
    #[must_use]
    pub const fn rate(&self) -> f64 {
        self.rate
    }
}

impl Distribution<f64> for Exponential {
    fn sample<R: RandomSource + ?Sized>(&self, src: &mut R) -> f64 {
        standard_exponential(src) / self.rate
    }
}

/// One draw from the exponential distribution with rate 1.
fn standard_exponential<R: RandomSource + ?Sized>(src: &mut R) -> f64 {
    -(1.0 - unit_f64(src)).ln()
}

// ---------------------------------------------------------------------------
// Poisson
// ---------------------------------------------------------------------------

/// The Poisson distribution: how many independent events occur in one unit of
/// time when they arrive at average rate `lambda`.
///
/// # Method, and its cost
///
/// Events are counted by summing exponential inter-arrival times until they
/// exceed one unit — which is the definition of the process, so the sampler is
/// the definition run forwards.
///
/// This is Knuth's algorithm expressed in logarithms rather than as a product
/// of uniforms, and the difference is not cosmetic. The product form multiplies
/// uniforms until the running product falls below `e^-lambda`, and **that
/// constant underflows to zero for `lambda` above about 745** — at which point
/// the loop can never terminate, because no positive product is below zero.
/// Summing logarithms has no such threshold: it is exact for every `lambda`
/// this type will accept.
///
/// The cost is `O(lambda)` draws, which is fine for the arrival counts a
/// simulation usually wants and poor for very large `lambda`. Constant-time
/// samplers exist (Hörmann's transformed rejection), and they need a
/// log-gamma; if a profile ever shows this loop mattering, that is the
/// replacement, and the tests below pin the behaviour it would have to
/// reproduce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Poisson {
    lambda: f64,
}

impl Poisson {
    /// A Poisson distribution with the given `lambda`, which is both its mean
    /// and its variance.
    ///
    /// # Errors
    ///
    /// [`DistError::NotFinite`] if `lambda` is NaN or infinite;
    /// [`DistError::NotPositive`] if it is zero or negative. Zero is refused
    /// for the same reason as [`Exponential`]'s: the distribution is then the
    /// constant `0`, and a caller that wanted no events should not be in a
    /// sampling loop at all.
    pub fn new(lambda: f64) -> Result<Self, DistError> {
        if !lambda.is_finite() {
            return Err(DistError::NotFinite);
        }
        if lambda <= 0.0 {
            return Err(DistError::NotPositive);
        }
        Ok(Self { lambda })
    }

    /// The lambda this was built with.
    #[must_use]
    pub const fn lambda(&self) -> f64 {
        self.lambda
    }
}

impl Distribution<u64> for Poisson {
    fn sample<R: RandomSource + ?Sized>(&self, src: &mut R) -> u64 {
        let mut elapsed = 0.0_f64;
        let mut count: u64 = 0;
        loop {
            elapsed += standard_exponential(src);
            if elapsed > self.lambda {
                return count;
            }
            // Saturating rather than wrapping: a count that wrapped to zero
            // would be indistinguishable from "no events", which is the one
            // answer a caller must be able to trust. Reaching u64::MAX needs a
            // lambda no `f64` can hold, so this is a statement of intent
            // rather than a live path.
            count = count.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that panics on bad data should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not. Same
    // block as `randrange`'s, for the same reason.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    // `float_cmp` is suppressed deliberately rather than worked around. The
    // determinism tests assert that two sources with the same seed produce
    // *bit-identical* samples, and an approximate comparison there would pass
    // for a sampler that was merely close -- which is the one thing a
    // reproducible simulation cannot tolerate. Where a tolerance is the right
    // instrument, the statistical tests below use one explicitly.
    #![allow(clippy::float_cmp)]

    use super::*;
    use randrange::SeededRng;

    /// Enough samples that the sample mean is stable to the tolerances below,
    /// and few enough that the suite stays fast.
    const N: usize = 100_000;

    fn rng() -> SeededRng {
        SeededRng::new(0x5EED_1234_ABCD_0001)
    }

    // -- unit_f64 -------------------------------------------------------

    #[test]
    fn unit_f64_stays_in_range() {
        let mut r = rng();
        for _ in 0..N {
            let u = unit_f64(&mut r);
            assert!((0.0..1.0).contains(&u), "{u} outside [0,1)");
        }
    }

    /// The upper bound is *strict*, and the reason is load-bearing:
    /// `Exponential` computes `ln(1 - u)`, so a `u` of exactly 1.0 would be
    /// `ln(0)` and an infinite sample.
    #[test]
    fn unit_f64_never_reaches_one() {
        let largest = 1.0 - f64::from(1u32) / 9_007_199_254_740_992.0;
        assert!(largest < 1.0);
        // The construction can produce at most (2^53 - 1) * 2^-53.
        let mut r = rng();
        let mut seen_max = 0.0_f64;
        for _ in 0..N {
            seen_max = seen_max.max(unit_f64(&mut r));
        }
        assert!(seen_max <= largest);
    }

    #[test]
    fn unit_f64_has_a_mean_near_a_half() {
        let mut r = rng();
        let sum: f64 = (0..N).map(|_| unit_f64(&mut r)).sum();
        #[allow(clippy::cast_precision_loss)]
        let mean = sum / N as f64;
        assert!((mean - 0.5).abs() < 0.01, "mean {mean}");
    }

    // -- determinism ----------------------------------------------------

    /// The same seed gives the same samples. This is the property a
    /// simulation's regression test rests on, and it belongs to the source
    /// rather than to the distribution — which is exactly why it is worth
    /// asserting through the distribution.
    #[test]
    fn a_seed_reproduces_every_distribution() {
        let normal = Normal::new(3.0, 2.0).expect("valid");
        let expo = Exponential::new(0.5).expect("valid");
        let poisson = Poisson::new(4.0).expect("valid");

        let (mut a, mut b) = (rng(), rng());
        for _ in 0..1_000 {
            assert_eq!(normal.sample(&mut a), normal.sample(&mut b));
            assert_eq!(expo.sample(&mut a), expo.sample(&mut b));
            assert_eq!(poisson.sample(&mut a), poisson.sample(&mut b));
        }
    }

    /// Different seeds do not. Guards against a sampler that ignores its
    /// source — which would pass every determinism test ever written.
    #[test]
    fn different_seeds_give_different_samples() {
        let normal = Normal::new(0.0, 1.0).expect("valid");
        let mut a = SeededRng::new(1);
        let mut b = SeededRng::new(2);
        let differ = (0..100).any(|_| normal.sample(&mut a) != normal.sample(&mut b));
        assert!(differ, "two seeds produced identical samples");
    }

    // -- Normal ---------------------------------------------------------

    #[test]
    fn normal_rejects_bad_parameters() {
        assert_eq!(Normal::new(f64::NAN, 1.0), Err(DistError::NotFinite));
        assert_eq!(Normal::new(0.0, f64::INFINITY), Err(DistError::NotFinite));
        assert_eq!(Normal::new(0.0, -1.0), Err(DistError::Negative));
    }

    /// A zero standard deviation is the constant `mean`, not an error.
    #[test]
    fn normal_with_no_spread_is_its_mean() {
        let d = Normal::new(7.5, 0.0).expect("zero spread is allowed");
        let mut r = rng();
        for _ in 0..100 {
            assert_eq!(d.sample(&mut r), 7.5);
        }
    }

    #[test]
    fn normal_matches_its_mean_and_spread() {
        let d = Normal::new(5.0, 2.0).expect("valid");
        let mut r = rng();
        let samples: Vec<f64> = (0..N).map(|_| d.sample(&mut r)).collect();

        #[allow(clippy::cast_precision_loss)]
        let n = N as f64;
        let mean = samples.iter().sum::<f64>() / n;
        let var = samples.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;

        assert!((mean - 5.0).abs() < 0.05, "mean {mean}");
        assert!((var.sqrt() - 2.0).abs() < 0.05, "std dev {}", var.sqrt());
    }

    /// Roughly 68% of a normal lies within one standard deviation, and 95%
    /// within two. A sampler that got the *shape* wrong while keeping the mean
    /// and variance right — a uniform of matching variance, say — fails here
    /// and passes the test above.
    #[test]
    fn normal_has_the_shape_and_not_merely_the_moments() {
        let d = Normal::new(0.0, 1.0).expect("valid");
        let mut r = rng();
        let (mut within1, mut within2) = (0_usize, 0_usize);
        for _ in 0..N {
            let x: f64 = d.sample(&mut r).abs();
            if x < 1.0 {
                within1 += 1;
            }
            if x < 2.0 {
                within2 += 1;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let n = N as f64;
        #[allow(clippy::cast_precision_loss)]
        let (f1, f2) = (within1 as f64 / n, within2 as f64 / n);
        assert!((f1 - 0.6827).abs() < 0.01, "within 1 sd: {f1}");
        assert!((f2 - 0.9545).abs() < 0.01, "within 2 sd: {f2}");
    }

    #[test]
    fn normal_samples_are_always_finite() {
        let d = Normal::new(0.0, 1.0).expect("valid");
        let mut r = rng();
        for _ in 0..N {
            let x: f64 = d.sample(&mut r);
            assert!(x.is_finite(), "{x}");
        }
    }

    // -- Exponential ----------------------------------------------------

    #[test]
    fn exponential_rejects_bad_parameters() {
        assert_eq!(Exponential::new(f64::NAN), Err(DistError::NotFinite));
        assert_eq!(Exponential::new(0.0), Err(DistError::NotPositive));
        assert_eq!(Exponential::new(-1.0), Err(DistError::NotPositive));
    }

    #[test]
    fn exponential_matches_its_mean() {
        for rate in [0.5_f64, 1.0, 3.0] {
            let d = Exponential::new(rate).expect("valid");
            let mut r = rng();
            let sum: f64 = (0..N).map(|_| d.sample(&mut r)).sum();
            #[allow(clippy::cast_precision_loss)]
            let mean = sum / N as f64;
            let want = 1.0 / rate;
            assert!(
                (mean - want).abs() < want * 0.02,
                "rate {rate}: mean {mean}"
            );
        }
    }

    /// Never negative, never infinite. The infinity guard is the point: it is
    /// what `1 - u` buys, and a regression to `ln(u)` shows up here.
    #[test]
    fn exponential_samples_are_finite_and_non_negative() {
        let d = Exponential::new(1.0).expect("valid");
        let mut r = rng();
        for _ in 0..N {
            let x: f64 = d.sample(&mut r);
            assert!(x.is_finite() && x >= 0.0, "{x}");
        }
    }

    /// The exponential is memoryless: `P(X > s + t | X > s) == P(X > t)`.
    /// Checked at one point, which is enough to catch a sampler that has the
    /// right mean and the wrong distribution.
    #[test]
    fn exponential_is_memoryless() {
        let d = Exponential::new(1.0).expect("valid");
        let mut r = rng();
        let samples: Vec<f64> = (0..N).map(|_| d.sample(&mut r)).collect();
        let beyond_1 = samples.iter().filter(|x| **x > 1.0).count();
        let beyond_2 = samples.iter().filter(|x| **x > 2.0).count();
        #[allow(clippy::cast_precision_loss)]
        let conditional = beyond_2 as f64 / beyond_1 as f64;
        // P(X > 1) = e^-1 ≈ 0.3679, and the conditional must match it.
        assert!(
            (conditional - core::f64::consts::E.recip()).abs() < 0.02,
            "conditional {conditional}"
        );
    }

    // -- Poisson --------------------------------------------------------

    #[test]
    fn poisson_rejects_bad_parameters() {
        assert_eq!(Poisson::new(f64::NAN), Err(DistError::NotFinite));
        assert_eq!(Poisson::new(0.0), Err(DistError::NotPositive));
        assert_eq!(Poisson::new(-2.0), Err(DistError::NotPositive));
    }

    /// Mean *and* variance both equal lambda — the property that identifies a
    /// Poisson rather than merely a non-negative integer distribution.
    #[test]
    fn poisson_mean_and_variance_both_match_lambda() {
        for lambda in [0.5_f64, 4.0, 20.0] {
            let d = Poisson::new(lambda).expect("valid");
            let mut r = rng();
            #[allow(clippy::cast_precision_loss)]
            let samples: Vec<f64> = (0..N).map(|_| d.sample(&mut r) as f64).collect();
            #[allow(clippy::cast_precision_loss)]
            let n = N as f64;
            let mean = samples.iter().sum::<f64>() / n;
            let var = samples.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
            assert!(
                (mean - lambda).abs() < lambda * 0.03,
                "lambda {lambda}: mean {mean}"
            );
            assert!(
                (var - lambda).abs() < lambda * 0.06,
                "lambda {lambda}: variance {var}"
            );
        }
    }

    /// A lambda past the point where `e^-lambda` underflows to zero.
    ///
    /// The product form of Knuth's algorithm cannot terminate here — it waits
    /// for a positive product to fall below zero. This is the regression test
    /// for choosing the logarithmic form, and it is a *liveness* test: if it
    /// fails, it hangs rather than reporting, which is the honest shape for
    /// the bug it guards.
    #[test]
    fn poisson_terminates_past_the_underflow_threshold() {
        assert_eq!(
            (-800.0_f64).exp(),
            0.0,
            "premise: e^-800 underflows, so a product form could not terminate"
        );
        let d = Poisson::new(800.0).expect("valid");
        let mut r = rng();
        let x = d.sample(&mut r);
        // Three standard deviations either side of the mean is sqrt(800) ≈ 28.3.
        assert!(
            (700..=900).contains(&x),
            "sample {x} implausible for lambda 800"
        );
    }

    #[test]
    fn poisson_with_a_small_lambda_often_yields_zero() {
        let d = Poisson::new(0.1).expect("valid");
        let mut r = rng();
        let zeros = (0..N).filter(|_| d.sample(&mut r) == 0).count();
        #[allow(clippy::cast_precision_loss)]
        let fraction = zeros as f64 / N as f64;
        // P(X = 0) = e^-0.1 ≈ 0.9048.
        assert!((fraction - 0.9048).abs() < 0.01, "fraction {fraction}");
    }

    // -- errors ---------------------------------------------------------

    #[test]
    fn errors_describe_themselves() {
        assert_eq!(DistError::NotFinite.to_string(), "parameter is not finite");
        assert_eq!(
            DistError::NotPositive.to_string(),
            "parameter must be greater than zero"
        );
        assert_eq!(
            DistError::Negative.to_string(),
            "parameter must not be negative"
        );
    }
}
