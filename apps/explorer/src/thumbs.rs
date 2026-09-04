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

/// Most pixels a source picture may have before the thumbnailer declines to
/// decode it and falls back to the aspect-ratio swatch.
///
/// Deliberately well below `imagecodec::Limits::DEFAULT_MAX_PIXELS` (7680×4320,
/// the compositor's own buffer ceiling), because the two are bounding different
/// things. That ceiling asks "could this be a wallpaper?" — one picture, chosen
/// by the user, decoded when they choose it. This one asks "should a directory
/// listing decode this?", and a directory listing decodes whatever is in the
/// directory, without being asked, while the user waits for the folder to open.
///
/// 24 megapixels is a 6000×4000 full-frame photograph, which is what the
/// overwhelming majority of picture files on a desktop actually are. The cost
/// of the ones above it is a swatch — exactly what *every* PNG got before this
/// crate could decode at all — so nothing regresses at the boundary.
///
/// **Why a cap is needed at all**, and what would remove it: `imagecodec` has
/// no scaled or partial decode, so producing a 128×128 thumbnail costs a
/// full-size decode. At this cap the transient peak is roughly 190 MB (the
/// inflate output and the pixel buffer are both live inside `decode`), held for
/// the milliseconds between decoding one picture and downscaling it, and one at
/// a time because generation is sequential. A decoder that box-filtered *during*
/// scanline reconstruction would never materialise the full picture and this
/// constant could go away. See known-issues.md
/// `TD-C-A-THUMBNAIL-COSTS-A-FULL-SIZE-DECODE`.
const DEFAULT_MAX_SOURCE_PIXELS: u64 = 24_000_000;

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

    /// The pixels in the byte order an upload to the compositor must carry, or
    /// `None` if `pixels` does not match `width` × `height`.
    ///
    /// **This is a real conversion, not a formality.** `pixels` is `A, R, G, B`
    /// — the order [`Canvas::to_argb`] writes and the disk cache stores —
    /// whereas `BufferFormat::Argb8888` is `B, G, R, A`, a little-endian `u32`
    /// of `0xAARRGGBB`. Both are called "ARGB". Handing the compositor the
    /// stored bytes unconverted is neither a compile error nor a panic: every
    /// thumbnail would come back with red and blue exchanged and its alpha read
    /// from the blue channel, which for an opaque photograph means a picture
    /// that is mostly transparent and wrongly coloured.
    ///
    /// Routed through `Canvas` rather than reversing each four bytes in place,
    /// even though that is what the answer amounts to. The two byte orders are
    /// facts about a disk format and a wire format respectively, and `Canvas`
    /// is the one place either is written down; a hand-rolled reverse here
    /// would be a third statement of the same fact, free to drift from both.
    /// It also gets the length check for nothing.
    #[must_use]
    pub fn to_wire_bytes(&self) -> Option<Vec<u8>> {
        Canvas::from_argb(self.width, self.height, &self.pixels).map(|c| c.to_argb8888())
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
///
/// The path is a `PathBuf`, not a `String`, for the same reason
/// [`Thumbnail::source_path`] is: a caller holding a `Path` can only reach a
/// `String` through `to_string_lossy`, and two names differing only in bytes
/// that are not UTF-8 collapse to one key — so one file is shown the other's
/// thumbnail. The disk cache was fixed for this; the in-memory cache sitting
/// in front of it had the identical hole, and a caller passing a lossy string
/// to both would have hit the memory one first.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    path: PathBuf,
    mtime: u64,
    size: u64,
}

impl CacheKey {
    fn new(path: impl AsRef<Path>, mtime: u64, size: u64) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            mtime,
            size,
        }
    }

    /// The compositor image id for the thumbnail this key holds.
    fn image_id(&self) -> u64 {
        image_id(&self.path, self.mtime, self.size)
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
    /// Image ids of thumbnails that have left the cache since last asked.
    ///
    /// See [`Self::take_evicted_image_ids`]. Recorded rather than acted on
    /// because this type holds no compositor connection and should not: it is
    /// a cache, and the connection belongs to whatever is hosting the window.
    evicted: Vec<u64>,
}

impl ThumbnailCache {
    /// Create a new cache with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            evicted: Vec::new(),
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
    pub fn get(&mut self, path: impl AsRef<Path>, mtime: u64, size: u64) -> Option<&Thumbnail> {
        let key = CacheKey::new(path, mtime, size);
        if self.map.contains_key(&key) {
            self.promote(&key);
            self.map.get(&key)
        } else {
            None
        }
    }

    /// Look up a thumbnail **without** promoting it.
    ///
    /// This is what a renderer wants. Drawing a frame must not be able to
    /// change which entries survive eviction: with [`Self::get`], scrolling a
    /// folder of ten thousand files past a five-hundred-entry cache would make
    /// eviction order follow the last frame drawn rather than the user's
    /// actual attention, and a renderer holding `&self` cannot call it anyway.
    pub fn peek(&self, path: impl AsRef<Path>, mtime: u64, size: u64) -> Option<&Thumbnail> {
        self.map.get(&CacheKey::new(path, mtime, size))
    }

    /// Insert (or replace) a thumbnail.  Evicts the LRU entry when full.
    pub fn insert(&mut self, path: impl AsRef<Path>, mtime: u64, size: u64, thumb: Thumbnail) {
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
    pub fn invalidate(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        let keys_to_remove: Vec<CacheKey> = self
            .map
            .keys()
            .filter(|k| k.path == path)
            .cloned()
            .collect();

        for key in &keys_to_remove {
            self.note_removed(key);
            self.remove_from_order(key);
        }
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        for key in self.map.keys() {
            self.evicted.push(key.image_id());
        }
        self.map.clear();
        self.order.clear();
    }

    /// The image ids of thumbnails that have left this cache since the last
    /// call, and clear the record.
    ///
    /// **This is what keeps the compositor's memory bounded.** Nothing evicts
    /// on the compositor's side — it holds what a client gives it until the
    /// client gives it back (design-decisions.md §556) — so a file manager that
    /// uploaded a thumbnail per file and never dropped one would grow its
    /// held-image total for as long as the user kept browsing, and would
    /// eventually be refused an upload with no way to make room.
    ///
    /// Mirroring *this* cache is the policy rather than inventing a second one:
    /// the cache is already bounded, already has an eviction order, and is
    /// already what the renderer reads — so an entry that has left it cannot be
    /// drawn anyway, and its pixels on the compositor are dead weight by
    /// definition. Two eviction policies for one set of pictures would be two
    /// things to keep in agreement, and the disagreement would show up as
    /// either a leak or a blank cell.
    ///
    /// Draining, not peeking, on the same terms as `App::take_images`: the
    /// caller sends what comes back, so a record that is not cleared re-sends
    /// the same drop every frame.
    pub fn take_evicted_image_ids(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.evicted)
    }

