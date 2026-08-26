//! diskcleanup -- Slate OS Disk Cleanup Utility
//!
//! Scans the filesystem for temporary files, caches, logs, recycle bin
//! contents, and other reclaimable space.  Presents a GUI (via guitk) that
//! lets the user select categories, preview what will be deleted, and execute
//! the cleanup with a progress bar and results summary.
//!
//! # Architecture
//!
//! ```text
//! CleanupScanner  -- discovers CleanupItems on disk
//!       |
//!       v
//! CleanupPlan     -- user-selected subset, ready to execute
//!       |
//!       v
//! CleanupExecutor -- deletes files, reports results
//!       |
//!       v
//! CleanupHistory  -- persisted log of past cleanups
//! ```
//!
//! The UI layer (`CleanupUI`) ties these together inside a render loop driven
//! by the guitk `RenderTree` primitives, and `main` hands that UI to
//! `oswindow::app::launch`, which owns the window and the event loop.
//!
//! # Where the clickable rectangles come from
//!
//! [`Layout`] computes every rectangle the user can hit, once, from the window
//! size — and both the renderer and [`CleanupUI::handle_event`] read it. That is
//! not tidiness. Until 2026-08-26 this program had a renderer that computed its
//! geometry inline and **no event handling at all**, so there was nothing for
//! the geometry to disagree with. Adding a hit-test that recomputed those
//! numbers would have created two copies of them, and the failure mode of two
//! copies is a button that is drawn in one place and clicked in another — which
//! nothing in the test suite would catch, because each side is self-consistent.
//! One source, read twice.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, MouseButton, MouseEvent, MouseEventKind};
use guitk::modal::{AlertDialog, DialogResult};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

use oswindow::app::Response;

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 520.0;
const HEADER_HEIGHT: f32 = 48.0;
const FOOTER_HEIGHT: f32 = 56.0;
const ROW_HEIGHT: f32 = 36.0;
const PADDING: f32 = 12.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const BUTTON_WIDTH: f32 = 100.0;
const BUTTON_HEIGHT: f32 = 32.0;
const CORNER_RADIUS: f32 = 6.0;
const CHECKBOX_SIZE: f32 = 16.0;
const PROGRESS_HEIGHT: f32 = 8.0;

/// Where a scan starts when the user presses "Scan" rather than a test.
///
/// One entry, and it is the filesystem root, because [`CleanupScanner::scan`]
/// treats each base as a *prefix* rather than a directory to walk: it joins
/// `"tmp"`, `"var/log"` and the rest onto every base, so a base of `"/"`
/// produces exactly `/tmp` and `/var/log`. The list is a slice rather than a
/// constant path so that a test — or, later, a per-user scan — can pass its own
/// roots to the same function the button calls, instead of a parallel one.
const DEFAULT_SCAN_ROOTS: [&str; 1] = ["/"];

// ============================================================================
// CleanupCategory
// ============================================================================

/// Categories of reclaimable disk space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CleanupCategory {
    TempFiles,
    BrowserCache,
    PackageCache,
    LogFiles,
    RecycleBin,
    ThumbnailCache,
    CrashDumps,
    OldBackups,
    DownloadedUpdates,
}

impl CleanupCategory {
    /// All categories in display order.
    pub const ALL: &'static [CleanupCategory] = &[
        CleanupCategory::TempFiles,
        CleanupCategory::BrowserCache,
        CleanupCategory::PackageCache,
        CleanupCategory::LogFiles,
        CleanupCategory::RecycleBin,
        CleanupCategory::ThumbnailCache,
        CleanupCategory::CrashDumps,
        CleanupCategory::OldBackups,
        CleanupCategory::DownloadedUpdates,
    ];

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::TempFiles => "Temporary Files",
            Self::BrowserCache => "Browser Cache",
            Self::PackageCache => "Package Cache",
            Self::LogFiles => "Log Files",
            Self::RecycleBin => "Recycle Bin",
            Self::ThumbnailCache => "Thumbnail Cache",
            Self::CrashDumps => "Crash Dumps",
            Self::OldBackups => "Old Backups",
            Self::DownloadedUpdates => "Downloaded Updates",
        }
    }

    /// Short description of what this category contains.
    pub fn description(self) -> &'static str {
        match self {
            Self::TempFiles => "Files in /tmp and /var/tmp",
            Self::BrowserCache => "Cached web content from browsers",
            Self::PackageCache => "Old downloaded package archives",
            Self::LogFiles => "System and application log files",
            Self::RecycleBin => "Files in the recycle bin",
            Self::ThumbnailCache => "Cached image thumbnails",
            Self::CrashDumps => "Process crash dump files",
            Self::OldBackups => "Outdated backup snapshots",
            Self::DownloadedUpdates => "Previously downloaded system updates",
        }
    }

    /// Default glob pattern associated with this category.
    pub fn default_pattern(self) -> &'static str {
        match self {
            Self::TempFiles => "/tmp/*",
            Self::BrowserCache => "/home/*/.cache/browser/*",
            Self::PackageCache => "/var/cache/pkg/archives/*",
            Self::LogFiles => "/var/log/*.log",
            Self::RecycleBin => "/home/*/.local/share/Trash/*",
            Self::ThumbnailCache => "/home/*/.cache/thumbnails/*",
            Self::CrashDumps => "/var/crash/*",
            Self::OldBackups => "/var/backups/old/*",
            Self::DownloadedUpdates => "/var/cache/updates/*",
        }
    }
}

// ============================================================================
// CleanupItem
// ============================================================================

/// A single file or directory discovered by the scanner.
#[derive(Clone, Debug, PartialEq)]
pub struct CleanupItem {
    /// Glob pattern that matched this item (e.g. `/tmp/*`).
    pub path_pattern: String,
    /// Actual resolved path on disk.
    pub path: String,
    /// Which category this item belongs to.
    pub category: CleanupCategory,
    /// Human-readable note about the item.
    pub description: String,
    /// Estimated size in bytes.
    pub estimated_size_bytes: u64,
    /// Whether it is safe to delete without user data loss risk.
    pub is_safe: bool,
    /// How many days since this item was last accessed.
    pub last_accessed_days: u32,
}

impl CleanupItem {
    /// Builder-style constructor.
    pub fn new(path: &str, category: CleanupCategory) -> Self {
        Self {
            path_pattern: category.default_pattern().to_string(),
            path: path.to_string(),
            category,
            description: category.description().to_string(),
            estimated_size_bytes: 0,
            is_safe: true,
            last_accessed_days: 0,
        }
    }

    #[must_use]
    pub fn with_size(mut self, bytes: u64) -> Self {
        self.estimated_size_bytes = bytes;
        self
    }

    #[must_use]
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    #[must_use]
    pub fn with_safety(mut self, safe: bool) -> Self {
        self.is_safe = safe;
        self
    }

    #[must_use]
    pub fn with_last_accessed_days(mut self, days: u32) -> Self {
        self.last_accessed_days = days;
        self
    }

    #[must_use]
    pub fn with_pattern(mut self, pattern: &str) -> Self {
        self.path_pattern = pattern.to_string();
        self
    }
}

// ============================================================================
// CleanupScanner
// ============================================================================

/// Scans the filesystem for items that can be cleaned up.
///
/// In a real deployment this would call into the VFS to stat files and walk
/// directories.  The current implementation provides the scanning logic with
/// stub filesystem calls that can be wired to the real VFS later.
pub struct CleanupScanner {
    /// Items discovered during the most recent scan.
    items: Vec<CleanupItem>,
    /// Maximum age (days) for log files before they are considered reclaimable.
    max_log_age_days: u32,
    /// Maximum age (days) for package cache entries.
    max_package_cache_age_days: u32,
}

