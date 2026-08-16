//! Thumbnail generation and caching for the file explorer's icon view.
//!
//! Provides:
//! - **Image thumbnails** (BMP/PNG/JPEG/GIF): header parsing for dimensions,
//!   box-filter downscale to configurable size, stored as raw ARGB pixels.
//! - **Text file previews**: first ~20 lines rendered as a minimap-style preview.
//! - **Folder thumbnails**: item count with mini-icon grid of first 4 entries.
//! - **PDF placeholders**: red document icon with page count text.
//! - **Default icons by type**: music note, film frame, archive box, gear, etc.
//!
//! Caching uses an in-memory LRU keyed on `(path, mtime, size)` so a changed
//! file automatically invalidates.  An optional disk cache under
//! `~/.cache/thumbs/` persists thumbnails across sessions.
//!
//! Background generation is supported via a request queue that can be polled
//! for completed thumbnails, keeping the UI thread non-blocking.

#![allow(dead_code)]

use guitk::canvas::Canvas;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ============================================================================
// Constants
// ============================================================================

/// Default thumbnail size in pixels (width and height).
const DEFAULT_THUMB_SIZE: u32 = 128;

/// Default LRU cache capacity (number of thumbnails).
const DEFAULT_CACHE_CAPACITY: usize = 500;

/// Maximum number of text lines to read for a text preview thumbnail.
const TEXT_PREVIEW_MAX_LINES: usize = 20;

/// Maximum bytes to read when sniffing a text file for preview.
const TEXT_PREVIEW_MAX_BYTES: usize = 4096;

/// Number of child items to show in a folder thumbnail grid (2x2).
const FOLDER_PREVIEW_ITEMS: usize = 4;

/// Disk cache directory name under the user's cache root.
const DISK_CACHE_DIR: &str = ".cache/thumbs";

// ============================================================================
// Thumbnail
// ============================================================================

/// A generated thumbnail image.
#[derive(Clone, Debug)]
pub struct Thumbnail {
    /// Thumbnail width in pixels.
    pub width: u32,
    /// Thumbnail height in pixels.
    pub height: u32,
    /// Raw pixel data in ARGB format (4 bytes per pixel, row-major).
    pub pixels: Vec<u8>,
    /// Absolute path of the source file or directory.
    ///
    /// A `PathBuf`, not a `String`: this is the disk cache's key, hashed by
    /// [`simple_hash`]. Held as a lossy string, two files whose names differ
    /// only in bytes that are not UTF-8 collapsed to the same key and hashed
    /// to the same cache filename — so one file was shown the other's
    /// thumbnail.
    pub source_path: PathBuf,
    /// Modification time of the source (seconds since epoch) for invalidation.
    pub source_mtime: u64,
}

impl Thumbnail {
    /// Total number of pixels.
    fn pixel_count(&self) -> usize {
        (self.width as usize).saturating_mul(self.height as usize)
    }

    /// Returns `true` if the pixel buffer is consistent with the dimensions.
    ///
    /// Nothing inside this module needs to call it any more — [`into_thumbnail`]
    /// is the only constructor here and it cannot produce a `Thumbnail` for
    /// which this is false. It survives because `pixels`, `width` and `height`
    /// are public, so code outside the module can still assemble one by hand.
    fn is_valid(&self) -> bool {
        self.pixels.len() == self.pixel_count().saturating_mul(4)
    }
}

/// Build a `Thumbnail` from a finished canvas.
///
/// This is the only way a `Thumbnail` is made in this module, and it takes the
/// `Canvas` by value, so [`Thumbnail::is_valid`] is true of the result by
/// construction: the dimensions and the buffer come from one object that has
/// already proved they agree.
fn into_thumbnail(canvas: Canvas, source_path: &Path, source_mtime: u64) -> Thumbnail {
    Thumbnail {
        width: canvas.width(),
        height: canvas.height(),
        // ARGB, which is what `Thumbnail::pixels` documents and what the disk
        // cache writes. A `Canvas` has no byte order of its own; this call and
        // `Canvas::from_argb` in `DiskCache::load` are the only two places the
        // choice is made, instead of it being re-derived at every pixel write.
        pixels: canvas.to_argb(),
        source_path: source_path.to_path_buf(),
        source_mtime,
    }
}

// ============================================================================
// Cache key
// ============================================================================

/// Composite key for the thumbnail cache: path + mtime + file size.
///
/// If any component changes the old entry will not match, giving automatic
/// invalidation when a file is modified.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    path: String,
    mtime: u64,
    size: u64,
}

impl CacheKey {
    fn new(path: &str, mtime: u64, size: u64) -> Self {
        Self {
            path: path.to_owned(),
            mtime,
            size,
        }
    }
}

// ============================================================================
// LRU cache
// ============================================================================

/// In-memory LRU thumbnail cache.
///
/// Uses a `VecDeque` as a usage-ordered list of keys together with a `HashMap`
/// for O(1) lookup.  When the cache is full, the least-recently-used entry
/// (front of the deque) is evicted.
pub struct ThumbnailCache {
    /// Maximum number of thumbnails to keep.
    capacity: usize,
    /// Map from cache key to the stored thumbnail.
    map: HashMap<CacheKey, Thumbnail>,
    /// Usage order: most-recently-used at the back, LRU at the front.
    order: VecDeque<CacheKey>,
}

impl ThumbnailCache {
    /// Create a new cache with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        }
    }

    /// Create a cache with the default capacity (500).
    pub fn default_capacity() -> Self {
        Self::new(DEFAULT_CACHE_CAPACITY)
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Look up a thumbnail.  Returns `None` on miss.  On hit the entry is
    /// promoted to most-recently-used.
    pub fn get(&mut self, path: &str, mtime: u64, size: u64) -> Option<&Thumbnail> {
        let key = CacheKey::new(path, mtime, size);
        if self.map.contains_key(&key) {
            self.promote(&key);
            self.map.get(&key)
        } else {
            None
        }
    }

    /// Insert (or replace) a thumbnail.  Evicts the LRU entry when full.
    pub fn insert(&mut self, path: &str, mtime: u64, size: u64, thumb: Thumbnail) {
        let key = CacheKey::new(path, mtime, size);

        // If updating an existing entry, remove the old order position.
        if self.map.contains_key(&key) {
            self.remove_from_order(&key);
        } else if self.map.len() >= self.capacity {
            self.evict_lru();
        }

        self.map.insert(key.clone(), thumb);
        self.order.push_back(key);
    }

    /// Remove all entries whose path matches `path` (regardless of mtime/size).
    pub fn invalidate(&mut self, path: &str) {
        let keys_to_remove: Vec<CacheKey> = self
            .map
            .keys()
            .filter(|k| k.path == path)
            .cloned()
            .collect();

        for key in &keys_to_remove {
            self.map.remove(key);
            self.remove_from_order(key);
        }
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    // -- internal helpers ---------------------------------------------------

    /// Move `key` to the back (most-recently-used position).
    fn promote(&mut self, key: &CacheKey) {
        self.remove_from_order(key);
        self.order.push_back(key.clone());
    }

    /// Remove `key` from the usage-order deque.
    fn remove_from_order(&mut self, key: &CacheKey) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
    }

    /// Evict the least-recently-used entry (front of the deque).
    fn evict_lru(&mut self) {
        if let Some(lru_key) = self.order.pop_front() {
            self.map.remove(&lru_key);
        }
    }
}

// ============================================================================
// Image header parsing
// ============================================================================

/// Dimensions extracted from an image file header.
#[derive(Clone, Copy, Debug)]
struct ImageDimensions {
    width: u32,
    height: u32,
}

/// Read a little-endian u32 from a byte slice at `offset`.
/// Returns `None` if out of bounds.
fn read_le_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(read_array(data, offset)?))
}

/// Read a big-endian u32 from a byte slice at `offset`.
fn read_be_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(read_array(data, offset)?))
}

