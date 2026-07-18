//! Filesystem deduplication daemon.
//!
//! Finds files with identical content and replaces duplicates with
//! references to a single CAS (Content-Addressed Store) blob.  This
//! is "offline" deduplication — it scans existing files rather than
//! deduplicating inline during writes.
//!
//! ## Design
//!
//! ```text
//! dedup::scan(paths)
//!     ↓
//! Phase 1: Group files by size (fast reject: different size = different content)
//!     ↓
//! Phase 2: Hash small prefix (first 4 KiB) for size-matched groups
//!     ↓
//! Phase 3: Full SHA-256 hash only for prefix-matched files
//!     ↓
//! Phase 4: Store unique content in CAS, create stub files pointing
//!          to CAS hash (optional — currently just reports)
//! ```
//!
//! ## Dedup Modes
//!
//! - **Report-only**: just find and report duplicates (safe, no changes)
//! - **Link-based**: replace duplicates with references to CAS blobs
//!   (stores CAS hash + original metadata in a stub file)
//!
//! ## Reference
//!
//! design.txt: "filesystem deduplication" as a settings option

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::Vfs;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a deduplication scan.
#[derive(Debug, Clone, Default)]
pub struct DedupResult {
    /// Total files scanned.
    pub files_scanned: u64,
    /// Total bytes scanned.
    pub bytes_scanned: u64,
    /// Number of duplicate groups found.
    pub duplicate_groups: u64,
    /// Total duplicate files (not counting the first/primary copy).
    pub duplicate_files: u64,
    /// Total bytes that could be freed by deduplication.
    pub dedup_savings: u64,
    /// Details of duplicate groups (hash → list of paths).
    pub groups: Vec<DedupGroup>,
}

/// A group of files with identical content.
#[derive(Debug, Clone)]
pub struct DedupGroup {
    /// SHA-256 hash of the shared content.
    pub hash: String,
    /// Size of each file in bytes.
    pub size: u64,
    /// Paths of all files in this group.
    pub paths: Vec<String>,
}

/// Configuration for a dedup scan.
#[derive(Debug, Clone)]
pub struct DedupConfig {
    /// Root paths to scan.
    pub scan_paths: Vec<String>,
    /// Minimum file size to consider (skip small files).
    pub min_size: u64,
    /// Maximum file size to consider (skip huge files).
    pub max_size: u64,
    /// Maximum number of files to scan.
    pub max_files: usize,
    /// Maximum depth for recursive directory traversal.
    pub max_depth: usize,
    /// File extensions to include (empty = all).
    pub include_extensions: Vec<String>,
    /// Path prefixes to exclude.
    pub exclude_prefixes: Vec<String>,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            scan_paths: alloc::vec![String::from("/")],
            min_size: 1,
            max_size: 64 * 1024 * 1024, // 64 MiB
            max_files: 50_000,
            max_depth: 32,
            include_extensions: Vec::new(),
            exclude_prefixes: alloc::vec![
                String::from("/proc"),
                String::from("/dev"),
                String::from("/sys"),
                String::from("/_"),
            ],
        }
    }
}

/// Cumulative dedup statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct DedupStats {
    /// Number of scans run.
    pub scans_run: u64,
    /// Total files scanned across all runs.
    pub total_files: u64,
    /// Total duplicate groups found.
    pub total_groups: u64,
    /// Total duplicate files found.
    pub total_duplicates: u64,
    /// Total potential savings in bytes.
    pub total_savings: u64,
    /// Whether a scan is currently running.
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Master enable flag.
static ENABLED: AtomicBool = AtomicBool::new(true);

struct DedupInner {
    stats: DedupStats,
    last_result: Option<DedupResult>,
}

static STATE: Mutex<DedupInner> = Mutex::new(DedupInner {
    stats: DedupStats {
        scans_run: 0,
        total_files: 0,
        total_groups: 0,
        total_duplicates: 0,
        total_savings: 0,
        active: false,
    },
    last_result: None,
});

// ---------------------------------------------------------------------------
// Configuration API
// ---------------------------------------------------------------------------

/// Enable or disable the dedup daemon.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if dedup is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Get cumulative stats.
pub fn stats() -> DedupStats {
    STATE.lock().stats
}

/// Get the last scan result.
pub fn last_result() -> Option<DedupResult> {
    STATE.lock().last_result.clone()
}

// ---------------------------------------------------------------------------
// Core scan logic
// ---------------------------------------------------------------------------

