//! `<fenv.h>` -- the floating-point environment: the rounding direction and
//! the exception flags, as glibc 2.39's `sysdeps/x86_64/fpu` keeps them.
//!
//! x86-64 has two floating-point units and a program uses both: SSE for every
//! `double` and `float`, the x87 unit for `long double`. Each has its own
//! rounding control and its own sticky exception flags -- the SSE unit in
//! `MXCSR`, the x87 unit in its control and status words -- so every function
//! here reads or writes both, as glibc's do, and a flag raised in either unit
//! counts. The layout of `fenv_t` is the x87 `fnstenv` image (28 bytes)
//! followed by `MXCSR`, glibc's and musl's alike.
//!
//! Missing until 2026-09-27: a C program calling any of these did not link,
//! and nothing could change the rounding direction `rint` and `nearbyint`
//! round in (`known-issues.md` -> `D-POSIX-MATH-HAS-NO-FENV-LONG-DOUBLE-OR-COMPLEX`).
//!
//! Each unit's state is per thread (the kernel saves and restores it with the
//! rest of a thread's registers), so none of this needs a lock.
//!
//! Not here yet: C23's `fegetmode`/`fesetmode`, whose `femode_t` musl's
//! headers -- which this library's ABI is checked against -- do not have.

use core::arch::asm;

/// Invalid operation (0/0, inf - inf, sqrt(-1) ...).
pub const FE_INVALID: i32 = 1;
/// The x87 "denormal operand" flag, which C does not name; glibc keeps it
/// out of `FE_ALL_EXCEPT` and masked.
const FE_DENORM: i32 = 2;
/// Division by zero.
pub const FE_DIVBYZERO: i32 = 4;
/// Overflow.
pub const FE_OVERFLOW: i32 = 8;
/// Underflow.
pub const FE_UNDERFLOW: i32 = 16;
/// Inexact result.
pub const FE_INEXACT: i32 = 32;
/// Every exception C names.
pub const FE_ALL_EXCEPT: i32 = 63;
/// Every exception the x87 unit has: C's five and the denormal operand.
const FE_ALL_EXCEPT_X86: i32 = FE_ALL_EXCEPT | FE_DENORM;

/// Round to nearest, ties to even: the default.
pub const FE_TONEAREST: i32 = 0;
/// Round toward -inf.
pub const FE_DOWNWARD: i32 = 0x400;
/// Round toward +inf.
pub const FE_UPWARD: i32 = 0x800;
/// Round toward zero.
pub const FE_TOWARDZERO: i32 = 0xc00;

/// The x87 control word's precision-control field set to 64-bit mantissas
/// (glibc's `_FPU_EXTENDED`), which is what `long double` needs.
const FPU_EXTENDED: u16 = 0x300;
/// `MXCSR`'s rounding-control field.
const MXCSR_ROUNDING: u32 = 0x6000;
/// `MXCSR`'s flush-to-zero (bit 15) and denormals-are-zero (bit 6) bits.
const MXCSR_FZ_DAZ: u32 = 0x8040;

/// A set of exception flags, as `fegetexceptflag` saves them: the `FE_*`
/// bits themselves, glibc's and musl's representation.
pub type FexceptT = u16;

/// The whole environment: the x87 unit's `fnstenv` image, then `MXCSR`.
///
/// `opcode` holds the 11-bit last-opcode field and its 5 unused bits --
/// C's `unsigned int __opcode:11, __unused4:5`, which occupy exactly these
/// two bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FenvT {
    /// x87 control word: exception masks, precision, rounding.
    pub control_word: u16,
    unused1: u16,
    /// x87 status word: the sticky exception flags among the rest.
    pub status_word: u16,
    unused2: u16,
    /// x87 tag word.
    pub tags: u16,
    unused3: u16,
    /// Last x87 instruction's address.
    pub eip: u32,
    /// Its code segment.
    pub cs_selector: u16,
    /// Its opcode (11 bits) and 5 unused bits.
    pub opcode: u16,
    /// Last x87 operand's address.
    pub data_offset: u32,
    /// Its data segment.
    pub data_selector: u16,
    unused5: u16,
    /// The SSE unit's control and status register.
    pub mxcsr: u32,
}

