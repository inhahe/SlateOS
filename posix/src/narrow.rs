//! C23's narrowing arithmetic -- `fadd`, `fsub`, `fmul`, `fdiv`, `fsqrt` and
//! `ffma` on `double`s into a `float`, and their `f...l` (`long double` into
//! `float`) and `d...l` (`long double` into `double`) forms: one operation
//! on the wider arguments, rounded once into the narrower result. Computing
//! in the wider type and converting would round twice, which is wrong in a
//! result's last bit whenever the first rounding lands on the second's
//! midpoint.
//!
//! glibc 2.39's `math/math-narrow.h`, as it does it: the operation in the
//! wider type rounding toward zero, the inexact flag folded into the
//! result's lowest significand bit -- "round to odd", which loses nothing
//! the second rounding needs, since every wider type here has at least two
//! bits more than its narrower one -- then one conversion in the caller's
//! direction, which raises what the one true rounding raises. A result that
//! is exactly zero is computed in the caller's direction instead, as round
//! to odd cannot give a zero its sign. `errno` is glibc's `CHECK_NARROW_*`:
//! `EDOM` for a NaN from arguments that were not, `ERANGE` for an infinity
//! from finite arguments and for a zero from nonzero ones; the `fma` forms
//! set none, as glibc's do not (its bug 6801; C allows either).
//!
//! `double` arguments compute and convert in the SSE unit, through
//! [`crate::fenv::toward_zero_sse`]; `long double` ones in the x87, through
//! [`crate::fenv::toward_zero_x87`], converting by an x87 store as glibc's
//! compiled code does -- so each honours its own unit's direction and
//! raises its flags there. `narrow_oracle.txt`
//! (`posix/tools/oracle/narrow_harness.py`) holds all eighteen to glibc's
//! answers in all four directions -- value, every exception and `errno` --
//! over its own tests' operands and ones placed on the result types'
//! rounding boundaries.

// Floating-point arithmetic is the point here, and IEEE's: an overflow is an
// infinity and a flag, never a panic.
#![allow(clippy::arithmetic_side_effects)]

use crate::errno;
use crate::fenv::{toward_zero_sse, toward_zero_x87};
use crate::x87::LongDouble as L;
use core::cmp::Ordering;
use core::hint::black_box as bb;

// ===========================================================================
// The pieces
// ===========================================================================

/// `x` with its lowest significand bit set if `inexact`: round to odd's
/// answer from the toward-zero one.
fn odd64(x: f64, inexact: bool) -> f64 {
    f64::from_bits(x.to_bits() | u64::from(inexact))
}

/// [`odd64`] for a long double.
fn odd_l(x: L, inexact: bool) -> L {
    L::from_bits(x.sign_exp, x.significand | u64::from(inexact))
}

/// `x` stored as a double by the x87 (`fstp qword`): rounded in the x87
/// unit's direction, raising there what that rounding raises.
fn store_f64(x: L) -> f64 {
    let mut r = 0.0f64;
    // SAFETY: one load of the 10-byte `x` and one store of the 8-byte `r`,
    // both locals; the value pushed is popped by the store.
    unsafe {
        core::arch::asm!(
            "fld tbyte ptr [{a}]",
            "fstp qword ptr [{r}]",
            a = in(reg) &raw const x,
            r = in(reg) &raw mut r,
            out("st(0)") _,
            options(nostack),
        );
    }
    r
}

/// `x` stored as a float by the x87 (`fstp dword`).
fn store_f32(x: L) -> f32 {
    let mut r = 0.0f32;
    // SAFETY: as [`store_f64`], into a 4-byte local.
    unsafe {
        core::arch::asm!(
            "fld tbyte ptr [{a}]",
            "fstp dword ptr [{r}]",
            a = in(reg) &raw const x,
            r = in(reg) &raw mut r,
            out("st(0)") _,
            options(nostack),
        );
    }
    r
}

/// C's tests of a `long double`, as GCC compiles them: comparisons on the
/// x87, which answer "unordered" for a NaN and for an encoding the unit
/// refuses, and raise invalid for those as the unit does.
fn nan_l(x: L) -> bool {
    x.compare(x).is_none()
}

fn finite_l(x: L) -> bool {
    matches!(
        x.abs().compare(MAX_L),
        Some(Ordering::Less | Ordering::Equal)
    )
}

fn inf_l(x: L) -> bool {
    x.abs().compare(MAX_L) == Some(Ordering::Greater)
}

fn zero_l(x: L) -> bool {
    x.compare(L::POS_ZERO) == Some(Ordering::Equal)
}