/// Read a big-endian u16 from a byte slice at `offset`.
fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(read_array(data, offset)?))
}

/// Read a little-endian u16 from a byte slice at `offset`.
fn read_le_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(read_array(data, offset)?))
}

/// Read `N` bytes at `offset`, or `None` if they are not all present.
///
/// `offset + N` is computed with `checked_add` because `offset` comes from an
/// image header: a header claiming an offset near `usize::MAX` would otherwise
/// wrap to a small number and read the wrong bytes rather than declining.
fn read_array<const N: usize>(data: &[u8], offset: usize) -> Option<[u8; N]> {
    let end = offset.checked_add(N)?;
    data.get(offset..end)?.try_into().ok()
}

/// Parse BMP header to extract dimensions.
///
/// BMP files start with `BM`, and the BITMAPINFOHEADER at offset 14 contains
/// width (LE i32 at +4) and height (LE i32 at +8, may be negative for
/// top-down bitmaps).
fn parse_bmp_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    if data.len() < 26 {
        return None;
    }
    if data.get(0..2)? != b"BM" {
        return None;
    }
    let width = read_le_u32(data, 18)? as i32;
    let height = (read_le_u32(data, 22)? as i32).abs();
    if width <= 0 || height == 0 {
        return None;
    }
    Some(ImageDimensions {
        width: width as u32,
        height: height as u32,
    })
}

/// Parse PNG header to extract dimensions.
///
/// PNG files start with the 8-byte magic `\x89PNG\r\n\x1A\n`, followed by the
/// IHDR chunk whose data starts at offset 16 (width BE u32, height BE u32).
fn parse_png_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    if data.len() < 24 {
        return None;
    }
    let magic: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.get(0..8)? != magic {
        return None;
    }
    let width = read_be_u32(data, 16)?;
    let height = read_be_u32(data, 20)?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(ImageDimensions { width, height })
}

/// Parse GIF header to extract dimensions.
///
/// GIF files start with `GIF87a` or `GIF89a`, and the logical screen
/// descriptor at offset 6 has width (LE u16) and height (LE u16).
fn parse_gif_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    if data.len() < 10 {
        return None;
    }
    let sig = data.get(0..6)?;
    if sig != b"GIF87a" && sig != b"GIF89a" {
        return None;
    }
    let width = read_le_u16(data, 6)? as u32;
    let height = read_le_u16(data, 8)? as u32;
    if width == 0 || height == 0 {
        return None;
    }
    Some(ImageDimensions { width, height })
}

/// Parse JPEG header to extract dimensions.
///
/// JPEG files start with `\xFF\xD8`.  We scan for a SOF0 (0xFFC0) or
/// SOF2 (0xFFC2) marker whose payload contains height (BE u16 at +3) and
/// width (BE u16 at +5) relative to the marker payload start.
fn parse_jpeg_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    if data.len() < 4 {
        return None;
    }
    if data.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }

    let mut pos = 2;
    while pos + 1 < data.len() {
        if *data.get(pos)? != 0xFF {
            pos += 1;
            continue;
        }
        let marker = *data.get(pos + 1)?;
        pos += 2;

        // Skip padding 0xFF bytes.
        if marker == 0xFF || marker == 0x00 {
            continue;
        }
        // Restart markers and standalone markers have no payload.
        if (0xD0..=0xD9).contains(&marker) {
            continue;
        }

        if pos + 2 > data.len() {
            return None;
        }
        let seg_len = read_be_u16(data, pos)? as usize;
        if seg_len < 2 {
            return None;
        }

        // SOF0 (baseline), SOF1 (extended sequential), SOF2 (progressive)
        if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
            if pos + 7 > data.len() {
                return None;
            }
            let height = read_be_u16(data, pos + 3)? as u32;
            let width = read_be_u16(data, pos + 5)? as u32;
            if width == 0 || height == 0 {
                return None;
            }
            return Some(ImageDimensions { width, height });
        }

        pos += seg_len;
    }
    None
}

/// Try to parse image dimensions from raw file header bytes.
///
/// Tries each format in order (BMP, PNG, GIF, JPEG) and returns the first
/// successful parse.
fn parse_image_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    parse_bmp_dimensions(data)
        .or_else(|| parse_png_dimensions(data))
        .or_else(|| parse_gif_dimensions(data))
        .or_else(|| parse_jpeg_dimensions(data))
}

// ============================================================================
// Image downscaling
// ============================================================================

/// Downscale a canvas to fit within `target_size x target_size`, preserving
/// aspect ratio. A source already that small is returned unscaled.
fn box_filter_downscale(src: &Canvas, target_size: u32) -> Canvas {
    let (dst_w, dst_h) = fit_dimensions(src.width(), src.height(), target_size);
    src.box_downscale(dst_w, dst_h)
}

/// Compute output dimensions that fit within `max_size` while preserving
/// the aspect ratio of `w x h`.
fn fit_dimensions(w: u32, h: u32, max_size: u32) -> (u32, u32) {
    if w == 0 || h == 0 || max_size == 0 {
        return (0, 0);
    }
    if w <= max_size && h <= max_size {
        return (w, h);
    }
    if w >= h {
        let new_h = (h as u64 * max_size as u64 / w as u64).max(1) as u32;
        (max_size, new_h)
    } else {
        let new_w = (w as u64 * max_size as u64 / h as u64).max(1) as u32;
        (new_w, max_size)
    }
}

// ============================================================================
// Thumbnail generation
// ============================================================================

/// Configuration for thumbnail generation.
#[derive(Clone, Debug)]
pub struct ThumbConfig {
    /// Thumbnail pixel size (both width and height cap).
    pub size: u32,
    /// Background color for text previews and placeholders.
    pub bg_color: Color,
    /// Text color for previews and labels.
    pub text_color: Color,
}

impl Default for ThumbConfig {
    fn default() -> Self {
        Self {
            size: DEFAULT_THUMB_SIZE,
            bg_color: Color::rgb(245, 245, 245),
            text_color: Color::rgb(100, 100, 100),
        }
    }
}

/// Category used to select the default placeholder icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbCategory {
    Image,
    Text,
    Folder,
    Pdf,
    Audio,
    Video,
    Archive,
    Executable,
    Unknown,
}

impl ThumbCategory {
    /// Determine the thumbnail category from a file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "bmp" | "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => Self::Image,
            "txt" | "log" | "md" | "rst" | "rs" | "py" | "c" | "h" | "cpp" | "js" | "ts"
            | "html" | "css" | "java" | "go" | "toml" | "yaml" | "json" | "xml" | "sh" | "cfg"
            | "ini" | "conf" => Self::Text,
            "pdf" => Self::Pdf,
            "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" => Self::Audio,
            "mp4" | "avi" | "mkv" | "webm" | "mov" | "flv" => Self::Video,
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => Self::Archive,
            "exe" | "bin" | "cmd" | "bat" => Self::Executable,
            _ => Self::Unknown,
        }
    }

    /// Single-character icon representation for this category (used in
    /// placeholder thumbnails rendered as text).
    fn icon_label(self) -> &'static str {
        match self {
            Self::Image => "\u{1F5BC}",  // framed picture
            Self::Text => "\u{1F4C4}",   // page
            Self::Folder => "\u{1F4C1}", // folder
            Self::Pdf => "PDF",
            Self::Audio => "\u{1F3B5}",     // musical note
            Self::Video => "\u{1F3AC}",     // clapper board
            Self::Archive => "\u{1F4E6}",   // package
            Self::Executable => "\u{2699}", // gear
            Self::Unknown => "\u{1F4C3}",   // page with curl
        }
    }

    /// Accent color for the placeholder icon background.
    fn accent_color(self) -> Color {
        match self {
            Self::Image => Color::rgb(76, 175, 80),       // green
            Self::Text => Color::rgb(158, 158, 158),      // gray
            Self::Folder => Color::rgb(255, 193, 7),      // amber
            Self::Pdf => Color::rgb(211, 47, 47),         // red
            Self::Audio => Color::rgb(156, 39, 176),      // purple
            Self::Video => Color::rgb(33, 150, 243),      // blue
            Self::Archive => Color::rgb(121, 85, 72),     // brown
            Self::Executable => Color::rgb(96, 125, 139), // blue-gray
            Self::Unknown => Color::rgb(189, 189, 189),   // light gray
        }
    }
}

