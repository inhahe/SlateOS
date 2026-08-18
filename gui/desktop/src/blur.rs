//! Aero-style blurry transparency for the taskbar and window title bars.
//!
//! Provides a software blur pipeline that operates on raw ARGB pixel buffers.
//! Three-pass box blur approximates a Gaussian to give the frosted-glass look
//! popularised by Windows Vista/7 "Aero Glass" and Windows 11 "Mica/Acrylic".
//!
//! # Architecture
//!
//! ```text
//!   Framebuffer ──► BlurRenderer.blur_region() ──► tint + noise ──► composite
//!                       (3-pass box blur)
//! ```
//!
//! The [`BlurManager`] coordinates all active [`BlurRegion`]s, caches results
//! to avoid redundant work, and exposes a single `update_all()` call per frame.
//!
//! # Usage
//!
//! ```ignore
//! let mut mgr = BlurManager::new();
//! mgr.register(0, BlurRegion::new(0.0, 920.0, 1920.0, 48.0, BlurEffect::taskbar()));
//! mgr.register(1, BlurRegion::new(100.0, 100.0, 800.0, 30.0, BlurEffect::title_bar()));
//!
//! // each frame:
//! mgr.update_all(&mut framebuffer, 1920, 1080);
//! ```

use guitk::color::Color;

use std::collections::HashMap;

// ============================================================================
// Catppuccin Mocha palette (blur-specific tints)
// ============================================================================

/// Catppuccin Mocha: base (used as heavy tint base for taskbar)
const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
/// Catppuccin Mocha: mantle (darker tint for title bars)
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
/// Catppuccin Mocha: surface0 (mid-tone for menus)
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);

// ============================================================================
// BlurEffect — configurable visual parameters
// ============================================================================

/// Visual parameters for a blur effect.
///
/// All numeric fields are clamped to their valid ranges on construction and
/// via setters to prevent degenerate rendering.
#[derive(Clone, Debug)]
pub struct BlurEffect {
    /// Blur kernel radius in pixels (clamped to 0.0..=100.0).
    pub radius: f32,
    /// Background opacity after blur (clamped to 0.0..=1.0).
    pub opacity: f32,
    /// Color tint applied over the blurred content.
    pub tint: Color,
    /// Saturation multiplier (1.0 = unchanged, clamped 0.0..=3.0).
    pub saturation: f32,
    /// Noise texture amount for realism (clamped 0.0..=1.0).
    pub noise_amount: f32,
}

impl BlurEffect {
    /// Create a new blur effect with the given parameters (values are clamped).
    pub fn new(radius: f32, opacity: f32, tint: Color, saturation: f32, noise_amount: f32) -> Self {
        Self {
            radius: radius.clamp(0.0, 100.0),
            opacity: opacity.clamp(0.0, 1.0),
            tint,
            saturation: saturation.clamp(0.0, 3.0),
            noise_amount: noise_amount.clamp(0.0, 1.0),
        }
    }

    /// Heavy blur with dark tint — Windows 11 taskbar style.
    pub fn taskbar() -> Self {
        Self::new(
            24.0,
            0.65,
            Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 160),
            1.3,
            0.03,
        )
    }

    /// Medium blur with lighter tint — window title bars.
    pub fn title_bar() -> Self {
        Self::new(
            16.0,
            0.75,
            Color::rgba(MOCHA_MANTLE.r, MOCHA_MANTLE.g, MOCHA_MANTLE.b, 120),
            1.1,
            0.02,
        )
    }

    /// Light blur for dropdown/context menus.
    pub fn menu() -> Self {
        Self::new(
            12.0,
            0.80,
            Color::rgba(MOCHA_SURFACE0.r, MOCHA_SURFACE0.g, MOCHA_SURFACE0.b, 100),
            1.0,
            0.01,
        )
    }

    /// Medium blur for notification panels.
    pub fn notification() -> Self {
        Self::new(
            18.0,
            0.70,
            Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 140),
            1.2,
            0.02,
        )
    }

    /// Fully opaque, no blur (accessibility/performance fallback).
    pub fn none() -> Self {
        Self::new(
            0.0,
            1.0,
            Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 255),
            1.0,
            0.0,
        )
    }
}

impl Default for BlurEffect {
    fn default() -> Self {
        Self::new(
            20.0,
            0.70,
            Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 140),
            1.2,
            0.02,
        )
    }
}

// ============================================================================
// BlurRegion — rectangular area with an associated blur effect
// ============================================================================

/// A rectangular area where blur is applied.
#[derive(Clone, Debug)]
pub struct BlurRegion {
    /// X position in framebuffer coordinates.
    pub x: f32,
    /// Y position in framebuffer coordinates.
    pub y: f32,
    /// Width of the blurred area.
    pub width: f32,
    /// Height of the blurred area.
    pub height: f32,
    /// Corner radius for rounded clipping.
    pub corner_radius: f32,
    /// The blur effect to apply.
    pub effect: BlurEffect,
    /// Whether blur is active for this region.
    pub enabled: bool,
}

impl BlurRegion {
    /// Create a new rectangular blur region with the given effect.
    pub fn new(x: f32, y: f32, width: f32, height: f32, effect: BlurEffect) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
            corner_radius: 0.0,
            effect,
            enabled: true,
        }
    }

    /// Create a blur region with rounded corners.
    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius.max(0.0);
        self
    }

    /// Toggle the enabled flag.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Integer bounding box clamped to framebuffer dimensions.
    fn pixel_bounds(&self, fb_width: u32, fb_height: u32) -> (u32, u32, u32, u32) {
        let x0 = (self.x.max(0.0) as u32).min(fb_width);
        let y0 = (self.y.max(0.0) as u32).min(fb_height);
        let x1 = ((self.x + self.width).ceil() as u32).min(fb_width);
        let y1 = ((self.y + self.height).ceil() as u32).min(fb_height);
        let w = x1.saturating_sub(x0);
        let h = y1.saturating_sub(y0);
        (x0, y0, w, h)
    }
}

// ============================================================================
// Rgb — the blur's per-channel accumulator
// ============================================================================

/// The three colour channels of a pixel, widened for accumulation.
///
/// The sliding window of a box blur holds the sum of up to `2 * radius + 1`
/// pixels, which does not fit in a `u8`. Keeping the three channels together
/// in one value — rather than as three loose `sr`/`sg`/`sb` locals threaded
/// through every loop — means the window is added to, subtracted from and
/// averaged as a single thing, so the three channels cannot drift apart.
///
/// Every operation saturates. The window's arithmetic is balanced by
/// construction, but "balanced by construction" is a proof that lives in a
/// different function from the subtraction that depends on it; saturating
/// makes the failure a clamped pixel rather than a channel that wraps from 0
/// to four billion and paints white.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Rgb {
    r: u32,
    g: u32,
    b: u32,
}

impl Rgb {
    /// Split a packed ARGB pixel into its three channels. Alpha is dropped:
    /// this pipeline composites onto an opaque framebuffer.
    #[inline]
    const fn from_argb(px: u32) -> Self {
        Self {
            r: (px >> 16) & 0xFF,
            g: (px >> 8) & 0xFF,
            b: px & 0xFF,
        }
    }

    /// Repack into an opaque ARGB pixel, clamping each channel to 8 bits.
    #[inline]
    const fn to_argb(self) -> u32 {
        let r = if self.r > 255 { 255 } else { self.r };
        let g = if self.g > 255 { 255 } else { self.g };
        let b = if self.b > 255 { 255 } else { self.b };
        0xFF00_0000 | (r << 16) | (g << 8) | b
    }

    /// This pixel counted `n` times — the edge replication a box blur needs
    /// when its window hangs off the end of a line.
    #[inline]
    fn scaled(self, n: u32) -> Self {
        Self {
            r: self.r.saturating_mul(n),
            g: self.g.saturating_mul(n),
            b: self.b.saturating_mul(n),
        }
    }

