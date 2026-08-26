//! Slate OS Archive Manager
//!
//! Graphical archive/compressed file manager supporting multiple formats:
//! - ZIP, TAR, TAR.GZ, TAR.BZ2, 7Z
//! - Browse archive contents in a tree view
//! - Extract all, extract selected, extract to folder
//! - Create new archives from file lists
//! - Add/remove files from existing archives
//! - Compression level selection (store/fast/normal/best)
//! - Progress tracking for operations
//! - File list with sortable columns
//! - Drag-and-drop model
//! - Password/encryption for ZIP/7Z
//! - Split archive support
//! - Archive testing/verification
//!
//! Uses the guitk library for UI rendering.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

/// Catppuccin Mocha dark theme colors.
pub mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const MANTLE: Color = Color::from_hex(0x181825);
}

// ============================================================================
// Archive formats
// ============================================================================

/// Supported archive formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    SevenZip,
}

impl ArchiveFormat {
    /// File extension for this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => ".zip",
            Self::Tar => ".tar",
            Self::TarGz => ".tar.gz",
            Self::TarBz2 => ".tar.bz2",
            Self::SevenZip => ".7z",
        }
    }

    /// Display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Zip => "ZIP Archive",
            Self::Tar => "TAR Archive",
            Self::TarGz => "TAR.GZ Archive",
            Self::TarBz2 => "TAR.BZ2 Archive",
            Self::SevenZip => "7-Zip Archive",
        }
    }

    /// Whether this format supports encryption/passwords.
    pub fn supports_encryption(self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip)
    }

    /// Whether this format supports per-file compression.
    pub fn supports_per_file_compression(self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip)
    }

    /// Whether this format supports split archives.
    pub fn supports_split(self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip)
    }

    /// Detect format from file path by examining the extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(Self::TarGz)
        } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
            Some(Self::TarBz2)
        } else if name.ends_with(".tar") {
            Some(Self::Tar)
        } else if name.ends_with(".zip") {
            Some(Self::Zip)
        } else if name.ends_with(".7z") {
            Some(Self::SevenZip)
        } else {
            None
        }
    }

    /// All supported formats.
    pub fn all() -> &'static [Self] {
        &[
            Self::Zip,
            Self::Tar,
            Self::TarGz,
            Self::TarBz2,
            Self::SevenZip,
        ]
    }
}

// ============================================================================
// Compression levels
// ============================================================================

/// Compression level presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CompressionLevel {
    /// No compression, store only.
    Store,
    /// Fast compression with lower ratio.
    Fast,
    /// Balanced compression (default).
    #[default]
    Normal,
    /// Maximum compression, slower.
    Best,
}

impl CompressionLevel {
    /// Display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Store => "Store (no compression)",
            Self::Fast => "Fast",
            Self::Normal => "Normal",
            Self::Best => "Best (slowest)",
        }
    }

    /// Numeric level (0-9 scale used by most compressors).
    pub fn numeric_level(self) -> u8 {
        match self {
            Self::Store => 0,
            Self::Fast => 3,
            Self::Normal => 6,
            Self::Best => 9,
        }
    }

    /// All levels.
    pub fn all() -> &'static [Self] {
        &[Self::Store, Self::Fast, Self::Normal, Self::Best]
    }
}

// ============================================================================
// Encryption settings
// ============================================================================

/// Encryption method for archives that support it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EncryptionMethod {
    /// Standard ZIP encryption (weak, legacy).
    ZipCrypto,
    /// AES-128.
    Aes128,
    /// AES-256.
    Aes256,
}

impl EncryptionMethod {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::ZipCrypto => "ZipCrypto (legacy)",
            Self::Aes128 => "AES-128",
            Self::Aes256 => "AES-256",
        }
    }
}

/// Encryption settings for an archive.
#[derive(Clone, Debug)]
pub struct EncryptionSettings {
    /// The password. Empty means no encryption.
    pub password: String,
    /// Encryption method.
    pub method: EncryptionMethod,
    /// Whether to encrypt file names (7z only).
    pub encrypt_filenames: bool,
}

impl Default for EncryptionSettings {
    fn default() -> Self {
        Self {
            password: String::new(),
            method: EncryptionMethod::Aes256,
            encrypt_filenames: false,
        }
    }
}

impl EncryptionSettings {
    /// Whether encryption is actually enabled (password is non-empty).
    pub fn is_enabled(&self) -> bool {
        !self.password.is_empty()
    }
}

// ============================================================================
// Split archive settings
// ============================================================================

/// Settings for split/multi-volume archives.
#[derive(Clone, Debug)]
pub struct SplitSettings {
    /// Whether splitting is enabled.
    pub enabled: bool,
    /// Volume size in bytes.
    pub volume_size: u64,
}

impl Default for SplitSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            volume_size: 700 * 1024 * 1024, // 700 MiB (CD-ROM)
        }
    }
}

impl SplitSettings {
    /// Common split size presets (label, size in bytes).
    pub fn presets() -> &'static [(&'static str, u64)] {
        &[
            ("1.44 MB (Floppy)", 1_440 * 1024),
            ("100 MB", 100 * 1024 * 1024),
            ("700 MB (CD)", 700 * 1024 * 1024),
            ("4.7 GB (DVD)", 4_700_000_000),
            ("25 GB (Blu-ray)", 25_000_000_000),
            ("Custom", 0),
        ]
    }
}

// ============================================================================
// Archive entry (file/directory inside an archive)
// ============================================================================

/// A single entry inside an archive.
#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    /// Full path within the archive (e.g., "src/main.rs").
    pub path: String,
    /// Display name (last component of path).
    pub name: String,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// Last modification timestamp (seconds since epoch).
    pub modified: u64,
    /// CRC-32 checksum.
    pub crc32: u32,
    /// Whether this entry is encrypted.
    pub encrypted: bool,
    /// Compression method used for this entry.
    pub method: String,
    /// Depth in the directory tree (0 = root level).
    pub depth: u32,
    /// Whether this tree node is expanded in the UI.
    pub expanded: bool,
    /// Whether this entry is selected in the UI.
    pub selected: bool,
    /// Unique id for stable references.
    pub id: u64,
}

impl ArchiveEntry {
    /// Compression ratio as a percentage (0..100).
    /// Returns 0 if uncompressed size is 0.
    pub fn compression_ratio(&self) -> f64 {
        if self.size == 0 {
            return 0.0;
        }
        let ratio = 1.0 - (self.compressed_size as f64 / self.size as f64);
        (ratio * 100.0).clamp(0.0, 100.0)
    }

    /// Format the size for display.
    pub fn format_size(bytes: u64) -> String {
        guitk::bytes::iec(bytes)
    }

    /// Format CRC as a hex string.
    pub fn format_crc(crc: u32) -> String {
        format!("{crc:08X}")
    }

    /// Format a Unix timestamp as a date string.
    ///
    /// `"-"` for zero stays here rather than moving into the shared
    /// formatter: an archive entry with no stored mtime is one whose time is
    /// unknown, which is a different fact from "written at the epoch".
    ///
    /// The rest was the same `secs % 86400` decomposition the file explorer,
    /// the RSS reader, the task scheduler and the undelete tool each wrote
    /// for themselves — the same shape, five times, and only four of them
    /// right. It renders a file's modification time, which is the very object
    /// the explorer's Date column renders, so it must not be a separate
    /// answer.
    ///
    /// UTC, explicitly, because this program has no zone to read: there is no
    /// per-process zone plumbing yet (known-issues
    /// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`). Saying so with `Tz::utc()`
    /// leaves a mark that can be found and fixed when there is one.
    pub fn format_date(timestamp: u64) -> String {
        if timestamp == 0 {
            return String::from("-");
        }
        guitk::datetime::stamp(
            i64::try_from(timestamp).unwrap_or(i64::MAX),
            &guitk::tzrules::Tz::utc(),
        )
    }

    /// Parent directory path, or empty string for root-level entries.
    pub fn parent_path(&self) -> &str {
        if let Some(pos) = self.path.rfind('/') {
            &self.path[..pos]
        } else {
            ""
        }
    }
}

// `days_to_ymd` lived here. It was a correct local transcription of Howard
// Hinnant's `civil_from_days`, kept alive only by `format_date` calling it and
// by two tests calling it directly. `format_date` now renders through
// `guitk::datetime`, which reaches the same algorithm through `tzrules` — the
// one the libc's `localtime` and the taskbar clock also use — so the last
// non-test caller is gone and the function with it. A private calendar with no
// production caller is not a spare; it is a second answer waiting to be picked
// up by the next person who needs a date here.

// ============================================================================
// Column definitions for the file list
// ============================================================================

/// Columns available in the file list view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    Name,
    Size,
    CompressedSize,
    Ratio,
    Date,
    Crc,
    Method,
}

impl Column {
    /// Display header text.
    pub fn header(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Size => "Size",
            Self::CompressedSize => "Packed",
            Self::Ratio => "Ratio",
            Self::Date => "Date",
            Self::Crc => "CRC-32",
            Self::Method => "Method",
        }
    }

    /// Default column width.
    pub fn default_width(self) -> f32 {
        match self {
            Self::Name => 300.0,
            Self::Size => 90.0,
            Self::CompressedSize => 90.0,
            Self::Ratio => 60.0,
            Self::Date => 140.0,
            Self::Crc => 80.0,
            Self::Method => 80.0,
        }
    }

    /// All columns in default display order.
    pub fn all() -> &'static [Self] {
        &[
            Self::Name,
            Self::Size,
            Self::CompressedSize,
            Self::Ratio,
            Self::Date,
            Self::Crc,
            Self::Method,
        ]
    }
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    /// Toggle the direction.
    pub fn toggle(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    /// Sort indicator character.
    pub fn indicator(self) -> &'static str {
        match self {
            Self::Ascending => " ^",
            Self::Descending => " v",
        }
    }
}

/// Sort state: which column and direction.
#[derive(Clone, Copy, Debug)]
pub struct SortState {
    pub column: Column,
    pub direction: SortDirection,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            column: Column::Name,
            direction: SortDirection::Ascending,
        }
    }
}

// ============================================================================
// Tree node for directory structure
// ============================================================================

/// A node in the archive's directory tree.
#[derive(Clone, Debug)]
pub struct TreeNode {
    /// Display name of this node.
    pub name: String,
    /// Full path within the archive.
    pub path: String,
    /// Whether this is expanded.
    pub expanded: bool,
    /// Children (subdirectories).
    pub children: Vec<TreeNode>,
    /// Number of files directly in this directory.
    pub file_count: usize,
    /// Total size of files in this directory.
    pub total_size: u64,
}

impl TreeNode {
    /// Create a new tree node.
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            expanded: false,
            children: Vec::new(),
            file_count: 0,
            total_size: 0,
        }
    }

    /// Recursively count all descendants.
    pub fn total_descendants(&self) -> usize {
        let mut count = self.children.len();
        for child in &self.children {
            count = count.saturating_add(child.total_descendants());
        }
        count
    }

    /// Find or create a child node at the given relative path.
    pub fn get_or_create_child(&mut self, name: &str, full_path: &str) -> &mut TreeNode {
        let idx = match self.children.iter().position(|c| c.name == name) {
            Some(idx) => idx,
            None => {
                self.children.push(TreeNode::new(name, full_path));
                self.children.len().saturating_sub(1)
            }
        };
        // Every safe accessor on `Vec` returns an `Option`, and there is no
        // sensible `TreeNode` to return in the `None` arm — a made-up node
        // would silently detach a whole subtree from the parent it belongs
        // to. `idx` is either what `position` just reported or the index of
        // the element `push` just added two lines up, so it is in range by
        // construction and the check the lint asks for is one the compiler
        // could do itself if it could see those two lines.
        #[allow(clippy::indexing_slicing)]
        &mut self.children[idx]
    }

    /// Toggle expansion state.
    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }

    /// Flip the expansion of the node whose full path is `path`.
    ///
    /// Returns whether such a node was found. Addressing by path rather than
    /// by flattened row index matters because expanding a node *changes* the
    /// flattened list: an index captured before the toggle names a different
    /// row after it, so a second click would collapse the wrong directory.
    pub fn toggle_path(&mut self, path: &str) -> bool {
        if self.path == path {
            self.toggle();
            return true;
        }
        self.children.iter_mut().any(|c| c.toggle_path(path))
    }

    /// Flatten the tree into a list for rendering, respecting expansion state.
    pub fn flatten(&self, depth: u32, out: &mut Vec<FlatTreeRow>) {
        out.push(FlatTreeRow {
            name: self.name.clone(),
            path: self.path.clone(),
            depth,
            expanded: self.expanded,
            has_children: !self.children.is_empty(),
            file_count: self.file_count,
        });
        if self.expanded {
            for child in &self.children {
                child.flatten(depth.saturating_add(1), out);
            }
        }
    }
}

/// A flattened tree row for rendering.
#[derive(Clone, Debug)]
pub struct FlatTreeRow {
    pub name: String,
    pub path: String,
    pub depth: u32,
    pub expanded: bool,
    pub has_children: bool,
    pub file_count: usize,
}

// ============================================================================
// Build the directory tree from a flat list of entries
// ============================================================================

/// Build a directory tree from archive entries.
pub fn build_directory_tree(entries: &[ArchiveEntry], archive_name: &str) -> TreeNode {
    let mut root = TreeNode::new(archive_name, "");
    root.expanded = true;

    for entry in entries {
        if entry.path.is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.path.split('/').collect();

        if entry.is_dir {
            // Create directory nodes for every component.
            let mut current = &mut root;
            let mut built_path = String::new();
            for part in &parts {
                if part.is_empty() {
                    continue;
                }
                if !built_path.is_empty() {
                    built_path.push('/');
                }
                built_path.push_str(part);
                current = current.get_or_create_child(part, &built_path);
            }
        } else {
            // For files, ensure parent directories exist and tally stats.
            let mut current = &mut root;
            let mut built_path = String::new();
            // Create directories for all but the last component: the last is
            // the file's own name, which is a row in the list, not a node in
            // the directory tree.
            if let Some((_file_name, dirs)) = parts.split_last() {
                for part in dirs {
                    if part.is_empty() {
                        continue;
                    }
                    if !built_path.is_empty() {
                        built_path.push('/');
                    }
                    built_path.push_str(part);
                    current = current.get_or_create_child(part, &built_path);
                }
            }
            current.file_count = current.file_count.saturating_add(1);
            current.total_size = current.total_size.saturating_add(entry.size);
        }
    }

    root
}

// ============================================================================
// Operations / actions
// ============================================================================

/// An operation that can be performed on an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveOperation {
    /// Open an archive file.
    Open(PathBuf),
    /// Extract all files to a destination directory.
    ExtractAll { destination: PathBuf },
    /// Extract only selected files.
    ExtractSelected {
        entries: Vec<String>,
        destination: PathBuf,
    },
    /// Create a new archive from a list of source files.
    Create {
        output: PathBuf,
        sources: Vec<PathBuf>,
        format: ArchiveFormat,
        level: CompressionLevel,
    },
    /// Add files to an existing archive.
    AddFiles { files: Vec<PathBuf> },
    /// Remove entries from an archive.
    RemoveEntries { paths: Vec<String> },
    /// Test archive integrity.
    TestArchive,
    /// Close the current archive.
    Close,
}

// ============================================================================
// Progress tracking
// ============================================================================

/// State of an ongoing operation.
#[derive(Clone, Debug)]
pub struct OperationProgress {
    /// What operation is in progress.
    pub operation: String,
    /// Current file being processed.
    pub current_file: String,
    /// Number of files processed so far.
    pub files_done: u64,
    /// Total number of files.
    pub files_total: u64,
    /// Bytes processed so far.
    pub bytes_done: u64,
    /// Total bytes to process.
    pub bytes_total: u64,
    /// Whether the operation has completed.
    pub completed: bool,
    /// Error message if the operation failed.
    pub error: Option<String>,
}

impl OperationProgress {
    /// Create a new progress tracker.
    pub fn new(operation: &str, files_total: u64, bytes_total: u64) -> Self {
        Self {
            operation: operation.to_string(),
            current_file: String::new(),
            files_done: 0,
            files_total,
            bytes_done: 0,
            bytes_total,
            completed: false,
            error: None,
        }
    }

    /// Percentage complete (0.0..100.0).
    ///
    /// Measured in bytes where there are bytes to measure, in files where
    /// there are not, and reported complete when there is neither — an
    /// archive of nothing is finished the moment it starts.
    #[must_use]
    pub fn percent(&self) -> f64 {
        ratio::percent(self.bytes_done, self.bytes_total)
            .or_else(|| ratio::percent(self.files_done, self.files_total))
            .unwrap_or(100.0)
    }

