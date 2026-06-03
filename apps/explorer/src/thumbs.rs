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

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
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
    pub source_path: String,
    /// Modification time of the source (seconds since epoch) for invalidation.
    pub source_mtime: u64,
}

impl Thumbnail {
    /// Total number of pixels.
    fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Returns `true` if the pixel buffer is consistent with the dimensions.
    fn is_valid(&self) -> bool {
        self.pixels.len() == self.pixel_count() * 4
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
    data.get(offset..offset + 4).map(|b| {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    })
}

/// Read a big-endian u32 from a byte slice at `offset`.
fn read_be_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4).map(|b| {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    })
}

/// Read a big-endian u16 from a byte slice at `offset`.
fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2).map(|b| {
        u16::from_be_bytes([b[0], b[1]])
    })
}

/// Read a little-endian u16 from a byte slice at `offset`.
fn read_le_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2).map(|b| {
        u16::from_le_bytes([b[0], b[1]])
    })
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

/// Downscale ARGB pixel data using a box filter.
///
/// `src` is row-major ARGB (4 bytes per pixel).  The output is sized to fit
/// within `target_size x target_size` while preserving aspect ratio.  If the
/// source is already smaller than the target, the original pixels are returned
/// unscaled.
fn box_filter_downscale(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    target_size: u32,
) -> (Vec<u8>, u32, u32) {
    if src_w == 0 || src_h == 0 || target_size == 0 {
        return (Vec::new(), 0, 0);
    }

    // Compute output dimensions preserving aspect ratio.
    let (dst_w, dst_h) = fit_dimensions(src_w, src_h, target_size);

    if dst_w >= src_w && dst_h >= src_h {
        // Source fits within target; return a copy.
        return (src.to_vec(), src_w, src_h);
    }

    let expected_len = (src_w as usize) * (src_h as usize) * 4;
    if src.len() < expected_len {
        // Incomplete pixel data — return a blank thumbnail.
        return (vec![0u8; (dst_w as usize) * (dst_h as usize) * 4], dst_w, dst_h);
    }

    let mut dst = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];

    for dy in 0..dst_h {
        for dx in 0..dst_w {
            // Source region that maps to this destination pixel.
            let sx0 = (dx as u64 * src_w as u64 / dst_w as u64) as u32;
            let sy0 = (dy as u64 * src_h as u64 / dst_h as u64) as u32;
            let sx1 = ((dx as u64 + 1) * src_w as u64).div_ceil(dst_w as u64)
                .min(src_w as u64) as u32;
            let sy1 = ((dy as u64 + 1) * src_h as u64).div_ceil(dst_h as u64)
                .min(src_h as u64) as u32;

            let mut r_acc: u64 = 0;
            let mut g_acc: u64 = 0;
            let mut b_acc: u64 = 0;
            let mut a_acc: u64 = 0;
            let mut count: u64 = 0;

            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let idx = (sy as usize * src_w as usize + sx as usize) * 4;
                    // ARGB order: [A, R, G, B]
                    a_acc += src[idx] as u64;
                    r_acc += src[idx + 1] as u64;
                    g_acc += src[idx + 2] as u64;
                    b_acc += src[idx + 3] as u64;
                    count += 1;
                }
            }

            // Single guard for all four channel divisions — converting each to
            // `checked_div` separately (per `manual_checked_ops`) would add
            // four redundant Option unwraps under the same `count > 0` proof.
            #[allow(clippy::manual_checked_ops)]
            if count > 0 {
                let dst_idx = (dy as usize * dst_w as usize + dx as usize) * 4;
                dst[dst_idx] = (a_acc / count) as u8;
                dst[dst_idx + 1] = (r_acc / count) as u8;
                dst[dst_idx + 2] = (g_acc / count) as u8;
                dst[dst_idx + 3] = (b_acc / count) as u8;
            }
        }
    }

    (dst, dst_w, dst_h)
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
            | "html" | "css" | "java" | "go" | "toml" | "yaml" | "json" | "xml" | "sh"
            | "cfg" | "ini" | "conf" => Self::Text,
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
            Self::Image => "\u{1F5BC}",     // framed picture
            Self::Text => "\u{1F4C4}",      // page
            Self::Folder => "\u{1F4C1}",    // folder
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
            Self::Image => Color::rgb(76, 175, 80),      // green
            Self::Text => Color::rgb(158, 158, 158),     // gray
            Self::Folder => Color::rgb(255, 193, 7),     // amber
            Self::Pdf => Color::rgb(211, 47, 47),        // red
            Self::Audio => Color::rgb(156, 39, 176),     // purple
            Self::Video => Color::rgb(33, 150, 243),     // blue
            Self::Archive => Color::rgb(121, 85, 72),    // brown
            Self::Executable => Color::rgb(96, 125, 139),// blue-gray
            Self::Unknown => Color::rgb(189, 189, 189),  // light gray
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
    if header.len() >= 2 && header[0] == b'B' && header[1] == b'M'
        && let Some(thumb) = try_bmp_thumbnail(path, dims, config, mtime) {
            return thumb;
        }

    // For other formats: create an aspect-ratio-correct color swatch since we
    // don't have a full decoder.  The swatch color is derived from the format.
    let (tw, th) = fit_dimensions(dims.width, dims.height, config.size);
    let size = config.size;
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];

    // Center the swatch within the thumbnail area.
    let off_x = (size - tw) / 2;
    let off_y = (size - th) / 2;
    let accent = ThumbCategory::Image.accent_color();

    for y in off_y..off_y + th {
        for x in off_x..off_x + tw {
            let idx = (y as usize * size as usize + x as usize) * 4;
            pixels[idx] = accent.a;
            pixels[idx + 1] = accent.r;
            pixels[idx + 2] = accent.g;
            pixels[idx + 3] = accent.b;
        }
    }

    Thumbnail {
        width: size,
        height: size,
        pixels,
        source_path: path.to_string_lossy().to_string(),
        source_mtime: mtime,
    }
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

    let bpp = bits_per_pixel as usize / 8;
    let row_size = (dims.width as usize * bpp).div_ceil(4) * 4; // rows padded to 4 bytes
    let expected_data = offset + row_size * dims.height as usize;
    if data.len() < expected_data {
        return None;
    }

    // BMP stores rows bottom-up by default (positive height).  Convert to
    // top-down ARGB.
    let height_raw = read_le_u32(&data, 22)? as i32;
    let bottom_up = height_raw > 0;

    let mut argb = vec![0u8; dims.width as usize * dims.height as usize * 4];
    for y in 0..dims.height as usize {
        let src_y = if bottom_up {
            dims.height as usize - 1 - y
        } else {
            y
        };
        let row_start = offset + src_y * row_size;
        for x in 0..dims.width as usize {
            let src_idx = row_start + x * bpp;
            let dst_idx = (y * dims.width as usize + x) * 4;
            // BMP pixel order is BGR(A).
            let b_val = *data.get(src_idx)?;
            let g_val = *data.get(src_idx + 1)?;
            let r_val = *data.get(src_idx + 2)?;
            let a_val = if bpp == 4 {
                *data.get(src_idx + 3)?
            } else {
                255
            };
            argb[dst_idx] = a_val;
            argb[dst_idx + 1] = r_val;
            argb[dst_idx + 2] = g_val;
            argb[dst_idx + 3] = b_val;
        }
    }

    let (scaled, sw, sh) = box_filter_downscale(&argb, dims.width, dims.height, config.size);

    Some(Thumbnail {
        width: sw,
        height: sh,
        pixels: scaled,
        source_path: path.to_string_lossy().to_string(),
        source_mtime: mtime,
    })
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
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];

    // Fill background.
    let bg = config.bg_color;
    for i in 0..(size as usize * size as usize) {
        pixels[i * 4] = bg.a;
        pixels[i * 4 + 1] = bg.r;
        pixels[i * 4 + 2] = bg.g;
        pixels[i * 4 + 3] = bg.b;
    }

    // Draw each line as a thin horizontal bar (minimap style).
    let line_height = 5u32;
    let line_gap = 1u32;
    let margin = 6u32;
    let text_col = config.text_color;

    for (i, line) in lines.iter().enumerate() {
        let y = margin + i as u32 * (line_height + line_gap);
        if y + line_height >= size - margin {
            break;
        }
        // Line width proportional to character count, capped at thumbnail width.
        let max_chars = ((size - 2 * margin) / 2) as usize;
        let bar_len = line.len().min(max_chars) as u32 * 2;
        if bar_len == 0 {
            continue;
        }

        for py in y..y + line_height.min(3) {
            for px in margin..margin + bar_len {
                if px >= size - margin {
                    break;
                }
                let idx = (py as usize * size as usize + px as usize) * 4;
                pixels[idx] = text_col.a;
                pixels[idx + 1] = text_col.r;
                pixels[idx + 2] = text_col.g;
                pixels[idx + 3] = text_col.b;
            }
        }
    }

    Thumbnail {
        width: size,
        height: size,
        pixels,
        source_path: path.to_string_lossy().to_string(),
        source_mtime: mtime,
    }
}

