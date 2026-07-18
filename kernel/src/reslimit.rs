//! Per-service / per-process resource limits (cgroup-equivalent).
//!
//! Provides a hierarchical resource control system where every process or
//! service can be assigned limits on:
//!
//! - **CPU**: maximum CPU time percentage, CPU affinity mask
//! - **Memory**: resident set size (RSS) cap, virtual memory cap
//! - **I/O**: read/write bandwidth limits, IOPS caps
//! - **Process**: maximum child processes, maximum threads
//! - **File**: maximum open file handles
//!
//! ## Design
//!
//! Based on Linux cgroups v2 concepts but simplified for our microkernel:
//!
//! - Limits are set at process launch by the service manager (per design.txt:
//!   "set resource limits at process launch, let the kernel enforce them").
//! - Each limit group has a unique ID and optional parent (for hierarchical
//!   nesting — a service's child processes inherit the service group's limits).
//! - Usage tracking is updated by kernel subsystems (memory allocator reports
//!   RSS changes, scheduler reports CPU time, etc.).
//! - Enforcement: hard limits trigger `ResourceExhausted` errors on the next
//!   allocation attempt; soft limits emit warnings via syslog.
//!
//! ## Integration
//!
//! - The service manager (`svcstart`) assigns a resource group to each service.
//! - The process manager assigns child processes to their parent's group.
//! - Kernel subsystems call `check_*()` functions before resource allocation.
//! - `/proc/reslimit` exposes current limits and usage.
//! - Kshell `reslimit` command for inspection and manual adjustment.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of resource groups.
const MAX_GROUPS: usize = 256;

/// Unlimited sentinel — when a limit field is set to this, no limit is enforced.
pub const UNLIMITED: u64 = u64::MAX;

/// Default maximum open file handles per group.
const DEFAULT_MAX_OPEN_FILES: u64 = 1024;

/// Default maximum child processes per group.
const DEFAULT_MAX_PROCESSES: u64 = 128;

/// Default maximum threads per group.
const DEFAULT_MAX_THREADS: u64 = 256;

// ---------------------------------------------------------------------------
// Types — CPU Limits
// ---------------------------------------------------------------------------

/// CPU resource limits for a group.
#[derive(Debug, Clone)]
pub struct CpuLimits {
    /// Maximum CPU time as percentage of one core (100 = one full core,
    /// 200 = two cores, 0 = unlimited). Enforced per scheduling period.
    pub max_cpu_percent: u64,

    /// CPU affinity bitmask — which CPUs this group may run on.
    /// 0 means "all CPUs" (no restriction).
    pub affinity_mask: u64,

    /// Scheduling weight for proportional sharing (1–1000, default 100).
    /// Higher weight = more CPU time relative to peers when contending.
    pub weight: u32,

    /// Whether this is a soft limit (warning only) or hard limit (enforcement).
    pub soft: bool,
}