    /// Update progress for a file.
    pub fn advance_file(&mut self, name: &str, bytes: u64) {
        self.current_file = name.to_string();
        self.files_done = self.files_done.saturating_add(1);
        self.bytes_done = self.bytes_done.saturating_add(bytes);
    }

    /// Mark the operation as complete.
    pub fn finish(&mut self) {
        self.completed = true;
        self.files_done = self.files_total;
        self.bytes_done = self.bytes_total;
    }

    /// Mark the operation as failed.
    pub fn fail(&mut self, error: &str) {
        self.completed = true;
        self.error = Some(error.to_string());
    }

    /// Whether the operation is still running.
    pub fn is_running(&self) -> bool {
        !self.completed
    }
}

// ============================================================================
// Drag and drop model
// ============================================================================

/// Drag state for drag-and-drop operations.
#[derive(Clone, Debug, Default)]
pub enum DragState {
    /// Not dragging anything.
    #[default]
    Idle,
    /// Dragging files from the archive out (to extract).
    DraggingOut {
        /// Paths of entries being dragged.
        entries: Vec<String>,
        /// Current mouse position.
        mouse_x: f32,
        mouse_y: f32,
    },
    /// Dragging files in from the OS (to add).
    DraggingIn {
        /// External file paths being dragged in.
        files: Vec<PathBuf>,
        /// Current mouse position.
        mouse_x: f32,
        mouse_y: f32,
    },
}

impl DragState {
    /// Whether a drag is currently active.
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Number of items being dragged.
    pub fn item_count(&self) -> usize {
        match self {
            Self::Idle => 0,
            Self::DraggingOut { entries, .. } => entries.len(),
            Self::DraggingIn { files, .. } => files.len(),
        }
    }
}

// ============================================================================
// Test/verification results
// ============================================================================

/// Result of testing a single archive entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestResult {
    /// Entry is intact.
    Ok,
    /// CRC mismatch.
    CrcMismatch { expected: u32, actual: u32 },
    /// Data corruption detected.
    Corrupted(String),
    /// Could not decrypt (wrong password).
    DecryptionFailed,
    /// Entry not tested yet.
    Pending,
}

impl TestResult {
    pub fn display_text(&self) -> &str {
        match self {
            Self::Ok => "OK",
            Self::CrcMismatch { .. } => "CRC Error",
            Self::Corrupted(_) => "Corrupted",
            Self::DecryptionFailed => "Decrypt Failed",
            Self::Pending => "Pending",
        }
    }

    pub fn display_color(&self) -> Color {
        match self {
            Self::Ok => theme::GREEN,
            Self::Pending => theme::SUBTEXT0,
            _ => theme::RED,
        }
    }
}

/// Results for testing an entire archive.
#[derive(Clone, Debug)]
pub struct ArchiveTestResults {
    pub results: HashMap<String, TestResult>,
    pub total_entries: usize,
    pub tested: usize,
    pub passed: usize,
    pub failed: usize,
}

impl ArchiveTestResults {
    pub fn new(total: usize) -> Self {
        Self {
            results: HashMap::new(),
            total_entries: total,
            tested: 0,
            passed: 0,
            failed: 0,
        }
    }

    /// Record a test result for an entry.
    pub fn record(&mut self, path: &str, result: TestResult) {
        self.tested = self.tested.saturating_add(1);
        match &result {
            TestResult::Ok => self.passed = self.passed.saturating_add(1),
            TestResult::Pending => {}
            _ => self.failed = self.failed.saturating_add(1),
        }
        self.results.insert(path.to_string(), result);
    }

    /// Overall pass rate as a percentage.
    pub fn pass_rate(&self) -> f64 {
        if self.tested == 0 {
            return 0.0;
        }
        (self.passed as f64 / self.tested as f64) * 100.0
    }

    /// Whether all tested entries passed.
    pub fn all_passed(&self) -> bool {
        self.failed == 0 && self.tested > 0
    }
}

// ============================================================================
// Archive model (the currently open archive)
// ============================================================================

/// Represents a currently open archive.
#[derive(Clone, Debug)]
pub struct ArchiveModel {
    /// Path to the archive file on disk.
    pub path: PathBuf,
    /// Detected format.
    pub format: ArchiveFormat,
    /// All entries in the archive.
    pub entries: Vec<ArchiveEntry>,
    /// Directory tree built from entries.
    pub tree: TreeNode,
    /// Total uncompressed size of all entries.
    pub total_size: u64,
    /// Total compressed size.
    pub total_compressed: u64,
    /// Number of files (non-directory entries).
    pub file_count: usize,
    /// Number of directories.
    pub dir_count: usize,
    /// Whether the archive is encrypted.
    pub encrypted: bool,
    /// Whether this is a split/multi-volume archive.
    pub is_split: bool,
    /// Comment embedded in the archive (ZIP/7z support this).
    pub comment: String,
    /// Next unique entry id.
    next_id: u64,
}

impl ArchiveModel {
    /// Create a new empty archive model.
    pub fn new(path: &Path, format: ArchiveFormat) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive");
        Self {
            path: path.to_path_buf(),
            format,
            entries: Vec::new(),
            tree: TreeNode::new(name, ""),
            total_size: 0,
            total_compressed: 0,
            file_count: 0,
            dir_count: 0,
            encrypted: false,
            is_split: false,
            comment: String::new(),
            next_id: 1,
        }
    }

    /// Add an entry to the archive model.
    pub fn add_entry(&mut self, mut entry: ArchiveEntry) {
        entry.id = self.next_id;
        // Ids only have to be distinct, not dense, so saturating at the top
        // of the range would hand two entries the same id and make selection
        // ambiguous. Wrapping cannot collide in any archive that fits in
        // memory, and it never stops handing out ids.
        self.next_id = self.next_id.wrapping_add(1);

        if entry.is_dir {
            self.dir_count = self.dir_count.saturating_add(1);
        } else {
            self.file_count = self.file_count.saturating_add(1);
            self.total_size = self.total_size.saturating_add(entry.size);
            self.total_compressed = self.total_compressed.saturating_add(entry.compressed_size);
        }

        self.entries.push(entry);
    }

    /// Rebuild the directory tree from current entries.
    pub fn rebuild_tree(&mut self) {
        let name = self
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive");
        self.tree = build_directory_tree(&self.entries, name);
    }

    /// Get entries for a specific directory path.
    pub fn entries_in_directory(&self, dir_path: &str) -> Vec<&ArchiveEntry> {
        self.entries
            .iter()
            .filter(|e| e.parent_path() == dir_path)
            .collect()
    }

    /// Overall compression ratio.
    pub fn overall_ratio(&self) -> f64 {
        if self.total_size == 0 {
            return 0.0;
        }
        let ratio = 1.0 - (self.total_compressed as f64 / self.total_size as f64);
        (ratio * 100.0).clamp(0.0, 100.0)
    }

    /// Get selected entries.
    pub fn selected_entries(&self) -> Vec<&ArchiveEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }

    /// Select all entries.
    pub fn select_all(&mut self) {
        for entry in &mut self.entries {
            entry.selected = true;
        }
    }

    /// Deselect all entries.
    pub fn deselect_all(&mut self) {
        for entry in &mut self.entries {
            entry.selected = false;
        }
    }

    /// Toggle selection of an entry by id.
    pub fn toggle_selection(&mut self, id: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.selected = !entry.selected;
        }
    }

    /// Remove entries matching the given paths.
    pub fn remove_entries(&mut self, paths: &[String]) {
        self.entries.retain(|e| !paths.contains(&e.path));
        self.recalculate_stats();
        self.rebuild_tree();
    }

    /// Recalculate aggregate stats from current entries.
    pub fn recalculate_stats(&mut self) {
        self.total_size = 0;
        self.total_compressed = 0;
        self.file_count = 0;
        self.dir_count = 0;
        for entry in &self.entries {
            if entry.is_dir {
                self.dir_count = self.dir_count.saturating_add(1);
            } else {
                self.file_count = self.file_count.saturating_add(1);
                self.total_size = self.total_size.saturating_add(entry.size);
                self.total_compressed = self.total_compressed.saturating_add(entry.compressed_size);
            }
        }
    }

    /// Sort entries by the given column and direction.
    pub fn sort_entries(&mut self, sort: &SortState) {
        let dir_mult: std::cmp::Ordering = match sort.direction {
            SortDirection::Ascending => std::cmp::Ordering::Less,
            SortDirection::Descending => std::cmp::Ordering::Greater,
        };

        self.entries.sort_by(|a, b| {
            // Directories always come before files.
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }

            let ord = match sort.column {
                Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Column::Size => a.size.cmp(&b.size),
                Column::CompressedSize => a.compressed_size.cmp(&b.compressed_size),
                Column::Ratio => {
                    let ra = a.compression_ratio();
                    let rb = b.compression_ratio();
                    ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal)
                }
                Column::Date => a.modified.cmp(&b.modified),
                Column::Crc => a.crc32.cmp(&b.crc32),
                Column::Method => a.method.cmp(&b.method),
            };

            if dir_mult == std::cmp::Ordering::Greater {
                ord.reverse()
            } else {
                ord
            }
        });
    }
}

// ============================================================================
// Create archive settings
// ============================================================================

/// Settings for creating a new archive.
#[derive(Clone, Debug)]
pub struct CreateArchiveSettings {
    /// Output path for the new archive.
    pub output_path: PathBuf,
    /// Archive format.
    pub format: ArchiveFormat,
    /// Compression level.
    pub level: CompressionLevel,
    /// Source files/directories to include.
    pub sources: Vec<PathBuf>,
    /// Encryption settings.
    pub encryption: EncryptionSettings,
    /// Split archive settings.
    pub split: SplitSettings,
    /// Archive comment.
    pub comment: String,
    /// Whether to include empty directories.
    pub include_empty_dirs: bool,
    /// Whether to store full paths or relative paths.
    pub store_full_paths: bool,
}

impl Default for CreateArchiveSettings {
    fn default() -> Self {
        Self {
            output_path: PathBuf::new(),
            format: ArchiveFormat::Zip,
            level: CompressionLevel::Normal,
            sources: Vec::new(),
            encryption: EncryptionSettings::default(),
            split: SplitSettings::default(),
            comment: String::new(),
            include_empty_dirs: true,
            store_full_paths: false,
        }
    }
}

impl CreateArchiveSettings {
    /// Validate settings before creating. Returns a list of problems.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.output_path.as_os_str().is_empty() {
            problems.push("Output path is required".into());
        }

        if self.sources.is_empty() {
            problems.push("No source files selected".into());
        }

        if self.encryption.is_enabled() && !self.format.supports_encryption() {
            problems.push(format!(
                "{} does not support encryption",
                self.format.display_name()
            ));
        }

        if self.split.enabled && !self.format.supports_split() {
            problems.push(format!(
                "{} does not support split archives",
                self.format.display_name()
            ));
        }

        if self.split.enabled && self.split.volume_size < 65536 {
            problems.push("Volume size must be at least 64 KiB".into());
        }

        if self.encryption.is_enabled() && self.encryption.password.is_empty() {
            problems.push("Password cannot be empty when encryption is enabled".into());
        }

        problems
    }
}

// ============================================================================
// Application state
// ============================================================================

/// View mode for the file list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Flat list of all entries.
    FlatList,
    /// Only entries in the currently selected directory.
    #[default]
    DirectoryView,
}

/// The full application state.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Currently open archive, if any.
    pub archive: Option<ArchiveModel>,
    /// Current working directory within the archive (for directory view).
    pub current_dir: String,
    /// Sort state.
    pub sort: SortState,
    /// View mode.
    pub view_mode: ViewMode,
    /// Current operation progress, if any.
    pub progress: Option<OperationProgress>,
    /// Drag-and-drop state.
    pub drag: DragState,
    /// Whether the sidebar (tree view) is visible.
    pub sidebar_visible: bool,
    /// Sidebar width in pixels.
    pub sidebar_width: f32,
    /// Window dimensions.
    pub window_width: f32,
    pub window_height: f32,
    /// Scroll offset for the file list.
    pub list_scroll_y: f32,
    /// Scroll offset for the tree view.
    pub tree_scroll_y: f32,
    /// Currently hovered entry id, if any.
    pub hovered_entry: Option<u64>,
    /// Test results, if a test is running or completed.
    pub test_results: Option<ArchiveTestResults>,
    /// Status bar message.
    pub status_message: String,
    /// Navigation history (directories visited).
    pub nav_history: Vec<String>,
    /// Position in navigation history.
    pub nav_position: usize,
    /// Carries the fractional part of a wheel delta between events.
    ///
    /// A trackpad sends many sub-row deltas; rounding each one on its own
    /// discards the remainder and the list never moves. The accumulator keeps
    /// it, so slow scrolling still advances a row eventually.
    pub wheel: wheel::Accumulator,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            archive: None,
            current_dir: String::new(),
            sort: SortState::default(),
            view_mode: ViewMode::default(),
            progress: None,
            drag: DragState::default(),
            sidebar_visible: true,
            sidebar_width: 220.0,
            window_width: 900.0,
            window_height: 600.0,
            list_scroll_y: 0.0,
            tree_scroll_y: 0.0,
            hovered_entry: None,
            test_results: None,
            status_message: String::from("Ready"),
            nav_history: vec![String::new()],
            nav_position: 0,
            wheel: wheel::Accumulator::default(),
        }
    }
}

impl AppState {
    /// Navigate to a directory within the archive.
    pub fn navigate_to(&mut self, dir: &str) {
        // Truncate forward history if we've gone back: navigating somewhere
        // new from the middle of the history abandons the branch ahead, the
        // same way a browser's forward button greys out.
        let next = self.nav_position.saturating_add(1);
        if next < self.nav_history.len() {
            self.nav_history.truncate(next);
        }
        self.current_dir = dir.to_string();
        self.nav_history.push(dir.to_string());
        self.nav_position = self.nav_history.len().saturating_sub(1);
        self.list_scroll_y = 0.0;
    }

    /// Navigate back in history.
    pub fn navigate_back(&mut self) -> bool {
        let Some(prev) = self.nav_position.checked_sub(1) else {
            return false;
        };
        let Some(dir) = self.nav_history.get(prev).cloned() else {
            return false;
        };
        self.nav_position = prev;
        self.current_dir = dir;
        self.list_scroll_y = 0.0;
        true
    }

    /// Navigate forward in history.
    pub fn navigate_forward(&mut self) -> bool {
        let next = self.nav_position.saturating_add(1);
        let Some(dir) = self.nav_history.get(next).cloned() else {
            return false;
        };
        self.nav_position = next;
        self.current_dir = dir;
        self.list_scroll_y = 0.0;
        true
    }

    /// Navigate up one directory level.
    pub fn navigate_up(&mut self) -> bool {
        if self.current_dir.is_empty() {
            return false;
        }
        let parent = if let Some(pos) = self.current_dir.rfind('/') {
            &self.current_dir[..pos]
        } else {
            ""
        };
        let parent_owned = parent.to_string();
        self.navigate_to(&parent_owned);
        true
    }

    /// Get entries to display in the current view.
    pub fn visible_entries(&self) -> Vec<&ArchiveEntry> {
        match &self.archive {
            None => Vec::new(),
            Some(archive) => match self.view_mode {
                ViewMode::FlatList => archive.entries.iter().collect(),
                ViewMode::DirectoryView => archive.entries_in_directory(&self.current_dir),
            },
        }
    }

    /// Column header text (with sort indicator if applicable).
    pub fn column_header_text(&self, col: Column) -> String {
        let base = col.header();
        if col == self.sort.column {
            format!("{base}{}", self.sort.direction.indicator())
        } else {
            base.to_string()
        }
    }

    /// Toggle sort on a column. If already sorting by this column, toggle
    /// direction. Otherwise switch to this column ascending.
    pub fn toggle_sort(&mut self, col: Column) {
        if self.sort.column == col {
            self.sort.direction = self.sort.direction.toggle();
        } else {
            self.sort.column = col;
            self.sort.direction = SortDirection::Ascending;
        }
        if let Some(archive) = &mut self.archive {
            archive.sort_entries(&self.sort);
        }
    }

    /// Format the status bar text.
    pub fn status_text(&self) -> String {
        match &self.archive {
            None => "No archive open".to_string(),
            Some(archive) => {
                let selected = archive.selected_entries().len();
                let total_files = archive.file_count;
                let ratio = archive.overall_ratio();
                if selected > 0 {
                    let sel_size: u64 = archive.selected_entries().iter().map(|e| e.size).sum();
                    format!(
                        "{selected} of {total_files} files selected ({}) | Ratio: {ratio:.1}%",
                        ArchiveEntry::format_size(sel_size)
                    )
                } else {
                    format!(
                        "{total_files} files, {} dirs | {} -> {} | Ratio: {ratio:.1}%",
                        archive.dir_count,
                        ArchiveEntry::format_size(archive.total_size),
                        ArchiveEntry::format_size(archive.total_compressed),
                    )
                }
            }
        }
    }
}

