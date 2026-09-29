//! The `long double` Bessel functions -- `j0l`, `j1l`, `jnl`, `y0l`, `y1l`
//! and `ynl`, GNU extensions `<math.h>` declares beside the double and float
//! ones (`math.rs`) -- written from the mathematics: musl has none, FreeBSD's
//! msun no 80-bit ones, and glibc's are LGPL (open-questions.md D-Q6).
//! glibc is the oracle for what C leaves open -- the special values, `errno`
//! and the exception flags (`besl_glibc.txt`) -- and mpmath for the values,
//! which glibc does not round correctly (`besl_oracle.txt`).
//!
//! # How
//!
//! Everything is computed in double-long-double arithmetic ([`DD`], a pair
//! of long doubles, about 128 bits) and rounded once at the end, in the
//! caller's direction, which makes nearly every answer the correctly rounded
//! one. For J0, J1, Y0 and Y1 at `x > 0`:
//!
//! - **the leading terms:** `1` and `x/2` below `x = 2^-33`, less a sticky
//!   bit for the direction's sake (the correction is below 2^-68 of a unit
//!   in the last place); `(2/pi)(ln(x/2) + gamma)` and `-2/(pi x)` below
//!   2^-70, where the next term is below 2^-134 of them.
//! - **`x <= 48`: Miller's backward recurrence** `f(k-1) = (2k/x) f(k) -
//!   f(k+1)` from an order `N` where `J_N(x)` is below 2^-140, normalised by
//!   `J0 + 2 (J2 + J4 + ...) = 1`; and for Y0 and Y1 the Neumann series over
//!   the same values (Abramowitz and Stegun 9.1.88), `Y0 = (2/pi)((ln(x/2) +
//!   gamma) J0 - 2 sum (-1)^k J(2k)/k)` and its order-1 kin.
//! - **within 2^-32 of a zero below 48: the Taylor series about the zero,**
//!   the zero to 192 bits, the coefficients from Bessel's equation
//!   (`posix/tools/oracle/besl_tables.py`), so the answer keeps its relative
//!   precision however close to the zero `x` comes -- where the recurrence's
//!   absolute error, about 2^-124, would be all of it.
//! - **`x > 48`: Hankel's expansion in phase and amplitude,** `J = M cos
//!   theta`, `Y = M sin theta`, `theta = x - (2 nu + 1) pi/4 + atan(Q/P)`,
//!   `M = sqrt(2/(pi x)) sqrt(P^2 + Q^2)`, its terms summed to below 2^-142
//!   of the first. The phase is reduced by pi/4 exactly -- by an exact sum
//!   of `x`, `K` times each of eight 24-bit pieces of pi/4 and the arctangent
//!   (below 2^39), by Payne and Hanek's reduction to 113 bits above -- so a
//!   zero's cancellation happens once, in an exact sum, and the answer keeps
//!   its relative precision near every zero there too.
//!
//! `jnl` and `ynl`: `Y_n` by the forward recurrence from Y0 and Y1, which is
//! stable for it everywhere; `J_n` by its power series where `x^2 < 4(n +
//! 1)`, the forward recurrence from J0 and J1 where `n <= x` past 48, and
//! Miller's recurrence to `n` elsewhere. An answer the Debye estimate puts
//! far past the range is answered as an overflow or underflow without the
//! recurrence. Near a zero of `J_n` or `Y_n` for `n >= 2` the answer is good
//! to about 2^-124 of the function's amplitude rather than of its value.
//!
//! # What C leaves open: glibc's answers
//!
//! The special values, `errno` and the flags are glibc's, as `math.rs`'s
//! double functions give them: the `y` functions are a domain error below 0
//! (a quiet NaN, invalid raised, `EDOM`) and a pole at 0 (`-inf`,
//! divide-by-zero, `ERANGE`); `j1l` and odd orders of `jnl` keep an
//! infinite or zero argument's sign, `J(-n, x) = J(n, -x)` and `Y(-n, x) =
//! (-1)^n Y(n, x)`, but `ynl` of an order past 1 at `+inf` is `+0`; a
//! result that underflows to zero, or overflows, from a finite argument is
//! `ERANGE`, decided on the round-to-nearest result and answered as the
//! direction rounds it ([`crate::math::ranged`]). The flags are the final
//! rounding's alone: the computation runs with the flags held
//! ([`crate::fenv::quietly_in_nearest_x87`]), so an underflow in a term far
//! below the sum is not reported.
//!
//! glibc's own answers depart from these in ways the tests allow. Its
//! values are not correctly rounded: two in three of `besl_oracle.txt`'s
//! away from the zeros are off, by up to 6 units in the last place, and next
//! to a zero every one is, by a median of 3 x 10^13 units and up to 4 x
//! 10^18 -- no correct bits (`posix/tools/oracle/besl_tables.py`'s points
//! within 10^-12 of a zero, 3,034 of them). Rounding in another direction it
//! answers as to nearest (`j0l` of a tiny `x` is 1 rounding downward, where
//! the exact value is below 1). At an encoding the x87 refuses its `jnl` and
//! `ynl` answer what their bit tests make of it -- 0 for a pseudo-infinity,
//! `ERANGE` for an unnormal -- where every function here answers the unit's
//! NaN with invalid, as `mathl.rs`'s do. `jnl` of a huge order takes it
//! seconds and can answer `-0` for a positive underflow. And `errno` in the
//! directed modes follows this library's one rule (design-decisions §1139)
//! rather than glibc's, which sets `ERANGE` for `j1l`'s underflow only where
//! the result is zero.

// Floating-point arithmetic on the x87, whose overflow is an infinity and a
// flag, never a panic; and orders and exponents whose ranges the algorithms
// bound (an order below 2^31, an exponent within +-2^16).
#![allow(clippy::arithmetic_side_effects)]

use crate::errno;
use crate::fenv::{FE_INEXACT, FE_UNDERFLOW};
use crate::math::{overflow_only, ranged, underflow_only};
use crate::x87::LongDouble as L;

const ZERO: L = L::POS_ZERO;
const ONE: L = L::ONE;
const HALF: L = L::from_bits(0x3FFE, 1 << 63);
const TWO: L = L::from_bits(0x4000, 1 << 63);
/// The largest finite long double.
const LDBL_MAX: L = L::from_bits(0x7FFE, u64::MAX);

/// `2^k`, for `k` a normal exponent (-16382 to 16383).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
const fn pow2(k: i32) -> L {
    L::from_bits((0x3FFF + k) as u16, 1 << 63)
}

/// Where Miller's recurrence gives way to Hankel's expansion, whose
/// smallest term there is below 2^-142 of its first.
const XH: L = L::from_bits(0x4004, 0xC000_0000_0000_0000); // 48
/// The half-width of the window about each zero below [`XH`] where its
/// Taylor series answers.
const W: L = pow2(-32);
/// Below this the leading terms alone answer Y0 and Y1: the next is below
/// 2^-134 of them.
const X_TINY: L = pow2(-70);
/// Below this J0 and J1 are their leading terms, `1` and `x/2`, less a
/// correction below 2^-68 of a unit in their last place: which way the
/// direction rounds is all it decides. (The recurrence cannot tell it: it
/// holds J0 to about 2^-126, where the correction is `x^2/4`.)
const X_SMALL_J: L = pow2(-33);
/// Below this the phase is reduced with [`PI_OVER_4_PIECES`], the multiple
/// of pi/4 then an integer below 2^40; above it, by Payne and Hanek.
const X_REDUCE: L = pow2(39);
/// Where Hankel's terms stop: below the expansion's accuracy anywhere past
/// [`XH`].
const HANKEL_EPS: L = pow2(-145);
/// Where a convergent series stops, against its sum.
const EPS: L = pow2(-140);
/// Values of a recurrence are scaled down by `2^-RESCALE` past `2^RESCALE`,
/// so that neither they nor a product of one with `2k/x` overflows.
const RESCALE: i32 = 4000;

/// 1/ln 2 and ln 2, for the estimates, which run in double precision.
const LN2_F64: f64 = core::f64::consts::LN_2;

// ---------------------------------------------------------------------------
// Double-long-double arithmetic
// ---------------------------------------------------------------------------

/// `a + b` as `(s, e)`: `s` the sum rounded, `s + e` exactly `a + b`
/// (Knuth's two-sum, `mathl.rs`'s).
fn two_sum(a: L, b: L) -> (L, L) {
    crate::mathl::two_sum(a, b)
}

/// [`two_sum`] when `|a| >= |b|` or `a` is zero (Dekker's fast two-sum).
fn fast_two_sum(a: L, b: L) -> (L, L) {
    let s = a + b;
    (s, b - (s - a))
}

/// `a b` as `(p, e)`, `p + e` exactly `a b` (Dekker's product, `mathl.rs`'s).
fn two_prod(a: L, b: L) -> (L, L) {
    crate::mathl::two_prod(a, b)
}

/// An integer as a long double, exactly (every `i64` is one).
fn int(n: i64) -> L {
    L::from_i64(n)
}

/// A double-long-double: `hi + lo`, `|lo| <= ulp(hi) / 2`, about 128 bits.
/// Every operation holds to about 2^-126 of its result, rounding to
/// nearest.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct DD {
    hi: L,
    lo: L,
}

impl DD {
    const ZERO: Self = Self { hi: ZERO, lo: ZERO };

    const fn l(x: L) -> Self {
        Self { hi: x, lo: ZERO }
    }

    fn norm((hi, lo): (L, L)) -> Self {
        let (hi, lo) = fast_two_sum(hi, lo);
        Self { hi, lo }
    }

    /// The sum, as [`DD::add`] computes it in one block (below).
    #[cfg(test)]
    fn add_ref(self, o: Self) -> Self {
        let (s, e) = two_sum(self.hi, o.hi);
        let (t, f) = two_sum(self.lo, o.lo);
        let (s, e) = fast_two_sum(s, e + t);
        Self::norm((s, e + f))
    }

    fn add_l(self, o: L) -> Self {
        let (s, e) = two_sum(self.hi, o);
        Self::norm((s, e + self.lo))
    }

    fn neg(self) -> Self {
        Self {
            hi: -self.hi,
            lo: -self.lo,
        }
    }

    fn sub(self, o: Self) -> Self {
        self.add(o.neg())
    }

    /// The product, as [`DD::mul`] computes it in one block (below).
    #[cfg(test)]
    fn mul_ref(self, o: Self) -> Self {
        let (p, e) = two_prod(self.hi, o.hi);
        Self::norm((p, e + (self.hi * o.lo + self.lo * o.hi)))
    }

    /// The product with a long double, as [`DD::mul_l`] computes it in one
    /// block (below).
    #[cfg(test)]
    fn mul_l_ref(self, o: L) -> Self {
        let (p, e) = two_prod(self.hi, o);
        Self::norm((p, e + self.lo * o))
    }

    fn div(self, o: Self) -> Self {
        let q1 = self.hi / o.hi;
        let r = self.sub(o.mul_l(q1));
        let q2 = r.hi / o.hi;
        let r = r.sub(o.mul_l(q2));
        let q3 = r.hi / o.hi;
        Self::norm((q1, q2)).add_l(q3)
    }

    fn div_l(self, o: L) -> Self {
        self.div(Self::l(o))
    }

    /// The square root, one Newton step in [`DD`] from the x87's.
    fn sqrt(self) -> Self {
        let s = self.hi.sqrt();
        let (p, e) = two_prod(s, s);
        let r = self.sub(Self { hi: p, lo: e });
        Self::norm((s, r.hi / (s + s)))
    }

    /// Times `2^k`: exact while the result stays normal.
    fn scale(self, k: i32) -> Self {
        Self {
            hi: self.hi.scalbn(k),
            lo: self.lo.scalbn(k),
        }
    }

    fn mag(self) -> L {
        self.hi.abs()
    }

    /// Twice the pair: exact, and two multiplications where
    /// [`DD::mul_l`] would be a whole Dekker product.
    fn twice(self) -> Self {
        Self {
            hi: self.hi * TWO,
            lo: self.lo * TWO,
        }
    }
}

// ---------------------------------------------------------------------------
// The pair operations as single blocks on the x87 register stack
//
// Each `LongDouble` operation in `ld80.rs` is its own assembly block: both
// operands loaded from memory, the result stored back, and the next
// operation loading it again, so a chain of them waits on an 80-bit store
// and load at every step. A pair operation is some twenty such steps. Here
// each of the three the algorithms spend their time in -- the pair's sum,
// its product, and its product with a long double -- is one block that
// loads its operands, keeps every intermediate value on the register stack
// and stores the two parts of the result. They perform the same operations
// in the same order as `DD::add_ref`, `DD::mul_ref` and `DD::mul_l_ref`
// below, on registers that hold exactly what a long double in memory holds
// (the register format is the 80-bit format), so they compute bit for bit
// the same thing; the tests hold them to it.
//
// The sequences, as macros so each is written once. A comment gives the
// stack after each instruction, top first. Only the `st, st(i)` and `st(i),
// st` forms are used whose meaning is not in doubt: `fsub st, st(i)` is st0
// - st(i), `fsubr st, st(i)` is st(i) - st0, and `fsubp st(1), st` is st1 -
// st0, popped (as `ld80.rs`'s `Sub` relies on).
// ---------------------------------------------------------------------------

/// Knuth's two-sum, `[x y ..] -> [s e ..]`: `s = x + y`, `e` its error,
/// `bb = s - x`, `e = (x - (s - bb)) + (y - bb)`. Three registers more.
macro_rules! two_sum_seq {
    () => {
        concat!(
            "fld st(0)\n",       // x x y
            "fadd st, st(2)\n",  // s x y
            "fld st(0)\n",       // s s x y
            "fsub st, st(2)\n",  // bb s x y
            "fld st(1)\n",       // s bb s x y
            "fsub st, st(1)\n",  // s-bb bb s x y
            "fsubr st, st(3)\n", // x-(s-bb) bb s x y
            "fxch st(1)\n",      // bb u s x y
            "fsubr st, st(4)\n", // y-bb u s x y
            "faddp st(1), st\n", // e s x y
            "fstp st(3)\n",      // s x e
            "fstp st(1)\n",      // s e
        )
    };
}

/// Dekker's fast two-sum, `[a b ..] -> [s e ..]`: `s = a + b`, `e = b - (s
/// - a)`, exact when `|a| >= |b|`. Two registers more.
macro_rules! fast_two_sum_seq {
    () => {
        concat!(
            "fld st(0)\n",       // a a b
            "fadd st, st(2)\n",  // s a b
            "fld st(0)\n",       // s s a b
            "fsub st, st(2)\n",  // s-a s a b
            "fsubr st, st(3)\n", // e=b-(s-a) s a b
            "fstp st(3)\n",      // s a e
            "fstp st(1)\n",      // s e
        )
    };
}

/// Veltkamp's split, `[a ..] -> [lo hi ..]`: `c = a (2^32 + 1)`, `hi = c -
/// (c - a)`, `lo = a - hi`; `{c}` is the constant's address. Two registers
/// more.
macro_rules! split_seq {
    () => {
        concat!(
            "fld tbyte ptr [{c}]\n", // C a
            "fmul st, st(1)\n",      // c a
            "fld st(0)\n",           // c c a
            "fsub st, st(2)\n",      // c-a c a
            "fsubp st(1), st\n",     // hi a
            "fxch st(1)\n",          // a hi
            "fsub st, st(1)\n",      // lo hi
        )
    };
}

/// Dekker's product of the long doubles at `{a}` and `{b}`, `[..] -> [p e
/// ..]`: `p = a b`, `e = ((ahi bhi - p) + ahi blo + alo bhi) + alo blo`.
/// Seven registers at most.
macro_rules! two_prod_seq {
    () => {
        concat!(
            "fld tbyte ptr [{a}]\n",
            split_seq!(), // alo ahi
            "fld tbyte ptr [{b}]\n",
            split_seq!(),            // blo bhi alo ahi
            "fld tbyte ptr [{a}]\n", // a blo bhi alo ahi
            "fld tbyte ptr [{b}]\n", // b a blo bhi alo ahi
            "fmulp st(1), st\n",     // p blo bhi alo ahi
            "fld st(4)\n",           // ahi p blo bhi alo ahi
            "fmul st, st(3)\n",      // ahi*bhi p ..
            "fsub st, st(1)\n",      // t p blo bhi alo ahi
            "fld st(5)\n",           // ahi t p blo bhi alo ahi
            "fmul st, st(3)\n",      // ahi*blo t ..
            "faddp st(1), st\n",     // t p blo bhi alo ahi
            "fld st(4)\n",           // alo t p blo bhi alo ahi
            "fmul st, st(4)\n",      // alo*bhi t ..
            "faddp st(1), st\n",     // t p blo bhi alo ahi
            "fld st(4)\n",           // alo t p blo bhi alo ahi
            "fmul st, st(3)\n",      // alo*blo t ..
            "faddp st(1), st\n",     // e p blo bhi alo ahi
            "fstp st(5)\n",          // p blo bhi alo e
            "fstp st(3)\n",          // blo bhi p e
            "fstp st(0)\n",          // bhi p e
            "fstp st(0)\n",          // p e
        )
    };
}

/// `2^32 + 1`, Veltkamp's splitting constant, where the blocks can load it.
static SPLITTER: L = L::from_bits(0x401F, 0x8000_0000_8000_0000);

