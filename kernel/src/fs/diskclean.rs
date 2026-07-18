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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// All cleanup categories, in display order.
const ALL_CATEGORIES: [CleanCategory; 10] = [
    CleanCategory::TempFiles, CleanCategory::SystemCache, CleanCategory::AppCache,
    CleanCategory::OldLogs, CleanCategory::PackageCache, CleanCategory::ThumbnailCache,
    CleanCategory::TrashBin, CleanCategory::DownloadedUpdates, CleanCategory::BrowserData,
    CleanCategory::CrashDumps,
];

/// Build per-category summaries from a set of items (categories with no items
/// are omitted).
fn build_summaries(items: &[CleanItem]) -> Vec<CategorySummary> {
    ALL_CATEGORIES.iter().filter_map(|&cat| {
        let matching: Vec<&CleanItem> = items.iter().filter(|i| i.category == cat).collect();
        if matching.is_empty() {
            None
        } else {
            Some(CategorySummary {
                category: cat,
                item_count: matching.len() as u64,
                total_bytes: matching.iter().map(|i| i.size_bytes).sum(),
            })
        }
    }).collect()
}

/// Scan for reclaimable items.
///
/// Performs a fresh scan: clears any previous results, records the scan, and
/// returns per-category summaries of what was found. A real scan walks the
/// reclaimable locations (`/tmp`, `/var/cache`, the trash bin, crash dumps, …)
/// and records each file via [`add_item`]. No filesystem-walk backend exists
/// yet, so an honest scan finds NOTHING rather than fabricating phantom files —
/// the `/proc/diskclean` generator and the `diskclean` kshell command surface
/// the scan results and cleaned-byte totals as if they were real, so inventing
/// reclaimable files would make the user believe gigabytes of junk exist (and
/// that cleaning them freed real space) when nothing was scanned.
///
/// (Previously this injected nine hardcoded fake items — `/tmp/session-*`
/// (50 MB), `/tmp/build-*` (120 MB), `/var/cache/apt` (300 MB),
/// `/home/.cache/app1` (75 MB), `/var/log/old/*.gz` (25 MB),
/// `/var/cache/packages` (500 MB), `/home/.thumbnails` (40 MB),
/// `/home/.trash` (200 MB) and `/var/crash` (150 MB) — totalling ~1.46 GB of
/// phantom reclaimable space.)
pub fn scan() -> KernelResult<Vec<CategorySummary>> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.last_scan_ns = now;
        state.total_scans += 1;
        // Fresh scan results. The real filesystem walk (when implemented) will
        // push each found file via add_item here; until then a scan honestly
        // finds nothing rather than fabricating phantom reclaimable files.
        state.items.clear();
        Ok(build_summaries(&state.items))
    })
}

/// Record a reclaimable item found during a scan.
///
/// This is the primitive a real filesystem-walk scan uses to report each
/// reclaimable file it finds (and is also how tests populate a known set of
/// items without a real backend). Returns [`KernelError::ResourceExhausted`] if
/// the item table is full.
pub fn add_item(category: CleanCategory, path: &str, size_bytes: u64, safe_to_remove: bool) -> KernelResult<()> {
    with_state(|state| {
        if state.items.len() >= MAX_ITEMS {
            return Err(KernelError::ResourceExhausted);
        }
        state.items.push(CleanItem {
            category, path: String::from(path), size_bytes, safe_to_remove,
        });
        Ok(())
    })
}

/// Per-category summaries of the currently-recorded reclaimable items.
pub fn summarize() -> Vec<CategorySummary> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| build_summaries(&s.items))
}

/// Clean items in specified categories.
pub fn clean(categories: &[CleanCategory]) -> KernelResult<(u64, u64)> {
    with_state(|state| {
        let mut cleaned_bytes: u64 = 0;
        let mut cleaned_items: u64 = 0;
        let _before = state.items.len();
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
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live scan-item table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no items, zeroed counters.
    assert!(list_items().is_empty());
    let (c0, s0, cb0, ci0, _) = stats();
    assert_eq!((c0, s0, cb0, ci0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Scan — honestly finds NOTHING (no filesystem-walk backend yet). The
    //    scan is still recorded (total_scans advances) but no phantom items are
    //    fabricated, so summaries and the item table stay empty.
    let summaries = scan().expect("scan");
    assert!(summaries.is_empty());
    assert!(list_items().is_empty());
    let (_, s1, _, _, _) = stats();
    assert_eq!(s1, 1);
    crate::serial_println!("  [2/8] honest empty scan: OK");

    // 3: Populate via the real primitive add_item (the API a real FS-walk scan,
    //    or a test, uses to report each reclaimable file it finds).
    add_item(CleanCategory::TempFiles, "/tmp/a", 1000, true).expect("add a");
    add_item(CleanCategory::TempFiles, "/tmp/b", 2000, true).expect("add b");
    add_item(CleanCategory::SystemCache, "/var/cache/x", 5000, true).expect("add x");
    add_item(CleanCategory::CrashDumps, "/var/crash/y", 3000, false).expect("add y");
    assert_eq!(list_items().len(), 4);
    crate::serial_println!("  [3/8] add_item: OK");

    // 4: Category summaries — TempFiles has 2 items totalling 3000 bytes.
    let sums = summarize();
    let temp_sum = sums.iter().find(|s| s.category == CleanCategory::TempFiles).expect("temp");
    assert_eq!(temp_sum.item_count, 2);
    assert_eq!(temp_sum.total_bytes, 3000);
    crate::serial_println!("  [4/8] summaries: OK");

    // 5: Estimate — sums only safe_to_remove items: 1000 + 2000 + 5000 = 8000
    //    (the 3000-byte crash dump is NOT safe_to_remove, so it is excluded).
    assert_eq!(estimate(), 8000);
    crate::serial_println!("  [5/8] estimate: OK");

    // 6: Clean temp files — both temp items are safe, so (2 items, 3000 bytes).
    let (items, bytes) = clean(&[CleanCategory::TempFiles]).expect("clean");
    assert_eq!((items, bytes), (2, 3000));
    let after = list_items();
    assert_eq!(after.iter().filter(|i| i.category == CleanCategory::TempFiles).count(), 0);
    crate::serial_println!("  [6/8] clean temp: OK");

    // 7: Clean SystemCache + CrashDumps — only the cache item is safe_to_remove
    //    (1 item, 5000 bytes); the crash dump is retained because it is unsafe.
    let (items2, bytes2) = clean(&[CleanCategory::SystemCache, CleanCategory::CrashDumps]).expect("clean2");
    assert_eq!((items2, bytes2), (1, 5000));
    assert_eq!(list_items().len(), 1); // the unsafe crash dump remains
    crate::serial_println!("  [7/8] clean cache (unsafe retained): OK");

    // 8: Final stats — 1 item left (crash dump), 1 scan, 8000 cleaned bytes
    //    across 3 cleaned items.
    let (count, scans, cleaned_bytes, cleaned_items, ops) = stats();
    assert_eq!((count, scans, cleaned_bytes, cleaned_items), (1, 1, 8000, 3));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("diskclean::self_test() — all 8 tests passed");
}