// ============================================================================
// Layout constants
// ============================================================================
//
// These were `let`s inside the renderers, and three of them were *also*
// written out again in `render_frame` so it could work out where the content
// area ended. Two copies of one number is how a status bar ends up 24 pixels
// tall and 28 pixels of layout: both copies look right, and only their
// disagreement is visible. There is one copy now, and the hit-test reads the
// same one the renderer does.

/// Height of the button toolbar at the top of the window.
const TOOLBAR_H: f32 = 40.0;
/// Height of a toolbar button (centred vertically within `TOOLBAR_H`).
const TOOLBAR_BUTTON_H: f32 = 28.0;
/// Height of the address/path bar below the toolbar.
const PATH_BAR_H: f32 = 32.0;
/// Side of the square back/forward/up buttons in the path bar.
const NAV_BUTTON_SIZE: f32 = 24.0;
/// Horizontal step between the path bar's nav buttons.
const NAV_BUTTON_STEP: f32 = 28.0;
/// Height of the file-list column header strip.
const HEADER_H: f32 = 24.0;
/// Height of one row, in both the file list and the sidebar tree.
const ROW_H: f32 = 22.0;
/// Vertical gap between the sidebar's "Archive Tree" caption and its first row.
const TREE_HEADER_H: f32 = 28.0;
/// Height of the progress strip shown while an operation runs.
const PROGRESS_H: f32 = 48.0;
/// Height of the status bar along the bottom.
const STATUS_H: f32 = 24.0;

/// Default window size, and the size the pure `render_frame` helper draws at.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 600.0;
/// The window refuses to be drawn smaller than this. Below roughly this size
/// the column headers alone are wider than the window, so every row is a
/// horizontal scroll and nothing is legible.
const MIN_WIDTH: f32 = 520.0;
const MIN_HEIGHT: f32 = 260.0;

// ============================================================================
// Geometry, targets, and the frame that carries both
// ============================================================================

/// An axis-aligned rectangle in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    #[must_use]
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Whether `(px, py)` is inside this rectangle.
    ///
    /// Half-open on both axes — the right and bottom edges belong to whatever
    /// is next. Two rows that share a boundary pixel would otherwise both
    /// claim it, and which one won would depend on the order they happened to
    /// be recorded in.
    #[must_use]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// The overlap between two rectangles, or `None` if they do not overlap.
    #[must_use]
    fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        if right <= x || bottom <= y {
            return None;
        }
        Some(Rect::new(x, y, right - x, bottom - y))
    }
}

/// The seven buttons along the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    Open,
    New,
    ExtractAll,
    ExtractSelected,
    Add,
    Delete,
    Test,
}

impl ToolbarAction {
    /// The label painted on the button, which is also what the status line
    /// names when the button is pressed. One string, so they cannot drift.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::New => "New",
            Self::ExtractAll => "Extract All",
            Self::ExtractSelected => "Extract Sel.",
            Self::Add => "Add",
            Self::Delete => "Delete",
            Self::Test => "Test",
        }
    }

    /// The single-character icon drawn before the label.
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Open => "O",
            Self::New => "N",
            Self::ExtractAll => "E",
            Self::ExtractSelected => "S",
            Self::Add => "+",
            Self::Delete => "X",
            Self::Test => "T",
        }
    }

    /// All toolbar buttons, left to right.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Open,
            Self::New,
            Self::ExtractAll,
            Self::ExtractSelected,
            Self::Add,
            Self::Delete,
            Self::Test,
        ]
    }
}

/// Everything in the window a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A toolbar button.
    Toolbar(ToolbarAction),
    /// Back one directory in history.
    NavBack,
    /// Forward one directory in history.
    NavForward,
    /// Up to the parent directory.
    NavUp,
    /// A row in the sidebar tree, by its index in the flattened row list.
    TreeRow(usize),
    /// The expand/collapse arrow on a sidebar tree row.
    TreeArrow(usize),
    /// A column header, which sorts by that column.
    ColumnHeader(Column),
    /// A row in the file list, by the entry's stable id.
    FileRow(u64),
}

/// A frame being drawn.
///
/// The renderer records the box it paints for every control as it paints it,
/// so the hit-test can be *the renderer*: run it, then read the boxes back.
/// The alternative — a `Layout` struct that measures everything a second time
/// — is two transcriptions of the same arithmetic, and the one that is wrong
/// is whichever one you are not currently reading.
pub struct Frame {
    /// The commands to hand to the compositor.
    pub tree: RenderTree,
    /// Every clickable box recorded this frame, in paint order.
    hits: Vec<(Target, Rect)>,
    /// The active clip stack, mirroring `PushClip`/`PopClip` in `tree`.
    clips: Vec<Rect>,
    /// The size this frame is being drawn at.
    width: f32,
    height: f32,
}

impl Frame {
    fn new(width: f32, height: f32) -> Self {
        Self {
            tree: RenderTree::new(),
            hits: Vec::new(),
            clips: Vec::new(),
            width: width.max(MIN_WIDTH),
            height: height.max(MIN_HEIGHT),
        }
    }

    /// Record a draw command, tracking clips as they are pushed and popped.
    fn push(&mut self, command: RenderCommand) {
        match &command {
            RenderCommand::PushClip {
                x,
                y,
                width,
                height,
            } => {
                let rect = Rect::new(*x, *y, *width, *height);
                // A nested clip can only shrink the visible region, never grow
                // it, so the effective clip is the intersection with the one
                // already in force.
                let effective = match self.clips.last() {
                    Some(outer) => outer.intersect(&rect),
                    None => Some(rect),
                };
                self.clips
                    .push(effective.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)));
            }
            RenderCommand::PopClip => {
                self.clips.pop();
            }
            _ => {}
        }
        self.tree.push(command);
    }

    /// Record that `target` occupies `rect`.
    ///
    /// The rect is trimmed to the clip in force, and dropped entirely if it
    /// falls outside: a row scrolled half off the top of the list is only
    /// clickable on the half that is actually on screen. Recording the whole
    /// row would make the invisible half of it steal clicks from whatever is
    /// painted above the list.
    fn hit(&mut self, target: Target, rect: Rect) {
        let visible = match self.clips.last() {
            Some(clip) => match clip.intersect(&rect) {
                Some(r) => r,
                None => return,
            },
            None => rect,
        };
        self.hits.push((target, visible));
    }

    /// The topmost control at `(x, y)`, if any.
    ///
    /// Back to front, because later commands paint over earlier ones: the
    /// drag overlay covers the file list, so it must also intercept its
    /// clicks.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Target> {
        self.hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }
}

// ============================================================================
// UI rendering
// ============================================================================

/// Whether `action` is available in `state`.
///
/// The renderer greys the button out with this and the click handler refuses
/// it with the same call, so a button that looks dead is dead. Two answers to
/// "is this enabled?" is how a greyed-out Delete still deletes.
#[must_use]
pub fn toolbar_enabled(state: &AppState, action: ToolbarAction) -> bool {
    let has_archive = state.archive.is_some();
    let has_selection = state
        .archive
        .as_ref()
        .is_some_and(|a| a.entries.iter().any(|e| e.selected));
    match action {
        ToolbarAction::Open | ToolbarAction::New => true,
        ToolbarAction::ExtractAll | ToolbarAction::Add | ToolbarAction::Test => has_archive,
        ToolbarAction::ExtractSelected | ToolbarAction::Delete => has_selection,
    }
}

