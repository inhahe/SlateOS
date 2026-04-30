//! Capability table — per-task storage of capability handles.
//!
//! Each task (eventually each process) has its own capability table.
//! Handles are small integers (indices) that are meaningless outside
//! the owning task's table.
//!
//! ## Current Implementation
//!
//! For now (kernel-only, single address space), a single global
//! capability table serves all tasks.  When per-process address
//! spaces are added (§1.6), the table will move into the process
//! control block and handles will be per-process.
//!
//! ## Capacity
//!
//! Each table has a fixed maximum number of entries.  This prevents
//! a single task from consuming unbounded kernel memory via handle
//! accumulation.
//!
//! ## Revocation
//!
//! When a resource is destroyed (e.g., a channel is closed), any
//! capability entries referencing it become stale.  The `revoke()`
//! function marks an entry as invalid.  Subsequent operations using
//! the handle will return `InvalidCapability`.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use super::{ResourceType, Rights};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum entries per capability table.
const MAX_ENTRIES: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A handle into a capability table.
///
/// Handles are opaque to the holder — just a u64 index.  The actual
/// resource binding is stored kernel-side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapHandle(u64);

impl CapHandle {
    /// Reconstruct from a raw value.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A single entry in the capability table.
///
/// Binds a handle to a specific kernel resource with specific rights.
#[derive(Debug, Clone)]
pub struct CapEntry {
    /// What type of resource this refers to.
    pub resource_type: ResourceType,
    /// The kernel-internal identifier for the resource (e.g., channel
    /// ID, pipe ID, etc.).
    pub resource_id: u64,
    /// What operations this capability permits.
    pub rights: Rights,
    /// Whether this entry is still valid (false = revoked).
    pub valid: bool,
}

/// A capability table for a single task (or process).
///
/// Handles are allocated as sequential u64 values.  The table is
/// a `BTreeMap` so handles are sparse (removed entries don't leave
/// gaps that could be confused with valid handles).
pub struct CapTable {
    /// The entries, keyed by handle value.
    entries: BTreeMap<u64, CapEntry>,
    /// Counter for allocating new handles.
    next_handle: u64,
}

impl CapTable {
    /// Create a new empty capability table.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_handle: 1, // 0 is reserved as "null handle".
        }
    }

    /// Insert a new capability, returning the handle.
    ///
    /// # Errors
    ///
    /// - `InvalidArgument` — table is at capacity.
    pub fn insert(
        &mut self,
        resource_type: ResourceType,
        resource_id: u64,
        rights: Rights,
    ) -> KernelResult<CapHandle> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err(KernelError::InvalidArgument);
        }

        let handle_val = self.next_handle;
        // Handle counter overflow is practically impossible (2^64),
        // but be safe.
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.next_handle = self.next_handle.wrapping_add(1);
        }
        if self.next_handle == 0 {
            self.next_handle = 1; // Skip 0.
        }

        let entry = CapEntry {
            resource_type,
            resource_id,
            rights,
            valid: true,
        };
        self.entries.insert(handle_val, entry);

        Ok(CapHandle(handle_val))
    }

    /// Look up a capability entry by handle.
    ///
    /// Returns the entry if the handle is valid and not revoked.
    ///
    /// # Errors
    ///
    /// - `InvalidCapability` — handle not found or revoked.
    pub fn lookup(&self, handle: CapHandle) -> KernelResult<&CapEntry> {
        let entry = self
            .entries
            .get(&handle.0)
            .ok_or(KernelError::InvalidCapability)?;

        if !entry.valid {
            return Err(KernelError::InvalidCapability);
        }

        Ok(entry)
    }

    /// Check if a handle has the required rights.
    ///
    /// # Errors
    ///
    /// - `InvalidCapability` — handle not found or revoked.
    /// - `PermissionDenied` — handle exists but lacks the required rights.
    pub fn check_rights(
        &self,
        handle: CapHandle,
        required: Rights,
    ) -> KernelResult<&CapEntry> {
        let entry = self.lookup(handle)?;

        if !entry.rights.contains(required) {
            return Err(KernelError::PermissionDenied);
        }

        Ok(entry)
    }

    /// Duplicate a capability with a (possibly reduced) set of rights.
    ///
    /// The new capability refers to the same resource but may have
    /// fewer rights.  You cannot add rights that the original doesn't
    /// have.
    ///
    /// # Errors
    ///
    /// - `InvalidCapability` — source handle not found or revoked.
    /// - `PermissionDenied` — source doesn't have DUPLICATE right,
    ///   or `new_rights` is not a subset of the source's rights.
    /// - `InvalidArgument` — table is at capacity.
    pub fn duplicate(
        &mut self,
        source: CapHandle,
        new_rights: Rights,
    ) -> KernelResult<CapHandle> {
        // Look up the source — must exist and be valid.
        let entry = self
            .entries
            .get(&source.0)
            .ok_or(KernelError::InvalidCapability)?;

        if !entry.valid {
            return Err(KernelError::InvalidCapability);
        }

        // Source must have the DUPLICATE right.
        if !entry.rights.contains(Rights::DUPLICATE) {
            return Err(KernelError::PermissionDenied);
        }

        // New rights must be a subset of source rights.
        if !new_rights.is_subset_of(entry.rights) {
            return Err(KernelError::PermissionDenied);
        }

        // Clone the entry info before mutating self.
        let rtype = entry.resource_type;
        let rid = entry.resource_id;

        self.insert(rtype, rid, new_rights)
    }

    /// Revoke a capability.
    ///
    /// The handle becomes invalid.  Subsequent lookups will return
    /// `InvalidCapability`.  Does not remove the entry — the handle
    /// slot is marked invalid so it can't be confused with a new
    /// allocation.
    ///
    /// Returns `true` if the entry was found and revoked, `false` if
    /// the handle didn't exist (not an error — idempotent).
    pub fn revoke(&mut self, handle: CapHandle) -> bool {
        if let Some(entry) = self.entries.get_mut(&handle.0) {
            entry.valid = false;
            true
        } else {
            false
        }
    }

    /// Remove a capability entirely (give up the handle).
    ///
    /// Unlike revoke (which marks invalid), this frees the slot.
    ///
    /// Returns the entry if it existed.
    pub fn remove(&mut self, handle: CapHandle) -> Option<CapEntry> {
        self.entries.remove(&handle.0)
    }

    /// How many valid entries are in the table.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.values().filter(|e| e.valid).count()
    }

    /// Check if the table contains a valid capability for the specified
    /// resource with sufficient rights.
    ///
    /// Used for implicit capability checks (e.g., does this process
    /// hold a Process capability for PID X with DELETE rights?).
    #[must_use]
    pub fn has_resource(
        &self,
        resource_type: ResourceType,
        resource_id: u64,
        required_rights: Rights,
    ) -> bool {
        self.entries.values().any(|e| {
            e.valid
                && e.resource_type == resource_type
                && e.resource_id == resource_id
                && e.rights.contains(required_rights)
        })
    }

    /// Revoke all entries referencing a specific resource.
    ///
    /// Called when a kernel object is destroyed (e.g., channel closed).
    /// Returns the number of entries revoked.
    pub fn revoke_by_resource(
        &mut self,
        resource_type: ResourceType,
        resource_id: u64,
    ) -> usize {
        let mut count = 0;
        for entry in self.entries.values_mut() {
            if entry.valid
                && entry.resource_type == resource_type
                && entry.resource_id == resource_id
            {
                entry.valid = false;
                count += 1;
            }
        }
        count
    }
}

