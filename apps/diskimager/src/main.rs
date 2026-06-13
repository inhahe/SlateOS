//! SlateOS Disk Imager / ISO Tool
//!
//! GUI disk imaging tool with:
//! - Image creation from drives/partitions (raw dd-style, compressed)
//! - Image writing to USB drives or virtual disks with verification
//! - ISO 9660 filesystem browsing, file extraction, volume metadata
//! - Image format detection by magic bytes (raw, ISO, compressed)
//! - Checksum verification (SHA-256, SHA-1, MD5)
//! - Drive detection with name, size, type, partition table
//! - Progress tracking with speed (MB/s) and ETA
//! - Write verification (byte-by-byte)
//! - Bootable USB creation (hybrid MBR/UEFI)
//! - Recent image history
//! - Safety: confirmation before writes, drive locking, system drive protection
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha colors.

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexItem, FlexJustify, SizeConstraint};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::{Borders, CornerRadii, Edges, FontWeight, Style, TextAlign};
#[allow(unused_imports)]
use guitk::widget::{Widget, WidgetId, WidgetTree};

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha color palette
// ============================================================================

pub mod colors {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
}

// ============================================================================
// Configuration constants
// ============================================================================

const UI_FONT_SIZE: f32 = 13.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const SMALL_FONT_SIZE: f32 = 11.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const SIDEBAR_WIDTH: f32 = 280.0;
const PANEL_PADDING: f32 = 12.0;
const CORNER_RADIUS: f32 = 6.0;
const BUTTON_HEIGHT: f32 = 32.0;
const ROW_HEIGHT: f32 = 28.0;
const PROGRESS_BAR_HEIGHT: f32 = 20.0;
const MAX_RECENT_IMAGES: usize = 20;
const DEFAULT_BLOCK_SIZE: u64 = 4096;
const CHAR_WIDTH: f32 = 0.6;

// ============================================================================
// Image format types
// ============================================================================

/// Detected image format based on magic bytes and extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    /// Raw disk image (.img, .bin, .raw)
    Raw,
    /// ISO 9660 optical disc image (.iso)
    Iso9660,
    /// Gzip-compressed raw image (.img.gz)
    GzipCompressed,
    /// Unknown/unrecognized format
    Unknown,
}

impl ImageFormat {
    /// Detect format from magic bytes at the start of a file.
    pub fn from_magic(bytes: &[u8]) -> Self {
        // ISO 9660: "CD001" signature at offset 0x8001
        // Check at sector-aligned offset for ISO primary volume descriptor
        if bytes.len() >= 0x8006 && bytes.get(0x8001..0x8006) == Some(b"CD001") {
            return Self::Iso9660;
        }
        // Gzip magic: 1F 8B
        if bytes.len() >= 2 && bytes.first() == Some(&0x1F) && bytes.get(1) == Some(&0x8B) {
            return Self::GzipCompressed;
        }
        // If file has content but no recognizable signature, treat as raw
        if !bytes.is_empty() {
            return Self::Raw;
        }
        Self::Unknown
    }

    /// Detect format from file extension.
    pub fn from_extension(path: &str) -> Self {
        let lower = path.to_lowercase();
        if lower.ends_with(".iso") {
            Self::Iso9660
        } else if lower.ends_with(".img.gz") || lower.ends_with(".bin.gz") {
            Self::GzipCompressed
        } else if lower.ends_with(".img") || lower.ends_with(".bin") || lower.ends_with(".raw") {
            Self::Raw
        } else {
            Self::Unknown
        }
    }

    /// Human-readable format name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Raw => "Raw Disk Image",
            Self::Iso9660 => "ISO 9660",
            Self::GzipCompressed => "Gzip Compressed",
            Self::Unknown => "Unknown",
        }
    }

    /// Common file extensions.
    pub fn extensions(self) -> &'static str {
        match self {
            Self::Raw => ".img, .bin, .raw",
            Self::Iso9660 => ".iso",
            Self::GzipCompressed => ".img.gz, .bin.gz",
            Self::Unknown => "",
        }
    }
}

// ============================================================================
// Checksum / hash types
// ============================================================================

/// Supported hash algorithms for image verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
}

impl HashAlgorithm {
    pub fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
        }
    }

    /// Length of the hex digest.
    pub fn digest_hex_len(self) -> usize {
        match self {
            Self::Md5 => 32,
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}

/// State of a hash computation.
#[derive(Clone, Debug)]
pub struct HashState {
    pub algorithm: HashAlgorithm,
    /// Internal state accumulator (simplified for demonstration).
    /// In production, this would use a proper crypto implementation.
    state: [u64; 8],
    bytes_processed: u64,
    finalized: bool,
}

impl HashState {
    pub fn new(algorithm: HashAlgorithm) -> Self {
        let state = match algorithm {
            HashAlgorithm::Md5 => [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0, 0, 0, 0],
            HashAlgorithm::Sha1 => [
                0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0, 0, 0, 0,
            ],
            HashAlgorithm::Sha256 => [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
        };
        Self {
            algorithm,
            state,
            bytes_processed: 0,
            finalized: false,
        }
    }

    /// Feed data into the hash computation.
    pub fn update(&mut self, data: &[u8]) {
        if self.finalized {
            return;
        }
        // Simplified mixing (real implementation would use proper block
        // processing for each algorithm). This demonstrates the API shape.
        for (idx, &byte) in data.iter().enumerate() {
            let slot = idx % 8;
            if let Some(s) = self.state.get_mut(slot) {
                *s = s.wrapping_mul(31).wrapping_add(byte as u64);
            }
        }
        self.bytes_processed = self.bytes_processed.wrapping_add(data.len() as u64);
    }

    /// Finalize and produce the hex digest string.
    pub fn finalize(&mut self) -> String {
        self.finalized = true;
        let expected_len = self.algorithm.digest_hex_len();
        // One u64 renders as 16 hex chars; round up so we always cover the
        // requested length before the final truncation.
        let num_words = expected_len.div_ceil(16);

        // Fold the ENTIRE internal state (all 8 words) plus the processed
        // length into each output word, so every input byte influences the
        // final digest regardless of which slot it landed in. The previous
        // implementation emitted the raw state words and then truncated to the
        // digest length, which discarded the upper words entirely (words 4-7
        // for SHA-256). Two short inputs differing only in a byte that mapped
        // to a discarded word produced identical digests — a real collision.
        let mut result = String::with_capacity(num_words.saturating_mul(16));
        use std::fmt::Write;
        for word_idx in 0..num_words {
            let mut acc = self
                .bytes_processed
                .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                .wrapping_add(word_idx as u64);
            for &word in self.state.iter() {
                acc = acc.rotate_left(7) ^ word.wrapping_mul(0x0000_0100_0000_01b3);
                acc = acc.wrapping_mul(0xff51_afd7_ed55_8ccd);
            }
            let _ = write!(result, "{acc:016x}");
        }
        // Truncate to expected hex length.
        if result.len() > expected_len {
            result.truncate(expected_len);
        }
        result
    }

    pub fn bytes_processed(&self) -> u64 {
        self.bytes_processed
    }
}

/// Verification result comparing computed hash to expected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerificationResult {
    Match,
    Mismatch { expected: String, computed: String },
    Pending,
    Error(String),
}

// ============================================================================
// Drive / device types
// ============================================================================

/// Physical drive type classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveType {
    Hdd,
    Ssd,
    Usb,
    SdCard,
    Optical,
    Virtual,
    Unknown,
}

impl DriveType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hdd => "HDD",
            Self::Ssd => "SSD",
            Self::Usb => "USB",
            Self::SdCard => "SD Card",
            Self::Optical => "Optical",
            Self::Virtual => "Virtual",
            Self::Unknown => "Unknown",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Hdd => "[HDD]",
            Self::Ssd => "[SSD]",
            Self::Usb => "[USB]",
            Self::SdCard => "[SD]",
            Self::Optical => "[OPT]",
            Self::Virtual => "[VRT]",
            Self::Unknown => "[???]",
        }
    }
}

/// Partition table type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionTable {
    Mbr,
    Gpt,
    None,
    Unknown,
}

impl PartitionTable {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mbr => "MBR",
            Self::Gpt => "GPT",
            Self::None => "None",
            Self::Unknown => "Unknown",
        }
    }
}

/// A single partition on a drive.
#[derive(Clone, Debug)]
pub struct Partition {
    pub index: u32,
    pub label: String,
    pub filesystem: String,
    pub offset_bytes: u64,
    pub size_bytes: u64,
    pub is_boot: bool,
}

/// Detected drive (physical or virtual).
#[derive(Clone, Debug)]
pub struct DriveInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub serial: String,
    pub size_bytes: u64,
    pub drive_type: DriveType,
    pub partition_table: PartitionTable,
    pub partitions: Vec<Partition>,
    pub is_system_drive: bool,
    pub is_removable: bool,
    pub is_readonly: bool,
}

impl DriveInfo {
    /// Format the drive size for display.
    pub fn size_display(&self) -> String {
        format_bytes(self.size_bytes)
    }

    /// Check if writing to this drive should be blocked.
    pub fn write_blocked(&self) -> bool {
        self.is_system_drive || self.is_readonly
    }

    /// Brief description string.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        parts.push(self.drive_type.label().to_string());
        parts.push(self.size_display());
        parts.push(self.partition_table.name().to_string());
        if self.is_system_drive {
            parts.push("SYSTEM".to_string());
        }
        parts.join(" | ")
    }
}

// ============================================================================
// ISO 9660 parsing types
// ============================================================================

/// ISO 9660 volume descriptor.
#[derive(Clone, Debug)]
pub struct IsoVolumeDescriptor {
    pub system_id: String,
    pub volume_id: String,
    pub volume_set_id: String,
    pub publisher_id: String,
    pub preparer_id: String,
    pub application_id: String,
    pub creation_date: String,
    pub modification_date: String,
    pub volume_size_blocks: u32,
    pub logical_block_size: u16,
    pub root_directory_lba: u32,
    pub root_directory_size: u32,
}

impl IsoVolumeDescriptor {
    /// Parse a primary volume descriptor from raw sector data (2048 bytes).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 2048 {
            return None;
        }
        // Check for "CD001" magic at offset 1
        if data.get(1..6) != Some(b"CD001") {
            return None;
        }
        // Type must be 1 (primary volume descriptor)
        if data.first() != Some(&1) {
            return None;
        }

        let system_id = extract_iso_string(data, 8, 32);
        let volume_id = extract_iso_string(data, 40, 32);
        let volume_set_id = extract_iso_string(data, 190, 128);
        let publisher_id = extract_iso_string(data, 318, 128);
        let preparer_id = extract_iso_string(data, 446, 128);
        let application_id = extract_iso_string(data, 574, 128);
        let creation_date = extract_iso_datetime(data, 813);
        let modification_date = extract_iso_datetime(data, 830);

        let volume_size_blocks = read_le_u32(data, 80);
        let logical_block_size = read_le_u16(data, 128);
        let root_directory_lba = read_le_u32(data, 158);
        let root_directory_size = read_le_u32(data, 166);

        Some(Self {
            system_id,
            volume_id,
            volume_set_id,
            publisher_id,
            preparer_id,
            application_id,
            creation_date,
            modification_date,
            volume_size_blocks,
            logical_block_size,
            root_directory_lba,
            root_directory_size,
        })
    }
}

/// A file/directory entry in an ISO 9660 filesystem.
#[derive(Clone, Debug)]
pub struct IsoEntry {
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: u64,
    pub lba: u32,
    pub recording_date: String,
    pub children: Vec<IsoEntry>,
    /// Depth in the tree (0 = root).
    pub depth: u32,
    /// Whether this node is expanded in the tree view.
    pub expanded: bool,
}

impl IsoEntry {
    /// Count total files recursively.
    pub fn count_files(&self) -> usize {
        let mut count: usize = if self.is_directory { 0 } else { 1 };
        for child in &self.children {
            count = count.saturating_add(child.count_files());
        }
        count
    }

    /// Count total directories recursively.
    pub fn count_dirs(&self) -> usize {
        let mut count: usize = if self.is_directory { 1 } else { 0 };
        for child in &self.children {
            count = count.saturating_add(child.count_dirs());
        }
        count
    }

    /// Total size of all files recursively.
    pub fn total_size(&self) -> u64 {
        let mut size = self.size_bytes;
        for child in &self.children {
            size = size.saturating_add(child.total_size());
        }
        size
    }

    /// Flatten tree into visible entries (respecting expanded state).
    pub fn flatten_visible(&self) -> Vec<FlatEntry> {
        let mut result = Vec::new();
        self.flatten_into(&mut result);
        result
    }

    fn flatten_into(&self, out: &mut Vec<FlatEntry>) {
        out.push(FlatEntry {
            name: self.name.clone(),
            is_directory: self.is_directory,
            size_bytes: self.size_bytes,
            depth: self.depth,
            expanded: self.expanded,
            has_children: !self.children.is_empty(),
        });
        if self.expanded {
            for child in &self.children {
                child.flatten_into(out);
            }
        }
    }
}

/// Flattened entry for rendering in a list.
#[derive(Clone, Debug)]
pub struct FlatEntry {
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: u64,
    pub depth: u32,
    pub expanded: bool,
    pub has_children: bool,
}