/// Render the toolbar.
pub fn render_toolbar(state: &AppState, frame: &mut Frame, y_offset: f32, width: f32) -> f32 {
    // Background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: y_offset,
        width,
        height: TOOLBAR_H,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let mut x = 8.0;
    let btn_y = y_offset + (TOOLBAR_H - TOOLBAR_BUTTON_H) / 2.0;

    for action in ToolbarAction::all() {
        let enabled = toolbar_enabled(state, *action);
        let text = format!("[{}] {}", action.icon(), action.label());
        let btn_w = text::padded_width(&text, 8.0, 12.0, FontWeightHint::Regular);
        let bg = if enabled {
            theme::SURFACE1
        } else {
            theme::MANTLE
        };
        let fg = if enabled {
            theme::TEXT
        } else {
            theme::OVERLAY0
        };

        frame.push(RenderCommand::FillRect {
            x,
            y: btn_y,
            width: btn_w,
            height: TOOLBAR_BUTTON_H,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::Text {
            x: x + 8.0,
            y: btn_y + 7.0,
            text,
            color: fg,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn_w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
        // A disabled button is still recorded, so that clicking it reports why
        // it did nothing rather than falling through to the toolbar behind it.
        frame.hit(
            Target::Toolbar(*action),
            Rect::new(x, btn_y, btn_w, TOOLBAR_BUTTON_H),
        );

        x += btn_w + 4.0;
    }

    // Separator line
    frame.push(RenderCommand::Line {
        x1: 0.0,
        y1: y_offset + TOOLBAR_H - 1.0,
        x2: width,
        y2: y_offset + TOOLBAR_H - 1.0,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    TOOLBAR_H
}

/// Render the address/path bar.
pub fn render_path_bar(state: &AppState, frame: &mut Frame, y_offset: f32, width: f32) -> f32 {
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: y_offset,
        width,
        height: PATH_BAR_H,
        color: theme::MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Back / Forward / Up buttons
    let nav_btns = [
        ("<", Target::NavBack),
        (">", Target::NavForward),
        ("^", Target::NavUp),
    ];
    let mut x = 4.0;
    for (btn_text, target) in &nav_btns {
        frame.push(RenderCommand::FillRect {
            x,
            y: y_offset + 4.0,
            width: NAV_BUTTON_SIZE,
            height: NAV_BUTTON_SIZE,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        frame.push(RenderCommand::Text {
            x: x + 8.0,
            y: y_offset + 10.0,
            text: (*btn_text).to_string(),
            color: theme::TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.hit(
            *target,
            Rect::new(x, y_offset + 4.0, NAV_BUTTON_SIZE, NAV_BUTTON_SIZE),
        );
        x += NAV_BUTTON_STEP;
    }

    // Path display
    let path_text = if let Some(archive) = &state.archive {
        let archive_name = archive
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive");
        if state.current_dir.is_empty() {
            archive_name.to_string()
        } else {
            format!("{archive_name}/{}", state.current_dir)
        }
    } else {
        "No archive open".to_string()
    };

    let path_x = x + 8.0;
    frame.push(RenderCommand::FillRect {
        x: path_x,
        y: y_offset + 4.0,
        width: width - path_x - 8.0,
        height: NAV_BUTTON_SIZE,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::Text {
        x: path_x + 8.0,
        y: y_offset + 10.0,
        text: path_text,
        color: theme::TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width - path_x - 24.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Bottom separator
    frame.push(RenderCommand::Line {
        x1: 0.0,
        y1: y_offset + PATH_BAR_H - 1.0,
        x2: width,
        y2: y_offset + PATH_BAR_H - 1.0,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    PATH_BAR_H
}

/// Render the sidebar tree view.
pub fn render_sidebar(state: &AppState, frame: &mut Frame, y_offset: f32, height: f32) -> f32 {
    if !state.sidebar_visible {
        return 0.0;
    }
    let w = state.sidebar_width;

    // Background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: y_offset,
        width: w,
        height,
        color: theme::MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Tree header
    frame.push(RenderCommand::Text {
        x: 8.0,
        y: y_offset + 8.0,
        text: "Archive Tree".to_string(),
        color: theme::BLUE,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(w - 16.0),
        overflow: TextOverflow::Ellipsis,
    });

    if let Some(archive) = &state.archive {
        let mut rows = Vec::new();
        archive.tree.flatten(0, &mut rows);

        let start_y = y_offset + TREE_HEADER_H;

        frame.push(RenderCommand::PushClip {
            x: 0.0,
            y: start_y,
            width: w,
            height: height - TREE_HEADER_H,
        });

        for (i, row) in rows.iter().enumerate() {
            let ry = start_y + i as f32 * ROW_H - state.tree_scroll_y;
            if ry + ROW_H < start_y || ry > y_offset + height {
                continue;
            }

            let indent = row.depth as f32 * 16.0 + 8.0;

            // Highlight if this is the current directory.
            if row.path == state.current_dir {
                frame.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: ry,
                    width: w,
                    height: ROW_H,
                    color: theme::SURFACE1,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Expand/collapse indicator.
            let arrow = if !row.has_children {
                " "
            } else if row.expanded {
                "v"
            } else {
                ">"
            };
            frame.push(RenderCommand::Text {
                x: indent,
                y: ry + 4.0,
                text: arrow.to_string(),
                color: theme::OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Folder icon and name.
            let display = format!("\u{1F4C1} {}", row.name);
            frame.push(RenderCommand::Text {
                x: indent + 12.0,
                y: ry + 4.0,
                text: display,
                color: theme::TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - indent - 20.0),
                overflow: TextOverflow::Ellipsis,
            });

            // File count badge.
            if row.file_count > 0 {
                let count_text = format!("{}", row.file_count);
                frame.push(RenderCommand::Text {
                    x: w - 30.0,
                    y: ry + 4.0,
                    text: count_text,
                    color: theme::SUBTEXT0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // The row navigates; the arrow expands. Recording the row first
            // and the arrow second is what makes the arrow win where they
            // overlap, because the hit-test reads back to front.
            frame.hit(Target::TreeRow(i), Rect::new(0.0, ry, w, ROW_H));
            if row.has_children {
                // The arrow's box is the indent column it is drawn in, not the
                // glyph's ink: a one-character target is unhittable.
                frame.hit(
                    Target::TreeArrow(i),
                    Rect::new(indent - 2.0, ry, 14.0, ROW_H),
                );
            }
        }

        frame.push(RenderCommand::PopClip);
    }

    // Right border.
    frame.push(RenderCommand::Line {
        x1: w - 1.0,
        y1: y_offset,
        x2: w - 1.0,
        y2: y_offset + height,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    w
}

/// Render the column headers for the file list.
pub fn render_column_headers(
    state: &AppState,
    frame: &mut Frame,
    x_offset: f32,
    y_offset: f32,
    width: f32,
) -> f32 {
    frame.push(RenderCommand::FillRect {
        x: x_offset,
        y: y_offset,
        width,
        height: HEADER_H,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let mut x = x_offset + 4.0;
    for col in Column::all() {
        let col_w = col.default_width();
        let text = state.column_header_text(*col);

        frame.push(RenderCommand::Text {
            x: x + 4.0,
            y: y_offset + 5.0,
            text,
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(col_w - 8.0),
            overflow: TextOverflow::Ellipsis,
        });

        // The whole column heading sorts, not just the width of its caption --
        // "Ratio" is five characters in a sixty-pixel column, and a user aims
        // at the column, not at the word.
        frame.hit(
            Target::ColumnHeader(*col),
            Rect::new(x, y_offset, col_w, HEADER_H),
        );

        // Column separator.
        x += col_w;
        frame.push(RenderCommand::Line {
            x1: x,
            y1: y_offset + 2.0,
            x2: x,
            y2: y_offset + HEADER_H - 2.0,
            color: theme::OVERLAY0,
            width: 1.0,
        });
    }

    // Bottom separator.
    frame.push(RenderCommand::Line {
        x1: x_offset,
        y1: y_offset + HEADER_H - 1.0,
        x2: x_offset + width,
        y2: y_offset + HEADER_H - 1.0,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    HEADER_H
}

/// Render a single row in the file list.
pub fn render_file_row(
    entry: &ArchiveEntry,
    frame: &mut Frame,
    x_offset: f32,
    y: f32,
    width: f32,
    is_hovered: bool,
) {
    // Row background.
    if entry.selected {
        frame.push(RenderCommand::FillRect {
            x: x_offset,
            y,
            width,
            height: ROW_H,
            color: Color::rgba(theme::BLUE.r, theme::BLUE.g, theme::BLUE.b, 60),
            corner_radii: CornerRadii::ZERO,
        });
    } else if is_hovered {
        frame.push(RenderCommand::FillRect {
            x: x_offset,
            y,
            width,
            height: ROW_H,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
    }

    let mut x = x_offset + 4.0;

    // Name column.
    let icon = if entry.is_dir {
        "\u{1F4C1} "
    } else if entry.encrypted {
        "\u{1F512} "
    } else {
        "\u{1F4C4} "
    };
    let name_text = format!("{icon}{}", entry.name);
    let name_color = if entry.is_dir {
        theme::BLUE
    } else {
        theme::TEXT
    };
    frame.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 4.0,
        text: name_text,
        color: name_color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(Column::Name.default_width() - 8.0),
        overflow: TextOverflow::Ellipsis,
    });
    x += Column::Name.default_width();

    // Size column.
    if !entry.is_dir {
        frame.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: ArchiveEntry::format_size(entry.size),
            color: theme::TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::Size.default_width() - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
    x += Column::Size.default_width();

    // Compressed size.
    if !entry.is_dir {
        frame.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: ArchiveEntry::format_size(entry.compressed_size),
            color: theme::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::CompressedSize.default_width() - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
    x += Column::CompressedSize.default_width();

    // Ratio.
    if !entry.is_dir {
        let ratio = entry.compression_ratio();
        let ratio_color = if ratio > 50.0 {
            theme::GREEN
        } else if ratio > 20.0 {
            theme::YELLOW
        } else {
            theme::PEACH
        };
        frame.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: format!("{ratio:.0}%"),
            color: ratio_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::Ratio.default_width() - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
    x += Column::Ratio.default_width();

    // Date.
    frame.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 4.0,
        text: ArchiveEntry::format_date(entry.modified),
        color: theme::SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(Column::Date.default_width() - 8.0),
        overflow: TextOverflow::Ellipsis,
    });
    x += Column::Date.default_width();

    // CRC.
    if !entry.is_dir {
        frame.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: ArchiveEntry::format_crc(entry.crc32),
            color: theme::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::Crc.default_width() - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
    x += Column::Crc.default_width();

    // Method.
    frame.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 4.0,
        text: entry.method.clone(),
        color: theme::SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(Column::Method.default_width() - 8.0),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Render the file list view.
pub fn render_file_list(
    state: &AppState,
    frame: &mut Frame,
    x_offset: f32,
    y_offset: f32,
    width: f32,
    height: f32,
) {
    // Background.
    frame.push(RenderCommand::FillRect {
        x: x_offset,
        y: y_offset,
        width,
        height,
        color: theme::BASE,
        corner_radii: CornerRadii::ZERO,
    });

    let entries = state.visible_entries();

    frame.push(RenderCommand::PushClip {
        x: x_offset,
        y: y_offset,
        width,
        height,
    });

    for (i, entry) in entries.iter().enumerate() {
        let ry = y_offset + i as f32 * ROW_H - state.list_scroll_y;
        if ry + ROW_H < y_offset || ry > y_offset + height {
            continue;
        }

        // Alternating row backgrounds.
        if i % 2 == 1 {
            frame.push(RenderCommand::FillRect {
                x: x_offset,
                y: ry,
                width,
                height: ROW_H,
                color: Color::rgba(theme::SURFACE0.r, theme::SURFACE0.g, theme::SURFACE0.b, 40),
                corner_radii: CornerRadii::ZERO,
            });
        }

        let is_hovered = state.hovered_entry == Some(entry.id);
        render_file_row(entry, frame, x_offset, ry, width, is_hovered);
        // By id, not by row number. The list re-sorts under the pointer when a
        // column header is clicked, and a row index would then name whatever
        // slid into that position.
        frame.hit(
            Target::FileRow(entry.id),
            Rect::new(x_offset, ry, width, ROW_H),
        );
    }

    frame.push(RenderCommand::PopClip);

    // "No archive open" message.
    if state.archive.is_none() {
        frame.push(RenderCommand::Text {
            x: x_offset + width / 2.0 - 80.0,
            y: y_offset + height / 2.0 - 20.0,
            text: "Drop an archive here".to_string(),
            color: theme::OVERLAY0,
            font_size: 16.0,
            font_weight: FontWeightHint::Light,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        frame.push(RenderCommand::Text {
            x: x_offset + width / 2.0 - 100.0,
            y: y_offset + height / 2.0 + 4.0,
            text: "or use Open to browse".to_string(),
            color: theme::OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(220.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// Render the progress bar for ongoing operations.
pub fn render_progress_bar(
    progress: &OperationProgress,
    frame: &mut Frame,
    x: f32,
    y: f32,
    width: f32,
) -> f32 {
    // Background.
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: PROGRESS_H,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    // Operation label.
    frame.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 4.0,
        text: format!("{}: {}", progress.operation, progress.current_file),
        color: theme::TEXT,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width - 16.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Progress bar track.
    let bar_x = x + 8.0;
    let bar_y = y + 22.0;
    let bar_w = width - 80.0;
    let bar_track_h = 12.0;

    frame.push(RenderCommand::FillRect {
        x: bar_x,
        y: bar_y,
        width: bar_w,
        height: bar_track_h,
        color: theme::MANTLE,
        corner_radii: CornerRadii::all(6.0),
    });

    // Progress fill.
    let pct = progress.percent() / 100.0;
    let fill_w = (bar_w * pct as f32).clamp(0.0, bar_w);
    let fill_color = if progress.error.is_some() {
        theme::RED
    } else if progress.completed {
        theme::GREEN
    } else {
        theme::BLUE
    };

    if fill_w > 0.0 {
        frame.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: fill_w,
            height: bar_track_h,
            color: fill_color,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    // Percentage text.
    frame.push(RenderCommand::Text {
        x: bar_x + bar_w + 8.0,
        y: bar_y + 1.0,
        text: format!("{:.0}%", progress.percent()),
        color: theme::TEXT,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // File count.
    frame.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 36.0,
        text: format!("{}/{} files", progress.files_done, progress.files_total),
        color: theme::SUBTEXT0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    PROGRESS_H
}

/// Render the status bar.
pub fn render_status_bar(state: &AppState, frame: &mut Frame, y: f32, width: f32) -> f32 {
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width,
        height: STATUS_H,
        color: theme::MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Top separator.
    frame.push(RenderCommand::Line {
        x1: 0.0,
        y1: y,
        x2: width,
        y2: y,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    // Status text.
    frame.push(RenderCommand::Text {
        x: 8.0,
        y: y + 6.0,
        text: state.status_text(),
        color: theme::SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width / 2.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Format badge on the right.
    if let Some(archive) = &state.archive {
        let format_text = archive.format.display_name();
        frame.push(RenderCommand::Text {
            x: width - 120.0,
            y: y + 6.0,
            text: format_text.to_string(),
            color: theme::PEACH,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(110.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    STATUS_H
}

/// Render the drag-and-drop overlay when dragging files.
pub fn render_drag_overlay(
    drag: &DragState,
    frame: &mut Frame,
    window_width: f32,
    window_height: f32,
) {
    match drag {
        DragState::Idle => {}
        DragState::DraggingOut {
            entries,
            mouse_x,
            mouse_y,
        } => {
            // Semi-transparent overlay.
            frame.push(RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: window_width,
                height: window_height,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::ZERO,
            });
            // Floating badge near cursor.
            let badge_w = 140.0;
            let badge_h = 28.0;
            frame.push(RenderCommand::FillRect {
                x: *mouse_x + 12.0,
                y: *mouse_y + 12.0,
                width: badge_w,
                height: badge_h,
                color: theme::SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            frame.push(RenderCommand::Text {
                x: *mouse_x + 20.0,
                y: *mouse_y + 20.0,
                text: format!("Extract {} file(s)", entries.len()),
                color: theme::GREEN,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(badge_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        DragState::DraggingIn {
            files,
            mouse_x,
            mouse_y,
        } => {
            frame.push(RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: window_width,
                height: window_height,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::ZERO,
            });
            frame.push(RenderCommand::StrokeRect {
                x: 20.0,
                y: 20.0,
                width: window_width - 40.0,
                height: window_height - 40.0,
                color: theme::BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(8.0),
            });
            let badge_w = 140.0;
            let badge_h = 28.0;
            frame.push(RenderCommand::FillRect {
                x: *mouse_x + 12.0,
                y: *mouse_y + 12.0,
                width: badge_w,
                height: badge_h,
                color: theme::SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            frame.push(RenderCommand::Text {
                x: *mouse_x + 20.0,
                y: *mouse_y + 20.0,
                text: format!("Add {} file(s)", files.len()),
                color: theme::BLUE,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(badge_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

/// Draw the whole window, recording where every control ended up.
///
/// This is the only renderer. `render_frame` throws the hit boxes away and
/// `AppState::hit_test` throws the drawing away, but both run *this*, so there
/// is no way for what the user sees and what the user can click to disagree.
#[must_use]
pub fn build_frame(state: &AppState, width: f32, height: f32) -> Frame {
    let mut frame = Frame::new(width, height);
    let w = frame.width;
    let h = frame.height;

    // Window background.
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: w,
        height: h,
        color: theme::BASE,
        corner_radii: CornerRadii::ZERO,
    });

    let mut y = 0.0;

    // Toolbar.
    y += render_toolbar(state, &mut frame, y, w);

    // Path bar.
    y += render_path_bar(state, &mut frame, y, w);

    // `content_band` is the single copy of this arithmetic: the scroll clamps
    // read it too, so the list the user can scroll is exactly the list that
    // was drawn. `y` is asserted against it rather than recomputed, so a
    // future change to a bar's height cannot silently desynchronise the two.
    let (content_top, content_h) = state.content_band(h);
    debug_assert!(
        (y - content_top).abs() < 0.5,
        "content band and renderer disagree"
    );
    y = content_top;

    // Status bar at the bottom.
    let status_y = h - STATUS_H;

    // Progress strip sits between the content and the status bar.
    if let Some(progress) = &state.progress {
        render_progress_bar(progress, &mut frame, 0.0, status_y - PROGRESS_H, w);
    }

    // Sidebar tree.
    let sidebar_w = render_sidebar(state, &mut frame, y, content_h);

    // Column headers.
    let list_x = sidebar_w;
    let list_w = w - sidebar_w;
    let header_h = render_column_headers(state, &mut frame, list_x, y, list_w);

    // File list.
    render_file_list(
        state,
        &mut frame,
        list_x,
        y + header_h,
        list_w,
        content_h - header_h,
    );

    // Status bar.
    render_status_bar(state, &mut frame, status_y, w);

    // Drag overlay (on top of everything).
    render_drag_overlay(&state.drag, &mut frame, w, h);

    frame
}

/// Render the entire application frame at the size recorded in `state`.
#[must_use]
pub fn render_frame(state: &AppState) -> Vec<RenderCommand> {
    build_frame(state, state.window_width, state.window_height)
        .tree
        .commands
}

// ============================================================================
// Interaction
// ============================================================================

/// What the window should do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing changed; do not repaint.
    None,
    /// Something changed; repaint.
    Redraw,
    /// Close the window.
    Quit,
}

impl AppState {
    /// The vertical band between the path bar and whatever is at the bottom.
    ///
    /// Returns `(top, height)`. The renderer and every scroll clamp read this
    /// one function, because the progress strip appearing changes the height
    /// of the list — and a scroll limit computed against the taller list would
    /// let the last rows be scrolled behind the progress bar and stay there.
    #[must_use]
    pub fn content_band(&self, height: f32) -> (f32, f32) {
        let top = TOOLBAR_H + PATH_BAR_H;
        let mut bottom = height.max(MIN_HEIGHT) - STATUS_H;
        if self.progress.is_some() {
            bottom -= PROGRESS_H;
        }
        (top, (bottom - top).max(0.0))
    }

    /// Largest useful `list_scroll_y` — the offset that puts the last row at
    /// the bottom of the viewport. Scrolling past it would show blank space.
    #[must_use]
    pub fn max_list_scroll(&self, height: f32) -> f32 {
        let (_, content_h) = self.content_band(height);
        let viewport = (content_h - HEADER_H).max(0.0);
        let total = self.visible_entries().len() as f32 * ROW_H;
        (total - viewport).max(0.0)
    }

    /// Largest useful `tree_scroll_y`.
    #[must_use]
    pub fn max_tree_scroll(&self, height: f32) -> f32 {
        let (_, content_h) = self.content_band(height);
        let viewport = (content_h - TREE_HEADER_H).max(0.0);
        let total = self.tree_rows().len() as f32 * ROW_H;
        (total - viewport).max(0.0)
    }

    /// The sidebar tree flattened to the rows currently on screen.
    ///
    /// The renderer computes this the same way; a click handler that guessed
    /// at it instead would disagree the moment a node was collapsed.
    #[must_use]
    pub fn tree_rows(&self) -> Vec<FlatTreeRow> {
        let mut rows = Vec::new();
        if let Some(archive) = &self.archive {
            archive.tree.flatten(0, &mut rows);
        }
        rows
    }

    /// The topmost control at `(x, y)` in a window of size `size`.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32, size: (f32, f32)) -> Option<Target> {
        build_frame(self, size.0, size.1).hit_test(x, y)
    }

    /// The entry ids currently on screen, in display order.
    #[must_use]
    fn visible_ids(&self) -> Vec<u64> {
        self.visible_entries().iter().map(|e| e.id).collect()
    }

    /// Scroll the list so that the row at `index` is fully visible.
    fn reveal_row(&mut self, index: usize, height: f32) {
        let (_, content_h) = self.content_band(height);
        let viewport = (content_h - HEADER_H).max(0.0);
        let row_top = index as f32 * ROW_H;
        let row_bottom = row_top + ROW_H;
        if row_top < self.list_scroll_y {
            self.list_scroll_y = row_top;
        } else if row_bottom > self.list_scroll_y + viewport {
            self.list_scroll_y = row_bottom - viewport;
        }
        self.list_scroll_y = self.list_scroll_y.clamp(0.0, self.max_list_scroll(height));
    }

    /// Move the keyboard cursor `delta` rows through the visible entries.
    ///
    /// `hovered_entry` is the cursor. Giving the keyboard its own second
    /// notion of "the current row" would mean two highlights that can point at
    /// different rows, and the user has no way to tell which one Enter uses.
    fn move_cursor(&mut self, delta: isize, height: f32) -> Action {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return Action::None;
        }
        let last = ids.len().saturating_sub(1);
        let next = match self
            .hovered_entry
            .and_then(|id| ids.iter().position(|i| *i == id))
        {
            // With no cursor yet, Down starts at the top and Up at the bottom,
            // so the first keypress always lands somewhere visible.
            None => {
                if delta < 0 {
                    last
                } else {
                    0
                }
            }
            Some(here) => {
                let moved = isize::try_from(here).unwrap_or(0).saturating_add(delta);
                usize::try_from(moved.max(0)).unwrap_or(0)
            }
        };
        self.set_cursor(next, height)
    }

    /// Put the keyboard cursor on visible row `index`, clamped to the list.
    ///
    /// Home and End name a row directly rather than moving by a delta large
    /// enough to overshoot: a delta of `isize::MIN` cannot even be negated,
    /// and "move very far" is a worse way to say "go to the first row".
    fn set_cursor(&mut self, index: usize, height: f32) -> Action {
        let ids = self.visible_ids();
        let index = index.min(ids.len().saturating_sub(1));
        let Some(id) = ids.get(index) else {
            return Action::None;
        };
        if self.hovered_entry == Some(*id) {
            return Action::None;
        }
        self.hovered_entry = Some(*id);
        self.reveal_row(index, height);
        Action::Redraw
    }

    /// Open the entry under the cursor: a directory is navigated into, a file
    /// has nothing to open into yet and says so.
    fn activate_cursor(&mut self) -> Action {
        let Some(id) = self.hovered_entry else {
            return Action::None;
        };
        self.open_entry(id)
    }

    /// Double-click / Enter behaviour for the entry with the given id.
    fn open_entry(&mut self, id: u64) -> Action {
        let Some(archive) = &self.archive else {
            return Action::None;
        };
        let Some(entry) = archive.entries.iter().find(|e| e.id == id) else {
            return Action::None;
        };
        if entry.is_dir {
            let path = entry.path.clone();
            self.navigate_to(&path);
            self.status_message = format!("Entered {path}");
            Action::Redraw
        } else {
            // Extracting one file to a temporary directory and handing it to
            // whatever opens that type is what this should do; there is no
            // extractor and no file-type launcher yet, so it names the file
            // rather than pretending to have opened it. See known-issues
            // `C-ARCHIVEMANAGER-CANNOT-ACTUALLY-READ-AN-ARCHIVE`.
            self.status_message = format!("{} — no viewer to open it with yet", entry.name);
            Action::Redraw
        }
    }

    /// Toggle the selection of one entry.
    fn toggle_entry(&mut self, id: u64) -> Action {
        let Some(archive) = &mut self.archive else {
            return Action::None;
        };
        archive.toggle_selection(id);
        self.hovered_entry = Some(id);
        Action::Redraw
    }

    /// Run a toolbar button.
    fn run_toolbar(&mut self, action: ToolbarAction) -> Action {
        if !toolbar_enabled(self, action) {
            // Saying *why* it did nothing beats a click that vanishes: the
            // difference between "this button is not for now" and "this
            // program has stopped responding" is otherwise invisible.
            self.status_message = format!(
                "{} is unavailable — {}",
                action.label(),
                match action {
                    ToolbarAction::ExtractSelected | ToolbarAction::Delete => "nothing is selected",
                    _ => "no archive is open",
                }
            );
            return Action::Redraw;
        }
        match action {
            ToolbarAction::Delete => {
                let paths: Vec<String> = self
                    .archive
                    .as_ref()
                    .map(|a| {
                        a.selected_entries()
                            .iter()
                            .map(|e| e.path.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let count = paths.len();
                if let Some(archive) = &mut self.archive {
                    archive.remove_entries(&paths);
                }
                // A removed entry cannot stay the cursor.
                self.hovered_entry = None;
                self.status_message =
                    format!("Removed {count} entr{} from the list", plural(count));
                Action::Redraw
            }
            // Everything else needs a back end that can read and write the
            // archive bytes, which does not exist yet. This reports the
            // request instead of silently dropping it.
            other => {
                self.status_message = format!(
                    "{}: not yet implemented — no archive back end",
                    other.label()
                );
                Action::Redraw
            }
        }
    }

    /// Perform whatever `target` names.
    pub fn activate(&mut self, target: Target, size: (f32, f32)) -> Action {
        match target {
            Target::Toolbar(action) => self.run_toolbar(action),
            Target::NavBack => {
                if self.navigate_back() {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Target::NavForward => {
                if self.navigate_forward() {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Target::NavUp => {
                if self.navigate_up() {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Target::TreeRow(i) => {
                let Some(row) = self.tree_rows().get(i).cloned() else {
                    return Action::None;
                };
                self.navigate_to(&row.path);
                Action::Redraw
            }
            Target::TreeArrow(i) => {
                let Some(row) = self.tree_rows().get(i).cloned() else {
                    return Action::None;
                };
                let Some(archive) = &mut self.archive else {
                    return Action::None;
                };
                if archive.tree.toggle_path(&row.path) {
                    // Collapsing shortens the list, which can leave the scroll
                    // offset past the end and the tree apparently empty.
                    self.tree_scroll_y =
                        self.tree_scroll_y.clamp(0.0, self.max_tree_scroll(size.1));
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Target::ColumnHeader(col) => {
                self.toggle_sort(col);
                Action::Redraw
            }
            Target::FileRow(id) => self.toggle_entry(id),
        }
    }

    /// Route a mouse click.
    pub fn handle_click(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        if button != MouseButton::Left {
            return Action::None;
        }
        match self.hit_test(x, y, size) {
            Some(target) => self.activate(target, size),
            // A click on bare background is not a mystery to report; it just
            // did not land on anything.
            None => Action::None,
        }
    }

    /// Route a double-click, which opens rather than selects.
    pub fn handle_double_click(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        if button != MouseButton::Left {
            return Action::None;
        }
        match self.hit_test(x, y, size) {
            Some(Target::FileRow(id)) => self.open_entry(id),
            // Anywhere else a double-click is two clicks, and the second one
            // should do what the first did rather than nothing.
            Some(target) => self.activate(target, size),
            None => Action::None,
        }
    }

    /// Route a key press.
    pub fn handle_key(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        if !key.pressed {
            // A key *release* must not repeat the action of its press.
            return Action::None;
        }
        let (_, content_h) = self.content_band(size.1);
        // A page is what the viewport can show, so Page Down lands on the row
        // that was at the bottom — the reader keeps one row of context rather
        // than jumping into text they have never seen.
        #[allow(clippy::cast_possible_truncation)]
        let page = (((content_h - HEADER_H) / ROW_H).max(1.0) as isize).max(1);
        match key.key {
            Key::Up => self.move_cursor(-1, size.1),
            Key::Down => self.move_cursor(1, size.1),
            Key::PageUp => self.move_cursor(page.saturating_neg(), size.1),
            Key::PageDown => self.move_cursor(page, size.1),
            Key::Home => self.set_cursor(0, size.1),
            Key::End => self.set_cursor(usize::MAX, size.1),
            Key::Enter => self.activate_cursor(),
            Key::Space => match self.hovered_entry {
                Some(id) => self.toggle_entry(id),
                None => Action::None,
            },
            Key::Backspace => {
                if self.navigate_up() {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Key::Delete => self.run_toolbar(ToolbarAction::Delete),
            Key::A if key.modifiers.ctrl => {
                let Some(archive) = &mut self.archive else {
                    return Action::None;
                };
                archive.select_all();
                Action::Redraw
            }
            Key::B if key.modifiers.ctrl => {
                self.sidebar_visible = !self.sidebar_visible;
                Action::Redraw
            }
            Key::Escape => {
                // Escape backs out of the smallest thing first. Closing the
                // window while a selection is up would throw away work the
                // user could not see they still had.
                let had_selection = self
                    .archive
                    .as_ref()
                    .is_some_and(|a| a.entries.iter().any(|e| e.selected));
                if had_selection {
                    if let Some(archive) = &mut self.archive {
                        archive.deselect_all();
                    }
                    Action::Redraw
                } else {
                    Action::Quit
                }
            }
            _ => Action::None,
        }
    }

    /// Route a wheel event to whichever pane the pointer is over.
    fn handle_scroll(&mut self, mouse: &MouseEvent, dy: f32, size: (f32, f32)) -> Action {
        let rows = self.wheel.rows(dy);
        if rows == 0 {
            // A trackpad's fractions accumulate inside `wheel` until they add
            // up to a row; asking for a repaint on every one of them would
            // repaint the window several times per finger-millimetre.
            return Action::None;
        }
        #[allow(clippy::cast_precision_loss)]
        let delta = rows as f32 * ROW_H;
        let over_sidebar = self.sidebar_visible && mouse.x < self.sidebar_width;
        if over_sidebar {
            let max = self.max_tree_scroll(size.1);
            let next = (self.tree_scroll_y + delta).clamp(0.0, max);
            if (next - self.tree_scroll_y).abs() < f32::EPSILON {
                return Action::None;
            }
            self.tree_scroll_y = next;
        } else {
            let max = self.max_list_scroll(size.1);
            let next = (self.list_scroll_y + delta).clamp(0.0, max);
            if (next - self.list_scroll_y).abs() < f32::EPSILON {
                return Action::None;
            }
            self.list_scroll_y = next;
        }
        Action::Redraw
    }

    /// Track the pointer so the row under it lights up.
    fn handle_move(&mut self, mouse: &MouseEvent, size: (f32, f32)) -> Action {
        let under = match self.hit_test(mouse.x, mouse.y, size) {
            Some(Target::FileRow(id)) => Some(id),
            _ => None,
        };
        if self.hovered_entry == under {
            return Action::None;
        }
        self.hovered_entry = under;
        Action::Redraw
    }

    /// Route a whole event.
    pub fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::DoubleClick(button) => {
                    self.handle_double_click(mouse.x, mouse.y, button, size)
                }
                MouseEventKind::Move => self.handle_move(mouse, size),
                MouseEventKind::Scroll { dy, .. } => self.handle_scroll(mouse, dy, size),
                MouseEventKind::Leave => {
                    if self.hovered_entry.is_none() {
                        return Action::None;
                    }
                    self.hovered_entry = None;
                    Action::Redraw
                }
                MouseEventKind::Release(_) | MouseEventKind::Enter => Action::None,
            },
            Event::Key(key) => self.handle_key(key, size),
            Event::CloseRequested => Action::Quit,
            _ => Action::None,
        }
    }
}

/// `""` or `"ies"`, so a count of one does not read "1 entries".
fn plural(count: usize) -> &'static str {
    if count == 1 { "y" } else { "ies" }
}

// ============================================================================
// Sample / demo data
// ============================================================================

/// Create a sample archive for demonstration/testing.
pub fn create_sample_archive() -> ArchiveModel {
    let path = PathBuf::from("/home/user/project.zip");
    let mut archive = ArchiveModel::new(&path, ArchiveFormat::Zip);

    let sample_entries = vec![
        ("src/", true, 0, 0, 1716000000, 0, "Stored"),
        (
            "src/main.rs",
            false,
            4096,
            1820,
            1716000000,
            0xABCD1234,
            "Deflate",
        ),
        (
            "src/lib.rs",
            false,
            8192,
            3100,
            1716000000,
            0x12345678,
            "Deflate",
        ),
        ("src/utils/", true, 0, 0, 1716000000, 0, "Stored"),
        (
            "src/utils/helpers.rs",
            false,
            2048,
            980,
            1716000000,
            0xDEADBEEF,
            "Deflate",
        ),
        ("tests/", true, 0, 0, 1716000000, 0, "Stored"),
        (
            "tests/test_main.rs",
            false,
            1024,
            620,
            1716000000,
            0xFEEDFACE,
            "Deflate",
        ),
        (
            "Cargo.toml",
            false,
            512,
            380,
            1716000000,
            0xCAFEBABE,
            "Deflate",
        ),
        (
            "README.md",
            false,
            3072,
            1400,
            1716000000,
            0x87654321,
            "Deflate",
        ),
        (
            "LICENSE", false, 1070, 640, 1716000000, 0x11223344, "Deflate",
        ),
        ("docs/", true, 0, 0, 1716000000, 0, "Stored"),
        (
            "docs/guide.md",
            false,
            15360,
            5200,
            1716000000,
            0xAABBCCDD,
            "Deflate",
        ),
        (
            "docs/api.md",
            false,
            8700,
            3100,
            1716000000,
            0x55667788,
            "Deflate",
        ),
    ];

    for (path_str, is_dir, size, compressed, modified, crc, method) in sample_entries {
        let name = path_str
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(path_str)
            .to_string();
        archive.add_entry(ArchiveEntry {
            path: path_str.trim_end_matches('/').to_string(),
            name,
            is_dir,
            size,
            compressed_size: compressed,
            modified,
            crc32: crc,
            encrypted: false,
            method: method.to_string(),
            depth: path_str.matches('/').count() as u32,
            expanded: false,
            selected: false,
            id: 0, // assigned by add_entry
        });
    }

    archive.rebuild_tree();
    archive
}

// ============================================================================
// Entry point
// ============================================================================

impl App for AppState {
    fn title(&self) -> String {
        match &self.archive {
            Some(archive) => format!(
                "{} — Archive Manager",
                archive.path.file_name().map_or_else(
                    || archive.path.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                )
            ),
            None => String::from("Archive Manager"),
        }
    }

    fn app_id(&self) -> String {
        String::from("os.slate.archivemanager")
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // Resize is handled here rather than in `handle_event` because it is
        // the one event whose *answer* is the new size: every other handler
        // needs the size as an input, so the field must already be updated.
        if let Event::Resize { width, height } = *event {
            #[allow(clippy::cast_precision_loss)]
            let (w, h) = (width as f32, height as f32);
            if (w - self.window_width).abs() < f32::EPSILON
                && (h - self.window_height).abs() < f32::EPSILON
            {
                return Response::Idle;
            }
            self.window_width = w;
            self.window_height = h;
            // A window that got shorter can leave both panes scrolled past
            // their new ends, showing blank space with no way back except
            // scrolling up blindly.
            self.list_scroll_y = self.list_scroll_y.min(self.max_list_scroll(h));
            self.tree_scroll_y = self.tree_scroll_y.min(self.max_tree_scroll(h));
            return Response::Redraw;
        }
        let size = (self.window_width, self.window_height);
        match self.handle_event(event, size) {
            Action::None => Response::Idle,
            Action::Redraw => Response::Redraw,
            Action::Quit => Response::Exit,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Believe the size we are handed: the first frame is drawn before any
        // `Resize` arrives, so trusting the stored size instead would draw the
        // opening frame at the default size whatever the window really is.
        self.window_width = width;
        self.window_height = height;
        build_frame(self, width, height).tree
    }
}

fn main() -> ExitCode {
    let mut state = AppState {
        archive: Some(create_sample_archive()),
        ..AppState::default()
    };
    state.status_message = state.status_text();
    app::launch("archivemanager", &mut state)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is
    // a failure report, and an index that is out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use guitk::event::Modifiers;

    // --- ArchiveFormat tests ---

    #[test]
    fn test_format_extension() {
        assert_eq!(ArchiveFormat::Zip.extension(), ".zip");
        assert_eq!(ArchiveFormat::Tar.extension(), ".tar");
        assert_eq!(ArchiveFormat::TarGz.extension(), ".tar.gz");
        assert_eq!(ArchiveFormat::TarBz2.extension(), ".tar.bz2");
        assert_eq!(ArchiveFormat::SevenZip.extension(), ".7z");
    }

    #[test]
    fn test_format_display_name() {
        assert_eq!(ArchiveFormat::Zip.display_name(), "ZIP Archive");
        assert_eq!(ArchiveFormat::SevenZip.display_name(), "7-Zip Archive");
    }

    #[test]
    fn test_format_supports_encryption() {
        assert!(ArchiveFormat::Zip.supports_encryption());
        assert!(ArchiveFormat::SevenZip.supports_encryption());
        assert!(!ArchiveFormat::Tar.supports_encryption());
        assert!(!ArchiveFormat::TarGz.supports_encryption());
        assert!(!ArchiveFormat::TarBz2.supports_encryption());
    }

    #[test]
    fn test_format_supports_split() {
        assert!(ArchiveFormat::Zip.supports_split());
        assert!(ArchiveFormat::SevenZip.supports_split());
        assert!(!ArchiveFormat::Tar.supports_split());
    }

    #[test]
    fn test_format_supports_per_file_compression() {
        assert!(ArchiveFormat::Zip.supports_per_file_compression());
        assert!(ArchiveFormat::SevenZip.supports_per_file_compression());
        assert!(!ArchiveFormat::Tar.supports_per_file_compression());
    }

    #[test]
    fn test_format_from_path_zip() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("archive.zip")),
            Some(ArchiveFormat::Zip)
        );
    }

    #[test]
    fn test_format_from_path_tar() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("backup.tar")),
            Some(ArchiveFormat::Tar)
        );
    }

    #[test]
    fn test_format_from_path_tar_gz() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("data.tar.gz")),
            Some(ArchiveFormat::TarGz)
        );
    }

    #[test]
    fn test_format_from_path_tgz() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("data.tgz")),
            Some(ArchiveFormat::TarGz)
        );
    }

    #[test]
    fn test_format_from_path_tar_bz2() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("data.tar.bz2")),
            Some(ArchiveFormat::TarBz2)
        );
    }

    #[test]
    fn test_format_from_path_tbz2() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("data.tbz2")),
            Some(ArchiveFormat::TarBz2)
        );
    }

    #[test]
    fn test_format_from_path_7z() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("archive.7z")),
            Some(ArchiveFormat::SevenZip)
        );
    }

    #[test]
    fn test_format_from_path_unknown() {
        assert_eq!(ArchiveFormat::from_path(Path::new("file.txt")), None);
    }

    #[test]
    fn test_format_from_path_case_insensitive() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("FILE.ZIP")),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("backup.TAR.GZ")),
            Some(ArchiveFormat::TarGz)
        );
    }

    #[test]
    fn test_format_all() {
        let all = ArchiveFormat::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&ArchiveFormat::Zip));
        assert!(all.contains(&ArchiveFormat::SevenZip));
    }

    // --- CompressionLevel tests ---

    #[test]
    fn test_compression_level_numeric() {
        assert_eq!(CompressionLevel::Store.numeric_level(), 0);
        assert_eq!(CompressionLevel::Fast.numeric_level(), 3);
        assert_eq!(CompressionLevel::Normal.numeric_level(), 6);
        assert_eq!(CompressionLevel::Best.numeric_level(), 9);
    }

    #[test]
    fn test_compression_level_default() {
        assert_eq!(CompressionLevel::default(), CompressionLevel::Normal);
    }

    #[test]
    fn test_compression_level_all() {
        let all = CompressionLevel::all();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_compression_level_display_name() {
        assert!(CompressionLevel::Store.display_name().contains("Store"));
        assert!(CompressionLevel::Best.display_name().contains("Best"));
    }

    // --- EncryptionSettings tests ---

    #[test]
    fn test_encryption_default_disabled() {
        let enc = EncryptionSettings::default();
        assert!(!enc.is_enabled());
    }

    #[test]
    fn test_encryption_enabled_with_password() {
        let enc = EncryptionSettings {
            password: "secret".into(),
            ..Default::default()
        };
        assert!(enc.is_enabled());
    }

    #[test]
    fn test_encryption_method_display() {
        assert_eq!(EncryptionMethod::Aes256.display_name(), "AES-256");
        assert!(
            EncryptionMethod::ZipCrypto
                .display_name()
                .contains("legacy")
        );
    }

    // --- SplitSettings tests ---

    #[test]
    fn test_split_default_disabled() {
        let split = SplitSettings::default();
        assert!(!split.enabled);
        assert_eq!(split.volume_size, 700 * 1024 * 1024);
    }

    #[test]
    fn test_split_presets_nonempty() {
        let presets = SplitSettings::presets();
        assert!(!presets.is_empty());
        // First preset should be floppy size.
        assert_eq!(presets[0].1, 1_440 * 1024);
    }

    // --- ArchiveEntry tests ---

    #[test]
    fn test_entry_compression_ratio_normal() {
        let entry = ArchiveEntry {
            path: "test.txt".into(),
            name: "test.txt".into(),
            is_dir: false,
            size: 1000,
            compressed_size: 600,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: "Deflate".into(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 1,
        };
        let ratio = entry.compression_ratio();
        assert!((ratio - 40.0).abs() < 0.1);
    }

    #[test]
    fn test_entry_compression_ratio_zero_size() {
        let entry = ArchiveEntry {
            path: "empty".into(),
            name: "empty".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: "Store".into(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 1,
        };
        assert_eq!(entry.compression_ratio(), 0.0);
    }

    #[test]
    fn test_entry_compression_ratio_clamped() {
        // compressed_size > size should clamp to 0%.
        let entry = ArchiveEntry {
            path: "bad".into(),
            name: "bad".into(),
            is_dir: false,
            size: 100,
            compressed_size: 200,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: "Store".into(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 1,
        };
        assert_eq!(entry.compression_ratio(), 0.0);
    }

    #[test]
    fn test_entry_format_size_bytes() {
        assert_eq!(ArchiveEntry::format_size(42), "42 B");
    }

    #[test]
    fn test_entry_format_size_kb() {
        let s = ArchiveEntry::format_size(2048);
        assert!(s.contains("KiB"));
    }

    #[test]
    fn test_entry_format_size_mb() {
        let s = ArchiveEntry::format_size(5 * 1024 * 1024);
        assert!(s.contains("MiB"));
    }

    #[test]
    fn test_entry_format_size_gb() {
        let s = ArchiveEntry::format_size(3 * 1024 * 1024 * 1024);
        assert!(s.contains("GiB"));
    }

    #[test]
    fn test_entry_format_crc() {
        assert_eq!(ArchiveEntry::format_crc(0xDEADBEEF), "DEADBEEF");
        assert_eq!(ArchiveEntry::format_crc(0x00000001), "00000001");
    }

    #[test]
    fn test_entry_format_date_zero() {
        assert_eq!(ArchiveEntry::format_date(0), "-");
    }

    /// Asserted by value, not by punctuation.
    ///
    /// The assertions this replaces were `contains('-')` and `contains(':')`,
    /// which "2026-13-40 25:99" also satisfies. A test that only checks the
    /// separators cannot notice a wrong calendar, and a wrong calendar is
    /// what four sibling programs' copies of this arithmetic turned out to
    /// hold.
    #[test]
    fn test_entry_format_date_nonzero() {
        // 2024-05-18 02:40:00 UTC.
        assert_eq!(ArchiveEntry::format_date(1_716_000_000), "2024-05-18 02:40");
    }

    #[test]
    fn test_entry_parent_path_root() {
        let entry = ArchiveEntry {
            path: "file.txt".into(),
            name: "file.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 1,
        };
        assert_eq!(entry.parent_path(), "");
    }

    #[test]
    fn test_entry_parent_path_nested() {
        let entry = ArchiveEntry {
            path: "src/utils/helpers.rs".into(),
            name: "helpers.rs".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 2,
            expanded: false,
            selected: false,
            id: 1,
        };
        assert_eq!(entry.parent_path(), "src/utils");
    }

    // --- Column tests ---

    #[test]
    fn test_column_headers() {
        assert_eq!(Column::Name.header(), "Name");
        assert_eq!(Column::Size.header(), "Size");
        assert_eq!(Column::CompressedSize.header(), "Packed");
        assert_eq!(Column::Ratio.header(), "Ratio");
        assert_eq!(Column::Crc.header(), "CRC-32");
    }

    #[test]
    fn test_column_default_widths_positive() {
        for col in Column::all() {
            assert!(col.default_width() > 0.0);
        }
    }

    #[test]
    fn test_column_all_count() {
        assert_eq!(Column::all().len(), 7);
    }

    // --- SortState tests ---

    #[test]
    fn test_sort_direction_toggle() {
        assert_eq!(SortDirection::Ascending.toggle(), SortDirection::Descending);
        assert_eq!(SortDirection::Descending.toggle(), SortDirection::Ascending);
    }

    #[test]
    fn test_sort_direction_indicator() {
        assert!(SortDirection::Ascending.indicator().contains('^'));
        assert!(SortDirection::Descending.indicator().contains('v'));
    }

    #[test]
    fn test_sort_state_default() {
        let s = SortState::default();
        assert_eq!(s.column, Column::Name);
        assert_eq!(s.direction, SortDirection::Ascending);
    }

    // --- TreeNode tests ---

    #[test]
    fn test_tree_node_new() {
        let node = TreeNode::new("src", "src");
        assert_eq!(node.name, "src");
        assert_eq!(node.path, "src");
        assert!(!node.expanded);
        assert!(node.children.is_empty());
    }

    #[test]
    fn test_tree_node_toggle() {
        let mut node = TreeNode::new("a", "a");
        assert!(!node.expanded);
        node.toggle();
        assert!(node.expanded);
        node.toggle();
        assert!(!node.expanded);
    }

    #[test]
    fn test_tree_node_get_or_create_child() {
        let mut root = TreeNode::new("root", "");
        root.get_or_create_child("src", "src");
        assert_eq!(root.children.len(), 1);
        // Getting the same child should not create a duplicate.
        root.get_or_create_child("src", "src");
        assert_eq!(root.children.len(), 1);
        // Different child.
        root.get_or_create_child("docs", "docs");
        assert_eq!(root.children.len(), 2);
    }

    #[test]
    fn test_tree_node_total_descendants() {
        let mut root = TreeNode::new("root", "");
        let src = root.get_or_create_child("src", "src");
        src.get_or_create_child("utils", "src/utils");
        root.get_or_create_child("docs", "docs");
        // root has 2 children (src, docs), src has 1 child (utils) = 3 total.
        assert_eq!(root.total_descendants(), 3);
    }

    #[test]
    fn test_tree_node_flatten_collapsed() {
        let mut root = TreeNode::new("root", "");
        root.get_or_create_child("src", "src");
        root.get_or_create_child("docs", "docs");
        // root is not expanded, so only root is shown.
        let mut flat = Vec::new();
        root.flatten(0, &mut flat);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "root");
    }

    #[test]
    fn test_tree_node_flatten_expanded() {
        let mut root = TreeNode::new("root", "");
        root.expanded = true;
        root.get_or_create_child("src", "src");
        root.get_or_create_child("docs", "docs");
        let mut flat = Vec::new();
        root.flatten(0, &mut flat);
        // root + 2 children = 3.
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[1].depth, 1);
    }

    #[test]
    fn test_tree_node_flatten_nested_expanded() {
        let mut root = TreeNode::new("root", "");
        root.expanded = true;
        {
            let src = root.get_or_create_child("src", "src");
            src.expanded = true;
            src.get_or_create_child("utils", "src/utils");
        }
        let mut flat = Vec::new();
        root.flatten(0, &mut flat);
        // root + src + utils = 3.
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[2].depth, 2);
        assert_eq!(flat[2].name, "utils");
    }

    // --- build_directory_tree tests ---

    #[test]
    fn test_build_tree_empty() {
        let tree = build_directory_tree(&[], "test.zip");
        assert_eq!(tree.name, "test.zip");
        assert!(tree.children.is_empty());
    }

    #[test]
    fn test_build_tree_single_file() {
        let entries = vec![ArchiveEntry {
            path: "readme.md".into(),
            name: "readme.md".into(),
            is_dir: false,
            size: 100,
            compressed_size: 80,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 1,
        }];
        let tree = build_directory_tree(&entries, "test.zip");
        assert_eq!(tree.file_count, 1);
        assert_eq!(tree.total_size, 100);
    }

    #[test]
    fn test_build_tree_nested_files() {
        let entries = vec![
            ArchiveEntry {
                path: "src".into(),
                name: "src".into(),
                is_dir: true,
                size: 0,
                compressed_size: 0,
                modified: 0,
                crc32: 0,
                encrypted: false,
                method: String::new(),
                depth: 0,
                expanded: false,
                selected: false,
                id: 1,
            },
            ArchiveEntry {
                path: "src/main.rs".into(),
                name: "main.rs".into(),
                is_dir: false,
                size: 500,
                compressed_size: 300,
                modified: 0,
                crc32: 0,
                encrypted: false,
                method: String::new(),
                depth: 1,
                expanded: false,
                selected: false,
                id: 2,
            },
        ];
        let tree = build_directory_tree(&entries, "test.zip");
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].name, "src");
        assert_eq!(tree.children[0].file_count, 1);
    }

    // --- OperationProgress tests ---

    #[test]
    fn test_progress_new() {
        let p = OperationProgress::new("Extract", 10, 1000);
        assert_eq!(p.files_total, 10);
        assert_eq!(p.bytes_total, 1000);
        assert!(!p.completed);
        assert!(p.is_running());
    }

    #[test]
    fn test_progress_percent_by_bytes() {
        let mut p = OperationProgress::new("Extract", 10, 1000);
        p.bytes_done = 500;
        assert!((p.percent() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_progress_percent_by_files_when_no_bytes() {
        let mut p = OperationProgress::new("Test", 4, 0);
        p.files_done = 2;
        assert!((p.percent() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_progress_percent_empty() {
        let p = OperationProgress::new("Nothing", 0, 0);
        assert_eq!(p.percent(), 100.0);
    }

    #[test]
    fn test_progress_advance_file() {
        let mut p = OperationProgress::new("Extract", 3, 300);
        p.advance_file("a.txt", 100);
        assert_eq!(p.files_done, 1);
        assert_eq!(p.bytes_done, 100);
        assert_eq!(p.current_file, "a.txt");
    }

    #[test]
    fn test_progress_finish() {
        let mut p = OperationProgress::new("Extract", 3, 300);
        p.finish();
        assert!(p.completed);
        assert!(!p.is_running());
        assert_eq!(p.files_done, 3);
    }

    #[test]
    fn test_progress_fail() {
        let mut p = OperationProgress::new("Extract", 3, 300);
        p.fail("Disk full");
        assert!(p.completed);
        assert_eq!(p.error.as_deref(), Some("Disk full"));
    }

    // --- DragState tests ---

    #[test]
    fn test_drag_idle() {
        let d = DragState::Idle;
        assert!(!d.is_active());
        assert_eq!(d.item_count(), 0);
    }

    #[test]
    fn test_drag_out() {
        let d = DragState::DraggingOut {
            entries: vec!["a.txt".into(), "b.txt".into()],
            mouse_x: 100.0,
            mouse_y: 200.0,
        };
        assert!(d.is_active());
        assert_eq!(d.item_count(), 2);
    }

    #[test]
    fn test_drag_in() {
        let d = DragState::DraggingIn {
            files: vec![PathBuf::from("/tmp/x.txt")],
            mouse_x: 0.0,
            mouse_y: 0.0,
        };
        assert!(d.is_active());
        assert_eq!(d.item_count(), 1);
    }

    // --- TestResult tests ---

    #[test]
    fn test_result_ok_display() {
        assert_eq!(TestResult::Ok.display_text(), "OK");
    }

    #[test]
    fn test_result_crc_mismatch_display() {
        let r = TestResult::CrcMismatch {
            expected: 0,
            actual: 1,
        };
        assert_eq!(r.display_text(), "CRC Error");
    }

    #[test]
    fn test_result_colors() {
        assert_eq!(TestResult::Ok.display_color(), theme::GREEN);
        assert_eq!(TestResult::Pending.display_color(), theme::SUBTEXT0);
        assert_eq!(TestResult::DecryptionFailed.display_color(), theme::RED);
    }

    // --- ArchiveTestResults tests ---

    #[test]
    fn test_archive_test_results_new() {
        let r = ArchiveTestResults::new(10);
        assert_eq!(r.total_entries, 10);
        assert_eq!(r.tested, 0);
        assert_eq!(r.pass_rate(), 0.0);
    }

    #[test]
    fn test_archive_test_results_record() {
        let mut r = ArchiveTestResults::new(3);
        r.record("a.txt", TestResult::Ok);
        r.record("b.txt", TestResult::Ok);
        r.record(
            "c.txt",
            TestResult::CrcMismatch {
                expected: 0,
                actual: 1,
            },
        );
        assert_eq!(r.tested, 3);
        assert_eq!(r.passed, 2);
        assert_eq!(r.failed, 1);
        assert!(!r.all_passed());
        assert!((r.pass_rate() - 66.666).abs() < 1.0);
    }

    #[test]
    fn test_archive_test_results_all_passed() {
        let mut r = ArchiveTestResults::new(2);
        r.record("a.txt", TestResult::Ok);
        r.record("b.txt", TestResult::Ok);
        assert!(r.all_passed());
        assert_eq!(r.pass_rate(), 100.0);
    }

    // --- ArchiveModel tests ---

    #[test]
    fn test_archive_model_new() {
        let m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        assert_eq!(m.format, ArchiveFormat::Zip);
        assert!(m.entries.is_empty());
        assert_eq!(m.file_count, 0);
    }

    #[test]
    fn test_archive_model_add_entry() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "a.txt".into(),
            name: "a.txt".into(),
            is_dir: false,
            size: 100,
            compressed_size: 50,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: "Deflate".into(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        assert_eq!(m.file_count, 1);
        assert_eq!(m.total_size, 100);
        assert_eq!(m.total_compressed, 50);
        // id should have been assigned.
        assert_eq!(m.entries[0].id, 1);
    }

    #[test]
    fn test_archive_model_add_dir() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "src".into(),
            name: "src".into(),
            is_dir: true,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: "Stored".into(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        assert_eq!(m.dir_count, 1);
        assert_eq!(m.file_count, 0);
    }

    #[test]
    fn test_archive_model_overall_ratio() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "a.txt".into(),
            name: "a.txt".into(),
            is_dir: false,
            size: 1000,
            compressed_size: 400,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        assert!((m.overall_ratio() - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_archive_model_overall_ratio_empty() {
        let m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        assert_eq!(m.overall_ratio(), 0.0);
    }

    #[test]
    fn test_archive_model_select_deselect() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "a.txt".into(),
            name: "a.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        m.add_entry(ArchiveEntry {
            path: "b.txt".into(),
            name: "b.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        assert_eq!(m.selected_entries().len(), 0);
        m.select_all();
        assert_eq!(m.selected_entries().len(), 2);
        m.deselect_all();
        assert_eq!(m.selected_entries().len(), 0);
    }

    #[test]
    fn test_archive_model_toggle_selection() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "a.txt".into(),
            name: "a.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        let id = m.entries[0].id;
        m.toggle_selection(id);
        assert!(m.entries[0].selected);
        m.toggle_selection(id);
        assert!(!m.entries[0].selected);
    }

    #[test]
    fn test_archive_model_remove_entries() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "a.txt".into(),
            name: "a.txt".into(),
            is_dir: false,
            size: 100,
            compressed_size: 50,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        m.add_entry(ArchiveEntry {
            path: "b.txt".into(),
            name: "b.txt".into(),
            is_dir: false,
            size: 200,
            compressed_size: 100,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        m.remove_entries(&["a.txt".to_string()]);
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.entries[0].path, "b.txt");
        assert_eq!(m.file_count, 1);
        assert_eq!(m.total_size, 200);
    }

    #[test]
    fn test_archive_model_sort_by_name() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "c.txt".into(),
            name: "c.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        m.add_entry(ArchiveEntry {
            path: "a.txt".into(),
            name: "a.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        let sort = SortState {
            column: Column::Name,
            direction: SortDirection::Ascending,
        };
        m.sort_entries(&sort);
        assert_eq!(m.entries[0].name, "a.txt");
        assert_eq!(m.entries[1].name, "c.txt");
    }

    #[test]
    fn test_archive_model_sort_by_size_desc() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "small.txt".into(),
            name: "small.txt".into(),
            is_dir: false,
            size: 10,
            compressed_size: 5,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        m.add_entry(ArchiveEntry {
            path: "big.txt".into(),
            name: "big.txt".into(),
            is_dir: false,
            size: 9999,
            compressed_size: 5000,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        let sort = SortState {
            column: Column::Size,
            direction: SortDirection::Descending,
        };
        m.sort_entries(&sort);
        assert_eq!(m.entries[0].name, "big.txt");
    }

    #[test]
    fn test_archive_model_sort_dirs_before_files() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "z_file.txt".into(),
            name: "z_file.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        m.add_entry(ArchiveEntry {
            path: "a_dir".into(),
            name: "a_dir".into(),
            is_dir: true,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        let sort = SortState::default();
        m.sort_entries(&sort);
        assert!(m.entries[0].is_dir, "directory should sort first");
    }

    #[test]
    fn test_archive_model_entries_in_directory() {
        let mut m = ArchiveModel::new(Path::new("test.zip"), ArchiveFormat::Zip);
        m.add_entry(ArchiveEntry {
            path: "root.txt".into(),
            name: "root.txt".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 0,
            expanded: false,
            selected: false,
            id: 0,
        });
        m.add_entry(ArchiveEntry {
            path: "src/main.rs".into(),
            name: "main.rs".into(),
            is_dir: false,
            size: 0,
            compressed_size: 0,
            modified: 0,
            crc32: 0,
            encrypted: false,
            method: String::new(),
            depth: 1,
            expanded: false,
            selected: false,
            id: 0,
        });
        let root_entries = m.entries_in_directory("");
        assert_eq!(root_entries.len(), 1);
        assert_eq!(root_entries[0].name, "root.txt");
        let src_entries = m.entries_in_directory("src");
        assert_eq!(src_entries.len(), 1);
        assert_eq!(src_entries[0].name, "main.rs");
    }

    // --- CreateArchiveSettings tests ---

    #[test]
    fn test_create_settings_validate_empty() {
        let s = CreateArchiveSettings::default();
        let problems = s.validate();
        assert!(problems.iter().any(|p| p.contains("Output path")));
        assert!(problems.iter().any(|p| p.contains("No source")));
    }

    #[test]
    fn test_create_settings_validate_ok() {
        let s = CreateArchiveSettings {
            output_path: PathBuf::from("out.zip"),
            sources: vec![PathBuf::from("file.txt")],
            ..Default::default()
        };
        let problems = s.validate();
        assert!(
            problems.is_empty(),
            "expected no problems, got: {problems:?}"
        );
    }

    #[test]
    fn test_create_settings_validate_encryption_unsupported() {
        let s = CreateArchiveSettings {
            output_path: PathBuf::from("out.tar"),
            format: ArchiveFormat::Tar,
            sources: vec![PathBuf::from("file.txt")],
            encryption: EncryptionSettings {
                password: "secret".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let problems = s.validate();
        assert!(problems.iter().any(|p| p.contains("encryption")));
    }

    #[test]
    fn test_create_settings_validate_split_unsupported() {
        let s = CreateArchiveSettings {
            output_path: PathBuf::from("out.tar.gz"),
            format: ArchiveFormat::TarGz,
            sources: vec![PathBuf::from("file.txt")],
            split: SplitSettings {
                enabled: true,
                volume_size: 1_000_000,
            },
            ..Default::default()
        };
        let problems = s.validate();
        assert!(problems.iter().any(|p| p.contains("split")));
    }

    #[test]
    fn test_create_settings_validate_split_too_small() {
        let s = CreateArchiveSettings {
            output_path: PathBuf::from("out.zip"),
            format: ArchiveFormat::Zip,
            sources: vec![PathBuf::from("file.txt")],
            split: SplitSettings {
                enabled: true,
                volume_size: 100, // too small
            },
            ..Default::default()
        };
        let problems = s.validate();
        assert!(problems.iter().any(|p| p.contains("64 KiB")));
    }

    // --- AppState tests ---

    #[test]
    fn test_app_state_default() {
        let s = AppState::default();
        assert!(s.archive.is_none());
        assert!(s.sidebar_visible);
        assert_eq!(s.current_dir, "");
    }

    #[test]
    fn test_app_state_navigate_to() {
        let mut s = AppState::default();
        s.navigate_to("src");
        assert_eq!(s.current_dir, "src");
        assert_eq!(s.nav_position, 1);
        s.navigate_to("src/utils");
        assert_eq!(s.current_dir, "src/utils");
        assert_eq!(s.nav_position, 2);
    }

    #[test]
    fn test_app_state_navigate_back() {
        let mut s = AppState::default();
        s.navigate_to("src");
        s.navigate_to("docs");
        assert!(s.navigate_back());
        assert_eq!(s.current_dir, "src");
        assert!(s.navigate_back());
        assert_eq!(s.current_dir, "");
        assert!(!s.navigate_back()); // already at start
    }

    #[test]
    fn test_app_state_navigate_forward() {
        let mut s = AppState::default();
        s.navigate_to("src");
        s.navigate_to("docs");
        s.navigate_back();
        s.navigate_back();
        assert!(s.navigate_forward());
        assert_eq!(s.current_dir, "src");
        assert!(s.navigate_forward());
        assert_eq!(s.current_dir, "docs");
        assert!(!s.navigate_forward()); // at end
    }

    #[test]
    fn test_app_state_navigate_up() {
        let mut s = AppState::default();
        s.navigate_to("src/utils");
        assert!(s.navigate_up());
        assert_eq!(s.current_dir, "src");
        assert!(s.navigate_up());
        assert_eq!(s.current_dir, "");
        assert!(!s.navigate_up()); // already at root
    }

    #[test]
    fn test_app_state_toggle_sort() {
        let mut s = AppState::default();
        s.toggle_sort(Column::Name);
        // Already sorting by Name ascending, should flip to descending.
        assert_eq!(s.sort.direction, SortDirection::Descending);
        s.toggle_sort(Column::Size);
        // Switch to Size ascending.
        assert_eq!(s.sort.column, Column::Size);
        assert_eq!(s.sort.direction, SortDirection::Ascending);
    }

    #[test]
    fn test_app_state_column_header_with_indicator() {
        let mut s = AppState::default();
        let h = s.column_header_text(Column::Name);
        assert!(h.contains('^'), "should have ascending indicator");
        s.toggle_sort(Column::Name);
        let h2 = s.column_header_text(Column::Name);
        assert!(h2.contains('v'), "should have descending indicator");
        let h3 = s.column_header_text(Column::Size);
        assert!(!h3.contains('^') && !h3.contains('v'));
    }

    #[test]
    fn test_app_state_status_text_no_archive() {
        let s = AppState::default();
        assert!(s.status_text().contains("No archive"));
    }

    #[test]
    fn test_app_state_status_text_with_archive() {
        let s = AppState {
            archive: Some(create_sample_archive()),
            ..AppState::default()
        };
        let text = s.status_text();
        assert!(text.contains("files"));
        assert!(text.contains("Ratio"));
    }

    // --- Rendering tests ---

    #[test]
    fn test_render_frame_no_archive() {
        let state = AppState::default();
        let cmds = render_frame(&state);
        assert!(!cmds.is_empty(), "should produce render commands");
        // Should have at least the background fill.
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_frame_with_archive() {
        let state = AppState {
            archive: Some(create_sample_archive()),
            ..AppState::default()
        };
        let cmds = render_frame(&state);
        assert!(
            cmds.len() > 20,
            "should produce many render commands with an archive open"
        );
    }

    #[test]
    fn test_render_frame_with_progress() {
        let state = AppState {
            archive: Some(create_sample_archive()),
            progress: Some(OperationProgress::new("Extracting", 10, 5000)),
            ..AppState::default()
        };
        let cmds = render_frame(&state);
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_toolbar_buttons() {
        let state = AppState::default();
        let mut frame = Frame::new(800.0, 600.0);
        let h = render_toolbar(&state, &mut frame, 0.0, 800.0);
        assert_eq!(h, 40.0);
        assert!(!frame.tree.commands.is_empty());
    }

    #[test]
    fn test_render_path_bar() {
        let state = AppState::default();
        let mut frame = Frame::new(800.0, 600.0);
        let h = render_path_bar(&state, &mut frame, 0.0, 800.0);
        assert_eq!(h, 32.0);
        assert!(!frame.tree.commands.is_empty());
    }

    #[test]
    fn test_render_sidebar_hidden() {
        let state = AppState {
            sidebar_visible: false,
            ..AppState::default()
        };
        let mut frame = Frame::new(800.0, 600.0);
        let w = render_sidebar(&state, &mut frame, 0.0, 400.0);
        assert_eq!(w, 0.0);
        assert!(frame.tree.commands.is_empty());
    }

    #[test]
    fn test_render_sidebar_visible() {
        let state = AppState {
            archive: Some(create_sample_archive()),
            ..AppState::default()
        };
        let mut frame = Frame::new(800.0, 600.0);
        let w = render_sidebar(&state, &mut frame, 0.0, 400.0);
        assert!(w > 0.0);
        assert!(!frame.tree.commands.is_empty());
    }

    // --- Sample data test ---

    #[test]
    fn test_create_sample_archive() {
        let a = create_sample_archive();
        assert!(a.file_count > 0);
        assert!(a.dir_count > 0);
        assert!(a.total_size > 0);
        assert!(a.total_compressed > 0);
        assert!(!a.tree.children.is_empty());
    }

    // --- calendar boundaries, asserted through the surface that renders them ---

    /// The two facts the deleted `days_to_ymd` tests pinned — the epoch, and
    /// that day 19723 is 2024-01-01 — restated through `format_date`, the only
    /// thing in this program that ever wanted a date.
    ///
    /// They are worth keeping because they are the boundaries a hand-rolled
    /// calendar gets wrong: the epoch itself, and a year far enough out that a
    /// leap-year rule has had chances to drift.
    #[test]
    fn the_epoch_and_a_distant_year_render_as_themselves() {
        // One second past the epoch, because zero is the "no stored mtime"
        // sentinel and never reaches the calendar at all.
        assert_eq!(ArchiveEntry::format_date(1), "1970-01-01 00:00");
        // Day 19723 * 86400.
        assert_eq!(ArchiveEntry::format_date(1_704_067_200), "2024-01-01 00:00");
    }

    // --- ViewMode test ---

    #[test]
    fn test_view_mode_default() {
        assert_eq!(ViewMode::default(), ViewMode::DirectoryView);
    }

    // ------------------------------------------------------------------
    // Interaction
    //
    // Every test here finds its coordinates by *rendering* and reading the
    // recorded hit boxes back. None of them recomputes a layout constant. A
    // test that computed `40.0 + 4.0` for a button's y would keep passing
    // after the renderer moved the button, which is the one failure it exists
    // to catch.
    // ------------------------------------------------------------------

    const SIZE: (f32, f32) = (900.0, 600.0);

    /// The centre of the first recorded box for `pred`, or a panic naming what
    /// was missing — a test that silently skipped a control it could not find
    /// would report success for a program with no such control.
    fn centre_of(state: &AppState, pred: impl Fn(&Target) -> bool, what: &str) -> (f32, f32) {
        let frame = build_frame(state, SIZE.0, SIZE.1);
        let (_, rect) = frame
            .hits
            .iter()
            .find(|(t, _)| pred(t))
            .unwrap_or_else(|| panic!("no {what} was drawn"));
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    fn loaded() -> AppState {
        AppState {
            archive: Some(create_sample_archive()),
            ..AppState::default()
        }
    }

    fn click(state: &mut AppState, at: (f32, f32)) -> Action {
        state.handle_click(at.0, at.1, MouseButton::Left, SIZE)
    }

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn ctrl(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            text: String::new(),
        }
    }

    #[test]
    fn a_click_on_empty_background_does_nothing() {
        let mut state = loaded();
        // The far bottom-left of the status bar is not a control.
        let before = state.clone();
        let action = click(&mut state, (5.0, SIZE.1 - 2.0));
        assert_eq!(action, Action::None);
        assert_eq!(state.status_message, before.status_message);
        assert_eq!(state.current_dir, before.current_dir);
    }

    #[test]
    fn clicking_a_column_header_sorts_by_that_column() {
        let mut state = loaded();
        let at = centre_of(
            &state,
            |t| matches!(t, Target::ColumnHeader(Column::Size)),
            "Size column header",
        );
        assert_ne!(state.sort.column, Column::Size);
        assert_eq!(click(&mut state, at), Action::Redraw);
        assert_eq!(state.sort.column, Column::Size);
    }

    #[test]
    fn clicking_the_same_header_twice_reverses_the_sort() {
        let mut state = loaded();
        let at = centre_of(
            &state,
            |t| matches!(t, Target::ColumnHeader(Column::Size)),
            "Size column header",
        );
        click(&mut state, at);
        let first = state.sort.direction;
        // Re-read the geometry: sorting redraws, and a header could move.
        let at = centre_of(
            &state,
            |t| matches!(t, Target::ColumnHeader(Column::Size)),
            "Size column header",
        );
        click(&mut state, at);
        assert_eq!(state.sort.column, Column::Size);
        assert_ne!(state.sort.direction, first);
    }

    #[test]
    fn clicking_a_file_row_selects_the_entry_that_was_drawn_there() {
        let mut state = loaded();
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        let (target, rect) = frame
            .hits
            .iter()
            .find(|(t, _)| matches!(t, Target::FileRow(_)))
            .expect("no file row was drawn");
        let Target::FileRow(id) = *target else {
            panic!("find matched a non-row")
        };
        let at = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        assert_eq!(click(&mut state, at), Action::Redraw);
        let archive = state.archive.as_ref().expect("archive");
        let entry = archive
            .entries
            .iter()
            .find(|e| e.id == id)
            .expect("clicked id must exist");
        assert!(entry.selected, "the row under the pointer was not selected");
    }

    #[test]
    fn re_sorting_does_not_move_the_selection_to_a_different_file() {
        // The bug this pins: addressing rows by *index* means a click after a
        // sort selects whatever slid into that slot. `Target::FileRow` carries
        // the entry id, so the selection follows the file.
        let mut state = loaded();
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        let (target, rect) = frame
            .hits
            .iter()
            .find(|(t, _)| matches!(t, Target::FileRow(_)))
            .expect("no file row was drawn");
        let Target::FileRow(id) = *target else {
            panic!("find matched a non-row")
        };
        let name = state
            .archive
            .as_ref()
            .expect("archive")
            .entries
            .iter()
            .find(|e| e.id == id)
            .expect("id")
            .name
            .clone();
        click(&mut state, (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));

        let header = centre_of(
            &state,
            |t| matches!(t, Target::ColumnHeader(Column::Size)),
            "Size column header",
        );
        click(&mut state, header);

        let selected: Vec<String> = state
            .archive
            .as_ref()
            .expect("archive")
            .selected_entries()
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(selected, vec![name]);
    }

    #[test]
    fn double_clicking_a_directory_navigates_into_it() {
        let mut state = loaded();
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        let dir_id = state
            .archive
            .as_ref()
            .expect("archive")
            .entries
            .iter()
            .find(|e| e.is_dir)
            .map(|e| e.id)
            .expect("the sample archive has a directory");
        let (_, rect) = frame
            .hits
            .iter()
            .find(|(t, _)| matches!(t, Target::FileRow(id) if *id == dir_id))
            .expect("the directory row was not drawn");
        let at = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        assert_eq!(
            state.handle_double_click(at.0, at.1, MouseButton::Left, SIZE),
            Action::Redraw
        );
        assert!(!state.current_dir.is_empty(), "did not enter the directory");
    }

    #[test]
    fn nav_back_returns_to_the_previous_directory_and_forward_undoes_it() {
        let mut state = loaded();
        state.navigate_to("src");
        assert_eq!(state.current_dir, "src");

        let back = centre_of(&state, |t| matches!(t, Target::NavBack), "back button");
        assert_eq!(click(&mut state, back), Action::Redraw);
        assert_eq!(state.current_dir, "");

        let fwd = centre_of(
            &state,
            |t| matches!(t, Target::NavForward),
            "forward button",
        );
        assert_eq!(click(&mut state, fwd), Action::Redraw);
        assert_eq!(state.current_dir, "src");
    }

    #[test]
    fn nav_back_at_the_start_of_history_reports_that_nothing_happened() {
        let mut state = loaded();
        let back = centre_of(&state, |t| matches!(t, Target::NavBack), "back button");
        assert_eq!(click(&mut state, back), Action::None);
    }

    #[test]
    fn the_tree_arrow_collapses_without_navigating() {
        let mut state = loaded();
        let before = state.tree_rows().len();
        let at = centre_of(&state, |t| matches!(t, Target::TreeArrow(_)), "tree arrow");
        let dir_before = state.current_dir.clone();
        assert_eq!(click(&mut state, at), Action::Redraw);
        assert_ne!(
            state.tree_rows().len(),
            before,
            "the arrow did not change the tree"
        );
        assert_eq!(
            state.current_dir, dir_before,
            "the arrow navigated as well as expanding"
        );
    }

    #[test]
    fn the_tree_row_navigates_without_collapsing() {
        let mut state = loaded();
        // Row 0 is the archive itself; find a row that names a directory.
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        let rows = state.tree_rows();
        let (target, rect) = frame
            .hits
            .iter()
            .find(|(t, _)| matches!(t, Target::TreeRow(i) if rows.get(*i).is_some_and(|r| !r.path.is_empty())))
            .expect("no directory row in the tree");
        let Target::TreeRow(i) = *target else {
            panic!("find matched a non-row")
        };
        let want = rows.get(i).expect("row").path.clone();
        let before = state.tree_rows().len();
        // Click well right of the arrow so the arrow's box cannot claim it.
        let at = (rect.x + rect.w - 4.0, rect.y + rect.h / 2.0);
        assert_eq!(click(&mut state, at), Action::Redraw);
        assert_eq!(state.current_dir, want);
        assert_eq!(state.tree_rows().len(), before, "navigating also collapsed");
    }

    #[test]
    fn a_disabled_toolbar_button_says_why_rather_than_doing_nothing() {
        // No archive is open, so Extract All cannot run.
        let mut state = AppState::default();
        assert!(!toolbar_enabled(&state, ToolbarAction::ExtractAll));
        let at = centre_of(
            &state,
            |t| matches!(t, Target::Toolbar(ToolbarAction::ExtractAll)),
            "Extract All button",
        );
        assert_eq!(click(&mut state, at), Action::Redraw);
        assert!(
            state.status_message.contains("no archive is open"),
            "status was {:?}",
            state.status_message
        );
    }

    #[test]
    fn delete_is_disabled_until_something_is_selected() {
        let mut state = loaded();
        assert!(!toolbar_enabled(&state, ToolbarAction::Delete));
        let at = centre_of(
            &state,
            |t| matches!(t, Target::Toolbar(ToolbarAction::Delete)),
            "Delete button",
        );
        click(&mut state, at);
        assert!(state.status_message.contains("nothing is selected"));

        state.archive.as_mut().expect("archive").select_all();
        assert!(toolbar_enabled(&state, ToolbarAction::Delete));
    }

    #[test]
    fn delete_removes_exactly_the_selected_entries() {
        let mut state = loaded();
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        let (target, rect) = frame
            .hits
            .iter()
            .find(|(t, _)| matches!(t, Target::FileRow(_)))
            .expect("no file row");
        let Target::FileRow(id) = *target else {
            panic!("non-row")
        };
        click(&mut state, (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
        let before = state.archive.as_ref().expect("archive").entries.len();

        let at = centre_of(
            &state,
            |t| matches!(t, Target::Toolbar(ToolbarAction::Delete)),
            "Delete button",
        );
        assert_eq!(click(&mut state, at), Action::Redraw);
        let archive = state.archive.as_ref().expect("archive");
        assert_eq!(archive.entries.len(), before - 1);
        assert!(
            !archive.entries.iter().any(|e| e.id == id),
            "the deleted entry is still in the list"
        );
        assert_eq!(state.hovered_entry, None, "cursor left on a deleted row");
    }

    #[test]
    fn open_reports_honestly_that_there_is_no_back_end_yet() {
        let mut state = loaded();
        let at = centre_of(
            &state,
            |t| matches!(t, Target::Toolbar(ToolbarAction::Open)),
            "Open button",
        );
        assert_eq!(click(&mut state, at), Action::Redraw);
        assert!(
            state.status_message.contains("not yet implemented"),
            "status was {:?}",
            state.status_message
        );
    }

    #[test]
    fn arrow_keys_move_the_cursor_through_the_visible_rows() {
        let mut state = loaded();
        assert_eq!(state.handle_key(&key(Key::Down), SIZE), Action::Redraw);
        let first = state.hovered_entry.expect("Down set no cursor");
        assert_eq!(state.handle_key(&key(Key::Down), SIZE), Action::Redraw);
        let second = state.hovered_entry.expect("cursor vanished");
        assert_ne!(first, second);
        assert_eq!(state.handle_key(&key(Key::Up), SIZE), Action::Redraw);
        assert_eq!(state.hovered_entry, Some(first));
    }

    #[test]
    fn up_at_the_top_and_down_at_the_bottom_stop_rather_than_wrap() {
        let mut state = loaded();
        state.handle_key(&key(Key::Home), SIZE);
        let top = state.hovered_entry;
        assert_eq!(state.handle_key(&key(Key::Up), SIZE), Action::None);
        assert_eq!(state.hovered_entry, top);

        state.handle_key(&key(Key::End), SIZE);
        let bottom = state.hovered_entry;
        assert_ne!(bottom, top);
        assert_eq!(state.handle_key(&key(Key::Down), SIZE), Action::None);
        assert_eq!(state.hovered_entry, bottom);
    }

    #[test]
    fn end_lands_on_the_last_visible_row() {
        let mut state = loaded();
        state.handle_key(&key(Key::End), SIZE);
        let last = state
            .visible_entries()
            .last()
            .map(|e| e.id)
            .expect("no visible entries");
        assert_eq!(state.hovered_entry, Some(last));
    }

    #[test]
    fn space_toggles_the_selection_of_the_row_under_the_cursor() {
        let mut state = loaded();
        state.handle_key(&key(Key::Down), SIZE);
        let id = state.hovered_entry.expect("cursor");
        state.handle_key(&key(Key::Space), SIZE);
        assert!(
            state
                .archive
                .as_ref()
                .expect("archive")
                .entries
                .iter()
                .any(|e| e.id == id && e.selected)
        );
        state.handle_key(&key(Key::Space), SIZE);
        assert!(
            state
                .archive
                .as_ref()
                .expect("archive")
                .selected_entries()
                .is_empty()
        );
    }

    #[test]
    fn ctrl_a_selects_every_entry() {
        let mut state = loaded();
        assert_eq!(state.handle_key(&ctrl(Key::A), SIZE), Action::Redraw);
        let archive = state.archive.as_ref().expect("archive");
        assert_eq!(archive.selected_entries().len(), archive.entries.len());
    }

    #[test]
    fn ctrl_b_hides_and_shows_the_sidebar() {
        let mut state = loaded();
        assert!(state.sidebar_visible);
        state.handle_key(&ctrl(Key::B), SIZE);
        assert!(!state.sidebar_visible);
        // And the tree really stops being drawn, not just the flag flipping.
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        assert!(
            !frame
                .hits
                .iter()
                .any(|(t, _)| matches!(t, Target::TreeRow(_)))
        );
        state.handle_key(&ctrl(Key::B), SIZE);
        assert!(state.sidebar_visible);
    }

    #[test]
    fn escape_clears_a_selection_before_it_closes_the_window() {
        let mut state = loaded();
        state.handle_key(&ctrl(Key::A), SIZE);
        assert_eq!(state.handle_key(&key(Key::Escape), SIZE), Action::Redraw);
        assert!(
            state
                .archive
                .as_ref()
                .expect("archive")
                .selected_entries()
                .is_empty()
        );
        // Only now, with nothing to lose, does Escape close.
        assert_eq!(state.handle_key(&key(Key::Escape), SIZE), Action::Quit);
    }

    #[test]
    fn backspace_goes_up_a_directory() {
        let mut state = loaded();
        state.navigate_to("src/gui");
        assert_eq!(state.handle_key(&key(Key::Backspace), SIZE), Action::Redraw);
        assert_eq!(state.current_dir, "src");
        assert_eq!(state.handle_key(&key(Key::Backspace), SIZE), Action::Redraw);
        assert_eq!(state.current_dir, "");
        // At the root there is nowhere to go, and it says so by doing nothing.
        assert_eq!(state.handle_key(&key(Key::Backspace), SIZE), Action::None);
    }

    #[test]
    fn a_key_release_does_not_repeat_the_press() {
        let mut state = loaded();
        state.handle_key(&key(Key::Down), SIZE);
        let after_press = state.hovered_entry;
        let release = KeyEvent {
            key: Key::Down,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert_eq!(state.handle_key(&release, SIZE), Action::None);
        assert_eq!(state.hovered_entry, after_press);
    }

    #[test]
    fn the_wheel_scrolls_the_list_and_stops_at_both_ends() {
        let mut state = loaded();
        // Make the list long enough to have somewhere to scroll to.
        let archive = state.archive.as_mut().expect("archive");
        for i in 0..200 {
            archive.add_entry(ArchiveEntry {
                path: format!("bulk{i}.txt"),
                name: format!("bulk{i}.txt"),
                size: 10,
                compressed_size: 5,
                is_dir: false,
                modified: 0,
                crc32: 0,
                encrypted: false,
                method: String::from("Deflate"),
                depth: 0,
                expanded: false,
                selected: false,
                id: 0,
            });
        }
        state.view_mode = ViewMode::FlatList;

        assert_eq!(state.list_scroll_y, 0.0);
        // Scrolling up at the top has nowhere to go.
        let up = MouseEvent {
            x: SIZE.0 - 40.0,
            y: 200.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: 5.0 },
        };
        assert_eq!(
            state.handle_event(&Event::Mouse(up), SIZE),
            Action::None,
            "scrolled above the first row"
        );

        let down = MouseEvent {
            x: SIZE.0 - 40.0,
            y: 200.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -5.0 },
        };
        assert_eq!(
            state.handle_event(&Event::Mouse(down.clone()), SIZE),
            Action::Redraw
        );
        assert!(state.list_scroll_y > 0.0);

        // And it cannot be scrolled past the last row.
        for _ in 0..500 {
            state.handle_event(&Event::Mouse(down.clone()), SIZE);
        }
        assert_eq!(state.list_scroll_y, state.max_list_scroll(SIZE.1));
        assert_eq!(
            state.handle_event(&Event::Mouse(down), SIZE),
            Action::None,
            "scrolled past the last row"
        );
    }

    #[test]
    fn a_row_scrolled_off_the_top_is_not_clickable_where_it_used_to_be() {
        // The clip stack is what makes this true: rows are drawn inside a
        // clip, and a hit box that falls outside it is dropped rather than
        // recorded. Without that, the header would sit on top of invisible
        // rows and clicking it would select a file.
        let mut state = loaded();
        state.view_mode = ViewMode::FlatList;
        let archive = state.archive.as_mut().expect("archive");
        for i in 0..200 {
            archive.add_entry(ArchiveEntry {
                path: format!("bulk{i}.txt"),
                name: format!("bulk{i}.txt"),
                size: 10,
                compressed_size: 5,
                is_dir: false,
                modified: 0,
                crc32: 0,
                encrypted: false,
                method: String::from("Deflate"),
                depth: 0,
                expanded: false,
                selected: false,
                id: 0,
            });
        }
        state.list_scroll_y = 100.0;
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        let (top, _) = state.content_band(SIZE.1);
        for (target, rect) in &frame.hits {
            if matches!(target, Target::FileRow(_)) {
                assert!(
                    rect.y >= top + HEADER_H - 0.01,
                    "a row was clickable at y={}, above the list at {}",
                    rect.y,
                    top + HEADER_H
                );
            }
        }
    }

    #[test]
    fn moving_the_pointer_highlights_the_row_under_it_and_only_redraws_on_change() {
        let mut state = loaded();
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        let (target, rect) = frame
            .hits
            .iter()
            .find(|(t, _)| matches!(t, Target::FileRow(_)))
            .expect("no file row");
        let Target::FileRow(id) = *target else {
            panic!("non-row")
        };
        let mv = MouseEvent {
            x: rect.x + rect.w / 2.0,
            y: rect.y + rect.h / 2.0,
            kind: MouseEventKind::Move,
        };
        assert_eq!(
            state.handle_event(&Event::Mouse(mv.clone()), SIZE),
            Action::Redraw
        );
        assert_eq!(state.hovered_entry, Some(id));
        // The same position again is not news.
        assert_eq!(state.handle_event(&Event::Mouse(mv), SIZE), Action::None);
    }

    #[test]
    fn the_pointer_leaving_the_window_clears_the_highlight() {
        let mut state = loaded();
        state.hovered_entry = Some(1);
        let leave = MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Leave,
        };
        assert_eq!(
            state.handle_event(&Event::Mouse(leave), SIZE),
            Action::Redraw
        );
        assert_eq!(state.hovered_entry, None);
    }

    #[test]
    fn a_right_click_is_not_a_left_click() {
        let mut state = loaded();
        let at = centre_of(
            &state,
            |t| matches!(t, Target::ColumnHeader(Column::Size)),
            "Size column header",
        );
        let before = state.sort.column;
        assert_eq!(
            state.handle_click(at.0, at.1, MouseButton::Right, SIZE),
            Action::None
        );
        assert_eq!(state.sort.column, before);
    }

    #[test]
    fn close_requested_exits() {
        let mut state = loaded();
        assert_eq!(
            state.handle_event(&Event::CloseRequested, SIZE),
            Action::Quit
        );
    }

    // --- the App trait: what the window actually calls ---

    #[test]
    fn the_first_frame_is_drawn_at_the_size_the_window_gives_it() {
        // `render` is called before any `Resize`, so a renderer that trusted
        // the stored size would draw the opening frame 900x600 whatever the
        // window really was.
        let mut state = loaded();
        let tree = state.render(1280.0, 720.0);
        assert_eq!(state.window_width, 1280.0);
        assert_eq!(state.window_height, 720.0);
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn resizing_updates_the_size_and_reclamps_both_scrolls() {
        let mut state = loaded();
        state.view_mode = ViewMode::FlatList;
        let archive = state.archive.as_mut().expect("archive");
        for i in 0..200 {
            archive.add_entry(ArchiveEntry {
                path: format!("bulk{i}.txt"),
                name: format!("bulk{i}.txt"),
                size: 10,
                compressed_size: 5,
                is_dir: false,
                modified: 0,
                crc32: 0,
                encrypted: false,
                method: String::from("Deflate"),
                depth: 0,
                expanded: false,
                selected: false,
                id: 0,
            });
        }
        state.list_scroll_y = state.max_list_scroll(600.0);
        let tall = state.list_scroll_y;

        let response = state.on_event(&Event::Resize {
            width: 900,
            height: 2000,
        });
        assert_eq!(response, Response::Redraw);
        assert_eq!(state.window_height, 2000.0);
        assert!(
            state.list_scroll_y < tall,
            "a taller window kept a scroll offset that now shows blank space"
        );
        assert_eq!(state.list_scroll_y, state.max_list_scroll(2000.0));
    }

    #[test]
    fn a_resize_to_the_same_size_is_not_a_redraw() {
        let mut state = loaded();
        state.window_width = 900.0;
        state.window_height = 600.0;
        assert_eq!(
            state.on_event(&Event::Resize {
                width: 900,
                height: 600
            }),
            Response::Idle
        );
    }

    #[test]
    fn escape_asks_the_window_to_exit() {
        let mut state = loaded();
        assert_eq!(
            state.on_event(&Event::Key(key(Key::Escape))),
            Response::Exit
        );
    }

    #[test]
    fn the_title_names_the_open_archive() {
        let state = loaded();
        let title = state.title();
        assert!(title.contains("Archive Manager"));
        let name = state
            .archive
            .as_ref()
            .expect("archive")
            .path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert!(title.contains(&name), "title was {title:?}");

        let empty = AppState::default();
        assert_eq!(empty.title(), "Archive Manager");
    }

    #[test]
    fn the_window_opens_at_a_size_the_layout_fits_in() {
        let state = AppState::default();
        let (w, h) = state.initial_size();
        assert!(f64::from(w) >= f64::from(MIN_WIDTH));
        assert!(f64::from(h) >= f64::from(MIN_HEIGHT));
    }

    #[test]
    fn every_toolbar_button_is_drawn_and_hit_testable() {
        // A button that is rendered but never recorded is a button the user
        // can see and cannot press.
        let state = loaded();
        let frame = build_frame(&state, SIZE.0, SIZE.1);
        for action in ToolbarAction::all() {
            let found = frame
                .hits
                .iter()
                .any(|(t, _)| matches!(t, Target::Toolbar(a) if a == action));
            assert!(found, "{} has no hit box", action.label());
        }
    }

    #[test]
    fn a_narrow_window_still_records_boxes_inside_itself() {
        // `Frame::new` clamps to a minimum, so a window dragged smaller than
        // the layout can survive does not produce controls at negative
        // coordinates or boxes wider than the window.
        let state = loaded();
        let frame = build_frame(&state, 100.0, 100.0);
        for (target, rect) in &frame.hits {
            assert!(rect.x >= 0.0 && rect.y >= 0.0, "{target:?} at {rect:?}");
            assert!(rect.w > 0.0 && rect.h > 0.0, "{target:?} is empty");
        }
    }

    #[test]
    fn the_progress_strip_shortens_the_list_rather_than_covering_it() {
        let mut state = loaded();
        let (_, without) = state.content_band(600.0);
        state.progress = Some(OperationProgress::new("Extracting", 10, 1000));
        let (_, with) = state.content_band(600.0);
        assert!(with < without);
        assert_eq!(without - with, PROGRESS_H);
        // And the scroll limit follows, so the last rows cannot hide behind it.
        assert!(state.max_list_scroll(600.0) >= 0.0);
    }
}