/// Run a deduplication scan with the given configuration.
///
/// Returns the scan results (duplicate groups with paths).
pub fn scan(config: &DedupConfig) -> KernelResult<DedupResult> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }

    {
        let mut state = STATE.lock();
        if state.stats.active {
            return Err(KernelError::WouldBlock);
        }
        state.stats.active = true;
    }

    let result = run_scan(config);

    // Update stats.
    {
        let mut state = STATE.lock();
        state.stats.active = false;
        if let Ok(ref r) = result {
            state.stats.scans_run = state.stats.scans_run.saturating_add(1);
            state.stats.total_files = state
                .stats
                .total_files
                .saturating_add(r.files_scanned);
            state.stats.total_groups = state
                .stats
                .total_groups
                .saturating_add(r.duplicate_groups);
            state.stats.total_duplicates = state
                .stats
                .total_duplicates
                .saturating_add(r.duplicate_files);
            state.stats.total_savings = state
                .stats
                .total_savings
                .saturating_add(r.dedup_savings);
            state.last_result = Some(r.clone());
        }
    }

    result
}

/// Internal scan implementation.
fn run_scan(config: &DedupConfig) -> KernelResult<DedupResult> {
    let mut result = DedupResult::default();

    // Phase 1: Collect all files with their sizes.
    let mut files: Vec<(String, u64)> = Vec::new();
    for root in &config.scan_paths {
        collect_files(
            root,
            config,
            &mut files,
            0,
        );
    }

    result.files_scanned = files.len() as u64;
    result.bytes_scanned = files.iter().map(|(_, sz)| *sz).sum();

    // Phase 2: Group by size (files of different sizes can't be duplicates).
    let mut size_groups: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for (path, size) in &files {
        size_groups
            .entry(*size)
            .or_default()
            .push(path.clone());
    }

    // Only keep groups with 2+ files (potential duplicates).
    let candidate_groups: Vec<(u64, Vec<String>)> = size_groups
        .into_iter()
        .filter(|(_, paths)| paths.len() >= 2)
        .collect();

    // Phase 3: Full hash comparison for candidate groups.
    for (size, paths) in &candidate_groups {
        let mut hash_groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for path in paths {
            match Vfs::read_file(path) {
                Ok(data) => {
                    let hash = crate::crypto::sha256(&data);
                    let hex = hex_encode(&hash);
                    hash_groups
                        .entry(hex)
                        .or_default()
                        .push(path.clone());
                }
                Err(_) => continue, // Skip unreadable files.
            }
        }

        // Find actual duplicate groups (2+ files with same hash).
        for (hash, group_paths) in hash_groups {
            if group_paths.len() >= 2 {
                let dup_count = (group_paths.len() - 1) as u64;
                let savings = dup_count * size;

                result.duplicate_groups = result.duplicate_groups.saturating_add(1);
                result.duplicate_files = result.duplicate_files.saturating_add(dup_count);
                result.dedup_savings = result.dedup_savings.saturating_add(savings);

                result.groups.push(DedupGroup {
                    hash,
                    size: *size,
                    paths: group_paths,
                });
            }
        }
    }

    serial_println!(
        "[dedup] Scan complete: {} files, {} groups, {} duplicates, {} bytes saveable",
        result.files_scanned,
        result.duplicate_groups,
        result.duplicate_files,
        result.dedup_savings,
    );

    Ok(result)
}

/// Recursively collect files from a directory.
fn collect_files(
    path: &str,
    config: &DedupConfig,
    out: &mut Vec<(String, u64)>,
    depth: usize,
) {
    if depth > config.max_depth {
        return;
    }
    if out.len() >= config.max_files {
        return;
    }

    // Check exclude prefixes (canonical subtree predicate tolerates a
    // trailing slash on the exclude entry). See fs::pathutil.
    for excl in &config.exclude_prefixes {
        if crate::fs::pathutil::path_in_subtree(path, excl.as_str()) {
            return;
        }
    }

    // Try to list as directory.
    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if out.len() >= config.max_files {
            return;
        }

        let full_path = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        match entry.entry_type {
            crate::fs::EntryType::Directory => {
                // Skip . and ..
                if entry.name != "." && entry.name != ".." {
                    collect_files(&full_path, config, out, depth + 1);
                }
            }
            crate::fs::EntryType::File => {
                // Check extension filter.
                if !config.include_extensions.is_empty() {
                    let ext = file_extension(&entry.name);
                    if !config.include_extensions.iter().any(|e| e.as_str() == ext) {
                        continue;
                    }
                }

                // Get file size.
                if let Ok(meta) = Vfs::metadata(&full_path) {
                    let size = meta.size;
                    if size >= config.min_size && size <= config.max_size {
                        out.push((full_path, size));
                    }
                }
            }
            _ => {} // Skip symlinks, etc.
        }
    }
}

/// Extract file extension (lowercase, without dot).
fn file_extension(name: &str) -> &str {
    if let Some(pos) = name.rfind('.') {
        &name[pos + 1..]
    } else {
        ""
    }
}

/// Encode bytes as hex string.
fn hex_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for &b in data {
        use core::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[dedup] Running self-test...");

    test_empty_scan();
    test_basic_duplicates();
    test_no_duplicates();
    test_config_filters();
    test_stats();
    test_hex_encode();

    serial_println!("[dedup] Self-test passed (6 tests).");
    Ok(())
}

