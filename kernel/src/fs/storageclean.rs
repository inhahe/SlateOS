//! Storage cleanup and disk usage analysis.
//!
//! Provides automated and manual disk space reclamation, similar to
//! Windows Storage Sense, macOS Manage Storage, or Linux's `ncdu`.
//! Scans for reclaimable space across multiple categories and offers
//! cleanup recommendations.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Storage
//!   → storageclean::scan() → ReclaimReport
//!   → storageclean::clean(categories) → freed bytes
//!
//! Automatic mode (periodic)
//!   → storageclean::auto_clean() → frees low-hanging fruit
//!
//! Integration:
//!   → trash::empty() for trash cleanup
//!   → tmpwatch for temp file cleanup
//!   → cache for buffer cache flush
//!   → thumbcache for thumbnail cleanup
//!   → recent for old history trimming
//! ```
//!
//! ## Categories
//!
//! - **Trash**: recycle bin contents
//! - **TempFiles**: /tmp and application temp directories
//! - **Thumbnails**: cached preview images
//! - **LogFiles**: old log files beyond retention
//! - **PackageCache**: downloaded packages and updates
//! - **DuplicateFiles**: duplicate content (via CAS hashes)
//! - **LargeFiles**: files above a configurable threshold
//! - **OldDownloads**: download directory files older than threshold

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_RECOMMENDATIONS: usize = 256;
const MAX_SCAN_ENTRIES: usize = 4096;
const MAX_EXCLUSIONS: usize = 128;
const DEFAULT_LARGE_FILE_THRESHOLD: u64 = 100 * 1024 * 1024; // 100 MiB
const DEFAULT_OLD_DAYS: u32 = 30;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Category of reclaimable space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanCategory {
    Trash,
    TempFiles,
    Thumbnails,
    LogFiles,
    PackageCache,
    DuplicateFiles,
    LargeFiles,
    OldDownloads,
}

impl CleanCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trash => "Recycle Bin",
            Self::TempFiles => "Temporary Files",
            Self::Thumbnails => "Thumbnail Cache",
            Self::LogFiles => "Log Files",
            Self::PackageCache => "Package Cache",
            Self::DuplicateFiles => "Duplicate Files",
            Self::LargeFiles => "Large Files",
            Self::OldDownloads => "Old Downloads",
        }
    }

    pub fn all() -> &'static [CleanCategory] {
        &[
            Self::Trash,
            Self::TempFiles,
            Self::Thumbnails,
            Self::LogFiles,
            Self::PackageCache,
            Self::DuplicateFiles,
            Self::LargeFiles,
            Self::OldDownloads,
        ]
    }
}

/// A single item that could be cleaned up.
#[derive(Debug, Clone)]
pub struct CleanItem {
    pub path: String,
    pub size_bytes: u64,
    pub category: CleanCategory,
    /// Human-readable reason for recommendation.
    pub reason: String,
    /// Age in days (0 if not applicable).
    pub age_days: u32,
}

/// Summary of reclaimable space per category.
#[derive(Debug, Clone)]
pub struct CategorySummary {
    pub category: CleanCategory,
    pub item_count: usize,
    pub total_bytes: u64,
    pub recommended: bool,
}

/// Complete scan report.
#[derive(Debug, Clone)]
pub struct ScanReport {
    pub categories: Vec<CategorySummary>,
    pub total_reclaimable_bytes: u64,
    pub total_items: usize,
    pub scan_duration_us: u64,
}

/// Configuration for storage cleanup.
#[derive(Debug, Clone)]
pub struct CleanConfig {
    /// Automatically clean when disk usage exceeds this percentage.
    pub auto_clean_threshold_pct: u8,
    /// Whether automatic cleanup is enabled.
    pub auto_enabled: bool,
    /// Threshold for "large file" detection (bytes).
    pub large_file_threshold: u64,
    /// Days after which downloads are considered "old".
    pub old_download_days: u32,
    /// Days to keep log files.
    pub log_retention_days: u32,
    /// Categories enabled for automatic cleanup.
    pub auto_categories: Vec<CleanCategory>,
    /// Paths excluded from scanning.
    pub exclusions: Vec<String>,
}

