//! Low-level CPU control instructions and feature detection.
//!
//! Wrappers around privileged `x86_64` instructions that the kernel uses
//! for interrupt management, halting, and other CPU-level operations.
//!
//! ## CPU Feature Detection
//!
//! The [`CpuFeatures`] struct is populated once at boot via [`detect_features`]
//! and cached globally.  Subsystems query features via [`features()`] instead
//! of running CPUID directly — this avoids duplicate calls and provides a
//! single source of truth for CPU capabilities.

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

// ---------------------------------------------------------------------------
// CPU Feature Detection
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicBool, Ordering};

/// Whether features have been detected (gate for [`features()`]).
static FEATURES_DETECTED: AtomicBool = AtomicBool::new(false);

/// Cached CPU features (set once during boot).
static mut CPU_FEATURES: CpuFeatures = CpuFeatures::empty();

/// CPU feature flags detected via CPUID at boot time.
///
/// Populated by [`detect_features`] and accessed via [`features()`].
/// All fields are `bool` for clarity — the struct is only 64 bytes.
#[derive(Debug, Clone, Copy)]
pub struct CpuFeatures {
    // --- CPUID leaf 1, ECX ---
    /// SSE3 (Streaming SIMD Extensions 3).
    pub sse3: bool,
    /// SSSE3 (Supplemental SSE3).
    pub ssse3: bool,
    /// SSE4.1.
    pub sse4_1: bool,
    /// SSE4.2.
    pub sse4_2: bool,
    /// POPCNT instruction.
    pub popcnt: bool,
    /// AVX (Advanced Vector Extensions) — 256-bit YMM registers.
    pub avx: bool,
    /// XSAVE/XRSTOR/XSETBV/XGETBV support.
    pub xsave: bool,
    /// OSXSAVE — OS has enabled XSAVE (CR4.OSXSAVE = 1).
    pub osxsave: bool,
    /// AES-NI (hardware AES instructions).
    pub aes_ni: bool,
    /// RDRAND (hardware random number generator).
    pub rdrand: bool,
    /// F16C (half-precision float conversion).
    pub f16c: bool,

    // --- CPUID leaf 1, EDX ---
    /// FXSAVE/FXRSTOR support (always true on x86_64).
    pub fxsr: bool,
    /// SSE (always true on x86_64).
    pub sse: bool,
    /// SSE2 (always true on x86_64).
    pub sse2: bool,
    /// TSC (Time Stamp Counter).
    pub tsc: bool,
    /// APIC on-chip.
    pub apic: bool,

    // --- CPUID leaf 7, subleaf 0, EBX ---
    /// AVX2 — 256-bit integer SIMD.
    pub avx2: bool,
    /// BMI1 (Bit Manipulation Instructions, group 1).
    pub bmi1: bool,
    /// BMI2 (Bit Manipulation Instructions, group 2).
    pub bmi2: bool,
    /// AVX-512 Foundation.
    pub avx512f: bool,
    /// SHA extensions (hardware SHA-1/SHA-256).
    pub sha: bool,
    /// RDSEED (hardware random seed).
    pub rdseed: bool,

    // --- CPUID leaf 7, subleaf 0, ECX ---
    /// VAES (vectorized AES).
    pub vaes: bool,
    /// RDPID (read processor ID without TSC).
    pub rdpid: bool,

    // --- CPUID leaf 0x80000001, EDX ---
    /// RDTSCP (read TSC + processor ID atomically).
    pub rdtscp: bool,
    /// 1 GiB pages (PDPE 1G).
    pub page_1g: bool,

    // --- CPUID leaf 0xD, subleaf 0 (XSAVE info) ---
    /// Maximum XSAVE area size (bytes) for all supported features.
    /// 0 if XSAVE not supported.
    pub xsave_area_size: u32,
    /// XCR0 supported feature bits (low 32 bits).
    pub xcr0_supported: u64,
}

