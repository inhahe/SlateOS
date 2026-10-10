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
//! **In memory only.** ACLs live in this module's table and nowhere else: they
//! do not survive a reboot. A program reaches them through the
//! `system.posix_acl_access` extended attribute, as on Linux -- `setfacl` and
//! `getfacl` -- which the VFS answers from this table and writes into it (the
//! ACL door, `vfs::acl_door_*`, design-decisions §1527), but no filesystem
//! stores that attribute. (This section once said they were stored there, with
//! the table as a cache; no code ever did that.) The filesystem's xattr is
//! where they belong -- it would make an ACL persist, and end with its inode --
//! and `known-issues.md` `A-PER-FILE-STATE-OUTLIVED-ITS-FILE` records that as
//! the long-term home.
//!
//! The table is keyed on the file's identity, so it has to be told when a
//! file goes: an ext4 inode number is reused by the next file created, and an
//! ACL left behind would govern that stranger. The VFS does tell it -- see
//! [`super::perfile`] and [`PER_FILE_STATE`].
//!
//! Directories' **default ACLs** (`system.posix_acl_default`, `setfacl -d`)
//! are a second table beside it, [`DEFAULTS`], kept and forgotten the same way
//! ([`DEFAULT_PER_FILE_STATE`]). The permission check never reads one: a
//! default ACL is what a file made in the directory begins with -- its access
//! ACL and its mode, by [`inherit`], with no umask (design-decisions §1531).
//! The VFS applies it when it makes the file.
//!
//! ## Reference
//!
//! POSIX 1003.1e draft 17 (ACL specification)
//! Linux: `man acl`, `man getfacl`, `man setfacl`

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::path::{Path, PathBuf};
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
    ///
    /// Keyed by `PathBuf`, not `String`, per design-decisions.md §261: our
    /// filesystems admit every byte except `/` and NUL in a name, so a
    /// `String` key is narrower than the thing it keys.  Two names that
    /// differ only in a byte with no UTF-8 spelling would fold onto one
    /// entry — and this table decides whether an access is *refused*, so
    /// the fold is doubly wrong: `check_access` fails open when it finds
    /// no entry, so the file whose ACL was displaced becomes unprotected,
    /// while the file that displaced it inherits restrictions nobody asked
    /// for.
    /// Keyed by [`AclKey`], valued by the path as DATA plus the ACL.
    ///
    /// The path is kept so `list_paths` can still report names without the
    /// key type reaching its signature.
    acls: BTreeMap<AclKey, (PathBuf, Acl)>,
    /// Statistics counters.
    checks_performed: u64,
    denials: u64,
}

/// The key an ACL is stored under: the file's identity when it has one,
/// otherwise its path.
///
/// POSIX keeps an ACL in the inode's extended attributes, which is why a
/// hard link shares its file's ACL there. Keying on the path string made
/// this table disagree with that: a second name for a protected file found
/// no entry, and `check_access` treats no entry as *allow* ("defer to
/// traditional permissions"), so the protection simply did not apply under
/// the other name.
///
/// That was not exploitable when it was found -- there was no ACL syscall, so
/// only a `kshell` command can put an entry in this table at all -- and the
/// reason to fix it anyway is that it is pre-positioned: adding
/// `SYS_ACL_SET` later would arm it, and the new syscall would look correct
/// in isolation. See `known-issues.md` 2026-09-21.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum AclKey {
    Id(crate::fs::vfs::FileId),
    Path(PathBuf),
}

/// Derive the key for a path. Every key is built by [`key_from`], which this
/// and the file-lifecycle hooks share, so a key can never be derived one way
/// at `insert` and another at `remove`.
///
/// MUST be called before taking the `ACLS` lock: `file_identity` calls into
/// the VFS, and a module global held across that call inverts
/// filesystem-lock -> module-state. Nine such sites were introduced and
/// caught by `scripts/check-vfs-under-lock.py` on 2026-09-21; this ordering
/// is what that gate enforces.
///
/// The path fallback is exact rather than approximate: every filesystem here
/// that can give one file two names assigns nonzero inodes (ext4 and memfs
/// leave `ino: 0` at 0 of 15 and 0 of 16 construction sites), so a file that
/// resolves to no identity is one that cannot be hard-linked anyway.
fn acl_key(path: &Path) -> AclKey {
    key_from(crate::fs::Vfs::file_identity(path).unwrap_or(None), path)
}

/// The key for a file whose identity is already known -- the one place an
/// `AclKey` is built, which [`acl_key`] and the file-lifecycle hooks share.
///
/// The hooks need it because they run after the name is gone: the VFS reads
/// the identity while the name still resolves and hands it over, and looking
/// the name up again would find nothing (or, for a replaced name, the wrong
/// file).
fn key_from(id: Option<crate::fs::vfs::FileId>, path: &Path) -> AclKey {
    match id {
        Some(id) => AclKey::Id(id),
        None => AclKey::Path(path.to_path_buf()),
    }
}

// ---------------------------------------------------------------------------
// File lifecycle (see `super::perfile`)
// ---------------------------------------------------------------------------

/// This table's part in the file lifecycle: an ACL ends with its file, and
/// moves with it when it is renamed. See [`super::perfile`] for why a
/// `FileId`-keyed entry that outlived its file would be a stranger's ACL.
pub(crate) const PER_FILE_STATE: super::perfile::Table = super::perfile::Table {
    name: "acl",
    forget: forget_file,
    rename: rename_names,
    unmounted: forget_filesystem,
    plant: plant_for_test,
    finds: finds_for_test,
    reports: reports_for_test,
};

/// The file is gone: drop its ACL.
fn forget_file(id: Option<crate::fs::vfs::FileId>, path: &Path) {
    // Every removal on the system passes through here, and almost no file has
    // an ACL; the count is the same filter `check_access`'s callers use.
    if ACL_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let key = key_from(id, path);
    let mut inner = ACLS.lock();
    if inner.acls.remove(&key).is_some() {
        ACL_COUNT.store(inner.acls.len(), Ordering::Relaxed);
    }
}