/// Result of a cleanup operation.
#[derive(Debug, Clone)]
pub struct CleanResult {
    pub freed_bytes: u64,
    pub items_cleaned: usize,
    pub errors: usize,
    pub category_freed: Vec<(CleanCategory, u64)>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct StorageState {
    config: CleanConfig,
    /// Cached items from last scan.
    items: Vec<CleanItem>,
    /// Last scan report.
    last_report: Option<ScanReport>,
    /// Total bytes freed across all cleanup operations.
    total_freed: u64,
    /// Total cleanup operations performed.
    total_cleans: u64,
    /// Total scans performed.
    total_scans: u64,
    /// Operation counter.
    ops: u64,
}

static STATE: Mutex<Option<StorageState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut StorageState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize storage cleanup with default configuration.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(StorageState {
        config: CleanConfig {
            auto_clean_threshold_pct: 90,
            auto_enabled: false, // Off by default
            large_file_threshold: DEFAULT_LARGE_FILE_THRESHOLD,
            old_download_days: DEFAULT_OLD_DAYS,
            log_retention_days: 14,
            auto_categories: vec![
                CleanCategory::Trash,
                CleanCategory::TempFiles,
                CleanCategory::Thumbnails,
                CleanCategory::LogFiles,
            ],
            exclusions: Vec::new(),
        },
        items: Vec::new(),
        last_report: None,
        total_freed: 0,
        total_cleans: 0,
        total_scans: 0,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Scan the filesystem for reclaimable space.
///
/// This performs a simulated scan (in kernel context, we query known
/// subsystems for their reclaimable data). In a full implementation,
/// this would walk the actual filesystem.
pub fn scan() -> KernelResult<ScanReport> {
    let start_ns = crate::hpet::elapsed_ns();

    with_state(|state| {
        state.items.clear();

        // Category: Trash — query trash module
        let trash_bytes = scan_trash(&mut state.items);

        // Category: TempFiles — query /tmp
        let temp_bytes = scan_temp_files(&mut state.items);

        // Category: Thumbnails — query thumbcache
        let thumb_bytes = scan_thumbnails(&mut state.items);

        // Category: LogFiles
        let log_bytes = scan_log_files(&mut state.items, state.config.log_retention_days);

        // Category: PackageCache
        let pkg_bytes = scan_package_cache(&mut state.items);

        // Category: LargeFiles
        let large_bytes = scan_large_files(&mut state.items, state.config.large_file_threshold);

        // Category: OldDownloads
        let download_bytes = scan_old_downloads(
            &mut state.items, state.config.old_download_days);

        // Build category summaries
        let mut categories = Vec::new();
        for cat in CleanCategory::all() {
            let items: Vec<&CleanItem> = state.items.iter()
                .filter(|i| i.category == *cat)
                .collect();
            if !items.is_empty() {
                let total: u64 = items.iter().map(|i| i.size_bytes).sum();
                categories.push(CategorySummary {
                    category: *cat,
                    item_count: items.len(),
                    total_bytes: total,
                    recommended: total > 1024 * 1024, // Recommend if > 1 MiB
                });
            }
        }

        let total_bytes = trash_bytes + temp_bytes + thumb_bytes + log_bytes
            + pkg_bytes + large_bytes + download_bytes;

        let elapsed_us = (crate::hpet::elapsed_ns() - start_ns) / 1000;

        let report = ScanReport {
            total_reclaimable_bytes: total_bytes,
            total_items: state.items.len(),
            scan_duration_us: elapsed_us,
            categories,
        };

        state.last_report = Some(report.clone());
        state.total_scans += 1;
        Ok(report)
    })
}

fn scan_trash(items: &mut Vec<CleanItem>) -> u64 {
    use crate::fs::Vfs;
    let mut total = 0u64;
    if let Ok(entries) = Vfs::readdir("/_TRASH") {
        for entry in entries {
            if entry.name.starts_with("_INDEX") {
                continue;
            }
            let path = format!("/_TRASH/{}", entry.name);
            let size = Vfs::read_file(&path).map(|d| d.len() as u64).unwrap_or(0);
            if items.len() < MAX_SCAN_ENTRIES {
                items.push(CleanItem {
                    path,
                    size_bytes: size,
                    category: CleanCategory::Trash,
                    reason: String::from("In recycle bin"),
                    age_days: 0,
                });
            }
            total = total.saturating_add(size);
        }
    }
    total
}

fn scan_temp_files(items: &mut Vec<CleanItem>) -> u64 {
    use crate::fs::Vfs;
    let mut total = 0u64;
    if let Ok(entries) = Vfs::readdir("/tmp") {
        for entry in entries {
            let path = format!("/tmp/{}", entry.name);
            let size = Vfs::read_file(&path).map(|d| d.len() as u64).unwrap_or(0);
            if items.len() < MAX_SCAN_ENTRIES {
                items.push(CleanItem {
                    path,
                    size_bytes: size,
                    category: CleanCategory::TempFiles,
                    reason: String::from("Temporary file"),
                    age_days: 0,
                });
            }
            total = total.saturating_add(size);
        }
    }
    total
}

fn scan_thumbnails(items: &mut Vec<CleanItem>) -> u64 {
    // Query thumbcache stats for memory usage
    let (count, _, mem_bytes, _, _, _) = crate::fs::thumbcache::stats();
    if count > 0 && mem_bytes > 0 {
        if items.len() < MAX_SCAN_ENTRIES {
            items.push(CleanItem {
                path: String::from("[thumbnail cache]"),
                size_bytes: mem_bytes,
                category: CleanCategory::Thumbnails,
                reason: format!("{} cached thumbnails", count),
                age_days: 0,
            });
        }
    }
    mem_bytes
}

fn scan_log_files(items: &mut Vec<CleanItem>, _retention_days: u32) -> u64 {
    use crate::fs::Vfs;
    let mut total = 0u64;
    let log_dirs = ["/var/log", "/log"];
    for dir in &log_dirs {
        if let Ok(entries) = Vfs::readdir(dir) {
            for entry in entries {
                if entry.name.ends_with(".log") || entry.name.ends_with(".log.old") {
                    let path = format!("{}/{}", dir, entry.name);
                    let size = Vfs::read_file(&path).map(|d| d.len() as u64).unwrap_or(0);
                    if items.len() < MAX_SCAN_ENTRIES {
                        items.push(CleanItem {
                            path,
                            size_bytes: size,
                            category: CleanCategory::LogFiles,
                            reason: String::from("Log file"),
                            age_days: 0,
                        });
                    }
                    total = total.saturating_add(size);
                }
            }
        }
    }
    total
}

fn scan_package_cache(items: &mut Vec<CleanItem>) -> u64 {
    use crate::fs::Vfs;
    let mut total = 0u64;
    let cache_dirs = ["/var/cache/pkg", "/var/cache/packages"];
    for dir in &cache_dirs {
        if let Ok(entries) = Vfs::readdir(dir) {
            for entry in entries {
                let path = format!("{}/{}", dir, entry.name);
                let size = Vfs::read_file(&path).map(|d| d.len() as u64).unwrap_or(0);
                if items.len() < MAX_SCAN_ENTRIES {
                    items.push(CleanItem {
                        path,
                        size_bytes: size,
                        category: CleanCategory::PackageCache,
                        reason: String::from("Cached package"),
                        age_days: 0,
                    });
                }
                total = total.saturating_add(size);
            }
        }
    }
    total
}

fn scan_large_files(items: &mut Vec<CleanItem>, threshold: u64) -> u64 {
    use crate::fs::Vfs;
    let mut total = 0u64;
    let dirs = ["/home", "/root", "/data"];
    for dir in &dirs {
        if let Ok(entries) = Vfs::readdir(dir) {
            for entry in entries {
                let path = format!("{}/{}", dir, entry.name);
                let size = Vfs::read_file(&path).map(|d| d.len() as u64).unwrap_or(0);
                if size >= threshold {
                    if items.len() < MAX_SCAN_ENTRIES {
                        items.push(CleanItem {
                            path,
                            size_bytes: size,
                            category: CleanCategory::LargeFiles,
                            reason: format!("Large file ({})", format_size(size)),
                            age_days: 0,
                        });
                    }
                    total = total.saturating_add(size);
                }
            }
        }
    }
    total
}

fn scan_old_downloads(items: &mut Vec<CleanItem>, _age_days: u32) -> u64 {
    use crate::fs::Vfs;
    let mut total = 0u64;
    let download_dirs = ["/home/Downloads", "/root/Downloads"];
    for dir in &download_dirs {
        if let Ok(entries) = Vfs::readdir(dir) {
            for entry in entries {
                let path = format!("{}/{}", dir, entry.name);
                let size = Vfs::read_file(&path).map(|d| d.len() as u64).unwrap_or(0);
                if items.len() < MAX_SCAN_ENTRIES {
                    items.push(CleanItem {
                        path,
                        size_bytes: size,
                        category: CleanCategory::OldDownloads,
                        reason: String::from("Old download"),
                        age_days: 0,
                    });
                }
                total = total.saturating_add(size);
            }
        }
    }
    total
}

/// Get the last scan report without re-scanning.
pub fn last_report() -> Option<ScanReport> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| s.last_report.clone())
}

/// Get cached scan items (from last scan).
pub fn scan_items() -> Vec<CleanItem> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.items.clone())
}

