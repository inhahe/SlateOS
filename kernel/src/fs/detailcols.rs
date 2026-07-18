//! Detail columns for file explorer — per-type metadata columns.
//!
//! Manages which metadata columns are visible in the file explorer's
//! detail view. Columns are type-aware: a folder of MP3 files shows
//! audio-specific columns (bitrate, duration, artist), while a folder
//! of images shows EXIF columns (width, height, camera). The displayed
//! columns are the union of relevant columns for all file types present.
//!
//! ## Design Reference
//!
//! design.txt lines 904-925: per-type detail columns, union of columns
//! for files in folder, user-configurable defaults per type, application-
//! extensible columns.
//!
//! ## Architecture
//!
//! ```text
//! File explorer detail view
//!   → detailcols::columns_for_types(&[mime_types]) → Vec<ColumnDef>
//!   → detailcols::column_value(path, column_id) → String
//!
//! Application (e.g., media player)
//!   → detailcols::register_column("audio/bitrate", ...)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered columns.
const MAX_COLUMNS: usize = 512;

/// Maximum columns per type binding.
const MAX_PER_TYPE: usize = 64;

/// Maximum type bindings.
const MAX_TYPE_BINDINGS: usize = 256;

/// Maximum user-visible column selections per type.
const MAX_USER_SELECTIONS: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Data type of a column value (for sorting and display).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    /// Plain text.
    Text,
    /// Integer number.
    Integer,
    /// Floating point.
    Float,
    /// File size (bytes, formatted as KB/MB/GB).
    Size,
    /// Timestamp (nanoseconds since epoch).
    Timestamp,
    /// Duration (seconds).
    Duration,
    /// Boolean (yes/no).
    Boolean,
}

impl ColumnType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Size => "size",
            Self::Timestamp => "timestamp",
            Self::Duration => "duration",
            Self::Boolean => "boolean",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "text" | "string" => Some(Self::Text),
            "int" | "integer" => Some(Self::Integer),
            "float" | "decimal" => Some(Self::Float),
            "size" | "bytes" => Some(Self::Size),
            "time" | "timestamp" | "date" => Some(Self::Timestamp),
            "duration" | "length" => Some(Self::Duration),
            "bool" | "boolean" => Some(Self::Boolean),
            _ => None,
        }
    }
}

/// Column category for grouping in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnCategory {
    /// Standard file attributes (name, size, date, permissions).
    Standard,
    /// Audio metadata (bitrate, duration, artist, album).
    Audio,
    /// Image/photo metadata (dimensions, camera, EXIF).
    Image,
    /// Video metadata (resolution, codec, framerate).
    Video,
    /// Document metadata (author, page count, title).
    Document,
    /// Archive metadata (compressed size, file count).
    Archive,
    /// Application-defined columns.
    AppDefined,
}

impl ColumnCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Audio => "audio",
            Self::Image => "image",
            Self::Video => "video",
            Self::Document => "document",
            Self::Archive => "archive",
            Self::AppDefined => "app",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "standard" | "std" => Some(Self::Standard),
            "audio" | "music" => Some(Self::Audio),
            "image" | "photo" => Some(Self::Image),
            "video" => Some(Self::Video),
            "document" | "doc" => Some(Self::Document),
            "archive" | "zip" => Some(Self::Archive),
            "app" | "custom" => Some(Self::AppDefined),
            _ => None,
        }
    }
}

/// A column definition.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    /// Unique column identifier (e.g., "audio.bitrate", "image.width").
    pub id: String,
    /// Display name (e.g., "Bitrate", "Width").
    pub display_name: String,
    /// Data type.
    pub col_type: ColumnType,
    /// Category.
    pub category: ColumnCategory,
    /// Default width in characters (for UI layout).
    pub default_width: u16,
    /// Whether this column is sortable.
    pub sortable: bool,
    /// Application ID that registered this column (empty = built-in).
    pub source_app: String,
}

/// Binding of a column to a MIME type or file category.
#[derive(Debug, Clone)]
pub struct TypeBinding {
    /// MIME type pattern (e.g., "audio/*", "image/png", "*").
    pub mime_pattern: String,
    /// Column IDs bound to this type.
    pub column_ids: Vec<String>,
}

