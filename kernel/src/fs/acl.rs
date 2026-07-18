//! POSIX-style Access Control Lists (ACLs) for fine-grained permissions.
//!
//! ACLs extend the basic Unix owner/group/other permission model with
//! per-user and per-group entries.  A file can have an ACL that grants
//! user 1001 read access even though the file's group permissions don't
//! include that user.
//!
//! ## Design
//!
//! ```text
//! VFS permission check
//!         ↓
//!   1. Owner check (uid match → use owner perms)
//!   2. Named user ACL check (uid in ACL → intersect with mask)
//!   3. Group check (gid match → intersect with mask)
//!   4. Named group ACL check (gid in ACL → intersect with mask)
//!   5. Other permissions (fallback)
//! ```
//!
//! ## POSIX ACL semantics
//!
//! - **ACL_USER_OBJ**: the file owner's permissions (maps to `chmod u`).
//! - **ACL_USER**: a named user entry (uid + permissions).
//! - **ACL_GROUP_OBJ**: the owning group's permissions (maps to `chmod g`).
//! - **ACL_GROUP**: a named group entry (gid + permissions).
//! - **ACL_MASK**: maximum permissions for named user/group entries and
//!   the owning group.  The effective permissions are the intersection
//!   of the entry's permissions and the mask.
//! - **ACL_OTHER**: permissions for everyone else (maps to `chmod o`).
//!
//! ## Storage
//!
//! ACLs are stored in the VFS xattr system under the key
//! `system.posix_acl_access`.  This is compatible with Linux ext4's
//! ACL storage.  In-memory, a BTreeMap caches ACLs for fast lookup.
//!
//! ## Reference
//!
//! POSIX 1003.1e draft 17 (ACL specification)
//! Linux: `man acl`, `man getfacl`, `man setfacl`

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// ACL entry tag type — identifies what the entry applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AclTag {
    /// File owner's permissions (ACL_USER_OBJ).
    UserObj,
    /// Named user entry (ACL_USER).
    User(u32),
    /// Owning group's permissions (ACL_GROUP_OBJ).
    GroupObj,
    /// Named group entry (ACL_GROUP).
    Group(u32),
    /// Maximum effective permissions for named entries (ACL_MASK).
    Mask,
    /// Everyone else (ACL_OTHER).
    Other,
}

/// Permission bits for an ACL entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AclPerm(pub u8);

#[allow(dead_code)]
impl AclPerm {
    /// Read permission.
    pub const READ: Self = Self(4);
    /// Write permission.
    pub const WRITE: Self = Self(2);
    /// Execute permission.
    pub const EXECUTE: Self = Self(1);
    /// All permissions (rwx).
    pub const ALL: Self = Self(7);
    /// No permissions.
    pub const NONE: Self = Self(0);

    /// Check if read is granted.
    #[inline]
    pub const fn can_read(self) -> bool {
        (self.0 & 4) != 0
    }

    /// Check if write is granted.
    #[inline]
    pub const fn can_write(self) -> bool {
        (self.0 & 2) != 0
    }

    /// Check if execute is granted.
    #[inline]
    pub const fn can_execute(self) -> bool {
        (self.0 & 1) != 0
    }

    /// Intersect with another permission set (bitwise AND).
    #[inline]
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Union with another permission set (bitwise OR).
    #[inline]
    #[allow(dead_code)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Format as "rwx" string.
    pub fn as_str(self) -> &'static str {
        match self.0 & 7 {
            0 => "---",
            1 => "--x",
            2 => "-w-",
            3 => "-wx",
            4 => "r--",
            5 => "r-x",
            6 => "rw-",
            7 => "rwx",
            _ => "???",
        }
    }
}

/// A single ACL entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AclEntry {
    /// Tag identifying what this entry applies to.
    pub tag: AclTag,
    /// Granted permissions.
    pub perm: AclPerm,
}

/// A complete ACL for a file.
#[derive(Debug, Clone)]
pub struct Acl {
    /// ACL entries, sorted by tag.
    pub entries: Vec<AclEntry>,
}

/// Requested access type for permission checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessRequest(pub u8);

