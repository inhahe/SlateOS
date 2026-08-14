//! A font face pinned to one pixel size, with a glyph cache.
//!
//! [`sfnt::Face`] answers questions in *font units* and [`raster`] turns one
//! outline into one mask. Neither is what a caller wants: a toolkit wants to
//! say "draw this string at 13 px" and have it happen, without re-flattening
//! the same 'e' for every word on the screen. [`ScaledFont`] is that layer.
//!
//! # Why the cache is behind `&mut self`
//!
//! Drawing mutates the cache, and the signature says so. The obvious
//! alternative — `&self` plus a `RefCell`/`Mutex` inside — buys the ability
//! to share one `ScaledFont` immutably at the cost of either giving up `Sync`
//! or paying for a lock on every glyph. Callers that genuinely share a font
//! across threads already have a lock around their draw state; callers that
//! don't (the common case: one cache per rendering thread) should not pay for
//! one. So the type stays a plain owned value and the borrow checker enforces
//! the exclusion.
//!
//! # Coordinates
//!
//! Everything here is in pixels with y increasing downward, matching the
//! framebuffer. A glyph's `top` is relative to the baseline, so it is
//! negative for the part of a glyph above the baseline — the usual
//! convention, and the one [`raster::GlyphMask`] already uses.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::FontMetrics;
use crate::raster::{GlyphMask, rasterize};
use crate::sfnt::{Face, PathCmd, SfntError};

/// How many rasterized glyphs one [`ScaledFont`] keeps before it starts
/// evicting.
///
/// A screenful of Latin text touches on the order of 100 distinct glyphs;
/// 512 leaves room for punctuation, accents and a little CJK without letting
/// a hostile string (say, a scroll through every codepoint in a CJK face)
/// grow the cache without bound. At 13 px a cached mask is a couple of
/// hundred bytes, so the ceiling is well under a megabyte.
pub const GLYPH_CACHE_LIMIT: usize = 512;

/// Why a [`ScaledFont`] could not be built or could not draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaledFontError {
    /// The underlying face could not be read.
    Sfnt(SfntError),
    /// The requested pixel size is zero, negative, or not a number.
    InvalidSize,
}

impl From<SfntError> for ScaledFontError {
    fn from(e: SfntError) -> Self {
        Self::Sfnt(e)
    }
}

impl core::fmt::Display for ScaledFontError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Sfnt(e) => write!(f, "{e}"),
            Self::InvalidSize => f.write_str("pixel size must be finite and positive"),
        }
    }
}

/// One glyph, rasterized and ready to blit.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// Anti-aliased coverage and its offset from the pen position.
    pub mask: GlyphMask,
    /// How far the pen advances after drawing this glyph, in pixels.
    pub advance: f32,
}

/// A face pinned to a pixel size, caching the glyphs it has drawn.
pub struct ScaledFont {
    face: Face,
    px_per_em: f32,
    scale: f32,
    metrics: FontMetrics,
    /// Keyed by glyph id, not character: two characters that map to the same
    /// glyph (and there are many — the space-like codepoints, the various
    /// hyphens in some faces) must not each get their own entry.
    cache: BTreeMap<u16, Glyph>,
    /// Insertion order, for eviction. A true LRU would need to reorder on
    /// every hit; text rendering hits the same small working set over and
    /// over, so first-in-first-out evicts almost the same entries for none of
    /// the bookkeeping.
    order: Vec<u16>,
}

impl core::fmt::Debug for ScaledFont {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ScaledFont")
            .field("px_per_em", &self.px_per_em)
            .field("units_per_em", &self.face.units_per_em())
            .field("num_glyphs", &self.face.num_glyphs())
            .field("cached", &self.cache.len())
            // The face bytes and the masks are megabytes; naming them here
            // would make `{:?}` on a font unusable.
            .finish_non_exhaustive()
    }
}

impl ScaledFont {
    /// Pin `face` to `px_per_em` pixels per em.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::InvalidSize`] if `px_per_em` is not finite and
    /// positive.
    pub fn new(face: Face, px_per_em: f32) -> Result<Self, ScaledFontError> {
        if !px_per_em.is_finite() || px_per_em <= 0.0 {
            return Err(ScaledFontError::InvalidSize);
        }
        let scale = face.scale_for_px(px_per_em);
        let metrics = Self::derive_metrics(&face, scale);
        Ok(Self {
            face,
            px_per_em,
            scale,
            metrics,
            cache: BTreeMap::new(),
            order: Vec::new(),
        })
    }

