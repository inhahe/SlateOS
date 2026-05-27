//! Hardware Performance Monitoring Counters (PMC).
//!
//! Provides access to the CPU's architectural performance monitoring
//! unit (PMU) for precise measurement of hardware events: cache misses,
//! branch mispredictions, instructions retired, etc.
//!
//! ## Architecture
//!
//! Intel/AMD x86_64 processors expose performance counters via MSRs:
//! - `IA32_PERFEVTSELx` (0x186 + x): Event select, umask, enable flags
//! - `IA32_PMCx` (0x0C1 + x): 48-bit counter value
//! - `IA32_PERF_GLOBAL_CTRL` (0x38F): Global enable bitmask
//!
//! This module supports **Architectural Performance Monitoring v1+**
//! (CPUID leaf 0x0A), which defines 7 pre-defined events that work
//! across all Intel CPUs and most AMD CPUs with PMU v2+.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::pmc;
//!
//! // Measure L3 cache misses for a code section:
//! pmc::configure(0, pmc::Event::LlcMisses);
//! pmc::start(0);
//! // ... code to measure ...
//! pmc::stop(0);
//! let misses = pmc::read(0);
//! ```
//!
//! ## Safety
//!
//! PMC access requires ring 0.  Counters are shared per-logical-CPU —
//! if you configure counter 0, it affects all code running on that CPU
//! until reconfigured.  Disable interrupts around measurements for
//! accurate results (ISR code will contribute to the count otherwise).
//!
//! ## References
//!
//! - Intel SDM Vol. 3, Chapter 19: Performance Monitoring
//! - Intel SDM Vol. 3, §19.2: Architectural Performance Monitoring
//! - AMD APM Vol. 2, §13: Performance Optimization

// PMC is a diagnostic/profiling subsystem; many event constants and
// helper methods are exposed for ad-hoc measurement but not currently
// invoked from production paths.
#![allow(dead_code)]

use crate::cpu;
use crate::serial_println;

// ---------------------------------------------------------------------------
// MSR addresses for Performance Monitoring
// ---------------------------------------------------------------------------

/// Base MSR for event selection registers (IA32_PERFEVTSELx).
const MSR_PERFEVTSEL_BASE: u32 = 0x186;

/// Base MSR for general-purpose PMC registers (IA32_PMCx).
const MSR_PMC_BASE: u32 = 0x0C1;

/// Global performance counter control (enable bits for all counters).
const MSR_PERF_GLOBAL_CTRL: u32 = 0x38F;

// ---------------------------------------------------------------------------
// Event Select register bits
// ---------------------------------------------------------------------------

/// Enable counting (bit 22 of IA32_PERFEVTSELx).
const PERFEVTSEL_EN: u64 = 1 << 22;

/// Count in OS mode / ring 0 (bit 17).
const PERFEVTSEL_OS: u64 = 1 << 17;

/// Count in user mode / ring 3 (bit 16).
const PERFEVTSEL_USR: u64 = 1 << 16;

// ---------------------------------------------------------------------------
// Pre-defined architectural events
// ---------------------------------------------------------------------------

/// Hardware performance events (Architectural Performance Monitoring v1).
///
/// These events are guaranteed to work across all processors that
/// support architectural PMU (CPUID.0AH:EAX[7:0] >= 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Event {
    /// CPU cycles while the core is not halted.
    /// Event 0x3C, UMask 0x00.
    UnhaltedCoreCycles = 0x003C,

    /// Instructions retired (completed execution).
    /// Event 0xC0, UMask 0x00.
    InstructionsRetired = 0x00C0,

    /// Reference cycles (unhalted, at fixed frequency).
    /// Event 0x3C, UMask 0x01.
    UnhaltedReferenceCycles = 0x013C,

    /// Last-level cache (L3) references.
    /// Event 0x2E, UMask 0x4F.
    LlcReferences = 0x4F2E,

    /// Last-level cache (L3) misses.
    /// Event 0x2E, UMask 0x41.
    LlcMisses = 0x412E,

    /// Branch instructions retired.
    /// Event 0xC4, UMask 0x00.
    BranchInstructions = 0x00C4,

    /// Branch misses retired (mispredictions).
    /// Event 0xC5, UMask 0x00.
    BranchMisses = 0x00C5,
}