/// `LDBL_MAX`.
const MAX_L: L = L::from_bits(0x7FFE, u64::MAX);

/// What the result says, for the `errno` tests: the result widened (exactly)
/// to a double.
#[derive(Clone, Copy)]
struct Ret(f64);

impl Ret {
    fn finite(self) -> bool {
        self.0.is_finite()
    }
    fn nan(self) -> bool {
        self.0.is_nan()
    }
    fn zero(self) -> bool {
        self.0 == 0.0
    }
}

fn edom() {
    errno::set_errno(errno::EDOM);
}

fn erange() {
    errno::set_errno(errno::ERANGE);
}

/// The four checks, over the argument type's own tests (`nan`, `finite`,
/// `inf`, `zero`, and whether `x` and `y` make an exact zero sum or
/// difference): glibc's `CHECK_NARROW_ADD`, `_SUB`, `_MUL`, `_DIV` and
/// `_SQRT`.
#[derive(Clone, Copy)]
enum Check {
    Add,
    Sub,
    Mul,
    Div,
}

/// `CHECK_NARROW_*` for two arguments, given how the argument type answers
/// C's tests of each and whether they cancel (`x == -y` for an addition,
/// `x == y` for a subtraction).
fn check2(ret: Ret, check: Check, x: Tests, y: Tests, cancel: bool) {
    if !ret.finite() {
        if ret.nan() {
            if !x.nan() && !y.nan() {
                edom();
            }
        } else {
            let range = match check {
                Check::Div => x.finite(),
                _ => x.finite() && y.finite(),
            };
            if range {
                erange();
            }
        }
    } else if ret.zero() {
        let range = match check {
            Check::Add | Check::Sub => !cancel,
            Check::Mul => !x.zero() && !y.zero(),
            Check::Div => !x.zero() && !y.inf(),
        };
        if range {
            erange();
        }
    }
}

/// `CHECK_NARROW_SQRT`.
fn check_sqrt(ret: Ret, x: Tests) {
    if !ret.finite() {
        if ret.nan() {
            if !x.nan() {
                edom();
            }
        } else if x.finite() {
            erange();
        }
    } else if ret.zero() && !x.zero() {
        erange();
    }
}

/// One argument, asked C's tests lazily -- in the order `CHECK_NARROW_*`
/// asks them and no further than `&&` goes, since on the x87 each question
/// is a comparison that can raise invalid.
#[derive(Clone, Copy)]
enum Tests {
    /// A double, asked with the SSE unit's comparisons.
    Double(f64),
    /// A long double, asked with the x87's.
    Extended(L),
}

impl Tests {
    fn nan(self) -> bool {
        match self {
            Self::Double(x) => x.is_nan(),
            Self::Extended(x) => nan_l(x),
        }
    }
    fn finite(self) -> bool {
        match self {
            Self::Double(x) => x.is_finite(),
            Self::Extended(x) => finite_l(x),
        }
    }
    fn inf(self) -> bool {
        match self {
            Self::Double(x) => x.is_infinite(),
            Self::Extended(x) => inf_l(x),
        }
    }
    fn zero(self) -> bool {
        match self {
            Self::Double(x) => x == 0.0,
            Self::Extended(x) => zero_l(x),
        }
    }
}

// ===========================================================================
// double -> float
// ===========================================================================

/// `x + y` into a float, rounded once.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fadd(x: f64, y: f64) -> f32 {
    let ret = if x == -y {
        // An exact zero: its sign is the caller's direction's.
        let s = bb(bb(x) + bb(y));
        s as f32
    } else {
        let (s, inexact) = toward_zero_sse((x, y), |(x, y)| x + y);
        bb(odd64(s, inexact)) as f32
    };
    check2(
        Ret(f64::from(ret)),
        Check::Add,
        Tests::Double(x),
        Tests::Double(y),
        x == -y,
    );
    ret
}

/// `x - y` into a float, rounded once.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsub(x: f64, y: f64) -> f32 {
    let ret = if x == y {
        let s = bb(bb(x) - bb(y));
        s as f32
    } else {
        let (s, inexact) = toward_zero_sse((x, y), |(x, y)| x - y);
        bb(odd64(s, inexact)) as f32
    };
    check2(
        Ret(f64::from(ret)),
        Check::Sub,
        Tests::Double(x),
        Tests::Double(y),
        x == y,
    );
    ret
}

