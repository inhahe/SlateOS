//! Slate OS Photo Manager
//!
//! A photo library management application with:
//! - Photo library with albums: made from the sidebar, filled by
//!   right-clicking a photograph, and browsable as a view; smart albums
//!   exist as a model only
//! - EXIF metadata parsing and display (camera, exposure, GPS, etc.)
//! - Thumbnail grid view at four card sizes (80/120/160/200 px), cycled from
//!   the toolbar, each card showing the photograph itself
//! - Single-photo view: the photograph itself, decoded through `imagecodec`
//!   and drawn at its own proportions
//! - Per-photograph adjustment values: brightness, contrast, saturation,
//!   exposure, temperature
//! - Star ratings (0-5), applied to every selected photograph; colour
//!   labels exist as a model only
//! - Several photographs at once, with Shift and an arrow
//! - Tagging and keyword system
//! - Timeline view grouping photos by date
//! - Slideshow mode with configurable interval and transitions
//! - Import of a single file through a picker, with its EXIF read
//! - A search box that filters the library by name, path or tag as you
//!   type
//! - The library is saved to `photolibrary.txt` and read back at start:
//!   photographs, ratings, flags, tags and colour labels survive closing
//!   the window. See `library` for the format and what it omits.
//! - Re-import detection by path and size, not by image content
//! - Batch operations: tag, rate, move, delete
//! - Multi-panel UI: sidebar, thumbnail grid, info panel
//!
//! # What it does not do yet
//!
//! Three claims in the list above used to say more than the code did, and they
//! failed for one shared reason: nothing in this application had ever held a
//! pixel. It could read a photograph -- `import_from_disk` has done that since
//! the repair noted on it -- and then drew a card with the file's name on it.
//! Two of the three are now true: the single-photo view decodes through
//! `imagecodec`, and the grid generates thumbnails through `thumbs`. What
//! follows is what is still owed.
//!
//! - **The adjustments cannot be set, let alone applied.** Each photograph
//!   carries brightness, contrast, saturation, exposure, temperature,
//!   highlights, shadows, sharpness, vignette and rotation; the info panel
//!   lists them when they differ from the default, and nothing in this
//!   application can make them differ. `ImageAdjustments::rotate_cw` and
//!   `rotate_ccw` exist and are called from tests only. So the panel's
//!   "adjusted" section has never been drawn outside a test, and no pixel has
//!   ever been changed by one. (An earlier revision of this list said they
//!   were "recorded, not applied", which is still too generous: there is no
//!   way to record one.)
//! - **Face regions are never detected.** `Photo::faces` is constructed empty
//!   and nothing ever pushes to it.
//! - **`batch_tag` is still unreachable.** Selecting several photographs and
//!   rating them, or putting them all in an album, both work now; tagging
//!   does not, because there is nowhere to type a tag. The function is
//!   written and tested and waiting for a text field.
//! - **Smart albums cannot be made either.** `create_smart_album` and the
//!   rule matching behind it are tested and unreachable, exactly as ordinary
//!   albums were until now.
//! - **Nothing is exported.** `ExportOptions` records a format, a quality and
//!   a size, has a `Default` and a test, and is read by nothing: no function
//!   in this crate writes a picture anywhere. The feature list offered
//!   "Export with format/quality selection", which is the options without the
//!   export -- a settings page is built when something obeys it.
//! - **Import is one file at a time.** The list offered "import from directory
//!   with date-based organization". There is no `read_dir` in this crate;
//!   `import_from_disk` takes a single path from the picker, and nothing
//!   organises anything by date.
//! - **The single-photo view has no zoom and no pan.** The list claimed
//!   both. The only `Zoom` in this file is the name of a slideshow
//!   transition. The grid's four card sizes, listed above, are a different
//!   thing and are real -- I deleted that entry too on the first pass, having
//!   judged it by the company it kept rather than by reading
//!   `cycle_thumb_size`. A feature list is corrected one claim at a time or
//!   not at all.
//!
//! Uses the guitk library for UI rendering.

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
#![allow(clippy::doc_markdown)]

mod library;

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
// The toolkit's rectangle rather than a private copy: this crate had
// the same four floats under `width`/`height`, with the same half-open
// `contains`. See `known-issues.md`
// `TD-C-TEN-RECTANGLE-TYPES-IN-THREE-SPELLINGS`.
use guitk::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::menu::{ContextMenu, MenuItem};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use oswindow::{Event, RenderTree};
use std::process::ExitCode;
use std::time::Duration;

use std::collections::HashMap;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

// ============================================================================
// Layout constants
// ============================================================================

const SIDEBAR_WIDTH: f32 = 200.0;
const INFO_PANEL_WIDTH: f32 = 260.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const THUMB_SIZES: [f32; 4] = [80.0, 120.0, 160.0, 200.0];
const THUMB_PADDING: f32 = 8.0;
const ITEM_HEIGHT: f32 = 28.0;
const CORNER_RADIUS: f32 = 4.0;
const SLIDESHOW_DEFAULT_INTERVAL_MS: u64 = 3000;
const MAX_STARS: u8 = 5;
/// How often the slideshow clock ticks.
const SLIDESHOW_TICK: Duration = Duration::from_millis(100);
/// A window narrower than the sidebar plus a column of thumbnails, or shorter
/// than the toolbar plus a row of them, has no layout left to follow.
const MIN_WINDOW_WIDTH: f32 = 640.0;
const MIN_WINDOW_HEIGHT: f32 = 400.0;
const WINDOW_WIDTH: f32 = 1400.0;
const WINDOW_HEIGHT: f32 = 900.0;

/// One row of the sidebar, as it is laid out down the column.
///
/// The sidebar was drawn by walking a `cy` down through four blocks with
/// different gaps between them, so the only record of where a row was, was
/// the pixels already drawn. Both the renderer and the hit test now walk this.
#[derive(Clone, Debug)]
enum SidebarRow {
    /// A section heading: LIBRARY, ALBUMS, SMART ALBUMS.
    Header(&'static str),
    /// Air between sections.
    Gap(f32),
    /// A row that does something rather than going somewhere.
    ///
    /// Separate from `Item` because `SidebarItem` answers "which collection am
    /// I looking at", and an action is not a collection. Folding one in would
    /// mean every `match` over a selection -- `visible_photos`, the title, the
    /// filter -- growing an arm for something that can never be selected.
    Action {
        label: String,
        action: SidebarAction,
    },
    /// A row that goes somewhere.
    Item {
        label: String,
        target: SidebarItem,
        /// How far in the label sits: albums are indented under their heading.
        indent: f32,
        /// The colour the label takes when it is the selected row.
        accent: Color,
    },
}

impl SidebarRow {
    const HEADER_H: f32 = 20.0;

    fn height(&self) -> f32 {
        match self {
            Self::Header(_) => Self::HEADER_H,
            Self::Gap(h) => *h,
            Self::Item { .. } | Self::Action { .. } => ITEM_HEIGHT,
        }
    }
}

/// Something a sidebar row does, rather than somewhere it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarAction {
    /// Begin naming a new album.
    NewAlbum,
}

/// A control in the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarControl {
    /// Switch to a view.
    View(ViewMode),
    /// Step the sort order along.
    Sort,
    /// Step the thumbnail size along.
    ThumbSize,
    /// The search box. Clicking it puts the keyboard there.
    ///
    /// It was drawn from the beginning and was not a control at all: its
    /// rectangle existed in the layout only to position the button after it,
    /// so the one thing in this toolbar that looks like it takes typing was
    /// the one thing that could not be clicked.
    Search,
    /// Start or stop the slideshow.
    Slideshow,
    /// Open the file picker and bring a photograph in.
    ///
    /// The window had no way to add a photograph at all until 2026-09-15 --
    /// every control here rearranged a library that `main` had invented. The
    /// EXIF parser, the album model and the whole grid were real; there was no
    /// door.
    Import,
}

// ============================================================================
// Unique IDs
// ============================================================================

pub type PhotoId = u64;
pub type AlbumId = u64;

/// Monotonic ID generator.
#[derive(Debug)]
struct IdGen {
    next: u64,
}

impl IdGen {
    const fn new(start: u64) -> Self {
        Self { next: start }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

// ============================================================================
// Image format
// ============================================================================

/// Supported image formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Bmp,
    Gif,
    Tiff,
    WebP,
    Heic,
    Raw,
}

impl ImageFormat {
    /// Detect format from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "bmp" => Some(Self::Bmp),
            "gif" => Some(Self::Gif),
            "tif" | "tiff" => Some(Self::Tiff),
            "webp" => Some(Self::WebP),
            "heic" | "heif" => Some(Self::Heic),
            "raw" | "cr2" | "nef" | "arw" | "dng" | "orf" | "rw2" => Some(Self::Raw),
            _ => None,
        }
    }

    /// File extension for this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
            Self::Tiff => "tiff",
            Self::WebP => "webp",
            Self::Heic => "heic",
            Self::Raw => "raw",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Bmp => "BMP",
            Self::Gif => "GIF",
            Self::Tiff => "TIFF",
            Self::WebP => "WebP",
            Self::Heic => "HEIC",
            Self::Raw => "RAW",
        }
    }
}

// ============================================================================
// Color label
// ============================================================================

/// Color labels for photo organization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorLabel {
    None,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl ColorLabel {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
        }
    }

    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::None => pal.overlay0,
            Self::Red => pal.red,
            Self::Orange => pal.peach,
            Self::Yellow => pal.yellow,
            Self::Green => pal.green,
            Self::Blue => pal.blue,
            Self::Purple => pal.mauve,
        }
    }

    pub fn all() -> &'static [ColorLabel] {
        &[
            Self::None,
            Self::Red,
            Self::Orange,
            Self::Yellow,
            Self::Green,
            Self::Blue,
            Self::Purple,
        ]
    }

    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Red,
            Self::Red => Self::Orange,
            Self::Orange => Self::Yellow,
            Self::Yellow => Self::Green,
            Self::Green => Self::Blue,
            Self::Blue => Self::Purple,
            Self::Purple => Self::None,
        }
    }
}

// ============================================================================
// EXIF metadata
// ============================================================================

/// Parsed EXIF metadata for a photo.
#[derive(Clone, Debug, Default)]
pub struct ExifData {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub focal_length_mm: Option<f32>,
    pub aperture: Option<f32>,
    pub shutter_speed: Option<String>,
    pub iso: Option<u32>,
    pub flash_fired: Option<bool>,
    pub date_taken: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f32>,
    pub orientation: Option<u16>,
    pub software: Option<String>,
    pub copyright: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub color_space: Option<String>,
    pub white_balance: Option<String>,
    pub metering_mode: Option<String>,
    pub exposure_program: Option<String>,
    pub exposure_bias: Option<f32>,
}

impl ExifData {
    /// Create empty EXIF data.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Sample EXIF data, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. It was reachable from production and
    /// `seeded_library` used it, so every photo in the window that opened was
    /// described as a Canon EOS R5 shot in San Francisco. A fixture that
    /// production can reach is a fixture that eventually ships.
    #[cfg(test)]
    pub fn sample() -> Self {
        Self {
            camera_make: Some("Canon".to_owned()),
            camera_model: Some("EOS R5".to_owned()),
            lens: Some("RF 24-70mm f/2.8L IS USM".to_owned()),
            focal_length_mm: Some(50.0),
            aperture: Some(2.8),
            shutter_speed: Some("1/250".to_owned()),
            iso: Some(400),
            flash_fired: Some(false),
            date_taken: Some("2025-06-15 14:30:22".to_owned()),
            gps_latitude: Some(37.7749),
            gps_longitude: Some(-122.4194),
            gps_altitude: Some(16.0),
            orientation: Some(1),
            software: Some("Adobe Lightroom 7.0".to_owned()),
            copyright: None,
            width: Some(8192),
            height: Some(5464),
            color_space: Some("sRGB".to_owned()),
            white_balance: Some("Auto".to_owned()),
            metering_mode: Some("Multi-segment".to_owned()),
            exposure_program: Some("Aperture Priority".to_owned()),
            exposure_bias: Some(0.0),
        }
    }

    /// Format resolution as "WxH" string.
    pub fn resolution_str(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w} x {h}"),
            _ => "Unknown".to_owned(),
        }
    }

    /// Format GPS coordinates as a readable string.
    pub fn gps_str(&self) -> Option<String> {
        match (self.gps_latitude, self.gps_longitude) {
            (Some(lat), Some(lon)) => {
                let lat_dir = if lat >= 0.0 { "N" } else { "S" };
                let lon_dir = if lon >= 0.0 { "E" } else { "W" };
                Some(format!(
                    "{:.4}{} {:.4}{}",
                    lat.abs(),
                    lat_dir,
                    lon.abs(),
                    lon_dir
                ))
            }
            _ => None,
        }
    }

    /// Format megapixels.
    pub fn megapixels(&self) -> Option<f32> {
        match (self.width, self.height) {
            (Some(w), Some(h)) => {
                let px = f64::from(w) * f64::from(h);
                Some((px / 1_000_000.0) as f32)
            }
            _ => None,
        }
    }

    /// Get exposure summary (aperture, shutter, ISO).
    pub fn exposure_summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ap) = self.aperture {
            parts.push(format!("f/{ap:.1}"));
        }
        if let Some(ref ss) = self.shutter_speed {
            parts.push(format!("{ss}s"));
        }
        if let Some(iso) = self.iso {
            parts.push(format!("ISO {iso}"));
        }
        if parts.is_empty() {
            "No exposure data".to_owned()
        } else {
            parts.join("  ")
        }
    }
}

/// Parse EXIF data from raw bytes (simplified parser for common tags).
pub fn parse_exif_from_bytes(data: &[u8]) -> ExifData {
    let mut exif = ExifData::empty();

    // Check for JPEG SOI marker + EXIF APP1 header
    if data.len() < 12 {
        return exif;
    }

    // Look for "Exif\0\0" marker
    let exif_header = b"Exif\0\0";
    let mut offset = None;
    for i in 0..data.len().saturating_sub(6) {
        if data.get(i..i.saturating_add(6)) == Some(exif_header) {
            offset = Some(i.saturating_add(6));
            break;
        }
    }

    let tiff_start = match offset {
        Some(o) => o,
        None => return exif,
    };

    // Determine byte order (II = little-endian, MM = big-endian)
    let little_endian = match data.get(tiff_start..tiff_start.saturating_add(2)) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return exif,
    };

    // Verify TIFF magic number
    let magic = read_u16(data, tiff_start.saturating_add(2), little_endian);
    if magic != Some(42) {
        return exif;
    }

    // Get IFD0 offset
    let ifd0_offset = match read_u32(data, tiff_start.saturating_add(4), little_endian) {
        Some(o) => tiff_start.saturating_add(o as usize),
        None => return exif,
    };

    // Parse IFD entries
    parse_ifd_entries(data, ifd0_offset, tiff_start, little_endian, &mut exif);

    exif
}

fn read_u16(data: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    let b0 = u16::from(*data.get(offset)?);
    let b1 = u16::from(*data.get(offset.saturating_add(1))?);
    if little_endian {
        Some(b0 | (b1 << 8))
    } else {
        Some((b0 << 8) | b1)
    }
}

fn read_u32(data: &[u8], offset: usize, little_endian: bool) -> Option<u32> {
    let lo = u32::from(read_u16(data, offset, little_endian)?);
    let hi = u32::from(read_u16(data, offset.saturating_add(2), little_endian)?);
    if little_endian {
        Some(lo | (hi << 16))
    } else {
        Some((lo << 16) | hi)
    }
}

fn read_ascii_string(data: &[u8], offset: usize, count: usize) -> Option<String> {
    let end = offset.saturating_add(count);
    let slice = data.get(offset..end)?;
    // Trim trailing nulls
    let trimmed = slice
        .iter()
        .copied()
        .take_while(|&b| b != 0)
        .collect::<Vec<u8>>();
    String::from_utf8(trimmed).ok()
}

/// Parse IFD entries for EXIF tags.
fn parse_ifd_entries(
    data: &[u8],
    ifd_offset: usize,
    tiff_start: usize,
    le: bool,
    exif: &mut ExifData,
) {
    let entry_count = match read_u16(data, ifd_offset, le) {
        Some(c) => c as usize,
        None => return,
    };

    let entries_start = ifd_offset.saturating_add(2);

    for i in 0..entry_count.min(200) {
        let entry_offset = entries_start.saturating_add(i.saturating_mul(12));
        let tag = match read_u16(data, entry_offset, le) {
            Some(t) => t,
            None => continue,
        };
        let data_type = match read_u16(data, entry_offset.saturating_add(2), le) {
            Some(t) => t,
            None => continue,
        };
        let count = match read_u32(data, entry_offset.saturating_add(4), le) {
            Some(c) => c as usize,
            None => continue,
        };
        let value_offset_raw = entry_offset.saturating_add(8);

        match tag {
            // ImageWidth
            0x0100 => {
                if let Some(v) =
                    read_value_u32(data, value_offset_raw, tiff_start, le, data_type, count)
                {
                    exif.width = Some(v);
                }
            }
            // ImageHeight
            0x0101 => {
                if let Some(v) =
                    read_value_u32(data, value_offset_raw, tiff_start, le, data_type, count)
                {
                    exif.height = Some(v);
                }
            }
            // Make
            0x010F => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    exif.camera_make = Some(s);
                }
            }
            // Model
            0x0110 => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    exif.camera_model = Some(s);
                }
            }
            // Orientation
            0x0112 => {
                if let Some(v) = read_u16(data, value_offset_raw, le) {
                    exif.orientation = Some(v);
                }
            }
            // Software
            0x0131 => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    exif.software = Some(s);
                }
            }
            // Copyright
            0x8298 => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    exif.copyright = Some(s);
                }
            }
            // ExifIFD pointer — recurse into the Exif sub-IFD
            0x8769 => {
                if let Some(sub_offset) = read_u32(data, value_offset_raw, le) {
                    parse_ifd_entries(
                        data,
                        tiff_start.saturating_add(sub_offset as usize),
                        tiff_start,
                        le,
                        exif,
                    );
                }
            }
            // GPS IFD pointer
            0x8825 => {
                if let Some(sub_offset) = read_u32(data, value_offset_raw, le) {
                    parse_gps_ifd(
                        data,
                        tiff_start.saturating_add(sub_offset as usize),
                        tiff_start,
                        le,
                        exif,
                    );
                }
            }
            // ExposureTime
            0x829A => {
                if let Some((num, den)) =
                    read_rational(data, value_offset_raw, tiff_start, le, count)
                    && den != 0
                {
                    if num < den {
                        exif.shutter_speed = Some(format!("{num}/{den}"));
                    } else {
                        let secs = f64::from(num) / f64::from(den);
                        exif.shutter_speed = Some(format!("{secs:.1}"));
                    }
                }
            }
            // FNumber
            0x829D => {
                if let Some((num, den)) =
                    read_rational(data, value_offset_raw, tiff_start, le, count)
                    && den != 0
                {
                    exif.aperture = Some(num as f32 / den as f32);
                }
            }
            // ISO
            0x8827 => {
                if let Some(v) = read_u16(data, value_offset_raw, le) {
                    exif.iso = Some(u32::from(v));
                }
            }
            // DateTimeOriginal
            0x9003 => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    exif.date_taken = Some(s);
                }
            }
            // Flash
            0x9209 => {
                if let Some(v) = read_u16(data, value_offset_raw, le) {
                    exif.flash_fired = Some((v & 1) != 0);
                }
            }
            // FocalLength
            0x920A => {
                if let Some((num, den)) =
                    read_rational(data, value_offset_raw, tiff_start, le, count)
                    && den != 0
                {
                    exif.focal_length_mm = Some(num as f32 / den as f32);
                }
            }
            // ColorSpace
            0xA001 => {
                if let Some(v) = read_u16(data, value_offset_raw, le) {
                    exif.color_space = Some(match v {
                        1 => "sRGB".to_owned(),
                        0xFFFF => "Uncalibrated".to_owned(),
                        _ => format!("Unknown({v})"),
                    });
                }
            }
            // PixelXDimension
            0xA002 => {
                if let Some(v) =
                    read_value_u32(data, value_offset_raw, tiff_start, le, data_type, count)
                {
                    exif.width = Some(v);
                }
            }
            // PixelYDimension
            0xA003 => {
                if let Some(v) =
                    read_value_u32(data, value_offset_raw, tiff_start, le, data_type, count)
                {
                    exif.height = Some(v);
                }
            }
            // WhiteBalance
            0xA403 => {
                if let Some(v) = read_u16(data, value_offset_raw, le) {
                    exif.white_balance = Some(match v {
                        0 => "Auto".to_owned(),
                        1 => "Manual".to_owned(),
                        _ => format!("Unknown({v})"),
                    });
                }
            }
            // ExposureMode
            0xA402 => {
                if let Some(v) = read_u16(data, value_offset_raw, le) {
                    exif.exposure_program = Some(match v {
                        0 => "Auto".to_owned(),
                        1 => "Manual".to_owned(),
                        2 => "Auto Bracket".to_owned(),
                        _ => format!("Mode {v}"),
                    });
                }
            }
            // MeteringMode
            0x9207 => {
                if let Some(v) = read_u16(data, value_offset_raw, le) {
                    exif.metering_mode = Some(match v {
                        0 => "Unknown".to_owned(),
                        1 => "Average".to_owned(),
                        2 => "Center-weighted".to_owned(),
                        3 => "Spot".to_owned(),
                        4 => "Multi-spot".to_owned(),
                        5 => "Multi-segment".to_owned(),
                        6 => "Partial".to_owned(),
                        _ => format!("Other({v})"),
                    });
                }
            }
            // ExposureBiasValue
            0x9204 => {
                if let Some((num, den)) =
                    read_rational_signed(data, value_offset_raw, tiff_start, le, count)
                    && den != 0
                {
                    exif.exposure_bias = Some(num as f32 / den as f32);
                }
            }
            // LensModel
            0xA434 => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    exif.lens = Some(s);
                }
            }
            _ => {}
        }
    }
}

/// Parse GPS IFD entries.
fn parse_gps_ifd(data: &[u8], ifd_offset: usize, tiff_start: usize, le: bool, exif: &mut ExifData) {
    let entry_count = match read_u16(data, ifd_offset, le) {
        Some(c) => c as usize,
        None => return,
    };

    let entries_start = ifd_offset.saturating_add(2);
    let mut lat_ref: Option<char> = None;
    let mut lon_ref: Option<char> = None;
    let mut lat_vals: Option<(f64, f64, f64)> = None;
    let mut lon_vals: Option<(f64, f64, f64)> = None;

    for i in 0..entry_count.min(50) {
        let entry_offset = entries_start.saturating_add(i.saturating_mul(12));
        let tag = match read_u16(data, entry_offset, le) {
            Some(t) => t,
            None => continue,
        };
        let _data_type = read_u16(data, entry_offset.saturating_add(2), le);
        let count = match read_u32(data, entry_offset.saturating_add(4), le) {
            Some(c) => c as usize,
            None => continue,
        };
        let value_offset_raw = entry_offset.saturating_add(8);

        match tag {
            // GPSLatitudeRef
            1 => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    lat_ref = s.chars().next();
                }
            }
            // GPSLatitude
            2 => {
                lat_vals = read_gps_dms(data, value_offset_raw, tiff_start, le);
            }
            // GPSLongitudeRef
            3 => {
                if let Some(s) = read_value_string(data, value_offset_raw, tiff_start, le, count) {
                    lon_ref = s.chars().next();
                }
            }
            // GPSLongitude
            4 => {
                lon_vals = read_gps_dms(data, value_offset_raw, tiff_start, le);
            }
            // GPSAltitude
            6 => {
                if let Some(offset_val) = read_u32(data, value_offset_raw, le) {
                    let abs_offset = tiff_start.saturating_add(offset_val as usize);
                    if let (Some(num), Some(den)) = (
                        read_u32(data, abs_offset, le),
                        read_u32(data, abs_offset.saturating_add(4), le),
                    ) && den != 0
                    {
                        exif.gps_altitude = Some(num as f32 / den as f32);
                    }
                }
            }
            _ => {}
        }
    }

    // Convert DMS to decimal degrees
    if let Some((d, m, s)) = lat_vals {
        let mut dec = d + m / 60.0 + s / 3600.0;
        if lat_ref == Some('S') {
            dec = -dec;
        }
        exif.gps_latitude = Some(dec);
    }
    if let Some((d, m, s)) = lon_vals {
        let mut dec = d + m / 60.0 + s / 3600.0;
        if lon_ref == Some('W') {
            dec = -dec;
        }
        exif.gps_longitude = Some(dec);
    }
}

fn read_gps_dms(
    data: &[u8],
    value_offset: usize,
    tiff_start: usize,
    le: bool,
) -> Option<(f64, f64, f64)> {
    let offset_val = read_u32(data, value_offset, le)? as usize;
    let abs = tiff_start.saturating_add(offset_val);

    let d_num = f64::from(read_u32(data, abs, le)?);
    let d_den = f64::from(read_u32(data, abs.saturating_add(4), le)?);
    let m_num = f64::from(read_u32(data, abs.saturating_add(8), le)?);
    let m_den = f64::from(read_u32(data, abs.saturating_add(12), le)?);
    let s_num = f64::from(read_u32(data, abs.saturating_add(16), le)?);
    let s_den = f64::from(read_u32(data, abs.saturating_add(20), le)?);

    if d_den == 0.0 || m_den == 0.0 || s_den == 0.0 {
        return None;
    }

    Some((d_num / d_den, m_num / m_den, s_num / s_den))
}

fn read_value_string(
    data: &[u8],
    value_offset: usize,
    tiff_start: usize,
    le: bool,
    count: usize,
) -> Option<String> {
    if count <= 4 {
        // Value stored inline in the 4-byte value field
        read_ascii_string(data, value_offset, count)
    } else {
        // Value stored at an offset
        let offset_val = read_u32(data, value_offset, le)? as usize;
        read_ascii_string(data, tiff_start.saturating_add(offset_val), count)
    }
}

fn read_value_u32(
    data: &[u8],
    value_offset: usize,
    _tiff_start: usize,
    le: bool,
    data_type: u16,
    _count: usize,
) -> Option<u32> {
    match data_type {
        3 => read_u16(data, value_offset, le).map(u32::from), // SHORT
        4 => read_u32(data, value_offset, le),                // LONG
        _ => None,
    }
}

fn read_rational(
    data: &[u8],
    value_offset: usize,
    tiff_start: usize,
    le: bool,
    _count: usize,
) -> Option<(u32, u32)> {
    let offset_val = read_u32(data, value_offset, le)? as usize;
    let abs = tiff_start.saturating_add(offset_val);
    let num = read_u32(data, abs, le)?;
    let den = read_u32(data, abs.saturating_add(4), le)?;
    Some((num, den))
}

fn read_rational_signed(
    data: &[u8],
    value_offset: usize,
    tiff_start: usize,
    le: bool,
    _count: usize,
) -> Option<(i32, i32)> {
    let offset_val = read_u32(data, value_offset, le)? as usize;
    let abs = tiff_start.saturating_add(offset_val);
    let num = read_u32(data, abs, le)? as i32;
    let den = read_u32(data, abs.saturating_add(4), le)? as i32;
    Some((num, den))
}

// ============================================================================
// Image adjustments
// ============================================================================

/// Non-destructive image adjustments stored per photo.
#[derive(Clone, Debug)]
pub struct ImageAdjustments {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub exposure: f32,
    pub temperature: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub sharpness: f32,
    pub vignette: f32,
    pub rotation: i32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}

impl Default for ImageAdjustments {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            exposure: 0.0,
            temperature: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            sharpness: 0.0,
            vignette: 0.0,
            rotation: 0,
            flip_horizontal: false,
            flip_vertical: false,
        }
    }
}

