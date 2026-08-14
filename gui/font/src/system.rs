//! One font type the rest of the OS draws with, whatever the font came from.
//!
//! There are two glyph sources in this crate and they have nothing in common
//! at the type level: [`ScaledFont`] rasterizes outlines from a real font file
//! at an arbitrary pixel size with anti-aliasing, while [`Font`] is the
//! built-in procedural 8x16 bitmap face that exists so text can appear before
//! there is a filesystem to load a font *from*.
//!
//! Without a facade, every caller — compositor, toolkit, desktop, each app —
//! has to know which one it has and branch. That is exactly what happened
//! before this module existed: the compositor grew its own private 8x14
//! bitmap font rather than depend on this crate at all, so the OS shipped two
//! hand-drawn faces and used the worse one. [`SystemFont`] is the single type
//! those callers hold, so the choice of backend is made once, at load time.
//!
//! # Coordinates
//!
//! Pixels, y down, and `y` in [`SystemFont::draw_text`] is the **baseline** —
//! matching [`ScaledFont`]. The bitmap backend converts internally: its glyphs
//! record `bearing_y`, the distance from the baseline up to the top of the
//! cell.

use alloc::string::String;
use alloc::vec::Vec;

use crate::raster::GlyphMask;
use crate::scaled::{ScaledFont, ScaledFontError, Target, blit_mask, pixel_coord};
use crate::{FONT_HEIGHT, Font, FontMetrics, GlyphBitmap};

/// A font that can draw text, backed by either an outline face or the
/// built-in bitmap face.
#[derive(Debug)]
pub struct SystemFont {
    backend: Backend,
}

#[derive(Debug)]
enum Backend {
    /// A scalable face loaded from a font file.
    Outline(ScaledFont),
    /// The built-in bitmap face, scaled by an integer factor.
    Bitmap(Font),
}

impl SystemFont {
    /// The built-in bitmap face at the integer scale closest to `px_per_em`.
    ///
    /// The bitmap face only exists at whole multiples of its 8x16 cell, so the
    /// requested size is rounded rather than honoured: asking for 20 px gets
    /// the 16 px face, not a blurry stretch of it. Callers that need the exact
    /// size must supply a real font file.
    #[must_use]
    pub fn builtin(px_per_em: f32) -> Self {
        Self {
            backend: Backend::Bitmap(Font::scaled(&Font::system_mono(), builtin_scale(px_per_em))),
        }
    }

    /// The built-in bold bitmap face at the integer scale closest to
    /// `px_per_em`.
    #[must_use]
    pub fn builtin_bold(px_per_em: f32) -> Self {
        Self {
            backend: Backend::Bitmap(Font::scaled(
                &Font::system_mono_bold(),
                builtin_scale(px_per_em),
            )),
        }
    }

    /// Loads an outline face from font-file bytes.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`ScaledFontError`] if the bytes are not a
    /// TrueType face this crate can read — see [`or_builtin`] for the
    /// "draw something regardless" path.
    ///
    /// [`or_builtin`]: SystemFont::or_builtin
    pub fn from_bytes(data: Vec<u8>, px_per_em: f32) -> Result<Self, ScaledFontError> {
        Ok(Self {
            backend: Backend::Outline(ScaledFont::from_bytes(data, px_per_em)?),
        })
    }

    /// Loads an outline face, falling back to the built-in bitmap face.
    ///
    /// The fallback is silent by design: a font file that turns out to be CFF,
    /// truncated, or simply not a font is a *configuration* problem, and the
    /// user is better served by ugly text than by a blank screen. Callers that
    /// want to report the failure should use [`SystemFont::from_bytes`] and
    /// decide for themselves.
    #[must_use]
    pub fn or_builtin(data: Vec<u8>, px_per_em: f32) -> Self {
        Self::from_bytes(data, px_per_em).unwrap_or_else(|_| Self::builtin(px_per_em))
    }

    /// Whether this font came from a real font file.
    ///
    /// Worth knowing because the bitmap fallback is monospace and covers only
    /// Latin-1, box drawing and block elements: a caller rendering user text
    /// may want to say so rather than fill the screen with tofu.
    #[must_use]
    pub fn is_scalable(&self) -> bool {
        matches!(self.backend, Backend::Outline(_))
    }

    /// The font's vertical metrics, in pixels.
    #[must_use]
    pub fn metrics(&self) -> &FontMetrics {
        match &self.backend {
            Backend::Outline(f) => f.metrics(),
            Backend::Bitmap(f) => f.metrics(),
        }
    }

    /// Baseline-to-baseline distance in pixels.
    #[must_use]
    pub fn line_height(&self) -> f32 {
        self.metrics().line_height
    }