impl Default for CpuLimits {
    fn default() -> Self {
        Self {
            max_cpu_percent: 0, // unlimited
            affinity_mask: 0,   // all CPUs
            weight: 100,
            soft: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Types — Memory Limits
// ---------------------------------------------------------------------------

/// Memory resource limits for a group.
#[derive(Debug, Clone)]
pub struct MemoryLimits {
    /// Maximum resident set size (bytes). UNLIMITED = no cap.
    pub max_rss: u64,

    /// Maximum virtual address space (bytes). UNLIMITED = no cap.
    pub max_virtual: u64,

    /// Maximum kernel memory (slab/stack, bytes). UNLIMITED = no cap.
    pub max_kernel_memory: u64,

    /// Low-memory threshold — when RSS exceeds this, the group becomes a
    /// candidate for memory reclaim (pages can be evicted). UNLIMITED = no
    /// soft threshold.
    pub soft_rss: u64,

    /// Whether to enable OOM kill for this group (kill the largest
    /// process if the hard limit is hit). If false, new allocations
    /// simply fail with OutOfMemory.
    pub oom_kill: bool,

    /// OOM kill priority adjustment (-1000 to 1000). Higher = more
    /// likely to be killed when system is under memory pressure.
    /// -1000 = never killed (critical service).
    pub oom_score_adj: i32,
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            max_rss: UNLIMITED,
            max_virtual: UNLIMITED,
            max_kernel_memory: UNLIMITED,
            soft_rss: UNLIMITED,
            oom_kill: true,
            oom_score_adj: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Types — I/O Limits
// ---------------------------------------------------------------------------

/// I/O resource limits for a group.
#[derive(Debug, Clone)]
pub struct IoLimits {
    /// Maximum read bandwidth (bytes/sec). UNLIMITED = no cap.
    pub max_read_bps: u64,

    /// Maximum write bandwidth (bytes/sec). UNLIMITED = no cap.
    pub max_write_bps: u64,

    /// Maximum read IOPS. UNLIMITED = no cap.
    pub max_read_iops: u64,

    /// Maximum write IOPS. UNLIMITED = no cap.
    pub max_write_iops: u64,

    /// I/O scheduling weight (1–1000, default 100). Higher = higher
    /// priority in the BFQ-equivalent I/O scheduler.
    pub weight: u32,

    /// Whether this group's I/O should be marked low-priority
    /// (background service loading, per roadmap 2.6).
    pub low_priority: bool,
}

impl Default for IoLimits {
    fn default() -> Self {
        Self {
            max_read_bps: UNLIMITED,
            max_write_bps: UNLIMITED,
            max_read_iops: UNLIMITED,
            max_write_iops: UNLIMITED,
            weight: 100,
            low_priority: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Types — Process/Thread Limits
// ---------------------------------------------------------------------------

/// Process and thread limits for a group.
#[derive(Debug, Clone)]
pub struct ProcessLimits {
    /// Maximum number of child processes. UNLIMITED = no cap.
    pub max_processes: u64,

    /// Maximum number of threads (across all processes in group).
    pub max_threads: u64,

    /// Maximum open file handles (across all processes in group).
    pub max_open_files: u64,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            max_processes: DEFAULT_MAX_PROCESSES,
            max_threads: DEFAULT_MAX_THREADS,
            max_open_files: DEFAULT_MAX_OPEN_FILES,
        }
    }
}

// ---------------------------------------------------------------------------
// Types — Usage Tracking
// ---------------------------------------------------------------------------

/// Current resource usage for a group, updated by kernel subsystems.
#[derive(Debug, Clone)]
pub struct Usage {
    // CPU
    /// Total CPU time consumed (nanoseconds), across all processes.
    pub cpu_time_ns: u64,
    /// CPU time consumed in the current scheduling period (ns).
    pub cpu_period_ns: u64,
    /// Timestamp of current period start (ns since boot).
    pub period_start_ns: u64,

    // Memory
    /// Current resident set size (bytes).
    pub rss_bytes: u64,
    /// Current virtual memory size (bytes).
    pub virtual_bytes: u64,
    /// Peak RSS observed (bytes).
    pub peak_rss_bytes: u64,
    /// Kernel memory usage (bytes).
    pub kernel_memory_bytes: u64,
    /// Number of OOM events.
    pub oom_events: u64,

    // I/O
    /// Total bytes read.
    pub io_read_bytes: u64,
    /// Total bytes written.
    pub io_write_bytes: u64,
    /// Total read operations.
    pub io_read_ops: u64,
    /// Total write operations.
    pub io_write_ops: u64,
    /// Bytes read in current second (for rate limiting).
    pub io_read_bps_current: u64,
    /// Bytes written in current second (for rate limiting).
    pub io_write_bps_current: u64,
    /// Read ops in current second.
    pub io_read_iops_current: u64,
    /// Write ops in current second.
    pub io_write_iops_current: u64,
    /// Timestamp of current I/O accounting second.
    pub io_period_start_ns: u64,

    // Processes/Threads/Files
    /// Current number of processes.
    pub process_count: u64,
    /// Current number of threads.
    pub thread_count: u64,
    /// Current number of open file handles.
    pub open_files: u64,
}

impl Usage {
    const fn new() -> Self {
        Self {
            cpu_time_ns: 0,
            cpu_period_ns: 0,
            period_start_ns: 0,
            rss_bytes: 0,
            virtual_bytes: 0,
            peak_rss_bytes: 0,
            kernel_memory_bytes: 0,
            oom_events: 0,
            io_read_bytes: 0,
            io_write_bytes: 0,
            io_read_ops: 0,
            io_write_ops: 0,
            io_read_bps_current: 0,
            io_write_bps_current: 0,
            io_read_iops_current: 0,
            io_write_iops_current: 0,
            io_period_start_ns: 0,
            process_count: 0,
            thread_count: 0,
            open_files: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Types — Resource Group
// ---------------------------------------------------------------------------

/// A resource group — a named collection of limits with usage tracking.
///
/// Analogous to a Linux cgroup. Each service or process is assigned to exactly
/// one resource group. Groups can be nested (child inherits parent limits).
#[derive(Debug, Clone)]
pub struct ResourceGroup {
    /// Unique group ID.
    pub id: u32,
    /// Human-readable name (e.g., service name or "system.default").
    pub name: String,
    /// Parent group ID (0 = root / no parent).
    pub parent_id: u32,
    /// CPU limits.
    pub cpu: CpuLimits,
    /// Memory limits.
    pub memory: MemoryLimits,
    /// I/O limits.
    pub io: IoLimits,
    /// Process/thread/file limits.
    pub process: ProcessLimits,
    /// Current resource usage.
    pub usage: Usage,
    /// Whether this group is active (has processes assigned).
    pub active: bool,
    /// Number of processes currently in this group.
    pub member_count: u32,
    /// Timestamp when the group was created (ns since boot).
    pub created_ns: u64,
    /// Whether enforcement is enabled (can be paused for debugging).
    pub enforce: bool,
}

// ---------------------------------------------------------------------------
// Types — Check Result
// ---------------------------------------------------------------------------

/// Result of a resource limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitCheck {
    /// Within limits, allocation may proceed.
    Ok,
    /// Soft limit exceeded — warning emitted, allocation proceeds.
    SoftExceeded,
    /// Hard limit exceeded — allocation must be denied.
    HardExceeded,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    /// All resource groups.
    groups: Vec<ResourceGroup>,
    /// Next group ID to assign.
    next_id: u32,
    /// Process-to-group mapping: (pid, group_id).
    assignments: Vec<(u32, u32)>,
    /// Total limit violations (hard).
    total_hard_violations: u64,
    /// Total soft limit warnings.
    total_soft_warnings: u64,
    /// Whether initialized.
    initialized: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            groups: Vec::new(),
            next_id: 1,
            assignments: Vec::new(),
            total_hard_violations: 0,
            total_soft_warnings: 0,
            initialized: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the resource limits subsystem.
///
/// Creates the root resource group (id=1, "root") with unlimited limits.
/// All processes belong to this group by default.
pub fn init() {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }

    let now = crate::hpet::elapsed_ns();
    let root = ResourceGroup {
        id: 1,
        name: String::from("root"),
        parent_id: 0,
        cpu: CpuLimits::default(),
        memory: MemoryLimits::default(),
        io: IoLimits::default(),
        process: ProcessLimits {
            max_processes: UNLIMITED,
            max_threads: UNLIMITED,
            max_open_files: UNLIMITED,
        },
        usage: Usage::new(),
        active: true,
        member_count: 0,
        created_ns: now,
        enforce: true,
    };

    state.groups.push(root);
    state.next_id = 2;
    state.initialized = true;

    crate::syslog!("init.reslimit", Info,
        "Resource limits subsystem initialized (root group created)");
}

// ---------------------------------------------------------------------------
// Group Management
// ---------------------------------------------------------------------------

/// Create a new resource group with default (unlimited) limits.
///
/// # Arguments
/// - `name`: Human-readable group name
/// - `parent_id`: Parent group ID (0 = root as parent)
///
/// # Returns
/// The new group's ID.
pub fn create_group(name: &str, parent_id: u32) -> KernelResult<u32> {
    let mut state = STATE.lock();

    if state.groups.len() >= MAX_GROUPS {
        return Err(KernelError::ResourceExhausted);
    }

    // If parent_id is specified (non-zero), verify it exists.
    let effective_parent = if parent_id == 0 { 1 } else { parent_id };
    if !state.groups.iter().any(|g| g.id == effective_parent) {
        return Err(KernelError::NotFound);
    }

    // Check for duplicate name under same parent.
    if state.groups.iter().any(|g| g.name == name && g.parent_id == effective_parent) {
        return Err(KernelError::AlreadyExists);
    }

    let now = crate::hpet::elapsed_ns();
    let id = state.next_id;
    #[allow(clippy::arithmetic_side_effects)]
    { state.next_id += 1; }

    let group = ResourceGroup {
        id,
        name: String::from(name),
        parent_id: effective_parent,
        cpu: CpuLimits::default(),
        memory: MemoryLimits::default(),
        io: IoLimits::default(),
        process: ProcessLimits::default(),
        usage: Usage::new(),
        active: false,
        member_count: 0,
        created_ns: now,
        enforce: true,
    };

    state.groups.push(group);

    crate::syslog!("init.reslimit", Info,
        "Created resource group '{}' (id={}, parent={})", name, id, effective_parent);

    Ok(id)
}

/// Remove a resource group.
///
/// Fails if the group has active members or child groups.
pub fn remove_group(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();

    // Cannot remove root group.
    if id <= 1 {
        return Err(KernelError::InvalidArgument);
    }

    let idx = state.groups.iter().position(|g| g.id == id)
        .ok_or(KernelError::NotFound)?;

    // Check for active members.
    if state.groups[idx].member_count > 0 {
        return Err(KernelError::DeviceBusy);
    }

    // Check for child groups.
    if state.groups.iter().any(|g| g.parent_id == id) {
        return Err(KernelError::NotEmpty);
    }

    // Remove any process assignments for this group.
    state.assignments.retain(|&(_, gid)| gid != id);

    let name = state.groups[idx].name.clone();
    state.groups.swap_remove(idx);

    crate::syslog!("init.reslimit", Info,
        "Removed resource group '{}' (id={})", name, id);

    Ok(())
}

/// List all resource groups.
pub fn list_groups() -> Vec<(u32, String, u32, u32, bool)> {
    let state = STATE.lock();
    state.groups.iter().map(|g| {
        (g.id, g.name.clone(), g.parent_id, g.member_count, g.enforce)
    }).collect()
}

// ---------------------------------------------------------------------------
// Limit Configuration
// ---------------------------------------------------------------------------

/// Set CPU limits for a resource group.
pub fn set_cpu_limits(group_id: u32, limits: CpuLimits) -> KernelResult<()> {
    let mut state = STATE.lock();
    let group = state.groups.iter_mut().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;
    group.cpu = limits;
    Ok(())
}

/// Set memory limits for a resource group.
pub fn set_memory_limits(group_id: u32, limits: MemoryLimits) -> KernelResult<()> {
    let mut state = STATE.lock();
    let group = state.groups.iter_mut().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;

    // Validate: soft_rss should not exceed max_rss.
    if limits.soft_rss != UNLIMITED && limits.max_rss != UNLIMITED
        && limits.soft_rss > limits.max_rss
    {
        return Err(KernelError::InvalidArgument);
    }

    group.memory = limits;
    Ok(())
}

/// Set I/O limits for a resource group.
pub fn set_io_limits(group_id: u32, limits: IoLimits) -> KernelResult<()> {
    let mut state = STATE.lock();
    let group = state.groups.iter_mut().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;
    group.io = limits;
    Ok(())
}

/// Set process/thread/file limits for a resource group.
pub fn set_process_limits(group_id: u32, limits: ProcessLimits) -> KernelResult<()> {
    let mut state = STATE.lock();
    let group = state.groups.iter_mut().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;
    group.process = limits;
    Ok(())
}

/// Enable or disable enforcement for a group (useful for debugging).
pub fn set_enforce(group_id: u32, enforce: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let group = state.groups.iter_mut().find(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;
    group.enforce = enforce;
    Ok(())
}

// ---------------------------------------------------------------------------
// Process Assignment
// ---------------------------------------------------------------------------

/// Assign a process to a resource group.
///
/// A process can only belong to one group. If already assigned, it is
/// moved to the new group.
pub fn assign_process(pid: u32, group_id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();

    // Verify group exists.
    let gidx = state.groups.iter().position(|g| g.id == group_id)
        .ok_or(KernelError::NotFound)?;

    // Remove from old group if already assigned.
    if let Some(pos) = state.assignments.iter().position(|&(p, _)| p == pid) {
        let old_gid = state.assignments[pos].1;
        if let Some(old_group) = state.groups.iter_mut().find(|g| g.id == old_gid) {
            old_group.member_count = old_group.member_count.saturating_sub(1);
            if old_group.member_count == 0 {
                old_group.active = false;
            }
        }
        state.assignments[pos].1 = group_id;
    } else {
        state.assignments.push((pid, group_id));
    }

    state.groups[gidx].member_count = state.groups[gidx].member_count.saturating_add(1);
    state.groups[gidx].active = true;

    Ok(())
}

/// Remove a process from its resource group (e.g., on process exit).
pub fn unassign_process(pid: u32) -> KernelResult<()> {
    let mut state = STATE.lock();

    let pos = state.assignments.iter().position(|&(p, _)| p == pid)
        .ok_or(KernelError::NotFound)?;

    let group_id = state.assignments[pos].1;
    state.assignments.swap_remove(pos);

    if let Some(group) = state.groups.iter_mut().find(|g| g.id == group_id) {
        group.member_count = group.member_count.saturating_sub(1);
        if group.member_count == 0 {
            group.active = false;
        }
    }

    Ok(())
}

/// Look up which group a process belongs to.
pub fn group_for_process(pid: u32) -> Option<u32> {
    let state = STATE.lock();
    state.assignments.iter()
        .find(|&&(p, _)| p == pid)
        .map(|&(_, gid)| gid)
}

// ---------------------------------------------------------------------------
// Usage Reporting (called by kernel subsystems)
// ---------------------------------------------------------------------------

/// Report CPU time consumed by a process (delta since last report).
///
/// Called by the scheduler at context switch or timer tick.
pub fn report_cpu_time(pid: u32, delta_ns: u64) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return, // Process not in any group — no accounting.
    };

    let now = crate::hpet::elapsed_ns();

    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        #[allow(clippy::arithmetic_side_effects)]
        { group.usage.cpu_time_ns += delta_ns; }

        // Reset period counter if a new 100ms period started.
        const PERIOD_NS: u64 = 100_000_000; // 100ms scheduling period
        if now.saturating_sub(group.usage.period_start_ns) >= PERIOD_NS {
            group.usage.cpu_period_ns = 0;
            group.usage.period_start_ns = now;
        }

        #[allow(clippy::arithmetic_side_effects)]
        { group.usage.cpu_period_ns += delta_ns; }
    }
}

/// Report memory allocation change for a process.
///
/// Called by the memory manager on mmap/munmap/page fault.
///
/// - `rss_delta`: signed change in RSS bytes (positive = allocated, negative = freed)
/// - `virtual_delta`: signed change in virtual memory
pub fn report_memory_change(pid: u32, rss_delta: i64, virtual_delta: i64) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };

    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        if rss_delta >= 0 {
            #[allow(clippy::arithmetic_side_effects)]
            { group.usage.rss_bytes += rss_delta as u64; }
        } else {
            group.usage.rss_bytes = group.usage.rss_bytes
                .saturating_sub(rss_delta.unsigned_abs());
        }

        if virtual_delta >= 0 {
            #[allow(clippy::arithmetic_side_effects)]
            { group.usage.virtual_bytes += virtual_delta as u64; }
        } else {
            group.usage.virtual_bytes = group.usage.virtual_bytes
                .saturating_sub(virtual_delta.unsigned_abs());
        }

        // Track peak RSS.
        if group.usage.rss_bytes > group.usage.peak_rss_bytes {
            group.usage.peak_rss_bytes = group.usage.rss_bytes;
        }
    }
}

/// Report an I/O operation for a process.
///
/// Called by the block layer or VFS on read/write completion.
pub fn report_io(pid: u32, read_bytes: u64, write_bytes: u64) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };

    let now = crate::hpet::elapsed_ns();

    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        // Reset per-second counters if a new second started.
        const SECOND_NS: u64 = 1_000_000_000;
        if now.saturating_sub(group.usage.io_period_start_ns) >= SECOND_NS {
            group.usage.io_read_bps_current = 0;
            group.usage.io_write_bps_current = 0;
            group.usage.io_read_iops_current = 0;
            group.usage.io_write_iops_current = 0;
            group.usage.io_period_start_ns = now;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            group.usage.io_read_bytes += read_bytes;
            group.usage.io_write_bytes += write_bytes;
            group.usage.io_read_bps_current += read_bytes;
            group.usage.io_write_bps_current += write_bytes;
        }

        if read_bytes > 0 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                group.usage.io_read_ops += 1;
                group.usage.io_read_iops_current += 1;
            }
        }
        if write_bytes > 0 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                group.usage.io_write_ops += 1;
                group.usage.io_write_iops_current += 1;
            }
        }
    }
}

/// Report process creation in a group.
pub fn report_process_created(pid: u32) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };
    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        #[allow(clippy::arithmetic_side_effects)]
        { group.usage.process_count += 1; }
    }
}

/// Report process exit in a group.
pub fn report_process_exited(pid: u32) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };
    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        group.usage.process_count = group.usage.process_count.saturating_sub(1);
    }
}

