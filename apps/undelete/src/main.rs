//! `SlateOS` File Recovery / Undelete Utility
//!
//! A comprehensive file recovery application that:
//! - Scans ext4 filesystem inode tables and directory entries for deleted files
//! - Detects file signatures (magic bytes) for recovery without directory entries
//! - Integrates with the OS recycle bin for easy restoration
//! - Assigns recovery confidence scores (High/Medium/Low/Unlikely)
//! - Previews metadata and first bytes of recoverable files
//! - Filters by file type, size range, deletion date, recovery confidence
//! - Supports batch recovery of multiple files to a target directory
//! - Shows scan progress with a progress bar and statistics
//! - Provides a multi-panel UI: scan panel, results list, preview panel, recovery
//! - Offers a deep scan mode for sector-by-sector file signature detection
//!
//! # Architecture
//!
//! ```text
//! InodeScanner        -- reads ext4 inode tables for deleted entries
//!       |
//!       v
//! SignatureDetector   -- magic-byte scanning for header-based recovery
//!       |
//!       v
//! RecycleBinReader    -- enumerates OS recycle bin contents
//!       |
//!       v
//! RecoveryEngine      -- orchestrates scanning, scoring, and recovery
//!       |
//!       v
//! UndeleteUI          -- multi-panel GUI via guitk
//! ```
//!
//! Uses the guitk library for UI rendering with a Catppuccin Mocha dark theme.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cognitive_complexity)]
#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::BTreeMap;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 760.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const PREVIEW_PANEL_WIDTH: f32 = 320.0;
const HEADER_HEIGHT: f32 = 56.0;
const FOOTER_HEIGHT: f32 = 48.0;
const PADDING: f32 = 12.0;
const ITEM_HEIGHT: f32 = 56.0;
const CORNER_RADIUS: f32 = 8.0;
const SMALL_RADIUS: f32 = 4.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const FONT_SIZE_TITLE: f32 = 20.0;
const BUTTON_WIDTH: f32 = 120.0;
const BUTTON_HEIGHT: f32 = 32.0;
const CHECKBOX_SIZE: f32 = 16.0;
const PROGRESS_HEIGHT: f32 = 8.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;

// ============================================================================
// File type / signature definitions
// ============================================================================

/// Known file types that can be identified by magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileSignatureKind {
    Jpeg,
    Png,
    Gif,
    Bmp,
    Webp,
    Pdf,
    Zip,
    Gzip,
    SevenZip,
    Rar,
    Mp3,
    Flac,
    Ogg,
    Wav,
    Mp4,
    Avi,
    Mkv,
    Doc,
    Docx,
    Xls,
    Ppt,
    Elf,
    Sqlite,
    Tar,
    Xml,
    Html,
    Unknown,
}

impl FileSignatureKind {
    pub const ALL: &'static [Self] = &[
        Self::Jpeg,
        Self::Png,
        Self::Gif,
        Self::Bmp,
        Self::Webp,
        Self::Pdf,
        Self::Zip,
        Self::Gzip,
        Self::SevenZip,
        Self::Rar,
        Self::Mp3,
        Self::Flac,
        Self::Ogg,
        Self::Wav,
        Self::Mp4,
        Self::Avi,
        Self::Mkv,
        Self::Doc,
        Self::Docx,
        Self::Xls,
        Self::Ppt,
        Self::Elf,
        Self::Sqlite,
        Self::Tar,
        Self::Xml,
        Self::Html,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG Image",
            Self::Png => "PNG Image",
            Self::Gif => "GIF Image",
            Self::Bmp => "BMP Image",
            Self::Webp => "WebP Image",
            Self::Pdf => "PDF Document",
            Self::Zip => "ZIP Archive",
            Self::Gzip => "GZIP Archive",
            Self::SevenZip => "7-Zip Archive",
            Self::Rar => "RAR Archive",
            Self::Mp3 => "MP3 Audio",
            Self::Flac => "FLAC Audio",
            Self::Ogg => "OGG Audio",
            Self::Wav => "WAV Audio",
            Self::Mp4 => "MP4 Video",
            Self::Avi => "AVI Video",
            Self::Mkv => "MKV Video",
            Self::Doc => "Word Document (legacy)",
            Self::Docx => "Word Document",
            Self::Xls => "Excel Spreadsheet",
            Self::Ppt => "PowerPoint Presentation",
            Self::Elf => "ELF Executable",
            Self::Sqlite => "SQLite Database",
            Self::Tar => "TAR Archive",
            Self::Xml => "XML Document",
            Self::Html => "HTML Document",
            Self::Unknown => "Unknown",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Webp => "webp",
            Self::Pdf => "pdf",
            Self::Zip => "zip",
            Self::Gzip => "gz",
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Wav => "wav",
            Self::Mp4 => "mp4",
            Self::Avi => "avi",
            Self::Mkv => "mkv",
            Self::Doc => "doc",
            Self::Docx => "docx",
            Self::Xls => "xls",
            Self::Ppt => "ppt",
            Self::Elf => "elf",
            Self::Sqlite => "sqlite",
            Self::Tar => "tar",
            Self::Xml => "xml",
            Self::Html => "html",
            Self::Unknown => "bin",
        }
    }

    /// Category grouping for filtering.
    pub fn category(self) -> FileCategory {
        match self {
            Self::Jpeg | Self::Png | Self::Gif | Self::Bmp | Self::Webp => FileCategory::Image,
            Self::Pdf | Self::Doc | Self::Docx | Self::Xls | Self::Ppt | Self::Xml | Self::Html => {
                FileCategory::Document
            }
            Self::Zip | Self::Gzip | Self::SevenZip | Self::Rar | Self::Tar => {
                FileCategory::Archive
            }
            Self::Mp3 | Self::Flac | Self::Ogg | Self::Wav => FileCategory::Audio,
            Self::Mp4 | Self::Avi | Self::Mkv => FileCategory::Video,
            Self::Elf | Self::Sqlite => FileCategory::Application,
            Self::Unknown => FileCategory::Other,
        }
    }

    /// Color for display in the UI.
    pub fn color(self) -> Color {
        match self.category() {
            FileCategory::Image => TEAL,
            FileCategory::Document => BLUE,
            FileCategory::Archive => PEACH,
            FileCategory::Audio => MAUVE,
            FileCategory::Video => LAVENDER,
            FileCategory::Application => GREEN,
            FileCategory::Other => SUBTEXT0,
        }
    }
}

/// Magic bytes signature for file detection.
#[derive(Debug, Clone)]
pub struct FileSignature {
    pub kind: FileSignatureKind,
    /// Byte offset from sector start where the signature appears.
    pub offset: usize,
    /// The magic byte pattern.
    pub magic: Vec<u8>,
    /// Optional secondary pattern to confirm (e.g. JFIF after JPEG header).
    pub secondary: Option<(usize, Vec<u8>)>,
}

impl FileSignature {
    pub fn new(kind: FileSignatureKind, offset: usize, magic: &[u8]) -> Self {
        Self {
            kind,
            offset,
            magic: magic.to_vec(),
            secondary: None,
        }
    }

    pub fn with_secondary(mut self, offset: usize, pattern: &[u8]) -> Self {
        self.secondary = Some((offset, pattern.to_vec()));
        self
    }

    /// Check if a data buffer matches this signature at the expected offset.
    pub fn matches(&self, data: &[u8]) -> bool {
        let end = self.offset.saturating_add(self.magic.len());
        if data.len() < end {
            return false;
        }
        let slice = data.get(self.offset..end);
        let primary_ok = slice == Some(self.magic.as_slice());
        if !primary_ok {
            return false;
        }
        if let Some((sec_off, ref sec_magic)) = self.secondary {
            let sec_end = sec_off.saturating_add(sec_magic.len());
            if data.len() < sec_end {
                return false;
            }
            data.get(sec_off..sec_end) == Some(sec_magic.as_slice())
        } else {
            true
        }
    }
}

/// Build the database of known file signatures.
pub fn build_signature_database() -> Vec<FileSignature> {
    vec![
        // JPEG: FF D8 FF
        FileSignature::new(FileSignatureKind::Jpeg, 0, &[0xFF, 0xD8, 0xFF]),
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        FileSignature::new(
            FileSignatureKind::Png,
            0,
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        ),
        // GIF87a / GIF89a
        FileSignature::new(FileSignatureKind::Gif, 0, b"GIF87a"),
        FileSignature::new(FileSignatureKind::Gif, 0, b"GIF89a"),
        // BMP: BM
        FileSignature::new(FileSignatureKind::Bmp, 0, b"BM"),
        // WebP: RIFF....WEBP
        FileSignature::new(FileSignatureKind::Webp, 0, b"RIFF").with_secondary(8, b"WEBP"),
        // PDF: %PDF
        FileSignature::new(FileSignatureKind::Pdf, 0, b"%PDF"),
        // ZIP (also DOCX/XLSX/PPTX via secondary check)
        FileSignature::new(FileSignatureKind::Zip, 0, &[0x50, 0x4B, 0x03, 0x04]),
        // DOCX (ZIP + word/ content)
        FileSignature::new(FileSignatureKind::Docx, 0, &[0x50, 0x4B, 0x03, 0x04])
            .with_secondary(30, b"word/"),
        // GZIP: 1F 8B
        FileSignature::new(FileSignatureKind::Gzip, 0, &[0x1F, 0x8B]),
        // 7-Zip: 37 7A BC AF 27 1C
        FileSignature::new(
            FileSignatureKind::SevenZip,
            0,
            &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C],
        ),
        // RAR: Rar!
        FileSignature::new(FileSignatureKind::Rar, 0, b"Rar!\x1a\x07"),
        // MP3 with ID3 tag
        FileSignature::new(FileSignatureKind::Mp3, 0, b"ID3"),
        // MP3 sync word (frame header)
        FileSignature::new(FileSignatureKind::Mp3, 0, &[0xFF, 0xFB]),
        // FLAC: fLaC
        FileSignature::new(FileSignatureKind::Flac, 0, b"fLaC"),
        // OGG: OggS
        FileSignature::new(FileSignatureKind::Ogg, 0, b"OggS"),
        // WAV: RIFF....WAVE
        FileSignature::new(FileSignatureKind::Wav, 0, b"RIFF").with_secondary(8, b"WAVE"),
        // MP4: various boxes (ftyp at offset 4)
        FileSignature::new(FileSignatureKind::Mp4, 4, b"ftyp"),
        // AVI: RIFF....AVI
        FileSignature::new(FileSignatureKind::Avi, 0, b"RIFF").with_secondary(8, b"AVI "),
        // MKV: 1A 45 DF A3 (EBML header)
        FileSignature::new(FileSignatureKind::Mkv, 0, &[0x1A, 0x45, 0xDF, 0xA3]),
        // DOC (OLE2 Compound File): D0 CF 11 E0 A1 B1 1A E1
        FileSignature::new(
            FileSignatureKind::Doc,
            0,
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        ),
        // XLS: same OLE2 header
        FileSignature::new(
            FileSignatureKind::Xls,
            0,
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        ),
        // PPT: same OLE2 header
        FileSignature::new(
            FileSignatureKind::Ppt,
            0,
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        ),
        // ELF: 7F 45 4C 46
        FileSignature::new(FileSignatureKind::Elf, 0, &[0x7F, 0x45, 0x4C, 0x46]),
        // SQLite: "SQLite format 3\0"
        FileSignature::new(FileSignatureKind::Sqlite, 0, b"SQLite format 3\0"),
        // TAR (ustar at offset 257)
        FileSignature::new(FileSignatureKind::Tar, 257, b"ustar"),
        // XML: <?xml
        FileSignature::new(FileSignatureKind::Xml, 0, b"<?xml"),
        // HTML: <!DOCTYPE html> or <html (case-insensitive handled separately)
        FileSignature::new(FileSignatureKind::Html, 0, b"<!DOCTYPE html"),
        FileSignature::new(FileSignatureKind::Html, 0, b"<!doctype html"),
        FileSignature::new(FileSignatureKind::Html, 0, b"<html"),
    ]
}

// ============================================================================
// File category (for filtering)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileCategory {
    Image,
    Document,
    Archive,
    Audio,
    Video,
    Application,
    Other,
}

impl FileCategory {
    pub const ALL: &'static [Self] = &[
        Self::Image,
        Self::Document,
        Self::Archive,
        Self::Audio,
        Self::Video,
        Self::Application,
        Self::Other,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Image => "Images",
            Self::Document => "Documents",
            Self::Archive => "Archives",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Application => "Applications",
            Self::Other => "Other",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Image => TEAL,
            Self::Document => BLUE,
            Self::Archive => PEACH,
            Self::Audio => MAUVE,
            Self::Video => LAVENDER,
            Self::Application => GREEN,
            Self::Other => SUBTEXT0,
        }
    }
}

// ============================================================================
// Recovery confidence levels
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecoveryConfidence {
    /// Recycle bin entry or intact inode with all data blocks allocated.
    High,
    /// Inode present but some blocks may be overwritten.
    Medium,
    /// File signature detected but no inode; contiguous recovery attempted.
    Low,
    /// Fragmented or heavily overwritten; partial recovery at best.
    Unlikely,
}