    /// Read a font file and pin it to a size in one step.
    ///
    /// # Errors
    ///
    /// Whatever [`Face::parse`] rejects the file with, or
    /// [`ScaledFontError::InvalidSize`].
    pub fn from_bytes(data: Vec<u8>, px_per_em: f32) -> Result<Self, ScaledFontError> {
        Self::new(Face::parse(data)?, px_per_em)
    }

    /// Translate the face's font-unit metrics into pixels.
    ///
    /// `cap_height` and `x_height` are measured from the outlines of 'H' and
    /// 'x' rather than read from `OS/2`, because `OS/2` is optional, is
    /// frequently absent from older and from bare `glyf` faces, and carries
    /// zeros in those fields in plenty of the faces that do have it. The
    /// outline is the ground truth and we already have a parser for it.
    fn derive_metrics(face: &Face, scale: f32) -> FontMetrics {
        let fm = face.metrics();
        let ascent = f32::from(fm.ascender) * scale;
        // `descender` is negative per spec; `FontMetrics::descent` is
        // positive-downward, so flip it.
        let descent = -f32::from(fm.descender) * scale;
        #[allow(clippy::cast_precision_loss)]
        // line_height() is a sum of three i16s, far inside f32's exact range.
        let line_height = fm.line_height() as f32 * scale;
        let max_advance = f32::from(fm.advance_width_max) * scale;

        let cap_height = Self::glyph_top(face, 'H', scale).unwrap_or(ascent * 0.7);
        let x_height = Self::glyph_top(face, 'x', scale).unwrap_or(ascent * 0.5);
        let average_advance = Self::average_advance(face, scale);

        FontMetrics {
            ascent,
            descent,
            line_height,
            max_advance,
            average_advance,
            cap_height,
            x_height,
        }
    }

