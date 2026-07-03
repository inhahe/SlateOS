//! Process accounting — records resource usage when tasks exit.
//!
//! When a task exits (naturally or via kill), a compact summary of its
//! resource consumption is recorded in a ring buffer.  This provides a
//! historical view of what ran, for how long, and how many resources it
//! used.
//!
//! ## Recorded Data
//!
//! For each exited task:
//! - Task ID and name
//! - Total CPU cycles consumed (TSC-based)
//! - Total CPU ticks (100 Hz timer-based)
//! - Number of times scheduled (context switches into)
//! - Total wait time in run queue (ticks)
//! - Exit timestamp (monotonic nanoseconds)
//!
//! ## Ring Buffer
//!
//! A fixed-size (128-entry) circular buffer.  Oldest entries are
//! overwritten.  No heap allocation on the recording path.
//!
//! ## Usage
//!
//! The `pacct` kshell command displays recent task exits with their
//! resource usage.  Useful for post-mortem analysis: "what was running
//! before the problem happened?"
//!
//! ## Integration
//!
//! Registers an exit hook via `sched::register_exit_hook()`.  The hook
//! runs in the exiting task's context, before the task is marked Dead.
//!
//! ## References
//!
//! - Linux `kernel/acct.c` — BSD process accounting
//! - Linux `struct taskstats` — per-task statistics
//! - FreeBSD `kern/kern_acct.c` — process accounting

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of entries in the accounting ring buffer.
const RING_SIZE: usize = 128;

/// Mask for modular indexing.
const RING_MASK: usize = RING_SIZE - 1;

// ---------------------------------------------------------------------------
// Accounting record
// ---------------------------------------------------------------------------

/// A single process accounting record.
#[derive(Debug, Clone, Copy)]
pub struct AcctRecord {
    /// Task ID.
    pub task_id: u64,
    /// Task name (truncated to 16 bytes for compactness).
    pub name: [u8; 16],
    /// Name length.
    pub name_len: u8,
    /// Total CPU cycles consumed (TSC-based, nanosecond precision).
    pub total_cycles: u64,
    /// Total CPU ticks (10ms each at 100 Hz).
    pub total_ticks: u64,
    /// Number of times the task was scheduled.
    pub schedule_count: u64,
    /// Total time spent waiting in the run queue (ticks).
    pub total_wait_ticks: u64,
    /// Maximum single wait duration (ticks).
    pub max_wait_ticks: u64,
    /// Exit timestamp (monotonic nanoseconds since boot).
    pub exit_ns: u64,
    /// Last CPU the task ran on.
    pub last_cpu: u8,
    /// Priority level.
    pub priority: u8,
}

impl AcctRecord {
    const fn empty() -> Self {
        Self {
            task_id: 0,
            name: [0; 16],
            name_len: 0,
            total_cycles: 0,
            total_ticks: 0,
            schedule_count: 0,
            total_wait_ticks: 0,
            max_wait_ticks: 0,
            exit_ns: 0,
            last_cpu: 0,
            priority: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

/// Ring buffer slot wrapper (for Sync impl).
struct AcctSlot(core::cell::UnsafeCell<AcctRecord>);

// SAFETY: Access is serialized by the atomic write index.  Readers may
// see partially-written records, but all bit patterns are valid.
unsafe impl Sync for AcctSlot {}

/// Ring buffer of accounting records.
static RING: [AcctSlot; RING_SIZE] = [const {
    AcctSlot(core::cell::UnsafeCell::new(AcctRecord::empty()))
}; RING_SIZE];

/// Write index (monotonically increasing, wraps via mask).
static WRITE_IDX: AtomicU32 = AtomicU32::new(0);

/// Total records written since boot.
static TOTAL_RECORDS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize process accounting.
///
/// Registers the exit hook with the scheduler.  Call once during boot
/// after the scheduler is initialized.
pub fn init() {
    crate::sched::register_exit_hook(on_task_exit);
    crate::serial_println!("[pacct] Process accounting initialized (ring size {})", RING_SIZE);
}

// ---------------------------------------------------------------------------
// Exit hook
// ---------------------------------------------------------------------------

/// Called when a task is about to exit.
///
/// Looks up the task's resource usage and records it in the ring buffer.
fn on_task_exit(task_id: u64) {
    // Look up just this one task.  We deliberately avoid task_list() here:
    // that builds a heap Vec of *every* task and runs an O(stack-size)
    // volatile stack scan per task — all under the SCHED lock.  In the poison
    // debug build that can take seconds, starving the timer tick of the lock
    // and tripping the hard-lockup watchdog.  task_info() looks up exactly one
    // task and skips the stack scan (which we don't use anyway).
    let Some(info) = crate::sched::task_info(task_id) else {
        return;
    };

    // Build the accounting record.
    let mut record = AcctRecord::empty();
    record.task_id = task_id;

    // Copy name (truncated to 16 bytes).
    let copy_len = info.name_len.min(16);
    record.name[..copy_len].copy_from_slice(&info.name[..copy_len]);
    #[allow(clippy::cast_possible_truncation)]
    {
        record.name_len = copy_len as u8;
    }

    record.total_cycles = info.total_cycles;
    record.total_ticks = info.total_ticks;
    record.schedule_count = info.schedule_count;
    record.total_wait_ticks = info.total_wait_ticks;
    record.max_wait_ticks = info.max_wait_ticks;
    record.exit_ns = crate::timekeeping::clock_monotonic();
    #[allow(clippy::cast_possible_truncation)]
    {
        record.last_cpu = info.last_cpu as u8;
        record.priority = info.priority;
    }

    // Write to ring buffer.
    let idx = WRITE_IDX.fetch_add(1, Ordering::Relaxed) as usize & RING_MASK;
    // SAFETY: idx is bounded by RING_MASK.  No concurrent writer to the
    // same slot (WRITE_IDX is atomic and each slot is written exactly once
    // per RING_SIZE increments).
    unsafe {
        core::ptr::write(RING[idx].0.get(), record);
    }
    TOTAL_RECORDS.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Get the most recent N accounting records (newest first).
///
/// Returns up to `count` records.  If fewer have been recorded, returns
/// only what's available.
pub fn recent(count: usize) -> alloc::vec::Vec<AcctRecord> {
    let total = TOTAL_RECORDS.load(Ordering::Relaxed);
    if total == 0 {
        return alloc::vec::Vec::new();
    }

    let available = total.min(RING_SIZE as u64) as usize;
    let n = count.min(available);
    let write_pos = WRITE_IDX.load(Ordering::Relaxed) as usize;

    let mut result = alloc::vec::Vec::with_capacity(n);
    for i in 0..n {
        // Read backwards from the most recent entry.
        let idx = write_pos.wrapping_sub(1).wrapping_sub(i) & RING_MASK;
        // SAFETY: idx is bounded by RING_MASK.
        let record = unsafe { core::ptr::read(RING[idx].0.get()) };
        if record.task_id != 0 {
            result.push(record);
        }
    }
    result
}

/// Get the total number of task exits recorded.
#[must_use]
pub fn total_recorded() -> u64 {
    TOTAL_RECORDS.load(Ordering::Relaxed)
}

extern crate alloc;
