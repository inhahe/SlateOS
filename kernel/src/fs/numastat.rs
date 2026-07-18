//! NUMA Statistics — non-uniform memory access node tracking.
//!
//! Tracks memory allocation, access latency, and migration
//! statistics per NUMA node. Helps optimize memory placement
//! for NUMA-aware workloads.
//!
//! ## Architecture
//!
//! ```text
//! NUMA statistics
//!   → numastat::get_node(id) → node statistics
//!   → numastat::record_alloc(node, bytes) → record allocation
//!   → numastat::record_access(node, latency) → record access
//!   → numastat::balance_report() → balance analysis
//!
//! Integration:
//!   → memlayout (memory layout)
//!   → cputopo (CPU topology)
//!   → schedtune (scheduler tuning)
//!   → perfmon (performance monitor)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// NUMA node state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Online,
    Offline,
    HotAdded,
    Draining,
}

impl NodeState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::HotAdded => "hot-added",
            Self::Draining => "draining",
        }
    }
}

/// Per-node statistics.
#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: u32,
    pub state: NodeState,
    pub total_memory: u64,
    pub free_memory: u64,
    pub used_memory: u64,
    pub local_allocs: u64,
    pub remote_allocs: u64,
    pub local_accesses: u64,
    pub remote_accesses: u64,
    pub avg_latency_ns: u64,
    pub migrations_in: u64,
    pub migrations_out: u64,
    pub cpus: Vec<u32>,
}

/// Inter-node distance (latency ratio).
#[derive(Debug, Clone)]
pub struct NodeDistance {
    pub from_node: u32,
    pub to_node: u32,
    pub distance: u32,  // 10 = local, higher = farther.
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_NODES: usize = 64;

struct State {
    nodes: Vec<NumaNode>,
    distances: Vec<NodeDistance>,
    total_allocs: u64,
    total_remote: u64,
    total_migrations: u64,
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

/// Initialise an **empty** NUMA-statistics table.
///
/// Seeds NO node rows, NO distance matrix, and zero totals.  Real NUMA
/// accounting is wired through [`register_node`] (one row per online node with
/// its real memory size and CPU set, populated from the ACPI SRAT at bring-up),
/// [`set_distance`] (from the ACPI SLIT), and the
/// `record_local_alloc`/`record_remote_alloc`/`record_access`/`record_migration`
/// functions; until those are called the tables are genuinely empty, so the
/// `/proc/numastat` file and the `numastat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded two fictional nodes (id 0/1, 8 GiB each, with
/// local_allocs 50_000/30_000, local_accesses 1_000_000/800_000, and
/// migration counts) plus a fabricated 2x2 distance matrix and invented
/// aggregate totals (total_allocs 87_000, total_remote 7_000, total_migrations
/// 250), which `/proc/numastat` then displayed as if they were real per-node
/// memory-placement measurements.  That demo data was removed; the self-test
/// now builds its own fixtures explicitly via the real API (see [`self_test`]).
/// The memory subsystem is expected to call [`register_node`]/[`set_distance`]
/// from the ACPI topology and the record_* functions as memory is placed.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        nodes: Vec::new(),
        distances: Vec::new(),
        total_allocs: 0,
        total_remote: 0,
        total_migrations: 0,
        ops: 0,
    });
}

/// Register a NUMA node.
///
/// The memory subsystem calls this once per online node at bring-up (from the
/// ACPI SRAT) so the per-node table reflects the real topology — the node's
/// actual memory size and CPU set — with all allocation/access/migration
/// counters zeroed.  The record_* functions return `NotFound` for an
/// unregistered node id.
pub fn register_node(id: u32, total_memory: u64, cpus: &[u32]) -> KernelResult<()> {
    with_state(|state| {
        if state.nodes.iter().any(|n| n.id == id) { return Err(KernelError::AlreadyExists); }
        if state.nodes.len() >= MAX_NODES { return Err(KernelError::ResourceExhausted); }
        state.nodes.push(NumaNode {
            id, state: NodeState::Online,
            total_memory, free_memory: total_memory, used_memory: 0,
            local_allocs: 0, remote_allocs: 0,
            local_accesses: 0, remote_accesses: 0,
            avg_latency_ns: 0, migrations_in: 0, migrations_out: 0,
            cpus: cpus.to_vec(),
        });
        Ok(())
    })
}