    /// Width of `text` in pixels, ignoring line breaks.
    #[must_use]
    pub fn measure(&self, text: &str) -> f32 {
        match &self.backend {
            Backend::Outline(f) => f.measure(text),
            Backend::Bitmap(f) => f.measure_line(text),
        }
    }

    /// Breaks `text` into lines no wider than `max_width`, at whitespace.
    #[must_use]
    pub fn wrap(&self, text: &str, max_width: f32) -> Vec<String> {
        match &self.backend {
            Backend::Outline(f) => f.wrap(text, max_width),
            // The bitmap face has no wrapper of its own; `ScaledFont`'s rule
            // (break at spaces, never inside a word) is not outline-specific,
            // so it is reimplemented here against `measure` rather than
            // duplicated into the bitmap type.
            Backend::Bitmap(_) => wrap_with(text, max_width, &|s| self.measure(s)),
        }
    }

    /// Draws `text` with its baseline at `y`, starting at pen position `x`.
    ///
    /// Returns the pen position after the last glyph, so runs can be chained
    /// without re-measuring.
    pub fn draw_text(&mut self, text: &str, target: &mut Target<'_>, x: f32, y: f32) -> f32 {
        match &mut self.backend {
            Backend::Outline(f) => f.draw_text(text, target, x, y),
            Backend::Bitmap(f) => draw_bitmap_text(f, text, target, x, y),
        }
    }

    /// The outline face behind this font, if there is one.
    ///
    /// Exposed for callers that need something only the scalable path has —
    /// the glyph cache statistics, or the underlying `Face` for a shaper.
    #[must_use]
    pub fn as_scaled(&self) -> Option<&ScaledFont> {
        match &self.backend {
            Backend::Outline(f) => Some(f),
            Backend::Bitmap(_) => None,
        }
    }
}

/// Picks the integer scale of the built-in face closest to `px_per_em`.
///
/// Clamped to at least 1: a zero or negative request would otherwise produce a
/// zero-size font whose every glyph is empty, which looks like a rendering bug
/// rather than a bad argument.
fn builtin_scale(px_per_em: f32) -> u32 {
    if !px_per_em.is_finite() || px_per_em <= 0.0 {
        return 1;
    }
    let cell = FONT_HEIGHT as f32;
    // `px_per_em` is finite and positive and `cell` is 16, so the quotient is
    // finite; the clamp keeps the cast in range whatever the caller asked for.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scale = (px_per_em / cell).round().clamp(1.0, 64.0) as u32;
    scale
}

/// Shared line-breaking rule, parameterised by how a run is measured.
///
/// Identical to [`ScaledFont::wrap`]: break at spaces, and leave a word that
/// is longer than `max_width` alone on an over-long line rather than cutting
/// it, because breaking inside a word is a per-script decision that belongs to
/// a real line breaker.
fn wrap_with(text: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            if line.is_empty() {
                line.push_str(word);
                continue;
            }
            let mut candidate = line.clone();
            candidate.push(' ');
            candidate.push_str(word);
            if measure(&candidate) <= max_width {
                line = candidate;
            } else {
                lines.push(core::mem::take(&mut line));
                line.push_str(word);
            }
        }
        lines.push(line);
    }
    lines
}

/// Draws a run with the bitmap backend.
///
/// The 1-bit glyphs are widened to the same 8-bit coverage the outline path
/// produces so both backends go through one blitter: a bitmap pixel is simply
/// fully covered or not covered at all. That costs one allocation per glyph
/// drawn, which is why it is the fallback path and not the main one.
fn draw_bitmap_text(font: &Font, text: &str, target: &mut Target<'_>, x: f32, y: f32) -> f32 {
    let mut pen = x;
    for ch in text.chars() {
        let glyph = font.glyph(ch);
        let mask = mask_from_bitmap(glyph);
        // The pen and baseline are caller-supplied and may be anything, which
        // is why the position goes through `pixel_coord` (which rejects the
        // degenerate cases) and then `blit_mask` (which clips the rest).
        if let (Some(gx), Some(gy)) = (pixel_coord(pen), pixel_coord(y - glyph.bearing_y)) {
            blit_mask(&mask, target, gx, gy);
        }
        pen += glyph.advance;
    }
    pen
}

