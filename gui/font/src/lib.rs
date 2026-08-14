//! Slate OS Font Library — bitmap and outline font rendering.
//!
//! This crate provides the system font rendering infrastructure for SlateOS.
//! It supports bitmap fonts with procedurally generated system glyphs,
//! text layout with word wrapping, and glyph rendering to ARGB buffers.
//!
//! # Architecture
//!
//! The font system has three layers:
//! 1. **Font data**: `Font`, `GlyphBitmap`, `FontMetrics` — storage of glyph bitmaps and metrics
//! 2. **Text layout**: `TextLayout`, `LayoutResult` — word wrapping, alignment, line breaking
//! 3. **Rendering**: `render_glyph_to_buffer` — stamping glyphs onto pixel buffers
//!
//! # Built-in Fonts
//!
//! The library includes a procedurally generated 8x16 monospace bitmap font covering:
//! - Basic Latin (U+0020..U+007E) — full glyph coverage
//! - Box drawing characters (U+2500..U+257F) — procedurally generated
//! - Block elements (U+2580..U+259F) — procedurally generated
//! - Missing glyphs render as a replacement box (hollow rectangle)
//!
//! # Dependencies and I/O
//!
//! Everything here is written in `alloc` terms — `alloc::vec::Vec`,
//! `alloc::string::String` — rather than through the `std` prelude, because
//! the intent is for this crate to be `no_std`. It is **not `no_std` yet**:
//! the rasterizer and the metric scaling call `f32::sqrt`, `floor`, `ceil`,
//! `round` and `mul_add`, which live in `std` and not in `core`, so the
//! declaration would need a `libm` dependency that this workspace does not
//! have. Tracked as `TD-FONT-NOT-ACTUALLY-NO-STD` in `known-issues.md`;
//! please keep writing `alloc::` paths so that closing it stays a small
//! change.
//!
//! Regardless of that, this crate **does no I/O** and should not start. Font
//! *discovery* — walking a directory, reading a file — belongs to the caller,
//! which knows whether it is talking to a host filesystem or to the SlateOS
//! VFS. This crate takes bytes and gives back glyphs.

extern crate alloc;

pub mod bidi;
mod bidi_tables;
pub mod cff;
mod context;
mod fallback;
mod gpos;
pub mod gsub;
mod indic;
mod indic_machine;
mod indic_tables;
mod joining;
mod joining_tables;
mod kern;
mod mark;
mod norm;
mod norm_tables;
mod otl;
pub mod raster;
pub mod scaled;
pub mod script;
mod script_tables;
pub mod select;
pub mod sfnt;
pub mod shape;
mod skip;
pub mod system;
mod would;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Font metrics
// ---------------------------------------------------------------------------

/// Overall metrics for a font face.
#[derive(Debug, Clone)]
pub struct FontMetrics {
    /// Distance from baseline to top of tallest glyph.
    pub ascent: f32,
    /// Distance from baseline to bottom of lowest descender (positive downward).
    pub descent: f32,
    /// Total line height (ascent + descent + leading).
    pub line_height: f32,
    /// Maximum horizontal advance of any glyph.
    pub max_advance: f32,
    /// Average horizontal advance across common characters.
    pub average_advance: f32,
    /// Height of capital letters above baseline.
    pub cap_height: f32,
    /// Height of lowercase 'x' above baseline.
    pub x_height: f32,
}

/// Per-glyph metrics describing dimensions and positioning.
#[derive(Debug, Clone)]
pub struct GlyphMetrics {
    /// Width of the glyph bitmap in pixels.
    pub width: u32,
    /// Height of the glyph bitmap in pixels.
    pub height: u32,
    /// Horizontal advance after rendering this glyph.
    pub advance_x: f32,
    /// Horizontal bearing (offset from pen position to left edge of bitmap).
    pub bearing_x: f32,
    /// Vertical bearing (offset from baseline to top edge of bitmap).
    pub bearing_y: f32,
}

// ---------------------------------------------------------------------------
// Glyph bitmap
// ---------------------------------------------------------------------------

/// A single glyph rendered as a 1-bit-per-pixel bitmap.
///
/// The bitmap is stored row-major, with each row padded to byte boundaries.
/// A set bit (1) means the pixel is "on" (foreground); 0 means background.
#[derive(Debug, Clone)]
pub struct GlyphBitmap {
    /// Width of the bitmap in pixels.
    pub width: u32,
    /// Height of the bitmap in pixels.
    pub height: u32,
    /// Horizontal advance to the next glyph's origin.
    pub advance: f32,
    /// Horizontal offset from pen position to left edge of bitmap.
    pub bearing_x: f32,
    /// Vertical offset from baseline to top edge of bitmap.
    pub bearing_y: f32,
    /// Raw bitmap data, 1 bit per pixel, row-major, rows padded to bytes.
    pub bitmap: Vec<u8>,
}

/// The mask selecting column `x`'s bit within its packed byte.
///
/// Bit 7 is the leftmost pixel, so column `x` lives at bit `7 - x % 8`.
/// Shifting a mask down is the same thing without the subtraction, whose
/// non-negativity a reader would otherwise have to re-derive.
const fn bit_mask(x: u32) -> u8 {
    0x80_u8 >> (x % 8)
}

impl GlyphBitmap {
    /// Returns whether the pixel at (x, y) is set.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub fn pixel_at(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let row_bytes = self.width.div_ceil(8) as usize;
        let byte_idx = (y as usize)
            .saturating_mul(row_bytes)
            .saturating_add(x as usize / 8);
        self.bitmap
            .get(byte_idx)
            .is_some_and(|b| b & bit_mask(x) != 0)
    }

    /// Creates a scaled version of this glyph by an integer factor.
    ///
    /// A factor large enough to overflow the pixel count saturates rather than
    /// wrapping; the allocation below would fail long before that point, so
    /// saturating here only makes the failure honest instead of silently
    /// producing a bitmap with the wrong dimensions.
    pub fn scaled(&self, factor: u32) -> Self {
        let new_width = self.width.saturating_mul(factor);
        let new_height = self.height.saturating_mul(factor);
        let new_row_bytes = new_width.div_ceil(8) as usize;
        let mut new_bitmap = vec![0u8; new_row_bytes.saturating_mul(new_height as usize)];

        for y in 0..self.height {
            for x in 0..self.width {
                if self.pixel_at(x, y) {
                    for dy in 0..factor {
                        for dx in 0..factor {
                            let nx = x.saturating_mul(factor).saturating_add(dx);
                            let ny = y.saturating_mul(factor).saturating_add(dy);
                            let idx = (ny as usize)
                                .saturating_mul(new_row_bytes)
                                .saturating_add(nx as usize / 8);
                            if let Some(byte) = new_bitmap.get_mut(idx) {
                                *byte |= bit_mask(nx);
                            }
                        }
                    }
                }
            }
        }

        Self {
            width: new_width,
            height: new_height,
            advance: self.advance * factor as f32,
            bearing_x: self.bearing_x * factor as f32,
            bearing_y: self.bearing_y * factor as f32,
            bitmap: new_bitmap,
        }
    }
}

// ---------------------------------------------------------------------------
// Text alignment
// ---------------------------------------------------------------------------

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

// ---------------------------------------------------------------------------
// Font style
// ---------------------------------------------------------------------------

/// Font style variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

// ---------------------------------------------------------------------------
// Font
// ---------------------------------------------------------------------------

/// A font containing glyph bitmaps and associated metrics.
///
/// Fonts are created via factory methods (`system_mono`, `system_mono_bold`)
/// or by scaling an existing font. Glyph lookup falls back to a replacement
/// character for unmapped codepoints.
#[derive(Debug, Clone)]
pub struct Font {
    name: String,
    style: FontStyle,
    metrics: FontMetrics,
    /// Scale factor relative to the base 8x16 font (1 = native).
    scale: u32,
    /// Glyph storage, keyed by character.
    ///
    /// A map rather than a list because `glyph()` is called once per character
    /// drawn: a linear scan over the ~450 glyphs in the built-in face would put
    /// an O(n) probe on the hot text path for every letter on screen.
    glyphs: BTreeMap<char, GlyphBitmap>,
    /// Replacement glyph for missing codepoints.
    replacement: GlyphBitmap,
}

impl Font {
    /// Returns the built-in 8x16 monospace system font.
    ///
    /// Covers Basic Latin (U+0020..U+007E), box drawing (U+2500..U+257F),
    /// and block elements (U+2580..U+259F).
    pub fn system_mono() -> Self {
        build_system_font(FontStyle::Regular)
    }

    /// Returns the built-in 8x16 monospace bold system font.
    ///
    /// Same coverage as `system_mono` but with thicker strokes.
    pub fn system_mono_bold() -> Self {
        build_system_font(FontStyle::Bold)
    }