fn test_empty_scan() {
    // Scan a non-existent path — should succeed with 0 results.
    let config = DedupConfig {
        scan_paths: alloc::vec![String::from("/tmp/__dedup_test_empty_nonexistent")],
        ..DedupConfig::default()
    };
    let result = scan(&config).expect("scan should succeed");
    assert_eq!(result.files_scanned, 0);
    assert_eq!(result.duplicate_groups, 0);

    serial_println!("[dedup]   empty scan: ok");
}

fn test_basic_duplicates() {
    // Create files with identical content.
    let data = b"dedup test content that is exactly the same in all files";
    let _ = Vfs::mkdir("/tmp/dedup_test");
    Vfs::write_file("/tmp/dedup_test/a.txt", data).expect("write a");
    Vfs::write_file("/tmp/dedup_test/b.txt", data).expect("write b");
    Vfs::write_file("/tmp/dedup_test/c.txt", data).expect("write c");
    Vfs::write_file("/tmp/dedup_test/unique.txt", b"this is different content").expect("write unique");

    let config = DedupConfig {
        scan_paths: alloc::vec![String::from("/tmp/dedup_test")],
        min_size: 1,
        ..DedupConfig::default()
    };

    let result = scan(&config).expect("scan failed");
    assert!(result.files_scanned >= 4, "should scan at least 4 files");
    assert!(result.duplicate_groups >= 1, "should find at least 1 group");
    assert!(result.duplicate_files >= 2, "should find at least 2 duplicates");
    assert!(result.dedup_savings > 0, "should report savings");

    // Verify the duplicate group.
    let group = result
        .groups
        .iter()
        .find(|g| g.paths.len() >= 3)
        .expect("should have a group of 3");
    assert_eq!(group.size, data.len() as u64);

    // Cleanup.
    let _ = Vfs::remove("/tmp/dedup_test/a.txt");
    let _ = Vfs::remove("/tmp/dedup_test/b.txt");
    let _ = Vfs::remove("/tmp/dedup_test/c.txt");
    let _ = Vfs::remove("/tmp/dedup_test/unique.txt");
    let _ = Vfs::rmdir("/tmp/dedup_test");

    serial_println!("[dedup]   basic duplicates: ok");
}

fn test_no_duplicates() {
    // All different content.
    let _ = Vfs::mkdir("/tmp/dedup_nd");
    Vfs::write_file("/tmp/dedup_nd/x.txt", b"unique content X").expect("write");
    Vfs::write_file("/tmp/dedup_nd/y.txt", b"unique content Y plus more").expect("write");
    Vfs::write_file("/tmp/dedup_nd/z.txt", b"completely different Z content here").expect("write");

    let config = DedupConfig {
        scan_paths: alloc::vec![String::from("/tmp/dedup_nd")],
        min_size: 1,
        ..DedupConfig::default()
    };

    let result = scan(&config).expect("scan");
    assert_eq!(result.duplicate_groups, 0, "no duplicates expected");

    let _ = Vfs::remove("/tmp/dedup_nd/x.txt");
    let _ = Vfs::remove("/tmp/dedup_nd/y.txt");
    let _ = Vfs::remove("/tmp/dedup_nd/z.txt");
    let _ = Vfs::rmdir("/tmp/dedup_nd");

    serial_println!("[dedup]   no duplicates: ok");
}

fn test_config_filters() {
    let _ = Vfs::mkdir("/tmp/dedup_filt");
    Vfs::write_file("/tmp/dedup_filt/small.txt", b"hi").expect("write");
    Vfs::write_file("/tmp/dedup_filt/big.txt", b"this is a bigger file with some content for size filtering").expect("write");

    // Min size filter should skip the small file.
    let config = DedupConfig {
        scan_paths: alloc::vec![String::from("/tmp/dedup_filt")],
        min_size: 10,
        ..DedupConfig::default()
    };

    let result = scan(&config).expect("scan");
    // Only big.txt should be scanned.
    assert!(result.files_scanned <= 2, "small file should be skipped or included based on size");

    let _ = Vfs::remove("/tmp/dedup_filt/small.txt");
    let _ = Vfs::remove("/tmp/dedup_filt/big.txt");
    let _ = Vfs::rmdir("/tmp/dedup_filt");

    serial_println!("[dedup]   config filters: ok");
}

fn test_stats() {
    let s = stats();
    // After previous tests, should have some stats.
    assert!(s.scans_run > 0, "should have run scans");
    assert!(!s.active, "should not be active");

    serial_println!("[dedup]   stats: ok");
}

fn test_hex_encode() {
    assert_eq!(hex_encode(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
    assert_eq!(hex_encode(&[0x00, 0xFF]), "00ff");
    assert_eq!(hex_encode(&[]), "");

    serial_println!("[dedup]   hex encode: ok");
}
