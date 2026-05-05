//! Task supervisor — automatic restart of critical kernel tasks.
//!
//! Monitors registered tasks and automatically restarts them if they
//! exit unexpectedly.  This is the kernel-side infrastructure for
//! driver crash recovery and reliable background services.
//!
//! ## Design
//!
//! Tasks are registered with a `RestartPolicy` that controls:
//! - Whether to restart at all
//! - Maximum restart count (prevent infinite restart loops)
//! - Backoff delay between restarts (exponential, capped)
//!
//! The supervisor hooks into the scheduler's exit-hook mechanism to
//! detect task death.  When a supervised task exits, the supervisor
//! schedules a restart via the kernel timer system (ktimer) after the
//! appropriate backoff delay.
//!
//! ## Restart Policies
//!
//! | Policy | Behavior |
//! |--------|----------|
//! | `Always` | Restart unconditionally, up to max_restarts |
//! | `OnFailure` | Restart only if exit was abnormal (future: exit code check) |
//! | `Never` | Never restart (monitoring only) |
//!
//! ## Backoff
//!
//! Restarts use exponential backoff: 1st restart = `base_delay`, 2nd =
//! 2×base, 3rd = 4×base, etc., capped at `max_delay`.  This prevents
//! a crashing task from consuming all CPU in a restart loop.
//!
//! ## Usage
//!
//! ```ignore
//! let policy = RestartPolicy::always(10, 10, 500);  // max 10, delay 100ms-5s
//! supervisor::register(task_id, policy);
//! ```
//!
//! ## References
//!
//! - Linux `kernel/exit.c` → `do_exit()` (doesn't restart, but signals parent)
//! - systemd's restart policies (Restart=always, on-failure, etc.)
//! - Erlang/OTP supervisor model (one-for-one, rest-for-one strategies)

use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use crate::error::KernelResult;
use crate::serial_println;
use crate::sched::task::TaskId;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of supervised tasks.
const MAX_SUPERVISED: usize = 32;

// ---------------------------------------------------------------------------
// Restart policy
// ---------------------------------------------------------------------------

/// How to handle task exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartMode {
    /// Always restart (regardless of exit reason).
    Always,
    /// Restart only on unexpected exit (future: check exit code).
    OnFailure,
    /// Never restart — just log the exit.
    Never,
}

/// Configuration for task restart behavior.
#[derive(Debug, Clone, Copy)]
pub struct RestartPolicy {
    /// When to restart.
    pub mode: RestartMode,
    /// Maximum number of restarts before giving up (0 = unlimited).
    pub max_restarts: u32,
    /// Base delay between restarts (in ticks, ~10ms each).
    /// First restart waits this long; subsequent restarts double it.
    pub base_delay_ticks: u64,
    /// Maximum delay between restarts (exponential backoff cap).
    pub max_delay_ticks: u64,
}

impl RestartPolicy {
    /// Create an "always restart" policy.
    ///
    /// - `max_restarts`: Stop after this many restarts (0 = infinite).
    /// - `base_delay_ticks`: Initial restart delay (~10ms per tick).
    /// - `max_delay_ticks`: Maximum backoff delay.
    #[must_use]
    pub const fn always(max_restarts: u32, base_delay_ticks: u64, max_delay_ticks: u64) -> Self {
        Self {
            mode: RestartMode::Always,
            max_restarts,
            base_delay_ticks,
            max_delay_ticks,
        }
    }

    /// Create a "restart on failure" policy.
    #[must_use]
    pub const fn on_failure(max_restarts: u32, base_delay_ticks: u64, max_delay_ticks: u64) -> Self {
        Self {
            mode: RestartMode::OnFailure,
            max_restarts,
            base_delay_ticks,
            max_delay_ticks,
        }
    }