/// `FE_DFL_ENV`: `(const fenv_t *) -1`, the environment a program starts in.
pub const FE_DFL_ENV: *const FenvT = usize::MAX as *const FenvT;
/// `FE_NOMASK_ENV` (a GNU extension): `(const fenv_t *) -2`, the default
/// with every exception unmasked, so that each traps.
pub const FE_NOMASK_ENV: *const FenvT = (usize::MAX - 1) as *const FenvT;

// ---------------------------------------------------------------------------
// The two units' registers
// ---------------------------------------------------------------------------

fn stmxcsr() -> u32 {
    let mut v: u32 = 0;
    // SAFETY: `stmxcsr` stores the four bytes of MXCSR at the operand, a
    // local that lives across the instruction; it reads no other memory.
    unsafe { asm!("stmxcsr [{}]", in(reg) &raw mut v, options(nostack, preserves_flags)) };
    v
}

fn ldmxcsr(v: u32) {
    // SAFETY: `ldmxcsr` loads MXCSR from the four bytes at the operand, a
    // local. Every value this module writes keeps MXCSR's reserved bits
    // (16..31) as the processor left them -- it only ever ORs and masks the
    // defined low bits -- so the load cannot fault.
    unsafe { asm!("ldmxcsr [{}]", in(reg) &raw const v, options(nostack, preserves_flags)) };
}

fn fnstcw() -> u16 {
    let mut v: u16 = 0;
    // SAFETY: stores the x87 control word's two bytes at a local.
    unsafe { asm!("fnstcw [{}]", in(reg) &raw mut v, options(nostack, preserves_flags)) };
    v
}

fn fldcw(v: u16) {
    // SAFETY: loads the x87 control word from a local's two bytes.
    unsafe { asm!("fldcw [{}]", in(reg) &raw const v, options(nostack, preserves_flags)) };
}

fn fnstsw() -> u16 {
    let mut v: u16 = 0;
    // SAFETY: stores the x87 status word's two bytes at a local.
    unsafe { asm!("fnstsw [{}]", in(reg) &raw mut v, options(nostack, preserves_flags)) };
    v
}

/// The x87 environment (28 bytes; `mxcsr` untouched). `fnstenv` masks
/// every x87 exception as a side effect, so callers that mean only to read
/// load it straight back ([`fegetenv`]).
fn fnstenv(env: &mut FenvT) {
    // SAFETY: `fnstenv` stores 28 bytes at the operand; `FenvT` is 32 bytes,
    // `repr(C)`, the image's layout.
    unsafe {
        asm!("fnstenv [{}]", in(reg) core::ptr::from_mut(env), options(nostack, preserves_flags))
    };
}

fn fldenv(env: &FenvT) {
    // SAFETY: `fldenv` loads 28 bytes from the operand, a whole `FenvT`.
    unsafe {
        asm!("fldenv [{}]", in(reg) core::ptr::from_ref(env), options(nostack, preserves_flags))
    };
}

/// glibc's `libc_feholdexcept_sse` (`sysdeps/x86/fpu/fenv_private.h`), for
/// the double and float functions, which run in the SSE unit alone: clear
/// its flags and mask its exceptions, and answer what to restore.
pub(crate) fn hold_sse() -> u32 {
    let old = stmxcsr();
    ldmxcsr((old | 0x1f80) & !0x3f);
    old
}

/// Put back what [`hold_sse`] took: the flags raised meanwhile are dropped.
pub(crate) fn restore_sse(old: u32) {
    ldmxcsr(old);
}