impl AccessRequest {
    /// Read access.
    pub const READ: Self = Self(4);
    /// Write access.
    pub const WRITE: Self = Self(2);
    /// Execute access.
    pub const EXECUTE: Self = Self(1);
    /// Read + write.
    #[allow(dead_code)]
    pub const RW: Self = Self(6);

    /// Check if this request is satisfied by the given permissions.
    #[inline]
    pub const fn is_satisfied_by(self, perm: AclPerm) -> bool {
        (perm.0 & self.0) == self.0
    }
}

/// Statistics about the ACL subsystem.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct AclStats {
    /// Number of files with ACLs.
    pub files_with_acls: usize,
    /// Total number of ACL entries across all files.
    pub total_entries: usize,
    /// Number of permission checks performed.
    pub checks_performed: u64,
    /// Number of permission denials.
    pub denials: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct AclInner {
    /// Path → ACL mapping.  Keyed by normalized absolute path.
    acls: BTreeMap<String, Acl>,
    /// Statistics counters.
    checks_performed: u64,
    denials: u64,
}

static ACLS: Mutex<AclInner> = Mutex::new(AclInner {
    acls: BTreeMap::new(),
    checks_performed: 0,
    denials: 0,
});

// ---------------------------------------------------------------------------
// Public API — ACL management
// ---------------------------------------------------------------------------

/// Set the ACL for a file path.
///
/// Replaces any existing ACL.  The ACL must contain at minimum:
/// - ACL_USER_OBJ (owner permissions)
/// - ACL_GROUP_OBJ (owning group permissions)
/// - ACL_OTHER (other permissions)
///
/// If named user or group entries are present, an ACL_MASK entry is
/// required.
pub fn set_acl(path: &str, acl: Acl) -> KernelResult<()> {
    // Validate: must have USER_OBJ, GROUP_OBJ, OTHER.
    let has_user_obj = acl.entries.iter().any(|e| e.tag == AclTag::UserObj);
    let has_group_obj = acl.entries.iter().any(|e| e.tag == AclTag::GroupObj);
    let has_other = acl.entries.iter().any(|e| e.tag == AclTag::Other);

    if !has_user_obj || !has_group_obj || !has_other {
        return Err(KernelError::InvalidArgument);
    }

    // If named entries exist, mask is required.
    let has_named = acl.entries.iter().any(|e| matches!(e.tag, AclTag::User(_) | AclTag::Group(_)));
    let has_mask = acl.entries.iter().any(|e| e.tag == AclTag::Mask);
    if has_named && !has_mask {
        return Err(KernelError::InvalidArgument);
    }

    ACLS.lock().acls.insert(String::from(path), acl);
    Ok(())
}

/// Get the ACL for a file path.
///
/// Returns None if no ACL is set (file uses only traditional permissions).
pub fn get_acl(path: &str) -> Option<Acl> {
    ACLS.lock().acls.get(path).cloned()
}

/// Remove the ACL from a file path.
///
/// After removal, the file uses only traditional permissions.
pub fn remove_acl(path: &str) -> bool {
    ACLS.lock().acls.remove(path).is_some()
}

