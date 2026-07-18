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
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Default PID-space ceiling for the root namespace.  This is a real
/// configuration default (the maximum PID value before wrap), not an observed
/// statistic — matching the classic 32768 ceiling.  The process manager may
/// override it per namespace via [`create_ns`].
const DEFAULT_MAX_PID: u32 = 32768;

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

/// Initialise the PID statistics table with **only** the root PID namespace.
///
/// The root namespace (`ns_id 0`, no parent) is real structure — it always
/// exists — so it is seeded, but with ZEROED activity counters and only the
/// `max_pid` configuration default ([`DEFAULT_MAX_PID`]).  Real PID accounting
/// is wired through `alloc_pid`/`free_pid` (per namespace) and child namespaces
/// via [`create_ns`]; until those are called the counters are genuinely zero,
/// so `/proc/pidstat` and the `pidstat` kshell command report zeros rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: the root namespace previously seeded fabricated OBSERVED counters
/// (active_pids 150, allocated 500k, freed 499_850, high_watermark 2048) plus
/// invented aggregate totals (total_allocated 500k, total_freed 499_850,
/// total_reuses 450k), which `/proc/pidstat` then displayed as if they were
/// real measured PID-allocation activity.  Those numbers were removed; the
/// structural root-namespace row and the `max_pid` config default remain.  The
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The process manager is expected to drive `alloc_pid`/
/// `free_pid` as it creates and reaps processes.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        namespaces: alloc::vec![
            PidNamespace {
                ns_id: 0,
                parent_id: None,
                active_pids: 0,
                max_pid: DEFAULT_MAX_PID,
                allocated: 0,
                freed: 0,
                high_watermark: 0,
            },
        ],
        next_ns_id: 1,
        total_allocated: 0,
        total_freed: 0,
        total_reuses: 0,
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
    // Begin from a clean table (only the structural root namespace, zeroed) and
    // build every fixture via the real API, so the test exercises genuine
    // accounting paths and never relies on fabricated seed data (which
    // /proc/pidstat must never surface).  Resetting first clears any residue
    // from a prior `pidstat test` run so the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: After init only the root namespace exists, with ZEROED counters and the
    //    max_pid config default.
    assert_eq!(ns_list().len(), 1);
    let root = ns_info(0).expect("root");
    assert_eq!(root.parent_id, None);
    assert_eq!(root.max_pid, DEFAULT_MAX_PID);
    assert_eq!((root.active_pids, root.allocated, root.freed, root.high_watermark), (0, 0, 0, 0));
    let (n0, a0, f0, r0, _o0) = stats();
    assert_eq!((n0, a0, f0, r0), (1, 0, 0, 0));
    crate::serial_println!("  [1/8] root-only init: OK");

    // 2: Alloc PID increments allocated + active exactly from zero.
    alloc_pid(0).expect("alloc");
    let root = ns_info(0).expect("root");
    assert_eq!(root.allocated, 1);
    assert_eq!(root.active_pids, 1);
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Free PID increments freed, decrements active back to zero.
    free_pid(0).expect("free");
    let root = ns_info(0).expect("root");
    assert_eq!(root.freed, 1);
    assert_eq!(root.active_pids, 0);
    crate::serial_println!("  [3/8] free: OK");

    // 4: Create child namespace — id assigned, parent linked, counters zero.
    let child_id = create_ns(0, 4096).expect("create_ns");
    assert_eq!(child_id, 1);
    assert_eq!(ns_list().len(), 2);
    let child = ns_info(child_id).expect("child");
    assert_eq!(child.parent_id, Some(0));
    assert_eq!(child.max_pid, 4096);
    assert_eq!((child.active_pids, child.allocated), (0, 0));
    crate::serial_println!("  [4/8] create ns: OK");

    // 5: Child namespace alloc accounts independently of the root.
    alloc_pid(child_id).expect("child_alloc");
    let child = ns_info(child_id).expect("child");
    assert_eq!(child.active_pids, 1);
    assert_eq!(child.allocated, 1);
    crate::serial_println!("  [5/8] child ns: OK");

    // 6: High watermark tracks peak active PIDs (6 allocs without frees → 6).
    for _ in 0..5 { alloc_pid(child_id).expect("alloc_multi"); }
    let child = ns_info(child_id).expect("child");
    assert_eq!(child.active_pids, 6);
    assert_eq!(child.high_watermark, 6);
    crate::serial_println!("  [6/8] watermark: OK");

    // 7: Unknown namespace → NotFound.
    assert!(alloc_pid(999).is_err());
    assert!(free_pid(999).is_err());
    assert!(create_ns(999, 100).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    //    Allocs: 1 (root) + 6 (child) = 7; frees: 1 (root) = 1.
    //    Reuses: bumped only when allocating into a namespace that has freed at
    //    least one PID — the root freed once then no further root alloc, and the
    //    child never freed, so total_reuses stays 0.
    let (nss, alloc, freed, reuses, ops) = stats();
    assert_eq!(nss, 2);
    assert_eq!(alloc, 7);
    assert_eq!(freed, 1);
    assert_eq!(reuses, 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/pidstat table.
    *STATE.lock() = None;

    crate::serial_println!("pidstat::self_test() — all 8 tests passed");
}
