//! Arbitrary-precision signed integers, in base-10^9 limbs.
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
//! ## What it does not do
//!
//! There is no `Add`/`Mul` operator implementation, on purpose: every method
//! here allocates, and an infix `a * b` in a loop hides that in a way
//! `a.mul(&b)` does not. Division by zero is the caller's to check — `divmod`
//! returns zero for it rather than panicking, because the callers all need to
//! report it in their own words and with their own exit status.

/// Base for each limb -- 10^9 fits comfortably in u32 and makes
/// decimal conversion trivial (each limb is exactly 9 decimal digits).
pub const LIMB_BASE: u64 = 1_000_000_000;
pub const LIMB_DIGITS: usize = 9;

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

    pub fn from_i64(mut v: i64) -> Self {
        let negative = v < 0;
        if negative {
            v = v.wrapping_neg();
        }
        let mut uv = v as u64;
        let mut limbs = Vec::new();
        if uv == 0 {
            limbs.push(0);
        } else {
            while uv > 0 {
                limbs.push((uv % LIMB_BASE) as u32);
                uv /= LIMB_BASE;
            }
        }
        Self { negative, limbs }
    }

    /// Remove leading zero limbs, keeping at least one.
    pub fn normalize(&mut self) {
        while self.limbs.len() > 1 && *self.limbs.last().unwrap_or(&1) == 0 {
            self.limbs.pop();
        }
        if self.is_zero() {
            self.negative = false;
        }
    }

    /// Compare magnitudes: 1 if |self| > |other|, -1 if <, 0 if equal.
    pub fn cmp_mag(&self, other: &Self) -> i32 {
        let a = &self.limbs;
        let b = &other.limbs;
        if a.len() != b.len() {
            return if a.len() > b.len() { 1 } else { -1 };
        }
        for i in (0..a.len()).rev() {
            if a[i] != b[i] {
                return if a[i] > b[i] { 1 } else { -1 };
            }
        }
        0
    }

    /// Add magnitudes, result is unsigned (caller sets sign).
    pub fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut result = Vec::with_capacity(a.len().max(b.len()) + 1);
        let mut carry: u64 = 0;
        let len = a.len().max(b.len());
        for i in 0..len {
            let va = if i < a.len() { a[i] as u64 } else { 0 };
            let vb = if i < b.len() { b[i] as u64 } else { 0 };
            let sum = va + vb + carry;
            result.push((sum % LIMB_BASE) as u32);
            carry = sum / LIMB_BASE;
        }
        if carry > 0 {
            result.push(carry as u32);
        }
        result
    }

    /// Subtract magnitudes (|a| >= |b| required).
    pub fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut result = Vec::with_capacity(a.len());
        let mut borrow: i64 = 0;
        for i in 0..a.len() {
            let va = a[i] as i64;
            let vb = if i < b.len() { b[i] as i64 } else { 0 };
            let mut diff = va - vb - borrow;
            if diff < 0 {
                diff += LIMB_BASE as i64;
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
    pub fn mul(&self, other: &Self) -> Self {
        let a = &self.limbs;
        let b = &other.limbs;
        let mut result = vec![0u64; a.len() + b.len()];
        for i in 0..a.len() {
            let mut carry: u64 = 0;
            for j in 0..b.len() {
                let prod = a[i] as u64 * b[j] as u64 + result[i + j] + carry;
                result[i + j] = prod % LIMB_BASE;
                carry = prod / LIMB_BASE;
            }
            result[i + b.len()] += carry;
        }
        let limbs: Vec<u32> = result.iter().map(|&v| v as u32).collect();
        let mut r = Self {
            negative: self.negative != other.negative,
            limbs,
        };
        r.normalize();
        r
    }

    /// Multiply by a single limb.
    pub fn mul_limb(&self, v: u32) -> Self {
        let mut result = Vec::with_capacity(self.limbs.len() + 1);
        let mut carry: u64 = 0;
        for &l in &self.limbs {
            let prod = l as u64 * v as u64 + carry;
            result.push((prod % LIMB_BASE) as u32);
            carry = prod / LIMB_BASE;
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
    pub fn div_limb(&self, d: u32) -> (Self, u32) {
        let mut quotient = vec![0u32; self.limbs.len()];
        let mut rem: u64 = 0;
        for i in (0..self.limbs.len()).rev() {
            let cur = rem * LIMB_BASE + self.limbs[i] as u64;
            quotient[i] = (cur / d as u64) as u32;
            rem = cur % d as u64;
        }
        let mut q = Self {
            negative: self.negative,
            limbs: quotient,
        };
        q.normalize();
        (q, rem as u32)
    }

    /// Long division: returns (quotient, remainder).  Panics on zero divisor.
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
        if other.limbs.len() == 1 {
            let (mut q, r) = self.div_limb(other.limbs[0]);
            q.negative = self.negative != other.negative;
            q.normalize();
            let mut rem = Self::from_i64(r as i64);
            rem.negative = self.negative;
            rem.normalize();
            return (q, rem);
        }

        // Knuth Algorithm D (simplified).
        let n = other.limbs.len();
        let m = self.limbs.len() - n;

        // Normalize: scale both so that divisor's top limb >= LIMB_BASE/2.
        let d_top = *other.limbs.last().unwrap_or(&1) as u64;
        let scale = (LIMB_BASE / (d_top + 1)) as u32;

        let u = self.mul_limb(scale);
        let v = other.mul_limb(scale);

        let mut u_limbs = u.limbs.clone();
        while u_limbs.len() <= m + n {
            u_limbs.push(0);
        }

        let v_top = *v.limbs.last().unwrap_or(&1) as u64;
        let mut q_limbs = vec![0u32; m + 1];

        for j in (0..=m).rev() {
            // Estimate q_hat.
            let idx = j + n;
            let u_hi = if idx < u_limbs.len() {
                u_limbs[idx] as u64
            } else {
                0
            };
            let u_mid = if idx >= 1 && idx - 1 < u_limbs.len() {
                u_limbs[idx - 1] as u64
            } else {
                0
            };
            let dividend = u_hi * LIMB_BASE + u_mid;
            let mut q_hat = dividend / v_top;
            let mut r_hat = dividend % v_top;

            let v_second = if v.limbs.len() >= 2 {
                v.limbs[v.limbs.len() - 2] as u64
            } else {
                0
            };
            let u_third = if idx >= 2 && idx - 2 < u_limbs.len() {
                u_limbs[idx - 2] as u64
            } else {
                0
            };

            loop {
                if q_hat >= LIMB_BASE || q_hat * v_second > LIMB_BASE * r_hat + u_third {
                    q_hat -= 1;
                    r_hat += v_top;
                    if r_hat < LIMB_BASE {
                        continue;
                    }
                }
                break;
            }

            // Multiply and subtract.
            let mut borrow: i64 = 0;
            for i in 0..n {
                let prod = q_hat * v.limbs[i] as u64;
                let idx2 = j + i;
                if idx2 < u_limbs.len() {
                    let cur = u_limbs[idx2] as i64 - (prod % LIMB_BASE) as i64 - borrow;
                    if cur < 0 {
                        u_limbs[idx2] = (cur + LIMB_BASE as i64) as u32;
                        borrow = prod as i64 / LIMB_BASE as i64 + 1;
                    } else {
                        u_limbs[idx2] = cur as u32;
                        borrow = prod as i64 / LIMB_BASE as i64;
                    }
                }
            }
            if j + n < u_limbs.len() {
                let cur = u_limbs[j + n] as i64 - borrow;
                if cur < 0 {
                    u_limbs[j + n] = (cur + LIMB_BASE as i64) as u32;
                    // Need to add back.
                    q_hat -= 1;
                    let mut carry: u64 = 0;
                    for i in 0..n {
                        let idx2 = j + i;
                        if idx2 < u_limbs.len() {
                            let sum = u_limbs[idx2] as u64 + v.limbs[i] as u64 + carry;
                            u_limbs[idx2] = (sum % LIMB_BASE) as u32;
                            carry = sum / LIMB_BASE;
                        }
                    }
                    if j + n < u_limbs.len() {
                        u_limbs[j + n] = (u_limbs[j + n] as u64 + carry) as u32;
                    }
                } else {
                    u_limbs[j + n] = cur as u32;
                }
            }
            q_limbs[j] = q_hat as u32;
        }

        // Unscale remainder.
        let rem_limbs: Vec<u32> = u_limbs[..n].to_vec();
        let rem_big = Self {
            negative: false,
            limbs: rem_limbs,
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
            let mut i = bytes.len();
            while i > 0 {
                let start = i.saturating_sub(LIMB_DIGITS);
                let chunk = &bytes[start..i];
                let mut val: u32 = 0;
                for &b in chunk {
                    val = val * 10 + (b.wrapping_sub(b'0')) as u32;
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
        let base_big = Self::from_i64(radix as i64);
        for &b in digits.as_bytes() {
            let d = char_to_digit(b);
            result = result.mul(&base_big).add(&Self::from_i64(d as i64));
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
        let base_big = Self::from_i64(radix as i64);
        while !tmp.is_zero() {
            let (q, r) = tmp.divmod(&base_big);
            let d = if r.is_zero() {
                0u32
            } else {
                r.limbs[0]
            };
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
        let mut s = String::new();
        // Top limb: no leading zeros.
        let top = self.limbs.len() - 1;
        s.push_str(&self.limbs[top].to_string());
        // Remaining limbs: zero-padded to LIMB_DIGITS.
        for i in (0..top).rev() {
            let chunk = format!("{:0>width$}", self.limbs[i], width = LIMB_DIGITS);
            s.push_str(&chunk);
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
        // Initial guess: half the number of digits.
        let digit_count = (self.limbs.len() - 1) * LIMB_DIGITS
            + self.limbs.last().map_or(1, |l| {
                let mut d = 0usize;
                let mut v = *l;
                while v > 0 {
                    d += 1;
                    v /= 10;
                }
                d.max(1)
            });
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

pub fn char_to_digit(b: u8) -> u32 {
    match b {
        b'0'..=b'9' => (b - b'0') as u32,
        b'A'..=b'F' => (b - b'A' + 10) as u32,
        b'a'..=b'f' => (b - b'a' + 10) as u32,
        _ => 0,
    }
}

pub fn digit_to_char(d: u32) -> char {
    if d < 10 {
        (b'0' + d as u8) as char
    } else {
        (b'A' + (d - 10) as u8) as char
    }
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
        for (a, b, q, r) in [("7", "2", "3", "1"), ("-7", "2", "-3", "-1"),
                             ("7", "-2", "-3", "1"), ("-7", "-2", "3", "-1")] {
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

    #[test]
    fn a_carry_ripples_across_every_limb() {
        // Base 10^9 means limb boundaries fall every nine digits; a carry that
        // is dropped at one of them is the classic bignum bug.
        let a = BigInt::from_str("999999999999999999999999999");
        assert_eq!(a.add(&BigInt::from_str("1")).to_string_base10(),
                   "1000000000000000000000000000");
        assert_eq!(BigInt::from_str("1000000000000000000000000000")
                       .sub(&BigInt::from_str("1"))
                       .to_string_base10(),
                   "999999999999999999999999999");
    }
}
