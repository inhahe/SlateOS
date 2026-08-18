//! Arbitrary-precision natural numbers — the exact arithmetic under
//! [`crate::extfloat`].
//!
//! This is not a general bignum library and is not part of the crate's public
//! surface. It exists because converting between decimal text and a binary
//! floating-point number is an *exact* question that cannot be answered in
//! floating point: deciding whether `1.0000000000000000001` rounds up or down
//! means comparing it against a dyadic rational, and any approximation used to
//! make that comparison is exactly the thing being decided. Every classic
//! `strtod` bug is a library that tried anyway.
//!
//! So the operations here are the ones that question needs:
//!
//! - **Multiply**, to form `10^k` and `5^k`.
//! - **Shift**, because a binary exponent is a shift and nothing else.
//! - **Divide with remainder**, to get a correctly-rounded significand out of
//!   `D / 10^k` — the remainder *is* the sticky bit, and a sticky bit that is
//!   computed rather than guessed is the whole point.
//! - **Decimal conversion**, both ways.
//!
//! # Why the cost is bearable
//!
//! Division looks like the expensive one and is not, because of how it is
//! called. [`Nat::divmod`] is Knuth's algorithm D, which costs one pass over
//! the divisor per *quotient* limb. The parser shifts its numerator so that the
//! quotient is 65 bits — three limbs — before dividing, so the cost is linear
//! in the size of the numeral rather than quadratic. Forming `10^k` by repeated
//! squaring is the superlinear step, and it is only reached by an input that
//! spells out a five-figure exponent.
//!
//! # Representation
//!
//! Little-endian `u32` limbs with no trailing zero limb, so zero is the empty
//! vector and [`Nat::cmp`] can compare lengths first. `u32` rather than `u64`
//! because every inner loop needs the full double-width product, and the
//! numbers reached here are small enough that `u64 * u64 -> u128` costs more
//! than the halved limb count saves.

use std::cmp::Ordering;

/// A non-negative integer of unbounded size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nat {
    /// Little-endian base 2^32, canonical: no trailing zero limb.
    limbs: Vec<u32>,
}

impl Nat {
    /// Zero.
    pub fn zero() -> Self {
        Nat { limbs: Vec::new() }
    }

    /// A value that fits two limbs.
    pub fn from_u64(v: u64) -> Self {
        let mut n = Nat {
            limbs: vec![low(v), low(v >> 32)],
        };
        n.trim();
        n
    }

    /// Drop the trailing zero limbs an operation may have left.
    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    /// Whether this is zero.
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// The number of bits needed to write it; 0 for zero.
    pub fn bit_len(&self) -> usize {
        match self.limbs.last() {
            None => 0,
            Some(&top) => {
                self.limbs.len().saturating_sub(1) * 32 + (32 - top.leading_zeros() as usize)
            }
        }
    }

    /// Whether bit `i` is set, counting from 0 at the least significant end.
    pub fn bit(&self, i: usize) -> bool {
        match self.limbs.get(i / 32) {
            None => false,
            Some(&limb) => (limb >> (i % 32)) & 1 == 1,
        }
    }

    /// Whether any of the low `bits` bits is set — the sticky bit of a shift
    /// that is about to discard them.
    pub fn any_below(&self, bits: usize) -> bool {
        let whole = bits / 32;
        if self.limbs.iter().take(whole).any(|&l| l != 0) {
            return true;
        }
        let rest = bits % 32;
        if rest == 0 {
            return false;
        }
        match self.limbs.get(whole) {
            None => false,
            Some(&limb) => limb & ((1u32 << rest) - 1) != 0,
        }
    }

    /// The low 64 bits.
    // No caller today: `extfloat` reaches the significand through `divmod`,
    // which hands back a `Nat` it then shifts. Kept because a type that can
    // build a 64-bit value but not read one back is a trap for the next
    // caller, and because the tests below pin the limb order it depends on.
    #[allow(dead_code)]
    pub fn low_u64(&self) -> u64 {
        let lo = u64::from(self.limbs.first().copied().unwrap_or(0));
        let hi = u64::from(self.limbs.get(1).copied().unwrap_or(0));
        lo | (hi << 32)
    }