    /// Creates a scaled version of a font by an integer factor.
    ///
    /// A factor of 2 produces a 16x32 font, factor 3 produces 24x48, etc.
    pub fn scaled(base: &Font, scale: u32) -> Self {
        let scale = if scale == 0 { 1 } else { scale };
        let scaled_glyphs: BTreeMap<char, GlyphBitmap> = base
            .glyphs
            .iter()
            .map(|(ch, g)| (*ch, g.scaled(scale)))
            .collect();

        let s = scale as f32;
        Font {
            name: base.name.clone(),
            style: base.style,
            metrics: FontMetrics {
                ascent: base.metrics.ascent * s,
                descent: base.metrics.descent * s,
                line_height: base.metrics.line_height * s,
                max_advance: base.metrics.max_advance * s,
                average_advance: base.metrics.average_advance * s,
                cap_height: base.metrics.cap_height * s,
                x_height: base.metrics.x_height * s,
            },
            scale: base.scale.saturating_mul(scale),
            glyphs: scaled_glyphs,
            replacement: base.replacement.scaled(scale),
        }
    }

    /// Looks up the glyph for a character, returning the replacement glyph if not found.
    pub fn glyph(&self, ch: char) -> &GlyphBitmap {
        self.glyphs.get(&ch).unwrap_or(&self.replacement)
    }

    /// Returns the font's overall metrics.
    pub fn metrics(&self) -> &FontMetrics {
        &self.metrics
    }

    /// Returns the font style.
    pub fn style(&self) -> FontStyle {
        self.style
    }

    /// Returns the font name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the current scale factor.
    pub fn scale_factor(&self) -> u32 {
        self.scale
    }

    /// Measures the bounding box of a text string.
    ///
    /// Returns (width, height) in pixels. Handles newlines by stacking lines.
    pub fn measure(&self, text: &str) -> (f32, f32) {
        if text.is_empty() {
            return (0.0, 0.0);
        }

        let mut max_width: f32 = 0.0;
        let mut current_width: f32 = 0.0;
        let mut line_count: u32 = 1;

        for ch in text.chars() {
            if ch == '\n' {
                if current_width > max_width {
                    max_width = current_width;
                }
                current_width = 0.0;
                line_count = line_count.saturating_add(1);
            } else if ch == '\t' {
                let tab_width = self.metrics.average_advance * 4.0;
                current_width += tab_width;
            } else {
                let glyph = self.glyph(ch);
                current_width += glyph.advance;
            }
        }

        if current_width > max_width {
            max_width = current_width;
        }

        let height = self.metrics.line_height * line_count as f32;
        (max_width, height)
    }

    /// Measures the width of a single line of text (no newline handling).
    pub fn measure_line(&self, text: &str) -> f32 {
        let mut width: f32 = 0.0;
        for ch in text.chars() {
            if ch == '\t' {
                width += self.metrics.average_advance * 4.0;
            } else {
                let glyph = self.glyph(ch);
                width += glyph.advance;
            }
        }
        width
    }

    /// Returns the advance width for a single character.
    pub fn char_width(&self, ch: char) -> f32 {
        if ch == '\t' {
            self.metrics.average_advance * 4.0
        } else {
            self.glyph(ch).advance
        }
    }

    /// Computes the total height for a given number of lines.
    pub fn text_height(&self, lines: u32) -> f32 {
        self.metrics.line_height * lines as f32
    }
}

// ---------------------------------------------------------------------------
// Font family
// ---------------------------------------------------------------------------

/// A font family containing regular, bold, italic, and bold-italic variants.
#[derive(Debug, Clone)]
pub struct FontFamily {
    pub regular: Font,
    pub bold: Font,
    pub italic: Option<Font>,
    pub bold_italic: Option<Font>,
}

impl FontFamily {
    /// Creates the built-in system monospace font family.
    pub fn system_mono() -> Self {
        Self {
            regular: Font::system_mono(),
            bold: Font::system_mono_bold(),
            italic: None,
            bold_italic: None,
        }
    }

    /// Selects the appropriate font variant for a given style.
    pub fn variant(&self, style: FontStyle) -> &Font {
        match style {
            FontStyle::Regular => &self.regular,
            FontStyle::Bold => &self.bold,
            FontStyle::Italic => self.italic.as_ref().unwrap_or(&self.regular),
            FontStyle::BoldItalic => self.bold_italic.as_ref().unwrap_or(&self.bold),
        }
    }
}

// ---------------------------------------------------------------------------
// Text layout
// ---------------------------------------------------------------------------

/// A positioned glyph within a layout result.
#[derive(Debug, Clone)]
pub struct GlyphPosition {
    /// Horizontal position of the glyph origin.
    pub x: f32,
    /// Vertical position of the glyph baseline.
    pub y: f32,
    /// The character this glyph represents.
    pub character: char,
    /// Line number (0-indexed).
    pub line_number: u32,
}

/// The result of text layout computation.
#[derive(Debug, Clone)]
pub struct LayoutResult {
    /// Positioned glyphs ready for rendering.
    pub glyphs: Vec<GlyphPosition>,
    /// Total width of the laid-out text.
    pub total_width: f32,
    /// Total height of the laid-out text.
    pub total_height: f32,
    /// Number of lines produced.
    pub line_count: u32,
}

/// Text layout engine supporting word wrapping, alignment, and truncation.
///
/// # Example
///
/// ```ignore
/// let font = Font::system_mono();
/// let layout = TextLayout::new("Hello, world!", &font, 100.0);
/// let result = layout.compute();
/// for pos in &result.glyphs {
///     // render glyph at (pos.x, pos.y)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct TextLayout {
    text: String,
    font_metrics: FontMetrics,
    glyph_advances: Vec<(char, f32)>,
    max_width: f32,
    alignment: TextAlign,
    line_spacing: f32,
    tab_width: u32,
    single_line: bool,
    ellipsis: bool,
}

impl TextLayout {
    /// Creates a new text layout with the given text, font, and maximum width.
    ///
    /// Word wrapping occurs at whitespace boundaries when text exceeds `max_width`.
    pub fn new(text: &str, font: &Font, max_width: f32) -> Self {
        let glyph_advances: Vec<(char, f32)> =
            text.chars().map(|ch| (ch, font.char_width(ch))).collect();

        Self {
            text: String::from(text),
            font_metrics: font.metrics().clone(),
            glyph_advances,
            max_width,
            alignment: TextAlign::Left,
            line_spacing: 1.0,
            tab_width: 4,
            single_line: false,
            ellipsis: false,
        }
    }

    /// Sets the text alignment.
    pub fn with_alignment(mut self, align: TextAlign) -> Self {
        self.alignment = align;
        self
    }

    /// Sets the line spacing multiplier (1.0 = normal).
    pub fn with_line_spacing(mut self, spacing: f32) -> Self {
        self.line_spacing = spacing;
        self
    }

    /// Sets the tab width in number of spaces.
    pub fn with_tab_width(mut self, width: u32) -> Self {
        self.tab_width = if width == 0 { 1 } else { width };
        self
    }

    /// Enables single-line mode with optional ellipsis truncation.
    pub fn with_single_line(mut self, ellipsis: bool) -> Self {
        self.single_line = true;
        self.ellipsis = ellipsis;
        self
    }

    /// Computes the layout, positioning all glyphs.
    pub fn compute(&self) -> LayoutResult {
        if self.text.is_empty() {
            return LayoutResult {
                glyphs: Vec::new(),
                total_width: 0.0,
                total_height: 0.0,
                line_count: 0,
            };
        }

        if self.single_line {
            return self.compute_single_line();
        }

        self.compute_multiline()
    }

    fn compute_single_line(&self) -> LayoutResult {
        let line_height = self.font_metrics.line_height * self.line_spacing;
        let mut glyphs = Vec::new();
        let mut x: f32 = 0.0;
        let ellipsis_width = self.font_metrics.average_advance * 3.0;

        for &(ch, advance) in &self.glyph_advances {
            if ch == '\n' {
                break;
            }

            let effective_advance = if ch == '\t' {
                self.font_metrics.average_advance * self.tab_width as f32
            } else {
                advance
            };

            // Check if we need ellipsis truncation
            if self.ellipsis && x + effective_advance + ellipsis_width > self.max_width {
                // Add ellipsis dots
                let dot_advance = self.font_metrics.average_advance;
                for _ in 0..3 {
                    if x + dot_advance <= self.max_width {
                        glyphs.push(GlyphPosition {
                            x,
                            y: self.font_metrics.ascent,
                            character: '.',
                            line_number: 0,
                        });
                        x += dot_advance;
                    }
                }
                break;
            }

            if ch != '\t' && ch != '\n' {
                glyphs.push(GlyphPosition {
                    x,
                    y: self.font_metrics.ascent,
                    character: ch,
                    line_number: 0,
                });
            }
            x += effective_advance;
        }

        let total_width = x;
        let end = glyphs.len();
        self.apply_alignment(&mut glyphs, 0, end, total_width);

        LayoutResult {
            glyphs,
            total_width,
            total_height: line_height,
            line_count: 1,
        }
    }

