//! PID Namespaces — process ID isolation for containers.
//!
//! Provides Linux-style PID namespace isolation.  Each PID namespace
//! has its own PID number space: a process has a different PID in each
//! namespace it's visible in.
//!
//! ## Design
//!
//! PID namespaces form a tree rooted at the **root namespace** (ID 0).
//! Every process belongs to exactly one namespace.
//!
//! - **Isolation**: A process can only see PIDs in its own namespace
//!   and descendant namespaces.  It cannot see PIDs in ancestor or
//!   sibling namespaces.
//! - **PID translation**: Each process has a global `ProcessId`
//!   (unique across all namespaces) and a namespace-local PID (unique
//!   within its namespace, starting from 1).
//! - **Hierarchy visibility**: A process in namespace N can see
//!   processes in N and all namespaces descended from N.  The global
//!   (root) namespace can see all processes.
//! - **Init per namespace**: The first process in a namespace gets
//!   local PID 1.  When this process exits, all other processes in
//!   the namespace are cleaned up (similar to Linux behavior).
//!
//! ## Integration Points
//!
//! - **Process creation**: `proc/pcb.rs` calls [`alloc_pid`] to assign
//!   a namespace-local PID to each new process.
//! - **SYS_PROCESS_ID**: Returns the namespace-local PID, not the
//!   global `ProcessId`.
//! - **Process lookup**: Syscalls like `SYS_PROCESS_KILL` translate
//!   the namespace-local PID to global via [`translate_to_global`].
//!
//! ## Capacity
//!
//! Up to [`MAX_NAMESPACES`] (64) PID namespaces, each supporting up
//! to [`MAX_PIDS_PER_NS`] (1024) processes.  Sufficient for container
//! workloads on a desktop OS.
//!
//! ## Performance
//!
//! The namespace table is behind a `spin::Mutex`.  PID translation
//! (the hot path) uses per-namespace `BTreeMap` lookups under the
//! lock.  For a desktop OS with a handful of containers, this is
//! fine — the lock is held briefly and contention is rare.
//!
//! ## References
//!
//! - Linux `kernel/pid_namespace.c`, `kernel/pid.c`
//! - `man 7 pid_namespaces`
//! - Design spec: container primitives for Docker support

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of PID namespaces.
pub const MAX_NAMESPACES: usize = 64;

/// Maximum processes tracked per namespace.
///
/// This limits the number of simultaneous processes in any single
/// namespace.  The global (root) namespace may have more since it
/// represents all processes.
pub const MAX_PIDS_PER_NS: usize = 1024;

/// The root PID namespace.  Always exists, cannot be deleted.
pub const ROOT_NS: PidNsId = 0;

/// Sentinel value for "no parent" (root namespace).
const NO_PARENT: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a PID namespace.
pub type PidNsId = u32;

/// A namespace-local PID.
///
/// This is the PID that processes within the namespace see.  PID 1
/// is the namespace's init process.
pub type LocalPid = u32;

/// Global process ID type (matches proc/pcb.rs ProcessId).
type GlobalPid = u64;

// ---------------------------------------------------------------------------
// Per-namespace data
// ---------------------------------------------------------------------------

/// A PID namespace node.
struct PidNamespace {
    /// Whether this slot is active.
    active: bool,
    /// Parent namespace ID (NO_PARENT for root).
    parent: u32,
    /// Next local PID to allocate.
    next_pid: AtomicU32,
    /// Number of active processes in this namespace.
    nr_procs: u32,
    /// Mapping: global ProcessId → namespace-local PID.
    global_to_local: BTreeMap<GlobalPid, LocalPid>,
    /// Mapping: namespace-local PID → global ProcessId.
    local_to_global: BTreeMap<LocalPid, GlobalPid>,
    /// The global PID of the namespace's init process (local PID 1).
    /// 0 means no init yet.
    init_pid: GlobalPid,
    /// Number of direct child namespaces.
    nr_children: u32,
}

impl PidNamespace {
    /// Create an empty (inactive) namespace slot.
    fn new_empty() -> Self {
        Self {
            active: false,
            parent: NO_PARENT,
            next_pid: AtomicU32::new(1),
            nr_procs: 0,
            global_to_local: BTreeMap::new(),
            local_to_global: BTreeMap::new(),
            init_pid: 0,
            nr_children: 0,
        }
    }