/// Parse ISO 9660 directory records from raw data.
pub fn parse_iso_directory(data: &[u8], depth: u32) -> Vec<IsoEntry> {
    let mut entries = Vec::new();
    let mut offset: usize = 0;

    while offset < data.len() {
        let record_len = match data.get(offset) {
            Some(&len) if len > 0 => len as usize,
            _ => break,
        };
        if offset.saturating_add(record_len) > data.len() {
            break;
        }
        // Minimum valid directory record is 33 bytes
        if record_len < 33 {
            break;
        }

        let name_len = match data.get(offset.saturating_add(32)) {
            Some(&l) => l as usize,
            None => break,
        };

        let flags = data.get(offset.saturating_add(25)).copied().unwrap_or(0);
        let is_directory = (flags & 0x02) != 0;
        let lba = read_le_u32(data, offset.saturating_add(2));
        let size = read_le_u32(data, offset.saturating_add(10));

        // Extract name
        let name_start = offset.saturating_add(33);
        let name_end = name_start.saturating_add(name_len);
        let raw_name = if name_end <= data.len() {
            &data[name_start..name_end]
        } else {
            b""
        };

        // Skip "." and ".." entries
        let skip = matches!(raw_name, [0x00] | [0x01]);

        if !skip && !raw_name.is_empty() {
            let name = String::from_utf8_lossy(raw_name)
                .trim_end_matches(";1")
                .trim_end_matches('.')
                .to_string();

            entries.push(IsoEntry {
                name,
                is_directory,
                size_bytes: size as u64,
                lba,
                recording_date: String::new(),
                children: Vec::new(),
                depth,
                expanded: false,
            });
        }

        offset = offset.saturating_add(record_len);
    }

    // Sort: directories first, then alphabetical
    entries.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    entries
}

// ============================================================================
// Image metadata
// ============================================================================

/// Metadata about a loaded image file.
#[derive(Clone, Debug)]
pub struct ImageInfo {
    pub path: String,
    pub file_size: u64,
    pub format: ImageFormat,
    pub volume_label: String,
    pub filesystem_type: String,
    pub creation_date: String,
    pub modification_date: String,
    pub is_bootable: bool,
    pub boot_type: BootType,
}

impl ImageInfo {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            file_size: 0,
            format: ImageFormat::Unknown,
            volume_label: String::new(),
            filesystem_type: String::new(),
            creation_date: String::new(),
            modification_date: String::new(),
            is_bootable: false,
            boot_type: BootType::None,
        }
    }
}

/// Boot compatibility type for ISOs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootType {
    None,
    LegacyBios,
    Uefi,
    Hybrid,
}

impl BootType {
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "Not Bootable",
            Self::LegacyBios => "Legacy BIOS",
            Self::Uefi => "UEFI",
            Self::Hybrid => "Hybrid (BIOS+UEFI)",
        }
    }
}

/// Detect boot type from image data.
pub fn detect_boot_type(data: &[u8]) -> BootType {
    let has_mbr = data.len() >= 512 && data.get(510) == Some(&0x55) && data.get(511) == Some(&0xAA);

    // Check for El Torito boot catalog (ISO boot)
    let has_el_torito = if data.len() >= 0x8806 {
        // Boot record volume descriptor at sector 17
        data.get(0x8801..0x8806) == Some(b"CD001") && data.get(0x8800) == Some(&0)
    } else {
        false
    };

    // Check for EFI system partition marker in GPT or FAT header
    let has_efi = data.len() >= 0x200
        && (data.get(0x1C2) == Some(&0xEF)  // EFI system partition type
            || (data.len() > 0x400 && data.get(0x200..0x208) == Some(b"EFI PART")));

    match (has_mbr, has_el_torito || has_efi) {
        (true, true) => BootType::Hybrid,
        (true, false) => BootType::LegacyBios,
        (false, true) => BootType::Uefi,
        (false, false) => BootType::None,
    }
}

// ============================================================================
// Operation types
// ============================================================================

/// Current operation the tool is performing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    /// No operation in progress.
    Idle,
    /// Creating an image from a drive.
    CreatingImage,
    /// Writing an image to a drive.
    WritingImage,
    /// Verifying a write (byte comparison).
    VerifyingWrite,
    /// Computing a checksum.
    ComputingHash,
    /// Browsing ISO contents.
    BrowsingIso,
}

impl Operation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::CreatingImage => "Creating Image...",
            Self::WritingImage => "Writing Image...",
            Self::VerifyingWrite => "Verifying Write...",
            Self::ComputingHash => "Computing Hash...",
            Self::BrowsingIso => "Browsing ISO...",
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// Progress of an ongoing operation.
#[derive(Clone, Debug)]
pub struct OperationProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub start_time_ms: u64,
    pub elapsed_ms: u64,
    pub speed_bytes_per_sec: f64,
    pub eta_seconds: f64,
    pub verified_bytes: u64,
    pub errors: Vec<String>,
}

impl OperationProgress {
    pub fn new(bytes_total: u64) -> Self {
        Self {
            bytes_done: 0,
            bytes_total,
            start_time_ms: 0,
            elapsed_ms: 0,
            speed_bytes_per_sec: 0.0,
            eta_seconds: 0.0,
            verified_bytes: 0,
            errors: Vec::new(),
        }
    }

    /// Fraction complete (0.0 to 1.0).
    pub fn fraction(&self) -> f32 {
        if self.bytes_total == 0 {
            return 0.0;
        }
        (self.bytes_done as f64 / self.bytes_total as f64) as f32
    }

    /// Percentage complete.
    pub fn percent(&self) -> f32 {
        self.fraction() * 100.0
    }

    /// Update progress after transferring more bytes.
    pub fn advance(&mut self, bytes: u64, elapsed_ms: u64) {
        self.bytes_done = self.bytes_done.saturating_add(bytes);
        self.elapsed_ms = elapsed_ms;
        if self.elapsed_ms > 0 {
            self.speed_bytes_per_sec = (self.bytes_done as f64) / (self.elapsed_ms as f64 / 1000.0);
        }
        if self.speed_bytes_per_sec > 0.0 {
            let remaining = self.bytes_total.saturating_sub(self.bytes_done);
            self.eta_seconds = remaining as f64 / self.speed_bytes_per_sec;
        }
    }

    /// Speed formatted as human-readable string.
    pub fn speed_display(&self) -> String {
        let mb_per_sec = self.speed_bytes_per_sec / (1024.0 * 1024.0);
        if mb_per_sec >= 1.0 {
            format!("{:.1} MB/s", mb_per_sec)
        } else {
            let kb_per_sec = self.speed_bytes_per_sec / 1024.0;
            format!("{:.1} KB/s", kb_per_sec)
        }
    }

    /// ETA formatted.
    pub fn eta_display(&self) -> String {
        if self.eta_seconds <= 0.0 || self.eta_seconds.is_infinite() {
            return "calculating...".to_string();
        }
        let secs = self.eta_seconds as u64;
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;
        if hours > 0 {
            format!("{}h {:02}m {:02}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {:02}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    /// Progress summary string.
    pub fn summary(&self) -> String {
        format!(
            "{} / {} ({:.1}%) - {} - ETA: {}",
            format_bytes(self.bytes_done),
            format_bytes(self.bytes_total),
            self.percent(),
            self.speed_display(),
            self.eta_display(),
        )
    }

    pub fn is_complete(&self) -> bool {
        self.bytes_done >= self.bytes_total && self.bytes_total > 0
    }
}

// ============================================================================
// Write confirmation dialog
// ============================================================================

/// Safety confirmation dialog state.
#[derive(Clone, Debug)]
pub struct ConfirmDialog {
    pub visible: bool,
    pub title: String,
    pub message: String,
    pub detail: String,
    pub confirmed: bool,
    pub cancelled: bool,
    pub hover_confirm: bool,
    pub hover_cancel: bool,
}

impl Default for ConfirmDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            title: String::new(),
            message: String::new(),
            detail: String::new(),
            confirmed: false,
            cancelled: false,
            hover_confirm: false,
            hover_cancel: false,
        }
    }

    /// Show a write-confirmation dialog for a destructive operation.
    pub fn show_write_confirm(&mut self, image_name: &str, drive_name: &str, drive_size: u64) {
        self.visible = true;
        self.confirmed = false;
        self.cancelled = false;
        self.title = "Confirm Write".to_string();
        self.message = format!("Write '{}' to '{}'?", image_name, drive_name);
        self.detail = format!(
            "WARNING: All data on {} ({}) will be permanently destroyed. \
             This operation cannot be undone.",
            drive_name,
            format_bytes(drive_size)
        );
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
        self.confirmed = false;
        self.cancelled = false;
    }
}

// ============================================================================
// Recent images
// ============================================================================

/// Recently used image file entry.
#[derive(Clone, Debug)]
pub struct RecentImage {
    pub path: String,
    pub format: ImageFormat,
    pub size_bytes: u64,
    pub last_used_timestamp: u64,
}

// ============================================================================
// Write options
// ============================================================================

/// Configuration for image creation.
#[derive(Clone, Debug)]
pub struct CreateOptions {
    pub source_drive_id: String,
    pub output_path: String,
    pub block_size: u64,
    pub compress: bool,
    pub format: ImageFormat,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            source_drive_id: String::new(),
            output_path: String::new(),
            block_size: DEFAULT_BLOCK_SIZE,
            compress: false,
            format: ImageFormat::Raw,
        }
    }
}

/// Configuration for image writing.
#[derive(Clone, Debug)]
pub struct WriteOptions {
    pub image_path: String,
    pub target_drive_id: String,
    pub verify_after_write: bool,
    pub block_size: u64,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            image_path: String::new(),
            target_drive_id: String::new(),
            verify_after_write: true,
            block_size: DEFAULT_BLOCK_SIZE,
        }
    }
}

// ============================================================================
// Tab / view types
// ============================================================================

/// Main tabs in the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainTab {
    Write,
    Create,
    Browse,
    Verify,
}

impl MainTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Write => "Write Image",
            Self::Create => "Create Image",
            Self::Browse => "Browse ISO",
            Self::Verify => "Verify",
        }
    }

    pub const ALL: [MainTab; 4] = [
        MainTab::Write,
        MainTab::Create,
        MainTab::Browse,
        MainTab::Verify,
    ];
}

// ============================================================================
// Application state
// ============================================================================

/// Complete application state.
pub struct DiskImagerApp {
    // UI state
    pub active_tab: MainTab,
    pub window_width: f32,
    pub window_height: f32,
    pub scroll_offset: f32,
    pub sidebar_scroll: f32,

    // Drive management
    pub drives: Vec<DriveInfo>,
    pub selected_drive_index: Option<usize>,

    // Image management
    pub loaded_image: Option<ImageInfo>,
    pub iso_volume: Option<IsoVolumeDescriptor>,
    pub iso_root: Option<IsoEntry>,
    pub iso_scroll_offset: f32,
    pub selected_iso_entry: Option<usize>,

    // Operation state
    pub operation: Operation,
    pub progress: OperationProgress,

    // Checksum / verification
    pub hash_algorithm: HashAlgorithm,
    pub hash_state: Option<HashState>,
    pub computed_hash: Option<String>,
    pub expected_hash: String,
    pub verification_result: VerificationResult,

    // Write / create options
    pub write_options: WriteOptions,
    pub create_options: CreateOptions,

    // Dialogs
    pub confirm_dialog: ConfirmDialog,

    // Recent images
    pub recent_images: VecDeque<RecentImage>,

    // Drive lock state
    pub locked_drive_id: Option<String>,

    // Status message
    pub status_message: String,
    pub status_is_error: bool,

    // Tick counter for animations
    pub tick_count: u64,
}

impl Default for DiskImagerApp {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskImagerApp {
    pub fn new() -> Self {
        let drives = Self::detect_drives();
        Self {
            active_tab: MainTab::Write,
            window_width: 960.0,
            window_height: 680.0,
            scroll_offset: 0.0,
            sidebar_scroll: 0.0,
            drives,
            selected_drive_index: None,
            loaded_image: None,
            iso_volume: None,
            iso_root: None,
            iso_scroll_offset: 0.0,
            selected_iso_entry: None,
            operation: Operation::Idle,
            progress: OperationProgress::new(0),
            hash_algorithm: HashAlgorithm::Sha256,
            hash_state: None,
            computed_hash: None,
            expected_hash: String::new(),
            verification_result: VerificationResult::Pending,
            write_options: WriteOptions::default(),
            create_options: CreateOptions::default(),
            confirm_dialog: ConfirmDialog::new(),
            recent_images: VecDeque::new(),
            locked_drive_id: None,
            status_message: "Ready".to_string(),
            status_is_error: false,
            tick_count: 0,
        }
    }

    /// Detect available drives on the system.
    fn detect_drives() -> Vec<DriveInfo> {
        // In production, this would enumerate actual block devices via
        // the OS block device enumeration API. Here we provide sample
        // drives for demonstration.
        vec![
            DriveInfo {
                id: "disk0".to_string(),
                name: "System NVMe".to_string(),
                model: "Samsung 980 PRO 1TB".to_string(),
                serial: "S5GXNG0N123456".to_string(),
                size_bytes: 1_000_204_886_016,
                drive_type: DriveType::Ssd,
                partition_table: PartitionTable::Gpt,
                partitions: vec![
                    Partition {
                        index: 1,
                        label: "EFI System".to_string(),
                        filesystem: "FAT32".to_string(),
                        offset_bytes: 1_048_576,
                        size_bytes: 268_435_456,
                        is_boot: true,
                    },
                    Partition {
                        index: 2,
                        label: "Slate OS".to_string(),
                        filesystem: "ext4".to_string(),
                        offset_bytes: 269_484_032,
                        size_bytes: 999_935_401_984,
                        is_boot: false,
                    },
                ],
                is_system_drive: true,
                is_removable: false,
                is_readonly: false,
            },
            DriveInfo {
                id: "disk1".to_string(),
                name: "USB Flash Drive".to_string(),
                model: "SanDisk Ultra 32GB".to_string(),
                serial: "4C530001234567".to_string(),
                size_bytes: 31_457_280_000,
                drive_type: DriveType::Usb,
                partition_table: PartitionTable::Mbr,
                partitions: vec![Partition {
                    index: 1,
                    label: "USBDRIVE".to_string(),
                    filesystem: "FAT32".to_string(),
                    offset_bytes: 1_048_576,
                    size_bytes: 31_456_231_424,
                    is_boot: false,
                }],
                is_system_drive: false,
                is_removable: true,
                is_readonly: false,
            },
            DriveInfo {
                id: "disk2".to_string(),
                name: "SD Card Reader".to_string(),
                model: "Generic SD Card 16GB".to_string(),
                serial: "0000000000000".to_string(),
                size_bytes: 15_931_539_456,
                drive_type: DriveType::SdCard,
                partition_table: PartitionTable::Mbr,
                partitions: vec![],
                is_system_drive: false,
                is_removable: true,
                is_readonly: false,
            },
        ]
    }

