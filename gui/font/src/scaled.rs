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
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::FontMetrics;
use crate::bidi::{self, Base, Level};
use crate::device::Ppem;
use crate::fallback::{self, Extents};
use crate::gpos::{Adjust, Run};
use crate::gsub::SubGlyph;
use crate::hangul;
use crate::indic::Char;
use crate::indic_shape::{Script, continues_word};
use crate::joining::{self, Form};
use crate::lang::Lang;
use crate::norm;
use crate::norm::{Ignorable, Piece};
use crate::raster::{GlyphMask, rasterize};
use crate::script::{self, ScriptTags};
use crate::sfnt::{Face, PathCmd, SfntError};
use crate::shape::{GlyphKey, ShapedGlyph, ShapedRun, TAB_WIDTH_IN_SPACES};
use crate::thai;

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

impl core::error::Error for ScaledFontError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Sfnt(e) => Some(e),
            Self::InvalidSize => None,
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
    /// Shared because a UI wants the same face at several sizes at once — a
    /// label, a title, a tooltip — and a `Face` owns the whole font file.
    /// Holding it by value meant a megabyte of `Vec<u8>` per size, and
    /// re-parsing the tables each time to get it.
    face: Arc<Face>,
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

/// One stretch of glyphs that shapes as a unit, in post-substitution glyph
/// indices.
///
/// The output of the substitution pass and the input to the positioning one.
/// A segment never spans a tab or a script change, and never contains one: a
/// layout table's lookups may not reach across either, so the two passes cut
/// the run the same way and the cut is made once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment {
    /// Index of the segment's first glyph.
    start: usize,
    /// One past its last.
    end: usize,
    /// The script the stretch was opened under, which chooses its features.
    script: Option<ScriptTags>,
}

/// What the measuring fallback is to make of one glyph.
///
/// Not the same question as "is this glyph a combining mark", and the
/// difference is the point. The fallback runs on some runs and not others — a
/// face that files its Myanmar features under `latn` is shaped by the default
/// shaper, which places marks by measurement, while a face that files them
/// under `mym2` is shaped by the Myanmar shaper, which leaves them to `GPOS`
/// — so a mark in the second is [`Role::Base`] here. That reads oddly and is
/// exactly right: the only thing this pass ever does with a `Base` is refuse
/// to move it and let it end the cluster before it, which is what "nothing
/// for this pass to do" has to look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// A glyph the marks after it attach to, and the end of the run of marks
    /// before it.
    Base,
    /// A combining mark this pass owns, carrying its combining class.
    ///
    /// Zero is a class like any other here. HarfBuzz neither moves nor zeroes
    /// a class-zero mark — Unicode gives it that class precisely because it
    /// needs no reordering and stacks with nothing — but it does keep it
    /// *inside* the cluster, counting its advance towards the marks that
    /// follow. Calling it a base instead restarts the measurement halfway
    /// through a syllable, which is how a Myanmar dot-below ends up a letter
    /// to the right of where it belongs.
    Mark(u8),
}

impl ScaledFont {
    /// Pin `face` to `px_per_em` pixels per em.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::InvalidSize`] if `px_per_em` is not finite and
    /// positive.
    pub fn new(face: Face, px_per_em: f32) -> Result<Self, ScaledFontError> {
        Self::shared(Arc::new(face), px_per_em)
    }