/// Names moved: rewrite every stored name `rename` maps to a new one.
///
/// An identity key stays as it is -- the file is the same file -- and only the
/// name kept to report it changes. A path key IS the name (the two are equal
/// by construction in [`set_acl`]), so it moves with it.
fn rename_names(rename: &super::perfile::NameMap<'_>) {
    if ACL_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let mut inner = ACLS.lock();
    // Collected first: a map key cannot be changed in place.
    let moved: Vec<(AclKey, PathBuf)> = inner
        .acls
        .iter()
        .filter_map(|(key, (name, _))| rename(name).map(|new| (key.clone(), new)))
        .collect();
    for (key, new_name) in moved {
        let Some((_, acl)) = inner.acls.remove(&key) else {
            continue;
        };
        let new_key = match key {
            AclKey::Id(id) => AclKey::Id(id),
            AclKey::Path(_) => AclKey::Path(new_name.clone()),
        };
        inner.acls.insert(new_key, (new_name, acl));
    }
    ACL_COUNT.store(inner.acls.len(), Ordering::Relaxed);
}

/// Self-test support: an ACL granting read and write to everyone -- the
/// lifecycle rungs need no more, and it refuses nothing they do.
fn plant_for_test(path: &Path) -> KernelResult<()> {
    set_acl(path, from_mode(0o666))
}

/// Self-test support: whether a lookup through `path` finds an ACL.
fn finds_for_test(path: &Path) -> bool {
    get_acl(path).is_some()
}

/// Self-test support: whether an ACL is reported under `name`.
fn reports_for_test(name: &Path) -> bool {
    ACLS.lock().acls.values().any(|(p, _)| p.as_path() == name)
}

/// A filesystem was unmounted: its mount id is never reused, so no identity
/// on it can match again, and its entries are only garbage.
fn forget_filesystem(fs_id: u64) {
    if ACL_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let mut inner = ACLS.lock();
    inner
        .acls
        .retain(|key, _| !matches!(key, AclKey::Id(id) if id.fs_id == fs_id));
    ACL_COUNT.store(inner.acls.len(), Ordering::Relaxed);
}
static ACLS: Mutex<AclInner> = Mutex::new(AclInner {
    acls: BTreeMap::new(),
    checks_performed: 0,
    denials: 0,
});

/// `ACLS.acls.len()`, readable without taking the lock.
///
/// The VFS consults ACLs on every path operation, and path lookup is on the
/// hot path of every open (target: a few hundred nanoseconds). Answering "are
/// there any ACLs at all?" with a mutex acquisition plus a `BTreeMap` walk
/// would put a lock on that path for the overwhelmingly common case of a
/// system with no ACLs set. A relaxed load costs nothing.
///
/// Kept in step with the map by every function that changes its size:
/// [`set_acl`], [`remove_acl`], [`clear`], and the file-lifecycle hooks
/// (`forget_file`, `rename_names`, `forget_filesystem`). Relaxed is sufficient: a
/// reader that misses a just-inserted ACL by a few cycles behaves exactly
/// like a reader that ran a moment earlier, and `check_access` re-reads the
/// map under the lock anyway, so the count is a filter and never the answer.
static ACL_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Number of paths that currently carry an ACL, without taking the lock.
///
/// Callers use this to skip ACL evaluation entirely; see [`ACL_COUNT`].
#[inline]
#[must_use]
pub fn count() -> usize {
    ACL_COUNT.load(Ordering::Relaxed)
}

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
pub fn set_acl(path: impl AsRef<Path>, acl: Acl) -> KernelResult<()> {
    let path = path.as_ref();
    // Validate: must have USER_OBJ, GROUP_OBJ, OTHER.
    let has_user_obj = acl.entries.iter().any(|e| e.tag == AclTag::UserObj);
    let has_group_obj = acl.entries.iter().any(|e| e.tag == AclTag::GroupObj);
    let has_other = acl.entries.iter().any(|e| e.tag == AclTag::Other);

    if !has_user_obj || !has_group_obj || !has_other {
        return Err(KernelError::InvalidArgument);
    }

    // If named entries exist, mask is required.
    let has_named = acl
        .entries
        .iter()
        .any(|e| matches!(e.tag, AclTag::User(_) | AclTag::Group(_)));
    let has_mask = acl.entries.iter().any(|e| e.tag == AclTag::Mask);
    if has_named && !has_mask {
        return Err(KernelError::InvalidArgument);
    }

    // Above the lock. See `acl_key`.
    let key = acl_key(path);
    let mut inner = ACLS.lock();
    inner.acls.insert(key, (path.to_path_buf(), acl));
    // Published under the lock so a concurrent `remove_acl` cannot interleave
    // its decrement between this insert and this store.
    ACL_COUNT.store(inner.acls.len(), Ordering::Relaxed);
    Ok(())
}

/// Get the ACL for a file path.
///
/// Returns None if no ACL is set (file uses only traditional permissions).
pub fn get_acl(path: impl AsRef<Path>) -> Option<Acl> {
    let key = acl_key(path.as_ref());
    ACLS.lock().acls.get(&key).map(|(_, acl)| acl.clone())
}

/// Remove the ACL from a file path.
///
/// After removal, the file uses only traditional permissions.
pub fn remove_acl(path: impl AsRef<Path>) -> bool {
    let key = acl_key(path.as_ref());
    let mut inner = ACLS.lock();
    let removed = inner.acls.remove(&key).is_some();
    ACL_COUNT.store(inner.acls.len(), Ordering::Relaxed);
    removed
}

/// Which file an ACL is asked of ([`check_access`]).
#[derive(Debug, Clone, Copy)]
pub enum AclFile<'a> {
    /// The file a path names now.
    Path(&'a Path),
    /// A file held open, by its identity, whatever its names now
    /// (`fs::vfs::FileObject`): its ACL, if it has one, is keyed by it.
    Held(crate::fs::vfs::FileId),
}