/// Both units' current exception flags.
fn flags() -> i32 {
    i32::from(fnstsw()) | i32::try_from(stmxcsr() & 0x3f).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Exceptions
// ---------------------------------------------------------------------------

/// Clear the flags in `excepts`, in both units (glibc's `fclrexcpt.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feclearexcept(excepts: i32) -> i32 {
    let excepts = excepts & FE_ALL_EXCEPT;
    let mut env = FenvT::default();
    fnstenv(&mut env);
    // glibc's `status_word &= excepts ^ FE_ALL_EXCEPT`: the other flags
    // stay, and everything above them -- the error-summary bit among it --
    // goes, so no stale "exception pending" survives its exception.
    env.status_word &= u16::try_from(excepts ^ FE_ALL_EXCEPT).unwrap_or(0);
    fldenv(&env);
    ldmxcsr(stmxcsr() & !u32::try_from(excepts).unwrap_or(0));
    0
}

/// Raise the exceptions in `excepts`, one at a time and in C's order, by
/// doing an operation that raises each -- so that one whose trap is enabled
/// traps, as glibc's `fraiseexcpt.c` intends.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feraiseexcept(excepts: i32) -> i32 {
    if excepts & FE_INVALID != 0 {
        raise_by_division(0.0, 0.0, FE_INVALID);
    }
    if excepts & FE_DIVBYZERO != 0 {
        raise_by_division(1.0, 0.0, FE_DIVBYZERO);
    }
    // Overflow, underflow and inexact have no operation that raises one
    // alone: set the x87 flag and let `fwait` deliver it.
    for bit in [FE_OVERFLOW, FE_UNDERFLOW, FE_INEXACT] {
        if excepts & bit != 0 {
            raise_in_x87(bit);
        }
    }
    0
}

/// Raise invalid (0/0) or divide-by-zero (1/0) as glibc does: by an SSE
/// division, so that an exception whose trap MXCSR enables traps. Every
/// SlateOS program has SSE -- the libc is built for
/// `x86_64-slateos-libc.json`, `+sse,+sse2`.
#[cfg(target_feature = "sse")]
fn raise_by_division(num: f32, den: f32, _bit: i32) {
    let mut f = num;
    // SAFETY: a register-only SSE division, which raises the flag it is
    // chosen for and touches no memory.
    unsafe { asm!("divss {0}, {1}", inout(xmm_reg) f, in(xmm_reg) den, options(nomem, nostack)) };
    let _ = f;
}

/// The crate's other build -- the soft-float `x86_64-unknown-none` its
/// `.cargo/config.toml` pins for a plain `cargo check`, which push gate 40
/// compiles -- has no SSE registers to name, and no SSE environment for a
/// flag to be in: there the flag is raised in the x87 unit, as the other
/// three always are.
#[cfg(not(target_feature = "sse"))]
fn raise_by_division(_num: f32, _den: f32, bit: i32) {
    raise_in_x87(bit);
}

/// Set `bit` in the x87 status word and let `fwait` deliver it -- trapping
/// if its trap is enabled, as raising must.
fn raise_in_x87(bit: i32) {
    let mut env = FenvT::default();
    fnstenv(&mut env);
    env.status_word |= u16::try_from(bit).unwrap_or(0);
    fldenv(&env);
    // SAFETY: `fwait` delivers pending unmasked x87 exceptions; it touches
    // no memory.
    unsafe { asm!("fwait", options(nomem, nostack)) };
}

/// Set the flags in `excepts` without raising them (C23; glibc's
/// `fesetexcept.c`: in the SSE unit, which is guaranteed not to trap).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fesetexcept(excepts: i32) -> i32 {
    ldmxcsr(stmxcsr() | u32::try_from(excepts & FE_ALL_EXCEPT).unwrap_or(0));
    0
}

/// Which of `excepts` are set, in either unit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fetestexcept(excepts: i32) -> i32 {
    flags() & excepts & FE_ALL_EXCEPT
}

