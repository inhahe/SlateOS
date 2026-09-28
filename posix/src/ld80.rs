//! Arithmetic on `long double` -- x87's 80-bit extended format -- done by the
//! x87 unit itself.
//!
//! Rust has no 80-bit float type, and the sysroot's `long double` functions
//! used to compute in `f64` (known-issues.md -> `TD-POSIX-LONG-DOUBLE-PRECISION`).
//! Every x86-64 processor carries a unit that computes in exactly this
//! format, so rather than write 80-bit arithmetic in software, each operation
//! here is the x87 instruction that performs it: `fadd`, `fmul`, `fdiv`,
//! `fsqrt`, `frndint`, `fscale`, `fucomip`, over operands in memory.
//!
//! # The x87 environment
//!
//! An x87 operation rounds to the precision its control word selects. SlateOS
//! starts every thread with `fninit`'s word, `0x037F` -- 64-bit significands,
//! round to nearest, every exception masked -- which is Linux's default too,
//! and what glibc's `long double` functions assume; so do these. A program
//! that lowers the precision gets what glibc gives it: the lower precision.
//! (The Windows host, where the tests run, starts threads at 53 bits; the
//! tests set 64 first -- `tests::extended`.)
//!
//! Every `asm!` block here leaves the x87 register stack as it found it,
//! empty, which the SysV ABI requires at every call and return; each block
//! declares the registers it pushes as clobbered.
//!
//! # Layout
//!
//! [`LongDouble`] is the ABI's 16-byte slot (`crate::x87`). Its first ten
//! bytes are what `fld tbyte` and `fstp tbyte` read and write.

use crate::x87::LongDouble;
use core::arch::asm;
use core::cmp::Ordering;

impl LongDouble {
    /// The value with sign-and-exponent `sign_exp` and significand `sig`, bit
    /// for bit (bit 63 of `sig` is the explicit integer bit).
    #[must_use]
    pub const fn from_bits(sign_exp: u16, sig: u64) -> Self {
        Self {
            significand: sig,
            sign_exp,
            pad: [0; 6],
        }
    }

    /// `+0.0L`.
    pub const POS_ZERO: Self = Self::from_bits(0, 0);
    /// `1.0L`.
    pub const ONE: Self = Self::from_bits(0x3FFF, 1 << 63);
    /// `+inf`.
    pub const INFINITY: Self = Self::from_bits(0x7FFF, 1 << 63);
    /// The default quiet NaN, as the x87 unit makes it (`0xFFFF C000...`
    /// negative, like SSE's): positive here, as C's `NAN` is.
    pub const NAN: Self = Self::from_bits(0x7FFF, 0xC000_0000_0000_0000);

    /// The sign bit.
    #[must_use]
    pub const fn is_sign_negative(self) -> bool {
        self.sign_exp & 0x8000 != 0
    }

    /// The biased exponent field, 0 to 0x7FFF.
    #[must_use]
    pub const fn biased_exponent(self) -> u16 {
        self.sign_exp & 0x7FFF
    }

    /// A NaN: the maximal exponent with any fraction bit set, or any encoding
    /// the unit rejects (a maximal or ordinary exponent with the integer bit
    /// clear), which it loads as NaN.
    #[must_use]
    pub const fn is_nan(self) -> bool {
        let e = self.biased_exponent();
        if e == 0x7FFF {
            return self.significand != 1 << 63;
        }
        e != 0 && self.significand >> 63 == 0
    }

    /// `+-inf`.
    #[must_use]
    pub const fn is_infinite(self) -> bool {
        self.biased_exponent() == 0x7FFF && self.significand == 1 << 63
    }

    /// Neither infinite nor NaN.
    #[must_use]
    pub const fn is_finite(self) -> bool {
        !self.is_nan() && !self.is_infinite()
    }

