//! File version history tracking.
//!
//! Provides automatic versioning of file contents.  When enabled for a
//! path, each modification saves the previous content to the CAS (content-
//! addressed store) and records the version in a per-file history chain.
//!
//! ## Design
//!
//! - **CAS-backed**: Old file versions are stored in `fs::cas`.  The CAS
//!   automatically deduplicates — if two files had identical content before
//!   being modified, only one copy of that content is stored.
//! - **Per-path history**: Each tracked path has a bounded list of version
//!   entries: `(timestamp, hash, size)`.  Older versions beyond the limit
//!   are evicted (and their CAS references released for GC).
//! - **Opt-in**: Not all paths are tracked.  The caller decides which
//!   paths/directories to watch.  The VFS can call `record_version()`
//!   before overwriting a file.
//! - **Bounded**: configurable max versions per file and max total entries
//!   to prevent unbounded memory growth.
//!
//! ## Use cases
//!
//! - **Undo**: Restore a file to a previous version.
//! - **Diff**: Compare current file content with a previous version.
//! - **Audit**: Know when a file was modified and what it contained before.
//! - **Package rollback**: The package manager can use this to roll back
//!   individual file changes within a generation.
//!
//! ## Reference
//!
//! design.txt: "make a snapshot or restore from snapshot feature, with
//! branching like a VM does? options for what to include in the snapshot?"

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::cas::Hash256;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single version entry for a file.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VersionEntry {
    /// SHA-256 hash of the file content (stored in CAS).
    pub hash: Hash256,
    /// File size in bytes at this version.
    pub size: u64,
    /// HPET timestamp (nanoseconds since boot) when this version was recorded.
    pub timestamp_ns: u64,
    /// Monotonically increasing version number per file.
    pub version: u64,
}

/// Configuration for the history system.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HistoryConfig {
    /// Maximum number of versions to keep per file.
    /// Older versions are evicted when this is exceeded.
    pub max_versions_per_file: usize,
    /// Maximum total number of version entries across all files.
    pub max_total_entries: usize,
    /// Whether history tracking is enabled.
    pub enabled: bool,
    /// Whether VFS auto-versioning is active.
    ///
    /// When true, the VFS automatically calls `record_version()` before
    /// overwriting or removing files.  Independent of `enabled` — manual
    /// recording via the kshell `fhist record` command works regardless.
    pub auto_version: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_versions_per_file: 16,
            max_total_entries: 10_000,
            enabled: true,
            auto_version: true,
        }
    }
}

/// Statistics about the history system.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct HistoryStats {
    /// Number of tracked files (files with at least one version).
    pub tracked_files: usize,
    /// Total number of version entries across all files.
    pub total_versions: usize,
    /// Number of versions that were evicted (exceeded per-file limit).
    pub evicted_versions: u64,
    /// Number of record operations performed.
    pub record_count: u64,
    /// Number of restore operations performed.
    pub restore_count: u64,
    /// Whether history tracking is currently enabled.
    pub enabled: bool,
    /// Whether VFS auto-versioning is active.
    pub auto_version: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Per-file version history.
struct FileHistory {
    /// Ordered list of versions, newest last.
    versions: Vec<VersionEntry>,
    /// Next version number.
    next_version: u64,
}

struct HistoryInner {
    /// Map from file path to its version history.
    files: BTreeMap<String, FileHistory>,
    /// Total number of version entries.
    total_entries: usize,
    /// Configuration.
    config: HistoryConfig,
    /// Statistics.
    evicted_versions: u64,
    record_count: u64,
    restore_count: u64,
}