impl<'a, S: AsRef<[u8]> + ?Sized> From<&'a S> for AclFile<'a> {
    fn from(path: &'a S) -> Self {
        Self::Path(Path::new(path))
    }
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
pub fn check_access<'a>(
    file: impl Into<AclFile<'a>>,
    requester_uid: u32,
    requester_gid: u32,
    file_uid: u32,
    file_gid: u32,
    request: AccessRequest,
) -> KernelResult<()> {
    // Above the lock: this is the site the lock-order gate exists for.
    let key = match file.into() {
        AclFile::Path(path) => acl_key(path),
        AclFile::Held(id) => AclKey::Id(id),
    };
    let mut inner = ACLS.lock();
    inner.checks_performed = inner.checks_performed.saturating_add(1);

    let acl = match inner.acls.get(&key) {
        Some((_, acl)) => acl,
        None => return Ok(()), // No ACL, defer to traditional permissions.
    };

    // Find the mask entry (if any).
    let mask = acl
        .entries
        .iter()
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
    if let Some(entry) = acl
        .entries
        .iter()
        .find(|e| e.tag == AclTag::User(requester_uid))
    {
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
pub fn list_paths() -> Vec<PathBuf> {
    ACLS.lock().acls.values().map(|(p, _)| p.clone()).collect()
}

/// Get statistics about the ACL subsystem.
pub fn stats() -> AclStats {
    let inner = ACLS.lock();
    let total_entries: usize = inner.acls.values().map(|(_, a)| a.entries.len()).sum();
    AclStats {
        files_with_acls: inner.acls.len(),
        total_entries,
        checks_performed: inner.checks_performed,
        denials: inner.denials,
    }
}

/// Clear all ACLs, default ACLs included (for testing).
#[allow(dead_code)]
pub fn clear() {
    let mut inner = ACLS.lock();
    inner.acls.clear();
    inner.checks_performed = 0;
    inner.denials = 0;
    ACL_COUNT.store(0, Ordering::Relaxed);
    drop(inner);
    DEFAULTS.lock().clear();
    DEFAULT_COUNT.store(0, Ordering::Relaxed);
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
            AclEntry {
                tag: AclTag::UserObj,
                perm: owner,
            },
            AclEntry {
                tag: AclTag::GroupObj,
                perm: group,
            },
            AclEntry {
                tag: AclTag::Other,
                perm: other,
            },
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

    entries.push(AclEntry {
        tag: AclTag::UserObj,
        perm: owner_perm,
    });

    for &(uid, perm) in named_users {
        entries.push(AclEntry {
            tag: AclTag::User(uid),
            perm,
        });
    }

    entries.push(AclEntry {
        tag: AclTag::GroupObj,
        perm: group_perm,
    });

    for &(gid, perm) in named_groups {
        entries.push(AclEntry {
            tag: AclTag::Group(gid),
            perm,
        });
    }

    // Compute mask: union of GROUP_OBJ + all named entries.
    let mut mask_bits = group_perm.0;
    for &(_, perm) in named_users {
        mask_bits |= perm.0;
    }
    for &(_, perm) in named_groups {
        mask_bits |= perm.0;
    }
    entries.push(AclEntry {
        tag: AclTag::Mask,
        perm: AclPerm(mask_bits),
    });

    entries.push(AclEntry {
        tag: AclTag::Other,
        perm: other_perm,
    });

    Acl { entries }
}

/// Format an ACL entry as a human-readable string.
pub fn format_entry(entry: &AclEntry) -> String {
    match entry.tag {
        AclTag::UserObj => alloc::format!("user::{}", entry.perm.as_str()),
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
        acl.entries
            .iter()
            .find(|e| e.tag == AclTag::Mask)
            .map(|e| e.perm)
    });

    acl.entries
        .iter()
        .map(|entry| {
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
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The Linux door: `system.posix_acl_access`
// ---------------------------------------------------------------------------
//
// Linux keeps a file's ACL in the extended attribute `system.posix_acl_access`,
// and `getfacl`/`setfacl` reach it there. The VFS (`Vfs::xattr_*`) answers that
// name from this table rather than from the filesystem, through the functions
// below: the codec, and get/set/remove by an identity the VFS already has, so
// they can run under the filesystem's lock without calling back into the VFS.

/// The xattr's name.
pub const XATTR_ACCESS: &[u8] = b"system.posix_acl_access";
/// The default ACL's xattr name: a directory's, which its new files and
/// directories inherit ([`inherit`]), kept in [`DEFAULTS`].
pub const XATTR_DEFAULT: &[u8] = b"system.posix_acl_default";

/// `POSIX_ACL_XATTR_VERSION`.
const XATTR_VERSION: u32 = 2;
/// `ACL_UNDEFINED_ID`: the id of an entry that names no one.
const UNDEFINED_ID: u32 = u32::MAX;
// Linux's `e_tag` values.
const TAG_USER_OBJ: u16 = 0x01;
const TAG_USER: u16 = 0x02;
const TAG_GROUP_OBJ: u16 = 0x04;
const TAG_GROUP: u16 = 0x08;
const TAG_MASK: u16 = 0x10;
const TAG_OTHER: u16 = 0x20;

/// An ACL as the xattr's bytes: Linux's `posix_acl_xattr_header` (version 2,
/// little-endian) and one 8-byte `posix_acl_xattr_entry` per entry, in the
/// ACL's order.
#[must_use]
pub fn encode_xattr(acl: &Acl) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&XATTR_VERSION.to_le_bytes());
    for e in &acl.entries {
        let (tag, id) = match e.tag {
            AclTag::UserObj => (TAG_USER_OBJ, UNDEFINED_ID),
            AclTag::User(uid) => (TAG_USER, uid),
            AclTag::GroupObj => (TAG_GROUP_OBJ, UNDEFINED_ID),
            AclTag::Group(gid) => (TAG_GROUP, gid),
            AclTag::Mask => (TAG_MASK, UNDEFINED_ID),
            AclTag::Other => (TAG_OTHER, UNDEFINED_ID),
        };
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&u16::from(e.perm.0 & 7).to_le_bytes());
        out.extend_from_slice(&id.to_le_bytes());
    }
    out
}

/// The xattr's bytes as an ACL, refused as Linux's `posix_acl_from_xattr`
/// and `posix_acl_valid` refuse it.
///
/// # Errors
///
/// `InvalidArgument` (`EINVAL`) for: a length that is not a header and whole
/// entries; a version other than 2; an unknown tag; a permission outside
/// `rwx`; entries out of Linux's order (owner, named users, owning group,
/// named groups, mask, other) or missing one of the three every ACL has; a
/// named entry without a mask. (A mask with no named entry is allowed, as
/// Linux allows it.)
pub fn decode_xattr(bytes: &[u8]) -> KernelResult<Acl> {
    let body = bytes.get(4..).ok_or(KernelError::InvalidArgument)?;
    let version = bytes
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_le_bytes)
        .ok_or(KernelError::InvalidArgument)?;
    let entries_raw = body.chunks_exact(8);
    if version != XATTR_VERSION || !entries_raw.remainder().is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut entries = Vec::new();
    for raw in entries_raw {
        let half = |b: Option<&[u8]>| b.and_then(|b| <[u8; 2]>::try_from(b).ok());
        let (Some(tag), Some(perm), Some(id)) = (
            half(raw.get(0..2)).map(u16::from_le_bytes),
            half(raw.get(2..4)).map(u16::from_le_bytes),
            raw.get(4..8)
                .and_then(|b| <[u8; 4]>::try_from(b).ok())
                .map(u32::from_le_bytes),
        ) else {
            return Err(KernelError::InvalidArgument);
        };
        if perm & !7 != 0 {
            return Err(KernelError::InvalidArgument);
        }
        let tag = match tag {
            TAG_USER_OBJ => AclTag::UserObj,
            TAG_USER => AclTag::User(id),
            TAG_GROUP_OBJ => AclTag::GroupObj,
            TAG_GROUP => AclTag::Group(id),
            TAG_MASK => AclTag::Mask,
            TAG_OTHER => AclTag::Other,
            _ => return Err(KernelError::InvalidArgument),
        };
        entries.push(AclEntry {
            tag,
            perm: AclPerm(u8::try_from(perm).map_err(|_| KernelError::InvalidArgument)?),
        });
    }
    let acl = Acl { entries };
    validate_order(&acl)?;
    Ok(acl)
}

