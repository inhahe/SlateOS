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

use crate::bidi::{self, Base, Level};
use crate::colr::ColourImage;
use crate::itemize::{self, Faces};
use crate::lang::Lang;
use crate::raster::{GlyphMask, Rendering};
use crate::scaled::{
    ScaledFont, ScaledFontError, Target, blit_image, blit_mask, byte_levels, pixel_coord,
};
use crate::sfnt::{Face, SfntError};
use crate::shape::{GlyphKey, ShapedGlyph, ShapedRun, TAB_WIDTH_IN_SPACES};
use crate::{FONT_HEIGHT, Font, FontMetrics, GlyphBitmap};

/// A font that can draw text, backed by either an outline face or the
/// built-in bitmap face.
///
/// An outline font may also draw from **fallback faces**
/// ([`SystemFont::with_fallbacks`]): the characters its own face has no glyph
/// for -- an emoji in a line of Latin, an Arabic name in an English list --
/// are drawn from the first fallback face that has them, so a line comes out
/// in as many faces as it needs rather than as a row of boxes. Which face
/// draws which part is decided a grapheme cluster at a time
/// ([`itemize`](crate::itemize)), and the parts are shaped each in its own
/// face and joined into one [`ShapedRun`], so measuring, drawing and
/// hit-testing still walk one list.
#[derive(Debug)]
pub struct SystemFont {
    backend: Backend,
    /// The fallback faces, at this font's size, in preference order. Empty
    /// for the bitmap backend, which draws from no face but its own.
    fallbacks: Vec<ScaledFont>,
}

