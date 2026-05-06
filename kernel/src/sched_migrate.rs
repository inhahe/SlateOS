//! Scheduler migration tracker — records task movement between CPUs.
//!
//! Tracks every time a task changes CPUs (via work-stealing or load
//! balancing).  Provides aggregate statistics and a recent event log
//! for diagnosing excessive migration (cache thrashing), imbalanced
//! load distribution, and affinity violations.
//!
//! ## Design
//!
//! - **Lock-free ring buffer**: 64-entry event log using an atomic write
//!   pointer.  Recording a migration is O(1) with no locking.
//! - **Per-CPU counters**: Each CPU tracks migrations-in and migrations-out
//!   independently, allowing O(1) queries without cross-CPU contention.
//! - **Aggregate stats**: Total migrations, per-pair hotspots (which
//!   CPU→CPU path is most trafficked).
//!
//! ## Integration
//!
//! The scheduler calls [`record(task_id, from_cpu, to_cpu)`] whenever a
//! task is moved to a different CPU's run queue (work stealing or push
//! balancing).  The kshell `migrate` command displays the data.
//!
//! ## Why This Matters
//!
//! Excessive migration destroys cache locality.  A task that bounces
//! between CPUs loses its warm L1/L2 cache lines on every move (~5-10μs
//! penalty for cache refill).  This tracker helps identify:
//! - Tasks migrating too often (need pinning or stronger affinity)
//! - CPUs that are always stealing (unbalanced load generation)
//! - Scheduling patterns that defeat cache-warm heuristics
//!
//! ## References
//!
//! - Linux `sched:sched_migrate_task` tracepoint
//! - Linux `/proc/schedstat` (per-CPU migration counts)
//! - perf sched: `perf sched map` visualizes task migration

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum CPUs tracked.
const MAX_CPUS: usize = 16;

/// Size of the migration event ring buffer.
const RING_SIZE: usize = 64;

/// Mask for modular indexing.
const RING_MASK: usize = RING_SIZE - 1;

// ---------------------------------------------------------------------------
// Migration event
// ---------------------------------------------------------------------------

/// A single migration event.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct MigrateEvent {
    /// Task ID that was migrated.
    pub task_id: u32,
    /// Source CPU.
    pub from_cpu: u8,
    /// Destination CPU.
    pub to_cpu: u8,
    /// Reason for migration.
    pub reason: MigrateReason,
    /// Reserved padding.
    _pad: u8,
    /// Timestamp (APIC tick count at time of migration).
    pub tick: u64,
}

/// Reason a task was migrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MigrateReason {
    /// Work stealing: idle CPU took task from busy CPU.
    WorkSteal = 0,
    /// Push balancing: overloaded CPU pushed task to lighter CPU.
    PushBalance = 1,
    /// Affinity change: task's affinity mask no longer includes current CPU.
    AffinityChange = 2,
    /// Explicit placement: task was explicitly placed on a specific CPU.
    Explicit = 3,
    /// Wake-up: task woke on a different CPU than where it last ran.
    WakeUp = 4,
}

impl MigrateReason {
    /// Short name for display.
    pub const fn name(self) -> &'static str {
        match self {
            Self::WorkSteal => "steal",
            Self::PushBalance => "push",
            Self::AffinityChange => "affinity",
            Self::Explicit => "explicit",
            Self::WakeUp => "wakeup",
        }
    }
}

impl MigrateEvent {
    /// Empty event (unused slot).
    pub const fn empty() -> Self {
        Self {
            task_id: 0,
            from_cpu: 0,
            to_cpu: 0,
            reason: MigrateReason::WorkSteal,
            _pad: 0,
            tick: 0,
        }
    }

    /// Whether this event has been written.
    pub fn is_valid(&self) -> bool {
        self.tick != 0
    }
}

/// Wrapper to allow static array of `UnsafeCell<MigrateEvent>`.
#[repr(transparent)]
struct MigrateSlot(core::cell::UnsafeCell<MigrateEvent>);

// SAFETY: The ring buffer uses atomic write_pos and single-writer-per-slot
// semantics.  Each slot is written exactly once per wrap cycle.  Readers
// may see partially-written data (torn reads) but this is acceptable for
// diagnostic purposes — we check `is_valid()` before using.
unsafe impl Sync for MigrateSlot {}

// ---------------------------------------------------------------------------
// Per-CPU migration counters
// ---------------------------------------------------------------------------

