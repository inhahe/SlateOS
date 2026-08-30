//! Slate OS Color Picker — system-wide color picker / eyedropper utility.
//!
//! A PowerToys-style color picker that allows picking colors from the screen,
//! converting between formats (Hex, RGB, HSL, HSV, CMYK), managing palettes,
//! tracking color history, suggesting harmonies, and checking WCAG contrast.
//!
//! Renders via guitk into a 600x700 window using the Catppuccin Mocha dark theme.
//!
//! Drawing and hit-testing are the *same walk*: the renderer records a
//! rectangle for every control it draws (see [`guitk::frame`]), so a control
//! that moved cannot be clicked where it used to be. That matters more here
//! than in most programs, because almost everything on screen is laid out by
//! running `y` down the window rather than at a fixed coordinate.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

mod mocha {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
}

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 600.0;
const WINDOW_HEIGHT: f32 = 700.0;
const PADDING: f32 = 12.0;
const CORNER_RADIUS: f32 = 6.0;
const SMALL_RADIUS: f32 = 4.0;
const SWATCH_SIZE: f32 = 120.0;
const SLIDER_HEIGHT: f32 = 24.0;
const SLIDER_TRACK_HEIGHT: f32 = 8.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_LARGE: f32 = 16.0;
const TAB_HEIGHT: f32 = 32.0;
const HISTORY_CELL: f32 = 28.0;
const HISTORY_GAP: f32 = 4.0;
const PALETTE_CELL: f32 = 32.0;
const PALETTE_GAP: f32 = 4.0;
const MAX_HISTORY: usize = 50;
const CONTRAST_PANEL_HEIGHT: f32 = 80.0;

// ============================================================================
// Controls
// ============================================================================

/// Which number a slider drags.
///
/// R/G/B and H/S/L are one enum rather than two because dragging is the same
/// operation for all six — take a fraction of the track, scale it, write it
/// back — and splitting them would mean two copies of that arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    R,
    G,
    B,
    H,
    S,
    L,
}

impl Channel {
    /// The label drawn to the left of the track.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::G => "G",
            Self::B => "B",
            Self::H => "H",
            Self::S => "S",
            Self::L => "L",
        }
    }

    /// The largest value this channel takes.
    ///
    /// 255 for the byte channels, 360 for a hue in degrees, and 1 for the
    /// two that are fractions — which is also how the slider decides whether
    /// to print `128` or `50%`.
    #[must_use]
    pub fn max(self) -> f32 {
        match self {
            Self::R | Self::G | Self::B => 255.0,
            Self::H => 360.0,
            Self::S | Self::L => 1.0,
        }
    }

    /// How far one arrow-key press moves this channel.
    ///
    /// One unit for the byte channels and for hue — the smallest change the
    /// value can actually show — and one percentage point for the fractions,
    /// because `0.01` is what the readout is rounded to and a smaller step
    /// would look like the key did nothing.
    #[must_use]
    pub fn step(self) -> f32 {
        match self {
            Self::R | Self::G | Self::B | Self::H => 1.0,
            Self::S | Self::L => 0.01,
        }
    }
}

/// A control the pointer can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The eyedropper toggle in the title bar.
    Eyedropper,
    /// The Copy button beside the swatch.
    CopyButton,
    /// One of the format tabs.
    FormatTab(ColorFormat),
    /// The value box under the tabs, which doubles as the hex entry field.
    ValueBox,
    /// A slider track. A press anywhere along it jumps the value there —
    /// clicking a track and having only the thumb respond is a control that
    /// looks broken until you find the thumb.
    Slider(Channel),
    /// One of the harmony suggestions, by position.
    Harmony(usize),
    /// The foreground/background swap in the contrast panel.
    SwapContrast,
    /// A swatch in the Recent strip, by position.
    Recent(usize),
    /// A swatch in the palette grid, by position.
    Swatch(usize),
}

/// A frame being drawn. See [`guitk::frame`] for why drawing and hit-testing
/// are the same walk.
pub type Frame = guitk::frame::Frame<Target>;

/// What a handled event asks the window to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing changed; do not repaint.
    None,
    /// State changed; repaint.
    Redraw,
    /// Close the window.
    Quit,
}

// ============================================================================
// Color format enum
// ============================================================================

/// Supported output color formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorFormat {
    Hex,
    Rgb,
    Hsl,
    Hsv,
    Cmyk,
}

impl ColorFormat {
    /// All formats in display order.
    pub const ALL: &[ColorFormat] = &[
        ColorFormat::Hex,
        ColorFormat::Rgb,
        ColorFormat::Hsl,
        ColorFormat::Hsv,
        ColorFormat::Cmyk,
    ];

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hex => "HEX",
            Self::Rgb => "RGB",
            Self::Hsl => "HSL",
            Self::Hsv => "HSV",
            Self::Cmyk => "CMYK",
        }
    }
}

// ============================================================================
// HSL type
// ============================================================================

/// Hue/Saturation/Lightness representation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsl {
    /// Hue in degrees [0, 360).
    pub h: f32,
    /// Saturation [0, 1].
    pub s: f32,
    /// Lightness [0, 1].
    pub l: f32,
}

// ============================================================================
// HSV type
// ============================================================================

/// Hue/Saturation/Value representation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    /// Hue in degrees [0, 360).
    pub h: f32,
    /// Saturation [0, 1].
    pub s: f32,
    /// Value [0, 1].
    pub v: f32,
}

// ============================================================================
// CMYK type
// ============================================================================

/// Cyan/Magenta/Yellow/Key (black) representation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cmyk {
    /// Cyan [0, 1].
    pub c: f32,
    /// Magenta [0, 1].
    pub m: f32,
    /// Yellow [0, 1].
    pub y: f32,
    /// Key (black) [0, 1].
    pub k: f32,
}

// ============================================================================
// PickedColor — the core color type with conversions
// ============================================================================

/// A picked color with RGBA components and conversion to all supported formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PickedColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl PickedColor {
    /// Create from RGBA components.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Create from RGB with full opacity.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Create from a guitk `Color`.
    pub const fn from_color(c: Color) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
            a: c.a,
        }
    }

    /// Convert to a guitk `Color`.
    pub const fn to_color(self) -> Color {
        Color::rgba(self.r, self.g, self.b, self.a)
    }

    // -- Hex conversion ----------------------------------------------------

    /// Format as `#RRGGBB` hex string.
    pub fn to_hex6(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Format as `#RRGGBBAA` hex string (includes alpha).
    pub fn to_hex8(self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
    }

    /// Format as the selected `ColorFormat`.
    pub fn format_as(self, fmt: ColorFormat) -> String {
        match fmt {
            ColorFormat::Hex => {
                if self.a == 255 {
                    self.to_hex6()
                } else {
                    self.to_hex8()
                }
            }
            ColorFormat::Rgb => {
                if self.a == 255 {
                    format!("rgb({}, {}, {})", self.r, self.g, self.b)
                } else {
                    format!("rgba({}, {}, {}, {})", self.r, self.g, self.b, self.a)
                }
            }
            ColorFormat::Hsl => {
                let hsl = self.to_hsl();
                format!(
                    "hsl({:.0}, {:.1}%, {:.1}%)",
                    hsl.h,
                    hsl.s * 100.0,
                    hsl.l * 100.0
                )
            }
            ColorFormat::Hsv => {
                let hsv = self.to_hsv();
                format!(
                    "hsv({:.0}, {:.1}%, {:.1}%)",
                    hsv.h,
                    hsv.s * 100.0,
                    hsv.v * 100.0
                )
            }
            ColorFormat::Cmyk => {
                let cmyk = self.to_cmyk();
                format!(
                    "cmyk({:.1}%, {:.1}%, {:.1}%, {:.1}%)",
                    cmyk.c * 100.0,
                    cmyk.m * 100.0,
                    cmyk.y * 100.0,
                    cmyk.k * 100.0
                )
            }
        }
    }

    /// Parse a hex color string. Accepts `#RGB`, `#RRGGBB`, `#RRGGBBAA` (with
    /// or without leading `#`).
    pub fn from_hex_str(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        match s.len() {
            3 => {
                // #RGB -> #RRGGBB
                let r = u8::from_str_radix(&s[0..1], 16).ok()?;
                let g = u8::from_str_radix(&s[1..2], 16).ok()?;
                let b = u8::from_str_radix(&s[2..3], 16).ok()?;
                // `r`, `g` and `b` are single hex digits, so each is at most
                // 15 and `15 * 17` is exactly 255 — the shorthand's whole
                // point. Saturating rather than asserting that, because a
                // wrapped channel would be a silently wrong colour.
                Some(Self::from_rgb(
                    r.saturating_mul(17),
                    g.saturating_mul(17),
                    b.saturating_mul(17),
                ))
            }
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self::from_rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                let a = u8::from_str_radix(&s[6..8], 16).ok()?;
                Some(Self::new(r, g, b, a))
            }
            _ => None,
        }
    }

    // -- RGB <-> HSL -------------------------------------------------------

    /// Convert to HSL.
    pub fn to_hsl(self) -> Hsl {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let l = f32::midpoint(max, min);

        if delta < f32::EPSILON {
            return Hsl { h: 0.0, s: 0.0, l };
        }

        let s = if l <= 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        let h = if (max - r).abs() < f32::EPSILON {
            let mut hue = (g - b) / delta;
            if hue < 0.0 {
                hue += 6.0;
            }
            hue * 60.0
        } else if (max - g).abs() < f32::EPSILON {
            ((b - r) / delta + 2.0) * 60.0
        } else {
            ((r - g) / delta + 4.0) * 60.0
        };

        Hsl { h, s, l }
    }

    /// Create from HSL values (h in [0,360), s and l in [0,1]).
    pub fn from_hsl(hsl: Hsl) -> Self {
        let Hsl { h, s, l } = hsl;
        if s < f32::EPSILON {
            let v = (l * 255.0).round() as u8;
            return Self::from_rgb(v, v, v);
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let h_norm = h / 360.0;

        let r = hue_to_rgb(p, q, h_norm + 1.0 / 3.0);
        let g = hue_to_rgb(p, q, h_norm);
        let b = hue_to_rgb(p, q, h_norm - 1.0 / 3.0);

        Self::from_rgb(
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        )
    }

    // -- RGB <-> HSV -------------------------------------------------------

    /// Convert to HSV.
    pub fn to_hsv(self) -> Hsv {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let v = max;
        let s = if max < f32::EPSILON { 0.0 } else { delta / max };

        if delta < f32::EPSILON {
            return Hsv { h: 0.0, s: 0.0, v };
        }

        let h = if (max - r).abs() < f32::EPSILON {
            let mut hue = (g - b) / delta;
            if hue < 0.0 {
                hue += 6.0;
            }
            hue * 60.0
        } else if (max - g).abs() < f32::EPSILON {
            ((b - r) / delta + 2.0) * 60.0
        } else {
            ((r - g) / delta + 4.0) * 60.0
        };

        Hsv { h, s, v }
    }

    /// Create from HSV values (h in [0,360), s and v in [0,1]).
    pub fn from_hsv(hsv: Hsv) -> Self {
        let Hsv { h, s, v } = hsv;
        if s < f32::EPSILON {
            let val = (v * 255.0).round() as u8;
            return Self::from_rgb(val, val, val);
        }

        let h_sector = h / 60.0;
        let i = h_sector.floor() as u32;
        let f = h_sector - i as f32;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));

        let (r, g, b) = match i % 6 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };

        Self::from_rgb(
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        )
    }

    // -- RGB -> CMYK -------------------------------------------------------

    /// Convert to CMYK.
    pub fn to_cmyk(self) -> Cmyk {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;

        let k = 1.0 - r.max(g).max(b);

        if (k - 1.0).abs() < f32::EPSILON {
            // Pure black — avoid division by zero.
            return Cmyk {
                c: 0.0,
                m: 0.0,
                y: 0.0,
                k: 1.0,
            };
        }

        let inv_k = 1.0 - k;
        Cmyk {
            c: (1.0 - r - k) / inv_k,
            m: (1.0 - g - k) / inv_k,
            y: (1.0 - b - k) / inv_k,
            k,
        }
    }

    /// Create from CMYK values (all in [0,1]).
    pub fn from_cmyk(cmyk: Cmyk) -> Self {
        let Cmyk { c, m, y, k } = cmyk;
        let inv_k = 1.0 - k;
        let r = ((1.0 - c) * inv_k * 255.0).round() as u8;
        let g = ((1.0 - m) * inv_k * 255.0).round() as u8;
        let b = ((1.0 - y) * inv_k * 255.0).round() as u8;
        Self::from_rgb(r, g, b)
    }

    // -- Relative luminance (WCAG 2.x) ------------------------------------

    /// Relative luminance per WCAG 2.x spec.
    /// Returns a value in [0, 1] where 0 = darkest and 1 = lightest.
    pub fn relative_luminance(self) -> f64 {
        fn linearize(channel: u8) -> f64 {
            let c = channel as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }

        let r = linearize(self.r);
        let g = linearize(self.g);
        let b = linearize(self.b);

        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    // -- Color harmony suggestions -----------------------------------------

    /// Complementary color (180 degrees opposite on the hue wheel).
    pub fn complementary(self) -> Self {
        let mut hsl = self.to_hsl();
        hsl.h = (hsl.h + 180.0) % 360.0;
        Self::from_hsl(hsl)
    }

    /// Analogous colors (30 degrees to each side).
    pub fn analogous(self) -> (Self, Self) {
        let hsl = self.to_hsl();
        let mut left = hsl;
        left.h = (hsl.h + 330.0) % 360.0; // -30
        let mut right = hsl;
        right.h = (hsl.h + 30.0) % 360.0;
        (Self::from_hsl(left), Self::from_hsl(right))
    }

    /// Triadic colors (120 degrees apart).
    pub fn triadic(self) -> (Self, Self) {
        let hsl = self.to_hsl();
        let mut a = hsl;
        a.h = (hsl.h + 120.0) % 360.0;
        let mut b = hsl;
        b.h = (hsl.h + 240.0) % 360.0;
        (Self::from_hsl(a), Self::from_hsl(b))
    }
}

