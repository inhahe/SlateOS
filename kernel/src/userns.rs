//! User Namespaces — UID/GID isolation for containers.
//!
//! Provides Linux-style user namespace isolation.  Each user namespace
//! defines a mapping between container-internal UIDs/GIDs and host
//! (root namespace) UIDs/GIDs.
//!
//! ## Design
//!
//! User namespaces allow unprivileged processes to appear as root
//! (UID 0) inside a container while actually running as a non-root
//! user on the host.  This is a core building block for rootless
//! containers.
//!
//! - **UID/GID mapping**: Each namespace defines up to
//!   [`MAX_MAPPINGS`] (16) ranges that translate between inner and
//!   outer IDs.  E.g., inner UID 0-999 → outer UID 100000-100999.
//! - **Hierarchy**: User namespaces form a tree.  The root namespace
//!   (ID 0) has identity mappings (inner == outer).  Child namespaces
//!   define mappings relative to their parent.
//! - **Ownership**: Each user namespace has an owner — the UID in the
//!   parent namespace that created it.  The owner has full privileges
//!   within the child namespace.
//!
//! ## Integration Points
//!
//! - **Capability checks**: When checking if a process can access a
//!   resource, translate the process's UID through its namespace
//!   mapping to get the host-visible UID for permission checks.
//! - **File ownership display**: When showing file owners, translate
//!   the stored (host) UID through the viewer's namespace mapping to
//!   show the namespace-local UID.
//! - **Process credentials**: `proc/pcb.rs` credentials store the
//!   global (host) UID/GID.  Syscalls that return or accept UIDs
//!   must translate through the namespace.
//!
//! ## References
//!
//! - Linux `kernel/user_namespace.c`
//! - `man 7 user_namespaces`
//! - Design spec: container primitives for Docker support

use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of user namespaces.
pub const MAX_NAMESPACES: usize = 64;

/// Maximum UID/GID mapping ranges per namespace.
pub const MAX_MAPPINGS: usize = 16;

/// The root user namespace.  Always exists, identity mapping.
pub const ROOT_NS: UserNsId = 0;

/// Sentinel value for "no parent" (root namespace).
const NO_PARENT: u32 = u32::MAX;

/// Overflow UID returned when a mapping doesn't cover the ID.
///
/// Matches Linux's `(uid_t)-1` convention.  A process whose UID
/// doesn't map into the viewer's namespace appears as this value.
pub const OVERFLOW_ID: u32 = 65534;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a user namespace.
pub type UserNsId = u32;

/// A single UID or GID mapping range.
///
/// Maps `count` IDs starting at `inner_start` (namespace-internal)
/// to IDs starting at `outer_start` (parent namespace).
#[derive(Debug, Clone, Copy)]
pub struct IdMapping {
    /// Start of the range inside the namespace.
    pub inner_start: u32,
    /// Start of the range in the parent namespace.
    pub outer_start: u32,
    /// Number of IDs in this range.
    pub count: u32,
}

impl IdMapping {
    /// Check if `inner_id` falls within this mapping range.
    #[inline]
    fn contains_inner(&self, inner_id: u32) -> bool {
        inner_id >= self.inner_start
            && inner_id.checked_sub(self.inner_start)
                .is_some_and(|offset| offset < self.count)
    }

    /// Check if `outer_id` falls within this mapping range.
    #[inline]
    fn contains_outer(&self, outer_id: u32) -> bool {
        outer_id >= self.outer_start
            && outer_id.checked_sub(self.outer_start)
                .is_some_and(|offset| offset < self.count)
    }

    /// Translate an inner ID to an outer ID.
    #[inline]
    fn to_outer(self, inner_id: u32) -> Option<u32> {
        if !self.contains_inner(inner_id) {
            return None;
        }
        inner_id.checked_sub(self.inner_start)
            .and_then(|offset| self.outer_start.checked_add(offset))
    }

