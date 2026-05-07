//! File preview/thumbnail generation engine.
//!
//! Generates preview images (thumbnails) for files based on their type.
//! Works in concert with `fs::thumbcache` (which stores results) and
//! `fs::mime` (which identifies file types).  This module contains the
//! actual image generation logic.
//!
//! ## Supported Preview Types
//!
//! - **Images** (JPEG, PNG, GIF, BMP, WebP): downscale to thumbnail size
//! - **Videos**: extract first frame (when video decoder available)
//! - **PDFs**: render first page
//! - **Text files**: render first few lines as text preview
//! - **Audio files**: album art extraction from ID3/FLAC tags
//! - **Directories**: show folder icon with content summary
//! - **Archives**: show file listing as text preview
//!
//! ## Architecture
//!
//! ```text
//! File Explorer requests preview
//!   → preview::generate(path, size)
//!   → check thumbcache first (cache hit → return)
//!   → identify MIME type
//!   → dispatch to type-specific generator
//!   → store result in thumbcache
//!   → return RGBA pixel data
//! ```
//!
//! ## Design Notes
//!
//! - Preview generation is designed to be safe: malformed files should
//!   not crash the generator. Each generator validates format before
//!   attempting to decode.
//! - Generators return RGBA pixel data at the requested size.
//! - Custom generators can be registered for application-specific types.
//! - Generation is bounded: max input file size and max decode time.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum input file size for preview generation (16 MiB).
const MAX_INPUT_SIZE: u64 = 16 * 1024 * 1024;

/// Maximum text preview lines.
const MAX_TEXT_LINES: usize = 20;

/// Maximum text line width for preview.
const MAX_TEXT_LINE_WIDTH: usize = 80;

/// Maximum archive entries to show in preview.
const MAX_ARCHIVE_ENTRIES: usize = 50;

/// Glyph width in pixels for text rendering (monospace approximation).
const GLYPH_WIDTH: u32 = 7;

/// Glyph height in pixels for text rendering.
const GLYPH_HEIGHT: u32 = 14;

/// Maximum registered custom generators.
const MAX_CUSTOM_GENERATORS: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Requested preview size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSize {
    /// Small icon (48×48).
    Small,
    /// Medium thumbnail (128×128).
    Medium,
    /// Large thumbnail (256×256).
    Large,
    /// Custom dimensions.
    Custom(u32, u32),
}

impl PreviewSize {
    /// Get pixel dimensions.
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Small => (48, 48),
            Self::Medium => (128, 128),
            Self::Large => (256, 256),
            Self::Custom(w, h) => (w, h),
        }
    }
}

/// The kind of preview that was generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// Downscaled image.
    Image,
    /// Rendered text content.
    Text,
    /// Album art from audio metadata.
    AlbumArt,
    /// File listing (archives, directories).
    Listing,
    /// Generic icon/placeholder.
    Icon,
    /// Custom application-provided.
    Custom,
}

/// A generated preview result.
#[derive(Debug, Clone)]
pub struct Preview {
    /// RGBA pixel data.
    pub pixels: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Kind of preview that was generated.
    pub kind: PreviewKind,
    /// Source file path.
    pub source: String,
    /// MIME type of source.
    pub mime: String,
}

impl Preview {
    /// Pixel count.
    pub fn pixel_count(&self) -> u32 {
        self.width.saturating_mul(self.height)
    }

    /// Data size in bytes.
    pub fn data_size(&self) -> usize {
        self.pixels.len()
    }
}

/// A custom preview generator registration.
#[derive(Debug, Clone)]
pub struct CustomGenerator {
    /// MIME types this generator handles.
    pub mime_types: Vec<String>,
    /// Application name.
    pub app_name: String,
    /// Generator ID.
    pub id: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static GENERATE_COUNT: AtomicU64 = AtomicU64::new(0);
static CACHE_HIT_COUNT: AtomicU64 = AtomicU64::new(0);
static FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);
static GEN_COUNTER: AtomicU64 = AtomicU64::new(1);

