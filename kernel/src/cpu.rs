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

/// Read the CR2 register (last page fault linear address).
///
/// Useful in panic diagnostics to show if a page fault contributed
/// to the crash, even if the panic isn't directly in the page fault
/// handler.
#[inline]
#[must_use]
#[allow(dead_code)]
pub fn read_cr2() -> u64 {
    let cr2: u64;
    // SAFETY: Reading CR2 is always safe in ring 0.
    unsafe {
        core::arch::asm!(
            "mov {}, cr2",
            out(reg) cr2,
            options(nomem, nostack, preserves_flags),
        );
    }
    cr2
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
        irqoff_tracker::record_disable();
    }
    let result = f();
    if were_enabled {
        irqoff_tracker::record_enable();
        // SAFETY: The IDT was already set up (interrupts were enabled
        // before we disabled them).
        unsafe { sti(); }
    }
    result
}

// ---------------------------------------------------------------------------
// Cache topology detection
// ---------------------------------------------------------------------------

/// Maximum number of cache levels to detect.
const MAX_CACHE_LEVELS: usize = 4;

/// Information about a single cache level.
#[derive(Debug, Clone, Copy)]
pub struct CacheInfo {
    /// Cache level (1 = L1, 2 = L2, 3 = L3, etc.).
    pub level: u8,
    /// Cache type: 1 = data, 2 = instruction, 3 = unified.
    pub cache_type: u8,
    /// Total cache size in bytes.
    pub size: u32,
    /// Cache line size in bytes (typically 64).
    pub line_size: u16,
    /// Number of ways of associativity.
    pub ways: u16,
    /// Number of sets.
    pub sets: u32,
    /// Whether this cache is shared across cores.
    pub shared: bool,
    /// Maximum number of logical processors sharing this cache.
    pub max_sharing: u16,
}

impl CacheInfo {
    const fn empty() -> Self {
        Self {
            level: 0,
            cache_type: 0,
            size: 0,
            line_size: 0,
            ways: 0,
            sets: 0,
            shared: false,
            max_sharing: 0,
        }
    }

    /// Human-readable cache type name.
    pub fn type_name(&self) -> &'static str {
        match self.cache_type {
            1 => "Data",
            2 => "Instruction",
            3 => "Unified",
            _ => "Unknown",
        }
    }
}

/// Detected cache topology.
static mut CACHE_TOPOLOGY: [CacheInfo; MAX_CACHE_LEVELS] = [CacheInfo::empty(); MAX_CACHE_LEVELS];

/// Number of valid entries in CACHE_TOPOLOGY.
static CACHE_LEVELS_DETECTED: core::sync::atomic::AtomicU8 =
    core::sync::atomic::AtomicU8::new(0);

/// Cache line size (bytes) — the L1 data cache line size.
/// Defaults to 64 if detection fails.
static CACHE_LINE_SIZE: core::sync::atomic::AtomicU16 =
    core::sync::atomic::AtomicU16::new(64);

/// Detect cache topology via CPUID leaf 4 (Intel) or leaf 0x8000001D (AMD).
///
/// Must be called after [`detect_features`] during early boot.
pub fn detect_cache_topology() {
    let max_leaf = cpuid_max_leaf();

    // Try Intel deterministic cache parameters (leaf 4).
    if max_leaf >= 4 {
        detect_cache_intel();
    } else {
        // Try AMD extended topology (leaf 0x8000001D).
        let max_ext = cpuid_max_extended_leaf();
        if max_ext >= 0x8000_001D {
            detect_cache_amd();
        }
    }

    // Set the global cache line size from L1 data cache.
    let count = CACHE_LEVELS_DETECTED.load(core::sync::atomic::Ordering::Relaxed) as usize;
    // SAFETY: We just wrote CACHE_TOPOLOGY during boot, single-threaded.
    for i in 0..count {
        let entry = unsafe {
            let ptr = core::ptr::addr_of!(CACHE_TOPOLOGY) as *const CacheInfo;
            core::ptr::read(ptr.add(i))
        };
        if entry.level == 1 && (entry.cache_type == 1 || entry.cache_type == 3) {
            CACHE_LINE_SIZE.store(entry.line_size, core::sync::atomic::Ordering::Relaxed);
            break;
        }
    }
}