    /// Pin an already-parsed, shared `face` to `px_per_em` pixels per em.
    ///
    /// This is the constructor a font cache wants: parsing a face is the
    /// expensive part and its result is immutable, so several sizes of the same
    /// family should share one. Only the rasterized glyphs differ per size, and
    /// those are this type's own.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::InvalidSize`] if `px_per_em` is not finite and
    /// positive.
    pub fn shared(face: Arc<Face>, px_per_em: f32) -> Result<Self, ScaledFontError> {
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

    /// The face these glyphs come from, for sharing with another size.
    #[must_use]
    pub fn shared_face(&self) -> Arc<Face> {
        Arc::clone(&self.face)
    }

    /// The face's design grid, in units per em.
    ///
    /// Only interesting to a caller comparing this crate's output with another
    /// shaper's: pin a face to this many pixels and the scale factor is one,
    /// so shaped advances come out in the font's own units, which is what
    /// every other shaper reports. `examples/shape_dump.rs` does exactly that.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.face.units_per_em()
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
            // A curve's control points are an upper bound on the curve, never
            // points on it, so only the endpoint counts — which is why every
            // arm below looks at the endpoint and nothing else.
            match *cmd {
                PathCmd::MoveTo(p)
                | PathCmd::LineTo(p)
                | PathCmd::QuadTo(_, p)
                | PathCmd::CurveTo(_, _, p) => note(p.y),
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

    /// The coverage bitmap for one glyph of a [`ShapedRun`] this font
    /// produced.
    ///
    /// `None` when the glyph cannot be decoded, which is not something a
    /// caller can act on — a run is drawn glyph by glyph and one that will not
    /// rasterize is skipped, the pen still advancing so the rest of the line
    /// stays where it was measured to be.
    ///
    /// This is the only way to get from a [`GlyphKey`] to pixels, and it is
    /// deliberately the only way: the key is opaque so that a caller cannot
    /// start treating an outline run and a bitmap run differently. See
    /// [`SystemFont::glyph_mask`](crate::system::SystemFont::glyph_mask) for
    /// the backend-agnostic form.
    pub fn glyph_mask(&mut self, key: GlyphKey) -> Option<&GlyphMask> {
        Some(&self.glyph(key.gid()).ok()?.mask)
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

    /// The gap to add between two glyphs, in pixels, on top of the first
    /// one's advance. Negative for the pairs that need pulling together.
    ///
    /// Separate from `glyph` because a caller that draws one glyph at a time
    /// (the compositor does, so that it can blend coverage through its own
    /// clip stack) has to be able to ask about a pair without giving up the
    /// per-glyph loop. Callers that use `measure` or `draw_text` get it
    /// applied for them.
    #[must_use]
    pub fn kern(&self, left: u16, right: u16) -> f32 {
        self.kern_across(left, right, &[])
    }

    /// The same, for a pair with `between` standing between them — the marks a
    /// face's "ignore marks" kerning is meant to be read across.
    ///
    /// Read at this font's pixel size, so that a pair whose `GPOS` record
    /// carries a device table is kerned the way it will be drawn rather than
    /// the way the design units alone would say.
    #[must_use]
    pub fn kern_across(&self, left: u16, right: u16, between: &[u16]) -> f32 {
        f32::from(self.face.kern_across_at(left, right, between, self.ppem())) * self.scale
    }

    /// The size device tables are read at for this font. See
    /// [`device`](crate::device).
    fn ppem(&self) -> Ppem {
        self.face.ppem(self.px_per_em)
    }

    /// The same, read from the legacy `kern` table alone.
    ///
    /// What the shaper charges a pair in a run whose script reaches no `GPOS`
    /// `kern` feature. Not public: a caller with no run behind it has no script
    /// to decide with, and [`kern_across`](Self::kern_across) is the answer for
    /// that caller.
    fn legacy_kern_across(&self, left: u16, right: u16, between: &[u16]) -> f32 {
        f32::from(self.face.legacy_kern_across(left, right, between)) * self.scale
    }

    /// Font units to pixels.
    ///
    /// The cast is exact for anything a layout table can produce: font units
    /// are bounded by the em square times a few, and a whole run's accumulated
    /// advance by the length of the run — both orders of magnitude inside the
    /// range where `f32` counts integers one at a time.
    #[must_use]
    fn px(&self, units: i32) -> f32 {
        units as f32 * self.scale
    }

    /// The glyphs `text` turns into, with final advances.
    ///
    /// Everything that walks text goes through here — measuring, drawing,
    /// hit-testing, truncating — so that they cannot come to different
    /// answers about the same string. See [`shape`](crate::shape) for why
    /// that is not a theoretical concern.
    ///
    /// Does not rasterize anything and does not touch the glyph cache: this
    /// needs `cmap`, `hmtx` and the layout tables only, which is what lets a
    /// widget measure its label without paying to draw it.
    ///
    /// Names no language, so every face answers with each script's default
    /// rules. [`shape_lang`](Self::shape_lang) is the same call for a caller
    /// that knows one, and [`shape_with`](Self::shape_with) for one that also
    /// knows which way its container runs.
    #[must_use]
    pub fn shape(&self, text: &str) -> ShapedRun {
        self.shape_lang(text, None)
    }

    /// The glyphs `text` turns into when it is known to be in `lang`.
    ///
    /// The same as [`shape`](Self::shape) in every respect but which of a
    /// face's rules apply: a font may spell one language differently from the
    /// rest of the writing system it shares, and this is how it is told which
    /// one it is looking at. Turkish suppresses the `fi` ligature, because the
    /// dotless `ı` makes the dot meaningful; Serbian Cyrillic italics draw `б`,
    /// `г`, `д`, `п` and `т` with different strokes from Russian's; Romanian
    /// wants a comma below `ș` where the Unicode chart shows a cedilla. On this
    /// crate's development host, 230 of 581 installed faces carry at least one
    /// such rule.
    ///
    /// `lang` is a [`Lang`], which is a BCP 47 tag that survived
    /// [`Lang::new`] — `tr`, `sr-Cyrl`, `ro-MD`. `None` selects each script's
    /// default rules, and so does any language a face has said nothing special
    /// about, which is the great majority. Pass `None` rather than a guess: a
    /// *wrong* language is worse than none, since shaping English as Turkish
    /// throws away ligatures the reader expects.
    ///
    /// The language never changes which script a run is shaped as, nor which
    /// characters there are; see [`lang`](crate::lang).
    ///
    /// Resolves the bidi base direction from the text itself. A caller that
    /// knows its container's direction should use
    /// [`shape_with`](Self::shape_with) instead.
    #[must_use]
    pub fn shape_lang(&self, text: &str, lang: Option<Lang>) -> ShapedRun {
        self.shape_with(text, lang, Base::Auto)
    }

    /// The full form: `text` in `lang`, laid out in a container that runs
    /// `base`.
    ///
    /// The other two entry points are this one with defaults —
    /// [`shape`](Self::shape) is `(text, None, Base::Auto)` and
    /// [`shape_lang`](Self::shape_lang) is `(text, lang, Base::Auto)` — so a
    /// caller only reaches for this when it has something to say that they
    /// cannot express.
    ///
    /// # What the base direction decides
    ///
    /// [`Base::Auto`] is UAX #9 rule P2: the first strong character wins. That
    /// is right for anything whose direction is a property of what it *says* —
    /// a message in a chat window, a line in a text editor — and wrong for
    /// anything whose direction is a property of where it *is*. Two cases,
    /// both of which `Auto` gets wrong and neither of which is exotic:
    ///
    /// * **A string with no strong character at all.** `"(123)"` has none, so
    ///   P2 falls back to left-to-right and the parentheses are drawn as
    ///   typed. In a Hebrew paragraph they should be mirrored — the character
    ///   means "the bracket that opens", and in right-to-left text the bracket
    ///   that opens is the one shaped `)`. `Base::Rtl` is what says so.
    /// * **A string whose first strong character runs against its
    ///   container.** An `"OK"` button in a Hebrew interface, or a Hebrew file
    ///   name in a left-to-right path bar: P2 reads the text and gets the
    ///   label's own direction, not the layout's, so the surrounding
    ///   punctuation and any trailing neutrals lay out the wrong way.
    ///
    /// The direction never changes which glyphs a face returns, only their
    /// levels — so it changes the drawn order, the mirroring, and where a
    /// caret goes, and nothing else. Passing `Base::Rtl` also costs the
    /// left-to-right fast path: a string of plain Latin resolves to level 2
    /// inside a level-1 paragraph rather than to nothing at all, which is
    /// correct and is not free.
    #[must_use]
    pub fn shape_with(&self, text: &str, lang: Option<Lang>, base: Base) -> ShapedRun {
        // Six passes, because each one needs all of the previous one's
        // output. Bidi settles which characters are mirrored and where the
        // direction boundaries are, and it reads the string as typed;
        // normalization settles *which characters there are* and so must
        // finish before any of them is looked up in `cmap`; `GSUB` decides
        // which glyphs there are, and cannot run while characters are still
        // arriving; kerning applies to the glyphs that *survive* substitution,
        // so `fi` must be kerned as the single glyph it became, not as the `f`
        // and `i` it was; reordering needs the finished glyphs; and a mark's
        // placement is measured from a pen that both kerning and reordering
        // are still moving.
        let space = self.glyph_id(' ');
        // A level per byte of `text`, indexed by the byte offset a character
        // starts at — which is what a glyph's cluster is, whatever
        // substitution did to the glyph count. Empty for text that needs no
        // bidi at all, which is every left-to-right string.
        let levels = byte_levels(text, base);
        let mut pieces = norm::pieces(text, |ch| self.face.glyph_index(ch).is_some());
        // Korean, which `norm::pieces` deliberately left spelled as the text
        // spelled it. Which spelling to draw is a question about the face —
        // whether it ships the 11,172 precomposed syllables, the conjoining
        // jamo, or both — so it is answered here, with the `cmap` in hand, and
        // not by a normalization pass that can only see the text.
        //
        // Before `piece_levels` below and not after: this rewrites `pieces`,
        // and that vector is one level per piece by index.
        //
        // `zero_width` is asked only about tone marks, and is the narrower
        // question than `has_glyph`: a face that draws a tone mark with no
        // advance is declaring that it overstrikes, and a mark that overstrikes
        // must not be moved to the front of its syllable.
        let mut jamo: Vec<Option<hangul::Jamo>> = Vec::new();
        hangul::preprocess(
            &mut pieces,
            &mut jamo,
            |ch| self.face.glyph_index(ch).is_some(),
            |ch| {
                self.face
                    .glyph_index(ch)
                    .is_some_and(|gid| self.face.advance(gid).is_ok_and(|adv| adv == 0))
            },
        );
        // Unicode variation sequences: two characters that name one glyph, and
        // often a glyph the ordinary `cmap` cannot reach at all — `mmrtext.ttf`
        // draws U+1000 U+FE00 with one no single character maps to. Collapsed
        // to one piece here, which is what HarfBuzz's normalizer does with
        // `handle_variation_selector_cluster`, and before every pass below that
        // would otherwise count the selector as a character of its own: the
        // level list, the run splitter and the joining forms are all one entry
        // per piece.
        //
        // After the Korean pass rather than before, because that one rewrites
        // `pieces` wholesale and `uvs` would not survive it. It takes no
        // interest in a variation selector, so the order costs nothing.
        //
        // Empty, and not even allocated, for every face with no `cmap` format
        // 14 subtable — which is nearly all of them.
        let mut uvs: Vec<Option<u16>> = Vec::new();
        if self.face.has_variation_sequences() {
            collapse_variation_sequences(&mut pieces, &mut jamo, &mut uvs, |base, selector| {
                self.face.variation_glyph(base, selector)
            });
        }
        // A level per *piece*, for the run splitter, and rule L4 while we are
        // here: a bracket in a right-to-left run is drawn as its pair, because
        // the character encodes the bracket that *opens* and which side that
        // is depends on which way the text runs.
        let piece_levels: Vec<Level> = if levels.is_empty() {
            Vec::new()
        } else {
            let out: Vec<Level> = pieces
                .iter()
                .map(|&(_, at)| levels.get(at).copied().unwrap_or(0))
                .collect();
            for (piece, level) in pieces.iter_mut().zip(out.iter()) {
                if !level.is_multiple_of(2)
                    && let Some(m) = bidi::mirror(piece.0)
                    && self.face.glyph_index(m).is_some()
                {
                    piece.0 = m;
                }
            }
            out
        };
        // Which cursive form each character takes, decided from the characters
        // rather than the glyphs because it is a property of the *text*: what
        // a letter joins to does not depend on which face is drawing it. Empty
        // for text that does not join, which is nearly all of it.
        let mut forms: Vec<Option<Form>> = Vec::new();
        joining::forms(&pieces, &mut forms);
        // Split now, while glyphs are still one per piece, so that a run
        // boundary counted in pieces is a boundary counted in glyphs. That
        // stops being true the moment anything ligates. Both users need it
        // before that happens: substitution picks its features per run, and
        // the fallback asks each run whether its script is one whose marks it
        // is allowed to place.
        let runs = script::runs(&pieces, &piece_levels);
        // Thai on a face that predates OpenType: the shifted tone marks are
        // extra glyphs in the private use area rather than `GSUB` rules, and
        // picking them is the engine's job. Empty — and not even allocated —
        // for every face that registers `thai`, which is every face designed
        // this century. See [`thai::pua_shape`].
        //
        // A replacement here changes which glyph is looked up and nothing
        // else: `pieces` still holds the real characters, so a shifted mai ek
        // is still a mark to everything downstream.
        //
        // The gate is HarfBuzz's `!plan->map.found_script[0]`: the face's
        // `GSUB` did not name `thai`, whether because it named `DFLT`/`latn`
        // instead or because there is no `GSUB` at all. Deliberately not
        // `shapes_as_default`, which answers `false` for a face with no
        // `GSUB` — and a face with no `GSUB` is precisely the legacy Thai
        // font this pass exists for.
        let mut pua: Vec<Option<char>> = Vec::new();
        let legacy = |t| thai::legacy_run(t) && self.face.gsub_chosen_script(t) != Some(*b"thai");
        if runs.iter().any(|&(_, t)| legacy(t)) {
            pua = alloc::vec![None; pieces.len()];
            let mut at = 0usize;
            for &(end, tags) in &runs {
                if legacy(tags) {
                    thai::pua_shape(&pieces, at..end, &mut pua, |ch| {
                        self.face.glyph_index(ch).is_some()
                    });
                }
                at = end;
            }
        }
        let mut glyphs: Vec<SubGlyph> = Vec::with_capacity(pieces.len());
        let mut tabs: Vec<bool> = Vec::with_capacity(pieces.len());
        // The run the piece loop is inside, and the three things the fallback
        // asks about its script: whether the face's `GPOS` applies to this run
        // at all, whether a mark here may be *placed* by measurement, and
        // whether a mark here takes no room. Three questions and not one — the
        // first is about the face as well as the script, and ten scripts answer
        // the last two differently, see [`fallback::zeroes_mark_advances`].
        // Walked forward with the loop rather than searched, since all three
        // are in piece order.
        // Whether this face leaves it to the shaper to say which glyphs are
        // marks. A face with a `GDEF` `GlyphClassDef` has stated it, and a
        // glyph the table omits is one it declined to call a mark; a face
        // without one has stated nothing, and the general category of the
        // *character* is the only answer available. HarfBuzz's
        // `fallback_glyph_classes`, and — this is the part that was missing —
        // it has nothing to do with whether `GPOS` applies, because zeroing a
        // mark's advance happens twice on two different grounds:
        // `zero_mark_widths_by_gdef` runs on every face and reads the glyph's
        // class, while the measuring fallback's own `zero_mark_advances` runs
        // only when the fallback does and reads the character's general
        // category. `DejaVuMathTeXGyre.ttf` on Thai needs the first: it has a
        // `GPOS` so no fallback runs, and no `GDEF` at all so the classes are
        // synthesized, and HarfBuzz zeroes two `Mn` characters it cannot draw
        // where we charged them a full missing-glyph box each.
        let by_category = !self.face.classifies_glyphs();
        let mut run = 0usize;
        // Which shaper the run reaches, which the last three all read: a
        // complex script in a face that files its features under `DFLT` or
        // `latn` is shaped by the default shaper, and that shaper places marks
        // by measurement, zeroes their advances, and does no reordering. See
        // [`Face::shapes_as_default`](crate::sfnt::Face::shapes_as_default).
        let mut simple = runs
            .first()
            .is_some_and(|&(_, t)| self.face.shapes_as_default(t));
        let mut synth = runs.first().is_none_or(|&(_, t)| !self.applies_gpos(t));
        let mut placeable = runs
            .first()
            .is_none_or(|&(_, t)| fallback::positions_marks(t, simple));
        // And whether this run is one the Indic shaper will lay out, which
        // decides whether the two facts it reads off the *character* are worth
        // deriving. Neither is free — one is a binary search of the Indic
        // table, the other of the bidi table — and neither is read anywhere
        // else, so a line of Latin pays for neither.
        // Khmer and Myanmar count here too, and not through `Script::shaping`:
        // they read the same [`Char`] out of the same table — the three shapers
        // share one category enum — but each has a shaper and a script tag of
        // its own. See [`indic`](crate::indic).
        let categorised = |t: Option<ScriptTags>| {
            Script::shaping(t).is_some() || crate::khmer::shapes(t) || crate::myanmar::shapes(t)
        };
        let mut indic = !simple && runs.first().is_some_and(|&(_, t)| categorised(t));
        // The same question for the Universal Shaping Engine, asked separately
        // because it reads a category table of its own: the USE categories are
        // a different, larger set than the Indic ones, and no script is shaped
        // by both engines. A run that reaches neither pays for neither.
        let mut use_run = !simple
            && runs
                .first()
                .is_some_and(|&(_, t)| crate::universal::shapes(t));
        for (i, &(ch, cluster)) in pieces.iter().enumerate() {
            while runs.get(run).is_some_and(|&(end, _)| end <= i) {
                run = run.saturating_add(1);
                simple = runs
                    .get(run)
                    .is_some_and(|&(_, t)| self.face.shapes_as_default(t));
                synth = runs.get(run).is_none_or(|&(_, t)| !self.applies_gpos(t));
                placeable = runs
                    .get(run)
                    .is_none_or(|&(_, t)| fallback::positions_marks(t, simple));
                indic = !simple && runs.get(run).is_some_and(|&(_, t)| categorised(t));
                use_run = !simple
                    && runs
                        .get(run)
                        .is_some_and(|&(_, t)| crate::universal::shapes(t));
            }
            // A tab has no glyph. Drawn through `cmap` it comes out as the
            // missing-glyph box, one space wide; the width every caller wants
            // is several spaces of nothing. Substituting the space glyph gets
            // both — it draws blank, and its advance is the unit to multiply.
            let tab = ch == '\t';
            let gid = if tab {
                space
            } else if let Some(gid) = uvs.get(i).copied().flatten() {
                // A variation sequence named this glyph outright, so the
                // `cmap` is not asked: the whole point of the sequence is that
                // the single character maps somewhere else.
                gid
            } else {
                self.glyph_id(pua.get(i).copied().flatten().unwrap_or(ch))
            };
            // Whether the measuring fallback owns this run's marks: it is the
            // one placing them, so it is the one that says which glyphs they
            // are. False for the runs `GPOS` reaches, and false for the
            // complex scripts whose shapers decline the fallback outright —
            // Myanmar's does, and its marks are placed by `GPOS` or not at all.
            let owned = synth && placeable && !tab;
            let klass = if owned { fallback::attach_class(ch) } else { 0 };
            glyphs.push(SubGlyph {
                klass,
                // Nothing more than "the character is `Mn`". What is *done*
                // with that is two separate decisions taken further down —
                // whether the advance is zeroed, and whether the glyph is a
                // mark the fallback places rather than a base other marks
                // attach to — and folding either of them in here would make
                // one of the two wrong.
                //
                // Derived only where one of the two will be asked, since it
                // costs a binary search of the general-category table: the
                // fallback owns this run's marks, or the face has no `GDEF`
                // `GlyphClassDef` and so cannot be asked which glyphs are
                // marks. A face with a `GDEF` to class the glyph and a `GPOS`
                // to place it needs neither.
                mark: (owned || (by_category && !tab)) && norm::is_mark(ch),
                // Answered here for the reason the two below are: it is a
                // property of the character, and after substitution there may
                // be no character left to ask. Unlike them it does not survive
                // substitution — see `SubGlyph::ignorable` — but it does have
                // to survive *reordering*, which is why it rides on the glyph
                // rather than in a parallel vector like `tabs`: the Indic and
                // Khmer shapers move glyphs within a syllable, and a joiner
                // moves with them.
                ignorable: if tab {
                    Ignorable::No
                } else {
                    norm::ignorable(ch)
                },
                // What the Indic shaper needs and cannot recover later: both
                // are properties of the character, and by the time it runs
                // there may be no character left to ask — a conjunct is one
                // glyph standing for four of them.
                indic: if indic && !tab {
                    Char::of(ch)
                } else {
                    Char::DEFAULT
                },
                // And what the Universal Shaping Engine needs, for the same
                // reason and on the same terms: its category decides which
                // cluster a character starts or joins, and its mark bit — which
                // is *not* recoverable from the category — decides whether a
                // ZWNJ before it is hidden from the grammar.
                universal: if use_run && !tab {
                    crate::universal::Char::of(ch)
                } else {
                    crate::universal::Char::DEFAULT
                },
                word: indic && !tab && continues_word(ch),
                // A conjoining jamo takes its slot's feature and gives up
                // `calt`; everything else takes its cursive form. The two are
                // exclusive — no character is both — and `jamo` is empty for
                // every run with no Korean in it, so the lookup costs nothing.
                ..if hangul::is_jamo(ch) {
                    SubGlyph::jamo(gid, cluster, jamo.get(i).copied().flatten())
                } else {
                    SubGlyph::cursive(gid, cluster, forms.get(i).copied().flatten())
                }
            });
            tabs.push(tab);
        }

        let segments = self.substitute_runs(&runs, lang, &mut glyphs, &mut tabs);

        // The same question the piece loop asked, re-asked per *glyph*, because
        // the two are no longer the same list: a stretch that ligated is
        // shorter than the pieces it came from, so a piece index cannot be used
        // to look anything up down here. The segments survive that — they are
        // rewritten by the substitution to say where each stretch landed — and
        // they carry the script, which is the only input the answer has.
        //
        // A glyph no segment covers is a tab, which is not a mark and is not
        // positioned by anything, so `false` is both answers at once.
        let mut synth_at: Vec<bool> = alloc::vec![false; glyphs.len()];
        // And, the same way and for the same reason, whether the segment's
        // kerning has to come from the legacy `kern` table. Also a per-segment
        // question, because `GPOS` files its `kern` feature under particular
        // scripts: Leelawadee registers only `thai`, so the Latin half of a
        // mixed line reaches no `GPOS` kerning and wants the legacy table while
        // the Thai half does not. A segment the pass skipped outright reaches
        // no `GPOS` feature of any kind, so it wants the legacy table too.
        let legacy = self.face.has_legacy_kern();
        let mut legacy_at: Vec<bool> = alloc::vec![false; glyphs.len()];
        // And whether a mark in the segment takes no room. A per-script
        // question and nothing to do with the face: HarfBuzz gates its whole
        // `GDEF` zeroing pass on `plan->zero_marks`, which eleven scripts turn
        // off because their "marks" are spacing letters that a zero advance
        // would pile on top of each other. See [`fallback::Zeroing`].
        let mut zeroed_at: Vec<bool> = alloc::vec![false; glyphs.len()];
        // What the measuring fallback is to make of each glyph, for the
        // segments it owns. `Role::Base` everywhere else, which is exactly
        // what a pass with nothing to do here should see: a run of bases has
        // no clusters in it. Left empty when no segment wants it — a line of
        // Latin in a face with a `GPOS` allocates nothing.
        let synthesize = segments.iter().any(|s| self.places_marks(s.script));
        let mut roles: Vec<Role> = if synthesize {
            alloc::vec![Role::Base; glyphs.len()]
        } else {
            Vec::new()
        };
        for segment in &segments {
            let applies = self.applies_gpos(segment.script);
            let answer = !applies;
            let kern = legacy && (!applies || !self.face.gpos_kerns(segment.script, lang));
            let zero = self.zeroes_marks(segment.script);
            for slot in synth_at
                .get_mut(segment.start..segment.end)
                .unwrap_or_default()
            {
                *slot = answer;
            }
            for slot in legacy_at
                .get_mut(segment.start..segment.end)
                .unwrap_or_default()
            {
                *slot = kern;
            }
            for slot in zeroed_at
                .get_mut(segment.start..segment.end)
                .unwrap_or_default()
            {
                *slot = zero;
            }
            if !self.places_marks(segment.script) {
                continue;
            }
            for at in segment.start..segment.end {
                let Some(glyph) = glyphs.get(at) else { break };
                if glyph.mark && let Some(slot) = roles.get_mut(at) {
                    *slot = Role::Mark(glyph.klass);
                }
            }
        }

        // Which glyphs are combining marks, and what each one's nominal width
        // is. Both are wanted twice — once by the positioning pass, which is
        // handed whole runs, and once by the loop below, which walks one glyph
        // at a time — so they are settled here rather than recomputed.
        //
        // This is the first of HarfBuzz's two mark-zeroing routes, and only
        // the first: `zero_mark_widths_by_gdef` takes the advance from every
        // glyph whose `GDEF` class is mark, on every face and whether or not
        // `GPOS` runs. The measuring fallback's own zeroing is a separate,
        // narrower thing — it reaches only the marks it actually places — and
        // lives in [`synthesize_marks`](Self::synthesize_marks).
        //
        // Which glyphs those are is asked of the *face* when it has a `GDEF`
        // `GlyphClassDef` and of the *character* when it has not, and never of
        // both: a glyph a face with classes omits from them is one it declined
        // to call a mark, and overruling that from the character's general
        // category would zero an advance the designer meant to keep. HarfBuzz
        // synthesizes classes under exactly the same condition —
        // `hb_synthesize_glyph_classes`, run only when
        // `!hb_ot_layout_has_glyph_classes` — which is why the answer here is
        // an either/or and not a union. `SubGlyph::mark` carries the
        // character's half, because substitution is free to change the glyph
        // id and a cluster cannot tell a base from the marks that share it.
        let by_gdef = self.face.classifies_glyphs();
        let marks: Vec<bool> = glyphs
            .iter()
            .enumerate()
            .map(|(i, glyph)| {
                let tab = tabs.get(i).copied().unwrap_or(false);
                zeroed_at.get(i).copied().unwrap_or(false)
                    && if by_gdef {
                        !tab && self.face.is_mark(glyph.gid)
                    } else {
                        glyph.mark
                    }
            })
            .collect();
        let advances: Vec<i32> = glyphs
            .iter()
            .map(|g| i32::from(self.face.advance(g.gid).unwrap_or(0)))
            .collect();
        // `kept_at` says, glyph by glyph, that the positioning pass has already
        // had the last word on this mark's advance: it zeroed the mark *before*
        // the lookups ran and then let one of them — the face's `dist` feature,
        // for Myanmar — charge an advance back on. The loop at the bottom zeroes
        // every mark it is handed, which is right for every other script and
        // would here throw away the half of the job the ordering existed to
        // allow. See [`fallback::Zeroing`].
        //
        // Reported by the pass rather than worked out from the script, because
        // "this segment is Myanmar and the face has a `GPOS`" is not the same
        // claim as "the pass ran on it": a face whose `GPOS` carries nothing
        // this crate reads is positioned by nobody, and its marks still keep
        // their nominal `hmtx` advance unless the loop below takes it away.
        // `DejaVuMathTeXGyre.ttf` is exactly that face, and asking the script
        // instead left every Myanmar mark in it a missing-glyph box wide.
        let (adjusted, kept_at) =
            self.position_segments(&segments, lang, &glyphs, &advances, &marks, &levels);
        // Whether pairs still have to be kerned one at a time here. They do
        // only where the run's kerning is the legacy `kern` table's, which the
        // positioning pass cannot read; pairs the pass has already charged must
        // not be charged again, in the company of every other lookup.
        let legacy_kerning = legacy_at.iter().any(|&yes| yes);
        let mut out: Vec<ShapedGlyph> = Vec::with_capacity(glyphs.len());
        // Where in `out` the left half of the next kerning pair sits, and the
        // glyphs standing between it and the position being filled. A tab is
        // never a left half: its advance is a layout decision, not a glyph
        // width, and a face that kerns after a space would quietly narrow it.
        let mut kern_left: Option<usize> = None;
        let mut between: Vec<u16> = Vec::new();
        for (i, glyph) in glyphs.iter().enumerate() {
            let tab = tabs.get(i).copied().unwrap_or(false);
            let gid = glyph.gid;
            // A combining mark is not part of the spacing, and real faces mark
            // their kerning lookups "ignore marks" so that `A` and `V` still
            // kern with an accent between them. The mark goes into `between`
            // and the face decides from its own lookup flags whether to read
            // across it; kerning *against* the mark instead would shove the
            // accent off the letter it belongs to.
            let mark = marks.get(i).copied().unwrap_or(false);
            // Whether the positioning pass has already had the last word on
            // this mark's advance — see `kept_at`. Only ever true of a mark,
            // and only in a Myanmar segment the pass really ran on.
            let kept = kept_at.get(i).copied().unwrap_or(false);
            // A never-drawn character is transparent to kerning: it is neither
            // half of a pair, and it does not stand between one. Every
            // positioning lookup steps over every default ignorable —
            // HarfBuzz's matcher ignores the joiners and the hidden ones alike
            // in `GPOS` — so a soft hyphen between `A` and `V` must not be
            // allowed to break the pair the way a letter would. Recording it in
            // `between` would do exactly that, since the face's flag says
            // nothing about a character it never expected to see.
            let erased = glyph.ignorable.erased();
            let adjust = adjusted
                .get(i)
                .copied()
                .unwrap_or_else(|| Adjust::plain(advances.get(i).copied().unwrap_or(0)));
            // Kerning is part of the width, not a drawing-time flourish: a
            // measurement that leaves it out is one that disagrees with what
            // the compositor puts on the screen, which is how a label ends up
            // centred half a pixel off in every button on the desktop. It is
            // charged to the pair's *left* glyph — not to whatever was pushed
            // last — so that the advances still sum to the run's width when
            // the pair was read across a mark.
            if legacy_at.get(i).copied().unwrap_or(false)
                && !tab
                && !mark
                && !erased
                && let Some(last) = kern_left.and_then(|at| out.get_mut(at))
            {
                let kern = self.legacy_kern_across(last.key.gid(), gid, &between);
                last.advance += kern;
                last.kern_next = kern;
            }
            let advance = self.px(adjust.x_advance);
            // How far back a zeroed mark has to be moved so that taking its
            // advance away does not also move its image.
            //
            // Zeroing the advance stops the pen travelling, but the mark is
            // drawn at the pen it *arrives* at, which is still the far side of
            // the letter. HarfBuzz's `adjust_mark_offsets` subtracts the
            // advance from the offset for exactly this reason, and it does so
            // for *every* mark it zeroes — no combining class in sight. What
            // makes that safe there is the order: the fallback runs afterwards
            // and overwrites the offset of every mark it places, so the blind
            // shift survives only on the marks nothing else had an answer for.
            // [`synthesize_marks`](Self::synthesize_marks) overwrites in the
            // same way and skips in the same cases — a class of zero, or a
            // glyph the face gives no bounding box — so the shift is applied
            // here unconditionally and left to be overwritten. Gating it on the
            // class instead left a mark with a class but no box unshifted: an
            // accent over `.notdef` in a face with no `GPOS`, which is what
            // `Hack-Bold.ttf` draws for every Myanmar character there is.
            //
            // Left-to-right only, as in HarfBuzz: in a right-to-left run the
            // pen arrives on the mark's *right*, which is where a mark drawn
            // at offset zero already belongs.
            let back = if mark
                && synth_at.get(i).copied().unwrap_or(false)
                && levels
                    .get(glyph.cluster)
                    .is_none_or(|l| l.is_multiple_of(2))
            {
                advance
            } else {
                0.0
            };
            out.push(ShapedGlyph {
                key: GlyphKey::outline(gid),
                // Substitution carried this along: a ligature reports its
                // first component's byte offset, so a caret or a truncation
                // can land before or after it but never inside it — there is
                // no boundary there to find.
                cluster: glyph.cluster,
                // A combining mark takes no room, whatever `hmtx` says. Many
                // faces give U+0301 a real advance — Segoe UI's is over half
                // an `e` — because the same outline doubles as the spacing
                // acute; honouring it would put a gap after every accented
                // letter and make `é` measure wider than `e`. HarfBuzz zeroes
                // mark advances for the same reason. The positioning pass has
                // already done it for the glyphs it saw; this catches the rest.
                // `kept` is the one mark the pass saw and deliberately left
                // with a width — it zeroed first and then let a lookup charge
                // one back on, and taking it away again here would undo the
                // half of the job the ordering existed to allow.
                advance: if mark && !kept {
                    0.0
                } else if tab {
                    advance * TAB_WIDTH_IN_SPACES
                } else {
                    advance
                },
                kern_next: self.px(adjust.kern),
                // Where `GPOS` put the glyph's image relative to the pen: a
                // mark's displacement onto its base, and the odd letter a
                // single adjustment nudges. Zero for everything the pass did
                // not touch, and for every glyph when there is no pass — the
                // fallback fills those in below. `y` points up, which is both
                // `GPOS`'s convention and `ShapedGlyph`'s, so it passes through
                // unflipped; the flip happens once, at the blit.
                offset: (self.px(adjust.x_offset) - back, self.px(adjust.y_offset)),
            });
            if erased {
                // Not in `between` and not a new left half: see above.
            } else if mark {
                // Keep the mark in the run between the pair, but only while
                // there is a pair to read across: a mark with no letter before
                // it starts nothing.
                if kern_left.is_some() {
                    between.push(gid);
                }
            } else {
                between.clear();
                // A tab ends the pair rather than starting one, so the glyph
                // after it kerns against nothing.
                kern_left = (!tab).then(|| out.len().saturating_sub(1));
            }
        }

        // The characters that instruct the shaper and are never drawn, erased
        // now that everything that had to read them has. Last, and it has to be
        // last: a joiner is what makes some faces' ligature fire, and a bidi
        // control is what set the levels, so anything that dropped them earlier
        // would be dropping the instruction along with the mark of it. Before
        // the visual order below, though, because that is a permutation of
        // `out`'s indices and a deletion moves them.
        if glyphs.iter().any(|g| g.ignorable.erased()) {
            hide_ignorables(&mut out, &mut roles, &glyphs, space);
        }

        // Rule L2, over glyphs rather than characters: a ligature is one glyph
        // for several characters and a decomposition several glyphs for one,
        // so by now the run has no one-to-one correspondence left with the
        // string — but every glyph still knows the byte it came from, and so
        // its level.
        //
        // The per-glyph levels outlive the permutation they produce: a caret
        // needs to know which side of a glyph is its *start*, and the
        // permutation cannot say — reversing a one-glyph right-to-left run is
        // the identity. So they are carried into the run alongside the order.
        let per_glyph: Vec<Level> = if levels.is_empty() {
            Vec::new()
        } else {
            out.iter()
                .map(|g| levels.get(g.cluster).copied().unwrap_or(0))
                .collect()
        };
        let visual = if per_glyph.is_empty() {
            Vec::new()
        } else {
            let order = bidi::visual_order(&per_glyph);
            if legacy_kerning {
                recharge_kerns(&mut out, &order);
            }
            order
                .into_iter()
                .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
                .collect()
        };

        if synthesize {
            // The only mark pass left here. A run the positioning pass *did*
            // reach had its marks placed there, in font units and before the
            // reordering — which is where the placement belongs, since a
            // mark's offset is measured against a pen the lookups themselves
            // were still moving. The two never touch the same glyph: a
            // segment the pass ran on is `Role::Base` throughout, because the
            // segment loop only writes `Role::Mark` where `applies_gpos` said
            // no, and a run of bases has no cluster in it for this pass to
            // find.
            self.synthesize_marks(&mut out, &visual, &roles, &levels);
        }
        ShapedRun::reordered(out, visual, per_glyph)
    }

    /// Cut `glyphs` into the stretches that shape together — between tabs and
    /// script changes — substitute each under its own script, and report where
    /// the survivors landed.
    ///
    /// Runs even for a face with no `GSUB`, because the cut is wanted by more
    /// than the substitution: the returned [`Segment`]s are what
    /// [`position_segments`](Self::position_segments) hands to `GPOS`, and a
    /// positioning lookup may no more reach across a tab or a script change
    /// than a substitution one may. `Face::substitute` is a no-op on a face
    /// with nothing to substitute, so the extra call costs a branch.
    ///
    /// Two boundaries, for two reasons.
    ///
    /// A substitution may not reach across a **tab**. The tab is not a glyph
    /// the font knows about, so joining what sits either side of it would
    /// silently swallow the gap it exists to make — and a `GSUB` lookup, which
    /// is handed a whole run and matches anywhere in it, has no way to be told
    /// about a boundary except by not being shown across it.
    ///
    /// A substitution may not reach across a **script change** either, and
    /// here the boundary is not only about reach but about *which rules*: the
    /// features on each side are chosen by different script tags, and a face
    /// that registers both Arabic and Latin has two features called `liga`
    /// that mean different things. Shaping the whole string under one script —
    /// which is what a single call would do, and what HarfBuzz's
    /// `guess_segment_properties` does — applies one writing system's rules to
    /// the other's half of the string.
    ///
    /// `runs` is [`script::runs`]'s output over the pieces these glyphs came
    /// from, so its ends are glyph indices for as long as nothing has ligated
    /// yet — which is why this is the pass that consumes them.
    ///
    /// Both vectors are rewritten, since a run that ligates comes out shorter
    /// and `tabs` has to keep lining up with it — which is the other reason the
    /// segments have to come from here. A caller could not compute them
    /// afterwards: once a stretch has ligated, nothing left in `glyphs` says
    /// where it began.
    fn substitute_runs(
        &self,
        runs: &[(usize, Option<ScriptTags>)],
        lang: Option<Lang>,
        glyphs: &mut Vec<SubGlyph>,
        tabs: &mut Vec<bool>,
    ) -> Vec<Segment> {
        let mut out: Vec<SubGlyph> = Vec::with_capacity(glyphs.len());
        let mut out_tabs: Vec<bool> = Vec::with_capacity(tabs.len());
        let mut segments: Vec<Segment> = Vec::new();
        let mut run: Vec<SubGlyph> = Vec::new();
        /// Shape the open stretch and append it, recording where it landed.
        ///
        /// A nested function rather than a closure because it needs the face
        /// *and* four separate `&mut`s to locals the loop also touches, which
        /// a closure would have to capture and so hold for its whole lifetime.
        fn flush(
            font: &ScaledFont,
            script: Option<ScriptTags>,
            lang: Option<Lang>,
            run: &mut Vec<SubGlyph>,
            out: &mut Vec<SubGlyph>,
            out_tabs: &mut Vec<bool>,
            segments: &mut Vec<Segment>,
        ) {
            if run.is_empty() {
                return;
            }
            font.face.substitute(script, lang, run);
            let start = out.len();
            segments.push(Segment {
                start,
                end: start.saturating_add(run.len()),
                script,
            });
            out_tabs.extend(core::iter::repeat_n(false, run.len()));
            out.append(run);
        }
        // Which script run `i` falls in. Advanced rather than searched: `i`
        // only moves forward, and there are a handful of runs at most.
        let mut at = 0usize;
        // The script the *open* stretch was collected under, which is not the
        // script at `i` once a boundary has been crossed. Keeping it separate
        // is what makes a stretch shaped under the script that opened it.
        let mut open: Option<ScriptTags> = None;
        // One past the end, where there is no glyph and `tabs` reads `true`,
        // so that the last stretch is flushed by the same code as every
        // stretch a tab ends — including the whole of a run with no tabs at
        // all, which is nearly every run.
        for i in 0..=glyphs.len() {
            // Ends are exclusive: `end == i` means the run stopped *before*
            // this glyph, so the stretch closes and `i` opens the next one.
            if runs.get(at).is_some_and(|&(end, _)| end <= i) {
                flush(
                    self,
                    open,
                    lang,
                    &mut run,
                    &mut out,
                    &mut out_tabs,
                    &mut segments,
                );
                while runs.get(at).is_some_and(|&(end, _)| end <= i) {
                    at = at.saturating_add(1);
                }
            }
            open = runs.get(at).and_then(|&(_, script)| script);
            if !tabs.get(i).copied().unwrap_or(true) {
                if let Some(glyph) = glyphs.get(i) {
                    run.push(*glyph);
                }
                continue;
            }
            flush(
                self,
                open,
                lang,
                &mut run,
                &mut out,
                &mut out_tabs,
                &mut segments,
            );
            if let Some(glyph) = glyphs.get(i) {
                out.push(*glyph);
                out_tabs.push(true);
            }
        }
        *glyphs = out;
        *tabs = out_tabs;
        segments
    }

    /// Whether the face's `GPOS` positions a run of `script` — and so, by its
    /// negation, whether that run's marks have to be placed by measurement.
    ///
    /// Two conditions, and the second is the one that makes this a question
    /// about the run rather than about the face. The face must carry a `GPOS`
    /// at all; see [`Face::has_positioning`](crate::sfnt::Face::has_positioning)
    /// for why a face that has one is taken at its word even when it positions
    /// nothing. And the run's script must accept it: Hebrew does not accept a
    /// `GPOS` written for some other script, which is
    /// [`fallback::demands_own_gpos_script`] and the only case there is.
    ///
    /// Asked once per run and once per segment rather than cached, because the
    /// answer is a binary search over a handful of four-byte tags and caching
    /// it would mean deciding where — the face cannot hold it, since it depends
    /// on the run.
    fn applies_gpos(&self, script: Option<ScriptTags>) -> bool {
        self.face.has_positioning()
            && fallback::demands_own_gpos_script(script)
                .is_none_or(|tag| self.face.gpos_names_script(&tag))
    }

    /// Whether a run of `script` in this face zeroes its marks' advances before
    /// the `GPOS` lookups rather than after.
    ///
    /// A question about the run *and* the face, because a face that files its
    /// features under `DFLT` has called the complex shaper off and the default
    /// shaper zeroes last — see [`fallback::shaped_as_default`]. Answered here
    /// rather than at either call site because both of them need it and neither
    /// is in a position to work out `simple` for itself.
    fn zeroes_marks_first(&self, script: Option<ScriptTags>) -> bool {
        let simple = self.face.shapes_as_default(script);
        fallback::zeroes_mark_advances(script, simple) == fallback::Zeroing::BeforeGpos
    }

    /// Whether a run of `script` in this face zeroes its marks' advances at
    /// all — before or after the lookups, either counts.
    ///
    /// HarfBuzz's `plan->zero_marks`, which gates the whole `GDEF` zeroing
    /// pass. Eleven scripts turn it off, because what Unicode calls a mark
    /// there is a spacing letter, and a zero advance would stack a syllable's
    /// worth of them in one cell. See [`fallback::zeroes_mark_advances`].
    fn zeroes_marks(&self, script: Option<ScriptTags>) -> bool {
        let simple = self.face.shapes_as_default(script);
        fallback::zeroes_mark_advances(script, simple) != fallback::Zeroing::Never
    }

    /// Whether the measuring fallback places a run of `script`'s marks.
    ///
    /// Two conditions, and both are needed. `GPOS` must not reach the run —
    /// a face that positions its own marks has said where they go, and
    /// measuring one into a different place would fight the design. And the
    /// run's shaper must be one that asks for the fallback at all: HarfBuzz's
    /// Indic, Khmer, Myanmar and Hangul shapers all end their struct with
    /// `fallback_position = false`, because their marks are `GPOS`'s business
    /// or nobody's. A face that files a complex script's features under
    /// `latn` is shaped by the *default* shaper, which does ask — which is why
    /// this cannot be answered from the script tag alone.
    fn places_marks(&self, script: Option<ScriptTags>) -> bool {
        !self.applies_gpos(script)
            && fallback::positions_marks(script, self.face.shapes_as_default(script))
    }

    /// Position each of `segments` with the face's `GPOS`, into one adjustment
    /// per glyph in `glyphs`.
    ///
    /// Whole segments at a time, because that is the unit a `GPOS` lookup
    /// applies to, exactly as for `GSUB`: each lookup runs across the segment
    /// before the next begins, and a mark's attachment is measured against the
    /// advances the earlier lookups left behind. See [`gpos`](crate::gpos).
    ///
    /// A glyph no segment covers — a tab, and every glyph when the face has no
    /// `GPOS` — keeps its nominal advance and no displacement, which is the
    /// same answer the pass would give for a glyph no lookup matched.
    ///
    /// The second half of the answer is one flag per glyph saying the pass
    /// zeroed that segment's marks *before* its lookups, so that whatever a
    /// lookup then charged back on is the final word and must not be zeroed a
    /// second time. True only where the pass actually ran — see the call site.
    fn position_segments(
        &self,
        segments: &[Segment],
        lang: Option<Lang>,
        glyphs: &[SubGlyph],
        advances: &[i32],
        marks: &[bool],
        levels: &[Level],
    ) -> (Vec<Adjust>, Vec<bool>) {
        let mut out: Vec<Adjust> = advances.iter().copied().map(Adjust::plain).collect();
        let mut kept: Vec<bool> = alloc::vec![false; advances.len()];
        if !self.face.has_gpos_lookups() {
            return (out, kept);
        }
        for segment in segments {
            // A segment whose script refuses this face's `GPOS` outright. Not
            // "no lookup matched" — those two look the same in the output and
            // are not the same claim, and it is this one that switches the
            // measuring fallback on for the segment.
            if !self.applies_gpos(segment.script) {
                continue;
            }
            let span = segment.start..segment.end;
            let (Some(run), Some(widths), Some(is_mark)) = (
                glyphs.get(span.clone()),
                advances.get(span.clone()),
                marks.get(span.clone()),
            ) else {
                continue;
            };
            // Which way the segment reads, taken from its first glyph's level.
            // One level for the whole segment is sound because a script run is
            // already a bidi run: `script::runs` splits on a level change as
            // well as on a script change, which is what makes this the pass
            // that can ask.
            let rtl = run.first().is_some_and(|glyph| {
                levels
                    .get(glyph.cluster)
                    .is_some_and(|level| !level.is_multiple_of(2))
            });
            let first = self.zeroes_marks_first(segment.script);
            let Some(done) = self.face.position(&Run {
                glyphs: run,
                advances: widths,
                marks: is_mark,
                zero_marks_first: first,
                rtl,
                script: segment.script,
                lang,
                ppem: self.ppem(),
            }) else {
                continue;
            };
            for (offset, adjust) in done.into_iter().enumerate() {
                let Some(at) = segment.start.checked_add(offset) else {
                    continue;
                };
                if let Some(slot) = out.get_mut(at) {
                    *slot = adjust;
                }
                // Recorded only for the glyphs the pass reached, and only when
                // it zeroed before its lookups rather than after: those two are
                // exactly the conditions under which its answer is final.
                if let Some(slot) = kept.get_mut(at) {
                    *slot = first;
                }
            }
        }
        (out, kept)
    }

    /// Place every combining mark in `glyphs` by measuring it against the
    /// glyph it follows, for a face that carries no `GPOS` at all.
    ///
    /// The counterpart to what [`gpos`](crate::gpos) does with a face's mark
    /// anchors, and reached on the same terms: offsets measured from the base
    /// glyph's origin with the pen travel taken back off. What differs is where
    /// the numbers come from — two ink boxes and a combining class rather than
    /// a pair of anchors — and when: this runs *after* reordering, because the
    /// ink boxes it measures have to be the ones on the line, whereas `GPOS`
    /// runs before it, in the logical order its lookups are written against.
    /// [`fallback`] holds the geometry and explains why it is HarfBuzz's
    /// geometry and not something invented here.
    ///
    /// `roles` is one [`Role`] per glyph, parallel to `glyphs`; `levels` is
    /// [`byte_levels`]'s output, consulted only for the double-width marks
    /// whose placement depends on which side the next glyph is on.
    ///
    /// # Clusters
    ///
    /// The unit is a *cluster*: a base and the marks that follow it, cut at
    /// the next glyph that is not a mark. This is HarfBuzz's
    /// `position_cluster_impl`, and the two things it does that a simpler rule
    /// would not are both load-bearing.
    ///
    /// A mark whose combining class is zero stays inside the cluster. It is
    /// neither moved nor zeroed — Unicode gives it class zero precisely
    /// because it needs no reordering and no stacking — but it is *not* a new
    /// base either, and treating it as one restarts the measurement halfway
    /// through a syllable. In `ကို့` the two vowel signs are class zero and the
    /// dot below is class seven: taking the second vowel sign for a base put
    /// the dot one letter to the right of where HarfBuzz draws it.
    ///
    /// And a base the face gives no bounding box for ends the cluster early:
    /// there is nothing to measure against, so the marks are zeroed where they
    /// stand and left there. That is HarfBuzz's "if extents don't work, zero
    /// marks and go home", and it zeroes *every* mark in the cluster, class
    /// zero included, because a mark with a width after a letter that has no
    /// visible mark on it is a gap with nothing in it.
    fn synthesize_marks(
        &self,
        glyphs: &mut [ShapedGlyph],
        visual: &[u32],
        roles: &[Role],
        levels: &[Level],
    ) {
        // Nothing in the run is a mark this pass owns, which is the
        // overwhelmingly common case: no clusters to walk and no boxes to read.
        if !roles.iter().any(|role| matches!(role, Role::Mark(_))) {
            return;
        }
        // Every cluster the pass will place, found and zeroed in one walk up
        // front. The zeroing has to happen before the pen positions the
        // placement measures against are added up, because a mark that takes
        // no room must not push the mark after it along the line — which is
        // the same order HarfBuzz works in, where each mark's advance is
        // zeroed as the loop reaches it and the running offset it feeds the
        // *next* mark is therefore already short of it.
        let mut clusters: Vec<(usize, usize, Extents)> = Vec::new();
        let mut at = 0usize;
        while at < roles.len() {
            // A mark with no base before it — a run that opens with a
            // combining character — attaches to nothing, exactly as in
            // HarfBuzz, where the first cluster simply contains no base.
            if matches!(roles.get(at), Some(Role::Mark(_))) {
                at = at.saturating_add(1);
                continue;
            }
            let base = at;
            let mut end = base.saturating_add(1);
            while matches!(roles.get(end), Some(Role::Mark(_))) {
                end = end.saturating_add(1);
            }
            at = end.max(base.saturating_add(1));
            if end <= base.saturating_add(1) {
                continue;
            }
            let Some(glyph) = glyphs.get(base) else { break };
            let gid = glyph.key.gid();
            // Whether the base is in right-to-left text, which decides which
            // edge a double-width mark straddles and — below — whether taking
            // an advance away also has to move the image, since in a
            // right-to-left run the pen arrives on the mark's right, where a
            // mark drawn at offset zero already belongs.
            let rtl = levels
                .get(glyph.cluster)
                .is_some_and(|l| !l.is_multiple_of(2));
            let origin = self.face.glyph_bbox(gid).map(|b| {
                let mut origin =
                    Extents::new(num(b.x_min), num(b.y_min), num(b.x_max), num(b.y_max));
                // Horizontal placement measures against the *cell*, not the
                // ink: a letter with no ink at all still has a width to centre
                // an accent in, and a letter whose ink overhangs its cell
                // (an italic `f`) would otherwise drag the accent out with it.
                origin.x_bearing = 0;
                origin.width = self.face.advance(gid).map_or(0, i32::from);
                origin
            });
            for i in base.saturating_add(1)..end {
                let Some(Role::Mark(k)) = roles.get(i).copied() else {
                    continue;
                };
                // A class-zero mark in a cluster that *has* a base keeps its
                // advance: HarfBuzz does not place it, and so does not zero
                // it — it only counts it into the offset of the marks after
                // it, which is what `pens` below does for free.
                if origin.is_some() && k == 0 {
                    continue;
                }
                let Some(glyph) = glyphs.get_mut(i) else { break };
                // Only on the no-base route does the offset move with the
                // advance. On the other one the mark is about to be placed
                // outright, offset and all, so shifting it first would be
                // undone a line later — and for the one mark placement gives
                // up on, for want of a box of its own, the shift it wants is
                // the whole cluster's travel and not just its own advance.
                if origin.is_none() && !rtl {
                    glyph.offset.0 -= glyph.advance;
                }
                glyph.advance = 0.0;
            }
            if let Some(origin) = origin {
                clusters.push((base, end, origin));
            }
        }
        if clusters.is_empty() {
            return;
        }
        let pens = pens(glyphs, visual);
        // The clearance between a letter and the mark over it, and between one
        // mark and the next. A sixteenth of the em is HarfBuzz's choice, and
        // matching it is the point — see [`fallback`].
        let gap = i32::from(self.face.units_per_em()) / 16;
        for &(base, end, origin) in &clusters {
            let rtl = glyphs
                .get(base)
                .and_then(|glyph| levels.get(glyph.cluster))
                .is_some_and(|l| !l.is_multiple_of(2));
            // The box as the marks placed so far have grown it, which is what
            // makes the second accent of a stack clear the first...
            let mut grown = origin;
            // ...and the class the open stack is of. Marks above and marks
            // below grow the box in opposite directions, so a change of class
            // starts a new stack from the letter rather than from the far side
            // of the previous mark. 255 is not a combining class, so the first
            // mark always starts one.
            let mut open: u8 = 255;
            for i in base.saturating_add(1)..end {
                let Some(Role::Mark(k)) = roles.get(i).copied() else {
                    continue;
                };
                if k == 0 {
                    continue;
                }
                // The offset is from the base's origin; the mark is drawn at
                // its own pen, which is however far the line has moved since.
                // Same subtraction `gpos`'s attachment propagation makes, and
                // right in a right-to-left run for the same reason: both pens
                // are real positions on the line.
                let back =
                    pens.get(base).copied().unwrap_or(0.0) - pens.get(i).copied().unwrap_or(0.0);
                let gid = glyphs.get(i).map_or(0, |glyph| glyph.key.gid());
                let mark = self
                    .face
                    .glyph_bbox(gid)
                    .map(|b| Extents::new(num(b.x_min), num(b.y_min), num(b.x_max), num(b.y_max)));
                let Some(mark) = mark else {
                    // No box to measure, so no placement — but the mark still
                    // travelled with the pen it can no longer pay for, and
                    // HarfBuzz still adds the cluster's running offset to
                    // whatever it had. Without this an accent over a letter
                    // the face draws as a missing-glyph box lands a box to the
                    // right of it.
                    if let Some(glyph) = glyphs.get_mut(i) {
                        glyph.offset.0 += back;
                    }
                    continue;
                };
                if open != k {
                    open = k;
                    grown = origin;
                }
                let (dx, dy) = fallback::place(&mut grown, &mark, k, gap, rtl);
                if let Some(glyph) = glyphs.get_mut(i) {
                    #[allow(clippy::cast_precision_loss)]
                    let (dx, dy) = (dx as f32, dy as f32);
                    glyph.offset = (dx.mul_add(self.scale, back), dy * self.scale);
                }
            }
        }
    }

    /// Width of `text` in pixels, ignoring line breaks.
    #[must_use]
    pub fn measure(&self, text: &str) -> f32 {
        self.shape(text).width()
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
        // Shaped up front, and into a local, because the loop needs `&mut
        // self` to rasterize while it walks the run.
        let run = self.shape(text);
        // Drawing order, not logical order: in a right-to-left run the two
        // differ, and it is this loop's accumulating pen that makes the
        // difference visible.
        let drawn: Vec<ShapedGlyph> = run.draw_order().copied().collect();
        for shaped in &drawn {
            let advance = shaped.advance;
            let Ok(glyph) = self.glyph(shaped.key.gid()) else {
                pen += advance;
                continue;
            };
            // A mask's left/top come from a bitmap bounded by
            // `MAX_GLYPH_PIXELS`, so they are small integers; the pen and
            // baseline are caller-supplied and may be anything, which is why
            // the sum goes through `pixel_coord` (which rejects the
            // degenerate cases) and then `blit_mask` (which clips the rest).
            // `offset` is zero except on an attached combining mark, and its
            // `y` points up where the screen's points down.
            #[allow(clippy::cast_precision_loss)]
            let placed = (
                pixel_coord(pen + shaped.offset.0 + glyph.mask.left as f32),
                pixel_coord(y - shaped.offset.1 + glyph.mask.top as f32),
            );
            if let (Some(gx), Some(gy)) = placed {
                blit_mask(&glyph.mask, target, gx, gy);
            }
            pen += advance;
        }
        pen
    }
}

/// A bidi embedding level per *byte* of `text`, at the offsets characters
/// start at, or empty for text that needs no bidi at all.
///
/// Indexed by byte offset rather than by character because that is what
/// survives the rest of the pipeline: normalization composes and decomposes,
/// substitution ligates, and neither keeps a character count — but every
/// piece and every glyph carries the byte offset it came from, and the level
/// of the character starting there is the level it should be drawn at.
///
/// The whole of it is skipped for a string with no right-to-left character
/// and no directional formatting in it, which is every string this crate is
/// asked to draw on an English desktop. See [`bidi::is_trivially_ltr`].
///
/// That skip is a claim about the *answer*, not about the text — "every level
/// comes out even" — so it holds only while the paragraph's own level is even.
/// Under `Base::Rtl` the same string resolves to level 2 inside a level-1
/// paragraph: an English word in a Hebrew sentence is still drawn left to
/// right, but it sits inside a run that is not, and the neutrals around it
/// take the paragraph's direction rather than the word's. Taking the fast path
/// there would silently ignore the base the caller just asked for, which is
/// the one thing this function must not do.
fn byte_levels(text: &str, base: Base) -> Vec<Level> {
    if base != Base::Rtl && bidi::is_trivially_ltr(text) {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let para = bidi::resolve(&chars, base);
    let mut out: Vec<Level> = vec![0; text.len()];
    for ((at, _), level) in text.char_indices().zip(para.render_levels()) {
        if let Some(slot) = out.get_mut(at) {
            *slot = level;
        }
    }
    out
}

/// Where each glyph's pen sits on the line, indexed by *logical* position.
///
/// Accumulated in *drawing* order, because a pen is a place on the line and
/// reordering moves it. In a right-to-left word a mark is drawn before the
/// letter it sits on, so its pen is the lower of the two and the displacement
/// a caller computes from the pair comes out positive; in a left-to-right word
/// it is the other way round. Neither case is special-cased: the subtraction
/// is the same one, and it is right because both pens are real positions.
///
/// `visual` is [`ShapedRun`]'s permutation, empty when nothing was reordered.
fn pens(glyphs: &[ShapedGlyph], visual: &[u32]) -> Vec<f32> {
    let mut out: Vec<f32> = alloc::vec![0.0; glyphs.len()];
    let mut pen = 0.0f32;
    let mut step = |i: usize| {
        let Some(glyph) = glyphs.get(i) else { return };
        if let Some(slot) = out.get_mut(i) {
            *slot = pen;
        }
        pen += glyph.advance;
    };
    if visual.is_empty() {
        for i in 0..glyphs.len() {
            step(i);
        }
    } else {
        for &v in visual {
            if let Ok(i) = usize::try_from(v) {
                step(i);
            }
        }
    }
    out
}

/// A font-unit measurement as the integer it always was.
///
/// [`BBox`](crate::sfnt::BBox) carries `f32` because an outline's box is
/// computed from `f32` points, but a `glyf` face's stated box is four `i16`s,
/// so nothing is lost on the faces this is used for. A CFF face's box really
/// is fractional; truncating it toward zero is the same rounding `glyf` did in
/// the file.
fn num(v: f32) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        v as i32
    }
}

/// Whether `ch` is a Unicode variation selector.
///
/// The two ranges HarfBuzz's `is_variation_selector` names, and deliberately
/// not the three Mongolian free variation selectors at U+180B: those are the
/// Arabic shaper's business, and no `cmap` keys on them.
fn is_variation_selector(ch: char) -> bool {
    matches!(ch, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// Fold every `base` + variation-selector pair the face recognises into one
/// piece, and record the glyph the face named for it.
///
/// `uvs` comes back the same length as `pieces`, holding the named glyph at
/// each position a pair collapsed to and `None` everywhere else. `jamo` is
/// shortened alongside `pieces` when it is not empty — it is one entry per
/// piece, and a deletion that left it behind would give every Korean glyph
/// after the pair its neighbour's syllable feature.
///
/// A pair the face does not recognise is left as two pieces, which is
/// HarfBuzz's fallback and gives the right answer without doing anything: the
/// selector is a default ignorable, so it is erased after shaping and draws
/// nothing. A selector that follows another selector is not a pair either —
/// HarfBuzz skips past a run of them, and only the first can attach.
fn collapse_variation_sequences(
    pieces: &mut Vec<Piece>,
    jamo: &mut Vec<Option<hangul::Jamo>>,
    uvs: &mut Vec<Option<u16>>,
    named: impl Fn(char, char) -> Option<u16>,
) {
    if !pieces.iter().any(|&(ch, _)| is_variation_selector(ch)) {
        return;
    }
    let korean = jamo.len() == pieces.len();
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len());
    let mut kept: Vec<Option<hangul::Jamo>> = Vec::new();
    uvs.clear();
    uvs.reserve(pieces.len());
    let mut i = 0usize;
    while let Some(&piece) = pieces.get(i) {
        let pair = (!is_variation_selector(piece.0))
            .then(|| pieces.get(i.checked_add(1)?).copied())
            .flatten()
            .filter(|&(next, _)| is_variation_selector(next))
            .and_then(|(next, _)| named(piece.0, next));
        out.push(piece);
        uvs.push(pair);
        if korean {
            kept.push(jamo.get(i).copied().flatten());
        }
        // Two pieces consumed when the face named the pair, one otherwise.
        i = i.saturating_add(if pair.is_some() { 2 } else { 1 });
    }
    *pieces = out;
    if korean {
        *jamo = kept;
    }
}

/// Erase the glyphs still standing for characters that are never drawn.
///
/// HarfBuzz's `hb_ot_hide_default_ignorables`, plus the zeroing that
/// `hb_ot_zero_width_default_ignorables` does just before it, which there are
/// two passes only because one belongs to positioning and one to substitution.
///
/// A joiner, a soft hyphen, a bidi control and a variation selector are
/// instructions, not letters. They must reach the shaper — a joiner deleted
/// early stops the ligature it exists to request — and must not reach the
/// screen. `cmap` will happily give each of them a glyph, and faces disagree
/// wildly about what: blank for ZWJ in most, but a **visible hyphen** for
/// U+00AD in many, which is the case that makes this a correctness bug rather
/// than a tidiness one. A word carrying a discretionary break would render
/// with a hyphen in the middle of it whether or not the line broke there.
///
/// Two dispositions, and which one is not a preference:
///
/// * **Replaced by the space glyph**, zero-width, when the face has one. The
///   glyph stays in the run, so clusters, the visual order and every index
///   into them are undisturbed.
/// * **Deleted**, when the face has no `space` — `glyph_id` answering `0`,
///   which is `.notdef`. Substituting the missing-glyph box would draw a
///   visible tofu for a character whose entire point is to be invisible, so
///   the glyph goes. HarfBuzz makes the same choice on the same test.
///
/// `roles` is deleted from in lockstep, and that is the whole reason it is
/// passed: it is indexed by position in `out` by
/// [`synthesize_marks`](ScaledFont::synthesize_marks), so a deletion here that
/// left it alone would shift every role one glyph to the left and stack the
/// accents on the wrong letters.
///
/// A *replaced* glyph keeps its role, which is deliberate and is HarfBuzz's
/// behaviour too. Some default ignorables are themselves combining marks —
/// U+034F COMBINING GRAPHEME JOINER and the variation selectors are `Mn` —
/// and calling one a base after hiding it would cut the cluster in half at a
/// character that was never meant to be visible in the first place.
fn hide_ignorables(
    out: &mut Vec<ShapedGlyph>,
    roles: &mut Vec<Role>,
    glyphs: &[SubGlyph],
    space: u16,
) {
    if space == 0 {
        let mut i = 0;
        // `retain` visits in order and exactly once, which is what makes the
        // running index line up with `glyphs`; `out` and `glyphs` are still
        // parallel here because the loop that built `out` pushed exactly one
        // glyph per iteration.
        out.retain(|_| {
            let keep = !glyphs.get(i).is_some_and(|g| g.ignorable.erased());
            i = i.saturating_add(1);
            keep
        });
        let mut j = 0;
        roles.retain(|_| {
            let keep = !glyphs.get(j).is_some_and(|g| g.ignorable.erased());
            j = j.saturating_add(1);
            keep
        });
        return;
    }
    for (glyph, sub) in out.iter_mut().zip(glyphs) {
        if !sub.ignorable.erased() {
            continue;
        }
        glyph.key = GlyphKey::outline(space);
        // Advance *and* the kern charged to it, because the kern was added
        // into the advance when it was charged and leaving it would make the
        // recorded pieces stop summing to the width. The x offset goes for the
        // same reason HarfBuzz zeroes it — a zero-advance glyph that is still
        // displaced would drag its blank somewhere — and the y offset stays,
        // also as in HarfBuzz, because nothing is drawn for it to move.
        glyph.advance = 0.0;
        glyph.kern_next = 0.0;
        glyph.offset.0 = 0.0;
    }
}

/// Move each kern onto the glyph that is now to the *left* of the pair.
///
/// Kerning is a correction to the gap between two glyph images. Which of the
/// two carries it in its advance is bookkeeping — but it is bookkeeping that
/// depends on the order they are drawn in, because an advance pushes the pen
/// rightwards regardless of which way the text reads. The shaping pass charges
/// every kern to the pair's logically-first glyph, which is the left one in
/// left-to-right text; after rule L2 has reversed a run, the left one is the
/// logically-*second*, and leaving the kern where it was would put the gap on
/// the far side of the pair — visible as a word whose letters are correctly
/// ordered and incorrectly spaced.
///
/// So: strip every kern, then walk the pairs that are actually adjacent on
/// the line and give each its own kern back. A pair that became adjacent only
/// through reordering — the two glyphs either side of a direction boundary —
/// gets nothing, which is right: they were never kerned as a pair, and
/// HarfBuzz does not kern across a run boundary either.
///
/// This is for the legacy `kern` table only, and the caller gates it on
/// [`kerns_outside_gpos`](crate::sfnt::Face::kerns_outside_gpos). A legacy
/// value is a pure gap with no direction in it, so it genuinely belongs to
/// whichever glyph ends up on the left. A `GPOS` pair is not: a font
/// expressing a right-to-left adjustment writes XPlacement *and* XAdvance
/// into the same value record — the placement opens the gap by moving the
/// ink, the advance keeps the rest of the line where it was — so the font
/// author has already written the correction for the reversal, and the value
/// belongs exactly where the record put it. Moving it again would apply the
/// same correction twice.
fn recharge_kerns(glyphs: &mut [ShapedGlyph], order: &[usize]) {
    let kerns: Vec<f32> = glyphs.iter().map(|g| g.kern_next).collect();
    if kerns.iter().all(|&k| k == 0.0) {
        return;
    }
    for glyph in glyphs.iter_mut() {
        glyph.advance -= glyph.kern_next;
        glyph.kern_next = 0.0;
    }
    for pair in order.windows(2) {
        let (Some(&left), Some(&right)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let kern = if right == left.saturating_add(1) {
            // Still in logical order: the kern is already the left glyph's.
            kerns.get(left).copied().unwrap_or(0.0)
        } else if left == right.saturating_add(1) {
            // Reversed: the pair was charged to what is now the right glyph.
            kerns.get(right).copied().unwrap_or(0.0)
        } else {
            0.0
        };
        if kern != 0.0
            && let Some(glyph) = glyphs.get_mut(left)
        {
            glyph.advance += kern;
            glyph.kern_next = kern;
        }
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

/// Converts a pen position to a whole-pixel coordinate, or `None` when it is
/// not a real number.
///
/// Rust's `as` maps NaN to `0` and saturates infinities, so feeding a
/// degenerate layout straight into `blit_mask` would stamp the text at the
/// origin instead of dropping it — a corrupt or uninitialised position would
/// paint garbage over the top-left of the screen rather than doing nothing
/// visible, which is far harder to diagnose. Infinities are excluded too:
/// they saturate to `i32::MIN`/`MAX`, which clips correctly today only
/// because `blit_mask` adds to them with `saturating_add`.
pub(crate) fn pixel_coord(v: f32) -> Option<i32> {
    if !v.is_finite() {
        return None;
    }
    // Guarded by `is_finite` above, and the range check keeps the cast from
    // saturating, so the truncation is exactly the intended floor-toward-zero.
    #[allow(clippy::cast_possible_truncation)]
    let clamped = (v >= -2_147_483_648.0 && v <= 2_147_483_647.0).then_some(v as i32);
    clamped
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
    use crate::sfnt::tests::{
        build_test_font, build_test_font_with_gdef_classes, build_test_font_with_gpos_scripts,
        build_test_font_with_gsub_and_classes, build_test_font_with_layout,
        build_test_font_with_uvs,
    };

    fn font(px: f32) -> ScaledFont {
        ScaledFont::from_bytes(build_test_font(), px).unwrap()
    }

    /// The fixture has no `space` glyph — its `cmap` is 'A', 'B' and 'C' —
    /// which puts every string here down the *deletion* branch. That is the
    /// branch worth testing against a real face, because it is the one that
    /// changes the glyph count and so the one that can desynchronise
    /// something.
    #[test]
    fn a_face_with_no_space_deletes_the_characters_that_are_never_drawn() {
        let f = font(1000.0);
        let plain: Vec<u16> = f.shape("AB").glyphs().iter().map(|g| g.key.gid()).collect();
        assert_eq!(plain, alloc::vec![1, 2]);
        for text in [
            "A\u{200d}B", // ZWJ
            "A\u{200c}B", // ZWNJ
            "A\u{ad}B",   // SOFT HYPHEN, the one faces draw visibly
            "A\u{fe0f}B", // VARIATION SELECTOR-16
            "A\u{2060}B", // WORD JOINER
            "A\u{034f}B", // COMBINING GRAPHEME JOINER
        ] {
            let run = f.shape(text);
            let gids: Vec<u16> = run.glyphs().iter().map(|g| g.key.gid()).collect();
            assert_eq!(gids, plain, "{text:?} should shape as {plain:?}");
        }
        // The control, and it matters: 'Z' is not in the fixture's `cmap`
        // either, so it is `.notdef` exactly as the ignorables were — and it
        // stays. Without this the test above would pass on a `shape` that
        // simply dropped every unmapped character.
        let gids: Vec<u16> = f
            .shape("AZB")
            .glyphs()
            .iter()
            .map(|g| g.key.gid())
            .collect();
        assert_eq!(gids, alloc::vec![1, 0, 2]);
    }

    /// The other branch, which no fixture here can reach because none has a
    /// space: the glyph stays in the run, so nothing indexed by position
    /// moves, and it is emptied instead — the space glyph, no advance, no
    /// horizontal offset.
    ///
    /// The vertical offset is deliberately *not* zeroed, which is HarfBuzz's
    /// behaviour: with no advance and nothing drawn there is nothing for it to
    /// move, and matching the reference exactly is worth more than tidying it.
    #[test]
    fn a_face_with_a_space_empties_the_glyph_rather_than_removing_it() {
        let ignorable = |yes: Ignorable| {
            let mut g = SubGlyph::new(0, 0);
            g.ignorable = yes;
            g
        };
        let subs = alloc::vec![
            ignorable(Ignorable::No),
            ignorable(Ignorable::Plain),
            ignorable(Ignorable::No)
        ];
        let filled = |gid: u16| ShapedGlyph {
            key: GlyphKey::outline(gid),
            cluster: 0,
            advance: 7.0,
            kern_next: 2.0,
            offset: (3.0, 5.0),
        };
        let mut out = alloc::vec![filled(10), filled(11), filled(12)];
        let mut roles = alloc::vec![Role::Base, Role::Mark(1), Role::Mark(2)];
        hide_ignorables(&mut out, &mut roles, &subs, 3);
        assert_eq!(out.len(), 3, "the run keeps its length");
        assert_eq!(
            roles,
            alloc::vec![Role::Base, Role::Mark(1), Role::Mark(2)],
            "and so does the role list"
        );
        assert_eq!(out[1].key.gid(), 3);
        assert_eq!((out[1].advance, out[1].kern_next), (0.0, 0.0));
        assert_eq!(out[1].offset, (0.0, 5.0));
        // The neighbours are untouched — this is not a pass that zeroes a run.
        assert_eq!(out[0].key.gid(), 10);
        assert_eq!((out[0].advance, out[0].offset), (7.0, (3.0, 5.0)));
        assert_eq!(out[2].key.gid(), 12);
    }

    /// The deletion branch must take the roles with it. They are indexed by
    /// position in the glyph run by `synthesize_marks`, so a deletion that
    /// left them alone would shift every accent one glyph left — silently,
    /// and only on faces that need the mark fallback.
    #[test]
    fn deleting_a_glyph_deletes_its_combining_class_too() {
        let ignorable = |yes: Ignorable| {
            let mut g = SubGlyph::new(0, 0);
            g.ignorable = yes;
            g
        };
        let subs = alloc::vec![
            ignorable(Ignorable::No),
            ignorable(Ignorable::Plain),
            ignorable(Ignorable::No)
        ];
        let filled = |gid: u16| ShapedGlyph {
            key: GlyphKey::outline(gid),
            cluster: 0,
            advance: 1.0,
            kern_next: 0.0,
            offset: (0.0, 0.0),
        };
        let mut out = alloc::vec![filled(10), filled(11), filled(12)];
        let mut roles = alloc::vec![Role::Base, Role::Mark(230), Role::Mark(220)];
        // `space` of 0 is `glyph_id` reporting the face has no space glyph.
        hide_ignorables(&mut out, &mut roles, &subs, 0);
        let gids: Vec<u16> = out.iter().map(|g| g.key.gid()).collect();
        assert_eq!(gids, alloc::vec![10, 12]);
        assert_eq!(roles, alloc::vec![Role::Base, Role::Mark(220)]);
    }

    /// LAMED then QAMATS: one Hebrew letter and one Hebrew point, neither of
    /// which the fixture has a glyph for, so both come out `.notdef` — which is
    /// exactly the case this is about. The point is still a point.
    const POINTED: &str = "\u{5dc}\u{5b8}";

    /// How far below the baseline each glyph is drawn, shaping [`POINTED`] at
    /// the em size against a face that registers `scripts` in its `GPOS`.
    ///
    /// The em size so the numbers are the font's own units: the fixture is a
    /// 1000-unit em, `.notdef` is 600 units wide, and the fallback's clearance
    /// is `upem / 16` = 62.
    ///
    /// The *vertical* offset, and not the advance which this used to read,
    /// because the advance no longer answers the question: a face with no
    /// `GDEF` zeroes a mark's advance whether or not anything placed it — see
    /// `by_category` in `shape`. Nothing but the measuring fallback ever moves
    /// a glyph off the baseline, so a non-zero drop here means it ran.
    fn pointed_drops(scripts: &[[u8; 4]]) -> Vec<f32> {
        let f = ScaledFont::from_bytes(build_test_font_with_gpos_scripts(scripts), 1000.0).unwrap();
        f.shape(POINTED)
            .glyphs()
            .iter()
            .map(|g| g.offset.1)
            .collect()
    }

    /// The bug this fixes: a face carrying a `GPOS` for some other script used
    /// to switch the measuring fallback off for *every* run in it, because the
    /// question was asked of the file rather than of the run. A Hebrew point in
    /// a Latin-and-Arabic face then kept its full nominal advance and sat
    /// beside its letter instead of under it.
    ///
    /// HarfBuzz's rule, and now ours: the Hebrew shaper is the one that sets a
    /// `gpos_tag`, so a Hebrew run refuses a `GPOS` whose ScriptList does not
    /// name `hebr` and is positioned by measurement instead. Measured against
    /// HarfBuzz over the host's 556 faces, this is the difference between 249
    /// and 6 disagreements on the corpus's pointed-Hebrew string.
    #[test]
    fn a_hebrew_run_refuses_a_gpos_written_for_another_script() {
        // No `hebr`: the fallback runs, and the point is measured onto the
        // underside of its letter — one clearance below the baseline, since
        // `.notdef` here draws nothing and its box is empty.
        for scripts in [
            [*b"DFLT", *b"arab", *b"latn"].as_slice(),
            [*b"latn"].as_slice(),
            [*b"cyrl", *b"grek"].as_slice(),
        ] {
            assert_eq!(
                pointed_drops(scripts),
                alloc::vec![0.0, -62.0],
                "a GPOS naming {scripts:?} says nothing about Hebrew"
            );
        }
        // `hebr` present: the face has been written with Hebrew in mind, so it
        // is taken at its word even though this `GPOS` positions nothing, and
        // the point is left on the baseline where `hmtx` put it.
        for scripts in [
            [*b"hebr"].as_slice(),
            [*b"DFLT", *b"hebr", *b"latn"].as_slice(),
        ] {
            assert_eq!(
                pointed_drops(scripts),
                alloc::vec![0.0, 0.0],
                "a GPOS naming {scripts:?} owns its Hebrew"
            );
        }
    }

    /// The other half of the same claim: a face with no `GPOS` at all falls
    /// back for every script, and one whose `GPOS` names the run's own script
    /// falls back for none — so the new gate cannot have been implemented by
    /// simply always falling back, or never.
    #[test]
    fn a_latin_run_takes_whatever_gpos_the_face_offers() {
        // 'A' plus a combining acute, which is an `Mn` mark. The fixture has no
        // glyph for the acute, so it comes out `.notdef` — 600 units wide and
        // drawing nothing.
        let acute = "A\u{301}";
        let shaped = |bytes: Vec<u8>| -> Vec<(f32, f32)> {
            ScaledFont::from_bytes(bytes, 1000.0)
                .unwrap()
                .shape(acute)
                .glyphs()
                .iter()
                .map(|g| (g.advance, g.offset.1))
                .collect::<Vec<_>>()
        };
        let with_gpos = |scripts: &[[u8; 4]]| shaped(build_test_font_with_gpos_scripts(scripts));
        // 'A' is glyph 1, 300 units wide. A `GPOS` under `DFLT` alone still
        // covers a Latin run, so nothing measures the acute onto it and it
        // stays on the baseline — but it is still an `Mn` character in a face
        // with no `GDEF`, so it still takes no room.
        assert_eq!(
            with_gpos(&[*b"DFLT"]),
            alloc::vec![(300.0, 0.0), (0.0, 0.0)]
        );
        assert_eq!(
            with_gpos(&[*b"hebr"]),
            alloc::vec![(300.0, 0.0), (0.0, 0.0)]
        );
        // No `GPOS` at all, and the fallback runs for everything: the acute is
        // lifted a clearance above the top of 'A', whose ink reaches y = 100.
        assert_eq!(
            shaped(build_test_font()),
            alloc::vec![(300.0, 0.0), (0.0, 162.0)]
        );
    }

    /// Both of HarfBuzz's zeroing passes, each shown where the other cannot
    /// reach.
    ///
    /// `A` plus a combining acute the fixture has no glyph for, so the acute is
    /// a 600-unit `.notdef` unless something takes its width away.
    ///
    /// - A face that classifies its glyphs and carries a `GPOS`: no fallback
    ///   runs, and the `GDEF` pass reads the class rather than the character.
    ///   This face calls every glyph it mentions a base, `.notdef` included, so
    ///   the acute keeps its width — a face that has stated which of its glyphs
    ///   are marks is believed.
    /// - The same classes with no `GPOS`: the fallback runs, and *its* zeroing
    ///   reads the general category, so the acute loses its width and is
    ///   measured onto the letter regardless of what `GDEF` called it. This is
    ///   the pass that the `GDEF` one cannot stand in for.
    #[test]
    fn a_face_that_classifies_its_glyphs_decides_which_of_them_take_room() {
        // Glyphs 0..=3, every one of them a base.
        const BASES: &[u16] = &[1, 1, 1, 1];
        let shaped = |bytes: Vec<u8>| -> Vec<(f32, f32)> {
            ScaledFont::from_bytes(bytes, 1000.0)
                .unwrap()
                .shape("A\u{301}")
                .glyphs()
                .iter()
                .map(|g| (g.advance, g.offset.1))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shaped(build_test_font_with_layout(&[*b"DFLT"], BASES)),
            alloc::vec![(300.0, 0.0), (600.0, 0.0)]
        );
        assert_eq!(
            shaped(build_test_font_with_gdef_classes(BASES)),
            alloc::vec![(300.0, 0.0), (0.0, 162.0)]
        );
    }

    /// KA, VOWEL SIGN I, VOWEL SIGN U, DOT BELOW: `ကို့`, one Myanmar syllable
    /// whose three marks are all `Mn` and whose combining classes are 0, 0 and
    /// 7.
    ///
    /// Shaped against a face whose `GSUB` files everything under `DFLT`, which
    /// is the face saying it does no complex shaping — so the run reaches the
    /// *default* shaper, the one that measures marks. A face registering
    /// nothing at all would not do: that one keeps the Myanmar shaper, whose
    /// marks are placed by `GPOS` or not at all. See
    /// [`fallback::shaped_as_default`].
    ///
    /// The fixture has no Myanmar glyphs, so every one of them comes out
    /// `.notdef`: 600 units wide, drawing nothing, and therefore centred on a
    /// 600-unit cell.
    const SYLLABLE: &str = "\u{1000}\u{102d}\u{102f}\u{1037}";

    /// [`SYLLABLE`]'s face: `DFLT` in `GSUB`, plus `classes` in `GDEF`.
    fn syllable_font(classes: &[u16]) -> Vec<u8> {
        build_test_font_with_gsub_and_classes(&[*b"DFLT"], classes)
    }

    /// A mark whose combining class is zero is still part of the cluster.
    ///
    /// This is the rule the pass used to get wrong, and the shape of the bug is
    /// worth keeping: a class-zero mark is not placed and not zeroed, which
    /// made it look exactly like a base, so the measurement restarted at it and
    /// every mark after it was positioned against the wrong glyph. In `ကို့` the
    /// dot below landed two letters to the right of where HarfBuzz draws it.
    ///
    /// HarfBuzz cuts clusters on the *general category* and picks the base as
    /// the first non-mark in one, so a class-zero mark can never be a base
    /// there; the class only decides whether the mark is moved once the base is
    /// known. Both halves are asserted here: the dot's offset is the whole
    /// cluster's travel back to KA, and the two class-zero vowel signs are left
    /// where the pen put them.
    #[test]
    fn a_class_zero_mark_does_not_start_a_new_cluster() {
        // No `GDEF`, so the `GDEF` zeroing pass reads the character's category
        // instead and takes the width off all three marks. The dot is then
        // 600 units — one KA — back from its own pen, and centred on the
        // 600-unit cell, so 300 - 600.
        let f = ScaledFont::from_bytes(syllable_font(&[]), 1000.0).unwrap();
        let shaped: Vec<(f32, f32)> = f
            .shape(SYLLABLE)
            .glyphs()
            .iter()
            .map(|g| (g.advance, g.offset.0))
            .collect();
        assert_eq!(
            shaped,
            alloc::vec![(600.0, 0.0), (0.0, -600.0), (0.0, -600.0), (0.0, -300.0)],
            "the dot is measured against KA, not against the vowel sign before it"
        );
    }

    /// The same syllable in a face that classifies its glyphs and calls none of
    /// them a mark — which is what a CJK face with no Myanmar in it looks like,
    /// and is the case that found the bug in the first place.
    ///
    /// Now the `GDEF` pass zeroes nothing, so the two class-zero vowel signs
    /// keep their full 600-unit advance and the measuring fallback zeroes only
    /// the one mark it actually places. The dot's offset is therefore three
    /// cells back rather than one — which is precisely the number the old code
    /// could not produce, because it measured from the vowel sign beside the
    /// dot and got zero travel.
    #[test]
    fn only_the_marks_the_fallback_places_lose_their_advance() {
        let f = ScaledFont::from_bytes(syllable_font(&[1, 1, 1, 1]), 1000.0).unwrap();
        let shaped: Vec<(f32, f32)> = f
            .shape(SYLLABLE)
            .glyphs()
            .iter()
            .map(|g| (g.advance, g.offset.0))
            .collect();
        assert_eq!(
            shaped,
            alloc::vec![
                (600.0, 0.0),
                (600.0, 0.0),
                (600.0, 0.0),
                (0.0, 300.0 - 1800.0)
            ],
            "a class-zero mark the face does not call a mark keeps its width"
        );
    }

    /// VARIATION SELECTOR-1.
    const VS: char = '\u{FE00}';

    /// Run the collapse over `pieces` with a face that names every pair as
    /// glyph 7, or names none at all.
    fn collapse(pieces: &[Piece], names: bool) -> (Vec<Piece>, Vec<Option<u16>>) {
        let mut pieces = pieces.to_vec();
        let mut jamo: Vec<Option<hangul::Jamo>> = Vec::new();
        let mut uvs: Vec<Option<u16>> = Vec::new();
        collapse_variation_sequences(&mut pieces, &mut jamo, &mut uvs, |_, _| names.then_some(7));
        (pieces, uvs)
    }

    /// A pair the face recognises is one piece from here on, and every pass
    /// after this one counts pieces: the level list, the run splitter, the
    /// joining forms. Leaving it as two would give the selector a level, a
    /// script and a joining form of its own.
    #[test]
    fn a_named_pair_becomes_one_piece_carrying_its_glyph() {
        let (pieces, uvs) = collapse(&[('A', 0), (VS, 1), ('B', 4)], true);
        assert_eq!(pieces, alloc::vec![('A', 0), ('B', 4)]);
        assert_eq!(uvs, alloc::vec![Some(7), None]);
    }

    /// A pair the face does not recognise stays two pieces, which is
    /// HarfBuzz's fallback and needs nothing further: the selector is a
    /// default ignorable, so it is erased after shaping and draws nothing.
    #[test]
    fn an_unnamed_pair_stays_two_pieces() {
        let (pieces, uvs) = collapse(&[('A', 0), (VS, 1), ('B', 4)], false);
        assert_eq!(pieces, alloc::vec![('A', 0), (VS, 1), ('B', 4)]);
        assert_eq!(uvs, alloc::vec![None, None, None]);
    }

    /// Only the first selector of a run can attach, and a selector is never a
    /// base. Both halves matter: a face that named every pair it was shown
    /// would otherwise fold a whole run of selectors into the letter, and each
    /// fold would swallow a piece that the erasure pass has to see.
    #[test]
    fn a_selector_never_pairs_with_the_selector_after_it() {
        let (pieces, uvs) = collapse(&[('A', 0), (VS, 1), (VS, 4), ('B', 7)], true);
        assert_eq!(pieces, alloc::vec![('A', 0), (VS, 4), ('B', 7)]);
        assert_eq!(uvs, alloc::vec![Some(7), None, None]);

        let (pieces, uvs) = collapse(&[(VS, 0), ('A', 3)], true);
        assert_eq!(pieces, alloc::vec![(VS, 0), ('A', 3)], "a leading selector");
        assert_eq!(uvs, alloc::vec![None, None]);
    }

    /// A trailing selector has nothing after it to pair with, and the
    /// look-ahead must not read past the end to discover that.
    #[test]
    fn a_trailing_selector_is_left_alone() {
        let (pieces, uvs) = collapse(&[('A', 0), (VS, 1)], true);
        assert_eq!(pieces, alloc::vec![('A', 0)], "the pair collapsed");
        assert_eq!(uvs, alloc::vec![Some(7)]);
    }

    /// Text with no selector in it is left untouched — including `uvs`, which
    /// stays empty rather than being filled with `None`s. That is the case for
    /// nearly every string, so every reader indexes `uvs` defensively; a test
    /// that let it silently become pieces-length would hide a reader that does
    /// not.
    #[test]
    fn text_with_no_selector_does_not_even_allocate() {
        let (pieces, uvs) = collapse(&[('A', 0), ('B', 1)], true);
        assert_eq!(pieces, alloc::vec![('A', 0), ('B', 1)]);
        assert!(uvs.is_empty());
    }

    /// `jamo` is one entry per piece and is shortened in lockstep. A deletion
    /// that left it behind would give every Korean glyph after the pair its
    /// neighbour's syllable feature — `ljmo` where `vjmo` belongs.
    #[test]
    fn the_korean_syllable_marks_are_shortened_in_lockstep() {
        let pieces = alloc::vec![('\u{1100}', 0), (VS, 3), ('\u{1161}', 6)];
        let mut pieces = pieces;
        let mut jamo = alloc::vec![
            Some(hangul::Jamo::Leading),
            Some(hangul::Jamo::Leading),
            Some(hangul::Jamo::Vowel),
        ];
        let mut uvs: Vec<Option<u16>> = Vec::new();
        collapse_variation_sequences(&mut pieces, &mut jamo, &mut uvs, |_, _| Some(7));
        assert_eq!(pieces.len(), 2);
        assert_eq!(
            jamo,
            alloc::vec![Some(hangul::Jamo::Leading), Some(hangul::Jamo::Vowel)]
        );
    }

    /// End to end: the face names a glyph its ordinary `cmap` has no entry
    /// for, and the shaped run is one glyph rather than a letter and an
    /// invisible selector beside it.
    #[test]
    fn a_recognised_pair_shapes_as_the_one_glyph_the_face_named() {
        let bytes = build_test_font_with_uvs(&[('\u{FE00}' as u32, &[], &[('A' as u32, 2)])]);
        let f = ScaledFont::from_bytes(bytes, 1000.0).unwrap();
        let shaped = f.shape("A\u{FE00}");
        let gids: Vec<u16> = shaped.glyphs().iter().map(|g| g.key.gid()).collect();
        assert_eq!(gids, alloc::vec![2], "one glyph, and the named one");
    }

    /// The same string in the same face with the pair unlisted: two glyphs go
    /// in, and the selector comes out erased rather than drawn, because it is
    /// a default ignorable. The fixture has no `space` glyph, so erased means
    /// deleted — which leaves one glyph again, but the base's own.
    #[test]
    fn an_unrecognised_pair_leaves_the_base_and_erases_the_selector() {
        let bytes = build_test_font_with_uvs(&[('\u{FE01}' as u32, &[], &[('A' as u32, 2)])]);
        let f = ScaledFont::from_bytes(bytes, 1000.0).unwrap();
        let shaped = f.shape("A\u{FE00}");
        let gids: Vec<u16> = shaped.glyphs().iter().map(|g| g.key.gid()).collect();
        assert_eq!(gids, alloc::vec![1], "the base's ordinary glyph");
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

    /// `n` glyphs 10 px wide, with `kern` charged to glyph `at` as the
    /// shaping pass charges it: added to the advance *and* recorded.
    fn kerned(n: usize, at: usize, kern: f32) -> alloc::vec::Vec<ShapedGlyph> {
        (0..n)
            .map(|i| ShapedGlyph {
                key: crate::shape::GlyphKey::outline(u16::try_from(i).unwrap()),
                cluster: i,
                advance: if i == at { 10.0 + kern } else { 10.0 },
                kern_next: if i == at { kern } else { 0.0 },
                offset: (0.0, 0.0),
            })
            .collect()
    }

    #[test]
    fn a_reversal_moves_a_kern_to_the_pair_s_new_left_glyph() {
        // Three glyphs, the pair (0,1) kerned by -2, drawn right to left.
        let mut glyphs = kerned(3, 0, -2.0);
        recharge_kerns(&mut glyphs, &[2, 1, 0]);
        // Glyph 1 is now the left half of the pair, so it holds the kern and
        // glyph 0 — now on the right, kerning against nothing — does not.
        assert!((glyphs[1].kern_next + 2.0).abs() < f32::EPSILON);
        assert!((glyphs[1].advance - 8.0).abs() < f32::EPSILON);
        assert!(glyphs[0].kern_next.abs() < f32::EPSILON);
        assert!((glyphs[0].advance - 10.0).abs() < f32::EPSILON);
        // The line is the same width either way round.
        let width: f32 = glyphs.iter().map(|g| g.advance).sum();
        assert!((width - 28.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_kern_survives_an_order_that_did_not_change() {
        let mut glyphs = kerned(3, 1, -2.0);
        recharge_kerns(&mut glyphs, &[0, 1, 2]);
        assert!((glyphs[1].kern_next + 2.0).abs() < f32::EPSILON);
        assert!((glyphs[1].advance - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_pair_the_reversal_invented_is_not_kerned() {
        // (0,1) were kerned; the order draws 0 beside 2, which never were a
        // pair. Charging them anything would invent a correction the face
        // never asked for.
        let mut glyphs = kerned(3, 0, -2.0);
        recharge_kerns(&mut glyphs, &[2, 0, 1]);
        assert!(glyphs[2].kern_next.abs() < f32::EPSILON);
        // 0 is followed by 1 here, so that pair keeps its kern.
        assert!((glyphs[0].kern_next + 2.0).abs() < f32::EPSILON);
        let width: f32 = glyphs.iter().map(|g| g.advance).sum();
        assert!((width - 28.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_unkerned_run_is_left_exactly_as_it_was() {
        let mut glyphs = kerned(3, 0, 0.0);
        let before = glyphs.clone();
        recharge_kerns(&mut glyphs, &[2, 1, 0]);
        assert_eq!(glyphs, before);
    }

    #[test]
    fn levels_are_not_resolved_for_text_that_cannot_need_them() {
        assert!(byte_levels("The quick brown fox", Base::Auto).is_empty());
        assert!(byte_levels("", Base::Auto).is_empty());
        // An explicit left-to-right base is the same answer as `Auto` here, so
        // it keeps the fast path: the paragraph level is 0 either way.
        assert!(byte_levels("The quick brown fox", Base::Ltr).is_empty());
        // Every byte of a two-byte character gets the level, so a lookup by
        // cluster start cannot miss.
        let levels = byte_levels("a\u{5d0}", Base::Auto);
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], 0);
        assert_eq!(levels[1], 1);
    }

    /// The fast path is about the answer, not the text. Latin in a
    /// right-to-left paragraph still needs resolving: the letters go to level
    /// 2 and the neutrals around them to the paragraph's own 1.
    #[test]
    fn a_right_to_left_base_does_not_take_the_left_to_right_fast_path() {
        let levels = byte_levels("ab", Base::Rtl);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0], 2);
        assert_eq!(levels[1], 2);
        // And the same string under `Auto` resolves to nothing at all, which
        // is what makes this a real difference rather than a spelling of it.
        assert!(byte_levels("ab", Base::Auto).is_empty());
    }

    /// The case the entry that asked for this named: `"(123)"` has no strong
    /// character, so rule P2 cannot help and only the caller knows.
    #[test]
    fn a_string_with_no_strong_character_takes_the_base_it_is_given() {
        // Under `Auto`, P2 finds nothing strong and falls back to
        // left-to-right — every level even, so the fast path is right.
        assert!(byte_levels("(123)", Base::Auto).is_empty());
        // Told otherwise, the brackets are in a right-to-left run and rule L4
        // will mirror them.
        let levels = byte_levels("(123)", Base::Rtl);
        assert_eq!(levels.len(), 5);
        assert!(!levels[0].is_multiple_of(2), "the opening bracket is at {}", levels[0]);
        assert!(!levels[4].is_multiple_of(2), "the closing bracket is at {}", levels[4]);
        // The digits are `EN`, which rule I1 raises to an even level inside an
        // odd-level run — they are drawn left to right inside a right-to-left
        // paragraph, which is how numbers work in Hebrew and Arabic.
        assert!(levels[1].is_multiple_of(2), "the digits are at {}", levels[1]);
    }
}
