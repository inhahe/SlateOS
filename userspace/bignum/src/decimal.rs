//! Fixed-point decimal numbers, exact to a stated number of fractional digits.
//!
//! A [`Decimal`] is a [`BigInt`] mantissa and a decimal `scale`: the value is
//! `digits * 10^-scale`. `1.25` is `125` with a scale of 2. Nothing here is
//! binary floating point, and that is the entire point — `0.1 + 0.2` is `0.3`,
//! exactly, because tenths are representable in base ten.
//!
//! ## Why this is in the crate and not in `bc`
//!
//! `bc` and `dc` are one calculator with two syntaxes. Historically `bc` was a
//! *preprocessor* that translated infix into RPN and piped it to `dc`; they are
//! documented together and specified together. Two implementations of their
//! shared number type is therefore not duplication in the ordinary sense — it
//! is two answers to a question that has one right answer, and the failure it
//! produces is the two programs disagreeing about `1/3` at the same scale.
//!
//! This type began as `bc`'s private `BcNum`. `dc` had no equivalent: it
//! computed in `f64` while its own documentation promised arbitrary precision,
//! so it was silently wrong above about 9e15. Lifting the type here is what
//! makes that fixable rather than reimplementable.
//!
//! ## Scale is an argument, not a property of the operation
//!
//! Addition and subtraction produce the larger of their operands' scales, which
//! is exact and needs no policy. Multiplication, division, powers and roots
//! *do* need one, because their exact answers may not be finite in base ten
//! (`1/3`), so they take the caller's working scale and truncate — never round
//! — to it. Truncation is `bc`'s documented behaviour, and rounding here would
//! make `scale=0; 1/2` answer `1` where POSIX says `0`.
//!
//! ## What a failure is, and what it is not
//!
//! [`Decimal::div`], [`Decimal::modulo`] and [`Decimal::sqrt`] return a
//! [`Result`]. The versions of these lifted from `bc` printed to stderr and
//! then returned **zero**, which is the worst available answer: a library type
//! writing a diagnostic it cannot phrase in the caller's terms, and then
//! handing back a plausible number the program keeps computing with. A division
//! by zero has to arrive at the interpreter as something it can refuse.

use crate::{BigInt, digit_to_char};

/// Why an operation could not produce a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalError {
    /// The divisor was zero, for `/` or `%`.
    DivideByZero,
    /// The square root of a negative number was asked for.
    NegativeSqrt,
}

impl core::fmt::Display for DecimalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The wording is the calculators' own, so a caller can print this
        // straight through without restating it and drifting from the other.
        f.write_str(match self {
            Self::DivideByZero => "divide by zero",
            Self::NegativeSqrt => "square root of a negative number",
        })
    }
}

/// A fixed-point decimal: `digits * 10^-scale`.
#[derive(Clone, Debug)]
pub struct Decimal {
    /// The unscaled integer. `1.25` stores `125` here.
    pub digits: BigInt,
    /// How many of `digits`' decimal places are fractional.
    pub scale: usize,
}

/// Ordering — and hence equality — is by *value*, so `1.5` and `1.50` are
/// equal and neither is less than the other.
///
/// None of this is derived. A derived implementation would compare the mantissa
/// and then the scale, so it would report `1.5 != 1.50` — two spellings of one
/// number — and would order `2` before `1.5` because `2` has the shorter
/// mantissa. Every caller that reaches for `==` or `<` means the number, not
/// the spelling; a caller that means the spelling can compare `scale` itself.
///
/// The order is total: there is no NaN here and no negative zero (see
/// [`Decimal::negate`]), which is what makes [`Ord`] available at all — a
/// binary float could only offer [`PartialOrd`].
impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Signs first, because it settles the answer without aligning the
        // scales — and aligning means growing the shorter mantissa by the
        // difference of the scales, which is the only allocation here.
        match (self.is_negative(), other.is_negative()) {
            (true, false) if !self.is_zero() || !other.is_zero() => {
                return core::cmp::Ordering::Less;
            }
            (false, true) if !self.is_zero() || !other.is_zero() => {
                return core::cmp::Ordering::Greater;
            }
            _ => {}
        }
        let s = self.scale.max(other.scale);
        let (a, b) = (self.rescale(s), other.rescale(s));
        let magnitude = a.digits.cmp_mag(&b.digits).cmp(&0);
        // Both are the same sign at this point, so a larger magnitude is the
        // larger number when positive and the smaller when negative.
        if a.digits.negative {
            magnitude.reverse()
        } else {
            magnitude
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == core::cmp::Ordering::Equal
    }
}

