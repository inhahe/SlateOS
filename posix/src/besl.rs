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
//! `jnl` and `ynl`, up to order 511: `Y_n` by the forward recurrence from
//! Y0 and Y1, which is stable for it everywhere; `J_n` by its power series
//! where `x^2 < 4(n + 1)`, the forward recurrence from J0 and J1 where `n <=
//! x` past 48, and Miller's recurrence to `n` elsewhere. From order 512,
//! Debye's expansions -- the sech(alpha) form before the turning point `x =
//! n`, the sec(beta) form past it -- except within `32 n^(1/3)` of it,
//! where from order 2048 a recurrence crosses from where they hold, so no
//! order costs more than about `70 n^(1/3)` steps. An answer the Debye estimate puts far past the range
//! is answered as an overflow or underflow at once. Near a zero of `J_n` or
//! `Y_n` for `n >= 2` the answer is good to about 2^-124 of the function's
//! amplitude rather than of its value; at a huge order the phase, which is
//! of the order of `n`, is good to 2^-128 of itself, so at order 2^31 the
//! answer is good to about 2^-97 of the amplitude.
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
    // In 64 bits: at order 2^31 the start is past 2^31 itself.
    #[allow(clippy::cast_precision_loss)]
    let enough = |big: u64| {
        let b = big as f64;
        debye_g(b, ln_x) >= target && 2.0 * debye_g(b + 1.0, ln_x) - 2.0 * gn >= target
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let floor_x = x.to_f64().min(4.0e9) as u64;
    let mut lo = u64::from(n).max(floor_x) + 2;
    let mut hi = lo;
    if !enough(hi) {
        // Double the step until enough, then halve back. The start is never
        // far past n: 70 n^(1/3) at most, by Debye's estimate.
        let mut step = 8u64;
        hi = lo + step;
        while !enough(hi) && hi < 1 << 40 {
            lo = hi;
            step *= 2;
            hi += step;
        }
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if enough(mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
    }
    u32::try_from(hi).unwrap_or(u32::MAX)
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
/// XH` and `psi` Hankel's or Debye's phase correction, with `x + psi > 0`.
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
    // x = n pi/2 + y, |y| <= pi/4; y + psi = k pi/2 + b, |b| <= pi/4 (psi
    // is Debye's phase at a huge order, up to about 2^21 there, or Hankel's,
    // small); then x + psi - (2(n + k) + j) pi/4 = b - j pi/4, j = +-1.
    let (n, y) = rem_pio2_113(x);
    let base = y.add(psi);
    #[allow(clippy::cast_possible_truncation)]
    let k = (base.hi * TWO_OVER_PI.hi).to_i64_rint();
    let b = base.sub(PI_OVER_4.twice().mul_l(int(k)));
    let (j, t) = if b.hi.is_sign_negative() {
        (-1i64, b.add(PI_OVER_4))
    } else {
        (1i64, b.sub(PI_OVER_4))
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let k8 = ((2 * (n + k) + j).rem_euclid(8)) as u32;
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
    phase_amplitude(nu, x, psi, amp)
}

/// `amp cos(theta)` and `amp sin(theta)`, `theta = x + psi - (2 nu + 1)
/// pi/4` (`nu_mod4` is `nu` mod 4): the phase reduced by pi/4 exactly
/// ([`reduce`]), then the quadrant's sine or cosine of what is left.
fn phase_amplitude(nu_mod4: u32, x: L, psi: DD, amp: DD) -> (DD, DD) {
    let (k8, t) = reduce(x, psi);
    // theta = t + m pi/2, K = 2m + 2 nu + 1.
    let m = ((k8 + 8 - (2 * nu_mod4 + 1)) % 8) / 2;
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
    if m >= DEBYE_ORDER {
        if let Some(v) = jn_large(m, x) {
            return v;
        }
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
    if m >= DEBYE_ORDER {
        if let Some(v) = yn_large(m, x) {
            return v;
        }
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
// Huge orders: Debye's expansions
//
// Past order DEBYE_ORDER a recurrence to the order would be long, so J_n and
// Y_n come from Debye's asymptotic expansions (DLMF 10.19.3 and 10.19.6),
// whose terms fall as powers of 1/n -- in the sech(alpha) form before the
// turning point x = n, the sec(beta) form past it. Both fail near the turning
// point; from DEBYE_GAP n^(1/3) away their sums hold to 2^-135 with at most
// 27 of Debye's polynomials, at every order (the distance in units of n^(1/3)
// is what the terms depend on). Within that window a recurrence crosses from
// where they hold: upward for Y, which grows that way; downward, Miller's,
// for J, scaled to Debye's values on the far side -- about 70 n^(1/3) steps,
// some 80,000 at the largest order.
// ---------------------------------------------------------------------------

/// From this order up `jnl` and `ynl` use Debye's expansions away from the
/// turning point: 12 to 20 microseconds, where the recurrences take a tenth
/// of a microsecond an order. (They agree with the recurrences to 2^-112
/// from order 256 up.)
const DEBYE_ORDER: u32 = 512;
/// From this order up they also cross the turning-point window from
/// Debye's values, rather than recurring all the way from order 0 or 1:
/// the window's two evaluations and its `70 n^(1/3)` steps cost about what
/// a whole recurrence does here.
const DEBYE_WINDOW_ORDER: u32 = 2048;
/// How far past the turning point, in units of `n^(1/3)`, Debye's
/// expansions answer.
const DEBYE_GAP: f64 = 32.0;

/// `atan(u)` in [`DD`] for a finite `u`: odd; past 1 `pi/2 - atan(1/u)`;
/// otherwise the argument halved four times, `atan(u) = 2 atan(u / (1 +
/// sqrt(1 + u^2)))`, to below `tan(pi/64)`, and the series there.
fn atan_dd(u: DD) -> DD {
    if u.hi.is_sign_negative() {
        return atan_dd(u.neg()).neg();
    }
    let one = DD::l(ONE);
    if u.hi > ONE {
        return PI_OVER_4.twice().sub(atan_dd(one.div(u)));
    }
    let mut v = u;
    for _ in 0..4 {
        v = v.div(one.add(one.add(v.mul(v)).sqrt()));
    }
    let v2 = v.mul(v);
    let mut acc = recip(35);
    for j in (0..17u32).rev() {
        acc = recip(2 * j + 1).sub(v2.mul(acc));
    }
    acc.mul(v).scale(4)
}

/// `ln q` for a pair `q > 0`: [`ln_dd`] of its high part, and its low part's
/// share to first order, which is all of it at 2^-128.
fn ln_of(q: DD) -> DD {
    ln_dd(q.hi).add(DD::l(q.lo / q.hi))
}

/// `e^y` as `(v, e)`, `e^y = v 2^e`, `v` within a factor `2^(1/2)` of 1, for
/// `|y|` below about 2^30: `y = e ln 2 + r`, `|r| <= ln 2 / 2`, and `e^r` by
/// its series to `r^30`.
fn exp_scaled(y: DD) -> (DD, i32) {
    #[allow(clippy::cast_possible_truncation)]
    let e = (y.hi * L::LOG2_E).to_i64_rint() as i32;
    let r = y.sub(LN_2.mul_l(int(i64::from(e))));
    let mut acc = inv_factorial(30);
    for k in (0..30usize).rev() {
        acc = acc.mul(r).add(inv_factorial(k));
    }
    (acc, e)
}

/// `atanh(tau) - tau` for `0 < tau < 1`: below 1/4 by its series, `tau^3/3 +
/// tau^5/5 + ...` -- the difference is what Debye's exponent needs, and near
/// the turning point it is all cancellation -- and above by the logarithm.
fn atanh_minus_id(tau: DD) -> DD {
    if tau.hi < pow2(-2) {
        let t2 = tau.mul(tau);
        // sum over j >= 1 of tau^(2j+1)/(2j+1), to j = 34: tau^68 < 2^-136.
        let mut acc = recip(69);
        for j in (1..34u32).rev() {
            acc = acc.mul(t2).add(recip(2 * j + 1));
        }
        return acc.mul(t2).mul(tau);
    }
    let one = DD::l(ONE);
    ln_of(one.add(tau).div(one.sub(tau))).scale(-1).sub(tau)
}

/// Debye's sums at `t` for order `n`: over the even `k` and over the odd, of
/// `u_k(t)/n^k` -- or, `oscillatory`, of `u_k(i t)/n^k` with the powers of
/// `i` taken out: the even sum is real, the odd one `i` times the second
/// value. Each term `(t/n)^k v_k(t^2)`, `v_k` row `k` of [`DEBYE`]; summed
/// until one is below 2^-135.
fn debye_sums(t: DD, n: L, oscillatory: bool) -> (DD, DD) {
    let t2 = t.mul(t);
    let w = if oscillatory { t2.neg() } else { t2 };
    let ratio = t.div_l(n);
    let mut base = DD::l(ONE);
    let (mut even, mut odd) = (DD::ZERO, DD::ZERO);
    for (k, row) in DEBYE.iter().enumerate() {
        let mut v = DD::ZERO;
        for &c in row.iter().rev() {
            v = v.mul(w).add(c);
        }
        let term = base.mul(v);
        // i^k: (-1)^(k/2), and a factor i more for odd k.
        let term = if oscillatory && (k / 2) % 2 == 1 {
            term.neg()
        } else {
            term
        };
        if k % 2 == 0 {
            even = even.add(term);
        } else {
            odd = odd.add(term);
        }
        if k >= 2 && term.mag() < pow2(-135) {
            break;
        }
        base = base.mul(ratio);
    }
    (even, odd)
}

/// `J_nu(x)` and `Y_nu(x)` past the turning point, `x >= nu + DEBYE_GAP
/// nu^(1/3)`: Debye's sec(beta) form, `J = sqrt(2/(pi s)) (E cos xi + R sin
/// xi)`, `Y = sqrt(2/(pi s)) (E sin xi - R cos xi)`, `s = sqrt(x^2 - nu^2) =
/// nu tan(beta)`, `xi = s - nu beta - pi/4`, `E` and `R` Debye's sums at `i
/// cot(beta)` -- written in phase and amplitude as Hankel's expansion is, and
/// the phase `x - (2 nu + 1) pi/4 + phi + atan(-R/E)`, `phi = s - x + nu
/// atan(nu/s)`, reduced by pi/4 as Hankel's is. Nothing is formed from `x`
/// but `nu/x`, so no size of `x` overflows.
fn debye_oscillatory(nu: u32, x: L) -> (DD, DD) {
    let n = int(i64::from(nu));
    let one = DD::l(ONE);
    let r = over_x(n, x);
    // s/x and cot(beta) = nu/s.
    let sq = one.sub(r).mul(one.add(r)).sqrt();
    let c = r.div(sq);
    let (e, o) = debye_sums(c, n, true);
    let phase = atan_dd(o.div(e)).neg();
    let size = e.mul(e).add(o.mul(o)).sqrt();
    // sqrt(2/(pi s)) = sqrt(2/(pi x)) / sqrt(s/x), x's power of 2 halved
    // outside the root.
    let (xs, ex) = split(x);
    let amp = TWO_OVER_PI
        .div_l(xs)
        .scale(-(ex & 1))
        .sqrt()
        .scale(-(ex >> 1))
        .mul(size)
        .div(sq.sqrt());
    // s - x = -x r^2 / (1 + s/x) = -nu r / (1 + s/x).
    let phi = atan_dd(c).sub(r.div(one.add(sq))).mul_l(n);
    phase_amplitude(nu % 4, x, phi.add(phase), amp)
}

/// `J_nu(x)` and `Y_nu(x)` before the turning point, `x <= nu - DEBYE_GAP
/// nu^(1/3)`, each as `(v, e)` for `v 2^e`: Debye's sech(alpha) form, `J =
/// e^-g (E + O) / sqrt(2 pi nu tanh(alpha))`, `Y = -e^g (E - O) /
/// sqrt(pi nu tanh(alpha) / 2)`, `g = nu (alpha - tanh(alpha))`, `E` and
/// `O` Debye's sums at `coth(alpha)`.
fn debye_evanescent(nu: u32, x: L) -> ((DD, i32), (DD, i32)) {
    let n = int(i64::from(nu));
    let one = DD::l(ONE);
    let r = DD::l(x).div_l(n);
    let tau = one.sub(r).mul(one.add(r)).sqrt();
    let (e, o) = debye_sums(one.div(tau), n, false);
    let g = atanh_minus_id(tau).mul_l(n);
    // sqrt(2 pi nu tanh(alpha)); pi/4 times 8 is 2 pi.
    let root = PI_OVER_4.scale(3).mul(tau).mul_l(n).sqrt();
    let (vj, ej) = exp_scaled(g.neg());
    let (vy, ey) = exp_scaled(g);
    let j = vj.mul(e.add(o)).div(root);
    let y = vy.mul(e.sub(o)).div(root).scale(1).neg();
    ((j, ej), (y, ey))
}

/// The order below the turning-point window from which the recurrences start
/// for `x`: at least `DEBYE_GAP x^(1/3)` below `x`, so Debye's oscillatory
/// form holds for it and the next, and at least 2 below `nu`.
fn window_start(nu: u32, x: L) -> u32 {
    let xf = x.to_f64();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let p = libm::floor(xf - DEBYE_GAP * libm::cbrt(xf)).max(2.0) as u32;
    p.saturating_sub(1).min(nu - 2)
}

/// `J_m(x)` for `m >= DEBYE_ORDER`, as `(v, e)` for `v 2^e` -- or `None`
/// within the turning-point window below [`DEBYE_WINDOW_ORDER`], where the
/// recurrences are the cheaper way.
fn jn_large(m: u32, x: L) -> Option<(DD, i32)> {
    let (mf, xf) = (f64::from(m), x.to_f64());
    let gap = DEBYE_GAP * libm::cbrt(mf);
    if xf >= mf + gap {
        return Some((debye_oscillatory(m, x).0, 0));
    }
    if xf <= mf - gap {
        return Some(debye_evanescent(m, x).0);
    }
    if m < DEBYE_WINDOW_ORDER {
        return None;
    }
    // Within the window: Miller's recurrence from start_index down to p,
    // scaled to Debye's J_p and J_(p+1) -- by least squares over the two, so
    // a zero of either costs nothing.
    let p = window_start(m, x);
    let (jp, _) = debye_oscillatory(p, x);
    let (jq, _) = debye_oscillatory(p + 1, x);
    let big_n = start_index(m, x);
    let two_over_x = over_x(TWO, x);
    let (mut next, mut cur) = (DD::ZERO, DD::l(ONE));
    let (mut ans, mut ans_e, mut e) = (DD::ZERO, 0i32, 0i32);
    for k in (p + 1..=big_n).rev() {
        if k == m {
            ans = cur;
            ans_e = e;
        }
        let prev = two_over_x.mul_l(int(i64::from(k))).mul(cur).sub(next);
        next = cur;
        cur = prev;
        if cur.mag() > pow2(RESCALE) {
            cur = cur.scale(-RESCALE);
            next = next.scale(-RESCALE);
            e += RESCALE;
        }
    }
    // cur is f(p), next f(p+1), both at 2^-e; J_k = S f(k).
    let s = jp
        .mul(cur)
        .add(jq.mul(next))
        .div(cur.mul(cur).add(next.mul(next)));
    Some((ans.mul(s), ans_e - e))
}

/// `Y_m(x)` for `m >= DEBYE_ORDER`, as `(v, e)` for `v 2^e` -- or `None`
/// as [`jn_large`] answers it.
fn yn_large(m: u32, x: L) -> Option<(DD, i32)> {
    let (mf, xf) = (f64::from(m), x.to_f64());
    let gap = DEBYE_GAP * libm::cbrt(mf);
    if xf >= mf + gap {
        return Some((debye_oscillatory(m, x).1, 0));
    }
    if xf <= mf - gap {
        return Some(debye_evanescent(m, x).1);
    }
    if m < DEBYE_WINDOW_ORDER {
        return None;
    }
    // Within the window: upward from Debye's Y_p and Y_(p+1).
    let p = window_start(m, x);
    let (_, mut a) = debye_oscillatory(p, x);
    let (_, mut b) = debye_oscillatory(p + 1, x);
    let two_over_x = over_x(TWO, x);
    let mut e = 0i32;
    for k in p + 1..m {
        let c = two_over_x.mul_l(int(i64::from(k))).mul(b).sub(a);
        a = b;
        b = c;
        if b.mag() > pow2(RESCALE) {
            a = a.scale(-RESCALE);
            b = b.scale(-RESCALE);
            e += RESCALE;
        }
    }
    Some((b, e))
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
/// Debye's polynomials u_0 .. u_28: u_k(t) = t^k times the
/// polynomial in t^2 whose coefficients, lowest first, are row k.
const DEBYE: [&[DD]; 29] = [
    &[DD {
        hi: L::from_bits(0x3FFF, 0x8000000000000000),
        lo: L::from_bits(0x0000, 0x0000000000000000),
    }],
    &[
        DD {
            hi: L::from_bits(0x3FFC, 0x8000000000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xBFFC, 0xD555555555555555),
            lo: L::from_bits(0xBFBB, 0xAAAAAAAAAAAAAAAB),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x3FFB, 0x9000000000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xBFFD, 0xCD55555555555555),
            lo: L::from_bits(0xBFBC, 0xAAAAAAAAAAAAAAAB),
        },
        DD {
            hi: L::from_bits(0x3FFD, 0xAB1C71C71C71C71C),
            lo: L::from_bits(0x3FBC, 0xE38E38E38E38E38E),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x3FFB, 0x9600000000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xBFFE, 0xE426666666666666),
            lo: L::from_bits(0xBFBD, 0xCCCCCCCCCCCCCCCD),
        },
        DD {
            hi: L::from_bits(0x3FFF, 0xEC58E38E38E38E39),
            lo: L::from_bits(0xBFBC, 0xE38E38E38E38E38E),
        },
        DD {
            hi: L::from_bits(0xBFFF, 0x834DD3C0CA4587E7),
            lo: L::from_bits(0x3FBE, 0x9161F9ADD3C0CA46),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x3FFB, 0xE5B0000000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC000, 0x974D333333333333),
            lo: L::from_bits(0xBFBE, 0xCCCCCCCCCCCCCCCD),
        },
        DD {
            hi: L::from_bits(0x4002, 0x8CA0400000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC002, 0xB34FE1F9ADD3C0CA),
            lo: L::from_bits(0xBFC1, 0x8B0FCD6E9E06522C),
        },
        DD {
            hi: L::from_bits(0x4001, 0x956D3C5010DB20A9),
            lo: L::from_bits(0xBFC0, 0xE172D4CE7C5010DB),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x3FFC, 0xE88F000000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC001, 0xEBCD29D41D41D41D),
            lo: L::from_bits(0xBFC0, 0x83A83A83A83A83A8),
        },
        DD {
            hi: L::from_bits(0x4004, 0xAA23D6B60B60B60B),
            lo: L::from_bits(0x3FC3, 0xC16C16C16C16C16C),
        },
        DD {
            hi: L::from_bits(0xC005, 0xB7A2F08E38E38E39),
            lo: L::from_bits(0x3FC2, 0xE38E38E38E38E38E),
        },
        DD {
            hi: L::from_bits(0x4005, 0xA945BE52B3183AFF),
            lo: L::from_bits(0xBFC1, 0xDB20A88F469598C2),
        },
        DD {
            hi: L::from_bits(0xC003, 0xE1B25318EECAF954),
            lo: L::from_bits(0x3FC0, 0x9215C5B4D9B91081),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x3FFE, 0x928F740000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC003, 0xD3EE731B6DB6DB6E),
            lo: L::from_bits(0x3FC2, 0x9249249249249249),
        },
        DD {
            hi: L::from_bits(0x4006, 0xDA30C560AEE487E2),
            lo: L::from_bits(0x3FC3, 0xBDD8AA577243F10C),
        },
        DD {
            hi: L::from_bits(0xC008, 0xAEE5189D6C16C16C),
            lo: L::from_bits(0xBFC5, 0xB60B60B60B60B60B),
        },
        DD {
            hi: L::from_bits(0x4009, 0x847FB1C980000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC008, 0xBF502870226A0D58),
            lo: L::from_bits(0xBFC6, 0x9215C5B4D9B91081),
        },
        DD {
            hi: L::from_bits(0x4006, 0xD491F40AD0E79D0D),
            lo: L::from_bits(0xBFC5, 0xCB493CD46A992FB8),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x3FFF, 0xDD262CC000000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC005, 0xD82E8D09DB6DB6DB),
            lo: L::from_bits(0xBFC4, 0xDB6DB6DB6DB6DB6E),
        },
        DD {
            hi: L::from_bits(0x4009, 0x961CE4AA41EB851F),
            lo: L::from_bits(0xBFC8, 0x8F5C28F5C28F5C29),
        },
        DD {
            hi: L::from_bits(0xC00B, 0xA5CD2D031F8E38E4),
            lo: L::from_bits(0x3FCA, 0xE38E38E38E38E38E),
        },
        DD {
            hi: L::from_bits(0x400C, 0xB61D92C6E625ED09),
            lo: L::from_bits(0x3FCB, 0xF684BDA12F684BDA),
        },
        DD {
            hi: L::from_bits(0xC00C, 0xD44A3334E2FCD6EA),
            lo: L::from_bits(0x3FC9, 0xFCD6E9E06522C3F3),
        },
        DD {
            hi: L::from_bits(0x400B, 0xFBEDC70737FC1921),
            lo: L::from_bits(0xBFCA, 0x86ECFF7E2589265B),
        },
        DD {
            hi: L::from_bits(0xC009, 0xEFEEA52B72456D44),
            lo: L::from_bits(0x3FC8, 0x8080304760B3617B),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4001, 0xC25E8D54C0000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC007, 0xF6F528B4F1249249),
            lo: L::from_bits(0xBFC5, 0x9249249249249249),
        },
        DD {
            hi: L::from_bits(0x400B, 0xDE2C1D4A9FA08C6F),
            lo: L::from_bits(0x3FC9, 0xB564EFE898231BCB),
        },
        DD {
            hi: L::from_bits(0xC00E, 0xA0E8A7AC0AAAE148),
            lo: L::from_bits(0x3FCD, 0xA3D70A3D70A3D70A),
        },
        DD {
            hi: L::from_bits(0x400F, 0xEEAC3B84904297B4),
            lo: L::from_bits(0x3FCD, 0x97B425ED097B425F),
        },
        DD {
            hi: L::from_bits(0xC010, 0xC6A20B588FF4BC3A),
            lo: L::from_bits(0x3FCF, 0xD14B802CF301C17E),
        },
        DD {
            hi: L::from_bits(0x4010, 0xBC08C014319CA7DB),
            lo: L::from_bits(0x3FCF, 0xF51D25932377BF63),
        },
        DD {
            hi: L::from_bits(0xC00F, 0xBD6A4C97FFB6358F),
            lo: L::from_bits(0xBFCD, 0xBD1C009280519316),
        },
        DD {
            hi: L::from_bits(0x400D, 0x9DD895295517D74D),
            lo: L::from_bits(0xBFCC, 0xB13455184A88AD61),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4003, 0xC30B5327B6000000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC00A, 0x9C3D49A74BB11746),
            lo: L::from_bits(0x3FC8, 0xBA2E8BA2E8BA2E8C),
        },
        DD {
            hi: L::from_bits(0x400E, 0xB0A2C4DBF66C2492),
            lo: L::from_bits(0x3FCD, 0x9249249249249249),
        },
        DD {
            hi: L::from_bits(0xC011, 0xA1EFA584FE58F436),
            lo: L::from_bits(0xBFCF, 0xE49886F9136840C1),
        },
        DD {
            hi: L::from_bits(0x4013, 0x9AD46A2FC33F7DD0),
            lo: L::from_bits(0x3FD1, 0xDA740DA740DA740E),
        },
        DD {
            hi: L::from_bits(0xC014, 0xABB9ECE8064CE6E1),
            lo: L::from_bits(0x3FD3, 0x81EE7113506AC124),
        },
        DD {
            hi: L::from_bits(0x4014, 0xE5B11D30CCD72270),
            lo: L::from_bits(0x3FD2, 0x8E56DAE4B9E24498),
        },
        DD {
            hi: L::from_bits(0xC014, 0xB6FBFFAC9540E27C),
            lo: L::from_bits(0xBFD3, 0xA021B641511E8D2B),
        },
        DD {
            hi: L::from_bits(0x4013, 0xA0209CEAD46C491C),
            lo: L::from_bits(0x3FD2, 0xEC6F361341FD25B7),
        },
        DD {
            hi: L::from_bits(0xC010, 0xED39CC069008B82A),
            lo: L::from_bits(0xBFCE, 0xA9955DDA3EC823E4),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4005, 0xDC08C69BFFB80000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC00C, 0xD8F85BE865FD88BA),
            lo: L::from_bits(0xBFCA, 0xBA2E8BA2E8BA2E8C),
        },
        DD {
            hi: L::from_bits(0x4011, 0x967B4CF296411191),
            lo: L::from_bits(0x3FCE, 0xC9615D90DC60E3FA),
        },
        DD {
            hi: L::from_bits(0xC014, 0xAA054883291877CC),
            lo: L::from_bits(0xBFD3, 0xEAD65B7A32846FF5),
        },
        DD {
            hi: L::from_bits(0x4016, 0xCAC53F2A9A9ECB11),
            lo: L::from_bits(0xBFD4, 0xA252ADB363BEC475),
        },
        DD {
            hi: L::from_bits(0xC018, 0x8F4EB22A49F25E11),
            lo: L::from_bits(0x3FD5, 0xDF8297736BD60755),
        },
        DD {
            hi: L::from_bits(0x4018, 0xFD15901195290E32),
            lo: L::from_bits(0x3FD7, 0xC3B76CFA7F971E51),
        },
        DD {
            hi: L::from_bits(0xC019, 0x8D582786C4E022E2),
            lo: L::from_bits(0xBFD6, 0xA7EF74C83E1E0B51),
        },
        DD {
            hi: L::from_bits(0x4018, 0xC25E669F87D1478B),
            lo: L::from_bits(0x3FD1, 0x9FD1CD5AA3CCA6D8),
        },
        DD {
            hi: L::from_bits(0xC017, 0x9659E18F28C986B9),
            lo: L::from_bits(0x3FD6, 0x8D428AA236DAD3A0),
        },
        DD {
            hi: L::from_bits(0x4014, 0xC877D7698BB75E4C),
            lo: L::from_bits(0xBFD3, 0xBC58B8D84923C4D6),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4008, 0x89D57F5272BBA000),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC00F, 0xA412B7804DCBF24E),
            lo: L::from_bits(0xBFCE, 0xA4B3055EE19101CA),
        },
        DD {
            hi: L::from_bits(0x4014, 0x88F2E0B6314D7617),
            lo: L::from_bits(0x3FD3, 0x9CBC14E5E0A72F05),
        },
        DD {
            hi: L::from_bits(0xC017, 0xBAB8E75CE501B41B),
            lo: L::from_bits(0x3FD5, 0x8A7849E52AC1D977),
        },
        DD {
            hi: L::from_bits(0x401A, 0x877B53BCC2B21015),
            lo: L::from_bits(0xBFD8, 0xCC364F574E3408CC),
        },
        DD {
            hi: L::from_bits(0xC01B, 0xEC756BC23343173D),
            lo: L::from_bits(0xBFDA, 0x84D41278259CE622),
        },
        DD {
            hi: L::from_bits(0x401D, 0x83F22981A5622703),
            lo: L::from_bits(0xBFDC, 0xC2759203CAE75920),
        },
        DD {
            hi: L::from_bits(0xC01D, 0xC13F73D03777F506),
            lo: L::from_bits(0xBFDC, 0x8A739297A02BB35E),
        },
        DD {
            hi: L::from_bits(0x401D, 0xB93403C7242311B2),
            lo: L::from_bits(0xBFDA, 0xD101620B6FCE2032),
        },
        DD {
            hi: L::from_bits(0xC01C, 0xDFFC3B5EB9EFB052),
            lo: L::from_bits(0xBFDB, 0xC4B1E0BAF6FE15D0),
        },
        DD {
            hi: L::from_bits(0x401B, 0x9B3ECE917C72C0C6),
            lo: L::from_bits(0x3FD8, 0x90912B6A05703346),
        },
        DD {
            hi: L::from_bits(0xC018, 0xBC2D196A8754CAA3),
            lo: L::from_bits(0x3FD7, 0xFAFACC0E6AF545CC),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x400A, 0xBDE172BB94B923C0),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC012, 0x863D253DBC70AFCB),
            lo: L::from_bits(0xBFD0, 0xE525982AF70C880E),
        },
        DD {
            hi: L::from_bits(0x4017, 0x84B6D1C6E8C1A9B2),
            lo: L::from_bits(0xBFD5, 0xB23705844AF837D3),
        },
        DD {
            hi: L::from_bits(0xC01A, 0xD6AD6FDE3B0B83AC),
            lo: L::from_bits(0x3FD9, 0x99AB7AB7AB7AB7AB),
        },
        DD {
            hi: L::from_bits(0x401D, 0xB9E171F1C22E0A71),
            lo: L::from_bits(0x3FDB, 0xCAC7C7AB95591408),
        },
        DD {
            hi: L::from_bits(0xC01F, 0xC399F5304F44B581),
            lo: L::from_bits(0x3FDE, 0xFC01797544A69706),
        },
        DD {
            hi: L::from_bits(0x4021, 0x85C4F1EC64FAAD05),
            lo: L::from_bits(0xBFDD, 0xFB8B4CFF6666E9E9),
        },
        DD {
            hi: L::from_bits(0xC021, 0xF6113D68B99F8622),
            lo: L::from_bits(0xBFE0, 0xF5D6E05327F672C3),
        },
        DD {
            hi: L::from_bits(0x4022, 0x99C7DA4EBC1046FE),
            lo: L::from_bits(0x3FE1, 0xEA69FFF9E551EF34),
        },
        DD {
            hi: L::from_bits(0xC022, 0x8103B0B7C28A5AB8),
            lo: L::from_bits(0xBFE0, 0x8C54C829E4AE2B2F),
        },
        DD {
            hi: L::from_bits(0x4021, 0x8B3CED52A97765D4),
            lo: L::from_bits(0xBFDF, 0x8C5E9F5C283FEBBF),
        },
        DD {
            hi: L::from_bits(0xC01F, 0xAED5B3AA06A2EE11),
            lo: L::from_bits(0xBFDE, 0xF9EA1582CDECD11A),
        },
        DD {
            hi: L::from_bits(0x401C, 0xC242C7A07926CFA2),
            lo: L::from_bits(0xBFDB, 0xB16DAF35C5DC6CFF),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x400D, 0x8EA382CD86CC4EDB),
            lo: L::from_bits(0x0000, 0x0000000000000000),
        },
        DD {
            hi: L::from_bits(0xC014, 0xEC5165C531C1453E),
            lo: L::from_bits(0xBFD3, 0xA879BBF8D6D33EA8),
        },
        DD {
            hi: L::from_bits(0x401A, 0x8886A74B8091BB82),
            lo: L::from_bits(0xBFD9, 0xCE0E0404D2964DF7),
        },
        DD {
            hi: L::from_bits(0xC01E, 0x812C5037394AA2A0),
            lo: L::from_bits(0x3FDD, 0xAA917A5312E809EE),
        },
        DD {
            hi: L::from_bits(0x4021, 0x8363944DDAB811B6),
            lo: L::from_bits(0x3FDF, 0xE6465687B9098F3B),
        },
        DD {
            hi: L::from_bits(0xC023, 0xA3AA48F5902FA5EA),
            lo: L::from_bits(0xBFE2, 0x84540C90B9AF7201),
        },
        DD {
            hi: L::from_bits(0x4025, 0x86106E93644D19F7),
            lo: L::from_bits(0x3FE2, 0x80F1A4FB4CCC70BE),
        },
        DD {
            hi: L::from_bits(0xC026, 0x9642BE687D606285),
            lo: L::from_bits(0x3FE4, 0x87124730CF175674),
        },
        DD {
            hi: L::from_bits(0x4026, 0xEABAE7E4F161D0F6),
            lo: L::from_bits(0x3FE5, 0xF2BB0280E8BCB972),
        },
        DD {
            hi: L::from_bits(0xC026, 0xFFBC42D171392AA3),
            lo: L::from_bits(0x3FE4, 0x83231DA0D4E46FA6),
        },
        DD {
            hi: L::from_bits(0x4026, 0xBEBD3CDFEC93CBCF),
            lo: L::from_bits(0x3FE5, 0xA2E2406DD843B336),
        },
        DD {
            hi: L::from_bits(0xC025, 0xB9D7F8B4F0EED8D2),
            lo: L::from_bits(0x3FE2, 0xA9DCDC71AEC6C557),
        },
        DD {
            hi: L::from_bits(0x4023, 0xD54503A92034DBD9),
            lo: L::from_bits(0x3FE1, 0xE2D1DD973B0E4F79),
        },
        DD {
            hi: L::from_bits(0xC020, 0xDABCF00FEC84FBBE),
            lo: L::from_bits(0x3FDB, 0xEF6D691A012EAEFA),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x400F, 0xE81B368F950FE29A),
            lo: L::from_bits(0x3FCC, 0xA000000000000000),
        },
        DD {
            hi: L::from_bits(0xC017, 0xDEB0920FA4EE7E3B),
            lo: L::from_bits(0x3FD6, 0xB0DCC157B8644073),
        },
        DD {
            hi: L::from_bits(0x401D, 0x94A7B41B0681F7BB),
            lo: L::from_bits(0xBFDA, 0xD25D3A2E8BA2E8BA),
        },
        DD {
            hi: L::from_bits(0xC021, 0xA297EE71B0EF669A),
            lo: L::from_bits(0xBFE0, 0x823BB8E6B6A5EE07),
        },
        DD {
            hi: L::from_bits(0x4024, 0xBFC5D7E101A3DB27),
            lo: L::from_bits(0x3FE1, 0xEE7E4882155CEC18),
        },
        DD {
            hi: L::from_bits(0xC027, 0x8B4C0E0022173EA3),
            lo: L::from_bits(0x3FE6, 0xC8FE21DE9FE5AB86),
        },
        DD {
            hi: L::from_bits(0x4029, 0x863F7C362E243912),
            lo: L::from_bits(0x3FE8, 0xE4A30A70B2F5B59D),
        },
        DD {
            hi: L::from_bits(0xC02A, 0xB349681FA7E49835),
            lo: L::from_bits(0x3FE8, 0x8DC65C6E747F200E),
        },
        DD {
            hi: L::from_bits(0x402B, 0xA9E1B78F492EBA3E),
            lo: L::from_bits(0xBFEA, 0xF4733709DDBA5F98),
        },
        DD {
            hi: L::from_bits(0xC02B, 0xE6688C8782894469),
            lo: L::from_bits(0x3FEA, 0xE291E35E9C78C517),
        },
        DD {
            hi: L::from_bits(0x402B, 0xDE63FB97D1FA909D),
            lo: L::from_bits(0xBFE8, 0xFAEE8BC3538403DF),
        },
        DD {
            hi: L::from_bits(0xC02B, 0x9547B4029FB37FC7),
            lo: L::from_bits(0x3FEA, 0x8BC2CAF80F84EAB4),
        },
        DD {
            hi: L::from_bits(0x402A, 0x848EF0BA4D7E6220),
            lo: L::from_bits(0xBFE9, 0xD1E3F9FD5F8709E3),
        },
        DD {
            hi: L::from_bits(0xC028, 0x8C10A204FF2040E6),
            lo: L::from_bits(0xBFE3, 0x9EAFDDACD2CAFB68),
        },
        DD {
            hi: L::from_bits(0x4025, 0x85652C970B5BAB86),
            lo: L::from_bits(0xBFE4, 0x88D70E3BCEE1A1CC),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4012, 0xCB55B4DD402F3FD9),
            lo: L::from_bits(0xBFD1, 0xD7A0000000000000),
        },
        DD {
            hi: L::from_bits(0xC01A, 0xDFB120B85B246A33),
            lo: L::from_bits(0xBFD8, 0xCFD21931CF5931CF),
        },
        DD {
            hi: L::from_bits(0x4020, 0xAADA6244DCAFEAE6),
            lo: L::from_bits(0x3FDF, 0xEAB1D2D08F377F1B),
        },
        DD {
            hi: L::from_bits(0xC024, 0xD5D9864EA0FC6227),
            lo: L::from_bits(0x3FE3, 0xBDCC36FAB121310B),
        },
        DD {
            hi: L::from_bits(0x4028, 0x90A5663EC88575F1),
            lo: L::from_bits(0x3FE7, 0xA1B1116BA8F5C28F),
        },
        DD {
            hi: L::from_bits(0xC02A, 0xF2118D1919C7B034),
            lo: L::from_bits(0xBFE9, 0xA9938D2086B45179),
        },
        DD {
            hi: L::from_bits(0x402D, 0x8748AF24C40E4A36),
            lo: L::from_bits(0xBFEC, 0x8C422FD70E2CAD4B),
        },
        DD {
            hi: L::from_bits(0xC02E, 0xD38D885607CBECDC),
            lo: L::from_bits(0xBFE9, 0x818776AF81DA92FA),
        },
        DD {
            hi: L::from_bits(0x402F, 0xEDDB9A3CB00653EA),
            lo: L::from_bits(0x3FE9, 0xEC59E6832FA801F8),
        },
        DD {
            hi: L::from_bits(0xC030, 0xC2F6CD11E65BC34A),
            lo: L::from_bits(0xBFED, 0x9A412FE2E0891E24),
        },
        DD {
            hi: L::from_bits(0x4030, 0xE980A8EA6A929F49),
            lo: L::from_bits(0xBFEF, 0x8D252F7A53B97F15),
        },
        DD {
            hi: L::from_bits(0xC030, 0xCA3F894858909C47),
            lo: L::from_bits(0x3FEF, 0xAD654A98B5D3CF66),
        },
        DD {
            hi: L::from_bits(0x402F, 0xF6CF3677F305DC82),
            lo: L::from_bits(0xBFEE, 0xFF734F89CED83DEB),
        },
        DD {
            hi: L::from_bits(0xC02E, 0xC950FA9605BD322B),
            lo: L::from_bits(0x3FEC, 0xCB9DFC8B02450433),
        },
        DD {
            hi: L::from_bits(0x402C, 0xC518BD222C88322E),
            lo: L::from_bits(0x3FEB, 0xB0D03F3821931EB0),
        },
        DD {
            hi: L::from_bits(0xC029, 0xAF326F3AD2402C9B),
            lo: L::from_bits(0x3FE4, 0xD7FC7CE1B0B72F1A),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4015, 0xBED32EFCA37C57AB),
            lo: L::from_bits(0x3FCE, 0x8906000000000000),
        },
        DD {
            hi: L::from_bits(0xC01D, 0xEE9D6AA0623081E0),
            lo: L::from_bits(0xBFDB, 0x8B24939FDAEE9FDB),
        },
        DD {
            hi: L::from_bits(0x4023, 0xCEBFAFEE996E9E42),
            lo: L::from_bits(0x3FE2, 0xEAE88C01F66AC7DF),
        },
        DD {
            hi: L::from_bits(0xC028, 0x92C626382836B21E),
            lo: L::from_bits(0x3FE7, 0xD2175E169AB3703A),
        },
        DD {
            hi: L::from_bits(0x402B, 0xE19BD9E41203B0FB),
            lo: L::from_bits(0x3FEA, 0xFDFDD8BD91E7288E),
        },
        DD {
            hi: L::from_bits(0xC02E, 0xD73BF60344C1406E),
            lo: L::from_bits(0xBFEC, 0xDE23FB373098839C),
        },
        DD {
            hi: L::from_bits(0x4031, 0x89DD82D1481D6D54),
            lo: L::from_bits(0x3FEF, 0xB88ECDB212001D33),
        },
        DD {
            hi: L::from_bits(0xC032, 0xF8EF940F38D6F701),
            lo: L::from_bits(0xBFF0, 0x96D17E58ABCB5AD4),
        },
        DD {
            hi: L::from_bits(0x4034, 0xA33C65845BA56070),
            lo: L::from_bits(0x3FF3, 0xA66A446B468B9198),
        },
        DD {
            hi: L::from_bits(0xC035, 0x9E39AA61C538CD20),
            lo: L::from_bits(0x3FF3, 0xA085DC63915B0C69),
        },
        DD {
            hi: L::from_bits(0x4035, 0xE45B2EA227D60266),
            lo: L::from_bits(0x3FF4, 0xCE6C6437784DB3E2),
        },
        DD {
            hi: L::from_bits(0xC035, 0xF4C3239B3A713165),
            lo: L::from_bits(0xBFF4, 0x8B518C9D3CFAD432),
        },
        DD {
            hi: L::from_bits(0x4035, 0xC03C3BB1D030B27B),
            lo: L::from_bits(0x3FF4, 0xCC6D0A2301D8BF41),
        },
        DD {
            hi: L::from_bits(0xC034, 0xD701FCCDDA9144B9),
            lo: L::from_bits(0xBFF2, 0xBF276C8C6EE7BD44),
        },
        DD {
            hi: L::from_bits(0x4033, 0xA22B077608F1B67A),
            lo: L::from_bits(0x3FF0, 0xCAEDC8757783C1CF),
        },
        DD {
            hi: L::from_bits(0xC031, 0x93E8742788C06DA8),
            lo: L::from_bits(0xBFF0, 0xAB5A7F711F6AF596),
        },
        DD {
            hi: L::from_bits(0x402D, 0xF6836C41E3EB616E),
            lo: L::from_bits(0xBFEA, 0xDEFA03B883E19AC4),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4018, 0xBF0015620C1B47C0),
            lo: L::from_bits(0xBFD7, 0xBC71FDA000000000),
        },
        DD {
            hi: L::from_bits(0xC021, 0x86B1780C150EDBA4),
            lo: L::from_bits(0x3FDC, 0xE2EC9CD899742456),
        },
        DD {
            hi: L::from_bits(0x4027, 0x83662761A16FE1CF),
            lo: L::from_bits(0xBFE6, 0xE2ABFE963DC06518),
        },
        DD {
            hi: L::from_bits(0xC02B, 0xD203164F9DB61500),
            lo: L::from_bits(0xBFE8, 0xFE62275323C9CF34),
        },
        DD {
            hi: L::from_bits(0x402F, 0xB5EB4DE0D22E1EEA),
            lo: L::from_bits(0xBFEC, 0xB9C55048AE5E37CA),
        },
        DD {
            hi: L::from_bits(0xC032, 0xC421B7AC591807DF),
            lo: L::from_bits(0xBFF1, 0x8F98EB9BD3F1D06F),
        },
        DD {
            hi: L::from_bits(0x4035, 0x8E90B27C0B7C359A),
            lo: L::from_bits(0xBFF4, 0xB272F56106A33A14),
        },
        DD {
            hi: L::from_bits(0xC037, 0x92EBE7FF98B0C965),
            lo: L::from_bits(0x3FF5, 0xEC56AF0DA83F9836),
        },
        DD {
            hi: L::from_bits(0x4038, 0xDDB14CB205A86846),
            lo: L::from_bits(0x3FF7, 0xB9FFD3F6F67A1F20),
        },
        DD {
            hi: L::from_bits(0xC039, 0xF9DBD42B71025F8D),
            lo: L::from_bits(0x3FF8, 0xB9C447497E9C3401),
        },
        DD {
            hi: L::from_bits(0x403A, 0xD49D7A7692B8FCA6),
            lo: L::from_bits(0xBFF9, 0xB10864EEC7A08F76),
        },
        DD {
            hi: L::from_bits(0xC03B, 0x88F55A645DC253F2),
            lo: L::from_bits(0x3FF8, 0xB92CD6A9401CC03F),
        },
        DD {
            hi: L::from_bits(0x403B, 0x84CE060DB8497609),
            lo: L::from_bits(0xBFFA, 0xE79781F4B4F6A572),
        },
        DD {
            hi: L::from_bits(0xC03A, 0xBECB26A2E2AC62F1),
            lo: L::from_bits(0x3FF7, 0x8F1A0070FD90A96A),
        },
        DD {
            hi: L::from_bits(0x4039, 0xC4F1C9859DB74013),
            lo: L::from_bits(0x3FF7, 0xD535E85E3C117D3E),
        },
        DD {
            hi: L::from_bits(0xC038, 0x8A2363E66E2B7468),
            lo: L::from_bits(0x3FF6, 0xF4C8B267CCC52730),
        },
        DD {
            hi: L::from_bits(0x4035, 0xEBD9498C8A2F2C30),
            lo: L::from_bits(0x3FF4, 0xEC91434C2C4E585A),
        },
        DD {
            hi: L::from_bits(0xC032, 0xB8FABC31FDF2CD53),
            lo: L::from_bits(0xBFF0, 0xBE615A8186C12B38),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x401B, 0xCB1A888409C33B2F),
            lo: L::from_bits(0x3FD8, 0xDE74085300000000),
        },
        DD {
            hi: L::from_bits(0xC024, 0xA0738664F566E924),
            lo: L::from_bits(0xBFDB, 0xE785D23AE40FD9F5),
        },
        DD {
            hi: L::from_bits(0x402A, 0xAF0FAC7CAF331151),
            lo: L::from_bits(0x3FE9, 0xF7A2EDDB16F51B57),
        },
        DD {
            hi: L::from_bits(0xC02F, 0x9C6AD4E6F251F947),
            lo: L::from_bits(0x3FEE, 0x9DCF054F5DF2FDB8),
        },
        DD {
            hi: L::from_bits(0x4033, 0x97A203226FEB3567),
            lo: L::from_bits(0x3FF2, 0xE01A9DEC046159F0),
        },
        DD {
            hi: L::from_bits(0xC036, 0xB7568F10660BAB70),
            lo: L::from_bits(0x3FF5, 0x804EB6D65C89B834),
        },
        DD {
            hi: L::from_bits(0x4039, 0x95F45D4EE0DB1C4A),
            lo: L::from_bits(0xBFF8, 0xDF58F759D61AA1CA),
        },
        DD {
            hi: L::from_bits(0xC03B, 0xAEB5A33C282FD7FF),
            lo: L::from_bits(0xBFF9, 0xD36547D985FCE0C0),
        },
        DD {
            hi: L::from_bits(0x403D, 0x95F5E0877ADE387E),
            lo: L::from_bits(0x3FFC, 0xE32F9B9C3FE3A039),
        },
        DD {
            hi: L::from_bits(0xC03E, 0xC1E243C71862B11F),
            lo: L::from_bits(0x3FFD, 0x8CF0F6063A2456F1),
        },
        DD {
            hi: L::from_bits(0x403F, 0xBF532F50C48BA095),
            lo: L::from_bits(0xBFFE, 0x8621314B46BA2BDF),
        },
        DD {
            hi: L::from_bits(0xC040, 0x90FBBEDBBFDDCE34),
            lo: L::from_bits(0xBFFF, 0xC637EBB78B2587D4),
        },
        DD {
            hi: L::from_bits(0x4040, 0xA89CF668795F815E),
            lo: L::from_bits(0xBFFD, 0xEE1623969B31A805),
        },
        DD {
            hi: L::from_bits(0xC040, 0x953D7D4E17124ECC),
            lo: L::from_bits(0x3FFE, 0x8FBEC09DAF123F21),
        },
        DD {
            hi: L::from_bits(0x403F, 0xC5845FDD26A3BAB0),
            lo: L::from_bits(0x3FFE, 0x815DAED6241AF36B),
        },
        DD {
            hi: L::from_bits(0xC03E, 0xBD48F49942550C01),
            lo: L::from_bits(0x3FFC, 0xB24ECBEED78D4DE5),
        },
        DD {
            hi: L::from_bits(0x403C, 0xF825605EDA3FFA65),
            lo: L::from_bits(0x3FFA, 0xB517AF0AE5539789),
        },
        DD {
            hi: L::from_bits(0xC03A, 0xC71675661AD78BA3),
            lo: L::from_bits(0x3FF9, 0xE399892212857961),
        },
        DD {
            hi: L::from_bits(0x4037, 0x9378EEAA72B2A052),
            lo: L::from_bits(0x3FF6, 0xF897C05889FBF1F1),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x401E, 0xE4A89BCC3AFDB0BC),
            lo: L::from_bits(0x3FDD, 0xCF0821B619000000),
        },
        DD {
            hi: L::from_bits(0xC027, 0xC921978B207FF05A),
            lo: L::from_bits(0x3FE3, 0xBCCF2A05E8DDD534),
        },
        DD {
            hi: L::from_bits(0x402D, 0xF3F86EEA8A7CC557),
            lo: L::from_bits(0x3FEC, 0xCD510530AB53EBCE),
        },
        DD {
            hi: L::from_bits(0xC032, 0xF243530279512569),
            lo: L::from_bits(0xBFF0, 0xB4494D735215183F),
        },
        DD {
            hi: L::from_bits(0x4037, 0x82977EDA30438407),
            lo: L::from_bits(0xBFF6, 0xDF03216CAD7EE21F),
        },
        DD {
            hi: L::from_bits(0xC03A, 0xAFE55F8E42FEFEDB),
            lo: L::from_bits(0xBFF9, 0x9D94B54ACF006E4F),
        },
        DD {
            hi: L::from_bits(0x403D, 0xA0B3A050B1221696),
            lo: L::from_bits(0x3FFB, 0xBCDFC55C95B0FA94),
        },
        DD {
            hi: L::from_bits(0xC03F, 0xD1F0DBF54D8A0FD3),
            lo: L::from_bits(0x3FFE, 0xD189F00595190DB8),
        },
        DD {
            hi: L::from_bits(0x4041, 0xCB17B3C446CDD8FF),
            lo: L::from_bits(0x3FFF, 0xD18D094F452B916D),
        },
        DD {
            hi: L::from_bits(0xC043, 0x94F3F5248C591FC7),
            lo: L::from_bits(0xC001, 0x98B5D55A6F416D4C),
        },
        DD {
            hi: L::from_bits(0x4044, 0xA8325CB75B01923F),
            lo: L::from_bits(0xBFFE, 0xA4C01D3E2C9BA972),
        },
        DD {
            hi: L::from_bits(0xC045, 0x93793D218B613B73),
            lo: L::from_bits(0xC003, 0x8953B32E06A39C09),
        },
        DD {
            hi: L::from_bits(0x4045, 0xC959209CCDF7D4F3),
            lo: L::from_bits(0x4003, 0xCC3D20D51D0F507B),
        },
        DD {
            hi: L::from_bits(0xC045, 0xD55754F8694CFE72),
            lo: L::from_bits(0x4004, 0xEE76094615FA51D4),
        },
        DD {
            hi: L::from_bits(0x4045, 0xADA7D2EE525D86BE),
            lo: L::from_bits(0xC004, 0xF47A5804DDD34D60),
        },
        DD {
            hi: L::from_bits(0xC044, 0xD50896B726BC7A76),
            lo: L::from_bits(0xC003, 0xD12B23086144EE23),
        },
        DD {
            hi: L::from_bits(0x4043, 0xBE81CA5A287E7130),
            lo: L::from_bits(0xC001, 0xC85E01A02D91C717),
        },
        DD {
            hi: L::from_bits(0xC041, 0xEA661C810468ABA4),
            lo: L::from_bits(0xBFFD, 0xADE200109E0DA3F3),
        },
        DD {
            hi: L::from_bits(0x403F, 0xB1626FACC5364EB1),
            lo: L::from_bits(0xBFFD, 0xBC2B56549ADB4139),
        },
        DD {
            hi: L::from_bits(0xC03B, 0xF8F5F211EC5E2F91),
            lo: L::from_bits(0x3FF9, 0x8A57C58437A87F79),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4022, 0x87DAFA2A710C871B),
            lo: L::from_bits(0x3FE0, 0x8E03100EFBB50000),
        },
        DD {
            hi: L::from_bits(0xC02B, 0x84546A588F784C44),
            lo: L::from_bits(0x3FEA, 0x87A921271B2CC354),
        },
        DD {
            hi: L::from_bits(0x4031, 0xB1802BD2463268E9),
            lo: L::from_bits(0xBFEF, 0xFE32DC5311A8C1A7),
        },
        DD {
            hi: L::from_bits(0xC036, 0xC2D4FEBCF52B008E),
            lo: L::from_bits(0xBFF4, 0x86B6710EF0A21B0F),
        },
        DD {
            hi: L::from_bits(0x403A, 0xE84BFFE780BC91B8),
            lo: L::from_bits(0x3FF7, 0xFC457F49E6F78CEA),
        },
        DD {
            hi: L::from_bits(0xC03E, 0xAD3F03E9DBFB1680),
            lo: L::from_bits(0xBFFD, 0xA9BDD98DE4B07232),
        },
        DD {
            hi: L::from_bits(0x4041, 0xAFABE95109C998D8),
            lo: L::from_bits(0xC000, 0x98C1B5106E3A7695),
        },
        DD {
            hi: L::from_bits(0xC043, 0xFF840E17D375BF3D),
            lo: L::from_bits(0xC000, 0x9E1CED7CF45E4037),
        },
        DD {
            hi: L::from_bits(0x4046, 0x8A2DF104D0215CC7),
            lo: L::from_bits(0xC002, 0xB7466E3F5330B4CF),
        },
        DD {
            hi: L::from_bits(0xC047, 0xE3D7C20D9ACA541A),
            lo: L::from_bits(0x4004, 0xDD0942918527A3F1),
        },
        DD {
            hi: L::from_bits(0x4049, 0x919AB91122E4F784),
            lo: L::from_bits(0xC008, 0xD03908C9D80AC422),
        },
        DD {
            hi: L::from_bits(0xC04A, 0x91C6894BC228EEAC),
            lo: L::from_bits(0xC005, 0xBC7FCE5A31AB7BC8),
        },
        DD {
            hi: L::from_bits(0x404A, 0xE5D6E2B1CD989806),
            lo: L::from_bits(0x4008, 0xD45E35EDC74E55B3),
        },
        DD {
            hi: L::from_bits(0xC04B, 0x8EB1E7D6B4640DD2),
            lo: L::from_bits(0x400A, 0xF689D63187AF8835),
        },
        DD {
            hi: L::from_bits(0x404B, 0x8AD13EF42159C094),
            lo: L::from_bits(0x4009, 0xA457E4A9AE3DE880),
        },
        DD {
            hi: L::from_bits(0xC04A, 0xD128078BE3F07FCA),
            lo: L::from_bits(0x4009, 0xC8F416CA7F5998FB),
        },
        DD {
            hi: L::from_bits(0x4049, 0xEF189ADBF844F36D),
            lo: L::from_bits(0x4008, 0xFAFEADBB7B467B6F),
        },
        DD {
            hi: L::from_bits(0xC048, 0xC8694AAD9262CA84),
            lo: L::from_bits(0x4004, 0xA1C5DA91A7A17841),
        },
        DD {
            hi: L::from_bits(0x4046, 0xE84E6E02DC98E859),
            lo: L::from_bits(0x4004, 0xE79A17512A4D698B),
        },
        DD {
            hi: L::from_bits(0xC044, 0xA65BD8111A01AFA9),
            lo: L::from_bits(0x4003, 0x8618A335D4B883D7),
        },
        DD {
            hi: L::from_bits(0x4040, 0xDDCFCAC178023F8C),
            lo: L::from_bits(0xBFFF, 0xB2CB8447C64B5A74),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4025, 0xA9EB994639F77A82),
            lo: L::from_bits(0xBFE3, 0xFCDB06C5A4093C00),
        },
        DD {
            hi: L::from_bits(0xC02E, 0xB66039B4592F1F1D),
            lo: L::from_bits(0xBFED, 0xA94D920C3C01BA3A),
        },
        DD {
            hi: L::from_bits(0x4035, 0x869B0FF3209AB3BA),
            lo: L::from_bits(0xBFF3, 0xCBF31C5BCD61F104),
        },
        DD {
            hi: L::from_bits(0xC03A, 0xA2868C859C62CC29),
            lo: L::from_bits(0xBFF3, 0xEB2152B4AC9B7011),
        },
        DD {
            hi: L::from_bits(0x403E, 0xD53B89075E3C69B5),
            lo: L::from_bits(0xBFFD, 0xCFF104E9F4960DC0),
        },
        DD {
            hi: L::from_bits(0xC042, 0xAF2F17F89B0702DF),
            lo: L::from_bits(0xBFFF, 0xBCD73560B8C65E24),
        },
        DD {
            hi: L::from_bits(0x4045, 0xC40A814EEA00BC2E),
            lo: L::from_bits(0xC004, 0xD55C68A9DE621B65),
        },
        DD {
            hi: L::from_bits(0xC048, 0x9DC137B5022A421F),
            lo: L::from_bits(0xC006, 0xFCCDEB403A893CB2),
        },
        DD {
            hi: L::from_bits(0x404A, 0xBD7352FD53910650),
            lo: L::from_bits(0xC009, 0xA29039A22F3A45AA),
        },
        DD {
            hi: L::from_bits(0xC04C, 0xAE33BA26446F2254),
            lo: L::from_bits(0xC00B, 0xB6740F0E17FF8641),
        },
        DD {
            hi: L::from_bits(0x404D, 0xF9BA73CB009A4B22),
            lo: L::from_bits(0x400C, 0xE830C2EE54514CD5),
        },
        DD {
            hi: L::from_bits(0xC04F, 0x8D367EF11E02A1EA),
            lo: L::from_bits(0xC00D, 0xEE8891A99D6BEB66),
        },
        DD {
            hi: L::from_bits(0x404F, 0xFDC353FCDF000BA9),
            lo: L::from_bits(0x400E, 0xE9038FF277CB7708),
        },
        DD {
            hi: L::from_bits(0xC050, 0xB5A0FF8532199C2E),
            lo: L::from_bits(0xC00E, 0xD912E12F5ACFDA72),
        },
        DD {
            hi: L::from_bits(0x4050, 0xCEBE32A3FDB91763),
            lo: L::from_bits(0x400C, 0xD4FC6BE367DEBB82),
        },
        DD {
            hi: L::from_bits(0xC050, 0xB9E03B25F2F727DC),
            lo: L::from_bits(0xC00F, 0xE022ADD1F251B491),
        },
        DD {
            hi: L::from_bits(0x4050, 0x824FA0C904294EBB),
            lo: L::from_bits(0x400D, 0xE6614350F1260F0A),
        },
        DD {
            hi: L::from_bits(0xC04F, 0x8B73FC14435C950C),
            lo: L::from_bits(0xC00E, 0xBF78FEBF6ED73743),
        },
        DD {
            hi: L::from_bits(0x404D, 0xDBFC985FB9768AAF),
            lo: L::from_bits(0x400B, 0xBFD8486BBE51A9FE),
        },
        DD {
            hi: L::from_bits(0xC04B, 0xF109D3894B88E9CC),
            lo: L::from_bits(0xC006, 0xD1FF5AEC340AF019),
        },
        DD {
            hi: L::from_bits(0x4049, 0xA3D045AD81B1290E),
            lo: L::from_bits(0x4006, 0x95D24A7AF877E60C),
        },
        DD {
            hi: L::from_bits(0xC045, 0xD00468BBD162FF4F),
            lo: L::from_bits(0x4003, 0xB53156CE5E2DC756),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4028, 0xDF241E30C47C7226),
            lo: L::from_bits(0x3FE7, 0xC93F185F231C7B6C),
        },
        DD {
            hi: L::from_bits(0xC032, 0x835C40DDCF7048E7),
            lo: L::from_bits(0xBFF1, 0xDC1A7129B45A1364),
        },
        DD {
            hi: L::from_bits(0x4038, 0xD4746B8ABDC7AD9C),
            lo: L::from_bits(0x3FF6, 0xE1FBACA2A63AF9AF),
        },
        DD {
            hi: L::from_bits(0xC03E, 0x8C77EC305977D081),
            lo: L::from_bits(0x3FFC, 0xF55705B8D2D597AD),
        },
        DD {
            hi: L::from_bits(0x4042, 0xC9E1A6909605E650),
            lo: L::from_bits(0x4001, 0xEB33856F79154E93),
        },
        DD {
            hi: L::from_bits(0xC046, 0xB5D89B96EDB3BE63),
            lo: L::from_bits(0xC003, 0xBF976F37A55A9CC2),
        },
        DD {
            hi: L::from_bits(0x4049, 0xDF73823080755D97),
            lo: L::from_bits(0xC005, 0x9A01F2CC24DAFE4E),
        },
        DD {
            hi: L::from_bits(0xC04C, 0xC5E041CEF51B3A10),
            lo: L::from_bits(0xC008, 0x994FD80BB1D995E1),
        },
        DD {
            hi: L::from_bits(0x404F, 0x8322435A88A64FAF),
            lo: L::from_bits(0x400E, 0x9F56AE7C37977726),
        },
        DD {
            hi: L::from_bits(0xC051, 0x859376ED2E0D9AF1),
            lo: L::from_bits(0xC00F, 0xB4F851AD722A0638),
        },
        DD {
            hi: L::from_bits(0x4052, 0xD51F858EF94CFBD6),
            lo: L::from_bits(0xC011, 0xFA43ADB44028A041),
        },
        DD {
            hi: L::from_bits(0xC054, 0x86E9C7A46A24A22C),
            lo: L::from_bits(0xC011, 0x91E057FF30B51C0C),
        },
        DD {
            hi: L::from_bits(0x4055, 0x88B168FED7B963DB),
            lo: L::from_bits(0xC014, 0xAAC8B4EA86EFEFB0),
        },
        DD {
            hi: L::from_bits(0xC055, 0xDEA9E75E69CD7B55),
            lo: L::from_bits(0x4014, 0x96CC15A8DC15A12E),
        },
        DD {
            hi: L::from_bits(0x4056, 0x91E1135253FBF0F4),
            lo: L::from_bits(0xC015, 0xB35A482FF3239759),
        },
        DD {
            hi: L::from_bits(0xC056, 0x993FE105ABB8F157),
            lo: L::from_bits(0xC015, 0x8369B744FC93EAB2),
        },
        DD {
            hi: L::from_bits(0x4056, 0x800E200048D07F1F),
            lo: L::from_bits(0x4013, 0x884271094CC94E3A),
        },
        DD {
            hi: L::from_bits(0xC055, 0xA7E2A8C0EC3BD1C2),
            lo: L::from_bits(0xC011, 0xEF1C6657788B887A),
        },
        DD {
            hi: L::from_bits(0x4054, 0xA8DFE2E2E87B14A2),
            lo: L::from_bits(0xC012, 0xE47D6367FDF504B5),
        },
        DD {
            hi: L::from_bits(0xC052, 0xFB8D919E248919B7),
            lo: L::from_bits(0x4010, 0x9E179CA1EFEDF741),
        },
        DD {
            hi: L::from_bits(0x4051, 0x82A8E47E5A2360A6),
            lo: L::from_bits(0xC00F, 0x908911134E5A74E0),
        },
        DD {
            hi: L::from_bits(0xC04E, 0xA8FBC821797BC886),
            lo: L::from_bits(0xC00D, 0xF07A8DA8FD9BF7E1),
        },
        DD {
            hi: L::from_bits(0x404A, 0xCCD4195EE0D419D9),
            lo: L::from_bits(0x4009, 0x97DA5E222BA5C79C),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x402C, 0x997C3C1210E3DC15),
            lo: L::from_bits(0x3FE7, 0x8AFA9064F053E492),
        },
        DD {
            hi: L::from_bits(0xC035, 0xC56C46DD2CF83166),
            lo: L::from_bits(0x3FF4, 0xAF4095AFC7C82217),
        },
        DD {
            hi: L::from_bits(0x403C, 0xAE392B848E9867B9),
            lo: L::from_bits(0x3FFB, 0x965B02C90EE8F497),
        },
        DD {
            hi: L::from_bits(0xC041, 0xFB4C7C21EC34F0EA),
            lo: L::from_bits(0xBFFE, 0xA878BCBE6A6B883C),
        },
        DD {
            hi: L::from_bits(0x4046, 0xC501F65E9ACF5F58),
            lo: L::from_bits(0x4003, 0xECB112516EA3AC1D),
        },
        DD {
            hi: L::from_bits(0xC04A, 0xC1BBAC89DD0B7548),
            lo: L::from_bits(0xC004, 0xBE6974D21047DBEE),
        },
        DD {
            hi: L::from_bits(0x404E, 0x821C8891ED1FABB9),
            lo: L::from_bits(0x400D, 0xFD291D0E15D3B55B),
        },
        DD {
            hi: L::from_bits(0xC050, 0xFC5BE00D62EDA4F9),
            lo: L::from_bits(0x400F, 0xE432CF87C18C04C7),
        },
        DD {
            hi: L::from_bits(0x4053, 0xB7994A62FEFD6AF6),
            lo: L::from_bits(0x4011, 0x835FF6F02EA125D0),
        },
        DD {
            hi: L::from_bits(0xC055, 0xCDF503B142A397D9),
            lo: L::from_bits(0x4014, 0xDD69B7C2AC56CA17),
        },
        DD {
            hi: L::from_bits(0x4057, 0xB5A77DBF2CC8BC5C),
            lo: L::from_bits(0x4014, 0xEF215A3642562F36),
        },
        DD {
            hi: L::from_bits(0xC058, 0xFF81A06DCF43FA03),
            lo: L::from_bits(0xC017, 0xFF943A79539FAE3C),
        },
        DD {
            hi: L::from_bits(0x405A, 0x90A9F5BFC5E0553D),
            lo: L::from_bits(0x4019, 0xBAAC17FC83C5FE34),
        },
        DD {
            hi: L::from_bits(0xC05B, 0x84A8B786B4F69F91),
            lo: L::from_bits(0xC019, 0xC34131CFAE92B214),
        },
        DD {
            hi: L::from_bits(0x405B, 0xC58554535DA60D1D),
            lo: L::from_bits(0xC017, 0x91738D02DDED597D),
        },
        DD {
            hi: L::from_bits(0xC05B, 0xEE8C65451BA7034A),
            lo: L::from_bits(0x4018, 0xDFB94C46AAFD3804),
        },
        DD {
            hi: L::from_bits(0x405B, 0xE8A014678F027727),
            lo: L::from_bits(0xC018, 0xE74514C48E9B15DD),
        },
        DD {
            hi: L::from_bits(0xC05B, 0xB58BC9761FC51363),
            lo: L::from_bits(0x401A, 0x996591482C238766),
        },
        DD {
            hi: L::from_bits(0x405A, 0xDF7B6F19347D38B2),
            lo: L::from_bits(0xC018, 0xC9CAFFFF9C468A3F),
        },
        DD {
            hi: L::from_bits(0xC059, 0xD40EF344413FFEB8),
            lo: L::from_bits(0xC016, 0xE8E241B8A63CD306),
        },
        DD {
            hi: L::from_bits(0x4058, 0x9599C3CA1508ED54),
            lo: L::from_bits(0xC017, 0x8C30081B32095352),
        },
        DD {
            hi: L::from_bits(0xC056, 0x93BD768D9869E597),
            lo: L::from_bits(0xC015, 0xABAF372B731DA5C7),
        },
        DD {
            hi: L::from_bits(0x4053, 0xB63C4105ACE78173),
            lo: L::from_bits(0xC012, 0xD192619FEC711E65),
        },
        DD {
            hi: L::from_bits(0xC04F, 0xD34991E17A8E4476),
            lo: L::from_bits(0xC00A, 0x94EDCB99887FA70C),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x402F, 0xDCBC2B0EA5F507AE),
            lo: L::from_bits(0xBFEE, 0xE1D75FC01805F597),
        },
        DD {
            hi: L::from_bits(0xC039, 0x9A830D3D431D0ED2),
            lo: L::from_bits(0x3FF8, 0xF7FC1E0916857ECB),
        },
        DD {
            hi: L::from_bits(0x4040, 0x9441B4B704D82AE6),
            lo: L::from_bits(0xBFFF, 0xB6FDA5D076101866),
        },
        DD {
            hi: L::from_bits(0xC045, 0xE867E3D20432A984),
            lo: L::from_bits(0x4004, 0xE7B1E7AFF313FFCD),
        },
        DD {
            hi: L::from_bits(0x404A, 0xC606958E3827DFAA),
            lo: L::from_bits(0x4009, 0x9B1A5955912943A8),
        },
        DD {
            hi: L::from_bits(0xC04E, 0xD3C62CD5B20E83B5),
            lo: L::from_bits(0x400D, 0xF9285DA62013B667),
        },
        DD {
            hi: L::from_bits(0x4052, 0x9AD665811344AAD7),
            lo: L::from_bits(0x4011, 0xC29C6D8D3C4E56C4),
        },
        DD {
            hi: L::from_bits(0xC055, 0xA3B9EFFAB5C3C0D5),
            lo: L::from_bits(0x4012, 0xE4C0767D4ED076BF),
        },
        DD {
            hi: L::from_bits(0x4058, 0x8226957D0DB7B873),
            lo: L::from_bits(0x4016, 0xCAD27CE507341468),
        },
        DD {
            hi: L::from_bits(0xC05A, 0x9FF3AEE739F10F70),
            lo: L::from_bits(0x4017, 0xE00F98AB84FD07F6),
        },
        DD {
            hi: L::from_bits(0x405C, 0x9B12C0D84098E3CF),
            lo: L::from_bits(0x401B, 0xE2D58EBA0BC60EAE),
        },
        DD {
            hi: L::from_bits(0xC05D, 0xF0BCD65B51D0F3FA),
            lo: L::from_bits(0xC01A, 0xF880F82945C22608),
        },
        DD {
            hi: L::from_bits(0x405F, 0x97304496C44536A8),
            lo: L::from_bits(0xC01B, 0x89FACFF458DF85EE),
        },
        DD {
            hi: L::from_bits(0xC060, 0x9AB977A91C0CF242),
            lo: L::from_bits(0x401F, 0x95E2E2C9995CF7D8),
        },
        DD {
            hi: L::from_bits(0x4061, 0x8183FCC957402C12),
            lo: L::from_bits(0xC01A, 0x87DB8EF38E8190F9),
        },
        DD {
            hi: L::from_bits(0xC061, 0xB1849BF756E459D2),
            lo: L::from_bits(0xC01D, 0x8610F77F9488BCDF),
        },
        DD {
            hi: L::from_bits(0x4061, 0xC6C7A4AEAC567053),
            lo: L::from_bits(0xC01F, 0xCBB4F1D739EBE936),
        },
        DD {
            hi: L::from_bits(0xC061, 0xB4D8A008C02EB4BD),
            lo: L::from_bits(0xC020, 0x8CDDDCEB3375274B),
        },
        DD {
            hi: L::from_bits(0x4061, 0x84623130AB124BB3),
            lo: L::from_bits(0xC01E, 0xBCC2229A30F297AD),
        },
        DD {
            hi: L::from_bits(0xC060, 0x999452FCAA426986),
            lo: L::from_bits(0x401F, 0xD1117ED61C464F86),
        },
        DD {
            hi: L::from_bits(0x405F, 0x89E8D54C86BA5354),
            lo: L::from_bits(0x401D, 0xE491B9B6D560C263),
        },
        DD {
            hi: L::from_bits(0xC05D, 0xB8D25C70C3A4FF72),
            lo: L::from_bits(0xC018, 0xCF125727E2B53A14),
        },
        DD {
            hi: L::from_bits(0x405B, 0xADF081A979B231C5),
            lo: L::from_bits(0x401A, 0xDD714D1B39AB3865),
        },
        DD {
            hi: L::from_bits(0xC058, 0xCD10FDBF809C1A85),
            lo: L::from_bits(0xC016, 0xC5F7CE33C3348C9C),
        },
        DD {
            hi: L::from_bits(0x4054, 0xE3D9FD7F72748F3F),
            lo: L::from_bits(0xC013, 0xAE76548DE8E2CE54),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4033, 0xA59EC8F24551D95C),
            lo: L::from_bits(0x3FF2, 0xC44EF6513AC6689C),
        },
        DD {
            hi: L::from_bits(0xC03C, 0xFB7EC8F6226AB373),
            lo: L::from_bits(0x3FFA, 0xF05C3215B84F535E),
        },
        DD {
            hi: L::from_bits(0x4044, 0x82BF37243920ED89),
            lo: L::from_bits(0xC003, 0xDD8F4F04EF3B92C8),
        },
        DD {
            hi: L::from_bits(0xC049, 0xDDFFAE90BCB86F9B),
            lo: L::from_bits(0x4008, 0x918462A57F67DE52),
        },
        DD {
            hi: L::from_bits(0x404E, 0xCCE3D4897CCBC551),
            lo: L::from_bits(0xC00C, 0xBACC6F6A765DAE9A),
        },
        DD {
            hi: L::from_bits(0xC052, 0xED729C328B54DCD4),
            lo: L::from_bits(0xC010, 0x85FB26E3FEF74D38),
        },
        DD {
            hi: L::from_bits(0x4056, 0xBC4D4D90C792F771),
            lo: L::from_bits(0xC015, 0xB7BD2C107A44BAE5),
        },
        DD {
            hi: L::from_bits(0xC059, 0xD8404F24C33DCBB7),
            lo: L::from_bits(0x4018, 0xE16A4BE42AF8B7D4),
        },
        DD {
            hi: L::from_bits(0x405C, 0xBB092BA4AD49428E),
            lo: L::from_bits(0x4019, 0xAD19F9FFB68A6EF0),
        },
        DD {
            hi: L::from_bits(0xC05E, 0xFAAAE5F22E4475A8),
            lo: L::from_bits(0x401B, 0xF7BAD291FF349F51),
        },
        DD {
            hi: L::from_bits(0x4061, 0x84E2C2E7EE419DEA),
            lo: L::from_bits(0xC020, 0xC212830B53F61EC2),
        },
        DD {
            hi: L::from_bits(0xC062, 0xE2638B3F377A4CF8),
            lo: L::from_bits(0xC021, 0x9D0A915BDB149A99),
        },
        DD {
            hi: L::from_bits(0x4064, 0x9CAF5C019398A6AD),
            lo: L::from_bits(0x4022, 0xF3094FAB6D06B950),
        },
        DD {
            hi: L::from_bits(0xC065, 0xB19D712143CA79CC),
            lo: L::from_bits(0xC024, 0x85481120C6E1F77E),
        },
        DD {
            hi: L::from_bits(0x4066, 0xA5B56E16536F7CB7),
            lo: L::from_bits(0xC025, 0xFB21CD998D937DCD),
        },
        DD {
            hi: L::from_bits(0xC066, 0xFF12216E88E5E2E3),
            lo: L::from_bits(0xC025, 0x96BE25488CE7B3E7),
        },
        DD {
            hi: L::from_bits(0x4067, 0xA1E4D16A089379AB),
            lo: L::from_bits(0xC023, 0xC98BF4B181917B90),
        },
        DD {
            hi: L::from_bits(0xC067, 0xA8F3752A132D21CF),
            lo: L::from_bits(0xC025, 0xC9E773D9FA543981),
        },
        DD {
            hi: L::from_bits(0x4067, 0x9008AEC3B5250D16),
            lo: L::from_bits(0xC026, 0x8E77315AC5E1FDA2),
        },
        DD {
            hi: L::from_bits(0xC066, 0xC68BA00B6E4DD617),
            lo: L::from_bits(0xC024, 0xDD93C7F93C053D91),
        },
        DD {
            hi: L::from_bits(0x4065, 0xD9C8FAF38C92C572),
            lo: L::from_bits(0x4023, 0xC6F12CADF06F4A57),
        },
        DD {
            hi: L::from_bits(0xC064, 0xB99A0C5DCD265D61),
            lo: L::from_bits(0x4022, 0xB65513F95D9323B9),
        },
        DD {
            hi: L::from_bits(0x4062, 0xECDAD7AECACFB2FD),
            lo: L::from_bits(0x4021, 0xB86250A2C9C47F89),
        },
        DD {
            hi: L::from_bits(0xC060, 0xD4E578B14E693787),
            lo: L::from_bits(0xC01F, 0xAC54DF736A34F377),
        },
        DD {
            hi: L::from_bits(0x405D, 0xF05E26FC42AE165D),
            lo: L::from_bits(0x4019, 0x9EA25B685C11E1B3),
        },
        DD {
            hi: L::from_bits(0xC05A, 0x803236ECF05CD8BA),
            lo: L::from_bits(0xC018, 0x9DAF3F633F79D9D4),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x4037, 0x8170CA6F31685A92),
            lo: L::from_bits(0x3FF5, 0x80636D04D0F2BF84),
        },
        DD {
            hi: L::from_bits(0xC040, 0xD485BFEE97FF663D),
            lo: L::from_bits(0xBFFE, 0xB197D2F4725E42D9),
        },
        DD {
            hi: L::from_bits(0x4047, 0xEEB51062C92B111F),
            lo: L::from_bits(0x4006, 0x923CEDA0544FC62E),
        },
        DD {
            hi: L::from_bits(0xC04D, 0xDAD2C244F0573531),
            lo: L::from_bits(0xC00C, 0x96432F4658E8A426),
        },
        DD {
            hi: L::from_bits(0x4052, 0xDA118877200FEA4F),
            lo: L::from_bits(0xC011, 0xE30C3C8C0F1D8730),
        },
        DD {
            hi: L::from_bits(0xC057, 0x887D64D9089F26E4),
            lo: L::from_bits(0x4016, 0x943DEE484D66F1A1),
        },
        DD {
            hi: L::from_bits(0x405A, 0xEA028843820C9B11),
            lo: L::from_bits(0xC018, 0x83C074A0ACE80689),
        },
        DD {
            hi: L::from_bits(0xC05E, 0x916B1F8C8E2DC6DC),
            lo: L::from_bits(0xC01C, 0xFC6A4C1CC46EF8C3),
        },
        DD {
            hi: L::from_bits(0x4061, 0x885271D458BB74CB),
            lo: L::from_bits(0x4020, 0x90E524ED19783251),
        },
        DD {
            hi: L::from_bits(0xC063, 0xC66A4D2CBCA85713),
            lo: L::from_bits(0x4021, 0xA250E684BA8D2C16),
        },
        DD {
            hi: L::from_bits(0x4065, 0xE5062D8FAF393710),
            lo: L::from_bits(0xC022, 0xFA4B9A17F8F4D85A),
        },
        DD {
            hi: L::from_bits(0xC067, 0xD50475FFAE911D57),
            lo: L::from_bits(0x4022, 0xB8D956652A3DF011),
        },
        DD {
            hi: L::from_bits(0x4069, 0xA18F541F745C5893),
            lo: L::from_bits(0xC027, 0xA3A6EAFE8FFB5A1C),
        },
        DD {
            hi: L::from_bits(0xC06A, 0xC98F8A36F0EE2F2C),
            lo: L::from_bits(0xC029, 0xBB6513D0CA7D1B81),
        },
        DD {
            hi: L::from_bits(0x406B, 0xD00A44E3D5646C6E),
            lo: L::from_bits(0x4029, 0xB5E9AAB178C11AB2),
        },
        DD {
            hi: L::from_bits(0xC06C, 0xB24096C7F3ED0829),
            lo: L::from_bits(0x402A, 0xAEDBBEFCCA4800E4),
        },
        DD {
            hi: L::from_bits(0x406C, 0xFDD75E03E131237A),
            lo: L::from_bits(0xC02B, 0xAF12A8D3A54A3D25),
        },
        DD {
            hi: L::from_bits(0xC06D, 0x9600448E727E537C),
            lo: L::from_bits(0xC02B, 0xBB17A765877EDCC5),
        },
        DD {
            hi: L::from_bits(0x406D, 0x928C54A9919A1D1A),
            lo: L::from_bits(0xC02C, 0xF63CA2A1A1F2A495),
        },
        DD {
            hi: L::from_bits(0xC06C, 0xEB0DCD1D1052A359),
            lo: L::from_bits(0x402A, 0xB301012083E8AA75),
        },
        DD {
            hi: L::from_bits(0x406C, 0x990D18F6E6E239EB),
            lo: L::from_bits(0x402A, 0xFE83D87E086A8177),
        },
        DD {
            hi: L::from_bits(0xC06B, 0x9F3411194360156B),
            lo: L::from_bits(0x4025, 0x9A762FC21722566D),
        },
        DD {
            hi: L::from_bits(0x406A, 0x81192DBF18A26B19),
            lo: L::from_bits(0xC026, 0x90675234C300FEB6),
        },
        DD {
            hi: L::from_bits(0xC068, 0x9D3C628010BEBB4E),
            lo: L::from_bits(0x4027, 0x93DDA46DE343E021),
        },
        DD {
            hi: L::from_bits(0x4066, 0x87416476978EF38B),
            lo: L::from_bits(0xC020, 0xB5CC6C67FE427C46),
        },
        DD {
            hi: L::from_bits(0xC063, 0x928162E527711B6E),
            lo: L::from_bits(0xC021, 0xF6B496E0DC3AB100),
        },
        DD {
            hi: L::from_bits(0x405F, 0x96431018FA8122B3),
            lo: L::from_bits(0xC01E, 0x952D3C78FEA6D32E),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x403A, 0xD26A761C290E8631),
            lo: L::from_bits(0x3FF9, 0xC7C2970DC433867A),
        },
        DD {
            hi: L::from_bits(0xC044, 0xBA385C3323DF44E2),
            lo: L::from_bits(0x4003, 0x9502B5D42B05335B),
        },
        DD {
            hi: L::from_bits(0x404B, 0xE14D990E299FFDA6),
            lo: L::from_bits(0x4009, 0x81F0796BD2022DA8),
        },
        DD {
            hi: L::from_bits(0xC051, 0xDE604DAD8295B69B),
            lo: L::from_bits(0xC010, 0xB4444915EBC5BFB8),
        },
        DD {
            hi: L::from_bits(0x4056, 0xEE97A143667C2F46),
            lo: L::from_bits(0xC014, 0xA1B636229E57A944),
        },
        DD {
            hi: L::from_bits(0xC05B, 0xA0D3DEB5510E41D2),
            lo: L::from_bits(0xC01A, 0xB3483878905B49BE),
        },
        DD {
            hi: L::from_bits(0x405F, 0x94928205460EC35F),
            lo: L::from_bits(0xC01C, 0x84AC5F3FC6F1CC93),
        },
        DD {
            hi: L::from_bits(0xC062, 0xC72EC0B7585BCE69),
            lo: L::from_bits(0xC021, 0xDBE581D4D6902BE1),
        },
        DD {
            hi: L::from_bits(0x4065, 0xC9AF83B2B13278ED),
            lo: L::from_bits(0xC022, 0xCB6BD113686AF7B4),
        },
        DD {
            hi: L::from_bits(0xC068, 0x9ECECDE5731041B5),
            lo: L::from_bits(0x4027, 0x8B17E7C2C0370499),
        },
        DD {
            hi: L::from_bits(0x406A, 0xC6C0DC5FDB4B9183),
            lo: L::from_bits(0x4028, 0xFA1A7FD565D4E69C),
        },
        DD {
            hi: L::from_bits(0xC06C, 0xC8F4A542D2D85542),
            lo: L::from_bits(0xC02A, 0xD9110BB35C1BD9E1),
        },
        DD {
            hi: L::from_bits(0x406E, 0xA631336495D0D102),
            lo: L::from_bits(0x402A, 0x8332E7BFACE41FAB),
        },
        DD {
            hi: L::from_bits(0xC06F, 0xE2EC09A5D7942906),
            lo: L::from_bits(0x402D, 0xD142F53D0E58AD43),
        },
        DD {
            hi: L::from_bits(0x4071, 0x80BBF6054C9C6AE4),
            lo: L::from_bits(0xC030, 0xF88FBF8E334CEC02),
        },
        DD {
            hi: L::from_bits(0xC071, 0xF3C9197506036865),
            lo: L::from_bits(0x402F, 0x8F533EACA4098B4B),
        },
        DD {
            hi: L::from_bits(0x4072, 0xC10B8CDBAD661376),
            lo: L::from_bits(0xC02F, 0xBF25C43B62AC4E75),
        },
        DD {
            hi: L::from_bits(0xC072, 0xFFB2B3A33AE368CA),
            lo: L::from_bits(0x4031, 0xD563DC8144570CA6),
        },
        DD {
            hi: L::from_bits(0x4073, 0x8D5336B6858ADD5F),
            lo: L::from_bits(0xC032, 0xC0E80964E89F27D4),
        },
        DD {
            hi: L::from_bits(0xC073, 0x81C5F6A903E2F0BF),
            lo: L::from_bits(0xC030, 0xABA147CD4DDCD8F6),
        },
        DD {
            hi: L::from_bits(0x4072, 0xC47B911DCCC3C2FF),
            lo: L::from_bits(0x402E, 0xBBDE9FCEF0176097),
        },
        DD {
            hi: L::from_bits(0xC071, 0xF275695F3F367C9A),
            lo: L::from_bits(0xC02F, 0xE73CE4A2E1B1E8C3),
        },
        DD {
            hi: L::from_bits(0x4070, 0xEFCD361239EDF872),
            lo: L::from_bits(0x402F, 0xE0152DFC65EBC09D),
        },
        DD {
            hi: L::from_bits(0xC06F, 0xB975E73B3D827528),
            lo: L::from_bits(0xC02B, 0xADF6AF30F6B75242),
        },
        DD {
            hi: L::from_bits(0x406D, 0xD807C4C36394BE41),
            lo: L::from_bits(0x402A, 0xB2181477DE2EDAC6),
        },
        DD {
            hi: L::from_bits(0xC06B, 0xB22BD620BAF583C9),
            lo: L::from_bits(0x402A, 0xB6BBC0A99EEB4FE4),
        },
        DD {
            hi: L::from_bits(0x4068, 0xB975255B077ACECA),
            lo: L::from_bits(0xC027, 0x9D8B0B597C32C66B),
        },
        DD {
            hi: L::from_bits(0xC064, 0xB72B0221043A14ED),
            lo: L::from_bits(0xC022, 0x8345D69E4065F9A4),
        },
    ],
    &[
        DD {
            hi: L::from_bits(0x403E, 0xB198DB427B829DB1),
            lo: L::from_bits(0xBFF9, 0xE1C673873C1F0918),
        },
        DD {
            hi: L::from_bits(0xC048, 0xA8FBF1B4694D11CB),
            lo: L::from_bits(0xC006, 0xC371D2CB17A73CEE),
        },
        DD {
            hi: L::from_bits(0x404F, 0xDBA33BA99F8692D1),
            lo: L::from_bits(0x400D, 0xB801309185CA13BD),
        },
        DD {
            hi: L::from_bits(0xC055, 0xE8CA7A954B58BCE1),
            lo: L::from_bits(0x4012, 0xE95E81827C08F5F8),
        },
        DD {
            hi: L::from_bits(0x405B, 0x8617D12EFC66A7F7),
            lo: L::from_bits(0xC01A, 0xAB8E935BFA5E9598),
        },
        DD {
            hi: L::from_bits(0xC05F, 0xC22715822B772E2E),
            lo: L::from_bits(0x401E, 0xE68BD8327AC2FC8C),
        },
        DD {
            hi: L::from_bits(0x4063, 0xC0BBCF17B6097E07),
            lo: L::from_bits(0xC022, 0xCC99DB910718E84C),
        },
        DD {
            hi: L::from_bits(0xC067, 0x8AF2106AFFE28F79),
            lo: L::from_bits(0x4024, 0xC585F4513409D69B),
        },
        DD {
            hi: L::from_bits(0x406A, 0x977C94A932D745BA),
            lo: L::from_bits(0x4027, 0xDEA237E2AB507E1E),
        },
        DD {
            hi: L::from_bits(0xC06D, 0x809FD7A58A071F2D),
            lo: L::from_bits(0xC02C, 0xB14FA74051467E2C),
        },
        DD {
            hi: L::from_bits(0x406F, 0xADE842C458E8EB13),
            lo: L::from_bits(0x402D, 0xE8885B0A2C2D1DFA),
        },
        DD {
            hi: L::from_bits(0xC071, 0xBE61B3E6DAE700CF),
            lo: L::from_bits(0x4030, 0xB21843E26A2694A5),
        },
        DD {
            hi: L::from_bits(0x4073, 0xAAEE0F69ACAAAE3E),
            lo: L::from_bits(0xC02E, 0xBD1AE6E1F0A02FEE),
        },
        DD {
            hi: L::from_bits(0xC074, 0xFE2EF7EE1EFBB666),
            lo: L::from_bits(0xC033, 0xFAA8FE48A8E8A8E4),
        },
        DD {
            hi: L::from_bits(0x4076, 0x9DA3C29E361F6B06),
            lo: L::from_bits(0xC035, 0xA0072AE3C0B880D5),
        },
        DD {
            hi: L::from_bits(0xC077, 0xA3E7F792CC8ED742),
            lo: L::from_bits(0x4035, 0xA72429D822E1921D),
        },
        DD {
            hi: L::from_bits(0x4078, 0x8F49C99D37BBC79A),
            lo: L::from_bits(0x4036, 0xD56FA531E24A77EC),
        },
        DD {
            hi: L::from_bits(0xC078, 0xD2E05526E28D2678),
            lo: L::from_bits(0xC037, 0xFDF154539A5F2576),
        },
        DD {
            hi: L::from_bits(0x4079, 0x8282A2DFAA5FBA73),
            lo: L::from_bits(0x4038, 0xC38F531226E8C570),
        },
        DD {
            hi: L::from_bits(0xC079, 0x877BD24AE070EBC4),
            lo: L::from_bits(0x4038, 0x9C2F3666AF7B3FBB),
        },
        DD {
            hi: L::from_bits(0x4078, 0xEAB0B91D663EFB69),
            lo: L::from_bits(0xC037, 0xD2DDE7F006F07325),
        },
        DD {
            hi: L::from_bits(0xC078, 0xA83A3573336C7E62),
            lo: L::from_bits(0x4037, 0xA4A77BA4A6DFE415),
        },
        DD {
            hi: L::from_bits(0x4077, 0xC53E7524B09FA804),
            lo: L::from_bits(0xC036, 0xC23E5D7DA287B97E),
        },
        DD {
            hi: L::from_bits(0xC076, 0xB9EF2D49B10E5D6E),
            lo: L::from_bits(0x4035, 0xAF3AEB7D19F63CB0),
        },
        DD {
            hi: L::from_bits(0x4075, 0x8970D5EE2054E097),
            lo: L::from_bits(0xC032, 0xC0479B4382DC9890),
        },
        DD {
            hi: L::from_bits(0xC073, 0x99675AA577CE42CE),
            lo: L::from_bits(0x4032, 0xC8B4CE331EF8FFE8),
        },
        DD {
            hi: L::from_bits(0x4070, 0xF305BDCF2A6087CC),
            lo: L::from_bits(0xC02F, 0xA472E9333DCDDEEB),
        },
        DD {
            hi: L::from_bits(0xC06D, 0xF37533B79BE1730A),
            lo: L::from_bits(0xC028, 0xEDD9B4941EF5EC2F),
        },
        DD {
            hi: L::from_bits(0x4069, 0xE7DD55D36FE2E777),
            lo: L::from_bits(0x4028, 0x8810019B1A394E0F),
        },
    ],
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

    /// Values at orders from 600 to 2^31 - 1, across the turning
    /// point (`posix/tools/oracle/besl_tables.py oracle-large`).
    const LARGE_ORACLE: &str = include_str!("besl_large_oracle.txt");

    /// At huge orders every value is the correctly rounded one too, but next
    /// to a zero, where the phase's error -- 2^-128 of a phase of the order of
    /// `n` -- is what is left.
    #[test]
    fn the_huge_orders_are_the_correctly_rounded_ones() {
        extended();
        let mut failures = Vec::new();
        let mut rows = 0;
        for line in LARGE_ORACLE
            .lines()
            .filter(|s| !s.starts_with('#') && !s.is_empty())
        {
            let w: Vec<&str> = line.split(' ').collect();
            let row = Row {
                func: w[0],
                n: w[1].parse().unwrap(),
                x: l(w[2]),
                y: l(w[4]),
                rel: w[5].chars().next().unwrap(),
            };
            rows += 1;
            for (name, mode) in MODES {
                assert_eq!(fesetround(mode), 0);
                let r = call(row.func, row.n, row.x);
                assert_eq!(fesetround(FE_TONEAREST), 0);
                let want = directed(row.y, row.rel, name);
                let d = ulps(r, want);
                if d == 0 || (near_a_zero(&row) && d <= 1) {
                    continue;
                }
                failures.push(format!(
                    "{name} {} {} {}: want {} ours {} ({d} ulps)",
                    row.func,
                    row.n,
                    hex(row.x),
                    hex(want),
                    hex(r)
                ));
            }
        }
        assert!(rows > 190, "{rows} rows");
        let shown = failures.len().min(60);
        assert!(
            failures.is_empty(),
            "{} differ; the first {shown}:\n{}",
            failures.len(),
            failures[..shown].join("\n")
        );
    }

    /// Where both hold -- orders from a little below DEBYE_ORDER to a few
    /// thousand, on either side of the turning point and across it --
    /// Debye's expansions and the recurrences they replace agree.
    #[test]
    fn debye_and_the_recurrences_agree() {
        extended();
        for m in [256u32, 512, 1000, 2048, 3100] {
            let mf = f64::from(m);
            for x in [
                mf * 0.8,
                mf - 40.0 * mf.cbrt(),
                mf - 7.3,
                mf + 0.5,
                mf + 9.25,
                mf + 40.0 * mf.cbrt(),
                mf * 1.5,
                mf * 7.0,
            ] {
                let lx = L::from_f64(x);
                let Some((a, ae)) = jn_large(m, lx) else {
                    assert!(m < DEBYE_WINDOW_ORDER, "J{m}({x}): no Debye value");
                    continue;
                };
                let (b, be) = jn_miller(m, lx);
                let (a, b) = (a.scale(ae), b.scale(be));
                let size = b
                    .mag()
                    .to_f64()
                    .max((2.0 / (core::f64::consts::PI * x)).sqrt() * 1e-3);
                assert!(
                    a.sub(b).mag().to_f64() <= size * 2f64.powi(-100),
                    "J{m}({x}): Debye {} recurrence {}",
                    hex(a.hi),
                    hex(b.hi)
                );
                let (c, ce) = yn_large(m, lx).unwrap();
                // The forward recurrence from Y0 and Y1, the old way.
                let (mut y0, mut y1, mut e) = y01_scaled(lx);
                let two_over_x = over_x(TWO, lx);
                for k in 1..m {
                    let y2 = two_over_x.mul_l(int(i64::from(k))).mul(y1).sub(y0);
                    y0 = y1;
                    y1 = y2;
                    if y1.mag() > pow2(RESCALE) {
                        y0 = y0.scale(-RESCALE);
                        y1 = y1.scale(-RESCALE);
                        e += RESCALE;
                    }
                }
                let (c, d) = (c.scale(ce), y1.scale(e));
                let size = d
                    .mag()
                    .to_f64()
                    .max((2.0 / (core::f64::consts::PI * x)).sqrt() * 1e-3);
                assert!(
                    c.sub(d).mag().to_f64() <= size * 2f64.powi(-100),
                    "Y{m}({x}): Debye {} recurrence {}",
                    hex(c.hi),
                    hex(d.hi)
                );
            }
        }
    }

    /// At the largest orders, where nothing else can check them, J and Y
    /// satisfy the Wronskian `J_(n+1) Y_n - J_n Y_(n+1) = 2/(pi x)` across
    /// the turning point: the window's recurrences and Debye's forms together.
    #[test]
    fn the_wronskian_holds_at_huge_orders() {
        extended();
        for n in [100_000i32, 16_777_216, 2_147_483_646] {
            let nf = f64::from(n);
            for d in [-50.0, -31.5, -3.0, 0.0, 0.5, 2.0, 31.5, 50.0, 1e4] {
                let x = L::from_f64(nf + d * nf.cbrt());
                let (jn, jn1, yn, yn1) = (jnl(n, x), jnl(n + 1, x), ynl(n, x), ynl(n + 1, x));
                let w = jn1 * yn - jn * yn1;
                let want = TWO_OVER_PI.hi / x;
                let rel = ((w - want) / want).abs().to_f64();
                assert!(rel < 1e-14, "n = {n}, x = {}: {rel:e}", x.to_f64());
            }
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