/// Report thread creation in a group.
pub fn report_thread_created(pid: u32) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };
    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        #[allow(clippy::arithmetic_side_effects)]
        { group.usage.thread_count += 1; }
    }
}

/// Report thread exit in a group.
pub fn report_thread_exited(pid: u32) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };
    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        group.usage.thread_count = group.usage.thread_count.saturating_sub(1);
    }
}

/// Report file handle open in a group.
pub fn report_file_opened(pid: u32) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };
    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        #[allow(clippy::arithmetic_side_effects)]
        { group.usage.open_files += 1; }
    }
}

/// Report file handle close in a group.
pub fn report_file_closed(pid: u32) {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return,
    };
    if let Some(group) = state.groups.iter_mut().find(|g| g.id == gid) {
        group.usage.open_files = group.usage.open_files.saturating_sub(1);
    }
}

// ---------------------------------------------------------------------------
// Limit Checking (called before resource allocation)
// ---------------------------------------------------------------------------

/// Check whether a memory allocation is allowed for a process.
///
/// Returns `LimitCheck::Ok` if within limits, `SoftExceeded` if soft limit
/// hit (allocation proceeds with warning), or `HardExceeded` if the
/// allocation must be denied.
pub fn check_memory(pid: u32, additional_bytes: u64) -> LimitCheck {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return LimitCheck::Ok, // Not in a group — no limits.
    };

    let idx = match state.groups.iter().position(|g| g.id == gid) {
        Some(i) => i,
        None => return LimitCheck::Ok,
    };

    if !state.groups[idx].enforce {
        return LimitCheck::Ok;
    }

    let new_rss = state.groups[idx].usage.rss_bytes.saturating_add(additional_bytes);

    // Check hard RSS limit.
    if state.groups[idx].memory.max_rss != UNLIMITED && new_rss > state.groups[idx].memory.max_rss {
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_hard_violations += 1; }
        return LimitCheck::HardExceeded;
    }

    // Check soft RSS limit.
    if state.groups[idx].memory.soft_rss != UNLIMITED && new_rss > state.groups[idx].memory.soft_rss {
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_soft_warnings += 1; }
        return LimitCheck::SoftExceeded;
    }

    LimitCheck::Ok
}

