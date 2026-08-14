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
//! [`FontCache`] is where that choice is actually made for a UI: it serves the
//! built-in face until someone hands it a real one with
//! [`FontCache::set_face`], which is what lets text appear during early boot
//! and improve later, without the drawing code knowing that happened.
//!
//! # Coordinates
//!
//! Pixels, y down, and `y` in [`SystemFont::draw_text`] is the **baseline** —
//! matching [`ScaledFont`]. The bitmap backend converts internally: its glyphs
//! record `bearing_y`, the distance from the baseline up to the top of the
//! cell.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::raster::GlyphMask;
use crate::scaled::{ScaledFont, ScaledFontError, Target, blit_mask, pixel_coord};
use crate::sfnt::{Face, SfntError};
use crate::shape::{GlyphKey, ShapedGlyph, ShapedRun, TAB_WIDTH_IN_SPACES};
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
    Bitmap {
        font: Font,
        /// Widened 1-bit glyphs, so a run of text does not re-expand the same
        /// letter once per occurrence. Unbounded on purpose: the built-in face
        /// has a few hundred glyphs and no more, so this cannot grow the way
        /// [`ScaledFont`]'s cache — fed by arbitrary faces — can.
        masks: BTreeMap<char, GlyphMask>,
    },
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
        Self::from_bitmap(Font::scaled(&Font::system_mono(), builtin_scale(px_per_em)))
    }

    /// The built-in bold bitmap face at the integer scale closest to
    /// `px_per_em`.
    #[must_use]
    pub fn builtin_bold(px_per_em: f32) -> Self {
        Self::from_bitmap(Font::scaled(
            &Font::system_mono_bold(),
            builtin_scale(px_per_em),
        ))
    }

    fn from_bitmap(font: Font) -> Self {
        Self {
            backend: Backend::Bitmap {
                font,
                masks: BTreeMap::new(),
            },
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
    /// The fallback is silent by design: a font file that is truncated, is a
    /// format this crate does not read, or is simply not a font is a
    /// *configuration* problem, and the user is better served by ugly text than
    /// by a blank screen. Callers that want to report the failure should use
    /// [`SystemFont::from_bytes`] and decide for themselves.
    #[must_use]
    pub fn or_builtin(data: Vec<u8>, px_per_em: f32) -> Self {
        Self::from_bytes(data, px_per_em).unwrap_or_else(|_| Self::builtin(px_per_em))
    }

    /// Pins an already-parsed, shared face to a size.
    ///
    /// Parsing a font file is the expensive part and its result is immutable,
    /// so a UI drawing one family at four sizes should parse once. Only the
    /// rasterized glyphs are per-size, and those belong to the returned font.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::InvalidSize`] if `px_per_em` is not finite and
    /// positive.
    pub fn from_shared(face: Arc<Face>, px_per_em: f32) -> Result<Self, ScaledFontError> {
        Ok(Self {
            backend: Backend::Outline(ScaledFont::shared(face, px_per_em)?),
        })
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
            Backend::Bitmap { font, .. } => font.metrics(),
        }
    }

    /// Baseline-to-baseline distance in pixels.
    #[must_use]
    pub fn line_height(&self) -> f32 {
        self.metrics().line_height
    }

    /// The glyphs `text` turns into, with final advances.
    ///
    /// The single entry point for anything that walks text: measuring,
    /// drawing, hit-testing and truncating all read the same run, so they
    /// cannot disagree about where a character sits. See
    /// [`shape`](crate::shape).
    #[must_use]
    pub fn shape(&self, text: &str) -> ShapedRun {
        match &self.backend {
            Backend::Outline(f) => f.shape(text),
            // The bitmap face is a fixed grid: one glyph per character, no
            // kerning to apply and no substitutions to make. It still shapes
            // rather than being special-cased at every call site, so that the
            // callers stay backend-agnostic.
            Backend::Bitmap { font, .. } => ShapedRun::new(
                text.char_indices()
                    .map(|(cluster, ch)| {
                        // Same tab rule as the outline path, for the same
                        // reason: the built-in face has no tab glyph either,
                        // and its measurement already widened one to four
                        // cells while its drawing did not.
                        let (key, advance) = if ch == '\t' {
                            (
                                GlyphKey::bitmap(' '),
                                font.glyph(' ').advance * TAB_WIDTH_IN_SPACES,
                            )
                        } else {
                            (GlyphKey::bitmap(ch), font.glyph(ch).advance)
                        };
                        ShapedGlyph {
                            key,
                            cluster,
                            advance,
                            // A fixed grid has no pair corrections to make,
                            // and no anchors to attach a combining mark with.
                            kern_next: 0.0,
                            offset: (0.0, 0.0),
                        }
                    })
                    .collect(),
            ),
        }
    }

    /// Width of `text` in pixels, ignoring line breaks.
    #[must_use]
    pub fn measure(&self, text: &str) -> f32 {
        self.shape(text).width()
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
            Backend::Bitmap { .. } => wrap_with(text, max_width, &|s| self.measure(s)),
        }
    }

    /// Draws `text` with its baseline at `y`, starting at pen position `x`.
    ///
    /// Returns the pen position after the last glyph, so runs can be chained
    /// without re-measuring.
    pub fn draw_text(&mut self, text: &str, target: &mut Target<'_>, x: f32, y: f32) -> f32 {
        let mut pen = x;
        // Shaped into a local because the loop needs `&mut self` to rasterize
        // while it walks the run.
        let run = self.shape(text);
        for shaped in run.glyphs() {
            let advance = shaped.advance;
            let Some(mask) = self.glyph_mask(shaped.key) else {
                pen += advance;
                continue;
            };
            // A mask's left/top are small integers bounded by the glyph size;
            // the pen and baseline are caller-supplied and may be anything,
            // which is why the sum goes through `pixel_coord` (which rejects
            // the degenerate cases) and then `blit_mask` (which clips).
            // `offset` is zero except on an attached combining mark, and its
            // `y` points up where the screen's points down.
            #[allow(clippy::cast_precision_loss)]
            let placed = (
                pixel_coord(pen + shaped.offset.0 + mask.left as f32),
                pixel_coord(y - shaped.offset.1 + mask.top as f32),
            );
            if let (Some(gx), Some(gy)) = placed {
                blit_mask(mask, target, gx, gy);
            }
            pen += advance;
        }
        pen
    }

    /// The coverage mask for a shaped glyph, rasterizing and caching it if
    /// this is the first time it has been asked for.
    ///
    /// [`draw_text`](Self::draw_text) covers the common case, but it can only
    /// draw into a [`Target`] — a flat `[u32]` surface with one colour and no
    /// clipping beyond its own bounds. The compositor blends every pixel
    /// through a clip stack and a per-window opacity, so it needs the
    /// coverage values themselves rather than a finished blit. Handing out
    /// the mask keeps that policy where it belongs instead of growing
    /// `Target` a field at a time until it is a compositor.
    ///
    /// The advance is *not* returned with it: it belongs to the
    /// [`ShapedGlyph`] the key came from, because it depends on the glyph's
    /// neighbours. A caller that took it from here would be back to spacing
    /// text differently from the way it was measured.
    ///
    /// Returns `None` only when an outline glyph fails to rasterize; the
    /// bitmap backend always answers, with tofu if it must.
    ///
    /// The mask is positioned by its `left`/`top`, measured from the pen
    /// position on the baseline with y growing downward — identical for both
    /// backends, so a caller never learns which one it has.
    pub fn glyph_mask(&mut self, key: GlyphKey) -> Option<&GlyphMask> {
        match &mut self.backend {
            Backend::Outline(f) => Some(&f.glyph(key.gid()).ok()?.mask),
            Backend::Bitmap { font, masks } => {
                let ch = key.ch();
                let glyph = font.glyph(ch);
                // `font` and `masks` are disjoint fields, so the closure may
                // read the face while the entry holds the cache.
                Some(masks.entry(ch).or_insert_with(|| mask_from_bitmap(glyph)))
            }
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
            Backend::Bitmap { .. } => None,
        }
    }
}