/// Intel deterministic cache parameters (CPUID leaf 4).
fn detect_cache_intel() {
    let mut idx: usize = 0;
    for subleaf in 0..MAX_CACHE_LEVELS {
        let (eax, ebx, ecx, _edx) = cpuid_leaf4(subleaf as u32);
        let cache_type = eax & 0x1F;
        if cache_type == 0 {
            break; // No more caches.
        }
        let level = ((eax >> 5) & 0x7) as u8;
        let max_sharing = ((eax >> 14) & 0xFFF) + 1;
        let line_size = ((ebx & 0xFFF) + 1) as u16;
        let partitions = ((ebx >> 12) & 0x3FF) + 1;
        let ways = ((ebx >> 22) & 0x3FF) + 1;
        let sets = ecx + 1;

        // size = ways × partitions × line_size × sets
        let size = ways
            .saturating_mul(partitions)
            .saturating_mul(line_size as u32)
            .saturating_mul(sets);

        if idx < MAX_CACHE_LEVELS {
            // SAFETY: Single-threaded boot, no concurrent readers yet.
            unsafe {
                CACHE_TOPOLOGY[idx] = CacheInfo {
                    level,
                    cache_type: cache_type as u8,
                    size,
                    line_size,
                    ways: ways as u16,
                    sets,
                    shared: max_sharing > 1,
                    max_sharing: max_sharing as u16,
                };
            }
            idx += 1;
        }
    }
    CACHE_LEVELS_DETECTED.store(idx as u8, core::sync::atomic::Ordering::Release);
}

/// AMD extended cache topology (CPUID leaf 0x8000001D).
/// Same format as Intel leaf 4.
fn detect_cache_amd() {
    let mut idx: usize = 0;
    for subleaf in 0..MAX_CACHE_LEVELS {
        let (eax, ebx, ecx, _edx) = cpuid_ext_1d(subleaf as u32);
        let cache_type = eax & 0x1F;
        if cache_type == 0 {
            break;
        }
        let level = ((eax >> 5) & 0x7) as u8;
        let max_sharing = ((eax >> 14) & 0xFFF) + 1;
        let line_size = ((ebx & 0xFFF) + 1) as u16;
        let partitions = ((ebx >> 12) & 0x3FF) + 1;
        let ways = ((ebx >> 22) & 0x3FF) + 1;
        let sets = ecx + 1;

        let size = ways
            .saturating_mul(partitions)
            .saturating_mul(line_size as u32)
            .saturating_mul(sets);

        if idx < MAX_CACHE_LEVELS {
            unsafe {
                CACHE_TOPOLOGY[idx] = CacheInfo {
                    level,
                    cache_type: cache_type as u8,
                    size,
                    line_size,
                    ways: ways as u16,
                    sets,
                    shared: max_sharing > 1,
                    max_sharing: max_sharing as u16,
                };
            }
            idx += 1;
        }
    }
    CACHE_LEVELS_DETECTED.store(idx as u8, core::sync::atomic::Ordering::Release);
}

/// Get detected cache topology.
///
/// Returns a slice of [`CacheInfo`] for each detected cache level.
#[must_use]
pub fn cache_topology() -> &'static [CacheInfo] {
    let count = CACHE_LEVELS_DETECTED.load(core::sync::atomic::Ordering::Acquire) as usize;
    // SAFETY: count was written during boot; CACHE_TOPOLOGY was written
    // before CACHE_LEVELS_DETECTED was set (Release/Acquire ordering).
    // The slice is valid for 'static because CACHE_TOPOLOGY is a static.
    unsafe {
        let ptr = core::ptr::addr_of!(CACHE_TOPOLOGY) as *const CacheInfo;
        core::slice::from_raw_parts(ptr, count)
    }
}

/// Get the L1 data cache line size (in bytes).
///
/// Returns 64 if detection failed (safe default for x86_64).
#[must_use]
pub fn cache_line_size() -> u16 {
    CACHE_LINE_SIZE.load(core::sync::atomic::Ordering::Relaxed)
}

/// CPUID leaf 4 (Intel Deterministic Cache Parameters).
fn cpuid_leaf4(subleaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY: Caller verified max_leaf >= 4.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 4",
            "mov ecx, {sub:e}",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            sub = in(reg) subleaf,
            ebx_out = out(reg) ebx,
            out("eax") eax,
            out("ecx") ecx,
            out("edx") edx,
            options(nomem, nostack),
        );
    }
    (eax, ebx, ecx, edx)
}

/// CPUID leaf 0x8000001D (AMD extended cache topology).
fn cpuid_ext_1d(subleaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY: Caller verified max_ext_leaf >= 0x8000001D.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 0x8000001D",
            "mov ecx, {sub:e}",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            sub = in(reg) subleaf,
            ebx_out = out(reg) ebx,
            out("eax") eax,
            out("ecx") ecx,
            out("edx") edx,
            options(nomem, nostack),
        );
    }
    (eax, ebx, ecx, edx)
}

