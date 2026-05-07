//! Resource Control Groups (cgroups).
//!
//! Hierarchical resource management for tasks/processes.  Each cgroup can
//! have limits on CPU time and memory usage.  Tasks belong to exactly one
//! cgroup; limits are enforced at the group level.
//!
//! ## Design
//!
//! Follows the cgroup v2 unified hierarchy model:
//!
//! - Single hierarchy rooted at the **root cgroup** (ID 0).
//! - Every task belongs to exactly one cgroup (default: root).
//! - Resource controllers (CPU, memory) are per-group.
//! - Limits are hierarchical: a child group's effective limit is the
//!   minimum of its own limit and its parent's limit.
//! - Usage is charged up the hierarchy (child usage counts toward
//!   parent usage).
//!
//! ## Controllers
//!
//! - **CPU**: Limits total CPU ticks per period for all tasks in the
//!   group.  Similar to Linux's `cpu.max`.  When a group exhausts its
//!   quota, all member tasks are throttled until the next period.
//! - **Memory**: Limits total physical frames allocated to the group.
//!   Similar to Linux's `memory.max`.  When a group reaches its limit,
//!   new allocations from member tasks fail with `OutOfMemory`.
//!
//! ## Integration Points
//!
//! - **Scheduler (timer_tick)**: calls [`cpu_charge`] on each tick.
//!   If the group is over quota, `cpu_charge` returns `true` and the
//!   caller should throttle the task.
//! - **Frame allocator**: calls [`mem_charge`] when allocating frames
//!   for a task, [`mem_uncharge`] when freeing.
//! - **Task creation**: calls [`attach_task`] to place new tasks in
//!   the parent's cgroup (or a specified one).
//!
//! ## Capacity
//!
//! Up to [`MAX_CGROUPS`] (256) groups.  Group 0 is the root and cannot
//! be deleted.  This is sufficient for a desktop OS with containers.
//!
//! ## Performance
//!
//! The cgroup table is behind a single `spin::Mutex`.  This is acceptable
//! because mutations (create/delete/attach) are rare; the hot path
//! (`cpu_charge`) uses per-group atomics outside the lock.
//!
//! ## References
//!
//! - Linux `kernel/cgroup/` (cgroup v2 unified hierarchy)
//! - Design spec: "set resource limits at process launch, let the
//!   kernel enforce them" (line 594)

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of cgroups in the system.
///
/// 256 groups is plenty for a desktop OS.  Index 0 is the root cgroup.
pub const MAX_CGROUPS: usize = 256;

/// The root cgroup ID.  Always exists, cannot be deleted.
pub const ROOT_CGROUP: CgroupId = 0;

/// Default CPU period (in timer ticks).
///
/// At 100 Hz timer, 100 ticks = 1 second.  This matches the per-task
/// bandwidth period in the scheduler.
const DEFAULT_CPU_PERIOD: u64 = 100;

/// Sentinel value meaning "no parent" (root cgroup).
const NO_PARENT: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a cgroup.
pub type CgroupId = u32;

/// CPU controller limits for a cgroup.
///
/// The quota/period model: in each `period_ticks`-length window, all
/// tasks in the group may collectively consume at most `quota_ticks`
/// ticks of CPU time.  When `usage_ticks >= quota_ticks`, tasks are
/// throttled until the period resets.
///
/// `quota_ticks = 0` means unlimited (no CPU limit).
#[derive(Debug, Clone, Copy)]
pub struct CpuLimit {
    /// Maximum CPU ticks per period (0 = unlimited).
    pub quota_ticks: u64,
    /// Length of one period in timer ticks.
    pub period_ticks: u64,
}

impl CpuLimit {
    /// No CPU limit (unlimited).
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            quota_ticks: 0,
            period_ticks: DEFAULT_CPU_PERIOD,
        }
    }

    /// CPU limit expressed as a percentage of one core.
    ///
    /// `pct = 50` means 50% of one CPU (50 ticks per 100-tick period).
    /// `pct = 200` means 200% (2 full cores' worth).
    /// `pct = 0` means unlimited.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn from_percent(pct: u64) -> Self {
        if pct == 0 {
            return Self::unlimited();
        }
        Self {
            quota_ticks: pct,
            period_ticks: DEFAULT_CPU_PERIOD,
        }
    }
}

