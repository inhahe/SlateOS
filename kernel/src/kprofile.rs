//! Kernel code profiler — TSC-based cycle counting for hot paths.
//!
//! Provides named measurement slots that track min/max/mean cycle counts
//! for instrumented code regions.  Designed for always-on low-overhead
//! profiling of kernel hot paths (context switch, page fault handling,
//! IPC dispatch, etc.).
//!
//! ## Usage
//!
//! ```ignore
//! // At the start of the region:
//! let t = crate::kprofile::begin(Slot::ContextSwitch);
//!
//! // ... do work ...
//!
//! // At the end:
//! crate::kprofile::end(Slot::ContextSwitch, t);
//! ```
//!
//! ## Design
//!
//! - Fixed 16-slot table (no heap allocation, no locks).
//! - Each slot uses atomic min/max/sum/count (lock-free, ~30 cycles overhead).
//! - Slots are identified by enum variants for type safety.
//! - The `begin()` function just reads TSC (~20 cycles).
//! - The `end()` function computes the delta and updates atomics (~10 cycles).
//!
//! Total overhead per measurement: ~30 cycles (negligible for any path
//! taking hundreds of cycles or more).

use core::sync::atomic::{AtomicU64, Ordering};

/// Named profiling slots.
///
/// Each variant identifies a specific kernel code region being measured.
/// Add new variants as needed — keep the total ≤ 16 (or bump `NUM_SLOTS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Slot {
    /// Full context switch (save old + restore new + CR3 switch).
    ContextSwitch = 0,
    /// Page fault resolution (entry to page mapped).
    PageFault = 1,
    /// Heap allocation (GlobalAlloc::alloc, both paths).
    HeapAlloc = 2,
    /// Heap deallocation.
    HeapDealloc = 3,
    /// Frame allocator (single frame alloc from buddy or per-CPU cache).
    FrameAlloc = 4,
    /// Frame free (return to buddy or per-CPU cache).
    FrameFree = 5,
    /// Scheduler pick_next_task (finding the next task to run).
    SchedPickNext = 6,
    /// IPC channel send (kernel-side message enqueue).
    IpcSend = 7,
    /// Syscall dispatch (entry → handler selection).
    SyscallDispatch = 8,
    /// TLB shootdown (IPI send + wait for acknowledgment).
    TlbShootdown = 9,
    /// Timer ISR full duration (entry → exit, including softirq).
    TimerIsr = 10,
    /// Work queue item execution.
    WorkQueueItem = 11,
    /// Futex wake (finding and waking waiters).
    FutexWake = 12,
    /// kswapd page reclaim (single reclaim cycle).
    Reclaim = 13,
    /// Reserved for ad-hoc measurement.
    Adhoc0 = 14,
    /// Reserved for ad-hoc measurement.
    Adhoc1 = 15,
}

/// Number of profiling slots.
const NUM_SLOTS: usize = 16;

/// Per-slot measurement data (all atomic for lock-free access from any CPU).
struct SlotData {
    /// Minimum cycle count observed.
    min: AtomicU64,
    /// Maximum cycle count observed.
    max: AtomicU64,
    /// Sum of all cycle counts (for mean computation).
    total: AtomicU64,
    /// Number of measurements recorded.
    count: AtomicU64,
}

/// Global profiling table.
static SLOTS: [SlotData; NUM_SLOTS] = {
    const INIT: SlotData = SlotData {
        min: AtomicU64::new(u64::MAX),
        max: AtomicU64::new(0),
        total: AtomicU64::new(0),
        count: AtomicU64::new(0),
    };
    [INIT; NUM_SLOTS]
};

/// Whether profiling is globally enabled.
///
/// When disabled, `begin()` still reads TSC (cheap) but `end()` skips
/// the atomic updates.  This allows instrumentation to remain in the
/// code permanently with zero cost when profiling is off.
static ENABLED: AtomicU64 = AtomicU64::new(1); // Enabled by default.

/// Begin a profiled region.  Returns the current TSC value.
///
/// This is extremely cheap (~20 cycles for rdtsc).  Always call this
/// unconditionally — the enable/disable check is in `end()`.
#[inline(always)]
#[must_use]
pub fn begin(_slot: Slot) -> u64 {
    crate::bench::rdtsc()
}

