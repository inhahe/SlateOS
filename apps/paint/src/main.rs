#![deny(clippy::all)]

//! OurOS Paint
//!
//! A full-featured drawing and image editing application with:
//! - Canvas with configurable dimensions and background color
//! - Drawing tools: Pencil, Line, Rectangle, Ellipse, Polygon, Fill,
//!   Eraser, Text, Eyedropper, Spray Can, Rounded Rectangle
//! - Brush settings: size (1-100px), opacity, hardness
//! - Color system: foreground/background, 48-color palette, RGB sliders,
//!   hex input, recent colors
//! - Selection: rectangular select, move, copy/paste
//! - Layers: create/delete/reorder, visibility, opacity, merge, active indicator
//! - Transform: flip H/V, rotate 90/180/270, resize canvas, crop to selection
//! - 50-step undo/redo history
//! - Zoom: 25%-800%, fit to window, actual size, scroll-to-zoom
//! - Grid overlay (toggleable, configurable spacing)
//! - BMP file format (32-bit BGRA) load/save
//! - Status bar and toolbar with keyboard shortcuts
//!
//! Uses the guitk library for UI rendering.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

/// Catppuccin Mocha base background.
const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
/// Catppuccin Mocha mantle (darker surface).
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
/// Catppuccin Mocha crust (darkest surface).
const MOCHA_CRUST: Color = Color::from_hex(0x11111B);
/// Catppuccin Mocha surface 0.
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
/// Catppuccin Mocha surface 1.
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
/// Catppuccin Mocha surface 2.
const MOCHA_SURFACE2: Color = Color::from_hex(0x585B70);
/// Catppuccin Mocha text.
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Catppuccin Mocha subtext 0.
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
/// Catppuccin Mocha subtext 1.
const MOCHA_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
/// Catppuccin Mocha blue accent.
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
/// Catppuccin Mocha green accent.
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
/// Catppuccin Mocha red accent.
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
/// Catppuccin Mocha yellow accent.
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
/// Catppuccin Mocha peach accent.
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
/// Catppuccin Mocha lavender accent.
const MOCHA_LAVENDER: Color = Color::from_hex(0xB4BEFE);
/// Catppuccin Mocha overlay 0.
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout constants
// ============================================================================

/// Width of the left-side tool panel.
const TOOLBAR_WIDTH: f32 = 48.0;
/// Height of the top menu/option bar.
const OPTION_BAR_HEIGHT: f32 = 36.0;
/// Height of the bottom status bar.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height of the color palette area on the left.
const PALETTE_HEIGHT: f32 = 200.0;
/// Width of the right-side layers panel.
const LAYERS_PANEL_WIDTH: f32 = 180.0;
/// Height of each layer row in the layers panel.
const LAYER_ROW_HEIGHT: f32 = 28.0;
/// Maximum number of undo steps.
const MAX_UNDO_STEPS: usize = 50;
/// Default canvas width.
const DEFAULT_CANVAS_WIDTH: u32 = 800;
/// Default canvas height.
const DEFAULT_CANVAS_HEIGHT: u32 = 600;
/// Minimum zoom level (25%).
const MIN_ZOOM: f32 = 0.25;
/// Maximum zoom level (800%).
const MAX_ZOOM: f32 = 8.0;
/// Number of recent colors to track.
const MAX_RECENT_COLORS: usize = 12;
/// Maximum brush size in pixels.
const MAX_BRUSH_SIZE: u32 = 100;

// ============================================================================
// Drawing tool enumeration
// ============================================================================

/// Available drawing tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Freehand pencil drawing.
    Pencil,
    /// Straight line from point A to B.
    Line,
    /// Axis-aligned rectangle (outline or filled).
    Rectangle,
    /// Ellipse inscribed in a bounding rectangle.
    Ellipse,
    /// Multi-point polygon.
    Polygon,
    /// Flood fill (bucket tool).
    Fill,
    /// Eraser (paints with background color).
    Eraser,
    /// Text insertion tool.
    Text,
    /// Eyedropper / color picker from canvas.
    Eyedropper,
    /// Spray can (randomized dot pattern).
    SprayCan,
    /// Rectangle with rounded corners.
    RoundedRectangle,
    /// Rectangular selection.
    Select,
}

impl Tool {
    /// Returns a short display label for the tool.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pencil => "Pen",
            Self::Line => "Line",
            Self::Rectangle => "Rect",
            Self::Ellipse => "Elli",
            Self::Polygon => "Poly",
            Self::Fill => "Fill",
            Self::Eraser => "Eras",
            Self::Text => "Text",
            Self::Eyedropper => "Eye",
            Self::SprayCan => "Spry",
            Self::RoundedRectangle => "RRct",
            Self::Select => "Sel",
        }
    }

    /// Returns the keyboard shortcut character for this tool.
    pub fn shortcut(self) -> Option<char> {
        match self {
            Self::Pencil => Some('B'),
            Self::Line => Some('L'),
            Self::Rectangle => Some('R'),
            Self::Ellipse => Some('O'),
            Self::Polygon => Some('P'),
            Self::Fill => Some('G'),
            Self::Eraser => Some('E'),
            Self::Text => Some('T'),
            Self::Eyedropper => Some('I'),
            Self::SprayCan => Some('A'),
            Self::RoundedRectangle => Some('U'),
            Self::Select => Some('S'),
        }
    }

    /// All tools in display order.
    pub fn all() -> &'static [Tool] {
        &[
            Self::Pencil,
            Self::Line,
            Self::Rectangle,
            Self::Ellipse,
            Self::RoundedRectangle,
            Self::Polygon,
            Self::Fill,
            Self::Eraser,
            Self::Text,
            Self::Eyedropper,
            Self::SprayCan,
            Self::Select,
        ]
    }
}

// ============================================================================
// Shape fill mode
// ============================================================================

/// Whether shape tools draw outlines or filled shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeMode {
    /// Draw only the outline.
    Outline,
    /// Draw a filled shape.
    Filled,
    /// Draw filled shape with outline border.
    FilledWithOutline,
}

// ============================================================================
// Brush settings
// ============================================================================

/// Configurable brush parameters.
#[derive(Clone, Debug)]
pub struct BrushSettings {
    /// Brush diameter in pixels (1-100).
    pub size: u32,
    /// Opacity from 0.0 (transparent) to 1.0 (opaque).
    pub opacity: f32,
    /// Hardness from 0.0 (soft edges) to 1.0 (hard edges).
    pub hardness: f32,
}

impl BrushSettings {
    /// Creates default brush settings.
    pub fn new() -> Self {
        Self {
            size: 3,
            opacity: 1.0,
            hardness: 1.0,
        }
    }

    /// Sets brush size, clamped to valid range.
    pub fn set_size(&mut self, size: u32) {
        self.size = size.clamp(1, MAX_BRUSH_SIZE);
    }

    /// Sets opacity, clamped to 0.0-1.0.
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Sets hardness, clamped to 0.0-1.0.
    pub fn set_hardness(&mut self, hardness: f32) {
        self.hardness = hardness.clamp(0.0, 1.0);
    }

    /// Applies opacity to a color.
    pub fn apply_opacity(&self, color: Color) -> Color {
        let alpha = (color.a as f32 * self.opacity) as u8;
        Color::rgba(color.r, color.g, color.b, alpha)
    }
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Pixel buffer — per-layer raster data
// ============================================================================

/// A 2D RGBA pixel buffer.
#[derive(Clone, Debug)]
pub struct PixelBuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA pixel data. Length = width * height * 4.
    pub data: Vec<u8>,
}

impl PixelBuffer {
    /// Creates a new pixel buffer filled with a solid color.
    pub fn new(width: u32, height: u32, fill: Color) -> Self {
        let pixel_count = (width as usize).saturating_mul(height as usize);
        let mut data = Vec::with_capacity(pixel_count.saturating_mul(4));
        for _ in 0..pixel_count {
            data.push(fill.r);
            data.push(fill.g);
            data.push(fill.b);
            data.push(fill.a);
        }
        Self { width, height, data }
    }

    /// Creates a transparent pixel buffer.
    pub fn transparent(width: u32, height: u32) -> Self {
        Self::new(width, height, Color::TRANSPARENT)
    }

    /// Returns the byte offset for a given (x, y) coordinate.
    fn offset(&self, x: u32, y: u32) -> Option<usize> {
        if x < self.width && y < self.height {
            Some(((y as usize) * (self.width as usize) + (x as usize)) * 4)
        } else {
            None
        }
    }

    /// Gets the color at (x, y), or None if out of bounds.
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        let off = self.offset(x, y)?;
        Some(Color::rgba(
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ))
    }

    /// Sets the pixel at (x, y). No-op if out of bounds.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if let Some(off) = self.offset(x, y) {
            self.data[off] = color.r;
            self.data[off + 1] = color.g;
            self.data[off + 2] = color.b;
            self.data[off + 3] = color.a;
        }
    }

    /// Alpha-blends a color onto the existing pixel at (x, y).
    pub fn blend_pixel(&mut self, x: u32, y: u32, color: Color) {
        if let Some(existing) = self.get_pixel(x, y) {
            let blended = color.over(existing);
            self.set_pixel(x, y, blended);
        }
    }

    /// Fills the entire buffer with a solid color.
    pub fn fill(&mut self, color: Color) {
        let len = self.data.len();
        let mut i = 0;
        while i < len {
            self.data[i] = color.r;
            self.data[i + 1] = color.g;
            self.data[i + 2] = color.b;
            self.data[i + 3] = color.a;
            i += 4;
        }
    }

    /// Copies a rectangular region from this buffer.
    pub fn copy_region(&self, x: u32, y: u32, w: u32, h: u32) -> PixelBuffer {
        let mut result = PixelBuffer::transparent(w, h);
        for dy in 0..h {
            for dx in 0..w {
                if let Some(c) = self.get_pixel(x.saturating_add(dx), y.saturating_add(dy)) {
                    result.set_pixel(dx, dy, c);
                }
            }
        }
        result
    }

    /// Pastes another buffer onto this one at the given offset (with alpha blending).
    pub fn paste(&mut self, src: &PixelBuffer, dest_x: i32, dest_y: i32) {
        for sy in 0..src.height {
            for sx in 0..src.width {
                let dx = dest_x.saturating_add(sx as i32);
                let dy = dest_y.saturating_add(sy as i32);
                if dx >= 0 && dy >= 0
                    && let Some(c) = src.get_pixel(sx, sy) {
                        self.blend_pixel(dx as u32, dy as u32, c);
                    }
            }
        }
    }

    /// Pastes another buffer onto this one, overwriting (no blending).
    pub fn paste_overwrite(&mut self, src: &PixelBuffer, dest_x: i32, dest_y: i32) {
        for sy in 0..src.height {
            for sx in 0..src.width {
                let dx = dest_x.saturating_add(sx as i32);
                let dy = dest_y.saturating_add(sy as i32);
                if dx >= 0 && dy >= 0
                    && let Some(c) = src.get_pixel(sx, sy) {
                        self.set_pixel(dx as u32, dy as u32, c);
                    }
            }
        }
    }

    /// Flips the buffer horizontally (left-right mirror).
    pub fn flip_horizontal(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width / 2 {
                let other_x = self.width - 1 - x;
                let left = self.get_pixel(x, y);
                let right = self.get_pixel(other_x, y);
                if let (Some(l), Some(r)) = (left, right) {
                    self.set_pixel(x, y, r);
                    self.set_pixel(other_x, y, l);
                }
            }
        }
    }

    /// Flips the buffer vertically (top-bottom mirror).
    pub fn flip_vertical(&mut self) {
        for y in 0..self.height / 2 {
            let other_y = self.height - 1 - y;
            for x in 0..self.width {
                let top = self.get_pixel(x, y);
                let bottom = self.get_pixel(x, other_y);
                if let (Some(t), Some(b)) = (top, bottom) {
                    self.set_pixel(x, y, b);
                    self.set_pixel(x, other_y, t);
                }
            }
        }
    }

    /// Rotates the buffer 90 degrees clockwise. Returns a new buffer.
    pub fn rotate_90_cw(&self) -> PixelBuffer {
        let new_w = self.height;
        let new_h = self.width;
        let mut result = PixelBuffer::transparent(new_w, new_h);
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(c) = self.get_pixel(x, y) {
                    let nx = self.height - 1 - y;
                    let ny = x;
                    result.set_pixel(nx, ny, c);
                }
            }
        }
        result
    }

    /// Rotates the buffer 90 degrees counter-clockwise. Returns a new buffer.
    pub fn rotate_90_ccw(&self) -> PixelBuffer {
        let new_w = self.height;
        let new_h = self.width;
        let mut result = PixelBuffer::transparent(new_w, new_h);
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(c) = self.get_pixel(x, y) {
                    let nx = y;
                    let ny = self.width - 1 - x;
                    result.set_pixel(nx, ny, c);
                }
            }
        }
        result
    }

    /// Rotates the buffer 180 degrees. Returns a new buffer.
    pub fn rotate_180(&self) -> PixelBuffer {
        let mut result = PixelBuffer::transparent(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(c) = self.get_pixel(x, y) {
                    let nx = self.width - 1 - x;
                    let ny = self.height - 1 - y;
                    result.set_pixel(nx, ny, c);
                }
            }
        }
        result
    }

    /// Resizes using nearest-neighbor interpolation. Returns a new buffer.
    pub fn resize_nearest(&self, new_width: u32, new_height: u32) -> PixelBuffer {
        if new_width == 0 || new_height == 0 {
            return PixelBuffer::transparent(new_width, new_height);
        }
        let mut result = PixelBuffer::transparent(new_width, new_height);
        for ny in 0..new_height {
            for nx in 0..new_width {
                let sx = ((nx as f64 * self.width as f64) / new_width as f64) as u32;
                let sy = ((ny as f64 * self.height as f64) / new_height as f64) as u32;
                let sx = sx.min(self.width.saturating_sub(1));
                let sy = sy.min(self.height.saturating_sub(1));
                if let Some(c) = self.get_pixel(sx, sy) {
                    result.set_pixel(nx, ny, c);
                }
            }
        }
        result
    }
}

// ============================================================================
// Drawing primitives on pixel buffers
// ============================================================================

/// Draws a line using Bresenham's algorithm.
pub fn draw_line(
    buf: &mut PixelBuffer,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: Color,
    thickness: u32,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: i32 = if x0 < x1 { 1 } else { -1 };
    let sy: i32 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut cx = x0;
    let mut cy = y0;

    let half = (thickness / 2) as i32;

    loop {
        // Draw a filled circle at each point for thick lines
        if thickness <= 1 {
            if cx >= 0 && cy >= 0 {
                buf.blend_pixel(cx as u32, cy as u32, color);
            }
        } else {
            draw_filled_circle_at(buf, cx, cy, half, color);
        }

        if cx == x1 && cy == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            cx += sx;
        }
        if e2 <= dx {
            err += dx;
            cy += sy;
        }
    }
}

/// Draws a filled circle centered at (cx, cy) with given radius.
fn draw_filled_circle_at(buf: &mut PixelBuffer, cx: i32, cy: i32, radius: i32, color: Color) {
    let r2 = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= r2 {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 {
                    buf.blend_pixel(px as u32, py as u32, color);
                }
            }
        }
    }
}

/// Draws an outlined rectangle on the pixel buffer.
pub fn draw_rect_outline(
    buf: &mut PixelBuffer,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: Color,
    thickness: u32,
) {
    let x2 = x + w - 1;
    let y2 = y + h - 1;
    draw_line(buf, x, y, x2, y, color, thickness);
    draw_line(buf, x2, y, x2, y2, color, thickness);
    draw_line(buf, x2, y2, x, y2, color, thickness);
    draw_line(buf, x, y2, x, y, color, thickness);
}

/// Draws a filled rectangle on the pixel buffer.
pub fn draw_rect_filled(buf: &mut PixelBuffer, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for dy in 0..h {
        for dx in 0..w {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && py >= 0 {
                buf.blend_pixel(px as u32, py as u32, color);
            }
        }
    }
}

/// Draws an outlined ellipse using the midpoint algorithm.
pub fn draw_ellipse_outline(
    buf: &mut PixelBuffer,
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    color: Color,
    thickness: u32,
) {
    if rx <= 0 || ry <= 0 {
        return;
    }

    let rx2 = (rx as i64) * (rx as i64);
    let ry2 = (ry as i64) * (ry as i64);

    let mut x: i64 = 0;
    let mut y: i64 = ry as i64;
    let mut px: i64 = 0;
    let mut py: i64 = 2 * rx2 * y;

    // Region 1
    let mut p = ry2 - rx2 * (ry as i64) + rx2 / 4;
    while px < py {
        plot_ellipse_points(buf, cx, cy, x as i32, y as i32, color, thickness);
        x += 1;
        px += 2 * ry2;
        if p < 0 {
            p += ry2 + px;
        } else {
            y -= 1;
            py -= 2 * rx2;
            p += ry2 + px - py;
        }
    }

    // Region 2
    p = ry2 * (x * x + x) + rx2 * (y - 1) * (y - 1) - rx2 * ry2;
    while y >= 0 {
        plot_ellipse_points(buf, cx, cy, x as i32, y as i32, color, thickness);
        y -= 1;
        py -= 2 * rx2;
        if p > 0 {
            p += rx2 - py;
        } else {
            x += 1;
            px += 2 * ry2;
            p += rx2 - py + px;
        }
    }
}

/// Plots the four symmetrical points of an ellipse.
fn plot_ellipse_points(
    buf: &mut PixelBuffer,
    cx: i32,
    cy: i32,
    x: i32,
    y: i32,
    color: Color,
    thickness: u32,
) {
    let points = [
        (cx + x, cy + y),
        (cx - x, cy + y),
        (cx + x, cy - y),
        (cx - x, cy - y),
    ];
    let half = (thickness / 2) as i32;
    for (px, py) in points {
        if thickness <= 1 {
            if px >= 0 && py >= 0 {
                buf.blend_pixel(px as u32, py as u32, color);
            }
        } else {
            draw_filled_circle_at(buf, px, py, half, color);
        }
    }
}

/// Draws a filled ellipse using horizontal scan lines.
pub fn draw_ellipse_filled(
    buf: &mut PixelBuffer,
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    color: Color,
) {
    if rx <= 0 || ry <= 0 {
        return;
    }
    for dy in -ry..=ry {
        // Calculate x extent at this y using the ellipse equation
        let y_ratio = (dy as f64) / (ry as f64);
        let x_extent = (rx as f64) * (1.0 - y_ratio * y_ratio).sqrt();
        let x_start = (cx as f64 - x_extent).ceil() as i32;
        let x_end = (cx as f64 + x_extent).floor() as i32;
        for px in x_start..=x_end {
            if px >= 0 && (cy + dy) >= 0 {
                buf.blend_pixel(px as u32, (cy + dy) as u32, color);
            }
        }
    }
}

