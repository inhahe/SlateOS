//! Extensible column system for the file explorer's detail view.
//!
//! Provides a pluggable architecture where **column providers** supply
//! per-file-type metadata columns (image dimensions, audio duration,
//! code line count, etc.) that the detail view renders alongside the
//! built-in Name/Size/Date/Type columns.
//!
//! ## Architecture
//!
//! - [`ColumnDef`] describes a single column (label, width, alignment,
//!   sort state, category).
//! - [`ColumnValue`] is the typed value for one cell (text, number,
//!   formatted size, duration, percentage, or empty).
//! - [`ColumnProvider`] is the trait that apps or built-in providers
//!   implement to feed data into columns.
//! - [`ColumnManager`] owns the set of registered providers and active
//!   columns, handles visibility / reorder / resize / sort, and looks
//!   up values by delegating to the correct provider.
//!
//! ## Built-in providers
//!
//! | Provider | Extensions | Columns |
//! |---|---|---|
//! | [`StandardColumns`] | *(all files)* | Name, Size, Date Modified, Type, Date Created, Attributes |
//! | [`ImageColumns`] | png, jpg, gif, bmp, svg | Dimensions, Color Depth, Aspect Ratio |
//! | [`AudioColumns`] | mp3, wav, flac, ogg | Duration, Bitrate, Sample Rate, Artist, Album, Title |
//! | [`CodeColumns`] | rs, c, cpp, py, js, ts, ... | Line Count, Language |
//! | [`ArchiveColumns`] | zip, tar, gz | Compressed Size, Compression Ratio, File Count Inside |

#![allow(dead_code)]

use guitk::color::Color;
use guitk::date;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

use std::cmp::Ordering;
use std::collections::HashMap;

// ============================================================================
// Column identity
// ============================================================================

/// Unique identifier for a column.
///
/// The u32 value is partitioned by provider:
/// - 0..99     — Standard columns
/// - 100..199  — Image columns
/// - 200..299  — Audio columns
/// - 300..399  — Code columns
/// - 400..499  — Archive columns
/// - 500+      — App-contributed columns
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColumnId(pub u32);

// Well-known standard column IDs.
impl ColumnId {
    pub const NAME: Self = Self(0);
    pub const SIZE: Self = Self(1);
    pub const DATE_MODIFIED: Self = Self(2);
    pub const TYPE: Self = Self(3);
    pub const DATE_CREATED: Self = Self(4);
    pub const ATTRIBUTES: Self = Self(5);

    // Image
    pub const DIMENSIONS: Self = Self(100);
    pub const COLOR_DEPTH: Self = Self(101);
    pub const ASPECT_RATIO: Self = Self(102);

    // Audio
    pub const DURATION: Self = Self(200);
    pub const BITRATE: Self = Self(201);
    pub const SAMPLE_RATE: Self = Self(202);
    pub const ARTIST: Self = Self(203);
    pub const ALBUM: Self = Self(204);
    pub const TITLE: Self = Self(205);

    // Code
    pub const LINE_COUNT: Self = Self(300);
    pub const LANGUAGE: Self = Self(301);

    // Archive
    pub const COMPRESSED_SIZE: Self = Self(400);
    pub const COMPRESSION_RATIO: Self = Self(401);
    pub const FILE_COUNT_INSIDE: Self = Self(402);
}

// ============================================================================
// Column width
// ============================================================================

/// How a column's width is determined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    /// Fixed pixel width.
    Fixed(f32),
    /// Flexible: grows/shrinks between min and max.
    Flexible { min: f32, max: f32 },
}

impl ColumnWidth {
    /// Resolve the effective width, clamping flexible columns to the given
    /// `available` space (or their max, whichever is smaller).
    pub fn resolve(&self, available: f32) -> f32 {
        match *self {
            Self::Fixed(px) => px,
            Self::Flexible { min, max } => available.clamp(min, max),
        }
    }
}

// ============================================================================
// Alignment
// ============================================================================

/// Text alignment within a column cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Alignment {
    #[default]
    Left,
    Right,
    Center,
}

// ============================================================================
// Sort order
// ============================================================================

/// Sort state for a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    None,
    Ascending,
    Descending,
}

// ============================================================================
// Column category
// ============================================================================

/// Grouping category shown in the column chooser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnCategory {
    Standard,
    Image,
    Audio,
    Video,
    Code,
    Document,
    Archive,
}

impl ColumnCategory {
    /// Human-readable label for the chooser UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Image => "Image",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Code => "Code",
            Self::Document => "Document",
            Self::Archive => "Archive",
        }
    }
}

// ============================================================================
// Column definition
// ============================================================================

/// Full description of one column.
#[derive(Clone, Debug)]
pub struct ColumnDef {
    pub id: ColumnId,
    /// Header text displayed in the column header row.
    pub label: String,
    /// How the column's width is determined.
    pub width: ColumnWidth,
    /// Text alignment within cells.
    pub alignment: Alignment,
    /// Whether clicking the header sorts by this column.
    pub sortable: bool,
    /// Current sort state.
    pub sort_order: SortOrder,
    /// Whether the column is currently visible.
    pub visible: bool,
    /// Grouping category for the chooser.
    pub category: ColumnCategory,
}

// ============================================================================
// Column value
// ============================================================================

/// A typed cell value that knows how to format itself for display.
///
/// `PartialEq`, `Eq`, `PartialOrd` and `Ord` are all written in terms of one
/// hand-written [`Ord::cmp`], so the four cannot disagree. See that impl for
/// why they are not derived.
#[derive(Clone, Debug)]
pub enum ColumnValue {
    /// Plain text.
    Text(String),
    /// Integer value displayed as-is.
    Number(i64),
    /// Byte count formatted as "1.2 MB".
    Size(u64),
    /// Unix-epoch seconds formatted as a date/time string.
    DateTime(u64),
    /// Duration in seconds formatted as "m:ss" or "h:mm:ss".
    Duration(u64),
    /// Fraction 0.0..1.0 formatted as "85%".
    Percentage(f32),
    /// No value for this cell.
    Empty,
}

impl ColumnValue {
    /// Render the value to a display string.
    pub fn display(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Number(n) => format_number(*n),
            Self::Size(bytes) => format_size(*bytes),
            Self::DateTime(epoch) => format_datetime(*epoch),
            Self::Duration(secs) => format_duration(*secs),
            Self::Percentage(frac) => format_percentage(*frac),
            Self::Empty => String::new(),
        }
    }

    /// Rank of the value's *kind*, the primary sort key when two cells in one
    /// column hold different kinds — which happens whenever a provider has a
    /// value for one file and not another. `Empty` always sorts last so blank
    /// cells collect at the bottom rather than the top.
    fn kind_rank(&self) -> u8 {
        match self {
            Self::Text(_) => 0,
            Self::Number(_) => 1,
            Self::Size(_) => 2,
            Self::DateTime(_) => 3,
            Self::Duration(_) => 4,
            Self::Percentage(_) => 5,
            Self::Empty => 255,
        }
    }
}

// `PartialEq` is written in terms of `cmp` rather than derived, and `cmp` is
// written by hand rather than derived, for reasons that are worth stating
// because the obvious shortcuts were both taken here before and both wrong.
//
// **Why not a packed integer key.** The original `sort_key` returned
// `(u8, i128)` and packed a text value into the i128 as its first sixteen
// lowercased bytes. That has two defects. The first is a truncation: two names
// sharing a sixteen-byte prefix compared `Equal` while `PartialEq` called them
// different, so `Ord`'s contract — `cmp` is `Equal` exactly when `==` — was
// violated, and `sort_by` is permitted to do anything at all with a comparator
// like that. The second is a sign flip: the leading byte was shifted into bit
// 127, the i128 sign bit, so any name whose first byte is >= 0x80 — that is,
// *every* name that does not begin with an ASCII character — produced a
// negative key and sorted before every ASCII name. A folder of accented or
// CJK filenames sorted as one block ahead of the rest, for no reason a user
// could see. Comparing the strings directly has neither problem and is
// cheaper: it stops at the first differing character instead of walking
// sixteen bytes and allocating a lowercased copy of the whole name.
//
// **Why not derive `Eq`.** `Percentage` holds an `f32`, and a derived
// `PartialEq` makes NaN unequal to itself, which `Eq` forbids. Rather than
// give up the total order, `cmp` uses `f32::total_cmp` and `eq` defers to it:
// a NaN percentage then equals itself and sorts after every real one. A NaN
// is a nonsense value in a percentage column either way, but this way it has
// a defined place instead of making the whole sort's behaviour arbitrary.
impl Ord for ColumnValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // Case-insensitive, because that is what every file manager does
            // and what the old key intended with its `to_lowercase`. The
            // tie-break on the raw strings is what keeps `cmp` and `eq`
            // consistent: without it `README` and `readme` would compare
            // `Equal` while `==` called them different.
            (Self::Text(a), Self::Text(b)) => {
                textfind::compare(a, b, textfind::Case::Insensitive).then_with(|| a.cmp(b))
            }
            (Self::Number(a), Self::Number(b)) => a.cmp(b),
            (Self::Size(a), Self::Size(b)) => a.cmp(b),
            (Self::DateTime(a), Self::DateTime(b)) => a.cmp(b),
            (Self::Duration(a), Self::Duration(b)) => a.cmp(b),
            (Self::Percentage(a), Self::Percentage(b)) => a.total_cmp(b),
            (Self::Empty, Self::Empty) => Ordering::Equal,
            _ => self.kind_rank().cmp(&other.kind_rank()),
        }
    }
}