/// Log detected cache topology to serial.
pub fn log_cache_topology() {
    let caches = cache_topology();
    if caches.is_empty() {
        crate::serial_println!("[cpu] Cache topology: not detected");
        return;
    }
    crate::serial_println!("[cpu] Cache topology ({} levels):", caches.len());
    for c in caches {
        let size_str = if c.size >= 1024 * 1024 {
            alloc::format!("{} MiB", c.size / (1024 * 1024))
        } else {
            alloc::format!("{} KiB", c.size / 1024)
        };
        crate::serial_println!(
            "[cpu]   L{} {}: {} ({}-way, {}-byte line, {} sets{})",
            c.level,
            c.type_name(),
            size_str,
            c.ways,
            c.line_size,
            c.sets,
            if c.shared { ", shared" } else { "" },
        );
    }
}

extern crate alloc;

// ---------------------------------------------------------------------------
// Interrupt-disable duration tracking
// ---------------------------------------------------------------------------

/// Tracks how long interrupts are disabled across all CPUs.
///
/// Maintains per-CPU TSC timestamps for when interrupts were last
/// disabled, and global max/total statistics for diagnosing excessive
/// interrupt-off durations.
///
/// ## Usage
///
/// The tracking is automatic when using [`without_interrupts`].
/// For direct `cli()`/`sti()` usage, call
/// [`irqoff_tracker::record_disable()`] after `cli()` and
/// [`irqoff_tracker::record_enable()`] before `sti()`.
///
/// ## Overhead
///
/// One `rdtsc` per disable/enable pair (~10 cycles each) plus one
/// atomic max-update on enable.  Negligible for typical workloads.
pub mod irqoff_tracker {
    use core::sync::atomic::{AtomicU64, Ordering};

    /// Per-CPU TSC at last interrupt disable.
    /// Index = CPU logical index.  0 means "not currently disabled" or
    /// "tracking not active".
    static DISABLE_TSC: [AtomicU64; crate::smp::MAX_CPUS] = {
        const ZERO: AtomicU64 = AtomicU64::new(0);
        [ZERO; crate::smp::MAX_CPUS]
    };

    /// Maximum interrupt-off duration observed (TSC cycles).
    static MAX_OFF_CYCLES: AtomicU64 = AtomicU64::new(0);

    /// Total interrupt-off duration accumulated (TSC cycles).
    /// May wrap on very long-running systems — use for relative
    /// measurement within a session, not absolute accounting.
    static TOTAL_OFF_CYCLES: AtomicU64 = AtomicU64::new(0);

    /// Number of interrupt-off sections completed.
    static SECTION_COUNT: AtomicU64 = AtomicU64::new(0);

    /// Whether tracking is enabled.
    static ENABLED: AtomicU64 = AtomicU64::new(1);

    /// Record that interrupts were just disabled on this CPU.
    #[inline]
    pub fn record_disable() {
        if ENABLED.load(Ordering::Relaxed) == 0 {
            return;
        }
        let cpu = crate::smp::current_cpu_index();
        if let Some(slot) = DISABLE_TSC.get(cpu) {
            slot.store(crate::bench::rdtsc(), Ordering::Relaxed);
        }
    }