impl RecoveryConfidence {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::Unlikely => "Unlikely",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::High => GREEN,
            Self::Medium => YELLOW,
            Self::Low => PEACH,
            Self::Unlikely => RED,
        }
    }

    pub fn percentage_range(self) -> (u8, u8) {
        match self {
            Self::High => (90, 100),
            Self::Medium => (50, 89),
            Self::Low => (20, 49),
            Self::Unlikely => (0, 19),
        }
    }
}

// ============================================================================
// Deletion source
// ============================================================================

/// How the file came to be deleted / discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionSource {
    /// Found in the recycle bin with full metadata.
    RecycleBin,
    /// Found via ext4 inode table scan (deleted inode).
    InodeScan,
    /// Found via magic-byte deep scan (no inode reference).
    SignatureScan,
    /// Found via directory entry remnants.
    DirectoryRemnant,
}

impl DeletionSource {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::RecycleBin => "Recycle Bin",
            Self::InodeScan => "Inode Scan",
            Self::SignatureScan => "Signature Scan",
            Self::DirectoryRemnant => "Directory Remnant",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::RecycleBin => "File is in the recycle bin with full metadata intact",
            Self::InodeScan => "Deleted inode found in ext4 inode table",
            Self::SignatureScan => "File header detected via magic byte scan",
            Self::DirectoryRemnant => "Directory entry remnant references this file",
        }
    }
}

// ============================================================================
// Simulated ext4 structures
// ============================================================================

/// Simulated ext4 inode (the fields relevant to recovery).
#[derive(Debug, Clone)]
pub struct Ext4Inode {
    pub inode_number: u64,
    pub file_size: u64,
    pub block_count: u64,
    /// 0 = deleted, nonzero = active
    pub link_count: u16,
    pub file_type: InodeFileType,
    pub permissions: u16,
    pub uid: u32,
    pub gid: u32,
    /// Unix timestamp of last access.
    pub access_time: u64,
    /// Unix timestamp of last modification.
    pub modify_time: u64,
    /// Unix timestamp of deletion (0 if not deleted).
    pub delete_time: u64,
    /// Direct block pointers (first 12).
    pub direct_blocks: Vec<u64>,
    /// Single-indirect block pointer.
    pub indirect_block: u64,
    /// Double-indirect block pointer.
    pub double_indirect_block: u64,
    /// Whether the inode data blocks have been reallocated.
    pub blocks_reallocated: bool,
}

impl Ext4Inode {
    pub fn new_deleted(inode_number: u64, file_size: u64) -> Self {
        Self {
            inode_number,
            file_size,
            block_count: (file_size.saturating_add(4095)) / 4096,
            link_count: 0,
            file_type: InodeFileType::Regular,
            permissions: 0o644,
            uid: 1000,
            gid: 1000,
            access_time: 1_700_000_000,
            modify_time: 1_700_000_000,
            delete_time: 1_700_100_000,
            direct_blocks: Vec::new(),
            indirect_block: 0,
            double_indirect_block: 0,
            blocks_reallocated: false,
        }
    }

    pub fn with_delete_time(mut self, ts: u64) -> Self {
        self.delete_time = ts;
        self
    }

    pub fn with_modify_time(mut self, ts: u64) -> Self {
        self.modify_time = ts;
        self
    }

    pub fn with_blocks_reallocated(mut self, reallocated: bool) -> Self {
        self.blocks_reallocated = reallocated;
        self
    }

    pub fn with_file_type(mut self, ft: InodeFileType) -> Self {
        self.file_type = ft;
        self
    }

    pub fn with_direct_blocks(mut self, blocks: Vec<u64>) -> Self {
        self.direct_blocks = blocks;
        self
    }

    pub fn is_deleted(&self) -> bool {
        self.delete_time > 0 && self.link_count == 0
    }

    /// Assess recovery likelihood based on inode state.
    pub fn recovery_confidence(&self) -> RecoveryConfidence {
        if !self.is_deleted() {
            return RecoveryConfidence::High;
        }
        if self.blocks_reallocated {
            return RecoveryConfidence::Unlikely;
        }
        if !self.direct_blocks.is_empty() && self.file_size > 0 {
            RecoveryConfidence::Medium
        } else if self.file_size > 0 {
            RecoveryConfidence::Low
        } else {
            RecoveryConfidence::Unlikely
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeFileType {
    Regular,
    Directory,
    Symlink,
    Socket,
    Fifo,
    BlockDevice,
    CharDevice,
}

impl InodeFileType {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Regular => "Regular file",
            Self::Directory => "Directory",
            Self::Symlink => "Symbolic link",
            Self::Socket => "Socket",
            Self::Fifo => "Named pipe",
            Self::BlockDevice => "Block device",
            Self::CharDevice => "Character device",
        }
    }
}

/// A simulated ext4 directory entry (deleted entries retain a reference to the
/// inode and partial filename).
#[derive(Debug, Clone)]
pub struct Ext4DirEntry {
    pub inode_number: u64,
    pub name: String,
    pub file_type: InodeFileType,
    pub deleted: bool,
}

/// Simulated ext4 block group descriptor (for scanning).
#[derive(Debug, Clone)]
pub struct BlockGroupDescriptor {
    pub group_number: u32,
    pub inode_table_block: u64,
    pub inode_count: u32,
    pub free_inodes: u32,
    pub block_bitmap_block: u64,
    pub inode_bitmap_block: u64,
}

impl BlockGroupDescriptor {
    pub fn new(group_number: u32) -> Self {
        let base_block = u64::from(group_number).saturating_mul(32768);
        Self {
            group_number,
            inode_table_block: base_block.saturating_add(3),
            inode_count: 8192,
            free_inodes: 1024,
            block_bitmap_block: base_block.saturating_add(1),
            inode_bitmap_block: base_block.saturating_add(2),
        }
    }
}

// ============================================================================
// Partition / device representation
// ============================================================================

#[derive(Debug, Clone)]
pub struct Partition {
    pub name: String,
    pub device_path: String,
    pub mount_point: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub block_groups: Vec<BlockGroupDescriptor>,
}

impl Partition {
    pub fn new(name: &str, device: &str, mount: &str, total: u64, free: u64) -> Self {
        let num_groups = (total / (32768 * 4096)).max(1) as u32;
        let mut groups = Vec::new();
        for i in 0..num_groups.min(16) {
            groups.push(BlockGroupDescriptor::new(i));
        }
        Self {
            name: name.to_string(),
            device_path: device.to_string(),
            mount_point: mount.to_string(),
            filesystem: String::from("ext4"),
            total_bytes: total,
            free_bytes: free,
            block_groups: groups,
        }
    }

    pub fn usage_percent(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        let used = self.total_bytes.saturating_sub(self.free_bytes);
        (used as f32) / (self.total_bytes as f32) * 100.0
    }
}

/// Create a set of simulated partitions for the UI.
pub fn simulated_partitions() -> Vec<Partition> {
    vec![
        Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        ),
        Partition::new(
            "/dev/sda2",
            "/dev/sda2",
            "/home",
            1_000_000_000_000,
            600_000_000_000,
        ),
        Partition::new(
            "/dev/sdb1",
            "/dev/sdb1",
            "/data",
            2_000_000_000_000,
            1_500_000_000_000,
        ),
    ]
}

// ============================================================================
// Recycle bin entry
// ============================================================================

/// A file in the recycle bin. Contains full metadata for easy restore.
#[derive(Debug, Clone)]
pub struct RecycleBinEntry {
    pub id: u64,
    pub original_path: String,
    pub recycle_path: String,
    pub file_size: u64,
    pub delete_timestamp: u64,
    pub file_type: FileSignatureKind,
}

impl RecycleBinEntry {
    pub fn new(
        id: u64,
        original_path: &str,
        recycle_path: &str,
        file_size: u64,
        delete_ts: u64,
        file_type: FileSignatureKind,
    ) -> Self {
        Self {
            id,
            original_path: original_path.to_string(),
            recycle_path: recycle_path.to_string(),
            file_size,
            delete_timestamp: delete_ts,
            file_type,
        }
    }
}

// ============================================================================
// Recoverable file (unified representation)
// ============================================================================

/// A potentially recoverable file, regardless of how it was discovered.
#[derive(Debug, Clone)]
pub struct RecoverableFile {
    pub id: u64,
    /// Original path (if known from dir entry or recycle bin).
    pub original_path: Option<String>,
    /// Filename (may be partial or generated from signature).
    pub filename: String,
    pub file_size: u64,
    pub file_type: FileSignatureKind,
    pub confidence: RecoveryConfidence,
    pub source: DeletionSource,
    /// Inode number (if found via inode scan).
    pub inode_number: Option<u64>,
    /// Sector/offset where the file header was found (signature scan).
    pub disk_offset: Option<u64>,
    /// Deletion timestamp (unix epoch seconds), 0 if unknown.
    pub delete_time: u64,
    /// Modification timestamp (unix epoch seconds).
    pub modify_time: u64,
    /// First N bytes of the file data (for preview).
    pub preview_bytes: Vec<u8>,
    /// Whether the user has selected this file for batch recovery.
    pub selected: bool,
    /// Partition/device this file was found on.
    pub partition_name: String,
    /// Recovery percentage estimate (0-100).
    pub recovery_percent: u8,
}

impl RecoverableFile {
    pub fn from_recycle_bin(entry: &RecycleBinEntry) -> Self {
        let filename = entry
            .original_path
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string();
        Self {
            id: entry.id,
            original_path: Some(entry.original_path.clone()),
            filename,
            file_size: entry.file_size,
            file_type: entry.file_type,
            confidence: RecoveryConfidence::High,
            source: DeletionSource::RecycleBin,
            inode_number: None,
            disk_offset: None,
            delete_time: entry.delete_timestamp,
            modify_time: entry.delete_timestamp.saturating_sub(86400),
            preview_bytes: Vec::new(),
            selected: false,
            partition_name: String::from("/dev/sda1"),
            recovery_percent: 100,
        }
    }

    pub fn from_inode(inode: &Ext4Inode, dir_entry: Option<&Ext4DirEntry>) -> Self {
        let confidence = inode.recovery_confidence();
        let (lo, hi) = confidence.percentage_range();
        let recovery_percent = (lo.saturating_add(hi)) / 2;

        let filename = dir_entry.map_or_else(
            || format!("inode_{}", inode.inode_number),
            |d| d.name.clone(),
        );

        let original_path = dir_entry.map(|d| format!("/recovered/{}", d.name));

        Self {
            id: inode.inode_number,
            original_path,
            filename,
            file_size: inode.file_size,
            file_type: FileSignatureKind::Unknown,
            confidence,
            source: if dir_entry.is_some() {
                DeletionSource::DirectoryRemnant
            } else {
                DeletionSource::InodeScan
            },
            inode_number: Some(inode.inode_number),
            disk_offset: None,
            delete_time: inode.delete_time,
            modify_time: inode.modify_time,
            preview_bytes: Vec::new(),
            selected: false,
            partition_name: String::from("/dev/sda1"),
            recovery_percent,
        }
    }

    pub fn from_signature(
        id: u64,
        kind: FileSignatureKind,
        offset: u64,
        estimated_size: u64,
    ) -> Self {
        Self {
            id,
            original_path: None,
            filename: format!("recovered_{:08x}.{}", offset, kind.extension()),
            file_size: estimated_size,
            file_type: kind,
            confidence: RecoveryConfidence::Low,
            source: DeletionSource::SignatureScan,
            inode_number: None,
            disk_offset: Some(offset),
            delete_time: 0,
            modify_time: 0,
            preview_bytes: Vec::new(),
            selected: false,
            partition_name: String::from("/dev/sda1"),
            recovery_percent: 35,
        }
    }

    pub fn category(&self) -> FileCategory {
        self.file_type.category()
    }

    /// Human-readable size string.
    pub fn size_display(&self) -> String {
        format_size(self.file_size)
    }

    /// Human-readable deletion time.
    pub fn delete_time_display(&self) -> String {
        if self.delete_time == 0 {
            return String::from("Unknown");
        }
        format_timestamp(self.delete_time)
    }
}

// ============================================================================
// Signature detector
// ============================================================================

/// Scans raw byte data for known file signatures. Used for deep scan mode
/// where no filesystem metadata is available.
pub struct SignatureDetector {
    signatures: Vec<FileSignature>,
}

impl Default for SignatureDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl SignatureDetector {
    pub fn new() -> Self {
        Self {
            signatures: build_signature_database(),
        }
    }

    pub fn with_signatures(signatures: Vec<FileSignature>) -> Self {
        Self { signatures }
    }

    /// Check a single data buffer against all known signatures.
    /// Returns all matching signature kinds.
    pub fn detect(&self, data: &[u8]) -> Vec<FileSignatureKind> {
        let mut found = Vec::new();
        for sig in &self.signatures {
            if sig.matches(data) && !found.contains(&sig.kind) {
                found.push(sig.kind);
            }
        }
        found
    }

    /// Detect the single best-matching signature (most specific first).
    pub fn detect_best(&self, data: &[u8]) -> Option<FileSignatureKind> {
        // Prefer signatures with secondary patterns (more specific).
        for sig in &self.signatures {
            if sig.secondary.is_some() && sig.matches(data) {
                return Some(sig.kind);
            }
        }
        for sig in &self.signatures {
            if sig.secondary.is_none() && sig.matches(data) {
                return Some(sig.kind);
            }
        }
        None
    }

