//! File/directory capability tags — path-based access control.
//!
//! Files and directories can be tagged with one or more capability group IDs.
//! When a process attempts to access a tagged path, the kernel enforces:
//!
//! - **AND-composition between groups**: if a path is tagged with groups
//!   A and B, the process must be a member of *both* A and B.
//! - **OR within a group**: having any of the process's gids match any
//!   member gid of the group satisfies that group's requirement.
//! - **Root (uid=0) bypass**: root always passes.
//! - **Inheritance**: a tagged directory's tags apply to all files and
//!   subdirectories beneath it (deepest ancestor wins for accumulated tags).
//!
//! ## Design (from design.txt)
//!
//! > "file/directory capabilities compose via intersection (AND), meaning
//! > that if a file or directory specifies more than one capability, all
//! > of them are required by a user or process to access it."
//!
//! > "If a file or directory has a capability group in its list, do the
//! > individual capabilities in the group compose via AND or OR? I think
//! > OR is right."
//!
//! ## Storage
//!
//! Tags are stored in an in-memory registry keyed by normalized path.
//! In the future, these can be persisted via extended attributes
//! (security.cap_tags xattr) on filesystems that support them.
//!
//! ## Lock ordering
//!
//! `FILE_TAGS` does not call into VFS, scheduler, or GROUPS lock.
//! Safe to acquire `GROUPS` lock *after* `FILE_TAGS` if needed
//! (but currently we release `FILE_TAGS` before checking membership).

use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use super::groups::{self, CapGroupId};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of tagged paths in the registry.
const MAX_TAGGED_PATHS: usize = 128;

/// Maximum group tags per path.
const MAX_TAGS_PER_PATH: usize = 8;

/// Maximum path length for a tagged entry.
const MAX_PATH_LEN: usize = 255;

// ---------------------------------------------------------------------------
// Tag entry
// ---------------------------------------------------------------------------

/// A file/directory capability tag entry.
struct FileTag {
    /// Whether this slot is active.
    active: bool,
    /// The tagged path (normalized, absolute).
    path: [u8; MAX_PATH_LEN + 1],
    /// Length of path.
    path_len: usize,
    /// Group IDs required to access this path.
    group_ids: [CapGroupId; MAX_TAGS_PER_PATH],
    /// Number of active group tags.
    tag_count: usize,
}

impl FileTag {
    const fn empty() -> Self {
        Self {
            active: false,
            path: [0; MAX_PATH_LEN + 1],
            path_len: 0,
            group_ids: [0; MAX_TAGS_PER_PATH],
            tag_count: 0,
        }
    }

    /// Get the path as a string slice.
    fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("")
    }
}

// ---------------------------------------------------------------------------
// Global tag registry
// ---------------------------------------------------------------------------