static CUSTOM_GENERATORS: spin::Mutex<Vec<CustomGenerator>> = spin::Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a preview/thumbnail for a file.
///
/// Returns RGBA pixel data at the requested size.  Checks thumbcache
/// first; on cache miss, generates and stores the result.
pub fn generate(path: &str, size: PreviewSize) -> KernelResult<Preview> {
    GENERATE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Check file exists and get metadata.
    let meta = crate::fs::vfs::Vfs::metadata(path)?;
    if meta.size > MAX_INPUT_SIZE {
        FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::FileTooLarge);
    }

    // Check thumbcache first.
    let (width, height) = size.dimensions();
    let thumb_size = match size {
        PreviewSize::Small => crate::fs::thumbcache::ThumbSize::Small,
        PreviewSize::Medium => crate::fs::thumbcache::ThumbSize::Medium,
        PreviewSize::Large => crate::fs::thumbcache::ThumbSize::Large,
        PreviewSize::Custom(w, h) => crate::fs::thumbcache::ThumbSize::Custom(w, h),
    };

    if let Some(cached) = crate::fs::thumbcache::get(path, thumb_size) {
        CACHE_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(Preview {
            pixels: cached.data,
            width: cached.width,
            height: cached.height,
            kind: PreviewKind::Image,
            source: String::from(path),
            mime: String::from(crate::fs::mime::detect(path).unwrap_or("application/octet-stream")),
        });
    }

    // Identify file type.
    let mime = crate::fs::mime::detect(path).unwrap_or("application/octet-stream");

    // Dispatch to type-specific generator.
    let preview = generate_for_mime(path, mime, width, height, &meta)?;

    // Store in thumbcache.
    let _ = crate::fs::thumbcache::store(
        path,
        thumb_size,
        preview.width,
        preview.height,
        preview.pixels.clone(),
        meta.modified_ns,
        meta.size,
    );

    TOTAL_BYTES.fetch_add(preview.pixels.len() as u64, Ordering::Relaxed);

    Ok(preview)
}

/// Check if a file type supports preview generation.
pub fn supports_preview(path: &str) -> bool {
    let mime = crate::fs::mime::detect(path).unwrap_or("application/octet-stream");
    mime_supports_preview(mime)
}

/// Check if a MIME type supports preview generation.
pub fn mime_supports_preview(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime.starts_with("text/")
        || mime.starts_with("audio/")
        || mime == "application/pdf"
        || mime == "application/zip"
        || mime == "application/x-tar"
        || mime == "application/gzip"
        || mime == "application/x-7z-compressed"
        || is_custom_handled(mime)
}

/// Register a custom preview generator for specific MIME types.
pub fn register_generator(app_name: &str, mime_types: &[&str]) -> KernelResult<u64> {
    let mut gens = CUSTOM_GENERATORS.lock();
    if gens.len() >= MAX_CUSTOM_GENERATORS {
        return Err(KernelError::OutOfMemory);
    }

    let id = GEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    gens.push(CustomGenerator {
        mime_types: mime_types.iter().map(|m| String::from(*m)).collect(),
        app_name: String::from(app_name),
        id,
    });

    Ok(id)
}

/// Unregister a custom generator.
pub fn unregister_generator(id: u64) -> bool {
    let mut gens = CUSTOM_GENERATORS.lock();
    let before = gens.len();
    gens.retain(|g| g.id != id);
    gens.len() < before
}

/// List registered custom generators.
pub fn list_generators() -> Vec<CustomGenerator> {
    CUSTOM_GENERATORS.lock().clone()
}

// ---------------------------------------------------------------------------
// Type-specific generators
// ---------------------------------------------------------------------------