    /// `+-0` (a pseudo-zero, exponent set and significand zero, is no zero:
    /// the unit loads it as NaN).
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.biased_exponent() == 0 && self.significand == 0
    }

    /// The same value with its sign bit flipped -- a NaN's too.
    #[must_use]
    pub const fn negate(self) -> Self {
        Self::from_bits(self.sign_exp ^ 0x8000, self.significand)
    }

    /// The same value with its sign bit clear.
    #[must_use]
    pub const fn abs(self) -> Self {
        Self::from_bits(self.sign_exp & 0x7FFF, self.significand)
    }

    /// `self` with `sign`'s sign bit.
    #[must_use]
    pub const fn copysign(self, sign: Self) -> Self {
        Self::from_bits(
            (self.sign_exp & 0x7FFF) | (sign.sign_exp & 0x8000),
            self.significand,
        )
    }

    /// `x`, exactly: every double is a long double.
    #[must_use]
    pub fn from_f64(x: f64) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: `fld qword` reads the eight bytes of a local; `fstp tbyte`
        // writes ten bytes into `r`, a 16-byte slot. One push, one pop.
        unsafe {
            asm!(
                "fld qword ptr [{x}]",
                "fstp tbyte ptr [{r}]",
                x = in(reg) &raw const x,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                options(nostack),
            );
        }
        r
    }

    /// `n`, exactly: every 64-bit integer is a long double.
    #[must_use]
    pub fn from_i64(n: i64) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: as in `from_f64`, with `fild qword`.
        unsafe {
            asm!(
                "fild qword ptr [{n}]",
                "fstp tbyte ptr [{r}]",
                n = in(reg) &raw const n,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                options(nostack),
            );
        }
        r
    }

    /// Correctly rounded to the nearest double -- `crate::x87::to_f64`, which
    /// does it in integer arithmetic, independent of the x87 control word.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        crate::x87::to_f64(self)
    }

    /// Round to an integer in the current rounding direction (`frndint`),
    /// kept as a long double.
    #[must_use]
    pub fn round_int(self) -> Self {
        self.unop_frndint()
    }

    /// The square root (`fsqrt`), correctly rounded.
    #[must_use]
    pub fn sqrt(self) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: one load, one store, both of long-double slots.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fsqrt",
                "fstp tbyte ptr [{r}]",
                a = in(reg) &raw const self,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                options(nostack),
            );
        }
        r
    }

    fn unop_frndint(self) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: one load, one store, both of long-double slots.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "frndint",
                "fstp tbyte ptr [{r}]",
                a = in(reg) &raw const self,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                options(nostack),
            );
        }
        r
    }

    /// `self * 2^n`, exactly unless it overflows or underflows (`fscale`,
    /// whose scale is the integer `n`, loaded exactly).
    #[must_use]
    pub fn scalbn(self, n: i32) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: `fild dword` reads the local `n`; the value is loaded
        // above it; `fscale` scales st(0) by st(1); two pops leave the stack
        // empty, the first into `r`.
        unsafe {
            asm!(
                "fild dword ptr [{n}]",
                "fld tbyte ptr [{a}]",
                "fscale",
                "fstp tbyte ptr [{r}]",
                "fstp st(0)",
                n = in(reg) &raw const n,
                a = in(reg) &raw const self,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                out("st(1)") _,
                options(nostack),
            );
        }
        r
    }

    /// The comparison the unit makes (`fucomip`, which raises invalid only
    /// for a signalling NaN): `None` when either is NaN.
    #[must_use]
    pub fn compare(self, other: Self) -> Option<Ordering> {
        let (below, equal, unordered): (u8, u8, u8);
        // SAFETY: two loads; `fucomip` compares st(0) (`self`) with st(1)
        // (`other`) and pops once; `fstp st(0)` pops the other. The flags it
        // sets are read at once.
        unsafe {
            asm!(
                "fld tbyte ptr [{b}]",
                "fld tbyte ptr [{a}]",
                "fucomip st, st(1)",
                "fstp st(0)",
                "setb {lt}",
                "sete {eq}",
                "setp {un}",
                a = in(reg) &raw const self,
                b = in(reg) &raw const other,
                lt = out(reg_byte) below,
                eq = out(reg_byte) equal,
                un = out(reg_byte) unordered,
                out("st(0)") _,
                out("st(1)") _,
                options(nostack),
            );
        }
        if unordered != 0 {
            None
        } else if equal != 0 {
            Some(Ordering::Equal)
        } else if below != 0 {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Greater)
        }
    }
}

