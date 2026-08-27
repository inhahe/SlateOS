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
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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

/// Whether the CPU-hotplug notifier has been installed.
///
/// [`adopt_topology`] is documented idempotent and really is called more than
/// once (`self_test` ends with a call, and `main` makes the real one), but
/// `cpu_hotplug::register_notifier` appends to a fixed-size table with no
/// duplicate check — so without this, repeated adoption would burn notifier
/// slots and run the same refresh several times per event.
static NOTIFIER_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Whether [`init_defaults`] has built the table yet.
fn is_initialized() -> bool {
    STATE.lock().is_some()
}

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
/// **Adoption is no longer one-shot.**  It used to be, and the CPU sets were a
/// boot snapshot that a CPU coming online afterwards never joined — which is
/// not theoretical, since `smp::init()` waits for APs on a *bounded* spin and a
/// straggler bumps the online count on its own after this has run.  Before
/// returning, this now registers a [`crate::cpu_hotplug`] notifier and then
/// calls [`refresh_topology`] once, which closes the window in both directions:
/// a CPU that arrived *during* adoption is picked up by that explicit refresh,
/// and one that arrives *after* is picked up by the notifier.  The registration
/// happens once however many times this function is called.
///
/// # Returns
///
/// The number of online CPUs placed across the per-node sets, or `0` if the
/// topology was not available yet and nothing was registered.
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

    // Snapshot the online set once, as a bitmask.  Every per-node loop below
    // must enumerate the *same* set of CPUs, or a CPU appearing mid-scan would
    // be placed on some nodes' lists and not others; taking the mask up front
    // makes that structural rather than a rule to remember.  It is also this
    // value, not a later re-read, that callers are told to check against.
    let mask = online_cpu_mask();
    let cpu_count = mask.count_ones() as usize;

    let mut adopted = 0usize;
    for (id, info) in topo.nodes.iter().enumerate().take(topo.node_count) {
        if !info.present {
            continue;
        }
        // Ask the placement map itself which CPUs belong here.
        let mut scratch: CpuScratch = [0; crate::smp::MAX_CPUS];
        let n = cpus_on_node(id, mask, &mut scratch);
        let cpus = scratch.get(..n).unwrap_or(&[]);
        let Ok(node_id) = u32::try_from(id) else {
            continue;
        };
        match register_node(node_id, info.total_memory, cpus) {
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

    // Subscribe, then sweep -- in that order, and never the reverse.
    //
    // Registering first means a CPU that changes state from here on cannot be
    // missed; sweeping second catches anything that changed while the loop
    // above was running off `mask`.  Doing it the other way round leaves a gap
    // between the sweep and the subscription in which an event is lost for
    // good, which is the exact bug this is fixing.
    //
    // The refresh is what this returns, so the count describes the table as it
    // stands on return rather than as the snapshot found it.
    if !NOTIFIER_REGISTERED.swap(true, Ordering::AcqRel) && register_hotplug_notifier().is_none() {
        // Not fatal -- the table is correct as of this instant and only
        // staleness is at stake -- but it silently reverts to the one-shot
        // behaviour this function's doc comment says it no longer has.
        crate::serial_println!(
            "[numastat] WARN: hotplug notifier table full; per-node CPU sets will not track CPUs coming online"
        );
    }

    refresh_topology()
}

/// Recompute every registered node's CPU set from the live view.
///
/// Called on every CPU online/offline event via the notifier
/// [`adopt_topology`] installs, and once directly by [`adopt_topology`] itself.
/// Idempotent, and cheap enough to call speculatively: it is one pass over at
/// most [`crate::smp::MAX_CPUS`] bits per node.
///
/// Nodes are updated with [`set_node_cpus`] rather than being removed and
/// re-registered — see that function for why a concurrent `/proc/numastat`
/// reader makes the difference matter.  A node the topology reports as present
/// but that was never adopted (adoption hit `ResourceExhausted`, say) is
/// registered here rather than skipped, so a later refresh can recover from a
/// transient failure instead of leaving the row missing forever.
///
/// # Returns
///
/// The number of online CPUs placed across the per-node sets, or `0` if the
/// topology is not available yet.
pub fn refresh_topology() -> usize {
    let topo = crate::numa::topology_info();

    // Same guard as `adopt_topology`: before `numa::init()` the static node
    // array advertises one node that is not present, and deriving from it would
    // replace every real CPU set with an empty one.
    if !topo.nodes.iter().take(topo.node_count).any(|n| n.present) {
        return 0;
    }

    // No table means nothing to refresh.  This is reachable: `self_test` takes
    // the table down to re-run its fixtures, and a hotplug event landing in
    // that window must be a no-op rather than a wall of warnings.  It costs
    // nothing to miss, because `self_test` ends by calling `adopt_topology`,
    // which rebuilds from the live view anyway.
    if !is_initialized() {
        return 0;
    }

    let mask = online_cpu_mask();

    for (id, info) in topo.nodes.iter().enumerate().take(topo.node_count) {
        if !info.present {
            continue;
        }
        let Ok(node_id) = u32::try_from(id) else {
            continue;
        };
        let mut scratch: CpuScratch = [0; crate::smp::MAX_CPUS];
        let n = cpus_on_node(id, mask, &mut scratch);
        let cpus = scratch.get(..n).unwrap_or(&[]);
        match set_node_cpus(node_id, cpus) {
            Ok(()) => {}
            // The node exists in the topology but has no row.  Adoption must
            // have failed for it; take the chance to put it right.
            Err(KernelError::NotFound) => {
                if let Err(e) = register_node(node_id, info.total_memory, cpus) {
                    crate::serial_println!(
                        "[numastat] WARN: node {node_id} present but unregistered, and re-registering failed: {e:?}"
                    );
                }
            }
            Err(e) => {
                crate::serial_println!(
                    "[numastat] WARN: could not refresh node {node_id} CPU set: {e:?} -- /proc/numastat CPU lists are stale"
                );
            }
        }
    }

    mask.count_ones() as usize
}

/// Hotplug notifier: keep the per-node CPU sets tracking the online set.
///
/// Only the `Post*` events do anything.  A `Pre*` event is a request for
/// permission, and this module has no grounds to veto one — a node's CPU list
/// is a description, not a constraint — so it answers `true` unconditionally.
/// Acting on `Pre*` would also be wrong on the merits: `PreOffline` fires while
/// the CPU is still running, so dropping it from the list there would make
/// `/proc/numastat` disagree with reality for the duration of the migration.
fn hotplug_notifier(_cpu: usize, event: crate::cpu_hotplug::HotplugEvent) -> bool {
    use crate::cpu_hotplug::HotplugEvent;
    if matches!(event, HotplugEvent::PostOnline | HotplugEvent::PostOffline) {
        refresh_topology();
    }
    true
}

/// Install [`hotplug_notifier`].  Separate only so the registration is a single
/// expression at its one call site.
fn register_hotplug_notifier() -> Option<usize> {
    crate::cpu_hotplug::register_notifier(hotplug_notifier)
}

/// Snapshot the set of scheduling-eligible CPUs as a bitmask.
///
/// A mask rather than a `Vec` because every caller wants to ask "is CPU *n* in
/// the set?" many times over, it needs no allocation on a path that can run
/// from an AP's bring-up, and — the reason it exists at all — passing one value
/// to every per-node derivation makes it impossible for two nodes in the same
/// pass to disagree about which CPUs exist.
///
/// The membership test is [`crate::cpu_hotplug::is_online`], not an index below
/// `smp::cpu_count()`.  Those coincide at boot but diverge the moment a CPU in
/// the middle is offlined: `smp`'s counter is a high-water mark of CPUs that
/// ever started, while this file is supposed to describe the CPUs a node
/// currently has.  Linux's `nodeN/cpulist` reports the online set too.
fn online_cpu_mask() -> u32 {
    const _: () = assert!(
        crate::smp::MAX_CPUS <= u32::BITS as usize,
        "online_cpu_mask needs one bit per CPU; widen the mask type"
    );
    let mut mask = 0u32;
    for cpu in 0..crate::smp::MAX_CPUS {
        if crate::cpu_hotplug::is_online(cpu) {
            mask |= 1u32 << cpu;
        }
    }
    mask
}

/// Scratch buffer for one node's CPU set.  Sized for the whole machine because
/// on a UMA box every CPU lands on node 0.
type CpuScratch = [u32; crate::smp::MAX_CPUS];

/// Write the CPUs from `mask` that [`crate::numa`] places on `node` into `out`,
/// returning how many were written; the answer is `&out[..n]`.
///
/// Writes into a caller-provided array rather than returning a `Vec` because
/// [`refresh_topology`] runs from the hotplug notifier, which a straggling AP
/// fires from its own bring-up path — before it has registered an idle task
/// with the scheduler.  Allocation there would very probably be fine (the same
/// function allocates an IRQ stack a few lines later), but "probably fine" is
/// not a good reason to put the heap on a CPU-bring-up path when a fixed
/// 16-element array does the job.  [`set_node_cpus`] may still grow the node's
/// own `Vec`; that is storage, not derivation, and is unavoidable without
/// changing [`NumaNode`]'s public shape.
fn cpus_on_node(node: usize, mask: u32, out: &mut CpuScratch) -> usize {
    let mut n = 0usize;
    for cpu in 0..crate::smp::MAX_CPUS {
        if mask & (1u32 << cpu) == 0 || crate::numa::cpu_node(cpu) != node {
            continue;
        }
        let (Ok(id), Some(slot)) = (u32::try_from(cpu), out.get_mut(n)) else {
            continue;
        };
        *slot = id;
        n = n.saturating_add(1);
    }
    n
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
/// The CPU checks are the sharpest of the group.  Node counts and memory sizes
/// are copied straight across and would have to be corrupted to differ, but the
/// CPU sets are *derived* — one `numa::cpu_node` query per online CPU — so a
/// CPU whose node is absent from the present set vanishes silently from every
/// node's list while every other number stays right.
///
/// **The derivation is checked per CPU, not by a total**, and that is
/// deliberate.  A count is both weaker and racier: weaker because two CPUs
/// swapped between nodes leave it unchanged, and racier because an AP coming
/// online mid-check moves it.  Asserting instead that every CPU on a row really
/// belongs to that row's node, and that no CPU is on two rows, is a property of
/// each element rather than of the whole, so a CPU arriving mid-walk cannot
/// falsify it.  `cpus_at_adoption` is then only used as a **lower bound** —
/// nothing that was placed may have been lost — which stays true no matter when
/// a straggler lands.
///
/// **`cpus_at_adoption` must be [`adopt_topology`]'s return value, not a fresh
/// `smp::cpu_count()`.** Those differ exactly when a CPU comes online between
/// the two calls, which `smp::init()` permits: it waits for APs with a bounded
/// spin, so a late AP bumps the online count itself after `init` returns.  Such
/// a CPU is now added to the table by the hotplug notifier rather than being
/// missed, so the table may legitimately be *ahead* of the value passed here —
/// which is exactly why the tally below is an inequality.
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
    crate::serial_println!("  [1/4] {present} present node(s), one row each: OK");

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
    crate::serial_println!("  [2/4] per-node memory matches numa: OK");

    // 3: every CPU on a row genuinely belongs to that row's node, and no CPU
    //    is on two rows.  Both are per-element facts, so a straggler AP joining
    //    mid-walk cannot make either of them false -- see the doc comment.
    let mut seen: u32 = 0;
    for row in &rows {
        for &cpu in &row.cpus {
            let idx = cpu as usize;
            assert!(
                idx < crate::smp::MAX_CPUS,
                "node {} lists CPU {cpu}, beyond MAX_CPUS",
                row.id
            );
            let bit = 1u32 << idx;
            assert_eq!(
                seen & bit,
                0,
                "CPU {cpu} appears on more than one node; numastat would double-count it"
            );
            seen |= bit;
            assert_eq!(
                crate::numa::cpu_node(idx) as u32,
                row.id,
                "numastat puts CPU {cpu} on node {} but numa::cpu_node says node {}",
                row.id,
                crate::numa::cpu_node(idx)
            );
        }
    }
    // Distinct CPUs, counted from the dedupe mask rather than by summing the
    // row lengths -- the two agree only because the duplicate assert above
    // passed, and deriving it from the mask makes that dependency explicit.
    let mapped = seen.count_ones() as usize;
    // 3b: nothing adoption placed has since been dropped.  An inequality, not
    //     an equality: the hotplug notifier legitimately adds CPUs after
    //     adoption returned its count.
    assert!(
        mapped >= cpus_at_adoption,
        "numastat holds {mapped} CPU(s) but adoption placed {cpus_at_adoption}; CPUs have been lost from the per-node sets"
    );
    crate::serial_println!(
        "  [3/4] {mapped} CPU(s) each on exactly one node, agreeing with numa (>= {cpus_at_adoption} placed): OK"
    );

    // 4: `refresh_topology` is a fixed point.  The hotplug notifier calls it on
    //    every CPU state change, so a version that drifted -- duplicating CPUs,
    //    dropping them, or disturbing the counters -- would corrupt
    //    /proc/numastat a little more on each event rather than failing once
    //    and visibly.  Running it against an unchanged machine must produce
    //    identical rows.
    //
    //    "Unchanged" is the precondition, and it is one this test cannot create,
    //    only observe: a straggler AP joining between the two snapshots would
    //    change the rows *correctly* and fail an equality that was never about
    //    it.  So the online set is read either side of the comparison and the
    //    result is only trusted if it held still.  Skipping is the honest
    //    outcome there, not a swallowed failure -- the property is untestable in
    //    that instant, and saying so beats a boot that fails one time in
    //    hundreds for a reason nobody can reproduce.
    let mut skips = crate::fs::selftest::Skips::new();
    let mask_before = online_cpu_mask();
    let before = list_nodes();
    let again = refresh_topology();
    let after = list_nodes();
    if online_cpu_mask() == mask_before {
        assert_eq!(
            again,
            mask_before.count_ones() as usize,
            "refresh_topology placed {again} CPU(s) but {} are online; a CPU's numa::cpu_node names a node that reports itself absent",
            mask_before.count_ones()
        );
        assert_eq!(
            after.len(),
            before.len(),
            "refresh changed the number of nodes"
        );
        for (a, b) in before.iter().zip(after.iter()) {
            assert_eq!(a.id, b.id, "refresh reordered the node rows");
            assert_eq!(a.cpus, b.cpus, "refresh changed node {}'s CPU set", a.id);
            assert_eq!(
                a.total_memory, b.total_memory,
                "refresh disturbed node {}'s memory",
                a.id
            );
            assert_eq!(
                (
                    a.local_allocs,
                    a.remote_allocs,
                    a.migrations_in,
                    a.migrations_out
                ),
                (
                    b.local_allocs,
                    b.remote_allocs,
                    b.migrations_in,
                    b.migrations_out
                ),
                "refresh disturbed node {}'s counters",
                a.id
            );
        }
        crate::serial_println!("  [4/4] refresh_topology is idempotent: OK");
    } else {
        skips.record(
            "refresh_topology idempotence",
            "a CPU changed state mid-check, so 'unchanged machine' did not hold",
        );
    }

    skips.report("numastat::self_test_adoption()");
    crate::serial_println!(
        "numastat::self_test_adoption() — all 4 tests passed{}",
        skips.suffix()
    );
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

/// Replace a registered node's CPU set in place.
///
/// The counterpart to [`register_node`] for a node that already exists, which
/// is the case [`refresh_topology`] hits on every call after the first:
/// `register_node` answers `AlreadyExists` and refuses, by design, so that a
/// repeat adoption cannot silently zero the counters.
///
/// **In place, rather than unregister-then-re-register.**  The node row is what
/// `/proc/numastat` reads, and a reader can be walking it on another CPU at any
/// moment.  Removing the row first would give that reader a brief, entirely
/// fictitious view of a machine with one fewer node — and if it were the last
/// node, of a machine with no memory at all.  Overwriting one field leaves
/// every other value continuously valid; the worst a concurrent reader sees is
/// the CPU set from a moment ago, which is the same staleness it would have
/// seen by arriving a microsecond earlier.
///
/// The allocation/access/migration counters are deliberately untouched: a CPU
/// joining or leaving a node does not un-count the pages already allocated
/// there.
///
/// # Errors
///
/// [`KernelError::NotFound`] if no node with `id` is registered.
pub fn set_node_cpus(id: u32, cpus: &[u32]) -> KernelResult<()> {
    with_state(|state| {
        let Some(node) = state.nodes.iter_mut().find(|n| n.id == id) else {
            return Err(KernelError::NotFound);
        };
        node.cpus.clear();
        node.cpus.extend_from_slice(cpus);
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
    crate::serial_println!("  [8/9] stats: OK");

    // 9: `set_node_cpus` replaces a node's CPU set in place, and only that.
    //    The counters are the point: `refresh_topology` calls this on every
    //    hotplug event, so if it reset accounting, a CPU coming online would
    //    silently erase the allocation history of the node it joined.
    let before = get_node(0).expect("before set_node_cpus");
    set_node_cpus(0, &[0, 1, 2, 3, 8]).expect("set_node_cpus");
    let after = get_node(0).expect("after set_node_cpus");
    assert_eq!(after.cpus, alloc::vec![0, 1, 2, 3, 8]);
    assert_eq!(after.local_allocs, before.local_allocs);
    assert_eq!(after.remote_allocs, before.remote_allocs);
    assert_eq!(after.used_memory, before.used_memory);
    assert_eq!(after.free_memory, before.free_memory);
    assert_eq!(after.avg_latency_ns, before.avg_latency_ns);
    assert_eq!(after.migrations_in, before.migrations_in);
    assert_eq!(after.migrations_out, before.migrations_out);
    // Shrinking must actually shrink -- an `extend` without the `clear` would
    // pass the growth case above and quietly accumulate here.
    set_node_cpus(0, &[7]).expect("set_node_cpus shrink");
    assert_eq!(get_node(0).expect("shrunk").cpus, alloc::vec![7]);
    // Unknown node id is refused rather than creating a row.
    assert_eq!(set_node_cpus(99, &[0]), Err(KernelError::NotFound));
    assert_eq!(
        list_nodes().len(),
        2,
        "a refused update must not add a node"
    );
    crate::serial_println!("  [9/9] set_node_cpus in place: OK");

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

    crate::serial_println!("numastat::self_test() — all 9 tests passed");
}