static HISTORY: Mutex<HistoryInner> = Mutex::new(HistoryInner {
    files: BTreeMap::new(),
    total_entries: 0,
    config: HistoryConfig {
        max_versions_per_file: 16,
        max_total_entries: 10_000,
        enabled: true,
        // Auto-versioning starts DISABLED and is turned on at BOOT_OK by
        // `main.rs` (see `set_auto_version(true)` there). Rationale: during
        // boot the kernel stages its own system files (e.g. the glibc tree for
        // the Path Z self-tests) with interrupts disabled (IF=0), before
        // "Step 21: Enable hardware interrupts". Auto-versioning would read and
        // SHA-256-hash the *old* content of each overwritten file on that path;
        // for a multi-megabyte file in a debug build that hash can run for
        // several seconds. With IF=0 the timer-driven hard-lockup watchdog kick
        // is starved, so under host-scheduling jitter the ~9.8 s watchdog fired
        // a false positive that presented as an intermittent "BSP-dead
        // total-silence hang" (known-issues.md B-PTHREAD-YIELDBUDGET). Versioning
        // OS files as they are staged is also pointless — nobody rolls those
        // back. Enabling only post-boot (IF=1, preemptible, staging complete)
        // fixes both the latency defect and the wasted work.
        auto_version: false,
    },
    evicted_versions: 0,
    record_count: 0,
    restore_count: 0,
});

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the history configuration.
#[allow(dead_code)]
pub fn set_config(config: HistoryConfig) {
    HISTORY.lock().config = config;
}

/// Check if history tracking is enabled.
#[allow(dead_code)]
pub fn is_enabled() -> bool {
    HISTORY.lock().config.enabled
}

/// Enable or disable history tracking.
pub fn set_enabled(enabled: bool) {
    HISTORY.lock().config.enabled = enabled;
}

/// Check if VFS auto-versioning is active.
///
/// Auto-versioning requires both `enabled` and `auto_version` to be true.
pub fn is_auto_version_enabled() -> bool {
    let inner = HISTORY.lock();
    inner.config.enabled && inner.config.auto_version
}

/// Enable or disable VFS auto-versioning.
///
/// When enabled, the VFS automatically records old file content before
/// overwriting or removing files.
pub fn set_auto_version(enabled: bool) {
    HISTORY.lock().config.auto_version = enabled;
}

/// Check if a path is eligible for automatic version recording.
///
/// Returns `false` for paths on virtual filesystems (procfs, devfs, sysfs),
/// temporary files (/tmp), and internal metadata files.
pub fn should_auto_version(path: &str) -> bool {
    // Skip virtual filesystems — no real data to version.
    if path.starts_with("/proc/")
        || path.starts_with("/dev/")
        || path.starts_with("/sys/")
    {
        return false;
    }
    // Skip temporary files — ephemeral by nature.
    if path.starts_with("/tmp/") {
        return false;
    }
    // Skip internal metadata files.
    if path.ends_with("/_TRASH/_INDEX") || path.ends_with("/_JOURNAL") {
        return false;
    }
    true
}

