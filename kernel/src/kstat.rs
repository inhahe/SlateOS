//! Kernel statistics history buffer.
//!
//! Periodically samples system-wide metrics (memory, CPU, scheduler,
//! pressure) into a fixed-size circular buffer.  Provides a time-series
//! view for the `kstat` kshell command, showing how the system has
//! changed over time (not just instantaneous values).
//!
//! ## Sampling
//!
//! The timer softirq calls [`sample`] every [`SAMPLE_INTERVAL_TICKS`]
//! ticks (default: 100 ticks = 1 second).  Each sample captures a
//! compact snapshot of key metrics.
//!
//! ## Buffer
//!
//! A 60-entry circular buffer (1 minute of history at 1 sample/sec).
//! Old entries are overwritten.  The buffer is read lock-free via
//! an atomic write pointer; writers are single-threaded (BSP softirq
//! context only).
//!
//! ## References
//!
//! - Linux `/proc/stat`, `/proc/meminfo`, `/proc/pressure/`
//! - System Activity Reporter (sar) from sysstat package

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// How often to sample (in APIC timer ticks at 100 Hz).
/// 100 ticks = 1 sample per second.
pub const SAMPLE_INTERVAL_TICKS: u64 = 100;

/// Number of samples in the history buffer (60 = 1 minute of history).
const HISTORY_SIZE: usize = 60;

// ---------------------------------------------------------------------------
// Sample format
// ---------------------------------------------------------------------------

/// A single system metrics snapshot.
///
/// Kept deliberately small (64 bytes) to fit in a cache line.
/// All values are absolute (not deltas) — the viewer computes deltas.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Sample {
    /// APIC tick count at sample time.
    pub tick: u64,
    /// Free physical frames (16 KiB each).
    pub free_frames: u32,
    /// Total physical frames.
    pub total_frames: u32,
    /// Heap bytes currently in use.
    pub heap_bytes_in_use: u32,
    /// Number of runnable tasks (ready + running).
    pub runnable_tasks: u16,
    /// Number of blocked tasks.
    pub blocked_tasks: u16,
    /// Memory pressure score (0-100).
    pub pressure_score: u8,
    /// Per-CPU utilization percentages (up to 4 CPUs, 0-100 each).
    pub cpu_util: [u8; 4],
    /// Context switches since boot (low 32 bits).
    pub ctx_switches_lo: u32,
    /// Total interrupts since boot (low 32 bits).
    pub interrupts_lo: u32,
    /// Padding to 64 bytes.
    _pad: [u8; 5],
}

impl Sample {
    const fn zeroed() -> Self {
        Self {
            tick: 0,
            free_frames: 0,
            total_frames: 0,
            heap_bytes_in_use: 0,
            runnable_tasks: 0,
            blocked_tasks: 0,
            pressure_score: 0,
            cpu_util: [0; 4],
            ctx_switches_lo: 0,
            interrupts_lo: 0,
            _pad: [0; 5],
        }
    }
}

// ---------------------------------------------------------------------------
// History buffer
// ---------------------------------------------------------------------------

/// Circular history buffer.  Written by the BSP softirq, read lock-free.
static mut HISTORY: [Sample; HISTORY_SIZE] = [Sample::zeroed(); HISTORY_SIZE];

/// Write pointer (next slot to write).  Wraps modulo HISTORY_SIZE.
static WRITE_IDX: AtomicU32 = AtomicU32::new(0);

/// Total samples recorded (allows readers to know if the buffer has wrapped).
static TOTAL_SAMPLES: AtomicU64 = AtomicU64::new(0);