    /// Translate an outer ID to an inner ID.
    #[inline]
    fn to_inner(self, outer_id: u32) -> Option<u32> {
        if !self.contains_outer(outer_id) {
            return None;
        }
        outer_id.checked_sub(self.outer_start)
            .and_then(|offset| self.inner_start.checked_add(offset))
    }
}

// ---------------------------------------------------------------------------
// Per-namespace data
// ---------------------------------------------------------------------------

/// A user namespace node.
struct UserNamespace {
    /// Whether this slot is active.
    active: bool,
    /// Parent namespace ID.
    parent: u32,
    /// UID of the creator in the parent namespace.
    owner_uid: u32,
    /// UID mapping table.
    uid_map: Vec<IdMapping>,
    /// GID mapping table.
    gid_map: Vec<IdMapping>,
    /// Number of child namespaces.
    nr_children: u32,
    /// Number of processes using this namespace.
    nr_procs: u32,
}

impl UserNamespace {
    fn new_empty() -> Self {
        Self {
            active: false,
            parent: NO_PARENT,
            owner_uid: 0,
            uid_map: Vec::new(),
            gid_map: Vec::new(),
            nr_children: 0,
            nr_procs: 0,
        }
    }

    fn init(&mut self, parent: u32, owner_uid: u32) {
        self.active = true;
        self.parent = parent;
        self.owner_uid = owner_uid;
        self.uid_map.clear();
        self.gid_map.clear();
        self.nr_children = 0;
        self.nr_procs = 0;
    }
}

// ---------------------------------------------------------------------------
// Snapshot type
// ---------------------------------------------------------------------------

/// Read-only snapshot of a user namespace's state.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API — fields read by kshell and syscall handlers.
pub struct UserNsStats {
    /// Namespace ID.
    pub id: UserNsId,
    /// Whether active.
    pub active: bool,
    /// Parent namespace ID.
    pub parent: u32,
    /// Owner UID (in parent namespace).
    pub owner_uid: u32,
    /// Number of UID mapping ranges.
    pub uid_map_count: usize,
    /// Number of GID mapping ranges.
    pub gid_map_count: usize,
    /// Number of child namespaces.
    pub nr_children: u32,
    /// Number of processes in this namespace.
    pub nr_procs: u32,
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

struct UserNsTable {
    namespaces: Vec<UserNamespace>,
    next_id: u32,
}

impl UserNsTable {
    fn new() -> Self {
        let mut namespaces = Vec::with_capacity(MAX_NAMESPACES);
        for _ in 0..MAX_NAMESPACES {
            namespaces.push(UserNamespace::new_empty());
        }
        // Root namespace: active, identity mapping (inner == outer).
        namespaces[0].active = true;
        namespaces[0].parent = NO_PARENT;
        namespaces[0].owner_uid = 0; // root
        Self {
            namespaces,
            next_id: 1,
        }
    }
}

static TABLE: Mutex<Option<UserNsTable>> = Mutex::new(None);

/// Initialize the user namespace subsystem.
pub fn init() {
    let mut table = TABLE.lock();
    *table = Some(UserNsTable::new());
    serial_println!("[userns] Initialized ({} max namespaces)", MAX_NAMESPACES);
}

fn with_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut UserNsTable) -> R,
{
    let mut guard = TABLE.lock();
    let table = guard.as_mut().expect("[userns] not initialized");
    f(table)
}

fn with_table_ref<F, R>(f: F) -> R
where
    F: FnOnce(&UserNsTable) -> R,
{
    let guard = TABLE.lock();
    let table = guard.as_ref().expect("[userns] not initialized");
    f(table)
}

// ---------------------------------------------------------------------------
// Public API: lifecycle
// ---------------------------------------------------------------------------

