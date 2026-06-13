//! Video playback engine for the Slate OS Image Viewer.
//!
//! Supports common container formats (AVI, MP4/MOV, MKV/WebM) with a
//! full-featured player state machine, playback controls UI, and keyboard
//! shortcuts.  Actual codec decoding is deferred to a future hardware-
//! accelerated decoder service; this module handles container parsing,
//! player state, control rendering, and frame presentation.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MOCHA_BASE: Color = Color::rgb(30, 30, 46);
const MOCHA_MANTLE: Color = Color::rgb(24, 24, 37);
const MOCHA_CRUST: Color = Color::rgb(17, 17, 27);
const MOCHA_SURFACE0: Color = Color::rgb(49, 50, 68);
const MOCHA_SURFACE1: Color = Color::rgb(69, 71, 90);
const MOCHA_SURFACE2: Color = Color::rgb(88, 91, 112);
const MOCHA_OVERLAY0: Color = Color::rgb(108, 112, 134);
const MOCHA_TEXT: Color = Color::rgb(205, 214, 244);
const MOCHA_SUBTEXT0: Color = Color::rgb(166, 173, 200);
const MOCHA_BLUE: Color = Color::rgb(137, 180, 250);
const MOCHA_GREEN: Color = Color::rgb(166, 227, 161);
const MOCHA_RED: Color = Color::rgb(243, 139, 168);
const MOCHA_YELLOW: Color = Color::rgb(249, 226, 175);
const MOCHA_MAUVE: Color = Color::rgb(203, 166, 247);
const MOCHA_PEACH: Color = Color::rgb(250, 179, 135);

// ============================================================================
// Layout constants
// ============================================================================

const CONTROL_BAR_HEIGHT: f32 = 72.0;
const SEEK_BAR_HEIGHT: f32 = 6.0;
const SEEK_BAR_HIT_HEIGHT: f32 = 20.0;
const CONTROL_BUTTON_SIZE: f32 = 32.0;
const VOLUME_SLIDER_WIDTH: f32 = 80.0;

const MIN_VOLUME: f32 = 0.0;
const MAX_VOLUME: f32 = 1.0;
const VOLUME_STEP: f32 = 0.05;

const MIN_SPEED: f32 = 0.25;
const MAX_SPEED: f32 = 4.0;

const SEEK_SMALL_MS: u64 = 5_000;
const SEEK_LARGE_MS: u64 = 30_000;

const SPEED_OPTIONS: &[f32] = &[0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

// ============================================================================
// Error type
// ============================================================================

/// Errors that can occur during video operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoError {
    /// The file could not be read.
    IoError(String),
    /// The container format is not recognised.
    UnsupportedFormat,
    /// The container is corrupt or truncated.
    ParseError(String),
    /// The codec inside the container is not supported.
    UnsupportedCodec(String),
    /// A seek target is out of range.
    SeekOutOfRange,
}

impl core::fmt::Display for VideoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
            Self::UnsupportedFormat => write!(f, "unsupported container format"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::UnsupportedCodec(codec) => write!(f, "unsupported codec: {codec}"),
            Self::SeekOutOfRange => write!(f, "seek position out of range"),
        }
    }
}

// ============================================================================
// Container format detection
// ============================================================================

/// Recognised video container format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerFormat {
    Avi,
    Mp4,
    Mkv,
    WebM,
}

impl ContainerFormat {
    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Avi => "AVI",
            Self::Mp4 => "MP4",
            Self::Mkv => "Matroska",
            Self::WebM => "WebM",
        }
    }

    /// Detect the container format from the first bytes of a file.
    ///
    /// Requires at least 12 bytes for reliable detection.
    pub fn detect(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        // AVI: RIFF....AVI  (bytes 0-3 = "RIFF", bytes 8-11 = "AVI ")
        if data.get(..4) == Some(b"RIFF") && data.get(8..12) == Some(b"AVI ") {
            return Some(Self::Avi);
        }

        // MP4/MOV: ftyp box (bytes 4-7 = "ftyp")
        if data.get(4..8) == Some(b"ftyp") {
            return Some(Self::Mp4);
        }

        // MKV/WebM: EBML header (0x1A 0x45 0xDF 0xA3)
        if data.get(..4) == Some(&[0x1A, 0x45, 0xDF, 0xA3]) {
            // Distinguish WebM from MKV by scanning for the DocType element.
            // WebM's DocType is "webm", Matroska's is "matroska".  We do a
            // simple byte scan over the first 64 bytes (the EBML header is
            // typically short).
            let scan_len = data.len().min(64);
            let haystack = &data[..scan_len];
            if contains_subsequence(haystack, b"webm") {
                return Some(Self::WebM);
            }
            return Some(Self::Mkv);
        }

        None
    }
}

/// Helper: check whether `haystack` contains `needle` as a contiguous
/// subsequence.
fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    for window in haystack.windows(needle.len()) {
        if window == needle {
            return true;
        }
    }
    false
}

// ============================================================================
// AVI container parsing
// ============================================================================

/// Parsed AVI header information.
#[derive(Clone, Debug)]
pub struct AviHeader {
    pub microseconds_per_frame: u32,
    pub width: u32,
    pub height: u32,
    pub total_frames: u32,
    /// FourCC codec identifier (e.g. "H264", "XVID").
    pub codec_fourcc: [u8; 4],
}

impl AviHeader {
    /// Frame rate derived from the micro-seconds-per-frame field.
    pub fn frame_rate(&self) -> f64 {
        if self.microseconds_per_frame == 0 {
            return 0.0;
        }
        1_000_000.0 / self.microseconds_per_frame as f64
    }

    /// Estimated duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        if self.microseconds_per_frame == 0 {
            return 0;
        }
        (self.total_frames as u64).saturating_mul(self.microseconds_per_frame as u64) / 1_000
    }
}

/// Parse an AVI container header from raw file data.
///
/// The minimum viable AVI file is at least 32 bytes for the RIFF header
/// plus the `avih` chunk embedded inside `hdrl`.  In practice we need at
/// least ~80 bytes.
pub fn parse_avi_header(data: &[u8]) -> Result<AviHeader, VideoError> {
    // Validate RIFF/AVI signature (already checked by ContainerFormat::detect
    // but we guard here for direct callers).
    if data.len() < 12 || data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"AVI ") {
        return Err(VideoError::ParseError("not a valid AVI file".into()));
    }

    // Scan for the `avih` chunk inside the file.  The `avih` chunk is
    // located inside the `hdrl` LIST but we just do a flat scan — it is
    // always close to the start of the file.
    let avih_pos = find_chunk_id(data, b"avih")
        .ok_or_else(|| VideoError::ParseError("avih chunk not found".into()))?;

    // avih chunk layout (after 8-byte chunk header):
    //   0..4   dwMicroSecPerFrame
    //   16..20 dwTotalFrames
    //   32..36 dwWidth
    //   36..40 dwHeight
    let chunk_data_start = avih_pos
        .checked_add(8)
        .ok_or_else(|| VideoError::ParseError("avih offset overflow".into()))?;

    let read_u32 = |offset: usize| -> Result<u32, VideoError> {
        let start = chunk_data_start
            .checked_add(offset)
            .ok_or_else(|| VideoError::ParseError("offset overflow".into()))?;
        let end = start
            .checked_add(4)
            .ok_or_else(|| VideoError::ParseError("offset overflow".into()))?;
        if end > data.len() {
            return Err(VideoError::ParseError("avih chunk truncated".into()));
        }
        let bytes: [u8; 4] = [
            data[start],
            data[start + 1],
            data[start + 2],
            data[start + 3],
        ];
        Ok(u32::from_le_bytes(bytes))
    };

    let microseconds_per_frame = read_u32(0)?;
    let total_frames = read_u32(16)?;
    let width = read_u32(32)?;
    let height = read_u32(36)?;

    // Try to find `strh` to extract the codec FourCC.
    let codec_fourcc = if let Some(strh_pos) = find_chunk_id(data, b"strh") {
        // strh layout: fccType(4) + fccHandler(4) at start of chunk data.
        let handler_start = strh_pos
            .checked_add(8 + 4)
            .ok_or_else(|| VideoError::ParseError("strh offset overflow".into()))?;
        let handler_end = handler_start
            .checked_add(4)
            .ok_or_else(|| VideoError::ParseError("strh offset overflow".into()))?;
        if handler_end <= data.len() {
            [
                data[handler_start],
                data[handler_start + 1],
                data[handler_start + 2],
                data[handler_start + 3],
            ]
        } else {
            [0; 4]
        }
    } else {
        [0; 4]
    };

    Ok(AviHeader {
        microseconds_per_frame,
        width,
        height,
        total_frames,
        codec_fourcc,
    })
}

/// Scan `data` for a 4-byte chunk ID and return its offset.
fn find_chunk_id(data: &[u8], id: &[u8; 4]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    for i in 0..data.len() - 3 {
        if &data[i..i + 4] == id {
            return Some(i);
        }
    }
    None
}

