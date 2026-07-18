//! Cgroup FS — cgroup v2 resource limit management.
//!
//! Manages hierarchical resource groups (cgroups): CPU weight,
//! memory limits, I/O bandwidth, PID limits. Supports nested
//! groups and per-group statistics.
//!
//! ## Architecture
//!
//! ```text
//! Cgroup management
//!   → cgroupfs::create(path) → create cgroup
//!   → cgroupfs::set_limit(path, resource, limit) → set limit
//!   → cgroupfs::add_pid(path, pid) → assign process
//!   → cgroupfs::stats(path) → group statistics
//!
//! Integration:
//!   → oomkiller (OOM killer)
//!   → schedtune (scheduler tuning)
//!   → iosched (I/O scheduler)
//!   → sysresource (resource monitoring)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Resource controller type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controller {
    Cpu,
    Memory,
    Io,
    Pids,
    Cpuset,
}

impl Controller {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Io => "io",
            Self::Pids => "pids",
            Self::Cpuset => "cpuset",
        }
    }
}

/// A cgroup entry.
#[derive(Debug, Clone)]
pub struct Cgroup {
    pub path: String,
    pub cpu_weight: u32,          // 1..10000, default 100.
    pub cpu_max_us: u64,          // Max CPU time per period (0 = unlimited).
    pub memory_max: u64,          // Max memory bytes (0 = unlimited).
    pub memory_current: u64,      // Current memory usage.
    pub io_weight: u32,           // 1..10000, default 100.
    pub pids_max: u32,            // Max PIDs (0 = unlimited).
    pub pids_current: u32,        // Current PID count.
    pub processes: Vec<u32>,      // Assigned PIDs.
    pub created_ns: u64,
    /// Backing in-kernel resource controller group
    /// (`crate::cgroup::CgroupId`).
    ///
    /// This is the enforcement engine (Q14 / design-decisions §39): the
    /// cgroupfs entry is the cgroup-v2 *frontend*, and writes to it are
    /// pushed through to this kernel group — `memory_max` →
    /// `cgroup::set_mem_limit`, process assignment → `set_task_cgroup`.
    /// The root group "/" maps to `cgroup::ROOT_CGROUP` (0).
    pub kernel_id: crate::cgroup::CgroupId,
}

/// Bytes per physical frame (16 KiB).  Memory limits in cgroup-v2 are
/// expressed in bytes; the in-kernel memory controller charges in whole
/// frames, so byte limits are converted by rounding **up** to the next
/// frame (a limit of 1 byte still permits one frame).
const FRAME_BYTES: u64 = 16 * 1024;

/// Derive the parent path of a cgroup path.
///
/// `"/app/sub"` → `"/app"`, `"/app"` → `"/"`, `"/"` → `"/"`.  Used to
/// locate the parent's kernel cgroup when creating a child so the kernel
/// hierarchy mirrors the cgroupfs path hierarchy (hierarchical limits).
fn parent_path(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/",
        Some(idx) => &trimmed[..idx],
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CGROUPS: usize = 128;

struct State {
    groups: Vec<Cgroup>,
    total_created: u64,
    total_deleted: u64,
    total_limit_changes: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        groups: alloc::vec![
            Cgroup {
                path: String::from("/"), cpu_weight: 100, cpu_max_us: 0,
                memory_max: 0, memory_current: 0, io_weight: 100,
                pids_max: 0, pids_current: 0, processes: Vec::new(), created_ns: now,
                // The cgroupfs root mirrors the kernel root cgroup.
                kernel_id: crate::cgroup::ROOT_CGROUP,
            },
        ],
        total_created: 1,
        total_deleted: 0,
        total_limit_changes: 0,
        ops: 0,
    });
}

/// List all cgroups.
pub fn list_groups() -> Vec<Cgroup> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.groups.clone())
}