/// Create a new user namespace as a child of `parent`.
///
/// `owner_uid` is the UID of the creating process in the parent
/// namespace.  This UID has full privileges within the child.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `parent` doesn't exist.
/// - [`KernelError::ResourceExhausted`] if all slots are full.
pub fn create(parent: UserNsId, owner_uid: u32) -> KernelResult<UserNsId> {
    with_table(|table| {
        let parent_idx = parent as usize;

        if parent_idx >= MAX_NAMESPACES || !table.namespaces[parent_idx].active {
            return Err(KernelError::InvalidArgument);
        }

        // Find a free slot.
        let start = table.next_id as usize;
        let mut found = None;
        for offset in 0..MAX_NAMESPACES {
            #[allow(clippy::arithmetic_side_effects)]
            let idx = (start + offset) % MAX_NAMESPACES;
            if idx == 0 { continue; }
            if !table.namespaces[idx].active {
                found = Some(idx);
                break;
            }
        }

        let idx = found.ok_or(KernelError::ResourceExhausted)?;

        table.namespaces[idx].init(parent, owner_uid);
        table.namespaces[parent_idx].nr_children =
            table.namespaces[parent_idx].nr_children.saturating_add(1);

        #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
        {
            table.next_id = ((idx + 1) % MAX_NAMESPACES) as u32;
        }

        Ok(idx as UserNsId)
    })
}

/// Delete a user namespace.
///
/// Must have no processes and no children.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `id` is root or doesn't exist.
/// - [`KernelError::NotEmpty`] if has processes or children.
pub fn delete(id: UserNsId) -> KernelResult<()> {
    if id == ROOT_NS {
        return Err(KernelError::InvalidArgument);
    }

    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.namespaces[idx].nr_procs > 0 || table.namespaces[idx].nr_children > 0 {
            return Err(KernelError::NotEmpty);
        }

        let parent = table.namespaces[idx].parent as usize;
        if parent < MAX_NAMESPACES && table.namespaces[parent].active {
            table.namespaces[parent].nr_children =
                table.namespaces[parent].nr_children.saturating_sub(1);
        }

        table.namespaces[idx].active = false;
        table.namespaces[idx].parent = NO_PARENT;
        table.namespaces[idx].uid_map.clear();
        table.namespaces[idx].gid_map.clear();

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Public API: mapping configuration
// ---------------------------------------------------------------------------

/// Add a UID mapping range to a namespace.
///
/// Maps `count` UIDs starting at `inner_start` (inside the namespace)
/// to UIDs starting at `outer_start` (in the parent namespace).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if namespace doesn't exist or
///   `count` is 0.
/// - [`KernelError::ResourceExhausted`] if too many mappings.
pub fn add_uid_mapping(
    ns_id: UserNsId,
    inner_start: u32,
    outer_start: u32,
    count: u32,
) -> KernelResult<()> {
    if count == 0 {
        return Err(KernelError::InvalidArgument);
    }

    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.namespaces[idx].uid_map.len() >= MAX_MAPPINGS {
            return Err(KernelError::ResourceExhausted);
        }

        table.namespaces[idx].uid_map.push(IdMapping {
            inner_start,
            outer_start,
            count,
        });

        Ok(())
    })
}

/// Add a GID mapping range to a namespace.
///
/// Same as [`add_uid_mapping`] but for group IDs.
pub fn add_gid_mapping(
    ns_id: UserNsId,
    inner_start: u32,
    outer_start: u32,
    count: u32,
) -> KernelResult<()> {
    if count == 0 {
        return Err(KernelError::InvalidArgument);
    }

    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.namespaces[idx].gid_map.len() >= MAX_MAPPINGS {
            return Err(KernelError::ResourceExhausted);
        }

        table.namespaces[idx].gid_map.push(IdMapping {
            inner_start,
            outer_start,
            count,
        });

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Public API: UID/GID translation
// ---------------------------------------------------------------------------

/// Translate a namespace-internal UID to a parent (outer) UID.
///
/// Returns `OVERFLOW_ID` if the inner UID doesn't map to any
/// outer UID.  For the root namespace, returns the UID unchanged
/// (identity mapping).
#[must_use]
pub fn uid_to_outer(ns_id: UserNsId, inner_uid: u32) -> u32 {
    if ns_id == ROOT_NS {
        return inner_uid; // Root namespace: identity.
    }

    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return OVERFLOW_ID;
        }

        for mapping in &table.namespaces[idx].uid_map {
            if let Some(outer) = mapping.to_outer(inner_uid) {
                return outer;
            }
        }

        OVERFLOW_ID
    })
}