/// `x * y` into a float, rounded once.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmul(x: f64, y: f64) -> f32 {
    let (s, inexact) = toward_zero_sse((x, y), |(x, y)| x * y);
    let ret = bb(odd64(s, inexact)) as f32;
    check2(
        Ret(f64::from(ret)),
        Check::Mul,
        Tests::Double(x),
        Tests::Double(y),
        false,
    );
    ret
}

/// `x / y` into a float, rounded once.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdiv(x: f64, y: f64) -> f32 {
    let (s, inexact) = toward_zero_sse((x, y), |(x, y)| x / y);
    let ret = bb(odd64(s, inexact)) as f32;
    check2(
        Ret(f64::from(ret)),
        Check::Div,
        Tests::Double(x),
        Tests::Double(y),
        false,
    );
    ret
}

/// The square root of `x` into a float, rounded once.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsqrt(x: f64) -> f32 {
    let (s, inexact) = toward_zero_sse(x, libm::sqrt);
    let ret = bb(odd64(s, inexact)) as f32;
    check_sqrt(Ret(f64::from(ret)), Tests::Double(x));
    ret
}

/// `x * y + z` into a float, rounded once. An exact zero is `x * y + z`
/// unfused in the caller's direction, for its sign.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ffma(x: f64, y: f64, z: f64) -> f32 {
    let (t, inexact) = toward_zero_sse((x, y, z), |(x, y, z)| crate::fmadd::fma(x, y, z));
    let t = odd64(t, inexact);
    if t == 0.0 {
        let p = bb(bb(x) * bb(y));
        let s = bb(p + bb(z));
        return s as f32;
    }
    bb(t) as f32
}

// ===========================================================================
// long double -> float and long double -> double
// ===========================================================================

/// The operations over long doubles, each into both narrower types.
#[derive(Clone, Copy)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

/// `x op y` in long double toward zero, rounded to odd.
fn odd_op_l(op: Op, x: L, y: L) -> L {
    let (s, inexact) = toward_zero_x87((op, x, y), |(op, x, y)| match op {
        Op::Add => x + y,
        Op::Sub => x - y,
        Op::Mul => x * y,
        Op::Div => x / y,
    });
    odd_l(s, inexact)
}

/// `x op y` into a narrower type by `store` (an x87 store), rounded once,
/// with glibc's `errno`.
fn binary_l<T: Copy>(op: Op, x: L, y: L, store: fn(L) -> T, widen: fn(T) -> f64) -> T {
    let (check, cancel) = match op {
        Op::Add => (Check::Add, Some(x.compare(-y) == Some(Ordering::Equal))),
        Op::Sub => (Check::Sub, Some(x.compare(y) == Some(Ordering::Equal))),
        Op::Mul => (Check::Mul, None),
        Op::Div => (Check::Div, None),
    };
    let ret = if cancel == Some(true) {
        // An exact zero, in the caller's direction.
        let s = match op {
            Op::Add => x + y,
            _ => x - y,
        };
        store(s)
    } else {
        store(odd_op_l(op, x, y))
    };
    check2(
        Ret(widen(ret)),
        check,
        Tests::Extended(x),
        Tests::Extended(y),
        cancel.unwrap_or(false),
    );
    ret
}

/// The square root into a narrower type, rounded once.
fn sqrt_l<T: Copy>(x: L, store: fn(L) -> T, widen: fn(T) -> f64) -> T {
    let (s, inexact) = toward_zero_x87(x, L::sqrt);
    let ret = store(odd_l(s, inexact));
    check_sqrt(Ret(widen(ret)), Tests::Extended(x));
    ret
}

/// `x * y + z` into a narrower type, rounded once (no `errno`, as glibc's).
fn fma_l<T: Copy>(x: L, y: L, z: L, store: fn(L) -> T) -> T {
    let (t, inexact) = toward_zero_x87((x, y, z), |(x, y, z)| crate::mathl::fmal(x, y, z));
    let t = odd_l(t, inexact);
    if t.compare(L::POS_ZERO) == Some(Ordering::Equal) {
        return store(x * y + z);
    }
    store(t)
}

fn widen32(x: f32) -> f64 {
    f64::from(x)
}

fn widen64(x: f64) -> f64 {
    x
}

/// `faddl`.
#[must_use]
pub fn faddl(x: L, y: L) -> f32 {
    binary_l(Op::Add, x, y, store_f32, widen32)
}

/// `fsubl`.
#[must_use]
pub fn fsubl(x: L, y: L) -> f32 {
    binary_l(Op::Sub, x, y, store_f32, widen32)
}

/// `fmull`.
#[must_use]
pub fn fmull(x: L, y: L) -> f32 {
    binary_l(Op::Mul, x, y, store_f32, widen32)
}