impl ImageAdjustments {
    /// Check if all adjustments are at their defaults.
    pub fn is_default(&self) -> bool {
        (self.brightness - 0.0).abs() < f32::EPSILON
            && (self.contrast - 0.0).abs() < f32::EPSILON
            && (self.saturation - 0.0).abs() < f32::EPSILON
            && (self.exposure - 0.0).abs() < f32::EPSILON
            && (self.temperature - 0.0).abs() < f32::EPSILON
            && (self.highlights - 0.0).abs() < f32::EPSILON
            && (self.shadows - 0.0).abs() < f32::EPSILON
            && (self.sharpness - 0.0).abs() < f32::EPSILON
            && (self.vignette - 0.0).abs() < f32::EPSILON
            && self.rotation == 0
            && !self.flip_horizontal
            && !self.flip_vertical
    }

    /// Reset all adjustments to defaults.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Rotate 90 degrees clockwise.
    pub fn rotate_cw(&mut self) {
        self.rotation = (self.rotation.saturating_add(90)) % 360;
    }

    /// Rotate 90 degrees counter-clockwise.
    pub fn rotate_ccw(&mut self) {
        self.rotation = (self.rotation.saturating_add(270)) % 360;
    }
}

// ============================================================================
// Import identity: "have I already imported this exact file?"
// ============================================================================

/// An identity key for an imported file: its path and its size, hashed.
///
/// **This was called `ImportKey` and it is not one.** It is FNV-1a over
/// the path string with the file size mixed in, and this program never sees a
/// pixel -- its whole dependency list is three GUI crates, imports arrive as
/// `(name, format, size)` tuples, and nothing decodes an image anywhere.
///
/// The name mattered because of what was built on it. There were `distance`
/// and `is_similar(threshold)` methods computing Hamming distance between two
/// of these, and a `find_similar(photo, threshold)` that returned the photos
/// within a given distance -- presented as visual similarity. FNV avalanches:
/// two different paths differ in about half their bits whatever the pictures
/// look like, so that answer was noise with a plausible shape. The one test
/// exercising it passed a threshold of 64 out of 64 bits, which accepts
/// everything.
///
/// Those are deleted. What survives is what was actually being used: an
/// exact-equality check at import time, which answers "have I already imported
/// this exact path at this exact size" -- a real and useful question, and a
/// different one from "is this the same picture".
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImportKey {
    pub hash: u64,
}

impl ImportKey {
    /// Hash the path and size. **Not** a function of the image's contents.
    pub fn from_metadata(path: impl AsRef<std::path::Path>, file_size: u64) -> Self {
        // Simple FNV-1a hash of the path + size.
        //
        // Over the path's *bytes*, through `as_encoded_bytes`, rather than
        // over a UTF-8 rendering of it: two files whose names differ only in
        // bytes that are not text would otherwise hash alike and the second
        // would be refused as a duplicate of the first. For an ordinary
        // ASCII path these are the same bytes, so nothing already imported
        // changes its key.
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in path.as_ref().as_os_str().as_encoded_bytes().iter().copied() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= file_size;
        hash = hash.wrapping_mul(0x100000001b3);
        Self { hash }
    }

    /// Is this the same path at the same size?
    ///
    /// Exact equality, with no threshold. A threshold over a hash that is not
    /// a function of the image would let the caller ask how *nearly* two
    /// unrelated paths collide, which is a question with a confident answer
    /// and no meaning.
    pub fn same_file(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

// ============================================================================
// Face region (placeholder)
// ============================================================================

/// A detected face region within a photo.
#[derive(Clone, Debug)]
pub struct FaceRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub name: Option<String>,
    pub confidence: f32,
}

impl FaceRegion {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            name: None,
            confidence: 0.0,
        }
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }
}

// ============================================================================
// Photo
// ============================================================================

/// A single photo in the library.
#[derive(Clone, Debug)]
pub struct Photo {
    pub id: PhotoId,
    /// Where the photograph is, as bytes rather than as text.
    ///
    /// A `PathBuf`, not a `String`. This was a `String` built through
    /// `to_string_lossy`, which replaces any byte that is not UTF-8 with
    /// U+FFFD -- and a name on this OS may hold every byte but `/` and NUL.
    /// The result was a path nobody could open, saved into the library as
    /// though it were the real one, so the photograph failed to decode while
    /// pointing at a file that exists under a name this program had thrown
    /// away.
    pub file_path: std::path::PathBuf,
    pub file_name: String,
    pub file_size: u64,
    pub format: ImageFormat,
    pub date_added: u64,
    pub date_taken: Option<u64>,
    pub rating: u8,
    pub color_label: ColorLabel,
    pub tags: Vec<String>,
    pub exif: ExifData,
    pub adjustments: ImageAdjustments,
    pub faces: Vec<FaceRegion>,
    pub import_key: ImportKey,
    pub flagged: bool,
    pub hidden: bool,
}

impl Photo {
    /// Create a new photo entry.
    pub fn new(
        id: PhotoId,
        path: impl AsRef<std::path::Path>,
        name: &str,
        format: ImageFormat,
        size: u64,
        date_added: u64,
    ) -> Self {
        Self {
            id,
            file_path: path.as_ref().to_path_buf(),
            file_name: name.to_owned(),
            file_size: size,
            format,
            date_added,
            date_taken: None,
            rating: 0,
            color_label: ColorLabel::None,
            tags: Vec::new(),
            exif: ExifData::empty(),
            adjustments: ImageAdjustments::default(),
            faces: Vec::new(),
            import_key: ImportKey::from_metadata(path, size),
            flagged: false,
            hidden: false,
        }
    }

    /// Set star rating (clamped to 0-5).
    pub fn set_rating(&mut self, stars: u8) {
        self.rating = stars.min(MAX_STARS);
    }

    /// Add a tag if not already present.
    pub fn add_tag(&mut self, tag: &str) {
        let t = tag.to_owned();
        if !self.tags.contains(&t) {
            self.tags.push(t);
        }
    }

    /// Remove a tag.
    pub fn remove_tag(&mut self, tag: &str) -> bool {
        if let Some(pos) = self.tags.iter().position(|t| t == tag) {
            self.tags.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check if this photo matches a search query.
    pub fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        if self.file_name.to_lowercase().contains(&q) {
            return true;
        }
        // Lossy on purpose, and safe here in a way it is not elsewhere:
        // this compares for a match and never opens anything, so a byte that
        // is not text costs a search hit rather than a file.
        if self.file_path.to_string_lossy().to_lowercase().contains(&q) {
            return true;
        }
        for tag in &self.tags {
            if tag.to_lowercase().contains(&q) {
                return true;
            }
        }
        if let Some(ref make) = self.exif.camera_make
            && make.to_lowercase().contains(&q)
        {
            return true;
        }
        if let Some(ref model) = self.exif.camera_model
            && model.to_lowercase().contains(&q)
        {
            return true;
        }
        false
    }

    /// Get the file extension.
    pub fn extension(&self) -> &str {
        self.format.extension()
    }

    /// Human-readable file size.
    pub fn human_size(&self) -> String {
        guitk::bytes::iec(self.file_size)
    }
}

// ============================================================================
// Album
// ============================================================================

/// An album containing a curated set of photos.
#[derive(Clone, Debug)]
pub struct Album {
    pub id: AlbumId,
    pub name: String,
    pub description: String,
    pub cover_photo: Option<PhotoId>,
    pub photo_ids: Vec<PhotoId>,
    pub created_at: u64,
    pub modified_at: u64,
}

impl Album {
    pub fn new(id: AlbumId, name: &str, created_at: u64) -> Self {
        Self {
            id,
            name: name.to_owned(),
            description: String::new(),
            cover_photo: None,
            photo_ids: Vec::new(),
            created_at,
            modified_at: created_at,
        }
    }

    pub fn add_photo(&mut self, photo_id: PhotoId) {
        if !self.photo_ids.contains(&photo_id) {
            self.photo_ids.push(photo_id);
        }
    }

    pub fn remove_photo(&mut self, photo_id: PhotoId) -> bool {
        if let Some(pos) = self.photo_ids.iter().position(|&id| id == photo_id) {
            self.photo_ids.remove(pos);
            // Clear cover if it was this photo
            if self.cover_photo == Some(photo_id) {
                self.cover_photo = None;
            }
            true
        } else {
            false
        }
    }

    pub fn photo_count(&self) -> usize {
        self.photo_ids.len()
    }
}

// ============================================================================
// Smart album (rule-based)
// ============================================================================

/// A rule for smart albums that automatically filter photos.
#[derive(Clone, Debug, PartialEq)]
pub enum SmartRule {
    MinRating(u8),
    HasTag(String),
    HasColorLabel(ColorLabel),
    FormatIs(ImageFormat),
    IsFlagged,
    DateAfter(u64),
    DateBefore(u64),
    CameraMake(String),
    CameraModel(String),
}

impl SmartRule {
    pub fn matches(&self, photo: &Photo) -> bool {
        match self {
            Self::MinRating(min) => photo.rating >= *min,
            Self::HasTag(tag) => photo.tags.iter().any(|t| t == tag),
            Self::HasColorLabel(label) => photo.color_label == *label,
            Self::FormatIs(fmt) => photo.format == *fmt,
            Self::IsFlagged => photo.flagged,
            Self::DateAfter(ts) => photo.date_added > *ts,
            Self::DateBefore(ts) => photo.date_added < *ts,
            Self::CameraMake(make) => photo
                .exif
                .camera_make
                .as_ref()
                .is_some_and(|m| m.to_lowercase().contains(&make.to_lowercase())),
            Self::CameraModel(model) => photo
                .exif
                .camera_model
                .as_ref()
                .is_some_and(|m| m.to_lowercase().contains(&model.to_lowercase())),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::MinRating(n) => format!("Rating >= {n}"),
            Self::HasTag(t) => format!("Tag: {t}"),
            Self::HasColorLabel(c) => format!("Color: {}", c.label()),
            Self::FormatIs(f) => format!("Format: {}", f.label()),
            Self::IsFlagged => "Flagged".to_owned(),
            Self::DateAfter(ts) => format!("After {ts}"),
            Self::DateBefore(ts) => format!("Before {ts}"),
            Self::CameraMake(m) => format!("Camera: {m}"),
            Self::CameraModel(m) => format!("Model: {m}"),
        }
    }
}

/// A smart album that automatically includes matching photos.
#[derive(Clone, Debug)]
pub struct SmartAlbum {
    pub id: AlbumId,
    pub name: String,
    pub rules: Vec<SmartRule>,
    pub match_all: bool,
}

impl SmartAlbum {
    pub fn new(id: AlbumId, name: &str, match_all: bool) -> Self {
        Self {
            id,
            name: name.to_owned(),
            rules: Vec::new(),
            match_all,
        }
    }

    pub fn add_rule(&mut self, rule: SmartRule) {
        self.rules.push(rule);
    }

    pub fn matches(&self, photo: &Photo) -> bool {
        if self.rules.is_empty() {
            return false;
        }
        if self.match_all {
            self.rules.iter().all(|r| r.matches(photo))
        } else {
            self.rules.iter().any(|r| r.matches(photo))
        }
    }
}

// ============================================================================
// Sort options
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhotoSort {
    DateAdded,
    DateTaken,
    FileName,
    FileSize,
    Rating,
}

impl PhotoSort {
    pub fn label(self) -> &'static str {
        match self {
            Self::DateAdded => "Date Added",
            Self::DateTaken => "Date Taken",
            Self::FileName => "Name",
            Self::FileSize => "Size",
            Self::Rating => "Rating",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::DateAdded => Self::DateTaken,
            Self::DateTaken => Self::FileName,
            Self::FileName => Self::FileSize,
            Self::FileSize => Self::Rating,
            Self::Rating => Self::DateAdded,
        }
    }
}

// ============================================================================
// View mode
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Grid,
    Single,
    Timeline,
    Slideshow,
}

impl ViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::Single => "Single",
            Self::Timeline => "Timeline",
            Self::Slideshow => "Slideshow",
        }
    }
}

// ============================================================================
// Slideshow transition
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideshowTransition {
    None,
    Fade,
    SlideLeft,
    SlideRight,
    Dissolve,
    Zoom,
}

impl SlideshowTransition {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Fade => "Fade",
            Self::SlideLeft => "Slide Left",
            Self::SlideRight => "Slide Right",
            Self::Dissolve => "Dissolve",
            Self::Zoom => "Zoom",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Fade,
            Self::Fade => Self::SlideLeft,
            Self::SlideLeft => Self::SlideRight,
            Self::SlideRight => Self::Dissolve,
            Self::Dissolve => Self::Zoom,
            Self::Zoom => Self::None,
        }
    }
}

// ============================================================================
// Slideshow state
// ============================================================================

#[derive(Clone, Debug)]
pub struct SlideshowState {
    pub photo_ids: Vec<PhotoId>,
    pub current_index: usize,
    pub interval_ms: u64,
    pub transition: SlideshowTransition,
    pub paused: bool,
    pub shuffle: bool,
    pub elapsed_ms: u64,
}

impl SlideshowState {
    pub fn new(photo_ids: Vec<PhotoId>) -> Self {
        Self {
            photo_ids,
            current_index: 0,
            interval_ms: SLIDESHOW_DEFAULT_INTERVAL_MS,
            transition: SlideshowTransition::Fade,
            paused: false,
            shuffle: false,
            elapsed_ms: 0,
        }
    }

    pub fn current_photo(&self) -> Option<PhotoId> {
        self.photo_ids.get(self.current_index).copied()
    }

    pub fn advance(&mut self) {
        if !self.photo_ids.is_empty() {
            self.current_index = self
                .current_index
                .saturating_add(1)
                .checked_rem(self.photo_ids.len())
                .unwrap_or(0);
        }
    }

    pub fn go_back(&mut self) {
        if !self.photo_ids.is_empty() {
            if self.current_index == 0 {
                self.current_index = self.photo_ids.len().saturating_sub(1);
            } else {
                self.current_index = self.current_index.saturating_sub(1);
            }
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }
}

// ============================================================================
// Export options
// ============================================================================

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub format: ImageFormat,
    pub quality: u8,
    pub max_dimension: Option<u32>,
    pub strip_exif: bool,
    pub destination_dir: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ImageFormat::Jpeg,
            quality: 90,
            max_dimension: None,
            strip_exif: false,
            destination_dir: "/home/exports".to_owned(),
        }
    }
}

// ============================================================================
// Import result
// ============================================================================

#[derive(Clone, Debug)]
pub struct ImportResult {
    pub imported_count: usize,
    pub skipped_count: usize,
    pub error_count: usize,
    pub duplicate_count: usize,
    pub total_size: u64,
}

impl ImportResult {
    pub fn empty() -> Self {
        Self {
            imported_count: 0,
            skipped_count: 0,
            error_count: 0,
            duplicate_count: 0,
            total_size: 0,
        }
    }
}

// ============================================================================
// Active panel
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivePanel {
    Sidebar,
    PhotoGrid,
    InfoPanel,
}

// ============================================================================
// Sidebar selection
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarItem {
    AllPhotos,
    Favorites,
    RecentImports,
    Album(AlbumId),
    SmartAlbum(AlbumId),
    Trash,
}

// ============================================================================
// Main application
// ============================================================================

/// The photo manager application.
/// The image id the single-photo view draws under.
///
/// One fixed number rather than one per photograph, because exactly one
/// picture is on screen here. A second id would buy nothing and cost a
/// lifecycle: something would have to release the ids of photographs that
/// have scrolled out of view, and nothing in this application is watching for
/// that. The grid's thumbnails will need such a pool; this view does not, and
/// borrowing the complexity early would be paying for it twice.
const PHOTO_IMAGE_ID: u64 = 1;

/// What the compositor is currently holding under [`PHOTO_IMAGE_ID`].
///
/// The pixels are deliberately not here. They are moved into the upload queue
/// and thence to the compositor, which is the only thing that draws them; a
/// retained copy would double this application's memory for a 24-megapixel
/// photograph -- about 96 MB in this form -- to serve a reader that does not
/// exist. What is kept is the pair of numbers the layout actually needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShownPicture {
    /// The photograph it was decoded from, so a stale upload is never drawn
    /// under a new selection.
    photo: PhotoId,
    width: u32,
    height: u32,
}

/// A file's modification time, in seconds since the epoch, or zero.
///
/// Zero for a file that cannot be stat'ed, and that is a usable key rather
/// than a failure: the thumbnail cached under it is invalidated the moment the
/// file becomes readable and reports a real time, because the cache key
/// carries the time it was made with.
fn file_mtime(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// The largest box with `w`:`h` proportions that fits within `max_w`/`max_h`.
///
/// Never enlarges. A small picture stretched to fill the pane is blurred in a
/// way that reads as a fault in the decoder rather than as a small file, and
/// the photograph's real size is information this view should not destroy.
/// Photographs are almost always larger than the pane, so the clamp bites
/// rarely and only where it helps.
#[allow(
    clippy::cast_precision_loss,
    reason = "a pixel dimension is exact in f32 far beyond any real sensor"
)]
fn fit_within(w: u32, h: u32, max_w: f32, max_h: f32) -> (f32, f32) {
    if w == 0 || h == 0 {
        // Not reachable through `imagecodec`, which refuses a zero dimension,
        // but the alternative to saying so is a division by zero.
        return (max_w, max_h);
    }
    let (w, h) = (w as f32, h as f32);
    let scale = (max_w / w).min(max_h / h).min(1.0);
    (w * scale, h * scale)
}

pub struct PhotoApp {
    pub photos: Vec<Photo>,
    pub albums: Vec<Album>,
    pub smart_albums: Vec<SmartAlbum>,
    pub trash: Vec<Photo>,
    pub selected_photo: Option<PhotoId>,
    pub selected_photos: Vec<PhotoId>,
    pub sidebar_selection: SidebarItem,
    pub view_mode: ViewMode,
    pub sort_order: PhotoSort,
    pub search_query: String,
    pub active_panel: ActivePanel,
    pub thumb_size_idx: usize,
    /// First row of thumbnails drawn in the grid.
    ///
    /// A row index rather than a pixel offset: the grid draws whole rows, so a
    /// pixel offset could only express positions the renderer then rounds
    /// away.
    pub grid_scroll: usize,
    pub show_info_panel: bool,
    pub slideshow: Option<SlideshowState>,
    pub export_options: ExportOptions,
    pub window_width: f32,
    pub window_height: f32,
    /// The open picker. Holds the dialog and the routing thirteen
    /// applications used to write out by hand.
    pub picker: FilePicker,
    /// What the last import attempt did, shown in the status bar.
    ///
    /// Carries the failure too. An import that silently does nothing is the
    /// defect this whole application was an instance of, so a file that cannot
    /// be read says so rather than leaving the grid unchanged and unexplained.
    /// The line the status bar shows, when there is something to say.
    ///
    /// Called `last_import` until it carried anything but an import.
    pub status_message: Option<String>,
    photo_id_gen: IdGen,
    album_id_gen: IdGen,
    timestamp_counter: u64,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Pictures waiting to go to the compositor, drained by `App::take_images`.
    pending_images: Vec<app::ImageChange>,
    /// The photograph whose pixels are uploaded, once it has been decoded.
    shown_picture: Option<ShownPicture>,
    /// The photograph the two fields above were computed for, whether that
    /// ended in a picture or in a reason.
    ///
    /// Separate from `shown_picture` because a failed decode has to be
    /// remembered too. Without it, a photograph that cannot be decoded would
    /// be retried on every single frame -- reading and failing to decode the
    /// file sixty times a second for as long as it stayed selected.
    picture_for: Option<PhotoId>,
    /// Why the selected photograph is not on screen, when it is not.
    ///
    /// Shown in place of its dimensions. A failure that left the card blank
    /// would be indistinguishable from one that had not been attempted yet.
    picture_error: Option<String>,
    /// The grid's thumbnails, and the image ids they are drawn under.
    thumb_cache: thumbs::ThumbnailCache,
    /// The queue that turns files into those thumbnails, a few per frame.
    thumb_gen: thumbs::ThumbnailGenerator,
    /// Which photographs have a thumbnail ready, and under what.
    ///
    /// The modification time is kept beside the id because the cache is keyed
    /// on it, and re-reading it from the disk to draw a frame would be a stat
    /// per visible card per frame.
    thumb_ready: HashMap<PhotoId, (u64, u64)>,
    /// Thumbnails waiting to go to the compositor.
    thumb_uploads: Vec<(u64, thumbs::Thumbnail)>,
    /// What the queue was last filled for.
    ///
    /// Requests are queued when the visible set changes, not every frame:
    /// pushing the same request sixty times a second would grow the queue
    /// without bound and starve the cards actually on screen behind it.
    thumb_queued_for: Option<u64>,
    /// Where the library is saved, or `None` for a library that is not saved
    /// at all -- which is what every test gets unless it asks otherwise.
    library_path: Option<std::path::PathBuf>,
    /// The exact text last written, so a save happens only when something
    /// changed.
    ///
    /// The whole serialization rather than a hash of it: a hash would be
    /// smaller and would make two different libraries compare equal once in a
    /// very long while, and the cost of that coincidence is a save that never
    /// happens. A few hundred kilobytes is the cheaper side of that trade.
    last_written: Option<String>,
    /// Why the library could not be loaded or saved, when it could not.
    library_note: Option<String>,
    /// Records the file held that this build could not read.
    ///
    /// **Saving is refused while this is non-zero.** A file with one corrupt
    /// line loads every other photograph; writing that back would delete the
    /// corrupt one permanently, turning a line somebody could still repair by
    /// hand into nothing at all.
    library_unread: usize,
    /// Whether typing goes to the search box.
    ///
    /// Without this the digits would still rate the selected photograph and
    /// `f` would still flag it, so searching for "flag5" would silently change
    /// the library while the user thought they were typing.
    search_focused: bool,
    /// The name being typed for a new album, when one is being made.
    ///
    /// `None` when nothing is being named. An empty `Some` is a row waiting
    /// for its first character, which is why this is not just a `String`.
    naming_album: Option<String>,
    /// The album menu, while it is open.
    ///
    /// Rebuilt on every opening rather than kept and updated, because its
    /// items are the albums and those change underneath it. A menu holding a
    /// name the library no longer has is worse than one that costs a few
    /// allocations to raise.
    photo_menu: Option<ContextMenu>,
    /// The photograph the open menu is about.
    ///
    /// Remembered rather than read from the selection when the menu is
    /// clicked: a menu is a question about the thing you opened it on, and
    /// nothing should be able to move the answer while it is up.
    menu_photo: Option<PhotoId>,
}

impl Default for PhotoApp {
    fn default() -> Self {
        Self::new()
    }
}