/// Translate a parent (outer) UID to a namespace-internal UID.
///
/// Returns `OVERFLOW_ID` if the outer UID doesn't map into the
/// namespace.  For the root namespace, returns the UID unchanged.
#[must_use]
pub fn uid_to_inner(ns_id: UserNsId, outer_uid: u32) -> u32 {
    if ns_id == ROOT_NS {
        return outer_uid; // Root namespace: identity.
    }

    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return OVERFLOW_ID;
        }

        for mapping in &table.namespaces[idx].uid_map {
            if let Some(inner) = mapping.to_inner(outer_uid) {
                return inner;
            }
        }

        OVERFLOW_ID
    })
}

/// Translate a namespace-internal GID to a parent (outer) GID.
#[must_use]
pub fn gid_to_outer(ns_id: UserNsId, inner_gid: u32) -> u32 {
    if ns_id == ROOT_NS {
        return inner_gid;
    }

    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return OVERFLOW_ID;
        }

        for mapping in &table.namespaces[idx].gid_map {
            if let Some(outer) = mapping.to_outer(inner_gid) {
                return outer;
            }
        }

        OVERFLOW_ID
    })
}

/// Translate a parent (outer) GID to a namespace-internal GID.
#[must_use]
pub fn gid_to_inner(ns_id: UserNsId, outer_gid: u32) -> u32 {
    if ns_id == ROOT_NS {
        return outer_gid;
    }

    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return OVERFLOW_ID;
        }

        for mapping in &table.namespaces[idx].gid_map {
            if let Some(inner) = mapping.to_inner(outer_gid) {
                return inner;
            }
        }

        OVERFLOW_ID
    })
}

/// Translate a UID from one namespace to the host (root namespace).
///
/// Walks up the namespace hierarchy, translating through each
/// parent's mapping until reaching the root.  Returns the host UID.
///
/// This is the full translation needed for permission checks:
/// "what host UID does inner UID X in namespace N correspond to?"
#[must_use]
pub fn uid_to_host(ns_id: UserNsId, inner_uid: u32) -> u32 {
    if ns_id == ROOT_NS {
        return inner_uid;
    }

    with_table_ref(|table| {
        let mut current_ns = ns_id as usize;
        let mut uid = inner_uid;

        for _ in 0..MAX_NAMESPACES {
            if current_ns >= MAX_NAMESPACES || !table.namespaces[current_ns].active {
                return OVERFLOW_ID;
            }
            if current_ns == ROOT_NS as usize {
                return uid; // Reached root — uid is now the host uid.
            }

            // Translate through this namespace's mapping.
            let mut found = false;
            for mapping in &table.namespaces[current_ns].uid_map {
                if let Some(outer) = mapping.to_outer(uid) {
                    uid = outer;
                    found = true;
                    break;
                }
            }
            if !found {
                return OVERFLOW_ID;
            }

            let parent = table.namespaces[current_ns].parent;
            if parent == NO_PARENT {
                return uid; // Parent is root.
            }
            current_ns = parent as usize;
        }

        OVERFLOW_ID
    })
}

// ---------------------------------------------------------------------------
// Public API: process tracking
// ---------------------------------------------------------------------------

/// Increment the process count for a namespace.
pub fn attach_process(ns_id: UserNsId) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.namespaces[idx].nr_procs =
            table.namespaces[idx].nr_procs.saturating_add(1);
        Ok(())
    })
}

