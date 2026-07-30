//! Exact decimal expansion of a binary floating-point value.
//!
//! `printf`'s float conversions have to answer one question: *what are the
//! decimal digits of this `f64`, correctly rounded to the place the format
//! asked for?*  The obvious implementation — peel the integer part off with a
//! cast and generate fraction digits by repeatedly multiplying the remainder
//! by ten — is what `printf.rs` used to do, and it is wrong twice over:
//!
//!   * `val as u64` saturates, so every value at or above 2^64 printed the
//!     same 20-digit garbage (`printf("%.2f", 1e20)` produced
//!     `18446744073709551615./0`); and
//!   * `remainder *= 10.0` rounds, so the digits drift after roughly the
//!     seventeenth significant one.  `%.30f` of `0.1` printed
//!     `0.100000000000000000000000000000` where the value really is
//!     `0.100000000000000005551115123126`.
//!
//! Both disappear if the expansion is computed *exactly* instead, which is
//! possible because every finite `f64` has a **finite** decimal expansion.
//! Write the value as `m * 2^e` with `m` odd (which `decompose` does).  Then
//!
//! ```text
//! e >= 0:   val = (m << e) * 10^0
//! e <  0:   val = (m * 5^-e) * 10^e
//! ```
//!
//! because `2^e = 5^-e * 10^e`.  Either way the value is an *integer* times a
//! power of ten, so converting that integer to decimal yields every digit the
//! value has, with nothing rounded anywhere.  The integer needs at most
//! `53 + 1074*log2(5) ~= 2548` bits, so a fixed-size limb array covers the
//! whole `f64` range with no allocation — which matters, since this runs
//! inside `printf` in a freestanding libc.
//!
//! Once the digits are exact, rounding is pure digit arithmetic: look at the
//! first dropped digit, and at whether anything nonzero follows it.  That also
//! makes ties *exactly* detectable ("digit is 5 and the rest is zero"), so the
//! ties-to-even rule glibc implements needs no separate machinery — the
//! `decompose`/`is_half_way` pair that used to answer that question from the
//! binary representation is subsumed here.
//!
//! Cost: for ordinary magnitudes the integer is a handful of limbs and the
//! conversion is a few divisions.  The full 2548-bit worst case only arises
//! for subnormals and other values near the bottom of the exponent range,
//! which is exactly where a fast approximate algorithm would be least
//! trustworthy anyway.

/// Number of 64-bit limbs needed for the largest exact expansion.
///
/// The worst case is the smallest subnormal: `m` has 53 bits and `5^1074` has
/// `ceil(1074 * log2 5) = 2495`, for 2548 bits total.  (The large-exponent
/// case, `m << 971`, needs only 1024.)
const LIMBS: usize = 40;

/// Largest number of significant decimal digits a finite `f64` can have.
///
/// 2548 bits is at most `ceil(2548 * log10 2) = 767` digits.  One extra slot
/// absorbs the carry when rounding turns `999…9` into `1000…0`.
pub(crate) const MAX_DIGITS: usize = 768;

/// `5^27`, the largest power of five that fits in a `u64`.
const POW5_CHUNK: u64 = 7_450_580_596_923_828_125;
/// The exponent of [`POW5_CHUNK`].
const POW5_CHUNK_EXP: u32 = 27;
/// `10^19`, the largest power of ten that fits in a `u64`.
const POW10_CHUNK: u64 = 10_000_000_000_000_000_000;
/// The exponent of [`POW10_CHUNK`].
const POW10_CHUNK_EXP: usize = 19;

/// A fixed-capacity unsigned big integer, little-endian limbs.
struct Big {
    limbs: [u64; LIMBS],
    /// Number of significant limbs; `0` means the value is zero.
    len: usize,
}

impl Big {
    fn from_u64(v: u64) -> Self {
        let mut limbs = [0u64; LIMBS];
        let len = if v == 0 {
            0
        } else {
            limbs[0] = v;
            1
        };
        Self { limbs, len }
    }

    fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// `self *= x`.  Saturates by dropping the overflow, which cannot happen
    /// for the inputs this module produces: [`LIMBS`] is sized for the worst
    /// case and every caller stays inside it.
    #[allow(clippy::arithmetic_side_effects)]
    fn mul_small(&mut self, x: u64) {
        if x == 0 || self.is_zero() {
            self.len = 0;
            return;
        }
        let mut carry: u128 = 0;
        for i in 0..self.len {
            // SAFETY-of-indexing: `i < self.len <= LIMBS`.
            let Some(slot) = self.limbs.get_mut(i) else {
                break;
            };
            let prod = u128::from(*slot) * u128::from(x) + carry;
            *slot = prod as u64;
            carry = prod >> 64;
        }
        while carry != 0 && self.len < LIMBS {
            if let Some(slot) = self.limbs.get_mut(self.len) {
                *slot = carry as u64;
            }
            carry >>= 64;
            self.len += 1;
        }
        debug_assert!(carry == 0, "big-integer overflow in mul_small");
    }

    /// `self <<= bits`.
    #[allow(clippy::arithmetic_side_effects)]
    fn shl(&mut self, bits: u32) {
        if self.is_zero() || bits == 0 {
            return;
        }
        let whole = (bits / 64) as usize;
        let part = bits % 64;

        // Move limbs up by `whole`, then shift within limbs by `part`.
        let old_len = self.len;
        let new_len = (old_len + whole + usize::from(part != 0)).min(LIMBS);
        let mut i = new_len;
        while i > 0 {
            i -= 1;
            let hi = i.checked_sub(whole).and_then(|j| self.limbs.get(j)).copied();
            let lo = i
                .checked_sub(whole)
                .and_then(|j| j.checked_sub(1))
                .and_then(|j| self.limbs.get(j))
                .copied();
            let v = match (hi, lo, part) {
                (Some(h), _, 0) => h,
                (Some(h), Some(l), p) => (h << p) | (l >> (64 - p)),
                (Some(h), None, p) => h << p,
                (None, _, _) => 0,
            };
            if let Some(slot) = self.limbs.get_mut(i) {
                *slot = v;
            }
        }
        self.len = new_len;
        self.normalize();
    }

    /// `self /= d`, returning the remainder.
    #[allow(clippy::arithmetic_side_effects)]
    fn divmod_small(&mut self, d: u64) -> u64 {
        debug_assert!(d != 0);
        let mut rem: u128 = 0;
        let mut i = self.len;
        while i > 0 {
            i -= 1;
            let Some(slot) = self.limbs.get_mut(i) else {
                continue;
            };
            let cur = (rem << 64) | u128::from(*slot);
            *slot = (cur / u128::from(d)) as u64;
            rem = cur % u128::from(d);
        }
        self.normalize();
        rem as u64
    }

    #[allow(clippy::arithmetic_side_effects)]
    fn normalize(&mut self) {
        while self.len > 0 && self.limbs.get(self.len - 1).copied() == Some(0) {
            self.len -= 1;
        }
    }
}

/// Decompose a finite, positive `f64` into `(m, e)` with `val == m * 2^e` and
/// `m` odd.  Returns `(0, 0)` for zero.
///
/// Reducing `m` to odd is not required for correctness, but it shrinks the
/// `5^-e` factor by up to 52 places for values with trailing zero bits — which
/// is most of them — and so keeps the common case cheap.
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn decompose(val: f64) -> (u64, i32) {
    let bits = val.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let raw_frac = bits & 0x000f_ffff_ffff_ffff;
    let (mut m, mut e) = if raw_exp == 0 {
        // Subnormal (or zero): no implicit leading bit, fixed exponent.
        (raw_frac, -1074)
    } else {
        (raw_frac | (1u64 << 52), raw_exp - 1075)
    };
    if m == 0 {
        return (0, 0);
    }
    let tz = m.trailing_zeros();
    m >>= tz;
    e += tz as i32;
    (m, e)
}