/// Save the flags in `excepts` into `*flagp`. A NULL `flagp` is not written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fegetexceptflag(flagp: *mut FexceptT, excepts: i32) -> i32 {
    if !flagp.is_null() {
        let v = u16::try_from(flags() & FE_ALL_EXCEPT & excepts).unwrap_or(0);
        // SAFETY: non-null, and the caller's `fexcept_t *`.
        unsafe { flagp.write(v) };
    }
    0
}

/// Set the flags in `excepts` to what `*flagp` saved, without raising them
/// (glibc's `fsetexcptflg.c`: clear in both units, set in the SSE unit).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fesetexceptflag(flagp: *const FexceptT, excepts: i32) -> i32 {
    if flagp.is_null() {
        return 0;
    }
    // SAFETY: non-null, and the caller's `const fexcept_t *`.
    let saved = i32::from(unsafe { flagp.read() });
    let excepts = excepts & FE_ALL_EXCEPT;
    let mut env = FenvT::default();
    fnstenv(&mut env);
    env.status_word &= !u16::try_from(excepts & !saved).unwrap_or(0);
    fldenv(&env);
    let m = stmxcsr();
    let e = u32::try_from(excepts).unwrap_or(0);
    let s = u32::try_from(saved & 0x3f).unwrap_or(0);
    ldmxcsr(m ^ ((m ^ s) & e));
    0
}

/// Which of `excepts` `*flagp` has set (C23; glibc's `ftestexceptflag.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fetestexceptflag(flagp: *const FexceptT, excepts: i32) -> i32 {
    if flagp.is_null() {
        return 0;
    }
    // SAFETY: non-null, and the caller's `const fexcept_t *`.
    i32::from(unsafe { flagp.read() }) & excepts & FE_ALL_EXCEPT
}

/// Unmask the exceptions in `excepts`, so each traps, and answer which were
/// unmasked before (a GNU extension, glibc's `feenablxcpt.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feenableexcept(excepts: i32) -> i32 {
    let excepts = excepts & FE_ALL_EXCEPT;
    let cw = fnstcw();
    let old = i32::from(!cw) & FE_ALL_EXCEPT;
    fldcw(cw & !u16::try_from(excepts).unwrap_or(0));
    ldmxcsr(stmxcsr() & !(u32::try_from(excepts).unwrap_or(0) << 7));
    old
}

/// Mask the exceptions in `excepts` again, and answer which were unmasked
/// before (glibc's `fedisblxcpt.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fedisableexcept(excepts: i32) -> i32 {
    let excepts = excepts & FE_ALL_EXCEPT;
    let cw = fnstcw();
    let old = i32::from(!cw) & FE_ALL_EXCEPT;
    fldcw(cw | u16::try_from(excepts).unwrap_or(0));
    ldmxcsr(stmxcsr() | (u32::try_from(excepts).unwrap_or(0) << 7));
    old
}

/// Which exceptions are unmasked (glibc's `fegetexcept.c`, from the x87
/// control word).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fegetexcept() -> i32 {
    i32::from(!fnstcw()) & FE_ALL_EXCEPT
}

// ---------------------------------------------------------------------------
// Rounding
// ---------------------------------------------------------------------------

/// The rounding direction, from the x87 control word (glibc's
/// `fegetround.c`: the SSE unit's is kept the same).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fegetround() -> i32 {
    i32::from(fnstcw()) & FE_TOWARDZERO
}

/// Round in `round` from now on, in both units; 1 (glibc's value) for a
/// `round` that is no rounding direction, which changes nothing.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fesetround(round: i32) -> i32 {
    if round & !FE_TOWARDZERO != 0 {
        return 1;
    }
    let r = u16::try_from(round).unwrap_or(0);
    fldcw((fnstcw() & !0xc00) | r);
    ldmxcsr((stmxcsr() & !MXCSR_ROUNDING) | (u32::from(r) << 3));
    0
}

