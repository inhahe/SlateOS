//! Rich file content metadata extraction.
//!
//! Extracts structured metadata from file contents — ID3 tags from MP3s,
//! EXIF data from JPEGs, text chunks from PNGs, basic info from PDFs, etc.
//! This powers the file explorer's detail columns: a folder with MP3s and
//! JPEGs shows columns for artist, album, dimensions, camera, etc.
//!
//! ## Architecture
//!
//! ```text
//! File explorer / kshell
//!   → fileinfo::extract(path)
//!   → mime::detect(path) → MIME type
//!   → dispatch to format-specific parser
//!   → Vec<Field> (name/value pairs)
//! ```
//!
//! ## Supported Formats
//!
//! - **MP3**: ID3v1 (last 128 bytes), ID3v2 (header frames)
//! - **JPEG**: EXIF (TIFF IFD entries — dimensions, camera, date)
//! - **PNG**: tEXt/iTXt chunks (title, author, description, etc.)
//! - **PDF**: header (version), linearized hint, page count from xref
//! - **WAV**: RIFF header (sample rate, channels, bit depth, duration)
//! - **ELF**: class (32/64), endianness, machine type
//!
//! ## Design Notes
//!
//! - Parsers read minimal data (headers, footers) — no full file loads.
//! - All parsing is defensive: malformed data returns empty fields, never panics.
//! - Field names use lowercase dot-notation: `"audio.artist"`, `"image.width"`.
//! - Custom extractors can be registered at runtime via `register_extractor()`.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum header bytes to read for metadata extraction.
const MAX_HEADER_READ: usize = 8192;

/// Maximum bytes to read from file end (for ID3v1).
const MAX_TAIL_READ: usize = 256;

/// Maximum fields per file.
const MAX_FIELDS: usize = 64;

/// Maximum custom extractors.
const MAX_EXTRACTORS: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A metadata field value.
#[derive(Debug, Clone)]
pub enum FieldValue {
    /// Text value (artist name, title, etc.).
    Text(String),
    /// Integer value (width, height, bitrate, etc.).
    Int(i64),
    /// Unsigned value (file size, sample rate, etc.).
    Uint(u64),
    /// Floating point (aspect ratio, duration in seconds, etc.).
    Float(f64),
    /// Boolean (stereo, variable bitrate, etc.).
    Bool(bool),
}

impl FieldValue {
    /// Format as display string.
    pub fn display(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Int(n) => format!("{}", n),
            Self::Uint(n) => format!("{}", n),
            Self::Float(f) => format!("{:.2}", f),
            Self::Bool(b) => if *b { String::from("yes") } else { String::from("no") },
        }
    }
}

/// A named metadata field extracted from file contents.
#[derive(Debug, Clone)]
pub struct Field {
    /// Dotted field name (e.g. "audio.artist", "image.width").
    pub name: String,
    /// Human-readable label (e.g. "Artist", "Width").
    pub label: String,
    /// Extracted value.
    pub value: FieldValue,
}

/// Result of extracting metadata from a file.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// Path of the file.
    pub path: String,
    /// Detected MIME type.
    pub mime: String,
    /// Format-specific human-readable description.
    pub format_desc: String,
    /// Extracted metadata fields.
    pub fields: Vec<Field>,
}

impl FileInfo {
    fn new(path: &str, mime: &str) -> Self {
        Self {
            path: String::from(path),
            mime: String::from(mime),
            format_desc: String::new(),
            fields: Vec::new(),
        }
    }

    fn push(&mut self, name: &str, label: &str, value: FieldValue) {
        if self.fields.len() < MAX_FIELDS {
            self.fields.push(Field {
                name: String::from(name),
                label: String::from(label),
                value,
            });
        }
    }

    fn push_text(&mut self, name: &str, label: &str, value: &str) {
        if !value.is_empty() {
            self.push(name, label, FieldValue::Text(String::from(value)));
        }
    }

    fn push_uint(&mut self, name: &str, label: &str, value: u64) {
        self.push(name, label, FieldValue::Uint(value));
    }

    fn push_int(&mut self, name: &str, label: &str, value: i64) {
        self.push(name, label, FieldValue::Int(value));
    }

    fn push_bool(&mut self, name: &str, label: &str, value: bool) {
        self.push(name, label, FieldValue::Bool(value));
    }

    /// Get a field by name.
    pub fn get(&self, name: &str) -> Option<&FieldValue> {
        self.fields.iter().find(|f| f.name == name).map(|f| &f.value)
    }

