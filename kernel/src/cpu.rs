//! Low-level CPU control instructions.
//!
//! Wrappers around privileged `x86_64` instructions that the kernel uses
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

/// Read the current stack pointer (RSP).
///
/// In a panic handler, the value reflects the handler's stack frame
/// rather than the faulting instruction.  Still useful for estimating
/// remaining stack space relative to the task's `stack_bottom`.
#[inline]
#[must_use]
pub fn read_rsp() -> u64 {
    let rsp: u64;
    // SAFETY: Reading RSP is always safe.
    unsafe {
        core::arch::asm!(
            "mov {}, rsp",
            out(reg) rsp,
            options(nomem, nostack, preserves_flags),
        );
    }
    rsp
}

/// Read the current value of the RFLAGS register.
#[inline]
#[allow(dead_code)] // Used by without_interrupts and future CPU state inspection.
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
#[allow(dead_code)] // Used by without_interrupts and debugging.
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
#[allow(dead_code)] // Standard critical-section primitive, will be used widely.
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

// ---------------------------------------------------------------------------
// TSC-based precise delay functions
// ---------------------------------------------------------------------------

/// Busy-wait for approximately `us` microseconds using the TSC.
///
/// Uses the calibrated TSC frequency from `bench::tsc_freq()`.
/// Falls back to an estimated loop if TSC is not yet calibrated
/// (assumes ~1 GHz TSC as a conservative estimate).
///
/// This is the standard delay primitive for hardware initialization
/// (reset waits, register polling, PCI config space timing).  For
/// sleep-style waiting that yields the CPU to other tasks, use the
/// timer/scheduler sleep instead.
///
/// # Performance
///
/// TSC resolution is typically 1-4 GHz on modern hardware (0.25-1ns
/// per tick), so microsecond delays are highly accurate.  Under QEMU
/// TCG, TSC emulation introduces ~20x overhead but relative delays
/// are still correct.
#[inline]
pub fn delay_us(us: u64) {
    let freq = crate::bench::tsc_freq();
    if freq == 0 {
        // Fallback: assume ~1 GHz TSC, loop approximately.
        let target_ticks = us.saturating_mul(1000);
        let start = crate::bench::rdtsc();
        while crate::bench::rdtsc().wrapping_sub(start) < target_ticks {
            core::hint::spin_loop();
        }
        return;
    }

    // target_cycles = us * freq / 1_000_000
    let target_cycles = us.saturating_mul(freq) / 1_000_000;
    let start = crate::bench::rdtsc();
    while crate::bench::rdtsc().wrapping_sub(start) < target_cycles {
        core::hint::spin_loop();
    }
}

/// Busy-wait for approximately `ns` nanoseconds using the TSC.
///
/// For sub-microsecond hardware timing requirements (register settling,
/// PHY reset hold times).  Accuracy depends on TSC resolution — delays
/// shorter than ~100ns are dominated by measurement overhead.
///
/// # Performance
///
/// The overhead of calling this function and reading rdtsc is typically
/// 20-200 cycles (~10-100ns), so very short delays (< 100ns) should
/// use `core::hint::spin_loop()` directly instead.
#[inline]
#[allow(dead_code)] // Public API for drivers and hardware init timing.
pub fn delay_ns(ns: u64) {
    let freq = crate::bench::tsc_freq();
    if freq == 0 {
        // Fallback: best-effort short spin.  Each spin_loop() is ~1ns
        // on modern hardware with PAUSE instruction.
        for _ in 0..ns {
            core::hint::spin_loop();
        }
        return;
    }

    // target_cycles = ns * freq / 1_000_000_000
    // To avoid overflow on large ns values: split the division.
    // freq / 1000 loses at most 999 Hz of precision (~0.0001% for GHz clocks).
    let target_cycles = ns
        .saturating_mul(freq / 1000)
        / 1_000_000;
    if target_cycles == 0 {
        // Sub-cycle delay — just do a single spin.
        core::hint::spin_loop();
        return;
    }
    let start = crate::bench::rdtsc();
    while crate::bench::rdtsc().wrapping_sub(start) < target_cycles {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Model-Specific Register (MSR) access
// ---------------------------------------------------------------------------

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
// Splitting a u64 into two u32 halves for the `wrmsr` instruction is
// intentionally truncating — each half is a separate 32-bit operand.
#[allow(clippy::cast_possible_truncation)]
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