impl Eq for Decimal {}

impl Decimal {
    /// Zero, at scale 0.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            digits: BigInt::zero(),
            scale: 0,
        }
    }

    /// One, at scale 0.
    #[must_use]
    pub fn one() -> Self {
        Self {
            digits: BigInt::one(),
            scale: 0,
        }
    }

    /// An integer, at scale 0.
    #[must_use]
    pub fn from_i64(v: i64) -> Self {
        Self {
            digits: BigInt::from_i64(v),
            scale: 0,
        }
    }

    /// Whether the value is zero, at any scale.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.digits.is_zero()
    }

    /// Whether the value is strictly less than zero.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.digits.negative
    }

    /// The value with its sign flipped. Negating zero gives zero, not `-0`.
    #[must_use]
    pub fn negate(&self) -> Self {
        let mut r = self.clone();
        r.digits.negative = !r.digits.negative;
        if r.digits.is_zero() {
            r.digits.negative = false;
        }
        r
    }

    /// The value without its sign.
    #[must_use]
    pub fn abs(&self) -> Self {
        let mut r = self.clone();
        r.digits.negative = false;
        r
    }

    /// The same value at a different number of fractional digits.
    ///
    /// Growing the scale is exact. Shrinking it **truncates toward zero** — it
    /// does not round — because that is what `bc` specifies and what makes
    /// `scale=0; 1/2` answer `0`.
    #[must_use]
    pub fn rescale(&self, new_scale: usize) -> Self {
        if new_scale == self.scale {
            return self.clone();
        }
        if let Some(grow) = new_scale.checked_sub(self.scale) {
            return Self {
                digits: self.digits.shift_left_decimal(grow),
                scale: new_scale,
            };
        }
        let shrink = self.scale.saturating_sub(new_scale);
        // Discarding more digits than the mantissa has leaves zero, and saying
        // so here is not an optimisation: `ten_to_the(shrink)` would otherwise
        // try to allocate a number with `shrink` digits, and `shrink` is
        // attacker-reachable through `scale` (a `mul` of two large scales
        // saturates to `usize::MAX`). Every limb holds at most nine digits, so
        // `limbs.len() * 9` is an upper bound that needs no counting.
        if shrink >= self.digits.limbs.len().saturating_mul(crate::LIMB_DIGITS) {
            return Self {
                digits: BigInt::zero(),
                scale: new_scale,
            };
        }
        let divisor = ten_to_the(shrink);
        let (q, _) = self.digits.divmod(&divisor);
        Self {
            digits: q,
            scale: new_scale,
        }
    }

    /// Sum. The result carries the larger of the two scales, which is exact.
    #[must_use]
    pub fn add(&self, other: &Self) -> Self {
        let s = self.scale.max(other.scale);
        Self {
            digits: self.rescale(s).digits.add(&other.rescale(s).digits),
            scale: s,
        }
    }

    /// Difference. The result carries the larger of the two scales.
    #[must_use]
    pub fn sub(&self, other: &Self) -> Self {
        let s = self.scale.max(other.scale);
        Self {
            digits: self.rescale(s).digits.sub(&other.rescale(s).digits),
            scale: s,
        }
    }

    /// The exact product, keeping every digit: scale `a + b`.
    ///
    /// This is the primitive the other products are built from. It never
    /// truncates, so a caller that truncates afterwards loses digits only where
    /// it meant to.
    #[must_use]
    pub fn mul_exact(&self, other: &Self) -> Self {
        Self {
            digits: self.digits.mul(&other.digits),
            scale: self.scale.saturating_add(other.scale),
        }
    }

    /// Product, truncated to `result_scale`.
    ///
    /// The full product is formed first and only then truncated, so no
    /// intermediate digit is lost that could have affected a kept one.
    ///
    /// This is the *explicit-scale* product. It is **not** what `bc`'s and
    /// `dc`'s `*` operator does — see [`Decimal::multiply`], which applies
    /// POSIX's scale rule instead. Use this one only when the caller genuinely
    /// knows the scale it wants.
    #[must_use]
    pub fn mul(&self, other: &Self, result_scale: usize) -> Self {
        self.mul_exact(other).rescale(result_scale)
    }

    /// The scale POSIX gives a product: `min(a + b, max(scale, a, b))`.
    ///
    /// The rule is not the obvious "truncate to `scale`", and the difference is
    /// visible at the very first example anyone tries: with `scale = 0`,
    /// `1.5 * 1.5` is **2.2**, not `2`. A product keeps enough digits to be
    /// worth having even when the user has asked for no fractional digits at
    /// all, because `scale` governs *division*, where digits must be invented,
    /// rather than multiplication, where they are already there. The `min` then
    /// stops it keeping digits the operands never had.
    #[must_use]
    pub fn product_scale(&self, other: &Self, scale: usize) -> usize {
        let exact = self.scale.saturating_add(other.scale);
        exact.min(scale.max(self.scale).max(other.scale))
    }

    /// The `*` of `bc` and `dc`: the product at POSIX's scale for it.
    ///
    /// `scale` is the calculator's `scale` (`bc`) or `k` (`dc`) register, not
    /// the scale of the result; [`Decimal::product_scale`] derives that.
    #[must_use]
    pub fn multiply(&self, other: &Self, scale: usize) -> Self {
        self.mul_exact(other)
            .rescale(self.product_scale(other, scale))
    }

    /// Quotient, truncated to `result_scale`.
    ///
    /// # Errors
    ///
    /// [`DecimalError::DivideByZero`] if `other` is zero at any scale.
    pub fn div(&self, other: &Self, result_scale: usize) -> Result<Self, DecimalError> {
        if other.is_zero() {
            return Err(DecimalError::DivideByZero);
        }
        // Integer division discards the remainder, so the fractional digits we
        // want have to exist in the dividend *before* it happens. Scaling up by
        // the divisor's own scale as well cancels the divisor's denominator.
        let needed = result_scale.saturating_add(other.scale);
        let a = if needed > self.scale {
            self.rescale(needed)
        } else {
            self.clone()
        };
        let (q, _) = a.digits.divmod(&other.digits);
        let quotient = Self {
            digits: q,
            scale: a.scale.saturating_sub(other.scale),
        };
        Ok(quotient.rescale(result_scale))
    }

    /// Remainder, defined as `self - (self / other) * other` with the quotient
    /// truncated to `result_scale` — which is `bc`'s definition, not the one
    /// that assumes an integer quotient.
    ///
    /// The `q * other` step is *exact*. Truncating it, as an earlier version
    /// did, breaks the identity the definition exists to state: the remainder
    /// would no longer be what is left over, and `a % b` could come back
    /// larger than `b`.
    ///
    /// # Errors
    ///
    /// [`DecimalError::DivideByZero`] if `other` is zero.
    pub fn modulo(&self, other: &Self, result_scale: usize) -> Result<Self, DecimalError> {
        let q = self.div(other, result_scale)?;
        Ok(self.sub(&q.mul_exact(other)))
    }

    /// `self` raised to an integer power.
    ///
    /// A fractional exponent is truncated to an integer first, because that is
    /// all `^` is defined for in `bc` and `dc`; a real power belongs to the
    /// math library, which builds it from `exp` and `ln`.
    ///
    /// `scale` is the calculator's `scale`/`k` register. For a non-negative
    /// exponent the result's scale is POSIX's `min(scale(a) * b, max(scale,
    /// scale(a)))`, and for a negative one it is `scale`, since that case is a
    /// division.
    ///
    /// Every squaring is **exact**. Truncating them to the result scale, as an
    /// earlier version did, made `1.5 ^ 2` answer `2` at `scale = 0`: the first
    /// squaring threw away the `.25` before anything could ask for it. The cost
    /// is that a fractional base with a large exponent builds a genuinely large
    /// intermediate — but that is the size of the exact answer, and truncating
    /// early buys speed by being wrong.
    ///
    /// # Errors
    ///
    /// [`DecimalError::DivideByZero`] if the exponent is negative and `self` is
    /// zero, since that is `1/0`.
    pub fn pow(&self, exp: &Self, scale: usize) -> Result<Self, DecimalError> {
        let e = exp.rescale(0);
        if e.is_negative() {
            let magnitude = self.pow(&e.negate(), scale)?;
            return Self::one().div(&magnitude, scale);
        }
        if e.is_zero() {
            // Including 0^0, which bc and dc both answer as 1.
            return Ok(Self::one());
        }
        // Square-and-multiply, so an exponent of a million is twenty
        // multiplications rather than a million of them.
        let mut result = Self::one();
        let mut base = self.clone();
        let mut exponent = e.digits.clone();
        let two = BigInt::from_i64(2);
        while !exponent.is_zero() {
            let (half, rem) = exponent.divmod(&two);
            if !rem.is_zero() {
                result = result.mul_exact(&base);
            }
            exponent = half;
            // The last squaring is never used, and for a large exponent it is
            // the most expensive one in the loop.
            if !exponent.is_zero() {
                base = base.mul_exact(&base);
            }
        }
        // `scale(a) * b`, saturating: the product only has to be *at least* the
        // target for the `min` to pick the other side, so a saturated value is
        // as good as the true one here.
        let exact = self.scale.saturating_mul(e.digits.to_usize_saturating());
        Ok(result.rescale(exact.min(scale.max(self.scale))))
    }

    /// Square root, truncated to `result_scale`.
    ///
    /// # Errors
    ///
    /// [`DecimalError::NegativeSqrt`] if `self` is negative.
    pub fn sqrt(&self, result_scale: usize) -> Result<Self, DecimalError> {
        if self.is_negative() {
            return Err(DecimalError::NegativeSqrt);
        }
        if self.is_zero() {
            return Ok(Self::zero());
        }
        // A square root halves the number of digits, so to keep `result_scale`
        // of them afterwards the input needs twice as many beforehand — plus a
        // couple so the truncation at the end cannot eat a digit we promised.
        let extra = result_scale.saturating_mul(2).saturating_add(2);
        let scaled = self.rescale(self.scale.saturating_add(extra));
        let root = Self {
            digits: scaled.digits.isqrt(),
            scale: scaled.scale.div_ceil(2),
        };
        Ok(root.rescale(result_scale))
    }

    /// Three-way comparison as `-1`, `0` or `1`.
    ///
    /// The calculators' relational operators want a small integer rather than
    /// an [`Ordering`](core::cmp::Ordering), and `bc` in particular pushes the
    /// result of a comparison onto the stack as a number. This is that spelling
    /// of [`Ord::cmp`], not a second implementation of it.
    #[must_use]
    pub fn signum_of_difference(&self, other: &Self) -> i32 {
        match self.cmp(other) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        }
    }

    /// Parse a number such as `123.456`, in the given input base.
    ///
    /// Anything unparseable is zero rather than an error, matching the
    /// calculators' lexers, which have already decided the token is a number
    /// before this is reached.
    #[must_use]
    pub fn parse(s: &str, ibase: u32) -> Self {
        let s = s.trim();
        if s.is_empty() {
            return Self::zero();
        }
        let (negative, body) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };

        // Split on the point by *byte* index from `find`, which is a character
        // boundary by construction, and take the two halves with `get` so a
        // malformed input cannot panic here.
        let (int_part, frac_part) = match body.find('.') {
            Some(dot) => (
                body.get(..dot).unwrap_or(""),
                body.get(dot.saturating_add(1)..).unwrap_or(""),
            ),
            None => (body, ""),
        };

        let mut digits = BigInt::from_str_radix(&format!("{int_part}{frac_part}"), ibase);
        digits.negative = negative;
        digits.normalize();
        Self {
            digits,
            scale: frac_part.len(),
        }
    }

    /// Render for output in the given base.
    #[must_use]
    pub fn format(&self, obase: u32) -> String {
        if obase == 10 {
            return self.format_base10();
        }
        let int_val = self.rescale(0);
        let mut result = int_val.digits.to_str_radix(obase);
        if self.scale == 0 {
            return result;
        }
        result.push('.');
        // The fractional digits of a non-decimal base are produced one at a
        // time by repeated multiplication, because they are not a substring of
        // anything the decimal mantissa already holds.
        let ten_pow = ten_to_the(self.scale);
        let mut frac = self.sub(&int_val.rescale(self.scale)).abs().digits;
        let base_big = BigInt::from_i64(i64::from(obase));
        for _ in 0..self.scale {
            frac = frac.mul(&base_big);
            let (q, r) = frac.divmod(&ten_pow);
            let d = q.limbs.first().copied().unwrap_or(0);
            result.push(digit_to_char(d));
            frac = r;
        }
        result
    }

    /// Render in base ten, with trailing fractional zeros removed.
    #[must_use]
    pub fn format_base10(&self) -> String {
        if self.scale == 0 {
            return self.digits.to_string_base10();
        }
        let s = self.digits.to_string_base10();
        let negative = s.starts_with('-');
        let abs_s = s.strip_prefix('-').unwrap_or(&s);

        let (int_part, frac_part) = match abs_s.len().checked_sub(self.scale) {
            // More digits than fractional places: the split is inside the string.
            Some(split) if split > 0 => (
                abs_s.get(..split).unwrap_or("0").to_string(),
                abs_s.get(split..).unwrap_or("").to_string(),
            ),
            // Fewer: the value is under one and the fraction needs leading zeros.
            _ => {
                let padding = self.scale.saturating_sub(abs_s.len());
                ("0".to_string(), format!("{}{}", "0".repeat(padding), abs_s))
            }
        };

        let frac_trimmed = frac_part.trim_end_matches('0');
        let prefix = if negative { "-" } else { "" };
        if frac_trimmed.is_empty() {
            format!("{prefix}{int_part}")
        } else {
            format!("{prefix}{int_part}.{frac_trimmed}")
        }
    }

    /// How many significant digits the mantissa has — `bc`'s `length`.
    #[must_use]
    pub fn length(&self) -> usize {
        self.digits.to_string_base10().trim_start_matches('-').len()
    }

    /// Whether the value is too small to be distinguished from zero at
    /// `working_scale` fractional digits.
    ///
    /// This is what lets an iterative series in the math library stop: once a
    /// term cannot move any digit that will be kept, the remaining terms cannot
    /// either.
    #[must_use]
    pub fn is_negligible(&self, working_scale: usize) -> bool {
        self.is_zero() || self.abs().rescale(working_scale).digits.is_zero()
    }
}

