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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::groups::{self, CapGroupId};
use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
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

    /// The tagged path, as bytes.
    ///
    /// This used to be a `fn path_str(&self) -> &str` that did
    /// `from_utf8(..).unwrap_or("")`. That was a **fail-open** access-control
    /// bug waiting for the first non-UTF-8 tagged path: the lookup key
    /// (`normalize_path` of a caller-supplied path) always begins with `/`, so
    /// it could never equal the `""` that a non-decodable entry degraded to.
    /// The tag would sit in the registry, `count()` would report it, and
    /// [`effective_tags`] would never find it — every access to the protected
    /// path would be permitted. Comparing bytes has no such failure mode.
    fn path(&self) -> &Path {
        Path::new(self.path.get(..self.path_len).unwrap_or(&[]))
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

/// Number of active entries in [`FILE_TAGS`], readable without the lock.
///
/// The VFS permission gate calls [`count`] on **every** path operation, purely
/// to decide whether there is anything to check at all — and on this machine
/// that is almost always "no". Answering it the obvious way meant taking
/// `FILE_TAGS` and scanning all `MAX_TAGGED_PATHS` slots on a path that is
/// supposed to cost 200–500 ns per component, and worse, made every unrelated
/// file operation contend for one global lock.
///
/// A relaxed load costs nothing and cannot mislead: the count is only ever a
/// filter. A stale "non-zero" costs one redundant scan under the lock, and a
/// stale "zero" can only be read by a thread whose operation was already
/// racing the `tag_path` that would have denied it — the same race the lock
/// would have had.
static TAG_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Recompute [`TAG_COUNT`] from the table.
///
/// Call this with the `FILE_TAGS` guard **still held**, from every path that
/// flips an entry's `active` flag, so the store cannot interleave with another
/// mutator's. Recomputing rather than incrementing is deliberate: the scan is
/// O(`MAX_TAGGED_PATHS`) but happens only when tags change (rare), never on
/// the read path (every file operation), and an absolute recompute cannot
/// drift the way a missed decrement would.
fn publish_count(tags: &[FileTag; MAX_TAGGED_PATHS]) {
    TAG_COUNT.store(tags.iter().filter(|e| e.active).count(), Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Tag a file or directory with a capability group requirement.
///
/// Adding the same group to a path that already has it is a no-op (success).
/// The group must exist in the groups registry.
pub fn tag_path(path: impl AsRef<Path>, group_id: CapGroupId) -> KernelResult<()> {
    let normalized = normalize_path(path.as_ref());
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
        if entry.active && entry.path() == normalized.as_path() {
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
    let slot = tags
        .iter()
        .position(|e| !e.active)
        .ok_or(KernelError::OutOfMemory)?;

    let entry = tags.get_mut(slot).ok_or(KernelError::OutOfMemory)?;
    entry.active = true;
    entry.path_len = normalized.len();
    entry
        .path
        .get_mut(..normalized.len())
        .ok_or(KernelError::InvalidArgument)?
        .copy_from_slice(normalized.as_bytes());
    entry.group_ids[0] = group_id;
    entry.tag_count = 1;
    publish_count(&tags);

    Ok(())
}

/// Remove a capability group tag from a file or directory.
///
/// If this was the last tag, the entry is removed entirely.
pub fn untag_path(path: impl AsRef<Path>, group_id: CapGroupId) -> KernelResult<()> {
    let normalized = normalize_path(path.as_ref());

    let mut tags = FILE_TAGS.lock();
    // Set when an entry is deactivated, so `publish_count` runs once after the
    // `iter_mut` borrow ends rather than inside it.
    let mut deactivated = false;
    for entry in tags.iter_mut() {
        if entry.active && entry.path() == normalized.as_path() {
            for i in 0..entry.tag_count {
                if entry.group_ids[i] == group_id {
                    // Swap with last and shrink.
                    let last = entry.tag_count.saturating_sub(1);
                    entry.group_ids[i] = entry.group_ids[last];
                    entry.tag_count = last;

                    // If no tags remain, deactivate entry.
                    if entry.tag_count == 0 {
                        entry.active = false;
                        deactivated = true;
                    }
                    break;
                }
            }
            // Group not found on this path — not an error; either way this is
            // the only entry that could match, so stop looking.
            break;
        }
    }
    if deactivated {
        publish_count(&tags);
    }
    // Path not found — not an error (idempotent).
    Ok(())
}

/// Remove all tags from a path.
pub fn clear_tags(path: impl AsRef<Path>) -> KernelResult<()> {
    let normalized = normalize_path(path.as_ref());

    let mut tags = FILE_TAGS.lock();
    let mut deactivated = false;
    for entry in tags.iter_mut() {
        if entry.active && entry.path() == normalized.as_path() {
            entry.active = false;
            deactivated = true;
            break;
        }
    }
    if deactivated {
        publish_count(&tags);
    }
    Ok(())
}

/// Get the group tags on a specific path (direct, not inherited).
pub fn get_tags(path: impl AsRef<Path>) -> Vec<CapGroupId> {
    let normalized = normalize_path(path.as_ref());

    let tags = FILE_TAGS.lock();
    for entry in tags.iter() {
        if entry.active && entry.path() == normalized.as_path() {
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
pub fn effective_tags(path: impl AsRef<Path>) -> Vec<CapGroupId> {
    let normalized = normalize_path(path.as_ref());
    let mut result: Vec<CapGroupId> = Vec::new();

    let tags = FILE_TAGS.lock();

    // Collect the tags on one exact path into `result`, de-duplicated.
    let collect = |prefix: &Path, result: &mut Vec<CapGroupId>| {
        for entry in tags.iter() {
            if entry.active && entry.path() == prefix {
                for &gid in entry.group_ids.get(..entry.tag_count).unwrap_or(&[]) {
                    if !result.contains(&gid) {
                        result.push(gid);
                    }
                }
            }
        }
    };

    // Check each ancestor, root first, then the path itself.
    // For "/a/b/c": "/", "/a", "/a/b", "/a/b/c".
    //
    // Walking *components* rather than splitting on `/` is what makes the
    // inheritance boundary component-aligned: a tag on `/a` can never be
    // inherited by `/ab`, no matter how the caller spelled either path.
    let mut prefix = PathBuf::from("/");
    collect(&prefix, &mut result);
    for comp in normalized.components() {
        prefix.push(comp);
        collect(&prefix, &mut result);
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
    path: impl AsRef<Path>,
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
pub fn list_all() -> Vec<(PathBuf, Vec<CapGroupId>)> {
    let tags = FILE_TAGS.lock();
    let mut result = Vec::new();
    for entry in tags.iter() {
        if entry.active {
            result.push((
                entry.path().to_path_buf(),
                entry
                    .group_ids
                    .get(..entry.tag_count)
                    .unwrap_or(&[])
                    .to_vec(),
            ));
        }
    }
    result
}

/// Count of active tagged paths.
///
/// Lock-free: reads the [`TAG_COUNT`] cache rather than scanning the table,
/// because the VFS permission gate calls this on every path operation.
#[must_use]
pub fn count() -> usize {
    TAG_COUNT.load(Ordering::Relaxed)
}

/// Count of active tagged paths, computed by scanning the table under the
/// lock.
///
/// Only the self-test uses this — to prove [`count`]'s cache agrees with the
/// table it claims to describe, which is the assertion that would catch a
/// future mutator that forgets to call [`publish_count`].
fn count_uncached() -> usize {
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
    // Unconditional: this walks every entry, so tracking whether any were
    // deactivated would cost more than the recompute it would save.
    publish_count(&tags);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalize a path: ensure a leading slash, collapse `//`, strip a trailing
/// `/`.
///
/// Rebuilding from [`Path::components`] does all three at once: the iterator
/// already drops empty components (which is what a `//` run or a trailing `/`
/// produces), and seeding the buffer with `/` makes the result absolute.
/// `.`/`..` are *not* resolved here — they never were, and the VFS resolves
/// them before a path reaches the tag check.
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::with_capacity(path.len().saturating_add(1));
    result.extend_bytes(b"/");
    for comp in path.components() {
        result.push(comp);
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
    test_non_utf8_and_boundaries()?;
    test_count_cache_tracks_table()?;

    serial_println!("[cap/file_tags] File capability tags self-test PASSED");
    Ok(())
}

/// Test 7: the lock-free [`count`] cache agrees with the table it describes.
///
/// [`count`] stopped scanning `FILE_TAGS` when the VFS permission gate began
/// calling it on every path operation — a mutex plus an O(n) scan is not
/// affordable on a lookup path budgeted at 200–500 ns per component. The price
/// of that is a cache that a future mutator can forget to refresh, and a stale
/// *zero* is not a slow answer but a wrong one: the gate reads it as "no tags
/// exist" and skips the check entirely, silently disabling mandatory access
/// control. So the invariant is asserted here after every kind of mutation the
/// module offers, against a fresh scan of the table.
fn test_count_cache_tracks_table() -> KernelResult<()> {
    fn agree(stage: &str) -> KernelResult<()> {
        let cached = count();
        let scanned = count_uncached();
        if cached == scanned {
            return Ok(());
        }
        serial_println!(
            "[cap/file_tags]   FAIL: count cache is {} but the table holds {} ({})",
            cached,
            scanned,
            stage
        );
        Err(KernelError::InternalError)
    }

    let baseline = count_uncached();
    agree("before any mutation")?;

    let gid_a = groups::create("ftag_count_a")?;
    let gid_b = groups::create("ftag_count_b")?;

    // tag_path: a brand-new entry.
    tag_path("/count/one", gid_a)?;
    agree("after tag_path created an entry")?;
    if count() != baseline.saturating_add(1) {
        serial_println!("[cap/file_tags]   FAIL: tag_path did not raise the count");
        clear_tags("/count/one").ok();
        groups::remove(gid_a).ok();
        groups::remove(gid_b).ok();
        return Err(KernelError::InternalError);
    }

    // tag_path again on the same path: adds a tag, not an entry.
    tag_path("/count/one", gid_b)?;
    agree("after a second tag on the same path")?;

    // untag_path with a tag remaining: the entry stays active.
    untag_path("/count/one", gid_b)?;
    agree("after untag_path left one tag")?;

    // untag_path of the last tag: the entry is deactivated.
    untag_path("/count/one", gid_a)?;
    agree("after untag_path removed the last tag")?;

    // clear_tags.
    tag_path("/count/two", gid_a)?;
    agree("after tag_path for clear_tags")?;
    clear_tags("/count/two")?;
    agree("after clear_tags")?;

    // remove_group_references, which deactivates entries in bulk.
    tag_path("/count/three", gid_a)?;
    tag_path("/count/four", gid_a)?;
    agree("after tagging two paths")?;
    remove_group_references(gid_a);
    agree("after remove_group_references")?;

    groups::remove(gid_a).ok();
    groups::remove(gid_b).ok();

    if count() != baseline {
        serial_println!(
            "[cap/file_tags]   FAIL: count is {} but started at {}",
            count(),
            baseline
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[cap/file_tags]   Count cache tracks the table: OK");
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
            serial_println!(
                "[cap/file_tags]   FAIL: other single-member allowed: {:?}",
                other
            );
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

/// Test 6: non-UTF-8 tagged paths, and component-aligned inheritance.
///
/// Both halves are regressions against the same root cause — the registry
/// used to key on a lossily-decoded `&str`:
///
/// * A tag on a path containing a non-UTF-8 byte decoded to `""`, which no
///   lookup key could ever equal, so the tag was **inert** and every access
///   to the protected path was permitted (fail-open).
/// * Two *different* non-UTF-8 paths both decoded to `""`, so a tag on one
///   would have matched the other had a lookup key ever been `""`.
///
/// The boundary half pins the other property a byte-prefix scheme gets
/// wrong: `/secure` must not confer its tags on `/secureX`.
fn test_non_utf8_and_boundaries() -> KernelResult<()> {
    let gid = groups::create("ftag_test6")?;

    // A directory whose name is not valid UTF-8.  `\xff` is never a legal
    // UTF-8 byte anywhere in a sequence, so this path cannot be spelled as
    // a `&str` at all.
    let secret = Path::new(b"/vault/\xff");
    let sibling = Path::new(b"/vault/\xfe");

    let cleanup = |gid| {
        clear_tags(Path::new(b"/vault/\xff")).ok();
        clear_tags("/secure").ok();
        groups::remove(gid).ok();
    };

    tag_path(secret, gid)?;

    // The tag must be findable by the exact same bytes.
    if !get_tags(secret).contains(&gid) {
        serial_println!("[cap/file_tags]   FAIL: non-UTF-8 tag not found after adding");
        cleanup(gid);
        return Err(KernelError::InternalError);
    }

    // A child of it inherits.
    if !effective_tags(Path::new(b"/vault/\xff/file")).contains(&gid) {
        serial_println!("[cap/file_tags]   FAIL: non-UTF-8 child did not inherit");
        cleanup(gid);
        return Err(KernelError::InternalError);
    }

    // A sibling differing only in that one byte does NOT.  This is the case
    // that lossy decoding collapsed together.
    if effective_tags(sibling).contains(&gid) {
        serial_println!("[cap/file_tags]   FAIL: distinct non-UTF-8 sibling matched");
        cleanup(gid);
        return Err(KernelError::InternalError);
    }

    // And a non-root, non-member process is actually denied — i.e. the tag
    // is live, not inert.  gid 4242 is in no group.
    if check_access(1000, 4242, &[], Path::new(b"/vault/\xff/file")).is_ok() {
        serial_println!("[cap/file_tags]   FAIL: non-UTF-8 tag failed open");
        cleanup(gid);
        return Err(KernelError::InternalError);
    }

    clear_tags(secret).ok();

    // Component boundary: a tag on "/secure" covers "/secure/x" but never
    // "/secureX", regardless of trailing-slash spelling.
    tag_path("/secure/", gid)?;
    for (probe, want) in [
        ("/secure", true),
        ("/secure/x", true),
        ("/secure//x", true),
        ("/secureX", false),
        ("/secureX/y", false),
    ] {
        if effective_tags(probe).contains(&gid) != want {
            serial_println!(
                "[cap/file_tags]   FAIL: boundary: {} should{} inherit",
                probe,
                if want { "" } else { " not" }
            );
            cleanup(gid);
            return Err(KernelError::InternalError);
        }
    }

    cleanup(gid);
    serial_println!("[cap/file_tags]   Non-UTF-8 + boundaries: OK");
    Ok(())
}