    /// Scan a large data buffer sector by sector. Returns (offset, kind) for
    /// each signature found.
    pub fn scan_sectors(&self, data: &[u8], sector_size: usize) -> Vec<(u64, FileSignatureKind)> {
        let mut results = Vec::new();
        if sector_size == 0 {
            return results;
        }
        let mut offset: usize = 0;
        while offset.saturating_add(sector_size) <= data.len() {
            let sector = data.get(offset..offset.saturating_add(sector_size));
            if let Some(sector_data) = sector
                && let Some(kind) = self.detect_best(sector_data)
            {
                results.push((offset as u64, kind));
            }
            offset = offset.saturating_add(sector_size);
        }
        results
    }

    pub fn signature_count(&self) -> usize {
        self.signatures.len()
    }
}

// ============================================================================
// Inode scanner
// ============================================================================

/// Scans simulated ext4 inode tables for deleted inodes.
pub struct InodeScanner {
    deleted_inodes: Vec<Ext4Inode>,
    dir_entries: Vec<Ext4DirEntry>,
    scanned_groups: u32,
    total_groups: u32,
}

impl Default for InodeScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl InodeScanner {
    pub fn new() -> Self {
        Self {
            deleted_inodes: Vec::new(),
            dir_entries: Vec::new(),
            scanned_groups: 0,
            total_groups: 0,
        }
    }

    /// Simulate scanning a partition's block groups for deleted inodes.
    pub fn scan_partition(&mut self, partition: &Partition) {
        self.deleted_inodes.clear();
        self.dir_entries.clear();
        self.total_groups = partition.block_groups.len() as u32;
        self.scanned_groups = 0;

        for bg in &partition.block_groups {
            self.scan_block_group(bg);
            self.scanned_groups = self.scanned_groups.saturating_add(1);
        }
    }

    fn scan_block_group(&mut self, bg: &BlockGroupDescriptor) {
        // Simulate finding deleted inodes in this block group.
        // In a real implementation this would read the inode bitmap and
        // inode table from disk.
        let base_inode = u64::from(bg.group_number).saturating_mul(8192);
        let num_deleted = bg.free_inodes.min(5);

        for i in 0..num_deleted {
            let inode_num = base_inode.saturating_add(u64::from(i)).saturating_add(100);
            let file_size = (u64::from(i).saturating_add(1))
                .saturating_mul(4096)
                .saturating_mul((u64::from(bg.group_number).saturating_add(1)) % 10);

            if file_size == 0 {
                continue;
            }

            let has_blocks = i % 3 != 2;
            let blocks_reallocated = i % 5 == 4;

            let mut inode = Ext4Inode::new_deleted(inode_num, file_size)
                .with_delete_time(
                    1_700_000_000_u64
                        .saturating_add(u64::from(bg.group_number).saturating_mul(86400))
                        .saturating_add(u64::from(i).saturating_mul(3600)),
                )
                .with_modify_time(
                    1_699_900_000_u64.saturating_add(u64::from(i).saturating_mul(7200)),
                )
                .with_blocks_reallocated(blocks_reallocated);

            if has_blocks {
                let blocks: Vec<u64> = (0..4)
                    .map(|b| {
                        bg.inode_table_block
                            .saturating_add(1000)
                            .saturating_add(u64::from(i).saturating_mul(10))
                            .saturating_add(b)
                    })
                    .collect();
                inode = inode.with_direct_blocks(blocks);
            }

            // Simulate a directory entry for some of the inodes.
            if i % 2 == 0 {
                let ext = match i % 4 {
                    0 => "txt",
                    1 => "jpg",
                    2 => "pdf",
                    _ => "bin",
                };
                let name = format!("file_{inode_num}.{ext}");
                self.dir_entries.push(Ext4DirEntry {
                    inode_number: inode_num,
                    name,
                    file_type: InodeFileType::Regular,
                    deleted: true,
                });
            }

            self.deleted_inodes.push(inode);
        }
    }

    pub fn deleted_inodes(&self) -> &[Ext4Inode] {
        &self.deleted_inodes
    }

    pub fn dir_entries(&self) -> &[Ext4DirEntry] {
        &self.dir_entries
    }

    /// Find the directory entry associated with an inode, if any.
    pub fn find_dir_entry(&self, inode_number: u64) -> Option<&Ext4DirEntry> {
        self.dir_entries
            .iter()
            .find(|e| e.inode_number == inode_number)
    }

    pub fn scan_progress(&self) -> f32 {
        if self.total_groups == 0 {
            return 0.0;
        }
        (self.scanned_groups as f32) / (self.total_groups as f32)
    }

    pub fn scanned_groups(&self) -> u32 {
        self.scanned_groups
    }

    pub fn total_groups(&self) -> u32 {
        self.total_groups
    }
}

// ============================================================================
// Recycle bin reader
// ============================================================================

/// Reads the OS recycle bin, returning entries that can be restored.
pub struct RecycleBinReader {
    entries: Vec<RecycleBinEntry>,
}

impl Default for RecycleBinReader {
    fn default() -> Self {
        Self::new()
    }
}

impl RecycleBinReader {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Simulate reading the recycle bin contents.
    pub fn scan(&mut self) {
        self.entries.clear();
        self.entries = simulated_recycle_bin();
    }

    pub fn entries(&self) -> &[RecycleBinEntry] {
        &self.entries
    }

    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.file_size).sum()
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Find entry by ID.
    pub fn find(&self, id: u64) -> Option<&RecycleBinEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

fn simulated_recycle_bin() -> Vec<RecycleBinEntry> {
    vec![
        RecycleBinEntry::new(
            10001,
            "/home/user/Documents/report_q4.pdf",
            "/home/user/.trash/10001_report_q4.pdf",
            245_760,
            1_700_200_000,
            FileSignatureKind::Pdf,
        ),
        RecycleBinEntry::new(
            10002,
            "/home/user/Photos/vacation_001.jpg",
            "/home/user/.trash/10002_vacation_001.jpg",
            3_145_728,
            1_700_180_000,
            FileSignatureKind::Jpeg,
        ),
        RecycleBinEntry::new(
            10003,
            "/home/user/Music/song.mp3",
            "/home/user/.trash/10003_song.mp3",
            5_242_880,
            1_700_150_000,
            FileSignatureKind::Mp3,
        ),
        RecycleBinEntry::new(
            10004,
            "/home/user/Documents/notes.txt",
            "/home/user/.trash/10004_notes.txt",
            1_024,
            1_700_100_000,
            FileSignatureKind::Unknown,
        ),
        RecycleBinEntry::new(
            10005,
            "/home/user/Downloads/archive.zip",
            "/home/user/.trash/10005_archive.zip",
            52_428_800,
            1_700_050_000,
            FileSignatureKind::Zip,
        ),
        RecycleBinEntry::new(
            10006,
            "/home/user/Photos/screenshot.png",
            "/home/user/.trash/10006_screenshot.png",
            524_288,
            1_700_000_000,
            FileSignatureKind::Png,
        ),
        RecycleBinEntry::new(
            10007,
            "/home/user/Videos/clip.mp4",
            "/home/user/.trash/10007_clip.mp4",
            104_857_600,
            1_699_900_000,
            FileSignatureKind::Mp4,
        ),
        RecycleBinEntry::new(
            10008,
            "/home/user/Documents/spreadsheet.xls",
            "/home/user/.trash/10008_spreadsheet.xls",
            131_072,
            1_699_800_000,
            FileSignatureKind::Xls,
        ),
    ]
}

// ============================================================================
// Recovery engine
// ============================================================================

/// Scan mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Quick scan: recycle bin + inode table scan only.
    Quick,
    /// Deep scan: adds sector-by-sector signature scanning.
    Deep,
}

impl ScanMode {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Quick => "Quick Scan",
            Self::Deep => "Deep Scan",
        }
    }
}

/// Progress tracking for scan operations.
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub phase_progress: f32,
    pub overall_progress: f32,
    pub files_found: usize,
    pub bytes_scanned: u64,
    pub total_bytes: u64,
    pub elapsed_seconds: u32,
}

impl Default for ScanProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            phase: ScanPhase::Idle,
            phase_progress: 0.0,
            overall_progress: 0.0,
            files_found: 0,
            bytes_scanned: 0,
            total_bytes: 0,
            elapsed_seconds: 0,
        }
    }

    pub fn estimated_remaining_seconds(&self) -> Option<u32> {
        if self.overall_progress <= 0.01 || self.elapsed_seconds == 0 {
            return None;
        }
        let remaining_frac = 1.0 - self.overall_progress;
        let rate = self.overall_progress / (self.elapsed_seconds as f32);
        if rate <= 0.0 {
            return None;
        }
        Some((remaining_frac / rate) as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    Idle,
    RecycleBin,
    InodeScan,
    DeepScan,
    Analyzing,
    Complete,
}

impl ScanPhase {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::RecycleBin => "Scanning Recycle Bin",
            Self::InodeScan => "Scanning Inode Tables",
            Self::DeepScan => "Deep Sector Scan",
            Self::Analyzing => "Analyzing Results",
            Self::Complete => "Scan Complete",
        }
    }
}

/// The main recovery engine that orchestrates all scan types.
pub struct RecoveryEngine {
    pub files: Vec<RecoverableFile>,
    pub progress: ScanProgress,
    pub scan_mode: ScanMode,
    inode_scanner: InodeScanner,
    recycle_reader: RecycleBinReader,
    signature_detector: SignatureDetector,
    next_id: u64,
}

impl Default for RecoveryEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryEngine {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            progress: ScanProgress::new(),
            scan_mode: ScanMode::Quick,
            inode_scanner: InodeScanner::new(),
            recycle_reader: RecycleBinReader::new(),
            signature_detector: SignatureDetector::new(),
            next_id: 20000,
        }
    }

    /// Run a full scan on the given partition.
    pub fn scan(&mut self, partition: &Partition, mode: ScanMode) {
        self.files.clear();
        self.scan_mode = mode;
        self.progress = ScanProgress::new();
        self.progress.total_bytes = partition.total_bytes;

        // Phase 1: Recycle bin
        self.progress.phase = ScanPhase::RecycleBin;
        self.progress.phase_progress = 0.0;
        self.scan_recycle_bin(partition);
        self.progress.phase_progress = 1.0;
        self.progress.overall_progress = 0.2;

        // Phase 2: Inode scan
        self.progress.phase = ScanPhase::InodeScan;
        self.progress.phase_progress = 0.0;
        self.scan_inodes(partition);
        self.progress.phase_progress = 1.0;
        self.progress.overall_progress = if mode == ScanMode::Deep { 0.4 } else { 0.8 };

        // Phase 3: Deep scan (if enabled)
        if mode == ScanMode::Deep {
            self.progress.phase = ScanPhase::DeepScan;
            self.progress.phase_progress = 0.0;
            self.scan_deep(partition);
            self.progress.phase_progress = 1.0;
            self.progress.overall_progress = 0.9;
        }

        // Phase 4: Analysis
        self.progress.phase = ScanPhase::Analyzing;
        self.deduplicate_results();
        self.progress.files_found = self.files.len();

        // Done
        self.progress.phase = ScanPhase::Complete;
        self.progress.overall_progress = 1.0;
        self.progress.phase_progress = 1.0;
        self.progress.elapsed_seconds = if mode == ScanMode::Deep { 45 } else { 12 };
    }

    fn scan_recycle_bin(&mut self, partition: &Partition) {
        self.recycle_reader.scan();
        for entry in self.recycle_reader.entries() {
            let mut file = RecoverableFile::from_recycle_bin(entry);
            file.partition_name.clone_from(&partition.name);
            self.files.push(file);
        }
    }

    fn scan_inodes(&mut self, partition: &Partition) {
        self.inode_scanner.scan_partition(partition);
        for inode in self.inode_scanner.deleted_inodes() {
            let dir_entry = self.inode_scanner.find_dir_entry(inode.inode_number);
            let mut file = RecoverableFile::from_inode(inode, dir_entry);
            file.partition_name.clone_from(&partition.name);
            self.files.push(file);
        }
        // Approximate bytes of inode table scanned: block_groups * 8192 inodes/group * 256 B/inode.
        self.progress.bytes_scanned = (partition.block_groups.len() as u64)
            .saturating_mul(8192)
            .saturating_mul(256);
    }

    fn scan_deep(&mut self, partition: &Partition) {
        // Simulate finding some files via signature scanning.
        // In a real implementation this would read raw sectors from the device.
        let simulated_finds: Vec<(u64, FileSignatureKind, u64)> = vec![
            (0x0010_0000, FileSignatureKind::Jpeg, 2_097_152),
            (0x0030_0000, FileSignatureKind::Png, 524_288),
            (0x0050_0000, FileSignatureKind::Pdf, 1_048_576),
            (0x0080_0000, FileSignatureKind::Mp3, 4_194_304),
            (0x00A0_0000, FileSignatureKind::Zip, 8_388_608),
            (0x00C0_0000, FileSignatureKind::Doc, 262_144),
            (0x00E0_0000, FileSignatureKind::Elf, 131_072),
            (0x0100_0000, FileSignatureKind::Flac, 16_777_216),
            (0x0200_0000, FileSignatureKind::Gif, 65_536),
            (0x0300_0000, FileSignatureKind::Wav, 10_485_760),
        ];

        for (offset, kind, size) in simulated_finds {
            let mut file = RecoverableFile::from_signature(self.next_id, kind, offset, size);
            file.partition_name.clone_from(&partition.name);
            self.next_id = self.next_id.saturating_add(1);
            self.files.push(file);
        }

        self.progress.bytes_scanned = partition.total_bytes;
    }

    /// Remove duplicate entries (e.g., same inode found via both inode scan and
    /// recycle bin). Prefers the source with higher confidence.
    fn deduplicate_results(&mut self) {
        // Sort by confidence descending so we keep the best entry.
        self.files.sort_by(|a, b| {
            a.confidence
                .cmp(&b.confidence)
                .then_with(|| a.id.cmp(&b.id))
        });
        // Remove duplicates based on inode number or disk offset.
        let mut seen_inodes: Vec<u64> = Vec::new();
        let mut seen_offsets: Vec<u64> = Vec::new();
        self.files.retain(|f| {
            if let Some(ino) = f.inode_number {
                if seen_inodes.contains(&ino) {
                    return false;
                }
                seen_inodes.push(ino);
            }
            if let Some(off) = f.disk_offset {
                if seen_offsets.contains(&off) {
                    return false;
                }
                seen_offsets.push(off);
            }
            true
        });
    }

    /// Get files matching the current filter criteria.
    pub fn filtered_files(&self, filter: &ScanFilter) -> Vec<&RecoverableFile> {
        self.files.iter().filter(|f| filter.matches(f)).collect()
    }

    /// Count of selected files.
    pub fn selected_count(&self) -> usize {
        self.files.iter().filter(|f| f.selected).count()
    }

    /// Total size of selected files.
    pub fn selected_total_size(&self) -> u64 {
        self.files
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.file_size)
            .sum()
    }

    /// Select all files matching the current filter.
    pub fn select_all(&mut self, filter: &ScanFilter) {
        for file in &mut self.files {
            if filter.matches(file) {
                file.selected = true;
            }
        }
    }

    /// Deselect all files.
    pub fn deselect_all(&mut self) {
        for file in &mut self.files {
            file.selected = false;
        }
    }

    /// Toggle selection for a file by ID.
    pub fn toggle_selection(&mut self, id: u64) {
        for file in &mut self.files {
            if file.id == id {
                file.selected = !file.selected;
                break;
            }
        }
    }

    /// Simulate recovering selected files to the given target directory.
    /// Returns a list of (filename, success) tuples.
    pub fn recover_selected(&self, target_dir: &str) -> Vec<RecoveryResult> {
        let mut results = Vec::new();
        for file in &self.files {
            if !file.selected {
                continue;
            }
            let dest = format!("{}/{}", target_dir, file.filename);
            let success = file.confidence != RecoveryConfidence::Unlikely;
            let bytes_recovered = if success {
                file.file_size
            } else {
                // Partial recovery for unlikely files.
                file.file_size / 4
            };
            results.push(RecoveryResult {
                filename: file.filename.clone(),
                destination: dest,
                original_size: file.file_size,
                bytes_recovered,
                success,
                error_message: if success {
                    None
                } else {
                    Some(String::from("Data blocks partially overwritten"))
                },
            });
        }
        results
    }

    /// Statistics about the current scan results.
    pub fn stats(&self) -> ScanStats {
        let mut by_confidence: BTreeMap<RecoveryConfidence, usize> = BTreeMap::new();
        let mut by_category: BTreeMap<FileCategory, usize> = BTreeMap::new();
        let mut by_source: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut total_size: u64 = 0;

        for f in &self.files {
            let c = by_confidence.entry(f.confidence).or_insert(0);
            *c = c.saturating_add(1);
            let c = by_category.entry(f.category()).or_insert(0);
            *c = c.saturating_add(1);
            let c = by_source.entry(f.source.display_name()).or_insert(0);
            *c = c.saturating_add(1);
            total_size = total_size.saturating_add(f.file_size);
        }

        ScanStats {
            total_files: self.files.len(),
            total_size,
            by_confidence,
            by_category,
            by_source,
        }
    }
}