impl DD {
    /// The sum of two pairs: [`DD::add_ref`] in one block.
    fn add(self, o: Self) -> Self {
        let mut r = Self::ZERO;
        // SAFETY: loads the four long doubles of `self` and `o` (each pair
        // `repr(C)`, `hi` at offset 0 and `lo` at 16) and stores the two of
        // `r`, all locals of this frame. The sequence pushes at most seven
        // values and ends with the stack empty, as the eight clobbers
        // declare it may leave every register changed.
        unsafe {
            core::arch::asm!(
                "fld tbyte ptr [{o} + 16]",
                "fld tbyte ptr [{s} + 16]", // slo olo
                two_sum_seq!(),             // t f
                "fld tbyte ptr [{o}]",
                "fld tbyte ptr [{s}]", // shi ohi t f
                two_sum_seq!(),        // s e t f
                "fxch st(1)",          // e s t f
                "fadd st, st(2)",      // e+t s t f
                "fxch st(1)",          // s e' t f
                fast_two_sum_seq!(),   // s e t f
                "fxch st(1)",          // e s t f
                "fadd st, st(3)",      // e+f s t f
                "fxch st(1)",          // s e' t f
                fast_two_sum_seq!(),   // s e t f
                "fstp tbyte ptr [{r}]",
                "fstp tbyte ptr [{r} + 16]",
                "fstp st(0)",
                "fstp st(0)",
                s = in(reg) &raw const self,
                o = in(reg) &raw const o,
                r = in(reg) &raw mut r,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        r
    }

    /// The product of two pairs: [`DD::mul_ref`] in one block.
    fn mul(self, o: Self) -> Self {
        let mut r = Self::ZERO;
        // SAFETY: as in `add`: loads of `self`, `o` and the splitting
        // constant, stores of `r`; eight values at most (the cross term under
        // Dekker's seven), the stack empty at the end.
        unsafe {
            core::arch::asm!(
                "fld tbyte ptr [{a}]",
                "fld tbyte ptr [{b} + 16]",
                "fmulp st(1), st", // shi*olo
                "fld tbyte ptr [{a} + 16]",
                "fld tbyte ptr [{b}]",
                "fmulp st(1), st", // slo*ohi x1
                "faddp st(1), st", // x = x1 + x2
                two_prod_seq!(),   // p e x
                "fxch st(1)",      // e p x
                "fadd st, st(2)",  // e+x p x
                "fxch st(1)",      // p e' x
                fast_two_sum_seq!(), // s e x
                "fstp tbyte ptr [{r}]",
                "fstp tbyte ptr [{r} + 16]",
                "fstp st(0)",
                a = in(reg) &raw const self,
                b = in(reg) &raw const o,
                c = in(reg) &raw const SPLITTER,
                r = in(reg) &raw mut r,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        r
    }

    /// A pair times a long double: [`DD::mul_l_ref`] in one block.
    fn mul_l(self, o: L) -> Self {
        let mut r = Self::ZERO;
        // SAFETY: as in `mul`, `o` the long double local.
        unsafe {
            core::arch::asm!(
                "fld tbyte ptr [{a} + 16]",
                "fld tbyte ptr [{b}]",
                "fmulp st(1), st", // x = slo*o
                two_prod_seq!(),   // p e x
                "fxch st(1)",      // e p x
                "fadd st, st(2)",  // e+x p x
                "fxch st(1)",      // p e' x
                fast_two_sum_seq!(), // s e x
                "fstp tbyte ptr [{r}]",
                "fstp tbyte ptr [{r} + 16]",
                "fstp st(0)",
                a = in(reg) &raw const self,
                b = in(reg) &raw const o,
                c = in(reg) &raw const SPLITTER,
                r = in(reg) &raw mut r,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        r
    }
}

/// `1/k` in [`DD`]: from the table, or divided out past it.
fn recip(k: u32) -> DD {
    usize::try_from(k)
        .ok()
        .and_then(|k| k.checked_sub(1))
        .and_then(|i| RECIPROCALS.get(i).copied())
        .unwrap_or_else(|| DD::l(ONE).div_l(int(i64::from(k))))
}

/// `1/k!` in [`DD`], `k <= 37`.
fn inv_factorial(k: usize) -> DD {
    INV_FACTORIALS.get(k).copied().unwrap_or(DD::ZERO)
}

/// `floor(log2 |x|)` for a finite nonzero `x`, a subnormal's included.
fn exponent(x: L) -> i32 {
    let e = i32::from(x.biased_exponent());
    if e == 0 {
        // significand 2^(-16382 - 63), the integer bit clear.
        -16382 - i32::try_from(x.significand.leading_zeros()).unwrap_or(0)
    } else {
        e - 0x3FFF
    }
}

/// `ln x` for a finite `x > 0`, in [`DD`]: `x = 2^e m`, `m` in `[sqrt(1/2),
/// sqrt(2))`, `ln m = 2 atanh(s)`, `s = (m - 1)/(m + 1)`, `|s| <= 0.1716`,
/// its series to below 2^-137.
fn ln_dd(x: L) -> DD {
    let mut e = exponent(x);
    let mut m = x.scalbn(-e);
    // sqrt(2), rounded: which side of it m falls matters to no bit of the answer.
    if m > L::from_bits(0x3FFF, 0xB504_F333_F9DE_6484) {
        m = m * HALF;
        e += 1;
    }
    // m - 1 is exact (Sterbenz); m + 1 needs a 65th bit.
    let s = DD::l(m - ONE).div(DD::norm(two_sum(m, ONE)));
    let s2 = s.mul(s);
    // atanh(s)/s = sum s^(2k) / (2k + 1), k <= 27.
    let mut acc = recip(55);
    for k in (0..27u32).rev() {
        acc = acc.mul(s2).add(recip(2 * k + 1));
    }
    LN_2.mul_l(int(i64::from(e))).add(acc.mul(s).mul_l(TWO))
}

/// `x` as `xs 2^e`, `xs` in `[1, 2)`, for a finite nonzero `x`.
fn split(x: L) -> (L, i32) {
    let e = exponent(x);
    (x.abs().scalbn(-e), e)
}

/// `k/x` in [`DD`] for a finite `x > 0` of any size: `x` scaled to `[1, 2)`
/// first, as Dekker's product overflows past 2^16351. Subnormal, and so
/// short of its bits, only for `x` past 2^16382, where it is a negligible
/// term.
fn over_x(k: L, x: L) -> DD {
    let (xs, e) = split(x);
    DD::l(k).div_l(xs).scale(-e)
}

// ---------------------------------------------------------------------------
// Estimates, in double precision: where an answer lies against the range,
// and where Miller's recurrence must start
// ---------------------------------------------------------------------------

/// `ln x` for a finite `x > 0`, in double precision.
fn ln_f64(x: L) -> f64 {
    let e = exponent(x);
    f64::from(e) * LN2_F64 + libm::log(x.scalbn(-e).to_f64())
}

/// Debye's exponent `nu (alpha - tanh alpha)`, `cosh alpha = nu / x`, for
/// `nu > x` (0 otherwise): `J_nu(x)` is about `e` to its negative, `Y_nu(x)`
/// to it, over a square root. `ln_x` is `ln x`.
fn debye_g(nu: f64, ln_x: f64) -> f64 {
    if nu <= 0.0 {
        return 0.0;
    }
    let lr = libm::log(nu) - ln_x;
    if lr <= 0.0 {
        return 0.0;
    }
    if lr > 30.0 {
        // cosh alpha past 10^13: alpha = ln(2 nu/x), tanh alpha = 1.
        return nu * (lr + LN2_F64 - 1.0);
    }
    let alpha = libm::acosh(libm::exp(lr));
    nu * (alpha - libm::tanh(alpha))
}

/// `ln |J_n(x)|` (`n > x`) or `ln |Y_n(x)|` (`n > x`, `for_y`) by Debye's
/// leading term, to within a few units; 0 where `n <= x`, where neither
/// comes near the range's ends.
fn debye_ln(n: u32, ln_x: f64, for_y: bool) -> f64 {
    let nu = f64::from(n);
    let g = debye_g(nu, ln_x);
    if g == 0.0 {
        return 0.0;
    }
    let lr = libm::log(nu) - ln_x;
    let tanh_alpha = if lr > 30.0 {
        1.0
    } else {
        libm::tanh(libm::acosh(libm::exp(lr)))
    };
    let root = libm::log(core::f64::consts::PI * nu * tanh_alpha);
    if for_y {
        g - 0.5 * (root - LN2_F64)
    } else {
        -g - 0.5 * (root + LN2_F64)
    }
}

/// Miller's starting order for `J_n(x)` (and every lower order) to 2^-140:
/// the least `N` past `n` and `x` with `J_N(x)` below 2^-140, and with
/// `J_(N+1)(x) / Y_(N+1)(x)` below 2^-140 of `J_n(x) / Y_n(x)` -- the two
/// errors the recurrence's start makes, in the normalising sum and in
/// `J_n` itself (checked against mpmath's recurrence at 400 points).
fn start_index(n: u32, x: L) -> u32 {
    let ln_x = ln_f64(x);
    let target = 140.0 * LN2_F64;
    let gn = debye_g(f64::from(n), ln_x);
    let enough = |big: u32| {
        debye_g(f64::from(big), ln_x) >= target
            && 2.0 * debye_g(f64::from(big) + 1.0, ln_x) - 2.0 * gn >= target
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let floor_x = x.to_f64().min(4.0e9) as u32;
    let mut lo = n.max(floor_x).saturating_add(2);
    if enough(lo) {
        return lo;
    }
    // Double the step until enough, then halve back.
    let mut step = 8u32;
    let mut hi = lo.saturating_add(step);
    while !enough(hi) && hi < u32::MAX / 2 {
        lo = hi;
        step = step.saturating_mul(2);
        hi = hi.saturating_add(step);
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if enough(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

// ---------------------------------------------------------------------------
// J0, J1, Y0, Y1
// ---------------------------------------------------------------------------

/// A zero of J0, J1, Y0 or Y1 below [`XH`] and the function's Taylor
/// coefficients about it: `f(z + h) = c1 h + c2 h^2 + c3 h^3 + d4 h^4 +
/// d5 h^5` over `|h| <= W`.
struct Zero {
    /// The zero, as three long doubles (192 bits).
    z: [L; 3],
    /// `c1`, `c2`, `c3`.
    c: [DD; 3],
    /// `c4`, `c5`.
    d: [L; 2],
}

/// The function near one of its zeros below [`XH`], if `x` is within [`W`]
/// of one: its Taylor series in `h = x - z`, `h` exact to 2^-128 of itself.
fn near_zero(zeros: &[Zero], x: L) -> Option<DD> {
    for zero in zeros {
        let [z0, z1, z2] = zero.z;
        // Exact where it counts (Sterbenz): x within a factor 2 of z0.
        let d = x - z0;
        if d.abs() < W {
            let h = DD::norm(two_sum(d, -z1)).add_l(-z2);
            let [c1, c2, c3] = zero.c;
            let [c4, c5] = zero.d;
            let tail = (c5 * h.hi + c4) * h.hi;
            let acc = c3.add_l(tail).mul(h).add(c2).mul(h).add(c1);
            return Some(acc.mul(h));
        }
    }
    None
}

/// Miller's recurrence at `x` (`X_TINY <= x <= XH`): J0 and J1, and the
/// Neumann sums `s0 = sum (-1)^k J(2k)/k` and `s1 = sum (-1)^k (1/k +
/// 1/(k+1)) J(2k+1)`, `k >= 1`.
struct Low {
    j0: DD,
    j1: DD,
    s0: DD,
    s1: DD,
}

fn miller01(x: L) -> Low {
    let big_n = start_index(1, x);
    let two_over_x = DD::l(TWO).div_l(x);
    // f(k+1) and f(k), from f(N+1) = 0, f(N) = 1.
    let (mut next, mut cur) = (DD::ZERO, DD::l(ONE));
    let (mut norm, mut s0, mut s1) = (DD::ZERO, DD::ZERO, DD::ZERO);
    for k in (1..=big_n).rev() {
        if k % 2 == 0 {
            let j = k / 2;
            norm = norm.add(cur.twice());
            let t = cur.mul(recip(j));
            s0 = if j % 2 == 0 { s0.add(t) } else { s0.sub(t) };
        } else if k >= 3 {
            let j = (k - 1) / 2;
            let t = cur.mul(recip(j).add(recip(j + 1)));
            s1 = if j % 2 == 0 { s1.add(t) } else { s1.sub(t) };
        }
        let prev = two_over_x.mul_l(int(i64::from(k))).mul(cur).sub(next);
        next = cur;
        cur = prev;
    }
    // cur is f(0), next f(1).
    norm = norm.add(cur);
    let inv = DD::l(ONE).div(norm);
    Low {
        j0: cur.mul(inv),
        j1: next.mul(inv),
        s0: s0.mul(inv),
        s1: s1.mul(inv),
    }
}

/// `ln(x/2) + gamma`.
fn log_term(x: L) -> DD {
    ln_dd(x).sub(LN_2).add(EULER_GAMMA)
}

/// Y0 from Miller's values: `(2/pi) ((ln(x/2) + gamma) J0 - 2 s0)`.
fn y0_low(m: &Low, x: L) -> DD {
    TWO_OVER_PI.mul(log_term(x).mul(m.j0).sub(m.s0.twice()))
}

/// Y1 from Miller's values: `(2/pi) (-J0/x + (ln(x/2) + gamma - 1) J1 -
/// s1)`.
fn y1_low(m: &Low, x: L) -> DD {
    let lg = log_term(x).add_l(-ONE);
    TWO_OVER_PI.mul(lg.mul(m.j1).sub(m.j0.div_l(x)).sub(m.s1))
}

/// `atan(u)` for `|u| < 2^-6` (Hankel's `Q/P` is below `3/(8x)`): `u (1 -
/// u^2/3 + u^4/5 - ...)`, to `u^18`.
fn atan_small(u: DD) -> DD {
    let u2 = u.mul(u);
    let mut acc = recip(19);
    for j in (0..9u32).rev() {
        acc = recip(2 * j + 1).sub(u2.mul(acc));
    }
    acc.mul(u)
}

/// `cos t` and `sin t` for `|t| <= 0.8`, by their Taylor series to `t^37`.
fn cos_sin(t: DD) -> (DD, DD) {
    let t2 = t.mul(t);
    let (mut c, mut s) = (inv_factorial(36), inv_factorial(37));
    for k in (0..18usize).rev() {
        c = inv_factorial(2 * k).sub(t2.mul(c));
        s = inv_factorial(2 * k + 1).sub(t2.mul(s));
    }
    (c, s.mul(t))
}

/// The exact sum of `terms`, as [`DD`]: two passes of the error-free
/// transformation (Ogita, Rump and Oishi's `VecSum`), then the errors added
/// to the last -- as good as summing in thrice the precision, so the sum of
/// terms that cancel to almost nothing keeps its relative precision.
fn exact_sum(terms: &mut [L]) -> DD {
    for _ in 0..2 {
        let mut iter = terms.iter_mut();
        let Some(mut prev) = iter.next() else {
            return DD::ZERO;
        };
        for cur in iter {
            let (s, e) = two_sum(*cur, *prev);
            *cur = s;
            *prev = e;
            prev = cur;
        }
    }
    let Some((last, errors)) = terms.split_last() else {
        return DD::ZERO;
    };
    let err = errors.iter().fold(ZERO, |acc, &e| acc + e);
    DD::norm(fast_two_sum(*last, err))
}

/// `x + psi` reduced by pi/4: the odd `K` nearest `(x + psi) 4/pi`, as `K`
/// mod 8, and `t = x + psi - K pi/4`, `|t| <= pi/4` or a hair past, for `x >
/// XH` and `psi` Hankel's small phase correction.
fn reduce(x: L, psi: DD) -> (u32, DD) {
    if x < X_REDUCE {
        // K below 2^40: K times each 24-bit piece of pi/4 is exact, and so
        // is the sum of them all with x and psi.
        let q = (x + psi.hi) * L::from_bits(0x3FFF, 0xA2F9_836E_4E44_152A); // 4/pi
        let k = (q * HALF).round_int_toward(1) * TWO + ONE;
        let [p0, p1, p2, p3, p4, p5, p6, p7] = PI_OVER_4_PIECES;
        let mut terms = [
            x,
            -(k * p0),
            -(k * p1),
            -(k * p2),
            -(k * p3),
            -(k * p4),
            -(k * p5),
            -(k * p6),
            -(k * p7),
            psi.hi,
            psi.lo,
        ];
        let t = exact_sum(&mut terms);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let k8 = (k.to_i64_rint() & 7) as u32;
        return (k8, t);
    }
    // x = n pi/2 + y, |y| <= pi/4; then x + psi - (2n + j) pi/4 = y + psi -
    // j pi/4, j = +-1.
    let (n, y) = rem_pio2_113(x);
    let base = y.add(psi);
    let (j, t) = if base.hi.is_sign_negative() {
        (-1i64, base.add(PI_OVER_4))
    } else {
        (1i64, base.sub(PI_OVER_4))
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let k8 = ((2 * n + j).rem_euclid(8)) as u32;
    (k8, t)
}

/// `x` (at least 2^39) reduced by pi/2: `n` mod 8 and the remainder, to 113
/// bits (Payne and Hanek's reduction, [`crate::rem_pio2_large`], at its
/// quad precision).
fn rem_pio2_113(x: L) -> (i64, DD) {
    let ex = i32::from(x.biased_exponent());
    // |x| scaled to [2^23, 2^24), cut into three 24-bit doubles.
    let mut z = L::from_bits(0x3FFF + 23, x.significand);
    let mut take24 = || {
        #[allow(clippy::cast_possible_truncation)]
        let whole = f64::from(z.to_f64() as i32);
        z = (z - L::from_f64(whole)) * L::from_f64(16_777_216.0);
        whole
    };
    let t0 = take24();
    let t1 = take24();
    let t2 = z.to_f64();
    let tx = [t0, t1, t2];
    let nx = if t2 != 0.0 {
        3
    } else if t1 != 0.0 {
        2
    } else {
        1
    };
    let mut ty = [0.0_f64; 3];
    let n = crate::rem_pio2_large::rem_pio2_large(
        tx.get(..nx).unwrap_or(&tx),
        &mut ty,
        ex - 0x3FFF - 23,
        3,
    );
    let [y0, y1, y2] = ty;
    let y = DD::norm(two_sum(L::from_f64(y0), L::from_f64(y1))).add_l(L::from_f64(y2));
    (i64::from(n) & 7, y)
}

/// `J_nu(x)` and `Y_nu(x)`, `nu` 0 or 1, for `x > XH`: Hankel's `P` and `Q`,
/// `P = 1 - a2/x^2 + a4/x^4 - ...`, `Q = a1/x - a3/x^3 + ...`, `a_k = prod
/// (4 nu^2 - (2j - 1)^2) / (k! 8^k)`, summed while the terms fall and are
/// above 2^-145; then `J = M cos theta`, `Y = M sin theta`.
fn hankel(nu: u32, x: L) -> (DD, DD) {
    let (xs, e) = split(x);
    let inv_xs = DD::l(ONE).div_l(xs);
    let mu = i64::from(4 * nu * nu);
    let (mut t, mut p, mut q) = (DD::l(ONE), DD::l(ONE), DD::ZERO);
    let mut last = ONE;
    // Past 2^200 the first term, (4 nu^2 - 1)/(8x), is below 2^-200: P is 1
    // and Q nothing, to every bit the phase needs.
    let terms = if e > 200 { 0 } else { 400u32 };
    let inv_8x = inv_xs.scale(-e - 3);
    for k in 1..terms {
        let odd = 2 * i64::from(k) - 1;
        t = t.mul_l(int(mu - odd * odd)).mul(recip(k)).mul(inv_8x);
        let size = t.mag();
        if size >= last {
            break;
        }
        last = size;
        match k % 4 {
            1 => q = q.add(t),
            2 => p = p.sub(t),
            3 => q = q.sub(t),
            _ => p = p.add(t),
        }
        if size < HANKEL_EPS {
            break;
        }
    }
    let psi = atan_small(q.div(p));
    // sqrt(2/(pi x)) (P^2 + Q^2)^(1/2), x's power of 2 halved outside the
    // root: 2^-8192 at the largest x is normal, 1/x there is not.
    let amp = TWO_OVER_PI
        .mul(inv_xs)
        .mul(p.mul(p).add(q.mul(q)))
        .scale(-(e & 1))
        .sqrt()
        .scale(-(e >> 1));
    let (k8, t) = reduce(x, psi);
    // theta = t + m pi/2, K = 2m + 2 nu + 1.
    let m = ((k8 + 8 - (2 * nu + 1)) % 8) / 2;
    let (c, s) = cos_sin(t);
    let (cos_theta, sin_theta) = match m {
        0 => (c, s),
        1 => (s.neg(), c),
        2 => (c.neg(), s.neg()),
        _ => (s, c.neg()),
    };
    (amp.mul(cos_theta), amp.mul(sin_theta))
}

/// Which of the four.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    J0,
    J1,
    Y0,
    Y1,
}

/// J0, J1, Y0 or Y1 at a finite `x >= X_TINY`, in [`DD`].
fn order01(kind: Kind, x: L) -> DD {
    let (nu, zeros) = match kind {
        Kind::J0 => (0, J0_ZEROS),
        Kind::J1 => (1, J1_ZEROS),
        Kind::Y0 => (0, Y0_ZEROS),
        Kind::Y1 => (1, Y1_ZEROS),
    };
    if x > XH {
        let (j, y) = hankel(nu, x);
        return if matches!(kind, Kind::J0 | Kind::J1) {
            j
        } else {
            y
        };
    }
    if let Some(v) = near_zero(zeros, x) {
        return v;
    }
    let low = miller01(x);
    match kind {
        Kind::J0 => low.j0,
        Kind::J1 => low.j1,
        Kind::Y0 => y0_low(&low, x),
        Kind::Y1 => y1_low(&low, x),
    }
}

/// Y0 and Y1 at a finite `x > 0`, scaled by a common `2^e` (`(y0, y1, e)`
/// for `Y0 = y0 2^e`...): below [`X_TINY`] Y1's pole `-2/(pi x)` overflows
/// as `x` nears the least subnormal, so it is carried scaled.
fn y01_scaled(x: L) -> (DD, DD, i32) {
    if x > XH {
        return (hankel(0, x).1, hankel(1, x).1, 0);
    }
    if x >= X_TINY {
        // One recurrence for both; a zero's window is 2^-32 wide, and the
        // two functions' zeros are apart.
        let low = miller01(x);
        let y0 = near_zero(Y0_ZEROS, x).unwrap_or_else(|| y0_low(&low, x));
        let y1 = near_zero(Y1_ZEROS, x).unwrap_or_else(|| y1_low(&low, x));
        return (y0, y1, 0);
    }
    let (xs, e) = split(x);
    let y1 = TWO_OVER_PI.div_l(xs).neg();
    let y0 = TWO_OVER_PI.mul(log_term(x));
    (y0.scale(e), y1, -e)
}

// ---------------------------------------------------------------------------
// The n-th orders
// ---------------------------------------------------------------------------

/// `J_m(x)` for `m >= 2` and a finite `x > 0`, as `(v, e)` for `v 2^e`.
fn jn_scaled(m: u32, x: L) -> (DD, i32) {
    let ln_x = ln_f64(x);
    let estimate = debye_ln(m, ln_x, false) / LN2_F64;
    if estimate < -16_445.0 - 16.0 {
        // Far below the least subnormal: positive (J_m has no zero below
        // m), and that is all the rounding needs.
        return (DD::l(ONE), -16_445 - 64);
    }
    // The series where (x/2)^2 < m + 1 -- x below 2^17, as m is below 2^31.
    if x < pow2(17) {
        let half = x * HALF;
        let q = DD::norm(two_prod(half, half));
        if q.hi < int(i64::from(m) + 1) {
            return jn_series(m, half, q);
        }
    }
    if x > XH && int(i64::from(m)) <= x {
        // Forward from J0 and J1, stable while the order stays below x.
        let (mut a, mut b) = (hankel(0, x).0, hankel(1, x).0);
        let two_over_x = over_x(TWO, x);
        for k in 1..m {
            let c = two_over_x.mul_l(int(i64::from(k))).mul(b).sub(a);
            a = b;
            b = c;
        }
        return (b, 0);
    }
    jn_miller(m, x)
}

/// `J_m(x)` by its series, `(x/2)^m / m! sum (-q)^k / (k! (m+1)...(m+k))`,
/// `q = (x/2)^2 < m + 1`: the terms fall from the first, so the sum is
/// between `1 - q/(m+1)` and 1. The power is carried scaled.
fn jn_series(m: u32, half: L, q: DD) -> (DD, i32) {
    let mut t = DD::l(ONE);
    let mut e = 0i32;
    for i in 1..=m {
        t = t.mul_l(half).div_l(int(i64::from(i)));
        let ex = exponent(t.hi);
        if !(-RESCALE..=RESCALE).contains(&ex) {
            t = t.scale(-ex);
            e += ex;
        }
    }
    if q.hi < pow2(-130) {
        // The sum is 1 - q/(m+1) + ..., its correction below what DD holds:
        // 1 with a sticky bit below it, for the rounding's sake.
        let sum = DD {
            hi: ONE,
            lo: -pow2(-200),
        };
        return (t.mul(sum), e);
    }
    let (mut term, mut sum) = (DD::l(ONE), DD::l(ONE));
    for k in 1..10_000u32 {
        let d = i64::from(k) * (i64::from(m) + i64::from(k));
        term = term.mul(q).div_l(int(d)).neg();
        sum = sum.add(term);
        if term.mag() < sum.mag() * EPS {
            break;
        }
    }
    (t.mul(sum), e)
}

/// `J_m(x)` by Miller's recurrence from [`start_index`] down to 0,
/// normalised by `J0 + 2 (J2 + J4 + ...) = 1`; the values are scaled down
/// as they grow, and `f(m)`'s scale remembered.
fn jn_miller(m: u32, x: L) -> (DD, i32) {
    let big_n = start_index(m, x);
    let two_over_x = DD::l(TWO).div_l(x);
    let (mut next, mut cur) = (DD::ZERO, DD::l(ONE));
    let mut norm = DD::ZERO;
    let (mut ans, mut ans_e) = (DD::ZERO, 0i32);
    let mut e = 0i32;
    for k in (1..=big_n).rev() {
        if k == m {
            ans = cur;
            ans_e = e;
        }
        if k % 2 == 0 {
            norm = norm.add(cur.twice());
        }
        let prev = two_over_x.mul_l(int(i64::from(k))).mul(cur).sub(next);
        next = cur;
        cur = prev;
        if cur.mag() > pow2(RESCALE) {
            cur = cur.scale(-RESCALE);
            next = next.scale(-RESCALE);
            norm = norm.scale(-RESCALE);
            e += RESCALE;
        }
    }
    norm = norm.add(cur);
    // J_m = (ans 2^ans_e) / (norm 2^e).
    (ans.div(norm), ans_e - e)
}

/// `Y_m(x)` for `m >= 2` and a finite `x > 0`, as `(v, e)` for `v 2^e`: the
/// forward recurrence from Y0 and Y1, scaled down as it grows.
fn yn_scaled(m: u32, x: L) -> (DD, i32) {
    let ln_x = ln_f64(x);
    let estimate = debye_ln(m, ln_x, true) / LN2_F64;
    if estimate > 16_384.0 + 16.0 {
        // Far past the largest finite value, negative (Y_m has no zero
        // below m).
        return (DD::l(-ONE), 16_384 + 64);
    }
    let (mut a, mut b, mut e) = y01_scaled(x);
    let two_over_x = over_x(TWO, x);
    for k in 1..m {
        let c = two_over_x.mul_l(int(i64::from(k))).mul(b).sub(a);
        a = b;
        b = c;
        if b.mag() > pow2(RESCALE) {
            a = a.scale(-RESCALE);
            b = b.scale(-RESCALE);
            e += RESCALE;
        }
        if e > 16_384 + RESCALE {
            break;
        }
    }
    (b, e)
}

// ---------------------------------------------------------------------------
// The last rounding
// ---------------------------------------------------------------------------

/// `v 2^e` rounded once to a long double in the caller's direction, with
/// the flags that rounding raises: inexact unless exact, underflow for a
/// tiny inexact result, overflow past the largest. `v.hi` is normal or
/// zero.
fn round_scaled(v: DD, e: i32) -> L {
    if v.hi.is_zero() {
        return v.hi;
    }
    let target = exponent(v.hi).saturating_add(e);
    if target > 16_383 {
        let big = core::hint::black_box(LDBL_MAX);
        return big.copysign(v.hi) * big;
    }
    if target >= -16_382 + 128 {
        let hi = v.hi.scalbn(e);
        let lo = if v.lo.is_zero() {
            ZERO
        } else if exponent(v.lo) + e > -16_382 {
            v.lo.scalbn(e)
        } else {
            // Too small to scale without underflowing: only its sign can
            // matter, and a sticky bit far below the last place keeps it.
            pow2(target - 80).copysign(v.lo)
        };
        return hi + lo;
    }
    // Near or below the least normal: round to a whole number of the
    // result's last place, 2^q.
    let q = if target >= -16_382 {
        target - 63
    } else {
        -16_445
    };
    let shift = e - q;
    let h = v.hi.scalbn(shift);
    let l = if v.lo.is_zero() {
        ZERO
    } else if exponent(v.lo) + shift > -16_382 {
        v.lo.scalbn(shift)
    } else {
        pow2(-100).copysign(v.lo)
    };
    let (n, inexact) = round_to_integer(h, l);
    // A zero keeps the value's sign, which `floor + 1` can lose.
    let r = if n.is_zero() {
        ZERO.copysign(v.hi)
    } else {
        n.scalbn(q)
    };
    if inexact {
        let tiny = r.abs() < pow2(-16_382);
        let flags = if tiny {
            FE_INEXACT | FE_UNDERFLOW
        } else {
            FE_INEXACT
        };
        // The return value only reports an argument outside FE_ALL_EXCEPT.
        let _ = crate::fenv::feraiseexcept(flags);
    }
    r
}

/// `h + l` rounded to an integer in the caller's direction, and whether that
/// was inexact; `|h| < 2^65`, `|l| <= ulp(h)/2`.
fn round_to_integer(h: L, l: L) -> (L, bool) {
    let t = h.round_int_toward(3);
    // h - t is exact; with l, the fraction as f + g exactly.
    let (f, g) = two_sum(h - t, l);
    let below = f < ZERO || (f.is_zero() && g < ZERO);
    let above = f > ZERO || (f.is_zero() && g > ZERO);
    let inexact = below || above;
    let floor = if below { t - ONE } else { t };
    let ceil = if above { t + ONE } else { t };
    let n = match crate::fenv::fegetround() {
        crate::fenv::FE_UPWARD => ceil,
        crate::fenv::FE_DOWNWARD => floor,
        crate::fenv::FE_TOWARDZERO => {
            if h.is_sign_negative() {
                ceil
            } else {
                floor
            }
        }
        _ => {
            // The distance above floor, d = (f - (floor - t)) + g, against 1/2.
            let d = f - (floor - t);
            let up = d > HALF || (d == HALF && (g > ZERO || (g.is_zero() && !is_even(floor))));
            if up { floor + ONE } else { floor }
        }
    };
    (n, inexact)
}

/// Whether an integer-valued long double is even.
fn is_even(n: L) -> bool {
    (n * HALF).round_int_toward(3) == n * HALF
}

/// The core of every function: `f(args)` computed with the flags held and to
/// nearest, then rounded once, negated first if `negate`.
fn finish<A>(args: A, negate: bool, f: impl FnOnce(A) -> (DD, i32)) -> L {
    let (v, e) = crate::fenv::quietly_in_nearest_x87(args, f);
    round_scaled(if negate { v.neg() } else { v }, e)
}

// ---------------------------------------------------------------------------
// The functions: special values, errno, the range
// ---------------------------------------------------------------------------

/// The `y` functions' domain: `EDOM` below 0, `ERANGE` at 0 (glibc's
/// `w_j0_template.c` and kin).
fn y_errno(x: L) {
    if x.is_nan() {
        return;
    }
    if x.is_zero() {
        errno::set_errno(errno::ERANGE);
    } else if x.is_sign_negative() {
        errno::set_errno(errno::EDOM);
    }
}

/// A `y` function at 0 (`-inf`, divide-by-zero) or below (a quiet NaN,
/// invalid), `None` elsewhere.
fn y_pole_or_domain(x: L) -> Option<L> {
    if x.is_zero() {
        return Some(-ONE / core::hint::black_box(ZERO));
    }
    if x.is_sign_negative() {
        // glibc's +NaN, where 0/0 on the x87 is the negative default NaN.
        return Some((ZERO / core::hint::black_box(ZERO)).abs());
    }
    None
}

/// Bessel function of the first kind, order 0.
#[must_use]
pub fn j0l(x: L) -> L {
    if x.is_nan() {
        return x + x;
    }
    if x.is_infinite() {
        return ZERO;
    }
    if x.is_zero() {
        return ONE;
    }
    let a = x.abs();
    if a < X_SMALL_J {
        // 1 - x^2/4: 1, or its neighbour below where the direction says.
        return ONE - core::hint::black_box(pow2(-200));
    }
    finish(a, false, |a| (order01(Kind::J0, a), 0))
}

/// `J1(x)` for a finite nonzero `x`, rounded in the caller's direction.
fn j1_finite(x: L) -> L {
    finish(x.abs(), x.is_sign_negative(), |a| {
        if a < X_SMALL_J {
            // x/2 - x^3/16: x scaled to 2^-100, where both parts are normal,
            // the second only a sticky bit below the first.
            let e = exponent(a);
            let s = a.scalbn(-100 - e);
            let v = DD {
                hi: s * HALF,
                lo: -(s * pow2(-200)),
            };
            (v, e + 100)
        } else {
            (order01(Kind::J1, a), 0)
        }
    })
}

/// Bessel function of the first kind, order 1: odd, `+-0` at `+-inf` and at
/// `+-0`, `ERANGE` where a nonzero argument's answer underflows to zero.
#[must_use]
pub fn j1l(x: L) -> L {
    if x.is_nan() {
        return x + x;
    }
    if x.is_infinite() {
        return ZERO.copysign(x);
    }
    if x.is_zero() {
        return x;
    }
    ranged(x, j1_finite, |r| underflow_only(r.is_zero()))
}

/// Bessel function of the first kind, order `n`: `J(-n, x) = J(n, -x)`, a
/// signed zero at 0 and at an infinity, `ERANGE` on underflow.
#[must_use]
pub fn jnl(n: i32, x: L) -> L {
    if x.is_nan() {
        return x + x;
    }
    let m = n.unsigned_abs();
    let xx = if n < 0 { -x } else { x };
    match m {
        0 => j0l(xx),
        1 => j1l(xx),
        _ => {
            let negate = m % 2 == 1 && xx.is_sign_negative();
            if xx.is_zero() || xx.is_infinite() {
                return if negate { -ZERO } else { ZERO };
            }
            ranged(
                (m, xx.abs(), negate),
                |(m, a, negate)| finish((m, a), negate, |(m, a)| jn_scaled(m, a)),
                |r| underflow_only(r.is_zero()),
            )
        }
    }
}

/// Bessel function of the second kind, order 0.
#[must_use]
pub fn y0l(x: L) -> L {
    y_errno(x);
    if x.is_nan() {
        return x + x;
    }
    if let Some(r) = y_pole_or_domain(x) {
        return r;
    }
    if x.is_infinite() {
        return ZERO;
    }
    finish(x, false, |x| {
        if x < X_TINY {
            (TWO_OVER_PI.mul(log_term(x)), 0)
        } else {
            (order01(Kind::Y0, x), 0)
        }
    })
}

/// `Y1(x)`, or `-Y1(x)` if `negate`, for a finite `x > 0`, rounded in the
/// caller's direction.
fn y1_finite(x: L, negate: bool) -> L {
    finish(x, negate, |x| {
        if x < X_TINY {
            // -2/(pi x), x scaled to [1, 2) so it cannot overflow here.
            let (xs, e) = split(x);
            (TWO_OVER_PI.div_l(xs).neg(), -e)
        } else {
            (order01(Kind::Y1, x), 0)
        }
    })
}

/// Bessel function of the second kind, order 1: `ERANGE` where `-2/(pi x)`
/// overflows.
#[must_use]
pub fn y1l(x: L) -> L {
    y1_signed(x, false)
}

/// [`y1l`], negated if `negate` (`ynl` of order -1): the pole and the zero at
/// `+inf` take the sign, the NaNs do not, and the value is rounded as the
/// negation is.
fn y1_signed(x: L, negate: bool) -> L {
    y_errno(x);
    if x.is_nan() {
        return x + x;
    }
    if let Some(r) = y_pole_or_domain(x) {
        return if negate && x.is_zero() { -r } else { r };
    }
    if x.is_infinite() {
        return if negate { -ZERO } else { ZERO };
    }
    ranged(x, |x| y1_finite(x, negate), |r| overflow_only(true, r))
}

/// Bessel function of the second kind, order `n`: `Y(-n, x) = (-1)^n Y(n,
/// x)`; `+0` at `+inf` for an order past 1 whatever its sign, as glibc
/// answers; `ERANGE` on overflow.
#[must_use]
pub fn ynl(n: i32, x: L) -> L {
    let m = n.unsigned_abs();
    let negate = n < 0 && m % 2 == 1;
    match m {
        0 => y0l(x),
        1 => y1_signed(x, negate),
        _ => {
            y_errno(x);
            if x.is_nan() {
                return x + x;
            }
            if let Some(r) = y_pole_or_domain(x) {
                // The pole's sign follows the order's; the NaN's does not.
                return if negate && x.is_zero() { -r } else { r };
            }
            if x.is_infinite() {
                return ZERO;
            }
            ranged(
                (m, x, negate),
                |(m, x, negate)| finish((m, x), negate, |(m, x)| yn_scaled(m, x)),
                |r| overflow_only(true, r),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// The C entry points
// ---------------------------------------------------------------------------

macro_rules! export_l_l {
    ($($c:literal $shim:ident $f:path;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, out: *mut L) {
            // SAFETY: called only by its thunk, with the caller's stack
            // argument and the thunk's result slot, both long-double slots.
            unsafe { out.write($f(x.read())) }
        }
        crate::ld_c!(l_l $c => $shim);
    )* };
}

export_l_l! {
    "j0l" __slate_ld_j0l j0l;
    "j1l" __slate_ld_j1l j1l;
    "y0l" __slate_ld_y0l y0l;
    "y1l" __slate_ld_y1l y1l;
}

/// `jnl(n, x)`: C's `(int, long double)`, which the ABI passes as it passes
/// `(long double, int)` -- the `int` in `%edi`, the `long double` on the
/// stack -- so it is the `l_li` shape.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_jnl(x: *const L, n: i32, out: *mut L) {
    // SAFETY: as in `export_l_l!`.
    unsafe { out.write(jnl(n, x.read())) }
}
crate::ld_c!(l_li "jnl" => __slate_ld_jnl);

/// `ynl(n, x)`, the same shape.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_ynl(x: *const L, n: i32, out: *mut L) {
    // SAFETY: as in `export_l_l!`.
    unsafe { out.write(ynl(n, x.read())) }
}
crate::ld_c!(l_li "ynl" => __slate_ld_ynl);

// ---------------------------------------------------------------------------
// The tables: `python posix/tools/oracle/besl_tables.py table` prints them,
// `... check` says whether these are still what it prints
// ---------------------------------------------------------------------------

// Generated by posix/tools/oracle/besl_tables.py (mpmath, 100 digits): do not edit.

const TWO_OVER_PI: DD = DD {
    hi: L::from_bits(0x3FFE, 0xA2F9836E4E44152A),
    lo: L::from_bits(0xBFB8, 0xF62A0B82B2C88FC9),
};
const EULER_GAMMA: DD = DD {
    hi: L::from_bits(0x3FFE, 0x93C467E37DB0C7A5),
    lo: L::from_bits(0xBFBC, 0xB90701FBFAB4D2A5),
};
const LN_2: DD = DD {
    hi: L::from_bits(0x3FFE, 0xB17217F7D1CF79AC),
    lo: L::from_bits(0xBFBC, 0xD871319FF0342543),
};
const PI_OVER_4: DD = DD {
    hi: L::from_bits(0x3FFE, 0xC90FDAA22168C235),
    lo: L::from_bits(0xBFBC, 0xECE675D1FC8F8CBB),
};
/// pi/4 as eight pieces of at most 24 significant bits, largest first:
/// their sum is pi/4 to 2^-190, and each times an integer below 2^40 is exact.
const PI_OVER_4_PIECES: [L; 8] = [
    L::from_bits(0x3FFE, 0xC90FDA0000000000),
    L::from_bits(0x3FE6, 0xA221680000000000),
    L::from_bits(0x3FCE, 0xC234C40000000000),
    L::from_bits(0x3FB6, 0xC6628B0000000000),
    L::from_bits(0x3F9E, 0x80DC1C0000000000),
    L::from_bits(0x3F86, 0xD129020000000000),
    L::from_bits(0x3F6D, 0x9C11140000000000),
    L::from_bits(0x3F55, 0xCF98E80000000000),
];
/// 1/k for k = 1 to 128, as DD (index k - 1).
const RECIPROCALS: [DD; 128] = [
    DD {
        hi: L::from_bits(0x3FFF, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFE, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFD, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFBC, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FFD, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFC, 0xCCCCCCCCCCCCCCCD),
        lo: L::from_bits(0xBFBA, 0xCCCCCCCCCCCCCCCD),
    },
    DD {
        hi: L::from_bits(0x3FFC, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFBB, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FFC, 0x9249249249249249),
        lo: L::from_bits(0x3FBA, 0x9249249249249249),
    },
    DD {
        hi: L::from_bits(0x3FFC, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0xE38E38E38E38E38E),
        lo: L::from_bits(0x3FB9, 0xE38E38E38E38E38E),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0xCCCCCCCCCCCCCCCD),
        lo: L::from_bits(0xBFB9, 0xCCCCCCCCCCCCCCCD),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0xBA2E8BA2E8BA2E8C),
        lo: L::from_bits(0xBFBA, 0xBA2E8BA2E8BA2E8C),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFBA, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0x9D89D89D89D89D8A),
        lo: L::from_bits(0xBFB9, 0x9D89D89D89D89D8A),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0x9249249249249249),
        lo: L::from_bits(0x3FB9, 0x9249249249249249),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0x8888888888888889),
        lo: L::from_bits(0xBFBA, 0xEEEEEEEEEEEEEEEF),
    },
    DD {
        hi: L::from_bits(0x3FFB, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xF0F0F0F0F0F0F0F1),
        lo: L::from_bits(0xBFB6, 0xF0F0F0F0F0F0F0F1),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xE38E38E38E38E38E),
        lo: L::from_bits(0x3FB8, 0xE38E38E38E38E38E),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xD79435E50D79435E),
        lo: L::from_bits(0x3FB9, 0xA1AF286BCA1AF287),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xCCCCCCCCCCCCCCCD),
        lo: L::from_bits(0xBFB8, 0xCCCCCCCCCCCCCCCD),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xC30C30C30C30C30C),
        lo: L::from_bits(0x3FB8, 0xC30C30C30C30C30C),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xBA2E8BA2E8BA2E8C),
        lo: L::from_bits(0xBFB9, 0xBA2E8BA2E8BA2E8C),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xB21642C8590B2164),
        lo: L::from_bits(0x3FB8, 0xB21642C8590B2164),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFB9, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xA3D70A3D70A3D70A),
        lo: L::from_bits(0x3FB8, 0xF5C28F5C28F5C28F),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0x9D89D89D89D89D8A),
        lo: L::from_bits(0xBFB8, 0x9D89D89D89D89D8A),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0x97B425ED097B425F),
        lo: L::from_bits(0xBFB8, 0xBDA12F684BDA12F7),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0x9249249249249249),
        lo: L::from_bits(0x3FB8, 0x9249249249249249),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0x8D3DCB08D3DCB08D),
        lo: L::from_bits(0x3FB8, 0xF72C234F72C234F7),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0x8888888888888889),
        lo: L::from_bits(0xBFB9, 0xEEEEEEEEEEEEEEEF),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0x8421084210842108),
        lo: L::from_bits(0x3FB9, 0x8421084210842108),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xF83E0F83E0F83E10),
        lo: L::from_bits(0xBFB8, 0xF83E0F83E0F83E10),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xF0F0F0F0F0F0F0F1),
        lo: L::from_bits(0xBFB5, 0xF0F0F0F0F0F0F0F1),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xEA0EA0EA0EA0EA0F),
        lo: L::from_bits(0xBFB8, 0xBE2BE2BE2BE2BE2C),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xE38E38E38E38E38E),
        lo: L::from_bits(0x3FB7, 0xE38E38E38E38E38E),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xDD67C8A60DD67C8A),
        lo: L::from_bits(0x3FB8, 0xC1BACF914C1BACF9),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xD79435E50D79435E),
        lo: L::from_bits(0x3FB8, 0xA1AF286BCA1AF287),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xD20D20D20D20D20D),
        lo: L::from_bits(0x3FB7, 0x8348348348348348),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xCCCCCCCCCCCCCCCD),
        lo: L::from_bits(0xBFB7, 0xCCCCCCCCCCCCCCCD),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xC7CE0C7CE0C7CE0C),
        lo: L::from_bits(0x3FB8, 0xF9C18F9C18F9C190),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xC30C30C30C30C30C),
        lo: L::from_bits(0x3FB7, 0xC30C30C30C30C30C),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xBE82FA0BE82FA0BF),
        lo: L::from_bits(0xBFB8, 0xFA0BE82FA0BE82FA),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xBA2E8BA2E8BA2E8C),
        lo: L::from_bits(0xBFB8, 0xBA2E8BA2E8BA2E8C),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xB60B60B60B60B60B),
        lo: L::from_bits(0x3FB8, 0xC16C16C16C16C16C),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xB21642C8590B2164),
        lo: L::from_bits(0x3FB7, 0xB21642C8590B2164),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xAE4C415C9882B931),
        lo: L::from_bits(0x3FB4, 0xAE4C415C9882B931),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFB8, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xA72F05397829CBC1),
        lo: L::from_bits(0x3FB8, 0x9CBC14E5E0A72F05),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xA3D70A3D70A3D70A),
        lo: L::from_bits(0x3FB7, 0xF5C28F5C28F5C28F),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0xA0A0A0A0A0A0A0A1),
        lo: L::from_bits(0xBFB8, 0xBEBEBEBEBEBEBEBF),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x9D89D89D89D89D8A),
        lo: L::from_bits(0xBFB7, 0x9D89D89D89D89D8A),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x9A90E7D95BC609A9),
        lo: L::from_bits(0x3FB5, 0xE7D95BC609A90E7E),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x97B425ED097B425F),
        lo: L::from_bits(0xBFB7, 0xBDA12F684BDA12F7),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x94F2094F2094F209),
        lo: L::from_bits(0x3FB8, 0x9E4129E4129E412A),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x9249249249249249),
        lo: L::from_bits(0x3FB7, 0x9249249249249249),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x8FB823EE08FB823F),
        lo: L::from_bits(0xBFB6, 0xFB823EE08FB823EE),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x8D3DCB08D3DCB08D),
        lo: L::from_bits(0x3FB7, 0xF72C234F72C234F7),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x8AD8F2FBA9386823),
        lo: L::from_bits(0xBFB8, 0x9386822B63CBEEA5),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x8888888888888889),
        lo: L::from_bits(0xBFB8, 0xEEEEEEEEEEEEEEEF),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x864B8A7DE6D1D608),
        lo: L::from_bits(0x3FB8, 0xC9714FBCDA3AC10D),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x8421084210842108),
        lo: L::from_bits(0x3FB8, 0x8421084210842108),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x8208208208208208),
        lo: L::from_bits(0x3FB7, 0x8208208208208208),
    },
    DD {
        hi: L::from_bits(0x3FF9, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xFC0FC0FC0FC0FC10),
        lo: L::from_bits(0xBFB6, 0xFC0FC0FC0FC0FC10),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xF83E0F83E0F83E10),
        lo: L::from_bits(0xBFB7, 0xF83E0F83E0F83E10),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xF4898D5F85BB3950),
        lo: L::from_bits(0x3FB6, 0xF4898D5F85BB3950),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xF0F0F0F0F0F0F0F1),
        lo: L::from_bits(0xBFB4, 0xF0F0F0F0F0F0F0F1),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xED7303B5CC0ED730),
        lo: L::from_bits(0x3FB6, 0xED7303B5CC0ED730),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xEA0EA0EA0EA0EA0F),
        lo: L::from_bits(0xBFB7, 0xBE2BE2BE2BE2BE2C),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xE6C2B4481CD85689),
        lo: L::from_bits(0x3FB2, 0xE6C2B4481CD85689),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xE38E38E38E38E38E),
        lo: L::from_bits(0x3FB6, 0xE38E38E38E38E38E),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xE070381C0E070382),
        lo: L::from_bits(0xBFB6, 0xFC7E3F1F8FC7E3F2),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xDD67C8A60DD67C8A),
        lo: L::from_bits(0x3FB7, 0xC1BACF914C1BACF9),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xDA740DA740DA740E),
        lo: L::from_bits(0xBFB7, 0xB17E4B17E4B17E4B),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xD79435E50D79435E),
        lo: L::from_bits(0x3FB7, 0xA1AF286BCA1AF287),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xD4C77B03531DEC0D),
        lo: L::from_bits(0x3FB7, 0x98EF606A63BD81AA),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xD20D20D20D20D20D),
        lo: L::from_bits(0x3FB6, 0x8348348348348348),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xCF6474A8819EC8E9),
        lo: L::from_bits(0x3FB7, 0xA2067B23A5440CF6),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xCCCCCCCCCCCCCCCD),
        lo: L::from_bits(0xBFB6, 0xCCCCCCCCCCCCCCCD),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xCA4587E6B74F0329),
        lo: L::from_bits(0x3FB5, 0xB0FCD6E9E06522C4),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xC7CE0C7CE0C7CE0C),
        lo: L::from_bits(0x3FB7, 0xF9C18F9C18F9C190),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xC565C87B5F9D4D1C),
        lo: L::from_bits(0xBFB6, 0xF6BF3A9A3784A063),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xC30C30C30C30C30C),
        lo: L::from_bits(0x3FB6, 0xC30C30C30C30C30C),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xC0C0C0C0C0C0C0C1),
        lo: L::from_bits(0xBFB6, 0xFCFCFCFCFCFCFCFD),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xBE82FA0BE82FA0BF),
        lo: L::from_bits(0xBFB7, 0xFA0BE82FA0BE82FA),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xBC52640BC52640BC),
        lo: L::from_bits(0x3FB7, 0xA4C8178A4C8178A5),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xBA2E8BA2E8BA2E8C),
        lo: L::from_bits(0xBFB7, 0xBA2E8BA2E8BA2E8C),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xB81702E05C0B8170),
        lo: L::from_bits(0x3FB6, 0xB81702E05C0B8170),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xB60B60B60B60B60B),
        lo: L::from_bits(0x3FB7, 0xC16C16C16C16C16C),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xB40B40B40B40B40B),
        lo: L::from_bits(0x3FB7, 0x8168168168168168),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xB21642C8590B2164),
        lo: L::from_bits(0x3FB6, 0xB21642C8590B2164),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xB02C0B02C0B02C0B),
        lo: L::from_bits(0x3FB2, 0xB02C0B02C0B02C0B),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xAE4C415C9882B931),
        lo: L::from_bits(0x3FB3, 0xAE4C415C9882B931),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xAC7691840AC76918),
        lo: L::from_bits(0x3FB7, 0x8158ED2308158ED2),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFB7, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xA8E83F5717C0A8E8),
        lo: L::from_bits(0x3FB6, 0xFD5C5F02A3A0FD5C),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xA72F05397829CBC1),
        lo: L::from_bits(0x3FB7, 0x9CBC14E5E0A72F05),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xA57EB50295FAD40A),
        lo: L::from_bits(0x3FB7, 0xAFD6A052BF5A814B),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xA3D70A3D70A3D70A),
        lo: L::from_bits(0x3FB6, 0xF5C28F5C28F5C28F),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xA237C32B16CFD772),
        lo: L::from_bits(0x3FB4, 0xF353A4C0A237C32B),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0xA0A0A0A0A0A0A0A1),
        lo: L::from_bits(0xBFB7, 0xBEBEBEBEBEBEBEBF),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x9F1165E7254813E2),
        lo: L::from_bits(0x3FB6, 0xB2F392A409F1165E),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x9D89D89D89D89D8A),
        lo: L::from_bits(0xBFB6, 0x9D89D89D89D89D8A),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x9C09C09C09C09C0A),
        lo: L::from_bits(0xBFB6, 0xFD8FD8FD8FD8FD90),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x9A90E7D95BC609A9),
        lo: L::from_bits(0x3FB4, 0xE7D95BC609A90E7E),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x991F1A515885FB37),
        lo: L::from_bits(0x3FB3, 0xE5AEA77A04C8F8D3),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x97B425ED097B425F),
        lo: L::from_bits(0xBFB6, 0xBDA12F684BDA12F7),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x964FDA6C0964FDA7),
        lo: L::from_bits(0xBFB6, 0xFDA6C0964FDA6C09),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x94F2094F2094F209),
        lo: L::from_bits(0x3FB7, 0x9E4129E4129E412A),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x939A85C40939A85C),
        lo: L::from_bits(0x3FB7, 0x8127350B88127351),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x9249249249249249),
        lo: L::from_bits(0x3FB6, 0x9249249249249249),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x90FDBC090FDBC091),
        lo: L::from_bits(0xBFB2, 0x90FDBC090FDBC091),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8FB823EE08FB823F),
        lo: L::from_bits(0xBFB5, 0xFB823EE08FB823EE),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8E78356D1408E783),
        lo: L::from_bits(0x3FB7, 0xADA2811CF06ADA28),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8D3DCB08D3DCB08D),
        lo: L::from_bits(0x3FB6, 0xF72C234F72C234F7),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8C08C08C08C08C09),
        lo: L::from_bits(0xBFB6, 0xFDCFDCFDCFDCFDD0),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8AD8F2FBA9386823),
        lo: L::from_bits(0xBFB7, 0x9386822B63CBEEA5),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x89AE4089AE4089AE),
        lo: L::from_bits(0x3FB7, 0x81135C81135C8113),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8888888888888889),
        lo: L::from_bits(0xBFB7, 0xEEEEEEEEEEEEEEEF),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8767AB5F34E47EF1),
        lo: L::from_bits(0x3FB6, 0xC2A50658DC08767B),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x864B8A7DE6D1D608),
        lo: L::from_bits(0x3FB7, 0xC9714FBCDA3AC10D),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8534085340853408),
        lo: L::from_bits(0x3FB7, 0xA6810A6810A6810A),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8421084210842108),
        lo: L::from_bits(0x3FB7, 0x8421084210842108),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x83126E978D4FDF3B),
        lo: L::from_bits(0x3FB7, 0xC8B4395810624DD3),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8208208208208208),
        lo: L::from_bits(0x3FB6, 0x8208208208208208),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8102040810204081),
        lo: L::from_bits(0x3FB2, 0x8102040810204081),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
];
/// 1/k! for k = 0 to 37, as DD.
const INV_FACTORIALS: [DD; 38] = [
    DD {
        hi: L::from_bits(0x3FFF, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFF, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFE, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    },
    DD {
        hi: L::from_bits(0x3FFC, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFBB, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FFA, 0xAAAAAAAAAAAAAAAB),
        lo: L::from_bits(0xBFB9, 0xAAAAAAAAAAAAAAAB),
    },
    DD {
        hi: L::from_bits(0x3FF8, 0x8888888888888889),
        lo: L::from_bits(0xBFB7, 0xEEEEEEEEEEEEEEEF),
    },
    DD {
        hi: L::from_bits(0x3FF5, 0xB60B60B60B60B60B),
        lo: L::from_bits(0x3FB4, 0xC16C16C16C16C16C),
    },
    DD {
        hi: L::from_bits(0x3FF2, 0xD00D00D00D00D00D),
        lo: L::from_bits(0x3FAA, 0xD00D00D00D00D00D),
    },
    DD {
        hi: L::from_bits(0x3FEF, 0xD00D00D00D00D00D),
        lo: L::from_bits(0x3FA7, 0xD00D00D00D00D00D),
    },
    DD {
        hi: L::from_bits(0x3FEC, 0xB8EF1D2AB6399C7D),
        lo: L::from_bits(0x3FAB, 0xAC1C88E500171DE4),
    },
    DD {
        hi: L::from_bits(0x3FE9, 0x93F27DBBC4FAE397),
        lo: L::from_bits(0x3FA8, 0xF016D3EA6678E4B6),
    },
    DD {
        hi: L::from_bits(0x3FE5, 0xD7322B3FAA271C7F),
        lo: L::from_bits(0x3FA3, 0xE8FC9706FB8E3C40),
    },
    DD {
        hi: L::from_bits(0x3FE2, 0x8F76C77FC6C4BDAA),
        lo: L::from_bits(0x3FA0, 0x9B530F59FD097D80),
    },
    DD {
        hi: L::from_bits(0x3FDE, 0xB092309D43684BE5),
        lo: L::from_bits(0x3F9B, 0xE0CC748EBDA134ED),
    },
    DD {
        hi: L::from_bits(0x3FDA, 0xC9CBA54603E4E906),
        lo: L::from_bits(0xBF98, 0xA41D7440B8362AE6),
    },
    DD {
        hi: L::from_bits(0x3FD6, 0xD73F9F399DC0F88F),
        lo: L::from_bits(0xBF94, 0xF3529E22E6A02DC3),
    },
    DD {
        hi: L::from_bits(0x3FD2, 0xD73F9F399DC0F88F),
        lo: L::from_bits(0xBF90, 0xF3529E22E6A02DC3),
    },
    DD {
        hi: L::from_bits(0x3FCE, 0xCA963B81856A5359),
        lo: L::from_bits(0x3F8C, 0xC0A32EEE35FFD4EF),
    },
    DD {
        hi: L::from_bits(0x3FCA, 0xB413C31DCBECBBDE),
        lo: L::from_bits(0xBF89, 0xFFB7795D3D55687A),
    },
    DD {
        hi: L::from_bits(0x3FC6, 0x97A4DA340A0AB926),
        lo: L::from_bits(0x3F85, 0xA1EC3B7B9674B57F),
    },
    DD {
        hi: L::from_bits(0x3FC1, 0xF2A15D201011283D),
        lo: L::from_bits(0x3F80, 0x9CAD2BF8F0BABBFE),
    },
    DD {
        hi: L::from_bits(0x3FBD, 0xB8DC77B6E7AB8C5F),
        lo: L::from_bits(0x3F7C, 0xF146FCEE6E452185),
    },
    DD {
        hi: L::from_bits(0x3FB9, 0x8671CB6DBFC294A3),
        lo: L::from_bits(0xBF78, 0xF36F480CC7138A88),
    },
    DD {
        hi: L::from_bits(0x3FB4, 0xBB0DA098B1C0CECC),
        lo: L::from_bits(0xBF72, 0x8F1F645013B0CF65),
    },
    DD {
        hi: L::from_bits(0x3FAF, 0xF96780CB97ABBE65),
        lo: L::from_bits(0x3F6D, 0x9680CF953B1440CE),
    },
    DD {
        hi: L::from_bits(0x3FAB, 0x9F9E66E8B2FD46A7),
        lo: L::from_bits(0x3F69, 0x894832EEDE217128),
    },
    DD {
        hi: L::from_bits(0x3FA6, 0xC4742FE35272CD1C),
        lo: L::from_bits(0x3F65, 0xF2050BA6B0149467),
    },
    DD {
        hi: L::from_bits(0x3FA1, 0xE8D58E16E6751905),
        lo: L::from_bits(0x3F60, 0x9A18F15D427734A0),
    },
    DD {
        hi: L::from_bits(0x3F9D, 0x850C5131A842E9BA),
        lo: L::from_bits(0xBF5A, 0xE8EB8F2AD5CAF56D),
    },
    DD {
        hi: L::from_bits(0x3F98, 0x92CFCC5A1AC56BD6),
        lo: L::from_bits(0xBF54, 0xE78C44C876B714CD),
    },
    DD {
        hi: L::from_bits(0x3F93, 0x9C9962823EB07306),
        lo: L::from_bits(0x3F52, 0xADED4C2989C574B2),
    },
    DD {
        hi: L::from_bits(0x3F8E, 0xA1A6973C1FADE217),
        lo: L::from_bits(0x3F4A, 0xF7237D35FE1C89DB),
    },
    DD {
        hi: L::from_bits(0x3F89, 0xA1A6973C1FADE217),
        lo: L::from_bits(0x3F45, 0xF7237D35FE1C89DB),
    },
    DD {
        hi: L::from_bits(0x3F84, 0x9CC092A6E86A8DAA),
        lo: L::from_bits(0xBF42, 0xFA6400AD17BB055E),
    },
    DD {
        hi: L::from_bits(0x3F7F, 0x9388118E07EBD0A0),
        lo: L::from_bits(0xBF3D, 0xEBA96A0C5291E6EF),
    },
    DD {
        hi: L::from_bits(0x3F7A, 0x86E2CE38B6C8F942),
        lo: L::from_bits(0xBF39, 0xC380A581F9DC4C50),
    },
    DD {
        hi: L::from_bits(0x3F74, 0xEFCC194861654958),
        lo: L::from_bits(0x3F32, 0xD71A254E4EB7D438),
    },
    DD {
        hi: L::from_bits(0x3F6F, 0xCF6468E4A742D7A6),
        lo: L::from_bits(0x3F2D, 0xF162B87B13A5E7F9),
    },
];

/// The zeros of J0 below 48, and its Taylor coefficients about each.
const J0_ZEROS: &[Zero] = &[
    Zero {
        z: [
            L::from_bits(0x4000, 0x99E8A974B8D9FDE1),
            L::from_bits(0x3FBF, 0xB18A09769C69B910),
            L::from_bits(0x3F7A, 0xADE2C3804AEFEF2F),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFE, 0x84E6D9B2A8940359),
                lo: L::from_bits(0xBFBB, 0xCC3D6BAFA488DD70),
            },
            DD {
                hi: L::from_bits(0x3FFB, 0xDD0EF75014A49C96),
                lo: L::from_bits(0xBFB8, 0xB1160E907897F5AA),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xE7D74321B46B8379),
                lo: L::from_bits(0x3FB8, 0xC5E7EE3D3F6905A0),
            },
        ],
        d: [
            L::from_bits(0xBFF8, 0x8DD8E5F0D20389A8),
            L::from_bits(0xBFF6, 0x8FCC92C86895E7C1),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4001, 0xB0A47AD9617228BB),
            L::from_bits(0xBFC0, 0xFAB329F5AE904B1E),
            L::from_bits(0xBF7E, 0xA130DFED0E8ABAEF),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFD, 0xAE3730504BC11651),
                lo: L::from_bits(0xBFBA, 0xBFBC72C1A973590E),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xFC7B973D4247012C),
                lo: L::from_bits(0xBFB6, 0xCA05B2C9A04ACCF9),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0xD90A865A0F46066E),
                lo: L::from_bits(0x3FB8, 0x9A9A5A29750211B3),
            },
        ],
        d: [
            L::from_bits(0x3FF7, 0x97BFFF4812B5D615),
            L::from_bits(0x3FF6, 0x93F18FF4D4BBC78F),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0x8A75AB6666F64EAE),
            L::from_bits(0x3FC1, 0xD1F1D6707B5B0B88),
            L::from_bits(0x3F80, 0xA28B9E18B25C9739),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFD, 0x8AFBCBBD3B969DE9),
                lo: L::from_bits(0x3FB9, 0xA2B00D1DEF292D94),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0x807BFE78C1F0689C),
                lo: L::from_bits(0xBFB8, 0xE4B99BD53198366B),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xB45CC2763249DD65),
                lo: L::from_bits(0xBFB8, 0xBFCE676DC0B68BC7),
            },
        ],
        d: [
            L::from_bits(0xBFF6, 0xA4731B006C20BD7A),
            L::from_bits(0xBFF6, 0x8706B01C2D3780F7),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xBCAA20041395B144),
            L::from_bits(0x3FC1, 0x9FAB043AB6100AEC),
            L::from_bits(0x3F7E, 0xE820A878EB9A891E),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xEE09F3356173B693),
                lo: L::from_bits(0xBFBB, 0xE5A81CF21663ECB6),
            },
            DD {
                hi: L::from_bits(0xBFF8, 0xA17F866E2C23197A),
                lo: L::from_bits(0xBFAF, 0xB0AC9B8E2006AB5E),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0x9C68EEC4C97023D5),
                lo: L::from_bits(0x3FB4, 0x8AD3BD27CCE2DEF6),
            },
        ],
        d: [
            L::from_bits(0x3FF5, 0xD2AF4D9A376E0700),
            L::from_bits(0x3FF5, 0xF170B7CBE85405CA),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xEEE509F7938E8CD1),
            L::from_bits(0xBFC0, 0xB3049B274266D0C0),
            L::from_bits(0xBF7F, 0x838CD7AA30884731),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xD380E87CB3A7FFB8),
                lo: L::from_bits(0x3FB7, 0x84CB77639A33EB18),
            },
            DD {
                hi: L::from_bits(0x3FF7, 0xE2A5C987F7C48D9A),
                lo: L::from_bits(0xBFB5, 0xFD95FFAE13F4E6A2),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0x8BBCC5504F88FA85),
                lo: L::from_bits(0xBFB9, 0xFF2A9E24183D950F),
            },
        ],
        d: [
            L::from_bits(0xBFF5, 0x9510A8A03EE84B24),
            L::from_bits(0xBFF5, 0xDAA0FC14DFDA09AF),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0x909189FC50CFADD3),
            L::from_bits(0x3FC2, 0x80BBDC2E404C1C51),
            L::from_bits(0xBF7E, 0x9B58D95951068FB0),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xC03BFAB64DBC10D2),
                lo: L::from_bits(0x3FBB, 0xF96A2520BAD3E6AE),
            },
            DD {
                hi: L::from_bits(0xBFF7, 0xAA33F5A9AEF550CC),
                lo: L::from_bits(0x3FB2, 0x89A7802626D71672),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xFEBE1D6B7ACEFE79),
                lo: L::from_bits(0xBFB8, 0xFC058BC0AA80BF0F),
            },
        ],
        d: [
            L::from_bits(0x3FF4, 0xE0DA3E404E2F5A1C),
            L::from_bits(0x3FF5, 0xC8B363E9F553E2C5),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xA9B16E8B9FBC911D),
            L::from_bits(0x3FC1, 0xB7E870ED6A0F98F8),
            L::from_bits(0x3F7E, 0xC2A3F4B59ED25225),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xB16C9D54E82DDA89),
                lo: L::from_bits(0x3FBB, 0xCB09D94FFC28017E),
            },
            DD {
                hi: L::from_bits(0x3FF7, 0x85D4E744494F8CA1),
                lo: L::from_bits(0xBFB6, 0xE3DCA6FA043905DE),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xEB839ED75D815E0A),
                lo: L::from_bits(0xBFB8, 0x9DC470515296FFB5),
            },
        ],
        d: [
            L::from_bits(0xBFF4, 0xB1409E3FAC399695),
            L::from_bits(0xBFF5, 0xBA54A4682B1C7047),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xC2D1DC980AB6EA11),
            L::from_bits(0xBFC2, 0xE0E77FFA880A88AC),
            L::from_bits(0xBF81, 0xCD6818FA8BBF1315),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xA595175FB0F66C34),
                lo: L::from_bits(0x3FBB, 0x9B2EAC198732D051),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0xD994BFED73C80EC8),
                lo: L::from_bits(0xBFB5, 0x9FA854158DC0D3C9),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xDC082EACD88A5E15),
                lo: L::from_bits(0x3FB7, 0xD5298C4EECF36D81),
            },
        ],
        d: [
            L::from_bits(0x3FF4, 0x9051FC60950FF867),
            L::from_bits(0x3FF5, 0xAE8C6B4EF367D56E),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xDBF2A52FEAF88BA5),
            L::from_bits(0x3FC2, 0xCDC525CACA0A9E44),
            L::from_bits(0xBF81, 0xAC371738B55B64E2),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x9BD56460D755D66B),
                lo: L::from_bits(0xBFBB, 0xF58D2EE0C628EE2A),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0xB5606971797C3BC3),
                lo: L::from_bits(0x3FB5, 0xE61C55DB9C823769),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xCF3A73AA7538F43A),
                lo: L::from_bits(0x3FB8, 0xD81C3C86B6E5FFBA),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0xF0E02C4F195D867D),
            L::from_bits(0xBFF5, 0xA4B0AC6E2FBFF42F),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xF513AC8E5DF68EC9),
            L::from_bits(0x3FC2, 0x88A87C6C19DFE779),
            L::from_bits(0xBF81, 0xBF00349ECF9AB2D2),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x93A03EFD6F73679E),
                lo: L::from_bits(0xBFBB, 0xE38BE7F8F5D12A5E),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0x9A34A85FEC8F85CA),
                lo: L::from_bits(0x3FB5, 0xE663895838A5C2A6),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xC46A468EA75BD445),
                lo: L::from_bits(0xBFB8, 0xB376F060BCF441F1),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0xCCF3491D6D4911F0),
            L::from_bits(0x3FF5, 0x9C4C25BB66FA5259),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x871A709D337F31A3),
            L::from_bits(0x3FC2, 0x9B3D0C1E0E56CC10),
            L::from_bits(0x3F7E, 0xAAEB558EF346A074),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x8C9791E71F028414),
                lo: L::from_bits(0xBFB9, 0xB4F758902C67C77E),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0x853340C2E00D887C),
                lo: L::from_bits(0xBFB5, 0xB5423CFE3479CD17),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xBB20A0EB29044B3C),
                lo: L::from_bits(0xBFB7, 0xD024B7CAD90BE32F),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0xB1221BD17F3B4DEB),
            L::from_bits(0xBFF5, 0x950C25F06CC4892A),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x93AB1BD4B0CF5EF5),
            L::from_bits(0xBFC3, 0xC0D1F0C38DE2F143),
            L::from_bits(0x3F82, 0x83800E6695DB362A),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x8679F682CE2B993F),
                lo: L::from_bits(0x3FB8, 0x99A5F56DB35E0138),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xE92155294C98E3E8),
                lo: L::from_bits(0x3FB4, 0xD191FE014578321C),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xB309EC9D80C057E5),
                lo: L::from_bits(0x3FB8, 0xA892C54344844D57),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0x9B13F930FA88B3C0),
            L::from_bits(0x3FF5, 0x8EB4E506C41CA2FF),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xA03BD3F6B149CFB3),
            L::from_bits(0xBFC2, 0xC86F4E2793622D3A),
            L::from_bits(0x3F7F, 0x9BBE03257FA9DEF3),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x81185CBCBD3D9535),
                lo: L::from_bits(0xBFBB, 0xF1882912AD0EF698),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xCE4041D95BA9D0BF),
                lo: L::from_bits(0xBFB3, 0xDD6F3844680FBB39),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xABE9901DF7EC7C77),
                lo: L::from_bits(0x3FB8, 0xA93407AB921DC713),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0x893E5D114496F27C),
            L::from_bits(0xBFF5, 0x891A238A2AD35DE7),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xACCC9632E86C68F7),
            L::from_bits(0xBFC3, 0xB701660BA10ACA1C),
            L::from_bits(0xBF81, 0xF043B0C4B8C082FF),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFB, 0xF89FD79964705303),
                lo: L::from_bits(0x3FBA, 0xF7E26C2A2C5B8C7C),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xB82AC6EEDBEA31E9),
                lo: L::from_bits(0xBFB2, 0x82312B68EB1B2074),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xA5926BD499CB924E),
                lo: L::from_bits(0x3FB8, 0xA4AF1BDCB1166802),
            },
        ],
        d: [
            L::from_bits(0x3FF2, 0xF529510A43D086AC),
            L::from_bits(0x3FF5, 0x841A6C79FEEAD59A),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xB95D607C08407EFA),
            L::from_bits(0x3FC3, 0xB0BF5B5F4E1DB739),
            L::from_bits(0xBF81, 0xD30F3D355DAD5287),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFB, 0xF00C6CCFAED0DB49),
                lo: L::from_bits(0x3FBA, 0x96B132169872AECE),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xA5C2C4BD9B132900),
                lo: L::from_bits(0x3FB2, 0xF4E26E7C45AB1E57),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0x9FE2210A9A1AE5A5),
                lo: L::from_bits(0xBFB8, 0xD3BCE08650FD39D8),
            },
        ],
        d: [
            L::from_bits(0xBFF2, 0xDCB4A6B8A4371BE4),
            L::from_bits(0xBFF4, 0xFF37EE3226EEF3F2),
        ],
    },
];

