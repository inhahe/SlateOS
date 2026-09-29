//! Fused multiply-add -- `x * y + z` rounded once -- by the processor's
//! instruction where it has one and the operating system saves the register
//! state it uses, and in software (the vendored libm's, which is musl's)
//! where not.
//!
//! Both are exact: an FMA has one correct answer in each rounding mode, and
//! both give it. So which one runs changes the speed and never a result,
//! which is what lets code that is correct only with a true FMA -- CORE-MATH's
//! double-double arithmetic in `lgamma.rs`, and C's `fma` itself -- use this
//! on any x86-64. The instruction is from 2013 (Haswell, Piledriver); the
//! QEMU CPU the boot test runs on (`qemu64`) lacks it, so there the software
//! path is what runs, at a few dozen nanoseconds a call against one cycle.
//!
//! "Usable" needs three things, all read once: CPUID leaf 1 reports FMA and
//! OSXSAVE, and XCR0 shows the OS saving the SSE and AVX state (bits 1 and
//! 2) -- a VEX-encoded instruction faults where it does not, whatever CPUID
//! says the silicon can do.

use core::sync::atomic::{AtomicU8, Ordering};

/// Not yet looked at.
const UNKNOWN: u8 = 0;
/// No usable FMA instruction: the software fallback.
const SOFT: u8 = 1;
/// The instruction.
const HARD: u8 = 2;

/// Which path, once known. A race between two first calls is harmless:
/// both work the same answer out.
static KIND: AtomicU8 = AtomicU8::new(UNKNOWN);

#[inline]
fn kind() -> u8 {
    let k = KIND.load(Ordering::Relaxed);
    if k != UNKNOWN {
        return k;
    }
    let k = if usable() { HARD } else { SOFT };
    KIND.store(k, Ordering::Relaxed);
    k
}

#[cfg(target_arch = "x86_64")]
fn usable() -> bool {
    // CPUID is architecturally present and unprivileged on every x86-64,
    // which is why `__cpuid` is a safe function.
    let ecx = core::arch::x86_64::__cpuid(1).ecx;
    let fma = ecx & (1 << 12) != 0;
    let osxsave = ecx & (1 << 27) != 0;
    if !(fma && osxsave) {
        return false;
    }
    // SAFETY: OSXSAVE is set, which is the processor saying XGETBV is
    // enabled; `xgetbv0` is the one instruction.
    let xcr0 = unsafe { xgetbv0() };
    xcr0 & 0b110 == 0b110
}

#[cfg(not(target_arch = "x86_64"))]
fn usable() -> bool {
    false
}

/// XCR0.
///
/// # Safety
///
/// Only where CPUID reports OSXSAVE.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "xsave")]
unsafe fn xgetbv0() -> u64 {
    // SAFETY: the caller has seen OSXSAVE, which is what makes XGETBV legal
    // to execute.
    unsafe { core::arch::x86_64::_xgetbv(0) }
}

/// `vfmadd231sd`.
///
/// # Safety
///
/// Only where [`usable`] said so.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "fma")]
unsafe fn fma_hw(x: f64, y: f64, z: f64) -> f64 {
    use core::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_set_sd};
    _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(x), _mm_set_sd(y), _mm_set_sd(z)))
}

/// `vfmadd231ss`; see [`fma_hw`].
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "fma")]
unsafe fn fmaf_hw(x: f32, y: f32, z: f32) -> f32 {
    use core::arch::x86_64::{_mm_cvtss_f32, _mm_fmadd_ss, _mm_set_ss};
    _mm_cvtss_f32(_mm_fmadd_ss(_mm_set_ss(x), _mm_set_ss(y), _mm_set_ss(z)))
}

/// `x * y + z`, rounded once, in the current rounding mode.
#[inline]
pub(crate) fn fma(x: f64, y: f64, z: f64) -> f64 {
    #[cfg(target_arch = "x86_64")]
    if kind() == HARD {
        // SAFETY: `kind` found the instruction usable.
        return unsafe { fma_hw(x, y, z) };
    }
    libm::fma(x, y, z)
}

/// [`fma`] for `float`.
#[inline]
pub(crate) fn fmaf(x: f32, y: f32, z: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if kind() == HARD {
        // SAFETY: `kind` found the instruction usable.
        return unsafe { fmaf_hw(x, y, z) };
    }
    libm::fmaf(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift64*, for bit patterns of every kind.
    struct Bits(u64);
    impl Bits {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
    }

    /// Both paths give the one correctly rounded answer, in every direction:
    /// on random bit patterns (every exponent, the specials among them) and
    /// on sums that cancel -- `z` the negated rounded product, where only a
    /// true FMA sees the product's low half. Nothing to compare where the
    /// processor has no FMA; the software path is libm's, tested there.
    #[test]
    fn the_instruction_and_the_software_agree() {
        if !usable() {
            return;
        }
        let restore = crate::fenv::fegetround();
        let mut r = Bits(0x9e37_79b9_7f4a_7c15);
        for mode in [
            crate::fenv::FE_TONEAREST,
            crate::fenv::FE_UPWARD,
            crate::fenv::FE_DOWNWARD,
            crate::fenv::FE_TOWARDZERO,
        ] {
            assert_eq!(crate::fenv::fesetround(mode), 0);
            for i in 0..50_000 {
                let x = f64::from_bits(r.next());
                let y = f64::from_bits(r.next());
                let z = if i % 2 == 0 {
                    f64::from_bits(r.next())
                } else {
                    -(x * y)
                };
                // SAFETY: `usable` said so, above.
                let hw = unsafe { fma_hw(x, y, z) };
                let sw = libm::fma(x, y, z);
                assert!(
                    hw.to_bits() == sw.to_bits() || (hw.is_nan() && sw.is_nan()),
                    "fma({x:e}, {y:e}, {z:e}) in {mode:#x}: {hw:e} and {sw:e}"
                );
                #[allow(clippy::cast_possible_truncation)]
                let (xf, yf) = (
                    f32::from_bits(r.next() as u32),
                    f32::from_bits(r.next() as u32),
                );
                #[allow(clippy::cast_possible_truncation)]
                let zf = if i % 2 == 0 {
                    f32::from_bits(r.next() as u32)
                } else {
                    -(xf * yf)
                };
                // SAFETY: as above.
                let hwf = unsafe { fmaf_hw(xf, yf, zf) };
                let swf = libm::fmaf(xf, yf, zf);
                assert!(
                    hwf.to_bits() == swf.to_bits() || (hwf.is_nan() && swf.is_nan()),
                    "fmaf({xf:e}, {yf:e}, {zf:e}) in {mode:#x}: {hwf:e} and {swf:e}"
                );
            }
        }
        assert_eq!(crate::fenv::fesetround(restore), 0);
    }

    /// Fused: (1 + 2^-52)(1 - 2^-52) - 1 is -2^-104, which a product rounded
    /// first loses entirely.
    #[test]
    fn fma_rounds_once() {
        let a = 1.0 + f64::EPSILON;
        let b = 1.0 - f64::EPSILON;
        assert_eq!(fma(a, b, -1.0), -(f64::EPSILON * f64::EPSILON));
        let af = 1.0 + f32::EPSILON;
        let bf = 1.0 - f32::EPSILON;
        assert_eq!(fmaf(af, bf, -1.0), -(f32::EPSILON * f32::EPSILON));
    }
}
