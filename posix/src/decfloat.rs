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

/// Which way a conversion rounds what it cannot keep. glibc's `printf` and
/// `strtod` both follow the current rounding direction (`fesetround`), and
/// so do these: the printed digits of 0.25 to one place are "0.3" upward and
/// "0.2" downward, and `strtod("0.1")` is a different `double` either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rounding {
    /// To the nearest, ties to even (`FE_TONEAREST`).
    Nearest,
    /// Toward positive infinity (`FE_UPWARD`).
    Upward,
    /// Toward negative infinity (`FE_DOWNWARD`).
    Downward,
    /// Toward zero (`FE_TOWARDZERO`).
    TowardZero,
}

impl Rounding {
    /// The current direction, as `fegetround` reads it (the x87 control
    /// word; `fesetround` keeps the SSE unit's the same).
    pub(crate) fn current() -> Self {
        match crate::fenv::fegetround() {
            crate::fenv::FE_UPWARD => Self::Upward,
            crate::fenv::FE_DOWNWARD => Self::Downward,
            crate::fenv::FE_TOWARDZERO => Self::TowardZero,
            _ => Self::Nearest,
        }
    }

    /// For a directed mode, whether a magnitude of this sign that has lost
    /// something nonzero goes up (away from zero): upward for a positive
    /// value, downward for a negative one. Meaningless for `Nearest`.
    pub(crate) fn away(self, negative: bool) -> bool {
        match self {
            Self::Upward => !negative,
            Self::Downward => negative,
            Self::Nearest | Self::TowardZero => false,
        }
    }

    /// Whether a magnitude of sign `negative` rounds up (away from zero),
    /// given what was `cut` off it and whether its last kept digit or bit is
    /// `odd`. The one rule for decimal digits and binary bits alike.
    pub(crate) fn rounds_up(self, negative: bool, odd: bool, cut: Cut) -> bool {
        match self {
            Self::Nearest => cut == Cut::AboveHalf || (cut == Cut::Half && odd),
            _ => cut != Cut::Zero && self.away(negative),
        }
    }
}

/// What rounding cut off a magnitude, measured against one unit of the last
/// place it kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cut {
    /// Nothing: the value was exact at that place.
    Zero,
    /// Something, but less than half a unit.
    BelowHalf,
    /// Exactly half a unit: a tie.
    Half,
    /// More than half a unit.
    AboveHalf,
}

impl Cut {
    /// From the first cut bit (the guard) and whether any bit below it is set.
    pub(crate) fn of_bits(guard: bool, rest: bool) -> Self {
        match (guard, rest) {
            (false, false) => Self::Zero,
            (false, true) => Self::BelowHalf,
            (true, false) => Self::Half,
            (true, true) => Self::AboveHalf,
        }
    }

    /// From the first cut decimal digit (ASCII) and whether any nonzero
    /// digit follows it.
    pub(crate) fn of_digit(first: u8, rest: bool) -> Self {
        match first {
            b'5' if !rest => Self::Half,
            b'5'..=b'9' => Self::AboveHalf,
            b'0' if !rest => Self::Zero,
            _ => Self::BelowHalf,
        }
    }

    /// From the cut part `rest` of a place whose half is `half`.
    pub(crate) fn of_part(rest: u64, half: u64) -> Self {
        if rest == 0 {
            return Self::Zero;
        }
        match rest.cmp(&half) {
            core::cmp::Ordering::Less => Self::BelowHalf,
            core::cmp::Ordering::Equal => Self::Half,
            core::cmp::Ordering::Greater => Self::AboveHalf,
        }
    }
}

/// glibc 2.39's `printf` and `strto*` answers, in every rounding mode
/// (`posix/tools/oracle/conv_harness.py`), for the tests of `printf.rs` and
/// `stdlib.rs` to replay: one copy for both.
#[cfg(test)]
pub(crate) const CONV_ORACLE: &str = include_str!("conv_oracle.txt");

/// Number of 64-bit limbs needed for the largest exact expansion.
///
/// The worst case is the smallest subnormal: `m` has 53 bits and `5^1074` has
/// `ceil(1074 * log2 5) = 2495`, for 2548 bits total.  (The large-exponent
/// case, `m << 971`, needs only 1024.)
const DEC_LIMBS: usize = 40;

/// Number of limbs for the parsing direction, which scales the significand up
/// by `2^L` before dividing so that the quotient is long enough to round.
/// See [`decimal_to_binary`]; the worst case there is about 5165 bits.
const PARSE_LIMBS: usize = 96;

/// Largest number of significant decimal digits that can affect *which* `f64`
/// an input rounds to.  Past this, further digits can only decide whether the
/// value sits exactly on a rounding boundary, which a sticky bit records.
pub(crate) const MAX_PARSE_DIGITS: usize = 768;

/// The same for a `long double`. A rounding boundary there is an odd
/// multiple of `2^-16446` at the finest, below `2^65` times it, so its
/// decimal expansion runs to `log10(5^16446 * 2^65)` -- 11,515 -- significant
/// digits, and an input agreeing with one that far is decided by the digits
/// after.
pub(crate) const LD_PARSE_DIGITS: usize = 11_520;

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

/// Element types a [`MallocBuf`] may hold: those for which all-zero bytes
/// are a value, so a `calloc` block is initialised.
pub(crate) trait Zeroable: Copy {}
impl Zeroable for u8 {}
impl Zeroable for u64 {}
impl Zeroable for usize {}

/// A `malloc` block of zeroed `T`s, freed when this goes: the storage for the
/// `long double` conversions, whose worst cases -- a thousand limbs, eleven
/// thousand digits -- are too big for a stack. A `printf` or `strtold` that
/// cannot get one fails with `ENOMEM`, as glibc's do.
pub(crate) struct MallocBuf<T: Zeroable> {
    ptr: *mut T,
    len: usize,
}

impl<T: Zeroable> MallocBuf<T> {
    /// `len` zeroed `T`s (at least one), or `None` when memory runs out.
    pub(crate) fn zeroed(len: usize) -> Option<Self> {
        let len = len.max(1);
        let ptr = crate::malloc::calloc(len, core::mem::size_of::<T>()).cast::<T>();
        (!ptr.is_null()).then_some(Self { ptr, len })
    }
}

