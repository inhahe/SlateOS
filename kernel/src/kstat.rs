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
///
/// Values are absolute (since boot, or as-of-now) and the viewer computes
/// deltas — **except [`cpu_util`](Self::cpu_util)**, which is already an
/// interval figure because the absolute form of it is a since-boot average
/// that cannot move.  That exception is the reason the field carries a
/// paragraph of its own.
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
    /// Number of tasks the scheduler could run at this instant — queued on
    /// some CPU's run queue, plus the one on each CPU that is not that CPU's
    /// idle task.  From [`crate::sched::nr_runnable`].
    ///
    /// **This field used to hold `live_tasks`' number under this name**, which
    /// meant a column labelled "runnable" only ever went up.  See that field.
    pub runnable_tasks: u16,
    /// Number of tasks that exist: spawned minus exited since boot.
    ///
    /// Not a count of *blocked* tasks, which is what this slot used to be
    /// called while being hardcoded to zero.  Blocked cannot be derived from
    /// the two by subtraction either, because the per-CPU idle tasks are live
    /// and never runnable but are registered directly into the scheduler's
    /// table without passing the spawn counter (`sched::register_ap_idle`), so
    /// the two populations do not nest.  Counting blocked tasks honestly needs
    /// a walk of the scheduler's task table, which takes `SCHED` — forbidden
    /// here; see [`sample`].
    pub live_tasks: u16,
    /// Memory pressure score (0-100).
    pub pressure_score: u8,
    /// Per-CPU utilization percentage over the interval *since the previous
    /// sample* (up to 4 CPUs, 0-100 each).
    ///
    /// An interval figure, deliberately, even though every other number in
    /// this struct is absolute: the scheduler publishes `(total, idle)` tick
    /// counts that are cumulative since boot, and `(total - idle) / total` on
    /// those is a since-boot *average*.  An average over an ever-longer window
    /// converges and stops moving — so a graph of it is flat by construction
    /// and cannot show the busy second this buffer exists to catch, while
    /// still looking like measured data.  The deltas are taken against
    /// [`PREV_CPU_TICKS`].
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
            live_tasks: 0,
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

/// The previous sample's cumulative per-CPU `(total_ticks, idle_ticks)`.
///
/// Exists so [`Sample::cpu_util`] can be an *interval* utilisation rather than
/// a since-boot average; see that field for why the difference is the whole
/// point of this buffer.  Indexed by CPU, four wide to match `cpu_util`.
///
/// Written only by the BSP softirq, the same single-writer discipline as
/// [`HISTORY`], so a plain `swap` per entry is enough — there is no second
/// writer for the read and the write to race against.  Relaxed ordering is
/// correct because each slot is read and written by that one thread of
/// control and nothing else is published through it.
///
/// **The first sample after boot diffs against zero**, so its utilisation is
/// the since-boot average — which is the right answer for it, there being no
/// earlier sample to measure an interval against.  Every sample after it is a
/// true interval.
static PREV_CPU_TICKS: [(AtomicU64, AtomicU64); 4] =
    [const { (AtomicU64::new(0), AtomicU64::new(0)) }; 4];

