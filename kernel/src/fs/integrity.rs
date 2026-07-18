//! File integrity monitoring and verification.
//!
//! Provides baseline-and-verify integrity checking for filesystem paths.
//! Stores SHA-256 hashes of file contents and can later verify that files
//! have not been modified, corrupted, or tampered with.
//!
//! ## Design
//!
//! - **Baseline**: Walk a directory tree (or individual files), compute
//!   SHA-256 for each, and store the (path → hash, size, mtime) mapping.
//! - **Verify**: Re-walk the same paths, recompute hashes, compare against
//!   the baseline.  Report: OK / MODIFIED / MISSING / NEW / SIZE_CHANGED.
//! - **In-memory store**: The baseline lives in a `BTreeMap` behind a
//!   spinlock.  Persistence (writing to a file) is a future enhancement.
//! - **Bounded**: configurable max entries to prevent OOM.
//!
//! ## Use cases
//!
//! - Detect unauthorized modifications to system files (rootkit detection).
//! - Verify package installation integrity.
//! - Detect bit-rot or silent data corruption.
//! - Audit trail: know exactly which files changed between two points in time.
//!
//! ## Reference
//!
//! design.txt: "Hash: compute on write, store in metadata, verify on read.
//! Per-block hashing is better (detects which part is corrupt, enables dedup)."
//! This module provides per-file hashing as the first layer; per-block
//! hashing can be added later on top of the CAS.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::cas::Hash256;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A baseline entry for a single file.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BaselineEntry {
    /// Absolute path to the file.
    pub path: String,
    /// SHA-256 hash of the file contents at baseline time.
    pub hash: Hash256,
    /// File size in bytes at baseline time.
    pub size: u64,
    /// Modification timestamp (nanoseconds since boot) at baseline time.
    pub mtime_ns: u64,
}

/// Result of verifying a single file against its baseline.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VerifyResult {
    /// The file path.
    pub path: String,
    /// Verification status.
    pub status: VerifyStatus,
    /// Baseline hash (if the file was in the baseline).
    pub baseline_hash: Option<Hash256>,
    /// Current hash (if the file currently exists).
    pub current_hash: Option<Hash256>,
    /// Baseline size.
    pub baseline_size: Option<u64>,
    /// Current size.
    pub current_size: Option<u64>,
}

/// Status of a file verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VerifyStatus {
    /// File matches its baseline hash exactly.
    Ok,
    /// File exists but its content hash differs from the baseline.
    Modified,
    /// File was in the baseline but no longer exists.
    Missing,
    /// File exists on disk but was not in the baseline.
    New,
    /// File could not be read (permissions, I/O error, etc.).
    Error,
}

/// Summary statistics from a verification run.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct VerifySummary {
    /// Total files checked.
    pub total: u64,
    /// Files that matched their baseline.
    pub ok: u64,
    /// Files with modified content.
    pub modified: u64,
    /// Files missing from disk.
    pub missing: u64,
    /// New files not in baseline.
    pub new: u64,
    /// Files that could not be read.
    pub errors: u64,
}

impl VerifySummary {
    fn new() -> Self {
        Self { total: 0, ok: 0, modified: 0, missing: 0, new: 0, errors: 0 }
    }
}

/// Configuration for integrity operations.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IntegrityConfig {
    /// Maximum number of entries in the baseline (prevent OOM).
    pub max_entries: usize,
    /// Maximum file size to hash (skip very large files).
    pub max_file_size: u64,
    /// Directories to exclude from baseline/verify walks.
    pub exclude_dirs: Vec<String>,
}