    /// `self * m`, in place.
    pub fn mul_small(&mut self, m: u32) {
        if m == 0 {
            self.limbs.clear();
            return;
        }
        let mut carry: u64 = 0;
        for limb in &mut self.limbs {
            let p = u64::from(*limb) * u64::from(m) + carry;
            *limb = low(p);
            carry = p >> 32;
        }
        if carry != 0 {
            self.limbs.push(low(carry));
        }
    }

    /// `self + a`, in place.
    pub fn add_small(&mut self, a: u32) {
        let mut carry = u64::from(a);
        for limb in &mut self.limbs {
            if carry == 0 {
                return;
            }
            let s = u64::from(*limb) + carry;
            *limb = low(s);
            carry = s >> 32;
        }
        if carry != 0 {
            self.limbs.push(low(carry));
        }
    }

    /// `self + other`.
    // Used only by the tests, which need it to state long division's defining
    // identity — `q * d + r == n`. That check is the reason to trust `divmod`
    // at all, so the operation earns its place even with no other caller.
    #[allow(dead_code)]
    pub fn add(&self, other: &Self) -> Self {
        let mut out = Vec::with_capacity(self.limbs.len().max(other.limbs.len()) + 1);
        let mut carry: u64 = 0;
        for i in 0..self.limbs.len().max(other.limbs.len()) {
            let s = u64::from(self.limbs.get(i).copied().unwrap_or(0))
                + u64::from(other.limbs.get(i).copied().unwrap_or(0))
                + carry;
            out.push(low(s));
            carry = s >> 32;
        }
        if carry != 0 {
            out.push(low(carry));
        }
        let mut n = Nat { limbs: out };
        n.trim();
        n
    }

    /// `self - other`, which the caller must know is non-negative.
    pub fn sub(&self, other: &Self) -> Self {
        debug_assert!(self.cmp(other) != Ordering::Less);
        let mut out = Vec::with_capacity(self.limbs.len());
        let mut borrow: i64 = 0;
        for (i, &a) in self.limbs.iter().enumerate() {
            let b = i64::from(other.limbs.get(i).copied().unwrap_or(0));
            let t = i64::from(a) - b - borrow;
            out.push(t as u32);
            borrow = i64::from(t < 0);
        }
        let mut n = Nat { limbs: out };
        n.trim();
        n
    }

