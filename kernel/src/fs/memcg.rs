//! Memory Cgroup — per-cgroup memory accounting.
//!
//! Tracks memory usage, limits, and reclaim statistics per
//! cgroup. Supports hard and soft limits, swap accounting,
//! and OOM notifications.
//!
//! ## Architecture
//!
//! ```text
//! Memory cgroup accounting
//!   → memcg::charge(cgroup, bytes) → charge memory
//!   → memcg::uncharge(cgroup, bytes) → release memory
//!   → memcg::get(cgroup) → current usage
//!   → memcg::set_limit(cgroup, limit) → set memory limit
//!
//! Integration:
//!   → cgroupfs (cgroup management)
//!   → oomkiller (OOM killer)
//!   → memlayout (memory layout)
//!   → procstat (process statistics)
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

/// Memory cgroup entry.
#[derive(Debug, Clone)]
pub struct MemCgroup {
    pub path: String,
    pub usage_bytes: u64,
    pub limit_bytes: u64,       // 0 = unlimited.
    pub soft_limit_bytes: u64,  // 0 = no soft limit.
    pub swap_usage: u64,
    pub swap_limit: u64,
    pub max_usage: u64,         // High watermark.
    pub failcnt: u64,           // Allocation failures due to limit.
    pub oom_kills: u64,
    pub charge_count: u64,
    pub uncharge_count: u64,
    pub processes: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CGROUPS: usize = 128;

struct State {
    groups: Vec<MemCgroup>,
    total_charges: u64,
    total_uncharges: u64,
    total_failures: u64,
    total_oom: u64,
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

/// Initialise the memory-cgroup accounting state.
///
/// Starts with no cgroups and all charge/uncharge/failure/OOM totals at
/// zero. The `/proc/memcg` generator and the `memcg` kshell command
/// surface this table as if it reflects real per-cgroup memory usage, so
/// seeding it with invented cgroups and usage figures would be fabricated
/// procfs data. The cgroup hierarchy is built at runtime through
/// [`create`] by the cgroupfs subsystem, and usage is accounted only
/// through real [`charge`] / [`uncharge`] calls.
///
/// (Previously this seeded three fictional cgroups — "/" 2GiB usage / 3GB
/// max / 500k charges; "/system" 512MiB usage / 1GiB limit / 100k
/// charges; "/user" 1GiB usage / 4GiB limit / 128MiB swap / 300k charges /
/// 2 failcnt — plus invented totals (900k charges, 865k uncharges, 2
/// failures).)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        groups: Vec::new(),
        total_charges: 0,
        total_uncharges: 0,
        total_failures: 0,
        total_oom: 0,
        ops: 0,
    });
}

/// Charge memory to a cgroup.
pub fn charge(path: &str, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path)
            .ok_or(KernelError::NotFound)?;
        let new_usage = g.usage_bytes.saturating_add(bytes);
        if g.limit_bytes > 0 && new_usage > g.limit_bytes {
            g.failcnt += 1;
            state.total_failures += 1;
            return Err(KernelError::OutOfMemory);
        }
        g.usage_bytes = new_usage;
        if g.usage_bytes > g.max_usage { g.max_usage = g.usage_bytes; }
        g.charge_count += 1;
        state.total_charges += 1;
        Ok(())
    })
}

/// Uncharge memory from a cgroup.
pub fn uncharge(path: &str, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path)
            .ok_or(KernelError::NotFound)?;
        g.usage_bytes = g.usage_bytes.saturating_sub(bytes);
        g.uncharge_count += 1;
        state.total_uncharges += 1;
        Ok(())
    })
}

/// Set memory limit.
pub fn set_limit(path: &str, limit: u64) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path)
            .ok_or(KernelError::NotFound)?;
        g.limit_bytes = limit;
        Ok(())
    })
}

/// Set soft limit.
pub fn set_soft_limit(path: &str, limit: u64) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path)
            .ok_or(KernelError::NotFound)?;
        g.soft_limit_bytes = limit;
        Ok(())
    })
}

/// Record an OOM kill in a cgroup.
pub fn record_oom(path: &str) -> KernelResult<()> {
    with_state(|state| {
        let g = state.groups.iter_mut().find(|g| g.path == path)
            .ok_or(KernelError::NotFound)?;
        g.oom_kills += 1;
        state.total_oom += 1;
        Ok(())
    })
}

