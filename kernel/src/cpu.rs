//! Low-level CPU control instructions.
//!
//! Wrappers around privileged x86_64 instructions that the kernel uses
//! for interrupt management, halting, and other CPU-level operations.

/// Halt the CPU until the next interrupt arrives.
///
/// This is the standard idle instruction — it puts the CPU into a
/// low-power state until an interrupt fires.
#[inline]
pub fn hlt() {
    // SAFETY: `hlt` is always safe to execute in ring 0.  It simply
    // waits for the next interrupt.
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

/// Disable hardware interrupts (clear the IF flag in RFLAGS).
///
/// # Safety
///
/// Disabling interrupts for too long will cause the system to become
/// unresponsive (no timer ticks, no keyboard input, etc.).  The caller
/// must re-enable interrupts in a timely manner.
#[inline]
pub unsafe fn cli() {
    // SAFETY: Caller will re-enable interrupts.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// Enable hardware interrupts (set the IF flag in RFLAGS).
///
/// # Safety
///
/// The IDT must be properly initialized before enabling interrupts.
/// Enabling interrupts with an uninitialized IDT will triple-fault.
#[inline]
pub unsafe fn sti() {
    // SAFETY: Caller guarantees the IDT is set up.
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Halt the CPU forever with interrupts disabled.
///
/// Used as the final stop in panic handlers and fatal error paths.
/// The CPU will never execute another instruction after this.
pub fn halt_loop() -> ! {
    loop {
        // SAFETY: We intentionally disable interrupts and halt forever.
        // This is a terminal state — the system is dead.
        unsafe {
            cli();
        }
        hlt();
    }
}

/// Read the current value of the RFLAGS register.
#[inline]
pub fn read_rflags() -> u64 {
    let rflags: u64;
    // SAFETY: Reading RFLAGS is always safe.
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            out(reg) rflags,
            options(nomem),
        );
    }
    rflags
}

/// Check whether hardware interrupts are currently enabled.
#[inline]
#[must_use]
pub fn interrupts_enabled() -> bool {
    const IF_FLAG: u64 = 1 << 9;
    read_rflags() & IF_FLAG != 0
}

/// Execute a closure with interrupts disabled, restoring the previous
/// interrupt state afterward.
///
/// This is the standard pattern for short critical sections that must
/// not be interrupted.
///
/// # Safety
///
/// The closure must not enable interrupts itself, and must complete
/// quickly to avoid excessive interrupt latency.
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let were_enabled = interrupts_enabled();
    if were_enabled {
        // SAFETY: We restore the interrupt state after the closure.
        unsafe { cli(); }
    }
    let result = f();
    if were_enabled {
        // SAFETY: The IDT was already set up (interrupts were enabled
        // before we disabled them).
        unsafe { sti(); }
    }
    result
}

/// Read a Model-Specific Register (MSR).
///
/// # Safety
///
/// The caller must ensure `msr` is a valid MSR number for the current
/// CPU.  Reading an invalid MSR causes a General Protection Fault.
#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY: Caller guarantees a valid MSR.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
    u64::from(high) << 32 | u64::from(low)
}

/// Write a Model-Specific Register (MSR).
///
/// # Safety
///
/// The caller must ensure `msr` is a valid MSR number and `value` is
/// a valid value for that MSR.  Writing invalid values can crash the
/// system or corrupt its state.
#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    // SAFETY: Caller guarantees valid MSR and value.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
}