/// Try to auto-record a version of a file before it is modified or deleted.
///
/// Called by VFS write/remove paths.  Failures are silently ignored —
/// version history is best-effort and must never prevent a write operation.
pub fn try_auto_record(path: &str) {
    if !is_auto_version_enabled() {
        return;
    }
    if !should_auto_version(path) {
        return;
    }
    // Non-fatal: ignore errors from recording.  The file operation
    // must succeed even if history recording fails (e.g., CAS full,
    // file too large, read error).
    let _ = record_version(path);
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Record a new version of a file before it is overwritten.
///
/// This should be called by the VFS (or other code) before modifying a file.
/// The current file contents are read, hashed, and stored in the CAS.
/// A version entry is added to the file's history.
///
/// Returns the CAS hash of the saved version.
///
/// If history is disabled or the file doesn't exist, returns Ok(None).
pub fn record_version(path: &str) -> KernelResult<Option<Hash256>> {
    use crate::fs::Vfs;

    // Check if enabled.
    let inner = HISTORY.lock();
    if !inner.config.enabled {
        return Ok(None);
    }
    let max_per_file = inner.config.max_versions_per_file;
    let max_total = inner.config.max_total_entries;
    drop(inner);

    // Read the current file content (outside the lock).
    let data = match Vfs::read_file(path) {
        Ok(d) => d,
        Err(KernelError::NotFound) => return Ok(None), // No file to version.
        Err(e) => return Err(e),
    };

    // Store in CAS.
    let hash = crate::fs::cas::put(&data)?;
    let size = data.len() as u64;
    let timestamp_ns = crate::hpet::elapsed_ns();

    // Add to history.
    let mut inner = HISTORY.lock();
    inner.record_count = inner.record_count.saturating_add(1);

    // Insert the version entry.
    // Scope the mutable borrow of inner.files so we can update counters after.
    {
        let fh = inner.files.entry(path.into()).or_insert(FileHistory {
            versions: Vec::new(),
            next_version: 0,
        });

        let version = fh.next_version;
        fh.next_version = fh.next_version.saturating_add(1);

        fh.versions.push(VersionEntry {
            hash,
            size,
            timestamp_ns,
            version,
        });
    }
    inner.total_entries = inner.total_entries.saturating_add(1);

    // Evict oldest versions if over the per-file limit.
    // Collect hashes to release after dropping the borrow on inner.files.
    let mut evicted_hashes: Vec<Hash256> = Vec::new();

    // Per-file eviction: count how many to remove, then update counters.
    let per_file_evicted = {
        let mut count = 0usize;
        if let Some(fh) = inner.files.get_mut(path) {
            while fh.versions.len() > max_per_file && !fh.versions.is_empty() {
                if let Some(old) = fh.versions.first() {
                    evicted_hashes.push(old.hash);
                }
                fh.versions.remove(0);
                count += 1;
            }
        }
        count
    };
    inner.total_entries = inner.total_entries.saturating_sub(per_file_evicted);
    inner.evicted_versions = inner.evicted_versions.saturating_add(per_file_evicted as u64);

    // Global eviction if over total limit.
    while inner.total_entries > max_total {
        // Find the file with the oldest entry.
        let oldest_path: Option<String> = {
            let mut best_path: Option<String> = None;
            let mut best_ts = u64::MAX;
            for (p, fh) in inner.files.iter() {
                if let Some(first) = fh.versions.first() {
                    if first.timestamp_ns < best_ts {
                        best_ts = first.timestamp_ns;
                        best_path = Some(p.clone());
                    }
                }
            }
            best_path
        };

        if let Some(ref op) = oldest_path {
            let (evicted_one, should_remove) = {
                let mut evicted = false;
                let mut empty = false;
                if let Some(fh) = inner.files.get_mut(op.as_str()) {
                    if !fh.versions.is_empty() {
                        if let Some(old) = fh.versions.first() {
                            evicted_hashes.push(old.hash);
                        }
                        fh.versions.remove(0);
                        evicted = true;
                    }
                    empty = fh.versions.is_empty();
                }
                (evicted, empty)
            };

            if evicted_one {
                inner.total_entries = inner.total_entries.saturating_sub(1);
                inner.evicted_versions = inner.evicted_versions.saturating_add(1);
            }
            if should_remove {
                inner.files.remove(op.as_str());
            }
        } else {
            break;
        }
    }

    drop(inner);

    // Release CAS references outside the HISTORY lock.
    for h in &evicted_hashes {
        crate::fs::cas::release(h).ok();
    }

    Ok(Some(hash))
}

/// Get the version history for a file.
///
/// Returns the list of versions, newest last.
/// Returns an empty list if the file has no history.
pub fn get_history(path: &str) -> Vec<VersionEntry> {
    let inner = HISTORY.lock();
    inner.files
        .get(path)
        .map(|fh| fh.versions.clone())
        .unwrap_or_default()
}

/// Get the most recent version of a file from history.
///
/// Returns the CAS hash and metadata, or None if no history exists.
#[allow(dead_code)]
pub fn latest_version(path: &str) -> Option<VersionEntry> {
    let inner = HISTORY.lock();
    inner.files
        .get(path)
        .and_then(|fh| fh.versions.last().cloned())
}

/// Get the content of a specific version from history.
///
/// Retrieves the data from the CAS by hash.
pub fn get_version_data(hash: &Hash256) -> KernelResult<Vec<u8>> {
    crate::fs::cas::get(hash)
}

/// Restore a file to a specific version from its history.
///
/// Writes the version's content back to the file.  Before restoring,
/// records the current content as a new version (so the restore itself
/// is undoable).
pub fn restore_version(path: &str, version_hash: &Hash256) -> KernelResult<()> {
    use crate::fs::Vfs;

    // First, record the current version (if it exists) so restore is undoable.
    record_version(path)?;

    // Get the old version data from CAS.
    let data = crate::fs::cas::get(version_hash)?;

    // Write it back to the file.
    Vfs::write_file(path, &data)?;

    // Update stats.
    let mut inner = HISTORY.lock();
    inner.restore_count = inner.restore_count.saturating_add(1);

    Ok(())
}

/// Clear all history for a specific file.
///
/// Releases all CAS references for that file's versions.
pub fn clear_file(path: &str) {
    let mut inner = HISTORY.lock();
    if let Some(fh) = inner.files.remove(path) {
        for v in &fh.versions {
            crate::fs::cas::release(&v.hash).ok();
        }
        inner.total_entries = inner.total_entries.saturating_sub(fh.versions.len());
    }
}

/// Clear all history for all files.
pub fn clear_all() {
    let mut inner = HISTORY.lock();
    for fh in inner.files.values() {
        for v in &fh.versions {
            crate::fs::cas::release(&v.hash).ok();
        }
    }
    inner.files.clear();
    inner.total_entries = 0;
}

/// Get the number of files being tracked.
pub fn tracked_files() -> usize {
    HISTORY.lock().files.len()
}

/// Get the total number of version entries.
#[allow(dead_code)]
pub fn total_versions() -> usize {
    HISTORY.lock().total_entries
}

/// Get history statistics.
pub fn stats() -> HistoryStats {
    let inner = HISTORY.lock();
    HistoryStats {
        tracked_files: inner.files.len(),
        total_versions: inner.total_entries,
        evicted_versions: inner.evicted_versions,
        record_count: inner.record_count,
        restore_count: inner.restore_count,
        enabled: inner.config.enabled,
        auto_version: inner.config.auto_version,
    }
}

/// List all tracked file paths.
///
/// Returns up to `max` paths, optionally filtered by prefix.
pub fn list_tracked(prefix: Option<&str>, max: usize) -> Vec<(String, usize)> {
    let inner = HISTORY.lock();
    let mut results = Vec::new();

    for (path, fh) in inner.files.iter() {
        if let Some(pfx) = prefix {
            if !path.starts_with(pfx) {
                continue;
            }
        }
        if results.len() >= max {
            break;
        }
        results.push((path.clone(), fh.versions.len()));
    }

    results
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the file version history module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[history] Running self-test...");

    // --- Test 1: record and retrieve version ---
    {
        use crate::fs::Vfs;

        let test_path = "/tmp/_history_test_1";
        let original = b"Original content v1";

        if let Err(e) = Vfs::write_file(test_path, original) {
            serial_println!("[history]   SKIP: cannot write test file: {:?}", e);
            serial_println!("[history] Self-test skipped (no writable filesystem).");
            return Ok(());
        }

        // Record the version.
        let hash = record_version(test_path)?;
        if hash.is_none() {
            serial_println!("[history]   ERROR: record_version returned None");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }
        let hash = hash.unwrap();

        // Verify the stored data matches.
        let stored = get_version_data(&hash)?;
        if stored.as_slice() != original {
            serial_println!("[history]   ERROR: stored data doesn't match original");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   record + retrieve OK");

        // Modify the file and record another version.
        Vfs::write_file(test_path, b"Modified content v2")?;
        let _hash2 = record_version(test_path)?.unwrap();

        // Should now have 2 versions.
        let history = get_history(test_path);
        if history.len() != 2 {
            serial_println!("[history]   ERROR: expected 2 versions, got {}", history.len());
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        // First version should be the original.
        if history.first().map(|v| v.hash) != Some(hash) {
            serial_println!("[history]   ERROR: first version hash mismatch");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   multi-version tracking OK");

        // Cleanup.
        clear_file(test_path);
        Vfs::remove(test_path).ok();
    }

    // --- Test 2: restore version ---
    {
        use crate::fs::Vfs;

        let test_path = "/tmp/_history_test_2";
        let v1_data = b"Version 1 data";
        let v2_data = b"Version 2 data";

        Vfs::write_file(test_path, v1_data)?;
        let v1_hash = record_version(test_path)?.unwrap();

        Vfs::write_file(test_path, v2_data)?;

        // Current content should be v2.
        let current = Vfs::read_file(test_path)?;
        if current.as_slice() != v2_data {
            serial_println!("[history]   ERROR: current data should be v2");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        // Restore to v1.
        restore_version(test_path, &v1_hash)?;

        // Content should now be v1.
        let restored = Vfs::read_file(test_path)?;
        if restored.as_slice() != v1_data {
            serial_println!("[history]   ERROR: restored data should be v1");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   restore OK");

        // Cleanup.
        clear_file(test_path);
        Vfs::remove(test_path).ok();
    }

    // --- Test 3: version eviction (per-file limit) ---
    {
        use crate::fs::Vfs;

        // Temporarily set a small limit.
        let old_config = HISTORY.lock().config.clone();
        {
            let mut inner = HISTORY.lock();
            inner.config.max_versions_per_file = 3;
        }

        let test_path = "/tmp/_history_test_3";

        for i in 0u32..6 {
            let data = alloc::format!("Version {}", i);
            Vfs::write_file(test_path, data.as_bytes())?;
            record_version(test_path)?;
        }

        // Should only have 3 versions (the 3 most recent).
        let history = get_history(test_path);
        if history.len() > 3 {
            serial_println!("[history]   ERROR: expected <= 3 versions after eviction, got {}", history.len());
            // Restore config.
            HISTORY.lock().config = old_config;
            clear_file(test_path);
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   eviction OK (kept {}/6 versions)", history.len());

        // Restore config.
        HISTORY.lock().config = old_config;
        clear_file(test_path);
        Vfs::remove(test_path).ok();
    }

    // --- Test 4: stats ---
    {
        let st = stats();
        if st.record_count < 1 {
            serial_println!("[history]   ERROR: record_count should be >= 1");
            return Err(KernelError::InternalError);
        }
        serial_println!("[history]   stats OK (records: {}, restores: {}, evicted: {})",
            st.record_count, st.restore_count, st.evicted_versions);
    }

    // --- Test 5: clear all ---
    {
        use crate::fs::Vfs;

        let test_path = "/tmp/_history_test_5";
        Vfs::write_file(test_path, b"data")?;
        record_version(test_path)?;

        let before = tracked_files();
        clear_all();

        if tracked_files() != 0 {
            serial_println!("[history]   ERROR: tracked_files should be 0 after clear_all");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   clear_all OK (was {} files)", before);
        Vfs::remove(test_path).ok();
    }

    // --- Test 6: disabled tracking ---
    {
        set_enabled(false);

        let result = record_version("/tmp/nonexistent_disable_test")?;
        if result.is_some() {
            serial_println!("[history]   ERROR: should return None when disabled");
            set_enabled(true);
            return Err(KernelError::InternalError);
        }

        set_enabled(true);
        serial_println!("[history]   disabled tracking OK");
    }

    // --- Test 7: VFS auto-versioning ---
    // Writes to a non-/tmp path should automatically record versions.
    {
        use crate::fs::Vfs;

        // Temporarily ensure auto-versioning is on.
        let old_auto = HISTORY.lock().config.auto_version;
        set_auto_version(true);

        let test_path = "/_history_autoversion_test";
        let v1 = b"Auto-versioned content v1";
        let v2 = b"Auto-versioned content v2";

        // Clear any prior history for this path.
        clear_file(test_path);

        // Write v1 — first write, no prior file to version.
        if let Err(e) = Vfs::write_file(test_path, v1) {
            serial_println!("[history]   SKIP auto-version test: cannot write to root: {:?}", e);
            set_auto_version(old_auto);
        } else {
            // History should be empty (no prior content to save).
            let h1 = get_history(test_path);
            // Write v2 — this should auto-record v1 before overwriting.
            Vfs::write_file(test_path, v2)?;

            let h2 = get_history(test_path);
            let auto_recorded = h2.len().saturating_sub(h1.len());

            if auto_recorded < 1 {
                serial_println!("[history]   ERROR: auto-version did not record previous content");
                clear_file(test_path);
                Vfs::remove(test_path).ok();
                set_auto_version(old_auto);
                return Err(KernelError::InternalError);
            }

            // Verify the auto-recorded version contains v1 data.
            if let Some(entry) = h2.last() {
                match get_version_data(&entry.hash) {
                    Ok(data) if data.as_slice() == v1 => {
                        serial_println!("[history]   auto-version on write OK");
                    }
                    Ok(_) => {
                        serial_println!("[history]   ERROR: auto-recorded data doesn't match v1");
                        clear_file(test_path);
                        Vfs::remove(test_path).ok();
                        set_auto_version(old_auto);
                        return Err(KernelError::InternalError);
                    }
                    Err(e) => {
                        serial_println!("[history]   ERROR: cannot read auto-recorded data: {:?}", e);
                        clear_file(test_path);
                        Vfs::remove(test_path).ok();
                        set_auto_version(old_auto);
                        return Err(KernelError::InternalError);
                    }
                }
            }

            clear_file(test_path);
            Vfs::remove(test_path).ok();
            set_auto_version(old_auto);
        }
    }

    // --- Test 8: should_auto_version path filter ---
    {
        // Virtual filesystems excluded.
        if should_auto_version("/proc/meminfo") {
            serial_println!("[history]   ERROR: /proc/ should be excluded");
            return Err(KernelError::InternalError);
        }
        if should_auto_version("/dev/null") {
            serial_println!("[history]   ERROR: /dev/ should be excluded");
            return Err(KernelError::InternalError);
        }
        if should_auto_version("/sys/kernel/version") {
            serial_println!("[history]   ERROR: /sys/ should be excluded");
            return Err(KernelError::InternalError);
        }
        // Temp files excluded.
        if should_auto_version("/tmp/scratch.txt") {
            serial_println!("[history]   ERROR: /tmp/ should be excluded");
            return Err(KernelError::InternalError);
        }
        // Internal metadata excluded.
        if should_auto_version("/mnt/data/_TRASH/_INDEX") {
            serial_println!("[history]   ERROR: _TRASH/_INDEX should be excluded");
            return Err(KernelError::InternalError);
        }
        if should_auto_version("/mnt/data/_JOURNAL") {
            serial_println!("[history]   ERROR: _JOURNAL should be excluded");
            return Err(KernelError::InternalError);
        }
        // Normal paths included.
        if !should_auto_version("/home/user/document.txt") {
            serial_println!("[history]   ERROR: normal path should be included");
            return Err(KernelError::InternalError);
        }
        if !should_auto_version("/etc/config.yaml") {
            serial_println!("[history]   ERROR: /etc/ path should be included");
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   path filter OK");
    }

    serial_println!("[history] Self-test passed (8 tests).");
    Ok(())
}