/// The zeros of J1 below 48, and its Taylor coefficients about each.
const J1_ZEROS: &[Zero] = &[
    Zero {
        z: [
            L::from_bits(0x4000, 0xF53AABAD7B784540),
            L::from_bits(0xBFBE, 0xAAD4E8D92B1FB72B),
            L::from_bits(0xBF7B, 0xB547AAF3BA46B623),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFD, 0xCE367AC165FBF6D2),
                lo: L::from_bits(0xBFBA, 0xF75E44D2B4FA05F1),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xD7451CFA8D681D29),
                lo: L::from_bits(0xBFB7, 0xD73329AE52BA6200),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xDAC4E8ED09C82511),
                lo: L::from_bits(0x3FB8, 0xC134B3A5650E05B1),
            },
        ],
        d: [
            L::from_bits(0xBFF7, 0xA9BAA26198ED361A),
            L::from_bits(0xBFF6, 0x9259A04CAC831FDE),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4001, 0xE07FAF9DA3927F27),
            L::from_bits(0xBFBE, 0x9B67490E09C9E75A),
            L::from_bits(0xBF7D, 0xA3A76E2BF6BC438E),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFD, 0x99A8C59C3A74535E),
                lo: L::from_bits(0x3FBC, 0x8B40CFB8291E516A),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xAF386E301B15F93B),
                lo: L::from_bits(0xBFB8, 0x9AC5308E563BE02C),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0xC0641DEF772D82EE),
                lo: L::from_bits(0x3FB9, 0x9E3A8825F4DBFDCB),
            },
        ],
        d: [
            L::from_bits(0x3FF6, 0xCD2594971EF2127B),
            L::from_bits(0x3FF6, 0x89FDE3EB4C10B862),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xA2C68685EFE14A05),
            L::from_bits(0xBFC0, 0xF7AD754EA4DA9B9F),
            L::from_bits(0x3F7E, 0xFD742CDAEBD090B4),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xFFB2A2A275E68905),
                lo: L::from_bits(0xBFBA, 0xA8E8950715AA1B3D),
            },
            DD {
                hi: L::from_bits(0x3FF8, 0xC911FF9603C2D772),
                lo: L::from_bits(0x3FB7, 0xBFD2BD15AFD72ABE),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xA5862EAED33C46DC),
                lo: L::from_bits(0x3FB9, 0xBB154DEAF42754EB),
            },
        ],
        d: [
            L::from_bits(0xBFF5, 0xFC8D4F706944B7D8),
            L::from_bits(0xBFF5, 0xFA8E1244DCF37478),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xD52DD798872D112C),
            L::from_bits(0xBFC0, 0xC600B9C65B2E30BC),
            L::from_bits(0xBF7F, 0xEB669AB57B76D43F),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xDF999BC39D3EC392),
                lo: L::from_bits(0xBFBB, 0xE64B46F7E770C7F6),
            },
            DD {
                hi: L::from_bits(0xBFF8, 0x8641D16BD6E99543),
                lo: L::from_bits(0x3FB2, 0xE25D542745159930),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0x928C2C008C0B557D),
                lo: L::from_bits(0x3FB8, 0x93496BE4ED7E40BE),
            },
        ],
        d: [
            L::from_bits(0x3FF5, 0xACF58B05FB96C082),
            L::from_bits(0x3FF5, 0xE2DE719D796BB80F),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0x83C3D9B02846245B),
            L::from_bits(0xBFC2, 0x9A3A22ACC5D43927),
            L::from_bits(0xBF7F, 0xB4463A571E73B4BA),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xC92E37E5047AA42D),
                lo: L::from_bits(0xBFBB, 0xAA0CCBA3F05C31E2),
            },
            DD {
                hi: L::from_bits(0x3FF7, 0xC36E99702CD87192),
                lo: L::from_bits(0xBFB6, 0x8212DB8CA39C12EE),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0x84A31DDE81B3F9DB),
                lo: L::from_bits(0xBFB8, 0xF93B9D11AE6C7638),
            },
        ],
        d: [
            L::from_bits(0xBFF4, 0xFED014C643B455BA),
            L::from_bits(0xBFF5, 0xCFA5F303AC7D8A03),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0x9CED473A0B651DBD),
            L::from_bits(0xBFBE, 0xC065FA7A17CC3A7C),
            L::from_bits(0x3F7D, 0xBCA91D17436D97F5),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xB86288913EAD525B),
                lo: L::from_bits(0x3FBB, 0x8015FE134EE5BE25),
            },
            DD {
                hi: L::from_bits(0xBFF7, 0x9665874BAAC6D0DB),
                lo: L::from_bits(0x3FB6, 0x8B1AF9308B2686C8),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xF3EE0473874CCE1A),
                lo: L::from_bits(0x3FB6, 0x93BEF6FB828DC91F),
            },
        ],
        d: [
            L::from_bits(0x3FF4, 0xC566E2D82C60702F),
            L::from_bits(0x3FF5, 0xC0281B9256984D98),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xB614A71EA6C55EE4),
            L::from_bits(0x3FBF, 0xCC5010770EE52FEF),
            L::from_bits(0x3F7D, 0x80EE791352BC1BB6),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xAB32709DB8310CB7),
                lo: L::from_bits(0xBFB8, 0xA97B7CAC16688D25),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0xF0B2AAF0846E329B),
                lo: L::from_bits(0xBFB2, 0xE7B56177CD48225E),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xE2F0D6CFD97A00FF),
                lo: L::from_bits(0x3FB7, 0xEB5FAD9C1F3A5469),
            },
        ],
        d: [
            L::from_bits(0xBFF4, 0x9E9B4FCAC72B4DED),
            L::from_bits(0xBFF5, 0xB37A7613D4B74A7E),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xCF3AB86E7508311A),
            L::from_bits(0x3FC1, 0x9A1920362081F459),
            L::from_bits(0x3F7D, 0xC173C5BCA9403F3C),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xA07C83C9B02DDA63),
                lo: L::from_bits(0xBFBA, 0x838016B5A20F4B3D),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0xC641983BFDF57050),
                lo: L::from_bits(0xBFB5, 0x8D69A8736EB2FAE8),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xD50670210E8D3FAC),
                lo: L::from_bits(0x3FB8, 0xF5B784ED8D00A59A),
            },
        ],
        d: [
            L::from_bits(0x3FF4, 0x82FD2CC77AE8E47E),
            L::from_bits(0x3FF5, 0xA8E986BC55A933E4),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xE85FE7A38FE65F5F),
            L::from_bits(0xBFC2, 0xCE39EC976FAB24F5),
            L::from_bits(0x3F80, 0xED5F1616077AD08C),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x979039731C679CEC),
                lo: L::from_bits(0xBFBB, 0xB82DF9E8E092D00D),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0xA6F9045DEA278BE9),
                lo: L::from_bits(0x3FB1, 0x95CCCEB33078E86D),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xC95DAF0F0ACFDE7D),
                lo: L::from_bits(0x3FB7, 0xECC3FB461578CFF7),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0xDD0C0E0344BE6753),
            L::from_bits(0xBFF5, 0x9FF4EADD5251E9A6),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x80C23B73595F7E42),
            L::from_bits(0xBFC2, 0xE873419AD989D272),
            L::from_bits(0xBF81, 0xF34767F8519F99C6),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x8FFAF763500E6920),
                lo: L::from_bits(0xBFBB, 0xC9D2F9A193B3871E),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0x8F21C5B9161DABF1),
                lo: L::from_bits(0xBFB4, 0xE7068E5C1B29D530),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xBF6AFFFE0E3B9CCB),
                lo: L::from_bits(0xBFB8, 0xD21304A918D8461B),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0xBDBCCBDD5ECE4C79),
            L::from_bits(0x3FF5, 0x9840EF7CB09626C6),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x8D54486E2F4BDC54),
            L::from_bits(0x3FC0, 0x8801F75F300EABDA),
            L::from_bits(0x3F7F, 0xBC69D984194C0516),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x896EABDF8C56CC32),
                lo: L::from_bits(0x3FBA, 0xE2415F33DEAED28F),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xF8F0F3F9C9F41665),
                lo: L::from_bits(0x3FB4, 0xEAFB076BA4CD5C60),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xB6CD7F44180FD251),
                lo: L::from_bits(0xBFB7, 0x8366A0AEA714437A),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0xA529C52414BCDA6C),
            L::from_bits(0xBFF5, 0x918B1285A571B608),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x99E6291EAE5B48CF),
            L::from_bits(0x3FC3, 0xAFECA0CA4FC9A167),
            L::from_bits(0xBF82, 0xA0A33B229AFD7513),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x83B41366160C8A34),
                lo: L::from_bits(0x3FBA, 0x9F17F72B43B48EDD),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xDB1442F00386306E),
                lo: L::from_bits(0x3FB2, 0xEC00933013F8BF98),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xAF3FA9800F258545),
                lo: L::from_bits(0x3FB8, 0xFA75C8430B8943A6),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0x9175F5C6E1100C49),
            L::from_bits(0x3FF5, 0x8BA2253D02667D1B),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xA677E78B9A5B10A4),
            L::from_bits(0x3FC3, 0x9BD556E5109CE3AE),
            L::from_bits(0x3F81, 0xE4AA39352D7A9E60),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFB, 0xFD45A0B88E41CC50),
                lo: L::from_bits(0xBFBA, 0xCEFA3A3A980CAF1B),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xC2BE9CB4CCBE8AC4),
                lo: L::from_bits(0x3FB3, 0xCD24512DD1D8B065),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xA88E36D6D5508CD4),
                lo: L::from_bits(0x3FB8, 0xCC32C0DE91449BB8),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0x816144EDEDEA75D8),
            L::from_bits(0xBFF5, 0x866111C6914FC8F4),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xB3098AEB5899FA9F),
            L::from_bits(0xBFC1, 0xE90CE8626FDF19E9),
            L::from_bits(0x3F80, 0x82D911FCEFF2D99F),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFB, 0xF4393ED51ED76810),
                lo: L::from_bits(0x3FB9, 0x82D0059652B09AD4),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xAE9A9F142A51B9A5),
                lo: L::from_bits(0x3FB3, 0x9E3279E1DDB73E74),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xA2926A409E612594),
                lo: L::from_bits(0x3FB7, 0xC1B6299289E9AFD6),
            },
        ],
        d: [
            L::from_bits(0x3FF2, 0xE81BABA7141B7E90),
            L::from_bits(0x3FF5, 0x81AB5DBA3D3B1AFE),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xBF9B18901456AC19),
            L::from_bits(0xBFC2, 0xA854216DBC7AB571),
            L::from_bits(0xBF80, 0xB1A2F283324F7DB0),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFB, 0xEC149D52AE8C7B96),
                lo: L::from_bits(0xBFB9, 0xF944CBE51C415E1E),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0x9DB5F5C1F92CAF79),
                lo: L::from_bits(0xBFB0, 0xB711C0F05BA8AC6B),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0x9D2E65E09533DAD8),
                lo: L::from_bits(0x3FB6, 0x91C5E2BD3A923052),
            },
        ],
        d: [
            L::from_bits(0xBFF2, 0xD1BB2E93552154FF),
            L::from_bits(0xBFF4, 0xFAD599BA4610A75D),
        ],
    },
];