impl Default for IntegrityConfig {
    fn default() -> Self {
        Self {
            max_entries: 50_000,
            max_file_size: 64 * 1024 * 1024, // 64 MiB
            exclude_dirs: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct IntegrityInner {
    /// The baseline: path → entry.
    baseline: BTreeMap<String, BaselineEntry>,
    /// Configuration.
    config: IntegrityConfig,
    /// When the baseline was last updated (HPET nanoseconds).
    baseline_timestamp: u64,
    /// Total number of baseline operations performed.
    baseline_count: u64,
    /// Total number of verify operations performed.
    verify_count: u64,
}

static INTEGRITY: Mutex<IntegrityInner> = Mutex::new(IntegrityInner {
    baseline: BTreeMap::new(),
    config: IntegrityConfig {
        max_entries: 50_000,
        max_file_size: 64 * 1024 * 1024,
        exclude_dirs: Vec::new(),
    },
    baseline_timestamp: 0,
    baseline_count: 0,
    verify_count: 0,
});

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the integrity monitoring configuration.
#[allow(dead_code)]
pub fn set_config(config: IntegrityConfig) {
    INTEGRITY.lock().config = config;
}

/// Get a clone of the current configuration.
#[allow(dead_code)]
pub fn get_config() -> IntegrityConfig {
    INTEGRITY.lock().config.clone()
}

// ---------------------------------------------------------------------------
// Baseline operations
// ---------------------------------------------------------------------------

/// Add a single file to the baseline.
///
/// Reads the file, computes its SHA-256 hash, and stores the entry.
/// Returns the hash on success.
///
/// Returns `NotFound` if the file doesn't exist, or `DiskFull` if the
/// baseline is at capacity.
pub fn baseline_file(path: &str) -> KernelResult<Hash256> {
    use crate::fs::Vfs;

    let meta = Vfs::stat(path)?;

    // Only baseline regular files.
    if meta.entry_type != crate::fs::EntryType::File {
        return Err(KernelError::InvalidArgument);
    }

    let inner = INTEGRITY.lock();
    let max_size = inner.config.max_file_size;
    let max_entries = inner.config.max_entries;
    let current_count = inner.baseline.len();
    drop(inner);

    // Size check.
    if meta.size > max_size {
        return Err(KernelError::MessageTooLarge); // Reusing for "too large"
    }

    // Capacity check.
    if current_count >= max_entries {
        return Err(KernelError::DiskFull);
    }

    // Read the file and compute hash (outside the lock).
    let data = Vfs::read_file(path)?;
    let hash = crate::crypto::sha256(&data);

    // Get mtime from metadata if available.
    let mtime_ns = {
        let full_meta = Vfs::metadata(path).unwrap_or_else(|_| {
            crate::fs::vfs::FileMeta::minimal(crate::fs::EntryType::File, data.len() as u64)
        });
        full_meta.modified_ns
    };

    // Store in the baseline.
    let mut inner = INTEGRITY.lock();
    inner.baseline.insert(path.into(), BaselineEntry {
        path: path.into(),
        hash,
        size: data.len() as u64,
        mtime_ns,
    });

    Ok(hash)
}

/// Baseline all files under a directory tree.
///
/// Walks the directory recursively, computing SHA-256 for each regular
/// file, and stores the results.  Returns the number of files baselined.
///
/// Skips files that are too large or in excluded directories.
/// Stops early if the baseline reaches max capacity.
pub fn baseline_dir(dir: &str) -> KernelResult<u64> {
    use crate::fs::{Vfs, EntryType};

    // Snapshot config (drop lock before I/O).
    let config = INTEGRITY.lock().config.clone();

    let mut count: u64 = 0;
    let mut dirs_to_visit: Vec<String> = Vec::new();
    dirs_to_visit.push(dir.into());

    while let Some(current_dir) = dirs_to_visit.pop() {
        // Check excluded directories.
        // Canonical subtree predicate; see fs::pathutil.  (Avoids a per-iter
        // `format!("{excl}/")` allocation the previous hand-rolled check made.)
        let skip = config
            .exclude_dirs
            .iter()
            .any(|excl| crate::fs::pathutil::path_in_subtree(current_dir.as_str(), excl.as_str()));
        if skip {
            continue;
        }

        let entries = match Vfs::readdir(&current_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in &entries {
            let path = if current_dir == "/" {
                alloc::format!("/{}", entry.name)
            } else {
                alloc::format!("{}/{}", current_dir, entry.name)
            };

            match entry.entry_type {
                EntryType::Directory => {
                    if entry.name != "." && entry.name != ".." {
                        dirs_to_visit.push(path);
                    }
                }
                EntryType::File => {
                    // Check capacity.
                    if INTEGRITY.lock().baseline.len() >= config.max_entries {
                        return Ok(count);
                    }

                    // Skip large files.
                    if entry.size > config.max_file_size {
                        continue;
                    }

                    // Read, hash, store.
                    match Vfs::read_file(&path) {
                        Ok(data) => {
                            let hash = crate::crypto::sha256(&data);

                            // Get mtime.
                            let mtime_ns = Vfs::metadata(&path)
                                .map(|m| m.modified_ns)
                                .unwrap_or(0);

                            let mut inner = INTEGRITY.lock();
                            inner.baseline.insert(path.clone(), BaselineEntry {
                                path,
                                hash,
                                size: data.len() as u64,
                                mtime_ns,
                            });
                            count = count.saturating_add(1);
                        }
                        Err(_) => continue, // Skip unreadable files.
                    }
                }
                _ => {} // Skip symlinks, etc.
            }
        }
    }

    // Update timestamp.
    let mut inner = INTEGRITY.lock();
    inner.baseline_timestamp = crate::hpet::elapsed_ns();
    inner.baseline_count = inner.baseline_count.saturating_add(1);

    Ok(count)
}

/// Clear the baseline.
pub fn clear_baseline() {
    let mut inner = INTEGRITY.lock();
    inner.baseline.clear();
    inner.baseline_timestamp = 0;
}

/// Get the number of entries in the baseline.
pub fn baseline_len() -> usize {
    INTEGRITY.lock().baseline.len()
}

/// Get the baseline timestamp (HPET nanoseconds).
#[allow(dead_code)]
pub fn baseline_timestamp() -> u64 {
    INTEGRITY.lock().baseline_timestamp
}

/// List baseline entries, optionally filtered by path prefix.
///
/// Returns up to `max` entries starting with `prefix` (or all if None).
/// Each entry is (path, hash, size).
pub fn list_entries(prefix: Option<&str>, max: usize) -> (Vec<(String, Hash256, u64)>, usize) {
    let inner = INTEGRITY.lock();
    let total = inner.baseline.len();
    let mut results = Vec::new();

    for (path, entry) in inner.baseline.iter() {
        if let Some(pfx) = prefix {
            // Canonical subtree predicate; see fs::pathutil.
            if !crate::fs::pathutil::path_in_subtree(path.as_str(), pfx) {
                continue;
            }
        }
        if results.len() >= max {
            break;
        }
        results.push((path.clone(), entry.hash, entry.size));
    }

    (results, total)
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Verify a single file against its baseline entry.
///
/// Returns `NotFound` if the file is not in the baseline.
pub fn verify_file(path: &str) -> KernelResult<VerifyResult> {
    use crate::fs::Vfs;

    let inner = INTEGRITY.lock();
    let entry = inner.baseline.get(path).ok_or(KernelError::NotFound)?;
    let baseline_hash = entry.hash;
    let baseline_size = entry.size;
    drop(inner);

    // Try to read the current file.
    match Vfs::read_file(path) {
        Ok(data) => {
            let current_hash = crate::crypto::sha256(&data);
            let current_size = data.len() as u64;

            let status = if current_hash == baseline_hash {
                VerifyStatus::Ok
            } else {
                VerifyStatus::Modified
            };

            Ok(VerifyResult {
                path: path.into(),
                status,
                baseline_hash: Some(baseline_hash),
                current_hash: Some(current_hash),
                baseline_size: Some(baseline_size),
                current_size: Some(current_size),
            })
        }
        Err(KernelError::NotFound) => {
            Ok(VerifyResult {
                path: path.into(),
                status: VerifyStatus::Missing,
                baseline_hash: Some(baseline_hash),
                current_hash: None,
                baseline_size: Some(baseline_size),
                current_size: None,
            })
        }
        Err(_) => {
            Ok(VerifyResult {
                path: path.into(),
                status: VerifyStatus::Error,
                baseline_hash: Some(baseline_hash),
                current_hash: None,
                baseline_size: Some(baseline_size),
                current_size: None,
            })
        }
    }
}

/// Verify all files in the baseline under a given directory prefix.
///
/// Also walks the directory tree to detect new files not in the baseline.
/// Returns a list of results and a summary.
pub fn verify_dir(dir: &str) -> (Vec<VerifyResult>, VerifySummary) {
    use crate::fs::{Vfs, EntryType};

    let mut results = Vec::new();
    let mut summary = VerifySummary::new();

    // Snapshot the baseline entries under this prefix.  The canonical
    // subtree predicate enforces the directory boundary (so a sibling like
    // "/tmp/dirX/.." never matches "/tmp/dir") and treats `dir == "/"` as
    // the whole tree.  A previous hand-rolled `byte-at-prefix.len() == '/'`
    // check was wrong against a trailing-slash prefix — it only matched
    // double-slash paths, so no real file was ever included and "missing"
    // detection in verify_dir silently never fired.  See fs::pathutil.
    let inner = INTEGRITY.lock();
    let baseline_paths: Vec<(String, Hash256, u64)> = inner.baseline
        .iter()
        .filter(|(p, _)| crate::fs::pathutil::path_in_subtree(p.as_str(), dir))
        .map(|(p, e)| (p.clone(), e.hash, e.size))
        .collect();
    let config = inner.config.clone();
    drop(inner);

    // Track which baseline paths we've verified (to detect missing files).
    let mut verified_paths: alloc::collections::BTreeSet<String> = alloc::collections::BTreeSet::new();

    // Walk the current filesystem to find current files.
    let mut dirs_to_visit: Vec<String> = Vec::new();
    dirs_to_visit.push(dir.into());

    while let Some(current_dir) = dirs_to_visit.pop() {
        // Check excluded directories.
        // Canonical subtree predicate; see fs::pathutil.  (Avoids a per-iter
        // `format!("{excl}/")` allocation the previous hand-rolled check made.)
        let skip = config
            .exclude_dirs
            .iter()
            .any(|excl| crate::fs::pathutil::path_in_subtree(current_dir.as_str(), excl.as_str()));
        if skip {
            continue;
        }

        let entries = match Vfs::readdir(&current_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in &entries {
            let path = if current_dir == "/" {
                alloc::format!("/{}", entry.name)
            } else {
                alloc::format!("{}/{}", current_dir, entry.name)
            };

            match entry.entry_type {
                EntryType::Directory => {
                    if entry.name != "." && entry.name != ".." {
                        dirs_to_visit.push(path);
                    }
                }
                EntryType::File => {
                    // Skip large files.
                    if entry.size > config.max_file_size {
                        continue;
                    }

                    // Check if this file is in the baseline.
                    let inner = INTEGRITY.lock();
                    let baseline_entry = inner.baseline.get(&path).cloned();
                    drop(inner);

                    if let Some(bl) = baseline_entry {
                        verified_paths.insert(path.clone());

                        // Read and hash current file.
                        match Vfs::read_file(&path) {
                            Ok(data) => {
                                let current_hash = crate::crypto::sha256(&data);
                                let current_size = data.len() as u64;

                                let status = if current_hash == bl.hash {
                                    VerifyStatus::Ok
                                } else {
                                    VerifyStatus::Modified
                                };

                                match status {
                                    VerifyStatus::Ok => summary.ok = summary.ok.saturating_add(1),
                                    VerifyStatus::Modified => summary.modified = summary.modified.saturating_add(1),
                                    _ => {}
                                }
                                summary.total = summary.total.saturating_add(1);

                                // Only add non-OK results to the list (to save memory).
                                if status != VerifyStatus::Ok {
                                    results.push(VerifyResult {
                                        path,
                                        status,
                                        baseline_hash: Some(bl.hash),
                                        current_hash: Some(current_hash),
                                        baseline_size: Some(bl.size),
                                        current_size: Some(current_size),
                                    });
                                }
                            }
                            Err(_) => {
                                summary.errors = summary.errors.saturating_add(1);
                                summary.total = summary.total.saturating_add(1);
                                results.push(VerifyResult {
                                    path,
                                    status: VerifyStatus::Error,
                                    baseline_hash: Some(bl.hash),
                                    current_hash: None,
                                    baseline_size: Some(bl.size),
                                    current_size: None,
                                });
                            }
                        }
                    } else {
                        // New file not in baseline.
                        summary.new = summary.new.saturating_add(1);
                        summary.total = summary.total.saturating_add(1);
                        results.push(VerifyResult {
                            path,
                            status: VerifyStatus::New,
                            baseline_hash: None,
                            current_hash: None, // Don't bother hashing new files.
                            baseline_size: None,
                            current_size: Some(entry.size),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // Check for missing files: baseline entries not seen during walk.
    for (bp, bh, bs) in &baseline_paths {
        if !verified_paths.contains(bp) {
            summary.missing = summary.missing.saturating_add(1);
            summary.total = summary.total.saturating_add(1);
            results.push(VerifyResult {
                path: bp.clone(),
                status: VerifyStatus::Missing,
                baseline_hash: Some(*bh),
                current_hash: None,
                baseline_size: Some(*bs),
                current_size: None,
            });
        }
    }

    // Update verify count.  Acquire the lock exactly once: writing
    // `INTEGRITY.lock().x = INTEGRITY.lock().y` keeps both temporary lock
    // guards alive until the end of the statement, which deadlocks the
    // non-reentrant mutex on the second acquisition.
    {
        let mut inner = INTEGRITY.lock();
        inner.verify_count = inner.verify_count.saturating_add(1);
    }

    (results, summary)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Statistics about the integrity monitoring system.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct IntegrityStats {
    /// Number of entries in the baseline.
    pub baseline_entries: usize,
    /// When the baseline was last updated (HPET nanoseconds, 0 = never).
    pub baseline_timestamp: u64,
    /// Total baseline operations.
    pub baseline_count: u64,
    /// Total verify operations.
    pub verify_count: u64,
    /// Maximum entries allowed.
    pub max_entries: usize,
    /// Maximum file size hashed.
    pub max_file_size: u64,
}

/// Get a snapshot of integrity monitoring statistics.
pub fn stats() -> IntegrityStats {
    let inner = INTEGRITY.lock();
    IntegrityStats {
        baseline_entries: inner.baseline.len(),
        baseline_timestamp: inner.baseline_timestamp,
        baseline_count: inner.baseline_count,
        verify_count: inner.verify_count,
        max_entries: inner.config.max_entries,
        max_file_size: inner.config.max_file_size,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the integrity monitoring module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[integrity] Running self-test...");

    // --- Test 1: baseline and verify a single file ---
    {
        use crate::fs::Vfs;

        // Create a test file in memfs.
        let test_path = "/tmp/_integrity_test_1";
        let test_data = b"Integrity test file content";

        if let Err(e) = Vfs::write_file(test_path, test_data) {
            serial_println!("[integrity]   SKIP: cannot write test file: {:?}", e);
            serial_println!("[integrity] Self-test skipped (no writable filesystem).");
            return Ok(());
        }

        // Baseline.
        let hash = baseline_file(test_path)?;
        if hash == [0u8; 32] {
            serial_println!("[integrity]   ERROR: baseline returned zero hash");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        // Verify (should be OK).
        let result = verify_file(test_path)?;
        if result.status != VerifyStatus::Ok {
            serial_println!("[integrity]   ERROR: expected Ok, got {:?}", result.status);
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[integrity]   baseline + verify (unmodified) OK");

        // Modify the file.
        Vfs::write_file(test_path, b"MODIFIED content")?;

        // Verify (should be Modified).
        let result = verify_file(test_path)?;
        if result.status != VerifyStatus::Modified {
            serial_println!("[integrity]   ERROR: expected Modified, got {:?}", result.status);
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[integrity]   verify (modified) OK");

        // Delete the file.
        Vfs::remove(test_path)?;

        // Verify (should be Missing).
        let result = verify_file(test_path)?;
        if result.status != VerifyStatus::Missing {
            serial_println!("[integrity]   ERROR: expected Missing, got {:?}", result.status);
            return Err(KernelError::InternalError);
        }

        serial_println!("[integrity]   verify (missing) OK");
    }

    // --- Test 2: directory baseline and verify ---
    {
        use crate::fs::Vfs;

        clear_baseline();

        // Create a small directory tree.
        Vfs::mkdir("/tmp/_integrity_dir").ok();
        Vfs::write_file("/tmp/_integrity_dir/file1.txt", b"File one")?;
        Vfs::write_file("/tmp/_integrity_dir/file2.txt", b"File two")?;

        // Baseline the directory.
        let count = baseline_dir("/tmp/_integrity_dir")?;
        if count < 2 {
            serial_println!("[integrity]   ERROR: expected at least 2 baselined files, got {}", count);
            Vfs::remove("/tmp/_integrity_dir/file1.txt").ok();
            Vfs::remove("/tmp/_integrity_dir/file2.txt").ok();
            Vfs::rmdir("/tmp/_integrity_dir").ok();
            return Err(KernelError::InternalError);
        }

        // Verify (all should be OK).
        let (_changes, summary) = verify_dir("/tmp/_integrity_dir");
        if summary.ok < 2 {
            serial_println!("[integrity]   ERROR: expected at least 2 OK files, got {}", summary.ok);
            Vfs::remove("/tmp/_integrity_dir/file1.txt").ok();
            Vfs::remove("/tmp/_integrity_dir/file2.txt").ok();
            Vfs::rmdir("/tmp/_integrity_dir").ok();
            return Err(KernelError::InternalError);
        }

        // Modify one file.
        Vfs::write_file("/tmp/_integrity_dir/file1.txt", b"TAMPERED")?;

        // Verify (should detect 1 modified).
        let (_changes, summary) = verify_dir("/tmp/_integrity_dir");
        if summary.modified < 1 {
            serial_println!("[integrity]   ERROR: expected at least 1 modified, got {}", summary.modified);
            Vfs::remove("/tmp/_integrity_dir/file1.txt").ok();
            Vfs::remove("/tmp/_integrity_dir/file2.txt").ok();
            Vfs::rmdir("/tmp/_integrity_dir").ok();
            return Err(KernelError::InternalError);
        }

        // Add a new file.
        Vfs::write_file("/tmp/_integrity_dir/file3.txt", b"New file")?;

        // Verify (should detect 1 new).
        let (_, summary) = verify_dir("/tmp/_integrity_dir");
        if summary.new < 1 {
            serial_println!("[integrity]   ERROR: expected at least 1 new, got {}", summary.new);
        }

        // Delete one baselined file.
        Vfs::remove("/tmp/_integrity_dir/file2.txt")?;

        // Verify (should detect 1 missing).
        let (_, summary) = verify_dir("/tmp/_integrity_dir");
        if summary.missing < 1 {
            serial_println!("[integrity]   ERROR: expected at least 1 missing, got {}", summary.missing);
        }

        // Cleanup.
        Vfs::remove("/tmp/_integrity_dir/file1.txt").ok();
        Vfs::remove("/tmp/_integrity_dir/file3.txt").ok();
        Vfs::rmdir("/tmp/_integrity_dir").ok();

        serial_println!("[integrity]   directory baseline + verify OK");
    }

    // --- Test 3: stats ---
    {
        let st = stats();
        if st.baseline_count < 1 {
            serial_println!("[integrity]   ERROR: baseline_count should be >= 1");
            return Err(KernelError::InternalError);
        }
        serial_println!("[integrity]   stats OK (entries: {}, baselines: {}, verifies: {})",
            st.baseline_entries, st.baseline_count, st.verify_count);
    }

    // --- Test 4: clear baseline ---
    {
        clear_baseline();
        if baseline_len() != 0 {
            serial_println!("[integrity]   ERROR: baseline should be empty after clear");
            return Err(KernelError::InternalError);
        }
        serial_println!("[integrity]   clear OK");
    }

    // --- Test 5: non-file baseline (should fail) ---
    {
        match baseline_file("/tmp") {
            Err(KernelError::InvalidArgument) => {}
            other => {
                serial_println!("[integrity]   ERROR: baselining a directory should return InvalidArgument, got {:?}", other);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[integrity]   non-file rejection OK");
    }

    // --- Test 6: verify non-baselined file ---
    {
        match verify_file("/nonexistent_baseline_entry") {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!("[integrity]   ERROR: verifying non-baselined file should return NotFound, got {:?}", other);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[integrity]   non-baselined verify OK");
    }

    serial_println!("[integrity] Self-test passed (6 tests).");
    Ok(())
}
