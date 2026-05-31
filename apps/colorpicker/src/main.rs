//! OurOS Color Picker — system-wide color picker / eyedropper utility.
//!
//! A PowerToys-style color picker that allows picking colors from the screen,
//! converting between formats (Hex, RGB, HSL, HSV, CMYK), managing palettes,
//! tracking color history, suggesting harmonies, and checking WCAG contrast.
//!
//! Renders via guitk into a 600x700 window using the Catppuccin Mocha dark theme.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

#[allow(dead_code)]
mod mocha {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
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
                Some(Self::from_rgb(r * 17, g * 17, b * 17))
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
        let l = (max + min) / 2.0;

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
#[allow(dead_code)]
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
    /// Window dimensions.
    width: f32,
    height: f32,
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
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// Set the current color and record it in history.
    pub fn set_color(&mut self, color: PickedColor) {
        self.current = color;
        self.history.push(color);
    }

    /// Copy the current color to clipboard in the active format.
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

    // -- Rendering ---------------------------------------------------------

    /// Render the entire color picker UI into a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: mocha::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut y = PADDING;

        // Title bar
        self.render_title(&mut cmds, &mut y);

        // Large color swatch preview
        self.render_swatch(&mut cmds, &mut y);

        // Format tabs
        self.render_format_tabs(&mut cmds, &mut y);

        // Format value display
        self.render_format_values(&mut cmds, &mut y);

        // RGB sliders
        self.render_rgb_sliders(&mut cmds, &mut y);

        // HSL sliders
        self.render_hsl_sliders(&mut cmds, &mut y);

        // Harmony suggestions
        self.render_harmonies(&mut cmds, &mut y);

        // Contrast checker
        self.render_contrast_panel(&mut cmds, &mut y);

        // History strip
        self.render_history(&mut cmds, &mut y);

        // Palette grid
        self.render_palette(&mut cmds, &mut y);