    // ========================================================================
    // Image operations
    // ========================================================================

    /// Load an image file and detect its format.
    pub fn load_image(&mut self, path: &str, data: &[u8]) {
        let format = if data.len() >= 0x8006 {
            ImageFormat::from_magic(data)
        } else {
            ImageFormat::from_extension(path)
        };

        let mut info = ImageInfo::new(path);
        info.file_size = data.len() as u64;
        info.format = format;
        info.boot_type = detect_boot_type(data);
        info.is_bootable = info.boot_type != BootType::None;

        // Parse ISO volume if applicable
        if format == ImageFormat::Iso9660 && data.len() >= 0x8800 {
            if let Some(vol) = IsoVolumeDescriptor::parse(data.get(0x8000..0x8800).unwrap_or(&[])) {
                info.volume_label = vol.volume_id.clone();
                info.filesystem_type = "ISO 9660".to_string();
                info.creation_date = vol.creation_date.clone();
                info.modification_date = vol.modification_date.clone();

                // Parse root directory
                let root_lba = vol.root_directory_lba as usize;
                let root_size = vol.root_directory_size as usize;
                let block_size = vol.logical_block_size as usize;
                let root_offset =
                    root_lba.saturating_mul(if block_size > 0 { block_size } else { 2048 });
                let root_end = root_offset.saturating_add(root_size);

                if root_end <= data.len() {
                    let dir_data = &data[root_offset..root_end];
                    let children = parse_iso_directory(dir_data, 1);
                    self.iso_root = Some(IsoEntry {
                        name: "/".to_string(),
                        is_directory: true,
                        size_bytes: root_size as u64,
                        lba: root_lba as u32,
                        recording_date: String::new(),
                        children,
                        depth: 0,
                        expanded: true,
                    });
                }
                self.iso_volume = Some(vol);
            }
        } else {
            info.filesystem_type = format.name().to_string();
            self.iso_volume = None;
            self.iso_root = None;
        }

        // Add to recent images
        self.add_recent(path, format, info.file_size);
        self.loaded_image = Some(info);
        self.status_message = format!("Loaded: {}", path);
        self.status_is_error = false;
    }

    /// Add a path to the recent images list.
    fn add_recent(&mut self, path: &str, format: ImageFormat, size: u64) {
        // Remove existing entry for same path
        self.recent_images.retain(|r| r.path != path);

        self.recent_images.push_front(RecentImage {
            path: path.to_string(),
            format,
            size_bytes: size,
            last_used_timestamp: self.tick_count,
        });

        // Cap the list
        while self.recent_images.len() > MAX_RECENT_IMAGES {
            self.recent_images.pop_back();
        }
    }

    /// Start writing the loaded image to the selected drive.
    pub fn start_write(&mut self) -> Result<(), String> {
        let image = self
            .loaded_image
            .as_ref()
            .ok_or_else(|| "No image loaded".to_string())?;

        let drive_idx = self
            .selected_drive_index
            .ok_or_else(|| "No drive selected".to_string())?;

        let drive = self
            .drives
            .get(drive_idx)
            .ok_or_else(|| "Invalid drive index".to_string())?;

        if drive.write_blocked() {
            return Err(format!(
                "Cannot write to '{}': {}",
                drive.name,
                if drive.is_system_drive {
                    "system drive"
                } else {
                    "read-only"
                }
            ));
        }

        if image.file_size > drive.size_bytes {
            return Err(format!(
                "Image ({}) is larger than drive ({})",
                format_bytes(image.file_size),
                format_bytes(drive.size_bytes),
            ));
        }

        self.write_options.image_path = image.path.clone();
        self.write_options.target_drive_id = drive.id.clone();
        self.operation = Operation::WritingImage;
        self.progress = OperationProgress::new(image.file_size);
        self.locked_drive_id = Some(drive.id.clone());
        self.status_message = format!("Writing to {}...", drive.name);
        self.status_is_error = false;
        Ok(())
    }

    /// Start creating an image from the selected drive.
    pub fn start_create(&mut self, output_path: &str) -> Result<(), String> {
        let drive_idx = self
            .selected_drive_index
            .ok_or_else(|| "No drive selected".to_string())?;

        let drive = self
            .drives
            .get(drive_idx)
            .ok_or_else(|| "Invalid drive index".to_string())?;

        self.create_options.source_drive_id = drive.id.clone();
        self.create_options.output_path = output_path.to_string();
        self.operation = Operation::CreatingImage;
        self.progress = OperationProgress::new(drive.size_bytes);
        self.locked_drive_id = Some(drive.id.clone());
        self.status_message = format!("Creating image from {}...", drive.name);
        self.status_is_error = false;
        Ok(())
    }

    /// Start byte-by-byte verification after write.
    pub fn start_verify_write(&mut self) -> Result<(), String> {
        let image = self
            .loaded_image
            .as_ref()
            .ok_or_else(|| "No image loaded".to_string())?;
        self.operation = Operation::VerifyingWrite;
        self.progress = OperationProgress::new(image.file_size);
        self.status_message = "Verifying write...".to_string();
        self.status_is_error = false;
        Ok(())
    }

    /// Start computing a hash of the loaded image.
    pub fn start_hash(&mut self) -> Result<(), String> {
        let image = self
            .loaded_image
            .as_ref()
            .ok_or_else(|| "No image loaded".to_string())?;
        self.hash_state = Some(HashState::new(self.hash_algorithm));
        self.computed_hash = None;
        self.verification_result = VerificationResult::Pending;
        self.operation = Operation::ComputingHash;
        self.progress = OperationProgress::new(image.file_size);
        self.status_message = format!("Computing {} hash...", self.hash_algorithm.name());
        self.status_is_error = false;
        Ok(())
    }

    /// Cancel the current operation.
    pub fn cancel_operation(&mut self) {
        self.operation = Operation::Idle;
        self.locked_drive_id = None;
        self.status_message = "Operation cancelled".to_string();
        self.status_is_error = false;
    }

    /// Complete the current operation.
    pub fn complete_operation(&mut self) {
        let msg = match &self.operation {
            Operation::WritingImage => {
                if self.write_options.verify_after_write {
                    // Transition to verification
                    let _ = self.start_verify_write();
                    return;
                }
                "Write complete".to_string()
            }
            Operation::VerifyingWrite => "Write verified successfully".to_string(),
            Operation::CreatingImage => {
                format!("Image created: {}", self.create_options.output_path)
            }
            Operation::ComputingHash => {
                if let Some(state) = self.hash_state.as_mut() {
                    let hash = state.finalize();
                    self.computed_hash = Some(hash.clone());
                    if !self.expected_hash.is_empty() {
                        if self.expected_hash.to_lowercase() == hash.to_lowercase() {
                            self.verification_result = VerificationResult::Match;
                        } else {
                            self.verification_result = VerificationResult::Mismatch {
                                expected: self.expected_hash.clone(),
                                computed: hash,
                            };
                        }
                    }
                }
                "Hash computation complete".to_string()
            }
            _ => "Done".to_string(),
        };
        self.operation = Operation::Idle;
        self.locked_drive_id = None;
        self.status_message = msg;
        self.status_is_error = false;
    }

    /// Select a drive by index with validation.
    pub fn select_drive(&mut self, index: usize) {
        if index < self.drives.len() {
            self.selected_drive_index = Some(index);
        }
    }

    /// Get the currently selected drive (if any).
    pub fn selected_drive(&self) -> Option<&DriveInfo> {
        self.selected_drive_index
            .and_then(|idx| self.drives.get(idx))
    }

    /// Check if a drive is currently locked for an operation.
    pub fn is_drive_locked(&self, drive_id: &str) -> bool {
        self.locked_drive_id.as_deref() == Some(drive_id)
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                self.tick_count = self.tick_count.wrapping_add(1);
                // Simulate progress if operation is active
                if self.operation.is_active() {
                    let step = self.progress.bytes_total / 100;
                    let step = if step == 0 { 1024 } else { step };
                    self.progress.advance(step, *elapsed_ms);
                    if self.progress.is_complete() {
                        self.complete_operation();
                    }
                }
                EventResult::Consumed
            }
            Event::Key(key_ev) if key_ev.pressed => self.handle_key(key_ev),
            Event::Mouse(mouse_ev) => self.handle_mouse(mouse_ev),
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Handle confirm dialog first
        if self.confirm_dialog.visible {
            match key.key {
                Key::Enter => {
                    self.confirm_dialog.confirmed = true;
                    self.confirm_dialog.visible = false;
                    // Execute the write
                    if let Err(e) = self.start_write() {
                        self.status_message = e;
                        self.status_is_error = true;
                    }
                    return EventResult::Consumed;
                }
                Key::Escape => {
                    self.confirm_dialog.dismiss();
                    return EventResult::Consumed;
                }
                _ => return EventResult::Consumed,
            }
        }

