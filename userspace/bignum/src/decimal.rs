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

/// The traditional line length for calculator output: a terminal of the era.
///
/// It is the value of `BC_LINE_LENGTH`/`DC_LINE_LENGTH`, *not* the number of
/// digits that fit on a line — see [`wrap_number`], which takes the latter,
/// because `bc` and `dc` disagree about how to get from one to the other.
pub const DEFAULT_LINE_LENGTH: usize = 70;

/// Break a rendered number every `chunk` characters, continuing with `\`.
///
/// A 300-digit answer arrives as five lines, the first four ending in `\`:
/// backslash-newline is the continuation the shell and every other reader of
/// `bc`'s output already understands, and it is what makes the output
/// re-readable as a single number.
///
/// # Why this takes a chunk width rather than a line length
///
/// Because `bc` and `dc` — the same program, from the same source tarball —
/// answer that question differently, and both answers are observable:
///
/// | | `LINE_LENGTH=10` emits | chunk | wrap off below |
/// |---|---|---|---|
/// | `bc` | `12345678\` — 9 columns | `L - 2` | `L = 3` |
/// | `dc` | `123456789\` — 10 columns | `L - 1` | `L = 2` |
///
/// So `bc` treats the length as a column the backslash may not reach and `dc`
/// treats it as one it may. Encoding either rule here would silently impose it
/// on the other front-end; each computes its own `chunk` and this function
/// keeps no opinion. A `chunk` of 0 means no wrapping, which is what
/// `BC_LINE_LENGTH=0` asks for and the only way a script gets one long line.
///
/// Every character counts toward the width, including a leading `-` or `.`:
/// `-1/3` at 20 places and `BC_LINE_LENGTH=10` begins `-.333333\`, eight
/// characters of which only six are digits.
///
/// Only *numbers* go through here. `print "…"` in `bc` and `dc`'s string `p`
/// emit what the program said, unbroken; inserting a backslash into a string
/// the user chose would corrupt it.
#[must_use]
pub fn wrap_number(text: &str, chunk: usize) -> String {
    if chunk == 0 {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= chunk {
        return text.to_string();
    }
    // Two bytes of continuation per line after the first; a hint only, so a
    // saturating estimate is as good as an exact one.
    let continuations = text.len().checked_div(chunk).unwrap_or(0).saturating_mul(2);
    let mut out = String::with_capacity(text.len().saturating_add(continuations));
    for (i, piece) in chars.chunks(chunk).enumerate() {
        if i > 0 {
            out.push_str("\\\n");
        }
        out.extend(piece.iter());
    }
    out
}

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
    ///
    /// A fractional part in a base other than ten is *divided down* rather than
    /// simply counted: with `ibase = 16`, `.8` is eight sixteenths — `0.5` —
    /// and not `0.8`. Treating the fractional digit count as a decimal scale,
    /// which an earlier version did, is only correct when the base is ten.
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

        let sign = |mut d: BigInt| {
            d.negative = negative;
            d.normalize();
            d
        };

        if ibase == 10 {
            return Self {
                digits: sign(BigInt::from_str_radix(
                    &format!("{int_part}{frac_part}"),
                    ibase,
                )),
                scale: frac_part.len(),
            };
        }

        let integer = Self {
            digits: BigInt::from_str_radix(int_part, ibase),
            scale: 0,
        };
        if frac_part.is_empty() {
            return Self {
                digits: sign(integer.digits),
                scale: 0,
            };
        }

        // The fraction is `frac / ibase^len`. How many decimal places that
        // needs to be *exact* is `len * log2(ibase)` whenever the base is a
        // power of two — 16^-1 is 0.0625, four places for one hex digit — and
        // for a base that is not, no finite number of them is enough and this
        // is a truncation. Erring high is safe because `trim_scale` below drops
        // whatever places the division did not actually need — without it the
        // over-estimate would be *printed*, and `16i .8 p` would answer
        // `.5000` instead of `.5`.
        let places_per_digit = usize::try_from(ibase.next_power_of_two().trailing_zeros())
            .unwrap_or(1)
            .max(1);
        let scale = frac_part.len().saturating_mul(places_per_digit);
        let numerator = Self {
            digits: BigInt::from_str_radix(frac_part, ibase),
            scale: 0,
        };
        let denominator = Self {
            digits: BigInt::from_i64(i64::from(ibase)).pow(&BigInt::from_i64(
                i64::try_from(frac_part.len()).unwrap_or(i64::MAX),
            )),
            scale: 0,
        };
        // The divisor is a power of the base and the base is at least two, so
        // it cannot be zero and this cannot fail; zero is nonetheless the right
        // answer if it somehow did.
        let fraction = numerator.div(&denominator, scale).unwrap_or_else(|_| Self {
            digits: BigInt::zero(),
            scale,
        });
        let total = integer.add(&fraction).trim_scale();
        Self {
            digits: sign(total.digits),
            scale: total.scale,
        }
    }

    /// Drop fractional places that hold nothing, without changing the value.
    ///
    /// This is *not* something to reach for after arithmetic. `bc` defines the
    /// scale of a result exactly, so `scale=20; 1/2` has twenty places and
    /// printing nineteen of them as `.5` would be wrong. It exists for the one
    /// case where the scale is an artefact rather than a decision: reading a
    /// fraction in a non-decimal base computes at an upper bound on the places
    /// the value could need, and the bound is usually loose.
    #[must_use]
    pub fn trim_scale(&self) -> Self {
        if self.scale == 0 {
            return self.clone();
        }
        if self.digits.is_zero() {
            return Self {
                digits: BigInt::zero(),
                scale: 0,
            };
        }
        let text = self.digits.to_string_base10();
        let kept = text.len().saturating_sub(text.trim_end_matches('0').len());
        // Never trim past the point: the integer digits are not ours to touch.
        self.rescale(self.scale.saturating_sub(kept.min(self.scale)))
    }

    /// Render for output in the given base.
    #[must_use]
    pub fn format(&self, obase: u32) -> String {
        if obase == 10 {
            return self.format_base10();
        }
        if obase > MAX_CHAR_BASE {
            return self.format_grouped(obase);
        }
        let int_val = self.rescale(0);
        if self.scale == 0 {
            return int_val.digits.to_str_radix(obase);
        }
        // Same rule as base ten: a value under one has no integer part to
        // write, so `.8` and not `0.8`.
        //
        // The sign has to be put back by hand in that case. `rescale(0)`
        // truncates -0.5 to zero and `BigInt` keeps no negative zero, so
        // `to_str_radix` would answer "0" and the minus would be lost
        // outright — this printed `-0.5` as `0.8` in base sixteen.
        let mut result = if int_val.digits.is_zero() {
            if self.is_negative() { "-" } else { "" }.to_string()
        } else {
            int_val.digits.to_str_radix(obase)
        };
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

    /// Render in a base too large to spend one character on a digit.
    ///
    /// Above base sixteen the digit characters run out, so both GNU `bc`
    /// 1.07.1 and GNU `dc` 1.4.1 switch to writing each digit as a *decimal*
    /// number, zero-padded to the width the largest digit needs and preceded
    /// by a space. Base 36 writes 1295 as ` 35 35`, base 1000 writes 999999
    /// as ` 999 999`, and base 17 writes 255 as ` 15 00` — note that 17 does
    /// *not* get `F0`, so the switch is at 16 exactly and is about the
    /// notation, not about whether letters would suffice.
    ///
    /// Three details that are easy to get wrong, all measured:
    ///
    /// - The space belongs *to* each digit rather than sitting between
    ///   digits, so the output begins with one: base 36 writes 1 as ` 01`.
    /// - A `.` takes the place of the first fractional digit's space, which
    ///   is why `scale=4; obase=20; 1/2` is `.10 00 00 00` and not
    ///   `. 10 00 00 00`.
    /// - Zero is plain `0`, and a value under one has no integer group at
    ///   all — the same rules base ten follows.
    #[must_use]
    fn format_grouped(&self, obase: u32) -> String {
        if self.digits.is_zero() {
            return "0".to_string();
        }
        let width = decimal_width(obase.saturating_sub(1));
        let base_big = BigInt::from_i64(i64::from(obase));
        let int_val = self.rescale(0);

        let mut result = String::new();
        if self.is_negative() {
            result.push('-');
        }

        // Integer digits, least significant first, then reversed: division is
        // the only way round, since a digit of a large base is not a substring
        // of the decimal mantissa.
        let mut magnitude = int_val.digits.clone();
        magnitude.negative = false;
        let mut groups: Vec<u32> = Vec::new();
        while !magnitude.is_zero() {
            let (q, r) = magnitude.divmod(&base_big);
            // The remainder is below the base, so it is one limb.
            groups.push(r.limbs.first().copied().unwrap_or(0));
            magnitude = q;
        }
        for digit in groups.iter().rev() {
            result.push(' ');
            push_padded(&mut result, *digit, width);
        }

        if self.scale == 0 {
            return result;
        }
        result.push('.');
        let ten_pow = ten_to_the(self.scale);
        let mut frac = self.sub(&int_val.rescale(self.scale)).abs().digits;
        for place in 0..self.scale {
            frac = frac.mul(&base_big);
            let (q, r) = frac.divmod(&ten_pow);
            let digit = q.limbs.first().copied().unwrap_or(0);
            if place > 0 {
                result.push(' ');
            }
            push_padded(&mut result, digit, width);
            frac = r;
        }
        result
    }

    /// Render in base ten with exactly [`Decimal::scale`] fractional digits.
    ///
    /// The trailing zeros are not noise to be tidied away — they are the whole
    /// observable meaning of `scale`. `bc`'s `scale=20; 1/2` is defined to
    /// produce twenty fractional digits and prints `.50000000000000000000`;
    /// `dc`'s `1.50 p` echoes `1.50` because that is the scale the literal was
    /// written with. A formatter that trimmed them would make `k` untestable
    /// and would silently disagree with every other `bc` about the one thing
    /// `bc` exists to control.
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
            // Fewer: the value is under one, so there is no integer part to
            // write at all — `1/2` prints as `.5`, not `0.5`. That is what
            // every traditional `bc` and `dc` does, and scripts compare
            // against it: `$(echo "scale=2;1/2" | bc)` is `.50` elsewhere, and
            // a test that checks for `.50` fails against `0.50`. The fraction
            // still needs its own leading zeros, which is what stops `0.001`
            // rendering as `.1`.
            _ => {
                let padding = self.scale.saturating_sub(abs_s.len());
                (String::new(), format!("{}{}", "0".repeat(padding), abs_s))
            }
        };

        // A zero prints as `0`, not `-0.000`: `negate` keeps no negative zero,
        // but a mantissa of zero with a scale still has fractional places, and
        // both calculators print plain `0` for it.
        if self.digits.is_zero() {
            return "0".to_string();
        }
        let prefix = if negative { "-" } else { "" };
        format!("{prefix}{int_part}.{frac_part}")
    }

    /// How many digits the number is written with — `bc`'s `length`, `dc`'s `Z`.
    ///
    /// The mantissa's digit count alone is not the answer: `0.001` has a
    /// one-digit mantissa but is written with three digits, which is what both
    /// calculators report. Taking the larger of the two counts covers that case
    /// without disturbing `1.001`, where the mantissa is already the longer.
    #[must_use]
    pub fn length(&self) -> usize {
        let mantissa = self.digits.to_string_base10().trim_start_matches('-').len();
        mantissa.max(self.scale)
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

/// The largest output base whose digits each fit in one character.
///
/// `0`–`9` and `A`–`F`. Above this both GNU calculators change notation
/// entirely — see [`Decimal::format_grouped`] — rather than carrying on
/// through `G`–`Z`, which is why this is 16 and not 36 even though
/// [`digit_to_char`] can name a digit as far as 35.
pub const MAX_CHAR_BASE: u32 = 16;

/// How many decimal digits it takes to write `n`.
fn decimal_width(n: u32) -> usize {
    let mut width = 1usize;
    let mut rest = n / 10;
    while rest > 0 {
        width = width.saturating_add(1);
        rest /= 10;
    }
    width
}

/// Append `digit` in decimal, zero-padded on the left to `width`.
fn push_padded(out: &mut String, digit: u32, width: usize) {
    let text = digit.to_string();
    for _ in text.len()..width {
        out.push('0');
    }
    out.push_str(&text);
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
        // One place, because `0.5` had one and the product rule keeps the
        // larger of the operands' scales -- the digit is a zero, and it is
        // still printed.
        assert_eq!(multiply(".5", "4", 10), "2.0");
    }

    #[test]
    fn a_product_at_a_high_scale_is_the_exact_one() {
        assert_eq!(multiply(".001", ".001", 10), ".000001");
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
        assert_eq!(pow("2", "-3", 5), ".12500");
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
            (".001", ".3", 4),
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
        assert_eq!(d("1").modulo(&d("3"), 5).unwrap().format_base10(), ".00001");
    }

    #[test]
    fn length_counts_the_digits_the_number_is_written_with() {
        assert_eq!(d("12345").length(), 5);
        assert_eq!(d("1.001").length(), 4);
        assert_eq!(d("100").length(), 3);
        assert_eq!(d(".5").length(), 1);
        // The case the mantissa count gets wrong: the leading zeros are digits.
        assert_eq!(d(".001").length(), 3);
        assert_eq!(d("-.001").length(), 3);
    }

    #[test]
    fn a_fraction_in_another_base_is_divided_down_not_counted() {
        // Hex .8 is eight sixteenths. Counting the digit as a decimal place
        // made it 0.8, which is 0.3 out on the very first hex fraction anyone
        // would type.
        assert_eq!(Decimal::parse(".8", 16).format_base10(), ".5");
        assert_eq!(Decimal::parse("A.8", 16).format_base10(), "10.5");
        assert_eq!(Decimal::parse(".1", 16).format_base10(), ".0625");
        assert_eq!(Decimal::parse(".01", 16).format_base10(), ".00390625");
        assert_eq!(Decimal::parse(".1", 2).format_base10(), ".5");
        assert_eq!(Decimal::parse("1.1", 2).format_base10(), "1.5");
        assert_eq!(Decimal::parse(".4", 8).format_base10(), ".5");
        assert_eq!(Decimal::parse("-.8", 16).format_base10(), "-.5");
        // Base ten is unchanged, including its exact scale.
        assert_eq!(Decimal::parse(".8", 10).format_base10(), ".8");
        assert_eq!(Decimal::parse("1.250", 10).scale, 3);
        // Integers in any base are unaffected.
        assert_eq!(Decimal::parse("FF", 16).format_base10(), "255");
        assert_eq!(Decimal::parse("-FF", 16).format_base10(), "-255");
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
        assert_eq!(d("-.001").scale, 3);
        assert!(d("-.001").is_negative());
    }

    #[test]
    fn a_trailing_or_leading_point_parses_rather_than_panicking() {
        assert_eq!(d("5.").format_base10(), "5");
        assert_eq!(d(".5").format_base10(), ".5");
        assert_eq!(d("").format_base10(), "0");
        assert_eq!(d("-").format_base10(), "0");
        assert_eq!(d(".").format_base10(), "0");
    }

    #[test]
    fn tenths_add_exactly() {
        // The whole reason this type exists rather than an f64: in binary,
        // 0.1 + 0.2 is 0.30000000000000004.
        assert_eq!(d(".1").add(&d(".2")).format_base10(), ".3");
    }

    #[test]
    fn addition_takes_the_larger_scale() {
        assert_eq!(d("1.5").add(&d("2.25")).format_base10(), "3.75");
        assert_eq!(d("1").add(&d(".001")).format_base10(), "1.001");
        assert_eq!(d("1.5").sub(&d("2.25")).format_base10(), "-.75");
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
        assert_eq!(d(".05").mul(&d(".05"), 4).format_base10(), ".0025");
        assert_eq!(d(".05").mul(&d(".05"), 2).format_base10(), "0");
        assert_eq!(d("1.5").mul(&d("1.5"), 2).format_base10(), "2.25");
    }

    #[test]
    fn division_truncates_and_does_not_round() {
        // scale=0; 1/2 is 0 in bc, not 1. Rounding here would be a POSIX
        // violation, not a matter of taste.
        assert_eq!(div("1", "2", 0), "0");
        assert_eq!(div("1", "2", 1), ".5");
        assert_eq!(div("-1", "2", 0), "0");
        assert_eq!(div("9", "10", 0), "0");
    }

    #[test]
    fn division_carries_the_requested_number_of_digits() {
        assert_eq!(div("1", "3", 5), ".33333");
        assert_eq!(div("2", "3", 5), ".66666");
        assert_eq!(div("10", "4", 2), "2.50");
        assert_eq!(div("1", "8", 3), ".125");
    }

    #[test]
    fn division_by_a_fraction_cancels_the_divisors_scale() {
        assert_eq!(div("1", ".5", 2), "2.00");
        assert_eq!(div("1", ".001", 0), "1000");
        assert_eq!(div(".001", ".001", 3), "1.000");
    }

    #[test]
    fn dividing_by_zero_is_an_error_and_not_a_zero() {
        // The version this was lifted from printed to stderr and returned
        // zero, so the caller went on computing with a plausible number.
        assert_eq!(d("1").div(&d("0"), 5), Err(DecimalError::DivideByZero));
        assert_eq!(d("1").div(&d(".000"), 5), Err(DecimalError::DivideByZero));
        assert_eq!(d("1").modulo(&d("0"), 5), Err(DecimalError::DivideByZero));
    }

    #[test]
    fn modulo_is_defined_from_the_truncated_quotient() {
        // bc's rule: a - (a/b)*b at the working scale, so the scale changes
        // the answer.
        assert_eq!(d("10").modulo(&d("3"), 0).unwrap().format_base10(), "1");
        assert_eq!(d("-10").modulo(&d("3"), 0).unwrap().format_base10(), "-1");
        assert_eq!(d("10").modulo(&d("3"), 1).unwrap().format_base10(), ".1");
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
        assert_eq!(d("2").pow(&d("-2"), 3).unwrap().format_base10(), ".250");
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
        assert_eq!(d("1.5").rescale(4).format_base10(), "1.5000");
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
        assert_eq!(d("0").signum_of_difference(&d(".000")), 0);
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
        assert!(d("0") > d("-.001"));
        let mut v = [d("2"), d("-1"), d("1.5"), d(".25")];
        v.sort();
        let rendered: Vec<String> = v.iter().map(Decimal::format_base10).collect();
        assert_eq!(rendered, ["-1", ".25", "1.5", "2"]);
    }

    #[test]
    fn a_long_number_is_continued_with_a_trailing_backslash() {
        // 69 digits beside the `\` makes 70 columns, which is what `dc` emits
        // at its default width. (`bc` puts 68 there; the difference lives in
        // the front-ends, which is why this function takes the chunk itself.)
        let n = d("2").pow(&d("1000"), 0).unwrap().format_base10();
        assert_eq!(n.len(), 302);
        let wrapped = wrap_number(&n, 69);
        let lines: Vec<&str> = wrapped.split('\n').collect();
        assert_eq!(lines.len(), 5);
        for line in lines.iter().take(4) {
            assert_eq!(line.len(), 70);
            assert!(line.ends_with('\\'));
        }
        assert_eq!(lines[4].len(), 26);
        assert!(!lines[4].ends_with('\\'));
        // And it is the same number afterwards, which is the point of the
        // backslash rather than a bare newline.
        let rejoined: String = lines
            .iter()
            .map(|l| l.strip_suffix('\\').unwrap_or(l))
            .collect();
        assert_eq!(rejoined, n);
    }

    #[test]
    fn a_number_that_fits_is_left_alone_and_zero_disables_the_wrap() {
        // 61 digits: shorter than the 69 a continued `dc` line carries.
        let short = d("2").pow(&d("200"), 0).unwrap().format_base10();
        assert_eq!(short.len(), 61);
        assert_eq!(wrap_number(&short, 69), short);
        // A chunk is the last width that does not wrap; one more is the first
        // that does. Off-by-one here would be invisible in ordinary use — and
        // was in fact wrong for `bc` until both tools were run side by side.
        let sixty_nine = "1".repeat(69);
        assert_eq!(wrap_number(&sixty_nine, 69), sixty_nine);
        let seventy = "1".repeat(70);
        assert_eq!(wrap_number(&seventy, 69), format!("{sixty_nine}\\\n1"));
        // A chunk of 0 means one long line, however long it is — what the
        // front-ends pass for `BC_LINE_LENGTH=0`.
        let long = d("2").pow(&d("1000"), 0).unwrap().format_base10();
        assert_eq!(wrap_number(&long, 0), long);
        // A chunk of 1 really does emit one character per line: it is the
        // front-end's job to decide that a width that small means "off".
        assert_eq!(wrap_number("123", 1), "1\\\n2\\\n3");
    }

    #[test]
    fn a_value_below_one_is_written_without_a_leading_zero() {
        // What every traditional `bc` and `dc` prints, and what scripts that
        // capture their output compare against.
        assert_eq!(div("1", "2", 1), ".5");
        assert_eq!(div("1", "2", 3), ".500");
        assert_eq!(d("-0.5").format_base10(), "-.5");
        assert_eq!(d("0.001").format_base10(), ".001");
        // A value of one or more keeps its integer digits, zeros included.
        assert_eq!(d("10.5").format_base10(), "10.5");
        assert_eq!(d("1.5").format_base10(), "1.5");
        assert_eq!(d("0").format_base10(), "0");
        // Same rule in another base -- and the sign survives, which it did
        // not when `rescale(0)` truncated -0.5 to an unsigned zero.
        assert_eq!(d("0.5").format(16), ".8");
        assert_eq!(d("-0.5").format(16), "-.8");
        assert_eq!(d("10.5").format(16), "A.8");
    }

    #[test]
    fn the_two_spellings_of_a_fraction_read_the_same() {
        // Output drops the leading zero; input still accepts it, because a
        // person typing into `dc` writes `0.5` as often as `.5`.
        for (with, without) in [("0.5", ".5"), ("-0.5", "-.5"), ("0.001", ".001")] {
            assert_eq!(d(with), d(without));
            assert_eq!(d(with).scale, d(without).scale);
            assert_eq!(d(with).format_base10(), d(without).format_base10());
        }
    }

    #[test]
    fn formatting_keeps_every_place_the_scale_claims() {
        // The trailing zeros are the scale, and the scale is what `bc`'s `k`
        // and `dc`'s `k` exist to set; a formatter that tidied them away would
        // make the setting unobservable.
        assert_eq!(d("1.500").format_base10(), "1.500");
        assert_eq!(d("1.000").format_base10(), "1.000");
        assert_eq!(d("-.500").format_base10(), "-.500");
        // Zero is the one exception: it prints as `0` at any scale, which is
        // what both calculators do rather than `0.000`.
        assert_eq!(d(".000").format_base10(), "0");
        assert_eq!(d("0").format_base10(), "0");
    }

    #[test]
    fn a_scale_that_is_an_artefact_rather_than_a_decision_is_trimmed() {
        // Reading `.8` in base sixteen computes at an upper bound of four
        // decimal places, but the value needs one. The bound must not be
        // printed: `16i .8 p` answers `.5`, not `.5000`.
        assert_eq!(Decimal::parse(".8", 16).format_base10(), ".5");
        assert_eq!(Decimal::parse(".4", 16).format_base10(), ".25");
        assert_eq!(Decimal::parse(".1", 2).format_base10(), ".5");
        // A base-ten literal is a decision, not an artefact, and keeps its
        // places -- which is why `trim_scale` is not applied to that path.
        assert_eq!(Decimal::parse("1.50", 10).format_base10(), "1.50");
    }

    #[test]
    fn letters_are_worth_ten_to_fifteen_in_every_input_base() {
        // POSIX: "the characters A-F shall represent the values 10-15,
        // respectively, regardless of the input base". The base-ten fast path
        // subtracts `b'0'` blind, so it scored `F` as 22 and read `FF` as 242.
        assert_eq!(Decimal::parse("F", 10).format_base10(), "15");
        assert_eq!(Decimal::parse("FF", 10).format_base10(), "165");
        assert_eq!(Decimal::parse("A", 10).format_base10(), "10");
        assert_eq!(Decimal::parse("FF", 16).format_base10(), "255");
        assert_eq!(Decimal::parse("10", 10).format_base10(), "10");
    }

    #[test]
    fn a_value_under_one_keeps_its_leading_zeros() {
        // The fractional digits are shorter than the scale here, so the
        // padding is what stops 0.001 rendering as 0.1.
        assert_eq!(d(".001").format_base10(), ".001");
        assert_eq!(div("1", "1000", 5), ".00100");
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
        assert!(d(".0001").is_negligible(3));
        assert!(!d(".0001").is_negligible(4));
        assert!(d("0").is_negligible(0));
        assert!(d("-.0001").is_negligible(3));
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