/// Generate a folder thumbnail showing a contents indicator.
///
/// Counts items inside the directory and draws a 2x2 grid of mini-icons for
/// the first 4 child entries on a folder-colored background.
fn generate_folder_thumbnail(path: &Path, config: &ThumbConfig, mtime: u64) -> Thumbnail {
    let size = config.size;
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];

    let folder_color = ThumbCategory::Folder.accent_color();

    // Fill with folder color.
    for i in 0..(size as usize * size as usize) {
        pixels[i * 4] = folder_color.a;
        pixels[i * 4 + 1] = folder_color.r;
        pixels[i * 4 + 2] = folder_color.g;
        pixels[i * 4 + 3] = folder_color.b;
    }

    // Draw a darker "tab" at the top-left (classic folder shape).
    let tab_w = size / 3;
    let tab_h = size / 8;
    let darker = Color::rgb(
        folder_color.r.saturating_sub(40),
        folder_color.g.saturating_sub(40),
        folder_color.b.saturating_sub(40),
    );
    for y in 0..tab_h {
        for x in 0..tab_w {
            let idx = (y as usize * size as usize + x as usize) * 4;
            pixels[idx] = darker.a;
            pixels[idx + 1] = darker.r;
            pixels[idx + 2] = darker.g;
            pixels[idx + 3] = darker.b;
        }
    }

    // Read up to 4 child entries and draw mini-icons in a 2x2 grid.
    if let Ok(entries) = fs::read_dir(path) {
        let children: Vec<_> = entries
            .filter_map(|e| e.ok())
            .take(FOLDER_PREVIEW_ITEMS)
            .collect();

        let cell = size / 4;
        let grid_x = size / 4;
        let grid_y = size / 4 + tab_h;

        for (i, entry) in children.iter().enumerate() {
            let col = (i % 2) as u32;
            let row = (i / 2) as u32;
            let cx = grid_x + col * (cell + 4);
            let cy = grid_y + row * (cell + 4);

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
            let icon_color = cat.accent_color();

            // Draw a small filled rectangle for this child item.
            let rect_size = cell.min(20);
            for py in cy..cy + rect_size {
                if py >= size {
                    break;
                }
                for px in cx..cx + rect_size {
                    if px >= size {
                        break;
                    }
                    let idx = (py as usize * size as usize + px as usize) * 4;
                    pixels[idx] = icon_color.a;
                    pixels[idx + 1] = icon_color.r;
                    pixels[idx + 2] = icon_color.g;
                    pixels[idx + 3] = icon_color.b;
                }
            }
        }
    }

    Thumbnail {
        width: size,
        height: size,
        pixels,
        source_path: path.to_string_lossy().to_string(),
        source_mtime: mtime,
    }
}

