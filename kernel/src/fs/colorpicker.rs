//! System-wide color picker utility.
//!
//! Provides a reusable color picker widget backend that any application can
//! invoke. Supports HSV/HSL/RGB/hex color models, saved palettes, a recent
//! colors history, and an eyedropper (screen color sampling) API.
//!
//! ## Design Reference
//!
//! design.txt line 777: "color picker like what's in qtpyrc"
//!
//! ## Architecture
//!
//! ```text
//! Application / theme settings
//!   → colorpicker::open_picker(initial_color)
//!   → colorpicker::set_hsv(h, s, v)
//!   → colorpicker::confirm() → Color
//!
//! Eyedropper tool
//!   → colorpicker::sample_screen(x, y) → Color
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum saved palette colors.
const MAX_PALETTE: usize = 256;

/// Maximum custom palettes.
const MAX_PALETTES: usize = 32;

/// Maximum recent colors.
const MAX_RECENT: usize = 32;

/// Maximum concurrent picker instances.
const MAX_PICKERS: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// RGBA color (0-255 per channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parse "#RRGGBB" or "#RRGGBBAA" hex string.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() == 6 {
            let r = u8::from_str_radix(s.get(0..2)?, 16).ok()?;
            let g = u8::from_str_radix(s.get(2..4)?, 16).ok()?;
            let b = u8::from_str_radix(s.get(4..6)?, 16).ok()?;
            Some(Self::rgb(r, g, b))
        } else if s.len() == 8 {
            let r = u8::from_str_radix(s.get(0..2)?, 16).ok()?;
            let g = u8::from_str_radix(s.get(2..4)?, 16).ok()?;
            let b = u8::from_str_radix(s.get(4..6)?, 16).ok()?;
            let a = u8::from_str_radix(s.get(6..8)?, 16).ok()?;
            Some(Self::rgba(r, g, b, a))
        } else {
            None
        }
    }

    /// Format as "#RRGGBB" (no alpha) or "#RRGGBBAA" (with alpha).
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            alloc::format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            alloc::format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Convert to HSV (hue 0-360, saturation 0-100, value 0-100).
    pub fn to_hsv(self) -> (u16, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let max = if r >= g && r >= b { r } else if g >= b { g } else { b };
        let min = if r <= g && r <= b { r } else if g <= b { g } else { b };
        let delta = max - min;

        let v = (max * 100.0) as u8;

        if delta < 0.00001 {
            return (0, 0, v);
        }

        let s = ((delta / max) * 100.0) as u8;

        let h = if (r - max).abs() < 0.00001 {
            60.0 * (((g - b) / delta) % 6.0)
        } else if (g - max).abs() < 0.00001 {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
        let h = if h < 0.0 { h + 360.0 } else { h };

        (h as u16, s, v)
    }

    /// Create from HSV (hue 0-360, saturation 0-100, value 0-100).
    pub fn from_hsv(h: u16, s: u8, v: u8) -> Self {
        let h = (h % 360) as f64;
        let s = (s.min(100) as f64) / 100.0;
        let v = (v.min(100) as f64) / 100.0;

        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;

        let (r1, g1, b1) = if h < 60.0 {
            (c, x, 0.0)
        } else if h < 120.0 {
            (x, c, 0.0)
        } else if h < 180.0 {
            (0.0, c, x)
        } else if h < 240.0 {
            (0.0, x, c)
        } else if h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Self::rgb(
            ((r1 + m) * 255.0) as u8,
            ((g1 + m) * 255.0) as u8,
            ((b1 + m) * 255.0) as u8,
        )
    }

    /// Convert to HSL (hue 0-360, saturation 0-100, lightness 0-100).
    pub fn to_hsl(self) -> (u16, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let max = if r >= g && r >= b { r } else if g >= b { g } else { b };
        let min = if r <= g && r <= b { r } else if g <= b { g } else { b };
        let delta = max - min;
        let l = (max + min) / 2.0;

        if delta < 0.00001 {
            return (0, 0, (l * 100.0) as u8);
        }

        let s = if l < 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        let h = if (r - max).abs() < 0.00001 {
            60.0 * (((g - b) / delta) % 6.0)
        } else if (g - max).abs() < 0.00001 {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
        let h = if h < 0.0 { h + 360.0 } else { h };

        (h as u16, (s * 100.0) as u8, (l * 100.0) as u8)
    }

    /// Create from HSL (hue 0-360, saturation 0-100, lightness 0-100).
    pub fn from_hsl(h: u16, s: u8, l: u8) -> Self {
        let h = (h % 360) as f64;
        let s = (s.min(100) as f64) / 100.0;
        let l = (l.min(100) as f64) / 100.0;

        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = l - c / 2.0;

        let (r1, g1, b1) = if h < 60.0 {
            (c, x, 0.0)
        } else if h < 120.0 {
            (x, c, 0.0)
        } else if h < 180.0 {
            (0.0, c, x)
        } else if h < 240.0 {
            (0.0, x, c)
        } else if h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Self::rgb(
            ((r1 + m) * 255.0) as u8,
            ((g1 + m) * 255.0) as u8,
            ((b1 + m) * 255.0) as u8,
        )
    }

    /// CMYK values (0-100 each).
    pub fn to_cmyk(self) -> (u8, u8, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;
        let k = 1.0 - if r >= g && r >= b { r } else if g >= b { g } else { b };
        if k >= 1.0 {
            return (0, 0, 0, 100);
        }
        let c = ((1.0 - r - k) / (1.0 - k) * 100.0) as u8;
        let m = ((1.0 - g - k) / (1.0 - k) * 100.0) as u8;
        let y = ((1.0 - b - k) / (1.0 - k) * 100.0) as u8;
        (c, m, y, (k * 100.0) as u8)
    }
}

/// Color model selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorModel {
    Rgb,
    Hsv,
    Hsl,
    Hex,
    Cmyk,
}