/// Result of attempting to recover a single file.
#[derive(Debug, Clone)]
pub struct RecoveryResult {
    pub filename: String,
    pub destination: String,
    pub original_size: u64,
    pub bytes_recovered: u64,
    pub success: bool,
    pub error_message: Option<String>,
}

/// Summary statistics for scan results.
#[derive(Debug, Clone)]
pub struct ScanStats {
    pub total_files: usize,
    pub total_size: u64,
    pub by_confidence: BTreeMap<RecoveryConfidence, usize>,
    pub by_category: BTreeMap<FileCategory, usize>,
    pub by_source: BTreeMap<&'static str, usize>,
}

// ============================================================================
// Scan filter
// ============================================================================

/// Filter criteria for the results list.
#[derive(Debug, Clone)]
pub struct ScanFilter {
    pub category: Option<FileCategory>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub min_confidence: Option<RecoveryConfidence>,
    pub source: Option<DeletionSource>,
    pub filename_search: String,
    pub min_delete_time: Option<u64>,
    pub max_delete_time: Option<u64>,
}

impl Default for ScanFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanFilter {
    pub fn new() -> Self {
        Self {
            category: None,
            min_size: None,
            max_size: None,
            min_confidence: None,
            source: None,
            filename_search: String::new(),
            min_delete_time: None,
            max_delete_time: None,
        }
    }

    pub fn with_category(mut self, cat: FileCategory) -> Self {
        self.category = Some(cat);
        self
    }

    pub fn with_min_size(mut self, size: u64) -> Self {
        self.min_size = Some(size);
        self
    }

    pub fn with_max_size(mut self, size: u64) -> Self {
        self.max_size = Some(size);
        self
    }

    pub fn with_min_confidence(mut self, conf: RecoveryConfidence) -> Self {
        self.min_confidence = Some(conf);
        self
    }

    pub fn with_source(mut self, source: DeletionSource) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_search(mut self, term: &str) -> Self {
        self.filename_search = term.to_lowercase();
        self
    }

    pub fn with_delete_time_range(mut self, min: u64, max: u64) -> Self {
        self.min_delete_time = Some(min);
        self.max_delete_time = Some(max);
        self
    }

    pub fn matches(&self, file: &RecoverableFile) -> bool {
        if let Some(cat) = self.category
            && file.category() != cat
        {
            return false;
        }
        if let Some(min) = self.min_size
            && file.file_size < min
        {
            return false;
        }
        if let Some(max) = self.max_size
            && file.file_size > max
        {
            return false;
        }
        if let Some(min_conf) = self.min_confidence
            && file.confidence > min_conf
        {
            return false;
        }
        if let Some(src) = self.source
            && file.source != src
        {
            return false;
        }
        if !self.filename_search.is_empty()
            && !file.filename.to_lowercase().contains(&self.filename_search)
        {
            return false;
        }
        if let Some(min_dt) = self.min_delete_time
            && file.delete_time < min_dt
            && file.delete_time > 0
        {
            return false;
        }
        if let Some(max_dt) = self.max_delete_time
            && file.delete_time > max_dt
        {
            return false;
        }
        true
    }