// ============================================================================
// MP4 container parsing
// ============================================================================

/// A single MP4 box (atom) parsed from the file.
#[derive(Clone, Debug)]
pub struct Mp4Box {
    /// Four-character box type (e.g. "ftyp", "moov", "mdat").
    pub box_type: [u8; 4],
    /// Offset in the file where this box starts (including header).
    pub offset: u64,
    /// Total size of the box (header + payload).
    pub size: u64,
}

/// Parsed MP4 metadata extracted from the moov box tree.
#[derive(Clone, Debug)]
pub struct Mp4Info {
    pub duration_ms: u64,
    pub timescale: u32,
    pub width: u32,
    pub height: u32,
    /// Codec FourCC from the sample entry (e.g. "avc1", "hvc1", "mp4a").
    pub video_codec: Option<[u8; 4]>,
    pub audio_codec: Option<[u8; 4]>,
    /// Brand from ftyp.
    pub major_brand: [u8; 4],
}

/// Parse top-level boxes from an MP4 file.
///
/// Returns a list of `(box_type, offset, size)` for each top-level box.
/// Does not recurse into container boxes — callers can do that by slicing
/// the data and calling `parse_mp4_boxes` on the payload.
pub fn parse_mp4_boxes(data: &[u8]) -> Result<Vec<Mp4Box>, VideoError> {
    let mut boxes = Vec::new();
    let mut pos: usize = 0;

    while pos + 8 <= data.len() {
        let size_bytes: [u8; 4] = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
        let raw_size = u32::from_be_bytes(size_bytes) as u64;

        let box_type: [u8; 4] = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];

        let (header_len, box_size) = if raw_size == 1 {
            // 64-bit extended size
            if pos + 16 > data.len() {
                break;
            }
            let ext_bytes: [u8; 8] = [
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
            ];
            (16usize, u64::from_be_bytes(ext_bytes))
        } else if raw_size == 0 {
            // Box extends to end of data.
            let remaining = (data.len() - pos) as u64;
            (8usize, remaining)
        } else {
            (8usize, raw_size)
        };

        if box_size < header_len as u64 {
            return Err(VideoError::ParseError("invalid MP4 box size".into()));
        }

        boxes.push(Mp4Box {
            box_type,
            offset: pos as u64,
            size: box_size,
        });

        let next_pos = pos.checked_add(box_size as usize);
        match next_pos {
            Some(np) if np > pos => pos = np,
            _ => break,
        }
    }

    Ok(boxes)
}

/// Extract high-level metadata from an MP4 file.
///
/// This walks the moov/trak/mdia/minf/stbl tree to find duration,
/// dimensions, and codec information.
pub fn parse_mp4_info(data: &[u8]) -> Result<Mp4Info, VideoError> {
    let top_boxes = parse_mp4_boxes(data)?;

    // Extract major brand from ftyp.
    let major_brand = top_boxes
        .iter()
        .find(|b| &b.box_type == b"ftyp")
        .and_then(|b| {
            let start = b.offset as usize + 8;
            if start + 4 <= data.len() {
                Some([
                    data[start],
                    data[start + 1],
                    data[start + 2],
                    data[start + 3],
                ])
            } else {
                None
            }
        })
        .unwrap_or([0; 4]);

    // Find moov box.
    let moov = top_boxes
        .iter()
        .find(|b| &b.box_type == b"moov")
        .ok_or(VideoError::ParseError("moov box not found".into()))?;

    let moov_start = (moov.offset as usize)
        .checked_add(8)
        .ok_or_else(|| VideoError::ParseError("moov offset overflow".into()))?;
    let moov_end = (moov.offset as usize)
        .checked_add(moov.size as usize)
        .ok_or_else(|| VideoError::ParseError("moov size overflow".into()))?
        .min(data.len());

    if moov_start >= moov_end {
        return Err(VideoError::ParseError("moov box empty".into()));
    }

    let moov_data = &data[moov_start..moov_end];

    // Parse mvhd for timescale and duration.
    let (timescale, duration_units) = parse_mvhd(moov_data)?;
    let duration_ms = if timescale > 0 {
        duration_units.saturating_mul(1000) / timescale as u64
    } else {
        0
    };

    // Walk trak boxes for video/audio info.
    let moov_children = parse_mp4_boxes(moov_data)?;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut video_codec: Option<[u8; 4]> = None;
    let mut audio_codec: Option<[u8; 4]> = None;

    for trak in moov_children.iter().filter(|b| &b.box_type == b"trak") {
        let trak_start = (trak.offset as usize)
            .checked_add(8)
            .unwrap_or(moov_data.len());
        let trak_end = (trak.offset as usize)
            .checked_add(trak.size as usize)
            .unwrap_or(moov_data.len())
            .min(moov_data.len());
        if trak_start >= trak_end {
            continue;
        }
        let trak_data = &moov_data[trak_start..trak_end];

        // Look for tkhd to get width/height.
        if let Some((w, h)) = parse_tkhd_dimensions(trak_data)
            && w > 0 && h > 0 && width == 0 {
                width = w;
                height = h;
            }

        // Recurse into mdia/minf/stbl/stsd for codec info.
        if let Some(codec) = extract_stsd_codec(trak_data) {
            // Heuristic: video codecs are avc1, hvc1, vp09, av01, mp4v etc.
            // Audio codecs are mp4a, ac-3, ec-3, Opus, etc.
            let is_video = matches!(
                &codec,
                b"avc1" | b"avc3" | b"hvc1" | b"hev1" | b"vp08" | b"vp09" | b"av01" | b"mp4v"
            );
            if is_video && video_codec.is_none() {
                video_codec = Some(codec);
            } else if !is_video && audio_codec.is_none() {
                audio_codec = Some(codec);
            }
        }
    }

    Ok(Mp4Info {
        duration_ms,
        timescale,
        width,
        height,
        video_codec,
        audio_codec,
        major_brand,
    })
}

/// Parse the `mvhd` box to extract timescale and duration.
fn parse_mvhd(moov_data: &[u8]) -> Result<(u32, u64), VideoError> {
    let children = parse_mp4_boxes(moov_data)?;
    let mvhd = children
        .iter()
        .find(|b| &b.box_type == b"mvhd")
        .ok_or(VideoError::ParseError("mvhd box not found".into()))?;

    let start = (mvhd.offset as usize)
        .checked_add(8)
        .ok_or_else(|| VideoError::ParseError("mvhd offset overflow".into()))?;

    if start >= moov_data.len() {
        return Err(VideoError::ParseError("mvhd truncated".into()));
    }

    let mvhd_data = &moov_data[start..];
    if mvhd_data.is_empty() {
        return Err(VideoError::ParseError("mvhd empty".into()));
    }

    let version = mvhd_data[0];
    if version == 0 {
        // Version 0: 4 bytes each for create/modify dates, then timescale(4), duration(4)
        if mvhd_data.len() < 4 + 4 + 4 + 4 + 4 {
            return Err(VideoError::ParseError("mvhd v0 truncated".into()));
        }
        let timescale = read_be_u32(mvhd_data, 12)?;
        let duration = read_be_u32(mvhd_data, 16)? as u64;
        Ok((timescale, duration))
    } else {
        // Version 1: 8 bytes each for create/modify dates, then timescale(4), duration(8)
        if mvhd_data.len() < 4 + 8 + 8 + 4 + 8 {
            return Err(VideoError::ParseError("mvhd v1 truncated".into()));
        }
        let timescale = read_be_u32(mvhd_data, 20)?;
        let duration = read_be_u64(mvhd_data, 24)?;
        Ok((timescale, duration))
    }
}

/// Parse the `tkhd` box inside a trak to get width and height.
/// Width/height are stored as 16.16 fixed-point at specific offsets.
fn parse_tkhd_dimensions(trak_data: &[u8]) -> Option<(u32, u32)> {
    let children = parse_mp4_boxes(trak_data).ok()?;
    let tkhd = children.iter().find(|b| &b.box_type == b"tkhd")?;
    let start = (tkhd.offset as usize).checked_add(8)?;
    if start >= trak_data.len() {
        return None;
    }
    let d = &trak_data[start..];
    let version = *d.first()?;

    // Width/height are at the very end of tkhd, as 16.16 fixed-point values.
    // Version 0: total full-box size = 84 bytes of payload
    //   -> width at offset 76, height at offset 80
    // Version 1: total full-box size = 96 bytes of payload
    //   -> width at offset 88, height at offset 92
    let (w_off, h_off) = if version == 0 { (76, 80) } else { (88, 92) };
    let w_fixed = read_be_u32(d, w_off).ok()?;
    let h_fixed = read_be_u32(d, h_off).ok()?;
    // 16.16 fixed-point: top 16 bits are the integer part.
    Some((w_fixed >> 16, h_fixed >> 16))
}