    /// Reset to a freshly-created state under the given parent.
    fn init(&mut self, parent: u32) {
        self.active = true;
        self.parent = parent;
        self.next_pid.store(1, Ordering::Relaxed);
        self.nr_procs = 0;
        self.global_to_local.clear();
        self.local_to_global.clear();
        self.init_pid = 0;
        self.nr_children = 0;
    }
}

// ---------------------------------------------------------------------------
// Snapshot types for queries
// ---------------------------------------------------------------------------

/// Read-only snapshot of a PID namespace's state.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API — fields read by kshell, syscall handlers.
pub struct PidNsStats {
    /// Namespace ID.
    pub id: PidNsId,
    /// Whether this namespace is active.
    pub active: bool,
    /// Parent namespace ID (NO_PARENT for root).
    pub parent: u32,
    /// Number of processes in this namespace.
    pub nr_procs: u32,
    /// Number of child namespaces.
    pub nr_children: u32,
    /// Whether the namespace has an init process.
    pub has_init: bool,
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

struct PidNsTable {
    namespaces: Vec<PidNamespace>,
    /// Next ID to try when creating.
    next_id: u32,
}

impl PidNsTable {
    fn new() -> Self {
        let mut namespaces = Vec::with_capacity(MAX_NAMESPACES);
        for _ in 0..MAX_NAMESPACES {
            namespaces.push(PidNamespace::new_empty());
        }
        // Root namespace is always active.
        namespaces[0].active = true;
        namespaces[0].parent = NO_PARENT;
        Self {
            namespaces,
            next_id: 1,
        }
    }
}

static TABLE: Mutex<Option<PidNsTable>> = Mutex::new(None);

/// Initialize the PID namespace subsystem.
///
/// Called during boot, after the heap is available.
pub fn init() {
    let mut table = TABLE.lock();
    *table = Some(PidNsTable::new());
    serial_println!("[pidns] Initialized ({} max namespaces)", MAX_NAMESPACES);
}

/// Helper: get a mutable reference to the table, panicking if not initialized.
fn with_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut PidNsTable) -> R,
{
    let mut guard = TABLE.lock();
    let table = guard.as_mut().expect("[pidns] not initialized");
    f(table)
}

/// Helper: get an immutable reference to the table.
fn with_table_ref<F, R>(f: F) -> R
where
    F: FnOnce(&PidNsTable) -> R,
{
    let guard = TABLE.lock();
    let table = guard.as_ref().expect("[pidns] not initialized");
    f(table)
}

// ---------------------------------------------------------------------------
// Public API: lifecycle
// ---------------------------------------------------------------------------

/// Create a new PID namespace as a child of `parent`.
///
/// Returns the new namespace's ID.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `parent` doesn't exist.
/// - [`KernelError::ResourceExhausted`] if all namespace slots are full.
pub fn create(parent: PidNsId) -> KernelResult<PidNsId> {
    with_table(|table| {
        let parent_idx = parent as usize;

        // Validate parent.
        if parent_idx >= MAX_NAMESPACES || !table.namespaces[parent_idx].active {
            return Err(KernelError::InvalidArgument);
        }

        // Find a free slot.
        let start = table.next_id as usize;
        let mut found = None;
        for offset in 0..MAX_NAMESPACES {
            #[allow(clippy::arithmetic_side_effects)]
            let idx = (start + offset) % MAX_NAMESPACES;
            if idx == 0 {
                continue; // Root slot is reserved.
            }
            if !table.namespaces[idx].active {
                found = Some(idx);
                break;
            }
        }

        let idx = found.ok_or(KernelError::ResourceExhausted)?;

        table.namespaces[idx].init(parent);
        table.namespaces[parent_idx].nr_children =
            table.namespaces[parent_idx].nr_children.saturating_add(1);

        // Advance hint.
        #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
        {
            table.next_id = ((idx + 1) % MAX_NAMESPACES) as u32;
        }

        Ok(idx as PidNsId)
    })
}