/// Check whether a new process can be created in a group.
pub fn check_process_limit(pid: u32) -> LimitCheck {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return LimitCheck::Ok,
    };

    let idx = match state.groups.iter().position(|g| g.id == gid) {
        Some(i) => i,
        None => return LimitCheck::Ok,
    };

    if !state.groups[idx].enforce {
        return LimitCheck::Ok;
    }

    let limit = state.groups[idx].process.max_processes;
    if limit != UNLIMITED && state.groups[idx].usage.process_count >= limit {
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_hard_violations += 1; }
        return LimitCheck::HardExceeded;
    }

    LimitCheck::Ok
}

/// Check whether a new thread can be created in a group.
pub fn check_thread_limit(pid: u32) -> LimitCheck {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return LimitCheck::Ok,
    };

    let idx = match state.groups.iter().position(|g| g.id == gid) {
        Some(i) => i,
        None => return LimitCheck::Ok,
    };

    if !state.groups[idx].enforce {
        return LimitCheck::Ok;
    }

    let limit = state.groups[idx].process.max_threads;
    if limit != UNLIMITED && state.groups[idx].usage.thread_count >= limit {
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_hard_violations += 1; }
        return LimitCheck::HardExceeded;
    }

    LimitCheck::Ok
}

/// Check whether a new file handle can be opened in a group.
pub fn check_open_files(pid: u32) -> LimitCheck {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return LimitCheck::Ok,
    };

    let idx = match state.groups.iter().position(|g| g.id == gid) {
        Some(i) => i,
        None => return LimitCheck::Ok,
    };

    if !state.groups[idx].enforce {
        return LimitCheck::Ok;
    }

    let limit = state.groups[idx].process.max_open_files;
    if limit != UNLIMITED && state.groups[idx].usage.open_files >= limit {
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_hard_violations += 1; }
        return LimitCheck::HardExceeded;
    }

    LimitCheck::Ok
}