/// Draws a rounded rectangle outline on the pixel buffer.
// 8 args mirror the (x,y,w,h,radius,color,thickness) geometry signature used
// throughout the paint primitives — splitting these into a struct would just
// shift the call-site verbosity without adding clarity.
#[allow(clippy::too_many_arguments)]
pub fn draw_rounded_rect_outline(
    buf: &mut PixelBuffer,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    color: Color,
    thickness: u32,
) {
    let r = radius.min(w / 2).min(h / 2);
    let x2 = x + w - 1;
    let y2 = y + h - 1;

    // Straight edges
    draw_line(buf, x + r, y, x2 - r, y, color, thickness);
    draw_line(buf, x + r, y2, x2 - r, y2, color, thickness);
    draw_line(buf, x, y + r, x, y2 - r, color, thickness);
    draw_line(buf, x2, y + r, x2, y2 - r, color, thickness);

    // Corner arcs (approximate with quarter-ellipses)
    draw_corner_arc(buf, x + r, y + r, r, r, 2, color, thickness);
    draw_corner_arc(buf, x2 - r, y + r, r, r, 1, color, thickness);
    draw_corner_arc(buf, x + r, y2 - r, r, r, 3, color, thickness);
    draw_corner_arc(buf, x2 - r, y2 - r, r, r, 0, color, thickness);
}

/// Draws a filled rounded rectangle on the pixel buffer.
pub fn draw_rounded_rect_filled(
    buf: &mut PixelBuffer,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    color: Color,
) {
    let r = radius.min(w / 2).min(h / 2);

    // Fill the center rectangle
    draw_rect_filled(buf, x + r, y, w - 2 * r, h, color);
    // Fill left and right side strips
    draw_rect_filled(buf, x, y + r, r, h - 2 * r, color);
    draw_rect_filled(buf, x + w - r, y + r, r, h - 2 * r, color);

    // Fill corners with quarter-circles
    fill_quarter_circle(buf, x + r, y + r, r, 2, color);
    fill_quarter_circle(buf, x + w - 1 - r, y + r, r, 1, color);
    fill_quarter_circle(buf, x + r, y + h - 1 - r, r, 3, color);
    fill_quarter_circle(buf, x + w - 1 - r, y + h - 1 - r, r, 0, color);
}

/// Draws a quarter arc. Quadrant: 0=bottom-right, 1=top-right, 2=top-left, 3=bottom-left.
// 8 args — same rationale as draw_rounded_rect_outline above (intrinsic
// geometry signature, struct-bundling would only shift verbosity).
#[allow(clippy::too_many_arguments)]
fn draw_corner_arc(
    buf: &mut PixelBuffer,
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    quadrant: u8,
    color: Color,
    thickness: u32,
) {
    let steps = (rx.max(ry) as f64 * 4.0).max(16.0) as i32;
    let half = (thickness / 2) as i32;
    let (angle_start, angle_end) = match quadrant {
        0 => (0.0_f64, std::f64::consts::FRAC_PI_2),
        1 => (std::f64::consts::FRAC_PI_2, std::f64::consts::PI),
        2 => (std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2),
        _ => (3.0 * std::f64::consts::FRAC_PI_2, 2.0 * std::f64::consts::PI),
    };

    for i in 0..=steps {
        let t = angle_start + (angle_end - angle_start) * (i as f64 / steps as f64);
        let px = cx + (rx as f64 * t.cos()) as i32;
        let py = cy - (ry as f64 * t.sin()) as i32;
        if thickness <= 1 {
            if px >= 0 && py >= 0 {
                buf.blend_pixel(px as u32, py as u32, color);
            }
        } else {
            draw_filled_circle_at(buf, px, py, half, color);
        }
    }
}

/// Fills a quarter circle. Quadrant: 0=bottom-right, 1=top-right, 2=top-left, 3=bottom-left.
fn fill_quarter_circle(
    buf: &mut PixelBuffer,
    cx: i32,
    cy: i32,
    r: i32,
    quadrant: u8,
    color: Color,
) {
    let r2 = r * r;
    for dy in 0..=r {
        for dx in 0..=r {
            if dx * dx + dy * dy <= r2 {
                let (px, py) = match quadrant {
                    0 => (cx + dx, cy + dy),
                    1 => (cx - dx, cy + dy),
                    2 => (cx - dx, cy - dy),
                    _ => (cx + dx, cy - dy),
                };
                if px >= 0 && py >= 0 {
                    buf.blend_pixel(px as u32, py as u32, color);
                }
            }
        }
    }
}

/// Flood fill starting at (start_x, start_y).
pub fn flood_fill(buf: &mut PixelBuffer, start_x: u32, start_y: u32, fill_color: Color) {
    let target_color = match buf.get_pixel(start_x, start_y) {
        Some(c) => c,
        None => return,
    };

    // Don't fill if target and fill color are the same
    if target_color == fill_color {
        return;
    }

    let mut stack: Vec<(u32, u32)> = Vec::new();
    stack.push((start_x, start_y));

    while let Some((px, py)) = stack.pop() {
        if let Some(current) = buf.get_pixel(px, py) {
            if current != target_color {
                continue;
            }
            buf.set_pixel(px, py, fill_color);

            if px > 0 {
                stack.push((px - 1, py));
            }
            if px + 1 < buf.width {
                stack.push((px + 1, py));
            }
            if py > 0 {
                stack.push((px, py - 1));
            }
            if py + 1 < buf.height {
                stack.push((px, py + 1));
            }
        }
    }
}

/// Spray paint effect: randomly scatter dots within a radius.
pub fn spray_paint(
    buf: &mut PixelBuffer,
    cx: i32,
    cy: i32,
    radius: i32,
    color: Color,
    density: u32,
    seed: u32,
) {
    // Simple pseudo-random number generator (xorshift)
    let mut rng_state = seed.wrapping_add(1);
    for _ in 0..density {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 17;
        rng_state ^= rng_state << 5;
        let angle = (rng_state as f64 / u32::MAX as f64) * 2.0 * std::f64::consts::PI;

        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 17;
        rng_state ^= rng_state << 5;
        let dist = (rng_state as f64 / u32::MAX as f64).sqrt() * radius as f64;

        let px = cx + (dist * angle.cos()) as i32;
        let py = cy + (dist * angle.sin()) as i32;
        if px >= 0 && py >= 0 {
            buf.blend_pixel(px as u32, py as u32, color);
        }
    }
}

// ============================================================================
// Layer
// ============================================================================

/// A single drawing layer.
#[derive(Clone, Debug)]
pub struct Layer {
    /// Display name.
    pub name: String,
    /// Pixel data for this layer.
    pub pixels: PixelBuffer,
    /// Whether this layer is visible.
    pub visible: bool,
    /// Layer opacity (0.0 - 1.0).
    pub opacity: f32,
}

impl Layer {
    /// Creates a new transparent layer with the given dimensions.
    pub fn new(name: String, width: u32, height: u32) -> Self {
        Self {
            name,
            pixels: PixelBuffer::transparent(width, height),
            visible: true,
            opacity: 1.0,
        }
    }

    /// Creates a new layer filled with a solid color.
    pub fn with_background(name: String, width: u32, height: u32, color: Color) -> Self {
        Self {
            name,
            pixels: PixelBuffer::new(width, height, color),
            visible: true,
            opacity: 1.0,
        }
    }

    /// Sets the layer opacity (clamped to 0.0-1.0).
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Toggles this layer's visibility.
    pub fn toggle_visibility(&mut self) {
        self.visible = !self.visible;
    }
}

// ============================================================================
// Selection
// ============================================================================

/// Rectangular selection state.
#[derive(Clone, Debug)]
pub struct Selection {
    /// Top-left X of selection.
    pub x: i32,
    /// Top-left Y of selection.
    pub y: i32,
    /// Width of selection.
    pub width: u32,
    /// Height of selection.
    pub height: u32,
    /// Pixel data that has been cut/copied (if any).
    pub content: Option<PixelBuffer>,
}

impl Selection {
    /// Creates a new empty selection.
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            content: None,
        }
    }

    /// Returns true if the selection has nonzero area.
    pub fn has_area(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Returns the selection rectangle as (x, y, w, h).
    pub fn rect(&self) -> (i32, i32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }

    /// Checks if a point is inside the selection.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.width as i32
            && py < self.y + self.height as i32
    }
}

// ============================================================================
// Undo / Redo history
// ============================================================================

/// A snapshot of the canvas state for undo/redo.
#[derive(Clone, Debug)]
pub struct HistorySnapshot {
    /// All layers cloned at this point.
    pub layers: Vec<Layer>,
    /// Active layer index.
    pub active_layer: usize,
    /// Description of what action produced this snapshot.
    pub description: String,
}

/// Manages undo/redo history.
#[derive(Clone, Debug)]
pub struct History {
    /// Undo stack (past states).
    pub undo_stack: VecDeque<HistorySnapshot>,
    /// Redo stack (future states that were undone).
    pub redo_stack: VecDeque<HistorySnapshot>,
    /// Maximum number of snapshots to keep.
    pub max_steps: usize,
}

impl History {
    /// Creates a new history manager.
    pub fn new(max_steps: usize) -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            max_steps,
        }
    }

    /// Pushes a snapshot onto the undo stack, clearing redo.
    pub fn push(&mut self, snapshot: HistorySnapshot) {
        self.redo_stack.clear();
        if self.undo_stack.len() >= self.max_steps {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(snapshot);
    }

    /// Pops the last undo state. Returns it if available.
    pub fn undo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        if let Some(prev) = self.undo_stack.pop_back() {
            self.redo_stack.push_back(current);
            Some(prev)
        } else {
            None
        }
    }

    /// Pops the last redo state. Returns it if available.
    pub fn redo(&mut self, current: HistorySnapshot) -> Option<HistorySnapshot> {
        if let Some(next) = self.redo_stack.pop_back() {
            self.undo_stack.push_back(current);
            Some(next)
        } else {
            None
        }
    }

    /// Returns the number of available undo steps.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Returns the number of available redo steps.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Clears all history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

// ============================================================================
// Polygon builder
// ============================================================================

/// Tracks polygon vertices while the user is building one.
#[derive(Clone, Debug)]
pub struct PolygonBuilder {
    /// Collected vertices so far.
    pub points: Vec<(i32, i32)>,
    /// Whether the polygon is closed/complete.
    pub closed: bool,
}

impl PolygonBuilder {
    /// Creates a new empty polygon builder.
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
        }
    }

    /// Adds a vertex.
    pub fn add_point(&mut self, x: i32, y: i32) {
        self.points.push((x, y));
    }

    /// Closes the polygon by connecting last vertex to first.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.points.len()
    }

    /// Draws the polygon outline onto a pixel buffer.
    pub fn draw_outline(&self, buf: &mut PixelBuffer, color: Color, thickness: u32) {
        let pts = &self.points;
        if pts.len() < 2 {
            return;
        }
        for i in 0..pts.len() - 1 {
            draw_line(buf, pts[i].0, pts[i].1, pts[i + 1].0, pts[i + 1].1, color, thickness);
        }
        if self.closed && pts.len() >= 3 {
            let last = pts.len() - 1;
            draw_line(buf, pts[last].0, pts[last].1, pts[0].0, pts[0].1, color, thickness);
        }
    }

    /// Draws the polygon filled (using scan line algorithm) onto a pixel buffer.
    pub fn draw_filled(&self, buf: &mut PixelBuffer, color: Color) {
        let pts = &self.points;
        if pts.len() < 3 {
            return;
        }

        // Find bounding box
        let min_y = pts.iter().map(|p| p.1).min().unwrap_or(0);
        let max_y = pts.iter().map(|p| p.1).max().unwrap_or(0);

        for y in min_y..=max_y {
            let mut intersections = Vec::new();
            let n = pts.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (y0, y1) = (pts[i].1, pts[j].1);
                let (x0, x1) = (pts[i].0, pts[j].0);

                if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                    let dy = y1 - y0;
                    if dy != 0 {
                        let t = (y - y0) as f64 / dy as f64;
                        let ix = x0 as f64 + t * (x1 - x0) as f64;
                        intersections.push(ix as i32);
                    }
                }
            }

            intersections.sort();

            let mut idx = 0;
            while idx + 1 < intersections.len() {
                let x_start = intersections[idx];
                let x_end = intersections[idx + 1];
                for x in x_start..=x_end {
                    if x >= 0 && y >= 0 {
                        buf.blend_pixel(x as u32, y as u32, color);
                    }
                }
                idx += 2;
            }
        }
    }

    /// Resets the builder for a new polygon.
    pub fn reset(&mut self) {
        self.points.clear();
        self.closed = false;
    }
}

impl Default for PolygonBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Color palette
// ============================================================================

/// Returns the default 48-color palette.
pub fn default_palette() -> Vec<Color> {
    vec![
        // Row 1: blacks, grays, whites
        Color::rgb(0, 0, 0),
        Color::rgb(64, 64, 64),
        Color::rgb(128, 128, 128),
        Color::rgb(192, 192, 192),
        Color::rgb(224, 224, 224),
        Color::rgb(255, 255, 255),
        // Row 2: reds/pinks
        Color::rgb(128, 0, 0),
        Color::rgb(192, 0, 0),
        Color::rgb(255, 0, 0),
        Color::rgb(255, 128, 128),
        Color::rgb(255, 192, 192),
        Color::rgb(255, 64, 64),
        // Row 3: oranges/browns
        Color::rgb(128, 64, 0),
        Color::rgb(192, 96, 0),
        Color::rgb(255, 128, 0),
        Color::rgb(255, 192, 128),
        Color::rgb(128, 96, 64),
        Color::rgb(192, 128, 64),
        // Row 4: yellows
        Color::rgb(128, 128, 0),
        Color::rgb(192, 192, 0),
        Color::rgb(255, 255, 0),
        Color::rgb(255, 255, 128),
        Color::rgb(255, 255, 192),
        Color::rgb(192, 192, 128),
        // Row 5: greens
        Color::rgb(0, 128, 0),
        Color::rgb(0, 192, 0),
        Color::rgb(0, 255, 0),
        Color::rgb(128, 255, 128),
        Color::rgb(192, 255, 192),
        Color::rgb(0, 192, 128),
        // Row 6: cyans
        Color::rgb(0, 128, 128),
        Color::rgb(0, 192, 192),
        Color::rgb(0, 255, 255),
        Color::rgb(128, 255, 255),
        Color::rgb(192, 255, 255),
        Color::rgb(0, 128, 192),
        // Row 7: blues
        Color::rgb(0, 0, 128),
        Color::rgb(0, 0, 192),
        Color::rgb(0, 0, 255),
        Color::rgb(128, 128, 255),
        Color::rgb(192, 192, 255),
        Color::rgb(0, 64, 192),
        // Row 8: purples/magentas
        Color::rgb(128, 0, 128),
        Color::rgb(192, 0, 192),
        Color::rgb(255, 0, 255),
        Color::rgb(255, 128, 255),
        Color::rgb(255, 192, 255),
        Color::rgb(128, 0, 255),
    ]
}

// ============================================================================
// BMP file format (32-bit BGRA)
// ============================================================================

/// Encodes a pixel buffer as a 32-bit BMP file.
pub fn encode_bmp(buf: &PixelBuffer) -> Vec<u8> {
    let w = buf.width;
    let h = buf.height;
    let row_size = w as usize * 4;
    let pixel_data_size = row_size * h as usize;
    let file_size = 54 + pixel_data_size;

    let mut out = Vec::with_capacity(file_size);

    // BMP file header (14 bytes)
    out.push(b'B');
    out.push(b'M');
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved1
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved2
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset

    // DIB header (BITMAPINFOHEADER, 40 bytes)
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&(w as i32).to_le_bytes()); // width
    out.extend_from_slice(&(h as i32).to_le_bytes()); // height (positive = bottom-up)
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // compression (BI_RGB)
    out.extend_from_slice(&(pixel_data_size as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes()); // h resolution (72 DPI)
    out.extend_from_slice(&2835u32.to_le_bytes()); // v resolution
    out.extend_from_slice(&0u32.to_le_bytes()); // palette colors
    out.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // Pixel data (bottom-up, BGRA)
    for y in (0..h).rev() {
        for x in 0..w {
            if let Some(c) = buf.get_pixel(x, y) {
                out.push(c.b);
                out.push(c.g);
                out.push(c.r);
                out.push(c.a);
            }
        }
    }

    out
}

/// Decodes a 32-bit BMP file into a pixel buffer. Returns None on invalid data.
pub fn decode_bmp(data: &[u8]) -> Option<PixelBuffer> {
    if data.len() < 54 {
        return None;
    }
    if data[0] != b'B' || data[1] != b'M' {
        return None;
    }

    let pixel_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    let bpp = u16::from_le_bytes([data[28], data[29]]);

    if width <= 0 || bpp != 32 {
        return None;
    }

    let w = width as u32;
    let bottom_up = height > 0;
    let h = height.unsigned_abs();

    let row_bytes = w as usize * 4;
    let needed = pixel_offset + row_bytes * h as usize;
    if data.len() < needed {
        return None;
    }

    let mut buf = PixelBuffer::transparent(w, h);

    for row in 0..h {
        let src_row = if bottom_up { h - 1 - row } else { row };
        let row_start = pixel_offset + src_row as usize * row_bytes;
        for col in 0..w {
            let off = row_start + col as usize * 4;
            let b = data[off];
            let g_val = data[off + 1];
            let r = data[off + 2];
            let a = data[off + 3];
            buf.set_pixel(col, row, Color::rgba(r, g_val, b, a));
        }
    }

    Some(buf)
}

// ============================================================================
// Text input buffer (for text tool and hex color input)
// ============================================================================

/// Simple text input buffer.
#[derive(Clone, Debug)]
pub struct TextInput {
    /// The current text content.
    pub text: String,
    /// Cursor position (byte offset).
    pub cursor: usize,
}

impl TextInput {
    /// Creates a new empty text input.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
        }
    }

    /// Creates a text input with initial content.
    pub fn with_text(text: &str) -> Self {
        let len = text.len();
        Self {
            text: text.to_string(),
            cursor: len,
        }
    }

    /// Inserts a character at the cursor.
    pub fn insert_char(&mut self, ch: char) {
        if self.cursor <= self.text.len() {
            self.text.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
        }
    }

    /// Deletes the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find previous character boundary
            let prev = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.text.remove(prev);
            self.cursor = prev;
        }
    }

    /// Deletes the character at the cursor (delete key).
    pub fn delete_forward(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    /// Moves cursor left by one character.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Moves cursor right by one character.
    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.text[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.text.len());
        }
    }

    /// Clears all text.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Returns the text content as a string slice.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl Default for TextInput {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Color picker state
// ============================================================================

/// Color picker with RGB sliders and hex input.
#[derive(Clone, Debug)]
pub struct ColorPicker {
    /// Current red value (0-255).
    pub red: u8,
    /// Current green value (0-255).
    pub green: u8,
    /// Current blue value (0-255).
    pub blue: u8,
    /// Current alpha value (0-255).
    pub alpha: u8,
    /// Hex input field.
    pub hex_input: TextInput,
    /// Whether the color picker dialog is open.
    pub is_open: bool,
    /// Whether we are editing foreground (true) or background (false).
    pub editing_foreground: bool,
    /// Which slider is being dragged (0=R, 1=G, 2=B, 3=A, None=nothing).
    pub active_slider: Option<u8>,
}