/// Decrement the process count for a namespace.
pub fn detach_process(ns_id: UserNsId) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.namespaces[idx].nr_procs =
            table.namespaces[idx].nr_procs.saturating_sub(1);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Get statistics for a user namespace.
#[must_use]
pub fn stats(id: UserNsId) -> Option<UserNsStats> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        let ns = &table.namespaces[idx];
        Some(UserNsStats {
            id,
            active: true,
            parent: ns.parent,
            owner_uid: ns.owner_uid,
            uid_map_count: ns.uid_map.len(),
            gid_map_count: ns.gid_map.len(),
            nr_children: ns.nr_children,
            nr_procs: ns.nr_procs,
        })
    })
}

/// Check if a namespace exists.
#[must_use]
pub fn exists(id: UserNsId) -> bool {
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

/// Get the UID mappings for a namespace.
#[must_use]
pub fn uid_mappings(id: UserNsId) -> Vec<IdMapping> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Vec::new();
        }
        table.namespaces[idx].uid_map.clone()
    })
}

/// Get the GID mappings for a namespace.
#[must_use]
pub fn gid_mappings(id: UserNsId) -> Vec<IdMapping> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Vec::new();
        }
        table.namespaces[idx].gid_map.clone()
    })
}

/// Get the owner UID of a namespace.
#[must_use]
#[allow(dead_code)] // Public API for privilege checks.
pub fn owner_uid(id: UserNsId) -> Option<u32> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        Some(table.namespaces[idx].owner_uid)
    })
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for user namespaces.
pub fn self_test() {
    serial_println!("[userns] Running self-test...");

    // Test 1: Root namespace exists.
    assert!(exists(ROOT_NS));
    assert_eq!(active_count(), 1);
    serial_println!("[userns]   Root exists: OK");

    // Test 2: Create child namespace.
    let ns1 = create(ROOT_NS, 1000).expect("create ns1");
    assert!(ns1 > 0);
    assert!(exists(ns1));
    assert_eq!(active_count(), 2);
    serial_println!("[userns]   Create namespace: OK");

    // Test 3: Root namespace identity mapping.
    assert_eq!(uid_to_outer(ROOT_NS, 0), 0);
    assert_eq!(uid_to_outer(ROOT_NS, 1000), 1000);
    assert_eq!(uid_to_inner(ROOT_NS, 0), 0);
    assert_eq!(gid_to_outer(ROOT_NS, 42), 42);
    serial_println!("[userns]   Root identity mapping: OK");

    // Test 4: Add UID mapping — inner 0-999 → outer 100000-100999.
    add_uid_mapping(ns1, 0, 100_000, 1000).expect("add uid mapping");
    assert_eq!(uid_to_outer(ns1, 0), 100_000);  // root inside → 100000 outside
    assert_eq!(uid_to_outer(ns1, 999), 100_999);
    assert_eq!(uid_to_outer(ns1, 1000), OVERFLOW_ID); // Unmapped.
    serial_println!("[userns]   UID inner→outer: OK");

    // Test 5: Reverse UID translation.
    assert_eq!(uid_to_inner(ns1, 100_000), 0);
    assert_eq!(uid_to_inner(ns1, 100_500), 500);
    assert_eq!(uid_to_inner(ns1, 99_999), OVERFLOW_ID); // Not mapped.
    assert_eq!(uid_to_inner(ns1, 101_000), OVERFLOW_ID);
    serial_println!("[userns]   UID outer→inner: OK");

    // Test 6: GID mapping.
    add_gid_mapping(ns1, 0, 200_000, 500).expect("add gid mapping");
    assert_eq!(gid_to_outer(ns1, 0), 200_000);
    assert_eq!(gid_to_outer(ns1, 499), 200_499);
    assert_eq!(gid_to_outer(ns1, 500), OVERFLOW_ID);
    assert_eq!(gid_to_inner(ns1, 200_000), 0);
    assert_eq!(gid_to_inner(ns1, 200_250), 250);
    serial_println!("[userns]   GID mapping: OK");

    // Test 7: Host UID translation (single level).
    assert_eq!(uid_to_host(ns1, 0), 100_000);
    assert_eq!(uid_to_host(ns1, 42), 100_042);
    assert_eq!(uid_to_host(ns1, 1500), OVERFLOW_ID); // Unmapped.
    serial_println!("[userns]   UID to host (single level): OK");

    // Test 8: Nested namespace — two levels of mapping.
    let ns2 = create(ns1, 0).expect("create ns2 under ns1");
    // ns2: inner 0-99 → outer 500-599 (which are inner UIDs in ns1)
    add_uid_mapping(ns2, 0, 500, 100).expect("add uid mapping ns2");
    // ns2 inner 0 → ns1 inner 500 → root 100500
    assert_eq!(uid_to_outer(ns2, 0), 500);
    assert_eq!(uid_to_host(ns2, 0), 100_500);
    assert_eq!(uid_to_host(ns2, 50), 100_550);
    assert_eq!(uid_to_host(ns2, 100), OVERFLOW_ID); // ns2 only maps 0-99
    serial_println!("[userns]   Nested UID to host (two levels): OK");

    // Test 9: Multiple mapping ranges.
    let ns3 = create(ROOT_NS, 0).expect("create ns3");
    // Range 1: inner 0-99 → outer 10000-10099
    add_uid_mapping(ns3, 0, 10_000, 100).expect("range 1");
    // Range 2: inner 1000-1999 → outer 20000-20999
    add_uid_mapping(ns3, 1000, 20_000, 1000).expect("range 2");

    assert_eq!(uid_to_outer(ns3, 0), 10_000);
    assert_eq!(uid_to_outer(ns3, 50), 10_050);
    assert_eq!(uid_to_outer(ns3, 500), OVERFLOW_ID); // Gap, unmapped.
    assert_eq!(uid_to_outer(ns3, 1000), 20_000);
    assert_eq!(uid_to_outer(ns3, 1999), 20_999);
    serial_println!("[userns]   Multiple mapping ranges: OK");

    // Test 10: Owner UID.
    assert_eq!(owner_uid(ns1), Some(1000));
    assert_eq!(owner_uid(ns2), Some(0)); // Created by "root" in ns1.
    serial_println!("[userns]   Owner UID: OK");

    // Test 11: Process tracking.
    attach_process(ns1).expect("attach");
    attach_process(ns1).expect("attach");
    let s = stats(ns1).unwrap();
    assert_eq!(s.nr_procs, 2);
    detach_process(ns1).expect("detach");
    let s = stats(ns1).unwrap();
    assert_eq!(s.nr_procs, 1);
    detach_process(ns1).expect("detach");
    serial_println!("[userns]   Process tracking: OK");

    // Test 12: Stats query.
    let s = stats(ns1).unwrap();
    assert_eq!(s.uid_map_count, 1);
    assert_eq!(s.gid_map_count, 1);
    assert_eq!(s.owner_uid, 1000);
    serial_println!("[userns]   Stats: OK");

    // Test 13: Delete non-empty fails.
    assert!(delete(ns1).is_err(), "ns1 has child ns2");
    serial_println!("[userns]   Delete non-empty rejected: OK");

    // Test 14: Delete root fails.
    assert!(delete(ROOT_NS).is_err());
    serial_println!("[userns]   Root delete protection: OK");

    // Test 15: Zero-count mapping rejected.
    assert!(add_uid_mapping(ns1, 0, 0, 0).is_err());
    serial_println!("[userns]   Zero-count mapping rejected: OK");

    // Cleanup.
    delete(ns2).expect("delete ns2");
    delete(ns1).expect("delete ns1");
    delete(ns3).expect("delete ns3");
    assert_eq!(active_count(), 1);
    serial_println!("[userns]   Cleanup: OK");

    serial_println!("[userns] Self-test PASSED (15 tests)");
}