/// Linux's `posix_acl_valid`: the entries in its order -- the owner, named
/// users, the owning group, named groups, the mask, the others -- each of the
/// three base entries once, and a mask wherever there is a named entry.
fn validate_order(acl: &Acl) -> KernelResult<()> {
    #[derive(PartialEq)]
    enum State {
        UserObj,
        User,
        Group,
        Other,
        Done,
    }
    let mut state = State::UserObj;
    let mut needs_mask = false;
    for e in &acl.entries {
        state = match (e.tag, &state) {
            (AclTag::UserObj, State::UserObj) => State::User,
            (AclTag::User(_), State::User) => {
                needs_mask = true;
                State::User
            }
            (AclTag::GroupObj, State::User) => State::Group,
            (AclTag::Group(_), State::Group) => {
                needs_mask = true;
                State::Group
            }
            (AclTag::Mask, State::Group) => State::Other,
            (AclTag::Other, State::Other) => State::Done,
            (AclTag::Other, State::Group) if !needs_mask => State::Done,
            _ => return Err(KernelError::InvalidArgument),
        };
    }
    if state == State::Done {
        Ok(())
    } else {
        Err(KernelError::InvalidArgument)
    }
}

/// The mode bits an ACL means: the owner's from its owner entry, the group's
/// from its mask when it has one (the owning group's otherwise), the others'
/// from its other entry -- Linux's `posix_acl_equiv_mode` and
/// `posix_acl_update_mode`. The bits above `0o777` are `mode`'s, kept.
#[must_use]
pub fn mode_of(acl: &Acl, mode: u16) -> u16 {
    let perm = |tag: AclTag| {
        acl.entries
            .iter()
            .find(|e| e.tag == tag)
            .map_or(0, |e| u16::from(e.perm.0 & 7))
    };
    let group = if acl.entries.iter().any(|e| e.tag == AclTag::Mask) {
        perm(AclTag::Mask)
    } else {
        perm(AclTag::GroupObj)
    };
    (mode & !0o777)
        | perm(AclTag::UserObj).wrapping_shl(6)
        | group.wrapping_shl(3)
        | perm(AclTag::Other)
}

/// Whether the ACL says nothing the mode cannot: the three base entries and
/// no more. Linux stores no ACL then, only the mode it means
/// (`posix_acl_equiv_mode` answering 0).
#[must_use]
pub fn is_minimal(acl: &Acl) -> bool {
    acl.entries
        .iter()
        .all(|e| matches!(e.tag, AclTag::UserObj | AclTag::GroupObj | AclTag::Other))
}

/// An ACL with the mode `mode` written into it, as Linux's `posix_acl_chmod`
/// writes a chmod into a file's ACL: the owner entry, the mask (or, without
/// one, the owning group's entry) and the other entry take the mode's three
/// sets of bits; named entries keep theirs, limited by the new mask.
#[must_use]
pub fn with_mode(acl: &Acl, mode: u16) -> Acl {
    let has_mask = acl.entries.iter().any(|e| e.tag == AclTag::Mask);
    let bits = |shift: u32| AclPerm(u8::try_from(mode.wrapping_shr(shift) & 7).unwrap_or(0));
    let entries = acl
        .entries
        .iter()
        .map(|e| {
            let perm = match e.tag {
                AclTag::UserObj => bits(6),
                AclTag::Mask => bits(3),
                AclTag::GroupObj if !has_mask => bits(3),
                AclTag::Other => bits(0),
                _ => e.perm,
            };
            AclEntry { tag: e.tag, perm }
        })
        .collect();
    Acl { entries }
}

/// The ACL of the file with identity `id` (or, where its filesystem gives
/// none, named `path`): [`get_acl`] for a caller that has the identity
/// already -- the VFS, under the filesystem's lock, which must not call back
/// into the VFS as `get_acl` does.
#[must_use]
pub fn get_acl_for(id: Option<crate::fs::vfs::FileId>, path: &Path) -> Option<Acl> {
    if count() == 0 {
        return None;
    }
    let key = key_from(id, path);
    ACLS.lock().acls.get(&key).map(|(_, acl)| acl.clone())
}