/// The zeros of Y0 below 48, and its Taylor coefficients about each.
const Y0_ZEROS: &[Zero] = &[
    Zero {
        z: [
            L::from_bits(0x3FFE, 0xE4C175C6A0BF51EB),
            L::from_bits(0xBFBD, 0xC5B1F9700FA2585F),
            L::from_bits(0x3F7C, 0xCA45997853F10787),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFE, 0xE121B8C225C44EDE),
                lo: L::from_bits(0xBFBD, 0xB93745E6ED70C935),
            },
            DD {
                hi: L::from_bits(0xBFFD, 0xFBF1C5236B881051),
                lo: L::from_bits(0xBFBC, 0x81BD718EB508C0DF),
            },
            DD {
                hi: L::from_bits(0x3FFC, 0xE1D899C579F4B7AE),
                lo: L::from_bits(0xBFBB, 0x9012F4CE6DA20F93),
            },
        ],
        d: [
            L::from_bits(0xBFFC, 0xE78C735259C4079B),
            L::from_bits(0x3FFC, 0xE034043614B75D54),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4000, 0xFD4A9A6CC2B4DE10),
            L::from_bits(0xBFBF, 0xD5CF008709B97F4B),
            L::from_bits(0xBF7E, 0x8C4314AE10C6A280),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFD, 0xCE1A12B5095060D2),
                lo: L::from_bits(0xBFB6, 0x9B90DB134AC93FB1),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xD04E49481B3F74C4),
                lo: L::from_bits(0x3FB8, 0x948BEE48169BF666),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xEFB6ACDFA875E756),
                lo: L::from_bits(0x3FB8, 0x9FA5AB5859F821A5),
            },
        ],
        d: [
            L::from_bits(0xBFF7, 0xE08B7EE2CC04B31B),
            L::from_bits(0xBFF6, 0x8F195E277C5206BF),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4001, 0xE2C0EE2739081734),
            L::from_bits(0x3FC0, 0x8B5B6A0A93103598),
            L::from_bits(0x3F7F, 0xAE73E453456294CF),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFD, 0x99A665034BD2D4E5),
                lo: L::from_bits(0x3FBC, 0xEDED20CB566AEC28),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xAD77B08FE26ABA84),
                lo: L::from_bits(0x3FB8, 0xE64DE4B050A8014F),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0xC4B4E3265FA29292),
                lo: L::from_bits(0xBFB9, 0xA6D0EAADFEC5F787),
            },
        ],
        d: [
            L::from_bits(0x3FF6, 0xD978A54AA93E59AB),
            L::from_bits(0x3FF6, 0x8E9AF42FEF151B55),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xA38EB9AD23EABC69),
            L::from_bits(0x3FC1, 0xD8021B8EDADED5A7),
            L::from_bits(0xBF80, 0x935DAB09E5CF395F),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xFFB1AE6395CF85C3),
                lo: L::from_bits(0x3FB9, 0xF568F8C6B93DF81B),
            },
            DD {
                hi: L::from_bits(0x3FF8, 0xC81B228FFABE28FD),
                lo: L::from_bits(0x3FB6, 0xDD7D16EB0C3B1FCC),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xA7333D38AAB57AC7),
                lo: L::from_bits(0xBFB9, 0xE90C30E920CB2439),
            },
        ],
        d: [
            L::from_bits(0xBFF6, 0x8192F720F4885D34),
            L::from_bits(0xBFF5, 0xFF11C8A7DC893C43),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xD5C70E250F3A4D04),
            L::from_bits(0x3FBE, 0xFC42F21B652C2469),
            L::from_bits(0x3F7D, 0x8E8E49B748450BE4),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xDF99513ACA00388A),
                lo: L::from_bits(0xBFBA, 0xCE6A6AB9A5729141),
            },
            DD {
                hi: L::from_bits(0xBFF8, 0x85E16C2732909F87),
                lo: L::from_bits(0x3FB7, 0xE7257B9258B561EC),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0x936559C5459B3CA6),
                lo: L::from_bits(0xBFB9, 0xABADC52058A40A85),
            },
        ],
        d: [
            L::from_bits(0x3FF5, 0xAF81F238B2EB9357),
            L::from_bits(0x3FF5, 0xE5553B71A4C967AF),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0x8401E3A00190A24B),
            L::from_bits(0x3FC2, 0x88DF448566AED80E),
            L::from_bits(0xBF81, 0xF60817D8909D38FF),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xC92E1ACC47714AB7),
                lo: L::from_bits(0xBFBA, 0xE1CF2138CED60574),
            },
            DD {
                hi: L::from_bits(0x3FF7, 0xC312A49B3AC8EFC5),
                lo: L::from_bits(0xBFB6, 0xA701622F1F3093F7),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0x85228901CEB51343),
                lo: L::from_bits(0xBFB9, 0xF44A56AB7851C6AE),
            },
        ],
        d: [
            L::from_bits(0xBFF5, 0x809D9C67DC948E6B),
            L::from_bits(0xBFF5, 0xD125090AFB3421A3),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0x9D2166FAFF5EB4E8),
            L::from_bits(0x3FBF, 0xDA55C08A2AD07A7F),
            L::from_bits(0x3F7A, 0xD26F694471901B8C),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xB8627B3655A3FAA0),
                lo: L::from_bits(0xBFBA, 0x85605BB60D4820A6),
            },
            DD {
                hi: L::from_bits(0xBFF7, 0x963398838F49B3C0),
                lo: L::from_bits(0xBFB6, 0xC9DA0C63935E6AB6),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xF4925C2D0B9B0608),
                lo: L::from_bits(0x3FB4, 0xD643792AEDECBBF1),
            },
        ],
        d: [
            L::from_bits(0x3FF4, 0xC6B61B26C9040DD8),
            L::from_bits(0x3FF5, 0xC123D816B587B2B9),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xB64197EBBD603B95),
            L::from_bits(0xBFC0, 0xAF7F30AC57C76F7A),
            L::from_bits(0xBF7F, 0x9F36D89C373A5C33),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xAB3269BE1BEBD602),
                lo: L::from_bits(0x3FBA, 0x840FDC522EDA78C6),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0xF0774764232CCCBE),
                lo: L::from_bits(0x3FB2, 0x882E860CB4224996),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xE3620AE4B8DA54F9),
                lo: L::from_bits(0xBFB8, 0x9DF41D2F429793D0),
            },
        ],
        d: [
            L::from_bits(0xBFF4, 0x9F624E5CA0FAA842),
            L::from_bits(0xBFF5, 0xB429D96823C4243D),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xCF62379F400A2F7E),
            L::from_bits(0x3FBF, 0xFD6A131FA8945EFF),
            L::from_bits(0xBF7E, 0xBF5399A7212A2E16),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xA07C7FEF84D2F9F3),
                lo: L::from_bits(0x3FBB, 0x8E9FF5A7FAED18D5),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0xC61BD14E22C37571),
                lo: L::from_bits(0x3FB0, 0x85CBC95897E32674),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xD5584C98A5904A91),
                lo: L::from_bits(0x3FB8, 0xE38CE208075ECCC8),
            },
        ],
        d: [
            L::from_bits(0x3FF4, 0x837B9AE607B532D5),
            L::from_bits(0x3FF5, 0xA9694D03401701BE),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xE883224B0B627A15),
            L::from_bits(0x3FC2, 0xAC5FB29D2E2A6FB6),
            L::from_bits(0xBF80, 0xED6623F448093F37),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x97903724C84E3969),
                lo: L::from_bits(0xBFBB, 0x8F2C88D0D80FD163),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0xA6DFB54FDC06BBF5),
                lo: L::from_bits(0x3FB5, 0xC6E0F51815D4439F),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xC99B22198C7685A5),
                lo: L::from_bits(0xBFB7, 0x8A0BCA736DD0AA0F),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0xDDB551EA74F3C21B),
            L::from_bits(0xBFF5, 0xA05552ECA5EC6642),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x80D22107255F73A6),
            L::from_bits(0xBFC1, 0xE7557570D13B68E9),
            L::from_bits(0xBF80, 0xA215A0399030D900),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x8FFAF5EEE9E1CC84),
                lo: L::from_bits(0xBFBB, 0x80A69D4ACC3D60F9),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0x8F101A992321E297),
                lo: L::from_bits(0xBFB5, 0xE94B817BD59EE7FB),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xBF9A836A50918B48),
                lo: L::from_bits(0xBFB8, 0xB56F2B4813AE47E4),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0xBE32E4981629D4E4),
            L::from_bits(0x3FF5, 0x988BC0B19A8A8411),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x8D62C464A213C7CC),
            L::from_bits(0x3FC3, 0x8BAA1FF24BBD7F48),
            L::from_bits(0x3F82, 0x9D66C2E139ADD0EE),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x896EAAEA5F1598F4),
                lo: L::from_bits(0x3FBA, 0xAB649AB0EEC8D1C3),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xF8D7718C0C68C525),
                lo: L::from_bits(0xBFB4, 0x97E9F25A2C7A4A7C),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xB6F3212154187DCC),
                lo: L::from_bits(0x3FB7, 0xDE15078715A3DF08),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0xA57EFC4FE530D2BD),
            L::from_bits(0xBFF5, 0x91C67E09D63B8735),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x99F3767AE59104C8),
            L::from_bits(0xBFC3, 0xF750F96455EBEC8C),
            L::from_bits(0xBF81, 0xE78BAA961A85C34E),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x83B412BED6AB4D79),
                lo: L::from_bits(0x3FBB, 0xB00A4EF438A3F15E),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xDB0153DF55523D8B),
                lo: L::from_bits(0xBFB4, 0xF9E48C00EDADEE61),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xAF5E1177E8495F4B),
                lo: L::from_bits(0x3FB8, 0xAEF36D826D7D0FD0),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0x91B53020A17307F5),
            L::from_bits(0x3FF5, 0x8BD2417D546C29DE),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xA68433F6109F9516),
            L::from_bits(0xBFC3, 0xEDFF354942FE2CBF),
            L::from_bits(0xBF81, 0xA6ABDA1A0D18B730),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFB, 0xFD459FCD721BA7DF),
                lo: L::from_bits(0xBFBA, 0xFA1EE2268F9C7F46),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xC2B039DBFD166783),
                lo: L::from_bits(0x3FB3, 0xCB4C9E6A99332E82),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xA8A732975974B2CD),
                lo: L::from_bits(0xBFB8, 0xCE59F25D39F2B53E),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0x81914C38C750D0CA),
            L::from_bits(0xBFF5, 0x8688A9FEF218D441),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xB314FA70F0190B86),
            L::from_bits(0xBFC2, 0xA488E0D9C7EF4FA5),
            L::from_bits(0x3F80, 0xFF78716A721D45E1),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFB, 0xF4393E2B951612D5),
                lo: L::from_bits(0xBFB6, 0xF449C15EEF56F075),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xAE8F7849559CA442),
                lo: L::from_bits(0xBFB3, 0xE18FDA6163AF5956),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xA2A73D39CAB1AEF9),
                lo: L::from_bits(0xBFB7, 0x8977C515E16F0800),
            },
        ],
        d: [
            L::from_bits(0x3FF2, 0xE8661D3FD369E99A),
            L::from_bits(0x3FF5, 0x81CC6965E816F799),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xBFA5C826E4D297CA),
            L::from_bits(0x3FBF, 0x845ECC4AB691CED7),
            L::from_bits(0x3F7E, 0xC6B4DC901F5D62E5),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFB, 0xEC149CD5B119C554),
                lo: L::from_bits(0xBFBA, 0xB6079D48ACDD0648),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0x9DAD2A422B3876A1),
                lo: L::from_bits(0xBFB3, 0x815699C4C5F53353),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0x9D3FF8B116F7BD9C),
                lo: L::from_bits(0x3FB7, 0x8FA85DAA37AB5D59),
            },
        ],
        d: [
            L::from_bits(0xBFF2, 0xD1F5E23B53036920),
            L::from_bits(0xBFF4, 0xFB0D6EF1D463085F),
        ],
    },
];