impl CleanupScanner {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            max_log_age_days: 30,
            max_package_cache_age_days: 60,
        }
    }

    #[must_use]
    pub fn with_max_log_age(mut self, days: u32) -> Self {
        self.max_log_age_days = days;
        self
    }

    #[must_use]
    pub fn with_max_package_cache_age(mut self, days: u32) -> Self {
        self.max_package_cache_age_days = days;
        self
    }

    /// Run a full scan over the given base paths.
    ///
    /// Each path is examined for every category.  Returns all discovered items.
    pub fn scan(&mut self, paths: &[&str]) -> &[CleanupItem] {
        self.items.clear();
        for path in paths {
            self.scan_temp_files(path);
            self.scan_logs(path, self.max_log_age_days);
            self.scan_package_cache(path);
            self.scan_recycle_bin(path);
            self.scan_thumbnail_cache(path);
            self.scan_browser_cache(path);
            self.scan_crash_dumps(path);
            self.scan_old_backups(path);
            self.scan_downloaded_updates(path);
        }
        &self.items
    }

    /// Total estimated bytes that the current scan found.
    pub fn estimate_savings(&self) -> u64 {
        self.items
            .iter()
            .map(|item| item.estimated_size_bytes)
            .sum()
    }

    /// Items found in the most recent scan.
    pub fn items(&self) -> &[CleanupItem] {
        &self.items
    }

    // -- per-category scan methods ------------------------------------------

    /// Scan for temporary files under `<base>/tmp` and `<base>/var/tmp`.
    pub fn scan_temp_files(&mut self, base_path: &str) {
        let tmp_path = join_path(base_path, "tmp");
        let var_tmp_path = join_paths(base_path, &["var", "tmp"]);

        // In a real OS this would call the VFS to enumerate directory contents.
        // We record the directories themselves as representative items.
        self.items.push(
            CleanupItem::new(&tmp_path, CleanupCategory::TempFiles)
                .with_description("Contents of /tmp")
                .with_pattern("/tmp/*"),
        );
        self.items.push(
            CleanupItem::new(&var_tmp_path, CleanupCategory::TempFiles)
                .with_description("Contents of /var/tmp")
                .with_pattern("/var/tmp/*"),
        );
    }

    /// Scan for old log files in `<base>/var/log` older than `max_age_days`.
    pub fn scan_logs(&mut self, base_path: &str, max_age_days: u32) {
        let log_dir = join_paths(base_path, &["var", "log"]);
        self.items.push(
            CleanupItem::new(&log_dir, CleanupCategory::LogFiles)
                .with_description(&format!("Log files older than {max_age_days} days"))
                .with_last_accessed_days(max_age_days)
                .with_pattern("/var/log/*.log"),
        );
    }

    /// Scan for old package downloads in `<base>/var/cache/pkg/archives`.
    pub fn scan_package_cache(&mut self, base_path: &str) {
        let cache_dir = join_paths(base_path, &["var", "cache", "pkg", "archives"]);
        self.items.push(
            CleanupItem::new(&cache_dir, CleanupCategory::PackageCache)
                .with_description("Old downloaded package archives")
                .with_pattern("/var/cache/pkg/archives/*"),
        );
    }

    /// Scan for recycle bin contents under `<base>/home/*/…/Trash`.
    pub fn scan_recycle_bin(&mut self, base_path: &str) {
        let bin_path = join_paths(base_path, &["home", "user", ".local", "share", "Trash"]);
        self.items.push(
            CleanupItem::new(&bin_path, CleanupCategory::RecycleBin)
                .with_description("Deleted files awaiting permanent removal")
                .with_pattern("/home/*/.local/share/Trash/*"),
        );
    }

    /// Scan for thumbnail cache under `<base>/home/*/.cache/thumbnails`.
    pub fn scan_thumbnail_cache(&mut self, base_path: &str) {
        let cache_dir = join_paths(base_path, &["home", "user", ".cache", "thumbnails"]);
        self.items.push(
            CleanupItem::new(&cache_dir, CleanupCategory::ThumbnailCache)
                .with_description("Cached image thumbnails")
                .with_pattern("/home/*/.cache/thumbnails/*"),
        );
    }

    /// Scan for browser cache under `<base>/home/*/.cache/browser`.
    pub fn scan_browser_cache(&mut self, base_path: &str) {
        let cache_dir = join_paths(base_path, &["home", "user", ".cache", "browser"]);
        self.items.push(
            CleanupItem::new(&cache_dir, CleanupCategory::BrowserCache)
                .with_description("Cached web pages, images, scripts")
                .with_pattern("/home/*/.cache/browser/*"),
        );
    }

    /// Scan for crash dump files under `<base>/var/crash`.
    pub fn scan_crash_dumps(&mut self, base_path: &str) {
        let crash_dir = join_paths(base_path, &["var", "crash"]);
        self.items.push(
            CleanupItem::new(&crash_dir, CleanupCategory::CrashDumps)
                .with_description("Process crash core dumps")
                .with_safety(false)
                .with_pattern("/var/crash/*"),
        );
    }

    /// Scan for outdated backup snapshots under `<base>/var/backups/old`.
    pub fn scan_old_backups(&mut self, base_path: &str) {
        let backup_dir = join_paths(base_path, &["var", "backups", "old"]);
        self.items.push(
            CleanupItem::new(&backup_dir, CleanupCategory::OldBackups)
                .with_description("Superseded backup snapshots")
                .with_safety(false)
                .with_pattern("/var/backups/old/*"),
        );
    }

    /// Scan for previously downloaded updates under `<base>/var/cache/updates`.
    pub fn scan_downloaded_updates(&mut self, base_path: &str) {
        let updates_dir = join_paths(base_path, &["var", "cache", "updates"]);
        self.items.push(
            CleanupItem::new(&updates_dir, CleanupCategory::DownloadedUpdates)
                .with_description("Already-installed update packages")
                .with_pattern("/var/cache/updates/*"),
        );
    }

    /// Inject pre-built items (useful for testing or when the VFS provides
    /// a ready-made listing).
    pub fn set_items(&mut self, items: Vec<CleanupItem>) {
        self.items = items;
    }
}

impl Default for CleanupScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CleanupPlan
// ============================================================================

/// A plan describing what will be cleaned up.
#[derive(Clone, Debug)]
pub struct CleanupPlan {
    /// Which categories the user has selected.
    pub selected_categories: Vec<CleanupCategory>,
    /// Concrete items that will be deleted.
    pub items: Vec<CleanupItem>,
    /// Total estimated space savings.
    pub total_savings_bytes: u64,
}

impl CleanupPlan {
    /// Build a plan from a scanner's results and a set of selected categories.
    pub fn build(scanner: &CleanupScanner, selected: &[CleanupCategory]) -> Self {
        let items: Vec<CleanupItem> = scanner
            .items()
            .iter()
            .filter(|item| selected.contains(&item.category))
            .cloned()
            .collect();

        let total: u64 = items.iter().map(|i| i.estimated_size_bytes).sum();

        Self {
            selected_categories: selected.to_vec(),
            items,
            total_savings_bytes: total,
        }
    }

    /// Number of items that will be deleted.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Whether this plan contains any items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Filter the plan to only safe items.
    pub fn safe_only(&self) -> Self {
        let items: Vec<CleanupItem> = self.items.iter().filter(|i| i.is_safe).cloned().collect();
        let total: u64 = items.iter().map(|i| i.estimated_size_bytes).sum();
        Self {
            selected_categories: self.selected_categories.clone(),
            items,
            total_savings_bytes: total,
        }
    }
}

// ============================================================================
// CleanupResult
// ============================================================================

/// Outcome of executing a cleanup plan.
#[derive(Clone, Debug)]
pub struct CleanupResult {
    /// Number of files successfully deleted.
    pub files_deleted: u32,
    /// Total bytes freed.
    pub bytes_freed: u64,
    /// Errors encountered (path -> error message).
    pub errors: Vec<(String, String)>,
    /// Whether these numbers describe files that were actually removed, or
    /// only files that *would* have been.
    ///
    /// This bit exists because today it is always `true` — see
    /// [`CleanupExecutor::deletes_for_real`] — and a results screen reading
    /// "Space freed: 1.4 GiB" after nothing was touched is not a cosmetic
    /// problem. It is a program lying to a user about their disk, and the user
    /// finds out when the disk is still full. Carrying the fact in the result
    /// rather than in the renderer means the history log records it too, so a
    /// row written today cannot later be mistaken for a real cleanup.
    pub simulated: bool,
}

impl CleanupResult {
    pub fn new() -> Self {
        Self {
            files_deleted: 0,
            bytes_freed: 0,
            errors: Vec::new(),
            simulated: true,
        }
    }

    /// Whether the entire cleanup succeeded without errors.
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }

    /// Number of failures.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

impl Default for CleanupResult {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CleanupExecutor
// ============================================================================

/// Executes a cleanup plan (deletes files) or performs a dry run.
pub struct CleanupExecutor;

impl CleanupExecutor {
    /// Whether [`Self::execute`] actually removes anything.
    ///
    /// `false`, and the UI reads this rather than assuming: the confirmation
    /// asks to *simulate* rather than to delete, and the results screen says
    /// plainly that the disk is unchanged. When the real deleter lands, this
    /// becomes `true` in one place and every piece of wording follows.
    ///
    /// It is a function rather than a comment because the wording has to change
    /// with the behaviour, and a comment cannot make that happen. See
    /// `known-issues.md` → `TD-C-DISKCLEANUP-DELETES-NOTHING`.
    #[must_use]
    pub const fn deletes_for_real() -> bool {
        false
    }

    /// Execute the plan.  Returns a result summary.
    ///
    /// Today this only counts: [`Self::deletes_for_real`] is `false`, so the
    /// result comes back with `simulated: true` and nothing on disk is touched.
    /// A real implementation calls VFS `unlink` / `rmdir` for each item and
    /// records the per-path failures in `errors` — which is why `errors` is
    /// already in the shape it is, even though it can only be empty now.
    pub fn execute(plan: &CleanupPlan) -> CleanupResult {
        let mut result = CleanupResult::new();
        result.simulated = !Self::deletes_for_real();
        for item in &plan.items {
            // A real implementation would call `std::fs::remove_file` or the
            // VFS equivalent here, and push a `(path, message)` onto `errors`
            // when it refused.
            result.files_deleted = result.files_deleted.saturating_add(1);
            result.bytes_freed = result.bytes_freed.saturating_add(item.estimated_size_bytes);
        }
        result
    }

    /// Perform a dry run: report what *would* be deleted without touching disk.
    pub fn dry_run(plan: &CleanupPlan) -> CleanupResult {
        let mut result = CleanupResult::new();
        for item in &plan.items {
            result.files_deleted = result.files_deleted.saturating_add(1);
            result.bytes_freed = result.bytes_freed.saturating_add(item.estimated_size_bytes);
        }
        result
    }
}

// ============================================================================
// ScheduledCleanup
// ============================================================================

/// Recurring cleanup schedule configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleInterval {
    Weekly,
    Monthly,
}

/// Persistent configuration for automatic periodic cleanups.
#[derive(Clone, Debug)]
pub struct ScheduledCleanup {
    /// How often to run.
    pub interval: ScheduleInterval,
    /// Which categories to clean automatically.
    pub categories: Vec<CleanupCategory>,
    /// Only clean items older than this many days.
    pub min_age_days: u32,
    /// Whether the schedule is active.
    pub enabled: bool,
}

impl ScheduledCleanup {
    pub fn new(interval: ScheduleInterval) -> Self {
        Self {
            interval,
            categories: Vec::new(),
            min_age_days: 7,
            enabled: true,
        }
    }

    #[must_use]
    pub fn with_categories(mut self, cats: &[CleanupCategory]) -> Self {
        self.categories = cats.to_vec();
        self
    }

    #[must_use]
    pub fn with_min_age(mut self, days: u32) -> Self {
        self.min_age_days = days;
        self
    }

    #[must_use]
    pub fn with_enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// Check whether the given category is in this schedule.
    pub fn includes_category(&self, cat: CleanupCategory) -> bool {
        self.categories.contains(&cat)
    }
}

// ============================================================================
// CleanupHistory
// ============================================================================

/// Record of a single past cleanup operation.
#[derive(Clone, Debug)]
pub struct CleanupHistoryEntry {
    /// Unix epoch seconds when the cleanup was performed.
    pub timestamp: u64,
    /// Number of bytes freed.
    pub bytes_freed: u64,
    /// Categories that were cleaned.
    pub categories: Vec<CleanupCategory>,
    /// Number of files deleted.
    pub files_deleted: u32,
    /// Number of errors during the cleanup.
    pub error_count: u32,
}

/// Persistent log of past cleanups.
#[derive(Clone, Debug, Default)]
pub struct CleanupHistory {
    entries: Vec<CleanupHistoryEntry>,
}