/// `FLT_ROUNDS`, which musl's `<float.h>` defines as a call to this: 0
/// toward zero, 1 to nearest, 2 upward, 3 downward.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __flt_rounds() -> i32 {
    match fegetround() {
        FE_TOWARDZERO => 0,
        FE_TONEAREST => 1,
        FE_UPWARD => 2,
        FE_DOWNWARD => 3,
        _ => -1,
    }
}

// ---------------------------------------------------------------------------
// The whole environment
// ---------------------------------------------------------------------------

/// Save both units' environment into `*envp` (glibc's `fegetenv.c`: the
/// x87 image loaded straight back, since storing it masks every x87
/// exception). A NULL `envp` is not written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fegetenv(envp: *mut FenvT) -> i32 {
    if envp.is_null() {
        return 0;
    }
    let mut env = FenvT::default();
    fnstenv(&mut env);
    fldenv(&env);
    env.mxcsr = stmxcsr();
    // SAFETY: non-null, and the caller's `fenv_t *`.
    unsafe { envp.write(env) };
    0
}

/// Save the environment into `*envp`, then clear every flag and mask every
/// exception, so the code that follows runs without traps (glibc's
/// `feholdexcpt.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feholdexcept(envp: *mut FenvT) -> i32 {
    let mut env = FenvT::default();
    // `fnstenv` masks every x87 exception, which is half of the point here.
    fnstenv(&mut env);
    env.mxcsr = stmxcsr();
    // SAFETY: `fnclex` clears the x87 exception flags; it touches no memory.
    unsafe { asm!("fnclex", options(nomem, nostack)) };
    ldmxcsr((env.mxcsr | 0x1f80) & !0x3f);
    if !envp.is_null() {
        // SAFETY: non-null, and the caller's `fenv_t *`.
        unsafe { envp.write(env) };
    }
    0
}

/// Install the environment at `envp` -- or the default for `FE_DFL_ENV`,
/// or the default with every exception unmasked for `FE_NOMASK_ENV` --
/// keeping the fields glibc's `fesetenv.c` does not take from a saved one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fesetenv(envp: *const FenvT) -> i32 {
    let mut temp = FenvT::default();
    fnstenv(&mut temp);
    temp.mxcsr = stmxcsr();
    let all = u16::try_from(FE_ALL_EXCEPT_X86).unwrap_or(0);
    let all_mx = u32::try_from(FE_ALL_EXCEPT_X86).unwrap_or(0);
    let toward_zero = u16::try_from(FE_TOWARDZERO).unwrap_or(0);
    if envp == FE_DFL_ENV || envp == FE_NOMASK_ENV {
        if envp == FE_DFL_ENV {
            temp.control_word |= all;
            temp.control_word &= !toward_zero;
        } else {
            temp.control_word &= !(u16::try_from(FE_ALL_EXCEPT).unwrap_or(0) | toward_zero);
            // The denormal-operand exception stays masked.
            temp.control_word |= u16::try_from(FE_DENORM).unwrap_or(0);
        }
        temp.control_word |= FPU_EXTENDED;
        temp.status_word &= !all;
        temp.eip = 0;
        temp.cs_selector = 0;
        temp.opcode = 0;
        temp.data_offset = 0;
        temp.data_selector = 0;
        temp.mxcsr &= !all_mx;
        if envp == FE_DFL_ENV {
            temp.mxcsr |= all_mx << 7;
        } else {
            temp.mxcsr &= !(u32::try_from(FE_ALL_EXCEPT).unwrap_or(0) << 7);
            temp.mxcsr |= u32::try_from(FE_DENORM).unwrap_or(0) << 7;
        }
        temp.mxcsr &= !MXCSR_ROUNDING;
        temp.mxcsr &= !MXCSR_FZ_DAZ;
    } else if !envp.is_null() {
        // SAFETY: neither sentinel and non-null: the caller's `const
        // fenv_t *`, a whole environment.
        let e = unsafe { envp.read() };
        let keep = all | toward_zero | FPU_EXTENDED;
        temp.control_word = (temp.control_word & !keep) | (e.control_word & keep);
        temp.status_word = (temp.status_word & !all) | (e.status_word & all);
        temp.eip = e.eip;
        temp.cs_selector = e.cs_selector;
        temp.opcode = e.opcode;
        temp.data_offset = e.data_offset;
        temp.data_selector = e.data_selector;
        temp.mxcsr = e.mxcsr;
    }
    fldenv(&temp);
    ldmxcsr(temp.mxcsr);
    0
}