impl<T: Zeroable> AsRef<[T]> for MallocBuf<T> {
    fn as_ref(&self) -> &[T] {
        // SAFETY: `ptr` is this buffer's own block of `len` initialised `T`s
        // (zeroed by `calloc`, a value for a `Zeroable`).
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl<T: Zeroable> AsMut<[T]> for MallocBuf<T> {
    fn as_mut(&mut self) -> &mut [T] {
        // SAFETY: as in `as_ref`, and `&mut self` makes the borrow unique.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl<T: Zeroable> Drop for MallocBuf<T> {
    fn drop(&mut self) {
        // SAFETY: `ptr` is this buffer's own `calloc` block.
        unsafe { crate::malloc::free(self.ptr.cast()) };
    }
}

/// A fixed-capacity unsigned big integer, little-endian limbs, in storage
/// `S`: a `[u64; N]` for the `f64` conversions, whose worst cases fit a
/// stack, or [`Limbs`] for the `long double` ones, whose worst cases (over a
/// thousand limbs) do not.
struct Big<S> {
    limbs: S,
    /// Number of significant limbs; `0` means the value is zero.
    len: usize,
}

impl<const N: usize> Big<[u64; N]> {
    fn from_u64(v: u64) -> Self {
        Self::with([0u64; N], v)
    }
}

/// A bignum's limbs where the size is known only at run time: inline while
/// they fit a `double`'s worst case, in a block of their own past it.
// The inline variant's size is the point: it is what lets an ordinary
// `long double` conversion run without an allocation, and the value is built
// in place, in one frame, never moved about.
#[allow(clippy::large_enum_variant)]
enum Limbs {
    Inline([u64; PARSE_LIMBS]),
    Heap(MallocBuf<u64>),
}

impl AsRef<[u64]> for Limbs {
    fn as_ref(&self) -> &[u64] {
        match self {
            Self::Inline(limbs) => limbs,
            Self::Heap(block) => block.as_ref(),
        }
    }
}

impl AsMut<[u64]> for Limbs {
    fn as_mut(&mut self) -> &mut [u64] {
        match self {
            Self::Inline(limbs) => limbs,
            Self::Heap(block) => block.as_mut(),
        }
    }
}

impl<S: AsRef<[u64]> + AsMut<[u64]>> Big<S> {
    /// `v` in `limbs`, which are all zero.
    fn with(mut limbs: S, v: u64) -> Self {
        let len = match limbs.as_mut().first_mut() {
            Some(first) if v != 0 => {
                *first = v;
                1
            }
            _ => 0,
        };
        Self { limbs, len }
    }

    fn l(&self) -> &[u64] {
        self.limbs.as_ref()
    }

    fn lm(&mut self) -> &mut [u64] {
        self.limbs.as_mut()
    }

    /// How many limbs the storage holds.
    fn cap(&self) -> usize {
        self.l().len()
    }

    fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// `self *= x`.  Saturates by dropping the overflow, which cannot happen
    /// for the inputs this module produces: the storage is sized for the
    /// worst case and every caller stays inside it.
    #[allow(clippy::arithmetic_side_effects)]
    fn mul_small(&mut self, x: u64) {
        if x == 0 || self.is_zero() {
            self.len = 0;
            return;
        }
        let mut carry: u128 = 0;
        for i in 0..self.len {
            // SAFETY-of-indexing: `i < self.len <= LIMBS`.
            let Some(slot) = self.lm().get_mut(i) else {
                break;
            };
            let prod = u128::from(*slot) * u128::from(x) + carry;
            *slot = prod as u64;
            carry = prod >> 64;
        }
        while carry != 0 && self.len < self.cap() {
            let at = self.len;
            if let Some(slot) = self.lm().get_mut(at) {
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
        let new_len = (old_len + whole + usize::from(part != 0)).min(self.cap());
        let mut i = new_len;
        while i > 0 {
            i -= 1;
            let hi = i.checked_sub(whole).and_then(|j| self.l().get(j)).copied();
            let lo = i
                .checked_sub(whole)
                .and_then(|j| j.checked_sub(1))
                .and_then(|j| self.l().get(j))
                .copied();
            let v = match (hi, lo, part) {
                (Some(h), _, 0) => h,
                (Some(h), Some(l), p) => (h << p) | (l >> (64 - p)),
                (Some(h), None, p) => h << p,
                (None, _, _) => 0,
            };
            if let Some(slot) = self.lm().get_mut(i) {
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
            let Some(slot) = self.lm().get_mut(i) else {
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
            let Some(slot) = self.lm().get_mut(i) else {
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
        match self.len.checked_sub(1).and_then(|i| self.l().get(i)) {
            Some(&top) if top != 0 => (self.len - 1) * 64 + (64 - top.leading_zeros() as usize),
            _ => 0,
        }
    }

    /// Is bit `i` set?
    #[allow(clippy::arithmetic_side_effects)]
    fn bit(&self, i: usize) -> bool {
        self.l()
            .get(i / 64)
            .is_some_and(|&w| (w >> (i % 64)) & 1 == 1)
    }

    /// Is any bit strictly below index `i` set?  This is the sticky test.
    #[allow(clippy::arithmetic_side_effects)]
    fn any_bits_below(&self, i: usize) -> bool {
        let top = i / 64;
        let off = (i % 64) as u32;
        for k in 0..top.min(self.len) {
            if self.l().get(k).copied().unwrap_or(0) != 0 {
                return true;
            }
        }
        off != 0
            && self
                .l()
                .get(top)
                .is_some_and(|&w| w & ((1u64 << off) - 1) != 0)
    }

    /// The 64-bit window of `self` starting at bit `i`, i.e. `(self >> i)` as
    /// a `u64`.  Callers guarantee the shifted value fits.
    #[allow(clippy::arithmetic_side_effects)]
    fn window(&self, i: usize) -> u64 {
        let idx = i / 64;
        let off = (i % 64) as u32;
        let lo = self.l().get(idx).copied().unwrap_or(0);
        if off == 0 {
            lo
        } else {
            let hi = self.l().get(idx + 1).copied().unwrap_or(0);
            (lo >> off) | (hi << (64 - off))
        }
    }

    #[allow(clippy::arithmetic_side_effects)]
    fn normalize(&mut self) {
        while self.len > 0 && self.l().get(self.len - 1).copied() == Some(0) {
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

/// The exact decimal expansion of a finite, non-negative binary value.
///
/// The value is `0.d[0]d[1]…d[len-1] * 10^decpt` — that is, `decpt` is the
/// number of digits that lie before the decimal point, and may be zero or
/// negative (the value is below 1) or greater than `len` (the value has
/// trailing zeros before the point).  Trailing zero digits are always
/// stripped, so `d[len-1]` is never `b'0'` and zero is `len == 0`.
///
/// The digits live in `D`: a fixed array for an `f64` ([`Decimal::new`]),
/// whose 767 digits at most fit a stack, or a [`DigitBuf`] for a `long
/// double` ([`Decimal::of_parts`]), whose can run to eleven thousand.
pub(crate) struct Decimal<D = [u8; MAX_DIGITS]> {
    digits: D,
    len: usize,
    decpt: i32,
}

/// Make `big` hold `big * 2^e / 10^scale` exactly, and return `scale`: a
/// shift for `e >= 0`, else a multiplication by `5^-e`, since
/// `2^e == 5^-e * 10^e` clears a binary exponent into a decimal one.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn scale_to_decimal<S: AsRef<[u64]> + AsMut<[u64]>>(big: &mut Big<S>, e: i32) -> i32 {
    if e >= 0 {
        big.shl(e as u32);
        return 0;
    }
    // Applied `POW5_CHUNK_EXP` at a time because that is the most that fits
    // in a limb multiplier.
    let mut left = e.unsigned_abs();
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
}

/// Write `big * 10^scale` into `out` as an exact expansion -- `out` must hold
/// all of `big`'s digits -- most significant first, trailing zeros stripped:
/// `(len, decpt)`. `big` is consumed (left zero).
///
/// The digits are produced least significant first, a `10^19` chunk at a time,
/// into the end of `out`, and then moved to its front.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn expand_into<S: AsRef<[u64]> + AsMut<[u64]>>(
    big: &mut Big<S>,
    scale: i32,
    out: &mut [u8],
) -> (usize, i32) {
    let cap = out.len();
    let mut pos = cap;
    while !big.is_zero() {
        let mut chunk = big.divmod_small(POW10_CHUNK);
        let last = big.is_zero();
        let mut emitted = 0usize;
        while pos > 0 && (chunk != 0 || (!last && emitted < POW10_CHUNK_EXP)) {
            pos -= 1;
            if let Some(slot) = out.get_mut(pos) {
                *slot = b'0' + (chunk % 10) as u8;
            }
            chunk /= 10;
            emitted += 1;
        }
        debug_assert!(chunk == 0, "decimal buffer too small");
    }
    let total = cap - pos;
    // Strip trailing zeros; `decpt` accounts for their place.
    let mut end = cap;
    while end > pos && out.get(end - 1).copied() == Some(b'0') {
        end -= 1;
    }
    let len = end - pos;
    out.copy_within(pos..end, 0);
    if len == 0 {
        return (0, 0);
    }
    // `scale` counts the digits that sit to the right of the point.
    (len, i32::try_from(total).unwrap_or(i32::MAX) + scale)
}

impl Decimal {
    /// Compute the exact expansion of `val`, which must be finite and `>= 0`.
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
        let mut big = Big::<[u64; DEC_LIMBS]>::from_u64(m);
        let scale = scale_to_decimal(&mut big, e);
        (out.len, out.decpt) = expand_into(&mut big, scale, &mut out.digits);
        out
    }
}

/// An expansion's digits where their count is known only at run time:
/// inline while they fit a `double`'s worst case, in a block of their own
/// past it.
// As for `Limbs`: the inline variant is what keeps an ordinary `%Lf` from
// allocating.
#[allow(clippy::large_enum_variant)]
pub(crate) enum DigitBuf {
    Inline([u8; MAX_DIGITS]),
    Heap(MallocBuf<u8>),
}

impl AsRef<[u8]> for DigitBuf {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Inline(digits) => digits,
            Self::Heap(block) => block.as_ref(),
        }
    }
}

impl AsMut<[u8]> for DigitBuf {
    fn as_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Inline(digits) => digits,
            Self::Heap(block) => block.as_mut(),
        }
    }
}

impl Decimal<DigitBuf> {
    /// The exact expansion of `m * 2^e` -- any 64-bit significand, any
    /// exponent: a `long double`'s, whose integer bit is explicit. `None`
    /// when memory runs out.
    ///
    /// The integer needs `bits(m) + e` bits for `e >= 0`, else `bits(m) +
    /// ceil(-e * log2 5)`: at most about 38,300, for the least subnormal
    /// `long double`, `2^-16445`, whose expansion has 11,496 significant
    /// digits. The integer and the digits are on the stack while they fit a
    /// `double`'s worst case -- every value from about `1e-300` to `1e760`
    /// -- and in blocks sized to the value past that.
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn of_parts(m: u64, e: i32) -> Option<Self> {
        if m == 0 {
            return Some(Self {
                digits: DigitBuf::Inline([0; MAX_DIGITS]),
                len: 0,
                decpt: 0,
            });
        }
        // An odd significand keeps the power of five as small as it can be.
        let tz = m.trailing_zeros();
        let m = m >> tz;
        let e = e.saturating_add(i32::try_from(tz).unwrap_or(0));
        // `bits(m)`; `m` is not 0 here, which returned above.
        let m_bits = u64::from(m.checked_ilog2().map_or(0, |top| top + 1));
        let e_bits = if e >= 0 {
            u64::from(e.unsigned_abs())
        } else {
            // log2 5 < 2.321929, so this never falls short.
            (u64::from(e.unsigned_abs()) * 2_321_929).div_ceil(1_000_000)
        };
        let bits = m_bits + e_bits;
        let limbs = usize::try_from(bits / 64 + 2).ok()?;
        let mut big = if limbs <= PARSE_LIMBS {
            Big::with(Limbs::Inline([0; PARSE_LIMBS]), m)
        } else {
            Big::with(Limbs::Heap(MallocBuf::zeroed(limbs)?), m)
        };
        let scale = scale_to_decimal(&mut big, e);
        // log10 2 < 0.30103: every digit the integer has, and a spare.
        let ndigits = usize::try_from((bits * 30_103).div_ceil(100_000) + 2).ok()?;
        let mut digits = if ndigits <= MAX_DIGITS {
            DigitBuf::Inline([0; MAX_DIGITS])
        } else {
            DigitBuf::Heap(MallocBuf::zeroed(ndigits)?)
        };
        let (len, decpt) = expand_into(&mut big, scale, digits.as_mut());
        Some(Self { digits, len, decpt })
    }
}

impl<D: AsRef<[u8]> + AsMut<[u8]>> Decimal<D> {
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
            Ok(u) if u < self.len => self.digits.as_ref().get(u).copied().unwrap_or(b'0'),
            _ => b'0',
        }
    }

    /// Round to at most `n` significant digits, ties to even.
    ///
    /// `n == 0` asks whether the value reaches half of `10^decpt`; a negative
    /// `n` is below even that and rounds to zero.
    #[cfg(test)]
    pub(crate) fn round_to_significant(&mut self, n: i32) {
        self.round_to_significant_in(n, Rounding::Nearest, false);
    }

    /// Round to at most `n` significant digits in direction `dir`, for a
    /// value of sign `negative` (a directed mode rounds a magnitude by its
    /// sign: upward is away from zero for a positive value, toward it for a
    /// negative one).
    ///
    /// `n == 0` puts the rounding place just above the leading digit; a
    /// negative `n` puts it higher still. Rounding to nearest makes such a
    /// value zero (or, for `n == 0`, one unit if it reaches half); rounding
    /// away from zero makes it one unit of that place, for it is not zero.
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn round_to_significant_in(&mut self, n: i32, dir: Rounding, negative: bool) {
        if self.len == 0 {
            return;
        }
        let Ok(keep) = usize::try_from(n) else {
            // Every digit is below the rounding place.
            if dir != Rounding::Nearest && dir.away(negative) {
                // One unit of the place, `10^(decpt - n)`.
                if let Some(slot) = self.digits.as_mut().get_mut(0) {
                    *slot = b'1';
                }
                self.len = 1;
                self.decpt = self.decpt.saturating_sub(n).saturating_add(1);
            } else {
                // Strictly below half of the place: zero.
                self.len = 0;
                self.decpt = 0;
            }
            return;
        };
        if keep >= self.len {
            return;
        }

        // Decide the direction from the first dropped digit and whether any
        // nonzero digit follows it.  Because the expansion is exact, "5 with
        // nothing after" is precisely a tie — no separate analysis needed.
        let first_dropped = self.digits.as_ref().get(keep).copied().unwrap_or(b'0');
        let rest_nonzero = self
            .digits
            .as_ref()
            .get(keep.wrapping_add(1)..self.len)
            .is_some_and(|tail| tail.iter().any(|&d| d != b'0'));
        // A dropped leading digit leaves an implicit 0 kept, which is even.
        let prev_odd = keep
            .checked_sub(1)
            .and_then(|i| self.digits.as_ref().get(i))
            .is_some_and(|&d| d.wrapping_sub(b'0') % 2 == 1);
        // The digits are stripped of trailing zeros, so something nonzero is
        // always cut here: the last digit, at the least.
        let round_up = dir.rounds_up(
            negative,
            prev_odd,
            Cut::of_digit(first_dropped, rest_nonzero),
        );

        self.len = keep;
        if round_up {
            let mut i = keep;
            loop {
                if i == 0 {
                    // Carried out of the most significant digit: the result is
                    // a single 1 one place higher.
                    if let Some(slot) = self.digits.as_mut().get_mut(0) {
                        *slot = b'1';
                    }
                    self.len = 1;
                    self.decpt += 1;
                    return;
                }
                i -= 1;
                match self.digits.as_mut().get_mut(i) {
                    Some(slot) if *slot == b'9' => *slot = b'0',
                    Some(slot) => {
                        *slot += 1;
                        break;
                    }
                    None => break,
                }
            }
        }
        while self.len > 0 && self.digits.as_ref().get(self.len - 1).copied() == Some(b'0') {
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
    #[cfg(test)]
    pub(crate) fn round_to_place(&mut self, p: i32) {
        self.round_to_place_in(p, Rounding::Nearest, false);
    }

    /// Round so that no digit lies below the `10^-p` place, in direction
    /// `dir` for a value of sign `negative` (see
    /// [`Decimal::round_to_significant_in`]): the digit at `10^-p` is
    /// significant index `decpt + p - 1`, so that keeps `decpt + p` digits.
    pub(crate) fn round_to_place_in(&mut self, p: i32, dir: Rounding, negative: bool) {
        if self.len == 0 {
            return;
        }
        self.round_to_significant_in(self.decpt.saturating_add(p), dir, negative);
    }

    /// Number of significant digits remaining.
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

/// Accumulates the significant digits of a floating-point literal exactly.
///
/// `strtod`, `wcstod` and `scanf`'s `%f`/`%e`/`%g` read their input from
/// different places — a C string, a wide string and a scan context — but the
/// digit bookkeeping is identical, and getting it subtly wrong is exactly how
/// a parser loses precision.  Driving one collector from all of them keeps
/// them in step.
///
/// A literal is either decimal or hexadecimal, and the collector holds both:
/// the value is `digits * 10^exp` for the first and `digits * 2^exp` for the
/// second, where a hex digit moves the point by four bits rather than one
/// decimal place.  Digits past the cap are not stored — they cannot change
/// *which* value the input rounds to, only whether it lands on a rounding
/// boundary — but they are still accounted for, either in the exponent or in
/// the sticky bit.
pub(crate) struct DigitCollector {
    /// The first significant digits as ASCII, most significant first.
    digits: [u8; MAX_PARSE_DIGITS],
    /// Every significant digit so far, once there are more than `digits`
    /// holds: a block of `limit` bytes, which only a `long double`'s
    /// collector ever needs.
    spill: Option<MallocBuf<u8>>,
    /// How many significant digits can decide the rounding:
    /// [`MAX_PARSE_DIGITS`] for a `double` or a `float`, [`LD_PARSE_DIGITS`]
    /// for a `long double`. Lowered to the digits stored if the spill cannot
    /// be allocated, which `out_of_memory` records.
    limit: usize,
    out_of_memory: bool,
    len: usize,
    /// Power of ten for a decimal literal, power of two for a hex one.
    exp: i32,
    truncated: bool,
    hex: bool,
}

impl DigitCollector {
    /// A collector for a `double` or a `float`, which never allocates.
    pub(crate) const fn new() -> Self {
        Self::with_limit(MAX_PARSE_DIGITS)
    }

    /// A collector for a `long double`: the first [`MAX_PARSE_DIGITS`]
    /// significant digits inline, as for a `double`, and a block for all of
    /// them -- [`LD_PARSE_DIGITS`] at the most -- only when there are more.
    pub(crate) const fn for_long_double() -> Self {
        Self::with_limit(LD_PARSE_DIGITS)
    }

    const fn with_limit(limit: usize) -> Self {
        Self {
            digits: [b'0'; MAX_PARSE_DIGITS],
            spill: None,
            limit,
            out_of_memory: false,
            len: 0,
            exp: 0,
            truncated: false,
            hex: false,
        }
    }

    /// Switch to hexadecimal, on seeing a `0x` prefix.
    ///
    /// Must be called before any digit is pushed; the scanner does so as soon
    /// as it commits to the hex grammar.
    pub(crate) fn set_hex(&mut self) {
        self.hex = true;
    }

    /// How many digits fit, and what one digit is worth in the exponent.
    ///
    /// Hex digits are capped far lower because they carry four bits each: 20
    /// of them are 80 bits, already more than a significand plus its guard and
    /// round bits, and unlike a decimal literal there is no base conversion
    /// that could let a distant digit matter.
    const fn shape(&self) -> (usize, i32) {
        if self.hex {
            (MAX_HEX_DIGITS, 4)
        } else {
            (self.limit, 1)
        }
    }

    /// Add a digit that appeared before the point.
    ///
    /// One that does not fit still scales everything already stored, hence the
    /// exponent bump; only its own contribution is lost, to the sticky bit.
    pub(crate) fn push_integer(&mut self, ascii: u8) {
        let (cap, step) = self.shape();
        if self.len == 0 && ascii == b'0' {
            // A leading zero contributes nothing at all.
            return;
        }
        if self.len >= cap || !self.store(ascii) {
            self.exp = self.exp.saturating_add(step);
            if ascii != b'0' {
                self.truncated = true;
            }
        }
    }

    /// Add a digit that appeared after the point.
    ///
    /// Each stored digit moves the point one place right, and so does each
    /// leading zero that precedes the first significant digit.  A digit that
    /// does not fit sits entirely below the last stored one, so it is pure
    /// sticky and does not touch the exponent.
    pub(crate) fn push_fraction(&mut self, ascii: u8) {
        let (cap, step) = self.shape();
        if self.len == 0 && ascii == b'0' {
            self.exp = self.exp.saturating_sub(step);
        } else if self.len < cap && self.store(ascii) {
            self.exp = self.exp.saturating_sub(step);
        } else if ascii != b'0' {
            self.truncated = true;
        }
    }

    /// Apply an explicit `e[+-]NN` (decimal) or `p[+-]NN` (hex) exponent.
    pub(crate) fn apply_exponent(&mut self, exp: i32) {
        self.exp = self.exp.saturating_add(exp);
    }

    /// Store a significant digit, below `limit`: inline while it fits,
    /// then in the spill, which the first digit past the inline array
    /// allocates. `false` if that allocation failed -- the collector then
    /// stores nothing more, and [`DigitCollector::to_ld80`] answers `None`.
    fn store(&mut self, ascii: u8) -> bool {
        if self.spill.is_none() {
            if let Some(slot) = self.digits.get_mut(self.len) {
                *slot = ascii;
                self.len = self.len.saturating_add(1);
                return true;
            }
            let Some(mut block) = MallocBuf::<u8>::zeroed(self.limit) else {
                self.out_of_memory = true;
                self.limit = self.len;
                return false;
            };
            if let Some(head) = block.as_mut().get_mut(..self.len) {
                head.copy_from_slice(self.digits.get(..self.len).unwrap_or(&[]));
            }
            self.spill = Some(block);
        }
        let Some(slot) = self
            .spill
            .as_mut()
            .and_then(|b| b.as_mut().get_mut(self.len))
        else {
            return false;
        };
        *slot = ascii;
        self.len = self.len.saturating_add(1);
        true
    }

    fn stored(&self) -> &[u8] {
        match &self.spill {
            Some(block) => block.as_ref().get(..self.len).unwrap_or(&[]),
            None => self.digits.get(..self.len).unwrap_or(&[]),
        }
    }

    /// The accumulated value's magnitude rounded into `fmt` in the current
    /// direction for a value of sign `negative`, and the `ERANGE` condition
    /// ([`round_to_binary`]); a decimal literal's bignum on the stack.
    fn to_rounded(&self, fmt: &Format, negative: bool) -> (Rounded, bool) {
        let dir = Rounding::current();
        if self.hex {
            hex_to_binary(self.stored(), self.exp, self.truncated, fmt, dir, negative)
        } else {
            decimal_to_binary(
                self.stored(),
                self.exp,
                self.truncated,
                fmt,
                dir,
                negative,
                |limbs| {
                    debug_assert!(limbs <= PARSE_LIMBS, "a double's bignum outgrew the stack");
                    Some(Big::<[u64; PARSE_LIMBS]>::from_u64(0))
                },
            )
            .unwrap_or((Rounded::Finite { field: 0, m: 0 }, false))
        }
    }

    /// The raw bits of `fmt` for [`DigitCollector::to_rounded`].
    fn to_bits(&self, fmt: &Format, negative: bool) -> (u64, bool) {
        let (r, oor) = self.to_rounded(fmt, negative);
        (r.pack_ieee(fmt), oor)
    }

    /// The accumulated value's magnitude as a `long double`, rounded in the
    /// current direction for a value of sign `negative`, as glibc's `strtold`
    /// rounds it: `(magnitude, out_of_range)`.
    ///
    /// The bignum is on the stack while it fits a `double`'s worst case,
    /// which covers a literal of a few digits for any value from about
    /// `1e-2550` to `1e1800`, and one of hundreds over a narrower span; past
    /// that it is a block sized to the literal, twelve hundred limbs at the
    /// most. `None` when that block, or the collector's spill, could not be
    /// allocated.
    pub(crate) fn to_ld80(&self, negative: bool) -> Option<(crate::x87::LongDouble, bool)> {
        if self.out_of_memory {
            return None;
        }
        let dir = Rounding::current();
        let (r, oor) = if self.hex {
            hex_to_binary(
                self.stored(),
                self.exp,
                self.truncated,
                &LD80_FORMAT,
                dir,
                negative,
            )
        } else {
            decimal_to_binary(
                self.stored(),
                self.exp,
                self.truncated,
                &LD80_FORMAT,
                dir,
                negative,
                |limbs| {
                    if limbs <= PARSE_LIMBS {
                        Some(Big::with(Limbs::Inline([0; PARSE_LIMBS]), 0))
                    } else {
                        Some(Big::with(Limbs::Heap(MallocBuf::zeroed(limbs)?), 0))
                    }
                },
            )?
        };
        let (field, m) = r.pack_x87();
        Some((crate::x87::LongDouble::from_bits(field, m), oor))
    }

    /// The accumulated value's magnitude as an `f64`, rounded in the current
    /// direction (`fesetround`) for a value of sign `negative`, as glibc's
    /// `strtod` rounds it: `(magnitude, out_of_range)`, the second being the
    /// `ERANGE` condition of [`round_to_binary`].
    pub(crate) fn to_f64(&self, negative: bool) -> (f64, bool) {
        let (bits, oor) = self.to_bits(&F64_FORMAT, negative);
        (f64::from_bits(bits), oor)
    }

    /// As [`DigitCollector::to_f64`], for an `f32`.
    ///
    /// Rounds straight from the literal rather than by way of `f64`; see
    /// `decimal_to_f32` for why that distinction matters.
    pub(crate) fn to_f32(&self, negative: bool) -> (f32, bool) {
        let (bits, oor) = self.to_bits(&F32_FORMAT, negative);
        (f32::from_bits(u32::try_from(bits).unwrap_or(0)), oor)
    }
}

/// Largest number of significant hex digits that can affect the result.
///
/// Twenty digits are 80 bits, 77 of them significant at the least (the first
/// digit may be a 1): more than a `long double`'s 64-bit significand plus the
/// guard bit, and so more than a `double`'s.  Anything past that can only tell
/// the rounding whether the tail is nonzero, which is what the sticky bit is
/// for.
const MAX_HEX_DIGITS: usize = 20;

/// Limbs needed for [`MAX_HEX_DIGITS`] digits: 80 bits fits in two.
const HEX_LIMBS: usize = 2;

/// The value of an ASCII hexadecimal digit.
// The subtractions cannot wrap and the additions cannot overflow: each arm
// has already established the range `ascii` is in.
#[allow(clippy::arithmetic_side_effects)]
const fn hex_val(ascii: u8) -> u64 {
    match ascii {
        b'0'..=b'9' => (ascii - b'0') as u64,
        b'a'..=b'f' => (ascii - b'a' + 10) as u64,
        b'A'..=b'F' => (ascii - b'A' + 10) as u64,
        _ => 0,
    }
}

/// Convert `digits * 2^exp2` — the digits being hexadecimal — to `fmt`.
///
/// Hexadecimal is where floating-point text becomes easy: sixteen is a power
/// of two, so the digits *are* the bits and there is no base conversion at
/// all.  The whole job is to assemble them into an integer and hand it to the
/// same rounder the decimal path uses, which is what makes `%a` output read
/// back bit-for-bit identical.
///
/// `truncated` is the sticky bit for digits the caller could not store.
/// Returns `(bits, out_of_range)` as [`decimal_to_binary`] does.
fn hex_to_binary(
    digits: &[u8],
    exp2: i32,
    truncated: bool,
    fmt: &Format,
    dir: Rounding,
    negative: bool,
) -> (Rounded, bool) {
    let mut b = Big::<[u64; HEX_LIMBS]>::from_u64(0);
    for &d in digits {
        b.mul_small(16);
        b.add_small(hex_val(d));
    }
    round_to_binary(&b, exp2, truncated, fmt, dir, negative)
}

// ---------------------------------------------------------------------------
// Subject-sequence scanner
// ---------------------------------------------------------------------------

/// A source of bytes for [`scan_float_token`].
///
/// `strtod` reads a C string and `wcstod` a wide one, but the grammar they
/// accept is identical and only the fetch differs.  Indices count *elements* of
/// the source, so a caller can turn the returned length straight into a pointer
/// offset.  A wide character outside ASCII reports as 0 and thereby ends the
/// subject sequence, exactly as a terminator does — no float syntax uses one.
pub(crate) trait ByteSource {
    /// The byte at `i`, or 0 at and past the end of the string.
    fn byte_at(&self, i: usize) -> u8;
}

/// What a floating-point subject sequence turned out to be.
///
/// `strtod`, `strtof` and `wcstod` differ only in the format they round to, so
/// the scan is shared and each finishes it in its own precision.  Rounding to
/// `f64` and narrowing afterwards would round twice, which is not the same as
/// rounding once — see `decimal_to_f32`.
pub(crate) enum FloatToken {
    /// No valid subject sequence.
    None,
    /// `nan`, with the payload glibc reads out of a `nan(n-char-sequence)`
    /// ([`nan_payload`]); `None` for the default NaN.
    Nan(Option<u64>),
    Infinity,
    /// Digits, accumulated into the caller's collector.
    Number,
}

/// A byte of an n-char-sequence, as glibc's `__strtod_nan` takes one:
/// `[0-9A-Za-z_]`.
pub(crate) const fn is_nchar(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The payload glibc's `__strtod_nan` (`stdlib/strtod_nan_main.c`) reads
/// from the n-chars at `start..end`: the run read as `strtoull(run, &e, 0)`
/// reads it -- `0x` hex, leading-`0` octal, else decimal -- and used only if
/// that read ends exactly at `end`. `None` otherwise, which is the default
/// NaN. A number past `ULLONG_MAX` is `ULLONG_MAX`, with `ERANGE`, as the
/// `strtoull` inside glibc's reads sets it.
pub(crate) fn nan_payload<S: ByteSource + ?Sized>(
    src: &S,
    start: usize,
    end: usize,
) -> Option<u64> {
    if start >= end {
        return None;
    }
    let at = |k: usize| src.byte_at(k);
    let hex = at(start) == b'0'
        && (at(start.wrapping_add(1)) | 0x20) == b'x'
        && start.wrapping_add(2) < end
        && at(start.wrapping_add(2)).is_ascii_hexdigit();
    let (mut k, radix) = if hex {
        (start.wrapping_add(2), 16)
    } else if at(start) == b'0' {
        (start.wrapping_add(1), 8)
    } else {
        (start, 10)
    };
    let mut v: u64 = 0;
    let mut overflow = false;
    while k < end {
        let d = char::from(at(k)).to_digit(radix)?;
        match v
            .checked_mul(u64::from(radix))
            .and_then(|m| m.checked_add(u64::from(d)))
        {
            Some(n) => v = n,
            None => overflow = true,
        }
        k = k.wrapping_add(1);
    }
    if overflow {
        crate::errno::set_errno(crate::errno::ERANGE);
        return Some(u64::MAX);
    }
    Some(v)
}

/// The quiet NaN with `payload` below its quiet bit, as glibc's
/// `SET_NAN_PAYLOAD` puts it there (and only if nonzero there), negated for
/// a `-nan`: glibc keeps the sign, `-nan` printing as `-nan`.
pub(crate) fn nan_f64(payload: Option<u64>, negative: bool) -> f64 {
    const LOW: u64 = (1 << 51) - 1;
    let mut bits = f64::NAN.to_bits();
    if let Some(p) = payload.filter(|p| p & LOW != 0) {
        bits |= p & LOW;
    }
    let v = f64::from_bits(bits);
    if negative { -v } else { v }
}

/// [`nan_f64`] for a float: 22 payload bits.
pub(crate) fn nan_f32(payload: Option<u64>, negative: bool) -> f32 {
    const LOW: u64 = (1 << 22) - 1;
    let mut bits = f32::NAN.to_bits();
    if let Some(p) = payload.filter(|p| p & LOW != 0) {
        // The mask keeps it under 2^22.
        #[allow(clippy::cast_possible_truncation)]
        let low = (p & LOW) as u32;
        bits |= low;
    }
    let v = f32::from_bits(bits);
    if negative { -v } else { v }
}

/// [`nan_f64`] for a `long double`: 62 payload bits, below x87's integer and
/// quiet bits -- glibc's ldbl-96 `SET_NAN_PAYLOAD`, as `nanl` has it.
pub(crate) fn nan_ld80(payload: Option<u64>, negative: bool) -> crate::x87::LongDouble {
    const LOW: u64 = (1 << 62) - 1;
    let sig = 0xC000_0000_0000_0000 | payload.map_or(0, |p| p & LOW);
    crate::x87::LongDouble::from_bits(if negative { 0xFFFF } else { 0x7FFF }, sig)
}

/// The `long double` a scanned subject sequence names, as glibc's `strtold`
/// gives it -- `(value, out_of_range)`, the second the C `ERANGE` condition
/// -- or `None` when its digits need more memory than there is
/// ([`DigitCollector::to_ld80`]).
pub(crate) fn ld80_of(
    token: FloatToken,
    negative: bool,
    acc: &DigitCollector,
) -> Option<(crate::x87::LongDouble, bool)> {
    let (magnitude, out_of_range) = match token {
        FloatToken::None => return Some((crate::x87::LongDouble::POS_ZERO, false)),
        FloatToken::Nan(p) => return Some((nan_ld80(p, negative), false)),
        FloatToken::Infinity => (crate::x87::LongDouble::INFINITY, false),
        FloatToken::Number => acc.to_ld80(negative)?,
    };
    let sign = if negative { 0x8000 } else { 0 };
    Some((
        crate::x87::LongDouble::from_bits(magnitude.sign_exp | sign, magnitude.significand),
        out_of_range,
    ))
}

/// A byte slice read as a C string: its bytes, then 0 forever.
pub(crate) struct SliceSource<'a>(pub(crate) &'a [u8]);

impl ByteSource for SliceSource<'_> {
    fn byte_at(&self, i: usize) -> u8 {
        self.0.get(i).copied().unwrap_or(0)
    }
}

/// ASCII whitespace, the set `strtod` skips before the subject sequence.
const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Scan a `strtod` subject sequence and report what it was.
///
/// Accepts `[whitespace][sign]` followed by `digits[.digits][e[sign]digits]`,
/// a hex float (`0x1.8p3`, the binary exponent optional), `inf`/`infinity`,
/// or `nan[(n-char-sequence)]`, the last two case-insensitively.  The
/// n-char-sequence is glibc's: letters, digits and `_`, closed by `)`; any
/// other byte before the `)` means only `nan` was the subject sequence, the
/// `(` left in place (glibc's `strtod_l.c`).
///
/// Returns `(token, negative, consumed)`.  `consumed` counts the elements that
/// belong to the subject sequence, and is 0 when there was none, which is
/// exactly where C says `*endptr` should land in each case.  The sign is
/// reported rather than applied so the caller negates in its own precision,
/// and the digits go into `acc` without being converted here.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
pub(crate) fn scan_float_token<S: ByteSource + ?Sized>(
    src: &S,
    acc: &mut DigitCollector,
) -> (FloatToken, bool, usize) {
    let mut i: usize = 0;

    // Skip whitespace.
    while is_space(src.byte_at(i)) {
        i = i.wrapping_add(1);
    }

    // Sign.
    let negative = src.byte_at(i) == b'-';
    if negative || src.byte_at(i) == b'+' {
        i = i.wrapping_add(1);
    }

    // "inf", "infinity", "nan" (case-insensitive).  Bytes are compared one at
    // a time and never past a terminator, which could sit at the end of a
    // mapped page.
    let c0 = src.byte_at(i);
    if c0 == 0 {
        // Empty subject string — fall through to digit parsing.
    } else if (c0 | 0x20) == b'i' {
        let c1 = src.byte_at(i.wrapping_add(1));
        if c1 != 0 {
            let c2 = src.byte_at(i.wrapping_add(2));
            if c2 != 0 && (c1 | 0x20) == b'n' && (c2 | 0x20) == b'f' {
                i = i.wrapping_add(3);
                // "infinity" extends "inf"; the longer match wins, but a
                // partial one must leave its bytes in place.
                let inity: [u8; 5] = *b"inity";
                let mut j: usize = 0;
                let mut all_match = true;
                while j < 5 {
                    let ch = src.byte_at(i.wrapping_add(j));
                    let expected = inity.get(j).copied().unwrap_or(0);
                    if ch == 0 || (ch | 0x20) != expected {
                        all_match = false;
                        break;
                    }
                    j = j.wrapping_add(1);
                }
                if all_match {
                    i = i.wrapping_add(5);
                }
                return (FloatToken::Infinity, negative, i);
            }
        }
    } else if (c0 | 0x20) == b'n' {
        let c1 = src.byte_at(i.wrapping_add(1));
        if c1 != 0 {
            let c2 = src.byte_at(i.wrapping_add(2));
            if c2 != 0 && (c1 | 0x20) == b'a' && (c2 | 0x20) == b'n' {
                i = i.wrapping_add(3);
                // The optional (n-char-sequence), and its payload. Until
                // 2026-09-27 any bytes up to a `)` were skipped and the
                // payload dropped.
                let mut payload = None;
                if src.byte_at(i) == b'(' {
                    let start = i.wrapping_add(1);
                    let mut j = start;
                    while is_nchar(src.byte_at(j)) {
                        j = j.wrapping_add(1);
                    }
                    if src.byte_at(j) == b')' {
                        payload = nan_payload(src, start, j);
                        i = j.wrapping_add(1);
                    }
                }
                return (FloatToken::Nan(payload), negative, i);
            }
        }
    }

    // Hexadecimal: a `0x` significand with an optional `p` exponent.  C99
    // requires `strtod` to accept this — it is how `%a` output reads back —
    // and unlike a source-code constant the binary exponent is optional here.
    //
    // A `0x` not followed by a hex digit is not a prefix at all: the subject
    // sequence is then just the `0`, and the `x` stays unconsumed.  Falling
    // through to the decimal loop below produces exactly that.
    if src.byte_at(i) == b'0' && (src.byte_at(i.wrapping_add(1)) | 0x20) == b'x' {
        let after_x = i.wrapping_add(2);
        let first = if src.byte_at(after_x) == b'.' {
            after_x.wrapping_add(1)
        } else {
            after_x
        };
        if src.byte_at(first).is_ascii_hexdigit() {
            return (
                FloatToken::Number,
                negative,
                scan_hex_body(src, acc, after_x),
            );
        }
    }

    // Digits, collected exactly: no floating-point arithmetic happens until
    // the caller's single correctly-rounded conversion.
    let mut has_digits = false;

    while src.byte_at(i).is_ascii_digit() {
        acc.push_integer(src.byte_at(i));
        has_digits = true;
        i = i.wrapping_add(1);
    }

    if src.byte_at(i) == b'.' {
        i = i.wrapping_add(1);
        while src.byte_at(i).is_ascii_digit() {
            acc.push_fraction(src.byte_at(i));
            has_digits = true;
            i = i.wrapping_add(1);
        }
    }

    if !has_digits {
        // No conversion performed: the caller leaves `*endptr` at the start.
        return (FloatToken::None, negative, 0);
    }

    (
        FloatToken::Number,
        negative,
        scan_exponent(src, acc, i, b'e'),
    )
}

/// Scan the body of a hex float after its `0x`, and return where it ended.
///
/// The caller has already checked that a hex digit follows, so there is no
/// failure case: a hex float always has a subject sequence once the prefix is
/// established.
fn scan_hex_body<S: ByteSource + ?Sized>(
    src: &S,
    acc: &mut DigitCollector,
    after_x: usize,
) -> usize {
    acc.set_hex();
    let mut i = after_x;

    while src.byte_at(i).is_ascii_hexdigit() {
        acc.push_integer(src.byte_at(i));
        i = i.wrapping_add(1);
    }
    if src.byte_at(i) == b'.' {
        i = i.wrapping_add(1);
        while src.byte_at(i).is_ascii_hexdigit() {
            acc.push_fraction(src.byte_at(i));
            i = i.wrapping_add(1);
        }
    }

    scan_exponent(src, acc, i, b'p')
}

/// Scan an optional `[marker][sign]digits` exponent and apply it to `acc`.
///
/// `marker` is the lowercase form; both cases are accepted.  A marker with no
/// digits after it is not part of the subject sequence at all — `"1e"`
/// converts as `1` with the `e` left unconsumed — so the index rolls back.
/// Returns where the scan ended.
#[allow(clippy::arithmetic_side_effects)]
fn scan_exponent<S: ByteSource + ?Sized>(
    src: &S,
    acc: &mut DigitCollector,
    start: usize,
    marker: u8,
) -> usize {
    let mut i = start;
    if (src.byte_at(i) | 0x20) != marker {
        return i;
    }
    i = i.wrapping_add(1);

    let exp_neg = src.byte_at(i) == b'-';
    if exp_neg || src.byte_at(i) == b'+' {
        i = i.wrapping_add(1);
    }
    if !src.byte_at(i).is_ascii_digit() {
        return start;
    }

    let mut exp_val: i32 = 0;
    while src.byte_at(i).is_ascii_digit() {
        exp_val = exp_val
            .saturating_mul(10)
            .saturating_add(i32::from(src.byte_at(i).wrapping_sub(b'0')));
        i = i.wrapping_add(1);
    }
    acc.apply_exponent(if exp_neg {
        exp_val.saturating_neg()
    } else {
        exp_val
    });
    i
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
/// Returns the rounded magnitude and the C `ERANGE` condition; the caller
/// applies the sign. The rounding is in direction `dir`, for a value of sign
/// `negative` ([`round_to_binary`]).
///
/// The bignum comes from `big`, given the limbs the literal needs once the
/// magnitude cut-offs have passed it -- so an absurd exponent never asks for
/// an absurd block. `None` if `big` has none to give.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_arguments)]
fn decimal_to_binary<S: AsRef<[u64]> + AsMut<[u64]>>(
    digits: &[u8],
    exp10: i32,
    truncated: bool,
    fmt: &Format,
    dir: Rounding,
    negative: bool,
    big: impl FnOnce(usize) -> Option<Big<S>>,
) -> Option<(Rounded, bool)> {
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
        return Some((Rounded::Finite { field: 0, m: 0 }, false));
    }

    // Position of the decimal point: the value lies in `[10^(mag-1), 10^mag)`.
    // `DBL_MAX` is just under `10^309` and the smallest subnormal is just over
    // `10^-324` -- `LDBL_MAX` under `10^4933`, the least subnormal `long
    // double` over `10^-4952` -- so these cut-offs are decided by magnitude
    // alone and keep the big-integer work bounded.
    let mag = exp10.saturating_add(i32::try_from(digits.len()).unwrap_or(i32::MAX));
    if mag > fmt.mag_max {
        return Some((Rounded::overflow(dir, negative), true));
    }
    if mag < fmt.mag_min {
        return Some((Rounded::underflow(dir, negative), true));
    }

    // `L = ceil(Q * log2 5)` plus the format's significand and eleven bits
    // for guard and round: `2321929/10^6` is above `log2 5`, so the ceiling
    // is never short and the quotient always keeps what the rounding needs.
    let q = exp10.min(0).unsigned_abs();
    let scaled = (u64::from(q) * 2_321_929).div_ceil(1_000_000);
    let l = u32::try_from(scaled)
        .unwrap_or(u32::MAX)
        .saturating_add(fmt.mant_bits + 11);
    // The bignum's size: the digits' integer (`log2 10 < 3.3220`), times
    // `10^exp10` or shifted by `L`, and a limb to spare.
    let digit_bits = (digits.len() as u64 * 33_220).div_ceil(10_000) + 1;
    let scale_bits = if exp10 >= 0 {
        (u64::from(exp10.unsigned_abs()) * 33_220).div_ceil(10_000)
    } else {
        u64::from(l)
    };
    let limbs = usize::try_from((digit_bits + scale_bits) / 64 + 2).unwrap_or(usize::MAX);
    let mut b = big(limbs)?;

    // The exact integer formed by the digits, absorbed 19 at a time because
    // `10^19` is the largest power of ten that fits in a `u64`.
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

    Some(round_to_binary(&b, e, sticky, fmt, dir, negative))
}

/// Convert `digits * 10^exp10` to the nearest `f64`, ties to even.
///
/// See [`decimal_to_binary`]; `truncated` is the caller's sticky bit and the
/// second result is the C `ERANGE` condition.
#[cfg(test)]
pub(crate) fn decimal_to_f64(digits: &[u8], exp10: i32, truncated: bool) -> (f64, bool) {
    let (r, out_of_range) = decimal_to_binary(
        digits,
        exp10,
        truncated,
        &F64_FORMAT,
        Rounding::Nearest,
        false,
        |_| Some(Big::<[u64; PARSE_LIMBS]>::from_u64(0)),
    )
    .unwrap_or((Rounded::Finite { field: 0, m: 0 }, false));
    (f64::from_bits(r.pack_ieee(&F64_FORMAT)), out_of_range)
}

/// Convert `digits * 10^exp10` to the nearest `f32`, ties to even.
///
/// Rounding to `f64` first and narrowing afterwards would round twice, and
/// two roundings are not one: a value a hair above an `f32` midpoint can land
/// exactly *on* that midpoint in `f64`, after which ties-to-even sends it the
/// wrong way.  `strtof("1.000000059604644830901776231257827021181583404541015625")`
/// is such a value — it must give `1.00000012`, but via `f64` it gives `1.0`.
/// So `f32` is rounded straight from the exact decimal expansion.
#[cfg(test)]
pub(crate) fn decimal_to_f32(digits: &[u8], exp10: i32, truncated: bool) -> (f32, bool) {
    let (r, out_of_range) = decimal_to_binary(
        digits,
        exp10,
        truncated,
        &F32_FORMAT,
        Rounding::Nearest,
        false,
        |_| Some(Big::<[u64; PARSE_LIMBS]>::from_u64(0)),
    )
    .unwrap_or((Rounded::Finite { field: 0, m: 0 }, false));
    let bits = r.pack_ieee(&F32_FORMAT);
    (
        f32::from_bits(u32::try_from(bits).unwrap_or(0)),
        out_of_range,
    )
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
    /// A decimal literal whose leading digit sits above `10^(mag_max - 1)`
    /// overflows whatever its digits, and one below `10^(mag_min - 1)`
    /// underflows: a cut-off decided by magnitude alone, which keeps the
    /// bignum work bounded. `float` shares `double`'s; the rounding catches
    /// anything between that the narrower format cannot hold.
    mag_max: i32,
    mag_min: i32,
}

impl Format {
    /// The bit pattern of positive infinity (an IEEE format's).
    fn infinity(&self) -> u64 {
        self.inf_field << self.mant_bits.saturating_sub(1)
    }
}

/// A value rounded into a format, before its encoding: the biased exponent
/// field -- 0 for zero and the subnormals -- and the significand, whose top
/// bit (`mant_bits - 1`) is set for a normal number and clear for a
/// subnormal; or an overflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rounded {
    Finite {
        field: u64,
        m: u64,
    },
    /// Past the largest finite value: infinity (`to_infinity`) or the
    /// largest finite value, by the rounding direction.
    Overflow {
        to_infinity: bool,
    },
}

impl Rounded {
    /// An overflow in direction `dir` for a value of sign `negative`:
    /// glibc's `MAX_VALUE * MAX_VALUE`, which is infinity when rounding to
    /// nearest or away from zero, the largest finite value toward it.
    fn overflow(dir: Rounding, negative: bool) -> Self {
        Self::Overflow {
            to_infinity: dir == Rounding::Nearest || dir.away(negative),
        }
    }

    /// A value too small for the rounding to reach: glibc's `MIN_VALUE *
    /// MIN_VALUE` -- zero, or the least subnormal when rounding away from
    /// zero.
    fn underflow(dir: Rounding, negative: bool) -> Self {
        Self::Finite {
            field: 0,
            m: u64::from(dir != Rounding::Nearest && dir.away(negative)),
        }
    }

    /// The IEEE encoding in `fmt`, whose leading significand bit is implicit.
    fn pack_ieee(self, fmt: &Format) -> u64 {
        // The stored fraction: the significand less its leading bit, 23 or
        // 52 bits.
        let frac_bits = fmt.mant_bits.saturating_sub(1);
        match self {
            Self::Finite { field, m } => {
                let frac_mask = (1u64 << frac_bits).wrapping_sub(1);
                (field << frac_bits) | (m & frac_mask)
            }
            Self::Overflow { to_infinity: true } => fmt.infinity(),
            // The largest finite value is the pattern just below infinity's.
            Self::Overflow { to_infinity: false } => fmt.infinity().saturating_sub(1),
        }
    }

    /// x87's encoding, whose integer bit is explicit: the exponent field and
    /// the significand as they are stored.
    fn pack_x87(self) -> (u16, u64) {
        match self {
            Self::Finite { field, m } => (u16::try_from(field).unwrap_or(0x7FFF), m),
            Self::Overflow { to_infinity: true } => (0x7FFF, 1 << 63),
            Self::Overflow { to_infinity: false } => (0x7FFE, u64::MAX),
        }
    }
}

/// IEEE-754 binary64: 53-bit significand, least subnormal `2^-1074`.
const F64_FORMAT: Format = Format {
    mant_bits: 53,
    min_exp: -1074,
    bias: 1075,
    inf_field: 0x7ff,
    mag_max: 310,
    mag_min: -330,
};

/// IEEE-754 binary32: 24-bit significand, least subnormal `2^-149`.
const F32_FORMAT: Format = Format {
    mant_bits: 24,
    min_exp: -149,
    bias: 150,
    inf_field: 0xff,
    mag_max: 310,
    mag_min: -330,
};

/// x87's 80-bit extended format, a `long double`: a 64-bit significand whose
/// integer bit is stored, least subnormal `2^-16445`.
const LD80_FORMAT: Format = Format {
    mant_bits: 64,
    min_exp: -16445,
    bias: 16446,
    inf_field: 0x7fff,
    mag_max: 4934,
    mag_min: -4972,
};

/// Round the exact value `b * 2^e` into `fmt` in direction `dir`, for a
/// value of sign `negative` (`b * 2^e` is its magnitude; a directed mode
/// rounds a magnitude by its sign).
///
/// `sticky_in` says the true value is strictly greater than `b * 2^e`, by less
/// than one unit in `b`'s last place.  Returns `(bits, out_of_range)`, the
/// second being glibc's `ERANGE`: an overflow -- to infinity, or to the
/// largest finite value when rounding toward zero -- or a result that is
/// both *tiny* and *inexact*. Tiny is x86's "tininess after rounding": still
/// below the least normal number when rounded to the format's full
/// precision, as if the exponent had no floor. So an exact subnormal is no
/// error, and a value just under the least normal number that rounds up to
/// it is no error either -- unless it was tiny all the same.
#[allow(clippy::arithmetic_side_effects)]
fn round_to_binary<S: AsRef<[u64]> + AsMut<[u64]>>(
    b: &Big<S>,
    e: i32,
    sticky_in: bool,
    fmt: &Format,
    dir: Rounding,
    negative: bool,
) -> (Rounded, bool) {
    let zero = Rounded::Finite { field: 0, m: 0 };
    let n = b.bits();
    if n == 0 {
        return (zero, false);
    }
    let prec = fmt.mant_bits as usize;
    let implicit = 1u64 << (fmt.mant_bits - 1);

    // `b` with its lowest `drop` bits cut off and rounded in `dir`: the kept
    // bits, possibly carried to `prec + 1` of them -- 65, for a `long
    // double`, hence the `u128` -- and whether anything nonzero was cut.
    let round_at = |drop: usize| -> (u128, bool) {
        let m = b.window(drop);
        let guard = drop > 0 && b.bit(drop.saturating_sub(1));
        let rest = sticky_in || (drop > 1 && b.any_bits_below(drop.saturating_sub(1)));
        let cut = Cut::of_bits(guard, rest);
        let up = dir.rounds_up(negative, m & 1 == 1, cut);
        (u128::from(m) + u128::from(up), cut != Cut::Zero)
    };

    // Keep the top `mant_bits` bits.
    let drop_full = n.saturating_sub(prec);
    let exp_full = e.saturating_add(i32::try_from(drop_full).unwrap_or(i32::MAX));

    // Tininess after rounding: that rounding, with no exponent floor, still
    // below the least normal number -- whose top bit is `prec - 1 + min_exp`.
    let tiny = {
        let (m, _) = round_at(drop_full);
        // The top bit's place: -1 for a significand rounded to 0.
        let top = m
            .checked_ilog2()
            .map_or(-1, |top| i32::try_from(top).unwrap_or(i32::MAX));
        top.saturating_add(exp_full) < i32::try_from(prec).unwrap_or(0) - 1 + fmt.min_exp
    };

    // Below the subnormal floor, cut further, so the exponent is exactly
    // `min_exp`; everything cut is summarised by the guard and sticky bits.
    let mut drop = drop_full;
    let mut exp = exp_full;
    if exp < fmt.min_exp {
        let extra = i64::from(fmt.min_exp).saturating_sub(i64::from(exp));
        drop = drop.saturating_add(usize::try_from(extra).unwrap_or(usize::MAX));
        exp = fmt.min_exp;
    }
    let (mut wide, inexact) = round_at(drop);
    if wide == u128::from(implicit) << 1 {
        wide >>= 1;
        exp = exp.saturating_add(1);
    }
    // At most `prec <= 64` bits now.
    let mut m = u64::try_from(wide).unwrap_or(u64::MAX);
    let underflow = tiny && inexact;

    if m == 0 {
        return (zero, underflow);
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
        // stored significand.
        return (Rounded::Finite { field: 0, m }, underflow);
    }

    let biased = exp.saturating_add(fmt.bias);
    if biased >= i32::try_from(fmt.inf_field).unwrap_or(i32::MAX) {
        return (Rounded::overflow(dir, negative), true);
    }
    let field = u64::try_from(biased).unwrap_or(0);
    (Rounded::Finite { field, m }, underflow)
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
        assert_eq!(
            conv("9007199254740992", 0),
            (9.007_199_254_740_992e15, false)
        );
        assert_eq!(
            conv("90071992547409920000", -4),
            (9.007_199_254_740_992e15, false)
        );
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
        assert_eq!(
            conv("2470328229206232720882843964341106861826", -363).0,
            f64::from_bits(1)
        );
        // Exactly half rounds down, because zero is the even side.
        assert_eq!(
            conv("2470328229206232720882843964341106861825", -363).0,
            0.0
        );
    }

    #[test]
    fn conversion_reports_the_erange_condition() {
        assert_eq!(conv("1", 400), (f64::INFINITY, true));
        assert_eq!(conv("1", -400), (0.0, true));
        // Normal results are never out of range; subnormals always are.
        assert!(!conv("1", 0).1);
        assert!(conv("1", -320).1);
        assert!(
            !conv("22250738585072014", -324).1,
            "least normal is in range"
        );
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
            let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
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
