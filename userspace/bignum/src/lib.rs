//! Arbitrary-precision signed integers, in base-10^9 limbs, and the fixed-point
//! [`Decimal`] the calculators build on top of them.
//!
//! ## Why this is a crate and not a copy
//!
//! Four programs in this tree do exact integer arithmetic — `bc`, `dc`,
//! `genius-cli` and `expr` — and until this crate existed each carried its own
//! bignum. That is not a tidiness complaint: `expr 9223372036854775807 + 1` is
//! the sort of thing a shell script does by accident, and the answer it gets
//! should not depend on which of four implementations the utility happened to
//! be built with. The version here is `bc`'s, which was the most complete and
//! the only one with division and `isqrt`.
//!
//! ## Why base 10^9 rather than base 2^64
//!
//! Every caller's *input and output* is decimal text. A binary base makes the
//! arithmetic marginally faster and the conversion at both ends a repeated
//! division by ten; a decimal base makes conversion a slice of nine digits at a
//! time. These programs parse and print far more than they multiply, so the
//! decimal base wins on the operation that dominates. 10^9 is the largest power
//! of ten whose square still fits in a `u64`, which is what lets `mul`
//! accumulate limb products without splitting them.
//!
//! ## The decimals live here too
//!
//! [`decimal::Decimal`] is a [`BigInt`] mantissa plus a decimal scale — the
//! number type `bc` and `dc` compute in. It is in this crate for the same
//! reason the integers are: the two calculators are one calculator with two
//! syntaxes, and two implementations of their shared number type is two answers
//! to a question that has one. See that module's own docs for why scale is an
//! argument rather than a property, and why the failing operations return a
//! `Result` where `bc`'s originals returned zero.
//!
//! ## What it does not do
//!
//! There is no `Add`/`Mul` operator implementation, on purpose: every method
//! here allocates, and an infix `a * b` in a loop hides that in a way
//! `a.mul(&b)` does not. Division by zero is the caller's to check — `divmod`
//! and [`BigInt::div_limb`] return zero for it rather than panicking, because
//! the callers all need to report it in their own words and with their own exit
//! status.
//!
//! ## No operation here can panic
//!
//! That is a requirement, not an aspiration: `expr "$a" + "$b"` and
//! `bc <<< "$x"` both put a value the user chose directly into this arithmetic,
//! so an index out of range or an overflow is a denial of service on whatever
//! script is running. Every read of a limb goes through [`limb`], which reads a
//! missing one as the zero it logically is, and every accumulator uses a
//! saturating or checked operation.
//!
//! None of those saturations can actually be reached — the invariant below says
//! why — so they cost a branch that never taken and buy the guarantee that a
//! bug in one of them degrades an answer rather than killing the process.
//!
//! ## The invariant every limb operation relies on
//!
//! **Every limb is in `0..LIMB_BASE`**, i.e. below 10^9, and `normalize` is
//! called before any `BigInt` is handed back. Two consequences are used
//! throughout:
//!
//! * a limb product plus two carries is at most
//!   `(10^9-1)^2 + 2*(10^9-1)` ≈ 10^18, which fits in a `u64` with a factor of
//!   18 to spare — this is exactly why the base is 10^9 and not 10^18;
//! * `carry * LIMB_BASE + limb` is likewise below 10^18 whenever `carry` is
//!   itself limb-sized, which is what long division needs.

pub mod decimal;

pub use decimal::{Decimal, DecimalError};

/// Base for each limb -- 10^9 fits comfortably in u32 and makes
/// decimal conversion trivial (each limb is exactly 9 decimal digits).
pub const LIMB_BASE: u64 = 1_000_000_000;
pub const LIMB_DIGITS: usize = 9;

/// Limb `i` of `s`, or zero past its end.
///
/// A shorter operand is the same number as one zero-extended to any length, so
/// reading off the end is meaningful rather than an error — which is what lets
/// every loop below run over the longer of two operands without a bounds check
/// or a panic.
#[inline]
fn limb(s: &[u32], i: usize) -> u64 {
    s.get(i).copied().map_or(0, u64::from)
}

/// Split an accumulator into the carry out and the limb it leaves behind.
///
/// `LIMB_BASE` is a non-zero constant, so neither division can trap; saying so
/// once here keeps the twenty call sites from each repeating the argument.
#[inline]
fn split(v: u64) -> (u64, u32) {
    let carry = v.checked_div(LIMB_BASE).unwrap_or(0);
    let low = v.checked_rem(LIMB_BASE).unwrap_or(0);
    (carry, low as u32)
}