/// Check whether an I/O read operation is within rate limits.
pub fn check_io_read(pid: u32, bytes: u64) -> LimitCheck {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return LimitCheck::Ok,
    };

    let idx = match state.groups.iter().position(|g| g.id == gid) {
        Some(i) => i,
        None => return LimitCheck::Ok,
    };

    if !state.groups[idx].enforce {
        return LimitCheck::Ok;
    }

    // Check bandwidth limit.
    let bps_limit = state.groups[idx].io.max_read_bps;
    if bps_limit != UNLIMITED {
        let new_bps = state.groups[idx].usage.io_read_bps_current.saturating_add(bytes);
        if new_bps > bps_limit {
            #[allow(clippy::arithmetic_side_effects)]
            { state.total_hard_violations += 1; }
            return LimitCheck::HardExceeded;
        }
    }

    // Check IOPS limit.
    let iops_limit = state.groups[idx].io.max_read_iops;
    if iops_limit != UNLIMITED
        && state.groups[idx].usage.io_read_iops_current >= iops_limit
    {
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_hard_violations += 1; }
        return LimitCheck::HardExceeded;
    }

    LimitCheck::Ok
}

/// Check whether an I/O write operation is within rate limits.
pub fn check_io_write(pid: u32, bytes: u64) -> LimitCheck {
    let mut state = STATE.lock();
    let gid = match state.assignments.iter().find(|&&(p, _)| p == pid) {
        Some(&(_, gid)) => gid,
        None => return LimitCheck::Ok,
    };

    let idx = match state.groups.iter().position(|g| g.id == gid) {
        Some(i) => i,
        None => return LimitCheck::Ok,
    };

    if !state.groups[idx].enforce {
        return LimitCheck::Ok;
    }

    let bps_limit = state.groups[idx].io.max_write_bps;
    if bps_limit != UNLIMITED {
        let new_bps = state.groups[idx].usage.io_write_bps_current.saturating_add(bytes);
        if new_bps > bps_limit {
            #[allow(clippy::arithmetic_side_effects)]
            { state.total_hard_violations += 1; }
            return LimitCheck::HardExceeded;
        }
    }

    let iops_limit = state.groups[idx].io.max_write_iops;
    if iops_limit != UNLIMITED
        && state.groups[idx].usage.io_write_iops_current >= iops_limit
    {
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_hard_violations += 1; }
        return LimitCheck::HardExceeded;
    }

    LimitCheck::Ok
}

// ---------------------------------------------------------------------------
// Group Query
// ---------------------------------------------------------------------------