    #[inline]
    fn plus(self, other: Self) -> Self {
        Self {
            r: self.r.saturating_add(other.r),
            g: self.g.saturating_add(other.g),
            b: self.b.saturating_add(other.b),
        }
    }

    #[inline]
    fn minus(self, other: Self) -> Self {
        Self {
            r: self.r.saturating_sub(other.r),
            g: self.g.saturating_sub(other.g),
            b: self.b.saturating_sub(other.b),
        }
    }

    /// Divide the accumulated window by its width, using a pre-computed
    /// fixed-point reciprocal instead of an integer division per channel.
    ///
    /// Adds half a fixed-point unit before the shift so the division rounds to
    /// nearest rather than truncating. With pure truncation each pass loses ~1
    /// per channel (because [`reciprocal_table`] rounds the reciprocal down),
    /// so the three-pass, two-direction pipeline drifted uniform images by up
    /// to 6. Rounding to nearest keeps uniform images stable.
    #[inline]
    fn average(self, inv: u32) -> u32 {
        let scale = |v: u32| {
            v.saturating_mul(inv)
                .saturating_add(0x8000)
                .checked_shr(16)
                .unwrap_or(0)
                .min(255)
        };
        Self {
            r: scale(self.r),
            g: scale(self.g),
            b: scale(self.b),
        }
        .to_argb()
    }
}

// ============================================================================
// PixelRect — a rectangle of pixels that carries its own dimensions
// ============================================================================

/// Opaque black — what a read outside a buffer yields, and what a freshly
/// allocated rectangle is filled with.
const OPAQUE_BLACK: u32 = 0xFF00_0000;

/// A rectangular block of ARGB pixels that knows its own shape.
///
/// The blur pipeline used to pass a `&[u32]` alongside a `width` and a
/// `height` and recompute `row * width + col` by hand at every access — six
/// separate nested loops, each with its own bounds handling, and each correct
/// only because of a clamp performed in some *other* function. That is how
/// this module came to blur with a kernel one sample short (see
/// [`box_blur_line`]) and to write a row's overhang onto the start of the next
/// scanline whenever a region reached the right-hand edge.
///
/// A buffer that carries its own dimensions moves that index arithmetic into
/// one place that can be tested on its own, and leaves no way to spell an
/// out-of-range access at the call sites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pixels: Vec<u32>,
    width: usize,
    height: usize,
}

impl PixelRect {
    /// An opaque black rectangle of the given size.
    pub fn new(width: u32, height: u32) -> Self {
        let width = width as usize;
        let height = height as usize;
        let len = width.saturating_mul(height);
        Self {
            pixels: vec![OPAQUE_BLACK; len],
            width,
            height,
        }
    }

    /// Copy a rectangle out of a framebuffer.
    ///
    /// Anything the requested rectangle covers that the framebuffer does not
    /// is filled with opaque black rather than left uninitialised or silently
    /// shortened, so the result always has exactly the size asked for — the
    /// callers downstream index it by its own dimensions.
    pub fn from_framebuffer(
        buffer: &[u32],
        fb_width: u32,
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
    ) -> Self {
        let mut rect = Self::new(rw, rh);
        let fb_w = fb_width as usize;
        if fb_w == 0 {
            return rect;
        }
        let (rx, ry) = (rx as usize, ry as usize);
        // A row of the region may hang off the right-hand edge of the
        // framebuffer. Bound it by the framebuffer's own row, not merely by
        // the buffer's total length: a length check alone lets the overhang
        // wrap onto the next scanline.
        let available = fb_w.saturating_sub(rx);
        let span = rect.width.min(available);
        for row in 0..rect.height {
            let Some(start) = ry
                .checked_add(row)
                .and_then(|r| r.checked_mul(fb_w))
                .and_then(|s| s.checked_add(rx))
            else {
                break;
            };
            let Some(end) = start.checked_add(span) else {
                break;
            };
            let (Some(src), Some(dst)) = (buffer.get(start..end), rect.row_mut(row)) else {
                continue;
            };
            let Some(dst) = dst.get_mut(..span) else {
                continue;
            };
            dst.copy_from_slice(src);
        }
        rect
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        u32::try_from(self.width).unwrap_or(u32::MAX)
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        u32::try_from(self.height).unwrap_or(u32::MAX)
    }

    /// Whether the rectangle holds no pixels.
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    /// The pixels in row-major order.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// The pixels in row-major order, mutably.
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.pixels
    }

    /// One row of pixels, or `None` if `row` is past the bottom.
    fn row(&self, row: usize) -> Option<&[u32]> {
        let start = row.checked_mul(self.width)?;
        let end = start.checked_add(self.width)?;
        self.pixels.get(start..end)
    }

    /// One row of pixels, mutably.
    fn row_mut(&mut self, row: usize) -> Option<&mut [u32]> {
        let start = row.checked_mul(self.width)?;
        let end = start.checked_add(self.width)?;
        self.pixels.get_mut(start..end)
    }

    /// The pixel at `(col, row)`, or `None` if it lies outside.
    pub fn get(&self, col: usize, row: usize) -> Option<u32> {
        self.row(row)?.get(col).copied()
    }

    /// Store a pixel at `(col, row)`. Out-of-range writes are dropped — a
    /// rectangle cannot be made to grow by writing past its edge.
    pub fn set(&mut self, col: usize, row: usize, px: u32) {
        if let Some(slot) = self.row_mut(row).and_then(|r| r.get_mut(col)) {
            *slot = px;
        }
    }

    /// The pixel at `(col, row)` with coordinates clamped to the rectangle.
    ///
    /// This is what makes the blur's edge handling a property of the buffer
    /// rather than a special case at every call site: sampling past an edge
    /// repeats that edge, which is what stops a blurred panel darkening into
    /// its own borders.
    pub fn sample(&self, col: usize, row: usize) -> u32 {
        let col = col.min(self.width.saturating_sub(1));
        let row = row.min(self.height.saturating_sub(1));
        self.get(col, row).unwrap_or(OPAQUE_BLACK)
    }

    /// Visit every pixel of this rectangle together with the framebuffer pixel
    /// it would be written to, at framebuffer position `(rx, ry)`.
    ///
    /// The single place in this module where region coordinates become
    /// framebuffer indices. Rows that fall outside the framebuffer, and the
    /// part of a row that overhangs its right-hand edge, are simply not
    /// visited.
    fn blit_with(
        &self,
        buffer: &mut [u32],
        fb_width: u32,
        rx: u32,
        ry: u32,
        mut visit: impl FnMut(usize, usize, u32, &mut u32),
    ) {
        let fb_w = fb_width as usize;
        if fb_w == 0 {
            return;
        }
        let (rx, ry) = (rx as usize, ry as usize);
        let span = self.width.min(fb_w.saturating_sub(rx));
        for row in 0..self.height {
            let Some(start) = ry
                .checked_add(row)
                .and_then(|r| r.checked_mul(fb_w))
                .and_then(|s| s.checked_add(rx))
            else {
                return;
            };
            let Some(end) = start.checked_add(span) else {
                return;
            };
            let (Some(src), Some(dst)) = (self.row(row), buffer.get_mut(start..end)) else {
                continue;
            };
            for (col, (&px, slot)) in src.iter().zip(dst.iter_mut()).enumerate() {
                visit(col, row, px, slot);
            }
        }
    }

    /// Copy this rectangle into a framebuffer at `(rx, ry)`.
    pub fn blit_into(&self, buffer: &mut [u32], fb_width: u32, rx: u32, ry: u32) {
        self.blit_with(buffer, fb_width, rx, ry, |_, _, px, slot| *slot = px);
    }
}

// ============================================================================
// BlurRenderer — software blur implementation
// ============================================================================