/// Store `acl` for the file with identity `id` (or named `path`), as
/// [`set_acl`] does; validated as the xattr door validates it.
///
/// # Errors
///
/// `InvalidArgument` for an ACL out of Linux's order or missing an entry.
pub fn set_acl_for(id: Option<crate::fs::vfs::FileId>, path: &Path, acl: Acl) -> KernelResult<()> {
    validate_order(&acl)?;
    let key = key_from(id, path);
    let mut inner = ACLS.lock();
    inner.acls.insert(key, (path.to_path_buf(), acl));
    ACL_COUNT.store(inner.acls.len(), Ordering::Relaxed);
    Ok(())
}

/// Drop the ACL of the file with identity `id` (or named `path`); whether
/// there was one.
pub fn remove_acl_for(id: Option<crate::fs::vfs::FileId>, path: &Path) -> bool {
    if count() == 0 {
        return false;
    }
    let key = key_from(id, path);
    let mut inner = ACLS.lock();
    let removed = inner.acls.remove(&key).is_some();
    ACL_COUNT.store(inner.acls.len(), Ordering::Relaxed);
    removed
}

// ---------------------------------------------------------------------------
// Default ACLs: `system.posix_acl_default`
// ---------------------------------------------------------------------------
//
// A directory's default ACL is what a file made in it begins with
// (`setfacl -d`): the new file's access ACL is the default narrowed by the
// mode its creator asked for, and a new directory takes the default as its
// own default too -- Linux's `posix_acl_create`. A directory with one gives
// its new files no umask: the default ACL says what they get instead.
//
// Kept beside the access ACLs, in their own table: the permission check never
// reads a default ACL, so the gate's "no ACLs at all" fast path (`count`) must
// not stop being taken because a directory has one.

/// Directories' default ACLs, keyed as the access ACLs are ([`AclKey`]).
static DEFAULTS: Mutex<BTreeMap<AclKey, (PathBuf, Acl)>> = Mutex::new(BTreeMap::new());

/// `DEFAULTS`' length, readable without its lock: every creation asks
/// whether any directory has a default ACL before it looks for its own's.
static DEFAULT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// How many directories have a default ACL, without taking the lock.
#[inline]
#[must_use]
pub fn default_count() -> usize {
    DEFAULT_COUNT.load(Ordering::Relaxed)
}

/// The default ACL of the directory with identity `id` (or, where its
/// filesystem gives none, named `path`), for a caller with the identity in
/// hand -- the VFS, under the filesystem's lock.
#[must_use]
pub fn get_default_for(id: Option<crate::fs::vfs::FileId>, path: &Path) -> Option<Acl> {
    if default_count() == 0 {
        return None;
    }
    let key = key_from(id, path);
    DEFAULTS.lock().get(&key).map(|(_, acl)| acl.clone())
}

/// Store `acl` as the default ACL of the directory with identity `id` (or
/// named `path`). Validated as an access ACL is; one the mode bits could say
/// is stored all the same, as Linux stores it -- a default ACL is not a mode.
///
/// # Errors
///
/// `InvalidArgument` for an ACL out of Linux's order or missing an entry.
pub fn set_default_for(
    id: Option<crate::fs::vfs::FileId>,
    path: &Path,
    acl: Acl,
) -> KernelResult<()> {
    validate_order(&acl)?;
    let key = key_from(id, path);
    let mut defaults = DEFAULTS.lock();
    defaults.insert(key, (path.to_path_buf(), acl));
    DEFAULT_COUNT.store(defaults.len(), Ordering::Relaxed);
    Ok(())
}

/// Drop the default ACL of the directory with identity `id` (or named
/// `path`); whether there was one.
pub fn remove_default_for(id: Option<crate::fs::vfs::FileId>, path: &Path) -> bool {
    if default_count() == 0 {
        return false;
    }
    let key = key_from(id, path);
    let mut defaults = DEFAULTS.lock();
    let removed = defaults.remove(&key).is_some();
    DEFAULT_COUNT.store(defaults.len(), Ordering::Relaxed);
    removed
}

/// What a file made in a directory whose default ACL is `default`, asked for
/// with the mode `mode`, begins with -- Linux's `posix_acl_create_masq`: its
/// mode and its access ACL. Each of the owner entry, the mask (the owning
/// group's entry where there is none) and the other entry is narrowed to the
/// matching bits of `mode`, and the mode to what those entries then grant;
/// named entries are kept, limited by the narrowed mask. The bits above
/// `0o777` are `mode`'s.
///
/// The access ACL is `None` when it says nothing the mode cannot -- Linux
/// keeps none then.
#[must_use]
pub fn inherit(default: &Acl, mode: u16) -> (u16, Option<Acl>) {
    let bits = |shift: u32| u8::try_from(mode.wrapping_shr(shift) & 7).unwrap_or(0);
    let has_mask = default.entries.iter().any(|e| e.tag == AclTag::Mask);
    let entries: Vec<AclEntry> = default
        .entries
        .iter()
        .map(|e| {
            let perm = match e.tag {
                AclTag::UserObj => e.perm.0 & bits(6),
                AclTag::Mask => e.perm.0 & bits(3),
                AclTag::GroupObj if !has_mask => e.perm.0 & bits(3),
                AclTag::Other => e.perm.0 & bits(0),
                AclTag::User(_) | AclTag::Group(_) | AclTag::GroupObj => e.perm.0,
            };
            AclEntry {
                tag: e.tag,
                perm: AclPerm(perm),
            }
        })
        .collect();
    let acl = Acl { entries };
    let mode = mode_of(&acl, mode);
    if is_minimal(&acl) {
        (mode, None)
    } else {
        (mode, Some(acl))
    }
}

/// Self-test support: a default ACL granting everyone everything -- changes
/// nothing the lifecycle rungs do.
fn plant_default_for_test(path: &Path) -> KernelResult<()> {
    let id = crate::fs::Vfs::file_identity(path).unwrap_or(None);
    set_default_for(id, path, from_mode(0o777))
}

/// Self-test support: whether a lookup through `path` finds a default ACL.
fn finds_default_for_test(path: &Path) -> bool {
    let id = crate::fs::Vfs::file_identity(path).unwrap_or(None);
    get_default_for(id, path).is_some()
}