/// `fdivl`.
#[must_use]
pub fn fdivl(x: L, y: L) -> f32 {
    binary_l(Op::Div, x, y, store_f32, widen32)
}

/// `fsqrtl`.
#[must_use]
pub fn fsqrtl(x: L) -> f32 {
    sqrt_l(x, store_f32, widen32)
}

/// `ffmal`.
#[must_use]
pub fn ffmal(x: L, y: L, z: L) -> f32 {
    fma_l(x, y, z, store_f32)
}

/// `daddl`.
#[must_use]
pub fn daddl(x: L, y: L) -> f64 {
    binary_l(Op::Add, x, y, store_f64, widen64)
}

/// `dsubl`.
#[must_use]
pub fn dsubl(x: L, y: L) -> f64 {
    binary_l(Op::Sub, x, y, store_f64, widen64)
}

/// `dmull`.
#[must_use]
pub fn dmull(x: L, y: L) -> f64 {
    binary_l(Op::Mul, x, y, store_f64, widen64)
}

/// `ddivl`.
#[must_use]
pub fn ddivl(x: L, y: L) -> f64 {
    binary_l(Op::Div, x, y, store_f64, widen64)
}

/// `dsqrtl`.
#[must_use]
pub fn dsqrtl(x: L) -> f64 {
    sqrt_l(x, store_f64, widen64)
}

/// `dfmal`.
#[must_use]
pub fn dfmal(x: L, y: L, z: L) -> f64 {
    fma_l(x, y, z, store_f64)
}

// ---------------------------------------------------------------------------
// Their C entry points: a long double argument arrives on the stack, a float
// or double result leaves in xmm0 -- so each thunk hands over the arguments'
// addresses and jumps (`ld_abi.rs`: `x_ll`, `x_lll`, and `i_l` for one).
// ---------------------------------------------------------------------------

macro_rules! export_ll {
    ($($c:literal $shim:ident $f:ident $t:ty;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, y: *const L) -> $t {
            // SAFETY: called only by its thunk, with the caller's two stack
            // arguments.
            unsafe { $f(x.read(), y.read()) }
        }
        crate::ld_c!(x_ll $c => $shim);
    )* };
}

macro_rules! export_l {
    ($($c:literal $shim:ident $f:ident $t:ty;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L) -> $t {
            // SAFETY: called only by its thunk, with the caller's stack
            // argument.
            $f(unsafe { x.read() })
        }
        crate::ld_c!(i_l $c => $shim);
    )* };
}

macro_rules! export_lll {
    ($($c:literal $shim:ident $f:ident $t:ty;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, y: *const L, z: *const L) -> $t {
            // SAFETY: called only by its thunk, with the caller's three
            // stack arguments.
            unsafe { $f(x.read(), y.read(), z.read()) }
        }
        crate::ld_c!(x_lll $c => $shim);
    )* };
}

export_ll! {
    "faddl" __slate_ld_faddl faddl f32;
    "fsubl" __slate_ld_fsubl fsubl f32;
    "fmull" __slate_ld_fmull fmull f32;
    "fdivl" __slate_ld_fdivl fdivl f32;
    "daddl" __slate_ld_daddl daddl f64;
    "dsubl" __slate_ld_dsubl dsubl f64;
    "dmull" __slate_ld_dmull dmull f64;
    "ddivl" __slate_ld_ddivl ddivl f64;
}

export_l! {
    "fsqrtl" __slate_ld_fsqrtl fsqrtl f32;
    "dsqrtl" __slate_ld_dsqrtl dsqrtl f64;
}

