//! Automatic temporary file cleanup (tmpwatch).
//!
//! Periodically scans configured directories (primarily `/tmp`) and
//! removes files that have not been accessed within a configurable
//! age threshold.  This prevents `/tmp` from growing unbounded on
//! long-running systems.
//!
//! ## Design
//!
//! - **Non-destructive by default**: only removes regular files, not
//!   directories or symlinks (unless explicitly configured).
//! - **Age-based**: files older than `max_age_secs` (based on the
//!   file's modification timestamp) are candidates for removal.
//! - **Exclude patterns**: critical paths and patterns can be excluded
//!   (e.g., `/tmp/overlay_*` used by the overlay filesystem).
//! - **Dry-run mode**: scan and report what would be removed without
//!   actually deleting anything.
//! - **Size threshold**: optionally only clean files above or below
//!   a certain size.
//! - **Statistics**: tracks total removed bytes, file count, and
//!   last run timestamp.
//!
//! ## Usage
//!
//! ```text
//! tmpwatch --run                   # Run cleanup now
//! tmpwatch --dry-run               # Show what would be removed
//! tmpwatch --max-age 3600          # Set max age to 1 hour
//! tmpwatch --add /var/tmp          # Add a watch directory
//! tmpwatch --status                # Show configuration and stats
//! ```
//!
//! ## Scheduling
//!
//! The module provides a `run()` function that performs one cleanup
//! pass.  The kshell or a timer can call it periodically.  The module
//! does not create its own timer or thread.
//!
//! ## Reference
//!
//! systemd-tmpfiles(8), tmpwatch(8), tmpreaper(8)

#![allow(dead_code)]

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::Vfs;
use crate::fs::EntryType;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default maximum file age in seconds (24 hours).
pub const DEFAULT_MAX_AGE_SECS: u64 = 86400;

/// Default minimum file size to consider (0 = all files).
pub const DEFAULT_MIN_SIZE: u64 = 0;

/// Maximum recursion depth when scanning directories.
const MAX_SCAN_DEPTH: u32 = 16;

/// Result of a cleanup run.
#[derive(Debug, Clone)]
pub struct CleanupResult {
    /// Number of files removed.
    pub files_removed: u64,
    /// Total bytes freed.
    pub bytes_freed: u64,
    /// Number of files skipped (excluded, too new, etc.).
    pub files_skipped: u64,
    /// Number of errors encountered during removal.
    pub errors: u64,
    /// Paths of files that were removed (for reporting).
    pub removed_paths: Vec<String>,
}