    /// Record that interrupts are about to be re-enabled on this CPU.
    ///
    /// Computes the duration since the last `record_disable()` call and
    /// updates the max/total statistics.
    #[inline]
    pub fn record_enable() {
        if ENABLED.load(Ordering::Relaxed) == 0 {
            return;
        }
        let cpu = crate::smp::current_cpu_index();
        let start = DISABLE_TSC.get(cpu)
            .map_or(0, |s| s.load(Ordering::Relaxed));
        if start == 0 {
            return; // No matching disable recorded.
        }
        let now = crate::bench::rdtsc();
        let duration = now.saturating_sub(start);

        SECTION_COUNT.fetch_add(1, Ordering::Relaxed);
        TOTAL_OFF_CYCLES.fetch_add(duration, Ordering::Relaxed);

        // Update max via CAS.
        let mut cur = MAX_OFF_CYCLES.load(Ordering::Relaxed);
        while duration > cur {
            match MAX_OFF_CYCLES.compare_exchange_weak(
                cur, duration, Ordering::Relaxed, Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }

        // Clear the slot.
        if let Some(slot) = DISABLE_TSC.get(cpu) {
            slot.store(0, Ordering::Relaxed);
        }
    }

    /// Interrupt-off duration statistics snapshot.
    #[derive(Debug, Clone, Copy)]
    pub struct IrqOffStats {
        /// Number of interrupt-off sections recorded.
        pub sections: u64,
        /// Total TSC cycles spent with interrupts off.
        pub total_cycles: u64,
        /// Maximum single interrupt-off duration (TSC cycles).
        pub max_cycles: u64,
        /// Mean interrupt-off duration (TSC cycles), or 0 if no sections.
        pub mean_cycles: u64,
    }

    /// Get current interrupt-off duration statistics.
    #[must_use]
    pub fn stats() -> IrqOffStats {
        let sections = SECTION_COUNT.load(Ordering::Relaxed);
        let total = TOTAL_OFF_CYCLES.load(Ordering::Relaxed);
        let max = MAX_OFF_CYCLES.load(Ordering::Relaxed);
        let mean = if sections > 0 { total / sections } else { 0 };
        IrqOffStats {
            sections,
            total_cycles: total,
            max_cycles: max,
            mean_cycles: mean,
        }
    }

    /// Reset all interrupt-off tracking counters.
    #[allow(dead_code)]
    pub fn reset() {
        SECTION_COUNT.store(0, Ordering::Relaxed);
        TOTAL_OFF_CYCLES.store(0, Ordering::Relaxed);
        MAX_OFF_CYCLES.store(0, Ordering::Relaxed);
    }

    /// Enable or disable interrupt-off tracking.
    #[allow(dead_code)]
    pub fn set_enabled(enabled: bool) {
        ENABLED.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
    }

    /// Check if tracking is enabled.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_enabled() -> bool {
        ENABLED.load(Ordering::Relaxed) != 0
    }
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
    /// MONITOR/MWAIT instructions (power-efficient idle).
    pub mwait: bool,
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
    /// CET Shadow Stacks (hardware return-address protection).
    pub cet_ss: bool,

    // --- CPUID leaf 7, subleaf 0, EDX ---
    /// CET Indirect Branch Tracking (ENDBR enforcement).
    pub cet_ibt: bool,

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

    // --- CPUID leaf 0x0A (Architectural Performance Monitoring) ---
    /// Performance monitoring version (0 = unsupported).
    pub pmu_version: u8,
    /// Number of general-purpose PMC registers per logical processor.
    pub pmu_counters: u8,
    /// Bit width of general-purpose PMC registers.
    pub pmu_counter_width: u8,
}