/// Descend through mdia -> minf -> stbl -> stsd to extract the codec FourCC.
fn extract_stsd_codec(trak_data: &[u8]) -> Option<[u8; 4]> {
    let trak_children = parse_mp4_boxes(trak_data).ok()?;
    let mdia = trak_children.iter().find(|b| &b.box_type == b"mdia")?;
    let mdia_start = (mdia.offset as usize).checked_add(8)?;
    let mdia_end = (mdia.offset as usize)
        .checked_add(mdia.size as usize)?
        .min(trak_data.len());
    if mdia_start >= mdia_end {
        return None;
    }
    let mdia_data = &trak_data[mdia_start..mdia_end];

    let mdia_children = parse_mp4_boxes(mdia_data).ok()?;
    let minf = mdia_children.iter().find(|b| &b.box_type == b"minf")?;
    let minf_start = (minf.offset as usize).checked_add(8)?;
    let minf_end = (minf.offset as usize)
        .checked_add(minf.size as usize)?
        .min(mdia_data.len());
    if minf_start >= minf_end {
        return None;
    }
    let minf_data = &mdia_data[minf_start..minf_end];

    let minf_children = parse_mp4_boxes(minf_data).ok()?;
    let stbl = minf_children.iter().find(|b| &b.box_type == b"stbl")?;
    let stbl_start = (stbl.offset as usize).checked_add(8)?;
    let stbl_end = (stbl.offset as usize)
        .checked_add(stbl.size as usize)?
        .min(minf_data.len());
    if stbl_start >= stbl_end {
        return None;
    }
    let stbl_data = &minf_data[stbl_start..stbl_end];

    let stbl_children = parse_mp4_boxes(stbl_data).ok()?;
    let stsd = stbl_children.iter().find(|b| &b.box_type == b"stsd")?;
    // stsd is a full box: 4 bytes version/flags + 4 bytes entry_count, then
    // the first sample entry whose box type is the codec FourCC.
    let stsd_payload_start = (stsd.offset as usize).checked_add(8 + 8)?;
    if stsd_payload_start + 4 > stbl_data.len() {
        return None;
    }
    // The sample entry begins here; its box type (at +4..+8) is the codec.
    let entry_start = stsd_payload_start;
    if entry_start + 8 > stbl_data.len() {
        return None;
    }
    Some([
        stbl_data[entry_start + 4],
        stbl_data[entry_start + 5],
        stbl_data[entry_start + 6],
        stbl_data[entry_start + 7],
    ])
}

// ============================================================================
// MKV / WebM (EBML) container parsing
// ============================================================================

/// Parsed Matroska/WebM header information.
#[derive(Clone, Debug)]
pub struct MkvInfo {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub codec_id: String,
    pub doc_type: String,
}

/// Parse basic info from an EBML (Matroska/WebM) container.
///
/// This does a lightweight scan of the EBML header and the first Segment
/// Info and Track elements.  Full EBML VINT decoding is out of scope for
/// the initial implementation; we use heuristic scanning.
pub fn parse_mkv_info(data: &[u8]) -> Result<MkvInfo, VideoError> {
    if data.len() < 4 || data.get(..4) != Some(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Err(VideoError::ParseError("not a valid EBML file".into()));
    }

    // Extract DocType from the EBML header.
    let doc_type = extract_ebml_string(data, 0x4282).unwrap_or_else(|| String::from("matroska"));

    // Scan for the Segment Info element (ID 0x1549_A966 as 4-byte EBML ID).
    // Inside it, look for Duration (ID 0x4489) and TimecodeScale (ID 0x2AD7B1).
    let duration_ms = extract_ebml_duration(data).unwrap_or(0);

    // Scan for first video track to get dimensions and codec.
    let (width, height, codec_id) = extract_ebml_video_track(data);

    Ok(MkvInfo {
        duration_ms,
        width,
        height,
        codec_id,
        doc_type,
    })
}

/// Scan for an EBML string element with the given (1- or 2-byte) element ID.
fn extract_ebml_string(data: &[u8], element_id: u16) -> Option<String> {
    // Encode the element ID as 2 bytes (big-endian) and scan.
    let id_bytes = element_id.to_be_bytes();
    let scan_limit = data.len().min(512);
    for i in 0..scan_limit.saturating_sub(3) {
        if data[i] == id_bytes[0] && data[i + 1] == id_bytes[1] {
            // Next byte(s) is the VINT-encoded length.
            let (len, len_size) = decode_ebml_vint(&data[i + 2..])?;
            let start = i + 2 + len_size;
            let end = start + len as usize;
            if end <= data.len() {
                return String::from_utf8(data[start..end].to_vec()).ok();
            }
        }
    }
    None
}

/// Extract duration in milliseconds from the Segment Info element.
fn extract_ebml_duration(data: &[u8]) -> Option<u64> {
    // TimecodeScale element ID: 0x2AD7B1 (3 bytes)
    // Duration element ID: 0x4489 (2 bytes)

    let scan_limit = data.len().min(4096);
    let mut timecode_scale: u64 = 1_000_000; // default 1ms

    // Look for TimecodeScale (0x2AD7B1).
    for i in 0..scan_limit.saturating_sub(6) {
        if data[i] == 0x2A && data[i + 1] == 0xD7 && data[i + 2] == 0xB1 {
            let (len, len_size) = decode_ebml_vint(&data[i + 3..])?;
            let start = i + 3 + len_size;
            let end = start + len as usize;
            if end <= data.len() && len <= 8 {
                timecode_scale = read_ebml_uint(&data[start..end]);
            }
            break;
        }
    }

    // Look for Duration (0x4489) — a float (4 or 8 bytes).
    for i in 0..scan_limit.saturating_sub(4) {
        if data[i] == 0x44 && data[i + 1] == 0x89 {
            let (len, len_size) = decode_ebml_vint(&data[i + 2..])?;
            let start = i + 2 + len_size;
            let end = start + len as usize;
            if end <= data.len() {
                let duration_ns = if len == 4 {
                    let bytes: [u8; 4] = [
                        data[start],
                        data[start + 1],
                        data[start + 2],
                        data[start + 3],
                    ];
                    f32::from_be_bytes(bytes) as f64
                } else if len == 8 {
                    let bytes: [u8; 8] = [
                        data[start],
                        data[start + 1],
                        data[start + 2],
                        data[start + 3],
                        data[start + 4],
                        data[start + 5],
                        data[start + 6],
                        data[start + 7],
                    ];
                    f64::from_be_bytes(bytes)
                } else {
                    return None;
                };
                // Duration is in timecode_scale units.
                let total_ns = duration_ns * timecode_scale as f64;
                return Some((total_ns / 1_000_000.0) as u64);
            }
        }
    }

    None
}

/// Extract video track dimensions and codec ID from the EBML data.
fn extract_ebml_video_track(data: &[u8]) -> (u32, u32, String) {
    let scan_limit = data.len().min(8192);
    let mut width = 0u32;
    let mut height = 0u32;
    let mut codec_id = String::new();

    // CodecID element ID: 0x86 (1 byte)
    for i in 0..scan_limit.saturating_sub(3) {
        if data[i] == 0x86
            && let Some((len, len_size)) = decode_ebml_vint(&data[i + 1..]) {
                let start = i + 1 + len_size;
                let end = start + len as usize;
                if end <= data.len() && len < 64
                    && let Ok(s) = core::str::from_utf8(&data[start..end])
                    && (s.starts_with("V_") || s.starts_with("A_"))
                    && codec_id.is_empty() && s.starts_with("V_") {
                        codec_id = s.to_string();
                    }
            }
    }

    // PixelWidth element ID: 0xB0 (1 byte)
    for i in 0..scan_limit.saturating_sub(3) {
        if data[i] == 0xB0
            && let Some((len, len_size)) = decode_ebml_vint(&data[i + 1..]) {
                let start = i + 1 + len_size;
                let end = start + len as usize;
                if end <= data.len() && len <= 4 {
                    width = read_ebml_uint(&data[start..end]) as u32;
                    if width > 0 {
                        break;
                    }
                }
            }
    }

    // PixelHeight element ID: 0xBA (1 byte)
    for i in 0..scan_limit.saturating_sub(3) {
        if data[i] == 0xBA
            && let Some((len, len_size)) = decode_ebml_vint(&data[i + 1..]) {
                let start = i + 1 + len_size;
                let end = start + len as usize;
                if end <= data.len() && len <= 4 {
                    height = read_ebml_uint(&data[start..end]) as u32;
                    if height > 0 {
                        break;
                    }
                }
            }
    }

    (width, height, codec_id)
}

/// Decode an EBML variable-length integer (VINT).
/// Returns `(value, number_of_bytes_consumed)`.
fn decode_ebml_vint(data: &[u8]) -> Option<(u64, usize)> {
    let first = *data.first()?;
    if first == 0 {
        return None;
    }

    let len = first.leading_zeros() as usize + 1;
    if len > 8 || data.len() < len {
        return None;
    }

    let mask = (1u8 << (8 - len)) - 1;
    let mut value = (first & mask) as u64;
    for byte in &data[1..len] {
        value = (value << 8) | (*byte as u64);
    }

    Some((value, len))
}