    /// Height of `ch`'s outline above the baseline, in pixels.
    ///
    /// `None` when the face has no such glyph or the glyph is blank, so the
    /// caller can fall back rather than record a height of zero.
    fn glyph_top(face: &Face, ch: char, scale: f32) -> Option<f32> {
        let gid = face.glyph_index(ch)?;
        let outline = face.outline(gid).ok()?;
        let mut top = f32::NEG_INFINITY;
        let mut note = |y: f32| {
            if y.is_finite() {
                top = top.max(y);
            }
        };
        for cmd in &outline.commands {
            // A quadratic's control point is an upper bound on the curve,
            // never a point on it, so only its endpoint counts — which is
            // why every arm below looks at the endpoint and nothing else.
            match *cmd {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) | PathCmd::QuadTo(_, p) => note(p.y),
                PathCmd::Close => {}
            }
        }
        (top > 0.0).then_some(top * scale)
    }

    /// Mean advance across the characters ordinary text is mostly made of.
    ///
    /// Averaging over *every* glyph would be dominated by whatever exotica
    /// the face happens to include; averaging over lowercase Latin plus space
    /// is what a caller asking "how wide is a character, roughly" means.
    fn average_advance(face: &Face, scale: f32) -> f32 {
        let mut total = 0.0_f32;
        let mut count = 0.0_f32;
        for ch in "abcdefghijklmnopqrstuvwxyz ".chars() {
            let Some(gid) = face.glyph_index(ch) else {
                continue;
            };
            let Ok(adv) = face.advance(gid) else { continue };
            total += f32::from(adv);
            count += 1.0;
        }
        if count == 0.0 {
            // A face with no Latin lowercase at all — a CJK or symbol face.
            // Its declared maximum is a better guess than zero.
            return f32::from(face.metrics().advance_width_max) * scale;
        }
        total / count * scale
    }

    /// The size this font was pinned to.
    #[must_use]
    pub fn px_per_em(&self) -> f32 {
        self.px_per_em
    }

    /// Pixel-space metrics for the face.
    #[must_use]
    pub fn metrics(&self) -> &FontMetrics {
        &self.metrics
    }

    /// The underlying face, for callers that need font-unit data.
    #[must_use]
    pub fn face(&self) -> &Face {
        &self.face
    }

    /// How many glyphs are currently rasterized and held.
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        self.cache.len()
    }

    /// Drop every cached glyph. Useful when memory is tight.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    /// The glyph id `ch` maps to, or glyph 0 (`.notdef`) if the face has no
    /// mapping for it.
    ///
    /// Substituting `.notdef` rather than returning `None` is deliberate:
    /// `.notdef` is the empty box every font draws for "I don't have this",
    /// which is exactly what should appear on screen, and it keeps the
    /// advance non-zero so the rest of the line does not collapse onto it.
    #[must_use]
    pub fn glyph_id(&self, ch: char) -> u16 {
        self.face.glyph_index(ch).unwrap_or(0)
    }

    /// Rasterize `gid` at this font's size, or return the cached result.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::Sfnt`] if the glyph cannot be decoded. A glyph that
    /// decodes but rasterizes to nothing (a space, a zero-area contour) is
    /// not an error — it yields an empty mask.
    pub fn glyph(&mut self, gid: u16) -> Result<&Glyph, ScaledFontError> {
        if !self.cache.contains_key(&gid) {
            let entry = self.rasterize_glyph(gid)?;
            self.insert(gid, entry);
        }
        self.cache
            .get(&gid)
            .ok_or(ScaledFontError::Sfnt(SfntError::GlyphOutOfRange))
    }

    fn rasterize_glyph(&self, gid: u16) -> Result<Glyph, ScaledFontError> {
        let outline = self.face.outline(gid)?;
        let advance = f32::from(self.face.advance(gid)?) * self.scale;
        // Every rasterizer failure is swallowed, and deliberately: a glyph
        // whose outline is absurd or malformed still has to occupy its
        // advance, or every following glyph on the line shifts left. Draw
        // nothing, keep the space — exactly what a blank glyph does.
        // (`InvalidScale` cannot occur at all here; `new` validated it.)
        let mask = rasterize(&outline, self.scale).unwrap_or_default();
        Ok(Glyph { mask, advance })
    }

    fn insert(&mut self, gid: u16, glyph: Glyph) {
        if self.cache.len() >= GLYPH_CACHE_LIMIT {
            // Evict the oldest still-present entry. The loop (rather than a
            // single `remove(0)`) skips ids already gone via `clear_cache`.
            while !self.order.is_empty() {
                let victim = self.order.remove(0);
                if self.cache.remove(&victim).is_some() {
                    break;
                }
            }
        }
        self.cache.insert(gid, glyph);
        self.order.push(gid);
    }

    /// Width of `text` in pixels, ignoring line breaks.
    ///
    /// This only needs `hmtx`, so it does not rasterize anything and does not
    /// touch the cache.
    #[must_use]
    pub fn measure(&self, text: &str) -> f32 {
        let mut w = 0.0_f32;
        for ch in text.chars() {
            let gid = self.glyph_id(ch);
            if let Ok(adv) = self.face.advance(gid) {
                w += f32::from(adv) * self.scale;
            }
        }
        w
    }

    /// Break `text` into lines no wider than `max_width`, at whitespace.
    ///
    /// A single word longer than `max_width` is left on its own over-long
    /// line rather than being cut mid-word: breaking inside a word is a
    /// per-script decision (it is wrong for Latin, required for CJK) that
    /// belongs to a real line breaker, not here.
    #[must_use]
    pub fn wrap(&self, text: &str, max_width: f32) -> Vec<String> {
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
                if self.measure(&candidate) <= max_width {
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

    /// Draw `text` with its baseline at `y`, starting at pen position `x`.
    ///
    /// Returns the pen position after the last glyph, so callers can chain
    /// runs (a bold span after a regular one, say) without re-measuring.
    ///
    /// `color` is `0xAARRGGBB`. Each glyph's coverage is multiplied into the
    /// alpha, so a fully opaque colour still anti-aliases.
    pub fn draw_text(&mut self, text: &str, target: &mut Target<'_>, x: f32, y: f32) -> f32 {
        let mut pen = x;
        for ch in text.chars() {
            let gid = self.glyph_id(ch);
            let Ok(glyph) = self.glyph(gid) else {
                continue;
            };
            let advance = glyph.advance;
            // A mask's left/top come from a bitmap bounded by
            // `MAX_GLYPH_PIXELS`, so they are small integers; the pen and
            // baseline are caller-supplied and may be anything, which is why
            // the result is only ever handed to `blit_mask`, which clips.
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let (gx, gy) = (
                (pen + glyph.mask.left as f32) as i32,
                (y + glyph.mask.top as f32) as i32,
            );
            blit_mask(&glyph.mask, target, gx, gy);
            pen += advance;
        }
        pen
    }
}

/// An ARGB surface to draw into, plus the colour to draw with.
///
/// Bundled into one struct because the alternative is a seven-argument
/// function that is easy to call wrongly — `stride` and `height` are both
/// `u32` and transposing them silently corrupts memory bounds.
pub struct Target<'a> {
    /// The pixel buffer, `0xAARRGGBB` per pixel, row-major.
    pub buffer: &'a mut [u32],
    /// Pixels per row. May exceed the visible width.
    pub stride: u32,
    /// Rows in the buffer.
    pub height: u32,
    /// Foreground colour, `0xAARRGGBB`.
    pub color: u32,
}