impl CpuFeatures {
    /// Create an empty features struct (all false/zero).
    const fn empty() -> Self {
        Self {
            sse3: false, ssse3: false, sse4_1: false, sse4_2: false,
            popcnt: false, avx: false, xsave: false, osxsave: false,
            mwait: false, aes_ni: false, rdrand: false, f16c: false,
            fxsr: false, sse: false, sse2: false, tsc: false, apic: false,
            avx2: false, bmi1: false, bmi2: false, avx512f: false,
            sha: false, rdseed: false, vaes: false, rdpid: false,
            cet_ss: false, cet_ibt: false,
            rdtscp: false, page_1g: false,
            xsave_area_size: 0, xcr0_supported: 0,
            pmu_version: 0, pmu_counters: 0, pmu_counter_width: 0,
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
    f.mwait = ecx1 & (1 << 3) != 0;
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
        let (ebx7, ecx7, edx7) = cpuid_leaf7_sub0();
        f.avx2 = ebx7 & (1 << 5) != 0;
        f.bmi1 = ebx7 & (1 << 3) != 0;
        f.bmi2 = ebx7 & (1 << 8) != 0;
        f.avx512f = ebx7 & (1 << 16) != 0;
        f.sha = ebx7 & (1 << 29) != 0;
        f.rdseed = ebx7 & (1 << 18) != 0;
        f.vaes = ecx7 & (1 << 9) != 0;
        f.rdpid = ecx7 & (1 << 22) != 0;
        // Intel CET (Control-flow Enforcement Technology).
        f.cet_ss = ecx7 & (1 << 7) != 0;   // Shadow Stack support
        f.cet_ibt = edx7 & (1 << 20) != 0; // Indirect Branch Tracking
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

    // --- Leaf 0x0A: Architectural Performance Monitoring ---
    if max_leaf >= 0x0A {
        let eax_a = cpuid_leaf_a_eax();
        // EAX[7:0]  = version ID
        // EAX[15:8] = number of GP PMC registers per CPU
        // EAX[23:16]= bit width of GP PMC registers
        f.pmu_version = (eax_a & 0xFF) as u8;
        f.pmu_counters = ((eax_a >> 8) & 0xFF) as u8;
        f.pmu_counter_width = ((eax_a >> 16) & 0xFF) as u8;
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
        "[cpu]   AES-NI={} SHA={} RDRAND={} RDSEED={} MWAIT={}",
        f.aes_ni, f.sha, f.rdrand, f.rdseed, f.mwait
    );
    crate::serial_println!(
        "[cpu]   RDTSCP={} 1GiB pages={} TSC={}",
        f.rdtscp, f.page_1g, f.tsc
    );
    crate::serial_println!(
        "[cpu]   CET: shadow_stack={} indirect_branch_tracking={}",
        f.cet_ss, f.cet_ibt
    );
    if f.pmu_version > 0 {
        crate::serial_println!(
            "[cpu]   PMU v{}: {} counters × {}-bit",
            f.pmu_version, f.pmu_counters, f.pmu_counter_width
        );
    }
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
fn cpuid_leaf7_sub0() -> (u32, u32, u32) {
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
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
            out("edx") edx,
            options(nomem, nostack),
        );
    }
    (ebx, ecx, edx)
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

/// CPUID leaf 0x0A: Architectural Performance Monitoring info.
///
/// Returns EAX which encodes version, counter count, and bit width.
fn cpuid_leaf_a_eax() -> u32 {
    let eax: u32;
    // SAFETY: Caller verified max_leaf >= 0x0A.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 0x0A",
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

// ---------------------------------------------------------------------------
// CPU identification strings (vendor + brand)
// ---------------------------------------------------------------------------

/// CPU vendor string (12 ASCII bytes from CPUID leaf 0).
///
/// Returns the vendor ID like "GenuineIntel" or "AuthenticAMD".
#[must_use]
pub fn vendor_string() -> [u8; 12] {
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY: CPUID leaf 0 is always valid on x86_64.
    // EBX:EDX:ECX contain the 12-byte vendor string.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "xor eax, eax",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            ebx_out = out(reg) ebx,
            out("eax") _,
            out("ecx") ecx,
            out("edx") edx,
            options(nomem, nostack),
        );
    }
    let mut result = [0u8; 12];
    result[0..4].copy_from_slice(&ebx.to_le_bytes());
    result[4..8].copy_from_slice(&edx.to_le_bytes());
    result[8..12].copy_from_slice(&ecx.to_le_bytes());
    result
}

/// CPU brand string (48 ASCII bytes from CPUID leaves 0x80000002–0x80000004).
///
/// Returns the full processor name string like "Intel(R) Core(TM) i7-...".
/// If extended leaves aren't available, returns all zeros.
#[must_use]
pub fn brand_string() -> [u8; 48] {
    let mut result = [0u8; 48];
    let max_ext = cpuid_max_extended_leaf();
    if max_ext < 0x8000_0004 {
        return result;
    }

    // Leaves 0x80000002, 0x80000003, 0x80000004 each return 16 bytes
    // in EAX:EBX:ECX:EDX.
    for (i, leaf) in [0x8000_0002u32, 0x8000_0003, 0x8000_0004].iter().enumerate() {
        let eax: u32;
        let ebx: u32;
        let ecx: u32;
        let edx: u32;
        // SAFETY: We verified max_ext >= 0x80000004.
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov eax, {leaf:e}",
                "cpuid",
                "mov {ebx_out:e}, ebx",
                "pop rbx",
                leaf = in(reg) *leaf,
                ebx_out = out(reg) ebx,
                out("eax") eax,
                out("ecx") ecx,
                out("edx") edx,
                options(nomem, nostack),
            );
        }
        let base = i * 16;
        result[base..base + 4].copy_from_slice(&eax.to_le_bytes());
        result[base + 4..base + 8].copy_from_slice(&ebx.to_le_bytes());
        result[base + 8..base + 12].copy_from_slice(&ecx.to_le_bytes());
        result[base + 12..base + 16].copy_from_slice(&edx.to_le_bytes());
    }
    result
}

/// CPU family, model, and stepping from CPUID leaf 1 EAX.
///
/// Returns (family, model, stepping) with extended model/family applied.
#[must_use]
pub fn cpu_family_model_stepping() -> (u32, u32, u32) {
    let eax: u32;
    // SAFETY: CPUID leaf 1 is always valid on x86_64.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            out("eax") eax,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    let stepping = eax & 0xF;
    let mut model = (eax >> 4) & 0xF;
    let mut family = (eax >> 8) & 0xF;
    let ext_model = (eax >> 16) & 0xF;
    let ext_family = (eax >> 20) & 0xFF;

    // Intel/AMD extended model/family encoding.
    if family == 0xF {
        family = family.wrapping_add(ext_family);
    }
    if family == 0x6 || family == 0xF {
        model = model.wrapping_add(ext_model << 4);
    }

    (family, model, stepping)
}
