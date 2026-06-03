//! DPI and scaling awareness for the GUI toolkit.
//!
//! Provides type-safe logical/physical pixel distinction, per-monitor scale factors,
//! and standardised dimensions so that all widgets render crisply at any display density.
//!
//! # Design
//!
//! Layout and widget code works exclusively in **logical pixels** (density-independent).
//! The rendering backend converts to physical pixels at the last moment using the current
//! [`ScaleContext`]. This keeps layout math simple and ensures identical proportions
//! regardless of display DPI.
//!
//! Standard DPI factors: 1.0 (96 DPI), 1.25, 1.5, 1.75, 2.0, 2.5, 3.0.
//! Custom fractional factors are also supported for non-standard monitors.

#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Pixel types — type-safe distinction between logical and physical pixels
// ---------------------------------------------------------------------------

/// A measurement in logical (density-independent) pixels.
///
/// Layout, widget sizes, and style values are expressed in logical pixels.
/// Convert to [`PhysicalPixel`] via [`logical_to_physical`] before passing to
/// a rendering backend.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct LogicalPixel(pub f32);

/// A measurement in physical (device) pixels — actual screen dots.
///
/// Rendering backends operate in physical pixels. Always an integer since
/// fractional device pixels produce blurry rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysicalPixel(pub u32);

/// Convert a logical pixel value to physical pixels at the given scale factor.
///
/// The result is rounded to the nearest integer to ensure crisp rendering.
#[inline]
pub fn logical_to_physical(lp: LogicalPixel, scale: f32) -> PhysicalPixel {
    // Round to nearest integer for crisp pixel-aligned rendering.
    PhysicalPixel((lp.0 * scale).round() as u32)
}

/// Convert a physical pixel value back to logical pixels at the given scale factor.
#[inline]
pub fn physical_to_logical(pp: PhysicalPixel, scale: f32) -> LogicalPixel {
    if scale == 0.0 {
        return LogicalPixel(0.0);
    }
    LogicalPixel(pp.0 as f32 / scale)
}

// ---------------------------------------------------------------------------
// Scaled geometry types — logical-space geometry with physical conversion
// ---------------------------------------------------------------------------

/// A size in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaledSize {
    pub width: f32,
    pub height: f32,
}

/// A point in logical pixel space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaledPoint {
    pub x: f32,
    pub y: f32,
}

/// A rectangle in logical pixel space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaledRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A rectangle in physical pixel space (integer coordinates).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicalRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A size in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

/// A point in physical pixel space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicalPoint {
    pub x: u32,
    pub y: u32,
}

impl ScaledSize {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Convert to physical size at the given scale factor.
    pub fn to_physical(self, scale: f32) -> PhysicalSize {
        PhysicalSize {
            width: (self.width * scale).round() as u32,
            height: (self.height * scale).round() as u32,
        }
    }

    /// Construct from a physical size at the given scale factor.
    pub fn from_physical(phys: PhysicalSize, scale: f32) -> Self {
        if scale == 0.0 {
            return Self { width: 0.0, height: 0.0 };
        }
        Self {
            width: phys.width as f32 / scale,
            height: phys.height as f32 / scale,
        }
    }
}

impl ScaledPoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Convert to physical point at the given scale factor.
    pub fn to_physical(self, scale: f32) -> PhysicalPoint {
        PhysicalPoint {
            x: (self.x * scale).round() as u32,
            y: (self.y * scale).round() as u32,
        }
    }

    /// Construct from a physical point at the given scale factor.
    pub fn from_physical(phys: PhysicalPoint, scale: f32) -> Self {
        if scale == 0.0 {
            return Self { x: 0.0, y: 0.0 };
        }
        Self {
            x: phys.x as f32 / scale,
            y: phys.y as f32 / scale,
        }
    }
}