/// Software blur renderer operating on ARGB pixel buffers.
///
/// Uses a three-pass box blur (horizontal, vertical, horizontal) to approximate
/// a Gaussian kernel — a well-known technique that produces smooth results at
/// roughly 1/3 the cost of a true Gaussian of the same radius.
pub struct BlurRenderer;

impl BlurRenderer {
    // ------------------------------------------------------------------
    // Public API
    // ------------------------------------------------------------------

    /// Apply a blur effect to a rectangular region of the framebuffer.
    ///
    /// The buffer is `&mut [u32]` in ARGB format (0xAA_RR_GG_BB). Pixels
    /// outside the region are not modified.
    pub fn blur_region(buffer: &mut [u32], fb_width: u32, fb_height: u32, region: &BlurRegion) {
        if !region.enabled || region.effect.radius < 0.5 {
            return;
        }

        let (rx, ry, rw, rh) = region.pixel_bounds(fb_width, fb_height);
        let mut sub = PixelRect::from_framebuffer(buffer, fb_width, rx, ry, rw, rh);
        if sub.is_empty() {
            return;
        }

        // Three-pass box blur (approximates Gaussian).
        let radius = (region.effect.radius as usize).max(1);
        for _ in 0..3 {
            Self::box_blur_pass(&mut sub, radius);
        }

        // Apply saturation adjustment.
        if (region.effect.saturation - 1.0).abs() > 0.01 {
            Self::apply_saturation(sub.pixels_mut(), region.effect.saturation);
        }

        // Apply noise texture.
        if region.effect.noise_amount > 0.001 {
            Self::apply_noise(&mut sub, region.effect.noise_amount);
        }

        // Write back with rounded corner mask and opacity.
        Self::write_back_with_clip(
            buffer,
            fb_width,
            &sub,
            rx,
            ry,
            region.corner_radius,
            region.effect.opacity,
        );
    }

    /// Composite a blurred background with a tinted foreground overlay.
    ///
    /// `background` is the already-blurred region data.
    /// Returns a new buffer with the tint blended on top using alpha compositing.
    pub fn composite_blur(background: &[u32], tint: Color, width: u32, height: u32) -> Vec<u32> {
        let len = (width as usize).saturating_mul(height as usize);
        let tint_argb = Self::color_to_argb(tint);
        let ta = tint.a as u32;
        let t = Rgb::from_argb(tint_argb);

        background
            .iter()
            .take(len)
            .map(|&bg_px| Self::blend_pixel(t.to_argb(), bg_px, ta))
            .collect()
    }

    // ------------------------------------------------------------------
    // Internal: write-back
    // ------------------------------------------------------------------

    /// Write the processed sub-buffer back, applying rounded-corner masking
    /// and global opacity.
    fn write_back_with_clip(
        buffer: &mut [u32],
        fb_width: u32,
        sub: &PixelRect,
        rx: u32,
        ry: u32,
        corner_radius: f32,
        opacity: f32,
    ) {
        let op = (opacity.clamp(0.0, 1.0) * 255.0) as u32;
        let (rw, rh) = (sub.width(), sub.height());
        sub.blit_with(buffer, fb_width, rx, ry, |col, row, src, slot| {
            let (Ok(col), Ok(row)) = (u32::try_from(col), u32::try_from(row)) else {
                return;
            };
            // Rounded corner test — outside the arc, leave the original pixel.
            if corner_radius > 0.5 && !Self::in_rounded_rect(col, row, rw, rh, corner_radius) {
                return;
            }
            *slot = if op >= 255 {
                src
            } else {
                Self::blend_pixel(src, *slot, op)
            };
        });
    }

    /// Test whether a pixel at (col, row) inside a rect of size (w, h) falls
    /// inside rounded corners of the given radius.
    fn in_rounded_rect(col: u32, row: u32, w: u32, h: u32, radius: f32) -> bool {
        let r = radius;
        let ru = r as u32;

        // Only need to test the four corner quadrants.
        let in_left = col < ru;
        let in_right = col >= w.saturating_sub(ru);
        let in_top = row < ru;
        let in_bottom = row >= h.saturating_sub(ru);

        if !(in_left || in_right) || !(in_top || in_bottom) {
            return true;
        }

        // Centre of the corner arc.
        let cx = if in_left { r } else { w as f32 - r };
        let cy = if in_top { r } else { h as f32 - r };

        let dx = col as f32 + 0.5 - cx;
        let dy = row as f32 + 0.5 - cy;
        dx.mul_add(dx, dy * dy) <= r * r
    }

    // ------------------------------------------------------------------
    // Internal: box blur (separable horizontal + vertical)
    // ------------------------------------------------------------------

    /// Single box blur pass: every row, then every column.
    fn box_blur_pass(rect: &mut PixelRect, radius: usize) {
        let mut tmp = PixelRect::new(rect.width(), rect.height());
        for row in 0..rect.height {
            box_blur_line(
                rect.width,
                radius,
                |i| rect.sample(i, row),
                |i, px| tmp.set(i, row, px),
            );
        }
        for col in 0..tmp.width {
            box_blur_line(
                tmp.height,
                radius,
                |i| tmp.sample(col, i),
                |i, px| rect.set(col, i, px),
            );
        }
    }

    // ------------------------------------------------------------------
    // Internal: saturation and noise post-processing
    // ------------------------------------------------------------------

    /// Adjust colour saturation of the buffer.
    ///
    /// `factor` > 1.0 boosts saturation; < 1.0 desaturates.
    fn apply_saturation(buf: &mut [u32], factor: f32) {
        for px in buf.iter_mut() {
            let c = Rgb::from_argb(*px);
            // Luma (Rec. 709 coefficients).
            let luma =
                (c.r as f32).mul_add(0.2126, (c.g as f32).mul_add(0.7152, c.b as f32 * 0.0722));
            *px = Rgb {
                r: Self::sat_channel(c.r, luma, factor),
                g: Self::sat_channel(c.g, luma, factor),
                b: Self::sat_channel(c.b, luma, factor),
            }
            .to_argb();
        }
    }

    fn sat_channel(val: u32, luma: f32, factor: f32) -> u32 {
        let v = (val as f32 - luma).mul_add(factor, luma);
        v.round().clamp(0.0, 255.0) as u32
    }