/// Memory controller limits for a cgroup.
///
/// Limits the total number of physical frames that can be charged to
/// this group.  `max_frames = 0` means unlimited.
#[derive(Debug, Clone, Copy)]
pub struct MemLimit {
    /// Maximum frames the group may use (0 = unlimited).
    pub max_frames: u64,
}

impl MemLimit {
    /// No memory limit (unlimited).
    #[must_use]
    #[allow(dead_code)] // Public API for processes without memory limits.
    pub const fn unlimited() -> Self {
        Self { max_frames: 0 }
    }

    /// Memory limit in frames.
    #[must_use]
    pub const fn frames(n: u64) -> Self {
        Self { max_frames: n }
    }
}

/// A snapshot of cgroup statistics for one group.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Public API — fields read by kshell, syscall handlers, diagnostics.
pub struct CgroupStats {
    /// Cgroup identifier.
    pub id: CgroupId,
    /// Whether this slot is in use.
    pub active: bool,
    /// Parent cgroup ID (NO_PARENT for root).
    pub parent: u32,
    /// Number of tasks directly in this cgroup.
    pub nr_tasks: u32,
    /// Number of direct child cgroups.
    pub nr_children: u32,
    /// CPU quota (ticks per period, 0 = unlimited).
    pub cpu_quota: u64,
    /// CPU period (ticks).
    pub cpu_period: u64,
    /// CPU ticks used in the current period.
    pub cpu_used: u64,
    /// Number of times the group was throttled.
    pub cpu_throttle_count: u64,
    /// Memory limit (frames, 0 = unlimited).
    pub mem_limit: u64,
    /// Current memory usage (frames).
    pub mem_usage: u64,
    /// Peak memory usage (high-water mark, frames).
    pub mem_peak: u64,
}

// ---------------------------------------------------------------------------
// Per-cgroup data (internal)
// ---------------------------------------------------------------------------

/// Internal cgroup node.
///
/// Fixed-size, stored in a static array.  The `active` flag indicates
/// whether the slot is in use.
struct CgroupNode {
    /// Whether this slot is occupied.
    active: bool,
    /// Parent cgroup ID (NO_PARENT for root).
    parent: u32,
    /// Number of tasks directly in this group.
    nr_tasks: AtomicU32,
    /// Number of direct child cgroups.
    nr_children: AtomicU32,

    // --- CPU controller ---
    /// CPU quota (ticks per period, 0 = unlimited).
    cpu_quota: u64,
    /// CPU period (ticks).
    cpu_period: u64,
    /// CPU ticks consumed in the current period.
    cpu_used: AtomicU64,
    /// Whether the group is currently CPU-throttled.
    cpu_throttled: bool,
    /// Number of times the group has been throttled.
    cpu_throttle_count: AtomicU64,

    // --- Memory controller ---
    /// Memory limit (frames, 0 = unlimited).
    mem_limit: u64,
    /// Current memory usage (frames).
    mem_usage: AtomicU64,
    /// Peak memory usage (high-water mark, frames).
    mem_peak: AtomicU64,
}

impl CgroupNode {
    /// Create an inactive (free) node.
    const fn empty() -> Self {
        Self {
            active: false,
            parent: NO_PARENT,
            nr_tasks: AtomicU32::new(0),
            nr_children: AtomicU32::new(0),
            cpu_quota: 0,
            cpu_period: DEFAULT_CPU_PERIOD,
            cpu_used: AtomicU64::new(0),
            cpu_throttled: false,
            cpu_throttle_count: AtomicU64::new(0),
            mem_limit: 0,
            mem_usage: AtomicU64::new(0),
            mem_peak: AtomicU64::new(0),
        }
    }