/// How heavy a face to draw with.
///
/// Deliberately two values rather than a numeric weight axis: the built-in
/// face has exactly two, and a `u16` weight would promise a range nothing can
/// currently deliver. Widening this when variable fonts arrive is a smaller
/// change than un-promising a range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weight {
    #[default]
    Regular,
    Bold,
}

/// The fonts a UI draws with, one per distinct size and weight.
///
/// Exists because measuring and drawing must agree. A widget asks "how wide is
/// this label" while laying out, and the compositor asks "what glyphs does this
/// label have" while drawing; if those two round the requested size
/// differently — or use different faces — labels overflow their buttons and
/// text cursors land between characters. Sharing one cache makes that class of
/// bug unrepresentable rather than merely unlikely.
///
/// Caching matters because a [`SystemFont`] owns rasterized glyphs: rebuilding
/// one per label would re-rasterize the alphabet on every frame. The map stays
/// small on its own, being keyed by the sizes a UI actually asks for, and a UI
/// has a handful of text sizes rather than a continuum of them.
/// A cache with no faces installed serves the built-in bitmap font, which is
/// why this is useful before there is a filesystem to load a real one from —
/// and why a UI that never calls [`FontCache::set_face`] still draws.
#[derive(Debug, Default)]
pub struct FontCache {
    fonts: BTreeMap<(u32, Weight), SystemFont>,
    /// The installed face per weight, if any. Shared rather than owned per
    /// size: a UI asks for the same family at a handful of sizes, and a face
    /// is the whole font file.
    faces: BTreeMap<Weight, Arc<Face>>,
}