impl CpuFeatures {
    /// Create an empty features struct (all false/zero).
    const fn empty() -> Self {
        Self {
            sse3: false, ssse3: false, sse4_1: false, sse4_2: false,
            popcnt: false, avx: false, xsave: false, osxsave: false,
            aes_ni: false, rdrand: false, f16c: false,
            fxsr: false, sse: false, sse2: false, tsc: false, apic: false,
            avx2: false, bmi1: false, bmi2: false, avx512f: false,
            sha: false, rdseed: false, vaes: false, rdpid: false,
            rdtscp: false, page_1g: false,
            xsave_area_size: 0, xcr0_supported: 0,
        }
    }
}

/// Detect CPU features via CPUID and cache the results.
///
/// Must be called once during early boot (before any subsystem queries
/// features).  Safe to call multiple times — subsequent calls are no-ops.
///
/// # Safety
///
/// Must be called from a single thread (boot CPU, interrupts disabled).
pub fn detect_features() {
    if FEATURES_DETECTED.load(Ordering::Acquire) {
        return;
    }

    let mut f = CpuFeatures::empty();

    // --- Leaf 1: basic feature flags ---
    let (ecx1, edx1) = cpuid_leaf1();
    f.sse3 = ecx1 & (1 << 0) != 0;
    f.ssse3 = ecx1 & (1 << 9) != 0;
    f.sse4_1 = ecx1 & (1 << 19) != 0;
    f.sse4_2 = ecx1 & (1 << 20) != 0;
    f.popcnt = ecx1 & (1 << 23) != 0;
    f.xsave = ecx1 & (1 << 26) != 0;
    f.osxsave = ecx1 & (1 << 27) != 0;
    f.avx = ecx1 & (1 << 28) != 0;
    f.aes_ni = ecx1 & (1 << 25) != 0;
    f.rdrand = ecx1 & (1 << 30) != 0;
    f.f16c = ecx1 & (1 << 29) != 0;

    f.fxsr = edx1 & (1 << 24) != 0;
    f.sse = edx1 & (1 << 25) != 0;
    f.sse2 = edx1 & (1 << 26) != 0;
    f.tsc = edx1 & (1 << 4) != 0;
    f.apic = edx1 & (1 << 9) != 0;

    // --- Leaf 7, subleaf 0: structured extended features ---
    let max_leaf = cpuid_max_leaf();
    if max_leaf >= 7 {
        let (ebx7, ecx7) = cpuid_leaf7_sub0();
        f.avx2 = ebx7 & (1 << 5) != 0;
        f.bmi1 = ebx7 & (1 << 3) != 0;
        f.bmi2 = ebx7 & (1 << 8) != 0;
        f.avx512f = ebx7 & (1 << 16) != 0;
        f.sha = ebx7 & (1 << 29) != 0;
        f.rdseed = ebx7 & (1 << 18) != 0;
        f.vaes = ecx7 & (1 << 9) != 0;
        f.rdpid = ecx7 & (1 << 22) != 0;
    }

    // --- Leaf 0x80000001: extended features ---
    let max_ext_leaf = cpuid_max_extended_leaf();
    if max_ext_leaf >= 0x8000_0001 {
        let edx_ext1 = cpuid_extended_leaf1_edx();
        f.rdtscp = edx_ext1 & (1 << 27) != 0;
        f.page_1g = edx_ext1 & (1 << 26) != 0;
    }

    // --- Leaf 0xD, subleaf 0: XSAVE area info ---
    if f.xsave && max_leaf >= 0xD {
        let (eax_d, _ebx_d, ecx_d, edx_d) = cpuid_leaf_d_sub0();
        f.xcr0_supported = u64::from(eax_d) | (u64::from(edx_d) << 32);
        f.xsave_area_size = ecx_d; // Maximum size for all features.
    }

    // SAFETY: We're the only writer (single-threaded boot), and readers
    // won't access until FEATURES_DETECTED is set (Acquire/Release).
    unsafe {
        CPU_FEATURES = f;
    }
    FEATURES_DETECTED.store(true, Ordering::Release);
}

/// Get the cached CPU features.
///
/// Returns `None` if [`detect_features`] hasn't been called yet.
#[must_use]
pub fn features() -> Option<&'static CpuFeatures> {
    if FEATURES_DETECTED.load(Ordering::Acquire) {
        // SAFETY: FEATURES_DETECTED guarantees CPU_FEATURES is fully
        // written and will never be written again.
        Some(unsafe { &*core::ptr::addr_of!(CPU_FEATURES) })
    } else {
        None
    }
}