/// Read a big-endian unsigned integer of 1-8 bytes from a slice.
fn read_ebml_uint(data: &[u8]) -> u64 {
    let mut value = 0u64;
    for &byte in data {
        value = (value << 8) | byte as u64;
    }
    value
}

// ============================================================================
// Binary reading helpers
// ============================================================================

fn read_be_u32(data: &[u8], offset: usize) -> Result<u32, VideoError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| VideoError::ParseError("offset overflow".into()))?;
    if end > data.len() {
        return Err(VideoError::ParseError("data truncated".into()));
    }
    Ok(u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

fn read_be_u64(data: &[u8], offset: usize) -> Result<u64, VideoError> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| VideoError::ParseError("offset overflow".into()))?;
    if end > data.len() {
        return Err(VideoError::ParseError("data truncated".into()));
    }
    Ok(u64::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

// ============================================================================
// Track / stream metadata
// ============================================================================

/// Description of an audio track inside a video file.
#[derive(Clone, Debug)]
pub struct AudioTrackInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub index: usize,
}

/// Description of a subtitle track inside a video file.
#[derive(Clone, Debug)]
pub struct SubtitleTrackInfo {
    pub language: String,
    pub format: String,
    pub index: usize,
}

/// Aggregated metadata returned when a video file is opened.
#[derive(Clone, Debug)]
pub struct VideoInfo {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub codec_name: String,
    pub container: ContainerFormat,
    pub audio_tracks: Vec<AudioTrackInfo>,
    pub subtitle_tracks: Vec<SubtitleTrackInfo>,
}

// ============================================================================
// Player state machine
// ============================================================================

/// Current state of the video player.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
    Buffering,
    Error(String),
}

/// Loop mode for the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    Off,
    Single,
    Playlist,
}

impl LoopMode {
    /// Cycle to the next loop mode.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Single,
            Self::Single => Self::Playlist,
            Self::Playlist => Self::Off,
        }
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Single => "Repeat One",
            Self::Playlist => "Repeat All",
        }
    }
}

/// The main video player struct.
///
/// Manages playback state, timing, volume, speed, and UI interaction.
/// Actual frame decoding is delegated to a codec service (not yet
/// implemented); this struct drives the state machine and presentation.
pub struct VideoPlayer {
    // -- State --
    state: PlayerState,
    video_info: Option<VideoInfo>,

    // -- Timing --
    position_ms: u64,
    duration_ms: u64,

    // -- Playback controls --
    volume: f32,
    muted: bool,
    playback_speed: f32,

    // -- Loop / playlist --
    loop_mode: LoopMode,
    playlist: Vec<String>,
    playlist_index: usize,

    // -- Subtitles --
    subtitles_enabled: bool,
    active_subtitle_track: usize,

    // -- UI state --
    fullscreen: bool,
    controls_visible: bool,
    controls_hide_timer_ms: u64,

    // -- Buffering indicator --
    buffered_ms: u64,

    // -- Frame --
    current_frame_id: u64,
    frame_width: u32,
    frame_height: u32,
}

impl VideoPlayer {
    /// Create a new video player in the stopped state.
    pub fn new() -> Self {
        Self {
            state: PlayerState::Stopped,
            video_info: None,
            position_ms: 0,
            duration_ms: 0,
            volume: 0.75,
            muted: false,
            playback_speed: 1.0,
            loop_mode: LoopMode::Off,
            playlist: Vec::new(),
            playlist_index: 0,
            subtitles_enabled: false,
            active_subtitle_track: 0,
            fullscreen: false,
            controls_visible: true,
            controls_hide_timer_ms: 0,
            buffered_ms: 0,
            current_frame_id: 0,
            frame_width: 0,
            frame_height: 0,
        }
    }

    /// Open a video file and extract its metadata.
    ///
    /// On success the player enters `Stopped` state with metadata populated.
    pub fn open(&mut self, path: &str) -> Result<VideoInfo, VideoError> {
        let data = std::fs::read(path).map_err(|e| VideoError::IoError(e.to_string()))?;

        let container = ContainerFormat::detect(&data).ok_or(VideoError::UnsupportedFormat)?;

        let info = match container {
            ContainerFormat::Avi => {
                let hdr = parse_avi_header(&data)?;
                let fourcc_str = String::from_utf8_lossy(&hdr.codec_fourcc).to_string();
                VideoInfo {
                    duration_ms: hdr.duration_ms(),
                    width: hdr.width,
                    height: hdr.height,
                    frame_rate: hdr.frame_rate(),
                    codec_name: fourcc_str,
                    container,
                    audio_tracks: Vec::new(),
                    subtitle_tracks: Vec::new(),
                }
            }
            ContainerFormat::Mp4 => {
                let mp4 = parse_mp4_info(&data)?;
                let codec_str = mp4
                    .video_codec
                    .map(|c| String::from_utf8_lossy(&c).to_string())
                    .unwrap_or_else(|| String::from("unknown"));
                VideoInfo {
                    duration_ms: mp4.duration_ms,
                    width: mp4.width,
                    height: mp4.height,
                    frame_rate: 0.0, // would need stts to compute
                    codec_name: codec_str,
                    container,
                    audio_tracks: Vec::new(),
                    subtitle_tracks: Vec::new(),
                }
            }
            ContainerFormat::Mkv | ContainerFormat::WebM => {
                let mkv = parse_mkv_info(&data)?;
                VideoInfo {
                    duration_ms: mkv.duration_ms,
                    width: mkv.width,
                    height: mkv.height,
                    frame_rate: 0.0, // would need DefaultDuration element
                    codec_name: mkv.codec_id,
                    container,
                    audio_tracks: Vec::new(),
                    subtitle_tracks: Vec::new(),
                }
            }
        };

        self.duration_ms = info.duration_ms;
        self.frame_width = info.width;
        self.frame_height = info.height;
        self.position_ms = 0;
        self.state = PlayerState::Stopped;
        self.video_info = Some(info.clone());
        Ok(info)
    }

    // -- Playback controls ------------------------------------------------

