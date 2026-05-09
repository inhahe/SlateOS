//! Disk Cleanup — automated storage space recovery.
//!
//! Scans for reclaimable storage including temp files, caches,
//! old logs, and package artifacts, providing safe cleanup
//! recommendations.
//!
//! ## Architecture
//!
//! ```text
//! Storage cleanup
//!   → diskclean::scan() → find reclaimable items
//!   → diskclean::clean(categories) → remove selected items
//!   → diskclean::estimate() → total reclaimable space
//!
//! Integration:
//!   → storageclean (storage cleaning)
//!   → reclaim (space reclamation)
//!   → diskuse (disk usage)
//!   → storagesense (storage sense)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Cleanup category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanCategory {
    TempFiles,
    SystemCache,
    AppCache,
    OldLogs,
    PackageCache,
    ThumbnailCache,
    TrashBin,
    DownloadedUpdates,
    BrowserData,
    CrashDumps,
}

impl CleanCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::TempFiles => "Temp Files",
            Self::SystemCache => "System Cache",
            Self::AppCache => "App Cache",
            Self::OldLogs => "Old Logs",
            Self::PackageCache => "Package Cache",
            Self::ThumbnailCache => "Thumbnails",
            Self::TrashBin => "Trash Bin",
            Self::DownloadedUpdates => "Downloaded Updates",
            Self::BrowserData => "Browser Data",
            Self::CrashDumps => "Crash Dumps",
        }
    }
}

/// A reclaimable item found during scan.
#[derive(Debug, Clone)]
pub struct CleanItem {
    pub category: CleanCategory,
    pub path: String,
    pub size_bytes: u64,
    pub safe_to_remove: bool,
}

/// Scan result summary per category.
#[derive(Debug, Clone)]
pub struct CategorySummary {
    pub category: CleanCategory,
    pub item_count: u64,
    pub total_bytes: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ITEMS: usize = 2000;

struct State {
    items: Vec<CleanItem>,
    total_scans: u64,
    total_cleaned_bytes: u64,
    total_cleaned_items: u64,
    last_scan_ns: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        items: Vec::new(),
        total_scans: 0,
        total_cleaned_bytes: 0,
        total_cleaned_items: 0,
        last_scan_ns: 0,
        ops: 0,
    });
}

/// Scan for reclaimable items (simulated).
pub fn scan() -> KernelResult<Vec<CategorySummary>> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.last_scan_ns = now;
        state.total_scans += 1;
        // Simulate finding items.
        state.items.clear();
        let simulated = alloc::vec![
            (CleanCategory::TempFiles, "/tmp/session-*", 50_000_000u64),
            (CleanCategory::TempFiles, "/tmp/build-*", 120_000_000),
            (CleanCategory::SystemCache, "/var/cache/apt", 300_000_000),
            (CleanCategory::AppCache, "/home/.cache/app1", 75_000_000),
            (CleanCategory::OldLogs, "/var/log/old/*.gz", 25_000_000),
            (CleanCategory::PackageCache, "/var/cache/packages", 500_000_000),
            (CleanCategory::ThumbnailCache, "/home/.thumbnails", 40_000_000),
            (CleanCategory::TrashBin, "/home/.trash", 200_000_000),
            (CleanCategory::CrashDumps, "/var/crash", 150_000_000),
        ];
        for (cat, path, size) in simulated {
            state.items.push(CleanItem {
                category: cat, path: String::from(path),
                size_bytes: size, safe_to_remove: true,
            });
        }
        // Build summaries.
        let categories = [
            CleanCategory::TempFiles, CleanCategory::SystemCache, CleanCategory::AppCache,
            CleanCategory::OldLogs, CleanCategory::PackageCache, CleanCategory::ThumbnailCache,
            CleanCategory::TrashBin, CleanCategory::DownloadedUpdates, CleanCategory::BrowserData,
            CleanCategory::CrashDumps,
        ];
        let summaries: Vec<CategorySummary> = categories.iter().filter_map(|&cat| {
            let items: Vec<&CleanItem> = state.items.iter().filter(|i| i.category == cat).collect();
            if items.is_empty() { None } else {
                Some(CategorySummary {
                    category: cat,
                    item_count: items.len() as u64,
                    total_bytes: items.iter().map(|i| i.size_bytes).sum(),
                })
            }
        }).collect();
        Ok(summaries)
    })
}

/// Clean items in specified categories.
pub fn clean(categories: &[CleanCategory]) -> KernelResult<(u64, u64)> {
    with_state(|state| {
        let mut cleaned_bytes: u64 = 0;
        let mut cleaned_items: u64 = 0;
        let before = state.items.len();
        state.items.retain(|item| {
            if categories.contains(&item.category) && item.safe_to_remove {
                cleaned_bytes += item.size_bytes;
                cleaned_items += 1;
                false
            } else {
                true
            }
        });
        state.total_cleaned_bytes += cleaned_bytes;
        state.total_cleaned_items += cleaned_items;
        Ok((cleaned_items, cleaned_bytes))
    })
}

/// Estimate total reclaimable space.
pub fn estimate() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| {
        s.items.iter().filter(|i| i.safe_to_remove).map(|i| i.size_bytes).sum()
    })
}

/// Get current scan items.
pub fn list_items() -> Vec<CleanItem> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.items.clone())
}

/// Statistics: (item_count, total_scans, total_cleaned_bytes, total_cleaned_items, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.items.len(), s.total_scans, s.total_cleaned_bytes, s.total_cleaned_items, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("diskclean::self_test() — running tests...");
    init_defaults();

    // 1: Empty state.
    assert!(list_items().is_empty());
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Scan.
    let summaries = scan().expect("scan");
    assert!(!summaries.is_empty());
    assert!(!list_items().is_empty());
    crate::serial_println!("  [2/8] scan: OK");

    // 3: Estimate.
    let est = estimate();
    assert!(est > 0);
    crate::serial_println!("  [3/8] estimate: OK");

    // 4: Category summaries.
    let temp_sum = summaries.iter().find(|s| s.category == CleanCategory::TempFiles);
    assert!(temp_sum.is_some());
    assert!(temp_sum.expect("temp").total_bytes > 0);
    crate::serial_println!("  [4/8] summaries: OK");

    // 5: Clean temp files.
    let (items, bytes) = clean(&[CleanCategory::TempFiles]).expect("clean");
    assert!(items > 0);
    assert!(bytes > 0);
    crate::serial_println!("  [5/8] clean temp: OK");

    // 6: Verify cleaned.
    let items_ref = list_items();
    let temp_remaining = items_ref.iter().filter(|i| i.category == CleanCategory::TempFiles).count();
    assert_eq!(temp_remaining, 0);
    crate::serial_println!("  [6/8] verified clean: OK");

    // 7: Clean all remaining.
    let cats = [CleanCategory::SystemCache, CleanCategory::AppCache, CleanCategory::OldLogs,
        CleanCategory::PackageCache, CleanCategory::ThumbnailCache, CleanCategory::TrashBin,
        CleanCategory::CrashDumps];
    let (items2, bytes2) = clean(&cats).expect("clean_all");
    assert!(items2 > 0);
    assert!(bytes2 > 0);
    assert!(list_items().is_empty());
    crate::serial_println!("  [7/8] clean all: OK");

    // 8: Stats.
    let (count, scans, cleaned_bytes, cleaned_items, ops) = stats();
    assert_eq!(count, 0);
    assert_eq!(scans, 1);
    assert!(cleaned_bytes > 0);
    assert!(cleaned_items > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("diskclean::self_test() — all 8 tests passed");
}