/// Generate a thumbnail for the file at `path`.
///
/// This reads file headers / first lines as needed and returns a `Thumbnail`
/// suitable for the cache.  For unsupported or unreadable files a
/// category-appropriate placeholder is returned.
pub fn generate_thumbnail(path: &Path, config: &ThumbConfig) -> Thumbnail {
    let mtime = file_mtime(path).unwrap_or(0);

    if path.is_dir() {
        return generate_folder_thumbnail(path, config, mtime);
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let category = ThumbCategory::from_extension(&ext);

    match category {
        ThumbCategory::Image => generate_image_thumbnail(path, config, mtime),
        ThumbCategory::Text => generate_text_thumbnail(path, config, mtime),
        ThumbCategory::Pdf => generate_pdf_placeholder(path, config, mtime),
        _ => generate_default_thumbnail(path, category, config, mtime),
    }
}

/// Generate a thumbnail from an image file (BMP/PNG/GIF/JPEG).
///
/// Reads enough of the file header to determine dimensions, then generates a
/// filled rectangle of the image's accent color scaled to the thumbnail size.
/// Full decode + downscale is used when the raw pixel data is available (BMP);
/// for compressed formats (PNG/JPEG/GIF) we produce a placeholder with the
/// correct aspect ratio since we lack a full decoder in this crate.
fn generate_image_thumbnail(path: &Path, config: &ThumbConfig, mtime: u64) -> Thumbnail {
    let header = match read_file_header(path, 1024) {
        Some(h) => h,
        None => return generate_default_thumbnail(path, ThumbCategory::Image, config, mtime),
    };

    let dims = match parse_image_dimensions(&header) {
        Some(d) => d,
        None => return generate_default_thumbnail(path, ThumbCategory::Image, config, mtime),
    };

    // For BMP we can attempt to read raw pixel data (uncompressed 32-bit).
    if header.starts_with(b"BM")
        && let Some(thumb) = try_bmp_thumbnail(path, dims, config, mtime)
    {
        return thumb;
    }

    // For other formats: create an aspect-ratio-correct color swatch since we
    // don't have a full decoder.  The swatch color is derived from the format.
    let (tw, th) = fit_dimensions(dims.width, dims.height, config.size);
    let size = config.size;
    let mut canvas = Canvas::transparent(size, size);

    // Centre the swatch within the thumbnail area. `fit_dimensions` caps both
    // returned dimensions at `size`, so these subtractions do not underflow —
    // but they are written saturating rather than relying on a proof that
    // lives in another function.
    let off_x = size.saturating_sub(tw) / 2;
    let off_y = size.saturating_sub(th) / 2;
    canvas.fill_rect(off_x, off_y, tw, th, ThumbCategory::Image.accent_color());

    into_thumbnail(canvas, path, mtime)
}

/// Attempt to create a real thumbnail from an uncompressed 32-bit BMP.
fn try_bmp_thumbnail(
    path: &Path,
    dims: ImageDimensions,
    config: &ThumbConfig,
    mtime: u64,
) -> Option<Thumbnail> {
    let data = fs::read(path).ok()?;
    if data.len() < 54 {
        return None;
    }

    let offset = read_le_u32(&data, 10)? as usize;
    let bits_per_pixel = read_le_u16(&data, 28)?;
    let compression = read_le_u32(&data, 30)?;

    // Only handle uncompressed 24-bit or 32-bit BMPs.
    if compression != 0 || (bits_per_pixel != 24 && bits_per_pixel != 32) {
        return None;
    }

    // Every size below is derived from the file's own header, so all of this
    // arithmetic is on attacker-chosen numbers: computed with `checked_*`, a
    // header that overflows a `usize` declines the thumbnail. Computed with
    // `*`, it wraps to a small number that then passes the length check below
    // while describing a buffer that is not there.
    let bpp = bits_per_pixel as usize / 8;
    let row_size = (dims.width as usize)
        .checked_mul(bpp)?
        .div_ceil(4)
        .checked_mul(4)?; // rows padded to 4 bytes
    let expected_data = row_size
        .checked_mul(dims.height as usize)?
        .checked_add(offset)?;
    if data.len() < expected_data {
        return None;
    }

    // BMP stores rows bottom-up by default (positive height).  Convert to
    // top-down ARGB.
    let height_raw = read_le_u32(&data, 22)? as i32;
    let bottom_up = height_raw > 0;

    let mut canvas = Canvas::transparent(dims.width, dims.height);
    for y in 0..dims.height {
        let src_y = if bottom_up {
            dims.height.saturating_sub(1).saturating_sub(y)
        } else {
            y
        };
        let row_start = (src_y as usize)
            .checked_mul(row_size)?
            .checked_add(offset)?;
        for x in 0..dims.width {
            let src_idx = (x as usize).checked_mul(bpp)?.checked_add(row_start)?;
            // BMP pixel order is BGR(A).
            let px = data.get(src_idx..src_idx.checked_add(bpp)?)?;
            let (&b_val, &g_val, &r_val) = match px {
                [b, g, r, ..] => (b, g, r),
                _ => return None,
            };
            let a_val = if bpp == 4 { *px.get(3)? } else { u8::MAX };
            canvas.set(x, y, Color::rgba(r_val, g_val, b_val, a_val));
        }
    }

    Some(into_thumbnail(
        box_filter_downscale(&canvas, config.size),
        path,
        mtime,
    ))
}

/// Generate a text-preview thumbnail for source/text files.
///
/// Reads the first `TEXT_PREVIEW_MAX_LINES` lines and fills a pixel buffer
/// with tiny gray "text lines" on a light background — a minimap effect.
fn generate_text_thumbnail(path: &Path, config: &ThumbConfig, mtime: u64) -> Thumbnail {
    let lines = match read_text_lines(path, TEXT_PREVIEW_MAX_LINES) {
        Some(l) if !l.is_empty() => l,
        _ => return generate_default_thumbnail(path, ThumbCategory::Text, config, mtime),
    };

    let size = config.size;
    let mut canvas = Canvas::filled(size, size, config.bg_color);

    // Draw each line as a thin horizontal bar (minimap style).
    let line_height = 5u32;
    let line_gap = 1u32;
    let margin = 6u32;
    let text_col = config.text_color;

    // `margin` is a constant but `size` is `ThumbConfig::size`, which callers
    // set. Every `size - margin` here used to be a plain subtraction, so a
    // configured size below 6 wrapped it to about four billion: the two guards
    // that were supposed to stop the drawing at the right-hand margin both
    // became vacuously true, and the write ran off the end of the buffer. The
    // bar is now positioned inside `content`, which is empty when the margins
    // do not fit, and drawn through a clipping `fill_rect` besides.
    let content = size.saturating_sub(margin.saturating_mul(2));
    let right_edge = size.saturating_sub(margin);

    for (i, line) in lines.iter().enumerate() {
        let y =
            margin.saturating_add((i as u32).saturating_mul(line_height.saturating_add(line_gap)));
        if y.saturating_add(line_height) >= right_edge {
            break;
        }
        // Line width proportional to character count, capped at thumbnail width.
        let max_chars = (content / 2) as usize;
        let bar_len = (line.len().min(max_chars) as u32).saturating_mul(2);
        if bar_len == 0 {
            continue;
        }
        let bar_len = bar_len.min(right_edge.saturating_sub(margin));

        canvas.fill_rect(margin, y, bar_len, line_height.min(3), text_col);
    }

    into_thumbnail(canvas, path, mtime)
}

/// Generate a folder thumbnail showing a contents indicator.
///
/// Counts items inside the directory and draws a 2x2 grid of mini-icons for
/// the first 4 child entries on a folder-colored background.
fn generate_folder_thumbnail(path: &Path, config: &ThumbConfig, mtime: u64) -> Thumbnail {
    let size = config.size;
    let folder_color = ThumbCategory::Folder.accent_color();
    let mut canvas = Canvas::filled(size, size, folder_color);

    // Draw a darker "tab" at the top-left (classic folder shape).
    let tab_w = size / 3;
    let tab_h = size / 8;
    let darker = Color::rgb(
        folder_color.r.saturating_sub(40),
        folder_color.g.saturating_sub(40),
        folder_color.b.saturating_sub(40),
    );
    canvas.fill_rect(0, 0, tab_w, tab_h, darker);

    // Read up to 4 child entries and draw mini-icons in a 2x2 grid.
    if let Ok(entries) = fs::read_dir(path) {
        let children: Vec<_> = entries
            .filter_map(|e| e.ok())
            .take(FOLDER_PREVIEW_ITEMS)
            .collect();

        let cell = size / 4;
        let grid_x = size / 4;
        let grid_y = (size / 4).saturating_add(tab_h);

        for (i, entry) in children.iter().enumerate() {
            let col = (i % 2) as u32;
            let row = (i / 2) as u32;
            let step = cell.saturating_add(4);
            let cx = grid_x.saturating_add(col.saturating_mul(step));
            let cy = grid_y.saturating_add(row.saturating_mul(step));

            let cat = if entry.path().is_dir() {
                ThumbCategory::Folder
            } else {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                ThumbCategory::from_extension(&ext)
            };
            // Draw a small filled rectangle for this child item. The grid runs
            // off the right and bottom edges for every `size` (the rightmost
            // column starts at `3/4 · size + 4`); `fill_rect` clips it, which
            // is what the hand-written `break` on each axis was doing.
            let rect_size = cell.min(20);
            canvas.fill_rect(cx, cy, rect_size, rect_size, cat.accent_color());
        }
    }

    into_thumbnail(canvas, path, mtime)
}

/// Generate a PDF placeholder thumbnail.
///
/// Red document icon with "PDF" text.  The page count is not determined here
/// (would require a full PDF parser); this is a recognizable placeholder.
fn generate_pdf_placeholder(path: &Path, config: &ThumbConfig, mtime: u64) -> Thumbnail {
    let size = config.size;
    let red = ThumbCategory::Pdf.accent_color();
    let white = Color::WHITE;

    let mut canvas = Canvas::filled(size, size, white);

    // Draw a red document rectangle (inset from edges).
    let margin = size / 8;
    let inner = size.saturating_sub(margin.saturating_mul(2));
    canvas.fill_rect(margin, margin, inner, inner, red);

    // Draw a white "dog ear" triangle in the top-right corner of the document.
    let ear_size = size / 6;
    let right_edge = size.saturating_sub(margin);
    let ear_x_start = right_edge.saturating_sub(ear_size);
    let ear_y_end = margin.saturating_add(ear_size);
    for y in margin..ear_y_end {
        let row_offset = y.saturating_sub(margin);
        let x_start = ear_x_start.saturating_add(row_offset);
        canvas.fill_rect(x_start, y, right_edge.saturating_sub(x_start), 1, white);
    }

    // Draw "PDF" text as white pixels in the center region (simple block font).
    draw_block_text(&mut canvas, "PDF", white, size / 3, size / 2);

    into_thumbnail(canvas, path, mtime)
}

/// Generate a default/placeholder thumbnail for a category.
///
/// Uses the category's accent color and icon label to produce a recognizable
/// placeholder.
fn generate_default_thumbnail(
    path: &Path,
    category: ThumbCategory,
    config: &ThumbConfig,
    mtime: u64,
) -> Thumbnail {
    let size = config.size;
    let accent = category.accent_color();
    let mut canvas = Canvas::filled(size, size, config.bg_color);

    // Draw centered accent-colored circle. Distances are compared in `i64`:
    // `radius` is `size / 3` and `size` is caller-supplied, so squaring it in
    // `i32` overflows for a thumbnail wider than about 92 000 pixels.
    let cx = i64::from(size / 2);
    let cy = i64::from(size / 2);
    let radius = i64::from(size / 3);
    let r2 = radius.saturating_mul(radius);
    for y in 0..size {
        for x in 0..size {
            let dx = i64::from(x).saturating_sub(cx);
            let dy = i64::from(y).saturating_sub(cy);
            let d2 = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            if d2 <= r2 {
                canvas.set(x, y, accent);
            }
        }
    }

    into_thumbnail(canvas, path, mtime)
}

// ============================================================================
// Block text rendering (pixel-level, for thumbnails)
// ============================================================================

/// Simple 5x7 pixel font glyphs for uppercase ASCII letters and digits.
///
/// Each glyph is stored as 7 rows of 5-bit bitmasks (MSB = leftmost column).
/// Only the characters needed for thumbnail labels (P, D, F, digits) are
/// included; unknown chars render as a blank space.
fn glyph_bitmap(ch: char) -> [u8; 7] {
    match ch {
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'D' => [
            0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        _ => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    }
}

/// Draw a string using the block font onto a canvas.
///
/// The text is centred on `(x, y)` and drawn at a 2x scale factor for
/// visibility on thumbnail-sized images. Anything falling outside the canvas
/// is clipped.
///
/// This took a `&mut [u8]` and a separate `size` describing it, which is two
/// arguments that have to agree and a panic if they ever did not; the canvas
/// carries its own dimensions, so there is nothing left to disagree.
fn draw_block_text(canvas: &mut Canvas, text: &str, color: Color, x: u32, y: u32) {
    const SCALE: u32 = 2;
    const GLYPH_W: u32 = 5 * SCALE;
    const GLYPH_H: u32 = 7 * SCALE;
    const ADVANCE: u32 = GLYPH_W + SCALE;

    let total_w = (text.chars().count() as u32).saturating_mul(ADVANCE);
    // Centre horizontally and vertically around the given point.
    let start_x = x.saturating_sub(total_w / 2);
    let start_y = y.saturating_sub(GLYPH_H / 2);

    for (ci, ch) in text.chars().enumerate() {
        let bitmap = glyph_bitmap(ch);
        let char_x = start_x.saturating_add((ci as u32).saturating_mul(ADVANCE));

        for (row, bits) in bitmap.iter().enumerate() {
            for col in 0..5u32 {
                // Bit 4 is the leftmost column of the 5-wide glyph.
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                let px = char_x.saturating_add(col.saturating_mul(SCALE));
                let py = start_y.saturating_add((row as u32).saturating_mul(SCALE));
                canvas.fill_rect(px, py, SCALE, SCALE, color);
            }
        }
    }
}

// ============================================================================
// Background generation queue
// ============================================================================

/// A request to generate a thumbnail in the background.
#[derive(Clone, Debug)]
pub struct ThumbnailRequest {
    /// File path to generate a thumbnail for.
    pub path: PathBuf,
    /// Modification time at the time of the request (for invalidation check).
    pub mtime: u64,
    /// File size at the time of the request.
    pub size: u64,
    /// Generation configuration.
    pub config: ThumbConfig,
}

/// Background thumbnail generator with a request queue.
///
/// Callers submit requests via [`push`], then call [`process_batch`] to
/// generate some number of thumbnails synchronously (suitable for calling once
/// per frame or on idle).  Completed thumbnails are collected via
/// [`take_completed`].
///
/// When the directory changes, call [`cancel_all`] to clear the pending queue.
pub struct ThumbnailGenerator {
    /// Pending requests (FIFO).
    pending: VecDeque<ThumbnailRequest>,
    /// Completed thumbnails ready for the caller.
    completed: Vec<(ThumbnailRequest, Thumbnail)>,
}

impl ThumbnailGenerator {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            completed: Vec::new(),
        }
    }

    /// Queue a thumbnail generation request.
    pub fn push(&mut self, req: ThumbnailRequest) {
        self.pending.push_back(req);
    }

    /// Number of pending requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Number of completed-but-not-yet-taken results.
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    /// Process up to `batch_size` pending requests synchronously.
    ///
    /// Returns the number of thumbnails generated this call.
    pub fn process_batch(&mut self, batch_size: usize) -> usize {
        let mut processed = 0;
        for _ in 0..batch_size {
            let req = match self.pending.pop_front() {
                Some(r) => r,
                None => break,
            };
            let thumb = generate_thumbnail(&req.path, &req.config);
            self.completed.push((req, thumb));
            processed += 1;
        }
        processed
    }

    /// Take all completed thumbnails, draining the completed buffer.
    pub fn take_completed(&mut self) -> Vec<(ThumbnailRequest, Thumbnail)> {
        std::mem::take(&mut self.completed)
    }

    /// Cancel all pending requests (e.g. when the user changes directories).
    pub fn cancel_all(&mut self) {
        self.pending.clear();
    }
}