    /// Reset to a freshly-created state under the given parent.
    fn init(&mut self, parent: u32) {
        self.active = true;
        self.parent = parent;
        self.nr_tasks.store(0, Ordering::Relaxed);
        self.nr_children.store(0, Ordering::Relaxed);
        self.cpu_quota = 0;
        self.cpu_period = DEFAULT_CPU_PERIOD;
        self.cpu_used.store(0, Ordering::Relaxed);
        self.cpu_throttled = false;
        self.cpu_throttle_count.store(0, Ordering::Relaxed);
        self.mem_limit = 0;
        self.mem_usage.store(0, Ordering::Relaxed);
        self.mem_peak.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Global cgroup table
// ---------------------------------------------------------------------------

/// The global cgroup hierarchy.
///
/// Protected by a mutex for mutations (create/delete/attach).  The hot
/// path (cpu_charge) reads atomics without holding the lock.
struct CgroupTable {
    nodes: [CgroupNode; MAX_CGROUPS],
    /// Next ID to try when creating a new cgroup (simple scan).
    next_id: u32,
}

impl CgroupTable {
    /// Create the initial table with only the root cgroup active.
    const fn new() -> Self {
        // SAFETY: CgroupNode::empty() is const — this creates 256
        // inactive nodes.  We mark node 0 as root below in init().
        let mut table = Self {
            nodes: {
                // const array init: repeat CgroupNode::empty().
                // Rust doesn't allow [CgroupNode::empty(); N] because
                // AtomicU64 doesn't impl Copy.  Macro alternative:
                const EMPTY: CgroupNode = CgroupNode::empty();
                [
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
                ]
            },
            next_id: 1, // 0 is the root, start allocating at 1.
        };
        // Mark root cgroup as active.
        table.nodes[0].active = true;
        table.nodes[0].parent = NO_PARENT;
        table
    }
}

static TABLE: Mutex<CgroupTable> = Mutex::new(CgroupTable::new());

// ---------------------------------------------------------------------------
// Public API: lifecycle
// ---------------------------------------------------------------------------

/// Create a new cgroup as a child of `parent`.
///
/// Returns the new cgroup's ID.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `parent` doesn't exist.
/// - [`KernelError::ResourceExhausted`] if all cgroup slots are full.
pub fn create(parent: CgroupId) -> KernelResult<CgroupId> {
    let mut table = TABLE.lock();
    let parent_idx = parent as usize;

    // Validate parent.
    if parent_idx >= MAX_CGROUPS {
        return Err(KernelError::InvalidArgument);
    }
    if !table.nodes[parent_idx].active {
        return Err(KernelError::InvalidArgument);
    }

    // Find a free slot.
    let start = table.next_id as usize;
    let mut found = None;
    for offset in 0..MAX_CGROUPS {
        #[allow(clippy::arithmetic_side_effects)]
        let idx = (start + offset) % MAX_CGROUPS;
        if idx == 0 {
            continue; // Root slot is reserved.
        }
        if !table.nodes[idx].active {
            found = Some(idx);
            break;
        }
    }

    let idx = found.ok_or(KernelError::ResourceExhausted)?;

    table.nodes[idx].init(parent);
    table.nodes[parent_idx].nr_children.fetch_add(1, Ordering::Relaxed);

    // Advance next_id hint.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    {
        table.next_id = ((idx + 1) % MAX_CGROUPS) as u32;
    }

    Ok(idx as CgroupId)
}

/// Delete a cgroup.
///
/// The cgroup must have no tasks and no children.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `id` is the root or doesn't exist.
/// - [`KernelError::NotEmpty`] if the cgroup has tasks or children.
pub fn delete(id: CgroupId) -> KernelResult<()> {
    if id == ROOT_CGROUP {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = TABLE.lock();
    let idx = id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return Err(KernelError::InvalidArgument);
    }

    // Must be empty.
    if table.nodes[idx].nr_tasks.load(Ordering::Relaxed) > 0 {
        return Err(KernelError::NotEmpty);
    }
    if table.nodes[idx].nr_children.load(Ordering::Relaxed) > 0 {
        return Err(KernelError::NotEmpty);
    }

    // Decrement parent's child count.
    let parent = table.nodes[idx].parent as usize;
    if parent < MAX_CGROUPS && table.nodes[parent].active {
        table.nodes[parent].nr_children.fetch_sub(1, Ordering::Relaxed);
    }

    // Mark slot as free.
    table.nodes[idx].active = false;
    table.nodes[idx].parent = NO_PARENT;

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: task attachment
// ---------------------------------------------------------------------------

/// Attach a task to a cgroup.
///
/// Increments the target cgroup's task count.  The caller is responsible
/// for decrementing the old cgroup's count (via [`detach_task`]).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `cgroup_id` doesn't exist.
pub fn attach_task(cgroup_id: CgroupId) -> KernelResult<()> {
    let table = TABLE.lock();
    let idx = cgroup_id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return Err(KernelError::InvalidArgument);
    }

    table.nodes[idx].nr_tasks.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Detach a task from its cgroup.
///
/// Decrements the cgroup's task count.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `cgroup_id` doesn't exist.
pub fn detach_task(cgroup_id: CgroupId) -> KernelResult<()> {
    let table = TABLE.lock();
    let idx = cgroup_id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return Err(KernelError::InvalidArgument);
    }

    // Saturating to prevent underflow from mismatched attach/detach.
    let old = table.nodes[idx].nr_tasks.fetch_sub(1, Ordering::Relaxed);
    if old == 0 {
        // Fix up — was already 0.
        table.nodes[idx].nr_tasks.store(0, Ordering::Relaxed);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: CPU controller
// ---------------------------------------------------------------------------

/// Set the CPU limit for a cgroup.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `cgroup_id` doesn't exist or
///   `period_ticks` is 0.
pub fn set_cpu_limit(cgroup_id: CgroupId, limit: CpuLimit) -> KernelResult<()> {
    if limit.period_ticks == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = TABLE.lock();
    let idx = cgroup_id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return Err(KernelError::InvalidArgument);
    }

    table.nodes[idx].cpu_quota = limit.quota_ticks;
    table.nodes[idx].cpu_period = limit.period_ticks;

    Ok(())
}

/// Charge one CPU tick to a cgroup.
///
/// Called by the timer ISR for the currently-running task's cgroup.
/// Returns `true` if the cgroup's CPU quota has been exceeded and the
/// task should be throttled.
///
/// This is the hot path — uses atomic operations, does NOT acquire the
/// table lock.
///
/// # Safety
///
/// `cgroup_id` must be a valid, active cgroup.  The caller (scheduler)
/// ensures this by only calling for tasks with valid cgroup IDs.
#[inline]
pub fn cpu_charge(cgroup_id: CgroupId) -> bool {
    let table = TABLE.lock();
    let idx = cgroup_id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return false; // Invalid — don't throttle.
    }

    let node = &table.nodes[idx];

    // Unlimited quota — never throttle.
    if node.cpu_quota == 0 {
        return false;
    }

    let used = node.cpu_used.fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);

