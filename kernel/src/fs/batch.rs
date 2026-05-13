//! Batch file operations.
//!
//! Provides pattern-based bulk rename, copy, move, and delete
//! with preview (dry run) support.  Integrates with the VFS
//! for actual file operations and journal for audit tracking.
//!
//! ## Design Reference
//!
//! design.txt line 755-756: directory drag-and-drop semantics,
//! command-line functions for copy/move directories, automatic
//! merge, foo(2) naming.
//!
//! ## Architecture
//!
//! ```text
//! batch::rename("/dir/*.txt", "*.bak")  → rename all .txt to .bak
//! batch::copy(["/a/1.txt", "/a/2.txt"], "/b/")  → bulk copy
//! batch::delete(["/tmp/old1", "/tmp/old2"])  → bulk delete
//! batch::move_files(["/a/x", "/a/y"], "/b/")  → bulk move
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a single batch operation on one file.
#[derive(Debug, Clone)]
pub struct BatchItem {
    /// Source path.
    pub src: String,
    /// Destination path (empty for delete operations).
    pub dst: String,
    /// Whether this item succeeded.
    pub ok: bool,
    /// Error message (empty if ok).
    pub error: String,
}

/// Summary of a batch operation.
#[derive(Debug, Clone, Default)]
pub struct BatchResult {
    /// Items processed.
    pub items: Vec<BatchItem>,
    /// Successful operations.
    pub succeeded: u64,
    /// Failed operations.
    pub failed: u64,
    /// Total bytes moved/copied.
    pub bytes: u64,
}

impl BatchResult {
    fn record_ok(&mut self, src: &str, dst: &str, bytes: u64) {
        self.items.push(BatchItem {
            src: String::from(src),
            dst: String::from(dst),
            ok: true,
            error: String::new(),
        });
        self.succeeded = self.succeeded.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
    }

    fn record_err(&mut self, src: &str, dst: &str, err: &str) {
        self.items.push(BatchItem {
            src: String::from(src),
            dst: String::from(dst),
            ok: false,
            error: String::from(err),
        });
        self.failed = self.failed.saturating_add(1);
    }
}

/// Conflict resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Skip files that already exist.
    Skip,
    /// Overwrite existing files.
    Overwrite,
    /// Rename with "(N)" suffix.
    Rename,
}