export_lll! {
    "ffmal" __slate_ld_ffmal ffmal f32;
    "dfmal" __slate_ld_dfmal dfmal f64;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;
    use crate::fenv::{
        FE_DIVBYZERO, FE_DOWNWARD, FE_INEXACT, FE_INVALID, FE_OVERFLOW, FE_TONEAREST,
        FE_TOWARDZERO, FE_UNDERFLOW, FE_UPWARD, feclearexcept, fesetround, fetestexcept,
    };
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers (`posix/tools/oracle/narrow_harness.py`): one
    /// call a line, `<function> <inputs> = ` and the four directions'
    /// `<result>:<flags>:<errno>`.
    const ORACLE: &str = include_str!("narrow_oracle.txt");

    const FIVE: i32 = FE_INVALID | FE_DIVBYZERO | FE_OVERFLOW | FE_UNDERFLOW | FE_INEXACT;

    /// The x87 unit at 64-bit precision: what a SlateOS thread starts with,
    /// and not the Windows host's 53 bits.
    fn extended() {
        let cw: u16 = 0x037F;
        // SAFETY: loads the control word from a local.
        unsafe {
            core::arch::asm!("fldcw [{}]", in(reg) &raw const cw, options(nostack, preserves_flags));
        }
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

    fn d(s: &str) -> f64 {
        f64::from_bits(u64::from_str_radix(s, 16).unwrap())
    }
    fn l(s: &str) -> L {
        let (se, m) = s.split_once(':').unwrap();
        L::from_bits(
            u16::from_str_radix(se, 16).unwrap(),
            u64::from_str_radix(m, 16).unwrap(),
        )
    }

    /// One call's answer, as the oracle writes it: the result's bits.
    fn call(func: &str, a: &[&str]) -> String {
        let f32r = |r: f32| format!("{:08x}", r.to_bits());
        let f64r = |r: f64| format!("{:016x}", r.to_bits());
        match func {
            "fadd" => f32r(fadd(d(a[0]), d(a[1]))),
            "fsub" => f32r(fsub(d(a[0]), d(a[1]))),
            "fmul" => f32r(fmul(d(a[0]), d(a[1]))),
            "fdiv" => f32r(fdiv(d(a[0]), d(a[1]))),
            "fsqrt" => f32r(fsqrt(d(a[0]))),
            "ffma" => f32r(ffma(d(a[0]), d(a[1]), d(a[2]))),
            "faddl" => f32r(faddl(l(a[0]), l(a[1]))),
            "fsubl" => f32r(fsubl(l(a[0]), l(a[1]))),
            "fmull" => f32r(fmull(l(a[0]), l(a[1]))),
            "fdivl" => f32r(fdivl(l(a[0]), l(a[1]))),
            "fsqrtl" => f32r(fsqrtl(l(a[0]))),
            "ffmal" => f32r(ffmal(l(a[0]), l(a[1]), l(a[2]))),
            "daddl" => f64r(daddl(l(a[0]), l(a[1]))),
            "dsubl" => f64r(dsubl(l(a[0]), l(a[1]))),
            "dmull" => f64r(dmull(l(a[0]), l(a[1]))),
            "ddivl" => f64r(ddivl(l(a[0]), l(a[1]))),
            "dsqrtl" => f64r(dsqrtl(l(a[0]))),
            "dfmal" => f64r(dfmal(l(a[0]), l(a[1]), l(a[2]))),
            other => panic!("no {other}"),
        }
    }

    /// Every call the oracle made, in each of the four directions, answered
    /// as glibc answered it: the result, every exception raised, and
    /// `errno`.
    #[test]
    fn every_answer_is_glibcs() {
        extended();
        let mut failures = Vec::new();
        let mut calls = 0;
        for line in ORACLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (lhs, glibc) = line.split_once(" = ").unwrap();
            let mut words = lhs.split(' ');
            let func = words.next().unwrap();
            let args: Vec<&str> = words.collect();
            let mut ours = Vec::new();
            for mode in [FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO] {
                assert_eq!(fesetround(mode), 0);
                feclearexcept(FIVE);
                errno::set_errno(0);
                let r = call(func, &args);
                let e = errno::get_errno();
                let fl = flags();
                assert_eq!(fesetround(FE_TONEAREST), 0);
                ours.push(format!("{r}:{fl}:{e}"));
            }
            calls += 4;
            let ours = ours.join(" ");
            if ours != glibc {
                failures.push(format!("{lhs}\n    glibc {glibc}\n    ours  {ours}"));
            }
        }
        feclearexcept(FIVE);
        assert!(calls > 40_000, "the oracle is whole: {calls} calls");
        let shown = failures.len().min(50);
        assert!(
            failures.is_empty(),
            "{} lines differ; the first {shown}:\n{}",
            failures.len(),
            failures[..shown].join("\n")
        );
    }

    /// The case narrowing exists for: a sum whose double rounding lands on a
    /// float midpoint. 1 + 2^-24 + 2^-60 is just above the midpoint between
    /// 1 and the next float; converted from its double (1 + 2^-24, a tie)
    /// it would round to even, 1; rounded once it is the float above 1.
    #[test]
    fn a_sum_is_rounded_once() {
        let x = 1.0 + f64::powi(2.0, -24);
        let y = f64::powi(2.0, -60);
        assert_eq!(((x + y) as f32).to_bits(), 1.0f32.to_bits(), "twice");
        assert_eq!(fadd(x, y).to_bits(), 1.0f32.to_bits() + 1, "once");
    }
}