impl PhotoApp {
    /// Create a new empty photo manager.
    pub fn new() -> Self {
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            pending_images: Vec::new(),
            shown_picture: None,
            picture_for: None,
            picture_error: None,
            thumb_cache: thumbs::ThumbnailCache::default_capacity(),
            thumb_gen: thumbs::ThumbnailGenerator::new(),
            thumb_ready: HashMap::new(),
            thumb_uploads: Vec::new(),
            thumb_queued_for: None,
            library_path: None,
            last_written: None,
            library_note: None,
            library_unread: 0,
            search_focused: false,
            naming_album: None,
            photo_menu: None,
            menu_photo: None,
            photos: Vec::new(),
            albums: Vec::new(),
            smart_albums: Vec::new(),
            trash: Vec::new(),
            selected_photo: None,
            selected_photos: Vec::new(),
            sidebar_selection: SidebarItem::AllPhotos,
            view_mode: ViewMode::Grid,
            sort_order: PhotoSort::DateAdded,
            search_query: String::new(),
            active_panel: ActivePanel::PhotoGrid,
            thumb_size_idx: 1,
            grid_scroll: 0,
            show_info_panel: true,
            slideshow: None,
            export_options: ExportOptions::default(),
            window_width: 1400.0,
            window_height: 900.0,
            picker: FilePicker::new(),
            status_message: None,
            photo_id_gen: IdGen::new(1),
            album_id_gen: IdGen::new(1),
            timestamp_counter: 1000,
        }
    }

    fn tick(&mut self) -> u64 {
        self.timestamp_counter = self.timestamp_counter.saturating_add(1);
        self.timestamp_counter
    }

    // -----------------------------------------------------------------------
    // Photo management
    // -----------------------------------------------------------------------

    /// Import a photo into the library.
    pub fn import_photo(
        &mut self,
        path: impl AsRef<std::path::Path>,
        name: &str,
        format: ImageFormat,
        size: u64,
    ) -> PhotoId {
        let ts = self.tick();
        let id = self.photo_id_gen.next_id();
        let photo = Photo::new(id, path, name, format, size, ts);
        self.photos.push(photo);
        id
    }

    /// Import a photo with EXIF data.
    pub fn import_photo_with_exif(
        &mut self,
        path: impl AsRef<std::path::Path>,
        name: &str,
        format: ImageFormat,
        size: u64,
        exif: ExifData,
    ) -> PhotoId {
        let id = self.import_photo(path.as_ref(), name, format, size);
        if let Some(photo) = self.find_photo_mut(id) {
            photo.exif = exif;
        }
        id
    }

    /// Simulate importing from a directory, returning results.
    /// Import a caller-supplied list of files, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. It has had no production caller since
    /// this app gained a real file picker and a real EXIF read earlier the
    /// same day -- `import_photo` is the live path. Left `pub` and ungated it
    /// looked like production API, which is what `find-reachable-fixtures.py`
    /// reported it as.
    ///
    /// Note that it never invented anything itself: the caller supplies the
    /// names, formats and sizes. That is why it is a fixture rather than a
    /// fabrication -- it makes no claim of its own.
    #[cfg(test)]
    pub fn simulate_import(
        &mut self,
        dir: &str,
        files: &[(&str, ImageFormat, u64)],
    ) -> ImportResult {
        let mut result = ImportResult::empty();
        for (name, format, size) in files {
            let path = format!("{dir}/{name}");
            // Already imported? Same path, same size -- not "same picture",
            // which this program has no way to determine.
            let key = ImportKey::from_metadata(&path, *size);
            let is_dup = self.photos.iter().any(|p| p.import_key.same_file(&key));
            if is_dup {
                result.duplicate_count = result.duplicate_count.saturating_add(1);
                result.skipped_count = result.skipped_count.saturating_add(1);
                continue;
            }
            self.import_photo(&path, name, *format, *size);
            result.imported_count = result.imported_count.saturating_add(1);
            result.total_size = result.total_size.saturating_add(*size);
        }
        result
    }

    /// Find a photo by ID.
    /// An application whose library is saved to `path`, and loaded from it now.
    #[must_use]
    pub fn with_storage(path: std::path::PathBuf) -> Self {
        let mut app = Self::new();
        app.library_path = Some(path);
        app.load_library();
        app
    }

    /// Read the library file, if there is one to read.
    ///
    /// A missing file is not an error: it is what a first run looks like, and
    /// saying so would be an alarm about the ordinary case.
    fn load_library(&mut self) {
        let Some(path) = self.library_path.clone() else {
            return;
        };
        let text = match safeio::read_to_string_capped(&path, Self::MAX_LIBRARY_BYTES) {
            Ok(read) if read.truncated => {
                self.library_note = Some(format!(
                    "the library file is larger than {} MiB and was not read",
                    Self::MAX_LIBRARY_BYTES / (1024 * 1024)
                ));
                // Nothing was loaded, so everything is unread; refusing to save
                // is exactly right.
                self.library_unread = 1;
                return;
            }
            Ok(read) => read.text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.library_note = Some(format!("could not read the library: {e}"));
                self.library_unread = 1;
                return;
            }
        };
        match library::parse(&text) {
            Ok(loaded) => {
                self.library_unread = loaded.skipped;
                if loaded.skipped > 0 {
                    self.library_note = Some(format!(
                        "{} record(s) in the library could not be read; \
                         it will not be overwritten",
                        loaded.skipped
                    ));
                }
                self.photo_id_gen = IdGen::new(
                    loaded
                        .photos
                        .iter()
                        .map(|p| p.id)
                        .max()
                        .map_or(1, |m| m.saturating_add(1)),
                );
                self.photos = loaded.photos;
                // What is on disk is what is in memory, so nothing is owed
                // until the user changes something.
                self.last_written = Some(library::serialize(&self.photos));
            }
            Err(e) => {
                self.library_note = Some(format!("could not read the library: {e}"));
                self.library_unread = 1;
            }
        }
    }

    /// Write the library, but only if it differs from what is already there.
    ///
    /// Called after every event rather than from each of the dozen places that
    /// change something. A flag set at each call site is one `self.dirty =
    /// true` away from losing a change silently, and the failure is invisible
    /// until someone notices their ratings did not survive a restart; a
    /// comparison against the bytes last written cannot be forgotten.
    fn persist_if_changed(&mut self) {
        let Some(path) = self.library_path.clone() else {
            return;
        };
        if self.library_unread > 0 {
            // See `library_unread`: never overwrite a file we could not read
            // in full.
            return;
        }
        let text = library::serialize(&self.photos);
        if self.last_written.as_deref() == Some(text.as_str()) {
            return;
        }
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            self.library_note = Some(format!("could not save the library: {e}"));
            return;
        }
        match safeio::write_atomically(&path, text.as_bytes()) {
            Ok(()) => {
                self.last_written = Some(text);
                self.library_note = None;
            }
            Err(e) => {
                self.library_note = Some(format!("could not save the library: {e}"));
            }
        }
    }

    /// The most bytes of library file to read.
    ///
    /// A library of a hundred thousand photographs is a few tens of megabytes
    /// of text, so this is generous; the point is that a file which has been
    /// corrupted into something enormous cannot be read into memory whole
    /// before anything objects.
    const MAX_LIBRARY_BYTES: usize = 64 * 1024 * 1024;

    /// Thumbnails generated per frame.
    ///
    /// Generation is synchronous -- `thumbs` has no worker thread -- so this
    /// is a frame budget, not a rate. Small, because the cards it fills are
    /// already on screen: four per frame fills a screenful in well under a
    /// second while leaving each frame short enough to stay smooth, where one
    /// batch of forty would be a visible stall on the first scroll.
    const THUMB_BATCH: usize = 4;

    /// A number that changes when the grid would draw a different set of
    /// cards.
    ///
    /// Cheap and deliberately approximate: it may change when the visible set
    /// has not (a sort that reorders identical photographs), which costs one
    /// wasted pass over an already-full cache. The opposite error -- missing a
    /// change -- would leave cards blank until something unrelated moved.
    fn thumb_fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for pid in self.visible_photos() {
            hash ^= pid;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        for part in [
            self.grid_scroll as u64,
            self.thumb_size_idx as u64,
            u64::from(self.view_mode == ViewMode::Grid),
        ] {
            hash ^= part;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    /// Queue a thumbnail for every photograph the grid will draw one for.
    ///
    /// Anything already in the cache is skipped: the key carries the file's
    /// modification time and size, so a hit is a hit on *this* version of the
    /// file and a miss after an edit is automatic.
    fn queue_thumbnails(&mut self) {
        self.thumb_gen.cancel_all();
        if self.view_mode != ViewMode::Grid {
            return;
        }
        let config = self.thumb_config();
        let wanted: Vec<PhotoId> = self.visible_photos();
        for pid in wanted {
            let Some(photo) = self.find_photo(pid) else {
                continue;
            };
            let path = photo.file_path.clone();
            let size = photo.file_size;
            // Read once here rather than per frame: this runs when the visible
            // set changes, which is a scroll or a filter, not a repaint.
            let mtime = file_mtime(&path);
            if self.thumb_cache.peek(&path, mtime, size).is_some() {
                self.thumb_ready
                    .insert(pid, (mtime, thumbs::image_id(&path, mtime, size)));
                continue;
            }
            self.thumb_gen.push(thumbs::ThumbnailRequest {
                path,
                mtime,
                size,
                config: config.clone(),
            });
        }
    }

    /// How a thumbnail should look: the user's colours, at the grid's size.
    fn thumb_config(&self) -> thumbs::ThumbConfig {
        thumbs::ThumbConfig {
            #[allow(
                clippy::cast_sign_loss,
                reason = "every entry of THUMB_SIZES is a positive constant"
            )]
            size: self.current_thumb_size() as u32,
            bg_color: self.palette.surface0,
            text_color: self.palette.text,
            ..thumbs::ThumbConfig::default()
        }
    }

    /// Generate a few queued thumbnails and file the results.
    ///
    /// Each result lands in three places: the cache the renderer reads, the
    /// upload list the compositor needs, and `thumb_ready`, which is what says
    /// a card may stop drawing its placeholder.
    fn pump_thumbnails(&mut self) {
        self.thumb_gen.process_batch(Self::THUMB_BATCH);
        for (req, thumb) in self.thumb_gen.take_completed() {
            let id = thumbs::image_id(&req.path, req.mtime, req.size);
            let owner = self
                .photos
                .iter()
                .find(|p| p.file_path == req.path)
                .map(|p| p.id);
            self.thumb_uploads.push((id, thumb.clone()));
            self.thumb_cache
                .insert(&req.path, req.mtime, req.size, thumb);
            if let Some(pid) = owner {
                self.thumb_ready.insert(pid, (req.mtime, id));
            }
        }
    }

    /// Keep the grid's thumbnails in step with what it is about to draw.
    ///
    /// Called from `render`, for the same reason `sync_picture` is: uploads
    /// queued here are drained between this frame's render and its submit, so
    /// a thumbnail generated now is held by the compositor before the frame
    /// naming it arrives. That is what lets this draw a card's picture on the
    /// frame it was generated, with no "generated but not yet uploaded" state
    /// to carry -- a distinction the file manager does have to make, because
    /// its uploads travel a different path.
    fn sync_thumbnails(&mut self) {
        let fingerprint = self.thumb_fingerprint();
        if self.thumb_queued_for != Some(fingerprint) {
            self.thumb_queued_for = Some(fingerprint);
            self.queue_thumbnails();
        }
        self.pump_thumbnails();
    }

    /// The most bytes of picture file to read.
    ///
    /// Generous -- a lossless photograph from a full-frame sensor runs to tens
    /// of megabytes -- and the point is not the number but that there is one.
    const MAX_PICTURE_BYTES: usize = 256 * 1024 * 1024;

    /// Decode the selected photograph, unless that is already what is uploaded.
    ///
    /// **Called from `render`, and it has to be.** `App::take_images` is
    /// drained *between* the render and the submit, so a picture queued here
    /// reaches the compositor in time for the very frame that names it.
    /// Queued from an event handler it would also work, but every one of the
    /// five places that move the selection would have to remember to do it.
    /// Queued from `take_images` itself it would be uploaded after the frame
    /// that wanted it and would not appear until something unrelated asked for
    /// another -- the photograph would show up when the mouse next moved.
    ///
    /// Cheap when nothing has changed: one `Option<PhotoId>` comparison.
    fn sync_picture(&mut self) {
        if self.picture_for == self.selected_photo {
            return;
        }
        self.picture_for = self.selected_photo;
        self.shown_picture = None;
        self.picture_error = None;

        let Some(pid) = self.selected_photo else {
            return;
        };
        // Copied out so the borrow of `self.photos` ends here; everything
        // below writes back to `self`.
        let Some(path) = self.find_photo(pid).map(|p| p.file_path.clone()) else {
            return;
        };

        let read = match safeio::read_capped(&path, Self::MAX_PICTURE_BYTES) {
            Ok(read) => read,
            Err(e) => {
                self.picture_error = Some(format!("could not be read: {e}"));
                return;
            }
        };
        if read.truncated {
            // Refused rather than decoded. A picture's tail is not optional --
            // a JPEG's scan runs to the last byte of the file -- so a cut file
            // decodes to something that is not the photograph, and would then
            // be shown without a word about it.
            self.picture_error = Some(format!(
                "is larger than {} MiB",
                Self::MAX_PICTURE_BYTES / (1024 * 1024)
            ));
            return;
        }
        let image = match imagecodec::decode(&read.bytes, imagecodec::Limits::default()) {
            Ok(image) => image,
            Err(e) => {
                self.picture_error = Some(format!("could not be decoded: {e}"));
                return;
            }
        };

        self.shown_picture = Some(ShownPicture {
            photo: pid,
            width: image.width,
            height: image.height,
        });
        // One id for whatever is on screen, so this replaces the last upload
        // rather than queueing behind it. Anything still waiting here has been
        // overtaken by this selection and has no frame left to appear in.
        self.pending_images.clear();
        self.pending_images.push(app::ImageChange::Upload {
            id: PHOTO_IMAGE_ID,
            width: image.width,
            height: image.height,
            stride: image.stride(),
            format: oswindow::PixelFormat::Argb8888,
            bytes: guitk::canvas::WireBytes::from_le_argb(&image.pixels),
        });
    }

    pub fn find_photo(&self, id: PhotoId) -> Option<&Photo> {
        self.photos.iter().find(|p| p.id == id)
    }

    /// Find a photo by ID (mutable).
    pub fn find_photo_mut(&mut self, id: PhotoId) -> Option<&mut Photo> {
        self.photos.iter_mut().find(|p| p.id == id)
    }

    /// Delete a photo (move to trash).
    pub fn trash_photo(&mut self, id: PhotoId) -> bool {
        if let Some(pos) = self.photos.iter().position(|p| p.id == id) {
            let photo = self.photos.remove(pos);
            self.trash.push(photo);
            // Remove from albums
            for album in &mut self.albums {
                album.remove_photo(id);
            }
            if self.selected_photo == Some(id) {
                self.selected_photo = None;
            }
            self.selected_photos.retain(|&pid| pid != id);
            true
        } else {
            false
        }
    }

    /// Restore a photo from trash.
    pub fn restore_from_trash(&mut self, id: PhotoId) -> bool {
        if let Some(pos) = self.trash.iter().position(|p| p.id == id) {
            let photo = self.trash.remove(pos);
            self.photos.push(photo);
            true
        } else {
            false
        }
    }

    /// Permanently delete from trash.
    pub fn empty_trash(&mut self) -> usize {
        let count = self.trash.len();
        self.trash.clear();
        count
    }

    /// Rate a photo.
    pub fn rate_photo(&mut self, id: PhotoId, stars: u8) -> bool {
        if let Some(photo) = self.find_photo_mut(id) {
            photo.set_rating(stars);
            true
        } else {
            false
        }
    }

    /// Set color label.
    pub fn set_color_label(&mut self, id: PhotoId, label: ColorLabel) -> bool {
        if let Some(photo) = self.find_photo_mut(id) {
            photo.color_label = label;
            true
        } else {
            false
        }
    }

    /// Toggle flagged status.
    pub fn toggle_flag(&mut self, id: PhotoId) -> bool {
        if let Some(photo) = self.find_photo_mut(id) {
            photo.flagged = !photo.flagged;
            true
        } else {
            false
        }
    }

    /// Add tag to a photo.
    pub fn add_tag(&mut self, id: PhotoId, tag: &str) -> bool {
        if let Some(photo) = self.find_photo_mut(id) {
            photo.add_tag(tag);
            true
        } else {
            false
        }
    }

    /// Remove tag from a photo.
    pub fn remove_tag(&mut self, id: PhotoId, tag: &str) -> bool {
        if let Some(photo) = self.find_photo_mut(id) {
            photo.remove_tag(tag)
        } else {
            false
        }
    }

    /// Get all unique tags.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .photos
            .iter()
            .flat_map(|p| p.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Batch rate multiple photos.
    pub fn batch_rate(&mut self, ids: &[PhotoId], stars: u8) -> usize {
        let mut count = 0usize;
        for &id in ids {
            if self.rate_photo(id, stars) {
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Batch tag multiple photos.
    pub fn batch_tag(&mut self, ids: &[PhotoId], tag: &str) -> usize {
        let mut count = 0usize;
        for &id in ids {
            if self.add_tag(id, tag) {
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Batch move to album.
    pub fn batch_add_to_album(&mut self, photo_ids: &[PhotoId], album_id: AlbumId) -> bool {
        if let Some(album) = self.albums.iter_mut().find(|a| a.id == album_id) {
            for &pid in photo_ids {
                album.add_photo(pid);
            }
            true
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Album management
    // -----------------------------------------------------------------------

    /// Create a new album.
    pub fn create_album(&mut self, name: &str) -> AlbumId {
        let ts = self.tick();
        let id = self.album_id_gen.next_id();
        self.albums.push(Album::new(id, name, ts));
        id
    }

    /// Delete an album.
    pub fn delete_album(&mut self, id: AlbumId) -> bool {
        let len_before = self.albums.len();
        self.albums.retain(|a| a.id != id);
        self.albums.len() < len_before
    }

    /// Rename an album.
    pub fn rename_album(&mut self, id: AlbumId, new_name: &str) -> bool {
        if let Some(album) = self.albums.iter_mut().find(|a| a.id == id) {
            album.name = new_name.to_owned();
            true
        } else {
            false
        }
    }

    /// Add a photo to an album.
    pub fn add_to_album(&mut self, album_id: AlbumId, photo_id: PhotoId) -> bool {
        if let Some(album) = self.albums.iter_mut().find(|a| a.id == album_id) {
            album.add_photo(photo_id);
            true
        } else {
            false
        }
    }

    /// Remove a photo from an album.
    pub fn remove_from_album(&mut self, album_id: AlbumId, photo_id: PhotoId) -> bool {
        if let Some(album) = self.albums.iter_mut().find(|a| a.id == album_id) {
            album.remove_photo(photo_id)
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Smart albums
    // -----------------------------------------------------------------------

    /// Create a smart album.
    pub fn create_smart_album(&mut self, name: &str, match_all: bool) -> AlbumId {
        let id = self.album_id_gen.next_id();
        self.smart_albums.push(SmartAlbum::new(id, name, match_all));
        id
    }

    /// Add a rule to a smart album.
    pub fn add_smart_rule(&mut self, album_id: AlbumId, rule: SmartRule) -> bool {
        if let Some(album) = self.smart_albums.iter_mut().find(|a| a.id == album_id) {
            album.add_rule(rule);
            true
        } else {
            false
        }
    }

    /// Get photos matching a smart album's rules.
    pub fn smart_album_photos(&self, album_id: AlbumId) -> Vec<PhotoId> {
        if let Some(album) = self.smart_albums.iter().find(|a| a.id == album_id) {
            self.photos
                .iter()
                .filter(|p| album.matches(p))
                .map(|p| p.id)
                .collect()
        } else {
            Vec::new()
        }
    }

    // -----------------------------------------------------------------------
    // Duplicate detection
    // -----------------------------------------------------------------------

    /// Group photos that share a path and a size.
    ///
    /// Not "the same picture": nothing here reads a pixel. Two copies of one
    /// photograph under different names are two photos to this program, and
    /// saying otherwise would need an image decoder it does not have.
    pub fn find_duplicates(&self) -> Vec<Vec<PhotoId>> {
        let mut hash_groups: HashMap<u64, Vec<PhotoId>> = HashMap::new();
        for photo in &self.photos {
            hash_groups
                .entry(photo.import_key.hash)
                .or_default()
                .push(photo.id);
        }
        hash_groups
            .into_values()
            .filter(|group| group.len() > 1)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Filtering and sorting
    // -----------------------------------------------------------------------

    /// Get visible photos based on current sidebar selection and filters.
    pub fn visible_photos(&self) -> Vec<PhotoId> {
        let mut photos: Vec<&Photo> = match &self.sidebar_selection {
            SidebarItem::AllPhotos => self.photos.iter().filter(|p| !p.hidden).collect(),
            SidebarItem::Favorites => self
                .photos
                .iter()
                .filter(|p| p.flagged && !p.hidden)
                .collect(),
            SidebarItem::RecentImports => {
                let threshold = self.timestamp_counter.saturating_sub(100);
                self.photos
                    .iter()
                    .filter(|p| p.date_added >= threshold && !p.hidden)
                    .collect()
            }
            SidebarItem::Album(id) => {
                if let Some(album) = self.albums.iter().find(|a| a.id == *id) {
                    self.photos
                        .iter()
                        .filter(|p| album.photo_ids.contains(&p.id))
                        .collect()
                } else {
                    Vec::new()
                }
            }
            SidebarItem::SmartAlbum(id) => {
                if let Some(album) = self.smart_albums.iter().find(|a| a.id == *id) {
                    self.photos.iter().filter(|p| album.matches(p)).collect()
                } else {
                    Vec::new()
                }
            }
            SidebarItem::Trash => return self.trash.iter().map(|p| p.id).collect(),
        };

        // Apply search filter
        if !self.search_query.is_empty() {
            photos.retain(|p| p.matches_search(&self.search_query));
        }

        // Sort
        match self.sort_order {
            PhotoSort::DateAdded => photos.sort_by_key(|p| core::cmp::Reverse(p.date_added)),
            PhotoSort::DateTaken => photos.sort_by_key(|p| core::cmp::Reverse(p.date_taken)),
            PhotoSort::FileName => photos.sort_by_key(|a| a.file_name.to_lowercase()),
            PhotoSort::FileSize => photos.sort_by_key(|p| core::cmp::Reverse(p.file_size)),
            PhotoSort::Rating => photos.sort_by_key(|p| core::cmp::Reverse(p.rating)),
        }

        photos.iter().map(|p| p.id).collect()
    }

    /// Set search query.
    pub fn set_search(&mut self, query: &str) {
        self.search_query = query.to_owned();
    }

    /// Cycle sort order.
    pub fn cycle_sort(&mut self) {
        self.sort_order = self.sort_order.next();
    }

    /// Cycle thumbnail size.
    pub fn cycle_thumb_size(&mut self) {
        self.thumb_size_idx = self
            .thumb_size_idx
            .saturating_add(1)
            .checked_rem(THUMB_SIZES.len())
            .unwrap_or(0);
    }

    /// Get current thumbnail size.
    pub fn current_thumb_size(&self) -> f32 {
        THUMB_SIZES
            .get(self.thumb_size_idx)
            .copied()
            .unwrap_or(120.0)
    }

    // -----------------------------------------------------------------------
    // Timeline
    // -----------------------------------------------------------------------

    /// Group photos by date (year-month) for timeline view.
    pub fn timeline_groups(&self) -> Vec<(String, Vec<PhotoId>)> {
        let visible = self.visible_photos();
        let mut groups: Vec<(String, Vec<PhotoId>)> = Vec::new();

        for pid in visible {
            let date_key = if let Some(photo) = self.find_photo(pid) {
                // Use date_added as a proxy, group by hundreds
                let group_idx = photo.date_added / 100;
                format!("Period {group_idx}")
            } else {
                "Unknown".to_owned()
            };

            if let Some(group) = groups.iter_mut().find(|(key, _)| key == &date_key) {
                group.1.push(pid);
            } else {
                groups.push((date_key, vec![pid]));
            }
        }

        groups
    }

    // -----------------------------------------------------------------------
    // Slideshow
    // -----------------------------------------------------------------------

    /// Start a slideshow with the currently visible photos.
    pub fn start_slideshow(&mut self) {
        let photo_ids = self.visible_photos();
        if !photo_ids.is_empty() {
            self.slideshow = Some(SlideshowState::new(photo_ids));
            self.view_mode = ViewMode::Slideshow;
        }
    }

    /// Stop the slideshow.
    pub fn stop_slideshow(&mut self) {
        self.slideshow = None;
        self.view_mode = ViewMode::Grid;
    }

    /// Advance slideshow to next photo.
    pub fn slideshow_next(&mut self) {
        if let Some(ss) = &mut self.slideshow {
            ss.advance();
        }
    }

    /// Go to previous slideshow photo.
    pub fn slideshow_prev(&mut self) {
        if let Some(ss) = &mut self.slideshow {
            ss.go_back();
        }
    }

    // -----------------------------------------------------------------------
    // Statistics
    // -----------------------------------------------------------------------

    /// Get library statistics.
    pub fn library_stats(&self) -> LibraryStats<'_> {
        let total_size: u64 = self.photos.iter().map(|p| p.file_size).sum();
        let total_tagged: usize = self.photos.iter().filter(|p| !p.tags.is_empty()).count();
        let total_rated: usize = self.photos.iter().filter(|p| p.rating > 0).count();
        let total_flagged: usize = self.photos.iter().filter(|p| p.flagged).count();

        let mut format_counts: HashMap<&str, usize> = HashMap::new();
        for photo in &self.photos {
            let slot = format_counts.entry(photo.format.label()).or_insert(0usize);
            *slot = slot.saturating_add(1);
        }

        LibraryStats {
            total_photos: self.photos.len(),
            total_albums: self.albums.len(),
            total_smart_albums: self.smart_albums.len(),
            total_size,
            total_tagged,
            total_rated,
            total_flagged,
            trash_count: self.trash.len(),
            format_counts,
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Layout the renderer draws and the pointer reads
    // ------------------------------------------------------------------

    /// Adopt a new window size. Returns whether it changed.
    pub fn set_window_size(&mut self, width: f32, height: f32) -> bool {
        let width = width.max(MIN_WINDOW_WIDTH);
        let height = height.max(MIN_WINDOW_HEIGHT);
        if (self.window_width - width).abs() < f32::EPSILON
            && (self.window_height - height).abs() < f32::EPSILON
        {
            return false;
        }
        self.window_width = width;
        self.window_height = height;
        true
    }

    /// The rectangle the main content is drawn in: everything left over once
    /// the toolbar, the status bar, the sidebar and the info panel have taken
    /// their share.
    pub fn content_rect(&self) -> Rect {
        let info_w = if self.show_info_panel {
            INFO_PANEL_WIDTH
        } else {
            0.0
        };
        Rect {
            x: SIDEBAR_WIDTH,
            y: TOOLBAR_HEIGHT,
            w: (self.window_width - SIDEBAR_WIDTH - info_w).max(1.0),
            h: (self.window_height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT).max(1.0),
        }
    }

    /// The sidebar, row by row, in the order they are drawn.
    fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let item = |label: &str, target, indent, accent| SidebarRow::Item {
            label: label.to_owned(),
            target,
            indent,
            accent,
        };
        let mut rows = vec![
            SidebarRow::Gap(8.0),
            SidebarRow::Header("LIBRARY"),
            item(
                "All Photos",
                SidebarItem::AllPhotos,
                16.0,
                self.palette.blue,
            ),
            item("Favorites", SidebarItem::Favorites, 16.0, self.palette.blue),
            item(
                "Recent",
                SidebarItem::RecentImports,
                16.0,
                self.palette.blue,
            ),
            SidebarRow::Gap(12.0),
            SidebarRow::Header("ALBUMS"),
        ];
        for album in &self.albums {
            rows.push(SidebarRow::Item {
                label: format!("{} ({})", album.name, album.photo_count()),
                target: SidebarItem::Album(album.id),
                indent: 20.0,
                accent: self.palette.blue,
            });
        }
        rows.push(SidebarRow::Action {
            // The row is the input while a name is being typed, rather than a
            // dialog over the top: the album will appear in this list, so this
            // is where it should be born.
            label: self
                .naming_album
                .as_ref()
                .map_or_else(|| "+  New Album".to_owned(), |typed| format!("{typed}|")),
            action: SidebarAction::NewAlbum,
        });
        rows.push(SidebarRow::Gap(12.0));
        if !self.smart_albums.is_empty() {
            rows.push(SidebarRow::Header("SMART ALBUMS"));
            for album in &self.smart_albums {
                rows.push(SidebarRow::Item {
                    label: album.name.clone(),
                    target: SidebarItem::SmartAlbum(album.id),
                    indent: 20.0,
                    accent: self.palette.mauve,
                });
            }
        }
        rows.push(SidebarRow::Gap(16.0));
        rows.push(SidebarRow::Item {
            label: format!("Trash ({})", self.trash.len()),
            target: SidebarItem::Trash,
            indent: 16.0,
            accent: self.palette.red,
        });
        rows
    }

    /// Which sidebar row a point is on, if any.
    pub fn sidebar_item_at(&self, x: f32, y: f32) -> Option<SidebarItem> {
        if x < 0.0 || x >= SIDEBAR_WIDTH {
            return None;
        }
        // No guard against a point above the list: the walk starts at the
        // first row's own y and the list opens with a gap, so anything higher
        // lands in that gap and answers `None` on its own.
        let mut row_y = TOOLBAR_HEIGHT;
        for row in self.sidebar_rows() {
            let next = row_y + row.height();
            if y < next {
                return match row {
                    SidebarRow::Item { target, .. } => Some(target),
                    // A heading, the air around it, and an action are not
                    // places to go. `sidebar_action_at` answers for the last.
                    SidebarRow::Header(_) | SidebarRow::Gap(_) | SidebarRow::Action { .. } => None,
                };
            }
            row_y = next;
        }
        None
    }

    /// Which sidebar action a point is on, if any.
    ///
    /// A separate walk from [`Self::sidebar_item_at`] rather than one function
    /// answering both, because the two have different answers for the same
    /// point and every caller wants exactly one of them.
    pub fn sidebar_action_at(&self, x: f32, y: f32) -> Option<SidebarAction> {
        if x < 0.0 || x >= SIDEBAR_WIDTH {
            return None;
        }
        let mut row_y = TOOLBAR_HEIGHT;
        for row in self.sidebar_rows() {
            let next = row_y + row.height();
            if y < next {
                return match row {
                    SidebarRow::Action { action, .. } => Some(action),
                    SidebarRow::Header(_) | SidebarRow::Gap(_) | SidebarRow::Item { .. } => None,
                };
            }
            row_y = next;
        }
        None
    }

    /// Every toolbar control, with the rectangle it is drawn in.
    ///
    /// The widths are measured from the labels, so the renderer could not have
    /// hard-coded them and the hit test could not have guessed them.
    pub fn toolbar_controls(&self) -> Vec<(ToolbarControl, Rect)> {
        let mut out = Vec::new();
        let mut vx = 160.0;
        for (label, mode) in [
            ("Grid", ViewMode::Grid),
            ("Single", ViewMode::Single),
            ("Timeline", ViewMode::Timeline),
        ] {
            let w = text::padded_width(label, 8.0, 11.0, FontWeightHint::Regular);
            out.push((
                ToolbarControl::View(mode),
                Rect {
                    x: vx,
                    y: 8.0,
                    w,
                    h: 24.0,
                },
            ));
            vx += w + 4.0;
        }
        let sort_x = vx + 12.0;
        out.push((
            ToolbarControl::Sort,
            Rect {
                x: sort_x,
                y: 8.0,
                w: 112.0,
                h: 24.0,
            },
        ));
        let search_x = sort_x + 124.0;
        let search_w = 200.0;
        out.push((
            ToolbarControl::Search,
            Rect {
                x: search_x,
                y: 8.0,
                w: search_w,
                h: 24.0,
            },
        ));
        out.push((
            ToolbarControl::ThumbSize,
            Rect {
                x: search_x + search_w + 16.0,
                y: 8.0,
                w: 48.0,
                h: 24.0,
            },
        ));
        out.push((
            ToolbarControl::Import,
            Rect {
                x: self.window_width - 196.0,
                y: 8.0,
                w: 88.0,
                h: 24.0,
            },
        ));
        out.push((
            ToolbarControl::Slideshow,
            Rect {
                x: self.window_width - 100.0,
                y: 8.0,
                w: 88.0,
                h: 24.0,
            },
        ));
        out
    }

    /// Which toolbar control a point is on, if any.
    pub fn toolbar_control_at(&self, x: f32, y: f32) -> Option<ToolbarControl> {
        self.toolbar_controls()
            .into_iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(control, _)| control)
    }

    /// How many thumbnails fit across the content area.
    pub fn grid_columns(&self) -> usize {
        let cell = self.current_thumb_size() + THUMB_PADDING;
        let width = self.content_rect().w;
        #[allow(clippy::cast_sign_loss)]
        let cols = (width / cell).floor() as usize;
        cols.max(1)
    }

    /// The rows of thumbnails on screen.
    ///
    /// The grid used to `break` when a row fell below the window, with no
    /// offset at all: everything past the first screenful was cut by the
    /// window edge rather than by a scroll position, so a library of a
    /// thousand photos showed the first twenty and hid the rest for good.
    pub fn grid_window(&self) -> scroll_window::Rows {
        let content = self.content_rect();
        let cell = self.current_thumb_size() + THUMB_PADDING;
        let total = self.visible_photos().len();
        let rows = total.div_ceil(self.grid_columns());
        scroll_window::visible(rows, cell, content.h - THUMB_PADDING, self.grid_scroll)
    }

    /// Where the thumbnail at `index` among the visible photos is drawn, or
    /// `None` if the grid has scrolled past it.
    pub fn thumb_rect(&self, index: usize) -> Option<Rect> {
        let cols = self.grid_columns();
        let row = index.checked_div(cols)?;
        let window = self.grid_window();
        if row < window.start || row >= window.end() {
            return None;
        }
        let content = self.content_rect();
        let thumb = self.current_thumb_size();
        let cell = thumb + THUMB_PADDING;
        let col = index.checked_rem(cols)?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "a row or column index is bounded by what fits on a screen"
        )]
        let (col, drawn) = (col as f32, row.saturating_sub(window.start) as f32);
        Some(Rect {
            x: content.x + THUMB_PADDING + col * cell,
            y: content.y + THUMB_PADDING + drawn * cell,
            w: thumb,
            h: thumb,
        })
    }

    /// Which photo a point in the grid is on, if any.
    pub fn photo_at(&self, x: f32, y: f32) -> Option<PhotoId> {
        let visible = self.visible_photos();
        visible.iter().enumerate().find_map(|(index, &pid)| {
            self.thumb_rect(index)
                .filter(|rect| rect.contains(x, y))
                .map(|_| pid)
        })
    }

    /// Scroll so that the row holding `index` is one of the drawn ones.
    fn scroll_selection_into_view(&mut self, index: usize) {
        let cols = self.grid_columns();
        let row = index.checked_div(cols).unwrap_or(0);
        let window = self.grid_window();
        if row < window.start {
            self.grid_scroll = row;
        } else if row >= window.end() {
            self.grid_scroll = row.saturating_add(1).saturating_sub(window.count.max(1));
        }
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Handle one input event. Returns whether anything changed.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        // The picker takes input first while it is up, or a click meant for a
        // filename lands on whatever is drawn beneath it.
        //
        // A tick comes back as `Ignored` and falls through, which the helper
        // this replaced got wrong: it returned `false` for anything that was
        // not input and `handle_event` returned on that, so
        // `Event::Tick => self.advance_slideshow(..)` never ran while the
        // import dialog was open. **The slideshow stopped between one picture
        // and the next**, for as long as the dialog stayed up.
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            Picked::Chose(path) => {
                self.status_message = Some(self.import_from_disk(&path));
                return true;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return true,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Tick { elapsed_ms } => self.advance_slideshow(*elapsed_ms),
            _ => false,
        }
    }

    /// Put the file picker up, listing the directory it starts in.
    ///
    /// The widget is pure -- it draws the entries it is given and does no I/O
    /// -- so the host reads the directory and hands it over, which is the
    /// convention `apps/fileassoc` and `apps/passwordgen` already follow.
    pub fn open_import_dialog(&mut self) {
        self.picker.open_to_read();
    }

    /// Read a file and put it in the library, EXIF and all.
    ///
    /// Returns what to say about it. The failure is a message rather than a
    /// silent no-op on purpose: the whole of this application used to be a
    /// window over a library that was never read from anywhere, and "nothing
    /// visibly happened" is exactly how that survived.
    fn import_from_disk(&mut self, path: &std::path::Path) -> String {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => return format!("Could not read {name}: {err}"),
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(format) = ImageFormat::from_extension(&ext) else {
            return format!("{name} is not an image format this reads");
        };
        // The EXIF parser was written, tested and never given a real file.
        let exif = parse_exif_from_bytes(&bytes);
        let size = bytes.len() as u64;
        self.import_photo_with_exif(path, &name, format, size, exif);
        format!("Imported {name}")
    }

    /// Advance the slideshow clock. Returns whether the picture changed.
    ///
    /// `SlideshowState` has carried an `interval_ms` and an `elapsed_ms` since
    /// it was written and nothing ever read either: the slides only moved when
    /// the user pressed Next, which is not a slideshow. This is lesson 47 --
    /// an app that keeps time but never receives the clock.
    pub fn advance_slideshow(&mut self, elapsed_ms: u64) -> bool {
        let Some(ss) = &mut self.slideshow else {
            return false;
        };
        if ss.paused {
            return false;
        }
        ss.elapsed_ms = ss.elapsed_ms.saturating_add(elapsed_ms);
        if ss.elapsed_ms < ss.interval_ms {
            return false;
        }
        ss.elapsed_ms = 0;
        ss.advance();
        true
    }

    /// Is there anything a tick would move?
    pub fn has_work(&self) -> bool {
        self.slideshow.as_ref().is_some_and(|ss| !ss.paused)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        // An open menu takes the click before anything under it does --
        // that is what being over everything means.
        if self.photo_menu.is_some() {
            if let MouseEventKind::Press(_) = event.kind {
                let chosen = self
                    .photo_menu
                    .as_mut()
                    .and_then(|m| m.handle_click(event.x, event.y));
                self.photo_menu = None;
                if let Some(id) = chosen {
                    self.choose_album_from_menu(id);
                }
                // Consumed either way: a click that dismisses a menu should
                // not also land on whatever was behind it.
                return true;
            }
        }
        if matches!(event.kind, MouseEventKind::Press(MouseButton::Right))
            && self.view_mode == ViewMode::Grid
            && let Some(pid) = self.photo_at(event.x, event.y)
        {
            self.selected_photo = Some(pid);
            self.open_photo_menu(pid, event.x, event.y);
            return true;
        }
        if !matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
            return false;
        }
        // Any press moves the keyboard out of the search box unless it is
        // the search box being pressed. Done here rather than in each of the
        // branches below so that a control added later cannot forget it.
        let pressed = self.toolbar_control_at(event.x, event.y);
        self.search_focused = pressed == Some(ToolbarControl::Search);
        if self.search_focused {
            // One text field at a time.
            self.naming_album = None;
        }
        if let Some(control) = pressed {
            self.press_toolbar(control);
            return true;
        }
        if let Some(action) = self.sidebar_action_at(event.x, event.y) {
            match action {
                SidebarAction::NewAlbum => self.begin_naming_album(),
            }
            return true;
        }
        if let Some(target) = self.sidebar_item_at(event.x, event.y) {
            // Clicking somewhere else abandons a half-typed name, the same way
            // clicking away from the search box abandons the search.
            self.naming_album = None;
            self.select_sidebar(target);
            return true;
        }
        if self.view_mode == ViewMode::Grid
            && let Some(pid) = self.photo_at(event.x, event.y)
        {
            self.selected_photo = Some(pid);
            // A plain click starts again. Without this, a card clicked after
            // a Shift+arrow run would join a set the user believes they have
            // just replaced, and the next rating would land on all of it.
            self.selected_photos.clear();
            return true;
        }
        false
    }

    /// Carry out a toolbar control.
    pub fn press_toolbar(&mut self, control: ToolbarControl) {
        match control {
            ToolbarControl::View(mode) => {
                // Leaving the slideshow by choosing another view has to stop
                // it, or its clock keeps running behind the grid.
                if self.slideshow.is_some() {
                    self.stop_slideshow();
                }
                self.view_mode = mode;
            }
            ToolbarControl::Sort => {
                self.cycle_sort();
                self.grid_scroll = 0;
            }
            ToolbarControl::ThumbSize => {
                self.cycle_thumb_size();
                self.grid_scroll = 0;
            }
            ToolbarControl::Slideshow => {
                if self.slideshow.is_some() {
                    self.stop_slideshow();
                    self.view_mode = ViewMode::Grid;
                } else {
                    self.start_slideshow();
                }
            }
            // The click already moved the keyboard here; there is nothing
            // else for pressing it to do.
            ToolbarControl::Search => {}
            ToolbarControl::Import => self.open_import_dialog(),
        }
    }

    /// Go where a sidebar row points.
    pub fn select_sidebar(&mut self, target: SidebarItem) {
        self.sidebar_selection = target;
        // A grid scrolled forty rows into one album shows nothing at all of a
        // shorter one, and a selection from the old album is not in the new.
        self.grid_scroll = 0;
        self.selected_photo = None;
        self.selected_photos.clear();
    }

    /// Typing while the search box has the keyboard.
    ///
    /// Returns whether the event was consumed. Everything is consumed while
    /// the box is focused, including keys this does not act on: a shortcut
    /// that fired mid-word would edit the library under a user who believed
    /// they were typing a query.
    fn handle_search_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Escape => {
                // Escape abandons the search rather than merely leaving the
                // box, because a filter left in place by an emptied box is a
                // library that looks half-missing for no visible reason.
                self.search_focused = false;
                self.set_search("");
                true
            }
            Key::Enter => {
                self.search_focused = false;
                true
            }
            Key::Backspace => {
                let mut query = self.search_query.clone();
                query.pop();
                self.set_search(&query);
                true
            }
            _ => {
                let typed: String = event.typed().collect();
                if !typed.is_empty() {
                    let mut query = self.search_query.clone();
                    query.push_str(&typed);
                    self.set_search(&query);
                }
                true
            }
        }
    }

    /// Raise the album menu over a photograph.
    ///
    /// With no albums the menu still opens, carrying one disabled row saying
    /// so. A menu that refuses to appear leaves the user with no way to find
    /// out *why* nothing happened, and the answer -- there is nowhere to put
    /// it yet -- is exactly what they need to know.
    fn open_photo_menu(&mut self, pid: PhotoId, x: f32, y: f32) {
        let items: Vec<MenuItem> = if self.albums.is_empty() {
            vec![MenuItem::Action {
                id: 0,
                label: "No albums yet - make one in the sidebar".to_owned(),
                shortcut: None,
                icon: None,
                enabled: false,
                checked: None,
            }]
        } else {
            self.albums
                .iter()
                .map(|album| MenuItem::Action {
                    // The album's own id *is* the menu item's id, so nothing
                    // has to map between two numbering schemes and get it
                    // wrong when an album is deleted.
                    id: album.id,
                    label: format!("Add to {}", album.name),
                    shortcut: None,
                    icon: None,
                    enabled: !album.photo_ids.contains(&pid),
                    checked: None,
                })
                .collect()
        };
        let mut menu = ContextMenu::new(items);
        menu.show(x, y, (self.window_width, self.window_height));
        self.photo_menu = Some(menu);
        self.menu_photo = Some(pid);
    }

    /// Put the menu's photograph into the album that was chosen.
    fn choose_album_from_menu(&mut self, album_id: AlbumId) {
        let Some(pid) = self.menu_photo else {
            return;
        };
        // The whole selection when the menu was raised on part of it, and that
        // photograph alone otherwise -- right-clicking a card outside the
        // selection is a statement about that card, not about the set.
        let ids = if self.selected_photos.contains(&pid) {
            self.selected_photos.clone()
        } else {
            vec![pid]
        };
        let name = self
            .albums
            .iter()
            .find(|a| a.id == album_id)
            .map(|a| a.name.clone());
        if self.batch_add_to_album(&ids, album_id) {
            let count = ids.len();
            self.status_message = name.map(|n| {
                if count == 1 {
                    format!("Added to {n}")
                } else {
                    format!("Added {count} photos to {n}")
                }
            });
        }
    }

    /// Start naming a new album.
    fn begin_naming_album(&mut self) {
        // One text field at a time: two rows both showing a caret would leave
        // no way to tell where the next character is going.
        self.search_focused = false;
        self.naming_album = Some(String::new());
    }

    /// Typing while a new album is being named.
    ///
    /// Consumes every key for the same reason the search box does: the digits
    /// rate the selected photograph and `f` flags it, so an album called
    /// "5 star" would otherwise rewrite the library as it was typed.
    fn handle_album_name_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Escape => {
                self.naming_album = None;
                true
            }
            Key::Enter => {
                self.commit_album_name();
                true
            }
            Key::Backspace => {
                if let Some(name) = self.naming_album.as_mut() {
                    name.pop();
                }
                true
            }
            _ => {
                let typed: String = event.typed().collect();
                if let Some(name) = self.naming_album.as_mut() {
                    name.push_str(&typed);
                }
                true
            }
        }
    }

    /// Turn the typed name into an album, and show it.
    ///
    /// An empty name cancels rather than making an album called nothing --
    /// pressing Enter on a row you have not typed into is much more likely to
    /// be a change of mind than a request for a nameless album.
    fn commit_album_name(&mut self) {
        let Some(name) = self.naming_album.take() else {
            return;
        };
        let name = name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        let id = self.create_album(&name);
        // Show it. A new album that did not become the view would leave the
        // user looking at the same screen, with the only evidence of success
        // one more row in a list.
        self.sidebar_selection = SidebarItem::Album(id);
        self.grid_scroll = 0;
    }

    fn handle_key(&mut self, event: &KeyEvent) -> bool {
        if self.naming_album.is_some() {
            return self.handle_album_name_key(event);
        }
        if self.search_focused {
            return self.handle_search_key(event);
        }
        if self.view_mode == ViewMode::Slideshow {
            return self.handle_slideshow_key(event);
        }
        let extend = event.modifiers.shift;
        match event.key {
            Key::Left => self.step_selection(-1, extend),
            Key::Right => self.step_selection(1, extend),
            Key::Up => {
                let cols = self.row_step();
                self.step_selection(cols.checked_neg().unwrap_or(-1), extend)
            }
            Key::Down => {
                let cols = self.row_step();
                self.step_selection(cols, extend)
            }
            Key::Home => self.select_index(0),
            Key::End => {
                let last = self.visible_photos().len().saturating_sub(1);
                self.select_index(last)
            }
            Key::Enter => {
                if self.selected_photo.is_some() && self.view_mode != ViewMode::Single {
                    self.view_mode = ViewMode::Single;
                    true
                } else {
                    false
                }
            }
            Key::Escape => {
                if self.view_mode == ViewMode::Single {
                    self.view_mode = ViewMode::Grid;
                    true
                } else {
                    false
                }
            }
            Key::Delete => self.trash_selection(),
            Key::Space => {
                self.start_slideshow();
                self.slideshow.is_some()
            }
            _ => self.handle_typed(event),
        }
    }

    fn handle_typed(&mut self, event: &KeyEvent) -> bool {
        let Some(ch) = event.typed().next() else {
            return false;
        };
        match ch {
            // The arm already bounds this to 0..=5, which is 0..=MAX_STARS;
            // capping it again here would be a second statement of the range.
            '0'..='5' => {
                let stars = u8::try_from(u32::from(ch).saturating_sub(u32::from('0'))).unwrap_or(0);
                let ids = self.acting_on();
                self.batch_rate(&ids, stars) > 0
            }
            'f' | 'F' => self.selected_photo.is_some_and(|pid| self.toggle_flag(pid)),
            'i' | 'I' => {
                self.show_info_panel = !self.show_info_panel;
                // The content area just changed width, so a column count and
                // therefore a row count changed with it.
                self.grid_scroll = 0;
                true
            }
            's' | 'S' => {
                self.press_toolbar(ToolbarControl::Sort);
                true
            }
            '+' | '=' | '-' => {
                self.press_toolbar(ToolbarControl::ThumbSize);
                true
            }
            _ => false,
        }
    }

    fn handle_slideshow_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Escape => {
                self.stop_slideshow();
                self.view_mode = ViewMode::Grid;
                true
            }
            Key::Space => {
                if let Some(ss) = &mut self.slideshow {
                    ss.toggle_pause();
                }
                true
            }
            Key::Right => {
                self.slideshow_next();
                self.reset_slide_clock();
                true
            }
            Key::Left => {
                self.slideshow_prev();
                self.reset_slide_clock();
                true
            }
            _ => false,
        }
    }

    /// A slide moved to by hand gets its full time on screen.
    fn reset_slide_clock(&mut self) {
        if let Some(ss) = &mut self.slideshow {
            ss.elapsed_ms = 0;
        }
    }

    /// One row's worth of movement, as a signed step.
    fn row_step(&self) -> isize {
        isize::try_from(self.grid_columns()).unwrap_or(1)
    }

    /// Where the selection is among the photos on screen.
    pub fn selected_index(&self) -> Option<usize> {
        let wanted = self.selected_photo?;
        self.visible_photos().iter().position(|&pid| pid == wanted)
    }

    /// Move the selection by `delta`, stopping at the ends.
    ///
    /// Stopping rather than wrapping: holding Right to the end of a library
    /// and silently arriving back at the first photo is a worse answer than
    /// stopping, because the grid looks much the same either way.
    /// Move the selection, growing it instead when `extend` is set.
    ///
    /// Both ends join the set on every step, so a run of Shift+Right collects
    /// everything it passes over rather than just where it started and
    /// stopped. Moving *without* extending drops the set: an arrow key on its
    /// own is a fresh single selection, and leaving six cards highlighted
    /// behind a cursor that has moved away from them is how a batch operation
    /// surprises somebody.
    fn step_selection(&mut self, delta: isize, extend: bool) -> bool {
        let anchor = self.selected_photo;
        let moved = self.move_selection(delta);
        if !extend {
            self.selected_photos.clear();
            return moved;
        }
        for pid in [anchor, self.selected_photo].into_iter().flatten() {
            if !self.selected_photos.contains(&pid) {
                self.selected_photos.push(pid);
            }
        }
        moved
    }

    /// Every photograph an action should apply to.
    ///
    /// The multi-selection when there is one, and the single selection
    /// otherwise -- so every caller gets a list and none has to remember that
    /// there are two ways to have selected something.
    fn acting_on(&self) -> Vec<PhotoId> {
        if self.selected_photos.is_empty() {
            self.selected_photo.into_iter().collect()
        } else {
            self.selected_photos.clone()
        }
    }

    fn move_selection(&mut self, delta: isize) -> bool {
        let total = self.visible_photos().len();
        if total == 0 {
            return false;
        }
        let next = match self.selected_index() {
            None => 0,
            Some(index) => {
                let moved = index
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                if moved == index {
                    return false;
                }
                moved
            }
        };
        self.select_index(next)
    }

    /// Select the photo at `index` among the visible ones.
    fn select_index(&mut self, index: usize) -> bool {
        let Some(&pid) = self.visible_photos().get(index) else {
            return false;
        };
        if self.selected_photo == Some(pid) {
            return false;
        }
        self.selected_photo = Some(pid);
        self.scroll_selection_into_view(index);
        true
    }

    /// Move the selected photo to the trash, and put the selection on
    /// whatever takes its place.
    fn trash_selection(&mut self) -> bool {
        let Some(pid) = self.selected_photo else {
            return false;
        };
        let index = self.selected_index();
        if !self.trash_photo(pid) {
            return false;
        }
        // Selection by position, not by id: the id is gone. Landing on the
        // photo that moved up into the gap is what a user expects after
        // deleting one of a run.
        let remaining = self.visible_photos();
        self.selected_photo = index
            .and_then(|i| remaining.get(i.min(remaining.len().saturating_sub(1))))
            .copied();
        true
    }

    /// Render the full application frame.
    pub fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar
        self.render_toolbar(&mut cmds, width);

        // Status bar
        self.render_status_bar(&mut cmds, width, height);

        let content_y = TOOLBAR_HEIGHT;
        let content_h = height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Sidebar
        self.render_sidebar(&mut cmds, content_y, content_h);

        // Info panel (right side)
        let info_w = if self.show_info_panel {
            INFO_PANEL_WIDTH
        } else {
            0.0
        };
        if self.show_info_panel {
            self.render_info_panel(&mut cmds, width - info_w, content_y, info_w, content_h);
        }

        // Main content area
        let main_x = SIDEBAR_WIDTH;
        let main_w = width - SIDEBAR_WIDTH - info_w;
        self.render_main_content(&mut cmds, main_x, content_y, main_w, content_h);

        // The picker goes last so it sits over everything, which is the same
        // order in which `handle_event` gives it the click.
        // Over the window, under the picker: the picker is modal, and a
        // menu raised before it opened has no business on top of it.
        if let Some(menu) = &self.photo_menu {
            cmds.extend(menu.render(&self.palette));
        }

        cmds.extend(self.picker.render(&self.palette, width, height));

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        // Background
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // App title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Photo Manager".to_owned(),
            color: self.palette.ink(self.palette.blue),
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(140.0),
            overflow: TextOverflow::Ellipsis,
        });

        // View mode buttons
        let controls = self.toolbar_controls();
        let rect_of = |wanted: ToolbarControl| {
            controls.iter().find(|(c, _)| *c == wanted).map_or(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                },
                |(_, r)| *r,
            )
        };
        for (label, mode) in [
            ("Grid", ViewMode::Grid),
            ("Single", ViewMode::Single),
            ("Timeline", ViewMode::Timeline),
        ] {
            let is_active = self.view_mode == mode;
            let rect = rect_of(ToolbarControl::View(mode));
            let bg = if is_active {
                self.palette.surface1
            } else {
                self.palette.surface0
            };
            let fg = if is_active {
                self.palette.blue
            } else {
                self.palette.subtext0
            };
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: rect.x + 8.0,
                y: 14.0,
                text: label.to_owned(),
                color: fg,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((rect.w - 12.0).max(1.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Sort button
        let sort_label = format!("Sort: {}", self.sort_order.label());
        let sort_x = rect_of(ToolbarControl::Sort).x;
        self.palette.push_surface(
            cmds,
            sort_x,
            8.0,
            110.0,
            24.0,
            CORNER_RADIUS,
            Surface::Strip(Edge::Bottom),
        );
        cmds.push(RenderCommand::Text {
            x: sort_x + 8.0,
            y: 14.0,
            text: sort_label,
            color: self.palette.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Search box. The rectangle comes from `toolbar_controls` rather
        // than being computed again here: it is the same law the click reads,
        // and two copies of a layout drift the first time one is adjusted.
        let search = rect_of(ToolbarControl::Search);
        let (search_x, search_w) = (search.x, search.w);
        self.palette.push_surface(
            cmds,
            search_x,
            8.0,
            search_w,
            24.0,
            CORNER_RADIUS,
            Surface::Card,
        );
        if self.search_focused {
            // Where the typing is going. Without it a focused empty box and an
            // unfocused empty box are the same picture, and the only way to
            // find out which one is in front of you is to type and see what
            // happens to the library.
            cmds.push(RenderCommand::StrokeRect {
                x: search_x,
                y: 8.0,
                width: search_w,
                height: 24.0,
                color: self.palette.blue,
                line_width: 2.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
        }
        let search_text = if self.search_query.is_empty() {
            if self.search_focused {
                // The placeholder would read as text already typed once a
                // caret is beside it.
                "|".to_owned()
            } else {
                "Search photos...".to_owned()
            }
        } else if self.search_focused {
            format!("{}|", self.search_query)
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            self.palette.overlay0
        } else {
            self.palette.text
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 8.0,
            y: 14.0,
            text: search_text,
            color: search_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Thumb size indicator
        let ts = self.current_thumb_size();
        let size_label = format!("{ts:.0}px");
        cmds.push(RenderCommand::Text {
            x: search_x + search_w + 16.0,
            y: 14.0,
            text: size_label,
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Import button.
        //
        // Drawn from the same rectangle the hit test uses, via `rect_of`,
        // rather than from a second copy of the arithmetic: a control the
        // pointer can reach and the eye cannot find is the same bug as one the
        // eye finds and the pointer cannot reach, and both come from two
        // predicates for one position.
        let import_rect = rect_of(ToolbarControl::Import);
        self.palette.push_surface(
            cmds,
            import_rect.x,
            import_rect.y,
            import_rect.w,
            import_rect.h,
            CORNER_RADIUS,
            Surface::Card,
        );
        cmds.push(RenderCommand::Text {
            x: import_rect.x + 8.0,
            y: 14.0,
            text: "Import".to_owned(),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((import_rect.w - 12.0).max(1.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Slideshow button
        let ss_x = width - 100.0;
        self.palette.push_surface(
            cmds,
            ss_x,
            8.0,
            80.0,
            24.0,
            CORNER_RADIUS,
            if self.slideshow.is_some() {
                Surface::Selected
            } else {
                Surface::Card
            },
        );
        cmds.push(RenderCommand::Text {
            x: ss_x + 8.0,
            y: 14.0,
            text: "Slideshow".to_owned(),
            color: if self.slideshow.is_some() {
                self.palette.ink(self.palette.green)
            } else {
                self.palette.subtext0
            },
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: width,
            y2: TOOLBAR_HEIGHT,
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;

        self.palette.push_surface(
            cmds,
            0.0,
            bar_y,
            width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: bar_y,
            x2: width,
            y2: bar_y,
            color: self.palette.surface0,
            width: 1.0,
        });

        let stats = self.library_stats();
        let visible = self.visible_photos().len();
        // The library note outranks the transient one. A failed load or a
        // refused save is a standing condition -- it is still true after the
        // next import reports success -- so it must not be pushed off the bar
        // by something that happened afterwards.
        //
        // This field was written and never read until 2026-09-17, which lane
        // A's sweep caught through `check-fields-written-never-read`: the
        // application recorded why it could not read the library and then
        // showed nobody. A test asserted the field was set and called that
        // "the user is told", which is the exact mistake design-decisions 856
        // names -- a thing is built when something obeys it, not when
        // something stores it.
        let status_text = self
            .library_note
            .as_ref()
            .or(self.status_message.as_ref())
            .map_or_else(
                || {
                    format!(
                        "{} photos shown  |  {} total  |  {} albums  |  {} in trash",
                        visible, stats.total_photos, stats.total_albums, stats.trash_count,
                    )
                },
                // The import result takes the bar until something else
                // happens. A read that failed has to be visible somewhere, and
                // the counts it replaces are the thing that would otherwise be
                // read as the answer -- an unchanged total looks like a
                // refusal nobody explained.
                Clone::clone,
            );
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: status_text,
            color: if self.library_note.is_some() {
                self.palette.ink(self.palette.red)
            } else {
                self.palette.subtext0
            },
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Show selected photo info on right
        if let Some(pid) = self.selected_photo
            && let Some(photo) = self.find_photo(pid)
        {
            let sel_text = format!("{} — {}", photo.file_name, photo.human_size());
            cmds.push(RenderCommand::Text {
                x: width - 300.0,
                y: bar_y + 6.0,
                text: sel_text,
                color: self.palette.text,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(280.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: SIDEBAR_WIDTH,
            height,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: y,
            x2: SIDEBAR_WIDTH,
            y2: y + height,
            color: self.palette.surface0,
            width: 1.0,
        });

        let mut cy = y;
        for row in self.sidebar_rows() {
            match &row {
                SidebarRow::Gap(_) => {}
                SidebarRow::Header(text) => cmds.push(RenderCommand::Text {
                    x: 12.0,
                    y: cy,
                    text: (*text).to_owned(),
                    color: self.palette.subtext0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(SIDEBAR_WIDTH - 24.0),
                    overflow: TextOverflow::Ellipsis,
                }),
                SidebarRow::Action { label, .. } => {
                    // Blue while it is taking typing, for the same reason the
                    // search box is: an empty row waiting for a name and an
                    // idle row offering to take one are otherwise the same
                    // picture.
                    let color = if self.naming_album.is_some() {
                        self.palette.blue
                    } else {
                        self.palette.subtext0
                    };
                    cmds.push(RenderCommand::Text {
                        x: 20.0,
                        y: cy + 8.0,
                        text: label.clone(),
                        color,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - 36.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
                SidebarRow::Item {
                    label,
                    target,
                    indent,
                    accent,
                } => {
                    let is_selected = self.sidebar_selection == *target;
                    if is_selected {
                        cmds.push(RenderCommand::FillRect {
                            x: 4.0,
                            y: cy,
                            width: SIDEBAR_WIDTH - 8.0,
                            height: ITEM_HEIGHT,
                            color: self.palette.surface0,
                            corner_radii: CornerRadii::all(CORNER_RADIUS),
                        });
                    }
                    let color = if is_selected {
                        *accent
                    } else if *target == SidebarItem::Trash {
                        self.palette.subtext0
                    } else {
                        self.palette.text
                    };
                    cmds.push(RenderCommand::Text {
                        x: *indent,
                        y: cy + 8.0,
                        text: label.clone(),
                        color,
                        font_size: 12.0,
                        font_weight: if is_selected {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        },
                        max_width: Some(SIDEBAR_WIDTH - indent - 16.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
            cy += row.height();
        }
    }
    fn render_info_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Background
        self.palette
            .push_surface(cmds, x, y, width, height, 0.0, Surface::Card);

        // Separator
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x,
            y2: y + height,
            color: self.palette.surface0,
            width: 1.0,
        });

        let Some(pid) = self.selected_photo else {
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 20.0,
                text: "No photo selected".to_owned(),
                color: self.palette.subtext0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        };

        let Some(photo) = self.find_photo(pid) else {
            return;
        };

        let mut cy = y + 12.0;
        let lx = x + 12.0;
        let max_w = width - 24.0;

        // File name
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: photo.file_name.clone(),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 20.0;

        // Format and size
        let info_line = format!("{} — {}", photo.format.label(), photo.human_size());
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: info_line,
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 18.0;

        // Rating stars
        let stars_text = format!("Rating: {}", "*".repeat(photo.rating as usize));
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: stars_text,
            color: self.palette.ink(self.palette.yellow),
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 18.0;

        // Color label
        if photo.color_label != ColorLabel::None {
            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: cy,
                width: 12.0,
                height: 12.0,
                color: photo.color_label.color(&self.palette),
                corner_radii: CornerRadii::all(2.0),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 18.0,
                y: cy,
                text: photo.color_label.label().to_owned(),
                color: self.palette.text,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 18.0;
        }

        // Tags
        if !photo.tags.is_empty() {
            cy += 8.0;
            cmds.push(RenderCommand::Text {
                x: lx,
                y: cy,
                text: "TAGS".to_owned(),
                color: self.palette.subtext0,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(max_w),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 16.0;
            let tags_line = photo.tags.join(", ");
            cmds.push(RenderCommand::Text {
                x: lx,
                y: cy,
                text: tags_line,
                color: self.palette.ink(self.palette.teal),
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 18.0;
        }

        // EXIF section
        cy += 8.0;
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "EXIF DATA".to_owned(),
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 16.0;

        let exif_entries = self.collect_exif_entries(photo);
        for (label, value) in &exif_entries {
            cmds.push(RenderCommand::Text {
                x: lx,
                y: cy,
                text: format!("{label}:"),
                color: self.palette.subtext0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: lx + 85.0,
                y: cy,
                text: value.clone(),
                color: self.palette.text,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 90.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 15.0;
        }

        // Adjustments section
        if !photo.adjustments.is_default() {
            cy += 8.0;
            cmds.push(RenderCommand::Text {
                x: lx,
                y: cy,
                text: "ADJUSTMENTS".to_owned(),
                color: self.palette.subtext0,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(max_w),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 16.0;

            let adj_entries = [
                ("Brightness", photo.adjustments.brightness),
                ("Contrast", photo.adjustments.contrast),
                ("Saturation", photo.adjustments.saturation),
                ("Exposure", photo.adjustments.exposure),
                ("Temperature", photo.adjustments.temperature),
            ];
            for (label, val) in &adj_entries {
                if val.abs() > f32::EPSILON {
                    let sign = if *val > 0.0 { "+" } else { "" };
                    cmds.push(RenderCommand::Text {
                        x: lx,
                        y: cy,
                        text: format!("{label}: {sign}{val:.1}"),
                        color: self.palette.subtext1,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cy += 15.0;
                }
            }
        }
    }

    fn collect_exif_entries(&self, photo: &Photo) -> Vec<(&'static str, String)> {
        let exif = &photo.exif;
        let mut entries = Vec::new();

        if let Some(ref make) = exif.camera_make {
            entries.push(("Camera", make.clone()));
        }
        if let Some(ref model) = exif.camera_model {
            entries.push(("Model", model.clone()));
        }
        if let Some(ref lens) = exif.lens {
            entries.push(("Lens", lens.clone()));
        }
        if let Some(mm) = exif.focal_length_mm {
            entries.push(("Focal Length", format!("{mm:.0} mm")));
        }
        entries.push(("Resolution", exif.resolution_str()));
        if let Some(mp) = exif.megapixels() {
            entries.push(("Megapixels", format!("{mp:.1} MP")));
        }
        let exposure = exif.exposure_summary();
        if exposure != "No exposure data" {
            entries.push(("Exposure", exposure));
        }
        if let Some(ref program) = exif.exposure_program {
            entries.push(("Program", program.clone()));
        }
        if let Some(bias) = exif.exposure_bias {
            // Signed, with the sign always shown. "0.3 EV" and "-0.3 EV" are a
            // stop and a half apart in what they mean and one character apart
            // on the panel, so the plus is not decoration.
            entries.push(("Exp. Bias", format!("{bias:+.1} EV")));
        }
        if let Some(fired) = exif.flash_fired {
            entries.push((
                "Flash",
                if fired { "Fired" } else { "Did not fire" }.to_owned(),
            ));
        }
        if let Some(ref date) = exif.date_taken {
            entries.push(("Date", date.clone()));
        }
        if let Some(gps) = exif.gps_str() {
            entries.push(("GPS", gps));
        }
        if let Some(alt) = exif.gps_altitude {
            entries.push(("Altitude", format!("{alt:.0} m")));
        }
        if let Some(ref cs) = exif.color_space {
            entries.push(("Color Space", cs.clone()));
        }
        if let Some(ref wb) = exif.white_balance {
            entries.push(("White Bal.", wb.clone()));
        }
        if let Some(ref mm) = exif.metering_mode {
            entries.push(("Metering", mm.clone()));
        }
        // Last, because they are about the file rather than the photograph.
        if let Some(ref software) = exif.software {
            entries.push(("Software", software.clone()));
        }
        if let Some(ref copyright) = exif.copyright {
            entries.push(("Copyright", copyright.clone()));
        }

        entries
    }

    fn render_main_content(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        match self.view_mode {
            ViewMode::Grid => self.render_grid_view(cmds, x, y, width, height),
            ViewMode::Single => self.render_single_view(cmds, x, y, width, height),
            ViewMode::Timeline => self.render_timeline_view(cmds, x, y, width, height),
            ViewMode::Slideshow => self.render_slideshow_view(cmds, x, y, width, height),
        }
    }

    fn render_grid_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let visible = self.visible_photos();
        let thumb = self.current_thumb_size();

        if visible.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 60.0,
                y: y + height / 2.0,
                text: "No photos".to_owned(),
                color: self.palette.subtext0,
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            return;
        }

        for (idx, &pid) in visible.iter().enumerate() {
            // `thumb_rect` is the same law the click reads, and it answers
            // `None` for a row the grid has scrolled past -- which is what
            // stops a thumbnail being drawn over the status bar.
            let Some(cell) = self.thumb_rect(idx) else {
                continue;
            };
            let (cx, cy) = (cell.x, cell.y);

            let is_selected =
                self.selected_photo == Some(pid) || self.selected_photos.contains(&pid);

            // Thumbnail placeholder
            let border_color = if is_selected {
                self.palette.blue
            } else {
                self.palette.surface1
            };
            self.palette.push_surface(
                cmds,
                cx,
                cy,
                thumb,
                thumb,
                CORNER_RADIUS,
                Surface::ControlTrack,
            );
            // The photograph, inside the card rather than instead of it: the
            // surface beneath shows through wherever the picture's proportions
            // leave the square unfilled, and the border below is drawn after,
            // so a selected card keeps its outline over its own picture.
            if let Some(&(mtime, id)) = self.thumb_ready.get(&pid)
                && let Some(photo) = self.find_photo(pid)
                && let Some(picture) =
                    self.thumb_cache
                        .peek(&photo.file_path, mtime, photo.file_size)
            {
                cmds.extend(thumbs::render_thumbnail(picture, id, cx, cy, thumb));
            }
            cmds.push(RenderCommand::StrokeRect {
                x: cx,
                y: cy,
                width: thumb,
                height: thumb,
                color: border_color,
                line_width: if is_selected { 2.0 } else { 1.0 },
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Photo icon/label in center
            if let Some(photo) = self.find_photo(pid) {
                cmds.push(RenderCommand::Text {
                    x: cx + 4.0,
                    y: cy + thumb - 16.0,
                    text: photo.file_name.clone(),
                    color: self.palette.subtext0,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(thumb - 8.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Rating stars in top-left
                if photo.rating > 0 {
                    cmds.push(RenderCommand::Text {
                        x: cx + 4.0,
                        y: cy + 4.0,
                        text: "*".repeat(photo.rating as usize),
                        color: self.palette.ink(self.palette.yellow),
                        font_size: 10.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(thumb - 8.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                // Flagged indicator
                if photo.flagged {
                    cmds.push(RenderCommand::FillRect {
                        x: cx + thumb - 14.0,
                        y: cy + 4.0,
                        width: 10.0,
                        height: 10.0,
                        color: self.palette.peach,
                        corner_radii: CornerRadii::all(5.0),
                    });
                }

                // Color label dot
                if photo.color_label != ColorLabel::None {
                    cmds.push(RenderCommand::FillRect {
                        x: cx + thumb - 14.0,
                        y: cy + 18.0,
                        width: 10.0,
                        height: 10.0,
                        color: photo.color_label.color(&self.palette),
                        corner_radii: CornerRadii::all(5.0),
                    });
                }
            }
        }
    }

    fn render_single_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let Some(pid) = self.selected_photo else {
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 80.0,
                y: y + height / 2.0,
                text: "Select a photo to view".to_owned(),
                color: self.palette.subtext0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            return;
        };

        let Some(photo) = self.find_photo(pid) else {
            return;
        };

        // The room a picture has, before its own shape is taken into account.
        let area_w = width - 40.0;
        let area_h = height - 60.0;

        // Only the picture decoded for *this* photograph may be drawn: the
        // upload outlives a change of selection by the frame it takes to
        // replace it, and drawing it under the new name would put the wrong
        // photograph on screen.
        let shown = self.shown_picture.filter(|s| s.photo == pid);

        // A real picture is drawn at its own proportions. The 4:3 this used to
        // assume was a guess -- one that every portrait photograph falsified,
        // and that nothing could correct because nothing here had opened the
        // file.
        let (display_w, display_h) = match shown {
            Some(picture) => fit_within(picture.width, picture.height, area_w, area_h),
            None => {
                let ratio = (area_w / area_h).min(4.0 / 3.0);
                (area_h * ratio, area_h)
            }
        };
        let display_x = x + (width - display_w) / 2.0;
        let display_y = y + 10.0;

        if shown.is_some() {
            cmds.push(RenderCommand::Image {
                x: display_x,
                y: display_y,
                width: display_w,
                height: display_h,
                image_id: PHOTO_IMAGE_ID,
            });
        } else {
            self.palette.push_surface(
                cmds,
                display_x,
                display_y,
                display_w,
                display_h,
                CORNER_RADIUS,
                Surface::Card,
            );
            cmds.push(RenderCommand::StrokeRect {
                x: display_x,
                y: display_y,
                width: display_w,
                height: display_h,
                color: self.palette.surface1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            cmds.push(RenderCommand::Text {
                x: display_x + display_w / 2.0 - 60.0,
                y: display_y + display_h / 2.0 - 10.0,
                text: photo.file_name.clone(),
                color: self.palette.text,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(display_w - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
            // The reason takes the place of the dimensions, and takes the
            // colour that says it is one: a card that simply stayed blank
            // could not be told from one whose photograph had not been
            // reached yet.
            let (detail, color) = match self.picture_error.as_ref() {
                Some(reason) => (reason.clone(), self.palette.red),
                None => (
                    format!("{} — {}", photo.exif.resolution_str(), photo.human_size()),
                    self.palette.subtext0,
                ),
            };
            cmds.push(RenderCommand::Text {
                x: display_x + display_w / 2.0 - 50.0,
                y: display_y + display_h / 2.0 + 10.0,
                text: detail,
                color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(display_w - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Bottom bar with nav hint
        cmds.push(RenderCommand::Text {
            x: x + width / 2.0 - 80.0,
            y: y + height - 20.0,
            text: "< Prev  |  Next >".to_owned(),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_timeline_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let groups = self.timeline_groups();
        let thumb = self.current_thumb_size() * 0.75;
        let cell_size = thumb + THUMB_PADDING;
        let cols = ((width / cell_size).floor() as usize).max(1);

        let mut cy = y + 8.0;

        for (label, photo_ids) in &groups {
            if cy > y + height {
                break;
            }

            // Group header
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy,
                text: format!("{label} ({} photos)", photo_ids.len()),
                color: self.palette.ink(self.palette.blue),
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 24.0;

            // Thumbnails
            for (idx, &pid) in photo_ids.iter().enumerate() {
                let col = idx.checked_rem(cols).unwrap_or(0);
                let row = idx.checked_div(cols).unwrap_or(0);
                let cx = x + THUMB_PADDING + (col as f32) * cell_size;
                let ty = cy + (row as f32) * cell_size;

                if ty > y + height {
                    break;
                }

                let is_selected = self.selected_photo == Some(pid);

                self.palette.push_surface(
                    cmds,
                    cx,
                    ty,
                    thumb,
                    thumb,
                    CORNER_RADIUS,
                    Surface::ControlTrack,
                );
                if is_selected {
                    cmds.push(RenderCommand::StrokeRect {
                        x: cx,
                        y: ty,
                        width: thumb,
                        height: thumb,
                        color: self.palette.blue,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(CORNER_RADIUS),
                    });
                }
            }

            let rows_needed = photo_ids.len().div_ceil(cols.max(1));
            cy += (rows_needed as f32) * cell_size + 12.0;
        }

        if groups.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 50.0,
                y: y + height / 2.0,
                text: "No photos".to_owned(),
                color: self.palette.subtext0,
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_slideshow_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let Some(ss) = &self.slideshow else {
            return;
        };

        // Full black background
        self.palette
            .push_surface(cmds, x, y, width, height, 0.0, Surface::Card);

        if let Some(pid) = ss.current_photo()
            && let Some(photo) = self.find_photo(pid)
        {
            // Photo placeholder (centered)
            let display_w = width * 0.8;
            let display_h = height * 0.8;
            let display_x = x + (width - display_w) / 2.0;
            let display_y = y + (height - display_h) / 2.0;

            self.palette.push_surface(
                cmds,
                display_x,
                display_y,
                display_w,
                display_h,
                8.0,
                Surface::Card,
            );

            cmds.push(RenderCommand::Text {
                x: display_x + display_w / 2.0 - 60.0,
                y: display_y + display_h / 2.0,
                text: photo.file_name.clone(),
                color: self.palette.text,
                font_size: 16.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(display_w - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Slideshow controls at bottom
        let ctrl_y = y + height - 40.0;
        let paused_label = if ss.paused { "Play" } else { "Pause" };
        let progress = format!(
            "{} / {}  |  {}  |  {}",
            ss.current_index.saturating_add(1),
            ss.photo_ids.len(),
            paused_label,
            ss.transition.label(),
        );
        cmds.push(RenderCommand::Text {
            x: x + width / 2.0 - 80.0,
            y: ctrl_y,
            text: progress,
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// `truncate_str` was removed here. Its one caller cut a thumbnail's file name
// to `(thumb / 7.0)` characters — a guess of 7 px per character for a label
// drawn at 9 px, where the average is nearer 4.5 — and then handed the result
// to a `Text` command that already carried `max_width: Some(thumb - 8.0)` and
// `TextOverflow::Ellipsis`. The renderer measures with the real font; the
// guess did not, so it cut a third of the name off a thumbnail that had room
// for it. Worse, `s.get(..n)` on a name like "Sommerferien_Österreich.jpg"
// straddles a character and silently returns the *whole* string, so the one
// case the truncation existed for was the one it did not handle. The name is
// now passed whole and cut by the renderer, which knows how wide it is.

/// Library-wide statistics.
pub struct LibraryStats<'a> {
    pub total_photos: usize,
    pub total_albums: usize,
    pub total_smart_albums: usize,
    pub total_size: u64,
    pub total_tagged: usize,
    pub total_rated: usize,
    pub total_flagged: usize,
    pub trash_count: usize,
    pub format_counts: HashMap<&'a str, usize>,
}

// ============================================================================
// Main entry point
// ============================================================================

impl App for PhotoApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        // What is being looked at, because that is what the window is: a
        // library behind three other windows is found again by its title.
        let count = self.visible_photos().len();
        let where_ = match self.sidebar_selection {
            SidebarItem::AllPhotos => "All Photos".to_owned(),
            SidebarItem::Favorites => "Favorites".to_owned(),
            SidebarItem::RecentImports => "Recent".to_owned(),
            SidebarItem::Trash => "Trash".to_owned(),
            SidebarItem::Album(id) => self
                .albums
                .iter()
                .find(|a| a.id == id)
                .map_or_else(|| "Album".to_owned(), |a| a.name.clone()),
            SidebarItem::SmartAlbum(id) => self
                .smart_albums
                .iter()
                .find(|a| a.id == id)
                .map_or_else(|| "Smart Album".to_owned(), |a| a.name.clone()),
        };
        format!("{where_} ({count}) - Photos")
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        }
    }

    /// A clock only while a slideshow is running.
    ///
    /// A library sitting still has nothing to advance, and waking the machine
    /// ten times a second to discover that is `known-issues.md` lesson 47 --
    /// which is also the defect being fixed here, from the other side.
    fn tick_interval(&self) -> Option<Duration> {
        self.has_work().then_some(SLIDESHOW_TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::CloseRequested => Response::Exit,
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension in pixels is exact in f32"
                )]
                let (w, h) = (*width as f32, *height as f32);
                if self.set_window_size(w, h) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            other => {
                let redraw = self.handle_event(other);
                // After the handler, not inside it: every path that changes a
                // rating, a flag or the photo list goes through here, and this
                // is the one place that cannot be forgotten when a new one is
                // added.
                self.persist_if_changed();
                if redraw {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
        }
    }

    fn take_images(&mut self) -> Vec<app::ImageChange> {
        // Drops first, and the order is load-bearing: the link checks its
        // image budget against `held - freed + incoming`, so a batch that
        // evicted as many thumbnails as it generated would be refused if it
        // asked the compositor to hold both sets at once -- which is exactly
        // the moment the cache is working as designed.
        let mut changes: Vec<app::ImageChange> = self
            .thumb_cache
            .take_evicted_image_ids()
            .into_iter()
            .map(app::ImageChange::Drop)
            .collect();
        // An evicted thumbnail is no longer drawable, and a card that kept
        // naming it would draw nothing at all: the compositor discards an
        // `Image` command for an id it does not hold, and says nothing.
        if !changes.is_empty() {
            let dropped: Vec<u64> = changes
                .iter()
                .filter_map(|c| match c {
                    app::ImageChange::Drop(id) => Some(*id),
                    app::ImageChange::Upload { .. } => None,
                })
                .collect();
            self.thumb_ready.retain(|_, (_, id)| !dropped.contains(id));
        }
        changes.append(&mut self.pending_images);
        for (id, thumb) in std::mem::take(&mut self.thumb_uploads) {
            let Some(bytes) = thumb.to_wire_bytes() else {
                // Skipped rather than uploaded wrong; the card keeps its
                // placeholder, which is what a card with no usable picture
                // should show.
                continue;
            };
            changes.push(app::ImageChange::Upload {
                id,
                width: thumb.width,
                height: thumb.height,
                stride: thumb.width.saturating_mul(4),
                format: oswindow::PixelFormat::Argb8888,
                bytes,
            });
        }
        changes
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The handed size wins over the recorded one: the first frame is drawn
        // before any `Event::Resize` arrives, so a window opened at another
        // size would be laid out for the size that was asked for, and every
        // hit box in it would name the wrong rectangle.
        self.set_window_size(width, height);
        // Before the commands are built, so the frame that names the picture
        // is the frame it is uploaded for. See `sync_picture`.
        self.sync_picture();
        self.sync_thumbnails();
        RenderTree {
            commands: self.render_commands(width, height),
        }
    }
}

fn main() -> ExitCode {
    // Starts empty. It used to call `seeded_library`, which built two albums
    // ("Vacation 2025", "Family") and three photos at paths like
    // `/photos/IMG_0001.jpg`, one of them carrying `ExifData::sample` -- a
    // Canon EOS R5, an RF 24-70mm lens, Adobe Lightroom, and GPS coordinates
    // in San Francisco. None of it was on the machine. The comment above that
    // function said why: "so the first window is not an empty grid".
    //
    // An empty grid is the truth here, and the grid already has a message for
    // it. A photo manager showing you albums you did not make, of photographs
    // that do not exist, taken on a camera you do not own, is a worse first
    // window than an empty one -- and it cannot be clicked through to anything
    // real, so the impression it makes is the only thing it ever does.
    // The library is read here rather than in `new()`, so that a test does
    // not depend on the machine it runs on -- the same split as
    // `load_appearance` elsewhere in the tree.
    let mut app = match library::default_path() {
        Some(path) => PhotoApp::with_storage(path),
        None => PhotoApp::new(),
    };
    app::launch("photomanager", &mut app)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use scratchdir::ScratchDir;

    use super::*;

    // --- ImageFormat tests ---

    #[test]
    fn test_format_from_extension() {
        assert_eq!(ImageFormat::from_extension("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("JPEG"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_extension("webp"), Some(ImageFormat::WebP));
        assert_eq!(ImageFormat::from_extension("heic"), Some(ImageFormat::Heic));
        assert_eq!(ImageFormat::from_extension("cr2"), Some(ImageFormat::Raw));
        assert_eq!(ImageFormat::from_extension("xyz"), None);
    }

    #[test]
    fn test_format_extension_and_label() {
        assert_eq!(ImageFormat::Jpeg.extension(), "jpg");
        assert_eq!(ImageFormat::Jpeg.label(), "JPEG");
        assert_eq!(ImageFormat::Raw.extension(), "raw");
    }

    // --- ColorLabel tests ---

    #[test]
    fn test_color_label_cycle() {
        let mut label = ColorLabel::None;
        label = label.next();
        assert_eq!(label, ColorLabel::Red);
        label = label.next();
        assert_eq!(label, ColorLabel::Orange);
        for _ in 0..5 {
            label = label.next();
        }
        assert_eq!(label, ColorLabel::None);
    }

    // --- ExifData tests ---

    #[test]
    fn test_exif_sample() {
        let exif = ExifData::sample();
        assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
        assert_eq!(exif.resolution_str(), "8192 x 5464");
        assert!(exif.megapixels().unwrap() > 44.0);
    }

    #[test]
    fn test_exif_gps_str() {
        let mut exif = ExifData::empty();
        assert!(exif.gps_str().is_none());
        exif.gps_latitude = Some(37.7749);
        exif.gps_longitude = Some(-122.4194);
        let gps = exif.gps_str().unwrap();
        assert!(gps.contains("37.7749"));
        assert!(gps.contains("122.4194"));
    }

    #[test]
    fn test_exif_exposure_summary() {
        let mut exif = ExifData::empty();
        assert_eq!(exif.exposure_summary(), "No exposure data");
        exif.aperture = Some(2.8);
        exif.iso = Some(400);
        let summary = exif.exposure_summary();
        assert!(summary.contains("f/2.8"));
        assert!(summary.contains("ISO 400"));
    }

    #[test]
    fn test_exif_parse_empty() {
        let data = vec![0u8; 10];
        let exif = parse_exif_from_bytes(&data);
        assert!(exif.camera_make.is_none());
    }

    // --- ImageAdjustments tests ---

    #[test]
    fn test_adjustments_default() {
        let adj = ImageAdjustments::default();
        assert!(adj.is_default());
    }

    #[test]
    fn test_adjustments_not_default() {
        let adj = ImageAdjustments {
            brightness: 0.5,
            ..ImageAdjustments::default()
        };
        assert!(!adj.is_default());
    }

    #[test]
    fn test_adjustments_reset() {
        let mut adj = ImageAdjustments {
            contrast: 1.0,
            saturation: -0.5,
            ..ImageAdjustments::default()
        };
        adj.reset();
        assert!(adj.is_default());
    }

    #[test]
    fn test_adjustments_rotate() {
        let mut adj = ImageAdjustments::default();
        adj.rotate_cw();
        assert_eq!(adj.rotation, 90);
        adj.rotate_cw();
        assert_eq!(adj.rotation, 180);
        adj.rotate_ccw();
        assert_eq!(adj.rotation, 90);
    }

    // --- ImportKey tests ---

    #[test]
    fn the_same_path_and_size_give_the_same_key() {
        let h1 = ImportKey::from_metadata("/photos/test.jpg", 1000);
        let h2 = ImportKey::from_metadata("/photos/test.jpg", 1000);
        assert_eq!(h1.hash, h2.hash);
        assert!(h1.same_file(&h2));
    }

    #[test]
    fn a_different_path_or_size_gives_a_different_key() {
        let h1 = ImportKey::from_metadata("/photos/a.jpg", 1000);
        let h2 = ImportKey::from_metadata("/photos/b.jpg", 2000);
        assert_ne!(h1.hash, h2.hash);
    }

    // --- Photo tests ---

    #[test]
    fn test_photo_rating() {
        let mut photo = Photo::new(1, "/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000, 1);
        assert_eq!(photo.rating, 0);
        photo.set_rating(5);
        assert_eq!(photo.rating, 5);
        photo.set_rating(10);
        assert_eq!(photo.rating, 5); // Clamped
    }

    #[test]
    fn test_photo_tags() {
        let mut photo = Photo::new(1, "/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000, 1);
        photo.add_tag("nature");
        photo.add_tag("sunset");
        photo.add_tag("nature"); // Duplicate
        assert_eq!(photo.tags.len(), 2);
        assert!(photo.remove_tag("nature"));
        assert_eq!(photo.tags.len(), 1);
        assert!(!photo.remove_tag("nonexistent"));
    }

    #[test]
    fn test_photo_search() {
        let mut photo = Photo::new(
            1,
            "/photos/sunset.jpg",
            "sunset.jpg",
            ImageFormat::Jpeg,
            1000,
            1,
        );
        photo.add_tag("beach");
        assert!(photo.matches_search("sunset"));
        assert!(photo.matches_search("beach"));
        assert!(photo.matches_search("SUNSET")); // Case-insensitive
        assert!(!photo.matches_search("mountain"));
        assert!(photo.matches_search("")); // Empty matches all
    }

    #[test]
    fn test_photo_human_size() {
        let p1 = Photo::new(1, "/a", "a", ImageFormat::Jpeg, 500, 1);
        assert_eq!(p1.human_size(), "500 B");
        let p2 = Photo::new(2, "/b", "b", ImageFormat::Jpeg, 5_242_880, 1);
        assert_eq!(p2.human_size(), "5.0 MiB");
    }

    // --- Album tests ---

    #[test]
    fn test_album_add_remove() {
        let mut album = Album::new(1, "Test Album", 1000);
        album.add_photo(1);
        album.add_photo(2);
        album.add_photo(1); // Duplicate
        assert_eq!(album.photo_count(), 2);
        assert!(album.remove_photo(1));
        assert_eq!(album.photo_count(), 1);
        assert!(!album.remove_photo(99));
    }

    #[test]
    fn test_album_cover_cleared_on_remove() {
        let mut album = Album::new(1, "Test", 1000);
        album.add_photo(1);
        album.cover_photo = Some(1);
        album.remove_photo(1);
        assert_eq!(album.cover_photo, None);
    }

    // --- SmartAlbum tests ---

    #[test]
    fn test_smart_album_match_all() {
        let mut sa = SmartAlbum::new(1, "Best", true);
        sa.add_rule(SmartRule::MinRating(4));
        sa.add_rule(SmartRule::IsFlagged);

        let mut photo = Photo::new(1, "/a", "a", ImageFormat::Jpeg, 1000, 1);
        photo.set_rating(5);
        assert!(!sa.matches(&photo)); // Not flagged

        photo.flagged = true;
        assert!(sa.matches(&photo)); // Now matches both
    }

    #[test]
    fn test_smart_album_match_any() {
        let mut sa = SmartAlbum::new(1, "Good", false);
        sa.add_rule(SmartRule::MinRating(4));
        sa.add_rule(SmartRule::IsFlagged);

        let mut photo = Photo::new(1, "/a", "a", ImageFormat::Jpeg, 1000, 1);
        photo.set_rating(5);
        assert!(sa.matches(&photo)); // Rating matches
    }

    #[test]
    fn test_smart_rule_has_tag() {
        let rule = SmartRule::HasTag("nature".to_owned());
        let mut photo = Photo::new(1, "/a", "a", ImageFormat::Jpeg, 1000, 1);
        assert!(!rule.matches(&photo));
        photo.add_tag("nature");
        assert!(rule.matches(&photo));
    }

    #[test]
    fn test_smart_rule_format() {
        let rule = SmartRule::FormatIs(ImageFormat::Raw);
        let photo_jpg = Photo::new(1, "/a", "a", ImageFormat::Jpeg, 1000, 1);
        let photo_raw = Photo::new(2, "/b", "b", ImageFormat::Raw, 2000, 1);
        assert!(!rule.matches(&photo_jpg));
        assert!(rule.matches(&photo_raw));
    }

    #[test]
    fn test_smart_rule_camera() {
        let rule = SmartRule::CameraMake("canon".to_owned());
        let mut photo = Photo::new(1, "/a", "a", ImageFormat::Jpeg, 1000, 1);
        assert!(!rule.matches(&photo));
        photo.exif.camera_make = Some("Canon EOS".to_owned());
        assert!(rule.matches(&photo));
    }

    // --- PhotoApp tests ---

    #[test]
    fn test_app_import_photo() {
        let mut app = PhotoApp::new();
        let id = app.import_photo("/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000);
        assert_eq!(app.photos.len(), 1);
        assert!(app.find_photo(id).is_some());
    }

    #[test]
    fn test_app_trash_and_restore() {
        let mut app = PhotoApp::new();
        let id = app.import_photo("/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000);
        assert!(app.trash_photo(id));
        assert_eq!(app.photos.len(), 0);
        assert_eq!(app.trash.len(), 1);

        assert!(app.restore_from_trash(id));
        assert_eq!(app.photos.len(), 1);
        assert_eq!(app.trash.len(), 0);
    }

    #[test]
    fn test_app_empty_trash() {
        let mut app = PhotoApp::new();
        let id1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        let id2 = app.import_photo("/b", "b", ImageFormat::Png, 200);
        app.trash_photo(id1);
        app.trash_photo(id2);
        let count = app.empty_trash();
        assert_eq!(count, 2);
        assert!(app.trash.is_empty());
    }

    #[test]
    fn test_app_rate_and_label() {
        let mut app = PhotoApp::new();
        let id = app.import_photo("/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000);
        assert!(app.rate_photo(id, 4));
        assert!(app.set_color_label(id, ColorLabel::Blue));
        let photo = app.find_photo(id).unwrap();
        assert_eq!(photo.rating, 4);
        assert_eq!(photo.color_label, ColorLabel::Blue);
    }

    #[test]
    fn test_app_tagging() {
        let mut app = PhotoApp::new();
        let id = app.import_photo("/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000);
        app.add_tag(id, "nature");
        app.add_tag(id, "sunset");
        let tags = app.all_tags();
        assert_eq!(tags, vec!["nature", "sunset"]);
    }

    #[test]
    fn test_app_batch_operations() {
        let mut app = PhotoApp::new();
        let id1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        let id2 = app.import_photo("/b", "b", ImageFormat::Png, 200);
        let id3 = app.import_photo("/c", "c", ImageFormat::Raw, 300);

        let rated = app.batch_rate(&[id1, id2, id3], 4);
        assert_eq!(rated, 3);

        let tagged = app.batch_tag(&[id1, id3], "favorite");
        assert_eq!(tagged, 2);
    }

    #[test]
    fn test_app_albums() {
        let mut app = PhotoApp::new();
        let album_id = app.create_album("Vacation");
        let p1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        let p2 = app.import_photo("/b", "b", ImageFormat::Png, 200);

        assert!(app.add_to_album(album_id, p1));
        assert!(app.add_to_album(album_id, p2));

        let album = app.albums.iter().find(|a| a.id == album_id).unwrap();
        assert_eq!(album.photo_count(), 2);

        assert!(app.remove_from_album(album_id, p1));
        let album = app.albums.iter().find(|a| a.id == album_id).unwrap();
        assert_eq!(album.photo_count(), 1);
    }

    #[test]
    fn test_app_rename_album() {
        let mut app = PhotoApp::new();
        let id = app.create_album("Old Name");
        assert!(app.rename_album(id, "New Name"));
        assert_eq!(app.albums.first().unwrap().name, "New Name");
    }

    #[test]
    fn test_app_delete_album() {
        let mut app = PhotoApp::new();
        let id = app.create_album("To Delete");
        assert!(app.delete_album(id));
        assert!(app.albums.is_empty());
    }

    #[test]
    fn test_app_smart_albums() {
        let mut app = PhotoApp::new();
        let sa_id = app.create_smart_album("Top Rated", true);
        app.add_smart_rule(sa_id, SmartRule::MinRating(4));

        let p1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        let p2 = app.import_photo("/b", "b", ImageFormat::Png, 200);
        app.rate_photo(p1, 5);
        app.rate_photo(p2, 2);

        let matches = app.smart_album_photos(sa_id);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], p1);
    }

    #[test]
    fn test_app_duplicate_detection() {
        let mut app = PhotoApp::new();
        // Same path + size = same hash = duplicate
        app.import_photo("/photos/dup.jpg", "dup.jpg", ImageFormat::Jpeg, 1000);
        app.import_photo("/photos/dup.jpg", "dup.jpg", ImageFormat::Jpeg, 1000);

        let dups = app.find_duplicates();
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].len(), 2);
    }

    #[test]
    fn test_app_simulate_import_detects_duplicates() {
        let mut app = PhotoApp::new();
        app.import_photo("/dir/photo1.jpg", "photo1.jpg", ImageFormat::Jpeg, 500);

        let result = app.simulate_import(
            "/dir",
            &[
                ("photo1.jpg", ImageFormat::Jpeg, 500), // Duplicate
                ("photo2.png", ImageFormat::Png, 1000), // New
            ],
        );

        assert_eq!(result.imported_count, 1);
        assert_eq!(result.duplicate_count, 1);
    }

    #[test]
    fn test_app_visible_photos_all() {
        let mut app = PhotoApp::new();
        app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        app.import_photo("/b", "b", ImageFormat::Png, 200);
        app.sidebar_selection = SidebarItem::AllPhotos;

        let visible = app.visible_photos();
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn test_app_visible_photos_favorites() {
        let mut app = PhotoApp::new();
        let p1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        let _p2 = app.import_photo("/b", "b", ImageFormat::Png, 200);
        app.toggle_flag(p1);
        app.sidebar_selection = SidebarItem::Favorites;

        let visible = app.visible_photos();
        assert_eq!(visible.len(), 1);
    }

    #[test]
    fn test_app_search_filter() {
        let mut app = PhotoApp::new();
        app.import_photo("/photos/sunset.jpg", "sunset.jpg", ImageFormat::Jpeg, 100);
        app.import_photo(
            "/photos/mountain.png",
            "mountain.png",
            ImageFormat::Png,
            200,
        );
        app.set_search("sunset");

        let visible = app.visible_photos();
        assert_eq!(visible.len(), 1);
    }

    #[test]
    fn test_app_sort_by_rating() {
        let mut app = PhotoApp::new();
        let p1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        let p2 = app.import_photo("/b", "b", ImageFormat::Png, 200);
        let p3 = app.import_photo("/c", "c", ImageFormat::Raw, 300);
        app.rate_photo(p1, 3);
        app.rate_photo(p2, 5);
        app.rate_photo(p3, 1);
        app.sort_order = PhotoSort::Rating;

        let visible = app.visible_photos();
        assert_eq!(visible[0], p2); // Highest rating first
    }

    #[test]
    fn test_app_slideshow() {
        let mut app = PhotoApp::new();
        app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        app.import_photo("/b", "b", ImageFormat::Png, 200);

        app.start_slideshow();
        assert!(app.slideshow.is_some());
        assert_eq!(app.view_mode, ViewMode::Slideshow);

        app.slideshow_next();
        assert_eq!(app.slideshow.as_ref().unwrap().current_index, 1);

        app.slideshow_prev();
        assert_eq!(app.slideshow.as_ref().unwrap().current_index, 0);

        app.stop_slideshow();
        assert!(app.slideshow.is_none());
        assert_eq!(app.view_mode, ViewMode::Grid);
    }

    #[test]
    fn test_app_timeline_groups() {
        let mut app = PhotoApp::new();
        // Import several photos (they'll get sequential timestamps)
        for i in 0..5 {
            app.import_photo(
                format!("/photo_{i}.jpg"),
                &format!("photo_{i}.jpg"),
                ImageFormat::Jpeg,
                1000,
            );
        }
        let groups = app.timeline_groups();
        assert!(!groups.is_empty());
    }

    #[test]
    fn test_app_cycle_sort() {
        let mut app = PhotoApp::new();
        assert_eq!(app.sort_order, PhotoSort::DateAdded);
        app.cycle_sort();
        assert_eq!(app.sort_order, PhotoSort::DateTaken);
        app.cycle_sort();
        assert_eq!(app.sort_order, PhotoSort::FileName);
    }

    #[test]
    fn test_app_cycle_thumb_size() {
        let mut app = PhotoApp::new();
        let initial = app.current_thumb_size();
        app.cycle_thumb_size();
        let next = app.current_thumb_size();
        assert_ne!(initial, next);
    }

    #[test]
    fn test_app_library_stats() {
        let mut app = PhotoApp::new();
        let p1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 1000);
        let p2 = app.import_photo("/b", "b", ImageFormat::Jpeg, 2000);
        let _p3 = app.import_photo("/c", "c", ImageFormat::Png, 3000);
        app.rate_photo(p1, 5);
        app.add_tag(p2, "test");
        app.toggle_flag(p1);
        app.create_album("Test");

        let stats = app.library_stats();
        assert_eq!(stats.total_photos, 3);
        assert_eq!(stats.total_size, 6000);
        assert_eq!(stats.total_rated, 1);
        assert_eq!(stats.total_tagged, 1);
        assert_eq!(stats.total_flagged, 1);
        assert_eq!(stats.total_albums, 1);
    }

    #[test]
    fn test_app_batch_add_to_album() {
        let mut app = PhotoApp::new();
        let album_id = app.create_album("Batch Test");
        let p1 = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        let p2 = app.import_photo("/b", "b", ImageFormat::Png, 200);

        assert!(app.batch_add_to_album(&[p1, p2], album_id));
        let album = app.albums.first().unwrap();
        assert_eq!(album.photo_count(), 2);
    }

    #[test]
    fn test_app_trash_removes_from_album() {
        let mut app = PhotoApp::new();
        let album_id = app.create_album("Test");
        let pid = app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        app.add_to_album(album_id, pid);

        app.trash_photo(pid);
        let album = app.albums.first().unwrap();
        assert_eq!(album.photo_count(), 0);
    }

    #[test]
    fn test_slideshow_state() {
        let mut ss = SlideshowState::new(vec![1, 2, 3]);
        assert_eq!(ss.current_photo(), Some(1));
        ss.advance();
        assert_eq!(ss.current_photo(), Some(2));
        ss.advance();
        assert_eq!(ss.current_photo(), Some(3));
        ss.advance();
        assert_eq!(ss.current_photo(), Some(1)); // Wraps around

        ss.go_back();
        assert_eq!(ss.current_photo(), Some(3));

        ss.toggle_pause();
        assert!(ss.paused);
        ss.toggle_pause();
        assert!(!ss.paused);
    }

    #[test]
    fn test_slideshow_empty() {
        let ss = SlideshowState::new(vec![]);
        assert_eq!(ss.current_photo(), None);
    }

    #[test]
    fn test_export_options_default() {
        let opts = ExportOptions::default();
        assert_eq!(opts.format, ImageFormat::Jpeg);
        assert_eq!(opts.quality, 90);
        assert!(opts.max_dimension.is_none());
        assert!(!opts.strip_exif);
    }

    #[test]
    fn test_face_region() {
        let face = FaceRegion::new(100.0, 200.0, 50.0, 50.0).with_name("Alice");
        assert_eq!(face.name.as_deref(), Some("Alice"));
        assert_eq!(face.x, 100.0);
    }

    /// A thumbnail's file name reaches the renderer whole, with the thumbnail
    /// width alongside it, so the cut is made by something that measured the
    /// text. The removed `truncate_str` cut it to `(thumb / 7.0)` characters
    /// first — a guess about a 9 px font that was wrong by roughly half — and
    /// its byte-offset slice silently declined to cut a name with an umlaut
    /// in it at all.
    #[test]
    fn a_thumbnail_name_is_bounded_by_width_not_pre_truncated() {
        let name = "Sommerferien_Österreich_2026_Abend_am_See.jpg";
        let mut app = PhotoApp::new();
        app.import_photo(format!("/photos/{name}"), name, ImageFormat::Jpeg, 1000);
        let cmds = app.render_commands(1400.0, 900.0);
        let label = cmds
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Text {
                    text, max_width, ..
                } if text == name => Some(*max_width),
                _ => None,
            })
            .expect("the thumbnail is labelled with the whole file name");
        assert!(
            label.is_some(),
            "and the renderer is told how wide the thumbnail is"
        );
    }

    #[test]
    fn test_render_produces_commands() {
        let mut app = PhotoApp::new();
        app.import_photo("/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000);
        let cmds = app.render_commands(1400.0, 900.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_single_view() {
        let mut app = PhotoApp::new();
        let pid = app.import_photo("/test.jpg", "test.jpg", ImageFormat::Jpeg, 1000);
        app.selected_photo = Some(pid);
        app.view_mode = ViewMode::Single;
        let cmds = app.render_commands(1400.0, 900.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_slideshow_view() {
        let mut app = PhotoApp::new();
        app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        app.import_photo("/b", "b", ImageFormat::Png, 200);
        app.start_slideshow();
        let cmds = app.render_commands(1400.0, 900.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_timeline_view() {
        let mut app = PhotoApp::new();
        app.import_photo("/a", "a", ImageFormat::Jpeg, 100);
        app.view_mode = ViewMode::Timeline;
        let cmds = app.render_commands(1400.0, 900.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_import_with_exif() {
        let mut app = PhotoApp::new();
        let id = app.import_photo_with_exif(
            "/photo.jpg",
            "photo.jpg",
            ImageFormat::Jpeg,
            5_000_000,
            ExifData::sample(),
        );
        let photo = app.find_photo(id).unwrap();
        assert_eq!(photo.exif.camera_make.as_deref(), Some("Canon"));
        assert_eq!(photo.exif.iso, Some(400));
    }

    #[test]
    fn test_photo_sort_next() {
        assert_eq!(PhotoSort::DateAdded.next(), PhotoSort::DateTaken);
        assert_eq!(PhotoSort::Rating.next(), PhotoSort::DateAdded);
    }

    #[test]
    fn test_slideshow_transition_cycle() {
        let mut t = SlideshowTransition::None;
        t = t.next();
        assert_eq!(t, SlideshowTransition::Fade);
        t = t.next();
        assert_eq!(t, SlideshowTransition::SlideLeft);
    }

    #[test]
    fn test_import_result_empty() {
        let result = ImportResult::empty();
        assert_eq!(result.imported_count, 0);
        assert_eq!(result.duplicate_count, 0);
    }

    #[test]
    fn test_smart_rule_label() {
        assert_eq!(SmartRule::MinRating(4).label(), "Rating >= 4");
        assert_eq!(SmartRule::IsFlagged.label(), "Flagged");
    }

    // ======================================================================
    // Window, input and the slideshow clock
    // ======================================================================

    use guitk::event::Modifiers;

    fn key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn typed(k: Key, ch: char) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        })
    }

    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn shift_key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            text: String::new(),
        })
    }

    /// Shift and an arrow select more than one photograph.
    ///
    /// `selected_photos` is a `Vec` production code never pushed to, so the
    /// batch operations were written, tested and unreachable. The grid already
    /// drew a card as selected when it was in this list.
    #[test]
    fn shift_and_an_arrow_selects_more_than_one() {
        let mut app = app_with_n_pictures("multi", 3);
        app.set_window_size(900.0, 700.0);
        // The first card *on screen*: the default sort is newest-first, so
        // `photos.first()` is the last one and an arrow from it goes nowhere.
        app.selected_photo = app.visible_photos().first().copied();

        app.handle_event(&shift_key(Key::Right));

        assert_eq!(
            app.selected_photos.len(),
            2,
            "both ends of the step should be in the set: {:?}",
            app.selected_photos
        );
        app.handle_event(&shift_key(Key::Right));
        assert_eq!(
            app.selected_photos.len(),
            3,
            "a run should collect what it passes over"
        );
    }

    /// An arrow on its own starts again.
    ///
    /// Leaving cards highlighted behind a cursor that has moved away from them
    /// is how a batch operation surprises somebody.
    #[test]
    fn an_arrow_without_shift_starts_the_selection_again() {
        let mut app = app_with_n_pictures("single", 3);
        app.set_window_size(900.0, 700.0);
        // The first card *on screen*: the default sort is newest-first, so
        // `photos.first()` is the last one and an arrow from it goes nowhere.
        app.selected_photo = app.visible_photos().first().copied();
        app.handle_event(&shift_key(Key::Right));
        assert!(!app.selected_photos.is_empty(), "the control failed");

        app.handle_event(&key(Key::Right));

        assert!(
            app.selected_photos.is_empty(),
            "a plain arrow left the old set behind"
        );
    }

    /// A rating applies to everything selected.
    ///
    /// `batch_rate` has been written and tested since this crate existed, with
    /// no way to reach it.
    #[test]
    fn a_rating_applies_to_everything_selected() {
        let mut app = app_with_n_pictures("rateall", 3);
        app.set_window_size(900.0, 700.0);
        // The first card *on screen*: the default sort is newest-first, so
        // `photos.first()` is the last one and an arrow from it goes nowhere.
        app.selected_photo = app.visible_photos().first().copied();
        app.handle_event(&shift_key(Key::Right));
        let chosen = app.selected_photos.clone();
        assert_eq!(chosen.len(), 2, "the control failed");

        app.handle_event(&typed(Key::Num4, '4'));

        for pid in &chosen {
            assert_eq!(
                app.find_photo(*pid).expect("a photo").rating,
                4,
                "photograph {pid} was not rated"
            );
        }
        // The third was never selected and must be untouched.
        let untouched = app
            .photos
            .iter()
            .filter(|p| !chosen.contains(&p.id))
            .all(|p| p.rating == 0);
        assert!(untouched, "a photograph outside the selection was rated");
    }

    /// A plain click starts the selection again.
    #[test]
    fn a_plain_click_starts_the_selection_again() {
        let mut app = app_with_n_pictures("clickreset", 3);
        app.set_window_size(900.0, 700.0);
        // The first card *on screen*: the default sort is newest-first, so
        // `photos.first()` is the last one and an arrow from it goes nowhere.
        app.selected_photo = app.visible_photos().first().copied();
        app.handle_event(&shift_key(Key::Right));
        assert!(!app.selected_photos.is_empty(), "the control failed");

        let cell = app.thumb_rect(2).expect("a third card");
        app.handle_event(&click(cell.x + 4.0, cell.y + 4.0));

        assert!(
            app.selected_photos.is_empty(),
            "the click joined the old set instead of replacing it"
        );
    }

    /// The menu adds the whole selection when it was raised on part of it.
    #[test]
    fn the_menu_adds_the_whole_selection() {
        let mut app = app_with_n_pictures("batchalbum", 3);
        app.set_window_size(900.0, 700.0);
        let album = app.create_album("Holiday");
        // The first card *on screen*: the default sort is newest-first, so
        // `photos.first()` is the last one and an arrow from it goes nowhere.
        app.selected_photo = app.visible_photos().first().copied();
        app.handle_event(&shift_key(Key::Right));
        let chosen = app.selected_photos.clone();
        assert_eq!(chosen.len(), 2, "the control failed");

        // Raise it on one of the selected cards.
        let cell = app.thumb_rect(0).expect("a first card");
        app.handle_event(&right_click(cell.x + 4.0, cell.y + 4.0));
        app.choose_album_from_menu(album);

        let in_album = app
            .albums
            .iter()
            .find(|a| a.id == album)
            .expect("the album")
            .photo_ids
            .clone();
        assert_eq!(in_album.len(), 2, "only part of the selection went in");
        assert_eq!(
            app.status_message.as_deref(),
            Some("Added 2 photos to Holiday")
        );
    }

    fn right_click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Right),
        })
    }

    /// Where the first grid card is drawn.
    fn first_card_point(app: &PhotoApp) -> (f32, f32) {
        let cell = app.thumb_rect(0).expect("a first card");
        (cell.x + 4.0, cell.y + 4.0)
    }

    /// Right-clicking a photograph offers the albums it could go into.
    #[test]
    fn right_clicking_a_photograph_offers_the_albums() {
        let mut app = app_with_n_pictures("menu", 1);
        app.set_window_size(900.0, 700.0);
        app.create_album("Holiday");
        let (x, y) = first_card_point(&app);

        app.handle_event(&right_click(x, y));

        assert!(app.photo_menu.is_some(), "no menu appeared");
        assert_eq!(
            app.menu_photo,
            app.photos.first().map(|p| p.id),
            "the menu is about the wrong photograph"
        );
    }

    /// Choosing an album puts the photograph in it.
    ///
    /// The point of the whole feature: `add_to_album` has been written and
    /// tested since this crate existed and had no caller outside the test
    /// module, so an album stayed empty however many photographs you had.
    #[test]
    fn choosing_an_album_puts_the_photograph_in_it() {
        let mut app = app_with_n_pictures("addto", 1);
        app.set_window_size(900.0, 700.0);
        let album = app.create_album("Holiday");
        let pid = app.photos.first().expect("one").id;
        let (x, y) = first_card_point(&app);
        app.handle_event(&right_click(x, y));

        app.choose_album_from_menu(album);

        let in_album = app
            .albums
            .iter()
            .find(|a| a.id == album)
            .expect("the album")
            .photo_ids
            .clone();
        assert_eq!(in_album, vec![pid], "the photograph did not go in");
        assert_eq!(
            app.status_message.as_deref(),
            Some("Added to Holiday"),
            "nothing on screen said it worked"
        );
    }

    /// With no albums the menu still opens and says why it is empty.
    ///
    /// A menu that refused to appear would leave no way to find out *why*
    /// nothing happened.
    #[test]
    fn with_no_albums_the_menu_still_opens_and_says_so() {
        let mut app = app_with_n_pictures("noalbums", 1);
        app.set_window_size(900.0, 700.0);
        assert!(app.albums.is_empty(), "the control failed");
        let (x, y) = first_card_point(&app);

        app.handle_event(&right_click(x, y));

        assert!(app.photo_menu.is_some(), "the menu refused to appear");
    }

    /// A click while the menu is up dismisses it and does not fall through.
    ///
    /// Without that, dismissing a menu over the grid would also select
    /// whatever card was underneath it.
    #[test]
    fn a_click_dismisses_the_menu_without_falling_through() {
        let mut app = app_with_n_pictures("dismiss", 2);
        app.set_window_size(900.0, 700.0);
        app.create_album("Holiday");
        let (x, y) = first_card_point(&app);
        app.handle_event(&right_click(x, y));
        let before = app.selected_photo;

        // Far from the menu, over the grid.
        let elsewhere = app.thumb_rect(1).expect("a second card");
        assert!(
            app.handle_event(&click(elsewhere.x + 4.0, elsewhere.y + 4.0)),
            "the click was not consumed"
        );

        assert!(app.photo_menu.is_none(), "the menu is still up");
        assert_eq!(
            app.selected_photo, before,
            "the dismissing click also selected the card behind it"
        );
    }

    /// A photograph already in an album is not offered it again.
    #[test]
    fn a_photograph_already_in_an_album_is_not_offered_it_twice() {
        let mut app = app_with_n_pictures("twice", 1);
        app.set_window_size(900.0, 700.0);
        let album = app.create_album("Holiday");
        let pid = app.photos.first().expect("one").id;
        assert!(app.add_to_album(album, pid), "the control failed");

        let (x, y) = first_card_point(&app);
        app.handle_event(&right_click(x, y));

        // Choosing it anyway must not duplicate the entry.
        app.choose_album_from_menu(album);
        let in_album = app
            .albums
            .iter()
            .find(|a| a.id == album)
            .expect("the album")
            .photo_ids
            .clone();
        assert_eq!(in_album, vec![pid], "the photograph was added twice");
    }

    /// Where the "New Album" row is drawn, so a click can land on it.
    fn new_album_row_y(app: &PhotoApp) -> f32 {
        let mut y = TOOLBAR_HEIGHT;
        for row in app.sidebar_rows() {
            let h = row.height();
            if matches!(row, SidebarRow::Action { .. }) {
                return y + h / 2.0;
            }
            y += h;
        }
        panic!("the sidebar has no New Album row");
    }

    fn start_naming(app: &mut PhotoApp) {
        app.set_window_size(900.0, 700.0);
        let y = new_album_row_y(app);
        app.handle_event(&click(20.0, y));
        assert!(
            app.naming_album.is_some(),
            "the click did not start naming an album"
        );
    }

    /// The sidebar offers a way to make an album.
    ///
    /// `create_album` has been written and tested since this crate existed,
    /// and every caller was in the test module -- so the ALBUMS heading has
    /// never had anything under it that a user put there.
    #[test]
    fn the_sidebar_offers_a_way_to_make_an_album() {
        let mut app = PhotoApp::new();
        app.set_window_size(900.0, 700.0);
        let y = new_album_row_y(&app);

        assert_eq!(
            app.sidebar_action_at(20.0, y),
            Some(SidebarAction::NewAlbum),
            "the row is drawn but is not a control"
        );
        assert_eq!(
            app.sidebar_item_at(20.0, y),
            None,
            "an action is not somewhere to go"
        );
    }

    /// Typing a name and pressing Enter makes the album.
    #[test]
    fn typing_a_name_and_pressing_enter_creates_the_album() {
        let mut app = PhotoApp::new();
        assert!(app.albums.is_empty(), "the control failed");
        start_naming(&mut app);

        for (k, ch) in [(Key::P, 'p'), (Key::I, 'i'), (Key::E, 'e'), (Key::R, 'r')] {
            app.handle_event(&typed(k, ch));
        }
        app.handle_event(&key(Key::Enter));

        assert_eq!(app.albums.len(), 1, "no album was made");
        let album = app.albums.first().expect("one");
        assert_eq!(album.name, "pier");
        assert_eq!(
            app.sidebar_selection,
            SidebarItem::Album(album.id),
            "the new album did not become the view"
        );
        assert!(app.naming_album.is_none(), "the row is still taking typing");
    }

    /// A digit typed into an album name does not rate a photograph.
    ///
    /// The same hazard as the search box: `0`-`5` rate the selection and `f`
    /// flags it, so an album called "5 star" would rewrite the library as it
    /// was typed.
    #[test]
    fn a_digit_typed_into_an_album_name_does_not_rate_a_photograph() {
        let mut app = app_with_n_pictures("albumdigit", 1);
        let pid = app.photos.first().expect("one").id;
        app.selected_photo = Some(pid);

        // Control: unfocused, the digit really does rate.
        app.handle_event(&typed(Key::Num5, '5'));
        assert_eq!(
            app.find_photo(pid).expect("one").rating,
            5,
            "the control failed: digits do not rate, so this proves nothing"
        );

        start_naming(&mut app);
        app.handle_event(&typed(Key::Num3, '3'));

        assert_eq!(
            app.find_photo(pid).expect("one").rating,
            5,
            "typing an album name changed a photograph's rating"
        );
        assert_eq!(app.naming_album.as_deref(), Some("3"));
    }

    /// Escape abandons a half-typed name.
    #[test]
    fn escape_abandons_a_half_typed_album_name() {
        let mut app = PhotoApp::new();
        start_naming(&mut app);
        app.handle_event(&typed(Key::P, 'p'));

        app.handle_event(&key(Key::Escape));

        assert!(app.naming_album.is_none(), "still naming");
        assert!(app.albums.is_empty(), "escape made an album anyway");
    }

    /// Enter on a row nobody typed into makes nothing.
    ///
    /// Much more likely a change of mind than a request for a nameless album.
    #[test]
    fn enter_on_an_empty_name_makes_no_album() {
        let mut app = PhotoApp::new();
        start_naming(&mut app);

        app.handle_event(&key(Key::Enter));

        assert!(app.albums.is_empty(), "an album with no name was made");
        assert!(app.naming_album.is_none(), "the row is still taking typing");
    }

    /// Only one text field takes the keyboard at a time.
    #[test]
    fn focusing_the_search_box_abandons_an_album_name() {
        let mut app = PhotoApp::new();
        start_naming(&mut app);
        app.handle_event(&typed(Key::P, 'p'));

        focus_search(&mut app);

        assert!(
            app.naming_album.is_none(),
            "two rows would both have been showing a caret"
        );
    }

    /// The search box is a control the click handler knows about.
    ///
    /// It was drawn from the start and was never in the layout as anything
    /// but a gap to position the next button past.
    #[test]
    fn the_search_box_is_a_control_that_can_be_clicked() {
        let mut app = PhotoApp::new();
        app.set_window_size(900.0, 700.0);
        let hit = app
            .toolbar_controls()
            .into_iter()
            .find(|(c, _)| *c == ToolbarControl::Search)
            .map(|(_, r)| r)
            .expect("the search box is not a control");
        assert_eq!(
            app.toolbar_control_at(hit.x + 4.0, hit.y + 4.0),
            Some(ToolbarControl::Search),
            "a click inside the drawn box does not land on it"
        );
    }

    /// Focus the search box, wherever the toolbar happens to put it.
    fn focus_search(app: &mut PhotoApp) {
        app.set_window_size(900.0, 700.0);
        let hit = app
            .toolbar_controls()
            .into_iter()
            .find(|(c, _)| *c == ToolbarControl::Search)
            .map(|(_, r)| r)
            .expect("the search box is a control");
        app.handle_event(&click(hit.x + 4.0, hit.y + 4.0));
        assert!(app.search_focused, "the click did not take the keyboard");
    }

    /// Typing filters the library, which is what the box has always promised.
    #[test]
    fn typing_into_the_search_box_filters_the_library() {
        let mut app = app_with_n_pictures("srch", 2);
        let first = app.photos.first().expect("one").id;
        app.add_tag(first, "pier");
        assert_eq!(app.visible_photos().len(), 2, "the control failed");

        focus_search(&mut app);
        for (k, ch) in [(Key::P, 'p'), (Key::I, 'i'), (Key::E, 'e'), (Key::R, 'r')] {
            app.handle_event(&typed(k, ch));
        }

        assert_eq!(app.search_query, "pier");
        assert_eq!(
            app.visible_photos().len(),
            1,
            "the query was stored but nothing was filtered"
        );
    }

    /// A digit typed into the search box does not rate a photograph.
    ///
    /// The reason focus has to consume everything. `0`-`5` rate the selected
    /// photograph and `f` flags it, so without this, searching for a filename
    /// with a digit in it would quietly edit the library while the user
    /// believed they were typing a query.
    #[test]
    fn a_digit_typed_into_the_search_box_does_not_rate_a_photograph() {
        let mut app = app_with_n_pictures("digits", 1);
        let pid = app.photos.first().expect("one").id;
        app.selected_photo = Some(pid);

        // Control: with the box unfocused, the digit really does rate.
        app.handle_event(&typed(Key::Num5, '5'));
        assert_eq!(
            app.find_photo(pid).expect("one").rating,
            5,
            "the control failed: digits do not rate at all, so this proves nothing"
        );

        focus_search(&mut app);
        app.handle_event(&typed(Key::Num3, '3'));

        assert_eq!(
            app.find_photo(pid).expect("one").rating,
            5,
            "typing into the search box changed a photograph's rating"
        );
        assert_eq!(app.search_query, "3", "and the digit went into the query");
    }

    /// `f` does not flag a photograph while the box has the keyboard either.
    #[test]
    fn a_letter_typed_into_the_search_box_does_not_flag_a_photograph() {
        let mut app = app_with_n_pictures("flagging", 1);
        let pid = app.photos.first().expect("one").id;
        app.selected_photo = Some(pid);
        assert!(!app.find_photo(pid).expect("one").flagged);

        focus_search(&mut app);
        app.handle_event(&typed(Key::F, 'f'));

        assert!(
            !app.find_photo(pid).expect("one").flagged,
            "typing into the search box flagged a photograph"
        );
        assert_eq!(app.search_query, "f");
    }

    /// Backspace removes the last character.
    #[test]
    fn backspace_removes_the_last_character() {
        let mut app = app_with_n_pictures("backspace", 1);
        focus_search(&mut app);
        app.handle_event(&typed(Key::P, 'p'));
        app.handle_event(&typed(Key::I, 'i'));
        assert_eq!(app.search_query, "pi");

        app.handle_event(&key(Key::Backspace));
        assert_eq!(app.search_query, "p");
    }

    /// Escape abandons the search rather than merely leaving the box.
    ///
    /// A filter left in place by a box that no longer looks active is a
    /// library that appears half-missing with nothing on screen explaining
    /// why.
    #[test]
    fn escape_abandons_the_search_and_restores_the_library() {
        let mut app = app_with_n_pictures("escape", 2);
        let first = app.photos.first().expect("one").id;
        app.add_tag(first, "pier");
        focus_search(&mut app);
        for (k, ch) in [(Key::P, 'p'), (Key::I, 'i'), (Key::E, 'e'), (Key::R, 'r')] {
            app.handle_event(&typed(k, ch));
        }
        assert_eq!(app.visible_photos().len(), 1, "the control failed");

        app.handle_event(&key(Key::Escape));

        assert!(!app.search_focused, "escape left the keyboard in the box");
        assert!(
            app.search_query.is_empty(),
            "escape left the query in place"
        );
        assert_eq!(
            app.visible_photos().len(),
            2,
            "the library did not come back"
        );
    }

    /// Clicking anything else takes the keyboard out of the box.
    #[test]
    fn clicking_elsewhere_takes_the_keyboard_out_of_the_search_box() {
        let mut app = app_with_n_pictures("unfocus", 1);
        focus_search(&mut app);

        // The sort button, which is a control and is not the search box.
        let sort = app
            .toolbar_controls()
            .into_iter()
            .find(|(c, _)| *c == ToolbarControl::Sort)
            .map(|(_, r)| r)
            .expect("sort is a control");
        app.handle_event(&click(sort.x + 4.0, sort.y + 4.0));

        assert!(!app.search_focused, "the keyboard stayed in the search box");
    }

    /// A library of `n` photos, all visible.
    fn library(n: usize) -> PhotoApp {
        let mut app = PhotoApp::new();
        app.set_window_size(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..n {
            app.import_photo(
                format!("/photos/p{i:04}.jpg"),
                &format!("p{i:04}.jpg"),
                ImageFormat::Jpeg,
                1_000_000,
            );
        }
        app
    }

    // --- the sidebar ---

    #[test]
    fn clicking_a_sidebar_row_goes_where_it_points() {
        let mut app = library(4);
        let album = app.create_album("Trip");
        // Find where the album row was drawn by walking the same rows.
        let mut y = TOOLBAR_HEIGHT;
        let mut target_y = None;
        for row in app.sidebar_rows() {
            if let SidebarRow::Item {
                target: SidebarItem::Album(id),
                ..
            } = row
                && id == album
            {
                target_y = Some(y + ITEM_HEIGHT / 2.0);
            }
            y += row.height();
        }
        let y = target_y.expect("the album has a row");
        assert_eq!(
            app.sidebar_item_at(20.0, y),
            Some(SidebarItem::Album(album))
        );
        assert!(app.handle_event(&click(20.0, y)));
        assert_eq!(app.sidebar_selection, SidebarItem::Album(album));
    }

    #[test]
    fn a_sidebar_heading_is_not_a_button() {
        let app = library(2);
        let mut y = TOOLBAR_HEIGHT;
        let mut checked = 0;
        for row in app.sidebar_rows() {
            if matches!(row, SidebarRow::Header(_) | SidebarRow::Gap(_)) {
                assert_eq!(
                    app.sidebar_item_at(20.0, y + 1.0),
                    None,
                    "LIBRARY is a label, not a place to go"
                );
                checked += 1;
            }
            y += row.height();
        }
        assert!(checked > 0);
    }

    #[test]
    fn every_sidebar_row_is_reachable_where_it_is_drawn() {
        let mut app = library(3);
        app.create_album("A");
        app.create_album("B");
        let smart = app.create_smart_album("Best", true);
        app.add_smart_rule(smart, SmartRule::MinRating(4));
        let mut y = TOOLBAR_HEIGHT;
        let mut items = 0;
        for row in app.sidebar_rows() {
            if let SidebarRow::Item { target, .. } = &row {
                assert_eq!(
                    app.sidebar_item_at(20.0, y + ITEM_HEIGHT / 2.0),
                    Some(*target),
                    "{target:?} is drawn at y={y} and must be clickable there"
                );
                items += 1;
            }
            y += row.height();
        }
        assert!(items >= 6, "the fixture should have six rows or more");
    }

    #[test]
    fn a_click_right_of_the_sidebar_is_not_a_sidebar_click() {
        let app = library(2);
        assert_eq!(
            app.sidebar_item_at(SIDEBAR_WIDTH + 4.0, TOOLBAR_HEIGHT + 40.0),
            None
        );
    }

    #[test]
    fn choosing_an_album_starts_at_the_top_of_it() {
        let mut app = library(200);
        app.grid_scroll = 12;
        app.selected_photo = app.visible_photos().last().copied();
        let album = app.create_album("Trip");
        app.select_sidebar(SidebarItem::Album(album));
        assert_eq!(app.grid_scroll, 0);
        assert_eq!(
            app.selected_photo, None,
            "a photo selected in one album is not in another"
        );
    }

    // --- the toolbar ---

    #[test]
    fn clicking_a_view_button_switches_view() {
        let mut app = library(3);
        let controls = app.toolbar_controls();
        let (_, rect) = controls
            .iter()
            .find(|(c, _)| *c == ToolbarControl::View(ViewMode::Timeline))
            .expect("a Timeline button");
        assert!(app.handle_event(&click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)));
        assert_eq!(app.view_mode, ViewMode::Timeline);
    }

    #[test]
    fn every_toolbar_control_is_reachable_where_it_is_drawn() {
        let app = library(3);
        for (control, rect) in app.toolbar_controls() {
            assert_eq!(
                app.toolbar_control_at(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                Some(control),
                "{control:?} is drawn at {rect:?} and must be clickable there"
            );
        }
    }

    #[test]
    fn the_slideshow_button_starts_and_stops_it() {
        let mut app = library(3);
        let controls = app.toolbar_controls();
        let (_, rect) = controls
            .iter()
            .find(|(c, _)| *c == ToolbarControl::Slideshow)
            .expect("a Slideshow button");
        let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        app.handle_event(&click(x, y));
        assert!(app.slideshow.is_some());
        assert_eq!(app.view_mode, ViewMode::Slideshow);
        app.handle_event(&click(x, y));
        assert!(app.slideshow.is_none());
        assert_eq!(app.view_mode, ViewMode::Grid);
    }

    #[test]
    fn the_slideshow_button_follows_the_window_edge() {
        let mut app = library(3);
        let narrow = app.toolbar_controls();
        app.set_window_size(1920.0, 1080.0);
        let wide = app.toolbar_controls();
        let x_of = |cs: &Vec<(ToolbarControl, Rect)>| {
            cs.iter()
                .find(|(c, _)| *c == ToolbarControl::Slideshow)
                .map_or(0.0, |(_, r)| r.x)
        };
        assert!(
            x_of(&wide) > x_of(&narrow),
            "it is pinned to the right-hand edge, so it moves with it"
        );
    }

    #[test]
    fn leaving_the_slideshow_by_a_view_button_stops_its_clock() {
        let mut app = library(3);
        app.start_slideshow();
        app.press_toolbar(ToolbarControl::View(ViewMode::Grid));
        assert!(
            app.slideshow.is_none(),
            "a slideshow left running behind the grid keeps asking for frames"
        );
        assert_eq!(app.tick_interval(), None);
    }

    // --- the grid ---

    #[test]
    fn the_grid_scrolls_instead_of_hiding_everything_past_the_first_screen() {
        let mut app = library(400);
        let window = app.grid_window();
        assert!(
            window.count < 400_usize.div_ceil(app.grid_columns()),
            "the fixture must overflow the window"
        );
        assert!(app.thumb_rect(0).is_some());
        let past = window.count.saturating_mul(app.grid_columns());
        assert_eq!(
            app.thumb_rect(past),
            None,
            "a row below the fold is not drawn"
        );
        app.grid_scroll = window.count;
        assert!(
            app.thumb_rect(past).is_some(),
            "and scrolling is what brings it into view -- there was no offset \
             at all before, so a library of four hundred photos showed the \
             first screenful and hid the rest for good"
        );
    }

    #[test]
    fn clicking_a_thumbnail_selects_that_photo() {
        let mut app = library(20);
        let visible = app.visible_photos();
        let rect = app.thumb_rect(5).expect("a sixth thumbnail");
        let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        assert_eq!(app.photo_at(x, y), Some(visible[5]));
        assert!(app.handle_event(&click(x, y)));
        assert_eq!(app.selected_photo, Some(visible[5]));
    }

    #[test]
    fn clicking_a_scrolled_grid_selects_the_photo_that_is_there() {
        let mut app = library(400);
        app.grid_scroll = 5;
        let cols = app.grid_columns();
        let visible = app.visible_photos();
        let index = cols.saturating_mul(5);
        let rect = app.thumb_rect(index).expect("the first drawn row");
        assert_eq!(
            app.photo_at(rect.x + 4.0, rect.y + 4.0),
            Some(visible[index]),
            "the top row of a grid scrolled by five is the sixth row of photos"
        );
    }

    #[test]
    fn clicking_the_gap_between_thumbnails_selects_nothing() {
        let mut app = library(20);
        let rect = app.thumb_rect(0).expect("a thumbnail");
        assert_eq!(
            app.photo_at(rect.x - 2.0, rect.y - 2.0),
            None,
            "the padding around a thumbnail is not the thumbnail"
        );
        assert!(!app.handle_event(&click(rect.x - 2.0, rect.y - 2.0)));
    }

    #[test]
    fn a_wider_window_fits_more_columns() {
        let mut app = library(50);
        app.set_window_size(800.0, 900.0);
        let narrow = app.grid_columns();
        app.set_window_size(1800.0, 900.0);
        assert!(
            app.grid_columns() > narrow,
            "a window a thousand pixels wider must fit more than {narrow} \
             columns"
        );
    }

    #[test]
    fn opening_the_info_panel_narrows_the_grid() {
        let mut app = library(50);
        app.show_info_panel = false;
        let wide = app.content_rect().w;
        app.show_info_panel = true;
        assert_eq!(app.content_rect().w, wide - INFO_PANEL_WIDTH);
    }

    // --- the keyboard ---

    #[test]
    fn the_arrows_move_the_selection_and_stop_at_the_ends() {
        let mut app = library(10);
        let visible = app.visible_photos();
        assert!(app.handle_event(&key(Key::Right)));
        assert_eq!(app.selected_photo, Some(visible[0]));
        app.handle_event(&key(Key::Right));
        assert_eq!(app.selected_photo, Some(visible[1]));
        assert!(app.handle_event(&key(Key::Left)));
        assert_eq!(app.selected_photo, Some(visible[0]));
        assert!(
            !app.handle_event(&key(Key::Left)),
            "Left at the first photo must stop, not wrap to the last"
        );
        app.handle_event(&key(Key::End));
        assert_eq!(app.selected_photo, visible.last().copied());
        assert!(!app.handle_event(&key(Key::Right)));
    }

    #[test]
    fn down_moves_a_whole_row() {
        let mut app = library(60);
        let cols = app.grid_columns();
        let visible = app.visible_photos();
        app.handle_event(&key(Key::Right));
        app.handle_event(&key(Key::Down));
        assert_eq!(app.selected_photo, Some(visible[cols]));
        app.handle_event(&key(Key::Up));
        assert_eq!(app.selected_photo, Some(visible[0]));
    }

    #[test]
    fn moving_the_selection_scrolls_it_into_view() {
        let mut app = library(400);
        for _ in 0..40 {
            app.handle_event(&key(Key::Down));
        }
        let index = app.selected_index().expect("a selection");
        let row = index / app.grid_columns();
        let window = app.grid_window();
        assert!(
            row >= window.start && row < window.end(),
            "row {row} is selected but the grid is showing {}..{}",
            window.start,
            window.end()
        );
    }

    #[test]
    fn moving_back_up_scrolls_the_grid_back_with_it() {
        let mut app = library(400);
        for _ in 0..40 {
            app.handle_event(&key(Key::Down));
        }
        assert!(app.grid_scroll > 0);
        for _ in 0..40 {
            app.handle_event(&key(Key::Up));
        }
        let index = app.selected_index().expect("a selection");
        let row = index / app.grid_columns();
        let window = app.grid_window();
        assert!(row >= window.start && row < window.end());
    }

    #[test]
    fn enter_opens_a_photo_and_escape_comes_back() {
        let mut app = library(5);
        app.handle_event(&key(Key::Right));
        assert!(app.handle_event(&key(Key::Enter)));
        assert_eq!(app.view_mode, ViewMode::Single);
        assert!(app.handle_event(&key(Key::Escape)));
        assert_eq!(app.view_mode, ViewMode::Grid);
        assert!(!app.handle_event(&key(Key::Escape)));
    }

    #[test]
    fn a_digit_rates_the_selected_photo() {
        let mut app = library(5);
        app.handle_event(&key(Key::Right));
        let pid = app.selected_photo.expect("a selection");
        assert!(app.handle_event(&typed(Key::Num4, '4')));
        assert_eq!(app.find_photo(pid).map(|p| p.rating), Some(4));
        assert!(app.handle_event(&typed(Key::Num0, '0')));
        assert_eq!(app.find_photo(pid).map(|p| p.rating), Some(0));
    }

    #[test]
    fn f_flags_the_selected_photo_and_i_toggles_the_info_panel() {
        let mut app = library(5);
        app.handle_event(&key(Key::Right));
        let pid = app.selected_photo.expect("a selection");
        assert!(app.handle_event(&typed(Key::F, 'f')));
        assert_eq!(app.find_photo(pid).map(|p| p.flagged), Some(true));

        let before = app.show_info_panel;
        assert!(app.handle_event(&typed(Key::I, 'i')));
        assert_ne!(app.show_info_panel, before);
    }

    #[test]
    fn delete_trashes_the_selection_and_lands_on_its_neighbour() {
        let mut app = library(5);
        let visible = app.visible_photos();
        app.selected_photo = Some(visible[2]);
        assert!(app.handle_event(&key(Key::Delete)));
        assert_eq!(app.trash.len(), 1);
        assert_eq!(
            app.selected_photo,
            Some(visible[3]),
            "the photo that moved up into the gap is the one a user expects \
             to be on after deleting one of a run"
        );
    }

    #[test]
    fn delete_at_the_end_of_the_library_lands_on_the_new_last_photo() {
        let mut app = library(3);
        let visible = app.visible_photos();
        app.selected_photo = visible.last().copied();
        app.handle_event(&key(Key::Delete));
        assert_eq!(app.selected_photo, Some(visible[1]));
    }

    #[test]
    fn an_unbound_key_asks_for_no_frame() {
        let mut app = library(5);
        assert_eq!(app.on_event(&key(Key::F9)), Response::Idle);
        assert_eq!(app.on_event(&typed(Key::Z, 'z')), Response::Idle);
    }

    // --- the slideshow keeps time ---

    #[test]
    fn a_slideshow_advances_on_its_own() {
        let mut app = library(5);
        app.start_slideshow();
        let interval = app.slideshow.as_ref().expect("a slideshow").interval_ms;
        assert_eq!(app.slideshow.as_ref().map(|ss| ss.current_index), Some(0));
        app.advance_slideshow(interval / 2);
        assert_eq!(
            app.slideshow.as_ref().map(|ss| ss.current_index),
            Some(0),
            "it has not been long enough yet"
        );
        app.advance_slideshow(interval);
        assert_eq!(
            app.slideshow.as_ref().map(|ss| ss.current_index),
            Some(1),
            "interval_ms and elapsed_ms had no reader at all: the slides only \
             moved when the user pressed Next, which is not a slideshow"
        );
    }

    #[test]
    fn a_paused_slideshow_does_not_advance() {
        let mut app = library(5);
        app.start_slideshow();
        let interval = app.slideshow.as_ref().expect("a slideshow").interval_ms;
        assert!(app.handle_event(&key(Key::Space)), "space pauses it");
        assert!(app.slideshow.as_ref().is_some_and(|ss| ss.paused));
        assert!(!app.advance_slideshow(interval * 4));
        assert_eq!(app.slideshow.as_ref().map(|ss| ss.current_index), Some(0));
        assert_eq!(app.tick_interval(), None, "and it asks for no frames");
    }

    #[test]
    fn stepping_a_slide_by_hand_gives_it_its_full_time() {
        let mut app = library(5);
        app.start_slideshow();
        let interval = app.slideshow.as_ref().expect("a slideshow").interval_ms;
        app.advance_slideshow(interval - 1);
        app.handle_event(&key(Key::Right));
        assert_eq!(app.slideshow.as_ref().map(|ss| ss.current_index), Some(1));
        app.advance_slideshow(2);
        assert_eq!(
            app.slideshow.as_ref().map(|ss| ss.current_index),
            Some(1),
            "a slide stepped to by hand must not vanish a millisecond later"
        );
    }

    #[test]
    fn escape_leaves_the_slideshow() {
        let mut app = library(5);
        app.start_slideshow();
        assert!(app.handle_event(&key(Key::Escape)));
        assert!(app.slideshow.is_none());
        assert_eq!(app.view_mode, ViewMode::Grid);
    }

    #[test]
    fn the_slideshow_wraps_round_at_the_end() {
        let mut app = library(3);
        app.start_slideshow();
        let interval = app.slideshow.as_ref().expect("a slideshow").interval_ms;
        for _ in 0..3 {
            app.advance_slideshow(interval);
        }
        assert_eq!(
            app.slideshow.as_ref().map(|ss| ss.current_index),
            Some(0),
            "a slideshow that stops at the last photo is a slide, not a show"
        );
    }

    // --- the strap ---

    #[test]
    fn the_title_says_where_you_are_and_how_much_is_there() {
        let mut app = library(7);
        assert_eq!(app.title(), "All Photos (7) - Photos");
        let album = app.create_album("Trip");
        app.select_sidebar(SidebarItem::Album(album));
        assert_eq!(app.title(), "Trip (0) - Photos");
        app.select_sidebar(SidebarItem::Trash);
        assert_eq!(app.title(), "Trash (0) - Photos");
    }

    #[test]
    fn the_clock_runs_only_while_a_slideshow_does() {
        let mut app = library(4);
        assert_eq!(app.tick_interval(), None);
        app.start_slideshow();
        assert_eq!(app.tick_interval(), Some(SLIDESHOW_TICK));
        app.stop_slideshow();
        assert_eq!(app.tick_interval(), None);
    }

    #[test]
    fn a_tick_with_nothing_to_do_asks_for_no_frame() {
        let mut app = library(4);
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 100 }),
            Response::Idle
        );
    }

    #[test]
    fn a_tick_that_turns_a_slide_asks_for_one() {
        let mut app = library(4);
        app.start_slideshow();
        let interval = app.slideshow.as_ref().expect("a slideshow").interval_ms;
        assert_eq!(
            app.on_event(&Event::Tick {
                elapsed_ms: interval
            }),
            Response::Redraw
        );
    }

    #[test]
    fn a_resize_relays_out_and_a_repeat_of_it_does_not() {
        let mut app = library(4);
        let resize = Event::Resize {
            width: 1000,
            height: 700,
        };
        assert_eq!(app.on_event(&resize), Response::Redraw);
        assert_eq!(app.window_width, 1000.0);
        assert_eq!(app.on_event(&resize), Response::Idle);
    }

    #[test]
    fn a_window_dragged_tiny_keeps_a_grid() {
        let mut app = library(4);
        app.set_window_size(1.0, 1.0);
        assert!(app.window_width >= MIN_WINDOW_WIDTH);
        assert!(app.window_height >= MIN_WINDOW_HEIGHT);
        assert!(app.grid_columns() >= 1);
        assert!(app.content_rect().w >= 1.0);
    }

    #[test]
    fn the_first_frame_uses_the_size_the_compositor_gave() {
        let mut app = library(4);
        let tree = app.render(1000.0, 800.0);
        assert_eq!(app.window_width, 1000.0);
        assert_eq!(app.window_height, 800.0);
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn the_close_button_exits() {
        let mut app = library(4);
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    /// A library with something in it, for the tests that need one.
    ///
    /// This was production code until 2026-09-15 and `main` called it, so the
    /// window opened on albums and photographs that were not on the machine.
    /// It is a perfectly good *fixture*; what was wrong was where it lived.
    fn seeded_library() -> PhotoApp {
        let mut app = PhotoApp::new();
        let _album = app.create_album("Vacation 2025");
        app.create_album("Family");

        let p1 = app.import_photo_with_exif(
            "/photos/IMG_0001.jpg",
            "IMG_0001.jpg",
            ImageFormat::Jpeg,
            5_242_880,
            ExifData::sample(),
        );
        let p2 = app.import_photo(
            "/photos/IMG_0002.png",
            "IMG_0002.png",
            ImageFormat::Png,
            3_145_728,
        );
        let _p3 = app.import_photo(
            "/photos/sunset.raw",
            "sunset.raw",
            ImageFormat::Raw,
            25_165_824,
        );

        app.rate_photo(p1, 5);
        app.rate_photo(p2, 3);
        app.add_tag(p1, "vacation");
        app.add_tag(p1, "beach");
        app.toggle_flag(p1);

        let smart_id = app.create_smart_album("Best Photos", true);
        app.add_smart_rule(smart_id, SmartRule::MinRating(4));
        app
    }

    /// Every EXIF field the parser fills reaches the panel.
    ///
    /// Seven of them did not, and were found by
    /// `scripts/check-fields-written-never-read.py`: focal length, whether the
    /// flash fired, GPS altitude, the writing software, the copyright, the
    /// exposure program and the exposure bias. The parser decoded all seven
    /// out of the file and `collect_exif_entries` listed neither, so the work
    /// was done and thrown away -- the same shape as an option a program
    /// parses and ignores.
    ///
    /// Written against the labels rather than the field names because the
    /// label is what a person actually sees, and a field renamed without its
    /// row being added back would otherwise pass.
    #[test]
    fn every_parsed_exif_field_reaches_the_panel() {
        let app = PhotoApp::new();
        let mut exif = ExifData::sample();
        exif.copyright = Some("(c) nobody".to_owned());
        exif.exposure_program = Some("Aperture priority".to_owned());
        exif.exposure_bias = Some(-0.7);
        exif.metering_mode = Some("Matrix".to_owned());
        exif.white_balance = Some("Auto".to_owned());
        exif.color_space = Some("sRGB".to_owned());

        let mut photo = Photo::new(1, "/photos/x.jpg", "x.jpg", ImageFormat::Jpeg, 1, 0);
        photo.exif = exif;
        let labels: Vec<&str> = app
            .collect_exif_entries(&photo)
            .into_iter()
            .map(|(label, _)| label)
            .collect();

        for expected in [
            "Camera",
            "Model",
            "Lens",
            "Focal Length",
            "Exposure",
            "Program",
            "Exp. Bias",
            "Flash",
            "Date",
            "GPS",
            "Altitude",
            "Color Space",
            "White Bal.",
            "Metering",
            "Software",
            "Copyright",
        ] {
            assert!(
                labels.contains(&expected),
                "the panel drops {expected:?}; it shows {labels:?}"
            );
        }
    }

    /// The exposure bias keeps its sign.
    ///
    /// "0.7 EV" and "-0.7 EV" are a stop and a half apart in what they mean
    /// and one character apart on the panel, so the plus is not decoration.
    #[test]
    fn a_positive_exposure_bias_is_shown_as_positive() {
        let app = PhotoApp::new();
        let mut photo = Photo::new(1, "/photos/x.jpg", "x.jpg", ImageFormat::Jpeg, 1, 0);
        photo.exif.exposure_bias = Some(0.7);
        let entries = app.collect_exif_entries(&photo);
        let bias = entries
            .iter()
            .find(|(label, _)| *label == "Exp. Bias")
            .expect("no bias row");
        assert_eq!(bias.1, "+0.7 EV");
    }

    /// The Import button is drawn where the hit test says it is.
    ///
    /// Both come from `toolbar_controls`, so this is not re-deriving the
    /// arithmetic -- it checks that the renderer actually used it. A control
    /// the pointer reaches and the eye cannot find is the same bug as the
    /// reverse, and both come from two predicates for one position.
    #[test]
    fn the_import_button_is_drawn_where_it_can_be_clicked() {
        let app = PhotoApp::new();
        let rect = app
            .toolbar_controls()
            .into_iter()
            .find(|(c, _)| *c == ToolbarControl::Import)
            .expect("no Import control")
            .1;
        assert_eq!(
            app.toolbar_control_at(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
            Some(ToolbarControl::Import),
            "the middle of the Import rectangle does not hit Import"
        );
        let drawn = app.render_commands(1000.0, 700.0).into_iter().any(|c| {
            matches!(c, RenderCommand::Text { ref text, x, .. }
                if text == "Import" && (x - (rect.x + 8.0)).abs() < 0.01)
        });
        assert!(drawn, "Import is clickable and never painted");
    }

    /// An app holding two real photos, which a slideshow needs.
    ///
    /// `PhotoApp::new()` opens empty -- this app invents nothing -- so
    /// `start_slideshow` is a no-op without them. The first version of the
    /// test below used `new()` and its own control caught that: "no slideshow
    /// to advance" rather than a green pass proving nothing.
    fn app_with_photos(tag: &str) -> PhotoApp {
        let mut app = PhotoApp::new();
        let dir = std::env::temp_dir().join("slateos-photomanager-slideshow");
        std::fs::create_dir_all(&dir).expect("temp dir");
        for i in 0..2 {
            let path = dir.join(format!("{tag}-{i}.png"));
            std::fs::write(&path, b"a real file, if not a real png").expect("write");
            app.import_from_disk(&path);
        }
        assert_eq!(app.photos.len(), 2, "the fixture did not import its photos");
        app
    }

    /// A photograph, its rating and its flag survive closing the window.
    ///
    /// The whole point. Before this, every import, rating and flag lasted
    /// exactly as long as the process.
    #[test]
    fn a_library_survives_a_restart() {
        let scratch = ScratchDir::new("photomanager-restart");
        let path = scratch.path("photolibrary.txt");
        let picture = scratch.path("holiday.png");
        std::fs::write(&picture, imagecodec::testing::png_gradient(4, 4)).expect("write");

        let pid = {
            let mut app = PhotoApp::with_storage(path.clone());
            app.import_from_disk(&picture);
            let pid = app.photos.first().expect("imported").id;
            assert!(app.rate_photo(pid, 5), "the control failed: not rated");
            assert!(app.toggle_flag(pid), "the control failed: not flagged");
            app.add_tag(pid, "pier");
            app.persist_if_changed();
            pid
        };

        let reopened = PhotoApp::with_storage(path);
        assert_eq!(reopened.photos.len(), 1, "the photograph did not come back");
        let photo = reopened.photos.first().expect("one");
        assert_eq!(photo.id, pid, "and it is the same one");
        assert_eq!(photo.rating, 5, "the rating did not survive");
        assert!(photo.flagged, "the flag did not survive");
        assert_eq!(
            photo.tags,
            vec!["pier".to_owned()],
            "the tag did not survive"
        );
        assert!(
            reopened.library_note.is_none(),
            "{:?}",
            reopened.library_note
        );
    }

    /// The save goes through `safeio`, not `fs::write`.
    ///
    /// The two leave identical bytes, so nothing else can tell them apart --
    /// and `fs::write` truncates before it writes, so an interrupted save
    /// would destroy the whole library rather than one photograph.
    #[test]
    fn the_save_is_atomic() {
        let scratch = ScratchDir::new("photomanager-atomic");
        let path = scratch.path("photolibrary.txt");
        let picture = scratch.path("p.png");
        std::fs::write(&picture, imagecodec::testing::png_gradient(4, 4)).expect("write");

        let mut app = PhotoApp::with_storage(path);
        app.import_from_disk(&picture);
        let before = safeio::writes_performed();
        app.persist_if_changed();
        assert!(
            safeio::writes_performed() > before,
            "the library must be written through safeio::write_atomically"
        );
    }

    /// Nothing is written when nothing changed.
    #[test]
    fn an_unchanged_library_is_not_rewritten() {
        let scratch = ScratchDir::new("photomanager-unchanged");
        let path = scratch.path("photolibrary.txt");
        let picture = scratch.path("p.png");
        std::fs::write(&picture, imagecodec::testing::png_gradient(4, 4)).expect("write");

        let mut app = PhotoApp::with_storage(path);
        app.import_from_disk(&picture);
        app.persist_if_changed();

        let before = safeio::writes_performed();
        app.persist_if_changed();
        app.persist_if_changed();
        assert_eq!(
            safeio::writes_performed(),
            before,
            "an unchanged library was written again"
        );
    }

    /// A library file that could not be read in full is never overwritten.
    ///
    /// The dangerous case, and the reason `library_unread` exists. One corrupt
    /// line still loads every other photograph -- and saving that back would
    /// delete the corrupt one for good, turning something a person could still
    /// repair in a text editor into nothing at all.
    #[test]
    fn a_library_that_did_not_fully_load_is_not_overwritten() {
        let scratch = ScratchDir::new("photomanager-unread");
        let path = scratch.path("photolibrary.txt");
        // The second record is truncated: too few fields to be a photograph.
        let original = "PHOTOLIBRARY|1\n\
             PHOTO|1|/a.jpg|a.jpg|10|jpeg|100||3|none|0|0|\n\
             PHOTO|2|/b.jpg\n";
        std::fs::write(&path, original).expect("write");

        let mut app = PhotoApp::with_storage(path.clone());
        assert_eq!(app.photos.len(), 1, "the good record still loaded");
        assert_eq!(
            app.library_unread, 1,
            "the control failed: nothing was skipped"
        );
        // Not `library_note.is_some()`: that asserts a field was written,
        // which is what let this ship with nothing displaying it. Assert the
        // words reach the screen.
        let tree = app.render(900.0, 700.0);
        let told = tree.commands.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("could not be read")),
        );
        assert!(told, "the reason was recorded but never put on screen");

        // Change something, then try to save.
        assert!(app.rate_photo(1, 5));
        app.persist_if_changed();

        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            original,
            "a file that could not be read in full was overwritten"
        );
    }

    /// A first run has no library file, and that is not an error.
    #[test]
    fn a_missing_library_is_an_ordinary_first_run() {
        let scratch = ScratchDir::new("photomanager-firstrun");
        let path = scratch.path("nothing-here.txt");

        let app = PhotoApp::with_storage(path);
        assert!(app.photos.is_empty());
        assert_eq!(app.library_unread, 0);
        assert!(
            app.library_note.is_none(),
            "a first run was reported as a problem: {:?}",
            app.library_note
        );
    }

    /// Ids carry on from where the saved library left off.
    ///
    /// Without this the generator would restart at 1 and the next import
    /// would be given an id a saved photograph already has -- so a rating
    /// typed on one would land on the other.
    #[test]
    fn ids_do_not_restart_over_a_saved_library() {
        let scratch = ScratchDir::new("photomanager-ids");
        let path = scratch.path("photolibrary.txt");
        let one = scratch.path("one.png");
        let two = scratch.path("two.png");
        std::fs::write(&one, imagecodec::testing::png_gradient(4, 4)).expect("write");
        std::fs::write(&two, imagecodec::testing::png_gradient(5, 4)).expect("write");

        let first_id = {
            let mut app = PhotoApp::with_storage(path.clone());
            app.import_from_disk(&one);
            app.persist_if_changed();
            app.photos.first().expect("one").id
        };

        let mut reopened = PhotoApp::with_storage(path);
        reopened.import_from_disk(&two);
        let ids: Vec<PhotoId> = reopened.photos.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), 2, "both photographs are present");
        assert_ne!(
            ids.first(),
            ids.get(1),
            "the new import reused a saved photograph's id: {ids:?} (first was {first_id})"
        );
    }

    /// A fixture whose files are real pictures, not merely real files.
    ///
    /// `app_with_photos` above writes "a real file, if not a real png", which
    /// was sufficient while nothing ever opened one. It is exactly what these
    /// tests must not use: every one of them would pass on the placeholder.
    fn app_with_pictures(tag: &str) -> PhotoApp {
        let mut app = PhotoApp::new();
        let dir = std::env::temp_dir().join("slateos-photomanager-pictures");
        std::fs::create_dir_all(&dir).expect("temp dir");
        for i in 0..2 {
            let path = dir.join(format!("{tag}-{i}.png"));
            std::fs::write(&path, imagecodec::testing::png_gradient(6, 4)).expect("write");
            app.import_from_disk(&path);
        }
        assert_eq!(app.photos.len(), 2, "the fixture did not import its photos");
        app.view_mode = ViewMode::Single;
        app
    }

    /// A library of `n` real pictures, for the grid's tests.
    fn app_with_n_pictures(tag: &str, n: usize) -> PhotoApp {
        let mut app = PhotoApp::new();
        let dir = std::env::temp_dir().join("slateos-photomanager-grid");
        std::fs::create_dir_all(&dir).expect("temp dir");
        for i in 0..n {
            let path = dir.join(format!("{tag}-{i}.png"));
            // Different sizes so the files differ, and so a thumbnail drawn
            // under the wrong id would be visibly the wrong picture.
            std::fs::write(
                &path,
                imagecodec::testing::png_gradient(
                    6u32.saturating_add(u32::try_from(i).unwrap_or(0)),
                    4,
                ),
            )
            .expect("write");
            app.import_from_disk(&path);
        }
        assert_eq!(app.photos.len(), n, "the fixture did not import its photos");
        app.view_mode = ViewMode::Grid;
        app
    }

    /// The grid draws the photographs, not cards with names on them.
    ///
    /// This is the increment. `sync_thumbnails` runs inside `render` and
    /// before the commands are built, so a thumbnail generated for this frame
    /// is drawable in this frame -- there is no state where a card has a
    /// picture that cannot yet be named.
    #[test]
    fn the_grid_draws_the_photographs_once_their_thumbnails_exist() {
        let mut app = app_with_n_pictures("drawn", 2);

        let tree = app.render(900.0, 700.0);

        let drawn = tree
            .commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::Image { .. }))
            .count();
        assert!(drawn >= 2, "the grid drew {drawn} photographs, expected 2");
        assert!(
            !app.take_images().is_empty(),
            "the pictures were drawn but never sent to the compositor"
        );
    }

    /// More photographs than fit in one frame's budget still all arrive.
    ///
    /// The budget is what keeps a scroll smooth; the thing to prove is that it
    /// bounds the work per frame without dropping any of it.
    #[test]
    fn a_screenful_fills_over_successive_frames() {
        let n = PhotoApp::THUMB_BATCH + 3;
        let mut app = app_with_n_pictures("fills", n);

        let after_one = {
            let _ = app.render(900.0, 700.0);
            let _ = app.take_images();
            app.thumb_ready.len()
        };
        assert_eq!(
            after_one,
            PhotoApp::THUMB_BATCH,
            "one frame did exactly its budget, no more and no less"
        );

        for _ in 0..3 {
            let _ = app.render(900.0, 700.0);
            let _ = app.take_images();
        }
        assert_eq!(app.thumb_ready.len(), n, "the rest never arrived");
    }

    /// Drops are announced before uploads, which the image budget requires.
    ///
    /// The compositor checks `held - freed + incoming`, so a batch that
    /// evicted as many thumbnails as it generated would be refused if it asked
    /// for both sets at once -- at exactly the moment the cache is doing its
    /// job. The cache here holds one, so importing two forces an eviction.
    #[test]
    fn an_eviction_is_announced_before_the_upload_that_caused_it() {
        let mut app = app_with_n_pictures("order", 2);
        app.thumb_cache = thumbs::ThumbnailCache::new(1);

        let _ = app.render(900.0, 700.0);
        let changes = app.take_images();

        let first_upload = changes
            .iter()
            .position(|c| matches!(c, oswindow::app::ImageChange::Upload { .. }))
            .expect("something was uploaded");
        let last_drop = changes
            .iter()
            .rposition(|c| matches!(c, oswindow::app::ImageChange::Drop(_)))
            .expect("the one-entry cache evicted something");
        assert!(
            last_drop < first_upload,
            "a drop was announced after an upload: {last_drop} vs {first_upload}"
        );
    }

    /// An evicted thumbnail stops being claimed as drawable.
    ///
    /// A card still naming a dropped id would draw nothing at all: the
    /// compositor discards an `Image` command for an id it does not hold, and
    /// says nothing about it. A placeholder is the honest fallback.
    #[test]
    fn an_evicted_thumbnail_is_no_longer_claimed_as_drawable() {
        let mut app = app_with_n_pictures("evicted", 2);
        app.thumb_cache = thumbs::ThumbnailCache::new(1);

        let _ = app.render(900.0, 700.0);
        let evicted = app
            .take_images()
            .iter()
            .filter(|c| matches!(c, oswindow::app::ImageChange::Drop(_)))
            .count();

        assert_eq!(evicted, 1, "the control failed: nothing was evicted");
        assert!(
            app.thumb_ready.len() < 2,
            "both photographs still claim a thumbnail after one was dropped"
        );
    }

    /// Selecting a photograph decodes it and queues its pixels.
    #[test]
    fn the_selected_photograph_is_decoded_and_queued_for_upload() {
        let mut app = app_with_pictures("upload");
        app.selected_photo = app.photos.first().map(|p| p.id);

        let _ = app.render(900.0, 700.0);

        let queued = app.take_images();
        assert_eq!(queued.len(), 1, "one photograph, one upload");
        match queued.first().expect("the upload") {
            oswindow::app::ImageChange::Upload {
                id, width, height, ..
            } => {
                assert_eq!(*id, PHOTO_IMAGE_ID);
                assert_eq!(
                    (*width, *height),
                    (6, 4),
                    "the picture's own size, read from the file"
                );
            }
            oswindow::app::ImageChange::Drop(id) => {
                panic!("expected an upload, got a drop of {id}")
            }
        }
    }

    /// The single-photo view draws the photograph instead of a card.
    ///
    /// This is the whole increment. Until it passed, every view in this
    /// application drew a rectangle with a file name in it over a file it had
    /// genuinely read.
    #[test]
    fn the_single_view_draws_the_picture_rather_than_a_card() {
        let mut app = app_with_pictures("drawn");
        app.selected_photo = app.photos.first().map(|p| p.id);

        let tree = app.render(900.0, 700.0);

        let drawn = tree.commands.iter().any(
            |c| matches!(c, RenderCommand::Image { image_id, .. } if *image_id == PHOTO_IMAGE_ID),
        );
        assert!(
            drawn,
            "a decoded photograph was still drawn as a placeholder"
        );
    }

    /// A file that is not a picture says why, rather than looking unloaded.
    #[test]
    fn a_file_that_is_not_a_picture_says_why_instead_of_staying_blank() {
        let mut app = PhotoApp::new();
        app.view_mode = ViewMode::Single;
        let dir = std::env::temp_dir().join("slateos-photomanager-pictures");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("not-really-a-picture.png");
        std::fs::write(&path, b"this file is named .png and is not one").expect("write");
        app.import_from_disk(&path);
        app.selected_photo = app.photos.first().map(|p| p.id);

        let tree = app.render(900.0, 700.0);

        assert!(app.picture_error.is_some(), "no reason was recorded");
        assert!(
            app.take_images().is_empty(),
            "nothing decodable, nothing uploaded"
        );
        let said = tree.commands.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("could not be decoded"))
        });
        assert!(said, "the reason was recorded but never put on screen");

        let _ = std::fs::remove_file(&path);
    }

    /// A photograph that cannot be decoded is attempted once, not every frame.
    ///
    /// Proved by deleting the file between the two frames: a retry would have
    /// to open it again, and would report that it was missing rather than that
    /// it was not a picture. Without `picture_for` this application would read
    /// and fail to decode the same file for as long as it stayed selected --
    /// sixty times a second.
    #[test]
    fn a_photograph_that_cannot_be_decoded_is_not_read_again_every_frame() {
        let mut app = PhotoApp::new();
        app.view_mode = ViewMode::Single;
        let dir = std::env::temp_dir().join("slateos-photomanager-pictures");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("attempted-once.png");
        std::fs::write(&path, b"not a picture either").expect("write");
        app.import_from_disk(&path);
        app.selected_photo = app.photos.first().map(|p| p.id);

        let _ = app.render(900.0, 700.0);
        let first = app.picture_error.clone();
        assert!(
            first.as_ref().is_some_and(|r| r.contains("decoded")),
            "the control failed: {first:?}"
        );

        std::fs::remove_file(&path).expect("remove");
        let _ = app.render(900.0, 700.0);

        assert_eq!(
            app.picture_error, first,
            "the file was opened a second time, so every frame re-reads it"
        );
    }

    /// A second selection replaces the upload rather than queueing behind it.
    ///
    /// The queue is deliberately not drained between the two frames, which is
    /// what a click arriving before the compositor has taken the last picture
    /// looks like. Both uploads carry the same id, so a queue holding both
    /// would send pixels that are already stale.
    #[test]
    fn a_new_selection_replaces_the_upload_rather_than_queueing_behind_it() {
        let mut app = app_with_pictures("replace");
        let ids: Vec<PhotoId> = app.photos.iter().map(|p| p.id).collect();

        app.selected_photo = ids.first().copied();
        let _ = app.render(900.0, 700.0);
        app.selected_photo = ids.get(1).copied();
        let _ = app.render(900.0, 700.0);

        assert_eq!(
            app.take_images().len(),
            1,
            "the overtaken upload was still in the queue"
        );
    }

    /// Fitting never enlarges a picture past its own size.
    #[test]
    fn fitting_a_small_picture_leaves_it_at_its_own_size() {
        assert_eq!(fit_within(10, 10, 900.0, 700.0), (10.0, 10.0));
    }

    /// A portrait photograph is bounded by the height, not the width.
    ///
    /// The case the discarded 4:3 assumption got wrong: it gave every
    /// photograph a landscape box regardless of which way the camera was held.
    #[test]
    fn a_portrait_picture_is_bounded_by_the_height() {
        let (w, h) = fit_within(2000, 4000, 900.0, 700.0);
        assert!((h - 700.0).abs() < 0.01, "height fills the pane: {h}");
        assert!((w - 350.0).abs() < 0.01, "width follows the shape: {w}");
    }

    /// A landscape photograph is bounded by the width.
    #[test]
    fn a_landscape_picture_is_bounded_by_the_width() {
        let (w, h) = fit_within(4000, 2000, 900.0, 700.0);
        assert!((w - 900.0).abs() < 0.01, "width fills the pane: {w}");
        assert!((h - 450.0).abs() < 0.01, "height follows the shape: {h}");
    }

    /// The slideshow keeps running while the picker is open.
    ///
    /// The helper this replaced returned `false` for anything that was not
    /// input, and `handle_event` returned on that -- so `Event::Tick` never
    /// reached `advance_slideshow`. **The slideshow stopped between one
    /// picture and the next** for as long as the import dialog was up, and
    /// `tick_interval` asks for ticks exactly when a slideshow is running, so
    /// this was live rather than latent.
    ///
    /// The test carries a control, because the obvious version of it passes
    /// whether or not the intercept is there: if the slideshow were not
    /// actually advancing in this fixture, "it did not advance" would hold for
    /// the wrong reason.
    #[test]
    fn the_slideshow_keeps_running_while_the_picker_is_open() {
        // The observable is which picture is showing, not the elapsed
        // counter: a tick past the interval resets that counter to zero, so
        // "elapsed_ms changed" is false at exactly the moment the slideshow
        // DID move on. The control caught that too.
        let tick = Event::Tick { elapsed_ms: 10_000 };

        // The control: with no picker up, a tick advances the slideshow.
        let mut control = app_with_photos("control");
        control.start_slideshow();
        assert!(
            control.slideshow.is_some(),
            "no slideshow to advance -- the control cannot prove anything"
        );
        let before = control.slideshow.as_ref().map(|s| s.current_index);
        control.handle_event(&tick);
        assert_ne!(
            control.slideshow.as_ref().map(|s| s.current_index),
            before,
            "the control is broken: a tick does not move the slideshow on here"
        );

        // The case: the same tick, with the import picker up.
        let mut app = app_with_photos("case");
        app.start_slideshow();
        let before = app.slideshow.as_ref().map(|s| s.current_index);
        app.press_toolbar(ToolbarControl::Import);
        assert!(app.picker.is_open(), "no picker came up");

        app.handle_event(&tick);
        assert_ne!(
            app.slideshow.as_ref().map(|s| s.current_index),
            before,
            "the slideshow stopped because a dialog was open"
        );
        assert!(app.picker.is_open(), "the tick closed the dialog");
    }

    /// Pressing Import puts a picker up, and it is drawn.
    #[test]
    fn pressing_import_opens_a_picker_that_is_actually_drawn() {
        let mut app = PhotoApp::new();
        let before = app.render_commands(1000.0, 700.0).len();
        app.press_toolbar(ToolbarControl::Import);
        assert!(app.picker.is_open(), "no picker came up");
        assert!(
            app.render_commands(1000.0, 700.0).len() > before,
            "the picker is open and nothing more is drawn"
        );
    }

    /// A file that cannot be read says so rather than doing nothing.
    ///
    /// The silent no-op is the failure this whole application was an instance
    /// of: a window over a library nothing ever filled. An import that changes
    /// no count and offers no reason is indistinguishable from one that was
    /// never attempted.
    #[test]
    fn an_unreadable_file_reports_instead_of_failing_quietly() {
        let mut app = PhotoApp::new();
        let missing = std::env::temp_dir().join("photomanager-no-such-file.jpg");
        let message = app.import_from_disk(&missing);
        assert!(app.photos.is_empty(), "a failed read imported something");
        assert!(
            message.contains("Could not read"),
            "the failure said {message:?}"
        );
    }

    /// A real file on disk becomes a photo, with its EXIF read from the bytes.
    ///
    /// `parse_exif_from_bytes` was written, tested and never given a file that
    /// existed -- the only EXIF this application ever held was
    /// `ExifData::sample`, a Canon EOS R5 that was not there.
    #[test]
    fn a_file_on_disk_becomes_a_photo() {
        let mut app = PhotoApp::new();
        let path = std::env::temp_dir().join("photomanager-import-test.png");
        std::fs::write(&path, b"not really a png, but it is really a file").expect("write");
        let message = app.import_from_disk(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(app.photos.len(), 1, "the import said {message:?}");
        let photo = app.photos.first().expect("no photo");
        assert_eq!(photo.file_name, "photomanager-import-test.png");
        assert_eq!(photo.format, ImageFormat::Png);
        assert!(photo.file_size > 0, "the size did not come from the file");
    }

    /// The window opens on an empty grid now, and says so.
    ///
    /// Replaces `the_seeded_library_opens_on_something_to_look_at`, which
    /// asserted the opposite and passed because `main` seeded a fake library.
    #[test]
    fn a_new_library_is_empty_and_the_grid_says_so() {
        let app = PhotoApp::new();
        assert!(app.photos.is_empty(), "a new library invented a photo");
        assert!(app.albums.is_empty(), "a new library invented an album");
        let text: Vec<String> = app
            .render_commands(1000.0, 700.0)
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            text.iter().any(|t| t.contains("No photos")),
            "an empty grid drew no explanation: {text:?}"
        );
    }

    /// The fixture still builds a library, which is what the tests below need.
    #[test]
    fn the_fixture_library_has_something_to_look_at() {
        let app = seeded_library();
        assert!(!app.photos.is_empty());
        assert!(!app.albums.is_empty());
        assert!(!app.smart_albums.is_empty());
    }

    #[test]
    fn a_click_on_a_sidebar_row_boundary_belongs_to_the_lower_row() {
        let app = library(2);
        let rows = app.sidebar_rows();
        let mut y = TOOLBAR_HEIGHT;
        let mut checked = 0;
        for (i, row) in rows.iter().enumerate() {
            let boundary = y + row.height();
            if let Some(SidebarRow::Item { target, .. }) = rows.get(i + 1) {
                assert_eq!(
                    app.sidebar_item_at(20.0, boundary),
                    Some(*target),
                    "the pixel a row ends on belongs to the next row, not to \
                     both"
                );
                checked += 1;
            }
            y = boundary;
        }
        assert!(checked > 0);
    }

    #[test]
    fn a_scrolled_grid_draws_its_first_row_at_the_top() {
        let mut app = library(400);
        let content = app.content_rect();
        let cols = app.grid_columns();
        app.grid_scroll = 5;
        let rect = app
            .thumb_rect(cols.saturating_mul(5))
            .expect("the first drawn row");
        assert!(
            (rect.y - (content.y + THUMB_PADDING)).abs() < 0.01,
            "a grid scrolled five rows must draw row five at the top of the \
             content area, not five rows below it: y={}",
            rect.y
        );
    }

    #[test]
    fn a_grid_too_narrow_for_one_thumbnail_still_has_a_column() {
        let mut app = library(20);
        app.set_window_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT);
        app.show_info_panel = true;
        while app.current_thumb_size() < 200.0 {
            app.cycle_thumb_size();
        }
        assert!(
            app.content_rect().w < app.current_thumb_size(),
            "the fixture must be narrower than one thumbnail"
        );
        assert!(
            app.grid_columns() >= 1,
            "zero columns divides by zero when the row count is worked out"
        );
        let _ = app.grid_window();
        let _ = app.thumb_rect(0);
    }

    #[test]
    fn the_content_area_starts_where_the_sidebar_ends() {
        let app = library(4);
        let content = app.content_rect();
        assert!(
            (content.x - SIDEBAR_WIDTH).abs() < f32::EPSILON,
            "content drawn from x={} would be underneath the sidebar",
            content.x
        );
        assert!((content.y - TOOLBAR_HEIGHT).abs() < f32::EPSILON);
    }

    #[test]
    fn opening_the_info_panel_re_anchors_the_grid() {
        let mut app = library(400);
        app.grid_scroll = 20;
        app.handle_event(&typed(Key::I, 'i'));
        assert_eq!(
            app.grid_scroll, 0,
            "the content just changed width, so the row a position named is \
             not the row it names now"
        );
    }

    #[test]
    fn a_slide_gets_its_full_time_after_an_automatic_turn() {
        let mut app = library(5);
        app.start_slideshow();
        let interval = app.slideshow.as_ref().expect("a slideshow").interval_ms;
        app.advance_slideshow(interval);
        assert_eq!(app.slideshow.as_ref().map(|ss| ss.current_index), Some(1));
        app.advance_slideshow(interval / 2);
        assert_eq!(
            app.slideshow.as_ref().map(|ss| ss.current_index),
            Some(1),
            "without resetting the clock every tick after the first would \
             turn another slide"
        );
    }

    #[test]
    fn releasing_the_mouse_is_not_a_click() {
        let mut app = library(20);
        let rect = app.thumb_rect(3).expect("a thumbnail");
        let release = Event::Mouse(MouseEvent {
            x: rect.x + 4.0,
            y: rect.y + 4.0,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        assert!(
            !app.handle_event(&release),
            "acting on both halves of a click runs every button twice"
        );
        assert_eq!(app.selected_photo, None);
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut PhotoApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = PhotoApp::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }
}