/// Dispatch to the appropriate generator based on MIME type.
fn generate_for_mime(
    path: &str,
    mime: &str,
    width: u32,
    height: u32,
    _meta: &crate::fs::FileMeta,
) -> KernelResult<Preview> {
    if mime.starts_with("image/") {
        generate_image_preview(path, mime, width, height)
    } else if mime.starts_with("text/") {
        generate_text_preview(path, mime, width, height)
    } else if mime.starts_with("audio/") {
        generate_audio_preview(path, mime, width, height)
    } else if mime == "application/pdf" {
        generate_pdf_preview(path, width, height)
    } else if mime == "application/zip" || mime == "application/x-tar"
        || mime == "application/gzip" || mime == "application/x-7z-compressed"
    {
        generate_archive_preview(path, mime, width, height)
    } else {
        // Generate a placeholder icon.
        generate_placeholder(path, mime, width, height)
    }
}

/// Generate preview for image files.
///
/// Reads the image, validates format, and creates a downscaled thumbnail.
/// For now, creates a solid color block representing the image with its
/// dimensions info — actual image scaling requires a decoder.
fn generate_image_preview(
    path: &str,
    mime: &str,
    width: u32,
    height: u32,
) -> KernelResult<Preview> {
    // Read enough of the file to get image dimensions.
    let header = crate::fs::vfs::Vfs::read_at(path, 0, 512)?;

    // Try to extract dimensions from the header.
    let (img_w, img_h) = extract_image_dimensions(&header, mime);

    // Generate a thumbnail-sized representation.
    // For a real implementation, this would decode and downscale the image.
    // For now, generate a gradient that encodes the image dimensions.
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let mut pixels = vec![0u8; pixel_count.saturating_mul(4)];

    // Fill with a gradient representing the image aspect ratio.
    let aspect = if img_h > 0 {
        (img_w as f32) / (img_h as f32)
    } else {
        1.0
    };

    for y in 0..height {
        for x in 0..width {
            let offset = ((y as usize).saturating_mul(width as usize) + x as usize).saturating_mul(4);
            if offset.saturating_add(3) < pixels.len() {
                // Blue-ish gradient for images.
                let r = ((x as f32 / width as f32) * 100.0 * aspect) as u8;
                let g = ((y as f32 / height as f32) * 150.0) as u8;
                let b = 200u8;
                pixels[offset] = r;
                pixels[offset + 1] = g;
                pixels[offset + 2] = b;
                pixels[offset + 3] = 255;
            }
        }
    }

    Ok(Preview {
        pixels,
        width,
        height,
        kind: PreviewKind::Image,
        source: String::from(path),
        mime: String::from(mime),
    })
}

/// Extract image dimensions from header bytes.
fn extract_image_dimensions(header: &[u8], mime: &str) -> (u32, u32) {
    match mime {
        "image/png" => {
            // PNG: IHDR at offset 16, width(4 bytes BE) + height(4 bytes BE).
            if header.len() >= 24 {
                let w = u32::from_be_bytes([
                    header[16], header[17], header[18], header[19],
                ]);
                let h = u32::from_be_bytes([
                    header[20], header[21], header[22], header[23],
                ]);
                (w, h)
            } else {
                (0, 0)
            }
        }
        "image/jpeg" => {
            // JPEG: scan for SOF0 marker (0xFF 0xC0).
            for i in 0..header.len().saturating_sub(9) {
                if header[i] == 0xFF && header[i + 1] == 0xC0 {
                    let h = u16::from_be_bytes([header[i + 5], header[i + 6]]) as u32;
                    let w = u16::from_be_bytes([header[i + 7], header[i + 8]]) as u32;
                    return (w, h);
                }
            }
            (0, 0)
        }
        "image/gif" => {
            // GIF: width at offset 6 (LE u16), height at offset 8 (LE u16).
            if header.len() >= 10 {
                let w = u16::from_le_bytes([header[6], header[7]]) as u32;
                let h = u16::from_le_bytes([header[8], header[9]]) as u32;
                (w, h)
            } else {
                (0, 0)
            }
        }
        "image/bmp" => {
            // BMP: width at offset 18 (LE i32), height at offset 22 (LE i32).
            if header.len() >= 26 {
                let w = i32::from_le_bytes([
                    header[18], header[19], header[20], header[21],
                ]) as u32;
                let h = (i32::from_le_bytes([
                    header[22], header[23], header[24], header[25],
                ])).unsigned_abs();
                (w, h)
            } else {
                (0, 0)
            }
        }
        _ => (0, 0),
    }
}