    fn compute_multiline(&self) -> LayoutResult {
        let line_height = self.font_metrics.line_height * self.line_spacing;
        let mut glyphs = Vec::new();
        let mut x: f32 = 0.0;
        let mut line_number: u32 = 0;
        let mut line_start_idx: usize = 0;
        let mut word_start_idx: usize = 0;
        let mut word_start_x: f32 = 0.0;
        let mut max_line_width: f32 = 0.0;
        let mut in_word = false;

        for &(ch, advance) in &self.glyph_advances {
            if ch == '\n' {
                // Explicit line break
                let line_width = x;
                if line_width > max_line_width {
                    max_line_width = line_width;
                }
                let end = glyphs.len();
                self.apply_alignment(&mut glyphs, line_start_idx, end, line_width);
                line_number = line_number.saturating_add(1);
                x = 0.0;
                line_start_idx = glyphs.len();
                in_word = false;
                continue;
            }

            let effective_advance = if ch == '\t' {
                self.font_metrics.average_advance * self.tab_width as f32
            } else {
                advance
            };

            let is_whitespace = ch == ' ' || ch == '\t';

            if is_whitespace {
                if in_word {
                    in_word = false;
                }
                // Check for wrap at whitespace
                if x + effective_advance > self.max_width && x > 0.0 {
                    let line_width = x;
                    if line_width > max_line_width {
                        max_line_width = line_width;
                    }
                    let end = glyphs.len();
                    self.apply_alignment(&mut glyphs, line_start_idx, end, line_width);
                    line_number = line_number.saturating_add(1);
                    x = 0.0;
                    line_start_idx = glyphs.len();
                }
                x += effective_advance;
            } else {
                if !in_word {
                    in_word = true;
                    word_start_idx = glyphs.len();
                    word_start_x = x;
                }

                // Would this character exceed max_width?
                if x + effective_advance > self.max_width && x > 0.0 {
                    if word_start_x > 0.0 {
                        // Move entire word to next line
                        let line_width = word_start_x;
                        if line_width > max_line_width {
                            max_line_width = line_width;
                        }
                        self.apply_alignment(
                            &mut glyphs,
                            line_start_idx,
                            word_start_idx,
                            line_width,
                        );

                        // Reposition word glyphs to new line
                        line_number = line_number.saturating_add(1);
                        let y = self.font_metrics.ascent + line_height * line_number as f32;
                        let offset = word_start_x;
                        for glyph in glyphs.iter_mut().skip(word_start_idx) {
                            glyph.x -= offset;
                            glyph.y = y;
                            glyph.line_number = line_number;
                        }

                        x -= offset;
                        line_start_idx = word_start_idx;
                        word_start_x = 0.0;
                    } else {
                        // Word is at start of line and still too long — break mid-word
                        let line_width = x;
                        if line_width > max_line_width {
                            max_line_width = line_width;
                        }
                        let end = glyphs.len();
                        self.apply_alignment(&mut glyphs, line_start_idx, end, line_width);
                        line_number = line_number.saturating_add(1);
                        x = 0.0;
                        line_start_idx = glyphs.len();
                        word_start_idx = glyphs.len();
                        word_start_x = 0.0;
                    }
                }

                glyphs.push(GlyphPosition {
                    x,
                    y: self.font_metrics.ascent + line_height * line_number as f32,
                    character: ch,
                    line_number,
                });
                x += effective_advance;
            }
        }

        // Finalize last line
        let line_width = x;
        if line_width > max_line_width {
            max_line_width = line_width;
        }
        let end = glyphs.len();
        self.apply_alignment(&mut glyphs, line_start_idx, end, line_width);

        let line_count = line_number.saturating_add(1);
        LayoutResult {
            glyphs,
            total_width: max_line_width,
            total_height: line_height * line_count as f32,
            line_count,
        }
    }

    fn apply_alignment(
        &self,
        glyphs: &mut [GlyphPosition],
        start: usize,
        end: usize,
        line_width: f32,
    ) {
        let offset = match self.alignment {
            TextAlign::Left => 0.0,
            TextAlign::Center => (self.max_width - line_width) / 2.0,
            TextAlign::Right => self.max_width - line_width,
        };

        if offset > 0.0 {
            for glyph in glyphs
                .iter_mut()
                .skip(start)
                .take(end.saturating_sub(start))
            {
                glyph.x += offset;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Glyph rendering
// ---------------------------------------------------------------------------

/// Renders a glyph bitmap onto an ARGB pixel buffer.
///
/// # Arguments
///
/// * `glyph` - The glyph bitmap to render
/// * `buffer` - Mutable slice of the ARGB pixel buffer (32 bits per pixel)
/// * `x` - Horizontal position in the buffer (can be negative for clipping)
/// * `y` - Vertical position in the buffer (can be negative for clipping)
/// * `stride` - Number of pixels per row in the buffer
/// * `buf_height` - Height of the buffer in pixels
/// * `color` - ARGB color to use for foreground pixels
///
/// Pixels outside the buffer bounds are silently clipped.
#[allow(clippy::too_many_arguments)]
pub fn render_glyph_to_buffer(
    glyph: &GlyphBitmap,
    buffer: &mut [u32],
    x: i32,
    y: i32,
    stride: u32,
    buf_height: u32,
    color: u32,
) {
    let alpha = (color >> 24) & 0xFF;

    for gy in 0..glyph.height {
        // Saturating rather than wrapping: a glyph positioned absurdly far
        // off-screen must stay off-screen, and a wrapped coordinate would
        // land back inside the buffer and paint over unrelated pixels.
        let py = y.saturating_add(i32::try_from(gy).unwrap_or(i32::MAX));
        if py < 0 || py >= buf_height as i32 {
            continue;
        }

        for gx in 0..glyph.width {
            let px = x.saturating_add(i32::try_from(gx).unwrap_or(i32::MAX));
            if px < 0 || px >= stride as i32 {
                continue;
            }

            if glyph.pixel_at(gx, gy) {
                // Both coordinates were just proved non-negative and inside
                // the buffer's extent, so this is the flat index of a pixel
                // that exists; `get_mut` still checks, for free.
                let idx = (py as usize)
                    .saturating_mul(stride as usize)
                    .saturating_add(px as usize);
                if let Some(dest) = buffer.get_mut(idx) {
                    if alpha >= 255 {
                        *dest = color;
                    } else {
                        // Alpha blend
                        *dest = alpha_blend(*dest, color, alpha);
                    }
                }
            }
        }
    }
}

/// Renders text onto an ARGB pixel buffer using a font.
///
/// This is a convenience function that handles layout and rendering in one call.
#[allow(clippy::too_many_arguments)]
pub fn render_text_to_buffer(
    text: &str,
    font: &Font,
    buffer: &mut [u32],
    x: i32,
    y: i32,
    stride: u32,
    buf_height: u32,
    color: u32,
) {
    let mut pen_x = x as f32;
    let baseline_y = y as f32 + font.metrics().ascent;

    for ch in text.chars() {
        if ch == '\n' {
            break;
        }
        if ch == '\t' {
            pen_x += font.metrics().average_advance * 4.0;
            continue;
        }

        let glyph = font.glyph(ch);
        let gx = (pen_x + glyph.bearing_x) as i32;
        let gy = (baseline_y - glyph.bearing_y) as i32;

        render_glyph_to_buffer(glyph, buffer, gx, gy, stride, buf_height, color);
        pen_x += glyph.advance;
    }
}

/// Alpha-blends a source color onto a destination color.
fn alpha_blend(dest: u32, src: u32, src_alpha: u32) -> u32 {
    let alpha = src_alpha & 0xFF;

    let r = blend_channel((src >> 16) & 0xFF, (dest >> 16) & 0xFF, alpha);
    let g = blend_channel((src >> 8) & 0xFF, (dest >> 8) & 0xFF, alpha);
    let b = blend_channel(src & 0xFF, dest & 0xFF, alpha);

    0xFF00_0000 | (r << 16) | (g << 8) | b
}

/// Blends one 8-bit channel.
///
/// Every input is masked to 8 bits by its caller, so the weighted sum
/// `src * alpha + dst * (255 - alpha)` peaks at `65_025` — far inside a `u32`,
/// so it cannot overflow. The saturating forms state that invariant in the
/// code rather than leaving the reader to re-derive it.
fn blend_channel(src: u32, dst: u32, alpha: u32) -> u32 {
    let inv = 255_u32.saturating_sub(alpha);
    src.saturating_mul(alpha)
        .saturating_add(dst.saturating_mul(inv))
        / 255
}

// ---------------------------------------------------------------------------
// Built-in system font generation
// ---------------------------------------------------------------------------

/// The base width and height of the built-in system font.
const FONT_WIDTH: u32 = 8;
const FONT_HEIGHT: u32 = 16;

/// Wraps packed bitmap data as a glyph in the fixed 8x16 cell.
///
/// Every glyph in the built-in face has the same box and the same baseline —
/// it is a monospace bitmap font — so the geometry belongs in one place rather
/// than repeated at each range.
fn cell(bitmap: Vec<u8>) -> GlyphBitmap {
    GlyphBitmap {
        width: FONT_WIDTH,
        height: FONT_HEIGHT,
        advance: FONT_WIDTH as f32,
        bearing_x: 0.0,
        bearing_y: FONT_HEIGHT as f32 - 2.0, // baseline at row 14
        bitmap,
    }
}

fn build_system_font(style: FontStyle) -> Font {
    let mut glyphs: BTreeMap<char, GlyphBitmap> = BTreeMap::new();
    let bold = style == FontStyle::Bold;

    // Basic Latin (U+0020..U+007E)
    for codepoint in 0x20u32..=0x7E {
        if let Some(ch) = char::from_u32(codepoint) {
            glyphs.insert(ch, cell(generate_ascii_glyph(ch, bold)));
        }
    }

    // Box drawing characters (U+2500..U+257F)
    for codepoint in 0x2500u32..=0x257F {
        if let Some(ch) = char::from_u32(codepoint) {
            glyphs.insert(ch, cell(generate_box_drawing(ch)));
        }
    }

    // Block elements (U+2580..U+259F)
    for codepoint in 0x2580u32..=0x259F {
        if let Some(ch) = char::from_u32(codepoint) {
            glyphs.insert(ch, cell(generate_block_element(ch)));
        }
    }

    // Latin-1 Supplement stubs (render as replacement box)
    for codepoint in 0x00A0u32..=0x00FF {
        if let Some(ch) = char::from_u32(codepoint) {
            glyphs.insert(ch, cell(generate_replacement_glyph()));
        }
    }

    let replacement = cell(generate_replacement_glyph());

    Font {
        name: String::from("Slate OS Mono"),
        style,
        metrics: FontMetrics {
            ascent: 14.0,
            descent: 2.0,
            line_height: FONT_HEIGHT as f32,
            max_advance: FONT_WIDTH as f32,
            average_advance: FONT_WIDTH as f32,
            cap_height: 10.0,
            x_height: 7.0,
        },
        scale: 1,
        glyphs,
        replacement,
    }
}

/// The replacement glyph: a hollow rectangle with a question mark inside,
/// drawn for any character the built-in font has no art for.
const REPLACEMENT: GlyphArt = [
    "........", "........", ".######.", ".#....#.", ".#....#.", ".#.##.#.", ".#..#.#.", ".#..#.#.",
    ".#.#..#.", ".#....#.", ".#.#..#.", ".#....#.", ".#....#.", ".######.", "........", "........",
];

/// Generates the replacement glyph: a hollow rectangle.
fn generate_replacement_glyph() -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];
    paint(&REPLACEMENT, &mut rows);
    pack_bitmap(&rows)
}

/// Packs an 8x16 pixel grid into a 1-bit-per-pixel byte array.
fn pack_bitmap(rows: &[[u8; 8]; 16]) -> Vec<u8> {
    rows.iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .filter(|&(_, &pixel)| pixel != 0)
                .fold(0u8, |byte, (x, _)| byte | 0x80_u8 >> (x % 8))
        })
        .collect()
}