/// Generate a PDF placeholder thumbnail.
///
/// Red document icon with "PDF" text.  The page count is not determined here
/// (would require a full PDF parser); this is a recognizable placeholder.
fn generate_pdf_placeholder(path: &Path, config: &ThumbConfig, mtime: u64) -> Thumbnail {
    let size = config.size;
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];

    let red = ThumbCategory::Pdf.accent_color();
    let white = Color::WHITE;

    // Fill background white.
    for i in 0..(size as usize * size as usize) {
        pixels[i * 4] = white.a;
        pixels[i * 4 + 1] = white.r;
        pixels[i * 4 + 2] = white.g;
        pixels[i * 4 + 3] = white.b;
    }

    // Draw a red document rectangle (inset from edges).
    let margin = size / 8;
    for y in margin..size - margin {
        for x in margin..size - margin {
            let idx = (y as usize * size as usize + x as usize) * 4;
            pixels[idx] = red.a;
            pixels[idx + 1] = red.r;
            pixels[idx + 2] = red.g;
            pixels[idx + 3] = red.b;
        }
    }

    // Draw a white "dog ear" triangle in the top-right corner of the document.
    let ear_size = size / 6;
    let ear_x_start = size - margin - ear_size;
    let ear_y_end = margin + ear_size;
    for y in margin..ear_y_end {
        let row_offset = y - margin;
        let x_start = ear_x_start + row_offset;
        for x in x_start..size - margin {
            let idx = (y as usize * size as usize + x as usize) * 4;
            pixels[idx] = white.a;
            pixels[idx + 1] = white.r;
            pixels[idx + 2] = white.g;
            pixels[idx + 3] = white.b;
        }
    }

    // Draw "PDF" text as white pixels in the center region (simple block font).
    draw_block_text(&mut pixels, size, "PDF", white, size / 3, size / 2);

    Thumbnail {
        width: size,
        height: size,
        pixels,
        source_path: path.to_string_lossy().to_string(),
        source_mtime: mtime,
    }
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
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];

    let accent = category.accent_color();
    let bg = config.bg_color;

    // Fill background.
    for i in 0..(size as usize * size as usize) {
        pixels[i * 4] = bg.a;
        pixels[i * 4 + 1] = bg.r;
        pixels[i * 4 + 2] = bg.g;
        pixels[i * 4 + 3] = bg.b;
    }

    // Draw centered accent-colored circle.
    let cx = size / 2;
    let cy = size / 2;
    let radius = size / 3;
    for y in 0..size {
        for x in 0..size {
            let dx = x as i32 - cx as i32;
            let dy = y as i32 - cy as i32;
            if (dx * dx + dy * dy) <= (radius * radius) as i32 {
                let idx = (y as usize * size as usize + x as usize) * 4;
                pixels[idx] = accent.a;
                pixels[idx + 1] = accent.r;
                pixels[idx + 2] = accent.g;
                pixels[idx + 3] = accent.b;
            }
        }
    }

    Thumbnail {
        width: size,
        height: size,
        pixels,
        source_path: path.to_string_lossy().to_string(),
        source_mtime: mtime,
    }
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
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        _   => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
    }
}