/// Arbitrary-precision signed integer stored as a sign flag and a
/// vector of base-10^9 limbs in little-endian order (limbs[0] is the
/// least significant).
#[derive(Clone, Debug)]
pub struct BigInt {
    pub negative: bool,
    /// Limbs in little-endian order.  Each limb is in 0..10^9.
    pub limbs: Vec<u32>,
}

impl BigInt {
    pub fn zero() -> Self {
        Self {
            negative: false,
            limbs: vec![0],
        }
    }

    pub fn one() -> Self {
        Self {
            negative: false,
            limbs: vec![1],
        }
    }

    pub fn is_zero(&self) -> bool {
        self.limbs.iter().all(|&l| l == 0)
    }

    pub fn from_i64(v: i64) -> Self {
        // `unsigned_abs`, not `-v`: `i64::MIN` has no positive counterpart, and
        // negating it is the one input that would overflow.
        let mut uv = v.unsigned_abs();
        let mut limbs = Vec::new();
        loop {
            let (carry, low) = split(uv);
            limbs.push(low);
            uv = carry;
            if uv == 0 {
                break;
            }
        }
        Self {
            negative: v < 0,
            limbs,
        }
    }

    /// The magnitude as a `usize`, or `usize::MAX` if it does not fit.
    ///
    /// The sign is dropped, so callers that care must check it themselves.
    /// Saturating rather than returning an `Option` is deliberate: every caller
    /// here is asking a question where "bigger than anything we can represent"
    /// and "the largest value" lead to the same behaviour — a loop bound that
    /// will never be reached, a scale that will be clamped, a repeat count that
    /// exhausts the machine either way. An `Option` would push a `None` arm on
    /// each of them whose only sensible body is `usize::MAX`.
    #[must_use]
    pub fn to_usize_saturating(&self) -> usize {
        let mut acc: usize = 0;
        // Most significant limb first, so the first overflow ends it.
        for &limb in self.limbs.iter().rev() {
            acc = match acc
                .checked_mul(LIMB_BASE as usize)
                .and_then(|shifted| shifted.checked_add(limb as usize))
            {
                Some(v) => v,
                None => return usize::MAX,
            };
        }
        acc
    }

    /// Remove leading zero limbs, keeping at least one.
    pub fn normalize(&mut self) {
        while self.limbs.len() > 1 && self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        if self.is_zero() {
            self.negative = false;
        }
    }

    /// Compare magnitudes: 1 if |self| > |other|, -1 if <, 0 if equal.
    ///
    /// Both operands are normalized, so a longer limb vector is a larger
    /// magnitude and only equal lengths need comparing limb by limb — from the
    /// most significant end, which is where the first difference decides.
    pub fn cmp_mag(&self, other: &Self) -> i32 {
        let (a, b) = (&self.limbs, &other.limbs);
        if a.len() != b.len() {
            return if a.len() > b.len() { 1 } else { -1 };
        }
        for (x, y) in a.iter().rev().zip(b.iter().rev()) {
            if x != y {
                return if x > y { 1 } else { -1 };
            }
        }
        0
    }

