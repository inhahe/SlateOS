//! Unified per-process resource limits.
//!
//! Provides a single struct [`ResourceLimits`] that aggregates all
//! kernel-enforced resource caps for a process.  This is the kernel-core
//! analogue of POSIX `setrlimit`/`getrlimit`, but adapted to our OS:
//!
//! - **RSS (resident set size)**: maximum physical memory frames a
//!   process's address space may map.  Enforced by
//!   [`mm::accounting::try_charge`](super::accounting::try_charge).
//! - **CPU quota**: percentage of one CPU core per bandwidth period
//!   (1 second).  Enforced by the scheduler's per-task throttling
//!   ([`sched::set_cpu_quota`](crate::sched::set_cpu_quota)).
//! - **Max threads**: maximum number of threads the process may create.
//! - **Max open handles**: maximum number of capability handles the
//!   process may hold.
//!
//! ## Design
//!
//! Resource limits are *declarative*: set once at process launch (or
//! modified via a privileged syscall), then enforced transparently by
//! the relevant kernel subsystem.  The process never sees the limit
//! checks — it just gets `ENOMEM` or `ELIMIT` when exceeding a cap.
//!
//! This module provides:
//! 1. The [`ResourceLimits`] data structure.
//! 2. A process-indexed table mapping PID → limits.
//! 3. The [`apply_limits`] function to wire limits into the enforcement
//!    subsystems (`mm::accounting`, `sched`).
//! 4. Query APIs for the process explorer and `ps` command.
//!
//! ## Limit values
//!
//! All limits use 0 = unlimited.  This matches the convention in
//! `mm::accounting::rss_limit_frames` and `sched::cpu_quota_pct`.
//!
//! ## References
//!
//! - Linux `setrlimit(2)` / `prlimit(2)` — per-process resource limits
//! - Linux cgroups v2 — group-level resource controls
//! - design.txt: "set resource limits at process launch, kernel-enforced"

use crate::serial_println;
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Resource limits struct
// ---------------------------------------------------------------------------

/// Per-process resource limits.
///
/// All fields use 0 = unlimited (no cap).  Non-zero values are hard
/// limits enforced by the kernel.
///
/// Set via [`apply_limits`] at process launch or [`update_limit`]
/// for runtime modification (requires appropriate capability).
#[derive(Debug, Clone, Copy)]
pub struct ResourceLimits {
    /// Maximum RSS in 16 KiB frames (0 = unlimited).
    ///
    /// Enforced by `mm::accounting::try_charge()` before every frame
    /// mapping.  When exceeded, the page fault handler returns
    /// `ENOMEM` and the OOM killer may be invoked.
    pub max_rss_frames: u64,

    /// CPU bandwidth quota as percentage of one core (0 = unlimited,
    /// 1–100 = percentage per 1-second period).
    ///
    /// Enforced by the scheduler: when the task exceeds its quota in
    /// a period, it is throttled until the next period reset.
    ///
    /// Note: this is per-task, not per-process.  For multi-threaded
    /// processes, each thread gets this quota independently.  A future
    /// enhancement could support per-process aggregate CPU limits.
    pub cpu_quota_pct: u8,

    /// Maximum number of threads the process may create (0 = unlimited).
    ///
    /// Enforced at thread creation time (`SYS_THREAD_CREATE`).  The
    /// main thread counts toward this limit.
    pub max_threads: u32,

    /// Maximum number of capability handles (0 = unlimited).
    ///
    /// Enforced by the capability table when inserting new handles.
    /// Prevents handle leaks from exhausting kernel memory.
    pub max_handles: u32,
}