    /// Drop `key` from the map, recording the image id it took with it.
    ///
    /// Only records when something was actually removed: a drop for an id the
    /// compositor never held is not harmless, it is an id that may since have
    /// been re-uploaded by a later insert of the same file.
    fn note_removed(&mut self, key: &CacheKey) {
        if self.map.remove(key).is_some() {
            self.evicted.push(key.image_id());
        }
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
            self.note_removed(&lru_key);
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

/// Parse BMP header to extract dimensions.
///
/// BMP files start with `BM`, and the BITMAPINFOHEADER at offset 14 contains
/// width (LE i32 at +4) and height (LE i32 at +8, may be negative for
/// top-down bitmaps).
fn parse_bmp_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    if !byteread::starts_with(data, b"BM") {
        return None;
    }
    let width = byteread::i32_le_at(data, 18)?;
    // A negative height means a top-down bitmap; `i32::MIN` has no positive
    // counterpart, so take the magnitude as a `u32` rather than negating.
    let height = byteread::i32_le_at(data, 22)?.unsigned_abs();
    if width <= 0 || height == 0 {
        return None;
    }
    Some(ImageDimensions {
        width: width.unsigned_abs(),
        height,
    })
}

/// Parse PNG header to extract dimensions.
///
/// Delegated to the decoder rather than read here. This used to be two
/// `u32_be_at` calls at offsets 16 and 20 — the right offsets for a *valid*
/// PNG, and unchecked for everything else, so a file that began with the eight
/// magic bytes and then went wrong reported whatever integers happened to sit
/// there. `imagecodec::png::dimensions` reads the IHDR as a chunk: it checks
/// the length field, the chunk type, the CRC's presence, and the bit
/// depth/colour-type combination, so a size it returns is one the picture
/// actually has.
///
/// It also means the icon view cannot disagree with the image viewer about how
/// big a picture is, which is the same argument that put the decoder in one
/// crate rather than one per caller (design-decisions.md §555).
fn parse_png_dimensions(data: &[u8]) -> Option<ImageDimensions> {
    let (width, height) = imagecodec::png::dimensions(data).ok()?;
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
    let width = byteread::u16_le_at(data, 6)? as u32;
    let height = byteread::u16_le_at(data, 8)? as u32;
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

    let mut pos: usize = 2;
    while pos.saturating_add(1) < data.len() {
        if *data.get(pos)? != 0xFF {
            pos = pos.saturating_add(1);
            continue;
        }
        let marker = *data.get(pos.checked_add(1)?)?;
        pos = pos.checked_add(2)?;

        // Skip padding 0xFF bytes.
        if marker == 0xFF || marker == 0x00 {
            continue;
        }
        // Restart markers and standalone markers have no payload.
        if (0xD0..=0xD9).contains(&marker) {
            continue;
        }

        if pos.saturating_add(2) > data.len() {
            return None;
        }
        let seg_len = byteread::u16_be_at(data, pos)? as usize;
        if seg_len < 2 {
            return None;
        }

        // SOF0 (baseline), SOF1 (extended sequential), SOF2 (progressive)
        if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
            if pos.saturating_add(7) > data.len() {
                return None;
            }
            let height = byteread::u16_be_at(data, pos.checked_add(3)?)? as u32;
            let width = byteread::u16_be_at(data, pos.checked_add(5)?)? as u32;
            if width == 0 || height == 0 {
                return None;
            }
            return Some(ImageDimensions { width, height });
        }

        // `seg_len >= 2` is enforced above, so this always advances — which
        // is what stops a crafted JPEG spinning here forever.
        pos = pos.checked_add(seg_len)?;
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
        let new_h = u64::from(h)
            .saturating_mul(u64::from(max_size))
            .checked_div(u64::from(w))
            .unwrap_or(1)
            .max(1) as u32;
        (max_size, new_h)
    } else {
        let new_w = u64::from(w)
            .saturating_mul(u64::from(max_size))
            .checked_div(u64::from(h))
            .unwrap_or(1)
            .max(1) as u32;
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
    /// Most pixels a source picture may have before it is declined rather than
    /// decoded. See [`DEFAULT_MAX_SOURCE_PIXELS`] for why this is not simply
    /// the decoder's own default.
    ///
    /// Configurable rather than a constant because the right answer depends on
    /// the machine: a workstation opening a photographer's directory can afford
    /// what a low-memory device cannot, and the failure mode of guessing high
    /// is an out-of-memory kill of the file manager.
    pub max_source_pixels: u64,
}

impl Default for ThumbConfig {
    fn default() -> Self {
        Self {
            size: DEFAULT_THUMB_SIZE,
            bg_color: Color::rgb(245, 245, 245),
            text_color: Color::rgb(100, 100, 100),
            max_source_pixels: DEFAULT_MAX_SOURCE_PIXELS,
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
/// Three outcomes, in preference order: the picture itself, downscaled; an
/// aspect-ratio-correct colour swatch, for a format no decoder here reads; and
/// the category placeholder, for a file whose header says nothing at all.
///
/// The middle one used to be the outcome for *every* compressed format,
/// including PNG, which is what `TD-C-NOTHING-DECODES-A-PICTURE-SO-EVERY-IMAGE
/// -ID-NAMES-NOTHING` was about: only uncompressed BMP had a real thumbnail,
/// and a directory of photographs was a grid of identical green rectangles
/// differing only in shape.
fn generate_image_thumbnail(path: &Path, config: &ThumbConfig, mtime: u64) -> Thumbnail {
    let header = match read_file_header(path, 1024) {
        Some(h) => h,
        None => return generate_default_thumbnail(path, ThumbCategory::Image, config, mtime),
    };

    let dims = match parse_image_dimensions(&header) {
        Some(d) => d,
        None => return generate_default_thumbnail(path, ThumbCategory::Image, config, mtime),
    };

    // Dispatched on the signature rather than tried in turn, because either
    // branch reads the whole file: offering the file to a decoder that will
    // reject it on its first eight bytes still costs the read that got those
    // eight bytes there.
    if header.starts_with(b"BM") {
        // BMP is the one format `imagecodec` does not read, and the one this
        // module already decoded: uncompressed 24/32-bit, straight out of the
        // file with no decompressor in the way.
        if let Some(thumb) = try_bmp_thumbnail(path, dims, config, mtime) {
            return thumb;
        }
    } else if let Some(thumb) = try_decoded_thumbnail(path, dims, config, mtime) {
        return thumb;
    }

    // Nothing decoded it: an aspect-ratio-correct colour swatch, which is at
    // least honest about the shape of the picture. Today this is GIF, JPEG,
    // SVG, WebP and ICO — and any PNG above `max_source_pixels`, or one that is
    // corrupt.
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

/// Decode the picture properly and downscale it, for the formats `imagecodec`
/// reads (today: PNG).
///
/// `None` — never an error — for a picture too large, a file that is not a
/// format the decoder claims, or one that is corrupt. All three are the same
/// thing from a directory listing's point of view: this entry does not get a
/// picture, and the caller's swatch is what it gets instead. A file manager
/// that reported a decode failure per file would produce a wall of dialogs for
/// one bad download.
///
/// `dims` is the header's answer and is used only to decline early; the
/// decoder's own answer is what the canvas is built from. They agree for
/// anything that decodes, since `imagecodec` sizes its buffers from the same
/// header — but the check has to happen before the decode, which is the whole
/// point of having it.
fn try_decoded_thumbnail(
    path: &Path,
    dims: ImageDimensions,
    config: &ThumbConfig,
    mtime: u64,
) -> Option<Thumbnail> {
    // Checked against the header we already have, before the file is read at
    // all. `imagecodec` would refuse the same picture from its own header a
    // moment later, but that moment costs a full read of a file that may be
    // hundreds of megabytes.
    let source_pixels = u64::from(dims.width).checked_mul(u64::from(dims.height))?;
    if source_pixels > config.max_source_pixels {
        return None;
    }

    let data = fs::read(path).ok()?;
    let limits = imagecodec::Limits {
        max_pixels: config.max_source_pixels,
        // The decompressed *byte* ceiling, kept in the same proportion the
        // crate's own default uses (16 bytes per pixel), which is what a
        // 16-bit-per-sample RGBA image costs before it is reduced to the
        // 4-bytes-per-pixel output. Deriving it from `max_pixels` rather than
        // repeating a number keeps the two from drifting apart.
        max_decompressed_bytes: usize::try_from(config.max_source_pixels.saturating_mul(16))
            .unwrap_or(usize::MAX),
    };
    let image = imagecodec::decode(&data, limits).ok()?;
    // Dropped before the pixel buffer is converted: for a 24-megapixel PNG this
    // is tens of megabytes of compressed data with no further reader, and the
    // conversion below is the peak of this function.
    drop(data);

    let (width, height) = (image.width, image.height);
    // Consuming the decoded pixels rather than borrowing them, so the picture
    // exists in one buffer and not two. `Image::to_argb_bytes` would have made
    // a third: a `Vec<u8>` between the `Vec<u32>` the decoder produced and the
    // `Vec<Color>` a canvas holds, all three full-size and all three alive at
    // once.
    let pixels: Vec<Color> = image.pixels.into_iter().map(argb_to_color).collect();
    let canvas = Canvas::from_pixels(width, height, pixels)?;

    Some(into_thumbnail(
        box_filter_downscale(&canvas, config.size),
        path,
        mtime,
    ))
}

/// One `0xAARRGGBB` word, as the toolkit's colour.
///
/// The decoder's output format is the compositor's storage format, which is a
/// packed word; the toolkit's is four fields. Neither is wrong and the
/// conversion is a shift, but it is written once here rather than inline so
/// that a future channel-order question has one place to be asked.
const fn argb_to_color(px: u32) -> Color {
    Color::rgba(
        ((px >> 16) & 0xFF) as u8,
        ((px >> 8) & 0xFF) as u8,
        (px & 0xFF) as u8,
        ((px >> 24) & 0xFF) as u8,
    )
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

    let offset = byteread::u32_le_at(&data, 10)? as usize;
    let bits_per_pixel = byteread::u16_le_at(&data, 28)?;
    let compression = byteread::u32_le_at(&data, 30)?;

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
    let bottom_up = byteread::i32_le_at(&data, 22)? > 0;

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
        // Line width proportional to character count, capped at thumbnail
        // width. Characters, not `line.len()`, which is the UTF-8 *byte*
        // count: a minimap says "this line is about this long", and a line of
        // Japanese encodes to three bytes a character, so measured in bytes it
        // drew a bar three times too long — every line in a CJK file pinned to
        // the cap, making a ragged file look like a solid block. There is no
        // font in this path to measure in (the thumbnail is a synthetic
        // minimap, not rendered text), so a character count is the honest
        // proxy for "how much text"; the byte count is not a proxy for
        // anything a reader can see.
        let max_chars = (content / 2) as usize;
        let bar_len = (line.chars().count().min(max_chars) as u32).saturating_mul(2);
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
            for col in 0..5u8 {
                // Bit 4 is the leftmost column of the 5-wide glyph.
                if bits & (1u8 << (4u8.saturating_sub(col))) == 0 {
                    continue;
                }
                let px = char_x.saturating_add(u32::from(col).saturating_mul(SCALE));
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
///
/// # The disk cache belongs here
///
/// A generator built with [`with_disk_cache`](Self::with_disk_cache) reads the
/// cache before generating and writes it after. That layering lives inside the
/// generator rather than at the call site because "how a thumbnail comes into
/// being" is the one thing this type is for: a caller that had to remember to
/// probe the disk first would be a caller that could forget, and the cost of
/// forgetting is re-decoding every file in the folder on every restart —
/// silently, since the result is correct either way.
pub struct ThumbnailGenerator {
    /// Pending requests (FIFO).
    pending: VecDeque<ThumbnailRequest>,
    /// Completed thumbnails ready for the caller.
    completed: Vec<(ThumbnailRequest, Thumbnail)>,
    /// Where a generated thumbnail is kept across restarts, if anywhere.
    ///
    /// `None` is a working configuration, not a degraded one: a session with no
    /// home directory, or a test that must not touch the user's cache,
    /// generates every time and is otherwise identical.
    disk: Option<DiskCache>,
}

impl ThumbnailGenerator {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            completed: Vec::new(),
            disk: None,
        }
    }

    /// A generator that persists what it makes, and reuses what it finds.
    #[must_use]
    pub fn with_disk_cache(disk: DiskCache) -> Self {
        Self {
            pending: VecDeque::new(),
            completed: Vec::new(),
            disk: Some(disk),
        }
    }

    /// A generator backed by the default cache directory, or an unbacked one
    /// if there is no home directory to put it in.
    #[must_use]
    pub fn with_default_disk_cache() -> Self {
        DiskCache::default_location().map_or_else(Self::new, Self::with_disk_cache)
    }

    /// The disk cache this generator reads and writes, if it has one.
    #[must_use]
    pub const fn disk_cache(&self) -> Option<&DiskCache> {
        self.disk.as_ref()
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
    /// Returns the number of requests retired this call — whether each one was
    /// read from the disk cache or generated afresh, since to the caller
    /// budgeting a frame the two are the same unit of work retired, and it is
    /// the *queue* draining that the number is used to watch.
    ///
    /// The disk lookup is keyed on the request's [`ThumbConfig::size`] as well
    /// as its path and mtime, so a user who changes the thumbnail size gets
    /// regeneration rather than yesterday's smaller picture scaled up.
    ///
    /// A failed save is ignored deliberately. The thumbnail is in hand and the
    /// view is about to draw it; a full or read-only cache directory is a
    /// reason to regenerate next time, not a reason to fail now.
    pub fn process_batch(&mut self, batch_size: usize) -> usize {
        let mut processed: usize = 0;
        for _ in 0..batch_size {
            let req = match self.pending.pop_front() {
                Some(r) => r,
                None => break,
            };
            let cap = req.config.size;
            let cached = self
                .disk
                .as_ref()
                .and_then(|d| d.load(&req.path, req.mtime, cap));
            let thumb = match cached {
                Some(t) => t,
                None => {
                    let t = generate_thumbnail(&req.path, &req.config);
                    if let Some(disk) = self.disk.as_ref() {
                        let _ = disk.save(&t, cap);
                    }
                    t
                }
            };
            self.completed.push((req, thumb));
            processed = processed.saturating_add(1);
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

    /// Compute the cache filename for a given path, mtime and size cap.
    ///
    /// The cap is part of the name, not merely of the contents, because
    /// [`fit_dimensions`] does not upscale: a 20x20 source yields a 20x20
    /// thumbnail at *every* cap, so the stored dimensions cannot be compared
    /// against the cap in force to tell a valid entry from a stale one. Keyed
    /// on the cap, a size change simply misses and regenerates, and entries at
    /// the old cap are collected by [`Self::purge_stale`] rather than served.
    fn cache_filename(&self, path: &Path, mtime: u64, cap: u32) -> PathBuf {
        let hash = simple_hash(path, mtime);
        self.cache_dir.join(format!("{hash:016x}-{cap}.thumb"))
    }

    /// Try to load a cached thumbnail from disk, made at the `cap` now in
    /// force.
    pub fn load(&self, path: &Path, mtime: u64, cap: u32) -> Option<Thumbnail> {
        let file_path = self.cache_filename(path, mtime, cap);
        let data = fs::read(&file_path).ok()?;

        // Format: [width: 4 LE][height: 4 LE][ARGB pixel data...]
        //
        // The header is the cache file's own claim about its contents, and the
        // cache directory is a plain directory in the user's home — nothing
        // stops a file there from claiming dimensions its pixel data does not
        // match. `Canvas::from_argb` is the check, and it is the same one every
        // other route into a `Canvas` goes through.
        let width = u32::from_le_bytes(byteread::array_at(&data, 0)?);
        let height = u32::from_le_bytes(byteread::array_at(&data, 4)?);
        let pixel_data = data.get(8..)?;
        Some(into_thumbnail(
            Canvas::from_argb(width, height, pixel_data)?,
            path,
            mtime,
        ))
    }

    /// Save a thumbnail to the disk cache, recorded as having been made at
    /// `cap`.
    pub fn save(&self, thumb: &Thumbnail, cap: u32) -> std::io::Result<()> {
        self.ensure_dir()?;
        let file_path = self.cache_filename(&thumb.source_path, thumb.source_mtime, cap);

        let mut data = Vec::with_capacity(thumb.pixels.len().saturating_add(8));
        data.extend_from_slice(&thumb.width.to_le_bytes());
        data.extend_from_slice(&thumb.height.to_le_bytes());
        data.extend_from_slice(&thumb.pixels);
        fs::write(file_path, &data)
    }

    /// Remove the cached thumbnail for a specific path/mtime/cap.
    pub fn remove(&self, path: &Path, mtime: u64, cap: u32) {
        let file_path = self.cache_filename(path, mtime, cap);
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

    /// Purge entries whose source file no longer exists at the recorded mtime.
    ///
    /// Since the cache filename is a hash (not the original path), this method
    /// requires scanning the in-memory cache for paths.  Pass the set of
    /// known-valid source paths; anything in the cache directory that doesn't
    /// correspond to a valid entry is removed.
    ///
    /// Matching is on the *hash* part of the name, not the whole of it, so a
    /// live file's entries are kept at every size cap they were made at rather
    /// than only at the one currently in force. A cap change should cost a
    /// regeneration, not a purge of every other size a second window might
    /// still be drawing from.
    pub fn purge_stale(&self, valid_entries: &HashMap<PathBuf, u64>) -> std::io::Result<()> {
        if !self.cache_dir.is_dir() {
            return Ok(());
        }

        let valid_prefixes: std::collections::HashSet<String> = valid_entries
            .iter()
            .map(|(path, mtime)| format!("{:016x}", simple_hash(path, *mtime)))
            .collect();

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            // Compared as bytes. Every name we write is
            // `{hash:016x}-{cap}.thumb`, so a name that is not UTF-8 is not one
            // of ours; rendering it lossily first could make it *look* like one
            // of ours and get it deleted.
            let raw = entry.file_name();
            let name = raw.as_encoded_bytes();
            if !name.ends_with(b".thumb") {
                continue;
            }
            // The hash is the fixed-width run before the first `-`. A name of
            // ours always has one; a `.thumb` file that does not is not ours
            // and is left alone, for the same reason the UTF-8 case is.
            let Some(sep) = name.iter().position(|&b| b == b'-') else {
                continue;
            };
            let Some(prefix) = name.get(..sep) else {
                continue;
            };
            if !valid_prefixes.iter().any(|v| v.as_bytes() == prefix) {
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
///
/// `image_id` is passed in rather than derived from `thumb`, because the id
/// identifies the *file* the pixels came from — path, mtime and length — and a
/// [`Thumbnail`] knows none of those. Deriving it here from what the thumbnail
/// does know would give two different files with the same dimensions the same
/// id. Use [`image_id`] with the same three facts the cache was keyed on.
pub fn render_thumbnail(
    thumb: &Thumbnail,
    image_id: u64,
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

    // The actual thumbnail image. Drawing an id the compositor holds no pixels
    // for renders nothing, silently and by design, so the caller must only ask
    // for this once it has uploaded them (see `ExplorerState::drawable_thumb`).
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

/// The compositor image id for the thumbnail of `path` as it was at `mtime`
/// and `size`.
///
/// **The same three facts the in-memory cache is keyed on, and deliberately
/// so.** This took only the path and the mtime once, which is one fact short:
/// a file rewritten twice inside the same second keeps its modification time
/// and changes its length, so the two versions were two distinct cache entries
/// sharing one image id. Evicting either dropped the pixels the other was
/// drawing with, and uploading either replaced the other's picture — one file
/// showing a stale version of itself, which is the same shape of bug as the
/// lossy-path-string collision `Thumbnail::source_path` documents.
///
/// Not the same hash the *disk* cache names its files with
/// ([`simple_hash`]), which is a filename and not an identity; that one is
/// left alone so an existing on-disk cache is not silently orphaned.
#[must_use]
pub fn image_id(path: &Path, mtime: u64, size: u64) -> u64 {
    let mut hash = simple_hash(path, mtime);
    for byte in size.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3); // FNV prime
    }
    hash
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
    use scratchdir::ScratchDir;
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

    /// The renderer reads the cache on every frame, so if reading promoted, the
    /// eviction order would be "whatever was last on screen" rather than
    /// "whatever was last *wanted*" — and a file scrolled past would outlive
    /// one the user actually opened. `peek` exists to make that impossible;
    /// `render` taking `&self` is the same fact enforced by the compiler.
    #[test]
    fn peeking_does_not_promote_so_drawing_cannot_reorder_the_cache() {
        let mut cache = ThumbnailCache::new(3);
        cache.insert("a", 1, 10, make_test_thumb("a", 10));
        cache.insert("b", 2, 20, make_test_thumb("b", 10));
        cache.insert("c", 3, 30, make_test_thumb("c", 10));

        // The one difference from `cache_promotes_on_get`.
        assert!(cache.peek("a", 1, 10).is_some());
        cache.insert("d", 4, 40, make_test_thumb("d", 10));

        assert!(
            cache.peek("a", 1, 10).is_none(),
            "still the LRU, so evicted"
        );
        assert!(cache.peek("b", 2, 20).is_some());
    }

    /// `to_string_lossy` maps every undecodable unit to the same replacement
    /// character, so two distinct filenames that differ only in such units
    /// would share one cache key — and one file would be shown the other's
    /// picture. The key is a `PathBuf` for exactly this reason.
    ///
    /// The sibling
    /// `two_names_differing_only_in_undecodable_units_do_not_share_a_cache_filename`
    /// asserts the same property of the *disk* cache, which keys on
    /// [`simple_hash`] rather than on `PathBuf`'s own `Hash`; neither implies
    /// the other.
    #[test]
    fn two_names_that_lossy_conversion_would_merge_stay_distinct_in_memory() {
        let (one, two) = undecodable_pair();
        assert_eq!(
            one.to_string_lossy(),
            two.to_string_lossy(),
            "the premise: these collapse to the same string"
        );
        assert_ne!(one, two, "but they are different paths");

        let mut cache = ThumbnailCache::new(4);
        cache.insert(&one, 7, 70, make_test_thumb("one", 10));
        cache.insert(&two, 7, 70, make_test_thumb("two", 12));

        assert_eq!(cache.len(), 2, "two files, two entries");
        assert_eq!(cache.peek(&one, 7, 70).expect("one").width, 10);
        assert_eq!(cache.peek(&two, 7, 70).expect("two").width, 12);
    }

    /// Two paths that are different byte-for-byte but identical after a lossy
    /// UTF-8 conversion. Built in memory, never created on disk: the cache is a
    /// pure map, and the filesystems this must be correct on are not all
    /// willing to create such a name.
    #[cfg(unix)]
    fn undecodable_pair() -> (PathBuf, PathBuf) {
        use std::os::unix::ffi::OsStrExt;
        let make = |b: u8| PathBuf::from(std::ffi::OsStr::from_bytes(&[b'x', b]));
        (make(0xE9), make(0xEA))
    }

    #[cfg(windows)]
    fn undecodable_pair() -> (PathBuf, PathBuf) {
        use std::os::windows::ffi::OsStringExt;
        // Unpaired surrogates: valid UTF-16 code units, no UTF-8 spelling.
        let make = |u: u16| PathBuf::from(std::ffi::OsString::from_wide(&[u16::from(b'x'), u]));
        (make(0xD800), make(0xD801))
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

    // -- What leaves the cache must leave the compositor --------------------

    /// The eviction bookkeeping the compositor's memory bound rests on: every
    /// route out of the cache records the id that went with it, because the
    /// compositor never evicts on its own and a route that forgot would leak
    /// that thumbnail's pixels for the life of the window.
    #[test]
    fn every_way_out_of_the_cache_records_the_id_that_left_with_it() {
        let evicted_by = |f: &dyn Fn(&mut ThumbnailCache)| {
            let mut cache = ThumbnailCache::new(2);
            cache.insert("a", 1, 10, make_test_thumb("a", 8));
            cache.insert("b", 2, 20, make_test_thumb("b", 8));
            f(&mut cache);
            cache.take_evicted_image_ids()
        };

        let id_a = image_id(Path::new("a"), 1, 10);
        let id_b = image_id(Path::new("b"), 2, 20);

        // Falling off the end of the LRU order.
        assert_eq!(
            evicted_by(&|c| c.insert("c", 3, 30, make_test_thumb("c", 8))),
            vec![id_a]
        );
        // Named explicitly, whatever its mtime and size.
        assert_eq!(evicted_by(&|c| c.invalidate("b")), vec![id_b]);
        // Cleared wholesale — order within one clear is unspecified, so sort.
        let mut all = evicted_by(&ThumbnailCache::clear);
        all.sort_unstable();
        let mut want = vec![id_a, id_b];
        want.sort_unstable();
        assert_eq!(all, want);
    }

    /// Draining, not peeking: `App::take_images` sends whatever comes back, so
    /// a record that survived the call would re-send the same drop every frame
    /// — and a drop re-sent after the file was thumbnailed again would take the
    /// *new* pixels down with it.
    #[test]
    fn taking_the_evicted_ids_clears_them() {
        let mut cache = ThumbnailCache::new(1);
        cache.insert("a", 1, 10, make_test_thumb("a", 8));
        cache.insert("b", 2, 20, make_test_thumb("b", 8));
        assert_eq!(cache.take_evicted_image_ids().len(), 1);
        assert!(cache.take_evicted_image_ids().is_empty());
    }

    /// Removing something that was never there records nothing. `invalidate`
    /// takes a path and removes every version of it, so it is routinely called
    /// for files the cache never held; emitting a drop for each would be a
    /// stream of ids the compositor has no pixels for — and one of them could
    /// later name a picture that *had* since been uploaded.
    #[test]
    fn removing_an_absent_entry_records_no_eviction() {
        let mut cache = ThumbnailCache::new(4);
        cache.insert("a", 1, 10, make_test_thumb("a", 8));
        cache.invalidate("never-cached");
        assert!(cache.take_evicted_image_ids().is_empty());
    }

    /// The reason the id carries the file's *length* as well as its path and
    /// modification time. A file rewritten twice inside one second keeps its
    /// mtime, so path+mtime alone gave two genuinely distinct cache entries one
    /// id: evicting either dropped the pixels the other was drawing with, and
    /// uploading either replaced the other's picture.
    #[test]
    fn two_versions_of_a_file_written_in_the_same_second_get_different_ids() {
        let short = image_id(Path::new("/notes.png"), 1_700_000_000, 4_096);
        let long = image_id(Path::new("/notes.png"), 1_700_000_000, 8_192);
        assert_ne!(short, long);

        // And the three facts still each move it on their own.
        assert_ne!(
            short,
            image_id(Path::new("/other.png"), 1_700_000_000, 4_096)
        );
        assert_ne!(
            short,
            image_id(Path::new("/notes.png"), 1_700_000_001, 4_096)
        );

        // Same three facts, same id — the cache key and the renderer derive it
        // separately and must agree.
        assert_eq!(
            short,
            image_id(Path::new("/notes.png"), 1_700_000_000, 4_096)
        );
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
        let scratch = ScratchDir::new("thumbs_test_text");
        let file_path = scratch.path("long.txt");

        {
            let mut f = fs::File::create(&file_path).unwrap();
            for i in 0..50 {
                writeln!(f, "Line {i}: some content here").unwrap();
            }
        }

        let lines = read_text_lines(&file_path, TEXT_PREVIEW_MAX_LINES).unwrap();
        assert!(lines.len() <= TEXT_PREVIEW_MAX_LINES);
        assert!(!lines.is_empty());
    }

    /// The minimap's bars say how long each line is, so two files whose lines
    /// are the same length must draw the same bars whatever alphabet they are
    /// written in. Measured in bytes — as this did — the Japanese file's lines
    /// counted three times over and every bar hit the cap, turning a ragged
    /// file into a solid block.
    #[test]
    fn a_minimap_bar_is_as_long_as_the_line_not_as_its_encoding() {
        let scratch = ScratchDir::new("thumbs_test_minimap");
        let latin_path = scratch.path("latin.txt");
        let cjk_path = scratch.path("cjk.txt");

        // Ten characters per line in both files, and short enough that neither
        // is capped — the cap is what hid this, by making every long-enough
        // line look the same.
        {
            let mut f = fs::File::create(&latin_path).unwrap();
            for _ in 0..3 {
                writeln!(f, "abcdefghij").unwrap();
            }
            let mut f = fs::File::create(&cjk_path).unwrap();
            for _ in 0..3 {
                writeln!(f, "あいうえおかきくけこ").unwrap();
            }
        }

        let config = ThumbConfig::default();
        let latin = generate_text_thumbnail(&latin_path, &config, 1);
        let cjk = generate_text_thumbnail(&cjk_path, &config, 1);
        assert_eq!(
            latin.pixels, cjk.pixels,
            "ten characters is ten characters in both files"
        );
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
    fn the_one_height_that_has_no_positive_counterpart_is_still_just_declined() {
        // A top-down BMP stores its height negated, so reading it means taking
        // a magnitude -- and `i32::MIN.abs()` panics, because +2147483648 is
        // not an i32. `unsigned_abs` returns it as the u32 it fits in. The
        // dimension is then rejected for being far too large by the caller,
        // which is what should have happened all along; the point is that it
        // is rejected rather than aborting the file manager.
        let mut header = vec![0u8; 54];
        header[0] = b'B';
        header[1] = b'M';
        header[18..22].copy_from_slice(&100u32.to_le_bytes());
        header[22..26].copy_from_slice(&i32::MIN.to_le_bytes());

        let dims = parse_bmp_dimensions(&header).expect("a magnitude, not a panic");
        assert_eq!(dims.width, 100);
        assert_eq!(dims.height, 2_147_483_648);
    }

    #[test]
    fn parse_png_valid() {
        // A genuine PNG rather than a 24-byte stub with the size poked into
        // it. The size now comes back through `imagecodec::png::dimensions`,
        // which reads the IHDR *as a chunk* -- its length, its type, its CRC,
        // and the bit depth and colour type after the size -- so a stub whose
        // remaining fields are zero is not a picture and reports nothing. That
        // is the decoder being right, not the test being unlucky: a file the
        // decoder would refuse should not be listed with a size.
        let dims = parse_png_dimensions(&imagecodec::testing::png_gradient(640, 480)).unwrap();
        assert_eq!(dims.width, 640);
        assert_eq!(dims.height, 480);
    }

    /// A file that begins with the PNG signature but is not a PNG has no
    /// dimensions to report. Before the decoder went in, the eight-byte
    /// signature plus two big-endian numbers at a fixed offset were the whole
    /// check, so any 24 bytes starting `\x89PNG` claimed to be a picture of
    /// whatever size those bytes happened to spell.
    #[test]
    fn a_png_signature_over_rubbish_has_no_dimensions() {
        let mut header = vec![0u8; 24];
        header[0..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        header[16..20].copy_from_slice(&640u32.to_be_bytes());
        header[20..24].copy_from_slice(&480u32.to_be_bytes());

        assert!(parse_png_dimensions(&header).is_none());
    }

    // -- Decoding a real picture --------------------------------------------

    /// One pixel of a finished thumbnail.
    ///
    /// Goes back through `Canvas` rather than indexing `Thumbnail::pixels`
    /// directly, so the test reads the byte order out of the same function
    /// [`into_thumbnail`] wrote it with. A test that restated `[a, r, g, b]`
    /// in its own words would keep passing if both ends were swapped together.
    fn thumb_pixel(thumb: &Thumbnail, x: u32, y: u32) -> Color {
        Canvas::from_argb(thumb.width, thumb.height, &thumb.pixels)
            .expect("a thumbnail's buffer always matches its dimensions")
            .get(x, y)
            .expect("coordinate inside the thumbnail")
    }

    /// A picture whose left half is red and whose right half is blue, so that
    /// "was this decoded?" and "was it decoded the right way round?" are
    /// separate answers. A gradient would show the first and hide the second.
    fn write_two_tone_png(path: &Path, width: u32, height: u32) {
        let bytes = imagecodec::testing::png_rgba(width, height, |x, _| {
            if x < width / 2 {
                [0xFF, 0x00, 0x00, 0xFF]
            } else {
                [0x00, 0x00, 0xFF, 0xFF]
            }
        });
        fs::write(path, bytes).unwrap();
    }

    /// The debt this whole change is about: a `.png` used to thumbnail as a
    /// flat green rectangle of the right shape and nothing else, because
    /// nothing in the tree decoded a compressed picture. A directory of
    /// photographs was a grid of identical rectangles.
    ///
    /// Three things are asserted, and the first two are the ones a wrong
    /// decode would still satisfy on its own: the thumbnail has the *source's*
    /// aspect ratio rather than the swatch's square canvas, the halves are the
    /// colours the file has and are on the sides the file put them, and the
    /// placeholder green appears nowhere at all.
    #[test]
    fn a_png_thumbnails_to_the_picture_and_not_a_coloured_rectangle() {
        let scratch = ScratchDir::new("thumbs_test_png_decode");
        let path = scratch.path("two-tone.png");
        write_two_tone_png(&path, 200, 100);

        let config = ThumbConfig::default();
        let thumb = generate_thumbnail(&path, &config);

        // 200x100 into a 128 box is 128x64. The swatch path would have
        // produced a 128x128 canvas with a rectangle centred in it, so the
        // height alone separates the two outcomes.
        assert_eq!((thumb.width, thumb.height), (128, 64));

        let left = thumb_pixel(&thumb, 20, 32);
        let right = thumb_pixel(&thumb, 108, 32);
        assert_eq!((left.r, left.g, left.b), (0xFF, 0x00, 0x00), "left half");
        assert_eq!(
            (right.r, right.g, right.b),
            (0x00, 0x00, 0xFF),
            "right half"
        );

        let accent = ThumbCategory::Image.accent_color();
        let canvas = Canvas::from_argb(thumb.width, thumb.height, &thumb.pixels).unwrap();
        for y in 0..thumb.height {
            for x in 0..thumb.width {
                assert_ne!(
                    canvas.get(x, y).unwrap(),
                    accent,
                    "placeholder green at ({x}, {y}) — this is still a swatch"
                );
            }
        }
    }

    /// Alpha comes through as the file wrote it. The decoder emits straight
    /// (non-premultiplied) alpha and the toolkit's `Color` stores it in its own
    /// field; a conversion that folded alpha into the colour channels would
    /// leave a translucent red looking like a dark opaque red, which is exactly
    /// the mistake that is invisible on the fully-opaque pictures every other
    /// test here uses.
    #[test]
    fn a_translucent_picture_keeps_its_alpha_through_the_thumbnail() {
        let scratch = ScratchDir::new("thumbs_test_png_alpha");
        let path = scratch.path("translucent.png");
        fs::write(
            &path,
            imagecodec::testing::png_rgba(64, 64, |_, _| [0xFF, 0x00, 0x00, 0x80]),
        )
        .unwrap();

        let thumb = generate_thumbnail(&path, &ThumbConfig::default());
        let px = thumb_pixel(&thumb, 32, 32);
        assert_eq!((px.r, px.g, px.b, px.a), (0xFF, 0x00, 0x00, 0x80));
    }

    /// A picture bigger than the cap is declined *from its header*, without the
    /// file being read — a directory listing decodes whatever is in it while
    /// the user waits, so the cost of one absurd file is paid by everything
    /// after it. The entry keeps the swatch it always had, which is why raising
    /// or lowering the cap cannot regress anything.
    #[test]
    fn a_picture_over_the_source_cap_keeps_the_swatch() {
        let scratch = ScratchDir::new("thumbs_test_png_too_big");
        let path = scratch.path("huge.png");
        write_two_tone_png(&path, 200, 100);

        let config = ThumbConfig {
            max_source_pixels: 10_000, // 200x100 is twice this.
            ..ThumbConfig::default()
        };
        let thumb = generate_thumbnail(&path, &config);

        // The swatch is drawn on a full square canvas, whatever the picture's
        // shape; the rectangle inside it is what carries the aspect ratio.
        assert_eq!((thumb.width, thumb.height), (config.size, config.size));
        assert_eq!(
            thumb_pixel(&thumb, config.size / 2, config.size / 2),
            ThumbCategory::Image.accent_color()
        );
    }

    /// A truncated download still has a readable IHDR, so it still reports a
    /// size — and then fails halfway through its pixel data. A file manager
    /// walking a directory must survive that quietly: the entry gets the swatch
    /// and the listing carries on. Reporting it would mean a dialog per bad
    /// file, and panicking would mean one bad file closing the file manager.
    #[test]
    fn a_truncated_png_is_a_swatch_and_not_a_panic() {
        let scratch = ScratchDir::new("thumbs_test_png_truncated");
        let path = scratch.path("half.png");
        let full = imagecodec::testing::png_gradient(200, 100);
        fs::write(&path, &full[..full.len() / 2]).unwrap();

        let config = ThumbConfig::default();
        let thumb = generate_thumbnail(&path, &config);

        assert_eq!((thumb.width, thumb.height), (config.size, config.size));
        assert_eq!(
            thumb_pixel(&thumb, config.size / 2, config.size / 2),
            ThumbCategory::Image.accent_color()
        );
    }

    /// A file whose bytes are not a picture at all, under a name that says it
    /// is. `parse_image_dimensions` finds no header it recognises, so this
    /// never reaches the decoder — it is the third outcome, the category
    /// placeholder, and the test exists to pin the boundary between it and the
    /// swatch.
    #[test]
    fn a_png_extension_over_arbitrary_bytes_falls_all_the_way_to_the_placeholder() {
        let scratch = ScratchDir::new("thumbs_test_png_not_a_picture");
        let path = scratch.path("lies.png");
        fs::write(&path, b"this is a text file wearing a hat").unwrap();

        let config = ThumbConfig::default();
        let thumb = generate_thumbnail(&path, &config);
        assert_eq!((thumb.width, thumb.height), (config.size, config.size));
        assert!(thumb.is_valid());
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
        let cmds = render_thumbnail(&thumb, 0xABCD, 10.0, 20.0, 100.0);

        // Should produce: BoxShadow, FillRect, Image, StrokeRect
        assert_eq!(cmds.len(), 4);
        assert!(matches!(cmds[0], RenderCommand::BoxShadow { .. }));
        assert!(matches!(cmds[1], RenderCommand::FillRect { .. }));
        assert!(matches!(cmds[3], RenderCommand::StrokeRect { .. }));

        // The id the caller asked for is the id the compositor is told to
        // draw. Anything else draws nothing, silently.
        let RenderCommand::Image { image_id, .. } = cmds[2] else {
            panic!("third command should be the picture itself");
        };
        assert_eq!(image_id, 0xABCD);
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
        let cmds = render_thumbnail(&thumb, 1, 0.0, 0.0, 64.0);
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
    /// The pair itself is built per-family by [`undecodable_pair`], so this
    /// runs on the Windows test host as well as on the real target — a
    /// regression asserted only on a platform we cannot execute is not
    /// asserted.
    #[test]
    fn two_names_differing_only_in_undecodable_units_do_not_share_a_cache_filename() {
        let (a, b) = undecodable_pair();
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
        let scratch = ScratchDir::new("thumbs_test_disk");
        let cache = DiskCache::new(scratch.dir().to_path_buf());
        cache.ensure_dir().unwrap();

        let thumb = make_test_thumb("test_disk.png", 4);
        cache.save(&thumb, 128).unwrap();

        let loaded = cache
            .load(Path::new("test_disk.png"), thumb.source_mtime, 128)
            .unwrap();
        assert_eq!(loaded.width, thumb.width);
        assert_eq!(loaded.height, thumb.height);
        assert_eq!(loaded.pixels.len(), thumb.pixels.len());
    }

    #[test]
    fn disk_cache_miss_wrong_mtime() {
        let scratch = ScratchDir::new("thumbs_test_disk_miss");
        let cache = DiskCache::new(scratch.dir().to_path_buf());
        cache.ensure_dir().unwrap();

        let thumb = make_test_thumb("miss.png", 4);
        cache.save(&thumb, 128).unwrap();

        // Different mtime => cache miss.
        assert!(
            cache
                .load(Path::new("miss.png"), thumb.source_mtime + 1, 128)
                .is_none()
        );
    }

    /// The cap is part of the key because [`fit_dimensions`] does not upscale:
    /// a source smaller than the cap yields a thumbnail of the *source's* size
    /// at every cap, so the stored dimensions cannot say which cap it was made
    /// at. Keyed only on path and mtime, raising the thumbnail size would serve
    /// the old small picture for as long as the file went unmodified.
    #[test]
    fn a_cached_thumbnail_is_not_served_at_a_size_it_was_not_made_at() {
        let scratch = ScratchDir::new("thumbs_test_disk_cap");
        let cache = DiskCache::new(scratch.dir().to_path_buf());
        cache.ensure_dir().unwrap();

        // A 4x4 thumbnail: smaller than either cap, so its dimensions are the
        // same whichever cap made it. This is the case the old key got wrong.
        let thumb = make_test_thumb("cap.png", 4);
        cache.save(&thumb, 64).unwrap();

        assert!(
            cache
                .load(Path::new("cap.png"), thumb.source_mtime, 64)
                .is_some(),
            "the cap it was made at hits"
        );
        assert!(
            cache
                .load(Path::new("cap.png"), thumb.source_mtime, 256)
                .is_none(),
            "a different cap must miss and regenerate"
        );
    }

    /// Purging is keyed on the hash alone, so a live file keeps its entries at
    /// every cap. A purge that matched whole filenames would delete the sizes
    /// the caller did not happen to name.
    #[test]
    fn purging_keeps_a_live_files_other_sizes() {
        let scratch = ScratchDir::new("thumbs_test_purge_caps");
        let cache = DiskCache::new(scratch.dir().to_path_buf());
        cache.ensure_dir().unwrap();

        let live = make_test_thumb("live.png", 4);
        cache.save(&live, 64).unwrap();
        cache.save(&live, 256).unwrap();
        let dead = make_test_thumb("dead.png", 4);
        cache.save(&dead, 64).unwrap();

        let mut valid = HashMap::new();
        valid.insert(live.source_path.clone(), live.source_mtime);
        cache.purge_stale(&valid).unwrap();

        assert!(
            cache
                .load(Path::new("live.png"), live.source_mtime, 64)
                .is_some()
        );
        assert!(
            cache
                .load(Path::new("live.png"), live.source_mtime, 256)
                .is_some()
        );
        assert!(
            cache
                .load(Path::new("dead.png"), dead.source_mtime, 64)
                .is_none(),
            "a file no longer in the listing is collected"
        );
    }

    /// A generator with a disk cache reuses what it wrote instead of decoding
    /// the file again. Asserted through the cache directory rather than through
    /// a timing measurement: the *observable* difference is that a file appears
    /// there, and that a second run of the same request produces the same
    /// pixels without the source having to still exist.
    #[test]
    fn a_generator_with_a_disk_cache_reuses_what_it_wrote() {
        let scratch = ScratchDir::new("thumbs_test_gen_disk");
        let source = scratch.dir().join("note.txt");
        fs::write(&source, b"hello").unwrap();
        let mtime = file_mtime(&source).unwrap_or(0);

        let cache_dir = scratch.dir().join("cache");
        let mut tg = ThumbnailGenerator::with_disk_cache(DiskCache::new(cache_dir.clone()));
        assert!(tg.disk_cache().is_some());

        let req = ThumbnailRequest {
            path: source.clone(),
            mtime,
            size: 5,
            config: ThumbConfig::default(),
        };
        tg.push(req.clone());
        assert_eq!(tg.process_batch(4), 1);
        let first = tg.take_completed().pop().expect("one result").1;

        let written: Vec<_> = fs::read_dir(&cache_dir).unwrap().flatten().collect();
        assert_eq!(written.len(), 1, "generating wrote exactly one cache file");

        // Delete the source. A second request can now only be satisfied from
        // the cache, so identical pixels prove the cache was the one consulted.
        fs::remove_file(&source).unwrap();
        tg.push(req);
        assert_eq!(tg.process_batch(4), 1);
        let second = tg.take_completed().pop().expect("one result").1;
        assert_eq!(second.width, first.width);
        assert_eq!(second.height, first.height);
        assert_eq!(second.pixels, first.pixels);
    }

    /// The unbacked generator is a working configuration, not a broken one.
    #[test]
    fn a_generator_without_a_disk_cache_still_generates() {
        let mut tg = ThumbnailGenerator::new();
        assert!(tg.disk_cache().is_none());
        tg.push(ThumbnailRequest {
            path: PathBuf::from("/nonexistent/x.txt"),
            mtime: 0,
            size: 0,
            config: ThumbConfig::default(),
        });
        assert_eq!(tg.process_batch(1), 1);
        assert_eq!(tg.take_completed().len(), 1);
    }

    // -- Helper -------------------------------------------------------------

    /// Create a minimal test thumbnail with solid-colored pixels.
    ///
    /// Built through a `Canvas` and [`into_thumbnail`], the way production
    /// code makes one, so the buffer is in the byte order the rest of the
    /// module assumes and its length agrees with the dimensions by
    /// construction — a hand-filled `vec![]` here would let a test pass that
    /// the real path could not.
    fn make_test_thumb(name: &str, size: u32) -> Thumbnail {
        let canvas = Canvas::filled(size, size, Color::rgba(128, 128, 128, 128));
        into_thumbnail(canvas, Path::new(name), 42)
    }

    /// Scaling preserves the aspect ratio and never returns a zero side.
    ///
    /// `fit_dimensions` has eleven callers and had **no test** before
    /// 2026-09-03, which was discovered by rewriting its division to
    /// `checked_div` and having nothing to say whether the behaviour was
    /// unchanged. The rewrite is safe *because* of the `w == 0 || h == 0`
    /// guard above it; this is what pins that the guard is still there.
    #[test]
    fn scaling_preserves_the_ratio_and_never_returns_zero() {
        // Already small enough: untouched.
        assert_eq!(fit_dimensions(40, 30, 128), (40, 30));

        // Landscape and portrait both pin the long side to `max_size`.
        assert_eq!(fit_dimensions(400, 200, 100), (100, 50));
        assert_eq!(fit_dimensions(200, 400, 100), (50, 100));
        assert_eq!(fit_dimensions(300, 300, 100), (100, 100));

        // A sliver does not scale to nothing — a zero-height thumbnail is an
        // invisible one, and `.max(1)` is what stops it.
        let (w, h) = fit_dimensions(10_000, 3, 100);
        assert_eq!(w, 100);
        assert!(h >= 1, "a very wide image scaled to zero height");

        // The degenerate inputs the guard exists for.
        assert_eq!(fit_dimensions(0, 10, 100), (0, 0));
        assert_eq!(fit_dimensions(10, 0, 100), (0, 0));
        assert_eq!(fit_dimensions(10, 10, 0), (0, 0));
    }

    /// No prefix of a JPEG panics or hangs the dimension parser.
    ///
    /// The segment walker advances by a length read out of the file. It is
    /// correct — `seg_len < 2` is refused, so the cursor always moves — but
    /// that is the property worth pinning rather than assuming, since the same
    /// shape in `apps/musicplayer`'s WAV parser could be made to loop forever.
    /// A missed bound shows up only at the length that reaches it, so this
    /// sweeps every prefix rather than sampling.
    #[test]
    fn no_prefix_of_a_jpeg_panics_or_hangs_the_parser() {
        let mut jpeg: Vec<u8> = vec![0xFF, 0xD8];
        // A comment segment, then SOF0 with 64x48.
        jpeg.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x04, 0xAA, 0xBB]);
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&48u16.to_be_bytes());
        jpeg.extend_from_slice(&64u16.to_be_bytes());
        jpeg.extend_from_slice(&[0u8; 8]);

        // `ImageDimensions` is not `PartialEq`, so the fields are compared —
        // and asserting the fixture parses is what keeps the sweep below
        // meaningful rather than a loop over something that is not a JPEG.
        let got = parse_jpeg_dimensions(&jpeg).expect("the fixture is a JPEG");
        assert_eq!((got.width, got.height), (64, 48));

        for len in 0..=jpeg.len() {
            let _ = parse_jpeg_dimensions(jpeg.get(..len).unwrap_or(&[]));
        }

        // A segment claiming a length of zero is refused rather than walked
        // forever: `seg_len < 2` is the check that makes the cursor advance.
        let stuck = vec![0xFF, 0xD8, 0xFF, 0xFE, 0x00, 0x00, 0x00, 0x00];
        assert!(parse_jpeg_dimensions(&stuck).is_none());
    }
}