impl PartialOrd for ColumnValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ColumnValue {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ColumnValue {}

// ============================================================================
// Column provider trait
// ============================================================================

/// Trait for components that supply column definitions and per-file values.
///
/// Built-in providers handle images, audio, code, and archives.
/// Applications can register additional providers for custom columns.
pub trait ColumnProvider {
    /// The column definitions this provider supplies.
    fn columns(&self) -> &[ColumnDef];

    /// Look up the value for a given file path and column.
    ///
    /// Must return [`ColumnValue::Empty`] for columns this provider
    /// does not own or files it does not handle.
    fn value(&self, path: &str, column_id: ColumnId) -> ColumnValue;

    /// File extensions this provider applies to (lowercase, without dot).
    ///
    /// An empty slice means the provider applies to all files.
    fn supported_extensions(&self) -> &[&str];
}

// ============================================================================
// File info — lightweight struct passed to auto_detect_columns
// ============================================================================

/// Minimal file info needed for column auto-detection.
pub struct FileInfo<'a> {
    pub path: &'a str,
    pub extension: &'a str,
}

// ============================================================================
// Column manager
// ============================================================================

/// Manages the active column set, registered providers, and column state.
pub struct ColumnManager {
    /// All known column definitions, keyed by id.
    all_columns: HashMap<ColumnId, ColumnDef>,
    /// The ordered list of column IDs currently shown.
    active_columns: Vec<ColumnId>,
    /// Registered providers.
    providers: Vec<Box<dyn ColumnProvider>>,
    /// Which column is currently being sorted, if any.
    sort_column: Option<ColumnId>,
    /// Current sort direction.
    sort_direction: SortOrder,
}

impl ColumnManager {
    /// Create a new manager with no providers registered.
    pub fn new() -> Self {
        Self {
            all_columns: HashMap::new(),
            active_columns: Vec::new(),
            providers: Vec::new(),
            sort_column: None,
            sort_direction: SortOrder::None,
        }
    }

    /// Create a manager pre-loaded with all built-in providers and
    /// standard columns active.
    pub fn with_defaults() -> Self {
        let mut mgr = Self::new();
        mgr.register_provider(Box::new(StandardColumns));
        mgr.register_provider(Box::new(ImageColumns));
        mgr.register_provider(Box::new(AudioColumns));
        mgr.register_provider(Box::new(CodeColumns));
        mgr.register_provider(Box::new(ArchiveColumns));

        // Activate standard columns by default.
        mgr.active_columns = vec![
            ColumnId::NAME,
            ColumnId::SIZE,
            ColumnId::DATE_MODIFIED,
            ColumnId::TYPE,
        ];
        mgr
    }

    // ------------------------------------------------------------------
    // Provider registration
    // ------------------------------------------------------------------

    /// Register a column provider.  Its column definitions are merged
    /// into the known set (last registration wins for duplicate IDs).
    pub fn register_provider(&mut self, provider: Box<dyn ColumnProvider>) {
        for col in provider.columns() {
            self.all_columns.insert(col.id, col.clone());
        }
        self.providers.push(provider);
    }

    // ------------------------------------------------------------------
    // Column visibility
    // ------------------------------------------------------------------

    /// Replace the entire active column set (order preserved).
    pub fn set_columns(&mut self, columns: Vec<ColumnId>) {
        self.active_columns = columns;
    }

    /// Add a column to the visible set (appended at the end).
    /// No-op if already visible.
    pub fn add_column(&mut self, id: ColumnId) {
        if !self.active_columns.contains(&id) {
            self.active_columns.push(id);
        }
    }

    /// Remove a column from the visible set.
    pub fn remove_column(&mut self, id: ColumnId) {
        self.active_columns.retain(|c| *c != id);
    }

    /// Whether a column is currently visible.
    pub fn is_visible(&self, id: ColumnId) -> bool {
        self.active_columns.contains(&id)
    }

    /// The currently active (visible) columns in display order.
    pub fn active_columns(&self) -> &[ColumnId] {
        &self.active_columns
    }

    /// All known column definitions across all providers.
    pub fn all_column_defs(&self) -> Vec<&ColumnDef> {
        self.all_columns.values().collect()
    }

    /// Look up a column definition by id.
    pub fn column_def(&self, id: ColumnId) -> Option<&ColumnDef> {
        self.all_columns.get(&id)
    }

    // ------------------------------------------------------------------
    // Reorder / resize
    // ------------------------------------------------------------------

    /// Move the column at index `from` to index `to`.
    /// Out-of-bounds indices are clamped.
    pub fn reorder(&mut self, from: usize, to: usize) {
        let len = self.active_columns.len();
        if len == 0 {
            return;
        }
        let from = from.min(len - 1);
        let to = to.min(len - 1);
        if from == to {
            return;
        }
        let id = self.active_columns.remove(from);
        self.active_columns.insert(to, id);
    }

    /// Resize a column, clamping to its width constraints.
    pub fn resize(&mut self, id: ColumnId, width: f32) {
        if let Some(def) = self.all_columns.get_mut(&id) {
            def.width = match def.width {
                ColumnWidth::Fixed(_) => ColumnWidth::Fixed(width.max(20.0)),
                ColumnWidth::Flexible { min, max } => ColumnWidth::Fixed(width.clamp(min, max)),
            };
        }
    }

    // ------------------------------------------------------------------
    // Sorting
    // ------------------------------------------------------------------

    /// Set (or toggle) the sort column.  If already sorting by `id`,
    /// flip direction; otherwise start ascending.
    pub fn sort_by(&mut self, id: ColumnId) {
        if self.sort_column == Some(id) {
            self.sort_direction = match self.sort_direction {
                SortOrder::Ascending => SortOrder::Descending,
                SortOrder::Descending => SortOrder::None,
                SortOrder::None => SortOrder::Ascending,
            };
        } else {
            // Clear previous column's sort indicator.
            if let Some(prev) = self.sort_column
                && let Some(def) = self.all_columns.get_mut(&prev)
            {
                def.sort_order = SortOrder::None;
            }
            self.sort_column = Some(id);
            self.sort_direction = SortOrder::Ascending;
        }

        // Update the definition's stored sort order.
        if let Some(def) = self.all_columns.get_mut(&id) {
            def.sort_order = self.sort_direction;
        }
    }

    /// Currently active sort column and direction.
    pub fn current_sort(&self) -> (Option<ColumnId>, SortOrder) {
        (self.sort_column, self.sort_direction)
    }

    // ------------------------------------------------------------------
    // Value lookup
    // ------------------------------------------------------------------

    /// Look up the value for a file path and column, delegating to the
    /// first provider that owns that column and supports the file's
    /// extension.
    pub fn get_value(&self, path: &str, column_id: ColumnId) -> ColumnValue {
        let ext = path_extension(path);

        for provider in &self.providers {
            // Check whether this provider owns the requested column.
            let owns_column = provider.columns().iter().any(|c| c.id == column_id);
            if !owns_column {
                continue;
            }

            // Check extension match (empty = universal provider).
            let exts = provider.supported_extensions();
            if !exts.is_empty() && !exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                continue;
            }

            return provider.value(path, column_id);
        }