/// The exact decimal expansion of a finite, non-negative `f64`.
///
/// The value is `0.d[0]d[1]…d[len-1] * 10^decpt` — that is, `decpt` is the
/// number of digits that lie before the decimal point, and may be zero or
/// negative (the value is below 1) or greater than `len` (the value has
/// trailing zeros before the point).  Trailing zero digits are always
/// stripped, so `d[len-1]` is never `b'0'` and zero is `len == 0`.
pub(crate) struct Decimal {
    digits: [u8; MAX_DIGITS],
    len: usize,
    decpt: i32,
}

impl Decimal {
    /// Compute the exact expansion of `val`, which must be finite and `>= 0`.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub(crate) fn new(val: f64) -> Self {
        let mut out = Self {
            digits: [0u8; MAX_DIGITS],
            len: 0,
            decpt: 0,
        };
        let (m, e) = decompose(val);
        if m == 0 {
            return out;
        }

        // val == big * 10^scale, exactly.
        let mut big = Big::from_u64(m);
        let scale: i32 = if e >= 0 {
            big.shl(e as u32);
            0
        } else {
            // 2^e == 5^-e * 10^e, so multiplying by 5^-e clears the binary
            // exponent into a decimal one.  Applied `POW5_CHUNK_EXP` at a
            // time because that is the most that fits in a limb multiplier.
            let mut left = (-e) as u32;
            while left >= POW5_CHUNK_EXP {
                big.mul_small(POW5_CHUNK);
                left -= POW5_CHUNK_EXP;
            }
            if left > 0 {
                let mut f: u64 = 1;
                for _ in 0..left {
                    f *= 5;
                }
                big.mul_small(f);
            }
            e
        };

        // Convert `big` to decimal, least-significant chunk first, into a
        // scratch buffer written back-to-front so the digits end up in order.
        let mut scratch = [b'0'; MAX_DIGITS];
        let mut pos = MAX_DIGITS;
        while !big.is_zero() {
            let mut chunk = big.divmod_small(POW10_CHUNK);
            let last = big.is_zero();
            let mut emitted = 0usize;
            while pos > 0 && (chunk != 0 || (!last && emitted < POW10_CHUNK_EXP)) {
                pos -= 1;
                if let Some(slot) = scratch.get_mut(pos) {
                    *slot = b'0' + (chunk % 10) as u8;
                }
                chunk /= 10;
                emitted += 1;
            }
            debug_assert!(chunk == 0, "decimal buffer too small");
        }