impl ColorPicker {
    /// Creates a new color picker initialized to the given color.
    pub fn new(color: Color) -> Self {
        let hex_str = format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b);
        Self {
            red: color.r,
            green: color.g,
            blue: color.b,
            alpha: color.a,
            hex_input: TextInput::with_text(&hex_str),
            is_open: false,
            editing_foreground: true,
            active_slider: None,
        }
    }

    /// Returns the currently selected color.
    pub fn color(&self) -> Color {
        Color::rgba(self.red, self.green, self.blue, self.alpha)
    }

    /// Sets the color and updates hex input.
    pub fn set_color(&mut self, color: Color) {
        self.red = color.r;
        self.green = color.g;
        self.blue = color.b;
        self.alpha = color.a;
        self.hex_input = TextInput::with_text(
            &format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b),
        );
    }

    /// Tries to parse the hex input and update the color sliders.
    pub fn apply_hex_input(&mut self) -> bool {
        let text = self.hex_input.text.trim().trim_start_matches('#');
        if text.len() == 6
            && let Ok(val) = u32::from_str_radix(text, 16) {
                self.red = ((val >> 16) & 0xFF) as u8;
                self.green = ((val >> 8) & 0xFF) as u8;
                self.blue = (val & 0xFF) as u8;
                return true;
            }
        false
    }

    /// Opens the dialog for editing the specified target.
    pub fn open_for(&mut self, foreground: bool, current: Color) {
        self.editing_foreground = foreground;
        self.set_color(current);
        self.is_open = true;
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.is_open = false;
        self.active_slider = None;
    }
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self::new(Color::BLACK)
    }
}

// ============================================================================
// Grid overlay settings
// ============================================================================

/// Grid overlay configuration.
#[derive(Clone, Debug)]
pub struct GridSettings {
    /// Whether the grid is visible.
    pub visible: bool,
    /// Grid spacing in pixels.
    pub spacing: u32,
    /// Grid line color.
    pub color: Color,
}

impl GridSettings {
    /// Creates default grid settings.
    pub fn new() -> Self {
        Self {
            visible: false,
            spacing: 16,
            color: Color::rgba(128, 128, 128, 80),
        }
    }

    /// Toggles grid visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Sets grid spacing (minimum 2 pixels).
    pub fn set_spacing(&mut self, spacing: u32) {
        self.spacing = spacing.max(2);
    }
}

impl Default for GridSettings {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Clipboard
// ============================================================================

/// Application-level clipboard for copy/paste.
#[derive(Clone, Debug, Default)]
pub struct Clipboard {
    /// Copied pixel data, if any.
    pub content: Option<PixelBuffer>,
}

impl Clipboard {
    /// Creates an empty clipboard.
    pub fn new() -> Self {
        Self { content: None }
    }

    /// Stores pixel data.
    pub fn store(&mut self, buf: PixelBuffer) {
        self.content = Some(buf);
    }

    /// Returns a reference to the stored content.
    pub fn get(&self) -> Option<&PixelBuffer> {
        self.content.as_ref()
    }

    /// Returns true if the clipboard has content.
    pub fn has_content(&self) -> bool {
        self.content.is_some()
    }

    /// Clears the clipboard.
    pub fn clear(&mut self) {
        self.content = None;
    }
}

// ============================================================================
// Drag state
// ============================================================================

/// Tracks mouse drag operations.
#[derive(Clone, Debug)]
pub struct DragState {
    /// Starting canvas X.
    pub start_x: i32,
    /// Starting canvas Y.
    pub start_y: i32,
    /// Current canvas X.
    pub current_x: i32,
    /// Current canvas Y.
    pub current_y: i32,
    /// Whether a drag is in progress.
    pub active: bool,
}

impl DragState {
    /// Creates an inactive drag state.
    pub fn new() -> Self {
        Self {
            start_x: 0,
            start_y: 0,
            current_x: 0,
            current_y: 0,
            active: false,
        }
    }

    /// Begins a drag at the given position.
    pub fn begin(&mut self, x: i32, y: i32) {
        self.start_x = x;
        self.start_y = y;
        self.current_x = x;
        self.current_y = y;
        self.active = true;
    }

    /// Updates the current drag position.
    pub fn update(&mut self, x: i32, y: i32) {
        self.current_x = x;
        self.current_y = y;
    }

    /// Ends the drag.
    pub fn end(&mut self) {
        self.active = false;
    }

    /// Returns the drag rectangle as (x, y, w, h) with normalized coordinates.
    pub fn rect(&self) -> (i32, i32, u32, u32) {
        let x = self.start_x.min(self.current_x);
        let y = self.start_y.min(self.current_y);
        let w = (self.start_x - self.current_x).unsigned_abs();
        let h = (self.start_y - self.current_y).unsigned_abs();
        (x, y, w, h)
    }

    /// Returns the drag as a line (start -> current).
    pub fn line(&self) -> (i32, i32, i32, i32) {
        (self.start_x, self.start_y, self.current_x, self.current_y)
    }
}

impl Default for DragState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// The complete paint application state.
pub struct PaintApp {
    /// Window width.
    pub window_width: f32,
    /// Window height.
    pub window_height: f32,
    /// Canvas width in pixels.
    pub canvas_width: u32,
    /// Canvas height in pixels.
    pub canvas_height: u32,
    /// Canvas background color.
    pub canvas_bg: Color,
    /// All layers (bottom to top).
    pub layers: Vec<Layer>,
    /// Index of the active (currently edited) layer.
    pub active_layer: usize,
    /// Current drawing tool.
    pub current_tool: Tool,
    /// Shape fill mode for rectangle, ellipse, polygon, rounded rect.
    pub shape_mode: ShapeMode,
    /// Brush settings (size, opacity, hardness).
    pub brush: BrushSettings,
    /// Foreground (primary) drawing color.
    pub fg_color: Color,
    /// Background (secondary) drawing color.
    pub bg_color: Color,
    /// 48-color palette.
    pub palette: Vec<Color>,
    /// Recently used colors.
    pub recent_colors: VecDeque<Color>,
    /// Color picker state.
    pub color_picker: ColorPicker,
    /// Undo/redo history.
    pub history: History,
    /// Current selection (if any).
    pub selection: Option<Selection>,
    /// Clipboard for copy/paste.
    pub clipboard: Clipboard,
    /// Polygon builder for the polygon tool.
    pub polygon_builder: PolygonBuilder,
    /// Text tool input buffer.
    pub text_input: TextInput,
    /// Current zoom level (1.0 = 100%).
    pub zoom: f32,
    /// Canvas scroll offset X (pixels in canvas space).
    pub scroll_x: f32,
    /// Canvas scroll offset Y (pixels in canvas space).
    pub scroll_y: f32,
    /// Grid overlay settings.
    pub grid: GridSettings,
    /// Mouse drag tracking.
    pub drag: DragState,
    /// Current mouse position in canvas coordinates.
    pub mouse_canvas_x: i32,
    /// Current mouse position in canvas coordinates.
    pub mouse_canvas_y: i32,
    /// Current mouse position in window coordinates.
    pub mouse_window_x: f32,
    /// Current mouse position in window coordinates.
    pub mouse_window_y: f32,
    /// Whether the mouse is currently pressed on the canvas.
    pub mouse_down: bool,
    /// Whether the selection is being moved.
    pub moving_selection: bool,
    /// Spray can RNG seed state.
    pub spray_seed: u32,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Rounded rectangle corner radius.
    pub rounded_rect_radius: i32,
    /// Text tool font size.
    pub text_font_size: f32,
}

impl PaintApp {
    /// Creates a new paint application with default settings.
    pub fn new(window_width: f32, window_height: f32) -> Self {
        let canvas_width = DEFAULT_CANVAS_WIDTH;
        let canvas_height = DEFAULT_CANVAS_HEIGHT;
        let bg = Color::WHITE;

        let layers = vec![Layer::with_background(
            "Background".to_string(),
            canvas_width,
            canvas_height,
            bg,
        )];

        Self {
            window_width,
            window_height,
            canvas_width,
            canvas_height,
            canvas_bg: bg,
            layers,
            active_layer: 0,
            current_tool: Tool::Pencil,
            shape_mode: ShapeMode::Outline,
            brush: BrushSettings::new(),
            fg_color: Color::BLACK,
            bg_color: Color::WHITE,
            palette: default_palette(),
            recent_colors: VecDeque::new(),
            color_picker: ColorPicker::default(),
            history: History::new(MAX_UNDO_STEPS),
            selection: None,
            clipboard: Clipboard::new(),
            polygon_builder: PolygonBuilder::new(),
            text_input: TextInput::new(),
            zoom: 1.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            grid: GridSettings::new(),
            drag: DragState::new(),
            mouse_canvas_x: 0,
            mouse_canvas_y: 0,
            mouse_window_x: 0.0,
            mouse_window_y: 0.0,
            mouse_down: false,
            moving_selection: false,
            spray_seed: 42,
            should_quit: false,
            rounded_rect_radius: 12,
            text_font_size: 16.0,
        }
    }

    // ========================================================================
    // Canvas coordinate conversion
    // ========================================================================

    /// Converts window (screen) coordinates to canvas pixel coordinates.
    pub fn window_to_canvas(&self, wx: f32, wy: f32) -> (i32, i32) {
        let canvas_area_x = TOOLBAR_WIDTH;
        let canvas_area_y = OPTION_BAR_HEIGHT;
        let cx = ((wx - canvas_area_x) / self.zoom + self.scroll_x) as i32;
        let cy = ((wy - canvas_area_y) / self.zoom + self.scroll_y) as i32;
        (cx, cy)
    }

    /// Converts canvas pixel coordinates to window (screen) coordinates.
    pub fn canvas_to_window(&self, cx: f32, cy: f32) -> (f32, f32) {
        let wx = (cx - self.scroll_x) * self.zoom + TOOLBAR_WIDTH;
        let wy = (cy - self.scroll_y) * self.zoom + OPTION_BAR_HEIGHT;
        (wx, wy)
    }

    /// Returns the visible canvas area in window coordinates.
    pub fn canvas_viewport(&self) -> (f32, f32, f32, f32) {
        let x = TOOLBAR_WIDTH;
        let y = OPTION_BAR_HEIGHT;
        let w = self.window_width - TOOLBAR_WIDTH - LAYERS_PANEL_WIDTH;
        let h = self.window_height - OPTION_BAR_HEIGHT - STATUS_BAR_HEIGHT;
        (x, y, w.max(0.0), h.max(0.0))
    }

    // ========================================================================
    // Zoom control
    // ========================================================================

    /// Sets the zoom level, clamped to allowed range.
    pub fn set_zoom(&mut self, new_zoom: f32) {
        self.zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Zooms in by one step.
    pub fn zoom_in(&mut self) {
        let steps = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];
        for &s in &steps {
            if s > self.zoom + 0.01 {
                self.set_zoom(s);
                return;
            }
        }
    }

    /// Zooms out by one step.
    pub fn zoom_out(&mut self) {
        let steps = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];
        for &s in steps.iter().rev() {
            if s < self.zoom - 0.01 {
                self.set_zoom(s);
                return;
            }
        }
    }

    /// Sets zoom to fit the canvas in the viewport.
    pub fn zoom_fit(&mut self) {
        let (_, _, vw, vh) = self.canvas_viewport();
        if vw <= 0.0 || vh <= 0.0 {
            return;
        }
        let zoom_x = vw / self.canvas_width as f32;
        let zoom_y = vh / self.canvas_height as f32;
        self.set_zoom(zoom_x.min(zoom_y));
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
    }

    /// Sets zoom to 100%.
    pub fn zoom_actual(&mut self) {
        self.set_zoom(1.0);
    }

    /// Returns the zoom percentage as a string.
    pub fn zoom_percent_str(&self) -> String {
        format!("{}%", (self.zoom * 100.0) as u32)
    }

    // ========================================================================
    // Layer management
    // ========================================================================

    /// Adds a new transparent layer above the active layer.
    pub fn add_layer(&mut self) {
        let idx = self.layers.len();
        let name = format!("Layer {}", idx + 1);
        self.layers.push(Layer::new(name, self.canvas_width, self.canvas_height));
        self.active_layer = self.layers.len() - 1;
    }

    /// Deletes the active layer. Cannot delete the last layer.
    pub fn delete_layer(&mut self) -> bool {
        if self.layers.len() <= 1 {
            return false;
        }
        self.layers.remove(self.active_layer);
        if self.active_layer >= self.layers.len() {
            self.active_layer = self.layers.len() - 1;
        }
        true
    }

    /// Moves the active layer up (towards the top). Returns true on success.
    pub fn move_layer_up(&mut self) -> bool {
        if self.active_layer + 1 < self.layers.len() {
            self.layers.swap(self.active_layer, self.active_layer + 1);
            self.active_layer += 1;
            true
        } else {
            false
        }
    }

    /// Moves the active layer down (towards the bottom). Returns true on success.
    pub fn move_layer_down(&mut self) -> bool {
        if self.active_layer > 0 {
            self.layers.swap(self.active_layer, self.active_layer - 1);
            self.active_layer -= 1;
            true
        } else {
            false
        }
    }

    /// Merges the active layer down onto the layer below it.
    pub fn merge_layer_down(&mut self) -> bool {
        if self.active_layer == 0 || self.layers.len() <= 1 {
            return false;
        }
        let upper = self.layers.remove(self.active_layer);
        self.active_layer -= 1;
        let lower = &mut self.layers[self.active_layer];

        // Blend upper onto lower
        for y in 0..upper.pixels.height {
            for x in 0..upper.pixels.width {
                if let Some(mut c) = upper.pixels.get_pixel(x, y) {
                    // Apply layer opacity
                    c = Color::rgba(c.r, c.g, c.b, (c.a as f32 * upper.opacity) as u8);
                    lower.pixels.blend_pixel(x, y, c);
                }
            }
        }
        true
    }

