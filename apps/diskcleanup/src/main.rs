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
//! by the guitk `RenderTree` / `RenderCommand` primitives.

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::BTreeMap;

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
        let items: Vec<CleanupItem> = self
            .items
            .iter()
            .filter(|i| i.is_safe)
            .cloned()
            .collect();
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
}

impl CleanupResult {
    pub fn new() -> Self {
        Self {
            files_deleted: 0,
            bytes_freed: 0,
            errors: Vec::new(),
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
    /// Execute the plan, deleting files.  Returns a result summary.
    ///
    /// In a real deployment this would call VFS `unlink` / `rmdir` for each
    /// item.  The current implementation simulates successful deletion.
    pub fn execute(plan: &CleanupPlan) -> CleanupResult {
        let mut result = CleanupResult::new();
        for item in &plan.items {
            // Simulate deletion -- a real implementation would call
            // `std::fs::remove_file` or the VFS equivalent here.
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
    /// Confirmation dialog before executing cleanup.
    ConfirmDialog,
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
        }
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
            *self.category_sizes.entry(item.category).or_insert(0) +=
                item.estimated_size_bytes;
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

    /// Show the confirmation dialog.
    pub fn show_confirm(&mut self) {
        self.screen = UiScreen::ConfirmDialog;
    }

    /// Cancel the confirmation dialog and go back.
    pub fn cancel_confirm(&mut self) {
        self.screen = UiScreen::CategoryList;
    }

    /// Total estimated savings from selected categories.
    pub fn selected_savings(&self) -> u64 {
        self.selected_categories()
            .iter()
            .filter_map(|cat| self.category_sizes.get(cat))
            .sum()
    }

    // -- rendering ----------------------------------------------------------

    /// Render the entire UI to a list of `RenderCommand`s.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Window background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        match self.screen {
            UiScreen::CategoryList => self.render_category_list(&mut cmds, width, height),
            UiScreen::FilePreview => self.render_file_preview(&mut cmds, width, height),
            UiScreen::Progress => self.render_progress(&mut cmds, width, height),
            UiScreen::Results => self.render_results(&mut cmds, width, height),
            UiScreen::ConfirmDialog => {
                // Render the list underneath, then the dialog overlay.
                self.render_category_list(&mut cmds, width, height);
                self.render_confirm_dialog(&mut cmds, width, height);
            }
        }

        cmds
    }

    // -- render sub-screens -------------------------------------------------

    fn render_category_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        // Header.
        self.render_header(cmds, width, "Disk Cleanup");

        // Category rows.
        let content_top = HEADER_HEIGHT;
        let content_height = height - HEADER_HEIGHT - FOOTER_HEIGHT;

        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_top,
            width,
            height: content_height,
        });

        for (i, cat) in CleanupCategory::ALL.iter().enumerate() {
            let y = content_top + (i as f32) * ROW_HEIGHT;
            if y > content_top + content_height {
                break;
            }
            let checked = self.selected.get(cat).copied().unwrap_or(false);
            let size = self.category_sizes.get(cat).copied().unwrap_or(0);
            self.render_category_row(cmds, y, width, *cat, checked, size);
        }

        cmds.push(RenderCommand::PopClip);

        // Footer with scan/clean buttons.
        self.render_footer(cmds, width, height);
    }

    fn render_category_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        y: f32,
        width: f32,
        cat: CleanupCategory,
        checked: bool,
        size_bytes: u64,
    ) {
        // Alternating row background.
        let cat_index = CleanupCategory::ALL
            .iter()
            .position(|c| *c == cat)
            .unwrap_or(0);
        if cat_index % 2 == 0 {
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y,
                width,
                height: ROW_HEIGHT,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        let cx = PADDING;
        let cy = y + (ROW_HEIGHT - CHECKBOX_SIZE) / 2.0;

        // Checkbox outline.
        cmds.push(RenderCommand::StrokeRect {
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
            cmds.push(RenderCommand::FillRect {
                x: cx + 3.0,
                y: cy + 3.0,
                width: CHECKBOX_SIZE - 6.0,
                height: CHECKBOX_SIZE - 6.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Category name.
        cmds.push(RenderCommand::Text {
            x: cx + CHECKBOX_SIZE + 10.0,
            y: y + 6.0,
            text: cat.display_name().to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });

        // Description (smaller, dimmer).
        cmds.push(RenderCommand::Text {
            x: cx + CHECKBOX_SIZE + 10.0,
            y: y + 20.0,
            text: cat.description().to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });

        // Size estimate (right-aligned).
        if self.scan_complete {
            let size_text = format_size(size_bytes);
            cmds.push(RenderCommand::Text {
                x: width - 120.0,
                y: y + 10.0,
                text: size_text,
                color: if size_bytes > 0 { COLOR_YELLOW } else { COLOR_SUBTEXT },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(110.0),
            });
        }

        // "View" link (far right).
        if self.scan_complete && size_bytes > 0 {
            cmds.push(RenderCommand::Text {
                x: width - PADDING - 30.0,
                y: y + 10.0,
                text: String::from("View"),
                color: COLOR_BLUE,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_header(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        title: &str,
    ) {
        cmds.push(RenderCommand::FillRect {
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

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: (HEADER_HEIGHT - FONT_SIZE_HEADING) / 2.0,
            text: title.to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
        });
    }

    fn render_footer(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        let y = height - FOOTER_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: FOOTER_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        });

        // Total savings label (left side).
        if self.scan_complete {
            let savings = self.selected_savings();
            let label = format!("Selected: {}", format_size(savings));
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: y + (FOOTER_HEIGHT - FONT_SIZE) / 2.0,
                text: label,
                color: COLOR_GREEN,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.4),
            });
        }

        // Buttons (right side).
        let btn_y = y + (FOOTER_HEIGHT - BUTTON_HEIGHT) / 2.0;
        let clean_x = width - PADDING - BUTTON_WIDTH;
        let scan_x = clean_x - PADDING - BUTTON_WIDTH;

        // Scan button.
        self.render_button(cmds, scan_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Scan", COLOR_BLUE);

        // Clean up button.
        let clean_enabled = self.scan_complete && !self.selected_categories().is_empty();
        let clean_color = if clean_enabled { COLOR_GREEN } else { COLOR_SURFACE1 };
        self.render_button(cmds, clean_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT, "Clean Up", clean_color);
    }

    // 8 args mirror the (cmds, x, y, w, h, label, bg) button signature used
    // elsewhere in the app's render layer; struct-bundling would only shift
    // the verbosity to the call site.
    #[allow(clippy::too_many_arguments)]
    fn render_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        bg: Color,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Center text horizontally (approximate).
        let text_width = label.len() as f32 * FONT_SIZE * 0.6;
        let text_x = x + (w - text_width) / 2.0;
        let text_y = y + (h - FONT_SIZE) / 2.0;

        cmds.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: label.to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w),
        });
    }

    fn render_file_preview(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        let cat = match self.preview_category {
            Some(c) => c,
            None => {
                // Should not happen, but degrade gracefully.
                self.render_category_list(cmds, width, height);
                return;
            }
        };

        let title = format!("Files: {}", cat.display_name());
        self.render_header(cmds, width, &title);

        let content_top = HEADER_HEIGHT + PADDING;
        let items: Vec<&CleanupItem> = self
            .scanner
            .items()
            .iter()
            .filter(|i| i.category == cat)
            .collect();

        if items.is_empty() {
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: content_top,
                text: String::from("No items found for this category."),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
            });
        } else {
            for (i, item) in items.iter().enumerate() {
                let y = content_top + (i as f32) * ROW_HEIGHT;
                if y > height - FOOTER_HEIGHT {
                    break;
                }

                // Path.
                cmds.push(RenderCommand::Text {
                    x: PADDING,
                    y,
                    text: item.path.clone(),
                    color: COLOR_TEXT,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width * 0.6),
                });

                // Size.
                cmds.push(RenderCommand::Text {
                    x: width - 120.0,
                    y,
                    text: format_size(item.estimated_size_bytes),
                    color: COLOR_YELLOW,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                // Safety indicator.
                let safety_text = if item.is_safe { "Safe" } else { "Caution" };
                let safety_color = if item.is_safe { COLOR_GREEN } else { COLOR_RED };
                cmds.push(RenderCommand::Text {
                    x: width - PADDING - 50.0,
                    y,
                    text: safety_text.to_string(),
                    color: safety_color,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        // Back button at bottom.
        let btn_y = height - FOOTER_HEIGHT + (FOOTER_HEIGHT - BUTTON_HEIGHT) / 2.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: height - FOOTER_HEIGHT,
            width,
            height: FOOTER_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        });
        self.render_button(
            cmds,
            PADDING,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Back",
            COLOR_BLUE,
        );
    }

    fn render_progress(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        self.render_header(cmds, width, "Cleaning Up...");

        let center_y = height / 2.0 - 30.0;

        // Progress label.
        let pct = (self.progress * 100.0).min(100.0);
        let label = format!("{pct:.0}% complete");
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: center_y,
            text: label,
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
        });

        // Progress bar track.
        let bar_y = center_y + 24.0;
        let bar_width = width - PADDING * 2.0;
        cmds.push(RenderCommand::FillRect {
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
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: bar_y,
                width: fill_width,
                height: PROGRESS_HEIGHT,
                color: COLOR_GREEN,
                corner_radii: CornerRadii::all(PROGRESS_HEIGHT / 2.0),
            });
        }
    }

    fn render_results(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        self.render_header(cmds, width, "Cleanup Complete");

        let result = match &self.last_result {
            Some(r) => r,
            None => return,
        };

        let mut y = HEADER_HEIGHT + PADDING * 2.0;

        // Files deleted.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: format!("Files deleted: {}", result.files_deleted),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
        });
        y += 24.0;

        // Space freed.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: format!("Space freed: {}", format_size(result.bytes_freed)),
            color: COLOR_GREEN,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
        });
        y += 24.0;

        // Errors (if any).
        if !result.errors.is_empty() {
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y,
                text: format!("Errors: {}", result.error_count()),
                color: COLOR_RED,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
            });
            y += 20.0;

            for (path, msg) in &result.errors {
                cmds.push(RenderCommand::Text {
                    x: PADDING * 2.0,
                    y,
                    text: format!("{path}: {msg}"),
                    color: COLOR_RED,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - PADDING * 3.0),
                });
                y += 18.0;
            }
        }

        // History summary.
        y += 16.0;
        let total_freed = self.history.total_bytes_freed();
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: format!(
                "Total freed across {} cleanups: {}",
                self.history.count(),
                format_size(total_freed)
            ),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
        });

        // Done button.
        let btn_y = height - FOOTER_HEIGHT + (FOOTER_HEIGHT - BUTTON_HEIGHT) / 2.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: height - FOOTER_HEIGHT,
            width,
            height: FOOTER_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        });
        self.render_button(
            cmds,
            width - PADDING - BUTTON_WIDTH,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Done",
            COLOR_BLUE,
        );
    }

    fn render_confirm_dialog(
        &self,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        // Dialog box.
        let dialog_w: f32 = 360.0;
        let dialog_h: f32 = 180.0;
        let dx = (width - dialog_w) / 2.0;
        let dy = (height - dialog_h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: String::from("Confirm Cleanup"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - PADDING * 2.0),
        });

        // Summary.
        let cats = self.selected_categories();
        let savings = self.selected_savings();
        let summary = format!(
            "Delete files from {} categories?\nEstimated space freed: {}",
            cats.len(),
            format_size(savings)
        );
        cmds.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 48.0,
            text: summary,
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - PADDING * 2.0),
        });

        // Buttons.
        let btn_y = dy + dialog_h - BUTTON_HEIGHT - PADDING;
        let cancel_x = dx + dialog_w - PADDING - BUTTON_WIDTH;
        let confirm_x = cancel_x - PADDING - BUTTON_WIDTH;

        self.render_button(
            cmds,
            confirm_x,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Delete",
            COLOR_RED,
        );
        self.render_button(
            cmds,
            cancel_x,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Cancel",
            COLOR_SURFACE1,
        );
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
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
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
        };
        let r2 = CleanupResult {
            files_deleted: 7,
            bytes_freed: 15_000,
            errors: vec![("x".into(), "err".into())],
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
        assert!(ui.selected.get(&CleanupCategory::TempFiles).copied().unwrap_or(false));

        ui.toggle_category(CleanupCategory::TempFiles);
        assert!(!ui.selected.get(&CleanupCategory::TempFiles).copied().unwrap_or(true));
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
        assert_eq!(ui.screen, UiScreen::ConfirmDialog);

        ui.cancel_confirm();
        assert_eq!(ui.screen, UiScreen::CategoryList);
    }

    // -- Render tests -------------------------------------------------------

    #[test]
    fn test_render_category_list_produces_commands() {
        let ui = CleanupUI::new();
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
        });
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_confirm_dialog() {
        let mut ui = CleanupUI::new();
        ui.screen = UiScreen::ConfirmDialog;
        let cmds = ui.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!cmds.is_empty());
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
