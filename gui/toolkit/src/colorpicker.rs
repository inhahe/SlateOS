#![allow(dead_code)]
//! Color picker widget with HSV wheel, RGB/HSV sliders, hex input, alpha,
//! preset palette, eyedropper mode, and recent-color history.
//!
//! Provides two flavours:
//! - [`ColorPicker`] — compact inline widget (SV square + hue bar + preview)
//! - [`ColorPickerDialog`] — full dialog with all panels (sliders, hex, presets)
//!
//! # Architecture
//!
//! The picker stores color in HSV internally (lossless hue preservation during
//! saturation/value edits) and lazily converts to RGB/hex on demand. The alpha
//! channel is stored separately as it is orthogonal to hue/saturation/value.
//!
//! All rendering produces `Vec<RenderCommand>` that any backend can consume.

use crate::color::Color;
use crate::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::render::{FontWeightHint, RenderCommand};
use crate::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette (UI chrome)
// ============================================================================

/// Base background.
const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
/// Raised surface.
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
/// Higher surface (input fields, selected areas).
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
/// Overlay / hover highlight.
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
/// Primary text.
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Subdued text.
const COLOR_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
/// Accent color.
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
/// Muted / disabled.
const COLOR_OVERLAY: Color = Color::from_hex(0x6C7086);
/// Error / cancel.
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
/// Teal accent (used for eyedropper highlight).
const COLOR_TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

const PADDING: f32 = 8.0;
const FONT_SIZE: f32 = 12.0;
const FONT_SIZE_SMALL: f32 = 10.0;
const CORNER_RADIUS: f32 = 4.0;
const SLIDER_HEIGHT: f32 = 16.0;
const SLIDER_TRACK_HEIGHT: f32 = 8.0;
const SWATCH_SIZE: f32 = 20.0;
const SWATCH_GAP: f32 = 4.0;
const PREVIEW_SIZE: f32 = 48.0;
const HUE_BAR_WIDTH: f32 = 20.0;
const SV_SQUARE_SIZE: f32 = 180.0;
const ALPHA_BAR_HEIGHT: f32 = 16.0;
const MAX_RECENT_COLORS: usize = 16;

// ============================================================================
// HSV ↔ RGB ↔ Hex conversion
// ============================================================================

/// HSV color representation.
/// - `h`: hue in degrees [0, 360)
/// - `s`: saturation [0.0, 1.0]
/// - `v`: value/brightness [0.0, 1.0]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub const fn new(h: f32, s: f32, v: f32) -> Self {
        Self { h, s, v }
    }
}