// ============================================================================
// Disk cache
// ============================================================================

/// Persistent disk cache for thumbnails.
///
/// Thumbnails are stored as raw ARGB files under `~/.cache/thumbs/` with
/// filenames derived from a simple hash of the source path and mtime.  This
/// avoids re-generating thumbnails across explorer restarts.
pub struct DiskCache {
    /// Root directory for the disk cache.
    cache_dir: PathBuf,
}

impl DiskCache {
    /// Create a new disk cache using the given directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Create a disk cache using the default location (`~/.cache/thumbs/`).
    pub fn default_location() -> Option<Self> {
        // Use HOME on Unix-like systems, USERPROFILE on Windows.
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        let dir = PathBuf::from(home).join(DISK_CACHE_DIR);
        Some(Self::new(dir))
    }

    /// Ensure the cache directory exists.
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.cache_dir)
    }

    /// Compute the cache filename for a given path and mtime.
    fn cache_filename(&self, path: &Path, mtime: u64) -> PathBuf {
        let hash = simple_hash(path, mtime);
        self.cache_dir.join(format!("{hash:016x}.thumb"))
    }

    /// Try to load a cached thumbnail from disk.
    pub fn load(&self, path: &Path, mtime: u64) -> Option<Thumbnail> {
        let file_path = self.cache_filename(path, mtime);
        let data = fs::read(&file_path).ok()?;

        // Format: [width: 4 LE][height: 4 LE][ARGB pixel data...]
        //
        // The header is the cache file's own claim about its contents, and the
        // cache directory is a plain directory in the user's home — nothing
        // stops a file there from claiming dimensions its pixel data does not
        // match. `Canvas::from_argb` is the check, and it is the same one every
        // other route into a `Canvas` goes through.
        let width = u32::from_le_bytes(read_array(&data, 0)?);
        let height = u32::from_le_bytes(read_array(&data, 4)?);
        let pixel_data = data.get(8..)?;
        Some(into_thumbnail(
            Canvas::from_argb(width, height, pixel_data)?,
            path,
            mtime,
        ))
    }

    /// Save a thumbnail to the disk cache.
    pub fn save(&self, thumb: &Thumbnail) -> std::io::Result<()> {
        self.ensure_dir()?;
        let file_path = self.cache_filename(&thumb.source_path, thumb.source_mtime);

        let mut data = Vec::with_capacity(8 + thumb.pixels.len());
        data.extend_from_slice(&thumb.width.to_le_bytes());
        data.extend_from_slice(&thumb.height.to_le_bytes());
        data.extend_from_slice(&thumb.pixels);
        fs::write(file_path, &data)
    }

    /// Remove the cached thumbnail for a specific path/mtime.
    pub fn remove(&self, path: &Path, mtime: u64) {
        let file_path = self.cache_filename(path, mtime);
        let _ = fs::remove_file(file_path); // Intentionally ignoring error: file may not exist.
    }

    /// Purge all entries from the disk cache.
    pub fn clear(&self) -> std::io::Result<()> {
        if self.cache_dir.is_dir() {
            for entry in fs::read_dir(&self.cache_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("thumb") {
                    let _ = fs::remove_file(&path); // Best-effort removal.
                }
            }
        }
        Ok(())
    }

    /// Purge entries whose source file no longer exists.
    ///
    /// Since the cache filename is a hash (not the original path), this method
    /// requires scanning the in-memory cache for paths.  Pass the set of
    /// known-valid source paths; anything in the cache directory that doesn't
    /// correspond to a valid entry is removed.
    pub fn purge_stale(&self, valid_entries: &HashMap<PathBuf, u64>) -> std::io::Result<()> {
        if !self.cache_dir.is_dir() {
            return Ok(());
        }

        let valid_filenames: std::collections::HashSet<String> = valid_entries
            .iter()
            .map(|(path, mtime)| format!("{:016x}.thumb", simple_hash(path, *mtime)))
            .collect();

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            // Compared as bytes. Every name we write is `{hash:016x}.thumb`, so
            // a name that is not UTF-8 is not one of ours; rendering it lossily
            // first could make it *look* like one of ours and get it deleted.
            let raw = entry.file_name();
            let name = raw.as_encoded_bytes();
            if name.ends_with(b".thumb")
                && !valid_filenames.iter().any(|valid| valid.as_bytes() == name)
            {
                let _ = fs::remove_file(entry.path()); // Best-effort removal.
            }
        }
        Ok(())
    }
}