impl Event {
    /// Extract the event select byte (bits 7:0 of the u16 encoding).
    const fn event_select(self) -> u8 {
        (self as u16 & 0xFF) as u8
    }

    /// Extract the unit mask byte (bits 15:8 of the u16 encoding).
    const fn umask(self) -> u8 {
        ((self as u16 >> 8) & 0xFF) as u8
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check whether performance monitoring is available.
///
/// Returns `true` if the CPU has architectural PMU v1+ with at least
/// one general-purpose counter.
#[must_use]
pub fn is_available() -> bool {
    cpu::features().map_or(false, |f| f.pmu_version >= 1 && f.pmu_counters > 0)
}

/// Number of general-purpose PMC registers available.
#[must_use]
pub fn num_counters() -> u8 {
    cpu::features().map_or(0, |f| f.pmu_counters)
}

/// Configure a counter to track a specific event.
///
/// Programs `IA32_PERFEVTSELx` with the event/umask but does NOT
/// start counting (enable bit is clear).  Call [`start`] to begin.
///
/// # Arguments
///
/// - `counter`: Counter index (0..num_counters()).
/// - `event`: The hardware event to track.
///
/// Returns `false` if PMU is unavailable or counter index is out of range.
pub fn configure(counter: u8, event: Event) -> bool {
    if !is_available() {
        return false;
    }
    let max = num_counters();
    if counter >= max {
        return false;
    }

    // Build the event select value:
    // bits 7:0  = event select
    // bits 15:8 = unit mask
    // bit 16    = USR (count in ring 3)
    // bit 17    = OS (count in ring 0)
    // bit 22    = EN (enable) — leave clear for now
    let evtsel = u64::from(event.event_select())
        | (u64::from(event.umask()) << 8)
        | PERFEVTSEL_OS
        | PERFEVTSEL_USR;

    let msr = MSR_PERFEVTSEL_BASE.wrapping_add(u32::from(counter));
    // SAFETY: We verified the PMU is available and the counter index is
    // valid.  Writing PERFEVTSELx configures what to count.
    unsafe { cpu::wrmsr(msr, evtsel); }

    // Zero the counter so measurements start fresh.
    let pmc_msr = MSR_PMC_BASE.wrapping_add(u32::from(counter));
    // SAFETY: Same as above — counter MSR is valid.
    unsafe { cpu::wrmsr(pmc_msr, 0); }

    true
}

/// Start counting on a previously configured counter.
///
/// Sets the EN bit (bit 22) in `IA32_PERFEVTSELx` and enables the
/// counter in `IA32_PERF_GLOBAL_CTRL`.
pub fn start(counter: u8) -> bool {
    if !is_available() || counter >= num_counters() {
        return false;
    }

    let evtsel_msr = MSR_PERFEVTSEL_BASE.wrapping_add(u32::from(counter));
    // SAFETY: PMU is available, counter index is valid.
    unsafe {
        let val = cpu::rdmsr(evtsel_msr);
        cpu::wrmsr(evtsel_msr, val | PERFEVTSEL_EN);
    }

    // Also set the corresponding bit in PERF_GLOBAL_CTRL.
    // SAFETY: PMU v2+ supports global control; v1 may not have this MSR,
    // but writing it on v1 is a no-op on most CPUs (the PERFEVTSELx EN
    // bit alone controls counting on v1).
    let features = cpu::features().unwrap();
    if features.pmu_version >= 2 {
        unsafe {
            let ctrl = cpu::rdmsr(MSR_PERF_GLOBAL_CTRL);
            cpu::wrmsr(MSR_PERF_GLOBAL_CTRL, ctrl | (1u64 << counter));
        }
    }

    true
}

/// Stop counting on a counter.
///
/// Clears the EN bit in `IA32_PERFEVTSELx`.  The counter value is
/// preserved and can be read with [`read`].
pub fn stop(counter: u8) -> bool {
    if !is_available() || counter >= num_counters() {
        return false;
    }

    let evtsel_msr = MSR_PERFEVTSEL_BASE.wrapping_add(u32::from(counter));
    // SAFETY: PMU is available, counter index is valid.
    unsafe {
        let val = cpu::rdmsr(evtsel_msr);
        cpu::wrmsr(evtsel_msr, val & !PERFEVTSEL_EN);
    }

    true
}

/// Read the current value of a PMC register.
///
/// Returns the 48-bit counter value (sign-extended to 64 bits on some
/// CPUs — we mask to the actual counter width).
///
/// Returns 0 if PMU is unavailable or counter is out of range.
#[must_use]
pub fn read(counter: u8) -> u64 {
    if !is_available() || counter >= num_counters() {
        return 0;
    }

    let pmc_msr = MSR_PMC_BASE.wrapping_add(u32::from(counter));
    // SAFETY: PMU is available, counter index is valid.
    let raw = unsafe { cpu::rdmsr(pmc_msr) };

    // Mask to the actual counter width to avoid sign-extension artifacts.
    let width = cpu::features().unwrap().pmu_counter_width;
    if width >= 64 {
        raw
    } else {
        raw & ((1u64 << width) - 1)
    }
}

/// Reset a counter to zero without changing its configuration.
pub fn reset(counter: u8) -> bool {
    if !is_available() || counter >= num_counters() {
        return false;
    }

    let pmc_msr = MSR_PMC_BASE.wrapping_add(u32::from(counter));
    // SAFETY: PMU is available, counter index is valid.
    unsafe { cpu::wrmsr(pmc_msr, 0); }

    true
}

// ---------------------------------------------------------------------------
// Convenience: measure a closure
// ---------------------------------------------------------------------------

/// Measure hardware events for a closure.
///
/// Configures counter 0 with the given event, runs the closure, and
/// returns the event count.  Existing counter 0 configuration is
/// overwritten.
///
/// Returns `None` if PMU is unavailable.
///
/// # Example
///
/// ```ignore
/// if let Some(misses) = pmc::measure(pmc::Event::LlcMisses, || {
///     // code to measure...
/// }) {
///     serial_println!("LLC misses: {}", misses);
/// }
/// ```
pub fn measure<F: FnOnce()>(event: Event, f: F) -> Option<u64> {
    if !configure(0, event) {
        return None;
    }
    start(0);
    f();
    stop(0);
    Some(read(0))
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the PMC subsystem.
///
/// If PMU is available, verifies that:
/// 1. Configure/start/stop/read work without faulting.
/// 2. Instructions Retired counter is non-zero after executing code.
/// 3. Counter reset zeros the value.
///
/// If PMU is unavailable, logs the skip and passes.
pub fn self_test() {
    serial_println!("[pmc] Running self-test...");

    if !is_available() {
        serial_println!("[pmc]   PMU not available (skipping hardware tests)");
        serial_println!("[pmc] Self-test PASSED (no PMU)");
        return;
    }

    let features = cpu::features().unwrap();
    serial_println!(
        "[pmc]   PMU v{}: {} counters × {}-bit",
        features.pmu_version, features.pmu_counters, features.pmu_counter_width,
    );

    // Test 1: Configure and read Instructions Retired.
    assert!(configure(0, Event::InstructionsRetired), "configure failed");
    assert!(start(0), "start failed");

    // Execute some instructions to ensure the counter advances.
    let mut dummy: u64 = 0;
    for i in 0..1000u64 {
        dummy = dummy.wrapping_add(i);
    }
    // Prevent the loop from being optimized away.
    core::hint::black_box(dummy);

    assert!(stop(0), "stop failed");
    let count = read(0);
    serial_println!("[pmc]   Instructions retired (1000-iter loop): {}", count);
    // Should be at least a few hundred instructions.
    assert!(count > 0, "Instructions Retired counter is zero — PMU not counting");

    // Test 2: Reset zeroes the counter.
    assert!(reset(0), "reset failed");
    let count_after_reset = read(0);
    assert_eq!(count_after_reset, 0, "counter not zero after reset");
    serial_println!("[pmc]   Counter reset: OK");

    // Test 3: Invalid counter index returns false.
    assert!(!configure(255, Event::LlcMisses), "should reject invalid counter");
    serial_println!("[pmc]   Bounds checking: OK");

    serial_println!("[pmc] Self-test PASSED");
}