/// Persistent statistics across runs.
#[derive(Debug, Clone, Copy)]
pub struct TmpwatchStats {
    /// Total files removed across all runs.
    pub total_files_removed: u64,
    /// Total bytes freed across all runs.
    pub total_bytes_freed: u64,
    /// Number of cleanup runs performed.
    pub runs: u64,
    /// Timestamp of the last run (0 = never).
    pub last_run: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct TmpwatchInner {
    /// Directories to scan.
    watch_dirs: Vec<String>,
    /// Path prefixes to exclude from cleanup.
    excludes: BTreeSet<String>,
    /// Maximum file age in seconds.
    max_age_secs: u64,
    /// Minimum file size to consider (0 = all).
    min_size: u64,
    /// Whether to also remove empty directories.
    remove_empty_dirs: bool,
    /// Whether the module is enabled.
    enabled: bool,
    /// Cumulative statistics.
    stats: TmpwatchStats,
}

static TMPWATCH: Mutex<TmpwatchInner> = Mutex::new(TmpwatchInner {
    watch_dirs: Vec::new(),
    excludes: BTreeSet::new(),
    max_age_secs: DEFAULT_MAX_AGE_SECS,
    min_size: DEFAULT_MIN_SIZE,
    remove_empty_dirs: false,
    enabled: true,
    stats: TmpwatchStats {
        total_files_removed: 0,
        total_bytes_freed: 0,
        runs: 0,
        last_run: 0,
    },
});

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize tmpwatch with default configuration.
///
/// Adds `/tmp` as a watched directory and sets up default excludes.
pub fn init() {
    let mut inner = TMPWATCH.lock();

    if inner.watch_dirs.is_empty() {
        inner.watch_dirs.push(String::from("/tmp"));
    }

    // Default excludes: overlay work dirs, pipe paths, etc.
    inner.excludes.insert(String::from("/tmp/overlay_"));
    inner.excludes.insert(String::from("/tmp/."));
}

// ---------------------------------------------------------------------------
// Configuration API
// ---------------------------------------------------------------------------

/// Add a directory to the watch list.
pub fn add_watch_dir(path: &str) -> KernelResult<()> {
    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut inner = TMPWATCH.lock();
    let s = String::from(path);
    if !inner.watch_dirs.contains(&s) {
        inner.watch_dirs.push(s);
    }
    Ok(())
}

/// Remove a directory from the watch list.
pub fn remove_watch_dir(path: &str) -> bool {
    let mut inner = TMPWATCH.lock();
    let before = inner.watch_dirs.len();
    inner.watch_dirs.retain(|d| d != path);
    inner.watch_dirs.len() < before
}

/// List watched directories.
pub fn watch_dirs() -> Vec<String> {
    TMPWATCH.lock().watch_dirs.clone()
}

/// Add a path prefix to exclude from cleanup.
pub fn add_exclude(prefix: &str) {
    TMPWATCH.lock().excludes.insert(String::from(prefix));
}

/// Remove an exclusion.
pub fn remove_exclude(prefix: &str) -> bool {
    TMPWATCH.lock().excludes.remove(prefix)
}

/// List exclusions.
pub fn excludes() -> Vec<String> {
    TMPWATCH.lock().excludes.iter().cloned().collect()
}

/// Set the maximum file age in seconds.
pub fn set_max_age(secs: u64) {
    TMPWATCH.lock().max_age_secs = secs;
}

/// Get the current maximum file age.
pub fn max_age() -> u64 {
    TMPWATCH.lock().max_age_secs
}

/// Set the minimum file size threshold.
pub fn set_min_size(bytes: u64) {
    TMPWATCH.lock().min_size = bytes;
}

/// Enable/disable automatic cleanup.
pub fn set_enabled(enabled: bool) {
    TMPWATCH.lock().enabled = enabled;
}

/// Check if tmpwatch is enabled.
pub fn is_enabled() -> bool {
    TMPWATCH.lock().enabled
}

/// Enable/disable empty directory removal.
pub fn set_remove_empty_dirs(remove: bool) {
    TMPWATCH.lock().remove_empty_dirs = remove;
}

/// Get statistics.
pub fn stats() -> TmpwatchStats {
    TMPWATCH.lock().stats
}

// ---------------------------------------------------------------------------
// Core — cleanup
// ---------------------------------------------------------------------------

/// Perform a cleanup pass on all watched directories.
///
/// `now` is the current timestamp in seconds since epoch.  Pass 0 to
/// use an internal heuristic (all files are considered old).
pub fn run(now: u64) -> KernelResult<CleanupResult> {
    let (dirs, excludes, max_age, min_size, enabled, remove_empty) = {
        let inner = TMPWATCH.lock();
        if !inner.enabled {
            return Ok(CleanupResult {
                files_removed: 0,
                bytes_freed: 0,
                files_skipped: 0,
                errors: 0,
                removed_paths: Vec::new(),
            });
        }
        (
            inner.watch_dirs.clone(),
            inner.excludes.clone(),
            inner.max_age_secs,
            inner.min_size,
            inner.enabled,
            inner.remove_empty_dirs,
        )
    };

    if !enabled {
        return Ok(CleanupResult {
            files_removed: 0,
            bytes_freed: 0,
            files_skipped: 0,
            errors: 0,
            removed_paths: Vec::new(),
        });
    }

    let mut result = CleanupResult {
        files_removed: 0,
        bytes_freed: 0,
        files_skipped: 0,
        errors: 0,
        removed_paths: Vec::new(),
    };

    for dir in &dirs {
        scan_directory(dir, now, max_age, min_size, &excludes, remove_empty, 0, &mut result);
    }

    // Update stats.
    let mut inner = TMPWATCH.lock();
    inner.stats.total_files_removed = inner.stats.total_files_removed
        .saturating_add(result.files_removed);
    inner.stats.total_bytes_freed = inner.stats.total_bytes_freed
        .saturating_add(result.bytes_freed);
    inner.stats.runs = inner.stats.runs.saturating_add(1);
    inner.stats.last_run = now;

    Ok(result)
}

/// Perform a dry-run (report what would be removed without deleting).
pub fn dry_run(now: u64) -> KernelResult<Vec<(String, u64)>> {
    let (dirs, excludes, max_age, min_size) = {
        let inner = TMPWATCH.lock();
        (
            inner.watch_dirs.clone(),
            inner.excludes.clone(),
            inner.max_age_secs,
            inner.min_size,
        )
    };

    let mut candidates = Vec::new();

    for dir in &dirs {
        collect_candidates(dir, now, max_age, min_size, &excludes, 0, &mut candidates);
    }

    Ok(candidates)
}

// ---------------------------------------------------------------------------
// Internal scanning
// ---------------------------------------------------------------------------

/// Recursively scan a directory and remove old files.
fn scan_directory(
    dir: &str,
    now: u64,
    max_age: u64,
    min_size: u64,
    excludes: &BTreeSet<String>,
    remove_empty: bool,
    depth: u32,
    result: &mut CleanupResult,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }

    let entries = match Vfs::readdir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let full_path = format!("{}/{}", dir.trim_end_matches('/'), entry.name);

        // Check excludes.
        if is_excluded(&full_path, excludes) {
            result.files_skipped = result.files_skipped.saturating_add(1);
            continue;
        }

        match entry.entry_type {
            EntryType::Directory => {
                // Recurse into subdirectory.
                scan_directory(&full_path, now, max_age, min_size, excludes, remove_empty,
                    depth + 1, result);

                // Optionally remove empty directories after their contents are cleaned.
                if remove_empty {
                    if let Ok(sub_entries) = Vfs::readdir(&full_path) {
                        if sub_entries.is_empty() {
                            if Vfs::rmdir(&full_path).is_ok() {
                                result.files_removed = result.files_removed.saturating_add(1);
                                result.removed_paths.push(full_path);
                            }
                        }
                    }
                }
            }
            EntryType::File => {
                if should_remove(&full_path, entry.size, now, max_age, min_size) {
                    if Vfs::remove(&full_path).is_ok() {
                        result.files_removed = result.files_removed.saturating_add(1);
                        result.bytes_freed = result.bytes_freed.saturating_add(entry.size);
                        result.removed_paths.push(full_path);
                    } else {
                        result.errors = result.errors.saturating_add(1);
                    }
                } else {
                    result.files_skipped = result.files_skipped.saturating_add(1);
                }
            }
            _ => {
                // Skip symlinks, devices, etc.
                result.files_skipped = result.files_skipped.saturating_add(1);
            }
        }
    }
}

/// Collect candidates for dry-run without removing.
fn collect_candidates(
    dir: &str,
    now: u64,
    max_age: u64,
    min_size: u64,
    excludes: &BTreeSet<String>,
    depth: u32,
    candidates: &mut Vec<(String, u64)>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }

    let entries = match Vfs::readdir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let full_path = format!("{}/{}", dir.trim_end_matches('/'), entry.name);

        if is_excluded(&full_path, excludes) {
            continue;
        }

        match entry.entry_type {
            EntryType::Directory => {
                collect_candidates(&full_path, now, max_age, min_size, excludes,
                    depth + 1, candidates);
            }
            EntryType::File => {
                if should_remove(&full_path, entry.size, now, max_age, min_size) {
                    candidates.push((full_path, entry.size));
                }
            }
            _ => {}
        }
    }
}

/// Check if a path matches any exclude prefix.
fn is_excluded(path: &str, excludes: &BTreeSet<String>) -> bool {
    for exc in excludes {
        if path.starts_with(exc.as_str()) {
            return true;
        }
    }
    false
}

