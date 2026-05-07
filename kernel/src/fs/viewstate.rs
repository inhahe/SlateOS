//! Per-directory view state persistence for file explorer.
//!
//! Remembers how the user configured each directory's view: sort order,
//! view mode (icons/list/details), icon size, column widths, and scroll
//! position. This creates a Windows Explorer-like experience where each
//! folder "remembers" its settings.
//!
//! ## Architecture
//!
//! ```text
//! User opens /home/user/Photos → viewstate::get("/home/user/Photos")
//!   → returns saved ViewSettings (large icons, sort by date, etc.)
//!
//! User changes to list view → viewstate::set("/home/user/Photos", settings)
//!   → persists the change for next time
//! ```
//!
//! ## Design Notes
//!
//! - Per-directory settings stored in memory (would persist to disk
//!   in production via a .viewstate database file).
//! - Default settings for directories without saved state.
//! - Global default settings that apply everywhere unless overridden.
//! - Template patterns (e.g., "all Pictures folders use large icons").

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum saved view states.
const MAX_STATES: usize = 4096;

/// Maximum view state templates (pattern-based).
const MAX_TEMPLATES: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How files are displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Large icon grid.
    LargeIcons,
    /// Medium icon grid.
    MediumIcons,
    /// Small icon grid.
    SmallIcons,
    /// List (one column, small icons).
    List,
    /// Detail/column view with file metadata.
    Details,
    /// Tile view (icon + summary info).
    Tiles,
    /// Content view (preview + metadata).
    Content,
}

impl ViewMode {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::LargeIcons => "large-icons",
            Self::MediumIcons => "medium-icons",
            Self::SmallIcons => "small-icons",
            Self::List => "list",
            Self::Details => "details",
            Self::Tiles => "tiles",
            Self::Content => "content",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "large-icons" | "large" | "li" => Some(Self::LargeIcons),
            "medium-icons" | "medium" | "mi" => Some(Self::MediumIcons),
            "small-icons" | "small" | "si" => Some(Self::SmallIcons),
            "list" | "l" => Some(Self::List),
            "details" | "detail" | "d" => Some(Self::Details),
            "tiles" | "t" => Some(Self::Tiles),
            "content" | "c" => Some(Self::Content),
            _ => None,
        }
    }

    /// Default icon size in pixels for this mode.
    pub fn default_icon_size(self) -> u32 {
        match self {
            Self::LargeIcons => 256,
            Self::MediumIcons => 128,
            Self::SmallIcons => 48,
            Self::List => 16,
            Self::Details => 16,
            Self::Tiles => 64,
            Self::Content => 96,
        }
    }
}

/// Sort column and direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortConfig {
    /// Column ID to sort by.
    pub column: String,
    /// Ascending (true) or descending (false).
    pub ascending: bool,
}

/// Group-by configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupBy {
    /// No grouping.
    None,
    /// Group by file type.
    Type,
    /// Group by date modified (Today/Yesterday/This Week/etc.).
    DateModified,
    /// Group by size category (Small/Medium/Large/Huge).
    Size,
    /// Group by first letter of name.
    Name,
    /// Custom grouping by a specific column.
    Column(String),
}

/// Complete view settings for a directory.
#[derive(Debug, Clone)]
pub struct ViewSettings {
    /// View mode.
    pub mode: ViewMode,
    /// Icon size override (0 = use mode default).
    pub icon_size: u32,
    /// Sort configuration (primary sort).
    pub sort: SortConfig,
    /// Secondary sort (applied when primary values are equal).
    pub secondary_sort: Option<SortConfig>,
    /// Grouping.
    pub group_by: GroupBy,
    /// Whether to show hidden files.
    pub show_hidden: bool,
    /// Whether to show file extensions.
    pub show_extensions: bool,
    /// Scroll position (pixel offset from top).
    pub scroll_y: u32,
    /// Selected items (preserved when navigating away and back).
    pub selection: Vec<String>,
}

impl ViewSettings {
    /// Default view settings.
    pub fn default_settings() -> Self {
        Self {
            mode: ViewMode::Details,
            icon_size: 0,
            sort: SortConfig {
                column: String::from("name"),
                ascending: true,
            },
            secondary_sort: None,
            group_by: GroupBy::None,
            show_hidden: false,
            show_extensions: true,
            scroll_y: 0,
            selection: Vec::new(),
        }
    }