// ============================================================================
// Rendering (RenderCommand output)
// ============================================================================

/// Render a thumbnail at position `(x, y)` within a `display_size x display_size`
/// bounding box, producing guitk `RenderCommand`s.
///
/// The thumbnail is scaled to fit within the display box while maintaining its
/// aspect ratio.  A thin border and optional shadow are added for image-type
/// thumbnails.
pub fn render_thumbnail(
    thumb: &Thumbnail,
    x: f32,
    y: f32,
    display_size: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    if thumb.width == 0 || thumb.height == 0 {
        return cmds;
    }

    // Compute display dimensions preserving aspect ratio.
    let (dw, dh) = fit_dimensions(thumb.width, thumb.height, display_size as u32);
    let dw = dw as f32;
    let dh = dh as f32;

    // Center within the display_size box.
    let off_x = (display_size - dw) / 2.0;
    let off_y = (display_size - dh) / 2.0;
    let rx = x + off_x;
    let ry = y + off_y;

    // Shadow behind the thumbnail (subtle drop shadow).
    cmds.push(RenderCommand::BoxShadow {
        x: rx,
        y: ry,
        width: dw,
        height: dh,
        offset_x: 1.0,
        offset_y: 2.0,
        blur: 4.0,
        spread: 0.0,
        color: Color::rgba(0, 0, 0, 60),
        corner_radii: CornerRadii::all(2.0),
    });

    // Background fill (in case the thumbnail has transparency).
    cmds.push(RenderCommand::FillRect {
        x: rx,
        y: ry,
        width: dw,
        height: dh,
        color: Color::WHITE,
        corner_radii: CornerRadii::all(2.0),
    });

    // The actual thumbnail image.  We emit an Image command with a synthesized
    // image_id derived from the source path hash, since the compositor
    // maintains an image asset store.  The caller is responsible for
    // registering the pixel data with the compositor under this ID.
    let image_id = thumbnail_image_id(thumb);
    cmds.push(RenderCommand::Image {
        x: rx,
        y: ry,
        width: dw,
        height: dh,
        image_id,
    });

    // Thin border around the thumbnail.
    cmds.push(RenderCommand::StrokeRect {
        x: rx,
        y: ry,
        width: dw,
        height: dh,
        color: Color::rgba(0, 0, 0, 30),
        line_width: 1.0,
        corner_radii: CornerRadii::all(2.0),
    });

    cmds
}