/// Per-CPU migration statistics.
#[repr(C)]
struct PerCpuMigrate {
    /// Tasks migrated TO this CPU (incoming).
    migrations_in: AtomicU64,
    /// Tasks migrated FROM this CPU (outgoing).
    migrations_out: AtomicU64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Ring buffer of recent migration events.
static RING: [MigrateSlot; RING_SIZE] = {
    const EMPTY: MigrateSlot =
        MigrateSlot(core::cell::UnsafeCell::new(MigrateEvent::empty()));
    [EMPTY; RING_SIZE]
};

/// Write pointer into the ring buffer (monotonically increasing).
static WRITE_POS: AtomicU32 = AtomicU32::new(0);

/// Per-CPU migration counters.
static PER_CPU: [PerCpuMigrate; MAX_CPUS] = {
    const INIT: PerCpuMigrate = PerCpuMigrate {
        migrations_in: AtomicU64::new(0),
        migrations_out: AtomicU64::new(0),
    };
    [INIT; MAX_CPUS]
};

/// Total migration count (all CPUs, all reasons).
static TOTAL_MIGRATIONS: AtomicU64 = AtomicU64::new(0);

/// Per-reason counters.
static REASON_COUNTS: [AtomicU64; 5] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; 5]
};


// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record a task migration event.
///
/// Called by the scheduler whenever a task is moved to a different CPU.
/// This is O(1) and lock-free.
#[inline]
pub fn record(task_id: u32, from_cpu: u8, to_cpu: u8, reason: MigrateReason) {
    let tick = crate::apic::tick_count();
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed) as usize & RING_MASK;

    let event = MigrateEvent {
        task_id,
        from_cpu,
        to_cpu,
        reason,
        _pad: 0,
        tick,
    };

    // SAFETY: pos is masked to valid range, single writer per position
    // (atomic increment ensures unique positions).
    unsafe {
        core::ptr::write_volatile(RING[pos].0.get(), event);
    }

    // Update per-CPU counters.
    if (from_cpu as usize) < MAX_CPUS {
        PER_CPU[from_cpu as usize].migrations_out.fetch_add(1, Ordering::Relaxed);
    }
    if (to_cpu as usize) < MAX_CPUS {
        PER_CPU[to_cpu as usize].migrations_in.fetch_add(1, Ordering::Relaxed);
    }

    // Update totals.
    TOTAL_MIGRATIONS.fetch_add(1, Ordering::Relaxed);
    let reason_idx = reason as usize;
    if reason_idx < REASON_COUNTS.len() {
        REASON_COUNTS[reason_idx].fetch_add(1, Ordering::Relaxed);
    }
}

/// Aggregate migration statistics.
#[derive(Debug, Clone)]
pub struct MigrateStats {
    /// Total migrations across all CPUs and reasons.
    pub total: u64,
    /// Per-reason breakdown.
    pub by_reason: [u64; 5],
    /// Per-CPU migrations in.
    pub per_cpu_in: [u64; MAX_CPUS],
    /// Per-CPU migrations out.
    pub per_cpu_out: [u64; MAX_CPUS],
}

/// Get current migration statistics.
pub fn stats() -> MigrateStats {
    let total = TOTAL_MIGRATIONS.load(Ordering::Relaxed);
    let mut by_reason = [0u64; 5];
    for (i, c) in REASON_COUNTS.iter().enumerate() {
        by_reason[i] = c.load(Ordering::Relaxed);
    }
    let mut per_cpu_in = [0u64; MAX_CPUS];
    let mut per_cpu_out = [0u64; MAX_CPUS];
    for i in 0..MAX_CPUS {
        per_cpu_in[i] = PER_CPU[i].migrations_in.load(Ordering::Relaxed);
        per_cpu_out[i] = PER_CPU[i].migrations_out.load(Ordering::Relaxed);
    }
    MigrateStats {
        total,
        by_reason,
        per_cpu_in,
        per_cpu_out,
    }
}

/// Get recent migration events (most recent first).
///
/// Fills `buf` with up to `buf.len()` recent events.
/// Returns the number of valid events written.
pub fn recent(buf: &mut [MigrateEvent]) -> usize {
    let wp = WRITE_POS.load(Ordering::Relaxed) as usize;
    let mut count = 0;

    for i in 0..buf.len().min(RING_SIZE) {
        let idx = wp.wrapping_sub(i + 1) & RING_MASK;
        // SAFETY: idx is masked to valid range.
        let event = unsafe { core::ptr::read_volatile(RING[idx].0.get()) };
        if event.is_valid() {
            buf[count] = event;
            count += 1;
        } else {
            break;
        }
    }

    count
}