    /// Create a "never restart" policy (monitoring only).
    #[must_use]
    pub const fn never() -> Self {
        Self {
            mode: RestartMode::Never,
            max_restarts: 0,
            base_delay_ticks: 0,
            max_delay_ticks: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Supervised task entry
// ---------------------------------------------------------------------------

/// Record of a supervised task.
#[derive(Clone, Copy)]
struct SupervisedTask {
    /// Current task ID (changes on restart).
    task_id: TaskId,
    /// Restart policy.
    policy: RestartPolicy,
    /// Number of times this task has been restarted.
    restart_count: u32,
    /// Task name (for re-spawning).
    name: [u8; 32],
    /// Name length.
    name_len: usize,
    /// Task priority.
    priority: u8,
    /// Entry point function.
    entry: extern "C" fn(u64),
    /// Argument to the entry function.
    arg: u64,
    /// Page table (PML4 physical address).
    pml4_phys: u64,
    /// Whether this slot is occupied.
    active: bool,
}

impl SupervisedTask {
    const fn empty() -> Self {
        Self {
            task_id: 0,
            policy: RestartPolicy::never(),
            restart_count: 0,
            name: [0u8; 32],
            name_len: 0,
            priority: 0,
            entry: dummy_entry,
            arg: 0,
            pml4_phys: 0,
            active: false,
        }
    }
}

/// Placeholder entry point (never actually called).
extern "C" fn dummy_entry(_: u64) {}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Table of supervised tasks.
static SUPERVISED: Mutex<[SupervisedTask; MAX_SUPERVISED]> =
    Mutex::new([SupervisedTask::empty(); MAX_SUPERVISED]);

/// Whether the supervisor's exit hook is registered.
static HOOK_REGISTERED: AtomicU64 = AtomicU64::new(0);

/// Total restarts performed since boot.
static TOTAL_RESTARTS: AtomicU64 = AtomicU64::new(0);

/// Total supervised exits detected since boot.
static TOTAL_EXITS: AtomicU64 = AtomicU64::new(0);

/// Total restart failures (max restarts exceeded, spawn failed).
static RESTART_FAILURES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the supervisor (register exit hook).
///
/// Called once during boot.  Idempotent — safe to call multiple times.
pub fn init() {
    if HOOK_REGISTERED.load(Ordering::Relaxed) != 0 {
        return; // Already initialized.
    }

    if let Some(slot) = super::register_exit_hook(on_task_exit) {
        HOOK_REGISTERED.store(slot as u64 + 1, Ordering::Release);
        serial_println!("[supervisor] Initialized (exit hook slot {})", slot);
    } else {
        serial_println!("[supervisor] WARNING: could not register exit hook");
    }
}

/// Register a task for supervision.
///
/// The task must already be spawned.  When it exits, the supervisor
/// will restart it according to the given policy.
///
/// # Parameters
///
/// - `task_id`: The running task's ID.
/// - `name`: Task name (for respawning).
/// - `priority`: Task priority.
/// - `entry`: Entry point function.
/// - `arg`: Argument to entry.
/// - `pml4_phys`: Page table address.
/// - `policy`: Restart policy.
///
/// # Returns
///
/// `Ok(())` on success, `Err` if the supervision table is full.
pub fn register(
    task_id: TaskId,
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    pml4_phys: u64,
    policy: RestartPolicy,
) -> KernelResult<()> {
    // Ensure the exit hook is installed.
    init();

    let mut table = SUPERVISED.lock();
    if let Some(slot) = table.iter_mut().find(|s| !s.active) {
        slot.task_id = task_id;
        slot.policy = policy;
        slot.restart_count = 0;
        slot.priority = priority;
        slot.entry = entry;
        slot.arg = arg;
        slot.pml4_phys = pml4_phys;
        slot.active = true;

        // Copy name.
        let copy_len = name.len().min(slot.name.len());
        slot.name[..copy_len].copy_from_slice(&name[..copy_len]);
        slot.name_len = copy_len;

        serial_println!(
            "[supervisor] Registered task {} ({:?}) with policy {:?}",
            task_id,
            core::str::from_utf8(&name[..copy_len]).unwrap_or("<?>"),
            slot.policy.mode,
        );
        Ok(())
    } else {
        Err(crate::error::KernelError::OutOfMemory)
    }
}

/// Unregister a task from supervision.
///
/// After this call, the task will no longer be restarted on exit.
/// Returns `true` if the task was found and unregistered.
#[allow(dead_code)]
pub fn unregister(task_id: TaskId) -> bool {
    let mut table = SUPERVISED.lock();
    if let Some(slot) = table.iter_mut().find(|s| s.active && s.task_id == task_id) {
        slot.active = false;
        true
    } else {
        false
    }
}

/// Get information about supervised tasks.
#[must_use]
pub fn active_count() -> usize {
    let table = SUPERVISED.lock();
    table.iter().filter(|s| s.active).count()
}

/// Total restarts since boot.
#[must_use]
pub fn total_restarts() -> u64 {
    TOTAL_RESTARTS.load(Ordering::Relaxed)
}

/// Total supervised exits since boot.
#[must_use]
pub fn total_exits() -> u64 {
    TOTAL_EXITS.load(Ordering::Relaxed)
}

/// Total restart failures since boot.
#[must_use]
pub fn total_failures() -> u64 {
    RESTART_FAILURES.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Exit hook callback
// ---------------------------------------------------------------------------

/// Called by the scheduler when any task exits.
///
/// Checks if the dead task is supervised and schedules a restart if
/// the policy allows it.
fn on_task_exit(task_id: TaskId) {
    TOTAL_EXITS.fetch_add(1, Ordering::Relaxed);

    let mut table = SUPERVISED.lock();

    // Find the slot index first (immutable scan).
    let slot_idx = match table.iter().position(|s| s.active && s.task_id == task_id) {
        Some(idx) => idx,
        None => return, // Not supervised.
    };

    // Now borrow mutably by index.
    let entry = &mut table[slot_idx];
    let policy = entry.policy;

    // Check if we should restart.
    let should_restart = match policy.mode {
        RestartMode::Always => true,
        RestartMode::OnFailure => true, // TODO: check exit code when available.
        RestartMode::Never => false,
    };

    if !should_restart {
        crate::klog!(Info, "sched.supervisor",
            "Supervised task {} exited (policy=Never, no restart)",
            task_id
        );
        entry.active = false;
        return;
    }

    // Check restart limit.
    if policy.max_restarts > 0 && entry.restart_count >= policy.max_restarts {
        crate::klog!(Warn, "sched.supervisor",
            "Task {} exceeded max restarts ({}), giving up",
            task_id, policy.max_restarts
        );
        entry.active = false;
        RESTART_FAILURES.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Calculate backoff delay.
    let delay = calculate_backoff(
        entry.restart_count,
        policy.base_delay_ticks,
        policy.max_delay_ticks,
    );

    // Prepare restart info (copy out while we hold the lock).
    let restart_info = RestartInfo {
        name: entry.name,
        name_len: entry.name_len,
        priority: entry.priority,
        entry: entry.entry,
        arg: entry.arg,
        pml4_phys: entry.pml4_phys,
        slot_idx,
    };

    entry.restart_count = entry.restart_count.saturating_add(1);

    // Mark the slot's task_id as 0 temporarily (will be updated on restart).
    entry.task_id = 0;

    drop(table);

    crate::klog!(Info, "sched.supervisor",
        "Task {} died, scheduling restart #{} in {} ticks",
        task_id, slot_idx, delay
    );

    // Schedule the restart via ktimer.
    // We can't spawn directly here because we might be in the dying
    // task's context (holding scheduler locks, etc.).
    schedule_restart(restart_info, delay);
}

// ---------------------------------------------------------------------------
// Restart scheduling
// ---------------------------------------------------------------------------

/// Info needed to restart a task (copied from the supervised table).
#[derive(Clone, Copy)]
struct RestartInfo {
    name: [u8; 32],
    name_len: usize,
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    pml4_phys: u64,
    slot_idx: usize,
}

/// Pending restart slots.  The ktimer callback uses the slot_idx
/// encoded in the argument to find the restart info.
static PENDING_RESTARTS: Mutex<[Option<RestartInfo>; MAX_SUPERVISED]> =
    Mutex::new([None; MAX_SUPERVISED]);

/// Schedule a deferred restart via ktimer.
fn schedule_restart(info: RestartInfo, delay_ticks: u64) {
    // Store the restart info in a pending slot.
    let mut pending = PENDING_RESTARTS.lock();
    if let Some(slot) = pending.get_mut(info.slot_idx) {
        *slot = Some(info);
    }
    drop(pending);

    // Schedule the timer callback.
    let handle = crate::ktimer::schedule(do_restart, info.slot_idx as u64, delay_ticks);
    if handle.is_none() {
        serial_println!("[supervisor] WARNING: ktimer full, attempting immediate restart");
        // Fallback: try workqueue.
        crate::workqueue::submit(do_restart, info.slot_idx as u64);
    }
}

/// Timer/workqueue callback that performs the actual restart.
fn do_restart(slot_idx: u64) {
    let idx = slot_idx as usize;

    // Extract the restart info.
    let info = {
        let mut pending = PENDING_RESTARTS.lock();
        pending.get_mut(idx).and_then(|s| s.take())
    };

    let Some(info) = info else {
        serial_println!("[supervisor] WARNING: restart slot {} empty (cancelled?)", idx);
        return;
    };

    // Spawn the new task.
    let name_slice = &info.name[..info.name_len];
    match crate::sched::spawn(
        name_slice,
        info.priority,
        info.entry,
        info.arg,
        info.pml4_phys,
    ) {
        Ok(new_tid) => {
            // Update the supervision table with the new task ID.
            let mut table = SUPERVISED.lock();
            if let Some(entry) = table.get_mut(idx) {
                if entry.active {
                    entry.task_id = new_tid;
                }
            }
            drop(table);

            TOTAL_RESTARTS.fetch_add(1, Ordering::Relaxed);
            crate::klog!(Info, "sched.supervisor",
                "Restarted task as tid={} (slot {})",
                new_tid, idx
            );
        }
        Err(e) => {
            RESTART_FAILURES.fetch_add(1, Ordering::Relaxed);
            crate::klog!(Error, "sched.supervisor",
                "Failed to restart task (slot {}): {:?}",
                idx, e
            );
        }
    }
}

/// Calculate exponential backoff delay.
#[allow(clippy::arithmetic_side_effects)]
fn calculate_backoff(restart_count: u32, base_delay: u64, max_delay: u64) -> u64 {
    if base_delay == 0 {
        return 1; // Minimum 1 tick.
    }

    // 2^restart_count * base_delay, capped at max_delay.
    let shift = restart_count.min(10); // Prevent shift overflow.
    let multiplier = 1u64 << shift;
    let delay = base_delay.saturating_mul(multiplier);
    delay.min(max_delay).max(1)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the task supervisor.
///
/// Tests policy creation, backoff calculation, and registration.
/// Full restart testing requires spawning a task that exits, which
/// is timing-dependent — covered in integration tests.
pub fn self_test() {
    serial_println!("[supervisor] Running self-test...");

    // --- 1. Policy creation ---
    let p1 = RestartPolicy::always(5, 10, 500);
    assert_eq!(p1.mode, RestartMode::Always);
    assert_eq!(p1.max_restarts, 5);
    assert_eq!(p1.base_delay_ticks, 10);
    assert_eq!(p1.max_delay_ticks, 500);

    let p2 = RestartPolicy::never();
    assert_eq!(p2.mode, RestartMode::Never);
    serial_println!("[supervisor]   Policy creation: OK");

    // --- 2. Backoff calculation ---
    assert_eq!(calculate_backoff(0, 10, 500), 10);   // 1×10 = 10
    assert_eq!(calculate_backoff(1, 10, 500), 20);   // 2×10 = 20
    assert_eq!(calculate_backoff(2, 10, 500), 40);   // 4×10 = 40
    assert_eq!(calculate_backoff(3, 10, 500), 80);   // 8×10 = 80
    assert_eq!(calculate_backoff(5, 10, 500), 320);  // 32×10 = 320
    assert_eq!(calculate_backoff(6, 10, 500), 500);  // 64×10 = 640 → capped at 500
    assert_eq!(calculate_backoff(10, 10, 500), 500); // way over cap
    assert_eq!(calculate_backoff(0, 0, 100), 1);     // base=0 → min 1
    serial_println!("[supervisor]   Backoff calculation: OK");

    // --- 3. Initialization ---
    init();
    assert!(HOOK_REGISTERED.load(Ordering::Relaxed) != 0);
    serial_println!("[supervisor]   Initialization: OK");

    // --- 4. Active count starts at 0 ---
    assert_eq!(active_count(), 0);
    serial_println!("[supervisor]   Active count (initial): OK");

    serial_println!("[supervisor] Self-test PASSED");
}