/// Determine if a file should be removed based on age and size.
fn should_remove(path: &str, size: u64, now: u64, max_age: u64, min_size: u64) -> bool {
    // Size check.
    if size < min_size {
        return false;
    }

    // Age check: use file metadata timestamp.
    if now == 0 {
        // No clock available — remove all matching files.
        return true;
    }

    if let Ok(meta) = Vfs::metadata(path) {
        let mtime_secs = meta.modified_ns / 1_000_000_000;
        if mtime_secs == 0 {
            // No timestamp — consider it old.
            return true;
        }
        // File age in seconds.
        let age = now.saturating_sub(mtime_secs);
        age >= max_age
    } else {
        // Can't stat — skip.
        false
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the tmpwatch module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[tmpwatch] Running self-test...");

    let test_dir = "/tmp/tmpwatch_test";
    let _ = Vfs::remove_recursive(test_dir);
    Vfs::mkdir(test_dir)?;

    // --- Test 1: Init and default config ---
    {
        init();
        let dirs = watch_dirs();
        if !dirs.contains(&String::from("/tmp")) {
            serial_println!("[tmpwatch]   ERROR: /tmp not in watch dirs");
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[tmpwatch]   init + defaults: OK");
    }

    // --- Test 2: Add/remove watch dir ---
    {
        add_watch_dir(test_dir)?;
        let dirs = watch_dirs();
        if !dirs.contains(&String::from(test_dir)) {
            serial_println!("[tmpwatch]   ERROR: test dir not added");
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }
        remove_watch_dir(test_dir);
        let dirs = watch_dirs();
        if dirs.contains(&String::from(test_dir)) {
            serial_println!("[tmpwatch]   ERROR: test dir not removed");
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[tmpwatch]   add/remove watch dir: OK");
    }

    // --- Test 3: Add/remove exclude ---
    {
        add_exclude("/tmp/important_");
        let exc = excludes();
        if !exc.contains(&String::from("/tmp/important_")) {
            serial_println!("[tmpwatch]   ERROR: exclude not added");
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }
        remove_exclude("/tmp/important_");
        serial_println!("[tmpwatch]   add/remove exclude: OK");
    }

    // --- Test 4: Exclude check ---
    {
        let mut exc = BTreeSet::new();
        exc.insert(String::from("/tmp/keep_"));
        if !is_excluded("/tmp/keep_this.txt", &exc) {
            serial_println!("[tmpwatch]   ERROR: excluded path not detected");
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }
        if is_excluded("/tmp/other.txt", &exc) {
            serial_println!("[tmpwatch]   ERROR: non-excluded path flagged");
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[tmpwatch]   exclude check: OK");
    }

    // --- Test 5: Cleanup removes old files ---
    {
        // Add test dir to watch list and create test files.
        // Use now=0 to make all files appear "old".
        add_watch_dir(test_dir)?;

        Vfs::write_file(&format!("{}/old1.tmp", test_dir), b"old data 1")?;
        Vfs::write_file(&format!("{}/old2.tmp", test_dir), b"old data 2")?;

        // Remove default excludes that might interfere.
        let saved_excludes = excludes();
        for e in &saved_excludes {
            remove_exclude(e);
        }

        let result = run(0)?;  // now=0 means all files are considered old

        if result.files_removed < 2 {
            serial_println!("[tmpwatch]   ERROR: only removed {} files", result.files_removed);
            // Restore excludes.
            for e in &saved_excludes {
                add_exclude(e);
            }
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        // Verify files are gone.
        if Vfs::exists(&format!("{}/old1.tmp", test_dir)) {
            serial_println!("[tmpwatch]   ERROR: old1.tmp still exists");
            for e in &saved_excludes { add_exclude(e); }
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        // Restore excludes.
        for e in &saved_excludes {
            add_exclude(e);
        }
        remove_watch_dir(test_dir);
        serial_println!("[tmpwatch]   cleanup removes files: OK (removed {})", result.files_removed);
    }

    // --- Test 6: Excludes protect files ---
    {
        let _ = Vfs::remove_recursive(test_dir);
        Vfs::mkdir(test_dir)?;

        add_watch_dir(test_dir)?;
        add_exclude(&format!("{}/keep", test_dir));

        Vfs::write_file(&format!("{}/delete_me.tmp", test_dir), b"delete")?;
        Vfs::write_file(&format!("{}/keep_me.tmp", test_dir), b"keep")?;

        // Remove all other excludes to prevent interference.
        let saved_excludes: Vec<String> = excludes().into_iter()
            .filter(|e| !e.starts_with(test_dir))
            .collect();
        for e in &saved_excludes {
            remove_exclude(e);
        }

        let result = run(0)?;

        // keep_me.tmp should still exist (matches exclude prefix).
        if !Vfs::exists(&format!("{}/keep_me.tmp", test_dir)) {
            serial_println!("[tmpwatch]   ERROR: excluded file was removed");
            for e in &saved_excludes { add_exclude(e); }
            remove_exclude(&format!("{}/keep", test_dir));
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        // delete_me.tmp should be gone.
        if Vfs::exists(&format!("{}/delete_me.tmp", test_dir)) {
            serial_println!("[tmpwatch]   ERROR: non-excluded file survived");
            for e in &saved_excludes { add_exclude(e); }
            remove_exclude(&format!("{}/keep", test_dir));
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        for e in &saved_excludes { add_exclude(e); }
        remove_exclude(&format!("{}/keep", test_dir));
        remove_watch_dir(test_dir);
        let _ = result;
        serial_println!("[tmpwatch]   excludes protect files: OK");
    }

    // --- Test 7: Dry-run doesn't delete ---
    {
        let _ = Vfs::remove_recursive(test_dir);
        Vfs::mkdir(test_dir)?;

        add_watch_dir(test_dir)?;

        Vfs::write_file(&format!("{}/dryrun.tmp", test_dir), b"dry run data")?;

        let saved_excludes: Vec<String> = excludes().into_iter()
            .filter(|e| !e.starts_with(test_dir))
            .collect();
        for e in &saved_excludes { remove_exclude(e); }

        let candidates = dry_run(0)?;

        // File should still exist.
        if !Vfs::exists(&format!("{}/dryrun.tmp", test_dir)) {
            serial_println!("[tmpwatch]   ERROR: dry run deleted files!");
            for e in &saved_excludes { add_exclude(e); }
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        if candidates.is_empty() {
            serial_println!("[tmpwatch]   ERROR: dry run found no candidates");
            for e in &saved_excludes { add_exclude(e); }
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        for e in &saved_excludes { add_exclude(e); }
        remove_watch_dir(test_dir);
        serial_println!("[tmpwatch]   dry-run: OK ({} candidates)", candidates.len());
    }

    // --- Test 8: Disabled skips cleanup ---
    {
        let _ = Vfs::remove_recursive(test_dir);
        Vfs::mkdir(test_dir)?;

        add_watch_dir(test_dir)?;
        Vfs::write_file(&format!("{}/disabled.tmp", test_dir), b"should stay")?;

        set_enabled(false);
        let result = run(0)?;
        set_enabled(true);

        if result.files_removed != 0 {
            serial_println!("[tmpwatch]   ERROR: disabled but still cleaned");
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        if !Vfs::exists(&format!("{}/disabled.tmp", test_dir)) {
            serial_println!("[tmpwatch]   ERROR: disabled but file removed");
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        remove_watch_dir(test_dir);
        serial_println!("[tmpwatch]   disabled skips: OK");
    }

    // --- Test 9: Stats accumulate ---
    {
        let s = stats();
        if s.runs == 0 {
            serial_println!("[tmpwatch]   ERROR: run count is 0");
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[tmpwatch]   stats: OK (runs={} files_removed={} bytes_freed={})",
            s.runs, s.total_files_removed, s.total_bytes_freed);
    }

    // --- Test 10: Recursive subdirectory scan ---
    {
        let _ = Vfs::remove_recursive(test_dir);
        Vfs::mkdir(test_dir)?;
        Vfs::mkdir(&format!("{}/sub", test_dir))?;
        Vfs::write_file(&format!("{}/sub/deep.tmp", test_dir), b"deep data")?;

        add_watch_dir(test_dir)?;

        let saved_excludes: Vec<String> = excludes().into_iter()
            .filter(|e| !e.starts_with(test_dir))
            .collect();
        for e in &saved_excludes { remove_exclude(e); }

        let result = run(0)?;

        if Vfs::exists(&format!("{}/sub/deep.tmp", test_dir)) {
            serial_println!("[tmpwatch]   ERROR: deep file not removed");
            for e in &saved_excludes { add_exclude(e); }
            remove_watch_dir(test_dir);
            let _ = Vfs::remove_recursive(test_dir);
            return Err(KernelError::InternalError);
        }

        for e in &saved_excludes { add_exclude(e); }
        remove_watch_dir(test_dir);
        serial_println!("[tmpwatch]   recursive scan: OK (removed {})", result.files_removed);
    }

    // --- Cleanup ---
    let _ = Vfs::remove_recursive(test_dir);

    serial_println!("[tmpwatch] Self-test passed (10 tests).");
    Ok(())
}