    /// Add magnitudes, result is unsigned (caller sets sign).
    pub fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
        let len = a.len().max(b.len());
        let mut result = Vec::with_capacity(len.saturating_add(1));
        let mut carry: u64 = 0;
        for i in 0..len {
            // Two limbs and a carry are each below 10^9, so this is below
            // 3*10^9 and the saturation is unreachable.
            let sum = limb(a, i).saturating_add(limb(b, i)).saturating_add(carry);
            let (next, low) = split(sum);
            result.push(low);
            carry = next;
        }
        if carry > 0 {
            result.push(carry as u32);
        }
        result
    }

    /// Subtract magnitudes (|a| >= |b| required).
    ///
    /// The precondition is the caller's — [`BigInt::add`] establishes it with
    /// [`BigInt::cmp_mag`] before every call — but violating it cannot panic:
    /// the borrow simply propagates off the end and the result is the
    /// two's-complement-like wrap, which `normalize` then reduces.
    pub fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut result = Vec::with_capacity(a.len());
        let mut borrow: i64 = 0;
        for i in 0..a.len() {
            // Both limbs are below 10^9 and the borrow is 0 or 1, so this stays
            // inside ±10^9 and the saturations are unreachable.
            let mut diff = (limb(a, i) as i64)
                .saturating_sub(limb(b, i) as i64)
                .saturating_sub(borrow);
            if diff < 0 {
                diff = diff.saturating_add(LIMB_BASE as i64);
                borrow = 1;
            } else {
                borrow = 0;
            }
            result.push(diff as u32);
        }
        result
    }

    pub fn add(&self, other: &Self) -> Self {
        if self.negative == other.negative {
            let mut r = Self {
                negative: self.negative,
                limbs: Self::add_mag(&self.limbs, &other.limbs),
            };
            r.normalize();
            r
        } else {
            match self.cmp_mag(other) {
                1 | 0 => {
                    let mut r = Self {
                        negative: self.negative,
                        limbs: Self::sub_mag(&self.limbs, &other.limbs),
                    };
                    r.normalize();
                    r
                }
                _ => {
                    let mut r = Self {
                        negative: other.negative,
                        limbs: Self::sub_mag(&other.limbs, &self.limbs),
                    };
                    r.normalize();
                    r
                }
            }
        }
    }

    pub fn sub(&self, other: &Self) -> Self {
        let neg_other = Self {
            negative: !other.negative,
            limbs: other.limbs.clone(),
        };
        self.add(&neg_other)
    }

    /// Schoolbook multiplication.
    ///
    /// The accumulator is `u64` rather than `u32` because each slot holds a
    /// limb product plus what was already there plus a carry; the base is 10^9
    /// precisely so that sum stays under 10^18 and needs no splitting.
    pub fn mul(&self, other: &Self) -> Self {
        let (a, b) = (&self.limbs, &other.limbs);
        let mut result = vec![0u64; a.len().saturating_add(b.len())];
        for (i, &av) in a.iter().enumerate() {
            let mut carry: u64 = 0;
            for (j, &bv) in b.iter().enumerate() {
                // `i + j < a.len() + b.len()`, so the slot is always there.
                let Some(slot) = i.checked_add(j).and_then(|k| result.get_mut(k)) else {
                    continue;
                };
                let product = u64::from(av)
                    .saturating_mul(u64::from(bv))
                    .saturating_add(*slot)
                    .saturating_add(carry);
                let (next, low) = split(product);
                *slot = u64::from(low);
                carry = next;
            }
            // The slot one past the inner loop has never been written — the
            // next outer round is the first to reduce it — so this leaves it
            // holding a single carry, itself below `LIMB_BASE`.
            if let Some(slot) = i.checked_add(b.len()).and_then(|k| result.get_mut(k)) {
                *slot = slot.saturating_add(carry);
            }
        }
        let mut r = Self {
            negative: self.negative != other.negative,
            limbs: result.iter().map(|&v| v as u32).collect(),
        };
        r.normalize();
        r
    }

    /// Multiply by a single limb.
    pub fn mul_limb(&self, v: u32) -> Self {
        let mut result = Vec::with_capacity(self.limbs.len().saturating_add(1));
        let mut carry: u64 = 0;
        for &l in &self.limbs {
            let product = u64::from(l)
                .saturating_mul(u64::from(v))
                .saturating_add(carry);
            let (next, low) = split(product);
            result.push(low);
            carry = next;
        }
        if carry > 0 {
            result.push(carry as u32);
        }
        let mut r = Self {
            negative: self.negative,
            limbs: result,
        };
        r.normalize();
        r
    }

    /// Divide self by a single limb, returning (quotient, remainder).
    ///
    /// A zero divisor yields `(0, 0)`, as [`BigInt::divmod`] does and for the
    /// same reason: the caller reports it, in its own words.
    pub fn div_limb(&self, d: u32) -> (Self, u32) {
        if d == 0 {
            return (Self::zero(), 0);
        }
        let mut quotient = vec![0u32; self.limbs.len()];
        let mut rem: u64 = 0;
        // Most significant limb first: each step divides the previous remainder
        // shifted up by one limb, plus this limb. `rem < d < 10^9`, so the
        // shifted value stays under 10^18.
        for (slot, &l) in quotient.iter_mut().zip(self.limbs.iter()).rev() {
            let cur = rem.saturating_mul(LIMB_BASE).saturating_add(u64::from(l));
            *slot = cur.checked_div(u64::from(d)).unwrap_or(0) as u32;
            rem = cur.checked_rem(u64::from(d)).unwrap_or(0);
        }
        let mut q = Self {
            negative: self.negative,
            limbs: quotient,
        };
        q.normalize();
        (q, rem as u32)
    }

    /// Long division: returns (quotient, remainder).
    ///
    /// Truncating toward zero, as C and every caller's specification require:
    /// `-7 / 2` is `-3` with remainder `-1`, not `-4` with remainder `1`. The
    /// remainder takes the *dividend's* sign, which is what keeps
    /// `a/b*b + a%b == a` true.
    ///
    /// A zero divisor yields `(0, 0)` rather than panicking — see the module
    /// docs.
    #[allow(clippy::too_many_lines)] // Knuth's algorithm D is one procedure; splitting it would hide the loop's invariants.
    pub fn divmod(&self, other: &Self) -> (Self, Self) {
        if other.is_zero() {
            // bc prints an error and returns 0 on division by zero.
            return (Self::zero(), Self::zero());
        }
        let cmp = self.cmp_mag(other);
        if cmp < 0 {
            return (Self::zero(), self.clone());
        }
        if cmp == 0 {
            let mut q = Self::one();
            q.negative = self.negative != other.negative;
            q.normalize();
            return (q, Self::zero());
        }

        // For single-limb divisor use the fast path.
        if let [d] = other.limbs.as_slice() {
            let (mut q, r) = self.div_limb(*d);
            q.negative = self.negative != other.negative;
            q.normalize();
            let mut rem = Self::from_i64(i64::from(r));
            rem.negative = self.negative;
            rem.normalize();
            return (q, rem);
        }

        // Knuth Algorithm D (simplified). `cmp > 0` above means `self` has at
        // least as many limbs as `other`, so this subtraction cannot wrap.
        let n = other.limbs.len();
        let m = self.limbs.len().saturating_sub(n);

        // Scale both so the divisor's top limb is at least LIMB_BASE/2, which is
        // what bounds the error in the quotient estimate below to one.
        // `other` is normalized and non-zero, so its top limb is in
        // `1..LIMB_BASE` and `scale` is in `1..=LIMB_BASE/2`.
        let d_top = other.limbs.last().copied().map_or(1, u64::from);
        let scale = LIMB_BASE
            .checked_div(d_top.saturating_add(1))
            .unwrap_or(1)
            .max(1) as u32;

        let u = self.mul_limb(scale);
        let v = other.mul_limb(scale);

        let mut u_limbs = u.limbs.clone();
        while u_limbs.len() <= m.saturating_add(n) {
            u_limbs.push(0);
        }

        // Non-zero, because `v` is `other` scaled by at least 1 and normalized.
        let v_top = v.limbs.last().copied().map_or(1, u64::from);
        // The two limbs below the top, used to refine the estimate. Reading a
        // missing one as zero is correct: the divisor has at least two limbs
        // here, and a shorter dividend prefix really is zero-extended.
        let v_second = v
            .limbs
            .len()
            .checked_sub(2)
            .map_or(0, |k| limb(&v.limbs, k));
        let mut q_limbs = vec![0u32; m.saturating_add(1)];

        for j in (0..=m).rev() {
            // Estimate one quotient limb from the top two limbs of what is left
            // of the dividend. `u_hi < LIMB_BASE`, so `dividend < 10^18`.
            let idx = j.saturating_add(n);
            let u_hi = limb(&u_limbs, idx);
            let u_mid = idx.checked_sub(1).map_or(0, |k| limb(&u_limbs, k));
            let u_third = idx.checked_sub(2).map_or(0, |k| limb(&u_limbs, k));
            let dividend = u_hi.saturating_mul(LIMB_BASE).saturating_add(u_mid);
            let mut q_hat = dividend.checked_div(v_top).unwrap_or(0);
            let mut r_hat = dividend.checked_rem(v_top).unwrap_or(0);

            // Knuth's correction: while the estimate is provably too large,
            // walk it down. It runs at most twice.
            loop {
                let too_big = q_hat >= LIMB_BASE
                    || q_hat.saturating_mul(v_second)
                        > r_hat.saturating_mul(LIMB_BASE).saturating_add(u_third);
                if !too_big {
                    break;
                }
                q_hat = q_hat.saturating_sub(1);
                r_hat = r_hat.saturating_add(v_top);
                if r_hat >= LIMB_BASE {
                    break;
                }
            }

            // Multiply the divisor by the estimate and subtract it out.
            let mut borrow: i64 = 0;
            for i in 0..n {
                let product = q_hat.saturating_mul(limb(&v.limbs, i));
                let (product_hi, product_lo) = split(product);
                let Some(slot) = j.checked_add(i).and_then(|k| u_limbs.get_mut(k)) else {
                    continue;
                };
                let cur = i64::from(*slot)
                    .saturating_sub(i64::from(product_lo))
                    .saturating_sub(borrow);
                if cur < 0 {
                    *slot = cur.saturating_add(LIMB_BASE as i64) as u32;
                    borrow = (product_hi as i64).saturating_add(1);
                } else {
                    *slot = cur as u32;
                    borrow = product_hi as i64;
                }
            }

            // If the subtraction went negative the estimate was one too large:
            // give a unit back and add the divisor in again.
            let top = j.saturating_add(n);
            if let Some(slot) = u_limbs.get_mut(top) {
                let cur = i64::from(*slot).saturating_sub(borrow);
                if cur < 0 {
                    *slot = cur.saturating_add(LIMB_BASE as i64) as u32;
                    q_hat = q_hat.saturating_sub(1);
                    let mut carry: u64 = 0;
                    for i in 0..n {
                        let vi = limb(&v.limbs, i);
                        let Some(slot) = j.checked_add(i).and_then(|k| u_limbs.get_mut(k)) else {
                            continue;
                        };
                        let sum = u64::from(*slot).saturating_add(vi).saturating_add(carry);
                        let (next, low) = split(sum);
                        *slot = low;
                        carry = next;
                    }
                    if let Some(slot) = u_limbs.get_mut(top) {
                        *slot = (u64::from(*slot).saturating_add(carry)) as u32;
                    }
                } else {
                    *slot = cur as u32;
                }
            }
            if let Some(slot) = q_limbs.get_mut(j) {
                *slot = q_hat as u32;
            }
        }

        // Unscale remainder.
        let rem_big = Self {
            negative: false,
            limbs: u_limbs.get(..n).unwrap_or(&u_limbs).to_vec(),
        };
        let (mut remainder, _) = rem_big.div_limb(scale);
        remainder.negative = self.negative;
        remainder.normalize();

        let mut quotient = Self {
            negative: self.negative != other.negative,
            limbs: q_limbs,
        };
        quotient.normalize();
        (quotient, remainder)
    }

    /// Parse a decimal string into a `BigInt`.
    ///
    /// Anything that is not a digit is skipped, so this is a *lenient* parse
    /// and the caller must have validated the string first. `expr` does, and
    /// has to: `expr abc + 1` is an error there, not `1`.
    // Not `FromStr`: that trait's `Result` promises the parse validates, and
    // this one deliberately does not — it is the fast path for a string a
    // lexer has already checked. Giving it a `Result<Self, Infallible>` to
    // satisfy the lint would advertise an error case that does not exist.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        Self::from_str_radix(s, 10)
    }

    /// Parse a string in the given radix (2..=16).
    pub fn from_str_radix(s: &str, radix: u32) -> Self {
        let s = s.trim();
        if s.is_empty() {
            return Self::zero();
        }
        let (negative, digits) = if let Some(rest) = s.strip_prefix('-') {
            (true, rest)
        } else {
            (false, s)
        };
        if digits.is_empty() {
            return Self::zero();
        }

        // For base 10, parse groups of LIMB_DIGITS directly.
        if radix == 10 {
            let bytes = digits.as_bytes();
            let mut limbs = Vec::new();
            // Nine digits at a time from the least significant end — the whole
            // reason the base is a power of ten.
            let mut i = bytes.len();
            while i > 0 {
                let start = i.saturating_sub(LIMB_DIGITS);
                let mut val: u32 = 0;
                for &b in bytes.get(start..i).unwrap_or_default() {
                    // At most nine digits, so this stays under 10^9.
                    val = val
                        .saturating_mul(10)
                        .saturating_add(u32::from(b.wrapping_sub(b'0')));
                }
                limbs.push(val);
                i = start;
            }
            let mut r = Self { negative, limbs };
            r.normalize();
            return r;
        }

        // General radix: multiply-and-add.
        let mut result = Self::zero();
        let base_big = Self::from_i64(i64::from(radix));
        for &b in digits.as_bytes() {
            let d = char_to_digit(b);
            result = result.mul(&base_big).add(&Self::from_i64(i64::from(d)));
        }
        result.negative = negative;
        result.normalize();
        result
    }

    /// Convert to string in the given radix.
    pub fn to_str_radix(&self, radix: u32) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        if radix == 10 {
            return self.to_string_base10();
        }
        // General radix: repeated division.
        let mut digits = Vec::new();
        let mut tmp = self.clone();
        tmp.negative = false;
        let base_big = Self::from_i64(i64::from(radix));
        while !tmp.is_zero() {
            let (q, r) = tmp.divmod(&base_big);
            // The remainder is below the radix, so it is one limb.
            let d = r.limbs.first().copied().unwrap_or(0);
            digits.push(digit_to_char(d));
            tmp = q;
        }
        if self.negative {
            digits.push('-');
        }
        digits.reverse();
        digits.into_iter().collect()
    }

    pub fn to_string_base10(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let Some((top, rest)) = self.limbs.split_last() else {
            return "0".to_string();
        };
        // The most significant limb prints without padding; every one below it
        // is exactly nine digits, because that is what a limb holds.
        let mut s = top.to_string();
        for l in rest.iter().rev() {
            s.push_str(&format!("{l:0>LIMB_DIGITS$}"));
        }
        if self.negative && !self.is_zero() {
            s.insert(0, '-');
        }
        s
    }

    /// Shift left by `n` decimal digits (multiply by 10^n).
    pub fn shift_left_decimal(&self, n: usize) -> Self {
        if n == 0 || self.is_zero() {
            return self.clone();
        }
        let ten = Self::from_i64(10);
        let mut result = self.clone();
        for _ in 0..n {
            result = result.mul(&ten);
        }
        result
    }

    /// Power: self^exp (exp must be non-negative).
    pub fn pow(&self, exp: &Self) -> Self {
        if exp.negative {
            return Self::zero();
        }
        if exp.is_zero() {
            return Self::one();
        }
        let mut base = self.clone();
        let mut result = Self::one();
        let mut e = exp.clone();
        let two = Self::from_i64(2);
        loop {
            if e.is_zero() {
                break;
            }
            let (half, rem) = e.divmod(&two);
            if !rem.is_zero() {
                result = result.mul(&base);
            }
            base = base.mul(&base);
            e = half;
        }
        result
    }

    /// Integer square root (Newton's method).
    pub fn isqrt(&self) -> Self {
        if self.negative || self.is_zero() {
            return Self::zero();
        }
        if self.cmp_mag(&Self::one()) == 0 {
            return Self::one();
        }
        // Initial guess: half the number of digits. Every limb below the top
        // contributes exactly LIMB_DIGITS; the top one contributes however many
        // it is written with.
        let digit_count = self
            .limbs
            .len()
            .saturating_sub(1)
            .saturating_mul(LIMB_DIGITS)
            .saturating_add(self.limbs.last().map_or(1, |l| {
                let mut d = 0usize;
                let mut v = *l;
                while v > 0 {
                    d = d.saturating_add(1);
                    v = v.checked_div(10).unwrap_or(0);
                }
                d.max(1)
            }));
        let half_digits = digit_count.div_ceil(2);
        // Start with 10^half_digits as initial guess.
        let mut guess = Self::one().shift_left_decimal(half_digits);

        loop {
            let (div, _) = self.divmod(&guess);
            let sum = guess.add(&div);
            let two = Self::from_i64(2);
            let (new_guess, _) = sum.divmod(&two);

            // If new_guess >= guess, we are done.
            if new_guess.cmp_mag(&guess) >= 0 {
                break;
            }
            guess = new_guess;
        }
        guess
    }
}