impl FontCache {
    /// An empty cache, serving the built-in bitmap face.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw `weight` with `face` from now on.
    ///
    /// Every font already built at this weight is dropped, because a cached
    /// `SystemFont` holds glyphs rasterized from the *previous* face: keeping
    /// them would make the size a UI happened to ask for first decide which
    /// face it gets. The glyphs are rebuilt lazily on the next `get`.
    ///
    /// Installing only a regular face is normal and supported — bold text then
    /// falls back to the built-in bold bitmap face rather than to a synthesised
    /// emboldening of the real one, which is the honest answer until there is a
    /// bold face to use.
    pub fn set_face(&mut self, weight: Weight, face: Arc<Face>) {
        self.faces.insert(weight, face);
        self.fonts.retain(|(_, w), _| *w != weight);
    }

    /// Parse `data` and install it for `weight`.
    ///
    /// The convenience form of [`FontCache::set_face`] for a caller that has
    /// just read a file and has no other use for the parsed face.
    ///
    /// # Errors
    ///
    /// Whatever [`Face::parse`] rejects the file with. The cache is left
    /// untouched on failure, so a bad file downgrades to the built-in face
    /// rather than leaving the UI without one.
    pub fn install_face(&mut self, weight: Weight, data: Vec<u8>) -> Result<(), SfntError> {
        self.set_face(weight, Arc::new(Face::parse(data)?));
        Ok(())
    }

    /// Whether a real face is installed for `weight`.
    #[must_use]
    pub fn has_face(&self, weight: Weight) -> bool {
        self.faces.contains_key(&weight)
    }

    /// The font for `px` and `weight`, building it on first use.
    pub fn get(&mut self, px: f32, weight: Weight) -> &mut SystemFont {
        let face = self.faces.get(&weight).map(Arc::clone);
        self.fonts.entry((round_px(px), weight)).or_insert_with(|| {
            // An installed face that will not scale to this size is a
            // per-size failure, not a reason to stop using the face: fall
            // back for this entry and leave the face installed.
            face.and_then(|f| SystemFont::from_shared(f, px).ok())
                .unwrap_or_else(|| match weight {
                    Weight::Regular => SystemFont::builtin(px),
                    Weight::Bold => SystemFont::builtin_bold(px),
                })
        })
    }

    /// How many distinct fonts have been built.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    /// Whether no font has been built yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }
}