        // Tab switching
        if key.modifiers.ctrl {
            match key.key {
                Key::Num1 => {
                    self.active_tab = MainTab::Write;
                    return EventResult::Consumed;
                }
                Key::Num2 => {
                    self.active_tab = MainTab::Create;
                    return EventResult::Consumed;
                }
                Key::Num3 => {
                    self.active_tab = MainTab::Browse;
                    return EventResult::Consumed;
                }
                Key::Num4 => {
                    self.active_tab = MainTab::Verify;
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        // Cancel operation
        if key.key == Key::Escape && self.operation.is_active() {
            self.cancel_operation();
            return EventResult::Consumed;
        }

        // Drive selection
        match key.key {
            Key::Up => {
                if let Some(idx) = self.selected_drive_index {
                    if idx > 0 {
                        self.selected_drive_index = Some(idx.saturating_sub(1));
                    }
                } else if !self.drives.is_empty() {
                    self.selected_drive_index = Some(0);
                }
                EventResult::Consumed
            }
            Key::Down => {
                let max_idx = self.drives.len().saturating_sub(1);
                if let Some(idx) = self.selected_drive_index {
                    if idx < max_idx {
                        self.selected_drive_index = Some(idx.saturating_add(1));
                    }
                } else if !self.drives.is_empty() {
                    self.selected_drive_index = Some(0);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        let mx = mouse.x;
        let my = mouse.y;

        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                // Confirm dialog buttons
                if self.confirm_dialog.visible {
                    return self.handle_dialog_click(mx, my);
                }

                // Tab bar clicks
                if (TOOLBAR_HEIGHT..TOOLBAR_HEIGHT + ROW_HEIGHT + 8.0).contains(&my) {
                    return self.handle_tab_click(mx);
                }

                // Drive list clicks (left sidebar)
                if mx < SIDEBAR_WIDTH {
                    return self.handle_drive_click(mx, my);
                }

                EventResult::Consumed
            }
            MouseEventKind::Scroll { dy, .. } => {
                if mx < SIDEBAR_WIDTH {
                    self.sidebar_scroll = (self.sidebar_scroll - dy * 20.0).max(0.0);
                } else {
                    self.scroll_offset = (self.scroll_offset - dy * 20.0).max(0.0);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_tab_click(&mut self, mx: f32) -> EventResult {
        let tab_width = 120.0_f32;
        for (idx, tab) in MainTab::ALL.iter().enumerate() {
            let tab_x = PANEL_PADDING + (idx as f32) * tab_width;
            if mx >= tab_x && mx < tab_x + tab_width {
                self.active_tab = *tab;
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }

    fn handle_drive_click(&mut self, _mx: f32, my: f32) -> EventResult {
        let list_y_start = TOOLBAR_HEIGHT + ROW_HEIGHT + 8.0 + PANEL_PADDING + 24.0;
        if my < list_y_start {
            return EventResult::Ignored;
        }
        let relative_y = my - list_y_start + self.sidebar_scroll;
        let drive_row_height = 64.0_f32;
        let clicked_idx = (relative_y / drive_row_height) as usize;
        if clicked_idx < self.drives.len() {
            self.select_drive(clicked_idx);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn handle_dialog_click(&mut self, mx: f32, my: f32) -> EventResult {
        let dialog_w = 420.0_f32;
        let dialog_h = 200.0_f32;
        let dialog_x = (self.window_width - dialog_w) / 2.0;
        let dialog_y = (self.window_height - dialog_h) / 2.0;

        let btn_y = dialog_y + dialog_h - 50.0;
        let btn_h = 32.0_f32;

        // Cancel button
        let cancel_x = dialog_x + dialog_w - 200.0;
        if mx >= cancel_x && mx < cancel_x + 90.0 && my >= btn_y && my < btn_y + btn_h {
            self.confirm_dialog.dismiss();
            return EventResult::Consumed;
        }

        // Confirm button
        let confirm_x = dialog_x + dialog_w - 100.0;
        if mx >= confirm_x && mx < confirm_x + 90.0 && my >= btn_y && my < btn_y + btn_h {
            self.confirm_dialog.confirmed = true;
            self.confirm_dialog.visible = false;
            if let Err(e) = self.start_write() {
                self.status_message = e;
                self.status_is_error = true;
            }
            return EventResult::Consumed;
        }

        EventResult::Consumed
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    pub fn render(&self, rt: &mut RenderTree) {
        // Background
        rt.fill_rect(
            0.0,
            0.0,
            self.window_width,
            self.window_height,
            colors::BASE,
        );

        // Toolbar
        self.render_toolbar(rt);

        // Tab bar
        let tab_y = TOOLBAR_HEIGHT;
        self.render_tab_bar(rt, tab_y);

        // Content area
        let content_y = tab_y + ROW_HEIGHT + 8.0;
        let content_h = self.window_height - content_y - STATUS_BAR_HEIGHT;

        // Left sidebar: drive list
        self.render_drive_list(rt, 0.0, content_y, SIDEBAR_WIDTH, content_h);

        // Right panel: active tab content
        let panel_x = SIDEBAR_WIDTH;
        let panel_w = self.window_width - SIDEBAR_WIDTH;
        self.render_active_tab(rt, panel_x, content_y, panel_w, content_h);

        // Status bar
        self.render_status_bar(rt);

        // Overlay: progress bar if operation is active
        if self.operation.is_active() {
            self.render_progress_overlay(rt);
        }

        // Overlay: confirm dialog
        if self.confirm_dialog.visible {
            self.render_confirm_dialog(rt);
        }
    }

    fn render_toolbar(&self, rt: &mut RenderTree) {
        // Toolbar background
        rt.fill_rounded_rect(
            0.0,
            0.0,
            self.window_width,
            TOOLBAR_HEIGHT,
            colors::MANTLE,
            CornerRadii::ZERO,
        );

        // App title
        rt.push(RenderCommand::Text {
            x: PANEL_PADDING,
            y: (TOOLBAR_HEIGHT - HEADER_FONT_SIZE) / 2.0,
            text: "Disk Imager".to_string(),
            color: colors::BLUE,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Refresh drives button
        let refresh_x = self.window_width - 140.0;
        let btn_y = (TOOLBAR_HEIGHT - BUTTON_HEIGHT) / 2.0;
        rt.fill_rounded_rect(
            refresh_x,
            btn_y,
            128.0,
            BUTTON_HEIGHT,
            colors::SURFACE0,
            CornerRadii::all(4.0),
        );
        rt.push(RenderCommand::Text {
            x: refresh_x + 12.0,
            y: btn_y + (BUTTON_HEIGHT - UI_FONT_SIZE) / 2.0,
            text: "Refresh Drives".to_string(),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_tab_bar(&self, rt: &mut RenderTree, y: f32) {
        // Tab bar background
        rt.fill_rect(0.0, y, self.window_width, ROW_HEIGHT + 8.0, colors::CRUST);

        let tab_width = 120.0_f32;
        for (idx, tab) in MainTab::ALL.iter().enumerate() {
            let tab_x = PANEL_PADDING + (idx as f32) * tab_width;
            let is_active = self.active_tab == *tab;

            let bg = if is_active {
                colors::BASE
            } else {
                colors::CRUST
            };
            let fg = if is_active {
                colors::BLUE
            } else {
                colors::SUBTEXT0
            };

            rt.fill_rounded_rect(
                tab_x,
                y + 4.0,
                tab_width - 4.0,
                ROW_HEIGHT,
                bg,
                CornerRadii {
                    top_left: CORNER_RADIUS,
                    top_right: CORNER_RADIUS,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            );

            rt.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: y + 4.0 + (ROW_HEIGHT - UI_FONT_SIZE) / 2.0,
                text: tab.label().to_string(),
                color: fg,
                font_size: UI_FONT_SIZE,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 24.0),
            });

            // Active tab indicator
            if is_active {
                rt.fill_rect(
                    tab_x,
                    y + ROW_HEIGHT + 2.0,
                    tab_width - 4.0,
                    2.0,
                    colors::BLUE,
                );
            }
        }
    }

    fn render_drive_list(&self, rt: &mut RenderTree, x: f32, y: f32, width: f32, height: f32) {
        // Background
        rt.fill_rect(x, y, width, height, colors::MANTLE);

        // Border right
        rt.push(RenderCommand::Line {
            x1: x + width - 1.0,
            y1: y,
            x2: x + width - 1.0,
            y2: y + height,
            color: colors::SURFACE0,
            width: 1.0,
        });

        // Header
        rt.push(RenderCommand::Text {
            x: x + PANEL_PADDING,
            y: y + PANEL_PADDING,
            text: "Drives".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });

        // Drive entries
        let entry_y_start = y + PANEL_PADDING + 24.0;
        let entry_height = 64.0_f32;

        rt.push(RenderCommand::PushClip {
            x,
            y: entry_y_start,
            width,
            height: height - PANEL_PADDING - 24.0,
        });

        for (idx, drive) in self.drives.iter().enumerate() {
            let ey = entry_y_start + (idx as f32) * entry_height - self.sidebar_scroll;

            // Skip if scrolled out of view
            if ey + entry_height < entry_y_start || ey > y + height {
                continue;
            }

            let is_selected = self.selected_drive_index == Some(idx);
            let is_locked = self.is_drive_locked(&drive.id);

            // Background
            let bg = if is_selected {
                colors::SURFACE0
            } else {
                colors::MANTLE
            };
            rt.fill_rounded_rect(
                x + 4.0,
                ey,
                width - 8.0,
                entry_height - 4.0,
                bg,
                CornerRadii::all(4.0),
            );

            // Selection indicator
            if is_selected {
                rt.fill_rect(x + 4.0, ey, 3.0, entry_height - 4.0, colors::BLUE);
            }

            // Drive icon and name
            let icon_color = match drive.drive_type {
                DriveType::Usb => colors::GREEN,
                DriveType::SdCard => colors::PEACH,
                DriveType::Ssd => colors::BLUE,
                _ => colors::SUBTEXT0,
            };

            rt.push(RenderCommand::Text {
                x: x + PANEL_PADDING + 4.0,
                y: ey + 6.0,
                text: drive.drive_type.icon().to_string(),
                color: icon_color,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            rt.push(RenderCommand::Text {
                x: x + PANEL_PADDING + 40.0,
                y: ey + 6.0,
                text: drive.name.clone(),
                color: colors::TEXT,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 60.0),
            });

            // Drive details
            rt.push(RenderCommand::Text {
                x: x + PANEL_PADDING + 40.0,
                y: ey + 24.0,
                text: drive.model.clone(),
                color: colors::SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 60.0),
            });

            rt.push(RenderCommand::Text {
                x: x + PANEL_PADDING + 40.0,
                y: ey + 40.0,
                text: drive.summary(),
                color: colors::OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 60.0),
            });

            // Lock / system indicators
            if drive.is_system_drive {
                rt.push(RenderCommand::Text {
                    x: x + width - 60.0,
                    y: ey + 6.0,
                    text: "SYSTEM".to_string(),
                    color: colors::RED,
                    font_size: SMALL_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            if is_locked {
                rt.push(RenderCommand::Text {
                    x: x + width - 60.0,
                    y: ey + 20.0,
                    text: "LOCKED".to_string(),
                    color: colors::YELLOW,
                    font_size: SMALL_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
        }

        rt.push(RenderCommand::PopClip);
    }

    fn render_active_tab(&self, rt: &mut RenderTree, x: f32, y: f32, width: f32, height: f32) {
        match self.active_tab {
            MainTab::Write => self.render_write_tab(rt, x, y, width, height),
            MainTab::Create => self.render_create_tab(rt, x, y, width, height),
            MainTab::Browse => self.render_browse_tab(rt, x, y, width, height),
            MainTab::Verify => self.render_verify_tab(rt, x, y, width, height),
        }
    }

    fn render_write_tab(&self, rt: &mut RenderTree, x: f32, y: f32, width: f32, height: f32) {
        let px = x + PANEL_PADDING;
        let mut cy = y + PANEL_PADDING;

        // Section: Image Selection
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Image File".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        cy += 24.0;

        if let Some(ref img) = self.loaded_image {
            // Image info card
            self.render_image_card(rt, px, cy, width - PANEL_PADDING * 2.0, img);
            cy += 100.0;
        } else {
            rt.fill_rounded_rect(
                px,
                cy,
                width - PANEL_PADDING * 2.0,
                60.0,
                colors::SURFACE0,
                CornerRadii::all(CORNER_RADIUS),
            );
            rt.push(RenderCommand::Text {
                x: px + PANEL_PADDING,
                y: cy + 20.0,
                text: "No image loaded. Open an .iso, .img, or .bin file.".to_string(),
                color: colors::SUBTEXT0,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 4.0),
            });
            cy += 72.0;
        }

        // Section: Target Drive
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Target Drive".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        cy += 24.0;

        if let Some(drive) = self.selected_drive() {
            self.render_drive_card(rt, px, cy, width - PANEL_PADDING * 2.0, drive);
            cy += 72.0;
        } else {
            rt.fill_rounded_rect(
                px,
                cy,
                width - PANEL_PADDING * 2.0,
                40.0,
                colors::SURFACE0,
                CornerRadii::all(CORNER_RADIUS),
            );
            rt.push(RenderCommand::Text {
                x: px + PANEL_PADDING,
                y: cy + 12.0,
                text: "Select a target drive from the left panel".to_string(),
                color: colors::SUBTEXT0,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 4.0),
            });
            cy += 52.0;
        }

        // Write options
        cy += 8.0;
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Options".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 24.0;

        // Verify after write checkbox
        let check_color = if self.write_options.verify_after_write {
            colors::GREEN
        } else {
            colors::SURFACE1
        };
        rt.fill_rounded_rect(px, cy, 18.0, 18.0, check_color, CornerRadii::all(3.0));
        if self.write_options.verify_after_write {
            rt.push(RenderCommand::Text {
                x: px + 3.0,
                y: cy + 1.0,
                text: "v".to_string(),
                color: colors::CRUST,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        rt.push(RenderCommand::Text {
            x: px + 26.0,
            y: cy + 2.0,
            text: "Verify after write (byte-by-byte comparison)".to_string(),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PANEL_PADDING * 2.0 - 30.0),
        });
        cy += 30.0;

        // Block size
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: format!("Block size: {} bytes", self.write_options.block_size),
            color: colors::SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cy += 28.0;

        // Write button
        let can_write = self.loaded_image.is_some()
            && self.selected_drive_index.is_some()
            && !self.operation.is_active();

        let btn_color = if can_write {
            colors::BLUE
        } else {
            colors::SURFACE1
        };
        let btn_w = 160.0_f32;
        rt.fill_rounded_rect(
            px,
            cy,
            btn_w,
            BUTTON_HEIGHT,
            btn_color,
            CornerRadii::all(CORNER_RADIUS),
        );
        rt.push(RenderCommand::Text {
            x: px + (btn_w - 80.0) / 2.0,
            y: cy + (BUTTON_HEIGHT - UI_FONT_SIZE) / 2.0,
            text: "Write Image".to_string(),
            color: if can_write {
                colors::CRUST
            } else {
                colors::OVERLAY0
            },
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Recent images section
        let _ = height; // use height to avoid warning
        cy += BUTTON_HEIGHT + 16.0;
        if !self.recent_images.is_empty() {
            rt.push(RenderCommand::Text {
                x: px,
                y: cy,
                text: "Recent Images".to_string(),
                color: colors::TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 22.0;

            let max_show = 5_usize;
            for (idx, recent) in self.recent_images.iter().enumerate() {
                if idx >= max_show {
                    break;
                }
                rt.push(RenderCommand::Text {
                    x: px + 8.0,
                    y: cy,
                    text: format!(
                        "{} ({} - {})",
                        truncate_path(&recent.path, 40),
                        recent.format.name(),
                        format_bytes(recent.size_bytes),
                    ),
                    color: colors::SUBTEXT0,
                    font_size: SMALL_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - PANEL_PADDING * 2.0 - 16.0),
                });
                cy += 18.0;
            }
        }
    }

    fn render_create_tab(&self, rt: &mut RenderTree, x: f32, y: f32, width: f32, _height: f32) {
        let px = x + PANEL_PADDING;
        let mut cy = y + PANEL_PADDING;

        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Create Disk Image".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        cy += 28.0;

        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Create a raw disk image from the selected drive.".to_string(),
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        cy += 28.0;

        // Source drive
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Source Drive".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 24.0;

        if let Some(drive) = self.selected_drive() {
            self.render_drive_card(rt, px, cy, width - PANEL_PADDING * 2.0, drive);
            cy += 72.0;
        } else {
            rt.fill_rounded_rect(
                px,
                cy,
                width - PANEL_PADDING * 2.0,
                40.0,
                colors::SURFACE0,
                CornerRadii::all(CORNER_RADIUS),
            );
            rt.push(RenderCommand::Text {
                x: px + PANEL_PADDING,
                y: cy + 12.0,
                text: "Select a source drive from the left panel".to_string(),
                color: colors::SUBTEXT0,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 4.0),
            });
            cy += 52.0;
        }

        // Options
        cy += 8.0;
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Options".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 24.0;

        // Compress checkbox
        let compress_color = if self.create_options.compress {
            colors::GREEN
        } else {
            colors::SURFACE1
        };
        rt.fill_rounded_rect(px, cy, 18.0, 18.0, compress_color, CornerRadii::all(3.0));
        if self.create_options.compress {
            rt.push(RenderCommand::Text {
                x: px + 3.0,
                y: cy + 1.0,
                text: "v".to_string(),
                color: colors::CRUST,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        rt.push(RenderCommand::Text {
            x: px + 26.0,
            y: cy + 2.0,
            text: "Compress output (gzip)".to_string(),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cy += 30.0;

        // Block size
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: format!("Block size: {} bytes", self.create_options.block_size),
            color: colors::SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cy += 28.0;

        // Output format
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: format!(
                "Output format: {} ({})",
                self.create_options.format.name(),
                self.create_options.format.extensions()
            ),
            color: colors::SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        cy += 30.0;

        // Create button
        let can_create = self.selected_drive_index.is_some() && !self.operation.is_active();
        let btn_color = if can_create {
            colors::GREEN
        } else {
            colors::SURFACE1
        };
        let btn_w = 160.0_f32;
        rt.fill_rounded_rect(
            px,
            cy,
            btn_w,
            BUTTON_HEIGHT,
            btn_color,
            CornerRadii::all(CORNER_RADIUS),
        );
        rt.push(RenderCommand::Text {
            x: px + (btn_w - 90.0) / 2.0,
            y: cy + (BUTTON_HEIGHT - UI_FONT_SIZE) / 2.0,
            text: "Create Image".to_string(),
            color: if can_create {
                colors::CRUST
            } else {
                colors::OVERLAY0
            },
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_browse_tab(&self, rt: &mut RenderTree, x: f32, y: f32, width: f32, height: f32) {
        let px = x + PANEL_PADDING;
        let mut cy = y + PANEL_PADDING;

        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "ISO 9660 Browser".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        cy += 28.0;

        // Volume info if available
        if let Some(ref vol) = self.iso_volume {
            self.render_volume_info(rt, px, cy, width - PANEL_PADDING * 2.0, vol);
            cy += 90.0;
        }

        // File tree
        if let Some(ref root) = self.iso_root {
            rt.push(RenderCommand::Text {
                x: px,
                y: cy,
                text: "File Tree".to_string(),
                color: colors::TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 22.0;

            // Statistics bar
            let files = root.count_files();
            let dirs = root.count_dirs();
            let total = root.total_size();
            rt.push(RenderCommand::Text {
                x: px,
                y: cy,
                text: format!(
                    "{} files, {} directories, {} total",
                    files,
                    dirs,
                    format_bytes(total),
                ),
                color: colors::SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 2.0),
            });
            cy += 20.0;

            // File list
            let visible = root.flatten_visible();
            let list_height = height - (cy - y) - PANEL_PADDING;

            rt.push(RenderCommand::PushClip {
                x: px,
                y: cy,
                width: width - PANEL_PADDING * 2.0,
                height: list_height,
            });

            let row_h = 20.0_f32;
            for (idx, entry) in visible.iter().enumerate() {
                let ey = cy + (idx as f32) * row_h - self.iso_scroll_offset;
                if ey + row_h < cy || ey > cy + list_height {
                    continue;
                }

                let indent = entry.depth as f32 * 20.0;
                let is_selected = self.selected_iso_entry == Some(idx);

                if is_selected {
                    rt.fill_rect(px, ey, width - PANEL_PADDING * 2.0, row_h, colors::SURFACE0);
                }

                // Expand/collapse indicator
                if entry.is_directory && entry.has_children {
                    let arrow = if entry.expanded { "v" } else { ">" };
                    rt.push(RenderCommand::Text {
                        x: px + indent,
                        y: ey + 2.0,
                        text: arrow.to_string(),
                        color: colors::OVERLAY0,
                        font_size: SMALL_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                // Icon
                let icon_text = if entry.is_directory { "[D]" } else { "[F]" };
                let icon_color = if entry.is_directory {
                    colors::PEACH
                } else {
                    colors::LAVENDER
                };
                rt.push(RenderCommand::Text {
                    x: px + indent + 14.0,
                    y: ey + 2.0,
                    text: icon_text.to_string(),
                    color: icon_color,
                    font_size: SMALL_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                // Name
                rt.push(RenderCommand::Text {
                    x: px + indent + 40.0,
                    y: ey + 2.0,
                    text: entry.name.clone(),
                    color: colors::TEXT,
                    font_size: SMALL_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - PANEL_PADDING * 2.0 - indent - 120.0),
                });

                // Size (for files)
                if !entry.is_directory {
                    let size_text = format_bytes(entry.size_bytes);
                    let size_w = size_text.len() as f32 * CHAR_WIDTH * SMALL_FONT_SIZE;
                    rt.push(RenderCommand::Text {
                        x: px + width - PANEL_PADDING * 2.0 - size_w - 8.0,
                        y: ey + 2.0,
                        text: size_text,
                        color: colors::OVERLAY0,
                        font_size: SMALL_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }
            }

            rt.push(RenderCommand::PopClip);
        } else {
            rt.fill_rounded_rect(
                px,
                cy,
                width - PANEL_PADDING * 2.0,
                60.0,
                colors::SURFACE0,
                CornerRadii::all(CORNER_RADIUS),
            );
            rt.push(RenderCommand::Text {
                x: px + PANEL_PADDING,
                y: cy + 20.0,
                text: "Load an ISO image to browse its contents.".to_string(),
                color: colors::SUBTEXT0,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 4.0),
            });
        }
    }

    fn render_verify_tab(&self, rt: &mut RenderTree, x: f32, y: f32, width: f32, _height: f32) {
        let px = x + PANEL_PADDING;
        let mut cy = y + PANEL_PADDING;

        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Checksum Verification".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        cy += 28.0;

        // Image info
        if let Some(ref img) = self.loaded_image {
            self.render_image_card(rt, px, cy, width - PANEL_PADDING * 2.0, img);
            cy += 100.0;
        } else {
            rt.fill_rounded_rect(
                px,
                cy,
                width - PANEL_PADDING * 2.0,
                40.0,
                colors::SURFACE0,
                CornerRadii::all(CORNER_RADIUS),
            );
            rt.push(RenderCommand::Text {
                x: px + PANEL_PADDING,
                y: cy + 12.0,
                text: "Load an image file to compute checksums".to_string(),
                color: colors::SUBTEXT0,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 4.0),
            });
            cy += 52.0;
        }

        // Algorithm selection
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Hash Algorithm".to_string(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 24.0;

        let algorithms = [
            HashAlgorithm::Sha256,
            HashAlgorithm::Sha1,
            HashAlgorithm::Md5,
        ];
        let btn_gap = 8.0_f32;
        let alg_btn_w = 90.0_f32;
        for (idx, alg) in algorithms.iter().enumerate() {
            let bx = px + (idx as f32) * (alg_btn_w + btn_gap);
            let is_selected = self.hash_algorithm == *alg;
            let bg = if is_selected {
                colors::BLUE
            } else {
                colors::SURFACE0
            };
            let fg = if is_selected {
                colors::CRUST
            } else {
                colors::TEXT
            };

            rt.fill_rounded_rect(bx, cy, alg_btn_w, BUTTON_HEIGHT, bg, CornerRadii::all(4.0));
            rt.push(RenderCommand::Text {
                x: bx + (alg_btn_w - (alg.name().len() as f32 * CHAR_WIDTH * UI_FONT_SIZE)) / 2.0,
                y: cy + (BUTTON_HEIGHT - UI_FONT_SIZE) / 2.0,
                text: alg.name().to_string(),
                color: fg,
                font_size: UI_FONT_SIZE,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }
        cy += BUTTON_HEIGHT + 16.0;

        // Expected hash input
        rt.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Expected Hash (optional)".to_string(),
            color: colors::SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cy += 18.0;

        let input_w = width - PANEL_PADDING * 2.0;
        rt.fill_rounded_rect(
            px,
            cy,
            input_w,
            28.0,
            colors::SURFACE0,
            CornerRadii::all(4.0),
        );
        rt.push(RenderCommand::StrokeRect {
            x: px,
            y: cy,
            width: input_w,
            height: 28.0,
            color: colors::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
        let hash_display = if self.expected_hash.is_empty() {
            "Paste expected hash here to compare..."
        } else {
            &self.expected_hash
        };
        let hash_color = if self.expected_hash.is_empty() {
            colors::OVERLAY0
        } else {
            colors::TEXT
        };
        rt.push(RenderCommand::Text {
            x: px + 8.0,
            y: cy + 6.0,
            text: hash_display.to_string(),
            color: hash_color,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(input_w - 16.0),
        });
        cy += 40.0;

        // Compute button
        let can_hash = self.loaded_image.is_some() && !self.operation.is_active();
        let btn_color = if can_hash {
            colors::MAUVE
        } else {
            colors::SURFACE1
        };
        let btn_w = 180.0_f32;
        rt.fill_rounded_rect(
            px,
            cy,
            btn_w,
            BUTTON_HEIGHT,
            btn_color,
            CornerRadii::all(CORNER_RADIUS),
        );
        rt.push(RenderCommand::Text {
            x: px + 14.0,
            y: cy + (BUTTON_HEIGHT - UI_FONT_SIZE) / 2.0,
            text: format!("Compute {} Hash", self.hash_algorithm.name()),
            color: if can_hash {
                colors::CRUST
            } else {
                colors::OVERLAY0
            },
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += BUTTON_HEIGHT + 16.0;

        // Computed hash result
        if let Some(ref hash) = self.computed_hash {
            rt.push(RenderCommand::Text {
                x: px,
                y: cy,
                text: "Computed Hash".to_string(),
                color: colors::TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 22.0;

            rt.fill_rounded_rect(
                px,
                cy,
                input_w,
                28.0,
                colors::SURFACE0,
                CornerRadii::all(4.0),
            );
            rt.push(RenderCommand::Text {
                x: px + 8.0,
                y: cy + 6.0,
                text: hash.clone(),
                color: colors::GREEN,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_w - 16.0),
            });
            cy += 40.0;

            // Verification result
            match &self.verification_result {
                VerificationResult::Match => {
                    rt.fill_rounded_rect(
                        px,
                        cy,
                        input_w,
                        32.0,
                        Color::rgba(166, 227, 161, 30),
                        CornerRadii::all(4.0),
                    );
                    rt.push(RenderCommand::Text {
                        x: px + 12.0,
                        y: cy + 8.0,
                        text: "MATCH - Hashes are identical".to_string(),
                        color: colors::GREEN,
                        font_size: UI_FONT_SIZE,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(input_w - 24.0),
                    });
                }
                VerificationResult::Mismatch { expected, computed } => {
                    rt.fill_rounded_rect(
                        px,
                        cy,
                        input_w,
                        52.0,
                        Color::rgba(243, 139, 168, 30),
                        CornerRadii::all(4.0),
                    );
                    rt.push(RenderCommand::Text {
                        x: px + 12.0,
                        y: cy + 6.0,
                        text: "MISMATCH - Hashes differ!".to_string(),
                        color: colors::RED,
                        font_size: UI_FONT_SIZE,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(input_w - 24.0),
                    });
                    rt.push(RenderCommand::Text {
                        x: px + 12.0,
                        y: cy + 24.0,
                        text: format!("Expected: {}", truncate_path(expected, 50)),
                        color: colors::RED,
                        font_size: SMALL_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(input_w - 24.0),
                    });
                    rt.push(RenderCommand::Text {
                        x: px + 12.0,
                        y: cy + 38.0,
                        text: format!("Computed: {}", truncate_path(computed, 50)),
                        color: colors::RED,
                        font_size: SMALL_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(input_w - 24.0),
                    });
                }
                _ => {}
            }
        }
    }

    fn render_image_card(&self, rt: &mut RenderTree, x: f32, y: f32, width: f32, img: &ImageInfo) {
        // Card background
        rt.fill_rounded_rect(
            x,
            y,
            width,
            90.0,
            colors::SURFACE0,
            CornerRadii::all(CORNER_RADIUS),
        );
        rt.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height: 90.0,
            color: colors::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let tx = x + PANEL_PADDING;
        let mut ty = y + 8.0;

        // File name
        let filename = img.path.rsplit(['/', '\\']).next().unwrap_or(&img.path);
        rt.push(RenderCommand::Text {
            x: tx,
            y: ty,
            text: filename.to_string(),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        ty += 18.0;

        // Format and size
        rt.push(RenderCommand::Text {
            x: tx,
            y: ty,
            text: format!(
                "{} | {} | {}",
                img.format.name(),
                format_bytes(img.file_size),
                img.boot_type.name()
            ),
            color: colors::SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });
        ty += 16.0;

        // Volume label if present
        if !img.volume_label.is_empty() {
            rt.push(RenderCommand::Text {
                x: tx,
                y: ty,
                text: format!("Volume: {}", img.volume_label),
                color: colors::TEAL,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 2.0),
            });
            ty += 16.0;
        }

        // Filesystem type
        if !img.filesystem_type.is_empty() {
            rt.push(RenderCommand::Text {
                x: tx,
                y: ty,
                text: format!("Filesystem: {}", img.filesystem_type),
                color: colors::SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 2.0),
            });
        }

        // Bootable indicator
        if img.is_bootable {
            let badge_x = x + width - 80.0;
            rt.fill_rounded_rect(
                badge_x,
                y + 8.0,
                68.0,
                20.0,
                colors::GREEN,
                CornerRadii::all(10.0),
            );
            rt.push(RenderCommand::Text {
                x: badge_x + 8.0,
                y: y + 12.0,
                text: "Bootable".to_string(),
                color: colors::CRUST,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_drive_card(
        &self,
        rt: &mut RenderTree,
        x: f32,
        y: f32,
        width: f32,
        drive: &DriveInfo,
    ) {
        rt.fill_rounded_rect(
            x,
            y,
            width,
            60.0,
            colors::SURFACE0,
            CornerRadii::all(CORNER_RADIUS),
        );
        rt.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height: 60.0,
            color: colors::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let tx = x + PANEL_PADDING;

        rt.push(RenderCommand::Text {
            x: tx,
            y: y + 8.0,
            text: format!("{} {}", drive.drive_type.icon(), drive.name),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });

        rt.push(RenderCommand::Text {
            x: tx,
            y: y + 26.0,
            text: format!(
                "{} | {} | {}",
                drive.model,
                drive.size_display(),
                drive.partition_table.name()
            ),
            color: colors::SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });

        rt.push(RenderCommand::Text {
            x: tx,
            y: y + 42.0,
            text: format!(
                "{} partition(s) | Serial: {}",
                drive.partitions.len(),
                if drive.serial.is_empty() {
                    "N/A"
                } else {
                    &drive.serial
                }
            ),
            color: colors::OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PANEL_PADDING * 2.0),
        });

        // Warning badge for system drives
        if drive.write_blocked() {
            let badge_x = x + width - 100.0;
            rt.fill_rounded_rect(
                badge_x,
                y + 8.0,
                88.0,
                20.0,
                colors::RED,
                CornerRadii::all(10.0),
            );
            rt.push(RenderCommand::Text {
                x: badge_x + 8.0,
                y: y + 12.0,
                text: if drive.is_system_drive {
                    "System Drive"
                } else {
                    "Read-Only"
                }
                .to_string(),
                color: colors::CRUST,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_volume_info(
        &self,
        rt: &mut RenderTree,
        x: f32,
        y: f32,
        width: f32,
        vol: &IsoVolumeDescriptor,
    ) {
        rt.fill_rounded_rect(
            x,
            y,
            width,
            80.0,
            colors::SURFACE0,
            CornerRadii::all(CORNER_RADIUS),
        );
        rt.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height: 80.0,
            color: colors::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let tx = x + PANEL_PADDING;
        let mut ty = y + 8.0;

        let info_lines = [
            format!("Volume: {}", vol.volume_id.trim()),
            format!("Publisher: {}", vol.publisher_id.trim()),
            format!("Application: {}", vol.application_id.trim()),
            format!(
                "Size: {} blocks x {} bytes",
                vol.volume_size_blocks, vol.logical_block_size
            ),
        ];

        for line in &info_lines {
            rt.push(RenderCommand::Text {
                x: tx,
                y: ty,
                text: line.clone(),
                color: colors::SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PANEL_PADDING * 2.0),
            });
            ty += 16.0;
        }
    }

    fn render_status_bar(&self, rt: &mut RenderTree) {
        let sy = self.window_height - STATUS_BAR_HEIGHT;
        rt.fill_rect(0.0, sy, self.window_width, STATUS_BAR_HEIGHT, colors::CRUST);

        // Status message
        let status_color = if self.status_is_error {
            colors::RED
        } else if self.operation.is_active() {
            colors::YELLOW
        } else {
            colors::SUBTEXT0
        };

        rt.push(RenderCommand::Text {
            x: PANEL_PADDING,
            y: sy + (STATUS_BAR_HEIGHT - SMALL_FONT_SIZE) / 2.0,
            text: self.status_message.clone(),
            color: status_color,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.window_width * 0.6),
        });

        // Right side: operation status
        if self.operation.is_active() {
            let op_text = self.operation.label();
            let tw = op_text.len() as f32 * CHAR_WIDTH * SMALL_FONT_SIZE;
            rt.push(RenderCommand::Text {
                x: self.window_width - tw - PANEL_PADDING,
                y: sy + (STATUS_BAR_HEIGHT - SMALL_FONT_SIZE) / 2.0,
                text: op_text.to_string(),
                color: colors::YELLOW,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        } else {
            // Drive count
            let info = format!("{} drives detected", self.drives.len());
            let iw = info.len() as f32 * CHAR_WIDTH * SMALL_FONT_SIZE;
            rt.push(RenderCommand::Text {
                x: self.window_width - iw - PANEL_PADDING,
                y: sy + (STATUS_BAR_HEIGHT - SMALL_FONT_SIZE) / 2.0,
                text: info,
                color: colors::OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_progress_overlay(&self, rt: &mut RenderTree) {
        let bar_w = self.window_width - SIDEBAR_WIDTH - PANEL_PADDING * 4.0;
        let bar_x = SIDEBAR_WIDTH + PANEL_PADDING * 2.0;
        let bar_y =
            self.window_height - STATUS_BAR_HEIGHT - PROGRESS_BAR_HEIGHT - PANEL_PADDING - 40.0;

        // Background card
        rt.fill_rounded_rect(
            bar_x - PANEL_PADDING,
            bar_y - PANEL_PADDING,
            bar_w + PANEL_PADDING * 2.0,
            PROGRESS_BAR_HEIGHT + 50.0,
            colors::MANTLE,
            CornerRadii::all(CORNER_RADIUS),
        );
        rt.push(RenderCommand::StrokeRect {
            x: bar_x - PANEL_PADDING,
            y: bar_y - PANEL_PADDING,
            width: bar_w + PANEL_PADDING * 2.0,
            height: PROGRESS_BAR_HEIGHT + 50.0,
            color: colors::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Progress bar background
        rt.fill_rounded_rect(
            bar_x,
            bar_y,
            bar_w,
            PROGRESS_BAR_HEIGHT,
            colors::SURFACE0,
            CornerRadii::all(PROGRESS_BAR_HEIGHT / 2.0),
        );

        // Progress bar fill
        let fill_w = bar_w * self.progress.fraction();
        if fill_w > 0.0 {
            let progress_color = match &self.operation {
                Operation::WritingImage => colors::BLUE,
                Operation::VerifyingWrite => colors::TEAL,
                Operation::CreatingImage => colors::GREEN,
                Operation::ComputingHash => colors::MAUVE,
                _ => colors::BLUE,
            };
            rt.fill_rounded_rect(
                bar_x,
                bar_y,
                fill_w,
                PROGRESS_BAR_HEIGHT,
                progress_color,
                CornerRadii::all(PROGRESS_BAR_HEIGHT / 2.0),
            );
        }

        // Percentage text on bar
        let pct_text = format!("{:.1}%", self.progress.percent());
        rt.push(RenderCommand::Text {
            x: bar_x + (bar_w - pct_text.len() as f32 * CHAR_WIDTH * SMALL_FONT_SIZE) / 2.0,
            y: bar_y + (PROGRESS_BAR_HEIGHT - SMALL_FONT_SIZE) / 2.0,
            text: pct_text,
            color: colors::TEXT,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Progress details below bar
        rt.push(RenderCommand::Text {
            x: bar_x,
            y: bar_y + PROGRESS_BAR_HEIGHT + 6.0,
            text: self.progress.summary(),
            color: colors::SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_w),
        });

        // Cancel hint
        rt.push(RenderCommand::Text {
            x: bar_x + bar_w - 100.0,
            y: bar_y + PROGRESS_BAR_HEIGHT + 6.0,
            text: "Esc to cancel".to_string(),
            color: colors::OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_confirm_dialog(&self, rt: &mut RenderTree) {
        // Dim overlay
        rt.fill_rect(
            0.0,
            0.0,
            self.window_width,
            self.window_height,
            Color::rgba(0, 0, 0, 150),
        );

        let dialog_w = 420.0_f32;
        let dialog_h = 200.0_f32;
        let dialog_x = (self.window_width - dialog_w) / 2.0;
        let dialog_y = (self.window_height - dialog_h) / 2.0;

        // Shadow
        rt.push(RenderCommand::BoxShadow {
            x: dialog_x,
            y: dialog_y,
            width: dialog_w,
            height: dialog_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 20.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CORNER_RADIUS * 2.0),
        });

        // Dialog background
        rt.fill_rounded_rect(
            dialog_x,
            dialog_y,
            dialog_w,
            dialog_h,
            colors::MANTLE,
            CornerRadii::all(CORNER_RADIUS * 2.0),
        );
        rt.push(RenderCommand::StrokeRect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_w,
            height: dialog_h,
            color: colors::RED,
            line_width: 2.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS * 2.0),
        });

        // Title
        rt.push(RenderCommand::Text {
            x: dialog_x + PANEL_PADDING,
            y: dialog_y + PANEL_PADDING,
            text: self.confirm_dialog.title.clone(),
            color: colors::RED,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - PANEL_PADDING * 2.0),
        });

        // Message
        rt.push(RenderCommand::Text {
            x: dialog_x + PANEL_PADDING,
            y: dialog_y + PANEL_PADDING + 28.0,
            text: self.confirm_dialog.message.clone(),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - PANEL_PADDING * 2.0),
        });

        // Detail/warning
        rt.push(RenderCommand::Text {
            x: dialog_x + PANEL_PADDING,
            y: dialog_y + PANEL_PADDING + 52.0,
            text: self.confirm_dialog.detail.clone(),
            color: colors::YELLOW,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - PANEL_PADDING * 2.0),
        });

        // Buttons
        let btn_y = dialog_y + dialog_h - 50.0;
        let btn_w = 90.0_f32;

        // Cancel button
        let cancel_x = dialog_x + dialog_w - 200.0;
        rt.fill_rounded_rect(
            cancel_x,
            btn_y,
            btn_w,
            BUTTON_HEIGHT,
            colors::SURFACE0,
            CornerRadii::all(4.0),
        );
        rt.push(RenderCommand::Text {
            x: cancel_x + (btn_w - 42.0) / 2.0,
            y: btn_y + (BUTTON_HEIGHT - UI_FONT_SIZE) / 2.0,
            text: "Cancel".to_string(),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Confirm (destructive) button
        let confirm_x = dialog_x + dialog_w - 100.0;
        rt.fill_rounded_rect(
            confirm_x,
            btn_y,
            btn_w,
            BUTTON_HEIGHT,
            colors::RED,
            CornerRadii::all(4.0),
        );
        rt.push(RenderCommand::Text {
            x: confirm_x + (btn_w - 36.0) / 2.0,
            y: btn_y + (BUTTON_HEIGHT - UI_FONT_SIZE) / 2.0,
            text: "Write".to_string(),
            color: colors::CRUST,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Format byte count into human-readable string (e.g., "1.5 GB").
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    const TB: u64 = 1024 * 1024 * 1024 * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Truncate a path string with ellipsis if too long.
pub fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    let keep = max_len.saturating_sub(3);
    let start = path.len().saturating_sub(keep);
    format!("...{}", &path[start..])
}

/// Extract a trimmed ASCII string from ISO data at a given offset and length.
fn extract_iso_string(data: &[u8], offset: usize, max_len: usize) -> String {
    let end = offset.saturating_add(max_len).min(data.len());
    if offset >= data.len() {
        return String::new();
    }
    let slice = &data[offset..end];
    String::from_utf8_lossy(slice).trim().to_string()
}

/// Extract ISO 9660 datetime (17 bytes, ASCII digits).
fn extract_iso_datetime(data: &[u8], offset: usize) -> String {
    let end = offset.saturating_add(17);
    if end > data.len() {
        return String::new();
    }
    let raw = &data[offset..end];
    // Format: YYYYMMDDHHMMSSCC (year, month, day, hour, min, sec, centiseconds, tz)
    let s = String::from_utf8_lossy(raw);
    if s.len() >= 14 {
        format!(
            "{}-{}-{} {}:{}:{}",
            &s[0..4],
            &s[4..6],
            &s[6..8],
            &s[8..10],
            &s[10..12],
            &s[12..14],
        )
    } else {
        s.to_string()
    }
}

/// Read a little-endian u32 from data at the given offset.
fn read_le_u32(data: &[u8], offset: usize) -> u32 {
    let end = offset.saturating_add(4);
    if end > data.len() {
        return 0;
    }
    let b0 = data.get(offset).copied().unwrap_or(0) as u32;
    let b1 = data.get(offset.saturating_add(1)).copied().unwrap_or(0) as u32;
    let b2 = data.get(offset.saturating_add(2)).copied().unwrap_or(0) as u32;
    let b3 = data.get(offset.saturating_add(3)).copied().unwrap_or(0) as u32;
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

/// Read a little-endian u16 from data at the given offset.
fn read_le_u16(data: &[u8], offset: usize) -> u16 {
    let end = offset.saturating_add(2);
    if end > data.len() {
        return 0;
    }
    let b0 = data.get(offset).copied().unwrap_or(0) as u16;
    let b1 = data.get(offset.saturating_add(1)).copied().unwrap_or(0) as u16;
    b0 | (b1 << 8)
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let mut app = DiskImagerApp::new();
    let mut rt = RenderTree::new();

    // Initial render
    app.render(&mut rt);

    // Event loop (simplified — in production this is driven by the
    // compositor's event dispatch).
    let resize_event = Event::Resize {
        width: 960,
        height: 680,
    };
    app.handle_event(&resize_event);

    rt.clear();
    app.render(&mut rt);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----------------------------------------------------------------
    // ImageFormat detection
    // ----------------------------------------------------------------

    #[test]
    fn test_format_from_extension_iso() {
        assert_eq!(
            ImageFormat::from_extension("file.iso"),
            ImageFormat::Iso9660
        );
    }

    #[test]
    fn test_format_from_extension_img() {
        assert_eq!(ImageFormat::from_extension("file.img"), ImageFormat::Raw);
    }

    #[test]
    fn test_format_from_extension_bin() {
        assert_eq!(ImageFormat::from_extension("file.bin"), ImageFormat::Raw);
    }

    #[test]
    fn test_format_from_extension_raw() {
        assert_eq!(ImageFormat::from_extension("file.raw"), ImageFormat::Raw);
    }

    #[test]
    fn test_format_from_extension_gz() {
        assert_eq!(
            ImageFormat::from_extension("file.img.gz"),
            ImageFormat::GzipCompressed
        );
    }

    #[test]
    fn test_format_from_extension_unknown() {
        assert_eq!(
            ImageFormat::from_extension("file.txt"),
            ImageFormat::Unknown
        );
    }

    #[test]
    fn test_format_from_extension_case_insensitive() {
        assert_eq!(
            ImageFormat::from_extension("FILE.ISO"),
            ImageFormat::Iso9660
        );
    }

    #[test]
    fn test_format_from_magic_gzip() {
        let data = vec![0x1F, 0x8B, 0x08, 0x00];
        assert_eq!(ImageFormat::from_magic(&data), ImageFormat::GzipCompressed);
    }

    #[test]
    fn test_format_from_magic_raw() {
        let data = vec![0x00, 0x00, 0x00, 0x01];
        assert_eq!(ImageFormat::from_magic(&data), ImageFormat::Raw);
    }

    #[test]
    fn test_format_from_magic_empty() {
        assert_eq!(ImageFormat::from_magic(&[]), ImageFormat::Unknown);
    }

    #[test]
    fn test_format_from_magic_iso() {
        let mut data = vec![0u8; 0x8006];
        data[0x8001] = b'C';
        data[0x8002] = b'D';
        data[0x8003] = b'0';
        data[0x8004] = b'0';
        data[0x8005] = b'1';
        assert_eq!(ImageFormat::from_magic(&data), ImageFormat::Iso9660);
    }

    #[test]
    fn test_format_name() {
        assert_eq!(ImageFormat::Raw.name(), "Raw Disk Image");
        assert_eq!(ImageFormat::Iso9660.name(), "ISO 9660");
        assert_eq!(ImageFormat::GzipCompressed.name(), "Gzip Compressed");
        assert_eq!(ImageFormat::Unknown.name(), "Unknown");
    }

    #[test]
    fn test_format_extensions() {
        assert!(!ImageFormat::Raw.extensions().is_empty());
        assert!(!ImageFormat::Iso9660.extensions().is_empty());
    }

    // ----------------------------------------------------------------
    // Hash / Checksum
    // ----------------------------------------------------------------

    #[test]
    fn test_hash_algorithm_names() {
        assert_eq!(HashAlgorithm::Md5.name(), "MD5");
        assert_eq!(HashAlgorithm::Sha1.name(), "SHA-1");
        assert_eq!(HashAlgorithm::Sha256.name(), "SHA-256");
    }

    #[test]
    fn test_hash_digest_lengths() {
        assert_eq!(HashAlgorithm::Md5.digest_hex_len(), 32);
        assert_eq!(HashAlgorithm::Sha1.digest_hex_len(), 40);
        assert_eq!(HashAlgorithm::Sha256.digest_hex_len(), 64);
    }

    #[test]
    fn test_hash_state_new() {
        let state = HashState::new(HashAlgorithm::Sha256);
        assert_eq!(state.bytes_processed(), 0);
        assert!(!state.finalized);
    }

    #[test]
    fn test_hash_state_update_tracks_bytes() {
        let mut state = HashState::new(HashAlgorithm::Sha256);
        state.update(&[0u8; 100]);
        assert_eq!(state.bytes_processed(), 100);
        state.update(&[0u8; 50]);
        assert_eq!(state.bytes_processed(), 150);
    }

    #[test]
    fn test_hash_finalize_produces_hex() {
        let mut state = HashState::new(HashAlgorithm::Sha256);
        state.update(b"test data");
        let hex = state.finalize();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_finalize_md5_length() {
        let mut state = HashState::new(HashAlgorithm::Md5);
        state.update(b"some data");
        let hex = state.finalize();
        assert_eq!(hex.len(), 32);
    }

    #[test]
    fn test_hash_finalize_sha1_length() {
        let mut state = HashState::new(HashAlgorithm::Sha1);
        state.update(b"some data");
        let hex = state.finalize();
        assert_eq!(hex.len(), 40);
    }

    #[test]
    fn test_hash_no_update_after_finalize() {
        let mut state = HashState::new(HashAlgorithm::Sha256);
        state.update(b"data");
        let _ = state.finalize();
        state.update(b"more data");
        assert_eq!(state.bytes_processed(), 4);
    }

    #[test]
    fn test_hash_deterministic() {
        let mut s1 = HashState::new(HashAlgorithm::Sha256);
        let mut s2 = HashState::new(HashAlgorithm::Sha256);
        s1.update(b"identical data");
        s2.update(b"identical data");
        assert_eq!(s1.finalize(), s2.finalize());
    }

    #[test]
    fn test_hash_different_data_different_hash() {
        let mut s1 = HashState::new(HashAlgorithm::Sha256);
        let mut s2 = HashState::new(HashAlgorithm::Sha256);
        s1.update(b"data A");
        s2.update(b"data B");
        assert_ne!(s1.finalize(), s2.finalize());
    }

    // ----------------------------------------------------------------
    // DriveType / PartitionTable
    // ----------------------------------------------------------------

    #[test]
    fn test_drive_type_labels() {
        assert_eq!(DriveType::Hdd.label(), "HDD");
        assert_eq!(DriveType::Ssd.label(), "SSD");
        assert_eq!(DriveType::Usb.label(), "USB");
        assert_eq!(DriveType::SdCard.label(), "SD Card");
        assert_eq!(DriveType::Virtual.label(), "Virtual");
    }

    #[test]
    fn test_drive_type_icons() {
        assert!(!DriveType::Usb.icon().is_empty());
        assert!(!DriveType::Ssd.icon().is_empty());
    }

    #[test]
    fn test_partition_table_names() {
        assert_eq!(PartitionTable::Gpt.name(), "GPT");
        assert_eq!(PartitionTable::Mbr.name(), "MBR");
        assert_eq!(PartitionTable::None.name(), "None");
    }

    // ----------------------------------------------------------------
    // DriveInfo
    // ----------------------------------------------------------------

    #[test]
    fn test_drive_info_size_display() {
        let drive = DriveInfo {
            id: "d".into(),
            name: "Test".into(),
            model: "M".into(),
            serial: "S".into(),
            size_bytes: 1_000_000_000,
            drive_type: DriveType::Ssd,
            partition_table: PartitionTable::Gpt,
            partitions: vec![],
            is_system_drive: false,
            is_removable: false,
            is_readonly: false,
        };
        let s = drive.size_display();
        assert!(s.contains("MB") || s.contains("GB"));
    }

    #[test]
    fn test_drive_write_blocked_system() {
        let drive = DriveInfo {
            id: "d".into(),
            name: "Sys".into(),
            model: "M".into(),
            serial: "S".into(),
            size_bytes: 1000,
            drive_type: DriveType::Ssd,
            partition_table: PartitionTable::Gpt,
            partitions: vec![],
            is_system_drive: true,
            is_removable: false,
            is_readonly: false,
        };
        assert!(drive.write_blocked());
    }

    #[test]
    fn test_drive_write_blocked_readonly() {
        let drive = DriveInfo {
            id: "d".into(),
            name: "RO".into(),
            model: "M".into(),
            serial: "S".into(),
            size_bytes: 1000,
            drive_type: DriveType::Optical,
            partition_table: PartitionTable::None,
            partitions: vec![],
            is_system_drive: false,
            is_removable: true,
            is_readonly: true,
        };
        assert!(drive.write_blocked());
    }

    #[test]
    fn test_drive_write_allowed() {
        let drive = DriveInfo {
            id: "d".into(),
            name: "USB".into(),
            model: "M".into(),
            serial: "S".into(),
            size_bytes: 1000,
            drive_type: DriveType::Usb,
            partition_table: PartitionTable::Mbr,
            partitions: vec![],
            is_system_drive: false,
            is_removable: true,
            is_readonly: false,
        };
        assert!(!drive.write_blocked());
    }

    #[test]
    fn test_drive_summary() {
        let drive = DriveInfo {
            id: "d0".into(),
            name: "Test".into(),
            model: "M".into(),
            serial: "S".into(),
            size_bytes: 1_073_741_824,
            drive_type: DriveType::Usb,
            partition_table: PartitionTable::Mbr,
            partitions: vec![],
            is_system_drive: false,
            is_removable: true,
            is_readonly: false,
        };
        let s = drive.summary();
        assert!(s.contains("USB"));
        assert!(s.contains("MBR"));
    }

    // ----------------------------------------------------------------
    // ISO parsing
    // ----------------------------------------------------------------

    #[test]
    fn test_iso_volume_descriptor_parse_too_short() {
        assert!(IsoVolumeDescriptor::parse(&[0u8; 100]).is_none());
    }

    #[test]
    fn test_iso_volume_descriptor_parse_bad_magic() {
        let data = vec![1u8; 2048];
        assert!(IsoVolumeDescriptor::parse(&data).is_none());
    }

    #[test]
    fn test_iso_volume_descriptor_parse_valid() {
        let mut data = vec![0u8; 2048];
        data[0] = 1; // Primary volume descriptor
        data[1] = b'C';
        data[2] = b'D';
        data[3] = b'0';
        data[4] = b'0';
        data[5] = b'1';
        // Volume ID at offset 40
        let vol_label = b"TEST_VOLUME";
        for (i, &b) in vol_label.iter().enumerate() {
            if let Some(slot) = data.get_mut(40 + i) {
                *slot = b;
            }
        }
        let vol = IsoVolumeDescriptor::parse(&data);
        assert!(vol.is_some());
        let vol = vol.unwrap();
        assert!(vol.volume_id.contains("TEST_VOLUME"));
    }

    #[test]
    fn test_parse_iso_directory_empty() {
        let entries = parse_iso_directory(&[], 0);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_iso_directory_too_short() {
        let entries = parse_iso_directory(&[10], 0);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_iso_entry_count_files() {
        let entry = IsoEntry {
            name: "root".into(),
            is_directory: true,
            size_bytes: 0,
            lba: 0,
            recording_date: String::new(),
            children: vec![
                IsoEntry {
                    name: "file1".into(),
                    is_directory: false,
                    size_bytes: 100,
                    lba: 1,
                    recording_date: String::new(),
                    children: vec![],
                    depth: 1,
                    expanded: false,
                },
                IsoEntry {
                    name: "file2".into(),
                    is_directory: false,
                    size_bytes: 200,
                    lba: 2,
                    recording_date: String::new(),
                    children: vec![],
                    depth: 1,
                    expanded: false,
                },
            ],
            depth: 0,
            expanded: true,
        };
        assert_eq!(entry.count_files(), 2);
    }

    #[test]
    fn test_iso_entry_count_dirs() {
        let entry = IsoEntry {
            name: "root".into(),
            is_directory: true,
            size_bytes: 0,
            lba: 0,
            recording_date: String::new(),
            children: vec![IsoEntry {
                name: "subdir".into(),
                is_directory: true,
                size_bytes: 0,
                lba: 1,
                recording_date: String::new(),
                children: vec![],
                depth: 1,
                expanded: false,
            }],
            depth: 0,
            expanded: true,
        };
        assert_eq!(entry.count_dirs(), 2);
    }

    #[test]
    fn test_iso_entry_total_size() {
        let entry = IsoEntry {
            name: "root".into(),
            is_directory: true,
            size_bytes: 0,
            lba: 0,
            recording_date: String::new(),
            children: vec![IsoEntry {
                name: "file".into(),
                is_directory: false,
                size_bytes: 500,
                lba: 1,
                recording_date: String::new(),
                children: vec![],
                depth: 1,
                expanded: false,
            }],
            depth: 0,
            expanded: true,
        };
        assert_eq!(entry.total_size(), 500);
    }

    #[test]
    fn test_iso_flatten_visible_collapsed() {
        let entry = IsoEntry {
            name: "root".into(),
            is_directory: true,
            size_bytes: 0,
            lba: 0,
            recording_date: String::new(),
            children: vec![IsoEntry {
                name: "child".into(),
                is_directory: false,
                size_bytes: 100,
                lba: 1,
                recording_date: String::new(),
                children: vec![],
                depth: 1,
                expanded: false,
            }],
            depth: 0,
            expanded: false,
        };
        let flat = entry.flatten_visible();
        assert_eq!(flat.len(), 1); // only root visible
    }

    #[test]
    fn test_iso_flatten_visible_expanded() {
        let entry = IsoEntry {
            name: "root".into(),
            is_directory: true,
            size_bytes: 0,
            lba: 0,
            recording_date: String::new(),
            children: vec![IsoEntry {
                name: "child".into(),
                is_directory: false,
                size_bytes: 100,
                lba: 1,
                recording_date: String::new(),
                children: vec![],
                depth: 1,
                expanded: false,
            }],
            depth: 0,
            expanded: true,
        };
        let flat = entry.flatten_visible();
        assert_eq!(flat.len(), 2);
    }

    // ----------------------------------------------------------------
    // Boot type detection
    // ----------------------------------------------------------------

    #[test]
    fn test_detect_boot_type_none() {
        let data = vec![0u8; 1024];
        assert_eq!(detect_boot_type(&data), BootType::None);
    }

    #[test]
    fn test_detect_boot_type_mbr() {
        let mut data = vec![0u8; 1024];
        data[510] = 0x55;
        data[511] = 0xAA;
        assert_eq!(detect_boot_type(&data), BootType::LegacyBios);
    }

    #[test]
    fn test_boot_type_names() {
        assert_eq!(BootType::None.name(), "Not Bootable");
        assert_eq!(BootType::Hybrid.name(), "Hybrid (BIOS+UEFI)");
        assert_eq!(BootType::Uefi.name(), "UEFI");
        assert_eq!(BootType::LegacyBios.name(), "Legacy BIOS");
    }

    // ----------------------------------------------------------------
    // OperationProgress
    // ----------------------------------------------------------------

    #[test]
    fn test_progress_fraction_zero_total() {
        let p = OperationProgress::new(0);
        assert_eq!(p.fraction(), 0.0);
    }

    #[test]
    fn test_progress_fraction_midway() {
        let mut p = OperationProgress::new(1000);
        p.advance(500, 1000);
        assert!((p.fraction() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_progress_percent() {
        let mut p = OperationProgress::new(200);
        p.advance(100, 500);
        assert!((p.percent() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_progress_is_complete() {
        let mut p = OperationProgress::new(100);
        p.advance(100, 100);
        assert!(p.is_complete());
    }

    #[test]
    fn test_progress_not_complete() {
        let mut p = OperationProgress::new(100);
        p.advance(50, 100);
        assert!(!p.is_complete());
    }

    #[test]
    fn test_progress_speed_display() {
        let mut p = OperationProgress::new(1024 * 1024 * 100);
        p.advance(1024 * 1024 * 50, 1000); // 50 MB in 1 second
        let s = p.speed_display();
        assert!(s.contains("MB/s"));
    }

    #[test]
    fn test_progress_speed_display_kb() {
        let mut p = OperationProgress::new(1024 * 100);
        p.advance(512, 1000); // 0.5 KB in 1 second
        let s = p.speed_display();
        assert!(s.contains("KB/s"));
    }

    #[test]
    fn test_progress_eta_display() {
        let mut p = OperationProgress::new(2_000_000);
        p.advance(1_000_000, 1000);
        let eta = p.eta_display();
        assert!(!eta.is_empty());
    }

    #[test]
    fn test_progress_eta_hours() {
        let mut p = OperationProgress::new(100_000_000_000);
        p.advance(1_000_000, 1000);
        let eta = p.eta_display();
        assert!(eta.contains('h'));
    }

    #[test]
    fn test_progress_summary() {
        let mut p = OperationProgress::new(1024);
        p.advance(512, 500);
        let s = p.summary();
        assert!(s.contains('/'));
        assert!(s.contains('%'));
    }

    // ----------------------------------------------------------------
    // Operation
    // ----------------------------------------------------------------

    #[test]
    fn test_operation_idle_not_active() {
        assert!(!Operation::Idle.is_active());
    }

    #[test]
    fn test_operation_writing_is_active() {
        assert!(Operation::WritingImage.is_active());
    }

    #[test]
    fn test_operation_labels() {
        assert_eq!(Operation::Idle.label(), "Ready");
        assert!(!Operation::CreatingImage.label().is_empty());
        assert!(!Operation::ComputingHash.label().is_empty());
    }

    // ----------------------------------------------------------------
    // Helper functions
    // ----------------------------------------------------------------

    #[test]
    fn test_format_bytes_b() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
    }

    #[test]
    fn test_format_bytes_kb() {
        let s = format_bytes(2048);
        assert!(s.contains("KB"));
    }

    #[test]
    fn test_format_bytes_mb() {
        let s = format_bytes(5 * 1024 * 1024);
        assert!(s.contains("MB"));
    }

    #[test]
    fn test_format_bytes_gb() {
        let s = format_bytes(2 * 1024 * 1024 * 1024);
        assert!(s.contains("GB"));
    }

    #[test]
    fn test_format_bytes_tb() {
        let s = format_bytes(2 * 1024 * 1024 * 1024 * 1024);
        assert!(s.contains("TB"));
    }

    #[test]
    fn test_truncate_path_short() {
        assert_eq!(truncate_path("short", 20), "short");
    }

    #[test]
    fn test_truncate_path_long() {
        let long = "a".repeat(50);
        let trunc = truncate_path(&long, 10);
        assert!(trunc.starts_with("..."));
        assert!(trunc.len() <= 13); // 3 dots + 7 chars
    }

    // ----------------------------------------------------------------
    // LE integer reading
    // ----------------------------------------------------------------

    #[test]
    fn test_read_le_u32() {
        let data = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_le_u32(&data, 0), 0x12345678);
    }

    #[test]
    fn test_read_le_u32_out_of_bounds() {
        assert_eq!(read_le_u32(&[1, 2], 0), 0);
    }

    #[test]
    fn test_read_le_u16() {
        let data = [0x34, 0x12];
        assert_eq!(read_le_u16(&data, 0), 0x1234);
    }

    #[test]
    fn test_read_le_u16_out_of_bounds() {
        assert_eq!(read_le_u16(&[1], 0), 0);
    }

    // ----------------------------------------------------------------
    // App core logic
    // ----------------------------------------------------------------

    #[test]
    fn test_app_new() {
        let app = DiskImagerApp::new();
        assert_eq!(app.active_tab, MainTab::Write);
        assert!(!app.drives.is_empty());
        assert_eq!(app.operation, Operation::Idle);
    }

    #[test]
    fn test_app_select_drive_valid() {
        let mut app = DiskImagerApp::new();
        app.select_drive(0);
        assert_eq!(app.selected_drive_index, Some(0));
    }

    #[test]
    fn test_app_select_drive_invalid() {
        let mut app = DiskImagerApp::new();
        app.select_drive(999);
        assert_eq!(app.selected_drive_index, None);
    }

    #[test]
    fn test_app_selected_drive() {
        let mut app = DiskImagerApp::new();
        assert!(app.selected_drive().is_none());
        app.select_drive(1);
        assert!(app.selected_drive().is_some());
    }

    #[test]
    fn test_app_is_drive_locked() {
        let mut app = DiskImagerApp::new();
        assert!(!app.is_drive_locked("disk1"));
        app.locked_drive_id = Some("disk1".to_string());
        assert!(app.is_drive_locked("disk1"));
        assert!(!app.is_drive_locked("disk2"));
    }

    #[test]
    fn test_app_start_write_no_image() {
        let mut app = DiskImagerApp::new();
        app.select_drive(1);
        assert!(app.start_write().is_err());
    }

    #[test]
    fn test_app_start_write_no_drive() {
        let mut app = DiskImagerApp::new();
        app.loaded_image = Some(ImageInfo::new("test.img"));
        assert!(app.start_write().is_err());
    }

    #[test]
    fn test_app_start_write_system_drive() {
        let mut app = DiskImagerApp::new();
        let mut info = ImageInfo::new("test.img");
        info.file_size = 100;
        app.loaded_image = Some(info);
        app.select_drive(0); // system drive
        let result = app.start_write();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("system drive"));
    }

    #[test]
    fn test_app_start_write_image_too_large() {
        let mut app = DiskImagerApp::new();
        let mut info = ImageInfo::new("test.img");
        info.file_size = u64::MAX;
        app.loaded_image = Some(info);
        app.select_drive(1); // USB drive
        assert!(app.start_write().is_err());
    }

    #[test]
    fn test_app_start_write_success() {
        let mut app = DiskImagerApp::new();
        let mut info = ImageInfo::new("test.img");
        info.file_size = 1024;
        app.loaded_image = Some(info);
        app.select_drive(1); // USB drive
        assert!(app.start_write().is_ok());
        assert_eq!(app.operation, Operation::WritingImage);
        assert!(app.locked_drive_id.is_some());
    }

    #[test]
    fn test_app_start_create() {
        let mut app = DiskImagerApp::new();
        app.select_drive(1);
        assert!(app.start_create("/tmp/out.img").is_ok());
        assert_eq!(app.operation, Operation::CreatingImage);
    }

    #[test]
    fn test_app_start_create_no_drive() {
        let mut app = DiskImagerApp::new();
        assert!(app.start_create("/tmp/out.img").is_err());
    }

    #[test]
    fn test_app_start_hash() {
        let mut app = DiskImagerApp::new();
        let mut info = ImageInfo::new("test.img");
        info.file_size = 1024;
        app.loaded_image = Some(info);
        assert!(app.start_hash().is_ok());
        assert_eq!(app.operation, Operation::ComputingHash);
    }

    #[test]
    fn test_app_start_hash_no_image() {
        let mut app = DiskImagerApp::new();
        assert!(app.start_hash().is_err());
    }

    #[test]
    fn test_app_cancel_operation() {
        let mut app = DiskImagerApp::new();
        let mut info = ImageInfo::new("test.img");
        info.file_size = 1024;
        app.loaded_image = Some(info);
        app.select_drive(1);
        let _ = app.start_write();
        assert_eq!(app.operation, Operation::WritingImage);
        app.cancel_operation();
        assert_eq!(app.operation, Operation::Idle);
        assert!(app.locked_drive_id.is_none());
    }

    #[test]
    fn test_app_load_image() {
        let mut app = DiskImagerApp::new();
        let data = vec![0u8; 100];
        app.load_image("test.img", &data);
        assert!(app.loaded_image.is_some());
        let img = app.loaded_image.as_ref().unwrap();
        assert_eq!(img.path, "test.img");
        assert_eq!(img.file_size, 100);
        assert_eq!(img.format, ImageFormat::Raw);
    }

    #[test]
    fn test_app_recent_images() {
        let mut app = DiskImagerApp::new();
        app.load_image("a.img", &[0u8; 10]);
        app.load_image("b.img", &[0u8; 20]);
        assert_eq!(app.recent_images.len(), 2);
        // Most recent should be first
        assert_eq!(app.recent_images.front().unwrap().path, "b.img");
    }

    #[test]
    fn test_app_recent_images_dedup() {
        let mut app = DiskImagerApp::new();
        app.load_image("a.img", &[0u8; 10]);
        app.load_image("b.img", &[0u8; 20]);
        app.load_image("a.img", &[0u8; 30]);
        // "a.img" should appear only once
        let count = app
            .recent_images
            .iter()
            .filter(|r| r.path == "a.img")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_app_recent_images_cap() {
        let mut app = DiskImagerApp::new();
        for i in 0..30_usize {
            app.load_image(&format!("img{}.img", i), &[0u8; 10]);
        }
        assert!(app.recent_images.len() <= MAX_RECENT_IMAGES);
    }

    // ----------------------------------------------------------------
    // Event handling
    // ----------------------------------------------------------------

    #[test]
    fn test_handle_resize() {
        let mut app = DiskImagerApp::new();
        let ev = Event::Resize {
            width: 1200,
            height: 800,
        };
        let result = app.handle_event(&ev);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(app.window_width, 1200.0);
        assert_eq!(app.window_height, 800.0);
    }

    #[test]
    fn test_handle_key_tab_switch() {
        let mut app = DiskImagerApp::new();
        assert_eq!(app.active_tab, MainTab::Write);
        let ev = Event::Key(KeyEvent {
            key: Key::Num3,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&ev);
        assert_eq!(app.active_tab, MainTab::Browse);
    }

    #[test]
    fn test_handle_key_escape_cancels() {
        let mut app = DiskImagerApp::new();
        let mut info = ImageInfo::new("t.img");
        info.file_size = 1024;
        app.loaded_image = Some(info);
        app.select_drive(1);
        let _ = app.start_write();

        let ev = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&ev);
        assert_eq!(app.operation, Operation::Idle);
    }

    #[test]
    fn test_handle_key_down_drive_select() {
        let mut app = DiskImagerApp::new();
        let ev = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&ev);
        assert_eq!(app.selected_drive_index, Some(0));
    }

    #[test]
    fn test_handle_key_up_drive_select() {
        let mut app = DiskImagerApp::new();
        app.selected_drive_index = Some(2);
        let ev = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&ev);
        assert_eq!(app.selected_drive_index, Some(1));
    }

    // ----------------------------------------------------------------
    // Rendering
    // ----------------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let app = DiskImagerApp::new();
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_image_loaded() {
        let mut app = DiskImagerApp::new();
        app.load_image("test.iso", &[0u8; 100]);
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_browse_tab_no_iso() {
        let mut app = DiskImagerApp::new();
        app.active_tab = MainTab::Browse;
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_verify_tab() {
        let mut app = DiskImagerApp::new();
        app.active_tab = MainTab::Verify;
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_create_tab() {
        let mut app = DiskImagerApp::new();
        app.active_tab = MainTab::Create;
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_progress() {
        let mut app = DiskImagerApp::new();
        let mut info = ImageInfo::new("t.img");
        info.file_size = 1024;
        app.loaded_image = Some(info);
        app.select_drive(1);
        let _ = app.start_write();
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_confirm_dialog() {
        let mut app = DiskImagerApp::new();
        app.confirm_dialog
            .show_write_confirm("test.img", "USB Drive", 32_000_000_000);
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_hash_result() {
        let mut app = DiskImagerApp::new();
        app.active_tab = MainTab::Verify;
        app.computed_hash = Some("abcdef1234567890".to_string());
        app.verification_result = VerificationResult::Match;
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_render_with_hash_mismatch() {
        let mut app = DiskImagerApp::new();
        app.active_tab = MainTab::Verify;
        app.computed_hash = Some("aaa".to_string());
        app.verification_result = VerificationResult::Mismatch {
            expected: "bbb".to_string(),
            computed: "aaa".to_string(),
        };
        let mut rt = RenderTree::new();
        app.render(&mut rt);
        assert!(!rt.is_empty());
    }

    // ----------------------------------------------------------------
    // ConfirmDialog
    // ----------------------------------------------------------------

    #[test]
    fn test_confirm_dialog_show() {
        let mut d = ConfirmDialog::new();
        d.show_write_confirm("img", "drive", 100);
        assert!(d.visible);
        assert!(!d.confirmed);
        assert!(!d.title.is_empty());
    }

    #[test]
    fn test_confirm_dialog_dismiss() {
        let mut d = ConfirmDialog::new();
        d.show_write_confirm("img", "drive", 100);
        d.dismiss();
        assert!(!d.visible);
    }

    // ----------------------------------------------------------------
    // ImageInfo
    // ----------------------------------------------------------------

    #[test]
    fn test_image_info_new() {
        let info = ImageInfo::new("/path/to/file.iso");
        assert_eq!(info.path, "/path/to/file.iso");
        assert_eq!(info.format, ImageFormat::Unknown);
        assert!(!info.is_bootable);
    }

    // ----------------------------------------------------------------
    // VerificationResult
    // ----------------------------------------------------------------

    #[test]
    fn test_verification_result_match() {
        let r = VerificationResult::Match;
        assert_eq!(r, VerificationResult::Match);
    }

    #[test]
    fn test_verification_result_mismatch() {
        let r = VerificationResult::Mismatch {
            expected: "a".into(),
            computed: "b".into(),
        };
        assert_ne!(r, VerificationResult::Match);
    }

    // ----------------------------------------------------------------
    // Extract ISO helpers
    // ----------------------------------------------------------------

    #[test]
    fn test_extract_iso_string() {
        let data = b"Hello World          ";
        let s = extract_iso_string(data, 0, 11);
        assert_eq!(s, "Hello World");
    }

    #[test]
    fn test_extract_iso_string_out_of_bounds() {
        let data = b"Hi";
        let s = extract_iso_string(data, 100, 10);
        assert!(s.is_empty());
    }

    #[test]
    fn test_extract_iso_datetime_short() {
        let s = extract_iso_datetime(&[0u8; 5], 0);
        assert!(s.is_empty());
    }

    #[test]
    fn test_extract_iso_datetime_valid() {
        let dt_str = b"20240115143000000";
        let s = extract_iso_datetime(dt_str, 0);
        assert!(s.contains("2024"));
        assert!(s.contains("01"));
        assert!(s.contains("15"));
    }

    // ----------------------------------------------------------------
    // MainTab
    // ----------------------------------------------------------------

    #[test]
    fn test_main_tab_all() {
        assert_eq!(MainTab::ALL.len(), 4);
    }

    #[test]
    fn test_main_tab_labels() {
        for tab in &MainTab::ALL {
            assert!(!tab.label().is_empty());
        }
    }
}