/// Render a thumbnail-sized default/placeholder icon using only primitive
/// drawing commands (no Image asset required).
///
/// This is useful when the full thumbnail pixel data has not been registered
/// with the compositor yet — the caller can show this placeholder immediately.
pub fn render_placeholder(
    category: ThumbCategory,
    label: Option<&str>,
    x: f32,
    y: f32,
    display_size: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    let accent = category.accent_color();

    // Background circle.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width: display_size,
        height: display_size,
        color: accent,
        corner_radii: CornerRadii::all(display_size / 4.0),
    });

    // Icon label text centered.
    let text = label.unwrap_or(category.icon_label());
    let font_size = display_size / 3.0;
    cmds.push(RenderCommand::Text {
        x: guitk::text::center_x(
            text,
            x + display_size / 2.0,
            font_size,
            FontWeightHint::Bold,
        ),
        y: y + display_size / 2.0 - font_size / 2.0,
        text: text.to_owned(),
        color: Color::WHITE,
        font_size,
        font_weight: FontWeightHint::Bold,
        max_width: Some(display_size),
        overflow: TextOverflow::Ellipsis,
    });

    cmds
}

/// Compute a stable image ID for a thumbnail, usable as a key in the
/// compositor's image asset store.
pub fn thumbnail_image_id(thumb: &Thumbnail) -> u64 {
    simple_hash(&thumb.source_path, thumb.source_mtime)
}

// ============================================================================
// Utility functions
// ============================================================================

/// Read the first `n` bytes of a file (for header parsing).
fn read_file_header(path: &Path, n: usize) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; n];
    let bytes_read = file.read(&mut buf).ok()?;
    buf.truncate(bytes_read);
    Some(buf)
}

/// Read the first `max_lines` lines of a text file.
fn read_text_lines(path: &Path, max_lines: usize) -> Option<Vec<String>> {
    let file = fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file.take(TEXT_PREVIEW_MAX_BYTES as u64));
    let lines: Vec<String> = reader
        .lines()
        .take(max_lines)
        .filter_map(|l| l.ok())
        .collect();
    Some(lines)
}

/// Get the modification time of a file as seconds since the Unix epoch.
fn file_mtime(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let duration = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(duration.as_secs())
}

