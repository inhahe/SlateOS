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
use crate::fs::path::{Path, PathBuf};
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a single batch operation on one file.
#[derive(Debug, Clone)]
pub struct BatchItem {
    /// Source path.
    pub src: PathBuf,
    /// Destination path (empty for delete operations).
    pub dst: PathBuf,
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
    fn record_ok(&mut self, src: &Path, dst: &Path, bytes: u64) {
        self.items.push(BatchItem {
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
            ok: true,
            error: String::new(),
        });
        self.succeeded = self.succeeded.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
    }

    fn record_err(&mut self, src: &Path, dst: &Path, err: &str) {
        self.items.push(BatchItem {
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
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
/// `pattern` is a glob (e.g., `*.txt`) matched against filenames in `dir`.
/// `replacement` is an extension or pattern to replace with (e.g., `*.bak`
/// replaces the extension).
///
/// Supports:
/// - Extension replacement: `rename("/dir", "*.txt", "*.bak")`
/// - Prefix replacement: `rename("/dir", "old_*", "new_*")`
///
/// The pattern and replacement are byte strings, not text: they are matched
/// against filenames, and a filename is a byte string that need not be valid
/// UTF-8.  Accepting `impl AsRef<[u8]>` keeps `&str` literals working while
/// letting a caller pass a pattern that came off the wire or out of a
/// directory listing.
pub fn rename<D: AsRef<Path> + ?Sized, G: AsRef<[u8]> + ?Sized, R: AsRef<[u8]> + ?Sized>(
    dir: &D,
    pattern: &G,
    replacement: &R,
    opts: &BatchOptions,
) -> KernelResult<BatchResult> {
    let dir = dir.as_ref();
    let (pattern, replacement) = (pattern.as_ref(), replacement.as_ref());
    let entries = Vfs::readdir(dir)?;
    let mut result = BatchResult::default();

    for entry in &entries {
        if entry.name.as_path() == Path::new(".") || entry.name.as_path() == Path::new("..") {
            continue;
        }
        if entry.entry_type != EntryType::File {
            continue;
        }

        if let Some(new_name) = apply_rename_pattern(&entry.name, pattern, replacement) {
            let src = dir.join(&entry.name);
            let dst = dir.join(&new_name);

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
        dir.display(),
        result.succeeded,
        result.failed,
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Batch copy
// ---------------------------------------------------------------------------

/// Copy multiple files to a destination directory.
pub fn copy<P: AsRef<Path>, D: AsRef<Path> + ?Sized>(
    paths: &[P],
    dest_dir: &D,
    opts: &BatchOptions,
) -> KernelResult<BatchResult> {
    let dest_dir = dest_dir.as_ref();
    let mut result = BatchResult::default();

    if !opts.dry_run {
        let _ = Vfs::mkdir(dest_dir);
    }

    for src in paths {
        let src = src.as_ref();
        // A source with no final component (`/`, or the empty path) names no
        // file to copy; skipping it is the only reading that does not invent
        // a destination name.
        let Some(filename) = src.file_name() else {
            result.record_err(src, Path::new(""), "source has no filename");
            continue;
        };
        let dst = dest_dir.join(filename);

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
        paths.len(),
        dest_dir.display(),
        result.succeeded,
        result.failed,
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Batch move
// ---------------------------------------------------------------------------

/// Move multiple files to a destination directory.
pub fn move_files<P: AsRef<Path>, D: AsRef<Path> + ?Sized>(
    paths: &[P],
    dest_dir: &D,
    opts: &BatchOptions,
) -> KernelResult<BatchResult> {
    let dest_dir = dest_dir.as_ref();
    let mut result = BatchResult::default();

    if !opts.dry_run {
        let _ = Vfs::mkdir(dest_dir);
    }

    for src in paths {
        let src = src.as_ref();
        let Some(filename) = src.file_name() else {
            result.record_err(src, Path::new(""), "source has no filename");
            continue;
        };
        let dst = dest_dir.join(filename);

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
        paths.len(),
        dest_dir.display(),
        result.succeeded,
        result.failed,
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Batch delete
// ---------------------------------------------------------------------------

/// Delete multiple files.
pub fn delete<P: AsRef<Path>>(paths: &[P], opts: &BatchOptions) -> KernelResult<BatchResult> {
    let mut result = BatchResult::default();

    // A delete has no destination; the empty path records that absence
    // without inventing a name.
    let none = Path::new("");
    for path in paths {
        let path = path.as_ref();
        if opts.dry_run {
            result.record_ok(path, none, 0);
        } else {
            match Vfs::remove(path) {
                Ok(()) => result.record_ok(path, none, 0),
                Err(e) => result.record_err(path, none, &alloc::format!("{:?}", e)),
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

/// Collect files in a directory matching a glob pattern.
///
/// See [`crate::fs::vfs::glob_match`] for the supported syntax.  Matching is
/// case-sensitive because the filesystem is: two names differing only in case
/// are two different files, and a glob must say so.
pub fn glob_files<D: AsRef<Path> + ?Sized, G: AsRef<[u8]> + ?Sized>(
    dir: &D,
    pattern: &G,
) -> KernelResult<Vec<PathBuf>> {
    let dir = dir.as_ref();
    let pattern = pattern.as_ref();
    let entries = Vfs::readdir(dir)?;
    let mut matched = Vec::new();

    for entry in &entries {
        if entry.name.as_path() == Path::new(".") || entry.name.as_path() == Path::new("..") {
            continue;
        }
        if crate::fs::vfs::glob_match(entry.name.as_bytes(), pattern, false) {
            matched.push(dir.join(&entry.name));
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
///
/// Operates on bytes throughout: the name comes from a directory listing and
/// need not be valid UTF-8, and the byte slicing here is index-free so a
/// multi-byte sequence cannot be split.
fn apply_rename_pattern(name: &Path, pattern: &[u8], replacement: &[u8]) -> Option<PathBuf> {
    // First check if the name matches the pattern.
    if !crate::fs::vfs::glob_match(name.as_bytes(), pattern, false) {
        return None;
    }
    let name = name.as_bytes();

    // Extension replacement: *.ext1 → *.ext2.  Both `*.` prefixes are dropped
    // and the leading `.` kept, so the suffix compared is `.ext1`.
    if let (Some(old_ext), Some(new_ext)) = (
        pattern.strip_prefix(b"*"),
        replacement.strip_prefix(b"*"),
    ) {
        if old_ext.starts_with(b".") && new_ext.starts_with(b".") {
            if let Some(base) = name.strip_suffix(old_ext) {
                let mut out = PathBuf::from(base);
                out.extend_bytes(new_ext);
                return Some(out);
            }
        }
    }

    // Prefix replacement: old_* → new_*.
    if let (Some(old_prefix), Some(new_prefix)) = (
        pattern.strip_suffix(b"*"),
        replacement.strip_suffix(b"*"),
    ) {
        if let Some(suffix) = name.strip_prefix(old_prefix) {
            let mut out = PathBuf::from(new_prefix);
            out.extend_bytes(suffix);
            return Some(out);
        }
    }

    // Exact replacement (no wildcards).
    if !pattern.contains(&b'*') && !replacement.contains(&b'*') && name == pattern {
        return Some(PathBuf::from(replacement));
    }

    None
}

/// Generate a unique filename by appending " (N)" before the extension.
///
/// The split uses [`Path::file_name`]/[`Path::extension`] rather than a
/// `rfind('.')`, so a dotfile keeps its name intact: `.bashrc` becomes
/// `.bashrc (2)`, not ` (2).bashrc`.
fn find_unique_name(path: &Path) -> PathBuf {
    let dir = path.parent();
    // A path with no final component names no file to disambiguate; treating
    // the whole path as the name is the closest thing to a useful answer and
    // matches what the caller passed in.
    let name = path.file_name().unwrap_or(path).as_bytes();
    // `extension()` measures from the same final component, so this offset is
    // in range by construction; `len - (ext + 1)` accounts for the dot.
    let stem_len = path.extension().map_or(name.len(), |e| {
        name.len().saturating_sub(e.as_bytes().len().saturating_add(1))
    });
    let (stem, ext) = name.split_at(stem_len.min(name.len()));

    let build = |suffix: &str| -> PathBuf {
        let mut leaf = PathBuf::from(stem);
        leaf.extend_bytes(suffix.as_bytes());
        leaf.extend_bytes(ext);
        match dir {
            Some(d) => d.join(&leaf),
            None => leaf,
        }
    };

    for n in 2u32..100 {
        let candidate = build(&alloc::format!(" ({})", n));
        if Vfs::metadata(&candidate).is_err() {
            return candidate;
        }
    }

    // Fallback: use timestamp.
    let ts = crate::timekeeping::clock_monotonic();
    build(&alloc::format!("_{}", ts))
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

/// The glob syntax itself lives in `vfs::glob_match` and is tested there;
/// what matters here is that batch's own callers reach it with the arguments
/// in the right order and case-sensitively.
fn test_glob_match() {
    let m = |pat: &str, name: &str| crate::fs::vfs::glob_match(name, pat, false);
    assert!(m("*.txt", "hello.txt"));
    assert!(m("*.txt", ".txt"));
    assert!(!m("*.txt", "hello.bak"));
    assert!(m("test*", "test123"));
    assert!(m("?est", "test"));
    assert!(!m("?est", "best2"));
    assert!(m("*", "anything"));
    assert!(m("a*b", "aXYZb"));
    // Case-sensitive filesystem: `A.TXT` and `a.txt` are two different files.
    assert!(!m("*.txt", "HELLO.TXT"));
    // A pattern is bytes, so it matches a name that is not text.
    assert!(crate::fs::vfs::glob_match(b"re\xffport.txt".as_slice(), "*.txt", false));
    serial_println!("[batch]   glob match: ok");
}

fn test_rename_pattern() {
    let ap = |name: &[u8], pat: &str, rep: &str| {
        apply_rename_pattern(Path::new(name), pat.as_bytes(), rep.as_bytes())
    };
    assert_eq!(ap(b"doc.txt", "*.txt", "*.bak"), Some(PathBuf::from("doc.bak")));
    assert_eq!(
        ap(b"old_data.csv", "old_*", "new_*"),
        Some(PathBuf::from("new_data.csv"))
    );
    assert_eq!(ap(b"doc.bak", "*.txt", "*.bak"), None);

    // A name that is not UTF-8 renames like any other: the transform is
    // byte-wise and never has to decode the part it keeps.
    assert_eq!(
        ap(b"re\xffport.txt", "*.txt", "*.bak"),
        Some(PathBuf::from(b"re\xffport.bak".as_slice()))
    );
    // ...including when the undecodable byte is in the pattern.
    assert_eq!(
        apply_rename_pattern(
            Path::new(b"re\xffport.txt".as_slice()),
            b"re\xff*",
            b"ok_*",
        ),
        Some(PathBuf::from("ok_port.txt"))
    );

    // Exact rename with no wildcard on either side.
    assert_eq!(ap(b"a.txt", "a.txt", "b.txt"), Some(PathBuf::from("b.txt")));
    assert_eq!(ap(b"c.txt", "a.txt", "b.txt"), None);
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
    // Nothing exists at the target, so the very first candidate is free.
    assert_eq!(
        find_unique_name(Path::new("/tmp/nonexistent.txt")),
        PathBuf::from("/tmp/nonexistent (2).txt")
    );

    // A dotfile has no extension, so the suffix goes at the end rather than
    // splitting the name at its leading dot.  The `rfind('.')` this replaced
    // produced `/tmp/ (2).bashrc`.
    assert_eq!(
        find_unique_name(Path::new("/tmp/.bashrc")),
        PathBuf::from("/tmp/.bashrc (2)")
    );

    // A name with no extension at all, and a relative one with no directory.
    assert_eq!(
        find_unique_name(Path::new("/tmp/README")),
        PathBuf::from("/tmp/README (2)")
    );
    assert_eq!(
        find_unique_name(Path::new("plain.txt")),
        PathBuf::from("plain (2).txt")
    );

    // A name that is not UTF-8 keeps its bytes, and the split still lands on
    // the extension boundary rather than inside the undecodable byte.
    assert_eq!(
        find_unique_name(Path::new(b"/tmp/re\xffport.txt".as_slice())),
        PathBuf::from(b"/tmp/re\xffport (2).txt".as_slice())
    );

    // An occupied candidate is skipped over rather than returned.
    Vfs::write_file("/tmp/batch_uniq.txt", b"x").expect("write");
    Vfs::write_file("/tmp/batch_uniq (2).txt", b"x").expect("write");
    assert_eq!(
        find_unique_name(Path::new("/tmp/batch_uniq.txt")),
        PathBuf::from("/tmp/batch_uniq (3).txt")
    );
    let _ = Vfs::remove("/tmp/batch_uniq.txt");
    let _ = Vfs::remove("/tmp/batch_uniq (2).txt");

    serial_println!("[batch]   unique name: ok");
}

fn test_stats() {
    let (renames, copies, moves, deletes) = stats();
    assert!(renames > 0 || copies > 0 || moves > 0 || deletes > 0, "should have operations");

    serial_println!("[batch]   stats: ok");
}