/// Install the environment at `envp`, then raise the exceptions that were
/// set before -- so a caller that held exceptions with [`feholdexcept`] gets
/// them now (glibc's `feupdateenv.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feupdateenv(envp: *const FenvT) -> i32 {
    let saved = flags() & FE_ALL_EXCEPT;
    fesetenv(envp);
    feraiseexcept(saved);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Put the thread back as a test found it: libtest runs each test on a
    /// thread of its own, but a test that failed half-way should not leave
    /// an upward rounding behind for the assertions after it.
    struct Restore(FenvT);

    impl Restore {
        fn take() -> Self {
            let mut e = FenvT::default();
            fegetenv(&raw mut e);
            Self(e)
        }
    }

    impl Drop for Restore {
        fn drop(&mut self) {
            fesetenv(&raw const self.0);
        }
    }

    /// `fenv_t` is the x87 image and MXCSR: 32 bytes, MXCSR at 28.
    #[test]
    fn fenv_t_is_the_x87_image_then_mxcsr() {
        assert_eq!(core::mem::size_of::<FenvT>(), 32);
        assert_eq!(core::mem::offset_of!(FenvT, status_word), 4);
        assert_eq!(core::mem::offset_of!(FenvT, opcode), 18);
        assert_eq!(core::mem::offset_of!(FenvT, mxcsr), 28);
    }

    /// The rounding direction set is the one `rint` rounds in -- both units,
    /// since `rint` is SSE arithmetic -- and the one read back.
    #[test]
    fn fesetround_changes_how_rint_rounds() {
        let _r = Restore::take();
        assert_eq!(fegetround(), FE_TONEAREST);
        assert_eq!(crate::math::rint(2.5), 2.0);
        for (mode, up, down) in [
            (FE_UPWARD, 3.0, -2.0),
            (FE_DOWNWARD, 2.0, -3.0),
            (FE_TOWARDZERO, 2.0, -2.0),
            (FE_TONEAREST, 2.0, -2.0),
        ] {
            assert_eq!(fesetround(mode), 0);
            assert_eq!(fegetround(), mode);
            let x = core::hint::black_box(2.5_f64);
            assert_eq!(crate::math::rint(x), up, "rint(2.5) in {mode:#x}");
            assert_eq!(crate::math::rint(-x), down, "rint(-2.5) in {mode:#x}");
        }
        assert_eq!(fesetround(0x123), 1, "no rounding direction: glibc's 1");
        assert_eq!(fegetround(), FE_TONEAREST, "and nothing changed");
    }

    /// `nearbyint` rounds as `rint` does and leaves the flags as it found
    /// them; `rint` raises inexact for a fraction.
    #[test]
    fn nearbyint_raises_nothing_and_rint_raises_inexact() {
        let _r = Restore::take();
        feclearexcept(FE_ALL_EXCEPT);
        let x = core::hint::black_box(2.5_f64);
        assert_eq!(crate::math::nearbyint(x), 2.0);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), 0, "nearbyint raised a flag");
        fesetround(FE_UPWARD);
        assert_eq!(
            crate::math::nearbyint(x),
            3.0,
            "nearbyint follows the direction"
        );
        assert_eq!(crate::math::nearbyintf(2.5), 3.0);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), 0);
        fesetround(FE_TONEAREST);
        assert_eq!(crate::math::rint(x), 2.0);
        assert_eq!(
            fetestexcept(FE_INEXACT),
            FE_INEXACT,
            "rint does raise inexact"
        );
        feraiseexcept(FE_OVERFLOW);
        let _ = crate::math::nearbyint(x);
        assert_eq!(
            fetestexcept(FE_ALL_EXCEPT),
            FE_OVERFLOW | FE_INEXACT,
            "flags before it survive"
        );
    }

    /// A flag raised stays until cleared; raising and testing agree, and
    /// the saved flags restore what they saved.
    #[test]
    fn flags_are_raised_tested_saved_and_cleared() {
        let _r = Restore::take();
        feclearexcept(FE_ALL_EXCEPT);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), 0);
        let zero = core::hint::black_box(0.0_f64);
        let _ = core::hint::black_box(1.0 / zero);
        assert_eq!(
            fetestexcept(FE_ALL_EXCEPT),
            FE_DIVBYZERO,
            "1/0 is division by zero"
        );
        feraiseexcept(FE_INVALID | FE_OVERFLOW);
        assert_eq!(
            fetestexcept(FE_INVALID | FE_OVERFLOW | FE_DIVBYZERO),
            FE_INVALID | FE_OVERFLOW | FE_DIVBYZERO
        );
        let mut saved: FexceptT = 0;
        fegetexceptflag(&raw mut saved, FE_OVERFLOW | FE_INEXACT);
        assert_eq!(i32::from(saved), FE_OVERFLOW);
        assert_eq!(
            fetestexceptflag(&raw const saved, FE_ALL_EXCEPT),
            FE_OVERFLOW
        );
        feclearexcept(FE_ALL_EXCEPT);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), 0);
        fesetexceptflag(&raw const saved, FE_ALL_EXCEPT);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), FE_OVERFLOW);
        fesetexcept(FE_UNDERFLOW);
        assert_eq!(fetestexcept(FE_UNDERFLOW), FE_UNDERFLOW);
        feclearexcept(FE_OVERFLOW);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), FE_UNDERFLOW);
    }

    /// `feholdexcept` saves, clears and masks; `feupdateenv` restores and
    /// raises what happened meanwhile; `FE_DFL_ENV` is the starting state.
    #[test]
    fn hold_update_and_the_default_environment() {
        let _r = Restore::take();
        fesetround(FE_UPWARD);
        feraiseexcept(FE_INEXACT);
        let mut held = FenvT::default();
        feholdexcept(&raw mut held);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), 0, "held: cleared");
        assert_eq!(fegetround(), FE_UPWARD, "the rounding direction is kept");
        feraiseexcept(FE_DIVBYZERO);
        feupdateenv(&raw const held);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), FE_INEXACT | FE_DIVBYZERO);
        fesetenv(FE_DFL_ENV);
        assert_eq!(fegetround(), FE_TONEAREST);
        assert_eq!(fetestexcept(FE_ALL_EXCEPT), 0);
        assert_eq!(fegetexcept(), 0, "every exception masked");
        assert_eq!(__flt_rounds(), 1);
    }

    /// Enabling an exception unmasks it in both units and reports the old
    /// set; disabling masks it again. (Nothing is raised meanwhile: an
    /// unmasked exception would trap the test.)
    #[test]
    fn exceptions_are_enabled_and_disabled_in_both_units() {
        let _r = Restore::take();
        assert_eq!(feenableexcept(FE_DIVBYZERO | FE_INVALID), 0);
        assert_eq!(fegetexcept(), FE_DIVBYZERO | FE_INVALID);
        let masks = u32::try_from(FE_DIVBYZERO | FE_INVALID).unwrap() << 7;
        assert_eq!(stmxcsr() & masks, 0, "SSE masks cleared");
        assert_eq!(fedisableexcept(FE_INVALID), FE_DIVBYZERO | FE_INVALID);
        assert_eq!(fegetexcept(), FE_DIVBYZERO);
        fedisableexcept(FE_ALL_EXCEPT);
        assert_eq!(fegetexcept(), 0);
    }
}