/// The zeros of Y1 below 48, and its Taylor coefficients about each.
const Y1_ZEROS: &[Zero] = &[
    Zero {
        z: [
            L::from_bits(0x4000, 0x8C9DF6A6FF921721),
            L::from_bits(0x3FBF, 0xE1AF2DE6402FC2AB),
            L::from_bits(0xBF7E, 0xF3A4CEFD8F3390EE),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFE, 0x85524221780A56B7),
                lo: L::from_bits(0xBFBC, 0xD9F13016AA7B7CC4),
            },
            DD {
                hi: L::from_bits(0xBFFB, 0xF2B7C110BDC78B1D),
                lo: L::from_bits(0x3FBA, 0xB5DD10280726D83B),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0x86957A74991C2D4A),
                lo: L::from_bits(0xBFB9, 0x8C307F73C48C48E8),
            },
        ],
        d: [
            L::from_bits(0xBFF7, 0x9D36F61B9485535E),
            L::from_bits(0x3FF7, 0xF338E3E88CB7E868),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4001, 0xADBFF2743D8173C0),
            L::from_bits(0xBFBF, 0xC229EEB8BA088FBA),
            L::from_bits(0x3F7E, 0xE33F70D05772F37B),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFD, 0xAE3E2AB7860CCC8E),
                lo: L::from_bits(0xBFBC, 0xB578EDA5FE15AF38),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0x805CFC2B8E50F68F),
                lo: L::from_bits(0x3FB8, 0xF9C00126789CAC75),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0xD0AEC96FF1F13438),
                lo: L::from_bits(0xBFB9, 0xA270EB2E89CF02D7),
            },
        ],
        d: [
            L::from_bits(0xBFF7, 0x885194F1611FD0AD),
            L::from_bits(0xBFF6, 0x8DF36DCC91D67AA2),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0x89893D730B4DA148),
            L::from_bits(0xBFC1, 0xC67F2E4DF732414A),
            L::from_bits(0x3F80, 0x9367E7AF9DF093E0),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFD, 0x8AFCC9FE755AE11E),
                lo: L::from_bits(0xBFBB, 0xE6356B0E9C08937D),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0x8159C99E790D88A3),
                lo: L::from_bits(0x3FB8, 0xA55824EB0C79C583),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0xB1CAEFF24FE69D78),
                lo: L::from_bits(0xBFB8, 0x90CDA43E9365FF26),
            },
        ],
        d: [
            L::from_bits(0x3FF6, 0x9E769517348C0088),
            L::from_bits(0x3FF6, 0x83D33C6B0005DBE6),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xBBFC89C6A1903022),
            L::from_bits(0xBFBD, 0xE1E4C7E7DFF652A2),
            L::from_bits(0xBF77, 0xA2681D25CCD2CECA),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xEE0A750A744FC6B3),
                lo: L::from_bits(0x3FB9, 0xA63BDE0B486DFC6D),
            },
            DD {
                hi: L::from_bits(0x3FF8, 0xA214FF7ADAFDEA68),
                lo: L::from_bits(0x3FB5, 0xBD04E7DF498D0F7F),
            },
            DD {
                hi: L::from_bits(0x3FFA, 0x9B3EBEB04725D3E3),
                lo: L::from_bits(0x3FB8, 0xD47AD89BCECCA0F8),
            },
        ],
        d: [
            L::from_bits(0xBFF5, 0xCEB7595E24F1AA9D),
            L::from_bits(0xBFF5, 0xEE27CC8D9EDC3013),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4002, 0xEE5BEC46F4245544),
            L::from_bits(0xBFBF, 0x91A50F8E04F1E7EB),
            L::from_bits(0xBF7E, 0xE1950F6CBDF3B020),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xD38115F0426CC97C),
                lo: L::from_bits(0xBFBB, 0xBDA65E06DC23FF8E),
            },
            DD {
                hi: L::from_bits(0xBFF7, 0xE3285B5C1884D2E6),
                lo: L::from_bits(0xBFB6, 0xD1567144E2EC758C),
            },
            DD {
                hi: L::from_bits(0xBFFA, 0x8B18C8E1855313A2),
                lo: L::from_bits(0x3FB6, 0xE1931D9DBFD8E097),
            },
        ],
        d: [
            L::from_bits(0x3FF5, 0x935822943EEE503A),
            L::from_bits(0x3FF5, 0xD8BB0142055FA509),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0x9058E34AF8F1D4BC),
            L::from_bits(0x3FC1, 0x8D9D5BC0910E8B45),
            L::from_bits(0xBF7F, 0xD22916D70C529EDC),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xC03C0E1921173A7B),
                lo: L::from_bits(0xBFBB, 0xDA3159CA0EBF1EAD),
            },
            DD {
                hi: L::from_bits(0x3FF7, 0xAA76D34BD004BC81),
                lo: L::from_bits(0xBFB2, 0xE41BF5430464D408),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xFDF36FC20423F932),
                lo: L::from_bits(0x3FB8, 0x83353A80BB655D0B),
            },
        ],
        d: [
            L::from_bits(0xBFF4, 0xDF18C6B093B706A6),
            L::from_bits(0xBFF5, 0xC77F7204A1BCE163),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xA9812A490C466A72),
            L::from_bits(0x3FC1, 0xD8A20829475C9A98),
            L::from_bits(0xBF80, 0xF94B6986EF239646),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0xB16CA6CBF42CDC5A),
                lo: L::from_bits(0x3FB9, 0xFEFA0C3184447D5F),
            },
            DD {
                hi: L::from_bits(0xBFF7, 0x85FB0A403819DFDE),
                lo: L::from_bits(0x3FB4, 0xECE6E29B0838FBC0),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xEAFC2BD153083AE0),
                lo: L::from_bits(0xBFB7, 0x983AD0337C4C2842),
            },
        ],
        d: [
            L::from_bits(0x3FF4, 0xB040D85BFF2B9292),
            L::from_bits(0x3FF5, 0xB983D81F1247C39C),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xC2A7D181C10650A9),
            L::from_bits(0x3FC2, 0xF75F025B20567C8B),
            L::from_bits(0x3F80, 0xA387F2252248CD31),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0xA5951C78D5CDA1E9),
                lo: L::from_bits(0xBFB8, 0x88E9FF2518894FC2),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0xD9C3C556F59A6966),
                lo: L::from_bits(0xBFB5, 0xFCED138368EC7924),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xDBA86C4D4D9AF55B),
                lo: L::from_bits(0x3FB8, 0x8061A077B680E44E),
            },
        ],
        d: [
            L::from_bits(0xBFF4, 0x8FB488B92D4AAFB5),
            L::from_bits(0xBFF5, 0xADF7737EA8E451D1),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xDBCD677467DBE718),
            L::from_bits(0x3FC2, 0xECF0415C4DBA3E87),
            L::from_bits(0x3F81, 0xE98D2A02A76E7A2B),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x9BD567564C3DC8CD),
                lo: L::from_bits(0xBFBB, 0x887D7A65FF3241A1),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0xB57F27F05E07BB3C),
                lo: L::from_bits(0x3FB4, 0xF4C27858DD56B3F8),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xCEF3D19DE1D4BBB2),
                lo: L::from_bits(0xBFB7, 0xF4050294DBD4BEFD),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0xF0127AB3D6243B59),
            L::from_bits(0x3FF5, 0xA4421E21355EFD4A),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4003, 0xF4F240302941DFC3),
            L::from_bits(0x3FBF, 0xB11F7053D347845C),
            L::from_bits(0x3F7E, 0xF23ACD5C50835523),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x93A040CF8E554C92),
                lo: L::from_bits(0x3FBB, 0xC9B8FEEA9BE518DD),
            },
            DD {
                hi: L::from_bits(0x3FF6, 0x9A49B4EE3C05D7F1),
                lo: L::from_bits(0x3FB4, 0xDC5CD2D5C9C4CDE5),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xC4346BA00DF97111),
                lo: L::from_bits(0x3FB7, 0x9274D9D765E37816),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0xCC668DF5F0A22792),
            L::from_bits(0xBFF5, 0x9BF77CD56F770FF8),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x870B483FC7DAACD3),
            L::from_bits(0xBFC3, 0xFAAFBDFB3D0185F6),
            L::from_bits(0xBF81, 0x9468C47D72099F23),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x8C979313D3A7177C),
                lo: L::from_bits(0xBFB8, 0xE15285693D9F65E4),
            },
            DD {
                hi: L::from_bits(0xBFF6, 0x85423541FF678D3B),
                lo: L::from_bits(0xBFB4, 0xB8D37492F681DEDE),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xBAF67555FBFC30DF),
                lo: L::from_bits(0xBFB7, 0x869C61AF4C70D53D),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0xB0BE2C0DF1AD88E1),
            L::from_bits(0x3FF5, 0x94C9A5BD422417DF),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0x939D3D9AD3D7F925),
            L::from_bits(0xBFC3, 0xFC2F0A40351AF93F),
            L::from_bits(0xBF7E, 0x9F50E42DCE00945D),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFC, 0x8679F74C7BB4D5E8),
                lo: L::from_bits(0x3FB8, 0xDC50C1D0D021D1A5),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xE9373D7928FBC47A),
                lo: L::from_bits(0x3FB4, 0xF1BF3607C2425298),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xB2E82CA454A34CD8),
                lo: L::from_bits(0xBFB8, 0x897B42266547931D),
            },
        ],
        d: [
            L::from_bits(0xBFF3, 0x9ACACDC2417201B3),
            L::from_bits(0xBFF5, 0x8E7F8BAE82E152E9),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xA02F0C1C9D7DA9A9),
            L::from_bits(0xBFBB, 0xA6AA6F73E3FC9686),
            L::from_bits(0xBF78, 0xA6DB49558F701F2C),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFC, 0x81185D4879438A23),
                lo: L::from_bits(0xBFBA, 0xCA433534C31F5DD0),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xCE50B7864B9A6C91),
                lo: L::from_bits(0xBFB4, 0xE53A35FD1F4F4F6A),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0xABCE0DEDE7E4CAEB),
                lo: L::from_bits(0xBFB8, 0xE67F2B83E916C2C1),
            },
        ],
        d: [
            L::from_bits(0x3FF3, 0x890767D62E00B912),
            L::from_bits(0x3FF5, 0x88EE935DF14A2F65),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xACC0BC3EB346D880),
            L::from_bits(0xBFC3, 0xDCE0298E9F2303C3),
            L::from_bits(0x3F81, 0xE2DD9264F190C5C8),
        ],
        c: [
            DD {
                hi: L::from_bits(0xBFFB, 0xF89FD860737E65E5),
                lo: L::from_bits(0x3FBA, 0xCB14188FAFED602E),
            },
            DD {
                hi: L::from_bits(0x3FF5, 0xB83769EC9AE802C8),
                lo: L::from_bits(0xBFB0, 0xABC46920F2A52787),
            },
            DD {
                hi: L::from_bits(0x3FF9, 0xA57BA65EBBDF7B50),
                lo: L::from_bits(0xBFB8, 0xDB84196097ADBF9C),
            },
        ],
        d: [
            L::from_bits(0xBFF2, 0xF4D4F3372D3C90E2),
            L::from_bits(0xBFF5, 0x83F64F6C127E5595),
        ],
    },
    Zero {
        z: [
            L::from_bits(0x4004, 0xB952542E618BEDB4),
            L::from_bits(0xBFC3, 0xE43C97C072AB6BC2),
            L::from_bits(0x3F80, 0x91B607A3C8991905),
        ],
        c: [
            DD {
                hi: L::from_bits(0x3FFB, 0xF00C6D60E0BF1723),
                lo: L::from_bits(0x3FB6, 0xE2ECF868508AB14C),
            },
            DD {
                hi: L::from_bits(0xBFF5, 0xA5CCA6E82E0FD6F9),
                lo: L::from_bits(0x3FB3, 0x9E2699B3C2F2E3AD),
            },
            DD {
                hi: L::from_bits(0xBFF9, 0x9FCF06D83F3F7A71),
                lo: L::from_bits(0xBFB8, 0xD3C34E90441F726D),
            },
        ],
        d: [
            L::from_bits(0x3FF2, 0xDC72ADBAD89D5796),
            L::from_bits(0x3FF4, 0xFEFB453C5D9E8EDB),
        ],
    },
];

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use crate::fenv::{
        FE_DIVBYZERO, FE_DOWNWARD, FE_INVALID, FE_OVERFLOW, FE_TONEAREST, FE_TOWARDZERO, FE_UPWARD,
        feclearexcept, fesetround, fetestexcept,
    };
    use std::collections::HashMap;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// mpmath's values, correctly rounded to nearest, and on which side of
    /// each the exact value lies (`posix/tools/oracle/besl_tables.py`).
    const ORACLE: &str = include_str!("besl_oracle.txt");
    /// glibc 2.39's answers at the special and extreme arguments, in the four
    /// directions (`posix/tools/oracle/besl_harness.py`).
    const GLIBC: &str = include_str!("besl_glibc.txt");

    const FIVE: i32 = FE_INVALID | FE_DIVBYZERO | FE_OVERFLOW | FE_UNDERFLOW | FE_INEXACT;
    const MODES: [(char, i32); 4] = [
        ('n', FE_TONEAREST),
        ('u', FE_UPWARD),
        ('d', FE_DOWNWARD),
        ('z', FE_TOWARDZERO),
    ];

    /// The x87 unit at 64-bit precision: what a SlateOS thread starts with,
    /// and not the Windows host's 53 bits.
    fn extended() {
        let cw: u16 = 0x037F;
        // SAFETY: loads the control word from a local.
        unsafe {
            core::arch::asm!("fldcw [{}]", in(reg) &raw const cw, options(nostack, preserves_flags));
        }
    }

    fn l(s: &str) -> L {
        let (se, m) = s.split_once(':').unwrap();
        L::from_bits(
            u16::from_str_radix(se, 16).unwrap(),
            u64::from_str_radix(m, 16).unwrap(),
        )
    }

    fn hex(x: L) -> String {
        format!("{:04x}:{:016x}", x.sign_exp, x.significand)
    }

    fn flags() -> String {
        let f = fetestexcept(FIVE);
        let mut s = String::new();
        for (bit, c) in [
            (FE_INVALID, 'I'),
            (FE_DIVBYZERO, 'Z'),
            (FE_OVERFLOW, 'O'),
            (FE_UNDERFLOW, 'U'),
            (FE_INEXACT, 'X'),
        ] {
            if f & bit != 0 {
                s.push(c);
            }
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }

    /// A finite long double's place among them all, as a signed count of
    /// long doubles from zero.
    fn ordinal(x: L) -> i128 {
        let e = i128::from(x.biased_exponent());
        let m = i128::from(x.significand);
        let o = if e == 0 { m } else { (e - 1) * (1 << 63) + m };
        if x.is_sign_negative() { -o } else { o }
    }

    fn from_ordinal(o: i128) -> L {
        let a = o.unsigned_abs();
        let e = (a >> 63) as u16;
        let m = if e == 0 {
            a as u64
        } else {
            ((a & ((1 << 63) - 1)) as u64) | (1 << 63)
        };
        L::from_bits(if o < 0 { e | 0x8000 } else { e }, m)
    }

    fn call(func: &str, n: i32, x: L) -> L {
        match func {
            "j0l" => j0l(x),
            "j1l" => j1l(x),
            "y0l" => y0l(x),
            "y1l" => y1l(x),
            "jnl" => jnl(n, x),
            "ynl" => ynl(n, x),
            other => panic!("no {other}"),
        }
    }

    struct Row<'a> {
        func: &'a str,
        n: i32,
        x: L,
        y: L,
        /// The exact value against `y`: `<`, `=` or `>`.
        rel: char,
    }

    fn oracle_rows() -> Vec<Row<'static>> {
        ORACLE
            .lines()
            .filter(|s| !s.starts_with('#') && !s.is_empty())
            .map(|s| {
                let w: Vec<&str> = s.split(' ').collect();
                Row {
                    func: w[0],
                    n: if w[1] == "-" {
                        0
                    } else {
                        w[1].parse().unwrap()
                    },
                    x: l(w[2]),
                    y: l(w[4]),
                    rel: w[5].chars().next().unwrap(),
                }
            })
            .collect()
    }

    /// The correctly rounded value in direction `mode`, from the nearest one
    /// and the exact value's side of it.
    fn directed(y: L, rel: char, mode: char) -> L {
        if rel == '=' || mode == 'n' {
            return y;
        }
        let o = ordinal(y);
        let above = rel == '>';
        let rounds_up = match mode {
            'u' => true,
            'd' => false,
            _ => y.is_sign_negative(),
        };
        match (rounds_up, above) {
            (true, true) => from_ordinal(o + 1),
            (false, false) => from_ordinal(o - 1),
            _ => y,
        }
    }

    /// How far `r` is from `y`, in units in the last place.
    fn ulps(r: L, y: L) -> i128 {
        (ordinal(r) - ordinal(y)).abs()
    }

    /// Whether a row is within a unit of a zero of its function above 48,
    /// where the phase is right to about 2^-150 but the value, 2^-58 of the
    /// amplitude a unit from the zero, keeps only about 90 bits of it:
    /// correctly rounded all but where the exact value lies within about
    /// 2^-26 of a unit of a rounding boundary. Or, for an order past 1,
    /// within 2^-40 of the amplitude of zero, where the recurrence's
    /// absolute error of 2^-124 of the amplitude is what is left.
    fn near_a_zero(row: &Row) -> bool {
        let x = row.x.abs().to_f64();
        let y = row.y.abs().to_f64();
        let amplitude = (2.0 / (core::f64::consts::PI * x)).sqrt().min(1.0);
        match row.func {
            "jnl" | "ynl" => y < amplitude * 2f64.powi(-40),
            _ => x > 48.0 && y < amplitude * 2f64.powi(-50),
        }
    }

    /// Every value is mpmath's, correctly rounded -- the exact value decides
    /// every rounding, as double-long-double arithmetic holds it to about
    /// 2^-124 -- but for the few `near_a_zero` rows, which may be a unit
    /// off.
    #[test]
    fn every_value_is_the_correctly_rounded_one() {
        extended();
        let rows = oracle_rows();
        assert!(
            rows.len() > 13_000,
            "the oracle is whole: {} rows",
            rows.len()
        );
        let mut failures = Vec::new();
        let mut near = 0;
        for row in &rows {
            let r = call(row.func, row.n, row.x);
            let d = ulps(r, row.y);
            if d == 0 {
                continue;
            }
            if near_a_zero(row) && d <= 1 {
                near += 1;
                continue;
            }
            failures.push(format!(
                "{} {} {} = {} (ours {}, {} ulps)",
                row.func,
                row.n,
                hex(row.x),
                hex(row.y),
                hex(r),
                d
            ));
        }
        let shown = failures.len().min(60);
        assert!(
            failures.is_empty(),
            "{} of {} values differ ({near} a unit off near a zero, allowed); the first {shown}:\n{}",
            failures.len(),
            rows.len(),
            failures[..shown].join("\n")
        );
    }

    /// In each directed mode every value is the exact value rounded that way.
    #[test]
    fn every_direction_rounds_the_exact_value() {
        extended();
        let rows = oracle_rows();
        let mut failures = Vec::new();
        let mut calls = 0;
        for (i, row) in rows.iter().enumerate() {
            // Every third row, in each of the three: a third of the time.
            if i % 3 != 0 {
                continue;
            }
            for (name, mode) in MODES.iter().skip(1) {
                assert_eq!(fesetround(*mode), 0);
                let r = call(row.func, row.n, row.x);
                assert_eq!(fesetround(FE_TONEAREST), 0);
                calls += 1;
                let want = directed(row.y, row.rel, *name);
                let d = ulps(r, want);
                if d == 0 || (near_a_zero(row) && d <= 1) {
                    continue;
                }
                failures.push(format!(
                    "{name} {} {} {}: want {} ours {}",
                    row.func,
                    row.n,
                    hex(row.x),
                    hex(want),
                    hex(r)
                ));
            }
        }
        assert!(calls > 13_000, "{calls} calls");
        let shown = failures.len().min(60);
        assert!(
            failures.is_empty(),
            "{} of {calls} differ; the first {shown}:\n{}",
            failures.len(),
            failures[..shown].join("\n")
        );
    }

    struct GlibcRow<'a> {
        mode: char,
        func: &'a str,
        n: i32,
        x: L,
        y: L,
        flags: &'a str,
        errno: i32,
    }

    fn glibc_rows() -> Vec<GlibcRow<'static>> {
        GLIBC
            .lines()
            .filter(|s| !s.starts_with('#') && !s.is_empty())
            .map(|s| {
                let w: Vec<&str> = s.split(' ').collect();
                GlibcRow {
                    mode: w[0].chars().next().unwrap(),
                    func: w[1],
                    n: if w[2] == "-" {
                        0
                    } else {
                        w[2].parse().unwrap()
                    },
                    x: l(w[3]),
                    y: l(w[5]),
                    flags: w[6],
                    errno: w[7].parse().unwrap(),
                }
            })
            .collect()
    }

    /// Whether an argument or a result is one of the values C fixes the
    /// answer at: a NaN (or an encoding the x87 refuses), an infinity, a
    /// zero.
    fn special(x: L) -> bool {
        x.is_nan() || x.is_infinite() || x.is_zero()
    }

    /// An encoding the x87 refuses: an unnormal, a pseudo-infinity or a
    /// pseudo-NaN -- the integer bit clear where the exponent is not 0.
    fn refused(x: L) -> bool {
        x.biased_exponent() != 0 && x.significand >> 63 == 0
    }

    /// glibc's answers where C leaves them to the implementation: at every
    /// special argument and for every special result -- a NaN (its bits),
    /// an infinity or a zero (their signs), the largest finite value and the
    /// least subnormal of a range error rounded away from the infinity --
    /// the same value, and everywhere the same invalid, divide-by-zero,
    /// overflow and underflow flags, inexact wherever glibc raises it, and
    /// the `errno` glibc sets rounding to nearest (this library's one rule
    /// for the four directions: `math.rs`). At an encoding the x87 refuses
    /// every function answers as `mathl.rs`'s do, the unit's NaN with
    /// invalid and `errno` untouched, where glibc's `jnl` and `ynl` answer
    /// what their bit tests happen to make of it -- 0 for a pseudo-infinity
    /// (which glibc's own `j0l` answers NaN), `ERANGE` for an unnormal.
    #[test]
    fn glibcs_special_values_flags_and_errno() {
        extended();
        let rows = glibc_rows();
        assert!(rows.len() > 8_000, "{} rows", rows.len());
        let nearest_errno: HashMap<(&str, i32, (u16, u64)), i32> = rows
            .iter()
            .filter(|r| r.mode == 'n')
            .map(|r| ((r.func, r.n, (r.x.sign_exp, r.x.significand)), r.errno))
            .collect();
        let mut failures = Vec::new();
        for row in &rows {
            let mode = MODES.iter().find(|(c, _)| *c == row.mode).unwrap().1;
            assert_eq!(fesetround(mode), 0);
            feclearexcept(FIVE);
            errno::set_errno(0);
            let r = call(row.func, row.n, row.x);
            let e = errno::get_errno();
            let fl = flags();
            assert_eq!(fesetround(FE_TONEAREST), 0);
            if refused(row.x) {
                if hex(r) != "ffff:c000000000000000" || fl != "I" || e != 0 {
                    failures.push(format!(
                        "{} {} {} {}: ours {} {fl} {e}, want the unit's NaN, I, 0",
                        row.mode,
                        row.func,
                        row.n,
                        hex(row.x),
                        hex(r)
                    ));
                }
                continue;
            }
            let at_the_edge =
                ordinal(row.y.abs()) == ordinal(LDBL_MAX) || ordinal(row.y.abs()) == 1;
            let value_fixed = special(row.x) || special(row.y) || at_the_edge;
            let mut why = Vec::new();
            if value_fixed && hex(r) != hex(row.y) {
                why.push("value");
            }
            let key = (row.func, row.n, (row.x.sign_exp, row.x.significand));
            if e != nearest_errno[&key] {
                why.push("errno");
            }
            // Underflow is decided by the result: glibc's where the two agree,
            // ours -- tiny and inexact -- where ours is the correctly rounded
            // value glibc's is not.
            let same = hex(r) == hex(row.y);
            let tiny = r.abs() < pow2(-16_382) && fl.contains('X');
            let strip = |f: &str| -> String {
                f.chars()
                    .filter(|&c| c != 'X' && c != '-' && c != 'U')
                    .collect()
            };
            let underflow_ok = if same {
                row.flags.contains('U') == fl.contains('U')
            } else {
                tiny == fl.contains('U')
            };
            if strip(row.flags) != strip(&fl)
                || !underflow_ok
                || (row.flags.contains('X') && !fl.contains('X'))
            {
                why.push("flags");
            }
            if !why.is_empty() {
                failures.push(format!(
                    "{} {} {} {}: glibc {} {} {}, ours {} {fl} {e} ({})",
                    row.mode,
                    row.func,
                    row.n,
                    hex(row.x),
                    hex(row.y),
                    row.flags,
                    row.errno,
                    hex(r),
                    why.join(", ")
                ));
            }
        }
        feclearexcept(FIVE);
        let shown = failures.len().min(60);
        assert!(
            failures.is_empty(),
            "{} of {} rows differ; the first {shown}:\n{}",
            failures.len(),
            rows.len(),
            failures[..shown].join("\n")
        );
    }

    /// `J0` is even and `J1` odd, in every direction; `J(-n, x) = J(n, -x) =
    /// (-1)^n J(n, x)` and `Y(-n, x) = (-1)^n Y(n, x)`, a negation's
    /// rounding the other way's.
    #[test]
    fn the_symmetries_hold_in_every_direction() {
        extended();
        // 0.75, 5.12, 16 pi and 10^6.
        let xs = [
            "3ffe:c000000000000000",
            "4001:a3d70a3d70a3d70a",
            "4004:c90fdaa22168c235",
            "4012:f424000000000000",
        ]
        .map(l);
        let opposite = |mode: i32| match mode {
            FE_UPWARD => FE_DOWNWARD,
            FE_DOWNWARD => FE_UPWARD,
            m => m,
        };
        for x in xs {
            for (_, mode) in MODES {
                let at = |m: i32, f: &dyn Fn() -> L| {
                    assert_eq!(fesetround(m), 0);
                    let r = f();
                    assert_eq!(fesetround(FE_TONEAREST), 0);
                    r
                };
                assert_eq!(hex(at(mode, &|| j0l(-x))), hex(at(mode, &|| j0l(x))));
                assert_eq!(
                    hex(at(mode, &|| j1l(-x))),
                    hex(-at(opposite(mode), &|| j1l(x)))
                );
                for n in [2, 3, 7] {
                    let sign = |r: L| if n % 2 == 1 { -r } else { r };
                    let m2 = if n % 2 == 1 { opposite(mode) } else { mode };
                    let jn = at(m2, &|| jnl(n, x));
                    assert_eq!(hex(at(mode, &|| jnl(-n, x))), hex(sign(jn)), "jnl(-{n})");
                    assert_eq!(hex(at(mode, &|| jnl(n, -x))), hex(sign(jn)), "jnl(-x)");
                    let yn = at(m2, &|| ynl(n, x));
                    assert_eq!(hex(at(mode, &|| ynl(-n, x))), hex(sign(yn)), "ynl(-{n})");
                }
            }
        }
    }

    /// The Wronskian `J1(x) Y0(x) - J0(x) Y1(x) = 2/(pi x)`, across the
    /// methods' ranges: an independent check of the four together.
    #[test]
    fn the_wronskian_holds() {
        extended();
        for x in [
            1e-30, 1e-5, 0.3, 1.0, 2.404, 7.5, 30.0, 47.99, 48.01, 100.0, 1e5, 1e12, 1e30,
        ] {
            let lx = L::from_f64(x);
            let (j0, j1, y0, y1) = (
                j0l(lx).to_f64(),
                j1l(lx).to_f64(),
                y0l(lx).to_f64(),
                y1l(lx).to_f64(),
            );
            let w = j1 * y0 - j0 * y1;
            let want = 2.0 / (core::f64::consts::PI * x);
            assert!(
                ((w - want) / want).abs() < 1e-14,
                "x = {x}: {w} against {want}"
            );
        }
    }

    /// The pair operations' assembly blocks compute exactly what their Rust
    /// references do, bit for bit, over pairs of every magnitude and sign.
    #[test]
    fn the_pair_blocks_are_the_rust_operations_bit_for_bit() {
        extended();
        // A small linear congruential generator: the same cases every run.
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };
        let pair = |next: &mut dyn FnMut() -> u64| {
            let e = (next() % 400) as i32 - 200;
            let hi = L::from_bits((0x3FFF + e) as u16, next() | (1 << 63));
            let hi = if next() % 2 == 0 { hi } else { -hi };
            let lo = L::from_bits(
                (0x3FFF + e - 64 - (next() % 3) as i32) as u16,
                next() | (1 << 63),
            );
            let lo = if next() % 2 == 0 { lo } else { -lo };
            DD::norm((hi, lo))
        };
        let same = |a: DD, b: DD| hex(a.hi) == hex(b.hi) && hex(a.lo) == hex(b.lo);
        for i in 0..20_000 {
            let a = pair(&mut next);
            let b = pair(&mut next);
            let b = if i % 7 == 0 { DD::l(b.hi) } else { b };
            assert!(same(a.add(b), a.add_ref(b)), "add {a:?} {b:?}");
            assert!(same(a.add(b.neg()), a.add_ref(b.neg())), "sub {a:?} {b:?}");
            assert!(same(a.mul(b), a.mul_ref(b)), "mul {a:?} {b:?}");
            assert!(same(a.mul_l(b.hi), a.mul_l_ref(b.hi)), "mul_l {a:?} {b:?}");
        }
        // Zeros and exact halves, where a sign or a tie decides.
        for (a, b) in [
            (DD::ZERO, DD::ZERO),
            (DD::l(ONE), DD::l(-ONE)),
            (
                DD::l(ONE),
                DD {
                    hi: pow2(-64),
                    lo: ZERO,
                },
            ),
            (
                DD {
                    hi: ONE,
                    lo: pow2(-65),
                },
                DD {
                    hi: ONE,
                    lo: -pow2(-65),
                },
            ),
        ] {
            assert!(same(a.add(b), a.add_ref(b)));
            assert!(same(a.mul(b), a.mul_ref(b)));
            assert!(same(a.mul_l(b.hi), a.mul_l_ref(b.hi)));
        }
    }

    /// Each zero's Taylor series and Miller's recurrence agree where they
    /// meet, just inside the window: a check of the table's zeros and
    /// coefficients, and of the recurrence's absolute accuracy.
    #[test]
    fn the_taylor_windows_meet_the_recurrence() {
        extended();
        for (kind, zeros) in [
            (Kind::J0, J0_ZEROS),
            (Kind::J1, J1_ZEROS),
            (Kind::Y0, Y0_ZEROS),
            (Kind::Y1, Y1_ZEROS),
        ] {
            for zero in zeros {
                for side in [ONE, -ONE] {
                    let x = zero.z[0] + side * W * L::from_f64(0.999);
                    let taylor = near_zero(zeros, x).unwrap();
                    let low = miller01(x);
                    let miller = match kind {
                        Kind::J0 => low.j0,
                        Kind::J1 => low.j1,
                        Kind::Y0 => y0_low(&low, x),
                        Kind::Y1 => y1_low(&low, x),
                    };
                    let diff = taylor.sub(miller).mag().to_f64();
                    // The value there is about c1 2^-32; the recurrence is
                    // good to 2^-120 or so absolutely.
                    let size = taylor.mag().to_f64();
                    assert!(
                        diff <= size * 2f64.powi(-80),
                        "zero {}: Taylor {} Miller {} ({diff:e} apart)",
                        hex(zero.z[0]),
                        hex(taylor.hi),
                        hex(miller.hi)
                    );
                }
            }
        }
    }

    /// Hankel's phase and amplitude and Miller's recurrence agree past 48,
    /// where both hold: a check of the phase's reduction and of the
    /// expansion.
    #[test]
    fn hankel_and_miller_agree_past_48() {
        extended();
        for x in [48.0, 48.5, 55.25, 71.0, 100.0, 150.75] {
            let x = L::from_f64(x);
            let low = miller01(x);
            let (j0, y0) = hankel(0, x);
            let (j1, y1) = hankel(1, x);
            let amplitude = (2.0 / (core::f64::consts::PI * x.to_f64())).sqrt();
            for (name, a, b) in [
                ("J0", j0, low.j0),
                ("J1", j1, low.j1),
                ("Y0", y0, y0_low(&low, x)),
                ("Y1", y1, y1_low(&low, x)),
            ] {
                let diff = a.sub(b).mag().to_f64();
                assert!(
                    diff <= amplitude * 2f64.powi(-110),
                    "{name}({}): Hankel {} Miller {} ({diff:e} apart)",
                    x.to_f64(),
                    hex(a.hi),
                    hex(b.hi)
                );
            }
        }
    }

    /// Where the power series of `J_n` and Miller's recurrence both hold,
    /// they agree.
    #[test]
    fn the_series_and_miller_agree() {
        extended();
        for (m, x) in [(2u32, 0.5), (5, 3.0), (10, 5.5), (40, 12.0), (100, 19.0)] {
            let x = L::from_f64(x);
            let half = x * HALF;
            let q = DD::norm(two_prod(half, half));
            let (s, se) = jn_series(m, half, q);
            let (b, be) = jn_miller(m, x);
            let s = s.scale(se);
            let b = b.scale(be);
            let diff = s.sub(b).mag().to_f64();
            assert!(
                diff <= s.mag().to_f64() * 2f64.powi(-110),
                "J{m}({}): series {} Miller {}",
                x.to_f64(),
                hex(s.hi),
                hex(b.hi)
            );
        }
    }
}