/// Get cgroup by path.
pub fn get_group(path: &str) -> Option<Cgroup> {
    STATE.lock().as_ref().and_then(|s| s.groups.iter().find(|g| g.path == path).cloned())
}

/// Create a cgroup.
pub fn create_group(path: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.groups.len() >= MAX_CGROUPS { return Err(KernelError::ResourceExhausted); }
        if state.groups.iter().any(|g| g.path == path) { return Err(KernelError::AlreadyExists); }

        // Create the backing kernel cgroup under the parent's kernel
        // group so the kernel hierarchy mirrors the cgroupfs path tree
        // (hierarchical limits walk up the kernel parent chain).  An
        // unknown parent path falls back to the kernel root.
        let parent_kernel_id = {
            let pp = parent_path(path);
            state.groups.iter().find(|g| g.path == pp)
                .map_or(crate::cgroup::ROOT_CGROUP, |g| g.kernel_id)
        };
        let kernel_id = crate::cgroup::create(parent_kernel_id)?;

        let now = crate::hpet::elapsed_ns();
        state.groups.push(Cgroup {
            path: String::from(path), cpu_weight: 100, cpu_max_us: 0,
            memory_max: 0, memory_current: 0, io_weight: 100,
            pids_max: 0, pids_current: 0, processes: Vec::new(), created_ns: now,
            kernel_id,
        });
        state.total_created += 1;
        Ok(())
    })
}

/// Delete a cgroup.
pub fn delete_group(path: &str) -> KernelResult<()> {
    with_state(|state| {
        if path == "/" { return Err(KernelError::PermissionDenied); }
        // Capture the backing kernel group before removing the frontend
        // entry so we can release it too.
        let kernel_id = state.groups.iter()
            .find(|g| g.path == path)
            .map(|g| g.kernel_id);
        let before = state.groups.len();
        state.groups.retain(|g| g.path != path);
        if state.groups.len() == before { return Err(KernelError::NotFound); }

        // Release the backing kernel cgroup.  This is best-effort: the
        // kernel controller refuses to delete a group that still has
        // tasks or child groups, whereas the cgroupfs frontend has
        // looser semantics.  A failure here only leaks one of the 256
        // kernel cgroup slots until reboot, never a correctness hazard,
        // so we don't fail the frontend delete on it.
        if let Some(id) = kernel_id {
            let _ = crate::cgroup::delete(id);
        }
        state.total_deleted += 1;
        Ok(())
    })
}

/// Set CPU weight.
pub fn set_cpu_weight(path: &str, weight: u32) -> KernelResult<()> {
    with_state(|state| {
        if weight == 0 || weight > 10000 { return Err(KernelError::InvalidArgument); }
        let g = state.groups.iter_mut().find(|g| g.path == path).ok_or(KernelError::NotFound)?;
        g.cpu_weight = weight;
        state.total_limit_changes += 1;
        Ok(())
    })
}

/// Set memory limit.
pub fn set_memory_max(path: &str, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path).ok_or(KernelError::NotFound)?;
        g.memory_max = bytes;
        let kernel_id = g.kernel_id;
        state.total_limit_changes += 1;

        // Push the limit through to the enforcement engine.  cgroup-v2
        // expresses the limit in bytes; the kernel memory controller
        // charges whole 16 KiB frames, so round up.  `bytes == 0` means
        // "unlimited" in both models → 0 frames.
        let frames = if bytes == 0 { 0 } else { bytes.div_ceil(FRAME_BYTES) };
        crate::cgroup::set_mem_limit(kernel_id, crate::cgroup::MemLimit::frames(frames))?;
        Ok(())
    })
}

/// Set PID limit.
pub fn set_pids_max(path: &str, max: u32) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path).ok_or(KernelError::NotFound)?;
        g.pids_max = max;
        state.total_limit_changes += 1;
        Ok(())
    })
}

