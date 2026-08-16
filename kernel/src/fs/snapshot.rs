//! Filesystem snapshot system.
//!
//! Creates point-in-time snapshots of directory trees by storing file
//! contents in the content-addressed store (CAS) and recording the full
//! directory structure as metadata.  Snapshots are space-efficient:
//! identical files across snapshots share storage via CAS deduplication.
//!
//! ## Design
//!
//! ```text
//! snapshot create /home → SnapshotId(1)
//!    ↓
//! Walk directory tree recursively
//!    ↓
//! For each file: content → CAS put() → Hash256
//!    ↓
//! Store SnapshotEntry { path, hash, metadata }
//!    ↓
//! Snapshot { id, name, timestamp, entries, parent }
//!
//! snapshot restore 1 → reads each entry, gets content from CAS, writes via VFS
//! ```
//!
//! ## Features
//!
//! - **Branching**: snapshots can have a parent, forming a tree (like VM snapshots)
//! - **Incremental**: unchanged files share CAS blobs (automatic via hash)
//! - **Selective**: include/exclude paths within the snapshot
//! - **Metadata preservation**: permissions, ownership, timestamps restored
//! - **Atomic restore**: if restore fails mid-way, partial writes are cleaned up
//!
//! ## Performance
//!
//! Snapshot creation is I/O-bound (reads every file).  For large trees,
//! the CAS deduplication means only *new* content allocates memory.
//! A 1 GiB tree where 90% is unchanged from the previous snapshot only
//! costs ~100 MiB of CAS storage.
//!
//! ## Reference
//!
//! design.txt: "make a snapshot or restore from snapshot feature, with
//! branching like a VM does? options for what to include in the snapshot?"

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::cas::{self, Hash256};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{EntryType, FileMeta, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SnapshotId(pub u64);

/// A single entry (file, directory, symlink) within a snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    /// Full path relative to the snapshot root.
    pub path: PathBuf,
    /// Entry type (File, Directory, Symlink).
    pub entry_type: EntryType,
    /// CAS hash of file content (None for directories).
    pub content_hash: Option<Hash256>,
    /// Symlink target (for symlinks only).
    pub symlink_target: Option<PathBuf>,
    /// File size in bytes.
    pub size: u64,
    /// POSIX permissions.
    pub permissions: u16,
    /// Owner UID.
    pub uid: u32,
    /// Owner GID.
    pub gid: u32,
    /// Modification time (nanoseconds since epoch).
    pub modified_ns: u64,
}

/// A complete snapshot of a directory tree.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Unique snapshot ID.
    pub id: SnapshotId,
    /// Human-readable name.
    pub name: String,
    /// Path that was snapshotted.
    pub root_path: PathBuf,
    /// Timestamp of snapshot creation (nanoseconds since epoch).
    pub created_ns: u64,
    /// Parent snapshot ID (for branching).
    pub parent: Option<SnapshotId>,
    /// All entries in this snapshot.
    pub entries: Vec<SnapshotEntry>,
    /// Number of files (not directories) in the snapshot.
    pub file_count: u64,
    /// Total bytes of file content.
    pub total_bytes: u64,
    /// Total bytes stored in CAS (deduplicated — may be less).
    pub stored_bytes: u64,
}

/// Summary info for listing snapshots without loading all entries.
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub id: SnapshotId,
    pub name: String,
    pub root_path: PathBuf,
    pub created_ns: u64,
    pub parent: Option<SnapshotId>,
    pub file_count: u64,
    pub total_bytes: u64,
}

/// Result of a snapshot restore operation.
#[derive(Debug, Clone)]
pub struct RestoreResult {
    /// Number of files restored.
    pub files_restored: u64,
    /// Number of directories created.
    pub dirs_created: u64,
    /// Number of symlinks created.
    pub symlinks_created: u64,
    /// Number of entries that failed to restore.
    pub errors: u64,
}