/// The x87 instructions the `long double` functions are built from, each over
/// memory operands and leaving the register stack empty.
impl LongDouble {
    /// `2^self - 1` (`f2xm1`), for `-1 <= self <= 1`; outside that range the
    /// instruction's result is undefined, so callers reduce first.
    #[must_use]
    pub fn f2xm1(self) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: one load, one store, both of long-double slots.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "f2xm1",
                "fstp tbyte ptr [{r}]",
                a = in(reg) &raw const self,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                options(nostack),
            );
        }
        r
    }

    /// `y * log2(x)` (`fyl2x`), rounded once.
    #[must_use]
    pub fn fyl2x(y: Self, x: Self) -> Self {
        Self::two_in(y, x, 0)
    }

    /// `y * log2(x + 1)` (`fyl2xp1`), rounded once; accurate only for
    /// `|x| < 1 - sqrt(2)/2`, where callers use it.
    #[must_use]
    pub fn fyl2xp1(y: Self, x: Self) -> Self {
        Self::two_in(y, x, 1)
    }

    /// `atan2(y, x)` (`fpatan`), in `[-pi, pi]`, rounded once.
    #[must_use]
    pub fn fpatan(y: Self, x: Self) -> Self {
        Self::two_in(y, x, 2)
    }

    /// `self * 2^trunc(n)` (`fscale`) for a long-double scale, as musl's
    /// assembly uses it.
    #[must_use]
    pub fn fscale(self, n: Self) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: loads `n` then `self`; `fscale` scales st(0) by st(1);
        // two pops, the first into `r`.
        unsafe {
            asm!(
                "fld tbyte ptr [{n}]",
                "fld tbyte ptr [{a}]",
                "fscale",
                "fstp tbyte ptr [{r}]",
                "fstp st(0)",
                n = in(reg) &raw const n,
                a = in(reg) &raw const self,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                out("st(1)") _,
                options(nostack),
            );
        }
        r
    }

    /// `self`'s significand and exponent (`fxtract`): the significand in
    /// `[1, 2)` with `self`'s sign, the exponent unbiased, as a long double.
    /// A zero gives itself and `-inf`, raising divide-by-zero; an infinity
    /// itself and `+inf`; a NaN itself, quieted, twice; an encoding the unit
    /// rejects the default NaN, raising invalid.
    #[must_use]
    pub fn fxtract(self) -> (Self, Self) {
        let mut sig = Self::POS_ZERO;
        let mut exp = Self::POS_ZERO;
        // SAFETY: one load; `fxtract` replaces st(0) with the exponent and
        // pushes the significand above it; two pops, into `sig` then `exp`,
        // leave the stack empty. All three slots are long doubles.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fxtract",
                "fstp tbyte ptr [{s}]",
                "fstp tbyte ptr [{e}]",
                a = in(reg) &raw const self,
                s = in(reg) &raw mut sig,
                e = in(reg) &raw mut exp,
                out("st(0)") _,
                out("st(1)") _,
                options(nostack),
            );
        }
        (sig, exp)
    }

    /// The three instructions that take two stack operands, `y` under `x`,
    /// and leave one result: `fyl2x` (0), `fyl2xp1` (1), `fpatan` (2).
    fn two_in(y: Self, x: Self, which: u8) -> Self {
        let mut r = Self::POS_ZERO;
        // SAFETY: loads y then x (st(1) = y, st(0) = x); each instruction
        // pops once and leaves its result in st(0); the store pops it.
        unsafe {
            match which {
                0 => asm!(
                    "fld tbyte ptr [{y}]",
                    "fld tbyte ptr [{x}]",
                    "fyl2x",
                    "fstp tbyte ptr [{r}]",
                    y = in(reg) &raw const y,
                    x = in(reg) &raw const x,
                    r = in(reg) &raw mut r,
                    out("st(0)") _,
                    out("st(1)") _,
                    options(nostack),
                ),
                1 => asm!(
                    "fld tbyte ptr [{y}]",
                    "fld tbyte ptr [{x}]",
                    "fyl2xp1",
                    "fstp tbyte ptr [{r}]",
                    y = in(reg) &raw const y,
                    x = in(reg) &raw const x,
                    r = in(reg) &raw mut r,
                    out("st(0)") _,
                    out("st(1)") _,
                    options(nostack),
                ),
                _ => asm!(
                    "fld tbyte ptr [{y}]",
                    "fld tbyte ptr [{x}]",
                    "fpatan",
                    "fstp tbyte ptr [{r}]",
                    y = in(reg) &raw const y,
                    x = in(reg) &raw const x,
                    r = in(reg) &raw mut r,
                    out("st(0)") _,
                    out("st(1)") _,
                    options(nostack),
                ),
            }
        }
        r
    }

    /// `x REM y` by `fprem` (`IEEE == false`: C's `fmod`, truncating) or
    /// `fprem1` (`IEEE == true`: `remainder`, to nearest), repeated until
    /// the reduction is complete, with the low three bits of the quotient
    /// the unit reports (C0, C3, C1 -> bits 2, 1, 0), which `remquol` needs.
    #[must_use]
    pub fn partial_remainder<const IEEE: bool>(x: Self, y: Self) -> (Self, u8) {
        let mut r = Self::POS_ZERO;
        let mut sw: u16;
        // SAFETY: loads y then x; the loop repeats the instruction while C2
        // (bit 10 of the status word) says the reduction is incomplete; the
        // result is stored and both registers popped.
        unsafe {
            if IEEE {
                asm!(
                    "fld tbyte ptr [{y}]",
                    "fld tbyte ptr [{x}]",
                    "2:",
                    "fprem1",
                    "fnstsw ax",
                    "test ah, 4",
                    "jnz 2b",
                    "fstp tbyte ptr [{r}]",
                    "fstp st(0)",
                    y = in(reg) &raw const y,
                    x = in(reg) &raw const x,
                    r = in(reg) &raw mut r,
                    out("ax") sw,
                    out("st(0)") _,
                    out("st(1)") _,
                    options(nostack),
                );
            } else {
                asm!(
                    "fld tbyte ptr [{y}]",
                    "fld tbyte ptr [{x}]",
                    "2:",
                    "fprem",
                    "fnstsw ax",
                    "test ah, 4",
                    "jnz 2b",
                    "fstp tbyte ptr [{r}]",
                    "fstp st(0)",
                    y = in(reg) &raw const y,
                    x = in(reg) &raw const x,
                    r = in(reg) &raw mut r,
                    out("ax") sw,
                    out("st(0)") _,
                    out("st(1)") _,
                    options(nostack),
                );
            }
        }
        // C0 is bit 8, C3 bit 14, C1 bit 9: the quotient's bits 2, 1 and 0.
        let q = (((sw >> 8) & 1) << 2) | (((sw >> 14) & 1) << 1) | ((sw >> 9) & 1);
        (r, q as u8)
    }

    /// Round to an integer in the direction `rc` (the control word's
    /// rounding field: 0 nearest, 1 down, 2 up, 3 toward zero) whatever the
    /// current one is, restoring it after -- musl's `floorl`, `ceill` and
    /// `truncl`.
    #[must_use]
    pub fn round_int_toward(self, rc: u16) -> Self {
        let mut r = Self::POS_ZERO;
        let mut saved: u16 = 0;
        let mut rounding: u16 = 0;
        // SAFETY: saves the control word to a local, loads one with the
        // rounding field replaced (from another local), rounds, restores the
        // saved word; one load and one store of long-double slots.
        unsafe {
            asm!(
                "fnstcw [{saved}]",
                "mov {t:x}, word ptr [{saved}]",
                "and {t:x}, 0xF3FF",
                "or {t:x}, {rc:x}",
                "mov word ptr [{tmp}], {t:x}",
                "fldcw [{tmp}]",
                "fld tbyte ptr [{a}]",
                "frndint",
                "fstp tbyte ptr [{r}]",
                "fldcw [{saved}]",
                saved = in(reg) &raw mut saved,
                tmp = in(reg) &raw mut rounding,
                t = out(reg) _,
                rc = in(reg) (rc & 3) << 10,
                a = in(reg) &raw const self,
                r = in(reg) &raw mut r,
                out("st(0)") _,
                options(nostack),
            );
        }
        r
    }

    /// To a 64-bit integer in the current rounding direction (`fistp`); an
    /// out-of-range or NaN operand gives the unit's "integer indefinite",
    /// `i64::MIN`, and raises invalid, as `lrintl` does.
    #[must_use]
    pub fn to_i64_rint(self) -> i64 {
        let mut n: i64 = 0;
        // SAFETY: one load of a long-double slot, one store of a local i64.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fistp qword ptr [{n}]",
                a = in(reg) &raw const self,
                n = in(reg) &raw mut n,
                out("st(0)") _,
                options(nostack),
            );
        }
        n
    }

    /// The unit's constants, rounded to nearest as `fld*` loads them in the
    /// default rounding mode: `log2 e`, `log2 10`, `log10 2`, `ln 2`, `pi`.
    pub const LOG2_E: Self = Self::from_bits(0x3FFF, 0xB8AA_3B29_5C17_F0BC);
    /// `log2 10`.
    pub const LOG2_10: Self = Self::from_bits(0x4000, 0xD49A_784B_CD1B_8AFE);
    /// `log10 2`.
    pub const LOG10_2: Self = Self::from_bits(0x3FFD, 0x9A20_9A84_FBCF_F799);
    /// `ln 2`.
    pub const LN_2: Self = Self::from_bits(0x3FFE, 0xB172_17F7_D1CF_79AC);
    /// `pi`.
    pub const PI: Self = Self::from_bits(0x4000, 0xC90F_DAA2_2168_C235);
}

