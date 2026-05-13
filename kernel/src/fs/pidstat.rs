//! PID Statistics — PID allocation and namespace monitoring.
//!
//! Tracks PID allocation patterns, PID namespace hierarchy,
//! PID reuse rates, and maximum PID watermark. Essential
//! for process management diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! PID monitoring
//!   → pidstat::alloc_pid(ns) → track PID allocation
//!   → pidstat::free_pid(ns) → track PID release
//!   → pidstat::create_ns(parent) → new PID namespace
//!   → pidstat::ns_info() → namespace hierarchy
//!
//! Integration:
//!   → procstat (process stats)
//!   → taskstats (per-task accounting)
//!   → prociso (process isolation)
//!   → cgroupfs (cgroup management)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// PID namespace info.
#[derive(Debug, Clone)]
pub struct PidNamespace {
    pub ns_id: u32,
    pub parent_id: Option<u32>,
    pub active_pids: u64,
    pub max_pid: u32,
    pub allocated: u64,
    pub freed: u64,
    pub high_watermark: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_NAMESPACES: usize = 64;

struct State {
    namespaces: Vec<PidNamespace>,
    next_ns_id: u32,
    total_allocated: u64,
    total_freed: u64,
    total_reuses: u64,
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
        namespaces: alloc::vec![
            PidNamespace { ns_id: 0, parent_id: None, active_pids: 150, max_pid: 32768, allocated: 500_000, freed: 499_850, high_watermark: 2048 },
        ],
        next_ns_id: 1,
        total_allocated: 500_000,
        total_freed: 499_850,
        total_reuses: 450_000,
        ops: 0,
    });
}

/// Allocate a PID in a namespace.
pub fn alloc_pid(ns_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let ns = state.namespaces.iter_mut().find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        ns.allocated += 1;
        ns.active_pids += 1;
        let current = ns.active_pids as u32;
        if current > ns.high_watermark {
            ns.high_watermark = current;
        }
        state.total_allocated += 1;
        // Reuse detection: if freed > 0, we're likely reusing PIDs.
        if ns.freed > 0 { state.total_reuses += 1; }
        Ok(())
    })
}

/// Free a PID in a namespace.
pub fn free_pid(ns_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let ns = state.namespaces.iter_mut().find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        ns.freed += 1;
        ns.active_pids = ns.active_pids.saturating_sub(1);
        state.total_freed += 1;
        Ok(())
    })
}

/// Create a child PID namespace.
pub fn create_ns(parent_id: u32, max_pid: u32) -> KernelResult<u32> {
    with_state(|state| {
        if !state.namespaces.iter().any(|n| n.ns_id == parent_id) {
            return Err(KernelError::NotFound);
        }
        if state.namespaces.len() >= MAX_NAMESPACES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_ns_id;
        state.next_ns_id += 1;
        state.namespaces.push(PidNamespace {
            ns_id: id, parent_id: Some(parent_id), active_pids: 0,
            max_pid, allocated: 0, freed: 0, high_watermark: 0,
        });
        Ok(id)
    })
}

/// List all namespaces.
pub fn ns_list() -> Vec<PidNamespace> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.namespaces.clone())
}

/// Get a specific namespace.
pub fn ns_info(ns_id: u32) -> Option<PidNamespace> {
    STATE.lock().as_ref().and_then(|s| {
        s.namespaces.iter().find(|n| n.ns_id == ns_id).cloned()
    })
}

/// Statistics: (ns_count, total_allocated, total_freed, total_reuses, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.namespaces.len(), s.total_allocated, s.total_freed, s.total_reuses, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pidstat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(ns_list().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Alloc PID.
    let before = ns_info(0).unwrap().active_pids;
    alloc_pid(0).expect("alloc");
    let after = ns_info(0).unwrap().active_pids;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Free PID.
    free_pid(0).expect("free");
    let after2 = ns_info(0).unwrap().active_pids;
    assert_eq!(after2, before);
    crate::serial_println!("  [3/8] free: OK");

    // 4: Create namespace.
    let child_id = create_ns(0, 4096).expect("create_ns");
    assert!(child_id >= 1);
    assert_eq!(ns_list().len(), 2);
    crate::serial_println!("  [4/8] create ns: OK");

    // 5: Child namespace ops.
    alloc_pid(child_id).expect("child_alloc");
    let child = ns_info(child_id).unwrap();
    assert_eq!(child.active_pids, 1);
    assert_eq!(child.parent_id, Some(0));
    crate::serial_println!("  [5/8] child ns: OK");

    // 6: High watermark.
    for _ in 0..5 { alloc_pid(child_id).expect("alloc_multi"); }
    let child = ns_info(child_id).unwrap();
    assert_eq!(child.high_watermark, 6);
    crate::serial_println!("  [6/8] watermark: OK");

    // 7: Not found.
    assert!(alloc_pid(999).is_err());
    assert!(create_ns(999, 100).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (nss, alloc, freed, reuses, ops) = stats();
    assert!(nss >= 2);
    assert!(alloc > 500_000);
    assert!(freed > 499_850);
    assert!(reuses > 450_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("pidstat::self_test() — all 8 tests passed");
}