// ---------------------------------------------------------------------------
// Global table (temporary — until per-process tables)
// ---------------------------------------------------------------------------

/// Global capability table ID counter.
static NEXT_TABLE_ID: AtomicU64 = AtomicU64::new(1);

/// Global registry of all capability tables, keyed by task/process ID.
///
/// In the future, each process will own its `CapTable` directly in
/// its PCB.  For now, we store them in a global map keyed by an ID.
static TABLES: Mutex<BTreeMap<u64, CapTable>> = Mutex::new(BTreeMap::new());

/// Create a new capability table and return its ID.
///
/// Called when a new task or process is created.
pub fn create_table() -> u64 {
    let id = NEXT_TABLE_ID.fetch_add(1, Ordering::Relaxed);
    let table = CapTable::new();
    let mut tables = TABLES.lock();
    tables.insert(id, table);
    id
}

/// Remove a capability table (called on task/process exit).
pub fn destroy_table(table_id: u64) {
    let mut tables = TABLES.lock();
    tables.remove(&table_id);
}

/// Execute an operation on a task's capability table.
///
/// This is the primary interface for syscall handlers: look up the
/// caller's table and perform an operation on it.
///
/// # Errors
///
/// - `InvalidHandle` — the table ID doesn't exist.
pub fn with_table<F, R>(table_id: u64, f: F) -> KernelResult<R>
where
    F: FnOnce(&mut CapTable) -> KernelResult<R>,
{
    let mut tables = TABLES.lock();
    let table = tables
        .get_mut(&table_id)
        .ok_or(KernelError::InvalidHandle)?;
    f(table)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run capability table self-tests.
pub fn self_test() -> KernelResult<()> {
    test_insert_and_lookup()?;
    test_rights_check()?;
    test_duplicate()?;
    test_revoke()?;
    test_revoke_by_resource()?;
    test_delegation_cannot_escalate()?;

    Ok(())
}

/// Test 1: insert and lookup.
fn test_insert_and_lookup() -> KernelResult<()> {
    let mut table = CapTable::new();

    let handle = table.insert(
        ResourceType::Channel,
        42,
        Rights::READ_WRITE,
    )?;

    let entry = table.lookup(handle)?;
    if entry.resource_type != ResourceType::Channel {
        serial_println!("[cap]   FAIL: resource type mismatch");
        return Err(KernelError::InternalError);
    }
    if entry.resource_id != 42 {
        serial_println!("[cap]   FAIL: resource id mismatch");
        return Err(KernelError::InternalError);
    }
    if !entry.rights.contains(Rights::READ) {
        serial_println!("[cap]   FAIL: missing READ right");
        return Err(KernelError::InternalError);
    }

    serial_println!("[cap]   Insert + lookup: OK");
    Ok(())
}

/// Test 2: rights check passes and fails correctly.
fn test_rights_check() -> KernelResult<()> {
    let mut table = CapTable::new();

    let handle = table.insert(
        ResourceType::Pipe,
        7,
        Rights::READ,
    )?;

    // READ should pass.
    table.check_rights(handle, Rights::READ)?;

    // WRITE should fail.
    match table.check_rights(handle, Rights::WRITE) {
        Err(KernelError::PermissionDenied) => {} // Expected.
        other => {
            serial_println!("[cap]   FAIL: write check on read-only: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[cap]   Rights check: OK");
    Ok(())
}

/// Test 3: duplicate with reduced rights.
fn test_duplicate() -> KernelResult<()> {
    let mut table = CapTable::new();

    let original = table.insert(
        ResourceType::SharedMemory,
        100,
        Rights::READ_WRITE, // r+w+wait+dup
    )?;

    // Duplicate with only read rights.
    let dup = table.duplicate(original, Rights::READ_ONLY)?;

    let dup_entry = table.lookup(dup)?;
    if dup_entry.resource_id != 100 {
        serial_println!("[cap]   FAIL: dup resource id mismatch");
        return Err(KernelError::InternalError);
    }
    if dup_entry.rights.contains(Rights::WRITE) {
        serial_println!("[cap]   FAIL: dup should not have WRITE");
        return Err(KernelError::InternalError);
    }
    if !dup_entry.rights.contains(Rights::READ) {
        serial_println!("[cap]   FAIL: dup should have READ");
        return Err(KernelError::InternalError);
    }

    serial_println!("[cap]   Duplicate: OK");
    Ok(())
}

/// Test 4: revoke invalidates a handle.
fn test_revoke() -> KernelResult<()> {
    let mut table = CapTable::new();

    let handle = table.insert(
        ResourceType::EventFd,
        55,
        Rights::READ | Rights::SIGNAL,
    )?;

    // Should be valid.
    table.lookup(handle)?;

    // Revoke.
    let revoked = table.revoke(handle);
    if !revoked {
        serial_println!("[cap]   FAIL: revoke returned false");
        return Err(KernelError::InternalError);
    }

    // Should now fail.
    match table.lookup(handle) {
        Err(KernelError::InvalidCapability) => {} // Expected.
        other => {
            serial_println!("[cap]   FAIL: lookup after revoke: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[cap]   Revoke: OK");
    Ok(())
}

/// Test 5: revoke by resource invalidates all entries for that resource.
fn test_revoke_by_resource() -> KernelResult<()> {
    let mut table = CapTable::new();

    let h1 = table.insert(ResourceType::Channel, 10, Rights::READ)?;
    let h2 = table.insert(ResourceType::Channel, 10, Rights::WRITE)?;
    let h3 = table.insert(ResourceType::Channel, 20, Rights::READ)?;

    // Revoke all entries for channel 10.
    let count = table.revoke_by_resource(ResourceType::Channel, 10);
    if count != 2 {
        serial_println!(
            "[cap]   FAIL: revoke_by_resource count {}, expected 2",
            count
        );
        return Err(KernelError::InternalError);
    }

    // h1 and h2 should be invalid.
    match table.lookup(h1) {
        Err(KernelError::InvalidCapability) => {}
        other => {
            serial_println!("[cap]   FAIL: h1 after revoke: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }
    match table.lookup(h2) {
        Err(KernelError::InvalidCapability) => {}
        other => {
            serial_println!("[cap]   FAIL: h2 after revoke: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // h3 should still be valid (different resource_id).
    table.lookup(h3)?;

    serial_println!("[cap]   Revoke by resource: OK");
    Ok(())
}

/// Test 6: delegation cannot escalate rights.
fn test_delegation_cannot_escalate() -> KernelResult<()> {
    let mut table = CapTable::new();

    let original = table.insert(
        ResourceType::Pipe,
        99,
        Rights::READ | Rights::DUPLICATE,
    )?;

    // Try to duplicate with WRITE (which original doesn't have).
    match table.duplicate(original, Rights::READ | Rights::WRITE) {
        Err(KernelError::PermissionDenied) => {} // Expected.
        other => {
            serial_println!(
                "[cap]   FAIL: escalation should be denied: {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[cap]   No escalation: OK");
    Ok(())
}
