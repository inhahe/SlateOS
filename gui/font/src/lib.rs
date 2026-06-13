//! SlateOS Font Library — bitmap and outline font rendering.
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

extern crate alloc;

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

impl GlyphBitmap {
    /// Returns whether the pixel at (x, y) is set.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub fn pixel_at(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let row_bytes = self.width.div_ceil(8);
        let byte_idx = (y * row_bytes + x / 8) as usize;
        let bit_idx = 7 - (x % 8);
        self.bitmap.get(byte_idx).is_some_and(|b| (b >> bit_idx) & 1 != 0)
    }

    /// Creates a scaled version of this glyph by an integer factor.
    pub fn scaled(&self, factor: u32) -> Self {
        let new_width = self.width * factor;
        let new_height = self.height * factor;
        let new_row_bytes = new_width.div_ceil(8);
        let mut new_bitmap = vec![0u8; (new_row_bytes * new_height) as usize];

        for y in 0..self.height {
            for x in 0..self.width {
                if self.pixel_at(x, y) {
                    for dy in 0..factor {
                        for dx in 0..factor {
                            let nx = x * factor + dx;
                            let ny = y * factor + dy;
                            let idx = (ny * new_row_bytes + nx / 8) as usize;
                            let bit = 7 - (nx % 8);
                            if let Some(byte) = new_bitmap.get_mut(idx) {
                                *byte |= 1 << bit;
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
    /// Glyph storage indexed internally.
    glyphs: Vec<(char, GlyphBitmap)>,
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
        let scaled_glyphs: Vec<(char, GlyphBitmap)> = base
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
            scale: base.scale * scale,
            glyphs: scaled_glyphs,
            replacement: base.replacement.scaled(scale),
        }
    }

    /// Looks up the glyph for a character, returning the replacement glyph if not found.
    pub fn glyph(&self, ch: char) -> &GlyphBitmap {
        self.glyphs
            .iter()
            .find(|(c, _)| *c == ch)
            .map(|(_, g)| g)
            .unwrap_or(&self.replacement)
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
        let glyph_advances: Vec<(char, f32)> = text
            .chars()
            .map(|ch| (ch, font.char_width(ch)))
            .collect();

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
                    self.apply_alignment(
                        &mut glyphs,
                        line_start_idx,
                        end,
                        line_width,
                    );
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
                        self.apply_alignment(
                            &mut glyphs,
                            line_start_idx,
                            end,
                            line_width,
                        );
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
            for glyph in glyphs.iter_mut().skip(start).take(end.saturating_sub(start)) {
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
        let py = y + gy as i32;
        if py < 0 || py >= buf_height as i32 {
            continue;
        }

        for gx in 0..glyph.width {
            let px = x + gx as i32;
            if px < 0 || px >= stride as i32 {
                continue;
            }

            if glyph.pixel_at(gx, gy) {
                let idx = (py as u32 * stride + px as u32) as usize;
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
    let inv_alpha = 255 - src_alpha;

    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = src & 0xFF;

    let dr = (dest >> 16) & 0xFF;
    let dg = (dest >> 8) & 0xFF;
    let db = dest & 0xFF;

    let r = (sr * src_alpha + dr * inv_alpha) / 255;
    let g = (sg * src_alpha + dg * inv_alpha) / 255;
    let b = (sb * src_alpha + db * inv_alpha) / 255;

    0xFF00_0000 | (r << 16) | (g << 8) | b
}

// ---------------------------------------------------------------------------
// Built-in system font generation
// ---------------------------------------------------------------------------

/// The base width and height of the built-in system font.
const FONT_WIDTH: u32 = 8;
const FONT_HEIGHT: u32 = 16;

fn build_system_font(style: FontStyle) -> Font {
    let mut glyphs: Vec<(char, GlyphBitmap)> = Vec::new();

    // Generate Basic Latin (U+0020..U+007E)
    for codepoint in 0x20u32..=0x7E {
        if let Some(ch) = char::from_u32(codepoint) {
            let bitmap_data = generate_ascii_glyph(ch, style == FontStyle::Bold);
            glyphs.push((ch, GlyphBitmap {
                width: FONT_WIDTH,
                height: FONT_HEIGHT,
                advance: FONT_WIDTH as f32,
                bearing_x: 0.0,
                bearing_y: FONT_HEIGHT as f32 - 2.0, // baseline at row 14
                bitmap: bitmap_data,
            }));
        }
    }

    // Generate box drawing characters (U+2500..U+257F)
    for codepoint in 0x2500u32..=0x257F {
        if let Some(ch) = char::from_u32(codepoint) {
            let bitmap_data = generate_box_drawing(ch);
            glyphs.push((ch, GlyphBitmap {
                width: FONT_WIDTH,
                height: FONT_HEIGHT,
                advance: FONT_WIDTH as f32,
                bearing_x: 0.0,
                bearing_y: FONT_HEIGHT as f32 - 2.0,
                bitmap: bitmap_data,
            }));
        }
    }

    // Generate block elements (U+2580..U+259F)
    for codepoint in 0x2580u32..=0x259F {
        if let Some(ch) = char::from_u32(codepoint) {
            let bitmap_data = generate_block_element(ch);
            glyphs.push((ch, GlyphBitmap {
                width: FONT_WIDTH,
                height: FONT_HEIGHT,
                advance: FONT_WIDTH as f32,
                bearing_x: 0.0,
                bearing_y: FONT_HEIGHT as f32 - 2.0,
                bitmap: bitmap_data,
            }));
        }
    }

    // Latin-1 Supplement stubs (render as replacement box)
    for codepoint in 0x00A0u32..=0x00FF {
        if let Some(ch) = char::from_u32(codepoint) {
            let bitmap_data = generate_replacement_glyph();
            glyphs.push((ch, GlyphBitmap {
                width: FONT_WIDTH,
                height: FONT_HEIGHT,
                advance: FONT_WIDTH as f32,
                bearing_x: 0.0,
                bearing_y: FONT_HEIGHT as f32 - 2.0,
                bitmap: bitmap_data,
            }));
        }
    }

    let replacement = GlyphBitmap {
        width: FONT_WIDTH,
        height: FONT_HEIGHT,
        advance: FONT_WIDTH as f32,
        bearing_x: 0.0,
        bearing_y: FONT_HEIGHT as f32 - 2.0,
        bitmap: generate_replacement_glyph(),
    };

    Font {
        name: String::from("SlateOS Mono"),
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

/// Generates the replacement glyph: a hollow rectangle.
#[allow(clippy::needless_range_loop)]
fn generate_replacement_glyph() -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];

    // Draw a hollow rectangle from rows 2..14, columns 1..7
    for col in 1..7 {
        rows[2][col] = 1;
        rows[13][col] = 1;
    }
    for row in 2..14 {
        rows[row][1] = 1;
        rows[row][6] = 1;
    }
    // Question mark inside
    rows[5][3] = 1;
    rows[5][4] = 1;
    rows[6][4] = 1;
    rows[7][4] = 1;
    rows[8][3] = 1;
    rows[10][3] = 1;

    pack_bitmap(&rows)
}

/// Packs an 8x16 pixel grid into a 1-bit-per-pixel byte array.
fn pack_bitmap(rows: &[[u8; 8]; 16]) -> Vec<u8> {
    let mut data = vec![0u8; 16]; // 8 pixels wide = 1 byte per row
    for (y, row) in rows.iter().enumerate() {
        let mut byte: u8 = 0;
        for (x, &pixel) in row.iter().enumerate() {
            if pixel != 0 {
                byte |= 1 << (7 - x);
            }
        }
        data[y] = byte;
    }
    data
}

/// Generates a bitmap for a printable ASCII character.
#[allow(clippy::needless_range_loop)]
fn generate_ascii_glyph(ch: char, bold: bool) -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];
    fill_ascii_glyph(ch, &mut rows);

    if bold {
        // Make bold by shifting right and OR-ing (thicker strokes)
        let original = rows;
        for y in 0..16 {
            for x in 0..7 {
                if original[y][x] != 0 {
                    rows[y][x + 1] = 1;
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
#[allow(clippy::too_many_lines, clippy::needless_range_loop)]
fn fill_ascii_glyph(ch: char, rows: &mut [[u8; 8]; 16]) {
    match ch {
        ' ' => {} // empty
        '!' => {
            for r in 3..10 { rows[r][3] = 1; }
            rows[12][3] = 1;
        }
        '"' => {
            for r in 3..6 { rows[r][2] = 1; rows[r][5] = 1; }
        }
        '#' => {
            for r in 3..13 { rows[r][2] = 1; rows[r][5] = 1; }
            for c in 1..7 { rows[5][c] = 1; rows[9][c] = 1; }
        }
        '$' => {
            for c in 2..6 { rows[3][c] = 1; rows[7][c] = 1; rows[11][c] = 1; }
            for r in 3..8 { rows[r][2] = 1; }
            for r in 7..12 { rows[r][5] = 1; }
            for r in 2..13 { rows[r][4] = 1; }
        }
        '%' => {
            rows[3][1] = 1; rows[3][2] = 1; rows[4][1] = 1; rows[4][2] = 1;
            rows[11][5] = 1; rows[11][6] = 1; rows[12][5] = 1; rows[12][6] = 1;
            for i in 0..8 { let r = 4 + i; let c = 1 + i; if r < 13 && c < 7 { rows[r][c] = 1; } }
        }
        '&' => {
            for c in 2..5 { rows[3][c] = 1; rows[7][c] = 1; }
            for r in 4..7 { rows[r][1] = 1; }
            for r in 8..12 { rows[r][1] = 1; rows[r][6] = 1; }
            for c in 2..6 { rows[12][c] = 1; }
        }
        '\'' => {
            for r in 3..6 { rows[r][3] = 1; }
        }
        '(' => {
            for r in 4..13 { rows[r][3] = 1; }
            rows[3][4] = 1; rows[13][4] = 1;
        }
        ')' => {
            for r in 4..13 { rows[r][4] = 1; }
            rows[3][3] = 1; rows[13][3] = 1;
        }
        '*' => {
            rows[4][3] = 1;
            rows[5][2] = 1; rows[5][3] = 1; rows[5][4] = 1;
            rows[6][3] = 1;
        }
        '+' => {
            for r in 5..11 { rows[r][3] = 1; }
            for c in 1..6 { rows[8][c] = 1; }
        }
        ',' => {
            rows[12][3] = 1; rows[13][3] = 1; rows[14][2] = 1;
        }
        '-' => {
            for c in 1..6 { rows[8][c] = 1; }
        }
        '.' => {
            rows[12][3] = 1;
        }
        '/' => {
            for i in 0..10 { let r = 3 + i; let c = 6 - i * 6 / 10; if r < 14 && c < 8 { rows[r][c] = 1; } }
        }
        '0' => {
            for c in 2..6 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 4..12 { rows[r][1] = 1; rows[r][6] = 1; }
            // Diagonal slash through zero
            for i in 0..6 { rows[5 + i][5 - i / 2] = 1; }
        }
        '1' => {
            for r in 3..13 { rows[r][4] = 1; }
            rows[4][3] = 1; rows[3][3] = 1;
            for c in 2..7 { rows[12][c] = 1; }
        }
        '2' => {
            for c in 2..6 { rows[3][c] = 1; rows[8][c] = 1; rows[12][c] = 1; }
            for r in 4..8 { rows[r][6] = 1; }
            for r in 9..12 { rows[r][1] = 1; }
        }
        '3' => {
            for c in 2..6 { rows[3][c] = 1; rows[7][c] = 1; rows[12][c] = 1; }
            for r in 4..7 { rows[r][6] = 1; }
            for r in 8..12 { rows[r][6] = 1; }
        }
        '4' => {
            for r in 3..8 { rows[r][2] = 1; rows[r][5] = 1; }
            for c in 1..7 { rows[8][c] = 1; }
            for r in 9..13 { rows[r][5] = 1; }
        }
        '5' => {
            for c in 1..7 { rows[3][c] = 1; rows[7][c] = 1; rows[12][c] = 1; }
            for r in 4..7 { rows[r][1] = 1; }
            for r in 8..12 { rows[r][6] = 1; }
        }
        '6' => {
            for c in 2..6 { rows[3][c] = 1; rows[7][c] = 1; rows[12][c] = 1; }
            for r in 4..12 { rows[r][1] = 1; }
            for r in 8..12 { rows[r][6] = 1; }
        }
        '7' => {
            for c in 1..7 { rows[3][c] = 1; }
            for r in 4..13 { rows[r][5] = 1; }
        }
        '8' => {
            for c in 2..6 { rows[3][c] = 1; rows[7][c] = 1; rows[12][c] = 1; }
            for r in 4..7 { rows[r][1] = 1; rows[r][6] = 1; }
            for r in 8..12 { rows[r][1] = 1; rows[r][6] = 1; }
        }
        '9' => {
            for c in 2..6 { rows[3][c] = 1; rows[7][c] = 1; rows[12][c] = 1; }
            for r in 4..7 { rows[r][1] = 1; rows[r][6] = 1; }
            for r in 8..12 { rows[r][6] = 1; }
        }
        ':' => {
            rows[5][3] = 1; rows[10][3] = 1;
        }
        ';' => {
            rows[5][3] = 1; rows[10][3] = 1; rows[11][2] = 1;
        }
        '<' => {
            rows[5][5] = 1; rows[6][4] = 1; rows[7][3] = 1; rows[8][2] = 1;
            rows[9][3] = 1; rows[10][4] = 1; rows[11][5] = 1;
        }
        '=' => {
            for c in 1..6 { rows[6][c] = 1; rows[9][c] = 1; }
        }
        '>' => {
            rows[5][2] = 1; rows[6][3] = 1; rows[7][4] = 1; rows[8][5] = 1;
            rows[9][4] = 1; rows[10][3] = 1; rows[11][2] = 1;
        }
        '?' => {
            for c in 2..6 { rows[3][c] = 1; }
            for r in 4..6 { rows[r][6] = 1; }
            rows[7][5] = 1; rows[8][4] = 1; rows[9][3] = 1;
            rows[12][3] = 1;
        }
        '@' => {
            for c in 2..6 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 4..12 { rows[r][1] = 1; }
            for r in 4..8 { rows[r][6] = 1; }
            for r in 6..10 { rows[r][4] = 1; rows[r][5] = 1; }
        }
        'A' => {
            for c in 2..6 { rows[3][c] = 1; rows[8][c] = 1; }
            for r in 4..13 { rows[r][1] = 1; rows[r][6] = 1; }
        }
        'B' => {
            for c in 1..6 { rows[3][c] = 1; rows[8][c] = 1; rows[12][c] = 1; }
            for r in 3..13 { rows[r][1] = 1; }
            for r in 4..8 { rows[r][6] = 1; }
            for r in 9..12 { rows[r][6] = 1; }
        }
        'C' => {
            for c in 2..6 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 4..12 { rows[r][1] = 1; }
        }
        'D' => {
            for c in 1..5 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 3..13 { rows[r][1] = 1; }
            for r in 4..12 { rows[r][6] = 1; }
            rows[3][5] = 1; rows[12][5] = 1;
        }
        'E' => {
            for c in 1..7 { rows[3][c] = 1; rows[8][c] = 1; rows[12][c] = 1; }
            for r in 3..13 { rows[r][1] = 1; }
        }
        'F' => {
            for c in 1..7 { rows[3][c] = 1; rows[8][c] = 1; }
            for r in 3..13 { rows[r][1] = 1; }
        }
        'G' => {
            for c in 2..6 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 4..12 { rows[r][1] = 1; }
            for r in 8..12 { rows[r][6] = 1; }
            for c in 4..7 { rows[8][c] = 1; }
        }
        'H' => {
            for r in 3..13 { rows[r][1] = 1; rows[r][6] = 1; }
            for c in 1..7 { rows[8][c] = 1; }
        }
        'I' => {
            for c in 2..6 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 3..13 { rows[r][4] = 1; }
        }
        'J' => {
            for r in 3..12 { rows[r][5] = 1; }
            for c in 2..5 { rows[12][c] = 1; }
            rows[11][1] = 1;
        }
        'K' => {
            for r in 3..13 { rows[r][1] = 1; }
            rows[5][5] = 1; rows[6][4] = 1; rows[7][3] = 1; rows[8][2] = 1;
            rows[9][3] = 1; rows[10][4] = 1; rows[11][5] = 1; rows[12][6] = 1;
        }
        'L' => {
            for r in 3..13 { rows[r][1] = 1; }
            for c in 1..7 { rows[12][c] = 1; }
        }
        'M' => {
            for r in 3..13 { rows[r][1] = 1; rows[r][6] = 1; }
            rows[4][2] = 1; rows[4][5] = 1;
            rows[5][3] = 1; rows[5][4] = 1;
        }
        'N' => {
            for r in 3..13 { rows[r][1] = 1; rows[r][6] = 1; }
            rows[4][2] = 1; rows[5][3] = 1; rows[6][3] = 1;
            rows[7][4] = 1; rows[8][4] = 1; rows[9][5] = 1;
        }
        'O' => {
            for c in 2..6 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 4..12 { rows[r][1] = 1; rows[r][6] = 1; }
        }
        'P' => {
            for c in 1..6 { rows[3][c] = 1; rows[8][c] = 1; }
            for r in 3..13 { rows[r][1] = 1; }
            for r in 4..8 { rows[r][6] = 1; }
        }
        'Q' => {
            for c in 2..6 { rows[3][c] = 1; rows[12][c] = 1; }
            for r in 4..12 { rows[r][1] = 1; rows[r][6] = 1; }
            rows[10][4] = 1; rows[11][5] = 1; rows[12][6] = 1;
        }
        'R' => {
            for c in 1..6 { rows[3][c] = 1; rows[8][c] = 1; }
            for r in 3..13 { rows[r][1] = 1; }
            for r in 4..8 { rows[r][6] = 1; }
            rows[9][4] = 1; rows[10][5] = 1; rows[11][5] = 1; rows[12][6] = 1;
        }
        'S' => {
            for c in 2..6 { rows[3][c] = 1; rows[8][c] = 1; rows[12][c] = 1; }
            for r in 4..8 { rows[r][1] = 1; }
            for r in 9..12 { rows[r][6] = 1; }
        }
        'T' => {
            for c in 1..7 { rows[3][c] = 1; }
            for r in 4..13 { rows[r][4] = 1; }
        }
        'U' => {
            for r in 3..12 { rows[r][1] = 1; rows[r][6] = 1; }
            for c in 2..6 { rows[12][c] = 1; }
        }
        'V' => {
            for r in 3..10 { rows[r][1] = 1; rows[r][6] = 1; }
            rows[10][2] = 1; rows[10][5] = 1;
            rows[11][3] = 1; rows[11][4] = 1;
            rows[12][3] = 1; rows[12][4] = 1;
        }
        'W' => {
            for r in 3..11 { rows[r][1] = 1; rows[r][6] = 1; }
            for r in 7..11 { rows[r][4] = 1; }
            rows[11][2] = 1; rows[11][5] = 1;
            rows[12][3] = 1; rows[12][4] = 1;
        }
        'X' => {
            for r in 3..7 { rows[r][1] = 1; rows[r][6] = 1; }
            rows[7][2] = 1; rows[7][5] = 1; rows[8][3] = 1; rows[8][4] = 1;
            rows[9][2] = 1; rows[9][5] = 1;
            for r in 10..13 { rows[r][1] = 1; rows[r][6] = 1; }
        }
        'Y' => {
            for r in 3..6 { rows[r][1] = 1; rows[r][6] = 1; }
            rows[6][2] = 1; rows[6][5] = 1;
            rows[7][3] = 1; rows[7][4] = 1;
            for r in 8..13 { rows[r][4] = 1; }
        }
        'Z' => {
            for c in 1..7 { rows[3][c] = 1; rows[12][c] = 1; }
            rows[4][6] = 1; rows[5][5] = 1; rows[6][5] = 1;
            rows[7][4] = 1; rows[8][3] = 1; rows[9][3] = 1;
            rows[10][2] = 1; rows[11][1] = 1;
        }
        '[' => {
            for c in 3..6 { rows[3][c] = 1; rows[13][c] = 1; }
            for r in 3..14 { rows[r][3] = 1; }
        }
        '\\' => {
            for i in 0..10 { let r = 3 + i; let c = 1 + i * 6 / 10; if r < 14 && c < 8 { rows[r][c] = 1; } }
        }
        ']' => {
            for c in 2..5 { rows[3][c] = 1; rows[13][c] = 1; }
            for r in 3..14 { rows[r][4] = 1; }
        }
        '^' => {
            rows[3][4] = 1;
            rows[4][3] = 1; rows[4][5] = 1;
            rows[5][2] = 1; rows[5][6] = 1;
        }
        '_' => {
            for c in 0..8 { rows[14][c] = 1; }
        }
        '`' => {
            rows[3][3] = 1; rows[4][4] = 1;
        }
        'a' => {
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 7..12 { rows[r][6] = 1; }
            for r in 9..12 { rows[r][1] = 1; }
            for c in 2..7 { rows[8][c] = 1; }
        }
        'b' => {
            for r in 3..13 { rows[r][1] = 1; }
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 7..12 { rows[r][6] = 1; }
        }
        'c' => {
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 7..12 { rows[r][1] = 1; }
        }
        'd' => {
            for r in 3..13 { rows[r][6] = 1; }
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 7..12 { rows[r][1] = 1; }
        }
        'e' => {
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 7..9 { rows[r][1] = 1; rows[r][6] = 1; }
            for c in 1..7 { rows[9][c] = 1; }
            for r in 10..12 { rows[r][1] = 1; }
        }
        'f' => {
            for c in 3..6 { rows[3][c] = 1; }
            for r in 4..13 { rows[r][3] = 1; }
            for c in 2..6 { rows[6][c] = 1; }
        }
        'g' => {
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; rows[15][c] = 1; }
            for r in 7..12 { rows[r][1] = 1; rows[r][6] = 1; }
            for r in 13..15 { rows[r][6] = 1; }
        }
        'h' => {
            for r in 3..13 { rows[r][1] = 1; }
            for c in 2..6 { rows[6][c] = 1; }
            for r in 7..13 { rows[r][6] = 1; }
        }
        'i' => {
            rows[4][3] = 1;
            for r in 6..13 { rows[r][3] = 1; }
        }
        'j' => {
            rows[4][5] = 1;
            for r in 6..14 { rows[r][5] = 1; }
            for c in 2..5 { rows[14][c] = 1; }
        }
        'k' => {
            for r in 3..13 { rows[r][1] = 1; }
            rows[7][5] = 1; rows[8][4] = 1; rows[9][3] = 1;
            rows[10][4] = 1; rows[11][5] = 1; rows[12][6] = 1;
        }
        'l' => {
            for r in 3..12 { rows[r][3] = 1; }
            rows[12][4] = 1; rows[12][5] = 1;
        }
        'm' => {
            for r in 7..13 { rows[r][1] = 1; rows[r][4] = 1; rows[r][7] = 1; }
            for c in 1..8 { rows[6][c] = 1; }
        }
        'n' => {
            for r in 6..13 { rows[r][1] = 1; rows[r][6] = 1; }
            for c in 2..6 { rows[6][c] = 1; }
        }
        'o' => {
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 7..12 { rows[r][1] = 1; rows[r][6] = 1; }
        }
        'p' => {
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 6..15 { rows[r][1] = 1; }
            for r in 7..12 { rows[r][6] = 1; }
        }
        'q' => {
            for c in 2..6 { rows[6][c] = 1; rows[12][c] = 1; }
            for r in 6..15 { rows[r][6] = 1; }
            for r in 7..12 { rows[r][1] = 1; }
        }
        'r' => {
            for r in 6..13 { rows[r][2] = 1; }
            for c in 3..6 { rows[6][c] = 1; }
            rows[7][6] = 1;
        }
        's' => {
            for c in 2..6 { rows[6][c] = 1; rows[9][c] = 1; rows[12][c] = 1; }
            for r in 7..9 { rows[r][1] = 1; }
            for r in 10..12 { rows[r][6] = 1; }
        }
        't' => {
            for r in 3..12 { rows[r][3] = 1; }
            for c in 2..6 { rows[6][c] = 1; }
            rows[12][4] = 1; rows[12][5] = 1;
        }
        'u' => {
            for r in 6..12 { rows[r][1] = 1; rows[r][6] = 1; }
            for c in 2..7 { rows[12][c] = 1; }
        }
        'v' => {
            for r in 6..10 { rows[r][1] = 1; rows[r][6] = 1; }
            rows[10][2] = 1; rows[10][5] = 1;
            rows[11][3] = 1; rows[11][4] = 1;
            rows[12][3] = 1;
        }
        'w' => {
            for r in 6..10 { rows[r][1] = 1; rows[r][4] = 1; rows[r][7] = 1; }
            rows[10][2] = 1; rows[10][6] = 1;
            rows[11][3] = 1; rows[11][5] = 1;
            rows[12][4] = 1;
        }
        'x' => {
            rows[6][1] = 1; rows[6][6] = 1;
            rows[7][2] = 1; rows[7][5] = 1;
            rows[8][3] = 1; rows[8][4] = 1;
            rows[9][3] = 1; rows[9][4] = 1;
            rows[10][2] = 1; rows[10][5] = 1;
            rows[11][1] = 1; rows[11][6] = 1;
        }
        'y' => {
            for r in 6..12 { rows[r][1] = 1; rows[r][6] = 1; }
            for c in 2..7 { rows[12][c] = 1; }
            for r in 13..15 { rows[r][6] = 1; }
            for c in 2..6 { rows[15][c] = 1; }
        }
        'z' => {
            for c in 1..7 { rows[6][c] = 1; rows[12][c] = 1; }
            rows[7][5] = 1; rows[8][4] = 1; rows[9][4] = 1;
            rows[10][3] = 1; rows[11][2] = 1;
        }
        '{' => {
            rows[3][5] = 1; rows[4][4] = 1;
            for r in 5..8 { rows[r][4] = 1; }
            rows[8][3] = 1;
            for r in 9..12 { rows[r][4] = 1; }
            rows[12][4] = 1; rows[13][5] = 1;
        }
        '|' => {
            for r in 3..14 { rows[r][4] = 1; }
        }
        '}' => {
            rows[3][3] = 1; rows[4][4] = 1;
            for r in 5..8 { rows[r][4] = 1; }
            rows[8][5] = 1;
            for r in 9..12 { rows[r][4] = 1; }
            rows[12][4] = 1; rows[13][3] = 1;
        }
        '~' => {
            rows[7][2] = 1; rows[6][3] = 1; rows[6][4] = 1;
            rows[7][5] = 1; rows[6][6] = 1;
        }
        _ => {
            // For any unhandled character, produce a filled block
            for r in 3..13 {
                for c in 1..7 {
                    rows[r][c] = 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Box drawing character generation (U+2500..U+257F)
// ---------------------------------------------------------------------------

/// Generates a box drawing character bitmap.
///
/// Box drawing characters are procedurally generated based on which edges
/// (top, bottom, left, right) have lines, and whether lines are single or double.
#[allow(clippy::needless_range_loop)]
fn generate_box_drawing(ch: char) -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];
    let cx: usize = 4; // center x
    let cy: usize = 8; // center y

    // Decode which sides to draw based on the character
    let code = ch as u32 - 0x2500;

    match code {
        0x00 => { // ─ horizontal line
            for c in 0..8 { rows[cy][c] = 1; }
        }
        0x01 => { // ━ heavy horizontal
            for c in 0..8 { rows[cy - 1][c] = 1; rows[cy][c] = 1; rows[cy + 1][c] = 1; }
        }
        0x02 => { // │ vertical line
            for r in 0..16 { rows[r][cx] = 1; }
        }
        0x03 => { // ┃ heavy vertical
            for r in 0..16 { rows[r][cx - 1] = 1; rows[r][cx] = 1; rows[r][cx + 1] = 1; }
        }
        0x0C => { // ┌ top-left corner
            for c in cx..8 { rows[cy][c] = 1; }
            for r in cy..16 { rows[r][cx] = 1; }
        }
        0x10 => { // ┐ top-right corner
            for c in 0..=cx { rows[cy][c] = 1; }
            for r in cy..16 { rows[r][cx] = 1; }
        }
        0x14 => { // └ bottom-left corner
            for c in cx..8 { rows[cy][c] = 1; }
            for r in 0..=cy { rows[r][cx] = 1; }
        }
        0x18 => { // ┘ bottom-right corner
            for c in 0..=cx { rows[cy][c] = 1; }
            for r in 0..=cy { rows[r][cx] = 1; }
        }
        0x1C => { // ├ left tee
            for r in 0..16 { rows[r][cx] = 1; }
            for c in cx..8 { rows[cy][c] = 1; }
        }
        0x24 => { // ┤ right tee
            for r in 0..16 { rows[r][cx] = 1; }
            for c in 0..=cx { rows[cy][c] = 1; }
        }
        0x2C => { // ┬ top tee
            for c in 0..8 { rows[cy][c] = 1; }
            for r in cy..16 { rows[r][cx] = 1; }
        }
        0x34 => { // ┴ bottom tee
            for c in 0..8 { rows[cy][c] = 1; }
            for r in 0..=cy { rows[r][cx] = 1; }
        }
        0x3C => { // ┼ cross
            for c in 0..8 { rows[cy][c] = 1; }
            for r in 0..16 { rows[r][cx] = 1; }
        }
        0x50 => { // ╔ double top-left
            for c in cx..8 { rows[cy - 1][c] = 1; rows[cy + 1][c] = 1; }
            for r in cy..16 { rows[r][cx - 1] = 1; rows[r][cx + 1] = 1; }
            rows[cy - 1][cx - 1] = 1; rows[cy + 1][cx + 1] = 1;
        }
        0x51 => { // ╗ double top-right
            for c in 0..=cx { rows[cy - 1][c] = 1; rows[cy + 1][c] = 1; }
            for r in cy..16 { rows[r][cx - 1] = 1; rows[r][cx + 1] = 1; }
            rows[cy - 1][cx + 1] = 1; rows[cy + 1][cx - 1] = 1;
        }
        0x54 => { // ╚ double bottom-left
            for c in cx..8 { rows[cy - 1][c] = 1; rows[cy + 1][c] = 1; }
            for r in 0..=cy { rows[r][cx - 1] = 1; rows[r][cx + 1] = 1; }
            rows[cy + 1][cx - 1] = 1; rows[cy - 1][cx + 1] = 1;
        }
        0x55 => { // ╝ double bottom-right
            for c in 0..=cx { rows[cy - 1][c] = 1; rows[cy + 1][c] = 1; }
            for r in 0..=cy { rows[r][cx - 1] = 1; rows[r][cx + 1] = 1; }
            rows[cy + 1][cx + 1] = 1; rows[cy - 1][cx - 1] = 1;
        }
        0x5E => { // ╬ double cross
            for c in 0..8 { rows[cy - 1][c] = 1; rows[cy + 1][c] = 1; }
            for r in 0..16 { rows[r][cx - 1] = 1; rows[r][cx + 1] = 1; }
            rows[cy - 1][cx - 1] = 0; rows[cy - 1][cx + 1] = 0;
            rows[cy + 1][cx - 1] = 0; rows[cy + 1][cx + 1] = 0;
        }
        _ => {
            // Fallback for other box drawing: draw a cross (safe default)
            for c in 0..8 { rows[cy][c] = 1; }
            for r in 0..16 { rows[r][cx] = 1; }
        }
    }

    pack_bitmap(&rows)
}

// ---------------------------------------------------------------------------
// Block element generation (U+2580..U+259F)
// ---------------------------------------------------------------------------

/// Generates a block element character bitmap.
///
/// Block elements are filled rectangles covering portions of the cell.
#[allow(clippy::needless_range_loop)]
fn generate_block_element(ch: char) -> Vec<u8> {
    let mut rows = [[0u8; 8]; 16];
    let code = ch as u32 - 0x2580;

    match code {
        0x00 => { // ▀ upper half block
            for r in 0..8 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x01 => { // ▁ lower one-eighth block
            for r in 14..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x02 => { // ▂ lower one-quarter block
            for r in 12..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x03 => { // ▃ lower three-eighths block
            for r in 10..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x04 => { // ▄ lower half block
            for r in 8..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x05 => { // ▅ lower five-eighths block
            for r in 6..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x06 => { // ▆ lower three-quarters block
            for r in 4..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x07 => { // ▇ lower seven-eighths block
            for r in 2..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x08 => { // █ full block
            for r in 0..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x09 => { // ▉ left seven-eighths block
            for r in 0..16 { for c in 0..7 { rows[r][c] = 1; } }
        }
        0x0A => { // ▊ left three-quarters block
            for r in 0..16 { for c in 0..6 { rows[r][c] = 1; } }
        }
        0x0B => { // ▋ left five-eighths block
            for r in 0..16 { for c in 0..5 { rows[r][c] = 1; } }
        }
        0x0C => { // ▌ left half block
            for r in 0..16 { for c in 0..4 { rows[r][c] = 1; } }
        }
        0x0D => { // ▍ left three-eighths block
            for r in 0..16 { for c in 0..3 { rows[r][c] = 1; } }
        }
        0x0E => { // ▎ left one-quarter block
            for r in 0..16 { for c in 0..2 { rows[r][c] = 1; } }
        }
        0x0F => { // ▏ left one-eighth block
            for r in 0..16 { rows[r][0] = 1; }
        }
        0x10 => { // ▐ right half block
            for r in 0..16 { for c in 4..8 { rows[r][c] = 1; } }
        }
        0x11 => { // ░ light shade
            for r in 0..16 {
                for c in 0..8 {
                    if (r + c) % 4 == 0 { rows[r][c] = 1; }
                }
            }
        }
        0x12 => { // ▒ medium shade
            for r in 0..16 {
                for c in 0..8 {
                    if (r + c) % 2 == 0 { rows[r][c] = 1; }
                }
            }
        }
        0x13 => { // ▓ dark shade
            for r in 0..16 {
                for c in 0..8 {
                    if (r + c) % 4 != 0 { rows[r][c] = 1; }
                }
            }
        }
        0x14 => { // ▔ upper one-eighth block
            for r in 0..2 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x15 => { // ▕ right one-eighth block
            for r in 0..16 { rows[r][7] = 1; }
        }
        0x16 => { // ▖ quadrant lower left
            for r in 8..16 { for c in 0..4 { rows[r][c] = 1; } }
        }
        0x17 => { // ▗ quadrant lower right
            for r in 8..16 { for c in 4..8 { rows[r][c] = 1; } }
        }
        0x18 => { // ▘ quadrant upper left
            for r in 0..8 { for c in 0..4 { rows[r][c] = 1; } }
        }
        0x19 => { // ▙ quadrant upper left + lower left + lower right
            for r in 0..8 { for c in 0..4 { rows[r][c] = 1; } }
            for r in 8..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        0x1A => { // ▚ quadrant upper left + lower right
            for r in 0..8 { for c in 0..4 { rows[r][c] = 1; } }
            for r in 8..16 { for c in 4..8 { rows[r][c] = 1; } }
        }
        0x1B => { // ▛ quadrant upper left + upper right + lower left
            for r in 0..8 { for c in 0..8 { rows[r][c] = 1; } }
            for r in 8..16 { for c in 0..4 { rows[r][c] = 1; } }
        }
        0x1C => { // ▜ quadrant upper left + upper right + lower right
            for r in 0..8 { for c in 0..8 { rows[r][c] = 1; } }
            for r in 8..16 { for c in 4..8 { rows[r][c] = 1; } }
        }
        0x1D => { // ▝ quadrant upper right
            for r in 0..8 { for c in 4..8 { rows[r][c] = 1; } }
        }
        0x1E => { // ▞ quadrant upper right + lower left
            for r in 0..8 { for c in 4..8 { rows[r][c] = 1; } }
            for r in 8..16 { for c in 0..4 { rows[r][c] = 1; } }
        }
        0x1F => { // ▟ quadrant upper right + lower left + lower right
            for r in 0..8 { for c in 4..8 { rows[r][c] = 1; } }
            for r in 8..16 { for c in 0..8 { rows[r][c] = 1; } }
        }
        _ => {} // unreachable for valid block element range
    }

    pack_bitmap(&rows)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_font_creation() {
        let font = Font::system_mono();
        assert_eq!(font.name(), "SlateOS Mono");
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
        let line0: Vec<&GlyphPosition> = result.glyphs.iter().filter(|g| g.line_number == 0).collect();
        let line1: Vec<&GlyphPosition> = result.glyphs.iter().filter(|g| g.line_number == 1).collect();
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
        let has_any_pixel = (0..g.height)
            .any(|y| (0..g.width).any(|x| g.pixel_at(x, y)));
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
                assert!(g.pixel_at(x, y), "pixel ({x}, {y}) should be set in full block");
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
                assert!(g.pixel_at(x, y), "pixel ({x}, {y}) should be set in upper half");
            }
        }
        // Lower half (rows 8..16) should be clear
        for y in 8..16 {
            for x in 0..g.width {
                assert!(!g.pixel_at(x, y), "pixel ({x}, {y}) should be clear in upper half");
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
        assert!(bold_count > reg_count, "bold should have more pixels than regular");
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
        assert_eq!(family.variant(FontStyle::Regular).style(), FontStyle::Regular);
        assert_eq!(family.variant(FontStyle::Bold).style(), FontStyle::Bold);
        // Italic falls back to regular since we don't have an italic variant
        assert_eq!(family.variant(FontStyle::Italic).style(), FontStyle::Regular);
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