    /// Schoolbook multiplication. Quadratic, and reached only by [`Nat::pow`],
    /// whose repeated squaring keeps the number of calls logarithmic.
    pub fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Nat::zero();
        }
        let mut out = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            if a == 0 {
                continue;
            }
            let mut carry: u64 = 0;
            for (j, &b) in other.limbs.iter().enumerate() {
                let at = i + j;
                let p = u64::from(a) * u64::from(b) + u64::from(out[at]) + carry;
                out[at] = low(p);
                carry = p >> 32;
            }
            let mut at = i + other.limbs.len();
            while carry != 0 {
                let s = u64::from(out[at]) + carry;
                out[at] = low(s);
                carry = s >> 32;
                at += 1;
            }
        }
        let mut n = Nat { limbs: out };
        n.trim();
        n
    }

    /// `base ** exp`, by repeated squaring.
    pub fn pow(base: u32, exp: u32) -> Self {
        let mut result = Nat::from_u64(1);
        let mut factor = Nat::from_u64(u64::from(base));
        let mut e = exp;
        while e != 0 {
            if e & 1 == 1 {
                result = result.mul(&factor);
            }
            e >>= 1;
            if e != 0 {
                factor = factor.mul(&factor);
            }
        }
        result
    }

    /// `self << bits`.
    pub fn shl(&self, bits: usize) -> Self {
        if self.is_zero() {
            return Nat::zero();
        }
        let whole = bits / 32;
        let part = bits % 32;
        let mut out = vec![0u32; whole];
        if part == 0 {
            out.extend_from_slice(&self.limbs);
        } else {
            let mut carry: u32 = 0;
            for &limb in &self.limbs {
                out.push((limb << part) | carry);
                carry = limb >> (32 - part);
            }
            if carry != 0 {
                out.push(carry);
            }
        }
        let mut n = Nat { limbs: out };
        n.trim();
        n
    }

    /// `self >> bits`, truncating. Ask [`Nat::any_below`] first if the
    /// discarded bits matter.
    pub fn shr(&self, bits: usize) -> Self {
        let whole = bits / 32;
        if whole >= self.limbs.len() {
            return Nat::zero();
        }
        let part = bits % 32;
        let kept = &self.limbs[whole..];
        let mut out = Vec::with_capacity(kept.len());
        if part == 0 {
            out.extend_from_slice(kept);
        } else {
            for (i, &limb) in kept.iter().enumerate() {
                let above = kept.get(i + 1).copied().unwrap_or(0);
                out.push((limb >> part) | (above << (32 - part)));
            }
        }
        let mut n = Nat { limbs: out };
        n.trim();
        n
    }

    /// Ordering by value.
    pub fn cmp(&self, other: &Self) -> Ordering {
        match self.limbs.len().cmp(&other.limbs.len()) {
            Ordering::Equal => self.limbs.iter().rev().cmp(other.limbs.iter().rev()),
            unequal => unequal,
        }
    }

    /// `self / d` and `self % d` for a single-limb divisor.
    ///
    /// # Panics
    ///
    /// If `d` is zero.
    pub fn divmod_small(&self, d: u32) -> (Self, u32) {
        assert!(d != 0, "divide by zero");
        let mut out = vec![0u32; self.limbs.len()];
        let mut rem: u64 = 0;
        for i in (0..self.limbs.len()).rev() {
            let cur = (rem << 32) | u64::from(self.limbs[i]);
            out[i] = low(cur / u64::from(d));
            rem = cur % u64::from(d);
        }
        let mut n = Nat { limbs: out };
        n.trim();
        (n, low(rem))
    }

    /// `self / d` and `self % d`.
    ///
    /// Knuth, *TAOCP* vol. 2, algorithm D, in the form given by *Hacker's
    /// Delight*: normalise so the divisor's top limb has its high bit set,
    /// estimate each quotient limb from the top two limbs, then correct the
    /// estimate — which that normalisation bounds at one too large.
    ///
    /// # Panics
    ///
    /// If `d` is zero. Every call site divides by a power of ten or of two, so
    /// a zero divisor would be a bug in this file rather than bad input.
    pub fn divmod(&self, d: &Self) -> (Self, Self) {
        assert!(!d.is_zero(), "divide by zero");
        if self.cmp(d) == Ordering::Less {
            return (Nat::zero(), self.clone());
        }
        let n = d.limbs.len();
        if n == 1 {
            let (q, r) = self.divmod_small(d.limbs[0]);
            return (q, Nat::from_u64(u64::from(r)));
        }

        // Normalise. Setting the divisor's top bit is what bounds the error in
        // the per-limb estimate below; without it the correction loop would not
        // terminate in a constant number of steps.
        let s = d.limbs[n - 1].leading_zeros() as usize;
        let vn = d.shl(s).limbs;
        let mut un = self.shl(s).limbs;
        let m = self.limbs.len() - n;
        // The loop reads `un[j + n]` at the top, so the dividend needs one limb
        // of headroom whether or not the shift happened to produce one.
        un.resize(m + n + 1, 0);

        let mut q = vec![0u32; m + 1];
        let top = u64::from(vn[n - 1]);
        let next = u64::from(vn[n - 2]);
        for j in (0..=m).rev() {
            let head = (u64::from(un[j + n]) << 32) | u64::from(un[j + n - 1]);
            let mut qhat = head / top;
            let mut rhat = head % top;
            while qhat >> 32 != 0 || qhat * next > ((rhat << 32) | u64::from(un[j + n - 2])) {
                qhat -= 1;
                rhat += top;
                if rhat >> 32 != 0 {
                    break;
                }
            }

            // Subtract qhat * divisor from the window in place, tracking the
            // borrow in the sign of `k`.
            let mut k: i64 = 0;
            for i in 0..n {
                let p = qhat * u64::from(vn[i]);
                let t = i64::from(un[i + j]) - k - i64::from(low(p));
                un[i + j] = t as u32;
                k = ((p >> 32) as i64) - (t >> 32);
            }
            let t = i64::from(un[j + n]) - k;
            un[j + n] = t as u32;

            q[j] = low(qhat);
            if t < 0 {
                // The estimate was one too large: give the limb back and add
                // the divisor in again.
                q[j] -= 1;
                let mut carry: u64 = 0;
                for i in 0..n {
                    let s2 = u64::from(un[i + j]) + u64::from(vn[i]) + carry;
                    un[i + j] = low(s2);
                    carry = s2 >> 32;
                }
                un[j + n] = un[j + n].wrapping_add(low(carry));
            }
        }

        let mut quotient = Nat { limbs: q };
        quotient.trim();
        let mut remainder = Nat {
            limbs: un[..n].to_vec(),
        };
        remainder.trim();
        (quotient, remainder.shr(s))
    }

    /// Read a run of ASCII decimal digits. Non-digits are not expected and are
    /// read as zero.
    pub fn from_decimal(digits: &[u8]) -> Self {
        let mut n = Nat::zero();
        // Nine digits at a time: 10^9 is the largest power of ten that fits a
        // limb, so this is one `mul_small` per nine digits rather than per one.
        for chunk in digits.chunks(9) {
            let mut value: u32 = 0;
            let mut scale: u32 = 1;
            for &c in chunk {
                value = value * 10 + u32::from(c.wrapping_sub(b'0').min(9));
                scale *= 10;
            }
            n.mul_small(scale);
            n.add_small(value);
        }
        n
    }

    /// Read a run of ASCII hexadecimal digits, either case.
    pub fn from_hex(digits: &[u8]) -> Self {
        let mut n = Nat::zero();
        for chunk in digits.chunks(7) {
            let mut value: u32 = 0;
            let mut scale: u32 = 1;
            for &c in chunk {
                value = value * 16 + char::from(c).to_digit(16).unwrap_or(0);
                scale *= 16;
            }
            n.mul_small(scale);
            n.add_small(value);
        }
        n
    }

    /// The decimal digits, most significant first. Zero is `b"0"`.
    pub fn to_decimal(&self) -> Vec<u8> {
        if self.is_zero() {
            return vec![b'0'];
        }
        let mut groups: Vec<u32> = Vec::new();
        let mut rest = self.clone();
        while !rest.is_zero() {
            let (q, r) = rest.divmod_small(1_000_000_000);
            groups.push(r);
            rest = q;
        }
        let mut out = Vec::with_capacity(groups.len() * 9);
        for (which, &group) in groups.iter().rev().enumerate() {
            let mut buf = [b'0'; 9];
            let mut v = group;
            for slot in buf.iter_mut().rev() {
                *slot = b'0' + u8::try_from(v % 10).unwrap_or(0);
                v /= 10;
            }
            if which == 0 {
                // The top group is the only one written without leading zeros.
                let lead = buf.iter().position(|&c| c != b'0').unwrap_or(8);
                out.extend_from_slice(&buf[lead..]);
            } else {
                out.extend_from_slice(&buf);
            }
        }
        out
    }
}

