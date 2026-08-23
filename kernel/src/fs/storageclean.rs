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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

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

    /// Is this category advisory (a recommendation the user must act on
    /// file-by-file) rather than something [`clean`] may delete wholesale?
    ///
    /// [`clean`] works at category granularity, so obeying it for these would
    /// mean deleting every large file under `/home` because the user asked to
    /// "clean all" — destroying data they never named. Reclaiming space in an
    /// advisory category is [`clean_paths`]' job.
    pub fn is_advisory(self) -> bool {
        matches!(
            self,
            Self::DuplicateFiles | Self::LargeFiles | Self::OldDownloads
        )
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
    /// The file this item names, or `None` for a synthetic item that does not
    /// correspond to a filesystem path (the in-memory thumbnail cache is the
    /// only such item today). Keeping this an `Option` rather than stuffing a
    /// human-readable label like `"[thumbnail cache]"` into a path field means
    /// [`clean`] can never hand a label to the VFS as if it were a real path.
    pub path: Option<PathBuf>,
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
    /// Paths excluded from scanning. A scan skips any file at or below one of
    /// these paths; see [`is_excluded`].
    pub exclusions: Vec<PathBuf>,
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

        // Snapshot the exclusion list up front: the scanners need to read it
        // while `state.items` is mutably borrowed, which a borrow of
        // `state.config` would forbid.
        let excl = state.config.exclusions.clone();
        let retention_days = state.config.log_retention_days;
        let large_threshold = state.config.large_file_threshold;
        let download_days = state.config.old_download_days;

        // Category: Trash — query trash module
        let trash_bytes = scan_trash(&mut state.items, &excl);

        // Category: TempFiles — query /tmp
        let temp_bytes = scan_temp_files(&mut state.items, &excl);

        // Category: Thumbnails — query thumbcache
        let thumb_bytes = scan_thumbnails(&mut state.items);

        // Category: LogFiles
        let log_bytes = scan_log_files(&mut state.items, &excl, retention_days);

        // Category: PackageCache
        let pkg_bytes = scan_package_cache(&mut state.items, &excl);

        // Category: LargeFiles
        let large_bytes = scan_large_files(&mut state.items, &excl, large_threshold);

        // Category: OldDownloads
        let download_bytes = scan_old_downloads(&mut state.items, &excl, download_days);

        // Build category summaries
        let mut categories = Vec::new();
        for cat in CleanCategory::all() {
            let items: Vec<&CleanItem> =
                state.items.iter().filter(|i| i.category == *cat).collect();
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

        let total_bytes = trash_bytes
            + temp_bytes
            + thumb_bytes
            + log_bytes
            + pkg_bytes
            + large_bytes
            + download_bytes;

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

/// Is `path` covered by one of the configured exclusions?
///
/// An exclusion names a subtree, not a byte prefix: excluding `/home/keep`
/// must not also exclude `/home/keepsakes`. [`path_in_subtree`] compares
/// component-wise, so it gets that right.
///
/// [`path_in_subtree`]: crate::fs::pathutil::path_in_subtree
fn is_excluded(path: &Path, exclusions: &[PathBuf]) -> bool {
    exclusions
        .iter()
        .any(|e| crate::fs::pathutil::path_in_subtree(path, e))
}

/// Shared body of the directory-walking scanners.
///
/// Reads `dir`, and for every entry that `accept` approves and that is not
/// excluded, records a [`CleanItem`] built by `describe` from the entry's path
/// and size. Returns the total size of the accepted entries — including any
/// that did not fit within [`MAX_SCAN_ENTRIES`], so the reported reclaimable
/// total stays honest even when the item list is truncated.
fn scan_dir<A, D>(
    items: &mut Vec<CleanItem>,
    exclusions: &[PathBuf],
    dir: &Path,
    accept: A,
    describe: D,
) -> u64
where
    A: Fn(&Path, u64) -> bool,
    D: Fn(&Path, u64) -> CleanItem,
{
    use crate::fs::Vfs;
    let Ok(entries) = Vfs::readdir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries {
        // `Path::join` inserts exactly one separator, so a root `dir` needs no
        // special case: `/` joined with `tmp` is `/tmp`, not `//tmp`.
        let path = dir.join(&entry.name);
        if is_excluded(&path, exclusions) {
            continue;
        }
        let size = Vfs::read_file(&path).map_or(0, |d| d.len() as u64);
        if !accept(&path, size) {
            continue;
        }
        if items.len() < MAX_SCAN_ENTRIES {
            items.push(describe(&path, size));
        }
        total = total.saturating_add(size);
    }
    total
}

fn scan_trash(items: &mut Vec<CleanItem>, exclusions: &[PathBuf]) -> u64 {
    scan_dir(
        items,
        exclusions,
        Path::new("/_TRASH"),
        // The trash index is bookkeeping, not a reclaimable item.
        |path, _| {
            path.file_name()
                .is_none_or(|n| !n.as_bytes().starts_with(b"_INDEX"))
        },
        |path, size| CleanItem {
            path: Some(path.to_path_buf()),
            size_bytes: size,
            category: CleanCategory::Trash,
            reason: String::from("In recycle bin"),
            age_days: 0,
        },
    )
}

fn scan_temp_files(items: &mut Vec<CleanItem>, exclusions: &[PathBuf]) -> u64 {
    scan_dir(
        items,
        exclusions,
        Path::new("/tmp"),
        |_, _| true,
        |path, size| CleanItem {
            path: Some(path.to_path_buf()),
            size_bytes: size,
            category: CleanCategory::TempFiles,
            reason: String::from("Temporary file"),
            age_days: 0,
        },
    )
}

fn scan_thumbnails(items: &mut Vec<CleanItem>) -> u64 {
    // Query thumbcache stats for memory usage
    let (count, _, mem_bytes, _, _, _) = crate::fs::thumbcache::stats();
    if count > 0 && mem_bytes > 0 && items.len() < MAX_SCAN_ENTRIES {
        items.push(CleanItem {
            // The thumbnail cache lives in memory, so this item names no file.
            path: None,
            size_bytes: mem_bytes,
            category: CleanCategory::Thumbnails,
            reason: format!("{} cached thumbnails", count),
            age_days: 0,
        });
    }
    mem_bytes
}

fn scan_log_files(items: &mut Vec<CleanItem>, exclusions: &[PathBuf], _retention_days: u32) -> u64 {
    let mut total = 0u64;
    for dir in ["/var/log", "/log"] {
        total = total.saturating_add(scan_dir(
            items,
            exclusions,
            Path::new(dir),
            |path, _| {
                path.file_name().is_some_and(|n| {
                    let n = n.as_bytes();
                    n.ends_with(b".log") || n.ends_with(b".log.old")
                })
            },
            |path, size| CleanItem {
                path: Some(path.to_path_buf()),
                size_bytes: size,
                category: CleanCategory::LogFiles,
                reason: String::from("Log file"),
                age_days: 0,
            },
        ));
    }
    total
}

fn scan_package_cache(items: &mut Vec<CleanItem>, exclusions: &[PathBuf]) -> u64 {
    let mut total = 0u64;
    for dir in ["/var/cache/pkg", "/var/cache/packages"] {
        total = total.saturating_add(scan_dir(
            items,
            exclusions,
            Path::new(dir),
            |_, _| true,
            |path, size| CleanItem {
                path: Some(path.to_path_buf()),
                size_bytes: size,
                category: CleanCategory::PackageCache,
                reason: String::from("Cached package"),
                age_days: 0,
            },
        ));
    }
    total
}

fn scan_large_files(items: &mut Vec<CleanItem>, exclusions: &[PathBuf], threshold: u64) -> u64 {
    let mut total = 0u64;
    for dir in ["/home", "/root", "/data"] {
        total = total.saturating_add(scan_dir(
            items,
            exclusions,
            Path::new(dir),
            |_, size| size >= threshold,
            |path, size| CleanItem {
                path: Some(path.to_path_buf()),
                size_bytes: size,
                category: CleanCategory::LargeFiles,
                reason: format!("Large file ({})", format_size(size)),
                age_days: 0,
            },
        ));
    }
    total
}

fn scan_old_downloads(items: &mut Vec<CleanItem>, exclusions: &[PathBuf], _age_days: u32) -> u64 {
    let mut total = 0u64;
    for dir in ["/home/Downloads", "/root/Downloads"] {
        total = total.saturating_add(scan_dir(
            items,
            exclusions,
            Path::new(dir),
            |_, _| true,
            |path, size| CleanItem {
                path: Some(path.to_path_buf()),
                size_bytes: size,
                category: CleanCategory::OldDownloads,
                reason: String::from("Old download"),
                age_days: 0,
            },
        ));
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
        s.items
            .iter()
            .filter(|i| i.category == cat)
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

/// Clean up items in the specified categories.
///
/// Advisory categories ([`CleanCategory::is_advisory`]) are skipped: their
/// items stay in the cache and contribute nothing to `freed_bytes`. Use
/// [`clean_paths`] to delete the specific files the user picked out of them.
pub fn clean(categories: &[CleanCategory]) -> KernelResult<CleanResult> {
    with_state(|state| {
        let mut freed = 0u64;
        let mut cleaned = 0usize;
        let mut errors = 0usize;
        let mut category_freed: Vec<(CleanCategory, u64)> = Vec::new();

        for cat in categories {
            if cat.is_advisory() {
                continue;
            }
            let mut cat_freed = 0u64;
            let items_to_clean: Vec<CleanItem> = state
                .items
                .iter()
                .filter(|i| i.category == *cat)
                .cloned()
                .collect();

            for item in &items_to_clean {
                match *cat {
                    CleanCategory::Trash
                    | CleanCategory::TempFiles
                    | CleanCategory::LogFiles
                    | CleanCategory::PackageCache => {
                        // Every scanned item in these categories names a real
                        // file, so a `None` path here is a scanner bug; count
                        // it as an error rather than swallowing it.
                        match item.path.as_ref() {
                            Some(p) if crate::fs::Vfs::remove(p).is_ok() => {
                                cat_freed = cat_freed.saturating_add(item.size_bytes);
                                cleaned = cleaned.saturating_add(1);
                            }
                            _ => errors = errors.saturating_add(1),
                        }
                    }
                    CleanCategory::Thumbnails => {
                        crate::fs::thumbcache::clear();
                        cat_freed = cat_freed.saturating_add(item.size_bytes);
                        cleaned = cleaned.saturating_add(1);
                    }
                    CleanCategory::DuplicateFiles
                    | CleanCategory::LargeFiles
                    | CleanCategory::OldDownloads => {
                        unreachable!("advisory categories skipped above")
                    }
                }
            }

            if cat_freed > 0 {
                category_freed.push((*cat, cat_freed));
            }
            freed = freed.saturating_add(cat_freed);
        }

        // Drop the cleaned items from the cache. Advisory categories were left
        // untouched on disk, so their items must survive here too — otherwise
        // the UI would show the recommendations vanishing as if acted upon.
        state
            .items
            .retain(|i| i.category.is_advisory() || !categories.contains(&i.category));
        state.total_freed = state.total_freed.saturating_add(freed);
        state.total_cleans = state.total_cleans.saturating_add(1);

        Ok(CleanResult {
            freed_bytes: freed,
            items_cleaned: cleaned,
            errors,
            category_freed,
        })
    })
}

/// Delete specific scanned files, whatever category they came from.
///
/// This is how space is reclaimed in an advisory category: the user picks the
/// individual files, so there is no risk of a category-wide `clean` sweeping
/// away data nobody named.
///
/// A path that is not in the cached scan results is reported as an error
/// rather than deleted — the cache is the record of what the user was shown,
/// and this call is not a general-purpose `rm`.
pub fn clean_paths<P: AsRef<Path>>(paths: &[P]) -> KernelResult<CleanResult> {
    with_state(|state| {
        let mut freed = 0u64;
        let mut cleaned = 0usize;
        let mut errors = 0usize;
        let mut category_freed: Vec<(CleanCategory, u64)> = Vec::new();
        let mut removed: Vec<PathBuf> = Vec::new();

        for want in paths {
            let want = want.as_ref();
            let Some(item) = state
                .items
                .iter()
                .find(|i| i.path.as_deref() == Some(want))
                .cloned()
            else {
                errors = errors.saturating_add(1);
                continue;
            };
            if crate::fs::Vfs::remove(want).is_err() {
                errors = errors.saturating_add(1);
                continue;
            }
            freed = freed.saturating_add(item.size_bytes);
            cleaned = cleaned.saturating_add(1);
            removed.push(want.to_path_buf());
            match category_freed.iter_mut().find(|(c, _)| *c == item.category) {
                Some((_, f)) => *f = f.saturating_add(item.size_bytes),
                None => category_freed.push((item.category, item.size_bytes)),
            }
        }

        state.items.retain(|i| match i.path.as_ref() {
            Some(p) => !removed.contains(p),
            None => true,
        });
        state.total_freed = state.total_freed.saturating_add(freed);
        state.total_cleans = state.total_cleans.saturating_add(1);

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

/// Add an exclusion path: a scan skips this file and everything under it.
///
/// The path must be absolute. A relative path could never match the absolute
/// paths the scanners build, so accepting one would silently do nothing, and
/// an empty path would exclude the entire filesystem.
pub fn add_exclusion<P: AsRef<Path> + ?Sized>(path: &P) -> KernelResult<()> {
    let path = path.as_ref();
    if !path.is_absolute() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.config.exclusions.len() >= MAX_EXCLUSIONS {
            return Err(KernelError::ResourceExhausted);
        }
        if !state.config.exclusions.iter().any(|e| e.as_path() == path) {
            state.config.exclusions.push(path.to_path_buf());
        }
        Ok(())
    })
}

/// Remove an exclusion path.
pub fn remove_exclusion<P: AsRef<Path> + ?Sized>(path: &P) -> KernelResult<()> {
    let path = path.as_ref();
    with_state(|state| {
        let idx = state
            .config
            .exclusions
            .iter()
            .position(|e| e.as_path() == path)
            .ok_or(KernelError::NotFound)?;
        state.config.exclusions.remove(idx);
        Ok(())
    })
}

/// List exclusion paths.
pub fn exclusions() -> Vec<PathBuf> {
    let guard = STATE.lock();
    guard
        .as_ref()
        .map_or_else(Vec::new, |s| s.config.exclusions.clone())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format byte count as human-readable string.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!(
            "{}.{} GiB",
            bytes / (1024 * 1024 * 1024),
            (bytes % (1024 * 1024 * 1024)) / (100 * 1024 * 1024)
        )
    } else if bytes >= 1024 * 1024 {
        format!(
            "{}.{} MiB",
            bytes / (1024 * 1024),
            (bytes % (1024 * 1024)) / (100 * 1024)
        )
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
        Some(s) => (
            s.items.len(),
            s.total_freed,
            s.total_scans,
            s.total_cleans,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
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
        // A relative or empty exclusion could never match the absolute paths
        // the scanners build, so it must be rejected rather than silently
        // ignored — and an empty one would exclude the whole filesystem.
        assert!(add_exclusion("home/important").is_err());
        assert!(add_exclusion("").is_err());
        remove_exclusion("/data/keep").expect("remove exclusion");
        assert!(exclusions().is_empty());
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

    // Test 12: a file whose name is not valid UTF-8 is scanned, listed under
    // its exact bytes, and can be excluded. A cleanup tool that cannot see a
    // file is one that silently leaves the user's disk full.
    {
        use crate::fs::Vfs;
        let wild = Path::new(b"/tmp/_sc\xffwild.bin".as_slice());
        Vfs::write_file(wild, b"junk").expect("write undecodable temp file");

        let _ = scan().expect("scan");
        assert!(
            items_for_category(CleanCategory::TempFiles)
                .iter()
                .any(|i| i.path.as_deref() == Some(wild)),
            "undecodable temp file missing from scan"
        );

        // Excluding it must actually take effect on the next scan — before
        // this, the exclusion list was consulted by nothing at all.
        add_exclusion(wild).expect("exclude undecodable path");
        let _ = scan().expect("rescan");
        assert!(
            !items_for_category(CleanCategory::TempFiles)
                .iter()
                .any(|i| i.path.as_deref() == Some(wild)),
            "exclusion did not take effect"
        );
        remove_exclusion(wild).expect("unexclude");

        // Excluding a sibling whose name merely shares a byte prefix must not
        // hide it: exclusions name subtrees, not byte prefixes.
        add_exclusion("/tmp/_sc").expect("exclude prefix");
        let _ = scan().expect("rescan");
        assert!(
            items_for_category(CleanCategory::TempFiles)
                .iter()
                .any(|i| i.path.as_deref() == Some(wild)),
            "byte-prefix exclusion wrongly hid a sibling"
        );
        remove_exclusion("/tmp/_sc").expect("unexclude prefix");

        Vfs::remove(wild).ok();
        serial_println!("[storageclean]  12. Undecodable name scanned & excludable — OK");
    }

    // Test 13: advisory categories are recommendations, not deletions.
    {
        {
            let mut guard = STATE.lock();
            let s = guard.as_mut().expect("state");
            s.items.push(CleanItem {
                path: Some(PathBuf::from("/home/_sc_absent_huge.bin")),
                size_bytes: 999,
                category: CleanCategory::LargeFiles,
                reason: String::from("synthetic"),
                age_days: 0,
            });
        }
        let freed_before = stats().1;
        let result = clean(&[CleanCategory::LargeFiles]).expect("clean advisory");
        // Nothing was deleted, so nothing may be reported as freed.
        assert_eq!(result.freed_bytes, 0);
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(stats().1, freed_before);
        // ...and the recommendation must survive, since it was not acted on.
        assert!(
            items_for_category(CleanCategory::LargeFiles)
                .iter()
                .any(|i| i.path.as_deref() == Some(Path::new("/home/_sc_absent_huge.bin"))),
            "advisory item wrongly dropped from the cache"
        );

        // clean_paths reports a failed delete as an error, never as freed space.
        let result = clean_paths(&["/home/_sc_absent_huge.bin"]).expect("clean_paths");
        assert_eq!(result.freed_bytes, 0);
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors, 1);
        // A path that was never scanned is refused rather than deleted.
        let result = clean_paths(&["/etc/passwd"]).expect("clean_paths unknown");
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors, 1);
        serial_println!("[storageclean]  13. Advisory categories are not deleted — OK");
    }

    serial_println!("[storageclean] All 13 self-tests passed.");
}