impl ScaledRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Convert to physical rectangle at the given scale factor.
    pub fn to_physical(self, scale: f32) -> PhysicalRect {
        PhysicalRect {
            x: (self.x * scale).round() as u32,
            y: (self.y * scale).round() as u32,
            w: (self.w * scale).round() as u32,
            h: (self.h * scale).round() as u32,
        }
    }

    /// Construct from a physical rectangle at the given scale factor.
    pub fn from_physical(phys: PhysicalRect, scale: f32) -> Self {
        if scale == 0.0 {
            return Self { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
        }
        Self {
            x: phys.x as f32 / scale,
            y: phys.y as f32 / scale,
            w: phys.w as f32 / scale,
            h: phys.h as f32 / scale,
        }
    }

    /// The right edge in logical space.
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    /// The bottom edge in logical space.
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Whether a logical point is inside this rectangle.
    pub fn contains(self, point: ScaledPoint) -> bool {
        point.x >= self.x
            && point.x < self.x + self.w
            && point.y >= self.y
            && point.y < self.y + self.h
    }
}

// ---------------------------------------------------------------------------
// IconSizeClass — standard icon size buckets
// ---------------------------------------------------------------------------

/// Predefined icon size classes used throughout the toolkit.
///
/// Widgets choose the appropriate class; the scale context resolves it to a
/// concrete pixel size appropriate for the current DPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconSizeClass {
    /// 16px at 1x — toolbar icons, tree-view decorations.
    Small,
    /// 24px at 1x — buttons, menu items.
    Medium,
    /// 32px at 1x — larger buttons, list items.
    Large,
    /// 48px at 1x — application icons, large thumbnails.
    XLarge,
}

impl IconSizeClass {
    /// The base logical size for this class at 1x scale.
    pub const fn base_size(self) -> f32 {
        match self {
            Self::Small => 16.0,
            Self::Medium => 24.0,
            Self::Large => 32.0,
            Self::XLarge => 48.0,
        }
    }

    /// Resolve to physical pixels at the given scale factor.
    pub fn to_physical(self, scale: f32) -> u32 {
        (self.base_size() * scale).round() as u32
    }
}

// ---------------------------------------------------------------------------
// ScaleContext — per-window/per-monitor scaling state
// ---------------------------------------------------------------------------

/// Per-window or per-monitor scaling context.
///
/// Widgets query this context to obtain correctly-scaled dimensions.
/// A separate `font_scale` allows users to independently adjust text size
/// for accessibility without affecting UI geometry.
#[derive(Clone, Debug)]
pub struct ScaleContext {
    /// The overall UI scale factor (1.0 = 96 DPI).
    pub scale_factor: f32,
    /// Independent font scale multiplier (applied on top of `scale_factor`).
    /// 1.0 means fonts scale with the UI; >1.0 makes text proportionally larger.
    pub font_scale: f32,
    /// Which icon size class to use by default.
    pub icon_size_class: IconSizeClass,
    /// Base font size in logical pixels (at 1x scale, before font_scale).
    pub base_font_size: f32,
}

impl Default for ScaleContext {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            font_scale: 1.0,
            icon_size_class: IconSizeClass::Medium,
            base_font_size: 14.0,
        }
    }
}

impl ScaleContext {
    /// Create a new scale context with the given display scale factor.
    pub fn new(scale_factor: f32) -> Self {
        Self {
            scale_factor,
            ..Self::default()
        }
    }

    /// Create a context with both UI scale and independent font scale.
    pub fn with_font_scale(scale_factor: f32, font_scale: f32) -> Self {
        Self {
            scale_factor,
            font_scale,
            ..Self::default()
        }
    }

    /// The effective font size in logical pixels, accounting for both
    /// the display scale factor and the independent font scale multiplier.
    ///
    /// The result is the size to pass to `RenderCommand::Text::font_size`
    /// (which is in logical units; the backend applies the display scale).
    pub fn effective_font_size(&self) -> f32 {
        self.base_font_size * self.font_scale
    }

    /// Scale a base spacing value (e.g., padding, margin) by the display factor.
    ///
    /// Returns a logical-pixel value already adjusted for DPI.
    pub fn spacing(&self, base: f32) -> f32 {
        base * self.scale_factor
    }

    /// Recommended border width for the current scale.
    ///
    /// At 1x this is 1.0; at higher scales it increases but is always at least
    /// 1.0 to remain visible.
    pub fn border_width(&self) -> f32 {
        (1.0_f32 * self.scale_factor).max(1.0)
    }

    /// The default icon size (physical pixels) for this context.
    pub fn icon_size(&self) -> u32 {
        self.icon_size_class.to_physical(self.scale_factor)
    }

    // --- Standard widget dimensions (logical pixels, pre-scaled) ----------