/// Simple non-cryptographic hash for cache keys.
///
/// Uses FNV-1a-style hashing on the path string concatenated with the mtime.
/// This is not meant to be collision-resistant — just a fast, deterministic
/// mapping to a 64-bit filename.
fn simple_hash(path: &Path, mtime: u64) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis
    // The path's own bytes, not a lossy rendering: two names that differ only
    // in undecodable bytes are different files and must not share a cache
    // entry.
    for byte in path.as_os_str().as_encoded_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3); // FNV prime
    }
    // Mix in the mtime.
    for byte in mtime.to_le_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use std::io::Write;

    // -- LRU cache tests ----------------------------------------------------

    #[test]
    fn cache_insert_and_get() {
        let mut cache = ThumbnailCache::new(10);
        let thumb = make_test_thumb("test.png", 100);
        cache.insert("test.png", 12345, 1000, thumb);

        assert_eq!(cache.len(), 1);
        let got = cache.get("test.png", 12345, 1000);
        assert!(got.is_some());
        assert_eq!(got.unwrap().source_path, Path::new("test.png"));
    }

    #[test]
    fn cache_miss_wrong_mtime() {
        let mut cache = ThumbnailCache::new(10);
        let thumb = make_test_thumb("file.txt", 100);
        cache.insert("file.txt", 100, 500, thumb);

        // Same path but different mtime => miss.
        assert!(cache.get("file.txt", 200, 500).is_none());
    }

    #[test]
    fn cache_miss_wrong_size() {
        let mut cache = ThumbnailCache::new(10);
        let thumb = make_test_thumb("file.txt", 100);
        cache.insert("file.txt", 100, 500, thumb);

        // Same path and mtime but different size => miss.
        assert!(cache.get("file.txt", 100, 999).is_none());
    }

    #[test]
    fn cache_evicts_lru() {
        let mut cache = ThumbnailCache::new(3);
        cache.insert("a", 1, 10, make_test_thumb("a", 10));
        cache.insert("b", 2, 20, make_test_thumb("b", 10));
        cache.insert("c", 3, 30, make_test_thumb("c", 10));

        // Cache is full (3 items). Inserting a 4th should evict "a" (LRU).
        cache.insert("d", 4, 40, make_test_thumb("d", 10));
        assert_eq!(cache.len(), 3);
        assert!(cache.get("a", 1, 10).is_none());
        assert!(cache.get("b", 2, 20).is_some());
        assert!(cache.get("d", 4, 40).is_some());
    }

    #[test]
    fn cache_promotes_on_get() {
        let mut cache = ThumbnailCache::new(3);
        cache.insert("a", 1, 10, make_test_thumb("a", 10));
        cache.insert("b", 2, 20, make_test_thumb("b", 10));
        cache.insert("c", 3, 30, make_test_thumb("c", 10));

        // Access "a" to promote it — now "b" is the LRU.
        let _ = cache.get("a", 1, 10);
        cache.insert("d", 4, 40, make_test_thumb("d", 10));

        assert!(cache.get("a", 1, 10).is_some()); // promoted, still there
        assert!(cache.get("b", 2, 20).is_none()); // evicted
    }

    #[test]
    fn cache_invalidate_removes_all_matching_path() {
        let mut cache = ThumbnailCache::new(10);
        cache.insert("x", 1, 10, make_test_thumb("x", 10));
        cache.insert("x", 2, 20, make_test_thumb("x", 10));
        cache.insert("y", 3, 30, make_test_thumb("y", 10));

        cache.invalidate("x");
        assert_eq!(cache.len(), 1);
        assert!(cache.get("x", 1, 10).is_none());
        assert!(cache.get("x", 2, 20).is_none());
        assert!(cache.get("y", 3, 30).is_some());
    }

    #[test]
    fn cache_clear() {
        let mut cache = ThumbnailCache::new(10);
        cache.insert("a", 1, 10, make_test_thumb("a", 10));
        cache.insert("b", 2, 20, make_test_thumb("b", 10));

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    // -- Cache key tests ----------------------------------------------------

    #[test]
    fn cache_key_equality() {
        let k1 = CacheKey::new("/foo/bar.png", 123, 456);
        let k2 = CacheKey::new("/foo/bar.png", 123, 456);
        let k3 = CacheKey::new("/foo/bar.png", 999, 456);
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn cache_key_hash_consistency() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let k1 = CacheKey::new("test", 42, 100);
        let k2 = CacheKey::new("test", 42, 100);

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        k1.hash(&mut h1);
        k2.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    // -- Default icon selection by extension ---------------------------------

    #[test]
    fn category_from_extension() {
        assert_eq!(ThumbCategory::from_extension("png"), ThumbCategory::Image);
        assert_eq!(ThumbCategory::from_extension("JPG"), ThumbCategory::Image);
        assert_eq!(ThumbCategory::from_extension("rs"), ThumbCategory::Text);
        assert_eq!(ThumbCategory::from_extension("pdf"), ThumbCategory::Pdf);
        assert_eq!(ThumbCategory::from_extension("mp3"), ThumbCategory::Audio);
        assert_eq!(ThumbCategory::from_extension("mkv"), ThumbCategory::Video);
        assert_eq!(ThumbCategory::from_extension("zip"), ThumbCategory::Archive);
        assert_eq!(
            ThumbCategory::from_extension("exe"),
            ThumbCategory::Executable
        );
        assert_eq!(ThumbCategory::from_extension("???"), ThumbCategory::Unknown);
    }

    #[test]
    fn category_icon_labels_non_empty() {
        let categories = [
            ThumbCategory::Image,
            ThumbCategory::Text,
            ThumbCategory::Folder,
            ThumbCategory::Pdf,
            ThumbCategory::Audio,
            ThumbCategory::Video,
            ThumbCategory::Archive,
            ThumbCategory::Executable,
            ThumbCategory::Unknown,
        ];
        for cat in categories {
            assert!(
                !cat.icon_label().is_empty(),
                "icon label empty for {:?}",
                cat
            );
        }
    }

    // -- Text preview truncation --------------------------------------------

    #[test]
    fn text_preview_truncates_to_max_lines() {
        let dir = std::env::temp_dir().join("thumbs_test_text");
        let _ = fs::create_dir_all(&dir);
        let file_path = dir.join("long.txt");

        {
            let mut f = fs::File::create(&file_path).unwrap();
            for i in 0..50 {
                writeln!(f, "Line {i}: some content here").unwrap();
            }
        }

        let lines = read_text_lines(&file_path, TEXT_PREVIEW_MAX_LINES).unwrap();
        assert!(lines.len() <= TEXT_PREVIEW_MAX_LINES);
        assert!(!lines.is_empty());

        let _ = fs::remove_file(&file_path);
        let _ = fs::remove_dir(&dir);
    }

    // -- Image downscale logic ----------------------------------------------

    #[test]
    fn fit_dimensions_preserves_aspect() {
        // Landscape image.
        let (w, h) = fit_dimensions(200, 100, 128);
        assert_eq!(w, 128);
        assert_eq!(h, 64);

        // Portrait image.
        let (w, h) = fit_dimensions(100, 200, 128);
        assert_eq!(w, 64);
        assert_eq!(h, 128);

        // Square image.
        let (w, h) = fit_dimensions(300, 300, 128);
        assert_eq!(w, 128);
        assert_eq!(h, 128);
    }

    #[test]
    fn fit_dimensions_no_upscale() {
        let (w, h) = fit_dimensions(50, 30, 128);
        assert_eq!(w, 50);
        assert_eq!(h, 30);
    }

    #[test]
    fn fit_dimensions_zero_handling() {
        assert_eq!(fit_dimensions(0, 100, 128), (0, 0));
        assert_eq!(fit_dimensions(100, 0, 128), (0, 0));
        assert_eq!(fit_dimensions(100, 100, 0), (0, 0));
    }

    #[test]
    fn box_filter_downscale_basic() {
        let red = Color::rgba(255, 0, 0, 255);
        let src = Canvas::filled(4, 4, red);

        let dst = box_filter_downscale(&src, 2);
        assert_eq!(dst.width(), 2);
        assert_eq!(dst.height(), 2);

        // Every pixel should still be red (uniform source).
        for y in 0..dst.height() {
            for x in 0..dst.width() {
                assert_eq!(dst.get(x, y), Some(red));
            }
        }
    }

    #[test]
    fn box_filter_downscale_empty() {
        let dst = box_filter_downscale(&Canvas::transparent(0, 0), 128);
        assert_eq!(dst.width(), 0);
        assert_eq!(dst.height(), 0);
    }

    #[test]
    fn box_filter_no_downscale_when_smaller() {
        let src = Canvas::filled(10, 10, Color::rgba(128, 128, 128, 128));
        let dst = box_filter_downscale(&src, 128);
        // Source already fits; should return original.
        assert_eq!(dst.width(), 10);
        assert_eq!(dst.height(), 10);
        assert_eq!(dst.get(5, 5), src.get(5, 5));
    }

    // -- Image header parsing -----------------------------------------------

    #[test]
    fn parse_bmp_valid() {
        let mut header = vec![0u8; 54];
        header[0] = b'B';
        header[1] = b'M';
        // Width = 320 (LE u32 at offset 18)
        header[18..22].copy_from_slice(&320u32.to_le_bytes());
        // Height = 240 (LE u32 at offset 22)
        header[22..26].copy_from_slice(&240u32.to_le_bytes());

        let dims = parse_bmp_dimensions(&header).unwrap();
        assert_eq!(dims.width, 320);
        assert_eq!(dims.height, 240);
    }

    #[test]
    fn parse_bmp_negative_height() {
        let mut header = vec![0u8; 54];
        header[0] = b'B';
        header[1] = b'M';
        header[18..22].copy_from_slice(&100u32.to_le_bytes());
        // Negative height (top-down BMP) stored as i32.
        header[22..26].copy_from_slice(&(-200i32 as u32).to_le_bytes());

        let dims = parse_bmp_dimensions(&header).unwrap();
        assert_eq!(dims.width, 100);
        assert_eq!(dims.height, 200);
    }

    #[test]
    fn parse_png_valid() {
        let mut header = vec![0u8; 24];
        header[0..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // Width at offset 16, height at offset 20 (BE u32).
        header[16..20].copy_from_slice(&640u32.to_be_bytes());
        header[20..24].copy_from_slice(&480u32.to_be_bytes());

        let dims = parse_png_dimensions(&header).unwrap();
        assert_eq!(dims.width, 640);
        assert_eq!(dims.height, 480);
    }

    #[test]
    fn parse_gif_valid() {
        let mut header = vec![0u8; 10];
        header[0..6].copy_from_slice(b"GIF89a");
        header[6..8].copy_from_slice(&256u16.to_le_bytes());
        header[8..10].copy_from_slice(&192u16.to_le_bytes());

        let dims = parse_gif_dimensions(&header).unwrap();
        assert_eq!(dims.width, 256);
        assert_eq!(dims.height, 192);
    }

    #[test]
    fn parse_jpeg_valid() {
        // Minimal JPEG with SOF0 marker.
        let mut data = vec![0xFF, 0xD8]; // SOI
        // APP0 marker (skip it)
        data.extend_from_slice(&[0xFF, 0xE0]);
        data.extend_from_slice(&16u16.to_be_bytes()); // segment length
        data.extend_from_slice(&[0u8; 14]); // payload
        // SOF0 marker
        data.extend_from_slice(&[0xFF, 0xC0]);
        data.extend_from_slice(&17u16.to_be_bytes()); // segment length
        data.push(8); // precision
        data.extend_from_slice(&480u16.to_be_bytes()); // height
        data.extend_from_slice(&640u16.to_be_bytes()); // width
        data.extend_from_slice(&[0u8; 10]); // rest of SOF

        let dims = parse_jpeg_dimensions(&data).unwrap();
        assert_eq!(dims.width, 640);
        assert_eq!(dims.height, 480);
    }

    #[test]
    fn parse_image_dimensions_tries_all_formats() {
        // BMP header.
        let mut bmp = vec![0u8; 54];
        bmp[0] = b'B';
        bmp[1] = b'M';
        bmp[18..22].copy_from_slice(&100u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&50u32.to_le_bytes());
        assert!(parse_image_dimensions(&bmp).is_some());

        // Garbage data.
        assert!(parse_image_dimensions(&[0, 1, 2, 3]).is_none());
    }

    // -- Render command generation ------------------------------------------

    #[test]
    fn render_thumbnail_produces_commands() {
        let thumb = make_test_thumb("test.png", 64);
        let cmds = render_thumbnail(&thumb, 10.0, 20.0, 100.0);

        // Should produce: BoxShadow, FillRect, Image, StrokeRect
        assert_eq!(cmds.len(), 4);
        assert!(matches!(cmds[0], RenderCommand::BoxShadow { .. }));
        assert!(matches!(cmds[1], RenderCommand::FillRect { .. }));
        assert!(matches!(cmds[2], RenderCommand::Image { .. }));
        assert!(matches!(cmds[3], RenderCommand::StrokeRect { .. }));
    }

    #[test]
    fn render_thumbnail_empty_returns_nothing() {
        let thumb = Thumbnail {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            source_path: PathBuf::new(),
            source_mtime: 0,
        };
        let cmds = render_thumbnail(&thumb, 0.0, 0.0, 64.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_placeholder_produces_commands() {
        let cmds = render_placeholder(ThumbCategory::Audio, None, 0.0, 0.0, 64.0);
        assert!(cmds.len() >= 2); // FillRect + Text at minimum
    }

    // -- Hash consistency ---------------------------------------------------

    #[test]
    fn simple_hash_deterministic() {
        let h1 = simple_hash(Path::new("/foo/bar.png"), 12345);
        let h2 = simple_hash(Path::new("/foo/bar.png"), 12345);
        assert_eq!(h1, h2);
    }

    #[test]
    fn simple_hash_varies_with_mtime() {
        let h1 = simple_hash(Path::new("/foo/bar.png"), 100);
        let h2 = simple_hash(Path::new("/foo/bar.png"), 200);
        assert_ne!(h1, h2);
    }

    #[test]
    fn simple_hash_varies_with_path() {
        let h1 = simple_hash(Path::new("/foo/bar.png"), 100);
        let h2 = simple_hash(Path::new("/foo/baz.png"), 100);
        assert_ne!(h1, h2);
    }

    /// The cache key must separate files the *filesystem* separates. Held as a
    /// lossy string, every undecodable byte became the same U+FFFD, so these
    /// two distinct files hashed to one cache filename and the explorer showed
    /// one of them the other's thumbnail.
    ///
    /// Unix-only: a Windows `OsString` cannot hold either of these paths, and
    /// our target is `target-family = ["unix"]`.
    #[cfg(unix)]
    #[test]
    fn two_names_differing_only_in_undecodable_bytes_do_not_share_a_cache_entry() {
        use std::os::unix::ffi::OsStrExt;
        let a = Path::new(std::ffi::OsStr::from_bytes(b"/foo/x\xE9.png"));
        let b = Path::new(std::ffi::OsStr::from_bytes(b"/foo/x\xFF.png"));
        assert_ne!(a, b, "the two paths are genuinely different files");
        assert_ne!(
            simple_hash(a, 100),
            simple_hash(b, 100),
            "and must not share a cache filename"
        );
    }

    /// The same property, expressed in the one form the Windows test host can
    /// represent: an unpaired surrogate is a legal `OsString` that
    /// `to_string_lossy` maps to U+FFFD. Without this the regression above is
    /// asserted only on a target we cannot execute here.
    #[cfg(windows)]
    #[test]
    fn two_names_differing_only_in_undecodable_units_do_not_share_a_cache_entry() {
        use std::os::windows::ffi::OsStringExt;
        let a = PathBuf::from(std::ffi::OsString::from_wide(&[0x2F, 0xD800]));
        let b = PathBuf::from(std::ffi::OsString::from_wide(&[0x2F, 0xD801]));
        assert_ne!(a, b, "the two paths are genuinely different files");
        assert_eq!(
            a.to_string_lossy(),
            b.to_string_lossy(),
            "precondition: a lossy rendering cannot tell them apart"
        );
        assert_ne!(
            simple_hash(&a, 100),
            simple_hash(&b, 100),
            "and must not share a cache filename"
        );
    }

    // -- Thumbnail generator queue ------------------------------------------

    #[test]
    fn generator_push_and_process() {
        let mut tg = ThumbnailGenerator::new();
        assert_eq!(tg.pending_count(), 0);
        assert_eq!(tg.completed_count(), 0);

        // Push a request for a non-existent file; generator should still
        // produce a default thumbnail (no panic).
        tg.push(ThumbnailRequest {
            path: PathBuf::from("/nonexistent/file.txt"),
            mtime: 0,
            size: 0,
            config: ThumbConfig::default(),
        });

        assert_eq!(tg.pending_count(), 1);
        let processed = tg.process_batch(10);
        assert_eq!(processed, 1);
        assert_eq!(tg.pending_count(), 0);
        assert_eq!(tg.completed_count(), 1);

        let results = tg.take_completed();
        assert_eq!(results.len(), 1);
        assert_eq!(tg.completed_count(), 0);
    }

    #[test]
    fn generator_cancel_all() {
        let mut tg = ThumbnailGenerator::new();
        for i in 0..5 {
            tg.push(ThumbnailRequest {
                path: PathBuf::from(format!("/file{i}.txt")),
                mtime: 0,
                size: 0,
                config: ThumbConfig::default(),
            });
        }
        assert_eq!(tg.pending_count(), 5);
        tg.cancel_all();
        assert_eq!(tg.pending_count(), 0);
    }

    // -- Block text rendering -----------------------------------------------

    #[test]
    fn glyph_bitmap_pdf_chars_not_blank() {
        let p = glyph_bitmap('P');
        let d = glyph_bitmap('D');
        let f = glyph_bitmap('F');
        // At least some rows should be non-zero.
        assert!(p.iter().any(|&r| r != 0), "P glyph is blank");
        assert!(d.iter().any(|&r| r != 0), "D glyph is blank");
        assert!(f.iter().any(|&r| r != 0), "F glyph is blank");
    }

    #[test]
    fn draw_block_text_does_not_panic() {
        let mut canvas = Canvas::transparent(64, 64);
        // Should not panic even with text near edges.
        draw_block_text(&mut canvas, "PDF", Color::WHITE, 32, 32);
        draw_block_text(&mut canvas, "123", Color::WHITE, 0, 0);
        draw_block_text(&mut canvas, "999", Color::WHITE, 63, 63);
        // Nor when the canvas is smaller than a single glyph.
        let mut tiny = Canvas::transparent(3, 3);
        draw_block_text(&mut tiny, "PDF", Color::WHITE, 1, 1);
        assert_eq!(tiny.width(), 3);
    }

    // -- Thumbnail validity -------------------------------------------------

    #[test]
    fn thumbnail_validity_check() {
        let good = make_test_thumb("ok.png", 4);
        assert!(good.is_valid());

        let bad = Thumbnail {
            width: 4,
            height: 4,
            pixels: vec![0u8; 10], // wrong length
            source_path: PathBuf::new(),
            source_mtime: 0,
        };
        assert!(!bad.is_valid());
    }

    // -- Disk cache (unit-level, using temp dir) ----------------------------

    #[test]
    fn disk_cache_save_load_roundtrip() {
        let dir = std::env::temp_dir().join("thumbs_test_disk");
        let cache = DiskCache::new(dir.clone());
        cache.ensure_dir().unwrap();

        let thumb = make_test_thumb("test_disk.png", 4);
        cache.save(&thumb).unwrap();

        let loaded = cache
            .load(Path::new("test_disk.png"), thumb.source_mtime)
            .unwrap();
        assert_eq!(loaded.width, thumb.width);
        assert_eq!(loaded.height, thumb.height);
        assert_eq!(loaded.pixels.len(), thumb.pixels.len());

        // Clean up.
        let _ = cache.clear();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_cache_miss_wrong_mtime() {
        let dir = std::env::temp_dir().join("thumbs_test_disk_miss");
        let cache = DiskCache::new(dir.clone());
        cache.ensure_dir().unwrap();

        let thumb = make_test_thumb("miss.png", 4);
        cache.save(&thumb).unwrap();

        // Different mtime => cache miss.
        assert!(
            cache
                .load(Path::new("miss.png"), thumb.source_mtime + 1)
                .is_none()
        );

        let _ = cache.clear();
        let _ = fs::remove_dir_all(&dir);
    }

    // -- Helper -------------------------------------------------------------

    /// Create a minimal test thumbnail with solid-colored pixels.
    fn make_test_thumb(name: &str, size: u32) -> Thumbnail {
        Thumbnail {
            width: size,
            height: size,
            pixels: vec![128u8; (size * size * 4) as usize],
            source_path: PathBuf::from(name),
            source_mtime: 42,
        }
    }
}