/// Generates a bitmap for a printable ASCII character.
fn generate_ascii_glyph(ch: char, bold: bool) -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];
    fill_ascii_glyph(ch, &mut rows);

    if bold {
        // Thicken strokes by smearing each row one pixel to the right. The
        // shifted-by-one zip reads the row as it was before this pass, so a
        // pixel the smear just wrote is not itself smeared again — otherwise a
        // single stem would bleed all the way to the right edge.
        for row in &mut rows {
            let original = *row;
            for (cell, &left) in row.iter_mut().skip(1).zip(original.iter()) {
                if left != 0 {
                    *cell = 1;
                }
            }
        }
    }

    pack_bitmap(&rows)
}

/// Fills the 8x16 pixel grid for a given ASCII character.
///
/// Characters are drawn in a coordinate system where:
/// - Row 0 is the top of the cell
/// - Row 13 is the baseline
/// - Rows 14-15 are below baseline (for descenders)
/// - Column 0 is leftmost, column 7 is rightmost
fn fill_ascii_glyph(ch: char, rows: &mut [[u8; 8]; 16]) {
    paint(ascii_art(ch), rows);
}

/// Paints pixel art onto a glyph grid.
///
/// Zipping rather than indexing means a short row or a short table leaves the
/// rest of the grid blank instead of panicking; `every_glyph_is_well_formed`
/// is the test that guarantees neither ever happens.
fn paint(art: &GlyphArt, rows: &mut [[u8; 8]; 16]) {
    for (row, line) in rows.iter_mut().zip(art.iter()) {
        for (cell, ink) in row.iter_mut().zip(line.bytes()) {
            *cell = u8::from(ink == b'#');
        }
    }
}