/// Log detected CPU features to serial.
///
/// Called once during boot after [`detect_features`].
pub fn log_features() {
    let Some(f) = features() else { return };

    crate::serial_println!("[cpu] Feature detection:");
    crate::serial_println!(
        "[cpu]   SSE3={} SSSE3={} SSE4.1={} SSE4.2={} POPCNT={}",
        f.sse3, f.ssse3, f.sse4_1, f.sse4_2, f.popcnt
    );
    crate::serial_println!(
        "[cpu]   AVX={} AVX2={} AVX-512F={} XSAVE={}",
        f.avx, f.avx2, f.avx512f, f.xsave
    );
    if f.xsave {
        crate::serial_println!(
            "[cpu]   XSAVE area: {} bytes, XCR0 supported: {:#x}",
            f.xsave_area_size, f.xcr0_supported
        );
    }
    crate::serial_println!(
        "[cpu]   AES-NI={} SHA={} RDRAND={} RDSEED={}",
        f.aes_ni, f.sha, f.rdrand, f.rdseed
    );
    crate::serial_println!(
        "[cpu]   RDTSCP={} 1GiB pages={} TSC={}",
        f.rdtscp, f.page_1g, f.tsc
    );
}

// ---------------------------------------------------------------------------
// CPUID helper functions
// ---------------------------------------------------------------------------

/// CPUID leaf 0: maximum supported standard leaf number.
fn cpuid_max_leaf() -> u32 {
    let eax: u32;
    // SAFETY: CPUID leaf 0 is always valid on x86_64.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "xor eax, eax",
            "cpuid",
            "pop rbx",
            out("eax") eax,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    eax
}

/// CPUID leaf 1: returns (ECX, EDX) feature flags.
fn cpuid_leaf1() -> (u32, u32) {
    let ecx: u32;
    let edx: u32;
    // SAFETY: CPUID leaf 1 is always valid on x86_64.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            out("eax") _,
            out("ecx") ecx,
            out("edx") edx,
            options(nomem, nostack),
        );
    }
    (ecx, edx)
}

/// CPUID leaf 7, subleaf 0: returns (EBX, ECX) structured extended features.
fn cpuid_leaf7_sub0() -> (u32, u32) {
    let ebx: u32;
    let ecx: u32;
    // SAFETY: Caller verified max_leaf >= 7.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 7",
            "xor ecx, ecx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            ebx_out = out(reg) ebx,
            out("eax") _,
            out("ecx") ecx,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    (ebx, ecx)
}

/// CPUID extended leaf 0x80000000: maximum supported extended leaf.
fn cpuid_max_extended_leaf() -> u32 {
    let eax: u32;
    // SAFETY: Extended CPUID leaf 0x80000000 is always valid on x86_64.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 0x80000000",
            "cpuid",
            "pop rbx",
            out("eax") eax,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    eax
}

/// CPUID extended leaf 0x80000001: returns EDX extended features.
fn cpuid_extended_leaf1_edx() -> u32 {
    let edx: u32;
    // SAFETY: Caller verified max_ext_leaf >= 0x80000001.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 0x80000001",
            "cpuid",
            "pop rbx",
            out("eax") _,
            out("ecx") _,
            out("edx") edx,
            options(nomem, nostack),
        );
    }
    edx
}

/// CPUID leaf 0xD, subleaf 0: XSAVE area information.
///
/// Returns (EAX, EBX, ECX, EDX):
/// - EAX: XCR0 supported bits (low 32)
/// - EBX: max XSAVE area size for currently-enabled features
/// - ECX: max XSAVE area size for all supported features
/// - EDX: XCR0 supported bits (high 32)
fn cpuid_leaf_d_sub0() -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY: Caller verified xsave is supported and max_leaf >= 0xD.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 0xD",
            "xor ecx, ecx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            ebx_out = out(reg) ebx,
            out("eax") eax,
            out("ecx") ecx,
            out("edx") edx,
            options(nomem, nostack),
        );
    }
    (eax, ebx, ecx, edx)
}