/// Set the distance between two nodes (from the ACPI SLIT).
///
/// 10 = local (same node), higher = farther.  Replaces any existing entry for
/// the same (from, to) pair so a re-read of the SLIT is idempotent.
pub fn set_distance(from: u32, to: u32, distance: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(d) = state.distances.iter_mut().find(|d| d.from_node == from && d.to_node == to) {
            d.distance = distance;
        } else {
            state.distances.push(NodeDistance { from_node: from, to_node: to, distance });
        }
        Ok(())
    })
}

/// Get node statistics.
pub fn get_node(id: u32) -> Option<NumaNode> {
    STATE.lock().as_ref().and_then(|s| s.nodes.iter().find(|n| n.id == id).cloned())
}

/// List all nodes.
pub fn list_nodes() -> Vec<NumaNode> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.nodes.clone())
}

/// Record a local allocation.
pub fn record_local_alloc(node_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let n = state.nodes.iter_mut().find(|n| n.id == node_id).ok_or(KernelError::NotFound)?;
        n.local_allocs += 1;
        n.used_memory = n.used_memory.saturating_add(bytes);
        n.free_memory = n.total_memory.saturating_sub(n.used_memory);
        state.total_allocs += 1;
        Ok(())
    })
}

/// Record a remote allocation (allocated on node_id but accessed from another).
pub fn record_remote_alloc(node_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let n = state.nodes.iter_mut().find(|n| n.id == node_id).ok_or(KernelError::NotFound)?;
        n.remote_allocs += 1;
        n.used_memory = n.used_memory.saturating_add(bytes);
        n.free_memory = n.total_memory.saturating_sub(n.used_memory);
        state.total_allocs += 1;
        state.total_remote += 1;
        Ok(())
    })
}

/// Record a memory access.
pub fn record_access(node_id: u32, is_local: bool, latency_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let n = state.nodes.iter_mut().find(|n| n.id == node_id).ok_or(KernelError::NotFound)?;
        if is_local {
            n.local_accesses += 1;
        } else {
            n.remote_accesses += 1;
        }
        // Update running average latency.
        let total = n.local_accesses + n.remote_accesses;
        if total > 0 {
            n.avg_latency_ns = (n.avg_latency_ns * (total - 1) + latency_ns) / total;
        }
        Ok(())
    })
}

/// Record a page migration between nodes.
pub fn record_migration(from_node: u32, to_node: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(n) = state.nodes.iter_mut().find(|n| n.id == from_node) {
            n.migrations_out += 1;
        }
        if let Some(n) = state.nodes.iter_mut().find(|n| n.id == to_node) {
            n.migrations_in += 1;
        }
        state.total_migrations += 1;
        Ok(())
    })
}

/// Get inter-node distance.
pub fn get_distance(from: u32, to: u32) -> Option<u32> {
    STATE.lock().as_ref().and_then(|s| {
        s.distances.iter().find(|d| d.from_node == from && d.to_node == to).map(|d| d.distance)
    })
}

/// List distances.
pub fn list_distances() -> Vec<NodeDistance> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.distances.clone())
}

/// Balance report: percentage of remote allocations.
pub fn remote_alloc_pct() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) if s.total_allocs > 0 => s.total_remote * 100 / s.total_allocs,
        _ => 0,
    }
}

