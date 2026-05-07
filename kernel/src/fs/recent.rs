//! Recently accessed files tracking.
//!
//! Tracks files that have been recently opened, modified, or created.
//! Essential for desktop file managers ("Recent Files" view), application
//! launch histories, and document quick-access features.
//!
//! ## Architecture
//!
//! ```text
//! VFS read/write/create
//!   → notify recent::record(path, access_type)
//!   → update in-kernel recent files ring buffer
//!
//! File manager / shell
//!   → recent::query(filter) → sorted recent file list
//! ```
//!
//! ## Features
//!
//! - **Ring buffer** — fixed capacity, oldest entries evicted automatically
//! - **Deduplication** — same file accessed twice updates timestamp, not duplicated
//! - **Category filtering** — filter by access type (open/modify/create)
//! - **Application tracking** — which "application" (kshell command) accessed it
//! - **Configurable retention** — time-based expiry (default: 30 days)
//! - **Exclusions** — ignore temp files, hidden files, system paths
//!
//! ## Design Notes
//!
//! - Maximum tracked entries: 1024 (ring buffer).
//! - Entries older than retention period are lazily evicted on query.
//! - Excluded path prefixes: /tmp, /proc, /sys, /dev (configurable).
//! - Thread-safe via spin::Mutex (low contention — recording is fast).

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum entries in the recent files buffer.
const MAX_ENTRIES: usize = 1024;

/// Default retention period (30 days in nanoseconds).
const DEFAULT_RETENTION_NS: u64 = 30 * 24 * 60 * 60 * 1_000_000_000;

/// Default excluded prefixes.
const DEFAULT_EXCLUDES: &[&str] = &["/tmp", "/proc", "/sys", "/dev"];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Type of file access recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// File was opened/read.
    Open,
    /// File was modified/written.
    Modify,
    /// File was created (new file).
    Create,
    /// File was executed.
    Execute,
}

impl AccessType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Modify => "modify",
            Self::Create => "create",
            Self::Execute => "exec",
        }
    }

    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "open" | "read" | "o" => Some(Self::Open),
            "modify" | "write" | "m" | "w" => Some(Self::Modify),
            "create" | "new" | "c" | "n" => Some(Self::Create),
            "exec" | "execute" | "x" => Some(Self::Execute),
            _ => None,
        }
    }
}

/// A recent file entry.
#[derive(Debug, Clone)]
pub struct RecentEntry {
    /// Full file path.
    pub path: String,
    /// Type of last access.
    pub access_type: AccessType,
    /// Timestamp of last access (nanoseconds since boot).
    pub timestamp_ns: u64,
    /// How many times this file has been accessed (across all types).
    pub access_count: u32,
    /// Application/context that triggered the access.
    pub source: String,
}

/// Filter options for querying recent files.
#[derive(Debug, Clone)]
pub struct RecentFilter {
    /// Filter by access type (None = all types).
    pub access_type: Option<AccessType>,
    /// Maximum number of results.
    pub limit: usize,
    /// Only entries newer than this (nanoseconds, 0 = no minimum).
    pub min_age_ns: u64,
    /// Glob pattern for path matching (empty = all).
    pub pattern: String,
}