/// Create a memory cgroup.
pub fn create(path: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.groups.len() >= MAX_CGROUPS { return Err(KernelError::ResourceExhausted); }
        if state.groups.iter().any(|g| g.path == path) { return Err(KernelError::AlreadyExists); }
        state.groups.push(MemCgroup {
            path: String::from(path), usage_bytes: 0, limit_bytes: 0,
            soft_limit_bytes: 0, swap_usage: 0, swap_limit: 0,
            max_usage: 0, failcnt: 0, oom_kills: 0,
            charge_count: 0, uncharge_count: 0, processes: 0,
        });
        Ok(())
    })
}

/// Get cgroup info.
pub fn get(path: &str) -> Option<MemCgroup> {
    STATE.lock().as_ref().and_then(|s| s.groups.iter().find(|g| g.path == path).cloned())
}

/// List all cgroups.
pub fn list() -> Vec<MemCgroup> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.groups.clone())
}

/// Check if any cgroup is over soft limit.
pub fn over_soft_limit() -> Vec<MemCgroup> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.groups.iter()
            .filter(|g| g.soft_limit_bytes > 0 && g.usage_bytes > g.soft_limit_bytes)
            .cloned().collect()
    })
}

/// Statistics: (group_count, total_charges, total_uncharges, total_failures, total_oom, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.groups.len(), s.total_charges, s.total_uncharges, s.total_failures, s.total_oom, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("memcg::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live cgroup table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated cgroups, all totals zero.
    assert_eq!(list().len(), 0);
    let (count0, charges0, uncharges0, failures0, oom0, _) = stats();
    assert_eq!((count0, charges0, uncharges0, failures0, oom0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Create cgroups; charging an unknown cgroup errors.
    create("/system").expect("create system");
    assert!(create("/system").is_err()); // duplicate
    assert!(charge("/unknown", 1).is_err()); // NotFound
    assert_eq!(get("/system").expect("get").usage_bytes, 0);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Charge accrues usage, max-usage watermark, and counts.
    charge("/system", 4096).expect("charge");
    let g = get("/system").expect("get2");
    assert_eq!(g.usage_bytes, 4096);
    assert_eq!(g.max_usage, 4096);
    assert_eq!(g.charge_count, 1);
    crate::serial_println!("  [3/8] charge: OK");

    // 4: Uncharge drops usage but leaves the max-usage watermark intact.
    uncharge("/system", 4096).expect("uncharge");
    let g = get("/system").expect("get3");
    assert_eq!(g.usage_bytes, 0);
    assert_eq!(g.max_usage, 4096); // watermark sticks
    assert_eq!(g.uncharge_count, 1);
    crate::serial_println!("  [4/8] uncharge: OK");

    // 5: Hard-limit enforcement bumps failcnt and rejects the charge.
    set_limit("/system", 1000).expect("limit");
    assert!(charge("/system", 2000).is_err()); // over limit
    let g = get("/system").expect("get4");
    assert_eq!(g.usage_bytes, 0); // rejected charge did not apply
    assert_eq!(g.failcnt, 1);
    crate::serial_println!("  [5/8] limit: OK");

    // 6: Soft-limit detection.
    create("/test").expect("create test");
    charge("/test", 100).expect("charge test");
    set_soft_limit("/test", 50).expect("soft");
    let over = over_soft_limit();
    assert!(over.iter().any(|g| g.path == "/test"));
    assert!(!over.iter().any(|g| g.path == "/system")); // no soft limit set
    crate::serial_println!("  [6/8] soft limit: OK");

    // 7: OOM accounting.
    record_oom("/test").expect("oom");
    assert_eq!(get("/test").expect("get5").oom_kills, 1);
    crate::serial_println!("  [7/8] oom: OK");

    // 8: Final stats reflect only the real activity above.
    //    charges: 4096 (test3) + 100 (test6) = 2 successful; the over-limit
    //    charge in test 5 failed and is NOT counted as a charge.
    let (count, charges, uncharges, failures, oom, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(charges, 2);
    assert_eq!(uncharges, 1);
    assert_eq!(failures, 1);
    assert_eq!(oom, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("memcg::self_test() — all 8 tests passed");
}