/// Get a snapshot of a group's limits and usage.
pub fn get_group(group_id: u32) -> KernelResult<ResourceGroup> {
    let state = STATE.lock();
    state.groups.iter().find(|g| g.id == group_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// Get all groups that are children of the given parent.
pub fn children_of(parent_id: u32) -> Vec<(u32, String)> {
    let state = STATE.lock();
    state.groups.iter()
        .filter(|g| g.parent_id == parent_id)
        .map(|g| (g.id, g.name.clone()))
        .collect()
}

/// Get the effective memory limit for a group, considering parent hierarchy.
///
/// The effective limit is the minimum of the group's own limit and all
/// ancestor limits. This ensures a child cannot exceed its parent's cap.
pub fn effective_memory_limit(group_id: u32) -> u64 {
    let state = STATE.lock();
    let mut limit = UNLIMITED;
    let mut current_id = group_id;

    // Walk up the hierarchy.
    for _ in 0..16 { // Max depth guard to prevent infinite loops.
        match state.groups.iter().find(|g| g.id == current_id) {
            Some(group) => {
                if group.memory.max_rss != UNLIMITED {
                    if group.memory.max_rss < limit {
                        limit = group.memory.max_rss;
                    }
                }
                if group.parent_id == 0 || group.parent_id == current_id {
                    break;
                }
                current_id = group.parent_id;
            }
            None => break,
        }
    }

    limit
}

/// Get the effective process limit for a group, considering parent hierarchy.
pub fn effective_process_limit(group_id: u32) -> u64 {
    let state = STATE.lock();
    let mut limit = UNLIMITED;
    let mut current_id = group_id;

    for _ in 0..16 {
        match state.groups.iter().find(|g| g.id == current_id) {
            Some(group) => {
                if group.process.max_processes != UNLIMITED {
                    if group.process.max_processes < limit {
                        limit = group.process.max_processes;
                    }
                }
                if group.parent_id == 0 || group.parent_id == current_id {
                    break;
                }
                current_id = group.parent_id;
            }
            None => break,
        }
    }

    limit
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate content for `/proc/reslimit`.
pub fn procfs_content() -> String {
    let state = STATE.lock();

    let mut out = String::from("=== Resource Limits ===\n\n");

    out.push_str(&format!("Groups: {}\n", state.groups.len()));
    out.push_str(&format!("Process assignments: {}\n", state.assignments.len()));
    out.push_str(&format!("Hard violations: {}\n", state.total_hard_violations));
    out.push_str(&format!("Soft warnings: {}\n\n", state.total_soft_warnings));

    for group in &state.groups {
        out.push_str(&format!("--- Group '{}' (id={}, parent={}) ---\n",
            group.name, group.id, group.parent_id));
        out.push_str(&format!("  Active: {}, Members: {}, Enforce: {}\n",
            group.active, group.member_count, group.enforce));

        // CPU
        out.push_str("  CPU: ");
        if group.cpu.max_cpu_percent == 0 {
            out.push_str("unlimited");
        } else {
            out.push_str(&format!("max {}%", group.cpu.max_cpu_percent));
        }
        out.push_str(&format!(", weight={}", group.cpu.weight));
        if group.cpu.affinity_mask != 0 {
            out.push_str(&format!(", affinity=0x{:x}", group.cpu.affinity_mask));
        }
        out.push('\n');
        out.push_str(&format!("    Used: {:.2}ms total\n",
            group.usage.cpu_time_ns as f64 / 1_000_000.0));

        // Memory
        out.push_str("  Memory: RSS ");
        if group.memory.max_rss == UNLIMITED {
            out.push_str("unlimited");
        } else {
            out.push_str(&format_bytes(group.memory.max_rss));
        }
        out.push_str(", Virtual ");
        if group.memory.max_virtual == UNLIMITED {
            out.push_str("unlimited");
        } else {
            out.push_str(&format_bytes(group.memory.max_virtual));
        }
        out.push('\n');
        out.push_str(&format!("    Used: RSS {}, Virtual {}, Peak RSS {}\n",
            format_bytes(group.usage.rss_bytes),
            format_bytes(group.usage.virtual_bytes),
            format_bytes(group.usage.peak_rss_bytes)));

        // I/O
        out.push_str("  I/O: ");
        if group.io.max_read_bps == UNLIMITED {
            out.push_str("read unlimited");
        } else {
            out.push_str(&format!("read {}/s", format_bytes(group.io.max_read_bps)));
        }
        out.push_str(", ");
        if group.io.max_write_bps == UNLIMITED {
            out.push_str("write unlimited");
        } else {
            out.push_str(&format!("write {}/s", format_bytes(group.io.max_write_bps)));
        }
        if group.io.low_priority {
            out.push_str(" [LOW PRIORITY]");
        }
        out.push('\n');
        out.push_str(&format!("    Total: read {}, write {}, {} read ops, {} write ops\n",
            format_bytes(group.usage.io_read_bytes),
            format_bytes(group.usage.io_write_bytes),
            group.usage.io_read_ops,
            group.usage.io_write_ops));

        // Process/Thread/Files
        out.push_str(&format!("  Processes: {}/{}, Threads: {}/{}, Files: {}/{}\n\n",
            group.usage.process_count, fmt_limit(group.process.max_processes),
            group.usage.thread_count, fmt_limit(group.process.max_threads),
            group.usage.open_files, fmt_limit(group.process.max_open_files)));
    }

    out
}

/// Format a byte count as human-readable.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a limit value (UNLIMITED → "∞").
fn fmt_limit(val: u64) -> String {
    if val == UNLIMITED {
        String::from("unlimited")
    } else {
        format!("{}", val)
    }
}

// ---------------------------------------------------------------------------
// Self-Tests
// ---------------------------------------------------------------------------

/// Run self-tests for the resource limits subsystem.
pub fn self_test() -> bool {
    crate::serial_println!("[reslimit] Running self-tests...");
    let mut passed = 0u32;
    let mut failed = 0u32;

    macro_rules! check {
        ($name:expr, $cond:expr) => {
            if $cond {
                crate::serial_println!("  [PASS] {}", $name);
                #[allow(clippy::arithmetic_side_effects)]
                { passed += 1; }
            } else {
                crate::serial_println!("  [FAIL] {}", $name);
                #[allow(clippy::arithmetic_side_effects)]
                { failed += 1; }
            }
        };
    }

    // Reset state for testing.
    {
        let mut state = STATE.lock();
        *state = State::new();
    }

    // Test 1: Init creates root group.
    init();
    {
        let state = STATE.lock();
        check!("init creates root group", state.groups.len() == 1);
        check!("root group has id 1", state.groups[0].id == 1);
        check!("root group is active", state.groups[0].active);
    }

    // Test 2: Create child group.
    let gid = create_group("test-service", 0);
    check!("create group succeeds", gid.is_ok());
    let gid = gid.unwrap_or(0);
    check!("created group has id 2", gid == 2);

    // Test 3: Duplicate name rejected.
    let dup = create_group("test-service", 0);
    check!("duplicate name rejected", dup.is_err());

    // Test 4: Set memory limits.
    let mem = MemoryLimits {
        max_rss: 100 * 1024 * 1024, // 100 MiB
        max_virtual: UNLIMITED,
        max_kernel_memory: UNLIMITED,
        soft_rss: 80 * 1024 * 1024, // 80 MiB
        oom_kill: true,
        oom_score_adj: 0,
    };
    let r = set_memory_limits(gid, mem);
    check!("set memory limits succeeds", r.is_ok());

    // Test 5: Invalid soft > hard memory rejected.
    let bad_mem = MemoryLimits {
        max_rss: 50 * 1024 * 1024,
        max_virtual: UNLIMITED,
        max_kernel_memory: UNLIMITED,
        soft_rss: 100 * 1024 * 1024, // soft > hard
        oom_kill: true,
        oom_score_adj: 0,
    };
    let r = set_memory_limits(gid, bad_mem);
    check!("soft > hard memory rejected", r.is_err());

    // Test 6: Assign process to group.
    let r = assign_process(100, gid);
    check!("assign process succeeds", r.is_ok());

    let found = group_for_process(100);
    check!("process found in group", found == Some(gid));

    // Test 7: Memory limit check.
    // Group has 100 MiB hard limit, 80 MiB soft limit.
    // Report 70 MiB RSS.
    report_memory_change(100, 70 * 1024 * 1024, 0);
    let chk = check_memory(100, 5 * 1024 * 1024); // 75 MiB total = under soft
    check!("memory check: under soft limit = Ok", chk == LimitCheck::Ok);

    // Report more to cross soft limit.
    report_memory_change(100, 15 * 1024 * 1024, 0); // Now 85 MiB
    let chk = check_memory(100, 5 * 1024 * 1024); // 90 MiB total = over soft, under hard
    check!("memory check: over soft limit = SoftExceeded", chk == LimitCheck::SoftExceeded);

    // Try to allocate past hard limit.
    let chk = check_memory(100, 20 * 1024 * 1024); // 105 MiB = over hard
    check!("memory check: over hard limit = HardExceeded", chk == LimitCheck::HardExceeded);

    // Test 8: Process limits.
    let proc_lim = ProcessLimits {
        max_processes: 2,
        max_threads: 4,
        max_open_files: 10,
    };
    let _ = set_process_limits(gid, proc_lim);

    // Simulate creating 2 processes.
    report_process_created(100);
    report_process_created(100);
    let chk = check_process_limit(100);
    check!("process limit at cap = HardExceeded", chk == LimitCheck::HardExceeded);

    // Free one.
    report_process_exited(100);
    let chk = check_process_limit(100);
    check!("process limit after exit = Ok", chk == LimitCheck::Ok);

    // Test 9: I/O rate limits.
    let io_lim = IoLimits {
        max_read_bps: 1_000_000, // 1 MB/s
        max_write_bps: 500_000,  // 500 KB/s
        max_read_iops: UNLIMITED,
        max_write_iops: UNLIMITED,
        weight: 100,
        low_priority: false,
    };
    let _ = set_io_limits(gid, io_lim);

    // Under limit.
    let chk = check_io_read(100, 500_000);
    check!("I/O read under limit = Ok", chk == LimitCheck::Ok);

    // Report usage, then check again.
    report_io(100, 800_000, 0);
    let chk = check_io_read(100, 300_000); // 800K + 300K > 1M
    check!("I/O read over limit = HardExceeded", chk == LimitCheck::HardExceeded);

    // Test 10: Unassign process.
    let r = unassign_process(100);
    check!("unassign process succeeds", r.is_ok());
    let found = group_for_process(100);
    check!("process no longer in group", found.is_none());

    // Test 11: Remove group (empty).
    let r = remove_group(gid);
    check!("remove empty group succeeds", r.is_ok());

    // Test 12: Cannot remove root.
    let r = remove_group(1);
    check!("cannot remove root group", r.is_err());

    // Test 13: Hierarchical limits.
    let parent = create_group("parent-svc", 0).unwrap_or(0);
    let child = create_group("child-worker", parent).unwrap_or(0);

    let parent_mem = MemoryLimits {
        max_rss: 200 * 1024 * 1024,
        ..MemoryLimits::default()
    };
    let _ = set_memory_limits(parent, parent_mem);

    let child_mem = MemoryLimits {
        max_rss: 500 * 1024 * 1024, // Child wants 500 MiB but parent caps at 200 MiB.
        ..MemoryLimits::default()
    };
    let _ = set_memory_limits(child, child_mem);

    let eff = effective_memory_limit(child);
    check!("effective limit = min(child, parent) = 200 MiB",
        eff == 200 * 1024 * 1024);

    // Test 14: Enforcement toggle.
    let _ = assign_process(200, parent);
    report_memory_change(200, 300 * 1024 * 1024, 0); // Exceed 200 MiB limit.
    let chk = check_memory(200, 1);
    check!("enforcement on: exceeds limit", chk == LimitCheck::HardExceeded);

    let _ = set_enforce(parent, false);
    let chk = check_memory(200, 1);
    check!("enforcement off: always Ok", chk == LimitCheck::Ok);

    // Test 15: Cannot remove group with members.
    let r = remove_group(parent);
    check!("cannot remove group with members", r.is_err());

    // Test 16: Procfs output is non-empty.
    let content = procfs_content();
    check!("procfs content is non-empty", content.len() > 50);

    crate::serial_println!("[reslimit] Tests complete: {} passed, {} failed", passed, failed);
    failed == 0
}