/// Samples abandoned because a memory lock was busy at tick time.
///
/// Counted rather than silently dropped: skipping is the correct response to
/// contention (see [`sample`]), but a sampler that quietly stopped recording
/// because some lock is *permanently* contended would look identical to one
/// with nothing to report.  A rising count here says which it is.
static SKIPPED_SAMPLES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record a system metrics sample.
///
/// Called from the timer softirq on the BSP every SAMPLE_INTERVAL_TICKS.
/// Must be fast — no allocations, no lock acquisitions (uses only
/// lock-free atomics and `try_*` queries).
///
/// **That last clause is a hard requirement, not a performance note.** This
/// runs in the timer softirq, on whatever CPU the tick landed on, interrupting
/// whatever that CPU was doing.  A *blocking* acquire of a lock the
/// interrupted code already holds can never succeed: the only thing that could
/// release it is the code this interrupt just suspended.  `sync.rs` catches
/// the recursive acquire and panics rather than hanging.  Every query below is
/// therefore either a lock-free atomic read or a `try_*` that reports busy.
///
/// When the memory snapshot is unavailable the whole sample is abandoned
/// rather than partially filled: a record with zeros where the real numbers
/// should be is indistinguishable to a reader from a genuinely empty system,
/// and one missing record is not.  The next tick gets it.
#[allow(clippy::cast_possible_truncation)]
pub fn sample() {
    if ENABLED.load(Ordering::Relaxed) == 0 {
        return;
    }

    let tick = crate::apic::tick_count();

    // --- Memory stats and pressure, from ONE non-blocking snapshot ---
    //
    // One call, not two: the frame counts recorded here and the pressure score
    // beside them now describe the same instant.  Previously the counts came
    // from `frame::try_stats()` and the score from a *blocking*
    // `memory_pressure()` that re-read everything — which is the call that
    // deadlocked (lane B, requests/b-a-kstat-sample-calls-memory-info-*).
    let Some(info) = crate::mm::try_memory_info() else {
        SKIPPED_SAMPLES.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let free_frames = info.free_frames as u32;
    let total_frames = info.total_frames as u32;

    // Lock-free: the heap's byte counters are plain relaxed atomics.
    let heap_bytes = crate::mm::heap::stats().bytes_in_use as u32;

    // --- Scheduler stats ---
    let sched = crate::sched::sched_stats();
    // `nr_runnable` documents itself ISR-safe: per-CPU `try_lock` reads and
    // atomic loads, no allocation, a contended queue contributing 0 rather
    // than blocking.  That is the only reason this can be asked here at all,
    // and it is why the answer is taken from there rather than recomputed.
    let runnable = u16::try_from(crate::sched::nr_runnable()).unwrap_or(u16::MAX);
    let live = u16::try_from(
        sched
            .total_tasks_spawned
            .saturating_sub(sched.total_tasks_exited),
    )
    .unwrap_or(u16::MAX);

    // --- Memory pressure --- scored from the snapshot already taken above, so
    // this adds no lock acquisition at all.
    let pressure_score = crate::mm::pressure_from_info(&info).score;

    // --- Per-CPU utilization, over the interval since the previous sample ---
    //
    // `cpu_ticks` is cumulative since boot, so the ratio has to be taken on
    // the *deltas*; see `Sample::cpu_util` for why the since-boot ratio this
    // replaced could not show a busy second.  The swap publishes this
    // sample's raw counts for the next one to diff against, so a CPU that
    // stops being sampled (num_cpus shrank) simply stops updating rather than
    // going stale in a way that inflates a later delta.
    let mut cpu_util = [0u8; 4];
    for i in 0..sched.num_cpus.min(4) {
        let (total, idle) = sched.cpu_ticks.get(i).copied().unwrap_or((0, 0));
        let Some(prev) = PREV_CPU_TICKS.get(i) else {
            continue;
        };
        let d_total = total.saturating_sub(prev.0.swap(total, Ordering::Relaxed));
        let d_idle = idle.saturating_sub(prev.1.swap(idle, Ordering::Relaxed));
        if d_total > 0 {
            let util = d_total.saturating_sub(d_idle).saturating_mul(100) / d_total;
            if let Some(slot) = cpu_util.get_mut(i) {
                *slot = u8::try_from(util.min(100)).unwrap_or(100);
            }
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
        live_tasks: live,
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

/// Number of samples abandoned because a memory lock was busy at tick time.
///
/// Expected to be small and to stop rising: it counts ticks that landed inside
/// somebody else's short critical section.  A count that climbs steadily means
/// a memory lock is held for a large fraction of wall time, which is a problem
/// worth chasing in its own right — this counter is how it becomes visible
/// instead of silently thinning the metrics history.
#[must_use]
pub fn skipped_samples() -> u64 {
    SKIPPED_SAMPLES.load(Ordering::Relaxed)
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
