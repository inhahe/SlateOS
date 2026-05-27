//! Boot timing — records APIC tick timestamps at each boot milestone.
//!
//! Used for tracking boot performance: which phases are slow, where
//! regressions appear.  Each milestone is recorded with a single atomic
//! store (zero overhead on hot paths).
//!
//! ## Usage
//!
//! At each boot phase, call `mark(Milestone::XYZ)`.  After boot, the
//! `milestones()` function returns all recorded timestamps for display.

// Diagnostic/profiling subsystem — all public API for tooling and kshell
// commands; many helpers may not have call sites in production paths yet.
#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};

/// Boot milestones in chronological order.
///
/// Each variant corresponds to a major phase of the boot sequence.
/// Keep this in order — the display assumes ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Milestone {
    /// Kernel entry point reached (kmain called).
    KernelEntry = 0,
    /// Serial console initialized (first debug output possible).
    Serial = 1,
    /// GDT and IDT installed.
    GdtIdt = 2,
    /// Physical frame allocator initialized.
    FrameAlloc = 3,
    /// Kernel heap allocator ready.
    Heap = 4,
    /// Page table subsystem initialized.
    PageTable = 5,
    /// APIC and timer initialized (ticks start flowing).
    ApicTimer = 6,
    /// Scheduler initialized.
    Scheduler = 7,
    /// IPC subsystem initialized.
    Ipc = 8,
    /// Filesystem initialized.
    Filesystem = 9,
    /// SMP (APs booted).
    Smp = 10,
    /// Self-tests complete.
    SelfTests = 11,
    /// Benchmarks complete (if run).
    Benchmarks = 12,
    /// Shell ready (interactive).
    ShellReady = 13,
}

/// Number of milestones.
const NUM_MILESTONES: usize = 14;

/// Recorded tick counts for each milestone (0 = not yet reached).
static MILESTONES: [AtomicU64; NUM_MILESTONES] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; NUM_MILESTONES]
};

/// Record the current APIC tick at a boot milestone.
///
/// Safe to call multiple times for the same milestone (later calls
/// are ignored — first-wins semantics via compare_exchange).
pub fn mark(milestone: Milestone) {
    let tick = crate::apic::tick_count();
    let idx = milestone as usize;
    if let Some(slot) = MILESTONES.get(idx) {
        // Only write if not already set (first-wins).
        let _ = slot.compare_exchange(0, tick, Ordering::Relaxed, Ordering::Relaxed);
    }
}

/// Get all milestone timestamps.
///
/// Returns an array of (name, tick) pairs.  Tick=0 means the milestone
/// hasn't been reached.
#[must_use]
pub fn milestones() -> [(& 'static str, u64); NUM_MILESTONES] {
    const NAMES: [&str; NUM_MILESTONES] = [
        "Kernel entry",
        "Serial console",
        "GDT/IDT",
        "Frame allocator",
        "Heap allocator",
        "Page tables",
        "APIC timer",
        "Scheduler",
        "IPC subsystem",
        "Filesystem",
        "SMP (APs online)",
        "Self-tests",
        "Benchmarks",
        "Shell ready",
    ];

    let mut result = [("", 0u64); NUM_MILESTONES];
    for (i, name) in NAMES.iter().enumerate() {
        let tick = MILESTONES.get(i).map_or(0, |a| a.load(Ordering::Relaxed));
        result[i] = (name, tick);
    }
    result
}