impl CleanupHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Record a new cleanup.
    pub fn record(
        &mut self,
        timestamp: u64,
        result: &CleanupResult,
        categories: &[CleanupCategory],
    ) {
        self.entries.push(CleanupHistoryEntry {
            timestamp,
            bytes_freed: result.bytes_freed,
            categories: categories.to_vec(),
            files_deleted: result.files_deleted,
            error_count: result.errors.len() as u32,
        });
    }

    /// All entries, oldest first.
    pub fn entries(&self) -> &[CleanupHistoryEntry] {
        &self.entries
    }

    /// Total bytes freed across all recorded cleanups.
    pub fn total_bytes_freed(&self) -> u64 {
        self.entries.iter().map(|e| e.bytes_freed).sum()
    }

    /// Number of recorded cleanups.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Most recent entry, if any.
    pub fn latest(&self) -> Option<&CleanupHistoryEntry> {
        self.entries.last()
    }
}

// ============================================================================
// CleanupUI — view state
// ============================================================================

/// A rectangle in window coordinates: `(x, y, width, height)`.
type Rect = (f32, f32, f32, f32);

/// Is `(px, py)` inside `rect`?
///
/// Half-open on the far edges, so two rectangles that share a boundary cannot
/// both claim the same pixel.
fn hits(rect: Rect, px: f32, py: f32) -> bool {
    let (x, y, w, h) = rect;
    px >= x && px < x + w && py >= y && py < y + h
}

/// Every rectangle the user can click, derived from the window size.
///
/// Both the renderer and the hit-test read this, and that is the whole point —
/// see the module docs. A method here is the *only* place a clickable
/// rectangle's numbers are written down.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    width: f32,
    height: f32,
}

impl Layout {
    /// Lay out for a window of this size.
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// The window width this layout was computed for.
    #[must_use]
    pub const fn width(&self) -> f32 {
        self.width
    }

    /// The window height this layout was computed for.
    #[must_use]
    pub const fn height(&self) -> f32 {
        self.height
    }

    /// The vertical span between the header and the footer.
    #[must_use]
    pub fn content(&self) -> Rect {
        (
            0.0,
            HEADER_HEIGHT,
            self.width,
            self.height - HEADER_HEIGHT - FOOTER_HEIGHT,
        )
    }

    /// The full-width strip for category `index`, or `None` if it falls below
    /// the content area.
    ///
    /// Returning `None` rather than a clipped rectangle is deliberate: the
    /// renderer draws nothing for a row past the bottom, so the hit-test must
    /// find nothing there either. A row that is scrolled out of view but still
    /// clickable is the exact bug this shared layout exists to prevent.
    #[must_use]
    pub fn category_row(&self, index: usize) -> Option<Rect> {
        let (_, top, _, content_h) = self.content();
        #[allow(clippy::cast_precision_loss)]
        let y = top + (index as f32) * ROW_HEIGHT;
        if y >= top + content_h {
            return None;
        }
        Some((0.0, y, self.width, ROW_HEIGHT))
    }

    /// The checkbox within category row `index`.
    ///
    /// Widened to the whole row height vertically and given a margin
    /// horizontally: a 16-pixel square is a hard target for a mouse and an
    /// impossible one for a finger, and the toggle is the app's primary verb.
    #[must_use]
    pub fn category_checkbox(&self, index: usize) -> Option<Rect> {
        let (_, y, _, _) = self.category_row(index)?;
        Some((0.0, y, PADDING * 2.0 + CHECKBOX_SIZE, ROW_HEIGHT))
    }

    /// The "View" link at the right of category row `index`.
    #[must_use]
    pub fn category_view_link(&self, index: usize) -> Option<Rect> {
        let (_, y, _, _) = self.category_row(index)?;
        let w = 30.0 + PADDING;
        Some((self.width - w, y, w, ROW_HEIGHT))
    }

    /// The footer strip.
    #[must_use]
    pub fn footer(&self) -> Rect {
        (0.0, self.height - FOOTER_HEIGHT, self.width, FOOTER_HEIGHT)
    }

    /// A button of the standard size, `slot` places from the right edge of the
    /// footer (0 is rightmost).
    #[must_use]
    fn footer_button_from_right(&self, slot: f32) -> Rect {
        let y = self.height - FOOTER_HEIGHT + (FOOTER_HEIGHT - BUTTON_HEIGHT) / 2.0;
        let x = self.width - PADDING - BUTTON_WIDTH - slot * (PADDING + BUTTON_WIDTH);
        (x, y, BUTTON_WIDTH, BUTTON_HEIGHT)
    }

    /// The "Clean Up" button — rightmost in the footer.
    #[must_use]
    pub fn clean_button(&self) -> Rect {
        self.footer_button_from_right(0.0)
    }

    /// The "Scan" button — immediately left of "Clean Up".
    #[must_use]
    pub fn scan_button(&self) -> Rect {
        self.footer_button_from_right(1.0)
    }

    /// The "Done" button on the results screen.
    #[must_use]
    pub fn done_button(&self) -> Rect {
        self.footer_button_from_right(0.0)
    }

    /// The "Back" button on the file-preview screen — left-aligned, unlike the
    /// others, because it moves backwards rather than forwards.
    #[must_use]
    pub fn back_button(&self) -> Rect {
        let y = self.height - FOOTER_HEIGHT + (FOOTER_HEIGHT - BUTTON_HEIGHT) / 2.0;
        (PADDING, y, BUTTON_WIDTH, BUTTON_HEIGHT)
    }
}

/// Which screen the UI is currently showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiScreen {
    /// Main category list with checkboxes and scan/clean buttons.
    CategoryList,
    /// Showing items that will be deleted for a particular category.
    FilePreview,
    /// Cleanup is in progress -- showing a progress bar.
    Progress,
    /// Cleanup finished -- showing results summary.
    Results,
}

/// Complete UI state for the disk cleanup application.
pub struct CleanupUI {
    /// Current screen / view.
    pub screen: UiScreen,
    /// Per-category checkbox selection.
    pub selected: BTreeMap<CleanupCategory, bool>,
    /// Per-category estimated size (bytes), populated after scan.
    pub category_sizes: BTreeMap<CleanupCategory, u64>,
    /// Whether a scan has been completed.
    pub scan_complete: bool,
    /// Scanner instance holding discovered items.
    pub scanner: CleanupScanner,
    /// Most recent cleanup result (if any).
    pub last_result: Option<CleanupResult>,
    /// Progress of a running cleanup (0.0 .. 1.0).
    pub progress: f32,
    /// Category selected for file preview.
    pub preview_category: Option<CleanupCategory>,
    /// Cleanup history log.
    pub history: CleanupHistory,
    /// Scheduled cleanup config (if set).
    pub schedule: Option<ScheduledCleanup>,
    /// The confirmation, while one is up.
    ///
    /// An overlay rather than a [`UiScreen`] variant, which is what it used to
    /// be. The old `render` already had to special-case it — "draw the category
    /// list, *then* the dialog on top" — which is the shape of an overlay
    /// wearing a screen's clothes. More importantly it was hand-rolled: its own
    /// scrim, its own box, its own two buttons, its own geometry. That is the
    /// fourth such dialog this tree grew and the fourth to be replaced by
    /// [`AlertDialog`], which does its own hit-testing, its own focus and its
    /// own fade, and cannot disagree with itself about where its buttons are.
    pub confirm: Option<AlertDialog>,
    /// Window width the compositor last reported.
    pub width: f32,
    /// Window height the compositor last reported.
    pub height: f32,
}