    pub fn is_active(&self) -> bool {
        self.category.is_some()
            || self.min_size.is_some()
            || self.max_size.is_some()
            || self.min_confidence.is_some()
            || self.source.is_some()
            || !self.filename_search.is_empty()
            || self.min_delete_time.is_some()
            || self.max_delete_time.is_some()
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

// ============================================================================
// UI state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiScreen {
    /// Device/partition selection + scan options.
    ScanSetup,
    /// Scanning in progress.
    Scanning,
    /// Results list with preview panel.
    Results,
    /// Recovery in progress / results.
    Recovering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Filename,
    Size,
    DeleteTime,
    Confidence,
    FileType,
}

impl SortField {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Filename => "Name",
            Self::Size => "Size",
            Self::DeleteTime => "Deleted",
            Self::Confidence => "Confidence",
            Self::FileType => "Type",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggle(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    pub fn indicator(self) -> &'static str {
        match self {
            Self::Ascending => " ^",
            Self::Descending => " v",
        }
    }
}

/// The main application state.
pub struct UndeleteApp {
    pub width: f32,
    pub height: f32,
    pub screen: UiScreen,
    pub engine: RecoveryEngine,
    pub partitions: Vec<Partition>,
    pub selected_partition: usize,
    pub scan_mode: ScanMode,
    pub filter: ScanFilter,
    pub sort_field: SortField,
    pub sort_direction: SortDirection,
    pub selected_file_idx: Option<usize>,
    pub scroll_offset: usize,
    pub recovery_target: String,
    pub recovery_results: Vec<RecoveryResult>,
    pub show_filter_panel: bool,
    pub active_category_filter: Option<usize>,
}

impl UndeleteApp {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            screen: UiScreen::ScanSetup,
            engine: RecoveryEngine::new(),
            partitions: simulated_partitions(),
            selected_partition: 0,
            scan_mode: ScanMode::Quick,
            filter: ScanFilter::new(),
            sort_field: SortField::Confidence,
            sort_direction: SortDirection::Ascending,
            selected_file_idx: None,
            scroll_offset: 0,
            recovery_target: String::from("/home/user/recovered"),
            recovery_results: Vec::new(),
            show_filter_panel: false,
            active_category_filter: None,
        }
    }

    /// Start a scan on the selected partition.
    pub fn start_scan(&mut self) {
        if let Some(partition) = self.partitions.get(self.selected_partition) {
            let partition = partition.clone();
            self.engine.scan(&partition, self.scan_mode);
            self.screen = UiScreen::Results;
            self.selected_file_idx = None;
            self.scroll_offset = 0;
        }
    }

    /// Run recovery on selected files.
    pub fn start_recovery(&mut self) {
        self.recovery_results = self.engine.recover_selected(&self.recovery_target);
        self.screen = UiScreen::Recovering;
    }

    /// Get the sorted, filtered file list.
    pub fn visible_files(&self) -> Vec<&RecoverableFile> {
        let mut files = self.engine.filtered_files(&self.filter);
        let sort_dir = self.sort_direction;
        let sort_field = self.sort_field;
        files.sort_by(|a, b| {
            let cmp = match sort_field {
                SortField::Filename => a.filename.cmp(&b.filename),
                SortField::Size => a.file_size.cmp(&b.file_size),
                SortField::DeleteTime => a.delete_time.cmp(&b.delete_time),
                SortField::Confidence => a.confidence.cmp(&b.confidence),
                SortField::FileType => a.file_type.cmp(&b.file_type),
            };
            match sort_dir {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
            }
        });
        files
    }

    /// Get the currently selected file, if any.
    pub fn selected_file(&self) -> Option<&RecoverableFile> {
        let files = self.visible_files();
        self.selected_file_idx
            .and_then(|idx| files.get(idx).copied())
    }

    /// Toggle the sort field; if already sorting by this field, flip direction.
    pub fn toggle_sort(&mut self, field: SortField) {
        if self.sort_field == field {
            self.sort_direction = self.sort_direction.toggle();
        } else {
            self.sort_field = field;
            self.sort_direction = SortDirection::Ascending;
        }
    }

    /// Set category filter by index (None = all).
    pub fn set_category_filter(&mut self, idx: Option<usize>) {
        self.active_category_filter = idx;
        self.filter.category = idx.and_then(|i| FileCategory::ALL.get(i).copied());
        self.selected_file_idx = None;
        self.scroll_offset = 0;
    }

    /// Navigate to a file in the results.
    pub fn select_file(&mut self, idx: usize) {
        let count = self.visible_files().len();
        if idx < count {
            self.selected_file_idx = Some(idx);
        }
    }

    /// Navigate selection up.
    pub fn select_prev(&mut self) {
        match self.selected_file_idx {
            Some(0) | None => {}
            Some(idx) => self.selected_file_idx = Some(idx.saturating_sub(1)),
        }
    }

    /// Navigate selection down.
    pub fn select_next(&mut self) {
        let count = self.visible_files().len();
        match self.selected_file_idx {
            None => {
                if count > 0 {
                    self.selected_file_idx = Some(0);
                }
            }
            Some(idx) => {
                if idx.saturating_add(1) < count {
                    self.selected_file_idx = Some(idx.saturating_add(1));
                }
            }
        }
    }

    /// Toggle selection of the currently highlighted file.
    pub fn toggle_current_selection(&mut self) {
        if let Some(file) = self.selected_file() {
            let id = file.id;
            self.engine.toggle_selection(id);
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Window background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        match self.screen {
            UiScreen::ScanSetup => self.render_scan_setup(&mut cmds),
            UiScreen::Scanning => self.render_scanning(&mut cmds),
            UiScreen::Results => self.render_results(&mut cmds),
            UiScreen::Recovering => self.render_recovering(&mut cmds),
        }

        cmds
    }

    // -- Scan setup screen --------------------------------------------------

    fn render_scan_setup(&self, cmds: &mut Vec<RenderCommand>) {
        // Header
        self.render_header(cmds, "File Recovery");

        let content_y = HEADER_HEIGHT + PADDING;

        // Partition selection section
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: content_y,
            text: String::from("Select Partition"),
            color: TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - PADDING * 2.0),
        });

        let list_y = content_y + 28.0;
        for (i, part) in self.partitions.iter().enumerate() {
            let y = list_y + (i as f32) * 64.0;
            let selected = i == self.selected_partition;
            self.render_partition_card(cmds, part, y, selected);
        }

        // Scan mode section
        let mode_y = list_y + (self.partitions.len() as f32) * 64.0 + PADDING;
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: mode_y,
            text: String::from("Scan Mode"),
            color: TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - PADDING * 2.0),
        });

        // Quick scan option
        let quick_y = mode_y + 28.0;
        self.render_radio_option(
            cmds,
            PADDING,
            quick_y,
            "Quick Scan - Recycle bin + inode tables (faster)",
            self.scan_mode == ScanMode::Quick,
        );

        // Deep scan option
        let deep_y = quick_y + 32.0;
        self.render_radio_option(
            cmds,
            PADDING,
            deep_y,
            "Deep Scan - Sector-by-sector signature detection (thorough)",
            self.scan_mode == ScanMode::Deep,
        );

        // Start button
        let btn_y = self.height - FOOTER_HEIGHT - PADDING;
        self.render_button(
            cmds,
            self.width - BUTTON_WIDTH - PADDING,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Start Scan",
            BLUE,
        );
    }

    fn render_partition_card(
        &self,
        cmds: &mut Vec<RenderCommand>,
        part: &Partition,
        y: f32,
        selected: bool,
    ) {
        let card_w = self.width - PADDING * 2.0;
        let card_color = if selected { SURFACE1 } else { SURFACE0 };

        // Card background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y,
            width: card_w,
            height: 56.0,
            color: card_color,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x: PADDING,
                y,
                width: card_w,
                height: 56.0,
                color: BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
        }

        // Partition name and mount point
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: y + 8.0,
            text: format!("{} ({})", part.name, part.mount_point),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(card_w * 0.5),
        });

        // Filesystem and size
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: y + 28.0,
            text: format!(
                "{} - {} / {} ({:.0}% used)",
                part.filesystem,
                format_size(part.total_bytes.saturating_sub(part.free_bytes)),
                format_size(part.total_bytes),
                part.usage_percent(),
            ),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card_w - 24.0),
        });

        // Usage bar
        let bar_x = card_w * 0.7 + PADDING;
        let bar_w = card_w * 0.25;
        let bar_y = y + 22.0;
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: PROGRESS_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::all(PROGRESS_HEIGHT / 2.0),
        });
        let fill_w = bar_w * (part.usage_percent() / 100.0);
        let bar_color = if part.usage_percent() > 90.0 {
            RED
        } else if part.usage_percent() > 70.0 {
            YELLOW
        } else {
            BLUE
        };
        if fill_w > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: fill_w,
                height: PROGRESS_HEIGHT,
                color: bar_color,
                corner_radii: CornerRadii::all(PROGRESS_HEIGHT / 2.0),
            });
        }
    }

    fn render_radio_option(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        label: &str,
        selected: bool,
    ) {
        let radio_size: f32 = 16.0;
        let cx = x + radio_size / 2.0;
        let cy = y + radio_size / 2.0;

        // Outer circle (approximated with small rounded rect)
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width: radio_size,
            height: radio_size,
            color: if selected { BLUE } else { OVERLAY0 },
            line_width: 1.5,
            corner_radii: CornerRadii::all(radio_size / 2.0),
        });

        if selected {
            // Inner filled circle
            cmds.push(RenderCommand::FillRect {
                x: cx - 4.0,
                y: cy - 4.0,
                width: 8.0,
                height: 8.0,
                color: BLUE,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        cmds.push(RenderCommand::Text {
            x: x + radio_size + 8.0,
            y: y + 1.0,
            text: label.to_string(),
            color: if selected { TEXT } else { SUBTEXT0 },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - x - radio_size - PADDING - 8.0),
        });
    }

    // -- Scanning progress screen -------------------------------------------

    fn render_scanning(&self, cmds: &mut Vec<RenderCommand>) {
        self.render_header(cmds, "Scanning...");

        let center_y = self.height / 2.0 - 60.0;

        // Phase label
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: center_y,
            text: self.engine.progress.phase.display_name().to_string(),
            color: BLUE,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - PADDING * 2.0),
        });

        // Progress bar
        let bar_y = center_y + 32.0;
        let bar_w = self.width - PADDING * 4.0;
        cmds.push(RenderCommand::FillRect {
            x: PADDING * 2.0,
            y: bar_y,
            width: bar_w,
            height: 12.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let fill = bar_w * self.engine.progress.overall_progress;
        if fill > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: PADDING * 2.0,
                y: bar_y,
                width: fill,
                height: 12.0,
                color: BLUE,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Progress percentage
        cmds.push(RenderCommand::Text {
            x: PADDING * 2.0,
            y: bar_y + 20.0,
            text: format!(
                "{:.0}% - {} files found",
                self.engine.progress.overall_progress * 100.0,
                self.engine.progress.files_found,
            ),
            color: SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_w),
        });

        // Bytes scanned
        cmds.push(RenderCommand::Text {
            x: PADDING * 2.0,
            y: bar_y + 40.0,
            text: format!(
                "Scanned: {} / {}",
                format_size(self.engine.progress.bytes_scanned),
                format_size(self.engine.progress.total_bytes),
            ),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_w),
        });

        // ETA
        if let Some(remaining) = self.engine.progress.estimated_remaining_seconds() {
            cmds.push(RenderCommand::Text {
                x: PADDING * 2.0,
                y: bar_y + 58.0,
                text: format!("Estimated time remaining: {remaining}s"),
                color: OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bar_w),
            });
        }
    }

    // -- Results screen (multi-panel) ---------------------------------------

    fn render_results(&self, cmds: &mut Vec<RenderCommand>) {
        self.render_header(cmds, "Recovery Results");

        let content_y = HEADER_HEIGHT;
        let content_h = self.height - HEADER_HEIGHT - FOOTER_HEIGHT - STATUS_BAR_HEIGHT;

        // Left sidebar: category filters
        self.render_category_sidebar(cmds, content_y, content_h);

        // Main list area
        let list_x = SIDEBAR_WIDTH;
        let list_w = self.width - SIDEBAR_WIDTH - PREVIEW_PANEL_WIDTH;
        self.render_file_list(cmds, list_x, content_y, list_w, content_h);

        // Right preview panel
        let preview_x = self.width - PREVIEW_PANEL_WIDTH;
        self.render_preview_panel(cmds, preview_x, content_y, PREVIEW_PANEL_WIDTH, content_h);

        // Footer with action buttons
        self.render_results_footer(cmds);

        // Status bar
        self.render_status_bar(cmds);
    }

    fn render_category_sidebar(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: SIDEBAR_WIDTH,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // "Categories" label
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + PADDING,
            text: String::from("Categories"),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
        });

        // "All Files" entry
        let all_y = y + 36.0;
        let all_selected = self.active_category_filter.is_none();
        if all_selected {
            cmds.push(RenderCommand::FillRect {
                x: 4.0,
                y: all_y,
                width: SIDEBAR_WIDTH - 8.0,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
        }
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: all_y + 6.0,
            text: format!("All Files ({})", self.engine.files.len()),
            color: if all_selected { BLUE } else { SUBTEXT1 },
            font_size: FONT_SIZE,
            font_weight: if all_selected {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
        });

        // Category entries
        let stats = self.engine.stats();
        for (i, cat) in FileCategory::ALL.iter().enumerate() {
            let item_y = all_y + 32.0 + (i as f32) * 28.0;
            let is_selected = self.active_category_filter == Some(i);
            let count = stats.by_category.get(cat).copied().unwrap_or(0);

            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: item_y,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: 28.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
            }

            // Category color indicator
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: item_y + 8.0,
                width: 10.0,
                height: 10.0,
                color: cat.color(),
                corner_radii: CornerRadii::all(2.0),
            });

            cmds.push(RenderCommand::Text {
                x: PADDING + 16.0,
                y: item_y + 6.0,
                text: format!("{} ({})", cat.display_name(), count),
                color: if is_selected { BLUE } else { SUBTEXT1 },
                font_size: FONT_SIZE,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 16.0),
            });
        }

        // Confidence filter section
        let conf_y = all_y + 32.0 + (FileCategory::ALL.len() as f32) * 28.0 + PADDING;
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: conf_y,
            width: SIDEBAR_WIDTH - PADDING * 2.0,
            height: 1.0,
            color: SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: conf_y + 8.0,
            text: String::from("By Confidence"),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
        });

        let confidences = [
            RecoveryConfidence::High,
            RecoveryConfidence::Medium,
            RecoveryConfidence::Low,
            RecoveryConfidence::Unlikely,
        ];
        for (i, conf) in confidences.iter().enumerate() {
            let cy = conf_y + 32.0 + (i as f32) * 24.0;
            let count = stats.by_confidence.get(conf).copied().unwrap_or(0);

            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: cy + 4.0,
                width: 8.0,
                height: 8.0,
                color: conf.color(),
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: PADDING + 14.0,
                y: cy + 1.0,
                text: format!("{}: {}", conf.display_name(), count),
                color: SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 14.0),
            });
        }
    }

    fn render_file_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // List background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Column headers
        let header_y = y;
        cmds.push(RenderCommand::FillRect {
            x,
            y: header_y,
            width,
            height: 28.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let columns: [(SortField, f32, f32); 5] = [
            (SortField::Filename, x + 32.0, width * 0.35),
            (SortField::Size, x + width * 0.35 + 32.0, width * 0.12),
            (SortField::FileType, x + width * 0.5 + 32.0, width * 0.15),
            (SortField::DeleteTime, x + width * 0.65 + 32.0, width * 0.18),
            (SortField::Confidence, x + width * 0.83 + 32.0, width * 0.15),
        ];

        for (field, col_x, col_w) in &columns {
            let label = if self.sort_field == *field {
                format!(
                    "{}{}",
                    field.display_name(),
                    self.sort_direction.indicator()
                )
            } else {
                field.display_name().to_string()
            };
            cmds.push(RenderCommand::Text {
                x: *col_x,
                y: header_y + 7.0,
                text: label,
                color: if self.sort_field == *field {
                    BLUE
                } else {
                    SUBTEXT0
                },
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(*col_w),
            });
        }

        // File rows
        let list_y = header_y + 28.0;
        let max_visible = ((height - 28.0) / ITEM_HEIGHT) as usize;
        let files = self.visible_files();

        cmds.push(RenderCommand::PushClip {
            x,
            y: list_y,
            width,
            height: height - 28.0,
        });

        for (i, file) in files
            .iter()
            .skip(self.scroll_offset)
            .take(max_visible)
            .enumerate()
        {
            let row_y = list_y + (i as f32) * ITEM_HEIGHT;
            let global_idx = i.saturating_add(self.scroll_offset);
            let is_selected = self.selected_file_idx == Some(global_idx);
            self.render_file_row(cmds, file, x, row_y, width, is_selected);
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_file_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        file: &RecoverableFile,
        x: f32,
        y: f32,
        width: f32,
        selected: bool,
    ) {
        // Row background
        let bg_color = if selected { SURFACE1 } else { BASE };
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: ITEM_HEIGHT,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Checkbox
        let cb_x = x + 8.0;
        let cb_y = y + (ITEM_HEIGHT - CHECKBOX_SIZE) / 2.0;
        cmds.push(RenderCommand::StrokeRect {
            x: cb_x,
            y: cb_y,
            width: CHECKBOX_SIZE,
            height: CHECKBOX_SIZE,
            color: OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });
        if file.selected {
            cmds.push(RenderCommand::FillRect {
                x: cb_x + 3.0,
                y: cb_y + 3.0,
                width: CHECKBOX_SIZE - 6.0,
                height: CHECKBOX_SIZE - 6.0,
                color: BLUE,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Filename (two lines: name + path)
        let name_x = x + 32.0;
        cmds.push(RenderCommand::Text {
            x: name_x,
            y: y + 8.0,
            text: file.filename.clone(),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.33),
        });
        if let Some(ref path) = file.original_path {
            cmds.push(RenderCommand::Text {
                x: name_x,
                y: y + 26.0,
                text: path.clone(),
                color: OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.33),
            });
        }

        // Size
        cmds.push(RenderCommand::Text {
            x: x + width * 0.35 + 32.0,
            y: y + 18.0,
            text: file.size_display(),
            color: SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.12),
        });

        // File type with color
        cmds.push(RenderCommand::FillRect {
            x: x + width * 0.5 + 32.0,
            y: y + 20.0,
            width: 8.0,
            height: 8.0,
            color: file.file_type.color(),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.5 + 44.0,
            y: y + 18.0,
            text: file.file_type.display_name().to_string(),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.14),
        });

        // Delete time
        cmds.push(RenderCommand::Text {
            x: x + width * 0.65 + 32.0,
            y: y + 18.0,
            text: file.delete_time_display(),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.16),
        });

        // Confidence badge
        let conf_x = x + width * 0.83 + 32.0;
        let badge_w = 64.0;
        cmds.push(RenderCommand::FillRect {
            x: conf_x,
            y: y + 16.0,
            width: badge_w,
            height: 20.0,
            color: Color::rgba(
                file.confidence.color().r,
                file.confidence.color().g,
                file.confidence.color().b,
                40,
            ),
            corner_radii: CornerRadii::all(10.0),
        });
        cmds.push(RenderCommand::Text {
            x: conf_x + 8.0,
            y: y + 19.0,
            text: file.confidence.display_name().to_string(),
            color: file.confidence.color(),
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(badge_w - 16.0),
        });

        // Bottom separator
        cmds.push(RenderCommand::FillRect {
            x,
            y: y + ITEM_HEIGHT - 1.0,
            width,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
    }

    fn render_preview_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 1.0,
            height,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        if let Some(file) = self.selected_file() {
            self.render_file_preview(cmds, file, x, y, width);
        } else {
            // No selection hint
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + height / 2.0 - 10.0,
                text: String::from("Select a file to preview"),
                color: OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
            });
        }
    }

    fn render_file_preview(
        &self,
        cmds: &mut Vec<RenderCommand>,
        file: &RecoverableFile,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let inner_x = x + PADDING;
        let inner_w = width - PADDING * 2.0;
        let mut cy = y + PADDING;

        // File icon placeholder (colored square)
        cmds.push(RenderCommand::FillRect {
            x: inner_x,
            y: cy,
            width: 48.0,
            height: 48.0,
            color: file.file_type.color(),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: inner_x + 8.0,
            y: cy + 16.0,
            text: file.file_type.extension().to_uppercase(),
            color: CRUST,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(32.0),
        });

        // Filename next to icon
        cmds.push(RenderCommand::Text {
            x: inner_x + 60.0,
            y: cy + 4.0,
            text: file.filename.clone(),
            color: TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner_w - 64.0),
        });
        cmds.push(RenderCommand::Text {
            x: inner_x + 60.0,
            y: cy + 26.0,
            text: file.file_type.display_name().to_string(),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner_w - 64.0),
        });

        cy += 64.0;

        // Separator
        cmds.push(RenderCommand::FillRect {
            x: inner_x,
            y: cy,
            width: inner_w,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        cy += 12.0;

        // Metadata rows
        let metadata: Vec<(&str, String)> = vec![
            ("Size", file.size_display()),
            (
                "Confidence",
                format!(
                    "{} ({}%)",
                    file.confidence.display_name(),
                    file.recovery_percent,
                ),
            ),
            ("Source", file.source.display_name().to_string()),
            ("Deleted", file.delete_time_display()),
            ("Modified", format_timestamp(file.modify_time)),
            ("Partition", file.partition_name.clone()),
        ];

        // Add optional metadata
        let mut all_meta = metadata;
        if let Some(ref path) = file.original_path {
            all_meta.push(("Original Path", path.clone()));
        }
        if let Some(ino) = file.inode_number {
            all_meta.push(("Inode", format!("{ino}")));
        }
        if let Some(off) = file.disk_offset {
            all_meta.push(("Disk Offset", format!("0x{off:08X}")));
        }

        for (label, value) in &all_meta {
            cmds.push(RenderCommand::Text {
                x: inner_x,
                y: cy,
                text: (*label).to_string(),
                color: OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(inner_w * 0.4),
            });
            cmds.push(RenderCommand::Text {
                x: inner_x + inner_w * 0.4,
                y: cy,
                text: value.clone(),
                color: SUBTEXT1,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner_w * 0.6),
            });
            cy += 20.0;
        }

        cy += 8.0;

        // Recovery confidence bar
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: cy,
            text: String::from("Recovery Estimate"),
            color: TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner_w),
        });
        cy += 18.0;

        cmds.push(RenderCommand::FillRect {
            x: inner_x,
            y: cy,
            width: inner_w,
            height: PROGRESS_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(PROGRESS_HEIGHT / 2.0),
        });
        let pct = f32::from(file.recovery_percent) / 100.0;
        let fill_w = inner_w * pct;
        if fill_w > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: inner_x,
                y: cy,
                width: fill_w,
                height: PROGRESS_HEIGHT,
                color: file.confidence.color(),
                corner_radii: CornerRadii::all(PROGRESS_HEIGHT / 2.0),
            });
        }
        cy += 16.0;

        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: cy,
            text: format!("{}% data likely recoverable", file.recovery_percent),
            color: file.confidence.color(),
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner_w),
        });
        cy += 24.0;

        // Source description
        cmds.push(RenderCommand::FillRect {
            x: inner_x,
            y: cy,
            width: inner_w,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        cy += 12.0;

        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: cy,
            text: String::from("Detection Method"),
            color: TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner_w),
        });
        cy += 18.0;

        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: cy,
            text: file.source.description().to_string(),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner_w),
        });
        cy += 24.0;

        // Preview bytes section
        if !file.preview_bytes.is_empty() {
            cmds.push(RenderCommand::Text {
                x: inner_x,
                y: cy,
                text: String::from("Data Preview (hex)"),
                color: TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(inner_w),
            });
            cy += 18.0;

            let hex_str = format_hex_preview(&file.preview_bytes, 16);
            cmds.push(RenderCommand::FillRect {
                x: inner_x,
                y: cy,
                width: inner_w,
                height: 60.0,
                color: CRUST,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: inner_x + 6.0,
                y: cy + 6.0,
                text: hex_str,
                color: GREEN,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner_w - 12.0),
            });
        }
    }

    fn render_results_footer(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - FOOTER_HEIGHT - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: FOOTER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Selection info
        let selected = self.engine.selected_count();
        let total_size = self.engine.selected_total_size();
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 14.0,
            text: format!(
                "{} file{} selected ({} total)",
                selected,
                if selected == 1 { "" } else { "s" },
                format_size(total_size),
            ),
            color: SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.4),
        });

        // Target directory
        cmds.push(RenderCommand::Text {
            x: self.width * 0.4,
            y: y + 14.0,
            text: format!("Recover to: {}", self.recovery_target),
            color: OVERLAY0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.3),
        });

        // Action buttons
        let btn_x = self.width - BUTTON_WIDTH - PADDING;
        if selected > 0 {
            self.render_button(
                cmds,
                btn_x,
                y + 8.0,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                "Recover",
                GREEN,
            );
        }

        // Select All button
        self.render_button(
            cmds,
            btn_x - BUTTON_WIDTH - PADDING,
            y + 8.0,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Select All",
            SURFACE2,
        );

        // New Scan button
        self.render_button(
            cmds,
            btn_x - (BUTTON_WIDTH + PADDING) * 2.0,
            y + 8.0,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "New Scan",
            SURFACE2,
        );
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let stats = self.engine.stats();
        let mode_str = self.engine.scan_mode.display_name();
        let files = self.visible_files();

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 7.0,
            text: format!(
                "{} | {} files found | {} total | Showing {} of {}",
                mode_str,
                stats.total_files,
                format_size(stats.total_size),
                files.len(),
                stats.total_files,
            ),
            color: OVERLAY0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - PADDING * 2.0),
        });
    }

    // -- Recovery results screen --------------------------------------------

    fn render_recovering(&self, cmds: &mut Vec<RenderCommand>) {
        self.render_header(cmds, "Recovery Results");

        let content_y = HEADER_HEIGHT + PADDING;

        // Summary
        let success_count = self.recovery_results.iter().filter(|r| r.success).count();
        let fail_count = self.recovery_results.len().saturating_sub(success_count);
        let total_recovered: u64 = self
            .recovery_results
            .iter()
            .map(|r| r.bytes_recovered)
            .sum();

        // Summary card
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: content_y,
            width: self.width - PADDING * 2.0,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        cmds.push(RenderCommand::Text {
            x: PADDING * 2.0,
            y: content_y + 12.0,
            text: String::from("Recovery Complete"),
            color: TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - PADDING * 4.0),
        });

        cmds.push(RenderCommand::Text {
            x: PADDING * 2.0,
            y: content_y + 36.0,
            text: format!(
                "{} succeeded, {} failed | {} recovered",
                success_count,
                fail_count,
                format_size(total_recovered),
            ),
            color: SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - PADDING * 4.0),
        });

        cmds.push(RenderCommand::Text {
            x: PADDING * 2.0,
            y: content_y + 54.0,
            text: format!("Target: {}", self.recovery_target),
            color: OVERLAY0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - PADDING * 4.0),
        });

        // Individual results
        let list_y = content_y + 96.0;
        cmds.push(RenderCommand::PushClip {
            x: PADDING,
            y: list_y,
            width: self.width - PADDING * 2.0,
            height: self.height - list_y - FOOTER_HEIGHT,
        });

        for (i, result) in self.recovery_results.iter().enumerate() {
            let ry = list_y + (i as f32) * 44.0;
            self.render_recovery_result_row(cmds, result, ry);
        }

        cmds.push(RenderCommand::PopClip);

        // Back button
        self.render_button(
            cmds,
            self.width - BUTTON_WIDTH - PADDING,
            self.height - FOOTER_HEIGHT + 8.0,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Done",
            BLUE,
        );
    }

    fn render_recovery_result_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        result: &RecoveryResult,
        y: f32,
    ) {
        let row_w = self.width - PADDING * 2.0;

        // Row background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y,
            width: row_w,
            height: 40.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        // Status indicator
        let status_color = if result.success { GREEN } else { RED };
        cmds.push(RenderCommand::FillRect {
            x: PADDING + 8.0,
            y: y + 14.0,
            width: 12.0,
            height: 12.0,
            color: status_color,
            corner_radii: CornerRadii::all(6.0),
        });

        // Filename
        cmds.push(RenderCommand::Text {
            x: PADDING + 28.0,
            y: y + 4.0,
            text: result.filename.clone(),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(row_w * 0.4),
        });

        // Destination
        cmds.push(RenderCommand::Text {
            x: PADDING + 28.0,
            y: y + 22.0,
            text: result.destination.clone(),
            color: OVERLAY0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(row_w * 0.4),
        });

        // Size recovered
        cmds.push(RenderCommand::Text {
            x: PADDING + row_w * 0.5,
            y: y + 12.0,
            text: format!(
                "{} / {}",
                format_size(result.bytes_recovered),
                format_size(result.original_size),
            ),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(row_w * 0.25),
        });

        // Status text
        let status_text = if result.success {
            "Recovered"
        } else {
            "Failed"
        };
        cmds.push(RenderCommand::Text {
            x: PADDING + row_w * 0.8,
            y: y + 8.0,
            text: status_text.to_string(),
            color: status_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(row_w * 0.18),
        });

        // Error message if failed
        if let Some(ref msg) = result.error_message {
            cmds.push(RenderCommand::Text {
                x: PADDING + row_w * 0.8,
                y: y + 24.0,
                text: msg.clone(),
                color: RED,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(row_w * 0.18),
            });
        }
    }

    // -- Shared rendering helpers -------------------------------------------

    fn render_header(&self, cmds: &mut Vec<RenderCommand>, title: &str) {
        // Header background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: HEADER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header shadow
        cmds.push(RenderCommand::BoxShadow {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: HEADER_HEIGHT,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 6.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 30),
            corner_radii: CornerRadii::ZERO,
        });

        // App icon placeholder
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: (HEADER_HEIGHT - 32.0) / 2.0,
            width: 32.0,
            height: 32.0,
            color: BLUE,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: PADDING + 6.0,
            y: (HEADER_HEIGHT - 32.0) / 2.0 + 8.0,
            text: String::from("UD"),
            color: CRUST,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(20.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING + 44.0,
            y: (HEADER_HEIGHT - FONT_SIZE_TITLE) / 2.0,
            text: title.to_string(),
            color: TEXT,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - PADDING * 2.0 - 44.0),
        });

        // Bottom border
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: HEADER_HEIGHT - 1.0,
            width: self.width,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
    }

    fn render_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        label: &str,
        color: Color,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + (height - FONT_SIZE) / 2.0,
            text: label.to_string(),
            color: if color.r > 100 || color.g > 100 || color.b > 100 {
                CRUST
            } else {
                TEXT
            },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 16.0),
        });
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a byte count as a human-readable string.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    const TB: u64 = 1024 * 1024 * 1024 * 1024;

    if bytes >= TB {
        format!("{:.1} TB", (bytes as f64) / (TB as f64))
    } else if bytes >= GB {
        format!("{:.1} GB", (bytes as f64) / (GB as f64))
    } else if bytes >= MB {
        format!("{:.1} MB", (bytes as f64) / (MB as f64))
    } else if bytes >= KB {
        format!("{:.1} KB", (bytes as f64) / (KB as f64))
    } else {
        format!("{bytes} B")
    }
}