/// A colour split into 8-bit channels, so the blend loop does not have to
/// re-extract them for every pixel.
#[derive(Clone, Copy)]
struct Channels {
    red: u32,
    green: u32,
    blue: u32,
}

impl Channels {
    fn from_argb(argb: u32) -> Self {
        Self {
            red: (argb >> 16) & 0xFF,
            green: (argb >> 8) & 0xFF,
            blue: argb & 0xFF,
        }
    }
}

/// Blend one 8-bit channel: `src` over `dst` at `alpha`/255.
///
/// Every input is masked to 8 bits by its caller, so the weighted sum
/// `src * alpha + dst * (255 - alpha)` peaks at `65_025` — far inside a
/// `u32`, so it cannot overflow. The saturating
/// forms state that invariant in the code rather than leaving the reader to
/// re-derive it.
fn blend_channel(src: u32, dst: u32, alpha: u32) -> u32 {
    let inv = 255_u32.saturating_sub(alpha);
    src.saturating_mul(alpha)
        .saturating_add(dst.saturating_mul(inv))
        / 255
}

/// Blend one anti-aliased mask onto a surface at `(x, y)`.
///
/// `x` and `y` are the mask's top-left corner in surface coordinates and may
/// be negative; anything outside the surface is clipped.
pub fn blit_mask(mask: &GlyphMask, target: &mut Target<'_>, x: i32, y: i32) {
    let src_alpha = (target.color >> 24) & 0xFF;
    if src_alpha == 0 {
        return;
    }
    let src = Channels::from_argb(target.color);
    let max_y = i32::try_from(target.height).unwrap_or(i32::MAX);
    let max_x = i32::try_from(target.stride).unwrap_or(i32::MAX);

    for row in 0..mask.height {
        let Ok(dy) = i32::try_from(row) else { break };
        let py = y.saturating_add(dy);
        if py < 0 || py >= max_y {
            continue;
        }
        for col in 0..mask.width {
            let coverage = u32::from(mask.at(col, row));
            if coverage == 0 {
                continue;
            }
            let Ok(dx) = i32::try_from(col) else { break };
            let px = x.saturating_add(dx);
            if px < 0 || px >= max_x {
                continue;
            }
            // Coverage scales the colour's own alpha: a 50%-covered pixel of
            // a 50%-transparent colour is 25% opaque.
            let alpha = src_alpha.saturating_mul(coverage) / 255;
            if alpha == 0 {
                continue;
            }
            let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) else {
                continue;
            };
            let Some(idx) = py
                .checked_mul(target.stride)
                .and_then(|start| start.checked_add(px))
                .map(|i| i as usize)
            else {
                continue;
            };
            let Some(dest) = target.buffer.get_mut(idx) else {
                continue;
            };
            let under = Channels::from_argb(*dest);
            *dest = 0xFF00_0000
                | (blend_channel(src.red, under.red, alpha) << 16)
                | (blend_channel(src.green, under.green, alpha) << 8)
                | blend_channel(src.blue, under.blue, alpha);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::sfnt::tests::build_test_font;

    fn font(px: f32) -> ScaledFont {
        ScaledFont::from_bytes(build_test_font(), px).unwrap()
    }

    #[test]
    fn rejects_a_nonsense_size() {
        for px in [0.0_f32, -12.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                ScaledFont::from_bytes(build_test_font(), px).err(),
                Some(ScaledFontError::InvalidSize),
                "{px} should be rejected"
            );
        }
    }

    #[test]
    fn metrics_scale_with_size() {
        // The fixture declares ascender 800, descender -200, lineGap 100 on a
        // 1000-unit em, so at 100 px those are exactly 80, 20 and 110.
        let f = font(100.0);
        let m = f.metrics();
        assert!((m.ascent - 80.0).abs() < 0.01, "ascent {}", m.ascent);
        assert!((m.descent - 20.0).abs() < 0.01, "descent {}", m.descent);
        assert!(
            (m.line_height - 110.0).abs() < 0.01,
            "line_height {}",
            m.line_height
        );
        // advanceWidthMax is 600 font units.
        assert!(
            (m.max_advance - 60.0).abs() < 0.01,
            "max_advance {}",
            m.max_advance
        );

        // Half the size, half the metrics.
        let h = font(50.0);
        assert!((h.metrics().ascent - 40.0).abs() < 0.01);
        assert!((h.metrics().line_height - 55.0).abs() < 0.01);
    }

    #[test]
    fn unmapped_characters_become_notdef_not_a_hole() {
        let f = font(32.0);
        // The fixture maps only A, B and C.
        assert_eq!(f.glyph_id('A'), 1);
        assert_eq!(f.glyph_id('\u{4e2d}'), 0);
        assert_eq!(f.glyph_id('z'), 0);
    }

    #[test]
    fn a_glyph_rasterizes_and_is_cached() {
        let mut f = font(64.0);
        assert_eq!(f.cached_glyphs(), 0);
        let gid = f.glyph_id('A');
        let first = f.glyph(gid).unwrap().clone();
        assert_eq!(f.cached_glyphs(), 1);
        assert!(!first.mask.is_empty(), "'A' should produce ink");

        // A second request must not re-rasterize, and must be identical.
        let second = f.glyph(gid).unwrap().clone();
        assert_eq!(f.cached_glyphs(), 1);
        assert_eq!(second.mask.width, first.mask.width);
        assert_eq!(second.mask.coverage, first.mask.coverage);
    }

    #[test]
    fn the_square_glyph_is_the_size_the_font_says_it_is() {
        // Glyph 1 of the fixture is a 100x100 unit square on a 1000-unit em,
        // so at 200 px per em it is exactly 20x20 pixels.
        let mut f = font(200.0);
        let gid = f.glyph_id('A');
        let g = f.glyph(gid).unwrap();
        assert_eq!(
            (g.mask.width, g.mask.height),
            (20, 20),
            "got {}x{}",
            g.mask.width,
            g.mask.height
        );
        // Fully inside, so fully covered.
        assert_eq!(g.mask.at(10, 10), 255);
    }

    #[test]
    fn advance_scales_and_measure_agrees_with_it() {
        let mut f = font(100.0);
        let gid = f.glyph_id('A');
        let adv = f.glyph(gid).unwrap().advance;
        assert!(adv > 0.0, "advance should be positive, got {adv}");
        assert!((f.measure("A") - adv).abs() < 0.01);
        // Three of them is three times as wide.
        assert!((f.measure("AAA") - adv * 3.0).abs() < 0.05);
        assert!((f.measure("") - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn measure_does_not_disturb_the_cache() {
        let f = font(24.0);
        let _ = f.measure("ABCABC");
        assert_eq!(f.cached_glyphs(), 0, "measuring must not rasterize");
    }

    #[test]
    fn the_cache_stops_growing_at_its_limit() {
        let mut f = font(8.0);
        // The fixture has only 4 glyphs, so drive the cache directly to prove
        // the eviction path rather than the glyph count.
        for gid in 0..u16::try_from(GLYPH_CACHE_LIMIT + 10).unwrap() {
            f.insert(
                gid,
                Glyph {
                    mask: GlyphMask::default(),
                    advance: 1.0,
                },
            );
        }
        assert_eq!(f.cached_glyphs(), GLYPH_CACHE_LIMIT);
        // The oldest entries went first.
        let newest = u16::try_from(GLYPH_CACHE_LIMIT + 9).unwrap();
        assert!(!f.cache.contains_key(&0));
        assert!(f.cache.contains_key(&newest));
    }

    #[test]
    fn clearing_the_cache_empties_it() {
        let mut f = font(16.0);
        let gid = f.glyph_id('A');
        let _ = f.glyph(gid).unwrap();
        assert_eq!(f.cached_glyphs(), 1);
        f.clear_cache();
        assert_eq!(f.cached_glyphs(), 0);
        // And it still works afterwards.
        let _ = f.glyph(gid).unwrap();
        assert_eq!(f.cached_glyphs(), 1);
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_keeps_long_words_whole() {
        let f = font(100.0);
        let one = f.measure("A");
        // Room for three glyphs plus their spaces.
        let lines = f.wrap("A A A A A A", one * 5.0);
        assert!(lines.len() > 1, "should have wrapped: {lines:?}");
        for line in &lines {
            assert!(!line.is_empty(), "no empty lines: {lines:?}");
        }
        // Nothing is lost.
        assert_eq!(lines.join(" "), "A A A A A A");

        // A word wider than the limit survives intact on its own line.
        let lines = f.wrap("AAAAAAAA", one * 2.0);
        assert_eq!(lines, ["AAAAAAAA"]);

        // Explicit newlines are honoured.
        let lines = f.wrap("A\nA", one * 100.0);
        assert_eq!(lines, ["A", "A"]);
    }

    #[test]
    fn drawing_puts_ink_in_the_buffer_and_returns_the_pen() {
        let mut f = font(64.0);
        let mut buf = alloc::vec![0xFF00_0000_u32; 128 * 128];
        let mut target = Target {
            buffer: &mut buf,
            stride: 128,
            height: 128,
            color: 0xFFFF_FFFF,
        };
        let end = f.draw_text("A", &mut target, 10.0, 64.0);
        assert!(end > 10.0, "pen should have advanced, ended at {end}");
        assert!((end - (10.0 + f.measure("A"))).abs() < 0.01);

        let lit = buf.iter().filter(|&&p| p != 0xFF00_0000).count();
        assert!(lit > 0, "drawing produced no visible pixels");
    }

    #[test]
    fn drawing_off_surface_clips_instead_of_corrupting() {
        let mut f = font(64.0);
        let mut buf = alloc::vec![0u32; 32 * 32];
        // Far off every edge, in turn. None of these may panic, and the
        // wildly-out-of-range ones must not touch the buffer.
        for (x, y) in [
            (-1000.0_f32, 16.0_f32),
            (1000.0, 16.0),
            (16.0, -1000.0),
            (16.0, 1000.0),
        ] {
            buf.fill(0);
            let mut target = Target {
                buffer: &mut buf,
                stride: 32,
                height: 32,
                color: 0xFFFF_FFFF,
            };
            f.draw_text("A", &mut target, x, y);
            assert!(
                buf.iter().all(|&p| p == 0),
                "({x},{y}) should have been clipped entirely"
            );
        }
    }

    #[test]
    fn a_transparent_colour_draws_nothing() {
        let mut f = font(64.0);
        let mut buf = alloc::vec![0u32; 128 * 128];
        let mut target = Target {
            buffer: &mut buf,
            stride: 128,
            height: 128,
            color: 0x0000_0000,
        };
        f.draw_text("A", &mut target, 10.0, 64.0);
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn coverage_blends_rather_than_replacing() {
        // A half-transparent white over black must land near mid grey, not at
        // either extreme — that is the whole point of the coverage multiply.
        let mask = GlyphMask {
            width: 1,
            height: 1,
            left: 0,
            top: 0,
            coverage: alloc::vec![128],
        };
        let mut buf = alloc::vec![0xFF00_0000_u32; 4];
        let mut target = Target {
            buffer: &mut buf,
            stride: 2,
            height: 2,
            color: 0xFFFF_FFFF,
        };
        blit_mask(&mask, &mut target, 0, 0);
        let grey = buf[0] & 0xFF;
        assert!((100..=160).contains(&grey), "expected mid grey, got {grey}");
    }
}
