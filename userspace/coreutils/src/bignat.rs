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

use core::num::NonZeroU32;

/// The decimal grouping radix, non-zero in the type so `divmod_small` needs
/// no runtime check for it.
const BILLION: NonZeroU32 = match NonZeroU32::new(1_000_000_000) {
    Some(v) => v,
    None => unreachable!(),
};
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
            // The window starting at `i` IS the `out[i + j]` the inner loop
            // used to compute, and `zip` stops at the shorter of the two, so
            // the index that had to be checked no longer exists. `out` is
            // sized `self + other`, so the window always outruns `other`.
            let Some(window) = out.get_mut(i..) else {
                break;
            };
            let mut carry: u64 = 0;
            for (slot, &b) in window.iter_mut().zip(other.limbs.iter()) {
                // Widened to u64 first: (2^32-1)^2 + 2*(2^32-1) < 2^64, so
                // the product plus the running limb plus the carry cannot
                // overflow the accumulator.
                let p = u64::from(a) * u64::from(b) + u64::from(*slot) + carry;
                *slot = low(p);
                carry = p >> 32;
            }
            for slot in window.iter_mut().skip(other.limbs.len()) {
                if carry == 0 {
                    break;
                }
                let s = u64::from(*slot) + carry;
                *slot = low(s);
                carry = s >> 32;
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
        // `whole < self.limbs.len()` from the early return above.
        let Some(kept) = self.limbs.get(whole..) else {
            return Nat::zero();
        };
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
    pub fn divmod_small(&self, d: NonZeroU32) -> (Self, u32) {
        let d = u64::from(d.get());
        let mut out = vec![0u32; self.limbs.len()];
        let mut rem: u64 = 0;
        // Walking both from the top. The `out[i]` this replaces was only
        // safe because `out` was built at `self.limbs.len()`; zipping says
        // that instead of relying on it.
        for (slot, &limb) in out.iter_mut().rev().zip(self.limbs.iter().rev()) {
            let cur = (rem << 32) | u64::from(limb);
            *slot = low(cur / d);
            rem = cur % d;
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
    // `indexing_slicing` is allowed HERE and nowhere else in this module.
    // Three reasons, each of which can be checked against the code:
    //
    // 1. THE BOUNDS ARE THE ALGORITHM'S, not this code's. `un` is resized to
    //    `m + n + 1` and `j` runs `0..=m`, so `un[j + n]` is its last element
    //    and every other index sits below that. `vn` has exactly `n` limbs
    //    and `i` runs `0..n`.
    //
    // 2. THEY CANNOT BE MADE PROVABLE. `n` is a runtime value, so no slice
    //    type can carry "length at least n + 1" -- a window taken with
    //    `get_mut(j..)` still needs `window[n]`. The zip that removed exactly
    //    these indices from `mul` works there because the iteration is
    //    bounded by the shorter of two slices; here the offsets are not a
    //    traversal.
    //
    // 3. `get()` WOULD BE WORSE HERE, which is the part worth arguing. Its
    //    fallback must be some limb value, and a wrong limb in a division
    //    kernel is a wrong quotient -- a number that satisfies no invariant
    //    and looks entirely ordinary. An out-of-range index panics at the
    //    point of the error instead. Trading a loud impossible failure for a
    //    quiet possible wrong answer is the wrong direction for code that
    //    `seq`, `printf`, `expr` and `bc` all compute through.
    //
    // WHAT CHECKS IT. All three paths through this function are exercised,
    // each demonstrated by a probe rather than assumed: the ordinary one, D3's
    // estimate correction (`long_division_corrects_an_estimate_of_a_whole_limb`)
    // and D6's add-back (`long_division_exercises_the_add_back`). On top of
    // those, `long_division_identity_holds_beyond_u128` checks `q*d + r == n`
    // and `r < d` from 5 to 21 limbs, which is what an off-by-one in any of
    // these indices breaks.
    //
    // This allow was CONSIDERED AND REFUSED earlier on 2026-09-15, when D3
    // and D6 were reached by no test at all. The justification is the
    // coverage; it could not be written before the coverage existed. See
    // `TD-B-BIGNAT-ADD-BACK-IS-UNREACHED-BY-ANY-TEST`.
    #[allow(clippy::indexing_slicing)]
    pub fn divmod(&self, d: &Self) -> (Self, Self) {
        assert!(!d.is_zero(), "divide by zero");
        if self.cmp(d) == Ordering::Less {
            return (Nat::zero(), self.clone());
        }
        let n = d.limbs.len();
        if n == 1 {
            // `d` is non-zero by the assertion above and has exactly one
            // limb here, so that limb cannot be zero. The `else` is
            // unreachable and returns the honest answer for a zero divisor
            // rather than inventing one.
            let Some(small) = d.limbs.first().copied().and_then(NonZeroU32::new) else {
                return (Nat::zero(), self.clone());
            };
            let (q, r) = self.divmod_small(small);
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
            // `un` was resized to `m + n + 1`, so the first `n` are there.
            limbs: un.get(..n).unwrap_or(&un).to_vec(),
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
            let (q, r) = rest.divmod_small(BILLION);
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
                out.extend_from_slice(buf.get(lead..).unwrap_or_default());
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
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
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
            // `divmod_small` takes the non-zero type now, so the divisors
            // this loop can express are exactly the ones it accepts.
            let nz = NonZeroU32::new(d).expect("the list has no zero");
            let (q, r) = n.divmod_small(nz);
            assert_eq!(text(&q), (u128::MAX / u128::from(d)).to_string(), "d={d}");
            assert_eq!(u128::from(r), u128::MAX % u128::from(d), "d={d}");
        }
    }

    /// Step D6 -- the add-back. The estimate survives D3 still one too
    /// large, the multiply-subtract goes negative, and the divisor has to be
    /// added back in while the quotient limb is given up.
    ///
    /// THIS BRANCH USED TO BE REACHED BY NOTHING. A `panic!()` inside it left
    /// all 23 tests passing, and deleting its `q[j] -= 1` did too.
    ///
    /// WHY IT IS HARD TO HIT, and why the obvious attempts fail. D3's
    /// correction loop consults `v[n-2]` and `u[j+n-2]`, so for a TWO-limb
    /// divisor it sees the whole of `v` and the estimate it leaves is exact:
    /// D6 is unreachable for `n == 2` no matter what the operands are. That
    /// is why Hacker's Delight's 2^95 / (2^63+1) vector -- which that book
    /// labels an add-back case for its own `divmnu` -- only exercises D3
    /// here. A THREE-limb divisor is the smallest that leaves a limb D3
    /// cannot see.
    ///
    /// Found by enumerating the corners of that shape rather than by random
    /// search: Knuth puts D6's probability near `2/b`, one division in two
    /// billion, and 20,000 random hard-shaped divisions fired it zero times.
    ///
    /// The estimate here comes out as 4294967294 and the true digit is
    /// 4294967293, so D6 gives back exactly one.
    #[test]
    fn long_division_exercises_the_add_back() {
        // Checkable against `python -c "print(divmod(n, d))"`.
        let n = dec("170141183381241069217422966122340155392");
        let d = dec("39614081257132168801066942463");

        let (q, r) = n.divmod(&d);

        assert_eq!(text(&q), "4294967293", "quotient after the give-back");
        assert_eq!(
            text(&r),
            "39614081238685424740242292733",
            "remainder after the divisor was added back"
        );
        assert_eq!(r.cmp(&d), Ordering::Less, "remainder must be below divisor");
        assert_eq!(text(&q.mul(&d).add(&r)), text(&n), "q*d + r == n");
    }

    /// Algorithm D's step D3, where the first estimate of a quotient limb
    /// comes out as `b` itself and has to be walked back.
    ///
    /// `u = 2^95`, `v = 2^63 + 1`. At `j = 0` the estimate is
    /// `0x8000_0000_0000_0000 / 0x8000_0000`, which is exactly `2^32` -- one
    /// more than a limb can hold -- so the `qhat >> 32 != 0` arm of the
    /// correction loop runs and brings it to `0xFFFF_FFFF`, which is right.
    ///
    /// MEASURED, not assumed: disabling the correction loop fails this test
    /// and `long_division_identity_holds_beyond_u128`, and no other test in
    /// this module. Before these two, D3's correction was dead as far as the
    /// suite could tell.
    ///
    /// This is Hacker's Delight's `divmnu` vector, which that book gives as
    /// an ADD-BACK case. It is not one here: after D3 the estimate is exact,
    /// the multiply-subtract stays non-negative, and D6 never runs. See
    /// `TD-B-BIGNAT-ADD-BACK-IS-UNREACHED-BY-ANY-TEST`.
    #[test]
    fn long_division_corrects_an_estimate_of_a_whole_limb() {
        // 2^95 and 2^63 + 1, written out so the numbers are checkable
        // against `python -c "print(2**95)"` rather than trusted.
        let n = dec("39614081257132168796771975168");
        let d = dec("9223372036854775809");

        let (q, r) = n.divmod(&d);

        // 2^32 - 1: the estimate overshoots to 2^32, which does not fit a
        // limb, and the correction brings it back.
        assert_eq!(text(&q), "4294967295", "quotient");
        // 2^63 - 2^32 + 1
        assert_eq!(text(&r), "9223372032559808513", "remainder");

        assert_eq!(r.cmp(&d), Ordering::Less, "remainder must be below divisor");
        assert_eq!(text(&q.mul(&d).add(&r)), text(&n), "q*d + r == n");
    }

    /// Algorithm D on operands far larger than `u128`, checked by its own
    /// identity rather than against a reference.
    ///
    /// WHY THIS EXISTS. `divmod` is Knuth 4.2 Algorithm D, and its index
    /// arithmetic -- `un[i + j]`, `un[j + n]`, `vn[n - 1]`, `q[j]` -- only
    /// does anything interesting when the divisor has several limbs and the
    /// dividend has many more. Every other division test here is written
    /// against `u128`, so none of them reaches beyond FOUR limbs, and the
    /// inner loops they exercise are the degenerate ones.
    ///
    /// `q * d + r == n` together with `r < d` pins the quotient and remainder
    /// completely, and needs no second implementation -- which is precisely
    /// why the `u128` tests could not go any bigger. An off-by-one in the
    /// borrow loop breaks the identity even when the result still looks like
    /// a plausible number.
    ///
    /// MEASURED: disabling D3's estimate-correction loop fails this test and
    /// `long_division_corrects_an_estimate_of_a_whole_limb`, and nothing
    /// else here. It does NOT reach D6, the add-back -- nothing does; see
    /// `TD-B-BIGNAT-ADD-BACK-IS-UNREACHED-BY-ANY-TEST`.
    #[test]
    fn long_division_identity_holds_beyond_u128() {
        // A tiny LCG, so the cases are many but reproducible. The value of
        // the digits does not matter; the number of LIMBS does.
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut digits = |count: usize| -> String {
            let mut s = String::with_capacity(count);
            for i in 0..count {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let d = (seed >> 33) % 10;
                // No leading zero, so the numeral really has `count` digits.
                let d = if i == 0 && d == 0 { 7 } else { d };
                s.push((b'0' + u8::try_from(d).unwrap_or(0)) as char);
            }
            s
        };

        // Dividend and divisor digit counts. 40 decimal digits is ~5 limbs,
        // 200 is ~21: past `u128` in every case, and the last two make the
        // quotient short, which is where the add-back step runs.
        for &(nd, dd) in &[
            (40usize, 9usize),
            (80, 20),
            (160, 41),
            (200, 3),
            (77, 76),
            (128, 127),
            (300, 150),
        ] {
            let n = dec(&digits(nd));
            let d = dec(&digits(dd));
            assert!(!d.is_zero(), "divisor must not be zero");

            let (q, r) = n.divmod(&d);

            // r < d, or the quotient was one too small.
            assert_eq!(
                r.cmp(&d),
                Ordering::Less,
                "remainder {} is not below divisor {} for {nd}/{dd} digits",
                text(&r),
                text(&d)
            );

            // q * d + r == n, or anything at all went wrong.
            let back = q.mul(&d).add(&r);
            assert_eq!(
                text(&back),
                text(&n),
                "q*d + r did not reconstruct the dividend for {nd}/{dd} digits"
            );
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