/// Options for batch operations.
#[derive(Debug, Clone)]
pub struct BatchOptions {
    /// How to handle destination conflicts.
    pub on_conflict: ConflictStrategy,
    /// Dry run — report without executing.
    pub dry_run: bool,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            on_conflict: ConflictStrategy::Skip,
            dry_run: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static RENAMES: AtomicU64 = AtomicU64::new(0);
static COPIES: AtomicU64 = AtomicU64::new(0);
static MOVES: AtomicU64 = AtomicU64::new(0);
static DELETES: AtomicU64 = AtomicU64::new(0);

/// Get counters: (renames, copies, moves, deletes).
pub fn stats() -> (u64, u64, u64, u64) {
    (
        RENAMES.load(Ordering::Relaxed),
        COPIES.load(Ordering::Relaxed),
        MOVES.load(Ordering::Relaxed),
        DELETES.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Batch rename
// ---------------------------------------------------------------------------

/// Rename files in a directory matching a glob pattern.
///
/// `pattern` is a simple glob (e.g., "*.txt") matched against filenames
/// in `dir`.  `replacement` is an extension or pattern to replace with
/// (e.g., "*.bak" replaces the extension).
///
/// Supports:
/// - Extension replacement: `rename("/dir", "*.txt", "*.bak")`
/// - Prefix replacement: `rename("/dir", "old_*", "new_*")`
pub fn rename(dir: &str, pattern: &str, replacement: &str, opts: &BatchOptions) -> KernelResult<BatchResult> {
    let entries = Vfs::readdir(dir)?;
    let mut result = BatchResult::default();

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if entry.entry_type != EntryType::File {
            continue;
        }

        if let Some(new_name) = apply_rename_pattern(&entry.name, pattern, replacement) {
            let src = alloc::format!("{}/{}", dir, entry.name);
            let dst = alloc::format!("{}/{}", dir, new_name);

            if !opts.dry_run {
                // Check for conflicts.
                if Vfs::metadata(&dst).is_ok() {
                    match opts.on_conflict {
                        ConflictStrategy::Skip => {
                            result.record_err(&src, &dst, "destination exists (skipped)");
                            continue;
                        }
                        ConflictStrategy::Overwrite => {
                            let _ = Vfs::remove(&dst);
                        }
                        ConflictStrategy::Rename => {
                            let alt = find_unique_name(&dst);
                            match Vfs::rename(&src, &alt) {
                                Ok(()) => result.record_ok(&src, &alt, 0),
                                Err(e) => result.record_err(&src, &alt, &alloc::format!("{:?}", e)),
                            }
                            continue;
                        }
                    }
                }
                match Vfs::rename(&src, &dst) {
                    Ok(()) => result.record_ok(&src, &dst, 0),
                    Err(e) => result.record_err(&src, &dst, &alloc::format!("{:?}", e)),
                }
            } else {
                result.record_ok(&src, &dst, 0);
            }
        }
    }

    RENAMES.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[batch] Rename in {}: {} succeeded, {} failed",
        dir, result.succeeded, result.failed,
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Batch copy
// ---------------------------------------------------------------------------

/// Copy multiple files to a destination directory.
pub fn copy(paths: &[&str], dest_dir: &str, opts: &BatchOptions) -> KernelResult<BatchResult> {
    let mut result = BatchResult::default();

    if !opts.dry_run {
        let _ = Vfs::mkdir(dest_dir);
    }

    for src in paths {
        let filename = src.rsplit('/').next().unwrap_or(src);
        let dst = alloc::format!("{}/{}", dest_dir, filename);

        // Handle conflicts.
        let final_dst = if Vfs::metadata(&dst).is_ok() {
            match opts.on_conflict {
                ConflictStrategy::Skip => {
                    result.record_err(src, &dst, "destination exists (skipped)");
                    continue;
                }
                ConflictStrategy::Overwrite => dst,
                ConflictStrategy::Rename => find_unique_name(&dst),
            }
        } else {
            dst
        };

        if opts.dry_run {
            if let Ok(meta) = Vfs::metadata(src) {
                result.record_ok(src, &final_dst, meta.size);
            } else {
                result.record_ok(src, &final_dst, 0);
            }
        } else {
            match Vfs::copy(src, &final_dst) {
                Ok(bytes) => result.record_ok(src, &final_dst, bytes),
                Err(e) => result.record_err(src, &final_dst, &alloc::format!("{:?}", e)),
            }
        }
    }

    COPIES.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[batch] Copy {} files to {}: {} ok, {} failed",
        paths.len(), dest_dir, result.succeeded, result.failed,
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Batch move
// ---------------------------------------------------------------------------

/// Move multiple files to a destination directory.
pub fn move_files(paths: &[&str], dest_dir: &str, opts: &BatchOptions) -> KernelResult<BatchResult> {
    let mut result = BatchResult::default();

    if !opts.dry_run {
        let _ = Vfs::mkdir(dest_dir);
    }

    for src in paths {
        let filename = src.rsplit('/').next().unwrap_or(src);
        let dst = alloc::format!("{}/{}", dest_dir, filename);

        let final_dst = if Vfs::metadata(&dst).is_ok() {
            match opts.on_conflict {
                ConflictStrategy::Skip => {
                    result.record_err(src, &dst, "destination exists (skipped)");
                    continue;
                }
                ConflictStrategy::Overwrite => {
                    if !opts.dry_run {
                        let _ = Vfs::remove(&dst);
                    }
                    dst
                }
                ConflictStrategy::Rename => find_unique_name(&dst),
            }
        } else {
            dst
        };

        if opts.dry_run {
            if let Ok(meta) = Vfs::metadata(src) {
                result.record_ok(src, &final_dst, meta.size);
            } else {
                result.record_ok(src, &final_dst, 0);
            }
        } else {
            match Vfs::rename(src, &final_dst) {
                Ok(()) => {
                    let bytes = Vfs::metadata(&final_dst).map_or(0, |m| m.size);
                    result.record_ok(src, &final_dst, bytes);
                }
                Err(e) => result.record_err(src, &final_dst, &alloc::format!("{:?}", e)),
            }
        }
    }

    MOVES.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[batch] Move {} files to {}: {} ok, {} failed",
        paths.len(), dest_dir, result.succeeded, result.failed,
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Batch delete
// ---------------------------------------------------------------------------

/// Delete multiple files.
pub fn delete(paths: &[&str], opts: &BatchOptions) -> KernelResult<BatchResult> {
    let mut result = BatchResult::default();

    for path in paths {
        if opts.dry_run {
            result.record_ok(path, "", 0);
        } else {
            match Vfs::remove(path) {
                Ok(()) => result.record_ok(path, "", 0),
                Err(e) => result.record_err(path, "", &alloc::format!("{:?}", e)),
            }
        }
    }

    DELETES.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[batch] Delete {} files: {} ok, {} failed",
        paths.len(), result.succeeded, result.failed,
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Glob-based file collection
// ---------------------------------------------------------------------------

/// Collect files in a directory matching a simple glob pattern.
///
/// Supports `*` wildcard matching and `?` single-character matching.
pub fn glob_files(dir: &str, pattern: &str) -> KernelResult<Vec<String>> {
    let entries = Vfs::readdir(dir)?;
    let mut matched = Vec::new();

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if glob_match(pattern, &entry.name) {
            let path = alloc::format!("{}/{}", dir, entry.name);
            matched.push(path);
        }
    }

    Ok(matched)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply a rename pattern to transform a filename.
///
/// Patterns use `*` as wildcard:
/// - `*.txt` → `*.bak`: replaces extension
/// - `old_*` → `new_*`: replaces prefix
fn apply_rename_pattern(name: &str, pattern: &str, replacement: &str) -> Option<String> {
    // First check if the name matches the pattern.
    if !glob_match(pattern, name) {
        return None;
    }

    // Extension replacement: *.ext1 → *.ext2
    if pattern.starts_with("*.") && replacement.starts_with("*.") {
        let old_ext = &pattern[1..]; // ".ext1"
        let new_ext = &replacement[1..]; // ".ext2"
        if name.ends_with(old_ext) {
            let base = &name[..name.len() - old_ext.len()];
            return Some(alloc::format!("{}{}", base, new_ext));
        }
    }

    // Prefix replacement: old_* → new_*
    if pattern.ends_with('*') && replacement.ends_with('*') {
        let old_prefix = &pattern[..pattern.len() - 1];
        let new_prefix = &replacement[..replacement.len() - 1];
        if name.starts_with(old_prefix) {
            let suffix = &name[old_prefix.len()..];
            return Some(alloc::format!("{}{}", new_prefix, suffix));
        }
    }

    // Exact replacement (no wildcards).
    if !pattern.contains('*') && !replacement.contains('*') {
        if name == pattern {
            return Some(String::from(replacement));
        }
    }

    None
}

/// Simple glob matching: `*` matches any sequence, `?` matches one char.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    glob_match_inner(p, t, 0, 0)
}

fn glob_match_inner(p: &[u8], t: &[u8], pi: usize, ti: usize) -> bool {
    if pi >= p.len() && ti >= t.len() {
        return true;
    }
    if pi >= p.len() {
        return false;
    }

    if p[pi] == b'*' {
        // Try matching zero or more characters.
        let mut ti2 = ti;
        while ti2 <= t.len() {
            if glob_match_inner(p, t, pi + 1, ti2) {
                return true;
            }
            ti2 += 1;
        }
        return false;
    }

    if ti >= t.len() {
        return false;
    }

    if p[pi] == b'?' || p[pi] == t[ti] {
        return glob_match_inner(p, t, pi + 1, ti + 1);
    }

    false
}

/// Generate a unique filename by appending " (N)" before the extension.
fn find_unique_name(path: &str) -> String {
    // Split into base and extension.
    let (dir, name) = if let Some(pos) = path.rfind('/') {
        (&path[..pos], &path[pos + 1..])
    } else {
        ("", path)
    };

    let (base, ext) = if let Some(dot) = name.rfind('.') {
        (&name[..dot], &name[dot..])
    } else {
        (name, "")
    };

    for n in 2u32..100 {
        let candidate = if dir.is_empty() {
            alloc::format!("{} ({}){}", base, n, ext)
        } else {
            alloc::format!("{}/{} ({}){}", dir, base, n, ext)
        };
        if Vfs::metadata(&candidate).is_err() {
            return candidate;
        }
    }

    // Fallback: use timestamp.
    let ts = crate::timekeeping::clock_monotonic();
    if dir.is_empty() {
        alloc::format!("{}_{}{}", base, ts, ext)
    } else {
        alloc::format!("{}/{}_{}{}", dir, base, ts, ext)
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[batch] Running self-test...");

    test_glob_match();
    test_rename_pattern();
    test_batch_rename();
    test_batch_copy();
    test_batch_move();
    test_batch_delete();
    test_glob_files();
    test_unique_name();
    test_stats();

    serial_println!("[batch] Self-test passed (9 tests).");
    Ok(())
}

fn test_glob_match() {
    assert!(glob_match("*.txt", "hello.txt"));
    assert!(glob_match("*.txt", ".txt"));
    assert!(!glob_match("*.txt", "hello.bak"));
    assert!(glob_match("test*", "test123"));
    assert!(glob_match("?est", "test"));
    assert!(!glob_match("?est", "best2"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("a*b", "aXYZb"));
    serial_println!("[batch]   glob match: ok");
}

fn test_rename_pattern() {
    assert_eq!(
        apply_rename_pattern("doc.txt", "*.txt", "*.bak"),
        Some(String::from("doc.bak"))
    );
    assert_eq!(
        apply_rename_pattern("old_data.csv", "old_*", "new_*"),
        Some(String::from("new_data.csv"))
    );
    assert_eq!(apply_rename_pattern("doc.bak", "*.txt", "*.bak"), None);
    serial_println!("[batch]   rename pattern: ok");
}

fn test_batch_rename() {
    let _ = Vfs::mkdir("/tmp/batch_ren");
    Vfs::write_file("/tmp/batch_ren/a.txt", b"a").expect("write");
    Vfs::write_file("/tmp/batch_ren/b.txt", b"b").expect("write");
    Vfs::write_file("/tmp/batch_ren/c.log", b"c").expect("write");

    let opts = BatchOptions::default();
    let result = rename("/tmp/batch_ren", "*.txt", "*.bak", &opts).expect("rename");
    assert_eq!(result.succeeded, 2, "should rename 2 .txt files");

    // .txt files should be gone, .bak files should exist.
    assert!(Vfs::metadata("/tmp/batch_ren/a.bak").is_ok(), "a.bak should exist");
    assert!(Vfs::metadata("/tmp/batch_ren/b.bak").is_ok(), "b.bak should exist");
    assert!(Vfs::metadata("/tmp/batch_ren/c.log").is_ok(), "c.log untouched");

    let _ = Vfs::remove("/tmp/batch_ren/a.bak");
    let _ = Vfs::remove("/tmp/batch_ren/b.bak");
    let _ = Vfs::remove("/tmp/batch_ren/c.log");
    let _ = Vfs::rmdir("/tmp/batch_ren");

    serial_println!("[batch]   batch rename: ok");
}

fn test_batch_copy() {
    let _ = Vfs::mkdir("/tmp/batch_cps");
    let _ = Vfs::mkdir("/tmp/batch_cpd");
    Vfs::write_file("/tmp/batch_cps/x.txt", b"x data").expect("write");
    Vfs::write_file("/tmp/batch_cps/y.txt", b"y data").expect("write");

    let paths = ["/tmp/batch_cps/x.txt", "/tmp/batch_cps/y.txt"];
    let opts = BatchOptions::default();
    let result = copy(&paths, "/tmp/batch_cpd", &opts).expect("copy");
    assert_eq!(result.succeeded, 2);

    let data = Vfs::read_file("/tmp/batch_cpd/x.txt").expect("read");
    assert_eq!(&data, b"x data");

    let _ = Vfs::remove("/tmp/batch_cps/x.txt");
    let _ = Vfs::remove("/tmp/batch_cps/y.txt");
    let _ = Vfs::remove("/tmp/batch_cpd/x.txt");
    let _ = Vfs::remove("/tmp/batch_cpd/y.txt");
    let _ = Vfs::rmdir("/tmp/batch_cps");
    let _ = Vfs::rmdir("/tmp/batch_cpd");

    serial_println!("[batch]   batch copy: ok");
}

fn test_batch_move() {
    let _ = Vfs::mkdir("/tmp/batch_mvs");
    let _ = Vfs::mkdir("/tmp/batch_mvd");
    Vfs::write_file("/tmp/batch_mvs/m.txt", b"move me").expect("write");

    let paths = ["/tmp/batch_mvs/m.txt"];
    let opts = BatchOptions::default();
    let result = move_files(&paths, "/tmp/batch_mvd", &opts).expect("move");
    assert_eq!(result.succeeded, 1);

    assert!(Vfs::metadata("/tmp/batch_mvs/m.txt").is_err(), "source should be gone");
    let data = Vfs::read_file("/tmp/batch_mvd/m.txt").expect("read");
    assert_eq!(&data, b"move me");

    let _ = Vfs::remove("/tmp/batch_mvd/m.txt");
    let _ = Vfs::rmdir("/tmp/batch_mvs");
    let _ = Vfs::rmdir("/tmp/batch_mvd");

    serial_println!("[batch]   batch move: ok");
}

fn test_batch_delete() {
    Vfs::write_file("/tmp/batch_del1.txt", b"del1").expect("write");
    Vfs::write_file("/tmp/batch_del2.txt", b"del2").expect("write");

    let paths = ["/tmp/batch_del1.txt", "/tmp/batch_del2.txt"];
    let opts = BatchOptions::default();
    let result = delete(&paths, &opts).expect("delete");
    assert_eq!(result.succeeded, 2);

    assert!(Vfs::metadata("/tmp/batch_del1.txt").is_err());
    assert!(Vfs::metadata("/tmp/batch_del2.txt").is_err());

    serial_println!("[batch]   batch delete: ok");
}

fn test_glob_files() {
    let _ = Vfs::mkdir("/tmp/batch_glob");
    Vfs::write_file("/tmp/batch_glob/a.txt", b"a").expect("write");
    Vfs::write_file("/tmp/batch_glob/b.txt", b"b").expect("write");
    Vfs::write_file("/tmp/batch_glob/c.log", b"c").expect("write");

    let matched = glob_files("/tmp/batch_glob", "*.txt").expect("glob");
    assert_eq!(matched.len(), 2);

    let _ = Vfs::remove("/tmp/batch_glob/a.txt");
    let _ = Vfs::remove("/tmp/batch_glob/b.txt");
    let _ = Vfs::remove("/tmp/batch_glob/c.log");
    let _ = Vfs::rmdir("/tmp/batch_glob");

    serial_println!("[batch]   glob files: ok");
}

fn test_unique_name() {
    // Without a file existing, find_unique_name should still work.
    let name = find_unique_name("/tmp/nonexistent.txt");
    assert!(name.contains("(2)") || name.contains("_"), "should generate unique name");

    serial_println!("[batch]   unique name: ok");
}

fn test_stats() {
    let (renames, copies, moves, deletes) = stats();
    assert!(renames > 0 || copies > 0 || moves > 0 || deletes > 0, "should have operations");

    serial_println!("[batch]   stats: ok");
}