/// Delete a PID namespace.
///
/// The namespace must have no processes and no child namespaces.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `id` is the root or doesn't exist.
/// - [`KernelError::NotEmpty`] if the namespace has processes or children.
pub fn delete(id: PidNsId) -> KernelResult<()> {
    if id == ROOT_NS {
        return Err(KernelError::InvalidArgument);
    }

    with_table(|table| {
        let idx = id as usize;

        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }

        if table.namespaces[idx].nr_procs > 0 {
            return Err(KernelError::NotEmpty);
        }
        if table.namespaces[idx].nr_children > 0 {
            return Err(KernelError::NotEmpty);
        }

        // Decrement parent's child count.
        let parent = table.namespaces[idx].parent as usize;
        if parent < MAX_NAMESPACES && table.namespaces[parent].active {
            table.namespaces[parent].nr_children =
                table.namespaces[parent].nr_children.saturating_sub(1);
        }

        table.namespaces[idx].active = false;
        table.namespaces[idx].parent = NO_PARENT;
        table.namespaces[idx].global_to_local.clear();
        table.namespaces[idx].local_to_global.clear();

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Public API: PID allocation and translation
// ---------------------------------------------------------------------------

/// Allocate a namespace-local PID for a process.
///
/// Called when a new process is created within this namespace.
/// Returns the namespace-local PID.
///
/// The first process in a namespace gets PID 1 (init).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the namespace doesn't exist.
/// - [`KernelError::ResourceExhausted`] if the namespace has too many PIDs.
pub fn alloc_pid(ns_id: PidNsId, global_pid: GlobalPid) -> KernelResult<LocalPid> {
    with_table(|table| {
        let idx = ns_id as usize;

        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }

        if table.namespaces[idx].nr_procs >= MAX_PIDS_PER_NS as u32 {
            return Err(KernelError::ResourceExhausted);
        }

        // Allocate next PID.
        let local_pid = table.namespaces[idx].next_pid.fetch_add(1, Ordering::Relaxed);

        // Insert bidirectional mapping.
        table.namespaces[idx].global_to_local.insert(global_pid, local_pid);
        table.namespaces[idx].local_to_global.insert(local_pid, global_pid);
        table.namespaces[idx].nr_procs = table.namespaces[idx].nr_procs.saturating_add(1);

        // First process is init (local PID 1).
        if local_pid == 1 {
            table.namespaces[idx].init_pid = global_pid;
        }

        Ok(local_pid)
    })
}

/// Free a namespace-local PID for a process that has exited.
///
/// Removes the PID mapping.  PIDs are not reused — the local PID
/// counter is monotonically increasing.
///
/// Returns `true` if this was the namespace's init process (PID 1),
/// meaning all other processes in the namespace should be cleaned up.
pub fn free_pid(ns_id: PidNsId, global_pid: GlobalPid) -> bool {
    with_table(|table| {
        let idx = ns_id as usize;

        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return false;
        }

        let was_init = table.namespaces[idx].init_pid == global_pid;

        if let Some(local) = table.namespaces[idx].global_to_local.remove(&global_pid) {
            table.namespaces[idx].local_to_global.remove(&local);
            table.namespaces[idx].nr_procs =
                table.namespaces[idx].nr_procs.saturating_sub(1);
        }

        if was_init {
            table.namespaces[idx].init_pid = 0;
        }

        was_init
    })
}

/// Translate a global ProcessId to a namespace-local PID.
///
/// Returns `None` if the process is not in this namespace.
#[must_use]
pub fn translate_to_local(ns_id: PidNsId, global_pid: GlobalPid) -> Option<LocalPid> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        table.namespaces[idx].global_to_local.get(&global_pid).copied()
    })
}

/// Translate a namespace-local PID to a global ProcessId.
///
/// Returns `None` if the PID doesn't exist in this namespace.
#[must_use]
pub fn translate_to_global(ns_id: PidNsId, local_pid: LocalPid) -> Option<GlobalPid> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        table.namespaces[idx].local_to_global.get(&local_pid).copied()
    })
}

/// List all global PIDs in a namespace.
///
/// Returns a vector of (local_pid, global_pid) pairs.
#[must_use]
pub fn list_pids(ns_id: PidNsId) -> Vec<(LocalPid, GlobalPid)> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Vec::new();
        }
        table.namespaces[idx]
            .local_to_global
            .iter()
            .map(|(&l, &g)| (l, g))
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Public API: visibility and hierarchy
// ---------------------------------------------------------------------------