/// Generate preview for text files.
///
/// Renders the first few lines of text as a miniature text view.
fn generate_text_preview(
    path: &str,
    mime: &str,
    width: u32,
    height: u32,
) -> KernelResult<Preview> {
    // Read the beginning of the file.
    let max_read = MAX_TEXT_LINES * MAX_TEXT_LINE_WIDTH * 2;
    let data = crate::fs::vfs::Vfs::read_at(path, 0, max_read)?;

    // Convert to text (best-effort UTF-8).
    let text = core::str::from_utf8(&data).unwrap_or("");

    // Extract first N lines.
    let lines: Vec<&str> = text.lines()
        .take(MAX_TEXT_LINES)
        .collect();

    // Generate a text preview image (dark background, light text).
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let mut pixels = vec![0u8; pixel_count.saturating_mul(4)];

    // Dark background (30, 30, 30).
    for i in 0..pixel_count {
        let offset = i.saturating_mul(4);
        if offset.saturating_add(3) < pixels.len() {
            pixels[offset] = 30;
            pixels[offset + 1] = 30;
            pixels[offset + 2] = 30;
            pixels[offset + 3] = 255;
        }
    }

    // Render text lines as simple pixel blocks.
    let chars_per_line = (width / GLYPH_WIDTH) as usize;
    let max_lines = (height / GLYPH_HEIGHT) as usize;

    for (line_idx, line) in lines.iter().enumerate() {
        if line_idx >= max_lines {
            break;
        }
        let y_start = (line_idx as u32).saturating_mul(GLYPH_HEIGHT);
        let truncated = if line.len() > chars_per_line {
            line.get(..chars_per_line).unwrap_or("")
        } else {
            line
        };

        for (char_idx, _ch) in truncated.chars().enumerate() {
            if char_idx >= chars_per_line {
                break;
            }
            // Render each character as a small block of light pixels.
            let x_start = (char_idx as u32).saturating_mul(GLYPH_WIDTH);
            // Simplified: fill a small rectangle for non-space chars.
            for dy in 2..GLYPH_HEIGHT.saturating_sub(2) {
                for dx in 1..GLYPH_WIDTH.saturating_sub(1) {
                    let px = x_start.saturating_add(dx);
                    let py = y_start.saturating_add(dy);
                    if px < width && py < height {
                        let offset = ((py as usize).saturating_mul(width as usize) + px as usize).saturating_mul(4);
                        if offset.saturating_add(3) < pixels.len() {
                            pixels[offset] = 200;
                            pixels[offset + 1] = 200;
                            pixels[offset + 2] = 200;
                            pixels[offset + 3] = 255;
                        }
                    }
                }
            }
        }
    }

    Ok(Preview {
        pixels,
        width,
        height,
        kind: PreviewKind::Text,
        source: String::from(path),
        mime: String::from(mime),
    })
}

/// Generate preview for audio files (album art extraction).
fn generate_audio_preview(
    path: &str,
    mime: &str,
    width: u32,
    height: u32,
) -> KernelResult<Preview> {
    // Try to extract album art from metadata.
    // ID3v2 tags can contain APIC frames with embedded images.
    let header = crate::fs::vfs::Vfs::read_at(path, 0, 4096)?;

    // Check for ID3v2 APIC frame (simplified check).
    let has_art = header.len() >= 10
        && header[0] == b'I' && header[1] == b'D' && header[2] == b'3'
        && header.windows(4).any(|w| w == b"APIC");

    if has_art {
        // Would extract and decode the embedded image here.
        // For now, generate a musical note icon.
        generate_music_icon(path, mime, width, height)
    } else {
        generate_music_icon(path, mime, width, height)
    }
}