/// Self-test support: whether a default ACL is reported under `name`.
fn reports_default_for_test(name: &Path) -> bool {
    DEFAULTS.lock().values().any(|(p, _)| p.as_path() == name)
}

/// The directory is gone: drop its default ACL.
fn forget_default(id: Option<crate::fs::vfs::FileId>, path: &Path) {
    if default_count() == 0 {
        return;
    }
    let key = key_from(id, path);
    let mut defaults = DEFAULTS.lock();
    if defaults.remove(&key).is_some() {
        DEFAULT_COUNT.store(defaults.len(), Ordering::Relaxed);
    }
}

/// Names moved: as [`rename_names`] does for the access ACLs.
fn rename_defaults(rename: &super::perfile::NameMap<'_>) {
    if default_count() == 0 {
        return;
    }
    let mut defaults = DEFAULTS.lock();
    let moved: Vec<(AclKey, PathBuf)> = defaults
        .iter()
        .filter_map(|(key, (name, _))| rename(name).map(|new| (key.clone(), new)))
        .collect();
    for (key, new_name) in moved {
        let Some((_, acl)) = defaults.remove(&key) else {
            continue;
        };
        let new_key = match key {
            AclKey::Id(id) => AclKey::Id(id),
            AclKey::Path(_) => AclKey::Path(new_name.clone()),
        };
        defaults.insert(new_key, (new_name, acl));
    }
    DEFAULT_COUNT.store(defaults.len(), Ordering::Relaxed);
}

/// A filesystem was unmounted: its default ACLs are garbage, as its access
/// ACLs are.
fn forget_defaults_on(fs_id: u64) {
    if default_count() == 0 {
        return;
    }
    let mut defaults = DEFAULTS.lock();
    defaults.retain(|key, _| !matches!(key, AclKey::Id(id) if id.fs_id == fs_id));
    DEFAULT_COUNT.store(defaults.len(), Ordering::Relaxed);
}

/// The default-ACL table's part in the file lifecycle ([`super::perfile`]),
/// as [`PER_FILE_STATE`] is the access ACLs'.
pub(crate) const DEFAULT_PER_FILE_STATE: super::perfile::Table = super::perfile::Table {
    name: "acl-default",
    forget: forget_default,
    rename: rename_defaults,
    unmounted: forget_defaults_on,
    plant: plant_default_for_test,
    finds: finds_default_for_test,
    reports: reports_default_for_test,
};

/// The door's codec and ACL-and-mode arithmetic, alone: what
/// `vfs::self_test_acl_door` stands on.
///
/// # Errors
///
/// `InternalError` naming the first case that answered wrongly.
pub fn self_test_xattr() -> KernelResult<()> {
    let named = build_acl(
        AclPerm::ALL,
        AclPerm(5),
        AclPerm(4),
        &[(1000, AclPerm(6))],
        &[(50, AclPerm(4))],
    );
    // Round trip, and the bytes Linux would write for it.
    let bytes = encode_xattr(&named);
    let back = decode_xattr(&bytes)?;
    let expected_len = named
        .entries
        .len()
        .checked_mul(8)
        .and_then(|n| n.checked_add(4));
    if back.entries != named.entries || Some(bytes.len()) != expected_len {
        serial_println!("[acl]   FAIL: the xattr round trip changed the ACL");
        return Err(KernelError::InternalError);
    }
    if bytes.get(..4) != Some(&2u32.to_le_bytes()[..])
        || bytes.get(12..14) != Some(&TAG_USER.to_le_bytes()[..])
        || bytes.get(16..20) != Some(&1000u32.to_le_bytes()[..])
    {
        serial_println!("[acl]   FAIL: the xattr is not Linux's layout: {:?}", bytes);
        return Err(KernelError::InternalError);
    }
    // Refusals: each a way setfacl's input could be malformed.
    let mut wrong_version = bytes.clone();
    if let Some(v) = wrong_version.get_mut(0) {
        *v = 1;
    }
    let minimal = encode_xattr(&from_mode(0o640));
    let mut no_mask = encode_xattr(&build_acl(
        AclPerm::ALL,
        AclPerm(5),
        AclPerm(4),
        &[(1000, AclPerm(6))],
        &[],
    ));
    // Cut the mask entry (the one before the last) out.
    let len = no_mask.len();
    no_mask.drain(len.saturating_sub(16)..len.saturating_sub(8));
    let mut out_of_order = minimal.clone();
    // Swap the owner and other entries: bytes 4..12 and 20..28.
    for k in 4..12_usize {
        out_of_order.swap(k, k.saturating_add(16));
    }
    let mut bad_perm = minimal.clone();
    if let Some(p) = bad_perm.get_mut(6) {
        *p = 8;
    }
    let refusals: [(&str, &[u8]); 6] = [
        ("no header", &[]),
        (
            "a ragged entry",
            bytes.get(..bytes.len().saturating_sub(1)).unwrap_or(&[]),
        ),
        ("version 1", &wrong_version),
        ("a named entry and no mask", &no_mask),
        ("other before owner", &out_of_order),
        ("a permission outside rwx", &bad_perm),
    ];
    for (what, input) in refusals {
        if decode_xattr(input).map(|_| ()) != Err(KernelError::InvalidArgument) {
            serial_println!("[acl]   FAIL: {} was accepted", what);
            return Err(KernelError::InternalError);
        }
    }
    // The mode an ACL means, and a chmod written back into one.
    let checks = [
        (
            "mode of a minimal ACL",
            mode_of(&from_mode(0o640), 0o100_000),
            0o100_640,
        ),
        // The mask is the union of the group's and the named entries': rwx.
        ("mode of a masked ACL", mode_of(&named, 0o644), 0o774),
        (
            "chmod into a masked ACL",
            mode_of(&with_mode(&named, 0o710), 0),
            0o710,
        ),
    ];
    for (what, got, want) in checks {
        if got != want {
            serial_println!("[acl]   FAIL: {}: {:o}, want {:o}", what, got, want);
            return Err(KernelError::InternalError);
        }
    }
    let chmodded = with_mode(&named, 0o710);
    let kept = chmodded
        .entries
        .iter()
        .find(|e| e.tag == AclTag::User(1000))
        .map(|e| e.perm);
    if kept != Some(AclPerm(6)) || !is_minimal(&from_mode(0o600)) || is_minimal(&named) {
        serial_println!("[acl]   FAIL: a chmod moved a named entry, or minimality is wrong");
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[acl]   xattr door: Linux's layout both ways, six malformed inputs refused, the mode an ACL means and a chmod written into one: OK"
    );
    Ok(())
}