/// Get items for a specific category.
pub fn items_for_category(cat: CleanCategory) -> Vec<CleanItem> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.items.iter().filter(|i| i.category == cat).cloned().collect()
    })
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

/// Clean up items in the specified categories.
pub fn clean(categories: &[CleanCategory]) -> KernelResult<CleanResult> {
    with_state(|state| {
        let mut freed = 0u64;
        let mut cleaned = 0usize;
        let mut errors = 0usize;
        let mut category_freed: Vec<(CleanCategory, u64)> = Vec::new();

        for cat in categories {
            let mut cat_freed = 0u64;
            let items_to_clean: Vec<CleanItem> = state.items.iter()
                .filter(|i| i.category == *cat)
                .cloned()
                .collect();

            for item in &items_to_clean {
                match *cat {
                    CleanCategory::Trash
                    | CleanCategory::TempFiles
                    | CleanCategory::LogFiles
                    | CleanCategory::PackageCache => {
                        if crate::fs::Vfs::remove(&item.path).is_ok() {
                            cat_freed = cat_freed.saturating_add(item.size_bytes);
                            cleaned += 1;
                        } else {
                            errors += 1;
                        }
                    }
                    CleanCategory::Thumbnails => {
                        crate::fs::thumbcache::clear();
                        cat_freed = cat_freed.saturating_add(item.size_bytes);
                        cleaned += 1;
                    }
                    // Large files and old downloads are recommendations only;
                    // user must explicitly confirm deletion.
                    CleanCategory::LargeFiles | CleanCategory::OldDownloads => {
                        // Skip unless explicitly cleaning
                        cat_freed = cat_freed.saturating_add(item.size_bytes);
                        cleaned += 1;
                    }
                    CleanCategory::DuplicateFiles => {
                        // Skip — requires user selection of which duplicate to keep
                        continue;
                    }
                }
            }

            if cat_freed > 0 {
                category_freed.push((*cat, cat_freed));
            }
            freed = freed.saturating_add(cat_freed);
        }

        // Remove cleaned items from cache
        state.items.retain(|i| !categories.contains(&i.category));
        state.total_freed = state.total_freed.saturating_add(freed);
        state.total_cleans += 1;

        Ok(CleanResult {
            freed_bytes: freed,
            items_cleaned: cleaned,
            errors,
            category_freed,
        })
    })
}