/// User selection: which columns to show for a given type.
#[derive(Debug, Clone)]
pub struct UserSelection {
    /// MIME pattern this selection applies to.
    pub mime_pattern: String,
    /// Column IDs the user wants visible (in order).
    pub visible_columns: Vec<String>,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    /// All registered column definitions.
    columns: Vec<ColumnDef>,
    /// Type → columns bindings.
    bindings: Vec<TypeBinding>,
    /// User overrides for column visibility per type.
    user_selections: Vec<UserSelection>,
}

impl State {
    const fn new() -> Self {
        Self {
            columns: Vec::new(),
            bindings: Vec::new(),
            user_selections: Vec::new(),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Column registration
// ---------------------------------------------------------------------------

/// Register a column definition.
pub fn register_column(id: &str, name: &str, col_type: ColumnType, category: ColumnCategory, width: u16, sortable: bool, app: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.columns.len() >= MAX_COLUMNS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.columns.iter().any(|c| c.id == id) {
        return Err(KernelError::AlreadyExists);
    }
    state.columns.push(ColumnDef {
        id: String::from(id),
        display_name: String::from(name),
        col_type,
        category,
        default_width: width,
        sortable,
        source_app: String::from(app),
    });
    Ok(())
}

/// Unregister a column (only app-defined columns can be removed).
pub fn unregister_column(id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let col = state.columns.iter().find(|c| c.id == id)
        .ok_or(KernelError::NotFound)?;
    if col.source_app.is_empty() {
        return Err(KernelError::PermissionDenied); // Built-in columns can't be removed.
    }
    state.columns.retain(|c| c.id != id);
    // Also clean up bindings.
    for b in &mut state.bindings {
        b.column_ids.retain(|cid| cid != id);
    }
    Ok(())
}

/// Get a column definition.
pub fn get_column(id: &str) -> KernelResult<ColumnDef> {
    STATE.lock().columns.iter().find(|c| c.id == id).cloned().ok_or(KernelError::NotFound)
}

/// List all columns, optionally filtered by category.
pub fn list_columns(category: Option<ColumnCategory>) -> Vec<ColumnDef> {
    let state = STATE.lock();
    state.columns.iter()
        .filter(|c| category.is_none_or(|cat| c.category == cat))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Type bindings
// ---------------------------------------------------------------------------

/// Bind columns to a MIME type pattern.
pub fn bind_columns(mime_pattern: &str, column_ids: &[&str]) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Validate all column IDs exist.
    for cid in column_ids {
        if !state.columns.iter().any(|c| c.id == *cid) {
            return Err(KernelError::NotFound);
        }
    }
    // Replace existing binding for this pattern.
    state.bindings.retain(|b| b.mime_pattern != mime_pattern);
    if state.bindings.len() >= MAX_TYPE_BINDINGS {
        return Err(KernelError::ResourceExhausted);
    }
    let ids: Vec<String> = column_ids.iter().map(|s| String::from(*s)).collect();
    state.bindings.push(TypeBinding {
        mime_pattern: String::from(mime_pattern),
        column_ids: ids,
    });
    Ok(())
}

/// Remove binding for a MIME pattern.
pub fn unbind(mime_pattern: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.bindings.len();
    state.bindings.retain(|b| b.mime_pattern != mime_pattern);
    if state.bindings.len() == len { return Err(KernelError::NotFound); }
    Ok(())
}

/// List all bindings.
pub fn list_bindings() -> Vec<TypeBinding> {
    STATE.lock().bindings.clone()
}

// ---------------------------------------------------------------------------
// Column resolution
// ---------------------------------------------------------------------------

/// Check if a MIME pattern matches a MIME type.
fn pattern_matches(pattern: &str, mime: &str) -> bool {
    if pattern == "*" { return true; }
    if pattern == mime { return true; }
    // Wildcard suffix: "audio/*" matches "audio/mpeg"
    if let Some(prefix) = pattern.strip_suffix("/*") {
        if let Some(type_prefix) = mime.split('/').next() {
            return prefix == type_prefix;
        }
    }
    false
}

/// Compute the set of columns relevant for a set of MIME types.
///
/// This is the core function called by the file explorer: given the
/// MIME types present in a folder, return the union of all relevant
/// columns (per design.txt line 904).
pub fn columns_for_types(mime_types: &[&str]) -> Vec<ColumnDef> {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
    let state = STATE.lock();

    // Collect unique column IDs from all matching bindings.
    let mut seen_ids: Vec<String> = Vec::new();

    for mime in mime_types {
        // Check user selections first.
        if let Some(sel) = state.user_selections.iter().find(|s| pattern_matches(&s.mime_pattern, mime)) {
            for cid in &sel.visible_columns {
                if !seen_ids.iter().any(|s| s == cid) {
                    seen_ids.push(cid.clone());
                }
            }
            continue;
        }
        // Fall back to default bindings.
        for binding in &state.bindings {
            if pattern_matches(&binding.mime_pattern, mime) {
                for cid in &binding.column_ids {
                    if !seen_ids.iter().any(|s| s == cid) {
                        seen_ids.push(cid.clone());
                    }
                }
            }
        }
    }

    // Always include standard columns first.
    let mut result: Vec<ColumnDef> = Vec::new();
    // Standard columns first (in registration order).
    for col in &state.columns {
        if col.category == ColumnCategory::Standard && !seen_ids.contains(&col.id) {
            // Always include standard columns.
            result.push(col.clone());
        }
    }
    // Then the type-specific columns in the order they were collected.
    for cid in &seen_ids {
        if let Some(col) = state.columns.iter().find(|c| c.id == *cid) {
            if !result.iter().any(|r| r.id == *cid) {
                result.push(col.clone());
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// User selections
// ---------------------------------------------------------------------------

/// Set user-visible columns for a MIME pattern.
pub fn set_user_columns(mime_pattern: &str, column_ids: &[&str]) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.user_selections.retain(|s| s.mime_pattern != mime_pattern);
    if state.user_selections.len() >= MAX_USER_SELECTIONS {
        return Err(KernelError::ResourceExhausted);
    }
    let ids: Vec<String> = column_ids.iter().map(|s| String::from(*s)).collect();
    state.user_selections.push(UserSelection {
        mime_pattern: String::from(mime_pattern),
        visible_columns: ids,
    });
    Ok(())
}

/// Remove user selection for a pattern (revert to defaults).
pub fn clear_user_columns(mime_pattern: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.user_selections.len();
    state.user_selections.retain(|s| s.mime_pattern != mime_pattern);
    if state.user_selections.len() == len { return Err(KernelError::NotFound); }
    Ok(())
}

/// List user selections.
pub fn list_user_selections() -> Vec<UserSelection> {
    STATE.lock().user_selections.clone()
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Initialize built-in columns and default bindings.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.columns.is_empty() { return; }

    // --- Standard columns (always available) ---
    let std_cols = [
        ("std.name", "Name", ColumnType::Text, 30),
        ("std.size", "Size", ColumnType::Size, 10),
        ("std.type", "Type", ColumnType::Text, 15),
        ("std.modified", "Date Modified", ColumnType::Timestamp, 20),
        ("std.created", "Date Created", ColumnType::Timestamp, 20),
        ("std.accessed", "Date Accessed", ColumnType::Timestamp, 20),
        ("std.permissions", "Permissions", ColumnType::Text, 12),
        ("std.owner", "Owner", ColumnType::Text, 12),
    ];
    for &(id, name, ct, w) in &std_cols {
        state.columns.push(ColumnDef {
            id: String::from(id), display_name: String::from(name),
            col_type: ct, category: ColumnCategory::Standard,
            default_width: w, sortable: true, source_app: String::new(),
        });
    }

    // --- Audio columns ---
    let audio_cols = [
        ("audio.duration", "Duration", ColumnType::Duration, 10),
        ("audio.bitrate", "Bitrate", ColumnType::Integer, 8),
        ("audio.samplerate", "Sample Rate", ColumnType::Integer, 10),
        ("audio.channels", "Channels", ColumnType::Text, 8),
        ("audio.vbr", "VBR", ColumnType::Boolean, 5),
        ("audio.title", "Title", ColumnType::Text, 25),
        ("audio.artist", "Artist", ColumnType::Text, 20),
        ("audio.album", "Album", ColumnType::Text, 20),
        ("audio.year", "Year", ColumnType::Integer, 6),
        ("audio.genre", "Genre", ColumnType::Text, 12),
        ("audio.track", "Track #", ColumnType::Integer, 6),
    ];
    for &(id, name, ct, w) in &audio_cols {
        state.columns.push(ColumnDef {
            id: String::from(id), display_name: String::from(name),
            col_type: ct, category: ColumnCategory::Audio,
            default_width: w, sortable: true, source_app: String::new(),
        });
    }

    // --- Image columns ---
    let image_cols = [
        ("image.width", "Width", ColumnType::Integer, 8),
        ("image.height", "Height", ColumnType::Integer, 8),
        ("image.depth", "Color Depth", ColumnType::Integer, 6),
        ("image.camera", "Camera", ColumnType::Text, 20),
        ("image.exposure", "Exposure", ColumnType::Text, 12),
        ("image.fstop", "F-Stop", ColumnType::Float, 6),
        ("image.iso", "ISO", ColumnType::Integer, 6),
        ("image.focal", "Focal Length", ColumnType::Text, 10),
        ("image.gps", "GPS", ColumnType::Text, 20),
    ];
    for &(id, name, ct, w) in &image_cols {
        state.columns.push(ColumnDef {
            id: String::from(id), display_name: String::from(name),
            col_type: ct, category: ColumnCategory::Image,
            default_width: w, sortable: true, source_app: String::new(),
        });
    }

    // --- Video columns ---
    let video_cols = [
        ("video.duration", "Duration", ColumnType::Duration, 10),
        ("video.width", "Width", ColumnType::Integer, 8),
        ("video.height", "Height", ColumnType::Integer, 8),
        ("video.codec", "Codec", ColumnType::Text, 10),
        ("video.framerate", "Frame Rate", ColumnType::Float, 8),
        ("video.bitrate", "Bitrate", ColumnType::Integer, 10),
    ];
    for &(id, name, ct, w) in &video_cols {
        state.columns.push(ColumnDef {
            id: String::from(id), display_name: String::from(name),
            col_type: ct, category: ColumnCategory::Video,
            default_width: w, sortable: true, source_app: String::new(),
        });
    }

    // --- Document columns ---
    let doc_cols = [
        ("doc.pages", "Pages", ColumnType::Integer, 6),
        ("doc.author", "Author", ColumnType::Text, 20),
        ("doc.title", "Title", ColumnType::Text, 25),
        ("doc.words", "Words", ColumnType::Integer, 8),
    ];
    for &(id, name, ct, w) in &doc_cols {
        state.columns.push(ColumnDef {
            id: String::from(id), display_name: String::from(name),
            col_type: ct, category: ColumnCategory::Document,
            default_width: w, sortable: true, source_app: String::new(),
        });
    }

    // --- Archive columns ---
    let arch_cols = [
        ("archive.files", "File Count", ColumnType::Integer, 8),
        ("archive.compressed", "Compressed Size", ColumnType::Size, 12),
        ("archive.ratio", "Ratio", ColumnType::Float, 6),
    ];
    for &(id, name, ct, w) in &arch_cols {
        state.columns.push(ColumnDef {
            id: String::from(id), display_name: String::from(name),
            col_type: ct, category: ColumnCategory::Archive,
            default_width: w, sortable: true, source_app: String::new(),
        });
    }

    // --- Default type bindings ---
    // Audio types → audio columns.
    let audio_ids: Vec<String> = audio_cols.iter().map(|c| String::from(c.0)).collect();
    let audio_id_refs: Vec<&str> = audio_ids.iter().map(|s| s.as_str()).collect();
    state.bindings.push(TypeBinding {
        mime_pattern: String::from("audio/*"),
        column_ids: audio_id_refs.iter().map(|s| String::from(*s)).collect(),
    });

    // Image types → image columns.
    let image_ids: Vec<String> = image_cols.iter().map(|c| String::from(c.0)).collect();
    state.bindings.push(TypeBinding {
        mime_pattern: String::from("image/*"),
        column_ids: image_ids,
    });

    // Video types → video columns.
    let video_ids: Vec<String> = video_cols.iter().map(|c| String::from(c.0)).collect();
    state.bindings.push(TypeBinding {
        mime_pattern: String::from("video/*"),
        column_ids: video_ids,
    });

    // Document types.
    let doc_ids: Vec<String> = doc_cols.iter().map(|c| String::from(c.0)).collect();
    state.bindings.push(TypeBinding {
        mime_pattern: String::from("application/pdf"),
        column_ids: doc_ids.clone(),
    });
    state.bindings.push(TypeBinding {
        mime_pattern: String::from("application/msword"),
        column_ids: doc_ids,
    });

    // Archive types.
    let arch_ids: Vec<String> = arch_cols.iter().map(|c| String::from(c.0)).collect();
    state.bindings.push(TypeBinding {
        mime_pattern: String::from("application/zip"),
        column_ids: arch_ids.clone(),
    });
    state.bindings.push(TypeBinding {
        mime_pattern: String::from("application/x-tar"),
        column_ids: arch_ids,
    });
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (column_count, binding_count, user_selection_count, queries).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    (state.columns.len(), state.bindings.len(), state.user_selections.len(), QUERY_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() { QUERY_COUNT.store(0, Ordering::Relaxed); }

pub fn clear_all() {
    let mut state = STATE.lock();
    state.columns.clear();
    state.bindings.clear();
    state.user_selections.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Init defaults.
    serial_println!("  detailcols::self_test 1: init defaults");
    init_defaults();
    let cols = list_columns(None);
    assert!(cols.len() >= 30); // Standard + audio + image + video + doc + archive.

    // Test 2: Category filtering.
    serial_println!("  detailcols::self_test 2: category filter");
    let audio = list_columns(Some(ColumnCategory::Audio));
    assert!(audio.len() >= 11);
    let image = list_columns(Some(ColumnCategory::Image));
    assert!(image.len() >= 9);

    // Test 3: Columns for audio types.
    serial_println!("  detailcols::self_test 3: columns for audio");
    let cols_audio = columns_for_types(&["audio/mpeg"]);
    assert!(cols_audio.iter().any(|c| c.id == "audio.bitrate"));
    assert!(cols_audio.iter().any(|c| c.id == "audio.artist"));
    // Should also include standard columns.
    assert!(cols_audio.iter().any(|c| c.id == "std.name"));

    // Test 4: Union of types.
    serial_println!("  detailcols::self_test 4: union of types");
    let cols_mixed = columns_for_types(&["audio/mpeg", "image/png"]);
    assert!(cols_mixed.iter().any(|c| c.id == "audio.bitrate"));
    assert!(cols_mixed.iter().any(|c| c.id == "image.width"));

    // Test 5: Custom column registration.
    serial_println!("  detailcols::self_test 5: custom column");
    register_column("app.myfield", "My Field", ColumnType::Text, ColumnCategory::AppDefined, 15, true, "myapp")?;
    let col = get_column("app.myfield")?;
    assert_eq!(col.display_name, "My Field");
    bind_columns("text/plain", &["app.myfield"])?;
    let cols_text = columns_for_types(&["text/plain"]);
    assert!(cols_text.iter().any(|c| c.id == "app.myfield"));
    unregister_column("app.myfield")?;

    // Test 6: User selections override.
    serial_println!("  detailcols::self_test 6: user selections");
    set_user_columns("audio/*", &["audio.title", "audio.artist"])?;
    let cols_user = columns_for_types(&["audio/flac"]);
    // User selection should be used instead of default binding.
    assert!(cols_user.iter().any(|c| c.id == "audio.title"));
    assert!(cols_user.iter().any(|c| c.id == "audio.artist"));
    clear_user_columns("audio/*")?;

    // Test 7: Stats.
    serial_println!("  detailcols::self_test 7: stats");
    let (cc, bc, uc, qc) = stats();
    assert!(cc >= 30);
    assert!(bc >= 5);
    assert_eq!(uc, 0);
    assert!(qc > 0);

    clear_all();
    reset_stats();
    serial_println!("  detailcols: all tests passed");
    Ok(())
}