impl Default for RecentFilter {
    fn default() -> Self {
        Self {
            access_type: None,
            limit: 50,
            min_age_ns: 0,
            pattern: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Recent files buffer.
static RECENT: spin::Mutex<Vec<RecentEntry>> = spin::Mutex::new(Vec::new());

/// Custom excluded prefixes.
static EXCLUDES: spin::Mutex<Vec<String>> = spin::Mutex::new(Vec::new());

/// Retention period.
static RETENTION_NS: AtomicU64 = AtomicU64::new(DEFAULT_RETENTION_NS);

/// Whether tracking is enabled.
static ENABLED: spin::Mutex<bool> = spin::Mutex::new(true);

/// Statistics.
static RECORD_COUNT: AtomicU64 = AtomicU64::new(0);
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static EVICTED_COUNT: AtomicU64 = AtomicU64::new(0);
static EXCLUDED_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Recording
// ---------------------------------------------------------------------------

/// Record a file access.
///
/// Called by the VFS or kshell when a file is accessed. The entry is
/// deduplicated: if the same path already exists, its timestamp and
/// access type are updated rather than creating a duplicate.
pub fn record(path: &str, access_type: AccessType, source: &str) {
    if !*ENABLED.lock() {
        return;
    }

    // Check exclusions.
    if is_excluded(path) {
        EXCLUDED_COUNT.fetch_add(1, Ordering::Relaxed);
        return;
    }

    RECORD_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    let mut recent = RECENT.lock();

    // Deduplicate: update existing entry.
    for entry in recent.iter_mut() {
        if entry.path == path {
            entry.access_type = access_type;
            entry.timestamp_ns = now;
            entry.access_count = entry.access_count.saturating_add(1);
            if !source.is_empty() {
                entry.source = String::from(source);
            }
            return;
        }
    }

    // New entry — evict oldest if at capacity.
    if recent.len() >= MAX_ENTRIES {
        // Find and remove oldest.
        if let Some(oldest_idx) = recent.iter().enumerate()
            .min_by_key(|(_, e)| e.timestamp_ns)
            .map(|(i, _)| i)
        {
            recent.swap_remove(oldest_idx);
            EVICTED_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    recent.push(RecentEntry {
        path: String::from(path),
        access_type,
        timestamp_ns: now,
        access_count: 1,
        source: String::from(source),
    });
}

// ---------------------------------------------------------------------------
// Public API — Querying
// ---------------------------------------------------------------------------

/// Query recent files with optional filtering.
///
/// Returns entries sorted by timestamp (newest first).
pub fn query(filter: &RecentFilter) -> Vec<RecentEntry> {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();
    let retention = RETENTION_NS.load(Ordering::Relaxed);

    let recent = RECENT.lock();

    let mut results: Vec<RecentEntry> = recent.iter()
        .filter(|e| {
            // Age filter.
            if retention > 0 && now.saturating_sub(e.timestamp_ns) > retention {
                return false;
            }
            // Min age filter.
            if filter.min_age_ns > 0 && now.saturating_sub(e.timestamp_ns) > filter.min_age_ns {
                return false;
            }
            // Type filter.
            if let Some(at) = filter.access_type {
                if e.access_type != at {
                    return false;
                }
            }
            // Pattern filter.
            if !filter.pattern.is_empty() {
                if !simple_glob(&filter.pattern, &e.path) {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();

    // Sort by timestamp, newest first.
    results.sort_by(|a, b| b.timestamp_ns.cmp(&a.timestamp_ns));

    // Apply limit.
    if filter.limit > 0 && results.len() > filter.limit {
        results.truncate(filter.limit);
    }

    results
}

/// Get the N most recently accessed files (convenience wrapper).
pub fn most_recent(n: usize) -> Vec<RecentEntry> {
    query(&RecentFilter { limit: n, ..Default::default() })
}

/// Get total number of tracked entries.
pub fn count() -> usize {
    RECENT.lock().len()
}

// ---------------------------------------------------------------------------
// Public API — Configuration
// ---------------------------------------------------------------------------

/// Enable or disable tracking.
pub fn set_enabled(enabled: bool) {
    *ENABLED.lock() = enabled;
}

/// Check if tracking is enabled.
pub fn is_enabled() -> bool {
    *ENABLED.lock()
}

/// Set retention period in nanoseconds.
pub fn set_retention_ns(ns: u64) {
    RETENTION_NS.store(ns, Ordering::Relaxed);
}

/// Get retention period in nanoseconds.
pub fn get_retention_ns() -> u64 {
    RETENTION_NS.load(Ordering::Relaxed)
}

/// Add a path prefix to the exclusion list.
pub fn add_exclude(prefix: &str) {
    let mut excludes = EXCLUDES.lock();
    if !excludes.iter().any(|e| e == prefix) {
        excludes.push(String::from(prefix));
    }
}

/// Remove a path prefix from the exclusion list.
pub fn remove_exclude(prefix: &str) -> bool {
    let mut excludes = EXCLUDES.lock();
    let len_before = excludes.len();
    excludes.retain(|e| e != prefix);
    excludes.len() < len_before
}

/// List current exclusion prefixes.
pub fn list_excludes() -> Vec<String> {
    EXCLUDES.lock().clone()
}

/// Clear all recent entries.
pub fn clear() {
    RECENT.lock().clear();
}

/// Remove a specific path from recent files.
pub fn remove(path: &str) -> bool {
    let mut recent = RECENT.lock();
    let len_before = recent.len();
    recent.retain(|e| e.path != path);
    recent.len() < len_before
}

/// Expire old entries beyond the retention period.
pub fn expire() -> usize {
    let now = crate::timekeeping::clock_monotonic();
    let retention = RETENTION_NS.load(Ordering::Relaxed);
    if retention == 0 {
        return 0;
    }

    let mut recent = RECENT.lock();
    let len_before = recent.len();
    recent.retain(|e| now.saturating_sub(e.timestamp_ns) <= retention);
    let expired = len_before - recent.len();
    EVICTED_COUNT.fetch_add(expired as u64, Ordering::Relaxed);
    expired
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get tracking statistics.
pub fn stats() -> (u64, u64, u64, u64, usize, bool) {
    let count = RECENT.lock().len();
    let enabled = *ENABLED.lock();
    (
        RECORD_COUNT.load(Ordering::Relaxed),
        QUERY_COUNT.load(Ordering::Relaxed),
        EVICTED_COUNT.load(Ordering::Relaxed),
        EXCLUDED_COUNT.load(Ordering::Relaxed),
        count,
        enabled,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    RECORD_COUNT.store(0, Ordering::Relaxed);
    QUERY_COUNT.store(0, Ordering::Relaxed);
    EVICTED_COUNT.store(0, Ordering::Relaxed);
    EXCLUDED_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if a path should be excluded from tracking.
fn is_excluded(path: &str) -> bool {
    // Check default excludes.
    for prefix in DEFAULT_EXCLUDES {
        if path.starts_with(prefix) {
            return true;
        }
    }
    // Check custom excludes.
    let excludes = EXCLUDES.lock();
    for prefix in excludes.iter() {
        if path.starts_with(prefix.as_str()) {
            return true;
        }
    }
    false
}

/// Simple glob matching (supports * and ?).
fn simple_glob(pattern: &str, text: &str) -> bool {
    let pat = pattern.as_bytes();
    let txt = text.as_bytes();

    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < txt.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }

    pi == pat.len()
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[recent] Running self-test...");

    test_record_and_query();
    test_deduplication();
    test_type_filter();
    test_exclusions();
    test_capacity();
    test_removal();

    serial_println!("[recent] Self-test passed (6 tests).");
    Ok(())
}

fn test_record_and_query() {
    clear();

    record("/home/user/doc.txt", AccessType::Open, "cat");
    record("/home/user/pic.png", AccessType::Modify, "editor");
    record("/home/user/code.rs", AccessType::Create, "touch");

    let results = most_recent(10);
    assert_eq!(results.len(), 3);

    // Should be newest first.
    assert_eq!(results[0].path, "/home/user/code.rs");

    clear();
    serial_println!("[recent]   record_and_query: ok");
}

fn test_deduplication() {
    clear();

    record("/home/user/file.txt", AccessType::Open, "cat");
    record("/home/user/file.txt", AccessType::Modify, "vim");

    // Should be one entry with updated type and count.
    let results = most_recent(10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].access_type, AccessType::Modify);
    assert_eq!(results[0].access_count, 2);

    clear();
    serial_println!("[recent]   deduplication: ok");
}

fn test_type_filter() {
    clear();

    record("/a.txt", AccessType::Open, "");
    record("/b.txt", AccessType::Modify, "");
    record("/c.txt", AccessType::Create, "");

    let filter = RecentFilter {
        access_type: Some(AccessType::Modify),
        ..Default::default()
    };
    let results = query(&filter);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "/b.txt");

    clear();
    serial_println!("[recent]   type_filter: ok");
}

fn test_exclusions() {
    clear();

    // Default excludes: /tmp, /proc, /sys, /dev
    record("/tmp/scratch", AccessType::Open, "");
    record("/proc/cpuinfo", AccessType::Open, "");
    record("/home/user/real.txt", AccessType::Open, "");

    let results = most_recent(10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "/home/user/real.txt");

    // Custom exclude.
    add_exclude("/var/log");
    record("/var/log/syslog", AccessType::Open, "");
    let results2 = most_recent(10);
    assert_eq!(results2.len(), 1); // Still just real.txt.

    remove_exclude("/var/log");
    clear();
    serial_println!("[recent]   exclusions: ok");
}

fn test_capacity() {
    clear();

    // Fill to capacity.
    for i in 0..MAX_ENTRIES + 10 {
        record(
            &alloc::format!("/cap/file_{}", i),
            AccessType::Open,
            "",
        );
    }

    // Should not exceed max.
    assert!(count() <= MAX_ENTRIES);

    clear();
    serial_println!("[recent]   capacity: ok");
}

fn test_removal() {
    clear();

    record("/remove/a.txt", AccessType::Open, "");
    record("/remove/b.txt", AccessType::Open, "");

    assert_eq!(count(), 2);
    assert!(remove("/remove/a.txt"));
    assert_eq!(count(), 1);
    assert!(!remove("/remove/nonexistent"));

    clear();
    serial_println!("[recent]   removal: ok");
}