/// Global registry of file/directory capability tags.
static FILE_TAGS: Mutex<[FileTag; MAX_TAGGED_PATHS]> = Mutex::new({
    const EMPTY: FileTag = FileTag::empty();
    [EMPTY; MAX_TAGGED_PATHS]
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Tag a file or directory with a capability group requirement.
///
/// Adding the same group to a path that already has it is a no-op (success).
/// The group must exist in the groups registry.
pub fn tag_path(path: &str, group_id: CapGroupId) -> KernelResult<()> {
    let normalized = normalize_path(path);
    if normalized.is_empty() || normalized.len() > MAX_PATH_LEN {
        return Err(KernelError::InvalidArgument);
    }

    // Verify the group exists.
    if !group_exists(group_id) {
        return Err(KernelError::InvalidHandle);
    }

    let mut tags = FILE_TAGS.lock();

    // Check if path already has an entry.
    for entry in tags.iter_mut() {
        if entry.active && entry.path_str() == normalized {
            // Check for duplicate tag.
            for i in 0..entry.tag_count {
                if entry.group_ids[i] == group_id {
                    return Ok(()); // Already tagged.
                }
            }
            // Add new tag.
            if entry.tag_count >= MAX_TAGS_PER_PATH {
                return Err(KernelError::OutOfMemory);
            }
            entry.group_ids[entry.tag_count] = group_id;
            entry.tag_count = entry.tag_count.saturating_add(1);
            return Ok(());
        }
    }

    // Create new entry.
    let slot = tags.iter().position(|e| !e.active)
        .ok_or(KernelError::OutOfMemory)?;

    let entry = &mut tags[slot];
    entry.active = true;
    entry.path_len = normalized.len();
    entry.path[..normalized.len()].copy_from_slice(normalized.as_bytes());
    entry.group_ids[0] = group_id;
    entry.tag_count = 1;

    Ok(())
}

/// Remove a capability group tag from a file or directory.
///
/// If this was the last tag, the entry is removed entirely.
pub fn untag_path(path: &str, group_id: CapGroupId) -> KernelResult<()> {
    let normalized = normalize_path(path);

    let mut tags = FILE_TAGS.lock();
    for entry in tags.iter_mut() {
        if entry.active && entry.path_str() == normalized {
            for i in 0..entry.tag_count {
                if entry.group_ids[i] == group_id {
                    // Swap with last and shrink.
                    let last = entry.tag_count.saturating_sub(1);
                    entry.group_ids[i] = entry.group_ids[last];
                    entry.tag_count = last;

                    // If no tags remain, deactivate entry.
                    if entry.tag_count == 0 {
                        entry.active = false;
                    }
                    return Ok(());
                }
            }
            // Group not found on this path — not an error.
            return Ok(());
        }
    }
    // Path not found — not an error (idempotent).
    Ok(())
}

/// Remove all tags from a path.
pub fn clear_tags(path: &str) -> KernelResult<()> {
    let normalized = normalize_path(path);

    let mut tags = FILE_TAGS.lock();
    for entry in tags.iter_mut() {
        if entry.active && entry.path_str() == normalized {
            entry.active = false;
            return Ok(());
        }
    }
    Ok(())
}

/// Get the group tags on a specific path (direct, not inherited).
pub fn get_tags(path: &str) -> Vec<CapGroupId> {
    let normalized = normalize_path(path);

    let tags = FILE_TAGS.lock();
    for entry in tags.iter() {
        if entry.active && entry.path_str() == normalized {
            return entry.group_ids[..entry.tag_count].to_vec();
        }
    }
    Vec::new()
}

/// Get all effective tags for a path (including inherited from ancestors).
///
/// Walks up the path hierarchy and collects all tags from ancestors.
/// All collected tags compose via AND — the process must be a member of
/// every group found on the path or any ancestor.
pub fn effective_tags(path: &str) -> Vec<CapGroupId> {
    let normalized = normalize_path(path);
    let mut result: Vec<CapGroupId> = Vec::new();

    let tags = FILE_TAGS.lock();

    // Check each ancestor (including the path itself).
    // For "/a/b/c", check "/", "/a", "/a/b", "/a/b/c".
    let mut prefix = String::new();
    let parts: Vec<&str> = normalized.split('/').collect();

    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            // Root.
            prefix.push('/');
        } else {
            if prefix.len() > 1 {
                prefix.push('/');
            }
            prefix.push_str(part);
        }

        // Find tags for this prefix.
        for entry in tags.iter() {
            if entry.active && entry.path_str() == prefix {
                for &gid in &entry.group_ids[..entry.tag_count] {
                    if !result.contains(&gid) {
                        result.push(gid);
                    }
                }
            }
        }
    }

    result
}

/// Check whether a process can access a tagged path.
///
/// Returns `Ok(())` if access is allowed, `Err(PermissionDenied)` otherwise.
///
/// ## Semantics
///
/// 1. Root (uid=0) always passes.
/// 2. Collect all effective group tags on the path (direct + inherited).
/// 3. For each required group: check if the process's gids match any
///    member gid of that group (OR within group).
/// 4. ALL required groups must pass (AND between groups).
pub fn check_access(
    uid: u32,
    primary_gid: u32,
    supplementary_gids: &[u32],
    path: &str,
) -> KernelResult<()> {
    // Root bypasses all tag checks.
    if uid == 0 {
        return Ok(());
    }

    let required_groups = effective_tags(path);

    // If no tags, access is unrestricted.
    if required_groups.is_empty() {
        return Ok(());
    }

    // AND-composition: process must be a member of ALL required groups.
    for &group_id in &required_groups {
        if !groups::is_member(group_id, primary_gid, supplementary_gids) {
            return Err(KernelError::PermissionDenied);
        }
    }

    Ok(())
}

/// List all tagged paths (for kshell/procfs).
///
/// Returns (path, group_ids) pairs for active entries.
pub fn list_all() -> Vec<(String, Vec<CapGroupId>)> {
    let tags = FILE_TAGS.lock();
    let mut result = Vec::new();
    for entry in tags.iter() {
        if entry.active {
            result.push((
                String::from(entry.path_str()),
                entry.group_ids[..entry.tag_count].to_vec(),
            ));
        }
    }
    result
}

/// Count of active tagged paths.
pub fn count() -> usize {
    let tags = FILE_TAGS.lock();
    tags.iter().filter(|e| e.active).count()
}

