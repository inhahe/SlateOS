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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        nodes: alloc::vec![
            NumaNode {
                id: 0, state: NodeState::Online,
                total_memory: 8_589_934_592, free_memory: 4_294_967_296, used_memory: 4_294_967_296,
                local_allocs: 50000, remote_allocs: 2000,
                local_accesses: 1_000_000, remote_accesses: 50_000,
                avg_latency_ns: 80, migrations_in: 100, migrations_out: 150,
                cpus: alloc::vec![0, 1, 2, 3],
            },
            NumaNode {
                id: 1, state: NodeState::Online,
                total_memory: 8_589_934_592, free_memory: 6_442_450_944, used_memory: 2_147_483_648,
                local_allocs: 30000, remote_allocs: 5000,
                local_accesses: 800_000, remote_accesses: 80_000,
                avg_latency_ns: 80, migrations_in: 150, migrations_out: 100,
                cpus: alloc::vec![4, 5, 6, 7],
            },
        ],
        distances: alloc::vec![
            NodeDistance { from_node: 0, to_node: 0, distance: 10 },
            NodeDistance { from_node: 0, to_node: 1, distance: 20 },
            NodeDistance { from_node: 1, to_node: 0, distance: 20 },
            NodeDistance { from_node: 1, to_node: 1, distance: 10 },
        ],
        total_allocs: 87000,
        total_remote: 7000,
        total_migrations: 250,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_nodes().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get node.
    let n = get_node(0).expect("get");
    assert_eq!(n.cpus.len(), 4);
    assert_eq!(n.state, NodeState::Online);
    crate::serial_println!("  [2/8] get node: OK");

    // 3: Local alloc.
    record_local_alloc(0, 4096).expect("alloc");
    let n = get_node(0).expect("get2");
    assert_eq!(n.local_allocs, 50001);
    crate::serial_println!("  [3/8] local alloc: OK");

    // 4: Remote alloc.
    record_remote_alloc(1, 8192).expect("remote");
    let n = get_node(1).expect("get3");
    assert_eq!(n.remote_allocs, 5001);
    crate::serial_println!("  [4/8] remote alloc: OK");

    // 5: Access.
    record_access(0, true, 70).expect("access");
    record_access(0, false, 200).expect("access2");
    crate::serial_println!("  [5/8] access: OK");

    // 6: Migration.
    record_migration(0, 1).expect("migrate");
    let n0 = get_node(0).expect("get4");
    let n1 = get_node(1).expect("get5");
    assert_eq!(n0.migrations_out, 151);
    assert_eq!(n1.migrations_in, 151);
    crate::serial_println!("  [6/8] migration: OK");

    // 7: Distance.
    let d = get_distance(0, 1).expect("dist");
    assert_eq!(d, 20);
    let d = get_distance(0, 0).expect("dist2");
    assert_eq!(d, 10);
    crate::serial_println!("  [7/8] distance: OK");

    // 8: Stats.
    let (nodes, allocs, remote, migrations, pct, ops) = stats();
    assert_eq!(nodes, 2);
    assert!(allocs > 87000);
    assert!(remote > 7000);
    assert!(migrations > 250);
    assert!(pct > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("numastat::self_test() — all 8 tests passed");
}