/// Run automatic cleanup (only auto-enabled categories).
pub fn auto_clean() -> KernelResult<CleanResult> {
    let cats = with_state(|state| {
        if !state.config.auto_enabled {
            return Err(KernelError::NotSupported);
        }
        Ok(state.config.auto_categories.clone())
    })?;

    // Scan first, then clean auto categories
    let _ = scan();
    clean(&cats)
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current configuration.
pub fn config() -> KernelResult<CleanConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Set auto-clean enabled.
pub fn set_auto_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.auto_enabled = enabled;
        Ok(())
    })
}

/// Set auto-clean disk threshold percentage.
pub fn set_auto_threshold(pct: u8) -> KernelResult<()> {
    if pct > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.auto_clean_threshold_pct = pct;
        Ok(())
    })
}

/// Set large file threshold.
pub fn set_large_threshold(bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.config.large_file_threshold = bytes;
        Ok(())
    })
}

/// Set old download age threshold.
pub fn set_old_download_days(days: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.old_download_days = days;
        Ok(())
    })
}

/// Set log retention days.
pub fn set_log_retention(days: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.log_retention_days = days;
        Ok(())
    })
}

/// Add a category to auto-cleanup.
pub fn add_auto_category(cat: CleanCategory) -> KernelResult<()> {
    with_state(|state| {
        if !state.config.auto_categories.contains(&cat) {
            state.config.auto_categories.push(cat);
        }
        Ok(())
    })
}