/// Check whether a specific access request is permitted by the ACL.
///
/// Follows the POSIX ACL evaluation algorithm:
/// 1. If requester is file owner → use USER_OBJ perms
/// 2. If requester matches a named USER entry → use entry perms ∩ MASK
/// 3. If requester is in owning group → use GROUP_OBJ perms ∩ MASK
/// 4. If requester matches a named GROUP entry → use entry perms ∩ MASK
/// 5. Use OTHER perms
///
/// Returns Ok(()) if access is granted, Err(PermissionDenied) otherwise.
///
/// If no ACL exists for the path, returns Ok(()) (delegate to traditional
/// permission checks elsewhere in VFS).
pub fn check_access(
    path: &str,
    requester_uid: u32,
    requester_gid: u32,
    file_uid: u32,
    file_gid: u32,
    request: AccessRequest,
) -> KernelResult<()> {
    let mut inner = ACLS.lock();
    inner.checks_performed = inner.checks_performed.saturating_add(1);

    let acl = match inner.acls.get(path) {
        Some(acl) => acl,
        None => return Ok(()), // No ACL, defer to traditional permissions.
    };

    // Find the mask entry (if any).
    let mask = acl.entries.iter()
        .find(|e| e.tag == AclTag::Mask)
        .map(|e| e.perm)
        .unwrap_or(AclPerm::ALL);

    // Step 1: Owner check.
    if requester_uid == file_uid {
        if let Some(entry) = acl.entries.iter().find(|e| e.tag == AclTag::UserObj) {
            if request.is_satisfied_by(entry.perm) {
                return Ok(());
            }
            inner.denials = inner.denials.saturating_add(1);
            return Err(KernelError::PermissionDenied);
        }
    }

    // Step 2: Named user check.
    if let Some(entry) = acl.entries.iter().find(|e| e.tag == AclTag::User(requester_uid)) {
        let effective = entry.perm.intersect(mask);
        if request.is_satisfied_by(effective) {
            return Ok(());
        }
        inner.denials = inner.denials.saturating_add(1);
        return Err(KernelError::PermissionDenied);
    }

    // Step 3: Owning group check.
    if requester_gid == file_gid {
        if let Some(entry) = acl.entries.iter().find(|e| e.tag == AclTag::GroupObj) {
            let effective = entry.perm.intersect(mask);
            if request.is_satisfied_by(effective) {
                return Ok(());
            }
            // Don't immediately deny — check named groups first.
        }
    }

    // Step 4: Named group check.
    for entry in &acl.entries {
        if let AclTag::Group(gid) = entry.tag {
            if gid == requester_gid {
                let effective = entry.perm.intersect(mask);
                if request.is_satisfied_by(effective) {
                    return Ok(());
                }
            }
        }
    }

    // Step 5: Other.
    if let Some(entry) = acl.entries.iter().find(|e| e.tag == AclTag::Other) {
        if request.is_satisfied_by(entry.perm) {
            return Ok(());
        }
    }

    inner.denials = inner.denials.saturating_add(1);
    Err(KernelError::PermissionDenied)
}

/// List all paths that have ACLs set.
#[allow(dead_code)]
pub fn list_paths() -> Vec<String> {
    ACLS.lock().acls.keys().cloned().collect()
}

/// Get statistics about the ACL subsystem.
pub fn stats() -> AclStats {
    let inner = ACLS.lock();
    let total_entries: usize = inner.acls.values().map(|a| a.entries.len()).sum();
    AclStats {
        files_with_acls: inner.acls.len(),
        total_entries,
        checks_performed: inner.checks_performed,
        denials: inner.denials,
    }
}

/// Clear all ACLs (for testing).
#[allow(dead_code)]
pub fn clear() {
    let mut inner = ACLS.lock();
    inner.acls.clear();
    inner.checks_performed = 0;
    inner.denials = 0;
}

// ---------------------------------------------------------------------------
// ACL construction helpers
// ---------------------------------------------------------------------------

/// Build a minimal ACL from traditional Unix permissions.
///
/// Given the traditional rwxrwxrwx mode bits, creates an ACL with
/// USER_OBJ, GROUP_OBJ, and OTHER entries.
pub fn from_mode(mode: u16) -> Acl {
    let owner = AclPerm(((mode >> 6) & 7) as u8);
    let group = AclPerm(((mode >> 3) & 7) as u8);
    let other = AclPerm((mode & 7) as u8);

    Acl {
        entries: Vec::from([
            AclEntry { tag: AclTag::UserObj, perm: owner },
            AclEntry { tag: AclTag::GroupObj, perm: group },
            AclEntry { tag: AclTag::Other, perm: other },
        ]),
    }
}