/// What a new file takes from a default ACL ([`inherit`], Linux's
/// `posix_acl_create_masq`): the owner, mask and other entries narrowed to
/// the mode asked for, named entries kept, the mode what the narrowed entries
/// grant with the special bits asked for, and no access ACL when the result
/// says nothing the mode cannot.
///
/// # Errors
///
/// `InternalError` naming the first case that answered wrongly.
pub fn self_test_inherit() -> KernelResult<()> {
    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("[acl]   FAIL: inherit: {}", what);
        Err(KernelError::InternalError)
    };
    let perm = |acl: &Acl, tag: AclTag| acl.entries.iter().find(|e| e.tag == tag).map(|e| e.perm.0);
    // user::rwx user:2000:rw- group::r-x mask::rwx other::r--
    let shared = build_acl(
        AclPerm::ALL,
        AclPerm(5),
        AclPerm(4),
        &[(2000, AclPerm(6))],
        &[],
    );
    // A file asked for 0666, as `touch` and editors ask: the owner and the
    // mask lose execute, the others keep read, the named user keeps rw-.
    let (mode, access) = inherit(&shared, 0o666);
    let Some(access) = access else {
        return fail("a default with a named entry gave a new file no ACL");
    };
    if mode != 0o664
        || perm(&access, AclTag::UserObj) != Some(6)
        || perm(&access, AclTag::User(2000)) != Some(6)
        || perm(&access, AclTag::GroupObj) != Some(5)
        || perm(&access, AclTag::Mask) != Some(6)
        || perm(&access, AclTag::Other) != Some(4)
    {
        serial_println!("[acl]     mode {:o}, entries {:?}", mode, access.entries);
        return fail("a file under a default ACL is not what Linux makes");
    }
    let cases: [(&str, u16, u16); 3] = [
        // A directory asked for 0777, as `mkdir` asks.
        ("a directory under it", inherit(&shared, 0o777).0, 0o774),
        // A default the mode bits say all of: the narrowed mode alone.
        (
            "a minimal default's mode",
            inherit(&from_mode(0o750), 0o666).0,
            0o640,
        ),
        // The special bits are the request's.
        (
            "the set-user-ID bit asked for",
            inherit(&from_mode(0o777), 0o4755).0,
            0o4755,
        ),
    ];
    for (what, got, want) in cases {
        if got != want {
            serial_println!("[acl]     {}: {:o}, want {:o}", what, got, want);
            return fail("a mode under a default ACL is wrong");
        }
    }
    if inherit(&from_mode(0o750), 0o666).1.is_some() {
        return fail("a minimal default gave an ACL, where Linux keeps only the mode");
    }
    serial_println!(
        "[acl]   default ACLs: a new file's and directory's mode and ACL, a minimal default, the special bits: OK"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the ACL subsystem.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[acl] Running self-test...");
    // Sections that skip record why, so the closing line cannot claim 12
    // tests passed when one of them did not run.
    let mut skips = crate::fs::selftest::Skips::new();

    // --- Test 1: Minimal ACL from mode ---
    {
        let acl = from_mode(0o755);
        if acl.entries.len() != 3 {
            serial_println!(
                "[acl]   ERROR: from_mode expected 3 entries, got {}",
                acl.entries.len()
            );
            return Err(KernelError::InternalError);
        }

        // Owner should be rwx (7).
        let owner = acl
            .entries
            .iter()
            .find(|e| e.tag == AclTag::UserObj)
            .unwrap();
        if owner.perm.0 != 7 {
            serial_println!("[acl]   ERROR: owner perm {:o}, expected 7", owner.perm.0);
            return Err(KernelError::InternalError);
        }

        // Group should be r-x (5).
        let group = acl
            .entries
            .iter()
            .find(|e| e.tag == AclTag::GroupObj)
            .unwrap();
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
            AclPerm::ALL,          // owner: rwx
            AclPerm::READ,         // group: r--
            AclPerm::NONE,         // other: ---
            &[(2000, AclPerm(6))], // user:2000 has rw-
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
            &[(500, AclPerm(4))], // group:500 has r--
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
                AclEntry {
                    tag: AclTag::GroupObj,
                    perm: AclPerm::ALL,
                },
                AclEntry {
                    tag: AclTag::Other,
                    perm: AclPerm::NONE,
                },
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
                AclEntry {
                    tag: AclTag::UserObj,
                    perm: AclPerm::ALL,
                },
                AclEntry {
                    tag: AclTag::User(100),
                    perm: AclPerm::READ,
                },
                AclEntry {
                    tag: AclTag::GroupObj,
                    perm: AclPerm::ALL,
                },
                AclEntry {
                    tag: AclTag::Other,
                    perm: AclPerm::NONE,
                },
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
        serial_println!(
            "[acl]   stats OK (checks={}, denials={})",
            st.checks_performed,
            st.denials
        );
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
        let result = check_access(
            "/nonexistent/no/acl",
            9999,
            9999,
            0,
            0,
            AccessRequest::WRITE,
        );
        if result.is_err() {
            serial_println!("[acl]   ERROR: no-ACL path should allow");
            return Err(KernelError::InternalError);
        }
        serial_println!("[acl]   no-ACL passthrough OK");
    }

    // --- Test 11: non-UTF-8 paths key distinct ACLs (design-decisions.md §261) ---
    //
    // Asserted through `check_access`, not by round-tripping the table: the
    // table is an implementation detail, whereas `check_access` is what the
    // VFS is meant to consult, and it is the function whose *failure mode*
    // makes the fold dangerous.  `\xFF` and `\xFE` are bytes no UTF-8
    // sequence can begin with, so under a `String` key both names would have
    // collapsed to the same U+FFFD-bearing spelling — and because
    // `check_access` returns `Ok(())` for an unknown path, the collapse
    // would have silently left one of these two files entirely unprotected.
    {
        let a = Path::new(&b"/tmp/_acl_\xFFn"[..]);
        let b = Path::new(&b"/tmp/_acl_\xFEn"[..]);

        // Deny-all-to-others ACL on `a` only.
        let acl = build_acl(AclPerm::ALL, AclPerm::NONE, AclPerm::NONE, &[], &[]);
        set_acl(a, acl)?;

        // `a` is governed by the ACL: a stranger is refused.
        if check_access(a, 9999, 9999, 0, 0, AccessRequest::READ).is_ok() {
            serial_println!("[acl]   ERROR: non-UTF-8 path A should be denied");
            return Err(KernelError::InternalError);
        }
        // `b` has no ACL of its own and must pass through untouched.  If the
        // keys had folded, this call would hit A's deny-all entry.
        if check_access(b, 9999, 9999, 0, 0, AccessRequest::READ).is_err() {
            serial_println!("[acl]   ERROR: non-UTF-8 path B should pass through");
            return Err(KernelError::InternalError);
        }
        // The owner still gets in, so the denial above was the ACL talking
        // and not a blanket refusal.
        if check_access(a, 0, 0, 0, 0, AccessRequest::READ).is_err() {
            serial_println!("[acl]   ERROR: owner should be allowed on path A");
            return Err(KernelError::InternalError);
        }

        // Removal is likewise byte-exact.
        if remove_acl(b) {
            serial_println!("[acl]   ERROR: removing B's absent ACL reported success");
            return Err(KernelError::InternalError);
        }
        if !remove_acl(a) {
            serial_println!("[acl]   ERROR: removing A's ACL reported failure");
            return Err(KernelError::InternalError);
        }
        serial_println!("[acl]   non-UTF-8 paths OK");
    }

    // --- Test 12: an ACL follows the FILE, not the name ---
    //
    // The pre-existing rungs above use synthetic paths that do not exist, so
    // `file_identity` returns NotFound, keying falls back to the path, and
    // every one of them passes identically whether this table is keyed by
    // identity or by name. They are therefore no evidence for the conversion.
    // This rung creates a real file and gives it a second name.
    {
        const A: &[u8] = b"/tmp/acl-id-a";
        const B: &[u8] = b"/tmp/acl-id-b";
        let _ = crate::fs::Vfs::remove(Path::new(A));
        let _ = crate::fs::Vfs::remove(Path::new(B));
        crate::fs::Vfs::write_file(Path::new(A), b"x")?;

        // Classified, not guessed. `.is_err()` would announce "no hard links
        // here" for a link refused with PermissionDenied or ENOSPC -- a cause
        // never established -- and return success.
        let link_unsupported =
            match crate::fs::selftest::classify(crate::fs::Vfs::link(Path::new(A), Path::new(B))) {
                crate::fs::selftest::Setup::Ready => false,
                crate::fs::selftest::Setup::Unsupported(_) => true,
                crate::fs::selftest::Setup::Failed(e) => {
                    serial_println!("[acl]   FAIL: link() refused with {:?}, which is not", e);
                    serial_println!(
                        "[acl]         'this system cannot' -- it was asked and said no"
                    );
                    let _ = crate::fs::Vfs::remove(Path::new(A));
                    return Err(e);
                }
            };
        if link_unsupported {
            serial_println!("[acl]   identity rung SKIPPED -- link() unsupported on this mount");
            skips.record("identity rung", "link() unsupported on /tmp");
            let _ = crate::fs::Vfs::remove(Path::new(A));
        } else {
            let ida = crate::fs::Vfs::file_identity(Path::new(A))?;
            let idb = crate::fs::Vfs::file_identity(Path::new(B))?;
            if ida.is_none() || ida != idb {
                serial_println!("[acl]   identity rung SKIPPED -- {:?} vs {:?}", ida, idb);
                skips.record("identity rung", "two names did not share an identity");
                let _ = crate::fs::Vfs::remove(Path::new(A));
                let _ = crate::fs::Vfs::remove(Path::new(B));
            } else {
                // 0o700: owner rwx, group and other nothing. Requester 1000
                // is neither the owner (0) nor in its group, so it lands on
                // the Other entry and must be refused.
                set_acl(Path::new(A), from_mode(0o700))?;
                let denied_via_a =
                    check_access(Path::new(A), 1000, 1000, 0, 0, AccessRequest::READ).is_err();
                let denied_via_b =
                    check_access(Path::new(B), 1000, 1000, 0, 0, AccessRequest::READ).is_err();
                let _ = remove_acl(Path::new(A));
                let _ = crate::fs::Vfs::remove(Path::new(A));
                let _ = crate::fs::Vfs::remove(Path::new(B));

                // The CONTROL, checked first and separately. Without it a
                // build where `check_access` refuses nothing at all would
                // satisfy the real assertion below by accident, and the rung
                // would report OK having demonstrated nothing (dd-954).
                if !denied_via_a {
                    serial_println!(
                        "[acl]   ERROR: control failed -- a 0o700 ACL did not deny uid 1000"
                    );
                    serial_println!("[acl]          under the file's OWN name, so the");
                    serial_println!("[acl]          identity assertion below proves nothing");
                    return Err(KernelError::InternalError);
                }
                if !denied_via_b {
                    serial_println!(
                        "[acl]   FAIL: the ACL denied under /tmp/acl-id-a but ALLOWED under"
                    );
                    serial_println!("[acl]         /tmp/acl-id-b, a second name for the same");
                    serial_println!("[acl]         inode -- a hard link walks past the ACL");
                    return Err(KernelError::InternalError);
                }
                serial_println!(
                    "[acl]   identity rung OK -- an ACL follows the file, not the name"
                );
            }
        }
    }
    // Test 13: the xattr door's codec and mode arithmetic.
    self_test_xattr()?;
    // Test 14: what a new file takes from a default ACL.
    self_test_inherit()?;
    skips.report("acl");
    serial_println!("[acl] Self-test passed (14 tests){}", skips.suffix());
    Ok(())
}