/// Draw a string using the block font into ARGB pixel data.
///
/// `pixels` is a square buffer of `size x size` pixels (4 bytes per pixel,
/// ARGB order).  The text is drawn starting at pixel position `(x, y)` with
/// a 2x scale factor for visibility on thumbnail-sized images.
fn draw_block_text(pixels: &mut [u8], size: u32, text: &str, color: Color, x: u32, y: u32) {
    let scale = 2u32;
    let glyph_w = 5 * scale;
    let glyph_h = 7 * scale;
    let spacing = scale;

    let total_w = text.len() as u32 * (glyph_w + spacing);
    // Center horizontally around the given x.
    let start_x = x.saturating_sub(total_w / 2);
    // Center vertically around the given y.
    let start_y = y.saturating_sub(glyph_h / 2);

    for (ci, ch) in text.chars().enumerate() {
        let bitmap = glyph_bitmap(ch);
        let char_x = start_x + ci as u32 * (glyph_w + spacing);

        for row in 0..7u32 {
            let bits = bitmap[row as usize];
            for col in 0..5u32 {
                if bits & (1 << (4 - col)) != 0 {
                    // Draw a scale x scale block.
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = char_x + col * scale + sx;
                            let py = start_y + row * scale + sy;
                            if px < size && py < size {
                                let idx = (py as usize * size as usize + px as usize) * 4;
                                pixels[idx] = color.a;
                                pixels[idx + 1] = color.r;
                                pixels[idx + 2] = color.g;
                                pixels[idx + 3] = color.b;
                            }
                        }
                    }
                }
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
    fn cache_filename(&self, path: &str, mtime: u64) -> PathBuf {
        let hash = simple_hash(path, mtime);
        self.cache_dir.join(format!("{hash:016x}.thumb"))
    }

    /// Try to load a cached thumbnail from disk.
    pub fn load(&self, path: &str, mtime: u64) -> Option<Thumbnail> {
        let file_path = self.cache_filename(path, mtime);
        let data = fs::read(&file_path).ok()?;

        // Format: [width: 4 LE][height: 4 LE][ARGB pixel data...]
        if data.len() < 8 {
            return None;
        }
        let width = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let height = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let pixel_data = &data[8..];
        let expected = width as usize * height as usize * 4;
        if pixel_data.len() != expected {
            return None;
        }

        Some(Thumbnail {
            width,
            height,
            pixels: pixel_data.to_vec(),
            source_path: path.to_owned(),
            source_mtime: mtime,
        })
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
    pub fn remove(&self, path: &str, mtime: u64) {
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
    pub fn purge_stale(&self, valid_entries: &HashMap<String, u64>) -> std::io::Result<()> {
        if !self.cache_dir.is_dir() {
            return Ok(());
        }

        let valid_filenames: std::collections::HashSet<String> = valid_entries
            .iter()
            .map(|(path, mtime)| format!("{:016x}.thumb", simple_hash(path, *mtime)))
            .collect();

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".thumb") && !valid_filenames.contains(&name) {
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
        x: x + display_size / 2.0 - font_size * text.len() as f32 / 4.0,
        y: y + display_size / 2.0 - font_size / 2.0,
        text: text.to_owned(),
        color: Color::WHITE,
        font_size,
        font_weight: FontWeightHint::Bold,
        max_width: Some(display_size),
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
fn simple_hash(path: &str, mtime: u64) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis
    for byte in path.as_bytes() {
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
        assert_eq!(got.unwrap().source_path, "test.png");
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
        assert_eq!(ThumbCategory::from_extension("exe"), ThumbCategory::Executable);
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
            assert!(!cat.icon_label().is_empty(), "icon label empty for {:?}", cat);
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
        assert!(lines.len() >= 1);

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
        // Create a simple 4x4 ARGB image (all red).
        let w = 4u32;
        let h = 4u32;
        let mut src = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            src[i * 4] = 255;     // A
            src[i * 4 + 1] = 255; // R
            src[i * 4 + 2] = 0;   // G
            src[i * 4 + 3] = 0;   // B
        }

        let (dst, dw, dh) = box_filter_downscale(&src, w, h, 2);
        assert_eq!(dw, 2);
        assert_eq!(dh, 2);
        assert_eq!(dst.len(), (2 * 2 * 4) as usize);

        // Every pixel should still be red (uniform source).
        for i in 0..(dw * dh) as usize {
            assert_eq!(dst[i * 4], 255, "alpha");
            assert_eq!(dst[i * 4 + 1], 255, "red");
            assert_eq!(dst[i * 4 + 2], 0, "green");
            assert_eq!(dst[i * 4 + 3], 0, "blue");
        }
    }

    #[test]
    fn box_filter_downscale_empty() {
        let (dst, w, h) = box_filter_downscale(&[], 0, 0, 128);
        assert!(dst.is_empty());
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn box_filter_no_downscale_when_smaller() {
        let w = 10u32;
        let h = 10u32;
        let src = vec![128u8; (w * h * 4) as usize];
        let (dst, dw, dh) = box_filter_downscale(&src, w, h, 128);
        // Source already fits; should return original.
        assert_eq!(dw, w);
        assert_eq!(dh, h);
        assert_eq!(dst, src);
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
            source_path: String::new(),
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
        let h1 = simple_hash("/foo/bar.png", 12345);
        let h2 = simple_hash("/foo/bar.png", 12345);
        assert_eq!(h1, h2);
    }

    #[test]
    fn simple_hash_varies_with_mtime() {
        let h1 = simple_hash("/foo/bar.png", 100);
        let h2 = simple_hash("/foo/bar.png", 200);
        assert_ne!(h1, h2);
    }

    #[test]
    fn simple_hash_varies_with_path() {
        let h1 = simple_hash("/foo/bar.png", 100);
        let h2 = simple_hash("/foo/baz.png", 100);
        assert_ne!(h1, h2);
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
        let size = 64u32;
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        // Should not panic even with text near edges.
        draw_block_text(&mut pixels, size, "PDF", Color::WHITE, 32, 32);
        draw_block_text(&mut pixels, size, "123", Color::WHITE, 0, 0);
        draw_block_text(&mut pixels, size, "999", Color::WHITE, 63, 63);
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
            source_path: String::new(),
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

        let loaded = cache.load("test_disk.png", thumb.source_mtime).unwrap();
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
        assert!(cache.load("miss.png", thumb.source_mtime + 1).is_none());

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
            source_path: name.to_owned(),
            source_mtime: 42,
        }
    }
}