/// `10^n` as a `BigInt`.
fn ten_to_the(n: usize) -> BigInt {
    BigInt::one().shift_left_decimal(n)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse(s, 10)
    }

    /// `a op b` at `scale`, rendered — the shape most of these tests want.
    fn div(a: &str, b: &str, scale: usize) -> String {
        d(a).div(&d(b), scale).unwrap().format_base10()
    }

    /// `a * b` at the calculator's `scale`, by POSIX's rule for `*`.
    fn multiply(a: &str, b: &str, scale: usize) -> String {
        d(a).multiply(&d(b), scale).format_base10()
    }

    /// `a ^ b` at the calculator's `scale`.
    fn pow(a: &str, b: &str, scale: usize) -> String {
        d(a).pow(&d(b), scale).unwrap().format_base10()
    }

    #[test]
    fn a_product_keeps_digits_that_scale_zero_would_seem_to_forbid() {
        // POSIX gives a product the scale min(a + b, max(scale, a, b)), so
        // `scale = 0` does *not* make multiplication integer: `scale` governs
        // division, where digits have to be invented. GNU bc answers 2.2 here.
        assert_eq!(multiply("1.5", "1.5", 0), "2.2");
        assert_eq!(multiply("1.5", "1.5", 10), "2.25");
        // The min stops it inventing digits the operands never had.
        assert_eq!(multiply("2", "3", 10), "6");
        assert_eq!(multiply("0.5", "4", 10), "2");
    }

    #[test]
    fn a_product_at_a_high_scale_is_the_exact_one() {
        assert_eq!(multiply("0.001", "0.001", 10), "0.000001");
        assert_eq!(multiply("1.11", "1.11", 4), "1.2321");
        // Cut short by the operands' own scales, not by `scale`.
        assert_eq!(multiply("1.11", "1.11", 1), "1.23");
    }

    #[test]
    fn a_power_squares_exactly_before_it_truncates() {
        // The bug this pins: truncating each squaring to the result scale made
        // `1.5 ^ 2` answer 2, because the .25 was discarded by the squaring
        // itself rather than by the truncation at the end.
        assert_eq!(pow("1.5", "2", 0), "2.2");
        assert_eq!(pow("1.5", "2", 10), "2.25");
        assert_eq!(pow("1.1", "3", 10), "1.331");
        assert_eq!(pow("2", "10", 0), "1024");
        // Square-and-multiply must agree with the schoolbook answer on an
        // exponent whose binary form exercises both branches of the loop.
        assert_eq!(pow("1.1", "5", 10), "1.61051");
    }

    #[test]
    fn a_power_of_zero_or_a_negative_exponent_behaves() {
        assert_eq!(pow("7", "0", 5), "1");
        assert_eq!(pow("0", "0", 5), "1");
        assert_eq!(pow("2", "-3", 5), "0.125");
        assert_eq!(pow("0", "5", 5), "0");
        assert_eq!(
            d("0").pow(&d("-1"), 5).unwrap_err(),
            DecimalError::DivideByZero
        );
    }

    #[test]
    fn a_remainder_is_what_is_actually_left_over() {
        // `a % b == a - (a/b)*b` has to hold exactly, or the remainder is not
        // the remainder. Truncating the product made it fail: the answer could
        // come back larger than the divisor.
        for (a, b, scale) in [
            ("17", "5", 0),
            ("17.5", "5", 0),
            ("1", "3", 5),
            ("-17", "5", 0),
            ("0.001", "0.3", 4),
        ] {
            let (x, y) = (d(a), d(b));
            let q = x.div(&y, scale).unwrap();
            let r = x.modulo(&y, scale).unwrap();
            assert_eq!(
                q.mul_exact(&y).add(&r),
                x,
                "q*b + r != a for {a} % {b} at scale {scale}"
            );
        }
        assert_eq!(d("17").modulo(&d("5"), 0).unwrap().format_base10(), "2");
        assert_eq!(
            d("1").modulo(&d("3"), 5).unwrap().format_base10(),
            "0.00001"
        );
    }

    #[test]
    fn a_bigint_converts_to_usize_or_saturates() {
        assert_eq!(BigInt::from_i64(0).to_usize_saturating(), 0);
        assert_eq!(
            BigInt::from_i64(1_000_000_007).to_usize_saturating(),
            1_000_000_007
        );
        // Sign is dropped, as documented.
        assert_eq!(BigInt::from_i64(-42).to_usize_saturating(), 42);
        let huge = BigInt::from_i64(10).pow(&BigInt::from_i64(40));
        assert_eq!(huge.to_usize_saturating(), usize::MAX);
    }

    #[test]
    fn parsing_records_the_fractional_length_as_the_scale() {
        assert_eq!(d("1.25").scale, 2);
        assert_eq!(d("1.25").digits.to_string_base10(), "125");
        assert_eq!(d("7").scale, 0);
        assert_eq!(d("-0.001").scale, 3);
        assert!(d("-0.001").is_negative());
    }

    #[test]
    fn a_trailing_or_leading_point_parses_rather_than_panicking() {
        assert_eq!(d("5.").format_base10(), "5");
        assert_eq!(d(".5").format_base10(), "0.5");
        assert_eq!(d("").format_base10(), "0");
        assert_eq!(d("-").format_base10(), "0");
        assert_eq!(d(".").format_base10(), "0");
    }

    #[test]
    fn tenths_add_exactly() {
        // The whole reason this type exists rather than an f64: in binary,
        // 0.1 + 0.2 is 0.30000000000000004.
        assert_eq!(d("0.1").add(&d("0.2")).format_base10(), "0.3");
    }

    #[test]
    fn addition_takes_the_larger_scale() {
        assert_eq!(d("1.5").add(&d("2.25")).format_base10(), "3.75");
        assert_eq!(d("1").add(&d("0.001")).format_base10(), "1.001");
        assert_eq!(d("1.5").sub(&d("2.25")).format_base10(), "-0.75");
    }

    #[test]
    fn exactness_holds_far_past_a_double() {
        // 2^53 is where an f64 stops being able to count by ones, and this is
        // the failure `dc` currently has.
        let big = d("9007199254740993"); // 2^53 + 1
        assert_eq!(big.add(&d("1")).format_base10(), "9007199254740994");
        let huge = d("99999999999999999999");
        assert_eq!(
            huge.mul(&huge, 0).format_base10(),
            "9999999999999999999800000000000000000001"
        );
    }

    #[test]
    fn multiplication_forms_the_full_product_before_truncating() {
        // 0.05 * 0.05 is 0.0025; truncating to 2 places gives 0.00, but
        // truncating the *operands* first would give 0.0 * 0.0 = 0 for the
        // wrong reason. The distinction shows when a kept digit survives.
        assert_eq!(d("0.05").mul(&d("0.05"), 4).format_base10(), "0.0025");
        assert_eq!(d("0.05").mul(&d("0.05"), 2).format_base10(), "0");
        assert_eq!(d("1.5").mul(&d("1.5"), 2).format_base10(), "2.25");
    }

    #[test]
    fn division_truncates_and_does_not_round() {
        // scale=0; 1/2 is 0 in bc, not 1. Rounding here would be a POSIX
        // violation, not a matter of taste.
        assert_eq!(div("1", "2", 0), "0");
        assert_eq!(div("1", "2", 1), "0.5");
        assert_eq!(div("-1", "2", 0), "0");
        assert_eq!(div("9", "10", 0), "0");
    }

    #[test]
    fn division_carries_the_requested_number_of_digits() {
        assert_eq!(div("1", "3", 5), "0.33333");
        assert_eq!(div("2", "3", 5), "0.66666");
        assert_eq!(div("10", "4", 2), "2.5");
        assert_eq!(div("1", "8", 3), "0.125");
    }

    #[test]
    fn division_by_a_fraction_cancels_the_divisors_scale() {
        assert_eq!(div("1", "0.5", 2), "2");
        assert_eq!(div("1", "0.001", 0), "1000");
        assert_eq!(div("0.001", "0.001", 3), "1");
    }

    #[test]
    fn dividing_by_zero_is_an_error_and_not_a_zero() {
        // The version this was lifted from printed to stderr and returned
        // zero, so the caller went on computing with a plausible number.
        assert_eq!(d("1").div(&d("0"), 5), Err(DecimalError::DivideByZero));
        assert_eq!(d("1").div(&d("0.000"), 5), Err(DecimalError::DivideByZero));
        assert_eq!(d("1").modulo(&d("0"), 5), Err(DecimalError::DivideByZero));
    }

    #[test]
    fn modulo_is_defined_from_the_truncated_quotient() {
        // bc's rule: a - (a/b)*b at the working scale, so the scale changes
        // the answer.
        assert_eq!(d("10").modulo(&d("3"), 0).unwrap().format_base10(), "1");
        assert_eq!(d("-10").modulo(&d("3"), 0).unwrap().format_base10(), "-1");
        assert_eq!(d("10").modulo(&d("3"), 1).unwrap().format_base10(), "0.1");
    }

    #[test]
    fn powers_use_square_and_multiply_and_stay_exact() {
        assert_eq!(d("2").pow(&d("10"), 0).unwrap().format_base10(), "1024");
        assert_eq!(
            d("2").pow(&d("64"), 0).unwrap().format_base10(),
            "18446744073709551616"
        );
        assert_eq!(d("1.5").pow(&d("2"), 2).unwrap().format_base10(), "2.25");
    }

    #[test]
    fn a_zero_exponent_is_one_and_a_negative_one_inverts() {
        assert_eq!(d("7").pow(&d("0"), 0).unwrap().format_base10(), "1");
        assert_eq!(d("0").pow(&d("0"), 0).unwrap().format_base10(), "1");
        assert_eq!(d("2").pow(&d("-2"), 3).unwrap().format_base10(), "0.25");
        // 1/0 reached through the exponent is still a division by zero.
        assert_eq!(d("0").pow(&d("-1"), 3), Err(DecimalError::DivideByZero));
    }

    #[test]
    fn a_fractional_exponent_truncates_to_an_integer() {
        assert_eq!(d("2").pow(&d("3.9"), 0).unwrap().format_base10(), "8");
    }

    #[test]
    fn square_roots_are_truncated_not_rounded() {
        assert_eq!(d("2").sqrt(5).unwrap().format_base10(), "1.41421");
        assert_eq!(d("4").sqrt(0).unwrap().format_base10(), "2");
        assert_eq!(d("0").sqrt(5).unwrap().format_base10(), "0");
        assert_eq!(
            d("10000000000000000000000")
                .sqrt(0)
                .unwrap()
                .format_base10(),
            "100000000000"
        );
    }

    #[test]
    fn the_square_root_of_a_negative_is_an_error() {
        assert_eq!(d("-1").sqrt(5), Err(DecimalError::NegativeSqrt));
        // Negative zero does not exist here, so this is not an error.
        assert!(d("-0").sqrt(5).is_ok());
    }

    #[test]
    fn rescaling_down_truncates_toward_zero_on_both_signs() {
        assert_eq!(d("1.99").rescale(0).format_base10(), "1");
        assert_eq!(d("-1.99").rescale(0).format_base10(), "-1");
        assert_eq!(d("1.5").rescale(4).format_base10(), "1.5");
        assert_eq!(d("1.5").rescale(4).scale, 4);
    }

    #[test]
    fn comparison_works_across_differing_scales() {
        assert_eq!(d("1.50").signum_of_difference(&d("1.5")), 0);
        assert_eq!(d("1.5").signum_of_difference(&d("1.50000")), 0);
        assert_eq!(d("1").signum_of_difference(&d("2")), -1);
        assert_eq!(d("2").signum_of_difference(&d("1")), 1);
        assert_eq!(d("-2").signum_of_difference(&d("-1")), -1);
        assert_eq!(d("-1").signum_of_difference(&d("1")), -1);
        assert_eq!(d("0").signum_of_difference(&d("0.000")), 0);
    }

    #[test]
    fn two_spellings_of_one_number_are_equal_and_neither_is_less() {
        // The point of hand-writing `Ord`: a derived one compares the mantissa
        // and would put 2 below 1.5, because 2 is the shorter number.
        assert_eq!(d("1.5"), d("1.50"));
        assert!(d("1.5") <= d("1.50"));
        assert!(d("1.5") >= d("1.50"));
        assert!(d("1.5") < d("2"));
        assert!(d("-3") < d("1.5"));
        assert!(d("0") > d("-0.001"));
        let mut v = [d("2"), d("-1"), d("1.5"), d("0.25")];
        v.sort();
        let rendered: Vec<String> = v.iter().map(Decimal::format_base10).collect();
        assert_eq!(rendered, ["-1", "0.25", "1.5", "2"]);
    }

    #[test]
    fn formatting_drops_trailing_zeros_but_keeps_the_value() {
        assert_eq!(d("1.500").format_base10(), "1.5");
        assert_eq!(d("1.000").format_base10(), "1");
        assert_eq!(d("0.000").format_base10(), "0");
        assert_eq!(d("-0.500").format_base10(), "-0.5");
    }

    #[test]
    fn a_value_under_one_keeps_its_leading_zeros() {
        // The fractional digits are shorter than the scale here, so the
        // padding is what stops 0.001 rendering as 0.1.
        assert_eq!(d("0.001").format_base10(), "0.001");
        assert_eq!(div("1", "1000", 5), "0.001");
    }

    #[test]
    fn other_bases_round_trip_the_integer_part() {
        assert_eq!(d("255").format(16), "FF");
        assert_eq!(d("8").format(2), "1000");
        assert_eq!(d("-255").format(16), "-FF");
        assert_eq!(Decimal::parse("FF", 16).format_base10(), "255");
    }

    #[test]
    fn negating_zero_gives_zero_and_not_minus_zero() {
        assert_eq!(d("0").negate().format_base10(), "0");
        assert!(!d("0").negate().is_negative());
        assert_eq!(d("1.5").negate().abs().format_base10(), "1.5");
    }

    #[test]
    fn negligible_is_what_lets_a_series_stop() {
        assert!(d("0.0001").is_negligible(3));
        assert!(!d("0.0001").is_negligible(4));
        assert!(d("0").is_negligible(0));
        assert!(d("-0.0001").is_negligible(3));
    }

    #[test]
    fn length_counts_the_mantissa_and_ignores_the_sign() {
        assert_eq!(d("12345").length(), 5);
        assert_eq!(d("-12345").length(), 5);
        assert_eq!(d("1.2345").length(), 5);
    }

    #[test]
    fn a_huge_scale_does_not_overflow_the_scale_arithmetic() {
        // The scale is a usize and these add; saturating rather than wrapping
        // is what keeps a preposterous request slow instead of wrong.
        let a = Decimal {
            digits: BigInt::one(),
            scale: usize::MAX,
        };
        assert!(a.mul(&a, 0).is_zero());
    }
}