/// Helper for HSL -> RGB conversion. Converts a single channel given p, q, and
/// the shifted hue.
fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

// ============================================================================
// Contrast ratio (WCAG)
// ============================================================================

/// Calculate the WCAG contrast ratio between two colors.
/// Returns a ratio >= 1.0 (e.g. 4.5 for AA normal text, 7.0 for AAA).
pub fn contrast_ratio(fg: PickedColor, bg: PickedColor) -> f64 {
    let l1 = fg.relative_luminance();
    let l2 = bg.relative_luminance();
    let lighter = l1.max(l2);
    let darker = l1.min(l2);
    (lighter + 0.05) / (darker + 0.05)
}

/// WCAG compliance level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WcagLevel {
    /// Fails both AA and AAA for normal text.
    Fail,
    /// Passes AA for large text only (ratio >= 3.0).
    AaLarge,
    /// Passes AA for normal text (ratio >= 4.5).
    Aa,
    /// Passes AAA for normal text (ratio >= 7.0).
    Aaa,
}

impl WcagLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fail => "Fail",
            Self::AaLarge => "AA Large",
            Self::Aa => "AA",
            Self::Aaa => "AAA",
        }
    }
}

/// Evaluate the WCAG compliance level for a contrast ratio.
pub fn wcag_level(ratio: f64) -> WcagLevel {
    if ratio >= 7.0 {
        WcagLevel::Aaa
    } else if ratio >= 4.5 {
        WcagLevel::Aa
    } else if ratio >= 3.0 {
        WcagLevel::AaLarge
    } else {
        WcagLevel::Fail
    }
}

// ============================================================================
// ColorPalette
// ============================================================================

/// A named collection of saved colors.
#[derive(Clone, Debug)]
pub struct ColorPalette {
    pub name: String,
    pub colors: Vec<(String, PickedColor)>,
}

impl ColorPalette {
    /// Create an empty palette with the given name.
    pub fn with_name(name: &str) -> Self {
        Self {
            name: name.to_string(),
            colors: Vec::new(),
        }
    }

    /// Add a named color. Returns `false` if the name is already taken.
    pub fn add(&mut self, name: &str, color: PickedColor) -> bool {
        if self.colors.iter().any(|(n, _)| n == name) {
            return false;
        }
        self.colors.push((name.to_string(), color));
        true
    }

    /// Remove a color by name. Returns `true` if found and removed.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.colors.len();
        self.colors.retain(|(n, _)| n != name);
        self.colors.len() < before
    }

    /// Rename a color entry. Returns `false` if the old name was not found or
    /// the new name is already taken.
    pub fn rename(&mut self, old_name: &str, new_name: &str) -> bool {
        if self.colors.iter().any(|(n, _)| n == new_name) {
            return false;
        }
        for (name, _) in &mut self.colors {
            if name == old_name {
                *name = new_name.to_string();
                return true;
            }
        }
        false
    }

    /// Look up a color by name.
    pub fn get(&self, name: &str) -> Option<PickedColor> {
        self.colors.iter().find(|(n, _)| n == name).map(|(_, c)| *c)
    }

    /// Number of colors in the palette.
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// Whether the palette is empty.
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

// ============================================================================
// ColorHistory — circular buffer of recent picks
// ============================================================================

/// Circular buffer of recently picked colors, newest first.
#[derive(Clone, Debug)]
pub struct ColorHistory {
    entries: Vec<PickedColor>,
    capacity: usize,
}

impl ColorHistory {
    /// Create a history with the given max capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a color into the history. If the color already exists, it is moved
    /// to the front. If the history is at capacity, the oldest entry is dropped.
    pub fn push(&mut self, color: PickedColor) {
        // Remove duplicate if present.
        self.entries.retain(|c| c != &color);
        // Insert at front.
        self.entries.insert(0, color);
        // Trim to capacity.
        if self.entries.len() > self.capacity {
            self.entries.truncate(self.capacity);
        }
    }

    /// Get color at the given index (0 = most recent).
    pub fn get(&self, idx: usize) -> Option<&PickedColor> {
        self.entries.get(idx)
    }

    /// Number of entries currently stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterator over all entries, newest first.
    pub fn iter(&self) -> impl Iterator<Item = &PickedColor> {
        self.entries.iter()
    }

    /// Clear all history entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================================
// Eyedropper state
// ============================================================================

/// Eyedropper mode state. In a real OS this would capture the screen and let
/// the user click to pick a color. Here we simulate it with a stored coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyedropperState {
    /// Whether the eyedropper mode is active.
    pub active: bool,
    /// Last picked screen coordinate (x).
    pub pick_x: f32,
    /// Last picked screen coordinate (y).
    pub pick_y: f32,
}

impl Default for EyedropperState {
    fn default() -> Self {
        Self {
            active: false,
            pick_x: 0.0,
            pick_y: 0.0,
        }
    }
}

// ============================================================================
// Application state
// ============================================================================

/// Complete application state for the color picker.
pub struct ColorPickerApp {
    /// The currently selected/active color.
    current: PickedColor,
    /// Active format tab.
    active_format: ColorFormat,
    /// Color history (recent picks).
    history: ColorHistory,
    /// Named palettes.
    palettes: Vec<ColorPalette>,
    /// Index of the active palette.
    active_palette_idx: usize,
    /// Eyedropper state.
    eyedropper: EyedropperState,
    /// Background color for contrast checking.
    contrast_bg: PickedColor,
    /// Clipboard text (simulated).
    clipboard: String,
    /// Hex input buffer.
    hex_input: String,
    /// Whether the value box has keyboard focus and is taking hex digits.
    editing: bool,
    /// The slider the pointer is currently dragging, if any.
    ///
    /// Held for the whole gesture rather than re-hit-testing each move,
    /// because a drag that leaves the track — which every drag does, the
    /// moment the pointer runs past either end — must keep controlling the
    /// slider it started on rather than stopping dead or grabbing its
    /// neighbour.
    dragging: Option<Channel>,
    /// One line under the title saying what the last action did.
    status: String,
    /// The size the compositor last told us the window is.
    ///
    /// Only a *record* of the last size, never the authority on it: every
    /// frame is drawn at the size `render` is handed, because the first frame
    /// goes out before any `Event::Resize` arrives. This copy exists so that
    /// an event handler — which is given no size — can hit-test against the
    /// same layout the user is looking at.
    window_size: (f32, f32),
}