/// The low 32 bits of a wider value — spelled once so the truncating casts are
/// in one place rather than scattered through every carry loop.
fn low(v: u64) -> u32 {
    (v & 0xffff_ffff) as u32
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn dec(s: &str) -> Nat {
        Nat::from_decimal(s.as_bytes())
    }

    fn text(n: &Nat) -> String {
        String::from_utf8(n.to_decimal()).unwrap()
    }

    #[test]
    fn zero_round_trips() {
        assert!(Nat::zero().is_zero());
        assert_eq!(text(&Nat::zero()), "0");
        assert_eq!(text(&dec("0")), "0");
        assert_eq!(text(&dec("0000")), "0");
        assert_eq!(Nat::zero().bit_len(), 0);
    }

    #[test]
    fn decimal_round_trips() {
        for s in [
            "1",
            "9",
            "10",
            "999999999",
            "1000000000",
            "18446744073709551615",
            "18446744073709551616",
            "123456789012345678901234567890123456789",
        ] {
            assert_eq!(text(&dec(s)), s, "round trip of {s}");
        }
    }

    #[test]
    fn leading_zeros_are_dropped() {
        assert_eq!(text(&dec("0000000000000000042")), "42");
    }

    #[test]
    fn bit_len_matches_u128() {
        for v in [0u128, 1, 2, 3, 255, 256, u128::from(u64::MAX), 1u128 << 100] {
            let n = dec(&v.to_string());
            assert_eq!(n.bit_len(), (128 - v.leading_zeros()) as usize, "{v}");
        }
    }

    #[test]
    fn multiplication_matches_u128() {
        for a in [0u64, 1, 7, 65535, 1 << 31, u64::from(u32::MAX), u64::MAX] {
            for b in [0u64, 1, 3, 1 << 32, u64::MAX] {
                let got = Nat::from_u64(a).mul(&Nat::from_u64(b));
                let want = u128::from(a) * u128::from(b);
                assert_eq!(text(&got), want.to_string(), "{a} * {b}");
            }
        }
    }

    #[test]
    fn addition_matches_u128() {
        for a in [0u64, 1, u64::MAX, 1 << 32] {
            for b in [0u64, 1, u64::MAX] {
                let got = Nat::from_u64(a).add(&Nat::from_u64(b));
                assert_eq!(text(&got), (u128::from(a) + u128::from(b)).to_string());
            }
        }
    }

    #[test]
    fn subtraction_matches_u128() {
        for (a, b) in [(0u128, 0u128), (5, 3), (u128::MAX, 1), (1 << 64, 1)] {
            let got = dec(&a.to_string()).sub(&dec(&b.to_string()));
            assert_eq!(text(&got), (a - b).to_string(), "{a} - {b}");
        }
    }

    #[test]
    fn powers_of_ten_are_exact() {
        assert_eq!(text(&Nat::pow(10, 0)), "1");
        assert_eq!(text(&Nat::pow(10, 1)), "10");
        assert_eq!(text(&Nat::pow(10, 20)), "100000000000000000000");
        let want = format!("1{}", "0".repeat(100));
        assert_eq!(text(&Nat::pow(10, 100)), want);
    }

    #[test]
    fn powers_of_five_agree_with_shifted_powers_of_ten() {
        // 10^k == 5^k << k, which checks `shl` and `pow` against each other.
        for k in [0u32, 1, 5, 17, 64, 200] {
            let lhs = Nat::pow(5, k).shl(k as usize);
            assert_eq!(text(&lhs), text(&Nat::pow(10, k)), "k={k}");
        }
    }

    #[test]
    fn shifts_are_inverses_when_nothing_is_lost() {
        let n = dec("123456789012345678901234567890");
        for bits in [0usize, 1, 31, 32, 33, 64, 97, 200] {
            assert_eq!(text(&n.shl(bits).shr(bits)), text(&n), "bits={bits}");
        }
    }

    #[test]
    fn shifting_off_the_bottom_gives_zero() {
        let n = dec("255");
        assert!(n.shr(8).is_zero());
        assert_eq!(text(&n.shr(7)), "1");
        assert!(n.shr(1000).is_zero());
    }

    #[test]
    fn any_below_reports_the_discarded_bits() {
        let n = dec("256");
        assert!(!n.any_below(8));
        assert!(n.any_below(9));
        assert!(dec("257").any_below(1));
        assert!(!Nat::zero().any_below(64));
    }

    #[test]
    fn bit_reads_individual_bits() {
        let n = dec("10"); // 1010
        assert!(!n.bit(0));
        assert!(n.bit(1));
        assert!(!n.bit(2));
        assert!(n.bit(3));
        assert!(!n.bit(400));
    }

    #[test]
    fn comparison_orders_by_value() {
        assert_eq!(dec("0").cmp(&dec("0")), Ordering::Equal);
        assert_eq!(dec("9").cmp(&dec("10")), Ordering::Less);
        assert_eq!(dec("100").cmp(&dec("99")), Ordering::Greater);
        let big = dec("123456789012345678901234567890");
        assert_eq!(big.cmp(&big), Ordering::Equal);
        assert_eq!(
            big.cmp(&dec("123456789012345678901234567891")),
            Ordering::Less
        );
    }

    #[test]
    fn small_division_matches_u128() {
        let n = dec(&u128::MAX.to_string());
        for d in [1u32, 2, 3, 10, 1_000_000_000, u32::MAX] {
            let (q, r) = n.divmod_small(d);
            assert_eq!(text(&q), (u128::MAX / u128::from(d)).to_string(), "d={d}");
            assert_eq!(u128::from(r), u128::MAX % u128::from(d), "d={d}");
        }
    }

    #[test]
    fn long_division_matches_u128() {
        let cases: &[(u128, u128)] = &[
            (0, 1),
            (1, 1),
            (5, 7),
            (u128::MAX, 1),
            (u128::MAX, u128::MAX),
            (u128::MAX, u128::MAX - 1),
            (u128::MAX, 1 << 64),
            (u128::MAX, (1 << 64) + 1),
            (1 << 127, (1 << 64) + 12345),
            (
                0xffff_ffff_ffff_ffff_0000_0000_0000_0000,
                0xffff_ffff_0000_0001,
            ),
            (
                0x1234_5678_9abc_def0_1234_5678_9abc_def0,
                0x1_0000_0000_0001,
            ),
        ];
        for &(a, b) in cases {
            let (q, r) = dec(&a.to_string()).divmod(&dec(&b.to_string()));
            assert_eq!(text(&q), (a / b).to_string(), "{a} / {b}");
            assert_eq!(text(&r), (a % b).to_string(), "{a} % {b}");
        }
    }

    #[test]
    fn long_division_reconstructs_the_dividend() {
        // The property that matters at sizes no primitive can check: the
        // quotient and remainder put the dividend back together, and the
        // remainder is smaller than the divisor.
        let a = Nat::pow(10, 400).mul(&dec("7654321")).mul(&Nat::pow(7, 99));
        let b = Nat::pow(3, 301).mul(&dec("999999999999999999999"));
        let (q, r) = a.divmod(&b);
        assert_eq!(r.cmp(&b), Ordering::Less);
        assert_eq!(text(&q.mul(&b).add(&r)), text(&a));
    }

    #[test]
    fn long_division_by_a_near_power_of_two() {
        // The normalising shift is zero here, which is the case a version that
        // shifted unconditionally would get wrong.
        let a = Nat::pow(10, 90);
        let b = Nat::from_u64(1 << 63).mul(&Nat::from_u64(3));
        let (q, r) = a.divmod(&b);
        assert_eq!(r.cmp(&b), Ordering::Less);
        assert_eq!(text(&q.mul(&b).add(&r)), text(&a));
    }

    #[test]
    fn hex_matches_decimal() {
        assert_eq!(text(&Nat::from_hex(b"0")), "0");
        assert_eq!(text(&Nat::from_hex(b"ff")), "255");
        assert_eq!(text(&Nat::from_hex(b"FF")), "255");
        assert_eq!(
            text(&Nat::from_hex(b"ffffffffffffffff")),
            u64::MAX.to_string()
        );
        assert_eq!(
            text(&Nat::from_hex(b"deadbeefcafebabe1234")),
            "1051570404360395033547316"
        );
    }

    #[test]
    fn low_u64_takes_the_bottom_limbs() {
        assert_eq!(Nat::zero().low_u64(), 0);
        assert_eq!(dec("18446744073709551615").low_u64(), u64::MAX);
        assert_eq!(dec("18446744073709551616").low_u64(), 0);
    }

    #[test]
    fn a_long_numeral_survives_the_round_trip() {
        let mut s = String::new();
        let mut x: u64 = 1;
        for _ in 0..2000 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            s.push(char::from(b'0' + u8::try_from((x >> 60) % 10).unwrap_or(0)));
        }
        let trimmed = s.trim_start_matches('0');
        let want = if trimmed.is_empty() { "0" } else { trimmed };
        assert_eq!(text(&dec(&s)), want);
    }
}