/// Add a PID to a cgroup.
pub fn add_pid(path: &str, pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path).ok_or(KernelError::NotFound)?;
        if g.pids_max > 0 && g.pids_current >= g.pids_max {
            return Err(KernelError::ResourceExhausted);
        }
        if g.processes.contains(&pid) { return Err(KernelError::AlreadyExists); }
        let kernel_id = g.kernel_id;
        g.processes.push(pid);
        g.pids_current += 1;

        // Assign the task to the backing kernel cgroup so its memory and
        // CPU usage are billed to (and limited by) this group — the
        // `cgroup.procs` → enforcement wiring.  Best-effort: a PID with
        // no live scheduler task (e.g. the synthetic PIDs used by the
        // self-test, or a process that just exited) yields
        // `InvalidArgument` from `set_task_cgroup`, which is not a
        // failure of the frontend assignment — the cgroupfs bookkeeping
        // still records the intended membership.
        let _ = crate::sched::set_task_cgroup(u64::from(pid), kernel_id);
        Ok(())
    })
}

/// Remove a PID from a cgroup.
pub fn remove_pid(path: &str, pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path).ok_or(KernelError::NotFound)?;
        let before = g.processes.len();
        g.processes.retain(|&p| p != pid);
        if g.processes.len() == before { return Err(KernelError::NotFound); }
        g.pids_current -= 1;

        // Return the task to the root cgroup (symmetric with `add_pid`).
        // Best-effort for the same reason: a synthetic or already-exited
        // PID has no task to move.
        let _ = crate::sched::set_task_cgroup(u64::from(pid), crate::cgroup::ROOT_CGROUP);
        Ok(())
    })
}

/// Statistics: (group_count, total_created, total_deleted, total_limit_changes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.groups.len(), s.total_created, s.total_deleted, s.total_limit_changes, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cgroupfs::self_test() — running tests...");
    init_defaults();

    // 1: Default root.
    assert_eq!(list_groups().len(), 1);
    let root = get_group("/").expect("root");
    assert_eq!(root.cpu_weight, 100);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create group.
    create_group("/app").expect("create");
    assert_eq!(list_groups().len(), 2);
    assert!(create_group("/app").is_err());
    crate::serial_println!("  [2/8] create: OK");

    // 3: Set CPU weight.
    set_cpu_weight("/app", 200).expect("cpu");
    let g = get_group("/app").expect("get");
    assert_eq!(g.cpu_weight, 200);
    assert!(set_cpu_weight("/app", 0).is_err());
    crate::serial_println!("  [3/8] cpu weight: OK");

    // 4: Set memory limit.
    set_memory_max("/app", 1_073_741_824).expect("mem");
    let g = get_group("/app").expect("get2");
    assert_eq!(g.memory_max, 1_073_741_824);
    crate::serial_println!("  [4/8] memory: OK");

    // 5: Add PIDs.
    add_pid("/app", 100).expect("add1");
    add_pid("/app", 200).expect("add2");
    let g = get_group("/app").expect("get3");
    assert_eq!(g.pids_current, 2);
    assert!(add_pid("/app", 100).is_err());
    crate::serial_println!("  [5/8] add pid: OK");

    // 6: PID limit.
    set_pids_max("/app", 2).expect("limit");
    assert!(add_pid("/app", 300).is_err()); // At limit.
    crate::serial_println!("  [6/8] pid limit: OK");

    // 7: Remove PID and delete.
    remove_pid("/app", 100).expect("rmpid");
    delete_group("/app").expect("delete");
    assert_eq!(list_groups().len(), 1);
    assert!(delete_group("/").is_err()); // Can't delete root.
    crate::serial_println!("  [7/8] delete: OK");

    // 8: Stats.
    let (count, created, deleted, changes, ops) = stats();
    assert_eq!(count, 1);
    assert!(created >= 2);
    assert!(deleted >= 1);
    assert!(changes >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cgroupfs::self_test() — all 8 tests passed");
}
