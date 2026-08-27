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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    pub distance: u32, // 10 = local, higher = farther.
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
/// Seeds NO node rows, NO distance matrix, and zero totals.  The node rows are
/// filled in immediately afterwards by [`adopt_topology`], which reads the real
/// topology out of [`crate::numa`]; the distance matrix and the
/// allocation/access/migration counters stay empty, because the ACPI SLIT is
/// not parsed and nothing records per-node page placement.  See
/// [`adopt_topology`] for why filling either from a plausible-looking source
/// would be worse than leaving it empty — the kernel's hard "never invent data
/// in procfs" rule.
///
/// NOTE: this previously seeded two fictional nodes (id 0/1, 8 GiB each, with
/// local_allocs 50_000/30_000, local_accesses 1_000_000/800_000, and
/// migration counts) plus a fabricated 2x2 distance matrix and invented
/// aggregate totals (total_allocs 87_000, total_remote 7_000, total_migrations
/// 250), which `/proc/numastat` then displayed as if they were real per-node
/// memory-placement measurements.  That demo data was removed; the self-test
/// now builds its own fixtures explicitly via the real API (see [`self_test`]).
///
/// This doc used to end by saying the memory subsystem "is expected to" call
/// [`register_node`]/[`set_distance`] from the ACPI topology.  It never did,
/// for as long as the module existed, and the sentence is why nobody checked:
/// a comment describing wiring that does not exist reads exactly like one
/// describing wiring that does.  The topology half is now genuinely wired
/// ([`adopt_topology`]); the record_* half still has no producer, and is
/// described above as absent rather than as expected.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        nodes: Vec::new(),
        distances: Vec::new(),
        total_allocs: 0,
        total_remote: 0,
        total_migrations: 0,
        ops: 0,
    });
}

/// Adopt the NUMA topology [`crate::numa`] detected at boot.
///
/// This is the wiring that makes `/proc/numastat` describe the machine it is
/// running on.  Without it the table stays empty for the whole boot and the
/// file reports `nodes: 0` — not "unknown", but a false count, since a machine
/// with memory has at least one node.  `numa::init()` has known the answer all
/// along (from the ACPI SRAT, or from the single-node fallback whose memory
/// total comes from the frame allocator); nothing had ever asked it.
///
/// **The CPU set comes from [`crate::numa::cpu_node`], one online CPU at a
/// time, rather than from the per-node CPU bitmask.** Both are derived from
/// the same SRAT, so today they agree — but `cpu_node` is the map the
/// *scheduler* actually places threads by, so sourcing from it means
/// `/proc/numastat` cannot report a CPU on a node the scheduler treats as
/// elsewhere.  Agreement by construction, not by coincidence.  It also has no
/// 32-CPU ceiling, which the `u32` bitmask does.
///
/// **What this deliberately does not populate:**
///
/// * **The allocation, access-latency and migration counters.**  Nothing
///   produces them: the frame allocator does not record which node a page came
///   from.  They stay zero because zero is what is known, and a plausible
///   number here would be indistinguishable from a measurement.
/// * **The distance matrix**, though [`crate::numa::distance`] is right there
///   and returns exactly the 10/20 shape [`set_distance`] wants.  That
///   function's own doc says it is a uniform-remote-cost *model* standing in
///   for the ACPI SLIT, which is not parsed yet.  [`NodeDistance`] has no
///   field separating modelled from measured, so writing those numbers here
///   would turn a guess into a reading the moment it reached procfs — the same
///   failure as the fabricated seed rows this module used to carry, arriving
///   by a longer route.  `get_distance` returning `NotFound` is the honest
///   answer until the SLIT is parsed.
///
/// Idempotent, and safe to call before `numa::init()`: a node that reports
/// itself absent means the topology has not been detected yet, and nothing is
/// registered.  That is what lets [`self_test`] — which wipes the table and
/// runs long before `numa::init()` at boot — end by calling this to restore
/// real data when it is invoked manually from kshell afterwards.
///
/// **Adoption is one-shot: the CPU sets are a snapshot.** A CPU that comes
/// online after this returns is not in `/proc/numastat`, because nothing
/// re-runs adoption on a hotplug event.  That is not merely theoretical —
/// `smp::init()` waits for APs with a *bounded* spin, so an AP that misses the
/// window bumps the online count on its own afterwards.  The returned count is
/// the snapshot this call enumerated, so a caller that needs to check the
/// result can compare against what was actually placed rather than against a
/// number that may have moved since.  Tracked in `known-issues.md` →
/// `A-NUMASTAT-CPU-SETS-ARE-A-BOOT-SNAPSHOT-NOT-A-HOTPLUG-VIEW`.
///
/// # Returns
///
/// The number of online CPUs enumerated while building the per-node sets, or
/// `0` if the topology was not available yet and nothing was registered.
pub fn adopt_topology() -> usize {
    init_defaults();

    let topo = crate::numa::topology_info();

    // `numa`'s node array is statically zeroed with `NODE_COUNT` defaulting to
    // 1, so before `numa::init()` runs it advertises one node that is not
    // `present`.  Adopting from that would register nothing while reporting
    // "0 of 1", which reads as a failure rather than as "asked too early".
    // Return silently instead: the boot-time call from `self_test` lands here,
    // and `main` calls again for real once the topology exists.
    if !topo.nodes.iter().take(topo.node_count).any(|n| n.present) {
        return 0;
    }

    // Read once and reuse.  Every per-node loop below must enumerate the *same*
    // set of CPUs, or a CPU appearing mid-scan would be placed on some nodes'
    // lists and not others; and it is this value, not a later re-read, that
    // callers are told to check against.
    let cpu_count = crate::smp::cpu_count();

    let mut adopted = 0usize;
    for (id, info) in topo.nodes.iter().enumerate().take(topo.node_count) {
        if !info.present {
            continue;
        }
        // Ask the placement map itself which CPUs belong here.
        let mut cpus = Vec::new();
        for cpu in 0..cpu_count {
            if crate::numa::cpu_node(cpu) != id {
                continue;
            }
            if let Ok(n) = u32::try_from(cpu) {
                cpus.push(n);
            }
        }
        let Ok(node_id) = u32::try_from(id) else {
            continue;
        };
        match register_node(node_id, info.total_memory, &cpus) {
            Ok(()) => adopted += 1,
            // `AlreadyExists` means this ran twice, which is fine and is why
            // the function is documented idempotent.  Anything else means the
            // table could not take the real topology, and a silently short
            // node list is precisely the failure this whole change exists to
            // remove -- so say so rather than letting procfs under-report.
            Err(KernelError::AlreadyExists) => {}
            Err(e) => {
                crate::serial_println!(
                    "[numastat] WARN: could not adopt node {node_id}: {e:?} -- /proc/numastat will under-report"
                );
            }
        }
    }

    crate::serial_println!(
        "[numastat] adopted {} of {} node(s) from numa topology ({}), {} CPU(s) placed; counters and distances stay empty (no producer / SLIT unparsed)",
        adopted,
        topo.node_count,
        if topo.is_numa { "SRAT" } else { "UMA fallback" },
        cpu_count,
    );

    cpu_count
}