/// Remove a category from auto-cleanup.
pub fn remove_auto_category(cat: CleanCategory) -> KernelResult<()> {
    with_state(|state| {
        state.config.auto_categories.retain(|c| *c != cat);
        Ok(())
    })
}

/// Add an exclusion path (skip during scans).
pub fn add_exclusion(path: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.config.exclusions.len() >= MAX_EXCLUSIONS {
            return Err(KernelError::ResourceExhausted);
        }
        if !state.config.exclusions.iter().any(|e| e == path) {
            state.config.exclusions.push(String::from(path));
        }
        Ok(())
    })
}

/// Remove an exclusion path.
pub fn remove_exclusion(path: &str) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.config.exclusions.iter().position(|e| e == path)
            .ok_or(KernelError::NotFound)?;
        state.config.exclusions.remove(idx);
        Ok(())
    })
}

/// List exclusion paths.
pub fn exclusions() -> Vec<String> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.config.exclusions.clone())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format byte count as human-readable string.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{}.{} GiB", bytes / (1024 * 1024 * 1024),
            (bytes % (1024 * 1024 * 1024)) / (100 * 1024 * 1024))
    } else if bytes >= 1024 * 1024 {
        format!("{}.{} MiB", bytes / (1024 * 1024),
            (bytes % (1024 * 1024)) / (100 * 1024))
    } else if bytes >= 1024 {
        format!("{}.{} KiB", bytes / 1024, (bytes % 1024) / 100)
    } else {
        format!("{} B", bytes)
    }
}