        ColumnValue::Empty
    }

    // ------------------------------------------------------------------
    // Auto-detection
    // ------------------------------------------------------------------

    /// Examine the files in view and automatically enable relevant
    /// category columns.  For example, if the directory contains .png
    /// and .jpg files, the Image columns become active.
    pub fn auto_detect_columns(&mut self, files: &[FileInfo<'_>]) {
        // Always keep standard columns.
        let mut detected: Vec<ColumnId> = vec![
            ColumnId::NAME,
            ColumnId::SIZE,
            ColumnId::DATE_MODIFIED,
            ColumnId::TYPE,
        ];

        let mut has_image = false;
        let mut has_audio = false;
        let mut has_code = false;
        let mut has_archive = false;

        for file in files {
            let ext = file.extension.to_lowercase();
            match ext.as_str() {
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" => has_image = true,
                "mp3" | "wav" | "flac" | "ogg" => has_audio = true,
                "rs" | "c" | "cpp" | "h" | "py" | "js" | "ts" | "java" | "go" | "rb" | "html"
                | "css" | "toml" | "yaml" | "json" | "xml" => has_code = true,
                "zip" | "tar" | "gz" | "7z" | "rar" => has_archive = true,
                _ => {}
            }
        }

        if has_image {
            detected.push(ColumnId::DIMENSIONS);
        }
        if has_audio {
            detected.push(ColumnId::DURATION);
            detected.push(ColumnId::ARTIST);
        }
        if has_code {
            detected.push(ColumnId::LINE_COUNT);
            detected.push(ColumnId::LANGUAGE);
        }
        if has_archive {
            detected.push(ColumnId::COMPRESSED_SIZE);
            detected.push(ColumnId::FILE_COUNT_INSIDE);
        }

        self.active_columns = detected;
    }

    // ------------------------------------------------------------------
    // Column chooser data
    // ------------------------------------------------------------------

    /// Columns grouped by category, for the chooser UI.
    pub fn columns_by_category(&self) -> HashMap<ColumnCategory, Vec<&ColumnDef>> {
        let mut map: HashMap<ColumnCategory, Vec<&ColumnDef>> = HashMap::new();
        for def in self.all_columns.values() {
            map.entry(def.category).or_default().push(def);
        }
        // Sort each category's columns by id for stable ordering.
        for defs in map.values_mut() {
            defs.sort_by_key(|d| d.id);
        }
        map
    }
}

impl Default for ColumnManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Built-in providers
// ============================================================================

// ---------------------------------------------------------------------------
// Standard columns (always available)
// ---------------------------------------------------------------------------

/// Provider for the default Name/Size/Date/Type/Attributes columns.
pub struct StandardColumns;

impl StandardColumns {
    const DEFS: &'static [ColumnDef] = &[
        ColumnDef {
            id: ColumnId::NAME,
            label: String::new(), // replaced at runtime
            width: ColumnWidth::Flexible {
                min: 120.0,
                max: 500.0,
            },
            alignment: Alignment::Left,
            sortable: true,
            sort_order: SortOrder::None,
            visible: true,
            category: ColumnCategory::Standard,
        },
        ColumnDef {
            id: ColumnId::SIZE,
            label: String::new(),
            width: ColumnWidth::Fixed(90.0),
            alignment: Alignment::Right,
            sortable: true,
            sort_order: SortOrder::None,
            visible: true,
            category: ColumnCategory::Standard,
        },
        ColumnDef {
            id: ColumnId::DATE_MODIFIED,
            label: String::new(),
            width: ColumnWidth::Fixed(140.0),
            alignment: Alignment::Left,
            sortable: true,
            sort_order: SortOrder::None,
            visible: true,
            category: ColumnCategory::Standard,
        },
        ColumnDef {
            id: ColumnId::TYPE,
            label: String::new(),
            width: ColumnWidth::Fixed(80.0),
            alignment: Alignment::Left,
            sortable: true,
            sort_order: SortOrder::None,
            visible: true,
            category: ColumnCategory::Standard,
        },
        ColumnDef {
            id: ColumnId::DATE_CREATED,
            label: String::new(),
            width: ColumnWidth::Fixed(140.0),
            alignment: Alignment::Left,
            sortable: true,
            sort_order: SortOrder::None,
            visible: false,
            category: ColumnCategory::Standard,
        },
        ColumnDef {
            id: ColumnId::ATTRIBUTES,
            label: String::new(),
            width: ColumnWidth::Fixed(80.0),
            alignment: Alignment::Left,
            sortable: false,
            sort_order: SortOrder::None,
            visible: false,
            category: ColumnCategory::Standard,
        },
    ];

    /// Build the runtime column defs with labels filled in.
    fn make_defs() -> Vec<ColumnDef> {
        let labels = [
            "Name",
            "Size",
            "Date Modified",
            "Type",
            "Date Created",
            "Attributes",
        ];
        Self::DEFS
            .iter()
            .zip(labels.iter())
            .map(|(def, label)| {
                let mut d = def.clone();
                d.label = (*label).to_string();
                d
            })
            .collect()
    }
}

impl ColumnProvider for StandardColumns {
    fn columns(&self) -> &[ColumnDef] {
        // Leak a Vec once so we can return a &[ColumnDef].
        // In a real allocator-aware build this would use a static OnceLock.
        static COLS: std::sync::OnceLock<Vec<ColumnDef>> = std::sync::OnceLock::new();
        COLS.get_or_init(StandardColumns::make_defs)
    }

    fn value(&self, path: &str, column_id: ColumnId) -> ColumnValue {
        match column_id {
            ColumnId::NAME => {
                let name = path.rsplit('/').next().unwrap_or(path);
                ColumnValue::Text(name.to_string())
            }
            ColumnId::SIZE => {
                // In a real implementation this would stat the file.
                // Return empty as a stub; the explorer already has size info.
                ColumnValue::Empty
            }
            ColumnId::DATE_MODIFIED | ColumnId::DATE_CREATED => ColumnValue::Empty,
            ColumnId::TYPE => {
                let ext = path_extension(path);
                if ext.is_empty() {
                    ColumnValue::Text("File".to_string())
                } else {
                    ColumnValue::Text(format!("{} File", ext.to_uppercase()))
                }
            }
            ColumnId::ATTRIBUTES => ColumnValue::Text(String::new()),
            _ => ColumnValue::Empty,
        }
    }

    fn supported_extensions(&self) -> &[&str] {
        // Applies to all files.
        &[]
    }
}

// ---------------------------------------------------------------------------
// Image columns
// ---------------------------------------------------------------------------

/// Provider for image-specific columns (dimensions, color depth, aspect ratio).
pub struct ImageColumns;