/// Format a unix timestamp as a human-readable date string.
pub fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return String::from("Unknown");
    }
    // Simple epoch-to-date conversion (no timezone, approximation).
    let days = ts / 86400;
    let years_approx = days / 365;
    let year = 1970_u64.saturating_add(years_approx);
    let remaining_days = days.saturating_sub(years_approx.saturating_mul(365));
    let month = (remaining_days / 30).saturating_add(1).min(12);
    let day = (remaining_days % 30).saturating_add(1).min(31);
    let hour = (ts % 86400) / 3600;
    let minute = (ts % 3600) / 60;
    format!("{year}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Format bytes as a hex preview string.
pub fn format_hex_preview(data: &[u8], bytes_per_line: usize) -> String {
    if data.is_empty() || bytes_per_line == 0 {
        return String::from("(empty)");
    }
    let mut lines = Vec::new();
    let mut offset: usize = 0;
    let max_lines: usize = 4;
    let mut line_count: usize = 0;

    while offset < data.len() && line_count < max_lines {
        let end = (offset.saturating_add(bytes_per_line)).min(data.len());
        let chunk = data.get(offset..end);
        if let Some(bytes) = chunk {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            let ascii: String = bytes
                .iter()
                .map(|b| {
                    if (0x20..=0x7E).contains(b) {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            lines.push(format!("{:04X}: {} | {}", offset, hex.join(" "), ascii));
        }
        offset = offset.saturating_add(bytes_per_line);
        line_count = line_count.saturating_add(1);
    }
    lines.join("\n")
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);

    // Render scan setup screen
    let cmds = app.render();
    let _ = cmds.len();

    // Start a scan
    app.start_scan();

    // Render results
    let cmds = app.render();
    let _ = cmds.len();

    // Select a file
    app.select_file(0);
    let cmds = app.render();
    let _ = cmds.len();

    // Toggle selection and recover
    app.toggle_current_selection();
    app.engine.select_all(&app.filter);
    app.start_recovery();
    let cmds = app.render();
    let _ = cmds.len();

    // Test deep scan mode
    let mut app2 = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    app2.scan_mode = ScanMode::Deep;
    app2.start_scan();
    let cmds = app2.render();
    let _ = cmds.len();

    // Test filtering
    app2.set_category_filter(Some(0));
    let cmds = app2.render();
    let _ = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // === File signature tests ===

    #[test]
    fn test_jpeg_signature_detection() {
        let detector = SignatureDetector::new();
        let jpeg_data = [0xFF_u8, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let results = detector.detect(&jpeg_data);
        assert!(results.contains(&FileSignatureKind::Jpeg));
    }

    #[test]
    fn test_png_signature_detection() {
        let detector = SignatureDetector::new();
        let png_data = [0x89_u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        let results = detector.detect(&png_data);
        assert!(results.contains(&FileSignatureKind::Png));
    }

    #[test]
    fn test_pdf_signature_detection() {
        let detector = SignatureDetector::new();
        let pdf_data = b"%PDF-1.4 some content here";
        let results = detector.detect(pdf_data);
        assert!(results.contains(&FileSignatureKind::Pdf));
    }

    #[test]
    fn test_zip_signature_detection() {
        let detector = SignatureDetector::new();
        let zip_data = [0x50_u8, 0x4B, 0x03, 0x04, 0x14, 0x00, 0x00];
        let results = detector.detect(&zip_data);
        assert!(results.contains(&FileSignatureKind::Zip));
    }

    #[test]
    fn test_elf_signature_detection() {
        let detector = SignatureDetector::new();
        let elf_data = [0x7F_u8, 0x45, 0x4C, 0x46, 0x02, 0x01, 0x01];
        let results = detector.detect(&elf_data);
        assert!(results.contains(&FileSignatureKind::Elf));
    }

    #[test]
    fn test_gif87a_signature() {
        let detector = SignatureDetector::new();
        let data = b"GIF87a\x00\x01";
        let results = detector.detect(data);
        assert!(results.contains(&FileSignatureKind::Gif));
    }

    #[test]
    fn test_gif89a_signature() {
        let detector = SignatureDetector::new();
        let data = b"GIF89a\x00\x01";
        let results = detector.detect(data);
        assert!(results.contains(&FileSignatureKind::Gif));
    }

    #[test]
    fn test_mp3_id3_signature() {
        let detector = SignatureDetector::new();
        let data = b"ID3\x03\x00\x00\x00";
        let results = detector.detect(data);
        assert!(results.contains(&FileSignatureKind::Mp3));
    }

    #[test]
    fn test_flac_signature() {
        let detector = SignatureDetector::new();
        let data = b"fLaC\x00\x00\x00\x22";
        let results = detector.detect(data);
        assert!(results.contains(&FileSignatureKind::Flac));
    }

    #[test]
    fn test_ogg_signature() {
        let detector = SignatureDetector::new();
        let data = b"OggS\x00\x02\x00\x00";
        let results = detector.detect(data);
        assert!(results.contains(&FileSignatureKind::Ogg));
    }

    #[test]
    fn test_unknown_data_no_match() {
        let detector = SignatureDetector::new();
        let data = [0x00_u8, 0x01, 0x02, 0x03, 0x04, 0x05];
        let results = detector.detect(&data);
        assert!(results.is_empty());
    }

    #[test]
    fn test_empty_data_no_match() {
        let detector = SignatureDetector::new();
        let results = detector.detect(&[]);
        assert!(results.is_empty());
    }

    #[test]
    fn test_too_short_data_no_match() {
        let detector = SignatureDetector::new();
        let results = detector.detect(&[0xFF]);
        assert!(results.is_empty());
    }

    #[test]
    fn test_detect_best_prefers_secondary() {
        let detector = SignatureDetector::new();
        // RIFF....WAVE should be detected as WAV (has secondary), not just RIFF
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"WAVE");
        let best = detector.detect_best(&data);
        assert_eq!(best, Some(FileSignatureKind::Wav));
    }

    #[test]
    fn test_detect_best_webp() {
        let detector = SignatureDetector::new();
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"WEBP");
        let best = detector.detect_best(&data);
        assert_eq!(best, Some(FileSignatureKind::Webp));
    }

    #[test]
    fn test_sector_scanning() {
        let detector = SignatureDetector::new();
        let sector_size = 512;
        let mut disk = vec![0u8; sector_size * 4];
        // Put JPEG at sector 0
        disk[0] = 0xFF;
        disk[1] = 0xD8;
        disk[2] = 0xFF;
        // Put PDF at sector 2
        disk[sector_size * 2..sector_size * 2 + 4].copy_from_slice(b"%PDF");
        let results = detector.scan_sectors(&disk, sector_size);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], (0, FileSignatureKind::Jpeg));
        assert_eq!(results[1], (1024, FileSignatureKind::Pdf));
    }

    #[test]
    fn test_sector_scan_empty() {
        let detector = SignatureDetector::new();
        let results = detector.scan_sectors(&[], 512);
        assert!(results.is_empty());
    }

    #[test]
    fn test_sector_scan_zero_sector_size() {
        let detector = SignatureDetector::new();
        let results = detector.scan_sectors(&[0; 100], 0);
        assert!(results.is_empty());
    }

    #[test]
    fn test_signature_count() {
        let detector = SignatureDetector::new();
        assert!(detector.signature_count() > 20);
    }

    #[test]
    fn test_secondary_pattern_match() {
        let sig = FileSignature::new(FileSignatureKind::Wav, 0, b"RIFF").with_secondary(8, b"WAVE");
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"WAVE");
        assert!(sig.matches(&data));
    }

    #[test]
    fn test_secondary_pattern_mismatch() {
        let sig = FileSignature::new(FileSignatureKind::Wav, 0, b"RIFF").with_secondary(8, b"WAVE");
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"AVI ");
        assert!(!sig.matches(&data));
    }

    // === Inode tests ===

    #[test]
    fn test_inode_is_deleted() {
        let inode = Ext4Inode::new_deleted(100, 4096);
        assert!(inode.is_deleted());
    }

    #[test]
    fn test_inode_not_deleted_if_link_count_nonzero() {
        let mut inode = Ext4Inode::new_deleted(100, 4096);
        inode.link_count = 1;
        assert!(!inode.is_deleted());
    }

    #[test]
    fn test_inode_not_deleted_if_no_delete_time() {
        let mut inode = Ext4Inode::new_deleted(100, 4096);
        inode.delete_time = 0;
        assert!(!inode.is_deleted());
    }

    #[test]
    fn test_inode_recovery_confidence_high_when_not_deleted() {
        let mut inode = Ext4Inode::new_deleted(100, 4096);
        inode.link_count = 1;
        inode.delete_time = 0;
        assert_eq!(inode.recovery_confidence(), RecoveryConfidence::High);
    }

    #[test]
    fn test_inode_recovery_confidence_unlikely_when_blocks_reallocated() {
        let inode = Ext4Inode::new_deleted(100, 4096).with_blocks_reallocated(true);
        assert_eq!(inode.recovery_confidence(), RecoveryConfidence::Unlikely);
    }

    #[test]
    fn test_inode_recovery_confidence_medium_with_blocks() {
        let inode = Ext4Inode::new_deleted(100, 4096).with_direct_blocks(vec![1000, 1001, 1002]);
        assert_eq!(inode.recovery_confidence(), RecoveryConfidence::Medium);
    }

    #[test]
    fn test_inode_recovery_confidence_low_without_blocks() {
        let inode = Ext4Inode::new_deleted(100, 4096);
        assert_eq!(inode.recovery_confidence(), RecoveryConfidence::Low);
    }

    #[test]
    fn test_inode_recovery_confidence_unlikely_zero_size() {
        let inode = Ext4Inode::new_deleted(100, 0);
        assert_eq!(inode.recovery_confidence(), RecoveryConfidence::Unlikely);
    }

    // === Inode scanner tests ===

    #[test]
    fn test_inode_scanner_finds_deleted_inodes() {
        let mut scanner = InodeScanner::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        scanner.scan_partition(&part);
        assert!(!scanner.deleted_inodes().is_empty());
    }

    #[test]
    fn test_inode_scanner_finds_dir_entries() {
        let mut scanner = InodeScanner::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        scanner.scan_partition(&part);
        assert!(!scanner.dir_entries().is_empty());
    }

    #[test]
    fn test_inode_scanner_progress() {
        let mut scanner = InodeScanner::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        scanner.scan_partition(&part);
        assert!((scanner.scan_progress() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_inode_scanner_find_dir_entry() {
        let mut scanner = InodeScanner::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        scanner.scan_partition(&part);
        // Dir entries only created for even-indexed inodes per group
        let first_dir = scanner.dir_entries().first();
        assert!(first_dir.is_some());
        let entry = first_dir.unwrap();
        let found = scanner.find_dir_entry(entry.inode_number);
        assert!(found.is_some());
    }

    #[test]
    fn test_inode_scanner_missing_dir_entry() {
        let scanner = InodeScanner::new();
        assert!(scanner.find_dir_entry(9999999).is_none());
    }

    // === Recycle bin tests ===

    #[test]
    fn test_recycle_bin_scan() {
        let mut reader = RecycleBinReader::new();
        reader.scan();
        assert!(reader.count() > 0);
    }

    #[test]
    fn test_recycle_bin_total_size() {
        let mut reader = RecycleBinReader::new();
        reader.scan();
        assert!(reader.total_size() > 0);
    }

    #[test]
    fn test_recycle_bin_find() {
        let mut reader = RecycleBinReader::new();
        reader.scan();
        assert!(reader.find(10001).is_some());
        assert!(reader.find(99999).is_none());
    }

    // === Recovery engine tests ===

    #[test]
    fn test_engine_quick_scan() {
        let mut engine = RecoveryEngine::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        engine.scan(&part, ScanMode::Quick);
        assert!(!engine.files.is_empty());
        assert_eq!(engine.progress.phase, ScanPhase::Complete);
        assert!((engine.progress.overall_progress - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_engine_deep_scan_finds_more() {
        let mut engine_quick = RecoveryEngine::new();
        let mut engine_deep = RecoveryEngine::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        engine_quick.scan(&part, ScanMode::Quick);
        engine_deep.scan(&part, ScanMode::Deep);
        assert!(engine_deep.files.len() > engine_quick.files.len());
    }

    #[test]
    fn test_engine_stats() {
        let mut engine = RecoveryEngine::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        engine.scan(&part, ScanMode::Quick);
        let stats = engine.stats();
        assert!(stats.total_files > 0);
        assert!(stats.total_size > 0);
    }

    #[test]
    fn test_engine_selection() {
        let mut engine = RecoveryEngine::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        engine.scan(&part, ScanMode::Quick);
        assert_eq!(engine.selected_count(), 0);

        let filter = ScanFilter::new();
        engine.select_all(&filter);
        assert!(engine.selected_count() > 0);
        assert!(engine.selected_total_size() > 0);

        engine.deselect_all();
        assert_eq!(engine.selected_count(), 0);
    }

    #[test]
    fn test_engine_toggle_selection() {
        let mut engine = RecoveryEngine::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        engine.scan(&part, ScanMode::Quick);
        let id = engine.files[0].id;
        engine.toggle_selection(id);
        assert!(engine.files[0].selected);
        engine.toggle_selection(id);
        assert!(!engine.files[0].selected);
    }

    #[test]
    fn test_engine_recovery() {
        let mut engine = RecoveryEngine::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        engine.scan(&part, ScanMode::Quick);
        let filter = ScanFilter::new();
        engine.select_all(&filter);
        let results = engine.recover_selected("/tmp/recovered");
        assert!(!results.is_empty());
        // At least some should succeed.
        assert!(results.iter().any(|r| r.success));
    }

    #[test]
    fn test_engine_recovery_no_selection() {
        let mut engine = RecoveryEngine::new();
        let part = Partition::new(
            "/dev/sda1",
            "/dev/sda1",
            "/",
            500_000_000_000,
            200_000_000_000,
        );
        engine.scan(&part, ScanMode::Quick);
        let results = engine.recover_selected("/tmp/recovered");
        assert!(results.is_empty());
    }

    // === Filter tests ===

    #[test]
    fn test_filter_by_category() {
        let filter = ScanFilter::new().with_category(FileCategory::Image);
        let file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 1024);
        assert!(filter.matches(&file));

        let file2 = RecoverableFile::from_signature(2, FileSignatureKind::Pdf, 0, 1024);
        assert!(!filter.matches(&file2));
    }

    #[test]
    fn test_filter_by_size_range() {
        let filter = ScanFilter::new().with_min_size(1000).with_max_size(5000);
        let mut file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 2000);
        assert!(filter.matches(&file));

        file.file_size = 500;
        assert!(!filter.matches(&file));

        file.file_size = 6000;
        assert!(!filter.matches(&file));
    }

    #[test]
    fn test_filter_by_confidence() {
        let filter = ScanFilter::new().with_min_confidence(RecoveryConfidence::Medium);
        let mut file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 1024);
        file.confidence = RecoveryConfidence::High;
        assert!(filter.matches(&file));

        file.confidence = RecoveryConfidence::Medium;
        assert!(filter.matches(&file));

        file.confidence = RecoveryConfidence::Unlikely;
        assert!(!filter.matches(&file));
    }

    #[test]
    fn test_filter_by_source() {
        let filter = ScanFilter::new().with_source(DeletionSource::RecycleBin);
        let mut file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 1024);
        file.source = DeletionSource::RecycleBin;
        assert!(filter.matches(&file));

        file.source = DeletionSource::InodeScan;
        assert!(!filter.matches(&file));
    }

    #[test]
    fn test_filter_by_search() {
        let filter = ScanFilter::new().with_search("report");
        let entry = RecycleBinEntry::new(
            1,
            "/home/user/report.pdf",
            "/trash/1",
            1024,
            1_700_000_000,
            FileSignatureKind::Pdf,
        );
        let file = RecoverableFile::from_recycle_bin(&entry);
        assert!(filter.matches(&file));

        let entry2 = RecycleBinEntry::new(
            2,
            "/home/user/photo.jpg",
            "/trash/2",
            1024,
            1_700_000_000,
            FileSignatureKind::Jpeg,
        );
        let file2 = RecoverableFile::from_recycle_bin(&entry2);
        assert!(!filter.matches(&file2));
    }

    #[test]
    fn test_filter_by_delete_time_range() {
        let filter = ScanFilter::new().with_delete_time_range(1_700_000_000, 1_700_200_000);
        let entry = RecycleBinEntry::new(
            1,
            "/home/user/file.txt",
            "/trash/1",
            1024,
            1_700_100_000,
            FileSignatureKind::Unknown,
        );
        let file = RecoverableFile::from_recycle_bin(&entry);
        assert!(filter.matches(&file));

        let entry2 = RecycleBinEntry::new(
            2,
            "/home/user/old.txt",
            "/trash/2",
            1024,
            1_700_300_000,
            FileSignatureKind::Unknown,
        );
        let file2 = RecoverableFile::from_recycle_bin(&entry2);
        assert!(!filter.matches(&file2));
    }

    #[test]
    fn test_filter_is_active() {
        let filter = ScanFilter::new();
        assert!(!filter.is_active());

        let filter2 = ScanFilter::new().with_category(FileCategory::Image);
        assert!(filter2.is_active());
    }

    #[test]
    fn test_filter_clear() {
        let mut filter = ScanFilter::new()
            .with_category(FileCategory::Image)
            .with_min_size(1000);
        assert!(filter.is_active());
        filter.clear();
        assert!(!filter.is_active());
    }

    #[test]
    fn test_filter_combined() {
        let filter = ScanFilter::new()
            .with_category(FileCategory::Image)
            .with_min_size(1000);
        let file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 2000);
        assert!(filter.matches(&file));

        // Wrong category
        let file2 = RecoverableFile::from_signature(2, FileSignatureKind::Pdf, 0, 2000);
        assert!(!filter.matches(&file2));

        // Wrong size
        let file3 = RecoverableFile::from_signature(3, FileSignatureKind::Jpeg, 0, 500);
        assert!(!filter.matches(&file3));
    }

    // === Recoverable file creation tests ===

    #[test]
    fn test_file_from_recycle_bin() {
        let entry = RecycleBinEntry::new(
            1,
            "/home/user/doc.pdf",
            "/trash/1",
            1024,
            1_700_000_000,
            FileSignatureKind::Pdf,
        );
        let file = RecoverableFile::from_recycle_bin(&entry);
        assert_eq!(file.filename, "doc.pdf");
        assert_eq!(file.confidence, RecoveryConfidence::High);
        assert_eq!(file.source, DeletionSource::RecycleBin);
        assert_eq!(file.recovery_percent, 100);
    }

    #[test]
    fn test_file_from_inode_with_dir_entry() {
        let inode = Ext4Inode::new_deleted(100, 4096).with_direct_blocks(vec![1000, 1001]);
        let dir = Ext4DirEntry {
            inode_number: 100,
            name: String::from("test.txt"),
            file_type: InodeFileType::Regular,
            deleted: true,
        };
        let file = RecoverableFile::from_inode(&inode, Some(&dir));
        assert_eq!(file.filename, "test.txt");
        assert_eq!(file.source, DeletionSource::DirectoryRemnant);
        assert!(file.original_path.is_some());
    }

    #[test]
    fn test_file_from_inode_without_dir_entry() {
        let inode = Ext4Inode::new_deleted(100, 4096);
        let file = RecoverableFile::from_inode(&inode, None);
        assert_eq!(file.filename, "inode_100");
        assert_eq!(file.source, DeletionSource::InodeScan);
        assert!(file.original_path.is_none());
    }

    #[test]
    fn test_file_from_signature() {
        let file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0x1000, 2048);
        assert_eq!(file.filename, "recovered_00001000.jpg");
        assert_eq!(file.confidence, RecoveryConfidence::Low);
        assert_eq!(file.source, DeletionSource::SignatureScan);
        assert_eq!(file.disk_offset, Some(0x1000));
    }

    // === UI tests ===

    #[test]
    fn test_ui_render_scan_setup() {
        let app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_results() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_results_with_selection() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        app.select_file(0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_recovery_results() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        app.engine.select_all(&app.filter);
        app.start_recovery();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_deep_scan_results() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.scan_mode = ScanMode::Deep;
        app.start_scan();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_category_filter() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        app.set_category_filter(Some(0));
        let cmds = app.render();
        assert!(!cmds.is_empty());
        assert_eq!(app.filter.category, Some(FileCategory::Image));
    }

    #[test]
    fn test_ui_category_filter_clear() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        app.set_category_filter(Some(0));
        app.set_category_filter(None);
        assert!(app.filter.category.is_none());
    }

    #[test]
    fn test_ui_sorting() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        app.toggle_sort(SortField::Size);
        assert_eq!(app.sort_field, SortField::Size);
        assert_eq!(app.sort_direction, SortDirection::Ascending);

        app.toggle_sort(SortField::Size);
        assert_eq!(app.sort_direction, SortDirection::Descending);
    }

    #[test]
    fn test_ui_navigation() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        assert!(app.selected_file_idx.is_none());
        app.select_next();
        assert_eq!(app.selected_file_idx, Some(0));
        app.select_next();
        assert_eq!(app.selected_file_idx, Some(1));
        app.select_prev();
        assert_eq!(app.selected_file_idx, Some(0));
        app.select_prev();
        assert_eq!(app.selected_file_idx, Some(0)); // Can't go below 0
    }

    #[test]
    fn test_ui_toggle_selection() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        app.select_file(0);
        app.toggle_current_selection();
        let file = app.visible_files()[0];
        assert!(file.selected);
    }

    // === Partition tests ===

    #[test]
    fn test_partition_usage_percent() {
        let part = Partition::new("test", "/dev/test", "/", 1000, 300);
        let pct = part.usage_percent();
        assert!((pct - 70.0).abs() < 0.1);
    }

    #[test]
    fn test_partition_usage_percent_zero_total() {
        let part = Partition::new("test", "/dev/test", "/", 0, 0);
        assert!((part.usage_percent() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_simulated_partitions() {
        let parts = simulated_partitions();
        assert_eq!(parts.len(), 3);
        assert!(!parts[0].block_groups.is_empty());
    }

    // === Utility function tests ===

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(2048), "2.0 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1_048_576), "1.0 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_format_size_tb() {
        assert_eq!(format_size(1_099_511_627_776), "1.0 TB");
    }

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "Unknown");
    }

    #[test]
    fn test_format_timestamp_nonzero() {
        let result = format_timestamp(1_700_000_000);
        assert!(!result.is_empty());
        assert_ne!(result, "Unknown");
    }

    #[test]
    fn test_format_hex_preview_empty() {
        assert_eq!(format_hex_preview(&[], 16), "(empty)");
    }

    #[test]
    fn test_format_hex_preview_zero_width() {
        assert_eq!(format_hex_preview(&[1, 2, 3], 0), "(empty)");
    }

    #[test]
    fn test_format_hex_preview_data() {
        let data = b"Hello, World!";
        let result = format_hex_preview(data, 8);
        assert!(result.contains("0000:"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_format_hex_preview_non_printable() {
        let data = [0x00_u8, 0x01, 0xFF, 0x41]; // non-printable + 'A'
        let result = format_hex_preview(&data, 16);
        assert!(result.contains("...A"));
    }

    // === Enumeration tests ===

    #[test]
    fn test_file_signature_kind_display() {
        assert_eq!(FileSignatureKind::Jpeg.display_name(), "JPEG Image");
        assert_eq!(FileSignatureKind::Pdf.display_name(), "PDF Document");
    }

    #[test]
    fn test_file_signature_kind_extension() {
        assert_eq!(FileSignatureKind::Jpeg.extension(), "jpg");
        assert_eq!(FileSignatureKind::Png.extension(), "png");
    }

    #[test]
    fn test_file_category_all_covered() {
        for kind in FileSignatureKind::ALL {
            let _cat = kind.category();
            let _name = kind.display_name();
            let _ext = kind.extension();
            let _color = kind.color();
        }
    }

    #[test]
    fn test_recovery_confidence_ordering() {
        assert!(RecoveryConfidence::High < RecoveryConfidence::Medium);
        assert!(RecoveryConfidence::Medium < RecoveryConfidence::Low);
        assert!(RecoveryConfidence::Low < RecoveryConfidence::Unlikely);
    }

    #[test]
    fn test_recovery_confidence_percentages() {
        let (lo, hi) = RecoveryConfidence::High.percentage_range();
        assert!(lo >= 90 && hi <= 100);
        let (lo, hi) = RecoveryConfidence::Unlikely.percentage_range();
        assert!(lo == 0 && hi < 20);
    }

    #[test]
    fn test_scan_progress_eta_early() {
        let p = ScanProgress::new();
        assert!(p.estimated_remaining_seconds().is_none());
    }

    #[test]
    fn test_scan_progress_eta_midway() {
        let mut p = ScanProgress::new();
        p.overall_progress = 0.5;
        p.elapsed_seconds = 10;
        let eta = p.estimated_remaining_seconds();
        assert!(eta.is_some());
        assert_eq!(eta.unwrap(), 10);
    }

    #[test]
    fn test_block_group_descriptor() {
        let bg = BlockGroupDescriptor::new(0);
        assert_eq!(bg.group_number, 0);
        assert_eq!(bg.inode_count, 8192);
        assert!(bg.inode_table_block > 0);
    }

    #[test]
    fn test_sort_direction_toggle() {
        assert_eq!(SortDirection::Ascending.toggle(), SortDirection::Descending);
        assert_eq!(SortDirection::Descending.toggle(), SortDirection::Ascending);
    }

    #[test]
    fn test_sort_direction_indicator() {
        assert_eq!(SortDirection::Ascending.indicator(), " ^");
        assert_eq!(SortDirection::Descending.indicator(), " v");
    }

    #[test]
    fn test_deletion_source_descriptions() {
        for source in &[
            DeletionSource::RecycleBin,
            DeletionSource::InodeScan,
            DeletionSource::SignatureScan,
            DeletionSource::DirectoryRemnant,
        ] {
            assert!(!source.display_name().is_empty());
            assert!(!source.description().is_empty());
        }
    }

    #[test]
    fn test_inode_file_type_display() {
        assert_eq!(InodeFileType::Regular.display_name(), "Regular file");
        assert_eq!(InodeFileType::Directory.display_name(), "Directory");
    }

    #[test]
    fn test_scan_phase_display() {
        assert_eq!(ScanPhase::Idle.display_name(), "Idle");
        assert_eq!(ScanPhase::Complete.display_name(), "Scan Complete");
    }

    #[test]
    fn test_scan_mode_display() {
        assert_eq!(ScanMode::Quick.display_name(), "Quick Scan");
        assert_eq!(ScanMode::Deep.display_name(), "Deep Scan");
    }

    #[test]
    fn test_file_category_display() {
        for cat in FileCategory::ALL {
            assert!(!cat.display_name().is_empty());
            let _c = cat.color();
        }
    }

    #[test]
    fn test_visible_files_sorted() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        app.sort_field = SortField::Size;
        app.sort_direction = SortDirection::Ascending;
        let files = app.visible_files();
        for pair in files.windows(2) {
            assert!(pair[0].file_size <= pair[1].file_size);
        }
    }

    #[test]
    fn test_selected_file_returns_none_when_no_selection() {
        let app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(app.selected_file().is_none());
    }

    #[test]
    fn test_recoverable_file_size_display() {
        let file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 1_048_576);
        assert_eq!(file.size_display(), "1.0 MB");
    }

    #[test]
    fn test_recoverable_file_delete_time_display_unknown() {
        let file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 1024);
        assert_eq!(file.delete_time_display(), "Unknown");
    }
}