impl ColorPickerApp {
    /// Create a new color picker with default state.
    pub fn create() -> Self {
        let mut default_palette = ColorPalette::with_name("Default");
        // Pre-populate with a few common colors.
        let _ = default_palette.add("White", PickedColor::from_rgb(255, 255, 255));
        let _ = default_palette.add("Black", PickedColor::from_rgb(0, 0, 0));
        let _ = default_palette.add("Red", PickedColor::from_rgb(255, 0, 0));
        let _ = default_palette.add("Green", PickedColor::from_rgb(0, 255, 0));
        let _ = default_palette.add("Blue", PickedColor::from_rgb(0, 0, 255));
        let _ = default_palette.add("Yellow", PickedColor::from_rgb(255, 255, 0));
        let _ = default_palette.add("Cyan", PickedColor::from_rgb(0, 255, 255));
        let _ = default_palette.add("Magenta", PickedColor::from_rgb(255, 0, 255));

        Self {
            current: PickedColor::from_rgb(137, 180, 250), // Catppuccin Blue
            active_format: ColorFormat::Hex,
            history: ColorHistory::with_capacity(MAX_HISTORY),
            palettes: vec![default_palette],
            active_palette_idx: 0,
            eyedropper: EyedropperState::default(),
            contrast_bg: PickedColor::from_rgb(30, 30, 46), // Catppuccin Base
            clipboard: String::new(),
            hex_input: String::new(),
            editing: false,
            dragging: None,
            status: String::new(),
            window_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Set the current color and record it in history.
    pub fn set_color(&mut self, color: PickedColor) {
        self.current = color;
        self.history.push(color);
    }

    /// Copy the current colour, in the active format, to *this program's*
    /// clipboard.
    ///
    /// Not the system's: there is no way to reach the clipboard service yet —
    /// it is a binary with no library target and no IPC endpoint. See
    /// `known-issues.md` →
    /// `TD-C-NOTHING-CAN-ACTUALLY-COPY-AND-PASTE-BETWEEN-PROGRAMS`. When
    /// `guitk::clipboard` exists this becomes a call to it and the field goes
    /// away; until then the status line says "in this window" rather than
    /// letting the user believe the colour is waiting in another one.
    pub fn copy_to_clipboard(&mut self) {
        self.clipboard = self.current.format_as(self.active_format);
    }

    /// Try to apply a hex string typed by the user.
    pub fn apply_hex_input(&mut self) -> bool {
        if let Some(c) = PickedColor::from_hex_str(&self.hex_input) {
            self.set_color(c);
            true
        } else {
            false
        }
    }

    /// Save the current color to the active palette.
    pub fn save_to_palette(&mut self, name: &str) -> bool {
        if let Some(palette) = self.palettes.get_mut(self.active_palette_idx) {
            palette.add(name, self.current)
        } else {
            false
        }
    }

    /// Toggle eyedropper mode.
    pub fn toggle_eyedropper(&mut self) {
        self.eyedropper.active = !self.eyedropper.active;
    }

    /// Simulate picking a color at screen coordinates.
    pub fn eyedrop_pick(&mut self, x: f32, y: f32, color: PickedColor) {
        self.eyedropper.pick_x = x;
        self.eyedropper.pick_y = y;
        self.eyedropper.active = false;
        self.set_color(color);
    }

    /// Set the R component of the current color.
    pub fn set_r(&mut self, r: u8) {
        self.current.r = r;
    }

    /// Set the G component of the current color.
    pub fn set_g(&mut self, g: u8) {
        self.current.g = g;
    }

    /// Set the B component of the current color.
    pub fn set_b(&mut self, b: u8) {
        self.current.b = b;
    }

    /// Set the current color from HSL, preserving alpha.
    pub fn set_from_hsl(&mut self, hsl: Hsl) {
        let mut c = PickedColor::from_hsl(hsl);
        c.a = self.current.a;
        self.current = c;
    }

    // -- Derived colors ----------------------------------------------------

    /// The five harmony suggestions, in the order they are drawn.
    ///
    /// One function, called by the renderer and by the click handler, so the
    /// swatch that is clicked is the colour that is applied. Computing them
    /// twice would let the two drift the moment a sixth is added.
    #[must_use]
    pub fn harmonies(&self) -> [(&'static str, PickedColor); 5] {
        let (ana_l, ana_r) = self.analogous_pair();
        let (tri_a, tri_b) = self.triadic_pair();
        [
            ("Comp", self.current.complementary()),
            ("Ana-", ana_l),
            ("Ana+", ana_r),
            ("Tri1", tri_a),
            ("Tri2", tri_b),
        ]
    }

    fn analogous_pair(&self) -> (PickedColor, PickedColor) {
        self.current.analogous()
    }

    fn triadic_pair(&self) -> (PickedColor, PickedColor) {
        self.current.triadic()
    }

    // -- Channels ----------------------------------------------------------

    /// The current value of one slider's channel.
    #[must_use]
    pub fn channel(&self, channel: Channel) -> f32 {
        let hsl = self.current.to_hsl();
        match channel {
            Channel::R => f32::from(self.current.r),
            Channel::G => f32::from(self.current.g),
            Channel::B => f32::from(self.current.b),
            Channel::H => hsl.h,
            Channel::S => hsl.s,
            Channel::L => hsl.l,
        }
    }

    /// Write one channel, clamped to its own range.
    ///
    /// Setting an HSL channel goes through the full colour rather than
    /// storing H, S and L separately, which is why this is not symmetrical:
    /// RGB is what the colour *is*, and HSL is a view of it. The cost is that
    /// dragging S to zero loses the hue — grey has no hue to remember — and
    /// that is a property of the colour space, not a bug to paper over.
    pub fn set_channel(&mut self, channel: Channel, value: f32) {
        let v = value.clamp(0.0, channel.max());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = v.round().clamp(0.0, 255.0) as u8;
        match channel {
            Channel::R => self.current.r = byte,
            Channel::G => self.current.g = byte,
            Channel::B => self.current.b = byte,
            Channel::H => {
                let mut hsl = self.current.to_hsl();
                hsl.h = v;
                self.set_from_hsl(hsl);
            }
            Channel::S => {
                let mut hsl = self.current.to_hsl();
                hsl.s = v;
                self.set_from_hsl(hsl);
            }
            Channel::L => {
                let mut hsl = self.current.to_hsl();
                hsl.l = v;
                self.set_from_hsl(hsl);
            }
        }
    }

    /// Move one channel by `steps` of its own step size.
    fn nudge(&mut self, channel: Channel, steps: f32) -> Action {
        let before = self.channel(channel);
        self.set_channel(channel, channel.step().mul_add(steps, before));
        if (self.channel(channel) - before).abs() < f32::EPSILON {
            // Already at the end of the range. Repainting an unchanged window
            // on every held arrow key is work nobody can see.
            return Action::None;
        }
        self.status = format!("{} = {}", channel.label(), self.readout(channel));
        Action::Redraw
    }

    /// One channel as the slider prints it.
    fn readout(&self, channel: Channel) -> String {
        let v = self.channel(channel);
        if channel.max() > 1.0 {
            format!("{v:.0}")
        } else {
            format!("{:.0}%", v * 100.0)
        }
    }

    // -- Interaction -------------------------------------------------------

    /// Which control, if any, is under the pointer.
    ///
    /// Runs the renderer and reads its recorded boxes back, so a control that
    /// is not drawn cannot be clicked.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32, size: (f32, f32)) -> Option<Target> {
        self.frame(size.0, size.1).hit_test(x, y)
    }

    /// The rectangle a control is drawn at in a window of `size`.
    fn rect_of(&self, target: Target, size: (f32, f32)) -> Option<Rect> {
        self.frame(size.0, size.1).rect_of(|t| *t == target)
    }

    /// Set a slider from a pointer position along its track.
    fn drag_slider(&mut self, channel: Channel, x: f32, size: (f32, f32)) -> Action {
        let Some(row) = self.rect_of(Target::Slider(channel), size) else {
            return Action::None;
        };
        let track = Self::track_rect(row.x, row.y, row.w);
        if track.w <= 0.0 {
            return Action::None;
        }
        let frac = ((x - track.x) / track.w).clamp(0.0, 1.0);
        let before = self.channel(channel);
        self.set_channel(channel, frac * channel.max());
        if (self.channel(channel) - before).abs() < f32::EPSILON {
            return Action::None;
        }
        self.status = format!("{} = {}", channel.label(), self.readout(channel));
        Action::Redraw
    }

    /// Adopt `color` as the current one and say so.
    fn pick(&mut self, color: PickedColor, from: &str) -> Action {
        self.set_color(color);
        self.status = format!("{from} → {}", color.to_hex6());
        Action::Redraw
    }

    /// Do whatever `target` names. `x` is the pointer position, which only
    /// the sliders need.
    fn activate(&mut self, target: Target, x: f32, size: (f32, f32)) -> Action {
        // Any click outside the value box ends the edit. Leaving a
        // half-typed hex string in a focused-looking field while the user
        // drags a slider would make the next keystroke go somewhere they had
        // stopped looking.
        if target != Target::ValueBox && self.editing {
            self.cancel_edit();
        }
        match target {
            Target::Eyedropper => {
                self.toggle_eyedropper();
                self.status = if self.eyedropper.active {
                    // Honest about what it can do: there is no screen capture
                    // to sample from yet, so the mode arms and waits rather
                    // than claiming it is reading pixels.
                    String::from("Eyedropper armed — click anywhere to sample")
                } else {
                    String::from("Eyedropper off")
                };
                Action::Redraw
            }
            Target::CopyButton => {
                self.copy_to_clipboard();
                // "in this window" because that is the truth: no other
                // program can read this. Saying plain "Copied" would send
                // the user to paste somewhere and find nothing there.
                self.status = format!("Copied {} (in this window)", self.clipboard);
                Action::Redraw
            }
            Target::FormatTab(fmt) => {
                if self.active_format == fmt {
                    return Action::None;
                }
                self.active_format = fmt;
                self.status = format!("{} format", fmt.label());
                Action::Redraw
            }
            Target::ValueBox => {
                if self.editing {
                    return Action::None;
                }
                self.begin_edit();
                Action::Redraw
            }
            Target::Slider(channel) => {
                self.dragging = Some(channel);
                self.drag_slider(channel, x, size);
                // Redraw unconditionally: the press itself started a drag,
                // which is a state change even when the value did not move.
                Action::Redraw
            }
            Target::Harmony(i) => match self.harmonies().get(i) {
                Some((label, color)) => {
                    let (label, color) = (*label, *color);
                    self.pick(color, label)
                }
                None => Action::None,
            },
            Target::SwapContrast => {
                std::mem::swap(&mut self.current, &mut self.contrast_bg);
                // Deliberately not `set_color`: a swap is not a new pick, and
                // pushing both halves of every swap into Recent would fill
                // the strip with colours the user never chose.
                self.status = String::from("Swapped foreground and background");
                Action::Redraw
            }
            Target::Recent(i) => {
                let picked = self.history.iter().nth(i).copied();
                match picked {
                    Some(color) => self.pick(color, "Recent"),
                    None => Action::None,
                }
            }
            Target::Swatch(i) => {
                let picked = self
                    .palettes
                    .get(self.active_palette_idx)
                    .and_then(|p| p.colors.get(i))
                    .map(|(name, color)| (name.clone(), *color));
                match picked {
                    Some((name, color)) => self.pick(color, &name),
                    None => Action::None,
                }
            }
        }
    }

    /// Focus the value box, seeded with the colour it is showing.
    ///
    /// Seeded rather than empty because the common edit is a small change to
    /// the current colour — `89B4FA` to `89B4FF` — and an empty field would
    /// make the user retype the five digits they wanted to keep.
    fn begin_edit(&mut self) {
        self.hex_input = self.current.to_hex6();
        self.editing = true;
        // The box shows hex while it is being edited, so the tab follows it:
        // a field labelled CMYK that only accepts hex is a field that lies
        // about what it wants.
        self.active_format = ColorFormat::Hex;
        self.status = String::from("Type a hex colour, then Enter");
    }

    /// Abandon the edit, leaving the colour alone.
    fn cancel_edit(&mut self) {
        self.editing = false;
        self.hex_input.clear();
        self.status.clear();
    }

    /// Apply what was typed, or say why it was not applied.
    fn commit_edit(&mut self) -> Action {
        if self.apply_hex_input() {
            let hex = self.current.to_hex6();
            self.editing = false;
            self.hex_input.clear();
            self.status = format!("Set {hex}");
        } else {
            // The field stays open with the text still in it: clearing it
            // would destroy what the user typed and leave them nothing to
            // correct.
            self.status = format!("{:?} is not a hex colour", self.hex_input);
        }
        Action::Redraw
    }

    /// Route a press.
    pub fn handle_click(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        if button != MouseButton::Left {
            return Action::None;
        }
        // An armed eyedropper takes the click before any control does, which
        // is the whole point of a mode: it samples what is under the pointer.
        if self.eyedropper.active {
            let color = self.color_under(x, y, size);
            self.eyedrop_pick(x, y, color);
            self.status = format!("Sampled {}", color.to_hex6());
            return Action::Redraw;
        }
        match self.hit_test(x, y, size) {
            Some(target) => self.activate(target, x, size),
            None => {
                if self.editing {
                    self.cancel_edit();
                    return Action::Redraw;
                }
                Action::None
            }
        }
    }

    /// The colour the window is showing at a point.
    ///
    /// This is what the eyedropper samples. It reads this program's own
    /// drawing rather than the screen, because there is no screen-capture
    /// service to ask — see `known-issues.md`. That makes it useful for
    /// lifting a colour out of the palette or a harmony swatch, and honest
    /// about not reaching outside the window.
    #[must_use]
    pub fn color_under(&self, x: f32, y: f32, size: (f32, f32)) -> PickedColor {
        match self.hit_test(x, y, size) {
            Some(Target::Harmony(i)) => self.harmonies().get(i).map_or(self.current, |(_, c)| *c),
            Some(Target::Recent(i)) => self.history.iter().nth(i).copied().unwrap_or(self.current),
            Some(Target::Swatch(i)) => self
                .palettes
                .get(self.active_palette_idx)
                .and_then(|p| p.colors.get(i))
                .map_or(self.current, |(_, c)| *c),
            Some(Target::SwapContrast) => self.contrast_bg,
            _ => self.current,
        }
    }

    /// Continue a slider drag.
    pub fn handle_move(&mut self, x: f32, size: (f32, f32)) -> Action {
        match self.dragging {
            Some(channel) => self.drag_slider(channel, x, size),
            None => Action::None,
        }
    }

    /// End a slider drag.
    pub fn handle_release(&mut self) -> Action {
        if self.dragging.take().is_none() {
            return Action::None;
        }
        // The colour is only recorded in Recent once the drag *ends*: every
        // intermediate value a drag passes through would otherwise be pushed,
        // and the strip would show a gradient of colours the user scrubbed
        // past rather than the one they chose.
        let color = self.current;
        self.history.push(color);
        Action::Redraw
    }

    /// Route a key.
    pub fn handle_key(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        if !key.pressed {
            // A key *release* must not repeat the action of its press.
            return Action::None;
        }
        if self.editing {
            return self.handle_edit_key(key);
        }
        match key.key {
            Key::Escape => {
                if self.eyedropper.active {
                    // Escape backs out of the smallest thing first: closing
                    // the window out from under an armed mode would look like
                    // a crash.
                    self.eyedropper.active = false;
                    self.status = String::from("Eyedropper off");
                    return Action::Redraw;
                }
                Action::Quit
            }
            Key::C if key.modifiers.ctrl => self.activate(Target::CopyButton, 0.0, size),
            Key::V if key.modifiers.ctrl => {
                // Paste is the same gesture as typing into the box, so it
                // opens the box rather than inventing a second entry path.
                self.begin_edit();
                Action::Redraw
            }
            Key::E if key.modifiers.ctrl => self.activate(Target::Eyedropper, 0.0, size),
            Key::Tab => {
                let formats = ColorFormat::ALL;
                let here = formats
                    .iter()
                    .position(|f| *f == self.active_format)
                    .unwrap_or(0);
                let step = if key.modifiers.shift {
                    formats.len().saturating_sub(1)
                } else {
                    1
                };
                let next = here
                    .saturating_add(step)
                    .checked_rem(formats.len())
                    .unwrap_or(0);
                match formats.get(next) {
                    Some(fmt) => self.activate(Target::FormatTab(*fmt), 0.0, size),
                    None => Action::None,
                }
            }
            // The arrows nudge whichever channel the active format makes
            // primary: R/G/B under a byte format, H/S/L under the others.
            // Left and Right move it; Up and Down move the one after it, so
            // two axes are reachable without a focus concept the window does
            // not otherwise have.
            Key::Left => self.nudge(self.primary_channel(), -1.0),
            Key::Right => self.nudge(self.primary_channel(), 1.0),
            Key::Down => self.nudge(self.secondary_channel(), -1.0),
            Key::Up => self.nudge(self.secondary_channel(), 1.0),
            _ => Action::None,
        }
    }

    /// The channel the arrow keys move left and right.
    fn primary_channel(&self) -> Channel {
        match self.active_format {
            ColorFormat::Hex | ColorFormat::Rgb | ColorFormat::Cmyk => Channel::R,
            ColorFormat::Hsl | ColorFormat::Hsv => Channel::H,
        }
    }

    /// The channel the arrow keys move up and down.
    fn secondary_channel(&self) -> Channel {
        match self.active_format {
            ColorFormat::Hex | ColorFormat::Rgb | ColorFormat::Cmyk => Channel::G,
            ColorFormat::Hsl | ColorFormat::Hsv => Channel::L,
        }
    }

    /// Keys while the value box has focus.
    fn handle_edit_key(&mut self, key: &KeyEvent) -> Action {
        match key.key {
            Key::Enter => self.commit_edit(),
            Key::Escape => {
                self.cancel_edit();
                Action::Redraw
            }
            Key::Backspace => {
                if self.hex_input.pop().is_none() {
                    return Action::None;
                }
                Action::Redraw
            }
            _ => {
                // Every character the keystroke typed, not just the first:
                // one press can produce several (a dead key composing, a
                // paste delivered as text), and taking only the first would
                // silently drop the rest.
                let mut took = false;
                for c in key.typed() {
                    // `#` is allowed because that is how a hex colour is
                    // written everywhere else, and `from_hex_str` strips it.
                    // Everything that is not a hex digit is dropped rather
                    // than shown and then rejected: the field cannot hold a
                    // `z`, so there is no state in which it does.
                    if c != '#' && !c.is_ascii_hexdigit() {
                        continue;
                    }
                    // Nine characters is `#` plus eight digits, which is the
                    // longest form `from_hex_str` accepts (#RRGGBBAA).
                    if self.hex_input.chars().count() >= 9 {
                        break;
                    }
                    self.hex_input.push(c.to_ascii_uppercase());
                    took = true;
                }
                if took { Action::Redraw } else { Action::None }
            }
        }
    }

    /// Route a whole event.
    pub fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::Move => self.handle_move(mouse.x, size),
                MouseEventKind::Release(_) => self.handle_release(),
                // A pointer that leaves the window mid-drag ends the drag: the
                // moves that follow are not ours to see, so continuing would
                // leave the slider stuck to a pointer we cannot track.
                MouseEventKind::Leave => self.handle_release(),
                _ => Action::None,
            },
            Event::Key(key) => self.handle_key(key, size),
            Event::CloseRequested => Action::Quit,
            _ => Action::None,
        }
    }

    // -- Rendering ---------------------------------------------------------

    /// Draw the whole window at `width` x `height`, recording a rectangle for
    /// every control.
    ///
    /// Takes the size rather than reading [`Self::window_size`] because the
    /// caller is the only party that knows it: the compositor hands a size to
    /// `render`, and a frame drawn at a remembered size would put every
    /// control somewhere the user is not clicking.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let mut cmds = Frame::new(width, height);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: mocha::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut y = PADDING;

        // Title bar
        self.render_title(&mut cmds, &mut y, width);

        // Large color swatch preview
        self.render_swatch(&mut cmds, &mut y, width);

        // Format tabs
        self.render_format_tabs(&mut cmds, &mut y, width);

        // Format value display
        self.render_format_values(&mut cmds, &mut y, width);

        // RGB sliders
        self.render_rgb_sliders(&mut cmds, &mut y, width);

        // HSL sliders
        self.render_hsl_sliders(&mut cmds, &mut y, width);

        // Harmony suggestions
        self.render_harmonies(&mut cmds, &mut y);

        // Contrast checker
        self.render_contrast_panel(&mut cmds, &mut y, width);

        // History strip
        self.render_history(&mut cmds, &mut y, width);

        // Palette grid
        self.render_palette(&mut cmds, &mut y, width);

        cmds
    }

    fn render_title(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Color Picker".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE_LARGE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Eyedropper button
        let btn_x = width - PADDING - 100.0;

        // The status line lives between the title and the button because
        // that is the only strip of the window that is never redrawn by a
        // control: every message here is about something the user just did,
        // and it must not push the sliders down when it appears.
        if !self.status.is_empty() {
            let status_x = PADDING + 110.0;
            let room = btn_x - status_x - 8.0;
            if room > 0.0 {
                cmds.push(RenderCommand::Text {
                    x: status_x,
                    y: *y + 4.0,
                    text: self.status.clone(),
                    color: mocha::SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(room),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        let btn_color = if self.eyedropper.active {
            mocha::BLUE
        } else {
            mocha::SURFACE1
        };
        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: *y - 2.0,
            width: 100.0,
            height: 24.0,
            color: btn_color,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.hit(Target::Eyedropper, Rect::new(btn_x, *y - 2.0, 100.0, 24.0));
        cmds.push(RenderCommand::Text {
            x: btn_x + 10.0,
            y: *y + 1.0,
            text: "Eyedropper".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        *y += 28.0;
    }

    fn render_swatch(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        let swatch_x = PADDING;

        // Checkerboard background (for alpha visibility)
        cmds.push(RenderCommand::FillRect {
            x: swatch_x,
            y: *y,
            width: SWATCH_SIZE,
            height: SWATCH_SIZE,
            color: Color::rgb(200, 200, 200),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Actual color swatch
        cmds.push(RenderCommand::FillRect {
            x: swatch_x,
            y: *y,
            width: SWATCH_SIZE,
            height: SWATCH_SIZE,
            color: self.current.to_color(),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Border around swatch
        cmds.push(RenderCommand::StrokeRect {
            x: swatch_x,
            y: *y,
            width: SWATCH_SIZE,
            height: SWATCH_SIZE,
            color: mocha::OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Color info to the right of the swatch
        let info_x = swatch_x + SWATCH_SIZE + PADDING;
        let mut info_y = *y + 4.0;

        // Show all format strings beside the swatch
        for fmt in ColorFormat::ALL {
            let text = self.current.format_as(*fmt);
            let label_color = if *fmt == self.active_format {
                mocha::BLUE
            } else {
                mocha::SUBTEXT0
            };
            cmds.push(RenderCommand::Text {
                x: info_x,
                y: info_y,
                text: format!("{}: {}", fmt.label(), text),
                color: label_color,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - info_x - PADDING),
                overflow: TextOverflow::Ellipsis,
            });
            info_y += 18.0;
        }

        // Copy button
        let copy_y = *y + SWATCH_SIZE - 24.0;
        cmds.push(RenderCommand::FillRect {
            x: info_x,
            y: copy_y,
            width: 60.0,
            height: 22.0,
            color: mocha::SURFACE1,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.hit(Target::CopyButton, Rect::new(info_x, copy_y, 60.0, 22.0));
        cmds.push(RenderCommand::Text {
            x: info_x + 12.0,
            y: copy_y + 3.0,
            text: "Copy".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        *y += SWATCH_SIZE + PADDING;
    }

    fn render_format_tabs(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        let tab_width = (width - 2.0 * PADDING) / ColorFormat::ALL.len() as f32;

        // Tab bar background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: *y,
            width: width - 2.0 * PADDING,
            height: TAB_HEIGHT,
            color: mocha::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        for (i, fmt) in ColorFormat::ALL.iter().enumerate() {
            let tab_x = PADDING + i as f32 * tab_width;
            let is_active = *fmt == self.active_format;
            cmds.hit(
                Target::FormatTab(*fmt),
                Rect::new(tab_x, *y, tab_width, TAB_HEIGHT),
            );

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: *y,
                    width: tab_width,
                    height: TAB_HEIGHT,
                    color: mocha::SURFACE1,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
            }

            let text_color = if is_active {
                mocha::BLUE
            } else {
                mocha::SUBTEXT0
            };

            cmds.push(RenderCommand::Text {
                x: tab_x + tab_width / 2.0 - 12.0,
                y: *y + 8.0,
                text: fmt.label().to_string(),
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width),
                overflow: TextOverflow::Ellipsis,
            });
        }

        *y += TAB_HEIGHT + PADDING;
    }

    fn render_format_values(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        let field_w = width - 2.0 * PADDING;

        // Input field background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: *y,
            width: field_w,
            height: 30.0,
            color: mocha::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.hit(Target::ValueBox, Rect::new(PADDING, *y, field_w, 30.0));

        // While typing, the border says whether what is in the box would be
        // accepted — a hex field that only tells you at Enter is a field you
        // have to guess at. An empty buffer is neither right nor wrong yet,
        // so it stays neutral rather than starting out red.
        let border = if self.editing {
            match (
                self.hex_input.is_empty(),
                PickedColor::from_hex_str(&self.hex_input).is_some(),
            ) {
                (true, _) => mocha::BLUE,
                (false, true) => mocha::GREEN,
                (false, false) => mocha::RED,
            }
        } else {
            mocha::OVERLAY0
        };
        cmds.push(RenderCommand::StrokeRect {
            x: PADDING,
            y: *y,
            width: field_w,
            height: 30.0,
            color: border,
            line_width: if self.editing { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        // A caret is drawn as part of the text rather than as a separate
        // command, because there is no cursor within the buffer to place one
        // at: typing always appends and Backspace always removes from the
        // end, so the caret is only ever at the end.
        let text = if self.editing {
            format!("{}_", self.hex_input)
        } else {
            self.current.format_as(self.active_format)
        };
        cmds.push(RenderCommand::Text {
            x: PADDING + 8.0,
            y: *y + 7.0,
            text,
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        *y += 30.0 + PADDING;
    }

    /// The track's rectangle, given the slider's own box.
    ///
    /// One function, called by both the renderer and the click handler, so a
    /// press lands on the value the thumb is drawn at. Two copies of this
    /// arithmetic would be two chances to disagree, and the symptom would be
    /// a slider that jumps a few units away from where it was clicked.
    fn track_rect(x: f32, y: f32, width: f32) -> Rect {
        Rect::new(
            x + 36.0,
            y + (SLIDER_HEIGHT - SLIDER_TRACK_HEIGHT) / 2.0,
            width - 80.0,
            SLIDER_TRACK_HEIGHT,
        )
    }

    // Slider geometry + value/max + a track color; grouping these would not
    // improve readability and would force allocations at the call site.
    #[allow(clippy::too_many_arguments)]
    fn render_slider(
        cmds: &mut Frame,
        x: f32,
        y: f32,
        width: f32,
        channel: Channel,
        value: f32,
        max_val: f32,
        track_color: Color,
    ) {
        // Label
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: channel.label().to_string(),
            color: mocha::SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let track = Self::track_rect(x, y, width);
        let (track_x, track_y, track_w) = (track.x, track.y, track.w);

        // The whole slider row is clickable, not just the eight-pixel track:
        // aiming at a line that thin is a fiddle, and the row has nothing
        // else in it to hit by mistake.
        cmds.hit(
            Target::Slider(channel),
            Rect::new(x, y, width, SLIDER_HEIGHT),
        );

        // Track background
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: track_y,
            width: track_w,
            height: SLIDER_TRACK_HEIGHT,
            color: mocha::SURFACE0,
            corner_radii: CornerRadii::all(SLIDER_TRACK_HEIGHT / 2.0),
        });

        // Filled portion
        let fill_frac = if max_val > 0.0 { value / max_val } else { 0.0 };
        let fill_w = track_w * fill_frac;
        if fill_w > 0.5 {
            cmds.push(RenderCommand::FillRect {
                x: track_x,
                y: track_y,
                width: fill_w,
                height: SLIDER_TRACK_HEIGHT,
                color: track_color,
                corner_radii: CornerRadii::all(SLIDER_TRACK_HEIGHT / 2.0),
            });
        }

        // Thumb
        let thumb_x = track_x + fill_w - 6.0;
        cmds.push(RenderCommand::FillRect {
            x: thumb_x,
            y: y + 2.0,
            width: 12.0,
            height: SLIDER_HEIGHT - 4.0,
            color: mocha::TEXT,
            corner_radii: CornerRadii::all(3.0),
        });

        // Value text
        let val_text = if max_val > 1.0 {
            format!("{:.0}", value)
        } else {
            format!("{:.0}%", value * 100.0)
        };
        cmds.push(RenderCommand::Text {
            x: track_x + track_w + 8.0,
            y: y + 4.0,
            text: val_text,
            color: mocha::TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_rgb_sliders(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "RGB".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *y += 18.0;

        let slider_w = width - 2.0 * PADDING;
        Self::render_slider(
            cmds,
            PADDING,
            *y,
            slider_w,
            Channel::R,
            self.current.r as f32,
            255.0,
            Color::rgb(self.current.r, 60, 60),
        );
        *y += SLIDER_HEIGHT + 4.0;

        Self::render_slider(
            cmds,
            PADDING,
            *y,
            slider_w,
            Channel::G,
            self.current.g as f32,
            255.0,
            Color::rgb(60, self.current.g, 60),
        );
        *y += SLIDER_HEIGHT + 4.0;

        Self::render_slider(
            cmds,
            PADDING,
            *y,
            slider_w,
            Channel::B,
            self.current.b as f32,
            255.0,
            Color::rgb(60, 60, self.current.b),
        );
        *y += SLIDER_HEIGHT + PADDING;
    }

    fn render_hsl_sliders(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        let hsl = self.current.to_hsl();

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "HSL".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *y += 18.0;

        let slider_w = width - 2.0 * PADDING;
        Self::render_slider(
            cmds,
            PADDING,
            *y,
            slider_w,
            Channel::H,
            hsl.h,
            360.0,
            mocha::MAUVE,
        );
        *y += SLIDER_HEIGHT + 4.0;

        Self::render_slider(
            cmds,
            PADDING,
            *y,
            slider_w,
            Channel::S,
            hsl.s,
            1.0,
            mocha::PEACH,
        );
        *y += SLIDER_HEIGHT + 4.0;

        Self::render_slider(
            cmds,
            PADDING,
            *y,
            slider_w,
            Channel::L,
            hsl.l,
            1.0,
            mocha::YELLOW,
        );
        *y += SLIDER_HEIGHT + PADDING;
    }

    fn render_harmonies(&self, cmds: &mut Frame, y: &mut f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Harmony".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *y += 18.0;

        // Drawn from the same function the click handler uses. If this
        // computed its own five colours, a swatch could show one colour and
        // apply another the moment either copy was edited.
        let harmony_colors = self.harmonies();

        let cell_w = 48.0;
        let cell_h = 36.0;
        let gap = 8.0;

        for (i, (label, color)) in harmony_colors.iter().enumerate() {
            let cx = PADDING + i as f32 * (cell_w + gap);
            // The label under the swatch is inside the target: it names the
            // swatch, so a click on it means the swatch.
            cmds.hit(Target::Harmony(i), Rect::new(cx, *y, cell_w, cell_h));

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: *y,
                width: cell_w,
                height: cell_h - 14.0,
                color: color.to_color(),
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: cx,
                y: *y,
                width: cell_w,
                height: cell_h - 14.0,
                color: mocha::OVERLAY0,
                line_width: 1.0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 4.0,
                y: *y + cell_h - 12.0,
                text: label.to_string(),
                color: mocha::SUBTEXT0,
                font_size: FONT_SIZE_SMALL - 1.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        *y += cell_h + PADDING;
    }

    fn render_contrast_panel(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Contrast Checker".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *y += 18.0;

        let ratio = contrast_ratio(self.current, self.contrast_bg);
        let level = wcag_level(ratio);

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: *y,
            width: width - 2.0 * PADDING,
            height: CONTRAST_PANEL_HEIGHT,
            color: mocha::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        // Foreground swatch
        let sw = 40.0;
        // Both swatches swap: the panel compares two colours and only one of
        // them is editable, so the only way to work on the background is to
        // make it the current colour. Clicking either one is the gesture,
        // because either one is what the user is pointing at when they think
        // "that one".
        cmds.hit(
            Target::SwapContrast,
            Rect::new(PADDING + 8.0, *y + 8.0, sw * 2.0 + 8.0, sw),
        );
        cmds.push(RenderCommand::FillRect {
            x: PADDING + 8.0,
            y: *y + 8.0,
            width: sw,
            height: sw,
            color: self.current.to_color(),
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: PADDING + 8.0,
            y: *y + 52.0,
            text: "FG".to_string(),
            color: mocha::SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Background swatch
        cmds.push(RenderCommand::FillRect {
            x: PADDING + 8.0 + sw + 8.0,
            y: *y + 8.0,
            width: sw,
            height: sw,
            color: self.contrast_bg.to_color(),
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: PADDING + 8.0 + sw + 8.0,
            y: *y + 52.0,
            text: "BG".to_string(),
            color: mocha::SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Ratio text
        let ratio_x = PADDING + 8.0 + sw * 2.0 + 24.0;
        cmds.push(RenderCommand::Text {
            x: ratio_x,
            y: *y + 12.0,
            text: format!("Ratio: {ratio:.2}:1"),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // WCAG level
        let level_color = match level {
            WcagLevel::Aaa => mocha::GREEN,
            WcagLevel::Aa => mocha::BLUE,
            WcagLevel::AaLarge => mocha::YELLOW,
            WcagLevel::Fail => mocha::RED,
        };
        cmds.push(RenderCommand::Text {
            x: ratio_x,
            y: *y + 30.0,
            text: format!("WCAG: {}", level.label()),
            color: level_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Sample text on background
        let sample_x = ratio_x;
        cmds.push(RenderCommand::FillRect {
            x: sample_x,
            y: *y + 48.0,
            width: 160.0,
            height: 22.0,
            color: self.contrast_bg.to_color(),
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: sample_x + 6.0,
            y: *y + 52.0,
            text: "Sample text".to_string(),
            color: self.current.to_color(),
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(148.0),
            overflow: TextOverflow::Ellipsis,
        });

        *y += CONTRAST_PANEL_HEIGHT + PADDING;
    }

    fn render_history(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Recent".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *y += 18.0;

        // Calculate how many cells fit in one row.
        let avail_w = width - 2.0 * PADDING;
        let cells_per_row = guitk::grid::columns_across(avail_w, HISTORY_CELL, HISTORY_GAP);

        for (i, color) in self.history.iter().enumerate() {
            let col = i % cells_per_row;
            let row = i / cells_per_row;
            if row > 1 {
                break; // Only show two rows of history.
            }
            let cx = PADDING + col as f32 * (HISTORY_CELL + HISTORY_GAP);
            let cy = *y + row as f32 * (HISTORY_CELL + HISTORY_GAP);
            cmds.hit(
                Target::Recent(i),
                Rect::new(cx, cy, HISTORY_CELL, HISTORY_CELL),
            );

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: HISTORY_CELL,
                height: HISTORY_CELL,
                color: color.to_color(),
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: cx,
                y: cy,
                width: HISTORY_CELL,
                height: HISTORY_CELL,
                color: mocha::OVERLAY0,
                line_width: 1.0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
        }

        // `.get()` is right here and nowhere else in this function: this is a
        // comparison, not a division, so there is no non-zero-ness for the
        // type to carry.
        let rows_shown = if self.history.len() > cells_per_row.get() {
            2
        } else if self.history.is_empty() {
            0
        } else {
            1
        };
        *y += rows_shown as f32 * (HISTORY_CELL + HISTORY_GAP) + PADDING;
    }

    fn render_palette(&self, cmds: &mut Frame, y: &mut f32, width: f32) {
        let palette = match self.palettes.get(self.active_palette_idx) {
            Some(p) => p,
            None => return,
        };

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: format!("Palette: {}", palette.name),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *y += 18.0;

        let avail_w = width - 2.0 * PADDING;
        let cells_per_row = guitk::grid::columns_across(avail_w, PALETTE_CELL, PALETTE_GAP);

        for (i, (name, color)) in palette.colors.iter().enumerate() {
            let col = i % cells_per_row;
            let row = i / cells_per_row;
            let cx = PADDING + col as f32 * (PALETTE_CELL + PALETTE_GAP);
            let cy = *y + row as f32 * (PALETTE_CELL + PALETTE_GAP + 14.0);
            // Including the name below the swatch, which is 14 pixels of
            // clickable text that would otherwise do nothing.
            cmds.hit(
                Target::Swatch(i),
                Rect::new(cx, cy, PALETTE_CELL, PALETTE_CELL + 14.0),
            );

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: PALETTE_CELL,
                height: PALETTE_CELL,
                color: color.to_color(),
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: cx,
                y: cy,
                width: PALETTE_CELL,
                height: PALETTE_CELL,
                color: mocha::OVERLAY0,
                line_width: 1.0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy + PALETTE_CELL + 2.0,
                text: name.clone(),
                color: mocha::SUBTEXT0,
                font_size: FONT_SIZE_SMALL - 1.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PALETTE_CELL),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// The window
// ============================================================================

impl App for ColorPickerApp {
    fn title(&self) -> String {
        "Color Picker".to_string()
    }

    fn app_id(&self) -> String {
        "colorpicker".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        // Cast from the same constants the layout is written against, so the
        // window opens at the size every rectangle in this file assumes.
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // A resize is recorded and nothing else: the layout is recomputed on
        // the next frame from the size `render` is handed, so there is no
        // second copy of the geometry here to get out of step with it. The
        // record exists only so the *next* click can be hit-tested against
        // the window the user is actually looking at.
        if let Event::Resize { width, height } = *event {
            self.window_size = (width as f32, height as f32);
            return Response::Redraw;
        }

        match self.handle_event(event, self.window_size) {
            Action::Quit => Response::Exit,
            Action::Redraw => Response::Redraw,
            Action::None => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Believe the size we are handed. The first frame is submitted before
        // any `Event::Resize` arrives, so the remembered size is a starting
        // guess and this is the correction.
        self.window_size = (width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for ColorPickerApp {
    type Target = Target;
    type Outcome = Action;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Action {
        self.handle_click(x, y, button, size)
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        self.handle_key(key, size)
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    let mut state = ColorPickerApp::create();
    app::launch("colorpicker", &mut state)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is
    // a failure report, and an index that is out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    // -- Hex conversion tests ----------------------------------------------

    #[test]
    fn hex6_format_black() {
        let c = PickedColor::from_rgb(0, 0, 0);
        assert_eq!(c.to_hex6(), "#000000");
    }

    #[test]
    fn hex6_format_white() {
        let c = PickedColor::from_rgb(255, 255, 255);
        assert_eq!(c.to_hex6(), "#FFFFFF");
    }

    #[test]
    fn hex6_format_red() {
        let c = PickedColor::from_rgb(255, 0, 0);
        assert_eq!(c.to_hex6(), "#FF0000");
    }

    #[test]
    fn hex8_format_with_alpha() {
        let c = PickedColor::new(255, 128, 0, 200);
        assert_eq!(c.to_hex8(), "#FF8000C8");
    }

    #[test]
    fn parse_hex6() {
        let c = PickedColor::from_hex_str("#FF8000").expect("should parse");
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 128);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn parse_hex6_no_hash() {
        let c = PickedColor::from_hex_str("FF8000").expect("should parse");
        assert_eq!(c.r, 255);
    }

    #[test]
    fn parse_hex8() {
        let c = PickedColor::from_hex_str("#FF8000C8").expect("should parse");
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 128);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 200);
    }

    #[test]
    fn parse_hex3() {
        let c = PickedColor::from_hex_str("#F80").expect("should parse");
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 136);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn parse_hex_invalid_length() {
        assert!(PickedColor::from_hex_str("#FFFFF").is_none());
    }

    #[test]
    fn parse_hex_invalid_chars() {
        assert!(PickedColor::from_hex_str("#GGGGGG").is_none());
    }

    #[test]
    fn parse_hex_empty() {
        assert!(PickedColor::from_hex_str("").is_none());
    }

    // -- RGB <-> HSL conversion tests --------------------------------------

    #[test]
    fn hsl_pure_red() {
        let c = PickedColor::from_rgb(255, 0, 0);
        let hsl = c.to_hsl();
        assert!((hsl.h - 0.0).abs() < 1.0);
        assert!((hsl.s - 1.0).abs() < 0.01);
        assert!((hsl.l - 0.5).abs() < 0.01);
    }

    #[test]
    fn hsl_pure_green() {
        let c = PickedColor::from_rgb(0, 255, 0);
        let hsl = c.to_hsl();
        assert!((hsl.h - 120.0).abs() < 1.0);
        assert!((hsl.s - 1.0).abs() < 0.01);
        assert!((hsl.l - 0.5).abs() < 0.01);
    }

    #[test]
    fn hsl_pure_blue() {
        let c = PickedColor::from_rgb(0, 0, 255);
        let hsl = c.to_hsl();
        assert!((hsl.h - 240.0).abs() < 1.0);
        assert!((hsl.s - 1.0).abs() < 0.01);
        assert!((hsl.l - 0.5).abs() < 0.01);
    }

    #[test]
    fn hsl_gray() {
        let c = PickedColor::from_rgb(128, 128, 128);
        let hsl = c.to_hsl();
        assert!(hsl.s < 0.01);
        assert!((hsl.l - 128.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn hsl_white() {
        let c = PickedColor::from_rgb(255, 255, 255);
        let hsl = c.to_hsl();
        assert!((hsl.l - 1.0).abs() < 0.01);
    }

    #[test]
    fn hsl_black() {
        let c = PickedColor::from_rgb(0, 0, 0);
        let hsl = c.to_hsl();
        assert!(hsl.l < 0.01);
    }

    #[test]
    fn hsl_roundtrip_red() {
        let orig = PickedColor::from_rgb(255, 0, 0);
        let hsl = orig.to_hsl();
        let back = PickedColor::from_hsl(hsl);
        assert_eq!(back.r, orig.r);
        assert_eq!(back.g, orig.g);
        assert_eq!(back.b, orig.b);
    }

    #[test]
    fn hsl_roundtrip_catppuccin_blue() {
        let orig = PickedColor::from_rgb(137, 180, 250);
        let hsl = orig.to_hsl();
        let back = PickedColor::from_hsl(hsl);
        assert!((back.r as i16 - orig.r as i16).abs() <= 1);
        assert!((back.g as i16 - orig.g as i16).abs() <= 1);
        assert!((back.b as i16 - orig.b as i16).abs() <= 1);
    }

    #[test]
    fn hsl_roundtrip_arbitrary() {
        let orig = PickedColor::from_rgb(42, 170, 99);
        let hsl = orig.to_hsl();
        let back = PickedColor::from_hsl(hsl);
        assert!((back.r as i16 - orig.r as i16).abs() <= 1);
        assert!((back.g as i16 - orig.g as i16).abs() <= 1);
        assert!((back.b as i16 - orig.b as i16).abs() <= 1);
    }

    #[test]
    fn from_hsl_saturated_red() {
        let c = PickedColor::from_hsl(Hsl {
            h: 0.0,
            s: 1.0,
            l: 0.5,
        });
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn from_hsl_zero_saturation() {
        let c = PickedColor::from_hsl(Hsl {
            h: 200.0,
            s: 0.0,
            l: 0.5,
        });
        // Gray regardless of hue.
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    // -- RGB <-> HSV conversion tests --------------------------------------

    #[test]
    fn hsv_pure_red() {
        let c = PickedColor::from_rgb(255, 0, 0);
        let hsv = c.to_hsv();
        assert!((hsv.h - 0.0).abs() < 1.0);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn hsv_pure_green() {
        let c = PickedColor::from_rgb(0, 255, 0);
        let hsv = c.to_hsv();
        assert!((hsv.h - 120.0).abs() < 1.0);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn hsv_gray() {
        let c = PickedColor::from_rgb(128, 128, 128);
        let hsv = c.to_hsv();
        assert!(hsv.s < 0.01);
    }

    #[test]
    fn hsv_black() {
        let c = PickedColor::from_rgb(0, 0, 0);
        let hsv = c.to_hsv();
        assert!(hsv.v < 0.01);
        assert!(hsv.s < 0.01);
    }

    #[test]
    fn hsv_roundtrip() {
        let orig = PickedColor::from_rgb(200, 100, 50);
        let hsv = orig.to_hsv();
        let back = PickedColor::from_hsv(hsv);
        assert!((back.r as i16 - orig.r as i16).abs() <= 1);
        assert!((back.g as i16 - orig.g as i16).abs() <= 1);
        assert!((back.b as i16 - orig.b as i16).abs() <= 1);
    }

    #[test]
    fn from_hsv_zero_saturation() {
        let c = PickedColor::from_hsv(Hsv {
            h: 100.0,
            s: 0.0,
            v: 0.75,
        });
        let expected = (0.75_f32 * 255.0).round() as u8;
        assert_eq!(c.r, expected);
        assert_eq!(c.g, expected);
        assert_eq!(c.b, expected);
    }

    #[test]
    fn from_hsv_sectors_cover_360() {
        // Test one color per HSV sector to ensure all 6 branches are exercised.
        for hue in [0.0, 60.0, 120.0, 180.0, 240.0, 300.0] {
            let c = PickedColor::from_hsv(Hsv {
                h: hue,
                s: 1.0,
                v: 1.0,
            });
            let back = c.to_hsv();
            assert!(
                (back.h - hue).abs() < 1.0,
                "hue {hue} roundtrip failed: got {:.1}",
                back.h
            );
        }
    }

    // -- CMYK conversion tests ---------------------------------------------

    #[test]
    fn cmyk_white() {
        let c = PickedColor::from_rgb(255, 255, 255);
        let cmyk = c.to_cmyk();
        assert!(cmyk.c < 0.01);
        assert!(cmyk.m < 0.01);
        assert!(cmyk.y < 0.01);
        assert!(cmyk.k < 0.01);
    }

    #[test]
    fn cmyk_black() {
        let c = PickedColor::from_rgb(0, 0, 0);
        let cmyk = c.to_cmyk();
        assert!((cmyk.k - 1.0).abs() < 0.01);
    }

    #[test]
    fn cmyk_pure_red() {
        let c = PickedColor::from_rgb(255, 0, 0);
        let cmyk = c.to_cmyk();
        assert!(cmyk.c < 0.01);
        assert!((cmyk.m - 1.0).abs() < 0.01);
        assert!((cmyk.y - 1.0).abs() < 0.01);
        assert!(cmyk.k < 0.01);
    }

    #[test]
    fn cmyk_roundtrip() {
        let orig = PickedColor::from_rgb(120, 80, 200);
        let cmyk = orig.to_cmyk();
        let back = PickedColor::from_cmyk(cmyk);
        assert!((back.r as i16 - orig.r as i16).abs() <= 1);
        assert!((back.g as i16 - orig.g as i16).abs() <= 1);
        assert!((back.b as i16 - orig.b as i16).abs() <= 1);
    }

    #[test]
    fn cmyk_cyan() {
        let c = PickedColor::from_rgb(0, 255, 255);
        let cmyk = c.to_cmyk();
        assert!((cmyk.c - 1.0).abs() < 0.01);
        assert!(cmyk.m < 0.01);
        assert!(cmyk.y < 0.01);
        assert!(cmyk.k < 0.01);
    }

    // -- Contrast ratio tests ----------------------------------------------

    #[test]
    fn contrast_black_white() {
        let black = PickedColor::from_rgb(0, 0, 0);
        let white = PickedColor::from_rgb(255, 255, 255);
        let ratio = contrast_ratio(black, white);
        assert!((ratio - 21.0).abs() < 0.1);
    }

    #[test]
    fn contrast_same_color() {
        let c = PickedColor::from_rgb(100, 100, 100);
        let ratio = contrast_ratio(c, c);
        assert!((ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn contrast_symmetric() {
        let a = PickedColor::from_rgb(200, 50, 50);
        let b = PickedColor::from_rgb(50, 200, 50);
        let r1 = contrast_ratio(a, b);
        let r2 = contrast_ratio(b, a);
        assert!((r1 - r2).abs() < 0.001);
    }

    #[test]
    fn wcag_level_aaa() {
        assert_eq!(wcag_level(7.5), WcagLevel::Aaa);
    }

    #[test]
    fn wcag_level_aa() {
        assert_eq!(wcag_level(5.0), WcagLevel::Aa);
    }

    #[test]
    fn wcag_level_aa_large() {
        assert_eq!(wcag_level(3.5), WcagLevel::AaLarge);
    }

    #[test]
    fn wcag_level_fail() {
        assert_eq!(wcag_level(2.0), WcagLevel::Fail);
    }

    #[test]
    fn wcag_boundary_7() {
        assert_eq!(wcag_level(7.0), WcagLevel::Aaa);
    }

    #[test]
    fn wcag_boundary_4_5() {
        assert_eq!(wcag_level(4.5), WcagLevel::Aa);
    }

    #[test]
    fn wcag_boundary_3() {
        assert_eq!(wcag_level(3.0), WcagLevel::AaLarge);
    }

    // -- Luminance tests ---------------------------------------------------

    #[test]
    fn luminance_black_zero() {
        let c = PickedColor::from_rgb(0, 0, 0);
        assert!(c.relative_luminance() < 0.001);
    }

    #[test]
    fn luminance_white_one() {
        let c = PickedColor::from_rgb(255, 255, 255);
        assert!((c.relative_luminance() - 1.0).abs() < 0.001);
    }

    #[test]
    fn luminance_green_highest() {
        // Green has the highest luminance weight (0.7152).
        let r = PickedColor::from_rgb(128, 0, 0).relative_luminance();
        let g = PickedColor::from_rgb(0, 128, 0).relative_luminance();
        let b = PickedColor::from_rgb(0, 0, 128).relative_luminance();
        assert!(g > r);
        assert!(g > b);
    }

    // -- Palette tests -----------------------------------------------------

    #[test]
    fn palette_add_and_get() {
        let mut p = ColorPalette::with_name("Test");
        assert!(p.add("Red", PickedColor::from_rgb(255, 0, 0)));
        assert_eq!(p.get("Red"), Some(PickedColor::from_rgb(255, 0, 0)));
    }

    #[test]
    fn palette_add_duplicate_name() {
        let mut p = ColorPalette::with_name("Test");
        assert!(p.add("Red", PickedColor::from_rgb(255, 0, 0)));
        assert!(!p.add("Red", PickedColor::from_rgb(200, 0, 0)));
    }

    #[test]
    fn palette_remove() {
        let mut p = ColorPalette::with_name("Test");
        p.add("Red", PickedColor::from_rgb(255, 0, 0));
        assert!(p.remove("Red"));
        assert!(p.get("Red").is_none());
        assert!(p.is_empty());
    }

    #[test]
    fn palette_remove_nonexistent() {
        let mut p = ColorPalette::with_name("Test");
        assert!(!p.remove("Nope"));
    }

    #[test]
    fn palette_rename() {
        let mut p = ColorPalette::with_name("Test");
        p.add("Red", PickedColor::from_rgb(255, 0, 0));
        assert!(p.rename("Red", "Crimson"));
        assert!(p.get("Red").is_none());
        assert_eq!(p.get("Crimson"), Some(PickedColor::from_rgb(255, 0, 0)));
    }

    #[test]
    fn palette_rename_to_existing() {
        let mut p = ColorPalette::with_name("Test");
        p.add("Red", PickedColor::from_rgb(255, 0, 0));
        p.add("Blue", PickedColor::from_rgb(0, 0, 255));
        assert!(!p.rename("Red", "Blue"));
    }

    #[test]
    fn palette_rename_nonexistent() {
        let mut p = ColorPalette::with_name("Test");
        assert!(!p.rename("Nope", "Also Nope"));
    }

    #[test]
    fn palette_len() {
        let mut p = ColorPalette::with_name("Test");
        assert_eq!(p.len(), 0);
        p.add("A", PickedColor::from_rgb(1, 2, 3));
        p.add("B", PickedColor::from_rgb(4, 5, 6));
        assert_eq!(p.len(), 2);
    }

    // -- History tests -----------------------------------------------------

    #[test]
    fn history_push_and_get() {
        let mut h = ColorHistory::with_capacity(5);
        h.push(PickedColor::from_rgb(10, 20, 30));
        assert_eq!(h.get(0), Some(&PickedColor::from_rgb(10, 20, 30)));
    }

    #[test]
    fn history_newest_first() {
        let mut h = ColorHistory::with_capacity(5);
        h.push(PickedColor::from_rgb(1, 1, 1));
        h.push(PickedColor::from_rgb(2, 2, 2));
        assert_eq!(h.get(0), Some(&PickedColor::from_rgb(2, 2, 2)));
        assert_eq!(h.get(1), Some(&PickedColor::from_rgb(1, 1, 1)));
    }

    #[test]
    fn history_capacity_limit() {
        let mut h = ColorHistory::with_capacity(3);
        for i in 0..5 {
            h.push(PickedColor::from_rgb(i, i, i));
        }
        assert_eq!(h.len(), 3);
        // Most recent should be the last pushed.
        assert_eq!(h.get(0), Some(&PickedColor::from_rgb(4, 4, 4)));
    }

    #[test]
    fn history_deduplicates() {
        let mut h = ColorHistory::with_capacity(10);
        let c = PickedColor::from_rgb(50, 50, 50);
        h.push(PickedColor::from_rgb(1, 1, 1));
        h.push(c);
        h.push(PickedColor::from_rgb(2, 2, 2));
        h.push(c); // duplicate — should move to front.
        assert_eq!(h.len(), 3);
        assert_eq!(h.get(0), Some(&c));
    }

    #[test]
    fn history_clear() {
        let mut h = ColorHistory::with_capacity(10);
        h.push(PickedColor::from_rgb(1, 2, 3));
        h.clear();
        assert!(h.is_empty());
    }

    // -- Format display tests ----------------------------------------------

    #[test]
    fn format_as_hex_opaque() {
        let c = PickedColor::from_rgb(255, 128, 0);
        assert_eq!(c.format_as(ColorFormat::Hex), "#FF8000");
    }

    #[test]
    fn format_as_hex_alpha() {
        let c = PickedColor::new(255, 128, 0, 128);
        assert_eq!(c.format_as(ColorFormat::Hex), "#FF800080");
    }

    #[test]
    fn format_as_rgb() {
        let c = PickedColor::from_rgb(10, 20, 30);
        assert_eq!(c.format_as(ColorFormat::Rgb), "rgb(10, 20, 30)");
    }

    #[test]
    fn format_as_rgba() {
        let c = PickedColor::new(10, 20, 30, 128);
        assert_eq!(c.format_as(ColorFormat::Rgb), "rgba(10, 20, 30, 128)");
    }

    #[test]
    fn format_as_hsl_contains_hsl() {
        let c = PickedColor::from_rgb(255, 0, 0);
        let s = c.format_as(ColorFormat::Hsl);
        assert!(s.starts_with("hsl("), "got: {s}");
    }

    #[test]
    fn format_as_hsv_contains_hsv() {
        let c = PickedColor::from_rgb(0, 255, 0);
        let s = c.format_as(ColorFormat::Hsv);
        assert!(s.starts_with("hsv("), "got: {s}");
    }

    #[test]
    fn format_as_cmyk_contains_cmyk() {
        let c = PickedColor::from_rgb(0, 0, 255);
        let s = c.format_as(ColorFormat::Cmyk);
        assert!(s.starts_with("cmyk("), "got: {s}");
    }

    // -- Harmony tests -----------------------------------------------------

    #[test]
    fn complementary_red_is_cyan() {
        let c = PickedColor::from_rgb(255, 0, 0);
        let comp = c.complementary();
        let hsl = comp.to_hsl();
        assert!((hsl.h - 180.0).abs() < 2.0);
    }

    #[test]
    fn analogous_returns_two_colors() {
        let c = PickedColor::from_rgb(100, 150, 200);
        let (a, b) = c.analogous();
        // They should differ from the original.
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn triadic_returns_two_colors() {
        let c = PickedColor::from_rgb(200, 100, 50);
        let (a, b) = c.triadic();
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn triadic_hue_separation() {
        let c = PickedColor::from_rgb(255, 0, 0);
        let (a, b) = c.triadic();
        let h_a = a.to_hsl().h;
        let h_b = b.to_hsl().h;
        assert!((h_a - 120.0).abs() < 2.0);
        assert!((h_b - 240.0).abs() < 2.0);
    }

    // -- Rendering tests ---------------------------------------------------

    #[test]
    fn render_produces_commands() {
        let app = ColorPickerApp::create();
        let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let cmds = frame.commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_starts_with_background() {
        let app = ColorPickerApp::create();
        let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let cmds = frame.commands();
        match &cmds[0] {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*width, WINDOW_WIDTH);
                assert_eq!(*height, WINDOW_HEIGHT);
            }
            _ => panic!("first command should be FillRect for background"),
        }
    }

    #[test]
    fn render_contains_text_commands() {
        let app = ColorPickerApp::create();
        let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let cmds = frame.commands();
        let has_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. }));
        assert!(has_text, "should contain Text commands");
    }

    #[test]
    fn render_with_history() {
        let mut app = ColorPickerApp::create();
        app.set_color(PickedColor::from_rgb(100, 100, 100));
        app.set_color(PickedColor::from_rgb(200, 200, 200));
        let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let cmds = frame.commands();
        // Should have more commands with history entries.
        assert!(cmds.len() > 10);
    }

    // -- App state tests ---------------------------------------------------

    #[test]
    fn set_color_adds_to_history() {
        let mut app = ColorPickerApp::create();
        let c = PickedColor::from_rgb(42, 42, 42);
        app.set_color(c);
        assert_eq!(app.history.get(0), Some(&c));
    }

    #[test]
    fn copy_to_clipboard() {
        let mut app = ColorPickerApp::create();
        app.current = PickedColor::from_rgb(255, 0, 0);
        app.active_format = ColorFormat::Hex;
        app.copy_to_clipboard();
        assert_eq!(app.clipboard, "#FF0000");
    }

    #[test]
    fn apply_hex_input_valid() {
        let mut app = ColorPickerApp::create();
        app.hex_input = "#00FF00".to_string();
        assert!(app.apply_hex_input());
        assert_eq!(app.current.r, 0);
        assert_eq!(app.current.g, 255);
        assert_eq!(app.current.b, 0);
    }

    #[test]
    fn apply_hex_input_invalid() {
        let mut app = ColorPickerApp::create();
        app.hex_input = "not-a-color".to_string();
        assert!(!app.apply_hex_input());
    }

    #[test]
    fn save_to_palette() {
        let mut app = ColorPickerApp::create();
        app.current = PickedColor::from_rgb(42, 42, 42);
        assert!(app.save_to_palette("Custom"));
        let palette = &app.palettes[0];
        assert_eq!(
            palette.get("Custom"),
            Some(PickedColor::from_rgb(42, 42, 42))
        );
    }

    #[test]
    fn eyedropper_toggle() {
        let mut app = ColorPickerApp::create();
        assert!(!app.eyedropper.active);
        app.toggle_eyedropper();
        assert!(app.eyedropper.active);
        app.toggle_eyedropper();
        assert!(!app.eyedropper.active);
    }

    #[test]
    fn eyedrop_pick_sets_color_and_deactivates() {
        let mut app = ColorPickerApp::create();
        app.toggle_eyedropper();
        let c = PickedColor::from_rgb(99, 88, 77);
        app.eyedrop_pick(100.0, 200.0, c);
        assert_eq!(app.current, c);
        assert!(!app.eyedropper.active);
    }

    #[test]
    fn set_rgb_components() {
        let mut app = ColorPickerApp::create();
        app.set_r(100);
        app.set_g(150);
        app.set_b(200);
        assert_eq!(app.current.r, 100);
        assert_eq!(app.current.g, 150);
        assert_eq!(app.current.b, 200);
    }

    #[test]
    fn set_from_hsl_preserves_alpha() {
        let mut app = ColorPickerApp::create();
        app.current.a = 128;
        app.set_from_hsl(Hsl {
            h: 0.0,
            s: 1.0,
            l: 0.5,
        });
        assert_eq!(app.current.a, 128);
        assert_eq!(app.current.r, 255);
    }

    #[test]
    fn a_window_too_narrow_for_one_swatch_still_renders() {
        // Both grids index with `i % cells_per_row` over a loop bounded by the
        // number of colors, so a window narrower than one cell used to divide
        // by zero in `render_history` — `render_palette` happened to carry a
        // `.max(1)` and `render_history` did not.
        let mut app = ColorPickerApp::create();
        for i in 0..12u8 {
            app.set_color(PickedColor::from_rgb(i, i, i));
        }
        assert!(!app.history.is_empty(), "history must be non-empty");
        assert!(
            app.palettes.first().is_some_and(|p| !p.colors.is_empty()),
            "default palette must be non-empty"
        );

        // A drag past the left edge, and the degenerate widths on either side
        // of it. Each is a width the user can produce with the mouse.
        for width in [0.0_f32, 1.0, 2.0 * PADDING, 2.0 * PADDING + 1.0, 30.0] {
            let frame = app.frame(width, WINDOW_HEIGHT);
            let cmds = frame.commands();
            assert!(
                !cmds.is_empty(),
                "rendering at width {width} produced nothing"
            );
        }
    }

    #[test]
    fn columns_across_never_returns_zero() {
        // The floor of one cell per row is what keeps the modulo defined; a
        // NaN or negative width saturates to zero on the `as usize` and must
        // land on the same floor rather than wrapping to `usize::MAX`.
        for avail in [f32::NAN, f32::NEG_INFINITY, -1000.0, -1.0, 0.0, 1.0, 27.9] {
            assert_eq!(
                guitk::grid::columns_across(avail, HISTORY_CELL, HISTORY_GAP).get(),
                1,
                "avail {avail} should floor to one cell"
            );
        }
        // And it still counts correctly once a row genuinely fits: the default
        // 600-wide window holds 18 history swatches across.
        assert_eq!(
            guitk::grid::columns_across(WINDOW_WIDTH - 2.0 * PADDING, HISTORY_CELL, HISTORY_GAP)
                .get(),
            18
        );
    }

    // -- Interaction tests -------------------------------------------------

    use guitk::event::{Modifiers, MouseEvent};
    use guitk::probe::{
        click, click_background, control_names, ctrl, is_visible, key, press, rect_of, shift,
        type_str, typing,
    };

    /// A pointer move, which the probe helpers do not cover because most
    /// windows have nothing that tracks one.
    fn move_to(app: &mut ColorPickerApp, x: f32) -> Action {
        app.handle_event(
            &Event::Mouse(MouseEvent {
                x,
                y: 0.0,
                kind: MouseEventKind::Move,
            }),
            ColorPickerApp::SIZE,
        )
    }

    fn release(app: &mut ColorPickerApp) -> Action {
        app.handle_event(
            &Event::Mouse(MouseEvent {
                x: 0.0,
                y: 0.0,
                kind: MouseEventKind::Release(MouseButton::Left),
            }),
            ColorPickerApp::SIZE,
        )
    }

    #[test]
    fn every_kind_of_control_is_drawn_and_therefore_reachable() {
        let app = ColorPickerApp::create();
        let names = control_names(&app);
        for wanted in [
            "Eyedropper",
            "CopyButton",
            "FormatTab",
            "ValueBox",
            "Slider",
            "Harmony",
            "SwapContrast",
            "Swatch",
        ] {
            assert!(
                names.iter().any(|n| n == wanted),
                "{wanted} is never drawn, so nothing can click it: {names:?}"
            );
        }
    }

    #[test]
    fn the_recent_strip_appears_only_once_something_is_in_it() {
        let mut app = ColorPickerApp::create();
        assert!(
            !is_visible(&app, Target::Recent(0)),
            "an empty Recent strip must not draw a swatch to click"
        );
        app.set_color(PickedColor::from_rgb(1, 2, 3));
        assert!(is_visible(&app, Target::Recent(0)));
    }

    #[test]
    fn clicking_a_palette_swatch_adopts_that_colour() {
        let mut app = ColorPickerApp::create();
        let wanted = app
            .palettes
            .first()
            .and_then(|p| p.colors.get(2))
            .map(|(_, c)| *c)
            .expect("the default palette is seeded with eight colours");
        assert_ne!(
            app.current, wanted,
            "the test would prove nothing otherwise"
        );

        assert_eq!(click(&mut app, Target::Swatch(2)), Action::Redraw);
        assert_eq!(app.current, wanted);
        assert!(
            app.status.contains(&wanted.to_hex6()),
            "the status line should name what was picked: {:?}",
            app.status
        );
    }

    #[test]
    fn clicking_a_harmony_swatch_applies_the_colour_that_was_drawn_there() {
        let mut app = ColorPickerApp::create();
        // Read the colour out of the same function the renderer draws from,
        // which is the whole reason there is only one such function.
        let (label, wanted) = app.harmonies()[3];

        assert_eq!(click(&mut app, Target::Harmony(3)), Action::Redraw);
        assert_eq!(app.current, wanted);
        assert!(app.status.starts_with(label), "status: {:?}", app.status);
    }

    #[test]
    fn a_slider_press_jumps_the_value_to_where_it_was_clicked() {
        let mut app = ColorPickerApp::create();
        let row = rect_of(&app, Target::Slider(Channel::R)).expect("the R slider is drawn");
        let track = ColorPickerApp::track_rect(row.x, row.y, row.w);

        // Two thirds along the track is 170 of 255, and no thumb is there to
        // grab: pressing a track and having nothing happen is the bug this
        // guards.
        app.handle_click(
            track.x + track.w * (2.0 / 3.0),
            row.y + row.h / 2.0,
            MouseButton::Left,
            ColorPickerApp::SIZE,
        );
        assert!(
            (f32::from(app.current.r) - 170.0).abs() <= 1.0,
            "expected about 170, got {}",
            app.current.r
        );
    }

    #[test]
    fn a_drag_moves_only_the_channel_it_started_on() {
        let mut app = ColorPickerApp::create();
        let before = (app.current.g, app.current.b);
        let row = rect_of(&app, Target::Slider(Channel::R)).expect("the R slider is drawn");
        let track = ColorPickerApp::track_rect(row.x, row.y, row.w);

        app.handle_click(
            track.x,
            row.y + row.h / 2.0,
            MouseButton::Left,
            ColorPickerApp::SIZE,
        );
        assert_eq!(app.current.r, 0, "the press should have taken the left end");

        // The pointer wanders across the whole window, well past both ends of
        // the track and over the rows above and below it. None of that may
        // hand the drag to another slider.
        for x in [track.x + track.w, -500.0, 5000.0, track.x + track.w / 2.0] {
            move_to(&mut app, x);
        }
        assert_eq!((app.current.g, app.current.b), before);
        assert!(
            (f32::from(app.current.r) - 127.0).abs() <= 2.0,
            "expected about half, got {}",
            app.current.r
        );
    }

    #[test]
    fn a_drag_records_one_colour_in_recent_when_it_ends_and_none_before() {
        let mut app = ColorPickerApp::create();
        let row = rect_of(&app, Target::Slider(Channel::G)).expect("the G slider is drawn");
        let track = ColorPickerApp::track_rect(row.x, row.y, row.w);

        app.handle_click(
            track.x,
            row.y + row.h / 2.0,
            MouseButton::Left,
            ColorPickerApp::SIZE,
        );
        for step in 1..=8_u8 {
            move_to(&mut app, track.x + track.w * (f32::from(step) / 8.0_f32));
        }
        assert_eq!(
            app.history.len(),
            0,
            "a drag in progress must not fill Recent with the colours it scrubbed past"
        );

        assert_eq!(release(&mut app), Action::Redraw);
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.history.iter().next().copied(), Some(app.current));

        // A second release with no drag under it is not a second pick.
        assert_eq!(release(&mut app), Action::None);
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn a_pointer_that_leaves_the_window_ends_the_drag() {
        let mut app = ColorPickerApp::create();
        let row = rect_of(&app, Target::Slider(Channel::B)).expect("the B slider is drawn");
        let track = ColorPickerApp::track_rect(row.x, row.y, row.w);
        app.handle_click(
            track.x,
            row.y + row.h / 2.0,
            MouseButton::Left,
            ColorPickerApp::SIZE,
        );

        app.handle_event(
            &Event::Mouse(MouseEvent {
                x: -1.0,
                y: -1.0,
                kind: MouseEventKind::Leave,
            }),
            ColorPickerApp::SIZE,
        );
        let parked = app.current;
        // Moves that arrive after the pointer left are not ours to act on.
        move_to(&mut app, track.x + track.w);
        assert_eq!(app.current, parked);
    }

    #[test]
    fn the_value_box_takes_hex_digits_and_refuses_everything_else() {
        let mut app = ColorPickerApp::create();
        click(&mut app, Target::ValueBox);
        assert!(app.editing, "clicking the box should focus it");
        assert_eq!(
            app.hex_input,
            app.current.to_hex6(),
            "the field is seeded so a one-digit change does not mean retyping six"
        );

        for _ in 0..7 {
            key(&mut app, &press(Key::Backspace));
        }
        assert!(app.hex_input.is_empty());
        assert_eq!(
            key(&mut app, &press(Key::Backspace)),
            Action::None,
            "backspace on an empty field changes nothing and must not repaint"
        );

        type_str(&mut app, "1z2Q3g");
        assert_eq!(
            app.hex_input, "123",
            "a field that cannot hold a non-hex character has no state in which it does"
        );
        type_str(&mut app, "456");
        assert_eq!(key(&mut app, &press(Key::Enter)), Action::Redraw);
        assert!(!app.editing);
        assert_eq!(app.current, PickedColor::from_rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn a_dead_key_that_types_several_characters_delivers_all_of_them() {
        let mut app = ColorPickerApp::create();
        click(&mut app, Target::ValueBox);
        for _ in 0..7 {
            key(&mut app, &press(Key::Backspace));
        }
        // One keystroke, three characters - the shape a composed dead key or
        // a text-input paste arrives in.
        key(&mut app, &typing("abc"));
        assert_eq!(app.hex_input, "ABC");
    }

    #[test]
    fn a_bad_hex_string_keeps_the_field_open_with_the_text_still_in_it() {
        let mut app = ColorPickerApp::create();
        let before = app.current;
        click(&mut app, Target::ValueBox);
        for _ in 0..7 {
            key(&mut app, &press(Key::Backspace));
        }
        type_str(&mut app, "12345");
        key(&mut app, &press(Key::Enter));

        assert_eq!(
            app.current, before,
            "an unparseable colour must not be applied"
        );
        assert!(
            app.editing,
            "clearing the field would destroy what they typed"
        );
        assert_eq!(app.hex_input, "12345");
        assert!(app.status.contains("12345"), "status: {:?}", app.status);
    }

    #[test]
    fn escape_abandons_the_edit_before_it_closes_the_window() {
        let mut app = ColorPickerApp::create();
        let before = app.current;
        click(&mut app, Target::ValueBox);
        type_str(&mut app, "F");

        assert_eq!(key(&mut app, &press(Key::Escape)), Action::Redraw);
        assert!(!app.editing);
        assert_eq!(app.current, before);
        // Only now does Escape mean "close".
        assert_eq!(key(&mut app, &press(Key::Escape)), Action::Quit);
    }

    #[test]
    fn clicking_away_from_the_value_box_abandons_the_edit() {
        let mut app = ColorPickerApp::create();
        click(&mut app, Target::ValueBox);
        type_str(&mut app, "FF");
        click(&mut app, Target::FormatTab(ColorFormat::Rgb));
        assert!(!app.editing);
        assert!(app.hex_input.is_empty());

        // And so does a click on nothing at all.
        click(&mut app, Target::ValueBox);
        assert!(app.editing);
        assert_eq!(click_background(&mut app), Action::Redraw);
        assert!(!app.editing);
    }

    #[test]
    fn an_armed_eyedropper_takes_the_click_before_any_control_does() {
        let mut app = ColorPickerApp::create();
        let wanted = app
            .palettes
            .first()
            .and_then(|p| p.colors.get(1))
            .map(|(_, c)| *c)
            .expect("the default palette is seeded");

        assert_eq!(click(&mut app, Target::Eyedropper), Action::Redraw);
        assert!(app.eyedropper.active);

        click(&mut app, Target::Swatch(1));
        assert_eq!(
            app.current, wanted,
            "the sample should be what was under the pointer"
        );
        assert!(
            app.status.starts_with("Sampled"),
            "a sample is not a pick, and should not read like one: {:?}",
            app.status
        );
        assert!(!app.eyedropper.active, "one click is one sample");
    }

    #[test]
    fn escape_disarms_the_eyedropper_before_it_closes_the_window() {
        let mut app = ColorPickerApp::create();
        click(&mut app, Target::Eyedropper);
        assert_eq!(key(&mut app, &press(Key::Escape)), Action::Redraw);
        assert!(!app.eyedropper.active);
        assert_eq!(key(&mut app, &press(Key::Escape)), Action::Quit);
    }

    #[test]
    fn tab_cycles_the_format_tabs_and_shift_tab_cycles_back() {
        let mut app = ColorPickerApp::create();
        let first = app.active_format;
        for _ in 0..ColorFormat::ALL.len() {
            key(&mut app, &press(Key::Tab));
        }
        assert_eq!(app.active_format, first, "a full cycle should return home");

        key(&mut app, &press(Key::Tab));
        let second = app.active_format;
        assert_ne!(second, first);
        key(&mut app, &shift(Key::Tab));
        assert_eq!(app.active_format, first);
    }

    #[test]
    fn clicking_the_tab_that_is_already_active_asks_for_no_repaint() {
        let mut app = ColorPickerApp::create();
        let active = app.active_format;
        assert_eq!(click(&mut app, Target::FormatTab(active)), Action::None);
    }

    #[test]
    fn the_arrow_keys_nudge_the_channels_the_active_format_makes_primary() {
        let mut app = ColorPickerApp::create();
        app.active_format = ColorFormat::Rgb;
        let before = app.current;
        key(&mut app, &press(Key::Right));
        assert_eq!(app.current.r, before.r.saturating_add(1));
        assert_eq!((app.current.g, app.current.b), (before.g, before.b));

        key(&mut app, &press(Key::Up));
        assert_eq!(app.current.g, before.g.saturating_add(1));

        // Under an HSL format the same keys move hue, not red.
        app.active_format = ColorFormat::Hsl;
        let hue_before = app.current.to_hsl().h;
        key(&mut app, &press(Key::Right));
        let hue_after = app.current.to_hsl().h;
        assert!(
            (hue_after - hue_before).abs() > 0.4,
            "hue should have moved: {hue_before} -> {hue_after}"
        );
    }

    #[test]
    fn a_nudge_at_the_end_of_a_channel_does_nothing_rather_than_wrapping() {
        let mut app = ColorPickerApp::create();
        app.active_format = ColorFormat::Rgb;
        app.current = PickedColor::from_rgb(0, 0, 0);
        assert_eq!(
            key(&mut app, &press(Key::Left)),
            Action::None,
            "red is already at zero, and must not wrap round to 255"
        );
        assert_eq!(app.current.r, 0);

        app.current = PickedColor::from_rgb(255, 0, 0);
        assert_eq!(key(&mut app, &press(Key::Right)), Action::None);
        assert_eq!(app.current.r, 255);
    }

    #[test]
    fn key_releases_do_not_repeat_the_action_of_their_press() {
        let mut app = ColorPickerApp::create();
        let mut event = press(Key::Tab);
        event.pressed = false;
        let before = app.active_format;
        assert_eq!(key(&mut app, &event), Action::None);
        assert_eq!(app.active_format, before);
    }

    #[test]
    fn ctrl_c_copies_the_value_the_active_tab_is_showing() {
        let mut app = ColorPickerApp::create();
        app.active_format = ColorFormat::Rgb;
        assert_eq!(key(&mut app, &ctrl(Key::C)), Action::Redraw);
        assert_eq!(app.clipboard, app.current.format_as(ColorFormat::Rgb));
        assert!(
            app.status.contains(&app.clipboard),
            "status: {:?}",
            app.status
        );

        // The button and the binding are the same action, not two.
        let mut clicked = ColorPickerApp::create();
        clicked.active_format = ColorFormat::Rgb;
        click(&mut clicked, Target::CopyButton);
        assert_eq!(clicked.clipboard, app.clipboard);
    }

    #[test]
    fn ctrl_e_arms_the_eyedropper_and_ctrl_v_opens_the_value_box() {
        let mut app = ColorPickerApp::create();
        key(&mut app, &ctrl(Key::E));
        assert!(app.eyedropper.active);
        key(&mut app, &ctrl(Key::E));
        assert!(!app.eyedropper.active);

        key(&mut app, &ctrl(Key::V));
        assert!(
            app.editing,
            "paste is the same gesture as typing into the box"
        );
    }

    #[test]
    fn swapping_the_contrast_pair_exchanges_them_without_filling_recent() {
        let mut app = ColorPickerApp::create();
        let (fg, bg) = (app.current, app.contrast_bg);
        assert_eq!(click(&mut app, Target::SwapContrast), Action::Redraw);
        assert_eq!((app.current, app.contrast_bg), (bg, fg));
        assert_eq!(
            app.history.len(),
            0,
            "a swap is not a pick, and should not appear in Recent"
        );
    }

    #[test]
    fn clicking_a_recent_swatch_brings_that_colour_back() {
        let mut app = ColorPickerApp::create();
        let first = PickedColor::from_rgb(10, 20, 30);
        app.set_color(first);
        app.set_color(PickedColor::from_rgb(200, 100, 50));
        assert_ne!(app.current, first);

        // Newest first, so the one before last is at index 1.
        click(&mut app, Target::Recent(1));
        assert_eq!(app.current, first);
    }

    #[test]
    fn a_right_click_is_not_a_left_click() {
        let mut app = ColorPickerApp::create();
        let before = app.current;
        let rect = rect_of(&app, Target::Swatch(2)).expect("the palette is drawn");
        let (cx, cy) = rect.centre();
        assert_eq!(
            app.handle_click(cx, cy, MouseButton::Right, ColorPickerApp::SIZE),
            Action::None
        );
        assert_eq!(app.current, before);
    }

    #[test]
    fn the_window_draws_at_the_size_it_is_handed_not_the_one_it_remembers() {
        let mut app = ColorPickerApp::create();
        // The very first frame goes out before any resize arrives, so a
        // renderer that trusted the remembered size would lay this one out
        // for a window nobody is looking at.
        let tree = app.render(WINDOW_WIDTH * 2.0, WINDOW_HEIGHT);
        assert!(!tree.is_empty());
        assert_eq!(app.window_size, (WINDOW_WIDTH * 2.0, WINDOW_HEIGHT));

        // And a click is hit-tested against that same wider layout: the tabs
        // stretch, so the last one now starts further to the right than it
        // did in the narrow window.
        let wide = (WINDOW_WIDTH * 2.0, WINDOW_HEIGHT);
        let last = ColorFormat::ALL[ColorFormat::ALL.len() - 1];
        let narrow_rect = app
            .rect_of(Target::FormatTab(last), ColorPickerApp::SIZE)
            .expect("tabs are drawn");
        let wide_rect = app
            .rect_of(Target::FormatTab(last), wide)
            .expect("tabs are drawn");
        assert!(
            wide_rect.x > narrow_rect.x,
            "the tabs should have stretched"
        );
    }

    #[test]
    fn a_resize_is_remembered_so_the_next_click_lands_where_the_user_sees_it() {
        let mut app = ColorPickerApp::create();
        assert_eq!(
            app.on_event(&Event::Resize {
                width: 900,
                height: 700
            }),
            Response::Redraw
        );
        assert_eq!(app.window_size, (900.0, 700.0));
    }

    #[test]
    fn the_close_button_closes_the_window() {
        let mut app = ColorPickerApp::create();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn an_event_that_changes_nothing_does_not_ask_for_a_repaint() {
        let mut app = ColorPickerApp::create();
        // A move with no drag under it is the commonest event a window sees,
        // and repainting for each one would keep the machine awake.
        assert_eq!(
            app.on_event(&Event::Mouse(MouseEvent {
                x: 10.0,
                y: 10.0,
                kind: MouseEventKind::Move,
            })),
            Response::Idle
        );
    }

    #[test]
    fn the_status_line_is_drawn_once_there_is_something_to_say() {
        let app = ColorPickerApp::create();
        assert!(app.status.is_empty());
        let quiet = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len();

        let mut app = ColorPickerApp::create();
        click(&mut app, Target::Eyedropper);
        let loud = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let loud = loud.commands();
        assert!(
            loud.len() > quiet,
            "the status line should have added a command"
        );
        assert!(
            loud.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == &app.status
            )),
            "the status the handler set is not the status on screen"
        );
    }

    #[test]
    fn no_modifier_is_needed_to_type_into_the_value_box() {
        // A `ctrl` chord must not reach the field as text: Ctrl+C while the
        // box is focused would otherwise type a `C`.
        let mut app = ColorPickerApp::create();
        click(&mut app, Target::ValueBox);
        for _ in 0..7 {
            key(&mut app, &press(Key::Backspace));
        }
        let mut chord = press(Key::C);
        chord.modifiers = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        key(&mut app, &chord);
        assert!(
            app.hex_input.is_empty(),
            "a chord typed no text, so nothing should have been appended"
        );
    }
}