impl ColorModel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Hsv => "HSV",
            Self::Hsl => "HSL",
            Self::Hex => "Hex",
            Self::Cmyk => "CMYK",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "rgb" | "RGB" => Some(Self::Rgb),
            "hsv" | "HSV" => Some(Self::Hsv),
            "hsl" | "HSL" => Some(Self::Hsl),
            "hex" | "Hex" | "HEX" => Some(Self::Hex),
            "cmyk" | "CMYK" => Some(Self::Cmyk),
            _ => None,
        }
    }
}

/// Named palette of saved colors.
#[derive(Debug, Clone)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<Color>,
}

/// A picker instance (one dialog).
#[derive(Debug, Clone)]
pub struct PickerInstance {
    pub id: u64,
    /// Currently selected color.
    pub color: Color,
    /// Color model being displayed.
    pub model: ColorModel,
    /// Whether alpha channel is enabled.
    pub alpha_enabled: bool,
    /// Initial color (for "revert" action).
    pub initial_color: Color,
    /// Whether the picker has been confirmed.
    pub confirmed: bool,
    /// Timestamp of creation.
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    /// Active picker instances.
    pickers: BTreeMap<u64, PickerInstance>,
    /// Named color palettes.
    palettes: Vec<Palette>,
    /// Recent colors (most recent first).
    recent: Vec<Color>,
    /// Screen sample result (set by eyedropper).
    last_sample: Option<Color>,
}

