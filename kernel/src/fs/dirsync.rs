//! Directory comparison and synchronization.
//!
//! Compares two directory trees and generates a change manifest listing
//! files that are new, modified, deleted, or unchanged.  Optionally
//! synchronizes the destination to match the source (one-way sync).
//!
//! ## Design Reference
//!
//! design.txt line 755: "make it atomic — can undo the whole copy or
//! move or delete before it's finished"
//!
//! design.txt line 997: "backup program"
//!
//! ## Architecture
//!
//! ```text
//! dirsync::compare("/src", "/dst")
//!   → DirDiff {
//!       new_files, modified_files, deleted_files, unchanged_files,
//!       new_dirs, deleted_dirs
//!     }
//!
//! dirsync::sync("/src", "/dst", &options)
//!   → SyncResult { copied, deleted, errors }
//! ```
//!
//! Comparison uses file size + modification time for fast detection,
//! with optional content hash verification for certainty.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum recursion depth.
const MAX_DEPTH: usize = 32;

/// Maximum files to compare.
const MAX_FILES: usize = 100_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A file entry with metadata for comparison.
#[derive(Debug, Clone)]
struct FileEntry {
    /// File size.
    size: u64,
    /// Last modified timestamp (nanoseconds).
    modified_ns: u64,
    /// Entry type.
    entry_type: EntryType,
}

/// Result of comparing two directory trees.
#[derive(Debug, Clone, Default)]
pub struct DirDiff {
    /// Files that exist only in the source.
    ///
    /// Every path in a [`DirDiff`] is *relative* to the tree root and carries
    /// no leading separator, so it can be joined onto either root with
    /// [`Path::join`].  These used to keep the leading `/` left over from a
    /// byte-prefix strip, which the caller then had to concatenate without a
    /// separator - a scheme that also mis-stripped `/ab` against a root of
    /// `/a`.
    pub new_files: Vec<PathBuf>,
    /// Files that exist in both but differ (size or mtime).
    pub modified_files: Vec<PathBuf>,
    /// Files that exist only in the destination.
    pub deleted_files: Vec<PathBuf>,
    /// Files that are identical in both.
    pub unchanged_files: Vec<PathBuf>,
    /// Directories that exist only in the source.
    pub new_dirs: Vec<PathBuf>,
    /// Directories that exist only in the destination.
    pub deleted_dirs: Vec<PathBuf>,
    /// Total source files.
    pub src_file_count: u64,
    /// Total destination files.
    pub dst_file_count: u64,
    /// Total source size.
    pub src_total_size: u64,
    /// Total destination size.
    pub dst_total_size: u64,
}

/// Options for synchronization.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Delete files in destination that don't exist in source.
    pub delete_extra: bool,
    /// Only sync if content hash differs (slower but more accurate).
    pub verify_content: bool,
    /// Dry run — report what would be done without doing it.
    pub dry_run: bool,
    /// Maximum depth.
    pub max_depth: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            delete_extra: false,
            verify_content: false,
            dry_run: false,
            max_depth: MAX_DEPTH,
        }
    }
}

/// Result of a sync operation.
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    /// Files copied (new or updated).
    pub copied: u64,
    /// Files deleted from destination.
    pub deleted: u64,
    /// Directories created.
    pub dirs_created: u64,
    /// Directories deleted.
    pub dirs_deleted: u64,
    /// Bytes transferred.
    pub bytes_copied: u64,
    /// Errors encountered (non-fatal).
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static COMPARISONS: AtomicU64 = AtomicU64::new(0);
static SYNCS: AtomicU64 = AtomicU64::new(0);