    /// Start or resume playback.
    pub fn play(&mut self) {
        match &self.state {
            PlayerState::Stopped | PlayerState::Paused => {
                self.state = PlayerState::Playing;
            }
            _ => {}
        }
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            self.state = PlayerState::Paused;
        }
    }

    /// Stop playback and reset position to zero.
    pub fn stop(&mut self) {
        self.state = PlayerState::Stopped;
        self.position_ms = 0;
    }

    /// Toggle between play and pause.
    pub fn toggle_play_pause(&mut self) {
        match &self.state {
            PlayerState::Playing => self.pause(),
            PlayerState::Paused | PlayerState::Stopped => self.play(),
            _ => {}
        }
    }

    /// Seek to an absolute position in milliseconds.
    /// The position is clamped to `[0, duration_ms]`.
    pub fn seek(&mut self, position_ms: u64) {
        self.position_ms = position_ms.min(self.duration_ms);
    }

    /// Seek forward by `delta_ms` milliseconds.
    pub fn seek_forward(&mut self, delta_ms: u64) {
        self.seek(self.position_ms.saturating_add(delta_ms));
    }

    /// Seek backward by `delta_ms` milliseconds.
    pub fn seek_backward(&mut self, delta_ms: u64) {
        self.seek(self.position_ms.saturating_sub(delta_ms));
    }

    // -- Volume -----------------------------------------------------------

    /// Set volume (clamped to 0.0..=1.0).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(MIN_VOLUME, MAX_VOLUME);
    }

    /// Increase volume by one step.
    pub fn volume_up(&mut self) {
        self.set_volume(self.volume + VOLUME_STEP);
    }

    /// Decrease volume by one step.
    pub fn volume_down(&mut self) {
        self.set_volume(self.volume - VOLUME_STEP);
    }

    /// Toggle mute.
    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    /// Effective volume accounting for mute.
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }

    // -- Playback speed ---------------------------------------------------

    /// Set playback speed (clamped to 0.25..=4.0).
    pub fn set_playback_speed(&mut self, speed: f32) {
        self.playback_speed = speed.clamp(MIN_SPEED, MAX_SPEED);
    }

    /// Increase playback speed to the next preset.
    pub fn speed_up(&mut self) {
        for &s in SPEED_OPTIONS {
            if s > self.playback_speed + 0.01 {
                self.playback_speed = s;
                return;
            }
        }
        // Already at or above the maximum preset.
    }

    /// Decrease playback speed to the previous preset.
    pub fn speed_down(&mut self) {
        for &s in SPEED_OPTIONS.iter().rev() {
            if s < self.playback_speed - 0.01 {
                self.playback_speed = s;
                return;
            }
        }
        // Already at or below the minimum preset.
    }

    // -- Loop / playlist --------------------------------------------------

    /// Toggle loop mode.
    pub fn toggle_loop(&mut self) {
        self.loop_mode = self.loop_mode.next();
    }

    /// Set the playlist of video file paths.
    pub fn set_playlist(&mut self, paths: Vec<String>) {
        self.playlist = paths;
        self.playlist_index = 0;
    }

    /// Move to the next item in the playlist.
    pub fn next_in_playlist(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        self.playlist_index = (self.playlist_index + 1) % self.playlist.len();
    }

    /// Move to the previous item in the playlist.
    pub fn prev_in_playlist(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        if self.playlist_index == 0 {
            self.playlist_index = self.playlist.len().saturating_sub(1);
        } else {
            self.playlist_index -= 1;
        }
    }

    // -- Subtitles --------------------------------------------------------

    /// Toggle subtitle display.
    pub fn toggle_subtitles(&mut self) {
        self.subtitles_enabled = !self.subtitles_enabled;
    }

    // -- Fullscreen -------------------------------------------------------

    /// Toggle fullscreen mode.
    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
    }

    // -- Query accessors --------------------------------------------------

    pub fn state(&self) -> &PlayerState {
        &self.state
    }

    pub fn current_position_ms(&self) -> u64 {
        self.position_ms
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.frame_width, self.frame_height)
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn playback_speed(&self) -> f32 {
        self.playback_speed
    }

    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    pub fn subtitles_enabled(&self) -> bool {
        self.subtitles_enabled
    }

    pub fn video_info(&self) -> Option<&VideoInfo> {
        self.video_info.as_ref()
    }

    // -- Tick / update ----------------------------------------------------

    /// Advance playback by `elapsed_ms` (in real time).
    ///
    /// This should be called every frame/tick. The position is advanced by
    /// `elapsed_ms * playback_speed`, clamped to the duration.  When the
    /// end is reached the loop mode determines the next action.
    pub fn tick(&mut self, elapsed_ms: u64) {
        if self.state != PlayerState::Playing {
            return;
        }

        // Auto-hide controls after 3 seconds of inactivity while playing.
        self.controls_hide_timer_ms = self.controls_hide_timer_ms.saturating_add(elapsed_ms);
        if self.controls_hide_timer_ms > 3000 {
            self.controls_visible = false;
        }

        let advance = (elapsed_ms as f64 * self.playback_speed as f64) as u64;
        self.position_ms = self.position_ms.saturating_add(advance);

        if self.position_ms >= self.duration_ms && self.duration_ms > 0 {
            self.position_ms = self.duration_ms;
            match self.loop_mode {
                LoopMode::Single => {
                    self.position_ms = 0;
                    // Stay playing — loops back to start.
                }
                LoopMode::Playlist => {
                    self.next_in_playlist();
                    self.position_ms = 0;
                    // In a real implementation we would open the next file
                    // here.  For now just reset position.
                }
                LoopMode::Off => {
                    self.state = PlayerState::Stopped;
                    self.position_ms = 0;
                }
            }
        }
    }

    /// Notify the player that the user interacted (for control auto-hide).
    pub fn user_activity(&mut self) {
        self.controls_visible = true;
        self.controls_hide_timer_ms = 0;
    }

    /// Are the on-screen controls currently visible?
    pub fn controls_visible(&self) -> bool {
        self.controls_visible
    }
}

impl Default for VideoPlayer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Time formatting
// ============================================================================

/// Format a duration in milliseconds as "H:MM:SS" or "M:SS".
///
/// - Durations under one hour omit the hour component.
/// - Durations of zero display as "0:00".
pub fn format_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

// ============================================================================
// Rendering — video controls overlay
// ============================================================================