        cmds
    }

    fn render_title(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Color Picker".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE_LARGE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Eyedropper button
        let btn_x = self.width - PADDING - 100.0;
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
        cmds.push(RenderCommand::Text {
            x: btn_x + 10.0,
            y: *y + 1.0,
            text: "Eyedropper".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        *y += 28.0;
    }

    fn render_swatch(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
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
                max_width: Some(self.width - info_x - PADDING),
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
        cmds.push(RenderCommand::Text {
            x: info_x + 12.0,
            y: copy_y + 3.0,
            text: "Copy".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        *y += SWATCH_SIZE + PADDING;
    }

    fn render_format_tabs(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        let tab_width = (self.width - 2.0 * PADDING) / ColorFormat::ALL.len() as f32;

        // Tab bar background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: *y,
            width: self.width - 2.0 * PADDING,
            height: TAB_HEIGHT,
            color: mocha::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        for (i, fmt) in ColorFormat::ALL.iter().enumerate() {
            let tab_x = PADDING + i as f32 * tab_width;
            let is_active = *fmt == self.active_format;

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
            });
        }

        *y += TAB_HEIGHT + PADDING;
    }

    fn render_format_values(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        let formatted = self.current.format_as(self.active_format);

        // Input field background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: *y,
            width: self.width - 2.0 * PADDING,
            height: 30.0,
            color: mocha::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: PADDING,
            y: *y,
            width: self.width - 2.0 * PADDING,
            height: 30.0,
            color: mocha::OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: PADDING + 8.0,
            y: *y + 7.0,
            text: formatted,
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - 2.0 * PADDING - 16.0),
        });

        *y += 30.0 + PADDING;
    }

    fn render_slider(
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: f32,
        max_val: f32,
        track_color: Color,
    ) {
        // Label
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: label.to_string(),
            color: mocha::SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let track_x = x + 36.0;
        let track_w = width - 80.0;
        let track_y = y + (SLIDER_HEIGHT - SLIDER_TRACK_HEIGHT) / 2.0;

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
        });
    }

    fn render_rgb_sliders(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "RGB".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        *y += 18.0;

        let slider_w = self.width - 2.0 * PADDING;
        Self::render_slider(
            cmds,
            PADDING,
            *y,
            slider_w,
            "R",
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
            "G",
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
            "B",
            self.current.b as f32,
            255.0,
            Color::rgb(60, 60, self.current.b),
        );
        *y += SLIDER_HEIGHT + PADDING;
    }

    fn render_hsl_sliders(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        let hsl = self.current.to_hsl();

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "HSL".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        *y += 18.0;

        let slider_w = self.width - 2.0 * PADDING;
        Self::render_slider(cmds, PADDING, *y, slider_w, "H", hsl.h, 360.0, mocha::MAUVE);
        *y += SLIDER_HEIGHT + 4.0;

        Self::render_slider(cmds, PADDING, *y, slider_w, "S", hsl.s, 1.0, mocha::PEACH);
        *y += SLIDER_HEIGHT + 4.0;

        Self::render_slider(cmds, PADDING, *y, slider_w, "L", hsl.l, 1.0, mocha::YELLOW);
        *y += SLIDER_HEIGHT + PADDING;
    }

    fn render_harmonies(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Harmony".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        *y += 18.0;

        let comp = self.current.complementary();
        let (ana_l, ana_r) = self.current.analogous();
        let (tri_a, tri_b) = self.current.triadic();

        let harmony_colors: &[(&str, PickedColor)] = &[
            ("Comp", comp),
            ("Ana-", ana_l),
            ("Ana+", ana_r),
            ("Tri1", tri_a),
            ("Tri2", tri_b),
        ];

        let cell_w = 48.0;
        let cell_h = 36.0;
        let gap = 8.0;

        for (i, (label, color)) in harmony_colors.iter().enumerate() {
            let cx = PADDING + i as f32 * (cell_w + gap);

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
            });
        }

        *y += cell_h + PADDING;
    }

    fn render_contrast_panel(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Contrast Checker".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        *y += 18.0;

        let ratio = contrast_ratio(self.current, self.contrast_bg);
        let level = wcag_level(ratio);

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: *y,
            width: self.width - 2.0 * PADDING,
            height: CONTRAST_PANEL_HEIGHT,
            color: mocha::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        // Foreground swatch
        let sw = 40.0;
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
        });

        *y += CONTRAST_PANEL_HEIGHT + PADDING;
    }

    fn render_history(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: *y,
            text: "Recent".to_string(),
            color: mocha::TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        *y += 18.0;

        // Calculate how many cells fit in one row.
        let avail_w = self.width - 2.0 * PADDING;
        let cells_per_row = ((avail_w + HISTORY_GAP) / (HISTORY_CELL + HISTORY_GAP)) as usize;

        for (i, color) in self.history.iter().enumerate() {
            let col = i % cells_per_row;
            let row = i / cells_per_row;
            if row > 1 {
                break; // Only show two rows of history.
            }
            let cx = PADDING + col as f32 * (HISTORY_CELL + HISTORY_GAP);
            let cy = *y + row as f32 * (HISTORY_CELL + HISTORY_GAP);

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

        let rows_shown = if self.history.len() > cells_per_row {
            2
        } else if self.history.is_empty() {
            0
        } else {
            1
        };
        *y += rows_shown as f32 * (HISTORY_CELL + HISTORY_GAP) + PADDING;
    }

    fn render_palette(&self, cmds: &mut Vec<RenderCommand>, y: &mut f32) {
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
        });
        *y += 18.0;

        let avail_w = self.width - 2.0 * PADDING;
        let cells_per_row = ((avail_w + PALETTE_GAP) / (PALETTE_CELL + PALETTE_GAP)) as usize;

        for (i, (name, color)) in palette.colors.iter().enumerate() {
            let col = i % cells_per_row.max(1);
            let row = i / cells_per_row.max(1);
            let cx = PADDING + col as f32 * (PALETTE_CELL + PALETTE_GAP);
            let cy = *y + row as f32 * (PALETTE_CELL + PALETTE_GAP + 14.0);

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
            });
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_starts_with_background() {
        let app = ColorPickerApp::create();
        let cmds = app.render();
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
        let cmds = app.render();
        let has_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. }));
        assert!(has_text, "should contain Text commands");
    }

    #[test]
    fn render_with_history() {
        let mut app = ColorPickerApp::create();
        app.set_color(PickedColor::from_rgb(100, 100, 100));
        app.set_color(PickedColor::from_rgb(200, 200, 200));
        let cmds = app.render();
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
}
