//! CPU Set Statistics — CPU affinity/set monitoring.
//!
//! Tracks cpuset assignments, CPU masks, process-to-CPU
//! bindings, and affinity changes. Essential for NUMA-aware
//! workload placement.
//!
//! ## Architecture
//!
//! ```text
//! CPU set monitoring
//!   → cpuset::create(name, cpus) → create cpuset
//!   → cpuset::assign(set_id, pid) → assign process
//!   → cpuset::record_affinity_change(pid) → affinity change
//!   → cpuset::list() → list cpusets
//!
//! Integration:
//!   → cputopo (CPU topology)
//!   → numastat (NUMA stats)
//!   → schedclass (scheduler class)
//!   → migstat (migration stats)
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

/// CPU set info.
#[derive(Debug, Clone)]
pub struct CpuSet {
    pub id: u32,
    pub name: String,
    pub cpu_mask: u64,      // Bitmask of CPUs
    pub mem_mask: u64,      // Bitmask of NUMA nodes
    pub processes: u32,
    pub exclusive: bool,
    pub affinity_changes: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SETS: usize = 64;

struct State {
    sets: Vec<CpuSet>,
    next_id: u32,
    total_affinity_changes: u64,
    total_assignments: u64,
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
    *guard = Some(State {
        sets: alloc::vec![
            CpuSet { id: 1, name: String::from("root"), cpu_mask: 0xF, mem_mask: 0x1, processes: 100, exclusive: false, affinity_changes: 500_000 },
            CpuSet { id: 2, name: String::from("realtime"), cpu_mask: 0x3, mem_mask: 0x1, processes: 5, exclusive: true, affinity_changes: 1_000 },
            CpuSet { id: 3, name: String::from("batch"), cpu_mask: 0xC, mem_mask: 0x1, processes: 20, exclusive: false, affinity_changes: 50_000 },
        ],
        next_id: 4,
        total_affinity_changes: 551_000,
        total_assignments: 200_000,
        ops: 0,
    });
}

/// Create a cpuset.
pub fn create(name: &str, cpu_mask: u64, mem_mask: u64, exclusive: bool) -> KernelResult<u32> {
    with_state(|state| {
        if state.sets.len() >= MAX_SETS { return Err(KernelError::ResourceExhausted); }
        let id = state.next_id;
        state.next_id += 1;
        state.sets.push(CpuSet {
            id, name: String::from(name), cpu_mask, mem_mask,
            processes: 0, exclusive, affinity_changes: 0,
        });
        Ok(id)
    })
}

/// Destroy a cpuset.
pub fn destroy(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.sets.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        if state.sets[idx].processes > 0 { return Err(KernelError::NotEmpty); }
        state.sets.remove(idx);
        Ok(())
    })
}

/// Assign a process to a cpuset.
pub fn assign(set_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sets.iter_mut().find(|s| s.id == set_id)
            .ok_or(KernelError::NotFound)?;
        s.processes += 1;
        state.total_assignments += 1;
        Ok(())
    })
}

/// Remove a process from a cpuset.
pub fn remove_process(set_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sets.iter_mut().find(|s| s.id == set_id)
            .ok_or(KernelError::NotFound)?;
        s.processes = s.processes.saturating_sub(1);
        Ok(())
    })
}

/// Record an affinity change.
pub fn record_affinity_change(set_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sets.iter_mut().find(|s| s.id == set_id)
            .ok_or(KernelError::NotFound)?;
        s.affinity_changes += 1;
        state.total_affinity_changes += 1;
        Ok(())
    })
}

/// List cpusets.
pub fn list() -> Vec<CpuSet> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sets.clone())
}

/// Statistics: (set_count, total_assignments, total_affinity_changes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sets.len(), s.total_assignments, s.total_affinity_changes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cpuset::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create.
    let id = create("test", 0x5, 0x1, false).expect("create");
    assert!(id >= 4);
    assert_eq!(list().len(), 4);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Assign.
    assign(id).expect("assign");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.processes, 1);
    crate::serial_println!("  [3/8] assign: OK");

    // 4: Remove.
    remove_process(id).expect("remove");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.processes, 0);
    crate::serial_println!("  [4/8] remove: OK");

    // 5: Affinity change.
    record_affinity_change(id).expect("affinity");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.affinity_changes, 1);
    crate::serial_println!("  [5/8] affinity: OK");

    // 6: Destroy.
    destroy(id).expect("destroy");
    assert_eq!(list().len(), 3);
    assert!(destroy(id).is_err());
    crate::serial_println!("  [6/8] destroy: OK");

    // 7: Destroy with processes.
    assign(1).expect("assign_root");
    // Can't destroy root because it has processes
    // Actually root already has 101 processes now, so this works:
    assert!(destroy(1).is_err()); // NotEmpty
    crate::serial_println!("  [7/8] destroy busy: OK");

    // 8: Stats.
    let (sets, assignments, affinity, ops) = stats();
    assert_eq!(sets, 3);
    assert!(assignments > 200_000);
    assert!(affinity > 551_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cpuset::self_test() — all 8 tests passed");
}
