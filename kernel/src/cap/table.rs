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

use super::{ResourceType, Rights};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum entries per capability table.
///
/// Public so that callers who *fill* a table can bound their input against the
/// real limit instead of a number they picked (`SYS_PROCESS_SPAWN_EX2`'s
/// capability subset does this). A cap larger than this cannot succeed anyway,
/// so accepting it only means doing the copy before refusing.
pub const MAX_ENTRIES: usize = 4096;

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
    /// Rights put aside, for a while: part of root's authority, while the
    /// process's effective user id is not 0 but another of its ids is
    /// ([`CapTable::suspend`], `pcb::change_credentials`) -- Linux's
    /// permitted capabilities, out of the effective set. They permit nothing
    /// until [`CapTable::restore`] adds them back to `rights`, and they go
    /// with the entry: a revocation takes them too, and a handle given away
    /// or copied carries `rights` alone. An entry with every right put aside
    /// stays in the table, valid, permitting nothing.
    pub suspended: Rights,
    /// Whether this entry is still valid (false = revoked).
    pub valid: bool,
}

/// A capability table for a single task (or process).
///
/// Handles are allocated as sequential u64 values.  The table is
/// a `BTreeMap` so handles are sparse (removed entries don't leave
/// gaps that could be confused with valid handles).
///
/// `Clone` duplicates the entire table, which is what `fork()` needs:
/// the child inherits the parent's capabilities (authority), each as an
/// independent copy keyed by the same handle values so the child's
/// copy-on-write address space (which references those handle values)
/// stays valid.
#[derive(Debug, Clone)]
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
        self.insert_with_suspended(resource_type, resource_id, rights, Rights::NONE)
    }

    /// [`insert`](Self::insert), with `suspended` rights put aside on the new
    /// entry: a spawned child's copy of a parent's entry whose root rights
    /// are put aside (`pcb::inherit_caps_from`), as a forked child's table is
    /// a copy of the parent's.
    ///
    /// # Errors
    ///
    /// - `InvalidArgument` — table is at capacity.
    pub fn insert_with_suspended(
        &mut self,
        resource_type: ResourceType,
        resource_id: u64,
        rights: Rights,
        suspended: Rights,
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
            suspended,
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
    pub fn check_rights(&self, handle: CapHandle, required: Rights) -> KernelResult<&CapEntry> {
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
    pub fn duplicate(&mut self, source: CapHandle, new_rights: Rights) -> KernelResult<CapHandle> {
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

    /// Every valid entry, in handle order.
    ///
    /// The counterpart to [`count`](Self::count), and the two must agree:
    /// `valid_entries().len() == count()` is the invariant `SYS_CAP_QUERY`
    /// relies on when a caller probes for a size and then asks for the data.
    /// Both filter on `valid`, so a revoked entry is absent from both.
    ///
    /// Handle values are deliberately **not** returned. What the caller of an
    /// enumeration wants to know is what authority exists, not which slot
    /// holds it; a handle is a token that can be *used*, so putting one in an
    /// informational list invites exactly that. `entries` is a `BTreeMap`
    /// keyed by handle, so the iteration order is nonetheless deterministic
    /// (ascending handle, i.e. grant order) rather than an artefact of the
    /// hash of the moment.
    ///
    /// Returns owned clones rather than references: a borrow into the map
    /// would tie its lifetime to the `PROCESS_TABLE` lock, and the whole point
    /// of the enumeration path is that the lock is *dropped* before user
    /// memory is touched (a fault there re-enters the same lock).
    #[must_use]
    pub fn valid_entries(&self) -> Vec<CapEntry> {
        self.entries.values().filter(|e| e.valid).cloned().collect()
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

    /// Check if the table contains a valid capability for the specified
    /// resource *type* with sufficient rights, ignoring resource ID.
    ///
    /// Used for "does this process have *any* File capability with READ
    /// rights?" style queries where the specific resource doesn't matter
    /// (e.g., path-based filesystem operations, general network access).
    #[must_use]
    pub fn has_capability_type(
        &self,
        resource_type: ResourceType,
        required_rights: Rights,
    ) -> bool {
        self.entries.values().any(|e| {
            e.valid && e.resource_type == resource_type && e.rights.contains(required_rights)
        })
    }

    /// Drain all valid entries from the table, consuming them.
    ///
    /// Returns a list of `(resource_type, resource_id)` pairs for all
    /// valid entries.  Used during process cleanup to identify all IPC
    /// resources that need to be released.
    #[allow(dead_code)]
    pub fn drain_valid(&mut self) -> Vec<(ResourceType, u64)> {
        let mut result = Vec::new();
        for entry in self.entries.values() {
            if entry.valid {
                result.push((entry.resource_type, entry.resource_id));
            }
        }
        self.entries.clear();
        result
    }

    /// Narrow every valid entry to the rights `keep` returns for it; an entry
    /// left with no rights is revoked. Returns how many entries changed.
    ///
    /// For a process losing a class of authority in one step -- its uid
    /// leaving 0 (`pcb::change_credentials`) -- where entry-by-entry handle
    /// calls would let a concurrent reader see it half-narrowed.
    pub fn narrow_all(&mut self, keep: impl Fn(&CapEntry) -> Rights) -> usize {
        let mut changed = 0usize;
        for entry in self.entries.values_mut() {
            if !entry.valid {
                continue;
            }
            let kept = keep(entry);
            if kept == entry.rights {
                continue;
            }
            changed = changed.saturating_add(1);
            if kept.is_empty() {
                entry.valid = false;
            } else {
                entry.rights = kept;
            }
        }
        changed
    }

    /// Put aside, in every valid entry, the rights `keep` does not return for
    /// it: they stay in the entry, as [`CapEntry::suspended`], and permit
    /// nothing until [`restore`](Self::restore). An entry left with no rights
    /// stays in the table, valid -- so a revocation or a resource's
    /// destruction meanwhile still finds it, and nothing a revocation took
    /// comes back. Returns how many entries had rights put aside.
    ///
    /// Root's authority while the process's effective user id is not 0 but
    /// another of its ids is (`pcb::change_credentials`).
    pub fn suspend(&mut self, keep: impl Fn(&CapEntry) -> Rights) -> usize {
        let mut changed = 0usize;
        for entry in self.entries.values_mut() {
            if !entry.valid {
                continue;
            }
            let gone = entry.rights.remove(keep(entry));
            if gone.is_empty() {
                continue;
            }
            entry.rights = entry.rights.remove(gone);
            entry.suspended = entry.suspended.union(gone);
            changed = changed.saturating_add(1);
        }
        changed
    }

    /// Give every valid entry back the rights [`suspend`](Self::suspend) put
    /// aside in it. Returns how many entries got rights back.
    pub fn restore(&mut self) -> usize {
        let mut changed = 0usize;
        for entry in self.entries.values_mut() {
            if !entry.valid || entry.suspended.is_empty() {
                continue;
            }
            entry.rights = entry.rights.union(entry.suspended);
            entry.suspended = Rights::NONE;
            changed = changed.saturating_add(1);
        }
        changed
    }

    /// Forget every right put aside, for good -- the process's user ids have
    /// all left 0 -- revoking an entry left with none. Returns how many
    /// entries lost rights.
    pub fn drop_suspended(&mut self) -> usize {
        let mut changed = 0usize;
        for entry in self.entries.values_mut() {
            if !entry.valid || entry.suspended.is_empty() {
                continue;
            }
            entry.suspended = Rights::NONE;
            if entry.rights.is_empty() {
                entry.valid = false;
            }
            changed = changed.saturating_add(1);
        }
        changed
    }

    /// How many valid entries have rights put aside.
    #[must_use]
    pub fn suspended_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| e.valid && !e.suspended.is_empty())
            .count()
    }

    /// Revoke all entries referencing a specific resource.
    ///
    /// Called when a kernel object is destroyed (e.g., channel closed).
    /// Returns the number of entries revoked.
    pub fn revoke_by_resource(&mut self, resource_type: ResourceType, resource_id: u64) -> usize {
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
// Self-test
// ---------------------------------------------------------------------------

/// [`CapTable::suspend`], [`CapTable::restore`] and
/// [`CapTable::drop_suspended`]: a right put aside permits nothing and comes
/// back; an entry left with none stays, valid and useless, and comes back
/// whole; a revocation meanwhile is final; a copy carries no right put aside;
/// and dropping what was put aside revokes an entry left with nothing.
fn test_suspend_restore() -> KernelResult<()> {
    let fail = |what: &str| {
        serial_println!("[cap/table]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    let both = Rights::READ.union(Rights::SET_CREDENTIALS);
    // Keep everything but the clock's WRITE and SET_CREDENTIALS anywhere.
    let keep = |e: &CapEntry| {
        if e.resource_type == ResourceType::SystemClock {
            Rights::NONE
        } else {
            e.rights.remove(Rights::SET_CREDENTIALS)
        }
    };
    let mut t = CapTable::new();
    let clock = t.insert(ResourceType::SystemClock, 0, Rights::WRITE)?;
    // DUPLICATE too, which `duplicate` asks of the entry it copies: a copy is
    // made of this one below. Without it the copy was refused, and the test
    // failed the boot with `PermissionDenied` before reaching its point.
    let own = t.insert(ResourceType::Process, 7, both.union(Rights::DUPLICATE))?;
    let file = t.insert(ResourceType::File, 0, Rights::READ)?;
    let gone = t.insert(ResourceType::Process, 8, both)?;
    if t.suspend(keep) != 3 || t.suspended_count() != 3 {
        return fail("suspend did not put aside rights in the three entries it should");
    }
    if t.lookup(clock).is_err() || t.check_rights(clock, Rights::WRITE).is_ok() {
        return fail("an entry left with nothing left the table, or still permitted");
    }
    if t.check_rights(own, Rights::SET_CREDENTIALS).is_ok()
        || t.check_rights(own, Rights::READ).is_err()
        || t.check_rights(file, Rights::READ).is_err()
        || t.has_capability_type(ResourceType::SystemClock, Rights::WRITE)
    {
        return fail("suspend put aside the wrong rights");
    }
    // A copy made meanwhile carries what may be used, nothing put aside.
    let copy = t.duplicate(own, Rights::READ)?;
    // A revocation meanwhile is final.
    t.revoke(gone);
    if t.restore() != 2 {
        return fail("restore did not give back the two entries still valid");
    }
    if t.check_rights(clock, Rights::WRITE).is_err()
        || t.check_rights(own, both).is_err()
        || t.check_rights(file, Rights::READ).is_err()
        || t.check_rights(copy, Rights::SET_CREDENTIALS).is_ok()
    {
        return fail("restore did not put the rights back where they were");
    }
    if t.lookup(gone).is_ok() {
        return fail("restore undid a revocation");
    }
    // For good: what is put aside is dropped, and an entry left with nothing
    // is revoked.
    t.suspend(keep);
    if t.drop_suspended() != 2 || t.suspended_count() != 0 || t.restore() != 0 {
        return fail("drop_suspended did not forget what was put aside");
    }
    if t.lookup(clock).is_ok()
        || t.check_rights(own, Rights::SET_CREDENTIALS).is_ok()
        || t.check_rights(own, Rights::READ).is_err()
    {
        return fail("drop_suspended left the clock, or took READ");
    }
    // A copy of an entry with rights put aside carries them when asked to.
    let mut u = CapTable::new();
    let h = u.insert_with_suspended(ResourceType::Process, 9, Rights::READ, both)?;
    if u.restore() != 1 || u.check_rights(h, both).is_err() {
        return fail("insert_with_suspended did not put the rights aside");
    }
    serial_println!("[cap/table]   suspend, restore and drop_suspended: OK");
    Ok(())
}

/// Run capability table self-tests.
pub fn self_test() -> KernelResult<()> {
    test_insert_and_lookup()?;
    test_rights_check()?;
    test_suspend_restore()?;
    test_duplicate()?;
    test_revoke()?;
    test_revoke_by_resource()?;
    test_delegation_cannot_escalate()?;
    test_has_capability_type()?;
    test_valid_entries()?;

    Ok(())
}

/// Test 1: insert and lookup.
fn test_insert_and_lookup() -> KernelResult<()> {
    let mut table = CapTable::new();

    let handle = table.insert(ResourceType::Channel, 42, Rights::READ_WRITE)?;

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

    let handle = table.insert(ResourceType::Pipe, 7, Rights::READ)?;

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

    let handle = table.insert(ResourceType::EventFd, 55, Rights::READ | Rights::SIGNAL)?;

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

    let original = table.insert(ResourceType::Pipe, 99, Rights::READ | Rights::DUPLICATE)?;

    // Try to duplicate with WRITE (which original doesn't have).
    match table.duplicate(original, Rights::READ | Rights::WRITE) {
        Err(KernelError::PermissionDenied) => {} // Expected.
        other => {
            serial_println!("[cap]   FAIL: escalation should be denied: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[cap]   No escalation: OK");
    Ok(())
}

/// Test 7: `has_capability_type` checks type+rights, ignoring resource ID.
fn test_has_capability_type() -> KernelResult<()> {
    let mut table = CapTable::new();

    // Insert a File capability with READ+WRITE for resource_id 42.
    table.insert(ResourceType::File, 42, Rights::READ | Rights::WRITE)?;

    // Type-level check should match regardless of resource_id.
    if !table.has_capability_type(ResourceType::File, Rights::READ) {
        serial_println!("[cap]   FAIL: has_capability_type should find File+READ");
        return Err(KernelError::InternalError);
    }

    // Should also find WRITE.
    if !table.has_capability_type(ResourceType::File, Rights::WRITE) {
        serial_println!("[cap]   FAIL: has_capability_type should find File+WRITE");
        return Err(KernelError::InternalError);
    }

    // Should NOT find EXECUTE (not granted).
    if table.has_capability_type(ResourceType::File, Rights::EXECUTE) {
        serial_println!("[cap]   FAIL: has_capability_type should not find File+EXECUTE");
        return Err(KernelError::InternalError);
    }

    // Should NOT find Socket (wrong type).
    if table.has_capability_type(ResourceType::Socket, Rights::READ) {
        serial_println!("[cap]   FAIL: has_capability_type should not find Socket+READ");
        return Err(KernelError::InternalError);
    }

    // Existing resource-specific check should still work for exact ID.
    if !table.has_resource(ResourceType::File, 42, Rights::READ) {
        serial_println!("[cap]   FAIL: has_resource should find File+42+READ");
        return Err(KernelError::InternalError);
    }

    // Exact ID mismatch should fail.
    if table.has_resource(ResourceType::File, 99, Rights::READ) {
        serial_println!("[cap]   FAIL: has_resource should NOT find File+99+READ");
        return Err(KernelError::InternalError);
    }

    serial_println!("[cap]   has_capability_type: OK");
    Ok(())
}

/// Test 8: `valid_entries` agrees with `count`, and hides what it should hide.
///
/// This is the invariant `SYS_CAP_QUERY` rests on: a caller probes with a null
/// buffer (answered from `count`), allocates that many slots, then asks for the
/// data (answered from `valid_entries`).  If the two ever disagreed, the second
/// call would either overflow the caller's buffer or silently under-report
/// authority the process genuinely holds — and under-reporting is the direction
/// nobody notices, because the missing capability just looks like a permission
/// the process was never given.
fn test_valid_entries() -> KernelResult<()> {
    let mut table = CapTable::new();

    // Empty table: both accessors must agree that there is nothing.
    if table.count() != 0 || !table.valid_entries().is_empty() {
        serial_println!("[cap]   FAIL: empty table should have no valid entries");
        return Err(KernelError::InternalError);
    }

    // Grant three capabilities of distinct types, in a known order.
    let h_chan = table.insert(ResourceType::Channel, 10, Rights::READ_WRITE)?;
    let h_pipe = table.insert(ResourceType::Pipe, 20, Rights::READ)?;
    // Bound but never used by handle: this one is revoked *by resource* below,
    // which is a third path to invalidity that the enumeration must also honour.
    let _h_file = table.insert(ResourceType::File, 30, Rights::READ | Rights::WRITE)?;

    let entries = table.valid_entries();
    if entries.len() != table.count() || entries.len() != 3 {
        serial_println!("[cap]   FAIL: valid_entries/count disagree after 3 inserts");
        return Err(KernelError::InternalError);
    }

    // Order is ascending handle, i.e. grant order — not an artefact of hashing.
    // A caller that renders the list wants it stable across calls.
    //
    // Compared as a whole slice rather than element by element: `a[0]`, `a[1]`
    // in kernel code is a panic waiting for the day the length check above is
    // edited apart from the reads below.  A self-test that panics takes the
    // kernel down instead of printing FAIL, which is a strictly worse way to
    // learn the same thing.
    let ids: Vec<u64> = entries.iter().map(|e| e.resource_id).collect();
    if ids.as_slice() != [10, 20, 30] {
        serial_println!("[cap]   FAIL: valid_entries not in grant order: {:?}", ids);
        return Err(KernelError::InternalError);
    }

    // Contents survive the copy: type and rights are what was granted, and — the
    // half that matters — rights are not *widened* by the copy.
    let chan_ok = entries.iter().any(|e| {
        e.resource_type == ResourceType::Channel
            && e.resource_id == 10
            && e.rights.contains(Rights::WRITE)
    });
    let pipe_ok = entries.iter().any(|e| {
        e.resource_type == ResourceType::Pipe
            && e.resource_id == 20
            && !e.rights.contains(Rights::WRITE)
    });
    if !chan_ok || !pipe_ok {
        serial_println!("[cap]   FAIL: valid_entries lost type or rights");
        return Err(KernelError::InternalError);
    }

    // Revoking the middle one must remove it from *both* accessors, and must
    // not disturb the relative order of the survivors.
    if !table.revoke(h_pipe) {
        serial_println!("[cap]   FAIL: revoke of a live handle returned false");
        return Err(KernelError::InternalError);
    }
    let entries = table.valid_entries();
    if entries.len() != table.count() || entries.len() != 2 {
        serial_println!("[cap]   FAIL: valid_entries/count disagree after revoke");
        return Err(KernelError::InternalError);
    }
    if entries
        .iter()
        .any(|e| e.resource_type == ResourceType::Pipe)
    {
        serial_println!("[cap]   FAIL: revoked entry still enumerated");
        return Err(KernelError::InternalError);
    }
    let ids: Vec<u64> = entries.iter().map(|e| e.resource_id).collect();
    if ids.as_slice() != [10, 30] {
        serial_println!(
            "[cap]   FAIL: revoke disturbed enumeration order: {:?}",
            ids
        );
        return Err(KernelError::InternalError);
    }

    // `remove` frees the slot outright; the two accessors must still agree.
    if table.remove(h_chan).is_none() {
        serial_println!("[cap]   FAIL: remove of a live handle returned None");
        return Err(KernelError::InternalError);
    }
    let entries = table.valid_entries();
    if entries.len() != table.count() || entries.len() != 1 {
        serial_println!("[cap]   FAIL: valid_entries/count disagree after remove");
        return Err(KernelError::InternalError);
    }
    let ids: Vec<u64> = entries.iter().map(|e| e.resource_id).collect();
    if ids.as_slice() != [30] {
        serial_println!("[cap]   FAIL: wrong survivor after remove: {:?}", ids);
        return Err(KernelError::InternalError);
    }

    // `revoke_by_resource` is the third way an entry can stop being valid;
    // it must be invisible to the enumeration too.
    if table.revoke_by_resource(ResourceType::File, 30) != 1 {
        serial_println!("[cap]   FAIL: revoke_by_resource should revoke exactly 1");
        return Err(KernelError::InternalError);
    }
    if !table.valid_entries().is_empty() || table.count() != 0 {
        serial_println!("[cap]   FAIL: valid_entries/count disagree after revoke_by_resource");
        return Err(KernelError::InternalError);
    }
    serial_println!("[cap]   valid_entries: OK");
    Ok(())
}