/// Check if `viewer_ns` can see processes in `target_ns`.
///
/// A namespace can see itself and all descendant namespaces.
/// The root namespace can see everything.
#[must_use]
pub fn is_visible(viewer_ns: PidNsId, target_ns: PidNsId) -> bool {
    if viewer_ns == ROOT_NS {
        return true; // Root sees everything.
    }
    if viewer_ns == target_ns {
        return true; // Same namespace.
    }

    // Walk up from target to see if we reach viewer.
    with_table_ref(|table| {
        let mut current = target_ns as usize;
        for _ in 0..MAX_NAMESPACES {
            if current >= MAX_NAMESPACES || !table.namespaces[current].active {
                return false;
            }
            let parent = table.namespaces[current].parent;
            if parent == NO_PARENT {
                return false; // Reached root without finding viewer.
            }
            if parent == viewer_ns {
                return true; // Found viewer in ancestor chain.
            }
            current = parent as usize;
        }
        false
    })
}

/// Get the namespace's init process global PID.
///
/// Returns `None` if the namespace doesn't exist or has no init.
#[must_use]
#[allow(dead_code)] // Public API for namespace cleanup.
pub fn init_pid(ns_id: PidNsId) -> Option<GlobalPid> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        let pid = table.namespaces[idx].init_pid;
        if pid == 0 { None } else { Some(pid) }
    })
}

/// Get the parent namespace of a given namespace.
///
/// Returns `None` for the root namespace or invalid IDs.
#[must_use]
#[allow(dead_code)] // Public API for namespace traversal.
pub fn parent_ns(ns_id: PidNsId) -> Option<PidNsId> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        let parent = table.namespaces[idx].parent;
        if parent == NO_PARENT { None } else { Some(parent) }
    })
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Get statistics for a PID namespace.
#[must_use]
pub fn stats(id: PidNsId) -> Option<PidNsStats> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        let ns = &table.namespaces[idx];
        Some(PidNsStats {
            id,
            active: true,
            parent: ns.parent,
            nr_procs: ns.nr_procs,
            nr_children: ns.nr_children,
            has_init: ns.init_pid != 0,
        })
    })
}

/// Check if a namespace exists.
#[must_use]
pub fn exists(id: PidNsId) -> bool {
    with_table_ref(|table| {
        let idx = id as usize;
        idx < MAX_NAMESPACES && table.namespaces[idx].active
    })
}