    /// Standard button height, scaled.
    pub fn button_height(&self) -> f32 {
        BUTTON_HEIGHT_BASE * self.scale_factor
    }

    /// Standard horizontal button padding, scaled.
    pub fn button_padding_h(&self) -> f32 {
        BUTTON_PADDING_H_BASE * self.scale_factor
    }

    /// Standard text-input height, scaled.
    pub fn input_height(&self) -> f32 {
        INPUT_HEIGHT_BASE * self.scale_factor
    }

    /// Standard scrollbar width, scaled (minimum 8px logical).
    pub fn scrollbar_width(&self) -> f32 {
        (SCROLLBAR_WIDTH_BASE * self.scale_factor).max(SCROLLBAR_WIDTH_MIN)
    }

    /// Tooltip font size, scaled via both DPI and font scale.
    pub fn tooltip_font_size(&self) -> f32 {
        TOOLTIP_FONT_SIZE_BASE * self.font_scale
    }

    /// Resolve a [`ScalableValue`] using this context's scale factor.
    pub fn resolve(&self, value: &ScalableValue) -> f32 {
        value.resolve(self.scale_factor)
    }
}

// ---------------------------------------------------------------------------
// Standard dimension constants (base values at 1x / 96 DPI)
// ---------------------------------------------------------------------------

/// Button height at 1x scale (logical px).
pub const BUTTON_HEIGHT_BASE: f32 = 32.0;
/// Horizontal padding inside buttons at 1x scale.
pub const BUTTON_PADDING_H_BASE: f32 = 12.0;
/// Text input height at 1x scale.
pub const INPUT_HEIGHT_BASE: f32 = 30.0;
/// Small icon size at 1x.
pub const ICON_SIZE_SM: f32 = 16.0;
/// Medium icon size at 1x.
pub const ICON_SIZE_MD: f32 = 24.0;
/// Large icon size at 1x.
pub const ICON_SIZE_LG: f32 = 32.0;
/// Scrollbar width at 1x scale.
pub const SCROLLBAR_WIDTH_BASE: f32 = 10.0;
/// Minimum scrollbar width (logical px) regardless of scale.
pub const SCROLLBAR_WIDTH_MIN: f32 = 8.0;
/// Tooltip font size at 1x scale.
pub const TOOLTIP_FONT_SIZE_BASE: f32 = 12.0;

// ---------------------------------------------------------------------------
// ScalableValue — values that adapt to the current scale
// ---------------------------------------------------------------------------

/// A value that adapts to the display scale factor in different ways.
///
/// Used for theming and configuration where a value's behavior at different
/// DPI levels is specified declaratively.
#[derive(Clone, Debug, PartialEq)]
pub enum ScalableValue {
    /// A fixed value that does not change with scale factor.
    Fixed(f32),
    /// A base value multiplied by the scale factor.
    Scaled(f32),
    /// A stepped table of (threshold, value) pairs.
    ///
    /// The resolved value is the `value` from the last entry whose `threshold`
    /// is <= the current scale factor. Entries must be sorted by threshold
    /// ascending. If the scale is below the first threshold, the first value
    /// is returned.
    Stepped(Vec<(f32, f32)>),
}