/// The 8x16 pixel art for one character: 16 rows of 8 columns, `#` meaning ink.
type GlyphArt = [&'static str; 16];

/// Every unmapped character renders as a filled block -- the traditional
/// "this font has no glyph for that" tofu.
const TOFU: GlyphArt = [
    "........", "........", "........", ".######.", ".######.", ".######.", ".######.", ".######.",
    ".######.", ".######.", ".######.", ".######.", ".######.", "........", "........", "........",
];

/// Looks up the pixel art for one ASCII character.
///
/// The glyphs are written as art rather than as a sequence of `rows[y][x] = 1`
/// assignments, because a hand-drawn font can only be reviewed by looking at
/// it -- and the imperative form additionally meant 450-odd unchecked index
/// writes into a fixed-size grid.
#[allow(clippy::too_many_lines)]
fn ascii_art(ch: char) -> &'static GlyphArt {
    match ch {
        ' ' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        '!' => &[
            "........", "........", "........", "...#....", "...#....", "...#....", "...#....",
            "...#....", "...#....", "...#....", "........", "........", "...#....", "........",
            "........", "........",
        ],
        '"' => &[
            "........", "........", "........", "..#..#..", "..#..#..", "..#..#..", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        '#' => &[
            "........", "........", "........", "..#..#..", "..#..#..", ".######.", "..#..#..",
            "..#..#..", "..#..#..", ".######.", "..#..#..", "..#..#..", "..#..#..", "........",
            "........", "........",
        ],
        '$' => &[
            "........", "........", "....#...", "..####..", "..#.#...", "..#.#...", "..#.#...",
            "..####..", "....##..", "....##..", "....##..", "..####..", "....#...", "........",
            "........", "........",
        ],
        '%' => &[
            "........", "........", "........", ".##.....", ".##.....", "..#.....", "...#....",
            "....#...", ".....#..", "......#.", "........", ".....##.", ".....##.", "........",
            "........", "........",
        ],
        '&' => &[
            "........", "........", "........", "..###...", ".#......", ".#......", ".#......",
            "..###...", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        '\'' => &[
            "........", "........", "........", "...#....", "...#....", "...#....", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        '(' => &[
            "........", "........", "........", "....#...", "...#....", "...#....", "...#....",
            "...#....", "...#....", "...#....", "...#....", "...#....", "...#....", "....#...",
            "........", "........",
        ],
        ')' => &[
            "........", "........", "........", "...#....", "....#...", "....#...", "....#...",
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "...#....",
            "........", "........",
        ],
        '*' => &[
            "........", "........", "........", "........", "...#....", "..###...", "...#....",
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        '+' => &[
            "........", "........", "........", "........", "........", "...#....", "...#....",
            "...#....", ".#####..", "...#....", "...#....", "........", "........", "........",
            "........", "........",
        ],
        ',' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........", "........", "........", "........", "...#....", "...#....",
            "..#.....", "........",
        ],
        '-' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", ".#####..", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        '.' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........", "........", "........", "........", "...#....", "........",
            "........", "........",
        ],
        '/' => &[
            "........", "........", "........", "......#.", "......#.", ".....#..", ".....#..",
            "....#...", "...#....", "...#....", "..#.....", "..#.....", ".#......", "........",
            "........", "........",
        ],
        '0' => &[
            "........", "........", "........", "..####..", ".#....#.", ".#...##.", ".#...##.",
            ".#..#.#.", ".#..#.#.", ".#.#..#.", ".#.#..#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        '1' => &[
            "........", "........", "........", "...##...", "...##...", "....#...", "....#...",
            "....#...", "....#...", "....#...", "....#...", "....#...", "..#####.", "........",
            "........", "........",
        ],
        '2' => &[
            "........", "........", "........", "..####..", "......#.", "......#.", "......#.",
            "......#.", "..####..", ".#......", ".#......", ".#......", "..####..", "........",
            "........", "........",
        ],
        '3' => &[
            "........", "........", "........", "..####..", "......#.", "......#.", "......#.",
            "..####..", "......#.", "......#.", "......#.", "......#.", "..####..", "........",
            "........", "........",
        ],
        '4' => &[
            "........", "........", "........", "..#..#..", "..#..#..", "..#..#..", "..#..#..",
            "..#..#..", ".######.", ".....#..", ".....#..", ".....#..", ".....#..", "........",
            "........", "........",
        ],
        '5' => &[
            "........", "........", "........", ".######.", ".#......", ".#......", ".#......",
            ".######.", "......#.", "......#.", "......#.", "......#.", ".######.", "........",
            "........", "........",
        ],
        '6' => &[
            "........", "........", "........", "..####..", ".#......", ".#......", ".#......",
            ".#####..", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        '7' => &[
            "........", "........", "........", ".######.", ".....#..", ".....#..", ".....#..",
            ".....#..", ".....#..", ".....#..", ".....#..", ".....#..", ".....#..", "........",
            "........", "........",
        ],
        '8' => &[
            "........", "........", "........", "..####..", ".#....#.", ".#....#.", ".#....#.",
            "..####..", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        '9' => &[
            "........", "........", "........", "..####..", ".#....#.", ".#....#.", ".#....#.",
            "..####..", "......#.", "......#.", "......#.", "......#.", "..####..", "........",
            "........", "........",
        ],
        ':' => &[
            "........", "........", "........", "........", "........", "...#....", "........",
            "........", "........", "........", "...#....", "........", "........", "........",
            "........", "........",
        ],
        ';' => &[
            "........", "........", "........", "........", "........", "...#....", "........",
            "........", "........", "........", "...#....", "..#.....", "........", "........",
            "........", "........",
        ],
        '<' => &[
            "........", "........", "........", "........", "........", ".....#..", "....#...",
            "...#....", "..#.....", "...#....", "....#...", ".....#..", "........", "........",
            "........", "........",
        ],
        '=' => &[
            "........", "........", "........", "........", "........", "........", ".#####..",
            "........", "........", ".#####..", "........", "........", "........", "........",
            "........", "........",
        ],
        '>' => &[
            "........", "........", "........", "........", "........", "..#.....", "...#....",
            "....#...", ".....#..", "....#...", "...#....", "..#.....", "........", "........",
            "........", "........",
        ],
        '?' => &[
            "........", "........", "........", "..####..", "......#.", "......#.", "........",
            ".....#..", "....#...", "...#....", "........", "........", "...#....", "........",
            "........", "........",
        ],
        '@' => &[
            "........", "........", "........", "..####..", ".#....#.", ".#....#.", ".#..###.",
            ".#..###.", ".#..##..", ".#..##..", ".#......", ".#......", "..####..", "........",
            "........", "........",
        ],
        'A' => &[
            "........", "........", "........", "..####..", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".######.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "........",
            "........", "........",
        ],
        'B' => &[
            "........", "........", "........", ".#####..", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#####..", ".#....#.", ".#....#.", ".#....#.", ".#####..", "........",
            "........", "........",
        ],
        'C' => &[
            "........", "........", "........", "..####..", ".#......", ".#......", ".#......",
            ".#......", ".#......", ".#......", ".#......", ".#......", "..####..", "........",
            "........", "........",
        ],
        'D' => &[
            "........", "........", "........", ".#####..", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#####..", "........",
            "........", "........",
        ],
        'E' => &[
            "........", "........", "........", ".######.", ".#......", ".#......", ".#......",
            ".#......", ".######.", ".#......", ".#......", ".#......", ".######.", "........",
            "........", "........",
        ],
        'F' => &[
            "........", "........", "........", ".######.", ".#......", ".#......", ".#......",
            ".#......", ".######.", ".#......", ".#......", ".#......", ".#......", "........",
            "........", "........",
        ],
        'G' => &[
            "........", "........", "........", "..####..", ".#......", ".#......", ".#......",
            ".#......", ".#..###.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        'H' => &[
            "........", "........", "........", ".#....#.", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".######.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "........",
            "........", "........",
        ],
        'I' => &[
            "........", "........", "........", "..####..", "....#...", "....#...", "....#...",
            "....#...", "....#...", "....#...", "....#...", "....#...", "..####..", "........",
            "........", "........",
        ],
        'J' => &[
            "........", "........", "........", ".....#..", ".....#..", ".....#..", ".....#..",
            ".....#..", ".....#..", ".....#..", ".....#..", ".#...#..", "..###...", "........",
            "........", "........",
        ],
        'K' => &[
            "........", "........", "........", ".#......", ".#......", ".#...#..", ".#..#...",
            ".#.#....", ".##.....", ".#.#....", ".#..#...", ".#...#..", ".#....#.", "........",
            "........", "........",
        ],
        'L' => &[
            "........", "........", "........", ".#......", ".#......", ".#......", ".#......",
            ".#......", ".#......", ".#......", ".#......", ".#......", ".######.", "........",
            "........", "........",
        ],
        'M' => &[
            "........", "........", "........", ".#....#.", ".##..##.", ".#.##.#.", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "........",
            "........", "........",
        ],
        'N' => &[
            "........", "........", "........", ".#....#.", ".##...#.", ".#.#..#.", ".#.#..#.",
            ".#..#.#.", ".#..#.#.", ".#...##.", ".#....#.", ".#....#.", ".#....#.", "........",
            "........", "........",
        ],
        'O' => &[
            "........", "........", "........", "..####..", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        'P' => &[
            "........", "........", "........", ".#####..", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#####..", ".#......", ".#......", ".#......", ".#......", "........",
            "........", "........",
        ],
        'Q' => &[
            "........", "........", "........", "..####..", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", ".#..#.#.", ".#...##.", "..#####.", "........",
            "........", "........",
        ],
        'R' => &[
            "........", "........", "........", ".#####..", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#####..", ".#..#...", ".#...#..", ".#...#..", ".#....#.", "........",
            "........", "........",
        ],
        'S' => &[
            "........", "........", "........", "..####..", ".#......", ".#......", ".#......",
            ".#......", "..####..", "......#.", "......#.", "......#.", "..####..", "........",
            "........", "........",
        ],
        'T' => &[
            "........", "........", "........", ".######.", "....#...", "....#...", "....#...",
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "........",
            "........", "........",
        ],
        'U' => &[
            "........", "........", "........", ".#....#.", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        'V' => &[
            "........", "........", "........", ".#....#.", ".#....#.", ".#....#.", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", "..#..#..", "...##...", "...##...", "........",
            "........", "........",
        ],
        'W' => &[
            "........", "........", "........", ".#....#.", ".#....#.", ".#....#.", ".#....#.",
            ".#..#.#.", ".#..#.#.", ".#..#.#.", ".#..#.#.", "..#..#..", "...##...", "........",
            "........", "........",
        ],
        'X' => &[
            "........", "........", "........", ".#....#.", ".#....#.", ".#....#.", ".#....#.",
            "..#..#..", "...##...", "..#..#..", ".#....#.", ".#....#.", ".#....#.", "........",
            "........", "........",
        ],
        'Y' => &[
            "........", "........", "........", ".#....#.", ".#....#.", ".#....#.", "..#..#..",
            "...##...", "....#...", "....#...", "....#...", "....#...", "....#...", "........",
            "........", "........",
        ],
        'Z' => &[
            "........", "........", "........", ".######.", "......#.", ".....#..", ".....#..",
            "....#...", "...#....", "...#....", "..#.....", ".#......", ".######.", "........",
            "........", "........",
        ],
        '[' => &[
            "........", "........", "........", "...###..", "...#....", "...#....", "...#....",
            "...#....", "...#....", "...#....", "...#....", "...#....", "...#....", "...###..",
            "........", "........",
        ],
        '\\' => &[
            "........", "........", "........", ".#......", ".#......", "..#.....", "..#.....",
            "...#....", "....#...", "....#...", ".....#..", ".....#..", "......#.", "........",
            "........", "........",
        ],
        ']' => &[
            "........", "........", "........", "..###...", "....#...", "....#...", "....#...",
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "..###...",
            "........", "........",
        ],
        '^' => &[
            "........", "........", "........", "....#...", "...#.#..", "..#...#.", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        '_' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "########", "........",
        ],
        '`' => &[
            "........", "........", "........", "...#....", "....#...", "........", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        'a' => &[
            "........", "........", "........", "........", "........", "........", "..####..",
            "......#.", "..#####.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        'b' => &[
            "........", "........", "........", ".#......", ".#......", ".#......", ".#####..",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#####..", "........",
            "........", "........",
        ],
        'c' => &[
            "........", "........", "........", "........", "........", "........", "..####..",
            ".#......", ".#......", ".#......", ".#......", ".#......", "..####..", "........",
            "........", "........",
        ],
        'd' => &[
            "........", "........", "........", "......#.", "......#.", "......#.", "..#####.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..#####.", "........",
            "........", "........",
        ],
        'e' => &[
            "........", "........", "........", "........", "........", "........", "..####..",
            ".#....#.", ".#....#.", ".######.", ".#......", ".#......", "..####..", "........",
            "........", "........",
        ],
        'f' => &[
            "........", "........", "........", "...###..", "...#....", "...#....", "..####..",
            "...#....", "...#....", "...#....", "...#....", "...#....", "...#....", "........",
            "........", "........",
        ],
        'g' => &[
            "........", "........", "........", "........", "........", "........", "..####..",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..####..", "......#.",
            "......#.", "..####..",
        ],
        'h' => &[
            "........", "........", "........", ".#......", ".#......", ".#......", ".#####..",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "........",
            "........", "........",
        ],
        'i' => &[
            "........", "........", "........", "........", "...#....", "........", "...#....",
            "...#....", "...#....", "...#....", "...#....", "...#....", "...#....", "........",
            "........", "........",
        ],
        'j' => &[
            "........", "........", "........", "........", ".....#..", "........", ".....#..",
            ".....#..", ".....#..", ".....#..", ".....#..", ".....#..", ".....#..", ".....#..",
            "..###...", "........",
        ],
        'k' => &[
            "........", "........", "........", ".#......", ".#......", ".#......", ".#......",
            ".#...#..", ".#..#...", ".#.#....", ".#..#...", ".#...#..", ".#....#.", "........",
            "........", "........",
        ],
        'l' => &[
            "........", "........", "........", "...#....", "...#....", "...#....", "...#....",
            "...#....", "...#....", "...#....", "...#....", "...#....", "....##..", "........",
            "........", "........",
        ],
        'm' => &[
            "........", "........", "........", "........", "........", "........", ".#######",
            ".#..#..#", ".#..#..#", ".#..#..#", ".#..#..#", ".#..#..#", ".#..#..#", "........",
            "........", "........",
        ],
        'n' => &[
            "........", "........", "........", "........", "........", "........", ".######.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "........",
            "........", "........",
        ],
        'o' => &[
            "........", "........", "........", "........", "........", "........", "..####..",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..####..", "........",
            "........", "........",
        ],
        'p' => &[
            "........", "........", "........", "........", "........", "........", ".#####..",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#####..", ".#......",
            ".#......", "........",
        ],
        'q' => &[
            "........", "........", "........", "........", "........", "........", "..#####.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..#####.", "......#.",
            "......#.", "........",
        ],
        'r' => &[
            "........", "........", "........", "........", "........", "........", "..####..",
            "..#...#.", "..#.....", "..#.....", "..#.....", "..#.....", "..#.....", "........",
            "........", "........",
        ],
        's' => &[
            "........", "........", "........", "........", "........", "........", "..####..",
            ".#......", ".#......", "..####..", "......#.", "......#.", "..####..", "........",
            "........", "........",
        ],
        't' => &[
            "........", "........", "........", "...#....", "...#....", "...#....", "..####..",
            "...#....", "...#....", "...#....", "...#....", "...#....", "....##..", "........",
            "........", "........",
        ],
        'u' => &[
            "........", "........", "........", "........", "........", "........", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..#####.", "........",
            "........", "........",
        ],
        'v' => &[
            "........", "........", "........", "........", "........", "........", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", "..#..#..", "...##...", "...#....", "........",
            "........", "........",
        ],
        'w' => &[
            "........", "........", "........", "........", "........", "........", ".#..#..#",
            ".#..#..#", ".#..#..#", ".#..#..#", "..#...#.", "...#.#..", "....#...", "........",
            "........", "........",
        ],
        'x' => &[
            "........", "........", "........", "........", "........", "........", ".#....#.",
            "..#..#..", "...##...", "...##...", "..#..#..", ".#....#.", "........", "........",
            "........", "........",
        ],
        'y' => &[
            "........", "........", "........", "........", "........", "........", ".#....#.",
            ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", "..#####.", "......#.",
            "......#.", "..####..",
        ],
        'z' => &[
            "........", "........", "........", "........", "........", "........", ".######.",
            ".....#..", "....#...", "....#...", "...#....", "..#.....", ".######.", "........",
            "........", "........",
        ],
        '{' => &[
            "........", "........", "........", ".....#..", "....#...", "....#...", "....#...",
            "....#...", "...#....", "....#...", "....#...", "....#...", "....#...", ".....#..",
            "........", "........",
        ],
        '|' => &[
            "........", "........", "........", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "........", "........",
        ],
        '}' => &[
            "........", "........", "........", "...#....", "....#...", "....#...", "....#...",
            "....#...", ".....#..", "....#...", "....#...", "....#...", "....#...", "...#....",
            "........", "........",
        ],
        '~' => &[
            "........", "........", "........", "........", "........", "........", "...##.#.",
            "..#..#..", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        _ => &TOFU,
    }
}

// ---------------------------------------------------------------------------
// Box drawing character generation (U+2500..U+257F)
// ---------------------------------------------------------------------------

/// Generates a box drawing character bitmap.
///
/// Matching on the character itself rather than on `ch as u32 - 0x2500` keeps
/// the source readable and removes an underflow that only the caller's choice
/// of range was preventing.
fn generate_box_drawing(ch: char) -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];
    paint(box_art(ch), &mut rows);
    pack_bitmap(&rows)
}

/// The pixel art for one box drawing character.
///
/// Anything in the block that is not drawn yet falls back to the cross, which
/// at least keeps a table's lines connected.
#[allow(clippy::too_many_lines)]
fn box_art(ch: char) -> &'static GlyphArt {
    match ch {
        // horizontal
        '\u{2500}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "########", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // heavy horizontal
        '\u{2501}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "########", "########", "########", "........", "........", "........", "........",
            "........", "........",
        ],
        // vertical
        '\u{2502}' => &[
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...",
        ],
        // heavy vertical
        '\u{2503}' => &[
            "...###..", "...###..", "...###..", "...###..", "...###..", "...###..", "...###..",
            "...###..", "...###..", "...###..", "...###..", "...###..", "...###..", "...###..",
            "...###..", "...###..",
        ],
        // top-left corner
        '\u{250c}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "....####", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...",
        ],
        // top-right corner
        '\u{2510}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "#####...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...",
        ],
        // bottom-left corner
        '\u{2514}' => &[
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....####", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // bottom-right corner
        '\u{2518}' => &[
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "#####...", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // left tee
        '\u{251c}' => &[
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....####", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...",
        ],
        // right tee
        '\u{2524}' => &[
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "#####...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...",
        ],
        // top tee
        '\u{252c}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "########", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...",
        ],
        // bottom tee
        '\u{2534}' => &[
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "########", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // cross
        '\u{253c}' => &[
            "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "########", "....#...", "....#...", "....#...", "....#...", "....#...",
            "....#...", "....#...",
        ],
        // double top-left
        '\u{2550}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "...#####", "...#.#..", "...#####", "...#.#..", "...#.#..", "...#.#..", "...#.#..",
            "...#.#..", "...#.#..",
        ],
        // double top-right
        '\u{2551}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "######..", "...#.#..", "######..", "...#.#..", "...#.#..", "...#.#..", "...#.#..",
            "...#.#..", "...#.#..",
        ],
        // double bottom-left
        '\u{2554}' => &[
            "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..",
            "...#####", "...#.#..", "...#####", "........", "........", "........", "........",
            "........", "........",
        ],
        // double bottom-right
        '\u{2555}' => &[
            "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..",
            "######..", "...#.#..", "######..", "........", "........", "........", "........",
            "........", "........",
        ],
        // double cross
        '\u{255e}' => &[
            "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..", "...#.#..",
            "###.#.##", "...#.#..", "###.#.##", "...#.#..", "...#.#..", "...#.#..", "...#.#..",
            "...#.#..", "...#.#..",
        ],
        _ => &BOX_CROSS,
    }
}

/// The fallback for a box drawing character that has no art yet.
const BOX_CROSS: GlyphArt = [
    "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
    "########", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...", "....#...",
];

// ---------------------------------------------------------------------------
// Block element generation (U+2580..U+259F)
// ---------------------------------------------------------------------------

/// Generates a block element character bitmap.
///
/// Block elements are filled rectangles covering portions of the cell.
fn generate_block_element(ch: char) -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];
    paint(block_art(ch), &mut rows);
    pack_bitmap(&rows)
}