/// Remove all tags that reference a specific group ID.
///
/// Called when a capability group is deleted to clean up dangling references.
pub fn remove_group_references(group_id: CapGroupId) {
    let mut tags = FILE_TAGS.lock();
    for entry in tags.iter_mut() {
        if !entry.active {
            continue;
        }
        // Remove this group_id from the entry's tag list.
        let mut i = 0;
        while i < entry.tag_count {
            if entry.group_ids[i] == group_id {
                let last = entry.tag_count.saturating_sub(1);
                entry.group_ids[i] = entry.group_ids[last];
                entry.tag_count = last;
                // Don't increment i — check the swapped-in value.
            } else {
                i += 1;
            }
        }
        // If no tags remain, deactivate.
        if entry.tag_count == 0 {
            entry.active = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalize a path: ensure leading slash, collapse "//", strip trailing "/".
fn normalize_path(path: &str) -> String {
    let mut result = String::with_capacity(path.len());

    if !path.starts_with('/') {
        result.push('/');
    }

    let mut last_was_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !last_was_slash {
                result.push('/');
            }
            last_was_slash = true;
        } else {
            result.push(ch);
            last_was_slash = false;
        }
    }

    // Strip trailing slash (unless root).
    if result.len() > 1 && result.ends_with('/') {
        result.pop();
    }

    result
}

/// Check if a group ID exists in the groups registry.
fn group_exists(group_id: CapGroupId) -> bool {
    // Use find_by_id (if available) or list-based check.
    // Since we don't have a direct lookup by ID, iterate the list.
    let all = groups::list();
    all.iter().any(|(id, _, _, _, _)| *id == group_id)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run file tag self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[cap/file_tags] Running file capability tags self-test...");

    test_tag_untag()?;
    test_inheritance()?;
    test_access_check_basic()?;
    test_and_composition()?;
    test_remove_group_refs()?;

    serial_println!("[cap/file_tags] File capability tags self-test PASSED");
    Ok(())
}

/// Test 1: basic tag and untag operations.
fn test_tag_untag() -> KernelResult<()> {
    // Create a test group.
    let gid = groups::create("ftag_test1")?;

    // Tag a path.
    tag_path("/test/secret", gid)?;

    // Should appear in tags.
    let tags = get_tags("/test/secret");
    if tags.len() != 1 || tags[0] != gid {
        serial_println!("[cap/file_tags]   FAIL: tag not found after adding");
        clear_tags("/test/secret").ok();
        groups::remove(gid).ok();
        return Err(KernelError::InternalError);
    }

    // Duplicate tag is idempotent.
    tag_path("/test/secret", gid)?;
    let tags = get_tags("/test/secret");
    if tags.len() != 1 {
        serial_println!("[cap/file_tags]   FAIL: duplicate tag doubled count");
        clear_tags("/test/secret").ok();
        groups::remove(gid).ok();
        return Err(KernelError::InternalError);
    }

    // Untag.
    untag_path("/test/secret", gid)?;
    let tags = get_tags("/test/secret");
    if !tags.is_empty() {
        serial_println!("[cap/file_tags]   FAIL: tag still present after untag");
        clear_tags("/test/secret").ok();
        groups::remove(gid).ok();
        return Err(KernelError::InternalError);
    }

    groups::remove(gid).ok();
    serial_println!("[cap/file_tags]   Tag/untag: OK");
    Ok(())
}

/// Test 2: tag inheritance from parent directories.
fn test_inheritance() -> KernelResult<()> {
    let gid = groups::create("ftag_test2")?;

    // Tag a parent directory.
    tag_path("/secure", gid)?;

    // Child paths should inherit.
    let eff = effective_tags("/secure/subdir/file.txt");
    if !eff.contains(&gid) {
        serial_println!("[cap/file_tags]   FAIL: child didn't inherit parent tag");
        clear_tags("/secure").ok();
        groups::remove(gid).ok();
        return Err(KernelError::InternalError);
    }

    // Unrelated path should not inherit.
    let eff2 = effective_tags("/other/file.txt");
    if eff2.contains(&gid) {
        serial_println!("[cap/file_tags]   FAIL: unrelated path inherited tag");
        clear_tags("/secure").ok();
        groups::remove(gid).ok();
        return Err(KernelError::InternalError);
    }

    clear_tags("/secure").ok();
    groups::remove(gid).ok();
    serial_println!("[cap/file_tags]   Inheritance: OK");
    Ok(())
}

/// Test 3: basic access check (member passes, non-member denied).
fn test_access_check_basic() -> KernelResult<()> {
    let gid = groups::create("ftag_test3")?;

    // Add OS gid 1000 as a member of this group.
    groups::add_member(gid, 1000)?;

    // Tag a path.
    tag_path("/protected/data", gid)?;

    // Process with gid 1000 should pass.
    match check_access(500, 1000, &[], "/protected/data") {
        Ok(()) => {}
        Err(e) => {
            serial_println!("[cap/file_tags]   FAIL: member denied: {:?}", e);
            clear_tags("/protected/data").ok();
            groups::remove(gid).ok();
            return Err(KernelError::InternalError);
        }
    }

    // Process with gid 2000 should be denied.
    match check_access(500, 2000, &[], "/protected/data") {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[cap/file_tags]   FAIL: non-member allowed: {:?}", other);
            clear_tags("/protected/data").ok();
            groups::remove(gid).ok();
            return Err(KernelError::InternalError);
        }
    }

    // Root always passes.
    match check_access(0, 2000, &[], "/protected/data") {
        Ok(()) => {}
        Err(e) => {
            serial_println!("[cap/file_tags]   FAIL: root denied: {:?}", e);
            clear_tags("/protected/data").ok();
            groups::remove(gid).ok();
            return Err(KernelError::InternalError);
        }
    }

    clear_tags("/protected/data").ok();
    groups::remove(gid).ok();
    serial_println!("[cap/file_tags]   Access check: OK");
    Ok(())
}