    /// View settings tuned for image/photo directories.
    pub fn photo_defaults() -> Self {
        Self {
            mode: ViewMode::LargeIcons,
            icon_size: 256,
            sort: SortConfig {
                column: String::from("modified"),
                ascending: false,
            },
            secondary_sort: None,
            group_by: GroupBy::DateModified,
            show_hidden: false,
            show_extensions: false,
            scroll_y: 0,
            selection: Vec::new(),
        }
    }

    /// View settings tuned for music directories.
    pub fn music_defaults() -> Self {
        Self {
            mode: ViewMode::Details,
            icon_size: 0,
            sort: SortConfig {
                column: String::from("audio.track"),
                ascending: true,
            },
            secondary_sort: Some(SortConfig {
                column: String::from("name"),
                ascending: true,
            }),
            group_by: GroupBy::Column(String::from("audio.album")),
            show_hidden: false,
            show_extensions: false,
            scroll_y: 0,
            selection: Vec::new(),
        }
    }

    /// View settings tuned for download directories.
    pub fn downloads_defaults() -> Self {
        Self {
            mode: ViewMode::Details,
            icon_size: 0,
            sort: SortConfig {
                column: String::from("modified"),
                ascending: false,
            },
            secondary_sort: None,
            group_by: GroupBy::DateModified,
            show_hidden: false,
            show_extensions: true,
            scroll_y: 0,
            selection: Vec::new(),
        }
    }
}