/// Normalises a requested pixel size to a cache key.
///
/// Rounded to a whole pixel so 13.9 and 14.0 share one entry rather than
/// rasterizing the alphabet twice for a difference no one can see. Clamped
/// because the size may have crossed a process boundary: a non-finite or
/// absurd request must yield a readable font rather than a panic or a
/// gigabyte of coverage.
fn round_px(px: f32) -> u32 {
    if !px.is_finite() {
        return 16;
    }
    // The clamp keeps the cast in range whatever the caller asked for.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rounded = px.clamp(1.0, 512.0).round() as u32;
    rounded
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

/// Widens a 1-bit glyph into the same 8-bit coverage the outline path
/// produces, so both backends share one blitter and one placement rule.
///
/// A bitmap pixel is simply fully covered or not covered at all, so this
/// trades memory for uniformity: 8x the bytes of the packed form, in exchange
/// for callers never having to ask which backend they hold.
fn mask_from_bitmap(glyph: &GlyphBitmap) -> GlyphMask {
    let mut coverage =
        Vec::with_capacity((glyph.width as usize).saturating_mul(glyph.height as usize));
    for y in 0..glyph.height {
        for x in 0..glyph.width {
            coverage.push(if glyph.pixel_at(x, y) { 255 } else { 0 });
        }
    }
    // `bearing_y` measures upward from the baseline to the top of the cell,
    // while `top` measures downward from it, so the sign flips. Negating in
    // here rather than at each draw site is what lets the outline and bitmap
    // masks be placed by identical arithmetic.
    // Saturating because `as` maps a NaN or absurd bearing to a saturated
    // `i32`, and negating `i32::MIN` would overflow.
    #[allow(clippy::cast_possible_truncation)]
    let top = (glyph.bearing_y as i32).saturating_neg();
    GlyphMask {
        width: glyph.width,
        height: glyph.height,
        // The built-in face is monospace with no side bearing: ink starts at
        // the pen.
        left: 0,
        top,
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
    fn both_backends_report_a_glyph_the_same_way() {
        // The whole point of the facade: a caller that pulls masks out itself
        // — the compositor does, because it blends through a clip stack —
        // must not be able to tell which backend answered.
        let outline = SystemFont::from_bytes(build_test_font(), 24.0).unwrap();
        for mut font in [SystemFont::builtin(16.0), outline] {
            let which = if font.is_scalable() {
                "outline"
            } else {
                "bitmap"
            };
            let run = font.shape("A");
            assert_eq!(run.len(), 1, "{which}: one letter, one glyph");
            let shaped = run.glyphs()[0];
            assert!(shaped.advance > 0.0, "{which}: pen must advance");
            assert_eq!(shaped.cluster, 0, "{which}: the glyph came from byte 0");
            let mask = font.glyph_mask(shaped.key).expect("'A' must render");
            assert!(mask.width > 0 && mask.height > 0, "{which}: empty mask");
            assert_eq!(
                mask.coverage.len(),
                (mask.width * mask.height) as usize,
                "{which}: coverage is not width*height"
            );
            assert!(
                mask.top < 0,
                "{which}: a capital A must sit above the baseline, got top {}",
                mask.top
            );
        }
    }

    #[test]
    fn a_repeated_letter_is_only_rasterized_once() {
        // Text redraws the same few dozen letters every frame; expanding each
        // one again per occurrence would put an allocation on that path.
        let mut font = SystemFont::builtin(16.0);
        let key = font.shape("e").glyphs()[0].key;
        let first = font.glyph_mask(key).unwrap().clone();
        let second = font.glyph_mask(key).unwrap();
        assert_eq!(&first, second, "cached mask differs from the first one");
        let Backend::Bitmap { masks, .. } = &font.backend else {
            panic!("builtin() must take the bitmap path");
        };
        assert_eq!(masks.len(), 1, "one distinct letter, one cache entry");
    }

    #[test]
    fn the_cache_keys_on_rounded_size_and_weight() {
        let mut cache = FontCache::new();
        assert!(cache.is_empty());
        cache.get(16.0, Weight::Regular);
        cache.get(16.4, Weight::Regular); // rounds to the same key
        assert_eq!(cache.len(), 1, "16.0 and 16.4 must share a font");
        cache.get(16.0, Weight::Bold);
        cache.get(32.0, Weight::Regular);
        assert_eq!(cache.len(), 3, "size and weight each key the cache");
    }

    #[test]
    fn an_installed_face_is_what_the_cache_serves() {
        // The gap this closes: before `set_face` existed, `get` could only ever
        // build the built-in bitmap font, so the entire outline pipeline was
        // unreachable from the compositor and the toolkit — the two callers
        // that hold a `FontCache`.
        let mut cache = FontCache::new();
        assert!(!cache.has_face(Weight::Regular));
        assert!(
            !cache.get(16.0, Weight::Regular).is_scalable(),
            "an empty cache must serve the built-in face"
        );

        cache.install_face(Weight::Regular, build_test_font()).unwrap();
        assert!(cache.has_face(Weight::Regular));
        assert!(
            cache.get(16.0, Weight::Regular).is_scalable(),
            "installing a face must replace the font already built at that size"
        );
        assert!(
            !cache.get(16.0, Weight::Bold).is_scalable(),
            "a weight with no face installed must still fall back"
        );
    }

    #[test]
    fn every_size_of_one_family_shares_a_single_parsed_face() {
        // A `Face` owns the whole font file. Parsing one per size would hold a
        // copy of a megabyte-scale file per size and re-parse the tables to get
        // it, which is what `Arc` in the cache is for.
        let mut cache = FontCache::new();
        cache.install_face(Weight::Regular, build_test_font()).unwrap();
        let mut faces = Vec::new();
        for px in [11.0, 16.0, 24.0, 48.0] {
            let scaled = cache
                .get(px, Weight::Regular)
                .as_scaled()
                .expect("installed face must scale")
                .shared_face();
            faces.push(scaled);
        }
        assert_eq!(cache.len(), 4, "four sizes, four fonts");
        for f in &faces[1..] {
            assert!(
                Arc::ptr_eq(&faces[0], f),
                "each size parsed its own copy of the face"
            );
        }
    }

    #[test]
    fn a_face_the_cache_cannot_parse_leaves_it_usable() {
        // A font path is configuration, and configuration is wrong sometimes.
        // The failure must not cost the UI its text.
        let mut cache = FontCache::new();
        assert!(
            cache
                .install_face(Weight::Regular, alloc::vec![0u8; 32])
                .is_err()
        );
        assert!(!cache.has_face(Weight::Regular));
        assert!(cache.get(16.0, Weight::Regular).measure("x") > 0.0);
    }

    #[test]
    fn a_hostile_size_still_yields_a_readable_font() {
        // A size may have crossed a process boundary, so it is not necessarily
        // a number at all -- and a 1e30-pixel face would be a memory bomb.
        let mut cache = FontCache::new();
        for px in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 1e30] {
            let font = cache.get(px, Weight::Regular);
            assert!(font.line_height() > 0.0, "px = {px} gave an unusable font");
            assert!(font.measure("x") > 0.0, "px = {px} measures to nothing");
        }
    }

    #[test]
    fn measuring_agrees_with_drawing() {
        // The reason the cache exists: a widget measures a label to size its
        // button, and the compositor draws it. If those disagree the text
        // overflows. `measure` must equal the pen distance `draw_text` covers.
        let mut cache = FontCache::new();
        for (px, weight) in [(16.0, Weight::Regular), (48.0, Weight::Bold)] {
            let font = cache.get(px, weight);
            let expected = font.measure("Hamburgefonstiv");
            let mut buf = surface(4096, 128);
            let mut target = Target {
                buffer: &mut buf,
                stride: 4096,
                height: 128,
                color: 0xFFFF_FFFF,
            };
            let end = font.draw_text("Hamburgefonstiv", &mut target, 0.0, 64.0);
            assert_eq!(end, expected, "{px}px {weight:?}: measure != pen advance");
        }
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