    /// Returns the active layer, or None if index is out of range.
    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.get_mut(self.active_layer)
    }

    /// Returns a reference to the active layer.
    pub fn active_layer_ref(&self) -> Option<&Layer> {
        self.layers.get(self.active_layer)
    }

    // ========================================================================
    // Composite / flatten
    // ========================================================================

    /// Flattens all visible layers into a single pixel buffer.
    pub fn flatten(&self) -> PixelBuffer {
        let mut result = PixelBuffer::new(self.canvas_width, self.canvas_height, self.canvas_bg);

        for layer in &self.layers {
            if !layer.visible {
                continue;
            }
            for y in 0..self.canvas_height {
                for x in 0..self.canvas_width {
                    if let Some(mut c) = layer.pixels.get_pixel(x, y) {
                        c = Color::rgba(c.r, c.g, c.b, (c.a as f32 * layer.opacity) as u8);
                        if c.a > 0 {
                            result.blend_pixel(x, y, c);
                        }
                    }
                }
            }
        }

        result
    }

    // ========================================================================
    // History (undo/redo)
    // ========================================================================

    /// Takes a snapshot of the current state and pushes it to history.
    pub fn push_history(&mut self, description: &str) {
        let snapshot = HistorySnapshot {
            layers: self.layers.clone(),
            active_layer: self.active_layer,
            description: description.to_string(),
        };
        self.history.push(snapshot);
    }

    /// Undoes the last action. Returns true if undo was performed.
    pub fn undo(&mut self) -> bool {
        let current = HistorySnapshot {
            layers: self.layers.clone(),
            active_layer: self.active_layer,
            description: String::new(),
        };
        if let Some(prev) = self.history.undo(current) {
            self.layers = prev.layers;
            self.active_layer = prev.active_layer;
            true
        } else {
            false
        }
    }

    /// Redoes the last undone action. Returns true if redo was performed.
    pub fn redo(&mut self) -> bool {
        let current = HistorySnapshot {
            layers: self.layers.clone(),
            active_layer: self.active_layer,
            description: String::new(),
        };
        if let Some(next) = self.history.redo(current) {
            self.layers = next.layers;
            self.active_layer = next.active_layer;
            true
        } else {
            false
        }
    }

    // ========================================================================
    // Color management
    // ========================================================================

    /// Swaps foreground and background colors.
    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.fg_color, &mut self.bg_color);
    }

    /// Adds a color to the recent colors list.
    pub fn add_recent_color(&mut self, color: Color) {
        // Remove duplicates
        self.recent_colors.retain(|c| *c != color);
        self.recent_colors.push_front(color);
        while self.recent_colors.len() > MAX_RECENT_COLORS {
            self.recent_colors.pop_back();
        }
    }

    /// Returns the current drawing color (foreground with brush opacity applied).
    pub fn drawing_color(&self) -> Color {
        self.brush.apply_opacity(self.fg_color)
    }

    /// Returns the eraser color (background).
    pub fn eraser_color(&self) -> Color {
        self.bg_color
    }

    // ========================================================================
    // Selection operations
    // ========================================================================

    /// Copies the selected region from the active layer.
    pub fn copy_selection(&mut self) {
        if let Some(sel) = &self.selection {
            if !sel.has_area() {
                return;
            }
            if let Some(layer) = self.layers.get(self.active_layer) {
                let region = layer.pixels.copy_region(
                    sel.x.max(0) as u32,
                    sel.y.max(0) as u32,
                    sel.width,
                    sel.height,
                );
                self.clipboard.store(region);
            }
        }
    }

    /// Cuts the selected region from the active layer.
    pub fn cut_selection(&mut self) {
        self.copy_selection();
        if let Some(sel) = &self.selection {
            if !sel.has_area() {
                return;
            }
            let sx = sel.x;
            let sy = sel.y;
            let sw = sel.width;
            let sh = sel.height;
            if let Some(layer) = self.layers.get_mut(self.active_layer) {
                for dy in 0..sh {
                    for dx in 0..sw {
                        let px = sx + dx as i32;
                        let py = sy + dy as i32;
                        if px >= 0 && py >= 0 {
                            layer.pixels.set_pixel(px as u32, py as u32, Color::TRANSPARENT);
                        }
                    }
                }
            }
        }
    }

    /// Pastes clipboard content at the top-left of the canvas.
    pub fn paste(&mut self) {
        if let Some(content) = self.clipboard.get().cloned() {
            self.push_history("paste");
            let sel = Selection {
                x: 0,
                y: 0,
                width: content.width,
                height: content.height,
                content: Some(content),
            };
            self.selection = Some(sel);
        }
    }

    /// Applies the floating selection (pastes it onto the active layer).
    pub fn apply_selection(&mut self) {
        if let Some(sel) = self.selection.take()
            && let Some(content) = &sel.content
                && let Some(layer) = self.layers.get_mut(self.active_layer) {
                    layer.pixels.paste(content, sel.x, sel.y);
                }
    }

    /// Crops the canvas to the current selection.
    pub fn crop_to_selection(&mut self) {
        let (sx, sy, sw, sh) = match &self.selection {
            Some(sel) if sel.has_area() => {
                (sel.x.max(0) as u32, sel.y.max(0) as u32, sel.width, sel.height)
            }
            _ => return,
        };

        self.push_history("crop");

        for layer in &mut self.layers {
            layer.pixels = layer.pixels.copy_region(sx, sy, sw, sh);
        }
        self.canvas_width = sw;
        self.canvas_height = sh;
        self.selection = None;
    }

    // ========================================================================
    // Transform operations
    // ========================================================================

    /// Flips the active layer horizontally.
    pub fn flip_horizontal(&mut self) {
        self.push_history("flip horizontal");
        if let Some(layer) = self.layers.get_mut(self.active_layer) {
            layer.pixels.flip_horizontal();
        }
    }

    /// Flips the active layer vertically.
    pub fn flip_vertical(&mut self) {
        self.push_history("flip vertical");
        if let Some(layer) = self.layers.get_mut(self.active_layer) {
            layer.pixels.flip_vertical();
        }
    }

    /// Rotates the active layer 90 degrees clockwise.
    pub fn rotate_90_cw(&mut self) {
        self.push_history("rotate 90 CW");
        if let Some(layer) = self.layers.get_mut(self.active_layer) {
            layer.pixels = layer.pixels.rotate_90_cw();
        }
    }

    /// Rotates the active layer 90 degrees counter-clockwise.
    pub fn rotate_90_ccw(&mut self) {
        self.push_history("rotate 90 CCW");
        if let Some(layer) = self.layers.get_mut(self.active_layer) {
            layer.pixels = layer.pixels.rotate_90_ccw();
        }
    }

    /// Rotates the active layer 180 degrees.
    pub fn rotate_180(&mut self) {
        self.push_history("rotate 180");
        if let Some(layer) = self.layers.get_mut(self.active_layer) {
            layer.pixels = layer.pixels.rotate_180();
        }
    }

    /// Resizes the canvas (and all layers) to new dimensions.
    pub fn resize_canvas(&mut self, new_width: u32, new_height: u32) {
        if new_width == 0 || new_height == 0 {
            return;
        }
        self.push_history("resize canvas");
        for layer in &mut self.layers {
            layer.pixels = layer.pixels.resize_nearest(new_width, new_height);
        }
        self.canvas_width = new_width;
        self.canvas_height = new_height;
    }

    // ========================================================================
    // File I/O
    // ========================================================================

    /// Saves the flattened image as a BMP to the given path.
    pub fn save_bmp(&self, path: &str) -> Result<(), String> {
        let flat = self.flatten();
        let data = encode_bmp(&flat);
        std::fs::write(path, &data).map_err(|e| format!("Failed to save BMP: {}", e))
    }

    /// Loads a BMP file into the active layer (replaces its content).
    pub fn load_bmp(&mut self, path: &str) -> Result<(), String> {
        let data = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
        let buf = decode_bmp(&data).ok_or_else(|| "Invalid BMP format".to_string())?;

        self.push_history("load BMP");
        self.canvas_width = buf.width;
        self.canvas_height = buf.height;

        // Replace all layers with a single background layer
        self.layers.clear();
        let mut layer = Layer::new("Background".to_string(), buf.width, buf.height);
        layer.pixels = buf;
        self.layers.push(layer);
        self.active_layer = 0;

        Ok(())
    }

    // ========================================================================
    // Drawing tool dispatch
    // ========================================================================

    /// Called when the mouse is pressed on the canvas.
    pub fn on_canvas_press(&mut self, canvas_x: i32, canvas_y: i32) {
        self.mouse_down = true;
        self.drag.begin(canvas_x, canvas_y);

        match self.current_tool {
            Tool::Pencil => {
                self.push_history("pencil stroke");
                let color = self.drawing_color();
                let size = self.brush.size;
                if let Some(layer) = self.layers.get_mut(self.active_layer) {
                    if size <= 1 {
                        layer.pixels.blend_pixel(canvas_x as u32, canvas_y as u32, color);
                    } else {
                        draw_filled_circle_at(
                            &mut layer.pixels,
                            canvas_x,
                            canvas_y,
                            (size / 2) as i32,
                            color,
                        );
                    }
                }
            }
            Tool::Eraser => {
                self.push_history("eraser stroke");
                let color = self.eraser_color();
                let size = self.brush.size;
                if let Some(layer) = self.layers.get_mut(self.active_layer) {
                    if size <= 1 {
                        layer.pixels.set_pixel(canvas_x as u32, canvas_y as u32, color);
                    } else {
                        let half = (size / 2) as i32;
                        let r2 = half * half;
                        for dy in -half..=half {
                            for dx in -half..=half {
                                if dx * dx + dy * dy <= r2 {
                                    let px = canvas_x + dx;
                                    let py = canvas_y + dy;
                                    if px >= 0 && py >= 0 {
                                        layer.pixels.set_pixel(px as u32, py as u32, color);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Tool::Fill => {
                self.push_history("flood fill");
                let color = self.drawing_color();
                if canvas_x >= 0 && canvas_y >= 0
                    && let Some(layer) = self.layers.get_mut(self.active_layer) {
                        flood_fill(&mut layer.pixels, canvas_x as u32, canvas_y as u32, color);
                    }
            }
            Tool::Eyedropper => {
                if canvas_x >= 0 && canvas_y >= 0 {
                    let flat = self.flatten();
                    if let Some(c) = flat.get_pixel(canvas_x as u32, canvas_y as u32) {
                        self.fg_color = c;
                        self.add_recent_color(c);
                    }
                }
            }
            Tool::SprayCan => {
                self.push_history("spray can");
                let color = self.drawing_color();
                let radius = (self.brush.size as i32) / 2;
                let density = self.brush.size.max(10);
                if let Some(layer) = self.layers.get_mut(self.active_layer) {
                    spray_paint(
                        &mut layer.pixels,
                        canvas_x,
                        canvas_y,
                        radius.max(1),
                        color,
                        density,
                        self.spray_seed,
                    );
                }
                self.spray_seed = self.spray_seed.wrapping_add(1);
            }
            Tool::Polygon => {
                if self.polygon_builder.vertex_count() == 0 {
                    self.push_history("polygon");
                }
                self.polygon_builder.add_point(canvas_x, canvas_y);
            }
            Tool::Text => {
                // Place text cursor; text input is handled via key events
                self.drag.begin(canvas_x, canvas_y);
            }
            Tool::Select => {
                // Check if clicking inside an existing selection
                if let Some(sel) = &self.selection
                    && sel.contains(canvas_x, canvas_y) {
                        self.moving_selection = true;
                        return;
                    }
                // Start a new selection
                self.apply_selection();
                self.selection = None;
                self.moving_selection = false;
            }
            Tool::Line | Tool::Rectangle | Tool::Ellipse | Tool::RoundedRectangle => {
                self.push_history(self.current_tool.label());
            }
        }
    }

    /// Called when the mouse moves on the canvas while pressed.
    pub fn on_canvas_drag(&mut self, canvas_x: i32, canvas_y: i32) {
        let prev_x = self.drag.current_x;
        let prev_y = self.drag.current_y;
        self.drag.update(canvas_x, canvas_y);

        match self.current_tool {
            Tool::Pencil => {
                let color = self.drawing_color();
                let size = self.brush.size;
                if let Some(layer) = self.layers.get_mut(self.active_layer) {
                    draw_line(&mut layer.pixels, prev_x, prev_y, canvas_x, canvas_y, color, size);
                }
            }
            Tool::Eraser => {
                let color = self.eraser_color();
                let size = self.brush.size;
                if let Some(layer) = self.layers.get_mut(self.active_layer) {
                    // For eraser, use overwrite not blend
                    draw_line(&mut layer.pixels, prev_x, prev_y, canvas_x, canvas_y, color, size);
                }
            }
            Tool::SprayCan => {
                let color = self.drawing_color();
                let radius = (self.brush.size as i32) / 2;
                let density = self.brush.size.max(10);
                if let Some(layer) = self.layers.get_mut(self.active_layer) {
                    spray_paint(
                        &mut layer.pixels,
                        canvas_x,
                        canvas_y,
                        radius.max(1),
                        color,
                        density,
                        self.spray_seed,
                    );
                }
                self.spray_seed = self.spray_seed.wrapping_add(1);
            }
            Tool::Select if self.moving_selection => {
                if let Some(sel) = &mut self.selection {
                    sel.x += canvas_x - prev_x;
                    sel.y += canvas_y - prev_y;
                }
            }
            Tool::Line
            | Tool::Rectangle
            | Tool::Ellipse
            | Tool::RoundedRectangle
            | Tool::Select => {
                // These tools use drag.start/current for preview; handled in render
            }
            _ => {}
        }
    }

    /// Called when the mouse is released on the canvas.
    pub fn on_canvas_release(&mut self, canvas_x: i32, canvas_y: i32) {
        self.mouse_down = false;
        self.drag.update(canvas_x, canvas_y);

        match self.current_tool {
            Tool::Line => {
                let (x0, y0, x1, y1) = self.drag.line();
                let color = self.drawing_color();
                let size = self.brush.size;
                if let Some(layer) = self.layers.get_mut(self.active_layer) {
                    draw_line(&mut layer.pixels, x0, y0, x1, y1, color, size);
                }
            }
            Tool::Rectangle => {
                let (rx, ry, rw, rh) = self.drag.rect();
                if rw > 0 && rh > 0 {
                    let color = self.drawing_color();
                    if let Some(layer) = self.layers.get_mut(self.active_layer) {
                        match self.shape_mode {
                            ShapeMode::Outline => {
                                draw_rect_outline(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    color,
                                    self.brush.size,
                                );
                            }
                            ShapeMode::Filled => {
                                draw_rect_filled(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    color,
                                );
                            }
                            ShapeMode::FilledWithOutline => {
                                draw_rect_filled(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    color,
                                );
                                draw_rect_outline(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    self.fg_color,
                                    self.brush.size,
                                );
                            }
                        }
                    }
                }
            }
            Tool::Ellipse => {
                let (rx, ry, rw, rh) = self.drag.rect();
                if rw > 0 && rh > 0 {
                    let cx = rx + rw as i32 / 2;
                    let cy = ry + rh as i32 / 2;
                    let erx = rw as i32 / 2;
                    let ery = rh as i32 / 2;
                    let color = self.drawing_color();
                    if let Some(layer) = self.layers.get_mut(self.active_layer) {
                        match self.shape_mode {
                            ShapeMode::Outline => {
                                draw_ellipse_outline(
                                    &mut layer.pixels,
                                    cx,
                                    cy,
                                    erx,
                                    ery,
                                    color,
                                    self.brush.size,
                                );
                            }
                            ShapeMode::Filled => {
                                draw_ellipse_filled(&mut layer.pixels, cx, cy, erx, ery, color);
                            }
                            ShapeMode::FilledWithOutline => {
                                draw_ellipse_filled(&mut layer.pixels, cx, cy, erx, ery, color);
                                draw_ellipse_outline(
                                    &mut layer.pixels,
                                    cx,
                                    cy,
                                    erx,
                                    ery,
                                    self.fg_color,
                                    self.brush.size,
                                );
                            }
                        }
                    }
                }
            }
            Tool::RoundedRectangle => {
                let (rx, ry, rw, rh) = self.drag.rect();
                if rw > 0 && rh > 0 {
                    let color = self.drawing_color();
                    let radius = self.rounded_rect_radius;
                    if let Some(layer) = self.layers.get_mut(self.active_layer) {
                        match self.shape_mode {
                            ShapeMode::Outline => {
                                draw_rounded_rect_outline(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    radius,
                                    color,
                                    self.brush.size,
                                );
                            }
                            ShapeMode::Filled => {
                                draw_rounded_rect_filled(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    radius,
                                    color,
                                );
                            }
                            ShapeMode::FilledWithOutline => {
                                draw_rounded_rect_filled(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    radius,
                                    color,
                                );
                                draw_rounded_rect_outline(
                                    &mut layer.pixels,
                                    rx,
                                    ry,
                                    rw as i32,
                                    rh as i32,
                                    radius,
                                    self.fg_color,
                                    self.brush.size,
                                );
                            }
                        }
                    }
                }
            }
            Tool::Select if !self.moving_selection => {
                let (rx, ry, rw, rh) = self.drag.rect();
                if rw > 0 && rh > 0 {
                    self.selection = Some(Selection::new(rx, ry, rw, rh));
                }
            }
            _ => {}
        }

        self.drag.end();
        self.moving_selection = false;

        // Add foreground color to recent
        if matches!(
            self.current_tool,
            Tool::Pencil
                | Tool::Line
                | Tool::Rectangle
                | Tool::Ellipse
                | Tool::RoundedRectangle
                | Tool::Fill
                | Tool::SprayCan
        ) {
            self.add_recent_color(self.fg_color);
        }
    }

    /// Finishes the polygon (double-click or Enter).
    pub fn finish_polygon(&mut self) {
        if self.polygon_builder.vertex_count() < 3 {
            self.polygon_builder.reset();
            return;
        }
        self.polygon_builder.close();

        let color = self.drawing_color();
        if let Some(layer) = self.layers.get_mut(self.active_layer) {
            match self.shape_mode {
                ShapeMode::Outline => {
                    self.polygon_builder
                        .draw_outline(&mut layer.pixels, color, self.brush.size);
                }
                ShapeMode::Filled => {
                    self.polygon_builder.draw_filled(&mut layer.pixels, color);
                }
                ShapeMode::FilledWithOutline => {
                    self.polygon_builder.draw_filled(&mut layer.pixels, color);
                    self.polygon_builder.draw_outline(
                        &mut layer.pixels,
                        self.fg_color,
                        self.brush.size,
                    );
                }
            }
        }
        self.polygon_builder.reset();
    }

    /// Places text on the canvas at the text tool anchor point.
    pub fn place_text(&mut self) {
        let text_str = self.text_input.text.clone();
        if text_str.is_empty() {
            return;
        }
        self.push_history("text");
        // Text is rendered as UI commands; for the pixel buffer we draw a basic
        // rasterized version (each character as a small filled block).
        // In a real system the compositor would rasterize the font; here we
        // approximate by treating each character as ~8px wide, font_size tall.
        let tx = self.drag.start_x;
        let ty = self.drag.start_y;
        let color = self.drawing_color();
        let char_w = (self.text_font_size * 0.6) as i32;
        let char_h = self.text_font_size as i32;

        if let Some(layer) = self.layers.get_mut(self.active_layer) {
            for (i, _ch) in text_str.chars().enumerate() {
                let cx = tx + (i as i32) * char_w;
                // Draw a small rectangle for each character (placeholder raster)
                draw_rect_filled(&mut layer.pixels, cx, ty, char_w - 1, char_h, color);
            }
        }
        self.text_input.clear();
    }

    // ========================================================================
    // Rendering — produce RenderCommands for the compositor
    // ========================================================================

    /// Renders the entire application UI and returns all render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        // Background fill
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: MOCHA_CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_option_bar(&mut cmds);
        self.render_toolbar(&mut cmds);
        self.render_canvas_area(&mut cmds);
        self.render_layers_panel(&mut cmds);
        self.render_status_bar(&mut cmds);

        if self.color_picker.is_open {
            self.render_color_picker_dialog(&mut cmds);
        }

        cmds
    }

    /// Renders the top option bar (tool options, shape mode, zoom).
    fn render_option_bar(&self, cmds: &mut Vec<RenderCommand>) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: OPTION_BAR_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: OPTION_BAR_HEIGHT,
            x2: self.window_width,
            y2: OPTION_BAR_HEIGHT,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });

        let mut ox = 8.0;

        // Tool name
        cmds.push(RenderCommand::Text {
            x: ox,
            y: 10.0,
            text: format!("Tool: {}", self.current_tool.label()),
            font_size: 13.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        ox += 90.0;

        // Brush size
        cmds.push(RenderCommand::Text {
            x: ox,
            y: 10.0,
            text: format!("Size: {}px", self.brush.size),
            font_size: 12.0,
            color: MOCHA_SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        ox += 80.0;

        // Opacity
        cmds.push(RenderCommand::Text {
            x: ox,
            y: 10.0,
            text: format!("Opacity: {}%", (self.brush.opacity * 100.0) as u32),
            font_size: 12.0,
            color: MOCHA_SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        ox += 100.0;

        // Hardness
        cmds.push(RenderCommand::Text {
            x: ox,
            y: 10.0,
            text: format!("Hard: {}%", (self.brush.hardness * 100.0) as u32),
            font_size: 12.0,
            color: MOCHA_YELLOW,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        ox += 80.0;

        // Shape mode (for shape tools)
        if matches!(
            self.current_tool,
            Tool::Rectangle | Tool::Ellipse | Tool::Polygon | Tool::RoundedRectangle
        ) {
            let mode_str = match self.shape_mode {
                ShapeMode::Outline => "Outline",
                ShapeMode::Filled => "Filled",
                ShapeMode::FilledWithOutline => "Fill+Line",
            };
            cmds.push(RenderCommand::Text {
                x: ox,
                y: 10.0,
                text: format!("Mode: {}", mode_str),
                font_size: 12.0,
                color: MOCHA_LAVENDER,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            ox += 100.0;
        }

        // Zoom
        cmds.push(RenderCommand::Text {
            x: ox,
            y: 10.0,
            text: format!("Zoom: {}", self.zoom_percent_str()),
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        ox += 100.0;

        // Grid status
        if self.grid.visible {
            cmds.push(RenderCommand::Text {
                x: ox,
                y: 10.0,
                text: format!("Grid: {}px", self.grid.spacing),
                font_size: 12.0,
                color: MOCHA_GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Renders the left-side tool panel.
    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        let tb_x = 0.0;
        let tb_y = OPTION_BAR_HEIGHT;
        let tb_h = self.window_height - OPTION_BAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: tb_x,
            y: tb_y,
            width: TOOLBAR_WIDTH,
            height: tb_h,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Right border
        cmds.push(RenderCommand::Line {
            x1: TOOLBAR_WIDTH,
            y1: tb_y,
            x2: TOOLBAR_WIDTH,
            y2: tb_y + tb_h,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });

        // Tool buttons
        let tools = Tool::all();
        for (i, &tool) in tools.iter().enumerate() {
            let btn_x = tb_x + 4.0;
            let btn_y = tb_y + 4.0 + i as f32 * 36.0;
            let btn_w = TOOLBAR_WIDTH - 8.0;
            let btn_h = 32.0;

            let is_active = tool == self.current_tool;
            let bg = if is_active { MOCHA_BLUE } else { MOCHA_SURFACE0 };
            let fg = if is_active { MOCHA_CRUST } else { MOCHA_TEXT };

            cmds.push(RenderCommand::FillRect {
                x: btn_x,
                y: btn_y,
                width: btn_w,
                height: btn_h,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: btn_x + 4.0,
                y: btn_y + 9.0,
                text: tool.label().to_string(),
                font_size: 11.0,
                color: fg,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(btn_w - 8.0),
            });
        }

        // Color swatches (below tools)
        let swatch_y = tb_y + 4.0 + tools.len() as f32 * 36.0 + 8.0;
        self.render_color_swatches(cmds, tb_x + 4.0, swatch_y);

        // Palette below swatches (constrained to PALETTE_HEIGHT area)
        let palette_y = swatch_y + 50.0;
        let palette_max_y = palette_y + PALETTE_HEIGHT;
        cmds.push(RenderCommand::PushClip {
            x: tb_x,
            y: palette_y,
            width: TOOLBAR_WIDTH,
            height: palette_max_y - palette_y,
        });
        self.render_palette_compact(cmds, tb_x, palette_y);
        cmds.push(RenderCommand::PopClip);
    }

    /// Renders foreground/background color swatches.
    fn render_color_swatches(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        // Background color (offset behind)
        cmds.push(RenderCommand::FillRect {
            x: x + 14.0,
            y: y + 14.0,
            width: 22.0,
            height: 22.0,
            color: self.bg_color,
            corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: x + 14.0,
            y: y + 14.0,
            width: 22.0,
            height: 22.0,
            color: MOCHA_OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Foreground color (overlapping in front)
        cmds.push(RenderCommand::FillRect {
            x: x + 2.0,
            y: y + 2.0,
            width: 22.0,
            height: 22.0,
            color: self.fg_color,
            corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: x + 2.0,
            y: y + 2.0,
            width: 22.0,
            height: 22.0,
            color: MOCHA_TEXT,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Swap indicator
        cmds.push(RenderCommand::Text {
            x: x + 28.0,
            y,
            text: "X".to_string(),
            font_size: 9.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Renders a compact palette display in the toolbar.
    fn render_palette_compact(&self, cmds: &mut Vec<RenderCommand>, base_x: f32, base_y: f32) {
        let cell_size = 6.0;
        let cols = 6u32;

        for (i, color) in self.palette.iter().enumerate() {
            let col = (i as u32) % cols;
            let row = (i as u32) / cols;
            let px = base_x + 3.0 + col as f32 * (cell_size + 1.0);
            let py = base_y + row as f32 * (cell_size + 1.0);

            cmds.push(RenderCommand::FillRect {
                x: px,
                y: py,
                width: cell_size,
                height: cell_size,
                color: *color,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }

    /// Renders the canvas area (all visible layers composited).
    fn render_canvas_area(&self, cmds: &mut Vec<RenderCommand>) {
        let (vx, vy, vw, vh) = self.canvas_viewport();

        // Canvas area background (checkerboard implied by dark bg)
        cmds.push(RenderCommand::FillRect {
            x: vx,
            y: vy,
            width: vw,
            height: vh,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Clip to canvas viewport
        cmds.push(RenderCommand::PushClip {
            x: vx,
            y: vy,
            width: vw,
            height: vh,
        });

        // Canvas background (white or user-set)
        let (cwx, cwy) = self.canvas_to_window(0.0, 0.0);
        let cw_scaled = self.canvas_width as f32 * self.zoom;
        let ch_scaled = self.canvas_height as f32 * self.zoom;

        // Checkerboard pattern for transparency
        self.render_checkerboard(cmds, cwx, cwy, cw_scaled, ch_scaled);

        // Draw each visible layer
        // For render commands we approximate by drawing colored rectangles for
        // each non-transparent pixel region. In a real compositor the pixel buffer
        // would be blitted directly. Here we draw one rect per horizontal run of
        // same-color pixels as an optimization.
        for layer in &self.layers {
            if !layer.visible {
                continue;
            }
            self.render_layer_pixels(cmds, layer, cwx, cwy);
        }

        // Grid overlay
        if self.grid.visible {
            self.render_grid_overlay(cmds, cwx, cwy, cw_scaled, ch_scaled);
        }

        // Selection marquee
        if let Some(sel) = &self.selection {
            self.render_selection_marquee(cmds, sel);
        }

        // Drag preview for shape tools
        if self.drag.active {
            self.render_drag_preview(cmds);
        }

        // Polygon builder preview
        if self.polygon_builder.vertex_count() > 0 {
            self.render_polygon_preview(cmds);
        }

        cmds.push(RenderCommand::PopClip);

        // Canvas border
        cmds.push(RenderCommand::StrokeRect {
            x: cwx - 1.0,
            y: cwy - 1.0,
            width: cw_scaled + 2.0,
            height: ch_scaled + 2.0,
            color: MOCHA_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::ZERO,
        });
    }

    /// Renders a checkerboard pattern to indicate transparency.
    fn render_checkerboard(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let check_size = 8.0 * self.zoom;
        let light = Color::rgb(220, 220, 220);
        let dark = Color::rgb(180, 180, 180);

        // Fill with light first
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: light,
            corner_radii: CornerRadii::ZERO,
        });

        // Draw dark squares
        let cols = (w / check_size).ceil() as u32;
        let rows = (h / check_size).ceil() as u32;
        for row in 0..rows {
            for col in 0..cols {
                if (row + col) % 2 == 1 {
                    cmds.push(RenderCommand::FillRect {
                        x: x + col as f32 * check_size,
                        y: y + row as f32 * check_size,
                        width: check_size,
                        height: check_size,
                        color: dark,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }
    }

    /// Renders pixels from a single layer as render commands.
    fn render_layer_pixels(
        &self,
        cmds: &mut Vec<RenderCommand>,
        layer: &Layer,
        base_x: f32,
        base_y: f32,
    ) {
        // Render as horizontal spans for efficiency
        let z = self.zoom;
        let pixel_w = z.max(1.0);
        let pixel_h = z.max(1.0);

        for py in 0..layer.pixels.height {
            let mut run_start: Option<(u32, Color)> = None;

            for px in 0..layer.pixels.width {
                let raw_color = match layer.pixels.get_pixel(px, py) {
                    Some(c) => c,
                    None => continue,
                };

                let mut c = raw_color;
                // Apply layer opacity
                c = Color::rgba(c.r, c.g, c.b, (c.a as f32 * layer.opacity) as u8);

                if c.a == 0 {
                    // End current run if any
                    if let Some((start, run_color)) = run_start.take() {
                        let run_len = px - start;
                        cmds.push(RenderCommand::FillRect {
                            x: base_x + start as f32 * z,
                            y: base_y + py as f32 * z,
                            width: run_len as f32 * pixel_w,
                            height: pixel_h,
                            color: run_color,
                            corner_radii: CornerRadii::ZERO,
                        });
                    }
                    continue;
                }

                match run_start {
                    Some((_, run_color)) if run_color == c => {
                        // Continue the run
                    }
                    Some((start, run_color)) => {
                        // End previous run, start new one
                        let run_len = px - start;
                        cmds.push(RenderCommand::FillRect {
                            x: base_x + start as f32 * z,
                            y: base_y + py as f32 * z,
                            width: run_len as f32 * pixel_w,
                            height: pixel_h,
                            color: run_color,
                            corner_radii: CornerRadii::ZERO,
                        });
                        run_start = Some((px, c));
                    }
                    None => {
                        run_start = Some((px, c));
                    }
                }
            }

            // Flush last run
            if let Some((start, run_color)) = run_start {
                let run_len = layer.pixels.width - start;
                cmds.push(RenderCommand::FillRect {
                    x: base_x + start as f32 * z,
                    y: base_y + py as f32 * z,
                    width: run_len as f32 * pixel_w,
                    height: pixel_h,
                    color: run_color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
    }

    /// Renders the grid overlay on the canvas.
    fn render_grid_overlay(
        &self,
        cmds: &mut Vec<RenderCommand>,
        base_x: f32,
        base_y: f32,
        w: f32,
        h: f32,
    ) {
        let spacing = self.grid.spacing as f32 * self.zoom;
        let color = self.grid.color;

        if spacing < 2.0 {
            return;
        }

        // Vertical lines
        let mut gx = 0.0;
        while gx <= w {
            cmds.push(RenderCommand::Line {
                x1: base_x + gx,
                y1: base_y,
                x2: base_x + gx,
                y2: base_y + h,
                color,
                width: 1.0,
            });
            gx += spacing;
        }

        // Horizontal lines
        let mut gy = 0.0;
        while gy <= h {
            cmds.push(RenderCommand::Line {
                x1: base_x,
                y1: base_y + gy,
                x2: base_x + w,
                y2: base_y + gy,
                color,
                width: 1.0,
            });
            gy += spacing;
        }
    }

    /// Renders the selection marquee (marching ants outline).
    fn render_selection_marquee(&self, cmds: &mut Vec<RenderCommand>, sel: &Selection) {
        let (sx, sy) = self.canvas_to_window(sel.x as f32, sel.y as f32);
        let sw = sel.width as f32 * self.zoom;
        let sh = sel.height as f32 * self.zoom;

        // Dashed outline (approximated with dotted stroke)
        cmds.push(RenderCommand::StrokeRect {
            x: sx,
            y: sy,
            width: sw,
            height: sh,
            color: Color::WHITE,
            line_width: 1.0,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::StrokeRect {
            x: sx + 1.0,
            y: sy + 1.0,
            width: sw - 2.0,
            height: sh - 2.0,
            color: Color::BLACK,
            line_width: 1.0,
            corner_radii: CornerRadii::ZERO,
        });

        // Show floating content if any
        if let Some(content) = &sel.content {
            // Render the floating content at the selection position
            let layer = Layer {
                name: String::new(),
                pixels: content.clone(),
                visible: true,
                opacity: 1.0,
            };
            self.render_layer_pixels(cmds, &layer, sx, sy);
        }
    }

    /// Renders a preview for drag-based shape tools.
    fn render_drag_preview(&self, cmds: &mut Vec<RenderCommand>) {
        let color = Color::rgba(self.fg_color.r, self.fg_color.g, self.fg_color.b, 160);

        match self.current_tool {
            Tool::Line => {
                let (sx, sy) = self.canvas_to_window(
                    self.drag.start_x as f32,
                    self.drag.start_y as f32,
                );
                let (ex, ey) = self.canvas_to_window(
                    self.drag.current_x as f32,
                    self.drag.current_y as f32,
                );
                cmds.push(RenderCommand::Line {
                    x1: sx,
                    y1: sy,
                    x2: ex,
                    y2: ey,
                    color,
                    width: self.brush.size as f32 * self.zoom,
                });
            }
            Tool::Rectangle => {
                let (rx, ry, rw, rh) = self.drag.rect();
                let (wx, wy) = self.canvas_to_window(rx as f32, ry as f32);
                cmds.push(RenderCommand::StrokeRect {
                    x: wx,
                    y: wy,
                    width: rw as f32 * self.zoom,
                    height: rh as f32 * self.zoom,
                    color,
                    line_width: self.brush.size as f32 * self.zoom,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            Tool::Ellipse => {
                let (rx, ry, rw, rh) = self.drag.rect();
                let (wx, wy) = self.canvas_to_window(rx as f32, ry as f32);
                // Approximate ellipse preview with a rounded rect
                let radius = (rw.min(rh) as f32 * self.zoom) / 2.0;
                cmds.push(RenderCommand::StrokeRect {
                    x: wx,
                    y: wy,
                    width: rw as f32 * self.zoom,
                    height: rh as f32 * self.zoom,
                    color,
                    line_width: self.brush.size as f32 * self.zoom,
                    corner_radii: CornerRadii::all(radius),
                });
            }
            Tool::RoundedRectangle => {
                let (rx, ry, rw, rh) = self.drag.rect();
                let (wx, wy) = self.canvas_to_window(rx as f32, ry as f32);
                let radius = self.rounded_rect_radius as f32 * self.zoom;
                cmds.push(RenderCommand::StrokeRect {
                    x: wx,
                    y: wy,
                    width: rw as f32 * self.zoom,
                    height: rh as f32 * self.zoom,
                    color,
                    line_width: self.brush.size as f32 * self.zoom,
                    corner_radii: CornerRadii::all(radius),
                });
            }
            Tool::Select => {
                let (rx, ry, rw, rh) = self.drag.rect();
                let (wx, wy) = self.canvas_to_window(rx as f32, ry as f32);
                cmds.push(RenderCommand::StrokeRect {
                    x: wx,
                    y: wy,
                    width: rw as f32 * self.zoom,
                    height: rh as f32 * self.zoom,
                    color: MOCHA_BLUE,
                    line_width: 1.0,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            _ => {}
        }
    }

    /// Renders the in-progress polygon.
    fn render_polygon_preview(&self, cmds: &mut Vec<RenderCommand>) {
        let pts = &self.polygon_builder.points;
        let color = Color::rgba(self.fg_color.r, self.fg_color.g, self.fg_color.b, 180);

        for i in 0..pts.len() {
            let (wx, wy) = self.canvas_to_window(pts[i].0 as f32, pts[i].1 as f32);

            // Draw vertex dot
            cmds.push(RenderCommand::FillRect {
                x: wx - 3.0,
                y: wy - 3.0,
                width: 6.0,
                height: 6.0,
                color: MOCHA_RED,
                corner_radii: CornerRadii::all(3.0),
            });

            // Draw edge to next vertex
            if i + 1 < pts.len() {
                let (wx2, wy2) =
                    self.canvas_to_window(pts[i + 1].0 as f32, pts[i + 1].1 as f32);
                cmds.push(RenderCommand::Line {
                    x1: wx,
                    y1: wy,
                    x2: wx2,
                    y2: wy2,
                    color,
                    width: 1.0,
                });
            }
        }
    }

    /// Renders the right-side layers panel.
    fn render_layers_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let px = self.window_width - LAYERS_PANEL_WIDTH;
        let py = OPTION_BAR_HEIGHT;
        let ph = self.window_height - OPTION_BAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: LAYERS_PANEL_WIDTH,
            height: ph,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::Line {
            x1: px,
            y1: py,
            x2: px,
            y2: py + ph,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: px + 8.0,
            y: py + 8.0,
            text: "Layers".to_string(),
            font_size: 13.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Layer buttons
        let btn_y = py + 26.0;
        let btn_labels = ["+", "-", "Up", "Dn", "Mrg"];
        let btn_w = 32.0;
        for (i, &label) in btn_labels.iter().enumerate() {
            let bx = px + 4.0 + i as f32 * (btn_w + 2.0);
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: btn_y,
                width: btn_w,
                height: 20.0,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 4.0,
                y: btn_y + 4.0,
                text: label.to_string(),
                font_size: 10.0,
                color: MOCHA_SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn_w - 8.0),
            });
        }

        // Layer list
        let list_y = btn_y + 28.0;
        for (i, layer) in self.layers.iter().enumerate().rev() {
            let ly = list_y + (self.layers.len() - 1 - i) as f32 * LAYER_ROW_HEIGHT;
            let is_active = i == self.active_layer;

            let bg = if is_active {
                MOCHA_SURFACE1
            } else {
                MOCHA_SURFACE0
            };

            cmds.push(RenderCommand::FillRect {
                x: px + 4.0,
                y: ly,
                width: LAYERS_PANEL_WIDTH - 8.0,
                height: LAYER_ROW_HEIGHT - 2.0,
                color: bg,
                corner_radii: CornerRadii::all(3.0),
            });

            // Active indicator
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: px + 4.0,
                    y: ly,
                    width: 3.0,
                    height: LAYER_ROW_HEIGHT - 2.0,
                    color: MOCHA_BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Visibility icon
            let vis_text = if layer.visible { "O" } else { "-" };
            let vis_color = if layer.visible {
                MOCHA_GREEN
            } else {
                MOCHA_OVERLAY0
            };
            cmds.push(RenderCommand::Text {
                x: px + 12.0,
                y: ly + 7.0,
                text: vis_text.to_string(),
                font_size: 11.0,
                color: vis_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Layer name
            cmds.push(RenderCommand::Text {
                x: px + 26.0,
                y: ly + 7.0,
                text: layer.name.clone(),
                font_size: 11.0,
                color: MOCHA_TEXT,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(LAYERS_PANEL_WIDTH - 70.0),
            });

            // Opacity
            let opacity_text = format!("{}%", (layer.opacity * 100.0) as u32);
            cmds.push(RenderCommand::Text {
                x: px + LAYERS_PANEL_WIDTH - 38.0,
                y: ly + 7.0,
                text: opacity_text,
                font_size: 10.0,
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Renders the bottom status bar.
    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let sy = self.window_height - STATUS_BAR_HEIGHT;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sy,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: sy,
            x2: self.window_width,
            y2: sy,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });

        let mut sx = 8.0;

        // Cursor position
        cmds.push(RenderCommand::Text {
            x: sx,
            y: sy + 5.0,
            text: format!("X: {} Y: {}", self.mouse_canvas_x, self.mouse_canvas_y),
            font_size: 11.0,
            color: MOCHA_SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        sx += 120.0;

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sx,
            y1: sy + 3.0,
            x2: sx,
            y2: sy + STATUS_BAR_HEIGHT - 3.0,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
        sx += 8.0;

        // Canvas size
        cmds.push(RenderCommand::Text {
            x: sx,
            y: sy + 5.0,
            text: format!("{}x{}", self.canvas_width, self.canvas_height),
            font_size: 11.0,
            color: MOCHA_SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        sx += 90.0;

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sx,
            y1: sy + 3.0,
            x2: sx,
            y2: sy + STATUS_BAR_HEIGHT - 3.0,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
        sx += 8.0;

        // Zoom
        cmds.push(RenderCommand::Text {
            x: sx,
            y: sy + 5.0,
            text: self.zoom_percent_str(),
            font_size: 11.0,
            color: MOCHA_SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        sx += 60.0;

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sx,
            y1: sy + 3.0,
            x2: sx,
            y2: sy + STATUS_BAR_HEIGHT - 3.0,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
        sx += 8.0;

        // Current tool
        cmds.push(RenderCommand::Text {
            x: sx,
            y: sy + 5.0,
            text: format!("Tool: {}", self.current_tool.label()),
            font_size: 11.0,
            color: MOCHA_BLUE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        sx += 80.0;

        // Selection dimensions (if any)
        if let Some(sel) = &self.selection {
            cmds.push(RenderCommand::Line {
                x1: sx,
                y1: sy + 3.0,
                x2: sx,
                y2: sy + STATUS_BAR_HEIGHT - 3.0,
                color: MOCHA_SURFACE0,
                width: 1.0,
            });
            sx += 8.0;

            cmds.push(RenderCommand::Text {
                x: sx,
                y: sy + 5.0,
                text: format!("Sel: {}x{}", sel.width, sel.height),
                font_size: 11.0,
                color: MOCHA_PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            sx += 80.0;
        }

        // Undo/redo counts
        cmds.push(RenderCommand::Line {
            x1: sx,
            y1: sy + 3.0,
            x2: sx,
            y2: sy + STATUS_BAR_HEIGHT - 3.0,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
        sx += 8.0;

        cmds.push(RenderCommand::Text {
            x: sx,
            y: sy + 5.0,
            text: format!(
                "Undo: {} Redo: {}",
                self.history.undo_count(),
                self.history.redo_count()
            ),
            font_size: 11.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Renders the color picker dialog (RGB sliders + hex input).
    fn render_color_picker_dialog(&self, cmds: &mut Vec<RenderCommand>) {
        let dlg_w = 280.0;
        let dlg_h = 300.0;
        let dlg_x = (self.window_width - dlg_w) / 2.0;
        let dlg_y = (self.window_height - dlg_h) / 2.0;

        // Shadow
        cmds.push(RenderCommand::BoxShadow {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 2.0,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::all(8.0),
        });

        // Dialog background
        cmds.push(RenderCommand::FillRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: MOCHA_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        let title = if self.color_picker.editing_foreground {
            "Foreground Color"
        } else {
            "Background Color"
        };
        cmds.push(RenderCommand::Text {
            x: dlg_x + 12.0,
            y: dlg_y + 12.0,
            text: title.to_string(),
            font_size: 14.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Color preview
        let preview_y = dlg_y + 36.0;
        cmds.push(RenderCommand::FillRect {
            x: dlg_x + 12.0,
            y: preview_y,
            width: 60.0,
            height: 40.0,
            color: self.color_picker.color(),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: dlg_x + 12.0,
            y: preview_y,
            width: 60.0,
            height: 40.0,
            color: MOCHA_OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Hex value display
        cmds.push(RenderCommand::Text {
            x: dlg_x + 80.0,
            y: preview_y + 4.0,
            text: format!(
                "#{:02X}{:02X}{:02X}",
                self.color_picker.red, self.color_picker.green, self.color_picker.blue
            ),
            font_size: 16.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // RGBA values
        cmds.push(RenderCommand::Text {
            x: dlg_x + 80.0,
            y: preview_y + 24.0,
            text: format!(
                "R:{} G:{} B:{} A:{}",
                self.color_picker.red,
                self.color_picker.green,
                self.color_picker.blue,
                self.color_picker.alpha
            ),
            font_size: 11.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Sliders
        let slider_x = dlg_x + 12.0;
        let slider_w = dlg_w - 24.0;
        let slider_labels = ["R", "G", "B", "A"];
        let slider_values = [
            self.color_picker.red,
            self.color_picker.green,
            self.color_picker.blue,
            self.color_picker.alpha,
        ];
        let slider_colors = [
            Color::rgb(255, 80, 80),
            Color::rgb(80, 200, 80),
            Color::rgb(80, 120, 255),
            Color::rgb(200, 200, 200),
        ];

        for i in 0..4 {
            let sy = preview_y + 52.0 + i as f32 * 40.0;

            // Label
            cmds.push(RenderCommand::Text {
                x: slider_x,
                y: sy,
                text: format!("{}: {}", slider_labels[i], slider_values[i]),
                font_size: 12.0,
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Slider track
            let track_y = sy + 16.0;
            cmds.push(RenderCommand::FillRect {
                x: slider_x,
                y: track_y,
                width: slider_w,
                height: 8.0,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            // Slider fill
            let fill_ratio = slider_values[i] as f32 / 255.0;
            cmds.push(RenderCommand::FillRect {
                x: slider_x,
                y: track_y,
                width: slider_w * fill_ratio,
                height: 8.0,
                color: slider_colors[i],
                corner_radii: CornerRadii::all(4.0),
            });

            // Slider handle
            let handle_x = slider_x + slider_w * fill_ratio - 6.0;
            cmds.push(RenderCommand::FillRect {
                x: handle_x,
                y: track_y - 2.0,
                width: 12.0,
                height: 12.0,
                color: Color::WHITE,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: handle_x,
                y: track_y - 2.0,
                width: 12.0,
                height: 12.0,
                color: MOCHA_SURFACE2,
                line_width: 1.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Hex input field
        let hex_y = preview_y + 52.0 + 4.0 * 40.0 + 8.0;
        cmds.push(RenderCommand::Text {
            x: slider_x,
            y: hex_y,
            text: "Hex:".to_string(),
            font_size: 12.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::FillRect {
            x: slider_x + 32.0,
            y: hex_y - 2.0,
            width: 100.0,
            height: 20.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        cmds.push(RenderCommand::Text {
            x: slider_x + 36.0,
            y: hex_y + 2.0,
            text: format!("#{}", self.color_picker.hex_input.as_str()),
            font_size: 12.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(92.0),
        });

        // OK / Cancel buttons
        let btn_y = hex_y + 28.0;
        let btn_labels_inner = ["OK", "Cancel"];
        let btn_colors = [MOCHA_GREEN, MOCHA_RED];
        for (i, &label) in btn_labels_inner.iter().enumerate() {
            let bx = slider_x + i as f32 * 80.0;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: btn_y,
                width: 70.0,
                height: 24.0,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: btn_y + 5.0,
                text: label.to_string(),
                font_size: 12.0,
                color: btn_colors[i],
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    // ========================================================================
    // Keyboard shortcut descriptions (for help/about)
    // ========================================================================

    /// Returns a list of keyboard shortcuts with descriptions.
    pub fn shortcuts_list() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y", "Redo"),
            ("Ctrl+C", "Copy selection"),
            ("Ctrl+V", "Paste"),
            ("Ctrl+X", "Cut selection"),
            ("Ctrl+S", "Save as BMP"),
            ("Ctrl+O", "Open BMP"),
            ("Ctrl+N", "New canvas"),
            ("Ctrl++", "Zoom in"),
            ("Ctrl+-", "Zoom out"),
            ("Ctrl+0", "Actual size"),
            ("Ctrl+F", "Fit to window"),
            ("B", "Pencil/Brush tool"),
            ("L", "Line tool"),
            ("R", "Rectangle tool"),
            ("O", "Ellipse tool"),
            ("U", "Rounded Rectangle"),
            ("P", "Polygon tool"),
            ("G", "Fill (bucket) tool"),
            ("E", "Eraser tool"),
            ("T", "Text tool"),
            ("I", "Eyedropper tool"),
            ("A", "Spray can tool"),
            ("S", "Selection tool"),
            ("X", "Swap FG/BG colors"),
            ("[", "Decrease brush size"),
            ("]", "Increase brush size"),
            ("H", "Flip horizontal"),
            ("V", "Flip vertical"),
            ("F5", "Toggle grid"),
            ("Enter", "Finish polygon / place text"),
            ("Escape", "Cancel / deselect"),
            ("Delete", "Clear selection"),
        ]
    }

    // ========================================================================
    // Full keyboard/mouse input dispatch (simplified)
    // ========================================================================

    /// Handles a key press event. Returns true if the event was consumed.
    pub fn handle_key_press(&mut self, key: char, ctrl: bool, shift: bool) -> bool {
        // Ctrl shortcuts
        if ctrl {
            match key {
                'z' | 'Z' => {
                    self.undo();
                    return true;
                }
                'y' | 'Y' => {
                    self.redo();
                    return true;
                }
                'c' | 'C' => {
                    self.copy_selection();
                    return true;
                }
                'x' | 'X' => {
                    self.push_history("cut");
                    self.cut_selection();
                    return true;
                }
                'v' | 'V' => {
                    self.paste();
                    return true;
                }
                'n' | 'N' => {
                    self.new_canvas(DEFAULT_CANVAS_WIDTH, DEFAULT_CANVAS_HEIGHT);
                    return true;
                }
                '+' | '=' => {
                    self.zoom_in();
                    return true;
                }
                '-' => {
                    self.zoom_out();
                    return true;
                }
                '0' => {
                    self.zoom_actual();
                    return true;
                }
                'f' | 'F' => {
                    self.zoom_fit();
                    return true;
                }
                _ => {}
            }
            return false;
        }

        // Shape mode toggle with shift
        if shift {
            match key {
                'M' | 'm' => {
                    self.shape_mode = match self.shape_mode {
                        ShapeMode::Outline => ShapeMode::Filled,
                        ShapeMode::Filled => ShapeMode::FilledWithOutline,
                        ShapeMode::FilledWithOutline => ShapeMode::Outline,
                    };
                    return true;
                }
                _ => {}
            }
        }

        // Tool shortcuts (no modifiers)
        let tool = match key {
            'b' | 'B' => Some(Tool::Pencil),
            'l' | 'L' => Some(Tool::Line),
            'r' | 'R' => Some(Tool::Rectangle),
            'o' | 'O' => Some(Tool::Ellipse),
            'p' | 'P' => Some(Tool::Polygon),
            'g' | 'G' => Some(Tool::Fill),
            'e' | 'E' => Some(Tool::Eraser),
            't' | 'T' => Some(Tool::Text),
            'i' | 'I' => Some(Tool::Eyedropper),
            'a' | 'A' => Some(Tool::SprayCan),
            'u' | 'U' => Some(Tool::RoundedRectangle),
            's' | 'S' => Some(Tool::Select),
            _ => None,
        };

        if let Some(t) = tool {
            self.current_tool = t;
            return true;
        }

        match key {
            'x' | 'X' => {
                self.swap_colors();
                true
            }
            '[' => {
                let new_size = self.brush.size.saturating_sub(1).max(1);
                self.brush.set_size(new_size);
                true
            }
            ']' => {
                let new_size = self.brush.size.saturating_add(1);
                self.brush.set_size(new_size);
                true
            }
            'h' | 'H' => {
                self.flip_horizontal();
                true
            }
            'v' | 'V' => {
                self.flip_vertical();
                true
            }
            _ => false,
        }
    }

    /// Handles special key presses (Enter, Escape, F-keys, etc.).
    pub fn handle_special_key(&mut self, key: SpecialKey) -> bool {
        match key {
            SpecialKey::Enter => {
                if self.current_tool == Tool::Polygon {
                    self.finish_polygon();
                    return true;
                }
                if self.current_tool == Tool::Text {
                    self.place_text();
                    return true;
                }
                false
            }
            SpecialKey::Escape => {
                if self.color_picker.is_open {
                    self.color_picker.close();
                    return true;
                }
                if self.polygon_builder.vertex_count() > 0 {
                    self.polygon_builder.reset();
                    return true;
                }
                if self.selection.is_some() {
                    self.apply_selection();
                    self.selection = None;
                    return true;
                }
                false
            }
            SpecialKey::Delete => {
                let sel_data = self.selection.as_ref().and_then(|sel| {
                    if sel.has_area() {
                        Some((sel.x, sel.y, sel.width, sel.height))
                    } else {
                        None
                    }
                });
                if let Some((sx, sy, sw, sh)) = sel_data {
                    self.push_history("delete selection");
                    if let Some(layer) = self.layers.get_mut(self.active_layer) {
                        for dy in 0..sh {
                            for dx in 0..sw {
                                let px = sx + dx as i32;
                                let py = sy + dy as i32;
                                if px >= 0 && py >= 0 {
                                    layer.pixels.set_pixel(
                                        px as u32,
                                        py as u32,
                                        Color::TRANSPARENT,
                                    );
                                }
                            }
                        }
                    }
                    self.selection = None;
                    return true;
                }
                false
            }
            SpecialKey::F5 => {
                self.grid.toggle();
                true
            }
            SpecialKey::Backspace => {
                if self.current_tool == Tool::Text {
                    self.text_input.backspace();
                    return true;
                }
                false
            }
        }
    }

    /// Handles a text character input (for the text tool).
    pub fn handle_text_char(&mut self, ch: char) -> bool {
        if self.current_tool == Tool::Text && self.drag.start_x != 0 {
            self.text_input.insert_char(ch);
            true
        } else {
            false
        }
    }

    /// Handles mouse scroll for zooming.
    pub fn handle_scroll(&mut self, delta_y: f32) {
        if delta_y > 0.0 {
            self.zoom_in();
        } else if delta_y < 0.0 {
            self.zoom_out();
        }
    }

    /// Creates a new blank canvas, discarding current content.
    pub fn new_canvas(&mut self, width: u32, height: u32) {
        self.push_history("new canvas");
        self.canvas_width = width;
        self.canvas_height = height;
        self.layers.clear();
        self.layers.push(Layer::with_background(
            "Background".to_string(),
            width,
            height,
            self.canvas_bg,
        ));
        self.active_layer = 0;
        self.selection = None;
        self.polygon_builder.reset();
        self.text_input.clear();
    }
}

// ============================================================================
// Special key enum (for keys that aren't simple characters)
// ============================================================================

/// Special (non-character) keys that the paint app handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialKey {
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Delete key.
    Delete,
    /// F5 key.
    F5,
    /// Backspace.
    Backspace,
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let app = PaintApp::new(1024.0, 768.0);

    // Render one frame to verify everything works
    let commands = app.render();

    // Basic output to confirm startup
    let _ = commands.len();

    // In a real OS environment, we would enter the event loop here:
    // loop {
    //     let event = wait_for_event();
    //     match event { ... }
    //     let cmds = app.render();
    //     submit_render_commands(cmds);
    // }

    // For now just confirm the app initializes properly
    let _flat = app.flatten();
    let _ = app.zoom_percent_str();
    let _ = PaintApp::shortcuts_list();

    // Ensure we do not quit immediately in a real environment
    let _ = app.should_quit;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    // ---- PixelBuffer tests ----

    #[test]
    fn test_pixel_buffer_new() {
        let buf = PixelBuffer::new(10, 10, Color::RED);
        assert_eq!(buf.width, 10);
        assert_eq!(buf.height, 10);
        assert_eq!(buf.data.len(), 400);
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::RED);
    }

    #[test]
    fn test_pixel_buffer_transparent() {
        let buf = PixelBuffer::transparent(5, 5);
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_pixel_buffer_set_get() {
        let mut buf = PixelBuffer::transparent(3, 3);
        buf.set_pixel(1, 1, Color::BLUE);
        assert_eq!(buf.get_pixel(1, 1).unwrap(), Color::BLUE);
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_pixel_buffer_out_of_bounds() {
        let buf = PixelBuffer::new(2, 2, Color::WHITE);
        assert!(buf.get_pixel(2, 0).is_none());
        assert!(buf.get_pixel(0, 2).is_none());
        assert!(buf.get_pixel(100, 100).is_none());
    }

    #[test]
    fn test_pixel_buffer_fill() {
        let mut buf = PixelBuffer::new(4, 4, Color::WHITE);
        buf.fill(Color::BLACK);
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::BLACK);
        assert_eq!(buf.get_pixel(3, 3).unwrap(), Color::BLACK);
    }

    #[test]
    fn test_pixel_buffer_copy_region() {
        let mut buf = PixelBuffer::new(10, 10, Color::WHITE);
        buf.set_pixel(2, 2, Color::RED);
        let region = buf.copy_region(1, 1, 4, 4);
        assert_eq!(region.width, 4);
        assert_eq!(region.height, 4);
        assert_eq!(region.get_pixel(1, 1).unwrap(), Color::RED);
    }

    #[test]
    fn test_pixel_buffer_paste() {
        let mut dest = PixelBuffer::transparent(10, 10);
        let src = PixelBuffer::new(3, 3, Color::GREEN);
        dest.paste(&src, 2, 2);
        assert_eq!(dest.get_pixel(2, 2).unwrap(), Color::GREEN);
        assert_eq!(dest.get_pixel(4, 4).unwrap(), Color::GREEN);
        assert_eq!(dest.get_pixel(5, 5).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_pixel_buffer_paste_overwrite() {
        let mut dest = PixelBuffer::new(10, 10, Color::RED);
        let src = PixelBuffer::new(3, 3, Color::BLUE);
        dest.paste_overwrite(&src, 0, 0);
        assert_eq!(dest.get_pixel(0, 0).unwrap(), Color::BLUE);
        assert_eq!(dest.get_pixel(3, 3).unwrap(), Color::RED);
    }

    #[test]
    fn test_pixel_buffer_flip_horizontal() {
        let mut buf = PixelBuffer::transparent(4, 1);
        buf.set_pixel(0, 0, Color::RED);
        buf.set_pixel(3, 0, Color::BLUE);
        buf.flip_horizontal();
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::BLUE);
        assert_eq!(buf.get_pixel(3, 0).unwrap(), Color::RED);
    }

    #[test]
    fn test_pixel_buffer_flip_vertical() {
        let mut buf = PixelBuffer::transparent(1, 4);
        buf.set_pixel(0, 0, Color::RED);
        buf.set_pixel(0, 3, Color::BLUE);
        buf.flip_vertical();
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::BLUE);
        assert_eq!(buf.get_pixel(0, 3).unwrap(), Color::RED);
    }

    #[test]
    fn test_pixel_buffer_rotate_90_cw() {
        let mut buf = PixelBuffer::transparent(3, 2);
        buf.set_pixel(0, 0, Color::RED);
        let rotated = buf.rotate_90_cw();
        assert_eq!(rotated.width, 2);
        assert_eq!(rotated.height, 3);
        assert_eq!(rotated.get_pixel(1, 0).unwrap(), Color::RED);
    }

    #[test]
    fn test_pixel_buffer_rotate_90_ccw() {
        let mut buf = PixelBuffer::transparent(3, 2);
        buf.set_pixel(2, 0, Color::BLUE);
        let rotated = buf.rotate_90_ccw();
        assert_eq!(rotated.width, 2);
        assert_eq!(rotated.height, 3);
        // 90° CCW sends the top-right corner (2,0) of the 3x2 source to the
        // top-left corner (0,0) of the 2x3 result.
        assert_eq!(rotated.get_pixel(0, 0).unwrap(), Color::BLUE);
    }

    #[test]
    fn test_pixel_buffer_rotate_180() {
        let mut buf = PixelBuffer::transparent(3, 3);
        buf.set_pixel(0, 0, Color::RED);
        let rotated = buf.rotate_180();
        assert_eq!(rotated.get_pixel(2, 2).unwrap(), Color::RED);
        assert_eq!(rotated.get_pixel(0, 0).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_pixel_buffer_resize_nearest() {
        let buf = PixelBuffer::new(4, 4, Color::RED);
        let resized = buf.resize_nearest(8, 8);
        assert_eq!(resized.width, 8);
        assert_eq!(resized.height, 8);
        assert_eq!(resized.get_pixel(0, 0).unwrap(), Color::RED);
        assert_eq!(resized.get_pixel(7, 7).unwrap(), Color::RED);
    }

    #[test]
    fn test_pixel_buffer_resize_zero() {
        let buf = PixelBuffer::new(4, 4, Color::RED);
        let resized = buf.resize_nearest(0, 0);
        assert_eq!(resized.width, 0);
        assert_eq!(resized.height, 0);
    }

    #[test]
    fn test_pixel_buffer_blend() {
        let mut buf = PixelBuffer::new(2, 2, Color::WHITE);
        let semi = Color::rgba(255, 0, 0, 128);
        buf.blend_pixel(0, 0, semi);
        let result = buf.get_pixel(0, 0).unwrap();
        // After blending red with 50% alpha over white, should be pinkish
        assert!(result.r > 128);
        assert!(result.g < 200);
    }

    // ---- Drawing primitives tests ----

    #[test]
    fn test_draw_line_horizontal() {
        let mut buf = PixelBuffer::transparent(10, 1);
        draw_line(&mut buf, 0, 0, 9, 0, Color::RED, 1);
        for x in 0..10 {
            assert_eq!(buf.get_pixel(x, 0).unwrap(), Color::RED);
        }
    }

    #[test]
    fn test_draw_line_vertical() {
        let mut buf = PixelBuffer::transparent(1, 10);
        draw_line(&mut buf, 0, 0, 0, 9, Color::BLUE, 1);
        for y in 0..10 {
            assert_eq!(buf.get_pixel(0, y).unwrap(), Color::BLUE);
        }
    }

    #[test]
    fn test_draw_line_diagonal() {
        let mut buf = PixelBuffer::transparent(10, 10);
        draw_line(&mut buf, 0, 0, 9, 9, Color::GREEN, 1);
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::GREEN);
        assert_eq!(buf.get_pixel(9, 9).unwrap(), Color::GREEN);
        assert_eq!(buf.get_pixel(5, 5).unwrap(), Color::GREEN);
    }

    #[test]
    fn test_draw_line_thick() {
        let mut buf = PixelBuffer::transparent(20, 20);
        draw_line(&mut buf, 5, 10, 15, 10, Color::RED, 4);
        // Center pixel should be set
        assert_eq!(buf.get_pixel(10, 10).unwrap(), Color::RED);
        // Pixels above/below should be set too (thickness 4 => radius 2)
        assert_eq!(buf.get_pixel(10, 9).unwrap(), Color::RED);
    }

    #[test]
    fn test_draw_rect_outline() {
        let mut buf = PixelBuffer::transparent(10, 10);
        draw_rect_outline(&mut buf, 1, 1, 8, 8, Color::RED, 1);
        // Top edge
        assert_eq!(buf.get_pixel(1, 1).unwrap(), Color::RED);
        assert_eq!(buf.get_pixel(8, 1).unwrap(), Color::RED);
        // Center should be transparent
        assert_eq!(buf.get_pixel(5, 5).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_draw_rect_filled() {
        let mut buf = PixelBuffer::transparent(10, 10);
        draw_rect_filled(&mut buf, 2, 2, 4, 4, Color::BLUE);
        assert_eq!(buf.get_pixel(2, 2).unwrap(), Color::BLUE);
        assert_eq!(buf.get_pixel(5, 5).unwrap(), Color::BLUE);
        assert_eq!(buf.get_pixel(6, 6).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_draw_ellipse_filled() {
        let mut buf = PixelBuffer::transparent(20, 20);
        draw_ellipse_filled(&mut buf, 10, 10, 5, 5, Color::RED);
        // Center should be filled
        assert_eq!(buf.get_pixel(10, 10).unwrap(), Color::RED);
        // Far corner should not
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_draw_ellipse_outline() {
        let mut buf = PixelBuffer::transparent(30, 30);
        draw_ellipse_outline(&mut buf, 15, 15, 10, 8, Color::GREEN, 1);
        // Center should not be filled for outline
        assert_eq!(buf.get_pixel(15, 15).unwrap(), Color::TRANSPARENT);
        // Top of ellipse should be colored
        assert_eq!(buf.get_pixel(15, 7).unwrap(), Color::GREEN);
    }

    #[test]
    fn test_draw_ellipse_zero_radius() {
        let mut buf = PixelBuffer::transparent(10, 10);
        draw_ellipse_filled(&mut buf, 5, 5, 0, 0, Color::RED);
        // Should not crash and center should remain transparent
        assert_eq!(buf.get_pixel(5, 5).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_draw_rounded_rect_outline() {
        let mut buf = PixelBuffer::transparent(30, 30);
        draw_rounded_rect_outline(&mut buf, 2, 2, 26, 26, 4, Color::RED, 1);
        // Should have pixels on the border
        assert_ne!(buf.get_pixel(15, 2).unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn test_draw_rounded_rect_filled() {
        let mut buf = PixelBuffer::transparent(30, 30);
        draw_rounded_rect_filled(&mut buf, 2, 2, 26, 26, 4, Color::BLUE);
        // Center should be filled
        assert_eq!(buf.get_pixel(15, 15).unwrap(), Color::BLUE);
    }

    #[test]
    fn test_flood_fill_basic() {
        let mut buf = PixelBuffer::new(5, 5, Color::WHITE);
        flood_fill(&mut buf, 0, 0, Color::RED);
        // All pixels should be red now
        for y in 0..5 {
            for x in 0..5 {
                assert_eq!(buf.get_pixel(x, y).unwrap(), Color::RED);
            }
        }
    }

    #[test]
    fn test_flood_fill_bounded() {
        let mut buf = PixelBuffer::new(5, 5, Color::WHITE);
        // Draw a border
        for i in 0..5 {
            buf.set_pixel(2, i, Color::BLACK);
        }
        flood_fill(&mut buf, 0, 0, Color::RED);
        // Left side should be red
        assert_eq!(buf.get_pixel(0, 0).unwrap(), Color::RED);
        assert_eq!(buf.get_pixel(1, 2).unwrap(), Color::RED);
        // Right side should still be white
        assert_eq!(buf.get_pixel(3, 0).unwrap(), Color::WHITE);
        // Border should still be black
        assert_eq!(buf.get_pixel(2, 0).unwrap(), Color::BLACK);
    }

    #[test]
    fn test_flood_fill_same_color() {
        let mut buf = PixelBuffer::new(3, 3, Color::RED);
        flood_fill(&mut buf, 1, 1, Color::RED);
        // Should not change anything (target = fill color)
        assert_eq!(buf.get_pixel(1, 1).unwrap(), Color::RED);
    }

    #[test]
    fn test_spray_paint() {
        let mut buf = PixelBuffer::transparent(20, 20);
        spray_paint(&mut buf, 10, 10, 5, Color::RED, 50, 42);
        // At least some pixels should be colored
        let mut colored = 0;
        for y in 0..20 {
            for x in 0..20 {
                if buf.get_pixel(x, y).unwrap() != Color::TRANSPARENT {
                    colored += 1;
                }
            }
        }
        assert!(colored > 0);
    }

    // ---- BMP tests ----

    #[test]
    fn test_bmp_encode_decode_roundtrip() {
        let mut buf = PixelBuffer::new(4, 4, Color::WHITE);
        buf.set_pixel(0, 0, Color::RED);
        buf.set_pixel(3, 3, Color::BLUE);

        let encoded = encode_bmp(&buf);
        let decoded = decode_bmp(&encoded).unwrap();

        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 4);
        assert_eq!(decoded.get_pixel(0, 0).unwrap(), Color::rgba(220, 50, 50, 255));
        assert_eq!(decoded.get_pixel(3, 3).unwrap(), Color::rgba(50, 100, 220, 255));
    }

    #[test]
    fn test_bmp_header() {
        let buf = PixelBuffer::new(2, 2, Color::BLACK);
        let encoded = encode_bmp(&buf);
        assert_eq!(encoded[0], b'B');
        assert_eq!(encoded[1], b'M');
        // 32bpp
        assert_eq!(encoded[28], 32);
    }

    #[test]
    fn test_bmp_decode_invalid() {
        assert!(decode_bmp(&[]).is_none());
        assert!(decode_bmp(&[0; 10]).is_none());
        assert!(decode_bmp(b"XX").is_none());
    }

    #[test]
    fn test_bmp_decode_too_short() {
        let mut data = vec![0u8; 54];
        data[0] = b'B';
        data[1] = b'M';
        // width = 0 should fail
        assert!(decode_bmp(&data).is_none());
    }

    // ---- Layer tests ----

    #[test]
    fn test_layer_new() {
        let layer = Layer::new("Test".to_string(), 100, 100);
        assert_eq!(layer.name, "Test");
        assert!(layer.visible);
        assert_eq!(layer.opacity, 1.0);
        assert_eq!(layer.pixels.width, 100);
    }

    #[test]
    fn test_layer_with_background() {
        let layer = Layer::with_background("BG".to_string(), 5, 5, Color::RED);
        assert_eq!(layer.pixels.get_pixel(0, 0).unwrap(), Color::RED);
    }

    #[test]
    fn test_layer_set_opacity() {
        let mut layer = Layer::new("L".to_string(), 1, 1);
        layer.set_opacity(0.5);
        assert_eq!(layer.opacity, 0.5);
        layer.set_opacity(-1.0);
        assert_eq!(layer.opacity, 0.0);
        layer.set_opacity(2.0);
        assert_eq!(layer.opacity, 1.0);
    }

    #[test]
    fn test_layer_toggle_visibility() {
        let mut layer = Layer::new("L".to_string(), 1, 1);
        assert!(layer.visible);
        layer.toggle_visibility();
        assert!(!layer.visible);
        layer.toggle_visibility();
        assert!(layer.visible);
    }

    // ---- Selection tests ----

    #[test]
    fn test_selection_new() {
        let sel = Selection::new(10, 20, 30, 40);
        assert_eq!(sel.rect(), (10, 20, 30, 40));
        assert!(sel.has_area());
    }

    #[test]
    fn test_selection_zero_area() {
        let sel = Selection::new(0, 0, 0, 0);
        assert!(!sel.has_area());
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection::new(10, 10, 20, 20);
        assert!(sel.contains(10, 10));
        assert!(sel.contains(15, 15));
        assert!(sel.contains(29, 29));
        assert!(!sel.contains(30, 30));
        assert!(!sel.contains(9, 10));
    }

    // ---- History tests ----

    #[test]
    fn test_history_push_and_undo() {
        let mut history = History::new(5);
        let snap1 = HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "action1".to_string(),
        };
        history.push(snap1);
        assert_eq!(history.undo_count(), 1);

        let current = HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "current".to_string(),
        };
        let prev = history.undo(current);
        assert!(prev.is_some());
        assert_eq!(history.undo_count(), 0);
        assert_eq!(history.redo_count(), 1);
    }

    #[test]
    fn test_history_redo() {
        let mut history = History::new(5);
        let snap1 = HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "a".to_string(),
        };
        history.push(snap1);

        let current = HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "b".to_string(),
        };
        let prev = history.undo(current).unwrap();

        let redone = history.redo(prev);
        assert!(redone.is_some());
        assert_eq!(history.redo_count(), 0);
        assert_eq!(history.undo_count(), 1);
    }

    #[test]
    fn test_history_max_steps() {
        let mut history = History::new(3);
        for i in 0..5 {
            history.push(HistorySnapshot {
                layers: vec![],
                active_layer: i,
                description: format!("step {}", i),
            });
        }
        assert_eq!(history.undo_count(), 3);
    }

    #[test]
    fn test_history_push_clears_redo() {
        let mut history = History::new(5);
        history.push(HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "a".to_string(),
        });
        let current = HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "b".to_string(),
        };
        history.undo(current);
        assert_eq!(history.redo_count(), 1);

        // Push new action should clear redo
        history.push(HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "c".to_string(),
        });
        assert_eq!(history.redo_count(), 0);
    }

    #[test]
    fn test_history_clear() {
        let mut history = History::new(5);
        history.push(HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "x".to_string(),
        });
        history.clear();
        assert_eq!(history.undo_count(), 0);
        assert_eq!(history.redo_count(), 0);
    }

    #[test]
    fn test_history_undo_empty() {
        let mut history = History::new(5);
        let current = HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "x".to_string(),
        };
        assert!(history.undo(current).is_none());
    }

    #[test]
    fn test_history_redo_empty() {
        let mut history = History::new(5);
        let current = HistorySnapshot {
            layers: vec![],
            active_layer: 0,
            description: "x".to_string(),
        };
        assert!(history.redo(current).is_none());
    }

    // ---- Polygon builder tests ----

    #[test]
    fn test_polygon_builder_new() {
        let pb = PolygonBuilder::new();
        assert_eq!(pb.vertex_count(), 0);
        assert!(!pb.closed);
    }

    #[test]
    fn test_polygon_builder_add_points() {
        let mut pb = PolygonBuilder::new();
        pb.add_point(0, 0);
        pb.add_point(10, 0);
        pb.add_point(5, 10);
        assert_eq!(pb.vertex_count(), 3);
    }

    #[test]
    fn test_polygon_builder_close() {
        let mut pb = PolygonBuilder::new();
        pb.add_point(0, 0);
        pb.add_point(10, 0);
        pb.add_point(5, 10);
        pb.close();
        assert!(pb.closed);
    }

    #[test]
    fn test_polygon_builder_draw_outline() {
        let mut pb = PolygonBuilder::new();
        pb.add_point(0, 0);
        pb.add_point(20, 0);
        pb.add_point(10, 20);
        pb.close();

        let mut buf = PixelBuffer::transparent(25, 25);
        pb.draw_outline(&mut buf, Color::RED, 1);
        // Top edge should have pixels
        assert_eq!(buf.get_pixel(10, 0).unwrap(), Color::RED);
    }

    #[test]
    fn test_polygon_builder_draw_filled() {
        let mut pb = PolygonBuilder::new();
        pb.add_point(0, 0);
        pb.add_point(20, 0);
        pb.add_point(20, 20);
        pb.add_point(0, 20);
        pb.close();

        let mut buf = PixelBuffer::transparent(25, 25);
        pb.draw_filled(&mut buf, Color::BLUE);
        // Center should be filled
        assert_eq!(buf.get_pixel(10, 10).unwrap(), Color::BLUE);
    }

    #[test]
    fn test_polygon_builder_reset() {
        let mut pb = PolygonBuilder::new();
        pb.add_point(1, 2);
        pb.close();
        pb.reset();
        assert_eq!(pb.vertex_count(), 0);
        assert!(!pb.closed);
    }

    #[test]
    fn test_polygon_builder_too_few_points_outline() {
        let mut pb = PolygonBuilder::new();
        pb.add_point(0, 0);
        let mut buf = PixelBuffer::transparent(10, 10);
        pb.draw_outline(&mut buf, Color::RED, 1);
        // Should not crash, no pixels drawn
    }

    #[test]
    fn test_polygon_builder_too_few_points_filled() {
        let mut pb = PolygonBuilder::new();
        pb.add_point(0, 0);
        pb.add_point(5, 5);
        let mut buf = PixelBuffer::transparent(10, 10);
        pb.draw_filled(&mut buf, Color::RED);
        // Should not crash (fewer than 3 points)
    }

    // ---- TextInput tests ----

    #[test]
    fn test_text_input_new() {
        let ti = TextInput::new();
        assert_eq!(ti.as_str(), "");
        assert_eq!(ti.cursor, 0);
    }

    #[test]
    fn test_text_input_with_text() {
        let ti = TextInput::with_text("hello");
        assert_eq!(ti.as_str(), "hello");
        assert_eq!(ti.cursor, 5);
    }

    #[test]
    fn test_text_input_insert() {
        let mut ti = TextInput::new();
        ti.insert_char('A');
        ti.insert_char('B');
        assert_eq!(ti.as_str(), "AB");
        assert_eq!(ti.cursor, 2);
    }

    #[test]
    fn test_text_input_backspace() {
        let mut ti = TextInput::with_text("ABC");
        ti.backspace();
        assert_eq!(ti.as_str(), "AB");
        ti.backspace();
        assert_eq!(ti.as_str(), "A");
    }

    #[test]
    fn test_text_input_backspace_empty() {
        let mut ti = TextInput::new();
        ti.backspace();
        assert_eq!(ti.as_str(), "");
    }

    #[test]
    fn test_text_input_delete_forward() {
        let mut ti = TextInput::with_text("ABC");
        ti.cursor = 0;
        ti.delete_forward();
        assert_eq!(ti.as_str(), "BC");
    }

    #[test]
    fn test_text_input_move_left_right() {
        let mut ti = TextInput::with_text("AB");
        assert_eq!(ti.cursor, 2);
        ti.move_left();
        assert_eq!(ti.cursor, 1);
        ti.move_left();
        assert_eq!(ti.cursor, 0);
        ti.move_left(); // Should stay at 0
        assert_eq!(ti.cursor, 0);
        ti.move_right();
        assert_eq!(ti.cursor, 1);
    }

    #[test]
    fn test_text_input_clear() {
        let mut ti = TextInput::with_text("hello");
        ti.clear();
        assert_eq!(ti.as_str(), "");
        assert_eq!(ti.cursor, 0);
    }

    // ---- ColorPicker tests ----

    #[test]
    fn test_color_picker_new() {
        let cp = ColorPicker::new(Color::rgb(128, 64, 32));
        assert_eq!(cp.red, 128);
        assert_eq!(cp.green, 64);
        assert_eq!(cp.blue, 32);
        assert_eq!(cp.color(), Color::rgb(128, 64, 32));
    }

    #[test]
    fn test_color_picker_set_color() {
        let mut cp = ColorPicker::new(Color::BLACK);
        cp.set_color(Color::rgb(255, 128, 0));
        assert_eq!(cp.red, 255);
        assert_eq!(cp.green, 128);
        assert_eq!(cp.blue, 0);
    }

    #[test]
    fn test_color_picker_apply_hex_valid() {
        let mut cp = ColorPicker::new(Color::BLACK);
        cp.hex_input = TextInput::with_text("FF8000");
        assert!(cp.apply_hex_input());
        assert_eq!(cp.red, 255);
        assert_eq!(cp.green, 128);
        assert_eq!(cp.blue, 0);
    }

    #[test]
    fn test_color_picker_apply_hex_with_hash() {
        let mut cp = ColorPicker::new(Color::BLACK);
        cp.hex_input = TextInput::with_text("#00FF00");
        assert!(cp.apply_hex_input());
        assert_eq!(cp.red, 0);
        assert_eq!(cp.green, 255);
        assert_eq!(cp.blue, 0);
    }

    #[test]
    fn test_color_picker_apply_hex_invalid() {
        let mut cp = ColorPicker::new(Color::BLACK);
        cp.hex_input = TextInput::with_text("XYZ");
        assert!(!cp.apply_hex_input());
    }

    #[test]
    fn test_color_picker_open_close() {
        let mut cp = ColorPicker::new(Color::BLACK);
        assert!(!cp.is_open);
        cp.open_for(true, Color::RED);
        assert!(cp.is_open);
        assert!(cp.editing_foreground);
        cp.close();
        assert!(!cp.is_open);
    }

    // ---- GridSettings tests ----

    #[test]
    fn test_grid_settings_new() {
        let grid = GridSettings::new();
        assert!(!grid.visible);
        assert_eq!(grid.spacing, 16);
    }

    #[test]
    fn test_grid_toggle() {
        let mut grid = GridSettings::new();
        grid.toggle();
        assert!(grid.visible);
        grid.toggle();
        assert!(!grid.visible);
    }

    #[test]
    fn test_grid_set_spacing() {
        let mut grid = GridSettings::new();
        grid.set_spacing(32);
        assert_eq!(grid.spacing, 32);
        grid.set_spacing(1);
        assert_eq!(grid.spacing, 2); // minimum is 2
    }

    // ---- Clipboard tests ----

    #[test]
    fn test_clipboard_empty() {
        let cb = Clipboard::new();
        assert!(!cb.has_content());
        assert!(cb.get().is_none());
    }

    #[test]
    fn test_clipboard_store_and_get() {
        let mut cb = Clipboard::new();
        cb.store(PixelBuffer::new(5, 5, Color::RED));
        assert!(cb.has_content());
        let content = cb.get().unwrap();
        assert_eq!(content.width, 5);
    }

    #[test]
    fn test_clipboard_clear() {
        let mut cb = Clipboard::new();
        cb.store(PixelBuffer::new(3, 3, Color::BLACK));
        cb.clear();
        assert!(!cb.has_content());
    }

    // ---- DragState tests ----

    #[test]
    fn test_drag_state_new() {
        let ds = DragState::new();
        assert!(!ds.active);
    }

    #[test]
    fn test_drag_state_begin_end() {
        let mut ds = DragState::new();
        ds.begin(10, 20);
        assert!(ds.active);
        assert_eq!(ds.start_x, 10);
        assert_eq!(ds.start_y, 20);
        ds.end();
        assert!(!ds.active);
    }

    #[test]
    fn test_drag_state_rect() {
        let mut ds = DragState::new();
        ds.begin(10, 10);
        ds.update(20, 30);
        let (x, y, w, h) = ds.rect();
        assert_eq!(x, 10);
        assert_eq!(y, 10);
        assert_eq!(w, 10);
        assert_eq!(h, 20);
    }

    #[test]
    fn test_drag_state_rect_reverse() {
        let mut ds = DragState::new();
        ds.begin(20, 30);
        ds.update(10, 10);
        let (x, y, w, h) = ds.rect();
        assert_eq!(x, 10);
        assert_eq!(y, 10);
        assert_eq!(w, 10);
        assert_eq!(h, 20);
    }

    #[test]
    fn test_drag_state_line() {
        let mut ds = DragState::new();
        ds.begin(5, 5);
        ds.update(15, 25);
        assert_eq!(ds.line(), (5, 5, 15, 25));
    }

    // ---- BrushSettings tests ----

    #[test]
    fn test_brush_settings_default() {
        let brush = BrushSettings::new();
        assert_eq!(brush.size, 3);
        assert_eq!(brush.opacity, 1.0);
        assert_eq!(brush.hardness, 1.0);
    }

    #[test]
    fn test_brush_set_size_clamp() {
        let mut brush = BrushSettings::new();
        brush.set_size(0);
        assert_eq!(brush.size, 1);
        brush.set_size(200);
        assert_eq!(brush.size, MAX_BRUSH_SIZE);
        brush.set_size(50);
        assert_eq!(brush.size, 50);
    }

    #[test]
    fn test_brush_set_opacity_clamp() {
        let mut brush = BrushSettings::new();
        brush.set_opacity(-0.5);
        assert_eq!(brush.opacity, 0.0);
        brush.set_opacity(1.5);
        assert_eq!(brush.opacity, 1.0);
        brush.set_opacity(0.5);
        assert_eq!(brush.opacity, 0.5);
    }

    #[test]
    fn test_brush_set_hardness_clamp() {
        let mut brush = BrushSettings::new();
        brush.set_hardness(-1.0);
        assert_eq!(brush.hardness, 0.0);
        brush.set_hardness(2.0);
        assert_eq!(brush.hardness, 1.0);
    }

    #[test]
    fn test_brush_apply_opacity() {
        let brush = BrushSettings {
            size: 1,
            opacity: 0.5,
            hardness: 1.0,
        };
        let c = Color::rgb(255, 0, 0);
        let result = brush.apply_opacity(c);
        assert_eq!(result.r, 255);
        assert_eq!(result.a, 127); // 255 * 0.5 ≈ 127
    }

    // ---- PaintApp tests ----

    #[test]
    fn test_paint_app_new() {
        let app = PaintApp::new(1024.0, 768.0);
        assert_eq!(app.canvas_width, DEFAULT_CANVAS_WIDTH);
        assert_eq!(app.canvas_height, DEFAULT_CANVAS_HEIGHT);
        assert_eq!(app.layers.len(), 1);
        assert_eq!(app.current_tool, Tool::Pencil);
        assert_eq!(app.zoom, 1.0);
    }

    #[test]
    fn test_paint_app_add_layer() {
        let mut app = PaintApp::new(100.0, 100.0);
        assert_eq!(app.layers.len(), 1);
        app.add_layer();
        assert_eq!(app.layers.len(), 2);
        assert_eq!(app.active_layer, 1);
    }

    #[test]
    fn test_paint_app_delete_layer() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_layer();
        assert!(app.delete_layer());
        assert_eq!(app.layers.len(), 1);
    }

    #[test]
    fn test_paint_app_delete_last_layer() {
        let mut app = PaintApp::new(100.0, 100.0);
        assert!(!app.delete_layer()); // Can't delete the only layer
        assert_eq!(app.layers.len(), 1);
    }

    #[test]
    fn test_paint_app_move_layer_up() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_layer();
        app.active_layer = 0;
        assert!(app.move_layer_up());
        assert_eq!(app.active_layer, 1);
    }

    #[test]
    fn test_paint_app_move_layer_down() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_layer();
        assert!(app.move_layer_down());
        assert_eq!(app.active_layer, 0);
    }

    #[test]
    fn test_paint_app_move_layer_up_at_top() {
        let mut app = PaintApp::new(100.0, 100.0);
        assert!(!app.move_layer_up()); // Only one layer, can't move up
    }

    #[test]
    fn test_paint_app_move_layer_down_at_bottom() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_layer();
        app.active_layer = 0;
        assert!(!app.move_layer_down());
    }

    #[test]
    fn test_paint_app_merge_layer_down() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_layer();
        app.layers[1].pixels.set_pixel(5, 5, Color::RED);
        assert!(app.merge_layer_down());
        assert_eq!(app.layers.len(), 1);
        // Merged pixel should be present
        assert_eq!(app.layers[0].pixels.get_pixel(5, 5).unwrap(), Color::RED);
    }

    #[test]
    fn test_paint_app_merge_at_bottom() {
        let mut app = PaintApp::new(100.0, 100.0);
        assert!(!app.merge_layer_down()); // Can't merge bottom layer
    }

    #[test]
    fn test_paint_app_flatten() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(0, 0, Color::RED);
        let flat = app.flatten();
        assert_eq!(flat.get_pixel(0, 0).unwrap(), Color::RED);
    }

    #[test]
    fn test_paint_app_flatten_invisible_layer() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_layer();
        app.layers[1].pixels.set_pixel(0, 0, Color::RED);
        app.layers[1].visible = false;
        let flat = app.flatten();
        // Should see the background, not the invisible layer's red pixel
        assert_ne!(flat.get_pixel(0, 0).unwrap(), Color::RED);
    }

    #[test]
    fn test_paint_app_zoom_in_out() {
        let mut app = PaintApp::new(1024.0, 768.0);
        assert_eq!(app.zoom, 1.0);
        app.zoom_in();
        assert!(app.zoom > 1.0);
        app.zoom_out();
        app.zoom_out();
        assert!(app.zoom < 1.0);
    }

    #[test]
    fn test_paint_app_zoom_clamp() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.set_zoom(0.01);
        assert_eq!(app.zoom, MIN_ZOOM);
        app.set_zoom(100.0);
        assert_eq!(app.zoom, MAX_ZOOM);
    }

    #[test]
    fn test_paint_app_zoom_actual() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.set_zoom(3.0);
        app.zoom_actual();
        assert_eq!(app.zoom, 1.0);
    }

    #[test]
    fn test_paint_app_zoom_percent_str() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.set_zoom(2.0);
        assert_eq!(app.zoom_percent_str(), "200%");
    }

    #[test]
    fn test_paint_app_swap_colors() {
        let mut app = PaintApp::new(100.0, 100.0);
        let fg = app.fg_color;
        let bg = app.bg_color;
        app.swap_colors();
        assert_eq!(app.fg_color, bg);
        assert_eq!(app.bg_color, fg);
    }

    #[test]
    fn test_paint_app_add_recent_color() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_recent_color(Color::RED);
        app.add_recent_color(Color::BLUE);
        assert_eq!(app.recent_colors.len(), 2);
        assert_eq!(*app.recent_colors.front().unwrap(), Color::BLUE);
    }

    #[test]
    fn test_paint_app_recent_color_no_dupes() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.add_recent_color(Color::RED);
        app.add_recent_color(Color::RED);
        assert_eq!(app.recent_colors.len(), 1);
    }

    #[test]
    fn test_paint_app_recent_color_max() {
        let mut app = PaintApp::new(100.0, 100.0);
        for i in 0..20 {
            app.add_recent_color(Color::rgb(i as u8, 0, 0));
        }
        assert_eq!(app.recent_colors.len(), MAX_RECENT_COLORS);
    }

    #[test]
    fn test_paint_app_undo_redo() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.push_history("initial");
        app.layers[0].pixels.set_pixel(0, 0, Color::RED);
        assert!(app.undo());
        assert_eq!(app.layers[0].pixels.get_pixel(0, 0).unwrap(), Color::WHITE);
        assert!(app.redo());
    }

    #[test]
    fn test_paint_app_window_to_canvas() {
        let app = PaintApp::new(1024.0, 768.0);
        let (cx, cy) = app.window_to_canvas(TOOLBAR_WIDTH, OPTION_BAR_HEIGHT);
        assert_eq!(cx, 0);
        assert_eq!(cy, 0);
    }

    #[test]
    fn test_paint_app_canvas_to_window() {
        let app = PaintApp::new(1024.0, 768.0);
        let (wx, wy) = app.canvas_to_window(0.0, 0.0);
        assert_eq!(wx, TOOLBAR_WIDTH);
        assert_eq!(wy, OPTION_BAR_HEIGHT);
    }

    #[test]
    fn test_paint_app_render_produces_commands() {
        let app = PaintApp::new(1024.0, 768.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_paint_app_render_with_selection() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.selection = Some(Selection::new(10, 10, 50, 50));
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_paint_app_render_with_grid() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.grid.visible = true;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_paint_app_render_with_color_picker() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.color_picker.is_open = true;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_paint_app_handle_key_tool_select() {
        let mut app = PaintApp::new(100.0, 100.0);
        assert!(app.handle_key_press('l', false, false));
        assert_eq!(app.current_tool, Tool::Line);
        assert!(app.handle_key_press('e', false, false));
        assert_eq!(app.current_tool, Tool::Eraser);
    }

    #[test]
    fn test_paint_app_handle_key_undo_redo() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.push_history("test");
        assert!(app.handle_key_press('z', true, false));
    }

    #[test]
    fn test_paint_app_handle_key_brush_size() {
        let mut app = PaintApp::new(100.0, 100.0);
        let original = app.brush.size;
        app.handle_key_press(']', false, false);
        assert_eq!(app.brush.size, original + 1);
        app.handle_key_press('[', false, false);
        assert_eq!(app.brush.size, original);
    }

    #[test]
    fn test_paint_app_handle_special_key_grid() {
        let mut app = PaintApp::new(100.0, 100.0);
        assert!(!app.grid.visible);
        app.handle_special_key(SpecialKey::F5);
        assert!(app.grid.visible);
    }

    #[test]
    fn test_paint_app_handle_special_key_escape() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.selection = Some(Selection::new(0, 0, 10, 10));
        assert!(app.handle_special_key(SpecialKey::Escape));
        assert!(app.selection.is_none());
    }

    #[test]
    fn test_paint_app_handle_scroll_zoom() {
        let mut app = PaintApp::new(100.0, 100.0);
        let z = app.zoom;
        app.handle_scroll(1.0);
        assert!(app.zoom > z);
    }

    #[test]
    fn test_paint_app_new_canvas() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.new_canvas(200, 150);
        assert_eq!(app.canvas_width, 200);
        assert_eq!(app.canvas_height, 150);
        assert_eq!(app.layers.len(), 1);
    }

    #[test]
    fn test_paint_app_flip_horizontal() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(0, 0, Color::RED);
        app.flip_horizontal();
        let w = app.canvas_width;
        assert_eq!(
            app.layers[0].pixels.get_pixel(w - 1, 0).unwrap(),
            Color::RED
        );
    }

    #[test]
    fn test_paint_app_flip_vertical() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(0, 0, Color::BLUE);
        app.flip_vertical();
        let h = app.canvas_height;
        assert_eq!(
            app.layers[0].pixels.get_pixel(0, h - 1).unwrap(),
            Color::BLUE
        );
    }

    #[test]
    fn test_paint_app_resize_canvas() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.resize_canvas(400, 300);
        assert_eq!(app.canvas_width, 400);
        assert_eq!(app.canvas_height, 300);
        assert_eq!(app.layers[0].pixels.width, 400);
        assert_eq!(app.layers[0].pixels.height, 300);
    }

    #[test]
    fn test_paint_app_resize_canvas_zero() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.resize_canvas(0, 0);
        // Should not change
        assert_eq!(app.canvas_width, DEFAULT_CANVAS_WIDTH);
    }

    #[test]
    fn test_paint_app_on_canvas_press_pencil() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Pencil;
        app.on_canvas_press(5, 5);
        assert!(app.mouse_down);
    }

    #[test]
    fn test_paint_app_on_canvas_press_fill() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Fill;
        app.fg_color = Color::RED;
        app.on_canvas_press(5, 5);
        assert_eq!(
            app.layers[0].pixels.get_pixel(5, 5).unwrap(),
            Color::RED,
        );
    }

    #[test]
    fn test_paint_app_on_canvas_press_eyedropper() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(10, 10, Color::rgb(100, 200, 50));
        app.current_tool = Tool::Eyedropper;
        app.on_canvas_press(10, 10);
        assert_eq!(app.fg_color, Color::rgb(100, 200, 50));
    }

    #[test]
    fn test_paint_app_finish_polygon() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Polygon;
        app.polygon_builder.add_point(10, 10);
        app.polygon_builder.add_point(50, 10);
        app.polygon_builder.add_point(30, 50);
        app.finish_polygon();
        assert_eq!(app.polygon_builder.vertex_count(), 0);
    }

    #[test]
    fn test_paint_app_finish_polygon_too_few() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.polygon_builder.add_point(0, 0);
        app.finish_polygon();
        assert_eq!(app.polygon_builder.vertex_count(), 0);
    }

    // ---- Tool tests ----

    #[test]
    fn test_tool_label() {
        assert_eq!(Tool::Pencil.label(), "Pen");
        assert_eq!(Tool::Line.label(), "Line");
        assert_eq!(Tool::Select.label(), "Sel");
    }

    #[test]
    fn test_tool_shortcut() {
        assert_eq!(Tool::Pencil.shortcut(), Some('B'));
        assert_eq!(Tool::Line.shortcut(), Some('L'));
    }

    #[test]
    fn test_tool_all() {
        let all = Tool::all();
        assert_eq!(all.len(), 12);
        assert!(all.contains(&Tool::Pencil));
        assert!(all.contains(&Tool::Select));
    }

    // ---- Default palette test ----

    #[test]
    fn test_default_palette() {
        let palette = default_palette();
        assert_eq!(palette.len(), 48);
    }

    // ---- Shortcuts test ----

    #[test]
    fn test_shortcuts_list() {
        let shortcuts = PaintApp::shortcuts_list();
        assert!(!shortcuts.is_empty());
        assert!(shortcuts.iter().any(|(k, _)| *k == "Ctrl+Z"));
    }

    // ---- Canvas coordinate conversion round-trip ----

    #[test]
    fn test_coordinate_roundtrip() {
        let app = PaintApp::new(1024.0, 768.0);
        let (wx, wy) = app.canvas_to_window(50.0, 30.0);
        let (cx, cy) = app.window_to_canvas(wx, wy);
        assert_eq!(cx, 50);
        assert_eq!(cy, 30);
    }

    #[test]
    fn test_coordinate_with_zoom() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.set_zoom(2.0);
        let (wx, wy) = app.canvas_to_window(10.0, 10.0);
        let (cx, cy) = app.window_to_canvas(wx, wy);
        assert_eq!(cx, 10);
        assert_eq!(cy, 10);
    }

    // ---- Shape mode test ----

    #[test]
    fn test_shape_mode_cycle() {
        let mut app = PaintApp::new(100.0, 100.0);
        assert_eq!(app.shape_mode, ShapeMode::Outline);
        app.handle_key_press('M', false, true);
        assert_eq!(app.shape_mode, ShapeMode::Filled);
        app.handle_key_press('M', false, true);
        assert_eq!(app.shape_mode, ShapeMode::FilledWithOutline);
        app.handle_key_press('M', false, true);
        assert_eq!(app.shape_mode, ShapeMode::Outline);
    }

    // ---- Copy/paste test ----

    #[test]
    fn test_copy_paste_selection() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(5, 5, Color::RED);
        app.selection = Some(Selection::new(5, 5, 2, 2));
        app.copy_selection();
        assert!(app.clipboard.has_content());
    }

    #[test]
    fn test_cut_selection() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(5, 5, Color::RED);
        app.selection = Some(Selection::new(5, 5, 2, 2));
        app.push_history("cut");
        app.cut_selection();
        assert!(app.clipboard.has_content());
        assert_eq!(
            app.layers[0].pixels.get_pixel(5, 5).unwrap(),
            Color::TRANSPARENT
        );
    }

    #[test]
    fn test_paste_creates_selection() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.clipboard.store(PixelBuffer::new(10, 10, Color::BLUE));
        app.paste();
        assert!(app.selection.is_some());
        let sel = app.selection.as_ref().unwrap();
        assert!(sel.content.is_some());
    }

    // ---- Canvas viewport test ----

    #[test]
    fn test_canvas_viewport() {
        let app = PaintApp::new(1024.0, 768.0);
        let (x, y, w, h) = app.canvas_viewport();
        assert_eq!(x, TOOLBAR_WIDTH);
        assert_eq!(y, OPTION_BAR_HEIGHT);
        assert!(w > 0.0);
        assert!(h > 0.0);
    }

    // ---- Drag preview rendering test ----

    #[test]
    fn test_render_with_active_drag() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.current_tool = Tool::Rectangle;
        app.drag.begin(10, 10);
        app.drag.update(50, 50);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // ---- Polygon preview rendering test ----

    #[test]
    fn test_render_with_polygon_preview() {
        let mut app = PaintApp::new(1024.0, 768.0);
        app.polygon_builder.add_point(10, 10);
        app.polygon_builder.add_point(50, 10);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // ---- Text input tool test ----

    #[test]
    fn test_text_tool_input() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Text;
        app.drag.start_x = 10;
        app.drag.start_y = 10;
        assert!(app.handle_text_char('H'));
        assert!(app.handle_text_char('i'));
        assert_eq!(app.text_input.as_str(), "Hi");
    }

    #[test]
    fn test_place_text() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Text;
        app.drag.start_x = 5;
        app.drag.start_y = 5;
        app.text_input.insert_char('A');
        app.place_text();
        assert_eq!(app.text_input.as_str(), "");
    }

    #[test]
    fn test_place_text_empty() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Text;
        let undo_before = app.history.undo_count();
        app.place_text();
        // Should not push history for empty text
        assert_eq!(app.history.undo_count(), undo_before);
    }

    // ---- Crop to selection test ----

    #[test]
    fn test_crop_to_selection() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.selection = Some(Selection::new(10, 10, 50, 50));
        app.crop_to_selection();
        assert_eq!(app.canvas_width, 50);
        assert_eq!(app.canvas_height, 50);
        assert!(app.selection.is_none());
    }

    // ---- Rotate tests on app ----

    #[test]
    fn test_paint_app_rotate_90_cw() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(0, 0, Color::RED);
        app.rotate_90_cw();
        // After 90 CW rotation of an 800x600 buffer:
        // (0,0) -> (height-1, 0) in the new buffer
        let new_w = app.layers[0].pixels.width;
        assert_eq!(
            app.layers[0].pixels.get_pixel(new_w - 1, 0).unwrap(),
            Color::RED
        );
    }

    #[test]
    fn test_paint_app_rotate_180() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.layers[0].pixels.set_pixel(0, 0, Color::BLUE);
        app.rotate_180();
        let w = app.layers[0].pixels.width;
        let h = app.layers[0].pixels.height;
        assert_eq!(
            app.layers[0].pixels.get_pixel(w - 1, h - 1).unwrap(),
            Color::BLUE
        );
    }

    // ---- Drawing color tests ----

    #[test]
    fn test_drawing_color_with_opacity() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.fg_color = Color::rgb(255, 0, 0);
        app.brush.set_opacity(0.5);
        let dc = app.drawing_color();
        assert_eq!(dc.r, 255);
        assert_eq!(dc.a, 127);
    }

    #[test]
    fn test_eraser_color() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.bg_color = Color::rgb(0, 128, 255);
        assert_eq!(app.eraser_color(), Color::rgb(0, 128, 255));
    }

    // ---- On canvas release for shape tools ----

    #[test]
    fn test_on_canvas_release_line() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Line;
        app.on_canvas_press(5, 5);
        app.on_canvas_release(50, 5);
        // Line should be drawn from (5,5) to (50,5)
        assert_eq!(
            app.layers[0].pixels.get_pixel(25, 5).unwrap(),
            Color::BLACK
        );
    }

    #[test]
    fn test_on_canvas_release_rectangle() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Rectangle;
        app.shape_mode = ShapeMode::Filled;
        app.on_canvas_press(10, 10);
        app.on_canvas_release(30, 30);
        // Center of rectangle should be filled
        assert_eq!(
            app.layers[0].pixels.get_pixel(20, 20).unwrap(),
            Color::BLACK
        );
    }

    #[test]
    fn test_on_canvas_release_ellipse() {
        let mut app = PaintApp::new(100.0, 100.0);
        app.current_tool = Tool::Ellipse;
        app.shape_mode = ShapeMode::Filled;
        app.on_canvas_press(10, 10);
        app.on_canvas_release(50, 40);
        // Center should be filled
        assert_ne!(
            app.layers[0].pixels.get_pixel(30, 25).unwrap(),
            Color::WHITE,
        );
    }
}