impl ResourceLimits {
    /// Default limits: everything unlimited.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_rss_frames: 0,
            cpu_quota_pct: 0,
            max_threads: 0,
            max_handles: 0,
        }
    }

    /// Check whether all limits are at their default (unlimited).
    #[must_use]
    pub const fn is_unlimited(&self) -> bool {
        self.max_rss_frames == 0
            && self.cpu_quota_pct == 0
            && self.max_threads == 0
            && self.max_handles == 0
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl core::fmt::Display for ResourceLimits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "rss=")?;
        if self.max_rss_frames == 0 {
            write!(f, "unlimited")?;
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            let kib = self.max_rss_frames.saturating_mul(
                super::frame::FRAME_SIZE as u64,
            ) / 1024;
            write!(f, "{} frames ({} KiB)", self.max_rss_frames, kib)?;
        }

        write!(f, ", cpu=")?;
        if self.cpu_quota_pct == 0 {
            write!(f, "unlimited")?;
        } else {
            write!(f, "{}%", self.cpu_quota_pct)?;
        }

        write!(f, ", threads=")?;
        if self.max_threads == 0 {
            write!(f, "unlimited")?;
        } else {
            write!(f, "{}", self.max_threads)?;
        }

        write!(f, ", handles=")?;
        if self.max_handles == 0 {
            write!(f, "unlimited")?;
        } else {
            write!(f, "{}", self.max_handles)?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Per-process limits table
// ---------------------------------------------------------------------------

/// Maximum number of processes we track limits for.
///
/// Matches `mm::accounting::MAX_ADDRESS_SPACES`.  In practice, 256
/// concurrent processes is generous for a desktop OS.
const MAX_PROCESSES: usize = 256;

/// Entry in the per-process limits table.
#[derive(Clone, Copy)]
struct LimitEntry {
    /// Process ID (0 = empty slot).
    pid: u64,
    /// The resource limits for this process.
    limits: ResourceLimits,
}

impl LimitEntry {
    const EMPTY: Self = Self {
        pid: 0,
        limits: ResourceLimits::unlimited(),
    };
}

/// Global table mapping PID → resource limits.
///
/// Uses a fixed-size array to avoid heap allocation.  Protected by a
/// spinlock (lock ordering: RLIMITS < SCHED < `frame_allocator`).
static RLIMITS: Mutex<[LimitEntry; MAX_PROCESSES]> = Mutex::named(
    [LimitEntry::EMPTY; MAX_PROCESSES], b"RLIMITS"
);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register resource limits for a process.
///
/// Called at process creation time (or when limits are first set).
/// If the process already has limits, they are replaced.
///
/// Also wires the limits into the enforcement subsystems:
/// - RSS limit → `mm::accounting::set_rss_limit`
/// - CPU quota → `sched::set_cpu_quota` (for the main task)
///
/// `main_task_id` is the task ID of the process's main thread, used
/// to set the CPU quota.  Pass 0 to skip CPU quota enforcement (e.g.,
/// when the main task hasn't been created yet — call
/// [`apply_cpu_quota`] separately later).
///
/// Returns `true` on success, `false` if the table is full.
#[allow(dead_code)] // Public API for proc/init zones (process launch).
pub fn apply_limits(
    pid: u64,
    pml4_phys: u64,
    main_task_id: u64,
    limits: &ResourceLimits,
) -> bool {
    if pid == 0 {
        return false; // PID 0 is reserved.
    }

    // Store in the table.
    {
        let mut table = RLIMITS.lock();
        let mut found = false;

        // Check if the PID already has an entry.
        for entry in table.iter_mut() {
            if entry.pid == pid {
                entry.limits = *limits;
                found = true;
                break;
            }
        }

        // If not found, allocate a new slot.
        if !found {
            let Some(slot) = table.iter_mut().find(|e| e.pid == 0) else {
                serial_println!(
                    "[rlimits] WARNING: table full ({} slots), cannot track PID {}",
                    MAX_PROCESSES, pid,
                );
                return false;
            };
            slot.pid = pid;
            slot.limits = *limits;
        }
    }

    // Wire into enforcement subsystems.

    // RSS limit → mm::accounting.
    if limits.max_rss_frames > 0 {
        super::accounting::set_rss_limit(pml4_phys, limits.max_rss_frames);
    } else {
        // Remove any existing limit.
        super::accounting::set_rss_limit(pml4_phys, 0);
    }

    // CPU quota → scheduler (if main task is known).
    if main_task_id != 0 {
        crate::sched::set_cpu_quota(main_task_id, limits.cpu_quota_pct);
    }

    serial_println!("[rlimits] PID {} limits: {}", pid, limits);
    true
}

/// Apply CPU quota to a specific task within a process.
///
/// Used when new threads are created in a process that has CPU limits.
/// Each thread in the process gets the same per-task quota.
///
/// Returns `true` if the task was found and quota was set.
#[allow(dead_code)] // Public API for proc zone (thread creation).
pub fn apply_cpu_quota(pid: u64, task_id: u64) -> bool {
    let quota = {
        let table = RLIMITS.lock();
        let Some(entry) = table.iter().find(|e| e.pid == pid) else {
            return false;
        };
        entry.limits.cpu_quota_pct
    };

    if quota > 0 {
        crate::sched::set_cpu_quota(task_id, quota);
    }
    true
}

/// Update a single resource limit for a running process.
///
/// `field` selects which limit to change.  The new value is applied
/// immediately (both in the table and in the enforcement subsystem).
///
/// Returns `true` if the PID was found and the limit was updated.
#[allow(dead_code)] // Public API for syscall handler (privileged limit change).
pub fn update_limit(
    pid: u64,
    pml4_phys: u64,
    main_task_id: u64,
    field: LimitField,
    value: u64,
) -> bool {
    let mut table = RLIMITS.lock();
    let Some(entry) = table.iter_mut().find(|e| e.pid == pid) else {
        return false;
    };

    match field {
        LimitField::MaxRssFrames => {
            entry.limits.max_rss_frames = value;
            super::accounting::set_rss_limit(pml4_phys, value);
        }
        LimitField::CpuQuotaPct => {
            let pct = value.min(100) as u8;
            entry.limits.cpu_quota_pct = pct;
            if main_task_id != 0 {
                crate::sched::set_cpu_quota(main_task_id, pct);
            }
        }
        LimitField::MaxThreads => {
            #[allow(clippy::cast_possible_truncation)] // Clamped to u32::MAX above.
            { entry.limits.max_threads = value.min(u64::from(u32::MAX)) as u32; }
        }
        LimitField::MaxHandles => {
            #[allow(clippy::cast_possible_truncation)] // Clamped to u32::MAX above.
            { entry.limits.max_handles = value.min(u64::from(u32::MAX)) as u32; }
        }
    }

    serial_println!(
        "[rlimits] PID {} updated {:?}={} ({})",
        pid, field, value, entry.limits,
    );
    true
}

/// Which resource limit field to update.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Public API — used by update_limit callers in proc/syscall zones.
pub enum LimitField {
    /// Maximum RSS in frames.
    MaxRssFrames,
    /// CPU quota percentage.
    CpuQuotaPct,
    /// Maximum threads.
    MaxThreads,
    /// Maximum capability handles.
    MaxHandles,
}

/// Query the resource limits for a process.
///
/// Returns `None` if the PID is not tracked.
#[must_use]
pub fn query(pid: u64) -> Option<ResourceLimits> {
    let table = RLIMITS.lock();
    table.iter()
        .find(|e| e.pid == pid)
        .map(|e| e.limits)
}

/// Remove resource limit tracking for a process.
///
/// Called when a process exits.  Does NOT clear the enforcement
/// (`mm::accounting::destroy_address_space` and sched task cleanup
/// handle that separately).
pub fn remove(pid: u64) {
    let mut table = RLIMITS.lock();
    if let Some(entry) = table.iter_mut().find(|e| e.pid == pid) {
        entry.pid = 0;
        entry.limits = ResourceLimits::unlimited();
    }
}

/// Get the thread limit for a process.
///
/// Returns 0 (unlimited) if the PID is not tracked or has no limit.
/// Called by thread creation to enforce `max_threads`.
#[must_use]
pub fn thread_limit(pid: u64) -> u32 {
    let table = RLIMITS.lock();
    table.iter()
        .find(|e| e.pid == pid)
        .map_or(0, |e| e.limits.max_threads)
}

/// Get the handle limit for a process.
///
/// Returns 0 (unlimited) if the PID is not tracked or has no limit.
/// Called by capability table insertion to enforce `max_handles`.
#[must_use]
pub fn handle_limit(pid: u64) -> u32 {
    let table = RLIMITS.lock();
    table.iter()
        .find(|e| e.pid == pid)
        .map_or(0, |e| e.limits.max_handles)
}

/// Count of processes currently tracked.
#[must_use]
#[allow(dead_code)] // Diagnostic API.
pub fn tracked_count() -> usize {
    let table = RLIMITS.lock();
    table.iter().filter(|e| e.pid != 0).count()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for resource limits.
#[allow(clippy::expect_used)] // Tests panic on unexpected state by design.
pub fn self_test() {
    serial_println!("[rlimits] Running self-test...");

    // --- 1. Default limits are unlimited ---
    let limits = ResourceLimits::unlimited();
    assert!(limits.is_unlimited(), "Default should be unlimited");
    serial_println!("[rlimits]   Default unlimited: OK");

    // --- 2. Display formatting ---
    let display = alloc::format!("{limits}");
    assert!(
        display.contains("unlimited"),
        "Display should show 'unlimited'",
    );
    serial_println!("[rlimits]   Display format: OK");

    // --- 3. Registration and query ---
    let test_pid: u64 = 9999;
    let test_limits = ResourceLimits {
        max_rss_frames: 100,
        cpu_quota_pct: 50,
        max_threads: 16,
        max_handles: 128,
    };

    // We can't call apply_limits here because it tries to wire into
    // mm::accounting (which requires a valid PML4).  Test the table
    // directly.
    {
        let mut table = RLIMITS.lock();
        let slot = table.iter_mut().find(|e| e.pid == 0).expect("Table not full");
        slot.pid = test_pid;
        slot.limits = test_limits;
    }

    let queried = query(test_pid);
    assert!(queried.is_some(), "Query should find test PID");
    let q = queried.expect("just checked");
    assert!(q.max_rss_frames == 100, "RSS should be 100");
    assert!(q.cpu_quota_pct == 50, "CPU quota should be 50");
    assert!(q.max_threads == 16, "Max threads should be 16");
    assert!(q.max_handles == 128, "Max handles should be 128");
    serial_println!("[rlimits]   Registration and query: OK");

    // --- 4. thread_limit and handle_limit helpers ---
    assert!(thread_limit(test_pid) == 16, "Thread limit should be 16");
    assert!(handle_limit(test_pid) == 128, "Handle limit should be 128");
    serial_println!("[rlimits]   Helper queries: OK");

    // --- 5. Update a single field ---
    {
        let mut table = RLIMITS.lock();
        let entry = table.iter_mut().find(|e| e.pid == test_pid).expect("found");
        entry.limits.max_threads = 32;
    }
    assert!(thread_limit(test_pid) == 32, "Updated thread limit should be 32");
    serial_println!("[rlimits]   Field update: OK");

    // --- 6. Remove ---
    remove(test_pid);
    assert!(query(test_pid).is_none(), "Query after remove should be None");
    assert!(thread_limit(test_pid) == 0, "Thread limit after remove should be 0");
    serial_println!("[rlimits]   Remove: OK");

    // --- 7. Nonexistent PID ---
    assert!(query(88888).is_none(), "Nonexistent PID should be None");
    assert!(thread_limit(88888) == 0, "Nonexistent thread limit should be 0");
    serial_println!("[rlimits]   Nonexistent PID: OK");

    // --- 8. is_unlimited check on non-default ---
    assert!(!test_limits.is_unlimited(), "Non-default should not be unlimited");
    serial_println!("[rlimits]   is_unlimited check: OK");

    serial_println!("[rlimits] Self-test PASSED");
}