/// A digit's value in bases up to 16, or `0` for a byte that is not one.
///
/// The lenient `0` matches [`BigInt::from_str_radix`]'s contract: the caller has
/// already decided the string is a number.
pub fn char_to_digit(b: u8) -> u32 {
    match b {
        b'0'..=b'9' => u32::from(b.wrapping_sub(b'0')),
        b'A'..=b'F' => u32::from(b.wrapping_sub(b'A')).saturating_add(10),
        b'a'..=b'f' => u32::from(b.wrapping_sub(b'a')).saturating_add(10),
        _ => 0,
    }
}

/// The digit for a value in bases up to 16, upper case.
pub fn digit_to_char(d: u32) -> char {
    let ascii = if d < 10 {
        b'0'.saturating_add(d as u8)
    } else {
        b'A'.saturating_add(d.saturating_sub(10) as u8)
    };
    char::from(ascii)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::BigInt;

    #[test]
    fn test_bigint_zero() {
        let z = BigInt::zero();
        assert!(z.is_zero());
        assert_eq!(z.to_string_base10(), "0");
    }

    #[test]
    fn test_bigint_from_i64() {
        let n = BigInt::from_i64(12345);
        assert_eq!(n.to_string_base10(), "12345");
        let neg = BigInt::from_i64(-42);
        assert_eq!(neg.to_string_base10(), "-42");
    }

    #[test]
    fn test_bigint_from_str() {
        let n = BigInt::from_str("999999999999999999");
        assert_eq!(n.to_string_base10(), "999999999999999999");
    }

    #[test]
    fn test_bigint_add() {
        let a = BigInt::from_str("999999999");
        let b = BigInt::from_str("1");
        assert_eq!(a.add(&b).to_string_base10(), "1000000000");
    }

    #[test]
    fn test_bigint_add_large() {
        let a = BigInt::from_str("123456789012345678901234567890");
        let b = BigInt::from_str("987654321098765432109876543210");
        let sum = a.add(&b);
        assert_eq!(sum.to_string_base10(), "1111111110111111111011111111100");
    }

    #[test]
    fn test_bigint_sub() {
        let a = BigInt::from_str("1000000000");
        let b = BigInt::from_str("1");
        assert_eq!(a.sub(&b).to_string_base10(), "999999999");
    }

    #[test]
    fn test_bigint_sub_negative() {
        let a = BigInt::from_str("5");
        let b = BigInt::from_str("10");
        let r = a.sub(&b);
        assert_eq!(r.to_string_base10(), "-5");
    }

    #[test]
    fn test_bigint_mul() {
        let a = BigInt::from_str("12345");
        let b = BigInt::from_str("67890");
        assert_eq!(a.mul(&b).to_string_base10(), "838102050");
    }

    #[test]
    fn test_bigint_mul_large() {
        let a = BigInt::from_str("99999999999999999999");
        let b = BigInt::from_str("2");
        assert_eq!(a.mul(&b).to_string_base10(), "199999999999999999998");
    }

    #[test]
    fn test_bigint_div() {
        let a = BigInt::from_str("100");
        let b = BigInt::from_str("7");
        let (q, r) = a.divmod(&b);
        assert_eq!(q.to_string_base10(), "14");
        assert_eq!(r.to_string_base10(), "2");
    }

    #[test]
    fn test_bigint_div_large() {
        let a = BigInt::from_str("123456789012345678901234567890");
        let b = BigInt::from_str("1000000000");
        let (q, _) = a.divmod(&b);
        assert_eq!(q.to_string_base10(), "123456789012345678901");
    }

    #[test]
    fn test_bigint_pow() {
        let base = BigInt::from_str("2");
        let exp = BigInt::from_str("64");
        let result = base.pow(&exp);
        assert_eq!(result.to_string_base10(), "18446744073709551616");
    }

    #[test]
    fn test_bigint_radix_hex() {
        let n = BigInt::from_str_radix("FF", 16);
        assert_eq!(n.to_string_base10(), "255");
    }

    #[test]
    fn test_bigint_radix_binary() {
        let n = BigInt::from_str_radix("1010", 2);
        assert_eq!(n.to_string_base10(), "10");
    }

    #[test]
    fn test_bigint_to_hex() {
        let n = BigInt::from_str("255");
        assert_eq!(n.to_str_radix(16), "FF");
    }

    // --- the cases the callers actually depend on -------------------------

    #[test]
    fn division_truncates_toward_zero_as_c_does() {
        // Not floor division: -7/2 is -3, not -4. `expr` and `bc` both promise
        // C's rule, and the remainder has to agree with it or `a/b*b + a%b`
        // stops equalling `a`.
        for (a, b, q, r) in [
            ("7", "2", "3", "1"),
            ("-7", "2", "-3", "-1"),
            ("7", "-2", "-3", "1"),
            ("-7", "-2", "3", "-1"),
        ] {
            let (qq, rr) = BigInt::from_str(a).divmod(&BigInt::from_str(b));
            assert_eq!(qq.to_string_base10(), q, "{a} / {b}");
            assert_eq!(rr.to_string_base10(), r, "{a} % {b}");
        }
    }

    #[test]
    fn dividing_by_zero_yields_zero_rather_than_panicking() {
        // The callers all report this themselves, with their own wording and
        // their own exit status, so the library must not take the process down.
        let (q, r) = BigInt::from_str("5").divmod(&BigInt::zero());
        assert_eq!(q.to_string_base10(), "0");
        assert_eq!(r.to_string_base10(), "0");
    }

    #[test]
    fn a_value_past_the_range_of_i64_survives_a_round_trip() {
        // The whole reason `expr` uses this rather than i64: GNU expr answers
        // 9223372036854775808 here, and a wrapping i64 would answer -9223372036854775808.
        let n = BigInt::from_str("9223372036854775807").add(&BigInt::from_str("1"));
        assert_eq!(n.to_string_base10(), "9223372036854775808");
    }

    #[test]
    fn zero_has_one_spelling() {
        // -0 must normalise, or two values that compare equal would print
        // differently and `expr 0 = -0` would disagree with `expr 0`.
        let z = BigInt::from_str("0").sub(&BigInt::from_str("0"));
        assert!(!z.negative);
        assert_eq!(z.to_string_base10(), "0");
        let z2 = BigInt::from_str("5").mul(&BigInt::from_str("0"));
        assert_eq!(z2.to_string_base10(), "0");
    }

    /// The property that defines division, over a deterministic sweep of
    /// multi-limb operands.
    ///
    /// The single-limb divisor takes a fast path; everything wider goes through
    /// Knuth's algorithm D, which is where the estimate-and-correct step lives
    /// and where a bignum's division bugs are. The named tests above only ever
    /// reach it with round numbers, so this walks a few hundred shapes instead
    /// — different limb counts on each side, values that straddle the limb
    /// boundary, and both signs.
    #[test]
    fn division_reconstructs_its_dividend() {
        // A 64-bit xorshift, so the sweep is the same on every machine and a
        // failure is reproducible from the seed alone.
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let digits = |n: usize, rng: &mut dyn FnMut() -> u64| {
            let mut s = String::new();
            while s.len() < n {
                s.push_str(&format!("{:019}", rng()));
            }
            s.truncate(n);
            // A leading zero would make the value narrower than intended.
            s.replace_range(
                0..1,
                "1234567890"
                    .get(n % 10..)
                    .unwrap_or("9")
                    .get(..1)
                    .unwrap_or("9"),
            );
            s
        };

        for a_len in [1usize, 8, 9, 10, 17, 18, 19, 27, 40] {
            for b_len in [1usize, 9, 10, 18, 19, 25] {
                if b_len > a_len {
                    continue;
                }
                for signs in [(false, false), (true, false), (false, true), (true, true)] {
                    let (an, bn) = (digits(a_len, &mut next), digits(b_len, &mut next));
                    let a = BigInt::from_str(&format!("{}{an}", if signs.0 { "-" } else { "" }));
                    let b = BigInt::from_str(&format!("{}{bn}", if signs.1 { "-" } else { "" }));
                    let (q, r) = a.divmod(&b);

                    let back = q.mul(&b).add(&r);
                    assert_eq!(
                        back.to_string_base10(),
                        a.to_string_base10(),
                        "q*b + r != a for {} / {}",
                        a.to_string_base10(),
                        b.to_string_base10()
                    );
                    // |r| < |b|, and r takes the dividend's sign.
                    assert!(
                        r.cmp_mag(&b) < 0,
                        "|r| >= |b| for {} / {}",
                        a.to_string_base10(),
                        b.to_string_base10()
                    );
                    if !r.is_zero() {
                        assert_eq!(
                            r.negative,
                            a.negative,
                            "remainder sign for {} / {}",
                            a.to_string_base10(),
                            b.to_string_base10()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_carry_ripples_across_every_limb() {
        // Base 10^9 means limb boundaries fall every nine digits; a carry that
        // is dropped at one of them is the classic bignum bug.
        let a = BigInt::from_str("999999999999999999999999999");
        assert_eq!(
            a.add(&BigInt::from_str("1")).to_string_base10(),
            "1000000000000000000000000000"
        );
        assert_eq!(
            BigInt::from_str("1000000000000000000000000000")
                .sub(&BigInt::from_str("1"))
                .to_string_base10(),
            "999999999999999999999999999"
        );
    }
}