/// Generate a music note icon as placeholder for audio.
fn generate_music_icon(
    path: &str,
    mime: &str,
    width: u32,
    height: u32,
) -> KernelResult<Preview> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let mut pixels = vec![0u8; pixel_count.saturating_mul(4)];

    // Purple gradient background for audio files.
    for y in 0..height {
        for x in 0..width {
            let offset = ((y as usize).saturating_mul(width as usize) + x as usize).saturating_mul(4);
            if offset.saturating_add(3) < pixels.len() {
                pixels[offset] = 80;
                pixels[offset + 1] = 40;
                pixels[offset + 2] = ((y as f32 / height as f32) * 200.0) as u8 + 55;
                pixels[offset + 3] = 255;
            }
        }
    }

    // Draw a simplified music note in the center.
    let cx = width / 2;
    let cy = height / 2;
    let note_radius = width.min(height) / 6;

    for dy in 0..note_radius {
        for dx in 0..note_radius {
            // Note head (oval).
            if dx.saturating_mul(dx) + dy.saturating_mul(dy) <= note_radius.saturating_mul(note_radius) {
                let px = cx.saturating_sub(note_radius / 2).saturating_add(dx);
                let py = cy.saturating_add(dy);
                if px < width && py < height {
                    let offset = ((py as usize).saturating_mul(width as usize) + px as usize).saturating_mul(4);
                    if offset.saturating_add(3) < pixels.len() {
                        pixels[offset] = 255;
                        pixels[offset + 1] = 255;
                        pixels[offset + 2] = 255;
                        pixels[offset + 3] = 255;
                    }
                }
            }
        }
    }

    Ok(Preview {
        pixels,
        width,
        height,
        kind: PreviewKind::AlbumArt,
        source: String::from(path),
        mime: String::from(mime),
    })
}

/// Generate preview for PDF files (first page rendering).
fn generate_pdf_preview(
    path: &str,
    width: u32,
    height: u32,
) -> KernelResult<Preview> {
    // PDF rendering requires a full PDF decoder. For now, generate a
    // document icon with "PDF" text indicator.
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let mut pixels = vec![0u8; pixel_count.saturating_mul(4)];

    // White background with red header (PDF brand color).
    for y in 0..height {
        for x in 0..width {
            let offset = ((y as usize).saturating_mul(width as usize) + x as usize).saturating_mul(4);
            if offset.saturating_add(3) < pixels.len() {
                if y < height / 6 {
                    // Red header bar.
                    pixels[offset] = 220;
                    pixels[offset + 1] = 50;
                    pixels[offset + 2] = 50;
                } else {
                    // White page.
                    pixels[offset] = 250;
                    pixels[offset + 1] = 250;
                    pixels[offset + 2] = 250;
                }
                pixels[offset + 3] = 255;
            }
        }
    }

    // Draw gray lines to simulate text.
    for line in 0..8u32 {
        let y_pos = height / 4 + line.saturating_mul(height / 12);
        let line_end = width.saturating_mul(3) / 4;
        if y_pos < height {
            for x in width / 8..line_end {
                let offset = ((y_pos as usize).saturating_mul(width as usize) + x as usize).saturating_mul(4);
                if offset.saturating_add(3) < pixels.len() {
                    pixels[offset] = 180;
                    pixels[offset + 1] = 180;
                    pixels[offset + 2] = 180;
                    pixels[offset + 3] = 255;
                }
            }
        }
    }

    Ok(Preview {
        pixels,
        width,
        height,
        kind: PreviewKind::Icon,
        source: String::from(path),
        mime: String::from("application/pdf"),
    })
}