impl ImageColumns {
    fn make_defs() -> Vec<ColumnDef> {
        vec![
            ColumnDef {
                id: ColumnId::DIMENSIONS,
                label: "Dimensions".to_string(),
                width: ColumnWidth::Fixed(110.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Image,
            },
            ColumnDef {
                id: ColumnId::COLOR_DEPTH,
                label: "Color Depth".to_string(),
                width: ColumnWidth::Fixed(80.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Image,
            },
            ColumnDef {
                id: ColumnId::ASPECT_RATIO,
                label: "Aspect Ratio".to_string(),
                width: ColumnWidth::Fixed(90.0),
                alignment: Alignment::Right,
                sortable: false,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Image,
            },
        ]
    }
}

impl ColumnProvider for ImageColumns {
    fn columns(&self) -> &[ColumnDef] {
        static COLS: std::sync::OnceLock<Vec<ColumnDef>> = std::sync::OnceLock::new();
        COLS.get_or_init(ImageColumns::make_defs)
    }

    fn value(&self, path: &str, column_id: ColumnId) -> ColumnValue {
        // Stub: in a real implementation, read image headers for metadata.
        let _ = path;
        match column_id {
            ColumnId::DIMENSIONS => {
                // Placeholder — would parse image header.
                ColumnValue::Text("1920 \u{00d7} 1080".to_string())
            }
            ColumnId::COLOR_DEPTH => ColumnValue::Text("24-bit".to_string()),
            ColumnId::ASPECT_RATIO => ColumnValue::Text("16:9".to_string()),
            _ => ColumnValue::Empty,
        }
    }

    fn supported_extensions(&self) -> &[&str] {
        &["png", "jpg", "jpeg", "gif", "bmp", "svg"]
    }
}

// ---------------------------------------------------------------------------
// Audio columns
// ---------------------------------------------------------------------------

/// Provider for audio-specific columns (duration, bitrate, sample rate,
/// artist, album, title).
pub struct AudioColumns;

impl AudioColumns {
    fn make_defs() -> Vec<ColumnDef> {
        vec![
            ColumnDef {
                id: ColumnId::DURATION,
                label: "Duration".to_string(),
                width: ColumnWidth::Fixed(70.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Audio,
            },
            ColumnDef {
                id: ColumnId::BITRATE,
                label: "Bitrate".to_string(),
                width: ColumnWidth::Fixed(80.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Audio,
            },
            ColumnDef {
                id: ColumnId::SAMPLE_RATE,
                label: "Sample Rate".to_string(),
                width: ColumnWidth::Fixed(90.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Audio,
            },
            ColumnDef {
                id: ColumnId::ARTIST,
                label: "Artist".to_string(),
                width: ColumnWidth::Flexible {
                    min: 80.0,
                    max: 200.0,
                },
                alignment: Alignment::Left,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Audio,
            },
            ColumnDef {
                id: ColumnId::ALBUM,
                label: "Album".to_string(),
                width: ColumnWidth::Flexible {
                    min: 80.0,
                    max: 200.0,
                },
                alignment: Alignment::Left,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Audio,
            },
            ColumnDef {
                id: ColumnId::TITLE,
                label: "Title".to_string(),
                width: ColumnWidth::Flexible {
                    min: 80.0,
                    max: 250.0,
                },
                alignment: Alignment::Left,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Audio,
            },
        ]
    }
}

impl ColumnProvider for AudioColumns {
    fn columns(&self) -> &[ColumnDef] {
        static COLS: std::sync::OnceLock<Vec<ColumnDef>> = std::sync::OnceLock::new();
        COLS.get_or_init(AudioColumns::make_defs)
    }

    fn value(&self, path: &str, column_id: ColumnId) -> ColumnValue {
        // Stub: would read ID3/Vorbis/FLAC tags.
        let _ = path;
        match column_id {
            ColumnId::DURATION => ColumnValue::Duration(222), // 3:42
            ColumnId::BITRATE => ColumnValue::Text("320 kbps".to_string()),
            ColumnId::SAMPLE_RATE => ColumnValue::Text("44.1 kHz".to_string()),
            ColumnId::ARTIST => ColumnValue::Text("Unknown Artist".to_string()),
            ColumnId::ALBUM => ColumnValue::Text("Unknown Album".to_string()),
            ColumnId::TITLE => {
                // Derive title from filename as a fallback.
                let name = path.rsplit('/').next().unwrap_or(path);
                let title = name.rsplit('.').next_back().unwrap_or(name);
                ColumnValue::Text(title.to_string())
            }
            _ => ColumnValue::Empty,
        }
    }

    fn supported_extensions(&self) -> &[&str] {
        &["mp3", "wav", "flac", "ogg"]
    }
}

// ---------------------------------------------------------------------------
// Code columns
// ---------------------------------------------------------------------------

/// Provider for source-code columns (line count, language).
pub struct CodeColumns;

impl CodeColumns {
    fn make_defs() -> Vec<ColumnDef> {
        vec![
            ColumnDef {
                id: ColumnId::LINE_COUNT,
                label: "Lines".to_string(),
                width: ColumnWidth::Fixed(70.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Code,
            },
            ColumnDef {
                id: ColumnId::LANGUAGE,
                label: "Language".to_string(),
                width: ColumnWidth::Fixed(90.0),
                alignment: Alignment::Left,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Code,
            },
        ]
    }

    /// Map extension to language name.
    fn language_for_ext(ext: &str) -> &'static str {
        match ext {
            "rs" => "Rust",
            "c" | "h" => "C",
            "cpp" | "cc" | "cxx" | "hpp" => "C++",
            "py" => "Python",
            "js" => "JavaScript",
            "ts" => "TypeScript",
            "java" => "Java",
            "go" => "Go",
            "rb" => "Ruby",
            "html" | "htm" => "HTML",
            "css" => "CSS",
            "toml" => "TOML",
            "yaml" | "yml" => "YAML",
            "json" => "JSON",
            "xml" => "XML",
            "sh" | "bash" => "Shell",
            _ => "Unknown",
        }
    }
}

impl ColumnProvider for CodeColumns {
    fn columns(&self) -> &[ColumnDef] {
        static COLS: std::sync::OnceLock<Vec<ColumnDef>> = std::sync::OnceLock::new();
        COLS.get_or_init(CodeColumns::make_defs)
    }

    fn value(&self, path: &str, column_id: ColumnId) -> ColumnValue {
        let ext = path_extension(path);
        match column_id {
            ColumnId::LINE_COUNT => {
                // Stub: would read and count newlines. Return placeholder.
                ColumnValue::Number(0)
            }
            ColumnId::LANGUAGE => ColumnValue::Text(Self::language_for_ext(&ext).to_string()),
            _ => ColumnValue::Empty,
        }
    }

    fn supported_extensions(&self) -> &[&str] {
        &[
            "rs", "c", "h", "cpp", "cc", "cxx", "hpp", "py", "js", "ts", "java", "go", "rb",
            "html", "htm", "css", "toml", "yaml", "yml", "json", "xml", "sh", "bash",
        ]
    }
}

// ---------------------------------------------------------------------------
// Archive columns
// ---------------------------------------------------------------------------

/// Provider for archive-specific columns (compressed size, ratio, file count).
pub struct ArchiveColumns;

impl ArchiveColumns {
    fn make_defs() -> Vec<ColumnDef> {
        vec![
            ColumnDef {
                id: ColumnId::COMPRESSED_SIZE,
                label: "Compressed".to_string(),
                width: ColumnWidth::Fixed(90.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Archive,
            },
            ColumnDef {
                id: ColumnId::COMPRESSION_RATIO,
                label: "Ratio".to_string(),
                width: ColumnWidth::Fixed(60.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Archive,
            },
            ColumnDef {
                id: ColumnId::FILE_COUNT_INSIDE,
                label: "Files Inside".to_string(),
                width: ColumnWidth::Fixed(80.0),
                alignment: Alignment::Right,
                sortable: true,
                sort_order: SortOrder::None,
                visible: false,
                category: ColumnCategory::Archive,
            },
        ]
    }
}

impl ColumnProvider for ArchiveColumns {
    fn columns(&self) -> &[ColumnDef] {
        static COLS: std::sync::OnceLock<Vec<ColumnDef>> = std::sync::OnceLock::new();
        COLS.get_or_init(ArchiveColumns::make_defs)
    }

    fn value(&self, path: &str, column_id: ColumnId) -> ColumnValue {
        // Stub: would parse archive headers for real metadata.
        let _ = path;
        match column_id {
            ColumnId::COMPRESSED_SIZE => ColumnValue::Size(0),
            ColumnId::COMPRESSION_RATIO => ColumnValue::Percentage(0.0),
            ColumnId::FILE_COUNT_INSIDE => ColumnValue::Number(0),
            _ => ColumnValue::Empty,
        }
    }

    fn supported_extensions(&self) -> &[&str] {
        &["zip", "tar", "gz", "7z", "rar"]
    }
}

// ============================================================================
// Rendering helpers
// ============================================================================

/// Colours used by column rendering.
struct ColumnColors;

impl ColumnColors {
    const HEADER_BG: Color = Color::rgba(224, 224, 224, 255);
    const HEADER_TEXT: Color = Color::rgba(51, 51, 51, 255);
    const SORT_ARROW: Color = Color::rgba(100, 100, 100, 255);
    const CELL_TEXT: Color = Color::rgba(0, 0, 0, 255);
    const CELL_DIM: Color = Color::rgba(128, 128, 128, 255);
    const SEPARATOR: Color = Color::rgba(200, 200, 200, 255);
    const CHOOSER_BG: Color = Color::rgba(255, 255, 255, 255);
    const CHOOSER_BORDER: Color = Color::rgba(180, 180, 180, 255);
    const CHOOSER_HOVER: Color = Color::rgba(230, 240, 255, 255);
    const CHECK_ON: Color = Color::rgba(0, 120, 212, 255);
    const CHECK_OFF: Color = Color::rgba(180, 180, 180, 255);
}

const HEADER_HEIGHT: f32 = 22.0;
const ROW_HEIGHT: f32 = 22.0;
const HEADER_FONT_SIZE: f32 = 11.0;
const CELL_FONT_SIZE: f32 = 11.0;
const CHOOSER_ROW_HEIGHT: f32 = 24.0;
const CHOOSER_FONT_SIZE: f32 = 12.0;
const CHOOSER_PAD: f32 = 4.0;

/// Render the column header row.
///
/// Returns render commands for a header bar at `y=0` across `total_width`,
/// with labels, sort arrows, and column separators.
pub fn render_column_header(manager: &ColumnManager, total_width: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Background bar.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: total_width,
        height: HEADER_HEIGHT,
        color: ColumnColors::HEADER_BG,
        corner_radii: CornerRadii::ZERO,
    });

    let active = manager.active_columns();
    let widths = resolve_widths(manager, total_width);
    let mut x = 0.0_f32;

    for (i, &col_id) in active.iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(80.0);
        let def = match manager.column_def(col_id) {
            Some(d) => d,
            None => {
                x += w;
                continue;
            }
        };

        // Label text.
        let text_x = match def.alignment {
            Alignment::Left => x + 4.0,
            Alignment::Right => text::right_x(
                &def.label,
                x + w - 4.0,
                HEADER_FONT_SIZE,
                FontWeightHint::Bold,
            ),
            Alignment::Center => text::center_x(
                &def.label,
                x + w / 2.0,
                HEADER_FONT_SIZE,
                FontWeightHint::Bold,
            ),
        };
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: 4.0,
            text: def.label.clone(),
            color: ColumnColors::HEADER_TEXT,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 8.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Sort arrow (if sorted by this column).
        if def.sort_order != SortOrder::None {
            let arrow = match def.sort_order {
                SortOrder::Ascending => "\u{25B2}",  // up triangle
                SortOrder::Descending => "\u{25BC}", // down triangle
                SortOrder::None => "",
            };
            if !arrow.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + w - 14.0,
                    y: 5.0,
                    text: arrow.to_string(),
                    color: ColumnColors::SORT_ARROW,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // Column separator line.
        if i + 1 < active.len() {
            cmds.push(RenderCommand::Line {
                x1: x + w,
                y1: 2.0,
                x2: x + w,
                y2: HEADER_HEIGHT - 2.0,
                color: ColumnColors::SEPARATOR,
                width: 1.0,
            });
        }

        x += w;
    }

    cmds
}

/// Render one row of column values for a file.
///
/// `y` is the top of the row.  Returns render commands for each cell.
pub fn render_column_values(
    manager: &ColumnManager,
    path: &str,
    y: f32,
    total_width: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    let active = manager.active_columns();
    let widths = resolve_widths(manager, total_width);
    let mut x = 0.0_f32;

    for (i, &col_id) in active.iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(80.0);
        let def = match manager.column_def(col_id) {
            Some(d) => d,
            None => {
                x += w;
                continue;
            }
        };

        let value = manager.get_value(path, col_id);
        let text = value.display();

        if !text.is_empty() {
            let color = match value {
                ColumnValue::Empty => ColumnColors::CELL_DIM,
                ColumnValue::Size(_) | ColumnValue::Number(_) | ColumnValue::Percentage(_) => {
                    ColumnColors::CELL_DIM
                }
                _ => ColumnColors::CELL_TEXT,
            };

            let text_x = match def.alignment {
                Alignment::Left => x + 4.0,
                Alignment::Right => guitk::text::right_x(
                    &text,
                    x + w - 4.0,
                    CELL_FONT_SIZE,
                    FontWeightHint::Regular,
                ),
                Alignment::Center => guitk::text::center_x(
                    &text,
                    x + w / 2.0,
                    CELL_FONT_SIZE,
                    FontWeightHint::Regular,
                ),
            };

            cmds.push(RenderCommand::Text {
                x: text_x,
                y: y + 4.0,
                text,
                color,
                font_size: CELL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        x += w;
    }

    cmds
}

/// Render a column chooser dropdown menu.
///
/// Shows all known columns with checkboxes indicating visibility.
/// `x`, `y` is the top-left corner of the dropdown.
pub fn render_column_chooser(manager: &ColumnManager, x: f32, y: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    let all_defs = {
        let mut defs: Vec<&ColumnDef> = manager.all_column_defs();
        defs.sort_by_key(|d| d.id);
        defs
    };

    let row_count = all_defs.len();
    let menu_w = 200.0_f32;
    let menu_h = row_count as f32 * CHOOSER_ROW_HEIGHT + CHOOSER_PAD * 2.0;

    // Background + border.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width: menu_w,
        height: menu_h,
        color: ColumnColors::CHOOSER_BG,
        corner_radii: CornerRadii::all(4.0),
    });
    cmds.push(RenderCommand::StrokeRect {
        x,
        y,
        width: menu_w,
        height: menu_h,
        color: ColumnColors::CHOOSER_BORDER,
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });

    // Shadow.
    cmds.push(RenderCommand::BoxShadow {
        x,
        y,
        width: menu_w,
        height: menu_h,
        offset_x: 0.0,
        offset_y: 2.0,
        blur: 6.0,
        spread: 0.0,
        color: Color::rgba(0, 0, 0, 40),
        corner_radii: CornerRadii::all(4.0),
    });

    let mut row_y = y + CHOOSER_PAD;
    for def in &all_defs {
        let is_active = manager.is_visible(def.id);

        // Checkbox.
        let cb_x = x + 8.0;
        let cb_y = row_y + 4.0;
        let cb_size = 14.0;
        cmds.push(RenderCommand::StrokeRect {
            x: cb_x,
            y: cb_y,
            width: cb_size,
            height: cb_size,
            color: if is_active {
                ColumnColors::CHECK_ON
            } else {
                ColumnColors::CHECK_OFF
            },
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });
        if is_active {
            // Fill checkbox.
            cmds.push(RenderCommand::FillRect {
                x: cb_x + 2.0,
                y: cb_y + 2.0,
                width: cb_size - 4.0,
                height: cb_size - 4.0,
                color: ColumnColors::CHECK_ON,
                corner_radii: CornerRadii::all(1.0),
            });
        }

        // Label.
        cmds.push(RenderCommand::Text {
            x: cb_x + cb_size + 8.0,
            y: row_y + 5.0,
            text: def.label.clone(),
            color: ColumnColors::HEADER_TEXT,
            font_size: CHOOSER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(menu_w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Category badge (dim, right-aligned).
        let cat_text = def.category.label();
        cmds.push(RenderCommand::Text {
            x: text::right_x(cat_text, x + menu_w - 8.0, 9.0, FontWeightHint::Regular),
            y: row_y + 7.0,
            text: cat_text.to_string(),
            color: ColumnColors::CELL_DIM,
            font_size: 9.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        row_y += CHOOSER_ROW_HEIGHT;
    }

    cmds
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Resolve widths for active columns, distributing remaining space
/// to flexible columns.
fn resolve_widths(manager: &ColumnManager, total_width: f32) -> Vec<f32> {
    let active = manager.active_columns();
    let mut widths = Vec::with_capacity(active.len());
    let mut fixed_total = 0.0_f32;
    let mut flex_count = 0_u32;

    for &col_id in active {
        if let Some(def) = manager.column_def(col_id) {
            match def.width {
                ColumnWidth::Fixed(px) => {
                    widths.push(px);
                    fixed_total += px;
                }
                ColumnWidth::Flexible { .. } => {
                    widths.push(0.0); // placeholder
                    flex_count += 1;
                }
            }
        } else {
            widths.push(80.0);
            fixed_total += 80.0;
        }
    }

    // Distribute remaining space among flexible columns.
    if flex_count > 0 {
        let remaining = (total_width - fixed_total).max(0.0);
        let per_flex = remaining / flex_count as f32;

        for (i, &col_id) in active.iter().enumerate() {
            if let Some(def) = manager.column_def(col_id)
                && let ColumnWidth::Flexible { min, max } = def.width
                && let Some(w) = widths.get_mut(i)
            {
                *w = per_flex.clamp(min, max);
            }
        }
    }

    widths
}

/// Extract the lowercase file extension from a path string.
fn path_extension(path: &str) -> String {
    // Only treat text after the last '.' as an extension if the path
    // actually contains a dot and the dot is not the first character.
    match path.rfind('.') {
        Some(pos) if pos > 0 && pos + 1 < path.len() => path[pos + 1..].to_lowercase(),
        _ => String::new(),
    }
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format a byte count as a human-readable size string.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Format an integer with thousand separators.
fn format_number(n: i64) -> String {
    if n.abs() < 1000 {
        return n.to_string();
    }
    let s = n.abs().to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    if n < 0 {
        result.push('-');
    }
    result.chars().rev().collect()
}

/// Format a Unix-epoch timestamp as "YYYY-MM-DD HH:MM", in UTC.
///
/// This is the Date Modified column, so it is the number the user compares
/// against `ls -l` and against the same file's date in the file dialog. All
/// three now derive it from [`guitk::date`], which derives it from `tzrules`
/// — the same era arithmetic the libc's `localtime` and the taskbar clock use.
///
/// This function used to carry its own calendar, and that calendar was wrong
/// for every file older than 2000-03-01. It shifted the epoch to 2000-03-01 to
/// "simplify leap-year handling" and then, for anything before that, returned a
/// **fabricated** date: `1970 + days / 365`, month `day_of_year / 30 + 1`,
/// clamped to at most the 12th month and the 28th day. A file last touched on
/// 1985-07-04 was listed as 1985-07-09; one from 1999-06-15 as 1999-06-23; and
/// 2000-02-29, a real leap day, as 2000-03-07. The error grew with the file's
/// age, and no clamp could have caught it, because every value it produced was
/// in range — just not the right one. Old files are exactly the ones a user
/// sorts by date to find.
fn format_datetime(epoch_secs: u64) -> String {
    let days = epoch_secs / 86400;
    let time_of_day = epoch_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;

    // A timestamp comes off the filesystem, so it is bounded by nothing this
    // program controls. Saturating lands in the year 5 881 580 rather than
    // wrapping into the past, which is the failure a reader would notice: a
    // file dated before the epoch sorts to the top of a list ordered by date.
    let (year, month, day) =
        date::Date::from_days_since_epoch(i32::try_from(days).unwrap_or(i32::MAX)).ymd();

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
}

/// Format seconds as "m:ss" or "h:mm:ss".
fn format_duration(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Format a fraction (0.0 .. 1.0) as a percentage string.
fn format_percentage(frac: f32) -> String {
    let pct = (frac * 100.0).round() as i32;
    format!("{pct}%")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    //
    // `float_cmp` is allowed for the same reason plus one of its own: the
    // widths under test are passed through `resolve` either unchanged or
    // clamped to one of the bounds, so the assertions compare a float to the
    // very bit pattern it was built from. An epsilon there would weaken the
    // test — it would start passing if `resolve` returned a *nearly* right
    // width, which is exactly the bug it is meant to catch.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::*;

    // ------------------------------------------------------------------
    // Measured alignment
    // ------------------------------------------------------------------

    #[test]
    fn a_right_aligned_header_ends_at_the_column_edge() {
        let mgr = ColumnManager::with_defaults();
        let total = 800.0;
        let widths = resolve_widths(&mgr, total);
        let cmds = render_column_header(&mgr, total);

        let mut x = 0.0_f32;
        let mut checked = 0;
        for (i, &id) in mgr.active_columns().iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(80.0);
            if let Some(def) = mgr.column_def(id)
                && def.alignment == Alignment::Right
            {
                let drawn = cmds
                    .iter()
                    .find_map(|c| match c {
                        RenderCommand::Text { x, text, .. } if *text == def.label => Some(*x),
                        _ => None,
                    })
                    .expect("header label not drawn");
                let end = drawn + text::measure(&def.label, HEADER_FONT_SIZE, FontWeightHint::Bold);
                assert!(
                    (end - (x + w - 4.0)).abs() < 0.01,
                    "{} ends at {end}, column edge is {}",
                    def.label,
                    x + w - 4.0
                );
                checked += 1;
            }
            x += w;
        }
        assert!(checked > 0, "no right-aligned column to check");
    }

    #[test]
    fn alignment_is_not_computed_from_byte_length() {
        // A label with a two-byte character in it must not be treated as one
        // glyph wider than the same label spelled in ASCII.
        let bytes = "Gr\u{f6}\u{df}e";
        let ascii = "Grosse";
        let a = text::measure(bytes, HEADER_FONT_SIZE, FontWeightHint::Bold);
        let b = text::measure(ascii, HEADER_FONT_SIZE, FontWeightHint::Bold);
        assert!(
            (a - b).abs() < HEADER_FONT_SIZE,
            "measurement differed by more than a glyph: {a} vs {b}"
        );
    }

    // ------------------------------------------------------------------
    // Formatting
    // ------------------------------------------------------------------

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 100), "100.0 KB");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 + 512 * 1024), "1.5 MB");
    }

    #[test]
    fn test_format_size_gigabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.00 GB");
    }

    #[test]
    fn test_format_duration_seconds_only() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(5), "0:05");
        assert_eq!(format_duration(59), "0:59");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1:00");
        assert_eq!(format_duration(222), "3:42");
        assert_eq!(format_duration(599), "9:59");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1:00:00");
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(7384), "2:03:04");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(0.0), "0%");
        assert_eq!(format_percentage(0.5), "50%");
        assert_eq!(format_percentage(0.85), "85%");
        assert_eq!(format_percentage(1.0), "100%");
    }

    #[test]
    fn test_format_percentage_rounding() {
        assert_eq!(format_percentage(0.333), "33%");
        assert_eq!(format_percentage(0.667), "67%");
    }

    #[test]
    fn test_format_number_small() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(42), "42");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(-5), "-5");
    }

    #[test]
    fn test_format_number_thousands() {
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1_234_567), "1,234,567");
        assert_eq!(format_number(-10_000), "-10,000");
    }

    #[test]
    fn test_format_datetime() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let s = format_datetime(1_704_067_200);
        assert!(s.starts_with("2024-01-01"), "got: {s}");
    }

    /// The old calendar was right after 2000-03-01 and fabricated a date
    /// before it, so a test that only sampled a recent timestamp — which is
    /// what the one above did, alone, for the whole life of the bug — could
    /// not have caught it. These are the dates it got wrong, and the comment
    /// records what it used to say so a regression is recognisable on sight.
    #[test]
    fn dates_before_2000_03_01_are_the_real_ones_not_an_estimate() {
        // Was 1970-01-01 — the one pre-2000 date the estimate got right.
        assert_eq!(format_datetime(0), "1970-01-01 00:00");
        // Was 1985-07-09.
        assert_eq!(format_datetime(489_328_200), "1985-07-04 12:30");
        // Was 1999-06-23.
        assert_eq!(format_datetime(929_433_900), "1999-06-15 08:05");
        // Was 2000-03-07 — a real leap day, reported a week late.
        assert_eq!(format_datetime(951_868_740), "2000-02-29 23:59");
        // The old seam itself, which was already correct and stays so.
        assert_eq!(format_datetime(951_868_800), "2000-03-01 00:00");
    }

    /// A timestamp far past what a `Date` can hold saturates into the distant
    /// future rather than wrapping into the past. `u64::MAX` seconds is not a
    /// date anyone means, but it is a date a corrupt inode can produce, and
    /// the answer must not sort *before* every real file.
    #[test]
    fn an_impossible_timestamp_saturates_forward() {
        let s = format_datetime(u64::MAX);
        let year: i64 = s
            .split('-')
            .next()
            .and_then(|y| y.parse().ok())
            .expect("the format starts with a year");
        assert!(year > 2024, "got: {s}");
    }

    // ------------------------------------------------------------------
    // ColumnValue display
    // ------------------------------------------------------------------

    #[test]
    fn test_column_value_display() {
        assert_eq!(ColumnValue::Text("hello".into()).display(), "hello");
        assert_eq!(ColumnValue::Number(42).display(), "42");
        assert_eq!(ColumnValue::Size(1024).display(), "1.0 KB");
        assert_eq!(ColumnValue::Duration(222).display(), "3:42");
        assert_eq!(ColumnValue::Percentage(0.85).display(), "85%");
        assert_eq!(ColumnValue::Empty.display(), "");
    }

    // ------------------------------------------------------------------
    // ColumnValue ordering
    //
    // Each of the first three tests fails against the packed-`i128`
    // `sort_key` this ordering replaced; see the comment above `impl Ord`.
    // ------------------------------------------------------------------

    fn text(s: &str) -> ColumnValue {
        ColumnValue::Text(s.to_string())
    }

    #[test]
    fn a_non_ascii_name_does_not_sort_ahead_of_every_ascii_one() {
        // The old key shifted the leading byte into the i128 sign bit, so any
        // name starting >= 0x80 — every non-ASCII name — went negative and
        // sorted ahead of every ASCII name, including ones that begin with
        // `a`. A folder of accented or CJK names became a block at the top,
        // ordered by nothing the user could see.
        //
        // What this asserts is only that the block is gone: ordering is by
        // Unicode scalar value after case folding, so `éclair` lands after
        // `zulu` rather than before `alpha`. That is not where a French
        // speaker expects it — proper placement needs the Unicode Collation
        // Algorithm's tables, which this tree does not have. Tracked in
        // `known-issues.md` as `TD-EXPLORER-SORT-IS-CODEPOINT-NOT-COLLATION`.
        assert_eq!(text("\u{e9}clair").cmp(&text("alpha")), Ordering::Greater);
        assert_eq!(text("\u{65e5}").cmp(&text("zulu")), Ordering::Greater);

        let mut names = [text("zulu"), text("\u{e9}clair"), text("alpha")];
        names.sort();
        let order: Vec<String> = names.iter().map(ColumnValue::display).collect();
        assert_eq!(order, ["alpha", "zulu", "\u{e9}clair"]);
    }

    #[test]
    fn names_are_distinguished_past_the_sixteenth_byte() {
        // The old key packed sixteen bytes and stopped. These two differ at
        // byte 20, so it called them Equal while `==` called them different —
        // an `Ord` contract violation, which lets `sort_by` do anything.
        let a = text("aaaaaaaaaaaaaaaaaaaa1");
        let b = text("aaaaaaaaaaaaaaaaaaaa2");
        assert_ne!(a, b);
        assert_eq!(a.cmp(&b), Ordering::Less);
    }

    #[test]
    fn ordering_and_equality_agree_over_a_mixed_sample() {
        // `Ord`'s contract: `a.cmp(b) == Equal` exactly when `a == b`.
        let sample = [
            text(""),
            text("a"),
            text("A"),
            text("readme"),
            text("README"),
            text("\u{130}"),
            text("i\u{307}"),
            ColumnValue::Number(-1),
            ColumnValue::Number(0),
            ColumnValue::Size(0),
            ColumnValue::DateTime(0),
            ColumnValue::Duration(0),
            ColumnValue::Percentage(0.5),
            ColumnValue::Percentage(f32::NAN),
            ColumnValue::Empty,
        ];
        for x in &sample {
            for y in &sample {
                assert_eq!(
                    x.cmp(y) == Ordering::Equal,
                    x == y,
                    "cmp/eq disagree: {x:?} vs {y:?}"
                );
                assert_eq!(x.cmp(y), y.cmp(x).reverse(), "antisymmetry: {x:?} {y:?}");
            }
        }
    }

    #[test]
    fn case_only_differences_sort_together_but_stay_distinct() {
        // Case-insensitive is the primary order, so `README` lands next to
        // `readme` rather than ahead of every lowercase name — but the two are
        // still different values, so the tie-break must order them.
        let mut names = [text("readme"), text("apple"), text("README"), text("Zebra")];
        names.sort();
        let order: Vec<String> = names.iter().map(ColumnValue::display).collect();
        assert_eq!(order, ["apple", "README", "readme", "Zebra"]);
    }

    #[test]
    fn a_nan_percentage_has_a_defined_place_rather_than_an_arbitrary_one() {
        // Derived `PartialEq` on f32 makes NaN unequal to itself, which `Eq`
        // forbids and which makes a sort's behaviour undefined. `total_cmp`
        // puts NaN after every real percentage and equal to itself.
        let nan = ColumnValue::Percentage(f32::NAN);
        assert_eq!(nan, nan.clone());
        assert_eq!(nan.cmp(&ColumnValue::Percentage(1.0)), Ordering::Greater);
    }

    #[test]
    fn empty_cells_collect_at_the_bottom() {
        let mut vals = [
            ColumnValue::Empty,
            ColumnValue::Size(3),
            text("name"),
            ColumnValue::Empty,
        ];
        vals.sort();
        assert_eq!(vals[0], text("name"));
        assert_eq!(vals[1], ColumnValue::Size(3));
        assert_eq!(vals[2], ColumnValue::Empty);
        assert_eq!(vals[3], ColumnValue::Empty);
    }

    // ------------------------------------------------------------------
    // Provider matching by extension
    // ------------------------------------------------------------------

    #[test]
    fn test_image_provider_extensions() {
        let prov = ImageColumns;
        let exts = prov.supported_extensions();
        assert!(exts.contains(&"png"));
        assert!(exts.contains(&"jpg"));
        assert!(exts.contains(&"svg"));
        assert!(!exts.contains(&"mp3"));
    }

    #[test]
    fn test_audio_provider_extensions() {
        let prov = AudioColumns;
        let exts = prov.supported_extensions();
        assert!(exts.contains(&"mp3"));
        assert!(exts.contains(&"flac"));
        assert!(!exts.contains(&"png"));
    }

    #[test]
    fn test_code_provider_extensions() {
        let prov = CodeColumns;
        let exts = prov.supported_extensions();
        assert!(exts.contains(&"rs"));
        assert!(exts.contains(&"py"));
        assert!(exts.contains(&"js"));
        assert!(!exts.contains(&"mp3"));
    }

    #[test]
    fn test_archive_provider_extensions() {
        let prov = ArchiveColumns;
        let exts = prov.supported_extensions();
        assert!(exts.contains(&"zip"));
        assert!(exts.contains(&"tar"));
        assert!(!exts.contains(&"rs"));
    }

    #[test]
    fn test_standard_provider_universal() {
        let prov = StandardColumns;
        assert!(prov.supported_extensions().is_empty());
    }

    // ------------------------------------------------------------------
    // Provider value lookups
    // ------------------------------------------------------------------

    #[test]
    fn test_standard_name_value() {
        let prov = StandardColumns;
        let val = prov.value("/home/user/readme.txt", ColumnId::NAME);
        assert_eq!(val, ColumnValue::Text("readme.txt".to_string()));
    }

    #[test]
    fn test_standard_type_value() {
        let prov = StandardColumns;
        let val = prov.value("/home/user/photo.png", ColumnId::TYPE);
        assert_eq!(val, ColumnValue::Text("PNG File".to_string()));
    }

    #[test]
    fn test_code_language_value() {
        let prov = CodeColumns;
        let val = prov.value("/src/main.rs", ColumnId::LANGUAGE);
        assert_eq!(val, ColumnValue::Text("Rust".to_string()));
    }

    #[test]
    fn test_audio_duration_value() {
        let prov = AudioColumns;
        let val = prov.value("/music/song.mp3", ColumnId::DURATION);
        assert_eq!(val, ColumnValue::Duration(222));
    }

    // ------------------------------------------------------------------
    // Auto-detect columns
    // ------------------------------------------------------------------

    #[test]
    fn test_auto_detect_images() {
        let mut mgr = ColumnManager::with_defaults();
        let files = [
            FileInfo {
                path: "photo.png",
                extension: "png",
            },
            FileInfo {
                path: "readme.txt",
                extension: "txt",
            },
        ];
        mgr.auto_detect_columns(&files);
        assert!(mgr.is_visible(ColumnId::DIMENSIONS));
        assert!(!mgr.is_visible(ColumnId::DURATION));
    }

    #[test]
    fn test_auto_detect_audio() {
        let mut mgr = ColumnManager::with_defaults();
        let files = [FileInfo {
            path: "song.mp3",
            extension: "mp3",
        }];
        mgr.auto_detect_columns(&files);
        assert!(mgr.is_visible(ColumnId::DURATION));
        assert!(mgr.is_visible(ColumnId::ARTIST));
        assert!(!mgr.is_visible(ColumnId::DIMENSIONS));
    }

    #[test]
    fn test_auto_detect_code() {
        let mut mgr = ColumnManager::with_defaults();
        let files = [
            FileInfo {
                path: "main.rs",
                extension: "rs",
            },
            FileInfo {
                path: "lib.py",
                extension: "py",
            },
        ];
        mgr.auto_detect_columns(&files);
        assert!(mgr.is_visible(ColumnId::LINE_COUNT));
        assert!(mgr.is_visible(ColumnId::LANGUAGE));
    }

    #[test]
    fn test_auto_detect_archives() {
        let mut mgr = ColumnManager::with_defaults();
        let files = [FileInfo {
            path: "backup.zip",
            extension: "zip",
        }];
        mgr.auto_detect_columns(&files);
        assert!(mgr.is_visible(ColumnId::COMPRESSED_SIZE));
        assert!(mgr.is_visible(ColumnId::FILE_COUNT_INSIDE));
    }

    #[test]
    fn test_auto_detect_mixed() {
        let mut mgr = ColumnManager::with_defaults();
        let files = [
            FileInfo {
                path: "photo.png",
                extension: "png",
            },
            FileInfo {
                path: "song.mp3",
                extension: "mp3",
            },
            FileInfo {
                path: "main.rs",
                extension: "rs",
            },
            FileInfo {
                path: "backup.zip",
                extension: "zip",
            },
        ];
        mgr.auto_detect_columns(&files);
        assert!(mgr.is_visible(ColumnId::DIMENSIONS));
        assert!(mgr.is_visible(ColumnId::DURATION));
        assert!(mgr.is_visible(ColumnId::LINE_COUNT));
        assert!(mgr.is_visible(ColumnId::COMPRESSED_SIZE));
    }

    #[test]
    fn test_auto_detect_no_special() {
        let mut mgr = ColumnManager::with_defaults();
        let files = [
            FileInfo {
                path: "readme.txt",
                extension: "txt",
            },
            FileInfo {
                path: "notes.doc",
                extension: "doc",
            },
        ];
        mgr.auto_detect_columns(&files);
        // Only standard columns.
        assert!(mgr.is_visible(ColumnId::NAME));
        assert!(mgr.is_visible(ColumnId::SIZE));
        assert!(!mgr.is_visible(ColumnId::DIMENSIONS));
        assert!(!mgr.is_visible(ColumnId::DURATION));
    }

    // ------------------------------------------------------------------
    // Sort
    // ------------------------------------------------------------------

    #[test]
    fn test_sort_by_toggles() {
        let mut mgr = ColumnManager::with_defaults();
        mgr.sort_by(ColumnId::NAME);
        let (col, dir) = mgr.current_sort();
        assert_eq!(col, Some(ColumnId::NAME));
        assert_eq!(dir, SortOrder::Ascending);

        // Click again toggles to descending.
        mgr.sort_by(ColumnId::NAME);
        let (_, dir) = mgr.current_sort();
        assert_eq!(dir, SortOrder::Descending);

        // Click again resets to none.
        mgr.sort_by(ColumnId::NAME);
        let (_, dir) = mgr.current_sort();
        assert_eq!(dir, SortOrder::None);
    }

    #[test]
    fn test_sort_by_different_column() {
        let mut mgr = ColumnManager::with_defaults();
        mgr.sort_by(ColumnId::NAME);
        mgr.sort_by(ColumnId::SIZE);
        let (col, dir) = mgr.current_sort();
        assert_eq!(col, Some(ColumnId::SIZE));
        assert_eq!(dir, SortOrder::Ascending);
    }

    #[test]
    fn test_sort_values_ordering() {
        let a = ColumnValue::Number(10);
        let b = ColumnValue::Number(20);
        assert!(a < b);

        let c = ColumnValue::Size(1024);
        let d = ColumnValue::Size(2048);
        assert!(c < d);

        let e = ColumnValue::Duration(60);
        let f = ColumnValue::Duration(120);
        assert!(e < f);
    }

    #[test]
    fn test_sort_empty_last() {
        let val = ColumnValue::Number(1);
        let empty = ColumnValue::Empty;
        assert!(val < empty);
    }

    // ------------------------------------------------------------------
    // Column visibility toggling
    // ------------------------------------------------------------------

    #[test]
    fn test_add_column() {
        let mut mgr = ColumnManager::with_defaults();
        assert!(!mgr.is_visible(ColumnId::DATE_CREATED));
        mgr.add_column(ColumnId::DATE_CREATED);
        assert!(mgr.is_visible(ColumnId::DATE_CREATED));
    }

    #[test]
    fn test_add_column_idempotent() {
        let mut mgr = ColumnManager::with_defaults();
        let before = mgr.active_columns().len();
        mgr.add_column(ColumnId::NAME); // already visible
        assert_eq!(mgr.active_columns().len(), before);
    }

    #[test]
    fn test_remove_column() {
        let mut mgr = ColumnManager::with_defaults();
        assert!(mgr.is_visible(ColumnId::TYPE));
        mgr.remove_column(ColumnId::TYPE);
        assert!(!mgr.is_visible(ColumnId::TYPE));
    }

    #[test]
    fn test_set_columns() {
        let mut mgr = ColumnManager::with_defaults();
        mgr.set_columns(vec![ColumnId::NAME, ColumnId::SIZE]);
        assert_eq!(mgr.active_columns().len(), 2);
        assert!(mgr.is_visible(ColumnId::NAME));
        assert!(mgr.is_visible(ColumnId::SIZE));
        assert!(!mgr.is_visible(ColumnId::TYPE));
    }

    // ------------------------------------------------------------------
    // Reorder
    // ------------------------------------------------------------------

    #[test]
    fn test_reorder_move_forward() {
        let mut mgr = ColumnManager::with_defaults();
        // Default: [NAME, SIZE, DATE_MODIFIED, TYPE]
        mgr.reorder(0, 2);
        let active = mgr.active_columns();
        assert_eq!(active[0], ColumnId::SIZE);
        assert_eq!(active[1], ColumnId::DATE_MODIFIED);
        assert_eq!(active[2], ColumnId::NAME);
    }

    #[test]
    fn test_reorder_move_backward() {
        let mut mgr = ColumnManager::with_defaults();
        mgr.reorder(3, 0);
        let active = mgr.active_columns();
        assert_eq!(active[0], ColumnId::TYPE);
        assert_eq!(active[1], ColumnId::NAME);
    }

    #[test]
    fn test_reorder_same_position() {
        let mut mgr = ColumnManager::with_defaults();
        let before: Vec<ColumnId> = mgr.active_columns().to_vec();
        mgr.reorder(1, 1);
        assert_eq!(mgr.active_columns(), &before);
    }

    #[test]
    fn test_reorder_out_of_bounds() {
        let mut mgr = ColumnManager::with_defaults();
        let len = mgr.active_columns().len();
        // Should clamp, not panic.
        mgr.reorder(100, 0);
        assert_eq!(mgr.active_columns().len(), len);
    }

    #[test]
    fn test_reorder_empty() {
        let mut mgr = ColumnManager::new();
        // No panic on empty.
        mgr.reorder(0, 1);
    }

    // ------------------------------------------------------------------
    // Resize
    // ------------------------------------------------------------------

    #[test]
    fn test_resize_fixed_column() {
        let mut mgr = ColumnManager::with_defaults();
        mgr.resize(ColumnId::SIZE, 150.0);
        let def = mgr.column_def(ColumnId::SIZE).unwrap();
        assert_eq!(def.width, ColumnWidth::Fixed(150.0));
    }

    #[test]
    fn test_resize_enforces_minimum() {
        let mut mgr = ColumnManager::with_defaults();
        mgr.resize(ColumnId::SIZE, 5.0);
        let def = mgr.column_def(ColumnId::SIZE).unwrap();
        // Fixed columns clamp at 20.0.
        assert_eq!(def.width, ColumnWidth::Fixed(20.0));
    }

    #[test]
    fn test_resize_flexible_column_clamps_to_min() {
        let mut mgr = ColumnManager::with_defaults();
        // NAME is Flexible { min: 120, max: 500 }.
        // Resizing below min clamps to min and converts to Fixed.
        mgr.resize(ColumnId::NAME, 50.0);
        let def = mgr.column_def(ColumnId::NAME).unwrap();
        assert_eq!(def.width, ColumnWidth::Fixed(120.0));
    }

    #[test]
    fn test_resize_flexible_column_clamps_to_max() {
        let mut mgr = ColumnManager::with_defaults();
        // NAME is Flexible { min: 120, max: 500 }.
        // Resizing above max clamps to max and converts to Fixed.
        mgr.resize(ColumnId::NAME, 999.0);
        let def = mgr.column_def(ColumnId::NAME).unwrap();
        assert_eq!(def.width, ColumnWidth::Fixed(500.0));
    }

    #[test]
    fn test_resize_flexible_column_within_range() {
        let mut mgr = ColumnManager::with_defaults();
        // NAME is Flexible { min: 120, max: 500 }.
        mgr.resize(ColumnId::NAME, 250.0);
        let def = mgr.column_def(ColumnId::NAME).unwrap();
        assert_eq!(def.width, ColumnWidth::Fixed(250.0));
    }

    // ------------------------------------------------------------------
    // ColumnWidth::resolve
    // ------------------------------------------------------------------

    #[test]
    fn test_column_width_resolve_fixed() {
        assert_eq!(ColumnWidth::Fixed(100.0).resolve(500.0), 100.0);
    }

    #[test]
    fn test_column_width_resolve_flexible() {
        let flex = ColumnWidth::Flexible {
            min: 80.0,
            max: 300.0,
        };
        assert_eq!(flex.resolve(200.0), 200.0);
        assert_eq!(flex.resolve(50.0), 80.0);
        assert_eq!(flex.resolve(500.0), 300.0);
    }

    // ------------------------------------------------------------------
    // Manager value lookup delegation
    // ------------------------------------------------------------------

    #[test]
    fn test_manager_get_value_delegates() {
        let mgr = ColumnManager::with_defaults();
        // Standard provider handles NAME for any file.
        let val = mgr.get_value("/home/user/test.txt", ColumnId::NAME);
        assert_eq!(val, ColumnValue::Text("test.txt".to_string()));
    }

    #[test]
    fn test_manager_get_value_wrong_extension() {
        let mgr = ColumnManager::with_defaults();
        // Image columns should not return values for .txt files.
        let val = mgr.get_value("/home/user/test.txt", ColumnId::DIMENSIONS);
        assert_eq!(val, ColumnValue::Empty);
    }

    #[test]
    fn test_manager_get_value_matching_extension() {
        let mgr = ColumnManager::with_defaults();
        let val = mgr.get_value("/photos/sunset.png", ColumnId::DIMENSIONS);
        // ImageColumns returns the stub "1920 x 1080".
        assert!(matches!(val, ColumnValue::Text(_)));
    }

    // ------------------------------------------------------------------
    // Rendering (smoke tests — verify commands are generated)
    // ------------------------------------------------------------------

    #[test]
    fn test_render_column_header_nonempty() {
        let mgr = ColumnManager::with_defaults();
        let cmds = render_column_header(&mgr, 800.0);
        assert!(!cmds.is_empty(), "header should produce render commands");
    }

    #[test]
    fn test_render_column_values_nonempty() {
        let mgr = ColumnManager::with_defaults();
        let cmds = render_column_values(&mgr, "/test/file.txt", 0.0, 800.0);
        assert!(!cmds.is_empty(), "row should produce render commands");
    }

    #[test]
    fn test_render_column_chooser_nonempty() {
        let mgr = ColumnManager::with_defaults();
        let cmds = render_column_chooser(&mgr, 10.0, 30.0);
        assert!(!cmds.is_empty(), "chooser should produce render commands");
    }

    // ------------------------------------------------------------------
    // path_extension helper
    // ------------------------------------------------------------------

    #[test]
    fn test_path_extension_simple() {
        assert_eq!(path_extension("/foo/bar.txt"), "txt");
        assert_eq!(path_extension("image.PNG"), "png");
    }

    #[test]
    fn test_path_extension_none() {
        assert_eq!(path_extension("/foo/bar"), "");
    }

    #[test]
    fn test_path_extension_dot_only() {
        // "." → last segment after rsplit('.') is empty, which is shorter
        // than the full path, so it returns "".
        // Actually "." rsplit by '.' gives ["", ""], next() = "" which has
        // len 0 < 1, so we return "".
        assert_eq!(path_extension("."), "");
    }

    // ------------------------------------------------------------------
    // columns_by_category
    // ------------------------------------------------------------------

    #[test]
    fn test_columns_by_category() {
        let mgr = ColumnManager::with_defaults();
        let by_cat = mgr.columns_by_category();
        assert!(by_cat.contains_key(&ColumnCategory::Standard));
        assert!(by_cat.contains_key(&ColumnCategory::Image));
        assert!(by_cat.contains_key(&ColumnCategory::Audio));
        assert!(by_cat.contains_key(&ColumnCategory::Code));
        assert!(by_cat.contains_key(&ColumnCategory::Archive));
    }
}