/// Find the hottest migration path (from_cpu → to_cpu with most events).
///
/// Returns `Some((from_cpu, to_cpu, count))` or `None` if no migrations.
pub fn hottest_path() -> Option<(u8, u8, u32)> {
    // Scan the ring buffer to find the most common (from, to) pair.
    // This is O(RING_SIZE²) worst case but the ring is small (64 entries).
    let wp = WRITE_POS.load(Ordering::Relaxed) as usize;
    let n = wp.min(RING_SIZE);
    if n == 0 {
        return None;
    }

    // Count occurrences of each (from, to) pair.
    // Use a simple flat array: MAX_CPUS * MAX_CPUS entries.
    let mut counts = [0u32; MAX_CPUS * MAX_CPUS];

    for i in 0..n {
        let idx = wp.wrapping_sub(i + 1) & RING_MASK;
        // SAFETY: idx is masked to valid range.
        let event = unsafe { core::ptr::read_volatile(RING[idx].0.get()) };
        if !event.is_valid() {
            break;
        }
        let from = event.from_cpu as usize;
        let to = event.to_cpu as usize;
        if from < MAX_CPUS && to < MAX_CPUS {
            counts[from * MAX_CPUS + to] += 1;
        }
    }

    // Find the maximum.
    let mut best_idx = 0;
    let mut best_count = 0u32;
    for (i, &c) in counts.iter().enumerate() {
        if c > best_count {
            best_count = c;
            best_idx = i;
        }
    }

    if best_count == 0 {
        return None;
    }

    let from_cpu = (best_idx / MAX_CPUS) as u8;
    let to_cpu = (best_idx % MAX_CPUS) as u8;
    Some((from_cpu, to_cpu, best_count))
}

/// Reset all migration statistics and clear the ring buffer.
pub fn reset() {
    WRITE_POS.store(0, Ordering::Relaxed);
    TOTAL_MIGRATIONS.store(0, Ordering::Relaxed);
    for c in &REASON_COUNTS {
        c.store(0, Ordering::Relaxed);
    }
    for cpu in &PER_CPU {
        cpu.migrations_in.store(0, Ordering::Relaxed);
        cpu.migrations_out.store(0, Ordering::Relaxed);
    }
    // Clear ring buffer entries.
    for slot in &RING {
        // SAFETY: We're the only writer (reset is called from kshell context).
        unsafe {
            core::ptr::write_volatile(slot.0.get(), MigrateEvent::empty());
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the migration tracker.
pub fn self_test() {
    serial_println!("[sched_migrate] Running self-test...");

    // Save current state.
    let initial_total = TOTAL_MIGRATIONS.load(Ordering::Relaxed);

    // Test 1: Record a migration event.
    record(42, 0, 1, MigrateReason::WorkSteal);
    let s = stats();
    assert!(s.total > initial_total,
        "total should increase after recording");
    serial_println!("[sched_migrate]   Record event: OK (total={})", s.total);

    // Test 2: Verify per-CPU counters updated.
    assert!(s.per_cpu_out[0] > 0, "CPU 0 out should be > 0");
    assert!(s.per_cpu_in[1] > 0, "CPU 1 in should be > 0");
    serial_println!("[sched_migrate]   Per-CPU counters: OK");

    // Test 3: Verify per-reason counters.
    assert!(s.by_reason[MigrateReason::WorkSteal as usize] > 0,
        "WorkSteal count should be > 0");
    serial_println!("[sched_migrate]   Per-reason counters: OK");

    // Test 4: Record more events with different reasons.
    record(43, 1, 2, MigrateReason::PushBalance);
    record(44, 0, 3, MigrateReason::WakeUp);
    let s2 = stats();
    assert_eq!(s2.total, s.total + 2);
    serial_println!("[sched_migrate]   Multiple events: OK (total={})", s2.total);

    // Test 5: Recent events.
    let mut buf = [MigrateEvent::empty(); 8];
    let n = recent(&mut buf);
    assert!(n >= 3, "should have at least 3 recent events");
    // Most recent should be the last recorded (task 44).
    assert_eq!(buf[0].task_id, 44);
    assert_eq!(buf[0].from_cpu, 0);
    assert_eq!(buf[0].to_cpu, 3);
    serial_println!("[sched_migrate]   Recent events: OK ({} events)", n);

    // Test 6: Hottest path.
    // Record several more on the same path to make it dominant.
    for _ in 0..5 {
        record(99, 2, 3, MigrateReason::WorkSteal);
    }
    if let Some((from, to, count)) = hottest_path() {
        assert_eq!(from, 2);
        assert_eq!(to, 3);
        assert!(count >= 5);
        serial_println!("[sched_migrate]   Hottest path: CPU{}→CPU{} ({}x): OK",
            from, to, count);
    } else {
        panic!("hottest_path() should return Some");
    }

    serial_println!("[sched_migrate] Self-test PASSED");
}