/// Verify that what `/proc/numastat` will report matches the machine
/// [`crate::numa`] found.
///
/// Separate from [`self_test`] because it tests a different thing at a
/// different time.  [`self_test`] runs near the top of boot and proves the
/// *accounting* works, on fixtures it builds itself; this runs after
/// [`adopt_topology`], with `numa::init()` behind it, and proves the *wiring*
/// works — that the numbers a user reads describe their hardware.  Merging
/// them is impossible in either direction: the fixture test must run before
/// there is real data to destroy, and this one cannot run before there is real
/// data to check.
///
/// It deliberately re-reads both instruments rather than trusting the return
/// codes [`adopt_topology`] already saw.  Accepted writes prove the table took
/// the values; only a read-back through the same API `procfs` uses proves the
/// file describes the machine.
///
/// The CPU tally is the sharpest of the three checks.  Node counts and memory
/// sizes are copied straight across and would have to be corrupted to differ,
/// but the CPU sets are *derived* — one `numa::cpu_node` query per online CPU
/// — so a CPU whose node is absent from the present set vanishes silently from
/// every node's list while every other number stays right.  Summing them and
/// demanding the enumerated count is what makes that visible.
///
/// **`cpus_at_adoption` must be [`adopt_topology`]'s return value, not a fresh
/// `smp::cpu_count()`.** Those differ exactly when a CPU comes online between
/// the two calls, which `smp::init()` permits: it waits for APs with a bounded
/// spin, so a late AP bumps the online count itself after `init` returns.
/// Re-reading would then fail the boot over a table that is entirely correct —
/// merely one CPU out of date — which is a flake, not a bug report.  What this
/// check is *for* is the derivation: every CPU adoption looked at must land on
/// exactly one node.  Staleness is a known property of one-shot adoption and is
/// documented on [`adopt_topology`], not asserted against here.
///
/// # Panics
///
/// Panics if `/proc/numastat` would not describe the machine `numa` found.
/// That is the intent: this runs only from the boot self-test path, where a
/// panic is the failure channel, and a procfs file that misdescribes the
/// hardware is worse than not booting.
pub fn self_test_adoption(cpus_at_adoption: usize) {
    crate::serial_println!("numastat::self_test_adoption() — running tests...");

    let topo = crate::numa::topology_info();
    let present = topo
        .nodes
        .iter()
        .take(topo.node_count)
        .filter(|n| n.present)
        .count();
    let rows = list_nodes();

    // 1: one row per present node -- never zero, which is what this whole
    //    wiring exists to stop `/proc/numastat` claiming.
    assert!(
        present > 0,
        "numa reports no present node after init; a machine with memory has at least one"
    );
    assert_eq!(
        rows.len(),
        present,
        "numastat holds {} node row(s) but numa reports {present} present node(s)",
        rows.len()
    );
    crate::serial_println!("  [1/3] {present} present node(s), one row each: OK");

    // 2: each row's memory is the node's, not a placeholder.
    for row in &rows {
        let info = topo
            .nodes
            .get(row.id as usize)
            .expect("every adopted id indexes topo.nodes: adopt_topology enumerates it");
        assert_eq!(
            row.total_memory, info.total_memory,
            "node {} memory disagrees: numastat {} vs numa {}",
            row.id, row.total_memory, info.total_memory
        );
        assert_eq!(
            row.state,
            NodeState::Online,
            "node {} adopted from a present numa node should be online",
            row.id
        );
    }
    crate::serial_println!("  [2/3] per-node memory matches numa: OK");

    // 3: every CPU adoption enumerated is placed on exactly one node.
    let mapped: usize = rows.iter().map(|n| n.cpus.len()).sum();
    assert_eq!(
        mapped, cpus_at_adoption,
        "numastat placed {mapped} CPU(s) across its nodes but adoption enumerated {cpus_at_adoption}; numa::cpu_node names a node that reports itself absent"
    );
    crate::serial_println!(
        "  [3/3] all {cpus_at_adoption} enumerated CPU(s) placed exactly once: OK"
    );

    crate::serial_println!("numastat::self_test_adoption() — all 3 tests passed");
}