/// Convert HSV to RGB.
///
/// Standard algorithm: divide the hue circle into 6 sectors and linearly
/// interpolate between primary/secondary colors.
pub fn hsv_to_rgb(hsv: Hsv) -> (u8, u8, u8) {
    let h = hsv.h.rem_euclid(360.0);
    let s = hsv.s.clamp(0.0, 1.0);
    let v = hsv.v.clamp(0.0, 1.0);

    let c = v * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - (h_prime.rem_euclid(2.0) - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let r = ((r1 + m) * 255.0 + 0.5) as u8;
    let g = ((g1 + m) * 255.0 + 0.5) as u8;
    let b = ((b1 + m) * 255.0 + 0.5) as u8;
    (r, g, b)
}

/// Convert RGB to HSV.
///
/// Returns hue in [0, 360), saturation and value in [0.0, 1.0].
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> Hsv {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let c_max = rf.max(gf).max(bf);
    let c_min = rf.min(gf).min(bf);
    let delta = c_max - c_min;

    let h = if delta < f32::EPSILON {
        0.0
    } else if (c_max - rf).abs() < f32::EPSILON {
        60.0 * (((gf - bf) / delta).rem_euclid(6.0))
    } else if (c_max - gf).abs() < f32::EPSILON {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };

    let s = if c_max < f32::EPSILON { 0.0 } else { delta / c_max };
    let v = c_max;

    Hsv { h, s, v }
}

/// Convert a `Color` to its 6-digit hex string (without `#` prefix).
pub fn color_to_hex_string(color: Color) -> String {
    format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// Convert a `Color` to its 8-digit hex string with alpha (without `#` prefix).
pub fn color_to_hex_string_alpha(color: Color) -> String {
    format!("{:02X}{:02X}{:02X}{:02X}", color.r, color.g, color.b, color.a)
}

/// Parse a hex color string (with or without `#`, 6 or 8 hex digits).
/// Returns `None` if the input is invalid.
pub fn parse_hex_color(input: &str) -> Option<Color> {
    let s = input.trim().strip_prefix('#').unwrap_or(input.trim());
    match s.len() {
        6 => {
            let val = u32::from_str_radix(s, 16).ok()?;
            Some(Color::from_hex(val))
        }
        8 => {
            let val = u32::from_str_radix(&s[..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            let base = Color::from_hex(val);
            Some(Color::rgba(base.r, base.g, base.b, a))
        }
        // Support 3-digit shorthand (#RGB → #RRGGBB)
        3 => {
            let chars: Vec<char> = s.chars().collect();
            let r = hex_char_to_u8(chars[0])?;
            let g = hex_char_to_u8(chars[1])?;
            let b = hex_char_to_u8(chars[2])?;
            Some(Color::rgb(r << 4 | r, g << 4 | g, b << 4 | b))
        }
        _ => None,
    }
}

/// Convert a single hex character to its 4-bit value.
fn hex_char_to_u8(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

// ============================================================================
// Preset palette (Windows-style basic colors + extended)
// ============================================================================

/// Standard preset color palette (48 colors, arranged similarly to the
/// Windows color picker basic/custom grid).
pub const PRESET_COLORS: &[Color] = &[
    // Row 1: pure primaries and secondaries
    Color::from_hex(0xFF0000), // Red
    Color::from_hex(0xFF8000), // Orange
    Color::from_hex(0xFFFF00), // Yellow
    Color::from_hex(0x80FF00), // Chartreuse
    Color::from_hex(0x00FF00), // Green
    Color::from_hex(0x00FF80), // Spring Green
    Color::from_hex(0x00FFFF), // Cyan
    Color::from_hex(0x0080FF), // Azure
    Color::from_hex(0x0000FF), // Blue
    Color::from_hex(0x8000FF), // Violet
    Color::from_hex(0xFF00FF), // Magenta
    Color::from_hex(0xFF0080), // Rose
    // Row 2: lighter tints
    Color::from_hex(0xFF8080), // Light Red
    Color::from_hex(0xFFBF80), // Light Orange
    Color::from_hex(0xFFFF80), // Light Yellow
    Color::from_hex(0xBFFF80), // Light Chartreuse
    Color::from_hex(0x80FF80), // Light Green
    Color::from_hex(0x80FFBF), // Light Spring
    Color::from_hex(0x80FFFF), // Light Cyan
    Color::from_hex(0x80BFFF), // Light Azure
    Color::from_hex(0x8080FF), // Light Blue
    Color::from_hex(0xBF80FF), // Light Violet
    Color::from_hex(0xFF80FF), // Light Magenta
    Color::from_hex(0xFF80BF), // Light Rose
    // Row 3: darker shades
    Color::from_hex(0x800000), // Dark Red
    Color::from_hex(0x804000), // Dark Orange
    Color::from_hex(0x808000), // Dark Yellow (Olive)
    Color::from_hex(0x408000), // Dark Chartreuse
    Color::from_hex(0x008000), // Dark Green
    Color::from_hex(0x008040), // Dark Spring
    Color::from_hex(0x008080), // Dark Cyan (Teal)
    Color::from_hex(0x004080), // Dark Azure
    Color::from_hex(0x000080), // Dark Blue (Navy)
    Color::from_hex(0x400080), // Dark Violet
    Color::from_hex(0x800080), // Dark Magenta (Purple)
    Color::from_hex(0x800040), // Dark Rose
    // Row 4: grayscale
    Color::from_hex(0x000000), // Black
    Color::from_hex(0x202020),
    Color::from_hex(0x404040),
    Color::from_hex(0x606060),
    Color::from_hex(0x808080),
    Color::from_hex(0xA0A0A0),
    Color::from_hex(0xC0C0C0),
    Color::from_hex(0xE0E0E0),
    Color::from_hex(0xFFFFFF), // White
    Color::from_hex(0xFFC0CB), // Pink
    Color::from_hex(0xA52A2A), // Brown
    Color::from_hex(0xF5DEB3), // Wheat
];

// ============================================================================
// Interaction state tracking
// ============================================================================

/// Which part of the color picker is currently being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragTarget {
    /// Dragging inside the saturation/value square.
    SvSquare,
    /// Dragging the hue bar.
    HueBar,
    /// Dragging the alpha bar.
    AlphaBar,
    /// Dragging one of the RGB sliders (channel index 0=R, 1=G, 2=B).
    RgbSlider(u8),
    /// Dragging one of the HSV sliders (0=H, 1=S, 2=V).
    HsvSlider(u8),
}

/// Active input mode for the color picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerMode {
    /// Normal interactive editing.
    Normal,
    /// Eyedropper mode: waiting for the compositor to report a pixel color.
    Eyedropper,
}

/// Events emitted by the color picker to the parent widget/application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorPickerEvent {
    /// The color was changed interactively (live preview).
    Changed(Color),
    /// The user confirmed their color selection.
    Confirmed(Color),
    /// The user cancelled (reverted to original color).
    Cancelled,
    /// Eyedropper mode was activated (compositor should start screen pick).
    EyedropperActivated,
    /// Eyedropper mode was deactivated.
    EyedropperDeactivated,
}

// ============================================================================
// ColorPicker — compact inline widget
// ============================================================================

/// Compact inline color picker widget.
///
/// Shows a saturation/value square, a hue bar, and a small preview swatch.
/// Suitable for embedding within property panels or forms where space is tight.
pub struct ColorPicker {
    /// Current color in HSV (authoritative internal representation).
    hsv: Hsv,
    /// Alpha channel (0-255).
    alpha: u8,
    /// Original color when the picker was opened (for cancel/revert).
    original: Color,
    /// Current drag target (if mouse is held).
    drag: Option<DragTarget>,
    /// Active mode.
    mode: PickerMode,
    /// Recent colors chosen by the user.
    recent_colors: Vec<Color>,
    /// Hex input buffer (for text entry).
    hex_input: String,
    /// Whether the hex input field is focused.
    hex_focused: bool,
    /// Width of the SV square (configurable for compact vs full).
    sv_size: f32,
}

impl ColorPicker {
    /// Create a new color picker starting at the given color.
    pub fn new(initial: Color) -> Self {
        let hsv = rgb_to_hsv(initial.r, initial.g, initial.b);
        Self {
            hsv,
            alpha: initial.a,
            original: initial,
            drag: None,
            mode: PickerMode::Normal,
            recent_colors: Vec::new(),
            hex_input: color_to_hex_string(initial),
            hex_focused: false,
            sv_size: SV_SQUARE_SIZE,
        }
    }

    /// Create a compact picker with a smaller SV square.
    pub fn compact(initial: Color) -> Self {
        let mut picker = Self::new(initial);
        picker.sv_size = 120.0;
        picker
    }

    /// Get the currently selected color (including alpha).
    pub fn current_color(&self) -> Color {
        let (r, g, b) = hsv_to_rgb(self.hsv);
        Color::rgba(r, g, b, self.alpha)
    }

    /// Get the original color the picker was opened with.
    pub fn original_color(&self) -> Color {
        self.original
    }

    /// Set the color externally (e.g., from eyedropper result).
    pub fn set_color(&mut self, color: Color) {
        self.hsv = rgb_to_hsv(color.r, color.g, color.b);
        self.alpha = color.a;
        self.hex_input = color_to_hex_string(color);
    }

    /// Feed an eyedropper result back into the picker.
    pub fn set_eyedropper_result(&mut self, color: Color) {
        self.set_color(color);
        self.mode = PickerMode::Normal;
    }

    /// Get the current mode.
    pub fn mode(&self) -> PickerMode {
        self.mode
    }

    /// Activate eyedropper mode.
    pub fn activate_eyedropper(&mut self) {
        self.mode = PickerMode::Eyedropper;
    }

    /// Cancel eyedropper mode without picking.
    pub fn cancel_eyedropper(&mut self) {
        self.mode = PickerMode::Normal;
    }

    /// Get the HSV value.
    pub fn hsv(&self) -> Hsv {
        self.hsv
    }

    /// Get alpha (0-255).
    pub fn alpha(&self) -> u8 {
        self.alpha
    }

    /// Set HSV directly (preserves alpha).
    pub fn set_hsv(&mut self, hsv: Hsv) {
        self.hsv = hsv;
        self.sync_hex_from_hsv();
    }

    /// Set alpha directly.
    pub fn set_alpha(&mut self, a: u8) {
        self.alpha = a;
    }

    /// Set RGB directly (updates internal HSV and hex).
    pub fn set_rgb(&mut self, r: u8, g: u8, b: u8) {
        self.hsv = rgb_to_hsv(r, g, b);
        self.sync_hex_from_hsv();
    }

    /// Push the current color into recent-colors history.
    pub fn commit_to_recent(&mut self) {
        let color = self.current_color();
        // Remove duplicate if exists, then push to front.
        self.recent_colors.retain(|c| *c != color);
        self.recent_colors.insert(0, color);
        if self.recent_colors.len() > MAX_RECENT_COLORS {
            self.recent_colors.truncate(MAX_RECENT_COLORS);
        }
    }

    /// Get recent colors.
    pub fn recent_colors(&self) -> &[Color] {
        &self.recent_colors
    }

    /// Set the recent colors list externally (e.g., restore from session).
    pub fn set_recent_colors(&mut self, colors: Vec<Color>) {
        self.recent_colors = colors;
        if self.recent_colors.len() > MAX_RECENT_COLORS {
            self.recent_colors.truncate(MAX_RECENT_COLORS);
        }
    }

    /// Get the hex input string.
    pub fn hex_input(&self) -> &str {
        &self.hex_input
    }

    /// Handle a mouse event. The `origin_x`/`origin_y` are the top-left corner
    /// of the picker widget in parent coordinates.
    pub fn handle_mouse(
        &mut self,
        event: &MouseEvent,
        origin_x: f32,
        origin_y: f32,
    ) -> Option<ColorPickerEvent> {
        if self.mode == PickerMode::Eyedropper {
            // In eyedropper mode, all clicks confirm the picked color
            // (actual pixel reading is done by the compositor).
            if matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
                self.mode = PickerMode::Normal;
                return Some(ColorPickerEvent::EyedropperDeactivated);
            }
            if matches!(event.kind, MouseEventKind::Press(MouseButton::Right)) {
                self.mode = PickerMode::Normal;
                return Some(ColorPickerEvent::EyedropperDeactivated);
            }
            return None;
        }

        let local_x = event.x - origin_x;
        let local_y = event.y - origin_y;

        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.drag = self.hit_test(local_x, local_y);
                if self.drag.is_some() {
                    self.apply_drag(local_x, local_y);
                    return Some(ColorPickerEvent::Changed(self.current_color()));
                }
            }
            MouseEventKind::Move => {
                if self.drag.is_some() {
                    self.apply_drag(local_x, local_y);
                    return Some(ColorPickerEvent::Changed(self.current_color()));
                }
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if self.drag.is_some() {
                    self.drag = None;
                    self.sync_hex_from_hsv();
                    return Some(ColorPickerEvent::Changed(self.current_color()));
                }
            }
            _ => {}
        }
        None
    }

    /// Handle a key event (primarily for hex input).
    pub fn handle_key(&mut self, event: &KeyEvent) -> Option<ColorPickerEvent> {
        if !event.pressed {
            return None;
        }

        if self.mode == PickerMode::Eyedropper {
            if event.key == Key::Escape {
                self.mode = PickerMode::Normal;
                return Some(ColorPickerEvent::EyedropperDeactivated);
            }
            return None;
        }

        if self.hex_focused {
            match event.key {
                Key::Backspace => {
                    self.hex_input.pop();
                    self.try_apply_hex();
                    return Some(ColorPickerEvent::Changed(self.current_color()));
                }
                Key::Enter => {
                    self.try_apply_hex();
                    self.hex_focused = false;
                    return Some(ColorPickerEvent::Changed(self.current_color()));
                }
                Key::Escape => {
                    self.hex_focused = false;
                    self.sync_hex_from_hsv();
                }
                _ => {
                    if let Some(ch) = event.text {
                        if ch.is_ascii_hexdigit() && self.hex_input.len() < 8 {
                            self.hex_input.push(ch.to_ascii_uppercase());
                            self.try_apply_hex();
                            return Some(ColorPickerEvent::Changed(self.current_color()));
                        }
                    }
                }
            }
        } else {
            // Global keyboard shortcuts
            match event.key {
                Key::Escape => {
                    self.set_color(self.original);
                    return Some(ColorPickerEvent::Cancelled);
                }
                Key::Enter => {
                    self.commit_to_recent();
                    return Some(ColorPickerEvent::Confirmed(self.current_color()));
                }
                _ => {}
            }
        }
        None
    }

    /// Render the compact picker at the given origin. Returns render commands.
    pub fn render(&self, x: f32, y: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let sv_size = self.sv_size;

        // Background panel
        let total_width = sv_size + PADDING + HUE_BAR_WIDTH + PADDING + PREVIEW_SIZE;
        let total_height = sv_size;
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: total_width + PADDING * 2.0,
            height: total_height + PADDING * 2.0,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let cx = x + PADDING;
        let cy = y + PADDING;

        // Saturation/Value square
        self.render_sv_square(&mut cmds, cx, cy, sv_size);

        // Hue bar (vertical, to the right of SV square)
        let hue_x = cx + sv_size + PADDING;
        self.render_hue_bar(&mut cmds, hue_x, cy, HUE_BAR_WIDTH, sv_size);

        // Preview swatch (to the right of hue bar)
        let preview_x = hue_x + HUE_BAR_WIDTH + PADDING;
        self.render_preview(&mut cmds, preview_x, cy);

        cmds
    }

    // --- Private rendering sub-methods ---

    fn render_sv_square(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, size: f32) {
        // The SV square is a gradient: left-to-right = saturation (0→1),
        // top-to-bottom = value (1→0). The base color is the current hue at
        // full saturation and value.
        //
        // We approximate this with a grid of colored rectangles.
        let steps = 16u32;
        let cell = size / steps as f32;

        for sy in 0..steps {
            for sx in 0..steps {
                let s = (sx as f32 + 0.5) / steps as f32;
                let v = 1.0 - (sy as f32 + 0.5) / steps as f32;
                let (r, g, b) = hsv_to_rgb(Hsv::new(self.hsv.h, s, v));
                cmds.push(RenderCommand::FillRect {
                    x: x + sx as f32 * cell,
                    y: y + sy as f32 * cell,
                    width: cell + 0.5, // slight overlap to avoid gaps
                    height: cell + 0.5,
                    color: Color::rgb(r, g, b),
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Border around the SV square
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width: size,
            height: size,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Crosshair indicator at current S/V position
        let cx = x + self.hsv.s * size;
        let cy = y + (1.0 - self.hsv.v) * size;
        let indicator_radius = 5.0;

        // White outline circle (approximated as a stroked rect)
        cmds.push(RenderCommand::StrokeRect {
            x: cx - indicator_radius,
            y: cy - indicator_radius,
            width: indicator_radius * 2.0,
            height: indicator_radius * 2.0,
            color: Color::WHITE,
            line_width: 2.0,
            corner_radii: CornerRadii::all(indicator_radius),
        });
        // Black inner circle for contrast
        cmds.push(RenderCommand::StrokeRect {
            x: cx - indicator_radius + 1.0,
            y: cy - indicator_radius + 1.0,
            width: (indicator_radius - 1.0) * 2.0,
            height: (indicator_radius - 1.0) * 2.0,
            color: Color::BLACK,
            line_width: 1.0,
            corner_radii: CornerRadii::all(indicator_radius - 1.0),
        });
    }

    fn render_hue_bar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Vertical hue bar: top = 0 degrees (red), bottom = 360 degrees (red).
        let segments = 36u32;
        let seg_height = height / segments as f32;

        for i in 0..segments {
            let hue = (i as f32 / segments as f32) * 360.0;
            let (r, g, b) = hsv_to_rgb(Hsv::new(hue, 1.0, 1.0));
            cmds.push(RenderCommand::FillRect {
                x,
                y: y + i as f32 * seg_height,
                width,
                height: seg_height + 0.5,
                color: Color::rgb(r, g, b),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Hue indicator (horizontal line)
        let indicator_y = y + (self.hsv.h / 360.0) * height;
        cmds.push(RenderCommand::FillRect {
            x: x - 2.0,
            y: indicator_y - 2.0,
            width: width + 4.0,
            height: 4.0,
            color: Color::WHITE,
            corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: x - 1.0,
            y: indicator_y - 1.0,
            width: width + 2.0,
            height: 2.0,
            color: Color::BLACK,
            corner_radii: CornerRadii::all(1.0),
        });
    }

    fn render_preview(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        let current = self.current_color();

        // Label
        cmds.push(RenderCommand::Text {
            x,
            y: y - 2.0,
            text: String::from("New"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Current color swatch
        let swatch_y = y + 12.0;
        render_checkerboard(cmds, x, swatch_y, PREVIEW_SIZE, PREVIEW_SIZE / 2.0);
        cmds.push(RenderCommand::FillRect {
            x,
            y: swatch_y,
            width: PREVIEW_SIZE,
            height: PREVIEW_SIZE / 2.0,
            color: current,
            corner_radii: CornerRadii {
                top_left: 3.0,
                top_right: 3.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Original color swatch
        let orig_y = swatch_y + PREVIEW_SIZE / 2.0;
        cmds.push(RenderCommand::Text {
            x,
            y: orig_y + PREVIEW_SIZE / 2.0 + 4.0,
            text: String::from("Prev"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        render_checkerboard(cmds, x, orig_y, PREVIEW_SIZE, PREVIEW_SIZE / 2.0);
        cmds.push(RenderCommand::FillRect {
            x,
            y: orig_y,
            width: PREVIEW_SIZE,
            height: PREVIEW_SIZE / 2.0,
            color: self.original,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: 3.0,
                bottom_right: 3.0,
            },
        });

        // Border around both swatches
        cmds.push(RenderCommand::StrokeRect {
            x,
            y: swatch_y,
            width: PREVIEW_SIZE,
            height: PREVIEW_SIZE,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });
    }

    // --- Private interaction helpers ---

    /// Determine which part of the picker a local coordinate hits.
    fn hit_test(&self, local_x: f32, local_y: f32) -> Option<DragTarget> {
        let sv_size = self.sv_size;

        // SV square: starts at (0, 0) in content area
        if local_x >= 0.0 && local_x <= sv_size && local_y >= 0.0 && local_y <= sv_size {
            return Some(DragTarget::SvSquare);
        }

        // Hue bar: to the right of SV square
        let hue_x = sv_size + PADDING;
        let hue_x_end = hue_x + HUE_BAR_WIDTH;
        if local_x >= hue_x && local_x <= hue_x_end && local_y >= 0.0 && local_y <= sv_size {
            return Some(DragTarget::HueBar);
        }

        None
    }

    /// Apply a drag interaction based on the current drag target and position.
    fn apply_drag(&mut self, local_x: f32, local_y: f32) {
        let sv_size = self.sv_size;

        match self.drag {
            Some(DragTarget::SvSquare) => {
                let s = (local_x / sv_size).clamp(0.0, 1.0);
                let v = 1.0 - (local_y / sv_size).clamp(0.0, 1.0);
                self.hsv.s = s;
                self.hsv.v = v;
            }
            Some(DragTarget::HueBar) => {
                let h = (local_y / sv_size).clamp(0.0, 1.0) * 360.0;
                self.hsv.h = h;
            }
            Some(DragTarget::AlphaBar) => {
                // Alpha bar occupies below the SV square in the full dialog.
                let alpha_width = sv_size;
                let a = (local_x / alpha_width).clamp(0.0, 1.0);
                self.alpha = (a * 255.0 + 0.5) as u8;
            }
            Some(DragTarget::RgbSlider(channel)) => {
                let slider_width = sv_size;
                let val = ((local_x / slider_width).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let (mut r, mut g, mut b) = hsv_to_rgb(self.hsv);
                match channel {
                    0 => r = val,
                    1 => g = val,
                    _ => b = val,
                }
                self.hsv = rgb_to_hsv(r, g, b);
            }
            Some(DragTarget::HsvSlider(component)) => {
                let slider_width = sv_size;
                let t = (local_x / slider_width).clamp(0.0, 1.0);
                match component {
                    0 => self.hsv.h = t * 360.0,
                    1 => self.hsv.s = t,
                    _ => self.hsv.v = t,
                }
            }
            None => {}
        }
    }

    /// Sync the hex input buffer from the current HSV state.
    fn sync_hex_from_hsv(&mut self) {
        let (r, g, b) = hsv_to_rgb(self.hsv);
        self.hex_input = format!("{:02X}{:02X}{:02X}", r, g, b);
    }

    /// Try to apply the hex input buffer to the color. If invalid, no change.
    fn try_apply_hex(&mut self) {
        if let Some(color) = parse_hex_color(&self.hex_input) {
            self.hsv = rgb_to_hsv(color.r, color.g, color.b);
            if self.hex_input.len() == 8 {
                self.alpha = color.a;
            }
        }
    }
}

// ============================================================================
// ColorPickerDialog — full dialog-style picker
// ============================================================================

/// Which tab/section is shown in the full dialog's slider area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderTab {
    Rgb,
    Hsv,
}

/// Full-featured color picker dialog.
///
/// Includes all components: SV square, hue bar, RGB sliders, HSV sliders,
/// hex input, alpha bar, color preview, preset palette, eyedropper button,
/// and recent colors.
pub struct ColorPickerDialog {
    /// The underlying picker state.
    picker: ColorPicker,
    /// Which slider tab is active.
    slider_tab: SliderTab,
    /// Whether the dialog has been confirmed.
    confirmed: bool,
    /// Whether the dialog has been cancelled.
    cancelled: bool,
}

impl ColorPickerDialog {
    /// Create a new color picker dialog starting at the given color.
    pub fn new(initial: Color) -> Self {
        Self {
            picker: ColorPicker::new(initial),
            slider_tab: SliderTab::Rgb,
            confirmed: false,
            cancelled: false,
        }
    }

    /// Get the underlying picker.
    pub fn picker(&self) -> &ColorPicker {
        &self.picker
    }

    /// Get the underlying picker mutably.
    pub fn picker_mut(&mut self) -> &mut ColorPicker {
        &mut self.picker
    }

    /// Get the currently selected color.
    pub fn current_color(&self) -> Color {
        self.picker.current_color()
    }

    /// Whether the dialog was confirmed.
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Whether the dialog was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Get the active slider tab.
    pub fn slider_tab(&self) -> SliderTab {
        self.slider_tab
    }

    /// Switch the slider tab.
    pub fn set_slider_tab(&mut self, tab: SliderTab) {
        self.slider_tab = tab;
    }

    /// Handle a key event for the dialog.
    pub fn handle_key(&mut self, event: &KeyEvent) -> Option<ColorPickerEvent> {
        if !event.pressed {
            return None;
        }

        match event.key {
            Key::Escape => {
                self.cancelled = true;
                self.picker.set_color(self.picker.original);
                Some(ColorPickerEvent::Cancelled)
            }
            Key::Enter if !self.picker.hex_focused => {
                self.confirmed = true;
                self.picker.commit_to_recent();
                Some(ColorPickerEvent::Confirmed(self.picker.current_color()))
            }
            Key::Tab if event.modifiers.ctrl => {
                // Switch slider tabs
                self.slider_tab = match self.slider_tab {
                    SliderTab::Rgb => SliderTab::Hsv,
                    SliderTab::Hsv => SliderTab::Rgb,
                };
                None
            }
            _ => self.picker.handle_key(event),
        }
    }

    /// Handle a mouse event (coordinates relative to the dialog origin).
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> Option<ColorPickerEvent> {
        // Delegate to the inner picker for the SV square / hue bar region.
        // The dialog places the picker at (PADDING, PADDING).
        if let Some(evt) = self.picker.handle_mouse(event, PADDING, PADDING) {
            return Some(evt);
        }

        // Check clicks on preset swatches, eyedropper button, etc.
        if matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
            let local_x = event.x;
            let local_y = event.y;

            // Preset palette area
            if let Some(color) = self.hit_test_presets(local_x, local_y) {
                self.picker.set_color(color);
                return Some(ColorPickerEvent::Changed(color));
            }

            // Recent colors area
            if let Some(color) = self.hit_test_recent(local_x, local_y) {
                self.picker.set_color(color);
                return Some(ColorPickerEvent::Changed(color));
            }

            // Eyedropper button
            if self.hit_test_eyedropper(local_x, local_y) {
                self.picker.activate_eyedropper();
                return Some(ColorPickerEvent::EyedropperActivated);
            }

            // Slider tab buttons
            if self.hit_test_slider_tab(local_x, local_y, SliderTab::Rgb) {
                self.slider_tab = SliderTab::Rgb;
                return None;
            }
            if self.hit_test_slider_tab(local_x, local_y, SliderTab::Hsv) {
                self.slider_tab = SliderTab::Hsv;
                return None;
            }

            // Alpha bar
            let alpha_y = self.alpha_bar_y();
            if local_y >= alpha_y && local_y <= alpha_y + ALPHA_BAR_HEIGHT {
                let sv_size = self.picker.sv_size;
                let t = ((local_x - PADDING) / sv_size).clamp(0.0, 1.0);
                self.picker.alpha = (t * 255.0 + 0.5) as u8;
                self.picker.drag = Some(DragTarget::AlphaBar);
                return Some(ColorPickerEvent::Changed(self.picker.current_color()));
            }

            // RGB/HSV sliders
            if let Some(target) = self.hit_test_sliders(local_x, local_y) {
                self.picker.drag = Some(target);
                self.apply_slider_drag(local_x);
                return Some(ColorPickerEvent::Changed(self.picker.current_color()));
            }
        }

        // Continue drag for sliders/alpha
        if matches!(event.kind, MouseEventKind::Move) && self.picker.drag.is_some() {
            match self.picker.drag {
                Some(DragTarget::AlphaBar) => {
                    let sv_size = self.picker.sv_size;
                    let t = ((event.x - PADDING) / sv_size).clamp(0.0, 1.0);
                    self.picker.alpha = (t * 255.0 + 0.5) as u8;
                    return Some(ColorPickerEvent::Changed(self.picker.current_color()));
                }
                Some(DragTarget::RgbSlider(_) | DragTarget::HsvSlider(_)) => {
                    self.apply_slider_drag(event.x);
                    return Some(ColorPickerEvent::Changed(self.picker.current_color()));
                }
                _ => {}
            }
        }

        if matches!(event.kind, MouseEventKind::Release(MouseButton::Left)) {
            self.picker.drag = None;
            self.picker.sync_hex_from_hsv();
        }

        None
    }

    /// Render the full dialog at the given dimensions.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Dialog background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS + 2.0),
        });

        // Title bar
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: 32.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: CORNER_RADIUS + 2.0,
                top_right: CORNER_RADIUS + 2.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 9.0,
            text: String::from("Color Picker"),
            color: COLOR_TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let content_y = 36.0;
        let sv_size = self.picker.sv_size;

        // --- Left column: SV square + Hue bar ---
        let sv_x = PADDING;
        let sv_y = content_y + PADDING;
        self.picker.render_sv_square(&mut cmds, sv_x, sv_y, sv_size);

        let hue_x = sv_x + sv_size + PADDING;
        self.picker.render_hue_bar(&mut cmds, hue_x, sv_y, HUE_BAR_WIDTH, sv_size);

        // Alpha bar below SV square
        let alpha_y = self.alpha_bar_y();
        self.render_alpha_bar(&mut cmds, sv_x, alpha_y, sv_size);

        // --- Right column: Preview, sliders, hex, eyedropper ---
        let right_x = hue_x + HUE_BAR_WIDTH + PADDING * 2.0;
        let right_width = width - right_x - PADDING;

        // Color preview (new vs old)
        self.picker.render_preview(&mut cmds, right_x, sv_y);

        // Hex input
        let hex_y = sv_y + PREVIEW_SIZE + PADDING * 2.0 + 16.0;
        self.render_hex_input(&mut cmds, right_x, hex_y, right_width);

        // Eyedropper button
        let eye_y = hex_y + 32.0;
        self.render_eyedropper_button(&mut cmds, right_x, eye_y);

        // --- Slider section ---
        let slider_y = alpha_y + ALPHA_BAR_HEIGHT + PADDING * 2.0;
        self.render_slider_tabs(&mut cmds, PADDING, slider_y);
        self.render_sliders(&mut cmds, PADDING, slider_y + 28.0, sv_size);

        // --- Preset palette ---
        let preset_y = slider_y + 28.0 + (SLIDER_HEIGHT + PADDING) * 3.0 + PADDING;
        self.render_preset_palette(&mut cmds, PADDING, preset_y, width - PADDING * 2.0);

        // --- Recent colors ---
        let recent_y = preset_y + self.preset_palette_height() + PADDING;
        self.render_recent_colors(&mut cmds, PADDING, recent_y, width - PADDING * 2.0);

        // --- Bottom buttons ---
        let btn_y = height - 44.0;
        self.render_bottom_buttons(&mut cmds, btn_y, width);

        cmds
    }

    // --- Private dialog-specific rendering ---

    fn render_alpha_bar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        // Checkerboard background to show transparency
        render_checkerboard(cmds, x, y, width, ALPHA_BAR_HEIGHT);

        // Alpha gradient from transparent to opaque (current color)
        let (r, g, b) = hsv_to_rgb(self.picker.hsv);
        let steps = 32u32;
        let step_width = width / steps as f32;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let a = (t * 255.0 + 0.5) as u8;
            cmds.push(RenderCommand::FillRect {
                x: x + i as f32 * step_width,
                y,
                width: step_width + 0.5,
                height: ALPHA_BAR_HEIGHT,
                color: Color::rgba(r, g, b, a),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height: ALPHA_BAR_HEIGHT,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Alpha indicator
        let t = self.picker.alpha as f32 / 255.0;
        let indicator_x = x + t * width;
        cmds.push(RenderCommand::FillRect {
            x: indicator_x - 2.0,
            y: y - 2.0,
            width: 4.0,
            height: ALPHA_BAR_HEIGHT + 4.0,
            color: Color::WHITE,
            corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: indicator_x - 1.0,
            y: y - 1.0,
            width: 2.0,
            height: ALPHA_BAR_HEIGHT + 2.0,
            color: Color::BLACK,
            corner_radii: CornerRadii::all(1.0),
        });

        // Label
        cmds.push(RenderCommand::Text {
            x: x + width + 6.0,
            y: y + 2.0,
            text: format!("A: {}", self.picker.alpha),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_hex_input(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        // Label
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: String::from("Hex:"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Input field
        let input_x = x + 32.0;
        let input_width = width - 32.0;
        let border_color = if self.picker.hex_focused {
            COLOR_BLUE
        } else {
            COLOR_SURFACE2
        };

        cmds.push(RenderCommand::FillRect {
            x: input_x,
            y: y - 2.0,
            width: input_width,
            height: 22.0,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: input_x,
            y: y - 2.0,
            width: input_width,
            height: 22.0,
            color: border_color,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });

        // "#" prefix
        cmds.push(RenderCommand::Text {
            x: input_x + 4.0,
            y: y + 2.0,
            text: String::from("#"),
            color: COLOR_OVERLAY,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        // Hex value
        cmds.push(RenderCommand::Text {
            x: input_x + 14.0,
            y: y + 2.0,
            text: self.picker.hex_input.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(input_width - 20.0),
        });
    }

    fn render_eyedropper_button(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        let is_active = self.picker.mode == PickerMode::Eyedropper;
        let bg = if is_active { COLOR_TEAL } else { COLOR_SURFACE1 };
        let text_color = if is_active { COLOR_BASE } else { COLOR_TEXT };

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 90.0,
            height: 24.0,
            color: bg,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 6.0,
            text: String::from("Eyedropper"),
            color: text_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_slider_tabs(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        // RGB tab
        let rgb_bg = if self.slider_tab == SliderTab::Rgb {
            COLOR_BLUE
        } else {
            COLOR_SURFACE1
        };
        let rgb_text = if self.slider_tab == SliderTab::Rgb {
            COLOR_BASE
        } else {
            COLOR_TEXT
        };
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 40.0,
            height: 22.0,
            color: rgb_bg,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 5.0,
            text: String::from("RGB"),
            color: rgb_text,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // HSV tab
        let hsv_bg = if self.slider_tab == SliderTab::Hsv {
            COLOR_BLUE
        } else {
            COLOR_SURFACE1
        };
        let hsv_text = if self.slider_tab == SliderTab::Hsv {
            COLOR_BASE
        } else {
            COLOR_TEXT
        };
        cmds.push(RenderCommand::FillRect {
            x: x + 46.0,
            y,
            width: 40.0,
            height: 22.0,
            color: hsv_bg,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 54.0,
            y: y + 5.0,
            text: String::from("HSV"),
            color: hsv_text,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_sliders(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        match self.slider_tab {
            SliderTab::Rgb => {
                let (r, g, b) = hsv_to_rgb(self.picker.hsv);
                self.render_channel_slider(
                    cmds,
                    x,
                    y,
                    width,
                    "R",
                    r as f32 / 255.0,
                    Color::RED,
                    r,
                );
                self.render_channel_slider(
                    cmds,
                    x,
                    y + SLIDER_HEIGHT + PADDING,
                    width,
                    "G",
                    g as f32 / 255.0,
                    Color::GREEN,
                    g,
                );
                self.render_channel_slider(
                    cmds,
                    x,
                    y + (SLIDER_HEIGHT + PADDING) * 2.0,
                    width,
                    "B",
                    b as f32 / 255.0,
                    Color::BLUE,
                    b,
                );
            }
            SliderTab::Hsv => {
                let hue_frac = self.picker.hsv.h / 360.0;
                let h_display = self.picker.hsv.h as u8;
                let s_display = (self.picker.hsv.s * 100.0 + 0.5) as u8;
                let v_display = (self.picker.hsv.v * 100.0 + 0.5) as u8;

                // Hue slider with rainbow gradient
                self.render_hue_slider(cmds, x, y, width, hue_frac, h_display);
                self.render_channel_slider(
                    cmds,
                    x,
                    y + SLIDER_HEIGHT + PADDING,
                    width,
                    "S",
                    self.picker.hsv.s,
                    COLOR_BLUE,
                    s_display,
                );
                self.render_channel_slider(
                    cmds,
                    x,
                    y + (SLIDER_HEIGHT + PADDING) * 2.0,
                    width,
                    "V",
                    self.picker.hsv.v,
                    Color::WHITE,
                    v_display,
                );
            }
        }
    }

    fn render_channel_slider(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        fraction: f32,
        color: Color,
        value_display: u8,
    ) {
        let label_width = 20.0;
        let value_width = 30.0;
        let track_x = x + label_width;
        let track_width = width - label_width - value_width - PADDING;

        // Label
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: label.to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Track background
        let track_y = y + (SLIDER_HEIGHT - SLIDER_TRACK_HEIGHT) / 2.0;
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: track_y,
            width: track_width,
            height: SLIDER_TRACK_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(SLIDER_TRACK_HEIGHT / 2.0),
        });

        // Filled portion
        let fill_width = fraction.clamp(0.0, 1.0) * track_width;
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: track_y,
            width: fill_width,
            height: SLIDER_TRACK_HEIGHT,
            color,
            corner_radii: CornerRadii::all(SLIDER_TRACK_HEIGHT / 2.0),
        });

        // Thumb
        let thumb_x = track_x + fill_width;
        let thumb_size = SLIDER_HEIGHT - 2.0;
        cmds.push(RenderCommand::FillRect {
            x: thumb_x - thumb_size / 2.0,
            y: y + 1.0,
            width: thumb_size,
            height: thumb_size,
            color: Color::WHITE,
            corner_radii: CornerRadii::all(thumb_size / 2.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: thumb_x - thumb_size / 2.0,
            y: y + 1.0,
            width: thumb_size,
            height: thumb_size,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(thumb_size / 2.0),
        });

        // Value text
        cmds.push(RenderCommand::Text {
            x: track_x + track_width + 4.0,
            y: y + 2.0,
            text: format!("{value_display}"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_hue_slider(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        fraction: f32,
        value_display: u8,
    ) {
        let label_width = 20.0;
        let value_width = 30.0;
        let track_x = x + label_width;
        let track_width = width - label_width - value_width - PADDING;

        // Label
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: String::from("H"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Rainbow gradient track
        let track_y = y + (SLIDER_HEIGHT - SLIDER_TRACK_HEIGHT) / 2.0;
        let segments = 36u32;
        let seg_width = track_width / segments as f32;
        for i in 0..segments {
            let hue = (i as f32 / segments as f32) * 360.0;
            let (r, g, b) = hsv_to_rgb(Hsv::new(hue, 1.0, 1.0));
            cmds.push(RenderCommand::FillRect {
                x: track_x + i as f32 * seg_width,
                y: track_y,
                width: seg_width + 0.5,
                height: SLIDER_TRACK_HEIGHT,
                color: Color::rgb(r, g, b),
                corner_radii: if i == 0 {
                    CornerRadii {
                        top_left: SLIDER_TRACK_HEIGHT / 2.0,
                        bottom_left: SLIDER_TRACK_HEIGHT / 2.0,
                        top_right: 0.0,
                        bottom_right: 0.0,
                    }
                } else if i == segments - 1 {
                    CornerRadii {
                        top_left: 0.0,
                        bottom_left: 0.0,
                        top_right: SLIDER_TRACK_HEIGHT / 2.0,
                        bottom_right: SLIDER_TRACK_HEIGHT / 2.0,
                    }
                } else {
                    CornerRadii::ZERO
                },
            });
        }

        // Thumb
        let thumb_x = track_x + fraction.clamp(0.0, 1.0) * track_width;
        let thumb_size = SLIDER_HEIGHT - 2.0;
        cmds.push(RenderCommand::FillRect {
            x: thumb_x - thumb_size / 2.0,
            y: y + 1.0,
            width: thumb_size,
            height: thumb_size,
            color: Color::WHITE,
            corner_radii: CornerRadii::all(thumb_size / 2.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: thumb_x - thumb_size / 2.0,
            y: y + 1.0,
            width: thumb_size,
            height: thumb_size,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(thumb_size / 2.0),
        });

        // Value text
        cmds.push(RenderCommand::Text {
            x: track_x + track_width + 4.0,
            y: y + 2.0,
            text: format!("{value_display}"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_preset_palette(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        available_width: f32,
    ) {
        // Section label
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: String::from("Presets"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let swatch_y = y + 14.0;
        let cols = self.preset_columns(available_width);

        for (i, color) in PRESET_COLORS.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let sx = x + col as f32 * (SWATCH_SIZE + SWATCH_GAP);
            let sy = swatch_y + row as f32 * (SWATCH_SIZE + SWATCH_GAP);

            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: sy,
                width: SWATCH_SIZE,
                height: SWATCH_SIZE,
                color: *color,
                corner_radii: CornerRadii::all(2.0),
            });

            // Highlight if this matches current color
            let current = self.picker.current_color();
            if color.r == current.r && color.g == current.g && color.b == current.b {
                cmds.push(RenderCommand::StrokeRect {
                    x: sx - 1.0,
                    y: sy - 1.0,
                    width: SWATCH_SIZE + 2.0,
                    height: SWATCH_SIZE + 2.0,
                    color: Color::WHITE,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(3.0),
                });
            }
        }
    }

    fn render_recent_colors(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        _available_width: f32,
    ) {
        if self.picker.recent_colors.is_empty() {
            return;
        }

        // Section label
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: String::from("Recent"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let swatch_y = y + 14.0;
        for (i, color) in self.picker.recent_colors.iter().enumerate() {
            let sx = x + i as f32 * (SWATCH_SIZE + SWATCH_GAP);
            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: swatch_y,
                width: SWATCH_SIZE,
                height: SWATCH_SIZE,
                color: *color,
                corner_radii: CornerRadii::all(2.0),
            });
        }
    }

    fn render_bottom_buttons(&self, cmds: &mut Vec<RenderCommand>, y: f32, width: f32) {
        let btn_width = 70.0;
        let btn_height = 28.0;

        // OK button
        let ok_x = width - btn_width * 2.0 - PADDING * 3.0;
        cmds.push(RenderCommand::FillRect {
            x: ok_x,
            y,
            width: btn_width,
            height: btn_height,
            color: COLOR_BLUE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: ok_x + (btn_width - 16.0) / 2.0,
            y: y + 8.0,
            text: String::from("OK"),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Cancel button
        let cancel_x = width - btn_width - PADDING;
        cmds.push(RenderCommand::FillRect {
            x: cancel_x,
            y,
            width: btn_width,
            height: btn_height,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: cancel_x + (btn_width - 42.0) / 2.0,
            y: y + 8.0,
            text: String::from("Cancel"),
            color: COLOR_RED,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // --- Private hit-test helpers ---

    fn hit_test_presets(&self, x: f32, y: f32) -> Option<Color> {
        let sv_size = self.picker.sv_size;
        let alpha_y = self.alpha_bar_y();
        let slider_y = alpha_y + ALPHA_BAR_HEIGHT + PADDING * 2.0;
        let preset_y = slider_y + 28.0 + (SLIDER_HEIGHT + PADDING) * 3.0 + PADDING + 14.0;
        let cols = self.preset_columns(sv_size + HUE_BAR_WIDTH + PADDING * 4.0 + PREVIEW_SIZE);

        for (i, color) in PRESET_COLORS.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let sx = PADDING + col as f32 * (SWATCH_SIZE + SWATCH_GAP);
            let sy = preset_y + row as f32 * (SWATCH_SIZE + SWATCH_GAP);

            if x >= sx && x <= sx + SWATCH_SIZE && y >= sy && y <= sy + SWATCH_SIZE {
                return Some(*color);
            }
        }
        None
    }

    fn hit_test_recent(&self, x: f32, y: f32) -> Option<Color> {
        if self.picker.recent_colors.is_empty() {
            return None;
        }

        let alpha_y = self.alpha_bar_y();
        let slider_y = alpha_y + ALPHA_BAR_HEIGHT + PADDING * 2.0;
        let preset_y = slider_y + 28.0 + (SLIDER_HEIGHT + PADDING) * 3.0 + PADDING;
        let recent_y = preset_y + self.preset_palette_height() + PADDING + 14.0;

        for (i, color) in self.picker.recent_colors.iter().enumerate() {
            let sx = PADDING + i as f32 * (SWATCH_SIZE + SWATCH_GAP);
            let sy = recent_y;
            if x >= sx && x <= sx + SWATCH_SIZE && y >= sy && y <= sy + SWATCH_SIZE {
                return Some(*color);
            }
        }
        None
    }

    fn hit_test_eyedropper(&self, x: f32, y: f32) -> bool {
        let sv_size = self.picker.sv_size;
        let content_y = 36.0;
        let right_x = PADDING + sv_size + PADDING + HUE_BAR_WIDTH + PADDING * 2.0;
        let hex_y = content_y + PADDING + PREVIEW_SIZE + PADDING * 2.0 + 16.0;
        let eye_y = hex_y + 32.0;

        x >= right_x && x <= right_x + 90.0 && y >= eye_y && y <= eye_y + 24.0
    }

    fn hit_test_slider_tab(&self, x: f32, local_y: f32, tab: SliderTab) -> bool {
        let alpha_y = self.alpha_bar_y();
        let slider_y = alpha_y + ALPHA_BAR_HEIGHT + PADDING * 2.0;

        let tab_x = match tab {
            SliderTab::Rgb => PADDING,
            SliderTab::Hsv => PADDING + 46.0,
        };
        let tab_width = 40.0;
        let tab_height = 22.0;

        x >= tab_x && x <= tab_x + tab_width && local_y >= slider_y && local_y <= slider_y + tab_height
    }

    fn hit_test_sliders(&self, x: f32, local_y: f32) -> Option<DragTarget> {
        let alpha_y = self.alpha_bar_y();
        let slider_y = alpha_y + ALPHA_BAR_HEIGHT + PADDING * 2.0 + 28.0;
        let sv_size = self.picker.sv_size;
        let label_width = 20.0;
        let track_x = PADDING + label_width;
        let value_width = 30.0;
        let track_width = sv_size - label_width - value_width - PADDING;

        for i in 0..3u8 {
            let sy = slider_y + (SLIDER_HEIGHT + PADDING) * i as f32;
            if local_y >= sy && local_y <= sy + SLIDER_HEIGHT && x >= track_x && x <= track_x + track_width {
                return match self.slider_tab {
                    SliderTab::Rgb => Some(DragTarget::RgbSlider(i)),
                    SliderTab::Hsv => Some(DragTarget::HsvSlider(i)),
                };
            }
        }
        None
    }

    fn apply_slider_drag(&mut self, mouse_x: f32) {
        let sv_size = self.picker.sv_size;
        let label_width = 20.0;
        let track_x = PADDING + label_width;
        let value_width = 30.0;
        let track_width = sv_size - label_width - value_width - PADDING;
        let t = ((mouse_x - track_x) / track_width).clamp(0.0, 1.0);

        match self.picker.drag {
            Some(DragTarget::RgbSlider(ch)) => {
                let val = (t * 255.0 + 0.5) as u8;
                let (mut r, mut g, mut b) = hsv_to_rgb(self.picker.hsv);
                match ch {
                    0 => r = val,
                    1 => g = val,
                    _ => b = val,
                }
                self.picker.hsv = rgb_to_hsv(r, g, b);
            }
            Some(DragTarget::HsvSlider(comp)) => match comp {
                0 => self.picker.hsv.h = t * 360.0,
                1 => self.picker.hsv.s = t,
                _ => self.picker.hsv.v = t,
            },
            _ => {}
        }
    }

    // --- Layout helpers ---

    fn alpha_bar_y(&self) -> f32 {
        let content_y = 36.0;
        content_y + PADDING + self.picker.sv_size + PADDING
    }

    fn preset_columns(&self, available_width: f32) -> usize {
        let cols = ((available_width + SWATCH_GAP) / (SWATCH_SIZE + SWATCH_GAP)) as usize;
        cols.max(1)
    }

    fn preset_palette_height(&self) -> f32 {
        let cols = self.preset_columns(
            self.picker.sv_size + HUE_BAR_WIDTH + PADDING * 4.0 + PREVIEW_SIZE,
        );
        let rows = (PRESET_COLORS.len() + cols - 1) / cols;
        14.0 + rows as f32 * (SWATCH_SIZE + SWATCH_GAP)
    }
}

// ============================================================================
// Utility rendering helpers
// ============================================================================

/// Render a checkerboard pattern (used as background for alpha preview).
fn render_checkerboard(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, height: f32) {
    let cell = 6.0;
    // Light background
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height,
        color: Color::from_hex(0xCCCCCC),
        corner_radii: CornerRadii::ZERO,
    });
    // Dark cells
    let cols = (width / cell) as u32 + 1;
    let rows = (height / cell) as u32 + 1;
    for row in 0..rows {
        for col in 0..cols {
            if (row + col) % 2 == 1 {
                let cx = x + col as f32 * cell;
                let cy = y + row as f32 * cell;
                let cw = cell.min(x + width - cx);
                let ch = cell.min(y + height - cy);
                if cw > 0.0 && ch > 0.0 {
                    cmds.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: cw,
                        height: ch,
                        color: Color::from_hex(0x999999),
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- HSV ↔ RGB conversion tests ---

    #[test]
    fn test_hsv_to_rgb_red() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(0.0, 1.0, 1.0));
        assert_eq!((r, g, b), (255, 0, 0));
    }

    #[test]
    fn test_hsv_to_rgb_green() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(120.0, 1.0, 1.0));
        assert_eq!((r, g, b), (0, 255, 0));
    }

    #[test]
    fn test_hsv_to_rgb_blue() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(240.0, 1.0, 1.0));
        assert_eq!((r, g, b), (0, 0, 255));
    }

    #[test]
    fn test_hsv_to_rgb_white() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(0.0, 0.0, 1.0));
        assert_eq!((r, g, b), (255, 255, 255));
    }

    #[test]
    fn test_hsv_to_rgb_black() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(0.0, 0.0, 0.0));
        assert_eq!((r, g, b), (0, 0, 0));
    }

    #[test]
    fn test_hsv_to_rgb_yellow() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(60.0, 1.0, 1.0));
        assert_eq!((r, g, b), (255, 255, 0));
    }

    #[test]
    fn test_hsv_to_rgb_cyan() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(180.0, 1.0, 1.0));
        assert_eq!((r, g, b), (0, 255, 255));
    }

    #[test]
    fn test_hsv_to_rgb_magenta() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(300.0, 1.0, 1.0));
        assert_eq!((r, g, b), (255, 0, 255));
    }

    #[test]
    fn test_hsv_to_rgb_half_saturation() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(0.0, 0.5, 1.0));
        assert_eq!((r, g, b), (255, 128, 128));
    }

    #[test]
    fn test_hsv_to_rgb_half_value() {
        let (r, g, b) = hsv_to_rgb(Hsv::new(0.0, 1.0, 0.5));
        assert_eq!((r, g, b), (128, 0, 0));
    }

    #[test]
    fn test_rgb_to_hsv_red() {
        let hsv = rgb_to_hsv(255, 0, 0);
        assert!((hsv.h - 0.0).abs() < 0.01);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rgb_to_hsv_green() {
        let hsv = rgb_to_hsv(0, 255, 0);
        assert!((hsv.h - 120.0).abs() < 0.01);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rgb_to_hsv_blue() {
        let hsv = rgb_to_hsv(0, 0, 255);
        assert!((hsv.h - 240.0).abs() < 0.01);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rgb_to_hsv_white() {
        let hsv = rgb_to_hsv(255, 255, 255);
        assert!((hsv.s - 0.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rgb_to_hsv_black() {
        let hsv = rgb_to_hsv(0, 0, 0);
        assert!((hsv.s - 0.0).abs() < 0.01);
        assert!((hsv.v - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_rgb_to_hsv_gray() {
        let hsv = rgb_to_hsv(128, 128, 128);
        assert!((hsv.s - 0.0).abs() < 0.01);
        assert!((hsv.v - 128.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn test_hsv_rgb_roundtrip() {
        // Test that converting RGB → HSV → RGB preserves the original values.
        let test_cases: &[(u8, u8, u8)] = &[
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (255, 255, 0),
            (0, 255, 255),
            (255, 0, 255),
            (128, 64, 192),
            (10, 200, 50),
            (255, 255, 255),
            (0, 0, 0),
            (100, 100, 100),
        ];

        for &(r, g, b) in test_cases {
            let hsv = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(hsv);
            assert!(
                (r as i16 - r2 as i16).unsigned_abs() <= 1
                    && (g as i16 - g2 as i16).unsigned_abs() <= 1
                    && (b as i16 - b2 as i16).unsigned_abs() <= 1,
                "Roundtrip failed for ({r}, {g}, {b}) -> HSV({:.1}, {:.3}, {:.3}) -> ({r2}, {g2}, {b2})",
                hsv.h, hsv.s, hsv.v,
            );
        }
    }

    // --- Hex parsing tests ---

    #[test]
    fn test_parse_hex_6_digits() {
        let c = parse_hex_color("FF8000").unwrap();
        assert_eq!(c, Color::rgba(255, 128, 0, 255));
    }

    #[test]
    fn test_parse_hex_with_hash() {
        let c = parse_hex_color("#00FF00").unwrap();
        assert_eq!(c, Color::rgba(0, 255, 0, 255));
    }

    #[test]
    fn test_parse_hex_8_digits_with_alpha() {
        let c = parse_hex_color("#FF000080").unwrap();
        assert_eq!(c, Color::rgba(255, 0, 0, 128));
    }

    #[test]
    fn test_parse_hex_3_digits_shorthand() {
        let c = parse_hex_color("F00").unwrap();
        assert_eq!(c, Color::rgba(255, 0, 0, 255));
    }

    #[test]
    fn test_parse_hex_3_digits_gray() {
        let c = parse_hex_color("#888").unwrap();
        assert_eq!(c, Color::rgba(0x88, 0x88, 0x88, 255));
    }

    #[test]
    fn test_parse_hex_invalid_length() {
        assert_eq!(parse_hex_color("12345"), None);
        assert_eq!(parse_hex_color("1234567890"), None);
    }

    #[test]
    fn test_parse_hex_invalid_chars() {
        assert_eq!(parse_hex_color("GGHHII"), None);
        assert_eq!(parse_hex_color("xyz123"), None);
    }

    #[test]
    fn test_parse_hex_lowercase() {
        let c = parse_hex_color("ff8040").unwrap();
        assert_eq!(c, Color::rgba(255, 128, 64, 255));
    }

    #[test]
    fn test_parse_hex_with_whitespace() {
        let c = parse_hex_color("  #AABBCC  ").unwrap();
        assert_eq!(c, Color::rgba(0xAA, 0xBB, 0xCC, 255));
    }

    // --- color_to_hex_string tests ---

    #[test]
    fn test_color_to_hex_string() {
        assert_eq!(color_to_hex_string(Color::rgb(255, 128, 0)), "FF8000");
        assert_eq!(color_to_hex_string(Color::rgb(0, 0, 0)), "000000");
        assert_eq!(color_to_hex_string(Color::rgb(255, 255, 255)), "FFFFFF");
    }

    #[test]
    fn test_color_to_hex_string_alpha() {
        assert_eq!(
            color_to_hex_string_alpha(Color::rgba(255, 0, 0, 128)),
            "FF000080"
        );
    }

    // --- ColorPicker state tests ---

    #[test]
    fn test_picker_initial_color() {
        let picker = ColorPicker::new(Color::rgb(255, 0, 0));
        let c = picker.current_color();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn test_picker_set_color() {
        let mut picker = ColorPicker::new(Color::BLACK);
        picker.set_color(Color::rgb(0, 128, 255));
        let c = picker.current_color();
        assert_eq!(c.r, 0);
        // Allow +/- 1 for rounding
        assert!((c.g as i16 - 128).unsigned_abs() <= 1);
        assert_eq!(c.b, 255);
    }

    #[test]
    fn test_picker_alpha_preserved() {
        let mut picker = ColorPicker::new(Color::rgba(100, 150, 200, 128));
        assert_eq!(picker.alpha(), 128);
        picker.set_alpha(64);
        assert_eq!(picker.alpha(), 64);
        assert_eq!(picker.current_color().a, 64);
    }

    #[test]
    fn test_picker_recent_colors() {
        let mut picker = ColorPicker::new(Color::BLACK);
        picker.set_color(Color::rgb(255, 0, 0));
        picker.commit_to_recent();
        picker.set_color(Color::rgb(0, 255, 0));
        picker.commit_to_recent();

        assert_eq!(picker.recent_colors().len(), 2);
        assert_eq!(picker.recent_colors()[0], Color::rgb(0, 255, 0));
        assert_eq!(picker.recent_colors()[1], Color::rgb(255, 0, 0));
    }

    #[test]
    fn test_picker_recent_colors_deduplication() {
        let mut picker = ColorPicker::new(Color::BLACK);
        picker.set_color(Color::rgb(255, 0, 0));
        picker.commit_to_recent();
        picker.set_color(Color::rgb(0, 255, 0));
        picker.commit_to_recent();
        // Commit red again — should move to front, not duplicate.
        picker.set_color(Color::rgb(255, 0, 0));
        picker.commit_to_recent();

        assert_eq!(picker.recent_colors().len(), 2);
        assert_eq!(picker.recent_colors()[0], Color::rgb(255, 0, 0));
        assert_eq!(picker.recent_colors()[1], Color::rgb(0, 255, 0));
    }

    #[test]
    fn test_picker_recent_colors_max_capacity() {
        let mut picker = ColorPicker::new(Color::BLACK);
        for i in 0..20u8 {
            picker.set_color(Color::rgb(i, i, i));
            picker.commit_to_recent();
        }
        assert_eq!(picker.recent_colors().len(), MAX_RECENT_COLORS);
    }

    #[test]
    fn test_picker_hex_input_sync() {
        let mut picker = ColorPicker::new(Color::rgb(255, 128, 0));
        assert_eq!(picker.hex_input(), "FF8000");

        picker.set_color(Color::rgb(0, 0, 0));
        assert_eq!(picker.hex_input(), "000000");
    }

    #[test]
    fn test_picker_eyedropper_mode() {
        let mut picker = ColorPicker::new(Color::BLACK);
        assert_eq!(picker.mode(), PickerMode::Normal);

        picker.activate_eyedropper();
        assert_eq!(picker.mode(), PickerMode::Eyedropper);

        picker.cancel_eyedropper();
        assert_eq!(picker.mode(), PickerMode::Normal);
    }

    #[test]
    fn test_picker_eyedropper_result() {
        let mut picker = ColorPicker::new(Color::BLACK);
        picker.activate_eyedropper();
        picker.set_eyedropper_result(Color::rgb(42, 84, 168));
        assert_eq!(picker.mode(), PickerMode::Normal);
        let c = picker.current_color();
        assert!((c.r as i16 - 42).unsigned_abs() <= 1);
        assert!((c.g as i16 - 84).unsigned_abs() <= 1);
        assert!((c.b as i16 - 168).unsigned_abs() <= 1);
    }

    // --- ColorPickerDialog tests ---

    #[test]
    fn test_dialog_creation() {
        let dialog = ColorPickerDialog::new(Color::rgb(100, 200, 50));
        assert!(!dialog.is_confirmed());
        assert!(!dialog.is_cancelled());
        assert_eq!(dialog.slider_tab(), SliderTab::Rgb);
    }

    #[test]
    fn test_dialog_escape_cancels() {
        let mut dialog = ColorPickerDialog::new(Color::rgb(100, 100, 100));
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: None,
        };
        let result = dialog.handle_key(&event);
        assert_eq!(result, Some(ColorPickerEvent::Cancelled));
        assert!(dialog.is_cancelled());
    }

    #[test]
    fn test_dialog_enter_confirms() {
        let mut dialog = ColorPickerDialog::new(Color::rgb(100, 100, 100));
        let event = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: None,
        };
        let result = dialog.handle_key(&event);
        assert!(matches!(result, Some(ColorPickerEvent::Confirmed(_))));
        assert!(dialog.is_confirmed());
    }

    #[test]
    fn test_dialog_tab_switching() {
        let mut dialog = ColorPickerDialog::new(Color::BLACK);
        assert_eq!(dialog.slider_tab(), SliderTab::Rgb);
        dialog.set_slider_tab(SliderTab::Hsv);
        assert_eq!(dialog.slider_tab(), SliderTab::Hsv);
    }

    #[test]
    fn test_dialog_render_produces_commands() {
        let dialog = ColorPickerDialog::new(Color::rgb(128, 64, 200));
        let cmds = dialog.render(400.0, 600.0);
        assert!(!cmds.is_empty());
        // Should have at least background + title + SV square cells
        assert!(cmds.len() > 50);
    }

    #[test]
    fn test_compact_picker_render() {
        let picker = ColorPicker::compact(Color::rgb(200, 100, 50));
        let cmds = picker.render(10.0, 10.0);
        assert!(!cmds.is_empty());
    }

    // --- Edge case tests ---

    #[test]
    fn test_hsv_hue_wraparound() {
        // Hue at 360 should be equivalent to hue at 0
        let (r1, g1, b1) = hsv_to_rgb(Hsv::new(360.0, 1.0, 1.0));
        let (r2, g2, b2) = hsv_to_rgb(Hsv::new(0.0, 1.0, 1.0));
        assert_eq!((r1, g1, b1), (r2, g2, b2));
    }

    #[test]
    fn test_hsv_negative_hue() {
        // Negative hue should wrap correctly
        let (r, g, b) = hsv_to_rgb(Hsv::new(-60.0, 1.0, 1.0));
        let (r2, g2, b2) = hsv_to_rgb(Hsv::new(300.0, 1.0, 1.0));
        assert_eq!((r, g, b), (r2, g2, b2));
    }

    #[test]
    fn test_hsv_clamping() {
        // Out-of-range saturation/value should be clamped
        let (r, g, b) = hsv_to_rgb(Hsv::new(0.0, 1.5, 2.0));
        // Clamped to s=1, v=1 → pure red
        assert_eq!((r, g, b), (255, 0, 0));
    }

    #[test]
    fn test_preset_colors_valid() {
        // All preset colors should have alpha=255
        for color in PRESET_COLORS {
            assert_eq!(color.a, 255);
        }
    }

    #[test]
    fn test_preset_colors_count() {
        assert_eq!(PRESET_COLORS.len(), 48);
    }
}