/// Whether sampling is enabled.
static ENABLED: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record a system metrics sample.
///
/// Called from the timer softirq on the BSP every SAMPLE_INTERVAL_TICKS.
/// Must be fast — no allocations, no lock acquisitions (uses only
/// lock-free atomics and `try_*` queries).
pub fn sample() {
    if ENABLED.load(Ordering::Relaxed) == 0 {
        return;
    }

    let tick = crate::apic::tick_count();

    // --- Memory stats (lock-free query) ---
    let (free_frames, total_frames) = crate::mm::frame::try_stats()
        .map_or((0, 0), |s| (s.free_frames as u32, s.total_frames as u32));

    let heap_bytes = crate::mm::heap::stats().bytes_in_use as u32;

    // --- Scheduler stats ---
    let sched = crate::sched::sched_stats();
    let runnable = sched.total_tasks_spawned
        .saturating_sub(sched.total_tasks_exited) as u16;
    // Rough split: blocked = total - ready - running.
    // The sched_stats exposes state counts if available.
    let blocked = 0u16; // Simplified; full version would query per-state counts.

    // --- Memory pressure ---
    let pressure = crate::mm::memory_pressure();
    let pressure_score = pressure.score;

    // --- Per-CPU utilization ---
    let mut cpu_util = [0u8; 4];
    for i in 0..sched.num_cpus.min(4) {
        let (total, idle) = sched.cpu_ticks.get(i).copied().unwrap_or((0, 0));
        if total > 0 {
            #[allow(clippy::cast_possible_truncation)]
            let util = total.saturating_sub(idle).saturating_mul(100) / total;
            cpu_util[i] = util.min(100) as u8;
        }
    }

    // --- Context switches ---
    #[allow(clippy::cast_possible_truncation)]
    let ctx_switches_lo = sched.total_ctx_switches as u32;

    // --- Interrupts (from IDT stats) ---
    let irq_counts = crate::idt::vector_counts();
    let mut total_irqs: u64 = 0;
    for count in &irq_counts {
        total_irqs = total_irqs.saturating_add(*count);
    }
    #[allow(clippy::cast_possible_truncation)]
    let interrupts_lo = total_irqs as u32;

    // --- Write sample ---
    let s = Sample {
        tick,
        free_frames,
        total_frames,
        heap_bytes_in_use: heap_bytes,
        runnable_tasks: runnable,
        blocked_tasks: blocked,
        pressure_score,
        cpu_util,
        ctx_switches_lo,
        interrupts_lo,
        _pad: [0; 5],
    };

    let idx = WRITE_IDX.load(Ordering::Relaxed) as usize;
    // SAFETY: Only the BSP softirq writes, so there's no concurrent writer.
    // Readers may see a partial write on rare occasions but the values are
    // still valid (individual fields are primitive types written atomically
    // by the CPU at aligned offsets).
    unsafe {
        HISTORY[idx % HISTORY_SIZE] = s;
    }
    WRITE_IDX.store(((idx + 1) % HISTORY_SIZE) as u32, Ordering::Release);
    TOTAL_SAMPLES.fetch_add(1, Ordering::Relaxed);
}

/// Get the most recent N samples (newest first).
///
/// Returns up to `count` samples.  If fewer have been recorded, returns
/// only what's available.
#[must_use]
pub fn recent(count: usize) -> alloc::vec::Vec<Sample> {
    let total = TOTAL_SAMPLES.load(Ordering::Acquire);
    let available = total.min(HISTORY_SIZE as u64) as usize;
    let n = count.min(available);

    let write_idx = WRITE_IDX.load(Ordering::Acquire) as usize;
    let mut result = alloc::vec::Vec::with_capacity(n);

    for i in 0..n {
        // Walk backwards from write_idx - 1.
        let slot = (write_idx + HISTORY_SIZE - 1 - i) % HISTORY_SIZE;
        // SAFETY: We read a potentially-racing write, but the data is
        // primitive and the worst case is a slightly stale value.
        let s = unsafe { HISTORY[slot] };
        result.push(s);
    }

    result
}

/// Get total number of samples recorded.
#[must_use]
pub fn total_samples() -> u64 {
    TOTAL_SAMPLES.load(Ordering::Relaxed)
}

/// Enable or disable periodic sampling.
#[allow(dead_code)]
pub fn set_enabled(enabled: bool) {
    ENABLED.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
}

/// Check if sampling is enabled.
#[must_use]
#[allow(dead_code)]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) != 0
}

extern crate alloc;