impl State {
    const fn new() -> Self {
        Self {
            pickers: BTreeMap::new(),
            palettes: Vec::new(),
            recent: Vec::new(),
            last_sample: None,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static PICK_COUNT: AtomicU64 = AtomicU64::new(0);
static SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Picker lifecycle
// ---------------------------------------------------------------------------

/// Open a new color picker with an initial color.
pub fn open_picker(initial: Color, alpha: bool) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.pickers.len() >= MAX_PICKERS {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ts = crate::hpet::elapsed_ns();
    state.pickers.insert(id, PickerInstance {
        id,
        color: initial,
        model: ColorModel::Hsv,
        alpha_enabled: alpha,
        initial_color: initial,
        confirmed: false,
        created_ns: ts,
    });
    Ok(id)
}

/// Set the color model for a picker.
pub fn set_model(picker_id: u64, model: ColorModel) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    p.model = model;
    Ok(())
}

/// Set color via RGB.
pub fn set_rgb(picker_id: u64, r: u8, g: u8, b: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    p.color = Color::rgb(r, g, b);
    Ok(())
}

/// Set color via RGBA.
pub fn set_rgba(picker_id: u64, r: u8, g: u8, b: u8, a: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    p.color = Color::rgba(r, g, b, a);
    Ok(())
}

/// Set color via HSV.
pub fn set_hsv(picker_id: u64, h: u16, s: u8, v: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    let a = p.color.a;
    p.color = Color::from_hsv(h, s, v);
    p.color.a = a;
    Ok(())
}

/// Set color via HSL.
pub fn set_hsl(picker_id: u64, h: u16, s: u8, l: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    let a = p.color.a;
    p.color = Color::from_hsl(h, s, l);
    p.color.a = a;
    Ok(())
}

/// Set color via hex string.
pub fn set_hex(picker_id: u64, hex: &str) -> KernelResult<()> {
    let c = Color::from_hex(hex).ok_or(KernelError::InvalidArgument)?;
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    p.color = c;
    Ok(())
}

/// Set alpha channel.
pub fn set_alpha(picker_id: u64, a: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    p.color.a = a;
    Ok(())
}

/// Revert to initial color.
pub fn revert(picker_id: u64) -> KernelResult<Color> {
    let mut state = STATE.lock();
    let p = state.pickers.get_mut(&picker_id).ok_or(KernelError::NotFound)?;
    p.color = p.initial_color;
    Ok(p.color)
}

/// Get current color of a picker.
pub fn current_color(picker_id: u64) -> KernelResult<Color> {
    let state = STATE.lock();
    let p = state.pickers.get(&picker_id).ok_or(KernelError::NotFound)?;
    Ok(p.color)
}

/// Get picker info.
pub fn get_picker(picker_id: u64) -> KernelResult<PickerInstance> {
    let state = STATE.lock();
    let p = state.pickers.get(&picker_id).ok_or(KernelError::NotFound)?;
    Ok(p.clone())
}

/// Confirm selection and close picker. Returns the selected color.
pub fn confirm(picker_id: u64) -> KernelResult<Color> {
    let mut state = STATE.lock();
    let p = state.pickers.remove(&picker_id).ok_or(KernelError::NotFound)?;
    PICK_COUNT.fetch_add(1, Ordering::Relaxed);

    // Add to recent colors.
    state.recent.retain(|c| *c != p.color);
    state.recent.insert(0, p.color);
    if state.recent.len() > MAX_RECENT {
        state.recent.truncate(MAX_RECENT);
    }

    Ok(p.color)
}

/// Cancel picker without selecting.
pub fn cancel(picker_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.pickers.remove(&picker_id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Close a picker (same as cancel but doesn't return error if missing).
pub fn close(picker_id: u64) {
    STATE.lock().pickers.remove(&picker_id);
}

// ---------------------------------------------------------------------------
// Eyedropper (screen color sampling)
// ---------------------------------------------------------------------------

/// Sample a color from screen coordinates.
///
/// In a real implementation, this reads the framebuffer pixel.
/// For now, we simulate by returning a color derived from coordinates.
pub fn sample_screen(x: u32, y: u32) -> Color {
    SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);
    // Simulate: derive a color from coordinates for testing.
    let r = (x & 0xFF) as u8;
    let g = (y & 0xFF) as u8;
    let b = ((x.wrapping_add(y)) & 0xFF) as u8;
    let c = Color::rgb(r, g, b);
    STATE.lock().last_sample = Some(c);
    c
}

/// Get the last sampled color.
pub fn last_sample() -> Option<Color> {
    STATE.lock().last_sample
}

// ---------------------------------------------------------------------------
// Palettes
// ---------------------------------------------------------------------------

/// Create a named palette.
pub fn create_palette(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.palettes.len() >= MAX_PALETTES {
        return Err(KernelError::ResourceExhausted);
    }
    if state.palettes.iter().any(|p| p.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    state.palettes.push(Palette {
        name: String::from(name),
        colors: Vec::new(),
    });
    Ok(())
}

/// Remove a palette.
pub fn remove_palette(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.palettes.len();
    state.palettes.retain(|p| p.name != name);
    if state.palettes.len() == len {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// Add a color to a palette.
pub fn palette_add(name: &str, color: Color) -> KernelResult<()> {
    let mut state = STATE.lock();
    let pal = state.palettes.iter_mut().find(|p| p.name == name)
        .ok_or(KernelError::NotFound)?;
    if pal.colors.len() >= MAX_PALETTE {
        return Err(KernelError::ResourceExhausted);
    }
    pal.colors.push(color);
    Ok(())
}

/// Remove a color from a palette by index.
pub fn palette_remove(name: &str, index: usize) -> KernelResult<()> {
    let mut state = STATE.lock();
    let pal = state.palettes.iter_mut().find(|p| p.name == name)
        .ok_or(KernelError::NotFound)?;
    if index >= pal.colors.len() {
        return Err(KernelError::InvalidArgument);
    }
    pal.colors.remove(index);
    Ok(())
}

/// List palettes.
pub fn list_palettes() -> Vec<Palette> {
    STATE.lock().palettes.clone()
}

/// Get a palette by name.
pub fn get_palette(name: &str) -> KernelResult<Palette> {
    let state = STATE.lock();
    state.palettes.iter().find(|p| p.name == name).cloned().ok_or(KernelError::NotFound)
}

/// Recent colors (most recent first).
pub fn recent_colors() -> Vec<Color> {
    STATE.lock().recent.clone()
}

/// Initialize built-in palettes.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.palettes.is_empty() { return; }

    // Basic web colors.
    state.palettes.push(Palette {
        name: String::from("Basic"),
        colors: alloc::vec![
            Color::rgb(0, 0, 0),       // Black
            Color::rgb(255, 255, 255), // White
            Color::rgb(255, 0, 0),     // Red
            Color::rgb(0, 255, 0),     // Green
            Color::rgb(0, 0, 255),     // Blue
            Color::rgb(255, 255, 0),   // Yellow
            Color::rgb(0, 255, 255),   // Cyan
            Color::rgb(255, 0, 255),   // Magenta
            Color::rgb(128, 128, 128), // Gray
            Color::rgb(128, 0, 0),     // Maroon
            Color::rgb(0, 128, 0),     // Dark green
            Color::rgb(0, 0, 128),     // Navy
            Color::rgb(255, 165, 0),   // Orange
            Color::rgb(128, 0, 128),   // Purple
            Color::rgb(0, 128, 128),   // Teal
            Color::rgb(192, 192, 192), // Silver
        ],
    });

    // Material palette accent colors.
    state.palettes.push(Palette {
        name: String::from("Material"),
        colors: alloc::vec![
            Color::rgb(244, 67, 54),   // Red
            Color::rgb(233, 30, 99),   // Pink
            Color::rgb(156, 39, 176),  // Purple
            Color::rgb(103, 58, 183),  // Deep Purple
            Color::rgb(63, 81, 181),   // Indigo
            Color::rgb(33, 150, 243),  // Blue
            Color::rgb(3, 169, 244),   // Light Blue
            Color::rgb(0, 188, 212),   // Cyan
            Color::rgb(0, 150, 136),   // Teal
            Color::rgb(76, 175, 80),   // Green
            Color::rgb(139, 195, 74),  // Light Green
            Color::rgb(205, 220, 57),  // Lime
            Color::rgb(255, 235, 59),  // Yellow
            Color::rgb(255, 193, 7),   // Amber
            Color::rgb(255, 152, 0),   // Orange
            Color::rgb(255, 87, 34),   // Deep Orange
        ],
    });
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (active_pickers, palette_count, recent_count, picks, samples).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let state = STATE.lock();
    (
        state.pickers.len(),
        state.palettes.len(),
        state.recent.len(),
        PICK_COUNT.load(Ordering::Relaxed),
        SAMPLE_COUNT.load(Ordering::Relaxed),
    )
}

pub fn reset_stats() {
    PICK_COUNT.store(0, Ordering::Relaxed);
    SAMPLE_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.pickers.clear();
    state.palettes.clear();
    state.recent.clear();
    state.last_sample = None;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Color conversions.
    serial_println!("  colorpicker::self_test 1: color conversions");
    let c = Color::rgb(255, 0, 0);
    let (h, s, v) = c.to_hsv();
    assert_eq!(h, 0);
    assert!(s >= 99);
    assert!(v >= 99);
    let c2 = Color::from_hsv(0, 100, 100);
    assert_eq!(c2.r, 255);
    assert_eq!(c2.g, 0);
    assert_eq!(c2.b, 0);

    // Test 2: Hex parsing.
    serial_println!("  colorpicker::self_test 2: hex parsing");
    let c3 = Color::from_hex("#ff8000").unwrap();
    assert_eq!(c3.r, 255);
    assert_eq!(c3.g, 128);
    assert_eq!(c3.b, 0);
    assert_eq!(c3.to_hex(), "#ff8000");
    let c4 = Color::from_hex("#00ff0080").unwrap();
    assert_eq!(c4.a, 128);

    // Test 3: Picker lifecycle.
    serial_println!("  colorpicker::self_test 3: picker lifecycle");
    let pid = open_picker(Color::rgb(100, 150, 200), false)?;
    set_rgb(pid, 255, 0, 0)?;
    let cur = current_color(pid)?;
    assert_eq!(cur.r, 255);
    let result = confirm(pid)?;
    assert_eq!(result.r, 255);
    let recent = recent_colors();
    assert!(!recent.is_empty());
    assert_eq!(recent[0].r, 255);

    // Test 4: HSV/HSL set.
    serial_println!("  colorpicker::self_test 4: HSV/HSL");
    let pid2 = open_picker(Color::rgb(0, 0, 0), true)?;
    set_hsv(pid2, 120, 100, 100)?;
    let c5 = current_color(pid2)?;
    assert!(c5.g > 200); // Green.
    set_hsl(pid2, 240, 100, 50)?;
    let c6 = current_color(pid2)?;
    assert!(c6.b > 200); // Blue.
    cancel(pid2)?;

    // Test 5: Palettes.
    serial_println!("  colorpicker::self_test 5: palettes");
    create_palette("test")?;
    palette_add("test", Color::rgb(10, 20, 30))?;
    palette_add("test", Color::rgb(40, 50, 60))?;
    let pal = get_palette("test")?;
    assert_eq!(pal.colors.len(), 2);
    palette_remove("test", 0)?;
    let pal2 = get_palette("test")?;
    assert_eq!(pal2.colors.len(), 1);
    remove_palette("test")?;

    // Test 6: Eyedropper.
    serial_println!("  colorpicker::self_test 6: eyedropper");
    let s1 = sample_screen(100, 200);
    assert_eq!(s1.r, 100);
    assert_eq!(s1.g, 200);
    assert!(last_sample().is_some());

    // Test 7: Built-in palettes.
    serial_println!("  colorpicker::self_test 7: built-in palettes");
    init_defaults();
    let pals = list_palettes();
    assert!(pals.len() >= 2);
    assert!(pals.iter().any(|p| p.name == "Basic"));
    assert!(pals.iter().any(|p| p.name == "Material"));
    assert!(pals[0].colors.len() >= 16);

    let (ap, pc, rc, picks, samples) = stats();
    assert_eq!(ap, 0);
    assert!(pc >= 2);
    assert!(rc > 0);
    assert!(picks > 0);
    assert!(samples > 0);

    clear_all();
    reset_stats();
    serial_println!("  colorpicker: all tests passed");
    Ok(())
}