/// The pixel art for one block element character.
#[allow(clippy::too_many_lines)]
fn block_art(ch: char) -> &'static GlyphArt {
    match ch {
        // upper half block
        '\u{2580}' => &[
            "########", "########", "########", "########", "########", "########", "########",
            "########", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // lower one-eighth block
        '\u{2581}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "########", "########",
        ],
        // lower one-quarter block
        '\u{2582}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........", "........", "........", "........", "########", "########",
            "########", "########",
        ],
        // lower three-eighths block
        '\u{2583}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........", "........", "########", "########", "########", "########",
            "########", "########",
        ],
        // lower half block
        '\u{2584}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "########", "########", "########", "########", "########", "########",
            "########", "########",
        ],
        // lower five-eighths block
        '\u{2585}' => &[
            "........", "........", "........", "........", "........", "........", "########",
            "########", "########", "########", "########", "########", "########", "########",
            "########", "########",
        ],
        // lower three-quarters block
        '\u{2586}' => &[
            "........", "........", "........", "........", "########", "########", "########",
            "########", "########", "########", "########", "########", "########", "########",
            "########", "########",
        ],
        // lower seven-eighths block
        '\u{2587}' => &[
            "........", "........", "########", "########", "########", "########", "########",
            "########", "########", "########", "########", "########", "########", "########",
            "########", "########",
        ],
        // full block
        '\u{2588}' => &[
            "########", "########", "########", "########", "########", "########", "########",
            "########", "########", "########", "########", "########", "########", "########",
            "########", "########",
        ],
        // left seven-eighths block
        '\u{2589}' => &[
            "#######.", "#######.", "#######.", "#######.", "#######.", "#######.", "#######.",
            "#######.", "#######.", "#######.", "#######.", "#######.", "#######.", "#######.",
            "#######.", "#######.",
        ],
        // left three-quarters block
        '\u{258a}' => &[
            "######..", "######..", "######..", "######..", "######..", "######..", "######..",
            "######..", "######..", "######..", "######..", "######..", "######..", "######..",
            "######..", "######..",
        ],
        // left five-eighths block
        '\u{258b}' => &[
            "#####...", "#####...", "#####...", "#####...", "#####...", "#####...", "#####...",
            "#####...", "#####...", "#####...", "#####...", "#####...", "#####...", "#####...",
            "#####...", "#####...",
        ],
        // left half block
        '\u{258c}' => &[
            "####....", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "####....",
        ],
        // left three-eighths block
        '\u{258d}' => &[
            "###.....", "###.....", "###.....", "###.....", "###.....", "###.....", "###.....",
            "###.....", "###.....", "###.....", "###.....", "###.....", "###.....", "###.....",
            "###.....", "###.....",
        ],
        // left one-quarter block
        '\u{258e}' => &[
            "##......", "##......", "##......", "##......", "##......", "##......", "##......",
            "##......", "##......", "##......", "##......", "##......", "##......", "##......",
            "##......", "##......",
        ],
        // left one-eighth block
        '\u{258f}' => &[
            "#.......", "#.......", "#.......", "#.......", "#.......", "#.......", "#.......",
            "#.......", "#.......", "#.......", "#.......", "#.......", "#.......", "#.......",
            "#.......", "#.......",
        ],
        // right half block
        '\u{2590}' => &[
            "....####", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "....####",
        ],
        // light shade block
        '\u{2591}' => &[
            "#...#...", "...#...#", "..#...#.", ".#...#..", "#...#...", "...#...#", "..#...#.",
            ".#...#..", "#...#...", "...#...#", "..#...#.", ".#...#..", "#...#...", "...#...#",
            "..#...#.", ".#...#..",
        ],
        // medium shade block
        '\u{2592}' => &[
            "#.#.#.#.", ".#.#.#.#", "#.#.#.#.", ".#.#.#.#", "#.#.#.#.", ".#.#.#.#", "#.#.#.#.",
            ".#.#.#.#", "#.#.#.#.", ".#.#.#.#", "#.#.#.#.", ".#.#.#.#", "#.#.#.#.", ".#.#.#.#",
            "#.#.#.#.", ".#.#.#.#",
        ],
        // dark shade block
        '\u{2593}' => &[
            ".###.###", "###.###.", "##.###.#", "#.###.##", ".###.###", "###.###.", "##.###.#",
            "#.###.##", ".###.###", "###.###.", "##.###.#", "#.###.##", ".###.###", "###.###.",
            "##.###.#", "#.###.##",
        ],
        // upper one-eighth block
        '\u{2594}' => &[
            "########", "########", "........", "........", "........", "........", "........",
            "........", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // right one-eighth block
        '\u{2595}' => &[
            ".......#", ".......#", ".......#", ".......#", ".......#", ".......#", ".......#",
            ".......#", ".......#", ".......#", ".......#", ".......#", ".......#", ".......#",
            ".......#", ".......#",
        ],
        // quadrant lower left block
        '\u{2596}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "####....",
        ],
        // quadrant lower right block
        '\u{2597}' => &[
            "........", "........", "........", "........", "........", "........", "........",
            "........", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "....####",
        ],
        // quadrant upper left block
        '\u{2598}' => &[
            "####....", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // quadrant upper left + lower left + lower right block
        '\u{2599}' => &[
            "####....", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "########", "########", "########", "########", "########", "########",
            "########", "########",
        ],
        // quadrant upper left + lower right block
        '\u{259a}' => &[
            "####....", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "....####",
        ],
        // quadrant upper left + upper right + lower left block
        '\u{259b}' => &[
            "########", "########", "########", "########", "########", "########", "########",
            "########", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "####....",
        ],
        // quadrant upper left + upper right + lower right block
        '\u{259c}' => &[
            "########", "########", "########", "########", "########", "########", "########",
            "########", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "....####",
        ],
        // quadrant upper right block
        '\u{259d}' => &[
            "....####", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "........", "........", "........", "........", "........", "........",
            "........", "........",
        ],
        // quadrant upper right + lower left block
        '\u{259e}' => &[
            "....####", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "####....", "####....", "####....", "####....", "####....", "####....",
            "####....", "####....",
        ],
        // quadrant upper right + lower left + lower right block
        '\u{259f}' => &[
            "....####", "....####", "....####", "....####", "....####", "....####", "....####",
            "....####", "########", "########", "########", "########", "########", "########",
            "########", "########",
        ],
        _ => &BLANK,
    }
}