/// Build an ACL with named user/group entries.
///
/// Automatically adds a MASK entry computed as the union of all
/// named entries and GROUP_OBJ.
pub fn build_acl(
    owner_perm: AclPerm,
    group_perm: AclPerm,
    other_perm: AclPerm,
    named_users: &[(u32, AclPerm)],
    named_groups: &[(u32, AclPerm)],
) -> Acl {
    let mut entries = Vec::with_capacity(3 + named_users.len() + named_groups.len() + 1);

    entries.push(AclEntry { tag: AclTag::UserObj, perm: owner_perm });

    for &(uid, perm) in named_users {
        entries.push(AclEntry { tag: AclTag::User(uid), perm });
    }

    entries.push(AclEntry { tag: AclTag::GroupObj, perm: group_perm });

    for &(gid, perm) in named_groups {
        entries.push(AclEntry { tag: AclTag::Group(gid), perm });
    }

    // Compute mask: union of GROUP_OBJ + all named entries.
    let mut mask_bits = group_perm.0;
    for &(_, perm) in named_users {
        mask_bits |= perm.0;
    }
    for &(_, perm) in named_groups {
        mask_bits |= perm.0;
    }
    entries.push(AclEntry { tag: AclTag::Mask, perm: AclPerm(mask_bits) });

    entries.push(AclEntry { tag: AclTag::Other, perm: other_perm });

    Acl { entries }
}

/// Format an ACL entry as a human-readable string.
pub fn format_entry(entry: &AclEntry) -> String {
    match entry.tag {
        AclTag::UserObj => alloc::format!("user::{}",  entry.perm.as_str()),
        AclTag::User(uid) => alloc::format!("user:{}:{}", uid, entry.perm.as_str()),
        AclTag::GroupObj => alloc::format!("group::{}", entry.perm.as_str()),
        AclTag::Group(gid) => alloc::format!("group:{}:{}", gid, entry.perm.as_str()),
        AclTag::Mask => alloc::format!("mask::{}", entry.perm.as_str()),
        AclTag::Other => alloc::format!("other::{}", entry.perm.as_str()),
    }
}

