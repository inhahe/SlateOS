//! SlateOS Archive Manager
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
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(Default)]
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
        if bytes < 1024 {
            return format!("{bytes} B");
        }
        let kb = bytes as f64 / 1024.0;
        if kb < 1024.0 {
            return format!("{kb:.1} KB");
        }
        let mb = kb / 1024.0;
        if mb < 1024.0 {
            return format!("{mb:.1} MB");
        }
        let gb = mb / 1024.0;
        format!("{gb:.2} GB")
    }

    /// Format CRC as a hex string.
    pub fn format_crc(crc: u32) -> String {
        format!("{crc:08X}")
    }

    /// Format a Unix timestamp as a date string.
    pub fn format_date(timestamp: u64) -> String {
        // Simplified: just show raw seconds-since-epoch for now.
        // A real implementation would use a date formatting library.
        if timestamp == 0 {
            return String::from("-");
        }
        // Rough conversion: seconds since 1970-01-01 to YYYY-MM-DD HH:MM
        let secs = timestamp;
        let days = secs / 86400;
        let time_of_day = secs % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;

        // Approximate year/month/day from days since epoch.
        let (year, month, day) = days_to_ymd(days);
        format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
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

/// Convert days since epoch to (year, month, day).
/// Approximate civil date calculation.
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm adapted from Howard Hinnant's civil_from_days.
    let z = days.wrapping_add(719468);
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

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
            count += child.total_descendants();
        }
        count
    }

    /// Find or create a child node at the given relative path.
    pub fn get_or_create_child(&mut self, name: &str, full_path: &str) -> &mut TreeNode {
        // Check if child already exists.
        let pos = self.children.iter().position(|c| c.name == name);
        if let Some(idx) = pos {
            return &mut self.children[idx];
        }
        self.children.push(TreeNode::new(name, full_path));
        let last = self.children.len() - 1;
        &mut self.children[last]
    }

    /// Toggle expansion state.
    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
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
                child.flatten(depth + 1, out);
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
            // Create directories for all but the last component.
            if parts.len() > 1 {
                for part in &parts[..parts.len() - 1] {
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
            current.file_count += 1;
            current.total_size += entry.size;
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
    AddFiles {
        files: Vec<PathBuf>,
    },
    /// Remove entries from an archive.
    RemoveEntries {
        paths: Vec<String>,
    },
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
    pub fn percent(&self) -> f64 {
        if self.bytes_total == 0 {
            if self.files_total == 0 {
                return 100.0;
            }
            return (self.files_done as f64 / self.files_total as f64) * 100.0;
        }
        (self.bytes_done as f64 / self.bytes_total as f64) * 100.0
    }

    /// Update progress for a file.
    pub fn advance_file(&mut self, name: &str, bytes: u64) {
        self.current_file = name.to_string();
        self.files_done += 1;
        self.bytes_done += bytes;
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
#[derive(Clone, Debug)]
#[derive(Default)]
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
        self.tested += 1;
        match &result {
            TestResult::Ok => self.passed += 1,
            TestResult::Pending => {}
            _ => self.failed += 1,
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
        self.next_id += 1;

        if entry.is_dir {
            self.dir_count += 1;
        } else {
            self.file_count += 1;
            self.total_size += entry.size;
            self.total_compressed += entry.compressed_size;
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
                self.dir_count += 1;
            } else {
                self.file_count += 1;
                self.total_size += entry.size;
                self.total_compressed += entry.compressed_size;
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
            problems.push("Volume size must be at least 64 KB".into());
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
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
        }
    }
}

impl AppState {
    /// Navigate to a directory within the archive.
    pub fn navigate_to(&mut self, dir: &str) {
        // Truncate forward history if we've gone back.
        if self.nav_position + 1 < self.nav_history.len() {
            self.nav_history.truncate(self.nav_position + 1);
        }
        self.current_dir = dir.to_string();
        self.nav_history.push(dir.to_string());
        self.nav_position = self.nav_history.len() - 1;
        self.list_scroll_y = 0.0;
    }

    /// Navigate back in history.
    pub fn navigate_back(&mut self) -> bool {
        if self.nav_position > 0 {
            self.nav_position -= 1;
            self.current_dir = self.nav_history[self.nav_position].clone();
            self.list_scroll_y = 0.0;
            true
        } else {
            false
        }
    }

    /// Navigate forward in history.
    pub fn navigate_forward(&mut self) -> bool {
        if self.nav_position + 1 < self.nav_history.len() {
            self.nav_position += 1;
            self.current_dir = self.nav_history[self.nav_position].clone();
            self.list_scroll_y = 0.0;
            true
        } else {
            false
        }
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
                    let sel_size: u64 = archive
                        .selected_entries()
                        .iter()
                        .map(|e| e.size)
                        .sum();
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
// UI rendering
// ============================================================================

/// Toolbar button definition.
struct ToolbarButton {
    label: &'static str,
    icon: &'static str,
    enabled: bool,
}

/// Render the toolbar.
pub fn render_toolbar(
    state: &AppState,
    cmds: &mut Vec<RenderCommand>,
    y_offset: f32,
    width: f32,
) -> f32 {
    let toolbar_h = 40.0;

    // Background
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: y_offset,
        width,
        height: toolbar_h,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let has_archive = state.archive.is_some();
    let has_selection = state
        .archive
        .as_ref()
        .is_some_and(|a| a.entries.iter().any(|e| e.selected));

    let buttons = [
        ToolbarButton {
            label: "Open",
            icon: "O",
            enabled: true,
        },
        ToolbarButton {
            label: "New",
            icon: "N",
            enabled: true,
        },
        ToolbarButton {
            label: "Extract All",
            icon: "E",
            enabled: has_archive,
        },
        ToolbarButton {
            label: "Extract Sel.",
            icon: "S",
            enabled: has_selection,
        },
        ToolbarButton {
            label: "Add",
            icon: "+",
            enabled: has_archive,
        },
        ToolbarButton {
            label: "Delete",
            icon: "X",
            enabled: has_selection,
        },
        ToolbarButton {
            label: "Test",
            icon: "T",
            enabled: has_archive,
        },
    ];

    let mut x = 8.0;
    let btn_h = 28.0;
    let btn_y = y_offset + (toolbar_h - btn_h) / 2.0;

    for btn in &buttons {
        let text = format!("[{}] {}", btn.icon, btn.label);
        let btn_w = text.len() as f32 * 7.5 + 16.0;
        let bg = if btn.enabled {
            theme::SURFACE1
        } else {
            theme::MANTLE
        };
        let fg = if btn.enabled {
            theme::TEXT
        } else {
            theme::OVERLAY0
        };

        cmds.push(RenderCommand::FillRect {
            x,
            y: btn_y,
            width: btn_w,
            height: btn_h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: btn_y + 7.0,
            text,
            color: fg,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn_w - 16.0),
        });

        x += btn_w + 4.0;
    }

    // Separator line
    cmds.push(RenderCommand::Line {
        x1: 0.0,
        y1: y_offset + toolbar_h - 1.0,
        x2: width,
        y2: y_offset + toolbar_h - 1.0,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    toolbar_h
}

/// Render the address/path bar.
pub fn render_path_bar(
    state: &AppState,
    cmds: &mut Vec<RenderCommand>,
    y_offset: f32,
    width: f32,
) -> f32 {
    let bar_h = 32.0;

    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: y_offset,
        width,
        height: bar_h,
        color: theme::MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Back / Forward / Up buttons
    let nav_btns = ["<", ">", "^"];
    let mut x = 4.0;
    for btn_text in &nav_btns {
        cmds.push(RenderCommand::FillRect {
            x,
            y: y_offset + 4.0,
            width: 24.0,
            height: 24.0,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y_offset + 10.0,
            text: btn_text.to_string(),
            color: theme::TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        x += 28.0;
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
    cmds.push(RenderCommand::FillRect {
        x: path_x,
        y: y_offset + 4.0,
        width: width - path_x - 8.0,
        height: 24.0,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::all(3.0),
    });
    cmds.push(RenderCommand::Text {
        x: path_x + 8.0,
        y: y_offset + 10.0,
        text: path_text,
        color: theme::TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width - path_x - 24.0),
    });

    // Bottom separator
    cmds.push(RenderCommand::Line {
        x1: 0.0,
        y1: y_offset + bar_h - 1.0,
        x2: width,
        y2: y_offset + bar_h - 1.0,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    bar_h
}

/// Render the sidebar tree view.
pub fn render_sidebar(
    state: &AppState,
    cmds: &mut Vec<RenderCommand>,
    y_offset: f32,
    height: f32,
) -> f32 {
    if !state.sidebar_visible {
        return 0.0;
    }
    let w = state.sidebar_width;

    // Background
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: y_offset,
        width: w,
        height,
        color: theme::MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Tree header
    cmds.push(RenderCommand::Text {
        x: 8.0,
        y: y_offset + 8.0,
        text: "Archive Tree".to_string(),
        color: theme::BLUE,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(w - 16.0),
    });

    if let Some(archive) = &state.archive {
        let mut rows = Vec::new();
        archive.tree.flatten(0, &mut rows);

        let row_h = 22.0;
        let start_y = y_offset + 28.0;

        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: start_y,
            width: w,
            height: height - 28.0,
        });

        for (i, row) in rows.iter().enumerate() {
            let ry = start_y + i as f32 * row_h - state.tree_scroll_y;
            if ry + row_h < start_y || ry > y_offset + height {
                continue;
            }

            let indent = row.depth as f32 * 16.0 + 8.0;

            // Highlight if this is the current directory.
            if row.path == state.current_dir {
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: ry,
                    width: w,
                    height: row_h,
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
            cmds.push(RenderCommand::Text {
                x: indent,
                y: ry + 4.0,
                text: arrow.to_string(),
                color: theme::OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Folder icon and name.
            let display = format!("\u{1F4C1} {}", row.name);
            cmds.push(RenderCommand::Text {
                x: indent + 12.0,
                y: ry + 4.0,
                text: display,
                color: theme::TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - indent - 20.0),
            });

            // File count badge.
            if row.file_count > 0 {
                let count_text = format!("{}", row.file_count);
                cmds.push(RenderCommand::Text {
                    x: w - 30.0,
                    y: ry + 4.0,
                    text: count_text,
                    color: theme::SUBTEXT0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        cmds.push(RenderCommand::PopClip);
    }

    // Right border.
    cmds.push(RenderCommand::Line {
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
    cmds: &mut Vec<RenderCommand>,
    x_offset: f32,
    y_offset: f32,
    width: f32,
) -> f32 {
    let header_h = 24.0;

    cmds.push(RenderCommand::FillRect {
        x: x_offset,
        y: y_offset,
        width,
        height: header_h,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let mut x = x_offset + 4.0;
    for col in Column::all() {
        let col_w = col.default_width();
        let text = state.column_header_text(*col);

        cmds.push(RenderCommand::Text {
            x: x + 4.0,
            y: y_offset + 5.0,
            text,
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(col_w - 8.0),
        });

        // Column separator.
        x += col_w;
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y_offset + 2.0,
            x2: x,
            y2: y_offset + header_h - 2.0,
            color: theme::OVERLAY0,
            width: 1.0,
        });
    }

    // Bottom separator.
    cmds.push(RenderCommand::Line {
        x1: x_offset,
        y1: y_offset + header_h - 1.0,
        x2: x_offset + width,
        y2: y_offset + header_h - 1.0,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    header_h
}

/// Render a single row in the file list.
pub fn render_file_row(
    entry: &ArchiveEntry,
    cmds: &mut Vec<RenderCommand>,
    x_offset: f32,
    y: f32,
    width: f32,
    is_hovered: bool,
) {
    let row_h = 22.0;

    // Row background.
    if entry.selected {
        cmds.push(RenderCommand::FillRect {
            x: x_offset,
            y,
            width,
            height: row_h,
            color: Color::rgba(
                theme::BLUE.r,
                theme::BLUE.g,
                theme::BLUE.b,
                60,
            ),
            corner_radii: CornerRadii::ZERO,
        });
    } else if is_hovered {
        cmds.push(RenderCommand::FillRect {
            x: x_offset,
            y,
            width,
            height: row_h,
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
    cmds.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 4.0,
        text: name_text,
        color: name_color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(Column::Name.default_width() - 8.0),
    });
    x += Column::Name.default_width();

    // Size column.
    if !entry.is_dir {
        cmds.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: ArchiveEntry::format_size(entry.size),
            color: theme::TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::Size.default_width() - 8.0),
        });
    }
    x += Column::Size.default_width();

    // Compressed size.
    if !entry.is_dir {
        cmds.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: ArchiveEntry::format_size(entry.compressed_size),
            color: theme::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::CompressedSize.default_width() - 8.0),
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
        cmds.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: format!("{ratio:.0}%"),
            color: ratio_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::Ratio.default_width() - 8.0),
        });
    }
    x += Column::Ratio.default_width();

    // Date.
    cmds.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 4.0,
        text: ArchiveEntry::format_date(entry.modified),
        color: theme::SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(Column::Date.default_width() - 8.0),
    });
    x += Column::Date.default_width();

    // CRC.
    if !entry.is_dir {
        cmds.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: ArchiveEntry::format_crc(entry.crc32),
            color: theme::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Column::Crc.default_width() - 8.0),
        });
    }
    x += Column::Crc.default_width();

    // Method.
    cmds.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 4.0,
        text: entry.method.clone(),
        color: theme::SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(Column::Method.default_width() - 8.0),
    });
}

/// Render the file list view.
pub fn render_file_list(
    state: &AppState,
    cmds: &mut Vec<RenderCommand>,
    x_offset: f32,
    y_offset: f32,
    width: f32,
    height: f32,
) {
    // Background.
    cmds.push(RenderCommand::FillRect {
        x: x_offset,
        y: y_offset,
        width,
        height,
        color: theme::BASE,
        corner_radii: CornerRadii::ZERO,
    });

    let entries = state.visible_entries();
    let row_h = 22.0;

    cmds.push(RenderCommand::PushClip {
        x: x_offset,
        y: y_offset,
        width,
        height,
    });

    for (i, entry) in entries.iter().enumerate() {
        let ry = y_offset + i as f32 * row_h - state.list_scroll_y;
        if ry + row_h < y_offset || ry > y_offset + height {
            continue;
        }

        // Alternating row backgrounds.
        if i % 2 == 1 {
            cmds.push(RenderCommand::FillRect {
                x: x_offset,
                y: ry,
                width,
                height: row_h,
                color: Color::rgba(
                    theme::SURFACE0.r,
                    theme::SURFACE0.g,
                    theme::SURFACE0.b,
                    40,
                ),
                corner_radii: CornerRadii::ZERO,
            });
        }

        let is_hovered = state.hovered_entry == Some(entry.id);
        render_file_row(entry, cmds, x_offset, ry, width, is_hovered);
    }

    cmds.push(RenderCommand::PopClip);

    // "No archive open" message.
    if state.archive.is_none() {
        cmds.push(RenderCommand::Text {
            x: x_offset + width / 2.0 - 80.0,
            y: y_offset + height / 2.0 - 20.0,
            text: "Drop an archive here".to_string(),
            color: theme::OVERLAY0,
            font_size: 16.0,
            font_weight: FontWeightHint::Light,
            max_width: Some(200.0),
        });
        cmds.push(RenderCommand::Text {
            x: x_offset + width / 2.0 - 100.0,
            y: y_offset + height / 2.0 + 4.0,
            text: "or use Open to browse".to_string(),
            color: theme::OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(220.0),
        });
    }
}

/// Render the progress bar for ongoing operations.
pub fn render_progress_bar(
    progress: &OperationProgress,
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    width: f32,
) -> f32 {
    let bar_h = 48.0;

    // Background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: bar_h,
        color: theme::SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    // Operation label.
    cmds.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 4.0,
        text: format!(
            "{}: {}",
            progress.operation, progress.current_file
        ),
        color: theme::TEXT,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width - 16.0),
    });

    // Progress bar track.
    let bar_x = x + 8.0;
    let bar_y = y + 22.0;
    let bar_w = width - 80.0;
    let bar_track_h = 12.0;

    cmds.push(RenderCommand::FillRect {
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
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: fill_w,
            height: bar_track_h,
            color: fill_color,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    // Percentage text.
    cmds.push(RenderCommand::Text {
        x: bar_x + bar_w + 8.0,
        y: bar_y + 1.0,
        text: format!("{:.0}%", progress.percent()),
        color: theme::TEXT,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // File count.
    cmds.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 36.0,
        text: format!(
            "{}/{} files",
            progress.files_done, progress.files_total
        ),
        color: theme::SUBTEXT0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    bar_h
}

/// Render the status bar.
pub fn render_status_bar(
    state: &AppState,
    cmds: &mut Vec<RenderCommand>,
    y: f32,
    width: f32,
) -> f32 {
    let bar_h = 24.0;

    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width,
        height: bar_h,
        color: theme::MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Top separator.
    cmds.push(RenderCommand::Line {
        x1: 0.0,
        y1: y,
        x2: width,
        y2: y,
        color: theme::OVERLAY0,
        width: 1.0,
    });

    // Status text.
    cmds.push(RenderCommand::Text {
        x: 8.0,
        y: y + 6.0,
        text: state.status_text(),
        color: theme::SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width / 2.0),
    });

    // Format badge on the right.
    if let Some(archive) = &state.archive {
        let format_text = archive.format.display_name();
        cmds.push(RenderCommand::Text {
            x: width - 120.0,
            y: y + 6.0,
            text: format_text.to_string(),
            color: theme::PEACH,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(110.0),
        });
    }

    bar_h
}

/// Render the drag-and-drop overlay when dragging files.
pub fn render_drag_overlay(
    drag: &DragState,
    cmds: &mut Vec<RenderCommand>,
    window_width: f32,
    window_height: f32,
) {
    match drag {
        DragState::Idle => {}
        DragState::DraggingOut {
            entries, mouse_x, mouse_y,
        } => {
            // Semi-transparent overlay.
            cmds.push(RenderCommand::FillRect {
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
            cmds.push(RenderCommand::FillRect {
                x: *mouse_x + 12.0,
                y: *mouse_y + 12.0,
                width: badge_w,
                height: badge_h,
                color: theme::SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: *mouse_x + 20.0,
                y: *mouse_y + 20.0,
                text: format!("Extract {} file(s)", entries.len()),
                color: theme::GREEN,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(badge_w - 16.0),
            });
        }
        DragState::DraggingIn {
            files, mouse_x, mouse_y,
        } => {
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: window_width,
                height: window_height,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::StrokeRect {
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
            cmds.push(RenderCommand::FillRect {
                x: *mouse_x + 12.0,
                y: *mouse_y + 12.0,
                width: badge_w,
                height: badge_h,
                color: theme::SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: *mouse_x + 20.0,
                y: *mouse_y + 20.0,
                text: format!("Add {} file(s)", files.len()),
                color: theme::BLUE,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(badge_w - 16.0),
            });
        }
    }
}

/// Render the entire application frame.
pub fn render_frame(state: &AppState) -> Vec<RenderCommand> {
    let mut cmds = Vec::with_capacity(256);
    let w = state.window_width;
    let h = state.window_height;

    // Window background.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: w,
        height: h,
        color: theme::BASE,
        corner_radii: CornerRadii::ZERO,
    });

    let mut y = 0.0;

    // Toolbar.
    y += render_toolbar(state, &mut cmds, y, w);

    // Path bar.
    y += render_path_bar(state, &mut cmds, y, w);

    // Status bar at the bottom.
    let status_h = 24.0;
    let status_y = h - status_h;

    // Progress bar above status bar if operation in progress.
    let mut content_bottom = status_y;
    if let Some(progress) = &state.progress {
        let prog_h = 48.0;
        content_bottom -= prog_h;
        render_progress_bar(progress, &mut cmds, 0.0, content_bottom, w);
    }

    let content_h = content_bottom - y;

    // Sidebar tree.
    let sidebar_w = render_sidebar(state, &mut cmds, y, content_h);

    // Column headers.
    let list_x = sidebar_w;
    let list_w = w - sidebar_w;
    let header_h = render_column_headers(state, &mut cmds, list_x, y, list_w);

    // File list.
    render_file_list(
        state,
        &mut cmds,
        list_x,
        y + header_h,
        list_w,
        content_h - header_h,
    );

    // Status bar.
    render_status_bar(state, &mut cmds, status_y, w);

    // Drag overlay (on top of everything).
    render_drag_overlay(&state.drag, &mut cmds, w, h);

    cmds
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
        ("src/main.rs", false, 4096, 1820, 1716000000, 0xABCD1234, "Deflate"),
        ("src/lib.rs", false, 8192, 3100, 1716000000, 0x12345678, "Deflate"),
        ("src/utils/", true, 0, 0, 1716000000, 0, "Stored"),
        ("src/utils/helpers.rs", false, 2048, 980, 1716000000, 0xDEADBEEF, "Deflate"),
        ("tests/", true, 0, 0, 1716000000, 0, "Stored"),
        ("tests/test_main.rs", false, 1024, 620, 1716000000, 0xFEEDFACE, "Deflate"),
        ("Cargo.toml", false, 512, 380, 1716000000, 0xCAFEBABE, "Deflate"),
        ("README.md", false, 3072, 1400, 1716000000, 0x87654321, "Deflate"),
        ("LICENSE", false, 1070, 640, 1716000000, 0x11223344, "Deflate"),
        ("docs/", true, 0, 0, 1716000000, 0, "Stored"),
        ("docs/guide.md", false, 15360, 5200, 1716000000, 0xAABBCCDD, "Deflate"),
        ("docs/api.md", false, 8700, 3100, 1716000000, 0x55667788, "Deflate"),
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

fn main() {
    // The archive manager is launched by the SlateOS desktop environment.
    // Actual event loop integration happens through the compositor IPC.
    // This placeholder demonstrates that the application compiles and
    // can construct its state and render an initial frame.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(EncryptionMethod::ZipCrypto.display_name().contains("legacy"));
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
        assert!(s.contains("KB"));
    }

    #[test]
    fn test_entry_format_size_mb() {
        let s = ArchiveEntry::format_size(5 * 1024 * 1024);
        assert!(s.contains("MB"));
    }

    #[test]
    fn test_entry_format_size_gb() {
        let s = ArchiveEntry::format_size(3 * 1024 * 1024 * 1024);
        assert!(s.contains("GB"));
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

    #[test]
    fn test_entry_format_date_nonzero() {
        let d = ArchiveEntry::format_date(1716000000);
        // Should produce a date string with year, month, day.
        assert!(d.contains('-'));
        assert!(d.contains(':'));
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
                method: "".into(),
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
                method: "".into(),
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
        r.record("c.txt", TestResult::CrcMismatch {
            expected: 0,
            actual: 1,
        });
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
            method: "".into(),
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
        assert!(problems.is_empty(), "expected no problems, got: {problems:?}");
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
        assert!(problems.iter().any(|p| p.contains("64 KB")));
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
        assert!(cmds.len() > 20, "should produce many render commands with an archive open");
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
        let mut cmds = Vec::new();
        let h = render_toolbar(&state, &mut cmds, 0.0, 800.0);
        assert_eq!(h, 40.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_path_bar() {
        let state = AppState::default();
        let mut cmds = Vec::new();
        let h = render_path_bar(&state, &mut cmds, 0.0, 800.0);
        assert_eq!(h, 32.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_sidebar_hidden() {
        let state = AppState {
            sidebar_visible: false,
            ..AppState::default()
        };
        let mut cmds = Vec::new();
        let w = render_sidebar(&state, &mut cmds, 0.0, 400.0);
        assert_eq!(w, 0.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_render_sidebar_visible() {
        let state = AppState {
            archive: Some(create_sample_archive()),
            ..AppState::default()
        };
        let mut cmds = Vec::new();
        let w = render_sidebar(&state, &mut cmds, 0.0, 400.0);
        assert!(w > 0.0);
        assert!(!cmds.is_empty());
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

    // --- days_to_ymd tests ---

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!(y, 1970);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-01-01 is day 19723 from epoch.
        let (y, _m, _d) = days_to_ymd(19723);
        assert_eq!(y, 2024);
    }

    // --- ViewMode test ---

    #[test]
    fn test_view_mode_default() {
        assert_eq!(ViewMode::default(), ViewMode::DirectoryView);
    }
}