    /// Add a subtle deterministic noise pattern.
    ///
    /// Uses a simple hash based on pixel position rather than a PRNG so results
    /// are reproducible and cache-friendly.
    fn apply_noise(rect: &mut PixelRect, amount: f32) {
        let strength = (amount.clamp(0.0, 1.0) * 255.0) as u32;
        if strength == 0 {
            return;
        }
        // The hash lands in [0, 2 * strength]; shifting by `strength` centres
        // it on zero so the noise brightens as often as it darkens.
        let span = strength.saturating_mul(2).saturating_add(1);
        for row in 0..rect.height {
            for col in 0..rect.width {
                let Some(px) = rect.get(col, row) else {
                    continue;
                };
                let (Ok(x), Ok(y)) = (u32::try_from(col), u32::try_from(row)) else {
                    continue;
                };
                let noise = i64::from(pixel_hash(x, y) % span) - i64::from(strength);
                let shift =
                    |v: u32| -> u32 { (i64::from(v).saturating_add(noise)).clamp(0, 255) as u32 };
                let c = Rgb::from_argb(px);
                rect.set(
                    col,
                    row,
                    Rgb {
                        r: shift(c.r),
                        g: shift(c.g),
                        b: shift(c.b),
                    }
                    .to_argb(),
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Internal: pixel packing helpers
    // ------------------------------------------------------------------

    /// Unpack ARGB u32 into (R, G, B) as u32 for accumulation.
    #[inline]
    fn unpack(px: u32) -> (u32, u32, u32) {
        let c = Rgb::from_argb(px);
        (c.r, c.g, c.b)
    }

    /// Pack RGB channels using a pre-computed reciprocal (fixed-point multiply
    /// instead of integer division).
    #[inline]
    fn pack_with_inv(sr: u32, sg: u32, sb: u32, inv: u32) -> u32 {
        Rgb {
            r: sr,
            g: sg,
            b: sb,
        }
        .average(inv)
    }

    /// Alpha-blend `src` over `dst` with the given source alpha (0..255).
    #[inline]
    fn blend_pixel(src: u32, dst: u32, alpha: u32) -> u32 {
        let alpha = alpha.min(255);
        let inv = 255u32.saturating_sub(alpha);
        let s = Rgb::from_argb(src).scaled(alpha);
        let d = Rgb::from_argb(dst).scaled(inv);
        let mix = |a: u32, b: u32| a.saturating_add(b).checked_div(255).unwrap_or(0);
        Rgb {
            r: mix(s.r, d.r),
            g: mix(s.g, d.g),
            b: mix(s.b, d.b),
        }
        .to_argb()
    }

    /// Convert a `Color` to packed ARGB u32.
    #[inline]
    fn color_to_argb(c: Color) -> u32 {
        (c.a as u32) << 24 | (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
    }
}

// ============================================================================
// BlurManager — tracks and updates all active blur regions
// ============================================================================

/// Manages all active blur regions and caches blurred output.
pub struct BlurManager {
    /// Active blur regions keyed by caller-assigned ID.
    regions: HashMap<u64, BlurRegion>,
    /// Cached blurred pixel data per region ID.
    ///
    /// A [`PixelRect`] and not a bare `Vec<u32>`: the cache outlives the frame
    /// that filled it, and a cached buffer that has forgotten its own width is
    /// one resize away from being blitted back at the wrong stride.
    cache: HashMap<u64, PixelRect>,
    /// Dirty flags per region (set when underlying content may have changed).
    dirty: HashMap<u64, bool>,
    /// Master toggle — when false, no blur processing occurs.
    enabled: bool,
}

impl BlurManager {
    /// Create a new, empty blur manager.
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
            cache: HashMap::new(),
            dirty: HashMap::new(),
            enabled: true,
        }
    }

    /// Register (or replace) a blur region under the given ID.
    pub fn register(&mut self, id: u64, region: BlurRegion) {
        self.regions.insert(id, region);
        self.dirty.insert(id, true);
        // Invalidate cached data for this ID.
        self.cache.remove(&id);
    }

    /// Remove a blur region by ID.
    pub fn unregister(&mut self, id: u64) {
        self.regions.remove(&id);
        self.cache.remove(&id);
        self.dirty.remove(&id);
    }

    /// Get a reference to a region by ID.
    pub fn get(&self, id: u64) -> Option<&BlurRegion> {
        self.regions.get(&id)
    }

    /// Get a mutable reference to a region by ID.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut BlurRegion> {
        self.dirty.insert(id, true);
        self.regions.get_mut(&id)
    }

    /// Mark a region as dirty (underlying content changed).
    pub fn invalidate(&mut self, id: u64) {
        self.dirty.insert(id, true);
    }

    /// Mark all regions as dirty.
    pub fn invalidate_all(&mut self) {
        for val in self.dirty.values_mut() {
            *val = true;
        }
    }

    /// Whether any regions are registered.
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Number of registered regions.
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Set the global enabled toggle.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Whether blur processing is globally enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Apply all registered blur effects to the framebuffer.
    ///
    /// Regions that are not dirty and have a valid cache entry are skipped.
    pub fn update_all(&mut self, buffer: &mut [u32], fb_width: u32, fb_height: u32) {
        if !self.enabled {
            return;
        }

        let ids: Vec<u64> = self.regions.keys().copied().collect();
        for id in ids {
            let is_dirty = self.dirty.get(&id).copied().unwrap_or(true);
            let Some(region) = self.regions.get(&id) else {
                continue;
            };
            if !region.enabled {
                continue;
            }
            let (rx, ry, rw, rh) = region.pixel_bounds(fb_width, fb_height);

            // Use cache if not dirty and cache exists.
            if !is_dirty && let Some(cached) = self.cache.get(&id) {
                cached.blit_into(buffer, fb_width, rx, ry);
                continue;
            }

            if rw == 0 || rh == 0 {
                continue;
            }
            let region = region.clone();

            // Apply blur to the framebuffer in-place, then composite the tint
            // over the blurred region and write that back.
            BlurRenderer::blur_region(buffer, fb_width, fb_height, &region);
            let sub = PixelRect::from_framebuffer(buffer, fb_width, rx, ry, rw, rh);
            let mut composited = sub;
            for px in composited.pixels_mut() {
                *px = BlurRenderer::blend_pixel(
                    BlurRenderer::color_to_argb(region.effect.tint),
                    *px,
                    region.effect.tint.a as u32,
                );
            }
            composited.blit_into(buffer, fb_width, rx, ry);

            self.cache.insert(id, composited);
            self.dirty.insert(id, false);
        }
    }
}

impl Default for BlurManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Free helpers
// ============================================================================

/// Run a one-dimensional box blur along a single line of pixels.
///
/// `read(i)` yields the i-th pixel of the line and `write(i, px)` stores the
/// result. A row and a column differ only in those two closures, so the
/// sliding window — the part that is easy to get wrong — exists exactly once.
///
/// It *was* wrong, in both of the two copies this replaces. The window for
/// output `i` covers `[i - radius, i + radius]`, but the old code seeded it
/// with `radius + 1` copies of the first pixel and then the pixels `0..radius`
/// *exclusive*: the sample at `+radius` was never in the sum, and the first
/// pixel was counted once too many. Because the sliding step was written
/// against the correct window, the error did not cancel — it persisted across
/// the whole line. A palindromic row `[10, 20, 30, 20, 10]` blurred at radius
/// 1 came back `[10, 17, 20, 17, 10]` where the kernel's own definition gives
/// `[13, 20, 23, 20, 13]`, and the three-pass, two-direction pipeline
/// compounded that bias into a visible misregistration between a blurred
/// panel and the wallpaper it was supposed to be blurring.
///
/// `radius` is capped at `len`: past that the window already spans the whole
/// line and further growth only shifts weight between the two replicated ends,
/// while the accumulator would have to hold `255 * (2 * radius + 1)`.
fn box_blur_line(
    len: usize,
    radius: usize,
    read: impl Fn(usize) -> u32,
    mut write: impl FnMut(usize, u32),
) {
    if len == 0 {
        return;
    }
    let last = len.saturating_sub(1);
    let radius = radius.min(len);
    let diameter = radius.saturating_mul(2).saturating_add(1);
    let inv = reciprocal_table(u32::try_from(diameter).unwrap_or(u32::MAX));

    // Sampling past either end of the line repeats that end's pixel.
    let at = |i: usize| Rgb::from_argb(read(i.min(last)));

    // Seed the window for output 0. It covers `[-radius, radius]`; every
    // position left of the line takes the first pixel, which is `radius`
    // copies of it, and then the pixels `0..=radius` themselves.
    let mut sum = at(0).scaled(u32::try_from(radius).unwrap_or(u32::MAX));
    for k in 0..=radius {
        sum = sum.plus(at(k));
    }

    for i in 0..len {
        write(i, sum.average(inv));
        // Slide to `i + 1`: the sample at `i - radius` leaves the window and
        // the one at `i + radius + 1` enters it. Both saturate into the
        // clamped edge, which is exactly the edge replication we want.
        let leaving = at(i.saturating_sub(radius));
        let entering = at(i.saturating_add(radius).saturating_add(1));
        sum = sum.minus(leaving).plus(entering);
    }
}

/// Fixed-point reciprocal (16-bit fractional) for integer division avoidance.
///
/// Returns `(1 << 16) / n` — multiply an accumulated sum by this value and
/// shift right by 16 to get the average.
#[inline]
fn reciprocal_table(n: u32) -> u32 {
    (1u32 << 16).checked_div(n).unwrap_or(0)
}

/// Deterministic spatial hash for noise generation.
///
/// Produces a pseudo-random u32 from (x, y) coordinates.
#[inline]
fn pixel_hash(x: u32, y: u32) -> u32 {
    // Minimal hash — good enough for visual noise, not crypto.
    let mut h = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;

    // ======================================================================
    // Helper: create a solid-colour framebuffer
    // ======================================================================

    fn solid_buffer(width: u32, height: u32, color: u32) -> Vec<u32> {
        vec![color; width as usize * height as usize]
    }

    fn gradient_buffer(width: u32, height: u32) -> Vec<u32> {
        let mut buf = Vec::with_capacity(width as usize * height as usize);
        for row in 0..height {
            for col in 0..width {
                let r = (col * 255 / width.max(1)) & 0xFF;
                let g = (row * 255 / height.max(1)) & 0xFF;
                let b = 128;
                buf.push(0xFF00_0000 | (r << 16) | (g << 8) | b);
            }
        }
        buf
    }

    // ======================================================================
    // BlurEffect parameter validation (clamping)
    // ======================================================================

    #[test]
    fn test_blur_effect_clamps_radius() {
        let e = BlurEffect::new(200.0, 0.5, Color::BLACK, 1.0, 0.0);
        assert_eq!(e.radius, 100.0);
        let e = BlurEffect::new(-5.0, 0.5, Color::BLACK, 1.0, 0.0);
        assert_eq!(e.radius, 0.0);
    }

    #[test]
    fn test_blur_effect_clamps_opacity() {
        let e = BlurEffect::new(10.0, 2.0, Color::BLACK, 1.0, 0.0);
        assert_eq!(e.opacity, 1.0);
        let e = BlurEffect::new(10.0, -0.5, Color::BLACK, 1.0, 0.0);
        assert_eq!(e.opacity, 0.0);
    }

    #[test]
    fn test_blur_effect_clamps_saturation() {
        let e = BlurEffect::new(10.0, 0.5, Color::BLACK, 5.0, 0.0);
        assert_eq!(e.saturation, 3.0);
        let e = BlurEffect::new(10.0, 0.5, Color::BLACK, -1.0, 0.0);
        assert_eq!(e.saturation, 0.0);
    }

    #[test]
    fn test_blur_effect_clamps_noise() {
        let e = BlurEffect::new(10.0, 0.5, Color::BLACK, 1.0, 3.0);
        assert_eq!(e.noise_amount, 1.0);
        let e = BlurEffect::new(10.0, 0.5, Color::BLACK, 1.0, -0.1);
        assert_eq!(e.noise_amount, 0.0);
    }

    // ======================================================================
    // Preset creation
    // ======================================================================

    #[test]
    fn test_preset_taskbar() {
        let e = BlurEffect::taskbar();
        assert!(e.radius > 20.0);
        assert!(e.opacity < 1.0);
        assert!(e.saturation > 1.0);
        assert!(e.noise_amount > 0.0);
    }

    #[test]
    fn test_preset_title_bar() {
        let e = BlurEffect::title_bar();
        assert!(e.radius > 10.0 && e.radius < 30.0);
        assert!(e.opacity > 0.5 && e.opacity < 1.0);
    }

    #[test]
    fn test_preset_menu() {
        let e = BlurEffect::menu();
        assert!(e.radius > 5.0 && e.radius < 20.0);
        assert!(e.opacity >= 0.7);
    }

    #[test]
    fn test_preset_notification() {
        let e = BlurEffect::notification();
        assert!(e.radius >= 15.0);
        assert!(e.noise_amount > 0.0);
    }

    #[test]
    fn test_preset_none() {
        let e = BlurEffect::none();
        assert_eq!(e.radius, 0.0);
        assert_eq!(e.opacity, 1.0);
        assert_eq!(e.noise_amount, 0.0);
    }

    #[test]
    fn test_default_effect() {
        let e = BlurEffect::default();
        assert_eq!(e.radius, 20.0);
        assert_eq!(e.opacity, 0.70);
        assert_eq!(e.saturation, 1.2);
        assert_eq!(e.noise_amount, 0.02);
    }

    // ======================================================================
    // Box blur correctness
    // ======================================================================

    #[test]
    fn test_blur_uniform_buffer_stays_uniform() {
        let color = 0xFF_80_60_40u32;
        let (w, h) = (32, 32);
        let mut buf = solid_buffer(w, h, color);

        let region = BlurRegion::new(
            0.0,
            0.0,
            w as f32,
            h as f32,
            BlurEffect::new(5.0, 1.0, Color::TRANSPARENT, 1.0, 0.0),
        );
        BlurRenderer::blur_region(&mut buf, w, h, &region);

        // A uniform image convolved with any kernel should remain uniform
        // (within rounding).
        for &px in &buf {
            let r = (px >> 16) & 0xFF;
            let g = (px >> 8) & 0xFF;
            let b = px & 0xFF;
            assert!(
                (r as i32 - 0x80).unsigned_abs() <= 2,
                "R channel drifted: {r:#X}"
            );
            assert!(
                (g as i32 - 0x60).unsigned_abs() <= 2,
                "G channel drifted: {g:#X}"
            );
            assert!(
                (b as i32 - 0x40).unsigned_abs() <= 2,
                "B channel drifted: {b:#X}"
            );
        }
    }

    #[test]
    fn test_blur_reduces_contrast() {
        // Checkerboard: alternate black and white pixels.
        let (w, h) = (64, 64);
        let mut buf = Vec::with_capacity(w as usize * h as usize);
        for row in 0..h {
            for col in 0..w {
                let c = if (row + col) % 2 == 0 {
                    0xFF_FF_FF_FF
                } else {
                    0xFF_00_00_00
                };
                buf.push(c);
            }
        }

        let region = BlurRegion::new(
            0.0,
            0.0,
            w as f32,
            h as f32,
            BlurEffect::new(4.0, 1.0, Color::TRANSPARENT, 1.0, 0.0),
        );
        BlurRenderer::blur_region(&mut buf, w, h, &region);

        // After blurring a checkerboard, interior pixels should converge toward mid-gray.
        let mid = &buf[(16 * w + 16) as usize..(16 * w + 48) as usize];
        for &px in mid {
            let r = (px >> 16) & 0xFF;
            // Should be much closer to 128 than 0 or 255.
            assert!(r > 80 && r < 180, "Expected near mid-gray, got R={r}");
        }
    }

    #[test]
    fn test_blur_zero_radius_noop() {
        let (w, h) = (16, 16);
        let original = gradient_buffer(w, h);
        let mut buf = original.clone();

        let region = BlurRegion::new(
            0.0,
            0.0,
            w as f32,
            h as f32,
            BlurEffect::new(0.0, 1.0, Color::TRANSPARENT, 1.0, 0.0),
        );
        BlurRenderer::blur_region(&mut buf, w, h, &region);

        assert_eq!(buf, original);
    }

    #[test]
    fn test_blur_disabled_region_noop() {
        let (w, h) = (16, 16);
        let original = gradient_buffer(w, h);
        let mut buf = original.clone();

        let mut region = BlurRegion::new(0.0, 0.0, w as f32, h as f32, BlurEffect::default());
        region.set_enabled(false);
        BlurRenderer::blur_region(&mut buf, w, h, &region);

        assert_eq!(buf, original);
    }

    // ======================================================================
    // Rounded corner clipping
    // ======================================================================

    #[test]
    fn test_rounded_rect_center_always_inside() {
        // A point in the center of a 100x100 rect is always inside any corner radius.
        assert!(BlurRenderer::in_rounded_rect(50, 50, 100, 100, 20.0));
    }

    #[test]
    fn test_rounded_rect_corner_outside() {
        // The very corner pixel (0,0) of a 100x100 rect with radius 20 is outside.
        assert!(!BlurRenderer::in_rounded_rect(0, 0, 100, 100, 20.0));
    }

    #[test]
    fn test_rounded_rect_just_inside_corner() {
        // A point at (radius, 0) — on the tangent of the top-left arc — should
        // be inside.
        assert!(BlurRenderer::in_rounded_rect(20, 0, 100, 100, 20.0));
    }

    #[test]
    fn test_rounded_rect_zero_radius_all_inside() {
        // Radius 0 means no rounding — everything is inside.
        assert!(BlurRenderer::in_rounded_rect(0, 0, 10, 10, 0.0));
    }

    #[test]
    fn test_blur_with_rounded_corners_skips_corners() {
        let (w, h) = (32u32, 32u32);
        let mut buf = solid_buffer(w, h, 0xFF_FF_00_00); // red
        // Put a different color as the "original" that we expect corners to keep.
        // We fill the buffer with red, then blur a region that has rounded
        // corners. The corner pixels should remain red (unblurred).
        let region = BlurRegion::new(0.0, 0.0, w as f32, h as f32, BlurEffect::default())
            .with_corner_radius(10.0);
        BlurRenderer::blur_region(&mut buf, w, h, &region);

        // The top-left corner (0,0) should be untouched (red).
        assert_eq!(buf[0], 0xFF_FF_00_00);
    }

    // ======================================================================
    // Composite blending
    // ======================================================================

    #[test]
    fn test_composite_opaque_tint() {
        let bg = vec![0xFF_80_80_80u32; 4]; // mid-gray
        let tint = Color::rgba(255, 0, 0, 255); // fully opaque red
        let out = BlurRenderer::composite_blur(&bg, tint, 2, 2);
        for &px in &out {
            let r = (px >> 16) & 0xFF;
            let g = (px >> 8) & 0xFF;
            let b = px & 0xFF;
            assert_eq!(r, 255);
            assert_eq!(g, 0);
            assert_eq!(b, 0);
        }
    }

    #[test]
    fn test_composite_transparent_tint_passthrough() {
        let bg = vec![0xFF_AA_BB_CCu32; 4];
        let tint = Color::rgba(0, 0, 0, 0); // fully transparent
        let out = BlurRenderer::composite_blur(&bg, tint, 2, 2);
        for &px in &out {
            let r = (px >> 16) & 0xFF;
            let g = (px >> 8) & 0xFF;
            let b = px & 0xFF;
            // With zero alpha tint, background should pass through.
            assert_eq!(r, 0xAA);
            assert_eq!(g, 0xBB);
            assert_eq!(b, 0xCC);
        }
    }

    #[test]
    fn test_composite_half_alpha_blends() {
        let bg = vec![0xFF_00_00_00u32; 1]; // black
        let tint = Color::rgba(255, 255, 255, 128); // ~50% white
        let out = BlurRenderer::composite_blur(&bg, tint, 1, 1);
        let r = (out[0] >> 16) & 0xFF;
        // Should be roughly 128 (half of 255).
        assert!(r > 120 && r < 136, "Expected ~128, got {r}");
    }

    // ======================================================================
    // BlurRegion pixel_bounds
    // ======================================================================

    #[test]
    fn test_region_pixel_bounds_clamp() {
        let region = BlurRegion::new(-10.0, -10.0, 100.0, 100.0, BlurEffect::default());
        let (x, y, w, h) = region.pixel_bounds(64, 64);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
        assert!(w <= 64);
        assert!(h <= 64);
    }

    #[test]
    fn test_region_pixel_bounds_fully_outside() {
        let region = BlurRegion::new(200.0, 200.0, 50.0, 50.0, BlurEffect::default());
        let (_, _, w, h) = region.pixel_bounds(100, 100);
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    // ======================================================================
    // BlurManager region management
    // ======================================================================

    #[test]
    fn test_manager_register_unregister() {
        let mut mgr = BlurManager::new();
        assert!(mgr.is_empty());
        assert_eq!(mgr.region_count(), 0);

        mgr.register(
            1,
            BlurRegion::new(0.0, 0.0, 100.0, 48.0, BlurEffect::taskbar()),
        );
        assert_eq!(mgr.region_count(), 1);
        assert!(!mgr.is_empty());

        mgr.register(
            2,
            BlurRegion::new(0.0, 0.0, 800.0, 30.0, BlurEffect::title_bar()),
        );
        assert_eq!(mgr.region_count(), 2);

        mgr.unregister(1);
        assert_eq!(mgr.region_count(), 1);
        assert!(mgr.get(1).is_none());
        assert!(mgr.get(2).is_some());
    }

    #[test]
    fn test_manager_replace_region() {
        let mut mgr = BlurManager::new();
        mgr.register(
            1,
            BlurRegion::new(0.0, 0.0, 100.0, 48.0, BlurEffect::taskbar()),
        );
        mgr.register(
            1,
            BlurRegion::new(10.0, 10.0, 200.0, 60.0, BlurEffect::menu()),
        );

        assert_eq!(mgr.region_count(), 1);
        let r = mgr.get(1).expect("region should exist");
        assert_eq!(r.x, 10.0);
        assert_eq!(r.width, 200.0);
    }

    #[test]
    fn test_manager_get_mut_marks_dirty() {
        let mut mgr = BlurManager::new();
        mgr.register(
            1,
            BlurRegion::new(0.0, 0.0, 100.0, 48.0, BlurEffect::taskbar()),
        );
        // Clear dirty flag manually.
        mgr.dirty.insert(1, false);

        let region = mgr.get_mut(1).expect("should exist");
        region.x = 50.0;

        // get_mut should have set dirty=true.
        assert_eq!(*mgr.dirty.get(&1).expect("dirty flag"), true);
    }

    // ======================================================================
    // BlurManager cache invalidation
    // ======================================================================

    #[test]
    fn test_manager_invalidate_single() {
        let mut mgr = BlurManager::new();
        mgr.register(1, BlurRegion::new(0.0, 0.0, 10.0, 10.0, BlurEffect::none()));
        // Simulate cached state.
        mgr.dirty.insert(1, false);
        mgr.cache.insert(1, PixelRect::new(10, 10));

        mgr.invalidate(1);
        assert_eq!(*mgr.dirty.get(&1).expect("dirty"), true);
    }

    #[test]
    fn test_manager_invalidate_all() {
        let mut mgr = BlurManager::new();
        mgr.register(1, BlurRegion::new(0.0, 0.0, 10.0, 10.0, BlurEffect::none()));
        mgr.register(2, BlurRegion::new(0.0, 0.0, 10.0, 10.0, BlurEffect::none()));
        mgr.dirty.insert(1, false);
        mgr.dirty.insert(2, false);

        mgr.invalidate_all();
        assert!(mgr.dirty.values().all(|&d| d));
    }

    #[test]
    fn test_manager_global_toggle() {
        let mut mgr = BlurManager::new();
        assert!(mgr.is_enabled());
        mgr.set_enabled(false);
        assert!(!mgr.is_enabled());

        // update_all should be a no-op when disabled.
        let (w, h) = (16, 16);
        let original = solid_buffer(w, h, 0xFF_AA_BB_CC);
        let mut buf = original.clone();
        mgr.register(
            1,
            BlurRegion::new(0.0, 0.0, w as f32, h as f32, BlurEffect::taskbar()),
        );
        mgr.update_all(&mut buf, w, h);
        assert_eq!(buf, original);
    }

    #[test]
    fn test_manager_update_all_modifies_buffer() {
        let (w, h) = (32, 32);
        let mut buf = gradient_buffer(w, h);
        let original = buf.clone();

        let mut mgr = BlurManager::new();
        mgr.register(
            0,
            BlurRegion::new(0.0, 0.0, w as f32, h as f32, BlurEffect::taskbar()),
        );
        mgr.update_all(&mut buf, w, h);

        // The buffer should have been modified (blur + tint applied).
        assert_ne!(buf, original);
    }

    #[test]
    fn test_manager_cached_pass_does_not_recompute() {
        let (w, h) = (16, 16);
        let mut buf = solid_buffer(w, h, 0xFF_88_88_88);

        let mut mgr = BlurManager::new();
        mgr.register(
            1,
            BlurRegion::new(0.0, 0.0, w as f32, h as f32, BlurEffect::taskbar()),
        );

        // First update computes blur.
        mgr.update_all(&mut buf, w, h);
        let after_first = buf.clone();

        // Region is now clean. Reset buffer to something different to prove
        // the cached path blits the old result.
        buf = solid_buffer(w, h, 0xFF_00_FF_00);
        mgr.update_all(&mut buf, w, h);

        // The cached blit should have overwritten the green buffer with the
        // previously computed result.
        assert_eq!(buf, after_first);
    }

    // ======================================================================
    // Edge cases
    // ======================================================================

    #[test]
    fn test_blur_region_zero_size() {
        let (w, h) = (16, 16);
        let mut buf = solid_buffer(w, h, 0xFF_AA_BB_CC);
        let original = buf.clone();

        let region = BlurRegion::new(5.0, 5.0, 0.0, 0.0, BlurEffect::default());
        BlurRenderer::blur_region(&mut buf, w, h, &region);

        assert_eq!(buf, original, "Zero-size region should be no-op");
    }

    #[test]
    fn test_blur_region_negative_dimensions_clamped() {
        // Negative width/height should be clamped to zero in constructor.
        let region = BlurRegion::new(0.0, 0.0, -10.0, -5.0, BlurEffect::default());
        assert_eq!(region.width, 0.0);
        assert_eq!(region.height, 0.0);
    }

    // ======================================================================
    // Pixel packing helpers
    // ======================================================================

    #[test]
    fn test_unpack_pack_roundtrip() {
        let px = 0xFF_AB_CD_EFu32;
        let (r, g, b) = BlurRenderer::unpack(px);
        assert_eq!(r, 0xAB);
        assert_eq!(g, 0xCD);
        assert_eq!(b, 0xEF);

        // Pack with identity reciprocal (1<<16)/1 = 65536 — should reproduce
        // the same values.
        let repacked = BlurRenderer::pack_with_inv(r, g, b, reciprocal_table(1));
        let (r2, g2, b2) = BlurRenderer::unpack(repacked);
        assert_eq!(r2, 0xAB);
        assert_eq!(g2, 0xCD);
        assert_eq!(b2, 0xEF);
    }

    #[test]
    fn test_blend_pixel_fully_opaque() {
        let src = 0xFF_FF_00_00u32; // red
        let dst = 0xFF_00_FF_00u32; // green
        let blended = BlurRenderer::blend_pixel(src, dst, 255);
        assert_eq!(blended, src);
    }

    #[test]
    fn test_blend_pixel_fully_transparent() {
        let src = 0xFF_FF_00_00u32;
        let dst = 0xFF_00_FF_00u32;
        let blended = BlurRenderer::blend_pixel(src, dst, 0);
        assert_eq!(blended, dst);
    }

    // ======================================================================
    // Deterministic noise
    // ======================================================================

    #[test]
    fn test_pixel_hash_deterministic() {
        let h1 = pixel_hash(42, 99);
        let h2 = pixel_hash(42, 99);
        assert_eq!(h1, h2, "Same input must produce same output");
    }

    #[test]
    fn test_pixel_hash_varies() {
        let h1 = pixel_hash(0, 0);
        let h2 = pixel_hash(1, 0);
        let h3 = pixel_hash(0, 1);
        // While hash collisions are theoretically possible, in practice these
        // neighbouring inputs should differ.
        assert!(h1 != h2 || h1 != h3, "Hash should vary across positions");
    }

    // ======================================================================
    // Saturation adjustment
    // ======================================================================

    #[test]
    fn test_saturation_identity() {
        let mut buf = vec![0xFF_80_40_C0u32];
        let original = buf[0];
        BlurRenderer::apply_saturation(&mut buf, 1.0);
        // With factor 1.0 the pixel should be unchanged.
        assert_eq!(buf[0], original);
    }

    #[test]
    fn test_saturation_desaturate_to_gray() {
        let mut buf = vec![0xFF_FF_00_00u32]; // pure red
        BlurRenderer::apply_saturation(&mut buf, 0.0);
        let (r, g, b) = BlurRenderer::unpack(buf[0]);
        // Factor 0 should collapse R=G=B to the luma value.
        assert_eq!(r, g);
        assert_eq!(g, b);
    }

    // ======================================================================
    // The box kernel itself
    //
    // The suite above passed for as long as this module existed while the
    // sliding window was seeded one sample short — every one of those tests
    // asks only whether the output got blurrier, which a wrong kernel does
    // just as well as a right one. These ask whether it is *the* kernel.
    // ======================================================================

    /// Blur one line by the kernel's definition: the mean of the window
    /// `[i - radius, i + radius]`, with positions off either end taking that
    /// end's pixel. Deliberately the slow, obvious formulation — it is the
    /// specification the fast sliding-window version has to agree with.
    fn reference_blur_line(line: &[u32], radius: usize) -> Vec<u32> {
        let n = line.len();
        if n == 0 {
            return Vec::new();
        }
        let radius = radius.min(n);
        (0..n)
            .map(|i| {
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for k in 0..=(2 * radius) {
                    let j = (i + k).saturating_sub(radius).min(n - 1);
                    let px = line[j];
                    r += (px >> 16) & 0xFF;
                    g += (px >> 8) & 0xFF;
                    b += px & 0xFF;
                }
                let d = (2 * radius + 1) as u32;
                let avg = |v: u32| ((v * 2 + d) / (d * 2)).min(255);
                0xFF00_0000 | (avg(r) << 16) | (avg(g) << 8) | avg(b)
            })
            .collect()
    }

    fn gray(v: u32) -> u32 {
        0xFF00_0000 | (v << 16) | (v << 8) | v
    }

    fn blur_line_via_module(line: &[u32], radius: usize) -> Vec<u32> {
        let mut out = vec![0u32; line.len()];
        box_blur_line(
            line.len(),
            radius,
            |i| line.get(i).copied().unwrap_or(0),
            |i, px| {
                if let Some(slot) = out.get_mut(i) {
                    *slot = px;
                }
            },
        );
        out
    }

    #[test]
    fn the_sliding_window_agrees_with_the_kernels_own_definition() {
        // Several shapes, each at several radii, against the brute-force mean.
        let lines: Vec<Vec<u32>> = vec![
            vec![10, 20, 30, 20, 10].into_iter().map(gray).collect(),
            vec![0, 0, 255, 0, 0, 0, 0, 0]
                .into_iter()
                .map(gray)
                .collect(),
            (0..40u32).map(|v| gray(v * 6)).collect(),
            vec![gray(255); 9],
            vec![gray(7)],
        ];
        for line in &lines {
            for radius in 0..=6 {
                let got = blur_line_via_module(line, radius);
                let want = reference_blur_line(line, radius);
                // The module divides by a fixed-point reciprocal rather than
                // exactly, so allow the one greylevel that costs; the point
                // of the test is the window's *contents*, not its rounding.
                assert_eq!(got.len(), want.len());
                for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
                    let (gv, wv) = (g & 0xFF, w & 0xFF);
                    assert!(
                        gv.abs_diff(wv) <= 1,
                        "radius {radius}, pixel {i}: got {gv}, kernel says {wv} \
                         (line len {})",
                        line.len()
                    );
                }
            }
        }
    }

    #[test]
    fn a_palindromic_line_blurs_to_a_palindrome() {
        // A centred kernel with symmetric edge handling cannot turn a
        // symmetric line into an asymmetric one. The old implementation could:
        // its window sat half a sample to the left of where it claimed to be.
        let line: Vec<u32> = vec![10u32, 20, 90, 200, 90, 20, 10]
            .into_iter()
            .map(gray)
            .collect();
        for radius in 0..=8 {
            let out = blur_line_via_module(&line, radius);
            for i in 0..out.len() {
                let mirror = out.len() - 1 - i;
                assert_eq!(
                    out[i] & 0xFF,
                    out[mirror] & 0xFF,
                    "radius {radius}: pixel {i} and its mirror {mirror} differ"
                );
            }
        }
    }

    #[test]
    fn a_flat_line_survives_any_radius_unchanged() {
        let line = vec![gray(0x5A); 12];
        for radius in 0..=30 {
            for (i, &px) in blur_line_via_module(&line, radius).iter().enumerate() {
                assert!(
                    (px & 0xFF).abs_diff(0x5A) <= 1,
                    "radius {radius}, pixel {i}: flat input drifted to {:#X}",
                    px & 0xFF
                );
            }
        }
    }

    #[test]
    fn a_blurred_point_of_light_stays_where_it_was_put() {
        // The bug this catches is the one a user would actually see: a blur
        // whose kernel is off-centre slides the frosted backdrop away from the
        // wallpaper it is frosting. A single bright pixel at the exact centre
        // of a square must blur to a pattern symmetric about both axes.
        let n = 33u32;
        let mut buf = vec![0xFF00_0000u32; (n * n) as usize];
        let centre = (n / 2 * n + n / 2) as usize;
        buf[centre] = 0xFF_FF_FF_FF;

        let region = BlurRegion::new(
            0.0,
            0.0,
            n as f32,
            n as f32,
            BlurEffect::new(3.0, 1.0, Color::TRANSPARENT, 1.0, 0.0),
        );
        BlurRenderer::blur_region(&mut buf, n, n, &region);

        let at = |col: u32, row: u32| buf[(row * n + col) as usize] & 0xFF;
        for row in 0..n {
            for col in 0..n {
                assert_eq!(
                    at(col, row),
                    at(n - 1 - col, row),
                    "left/right asymmetry at ({col}, {row})"
                );
                assert_eq!(
                    at(col, row),
                    at(col, n - 1 - row),
                    "top/bottom asymmetry at ({col}, {row})"
                );
                // Transposing is not an exact symmetry of a separable integer
                // blur: the horizontal pass rounds to whole greylevels before
                // the vertical one reads them, so rows-then-columns and
                // columns-then-rows can land a level apart. The alignment of
                // the two axes — which is what a mis-seeded window breaks — is
                // asserted exactly by the two mirror checks above.
                assert!(
                    at(col, row).abs_diff(at(row, col)) <= 1,
                    "the horizontal and vertical passes disagree at ({col}, {row}): \
                     {} vs {}",
                    at(col, row),
                    at(row, col)
                );
            }
        }
        assert!(
            at(n / 2, n / 2) > at(0, 0),
            "the light should still be brightest where it was placed"
        );
    }

    // ======================================================================
    // PixelRect
    // ======================================================================

    #[test]
    fn sampling_outside_a_rect_repeats_its_edge() {
        let mut rect = PixelRect::new(3, 2);
        for row in 0..2 {
            for col in 0..3 {
                rect.set(col, row, gray((col * 10 + row) as u32));
            }
        }
        // Past the right edge and past the bottom, in both directions.
        assert_eq!(rect.sample(99, 0), rect.sample(2, 0));
        assert_eq!(rect.sample(0, 99), rect.sample(0, 1));
        assert_eq!(rect.sample(usize::MAX, usize::MAX), rect.sample(2, 1));
        // Inside, sampling and getting agree.
        assert_eq!(rect.sample(1, 1), rect.get(1, 1).expect("in range"));
    }

    #[test]
    fn a_rect_cannot_be_grown_by_writing_past_its_edge() {
        let mut rect = PixelRect::new(2, 2);
        let before = rect.pixels().len();
        rect.set(2, 0, 0xFF_FF_FF_FF);
        rect.set(0, 2, 0xFF_FF_FF_FF);
        rect.set(usize::MAX, usize::MAX, 0xFF_FF_FF_FF);
        assert_eq!(rect.pixels().len(), before);
        assert!(rect.pixels().iter().all(|&px| px == OPAQUE_BLACK));
    }

    #[test]
    fn an_empty_rect_is_safe_to_blur_sample_and_blit() {
        for (w, h) in [(0u32, 0u32), (0, 4), (4, 0)] {
            let mut rect = PixelRect::new(w, h);
            assert!(rect.is_empty());
            assert_eq!(rect.sample(0, 0), OPAQUE_BLACK);
            rect.set(0, 0, 0xFF_FF_FF_FF);
            BlurRenderer::box_blur_pass(&mut rect, 4);
            let mut fb = vec![0u32; 16];
            rect.blit_into(&mut fb, 4, 0, 0);
            assert!(fb.iter().all(|&px| px == 0), "{w}x{h} rect wrote something");
        }
    }

    #[test]
    fn a_region_at_the_right_edge_does_not_wrap_onto_the_next_scanline() {
        // The old write-back checked only `index < buffer.len()`, so a row
        // whose width overhung the framebuffer spilled onto the start of the
        // row below. `pixel_bounds` happened to prevent that — a fact proved
        // two functions away from the loop that depended on it.
        let (fb_w, fb_h) = (8usize, 4usize);
        let mut fb = vec![0u32; fb_w * fb_h];
        // A 4-wide rect placed so that half of it hangs off the right edge.
        let mut rect = PixelRect::new(4, 2);
        for px in rect.pixels_mut() {
            *px = 0xFF_FF_FF_FF;
        }
        rect.blit_into(&mut fb, fb_w as u32, 6, 1);

        for row in 0..fb_h {
            for col in 0..fb_w {
                let want_lit = (1..3).contains(&row) && col >= 6;
                let got = fb[row * fb_w + col];
                assert_eq!(
                    got != 0,
                    want_lit,
                    "pixel ({col}, {row}) should {} be written",
                    if want_lit { "" } else { "not" }
                );
            }
        }
    }

    #[test]
    fn extracting_past_the_framebuffer_yields_black_not_a_short_buffer() {
        // Callers index the extracted rect by its own dimensions, so it has to
        // be exactly the size asked for even when the framebuffer runs out.
        let fb = vec![0xFF_11_22_33u32; 4 * 4];
        let rect = PixelRect::from_framebuffer(&fb, 4, 2, 2, 4, 4);
        assert_eq!(rect.pixels().len(), 16);
        assert_eq!(rect.get(0, 0), Some(0xFF_11_22_33));
        // Past the framebuffer's right edge and bottom.
        assert_eq!(rect.get(3, 0), Some(OPAQUE_BLACK));
        assert_eq!(rect.get(0, 3), Some(OPAQUE_BLACK));
    }

    #[test]
    fn the_accumulator_saturates_rather_than_wrapping() {
        // The window's arithmetic is balanced by construction, but the
        // subtraction must not be able to wrap a channel from 0 to four
        // billion and paint white where black belongs.
        let zero = Rgb::default();
        let one = Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(zero.minus(one), zero);
        assert_eq!(
            Rgb {
                r: u32::MAX,
                g: 0,
                b: 0
            }
            .plus(one)
            .r,
            u32::MAX
        );
        assert_eq!(Rgb { r: 2, g: 0, b: 0 }.scaled(u32::MAX).r, u32::MAX);
        // And packing clamps rather than bleeding into the next channel.
        assert_eq!(
            Rgb {
                r: 999,
                g: 999,
                b: 999
            }
            .to_argb(),
            0xFF_FF_FF_FF
        );
    }
}
