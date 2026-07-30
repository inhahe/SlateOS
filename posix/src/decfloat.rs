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
const DEC_LIMBS: usize = 40;

/// Number of limbs for the parsing direction, which scales the significand up
/// by `2^L` before dividing so that the quotient is long enough to round.
/// See [`decimal_to_f64`]; the worst case there is about 5165 bits.
const PARSE_LIMBS: usize = 96;

/// Largest number of significant decimal digits that can affect *which* `f64`
/// an input rounds to.  Past this, further digits can only decide whether the
/// value sits exactly on a rounding boundary, which a sticky bit records.
pub(crate) const MAX_PARSE_DIGITS: usize = 768;

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
struct Big<const N: usize> {
    limbs: [u64; N],
    /// Number of significant limbs; `0` means the value is zero.
    len: usize,
}

impl<const N: usize> Big<N> {
    fn from_u64(v: u64) -> Self {
        let mut limbs = [0u64; N];
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
    /// for the inputs this module produces: `N` is sized for the worst case
    /// and every caller stays inside it.
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
        while carry != 0 && self.len < N {
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
        let new_len = (old_len + whole + usize::from(part != 0)).min(N);
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

    /// `self += x`.
    #[allow(clippy::arithmetic_side_effects)]
    fn add_small(&mut self, x: u64) {
        if x == 0 {
            return;
        }
        let mut carry = x;
        let mut i = 0usize;
        while carry != 0 {
            let Some(slot) = self.limbs.get_mut(i) else {
                debug_assert!(false, "big-integer overflow in add_small");
                return;
            };
            let (sum, over) = slot.overflowing_add(carry);
            *slot = sum;
            carry = u64::from(over);
            i += 1;
        }
        if i > self.len {
            self.len = i;
        }
    }

    /// Position of the most significant set bit, plus one; `0` when zero.
    #[allow(clippy::arithmetic_side_effects)]
    fn bits(&self) -> usize {
        match self.len.checked_sub(1).and_then(|i| self.limbs.get(i)) {
            Some(&top) if top != 0 => (self.len - 1) * 64 + (64 - top.leading_zeros() as usize),
            _ => 0,
        }
    }

    /// Is bit `i` set?
    #[allow(clippy::arithmetic_side_effects)]
    fn bit(&self, i: usize) -> bool {
        self.limbs
            .get(i / 64)
            .is_some_and(|&w| (w >> (i % 64)) & 1 == 1)
    }

    /// Is any bit strictly below index `i` set?  This is the sticky test.
    #[allow(clippy::arithmetic_side_effects)]
    fn any_bits_below(&self, i: usize) -> bool {
        let top = i / 64;
        let off = (i % 64) as u32;
        for k in 0..top.min(self.len) {
            if self.limbs.get(k).copied().unwrap_or(0) != 0 {
                return true;
            }
        }
        off != 0
            && self
                .limbs
                .get(top)
                .is_some_and(|&w| w & ((1u64 << off) - 1) != 0)
    }

    /// The 64-bit window of `self` starting at bit `i`, i.e. `(self >> i)` as
    /// a `u64`.  Callers guarantee the shifted value fits.
    #[allow(clippy::arithmetic_side_effects)]
    fn window(&self, i: usize) -> u64 {
        let idx = i / 64;
        let off = (i % 64) as u32;
        let lo = self.limbs.get(idx).copied().unwrap_or(0);
        if off == 0 {
            lo
        } else {
            let hi = self.limbs.get(idx + 1).copied().unwrap_or(0);
            (lo >> off) | (hi << (64 - off))
        }
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
        let mut big = Big::<DEC_LIMBS>::from_u64(m);
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

/// Accumulates the decimal digits of a floating-point literal exactly.
///
/// `strtod` and `scanf`'s `%f`/`%e`/`%g` read their input from different
/// places — a C string and a scan context — but the digit bookkeeping is
/// identical, and getting it subtly wrong is exactly how a parser loses
/// precision.  Driving one collector from both keeps them in step.
///
/// The value accumulated is exactly `digits * 10^exp10`.  Digits past
/// [`MAX_PARSE_DIGITS`] are not stored — they cannot change which `f64` the
/// input rounds to, only whether it lands on a tie — but they are still
/// accounted for, either in the exponent or in the sticky bit.
pub(crate) struct DigitCollector {
    digits: [u8; MAX_PARSE_DIGITS],
    len: usize,
    exp10: i32,
    truncated: bool,
}

impl DigitCollector {
    pub(crate) const fn new() -> Self {
        Self {
            digits: [b'0'; MAX_PARSE_DIGITS],
            len: 0,
            exp10: 0,
            truncated: false,
        }
    }

    /// Add a digit that appeared before the decimal point.
    ///
    /// One that does not fit still scales everything already stored, hence the
    /// exponent bump; only its own contribution is lost, to the sticky bit.
    pub(crate) fn push_integer(&mut self, ascii: u8) {
        if self.len == 0 && ascii == b'0' {
            // A leading zero contributes nothing at all.
        } else if self.len < MAX_PARSE_DIGITS {
            if let Some(slot) = self.digits.get_mut(self.len) {
                *slot = ascii;
            }
            self.len = self.len.saturating_add(1);
        } else {
            self.exp10 = self.exp10.saturating_add(1);
            if ascii != b'0' {
                self.truncated = true;
            }
        }
    }

    /// Add a digit that appeared after the decimal point.
    ///
    /// Each stored digit moves the point one place right, and so does each
    /// leading zero that precedes the first significant digit.  A digit that
    /// does not fit sits entirely below the last stored one, so it is pure
    /// sticky and does not touch the exponent.
    pub(crate) fn push_fraction(&mut self, ascii: u8) {
        if self.len == 0 && ascii == b'0' {
            self.exp10 = self.exp10.saturating_sub(1);
        } else if self.len < MAX_PARSE_DIGITS {
            if let Some(slot) = self.digits.get_mut(self.len) {
                *slot = ascii;
            }
            self.len = self.len.saturating_add(1);
            self.exp10 = self.exp10.saturating_sub(1);
        } else if ascii != b'0' {
            self.truncated = true;
        }
    }

    /// Apply an explicit `e[+-]NN` exponent.
    pub(crate) fn apply_exponent(&mut self, exp: i32) {
        self.exp10 = self.exp10.saturating_add(exp);
    }

    /// Round the accumulated value to the nearest `f64`, ties to even.
    ///
    /// Returns `(value, out_of_range)` as [`decimal_to_f64`] does.
    pub(crate) fn to_f64(&self) -> (f64, bool) {
        decimal_to_f64(
            self.digits.get(..self.len).unwrap_or(&[]),
            self.exp10,
            self.truncated,
        )
    }

    /// Round the accumulated value to the nearest `f32`, ties to even.
    ///
    /// Rounds straight from the decimal rather than by way of `f64`; see
    /// [`decimal_to_f32`] for why that distinction matters.
    pub(crate) fn to_f32(&self) -> (f32, bool) {
        decimal_to_f32(
            self.digits.get(..self.len).unwrap_or(&[]),
            self.exp10,
            self.truncated,
        )
    }
}

/// Convert `digits * 10^exp10` to the nearest value of `fmt`, ties to even.
///
/// `digits` holds the ASCII decimal digits of the significant part; `exp10` is
/// the power of ten it is scaled by.  `truncated` says the caller had more
/// nonzero digits than it could store, so the true value is strictly greater
/// than `digits * 10^exp10`.  That is exactly a sticky bit: past
/// [`MAX_PARSE_DIGITS`] digits no further digit can move the result to a
/// different `f64` — it can only decide a tie, and knowing *that* a nonzero
/// tail exists is enough to decide one.
///
/// The conversion is exact-then-round, never a chain of floating-point
/// multiplies:
///
/// ```text
///   exp10 >= 0:  value = (D * 10^exp10) * 2^0
///   exp10 <  0:  value = D / 5^Q / 2^Q                     with Q = -exp10
///                      = floor(D * 2^L / 5^Q) * 2^-(L+Q)   plus a remainder
/// ```
///
/// `L` is chosen so the quotient keeps at least 64 bits — 53 for the
/// significand, the rest for guard and round — and a nonzero division
/// remainder feeds the sticky bit, so the final rounding step sees the true
/// value and not an approximation of it.
///
/// Returns `(bits, out_of_range)` — the raw encoding of a *positive* value;
/// the caller applies the sign.  `out_of_range` is the C `ERANGE` condition:
/// overflow to infinity, underflow to zero, or a subnormal result (gradual
/// underflow).  glibc reports `ERANGE` for all three.
#[allow(clippy::arithmetic_side_effects)]
fn decimal_to_binary(digits: &[u8], exp10: i32, truncated: bool, fmt: &Format) -> (u64, bool) {
    let mut sticky = truncated;
    let mut exp10 = exp10;

    // Leading zeros contribute nothing; trailing zeros move into the exponent,
    // which keeps the big integer as short as the value allows.
    let mut start = 0usize;
    while digits.get(start) == Some(&b'0') {
        start = start.saturating_add(1);
    }
    let mut end = digits.len();
    while end > start && digits.get(end.saturating_sub(1)) == Some(&b'0') {
        end = end.saturating_sub(1);
        exp10 = exp10.saturating_add(1);
    }
    let digits = digits.get(start..end).unwrap_or(&[]);
    if digits.is_empty() {
        return (0, false);
    }

    // Position of the decimal point: the value lies in `[10^(mag-1), 10^mag)`.
    // `DBL_MAX` is just under `10^309` and the smallest subnormal is just over
    // `10^-324`, so these cut-offs are decided by magnitude alone and keep the
    // big-integer work bounded.
    let mag = exp10.saturating_add(i32::try_from(digits.len()).unwrap_or(i32::MAX));
    // The bounds are the `f64` ones even when rounding to `f32`; they exist
    // only to keep the big-integer work finite, and the rounding step below
    // handles anything inside them that still overflows the narrower format.
    if mag > 310 {
        return (fmt.infinity(), true);
    }
    if mag < -330 {
        return (0, true);
    }

    // The exact integer formed by the digits, absorbed 19 at a time because
    // `10^19` is the largest power of ten that fits in a `u64`.
    let mut b = Big::<PARSE_LIMBS>::from_u64(0);
    let mut i = 0usize;
    while i < digits.len() {
        let take = POW10_CHUNK_EXP.min(digits.len() - i);
        let mut chunk = 0u64;
        let mut scale = 1u64;
        for k in 0..take {
            let d = digits.get(i + k).copied().unwrap_or(b'0');
            chunk = chunk * 10 + u64::from(d.wrapping_sub(b'0'));
            scale *= 10;
        }
        b.mul_small(scale);
        b.add_small(chunk);
        i += take;
    }

    let e: i32;
    if exp10 >= 0 {
        let mut left = exp10;
        while left > 0 {
            let step = left.min(i32::try_from(POW10_CHUNK_EXP).unwrap_or(19));
            let mut p = 1u64;
            for _ in 0..step {
                p *= 10;
            }
            b.mul_small(p);
            left -= step;
        }
        e = 0;
    } else {
        let q = exp10.unsigned_abs();
        // `L = ceil(Q * log2 5) + 64`.  `2321929/10^6` is above `log2 5`, so
        // the ceiling is never short and the quotient always keeps >= 64 bits.
        let scaled = (u64::from(q) * 2_321_929 + 999_999) / 1_000_000;
        let l = u32::try_from(scaled).unwrap_or(u32::MAX).saturating_add(64);
        b.shl(l);
        let mut left = q;
        while left > 0 {
            let step = left.min(POW5_CHUNK_EXP);
            let d = if step == POW5_CHUNK_EXP {
                POW5_CHUNK
            } else {
                let mut p = 1u64;
                for _ in 0..step {
                    p *= 5;
                }
                p
            };
            // A nonzero remainder is value we are about to discard, and it
            // sits below every bit of the quotient: pure sticky.
            if b.divmod_small(d) != 0 {
                sticky = true;
            }
            left -= step;
        }
        e = -i32::try_from(l).unwrap_or(i32::MAX) - i32::try_from(q).unwrap_or(i32::MAX);
    }

    round_to_binary(&b, e, sticky, fmt)
}

/// Convert `digits * 10^exp10` to the nearest `f64`, ties to even.
///
/// See [`decimal_to_binary`]; `truncated` is the caller's sticky bit and the
/// second result is the C `ERANGE` condition.
pub(crate) fn decimal_to_f64(digits: &[u8], exp10: i32, truncated: bool) -> (f64, bool) {
    let (bits, out_of_range) = decimal_to_binary(digits, exp10, truncated, &F64_FORMAT);
    (f64::from_bits(bits), out_of_range)
}

/// Convert `digits * 10^exp10` to the nearest `f32`, ties to even.
///
/// Rounding to `f64` first and narrowing afterwards would round twice, and
/// two roundings are not one: a value a hair above an `f32` midpoint can land
/// exactly *on* that midpoint in `f64`, after which ties-to-even sends it the
/// wrong way.  `strtof("1.000000059604644830901776231257827021181583404541015625")`
/// is such a value — it must give `1.00000012`, but via `f64` it gives `1.0`.
/// So `f32` is rounded straight from the exact decimal expansion.
pub(crate) fn decimal_to_f32(digits: &[u8], exp10: i32, truncated: bool) -> (f32, bool) {
    let (bits, out_of_range) = decimal_to_binary(digits, exp10, truncated, &F32_FORMAT);
    (f32::from_bits(u32::try_from(bits).unwrap_or(0)), out_of_range)
}

/// The shape of a binary floating-point format, as much of it as rounding into
/// the format needs to know.
struct Format {
    /// Bits in the significand, counting the implicit leading one.
    mant_bits: u32,
    /// Binary exponent of the smallest subnormal, the floor below which
    /// results are gradually flushed towards zero.
    min_exp: i32,
    /// Added to the exponent of `m * 2^exp` (with `m` normalised to
    /// `mant_bits` bits) to get the stored exponent field.
    bias: i32,
    /// The reserved all-ones exponent field, which encodes infinity.
    inf_field: u64,
}

impl Format {
    /// The bit pattern of positive infinity.
    fn infinity(&self) -> u64 {
        self.inf_field << self.mant_bits.saturating_sub(1)
    }
}

/// IEEE-754 binary64: 53-bit significand, least subnormal `2^-1074`.
const F64_FORMAT: Format = Format {
    mant_bits: 53,
    min_exp: -1074,
    bias: 1075,
    inf_field: 0x7ff,
};

/// IEEE-754 binary32: 24-bit significand, least subnormal `2^-149`.
const F32_FORMAT: Format = Format {
    mant_bits: 24,
    min_exp: -149,
    bias: 150,
    inf_field: 0xff,
};

/// Round the exact value `b * 2^e` into `fmt`, ties to even.
///
/// `sticky_in` says the true value is strictly greater than `b * 2^e`, by less
/// than one unit in `b`'s last place.  Returns `(bits, out_of_range)` with the
/// same `ERANGE` meaning as [`decimal_to_binary`].
#[allow(clippy::arithmetic_side_effects)]
fn round_to_binary<const N: usize>(b: &Big<N>, e: i32, sticky_in: bool, fmt: &Format) -> (u64, bool) {
    let n = b.bits();
    if n == 0 {
        return (0, false);
    }
    let prec = fmt.mant_bits as usize;
    let implicit = 1u64 << (fmt.mant_bits - 1);

    // Keep the top `mant_bits` bits, then, if that lands below the subnormal
    // floor, drop further bits so the exponent is exactly `min_exp`.
    // Everything below the cut is summarised by one guard bit and one sticky
    // bit, which is all round-to-nearest-even needs.
    let mut drop = n.saturating_sub(prec);
    let mut exp = e.saturating_add(i32::try_from(drop).unwrap_or(i32::MAX));
    if exp < fmt.min_exp {
        let extra = i64::from(fmt.min_exp).saturating_sub(i64::from(exp));
        drop = drop.saturating_add(usize::try_from(extra).unwrap_or(usize::MAX));
        exp = fmt.min_exp;
    }

    let mut m = b.window(drop);
    let guard = drop > 0 && b.bit(drop.saturating_sub(1));
    let sticky = sticky_in || (drop > 1 && b.any_bits_below(drop.saturating_sub(1)));
    if guard && (sticky || m & 1 == 1) {
        m += 1;
        if m == implicit << 1 {
            m >>= 1;
            exp = exp.saturating_add(1);
        }
    }

    if m == 0 {
        return (0, true);
    }
    // A short significand (fewer bits than the format holds, so nothing was
    // dropped and nothing was rounded) is normalised by scaling up until it
    // reaches the implicit-bit position or the subnormal floor stops us.
    while m < implicit && exp > fmt.min_exp {
        m <<= 1;
        exp -= 1;
    }
    if m < implicit {
        // Subnormal: the exponent is pinned at the floor, so `m` *is* the
        // stored bit pattern.
        return (m, true);
    }

    let biased = exp.saturating_add(fmt.bias);
    if biased >= i32::try_from(fmt.inf_field).unwrap_or(i32::MAX) {
        return (fmt.infinity(), true);
    }
    let biased = u64::try_from(biased).unwrap_or(0);
    ((biased << (fmt.mant_bits - 1)) | (m - implicit), false)
}

#[cfg(test)]
mod tests {
    use super::{Decimal, decimal_to_f32, decimal_to_f64, decompose};

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

    // -- decimal -> binary --

    fn conv(digits: &str, exp10: i32) -> (f64, bool) {
        decimal_to_f64(digits.as_bytes(), exp10, false)
    }

    #[test]
    fn conversion_is_exact_where_it_can_be() {
        assert_eq!(conv("1", 0), (1.0, false));
        assert_eq!(conv("0", 0), (0.0, false));
        assert_eq!(conv("", 0), (0.0, false));
        assert_eq!(conv("000", 5), (0.0, false));
        assert_eq!(conv("5", -1), (0.5, false));
        assert_eq!(conv("125", -3), (0.125, false));
        // Every integer below 2^53 is exact, whatever route it takes.
        assert_eq!(conv("9007199254740992", 0), (9.007_199_254_740_992e15, false));
        assert_eq!(conv("90071992547409920000", -4), (9.007_199_254_740_992e15, false));
    }

    #[test]
    fn conversion_rounds_ties_to_even() {
        // 2^53 + 1 is exactly half-way; 2^53 has an even last bit and wins.
        assert_eq!(conv("9007199254740993", 0).0, 9_007_199_254_740_992.0);
        // 2^53 + 3 is half-way the other side, where the even neighbour is above.
        assert_eq!(conv("9007199254740995", 0).0, 9_007_199_254_740_996.0);
        // A tie broken by a sticky bit the caller reports rather than stores.
        assert_eq!(
            decimal_to_f64(b"9007199254740993", 0, true).0,
            9_007_199_254_740_994.0
        );
    }

    #[test]
    fn conversion_spans_the_whole_exponent_range() {
        assert_eq!(conv("17976931348623157", 292), (f64::MAX, false));
        assert_eq!(conv("5", -324), (f64::from_bits(1), true));
        assert_eq!(conv("1", -310), ("1e-310".parse::<f64>().unwrap(), true));
        // Just over half an ulp above zero rounds up to the least subnormal.
        assert_eq!(conv("2470328229206232720882843964341106861826", -363).0, f64::from_bits(1));
        // Exactly half rounds down, because zero is the even side.
        assert_eq!(conv("2470328229206232720882843964341106861825", -363).0, 0.0);
    }

    #[test]
    fn conversion_reports_the_erange_condition() {
        assert_eq!(conv("1", 400), (f64::INFINITY, true));
        assert_eq!(conv("1", -400), (0.0, true));
        // Normal results are never out of range; subnormals always are.
        assert!(!conv("1", 0).1);
        assert!(conv("1", -320).1);
        assert!(!conv("22250738585072014", -324).1, "least normal is in range");
    }

    #[test]
    fn conversion_matches_rusts_parser_over_a_sweep() {
        // Rust's `str::parse::<f64>()` is correctly rounded, so it decides.
        let mut st: u64 = 0x0123_4567_89AB_CDEF;
        for _ in 0..4000 {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            let v = f64::from_bits(st);
            if !v.is_finite() {
                continue;
            }
            let text = format!("{:.25e}", v.abs());
            let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
            let digits: String =
                mantissa.chars().filter(char::is_ascii_digit).collect();
            // `{:.25e}` writes one digit before the point and 25 after it.
            let exp10 = exponent.parse::<i32>().unwrap_or(0) - 25;
            assert_eq!(
                decimal_to_f64(digits.as_bytes(), exp10, false).0,
                text.parse::<f64>().unwrap_or(f64::NAN),
                "{text}"
            );
        }
    }

    #[test]
    fn f32_conversion_rounds_once_not_twice() {
        // A hair above the midpoint between 1.0f32 and its successor, but
        // close enough to that midpoint that rounding to f64 first lands
        // exactly on it — at which point ties-to-even wrongly picks 1.0.
        let text = "1.000000059604644830901776231257827021181583404541015625";
        let digits: String = text.chars().filter(char::is_ascii_digit).collect();
        let exp10 = -(text.len() as i32 - 2);
        let (via_f64, _) = decimal_to_f64(digits.as_bytes(), exp10, false);
        assert_eq!(via_f64 as f32, 1.0_f32, "the trap this test guards against");
        let (direct, _) = decimal_to_f32(digits.as_bytes(), exp10, false);
        assert_eq!(direct.to_bits(), 1.0_f32.to_bits() + 1);
    }

    #[test]
    fn f32_conversion_spans_its_own_range() {
        assert_eq!(decimal_to_f32(b"1", 0, false), (1.0_f32, false));
        assert_eq!(decimal_to_f32(b"34028235", 31, false), (f32::MAX, false));
        // The least f32 subnormal, and half of it (a tie that rounds to zero).
        assert_eq!(decimal_to_f32(b"14", -46, false).0, f32::from_bits(1));
        assert_eq!(decimal_to_f32(b"7", -46, false).0, 0.0_f32);
        // In range for f64, out of range for f32.
        assert_eq!(decimal_to_f32(b"1", 39, false), (f32::INFINITY, true));
        assert_eq!(decimal_to_f32(b"1", -46, false), (0.0_f32, true));
    }

    #[test]
    fn f32_conversion_matches_rusts_parser_over_a_sweep() {
        let mut st: u64 = 0xDEAD_BEEF_1234_5678;
        for _ in 0..4000 {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            let v = f32::from_bits((st >> 32) as u32);
            if !v.is_finite() {
                continue;
            }
            let text = format!("{:.20e}", v.abs());
            let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
            let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
            let exp10 = exponent.parse::<i32>().unwrap_or(0) - 20;
            assert_eq!(
                decimal_to_f32(digits.as_bytes(), exp10, false).0,
                text.parse::<f32>().unwrap_or(f32::NAN),
                "{text}"
            );
        }
    }
}