/// Register a NUMA node.
///
/// [`adopt_topology`] calls this once per online node just after
/// `numa::init()`, so the per-node table reflects the real topology — the
/// node's actual memory size and CPU set — with all allocation/access/
/// migration counters zeroed.  The record_* functions return `NotFound` for an
/// unregistered node id.
pub fn register_node(id: u32, total_memory: u64, cpus: &[u32]) -> KernelResult<()> {
    with_state(|state| {
        if state.nodes.iter().any(|n| n.id == id) {
            return Err(KernelError::AlreadyExists);
        }
        if state.nodes.len() >= MAX_NODES {
            return Err(KernelError::ResourceExhausted);
        }
        state.nodes.push(NumaNode {
            id,
            state: NodeState::Online,
            total_memory,
            free_memory: total_memory,
            used_memory: 0,
            local_allocs: 0,
            remote_allocs: 0,
            local_accesses: 0,
            remote_accesses: 0,
            avg_latency_ns: 0,
            migrations_in: 0,
            migrations_out: 0,
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
        if let Some(d) = state
            .distances
            .iter_mut()
            .find(|d| d.from_node == from && d.to_node == to)
        {
            d.distance = distance;
        } else {
            state.distances.push(NodeDistance {
                from_node: from,
                to_node: to,
                distance,
            });
        }
        Ok(())
    })
}

/// Get node statistics.
pub fn get_node(id: u32) -> Option<NumaNode> {
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.nodes.iter().find(|n| n.id == id).cloned())
}

/// List all nodes.
pub fn list_nodes() -> Vec<NumaNode> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.nodes.clone())
}

/// Record a local allocation.
pub fn record_local_alloc(node_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let n = state
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or(KernelError::NotFound)?;
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
        let n = state
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or(KernelError::NotFound)?;
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
        let n = state
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or(KernelError::NotFound)?;
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
        s.distances
            .iter()
            .find(|d| d.from_node == from && d.to_node == to)
            .map(|d| d.distance)
    })
}

/// List distances.
pub fn list_distances() -> Vec<NodeDistance> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.distances.clone())
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
            let pct = if s.total_allocs > 0 {
                s.total_remote * 100 / s.total_allocs
            } else {
                0
            };
            (
                s.nodes.len(),
                s.total_allocs,
                s.total_remote,
                s.total_migrations,
                pct,
                s.ops,
            )
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

    // Leave the table LIVE, not DEAD and not stale: clear the fixtures, then
    // re-open it and re-adopt the real topology.
    //
    // Clearing alone would switch this module off for the rest of the boot
    // -- `init_defaults` runs once, that once is here, and every later write
    // would take the `NotSupported` arm and be dropped by a caller that must
    // not let statistics fail a real operation.  known-issues.md:
    // A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT.
    //
    // Re-opening but not re-adopting is the subtler trap, and it is why
    // `adopt_topology` is called here rather than only from `main`: this
    // function is also reachable as `numastat test` from kshell, long after
    // boot.  Without this line, running the self-test would silently empty
    // /proc/numastat for the rest of the boot -- a diagnostic command
    // destroying the thing it diagnoses.  At boot the call is a documented
    // no-op, because `numa::init()` has not run yet and no node reports
    // itself present; `main` adopts for real a moment later.
    // Two steps, not one, even though `adopt_topology` opens the table itself:
    // re-opening and re-adopting are separate obligations and the call site is
    // where that should be legible.  `check-selftest-reinit.py` also scans for
    // the literal `init_defaults()` after a clear and cannot see through a
    // call, which is the right tradeoff -- a gate that chased callees would
    // pass on any function that merely might re-open the table.
    //
    // The CPU count it returns is discarded on purpose: this call is here to
    // *restore* the table, not to check it.  Only `main`'s call feeds
    // `self_test_adoption`, which is the one that verifies the placement.
    *STATE.lock() = None;
    init_defaults();
    adopt_topology();

    crate::serial_println!("numastat::self_test() — all 8 tests passed");
}