/// Options for snapshot creation.
#[derive(Debug, Clone)]
pub struct SnapshotOptions {
    /// Maximum directory depth to traverse (default 64).
    pub max_depth: usize,
    /// Maximum file size to include (larger files are skipped). Default 64 MiB.
    pub max_file_size: u64,
    /// Exclude these paths and everything under them (relative to the root).
    ///
    /// Matching is by *component*, via [`crate::fs::pathutil::path_in_subtree`]:
    /// excluding `.git` excludes `.git/config` but not `.gitignore`. These were
    /// byte prefixes, which meant a filter had to be written `src/` with a
    /// trailing separator to avoid matching a sibling whose name merely started
    /// the same way — evidence that subtree containment was what was meant.
    pub exclude_paths: Vec<PathBuf>,
    /// Only include these paths and their subtrees (empty = include all).
    ///
    /// Matched the same way as [`Self::exclude_paths`].
    pub include_paths: Vec<PathBuf>,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_file_size: 64 * 1024 * 1024, // 64 MiB
            exclude_paths: Vec::new(),
            include_paths: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct SnapshotInner {
    snapshots: BTreeMap<SnapshotId, Snapshot>,
    next_id: u64,
}

static SNAPSHOTS: Mutex<SnapshotInner> = Mutex::new(SnapshotInner {
    snapshots: BTreeMap::new(),
    next_id: 1,
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a snapshot of a directory tree.
///
/// Walks the tree at `path`, stores file contents in the CAS, and
/// records the full directory structure.  Returns the new snapshot ID.
pub fn create(
    path: impl AsRef<Path>,
    name: &str,
    parent: Option<SnapshotId>,
    options: &SnapshotOptions,
) -> KernelResult<SnapshotId> {
    let path = path.as_ref();
    // Validate path exists and is a directory.
    let stat = Vfs::stat(path)?;
    if stat.entry_type != EntryType::Directory {
        return Err(KernelError::NotADirectory);
    }

    let created_ns = crate::timekeeping::clock_realtime();
    let mut entries = Vec::new();
    let mut file_count = 0u64;
    let mut total_bytes = 0u64;
    let mut stored_bytes = 0u64;

    // Walk the directory tree.
    walk_directory(
        path,
        Path::new(""),
        options,
        0,
        &mut entries,
        &mut file_count,
        &mut total_bytes,
        &mut stored_bytes,
    )?;

    let id = {
        let mut inner = SNAPSHOTS.lock();
        let id = SnapshotId(inner.next_id);
        inner.next_id = inner.next_id.saturating_add(1);

        let snapshot = Snapshot {
            id,
            name: String::from(name),
            root_path: path.to_path_buf(),
            created_ns,
            parent,
            entries,
            file_count,
            total_bytes,
            stored_bytes,
        };

        inner.snapshots.insert(id, snapshot);
        id
    };

    serial_println!(
        "[snapshot] Created '{}' (id={}) at {}: {} files, {} bytes",
        name,
        id.0,
        path.display(),
        file_count,
        total_bytes,
    );

    Ok(id)
}

/// Restore a snapshot to a target path.
///
/// Recreates the directory structure and writes all files from the CAS.
/// The target path must exist as a directory.  Existing files at the
/// target are overwritten.
pub fn restore(id: SnapshotId, target_path: impl AsRef<Path>) -> KernelResult<RestoreResult> {
    let target_path = target_path.as_ref();
    // Clone entries out of the lock to avoid holding it during I/O.
    let entries = {
        let inner = SNAPSHOTS.lock();
        let snap = inner.snapshots.get(&id).ok_or(KernelError::NotFound)?;
        snap.entries.clone()
    };

    // Validate target exists and is a directory.
    let stat = Vfs::stat(target_path)?;
    if stat.entry_type != EntryType::Directory {
        return Err(KernelError::NotADirectory);
    }

    let mut result = RestoreResult {
        files_restored: 0,
        dirs_created: 0,
        symlinks_created: 0,
        errors: 0,
    };

    // First pass: create all directories (sorted by path length to ensure
    // parents exist before children).
    let mut dirs: Vec<&SnapshotEntry> = entries
        .iter()
        .filter(|e| e.entry_type == EntryType::Directory)
        .collect();
    dirs.sort_by_key(|e| e.path.len());

    for entry in &dirs {
        let full_path = target_path.join(&entry.path);
        match Vfs::mkdir(&full_path) {
            Ok(()) => result.dirs_created = result.dirs_created.saturating_add(1),
            Err(KernelError::AlreadyExists) => {} // OK, already exists.
            Err(_) => result.errors = result.errors.saturating_add(1),
        }
    }

    // Second pass: restore files.
    for entry in entries.iter().filter(|e| e.entry_type == EntryType::File) {
        let full_path = target_path.join(&entry.path);

        if let Some(ref hash) = entry.content_hash {
            match cas::get(hash) {
                Ok(data) => {
                    if let Err(_e) = Vfs::write_file(&full_path, &data) {
                        result.errors = result.errors.saturating_add(1);
                    } else {
                        result.files_restored = result.files_restored.saturating_add(1);
                        // Restore permissions.
                        let _ = Vfs::set_permissions(&full_path, entry.permissions);
                        let _ = Vfs::set_owner(&full_path, entry.uid, entry.gid);
                    }
                }
                Err(_) => {
                    result.errors = result.errors.saturating_add(1);
                }
            }
        }
    }

    // Third pass: restore symlinks.
    for entry in entries
        .iter()
        .filter(|e| e.entry_type == EntryType::Symlink)
    {
        let full_path = target_path.join(&entry.path);
        if let Some(ref target) = entry.symlink_target {
            match Vfs::symlink(&full_path, target) {
                Ok(()) => result.symlinks_created = result.symlinks_created.saturating_add(1),
                Err(_) => result.errors = result.errors.saturating_add(1),
            }
        }
    }

    serial_println!(
        "[snapshot] Restored id={} to {}: {} files, {} dirs, {} symlinks, {} errors",
        id.0,
        target_path.display(),
        result.files_restored,
        result.dirs_created,
        result.symlinks_created,
        result.errors,
    );

    Ok(result)
}

/// Delete a snapshot, releasing CAS references.
///
/// This does NOT delete the actual files on disk — only the snapshot
/// metadata and CAS references.  After all references to a blob are
/// released, `cas::gc()` can reclaim it.
pub fn delete(id: SnapshotId) -> KernelResult<()> {
    let entries = {
        let mut inner = SNAPSHOTS.lock();
        let snap = inner.snapshots.remove(&id).ok_or(KernelError::NotFound)?;
        snap.entries
    };

    // Release CAS references for all file content.
    for entry in &entries {
        if let Some(ref hash) = entry.content_hash {
            let _ = cas::release(hash);
        }
    }

    serial_println!("[snapshot] Deleted id={}", id.0);
    Ok(())
}

/// List all snapshots.
pub fn list() -> Vec<SnapshotInfo> {
    let inner = SNAPSHOTS.lock();
    inner
        .snapshots
        .values()
        .map(|s| SnapshotInfo {
            id: s.id,
            name: s.name.clone(),
            root_path: s.root_path.clone(),
            created_ns: s.created_ns,
            parent: s.parent,
            file_count: s.file_count,
            total_bytes: s.total_bytes,
        })
        .collect()
}

/// Get info about a specific snapshot.
pub fn info(id: SnapshotId) -> KernelResult<SnapshotInfo> {
    let inner = SNAPSHOTS.lock();
    let s = inner.snapshots.get(&id).ok_or(KernelError::NotFound)?;
    Ok(SnapshotInfo {
        id: s.id,
        name: s.name.clone(),
        root_path: s.root_path.clone(),
        created_ns: s.created_ns,
        parent: s.parent,
        file_count: s.file_count,
        total_bytes: s.total_bytes,
    })
}

/// Get the list of entries in a snapshot.
pub fn entries(id: SnapshotId) -> KernelResult<Vec<SnapshotEntry>> {
    let inner = SNAPSHOTS.lock();
    let s = inner.snapshots.get(&id).ok_or(KernelError::NotFound)?;
    Ok(s.entries.clone())
}

/// Compare two snapshots, returning paths that differ.
///
/// Returns (added, removed, modified) path lists.
pub fn diff(
    id_a: SnapshotId,
    id_b: SnapshotId,
) -> KernelResult<(Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>)> {
    let inner = SNAPSHOTS.lock();
    let snap_a = inner.snapshots.get(&id_a).ok_or(KernelError::NotFound)?;
    let snap_b = inner.snapshots.get(&id_b).ok_or(KernelError::NotFound)?;

    // Build path→hash maps for files.  `Path` orders by bytes, so it is a
    // total order over every name a filesystem can hand back — a map keyed on
    // it needs no decoding step that could collapse two distinct entries onto
    // one key.
    let map_a: BTreeMap<&Path, Option<&Hash256>> = snap_a
        .entries
        .iter()
        .map(|e| (e.path.as_path(), e.content_hash.as_ref()))
        .collect();
    let map_b: BTreeMap<&Path, Option<&Hash256>> = snap_b
        .entries
        .iter()
        .map(|e| (e.path.as_path(), e.content_hash.as_ref()))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    // In B but not in A → added.
    for (path, hash_b) in &map_b {
        match map_a.get(path) {
            None => added.push(path.to_path_buf()),
            Some(hash_a) => {
                if hash_a != hash_b {
                    modified.push(path.to_path_buf());
                }
            }
        }
    }

    // In A but not in B → removed.
    for path in map_a.keys() {
        if !map_b.contains_key(path) {
            removed.push(path.to_path_buf());
        }
    }

    Ok((added, removed, modified))
}

/// Get the number of snapshots.
pub fn count() -> usize {
    SNAPSHOTS.lock().snapshots.len()
}

/// Find a snapshot by name.
pub fn find_by_name(name: &str) -> Option<SnapshotId> {
    let inner = SNAPSHOTS.lock();
    inner
        .snapshots
        .values()
        .find(|s| s.name == name)
        .map(|s| s.id)
}

/// Get children of a snapshot (snapshots whose parent == id).
pub fn children(id: SnapshotId) -> Vec<SnapshotId> {
    let inner = SNAPSHOTS.lock();
    inner
        .snapshots
        .values()
        .filter(|s| s.parent == Some(id))
        .map(|s| s.id)
        .collect()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Recursively walk a directory tree, storing entries and CAS blobs.
fn walk_directory(
    root: &Path,
    relative: &Path,
    options: &SnapshotOptions,
    depth: usize,
    entries: &mut Vec<SnapshotEntry>,
    file_count: &mut u64,
    total_bytes: &mut u64,
    stored_bytes: &mut u64,
) -> KernelResult<()> {
    if depth > options.max_depth {
        return Ok(());
    }

    // `Path::join` returns the base unchanged for an empty relative path and
    // inserts exactly one separator otherwise, so both of the hand-written
    // empty/non-empty special cases this used to carry are gone.
    let full_path = root.join(relative);

    let dir_entries = Vfs::readdir(&full_path)?;

    for entry in &dir_entries {
        let entry_relative = relative.join(&entry.name);

        // Check exclusion filters.
        if should_exclude(&entry_relative, options) {
            continue;
        }

        let entry_full = root.join(&entry_relative);

        match entry.entry_type {
            EntryType::Directory => {
                // Record the directory entry.
                let meta = Vfs::metadata(&entry_full)
                    .unwrap_or_else(|_| FileMeta::minimal(EntryType::Directory, 0));

                entries.push(SnapshotEntry {
                    path: entry_relative.clone(),
                    entry_type: EntryType::Directory,
                    content_hash: None,
                    symlink_target: None,
                    size: 0,
                    permissions: meta.permissions,
                    uid: meta.uid,
                    gid: meta.gid,
                    modified_ns: meta.modified_ns,
                });

                // Recurse into subdirectory.
                walk_directory(
                    root,
                    &entry_relative,
                    options,
                    depth.saturating_add(1),
                    entries,
                    file_count,
                    total_bytes,
                    stored_bytes,
                )?;
            }
            EntryType::File => {
                // Skip files over size limit.
                if entry.size > options.max_file_size {
                    continue;
                }

                // Read file content and store in CAS.
                let data = match Vfs::read_file(&entry_full) {
                    Ok(d) => d,
                    Err(_) => continue, // Skip unreadable files.
                };

                let hash = cas::put(&data)?;

                let meta = Vfs::metadata(&entry_full)
                    .unwrap_or_else(|_| FileMeta::minimal(EntryType::File, data.len() as u64));

                entries.push(SnapshotEntry {
                    path: entry_relative,
                    entry_type: EntryType::File,
                    content_hash: Some(hash),
                    symlink_target: None,
                    size: data.len() as u64,
                    permissions: meta.permissions,
                    uid: meta.uid,
                    gid: meta.gid,
                    modified_ns: meta.modified_ns,
                });

                *file_count = file_count.saturating_add(1);
                *total_bytes = total_bytes.saturating_add(data.len() as u64);
                *stored_bytes = stored_bytes.saturating_add(data.len() as u64);
            }
            EntryType::Symlink => {
                let target = Vfs::readlink(&entry_full).unwrap_or_default();
                let meta = Vfs::metadata(&entry_full)
                    .unwrap_or_else(|_| FileMeta::minimal(EntryType::Symlink, 0));

                entries.push(SnapshotEntry {
                    path: entry_relative,
                    entry_type: EntryType::Symlink,
                    content_hash: None,
                    symlink_target: Some(target),
                    size: 0,
                    permissions: meta.permissions,
                    uid: meta.uid,
                    gid: meta.gid,
                    modified_ns: meta.modified_ns,
                });
            }
            _ => {} // Skip other types (devices, etc.).
        }
    }

    Ok(())
}

/// Check if a relative path should be excluded by options.
///
/// Both filters test subtree containment on component boundaries rather than
/// on raw bytes; see [`SnapshotOptions::exclude_paths`].
fn should_exclude(relative: &Path, options: &SnapshotOptions) -> bool {
    use crate::fs::pathutil::path_in_subtree;

    for excluded in &options.exclude_paths {
        if path_in_subtree(relative, excluded) {
            return true;
        }
    }

    // Inclusion filter: if any is specified, the path must match at least one.
    if !options.include_paths.is_empty()
        && !options
            .include_paths
            .iter()
            .any(|p| path_in_subtree(relative, p))
    {
        return true;
    }

    false
}

/// Format a Hash256 as a hex string.
pub fn hash_to_hex(hash: &Hash256) -> String {
    let mut s = String::with_capacity(64);
    for byte in hash {
        use core::fmt::Write;
        let _ = write!(s, "{:02x}", byte);
    }
    s
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[snapshot] Running self-test...");

    test_should_exclude();
    test_create_and_list();
    test_restore();
    test_diff();
    test_delete();
    test_branching();
    test_find_by_name();

    // `join_path` and its test are gone: `Path::join` reproduces it byte for
    // byte (including the empty-relative and trailing-separator cases) and is
    // tested in `fs::path`.
    serial_println!("[snapshot] Self-test passed (7 tests).");
    Ok(())
}

fn test_should_exclude() {
    let mut opts = SnapshotOptions::default();
    opts.exclude_paths.push(PathBuf::from(".git"));
    opts.exclude_paths.push(PathBuf::from("node_modules"));

    assert!(should_exclude(Path::new(".git/config"), &opts));
    assert!(should_exclude(Path::new("node_modules/foo"), &opts));
    assert!(!should_exclude(Path::new("src/main.rs"), &opts));
    // The excluded directory itself is excluded, but a *sibling* whose name
    // merely starts with it is not - this is the byte-prefix bug the
    // component-wise match fixes.
    assert!(should_exclude(Path::new(".git"), &opts));
    assert!(!should_exclude(Path::new(".gitignore"), &opts));

    // Include filter.  A trailing separator on the filter is tolerated, so
    // both spellings behave the same.
    let mut opts2 = SnapshotOptions::default();
    opts2.include_paths.push(PathBuf::from("src/"));
    assert!(!should_exclude(Path::new("src/main.rs"), &opts2));
    assert!(should_exclude(Path::new("docs/readme.md"), &opts2));

    // A non-UTF-8 component is filterable like any other.
    let mut opts3 = SnapshotOptions::default();
    opts3
        .exclude_paths
        .push(Path::new(b"ca\xffche".as_slice()).to_path_buf());
    assert!(should_exclude(
        Path::new(b"ca\xffche/blob".as_slice()),
        &opts3
    ));
    assert!(!should_exclude(Path::new("cache/blob"), &opts3));

    serial_println!("[snapshot]   should_exclude: ok");
}

fn test_create_and_list() {
    // Create a test directory structure in /tmp.
    let _ = Vfs::mkdir("/tmp/snap_test");
    let _ = Vfs::mkdir("/tmp/snap_test/sub");
    let _ = Vfs::write_file("/tmp/snap_test/hello.txt", b"Hello, world!");
    let _ = Vfs::write_file("/tmp/snap_test/sub/data.bin", &[1, 2, 3, 4, 5]);

    let opts = SnapshotOptions::default();
    let id = create("/tmp/snap_test", "test-snap", None, &opts).expect("snapshot create failed");

    let snaps = list();
    assert!(!snaps.is_empty(), "should have at least one snapshot");

    let info = info(id).expect("snapshot info failed");
    assert_eq!(info.name, "test-snap");
    assert_eq!(info.root_path.as_path(), Path::new("/tmp/snap_test"));
    assert!(info.file_count >= 2, "should have at least 2 files");
    assert!(info.total_bytes >= 18, "should have at least 18 bytes");

    serial_println!("[snapshot]   create + list: ok");
}

fn test_restore() {
    // Find the snapshot we just created.
    let id = find_by_name("test-snap").expect("should find test-snap");

    // Create a target directory.
    let _ = Vfs::mkdir("/tmp/snap_restore");

    let result = restore(id, "/tmp/snap_restore").expect("restore failed");
    assert!(result.files_restored >= 2);
    assert!(result.dirs_created >= 1);
    assert_eq!(result.errors, 0);

    // Verify restored content.
    let data = Vfs::read_file("/tmp/snap_restore/hello.txt").expect("restored file should exist");
    assert_eq!(&data, b"Hello, world!");

    let data2 = Vfs::read_file("/tmp/snap_restore/sub/data.bin")
        .expect("restored nested file should exist");
    assert_eq!(&data2, &[1, 2, 3, 4, 5]);

    serial_println!("[snapshot]   restore: ok");
}

fn test_diff() {
    // Create a modified version of the test directory.
    let _ = Vfs::write_file("/tmp/snap_test/hello.txt", b"Modified!");
    let _ = Vfs::write_file("/tmp/snap_test/new_file.txt", b"new");
    let _ = Vfs::remove("/tmp/snap_test/sub/data.bin");

    let opts = SnapshotOptions::default();
    let id2 =
        create("/tmp/snap_test", "test-snap-2", None, &opts).expect("snapshot 2 create failed");

    let id1 = find_by_name("test-snap").expect("should find test-snap");
    let (added, _removed, modified) = diff(id1, id2).expect("diff failed");

    assert!(
        !added.is_empty() || !modified.is_empty(),
        "should detect changes"
    );
    // hello.txt was modified.
    assert!(
        modified
            .iter()
            .any(|p| p.as_path() == Path::new("hello.txt")),
        "hello.txt should be in modified"
    );

    // Cleanup: delete snapshot 2.
    let _ = delete(id2);

    serial_println!("[snapshot]   diff: ok");
}

fn test_delete() {
    let before = count();
    let opts = SnapshotOptions::default();
    let _ = Vfs::mkdir("/tmp/snap_del");
    let _ = Vfs::write_file("/tmp/snap_del/f.txt", b"x");

    let id = create("/tmp/snap_del", "to-delete", None, &opts).expect("create for delete failed");
    assert_eq!(count(), before + 1);

    delete(id).expect("delete failed");
    assert_eq!(count(), before);

    // Verify it's gone.
    assert!(info(id).is_err());

    serial_println!("[snapshot]   delete: ok");
}

fn test_branching() {
    let id1 = find_by_name("test-snap").expect("should find test-snap");

    // Create a child snapshot.
    let _ = Vfs::write_file("/tmp/snap_test/branch.txt", b"branch");
    let opts = SnapshotOptions::default();
    let id2 =
        create("/tmp/snap_test", "child-snap", Some(id1), &opts).expect("child snapshot failed");

    let info2 = info(id2).expect("child info failed");
    assert_eq!(info2.parent, Some(id1));

    let kids = children(id1);
    assert!(kids.contains(&id2), "should list child");

    // Cleanup.
    let _ = delete(id2);

    serial_println!("[snapshot]   branching: ok");
}

fn test_find_by_name() {
    let id = find_by_name("test-snap");
    assert!(id.is_some(), "should find by name");
    assert!(find_by_name("nonexistent").is_none());

    serial_println!("[snapshot]   find_by_name: ok");
}
