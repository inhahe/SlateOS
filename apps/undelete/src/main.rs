//! `Slate OS` File Recovery / Undelete Utility
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

#[allow(unused_imports)]
use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

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

/// How long one phase of a scan is shown for.
///
/// The scan is simulated, so this is the pace of the animation rather than of
/// any work. Long enough to read the phase name -- "Scanning Inode Tables" --
/// which is the only reason the screen exists.
const SCAN_STEP: Duration = Duration::from_millis(500);

/// The sector size a deep scan reads in.
///
/// 512 bytes: the traditional disk sector, and comfortably more than the
/// longest signature in the table plus its secondary pattern (`Docx`, whose
/// confirmation sits at offset 30).
const DEEP_SCAN_SECTOR: usize = 512;

/// How tall one row of the category sidebar is.
const CATEGORY_ROW_HEIGHT: f32 = 28.0;

/// How tall one partition card is on the setup screen.
const PARTITION_CARD_HEIGHT: f32 = 64.0;
const CORNER_RADIUS: f32 = 8.0;

/// Room the preview panel reserves for its "Detection Method" sentence.
///
/// A one-line sentence is shorter than this; the rest of the panel was laid out
/// against this figure, so the short case keeps its original spacing.
const DETECTION_METHOD_ROW_HEIGHT: f32 = 24.0;

/// Share of the preview panel's metadata row given to the value column.
///
/// The label column takes the rest. Written once because the value's `x` and
/// its width are the same quantity seen from two ends, and were previously
/// spelled out as two separate `inner_w * 0.4` / `inner_w * 0.6` expressions
/// that were free to stop adding up.
const META_VALUE_FRACTION: f32 = 0.6;
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
// Recovered-file list geometry
// ============================================================================

/// The recovered-file list's columns: the field each one heads and the share
/// of the panel it spans, left to right.
///
/// One array, because these were previously five quantities written twice and
/// never the same twice. The heading row bounded Name at `width * 0.35` while
/// the row drew it at `width * 0.33`; Type was `0.15` against `0.14`; Deleted
/// `0.18` against `0.16`. Neither number was the column — the column was the
/// distance to the *next* heading, which no line of code mentioned — so no cell
/// was ever checked against the space it actually had, and every one of them
/// clipped with no marker. On this screen that matters more than most: its
/// whole job is to let someone pick which recovered files to restore, from
/// names and paths carved off a damaged disk.
///
/// The fractions sum to 1, which is asserted by a test — a layout that adds up
/// to less leaves dead space, and one that adds up to more runs off the panel.
const FILE_COLUMNS: [(SortField, f32); 5] = [
    (SortField::Filename, 0.35),
    (SortField::Size, 0.15),
    (SortField::FileType, 0.15),
    (SortField::DeleteTime, 0.18),
    (SortField::Confidence, 0.17),
];

const FILE_NAME: usize = 0;
const FILE_SIZE: usize = 1;
const FILE_TYPE: usize = 2;
const FILE_DELETED: usize = 3;
const FILE_CONFIDENCE: usize = 4;

/// Gap between one column's text and the next column's left edge.
const FILE_COL_GAP: f32 = 12.0;

/// Left inset of the first column, clearing the row's selection checkbox.
///
/// Taken back out of the last column so the table still ends at the panel's
/// right edge rather than [`FILE_COL_INSET`] pixels past it.
const FILE_COL_INSET: f32 = 32.0;

/// Side of the colour swatch drawn at the left of the Type column, and the gap
/// between it and the type name.
const TYPE_SWATCH: f32 = 8.0;
const TYPE_SWATCH_GAP: f32 = 4.0;

/// Widest the confidence badge is drawn, and its horizontal interior padding.
const CONF_BADGE_MAX: f32 = 64.0;
const CONF_BADGE_PAD: f32 = 8.0;

/// Column widths for a recovered-file list `width` pixels wide.
///
/// Every width is clamped at zero: a fraction of a narrow panel minus a fixed
/// gap goes negative, and a negative width elides to the empty string, so an
/// unclamped column would blank rather than shrink.
fn file_list_columns(width: f32) -> [Column; 5] {
    let mut columns = [Column {
        label: "",
        width: 0.0,
    }; 5];
    for (column, (field, fraction)) in columns.iter_mut().zip(FILE_COLUMNS) {
        *column = Column {
            // The heading is the sort field's own name, so the label and the
            // thing clicking it sorts by cannot come to disagree.
            label: field.display_name(),
            width: (width * fraction - FILE_COL_GAP).max(0.0),
        };
    }
    if let Some(last) = columns.last_mut() {
        last.width = (last.width - FILE_COL_INSET).max(0.0);
    }
    columns
}