/// Statistics: (node_count, total_allocs, total_remote, total_migrations, remote_pct, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let pct = if s.total_allocs > 0 { s.total_remote * 100 / s.total_allocs } else { 0 };
            (s.nodes.len(), s.total_allocs, s.total_remote, s.total_migrations, pct, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("numastat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/numastat must never surface).
    // Resetting first clears any residue from a prior `numastat test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated nodes, distances, or totals.
    assert_eq!(list_nodes().len(), 0);
    assert_eq!(list_distances().len(), 0);
    let (c0, a0, r0, m0, p0, _o0) = stats();
    assert_eq!((c0, a0, r0, m0, p0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register two nodes (real memory + CPU sets, zeroed counters);
    //    duplicate registration fails.
    register_node(0, 8_589_934_592, &[0, 1, 2, 3]).expect("node0");
    register_node(1, 8_589_934_592, &[4, 5, 6, 7]).expect("node1");
    assert!(register_node(0, 1, &[]).is_err());
    let n = get_node(0).expect("get");
    assert_eq!(n.cpus.len(), 4);
    assert_eq!(n.state, NodeState::Online);
    assert_eq!(n.free_memory, 8_589_934_592); // all free at registration
    assert_eq!(n.local_allocs, 0);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Local alloc (exact, from zero); free memory drops by the alloc size.
    record_local_alloc(0, 4096).expect("alloc");
    let n = get_node(0).expect("get2");
    assert_eq!(n.local_allocs, 1);
    assert_eq!(n.used_memory, 4096);
    assert_eq!(n.free_memory, 8_589_934_592 - 4096);
    crate::serial_println!("  [3/8] local alloc: OK");

    // 4: Remote alloc bumps remote counter and aggregate remote total.
    record_remote_alloc(1, 8192).expect("remote");
    let n = get_node(1).expect("get3");
    assert_eq!(n.remote_allocs, 1);
    assert!(record_local_alloc(99, 1).is_err()); // NotFound on unknown node
    crate::serial_println!("  [4/8] remote alloc: OK");

    // 5: Access updates the running latency average exactly (cold-start: first
    //    sample seeds the average, second blends): (70, then (70+200)/2 = 135).
    record_access(0, true, 70).expect("access");
    let n = get_node(0).expect("acc1");
    assert_eq!(n.avg_latency_ns, 70);
    record_access(0, false, 200).expect("access2");
    let n = get_node(0).expect("acc2");
    assert_eq!(n.avg_latency_ns, 135);
    assert_eq!(n.local_accesses, 1);
    assert_eq!(n.remote_accesses, 1);
    crate::serial_println!("  [5/8] access: OK");

    // 6: Migration bumps out/in counters on the respective nodes exactly.
    record_migration(0, 1).expect("migrate");
    let n0 = get_node(0).expect("get4");
    let n1 = get_node(1).expect("get5");
    assert_eq!(n0.migrations_out, 1);
    assert_eq!(n1.migrations_in, 1);
    crate::serial_println!("  [6/8] migration: OK");

    // 7: Distances set from the (simulated) SLIT; set_distance is idempotent.
    set_distance(0, 0, 10).expect("d00");
    set_distance(0, 1, 20).expect("d01");
    set_distance(0, 1, 21).expect("d01b"); // overwrite, not duplicate
    assert_eq!(get_distance(0, 1).expect("dist"), 21);
    assert_eq!(get_distance(0, 0).expect("dist2"), 10);
    assert_eq!(list_distances().len(), 2); // (0,0) and (0,1) — no dup
    crate::serial_println!("  [7/8] distance: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    //    total_allocs = 1 local + 1 remote = 2; remote = 1; pct = 50.
    let (nodes, allocs, remote, migrations, pct, ops) = stats();
    assert_eq!(nodes, 2);
    assert_eq!(allocs, 2);
    assert_eq!(remote, 1);
    assert_eq!(migrations, 1);
    assert_eq!(pct, 50); // 1 remote / 2 allocs
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/numastat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the memory subsystem
    // wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("numastat::self_test() — all 8 tests passed");
}