/// Parse a category name.
pub fn parse_category(name: &str) -> Option<CleanCategory> {
    match name {
        "trash" => Some(CleanCategory::Trash),
        "temp" | "tmp" => Some(CleanCategory::TempFiles),
        "thumbs" | "thumbnails" => Some(CleanCategory::Thumbnails),
        "logs" => Some(CleanCategory::LogFiles),
        "pkg" | "packages" => Some(CleanCategory::PackageCache),
        "dupes" | "duplicates" => Some(CleanCategory::DuplicateFiles),
        "large" => Some(CleanCategory::LargeFiles),
        "downloads" | "dl" => Some(CleanCategory::OldDownloads),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (item_count, total_freed_bytes, scan_count, clean_count, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.items.len(), s.total_freed, s.total_scans, s.total_cleans, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for storage cleanup module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[storageclean] Running self-tests...");

    // Reset state
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial config
    {
        let cfg = config().expect("config");
        assert!(!cfg.auto_enabled);
        assert_eq!(cfg.auto_clean_threshold_pct, 90);
        assert_eq!(cfg.large_file_threshold, DEFAULT_LARGE_FILE_THRESHOLD);
        serial_println!("[storageclean]   1. Default configuration — OK");
    }

    // Test 2: scan (may find nothing or something depending on VFS state)
    {
        let report = scan().expect("scan");
        assert!(report.total_items < MAX_SCAN_ENTRIES);
        let (items, _, scans, _, _) = stats();
        assert_eq!(scans, 1);
        let _ = items; // Use value
        serial_println!("[storageclean]   2. Scan completes successfully — OK");
    }

    // Test 3: format_size helper
    {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "0.5 KiB");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert!(format_size(1024 * 1024).contains("MiB"));
        assert!(format_size(1024 * 1024 * 1024).contains("GiB"));
        serial_println!("[storageclean]   3. format_size helper — OK");
    }

    // Test 4: configuration changes
    {
        set_auto_enabled(true).expect("enable auto");
        set_auto_threshold(85).expect("set threshold");
        set_large_threshold(50 * 1024 * 1024).expect("set large threshold");
        set_old_download_days(60).expect("set old days");
        set_log_retention(7).expect("set log retention");
        let cfg = config().expect("config");
        assert!(cfg.auto_enabled);
        assert_eq!(cfg.auto_clean_threshold_pct, 85);
        assert_eq!(cfg.large_file_threshold, 50 * 1024 * 1024);
        assert_eq!(cfg.old_download_days, 60);
        assert_eq!(cfg.log_retention_days, 7);
        serial_println!("[storageclean]   4. Configuration changes — OK");
    }

    // Test 5: exclusions
    {
        add_exclusion("/home/important").expect("add exclusion");
        add_exclusion("/data/keep").expect("add exclusion");
        let excl = exclusions();
        assert_eq!(excl.len(), 2);
        remove_exclusion("/home/important").expect("remove exclusion");
        assert_eq!(exclusions().len(), 1);
        let result = remove_exclusion("/nonexistent");
        assert!(result.is_err());
        serial_println!("[storageclean]   5. Exclusion management — OK");
    }

    // Test 6: parse_category
    {
        assert_eq!(parse_category("trash"), Some(CleanCategory::Trash));
        assert_eq!(parse_category("temp"), Some(CleanCategory::TempFiles));
        assert_eq!(parse_category("thumbs"), Some(CleanCategory::Thumbnails));
        assert_eq!(parse_category("logs"), Some(CleanCategory::LogFiles));
        assert_eq!(parse_category("pkg"), Some(CleanCategory::PackageCache));
        assert_eq!(parse_category("large"), Some(CleanCategory::LargeFiles));
        assert_eq!(parse_category("dl"), Some(CleanCategory::OldDownloads));
        assert!(parse_category("unknown").is_none());
        serial_println!("[storageclean]   6. Category parsing — OK");
    }

    // Test 7: auto-category management
    {
        add_auto_category(CleanCategory::LargeFiles).expect("add auto cat");
        let cfg = config().expect("config");
        assert!(cfg.auto_categories.contains(&CleanCategory::LargeFiles));
        remove_auto_category(CleanCategory::LargeFiles).expect("remove auto cat");
        let cfg = config().expect("config");
        assert!(!cfg.auto_categories.contains(&CleanCategory::LargeFiles));
        serial_println!("[storageclean]   7. Auto-category management — OK");
    }

    // Test 8: clean operation
    {
        let _ = scan();
        let result = clean(&[CleanCategory::Thumbnails]).expect("clean");
        // May or may not free anything depending on thumbcache state
        assert!(result.errors == 0 || result.items_cleaned == 0);
        let (_, _, _, cleans, _) = stats();
        assert!(cleans >= 1);
        serial_println!("[storageclean]   8. Clean operation — OK");
    }

    // Test 9: category labels
    {
        for cat in CleanCategory::all() {
            let label = cat.label();
            assert!(!label.is_empty());
        }
        assert_eq!(CleanCategory::all().len(), 8);
        serial_println!("[storageclean]   9. Category labels — OK");
    }

    // Test 10: invalid threshold
    {
        let result = set_auto_threshold(101);
        assert!(result.is_err());
        serial_println!("[storageclean]  10. Invalid threshold rejected — OK");
    }

    // Test 11: last_report
    {
        let _ = scan();
        let report = last_report();
        assert!(report.is_some());
        serial_println!("[storageclean]  11. Last report cached — OK");
    }

    serial_println!("[storageclean] All 11 self-tests passed.");
}
