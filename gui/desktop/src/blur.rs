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
        Self::new(0.0, 1.0, Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 255), 1.0, 0.0)
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
    pub fn blur_region(
        buffer: &mut [u32],
        fb_width: u32,
        fb_height: u32,
        region: &BlurRegion,
    ) {
        if !region.enabled || region.effect.radius < 0.5 {
            return;
        }

        let (rx, ry, rw, rh) = region.pixel_bounds(fb_width, fb_height);
        if rw == 0 || rh == 0 {
            return;
        }

        // Extract sub-buffer for the region.
        let mut sub = Self::extract_sub(buffer, fb_width, rx, ry, rw, rh);

        // Three-pass box blur (approximates Gaussian).
        let radius = region.effect.radius as u32;
        let radius = radius.max(1);
        Self::box_blur_pass(&mut sub, rw, rh, radius);
        Self::box_blur_pass(&mut sub, rw, rh, radius);
        Self::box_blur_pass(&mut sub, rw, rh, radius);

        // Apply saturation adjustment.
        if (region.effect.saturation - 1.0).abs() > 0.01 {
            Self::apply_saturation(&mut sub, region.effect.saturation);
        }

        // Apply noise texture.
        if region.effect.noise_amount > 0.001 {
            Self::apply_noise(&mut sub, rw, rh, region.effect.noise_amount);
        }

        // Write back with rounded corner mask and opacity.
        Self::write_back_with_clip(
            buffer,
            fb_width,
            &sub,
            rx,
            ry,
            rw,
            rh,
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
        let inv_ta = 255u32.saturating_sub(ta);
        let tr = ((tint_argb >> 16) & 0xFF);
        let tg = ((tint_argb >> 8) & 0xFF);
        let tb = (tint_argb & 0xFF);

        let mut out = Vec::with_capacity(len);
        for &bg_px in background.iter().take(len) {
            let br = ((bg_px >> 16) & 0xFF);
            let bg_g = ((bg_px >> 8) & 0xFF);
            let bb = (bg_px & 0xFF);

            let r = (tr * ta + br * inv_ta) / 255;
            let g = (tg * ta + bg_g * inv_ta) / 255;
            let b = (tb * ta + bb * inv_ta) / 255;

            out.push(0xFF00_0000 | (r.min(255) << 16) | (g.min(255) << 8) | b.min(255));
        }
        out
    }

    // ------------------------------------------------------------------
    // Internal: sub-buffer extraction / write-back
    // ------------------------------------------------------------------

    /// Copy a rectangular region from the framebuffer into a contiguous Vec.
    fn extract_sub(
        buffer: &[u32],
        fb_width: u32,
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
    ) -> Vec<u32> {
        let fb_w = fb_width as usize;
        let rw_us = rw as usize;
        let mut sub = Vec::with_capacity(rw_us * rh as usize);
        for row in 0..rh {
            let start = (ry + row) as usize * fb_w + rx as usize;
            let end = start + rw_us;
            if end <= buffer.len() {
                sub.extend_from_slice(&buffer[start..end]);
            } else {
                // Pad with black if out-of-bounds (should not happen with clamping).
                sub.resize(sub.len() + rw_us, 0xFF00_0000);
            }
        }
        sub
    }

    /// Write the processed sub-buffer back, applying rounded-corner masking
    /// and global opacity.
    fn write_back_with_clip(
        buffer: &mut [u32],
        fb_width: u32,
        sub: &[u32],
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        corner_radius: f32,
        opacity: f32,
    ) {
        let fb_w = fb_width as usize;
        let rw_us = rw as usize;
        let op = (opacity.clamp(0.0, 1.0) * 255.0) as u32;

        for row in 0..rh {
            let fb_row_start = (ry + row) as usize * fb_w + rx as usize;
            let sub_row_start = row as usize * rw_us;

            for col in 0..rw {
                let sub_idx = sub_row_start + col as usize;
                let fb_idx = fb_row_start + col as usize;
                if fb_idx >= buffer.len() || sub_idx >= sub.len() {
                    continue;
                }

                // Rounded corner test.
                if corner_radius > 0.5 && !Self::in_rounded_rect(col, row, rw, rh, corner_radius) {
                    continue; // Leave the original pixel.
                }

                let src = sub[sub_idx];
                if op >= 255 {
                    buffer[fb_idx] = src;
                } else {
                    let dst = buffer[fb_idx];
                    buffer[fb_idx] = Self::blend_pixel(src, dst, op);
                }
            }
        }
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
        let cx = if in_left {
            r
        } else {
            w as f32 - r
        };
        let cy = if in_top {
            r
        } else {
            h as f32 - r
        };

        let dx = col as f32 + 0.5 - cx;
        let dy = row as f32 + 0.5 - cy;
        dx * dx + dy * dy <= r * r
    }

    // ------------------------------------------------------------------
    // Internal: box blur (separable horizontal + vertical)
    // ------------------------------------------------------------------

    /// Single box blur pass (horizontal then vertical).
    fn box_blur_pass(buf: &mut [u32], width: u32, height: u32, radius: u32) {
        let mut tmp = vec![0u32; buf.len()];
        Self::box_blur_h(buf, &mut tmp, width, height, radius);
        Self::box_blur_v(&tmp, buf, width, height, radius);
    }

    /// Horizontal box blur: `src` → `dst`.
    fn box_blur_h(src: &[u32], dst: &mut [u32], width: u32, height: u32, radius: u32) {
        let w = width as usize;
        let r = radius as usize;
        let diameter = 2 * r + 1;
        let inv = reciprocal_table(diameter as u32);

        for row in 0..height as usize {
            let row_start = row * w;

            // Seed the running sum with the leftmost pixel replicated for the
            // out-of-bounds region, plus the first `radius` real pixels.
            let first = Self::unpack(src[row_start]);
            let (mut sr, mut sg, mut sb) = (
                first.0 * (r as u32 + 1),
                first.1 * (r as u32 + 1),
                first.2 * (r as u32 + 1),
            );

            for i in 0..r.min(w) {
                let px = Self::unpack(src[row_start + i]);
                sr += px.0;
                sg += px.1;
                sb += px.2;
            }
            // Replicate edge for initial right side.
            if r > w {
                let edge = Self::unpack(src[row_start + w.saturating_sub(1)]);
                let extra = (r - w) as u32;
                sr += edge.0 * extra;
                sg += edge.1 * extra;
                sb += edge.2 * extra;
            }

            for col in 0..w {
                dst[row_start + col] = Self::pack_with_inv(sr, sg, sb, inv);

                // Advance the sliding window.
                let add_idx = (col + r + 1).min(w - 1);
                let rem_idx = col.saturating_sub(r);

                let add = Self::unpack(src[row_start + add_idx]);
                let rem = Self::unpack(src[row_start + rem_idx]);

                sr = sr.wrapping_add(add.0).wrapping_sub(rem.0);
                sg = sg.wrapping_add(add.1).wrapping_sub(rem.1);
                sb = sb.wrapping_add(add.2).wrapping_sub(rem.2);
            }
        }
    }

    /// Vertical box blur: `src` → `dst`.
    fn box_blur_v(src: &[u32], dst: &mut [u32], width: u32, height: u32, radius: u32) {
        let w = width as usize;
        let h = height as usize;
        let r = radius as usize;
        let diameter = 2 * r + 1;
        let inv = reciprocal_table(diameter as u32);

        for col in 0..w {
            let first = Self::unpack(src[col]);
            let (mut sr, mut sg, mut sb) = (
                first.0 * (r as u32 + 1),
                first.1 * (r as u32 + 1),
                first.2 * (r as u32 + 1),
            );

            for i in 0..r.min(h) {
                let px = Self::unpack(src[i * w + col]);
                sr += px.0;
                sg += px.1;
                sb += px.2;
            }
            if r > h {
                let edge = Self::unpack(src[h.saturating_sub(1) * w + col]);
                let extra = (r - h) as u32;
                sr += edge.0 * extra;
                sg += edge.1 * extra;
                sb += edge.2 * extra;
            }

            for row in 0..h {
                dst[row * w + col] = Self::pack_with_inv(sr, sg, sb, inv);

                let add_idx = (row + r + 1).min(h - 1);
                let rem_idx = row.saturating_sub(r);

                let add = Self::unpack(src[add_idx * w + col]);
                let rem = Self::unpack(src[rem_idx * w + col]);

                sr = sr.wrapping_add(add.0).wrapping_sub(rem.0);
                sg = sg.wrapping_add(add.1).wrapping_sub(rem.1);
                sb = sb.wrapping_add(add.2).wrapping_sub(rem.2);
            }
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
            let (r, g, b) = Self::unpack(*px);
            // Luma (Rec. 709 coefficients).
            let luma = (r as f32 * 0.2126 + g as f32 * 0.7152 + b as f32 * 0.0722) as u32;
            let nr = Self::sat_channel(r, luma, factor);
            let ng = Self::sat_channel(g, luma, factor);
            let nb = Self::sat_channel(b, luma, factor);
            *px = 0xFF00_0000 | (nr << 16) | (ng << 8) | nb;
        }
    }

    fn sat_channel(val: u32, luma: u32, factor: f32) -> u32 {
        let v = luma as f32 + (val as f32 - luma as f32) * factor;
        (v.round() as u32).min(255)
    }

    /// Add a subtle deterministic noise pattern.
    ///
    /// Uses a simple hash based on pixel position rather than a PRNG so results
    /// are reproducible and cache-friendly.
    fn apply_noise(buf: &mut [u32], width: u32, height: u32, amount: f32) {
        let strength = (amount * 255.0) as i32;
        if strength == 0 {
            return;
        }
        for row in 0..height {
            for col in 0..width {
                let idx = row as usize * width as usize + col as usize;
                if idx >= buf.len() {
                    continue;
                }
                // Simple spatial hash → value in [-strength, +strength].
                let hash = pixel_hash(col, row);
                let noise = (hash % (2 * strength as u32 + 1)) as i32 - strength;

                let (r, g, b) = Self::unpack(buf[idx]);
                let nr = (r as i32 + noise).clamp(0, 255) as u32;
                let ng = (g as i32 + noise).clamp(0, 255) as u32;
                let nb = (b as i32 + noise).clamp(0, 255) as u32;
                buf[idx] = 0xFF00_0000 | (nr << 16) | (ng << 8) | nb;
            }
        }
    }

    // ------------------------------------------------------------------
    // Internal: pixel packing helpers
    // ------------------------------------------------------------------

    /// Unpack ARGB u32 into (R, G, B) as u32 for accumulation.
    #[inline]
    fn unpack(px: u32) -> (u32, u32, u32) {
        ((px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
    }

    /// Pack RGB channels using a pre-computed reciprocal (fixed-point multiply
    /// instead of integer division).
    ///
    /// Adds 0x8000 (half the fixed-point unit) before the shift so the division
    /// rounds to nearest rather than truncating. With pure truncation each box
    /// blur pass loses ~1 per channel (because `reciprocal_table` rounds the
    /// reciprocal down), so the 3-pass × 2-direction pipeline drifted uniform
    /// images by up to 6. Rounding to nearest keeps uniform images stable.
    #[inline]
    fn pack_with_inv(sr: u32, sg: u32, sb: u32, inv: u32) -> u32 {
        let r = ((sr * inv + 0x8000) >> 16).min(255);
        let g = ((sg * inv + 0x8000) >> 16).min(255);
        let b = ((sb * inv + 0x8000) >> 16).min(255);
        0xFF00_0000 | (r << 16) | (g << 8) | b
    }

    /// Alpha-blend `src` over `dst` with the given source alpha (0..255).
    #[inline]
    fn blend_pixel(src: u32, dst: u32, alpha: u32) -> u32 {
        let inv = 255u32.saturating_sub(alpha);

        let sr = (src >> 16) & 0xFF;
        let sg = (src >> 8) & 0xFF;
        let sb = src & 0xFF;

        let dr = (dst >> 16) & 0xFF;
        let dg = (dst >> 8) & 0xFF;
        let db = dst & 0xFF;

        let r = (sr * alpha + dr * inv) / 255;
        let g = (sg * alpha + dg * inv) / 255;
        let b = (sb * alpha + db * inv) / 255;

        0xFF00_0000 | (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
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
    cache: HashMap<u64, Vec<u32>>,
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

            // Use cache if not dirty and cache exists.
            if !is_dirty
                && let Some(cached) = self.cache.get(&id) {
                    if let Some(region) = self.regions.get(&id)
                        && region.enabled {
                            let (rx, ry, rw, rh) = region.pixel_bounds(fb_width, fb_height);
                            Self::blit_cached(buffer, fb_width, cached, rx, ry, rw, rh);
                        }
                    continue;
                }

            // Need to recompute.
            if let Some(region) = self.regions.get(&id) {
                if !region.enabled {
                    continue;
                }
                let region_clone = region.clone();
                let (rx, ry, rw, rh) = region_clone.pixel_bounds(fb_width, fb_height);
                if rw == 0 || rh == 0 {
                    continue;
                }

                // Apply blur to the framebuffer in-place.
                BlurRenderer::blur_region(buffer, fb_width, fb_height, &region_clone);

                // Composite the tint over the blurred region.
                let sub = BlurRenderer::extract_sub(buffer, fb_width, rx, ry, rw, rh);
                let composited =
                    BlurRenderer::composite_blur(&sub, region_clone.effect.tint, rw, rh);

                // Write composited result back.
                let fb_w = fb_width as usize;
                let rw_us = rw as usize;
                for row in 0..rh {
                    let fb_start = (ry + row) as usize * fb_w + rx as usize;
                    let sub_start = row as usize * rw_us;
                    for col in 0..rw_us {
                        let fb_idx = fb_start + col;
                        let sub_idx = sub_start + col;
                        if fb_idx < buffer.len() && sub_idx < composited.len() {
                            buffer[fb_idx] = composited[sub_idx];
                        }
                    }
                }

                // Cache the result.
                self.cache.insert(id, composited);
                self.dirty.insert(id, false);
            }
        }
    }

    /// Blit cached pixel data back into the framebuffer.
    fn blit_cached(
        buffer: &mut [u32],
        fb_width: u32,
        cached: &[u32],
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
    ) {
        let fb_w = fb_width as usize;
        let rw_us = rw as usize;
        for row in 0..rh {
            let fb_start = (ry + row) as usize * fb_w + rx as usize;
            let sub_start = row as usize * rw_us;
            for col in 0..rw_us {
                let fb_idx = fb_start + col;
                let sub_idx = sub_start + col;
                if fb_idx < buffer.len() && sub_idx < cached.len() {
                    buffer[fb_idx] = cached[sub_idx];
                }
            }
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

/// Fixed-point reciprocal (16-bit fractional) for integer division avoidance.
///
/// Returns `(1 << 16) / n` — multiply an accumulated sum by this value and
/// shift right by 16 to get the average.
#[inline]
fn reciprocal_table(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    (1u32 << 16) / n
}

/// Deterministic spatial hash for noise generation.
///
/// Produces a pseudo-random u32 from (x, y) coordinates.
#[inline]
fn pixel_hash(x: u32, y: u32) -> u32 {
    // Minimal hash — good enough for visual noise, not crypto.
    let mut h = x.wrapping_mul(374_761_393).wrapping_add(y.wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
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
            assert!((r as i32 - 0x80).unsigned_abs() <= 2, "R channel drifted: {r:#X}");
            assert!((g as i32 - 0x60).unsigned_abs() <= 2, "G channel drifted: {g:#X}");
            assert!((b as i32 - 0x40).unsigned_abs() <= 2, "B channel drifted: {b:#X}");
        }
    }

    #[test]
    fn test_blur_reduces_contrast() {
        // Checkerboard: alternate black and white pixels.
        let (w, h) = (64, 64);
        let mut buf = Vec::with_capacity(w as usize * h as usize);
        for row in 0..h {
            for col in 0..w {
                let c = if (row + col) % 2 == 0 { 0xFF_FF_FF_FF } else { 0xFF_00_00_00 };
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

        let mut region = BlurRegion::new(
            0.0,
            0.0,
            w as f32,
            h as f32,
            BlurEffect::default(),
        );
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

        mgr.register(1, BlurRegion::new(0.0, 0.0, 100.0, 48.0, BlurEffect::taskbar()));
        assert_eq!(mgr.region_count(), 1);
        assert!(!mgr.is_empty());

        mgr.register(2, BlurRegion::new(0.0, 0.0, 800.0, 30.0, BlurEffect::title_bar()));
        assert_eq!(mgr.region_count(), 2);

        mgr.unregister(1);
        assert_eq!(mgr.region_count(), 1);
        assert!(mgr.get(1).is_none());
        assert!(mgr.get(2).is_some());
    }

    #[test]
    fn test_manager_replace_region() {
        let mut mgr = BlurManager::new();
        mgr.register(1, BlurRegion::new(0.0, 0.0, 100.0, 48.0, BlurEffect::taskbar()));
        mgr.register(1, BlurRegion::new(10.0, 10.0, 200.0, 60.0, BlurEffect::menu()));

        assert_eq!(mgr.region_count(), 1);
        let r = mgr.get(1).expect("region should exist");
        assert_eq!(r.x, 10.0);
        assert_eq!(r.width, 200.0);
    }

    #[test]
    fn test_manager_get_mut_marks_dirty() {
        let mut mgr = BlurManager::new();
        mgr.register(1, BlurRegion::new(0.0, 0.0, 100.0, 48.0, BlurEffect::taskbar()));
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
        mgr.cache.insert(1, vec![0u32; 100]);

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
        mgr.register(1, BlurRegion::new(0.0, 0.0, w as f32, h as f32, BlurEffect::taskbar()));
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
}