/// One two-operand x87 operation over memory: `a OP b`, rounded once, as C
/// evaluates `a OP b` on `long double`s.
macro_rules! binop {
    ($trait:ident, $method:ident, $insn:literal) => {
        impl core::ops::$trait for LongDouble {
            type Output = Self;
            fn $method(self, b: Self) -> Self {
                let mut r = Self::POS_ZERO;
                // SAFETY: two loads push `self` then `b`; the operation pops
                // one (st(1) = st(1) OP st(0)); the store pops the result.
                // Every operand is a long-double slot on this frame.
                unsafe {
                    asm!(
                        "fld tbyte ptr [{a}]",
                        "fld tbyte ptr [{b}]",
                        $insn,
                        "fstp tbyte ptr [{r}]",
                        a = in(reg) &raw const self,
                        b = in(reg) &raw const b,
                        r = in(reg) &raw mut r,
                        out("st(0)") _,
                        out("st(1)") _,
                        options(nostack),
                    );
                }
                r
            }
        }
    };
}

binop!(Add, add, "faddp st(1), st");
// `fsubp st(1), st` is st(1) - st(0) -- self - b -- in Intel syntax as LLVM
// assembles it (the historical AT&T/Intel reversal of fsubp and fsubrp does
// not apply to the explicit-operand Intel form).
binop!(Sub, sub, "fsubp st(1), st");
binop!(Mul, mul, "fmulp st(1), st");
binop!(Div, div, "fdivp st(1), st");

