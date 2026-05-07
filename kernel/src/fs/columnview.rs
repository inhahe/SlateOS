//! Detail column view configuration for file explorer.
//!
//! Implements the design spec requirement (lines 904-924) for file
//! explorer detail view columns:
//!
//! > "Use the subset of possible columns which is the union of the
//! > columns relevant to each file in the given folder."
//!
//! This module provides:
//! - Per-file-type column definitions (which columns are relevant)
//! - Dynamic column set computation for a directory
//! - User column preferences (show/hide, reorder, width)
//! - Application column registration (apps add columns for their types)
//!
//! ## Architecture
//!
//! ```text
//! File Explorer opens directory
//!   → columnview::compute_columns(dir)
//!   → scans files with fs::mime to identify types
//!   → unions per-type default columns
//!   → applies user preferences (show/hide/reorder)
//!   → returns ordered ColumnDef list for rendering
//! ```
//!
//! ## Design Spec Example (line 904)
//!
//! "If the folder has 10 mp3's and one jpg, the columns displayed will
//! be size, date, length, bitrate, title, author, width, and height."
//!
//! Common columns (size, date) always appear. Type-specific columns
//! (length, bitrate for audio; width, height for images) appear only
//! when files of that type are present.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum columns in a view.
const MAX_COLUMNS: usize = 64;

/// Maximum registered column definitions.
const MAX_COLUMN_DEFS: usize = 512;

/// Maximum user column preferences.
const MAX_USER_PREFS: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Column data type (for alignment and formatting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    /// Text (left-aligned).
    Text,
    /// Integer number (right-aligned).
    Integer,
    /// File size with units (right-aligned).
    Size,
    /// Date/time (right-aligned).
    DateTime,
    /// Duration (e.g., "3:45" for audio length).
    Duration,
    /// Boolean (checkbox-style).
    Boolean,
    /// Image dimensions ("1920×1080").
    Dimensions,
}

impl ColumnType {
    /// Default alignment for this type.
    pub fn alignment(self) -> Alignment {
        match self {
            Self::Text => Alignment::Left,
            Self::Integer | Self::Size | Self::DateTime | Self::Duration => Alignment::Right,
            Self::Boolean => Alignment::Center,
            Self::Dimensions => Alignment::Right,
        }
    }
}

/// Column text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// A column definition (metadata about what a column shows).
#[derive(Debug, Clone)]
pub struct ColumnDef {
    /// Unique column identifier (e.g., "name", "size", "audio.bitrate").
    pub id: String,
    /// Display header text.
    pub header: String,
    /// Column data type.
    pub col_type: ColumnType,
    /// Default width in pixels (0 = auto).
    pub default_width: u32,
    /// Minimum width in pixels.
    pub min_width: u32,
    /// Whether this column is sortable.
    pub sortable: bool,
    /// MIME types this column applies to (empty = universal).
    pub applies_to: Vec<String>,
    /// Source application that registered this column.
    pub source: String,
    /// Display priority (lower = earlier in default order).
    pub priority: u32,
    /// Whether this is a built-in system column.
    pub system: bool,
    /// The fileinfo field name this maps to (for data extraction).
    pub field_name: String,
}

/// User preference for a specific column in a specific directory.
#[derive(Debug, Clone)]
pub struct ColumnPref {
    /// Directory path this preference applies to ("*" = global default).
    pub directory: String,
    /// Column ID.
    pub column_id: String,
    /// Whether the column is visible.
    pub visible: bool,
    /// Display width (0 = use default).
    pub width: u32,
    /// Sort position (lower = earlier).
    pub position: u32,
}