/// Expands a 1-bit glyph into a coverage mask.
fn mask_from_bitmap(glyph: &GlyphBitmap) -> GlyphMask {
    let mut coverage =
        Vec::with_capacity((glyph.width as usize).saturating_mul(glyph.height as usize));
    for y in 0..glyph.height {
        for x in 0..glyph.width {
            coverage.push(if glyph.pixel_at(x, y) { 255 } else { 0 });
        }
    }
    GlyphMask {
        width: glyph.width,
        height: glyph.height,
        // `blit_mask` places the mask's top-left corner, and the caller has
        // already subtracted `bearing_y` from the baseline, so both offsets
        // are zero here rather than repeating that adjustment.
        left: 0,
        top: 0,
        coverage,
    }
}

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
    use crate::sfnt::tests::build_test_font;

    fn surface(w: u32, h: u32) -> Vec<u32> {
        alloc::vec![0xFF00_0000_u32; (w * h) as usize]
    }

    #[test]
    fn builtin_scale_rounds_to_whole_cells() {
        assert_eq!(builtin_scale(16.0), 1);
        assert_eq!(builtin_scale(20.0), 1); // 1.25 rounds down
        assert_eq!(builtin_scale(25.0), 2); // 1.56 rounds up
        assert_eq!(builtin_scale(32.0), 2);
        assert_eq!(builtin_scale(48.0), 3);
    }

    #[test]
    fn a_nonsense_size_still_yields_a_usable_font() {
        // A zero-scale font would draw nothing at all, which reads as a
        // rendering bug rather than as the bad argument it is.
        for px in [0.0, -5.0, f32::NAN, f32::INFINITY] {
            assert_eq!(builtin_scale(px), 1, "px_per_em = {px}");
        }
        let f = SystemFont::builtin(0.0);
        assert!(f.measure("x") > 0.0);
        assert!(f.line_height() > 0.0);
    }

    #[test]
    fn garbage_bytes_fall_back_to_the_builtin_face() {
        let f = SystemFont::or_builtin(alloc::vec![0u8; 64], 16.0);
        assert!(!f.is_scalable());
        assert!(f.as_scaled().is_none());
        // Falling back is only useful if the fallback can actually draw.
        assert!(f.measure("hello") > 0.0);
    }

    #[test]
    fn a_real_face_takes_the_outline_path() {
        let f = SystemFont::from_bytes(build_test_font(), 32.0).expect("fixture must parse");
        assert!(f.is_scalable());
        assert!(f.as_scaled().is_some());
    }

    #[test]
    fn both_backends_put_ink_on_the_baseline_side() {
        // The two backends compute glyph placement completely differently —
        // one from a mask's `top`, the other from `bearing_y` — so the shared
        // promise (y is the baseline, ink sits above it) is worth pinning for
        // both. A sign error here would look fine in isolation and misalign
        // text the moment the two are mixed on one line.
        let (w, h) = (160_u32, 64_u32);
        for mut font in [
            SystemFont::builtin(16.0),
            SystemFont::from_bytes(build_test_font(), 32.0).expect("fixture must parse"),
        ] {
            let mut buf = surface(w, h);
            let mut target = Target {
                buffer: &mut buf,
                stride: w,
                height: h,
                color: 0xFFFF_FFFF,
            };
            let baseline = 48.0_f32;
            let end = font.draw_text("AB", &mut target, 4.0, baseline);
            assert!(end > 4.0, "pen must advance");

            let lit: Vec<u32> = (0..h)
                .filter(|&row| (0..w).any(|col| buf[(row * w + col) as usize] & 0x00FF_FFFF != 0))
                .collect();
            assert!(!lit.is_empty(), "nothing was drawn");
            let lowest = *lit.iter().max().unwrap();
            assert!(
                lowest <= baseline as u32,
                "ink at row {lowest} is below the baseline at {baseline}"
            );
        }
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_never_inside_a_word() {
        let font = SystemFont::builtin(16.0);
        let width = font.measure("aaaa bbbb");
        let lines = font.wrap("aaaa bbbb cccc dddd", width);
        assert_eq!(lines, alloc::vec!["aaaa bbbb", "cccc dddd"]);

        // A word wider than the whole line stays whole on its own line.
        let lines = font.wrap("aa supercalifragilistic bb", font.measure("aaaaa"));
        assert_eq!(lines, alloc::vec!["aa", "supercalifragilistic", "bb"]);
    }

    #[test]
    fn drawing_off_surface_is_clipped_not_a_panic() {
        // The compositor scrolls text out of view constantly; a coordinate
        // far outside the buffer must clip rather than wrap into it.
        let (w, h) = (32_u32, 16_u32);
        let mut font = SystemFont::builtin(16.0);
        let mut buf = surface(w, h);
        let mut target = Target {
            buffer: &mut buf,
            stride: w,
            height: h,
            color: 0xFFFF_FFFF,
        };
        for (x, y) in [
            (-1e9_f32, 8.0_f32),
            (1e9, 8.0),
            (0.0, -1e9),
            (0.0, 1e9),
            (f32::NAN, f32::NAN),
        ] {
            font.draw_text("clip", &mut target, x, y);
        }
        assert!(
            buf.iter().all(|p| p & 0x00FF_FFFF == 0),
            "off-surface text leaked into the buffer"
        );
    }
}