impl core::ops::Neg for LongDouble {
    type Output = Self;
    fn neg(self) -> Self {
        self.negate()
    }
}

impl PartialEq for LongDouble {
    fn eq(&self, other: &Self) -> bool {
        self.compare(*other) == Some(Ordering::Equal)
    }
}

impl PartialOrd for LongDouble {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.compare(*other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The x87 precision control at 64 bits, round to nearest: the default a
    /// SlateOS (or Linux) thread starts with, and not the Windows host's,
    /// which is 53. Each test sets it on its own thread first.
    pub(crate) fn extended() {
        let cw: u16 = 0x037F;
        // SAFETY: loads the control word from a local.
        unsafe { asm!("fldcw [{}]", in(reg) &raw const cw, options(nostack, preserves_flags)) };
    }

    fn ld(x: f64) -> LongDouble {
        LongDouble::from_f64(x)
    }

    fn bits(x: LongDouble) -> (u16, u64) {
        (x.sign_exp, x.significand)
    }

    #[test]
    fn a_double_loads_exactly() {
        extended();
        assert_eq!(bits(ld(1.0)), (0x3FFF, 1 << 63));
        assert_eq!(bits(ld(-2.0)), (0xC000, 1 << 63));
        assert_eq!(bits(ld(0.0)), (0, 0));
        assert_eq!(bits(ld(-0.0)), (0x8000, 0));
        assert_eq!(ld(0.1).to_f64(), 0.1, "and narrows back");
        assert!(ld(f64::NAN).is_nan());
        assert!(ld(f64::INFINITY).is_infinite());
    }

    /// The whole point: 64 bits of significand, where a double has 53.
    /// 1 + 2^-60 is exact here; as a double it is 1.
    #[test]
    fn arithmetic_keeps_sixty_four_bits() {
        extended();
        let tiny = ld(1.0).scalbn(-60);
        let sum = ld(1.0) + tiny;
        assert_eq!(bits(sum), (0x3FFF, (1 << 63) | (1 << 3)));
        assert_eq!(sum - ld(1.0), tiny, "and the difference is exact");
        // 1/3 to 64 bits: 0xAAAA...AAAB x 2^-65, rounded to nearest.
        let third = ld(1.0) / ld(3.0);
        assert_eq!(bits(third), (0x3FFD, 0xAAAA_AAAA_AAAA_AAAB));
        assert_eq!(bits(ld(3.0) * ld(7.0)), bits(ld(21.0)));
        assert_eq!(bits(-ld(5.0)), bits(ld(-5.0)));
    }

    /// `a - b` is `a - b`, not `b - a` -- the operand order of `fsubp` and
    /// `fdivp` is the classic trap.
    #[test]
    fn subtraction_and_division_are_not_reversed() {
        extended();
        assert_eq!(bits(ld(10.0) - ld(3.0)), bits(ld(7.0)));
        assert_eq!(bits(ld(12.0) / ld(4.0)), bits(ld(3.0)));
    }

    #[test]
    fn sqrt_round_int_and_scalbn() {
        extended();
        assert_eq!(bits(ld(2.0).sqrt()), (0x3FFF, 0xB504_F333_F9DE_6484));
        assert_eq!(bits(ld(2.5).round_int()), bits(ld(2.0)), "to nearest even");
        assert_eq!(bits(ld(-3.5).round_int()), bits(ld(-4.0)));
        assert_eq!(bits(ld(3.0).scalbn(4)), bits(ld(48.0)));
        assert!(ld(1.0).scalbn(20000).is_infinite());
        assert_eq!(
            bits(LongDouble::from_i64(i64::MAX)),
            (0x403D, u64::MAX << 1)
        );
    }

    /// The constants are what the unit's own load instructions give, in the
    /// default rounding mode.
    #[test]
    fn the_constants_are_the_units() {
        extended();
        let load = |which: u8| {
            let mut r = LongDouble::POS_ZERO;
            // SAFETY: one constant pushed, one store; test-only.
            unsafe {
                match which {
                    0 => {
                        asm!("fldl2e", "fstp tbyte ptr [{r}]", r = in(reg) &raw mut r, out("st(0)") _, options(nostack))
                    }
                    1 => {
                        asm!("fldl2t", "fstp tbyte ptr [{r}]", r = in(reg) &raw mut r, out("st(0)") _, options(nostack))
                    }
                    2 => {
                        asm!("fldlg2", "fstp tbyte ptr [{r}]", r = in(reg) &raw mut r, out("st(0)") _, options(nostack))
                    }
                    3 => {
                        asm!("fldln2", "fstp tbyte ptr [{r}]", r = in(reg) &raw mut r, out("st(0)") _, options(nostack))
                    }
                    _ => {
                        asm!("fldpi", "fstp tbyte ptr [{r}]", r = in(reg) &raw mut r, out("st(0)") _, options(nostack))
                    }
                }
            }
            bits(r)
        };
        assert_eq!(load(0), bits(LongDouble::LOG2_E));
        assert_eq!(load(1), bits(LongDouble::LOG2_10));
        assert_eq!(load(2), bits(LongDouble::LOG10_2));
        assert_eq!(load(3), bits(LongDouble::LN_2));
        assert_eq!(load(4), bits(LongDouble::PI));
    }

    #[test]
    fn the_transcendental_instructions() {
        extended();
        // 2^0.5 - 1, and log2 8 = 3, and atan2(1, 1) = pi/4, all exact to
        // the unit's rounding.
        assert_eq!(ld(0.5).f2xm1() + ld(1.0), ld(2.0).sqrt());
        assert_eq!(bits(LongDouble::fyl2x(ld(1.0), ld(8.0))), bits(ld(3.0)));
        assert_eq!(
            bits(LongDouble::fpatan(ld(1.0), ld(1.0))),
            bits(LongDouble::PI.scalbn(-2))
        );
        assert_eq!(
            bits(ld(3.0).fscale(ld(2.9))),
            bits(ld(12.0)),
            "trunc(2.9) = 2"
        );
    }

    #[test]
    fn remainders_and_directed_rounding() {
        extended();
        // fmod(7.5, 2) = 1.5, quotient 3; remainder(7.5, 2) = -0.5, quotient 4.
        let (r, q) = LongDouble::partial_remainder::<false>(ld(7.5), ld(2.0));
        assert_eq!((bits(r), q), (bits(ld(1.5)), 3));
        let (r, q) = LongDouble::partial_remainder::<true>(ld(7.5), ld(2.0));
        assert_eq!((bits(r), q & 7), (bits(ld(-0.5)), 4));
        // A huge quotient: the loop completes the reduction.
        let (r, _) = LongDouble::partial_remainder::<false>(ld(1e300), ld(3.0));
        assert!(r >= ld(0.0) && r < ld(3.0), "{r:?}");
        assert_eq!(bits(ld(-2.5).round_int_toward(1)), bits(ld(-3.0)), "floor");
        assert_eq!(bits(ld(-2.5).round_int_toward(2)), bits(ld(-2.0)), "ceil");
        assert_eq!(bits(ld(-2.7).round_int_toward(3)), bits(ld(-2.0)), "trunc");
        assert_eq!(
            bits(ld(2.5).round_int()),
            bits(ld(2.0)),
            "and the mode is restored"
        );
        assert_eq!(ld(-7.5).to_i64_rint(), -8);
        assert_eq!(
            ld(1e30).to_i64_rint(),
            i64::MIN,
            "out of range: integer indefinite"
        );
    }

    #[test]
    fn comparison_is_the_units() {
        extended();
        assert!(ld(1.0) < ld(2.0));
        assert!(ld(-0.0) == ld(0.0));
        assert!(ld(f64::NAN).partial_cmp(&ld(1.0)).is_none());
        assert!(ld(f64::NAN) != ld(f64::NAN));
        let a = ld(1.0) + ld(1.0).scalbn(-63);
        assert!(
            a > ld(1.0),
            "a difference below a double's precision is seen"
        );
    }
}