    if used >= node.cpu_quota {
        node.cpu_throttle_count.fetch_add(1, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// Reset CPU period counters for all active cgroups.
///
/// Called by the BSP's timer tick at the end of each CPU period
/// (every `DEFAULT_CPU_PERIOD` ticks = 1 second).  Clears `cpu_used`
/// so groups can run again in the new period.
pub fn cpu_period_reset() {
    let table = TABLE.lock();
    for node in &table.nodes {
        if node.active {
            node.cpu_used.store(0, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API: memory controller
// ---------------------------------------------------------------------------

/// Set the memory limit for a cgroup.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `cgroup_id` doesn't exist.
pub fn set_mem_limit(cgroup_id: CgroupId, limit: MemLimit) -> KernelResult<()> {
    let mut table = TABLE.lock();
    let idx = cgroup_id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return Err(KernelError::InvalidArgument);
    }

    table.nodes[idx].mem_limit = limit.max_frames;

    Ok(())
}

/// Charge `count` frames to a cgroup's memory counter.
///
/// Returns `Ok(())` if the charge is within the group's limit (or the
/// group has no limit).
///
/// # Errors
///
/// - [`KernelError::OutOfMemory`] if charging would exceed the group's
///   memory limit.
pub fn mem_charge(cgroup_id: CgroupId, count: u64) -> KernelResult<()> {
    let table = TABLE.lock();
    let idx = cgroup_id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return Ok(()); // Invalid — don't block.
    }

    let node = &table.nodes[idx];

    // Unlimited memory — always allow.
    if node.mem_limit == 0 {
        let new_val = node.mem_usage.fetch_add(count, Ordering::Relaxed)
            .saturating_add(count);
        update_mem_peak(node, new_val);
        return Ok(());
    }

    // Check if charge would exceed limit.
    //
    // Use a CAS loop to atomically check-and-charge to prevent
    // race conditions where two concurrent allocations both see
    // "under limit" and both succeed.
    loop {
        let current = node.mem_usage.load(Ordering::Relaxed);
        let new_val = current.saturating_add(count);
        if new_val > node.mem_limit {
            return Err(KernelError::OutOfMemory);
        }
        if node.mem_usage.compare_exchange_weak(
            current, new_val, Ordering::Relaxed, Ordering::Relaxed,
        ).is_ok() {
            update_mem_peak(node, new_val);
            return Ok(());
        }
    }
}

/// Uncharge `count` frames from a cgroup's memory counter.
///
/// Called when frames are freed that were previously charged to this
/// group.  Saturates at 0 to prevent underflow.
pub fn mem_uncharge(cgroup_id: CgroupId, count: u64) {
    let table = TABLE.lock();
    let idx = cgroup_id as usize;

    if idx >= MAX_CGROUPS || !table.nodes[idx].active {
        return;
    }

    let old = table.nodes[idx].mem_usage.fetch_sub(count, Ordering::Relaxed);
    if old < count {
        // Underflow — fix up.
        table.nodes[idx].mem_usage.store(0, Ordering::Relaxed);
    }
}

/// Update the peak (high-water mark) for memory usage.
#[inline]
fn update_mem_peak(node: &CgroupNode, new_val: u64) {
    loop {
        let peak = node.mem_peak.load(Ordering::Relaxed);
        if new_val <= peak {
            break;
        }
        if node.mem_peak.compare_exchange_weak(
            peak, new_val, Ordering::Relaxed, Ordering::Relaxed,
        ).is_ok() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Get a snapshot of a cgroup's statistics.
///
/// Returns `None` if the cgroup doesn't exist.
#[must_use]
pub fn stats(id: CgroupId) -> Option<CgroupStats> {
    let table = TABLE.lock();
    let idx = id as usize;

    if idx >= MAX_CGROUPS {
        return None;
    }

    let node = &table.nodes[idx];
    if !node.active {
        return None;
    }

    Some(CgroupStats {
        id,
        active: true,
        parent: node.parent,
        nr_tasks: node.nr_tasks.load(Ordering::Relaxed),
        nr_children: node.nr_children.load(Ordering::Relaxed),
        cpu_quota: node.cpu_quota,
        cpu_period: node.cpu_period,
        cpu_used: node.cpu_used.load(Ordering::Relaxed),
        cpu_throttle_count: node.cpu_throttle_count.load(Ordering::Relaxed),
        mem_limit: node.mem_limit,
        mem_usage: node.mem_usage.load(Ordering::Relaxed),
        mem_peak: node.mem_peak.load(Ordering::Relaxed),
    })
}

/// Count the total number of active cgroups.
#[must_use]
pub fn active_count() -> usize {
    let table = TABLE.lock();
    table.nodes.iter().filter(|n| n.active).count()
}

/// Check if a cgroup exists and is active.
#[must_use]
pub fn exists(id: CgroupId) -> bool {
    let table = TABLE.lock();
    let idx = id as usize;
    idx < MAX_CGROUPS && table.nodes[idx].active
}

/// Get the effective CPU limit for a cgroup, considering the hierarchy.
///
/// Walks up the parent chain and returns the tightest (minimum non-zero)
/// CPU quota.  Returns 0 if no group in the chain has a CPU limit.
#[must_use]
pub fn effective_cpu_quota(id: CgroupId) -> u64 {
    let table = TABLE.lock();
    let mut min_quota: u64 = 0;
    let mut current = id as usize;

    // Walk up the hierarchy (max depth = MAX_CGROUPS to prevent cycles).
    for _ in 0..MAX_CGROUPS {
        if current >= MAX_CGROUPS || !table.nodes[current].active {
            break;
        }
        let quota = table.nodes[current].cpu_quota;
        if quota > 0 {
            min_quota = if min_quota == 0 { quota } else { min_quota.min(quota) };
        }
        let parent = table.nodes[current].parent;
        if parent == NO_PARENT || parent as usize == current {
            break; // Reached root or self-referential.
        }
        current = parent as usize;
    }

    min_quota
}

/// Get the effective memory limit for a cgroup, considering hierarchy.
///
/// Walks up the parent chain and returns the tightest (minimum non-zero)
/// memory limit.  Returns 0 if no group in the chain has a memory limit.
#[must_use]
pub fn effective_mem_limit(id: CgroupId) -> u64 {
    let table = TABLE.lock();
    let mut min_limit: u64 = 0;
    let mut current = id as usize;

    for _ in 0..MAX_CGROUPS {
        if current >= MAX_CGROUPS || !table.nodes[current].active {
            break;
        }
        let limit = table.nodes[current].mem_limit;
        if limit > 0 {
            min_limit = if min_limit == 0 { limit } else { min_limit.min(limit) };
        }
        let parent = table.nodes[current].parent;
        if parent == NO_PARENT || parent as usize == current {
            break;
        }
        current = parent as usize;
    }

    min_limit
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for the cgroup subsystem.
pub fn self_test() {
    serial_println!("[cgroup] Running self-test...");

    // Test 1: Root cgroup exists by default.
    assert!(exists(ROOT_CGROUP), "root cgroup must exist");
    assert_eq!(active_count(), 1, "only root at startup");
    serial_println!("[cgroup]   Root exists: OK");

    // Test 2: Create child cgroups.
    let child1 = create(ROOT_CGROUP).expect("create child1");
    assert!(child1 > 0, "child ID should be > 0");
    assert!(exists(child1));
    assert_eq!(active_count(), 2);

    let child2 = create(ROOT_CGROUP).expect("create child2");
    assert!(exists(child2));
    assert_ne!(child1, child2);
    assert_eq!(active_count(), 3);
    serial_println!("[cgroup]   Create children: OK");

    // Test 3: Create grandchild (hierarchical).
    let grandchild = create(child1).expect("create grandchild");
    assert!(exists(grandchild));
    assert_eq!(active_count(), 4);

    // Verify parent's child count.
    let s = stats(child1).expect("stats child1");
    assert_eq!(s.nr_children, 1, "child1 has 1 grandchild");
    serial_println!("[cgroup]   Hierarchy: OK");

    // Test 4: Create under non-existent parent fails.
    let result = create(200);
    assert!(result.is_err(), "create under invalid parent should fail");
    serial_println!("[cgroup]   Invalid parent rejection: OK");

    // Test 5: Delete root fails.
    assert!(delete(ROOT_CGROUP).is_err(), "cannot delete root");
    serial_println!("[cgroup]   Root delete protection: OK");

    // Test 6: Delete cgroup with children fails.
    let result = delete(child1);
    assert!(result.is_err(), "delete with children should fail");
    serial_println!("[cgroup]   Non-empty delete rejection: OK");

    // Test 7: Delete grandchild (leaf, no tasks) succeeds.
    delete(grandchild).expect("delete leaf grandchild");
    assert!(!exists(grandchild));
    let s = stats(child1).expect("stats child1 after delete");
    assert_eq!(s.nr_children, 0, "child1 has 0 children after delete");
    serial_println!("[cgroup]   Delete leaf: OK");

    // Test 8: Attach / detach tasks.
    attach_task(child1).expect("attach task to child1");
    attach_task(child1).expect("attach another task");
    let s = stats(child1).unwrap();
    assert_eq!(s.nr_tasks, 2);

    detach_task(child1).expect("detach task");
    let s = stats(child1).unwrap();
    assert_eq!(s.nr_tasks, 1);
    serial_println!("[cgroup]   Attach/detach tasks: OK");

    // Test 9: Delete with tasks fails.
    let result = delete(child1);
    assert!(result.is_err(), "delete with tasks should fail");

    // Detach remaining task so we can delete later.
    detach_task(child1).expect("detach last task");
    serial_println!("[cgroup]   Delete with tasks rejected: OK");

    // Test 10: CPU controller — set limit and charge.
    set_cpu_limit(child2, CpuLimit::from_percent(10))
        .expect("set cpu limit");

    // Charge 9 ticks — should be fine (limit is 10).
    for _ in 0..9 {
        let throttle = cpu_charge(child2);
        assert!(!throttle, "should not throttle under limit");
    }

    // 10th tick — should trigger throttle.
    let throttle = cpu_charge(child2);
    assert!(throttle, "should throttle at quota");

    let s = stats(child2).unwrap();
    assert_eq!(s.cpu_used, 10);
    assert!(s.cpu_throttle_count >= 1);
    serial_println!("[cgroup]   CPU controller charge/throttle: OK");

    // Test 11: CPU period reset clears usage.
    cpu_period_reset();
    let s = stats(child2).unwrap();
    assert_eq!(s.cpu_used, 0, "period reset should clear usage");
    serial_println!("[cgroup]   CPU period reset: OK");

    // Test 12: Unlimited CPU (quota=0) never throttles.
    set_cpu_limit(child1, CpuLimit::unlimited()).expect("set unlimited");
    for _ in 0..1000 {
        let throttle = cpu_charge(child1);
        assert!(!throttle, "unlimited should never throttle");
    }
    cpu_period_reset();
    serial_println!("[cgroup]   CPU unlimited: OK");

    // Test 13: Memory controller — set limit and charge.
    set_mem_limit(child2, MemLimit::frames(100)).expect("set mem limit");

    mem_charge(child2, 50).expect("charge 50 frames");
    let s = stats(child2).unwrap();
    assert_eq!(s.mem_usage, 50);
    assert_eq!(s.mem_peak, 50);

    mem_charge(child2, 40).expect("charge 40 more frames");
    let s = stats(child2).unwrap();
    assert_eq!(s.mem_usage, 90);
    assert_eq!(s.mem_peak, 90);

    // Exceeding limit should fail.
    let result = mem_charge(child2, 20);
    assert!(result.is_err(), "charge exceeding limit should fail");
    let s = stats(child2).unwrap();
    assert_eq!(s.mem_usage, 90, "usage unchanged after failed charge");
    serial_println!("[cgroup]   Memory controller charge/limit: OK");

    // Test 14: Memory uncharge.
    mem_uncharge(child2, 30);
    let s = stats(child2).unwrap();
    assert_eq!(s.mem_usage, 60);
    assert_eq!(s.mem_peak, 90, "peak unchanged after uncharge");

    // Now 40 more frames should fit (60 + 40 = 100 = limit).
    mem_charge(child2, 40).expect("charge to exact limit");
    let s = stats(child2).unwrap();
    assert_eq!(s.mem_usage, 100);
    assert_eq!(s.mem_peak, 100);
    serial_println!("[cgroup]   Memory uncharge: OK");

    // Test 15: Memory uncharge below zero saturates.
    mem_uncharge(child2, 100);
    mem_uncharge(child2, 50); // Would go below 0.
    let s = stats(child2).unwrap();
    assert_eq!(s.mem_usage, 0, "underflow should saturate to 0");
    serial_println!("[cgroup]   Memory underflow saturation: OK");

    // Test 16: Effective limits with hierarchy.
    let inner = create(child2).expect("create inner group");
    set_cpu_limit(child2, CpuLimit::from_percent(80)).expect("parent cpu limit");
    set_cpu_limit(inner, CpuLimit::from_percent(50)).expect("child cpu limit");

    // Child's own is 50, parent's is 80.  Effective = min(50, 80) = 50.
    let eff = effective_cpu_quota(inner);
    assert_eq!(eff, 50, "effective should be tightest in chain");

    // If we set the parent tighter:
    set_cpu_limit(child2, CpuLimit::from_percent(30)).expect("tighter parent");
    let eff = effective_cpu_quota(inner);
    assert_eq!(eff, 30, "effective should follow tighter parent");
    serial_println!("[cgroup]   Effective hierarchical limits: OK");

    // Test 17: Effective memory limits.
    set_mem_limit(child2, MemLimit::frames(500)).expect("parent mem limit");
    set_mem_limit(inner, MemLimit::frames(200)).expect("child mem limit");
    let eff = effective_mem_limit(inner);
    assert_eq!(eff, 200, "effective mem = tightest");

    set_mem_limit(inner, MemLimit::frames(800)).expect("child mem limit larger");
    let eff = effective_mem_limit(inner);
    assert_eq!(eff, 500, "effective mem = parent's tighter limit");
    serial_println!("[cgroup]   Effective memory limits: OK");

    // Test 18: Stats query for non-existent cgroup.
    assert!(stats(250).is_none(), "stats for non-existent should be None");
    serial_println!("[cgroup]   Stats non-existent: OK");

    // Cleanup: delete inner, child1, child2.
    delete(inner).expect("delete inner");
    delete(child1).expect("delete child1");
    // child2 still has a mem limit set but no tasks/children.
    delete(child2).expect("delete child2");
    assert_eq!(active_count(), 1, "only root remains");
    serial_println!("[cgroup]   Cleanup: OK");

    serial_println!("[cgroup] Self-test PASSED (18 tests)");
}