impl ScalableValue {
    /// Resolve this value at the given scale factor.
    pub fn resolve(&self, scale: f32) -> f32 {
        match self {
            Self::Fixed(v) => *v,
            Self::Scaled(base) => base * scale,
            Self::Stepped(table) => {
                if table.is_empty() {
                    return 0.0;
                }
                // Find the last entry whose threshold <= scale.
                let mut result = table[0].1;
                for &(threshold, value) in table {
                    if scale >= threshold {
                        result = value;
                    } else {
                        break;
                    }
                }
                result
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Image scaling helpers
// ---------------------------------------------------------------------------

/// Standard icon sizes available in icon themes (in physical pixels).
const STANDARD_ICON_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];

/// Snap a target pixel size to the nearest standard icon size.
///
/// If the target is exactly between two sizes, rounds up (prefer the larger
/// icon to avoid downscaling artifacts).
pub fn nearest_icon_size(target: u32) -> u32 {
    if target == 0 {
        return STANDARD_ICON_SIZES[0];
    }
    let mut best = STANDARD_ICON_SIZES[0];
    let mut best_dist = (target as i64 - best as i64).unsigned_abs();
    for &size in &STANDARD_ICON_SIZES[1..] {
        let dist = (target as i64 - size as i64).unsigned_abs();
        if dist < best_dist || (dist == best_dist && size > best) {
            best = size;
            best_dist = dist;
        }
    }
    best
}

/// Strategy for scaling images/icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageScaleMode {
    /// Fastest — good for pixel art and icons that already match target size.
    NearestNeighbor,
    /// Smooth scaling — good for photographs and gradients.
    Bilinear,
    /// Highest quality downscale — good for large-to-small reductions.
    Lanczos,
}

/// Determine whether the @2x (high-resolution) asset variant should be used.
///
/// Returns true when the effective scale factor is 1.5 or above, which is
/// the threshold where a 2x asset downscaled looks better than a 1x asset
/// upscaled.
pub fn should_use_2x_asset(scale: f32) -> bool {
    scale >= 1.5
}

// ---------------------------------------------------------------------------
// Global scale manager
// ---------------------------------------------------------------------------

/// Maximum number of monitors supported by the global scale manager.
const MAX_MONITORS: usize = 16;

/// Global scale state, stored as `f32` bit patterns in `AtomicU32`.
///
/// Index 0 is the global/fallback scale. Indices 1..=MAX_MONITORS are
/// per-monitor overrides (0 bits = not set, use global).
static SCALE_TABLE: [AtomicU32; MAX_MONITORS + 1] = {
    // Default 1.0 for global slot, 0 (unset) for monitor slots.
    // Use `[const { ... }; N]` instead of `[CONST; N]` to avoid duplicating
    // the AtomicU32 (which has interior mutability) — `[CONST; N]` would
    // copy the same value into every slot, which clippy correctly warns
    // against for interior-mutable types.
    let mut table: [AtomicU32; MAX_MONITORS + 1] =
        [const { AtomicU32::new(0) }; MAX_MONITORS + 1];
    // Slot 0 = global default 1.0
    table[0] = AtomicU32::new(f32_to_bits(1.0));
    table
};

/// Compile-time f32-to-bits for static initialisation.
const fn f32_to_bits(v: f32) -> u32 {
    v.to_bits()
}

/// Set the global scale factor (used when per-monitor is not configured).
///
/// Clamps to [0.25, 8.0] to avoid nonsense values.
pub fn set_global_scale(factor: f32) {
    let clamped = factor.clamp(0.25, 8.0);
    SCALE_TABLE[0].store(clamped.to_bits(), Ordering::Relaxed);
}

/// Set the scale factor for a specific monitor.
///
/// `monitor_id` should be in range 0..MAX_MONITORS. Out-of-range IDs are
/// silently ignored. Pass 0.0 to clear the override (falls back to global).
pub fn set_monitor_scale(monitor_id: usize, factor: f32) {
    let slot = monitor_id.wrapping_add(1);
    if slot > MAX_MONITORS {
        return;
    }
    if factor <= 0.0 {
        // Clear override — store 0 bits (which isn't a valid positive scale).
        SCALE_TABLE[slot].store(0, Ordering::Relaxed);
    } else {
        let clamped = factor.clamp(0.25, 8.0);
        SCALE_TABLE[slot].store(clamped.to_bits(), Ordering::Relaxed);
    }
}

/// Get the effective scale factor for a monitor.
///
/// Returns the per-monitor override if set, otherwise falls back to the
/// global scale.
pub fn get_effective_scale(monitor_id: usize) -> f32 {
    let slot = monitor_id.wrapping_add(1);
    if slot <= MAX_MONITORS {
        let bits = SCALE_TABLE[slot].load(Ordering::Relaxed);
        if bits != 0 {
            return f32::from_bits(bits);
        }
    }
    // Fallback to global.
    f32::from_bits(SCALE_TABLE[0].load(Ordering::Relaxed))
}

/// Get the global scale factor (ignoring per-monitor overrides).
pub fn get_global_scale() -> f32 {
    f32::from_bits(SCALE_TABLE[0].load(Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Logical <-> Physical conversions ---

    #[test]
    fn logical_to_physical_1x() {
        assert_eq!(logical_to_physical(LogicalPixel(100.0), 1.0), PhysicalPixel(100));
    }

    #[test]
    fn logical_to_physical_1_5x() {
        // 100 * 1.5 = 150
        assert_eq!(logical_to_physical(LogicalPixel(100.0), 1.5), PhysicalPixel(150));
        // 7 * 1.5 = 10.5 -> rounds to 11 (nearest integer)
        assert_eq!(logical_to_physical(LogicalPixel(7.0), 1.5), PhysicalPixel(11));
    }

    #[test]
    fn logical_to_physical_2x() {
        assert_eq!(logical_to_physical(LogicalPixel(50.0), 2.0), PhysicalPixel(100));
    }

    #[test]
    fn logical_to_physical_3x() {
        assert_eq!(logical_to_physical(LogicalPixel(33.0), 3.0), PhysicalPixel(99));
    }

    #[test]
    fn physical_to_logical_roundtrip() {
        let original = LogicalPixel(64.0);
        let phys = logical_to_physical(original, 2.0);
        let back = physical_to_logical(phys, 2.0);
        assert_eq!(back, original);
    }

    #[test]
    fn physical_to_logical_zero_scale_is_safe() {
        let result = physical_to_logical(PhysicalPixel(100), 0.0);
        assert_eq!(result, LogicalPixel(0.0));
    }

    // --- ScalableValue resolution ---

    #[test]
    fn scalable_value_fixed() {
        let v = ScalableValue::Fixed(10.0);
        assert_eq!(v.resolve(1.0), 10.0);
        assert_eq!(v.resolve(2.0), 10.0);
        assert_eq!(v.resolve(3.0), 10.0);
    }

    #[test]
    fn scalable_value_scaled() {
        let v = ScalableValue::Scaled(8.0);
        assert_eq!(v.resolve(1.0), 8.0);
        assert_eq!(v.resolve(1.5), 12.0);
        assert_eq!(v.resolve(2.0), 16.0);
    }

    #[test]
    fn scalable_value_stepped() {
        // Example: border width steps
        let v = ScalableValue::Stepped(vec![
            (1.0, 1.0),
            (1.5, 2.0),
            (2.0, 2.0),
            (2.5, 3.0),
        ]);
        assert_eq!(v.resolve(1.0), 1.0);
        assert_eq!(v.resolve(1.25), 1.0); // between 1.0 and 1.5 -> use 1.0's value
        assert_eq!(v.resolve(1.5), 2.0);
        assert_eq!(v.resolve(2.0), 2.0);
        assert_eq!(v.resolve(3.0), 3.0); // above last threshold -> use last
    }

    #[test]
    fn scalable_value_stepped_empty() {
        let v = ScalableValue::Stepped(vec![]);
        assert_eq!(v.resolve(1.0), 0.0);
    }

    #[test]
    fn scalable_value_stepped_below_first() {
        // Scale below the first threshold still returns first value.
        let v = ScalableValue::Stepped(vec![(1.5, 10.0), (2.0, 20.0)]);
        assert_eq!(v.resolve(1.0), 10.0);
    }

    // --- Standard dimension scaling ---

    #[test]
    fn button_height_scales() {
        let ctx = ScaleContext::new(2.0);
        assert_eq!(ctx.button_height(), 64.0); // 32 * 2
    }

    #[test]
    fn button_padding_scales() {
        let ctx = ScaleContext::new(1.5);
        assert_eq!(ctx.button_padding_h(), 18.0); // 12 * 1.5
    }

    #[test]
    fn input_height_scales() {
        let ctx = ScaleContext::new(2.0);
        assert_eq!(ctx.input_height(), 60.0); // 30 * 2
    }

    #[test]
    fn scrollbar_width_respects_minimum() {
        // At very low scales the minimum kicks in.
        let ctx = ScaleContext::new(0.5);
        // 10 * 0.5 = 5.0, but min is 8.0
        assert_eq!(ctx.scrollbar_width(), 8.0);
    }

    #[test]
    fn scrollbar_width_scales_above_minimum() {
        let ctx = ScaleContext::new(2.0);
        // 10 * 2.0 = 20.0, above min
        assert_eq!(ctx.scrollbar_width(), 20.0);
    }

    // --- Icon size snapping ---

    #[test]
    fn nearest_icon_size_exact_match() {
        assert_eq!(nearest_icon_size(16), 16);
        assert_eq!(nearest_icon_size(32), 32);
        assert_eq!(nearest_icon_size(64), 64);
        assert_eq!(nearest_icon_size(256), 256);
    }

    #[test]
    fn nearest_icon_size_rounds_to_closest() {
        assert_eq!(nearest_icon_size(15), 16);
        assert_eq!(nearest_icon_size(17), 16);
        assert_eq!(nearest_icon_size(20), 24); // 20 is equidistant from 16 and 24 -> prefer larger
        assert_eq!(nearest_icon_size(36), 32);
        assert_eq!(nearest_icon_size(40), 48);
        assert_eq!(nearest_icon_size(100), 128);
    }

    #[test]
    fn nearest_icon_size_zero() {
        assert_eq!(nearest_icon_size(0), 16);
    }

    #[test]
    fn nearest_icon_size_very_large() {
        // Larger than 256 -> snaps to 256
        assert_eq!(nearest_icon_size(300), 256);
        assert_eq!(nearest_icon_size(1000), 256);
    }

    // --- Font size calculation ---

    #[test]
    fn effective_font_size_default() {
        let ctx = ScaleContext::default();
        assert_eq!(ctx.effective_font_size(), 14.0); // 14 * 1.0
    }

    #[test]
    fn effective_font_size_with_font_scale() {
        let ctx = ScaleContext::with_font_scale(2.0, 1.5);
        // base=14, font_scale=1.5 -> 14 * 1.5 = 21
        assert_eq!(ctx.effective_font_size(), 21.0);
    }

    #[test]
    fn tooltip_font_size_scales_with_font_scale() {
        let ctx = ScaleContext::with_font_scale(1.0, 2.0);
        // tooltip base = 12, font_scale = 2.0 -> 24
        assert_eq!(ctx.tooltip_font_size(), 24.0);
    }

    // --- Rounding behavior (physical pixels are integers) ---

    #[test]
    fn rounding_at_fractional_scales() {
        // 10 * 1.25 = 12.5 -> rounds to 13
        let phys = logical_to_physical(LogicalPixel(10.0), 1.25);
        assert_eq!(phys, PhysicalPixel(13));

        // 10 * 1.75 = 17.5 -> rounds to 18
        let phys = logical_to_physical(LogicalPixel(10.0), 1.75);
        assert_eq!(phys, PhysicalPixel(18));

        // 3 * 1.5 = 4.5 -> rounds to 5 (banker's rounding, but .round() does away-from-zero)
        let phys = logical_to_physical(LogicalPixel(3.0), 1.5);
        assert_eq!(phys, PhysicalPixel(5));
    }

    #[test]
    fn scaled_rect_to_physical_rounds() {
        let rect = ScaledRect::new(10.3, 20.7, 100.5, 50.2);
        let phys = rect.to_physical(1.5);
        // 10.3*1.5=15.45->15, 20.7*1.5=31.05->31, 100.5*1.5=150.75->151, 50.2*1.5=75.3->75
        assert_eq!(phys.x, 15);
        assert_eq!(phys.y, 31);
        assert_eq!(phys.w, 151);
        assert_eq!(phys.h, 75);
    }

    // --- ScaleContext derived values ---

    #[test]
    fn border_width_at_1x() {
        let ctx = ScaleContext::new(1.0);
        assert_eq!(ctx.border_width(), 1.0);
    }

    #[test]
    fn border_width_at_2x() {
        let ctx = ScaleContext::new(2.0);
        assert_eq!(ctx.border_width(), 2.0);
    }

    #[test]
    fn border_width_never_below_one() {
        let ctx = ScaleContext::new(0.5);
        // 0.5 * 1.0 = 0.5, clamped to 1.0
        assert_eq!(ctx.border_width(), 1.0);
    }

    #[test]
    fn spacing_scales_correctly() {
        let ctx = ScaleContext::new(1.5);
        assert_eq!(ctx.spacing(8.0), 12.0); // 8 * 1.5
        assert_eq!(ctx.spacing(4.0), 6.0);  // 4 * 1.5
    }

    #[test]
    fn icon_size_from_context() {
        let mut ctx = ScaleContext::new(2.0);
        ctx.icon_size_class = IconSizeClass::Small;
        assert_eq!(ctx.icon_size(), 32); // 16 * 2

        ctx.icon_size_class = IconSizeClass::Large;
        assert_eq!(ctx.icon_size(), 64); // 32 * 2
    }

    // --- Image scaling helpers ---

    #[test]
    fn should_use_2x_below_threshold() {
        assert!(!should_use_2x_asset(1.0));
        assert!(!should_use_2x_asset(1.25));
        assert!(!should_use_2x_asset(1.49));
    }

    #[test]
    fn should_use_2x_at_and_above_threshold() {
        assert!(should_use_2x_asset(1.5));
        assert!(should_use_2x_asset(2.0));
        assert!(should_use_2x_asset(3.0));
    }

    // --- Global scale manager ---

    #[test]
    fn global_scale_default_is_1() {
        // Reset to known state.
        set_global_scale(1.0);
        assert_eq!(get_global_scale(), 1.0);
    }

    #[test]
    fn set_and_get_global_scale() {
        set_global_scale(2.0);
        assert_eq!(get_global_scale(), 2.0);
        // Reset
        set_global_scale(1.0);
    }

    #[test]
    fn global_scale_clamped() {
        set_global_scale(100.0);
        assert_eq!(get_global_scale(), 8.0);
        set_global_scale(0.01);
        assert_eq!(get_global_scale(), 0.25);
        // Reset
        set_global_scale(1.0);
    }

    #[test]
    fn per_monitor_override() {
        set_global_scale(1.0);
        set_monitor_scale(0, 2.5);
        assert_eq!(get_effective_scale(0), 2.5);
        // Other monitors fall back to global
        set_monitor_scale(1, 0.0); // clear
        assert_eq!(get_effective_scale(1), 1.0);
        // Cleanup
        set_monitor_scale(0, 0.0);
    }

    #[test]
    fn per_monitor_clear_falls_back() {
        set_global_scale(1.5);
        set_monitor_scale(2, 3.0);
        assert_eq!(get_effective_scale(2), 3.0);
        set_monitor_scale(2, 0.0); // clear override
        assert_eq!(get_effective_scale(2), 1.5);
        // Reset
        set_global_scale(1.0);
    }

    // --- Geometry type conversions ---

    #[test]
    fn scaled_size_roundtrip() {
        let s = ScaledSize::new(120.0, 80.0);
        let phys = s.to_physical(2.0);
        assert_eq!(phys, PhysicalSize { width: 240, height: 160 });
        let back = ScaledSize::from_physical(phys, 2.0);
        assert_eq!(back, s);
    }

    #[test]
    fn scaled_point_roundtrip() {
        let p = ScaledPoint::new(50.0, 75.0);
        let phys = p.to_physical(2.0);
        assert_eq!(phys, PhysicalPoint { x: 100, y: 150 });
        let back = ScaledPoint::from_physical(phys, 2.0);
        assert_eq!(back, p);
    }

    #[test]
    fn scaled_rect_contains() {
        let r = ScaledRect::new(10.0, 10.0, 100.0, 50.0);
        assert!(r.contains(ScaledPoint::new(10.0, 10.0))); // top-left corner
        assert!(r.contains(ScaledPoint::new(50.0, 30.0))); // center
        assert!(!r.contains(ScaledPoint::new(110.0, 30.0))); // right edge (exclusive)
        assert!(!r.contains(ScaledPoint::new(5.0, 30.0))); // outside left
    }

    #[test]
    fn scaled_rect_edges() {
        let r = ScaledRect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.right(), 110.0);
        assert_eq!(r.bottom(), 70.0);
    }

    #[test]
    fn from_physical_zero_scale_safe() {
        let s = ScaledSize::from_physical(PhysicalSize { width: 100, height: 100 }, 0.0);
        assert_eq!(s, ScaledSize::new(0.0, 0.0));

        let p = ScaledPoint::from_physical(PhysicalPoint { x: 50, y: 50 }, 0.0);
        assert_eq!(p, ScaledPoint::new(0.0, 0.0));

        let r = ScaledRect::from_physical(PhysicalRect { x: 10, y: 10, w: 50, h: 50 }, 0.0);
        assert_eq!(r, ScaledRect::new(0.0, 0.0, 0.0, 0.0));
    }
}