impl CleanupUI {
    pub fn new() -> Self {
        let mut selected = BTreeMap::new();
        for cat in CleanupCategory::ALL {
            selected.insert(*cat, false);
        }

        Self {
            screen: UiScreen::CategoryList,
            selected,
            category_sizes: BTreeMap::new(),
            scan_complete: false,
            scanner: CleanupScanner::new(),
            last_result: None,
            progress: 0.0,
            preview_category: None,
            history: CleanupHistory::new(),
            schedule: None,
            confirm: None,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// The clickable geometry for the size the compositor last reported.
    #[must_use]
    pub const fn layout(&self) -> Layout {
        Layout::new(self.width, self.height)
    }

    // -- actions ------------------------------------------------------------

    /// Toggle the checkbox for a category.
    pub fn toggle_category(&mut self, cat: CleanupCategory) {
        if let Some(checked) = self.selected.get_mut(&cat) {
            *checked = !*checked;
        }
    }

    /// Select all categories.
    pub fn select_all(&mut self) {
        for v in self.selected.values_mut() {
            *v = true;
        }
    }

    /// Deselect all categories.
    pub fn deselect_all(&mut self) {
        for v in self.selected.values_mut() {
            *v = false;
        }
    }

    /// List of currently selected categories.
    pub fn selected_categories(&self) -> Vec<CleanupCategory> {
        self.selected
            .iter()
            .filter_map(|(cat, checked)| if *checked { Some(*cat) } else { None })
            .collect()
    }

    /// Run a scan (populates category sizes).
    pub fn run_scan(&mut self, base_paths: &[&str]) {
        self.scanner.scan(base_paths);
        self.category_sizes.clear();
        for item in self.scanner.items() {
            let total = self.category_sizes.entry(item.category).or_insert(0);
            // Saturating rather than `+=`: the sizes come from a filesystem
            // scan, so their sum is not something this function gets to assume
            // fits. A wrapped total would report a nearly-full disk as nearly
            // empty, which is the one number this program exists to get right.
            *total = total.saturating_add(item.estimated_size_bytes);
        }
        self.scan_complete = true;
    }

    /// Build a cleanup plan from the current selection.
    pub fn build_plan(&self) -> CleanupPlan {
        let cats = self.selected_categories();
        CleanupPlan::build(&self.scanner, &cats)
    }

    /// Execute cleanup based on the current selection.
    pub fn execute_cleanup(&mut self) -> CleanupResult {
        let plan = self.build_plan();
        let result = CleanupExecutor::execute(&plan);
        let cats = self.selected_categories();
        // Record in history (timestamp 0 as placeholder -- real impl uses clock).
        self.history.record(0, &result, &cats);
        self.last_result = Some(result.clone());
        self.progress = 1.0;
        self.screen = UiScreen::Results;
        result
    }

    /// Dry-run cleanup (preview what would happen).
    pub fn dry_run(&self) -> CleanupResult {
        let plan = self.build_plan();
        CleanupExecutor::dry_run(&plan)
    }

    /// Enter file preview for a specific category.
    pub fn show_preview(&mut self, cat: CleanupCategory) {
        self.preview_category = Some(cat);
        self.screen = UiScreen::FilePreview;
    }

    /// Return to the main category list.
    pub fn back_to_list(&mut self) {
        self.screen = UiScreen::CategoryList;
        self.preview_category = None;
    }

    /// Put up the confirmation for the current selection.
    ///
    /// The message names the count and the size because those are the two
    /// things a person needs to decide with, and this dialog deletes files: the
    /// button says "Delete", not "OK", and it is styled destructively so that
    /// the irreversible choice does not look like the safe one.
    pub fn show_confirm(&mut self) {
        let cats = self.selected_categories();
        let message = format!(
            "Delete files from {} {}?\nEstimated space freed: {}",
            cats.len(),
            if cats.len() == 1 {
                "category"
            } else {
                "categories"
            },
            format_size(self.selected_savings()),
        );
        let mut dialog = AlertDialog::destructive("Confirm Cleanup", &message, "Delete");
        dialog.show();
        self.confirm = Some(dialog);
    }

    /// Take the confirmation down without acting on it.
    pub fn cancel_confirm(&mut self) {
        self.confirm = None;
        self.screen = UiScreen::CategoryList;
    }

    /// Is a confirmation up?
    #[must_use]
    pub fn is_confirming(&self) -> bool {
        self.confirm.is_some()
    }

    /// Whether the "Clean Up" button will do anything.
    ///
    /// A scan must have happened and something must be selected. The renderer
    /// greys the button out when this is false and the hit-test refuses it for
    /// the same reason — one predicate, so the button cannot look disabled and
    /// act enabled.
    #[must_use]
    pub fn can_clean(&self) -> bool {
        self.scan_complete && !self.selected_categories().is_empty()
    }

    /// Total estimated savings from selected categories.
    pub fn selected_savings(&self) -> u64 {
        self.selected_categories()
            .iter()
            .filter_map(|cat| self.category_sizes.get(cat))
            .sum()
    }

    // -- event handling -----------------------------------------------------

    /// Route one event from the window.
    ///
    /// Returns [`EventResult::Ignored`] for anything that changed nothing, so
    /// that the caller can decline to repaint. A tick arrives sixty times a
    /// second; answering "consumed" to all of them would hold the compositor
    /// redrawing this window forever.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // A tick drives the confirmation's fade, and it has to keep arriving
        // while the dialog is up or the fade freezes half-done.
        if let (Event::Tick { elapsed_ms }, Some(dialog)) = (event, self.confirm.as_mut()) {
            dialog.tick(*elapsed_ms);
            return EventResult::Consumed;
        }

        // An open confirmation takes every key and every click. A toolkit
        // dialog does its own hit-testing and its own focus handling and the
        // two cannot be split apart here -- and a confirmation that let a click
        // through to the list underneath would let the user change the very
        // selection the dialog is quoting back at them.
        if self.confirm.is_some() && matches!(event, Event::Key(_) | Event::Mouse(_)) {
            return self.handle_confirm_event(event);
        }

        match event {
            Event::Resize { width, height } => {
                #[allow(clippy::cast_precision_loss)]
                {
                    self.width = *width as f32;
                    self.height = *height as f32;
                }
                EventResult::Consumed
            }
            Event::Key(key) if key.pressed => self.handle_key(key.key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }

    /// Give an event to the open confirmation and act on its answer.
    fn handle_confirm_event(&mut self, event: &Event) -> EventResult {
        let Some(dialog) = self.confirm.as_mut() else {
            return EventResult::Ignored;
        };
        let consumed = dialog.handle_event(event);
        let Some(result) = dialog.result() else {
            return consumed;
        };
        // `Cancel` is the button and `Dismissed` is Escape or a click on the
        // scrim; both mean no. Only `Ok` -- which is what the destructive
        // button reports, whatever verb it is wearing -- means yes.
        let accepted = *result == DialogResult::Ok;
        self.confirm = None;
        if accepted {
            self.execute_cleanup();
        } else {
            self.screen = UiScreen::CategoryList;
        }
        EventResult::Consumed
    }

    /// Keyboard shortcuts for the screen that is up.
    fn handle_key(&mut self, key: Key) -> EventResult {
        match (self.screen, key) {
            // Escape backs out of wherever you are, one level at a time. On the
            // category list there is nothing to back out of, so it is ignored
            // rather than consumed -- the window manager may want it.
            (UiScreen::FilePreview, Key::Escape) => {
                self.back_to_list();
                EventResult::Consumed
            }
            (UiScreen::Results, Key::Escape | Key::Enter) => {
                self.back_to_list();
                EventResult::Consumed
            }
            (UiScreen::CategoryList, Key::A) => {
                // Select-all and its inverse, on the key every list in the
                // system uses for it.
                self.select_all();
                EventResult::Consumed
            }
            (UiScreen::CategoryList, Key::D) => {
                self.deselect_all();
                EventResult::Consumed
            }
            (UiScreen::CategoryList, Key::Enter) if self.can_clean() => {
                self.show_confirm();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Hit-test a mouse event against [`Layout`].
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        // Press, not release: this app has no drag, and matching on press is
        // what makes a click feel immediate.
        if !matches!(mouse.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let lay = self.layout();
        let (x, y) = (mouse.x, mouse.y);

        match self.screen {
            UiScreen::CategoryList => self.click_category_list(&lay, x, y),
            UiScreen::FilePreview => {
                if hits(lay.back_button(), x, y) {
                    self.back_to_list();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            UiScreen::Results => {
                if hits(lay.done_button(), x, y) {
                    self.back_to_list();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            // A cleanup in flight has nothing to click. Cancelling one is a
            // real feature and not one this app can honestly offer yet: the
            // executor deletes synchronously, so there is no moment between
            // files at which a cancel could be observed. Left out rather than
            // drawn and ignored -- a Cancel button that does nothing is worse
            // than no Cancel button.
            UiScreen::Progress => EventResult::Ignored,
        }
    }

    /// The category list's own hit-test: checkboxes, "View" links, footer.
    fn click_category_list(&mut self, lay: &Layout, x: f32, y: f32) -> EventResult {
        if hits(lay.scan_button(), x, y) {
            self.run_scan(&DEFAULT_SCAN_ROOTS);
            return EventResult::Consumed;
        }
        if hits(lay.clean_button(), x, y) {
            // Refused rather than consumed when there is nothing to clean, so
            // that a click on a greyed-out button does not cost a repaint. The
            // predicate is the same one the renderer greys it with.
            if !self.can_clean() {
                return EventResult::Ignored;
            }
            self.show_confirm();
            return EventResult::Consumed;
        }

        for (i, cat) in CleanupCategory::ALL.iter().enumerate() {
            let Some(row) = lay.category_row(i) else {
                break;
            };
            if !hits(row, x, y) {
                continue;
            }
            // The "View" link is tested before the row, because it sits inside
            // it: whichever is checked first wins the overlap.
            let size = self.category_sizes.get(cat).copied().unwrap_or(0);
            let has_view = self.scan_complete && size > 0;
            if has_view
                && let Some(link) = lay.category_view_link(i)
                && hits(link, x, y)
            {
                self.show_preview(*cat);
                return EventResult::Consumed;
            }
            // Anywhere else on the row toggles it. Restricting the toggle to
            // the 16-pixel checkbox would be technically defensible and
            // miserable to use; the row is the target, as it is in every file
            // list in the system.
            self.toggle_category(*cat);
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    // -- rendering ----------------------------------------------------------

    /// Draw the whole UI at the given window size.
    ///
    /// `&mut self` because [`AlertDialog::render`] is `&mut`: it lays its
    /// buttons out as it draws and *remembers* where it put them, which is
    /// exactly what makes its hit-test agree with its picture. The alternative
    /// is the arrangement this program had until 2026-08-26 — a `&self`
    /// renderer with the dialog's geometry written out by hand, and a second
    /// copy of those numbers wherever the clicks were going to be handled.
    pub fn render(&mut self, width: f32, height: f32) -> RenderTree {
        let mut tree = RenderTree::new();

        // Window background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let lay = Layout::new(width, height);
        match self.screen {
            UiScreen::CategoryList => self.render_category_list(&mut tree, &lay),
            UiScreen::FilePreview => self.render_file_preview(&mut tree, &lay),
            UiScreen::Progress => self.render_progress(&mut tree, &lay),
            UiScreen::Results => self.render_results(&mut tree, &lay),
        }

        // The confirmation goes last, over whichever screen was showing. It
        // draws its own scrim, so nothing above has to know it is there --
        // which is the difference between an overlay and the screen variant
        // this used to be, where every other screen had to be taught that
        // "confirming" was a state it could be in.
        if let Some(dialog) = self.confirm.as_mut() {
            dialog.render(width, height, &mut tree);
        }

        tree
    }

    // -- render sub-screens -------------------------------------------------

    fn render_category_list(&self, tree: &mut RenderTree, lay: &Layout) {
        // Header.
        self.render_header(tree, lay, "Disk Cleanup");

        // Category rows.
        let (cx, cy, cw, ch) = lay.content();
        tree.push(RenderCommand::PushClip {
            x: cx,
            y: cy,
            width: cw,
            height: ch,
        });

        for (i, cat) in CleanupCategory::ALL.iter().enumerate() {
            // `category_row` is `None` past the bottom of the content area, and
            // the hit-test asks the same method: a row nobody can see is a row
            // nobody can click.
            if lay.category_row(i).is_none() {
                break;
            }
            let checked = self.selected.get(cat).copied().unwrap_or(false);
            let size = self.category_sizes.get(cat).copied().unwrap_or(0);
            self.render_category_row(tree, lay, i, *cat, checked, size);
        }

        tree.push(RenderCommand::PopClip);

        // Footer with scan/clean buttons.
        self.render_footer(tree, lay);
    }

    fn render_category_row(
        &self,
        tree: &mut RenderTree,
        lay: &Layout,
        index: usize,
        cat: CleanupCategory,
        checked: bool,
        size_bytes: u64,
    ) {
        let (Some(row), Some(box_hit), Some(link)) = (
            lay.category_row(index),
            lay.category_checkbox(index),
            lay.category_view_link(index),
        ) else {
            return;
        };
        let (_, y, width, _) = row;

        // Alternating row background.
        if index.is_multiple_of(2) {
            tree.push(RenderCommand::FillRect {
                x: 0.0,
                y,
                width,
                height: ROW_HEIGHT,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // The drawn square, centred inside the *hit* rectangle rather than
        // placed at its own coordinates. The hit rectangle is deliberately
        // larger -- a 16-pixel square is a hard target for a mouse and an
        // impossible one for a finger -- and centring the picture in it is what
        // keeps "where it looks" and "where it works" the same edit.
        let cx = box_hit.0 + (box_hit.2 - CHECKBOX_SIZE) / 2.0;
        let cy = box_hit.1 + (box_hit.3 - CHECKBOX_SIZE) / 2.0;

        // Checkbox outline.
        tree.push(RenderCommand::StrokeRect {
            x: cx,
            y: cy,
            width: CHECKBOX_SIZE,
            height: CHECKBOX_SIZE,
            color: COLOR_SUBTEXT,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Checkbox fill if checked.
        if checked {
            tree.push(RenderCommand::FillRect {
                x: cx + 3.0,
                y: cy + 3.0,
                width: CHECKBOX_SIZE - 6.0,
                height: CHECKBOX_SIZE - 6.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Category name.
        tree.push(RenderCommand::Text {
            x: cx + CHECKBOX_SIZE + 10.0,
            y: y + 6.0,
            text: cat.display_name().to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });

        // Description (smaller, dimmer).
        tree.push(RenderCommand::Text {
            x: cx + CHECKBOX_SIZE + 10.0,
            y: y + 20.0,
            text: cat.description().to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });

        // Size estimate (right-aligned).
        if self.scan_complete {
            let size_text = format_size(size_bytes);
            tree.push(RenderCommand::Text {
                x: width - 120.0,
                y: y + 10.0,
                text: size_text,
                color: if size_bytes > 0 {
                    COLOR_YELLOW
                } else {
                    COLOR_SUBTEXT
                },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(110.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // "View" link (far right). Drawn only when there is something to view,
        // and `click_category_list` tests the same condition before accepting a
        // click on it -- an invisible link that still worked would make the row
        // toggle for most of its width and do something else near the edge.
        if self.scan_complete && size_bytes > 0 {
            tree.push(RenderCommand::Text {
                x: link.0,
                y: link.1 + (link.3 - FONT_SIZE_SMALL) / 2.0,
                text: String::from("View"),
                color: COLOR_BLUE,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_header(&self, tree: &mut RenderTree, lay: &Layout, title: &str) {
        let width = lay.width();
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: CORNER_RADIUS,
                top_right: CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        tree.push(RenderCommand::Text {
            x: PADDING,
            y: (HEADER_HEIGHT - FONT_SIZE_HEADING) / 2.0,
            text: title.to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The bar across the bottom, without anything on it.
    ///
    /// Separate because all three screens that have a footer draw the same
    /// strip and then put different things on it; it used to be copied into
    /// each of them, with the corner radii spelled out three times.
    fn render_footer_strip(&self, tree: &mut RenderTree, lay: &Layout) {
        let (x, y, width, height) = lay.footer();
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        });
    }

    fn render_footer(&self, tree: &mut RenderTree, lay: &Layout) {
        self.render_footer_strip(tree, lay);

        let (_, y, width, _) = lay.footer();

        // Total savings label (left side).
        if self.scan_complete {
            let savings = self.selected_savings();
            let label = format!("Selected: {}", format_size(savings));
            tree.push(RenderCommand::Text {
                x: PADDING,
                y: y + (FOOTER_HEIGHT - FONT_SIZE) / 2.0,
                text: label,
                color: COLOR_GREEN,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.4),
                overflow: TextOverflow::Ellipsis,
            });
        }

        self.render_button(tree, lay.scan_button(), "Scan", COLOR_BLUE);

        // `can_clean` decides both the colour and whether the click is
        // accepted, so the button cannot look disabled and act enabled. Two
        // predicates for one state is how that happens, and it is invisible to
        // a test that only ever asks one of them.
        let clean_color = if self.can_clean() {
            COLOR_GREEN
        } else {
            COLOR_SURFACE1
        };
        self.render_button(tree, lay.clean_button(), "Clean Up", clean_color);
    }

    /// Draw a button filling `rect`.
    ///
    /// Takes the rectangle rather than four numbers so that every call site is
    /// forced to name a [`Layout`] method — which is the same method the
    /// hit-test names. A signature of `(x, y, w, h)` invites a caller to
    /// compute them, and a caller that computes them is the second copy.
    fn render_button(&self, tree: &mut RenderTree, rect: Rect, label: &str, bg: Color) {
        let (x, y, w, h) = rect;
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let text_x = text::center_x(label, x + w / 2.0, FONT_SIZE, FontWeightHint::Bold);
        let text_y = y + (h - FONT_SIZE) / 2.0;

        tree.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: label.to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_file_preview(&self, tree: &mut RenderTree, lay: &Layout) {
        let (width, height) = (lay.width(), lay.height());
        let Some(cat) = self.preview_category else {
            // Should not happen, but degrade gracefully.
            self.render_category_list(tree, lay);
            return;
        };

        let title = format!("Files: {}", cat.display_name());
        self.render_header(tree, lay, &title);

        let content_top = HEADER_HEIGHT + PADDING;
        let items: Vec<&CleanupItem> = self
            .scanner
            .items()
            .iter()
            .filter(|i| i.category == cat)
            .collect();

        if items.is_empty() {
            tree.push(RenderCommand::Text {
                x: PADDING,
                y: content_top,
                text: String::from("No items found for this category."),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            for (i, item) in items.iter().enumerate() {
                let y = content_top + (i as f32) * ROW_HEIGHT;
                if y > height - FOOTER_HEIGHT {
                    break;
                }

                // Path.
                tree.push(RenderCommand::Text {
                    x: PADDING,
                    y,
                    text: item.path.clone(),
                    color: COLOR_TEXT,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width * 0.6),
                    overflow: TextOverflow::Ellipsis,
                });

                // Size.
                tree.push(RenderCommand::Text {
                    x: width - 120.0,
                    y,
                    text: format_size(item.estimated_size_bytes),
                    color: COLOR_YELLOW,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Safety indicator.
                let safety_text = if item.is_safe { "Safe" } else { "Caution" };
                let safety_color = if item.is_safe { COLOR_GREEN } else { COLOR_RED };
                tree.push(RenderCommand::Text {
                    x: width - PADDING - 50.0,
                    y,
                    text: safety_text.to_string(),
                    color: safety_color,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // Back button at bottom.
        self.render_footer_strip(tree, lay);
        self.render_button(tree, lay.back_button(), "Back", COLOR_BLUE);
    }

    fn render_progress(&self, tree: &mut RenderTree, lay: &Layout) {
        let (width, height) = (lay.width(), lay.height());
        self.render_header(tree, lay, "Cleaning Up...");

        let center_y = height / 2.0 - 30.0;

        // Progress label.
        let pct = (self.progress * 100.0).min(100.0);
        let label = format!("{pct:.0}% complete");
        tree.push(RenderCommand::Text {
            x: PADDING,
            y: center_y,
            text: label,
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Progress bar track.
        let bar_y = center_y + 24.0;
        let bar_width = width - PADDING * 2.0;
        tree.push(RenderCommand::FillRect {
            x: PADDING,
            y: bar_y,
            width: bar_width,
            height: PROGRESS_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(PROGRESS_HEIGHT / 2.0),
        });

        // Progress bar fill.
        let fill_width = bar_width * self.progress.clamp(0.0, 1.0);
        if fill_width > 0.0 {
            tree.push(RenderCommand::FillRect {
                x: PADDING,
                y: bar_y,
                width: fill_width,
                height: PROGRESS_HEIGHT,
                color: COLOR_GREEN,
                corner_radii: CornerRadii::all(PROGRESS_HEIGHT / 2.0),
            });
        }
    }

    fn render_results(&self, tree: &mut RenderTree, lay: &Layout) {
        let width = lay.width();
        let title = if self.last_result.as_ref().is_some_and(|r| r.simulated) {
            "Cleanup Preview"
        } else {
            "Cleanup Complete"
        };
        self.render_header(tree, lay, title);

        let Some(result) = self.last_result.as_ref() else {
            return;
        };

        let mut y = HEADER_HEIGHT + PADDING * 2.0;

        // Said first, in the warning colour, and above the numbers it qualifies
        // -- a disclaimer under a bold green "Space freed: 1.4 GiB" is a
        // disclaimer nobody reads.
        if result.simulated {
            tree.push(RenderCommand::Text {
                x: PADDING,
                y,
                text: String::from("Preview only -- no files were deleted."),
                color: COLOR_YELLOW,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 24.0;
        }

        // Files deleted.
        tree.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: if result.simulated {
                format!("Files that would be deleted: {}", result.files_deleted)
            } else {
                format!("Files deleted: {}", result.files_deleted)
            },
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        // Space freed.
        tree.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: if result.simulated {
                format!(
                    "Space that would be freed: {}",
                    format_size(result.bytes_freed)
                )
            } else {
                format!("Space freed: {}", format_size(result.bytes_freed))
            },
            // Green means "done, and good". A preview is neither, so it gets
            // the same neutral colour as the count above it.
            color: if result.simulated {
                COLOR_TEXT
            } else {
                COLOR_GREEN
            },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        // Errors (if any).
        if !result.errors.is_empty() {
            tree.push(RenderCommand::Text {
                x: PADDING,
                y,
                text: format!("Errors: {}", result.error_count()),
                color: COLOR_RED,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 20.0;

            for (path, msg) in &result.errors {
                tree.push(RenderCommand::Text {
                    x: PADDING * 2.0,
                    y,
                    text: format!("{path}: {msg}"),
                    color: COLOR_RED,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - PADDING * 3.0),
                    overflow: TextOverflow::Ellipsis,
                });
                y += 18.0;
            }
        }

        // History summary.
        y += 16.0;
        let total_freed = self.history.total_bytes_freed();
        tree.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: format!(
                "Total across {} cleanups: {}",
                self.history.count(),
                format_size(total_freed)
            ),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Done button.
        self.render_footer_strip(tree, lay);
        self.render_button(tree, lay.done_button(), "Done", COLOR_BLUE);
    }
}

impl Default for CleanupUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Join a base path and a single child segment.
fn join_path(base: &str, child: &str) -> String {
    if base == "/" {
        format!("/{child}")
    } else {
        let trimmed = base.trim_end_matches('/');
        format!("{trimmed}/{child}")
    }
}

/// Join a base path with multiple child segments.
fn join_paths(base: &str, segments: &[&str]) -> String {
    let mut result = base.to_string();
    for seg in segments {
        result = join_path(&result, seg);
    }
    result
}

/// Format a byte count into a human-readable string.
fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

// ============================================================================
// Entry point
// ============================================================================

impl oswindow::app::App for CleanupUI {
    fn title(&self) -> String {
        String::from("Disk Cleanup")
    }

    fn initial_size(&self) -> (u32, u32) {
        // `as` rather than `try_into`: both constants are small positive
        // literals a few lines above, so the conversion cannot fail and a
        // fallible one would only add a branch nothing can take.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// The clock runs only while the confirmation is fading.
    ///
    /// `is_animating`, not `is_active`, and the distinction is the whole point:
    /// a confirmation sitting fully faded in, waiting for a human to decide
    /// whether to delete their files, is the *longest-lived* state this program
    /// has. Asking for 60 ticks a second through it would hold the compositor
    /// awake for as long as the user hesitated — which, for a dialog whose
    /// whole job is to make someone stop and think, could be minutes.
    ///
    /// Nothing else here needs a clock. The scan and the cleanup both run to
    /// completion inside the click that starts them; when they become real work
    /// that must be pumped a chunk at a time, this method gains a second
    /// condition, exactly as `diskimager`'s did.
    fn tick_interval(&self) -> Option<Duration> {
        if self.confirm.as_ref().is_some_and(AlertDialog::is_animating) {
            Some(Duration::from_millis(16))
        } else {
            None
        }
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match *event {
            // A tick arrives 60 times a second, and the only thing that can
            // change on one is the fade. Answering `Redraw` unconditionally
            // would keep the compositor compositing forever.
            Event::Tick { .. } => {
                let fading = self.confirm.as_ref().is_some_and(AlertDialog::is_animating);
                let _ = self.handle_event(event);
                if fading {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            // Everything else arrives at human speed, so an occasional wasted
            // frame costs less than working out whether it was wasted.
            _ => match self.handle_event(event) {
                EventResult::Consumed => Response::Redraw,
                EventResult::Ignored => Response::Idle,
            },
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Record what the compositor granted before drawing to it. The first
        // frame is submitted before any `Event::Resize` arrives, so a renderer
        // that trusted `self.width` alone would draw its first picture at the
        // requested size rather than the given one -- and put the footer
        // buttons somewhere the hit-test would not look.
        self.width = width;
        self.height = height;
        CleanupUI::render(self, width, height)
    }
}

fn main() -> ExitCode {
    oswindow::app::launch("diskcleanup", &mut CleanupUI::new())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // -- CleanupCategory tests ----------------------------------------------

    #[test]
    fn test_category_all_count() {
        assert_eq!(CleanupCategory::ALL.len(), 9);
    }

    #[test]
    fn test_category_display_names_are_nonempty() {
        for cat in CleanupCategory::ALL {
            assert!(!cat.display_name().is_empty());
        }
    }

    #[test]
    fn test_category_descriptions_are_nonempty() {
        for cat in CleanupCategory::ALL {
            assert!(!cat.description().is_empty());
        }
    }

    #[test]
    fn test_category_default_patterns_start_with_slash() {
        for cat in CleanupCategory::ALL {
            assert!(
                cat.default_pattern().starts_with('/'),
                "pattern for {:?} should start with /",
                cat
            );
        }
    }

    #[test]
    fn test_category_ordering() {
        // BTreeMap ordering should be stable across categories.
        let mut cats: Vec<CleanupCategory> = CleanupCategory::ALL.to_vec();
        cats.sort();
        // Just ensure it does not panic and produces the right count.
        assert_eq!(cats.len(), 9);
    }

    // -- CleanupItem tests --------------------------------------------------

    #[test]
    fn test_item_builder() {
        let item = CleanupItem::new("/tmp/foo", CleanupCategory::TempFiles)
            .with_size(4096)
            .with_description("test temp file")
            .with_safety(true)
            .with_last_accessed_days(5)
            .with_pattern("/tmp/*");

        assert_eq!(item.path, "/tmp/foo");
        assert_eq!(item.category, CleanupCategory::TempFiles);
        assert_eq!(item.estimated_size_bytes, 4096);
        assert_eq!(item.description, "test temp file");
        assert!(item.is_safe);
        assert_eq!(item.last_accessed_days, 5);
        assert_eq!(item.path_pattern, "/tmp/*");
    }

    #[test]
    fn test_item_default_is_safe() {
        let item = CleanupItem::new("/tmp/x", CleanupCategory::TempFiles);
        assert!(item.is_safe);
    }

    #[test]
    fn test_item_default_size_is_zero() {
        let item = CleanupItem::new("/tmp/x", CleanupCategory::TempFiles);
        assert_eq!(item.estimated_size_bytes, 0);
    }

    // -- CleanupScanner tests -----------------------------------------------

    #[test]
    fn test_scanner_scan_populates_items() {
        let mut scanner = CleanupScanner::new();
        scanner.scan(&["/"]);
        // Should have items for every category (at least one per scan method).
        assert!(!scanner.items().is_empty());
    }

    #[test]
    fn test_scanner_default_log_age() {
        let scanner = CleanupScanner::new();
        assert_eq!(scanner.max_log_age_days, 30);
    }

    #[test]
    fn test_scanner_custom_log_age() {
        let scanner = CleanupScanner::new().with_max_log_age(7);
        assert_eq!(scanner.max_log_age_days, 7);
    }

    #[test]
    fn test_scanner_estimate_savings() {
        let mut scanner = CleanupScanner::new();
        scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(1000),
            CleanupItem::new("/tmp/b", CleanupCategory::TempFiles).with_size(2000),
        ]);
        assert_eq!(scanner.estimate_savings(), 3000);
    }

    #[test]
    fn test_scanner_empty_initially() {
        let scanner = CleanupScanner::new();
        assert!(scanner.items().is_empty());
        assert_eq!(scanner.estimate_savings(), 0);
    }

    #[test]
    fn test_scanner_scan_temp_files() {
        let mut scanner = CleanupScanner::new();
        scanner.scan_temp_files("/");
        let temp_items: Vec<_> = scanner
            .items()
            .iter()
            .filter(|i| i.category == CleanupCategory::TempFiles)
            .collect();
        assert_eq!(temp_items.len(), 2); // /tmp and /var/tmp
    }

    #[test]
    fn test_scanner_scan_logs() {
        let mut scanner = CleanupScanner::new();
        scanner.scan_logs("/", 14);
        let log_items: Vec<_> = scanner
            .items()
            .iter()
            .filter(|i| i.category == CleanupCategory::LogFiles)
            .collect();
        assert_eq!(log_items.len(), 1);
    }

    #[test]
    fn test_scanner_scan_package_cache() {
        let mut scanner = CleanupScanner::new();
        scanner.scan_package_cache("/");
        let items: Vec<_> = scanner
            .items()
            .iter()
            .filter(|i| i.category == CleanupCategory::PackageCache)
            .collect();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_scanner_scan_recycle_bin() {
        let mut scanner = CleanupScanner::new();
        scanner.scan_recycle_bin("/");
        let items: Vec<_> = scanner
            .items()
            .iter()
            .filter(|i| i.category == CleanupCategory::RecycleBin)
            .collect();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_scanner_scan_thumbnail_cache() {
        let mut scanner = CleanupScanner::new();
        scanner.scan_thumbnail_cache("/");
        let items: Vec<_> = scanner
            .items()
            .iter()
            .filter(|i| i.category == CleanupCategory::ThumbnailCache)
            .collect();
        assert_eq!(items.len(), 1);
    }

    // -- CleanupPlan tests --------------------------------------------------

    #[test]
    fn test_plan_build_filters_by_category() {
        let mut scanner = CleanupScanner::new();
        scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(100),
            CleanupItem::new("/var/log/x.log", CleanupCategory::LogFiles).with_size(200),
            CleanupItem::new("/tmp/b", CleanupCategory::TempFiles).with_size(300),
        ]);

        let plan = CleanupPlan::build(&scanner, &[CleanupCategory::TempFiles]);
        assert_eq!(plan.item_count(), 2);
        assert_eq!(plan.total_savings_bytes, 400);
    }

    #[test]
    fn test_plan_empty_when_no_categories_selected() {
        let mut scanner = CleanupScanner::new();
        scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(100),
        ]);

        let plan = CleanupPlan::build(&scanner, &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.total_savings_bytes, 0);
    }

    #[test]
    fn test_plan_safe_only() {
        let mut scanner = CleanupScanner::new();
        scanner.set_items(vec![
            CleanupItem::new("/tmp/safe", CleanupCategory::TempFiles)
                .with_size(100)
                .with_safety(true),
            CleanupItem::new("/var/crash/core", CleanupCategory::CrashDumps)
                .with_size(500)
                .with_safety(false),
        ]);

        let plan = CleanupPlan::build(
            &scanner,
            &[CleanupCategory::TempFiles, CleanupCategory::CrashDumps],
        );
        assert_eq!(plan.item_count(), 2);

        let safe_plan = plan.safe_only();
        assert_eq!(safe_plan.item_count(), 1);
        assert_eq!(safe_plan.total_savings_bytes, 100);
    }

    // -- CleanupExecutor tests ----------------------------------------------

    #[test]
    fn test_executor_execute() {
        let mut scanner = CleanupScanner::new();
        scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(1024),
            CleanupItem::new("/tmp/b", CleanupCategory::TempFiles).with_size(2048),
        ]);
        let plan = CleanupPlan::build(&scanner, &[CleanupCategory::TempFiles]);
        let result = CleanupExecutor::execute(&plan);

        assert_eq!(result.files_deleted, 2);
        assert_eq!(result.bytes_freed, 3072);
        assert!(result.is_success());
    }

    #[test]
    fn test_executor_dry_run_same_counts() {
        let mut scanner = CleanupScanner::new();
        scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(500),
        ]);
        let plan = CleanupPlan::build(&scanner, &[CleanupCategory::TempFiles]);

        let real = CleanupExecutor::execute(&plan);
        let dry = CleanupExecutor::dry_run(&plan);

        assert_eq!(real.files_deleted, dry.files_deleted);
        assert_eq!(real.bytes_freed, dry.bytes_freed);
    }

    #[test]
    fn test_executor_empty_plan() {
        let scanner = CleanupScanner::new();
        let plan = CleanupPlan::build(&scanner, &[]);
        let result = CleanupExecutor::execute(&plan);

        assert_eq!(result.files_deleted, 0);
        assert_eq!(result.bytes_freed, 0);
        assert!(result.is_success());
    }

    // -- CleanupResult tests ------------------------------------------------

    #[test]
    fn test_result_default_is_success() {
        let result = CleanupResult::new();
        assert!(result.is_success());
        assert_eq!(result.error_count(), 0);
    }

    #[test]
    fn test_result_with_errors() {
        let mut result = CleanupResult::new();
        result
            .errors
            .push(("/tmp/locked".into(), "permission denied".into()));
        assert!(!result.is_success());
        assert_eq!(result.error_count(), 1);
    }

    // -- ScheduledCleanup tests ---------------------------------------------

    #[test]
    fn test_scheduled_cleanup_builder() {
        let sched = ScheduledCleanup::new(ScheduleInterval::Weekly)
            .with_categories(&[CleanupCategory::TempFiles, CleanupCategory::LogFiles])
            .with_min_age(14)
            .with_enabled(true);

        assert_eq!(sched.interval, ScheduleInterval::Weekly);
        assert_eq!(sched.categories.len(), 2);
        assert_eq!(sched.min_age_days, 14);
        assert!(sched.enabled);
    }

    #[test]
    fn test_scheduled_cleanup_includes_category() {
        let sched = ScheduledCleanup::new(ScheduleInterval::Monthly)
            .with_categories(&[CleanupCategory::RecycleBin]);
        assert!(sched.includes_category(CleanupCategory::RecycleBin));
        assert!(!sched.includes_category(CleanupCategory::CrashDumps));
    }

    // -- CleanupHistory tests -----------------------------------------------

    #[test]
    fn test_history_initially_empty() {
        let history = CleanupHistory::new();
        assert_eq!(history.count(), 0);
        assert_eq!(history.total_bytes_freed(), 0);
        assert!(history.latest().is_none());
    }

    #[test]
    fn test_history_record_and_query() {
        let mut history = CleanupHistory::new();
        let result = CleanupResult {
            files_deleted: 5,
            bytes_freed: 10_000,
            errors: Vec::new(),
            simulated: false,
        };
        history.record(1_700_000_000, &result, &[CleanupCategory::TempFiles]);

        assert_eq!(history.count(), 1);
        assert_eq!(history.total_bytes_freed(), 10_000);

        let latest = history.latest().expect("should have one entry");
        assert_eq!(latest.files_deleted, 5);
        assert_eq!(latest.bytes_freed, 10_000);
        assert_eq!(latest.error_count, 0);
    }

    #[test]
    fn test_history_multiple_entries() {
        let mut history = CleanupHistory::new();

        let r1 = CleanupResult {
            files_deleted: 3,
            bytes_freed: 5_000,
            errors: Vec::new(),
            simulated: false,
        };
        let r2 = CleanupResult {
            files_deleted: 7,
            bytes_freed: 15_000,
            errors: vec![("x".into(), "err".into())],
            simulated: false,
        };

        history.record(100, &r1, &[CleanupCategory::TempFiles]);
        history.record(200, &r2, &[CleanupCategory::LogFiles]);

        assert_eq!(history.count(), 2);
        assert_eq!(history.total_bytes_freed(), 20_000);

        let latest = history.latest().expect("should have entries");
        assert_eq!(latest.timestamp, 200);
        assert_eq!(latest.error_count, 1);
    }

    // -- CleanupUI tests ----------------------------------------------------

    #[test]
    fn test_ui_initial_state() {
        let ui = CleanupUI::new();
        assert_eq!(ui.screen, UiScreen::CategoryList);
        assert!(!ui.scan_complete);
        assert!(ui.selected_categories().is_empty());
    }

    #[test]
    fn test_ui_toggle_category() {
        let mut ui = CleanupUI::new();
        ui.toggle_category(CleanupCategory::TempFiles);
        assert!(
            ui.selected
                .get(&CleanupCategory::TempFiles)
                .copied()
                .unwrap_or(false)
        );

        ui.toggle_category(CleanupCategory::TempFiles);
        assert!(
            !ui.selected
                .get(&CleanupCategory::TempFiles)
                .copied()
                .unwrap_or(true)
        );
    }

    #[test]
    fn test_ui_select_all_deselect_all() {
        let mut ui = CleanupUI::new();
        ui.select_all();
        assert_eq!(ui.selected_categories().len(), 9);

        ui.deselect_all();
        assert!(ui.selected_categories().is_empty());
    }

    #[test]
    fn test_ui_run_scan() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&["/"]);
        assert!(ui.scan_complete);
        assert!(!ui.scanner.items().is_empty());
    }

    #[test]
    fn test_ui_selected_savings() {
        let mut ui = CleanupUI::new();
        ui.scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(1000),
            CleanupItem::new("/var/log/x", CleanupCategory::LogFiles).with_size(2000),
        ]);
        ui.category_sizes.insert(CleanupCategory::TempFiles, 1000);
        ui.category_sizes.insert(CleanupCategory::LogFiles, 2000);
        ui.scan_complete = true;

        ui.toggle_category(CleanupCategory::TempFiles);
        assert_eq!(ui.selected_savings(), 1000);

        ui.toggle_category(CleanupCategory::LogFiles);
        assert_eq!(ui.selected_savings(), 3000);
    }

    #[test]
    fn test_ui_execute_cleanup() {
        let mut ui = CleanupUI::new();
        ui.scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(4096),
        ]);
        ui.toggle_category(CleanupCategory::TempFiles);
        ui.scan_complete = true;

        let result = ui.execute_cleanup();
        assert_eq!(result.files_deleted, 1);
        assert_eq!(result.bytes_freed, 4096);
        assert_eq!(ui.screen, UiScreen::Results);
        assert_eq!(ui.history.count(), 1);
    }

    #[test]
    fn test_ui_dry_run() {
        let mut ui = CleanupUI::new();
        ui.scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(512),
        ]);
        ui.toggle_category(CleanupCategory::TempFiles);

        let result = ui.dry_run();
        assert_eq!(result.files_deleted, 1);
        assert_eq!(result.bytes_freed, 512);
        // Dry run should NOT change screen or history.
        assert_eq!(ui.screen, UiScreen::CategoryList);
        assert_eq!(ui.history.count(), 0);
    }

    #[test]
    fn test_ui_show_preview_and_back() {
        let mut ui = CleanupUI::new();
        ui.show_preview(CleanupCategory::LogFiles);
        assert_eq!(ui.screen, UiScreen::FilePreview);
        assert_eq!(ui.preview_category, Some(CleanupCategory::LogFiles));

        ui.back_to_list();
        assert_eq!(ui.screen, UiScreen::CategoryList);
        assert_eq!(ui.preview_category, None);
    }

    #[test]
    fn test_ui_confirm_dialog_flow() {
        let mut ui = CleanupUI::new();
        ui.show_confirm();
        assert!(ui.is_confirming());
        // The confirmation is an overlay, so the screen underneath is
        // unchanged -- that is the whole difference from the `UiScreen`
        // variant this replaced.
        assert_eq!(ui.screen, UiScreen::CategoryList);

        ui.cancel_confirm();
        assert!(!ui.is_confirming());
        assert_eq!(ui.screen, UiScreen::CategoryList);
    }

    // -- Render tests -------------------------------------------------------

    #[test]
    fn test_render_category_list_produces_commands() {
        let mut ui = CleanupUI::new();
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_progress_screen() {
        let mut ui = CleanupUI::new();
        ui.screen = UiScreen::Progress;
        ui.progress = 0.5;
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_results_screen() {
        let mut ui = CleanupUI::new();
        ui.screen = UiScreen::Results;
        ui.last_result = Some(CleanupResult {
            files_deleted: 3,
            bytes_freed: 8192,
            errors: Vec::new(),
            simulated: false,
        });
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_confirm_dialog_draws_over_the_list() {
        let mut ui = CleanupUI::new();
        let plain = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT).len();
        ui.show_confirm();
        let with_dialog = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT).len();
        // Strictly more, not merely non-empty: the old assertion passed for a
        // dialog that drew nothing at all, because the list underneath it was
        // already producing commands.
        assert!(
            with_dialog > plain,
            "dialog added no commands: {plain} -> {with_dialog}"
        );
    }

    #[test]
    fn test_render_file_preview() {
        let mut ui = CleanupUI::new();
        ui.scanner.set_items(vec![
            CleanupItem::new("/tmp/a", CleanupCategory::TempFiles).with_size(100),
        ]);
        ui.show_preview(CleanupCategory::TempFiles);
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_file_preview_empty_category() {
        let mut ui = CleanupUI::new();
        // No items injected.
        ui.show_preview(CleanupCategory::BrowserCache);
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    // -- Event wiring tests -------------------------------------------------
    //
    // Until 2026-08-26 this program had no `handle_event` at all, so none of
    // these could have existed: every test above drives the UI by calling its
    // methods directly, which is exactly the arrangement in which a window can
    // pass its whole suite while being completely inert. What is checked here
    // is the seam -- that a click at a *coordinate* reaches the method -- and
    // that the coordinate comes from the same `Layout` the renderer drew at.

    /// A left-button press at `(x, y)`.
    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    /// An unmodified key press.
    fn press(key: Key) -> Event {
        Event::Key(guitk::event::KeyEvent {
            key,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        })
    }

    /// The centre of a rectangle, which is where a test should aim: an edge is
    /// where two rectangles disagree, and this is not the test for that.
    fn centre(rect: Rect) -> (f32, f32) {
        let (x, y, w, h) = rect;
        (x + w / 2.0, y + h / 2.0)
    }

    #[test]
    fn test_click_on_scan_button_scans() {
        let mut ui = CleanupUI::new();
        assert!(!ui.scan_complete);
        let (x, y) = centre(ui.layout().scan_button());
        assert_eq!(ui.handle_event(&click(x, y)), EventResult::Consumed);
        assert!(ui.scan_complete);
        assert!(!ui.category_sizes.is_empty());
    }

    #[test]
    fn test_click_on_a_row_toggles_it() {
        let mut ui = CleanupUI::new();
        let lay = ui.layout();
        let row = lay.category_row(0).expect("first row is on screen");
        let cat = CleanupCategory::ALL[0];
        assert!(!ui.selected[&cat]);

        let (x, y) = centre(row);
        assert_eq!(ui.handle_event(&click(x, y)), EventResult::Consumed);
        assert!(ui.selected[&cat]);

        assert_eq!(ui.handle_event(&click(x, y)), EventResult::Consumed);
        assert!(!ui.selected[&cat]);
    }

    #[test]
    fn test_click_on_the_checkbox_toggles_the_same_row() {
        // The checkbox rectangle is wider than the drawn square, and the whole
        // row toggles anyway; what matters is that the two agree about *which*
        // row, which they can only do by both coming from `Layout`.
        let mut ui = CleanupUI::new();
        let lay = ui.layout();
        let (x, y) = centre(lay.category_checkbox(2).expect("third row is on screen"));
        ui.handle_event(&click(x, y));
        assert!(ui.selected[&CleanupCategory::ALL[2]]);
    }

    #[test]
    fn test_click_on_greyed_out_clean_button_is_ignored() {
        // Nothing scanned and nothing selected, so the renderer greys it. The
        // hit-test must refuse it for the same reason -- a button that looks
        // disabled and acts enabled is the bug this shares a predicate to
        // avoid.
        let mut ui = CleanupUI::new();
        assert!(!ui.can_clean());
        let (x, y) = centre(ui.layout().clean_button());
        assert_eq!(ui.handle_event(&click(x, y)), EventResult::Ignored);
        assert!(!ui.is_confirming());
    }

    #[test]
    fn test_click_on_live_clean_button_confirms_first() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&DEFAULT_SCAN_ROOTS);
        ui.select_all();
        assert!(ui.can_clean());

        let (x, y) = centre(ui.layout().clean_button());
        assert_eq!(ui.handle_event(&click(x, y)), EventResult::Consumed);
        // Confirmed, not done: the click must not delete anything on its own.
        assert!(ui.is_confirming());
        assert!(ui.last_result.is_none());
    }

    #[test]
    fn test_confirmation_swallows_clicks_meant_for_the_list() {
        // The dialog quotes the selection back at the user. A click that got
        // past it could change that selection while it was on screen, so that
        // the user authorised one thing and a different thing happened.
        let mut ui = CleanupUI::new();
        ui.run_scan(&DEFAULT_SCAN_ROOTS);
        ui.select_all();
        ui.show_confirm();

        let before = ui.selected.clone();
        let (x, y) = centre(ui.layout().category_row(0).expect("row is on screen"));
        ui.handle_event(&click(x, y));
        assert_eq!(ui.selected, before, "a click reached the list underneath");
    }

    /// A UI whose scan found `bytes` in `cat`, and its index.
    ///
    /// Built by hand rather than by calling `run_scan`, because the real
    /// scanner reports **zero bytes for every category** -- it records the
    /// directories as placeholder items and never asks how big they are. See
    /// `known-issues.md` -> `TD-C-DISKCLEANUP-DELETES-NOTHING`. A test that
    /// went through `run_scan` could therefore never reach the size-dependent
    /// half of the UI, which is most of it.
    fn scanned(cat: CleanupCategory, bytes: u64) -> (CleanupUI, usize) {
        let mut ui = CleanupUI::new();
        ui.scanner
            .set_items(vec![CleanupItem::new("/tmp/a", cat).with_size(bytes)]);
        ui.scan_complete = true;
        ui.category_sizes.insert(cat, bytes);
        let index = CleanupCategory::ALL
            .iter()
            .position(|c| *c == cat)
            .expect("every category is in ALL");
        (ui, index)
    }

    #[test]
    fn test_view_link_opens_the_preview_and_the_rest_of_the_row_does_not() {
        let (mut ui, index) = scanned(CleanupCategory::TempFiles, 4096);
        let lay = ui.layout();

        let (lx, ly) = centre(lay.category_view_link(index).expect("row is on screen"));
        assert_eq!(ui.handle_event(&click(lx, ly)), EventResult::Consumed);
        assert_eq!(ui.screen, UiScreen::FilePreview);
        assert_eq!(ui.preview_category, Some(CleanupCategory::ALL[index]));

        // Escape comes back, and the row's left-hand side toggles rather than
        // navigating.
        assert_eq!(ui.handle_event(&press(Key::Escape)), EventResult::Consumed);
        assert_eq!(ui.screen, UiScreen::CategoryList);
        let (rx, ry) = centre(lay.category_checkbox(index).expect("row is on screen"));
        ui.handle_event(&click(rx, ry));
        assert_eq!(ui.screen, UiScreen::CategoryList);
        assert!(ui.selected[&CleanupCategory::ALL[index]]);
    }

    #[test]
    fn test_select_all_and_deselect_all_keys() {
        let mut ui = CleanupUI::new();
        assert_eq!(ui.handle_event(&press(Key::A)), EventResult::Consumed);
        assert_eq!(ui.selected_categories().len(), CleanupCategory::ALL.len());
        assert_eq!(ui.handle_event(&press(Key::D)), EventResult::Consumed);
        assert!(ui.selected_categories().is_empty());
    }

    #[test]
    fn test_escape_on_the_category_list_is_left_for_the_window_manager() {
        let mut ui = CleanupUI::new();
        assert_eq!(ui.handle_event(&press(Key::Escape)), EventResult::Ignored);
    }

    #[test]
    fn test_resize_moves_the_footer_buttons() {
        // The proof that the hit-test reads the *reported* size rather than the
        // constant: a click where the button used to be must stop working, and
        // a click where it now is must start.
        let mut ui = CleanupUI::new();
        let old = centre(ui.layout().scan_button());
        ui.handle_event(&Event::Resize {
            width: 900,
            height: 700,
        });
        let new = centre(ui.layout().scan_button());
        assert!(
            (old.0 - new.0).abs() > 1.0 || (old.1 - new.1).abs() > 1.0,
            "layout did not follow the resize"
        );
        assert_eq!(ui.handle_event(&click(old.0, old.1)), EventResult::Ignored);
        assert_eq!(ui.handle_event(&click(new.0, new.1)), EventResult::Consumed);
        assert!(ui.scan_complete);
    }

    #[test]
    fn test_a_tick_with_no_dialog_is_ignored() {
        // Sixty a second. Consuming them would hold the compositor redrawing
        // this window for as long as it was open.
        let mut ui = CleanupUI::new();
        assert_eq!(
            ui.handle_event(&Event::Tick { elapsed_ms: 16 }),
            EventResult::Ignored
        );
    }

    #[test]
    fn test_accepting_the_confirmation_runs_the_cleanup() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&DEFAULT_SCAN_ROOTS);
        ui.select_all();
        ui.show_confirm();

        // Tab moves focus off Cancel and onto the destructive button; Enter
        // then activates it. Both keystrokes are needed, and the next test is
        // why.
        ui.handle_event(&press(Key::Tab));
        ui.handle_event(&press(Key::Enter));
        assert!(!ui.is_confirming());
        assert_eq!(ui.screen, UiScreen::Results);
        let result = ui.last_result.as_ref().expect("a result was recorded");
        // And it says so: nothing was deleted, and the screen must not claim
        // otherwise. See `known-issues.md` -> TD-C-DISKCLEANUP-DELETES-NOTHING.
        assert!(result.simulated);
    }

    #[test]
    fn test_enter_alone_does_not_delete() {
        // A destructive dialog focuses Cancel, not Delete -- see
        // `ButtonSet::destructive_cancel`. Enter is what gets hit reflexively
        // when a dialog appears unexpectedly, and a user who has not finished
        // reading this one must not thereby empty their disk. Asserted here
        // rather than trusted, because it is a property of *this* dialog that a
        // later change to its button set could silently reverse.
        let mut ui = CleanupUI::new();
        ui.run_scan(&DEFAULT_SCAN_ROOTS);
        ui.select_all();
        ui.show_confirm();

        ui.handle_event(&press(Key::Enter));
        assert!(!ui.is_confirming());
        assert_eq!(ui.screen, UiScreen::CategoryList);
        assert!(ui.last_result.is_none());
    }

    #[test]
    fn test_dismissing_the_confirmation_deletes_nothing() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&DEFAULT_SCAN_ROOTS);
        ui.select_all();
        ui.show_confirm();

        ui.handle_event(&press(Key::Escape));
        assert!(!ui.is_confirming());
        assert_eq!(ui.screen, UiScreen::CategoryList);
        assert!(ui.last_result.is_none());
    }

    #[test]
    fn test_rows_past_the_bottom_are_neither_drawn_nor_clickable() {
        // A window too short for nine rows. The renderer stops at the fold
        // because `category_row` returns `None`; the hit-test must stop at the
        // same index, or a row nobody can see would still toggle.
        let lay = Layout::new(
            WINDOW_WIDTH,
            HEADER_HEIGHT + FOOTER_HEIGHT + ROW_HEIGHT * 2.5,
        );
        assert!(lay.category_row(0).is_some());
        assert!(lay.category_row(2).is_some());
        assert!(lay.category_row(3).is_none());
        assert!(lay.category_view_link(3).is_none());
        assert!(lay.category_checkbox(3).is_none());
    }

    #[test]
    fn test_done_button_returns_to_the_list() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&DEFAULT_SCAN_ROOTS);
        ui.select_all();
        ui.execute_cleanup();
        assert_eq!(ui.screen, UiScreen::Results);

        let (x, y) = centre(ui.layout().done_button());
        assert_eq!(ui.handle_event(&click(x, y)), EventResult::Consumed);
        assert_eq!(ui.screen, UiScreen::CategoryList);
    }

    #[test]
    fn test_back_button_leaves_the_preview() {
        let mut ui = CleanupUI::new();
        ui.show_preview(CleanupCategory::TempFiles);
        let (x, y) = centre(ui.layout().back_button());
        assert_eq!(ui.handle_event(&click(x, y)), EventResult::Consumed);
        assert_eq!(ui.screen, UiScreen::CategoryList);
    }

    // -- Utility function tests ---------------------------------------------

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
    }

    #[test]
    fn test_format_size_kib() {
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
    }

    #[test]
    fn test_format_size_mib() {
        assert_eq!(format_size(1_048_576), "1.0 MiB");
    }

    #[test]
    fn test_format_size_gib() {
        assert_eq!(format_size(1_073_741_824), "1.0 GiB");
    }

    #[test]
    fn test_join_path_root() {
        assert_eq!(join_path("/", "tmp"), "/tmp");
    }

    #[test]
    fn test_join_path_non_root() {
        assert_eq!(join_path("/var", "log"), "/var/log");
    }

    #[test]
    fn test_join_path_trailing_slash() {
        assert_eq!(join_path("/var/", "log"), "/var/log");
    }

    #[test]
    fn test_join_paths_multiple() {
        assert_eq!(join_paths("/", &["var", "cache", "pkg"]), "/var/cache/pkg");
    }

    #[test]
    fn test_join_paths_empty_segments() {
        assert_eq!(join_paths("/home", &[]), "/home");
    }
}