/// Format a complete ACL in getfacl-style output.
pub fn format_acl(acl: &Acl, mask: Option<AclPerm>) -> Vec<String> {
    let mask_perm = mask.or_else(|| {
        acl.entries.iter()
            .find(|e| e.tag == AclTag::Mask)
            .map(|e| e.perm)
    });

    acl.entries.iter().map(|entry| {
        let base = format_entry(entry);
        // Show effective permissions when mask applies.
        match entry.tag {
            AclTag::User(_) | AclTag::GroupObj | AclTag::Group(_) => {
                if let Some(m) = mask_perm {
                    let effective = entry.perm.intersect(m);
                    if effective != entry.perm {
                        return alloc::format!("{}\t#effective:{}", base, effective.as_str());
                    }
                }
            }
            _ => {}
        }
        base
    }).collect()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the ACL subsystem.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[acl] Running self-test...");

    // --- Test 1: Minimal ACL from mode ---
    {
        let acl = from_mode(0o755);
        if acl.entries.len() != 3 {
            serial_println!("[acl]   ERROR: from_mode expected 3 entries, got {}", acl.entries.len());
            return Err(KernelError::InternalError);
        }

        // Owner should be rwx (7).
        let owner = acl.entries.iter().find(|e| e.tag == AclTag::UserObj).unwrap();
        if owner.perm.0 != 7 {
            serial_println!("[acl]   ERROR: owner perm {:o}, expected 7", owner.perm.0);
            return Err(KernelError::InternalError);
        }

        // Group should be r-x (5).
        let group = acl.entries.iter().find(|e| e.tag == AclTag::GroupObj).unwrap();
        if group.perm.0 != 5 {
            serial_println!("[acl]   ERROR: group perm {:o}, expected 5", group.perm.0);
            return Err(KernelError::InternalError);
        }

        // Other should be r-x (5).
        let other = acl.entries.iter().find(|e| e.tag == AclTag::Other).unwrap();
        if other.perm.0 != 5 {
            serial_println!("[acl]   ERROR: other perm {:o}, expected 5", other.perm.0);
            return Err(KernelError::InternalError);
        }

        serial_println!("[acl]   from_mode OK");
    }

    // --- Test 2: Set/get ACL ---
    {
        let test_path = "/tmp/_acl_test";
        let acl = from_mode(0o640);
        set_acl(test_path, acl.clone())?;

        let retrieved = get_acl(test_path);
        if retrieved.is_none() {
            serial_println!("[acl]   ERROR: get_acl returned None");
            return Err(KernelError::InternalError);
        }
        let retrieved = retrieved.unwrap();
        if retrieved.entries.len() != acl.entries.len() {
            serial_println!("[acl]   ERROR: entry count mismatch");
            return Err(KernelError::InternalError);
        }

        remove_acl(test_path);
        serial_println!("[acl]   set/get/remove OK");
    }

    // --- Test 3: Owner access check ---
    {
        let test_path = "/tmp/_acl_test_owner";
        let acl = from_mode(0o700); // Owner: rwx, group: ---, other: ---
        set_acl(test_path, acl)?;

        // Owner (uid 1000, gid 1000) accessing owned file → allowed.
        let result = check_access(test_path, 1000, 1000, 1000, 1000, AccessRequest::READ);
        if result.is_err() {
            serial_println!("[acl]   ERROR: owner read denied");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        // Non-owner (uid 2000) → denied (other perms are ---).
        let result = check_access(test_path, 2000, 2000, 1000, 1000, AccessRequest::READ);
        if result.is_ok() {
            serial_println!("[acl]   ERROR: non-owner read allowed");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        remove_acl(test_path);
        serial_println!("[acl]   owner check OK");
    }

    // --- Test 4: Named user ACL ---
    {
        let test_path = "/tmp/_acl_test_named";
        let acl = build_acl(
            AclPerm::ALL,     // owner: rwx
            AclPerm::READ,    // group: r--
            AclPerm::NONE,    // other: ---
            &[(2000, AclPerm(6))],  // user:2000 has rw-
            &[],
        );
        set_acl(test_path, acl)?;

        // Named user 2000 reading → allowed (rw- & mask includes r).
        let result = check_access(test_path, 2000, 9999, 1000, 1000, AccessRequest::READ);
        if result.is_err() {
            serial_println!("[acl]   ERROR: named user read denied");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        // Named user 2000 writing → allowed (rw- & mask includes w).
        let result = check_access(test_path, 2000, 9999, 1000, 1000, AccessRequest::WRITE);
        if result.is_err() {
            serial_println!("[acl]   ERROR: named user write denied");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        // Named user 2000 executing → denied (rw- doesn't include x).
        let result = check_access(test_path, 2000, 9999, 1000, 1000, AccessRequest::EXECUTE);
        if result.is_ok() {
            serial_println!("[acl]   ERROR: named user execute allowed");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        // Random user (uid 3000, not owner, not named) → denied (other is ---).
        let result = check_access(test_path, 3000, 9999, 1000, 1000, AccessRequest::READ);
        if result.is_ok() {
            serial_println!("[acl]   ERROR: other user read allowed");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        remove_acl(test_path);
        serial_println!("[acl]   named user ACL OK");
    }

    // --- Test 5: Mask enforcement ---
    {
        let test_path = "/tmp/_acl_test_mask";
        // User:2000 has rwx but mask is r--, so effective is r--.
        let mut acl = build_acl(
            AclPerm::ALL,
            AclPerm::NONE,
            AclPerm::NONE,
            &[(2000, AclPerm::ALL)],
            &[],
        );
        // Override mask to be restrictive.
        if let Some(mask_entry) = acl.entries.iter_mut().find(|e| e.tag == AclTag::Mask) {
            mask_entry.perm = AclPerm::READ; // mask: r--
        }
        set_acl(test_path, acl)?;

        // User 2000 reading → allowed (rwx & r-- = r--).
        let result = check_access(test_path, 2000, 9999, 1000, 1000, AccessRequest::READ);
        if result.is_err() {
            serial_println!("[acl]   ERROR: masked read denied");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        // User 2000 writing → denied (rwx & r-- = r--, no write).
        let result = check_access(test_path, 2000, 9999, 1000, 1000, AccessRequest::WRITE);
        if result.is_ok() {
            serial_println!("[acl]   ERROR: masked write allowed");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        remove_acl(test_path);
        serial_println!("[acl]   mask enforcement OK");
    }

    // --- Test 6: Named group ACL ---
    {
        let test_path = "/tmp/_acl_test_group";
        let acl = build_acl(
            AclPerm::ALL,
            AclPerm::NONE,
            AclPerm::NONE,
            &[],
            &[(500, AclPerm(4))],  // group:500 has r--
        );
        set_acl(test_path, acl)?;

        // User 3000 in group 500 reading → allowed.
        let result = check_access(test_path, 3000, 500, 1000, 1000, AccessRequest::READ);
        if result.is_err() {
            serial_println!("[acl]   ERROR: named group read denied");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        // User 3000 in group 500 writing → denied.
        let result = check_access(test_path, 3000, 500, 1000, 1000, AccessRequest::WRITE);
        if result.is_ok() {
            serial_println!("[acl]   ERROR: named group write allowed");
            remove_acl(test_path);
            return Err(KernelError::InternalError);
        }

        remove_acl(test_path);
        serial_println!("[acl]   named group ACL OK");
    }

    // --- Test 7: Validation ---
    {
        // Missing USER_OBJ → invalid.
        let bad_acl = Acl {
            entries: Vec::from([
                AclEntry { tag: AclTag::GroupObj, perm: AclPerm::ALL },
                AclEntry { tag: AclTag::Other, perm: AclPerm::NONE },
            ]),
        };
        match set_acl("/tmp/_acl_bad", bad_acl) {
            Err(KernelError::InvalidArgument) => {}
            _ => {
                serial_println!("[acl]   ERROR: invalid ACL accepted");
                return Err(KernelError::InternalError);
            }
        }

        // Named user without mask → invalid.
        let bad_acl2 = Acl {
            entries: Vec::from([
                AclEntry { tag: AclTag::UserObj, perm: AclPerm::ALL },
                AclEntry { tag: AclTag::User(100), perm: AclPerm::READ },
                AclEntry { tag: AclTag::GroupObj, perm: AclPerm::ALL },
                AclEntry { tag: AclTag::Other, perm: AclPerm::NONE },
            ]),
        };
        match set_acl("/tmp/_acl_bad2", bad_acl2) {
            Err(KernelError::InvalidArgument) => {}
            _ => {
                serial_println!("[acl]   ERROR: ACL without mask accepted");
                return Err(KernelError::InternalError);
            }
        }

        serial_println!("[acl]   validation OK");
    }

    // --- Test 8: Stats ---
    {
        let st = stats();
        // At least our check operations should be counted.
        if st.checks_performed == 0 {
            serial_println!("[acl]   ERROR: no checks counted");
            return Err(KernelError::InternalError);
        }
        if st.denials == 0 {
            serial_println!("[acl]   ERROR: no denials counted");
            return Err(KernelError::InternalError);
        }
        serial_println!("[acl]   stats OK (checks={}, denials={})", st.checks_performed, st.denials);
    }

    // --- Test 9: Format output ---
    {
        let acl = build_acl(
            AclPerm::ALL,
            AclPerm::READ,
            AclPerm::NONE,
            &[(1001, AclPerm(6))],
            &[],
        );
        let lines = format_acl(&acl, None);
        if lines.is_empty() {
            serial_println!("[acl]   ERROR: format_acl returned empty");
            return Err(KernelError::InternalError);
        }
        // Should contain "user::rwx" and "user:1001:rw-".
        let has_owner = lines.iter().any(|l| l.contains("user::rwx"));
        let has_named = lines.iter().any(|l| l.contains("user:1001:rw-"));
        if !has_owner || !has_named {
            serial_println!("[acl]   ERROR: format_acl missing expected entries");
            return Err(KernelError::InternalError);
        }
        serial_println!("[acl]   format OK");
    }

    // --- Test 10: No ACL → allow all ---
    {
        let result = check_access("/nonexistent/no/acl", 9999, 9999, 0, 0, AccessRequest::WRITE);
        if result.is_err() {
            serial_println!("[acl]   ERROR: no-ACL path should allow");
            return Err(KernelError::InternalError);
        }
        serial_println!("[acl]   no-ACL passthrough OK");
    }

    serial_println!("[acl] Self-test passed (10 tests).");
    Ok(())
}