/// A fully resolved column for display.
#[derive(Debug, Clone)]
pub struct DisplayColumn {
    /// Column definition.
    pub def: ColumnDef,
    /// Actual display width.
    pub width: u32,
    /// Position in the view (0-based).
    pub position: u32,
    /// Whether currently sorted, and direction.
    pub sort: Option<SortDir>,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Ascending,
    Descending,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static COMPUTE_COUNT: AtomicU64 = AtomicU64::new(0);

static COLUMN_DEFS: spin::Mutex<Vec<ColumnDef>> = spin::Mutex::new(Vec::new());
static USER_PREFS: spin::Mutex<Vec<ColumnPref>> = spin::Mutex::new(Vec::new());
static INITIALIZED: spin::Mutex<bool> = spin::Mutex::new(false);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the column view system with built-in column definitions.
pub fn init() {
    let mut initialized = INITIALIZED.lock();
    if *initialized {
        return;
    }

    let defaults: Vec<(&str, &str, ColumnType, u32, &[&str], u32, &str)> = vec![
        // Universal columns (apply to all file types).
        ("name",        "Name",          ColumnType::Text,      200, &[],                    0,  "name"),
        ("size",        "Size",          ColumnType::Size,       80, &[],                    1,  "size"),
        ("modified",    "Date Modified", ColumnType::DateTime,  140, &[],                    2,  "modified"),
        ("type",        "Type",          ColumnType::Text,      100, &[],                    3,  "mime_type"),
        ("created",     "Date Created",  ColumnType::DateTime,  140, &[],                   10,  "created"),
        ("accessed",    "Date Accessed", ColumnType::DateTime,  140, &[],                   11,  "accessed"),
        ("permissions", "Permissions",   ColumnType::Text,       80, &[],                   12,  "permissions"),
        ("owner",       "Owner",         ColumnType::Text,       80, &[],                   13,  "owner"),

        // Audio columns.
        ("audio.title",     "Title",      ColumnType::Text,     160, &["audio/mpeg", "audio/flac", "audio/ogg", "audio/wav"],  20, "title"),
        ("audio.artist",    "Artist",     ColumnType::Text,     120, &["audio/mpeg", "audio/flac", "audio/ogg"],               21, "artist"),
        ("audio.album",     "Album",      ColumnType::Text,     120, &["audio/mpeg", "audio/flac", "audio/ogg"],               22, "album"),
        ("audio.duration",  "Duration",   ColumnType::Duration,  70, &["audio/mpeg", "audio/flac", "audio/ogg", "audio/wav"],  23, "duration"),
        ("audio.bitrate",   "Bitrate",    ColumnType::Integer,   70, &["audio/mpeg", "audio/flac", "audio/ogg"],               24, "bitrate"),
        ("audio.samplerate","Sample Rate",ColumnType::Integer,   80, &["audio/mpeg", "audio/flac", "audio/ogg", "audio/wav"],  25, "sample_rate"),
        ("audio.channels",  "Channels",   ColumnType::Text,      70, &["audio/mpeg", "audio/flac", "audio/ogg", "audio/wav"],  26, "channels"),
        ("audio.year",      "Year",       ColumnType::Integer,   50, &["audio/mpeg", "audio/flac", "audio/ogg"],               27, "year"),
        ("audio.genre",     "Genre",      ColumnType::Text,      80, &["audio/mpeg", "audio/flac", "audio/ogg"],               28, "genre"),
        ("audio.track",     "Track",      ColumnType::Integer,   50, &["audio/mpeg", "audio/flac", "audio/ogg"],               29, "track"),

        // Image columns.
        ("image.width",      "Width",       ColumnType::Integer,    60, &["image/png", "image/jpeg", "image/gif", "image/bmp", "image/webp"],  30, "width"),
        ("image.height",     "Height",      ColumnType::Integer,    60, &["image/png", "image/jpeg", "image/gif", "image/bmp", "image/webp"],  31, "height"),
        ("image.dimensions", "Dimensions",  ColumnType::Dimensions, 90, &["image/png", "image/jpeg", "image/gif", "image/bmp", "image/webp"],  32, "dimensions"),
        ("image.colordepth", "Color Depth", ColumnType::Integer,    80, &["image/png", "image/jpeg", "image/gif", "image/bmp"],                33, "color_depth"),

        // Video columns.
        ("video.duration",   "Duration",    ColumnType::Duration,   70, &["video/mp4", "video/webm", "video/x-matroska", "video/avi"],    40, "duration"),
        ("video.resolution", "Resolution",  ColumnType::Dimensions, 90, &["video/mp4", "video/webm", "video/x-matroska", "video/avi"],    41, "resolution"),
        ("video.codec",      "Video Codec", ColumnType::Text,       80, &["video/mp4", "video/webm", "video/x-matroska"],                 42, "video_codec"),

        // Document columns.
        ("doc.pages",   "Pages",   ColumnType::Integer, 50, &["application/pdf"],                                               50, "pages"),
        ("doc.words",   "Words",   ColumnType::Integer, 60, &["text/plain", "text/markdown", "application/rtf"],                 51, "words"),
        ("doc.lines",   "Lines",   ColumnType::Integer, 60, &["text/plain", "text/x-python", "text/x-c", "text/x-rust"],        52, "lines"),

        // Executable/binary columns.
        ("elf.arch",    "Architecture", ColumnType::Text, 80, &["application/x-executable", "application/x-sharedlib"],          60, "arch"),
        ("elf.type",    "Binary Type",  ColumnType::Text, 80, &["application/x-executable", "application/x-sharedlib"],          61, "binary_type"),
    ];

    let mut defs = COLUMN_DEFS.lock();
    for (id, header, col_type, width, applies_to, priority, field_name) in defaults {
        defs.push(ColumnDef {
            id: String::from(id),
            header: String::from(header),
            col_type,
            default_width: width,
            min_width: 30,
            sortable: true,
            applies_to: applies_to.iter().map(|s| String::from(*s)).collect(),
            source: String::from("system"),
            priority,
            system: true,
            field_name: String::from(field_name),
        });
    }

    *initialized = true;
}

// ---------------------------------------------------------------------------
// Column computation (the core design spec feature)
// ---------------------------------------------------------------------------

/// Compute the set of columns to display for a directory.
///
/// This is the main entry point implementing the design spec:
/// "use the subset of possible columns which is the union of the
/// columns relevant to each file in the given folder."
pub fn compute_columns(directory: &str) -> KernelResult<Vec<DisplayColumn>> {
    init();
    COMPUTE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Step 1: Scan directory to find all MIME types present.
    let entries = crate::fs::vfs::Vfs::readdir(directory)?;
    let mut mime_types: Vec<String> = Vec::new();

    for entry in &entries {
        if entry.entry_type == crate::fs::EntryType::File {
            let path = if directory == "/" {
                alloc::format!("/{}", entry.name)
            } else {
                alloc::format!("{}/{}", directory, entry.name)
            };
            if let Ok(mime) = crate::fs::mime::detect(&path) {
                let mime_str = String::from(mime);
                if !mime_types.contains(&mime_str) {
                    mime_types.push(mime_str);
                }
            }
        }
    }

    // Step 2: Find all columns that apply to present MIME types.
    let defs = COLUMN_DEFS.lock();
    let mut applicable: Vec<ColumnDef> = Vec::new();

    for def in defs.iter() {
        if def.applies_to.is_empty() {
            // Universal column — always include.
            applicable.push(def.clone());
        } else {
            // Type-specific — include only if a matching MIME type is present.
            let matches = def.applies_to.iter()
                .any(|dt| mime_types.iter().any(|mt| mt == dt));
            if matches {
                applicable.push(def.clone());
            }
        }
    }
    drop(defs);

    // Step 3: Apply user preferences.
    let prefs = USER_PREFS.lock();
    let mut columns: Vec<DisplayColumn> = Vec::new();

    for def in &applicable {
        // Check for directory-specific or global preference.
        let pref = prefs.iter()
            .find(|p| p.column_id == def.id && (p.directory == directory || p.directory == "*"));

        let visible = pref.map(|p| p.visible).unwrap_or(true);
        if !visible {
            continue;
        }

        let width = pref.and_then(|p| if p.width > 0 { Some(p.width) } else { None })
            .unwrap_or(def.default_width);
        let position = pref.map(|p| p.position).unwrap_or(def.priority);

        columns.push(DisplayColumn {
            def: def.clone(),
            width,
            position,
            sort: None,
        });
    }

    // Step 4: Sort by position.
    columns.sort_by(|a, b| a.position.cmp(&b.position));

    // Renumber positions.
    for (i, col) in columns.iter_mut().enumerate() {
        col.position = i as u32;
    }

    Ok(columns)
}

// ---------------------------------------------------------------------------
// Column definition management
// ---------------------------------------------------------------------------

/// Register a new column definition.
pub fn register_column(
    id: &str,
    header: &str,
    col_type: ColumnType,
    default_width: u32,
    applies_to: &[&str],
    field_name: &str,
    source: &str,
) -> KernelResult<()> {
    let mut defs = COLUMN_DEFS.lock();
    if defs.len() >= MAX_COLUMN_DEFS {
        return Err(KernelError::OutOfMemory);
    }

    // Don't allow duplicates.
    if defs.iter().any(|d| d.id == id) {
        return Err(KernelError::AlreadyExists);
    }

    defs.push(ColumnDef {
        id: String::from(id),
        header: String::from(header),
        col_type,
        default_width,
        min_width: 30,
        sortable: true,
        applies_to: applies_to.iter().map(|s| String::from(*s)).collect(),
        source: String::from(source),
        priority: 100, // App-registered columns go after system ones.
        system: false,
        field_name: String::from(field_name),
    });

    Ok(())
}

/// Unregister a column definition (only non-system).
pub fn unregister_column(id: &str) -> bool {
    let mut defs = COLUMN_DEFS.lock();
    let before = defs.len();
    defs.retain(|d| d.id != id || d.system);
    defs.len() < before
}

/// List all registered column definitions.
pub fn list_columns() -> Vec<ColumnDef> {
    init();
    let defs = COLUMN_DEFS.lock();
    let mut result: Vec<ColumnDef> = defs.clone();
    result.sort_by(|a, b| a.priority.cmp(&b.priority));
    result
}

/// Unregister all columns from a source.
pub fn unregister_source_columns(source: &str) -> usize {
    let mut defs = COLUMN_DEFS.lock();
    let before = defs.len();
    defs.retain(|d| d.source != source || d.system);
    before.saturating_sub(defs.len())
}

// ---------------------------------------------------------------------------
// User preferences
// ---------------------------------------------------------------------------

/// Set a user column preference.
pub fn set_preference(
    directory: &str,
    column_id: &str,
    visible: bool,
    width: u32,
    position: u32,
) -> KernelResult<()> {
    let mut prefs = USER_PREFS.lock();

    // Update existing or add new.
    if let Some(pref) = prefs.iter_mut().find(|p| p.directory == directory && p.column_id == column_id) {
        pref.visible = visible;
        pref.width = width;
        pref.position = position;
        return Ok(());
    }

    if prefs.len() >= MAX_USER_PREFS {
        return Err(KernelError::OutOfMemory);
    }

    prefs.push(ColumnPref {
        directory: String::from(directory),
        column_id: String::from(column_id),
        visible,
        width,
        position,
    });

    Ok(())
}

/// Remove user preferences for a directory.
pub fn clear_preferences(directory: &str) -> usize {
    let mut prefs = USER_PREFS.lock();
    let before = prefs.len();
    prefs.retain(|p| p.directory != directory);
    before.saturating_sub(prefs.len())
}

/// List user preferences.
pub fn list_preferences() -> Vec<ColumnPref> {
    USER_PREFS.lock().clone()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (column_count, pref_count, compute_count).
pub fn stats() -> (usize, usize, u64) {
    let col_count = COLUMN_DEFS.lock().len();
    let pref_count = USER_PREFS.lock().len();
    (col_count, pref_count, COMPUTE_COUNT.load(Ordering::Relaxed))
}

/// Reset statistics.
pub fn reset_stats() {
    COMPUTE_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the column view system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: initialization.
    {
        init();
        let (col_count, _, _) = stats();
        assert!(col_count > 0);
        serial_println!("[columnview] test 1 passed: init ({} columns)", col_count);
    }

    // Test 2: list columns.
    {
        let cols = list_columns();
        assert!(!cols.is_empty());
        // Should include "name", "size", etc.
        assert!(cols.iter().any(|c| c.id == "name"));
        assert!(cols.iter().any(|c| c.id == "size"));
        assert!(cols.iter().any(|c| c.id == "modified"));
        serial_println!("[columnview] test 2 passed: list columns");
    }

    // Test 3: type-specific columns exist.
    {
        let cols = list_columns();
        assert!(cols.iter().any(|c| c.id == "audio.bitrate"));
        assert!(cols.iter().any(|c| c.id == "image.width"));
        assert!(cols.iter().any(|c| c.id == "doc.pages"));
        serial_println!("[columnview] test 3 passed: type-specific columns");
    }

    // Test 4: register custom column.
    {
        register_column(
            "test.custom", "Test Col", ColumnType::Text, 100,
            &["application/x-test"], "test_field", "test-app",
        )?;
        let cols = list_columns();
        assert!(cols.iter().any(|c| c.id == "test.custom"));
        assert!(unregister_column("test.custom"));
        serial_println!("[columnview] test 4 passed: register + unregister");
    }

    // Test 5: user preferences.
    {
        set_preference("*", "size", true, 120, 1)?;
        let prefs = list_preferences();
        assert!(!prefs.is_empty());
        clear_preferences("*");
        serial_println!("[columnview] test 5 passed: user preferences");
    }

    // Test 6: column types.
    {
        assert_eq!(ColumnType::Text.alignment(), Alignment::Left);
        assert_eq!(ColumnType::Size.alignment(), Alignment::Right);
        assert_eq!(ColumnType::Boolean.alignment(), Alignment::Center);
        serial_println!("[columnview] test 6 passed: column types");
    }

    serial_println!("[columnview] all 6 self-tests passed");
    Ok(())
}