        let total = MAX_DIGITS - pos;
        // `scale` counts the digits that sit to the right of the point.
        out.decpt = total as i32 + scale;
        // Strip trailing zeros; `decpt` already accounts for their place.
        let mut end = MAX_DIGITS;
        while end > pos && scratch.get(end - 1).copied() == Some(b'0') {
            end -= 1;
        }
        out.len = end - pos;
        if let (Some(dst), Some(src)) = (
            out.digits.get_mut(..out.len),
            scratch.get(pos..end),
        ) {
            dst.copy_from_slice(src);
        }
        if out.len == 0 {
            out.decpt = 0;
        }
        out
    }

    /// Is the value zero?
    pub(crate) fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// Number of digits before the decimal point (see the type docs).
    pub(crate) fn decpt(&self) -> i32 {
        self.decpt
    }

    /// The digit at significant position `i`, or `b'0'` past the end.
    ///
    /// Indices outside `0..len` are genuinely zero rather than out of range:
    /// the expansion is exact, so every digit the value does not have *is* a
    /// zero.
    pub(crate) fn digit(&self, i: i32) -> u8 {
        match usize::try_from(i) {
            Ok(u) if u < self.len => self.digits.get(u).copied().unwrap_or(b'0'),
            _ => b'0',
        }
    }

    /// Round to at most `n` significant digits, ties to even.
    ///
    /// `n == 0` asks whether the value reaches half of `10^decpt`; a negative
    /// `n` is below even that and rounds to zero.
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn round_to_significant(&mut self, n: i32) {
        if self.len == 0 {
            return;
        }
        let Ok(keep) = usize::try_from(n) else {
            // Every digit is past the rounding place, and the value is
            // strictly below half of it, so the result is zero.
            self.len = 0;
            self.decpt = 0;
            return;
        };
        if keep >= self.len {
            return;
        }

        // Decide the direction from the first dropped digit and whether any
        // nonzero digit follows it.  Because the expansion is exact, "5 with
        // nothing after" is precisely a tie — no separate analysis needed.
        let first_dropped = self.digits.get(keep).copied().unwrap_or(b'0');
        let rest_nonzero = self
            .digits
            .get(keep.wrapping_add(1)..self.len)
            .is_some_and(|tail| tail.iter().any(|&d| d != b'0'));
        let round_up = if first_dropped > b'5' {
            true
        } else if first_dropped < b'5' {
            false
        } else if rest_nonzero {
            true
        } else {
            // Exact tie: round towards an even last kept digit.  A dropped
            // leading digit leaves an implicit 0, which is even.
            let prev = keep
                .checked_sub(1)
                .and_then(|i| self.digits.get(i))
                .map_or(0, |&d| d.wrapping_sub(b'0'));
            prev % 2 == 1
        };

        self.len = keep;
        if round_up {
            let mut i = keep;
            loop {
                if i == 0 {
                    // Carried out of the most significant digit: the result is
                    // a single 1 one place higher.
                    if let Some(slot) = self.digits.get_mut(0) {
                        *slot = b'1';
                    }
                    self.len = 1;
                    self.decpt += 1;
                    return;
                }
                i -= 1;
                match self.digits.get_mut(i) {
                    Some(slot) if *slot == b'9' => *slot = b'0',
                    Some(slot) => {
                        *slot += 1;
                        break;
                    }
                    None => break,
                }
            }
        }
        while self.len > 0 && self.digits.get(self.len - 1).copied() == Some(b'0') {
            self.len -= 1;
        }
        if self.len == 0 {
            self.decpt = 0;
        }
    }

    /// Round so that no digit lies below the `10^-p` place.
    ///
    /// The digit at `10^-p` is significant index `decpt + p - 1`, so keeping
    /// everything at or above it means keeping `decpt + p` digits.
    pub(crate) fn round_to_place(&mut self, p: i32) {
        if self.len == 0 {
            return;
        }
        self.round_to_significant(self.decpt.saturating_add(p));
    }

    /// Number of significant digits remaining.
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::{Decimal, decompose};

    /// Render the whole exact expansion the way `%f` would, for comparison
    /// against Rust's own (exact) formatter.
    fn exact_fixed(v: f64, p: usize) -> String {
        let mut d = Decimal::new(v);
        d.round_to_place(i32::try_from(p).unwrap());
        let mut s = String::new();
        if d.decpt() <= 0 {
            s.push('0');
        } else {
            for i in 0..d.decpt() {
                s.push(char::from(d.digit(i)));
            }
        }
        if p > 0 {
            s.push('.');
            for j in 1..=i32::try_from(p).unwrap() {
                s.push(char::from(d.digit(d.decpt() + j - 1)));
            }
        }
        s
    }

    #[test]
    fn decompose_reduces_to_an_odd_significand() {
        for &v in &[1.0f64, 0.5, 8.25, 1e20, 1e-20, f64::MIN_POSITIVE] {
            let (m, e) = decompose(v);
            assert_ne!(m, 0);
            assert_eq!(m % 2, 1, "significand of {v} is not odd");
            // Reconstructing must give the value back exactly.
            let mut back = m as f64;
            let mut k = e;
            while k > 0 {
                back *= 2.0;
                k -= 1;
            }
            while k < 0 {
                back *= 0.5;
                k += 1;
            }
            assert_eq!(back, v);
        }
        assert_eq!(decompose(0.0), (0, 0));
    }

    #[test]
    fn expansion_matches_rusts_exact_formatter() {
        // Rust's `{:.*}` is exact and correctly rounded, so it is a ground
        // truth for both the huge-magnitude case (which the old cast-based
        // code could not represent at all) and the long-tail case (which it
        // could not compute).
        let cases: &[(f64, usize)] = &[
            (0.0, 0),
            (0.0, 5),
            (1.0, 0),
            (1.0, 3),
            (0.1, 30),
            (0.1, 60),
            (1.0 / 3.0, 40),
            (1e20, 2),
            (1e25, 2),
            (f64::MAX, 2),
            (1e-20, 40),
            (123_456_789.123_456_79, 20),
            (2.5, 0),
            (3.5, 0),
            (8.25, 1),
            (1234.5, 0),
            (9.5, 0),
            (0.125, 2),
            (0.375, 2),
            (f64::MIN_POSITIVE, 330),
            (5e-324, 340),
        ];
        for &(v, p) in cases {
            assert_eq!(exact_fixed(v, p), format!("{v:.p$}"), "%.{p}f of {v:e}");
        }
    }

    #[test]
    fn expansion_matches_rust_over_a_sweep() {
        // A deterministic sweep across exponents and significands, at a
        // precision long enough to expose any digit drift.
        let mut bits: u64 = 0x3ff0_0000_0000_0001;
        for _ in 0..2000 {
            let v = f64::from_bits(bits);
            if v.is_finite() {
                for &p in &[0usize, 1, 6, 17, 25] {
                    assert_eq!(exact_fixed(v, p), format!("{v:.p$}"), "%.{p}f of {v:e}");
                }
            }
            // A large odd stride walks exponent and mantissa together without
            // repeating.
            bits = bits.wrapping_add(0x0004_7f3a_91c5_2b17);
            bits &= 0x7fef_ffff_ffff_ffff;
        }
    }

    #[test]
    fn rounding_ties_go_to_even() {
        // Exact halves, so the direction is fully determined.
        assert_eq!(exact_fixed(8.25, 1), "8.2");
        assert_eq!(exact_fixed(8.75, 1), "8.8");
        assert_eq!(exact_fixed(2.5, 0), "2");
        assert_eq!(exact_fixed(3.5, 0), "4");
        assert_eq!(exact_fixed(0.5, 0), "0");
        // A tie whose even neighbour needs a carry all the way out.
        assert_eq!(exact_fixed(9.5, 0), "10");
        // Not a tie: 1.005 is really 1.00499…, so it must round down.
        assert_eq!(exact_fixed(1.005, 2), "1.00");
    }

    #[test]
    fn rounding_below_the_first_digit_yields_zero_or_one() {
        let mut d = Decimal::new(0.4);
        d.round_to_place(0);
        assert!(d.is_zero());

        let mut d = Decimal::new(0.6);
        d.round_to_place(0);
        assert_eq!(d.len(), 1);
        assert_eq!(d.digit(0), b'1');
        assert_eq!(d.decpt(), 1);

        // Far below the rounding place.
        let mut d = Decimal::new(1e-30);
        d.round_to_place(3);
        assert!(d.is_zero());
    }

    #[test]
    fn significant_rounding_tracks_the_decimal_point() {
        let mut d = Decimal::new(999.9);
        d.round_to_significant(3);
        assert_eq!(d.len(), 1);
        assert_eq!(d.digit(0), b'1');
        assert_eq!(d.decpt(), 4); // 1000
    }

    #[test]
    fn trailing_zeros_are_stripped_but_the_point_is_kept() {
        let d = Decimal::new(100.0);
        assert_eq!(d.len(), 1);
        assert_eq!(d.digit(0), b'1');
        assert_eq!(d.decpt(), 3);
    }
}