/// Test 4: AND-composition between multiple groups.
fn test_and_composition() -> KernelResult<()> {
    let gid_a = groups::create("ftag_test4a")?;
    let gid_b = groups::create("ftag_test4b")?;

    // Add different OS gids as members.
    groups::add_member(gid_a, 100)?; // OS group 100 → cap group A
    groups::add_member(gid_b, 200)?; // OS group 200 → cap group B

    // Tag path with both groups (AND — must be member of BOTH).
    tag_path("/top_secret", gid_a)?;
    tag_path("/top_secret", gid_b)?;

    // Process in both groups → allowed.
    match check_access(500, 100, &[200], "/top_secret") {
        Ok(()) => {}
        Err(e) => {
            serial_println!("[cap/file_tags]   FAIL: dual-member denied: {:?}", e);
            clear_tags("/top_secret").ok();
            groups::remove(gid_a).ok();
            groups::remove(gid_b).ok();
            return Err(KernelError::InternalError);
        }
    }

    // Process in only group A → denied.
    match check_access(500, 100, &[], "/top_secret") {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[cap/file_tags]   FAIL: single-member allowed: {:?}", other);
            clear_tags("/top_secret").ok();
            groups::remove(gid_a).ok();
            groups::remove(gid_b).ok();
            return Err(KernelError::InternalError);
        }
    }

    // Process in only group B �� denied.
    match check_access(500, 200, &[], "/top_secret") {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[cap/file_tags]   FAIL: other single-member allowed: {:?}", other);
            clear_tags("/top_secret").ok();
            groups::remove(gid_a).ok();
            groups::remove(gid_b).ok();
            return Err(KernelError::InternalError);
        }
    }

    // Process in neither → denied.
    match check_access(500, 999, &[], "/top_secret") {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[cap/file_tags]   FAIL: non-member allowed: {:?}", other);
            clear_tags("/top_secret").ok();
            groups::remove(gid_a).ok();
            groups::remove(gid_b).ok();
            return Err(KernelError::InternalError);
        }
    }

    clear_tags("/top_secret").ok();
    groups::remove(gid_a).ok();
    groups::remove(gid_b).ok();
    serial_println!("[cap/file_tags]   AND-composition: OK");
    Ok(())
}

/// Test 5: removing a group cleans up file tags.
fn test_remove_group_refs() -> KernelResult<()> {
    let gid = groups::create("ftag_test5")?;

    tag_path("/ephemeral", gid)?;

    // Tags should be present.
    let tags = get_tags("/ephemeral");
    if tags.is_empty() {
        serial_println!("[cap/file_tags]   FAIL: tag missing before removal");
        groups::remove(gid).ok();
        return Err(KernelError::InternalError);
    }

    // Clean up references.
    remove_group_references(gid);

    // Tags should be gone.
    let tags = get_tags("/ephemeral");
    if !tags.is_empty() {
        serial_println!("[cap/file_tags]   FAIL: tag still present after group removal");
        clear_tags("/ephemeral").ok();
        groups::remove(gid).ok();
        return Err(KernelError::InternalError);
    }

    groups::remove(gid).ok();
    serial_println!("[cap/file_tags]   Remove group refs: OK");
    Ok(())
}