/// The most fallback faces a font draws from: a glyph names its face in one
/// byte, and face 0 is the font's own.
pub const MAX_FALLBACKS: usize = 255;

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
            fallbacks: Vec::new(),
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
            fallbacks: Vec::new(),
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
            fallbacks: Vec::new(),
        })
    }

    /// This font, drawing whatever its own face has no glyph for from
    /// `faces`, in that order: face fallback. Each face is used at this
    /// font's size, moved to `axes` where it has them -- `[(*b"wght",
    /// 700.0)]` for a bold font, so that a variable fallback face is bold too
    /// -- and a face that cannot be used at this size is skipped. At most
    /// [`MAX_FALLBACKS`] are kept.
    ///
    /// The built-in bitmap face takes no fallbacks: it is keyed by character
    /// rather than by glyph, and is what draws before there are any faces to
    /// fall back to. It is returned unchanged.
    #[must_use]
    pub fn with_fallbacks(mut self, faces: &[Arc<Face>], axes: &[([u8; 4], f32)]) -> Self {
        let Backend::Outline(primary) = &self.backend else {
            return self;
        };
        let px = primary.px_per_em();
        self.fallbacks = faces
            .iter()
            .filter_map(|face| {
                let mut font = ScaledFont::shared(Arc::clone(face), px).ok()?;
                font.set_axes(axes);
                Some(font)
            })
            .take(MAX_FALLBACKS)
            .collect();
        self
    }

    /// How many fallback faces this font draws from.
    #[must_use]
    pub fn fallback_count(&self) -> usize {
        self.fallbacks.len()
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
    ///
    /// Names no language, so a face answers with each script's default rules.
    /// [`shape_lang`](Self::shape_lang) is the same call for a caller that
    /// knows one.
    #[must_use]
    pub fn shape(&self, text: &str) -> ShapedRun {
        self.shape_lang(text, None)
    }

    /// The glyphs `text` turns into when it is known to be in `lang`.
    ///
    /// See [`ScaledFont::shape_lang`]. The bitmap fallback is a fixed grid with
    /// no layout tables at all, so for it this is [`shape`](Self::shape): a
    /// face that states no rules has none to vary by language.
    #[must_use]
    pub fn shape_lang(&self, text: &str, lang: Option<Lang>) -> ShapedRun {
        match &self.backend {
            Backend::Outline(f) if self.fallbacks.is_empty() => f.shape_lang(text, lang),
            Backend::Outline(f) => self.shape_across_faces(f, text, lang),
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

    /// [`shape_lang`](Self::shape_lang) for an outline font with fallback
    /// faces: `text` cut into stretches by the face that draws each
    /// ([`itemize`]), each shaped in its face, joined into one run.
    ///
    /// The bidi levels are resolved once, over the whole line, and each
    /// stretch is shaped with its share of them: a stretch shaped alone would
    /// resolve its own, and an Arabic word drawn from a fallback face in the
    /// middle of an English sentence would then decide it was a paragraph of
    /// its own, with its own base direction. The drawing order is worked out
    /// again over the joined run, from those levels, because it is a property
    /// of the line and not of any stretch. Inside one stretch it comes out
    /// exactly as the stretch's own did -- which glyphs a reversal swaps
    /// depends only on the levels between them -- so what each stretch's
    /// shaping did with its own order (kerning charged across a reversal,
    /// marks placed against moving pens) stays right.
    fn shape_across_faces(
        &self,
        primary: &ScaledFont,
        text: &str,
        lang: Option<Lang>,
    ) -> ShapedRun {
        let faces = FaceSet {
            primary,
            fallbacks: &self.fallbacks,
        };
        let stretches = itemize::stretches(text, &faces);
        if let [only] = stretches.as_slice()
            && only.face == 0
        {
            // The whole line in the font's own face: the common case, and
            // exactly what a font with no fallbacks would have shaped.
            return primary.shape_lang(text, lang);
        }
        let levels = byte_levels(text, Base::Auto);
        let mut glyphs: Vec<ShapedGlyph> = Vec::new();
        for stretch in &stretches {
            let Some(part) = text.get(stretch.range.clone()) else {
                continue;
            };
            let share = levels
                .get(stretch.range.clone())
                .map(<[Level]>::to_vec)
                .unwrap_or_default();
            let run = faces.font(stretch.face).shape_leveled(part, lang, share);
            let face = u8::try_from(stretch.face).unwrap_or(0);
            glyphs.extend(run.glyphs().iter().map(|g| ShapedGlyph {
                key: GlyphKey::in_face(face, g.key.gid()),
                cluster: g.cluster.saturating_add(stretch.range.start),
                ..*g
            }));
        }
        let per_glyph: Vec<Level> = if levels.is_empty() {
            Vec::new()
        } else {
            glyphs
                .iter()
                .map(|g| levels.get(g.cluster).copied().unwrap_or(0))
                .collect()
        };
        let visual = if per_glyph.is_empty() {
            Vec::new()
        } else {
            bidi::visual_order(&per_glyph)
                .into_iter()
                .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
                .collect()
        };
        ShapedRun::reordered(glyphs, visual, per_glyph)
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
            Backend::Outline(f) if self.fallbacks.is_empty() => f.wrap(text, max_width),
            // With fallback faces the widths are this font's, not its own
            // face's: measured, like the bitmap face's, by the same rule.
            Backend::Outline(_) => wrap_with(text, max_width, &|s| self.measure(s)),
            // The bitmap face has no wrapper of its own; `ScaledFont`'s rule
            // (break at spaces, never inside a word) is not outline-specific,
            // so it is reimplemented here against `measure` rather than
            // duplicated into the bitmap type.
            Backend::Bitmap { .. } => wrap_with(text, max_width, &|s| self.measure(s)),
        }
    }

    /// [`wrap`](Self::wrap), except that a word wider than `max_width` is
    /// broken by measured fit rather than left on an over-long line. See
    /// [`ScaledFont::wrap_hard`] for when that is the rule to want.
    ///
    /// Unlike [`wrap`](Self::wrap) this needs no per-backend arm: the break
    /// points come from [`ShapedRun::hard_breaks`], and both backends shape.
    #[must_use]
    pub fn wrap_hard(&self, text: &str, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for line in self.wrap(text, max_width) {
            if self.measure(&line) <= max_width {
                lines.push(line);
                continue;
            }
            let cuts = self.shape(&line).hard_breaks(max_width);
            let mut start = 0;
            for cut in cuts {
                if let Some(piece) = line.get(start..cut) {
                    lines.push(piece.to_string());
                    start = cut;
                }
            }
            if let Some(rest) = line.get(start..) {
                lines.push(rest.to_string());
            }
        }
        lines
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
        // Drawing order, not logical order: they differ for a right-to-left
        // run, and it is this loop's accumulating pen that makes the
        // difference visible.
        let drawn: Vec<ShapedGlyph> = run.draw_order().copied().collect();
        for shaped in &drawn {
            let advance = shaped.advance;
            // A colour glyph -- an emoji -- is drawn as a picture, its own
            // colours with the text's alpha as its opacity; see `glyph_image`.
            if let Some(image) = self.glyph_image(shaped.key, target.color) {
                #[allow(clippy::cast_precision_loss)]
                let placed = (
                    pixel_coord(pen + shaped.offset.0 + image.left as f32),
                    pixel_coord(y - shaped.offset.1 + image.top as f32),
                );
                if let (Some(gx), Some(gy)) = placed {
                    blit_image(image, target, gx, gy);
                }
                pen += advance;
                continue;
            }
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
            Backend::Outline(f) => {
                // Which face the glyph is in: 0 for this font's own, and the
                // fallbacks from 1 (see `GlyphKey::in_face`).
                let font = match usize::from(key.face()) {
                    0 => f,
                    face => self.fallbacks.get_mut(face.checked_sub(1)?)?,
                };
                Some(&font.glyph(key.gid()).ok()?.mask)
            }
            Backend::Bitmap { font, masks } => {
                let ch = key.ch();
                let glyph = font.glyph(ch);
                // `font` and `masks` are disjoint fields, so the closure may
                // read the face while the entry holds the cache.
                Some(masks.entry(ch).or_insert_with(|| mask_from_bitmap(glyph)))
            }
        }
    }

    /// The colour image for a shaped glyph, if its face paints it in colour
    /// -- an emoji from a face with a `COLR` table. `None` means the glyph is
    /// drawn from its coverage mask ([`glyph_mask`](Self::glyph_mask)) in the
    /// text colour, as every glyph of an ordinary face, and of the built-in
    /// bitmap face, is.
    ///
    /// `foreground` is the text colour, which a colour glyph may paint parts
    /// of itself in. Only its colour is used, never its alpha: the image is
    /// drawn as for opaque text, and a caller drawing translucent text applies
    /// the alpha to the whole image, as [`blit_image`] does -- a glyph that
    /// painted with a half-transparent text colour and was then drawn half
    /// transparent would come out a quarter opaque where it used it.
    ///
    /// Placed as a mask is, by `left` and `top` from the pen position on the
    /// baseline, y down. The pixels are premultiplied `0xAARRGGBB`.
    pub fn glyph_image(&mut self, key: GlyphKey, foreground: u32) -> Option<&ColourImage> {
        let Backend::Outline(f) = &mut self.backend else {
            return None;
        };
        let font = match usize::from(key.face()) {
            0 => f,
            face => self.fallbacks.get_mut(face.checked_sub(1)?)?,
        };
        font.colour_glyph(key.gid(), foreground | 0xFF00_0000)
    }

    /// Rasterize glyphs `rendering`'s way from now on -- anti-aliased or
    /// not, grey or subpixel -- in this font's own face and every fallback.
    /// The built-in bitmap face has one way to draw and ignores it.
    pub fn set_rendering(&mut self, rendering: Rendering) {
        if let Backend::Outline(f) = &mut self.backend {
            f.set_rendering(rendering);
        }
        for font in &mut self.fallbacks {
            font.set_rendering(rendering);
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

/// An outline font's own face and its fallbacks, as face fallback numbers
/// them: 0 the font's own, then the fallbacks in order.
struct FaceSet<'a> {
    primary: &'a ScaledFont,
    fallbacks: &'a [ScaledFont],
}

impl FaceSet<'_> {
    /// Face `face`, or the font's own for a number it does not have.
    fn font(&self, face: usize) -> &ScaledFont {
        match face.checked_sub(1) {
            Some(i) => self.fallbacks.get(i).unwrap_or(self.primary),
            None => self.primary,
        }
    }
}

impl Faces for FaceSet<'_> {
    fn count(&self) -> usize {
        self.fallbacks.len().saturating_add(1)
    }

    fn has(&self, face: usize, ch: char) -> bool {
        self.font(face)
            .face()
            .glyph_index(ch)
            .is_some_and(|gid| gid != 0)
    }

    fn is_colour(&self, face: usize) -> bool {
        self.font(face).face().has_colour_glyphs()
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

/// Which kind of face a caller is asking for.
///
/// Not a family *name*: the cache holds parsed faces and has no opinion about
/// what they are called. This is the distinction that changes what the caller
/// may assume about the answer — [`Mono`](Family::Mono) promises every glyph
/// has the same advance, and [`Ui`](Family::Ui) promises nothing of the kind.
///
/// It exists because a terminal is a grid: column 40 of row 3 must sit above
/// column 40 of row 4, which is only true if the face is fixed-pitch. Drawing
/// a terminal in the proportional UI face means a `W` overhangs its cell and
/// the block cursor lands beside the character it marks. There is no way to
/// ask for a grid-safe face without saying which kind of face you want, so
/// this is that question.
///
/// When no face is installed for a family the built-in 8x16 bitmap font
/// answers, and that face *is* monospace — so a `Mono` request is grid-safe
/// even on a host with no fixed-pitch font at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    /// The system's UI face: proportional, what labels are drawn in.
    #[default]
    Ui,
    /// A fixed-pitch face, where every glyph advances the same distance.
    Mono,
}

/// The fonts a UI draws with, one per distinct size, weight and family.
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
    fonts: BTreeMap<(u32, Weight, Family), SystemFont>,
    /// The installed face per family and weight, if any. Shared rather than
    /// owned per size: a UI asks for the same family at a handful of sizes,
    /// and a face is the whole font file.
    faces: BTreeMap<(Family, Weight), Arc<Face>>,
    /// The faces every installed face falls back to, in order. See
    /// [`FontCache::set_fallbacks`].
    fallbacks: Vec<Arc<Face>>,
    /// How every font this cache builds rasterizes. See
    /// [`FontCache::set_rendering`].
    rendering: Rendering,
}

impl FontCache {
    /// An empty cache, serving the built-in bitmap face.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw `family` at `weight` with `face` from now on.
    ///
    /// Every font already built at this family and weight is dropped, because
    /// a cached `SystemFont` holds glyphs rasterized from the *previous* face:
    /// keeping them would make the size a UI happened to ask for first decide
    /// which face it gets. The glyphs are rebuilt lazily on the next `get`.
    ///
    /// Installing only a regular face is normal and supported — bold text then
    /// falls back to the built-in bold bitmap face rather than to a synthesised
    /// emboldening of the real one, which is the honest answer until there is a
    /// bold face to use.
    pub fn set_face(&mut self, family: Family, weight: Weight, face: Arc<Face>) {
        self.faces.insert((family, weight), face);
        self.fonts
            .retain(|(_, w, f), _| (*f, *w) != (family, weight));
    }

    /// Parse `data` and install it for `family` at `weight`.
    ///
    /// The convenience form of [`FontCache::set_face`] for a caller that has
    /// just read a file and has no other use for the parsed face.
    ///
    /// # Errors
    ///
    /// Whatever [`Face::parse`] rejects the file with. The cache is left
    /// untouched on failure, so a bad file downgrades to the built-in face
    /// rather than leaving the UI without one.
    pub fn install_face(
        &mut self,
        family: Family,
        weight: Weight,
        data: Vec<u8>,
    ) -> Result<(), SfntError> {
        self.set_face(family, weight, Arc::new(Face::parse(data)?));
        Ok(())
    }

    /// Whether a real face is installed for `family` at `weight`.
    #[must_use]
    pub fn has_face(&self, family: Family, weight: Weight) -> bool {
        self.faces.contains_key(&(family, weight))
    }

    /// Draw whatever an installed face has no glyph for from `faces`, in
    /// order, for every family and weight: face fallback (see
    /// [`SystemFont::with_fallbacks`]). A bold font takes each fallback face
    /// at weight 700 where it has a weight axis.
    ///
    /// Every font already built is dropped, as [`set_face`](Self::set_face)
    /// drops a family's: each holds the fallbacks it was built with.
    ///
    /// Two caches that are to agree -- the toolkit's, which measures, and the
    /// compositor's, which draws -- must be given the same list in the same
    /// order, or a line is measured in one face and drawn in another.
    pub fn set_fallbacks(&mut self, faces: Vec<Arc<Face>>) {
        self.fallbacks = faces;
        self.fonts.clear();
    }

    /// The fallback faces, in order.
    #[must_use]
    pub fn fallbacks(&self) -> &[Arc<Face>] {
        &self.fallbacks
    }

    /// Rasterize every font's glyphs `rendering`'s way -- the appearance
    /// settings' smoothing and subpixel order -- the fonts built already and
    /// those built later alike.
    pub fn set_rendering(&mut self, rendering: Rendering) {
        self.rendering = rendering;
        for font in self.fonts.values_mut() {
            font.set_rendering(rendering);
        }
    }

    /// How this cache's fonts rasterize.
    #[must_use]
    pub fn rendering(&self) -> Rendering {
        self.rendering
    }

    /// The font for `px`, `weight` and `family`, building it on first use.
    ///
    /// A family with no face installed falls back to the built-in bitmap face
    /// rather than to another family's: a terminal that asked for
    /// [`Family::Mono`] and silently got the proportional UI face would draw a
    /// broken grid, which is worse than drawing an unfashionable one.
    ///
    /// The entry is built at the **key's** size, not at `px`. Every request
    /// that rounds to the same key shares one entry, so building from the raw
    /// `px` would let whichever caller happened to arrive first decide the size
    /// for all of them: 13.6 and 14.4 share key 14, and which one won depended
    /// on the order the UI drew in. Worse, it was sticky in the failing
    /// direction — a single request of zero, a negative, or a size the face
    /// cannot scale to made [`SystemFont::from_shared`] fail and installed the
    /// coarse 8x16 bitmap fallback under a key that legitimate callers also
    /// use, so a sub-pixel label elsewhere silently measured sixteen times too
    /// tall for the rest of the process. `round_px` clamps to `1..=MAX_PX`, which
    /// every face can scale to, so keying and building agree and the fallback
    /// is reached only when there is genuinely no usable face.
    pub fn get(&mut self, px: f32, weight: Weight, family: Family) -> &mut SystemFont {
        let face = self.faces.get(&(family, weight)).map(Arc::clone);
        let key = round_px(px);
        let size = key_px(key);
        let fallbacks = &self.fallbacks;
        let rendering = self.rendering;
        self.fonts.entry((key, weight, family)).or_insert_with(|| {
            let axes: &[([u8; 4], f32)] = match weight {
                Weight::Regular => &[],
                Weight::Bold => &[(*b"wght", 700.0)],
            };
            // An installed face that will not scale to this size is a
            // per-size failure, not a reason to stop using the face: fall
            // back for this entry and leave the face installed.
            let mut font = face
                .and_then(|f| SystemFont::from_shared(f, size).ok())
                .map(|font| font.with_fallbacks(fallbacks, axes))
                .unwrap_or_else(|| match weight {
                    Weight::Regular => SystemFont::builtin(size),
                    Weight::Bold => SystemFont::builtin_bold(size),
                });
            font.set_rendering(rendering);
            font
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

/// The largest pixel size [`FontCache::get`] builds a font at. A request for
/// more draws at this size.
///
/// Named because it is a promise other code relies on: a size that crossed a
/// process boundary can be anything, and a caller — or a test — asking what an
/// absurd one will draw needs to know the answer is exactly this, not merely
/// "something smaller".
pub const MAX_PX: u16 = 512;

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
    let rounded = px.clamp(1.0, f32::from(MAX_PX)).round() as u32;
    rounded
}

/// The pixel size a cache entry with key `key` is built at.
///
/// The inverse of [`round_px`], and the reason [`FontCache::get`] is
/// order-independent: the key and the size it is built at are the same number,
/// so two callers whose requests round together get the same font whichever
/// asks first.
fn key_px(key: u32) -> f32 {
    // `round_px` clamps to `1..=MAX_PX`; the `min` restates that bound here so
    // the cast stays exact even if the clamp above ever moves.
    #[allow(clippy::cast_precision_loss)]
    let px = key.min(u32::from(MAX_PX)) as f32;
    px
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
        lcd: None,
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
    use crate::sfnt::tests::{build_test_font, build_test_font_at};

    /// The fixture face at 1000 px per em -- one font unit to the pixel
    /// whatever its units per em, as the advances below assume -- with the
    /// Greek fixture and a colour fixture at U+2764 as its fallbacks.
    fn with_fallbacks() -> SystemFont {
        let greek = Arc::new(Face::parse(build_test_font_at(0x03B1, false)).unwrap());
        let hebrew = Arc::new(Face::parse(build_test_font_at(0x05D0, false)).unwrap());
        let colour = Arc::new(Face::parse(build_test_font_at(0x2764, true)).unwrap());
        let face = Arc::new(Face::parse(build_test_font()).unwrap());
        let px = f32::from(face.units_per_em());
        SystemFont::from_shared(face, px)
            .unwrap()
            .with_fallbacks(&[greek, hebrew, colour], &[])
    }

    fn keys(run: &ShapedRun) -> Vec<(u8, u16)> {
        run.glyphs()
            .iter()
            .map(|g| (g.key.face(), g.key.gid()))
            .collect()
    }

    #[test]
    fn a_character_its_face_lacks_is_drawn_from_the_first_fallback_that_has_it() {
        let font = with_fallbacks();
        assert_eq!(font.fallback_count(), 3);
        // A and B from the font's own face; alpha and gamma from the Greek
        // one, which is fallback 1, face number 1.
        let run = font.shape("A\u{03B1}B\u{03B3}");
        assert_eq!(keys(&run), [(0, 1), (1, 1), (0, 2), (1, 3)]);
        let clusters: Vec<usize> = run.glyphs().iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, [0, 1, 3, 4]);
        // The fixture's glyphs advance 300, 400 and 400 units whichever
        // face they are in.
        assert_eq!(run.width(), 300.0 + 300.0 + 400.0 + 400.0);
        assert_eq!(font.measure("A\u{03B1}B\u{03B3}"), run.width());
        // A character no face has is the font's own missing-glyph box.
        assert_eq!(keys(&font.shape("\u{4E00}")), [(0, 0)]);
    }

    #[test]
    fn a_line_its_own_face_draws_whole_shapes_as_it_would_without_fallbacks() {
        let fallback = with_fallbacks();
        let face = Arc::new(Face::parse(build_test_font()).unwrap());
        let px = f32::from(face.units_per_em());
        let alone = SystemFont::from_shared(face, px).unwrap();
        assert_eq!(fallback.shape("ABCA"), alone.shape("ABCA"));
    }

    #[test]
    fn a_fallback_glyph_is_rasterized_from_its_own_face() {
        let mut font = with_fallbacks();
        let run = font.shape("A\u{03B2}");
        let [a, beta] = [run.glyphs()[0].key, run.glyphs()[1].key];
        // Glyph 1 and glyph 2 of the fixture are a square and a triangle; the
        // beta is glyph 2 of the Greek face, so it must come out as the
        // triangle and not as whatever glyph 2 of the font's own face is --
        // which here is the same triangle, so compare against the Greek face's
        // own rasterization instead of against a shape.
        let mut greek = SystemFont::from_shared(
            Arc::new(Face::parse(build_test_font_at(0x03B1, false)).unwrap()),
            f32::from(Face::parse(build_test_font()).unwrap().units_per_em()),
        )
        .unwrap();
        let greek_beta = greek.shape("\u{03B2}").glyphs()[0].key;
        let expected = greek.glyph_mask(greek_beta).cloned();
        assert!(font.glyph_mask(a).is_some());
        assert_eq!(font.glyph_mask(beta).cloned(), expected);
        // A key naming a face the font does not have is no glyph at all.
        assert!(font.glyph_mask(GlyphKey::in_face(9, 1)).is_none());
    }

    #[test]
    fn a_right_to_left_stretch_from_a_fallback_is_ordered_with_the_whole_line() {
        let font = with_fallbacks();
        // Alef bet A, in the Hebrew fixture (fallback 2) and the font's own:
        // the first strong character is right-to-left, so the paragraph is,
        // and the Latin letter sits at level 2 inside it. Drawn left to
        // right: A, then bet, then alef.
        let run = font.shape("\u{05D0}\u{05D1}A");
        assert_eq!(keys(&run), [(2, 1), (2, 2), (0, 1)]);
        let drawn: Vec<usize> = run.draw_order().map(|g| g.cluster).collect();
        assert_eq!(drawn, [4, 2, 0]);
        // Shaped alone, the Hebrew stretch would have been its own paragraph;
        // in the line, what decides is still the first strong character. A
        // Latin line with one Hebrew letter keeps its order.
        let run = font.shape("A\u{05D0}B");
        let drawn: Vec<usize> = run.draw_order().map(|g| g.cluster).collect();
        assert_eq!(drawn, [0, 1, 3]);
    }

    #[test]
    fn the_emoji_form_is_drawn_from_the_colour_face_and_the_text_form_is_not() {
        // U+2764 in a text face and in a colour face, so that which one draws
        // it is the presentation's choice and not a matter of coverage.
        let text_heart = Arc::new(Face::parse(build_test_font_at(0x2764, false)).unwrap());
        let colour_heart = Arc::new(Face::parse(build_test_font_at(0x2764, true)).unwrap());
        let face = Arc::new(Face::parse(build_test_font()).unwrap());
        let px = f32::from(face.units_per_em());
        let font = SystemFont::from_shared(face, px)
            .unwrap()
            .with_fallbacks(&[colour_heart, text_heart], &[]);
        // No selector: U+2764 is text by default, so the text face (fallback
        // 2) draws it though the colour face comes first in the list.
        assert_eq!(keys(&font.shape("\u{2764}"))[0].0, 2);
        // With U+FE0F, the colour face. The selector itself is no glyph a
        // face must have, and keeps to its heart's face.
        let emoji = font.shape("\u{2764}\u{FE0F}");
        assert_eq!(keys(&emoji)[0].0, 1);
        assert!(emoji.glyphs().iter().all(|g| g.key.face() == 1));
    }

    #[test]
    fn a_cache_builds_its_fonts_with_its_fallbacks() {
        let mut cache = FontCache::new();
        let face = Arc::new(Face::parse(build_test_font()).unwrap());
        cache.set_face(Family::Ui, Weight::Regular, face);
        assert_eq!(
            cache
                .get(16.0, Weight::Regular, Family::Ui)
                .fallback_count(),
            0
        );
        let greek = Arc::new(Face::parse(build_test_font_at(0x03B1, false)).unwrap());
        cache.set_fallbacks(alloc::vec![greek]);
        assert_eq!(cache.fallbacks().len(), 1);
        // Built again, with the fallback.
        assert_eq!(
            cache
                .get(16.0, Weight::Regular, Family::Ui)
                .fallback_count(),
            1
        );
        // The bitmap face -- nothing installed for Mono -- takes none.
        assert_eq!(
            cache
                .get(16.0, Weight::Regular, Family::Mono)
                .fallback_count(),
            0
        );
    }

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

    /// What a glyph costs *before* anything is drawn.
    ///
    /// The compositor's 4K frame attributes ~1.29 us to each of ~9 400 glyphs,
    /// and `compositor::bench_glyph_blit_phases` measured the blit at 5.8 ns a
    /// covered pixel -- about 0.23 us for a glyph of forty covered pixels. So
    /// four fifths of a glyph's cost happens somewhere other than the loop
    /// that puts it on the screen, and this is the first candidate: the cache
    /// lookup that hands the blit its mask.
    ///
    /// Warm on purpose. A cold lookup rasterises, and rasterising once per
    /// glyph per session is not what 9 400 glyphs a frame are paying for.
    ///
    /// ```text
    /// cargo test -p osfont --target x86_64-pc-windows-gnu --release     ///   -- --ignored --nocapture bench_warm_glyph_mask_lookup
    /// ```
    #[test]
    #[ignore = "measurement benchmark; run explicitly with --release --ignored --nocapture"]
    fn bench_warm_glyph_mask_lookup() {
        const REPS: usize = 200_000;
        const ROUNDS: usize = 7;

        let mut font = SystemFont::builtin(14.0);
        // A realistic alphabet, shaped once so the keys are the real ones.
        let run = font.shape("The quick brown fox jumps over the lazy dog, 0123456789.");
        let keys: Vec<_> = run.draw_order().map(|g| g.key).collect();
        assert!(!keys.is_empty(), "nothing shaped, so nothing is measured");

        // Warm every entry, so this measures the hit and not the rasteriser.
        for &k in &keys {
            let _ = font.glyph_mask(k);
        }

        let mut best = f64::MAX;
        for _ in 0..ROUNDS {
            let t = std::time::Instant::now();
            let mut covered = 0usize;
            for i in 0..REPS {
                let k = keys[i % keys.len()];
                if let Some(mask) = font.glyph_mask(k) {
                    // Read one byte so the lookup cannot be optimised away.
                    covered += mask.coverage.first().map_or(0, |&c| c as usize);
                }
            }
            assert!(covered < usize::MAX, "the loop must not be elided");
            best = best.min(t.elapsed().as_nanos() as f64 / REPS as f64);
        }

        println!(
            "warm glyph_mask lookup: {best:.1} ns ({} distinct glyphs)",
            keys.len()
        );
        println!(
            "  a 4K frame's ~9 400 glyphs would spend {:.2} ms here",
            best * 9_400.0 / 1e6
        );
        assert!(
            best > 0.0,
            "a lookup that takes no measurable time did not happen"
        );
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
        cache.get(16.0, Weight::Regular, Family::Ui);
        cache.get(16.4, Weight::Regular, Family::Ui); // rounds to the same key
        assert_eq!(cache.len(), 1, "16.0 and 16.4 must share a font");
        cache.get(16.0, Weight::Bold, Family::Ui);
        cache.get(32.0, Weight::Regular, Family::Ui);
        assert_eq!(cache.len(), 3, "size and weight each key the cache");
        cache.get(16.0, Weight::Regular, Family::Mono);
        assert_eq!(cache.len(), 4, "the family keys the cache too");
    }

    #[test]
    fn two_requests_that_share_a_key_get_the_same_font_whichever_asks_first() {
        // The entry is built at the key's size, so the order the UI happens to
        // draw in cannot change anybody's metrics. Building from the raw `px`
        // instead made 13.6 and 14.4 -- one cache key -- measure differently
        // depending on which label was painted first.
        for pair in [(13.6_f32, 14.4_f32), (7.5, 8.4), (100.4, 99.5)] {
            let mut first = FontCache::new();
            first
                .install_face(Family::Ui, Weight::Regular, build_test_font())
                .unwrap();
            let mut second = FontCache::new();
            second
                .install_face(Family::Ui, Weight::Regular, build_test_font())
                .unwrap();
            first.get(pair.0, Weight::Regular, Family::Ui);
            second.get(pair.1, Weight::Regular, Family::Ui);
            let a = first.get(pair.1, Weight::Regular, Family::Ui).measure("Wq");
            let b = second
                .get(pair.0, Weight::Regular, Family::Ui)
                .measure("Wq");
            assert_eq!(first.len(), 1, "{pair:?} did not share one entry");
            assert!(
                (a - b).abs() < f32::EPSILON,
                "{pair:?} measured {a} or {b} depending on which was asked for first"
            );
        }
    }

    #[test]
    fn a_size_no_face_can_scale_to_does_not_poison_the_key_it_rounds_to() {
        // Zero, a negative and a NaN all round into a key that real callers
        // use: 0.0 and 1.4 both key on 1. Building from the raw size made
        // `from_shared` fail for the bad one and installed the coarse 8x16
        // bitmap under that key, so every later sub-pixel label measured
        // sixteen times too tall -- for the rest of the process.
        for bad in [0.0_f32, -3.0, f32::NAN, f32::INFINITY] {
            let mut cache = FontCache::new();
            cache
                .install_face(Family::Ui, Weight::Regular, build_test_font())
                .unwrap();
            cache.get(bad, Weight::Regular, Family::Ui);
            let good = if bad.is_finite() && bad > 0.0 {
                1.4
            } else {
                16.2
            };
            assert!(
                cache.get(good, Weight::Regular, Family::Ui).is_scalable(),
                "a request of {bad} left {good} on the bitmap fallback"
            );
        }
    }

    #[test]
    fn a_family_with_no_face_falls_back_to_the_builtin_not_to_the_other_family() {
        // The rule a terminal depends on. Serving the proportional UI face to
        // a `Mono` request would draw a grid whose columns do not line up,
        // which is worse than serving the plain bitmap face: that one at least
        // really is fixed-pitch.
        let mut cache = FontCache::new();
        cache
            .install_face(Family::Ui, Weight::Regular, build_test_font())
            .unwrap();
        assert!(cache.get(16.0, Weight::Regular, Family::Ui).is_scalable());
        assert!(
            !cache.get(16.0, Weight::Regular, Family::Mono).is_scalable(),
            "a Mono request took the Ui family's face"
        );
    }

    #[test]
    fn every_glyph_of_the_fallback_mono_face_has_one_advance() {
        // What `Family::Mono` promises: a caller may take one character's
        // advance as the cell width. If that is false the block cursor lands
        // beside the character it marks.
        let mut cache = FontCache::new();
        let font = cache.get(14.0, Weight::Regular, Family::Mono);
        let cell = font.measure("0");
        assert!(cell > 0.0);
        for ch in ['W', 'i', '#', '\u{e9}', 'M', '.'] {
            let mut buf = [0u8; 4];
            let w = font.measure(ch.encode_utf8(&mut buf));
            assert_eq!(w, cell, "{ch:?} advances {w}, not the cell's {cell}");
        }
    }

    #[test]
    fn installing_one_family_does_not_drop_the_other_ones_glyphs() {
        // `set_face` throws away the fonts built from the face it replaces.
        // Throwing away the *other* family's too would re-rasterize the
        // alphabet every time a second family is installed at startup.
        let mut cache = FontCache::new();
        cache.get(16.0, Weight::Regular, Family::Ui);
        cache.get(16.0, Weight::Regular, Family::Mono);
        assert_eq!(cache.len(), 2);
        cache
            .install_face(Family::Mono, Weight::Regular, build_test_font())
            .unwrap();
        assert_eq!(
            cache.len(),
            1,
            "only the Mono entry should have been dropped"
        );
        assert!(
            cache.get(16.0, Weight::Regular, Family::Mono).is_scalable(),
            "the newly installed Mono face was not picked up"
        );
    }

    #[test]
    fn an_installed_face_is_what_the_cache_serves() {
        // The gap this closes: before `set_face` existed, `get` could only ever
        // build the built-in bitmap font, so the entire outline pipeline was
        // unreachable from the compositor and the toolkit — the two callers
        // that hold a `FontCache`.
        let mut cache = FontCache::new();
        assert!(!cache.has_face(Family::Ui, Weight::Regular));
        assert!(
            !cache.get(16.0, Weight::Regular, Family::Ui).is_scalable(),
            "an empty cache must serve the built-in face"
        );

        cache
            .install_face(Family::Ui, Weight::Regular, build_test_font())
            .unwrap();
        assert!(cache.has_face(Family::Ui, Weight::Regular));
        assert!(
            cache.get(16.0, Weight::Regular, Family::Ui).is_scalable(),
            "installing a face must replace the font already built at that size"
        );
        assert!(
            !cache.get(16.0, Weight::Bold, Family::Ui).is_scalable(),
            "a weight with no face installed must still fall back"
        );
    }

    #[test]
    fn every_size_of_one_family_shares_a_single_parsed_face() {
        // A `Face` owns the whole font file. Parsing one per size would hold a
        // copy of a megabyte-scale file per size and re-parse the tables to get
        // it, which is what `Arc` in the cache is for.
        let mut cache = FontCache::new();
        cache
            .install_face(Family::Ui, Weight::Regular, build_test_font())
            .unwrap();
        let mut faces = Vec::new();
        for px in [11.0, 16.0, 24.0, 48.0] {
            let scaled = cache
                .get(px, Weight::Regular, Family::Ui)
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
                .install_face(Family::Ui, Weight::Regular, alloc::vec![0u8; 32])
                .is_err()
        );
        assert!(!cache.has_face(Family::Ui, Weight::Regular));
        assert!(cache.get(16.0, Weight::Regular, Family::Ui).measure("x") > 0.0);
    }

    #[test]
    fn a_hostile_size_still_yields_a_readable_font() {
        // A size may have crossed a process boundary, so it is not necessarily
        // a number at all -- and a 1e30-pixel face would be a memory bomb.
        let mut cache = FontCache::new();
        for px in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 1e30] {
            let font = cache.get(px, Weight::Regular, Family::Ui);
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
            let font = cache.get(px, weight, Family::Ui);
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

    #[test]
    fn a_colour_glyph_is_drawn_in_its_own_colours_at_the_texts_opacity() {
        let face = Arc::new(Face::parse(crate::colr::tests::colour_fixture()).unwrap());
        let mut font = SystemFont::from_shared(face, 100.0).unwrap();
        let (w, h) = (40_u32, 30_u32);
        let draw = |font: &mut SystemFont, text: &str, color: u32| {
            let mut buf = alloc::vec![0xFFFF_FFFF_u32; (w * h) as usize];
            let mut target = Target {
                buffer: &mut buf,
                stride: w,
                height: h,
                color,
            };
            font.draw_text(text, &mut target, 0.0, 20.0);
            buf
        };
        // `A` is a red square from x 10 to 20, and from the baseline at 20 up
        // to 10: red in black text, and not black anywhere.
        let buf = draw(&mut font, "A", 0xFF00_0000);
        assert_eq!(buf[(15 * w + 15) as usize], 0xFFFF_0000);
        assert!(!buf.contains(&0xFF00_0000));
        // In half-transparent text, half-transparent over the white.
        let buf = draw(&mut font, "A", 0x8000_0000);
        assert_eq!(buf[(15 * w + 15) as usize], 0xFFFF_7F7F);
        // `B` paints in the text colour, whichever it is.
        let buf = draw(&mut font, "B", 0xFF00_00FF);
        assert_eq!(buf[(18 * w + 5) as usize], 0xFF00_00FF);
        let buf = draw(&mut font, "B", 0xFF00_FF00);
        assert_eq!(buf[(18 * w + 5) as usize], 0xFF00_FF00);
        // `C` has no colour recipe: a mask, in the text colour.
        let c = font.shape("C").glyphs()[0].key;
        assert!(font.glyph_image(c, 0xFF00_0000).is_none());
        assert!(font.glyph_mask(c).is_some());
    }

    #[test]
    fn a_colour_glyph_from_a_fallback_face_is_drawn_from_that_face() {
        // The Greek fixture first, with `COLR` second: `A` is in both, so the
        // font's own face draws it -- in outline -- and a key naming the
        // colour face gets the colour face's picture.
        let face = Arc::new(Face::parse(build_test_font_at(0x03B1, false)).unwrap());
        let colour = Arc::new(Face::parse(crate::colr::tests::colour_fixture()).unwrap());
        let mut font = SystemFont::from_shared(face, 100.0)
            .unwrap()
            .with_fallbacks(&[colour], &[]);
        let own = font.shape("\u{03B1}").glyphs()[0].key;
        assert_eq!(own.face(), 0);
        assert!(font.glyph_image(own, 0xFF00_0000).is_none());
        let a = font.shape("A").glyphs()[0].key;
        assert_eq!((a.face(), a.gid()), (1, 1));
        let image = font.glyph_image(a, 0xFF00_0000).unwrap();
        assert!(image.pixels.iter().all(|&p| p == 0xFFFF_0000));
        // The bitmap face has no colour glyphs at all.
        let mut builtin = SystemFont::builtin(16.0);
        let key = builtin.shape("A").glyphs()[0].key;
        assert!(builtin.glyph_image(key, 0xFF00_0000).is_none());
    }

    #[test]
    fn a_rendering_mode_reaches_every_font_and_changes_the_pixels() {
        use crate::raster::{Rendering, Subpixel};
        let lcd = Rendering {
            smoothing: true,
            subpixel: Subpixel::Rgb,
            hinting: false,
        };
        let mut cache = FontCache::new();
        let face = Arc::new(Face::parse(build_test_font()).unwrap());
        cache.set_face(Family::Ui, Weight::Regular, face);
        let greek = Arc::new(Face::parse(build_test_font_at(0x03B1, false)).unwrap());
        cache.set_fallbacks(alloc::vec![greek]);
        // A font built before the change, and one after.
        let before = cache
            .get(40.0, Weight::Regular, Family::Ui)
            .shape("A")
            .glyphs()[0]
            .key;
        cache.set_rendering(lcd);
        assert_eq!(cache.rendering(), lcd);
        let font = cache.get(40.0, Weight::Regular, Family::Ui);
        assert!(font.glyph_mask(before).unwrap().lcd.is_some());
        // The fallback face too.
        let alpha = font.shape("\u{03B1}").glyphs()[0].key;
        assert_eq!(alpha.face(), 1);
        assert!(font.glyph_mask(alpha).unwrap().lcd.is_some());
        let later = cache.get(24.0, Weight::Regular, Family::Ui);
        let key = later.shape("B").glyphs()[0].key;
        assert!(later.glyph_mask(key).unwrap().lcd.is_some());
        // Back to grey: the cached LCD masks are dropped, not reused.
        cache.set_rendering(Rendering::default());
        let font = cache.get(40.0, Weight::Regular, Family::Ui);
        assert!(font.glyph_mask(before).unwrap().lcd.is_none());
    }

    #[test]
    fn subpixel_text_fringes_its_edges_in_colour_and_grey_text_does_not() {
        use crate::raster::{Rendering, Subpixel};
        // Black text on white; the fixture's `A` is a square, whose vertical
        // edges are where subpixel rendering shows. At 100 px to the em its
        // left edge is at x 10.3 -- a third of the way into a pixel.
        let draw = |rendering: Rendering| {
            let face = Arc::new(Face::parse(build_test_font()).unwrap());
            let mut font = SystemFont::from_shared(face, 100.0).unwrap();
            font.set_rendering(rendering);
            let (w, h) = (40_u32, 30_u32);
            let mut buf = alloc::vec![0xFFFF_FFFF_u32; (w * h) as usize];
            let mut target = Target {
                buffer: &mut buf,
                stride: w,
                height: h,
                color: 0xFF00_0000,
            };
            font.draw_text("A", &mut target, 0.3, 20.0);
            buf
        };
        let coloured = |buf: &[u32]| {
            buf.iter()
                .filter(|&&p| {
                    let [_, r, g, b] = p.to_be_bytes();
                    r != g || g != b
                })
                .count()
        };
        let lcd = draw(Rendering {
            smoothing: true,
            subpixel: Subpixel::Rgb,
            hinting: false,
        });
        assert!(coloured(&lcd) > 0, "no colour fringe at all");
        let grey = draw(Rendering::default());
        assert_eq!(coloured(&grey), 0);
        // Inside the square both are black.
        assert_eq!(lcd[(15 * 40 + 15) as usize], 0xFF00_0000);
        assert_eq!(grey[(15 * 40 + 15) as usize], 0xFF00_0000);
    }
}