/// Get counters: (comparisons, syncs).
pub fn stats() -> (u64, u64) {
    (
        COMPARISONS.load(Ordering::Relaxed),
        SYNCS.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Directory comparison
// ---------------------------------------------------------------------------

/// Compare two directory trees and report differences.
pub fn compare(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> KernelResult<DirDiff> {
    let (src, dst) = (src.as_ref(), dst.as_ref());
    let mut src_entries = BTreeMap::new();
    let mut dst_entries = BTreeMap::new();

    collect_tree(src, src, &mut src_entries, 0)?;
    collect_tree(dst, dst, &mut dst_entries, 0)?;

    let mut diff = DirDiff::default();

    // Count totals.
    for e in src_entries.values() {
        if e.entry_type == EntryType::File {
            diff.src_file_count = diff.src_file_count.saturating_add(1);
            diff.src_total_size = diff.src_total_size.saturating_add(e.size);
        }
    }
    for e in dst_entries.values() {
        if e.entry_type == EntryType::File {
            diff.dst_file_count = diff.dst_file_count.saturating_add(1);
            diff.dst_total_size = diff.dst_total_size.saturating_add(e.size);
        }
    }

    // Compare entries.
    for (rel, src_entry) in &src_entries {
        if let Some(dst_entry) = dst_entries.get(rel) {
            // Both exist — compare.
            if src_entry.entry_type == EntryType::File && dst_entry.entry_type == EntryType::File {
                if src_entry.size != dst_entry.size || src_entry.modified_ns != dst_entry.modified_ns {
                    diff.modified_files.push(rel.clone());
                } else {
                    diff.unchanged_files.push(rel.clone());
                }
            }
        } else {
            // Only in source — new.
            match src_entry.entry_type {
                EntryType::File => diff.new_files.push(rel.clone()),
                EntryType::Directory => diff.new_dirs.push(rel.clone()),
                _ => {}
            }
        }
    }

    // Find entries only in destination (deleted from source's perspective).
    for (rel, dst_entry) in &dst_entries {
        if !src_entries.contains_key(rel) {
            match dst_entry.entry_type {
                EntryType::File => diff.deleted_files.push(rel.clone()),
                EntryType::Directory => diff.deleted_dirs.push(rel.clone()),
                _ => {}
            }
        }
    }

    COMPARISONS.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[dirsync] Compare: {} new, {} modified, {} deleted, {} unchanged",
        diff.new_files.len(),
        diff.modified_files.len(),
        diff.deleted_files.len(),
        diff.unchanged_files.len(),
    );

    Ok(diff)
}

/// Synchronize source to destination (one-way).
pub fn sync(src: impl AsRef<Path>, dst: impl AsRef<Path>, opts: &SyncOptions) -> KernelResult<SyncResult> {
    let (src, dst) = (src.as_ref(), dst.as_ref());
    let diff = compare(src, dst)?;
    let mut result = SyncResult::default();

    // Create new directories.
    let mut sorted_dirs = diff.new_dirs.clone();
    sorted_dirs.sort(); // Sort to create parents before children.

    for rel in &sorted_dirs {
        let dst_path = dst.join(rel);
        if opts.dry_run {
            result.dirs_created = result.dirs_created.saturating_add(1);
        } else {
            match Vfs::mkdir(&dst_path) {
                Ok(()) => result.dirs_created = result.dirs_created.saturating_add(1),
                Err(KernelError::AlreadyExists) => {}
                Err(e) => result.errors.push(
                    alloc::format!("mkdir {}: {:?}", dst_path.display(), e)),
            }
        }
    }

    // Copy new files.
    for rel in &diff.new_files {
        let src_path = src.join(rel);
        let dst_path = dst.join(rel);

        if opts.dry_run {
            result.copied = result.copied.saturating_add(1);
        } else {
            match copy_file(&src_path, &dst_path) {
                Ok(bytes) => {
                    result.copied = result.copied.saturating_add(1);
                    result.bytes_copied = result.bytes_copied.saturating_add(bytes);
                }
                Err(e) => result.errors.push(
                    alloc::format!("copy {}: {:?}", rel.display(), e)),
            }
        }
    }

    // Copy modified files.
    for rel in &diff.modified_files {
        let src_path = src.join(rel);
        let dst_path = dst.join(rel);

        // If verify_content, check hash before copying.
        if opts.verify_content {
            let src_data = Vfs::read_file(&src_path).ok();
            let dst_data = Vfs::read_file(&dst_path).ok();
            if let (Some(s), Some(d)) = (src_data.as_ref(), dst_data.as_ref()) {
                if crate::crypto::sha256(s) == crate::crypto::sha256(d) {
                    continue; // Content is actually the same.
                }
            }
        }

        if opts.dry_run {
            result.copied = result.copied.saturating_add(1);
        } else {
            match copy_file(&src_path, &dst_path) {
                Ok(bytes) => {
                    result.copied = result.copied.saturating_add(1);
                    result.bytes_copied = result.bytes_copied.saturating_add(bytes);
                }
                Err(e) => result.errors.push(
                    alloc::format!("update {}: {:?}", rel.display(), e)),
            }
        }
    }

    // Delete extra files in destination.
    if opts.delete_extra {
        for rel in &diff.deleted_files {
            let dst_path = dst.join(rel);
            if opts.dry_run {
                result.deleted = result.deleted.saturating_add(1);
            } else {
                match Vfs::remove(&dst_path) {
                    Ok(()) => result.deleted = result.deleted.saturating_add(1),
                    Err(e) => result.errors.push(
                        alloc::format!("delete {}: {:?}", rel.display(), e)),
                }
            }
        }

        // Delete extra directories (reverse order to delete children first).
        let mut sorted_del_dirs = diff.deleted_dirs.clone();
        sorted_del_dirs.sort();
        sorted_del_dirs.reverse();

        for rel in &sorted_del_dirs {
            let dst_path = dst.join(rel);
            if opts.dry_run {
                result.dirs_deleted = result.dirs_deleted.saturating_add(1);
            } else {
                match Vfs::rmdir(&dst_path) {
                    Ok(()) => result.dirs_deleted = result.dirs_deleted.saturating_add(1),
                    Err(e) => result.errors.push(
                        alloc::format!("rmdir {}: {:?}", rel.display(), e)),
                }
            }
        }
    }

    SYNCS.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[dirsync] Sync: {} copied, {} deleted, {} dirs created, {} errors",
        result.copied, result.deleted, result.dirs_created, result.errors.len(),
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Recursively collect file entries from a directory tree.
fn collect_tree(
    root: &Path,
    path: &Path,
    out: &mut BTreeMap<PathBuf, FileEntry>,
    depth: usize,
) -> KernelResult<()> {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return Ok(());
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in &entries {
        if entry.name.as_path() == Path::new(".") || entry.name.as_path() == Path::new("..") {
            continue;
        }
        if out.len() >= MAX_FILES {
            return Ok(());
        }

        // `Path::join` collapses the root case, so `path == "/"` needs no arm.
        let full = path.join(&entry.name);

        // Relative path.  `Path::strip_prefix` matches whole components and
        // returns a path with no leading separator, so it handles a root of
        // `/` uniformly and cannot mis-strip `/a` off `/ab` the way the old
        // byte-prefix strip could.  A non-match can only mean `readdir`
        // returned something outside the tree it was asked about; keeping the
        // absolute path is the same fallback as before.
        let rel = full.as_path().strip_prefix(root).unwrap_or_else(|| full.clone());

        match entry.entry_type {
            EntryType::File => {
                if let Ok(meta) = Vfs::metadata(&full) {
                    out.insert(rel, FileEntry {
                        size: meta.size,
                        modified_ns: meta.modified_ns,
                        entry_type: EntryType::File,
                    });
                }
            }
            EntryType::Directory => {
                out.insert(rel.clone(), FileEntry {
                    size: 0,
                    modified_ns: 0,
                    entry_type: EntryType::Directory,
                });
                collect_tree(root, &full, out, depth + 1)?;
            }
            _ => {} // Skip symlinks etc.
        }
    }

    Ok(())
}

/// Copy a single file from src to dst.
fn copy_file(src: &Path, dst: &Path) -> KernelResult<u64> {
    let data = Vfs::read_file(src)?;
    let len = data.len() as u64;
    Vfs::write_file(dst, &data)?;
    Ok(len)
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[dirsync] Running self-test...");

    test_compare_identical();
    test_compare_different();
    test_sync_basic();
    test_sync_delete();
    test_sync_dry_run();
    test_stats();

    serial_println!("[dirsync] Self-test passed (6 tests).");
    Ok(())
}

fn test_compare_identical() {
    let _ = Vfs::mkdir("/tmp/ds_a");
    let _ = Vfs::mkdir("/tmp/ds_b");
    Vfs::write_file("/tmp/ds_a/f.txt", b"hello").expect("write");
    Vfs::write_file("/tmp/ds_b/f.txt", b"hello").expect("write");

    let diff = compare("/tmp/ds_a", "/tmp/ds_b").expect("compare");
    assert!(diff.new_files.is_empty(), "no new files");
    assert!(diff.deleted_files.is_empty(), "no deleted files");
    // Modified check depends on timestamps; they may differ.
    assert_eq!(diff.src_file_count, 1);
    assert_eq!(diff.dst_file_count, 1);

    let _ = Vfs::remove("/tmp/ds_a/f.txt");
    let _ = Vfs::remove("/tmp/ds_b/f.txt");
    let _ = Vfs::rmdir("/tmp/ds_a");
    let _ = Vfs::rmdir("/tmp/ds_b");

    serial_println!("[dirsync]   compare identical: ok");
}

fn test_compare_different() {
    let _ = Vfs::mkdir("/tmp/ds_c");
    let _ = Vfs::mkdir("/tmp/ds_d");
    Vfs::write_file("/tmp/ds_c/a.txt", b"alpha").expect("write");
    Vfs::write_file("/tmp/ds_c/b.txt", b"beta").expect("write");
    Vfs::write_file("/tmp/ds_d/a.txt", b"alpha modified").expect("write");
    Vfs::write_file("/tmp/ds_d/c.txt", b"charlie").expect("write");

    let diff = compare("/tmp/ds_c", "/tmp/ds_d").expect("compare");
    // b.txt is only in source → new
    assert!(diff.new_files.iter().any(|f| f.as_path() == Path::new("b.txt")), "b.txt should be new");
    // c.txt is only in destination → deleted
    assert!(diff.deleted_files.iter().any(|f| f.as_path() == Path::new("c.txt")), "c.txt should be deleted");
    // a.txt differs (different content/size) → modified
    assert!(diff.modified_files.iter().any(|f| f.as_path() == Path::new("a.txt")), "a.txt should be modified");

    let _ = Vfs::remove("/tmp/ds_c/a.txt");
    let _ = Vfs::remove("/tmp/ds_c/b.txt");
    let _ = Vfs::remove("/tmp/ds_d/a.txt");
    let _ = Vfs::remove("/tmp/ds_d/c.txt");
    let _ = Vfs::rmdir("/tmp/ds_c");
    let _ = Vfs::rmdir("/tmp/ds_d");

    serial_println!("[dirsync]   compare different: ok");
}

fn test_sync_basic() {
    let _ = Vfs::mkdir("/tmp/ds_src");
    let _ = Vfs::mkdir("/tmp/ds_dst");
    Vfs::write_file("/tmp/ds_src/x.txt", b"source data").expect("write");

    let opts = SyncOptions::default();
    let result = sync("/tmp/ds_src", "/tmp/ds_dst", &opts).expect("sync");
    assert!(result.copied >= 1, "should copy at least 1 file");

    // Verify the file was copied.
    let data = Vfs::read_file("/tmp/ds_dst/x.txt").expect("read copied");
    assert_eq!(&data, b"source data");

    let _ = Vfs::remove("/tmp/ds_src/x.txt");
    let _ = Vfs::remove("/tmp/ds_dst/x.txt");
    let _ = Vfs::rmdir("/tmp/ds_src");
    let _ = Vfs::rmdir("/tmp/ds_dst");

    serial_println!("[dirsync]   sync basic: ok");
}

fn test_sync_delete() {
    let _ = Vfs::mkdir("/tmp/ds_s2");
    let _ = Vfs::mkdir("/tmp/ds_d2");
    Vfs::write_file("/tmp/ds_s2/keep.txt", b"keep").expect("write");
    Vfs::write_file("/tmp/ds_d2/keep.txt", b"keep").expect("write");
    Vfs::write_file("/tmp/ds_d2/extra.txt", b"extra").expect("write");

    let opts = SyncOptions {
        delete_extra: true,
        ..SyncOptions::default()
    };
    let result = sync("/tmp/ds_s2", "/tmp/ds_d2", &opts).expect("sync");
    assert!(result.deleted >= 1, "should delete extra file");

    // extra.txt should be gone.
    assert!(Vfs::read_file("/tmp/ds_d2/extra.txt").is_err(), "extra.txt should be deleted");

    let _ = Vfs::remove("/tmp/ds_s2/keep.txt");
    let _ = Vfs::remove("/tmp/ds_d2/keep.txt");
    let _ = Vfs::rmdir("/tmp/ds_s2");
    let _ = Vfs::rmdir("/tmp/ds_d2");

    serial_println!("[dirsync]   sync delete: ok");
}

fn test_sync_dry_run() {
    let _ = Vfs::mkdir("/tmp/ds_dry_s");
    let _ = Vfs::mkdir("/tmp/ds_dry_d");
    Vfs::write_file("/tmp/ds_dry_s/new.txt", b"new data").expect("write");

    let opts = SyncOptions {
        dry_run: true,
        ..SyncOptions::default()
    };
    let result = sync("/tmp/ds_dry_s", "/tmp/ds_dry_d", &opts).expect("sync");
    assert!(result.copied >= 1, "should report copy in dry run");

    // File should NOT actually exist in destination.
    assert!(Vfs::read_file("/tmp/ds_dry_d/new.txt").is_err(), "dry run should not copy");

    let _ = Vfs::remove("/tmp/ds_dry_s/new.txt");
    let _ = Vfs::rmdir("/tmp/ds_dry_s");
    let _ = Vfs::rmdir("/tmp/ds_dry_d");

    serial_println!("[dirsync]   sync dry run: ok");
}

fn test_stats() {
    let (comps, syncs_count) = stats();
    assert!(comps > 0, "should have comparisons");
    assert!(syncs_count > 0, "should have syncs");

    serial_println!("[dirsync]   stats: ok");
}