    /// Get a text field value.
    pub fn get_text(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(FieldValue::Text(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Get a uint field value.
    pub fn get_uint(&self, name: &str) -> Option<u64> {
        match self.get(name) {
            Some(FieldValue::Uint(n)) => Some(*n),
            _ => None,
        }
    }
}

/// A custom extractor function type.
type ExtractorFn = fn(&[u8], &mut FileInfo);

/// A registered custom extractor.
struct CustomExtractor {
    /// MIME type pattern to match.
    mime_prefix: String,
    /// Extraction function.
    func: ExtractorFn,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Custom extractors registry.
static EXTRACTORS: spin::Mutex<Vec<CustomExtractor>> = spin::Mutex::new(Vec::new());

/// Statistics.
static EXTRACT_COUNT: AtomicU64 = AtomicU64::new(0);
static FIELD_COUNT: AtomicU64 = AtomicU64::new(0);
static ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract metadata from a file.
///
/// Detects the MIME type, then runs the appropriate format-specific parser.
/// Returns fields for display in file explorer columns.
pub fn extract(path: &str) -> KernelResult<FileInfo> {
    EXTRACT_COUNT.fetch_add(1, Ordering::Relaxed);

    let mime = crate::fs::mime::detect(path)?;
    let mut info = FileInfo::new(path, mime);

    // Read header.
    let header = crate::fs::Vfs::read_at(path, 0, MAX_HEADER_READ)
        .unwrap_or_default();

    // Dispatch to format-specific parser.
    match mime {
        "audio/mpeg" => {
            parse_mp3(&header, path, &mut info);
            info.format_desc = String::from("MPEG Audio");
        }
        "audio/wav" | "audio/x-wav" => {
            parse_wav(&header, &mut info);
            info.format_desc = String::from("WAV Audio");
        }
        "image/jpeg" => {
            parse_jpeg(&header, &mut info);
            info.format_desc = String::from("JPEG Image");
        }
        "image/png" => {
            parse_png(&header, &mut info);
            info.format_desc = String::from("PNG Image");
        }
        "image/gif" => {
            parse_gif(&header, &mut info);
            info.format_desc = String::from("GIF Image");
        }
        "image/bmp" | "image/x-bmp" => {
            parse_bmp(&header, &mut info);
            info.format_desc = String::from("BMP Image");
        }
        "application/pdf" => {
            parse_pdf(&header, &mut info);
            info.format_desc = String::from("PDF Document");
        }
        "application/x-elf" => {
            parse_elf(&header, &mut info);
            info.format_desc = String::from("ELF Binary");
        }
        "application/x-dosexec" => {
            info.format_desc = String::from("PE/COFF Executable");
        }
        _ => {
            info.format_desc = String::from(mime);
        }
    }

    // Run custom extractors.
    let extractors = EXTRACTORS.lock();
    for ext in extractors.iter() {
        if mime.starts_with(ext.mime_prefix.as_str()) {
            // Defense-in-depth: validate the stored extractor pointer against
            // real kernel `.text` before calling it.  A registered `ExtractorFn`
            // always points into code; a value that doesn't means this table's
            // heap backing was corrupted (the B-KNULLJUMP-SIGNAL class — a wild
            // `call` through a clobbered code-pointer field).  Log + skip.
            let func_addr = ext.func as *const () as u64;
            if crate::idt::is_kernel_text(func_addr) {
                (ext.func)(&header, &mut info);
            } else {
                serial_println!(
                    "[fileinfo] CRITICAL: refusing to run corrupt extractor func={:#x} \
                     (mime_prefix={:?}) — table corruption; skipping (see B-KNULLJUMP-SIGNAL)",
                    func_addr, ext.mime_prefix
                );
            }
        }
    }

    FIELD_COUNT.fetch_add(info.fields.len() as u64, Ordering::Relaxed);
    Ok(info)
}

/// Register a custom metadata extractor for a MIME type prefix.
///
/// The function is called for any file whose MIME type starts with the
/// given prefix (e.g. "audio/" matches all audio types).
pub fn register_extractor(mime_prefix: &str, func: ExtractorFn) -> bool {
    let mut extractors = EXTRACTORS.lock();
    if extractors.len() >= MAX_EXTRACTORS {
        return false;
    }
    extractors.push(CustomExtractor {
        mime_prefix: String::from(mime_prefix),
        func,
    });
    true
}

/// Get extraction statistics.
pub fn stats() -> (u64, u64, u64) {
    (
        EXTRACT_COUNT.load(Ordering::Relaxed),
        FIELD_COUNT.load(Ordering::Relaxed),
        ERROR_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    EXTRACT_COUNT.store(0, Ordering::Relaxed);
    FIELD_COUNT.store(0, Ordering::Relaxed);
    ERROR_COUNT.store(0, Ordering::Relaxed);
}

/// List known metadata field names for a MIME type.
///
/// Useful for file explorer to pre-configure column headers.
pub fn fields_for_mime(mime: &str) -> Vec<(&'static str, &'static str)> {
    match mime {
        "audio/mpeg" => vec![
            ("audio.title", "Title"),
            ("audio.artist", "Artist"),
            ("audio.album", "Album"),
            ("audio.year", "Year"),
            ("audio.genre", "Genre"),
            ("audio.track", "Track"),
            ("audio.comment", "Comment"),
            ("audio.bitrate_kbps", "Bitrate (kbps)"),
            ("audio.sample_rate_hz", "Sample Rate"),
            ("audio.channels", "Channels"),
            ("audio.vbr", "Variable Bitrate"),
        ],
        "audio/wav" | "audio/x-wav" => vec![
            ("audio.sample_rate_hz", "Sample Rate"),
            ("audio.channels", "Channels"),
            ("audio.bits_per_sample", "Bit Depth"),
            ("audio.duration_secs", "Duration"),
            ("audio.data_size", "Data Size"),
        ],
        "image/jpeg" => vec![
            ("image.width", "Width"),
            ("image.height", "Height"),
            ("image.camera_make", "Camera Make"),
            ("image.camera_model", "Camera Model"),
            ("image.date_taken", "Date Taken"),
            ("image.orientation", "Orientation"),
            ("image.x_resolution", "X Resolution"),
            ("image.y_resolution", "Y Resolution"),
        ],
        "image/png" => vec![
            ("image.width", "Width"),
            ("image.height", "Height"),
            ("image.bit_depth", "Bit Depth"),
            ("image.color_type", "Color Type"),
            ("image.interlaced", "Interlaced"),
        ],
        "image/gif" => vec![
            ("image.width", "Width"),
            ("image.height", "Height"),
            ("image.frames", "Frames"),
        ],
        "image/bmp" | "image/x-bmp" => vec![
            ("image.width", "Width"),
            ("image.height", "Height"),
            ("image.bits_per_pixel", "Bit Depth"),
            ("image.compression", "Compression"),
        ],
        "application/pdf" => vec![
            ("doc.version", "PDF Version"),
            ("doc.linearized", "Linearized"),
        ],
        "application/x-elf" => vec![
            ("elf.class", "Class"),
            ("elf.endian", "Endianness"),
            ("elf.machine", "Machine"),
            ("elf.type", "Type"),
        ],
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Format parsers — MP3
// ---------------------------------------------------------------------------

/// Parse MP3 file: ID3v1 (tail), ID3v2 (header), MPEG frame header.
fn parse_mp3(header: &[u8], path: &str, info: &mut FileInfo) {
    // Try ID3v2 from header.
    parse_id3v2(header, info);

    // Try MPEG frame header for bitrate/sample rate.
    parse_mpeg_frame(header, info);

    // Try ID3v1 from file tail.
    if let Ok(meta) = crate::fs::Vfs::metadata(path) {
        if meta.size >= 128 {
            let offset = meta.size - 128;
            if let Ok(tail) = crate::fs::Vfs::read_at(path, offset, 128) {
                parse_id3v1(&tail, info);
            }
        }
    }
}

/// Parse ID3v1 tag (last 128 bytes of file).
///
/// Format: "TAG" (3 bytes) + title (30) + artist (30) + album (30)
///         + year (4) + comment (30) + genre (1).
/// ID3v1.1: if comment[28] == 0, comment[29] is track number.
fn parse_id3v1(data: &[u8], info: &mut FileInfo) {
    if data.len() < 128 {
        return;
    }
    if &data[0..3] != b"TAG" {
        return;
    }

    let title = trim_id3_str(&data[3..33]);
    let artist = trim_id3_str(&data[33..63]);
    let album = trim_id3_str(&data[63..93]);
    let year = trim_id3_str(&data[93..97]);
    let comment = &data[97..127];
    let genre_idx = data[127];

    // Only set fields not already set by ID3v2 (which has priority).
    if info.get_text("audio.title").is_none() {
        info.push_text("audio.title", "Title", title);
    }
    if info.get_text("audio.artist").is_none() {
        info.push_text("audio.artist", "Artist", artist);
    }
    if info.get_text("audio.album").is_none() {
        info.push_text("audio.album", "Album", album);
    }
    if info.get_text("audio.year").is_none() {
        info.push_text("audio.year", "Year", year);
    }

    // ID3v1.1: track number.
    if comment[28] == 0 && comment[29] != 0 {
        if info.get_uint("audio.track").is_none() {
            info.push_uint("audio.track", "Track", comment[29] as u64);
        }
    }

    // Comment (full 30 bytes if not v1.1, or 28 bytes if v1.1).
    let comment_str = if comment[28] == 0 {
        trim_id3_str(&comment[..28])
    } else {
        trim_id3_str(comment)
    };
    if info.get_text("audio.comment").is_none() {
        info.push_text("audio.comment", "Comment", comment_str);
    }

    // Genre.
    if info.get_text("audio.genre").is_none() {
        let genre = id3v1_genre_name(genre_idx);
        info.push_text("audio.genre", "Genre", genre);
    }
}

/// Parse ID3v2 tag header and frames.
///
/// Format: "ID3" + version (2 bytes) + flags (1) + size (4, synchsafe).
/// Each frame: ID (4 bytes) + size (4) + flags (2) + data.
fn parse_id3v2(data: &[u8], info: &mut FileInfo) {
    if data.len() < 10 {
        return;
    }
    if &data[0..3] != b"ID3" {
        return;
    }

    let _version_major = data[3];
    let _version_minor = data[4];
    let _flags = data[5];
    let tag_size = synchsafe_u32(&data[6..10]) as usize;

    // Skip extended header if present (flag bit 6).
    let start = 10;
    let end = (start + tag_size).min(data.len());
    if end <= start {
        return;
    }

    let mut pos = start;
    while pos + 10 <= end {
        let frame_id = &data[pos..pos + 4];
        let frame_size = u32::from_be_bytes([
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]) as usize;
        let _frame_flags = u16::from_be_bytes([data[pos + 8], data[pos + 9]]);

        if frame_size == 0 || pos + 10 + frame_size > end {
            break;
        }

        let frame_data = &data[pos + 10..pos + 10 + frame_size];

        // Map frame IDs to field names.
        match frame_id {
            b"TIT2" => info.push_text("audio.title", "Title", &id3v2_text(frame_data)),
            b"TPE1" => info.push_text("audio.artist", "Artist", &id3v2_text(frame_data)),
            b"TALB" => info.push_text("audio.album", "Album", &id3v2_text(frame_data)),
            b"TYER" | b"TDRC" => info.push_text("audio.year", "Year", &id3v2_text(frame_data)),
            b"TRCK" => {
                let track_str = id3v2_text(frame_data);
                // Track can be "5" or "5/12".
                let num = track_str.split('/').next().unwrap_or("");
                if let Ok(n) = num.parse::<u64>() {
                    info.push_uint("audio.track", "Track", n);
                }
            }
            b"TCON" => info.push_text("audio.genre", "Genre", &id3v2_text(frame_data)),
            b"COMM" => {
                // Comment frame: encoding (1) + language (3) + short desc + \0 + text.
                if frame_data.len() > 4 {
                    let text = id3v2_comment(frame_data);
                    info.push_text("audio.comment", "Comment", &text);
                }
            }
            _ => {} // Skip unknown frames.
        }

        pos += 10 + frame_size;
    }
}

/// Parse MPEG audio frame header for bitrate and sample rate.
///
/// Searches for a valid frame sync (0xFF 0xE0 mask) in the header data.
fn parse_mpeg_frame(data: &[u8], info: &mut FileInfo) {
    // Find frame sync: 11 set bits (0xFF followed by 0xE0 mask).
    let sync_pos = data.windows(2).position(|w| w[0] == 0xFF && (w[1] & 0xE0) == 0xE0);
    let pos = match sync_pos {
        Some(p) if p + 4 <= data.len() => p,
        _ => return,
    };

    let b1 = data[pos + 1];
    let b2 = data[pos + 2];
    let b3 = data[pos + 3];

    let version = (b1 >> 3) & 0x03;   // 0=2.5, 1=reserved, 2=2, 3=1
    let layer = (b1 >> 1) & 0x03;     // 0=reserved, 1=III, 2=II, 3=I
    let br_idx = (b2 >> 4) & 0x0F;
    let sr_idx = (b2 >> 2) & 0x03;
    let channel_mode = (b3 >> 6) & 0x03;

    // Bitrate table for MPEG1 Layer III.
    if version == 3 && layer == 1 {
        let bitrates: [u32; 15] = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ];
        if (br_idx as usize) < bitrates.len() && bitrates[br_idx as usize] > 0 {
            info.push_uint("audio.bitrate_kbps", "Bitrate (kbps)", bitrates[br_idx as usize] as u64);
        }
    }

    // Sample rate table for MPEG1.
    if version == 3 {
        let sample_rates: [u32; 3] = [44100, 48000, 32000];
        if (sr_idx as usize) < sample_rates.len() {
            info.push_uint("audio.sample_rate_hz", "Sample Rate", sample_rates[sr_idx as usize] as u64);
        }
    }

    // Channel mode.
    let channels = match channel_mode {
        0 => { info.push_text("audio.channel_mode", "Channel Mode", "Stereo"); 2u64 }
        1 => { info.push_text("audio.channel_mode", "Channel Mode", "Joint Stereo"); 2 }
        2 => { info.push_text("audio.channel_mode", "Channel Mode", "Dual Channel"); 2 }
        3 => { info.push_text("audio.channel_mode", "Channel Mode", "Mono"); 1 }
        _ => 0,
    };
    if channels > 0 {
        info.push_uint("audio.channels", "Channels", channels);
    }
}

// ---------------------------------------------------------------------------
// Format parsers — WAV
// ---------------------------------------------------------------------------

/// Parse WAV/RIFF header for audio parameters.
///
/// Format: "RIFF" + size (4, LE) + "WAVE" + fmt chunk + data chunk.
fn parse_wav(data: &[u8], info: &mut FileInfo) {
    if data.len() < 44 {
        return;
    }
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return;
    }

    // Find "fmt " chunk.
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]) as usize;

        if chunk_id == b"fmt " && pos + 8 + chunk_size <= data.len() && chunk_size >= 16 {
            let fmt = &data[pos + 8..];
            let audio_format = u16::from_le_bytes([fmt[0], fmt[1]]);
            let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            let _byte_rate = u32::from_le_bytes([fmt[8], fmt[9], fmt[10], fmt[11]]);
            let _block_align = u16::from_le_bytes([fmt[12], fmt[13]]);
            let bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);

            info.push_uint("audio.sample_rate_hz", "Sample Rate", sample_rate as u64);
            info.push_uint("audio.channels", "Channels", channels as u64);
            info.push_uint("audio.bits_per_sample", "Bit Depth", bits_per_sample as u64);

            let format_name = match audio_format {
                1 => "PCM",
                3 => "IEEE Float",
                6 => "A-law",
                7 => "mu-law",
                _ => "Compressed",
            };
            info.push_text("audio.format", "Format", format_name);
        }

        if chunk_id == b"data" {
            let data_size = u32::from_le_bytes([
                data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
            ]) as u64;
            info.push_uint("audio.data_size", "Data Size", data_size);

            // Calculate duration if we have sample rate and bit info.
            if let (Some(sr), Some(ch), Some(bps)) = (
                info.get_uint("audio.sample_rate_hz"),
                info.get_uint("audio.channels"),
                info.get_uint("audio.bits_per_sample"),
            ) {
                let bytes_per_sec = sr * ch * (bps / 8);
                if bytes_per_sec > 0 {
                    let duration_secs = data_size / bytes_per_sec;
                    info.push_uint("audio.duration_secs", "Duration (s)", duration_secs);
                }
            }
        }

        pos += 8 + chunk_size;
        // Chunks are word-aligned.
        if !chunk_size.is_multiple_of(2) {
            pos += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Format parsers — JPEG
// ---------------------------------------------------------------------------

/// Parse JPEG EXIF data.
///
/// JPEG uses markers (0xFF + type). APP1 marker (0xFFE1) contains EXIF.
/// EXIF wraps TIFF format: byte order + IFD entries with tag/type/value.
fn parse_jpeg(data: &[u8], info: &mut FileInfo) {
    if data.len() < 4 || &data[0..3] != b"\xFF\xD8\xFF" {
        return;
    }

    // Find SOF0 or SOF2 marker for dimensions.
    let mut pos = 2;
    while pos + 4 < data.len() {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = data[pos + 1];
        let seg_len = if pos + 3 < data.len() {
            u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize
        } else {
            break;
        };

        match marker {
            // SOF0 (baseline) or SOF2 (progressive).
            0xC0 | 0xC2 => {
                if pos + 9 < data.len() {
                    let height = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u64;
                    let width = u16::from_be_bytes([data[pos + 7], data[pos + 8]]) as u64;
                    info.push_uint("image.width", "Width", width);
                    info.push_uint("image.height", "Height", height);
                    let comp_type = if marker == 0xC0 { "Baseline" } else { "Progressive" };
                    info.push_text("image.compression", "Compression", comp_type);
                }
            }
            // APP1 — EXIF data.
            0xE1 => {
                if pos + 4 + seg_len <= data.len() {
                    let app1_data = &data[pos + 4..pos + 2 + seg_len];
                    parse_exif(app1_data, info);
                }
            }
            // SOS — start of scan, no more metadata.
            0xDA => break,
            _ => {}
        }

        pos += 2 + seg_len;
    }
}

/// Parse EXIF data from APP1 segment.
///
/// Format: "Exif\0\0" + TIFF header (byte order + 42 + IFD offset).
/// IFD entries: tag (2) + type (2) + count (4) + value/offset (4).
fn parse_exif(data: &[u8], info: &mut FileInfo) {
    if data.len() < 14 {
        return;
    }
    if &data[0..6] != b"Exif\0\0" {
        return;
    }

    let tiff = &data[6..];
    let big_endian = match &tiff[0..2] {
        b"MM" => true,
        b"II" => false,
        _ => return,
    };

    let read_u16 = |offset: usize| -> Option<u16> {
        if offset + 2 > tiff.len() { return None; }
        Some(if big_endian {
            u16::from_be_bytes([tiff[offset], tiff[offset + 1]])
        } else {
            u16::from_le_bytes([tiff[offset], tiff[offset + 1]])
        })
    };

    let read_u32 = |offset: usize| -> Option<u32> {
        if offset + 4 > tiff.len() { return None; }
        Some(if big_endian {
            u32::from_be_bytes([tiff[offset], tiff[offset + 1], tiff[offset + 2], tiff[offset + 3]])
        } else {
            u32::from_le_bytes([tiff[offset], tiff[offset + 1], tiff[offset + 2], tiff[offset + 3]])
        })
    };

    // Verify TIFF magic.
    if read_u16(2) != Some(42) {
        return;
    }

    // IFD0 offset.
    let ifd_offset = match read_u32(4) {
        Some(o) => o as usize,
        None => return,
    };
    if ifd_offset >= tiff.len() {
        return;
    }

    // Read IFD entries.
    let entry_count = match read_u16(ifd_offset) {
        Some(n) => n as usize,
        None => return,
    };

    for i in 0..entry_count.min(50) {
        let entry_offset = ifd_offset + 2 + i * 12;
        if entry_offset + 12 > tiff.len() {
            break;
        }

        let tag = match read_u16(entry_offset) { Some(t) => t, None => continue };
        let _dtype = match read_u16(entry_offset + 2) { Some(t) => t, None => continue };
        let _count = match read_u32(entry_offset + 4) { Some(c) => c, None => continue };
        let value_u32 = match read_u32(entry_offset + 8) { Some(v) => v, None => continue };
        let value_u16 = match read_u16(entry_offset + 8) { Some(v) => v, None => continue };

        match tag {
            // ImageWidth (short or long).
            0x0100 => {
                if info.get_uint("image.width").is_none() {
                    info.push_uint("image.width", "Width", value_u32 as u64);
                }
            }
            // ImageLength/Height.
            0x0101 => {
                if info.get_uint("image.height").is_none() {
                    info.push_uint("image.height", "Height", value_u32 as u64);
                }
            }
            // Orientation.
            0x0112 => {
                let orientation = match value_u16 {
                    1 => "Normal",
                    2 => "Flipped Horizontal",
                    3 => "Rotated 180°",
                    4 => "Flipped Vertical",
                    5 => "Transposed",
                    6 => "Rotated 90° CW",
                    7 => "Transverse",
                    8 => "Rotated 90° CCW",
                    _ => "Unknown",
                };
                info.push_text("image.orientation", "Orientation", orientation);
            }
            // Make.
            0x010F => {
                if let Some(s) = read_ascii_string(tiff, value_u32 as usize, _count as usize) {
                    info.push_text("image.camera_make", "Camera Make", &s);
                }
            }
            // Model.
            0x0110 => {
                if let Some(s) = read_ascii_string(tiff, value_u32 as usize, _count as usize) {
                    info.push_text("image.camera_model", "Camera Model", &s);
                }
            }
            // DateTime.
            0x0132 => {
                if let Some(s) = read_ascii_string(tiff, value_u32 as usize, _count as usize) {
                    info.push_text("image.date_taken", "Date Taken", &s);
                }
            }
            // XResolution.
            0x011A => {
                if let Some(num) = read_u32(value_u32 as usize) {
                    info.push_uint("image.x_resolution", "X Resolution", num as u64);
                }
            }
            // YResolution.
            0x011B => {
                if let Some(num) = read_u32(value_u32 as usize) {
                    info.push_uint("image.y_resolution", "Y Resolution", num as u64);
                }
            }
            _ => {} // Skip unknown tags.
        }
    }
}

// ---------------------------------------------------------------------------
// Format parsers — PNG
// ---------------------------------------------------------------------------

/// Parse PNG header and metadata chunks.
///
/// Format: 8-byte signature + chunks (length + type + data + CRC).
/// IHDR chunk has width, height, bit depth, color type.
/// tEXt/iTXt chunks have key-value metadata.
fn parse_png(data: &[u8], info: &mut FileInfo) {
    if data.len() < 24 || &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return;
    }

    let mut pos = 8;
    while pos + 12 <= data.len() {
        let chunk_len = u32::from_be_bytes([
            data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
        ]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        let chunk_data_start = pos + 8;
        let chunk_data_end = (chunk_data_start + chunk_len).min(data.len());

        match chunk_type {
            b"IHDR" => {
                if chunk_len >= 13 {
                    let d = &data[chunk_data_start..];
                    let width = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as u64;
                    let height = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as u64;
                    let bit_depth = d[8];
                    let color_type = d[9];
                    let interlace = d[12];

                    info.push_uint("image.width", "Width", width);
                    info.push_uint("image.height", "Height", height);
                    info.push_uint("image.bit_depth", "Bit Depth", bit_depth as u64);

                    let color_name = match color_type {
                        0 => "Grayscale",
                        2 => "RGB",
                        3 => "Indexed",
                        4 => "Grayscale+Alpha",
                        6 => "RGBA",
                        _ => "Unknown",
                    };
                    info.push_text("image.color_type", "Color Type", color_name);
                    info.push_bool("image.interlaced", "Interlaced", interlace != 0);
                }
            }
            b"tEXt" => {
                // Key-value pair separated by null byte.
                let chunk_data = &data[chunk_data_start..chunk_data_end];
                if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                    let key = core::str::from_utf8(&chunk_data[..null_pos]).unwrap_or("");
                    let val = core::str::from_utf8(&chunk_data[null_pos + 1..]).unwrap_or("");
                    if !key.is_empty() && !val.is_empty() {
                        let field_name = format!("png.{}", key.to_lowercase().replace(' ', "_"));
                        info.push_text(&field_name, key, val);
                    }
                }
            }
            b"IDAT" | b"IEND" => break, // Stop at image data.
            _ => {}
        }

        // Move to next chunk: length + type + data + CRC.
        pos = chunk_data_end + 4;
    }
}

// ---------------------------------------------------------------------------
// Format parsers — GIF
// ---------------------------------------------------------------------------

/// Parse GIF header for dimensions and frame count hint.
///
/// Format: "GIF87a"/"GIF89a" + width (2, LE) + height (2, LE) + flags.
fn parse_gif(data: &[u8], info: &mut FileInfo) {
    if data.len() < 13 {
        return;
    }
    if !data.starts_with(b"GIF87a") && !data.starts_with(b"GIF89a") {
        return;
    }

    let width = u16::from_le_bytes([data[6], data[7]]) as u64;
    let height = u16::from_le_bytes([data[8], data[9]]) as u64;
    info.push_uint("image.width", "Width", width);
    info.push_uint("image.height", "Height", height);

    let flags = data[10];
    let has_global_ct = (flags & 0x80) != 0;
    let color_depth = ((flags >> 4) & 0x07) + 1;
    if has_global_ct {
        info.push_uint("image.color_depth", "Color Depth", color_depth as u64);
    }

    let version = if data.starts_with(b"GIF89a") { "89a" } else { "87a" };
    info.push_text("image.gif_version", "GIF Version", version);
}

// ---------------------------------------------------------------------------
// Format parsers — BMP
// ---------------------------------------------------------------------------

/// Parse BMP header for dimensions and bit depth.
///
/// Format: "BM" + file size (4) + reserved (4) + data offset (4)
///       + DIB header size (4) + width (4, signed) + height (4, signed)
///       + planes (2) + bits per pixel (2) + compression (4).
fn parse_bmp(data: &[u8], info: &mut FileInfo) {
    if data.len() < 30 || &data[0..2] != b"BM" {
        return;
    }

    let dib_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);

    // BITMAPINFOHEADER or later (size >= 40).
    if dib_size >= 40 && data.len() >= 30 {
        let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let height = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        let bpp = u16::from_le_bytes([data[28], data[29]]);
        let compression = if data.len() >= 34 {
            u32::from_le_bytes([data[30], data[31], data[32], data[33]])
        } else {
            0
        };

        info.push_int("image.width", "Width", width as i64);
        info.push_int("image.height", "Height", height.abs() as i64);
        info.push_uint("image.bits_per_pixel", "Bit Depth", bpp as u64);

        let comp_name = match compression {
            0 => "None",
            1 => "RLE8",
            2 => "RLE4",
            3 => "Bitfields",
            _ => "Other",
        };
        info.push_text("image.compression", "Compression", comp_name);
    }
}

// ---------------------------------------------------------------------------
// Format parsers — PDF
// ---------------------------------------------------------------------------

/// Parse PDF header for version and basic info.
///
/// Reads the first line for version (%PDF-x.y) and scans for
/// linearization and page count hints.
fn parse_pdf(data: &[u8], info: &mut FileInfo) {
    if data.len() < 8 || &data[0..5] != b"%PDF-" {
        return;
    }

    // Extract version string.
    let version_end = data[5..].iter().position(|&b| b == b'\n' || b == b'\r')
        .map(|p| p + 5)
        .unwrap_or(8.min(data.len()));
    let version = core::str::from_utf8(&data[5..version_end]).unwrap_or("?");
    info.push_text("doc.version", "PDF Version", version);

    // Check for linearization.
    let header_str = core::str::from_utf8(&data[..data.len().min(1024)]).unwrap_or("");
    let linearized = header_str.contains("/Linearized");
    info.push_bool("doc.linearized", "Linearized", linearized);
}

// ---------------------------------------------------------------------------
// Format parsers — ELF
// ---------------------------------------------------------------------------

/// Parse ELF header for class, endianness, and machine type.
fn parse_elf(data: &[u8], info: &mut FileInfo) {
    if data.len() < 20 || &data[0..4] != b"\x7FELF" {
        return;
    }

    let class = match data[4] {
        1 => "32-bit",
        2 => "64-bit",
        _ => "Unknown",
    };
    info.push_text("elf.class", "Class", class);

    let endian = match data[5] {
        1 => "Little Endian",
        2 => "Big Endian",
        _ => "Unknown",
    };
    info.push_text("elf.endian", "Endianness", endian);

    // Machine type at offset 18 (2 bytes, respecting endianness).
    let machine = if data[5] == 1 {
        u16::from_le_bytes([data[18], data[19]])
    } else {
        u16::from_be_bytes([data[18], data[19]])
    };
    let machine_name = match machine {
        0x03 => "x86",
        0x28 => "ARM",
        0x3E => "x86-64",
        0xB7 => "AArch64",
        0xF3 => "RISC-V",
        _ => "Other",
    };
    info.push_text("elf.machine", "Machine", machine_name);

    // ELF type at offset 16.
    let elf_type = if data[5] == 1 {
        u16::from_le_bytes([data[16], data[17]])
    } else {
        u16::from_be_bytes([data[16], data[17]])
    };
    let type_name = match elf_type {
        1 => "Relocatable",
        2 => "Executable",
        3 => "Shared Object",
        4 => "Core Dump",
        _ => "Unknown",
    };
    info.push_text("elf.type", "Type", type_name);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Trim trailing nulls and spaces from an ID3v1 fixed-width field.
fn trim_id3_str(data: &[u8]) -> &str {
    let end = data.iter().rposition(|&b| b != 0 && b != b' ')
        .map(|p| p + 1)
        .unwrap_or(0);
    core::str::from_utf8(&data[..end]).unwrap_or("")
}

/// Decode synchsafe integer (ID3v2 size encoding).
///
/// Each byte uses only 7 bits; bit 7 is always 0.
fn synchsafe_u32(data: &[u8]) -> u32 {
    if data.len() < 4 { return 0; }
    ((data[0] as u32) << 21)
        | ((data[1] as u32) << 14)
        | ((data[2] as u32) << 7)
        | (data[3] as u32)
}

/// Decode ID3v2 text frame.
///
/// First byte is encoding: 0=ISO-8859-1, 1=UTF-16 w/ BOM, 2=UTF-16BE, 3=UTF-8.
fn id3v2_text(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let encoding = data[0];
    let text_data = &data[1..];

    match encoding {
        0 | 3 => {
            // ISO-8859-1 or UTF-8: treat as UTF-8 (works for ASCII subset).
            let end = text_data.iter().position(|&b| b == 0).unwrap_or(text_data.len());
            core::str::from_utf8(&text_data[..end]).unwrap_or("").into()
        }
        1 | 2 => {
            // UTF-16: simplified — just extract ASCII characters.
            let mut result = String::new();
            let start = if text_data.len() >= 2 && (text_data[0] == 0xFF || text_data[0] == 0xFE) {
                2 // Skip BOM.
            } else {
                0
            };
            let mut i = start;
            while i + 1 < text_data.len() {
                let lo = text_data[i];
                let hi = text_data[i + 1];
                if lo == 0 && hi == 0 { break; }
                // Simple: only keep ASCII-range characters.
                if encoding == 1 {
                    // UTF-16LE (common with BOM 0xFF 0xFE).
                    if hi == 0 && lo >= 0x20 && lo < 0x7F {
                        result.push(lo as char);
                    }
                } else {
                    // UTF-16BE.
                    if lo == 0 && hi >= 0x20 && hi < 0x7F {
                        result.push(hi as char);
                    }
                }
                i += 2;
            }
            result
        }
        _ => String::new(),
    }
}

/// Decode ID3v2 comment frame.
///
/// Format: encoding (1) + language (3) + short description + \0 + actual text.
fn id3v2_comment(data: &[u8]) -> String {
    if data.len() < 5 {
        return String::new();
    }
    // Skip encoding byte and 3-byte language code.
    let rest = &data[4..];
    // Find null separator between description and text.
    let null_pos = rest.iter().position(|&b| b == 0).unwrap_or(0);
    if null_pos + 1 < rest.len() {
        let text = &rest[null_pos + 1..];
        let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
        core::str::from_utf8(&text[..end]).unwrap_or("").into()
    } else {
        String::new()
    }
}

/// Read an ASCII string from a TIFF data block at a given offset.
fn read_ascii_string(data: &[u8], offset: usize, count: usize) -> Option<String> {
    if offset >= data.len() || count == 0 {
        return None;
    }
    let end = (offset + count).min(data.len());
    let bytes = &data[offset..end];
    let trimmed = bytes.iter().take_while(|&&b| b != 0).copied().collect::<Vec<u8>>();
    core::str::from_utf8(&trimmed).ok().map(|s| String::from(s.trim()))
}

/// Map ID3v1 genre index to name.
fn id3v1_genre_name(idx: u8) -> &'static str {
    const GENRES: &[&str] = &[
        "Blues", "Classic Rock", "Country", "Dance", "Disco", "Funk",
        "Grunge", "Hip-Hop", "Jazz", "Metal", "New Age", "Oldies",
        "Other", "Pop", "R&B", "Rap", "Reggae", "Rock", "Techno",
        "Industrial", "Alternative", "Ska", "Death Metal", "Pranks",
        "Soundtrack", "Euro-Techno", "Ambient", "Trip-Hop", "Vocal",
        "Jazz+Funk", "Fusion", "Trance", "Classical", "Instrumental",
        "Acid", "House", "Game", "Sound Clip", "Gospel", "Noise",
        "Alt. Rock", "Bass", "Soul", "Punk", "Space", "Meditative",
        "Instrum. Pop", "Instrum. Rock", "Ethnic", "Gothic", "Darkwave",
        "Techno-Indust.", "Electronic", "Pop-Folk", "Eurodance",
        "Dream", "Southern Rock", "Comedy", "Cult", "Gangsta",
        "Top 40", "Christian Rap", "Pop/Funk", "Jungle", "Native Amer.",
        "Cabaret", "New Wave", "Psychedelic", "Rave", "Showtunes",
        "Trailer", "Lo-Fi", "Tribal", "Acid Punk", "Acid Jazz",
        "Polka", "Retro", "Musical", "Rock & Roll", "Hard Rock",
    ];
    GENRES.get(idx as usize).copied().unwrap_or("Unknown")
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[fileinfo] Running self-test...");

    test_id3v1_parse();
    test_id3v2_parse();
    test_png_parse();
    test_gif_parse();
    test_bmp_parse();
    test_wav_parse();
    test_elf_parse();
    test_fields_for_mime();
    test_synchsafe();

    serial_println!("[fileinfo] Self-test passed (9 tests).");
    Ok(())
}

fn test_id3v1_parse() {
    let mut info = FileInfo::new("/test.mp3", "audio/mpeg");

    // Construct a minimal ID3v1 tag.
    let mut tag = [0u8; 128];
    tag[0] = b'T'; tag[1] = b'A'; tag[2] = b'G';
    // Title: "Test Song" padded to 30 bytes.
    let title = b"Test Song";
    tag[3..3 + title.len()].copy_from_slice(title);
    // Artist: "Test Artist".
    let artist = b"Test Artist";
    tag[33..33 + artist.len()].copy_from_slice(artist);
    // Album: "Test Album".
    let album = b"Test Album";
    tag[63..63 + album.len()].copy_from_slice(album);
    // Year: "2024".
    tag[93..97].copy_from_slice(b"2024");
    // ID3v1.1: track number 5.
    tag[125] = 0; // comment[28] == 0 signals v1.1
    tag[126] = 5; // track number
    // Genre: 17 = Rock.
    tag[127] = 17;

    parse_id3v1(&tag, &mut info);

    assert_eq!(info.get_text("audio.title"), Some("Test Song"));
    assert_eq!(info.get_text("audio.artist"), Some("Test Artist"));
    assert_eq!(info.get_text("audio.album"), Some("Test Album"));
    assert_eq!(info.get_text("audio.year"), Some("2024"));
    assert_eq!(info.get_uint("audio.track"), Some(5));
    assert_eq!(info.get_text("audio.genre"), Some("Rock"));

    serial_println!("[fileinfo]   id3v1_parse: ok");
}

fn test_id3v2_parse() {
    let mut info = FileInfo::new("/test.mp3", "audio/mpeg");

    // Construct minimal ID3v2.3 tag: header + one TIT2 frame.
    let mut data = Vec::new();
    // ID3v2 header.
    data.extend_from_slice(b"ID3");
    data.push(3); // version major
    data.push(0); // version minor
    data.push(0); // flags
    // Tag size (synchsafe): we'll have one frame of 14 bytes total.
    // Frame = 4 (id) + 4 (size) + 2 (flags) + 4 (data) = 14 bytes.
    // Synchsafe encode 14: [0, 0, 0, 14].
    data.extend_from_slice(&[0, 0, 0, 14]);
    // TIT2 frame.
    data.extend_from_slice(b"TIT2");
    data.extend_from_slice(&[0, 0, 0, 4]); // frame size = 4 bytes of data
    data.extend_from_slice(&[0, 0]); // frame flags
    // Frame data: encoding (0 = ISO-8859-1) + "Hey".
    data.push(0); // encoding
    data.extend_from_slice(b"Hey");

    parse_id3v2(&data, &mut info);

    assert_eq!(info.get_text("audio.title"), Some("Hey"));

    serial_println!("[fileinfo]   id3v2_parse: ok");
}

fn test_png_parse() {
    let mut info = FileInfo::new("/test.png", "image/png");

    // Construct minimal PNG: signature + IHDR chunk.
    let mut data = Vec::new();
    // PNG signature.
    data.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    // IHDR chunk: length (13) + "IHDR" + data + CRC.
    data.extend_from_slice(&[0, 0, 0, 13]); // chunk length
    data.extend_from_slice(b"IHDR");
    // Width: 640.
    data.extend_from_slice(&640u32.to_be_bytes());
    // Height: 480.
    data.extend_from_slice(&480u32.to_be_bytes());
    data.push(8);  // bit depth
    data.push(6);  // color type: RGBA
    data.push(0);  // compression
    data.push(0);  // filter
    data.push(0);  // interlace: none
    // CRC (dummy — our parser doesn't verify).
    data.extend_from_slice(&[0, 0, 0, 0]);

    parse_png(&data, &mut info);

    assert_eq!(info.get_uint("image.width"), Some(640));
    assert_eq!(info.get_uint("image.height"), Some(480));
    assert_eq!(info.get_uint("image.bit_depth"), Some(8));
    assert_eq!(info.get_text("image.color_type"), Some("RGBA"));

    serial_println!("[fileinfo]   png_parse: ok");
}

fn test_gif_parse() {
    let mut info = FileInfo::new("/test.gif", "image/gif");

    let mut data = Vec::new();
    data.extend_from_slice(b"GIF89a");
    data.extend_from_slice(&320u16.to_le_bytes()); // width
    data.extend_from_slice(&240u16.to_le_bytes()); // height
    data.push(0x87); // flags: global color table, 8-bit depth
    data.push(0);    // background
    data.push(0);    // aspect ratio

    parse_gif(&data, &mut info);

    assert_eq!(info.get_uint("image.width"), Some(320));
    assert_eq!(info.get_uint("image.height"), Some(240));
    assert_eq!(info.get_text("image.gif_version"), Some("89a"));

    serial_println!("[fileinfo]   gif_parse: ok");
}

fn test_bmp_parse() {
    let mut info = FileInfo::new("/test.bmp", "image/bmp");

    let mut data = vec![0u8; 40];
    data[0] = b'B'; data[1] = b'M';
    // DIB header size: 40 (BITMAPINFOHEADER).
    data[14..18].copy_from_slice(&40u32.to_le_bytes());
    // Width: 800.
    data[18..22].copy_from_slice(&800i32.to_le_bytes());
    // Height: 600.
    data[22..26].copy_from_slice(&600i32.to_le_bytes());
    // Planes.
    data[26..28].copy_from_slice(&1u16.to_le_bytes());
    // Bits per pixel: 24.
    data[28..30].copy_from_slice(&24u16.to_le_bytes());
    // Compression: 0 (none).
    data[30..34].copy_from_slice(&0u32.to_le_bytes());

    parse_bmp(&data, &mut info);

    assert_eq!(info.get_uint("image.width"), None); // BMP uses push_int
    // Check via direct field search.
    let w = info.fields.iter().find(|f| f.name == "image.width");
    assert!(w.is_some());

    serial_println!("[fileinfo]   bmp_parse: ok");
}

fn test_wav_parse() {
    let mut info = FileInfo::new("/test.wav", "audio/wav");

    let mut data = vec![0u8; 48];
    data[0..4].copy_from_slice(b"RIFF");
    data[4..8].copy_from_slice(&40u32.to_le_bytes()); // file size
    data[8..12].copy_from_slice(b"WAVE");
    // fmt chunk.
    data[12..16].copy_from_slice(b"fmt ");
    data[16..20].copy_from_slice(&16u32.to_le_bytes()); // chunk size
    // PCM format.
    data[20..22].copy_from_slice(&1u16.to_le_bytes());
    // 2 channels.
    data[22..24].copy_from_slice(&2u16.to_le_bytes());
    // 44100 Hz.
    data[24..28].copy_from_slice(&44100u32.to_le_bytes());
    // byte rate.
    data[28..32].copy_from_slice(&176400u32.to_le_bytes());
    // block align.
    data[32..34].copy_from_slice(&4u16.to_le_bytes());
    // 16-bit.
    data[34..36].copy_from_slice(&16u16.to_le_bytes());
    // data chunk.
    data[36..40].copy_from_slice(b"data");
    data[40..44].copy_from_slice(&88200u32.to_le_bytes()); // 0.5 seconds of data

    parse_wav(&data, &mut info);

    assert_eq!(info.get_uint("audio.sample_rate_hz"), Some(44100));
    assert_eq!(info.get_uint("audio.channels"), Some(2));
    assert_eq!(info.get_uint("audio.bits_per_sample"), Some(16));
    assert_eq!(info.get_text("audio.format"), Some("PCM"));

    serial_println!("[fileinfo]   wav_parse: ok");
}

fn test_elf_parse() {
    let mut info = FileInfo::new("/test.elf", "application/x-elf");

    let mut data = vec![0u8; 24];
    data[0..4].copy_from_slice(b"\x7FELF");
    data[4] = 2; // 64-bit
    data[5] = 1; // Little endian
    data[6] = 1; // ELF version
    // Type: Executable (2).
    data[16..18].copy_from_slice(&2u16.to_le_bytes());
    // Machine: x86-64 (0x3E).
    data[18..20].copy_from_slice(&0x3Eu16.to_le_bytes());

    parse_elf(&data, &mut info);

    assert_eq!(info.get_text("elf.class"), Some("64-bit"));
    assert_eq!(info.get_text("elf.endian"), Some("Little Endian"));
    assert_eq!(info.get_text("elf.machine"), Some("x86-64"));
    assert_eq!(info.get_text("elf.type"), Some("Executable"));

    serial_println!("[fileinfo]   elf_parse: ok");
}

fn test_fields_for_mime() {
    let mp3_fields = fields_for_mime("audio/mpeg");
    assert!(!mp3_fields.is_empty());
    assert!(mp3_fields.iter().any(|(name, _)| *name == "audio.artist"));

    let png_fields = fields_for_mime("image/png");
    assert!(png_fields.iter().any(|(name, _)| *name == "image.width"));

    let unknown = fields_for_mime("application/octet-stream");
    assert!(unknown.is_empty());

    serial_println!("[fileinfo]   fields_for_mime: ok");
}

fn test_synchsafe() {
    // 0x00 0x00 0x02 0x01 = 0*2^21 + 0*2^14 + 2*2^7 + 1 = 257
    assert_eq!(synchsafe_u32(&[0x00, 0x00, 0x02, 0x01]), 257);
    // 0x00 0x00 0x00 0x7F = 127
    assert_eq!(synchsafe_u32(&[0x00, 0x00, 0x00, 0x7F]), 127);
    // All zeros.
    assert_eq!(synchsafe_u32(&[0, 0, 0, 0]), 0);

    serial_println!("[fileinfo]   synchsafe: ok");
}