/// Render the video player UI overlay (control bar, seek bar, etc.) into
/// a `RenderTree`.
///
/// This is called each frame when video mode is active.  The caller passes
/// the total area available and the player state.
pub fn render_controls(
    player: &VideoPlayer,
    tree: &mut RenderTree,
    area_x: f32,
    area_y: f32,
    area_w: f32,
    area_h: f32,
) {
    // Video frame area (black letterbox background).
    tree.push(RenderCommand::FillRect {
        x: area_x,
        y: area_y,
        width: area_w,
        height: area_h,
        color: MOCHA_CRUST,
        corner_radii: CornerRadii::ZERO,
    });

    // If a frame is loaded, render it centered.
    if player.frame_width > 0 && player.frame_height > 0 {
        let (fw, fh) = fit_dimensions(
            player.frame_width,
            player.frame_height,
            area_w,
            area_h - CONTROL_BAR_HEIGHT,
        );
        let fx = area_x + (area_w - fw) / 2.0;
        let fy = area_y + (area_h - CONTROL_BAR_HEIGHT - fh) / 2.0;

        tree.push(RenderCommand::Image {
            x: fx,
            y: fy,
            width: fw,
            height: fh,
            image_id: player.current_frame_id,
        });
    }

    if !player.controls_visible {
        return;
    }

    let bar_y = area_y + area_h - CONTROL_BAR_HEIGHT;

    // Semi-transparent control bar background.
    tree.push(RenderCommand::FillRect {
        x: area_x,
        y: bar_y,
        width: area_w,
        height: CONTROL_BAR_HEIGHT,
        color: Color::rgba(24, 24, 37, 220),
        corner_radii: CornerRadii::ZERO,
    });

    // -- Seek bar --
    render_seek_bar(player, tree, area_x, bar_y, area_w);

    // -- Time display --
    let time_y = bar_y + SEEK_BAR_HIT_HEIGHT + 8.0;
    let current_str = format_time(player.position_ms);
    let duration_str = format_time(player.duration_ms);
    let time_text = format!("{current_str} / {duration_str}");

    tree.push(RenderCommand::Text {
        x: area_x + 8.0,
        y: time_y,
        text: time_text,
        color: MOCHA_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // -- Playback buttons (centered) --
    let button_y = bar_y + SEEK_BAR_HIT_HEIGHT + 4.0;
    let center_x = area_x + area_w / 2.0;

    render_playback_buttons(player, tree, center_x, button_y);

    // -- Volume (right side) --
    render_volume(player, tree, area_x + area_w - 160.0, button_y, 120.0);

    // -- Speed indicator --
    let speed_text = format!("{:.2}x", player.playback_speed);
    tree.push(RenderCommand::Text {
        x: area_x + area_w - 200.0,
        y: time_y,
        text: speed_text,
        color: if (player.playback_speed - 1.0).abs() < 0.01 {
            MOCHA_SUBTEXT0
        } else {
            MOCHA_PEACH
        },
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // -- Status indicators (left, below time) --
    let indicator_y = time_y + 16.0;
    let mut ix = area_x + 8.0;

    // Loop mode badge
    if player.loop_mode != LoopMode::Off {
        let label = player.loop_mode.label();
        render_badge(tree, ix, indicator_y, label, MOCHA_MAUVE);
        ix += label.len() as f32 * 6.5 + 16.0;
    }

    // Subtitle indicator
    if player.subtitles_enabled {
        render_badge(tree, ix, indicator_y, "CC", MOCHA_GREEN);
        ix += 28.0;
    }

    // Muted indicator
    if player.muted {
        render_badge(tree, ix, indicator_y, "MUTED", MOCHA_RED);
    }

    // -- State overlay (buffering / error) --
    match &player.state {
        PlayerState::Buffering => {
            let msg = "Buffering...";
            let tw = msg.len() as f32 * 7.0;
            tree.push(RenderCommand::Text {
                x: area_x + (area_w - tw) / 2.0,
                y: area_y + area_h / 2.0 - 8.0,
                text: String::from(msg),
                color: MOCHA_YELLOW,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        PlayerState::Error(msg) => {
            let display = if msg.len() > 60 {
                let truncated: String = msg.chars().take(57).collect();
                format!("{truncated}...")
            } else {
                msg.clone()
            };
            tree.push(RenderCommand::Text {
                x: area_x + 20.0,
                y: area_y + area_h / 2.0 - 8.0,
                text: display,
                color: MOCHA_RED,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(area_w - 40.0),
            });
        }
        _ => {}
    }
}

/// Render the seek bar with progress and buffered region.
fn render_seek_bar(player: &VideoPlayer, tree: &mut RenderTree, x: f32, bar_y: f32, width: f32) {
    let seek_x = x + 8.0;
    let seek_w = width - 16.0;
    let seek_y = bar_y + (SEEK_BAR_HIT_HEIGHT - SEEK_BAR_HEIGHT) / 2.0;

    // Background track
    tree.push(RenderCommand::FillRect {
        x: seek_x,
        y: seek_y,
        width: seek_w,
        height: SEEK_BAR_HEIGHT,
        color: MOCHA_SURFACE0,
        corner_radii: CornerRadii::all(3.0),
    });

    if player.duration_ms > 0 {
        // Buffered region (slightly lighter)
        let buffered_frac =
            (player.buffered_ms as f64 / player.duration_ms as f64).clamp(0.0, 1.0) as f32;
        if buffered_frac > 0.0 {
            tree.push(RenderCommand::FillRect {
                x: seek_x,
                y: seek_y,
                width: seek_w * buffered_frac,
                height: SEEK_BAR_HEIGHT,
                color: MOCHA_SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
        }

        // Progress bar (accent color)
        let progress_frac =
            (player.position_ms as f64 / player.duration_ms as f64).clamp(0.0, 1.0) as f32;
        if progress_frac > 0.0 {
            tree.push(RenderCommand::FillRect {
                x: seek_x,
                y: seek_y,
                width: seek_w * progress_frac,
                height: SEEK_BAR_HEIGHT,
                color: MOCHA_BLUE,
                corner_radii: CornerRadii::all(3.0),
            });
        }

        // Seek handle (small circle at current position)
        let handle_x = seek_x + seek_w * progress_frac - 5.0;
        let handle_y = seek_y - 2.0;
        tree.push(RenderCommand::FillRect {
            x: handle_x,
            y: handle_y,
            width: 10.0,
            height: 10.0,
            color: MOCHA_TEXT,
            corner_radii: CornerRadii::all(5.0),
        });
    }
}

/// Render the central playback buttons (prev, play/pause, stop, next).
fn render_playback_buttons(player: &VideoPlayer, tree: &mut RenderTree, center_x: f32, y: f32) {
    let btn_gap = 8.0;
    let total_width = CONTROL_BUTTON_SIZE * 5.0 + btn_gap * 4.0;
    let start_x = center_x - total_width / 2.0;

    let buttons: &[(&str, Color)] = &[
        // Previous
        ("\u{23EE}", MOCHA_TEXT), // |<<
        // Stop
        ("\u{23F9}", MOCHA_TEXT), // Stop
        // Play/Pause
        (
            if player.state == PlayerState::Playing {
                "\u{23F8}"
            } else {
                "\u{25B6}"
            },
            if player.state == PlayerState::Playing {
                MOCHA_BLUE
            } else {
                MOCHA_GREEN
            },
        ),
        // stop (second is actually not needed, but we have a nice layout)
        // Actually: Subtitle toggle
        (
            if player.subtitles_enabled { "CC" } else { "cc" },
            if player.subtitles_enabled {
                MOCHA_GREEN
            } else {
                MOCHA_OVERLAY0
            },
        ),
        // Next
        ("\u{23ED}", MOCHA_TEXT), // >>|
    ];

    for (i, (label, color)) in buttons.iter().enumerate() {
        let bx = start_x + (CONTROL_BUTTON_SIZE + btn_gap) * i as f32;

        // Button background
        tree.push(RenderCommand::FillRect {
            x: bx,
            y,
            width: CONTROL_BUTTON_SIZE,
            height: CONTROL_BUTTON_SIZE,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Button label
        tree.push(RenderCommand::Text {
            x: bx + 4.0,
            y: y + 8.0,
            text: String::from(*label),
            color: *color,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(CONTROL_BUTTON_SIZE - 8.0),
        });
    }
}

/// Render the volume slider.
fn render_volume(player: &VideoPlayer, tree: &mut RenderTree, x: f32, y: f32, width: f32) {
    // Volume icon
    let icon = if player.muted || player.volume < 0.01 {
        "\u{1F507}" // muted speaker
    } else if player.volume < 0.5 {
        "\u{1F509}" // low volume
    } else {
        "\u{1F50A}" // high volume
    };

    tree.push(RenderCommand::Text {
        x,
        y: y + 8.0,
        text: String::from(icon),
        color: if player.muted { MOCHA_RED } else { MOCHA_TEXT },
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Slider track
    let slider_x = x + 24.0;
    let slider_y = y + 14.0;
    let slider_w = width - 24.0;
    let slider_h = 4.0;

    tree.push(RenderCommand::FillRect {
        x: slider_x,
        y: slider_y,
        width: slider_w,
        height: slider_h,
        color: MOCHA_SURFACE0,
        corner_radii: CornerRadii::all(2.0),
    });

    // Filled portion
    let eff = player.effective_volume();
    if eff > 0.0 {
        tree.push(RenderCommand::FillRect {
            x: slider_x,
            y: slider_y,
            width: slider_w * eff,
            height: slider_h,
            color: if player.muted { MOCHA_RED } else { MOCHA_BLUE },
            corner_radii: CornerRadii::all(2.0),
        });
    }

    // Volume percentage text
    let pct = (player.volume * 100.0) as u32;
    tree.push(RenderCommand::Text {
        x: slider_x + slider_w + 4.0,
        y: y + 8.0,
        text: format!("{pct}%"),
        color: MOCHA_SUBTEXT0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

/// Render a small status badge.
fn render_badge(tree: &mut RenderTree, x: f32, y: f32, label: &str, color: Color) {
    let w = label.len() as f32 * 6.5 + 8.0;
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: 16.0,
        color: Color::rgba(color.r, color.g, color.b, 60),
        corner_radii: CornerRadii::all(3.0),
    });
    tree.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 2.0,
        text: String::from(label),
        color,
        font_size: 10.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
}

/// Compute fitted dimensions that maintain aspect ratio within the given area.
fn fit_dimensions(src_w: u32, src_h: u32, area_w: f32, area_h: f32) -> (f32, f32) {
    if src_w == 0 || src_h == 0 {
        return (0.0, 0.0);
    }
    let scale_x = area_w / src_w as f32;
    let scale_y = area_h / src_h as f32;
    let scale = scale_x.min(scale_y);
    (src_w as f32 * scale, src_h as f32 * scale)
}

// ============================================================================
// Keyboard shortcut handling
// ============================================================================

/// Video playback actions triggered by keyboard shortcuts or UI buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoAction {
    PlayPause,
    Stop,
    SeekForward,
    SeekBackward,
    SeekForwardLarge,
    SeekBackwardLarge,
    VolumeUp,
    VolumeDown,
    MuteToggle,
    FullscreenToggle,
    SubtitleToggle,
    SpeedUp,
    SpeedDown,
    LoopToggle,
    NextTrack,
    PrevTrack,
}

/// Map a key event to a video action, if applicable.
///
/// Returns `None` if the key does not correspond to a video shortcut.
pub fn map_key_to_action(key: &str, shift: bool) -> Option<VideoAction> {
    match key {
        "Space" => Some(VideoAction::PlayPause),
        "Left" if !shift => Some(VideoAction::SeekBackward),
        "Right" if !shift => Some(VideoAction::SeekForward),
        "Left" if shift => Some(VideoAction::SeekBackwardLarge),
        "Right" if shift => Some(VideoAction::SeekForwardLarge),
        "Up" => Some(VideoAction::VolumeUp),
        "Down" => Some(VideoAction::VolumeDown),
        "M" | "m" => Some(VideoAction::MuteToggle),
        "F" | "f" => Some(VideoAction::FullscreenToggle),
        "S" | "s" => Some(VideoAction::SubtitleToggle),
        "BracketLeft" | "[" => Some(VideoAction::SpeedDown),
        "BracketRight" | "]" => Some(VideoAction::SpeedUp),
        "L" | "l" => Some(VideoAction::LoopToggle),
        "N" | "n" => Some(VideoAction::NextTrack),
        "P" | "p" => Some(VideoAction::PrevTrack),
        _ => None,
    }
}

/// Execute a `VideoAction` on the player, returning `true` if state changed.
pub fn execute_action(player: &mut VideoPlayer, action: VideoAction) -> bool {
    player.user_activity();
    match action {
        VideoAction::PlayPause => {
            player.toggle_play_pause();
            true
        }
        VideoAction::Stop => {
            player.stop();
            true
        }
        VideoAction::SeekForward => {
            player.seek_forward(SEEK_SMALL_MS);
            true
        }
        VideoAction::SeekBackward => {
            player.seek_backward(SEEK_SMALL_MS);
            true
        }
        VideoAction::SeekForwardLarge => {
            player.seek_forward(SEEK_LARGE_MS);
            true
        }
        VideoAction::SeekBackwardLarge => {
            player.seek_backward(SEEK_LARGE_MS);
            true
        }
        VideoAction::VolumeUp => {
            player.volume_up();
            true
        }
        VideoAction::VolumeDown => {
            player.volume_down();
            true
        }
        VideoAction::MuteToggle => {
            player.toggle_mute();
            true
        }
        VideoAction::FullscreenToggle => {
            player.toggle_fullscreen();
            true
        }
        VideoAction::SubtitleToggle => {
            player.toggle_subtitles();
            true
        }
        VideoAction::SpeedUp => {
            player.speed_up();
            true
        }
        VideoAction::SpeedDown => {
            player.speed_down();
            true
        }
        VideoAction::LoopToggle => {
            player.toggle_loop();
            true
        }
        VideoAction::NextTrack => {
            player.next_in_playlist();
            true
        }
        VideoAction::PrevTrack => {
            player.prev_in_playlist();
            true
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- State machine transitions ----------------------------------------

    #[test]
    fn test_stopped_to_playing() {
        let mut p = VideoPlayer::new();
        assert_eq!(*p.state(), PlayerState::Stopped);
        p.play();
        assert_eq!(*p.state(), PlayerState::Playing);
    }

    #[test]
    fn test_playing_to_paused() {
        let mut p = VideoPlayer::new();
        p.play();
        p.pause();
        assert_eq!(*p.state(), PlayerState::Paused);
    }

    #[test]
    fn test_paused_to_playing() {
        let mut p = VideoPlayer::new();
        p.play();
        p.pause();
        p.play();
        assert_eq!(*p.state(), PlayerState::Playing);
    }

    #[test]
    fn test_playing_to_stopped() {
        let mut p = VideoPlayer::new();
        p.play();
        p.stop();
        assert_eq!(*p.state(), PlayerState::Stopped);
        assert_eq!(p.current_position_ms(), 0);
    }

    #[test]
    fn test_toggle_play_pause() {
        let mut p = VideoPlayer::new();
        p.toggle_play_pause();
        assert_eq!(*p.state(), PlayerState::Playing);
        p.toggle_play_pause();
        assert_eq!(*p.state(), PlayerState::Paused);
        p.toggle_play_pause();
        assert_eq!(*p.state(), PlayerState::Playing);
    }

    #[test]
    fn test_stop_resets_position() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 10_000;
        p.play();
        p.seek(5000);
        assert_eq!(p.current_position_ms(), 5000);
        p.stop();
        assert_eq!(p.current_position_ms(), 0);
    }

    // -- Time formatting --------------------------------------------------

    #[test]
    fn test_format_time_zero() {
        assert_eq!(format_time(0), "0:00");
    }

    #[test]
    fn test_format_time_seconds_only() {
        assert_eq!(format_time(42_000), "0:42");
    }

    #[test]
    fn test_format_time_minutes_and_seconds() {
        assert_eq!(format_time(222_000), "3:42");
    }

    #[test]
    fn test_format_time_hours() {
        // 1 hour, 23 minutes, 45 seconds = 5025 seconds
        assert_eq!(format_time(5_025_000), "1:23:45");
    }

    #[test]
    fn test_format_time_padding() {
        // 1:05:09
        assert_eq!(format_time(3_909_000), "1:05:09");
    }

    // -- Seek clamping ----------------------------------------------------

    #[test]
    fn test_seek_clamps_to_duration() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 10_000;
        p.seek(999_999);
        assert_eq!(p.current_position_ms(), 10_000);
    }

    #[test]
    fn test_seek_backward_does_not_underflow() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 10_000;
        p.seek(1000);
        p.seek_backward(5000);
        assert_eq!(p.current_position_ms(), 0);
    }

    #[test]
    fn test_seek_forward_clamps() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 10_000;
        p.seek(9000);
        p.seek_forward(5000);
        assert_eq!(p.current_position_ms(), 10_000);
    }

    // -- Volume clamping --------------------------------------------------

    #[test]
    fn test_volume_clamp_upper() {
        let mut p = VideoPlayer::new();
        p.set_volume(5.0);
        assert!((p.volume() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_volume_clamp_lower() {
        let mut p = VideoPlayer::new();
        p.set_volume(-1.0);
        assert!((p.volume()).abs() < f32::EPSILON);
    }

    #[test]
    fn test_volume_step_up_down() {
        let mut p = VideoPlayer::new();
        p.set_volume(0.5);
        p.volume_up();
        assert!((p.volume() - 0.55).abs() < 0.001);
        p.volume_down();
        assert!((p.volume() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_mute_toggle() {
        let mut p = VideoPlayer::new();
        assert!(!p.is_muted());
        p.toggle_mute();
        assert!(p.is_muted());
        assert!((p.effective_volume()).abs() < f32::EPSILON);
        p.toggle_mute();
        assert!(!p.is_muted());
        assert!(p.effective_volume() > 0.0);
    }

    // -- Playback speed clamping ------------------------------------------

    #[test]
    fn test_speed_clamp_upper() {
        let mut p = VideoPlayer::new();
        p.set_playback_speed(100.0);
        assert!((p.playback_speed() - MAX_SPEED).abs() < f32::EPSILON);
    }

    #[test]
    fn test_speed_clamp_lower() {
        let mut p = VideoPlayer::new();
        p.set_playback_speed(0.01);
        assert!((p.playback_speed() - MIN_SPEED).abs() < f32::EPSILON);
    }

    #[test]
    fn test_speed_up_cycles_presets() {
        let mut p = VideoPlayer::new();
        p.set_playback_speed(1.0);
        p.speed_up();
        assert!((p.playback_speed() - 1.5).abs() < 0.01);
        p.speed_up();
        assert!((p.playback_speed() - 2.0).abs() < 0.01);
        p.speed_up();
        assert!((p.playback_speed() - 4.0).abs() < 0.01);
        // At max preset, should not change.
        p.speed_up();
        assert!((p.playback_speed() - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_speed_down_cycles_presets() {
        let mut p = VideoPlayer::new();
        p.set_playback_speed(1.0);
        p.speed_down();
        assert!((p.playback_speed() - 0.5).abs() < 0.01);
        p.speed_down();
        assert!((p.playback_speed() - 0.25).abs() < 0.01);
        // At min preset, should not change.
        p.speed_down();
        assert!((p.playback_speed() - 0.25).abs() < 0.01);
    }

    // -- Container format detection ---------------------------------------

    #[test]
    fn test_detect_avi() {
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"AVI ");
        assert_eq!(ContainerFormat::detect(&data), Some(ContainerFormat::Avi));
    }

    #[test]
    fn test_detect_mp4_ftyp() {
        let mut data = vec![0u8; 16];
        // size (8 bytes) + "ftyp" + brand
        data[0..4].copy_from_slice(&16u32.to_be_bytes());
        data[4..8].copy_from_slice(b"ftyp");
        data[8..12].copy_from_slice(b"isom");
        assert_eq!(ContainerFormat::detect(&data), Some(ContainerFormat::Mp4));
    }

    #[test]
    fn test_detect_mkv() {
        let mut data = vec![0u8; 64];
        data[..4].copy_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]);
        // No "webm" substring => Matroska.
        data[4..12].copy_from_slice(b"matroska");
        assert_eq!(ContainerFormat::detect(&data), Some(ContainerFormat::Mkv));
    }

    #[test]
    fn test_detect_webm() {
        let mut data = vec![0u8; 64];
        data[..4].copy_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]);
        data[10..14].copy_from_slice(b"webm");
        assert_eq!(ContainerFormat::detect(&data), Some(ContainerFormat::WebM));
    }

    #[test]
    fn test_detect_unknown() {
        let data = vec![0u8; 16];
        assert_eq!(ContainerFormat::detect(&data), None);
    }

    #[test]
    fn test_detect_too_short() {
        let data = [0u8; 4];
        assert_eq!(ContainerFormat::detect(&data), None);
    }

    // -- MP4 box parsing --------------------------------------------------

    #[test]
    fn test_parse_mp4_single_box() {
        // A minimal ftyp box: size(4) + type(4) + brand(4) + version(4) = 16 bytes
        let mut data = vec![0u8; 16];
        data[0..4].copy_from_slice(&16u32.to_be_bytes());
        data[4..8].copy_from_slice(b"ftyp");
        data[8..12].copy_from_slice(b"isom");

        let boxes = parse_mp4_boxes(&data).expect("should parse");
        assert_eq!(boxes.len(), 1);
        assert_eq!(&boxes[0].box_type, b"ftyp");
        assert_eq!(boxes[0].size, 16);
        assert_eq!(boxes[0].offset, 0);
    }

    #[test]
    fn test_parse_mp4_two_boxes() {
        let mut data = vec![0u8; 32];
        // Box 1: 16 bytes, type "ftyp"
        data[0..4].copy_from_slice(&16u32.to_be_bytes());
        data[4..8].copy_from_slice(b"ftyp");
        // Box 2: 16 bytes, type "mdat"
        data[16..20].copy_from_slice(&16u32.to_be_bytes());
        data[20..24].copy_from_slice(b"mdat");

        let boxes = parse_mp4_boxes(&data).expect("should parse");
        assert_eq!(boxes.len(), 2);
        assert_eq!(&boxes[0].box_type, b"ftyp");
        assert_eq!(&boxes[1].box_type, b"mdat");
        assert_eq!(boxes[1].offset, 16);
    }

    #[test]
    fn test_parse_mp4_invalid_size() {
        // Box with size smaller than header (size = 4, but header is 8 bytes).
        let mut data = vec![0u8; 16];
        data[0..4].copy_from_slice(&4u32.to_be_bytes());
        data[4..8].copy_from_slice(b"ftyp");

        let result = parse_mp4_boxes(&data);
        assert!(result.is_err());
    }

    // -- Loop mode --------------------------------------------------------

    #[test]
    fn test_loop_mode_cycle() {
        let m = LoopMode::Off;
        assert_eq!(m.next(), LoopMode::Single);
        assert_eq!(m.next().next(), LoopMode::Playlist);
        assert_eq!(m.next().next().next(), LoopMode::Off);
    }

    // -- Tick / end-of-video behavior -------------------------------------

    #[test]
    fn test_tick_advances_position() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 10_000;
        p.play();
        p.tick(100);
        assert_eq!(p.current_position_ms(), 100);
    }

    #[test]
    fn test_tick_at_2x_speed() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 10_000;
        p.set_playback_speed(2.0);
        p.play();
        p.tick(100);
        assert_eq!(p.current_position_ms(), 200);
    }

    #[test]
    fn test_tick_stops_at_end_no_loop() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 1000;
        p.loop_mode = LoopMode::Off;
        p.play();
        p.tick(2000);
        assert_eq!(*p.state(), PlayerState::Stopped);
        assert_eq!(p.current_position_ms(), 0);
    }

    #[test]
    fn test_tick_loops_single() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 1000;
        p.loop_mode = LoopMode::Single;
        p.play();
        p.tick(2000);
        // Should still be playing, position reset to 0.
        assert_eq!(*p.state(), PlayerState::Playing);
        assert_eq!(p.current_position_ms(), 0);
    }

    #[test]
    fn test_tick_does_not_advance_when_paused() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 10_000;
        p.play();
        p.pause();
        p.tick(1000);
        assert_eq!(p.current_position_ms(), 0);
    }

    // -- Playlist ---------------------------------------------------------

    #[test]
    fn test_playlist_navigation() {
        let mut p = VideoPlayer::new();
        p.set_playlist(vec![
            String::from("a.mp4"),
            String::from("b.mp4"),
            String::from("c.mp4"),
        ]);
        assert_eq!(p.playlist_index, 0);
        p.next_in_playlist();
        assert_eq!(p.playlist_index, 1);
        p.next_in_playlist();
        assert_eq!(p.playlist_index, 2);
        p.next_in_playlist();
        assert_eq!(p.playlist_index, 0); // wraps

        p.prev_in_playlist();
        assert_eq!(p.playlist_index, 2); // wraps back
    }

    // -- Fullscreen / subtitles -------------------------------------------

    #[test]
    fn test_fullscreen_toggle() {
        let mut p = VideoPlayer::new();
        assert!(!p.is_fullscreen());
        p.toggle_fullscreen();
        assert!(p.is_fullscreen());
        p.toggle_fullscreen();
        assert!(!p.is_fullscreen());
    }

    #[test]
    fn test_subtitle_toggle() {
        let mut p = VideoPlayer::new();
        assert!(!p.subtitles_enabled());
        p.toggle_subtitles();
        assert!(p.subtitles_enabled());
        p.toggle_subtitles();
        assert!(!p.subtitles_enabled());
    }

    // -- Key mapping ------------------------------------------------------

    #[test]
    fn test_key_map_space() {
        assert_eq!(
            map_key_to_action("Space", false),
            Some(VideoAction::PlayPause)
        );
    }

    #[test]
    fn test_key_map_seek() {
        assert_eq!(
            map_key_to_action("Left", false),
            Some(VideoAction::SeekBackward)
        );
        assert_eq!(
            map_key_to_action("Right", false),
            Some(VideoAction::SeekForward)
        );
        assert_eq!(
            map_key_to_action("Left", true),
            Some(VideoAction::SeekBackwardLarge)
        );
        assert_eq!(
            map_key_to_action("Right", true),
            Some(VideoAction::SeekForwardLarge)
        );
    }

    #[test]
    fn test_key_map_volume() {
        assert_eq!(map_key_to_action("Up", false), Some(VideoAction::VolumeUp));
        assert_eq!(
            map_key_to_action("Down", false),
            Some(VideoAction::VolumeDown)
        );
        assert_eq!(map_key_to_action("M", false), Some(VideoAction::MuteToggle));
    }

    #[test]
    fn test_key_map_speed_brackets() {
        assert_eq!(map_key_to_action("[", false), Some(VideoAction::SpeedDown));
        assert_eq!(map_key_to_action("]", false), Some(VideoAction::SpeedUp));
    }

    #[test]
    fn test_key_map_unknown() {
        assert_eq!(map_key_to_action("Q", false), None);
    }

    // -- Rendering --------------------------------------------------------

    #[test]
    fn test_render_controls_produces_commands() {
        let p = VideoPlayer::new();
        let mut tree = RenderTree::new();
        render_controls(&p, &mut tree, 0.0, 0.0, 800.0, 600.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_controls_with_video_loaded() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 60_000;
        p.frame_width = 1920;
        p.frame_height = 1080;
        p.current_frame_id = 42;
        p.play();

        let mut tree = RenderTree::new();
        render_controls(&p, &mut tree, 0.0, 0.0, 800.0, 600.0);

        // Should include an Image command for the video frame.
        let has_image = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Image { .. }));
        assert!(has_image);
    }

    // -- Fit dimensions ---------------------------------------------------

    #[test]
    fn test_fit_dimensions_landscape() {
        let (w, h) = fit_dimensions(1920, 1080, 800.0, 600.0);
        // Aspect ratio should be preserved.
        let ratio = w / h;
        let expected_ratio = 1920.0 / 1080.0;
        assert!((ratio - expected_ratio).abs() < 0.01);
        // Should fit within the area.
        assert!(w <= 800.0 + 0.01);
        assert!(h <= 600.0 + 0.01);
    }

    #[test]
    fn test_fit_dimensions_portrait() {
        let (w, h) = fit_dimensions(1080, 1920, 800.0, 600.0);
        assert!(w <= 800.0 + 0.01);
        assert!(h <= 600.0 + 0.01);
    }

    #[test]
    fn test_fit_dimensions_zero() {
        let (w, h) = fit_dimensions(0, 0, 800.0, 600.0);
        assert!((w).abs() < f32::EPSILON);
        assert!((h).abs() < f32::EPSILON);
    }

    // -- Execute action ---------------------------------------------------

    #[test]
    fn test_execute_action_play_pause() {
        let mut p = VideoPlayer::new();
        assert!(execute_action(&mut p, VideoAction::PlayPause));
        assert_eq!(*p.state(), PlayerState::Playing);
        assert!(execute_action(&mut p, VideoAction::PlayPause));
        assert_eq!(*p.state(), PlayerState::Paused);
    }

    #[test]
    fn test_execute_action_seek() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 60_000;
        assert!(execute_action(&mut p, VideoAction::SeekForward));
        assert_eq!(p.current_position_ms(), SEEK_SMALL_MS);
        assert!(execute_action(&mut p, VideoAction::SeekBackward));
        assert_eq!(p.current_position_ms(), 0);
    }

    // -- AVI header parsing -----------------------------------------------

    #[test]
    fn test_avi_header_frame_rate() {
        let hdr = AviHeader {
            microseconds_per_frame: 33333, // ~30 fps
            width: 640,
            height: 480,
            total_frames: 900,
            codec_fourcc: *b"H264",
        };
        let fps = hdr.frame_rate();
        assert!((fps - 30.0).abs() < 0.1);
    }

    #[test]
    fn test_avi_header_duration() {
        let hdr = AviHeader {
            microseconds_per_frame: 40000, // 25 fps
            width: 1920,
            height: 1080,
            total_frames: 250,
            codec_fourcc: *b"XVID",
        };
        // 250 frames * 40000 us = 10,000,000 us = 10,000 ms
        assert_eq!(hdr.duration_ms(), 10_000);
    }

    // -- EBML VINT decoding -----------------------------------------------

    #[test]
    fn test_ebml_vint_single_byte() {
        // 0x81 = 1000_0001 -> length 1, value = 0x01
        assert_eq!(decode_ebml_vint(&[0x81]), Some((1, 1)));
    }

    #[test]
    fn test_ebml_vint_two_bytes() {
        // 0x40 0x01 -> length 2, value = 0x0001
        assert_eq!(decode_ebml_vint(&[0x40, 0x01]), Some((1, 2)));
    }

    #[test]
    fn test_ebml_vint_zero_byte() {
        assert_eq!(decode_ebml_vint(&[0x00]), None);
    }

    // -- Controls visibility / auto-hide ----------------------------------

    #[test]
    fn test_controls_auto_hide() {
        let mut p = VideoPlayer::new();
        p.duration_ms = 60_000;
        p.play();

        assert!(p.controls_visible());
        // Simulate 4 seconds of playback — controls should hide.
        p.tick(4000);
        assert!(!p.controls_visible());

        // User activity should bring them back.
        p.user_activity();
        assert!(p.controls_visible());
    }
}