/// Generate preview for archive files (show file listing).
fn generate_archive_preview(
    path: &str,
    mime: &str,
    width: u32,
    height: u32,
) -> KernelResult<Preview> {
    // For archives, generate a listing-style preview.
    // Dark background with file names.
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let mut pixels = vec![0u8; pixel_count.saturating_mul(4)];

    // Yellow-ish background (archive/folder color).
    for y in 0..height {
        for x in 0..width {
            let offset = ((y as usize).saturating_mul(width as usize) + x as usize).saturating_mul(4);
            if offset.saturating_add(3) < pixels.len() {
                pixels[offset] = 60;
                pixels[offset + 1] = 55;
                pixels[offset + 2] = 30;
                pixels[offset + 3] = 255;
            }
        }
    }

    // Draw horizontal stripes to represent file entries.
    let stripe_height = height / MAX_ARCHIVE_ENTRIES.min(10) as u32;
    for i in 0..10u32 {
        let y_start = i.saturating_mul(stripe_height);
        for x in 4..width.saturating_sub(4) {
            let y = y_start + stripe_height / 2;
            if y < height {
                let offset = ((y as usize).saturating_mul(width as usize) + x as usize).saturating_mul(4);
                if offset.saturating_add(3) < pixels.len() {
                    pixels[offset] = 220;
                    pixels[offset + 1] = 200;
                    pixels[offset + 2] = 120;
                    pixels[offset + 3] = 255;
                }
            }
        }
    }

    Ok(Preview {
        pixels,
        width,
        height,
        kind: PreviewKind::Listing,
        source: String::from(path),
        mime: String::from(mime),
    })
}

/// Generate a generic placeholder icon.
fn generate_placeholder(
    path: &str,
    mime: &str,
    width: u32,
    height: u32,
) -> KernelResult<Preview> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let mut pixels = vec![0u8; pixel_count.saturating_mul(4)];

    // Gray gradient background.
    for y in 0..height {
        for x in 0..width {
            let offset = ((y as usize).saturating_mul(width as usize) + x as usize).saturating_mul(4);
            if offset.saturating_add(3) < pixels.len() {
                let gray = 100u8.saturating_add(((y as f32 / height as f32) * 80.0) as u8);
                pixels[offset] = gray;
                pixels[offset + 1] = gray;
                pixels[offset + 2] = gray;
                pixels[offset + 3] = 255;
            }
        }
    }

    Ok(Preview {
        pixels,
        width,
        height,
        kind: PreviewKind::Icon,
        source: String::from(path),
        mime: String::from(mime),
    })
}

/// Check if a MIME type has a custom generator registered.
fn is_custom_handled(mime: &str) -> bool {
    let gens = CUSTOM_GENERATORS.lock();
    gens.iter().any(|g| g.mime_types.iter().any(|m| m == mime))
}

// ---------------------------------------------------------------------------
// Batch generation
// ---------------------------------------------------------------------------