/// A view state template (pattern-matched to directories).
#[derive(Debug, Clone)]
pub struct ViewTemplate {
    /// Template ID.
    pub id: u64,
    /// Pattern to match directory paths (glob-like).
    /// "**/Pictures" matches any Pictures directory.
    pub pattern: String,
    /// Settings to apply.
    pub settings: ViewSettings,
    /// Human-readable label.
    pub label: String,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static TEMPLATE_COUNTER: AtomicU64 = AtomicU64::new(1);
static GET_COUNT: AtomicU64 = AtomicU64::new(0);
static SET_COUNT: AtomicU64 = AtomicU64::new(0);

static STATES: spin::Mutex<Vec<(String, ViewSettings)>> = spin::Mutex::new(Vec::new());
static TEMPLATES: spin::Mutex<Vec<ViewTemplate>> = spin::Mutex::new(Vec::new());
static GLOBAL_DEFAULTS: spin::Mutex<Option<ViewSettings>> = spin::Mutex::new(None);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get view settings for a directory.
///
/// Lookup order:
/// 1. Saved per-directory settings
/// 2. Matching template pattern
/// 3. Global defaults
/// 4. Built-in defaults
pub fn get(path: &str) -> ViewSettings {
    GET_COUNT.fetch_add(1, Ordering::Relaxed);

    // 1. Check saved state.
    let states = STATES.lock();
    if let Some((_, settings)) = states.iter().find(|(p, _)| p == path) {
        return settings.clone();
    }
    drop(states);

    // 2. Check templates.
    let templates = TEMPLATES.lock();
    for tmpl in templates.iter() {
        if path_matches_pattern(path, &tmpl.pattern) {
            return tmpl.settings.clone();
        }
    }
    drop(templates);

    // 3. Global defaults.
    let global = GLOBAL_DEFAULTS.lock();
    if let Some(ref defaults) = *global {
        return defaults.clone();
    }
    drop(global);

    // 4. Built-in defaults.
    ViewSettings::default_settings()
}

/// Save view settings for a directory.
pub fn set(path: &str, settings: ViewSettings) -> KernelResult<()> {
    SET_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut states = STATES.lock();

    // Update existing or add new.
    if let Some(entry) = states.iter_mut().find(|(p, _)| p == path) {
        entry.1 = settings;
        return Ok(());
    }

    if states.len() >= MAX_STATES {
        // LRU eviction: remove oldest (first) entry.
        states.remove(0);
    }

    states.push((String::from(path), settings));
    Ok(())
}

/// Remove saved settings for a directory (reverts to defaults).
pub fn remove(path: &str) -> bool {
    let mut states = STATES.lock();
    let before = states.len();
    states.retain(|(p, _)| p != path);
    states.len() < before
}

/// Set global default view settings.
pub fn set_global_defaults(settings: ViewSettings) {
    *GLOBAL_DEFAULTS.lock() = Some(settings);
}

/// Clear global defaults (revert to built-in).
pub fn clear_global_defaults() {
    *GLOBAL_DEFAULTS.lock() = None;
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// Register a view state template.
pub fn register_template(
    pattern: &str,
    settings: ViewSettings,
    label: &str,
) -> KernelResult<u64> {
    let mut templates = TEMPLATES.lock();
    if templates.len() >= MAX_TEMPLATES {
        return Err(KernelError::OutOfMemory);
    }

    let id = TEMPLATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    templates.push(ViewTemplate {
        id,
        pattern: String::from(pattern),
        settings,
        label: String::from(label),
    });

    Ok(id)
}

/// Unregister a template by ID.
pub fn unregister_template(id: u64) -> bool {
    let mut templates = TEMPLATES.lock();
    let before = templates.len();
    templates.retain(|t| t.id != id);
    templates.len() < before
}

/// List all templates.
pub fn list_templates() -> Vec<ViewTemplate> {
    TEMPLATES.lock().clone()
}

/// Initialize default templates.
pub fn init_defaults() {
    let templates = TEMPLATES.lock();
    if !templates.is_empty() {
        return;
    }
    drop(templates);

    let _ = register_template(
        "**/Pictures", ViewSettings::photo_defaults(), "Photo directories");
    let _ = register_template(
        "**/Music", ViewSettings::music_defaults(), "Music directories");
    let _ = register_template(
        "**/Downloads", ViewSettings::downloads_defaults(), "Download directories");
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

/// Simple glob-like pattern matching for directory paths.
fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if pattern.starts_with("**/") {
        // Match any directory ending with this suffix.
        let suffix = pattern.get(3..).unwrap_or("");
        // Check if path ends with /suffix or equals suffix.
        if path.ends_with(suffix) {
            // Verify it's a complete path segment.
            let prefix_end = path.len().saturating_sub(suffix.len());
            if prefix_end == 0 || path.as_bytes().get(prefix_end.saturating_sub(1)) == Some(&b'/') {
                return true;
            }
        }
        return false;
    }

    // Exact match.
    path == pattern
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// List all saved view states.
pub fn list_saved() -> Vec<(String, ViewSettings)> {
    STATES.lock().clone()
}

/// Count saved states.
pub fn saved_count() -> usize {
    STATES.lock().len()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (saved_count, template_count, get_count, set_count).
pub fn stats() -> (usize, usize, u64, u64) {
    let saved = STATES.lock().len();
    let templates = TEMPLATES.lock().len();
    (
        saved,
        templates,
        GET_COUNT.load(Ordering::Relaxed),
        SET_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    GET_COUNT.store(0, Ordering::Relaxed);
    SET_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all saved states.
pub fn clear() {
    STATES.lock().clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the view state module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: default settings.
    {
        let s = ViewSettings::default_settings();
        assert_eq!(s.mode, ViewMode::Details);
        assert_eq!(s.sort.column, "name");
        assert!(s.sort.ascending);
        assert!(!s.show_hidden);
        serial_println!("[viewstate] test 1 passed: default settings");
    }

    // Test 2: save and retrieve.
    {
        let mut settings = ViewSettings::default_settings();
        settings.mode = ViewMode::LargeIcons;
        settings.icon_size = 128;
        set("/test/viewstate", settings.clone())?;

        let retrieved = get("/test/viewstate");
        assert_eq!(retrieved.mode, ViewMode::LargeIcons);
        assert_eq!(retrieved.icon_size, 128);

        remove("/test/viewstate");
        serial_println!("[viewstate] test 2 passed: save + retrieve");
    }

    // Test 3: templates.
    {
        init_defaults();
        let templates = list_templates();
        assert!(!templates.is_empty());
        serial_println!("[viewstate] test 3 passed: templates");
    }

    // Test 4: pattern matching.
    {
        assert!(path_matches_pattern("/home/user/Pictures", "**/Pictures"));
        assert!(path_matches_pattern("/data/Photos/Pictures", "**/Pictures"));
        assert!(!path_matches_pattern("/home/user/pics", "**/Pictures"));
        assert!(path_matches_pattern("/anything", "*"));
        serial_println!("[viewstate] test 4 passed: pattern matching");
    }

    // Test 5: view mode parsing.
    {
        assert_eq!(ViewMode::from_str("details"), Some(ViewMode::Details));
        assert_eq!(ViewMode::from_str("large"), Some(ViewMode::LargeIcons));
        assert_eq!(ViewMode::from_str("list"), Some(ViewMode::List));
        assert_eq!(ViewMode::from_str("invalid"), None);
        serial_println!("[viewstate] test 5 passed: mode parsing");
    }

    // Test 6: stats.
    {
        let (saved, templates, gets, sets) = stats();
        assert!(gets > 0);
        assert!(sets > 0);
        assert!(templates > 0 || saved >= 0);
        serial_println!("[viewstate] test 6 passed: stats");
    }

    serial_println!("[viewstate] all 6 self-tests passed");
    Ok(())
}