/// An entirely empty cell.
const BLANK: GlyphArt = [
    "........", "........", "........", "........", "........", "........", "........", "........",
    "........", "........", "........", "........", "........", "........", "........", "........",
];

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

// A test asserting an exact metric *wants* `==`, and a test that indexes past
// the end of its own fixture *should* panic — that is the failure being
// reported, not a defect to guard against.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// `paint` zips rather than indexes, so a row that is too short or a table
    /// with too few rows would silently leave part of a glyph blank instead of
    /// failing. This is the check that makes that impossible: it is the only
    /// thing standing between a typo in the art and a quietly truncated letter.
    #[test]
    fn every_glyph_is_well_formed() {
        let mut checked = 0usize;
        let mut check = |what: &str, art: &GlyphArt| {
            assert_eq!(art.len(), 16, "{what}: wrong row count");
            for (y, row) in art.iter().enumerate() {
                assert_eq!(row.len(), 8, "{what}: row {y} is {:?}", row);
                assert!(
                    row.bytes().all(|b| b == b'#' || b == b'.'),
                    "{what}: row {y} has a character that is neither ink nor gap: {row:?}"
                );
            }
            checked += 1;
        };

        for code in 0x20u32..=0x7E {
            let ch = char::from_u32(code).expect("printable ASCII is a valid char");
            check(&format!("U+{code:04X}"), ascii_art(ch));
        }
        for code in 0x2500u32..=0x257F {
            let ch = char::from_u32(code).expect("box drawing is a valid char");
            check(&format!("U+{code:04X}"), box_art(ch));
        }
        for code in 0x2580u32..=0x259F {
            let ch = char::from_u32(code).expect("block elements are valid chars");
            check(&format!("U+{code:04X}"), block_art(ch));
        }
        check("replacement", &REPLACEMENT);
        check("tofu", &TOFU);

        // 95 printable ASCII + 128 box drawing + 32 block elements + 2 spares.
        assert_eq!(checked, 257);
    }

    /// The art tables replaced hand-written `rows[y][x] = 1` sequences. These
    /// three glyphs are the ones whose shapes the rest of the suite leans on,
    /// so they are pinned exactly rather than merely "renders something".
    #[test]
    fn art_matches_the_shapes_it_replaced() {
        // 'l' is a stem in column 3, rows 3..=12, with a foot turning right.
        assert_eq!(
            ascii_art('l'),
            &[
                "........", "........", "........", "...#....", "...#....", "...#....", "...#....",
                "...#....", "...#....", "...#....", "...#....", "...#....", "....##..", "........",
                "........", "........",
            ]
        );
        // The full block is solid; the left half block stops at column 3.
        assert!(block_art('\u{2588}').iter().all(|r| *r == "########"));
        assert!(block_art('\u{258C}').iter().all(|r| *r == "####...."));
        // A horizontal rule sits on the vertical centre line, row 8.
        assert_eq!(box_art('\u{2500}')[8], "########");
        assert_eq!(box_art('\u{2500}')[7], "........");
    }

    #[test]
    fn test_system_font_creation() {
        let font = Font::system_mono();
        assert_eq!(font.name(), "Slate OS Mono");
        assert_eq!(font.style(), FontStyle::Regular);
        assert_eq!(font.scale_factor(), 1);
    }

    #[test]
    fn test_font_metrics() {
        let font = Font::system_mono();
        let m = font.metrics();
        assert_eq!(m.ascent, 14.0);
        assert_eq!(m.descent, 2.0);
        assert_eq!(m.line_height, 16.0);
        assert_eq!(m.max_advance, 8.0);
    }

    #[test]
    fn test_measure_empty() {
        let font = Font::system_mono();
        let (w, h) = font.measure("");
        assert_eq!(w, 0.0);
        assert_eq!(h, 0.0);
    }

    #[test]
    fn test_measure_single_char() {
        let font = Font::system_mono();
        let (w, h) = font.measure("A");
        assert_eq!(w, 8.0);
        assert_eq!(h, 16.0);
    }

    #[test]
    fn test_measure_multiline() {
        let font = Font::system_mono();
        let (w, h) = font.measure("AB\nCDE");
        assert_eq!(w, 24.0); // "CDE" is widest
        assert_eq!(h, 32.0); // 2 lines * 16
    }

    #[test]
    fn test_measure_line_width() {
        let font = Font::system_mono();
        let w = font.measure_line("Hello");
        assert_eq!(w, 40.0); // 5 * 8
    }

    #[test]
    fn test_char_width_monospace() {
        let font = Font::system_mono();
        assert_eq!(font.char_width('A'), 8.0);
        assert_eq!(font.char_width('z'), 8.0);
        assert_eq!(font.char_width(' '), 8.0);
    }

    #[test]
    fn test_text_height() {
        let font = Font::system_mono();
        assert_eq!(font.text_height(1), 16.0);
        assert_eq!(font.text_height(3), 48.0);
    }

    #[test]
    fn test_glyph_lookup() {
        let font = Font::system_mono();
        let g = font.glyph('A');
        assert_eq!(g.width, 8);
        assert_eq!(g.height, 16);
        assert_eq!(g.advance, 8.0);
    }

    #[test]
    fn test_replacement_glyph() {
        let font = Font::system_mono();
        // A character outside our coverage should return the replacement glyph
        let g = font.glyph('\u{1F600}'); // emoji, not covered
        assert_eq!(g.width, 8);
        assert_eq!(g.height, 16);
    }

    #[test]
    fn test_layout_basic() {
        let font = Font::system_mono();
        let layout = TextLayout::new("Hi", &font, 100.0);
        let result = layout.compute();
        assert_eq!(result.glyphs.len(), 2);
        assert_eq!(result.line_count, 1);
        assert_eq!(result.glyphs[0].character, 'H');
        assert_eq!(result.glyphs[1].character, 'i');
    }

    #[test]
    fn test_layout_word_wrap() {
        let font = Font::system_mono();
        // max_width 32px = 4 chars. "Hello World" should wrap.
        let layout = TextLayout::new("Hello World", &font, 32.0);
        let result = layout.compute();
        assert!(result.line_count >= 2);
    }

    #[test]
    fn test_layout_explicit_newline() {
        let font = Font::system_mono();
        let layout = TextLayout::new("AB\nCD", &font, 200.0);
        let result = layout.compute();
        assert_eq!(result.line_count, 2);
        // First line has A, B
        let line0: Vec<&GlyphPosition> = result
            .glyphs
            .iter()
            .filter(|g| g.line_number == 0)
            .collect();
        let line1: Vec<&GlyphPosition> = result
            .glyphs
            .iter()
            .filter(|g| g.line_number == 1)
            .collect();
        assert_eq!(line0.len(), 2);
        assert_eq!(line1.len(), 2);
    }

    #[test]
    fn test_layout_alignment_center() {
        let font = Font::system_mono();
        // "AB" = 16px wide, max_width = 100
        let layout = TextLayout::new("AB", &font, 100.0).with_alignment(TextAlign::Center);
        let result = layout.compute();
        // Centered offset should be (100 - 16) / 2 = 42
        assert!((result.glyphs[0].x - 42.0).abs() < 0.1);
    }

    #[test]
    fn test_layout_alignment_right() {
        let font = Font::system_mono();
        // "AB" = 16px wide, max_width = 100
        let layout = TextLayout::new("AB", &font, 100.0).with_alignment(TextAlign::Right);
        let result = layout.compute();
        // Right-aligned offset should be 100 - 16 = 84
        assert!((result.glyphs[0].x - 84.0).abs() < 0.1);
    }

    #[test]
    fn test_layout_single_line_ellipsis() {
        let font = Font::system_mono();
        // max_width 40px = 5 chars, text is longer
        let layout = TextLayout::new("Hello World", &font, 40.0).with_single_line(true);
        let result = layout.compute();
        // Should have been truncated with "..."
        let has_dots = result.glyphs.iter().any(|g| g.character == '.');
        assert!(has_dots);
    }

    #[test]
    fn test_glyph_pixel_at() {
        let font = Font::system_mono();
        let g = font.glyph('A');
        // 'A' has pixels set (it's not blank)
        let has_any_pixel = (0..g.height).any(|y| (0..g.width).any(|x| g.pixel_at(x, y)));
        assert!(has_any_pixel);
    }

    #[test]
    fn test_glyph_pixel_at_oob() {
        let font = Font::system_mono();
        let g = font.glyph('A');
        assert!(!g.pixel_at(100, 100));
        assert!(!g.pixel_at(8, 0));
        assert!(!g.pixel_at(0, 16));
    }

    #[test]
    fn test_scaled_font() {
        let base = Font::system_mono();
        let scaled = Font::scaled(&base, 2);
        assert_eq!(scaled.scale_factor(), 2);
        assert_eq!(scaled.metrics().line_height, 32.0);
        assert_eq!(scaled.metrics().max_advance, 16.0);
        let g = scaled.glyph('A');
        assert_eq!(g.width, 16);
        assert_eq!(g.height, 32);
    }

    #[test]
    fn test_render_glyph_to_buffer() {
        let font = Font::system_mono();
        let g = font.glyph('X');
        let stride = 32u32;
        let height = 32u32;
        let mut buffer = vec![0u32; (stride * height) as usize];
        let color = 0xFFFF_FFFFu32; // opaque white

        render_glyph_to_buffer(g, &mut buffer, 0, 0, stride, height, color);

        // At least one pixel should have been written
        let written = buffer.iter().any(|&p| p != 0);
        assert!(written);
    }

    #[test]
    fn test_box_drawing_horizontal() {
        let font = Font::system_mono();
        let g = font.glyph('\u{2500}'); // ─
        // Row 8 (center) should have pixels set
        let center_has_pixels = (0..g.width).any(|x| g.pixel_at(x, 8));
        assert!(center_has_pixels);
    }

    #[test]
    fn test_block_element_full_block() {
        let font = Font::system_mono();
        let g = font.glyph('\u{2588}'); // █ full block
        // Every pixel should be set
        for y in 0..g.height {
            for x in 0..g.width {
                assert!(
                    g.pixel_at(x, y),
                    "pixel ({x}, {y}) should be set in full block"
                );
            }
        }
    }

    #[test]
    fn test_block_element_upper_half() {
        let font = Font::system_mono();
        let g = font.glyph('\u{2580}'); // ▀ upper half block
        // Upper half (rows 0..8) should be set
        for y in 0..8 {
            for x in 0..g.width {
                assert!(
                    g.pixel_at(x, y),
                    "pixel ({x}, {y}) should be set in upper half"
                );
            }
        }
        // Lower half (rows 8..16) should be clear
        for y in 8..16 {
            for x in 0..g.width {
                assert!(
                    !g.pixel_at(x, y),
                    "pixel ({x}, {y}) should be clear in upper half"
                );
            }
        }
    }

    #[test]
    fn test_bold_font_thicker() {
        let regular = Font::system_mono();
        let bold = Font::system_mono_bold();
        let reg_a = regular.glyph('A');
        let bold_a = bold.glyph('A');

        // Count set pixels — bold should have more
        let count_pixels = |g: &GlyphBitmap| -> u32 {
            let mut count = 0u32;
            for y in 0..g.height {
                for x in 0..g.width {
                    if g.pixel_at(x, y) {
                        count += 1;
                    }
                }
            }
            count
        };

        let reg_count = count_pixels(reg_a);
        let bold_count = count_pixels(bold_a);
        assert!(
            bold_count > reg_count,
            "bold should have more pixels than regular"
        );
    }

    #[test]
    fn test_alpha_blend() {
        // Fully opaque source should overwrite destination
        let result = alpha_blend(0xFF00_0000, 0xFFFF_FFFF, 255);
        assert_eq!(result, 0xFFFF_FFFF);

        // Fully transparent source should leave destination (approximately)
        let result = alpha_blend(0xFF80_8080, 0xFF00_0000, 0);
        // With 0 alpha, result should be close to dest
        let r = (result >> 16) & 0xFF;
        let g = (result >> 8) & 0xFF;
        let b = result & 0xFF;
        assert_eq!(r, 0x80);
        assert_eq!(g, 0x80);
        assert_eq!(b, 0x80);
    }

    #[test]
    fn test_font_family() {
        let family = FontFamily::system_mono();
        assert_eq!(
            family.variant(FontStyle::Regular).style(),
            FontStyle::Regular
        );
        assert_eq!(family.variant(FontStyle::Bold).style(), FontStyle::Bold);
        // Italic falls back to regular since we don't have an italic variant
        assert_eq!(
            family.variant(FontStyle::Italic).style(),
            FontStyle::Regular
        );
    }

    #[test]
    fn test_tab_expansion() {
        let font = Font::system_mono();
        let w = font.char_width('\t');
        // Default tab = 4 spaces * 8px = 32px
        assert_eq!(w, 32.0);
    }

    #[test]
    fn test_layout_empty() {
        let font = Font::system_mono();
        let layout = TextLayout::new("", &font, 100.0);
        let result = layout.compute();
        assert_eq!(result.glyphs.len(), 0);
        assert_eq!(result.line_count, 0);
    }
}