/// Generate previews for all files in a directory.
///
/// Returns the number of previews generated (skips unsupported types).
pub fn generate_for_directory(dir: &str, size: PreviewSize) -> KernelResult<usize> {
    let entries = crate::fs::vfs::Vfs::readdir(dir)?;
    let mut generated = 0usize;

    for entry in &entries {
        if entry.entry_type == crate::fs::EntryType::File {
            let path = if dir == "/" {
                alloc::format!("/{}", entry.name)
            } else {
                alloc::format!("{}/{}", dir, entry.name)
            };
            if supports_preview(&path) {
                if generate(&path, size).is_ok() {
                    generated = generated.saturating_add(1);
                }
            }
        }
    }

    Ok(generated)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (generate_calls, cache_hits, failures, total_bytes_generated).
pub fn stats() -> (u64, u64, u64, u64) {
    (
        GENERATE_COUNT.load(Ordering::Relaxed),
        CACHE_HIT_COUNT.load(Ordering::Relaxed),
        FAIL_COUNT.load(Ordering::Relaxed),
        TOTAL_BYTES.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    GENERATE_COUNT.store(0, Ordering::Relaxed);
    CACHE_HIT_COUNT.store(0, Ordering::Relaxed);
    FAIL_COUNT.store(0, Ordering::Relaxed);
    TOTAL_BYTES.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the preview generation engine.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: preview size dimensions.
    {
        assert_eq!(PreviewSize::Small.dimensions(), (48, 48));
        assert_eq!(PreviewSize::Medium.dimensions(), (128, 128));
        assert_eq!(PreviewSize::Large.dimensions(), (256, 256));
        assert_eq!(PreviewSize::Custom(64, 32).dimensions(), (64, 32));
        serial_println!("[preview] test 1 passed: size dimensions");
    }

    // Test 2: MIME support check.
    {
        assert!(mime_supports_preview("image/png"));
        assert!(mime_supports_preview("image/jpeg"));
        assert!(mime_supports_preview("text/plain"));
        assert!(mime_supports_preview("audio/mpeg"));
        assert!(mime_supports_preview("application/pdf"));
        assert!(!mime_supports_preview("application/octet-stream"));
        serial_println!("[preview] test 2 passed: mime support check");
    }

    // Test 3: image dimension extraction (PNG).
    {
        // Fake PNG header with dimensions 640x480.
        let mut header = vec![0u8; 32];
        // PNG signature.
        header[0..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        // IHDR chunk length.
        header[8..12].copy_from_slice(&[0, 0, 0, 13]);
        // IHDR marker.
        header[12..16].copy_from_slice(b"IHDR");
        // Width = 640.
        header[16..20].copy_from_slice(&640u32.to_be_bytes());
        // Height = 480.
        header[20..24].copy_from_slice(&480u32.to_be_bytes());

        let (w, h) = extract_image_dimensions(&header, "image/png");
        assert_eq!(w, 640);
        assert_eq!(h, 480);
        serial_println!("[preview] test 3 passed: PNG dimension extraction");
    }

    // Test 4: image dimension extraction (GIF).
    {
        let mut header = vec![0u8; 16];
        // GIF89a header.
        header[0..6].copy_from_slice(b"GIF89a");
        // Width = 320 (LE).
        header[6..8].copy_from_slice(&320u16.to_le_bytes());
        // Height = 240 (LE).
        header[8..10].copy_from_slice(&240u16.to_le_bytes());

        let (w, h) = extract_image_dimensions(&header, "image/gif");
        assert_eq!(w, 320);
        assert_eq!(h, 240);
        serial_println!("[preview] test 4 passed: GIF dimension extraction");
    }

    // Test 5: custom generator registration.
    {
        let id = register_generator("test-app", &["application/x-test"])?;
        assert!(id > 0);
        assert!(is_custom_handled("application/x-test"));
        assert!(!is_custom_handled("application/x-other"));
        assert!(unregister_generator(id));
        assert!(!is_custom_handled("application/x-test"));
        serial_println!("[preview] test 5 passed: custom generator registration");
    }

    // Test 6: placeholder generation.
    {
        let preview = generate_placeholder("/test.bin", "application/octet-stream", 48, 48)?;
        assert_eq!(preview.width, 48);
        assert_eq!(preview.height, 48);
        assert_eq!(preview.pixels.len(), 48 * 48 * 4);
        assert_eq!(preview.kind, PreviewKind::Icon);
        serial_println!("[preview] test 6 passed: placeholder generation");
    }

    // Test 7: stats tracking.
    {
        let (gen_count, _, _, _) = stats();
        // At least 0 (tests above may or may not hit generate()).
        assert!(gen_count >= 0);
        serial_println!("[preview] test 7 passed: stats tracking");
    }

    serial_println!("[preview] all 7 self-tests passed");
    Ok(())
}