/// Column geometry for a recovered-file list panel at `x`.
///
/// The anchor is pulled back by one gap so that the first column's text starts
/// exactly [`FILE_COL_INSET`] right of the panel edge, where it has always been.
fn file_list_table(columns: &[Column], x: f32) -> Table<'_> {
    Table::with_gap(columns, x + FILE_COL_INSET - FILE_COL_GAP, FILE_COL_GAP)
}

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

    /// The table's own entry for a kind, if it has one.
    fn signature_for(&self, kind: FileSignatureKind) -> Option<&FileSignature> {
        // The most specific first, matching `detect_best`'s preference, so that
        // a kind with a secondary pattern is planted with it -- a JPEG written
        // without its JFIF marker is not what `detect_best` would call a JPEG.
        self.signatures
            .iter()
            .find(|sig| sig.kind == kind && sig.secondary.is_some())
            .or_else(|| self.signatures.iter().find(|sig| sig.kind == kind))
    }

    /// Lay out one sector per planted file, each carrying that kind's real
    /// magic bytes.
    ///
    /// This is the stand-in for a device. A real deep scan reads raw sectors
    /// off the partition; there is none here, so the simulation writes the
    /// sectors that matter and the scan reads them back -- through
    /// [`Self::scan_sectors`] and [`Self::detect_best`], which is the point.
    ///
    /// Before this the deep scan returned a hard-coded list of ten finds and
    /// never consulted the detector at all: it was constructed, stored on the
    /// engine, and read by nothing, so "sector-by-sector signature detection"
    /// detected nothing and the table's twenty-odd entries could all have been
    /// wrong without any screen changing. Going through the table puts it on
    /// the production path, so a bad entry now costs a file type in the
    /// results -- the same failure a real disk would produce.
    ///
    /// A kind the table does not know is planted as a sector of zeroes, and the
    /// scan finds nothing there. That is the honest outcome: the detector
    /// cannot report what it has no signature for.
    pub fn plant_sectors(&self, kinds: &[FileSignatureKind], sector_size: usize) -> Vec<u8> {
        let mut image = vec![0u8; kinds.len().saturating_mul(sector_size)];
        for (index, kind) in kinds.iter().enumerate() {
            let Some(sig) = self.signature_for(*kind) else {
                continue;
            };
            let base = index.saturating_mul(sector_size);
            let mut write = |offset: usize, bytes: &[u8]| {
                let from = base.saturating_add(offset);
                let to = from.saturating_add(bytes.len());
                if let Some(slot) = image.get_mut(from..to) {
                    slot.copy_from_slice(bytes);
                }
            };
            write(sig.offset, &sig.magic);
            if let Some((offset, pattern)) = sig.secondary.as_ref() {
                write(*offset, pattern);
            }
        }
        image
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
    /// Scan a partition from beginning to end, in one call.
    ///
    /// Every phase, and the progress recorded at each one, without returning in
    /// between -- so nothing can draw a frame while it runs. That is what it
    /// was, and what it stays for callers that want a finished result: the
    /// tests, and anything that is not a window. A window uses
    /// [`Self::begin_scan`] and [`Self::scan_step`], which are the same phases
    /// with a return between them.
    pub fn scan(&mut self, partition: &Partition, mode: ScanMode) {
        self.begin_scan(partition, mode);
        // Bounded by the phase machine: each step advances `progress.phase`
        // along a fixed sequence ending at `Complete`, so this cannot spin.
        while self.scan_step(partition) {}
    }

    /// Start a scan, without doing any of it.
    ///
    /// The scan's four phases each record what they are doing --
    /// `progress.phase`, `phase_progress`, `overall_progress`, `files_found` --
    /// and the whole point of recording it is the screen `render_scanning`
    /// draws from it. That screen was unreachable: `scan` ran every phase
    /// inside one call, so each of those values was overwritten by the next
    /// phase before any frame could be drawn, and `UiScreen::Scanning` was
    /// never set by anything. A progress bar that goes from 0 to 1 with no
    /// frame in between is a progress bar nobody has ever seen.
    pub fn begin_scan(&mut self, partition: &Partition, mode: ScanMode) {
        self.files.clear();
        self.scan_mode = mode;
        self.progress = ScanProgress::new();
        self.progress.total_bytes = partition.total_bytes;
        self.progress.phase = ScanPhase::RecycleBin;
        self.progress.phase_progress = 0.0;
    }

    /// Do the next phase of a scan. `false` once there is nothing left.
    ///
    /// One phase per call rather than one *inode* per call: a phase is the unit
    /// the progress screen names, so it is the unit at which the name on screen
    /// changes. Finer steps would move the bar more smoothly and say the same
    /// four things while doing it.
    pub fn scan_step(&mut self, partition: &Partition) -> bool {
        let deep = self.scan_mode == ScanMode::Deep;
        match self.progress.phase {
            ScanPhase::RecycleBin => {
                self.scan_recycle_bin(partition);
                self.progress.files_found = self.files.len();
                self.progress.phase_progress = 1.0;
                self.progress.overall_progress = 0.2;
                self.progress.phase = ScanPhase::InodeScan;
                true
            }
            ScanPhase::InodeScan => {
                self.scan_inodes(partition);
                self.progress.files_found = self.files.len();
                self.progress.phase_progress = 1.0;
                self.progress.overall_progress = if deep { 0.4 } else { 0.8 };
                self.progress.phase = if deep {
                    ScanPhase::DeepScan
                } else {
                    ScanPhase::Analyzing
                };
                true
            }
            ScanPhase::DeepScan => {
                self.scan_deep(partition);
                self.progress.files_found = self.files.len();
                self.progress.phase_progress = 1.0;
                self.progress.overall_progress = 0.9;
                self.progress.phase = ScanPhase::Analyzing;
                true
            }
            ScanPhase::Analyzing => {
                self.deduplicate_results();
                self.progress.files_found = self.files.len();
                self.progress.phase = ScanPhase::Complete;
                self.progress.overall_progress = 1.0;
                self.progress.phase_progress = 1.0;
                // What a scan of this size takes on real hardware, which is not
                // what this animation takes. Kept because the results screen
                // reports it and because `estimated_remaining_seconds` divides
                // by it -- a zero here would make the estimate `None` for the
                // whole scan.
                self.progress.elapsed_seconds = if deep { 45 } else { 12 };
                false
            }
            ScanPhase::Idle | ScanPhase::Complete => false,
        }
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

    /// Read the disk sector by sector, and take whatever the detector
    /// recognises.
    ///
    /// The disk is simulated -- `plant_sectors` writes the sectors this would
    /// otherwise read -- but the *detection* is not: every find below comes out
    /// of `scan_sectors`, so what the results list shows is what the signature
    /// table can actually recognise. The kind is the detector's answer and not
    /// the plan's, which is the whole difference: if an entry in the table
    /// names the wrong magic bytes, that file type stops appearing.
    fn scan_deep(&mut self, partition: &Partition) {
        // Where the simulation says these files are on the real disk, and how
        // big they are -- neither of which a signature can tell you, so neither
        // of which comes back from the scan.
        let planted: [(u64, FileSignatureKind, u64); 10] = [
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

        let kinds: Vec<FileSignatureKind> = planted.iter().map(|(_, kind, _)| *kind).collect();
        let image = self
            .signature_detector
            .plant_sectors(&kinds, DEEP_SCAN_SECTOR);

        for (image_offset, kind) in self
            .signature_detector
            .scan_sectors(&image, DEEP_SCAN_SECTOR)
        {
            // `checked_div` states the non-zero divisor in the arithmetic;
            // `DEEP_SCAN_SECTOR` is a positive constant, so the `else` is
            // unreachable and skipping the find is the right thing anyway.
            let Some(index) = image_offset
                .checked_div(DEEP_SCAN_SECTOR as u64)
                .and_then(|i| usize::try_from(i).ok())
            else {
                continue;
            };
            let Some(&(disk_offset, _, size)) = planted.get(index) else {
                continue;
            };
            let mut file = RecoverableFile::from_signature(self.next_id, kind, disk_offset, size);
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

/// Something on screen that can be clicked, and what clicking it does.
///
/// See [`UndeleteApp::controls`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Choose a partition to scan.
    Partition(usize),
    /// Choose quick or deep scanning.
    Mode(ScanMode),
    /// Begin the scan.
    StartScan,
    /// Filter the results to one category, or `None` for all of them.
    Category(Option<usize>),
    /// Sort by a column; again to reverse it.
    SortBy(SortField),
    /// Select a file in the results list; again to tick it for recovery.
    File(usize),
    /// Tick every file the current filter shows.
    SelectAll,
    /// Go back to the setup screen.
    NewScan,
    /// Recover everything ticked.
    Recover,
    /// Leave the recovery report.
    Done,
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
    /// Begin scanning the selected partition.
    ///
    /// The scan runs a phase per tick from here, so the progress screen is
    /// drawn between the phases -- it used to run the whole scan in this
    /// function and jump straight to the results, which is why
    /// `UiScreen::Scanning` was never set by anything and `render_scanning`
    /// could not be reached.
    pub fn start_scan(&mut self) {
        let Some(partition) = self.partitions.get(self.selected_partition).cloned() else {
            return;
        };
        self.engine.begin_scan(&partition, self.scan_mode);
        self.screen = UiScreen::Scanning;
        self.selected_file_idx = None;
        self.scroll_offset = 0;
        self.clear_filters();
    }

    /// Run recovery on selected files.
    pub fn start_recovery(&mut self) {
        self.recovery_results = self.engine.recover_selected(&self.recovery_target);
        self.screen = UiScreen::Recovering;
    }

    // ====================================================================
    // Input
    //
    // This program had none: no key handler, no mouse handler, no
    // `handle_event`. Nineteen functions had no caller outside the tests,
    // including the whole of the filter builder -- `with_file_type`,
    // `with_min_size`, `with_max_size`, `with_min_confidence`, `with_search`,
    // `with_source`, `with_delete_time_range` -- which is four of the bullet
    // points in this file's own module doc, and `select_next`/`select_prev`,
    // which is the keyboard.
    // ====================================================================

    /// Handle one event from the window.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                self.width = *width as f32;
                self.height = *height as f32;
                self.clamp_scroll();
                EventResult::Consumed
            }
            Event::Tick { .. } => self.handle_tick(),
            _ => EventResult::Ignored,
        }
    }

    /// One phase of a running scan.
    fn handle_tick(&mut self) -> EventResult {
        if self.screen != UiScreen::Scanning {
            return EventResult::Ignored;
        }
        let Some(partition) = self.partitions.get(self.selected_partition).cloned() else {
            // Nothing to scan. Back to the setup screen rather than parking on
            // a progress bar that will never move.
            self.screen = UiScreen::ScanSetup;
            return EventResult::Consumed;
        };
        if !self.engine.scan_step(&partition) {
            self.screen = UiScreen::Results;
            self.selected_file_idx = None;
            self.scroll_offset = 0;
        }
        EventResult::Consumed
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if key.modifiers.ctrl {
            return match key.key {
                Key::A => {
                    self.engine.select_all(&self.filter);
                    EventResult::Consumed
                }
                Key::D => {
                    // `deselect_all` was written and never called, so a
                    // select-all was a decision with no way back short of
                    // clicking every row again.
                    self.engine.deselect_all();
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }

        match self.screen {
            UiScreen::ScanSetup => self.handle_setup_key(key),
            UiScreen::Scanning => {
                if key.key == Key::Escape {
                    self.screen = UiScreen::ScanSetup;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            UiScreen::Results => self.handle_results_key(key),
            UiScreen::Recovering => {
                if matches!(key.key, Key::Escape | Key::Enter) {
                    self.screen = UiScreen::Results;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    /// Keys on the setup screen.
    fn handle_setup_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up => {
                self.selected_partition = self.selected_partition.saturating_sub(1);
                EventResult::Consumed
            }
            Key::Down => {
                let last = self.partitions.len().saturating_sub(1);
                self.selected_partition = self.selected_partition.saturating_add(1).min(last);
                EventResult::Consumed
            }
            Key::Tab => {
                self.scan_mode = match self.scan_mode {
                    ScanMode::Quick => ScanMode::Deep,
                    ScanMode::Deep => ScanMode::Quick,
                };
                EventResult::Consumed
            }
            Key::Enter => {
                self.start_scan();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Keys on the results screen.
    fn handle_results_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up => {
                self.select_prev();
                EventResult::Consumed
            }
            Key::Down => {
                self.select_next();
                EventResult::Consumed
            }
            Key::Home => {
                self.select_file(0);
                EventResult::Consumed
            }
            Key::End => {
                let last = self.visible_files().len().saturating_sub(1);
                self.select_file(last);
                EventResult::Consumed
            }
            Key::Space => {
                self.toggle_current_selection();
                EventResult::Consumed
            }
            Key::Enter => {
                if self.engine.selected_count() > 0 {
                    self.start_recovery();
                }
                EventResult::Consumed
            }
            Key::Tab => {
                // Through the sort columns, and a second visit to the same
                // column reverses it -- which is what `toggle_sort` is for and
                // what nothing called.
                let fields: Vec<SortField> = FILE_COLUMNS.iter().map(|(field, _)| *field).collect();
                let current = fields.iter().position(|f| *f == self.sort_field);
                let step: isize = if key.modifiers.shift { -1 } else { 1 };
                let next = match current {
                    Some(i) => {
                        let count = fields.len() as isize;
                        let at = (i as isize).saturating_add(step).rem_euclid(count);
                        fields
                            .get(at.unsigned_abs())
                            .copied()
                            .unwrap_or(self.sort_field)
                    }
                    // Sorting by a column that is not one of the headings:
                    // reverse the current sort rather than jumping somewhere
                    // the user cannot see.
                    None => self.sort_field,
                };
                self.toggle_sort(next);
                EventResult::Consumed
            }
            Key::Escape => {
                if self.filter.is_active() {
                    // One key that clears every filter, because a search that
                    // hides everything otherwise looks like a scan that found
                    // nothing.
                    self.clear_filters();
                    EventResult::Consumed
                } else {
                    self.screen = UiScreen::ScanSetup;
                    EventResult::Consumed
                }
            }
            Key::Backspace => {
                let mut search = self.filter.filename_search.clone();
                if search.pop().is_none() {
                    return EventResult::Ignored;
                }
                self.set_search(&search);
                EventResult::Consumed
            }
            _ => {
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return EventResult::Ignored;
                }
                let mut search = self.filter.filename_search.clone();
                search.push_str(&typed);
                self.set_search(&search);
                EventResult::Consumed
            }
        }
    }

    /// Handle a mouse event.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_click(mouse.x, mouse.y),
            MouseEventKind::Scroll { dy, .. } => {
                if self.screen != UiScreen::Results {
                    return EventResult::Ignored;
                }
                let rows = guitk::wheel::rows_f(dy);
                let delta = rows as isize;
                self.scroll_offset = self.scroll_offset.saturating_add_signed(delta);
                self.clamp_scroll();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a left click.
    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        if let Some(control) = self
            .controls()
            .into_iter()
            .find(|(rect, _)| rect.contains(x, y))
            .map(|(_, control)| control)
        {
            self.apply_control(control);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    /// Do what a control says.
    fn apply_control(&mut self, control: Control) {
        match control {
            Control::Partition(index) => self.selected_partition = index,
            Control::Mode(mode) => self.scan_mode = mode,
            Control::StartScan => self.start_scan(),
            Control::Category(index) => self.set_category_filter(index),
            Control::SortBy(field) => self.toggle_sort(field),
            Control::File(index) => {
                if self.selected_file_idx == Some(index) {
                    self.toggle_current_selection();
                } else {
                    self.select_file(index);
                }
            }
            Control::SelectAll => self.engine.select_all(&self.filter),
            Control::NewScan => {
                self.screen = UiScreen::ScanSetup;
                self.clear_filters();
            }
            Control::Recover => {
                if self.engine.selected_count() > 0 {
                    self.start_recovery();
                }
            }
            Control::Done => self.screen = UiScreen::Results,
        }
    }

    /// Every control the current screen draws, and where it is.
    ///
    /// One law, two callers: the renderer draws these rectangles and
    /// [`Self::handle_click`] hit-tests them. Nothing outside the render
    /// functions knew where any of this was, which is why none of it could be
    /// pressed.
    pub fn controls(&self) -> Vec<(Rect, Control)> {
        match self.screen {
            UiScreen::ScanSetup => self.setup_controls(),
            UiScreen::Scanning => Vec::new(),
            UiScreen::Results => self.results_controls(),
            UiScreen::Recovering => vec![(
                Rect::new(
                    self.width - BUTTON_WIDTH - PADDING,
                    self.height - FOOTER_HEIGHT + 8.0,
                    BUTTON_WIDTH,
                    BUTTON_HEIGHT,
                ),
                Control::Done,
            )],
        }
    }

    /// The setup screen's partition cards, mode radios and Start button.
    fn setup_controls(&self) -> Vec<(Rect, Control)> {
        let mut out = Vec::new();
        let list_y = HEADER_HEIGHT + PADDING + 28.0;
        let card_w = self.width - PADDING * 2.0;
        for index in 0..self.partitions.len() {
            out.push((
                Rect::new(
                    PADDING,
                    list_y + index as f32 * PARTITION_CARD_HEIGHT,
                    card_w,
                    PARTITION_CARD_HEIGHT - 8.0,
                ),
                Control::Partition(index),
            ));
        }

        let mode_y = list_y + self.partitions.len() as f32 * PARTITION_CARD_HEIGHT + PADDING;
        let quick_y = mode_y + 28.0;
        out.push((
            Rect::new(PADDING, quick_y, card_w, 24.0),
            Control::Mode(ScanMode::Quick),
        ));
        out.push((
            Rect::new(PADDING, quick_y + 32.0, card_w, 24.0),
            Control::Mode(ScanMode::Deep),
        ));

        out.push((
            Rect::new(
                self.width - BUTTON_WIDTH - PADDING,
                self.height - FOOTER_HEIGHT - PADDING,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
            ),
            Control::StartScan,
        ));
        out
    }

    /// The results screen's sidebar, column headers, rows and footer buttons.
    fn results_controls(&self) -> Vec<(Rect, Control)> {
        let mut out = Vec::new();
        let content_y = HEADER_HEIGHT;
        let content_h = self.height - HEADER_HEIGHT - FOOTER_HEIGHT - STATUS_BAR_HEIGHT;

        // Category sidebar: "All", then one row per category.
        let all_y = content_y + 36.0;
        out.push((
            Rect::new(8.0, all_y, SIDEBAR_WIDTH - 16.0, CATEGORY_ROW_HEIGHT),
            Control::Category(None),
        ));
        for index in 0..FileCategory::ALL.len() {
            out.push((
                Rect::new(
                    8.0,
                    all_y + 32.0 + index as f32 * CATEGORY_ROW_HEIGHT,
                    SIDEBAR_WIDTH - 16.0,
                    CATEGORY_ROW_HEIGHT,
                ),
                Control::Category(Some(index)),
            ));
        }

        // Column headers, which sort.
        let list_x = SIDEBAR_WIDTH;
        let list_w = self.width - SIDEBAR_WIDTH - PREVIEW_PANEL_WIDTH;
        let columns = file_list_columns(list_w);
        let table = file_list_table(&columns, list_x);
        for (index, (field, _)) in FILE_COLUMNS.iter().enumerate() {
            // `Table::left`/`width` are the same numbers the header text is
            // drawn at, so a click on a heading lands on the column that
            // heading names.
            out.push((
                Rect::new(table.left(index), content_y, table.width(index), 24.0),
                Control::SortBy(*field),
            ));
        }

        // File rows.
        let list_y = content_y + 28.0;
        let max_visible = ((content_h - 28.0) / ITEM_HEIGHT) as usize;
        let shown = self
            .visible_files()
            .len()
            .saturating_sub(self.scroll_offset)
            .min(max_visible);
        for row in 0..shown {
            out.push((
                Rect::new(
                    list_x,
                    list_y + row as f32 * ITEM_HEIGHT,
                    list_w,
                    ITEM_HEIGHT,
                ),
                Control::File(row.saturating_add(self.scroll_offset)),
            ));
        }

        // Footer buttons, in the order the renderer places them right to left.
        let footer_y = self.height - FOOTER_HEIGHT - STATUS_BAR_HEIGHT + 8.0;
        let btn_x = self.width - BUTTON_WIDTH - PADDING;
        if self.engine.selected_count() > 0 {
            out.push((
                Rect::new(btn_x, footer_y, BUTTON_WIDTH, BUTTON_HEIGHT),
                Control::Recover,
            ));
        }
        out.push((
            Rect::new(
                btn_x - BUTTON_WIDTH - PADDING,
                footer_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
            ),
            Control::SelectAll,
        ));
        out.push((
            Rect::new(
                btn_x - (BUTTON_WIDTH + PADDING) * 2.0,
                footer_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
            ),
            Control::NewScan,
        ));
        out
    }

    /// Set the filename search, and keep the selection on something visible.
    ///
    /// Through `ScanFilter::with_search`, which is the builder this file
    /// already had and which nothing called.
    pub fn set_search(&mut self, query: &str) {
        self.filter = self.filter.clone().with_search(query);
        self.reanchor_selection();
    }

    /// Clear every filter, including the category sidebar's.
    pub fn clear_filters(&mut self) {
        self.filter = ScanFilter::new();
        self.active_category_filter = None;
        self.reanchor_selection();
    }

    /// Keep the selection and the scroll offset inside a list that has just
    /// been re-filtered.
    fn reanchor_selection(&mut self) {
        let count = self.visible_files().len();
        if count == 0 {
            self.selected_file_idx = None;
            self.scroll_offset = 0;
            return;
        }
        if let Some(index) = self.selected_file_idx
            && index >= count
        {
            self.selected_file_idx = Some(count.saturating_sub(1));
        }
        self.clamp_scroll();
    }

    /// Keep the scroll offset from running past the end of the list.
    fn clamp_scroll(&mut self) {
        let content_h = self.height - HEADER_HEIGHT - FOOTER_HEIGHT - STATUS_BAR_HEIGHT;
        let max_visible = ((content_h - 28.0) / ITEM_HEIGHT).max(1.0) as usize;
        let max = self.visible_files().len().saturating_sub(max_visible);
        self.scroll_offset = self.scroll_offset.min(max);
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

    /// Draw the current screen.
    ///
    /// Not `render`: [`App::render`] is the one the window calls, and an
    /// inherent method of the same name shadows a trait method at equal arity.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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

        // Drawn as cells rather than with `Table::header`, because a heading
        // here is not a fixed label: the sorted column gains a direction arrow
        // and is tinted. `cell_weighted` fits it to the same column the body
        // uses, which is the property that was missing — a heading and its
        // rows previously carried different widths for the same column.
        let columns = file_list_columns(width);
        let table = file_list_table(&columns, x);
        for (i, (field, _)) in FILE_COLUMNS.iter().enumerate() {
            let sorted = self.sort_field == *field;
            let label = if sorted {
                format!(
                    "{}{}",
                    field.display_name(),
                    self.sort_direction.indicator()
                )
            } else {
                field.display_name().to_string()
            };
            table.cell_weighted(
                cmds,
                i,
                header_y + 7.0,
                &label,
                if sorted { BLUE } else { SUBTEXT0 },
                FONT_SIZE_SMALL,
                Fit::Start,
                FontWeightHint::Bold,
            );
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

        let columns = file_list_columns(width);
        let table = file_list_table(&columns, x);

        // Filename, and the path it was deleted from beneath it.
        //
        // Both are cut from the *front*. A recovered name is very often one
        // this program generated: `RecoverableFile::from_signature` produces
        // `recovered_{offset:08x}.{ext}` and `from_inode` produces
        // `inode_{n}`, so a deep-scan result set is a column of names sharing
        // a ten-character prefix and differing only in the tail. Cut the usual
        // way they all render as `recovered_00…` — one repeated string in the
        // list the user is choosing from. Cut at the front they keep the
        // offset and the extension, which is the entire difference between
        // them. The same argument, for the same reason, applies to the path:
        // its leaf is what names it.
        table.cell(
            cmds,
            FILE_NAME,
            y + 8.0,
            &file.filename,
            TEXT,
            FONT_SIZE,
            Fit::End,
        );
        if let Some(ref path) = file.original_path {
            table.cell(
                cmds,
                FILE_NAME,
                y + 26.0,
                path,
                OVERLAY0,
                FONT_SIZE_SMALL,
                Fit::End,
            );
        }

        // Size
        table.cell(
            cmds,
            FILE_SIZE,
            y + 18.0,
            &file.size_display(),
            SUBTEXT0,
            FONT_SIZE,
            Fit::Start,
        );

        // File type: a colour swatch, then the name. The swatch's width comes
        // out of the column rather than being added to it, so the name is cut
        // to the room the swatch leaves it.
        cmds.push(RenderCommand::FillRect {
            x: table.left(FILE_TYPE),
            y: y + 20.0,
            width: TYPE_SWATCH,
            height: TYPE_SWATCH,
            color: file.file_type.color(),
            corner_radii: CornerRadii::all(TYPE_SWATCH / 2.0),
        });
        let swatch = TYPE_SWATCH + TYPE_SWATCH_GAP;
        Table::fitted(
            cmds,
            table.left(FILE_TYPE) + swatch,
            table.width(FILE_TYPE) - swatch,
            y + 18.0,
            file.file_type.display_name(),
            SUBTEXT0,
            FONT_SIZE_SMALL,
            Fit::Start,
            FontWeightHint::Regular,
        );

        // Delete time
        table.cell(
            cmds,
            FILE_DELETED,
            y + 18.0,
            &file.delete_time_display(),
            SUBTEXT0,
            FONT_SIZE_SMALL,
            Fit::Start,
        );

        // Confidence badge.
        //
        // The pill is clamped to its column. It used to be a flat 64px drawn
        // at `x + width * 0.83 + 32`, which is inside the panel only while the
        // panel is wide: at width 400 the badge ended 28px past the panel's own
        // right edge, over whatever the results screen draws beside the list.
        let badge_w = table.width(FILE_CONFIDENCE).min(CONF_BADGE_MAX);
        cmds.push(RenderCommand::FillRect {
            x: table.left(FILE_CONFIDENCE),
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
        Table::fitted(
            cmds,
            table.left(FILE_CONFIDENCE) + CONF_BADGE_PAD,
            badge_w - CONF_BADGE_PAD * 2.0,
            y + 19.0,
            file.confidence.display_name(),
            file.confidence.color(),
            FONT_SIZE_SMALL,
            Fit::Start,
            FontWeightHint::Bold,
        );

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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: inner_x + 60.0,
            y: cy + 26.0,
            text: file.file_type.display_name().to_string(),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner_w - 64.0),
            overflow: TextOverflow::Ellipsis,
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

        let value_w = inner_w * META_VALUE_FRACTION;
        for (label, value) in &all_meta {
            cmds.push(RenderCommand::Text {
                x: inner_x,
                y: cy,
                text: (*label).to_string(),
                color: OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(inner_w - value_w),
                overflow: TextOverflow::Ellipsis,
            });
            // These rows are a fixed 20px, so a long value is elided rather
            // than wrapped — but *which end* is cut matters. A path's tail is
            // its filename, the one part the user is looking for, so paths are
            // elided from the front: `…/2024/report.pdf`, not `/home/user/Doc…`.
            let fitted = if *label == "Original Path" {
                text::elide_start(
                    value,
                    value_w,
                    "…",
                    FONT_SIZE_SMALL,
                    FontWeightHint::Regular,
                )
            } else {
                text::elide(
                    value,
                    value_w,
                    "…",
                    FONT_SIZE_SMALL,
                    FontWeightHint::Regular,
                )
            };
            cmds.push(RenderCommand::Text {
                x: inner_x + inner_w - value_w,
                y: cy,
                text: fitted,
                color: SUBTEXT1,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(value_w),
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
        cy += 18.0;

        // A full sentence in a 296px column, so it wraps rather than being cut
        // at whatever word the column edge lands on, and the cursor advances by
        // the height the paragraph reports rather than a guess.
        cy += text::Paragraph::new(file.source.description(), SUBTEXT0)
            .at(inner_x, cy, inner_w)
            .font(FONT_SIZE_SMALL, FontWeightHint::Regular)
            .draw(cmds)
            .max(DETECTION_METHOD_ROW_HEIGHT);

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
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: PADDING * 2.0,
            y: content_y + 54.0,
            text: format!("Target: {}", self.recovery_target),
            color: OVERLAY0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - PADDING * 4.0),
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a byte count as a human-readable string.
pub fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

/// Format a unix timestamp as a human-readable date string.
///
/// `"Unknown"` stays here rather than moving into the shared formatter: an
/// inode with a zero deletion time is one whose time this program could not
/// recover, which is a different fact from "deleted at the epoch".
///
/// Everything below that line used to be a home-made calendar, and it was
/// **wrong** — not roughly, but by a fortnight. It derived the year as
/// `days / 365` and the month as `remaining / 30`, so it lost the leap days
/// and the five extra days a year that months actually have; a file deleted
/// on 2026-08-18 was listed as deleted 2026-09-04, and the error grows by
/// about five days for every year that passes. The deletion date is the
/// column a user reads to tell two recoverable copies of the same file
/// apart, so it was the worst field in the program to be wrong in.
///
/// UTC, explicitly, because this program has no zone to read: there is no
/// per-process zone plumbing yet (known-issues `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`).
/// Saying so with `Tz::utc()` leaves a mark that can be found and fixed when
/// there is; `secs % 86400` left none.
pub fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return String::from("Unknown");
    }
    guitk::datetime::stamp(
        i64::try_from(ts).unwrap_or(i64::MAX),
        &guitk::tzrules::Tz::utc(),
    )
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

impl App for UndeleteApp {
    fn title(&self) -> String {
        // What the window is doing, because these four screens are four
        // different jobs and a taskbar entry saying only "Undelete" cannot tell
        // a finished scan from one still running. The harness re-reads this.
        match self.screen {
            UiScreen::ScanSetup => "Undelete".to_string(),
            UiScreen::Scanning => format!(
                "Scanning {}% - Undelete",
                (self.engine.progress.overall_progress * 100.0) as u32
            ),
            UiScreen::Results => {
                format!("{} recoverable files - Undelete", self.engine.files.len())
            }
            UiScreen::Recovering => format!(
                "Recovered {} files - Undelete",
                self.recovery_results.iter().filter(|r| r.success).count()
            ),
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// A clock only while a scan is running.
    ///
    /// Nothing else in this program ages: a results list does not change until
    /// the user changes it, and a finished recovery is finished. A scan does,
    /// and the screen that shows it -- phase name, phase bar, overall bar,
    /// files found so far -- could not be reached at all before, because the
    /// scan ran its four phases inside one call. See
    /// [`RecoveryEngine::begin_scan`].
    fn tick_interval(&self) -> Option<Duration> {
        (self.screen == UiScreen::Scanning).then_some(SCAN_STEP)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // From the frame being drawn, not from the last `Resize`: the first
        // frame is drawn before any `Resize` arrives, and every hit test here
        // is derived from these two numbers.
        self.width = width;
        self.height = height;
        RenderTree {
            commands: self.render_commands(),
        }
    }
}

fn main() -> ExitCode {
    let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    app::launch("undelete", &mut app)
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

    // ------------------------------------------------------------------
    // Input
    //
    // This program had no key handler, no mouse handler and no
    // `handle_event`. All of the below is new.
    // ------------------------------------------------------------------

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        })
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        })
    }

    fn types(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: c.to_string(),
        })
    }

    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn click_control(app: &mut UndeleteApp, wanted: Control) {
        let rect = app
            .controls()
            .into_iter()
            .find(|(_, c)| *c == wanted)
            .unwrap_or_else(|| panic!("{wanted:?} is not drawn on this screen"))
            .0;
        app.handle_event(&click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
    }

    fn scanned() -> UndeleteApp {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
        app
    }

    // -- the scan is something you can watch --

    /// `UiScreen::Scanning` was never set by anything, so the whole progress
    /// screen -- phase name, phase bar, overall bar, files found -- was
    /// unreachable. The scan ran its four phases inside one call.
    #[test]
    fn a_scan_passes_through_its_phases_where_they_can_be_seen() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.scan_mode = ScanMode::Deep;
        app.start_scan();

        assert_eq!(app.screen, UiScreen::Scanning);
        assert_eq!(
            app.tick_interval(),
            Some(SCAN_STEP),
            "a scanning window needs a clock; nothing else here does"
        );

        let mut phases = vec![app.engine.progress.phase];
        let mut progress = vec![app.engine.progress.overall_progress];
        for _ in 0..16 {
            if app.screen != UiScreen::Scanning {
                break;
            }
            app.handle_event(&Event::Tick { elapsed_ms: 500 });
            phases.push(app.engine.progress.phase);
            progress.push(app.engine.progress.overall_progress);
        }

        assert_eq!(app.screen, UiScreen::Results);
        assert!(
            phases.contains(&ScanPhase::InodeScan)
                && phases.contains(&ScanPhase::DeepScan)
                && phases.contains(&ScanPhase::Analyzing),
            "each phase should have been the current one at some point, got {phases:?}"
        );
        for pair in progress.windows(2) {
            assert!(pair[1] >= pair[0], "the progress bar went backwards");
        }
        assert!(
            progress.iter().any(|p| *p > 0.0 && *p < 1.0),
            "there was never a frame with the bar part-way across"
        );
        assert_eq!(app.tick_interval(), None, "and the clock stops afterwards");
    }

    /// A quick scan skips the deep phase, and says so on the way past.
    #[test]
    fn a_quick_scan_does_not_go_through_the_deep_phase() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        let mut phases = Vec::new();
        for _ in 0..16 {
            if app.screen != UiScreen::Scanning {
                break;
            }
            phases.push(app.engine.progress.phase);
            app.handle_event(&Event::Tick { elapsed_ms: 500 });
        }
        assert!(!phases.contains(&ScanPhase::DeepScan), "got {phases:?}");
    }

    /// The one-call `scan` still works, and gives the same answer as the
    /// stepwise one -- which is what lets the two exist without drifting.
    #[test]
    fn the_stepwise_scan_finds_what_the_one_call_scan_finds() {
        let partition = simulated_partitions().remove(0);
        let mut whole = RecoveryEngine::new();
        whole.scan(&partition, ScanMode::Deep);

        let mut stepped = RecoveryEngine::new();
        stepped.begin_scan(&partition, ScanMode::Deep);
        while stepped.scan_step(&partition) {}

        assert_eq!(whole.files.len(), stepped.files.len());
        assert_eq!(whole.progress.phase, ScanPhase::Complete);
        assert_eq!(stepped.progress.phase, ScanPhase::Complete);
    }

    // -- the deep scan actually detects --

    /// "Sector-by-sector signature detection" detected nothing: `scan_deep`
    /// returned ten hard-coded finds and `signature_detector` was never read.
    /// The table's twenty-odd entries could all have been wrong.
    #[test]
    fn the_deep_scan_finds_its_files_through_the_signature_table() {
        let detector = SignatureDetector::new();
        let kinds = [
            FileSignatureKind::Jpeg,
            FileSignatureKind::Png,
            FileSignatureKind::Pdf,
        ];
        let image = detector.plant_sectors(&kinds, DEEP_SCAN_SECTOR);
        let found = detector.scan_sectors(&image, DEEP_SCAN_SECTOR);

        assert_eq!(found.len(), 3, "every planted sector should be recognised");
        for (index, kind) in &kinds.iter().enumerate().collect::<Vec<_>>() {
            let at = (index * DEEP_SCAN_SECTOR) as u64;
            assert!(
                found.contains(&(at, **kind)),
                "{kind:?} was planted at {at} and not found: {found:?}"
            );
        }
    }

    /// The consequence that makes it worth routing through the table: a wrong
    /// entry costs a file type in the results, rather than nothing at all.
    #[test]
    fn a_kind_the_table_does_not_know_is_simply_not_found() {
        let detector = SignatureDetector::new();
        // A sector of zeroes stands for a kind with no signature.
        let image = vec![0u8; DEEP_SCAN_SECTOR];
        assert!(
            detector.scan_sectors(&image, DEEP_SCAN_SECTOR).is_empty(),
            "the detector reported something in an empty sector"
        );
    }

    /// The point of routing the deep scan through the table: the *kind* each
    /// find is reported as comes back from the detector. Asserting only that
    /// the right number of files turn up would pass just as well against the
    /// hard-coded list this replaced -- so what is asserted is the variety.
    #[test]
    fn the_deep_scans_file_types_come_from_the_detector() {
        let partition = simulated_partitions().remove(0);
        let mut quick = RecoveryEngine::new();
        quick.scan(&partition, ScanMode::Quick);
        let mut deep = RecoveryEngine::new();
        deep.scan(&partition, ScanMode::Deep);

        let mut kinds: Vec<FileSignatureKind> = deep
            .files
            .iter()
            .filter(|f| !quick.files.iter().any(|q| q.id == f.id))
            .map(|f| f.file_type)
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert!(
            kinds.len() >= 5,
            "the signature-only finds should be of several types, got {kinds:?} -- \
             one type for all of them means the kind is not coming from the scan"
        );
        assert!(
            kinds.contains(&FileSignatureKind::Png) && kinds.contains(&FileSignatureKind::Pdf),
            "got {kinds:?}"
        );
    }

    /// A signature with a confirming pattern is only that signature when the
    /// pattern is there too -- which is what `detect_best` prefers and what a
    /// planted sector therefore has to carry.
    #[test]
    fn a_signature_with_a_secondary_pattern_needs_both_halves() {
        let detector = SignatureDetector::new();
        // Docx is Zip's magic plus `word/` at offset 30. Planted whole, it is
        // a Docx; with only the first half it is a Zip, which is exactly the
        // misreading the secondary exists to prevent.
        let image = detector.plant_sectors(&[FileSignatureKind::Docx], DEEP_SCAN_SECTOR);
        assert_eq!(
            detector.scan_sectors(&image, DEEP_SCAN_SECTOR),
            vec![(0, FileSignatureKind::Docx)],
            "a planted Docx should come back a Docx"
        );

        let mut without = image.clone();
        if let Some(slot) = without.get_mut(30..35) {
            slot.fill(0);
        }
        assert_eq!(
            detector.scan_sectors(&without, DEEP_SCAN_SECTOR),
            vec![(0, FileSignatureKind::Zip)],
            "without its confirmation it is only a zip"
        );
    }

    #[test]
    fn a_deep_scan_recovers_more_than_a_quick_one() {
        let partition = simulated_partitions().remove(0);
        let mut quick = RecoveryEngine::new();
        quick.scan(&partition, ScanMode::Quick);
        let mut deep = RecoveryEngine::new();
        deep.scan(&partition, ScanMode::Deep);
        assert!(
            deep.files.len() > quick.files.len(),
            "the deep scan should find the signature-only files as well"
        );
    }

    // -- the setup screen --

    #[test]
    fn the_partition_cards_and_mode_radios_can_be_clicked() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            app.partitions.len() > 1,
            "the sample has several partitions"
        );

        click_control(&mut app, Control::Partition(1));
        assert_eq!(app.selected_partition, 1);

        click_control(&mut app, Control::Mode(ScanMode::Deep));
        assert_eq!(app.scan_mode, ScanMode::Deep);
        click_control(&mut app, Control::Mode(ScanMode::Quick));
        assert_eq!(app.scan_mode, ScanMode::Quick);
    }

    #[test]
    fn the_start_button_starts_the_scan() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        click_control(&mut app, Control::StartScan);
        assert_eq!(app.screen, UiScreen::Scanning);
    }

    #[test]
    fn the_keyboard_chooses_a_partition_and_a_mode() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_partition, 1);
        app.handle_event(&press(Key::Up));
        assert_eq!(app.selected_partition, 0);
        app.handle_event(&press(Key::Up));
        assert_eq!(app.selected_partition, 0, "stops at the first");

        app.handle_event(&press(Key::Tab));
        assert_eq!(app.scan_mode, ScanMode::Deep);
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.screen, UiScreen::Scanning);
    }

    // -- the results screen --

    #[test]
    fn the_arrows_walk_the_results_and_space_ticks_one() {
        let mut app = scanned();
        assert!(!app.visible_files().is_empty());

        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_file_idx, Some(0));
        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_file_idx, Some(1));
        app.handle_event(&press(Key::Up));
        assert_eq!(app.selected_file_idx, Some(0));

        assert_eq!(app.engine.selected_count(), 0);
        app.handle_event(&press(Key::Space));
        assert_eq!(app.engine.selected_count(), 1, "space should tick the row");
        app.handle_event(&press(Key::Space));
        assert_eq!(app.engine.selected_count(), 0, "and untick it");
    }

    /// Typing filters by filename, through `ScanFilter::with_search` -- one of
    /// eight filter builders that had no caller.
    #[test]
    fn typing_filters_by_filename() {
        let mut app = scanned();
        let all = app.visible_files().len();
        let first = app
            .visible_files()
            .first()
            .map(|f| f.filename.clone())
            .expect("the scan found something");
        let needle: String = first.chars().take(3).collect();

        for c in needle.chars() {
            app.handle_event(&types(c));
        }
        assert_eq!(app.filter.filename_search, needle);
        assert!(app.filter.is_active(), "the filter should say it is on");
        let shown = app.visible_files().len();
        assert!(shown <= all);
        assert!(
            app.visible_files()
                .iter()
                .all(|f| f.filename.to_lowercase().contains(&needle.to_lowercase())),
            "a file that does not match the search is still shown"
        );

        app.handle_event(&press(Key::Backspace));
        assert_eq!(app.filter.filename_search, needle[..needle.len() - 1]);

        app.handle_event(&press(Key::Escape));
        assert!(!app.filter.is_active(), "Escape should clear the filters");
        assert_eq!(app.visible_files().len(), all);
    }

    /// A search that shrinks the list has to bring the selection with it. The
    /// selection is a row *number*, so a query that leaves fewer rows than the
    /// selected index leaves it pointing past the end -- at nothing, or at
    /// whatever ends up there next.
    #[test]
    fn a_search_that_shrinks_the_list_moves_the_selection_into_it() {
        let mut app = scanned();
        let all = app.visible_files().len();
        assert!(all > 2, "need a list to shrink");
        app.select_file(all.saturating_sub(1));
        assert_eq!(app.selected_file_idx, Some(all - 1));

        // A query that matches something, but much less than everything.
        let needle = app
            .visible_files()
            .first()
            .map(|f| f.filename.clone())
            .expect("non-empty");
        for c in needle.chars() {
            app.handle_event(&types(c));
        }

        let shown = app.visible_files().len();
        assert!(shown < all, "the search did not narrow anything");
        assert!(
            app.selected_file_idx.is_some_and(|i| i < shown),
            "the selection is at {:?} in a list of {shown}",
            app.selected_file_idx
        );
    }

    /// The category sidebar is drawn with a row per category and a count beside
    /// each; none of them could be clicked.
    #[test]
    fn the_category_sidebar_filters_the_list() {
        let mut app = scanned();
        let all = app.visible_files().len();

        // Whichever category actually has files in it, so the test is about the
        // filter and not about the sample data.
        let index = (0..FileCategory::ALL.len())
            .find(|i| {
                let mut probe = ScanFilter::new();
                if let Some(cat) = FileCategory::ALL.get(*i) {
                    probe = probe.with_category(*cat);
                }
                let n = app.engine.filtered_files(&probe).len();
                n > 0 && n < all
            })
            .expect("some category holds some but not all of the files");

        click_control(&mut app, Control::Category(Some(index)));
        assert_eq!(app.active_category_filter, Some(index));
        let shown = app.visible_files().len();
        assert!(shown > 0 && shown < all, "the sidebar did not filter");

        click_control(&mut app, Control::Category(None));
        assert_eq!(
            app.visible_files().len(),
            all,
            "\"All\" should restore them"
        );
    }

    /// Clicking a column heading sorts by it; clicking again reverses.
    #[test]
    fn the_column_headings_sort_the_list() {
        let mut app = scanned();
        click_control(&mut app, Control::SortBy(SortField::Filename));
        assert_eq!(app.sort_field, SortField::Filename);
        let first = app.visible_files().first().map(|f| f.filename.clone());

        click_control(&mut app, Control::SortBy(SortField::Filename));
        assert_eq!(app.sort_field, SortField::Filename);
        let reversed = app.visible_files().first().map(|f| f.filename.clone());
        assert_ne!(first, reversed, "a second click should reverse the order");
    }

    #[test]
    fn clicking_a_row_selects_it_and_clicking_again_ticks_it() {
        let mut app = scanned();
        click_control(&mut app, Control::File(1));
        assert_eq!(app.selected_file_idx, Some(1));
        assert_eq!(app.engine.selected_count(), 0);

        click_control(&mut app, Control::File(1));
        assert_eq!(app.engine.selected_count(), 1);
    }

    /// Select-all had no way back: `deselect_all` was written and never called.
    #[test]
    fn ctrl_a_ticks_everything_and_ctrl_d_unticks_it() {
        let mut app = scanned();
        app.handle_event(&press_ctrl(Key::A));
        assert!(app.engine.selected_count() > 1);
        app.handle_event(&press_ctrl(Key::D));
        assert_eq!(app.engine.selected_count(), 0);
    }

    #[test]
    fn recovering_needs_something_selected_and_then_reports() {
        let mut app = scanned();
        app.handle_event(&press(Key::Enter));
        assert_eq!(
            app.screen,
            UiScreen::Results,
            "there was nothing ticked, so there is nothing to recover"
        );

        app.handle_event(&press_ctrl(Key::A));
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.screen, UiScreen::Recovering);
        assert!(!app.recovery_results.is_empty());

        app.handle_event(&press(Key::Escape));
        assert_eq!(app.screen, UiScreen::Results, "and back to the list");
    }

    #[test]
    fn new_scan_goes_back_and_forgets_the_filters() {
        let mut app = scanned();
        for c in "zzz".chars() {
            app.handle_event(&types(c));
        }
        assert!(app.filter.is_active());

        click_control(&mut app, Control::NewScan);
        assert_eq!(app.screen, UiScreen::ScanSetup);
        assert!(
            !app.filter.is_active(),
            "a search from the last scan must not hide the results of the next"
        );
    }

    /// Every control the current screen offers is inside the window and none
    /// overlaps another, so each can be hit and none steals its neighbour's
    /// click.
    #[test]
    fn no_two_controls_on_a_screen_overlap() {
        for mut app in [UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT), scanned()] {
            app.handle_event(&press_ctrl(Key::A));
            let controls = app.controls();
            assert!(!controls.is_empty(), "{:?} draws nothing", app.screen);
            for (i, (a, ca)) in controls.iter().enumerate() {
                assert!(
                    a.x >= 0.0 && a.y >= 0.0 && a.x + a.w <= app.width && a.y + a.h <= app.height,
                    "{ca:?} is drawn outside the window"
                );
                for (b, cb) in controls.iter().skip(i + 1) {
                    let apart = a.x + a.w <= b.x
                        || b.x + b.w <= a.x
                        || a.y + a.h <= b.y
                        || b.y + b.h <= a.y;
                    assert!(apart, "{ca:?} overlaps {cb:?}");
                }
            }
        }
    }

    /// The window follows the size it is handed.
    #[test]
    fn the_layout_follows_the_window() {
        let mut app = scanned();
        let before = app
            .controls()
            .into_iter()
            .find(|(_, c)| *c == Control::NewScan)
            .expect("drawn")
            .0;
        let _ = App::render(&mut app, WINDOW_WIDTH + 200.0, WINDOW_HEIGHT);
        let after = app
            .controls()
            .into_iter()
            .find(|(_, c)| *c == Control::NewScan)
            .expect("drawn")
            .0;
        assert!(
            after.x > before.x,
            "the footer buttons did not follow the edge"
        );
    }

    #[test]
    fn the_title_says_what_the_window_is_doing() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(app.title(), "Undelete");
        app.start_scan();
        assert!(app.title().starts_with("Scanning"), "got {:?}", app.title());
        run_scan(&mut app);
        assert!(
            app.title().contains("recoverable files"),
            "got {:?}",
            app.title()
        );
    }

    /// Drive a started scan to completion, one tick per phase.
    ///
    /// `start_scan` begins the scan and the window's ticks finish it, so a test
    /// that wants results has to do what the window does. This is also the only
    /// way the progress screen is reachable at all -- see
    /// `RecoveryEngine::begin_scan`.
    fn run_scan(app: &mut UndeleteApp) {
        // Bounded: the phase machine advances along a fixed sequence, so this
        // cannot spin even if a step stops returning `false`.
        for _ in 0..16 {
            if app.screen != UiScreen::Scanning {
                break;
            }
            app.handle_event(&Event::Tick { elapsed_ms: 500 });
        }
        assert_ne!(app.screen, UiScreen::Scanning, "the scan never finished");
    }

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
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_results() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_results_with_selection() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
        app.select_file(0);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    // --- Preview panel text layout ---

    /// A scanned app with one file selected, ready for the preview panel.
    fn app_with_selection() -> UndeleteApp {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
        app.select_file(0);
        app
    }

    /// Text drawn in the preview panel: `(text, size, weight, max_width)`.
    ///
    /// The weight is carried through because bold text measures wider than
    /// regular at the same size — measuring a bold row as regular would let a
    /// genuine overflow slip past.
    fn preview_panel_text(app: &UndeleteApp) -> Vec<(String, f32, FontWeightHint, f32)> {
        let panel_x = WINDOW_WIDTH - PREVIEW_PANEL_WIDTH;
        app.render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    max_width: Some(w),
                    overflow: TextOverflow::Ellipsis,
                    ..
                } if x >= panel_x => Some((text, font_size, font_weight, w)),
                _ => None,
            })
            .collect()
    }

    /// Everything the preview panel draws fits the column it is drawn in.
    ///
    /// The detection-method line is a full sentence in a 296px column, so it
    /// wraps; the metadata values are fixed-height rows, so they are elided.
    /// Either way, nothing may be silently cut by the render command.
    #[test]
    fn every_preview_panel_line_fits_its_column() {
        let mut app = app_with_selection();
        if let Some(file) = app.engine.files.first_mut() {
            file.original_path =
                Some("/home/user/Documents/archive/2024/quarterly/report-final.pdf".to_string());
            file.partition_name = "a-very-long-partition-label-indeed".to_string();
        }
        let mut checked = 0;
        for (text, size, weight, max_width) in preview_panel_text(&app) {
            let measured = text::measure(&text, size, weight);
            assert!(
                measured <= max_width + 0.5,
                "preview line {text:?} measures {measured} in a {max_width} column",
            );
            checked += 1;
        }
        assert!(
            checked >= 10,
            "expected the panel's lines, checked {checked}"
        );
    }

    /// A path too long for its column is cut at the *front*, because its tail
    /// is the filename — the one part the user is looking for.
    #[test]
    fn a_long_original_path_keeps_its_filename() {
        let mut app = app_with_selection();
        if let Some(file) = app.engine.files.first_mut() {
            file.original_path =
                Some("/home/user/Documents/archive/2024/quarterly/report-final.pdf".to_string());
        }
        let drawn: Vec<String> = preview_panel_text(&app)
            .into_iter()
            .map(|(t, _, _, _)| t)
            .filter(|t| t.contains("report-final.pdf") || t.contains("/home/user"))
            .collect();
        assert_eq!(drawn.len(), 1, "expected one path row, got {drawn:?}");
        assert!(
            drawn[0].ends_with("report-final.pdf"),
            "the filename was elided away: {:?}",
            drawn[0],
        );
        assert!(
            drawn[0].starts_with('…'),
            "expected the cut marked at the front: {:?}",
            drawn[0],
        );
    }

    /// A short value is drawn verbatim — the elision must not touch the common
    /// case.
    #[test]
    fn a_short_metadata_value_is_not_elided() {
        let mut app = app_with_selection();
        if let Some(file) = app.engine.files.first_mut() {
            file.partition_name = "sda1".to_string();
        }
        assert!(
            preview_panel_text(&app)
                .iter()
                .any(|(t, _, _, _)| t == "sda1"),
            "expected the partition name drawn in full",
        );
    }

    #[test]
    fn test_ui_render_recovery_results() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
        app.engine.select_all(&app.filter);
        app.start_recovery();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_deep_scan_results() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.scan_mode = ScanMode::Deep;
        app.start_scan();
        run_scan(&mut app);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_category_filter() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
        app.set_category_filter(Some(0));
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
        assert_eq!(app.filter.category, Some(FileCategory::Image));
    }

    #[test]
    fn test_ui_category_filter_clear() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
        app.set_category_filter(Some(0));
        app.set_category_filter(None);
        assert!(app.filter.category.is_none());
    }

    #[test]
    fn test_ui_sorting() {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.start_scan();
        run_scan(&mut app);
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
        run_scan(&mut app);
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
        run_scan(&mut app);
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
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1_048_576), "1.0 MiB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1_073_741_824), "1.0 GiB");
    }

    #[test]
    fn test_format_size_tb() {
        assert_eq!(format_size(1_099_511_627_776), "1.0 TiB");
    }

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "Unknown");
    }

    /// The date the old home-made calendar got wrong, now asserted by value.
    ///
    /// The test this replaces checked that the string was non-empty and was
    /// not `"Unknown"` — and passed for years while the function reported
    /// 2023-12-01 for an instant in the middle of November. A test that never
    /// looks at the value cannot notice a calendar that is a fortnight out;
    /// that gap is where the bug lived.
    #[test]
    fn test_format_timestamp_nonzero() {
        // 2023-11-14 22:13:20 UTC.
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13");
        // 2026-08-18 16:30:45 UTC — the old code said 2026-09-04.
        assert_eq!(format_timestamp(1_787_070_645), "2026-08-18 16:30");
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
        run_scan(&mut app);
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
        assert_eq!(file.size_display(), "1.0 MiB");
    }

    #[test]
    fn test_recoverable_file_delete_time_display_unknown() {
        let file = RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0, 1024);
        assert_eq!(file.delete_time_display(), "Unknown");
    }

    // === Recovered-file list column fitting ===

    const LIST_X: f32 = 40.0;
    const LIST_Y: f32 = 100.0;
    const LIST_W: f32 = 760.0;
    const LIST_H: f32 = 400.0;

    /// An app whose result list holds files with the long, prefix-sharing
    /// names a real deep scan produces.
    fn app_with_long_recovered_names() -> UndeleteApp {
        let mut app = UndeleteApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.screen = UiScreen::Results;
        app.engine.files = vec![
            RecoverableFile::from_signature(1, FileSignatureKind::Jpeg, 0x0009_a1c4, 4_194_304),
            RecoverableFile::from_signature(2, FileSignatureKind::Jpeg, 0x0009_a1d8, 4_194_304),
            RecoverableFile::from_signature(3, FileSignatureKind::Png, 0x0009_a1ec, 2_097_152),
        ];
        for (file, path) in app.engine.files.iter_mut().zip([
            "/home/user/Pictures/2026/Summer/Portugal/day-three/DSC_04871.jpg",
            "/home/user/Pictures/2026/Summer/Portugal/day-three/DSC_04872.jpg",
            "/home/user/Pictures/2026/Summer/Portugal/day-three/DSC_04873.png",
        ]) {
            file.original_path = Some(String::from(path));
            file.delete_time = 1_700_000_000;
        }
        app
    }

    /// Every `Text` command the list draws, as `(x, text, size, weight)`.
    fn file_list_texts(app: &UndeleteApp, w: f32) -> Vec<(f32, String, f32, FontWeightHint)> {
        let mut cmds = Vec::new();
        app.render_file_list(&mut cmds, LIST_X, LIST_Y, w, LIST_H);
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => Some((*x, text.clone(), *font_size, *font_weight)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_file_list_columns_share_out_the_whole_panel() {
        // Less than 1 leaves dead space at the right; more than 1 puts the
        // last column off the panel. The old layout's numbers summed to
        // neither, because the header's widths and the rows' widths were
        // different sets of numbers and neither set was the column.
        let total: f32 = FILE_COLUMNS.iter().map(|(_, fraction)| fraction).sum();
        assert!(
            (total - 1.0).abs() < 0.0001,
            "column fractions sum to {total}, not 1"
        );
    }

    #[test]
    fn no_recovered_file_cell_escapes_its_column() {
        let app = app_with_long_recovered_names();
        let columns = file_list_columns(LIST_W);
        let table = file_list_table(&columns, LIST_X);
        let spans = table.spans();
        let mut checked = 0usize;
        for (x, drawn, size, weight) in file_list_texts(&app, LIST_W) {
            let (_, right) = spans
                .iter()
                .copied()
                .find(|(l, r)| x >= l - 0.01 && x <= r + 0.01)
                .unwrap_or_else(|| panic!("cell {drawn:?} at {x} is not inside any column"));
            let ends = x + text::measure(&drawn, size, weight);
            assert!(
                ends <= right + 0.01,
                "cell {drawn:?} at {x} draws to {ends}, past its column edge {right}"
            );
            checked = checked.saturating_add(1);
        }
        // Five headings, plus name/path/size/type/time/confidence per row.
        assert!(checked >= 5 + 3 * 6, "only {checked} cells checked");
    }

    #[test]
    fn the_file_list_ends_inside_its_panel() {
        // A per-cell bound says nothing about the row. The last column has to
        // be asked separately whether it lands inside the panel it was
        // apportioned from.
        for w in [280.0_f32, 480.0, LIST_W, 1600.0] {
            let columns = file_list_columns(w);
            let table = file_list_table(&columns, LIST_X);
            let end = table.right(FILE_CONFIDENCE);
            assert!(
                end <= LIST_X + w + 0.01,
                "at width {w} the last column ends at {end}, past the panel edge {}",
                LIST_X + w
            );
        }
    }

    #[test]
    fn an_overlong_recovered_name_keeps_what_tells_it_apart() {
        // `from_signature` names files `recovered_{offset:08x}.{ext}`, so a
        // scan's results share a ten-character prefix. Cut at the end they all
        // render identically, and "it was marked as cut" would still pass —
        // which is why the assertion is that they are *distinct*.
        let app = app_with_long_recovered_names();
        let names: Vec<String> = app
            .engine
            .files
            .iter()
            .map(|f| f.filename.clone())
            .collect();
        assert!(
            names.iter().all(|n| n.starts_with("recovered_")),
            "test fixture no longer shares a prefix: {names:?}"
        );

        // Narrow enough that the full name cannot fit.
        let w = 360.0;
        let columns = file_list_columns(w);
        let table = file_list_table(&columns, LIST_X);
        let drawn: Vec<String> = file_list_texts(&app, w)
            .into_iter()
            .filter(|(x, t, ..)| (x - table.left(FILE_NAME)).abs() < 0.01 && t.ends_with(".jpg"))
            .map(|(_, t, ..)| t)
            .collect();
        assert!(drawn.len() >= 2, "expected cut names, got {drawn:?}");
        assert!(
            drawn.iter().any(|t| t.starts_with('…')),
            "names were not actually cut: {drawn:?}"
        );
        let distinct: std::collections::BTreeSet<&String> = drawn.iter().collect();
        assert_eq!(
            distinct.len(),
            drawn.len(),
            "cut names collapsed to the same string: {drawn:?}"
        );
    }

    #[test]
    fn the_confidence_badge_stays_inside_the_narrow_panel() {
        // The badge was a flat 64px pill placed at `x + width * 0.83 + 32`.
        // That is inside the panel only while the panel is wide.
        for w in [240.0_f32, 320.0, 400.0, LIST_W] {
            let mut cmds = Vec::new();
            let app = app_with_long_recovered_names();
            app.render_file_list(&mut cmds, LIST_X, LIST_Y, w, LIST_H);
            let columns = file_list_columns(w);
            let table = file_list_table(&columns, LIST_X);
            let badge_left = table.left(FILE_CONFIDENCE);
            for cmd in &cmds {
                if let RenderCommand::FillRect {
                    x, width: rect_w, ..
                } = cmd
                    && (x - badge_left).abs() < 0.01
                {
                    assert!(
                        x + rect_w <= LIST_X + w + 0.01,
                        "at width {w} the badge spans {x}..{} past the panel edge {}",
                        x + rect_w,
                        LIST_X + w
                    );
                }
            }
        }
    }

    #[test]
    fn the_file_list_header_and_rows_agree_on_where_a_column_starts() {
        let app = app_with_long_recovered_names();
        let columns = file_list_columns(LIST_W);
        let table = file_list_table(&columns, LIST_X);
        let heading_x: Vec<f32> = file_list_texts(&app, LIST_W)
            .into_iter()
            .take(FILE_COLUMNS.len())
            .map(|(x, ..)| x)
            .collect();
        let expected: Vec<f32> = (0..FILE_COLUMNS.len()).map(|i| table.left(i)).collect();
        assert_eq!(heading_x.len(), expected.len());
        for (got, want) in heading_x.iter().zip(&expected) {
            assert!(
                (got - want).abs() < 0.01,
                "heading at {got}, column at {want}"
            );
        }
    }

    #[test]
    fn a_narrow_panel_does_not_give_a_column_negative_width() {
        // A fraction of a small panel minus a fixed gap is negative, and
        // `text::elide` of a negative width returns "" — the column would
        // blank rather than shrink.
        for w in [0.0_f32, 1.0, 20.0, 60.0, 120.0] {
            for column in file_list_columns(w) {
                assert!(
                    column.width >= 0.0,
                    "column {:?} is {} wide at panel width {w}",
                    column.label,
                    column.width
                );
            }
        }
    }

    #[test]
    fn a_short_recovered_name_is_drawn_verbatim() {
        let mut app = app_with_long_recovered_names();
        if let Some(file) = app.engine.files.first_mut() {
            file.filename = String::from("notes.txt");
            file.original_path = None;
        }
        let drawn = file_list_texts(&app, LIST_W);
        assert!(
            drawn.iter().any(|(_, t, ..)| t == "notes.txt"),
            "a name that fits was altered: {drawn:?}"
        );
    }
}