/// Count active namespaces.
#[must_use]
pub fn active_count() -> usize {
    with_table_ref(|table| {
        table.namespaces.iter().filter(|ns| ns.active).count()
    })
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for PID namespaces.
pub fn self_test() {
    serial_println!("[pidns] Running self-test...");

    // Test 1: Root namespace exists.
    assert!(exists(ROOT_NS), "root namespace must exist");
    assert_eq!(active_count(), 1, "only root at startup");
    serial_println!("[pidns]   Root exists: OK");

    // Test 2: Create child namespaces.
    let ns1 = create(ROOT_NS).expect("create ns1");
    assert!(ns1 > 0);
    assert!(exists(ns1));
    assert_eq!(active_count(), 2);

    let ns2 = create(ROOT_NS).expect("create ns2");
    assert!(exists(ns2));
    assert_ne!(ns1, ns2);
    assert_eq!(active_count(), 3);
    serial_println!("[pidns]   Create namespaces: OK");

    // Test 3: Create nested namespace.
    let ns1_child = create(ns1).expect("create ns1_child");
    assert!(exists(ns1_child));
    assert_eq!(active_count(), 4);
    let s = stats(ns1).unwrap();
    assert_eq!(s.nr_children, 1);
    serial_println!("[pidns]   Nested namespace: OK");

    // Test 4: Invalid parent rejected.
    assert!(create(100).is_err());
    serial_println!("[pidns]   Invalid parent rejected: OK");

    // Test 5: Allocate PIDs.
    let pid1 = alloc_pid(ns1, 100).expect("alloc pid for global 100");
    assert_eq!(pid1, 1, "first process should get PID 1 (init)");
    let pid2 = alloc_pid(ns1, 101).expect("alloc pid for global 101");
    assert_eq!(pid2, 2, "second process gets PID 2");
    let pid3 = alloc_pid(ns1, 102).expect("alloc pid for global 102");
    assert_eq!(pid3, 3);

    let s = stats(ns1).unwrap();
    assert_eq!(s.nr_procs, 3);
    assert!(s.has_init);
    serial_println!("[pidns]   Alloc PIDs: OK");

    // Test 6: PID translation.
    assert_eq!(translate_to_local(ns1, 100), Some(1));
    assert_eq!(translate_to_local(ns1, 101), Some(2));
    assert_eq!(translate_to_local(ns1, 999), None); // Not in this ns.
    assert_eq!(translate_to_global(ns1, 1), Some(100));
    assert_eq!(translate_to_global(ns1, 2), Some(101));
    assert_eq!(translate_to_global(ns1, 99), None);
    serial_println!("[pidns]   PID translation: OK");

    // Test 7: List PIDs.
    let pids = list_pids(ns1);
    assert_eq!(pids.len(), 3);
    serial_println!("[pidns]   List PIDs: OK");

    // Test 8: Free PIDs.
    let was_init = free_pid(ns1, 101); // PID 2, not init.
    assert!(!was_init);
    assert_eq!(translate_to_local(ns1, 101), None); // Gone.
    assert_eq!(translate_to_global(ns1, 2), None);
    let s = stats(ns1).unwrap();
    assert_eq!(s.nr_procs, 2);

    // Free init (global PID 100 = local PID 1).
    let was_init = free_pid(ns1, 100);
    assert!(was_init, "freeing PID 1 should report init exit");
    let s = stats(ns1).unwrap();
    assert!(!s.has_init);
    serial_println!("[pidns]   Free PIDs (init detection): OK");

    // Test 9: Visibility — root sees everything.
    assert!(is_visible(ROOT_NS, ns1));
    assert!(is_visible(ROOT_NS, ns2));
    assert!(is_visible(ROOT_NS, ns1_child));

    // Test 10: Visibility — same namespace.
    assert!(is_visible(ns1, ns1));

    // Test 11: Visibility — parent sees child.
    assert!(is_visible(ns1, ns1_child));

    // Test 12: Visibility — child cannot see parent.
    assert!(!is_visible(ns1_child, ns1));

    // Test 13: Visibility — siblings cannot see each other.
    assert!(!is_visible(ns1, ns2));
    assert!(!is_visible(ns2, ns1));
    serial_println!("[pidns]   Visibility hierarchy: OK");

    // Test 14: Init PID query.
    // ns1 init was freed — should be None.
    assert!(init_pid(ns1).is_none());
    // Allocate a new process as init in ns2.
    let _ = alloc_pid(ns2, 200);
    assert_eq!(init_pid(ns2), Some(200));
    serial_println!("[pidns]   Init PID query: OK");

    // Test 15: Parent namespace query.
    assert_eq!(parent_ns(ROOT_NS), None);
    assert_eq!(parent_ns(ns1), Some(ROOT_NS));
    assert_eq!(parent_ns(ns1_child), Some(ns1));
    serial_println!("[pidns]   Parent namespace query: OK");

    // Test 16: Delete requires empty.
    assert!(delete(ns1).is_err(), "ns1 has procs and children");

    // Free remaining PIDs.
    free_pid(ns1, 102); // Last proc in ns1.
    assert!(delete(ns1).is_err(), "ns1 still has child ns");

    // Delete child first.
    delete(ns1_child).expect("delete empty child");
    assert!(!exists(ns1_child));
    delete(ns1).expect("delete ns1 now empty");
    assert!(!exists(ns1));
    serial_println!("[pidns]   Delete lifecycle: OK");

    // Test 17: Delete root is forbidden.
    assert!(delete(ROOT_NS).is_err());
    serial_println!("[pidns]   Root delete protection: OK");

    // Test 18: Separate namespace PID spaces.
    let nsa = create(ROOT_NS).expect("create nsa");
    let nsb = create(ROOT_NS).expect("create nsb");
    let pa = alloc_pid(nsa, 500).expect("alloc in nsa");
    let pb = alloc_pid(nsb, 600).expect("alloc in nsb");
    // Both get PID 1 in their respective namespaces.
    assert_eq!(pa, 1);
    assert_eq!(pb, 1);
    // But they map to different globals.
    assert_eq!(translate_to_global(nsa, 1), Some(500));
    assert_eq!(translate_to_global(nsb, 1), Some(600));
    // Cross-namespace lookup fails.
    assert_eq!(translate_to_local(nsa, 600), None);
    assert_eq!(translate_to_local(nsb, 500), None);
    serial_println!("[pidns]   Separate PID spaces: OK");

    // Cleanup.
    free_pid(nsa, 500);
    free_pid(nsb, 600);
    free_pid(ns2, 200);
    delete(nsa).expect("delete nsa");
    delete(nsb).expect("delete nsb");
    delete(ns2).expect("delete ns2");
    assert_eq!(active_count(), 1, "only root remains");
    serial_println!("[pidns]   Cleanup: OK");

    serial_println!("[pidns] Self-test PASSED (18 tests)");
}