/// End a profiled region.  Records the cycle delta into the slot.
///
/// `start_tsc` must be the value returned by the corresponding `begin()`.
/// If profiling is disabled, this is a no-op (single atomic load + branch).
#[inline]
pub fn end(slot: Slot, start_tsc: u64) {
    if ENABLED.load(Ordering::Relaxed) == 0 {
        return;
    }

    let end_tsc = crate::bench::rdtsc();
    let delta = end_tsc.saturating_sub(start_tsc);
    let idx = slot as usize;

    if let Some(data) = SLOTS.get(idx) {
        data.count.fetch_add(1, Ordering::Relaxed);
        data.total.fetch_add(delta, Ordering::Relaxed);

        // Update min (CAS loop — typically 1 iteration).
        let _ = data.min.fetch_update(
            Ordering::Relaxed, Ordering::Relaxed,
            |cur| if delta < cur { Some(delta) } else { None },
        );
        // Update max.
        let _ = data.max.fetch_update(
            Ordering::Relaxed, Ordering::Relaxed,
            |cur| if delta > cur { Some(delta) } else { None },
        );
    }
}

/// Enable or disable profiling globally.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
}

/// Check if profiling is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) != 0
}

/// Reset all profiling counters.
pub fn reset() {
    for data in &SLOTS {
        data.min.store(u64::MAX, Ordering::Relaxed);
        data.max.store(0, Ordering::Relaxed);
        data.total.store(0, Ordering::Relaxed);
        data.count.store(0, Ordering::Relaxed);
    }
}

/// Snapshot of a single slot's measurements.
#[derive(Debug, Clone, Copy)]
pub struct SlotSnapshot {
    /// Slot identifier.
    pub slot: Slot,
    /// Human-readable name.
    pub name: &'static str,
    /// Minimum cycles observed.
    pub min_cycles: u64,
    /// Maximum cycles observed.
    pub max_cycles: u64,
    /// Mean cycles (total / count).
    pub mean_cycles: u64,
    /// Total measurements.
    pub count: u64,
}

/// Slot names for display.
const SLOT_NAMES: [&str; NUM_SLOTS] = [
    "ctx_switch",
    "page_fault",
    "heap_alloc",
    "heap_dealloc",
    "frame_alloc",
    "frame_free",
    "sched_pick",
    "ipc_send",
    "syscall_disp",
    "tlb_shootdown",
    "timer_isr",
    "workqueue",
    "futex_wake",
    "reclaim",
    "adhoc_0",
    "adhoc_1",
];

/// All slot variants in order (for iteration).
const ALL_SLOTS: [Slot; NUM_SLOTS] = [
    Slot::ContextSwitch,
    Slot::PageFault,
    Slot::HeapAlloc,
    Slot::HeapDealloc,
    Slot::FrameAlloc,
    Slot::FrameFree,
    Slot::SchedPickNext,
    Slot::IpcSend,
    Slot::SyscallDispatch,
    Slot::TlbShootdown,
    Slot::TimerIsr,
    Slot::WorkQueueItem,
    Slot::FutexWake,
    Slot::Reclaim,
    Slot::Adhoc0,
    Slot::Adhoc1,
];

/// Read all slot snapshots (only slots with count > 0).
#[must_use]
pub fn snapshots() -> [Option<SlotSnapshot>; NUM_SLOTS] {
    let mut result = [None; NUM_SLOTS];

    for (i, data) in SLOTS.iter().enumerate() {
        let count = data.count.load(Ordering::Relaxed);
        if count == 0 {
            continue;
        }
        let total = data.total.load(Ordering::Relaxed);
        let mean = total.checked_div(count).unwrap_or(0);

        result[i] = Some(SlotSnapshot {
            slot: ALL_SLOTS[i],
            name: SLOT_NAMES[i],
            min_cycles: data.min.load(Ordering::Relaxed),
            max_cycles: data.max.load(Ordering::Relaxed),
            mean_cycles: mean,
            count,
        });
    }

    result
}
